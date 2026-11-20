//! Serve mode client (§7.1, §7.2, C8, §14, §15.3, §17): the team server's config and plugin list, the session, its
//! heartbeat and its usage rows, each call within the §7 bounds; the pending-sync mark (§14) and the session's team
//! file. No error text holds the team token (C3).

use std::ffi::OsString;
use std::fmt;
use std::io::Write;
use std::path::Path;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread::JoinHandle;
use std::time::Duration;

use reqwest::header::{HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use zygo_config::keys::{error_kind, resolve_with, Secret, SecretStore};
use zygo_config::Config;
pub use zygo_config::{read_team, TEAM_FILE};
use zygo_core::session_dir::live;
use zygo_proxy::client::transport_allowed;
pub use zygo_proxy::client::ClientError;
use zygo_proxy::session::{SessionGrant, SessionRequest};
use zygo_proxy::team::{PluginsReply, TeamConfigReply, TEAM_SECTIONS};
use zygo_proxy::Url;
use zygo_usage::{RequestRow, Store};

pub use zygo_core::session_dir::PID_FILE;

/// Connect bound of every control call. Spec §7 note 2026-11-19 (owner: follow sensible standards).
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Whole-call bound of every control call. Spec §7 note 2026-11-19 (owner: follow sensible standards).
pub const CALL_TIMEOUT: Duration = Duration::from_secs(30);
/// §7.2.
pub const HEARTBEAT_EVERY: Duration = Duration::from_secs(60);
/// §7.2: the session is gone on the server.
pub const RESTARTED: &str = "zygo server restarted; relaunch the session";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bounds {
    pub connect: Duration,
    pub call: Duration,
}

impl Default for Bounds {
    fn default() -> Self {
        Bounds { connect: CONNECT_TIMEOUT, call: CALL_TIMEOUT }
    }
}

/// Serve mode cannot start: the launcher exits 2 for `Url` (C8), else 1.
#[derive(Debug)]
pub enum Refused {
    Url,
    NoToken,
    Store(String),
    Client(String),
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refused::Url => f.write_str(
                "--server/[server] url must be https://, or http:// to a loopback host, with no user name or password (C8)",
            ),
            Refused::NoToken => f.write_str("serve mode needs a team token: [server] token is not set or names an empty value"),
            Refused::Store(k) => write!(f, "team token: the secret store failed ({k})"),
            Refused::Client(e) => write!(f, "serve client: {e}"),
        }
    }
}

impl std::error::Error for Refused {}

/// The server did not answer (connect failure or a timeout): exit 12 (§8.2, §7 note). The text is the whole line.
#[derive(Debug)]
pub struct Unreachable(pub String);

impl fmt::Display for Unreachable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Unreachable {}

/// Synchronous (call it outside a tokio runtime). `.no_proxy()`, no redirect: the team token goes only to the server
/// URL (C3, C8).
pub struct ServeClient {
    base: Url,
    token: Secret,
    bounds: Bounds,
    rt: tokio::runtime::Runtime,
    http: reqwest::Client,
}

impl fmt::Debug for ServeClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServeClient").field("base", &self.base.as_str()).field("token", &"[redacted]").finish()
    }
}

fn http_client(b: Bounds) -> Result<reqwest::Client, String> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(b.connect)
        .timeout(b.call)
        .build()
        .map_err(|e| format!("http client: {e}"))
}

fn runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| format!("runtime: {e}"))
}

/// `u` with a path that ends in `/`.
fn with_slash(u: &Url) -> Url {
    let mut base = u.clone();
    if !base.path().ends_with('/') {
        let p = format!("{}/", base.path());
        base.set_path(&p);
    }
    base
}

fn bounded(url: &Url, token: &Secret, bounds: Bounds) -> Result<ServeClient, Refused> {
    if !transport_allowed(url) {
        return Err(Refused::Url);
    }
    let rt = runtime().map_err(Refused::Client)?;
    let http = http_client(bounds).map_err(Refused::Client)?;
    Ok(ServeClient { base: with_slash(url), token: token.clone(), bounds, rt, http })
}

/// [`ServeClient::connect`] with other bounds.
#[cfg(feature = "test-hooks")]
pub fn connect_with(url: &Url, token: &Secret, bounds: Bounds) -> anyhow::Result<ServeClient> {
    Ok(bounded(url, token, bounds)?)
}

/// A transport error and its source chain, `: ` between links (S4). `without_url`: no link names the URL (a TLS name
/// error names the host only); no link holds a header, so the token never appears (C3).
fn transport_text(e: reqwest::Error) -> String {
    let e = e.without_url();
    let mut text = e.to_string();
    let mut link = std::error::Error::source(&e);
    while let Some(c) = link {
        text.push_str(": ");
        text.push_str(&c.to_string());
        link = c.source();
    }
    text
}

