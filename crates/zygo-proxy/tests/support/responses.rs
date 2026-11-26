#![allow(dead_code)]
//! The Responses route harness (K§6.4): a proxy on a `ManualClock` with one session and one mock upstream on
//! 127.0.0.1. Included by each Responses test file with `#[path = "support/responses.rs"] mod support;`.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::Request;
use axum::response::Response;
use bytes::Bytes;
use http::{HeaderMap, StatusCode};
use serde_json::Value;
use tempfile::TempDir;
use zygo_config::keys::Secret;
use zygo_config::profile::Profile;
use zygo_config::{load, Dialect, KeyRef, LoadContext};
use zygo_core::ids::ulid;
use zygo_core::paths::Paths;
use zygo_core::Agent;
use zygo_proxy::provider::KeyResolver;
use zygo_proxy::redact_stage::RedactLimits;
use zygo_proxy::server::StartOpts;
use zygo_proxy::session::{ManualClock, SessionRequest, TeamAuth};
use zygo_proxy::{Proxy, ProxyHandle};
use zygo_redact::DeadlinePolicy;
use zygo_usage::{RequestRow, Store};

/// The catalog id every captured Codex body names.
pub const MODEL: &str = "gpt-mock";
pub const UPSTREAM_MODEL: &str = "up-rs";
pub const PROVIDER: &str = "p-rs";
/// `facts.max_output_tokens` of `MODEL`: the `max_tokens` of a body that sends no `max_output_tokens`.
pub const MAX_OUTPUT: u64 = 32000;

type Got = (String, HeaderMap, Vec<u8>);
type Reply = (u16, String, Vec<u8>);

struct Upstream {
    addr: SocketAddr,
    got: Arc<Mutex<Vec<Got>>>,
    reply: Arc<Mutex<Reply>>,
    _stop: tokio::sync::oneshot::Sender<()>,
}

impl Upstream {
    /// 127.0.0.1:0, own runtime thread, stopped on drop; records (path with query, headers, body) and answers the
    /// current reply.
    fn start(reply: Reply) -> Upstream {
        let got: Arc<Mutex<Vec<Got>>> = Arc::default();
        let reply = Arc::new(Mutex::new(reply));
        let l = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        l.set_nonblocking(true).unwrap();
        let addr = l.local_addr().unwrap();
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let (g2, r2) = (got.clone(), reply.clone());
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
            rt.block_on(async move {
                let l = tokio::net::TcpListener::from_std(l).unwrap();
                let app = axum::Router::new().fallback(move |req: Request| {
                    let (g3, r3) = (g2.clone(), r2.clone());
                    async move {
                        let path = req.uri().path_and_query().map(|p| p.as_str().to_string()).unwrap_or_default();
                        let headers = req.headers().clone();
                        let body = axum::body::to_bytes(req.into_body(), usize::MAX).await.unwrap().to_vec();
                        g3.lock().unwrap().push((path, headers, body));
                        let (status, ctype, b) = r3.lock().unwrap().clone();
                        Response::builder()
                            .status(status)
                            .header("content-type", ctype)
                            .body(axum::body::Body::from(b))
                            .unwrap()
                    }
                });
                tokio::select! {
                    _ = async { axum::serve(l, app).await } => {}
                    _ = rx => {}
                }
            });
        });
        Upstream { addr, got, reply, _stop: tx }
    }
}

struct Keys(HashMap<String, Secret>);

impl KeyResolver for Keys {
    fn resolve(&self, r: &KeyRef) -> Option<Secret> {
        match r {
            KeyRef::Keychain(n) => self.0.get(n).cloned(),
            KeyRef::Env(_) => None,
        }
    }
}

pub struct Harness {
    d: TempDir,
    up: Upstream,
    h: ProxyHandle,
    token: String,
    session: String,
    /// The provider key's value (built at run time).
    key: String,
    /// `post` calls: each reaches a model route and writes one usage row.
    posts: AtomicU64,
    clock: Arc<ManualClock>,
    /// The one team token the proxy's test team-auth accepts (built at run time).
    team_token: String,
}

struct OneTeam(String);

impl TeamAuth for OneTeam {
    fn label_for(&self, token: &str) -> Option<String> {
        (token == self.0).then(|| "rs-team".to_string())
    }
}

