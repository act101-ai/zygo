//! N17: passthrough on the proxy (§12.2, §12.4, §12.5; §20 #25, #26 local part, #27). Fake upstreams on 127.0.0.1;
//! `api.anthropic.com` is resolved to a loopback fake through `upstream_test`, so no real port 443 is used.
//! Sample credentials are built at run time or are not credential-shaped.
#![allow(dead_code)]
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use axum::extract::Request;
use axum::response::Response;
use bytes::Bytes;
use http::HeaderMap;
use serde_json::{json, Value};
use tempfile::TempDir;
use zygo_config::keys::Secret;
use zygo_config::{load, Dialect, KeyRef, LoadContext, Provider};
use zygo_core::{ids::ulid, paths::Paths, Agent};
use zygo_proxy::client::SessionStatus;
use zygo_proxy::errors::ZygoError;
use zygo_proxy::passthrough::{no_login, token_invalid, CredentialType, PassthroughStage, CLAUDE_AI, TYPES};
use zygo_proxy::pipeline::{ModelRoute, Pipeline, RequestCtx, Stage};
use zygo_proxy::provider::KeyResolver;
use zygo_proxy::redact_stage::RedactLimits;
use zygo_proxy::server::StartOpts;
use zygo_proxy::session::{ManualClock, SessionGrant, SessionRequest, IDLE_TTL};
use zygo_proxy::{Proxy, ProxyHandle, Url};
use zygo_redact::DeadlinePolicy;
use zygo_usage::{RedactionCount, RequestRow, Store};

const M: &str = "anthropic/v1/messages";
const CT: &str = "anthropic/v1/messages/count_tokens";
const MODELS: &str = "anthropic/v1/models";
/// The §20 #25 values; not credential-shaped (two digits must follow `oat`).
const OAT: &str = "sk-ant-oat-x";
const BEARER: &str = "Bearer sk-ant-oat-x";
const FINAL: (&str, &str) = ("x-claude-code-agent-id", "final");

#[derive(Debug, Clone)]
struct Got { path: String, headers: HeaderMap, body: Vec<u8> }

/// The reply body of a new `Upstream`.
const REPLY: &str = r#"{"id":"m","type":"message","role":"assistant","content":[],"usage":{"input_tokens":11,"output_tokens":22}}"#;

struct Upstream {
    addr: SocketAddr,
    got: Arc<Mutex<Vec<Got>>>,
    reply: Arc<Mutex<String>>,
    status: Arc<Mutex<u16>>,
    headers: Arc<Mutex<Vec<(String, String)>>>,
    _stop: tokio::sync::oneshot::Sender<()>,
}
impl Upstream {
    /// 127.0.0.1:0, own runtime thread; every request is recorded and answered 200 JSON (`REPLY` until `reply`;
    /// status, extra headers and body until `answer`).
    fn start() -> Upstream {
        let got: Arc<Mutex<Vec<Got>>> = Arc::default();
        let reply = Arc::new(Mutex::new(REPLY.to_string()));
        let status = Arc::new(Mutex::new(200u16));
        let headers: Arc<Mutex<Vec<(String, String)>>> = Arc::default();
        let (r2, s2, h2) = (reply.clone(), status.clone(), headers.clone());
        let l = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        l.set_nonblocking(true).unwrap();
        let addr = l.local_addr().unwrap();
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let g2 = got.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
            rt.block_on(async move {
                let l = tokio::net::TcpListener::from_std(l).unwrap();
                let app = axum::Router::new().fallback(move |req: Request| {
                    let (g3, r3, s3, h3) = (g2.clone(), r2.clone(), s2.clone(), h2.clone());
                    async move {
                        let path = req.uri().path_and_query().map(|p| p.as_str().to_string()).unwrap_or_default();
                        let headers = req.headers().clone();
                        let body = axum::body::to_bytes(req.into_body(), usize::MAX).await.unwrap().to_vec();
                        g3.lock().unwrap().push(Got { path, headers, body });
                        let mut b = Response::builder().status(*s3.lock().unwrap()).header("content-type", "application/json");
                        for (k, v) in h3.lock().unwrap().iter() {
                            b = b.header(k.as_str(), v.as_str());
                        }
                        b.body(axum::body::Body::from(r3.lock().unwrap().clone())).unwrap()
                    }
                });
                tokio::select! {
                    _ = async { axum::serve(l, app).await } => {}
                    _ = rx => {}
                }
            });
        });
        Upstream { addr, got, reply, status, headers, _stop: tx }
    }
    /// Every later request is answered with `body`.
    fn reply(&self, body: &Value) { *self.reply.lock().unwrap() = body.to_string(); }
    /// Every later request is answered with `status`, `headers` (after `content-type`) and `body`.
    fn answer(&self, status: u16, headers: &[(&str, &str)], body: &Value) {
        *self.status.lock().unwrap() = status;
        *self.headers.lock().unwrap() = headers.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        self.reply(body);
    }
    fn port(&self) -> u16 { self.addr.port() }
    fn got(&self) -> Vec<Got> { self.got.lock().unwrap().clone() }
    /// The last recorded request whose path starts with `prefix`.
    fn at(&self, prefix: &str) -> Got { self.got().into_iter().rev().find(|g| g.path.starts_with(prefix)).expect("a recorded request") }
}

/// Counts every key lookup; `keychain:k-same` resolves, every other name is absent.
#[derive(Default)]
struct CountKeys { calls: AtomicUsize }
impl KeyResolver for CountKeys {
    fn resolve(&self, r: &KeyRef) -> Option<Secret> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match r {
            KeyRef::Keychain(n) if n == "k-same" => Some(Secret::new("test-key-same")),
            _ => None,
        }
    }
}

struct T { d: TempDir, up: Upstream, anth: Upstream, c: Arc<ManualClock>, keys: Arc<CountKeys>, h: ProxyHandle }

fn prov(name: &str, url: &str, upstream: &str, more: &str) -> String {
    format!("[[model.provider]]\nname = \"{name}\"\nbase_url = \"{url}\"\ndialects = [\"anthropic\"]\nupstream_model = \"{upstream}\"\n{more}\n")
}
fn model(id: &str, provs: &[String]) -> String { format!("[[model]]\nid = \"{id}\"\n{}\n", provs.concat()) }

/// The user config of the plan's harness; `extra` is appended last.
fn config(u: &str, a: &str, extra: &str) -> String {
    let prices = "[model.provider.price_usd_per_mtok]\ninput = 4.0\noutput = 20.0\ncache_read = 0.2\ncache_write = 8.0\n";
    let mut t = String::from("[role.code]\nmodel = \"test-same\"\n\n");
    t += &model("test-same", &[prov("p-same", &format!("{u}/same"), "test-same", "key = \"keychain:k-same\"\n")]);
    t += &model("test-oai-dup", &[prov("p-oai", &format!("{u}/oai"), "claude-test-1", "").replace("[\"anthropic\"]", "[\"openai\"]")]);
    t += &model("test-pt", &[
        prov("p-other", &format!("{u}/other"), "claude-test-1", "key = \"keychain:k-same\"\n"),
        prov("p-anth", a, "claude-test-1", &format!("key = \"keychain:k-absent\"\n{prices}")),
    ]);
    t += &model("test-pt-late", &[prov("p-late", &format!("{a}/late"), "claude-test-1", "")]);
    t += &model("test-elsewhere", &[prov("p-else", &format!("{u}/else"), "claude-else-1", "")]);
    t += &model("test-off", &[prov("p-off", &format!("{a}/off"), "claude-off-1", "enabled = false\n")]);
    t += "[[redaction.pattern]]\nid = \"tracer_block\"\npattern = \"TRACERBLOCK_[0-9]{6}\"\naction = \"block\"\n\n";
    t + extra
}

/// A pass that must finish never hits its deadline.
fn generous() -> RedactLimits {
    RedactLimits { secret: DeadlinePolicy { base: Duration::from_secs(60), per_mib: Duration::ZERO, expire_after_scans: None }, content_budget: Duration::from_secs(60), ..Default::default() }
}

