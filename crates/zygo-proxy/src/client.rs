//! Session API client for the CLI, hooks and status line (§10.2.4, §17, §17.1).

use std::fmt;
use std::path::Path;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use zygo_core::session_dir::SessionDir;

use crate::Url;
pub use crate::session::{SessionSettings, WindowArg};

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("session directory: {0}")]
    Session(String),
    /// A transport error with no HTTP response (a request that cannot be built is `Session`).
    #[error("zygo proxy unreachable: {0}")]
    Unreachable(String),
    /// `error.type` / `error.message` of the reply; "" when absent. With both absent the text is `HTTP <status>`; with
    /// no type, the message alone.
    #[error("{}", reply_text(*.status, .code, .message))]
    Reply { status: u16, code: String, message: String },
    /// A request that cannot be built: a token or session id that is not a valid header or path value.
    #[error("{0}")]
    Request(String),
}

fn reply_text(status: u16, code: &str, message: &str) -> String {
    match (code.is_empty(), message.is_empty()) {
        (true, true) => format!("HTTP {status}"),
        (true, false) => message.to_string(),
        _ => format!("{code}: {message}"),
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SessionStatus {
    pub session: String,
    pub profile: Option<String>,
    pub work: Option<String>,
    pub used_context: Option<u64>,
    pub effective_window: Option<u64>,
    pub compact: Option<WindowArg>,
    pub cache_read_share: Option<f64>,
    pub expires_at: String,
    /// `USAGE_OFF_NOTE` when the usage database did not open.
    #[serde(default)]
    pub usage_note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub threshold: Option<u64>,
}

/// Synchronous (call it outside a tokio runtime); owns a current-thread runtime and a reqwest client built with
/// `.no_proxy()` and `.timeout(t)`: the session token never goes through a system proxy (C3).
/// Sends `authorization: Bearer <token>` and `x-zygo-session: <id>`.
pub struct SessionClient {
    base: Url,
    session: String,
    token: String,
    timeout: Duration,
    rt: tokio::runtime::Runtime,
    http: reqwest::Client,
}

impl fmt::Debug for SessionClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SessionClient")
            .field("base", &self.base.as_str())
            .field("session", &self.session)
            .field("token", &"[redacted]")
            .field("timeout", &self.timeout)
            .finish()
    }
}

/// Default request timeout (§10.2.4: `zygo compact` exits 12 when unreachable within 10 s).
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

fn session_err(what: &str) -> ClientError {
    ClientError::Session(what.to_string())
}

/// `.no_proxy()`: the session token never goes through a system proxy (C3). No redirect is followed: the token goes
/// only to the URL of the session directory.
fn build_http(timeout: Duration) -> Result<reqwest::Client, ClientError> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(timeout)
        .build()
        .map_err(|e| ClientError::Session(format!("http client: {e}")))
}

/// C8: HTTPS, or plain HTTP to a loopback address only; no user name or password (reqwest would send them as a
/// second `authorization` header, and the URL is passed on to the agent).
pub fn transport_allowed(u: &Url) -> bool {
    if !u.username().is_empty() || u.password().is_some() {
        return false;
    }
    match u.scheme() {
        "https" => true,
        "http" => u.host_str().is_some_and(|h| {
            h.eq_ignore_ascii_case("localhost")
                || h.trim_start_matches('[').trim_end_matches(']').parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback())
        }),
        _ => false,
    }
}

impl SessionClient {
    /// Reads `SessionDir::url()` and `SessionDir::read_token()`; the session id is the directory's file name (§8.2 step 2).
    pub fn from_session_dir(dir: &Path) -> Result<SessionClient, ClientError> {
        let sd = SessionDir::new(dir.to_path_buf());
        let session =
            dir.file_name().and_then(|n| n.to_str()).ok_or_else(|| session_err("no session id in the path"))?.to_string();
        let raw = std::fs::read_to_string(sd.url()).map_err(|_| session_err("cannot read the proxy url file"))?;
        let mut base = Url::parse(raw.trim()).map_err(|_| session_err("the proxy url file holds no valid URL"))?;
        if !base.username().is_empty() || base.password().is_some() {
            return Err(session_err("the proxy url holds a user name or password (C8)"));
        }
        if !transport_allowed(&base) {
            return Err(session_err("the proxy url is not HTTPS and not a loopback address (C8)"));
        }
        if !base.path().ends_with('/') {
            let p = format!("{}/", base.path());
            base.set_path(&p);
        }
        // The token file's content never goes into the error.
        let token = sd.read_token().map_err(|_| session_err("cannot read the session token file"))?;
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| ClientError::Session(format!("runtime: {e}")))?;
        let http = build_http(DEFAULT_TIMEOUT)?;
        Ok(SessionClient { base, session, token, timeout: DEFAULT_TIMEOUT, rt, http })
    }

    /// Default 10 s (§10.2.4).
    pub fn timeout(mut self, d: Duration) -> SessionClient {
        if let Ok(http) = build_http(d) {
            self.http = http;
            self.timeout = d;
        }
        self
    }

    /// `POST …/compact`
    pub fn set_compact(&self, w: WindowArg) -> Result<Option<WindowArg>, ClientError> {
        let r: WindowReply = self.call(reqwest::Method::POST, "compact", Some(json!({ "window": w })))?;
        Ok(r.window)
    }

    /// `DELETE …/compact`
    pub fn clear_compact(&self) -> Result<Option<WindowArg>, ClientError> {
        let r: WindowReply = self.call(reqwest::Method::DELETE, "compact", None)?;
        Ok(r.window)
    }

    /// `GET …/status`
    pub fn status(&self) -> Result<SessionStatus, ClientError> {
        self.call(reqwest::Method::GET, "status", None)
    }

    /// `PATCH …/settings`
    pub fn set_settings(&self, s: &SessionSettings) -> Result<SessionSettings, ClientError> {
        let body = serde_json::to_value(s).map_err(|e| ClientError::Session(e.to_string()))?;
        self.call(reqwest::Method::PATCH, "settings", Some(body))
    }

    fn call<T: DeserializeOwned>(&self, m: reqwest::Method, leaf: &str, body: Option<Value>) -> Result<T, ClientError> {
        let url = self
            .base
            .join(&format!("v1/zygo/sessions/{}/{leaf}", self.session))
            .map_err(|_| session_err("invalid session id"))?;
        let mut rb = self
            .http
            .request(m, url)
            .header("authorization", format!("Bearer {}", self.token))
            .header("x-zygo-session", &self.session);
        if let Some(b) = body {
            rb = rb.json(&b);
        }
        let (status, bytes) = self.rt.block_on(async {
            let r = rb.send().await?;
            let status = r.status();
            Ok::<_, reqwest::Error>((status, r.bytes().await?))
        })
        // `without_url`: the error text names no URL; the token is never in one. A builder error is a bad session
        // file (a token or id that is no valid header value), with a fixed text.
        .map_err(|e| {
            if e.is_builder() {
                session_err("the session token or id is not a valid header value")
            } else {
                ClientError::Unreachable(e.without_url().to_string())
            }
        })?;
        if !status.is_success() {
            let v: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
            let field = |k: &str| v["error"][k].as_str().unwrap_or_default().to_string();
            return Err(ClientError::Reply { status: status.as_u16(), code: field("type"), message: field("message") });
        }
        serde_json::from_slice(&bytes).map_err(|_| ClientError::Reply {
            status: status.as_u16(),
            code: String::new(),
            message: "invalid reply".into(),
        })
    }
}

#[derive(Deserialize)]
struct WindowReply {
    window: Option<WindowArg>,
}
