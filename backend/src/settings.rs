use axum::{Json, extract::State, http::StatusCode};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::{
    AppState,
    error::{ApiError, ApiResult},
};

pub const DEFAULT_FXTWITTER_BASE_URL: &str = "https://api.fxtwitter.com";
const FXTWITTER_KEY: &str = "fxtwitter_base_url";

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

#[derive(Serialize)]
pub struct SettingsView {
    fxtwitter_base_url: Resolved,
}

pub async fn get(State(state): State<AppState>) -> ApiResult<Json<SettingsView>> {
    let env = state.fxtwitter_base_url_env.as_deref();
    Ok(Json(SettingsView {
        fxtwitter_base_url: resolve(&state.db(), FXTWITTER_KEY, env, DEFAULT_FXTWITTER_BASE_URL)?,
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