fn invalid_reply(status: u16) -> ClientError {
    ClientError::Reply { status, code: String::new(), message: "invalid reply".into() }
}

/// The longest session id: a ULID (§7.2 `<ulid>`).
pub const SESSION_ID_MAX: usize = 26;

/// `v1/zygo/sessions/<id>[/<leaf>]`; an id that is empty, longer than [`SESSION_ID_MAX`] or not ASCII alphanumeric is
/// refused.
fn sid(id: &str, leaf: &str) -> Result<String, ClientError> {
    if id.is_empty() || id.len() > SESSION_ID_MAX || !id.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(ClientError::Request("invalid session id".into()));
    }
    Ok(if leaf.is_empty() { format!("v1/zygo/sessions/{id}") } else { format!("v1/zygo/sessions/{id}/{leaf}") })
}

/// The rows of a session (§14) and whether the server's queue for it was empty (Q2).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Rows {
    #[serde(rename = "requests")]
    pub rows: Vec<RequestRow>,
    pub complete: bool,
}

impl ServeClient {
    /// C8 (`Refused::Url`) before anything else; the §7 bounds.
    pub fn connect(url: &Url, token: &Secret) -> anyhow::Result<ServeClient> {
        Ok(bounded(url, token, Bounds::default())?)
    }

    /// The server URL, its path ending in `/`.
    pub fn url(&self) -> Url {
        self.base.clone()
    }

    /// A second client with the same URL, token and bounds (for the heartbeat thread).
    pub fn try_clone(&self) -> anyhow::Result<ServeClient> {
        Ok(bounded(&self.base, &self.token, self.bounds)?)
    }

    fn send(&self, m: reqwest::Method, path: &str, body: Option<Vec<u8>>) -> Result<(u16, Vec<u8>), ClientError> {
        let mut auth = HeaderValue::from_str(&format!("Bearer {}", self.token.expose()))
            .map_err(|_| ClientError::Request("the team token is not a valid header value".into()))?;
        auth.set_sensitive(true);
        let url = self.base.join(path).map_err(|_| ClientError::Request("invalid session id".into()))?;
        let mut rb = self.http.request(m, url).header(AUTHORIZATION, auth);
        if let Some(b) = body {
            rb = rb.header(CONTENT_TYPE, "application/json").body(b);
        }
        let (status, bytes) = self
            .rt
            .block_on(async {
                let r = rb.send().await?;
                let status = r.status();
                Ok::<_, reqwest::Error>((status, r.bytes().await?))
            })
            .map_err(|e| ClientError::Unreachable(transport_text(e)))?;
        if !status.is_success() {
            let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
            let field = |k: &str| v["error"][k].as_str().unwrap_or_default().to_string();
            return Err(ClientError::Reply { status: status.as_u16(), code: field("type"), message: field("message") });
        }
        Ok((status.as_u16(), bytes.to_vec()))
    }

    fn json<T: DeserializeOwned>(&self, m: reqwest::Method, path: &str, body: Option<Vec<u8>>) -> Result<T, ClientError> {
        let (status, bytes) = self.send(m, path, body)?;
        serde_json::from_slice(&bytes).map_err(|_| invalid_reply(status))
    }

    /// `GET /v1/zygo/config` (§7.2).
    pub fn team_config(&self) -> Result<TeamConfigReply, ClientError> {
        self.json(reqwest::Method::GET, "v1/zygo/config", None)
    }

    /// `GET /v1/zygo/plugins` (§15.3).
    pub fn plugins(&self) -> Result<PluginsReply, ClientError> {
        self.json(reqwest::Method::GET, "v1/zygo/plugins", None)
    }

    /// The team layer (§6.1 layer 2): the config reply with the plugin reply's marketplaces.
    pub fn team_table(&self) -> Result<toml::Table, ClientError> {
        let (c, p) = (self.team_config()?, self.plugins()?);
        team_table(&c, &p).map_err(|_| invalid_reply(200))
    }

    /// `POST /v1/zygo/sessions` (§7.2); the body has only the HTTP fields (§20 #29d). The reply is untrusted (§8.2
    /// step 2 note 2026-11-19): an id that `sid` refuses or a token that is not a header value is an invalid reply.
    pub fn create_session(&self, req: &SessionRequest) -> Result<SessionGrant, ClientError> {
        let body = serde_json::to_vec(req).map_err(|e| ClientError::Request(e.to_string()))?;
        let g: SessionGrant = self.json(reqwest::Method::POST, "v1/zygo/sessions", Some(body))?;
        if sid(&g.session, "").is_err() || HeaderValue::from_str(&g.token).is_err() {
            return Err(invalid_reply(200));
        }
        Ok(g)
    }