fn setup(extra: &str) -> T { setup_with(extra, None) }

/// `setup`, with `pipeline` in place of the default stage list. `{UP}` in `extra` is the URL of `up`.
fn setup_with(extra: &str, pipeline: Option<Pipeline>) -> T {
    let d = TempDir::new().unwrap();
    let (up, anth) = (Upstream::start(), Upstream::start());
    let p = d.path();
    let (u, a) = (format!("http://127.0.0.1:{}", up.port()), "http://api.anthropic.com".to_string());
    std::fs::write(p.join("config.toml"), config(&u, &a, &extra.replace("{UP}", &u))).unwrap();
    let paths = Paths { config_file: p.join("config.toml"), cache_dir: p.join("cache"), data_dir: p.join("data"), sessions_dir: p.join("cache/sessions"),
                        marketplaces_dir: p.join("cache/marketplaces"), plugin_store: p.join("data/plugins"), approvals_file: p.join("data/approvals.toml"),
                        usage_db: p.join("data/usage.db") };
    let cfg = load(&LoadContext { paths, repo: None, team: None }).unwrap();
    let c = ManualClock::new();
    let keys = Arc::new(CountKeys::default());
    let o = StartOpts { clock: c.clone(), keys: keys.clone(), usage_db: Some(p.join("data/usage.db")),
                        upstream_test: Some(vec![("api.anthropic.com".into(), anth.addr)]), redact_limits: Some(generous()), pipeline, ..Default::default() };
    let h = Proxy::start_local_with(Arc::new(cfg), o).unwrap();
    T { d, up, anth, c, keys, h }
}

fn grant(t: &T, agent: Agent, passthrough: bool) -> SessionGrant {
    t.h.create_session(SessionRequest {
        agent,
        agent_version: "2.1.328".into(),
        profile: None,
        passthrough,
        repo: None,
        work: None,
        stored_values: vec![],
        known_plugins: Default::default(),
    })
    .unwrap()
}
fn pt(g: &SessionGrant) -> Vec<(&str, &str)> { vec![("x-zygo-token", g.token.as_str()), ("x-zygo-session", g.session.as_str())] }
/// A `RequestCtx` as stage 2 sees it.
fn ctx(g: &SessionGrant, t: &T, b: Value) -> RequestCtx {
    let s = t.h.session(&g.session).unwrap();
    RequestCtx { cfg: s.cfg.clone(), session: s, route: ModelRoute::Messages, dialect: Dialect::Anthropic, headers: HeaderMap::new(), raw: Bytes::new(), body: b,
                 changed: false, resolved: None, candidates: vec![], adjustments: vec![], redactions: BTreeMap::new(), redacted: false, line_mode: false }
}
fn hm(pairs: &[(&str, &str)]) -> HeaderMap {
    let mut h = HeaderMap::new();
    for (k, v) in pairs {
        h.append(http::header::HeaderName::from_bytes(k.as_bytes()).unwrap(), v.parse().unwrap());
    }
    h
}
fn sorted_names(h: &HeaderMap) -> Vec<String> {
    let mut v: Vec<String> = h.keys().map(|k| k.as_str().to_string()).collect();
    v.sort();
    v
}
fn hv<'a>(h: &'a HeaderMap, n: &str) -> &'a str { h.get(n).map(|v| v.to_str().unwrap()).unwrap_or("") }
fn key_sample() -> String { ["sk-", "ant-api03-", &"Ab1_".repeat(24)].concat() }
fn rows(t: &T, g: &SessionGrant) -> Vec<RequestRow> {
    Store::open(&t.d.path().join("data/usage.db")).unwrap().requests_for_session(&g.session).unwrap()
}
fn has(hay: &[u8], needle: &[u8]) -> bool { hay.windows(needle.len()).any(|w| w == needle) }
/// True when any recorded header value or body contains `OAT`.
fn oat_seen(up: &Upstream) -> bool {
    up.got().iter().any(|g| has(&g.body, OAT.as_bytes()) || g.headers.values().any(|v| has(v.as_bytes(), OAT.as_bytes())))
}
/// The bytes of every file under `dir` except the input `config.toml`.
fn read_all(dir: &std::path::Path) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            out.extend(read_all(&p));
        } else if p.file_name().is_some_and(|n| n != "config.toml")
            && let Ok(b) = std::fs::read(&p)
        {
            out.push(b);
        }
    }
    out
}
/// (status, headers, body).
fn send(t: &T, method: &str, path: &str, headers: &[(&str, &str)], body: Option<Vec<u8>>) -> (u16, HeaderMap, Vec<u8>) {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let m = reqwest::Method::from_bytes(method.as_bytes()).unwrap();
        let c = reqwest::Client::builder().no_proxy().build().unwrap();
        let mut rb = c.request(m, t.h.url().join(path).unwrap());
        for (k, v) in headers {
            rb = rb.header(*k, *v);
        }
        if let Some(b) = body {
            rb = rb.header("content-type", "application/json").body(b);
        }
        let r = rb.send().await.unwrap();
        let s = r.status().as_u16();
        let hd = r.headers().clone();
        (s, hd, r.bytes().await.unwrap().to_vec())
    })
}
/// A messages body for `model`.
fn mb(model: &str) -> Vec<u8> { serde_json::to_vec(&json!({"model": model, "max_tokens": 8, "messages": [{"role":"user","content":"hi"}]})).unwrap() }
/// POST with `pt(g)` + `authorization: BEARER` + `extra`.
fn post_pt(t: &T, g: &SessionGrant, path: &str, body: Vec<u8>, extra: &[(&str, &str)]) -> (u16, HeaderMap, Vec<u8>) {
    let mut hd = pt(g);
    hd.push(("authorization", BEARER));
    hd.extend_from_slice(extra);
    send(t, "POST", path, &hd, Some(body))
}
/// (status, error.type); asserts the Anthropic error shape.
fn st(r: &(u16, HeaderMap, Vec<u8>)) -> (u16, String) {
    let v: Value = serde_json::from_slice(&r.2).unwrap_or_else(|_| panic!("not JSON"));
    assert_eq!(v["type"], "error");
    assert!(v["error"]["message"].is_string());
    (r.0, v["error"]["type"].as_str().unwrap().to_string())
}
fn msg(r: &(u16, HeaderMap, Vec<u8>)) -> String {
    serde_json::from_slice::<Value>(&r.2).unwrap()["error"]["message"].as_str().unwrap().to_string()
}
fn no_login_r(r: &(u16, HeaderMap, Vec<u8>)) {
    assert_eq!(st(r), (401, "zygo_passthrough_no_login".to_string()));
    assert_eq!(msg(r), "passthrough needs a claude.ai login (/login)");
}
fn token_invalid_r(r: &(u16, HeaderMap, Vec<u8>)) {
    assert_eq!(st(r), (401, "zygo_passthrough_token_invalid".to_string()));
    assert_eq!(msg(r), "relaunch zygo claude --passthrough");
}
/// Row-count rule: a last request with `FINAL`, then exactly `k` rows, the last one `final`, statuses in order.
fn final_rows(t: &T, g: &SessionGrant, k: u64, statuses: &[u16]) -> (Vec<RequestRow>, (u16, HeaderMap, Vec<u8>)) {
    let mut hd = pt(g);
    hd.extend_from_slice(&[("authorization", BEARER), FINAL]);
    let resp = send(t, "POST", M, &hd, Some(mb("claude-test-1")));
    assert_eq!(resp.0, 200);
    assert!(t.h.usage_written(k, Duration::from_secs(10)), "usage rows not written");
    let r = rows(t, g);
    assert_eq!(r.len() as u64, k);
    assert_eq!(r.last().unwrap().agent_id.as_deref(), Some("final"));
    assert_eq!(r.iter().map(|w| w.status).collect::<Vec<_>>(), statuses);
    (r, resp)
}

