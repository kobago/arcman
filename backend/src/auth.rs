use std::time::{Duration, Instant};

use argon2::{
    Argon2,
    password_hash::{PasswordHasher, PasswordVerifier, phc::PasswordHash},
};
use axum::{
    Extension, Json,
    extract::{Request, State},
    http::{HeaderMap, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{
    AppState, BoxResult,
    db::NOW,
    error::{ApiError, ApiResult},
};

const COOKIE: &str = "arcman_session";
const SESSION_DAYS: i64 = 30;
const MAX_FAILURES: u32 = 5;
const LOCKOUT: Duration = Duration::from_secs(60);

#[derive(Default)]
pub struct LoginGuard {
    failures: u32,
    locked_until: Option<Instant>,
}

impl LoginGuard {
    fn is_locked(&self, now: Instant) -> bool {
        self.locked_until.is_some_and(|t| now < t)
    }

    fn record(&mut self, ok: bool, now: Instant) {
        if ok {
            *self = Self::default();
            return;
        }
        self.failures += 1;
        if self.failures >= MAX_FAILURES {
            self.failures = 0;
            self.locked_until = Some(now + LOCKOUT);
        }
    }
}

pub fn set_password(conn: &Connection, password: &str) -> BoxResult<()> {
    let hash = Argon2::default()
        .hash_password(password.as_bytes())
        .map_err(|e| e.to_string())?
        .to_string();
    conn.execute(
        "INSERT INTO credentials (id, password_hash) VALUES (1, ?1)
         ON CONFLICT (id) DO UPDATE SET password_hash = excluded.password_hash",
        [hash],
    )?;
    Ok(())
}

fn verify(hash: &str, password: &str) -> bool {
    PasswordHash::new(hash).is_ok_and(|h| {
        Argon2::default()
            .verify_password(password.as_bytes(), &h)
            .is_ok()
    })
}

fn sha256_hex(s: &str) -> String {
    Sha256::digest(s.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn session_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .find_map(|kv| kv.trim().strip_prefix(COOKIE)?.strip_prefix('='))
}

fn cookie(headers: &HeaderMap, value: &str, max_age: i64) -> String {
    let secure = headers
        .get("x-forwarded-proto")
        .is_some_and(|v| v.as_bytes().eq_ignore_ascii_case(b"https"));
    format!(
        "{COOKIE}={value}; HttpOnly; SameSite=Strict; Path=/; Max-Age={max_age}{}",
        if secure { "; Secure" } else { "" }
    )
}

#[derive(Deserialize)]
pub struct Login {
    password: String,
}

pub async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Login>,
) -> ApiResult<Response> {
    if state.login_guard.lock().unwrap().is_locked(Instant::now()) {
        return Err(ApiError::TooManyRequests);
    }
    let hash: Option<String> = state
        .db()
        .query_row(
            "SELECT password_hash FROM credentials WHERE id = 1",
            [],
            |r| r.get(0),
        )
        .optional()?;
    let ok = match hash {
        Some(hash) => tokio::task::spawn_blocking(move || verify(&hash, &body.password))
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?,
        None => false,
    };
    state.login_guard.lock().unwrap().record(ok, Instant::now());
    if !ok {
        return Err(ApiError::Unauthorized);
    }
    let token: String = rand::random::<[u8; 32]>()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    state.db().execute(
        "INSERT INTO sessions (token_hash, expires_at) VALUES (?1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now', ?2))",
        params![sha256_hex(&token), format!("+{SESSION_DAYS} days")],
    )?;
    let set = cookie(&headers, &token, SESSION_DAYS * 86400);
    Ok((StatusCode::NO_CONTENT, [(header::SET_COOKIE, set)]).into_response())
}

#[derive(Clone)]
pub struct SessionHash(String);

pub async fn require_session(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Response {
    let Some(hash) = session_token(req.headers()).map(sha256_hex) else {
        return ApiError::Unauthorized.into_response();
    };
    let valid = state
        .db()
        .query_row(
            &format!("SELECT 1 FROM sessions WHERE token_hash = ?1 AND expires_at > {NOW}"),
            [&hash],
            |_| Ok(()),
        )
        .optional();
    match valid {
        Ok(Some(())) => {
            req.extensions_mut().insert(SessionHash(hash));
            next.run(req).await
        }
        Ok(None) => ApiError::Unauthorized.into_response(),
        Err(e) => ApiError::from(e).into_response(),
    }
}

pub async fn logout(
    State(state): State<AppState>,
    Extension(SessionHash(hash)): Extension<SessionHash>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    state
        .db()
        .execute("DELETE FROM sessions WHERE token_hash = ?1", [hash])?;
    Ok((
        StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, cookie(&headers, "", 0))],
    )
        .into_response())
}

pub async fn logout_all(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Response> {
    state.db().execute("DELETE FROM sessions", [])?;
    Ok((
        StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, cookie(&headers, "", 0))],
    )
        .into_response())
}

pub async fn session() -> StatusCode {
    StatusCode::NO_CONTENT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locks_after_five_consecutive_failures_and_unlocks_after_a_minute() {
        let mut g = LoginGuard::default();
        let t0 = Instant::now();
        for _ in 0..4 {
            g.record(false, t0);
        }
        assert!(!g.is_locked(t0));
        g.record(false, t0);
        assert!(g.is_locked(t0));
        assert!(!g.is_locked(t0 + LOCKOUT));
    }

    #[test]
    fn a_success_resets_the_failure_count() {
        let mut g = LoginGuard::default();
        let t0 = Instant::now();
        for _ in 0..4 {
            g.record(false, t0);
        }
        g.record(true, t0);
        g.record(false, t0);
        assert!(!g.is_locked(t0));
    }

    #[test]
    fn verifies_only_the_password_that_was_set() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE credentials (id INTEGER PRIMARY KEY, password_hash TEXT NOT NULL)",
        )
        .unwrap();
        set_password(&conn, "correct horse").unwrap();
        let hash: String = conn
            .query_row("SELECT password_hash FROM credentials", [], |r| r.get(0))
            .unwrap();
        assert!(hash.starts_with("$argon2id$"));
        assert!(verify(&hash, "correct horse"));
        assert!(!verify(&hash, "wrong"));
    }
}
