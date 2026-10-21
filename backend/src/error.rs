use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

#[derive(Debug)]
pub enum ApiError {
    BadRequest(&'static str),
    Unauthorized,
    NotFound,
    Conflict(&'static str),
    TooManyRequests,
    Unprocessable(&'static str),
    BadGateway(&'static str),
    Internal(String),
}

pub type ApiResult<T> = Result<T, ApiError>;

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        if let Self::Unprocessable(reason) = self {
            let body = json!({ "error": "unavailable", "reason": reason });
            return (StatusCode::UNPROCESSABLE_ENTITY, Json(body)).into_response();
        }
        let (status, code) = match &self {
            Self::BadRequest(c) => (StatusCode::BAD_REQUEST, *c),
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            Self::NotFound => (StatusCode::NOT_FOUND, "not_found"),
            Self::Conflict(c) => (StatusCode::CONFLICT, *c),
            Self::TooManyRequests => (StatusCode::TOO_MANY_REQUESTS, "too_many_attempts"),
            Self::Unprocessable(_) => unreachable!(),
            Self::BadGateway(c) => (StatusCode::BAD_GATEWAY, *c),
            Self::Internal(e) => {
                eprintln!("internal error: {e}");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal")
            }
        };
        (status, Json(json!({ "error": code }))).into_response()
    }
}

impl From<rusqlite::Error> for ApiError {
    fn from(e: rusqlite::Error) -> Self {
        match e {
            rusqlite::Error::QueryReturnedNoRows => Self::NotFound,
            e => Self::Internal(e.to_string()),
        }
    }
}

impl From<std::io::Error> for ApiError {
    fn from(e: std::io::Error) -> Self {
        Self::Internal(e.to_string())
    }
}