// ---- unit ----

#[test] // §12.2 row; §12.4 allowlist; reading 6 host match
fn unit_credential_type_row_and_allowlist() {
    assert_eq!(TYPES, &[CLAUDE_AI]);
    assert_eq!((CLAUDE_AI.name, CLAUDE_AI.agent, CLAUDE_AI.upstream_host, CLAUDE_AI.dialect, CLAUDE_AI.header, CLAUDE_AI.scheme, CLAUDE_AI.prefix),
               ("claude-ai", Agent::ClaudeCode, "api.anthropic.com", Dialect::Anthropic, "authorization", "Bearer", "sk-ant-oat"));
    assert_eq!((CLAUDE_AI.refused, CLAUDE_AI.forwarded), (&["x-api-key"][..], &["authorization", "anthropic-beta"][..]));
    assert_eq!((CredentialType::for_agent(Agent::ClaudeCode), CredentialType::for_agent(Agent::OpenCode)), (Some(&CLAUDE_AI), None));
    for (h, ok) in [(hm(&[("authorization", BEARER)]), true), (hm(&[("authorization", "bearer sk-ant-oat-x")]), true),
        (hm(&[]), false), (hm(&[("x-api-key", OAT)]), false), (hm(&[("authorization", BEARER), ("x-api-key", "k")]), false),
        (hm(&[("authorization", "Bearer zyg_s_abc")]), false), (hm(&[("authorization", "Bearer other")]), false),
        (hm(&[("authorization", "Basic sk-ant-oat-x")]), false), (hm(&[("authorization", BEARER), ("authorization", BEARER)]), false)] {
        assert_eq!(CLAUDE_AI.accepts(&h), ok, "{:?}", h.keys().collect::<Vec<_>>());
    }
    for (u, ok) in [("https://api.anthropic.com/x", true), ("https://API.anthropic.com:443/x", true), ("http://api.anthropic.com:443/", true),
        ("http://API.anthropic.com:9/", false), ("https://api.anthropic.com:8443/", false), ("http://api.anthropic.com/", false),
        ("https://api.anthropic.com.evil.test/", false), ("https://xapi.anthropic.com/", false), ("https://evil-api.anthropic.com/", false),
        ("http://127.0.0.1:9/", false)] {
        assert_eq!(CLAUDE_AI.host_matches(&Url::parse(u).unwrap()), ok, "{u}");
    }
    let (a, b) = (token_invalid(), no_login());
    assert_eq!((a.status.as_u16(), a.code, a.message.as_str()), (401, "zygo_passthrough_token_invalid", "relaunch zygo claude --passthrough"));
    assert_eq!((b.status.as_u16(), b.code, b.message.as_str()), (401, "zygo_passthrough_no_login", "passthrough needs a claude.ai login (/login)"));
    assert_eq!(CLAUDE_AI.builtin_provider("m"), Provider { name: "claude-ai".into(), base_url: "https://api.anthropic.com".into(),
        dialects: vec![Dialect::Anthropic], upstream_model: "m".into(), ..Default::default() });
}

#[test] // §12.4 proxy; reading 9
fn unit_outbound_headers() {
    let c = hm(&[("authorization", BEARER), ("x-api-key", "k"), ("x-zygo-token", "z"), ("x-zygo-session", "s"), ("x-claude-code-agent-id", "a"),
                 ("anthropic-beta", "b1"), ("anthropic-version", "2023-06-01"), ("content-type", "application/json"), ("cookie", "c=1"),
                 ("accept-encoding", "gzip")]);
    let p = Provider { name: "p".into(), base_url: "https://api.anthropic.com".into(), dialects: vec![Dialect::Anthropic], upstream_model: "m".into(),
                       forward_headers: Some(vec!["content-type".into()]), ..Default::default() };
    let o = CLAUDE_AI.outbound(&c, &p);
    assert_eq!(sorted_names(&o), ["anthropic-beta", "authorization", "content-type"]);
    assert_eq!((o["authorization"].to_str().unwrap(), o["authorization"].is_sensitive(), o["anthropic-beta"].to_str().unwrap()), (BEARER, true, "b1"));
    let o = CLAUDE_AI.outbound(&c, &Provider { forward_headers: None, ..p });
    assert_eq!(sorted_names(&o), ["anthropic-beta", "anthropic-version", "authorization", "content-type"]);
    assert_eq!(o.get_all("anthropic-beta").iter().count(), 1);
}

// ---- stage 2 ----

/// `stage.on_request` on a fresh ctx; a passthrough `Ok` has one candidate with no key and no price.
fn run(t: &T, stage: &PassthroughStage, g: &SessionGrant, m: Value) -> Result<RequestCtx, ZygoError> {
    let mut c = ctx(g, t, json!({"model": m, "max_tokens": 8, "messages": []}));
    stage.on_request(&mut c)?;
    if c.session.passthrough {
        assert_eq!(c.candidates.len(), 1);
        assert!(c.candidates.iter().all(|k| k.key.is_none() && k.provider.key.is_none() && k.provider.price_usd_per_mtok.is_none()));
    }
    Ok(c)
}

#[test] // §12.4 stage 2; readings 5, 7, 8
fn stage_catalog_match_and_builtin() {
    // test-q: an on-host provider that does not speak the row dialect is not the candidate (mutant Q).
    let q = format!("{}{}", prov("p-q-oai", "http://api.anthropic.com/qoai", "claude-q-1", "").replace("[\"anthropic\"]", "[\"openai\"]"),
                    prov("p-q", "http://127.0.0.1:9/q", "claude-q-1", ""));
    let t = setup(&format!("[[model]]\nid = \"test-q\"\n{q}\n"));
    let (p, n) = (grant(&t, Agent::ClaudeCode, true), grant(&t, Agent::ClaudeCode, false));
    let (prod, seam) = (PassthroughStage::new(), PassthroughStage::with_test_hosts(vec!["api.anthropic.com".into()]));
    let c = run(&t, &seam, &p, json!("claude-test-1[1m]")).unwrap();
    let r = c.resolved.as_ref().unwrap();
    assert_eq!((r.requested.as_str(), r.one_m, r.role.as_str(), r.model.as_str()), ("claude-test-1", true, "subscription", "test-pt"));
    let k = &c.candidates[0];
    assert_eq!((k.provider.name.as_str(), k.provider.base_url.clone(), k.provider.upstream_model.as_str(), k.dialect, c.changed),
               ("p-anth", "http://api.anthropic.com".to_string(), "claude-test-1[1m]", Dialect::Anthropic, false));
    // Without the test seam, `http://api.anthropic.com` (port 80) is not on the type's host (port 443): the built-in provider.
    let c = run(&t, &prod, &p, json!("claude-test-1")).unwrap();
    assert_eq!((c.resolved.as_ref().unwrap().model.as_str(), &c.candidates[0].provider), ("test-pt", &CLAUDE_AI.builtin_provider("claude-test-1")));
    let c = run(&t, &seam, &p, json!("claude-q-1")).unwrap();
    assert_eq!((c.resolved.as_ref().unwrap().model.as_str(), c.candidates[0].provider.name.as_str()), ("test-q", "claude-ai"));
    let c = run(&t, &prod, &p, json!("claude-unknown-9")).unwrap();
    assert_eq!((c.resolved.as_ref().unwrap().model.as_str(), &c.candidates[0].provider), ("claude-unknown-9", &CLAUDE_AI.builtin_provider("claude-unknown-9")));
    assert_eq!(run(&t, &seam, &p, json!("claude-unknown-9")).unwrap().candidates[0].provider.base_url, "http://api.anthropic.com");
    let c = run(&t, &prod, &p, json!("claude-else-1")).unwrap();
    assert_eq!((c.resolved.as_ref().unwrap().model.as_str(), c.candidates[0].provider.name.as_str()), ("test-elsewhere", "claude-ai"));
    let c = run(&t, &seam, &p, json!("claude-off-1")).unwrap();
    assert_eq!((c.candidates[0].provider.name.as_str(), c.candidates[0].provider.enabled), ("p-off", false));
    assert_eq!(run(&t, &prod, &p, json!(5)).err().unwrap().code, "zygo_bad_request");
    let c = run(&t, &prod, &n, json!("code")).unwrap();
    let r = c.resolved.as_ref().unwrap();
    assert_eq!((r.role.as_str(), r.model.as_str(), c.candidates[0].provider.name.as_str()), ("code", "test-same", "p-same"));
}

