//! The proxy's own error replies (§7.3).

use axum::response::{IntoResponse, Response};
use axum::Json;
use http::StatusCode;
use serde_json::{json, Value};
use zygo_config::Dialect;

/// An error the proxy answers itself (§7.3): `type` = `code`. Messages are fixed texts (no header, token or body value),
/// except that a `400 zygo_bad_request` may echo a client field name or type name (K§6.4.3), cut to 64 characters with
/// each control, format, line separator or paragraph separator character (Unicode Cc, Cf, Zl, Zp) replaced by `?`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{code}: {message}")]
pub struct ZygoError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
}

impl ZygoError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> ZygoError {
        ZygoError { status, code, message: message.into() }
    }
    pub fn token_invalid() -> ZygoError {
        ZygoError::new(StatusCode::UNAUTHORIZED, "zygo_token_invalid", "invalid session token")
    }
    pub fn session_unknown() -> ZygoError {
        ZygoError::new(StatusCode::UNAUTHORIZED, "zygo_session_unknown", "unknown session")
    }
    pub fn session_suspended() -> ZygoError {
        ZygoError::new(StatusCode::UNAUTHORIZED, "zygo_session_suspended", "session suspended: no heartbeat for 5 min")
    }
    pub fn bad_request(message: impl Into<String>) -> ZygoError {
        ZygoError::new(StatusCode::BAD_REQUEST, "zygo_bad_request", message)
    }
    /// Anthropic: `{"type":"error","error":{"type":code,"message":m}}`; Openai and OpenaiChat: `{"error":{"type":code,"message":m}}`.
    pub fn body(&self, d: Dialect) -> Value {
        let error = json!({ "type": self.code, "message": self.message });
        match d {
            Dialect::Anthropic => json!({ "type": "error", "error": error }),
            Dialect::Openai | Dialect::OpenaiChat => json!({ "error": error }),
        }
    }
}

impl ZygoError {
    /// The error reply in the client's dialect `d`.
    pub fn response(&self, d: Dialect) -> Response {
        (self.status, Json(self.body(d))).into_response()
    }
}

impl IntoResponse for ZygoError {
    fn into_response(self) -> Response {
        (self.status, Json(self.body(Dialect::Anthropic))).into_response()
    }
}