fn paths(d: &TempDir) -> Paths {
    let d = d.path();
    Paths {
        config_file: d.join("config.toml"),
        cache_dir: d.join("cache"),
        data_dir: d.join("data"),
        sessions_dir: d.join("cache/sessions"),
        marketplaces_dir: d.join("cache/marketplaces"),
        plugin_store: d.join("data/plugins"),
        approvals_file: d.join("data/approvals.toml"),
        usage_db: d.join("data/usage.db"),
    }
}

fn dialect_name(d: Dialect) -> &'static str {
    match d {
        Dialect::Anthropic => "anthropic",
        Dialect::Openai => "openai",
        Dialect::OpenaiChat => "openai-chat",
    }
}

impl Harness {
    /// A proxy on a `ManualClock` with one session of `agent` and one mock upstream of dialect `upstream`; the config
    /// is the role `code` → `MODEL` served by `PROVIDER` with the key `keychain:k-rs`, then `extra_toml`.
    pub fn start(upstream: Dialect, agent: Agent, extra_toml: &str) -> Harness {
        Harness::start_with_profile(upstream, agent, extra_toml, None)
    }

    /// As `start`, with the session's `profile`.
    pub fn start_with_profile(upstream: Dialect, agent: Agent, extra_toml: &str, profile: Option<Profile>) -> Harness {
        let d = TempDir::new().unwrap();
        let body = if upstream == Dialect::Anthropic {
            br#"{"type":"error","error":{"type":"invalid_request_error","message":"rs3 mock"}}"#.to_vec()
        } else {
            br#"{"error":{"type":"invalid_request_error","message":"rs3 mock"}}"#.to_vec()
        };
        let up = Upstream::start((400, "application/json".into(), body));
        let toml = format!(
            "[role.code]\nmodel = \"{MODEL}\"\n\n[[model]]\nid = \"{MODEL}\"\n[model.facts]\nreasoning = \"always\"\n\
             effort_levels = [\"low\", \"medium\", \"high\"]\nmax_output_tokens = {MAX_OUTPUT}\ncontext_window = 200000\n\
             sampling = true\nvision = true\nstructured_output = \"native\"\nforced_tool_choice = true\n\n\
             [[model.provider]]\nname = \"{PROVIDER}\"\nbase_url = \"http://127.0.0.1:{}\"\ndialects = [\"{}\"]\n\
             upstream_model = \"{UPSTREAM_MODEL}\"\nkey = \"keychain:k-rs\"\n\n{extra_toml}",
            up.addr.port(),
            dialect_name(upstream),
        );
        std::fs::write(d.path().join("config.toml"), toml).unwrap();
        let cfg = load(&LoadContext { paths: paths(&d), repo: None, team: None }).unwrap();
        // Built at run time (no credential-shaped literal).
        let key = format!("test-key-k-rs-{}", ulid());
        let keys = Keys([("k-rs".to_string(), Secret::new(key.clone()))].into_iter().collect());
        let clock = ManualClock::new();
        let team_token = format!("team-{}", ulid());
        let o = StartOpts {
            clock: clock.clone(),
            team: Some(Arc::new(OneTeam(team_token.clone()))),
            keys: Arc::new(keys),
            usage_db: Some(d.path().join("data/usage.db")),
            // A pass that must finish never hits its deadline.
            redact_limits: Some(RedactLimits {
                secret: DeadlinePolicy { base: Duration::from_secs(60), per_mib: Duration::ZERO, expire_after_scans: None },
                content_budget: Duration::from_secs(60),
                ..Default::default()
            }),
            ..Default::default()
        };
        let h = Proxy::start_local_with(Arc::new(cfg), o).unwrap();
        let g = h
            .create_session(SessionRequest {
                agent,
                agent_version: "2.1.328".into(),
                profile,
                passthrough: false,
                repo: None,
                work: None,
                stored_values: vec![],
                known_plugins: Default::default(),
            })
            .unwrap();
        assert_eq!(g.profile_warning, None, "the profile is valid");
        Harness { d, up, h, token: g.token, session: g.session, key, posts: AtomicU64::new(0), clock, team_token }
    }

    /// Advances the proxy's `ManualClock`.
    pub fn advance(&self, d: Duration) {
        self.clock.advance(d);
    }