// ---- e2e ----

#[test] // §20 #25
fn e2e_login_reaches_upstream_unchanged() {
    let t = setup("");
    let g = grant(&t, Agent::ClaudeCode, true);
    let b = br#"{"model":"claude-test-1","max_tokens":8,"messages":[{"role":"user","content":"hi"}]}"#;
    let r = post_pt(&t, &g, M, b.to_vec(), &[("anthropic-beta", "b1"), ("anthropic-version", "2023-06-01"),
                                             ("x-claude-code-request-class", "main"), ("x-zygo-repo", "r")]);
    assert_eq!(r.0, 200);
    assert_eq!((hv(&r.1, "x-zygo-role"), hv(&r.1, "x-zygo-model")), ("subscription", "test-pt"));
    let got = t.anth.got(); assert_eq!(got.len(), 1); assert!(t.up.got().is_empty());
    assert_eq!((got[0].path.as_str(), got[0].body.as_slice()), ("/v1/messages", &b[..]));
    let h = &got[0].headers;
    assert_eq!(h.get_all("authorization").iter().collect::<Vec<_>>(), [BEARER]);
    assert!(h.get("x-api-key").is_none());
    assert!(h.keys().all(|k| !k.as_str().starts_with("x-zygo-") && !k.as_str().starts_with("x-claude-code-")));
    assert_eq!((hv(h, "anthropic-beta"), hv(h, "anthropic-version")), ("b1", "2023-06-01"));
    assert!(t.h.usage_written(1, Duration::from_secs(10)));
    let w = &rows(&t, &g)[0];
    assert_eq!((w.role_requested.as_deref(), w.role_served.as_deref(), w.model.as_str(), w.provider.as_str(), w.passthrough, w.status),
               (Some("claude-test-1"), Some("subscription"), "test-pt", "p-anth", true, 200));
    assert_eq!((w.input_tokens, w.output_tokens, w.estimated_usd), (11, 22, 0.0)); // p-anth has prices: 0 is the passthrough rule
}

/// The Claude Code header set of a messages request (2.1.330), with `class` as `x-claude-code-request-class`.
fn cc_headers(class: &str) -> Vec<(&str, &str)> {
    vec![("anthropic-beta", "oauth-2025-04-20"), ("anthropic-version", "2023-06-01"), ("accept", "application/json"),
         ("user-agent", "claude-cli/2.1.330 (external, cli)"), ("x-app", "cli"), ("x-stainless-lang", "js"),
         ("anthropic-dangerous-direct-browser-access", "true"), ("x-claude-code-session-id", "s1"), ("x-claude-code-request-class", class)]
}

#[test] // FIX-8 (LIV-1 D8): stage 2 and the outbound headers have no branch on the request class (§12.4)
fn auxiliary_request_reaches_upstream_as_main_does() {
    let t = setup("");
    let g = grant(&t, Agent::ClaudeCode, true);
    let b = mb("claude-test-1");
    for class in ["main", "auxiliary"] {
        assert_eq!(post_pt(&t, &g, M, b.clone(), &cc_headers(class)).0, 200, "{class}");
    }
    let got = t.anth.got();
    assert_eq!(got.len(), 2);
    assert!(got.iter().all(|x| x.body == b));
    assert_eq!(got[0].headers, got[1].headers);
    for x in &got {
        let h = &x.headers;
        for n in ["user-agent", "x-app", "x-stainless-lang", "anthropic-dangerous-direct-browser-access"] {
            assert!(h.get(n).is_none(), "{n} sent");
        }
        assert!(h.keys().all(|k| !k.as_str().starts_with("x-claude-code-")));
        assert_eq!((hv(h, "authorization"), hv(h, "anthropic-beta")), (BEARER, "oauth-2025-04-20"));
    }
    eprintln!("upstream header names: {:?}", sorted_names(&got[0].headers));
    assert!(t.h.usage_written(2, Duration::from_secs(10)), "usage rows not written");
    let r = rows(&t, &g);
    assert_eq!(r.iter().map(|w| w.request_class.as_deref()).collect::<Vec<_>>(), [Some("main"), Some("auxiliary")]);
    assert_eq!(r[0].model, r[1].model);
}

#[test] // FIX-8 (LIV-1 D8): a 429 is kept as the last answer and relayed once, as received (§7.3, §12.4)
fn upstream_429_reaches_the_client_once_and_unchanged() {
    let t = setup("");
    let g = grant(&t, Agent::ClaudeCode, true);
    let e = json!({"type":"error","error":{"type":"rate_limit_error","message":"limit"}});
    t.anth.answer(429, &[("retry-after", "7")], &e);
    let r = post_pt(&t, &g, M, mb("claude-test-1"), &cc_headers("auxiliary"));
    assert_eq!((r.0, hv(&r.1, "retry-after")), (429, "7"));
    assert_eq!(serde_json::from_slice::<Value>(&r.2).unwrap(), e);
    assert_eq!(t.anth.got().len(), 1);
    assert!(t.h.usage_written(1, Duration::from_secs(10)), "usage row not written");
    let w = rows(&t, &g);
    assert_eq!(w.len(), 1);
    assert_eq!((w[0].status, w[0].request_class.as_deref()), (429, Some("auxiliary")));
}

#[test] // §12.4 stage 2: no match → built-in provider
fn e2e_builtin_provider_cost_0() {
    let t = setup("");
    let g = grant(&t, Agent::ClaudeCode, true);
    let r = post_pt(&t, &g, M, mb("claude-unknown-9"), &[]);
    assert_eq!((r.0, hv(&r.1, "x-zygo-model")), (200, "claude-unknown-9"));
    let a = t.anth.at("");
    assert_eq!((a.path.as_str(), hv(&a.headers, "authorization")), ("/v1/messages", BEARER));
    assert!(t.up.got().is_empty());
    assert!(t.h.usage_written(1, Duration::from_secs(10)));
    let w = &rows(&t, &g)[0];
    assert_eq!((w.provider.as_str(), w.role_served.as_deref(), w.estimated_usd), ("claude-ai", Some("subscription"), 0.0));
}

#[test] // reading 6: the login goes only to the type's host
fn e2e_host_bound_to_type_host() {
    let t = setup("");
    let g = grant(&t, Agent::ClaudeCode, true);
    let r = post_pt(&t, &g, M, mb("claude-else-1"), &[]);
    assert!(t.up.got().is_empty());
    assert!(!oat_seen(&t.up));
    assert_eq!(r.0, 200, "{:?}", String::from_utf8_lossy(&r.2));
    assert_eq!(t.anth.at("").path, "/v1/messages");
}

#[test] // §20 #26 local part
fn e2e_no_login_refused_nothing_upstream() {
    let t = setup("");
    let g = grant(&t, Agent::ClaudeCode, true);
    let bt = format!("Bearer {}", g.token);
    let cases: Vec<Vec<(&str, &str)>> = vec![
        vec![],
        vec![("x-api-key", OAT)],
        vec![("authorization", BEARER), ("x-api-key", "k")],
        vec![("authorization", bt.as_str())],
        vec![("authorization", "Bearer other")],
    ];
    let mut all = Vec::new();
    for c in cases {
        let mut hd = pt(&g);
        hd.extend(c);
        all.push(send(&t, "POST", M, &hd, Some(mb("claude-test-1"))));
    }
    all.push(send(&t, "POST", M, &[("x-api-key", g.token.as_str()), ("x-zygo-session", g.session.as_str())], Some(mb("claude-test-1"))));
    all.push(send(&t, "POST", M, &[("authorization", bt.as_str()), ("x-zygo-session", g.session.as_str())], Some(mb("claude-test-1"))));
    for r in &all {
        no_login_r(r);
        assert!(!has(&r.2, g.token.as_bytes()));
    }
    assert!(t.up.got().is_empty() && t.anth.got().is_empty());
    final_rows(&t, &g, 1, &[200]);
}