    /// `POST /v1/zygo/sessions/<id>/heartbeat` (§7.2).
    pub fn heartbeat(&self, id: &str) -> Result<(), ClientError> {
        self.send(reqwest::Method::POST, &sid(id, "heartbeat")?, None).map(drop)
    }

    /// `GET /v1/zygo/sessions/<id>/requests` (§14).
    pub fn requests(&self, id: &str) -> Result<Rows, ClientError> {
        self.json(reqwest::Method::GET, &sid(id, "requests")?, None)
    }

    /// `DELETE /v1/zygo/sessions/<id>` (§7.2).
    pub fn delete_session(&self, id: &str) -> Result<(), ClientError> {
        self.send(reqwest::Method::DELETE, &sid(id, "")?, None).map(drop)
    }
}

/// The config reply as a table, its `marketplace` list replaced by the plugin reply's (none when that is empty). Only the
/// team sections and the catalog are kept (§7.2): a server cannot set `[user]`, `[work]` or `[server]` on the client.
pub fn team_table(config: &TeamConfigReply, plugins: &PluginsReply) -> Result<toml::Table, toml::ser::Error> {
    let mut t = toml::Table::try_from(config)?;
    t.retain(|k, _| k == "model" || TEAM_SECTIONS.contains(&k));
    if !plugins.marketplace.is_empty() {
        t.insert("marketplace".into(), toml::Value::try_from(&plugins.marketplace)?);
    }
    Ok(t)
}

/// `401 zygo_session_unknown`: the server has no such session (§7.2).
pub fn is_session_unknown(e: &ClientError) -> bool {
    matches!(e, ClientError::Reply { status: 401, code, .. } if code == "zygo_session_unknown")
}

/// [`from_config_with`] with the §7 bounds.
pub fn from_config(
    flag: Option<&str>,
    cfg: &Config,
    secrets: &dyn SecretStore,
    env: &dyn Fn(&str) -> Option<OsString>,
) -> Result<Option<ServeClient>, Refused> {
    from_config_with(flag, cfg, secrets, env, Bounds::default())
}

/// `--server` (layer 6) over `[server] url` (layer 3); `None` when neither is set (local mode). The URL and C8 are
/// checked before the token.
pub fn from_config_with(
    flag: Option<&str>,
    cfg: &Config,
    secrets: &dyn SecretStore,
    env: &dyn Fn(&str) -> Option<OsString>,
    bounds: Bounds,
) -> Result<Option<ServeClient>, Refused> {
    let Some(raw) = flag.or(cfg.server.url.as_deref()) else { return Ok(None) };
    let url = Url::parse(raw).map_err(|_| Refused::Url)?;
    if !transport_allowed(&url) {
        return Err(Refused::Url);
    }
    let token = match &cfg.server.token {
        None => None,
        Some(k) => resolve_with(k, secrets, env).map_err(|e| Refused::Store(error_kind(&e).into()))?,
    };
    bounded(&url, &token.ok_or(Refused::NoToken)?, bounds).map(Some)
}

