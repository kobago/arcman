use axum::{Json, extract::State, http::StatusCode};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::{
    AppState,
    error::{ApiError, ApiResult},
};

pub const DEFAULT_FXTWITTER_BASE_URL: &str = "https://api.fxtwitter.com";
const FXTWITTER_KEY: &str = "fxtwitter_base_url";
const PIXIV_SESSION_KEY: &str = "pixiv_session";

#[derive(Serialize, Debug, PartialEq)]
pub struct Resolved {
    pub value: String,
    pub source: &'static str,
}

fn resolve(
    conn: &Connection,
    key: &str,
    env: Option<&str>,
    default: &str,
) -> rusqlite::Result<Resolved> {
    let saved: Option<String> = conn
        .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
            r.get(0)
        })
        .optional()?;
    Ok(match (saved, env) {
        (Some(v), _) => Resolved {
            value: v,
            source: "saved",
        },
        (None, Some(v)) => Resolved {
            value: v.to_string(),
            source: "env",
        },
        (None, None) => Resolved {
            value: default.to_string(),
            source: "default",
        },
    })
}

pub fn fxtwitter_base_url(state: &AppState) -> rusqlite::Result<String> {
    let env = state.fxtwitter_base_url_env.as_deref();
    Ok(resolve(&state.db(), FXTWITTER_KEY, env, DEFAULT_FXTWITTER_BASE_URL)?.value)
}

/// The PHPSESSID cookie of a signed-in pixiv account, which R-18 works need.
pub fn pixiv_session(state: &AppState) -> rusqlite::Result<Option<String>> {
    state
        .db()
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            [PIXIV_SESSION_KEY],
            |r| r.get(0),
        )
        .optional()
}

/// The session itself is never sent back, only whether one is saved.
#[derive(Serialize)]
pub struct PixivSessionView {
    saved: bool,
}

#[derive(Serialize)]
pub struct SettingsView {
    fxtwitter_base_url: Resolved,
    pixiv_session: PixivSessionView,
}

pub async fn get(State(state): State<AppState>) -> ApiResult<Json<SettingsView>> {
    let env = state.fxtwitter_base_url_env.as_deref();
    let fxtwitter_base_url = resolve(&state.db(), FXTWITTER_KEY, env, DEFAULT_FXTWITTER_BASE_URL)?;
    Ok(Json(SettingsView {
        fxtwitter_base_url,
        pixiv_session: PixivSessionView {
            saved: pixiv_session(&state)?.is_some(),
        },
    }))
}

#[derive(Deserialize)]
pub struct SettingsInput {
    fxtwitter_base_url: String,
}

pub async fn put(
    State(state): State<AppState>,
    Json(input): Json<SettingsInput>,
) -> ApiResult<StatusCode> {
    let url = input.fxtwitter_base_url.trim().trim_end_matches('/');
    if !(url.starts_with("https://") || url.starts_with("http://"))
        || reqwest::Url::parse(url).is_err()
    {
        return Err(ApiError::BadRequest("invalid_url"));
    }
    state.db().execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        [FXTWITTER_KEY, url],
    )?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct PixivSessionInput {
    session: String,
}

/// Accepts the bare value or `PHPSESSID=...` as copied from the browser. Empty clears it.
pub async fn put_pixiv_session(
    State(state): State<AppState>,
    Json(input): Json<PixivSessionInput>,
) -> ApiResult<StatusCode> {
    let session = input.session.trim();
    let session = session.strip_prefix("PHPSESSID=").unwrap_or(session).trim();
    if session.is_empty() {
        state
            .db()
            .execute("DELETE FROM settings WHERE key = ?1", [PIXIV_SESSION_KEY])?;
        return Ok(StatusCode::NO_CONTENT);
    }
    if !session
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(ApiError::BadRequest("invalid_session"));
    }
    state.db().execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        [PIXIV_SESSION_KEY, session],
    )?;
    Ok(StatusCode::NO_CONTENT)
}