#[test] // §12.4 stage 1: invalid or expired → token_invalid
fn e2e_token_invalid_cases() {
    let t = setup("");
    let g = grant(&t, Agent::ClaudeCode, true);
    let n = grant(&t, Agent::ClaudeCode, false);
    let bad = format!("zyg_s_{}", ulid());
    let other = ulid();
    let post = |hd: &[(&str, &str)]| {
        let mut hd = hd.to_vec();
        hd.push(("authorization", BEARER));
        send(&t, "POST", M, &hd, Some(mb("claude-test-1")))
    };
    token_invalid_r(&post(&[("x-zygo-token", &bad), ("x-zygo-session", &g.session)]));
    token_invalid_r(&post(&[("x-zygo-token", &n.token), ("x-zygo-session", &n.session)]));
    token_invalid_r(&post(&[("x-zygo-token", &g.token), ("x-zygo-session", &other)]));
    token_invalid_r(&post(&[("x-zygo-token", &g.token)]));
    t.c.advance(IDLE_TTL + Duration::from_secs(1));
    token_invalid_r(&post(&pt(&g)));
    assert!(t.up.got().is_empty() && t.anth.got().is_empty());
}

#[test] // §12.5, §7.2 note: an accepted request refreshes the idle TTL, a refused one does not
fn e2e_accepted_request_refreshes_ttl() {
    let t = setup("");
    let g = grant(&t, Agent::ClaudeCode, true);
    t.c.advance(IDLE_TTL - Duration::from_secs(1));
    assert_eq!(post_pt(&t, &g, M, mb("claude-test-1"), &[]).0, 200);
    t.c.advance(Duration::from_secs(2));
    assert_eq!(post_pt(&t, &g, M, mb("claude-test-1"), &[]).0, 200);
    let g2 = grant(&t, Agent::ClaudeCode, true);
    t.c.advance(IDLE_TTL - Duration::from_secs(1));
    no_login_r(&send(&t, "POST", M, &pt(&g2), Some(mb("claude-test-1"))));
    t.c.advance(Duration::from_secs(2));
    token_invalid_r(&post_pt(&t, &g2, M, mb("claude-test-1"), &[]));
}

#[test] // reading 4: an agent with no type row matches no login
fn e2e_opencode_session_no_login() {
    let t = setup("");
    let g = grant(&t, Agent::OpenCode, true);
    no_login_r(&post_pt(&t, &g, M, mb("claude-test-1"), &[]));
    assert!(t.up.got().is_empty() && t.anth.got().is_empty());
}

#[test] // reading 1: GET /anthropic/v1/models runs the passthrough identity
fn e2e_models_route_passthrough_identity() {
    let t = setup("");
    let g = grant(&t, Agent::ClaudeCode, true);
    let mut hd = pt(&g);
    hd.push(("authorization", BEARER));
    let r = send(&t, "GET", MODELS, &hd, None);
    assert_eq!(r.0, 200);
    assert!(serde_json::from_slice::<Value>(&r.2).unwrap()["data"].is_array());
    no_login_r(&send(&t, "GET", MODELS, &pt(&g), None));
}

#[test] // a normal session is unchanged: identity from x-api-key, provider key sent, no client authorization
fn e2e_normal_session_unchanged() {
    let t = setup("");
    let g = grant(&t, Agent::ClaudeCode, false);
    let hd = [("x-api-key", g.token.as_str()), ("x-zygo-session", g.session.as_str()), ("authorization", BEARER)];
    let r = send(&t, "POST", M, &hd, Some(mb("code")));
    assert_eq!((r.0, hv(&r.1, "x-zygo-role")), (200, "code"));
    let u = t.up.at("/same/v1/messages");
    assert_eq!(hv(&u.headers, "x-api-key"), "test-key-same");
    assert!(u.headers.get("authorization").is_none());
}

// ---- coverage ----