    /// A second session, a team session created over HTTP with the team token (only a team session suspends, §7.2):
    /// (session id, session token).
    pub fn team_session(&self) -> (String, String) {
        let bearer = format!("Bearer {}", self.team_token);
        let body = serde_json::json!({"agent":"claude-code","agent_version":"2.1.328","profile":null,"passthrough":false,"repo":null,"work":null});
        let (s, _, b) = self.send("POST", "v1/zygo/sessions", &[("authorization", &bearer)], Some(&body));
        assert_eq!(s, 200, "{}", String::from_utf8_lossy(&b));
        let v: Value = serde_json::from_slice(&b).unwrap();
        (v["session"].as_str().unwrap().to_string(), v["token"].as_str().unwrap().to_string())
    }

    /// The value of the provider's key (`keychain:k-rs`).
    pub fn provider_key(&self) -> &str {
        &self.key
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    pub fn session_id(&self) -> &str {
        &self.session
    }

    fn send(&self, method: &str, path: &str, headers: &[(&str, &str)], body: Option<&Value>) -> (StatusCode, HeaderMap, Bytes) {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            let _ = rustls::crypto::ring::default_provider().install_default();
            let client = reqwest::Client::builder().no_proxy().build().unwrap();
            let m = reqwest::Method::from_bytes(method.as_bytes()).unwrap();
            let mut rb = client.request(m, self.h.url().join(path).unwrap());
            for (k, v) in headers {
                rb = rb.header(*k, *v);
            }
            if let Some(b) = body {
                rb = rb.header("content-type", "application/json").body(b.to_string());
            }
            let r = rb.send().await.unwrap();
            let s = StatusCode::from_u16(r.status().as_u16()).unwrap();
            let hd = r.headers().clone();
            (s, hd, r.bytes().await.unwrap())
        })
    }

    /// POST with the session's identity in the Codex form (`authorization: Bearer`, `x-zygo-session`); counts one
    /// model post.
    pub fn post(&self, path: &str, body: &Value) -> (StatusCode, HeaderMap, Bytes) {
        self.posts.fetch_add(1, Ordering::SeqCst);
        let bearer = format!("Bearer {}", self.token);
        self.send("POST", path, &[("authorization", &bearer), ("x-zygo-session", &self.session)], Some(body))
    }

    /// POST with exactly `headers` (plus `content-type`); counts nothing.
    pub fn post_as(&self, path: &str, headers: &[(&str, &str)], body: &Value) -> (StatusCode, HeaderMap, Bytes) {
        self.send("POST", path, headers, Some(body))
    }

    pub fn get(&self, path: &str) -> (StatusCode, HeaderMap, Bytes) {
        let bearer = format!("Bearer {}", self.token);
        self.send("GET", path, &[("authorization", &bearer), ("x-zygo-session", &self.session)], None)
    }

    /// GET with exactly `headers`; counts nothing.
    pub fn get_as(&self, path: &str, headers: &[(&str, &str)]) -> (StatusCode, HeaderMap, Bytes) {
        self.send("GET", path, headers, None)
    }

    /// Each upstream request body as JSON; panics on a non-JSON body.
    pub fn upstream_bodies(&self) -> Vec<Value> {
        self.up.got.lock().unwrap().iter().map(|g| serde_json::from_slice(&g.2).expect("upstream body is JSON")).collect()
    }

    pub fn upstream_paths(&self) -> Vec<String> {
        self.up.got.lock().unwrap().iter().map(|g| g.0.clone()).collect()
    }

    pub fn upstream_headers(&self) -> Vec<HeaderMap> {
        self.up.got.lock().unwrap().iter().map(|g| g.1.clone()).collect()
    }

    /// The mock's reply to every later request.
    pub fn reply_with(&self, status: u16, content_type: &str, body: Vec<u8>) {
        *self.up.reply.lock().unwrap() = (status, content_type.to_string(), body);
    }

    fn usage_db(&self) -> PathBuf {
        self.d.path().join("data/usage.db")
    }

    /// The session's usage rows, once every `post` wrote its row. Call it only when every `post` reached a model route.
    pub fn usage_rows(&self) -> Vec<RequestRow> {
        // The bound the other proxy test files use for this wait; it returns as soon as the rows are written.
        assert!(self.h.usage_written(self.posts.load(Ordering::SeqCst), Duration::from_secs(10)), "usage rows not written");
        Store::open(&self.usage_db()).unwrap().requests_for_session(&self.session).unwrap()
    }
}

/// `fixtures/codex/requests/<name>.json`: its `body` value.
pub fn codex_request(name: &str) -> Value {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/codex/requests").join(format!("{name}.json"));
    let v: Value = serde_json::from_slice(&std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))).unwrap();
    v["body"].clone()
}