/// Beats every `every` until dropped (§7.2). `401 zygo_session_unknown` reports `RESTARTED` once and ends the loop;
/// other errors are ignored. Drop joins the thread, so no beat reaches the server after it (a beat in flight ends
/// within the call bound).
pub struct Heartbeat {
    stop: Option<mpsc::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl Heartbeat {
    pub fn start(client: ServeClient, session: String, every: Duration, report: Box<dyn Fn(&str) + Send>) -> Heartbeat {
        let (tx, rx) = mpsc::channel::<()>();
        let thread = std::thread::Builder::new()
            .name("zygo-heartbeat".into())
            .spawn(move || {
                while let Err(RecvTimeoutError::Timeout) = rx.recv_timeout(every) {
                    if let Err(e) = client.heartbeat(&session)
                        && is_session_unknown(&e)
                    {
                        report(RESTARTED);
                        return;
                    }
                }
            })
            .ok();
        Heartbeat { stop: Some(tx), thread }
    }
}

impl Drop for Heartbeat {
    fn drop(&mut self) {
        drop(self.stop.take());
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Why a session stays pending sync at exit (R6).
#[derive(Debug, Clone, PartialEq)]
pub enum RowsEnd {
    Incomplete,
    NotFetched(String),
}

impl fmt::Display for RowsEnd {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RowsEnd::Incomplete => f.write_str("usage rows incomplete"),
            RowsEnd::NotFetched(e) => write!(f, "usage rows not fetched ({e})"),
        }
    }
}

impl std::error::Error for RowsEnd {}

/// The launcher's exit (§14, B2, S5, R1): the fetched rows replace the session's local rows; the pending mark is
/// cleared when they are complete or the server does not know the session, and set again on every other path (a
/// mark error is ignored).
pub fn end_rows(store: &Store, session: &str, fetched: Result<Rows, ClientError>) -> Result<(), RowsEnd> {
    let end = match fetched {
        Ok(r) => match store.replace_requests(session, &r.rows) {
            Err(e) => RowsEnd::NotFetched(e.to_string()),
            Ok(_) if r.complete => {
                let _ = store.clear_sync_pending(session);
                return Ok(());
            }
            Ok(_) => RowsEnd::Incomplete,
        },
        Err(e) if is_session_unknown(&e) => {
            let _ = store.clear_sync_pending(session);
            return Ok(());
        }
        Err(e) => RowsEnd::NotFetched(e.to_string()),
    };
    let _ = store.mark_sync_pending(session);
    Err(end)
}

/// `zygo usage --sync`: `tried` pending sessions fetched, `synced` of them complete, `live` skipped; one line per
/// session that stays pending or failed.
#[derive(Debug, Default, PartialEq)]
pub struct SyncReport {
    pub synced: usize,
    pub tried: usize,
    pub live: usize,
    pub unreachable: bool,
    pub lines: Vec<String>,
}

/// Retries each pending session (§14, B2): a live one ([`live`], R1, U3) is skipped; an unreachable one is reported
/// and the next one tried (R5). `Err` only for a store error.
pub fn sync_pending(client: &ServeClient, store: &Store, sessions_dir: &Path) -> anyhow::Result<SyncReport> {
    let mut r = SyncReport::default();
    for id in store.sync_pending()? {
        if live(&sessions_dir.join(&id)) {
            r.live += 1;
            continue;
        }
        r.tried += 1;
        match client.requests(&id) {
            Ok(rows) => {
                store.replace_requests(&id, &rows.rows)?;
                if rows.complete {
                    store.clear_sync_pending(&id)?;
                    r.synced += 1;
                } else {
                    r.lines.push(format!("session {id}: {}", RowsEnd::Incomplete));
                }
            }
            Err(e) if is_session_unknown(&e) => store.clear_sync_pending(&id)?,
            Err(ClientError::Unreachable(d)) => {
                r.unreachable = true;
                r.lines.push(format!("session {id}: server unreachable: {d}"));
            }
            Err(e) => r.lines.push(format!("session {id}: {e}")),
        }
    }
    Ok(r)
}

/// Writes `<dir>/team.toml` (created 0600 on unix, truncated).
pub fn write_team(dir: &Path, team: &toml::Table) -> std::io::Result<()> {
    let text = toml::to_string(team).map_err(std::io::Error::other)?;
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
    o.open(dir.join(TEAM_FILE))?.write_all(text.as_bytes())
}

/// The `zygo doctor` server line after `server: ` (§17): `GET <url>/healthz` with no token, within the §7 bounds.
pub fn reachability(url: &str) -> String {
    reachability_bounded(url, Bounds::default())
}

/// [`reachability`] with other bounds.
#[cfg(feature = "test-hooks")]
pub fn reachability_with(url: &str, bounds: Bounds) -> String {
    reachability_bounded(url, bounds)
}

/// The URL is printed without its user and password (N5); a value that does not parse is not printed (U2).
fn reachability_bounded(url: &str, bounds: Bounds) -> String {
    const NOT_A_URL: &str = "[server] url is not a URL";
    let Ok(u) = Url::parse(url) else { return NOT_A_URL.into() };
    let mut bare = u.clone();
    let _ = (bare.set_username(""), bare.set_password(None));
    let url = if bare == u { url } else { bare.as_str() };
    if bare != u {
        return format!("{url} not allowed: the URL must not hold a user name or password (C8)");
    }
    if !transport_allowed(&u) {
        return format!("{url} not allowed: not https and not a loopback host (C8)");
    }
    let Ok(health) = with_slash(&u).join("healthz") else { return NOT_A_URL.into() };
    let (rt, http) = match (runtime(), http_client(bounds)) {
        (Ok(rt), Ok(h)) => (rt, h),
        (Err(e), _) | (_, Err(e)) => return format!("{url} unreachable ({e})"),
    };
    match rt.block_on(async { http.get(health).send().await }) {
        Ok(r) if r.status().is_success() => format!("{url} reachable"),
        Ok(r) => format!("{url} unreachable (HTTP {})", r.status().as_u16()),
        Err(e) => format!("{url} unreachable ({})", transport_text(e)),
    }
}