#[test] // §20 #25: `[1m]` sent as received; count_tokens on the same path
fn cover_one_m_and_count_tokens() {
    let t = setup("");
    let g = grant(&t, Agent::ClaudeCode, true);
    let b = mb("claude-test-1[1m]");
    let r = post_pt(&t, &g, M, b.clone(), &[]);
    assert_eq!((r.0, hv(&r.1, "x-zygo-model")), (200, "test-pt"));
    assert_eq!(t.anth.at("/v1/messages").body, b);
    let r = post_pt(&t, &g, CT, br#"{"model":"claude-test-1","messages":[]}"#.to_vec(), &[]);
    assert_eq!(r.0, 200);
    let a = t.anth.got().pop().unwrap();
    assert_eq!((a.path.as_str(), hv(&a.headers, "authorization")), ("/v1/messages/count_tokens", BEARER));
}

#[test] // §12.4 stage 5: no key is read; `enabled = false` still applies
fn cover_disabled_provider_and_no_key_read() {
    let t = setup("");
    let g = grant(&t, Agent::ClaudeCode, true);
    let n0 = t.keys.calls.load(Ordering::SeqCst);
    assert_eq!(post_pt(&t, &g, M, mb("claude-test-1"), &[]).0, 200);
    assert_eq!(t.keys.calls.load(Ordering::SeqCst), n0);
    let before = t.anth.got().len();
    assert_eq!(st(&post_pt(&t, &g, M, mb("claude-off-1"), &[])), (503, "zygo_provider_unavailable".to_string()));
    assert_eq!(t.anth.got().len(), before);
    let n = grant(&t, Agent::ClaudeCode, false);
    let r = send(&t, "POST", M, &[("x-api-key", n.token.as_str()), ("x-zygo-session", n.session.as_str())], Some(mb("code")));
    assert_eq!(r.0, 200);
    assert_eq!(t.keys.calls.load(Ordering::SeqCst), n0 + 1);
}

#[test] // §20 #27: the secret pass runs on a passthrough request
fn cover_secret_redaction_on_passthrough() {
    let t = setup("");
    let g = grant(&t, Agent::ClaudeCode, true);
    let key = key_sample();
    let b = json!({"model": "claude-test-1", "max_tokens": 8,
                   "messages": [{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":format!("env: {key} end")}]}]});
    let r = post_pt(&t, &g, M, serde_json::to_vec(&b).unwrap(), &[]);
    assert_eq!((r.0, hv(&r.1, "x-zygo-redacted")), (200, "1"));
    let a = t.anth.at("/v1/messages");
    let v: Value = serde_json::from_slice(&a.body).unwrap();
    assert_eq!((v["messages"][0]["content"][0]["content"].as_str(), v["model"].as_str()),
               (Some("env: [REDACTED:anthropic_api_key] end"), Some("claude-test-1")));
    assert!(!has(&a.body, key.as_bytes()));
    assert_eq!(hv(&a.headers, "authorization"), BEARER);
    assert!(t.h.usage_written(1, Duration::from_secs(10)));
    assert_eq!(rows(&t, &g)[0].redactions, vec![RedactionCount { id: "anthropic_api_key".into(), count: 1 }]);
}

#[test] // §7.3 note (a) on passthrough
fn cover_repeated_key_serialized() {
    let t = setup("");
    let g = grant(&t, Agent::ClaudeCode, true);
    let key = key_sample();
    let raw = format!(r#"{{"model":"claude-test-1","max_tokens":8,"messages":[{{"role":"user","content":"{key}"}}],"messages":[{{"role":"user","content":"hi"}}]}}"#);
    assert_eq!(post_pt(&t, &g, M, raw.into_bytes(), &[]).0, 200);
    let a = t.anth.at("/v1/messages");
    assert!(!has(&a.body, key.as_bytes()));
    assert_eq!(a.body, br#"{"model":"claude-test-1","max_tokens":8,"messages":[{"role":"user","content":"hi"}]}"#.to_vec());
}

#[test] // §7.3 note (a), C4 (round 2 B1): passthrough refuses a serde_json private key 400; nothing reaches the vendor
fn cover_reserved_key_refused_400() {
    let t = setup("");
    let g = grant(&t, Agent::ClaudeCode, true);
    let key = key_sample();
    let raw = format!(r#"{{"model":"claude-test-1","max_tokens":8,"messages":[{{"role":"user","content":"hi"}}],"metadata":{{"$serde_json::private::RawValue":"{{\"user_id\":\"{key}\",\"user_id\":\"u\"}}"}}}}"#);
    let before = t.anth.got().len();
    assert_eq!(st(&post_pt(&t, &g, M, raw.into_bytes(), &[])), (400, "zygo_bad_request".to_string()));
    assert_eq!(t.anth.got().len(), before);
}

#[test] // §7.3 note (b), C4: the redacted name goes upstream
fn cover_model_secret_redacted_upstream() {
    let t = setup("");
    let g = grant(&t, Agent::ClaudeCode, true);
    let pw = ["pass", "word=Abcdefghij", "klmnop1"].concat();
    let r = post_pt(&t, &g, M, mb(&pw), &[]);
    assert_eq!((r.0, hv(&r.1, "x-zygo-redacted"), hv(&r.1, "x-zygo-model")), (200, "1", "password=[REDACTED:generic_secret_assignment]"));
    let a = t.anth.at("/v1/messages");
    assert!(!has(&a.body, pw.as_bytes()));
    assert_eq!(a.body, mb("password=[REDACTED:generic_secret_assignment]"));
    assert!(t.h.usage_written(1, Duration::from_secs(10)));
    assert_eq!(rows(&t, &g)[0].redactions, vec![RedactionCount { id: "generic_secret_assignment".into(), count: 1 }]);
}

#[test] // §7.3 note (b), §14, C3: the row's model holds the redacted name; a blocked name leaves it empty
fn cover_model_secret_not_in_row() {
    let t = setup("");
    let g = grant(&t, Agent::ClaudeCode, true);
    let pw = ["pass", "word=Abcdefghij", "klmnop1"].concat();
    assert_eq!(post_pt(&t, &g, M, mb(&format!("{pw}[1m]")), &[]).0, 200);
    let b = grant(&t, Agent::ClaudeCode, true);
    let blocked = format!("TRACERBLOCK_{}", 123456);
    assert_eq!(st(&post_pt(&t, &b, M, mb(&blocked), &[])), (422, "zygo_redaction_block".to_string()));
    assert!(t.h.usage_written(2, Duration::from_secs(10)));
    let w = &rows(&t, &g)[0];
    assert_eq!((w.model.as_str(), w.role_requested.as_deref()), ("password=[REDACTED:generic_secret_assignment]", Some("password=[REDACTED:generic_secret_assignment]")));
    let w = &rows(&t, &b)[0];
    assert_eq!((w.model.as_str(), w.role_requested.as_deref(), w.status), ("", None, 422));
    assert!(read_all(t.d.path()).iter().all(|f| !has(f, pw.as_bytes()) && !has(f, blocked.as_bytes())));
}

#[test] // reading 8: require_zdr still applies
fn cover_require_zdr_applies() {
    let t = setup("[proxy]\nrequire_zdr = true\n");
    let g = grant(&t, Agent::ClaudeCode, true);
    for m in ["claude-test-1", "claude-unknown-9"] {
        assert_eq!(st(&post_pt(&t, &g, M, mb(m), &[])), (503, "zygo_provider_unavailable".to_string()), "{m}");
    }
    assert!(t.up.got().is_empty() && t.anth.got().is_empty());
}

#[test] // C3: the login and the session token reach no file, response or usage row
fn cover_credential_never_written() {
    let t = setup("");
    let g = grant(&t, Agent::ClaudeCode, true);
    let bad = format!("zyg_s_{}", ulid());
    let block = serde_json::to_vec(&json!({"model": "claude-test-1", "max_tokens": 8,
                                           "messages": [{"role":"user","content": format!("TRACERBLOCK_{}", 123456)}]})).unwrap();
    let mut resp = vec![
        post_pt(&t, &g, M, mb("claude-test-1"), &[]),
        post_pt(&t, &g, CT, br#"{"model":"claude-test-1","messages":[]}"#.to_vec(), &[]),
        send(&t, "POST", M, &[("x-zygo-token", bad.as_str()), ("x-zygo-session", g.session.as_str()), ("authorization", BEARER)], Some(mb("claude-test-1"))),
        post_pt(&t, &g, M, mb("claude-test-1"), &[("x-api-key", OAT)]),
        post_pt(&t, &g, M, block, &[]),
        post_pt(&t, &g, M, mb("claude-off-1"), &[]),
    ];
    assert_eq!(resp.iter().map(|r| r.0).collect::<Vec<_>>(), [200, 200, 401, 401, 422, 503]);
    let (rows, last) = final_rows(&t, &g, 5, &[200, 200, 422, 503, 200]);
    resp.push(last);
    let mut out = Vec::new();
    for (_, h, b) in &resp {
        for (k, v) in h {
            out.extend_from_slice(k.as_str().as_bytes());
            out.extend_from_slice(v.as_bytes());
        }
        out.extend_from_slice(b);
    }
    let files = read_all(t.d.path());
    let rows_text = format!("{rows:?}");
    for needle in [OAT, g.token.as_str()] {
        assert!(files.iter().all(|f| !has(f, needle.as_bytes())), "file");
        assert!(!has(&out, needle.as_bytes()), "response");
        assert!(!rows_text.contains(needle), "usage row");
    }
}


#[test] // §7.3: the production stage list (`Pipeline::new`) runs stage 2 (`PassthroughStage`) and stage 6 (`RedactStage`)
fn unit_production_pipeline_passthrough_and_redaction() {
    let t = setup("");
    let g = grant(&t, Agent::ClaudeCode, true);
    let p = Pipeline::new(t.keys.clone());
    let mut c = ctx(&g, &t, json!({"model": "claude-unknown-9", "max_tokens": 8, "messages": [{"role":"user","content":"hi"}]}));
    p.on_request(&mut c).unwrap();
    let r = c.resolved.as_ref().unwrap();
    assert_eq!((r.role.as_str(), r.model.as_str(), c.candidates.len()), ("subscription", "claude-unknown-9", 1));
    assert_eq!((&c.candidates[0].provider, c.candidates[0].key.is_none()), (&CLAUDE_AI.builtin_provider("claude-unknown-9"), true));
    assert!(!c.redacted && !c.changed);
    let key = key_sample();
    let mut c = ctx(&g, &t, json!({"model": "claude-unknown-9", "max_tokens": 8, "messages": [{"role":"user","content": format!("env: {key} end")}]}));
    p.on_request(&mut c).unwrap();
    assert_eq!(c.resolved.as_ref().unwrap().role, "subscription");
    assert!(c.redacted && c.changed);
    assert_eq!(c.body["messages"][0]["content"], "env: [REDACTED:anthropic_api_key] end");
    assert_eq!(c.redactions.get("anthropic_api_key"), Some(&1));
}

/// POST `M` with header values given as `HeaderValue` (empty or non-UTF-8 values).
fn send_hv(t: &T, headers: Vec<(&str, http::HeaderValue)>, body: Vec<u8>) -> (u16, HeaderMap, Vec<u8>) {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let c = reqwest::Client::builder().no_proxy().build().unwrap();
        let mut rb = c.post(t.h.url().join(M).unwrap()).header("content-type", "application/json");
        for (k, v) in headers {
            rb = rb.header(k, v);
        }
        let r = rb.body(body).send().await.unwrap();
        let (s, hd) = (r.status().as_u16(), r.headers().clone());
        (s, hd, r.bytes().await.unwrap().to_vec())
    })
}

#[test] // §12.4 stage 1: a present X-Zygo-Token decides identity, even when empty or not UTF-8
fn e2e_present_token_empty_or_not_utf8() {
    let t = setup("");
    let g = grant(&t, Agent::ClaudeCode, true);
    let n = grant(&t, Agent::ClaudeCode, false);
    let v = |s: &str| http::HeaderValue::from_str(s).unwrap();
    let not_utf8 = http::HeaderValue::from_bytes(&[b"zyg_s_".as_slice(), &[0xff]].concat()).unwrap();
    token_invalid_r(&send_hv(&t, vec![("x-zygo-token", v("")), ("x-zygo-session", v(&g.session)), ("authorization", v(BEARER))], mb("claude-test-1")));
    token_invalid_r(&send_hv(&t, vec![("x-zygo-token", not_utf8), ("x-zygo-session", v(&g.session)), ("authorization", v(BEARER))], mb("claude-test-1")));
    token_invalid_r(&send_hv(&t, vec![("x-zygo-token", v("")), ("x-zygo-session", v(&n.session)), ("x-api-key", v(&n.token))], mb("code")));
    assert!(t.up.got().is_empty() && t.anth.got().is_empty());
}

#[test] // §12.2: only `Bearer ` + one token that starts with the prefix (mutant B)
fn e2e_login_value_exact() {
    let t = setup("");
    let g = grant(&t, Agent::ClaudeCode, true);
    let p = CLAUDE_AI.prefix;
    for a in [format!("Bearer {OAT}, Bearer evil"), format!("Bearer  {OAT}"), format!("Bearer x-{OAT}"), format!("Bearer {p}")] {
        let mut hd = pt(&g);
        hd.push(("authorization", a.as_str()));
        no_login_r(&send(&t, "POST", M, &hd, Some(mb("claude-test-1"))));
    }
    assert!(t.up.got().is_empty() && t.anth.got().is_empty());
    assert_eq!(post_pt(&t, &g, M, mb("claude-test-1"), &[]).0, 200);
}

#[test] // §12.2, reading 6: the type's host on another port is not the type's host
fn e2e_host_other_port_gets_no_login() {
    let up2 = Upstream::start();
    let t = setup(&model("test-port", &[prov("p-port", &format!("http://api.anthropic.com:{}/port", up2.port()), "claude-port-1", "")]));
    let g = grant(&t, Agent::ClaudeCode, true);
    let r = post_pt(&t, &g, M, mb("claude-port-1"), &[]);
    assert!(up2.got().is_empty() && !oat_seen(&up2));
    assert_eq!((r.0, hv(&r.1, "x-zygo-model")), (200, "test-port"));
    let a = t.anth.at("");
    assert_eq!((a.path.as_str(), hv(&a.headers, "authorization")), ("/v1/messages", BEARER));
}

/// Stage 2 through the test seam, then the candidate moved to `to` (a stage that yields an off-host candidate).
struct OffHost { to: String }
impl Stage for OffHost {
    fn on_request(&self, ctx: &mut RequestCtx) -> Result<(), ZygoError> {
        PassthroughStage::with_test_hosts(vec!["api.anthropic.com".into()]).on_request(ctx)?;
        ctx.candidates[0].provider.base_url = self.to.clone();
        Ok(())
    }
}

#[test] // reading 6: the proxy checks the host again before it sends the login (mutant A)
fn e2e_forward_refuses_off_host_candidate() {
    let up2 = Upstream::start();
    let stage = OffHost { to: format!("http://127.0.0.1:{}/evil", up2.port()) };
    let t = setup_with("", Some(Pipeline { stages: vec![Arc::new(stage)] }));
    let g = grant(&t, Agent::ClaudeCode, true);
    assert_eq!(st(&post_pt(&t, &g, M, mb("claude-unknown-9"), &[])), (503, "zygo_provider_unavailable".to_string()));
    assert!(up2.got().is_empty() && !oat_seen(&up2));
    assert!(t.anth.got().is_empty());
}

// ---- docs ----

#[test] // §12.1 README risk text; §12.2, §12.4, §12.5 documented
fn docs_readme_and_passthrough_md() {
    let readme = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../README.md")).unwrap();
    for s in ["--passthrough", "consumer terms", "third-party software", "your own account", "docs/passthrough.md"] {
        assert!(readme.contains(s), "README.md: {s}");
    }
    let doc = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/passthrough.md")).unwrap();
    for s in ["claude-ai", "api.anthropic.com", "sk-ant-oat", "X-Zygo-Token", "zygo_passthrough_token_invalid", "zygo_passthrough_no_login",
              "zygo_passthrough_local_only", "24 h", "/model", "consumer terms"] {
        assert!(doc.contains(s), "docs/passthrough.md: {s}");
    }
}

#[test] // AU29, H§7.2 `model`: a name the secret pass blocks is "" in the row; the status then has no model
fn status_no_model_for_blocked_name() {
    let t = setup("\n[features.redaction]\nsecrets = false\n");
    let g = grant(&t, Agent::ClaudeCode, true);
    let status = |n: u64| {
        assert!(t.h.usage_written(n, Duration::from_secs(10)));
        let s = t.h.session(&g.session).unwrap().status();
        (s.model, s.context_window, s.effective_window, s.threshold)
    };
    assert_eq!(post_pt(&t, &g, M, mb("claude-test-1"), &[]).0, 200);
    assert_eq!(status(1), (Some("test-pt".into()), None, None, None)); // the catalog match; test-pt has no facts
    let blocked = format!("TRACERBLOCK_{}", 123456);
    assert_eq!(post_pt(&t, &g, M, mb(&blocked), &[]).0, 200); // the secret pass is off: the request is served
    assert_eq!(status(2), (None, None, None, None));
    assert!(rows(&t, &g).iter().any(|r| r.model.is_empty() && r.status == 200));
}

// ---- HUD-6: session spend and tokens ----

/// (tokens_in, tokens_out, spend_usd, requests_total, requests_priced).
type Spend = (Option<u64>, Option<u64>, Option<f64>, Option<u64>, Option<u64>);

/// The HUD-6 status fields of `g` once `n` usage rows are handled.
fn spend(t: &T, g: &SessionGrant, n: u64) -> Spend {
    assert!(t.h.usage_written(n, Duration::from_secs(10)));
    let s = t.h.session(&g.session).unwrap().status();
    (s.tokens_in, s.tokens_out, s.spend_usd, s.requests_total, s.requests_priced)
}

#[test] // H§7.3 step 4, AU15, AU16, §14: passthrough rows cost 0; the session spend is notional
fn e2e_passthrough_notional_spend() {
    let t = setup("");
    let g = grant(&t, Agent::ClaudeCode, true);
    let s = t.h.session(&g.session).unwrap().status();
    assert_eq!((s.tokens_in, s.tokens_out, s.spend_usd, s.requests_total, s.requests_priced), (Some(0), Some(0), Some(0.0), Some(0), Some(0)));
    assert_eq!((s.spend_notional, s.passthrough, s.team_session), (Some(true), Some(true), Some(false)));
    // test-pt: p-other (first, no price), then p-anth (4 / 20 / 0.2 / 8): the first provider on the dialect with a price.
    assert_eq!(post_pt(&t, &g, M, mb("claude-test-1"), &[]).0, 200);
    let one = (11.0 * 4.0 + 22.0 * 20.0) / 1e6;
    assert_eq!(spend(&t, &g, 1), (Some(11), Some(22), Some(one), Some(1), Some(1)));
    // `[1m]` is not part of the matched name.
    assert_eq!(post_pt(&t, &g, M, mb("claude-test-1[1m]"), &[]).0, 200);
    assert_eq!(spend(&t, &g, 2), (Some(22), Some(44), Some(one + one), Some(2), Some(2)));
    // No catalog match: counted, not priced.
    assert_eq!(post_pt(&t, &g, M, mb("claude-unknown-9"), &[]).0, 200);
    assert_eq!(spend(&t, &g, 3), (Some(33), Some(66), Some(one + one), Some(3), Some(2)));
    // A match with no priced provider: counted, not priced.
    assert_eq!(post_pt(&t, &g, M, mb("claude-else-1"), &[]).0, 200);
    assert_eq!(spend(&t, &g, 4), (Some(44), Some(88), Some(one + one), Some(4), Some(2)));
    // A request with no token (refused before any upstream) is in neither count (AU15).
    assert_eq!(post_pt(&t, &g, M, br#"{"model":5}"#.to_vec(), &[]).0, 400);
    assert_eq!(spend(&t, &g, 5), (Some(44), Some(88), Some(one + one), Some(4), Some(2)));
    let rows = rows(&t, &g);
    assert_eq!(rows.len(), 5);
    assert!(rows.iter().all(|r| r.estimated_usd == 0.0 && r.passthrough), "§14: a passthrough row costs 0");
    assert_eq!(t.h.session(&g.session).unwrap().status().spend_notional, Some(true));
}

#[test] // AU5, H§7.3 step 3, AU33: the row cost and the session spend are one `Prices::cost` result
fn e2e_local_spend_is_the_row_cost() {
    let priced = "key = \"keychain:k-same\"\n[model.provider.price_usd_per_mtok]\ninput = 4.0\noutput = 20.0\ncache_read = 0.2\ncache_write = 8.0\ncache_write_5m = 5.0\nlong_context = { above = 5000, input = 40.0, output = 200.0, cache_read = 2.0, cache_write = 80.0 }\n";
    let t = setup(&model("test-priced", &[prov("p-priced", "{UP}/priced", "up-priced", priced)]));
    let g = grant(&t, Agent::ClaudeCode, false);
    let hd = [("x-api-key", g.token.as_str()), ("x-zygo-session", g.session.as_str())];
    let s = t.h.session(&g.session).unwrap().status();
    assert_eq!((s.spend_usd, s.spend_notional, s.passthrough, s.team_session), (Some(0.0), Some(false), Some(false), Some(false)));
    // The split is read: 200 five-minute tokens at 5.0, 100 one-hour tokens at 8.0.
    t.up.reply(&json!({"id":"m","type":"message","role":"assistant","content":[],"usage":{"input_tokens":100,"output_tokens":10,
        "cache_read_input_tokens":1000,"cache_creation_input_tokens":300,"cache_creation":{"ephemeral_5m_input_tokens":200,"ephemeral_1h_input_tokens":100}}}));
    assert_eq!(send(&t, "POST", M, &hd, Some(mb("test-priced"))).0, 200);
    let one = (100.0 * 4.0 + 10.0 * 20.0 + 1000.0 * 0.2 + 200.0 * 5.0 + 100.0 * 8.0) / 1e6;
    assert_eq!(spend(&t, &g, 1), (Some(1400), Some(10), Some(one), Some(1), Some(1)));
    let w = rows(&t, &g);
    assert_eq!((w[0].estimated_usd, w[0].cache_write_tokens, w[0].provider.as_str()), (one, 300, "p-priced"));
    // No split: every write at `cache_write`; a prompt above `long_context.above` takes the tier (H§7.3 step 1).
    t.up.reply(&json!({"id":"m","type":"message","role":"assistant","content":[],"usage":{"input_tokens":5000,"output_tokens":1,
        "cache_read_input_tokens":0,"cache_creation_input_tokens":10}}));
    assert_eq!(send(&t, "POST", M, &hd, Some(mb("test-priced"))).0, 200);
    let two = (5000.0 * 40.0 + 200.0 + 10.0 * 80.0) / 1e6;
    assert_eq!(spend(&t, &g, 2), (Some(1400 + 5010), Some(11), Some(one + two), Some(2), Some(2)));
    let w = rows(&t, &g);
    assert_eq!(w.iter().map(|r| r.estimated_usd).sum::<f64>(), one + two);
    // A route with no price: counted, not priced, row cost 0.
    assert_eq!(send(&t, "POST", M, &hd, Some(mb("code"))).0, 200);
    assert_eq!(spend(&t, &g, 3), (Some(1400 + 5010 + 5010), Some(12), Some(one + two), Some(3), Some(2)));
    // A reply with no token: in neither count (AU15).
    t.up.reply(&json!({"id":"m","type":"message","role":"assistant","content":[],"usage":{"input_tokens":0,"output_tokens":0}}));
    assert_eq!(send(&t, "POST", M, &hd, Some(mb("test-priced"))).0, 200);
    assert_eq!(spend(&t, &g, 4), (Some(1400 + 5010 + 5010), Some(12), Some(one + two), Some(3), Some(2)));
}

// ---- HUD-9: H§14 #14, proxy part ----

#[test] // H§14 #14 (proxy part): H§7.3 step 4, AU15; the status reply over HTTP
fn h14_14_passthrough_rows_cost_zero_and_the_status_spend_is_notional() {
    let t = setup("");
    let g = grant(&t, Agent::ClaudeCode, true);
    // The reply of a status request of the session, parsed as the session client parses it.
    let reply = |n: u64| -> SessionStatus {
        assert!(t.h.usage_written(n, Duration::from_secs(10)), "usage rows not written");
        let hd = [("x-api-key", g.token.as_str()), ("x-zygo-session", g.session.as_str())];
        let r = send(&t, "GET", &format!("v1/zygo/sessions/{}/status", g.session), &hd, None);
        assert_eq!(r.0, 200, "{}", String::from_utf8_lossy(&r.2));
        serde_json::from_slice(&r.2).unwrap()
    };
    // Every token kind in the upstream usage, so the notional cost uses each of the four rates.
    t.anth.reply(&json!({"id":"m","type":"message","role":"assistant","content":[],"usage":{"input_tokens":100,"output_tokens":10,
        "cache_read_input_tokens":1000,"cache_creation_input_tokens":300}}));
    assert_eq!(post_pt(&t, &g, M, mb("claude-test-1"), &[]).0, 200);
    assert_eq!(post_pt(&t, &g, M, mb("claude-test-1"), &[]).0, 200);
    let s = reply(2);
    // The rows: the matched catalog row `test-pt` on `p-anth`, which has a price; the row cost is 0 (§14).
    let w = rows(&t, &g);
    assert_eq!(w.len(), 2);
    for r in &w {
        assert_eq!((r.model.as_str(), r.provider.as_str(), r.passthrough, r.status), ("test-pt", "p-anth", true, 200));
        assert_eq!((r.input_tokens, r.output_tokens, r.cache_read_tokens, r.cache_write_tokens), (100, 10, 1000, 300));
        assert_eq!(r.estimated_usd, 0.0);
    }
    // The notional cost: the tokens × the price of `p-anth` (4 / 20 / 0.2 / 8 USD per million tokens), per request.
    let one = (100.0 * 4.0 + 10.0 * 20.0 + 1000.0 * 0.2 + 300.0 * 8.0) / 1e6;
    assert_eq!(t.h.session(&g.session).unwrap().status().spend_usd, Some(one + one));
    assert!((s.spend_usd.unwrap() - 2.0 * one).abs() < 1e-12, "{:?} != {}", s.spend_usd, 2.0 * one); // a JSON number
    assert_eq!((s.spend_notional, s.passthrough, s.team_session), (Some(true), Some(true), Some(false)));
    assert_eq!((s.requests_total, s.requests_priced), (Some(2), Some(2)));
    assert_eq!((s.tokens_in, s.tokens_out), (Some(2 * 1400), Some(20)));
    // A session that is not passthrough, same proxy: `spend_notional` is false.
    let n = grant(&t, Agent::ClaudeCode, false);
    assert_eq!(t.h.session(&n.session).unwrap().status().spend_notional, Some(false));
}
