//! N15: soft context window, triggers and the usage rewrite (§10.2, §20 #15, #16, #16a).

use std::cell::Cell;
use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Body;
use axum::extract::Request;
use axum::response::Response;
use bytes::Bytes;
use http::HeaderMap;
use serde_json::{json, Value};
use tempfile::TempDir;
use zygo_config::keys::Secret;
use zygo_config::{load, KeyRef, LoadContext, Merged, ModelRow};
use zygo_core::ids::ulid;
use zygo_core::paths::Paths;
use zygo_core::Agent;
use zygo_proxy::provider::KeyResolver;
use zygo_proxy::redact_stage::RedactLimits;
use zygo_proxy::server::StartOpts;
use zygo_proxy::session::{ManualClock, SessionGrant, SessionRequest, WindowArg};
use zygo_proxy::tee::{Usage, HOLD_MAX};
use zygo_proxy::translate::SseEvent;
use zygo_proxy::window::{
    agent_window, compact_tool, context_window, one_m, prompt, rewrite_usage, safety, soft_threshold, window, ONE_M_WINDOW,
};
use zygo_proxy::{Proxy, ProxyHandle};
use zygo_redact::DeadlinePolicy;
use zygo_usage::{RequestRow, Store};

// ---- Harness ----

const M: &str = "anthropic/v1/messages";
/// §20 #25 value; not credential-shaped (two digits must follow `oat`).
const OAT_BEARER: &str = "Bearer sk-ant-oat-x";

fn fixture(name: &str) -> String {
    let p = format!("{}/../../fixtures/claude-code/sse/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(p).unwrap()
}

/// `"event: <t>\ndata: <json>\n\n"` per event, with `"type": <t>` in the data.
fn sse(events: &[(&str, Value)]) -> String {
    events
        .iter()
        .map(|(t, d)| {
            let mut d = d.clone();
            d["type"] = json!(t);
            format!("event: {t}\ndata: {d}\n\n")
        })
        .collect()
}

#[derive(Clone)]
struct Reply {
    status: u16,
    ctype: &'static str,
    body: String,
    /// Chunk size; 0 = three chunks at byte thirds.
    chunk: usize,
    /// One `Body::from(bytes)`: the upstream sends `content-length`.
    cl: bool,
    /// After the body, a `ping` event every 50 ms, never ending.
    pings: bool,
}

fn sse_reply(s: &str) -> Reply {
    Reply { status: 200, ctype: "text/event-stream", body: s.into(), chunk: 0, cl: false, pings: false }
}

fn json_reply(v: Value) -> Reply {
    Reply { status: 200, ctype: "application/json", body: v.to_string(), chunk: 0, cl: false, pings: false }
}

#[derive(Clone)]
struct Got {
    path: String,
    body: Vec<u8>,
}

/// Path only.
impl fmt::Debug for Got {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Got").field("path", &self.path).finish()
    }
}

#[derive(Default)]
struct UpState {
    got: Mutex<Vec<Got>>,
    script: Mutex<HashMap<String, VecDeque<Reply>>>,
}

/// A fake upstream on 127.0.0.1:0 with its own runtime thread; records each request.
struct Upstream {
    addr: SocketAddr,
    st: Arc<UpState>,
    _stop: tokio::sync::oneshot::Sender<()>,
}

/// "/a/v1/messages" → "/a", "/v1/messages" → "".
fn prefix(path: &str) -> String {
    if path.starts_with("/v1/") {
        return String::new();
    }
    let rest = &path[1..];
    format!("/{}", &rest[..rest.find('/').unwrap_or(rest.len())])
}

/// The scripted reply for the path's prefix.
async fn upstream_handler(st: Arc<UpState>, req: Request) -> Response {
    let path = req.uri().path_and_query().map(|p| p.as_str().to_string()).unwrap_or_default();
    let body = axum::body::to_bytes(req.into_body(), usize::MAX).await.unwrap().to_vec();
    let p = prefix(&path);
    st.got.lock().unwrap().push(Got { path, body });
    let r = st.script.lock().unwrap().get_mut(&p).and_then(|q| q.pop_front()).unwrap_or_else(|| panic!("no reply at {p:?}"));
    let b = r.body.into_bytes();
    let body = if r.cl {
        Body::from(b)
    } else {
        let parts: Vec<Result<Bytes, std::io::Error>> = if r.chunk == 0 {
            let (a, c) = (b.len() / 3, 2 * b.len() / 3);
            [&b[..a], &b[a..c], &b[c..]].iter().map(|s| Ok(Bytes::copy_from_slice(s))).collect()
        } else {
            b.chunks(r.chunk).map(|s| Ok(Bytes::copy_from_slice(s))).collect()
        };
        let parts = futures_util::stream::iter(parts);
        if r.pings {
            Body::from_stream(futures_util::StreamExt::chain(parts, futures_util::stream::unfold((), |()| async {
                tokio::time::sleep(Duration::from_millis(50)).await;
                Some((Ok::<_, std::io::Error>(Bytes::from_static(b"event: ping\ndata: {\"type\":\"ping\"}\n\n")), ()))
            })))
        } else {
            Body::from_stream(parts)
        }
    };
    Response::builder().status(r.status).header("content-type", r.ctype).body(body).unwrap()
}

impl Upstream {
    fn start() -> Upstream {
        let st = Arc::new(UpState::default());
        let l = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        l.set_nonblocking(true).unwrap();
        let addr = l.local_addr().unwrap();
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let s2 = st.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
            rt.block_on(async move {
                let l = tokio::net::TcpListener::from_std(l).unwrap();
                let app = axum::Router::new().fallback(move |req: Request| upstream_handler(s2.clone(), req));
                tokio::select! {
                    _ = async { axum::serve(l, app).await } => {}
                    _ = rx => {}
                }
            });
        });
        Upstream { addr, st, _stop: tx }
    }
    fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.addr.port())
    }
    fn script(&self, prefix: &str, r: Vec<Reply>) {
        self.st.script.lock().unwrap().insert(prefix.to_string(), r.into());
    }
    fn got(&self) -> Vec<Got> {
        self.st.got.lock().unwrap().clone()
    }
    /// The last request under `p`.
    fn at(&self, p: &str) -> Got {
        self.got().into_iter().rfind(|g| prefix(&g.path) == p).unwrap_or_else(|| panic!("no request at {p:?}"))
    }
}

struct TestKeys(HashMap<String, Secret>);

impl KeyResolver for TestKeys {
    fn resolve(&self, r: &KeyRef) -> Option<Secret> {
        match r {
            KeyRef::Keychain(n) => self.0.get(n).cloned(),
            KeyRef::Env(_) => None,
        }
    }
}

/// Built at run time (no credential-shaped literal).
fn keys() -> TestKeys {
    TestKeys(["k-oai"].iter().map(|n| (n.to_string(), Secret::new(format!("test-key-{n}-{}", ulid())))).collect())
}

/// One catalog model: `soft`, `context_window` facts (none for 0), one provider at `url` whose upstream model is the id.
fn model(id: &str, soft: Option<u64>, cw: u64, url: &str, openai: bool) -> String {
    let soft = soft.map(|s| format!("soft_context_window = {s}\n")).unwrap_or_default();
    let facts = if cw > 0 { format!("[model.facts]\ncontext_window = {cw}\nmax_output_tokens = 128000\n\n") } else { String::new() };
    let (dialects, key) = if openai { ("[\"openai\"]", "key = \"keychain:k-oai\"\n") } else { ("[\"anthropic\"]", "") };
    format!(
        "[[model]]\nid = \"{id}\"\n{soft}\n{facts}[[model.provider]]\nname = \"p-{id}\"\nbase_url = \"{url}\"\ndialects = {dialects}\nupstream_model = \"{id}\"\n{key}\n"
    )
}

fn config(u: &str, lead: &str) -> String {
    let mut t = String::from("[proxy]\nstrict_schemas = true\n\n");
    for (role, m) in [("lead", lead), ("plan", "claude-ns"), ("triage", "claude-big"), ("code", "gpt-t-1"), ("review", "gpt-t-2"), ("read", "n-t-1")] {
        t += &format!("[role.{role}]\nmodel = \"{m}\"\n\n");
    }
    t += &model("claude-t-1", Some(150_000), 200_000, &format!("{u}/c"), false);
    t += &model("claude-ns", None, 200_000, &format!("{u}/ns"), false);
    t += &model("claude-big", Some(900_000), 2_000_000, &format!("{u}/b"), false);
    t += &model("gpt-t-1", None, 400_000, &format!("{u}/g"), false);
    t += &model("gpt-t-2", None, 400_000, &format!("{u}/o"), true);
    t += &model("n-t-1", None, 0, &format!("{u}/n"), false);
    t
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

fn cfg(d: &TempDir, toml: &str) -> Merged {
    std::fs::write(d.path().join("config.toml"), toml).unwrap();
    load(&LoadContext { paths: paths(d), repo: None, team: None }).unwrap()
}

/// A catalog row of the test config.
fn row(id: &str) -> ModelRow {
    let d = TempDir::new().unwrap();
    cfg(&d, &config("http://127.0.0.1:9", "claude-t-1")).config.catalog.get(id).unwrap().clone()
}

/// A pass that must finish never hits its deadline.
fn generous() -> RedactLimits {
    RedactLimits {
        secret: DeadlinePolicy { base: Duration::from_secs(60), per_mib: Duration::ZERO, expire_after_scans: None },
        content_budget: Duration::from_secs(60),
        ..Default::default()
    }
}

struct T {
    d: TempDir,
    up: Upstream,
    anth: Upstream,
    h: ProxyHandle,
    /// Model-route requests sent (one usage row each).
    n: Cell<u64>,
}

fn setup(lead: &str) -> T {
    let d = TempDir::new().unwrap();
    let (up, anth) = (Upstream::start(), Upstream::start());
    let o = StartOpts {
        clock: ManualClock::new(),
        keys: Arc::new(keys()),
        usage_db: Some(d.path().join("data/usage.db")),
        upstream_test: Some(vec![("api.anthropic.com".into(), anth.addr)]),
        redact_limits: Some(generous()),
        ..Default::default()
    };
    let h = Proxy::start_local_with(Arc::new(cfg(&d, &config(&up.url(), lead))), o).unwrap();
    T { d, up, anth, h, n: Cell::new(0) }
}

fn grant_with(t: &T, agent: Agent, passthrough: bool) -> SessionGrant {
    t.h.create_session(SessionRequest {
        agent,
        agent_version: "2.1.328".into(),
        profile: None,
        passthrough,
        repo: None,
        work: None,
        stored_values: Vec::new(),
        known_plugins: Default::default(),
    })
    .unwrap()
}

fn grant(t: &T, agent: Agent) -> SessionGrant {
    grant_with(t, agent, false)
}

fn http_client() -> reqwest::Client {
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::builder().no_proxy().build().unwrap()
}

/// A request to the proxy with exactly `headers` (plus content-type when a body is sent).
fn send(h: &ProxyHandle, method: &str, path: &str, headers: &[(&str, &str)], body: Option<Vec<u8>>) -> (u16, HeaderMap, Vec<u8>) {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let m = reqwest::Method::from_bytes(method.as_bytes()).unwrap();
        let mut rb = http_client().request(m, h.url().join(path).unwrap());
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

fn auth(g: &SessionGrant) -> Vec<(&str, &str)> {
    vec![("x-api-key", g.token.as_str()), ("x-zygo-session", g.session.as_str())]
}

fn pt(g: &SessionGrant) -> Vec<(&str, &str)> {
    vec![("x-zygo-token", g.token.as_str()), ("x-zygo-session", g.session.as_str())]
}

fn req_body(role: &str) -> Value {
    json!({"model":role,"max_tokens":64,"stream":true,"messages":[{"role":"user","content":"hi"}]})
}

/// POSTs `body` with the session headers and `extra`.
fn post_body(t: &T, g: &SessionGrant, body: &Value, extra: &[(&str, &str)]) -> (u16, HeaderMap, Vec<u8>) {
    let mut h = auth(g);
    h.extend_from_slice(extra);
    t.n.set(t.n.get() + 1);
    send(&t.h, "POST", M, &h, Some(body.to_string().into_bytes()))
}

fn post(t: &T, g: &SessionGrant, role: &str, extra: &[(&str, &str)]) -> (u16, HeaderMap, Vec<u8>) {
    post_body(t, g, &req_body(role), extra)
}

const MAIN: (&str, &str) = ("x-claude-code-request-class", "main");
const SUB: (&str, &str) = ("x-claude-code-request-class", "subagent");

/// Sets the session override (§10.2.4).
fn compact(t: &T, g: &SessionGrant, w: Value) {
    let path = format!("v1/zygo/sessions/{}/compact", g.session);
    let r = send(&t.h, "POST", &path, &auth(g), Some(json!({ "window": w }).to_string().into_bytes()));
    assert_eq!(r.0, 200, "{}", String::from_utf8_lossy(&r.2));
}

/// The usage rows of `g`, once `n` rows were written in all.
fn rows(t: &T, g: &SessionGrant, n: u64) -> Vec<RequestRow> {
    assert!(t.h.usage_written(n, Duration::from_secs(10)), "usage rows not written");
    Store::open(&t.d.path().join("data/usage.db")).unwrap().requests_for_session(&g.session).unwrap()
}

/// The row of the last request, once every request's row was written.
fn last(t: &T, g: &SessionGrant) -> RequestRow {
    rows(t, g, t.n.get()).pop().expect("a row")
}

fn hv<'a>(h: &'a HeaderMap, n: &str) -> &'a str {
    h.get(n).map(|v| v.to_str().unwrap()).unwrap_or("")
}

/// The `context_*` adjustments of a row.
fn codes(r: &RequestRow) -> Vec<&str> {
    r.adjustments.iter().map(String::as_str).filter(|a| a.starts_with("context_")).collect()
}

fn tokens(r: &RequestRow) -> (u64, u64, u64, u64) {
    (r.input_tokens, r.output_tokens, r.cache_read_tokens, r.cache_write_tokens)
}

/// The SSE events of a response body.
fn events(b: &[u8]) -> Vec<SseEvent> {
    let s = std::str::from_utf8(b).unwrap();
    s.split("\n\n")
        .filter(|e| !e.trim().is_empty())
        .map(|e| {
            let mut name = String::new();
            let mut data = Value::Null;
            for l in e.lines() {
                if let Some(x) = l.strip_prefix("event: ") {
                    name = x.into();
                } else if let Some(x) = l.strip_prefix("data: ") {
                    data = serde_json::from_str(x).unwrap();
                }
            }
            SseEvent { event: name, data }
        })
        .collect()
}

/// The `usage` of the last `message_delta` event.
fn delta_usage(b: &[u8]) -> Value {
    events(b).into_iter().rfind(|e| e.event == "message_delta").expect("a message_delta").data["usage"].clone()
}

/// `message_start` with `start` usage, `blocks`, `message_delta` with `stop` and `usage`, `message_stop`.
fn reply(start: Value, blocks: &[(&str, Value)], stop: &str, usage: Value) -> String {
    let mut evs = vec![("message_start", json!({"message":{"id":"msg_1","type":"message","role":"assistant","content":[],"usage":start}}))];
    evs.extend(blocks.iter().cloned());
    evs.push(("message_delta", json!({"delta":{"stop_reason":stop,"stop_sequence":null},"usage":usage})));
    evs.push(("message_stop", json!({})));
    sse(&evs)
}

#[allow(non_snake_case)]
fn S0() -> Value {
    json!({"input_tokens":1,"output_tokens":1})
}

#[allow(non_snake_case)]
fn TEXT() -> Vec<(&'static str, Value)> {
    vec![
        ("content_block_start", json!({"index":0,"content_block":{"type":"text","text":""}})),
        ("content_block_stop", json!({"index":0})),
    ]
}

fn tool(name: &str) -> Vec<(&'static str, Value)> {
    vec![
        ("content_block_start", json!({"index":0,"content_block":{"type":"tool_use","id":"toolu_1","name":name,"input":{}}})),
        ("content_block_stop", json!({"index":0})),
    ]
}

/// Usage in this key order.
fn u4(i: u64, w: u64, r: u64, o: u64) -> Value {
    json!({"input_tokens":i,"cache_creation_input_tokens":w,"cache_read_input_tokens":r,"output_tokens":o})
}

// ---- A. Window arithmetic and the splice ----

#[test]
fn unit_windows() {
    assert_eq!(safety(400_000, 1_000_000), Some(360_000));
    assert_eq!(safety(922_000, 1_000_000), Some(829_800));
    assert_eq!(safety(200_000, 200_000), None);
    assert_eq!(window(400_000, 1_000_000), 1_000_000);
    assert_eq!(window(2_000_000, 2_000_000), 2_000_000);
    assert_eq!(prompt(Usage { input: 1000, output: 5, cache_read: 140_000, cache_write: 10_000 }), 151_000);
    assert_eq!(prompt(Usage { input: u64::MAX, output: u64::MAX, cache_read: u64::MAX, cache_write: u64::MAX }), u64::MAX);
    assert_eq!(agent_window(&row("claude-t-1")), 200_000);
    assert_eq!(agent_window(&row("gpt-t-1")), 1_000_000);
    assert_eq!(context_window(&row("n-t-1")), None);
    assert_eq!(context_window(&row("gpt-t-1")), Some(400_000));
    assert!(compact_tool("mcp__zygo__compact"));
    assert!(compact_tool("mcp__plugin_x_zygo__compact"));
    for n in ["mcp__zygo__compactx", "mcp__zygox__compact", "mcp__other__compact", "mcp____compact", "zygo__compact", "compact"] {
        assert!(!compact_tool(n), "{n}");
    }
}

#[test]
fn unit_one_m() {
    let hm = |vals: &[&str]| {
        let mut h = HeaderMap::new();
        for v in vals {
            h.append("anthropic-beta", v.parse().unwrap());
        }
        h
    };
    assert!(one_m(true, &HeaderMap::new()));
    assert!(one_m(false, &hm(&[q7_beta("fable").as_str()])));
    assert!(one_m(false, &hm(&["claude-code-20250219", "effort-2025-11-24, context-1m-2025-08-07"])));
    assert!(one_m(false, &hm(&["context-1m-2025-08-07", "effort-2025-11-24"])));
    assert!(!one_m(false, &hm(&[q7_beta("opus").as_str()])));
    assert!(!one_m(false, &hm(&["xcontext-1m-2025-08-07,context-1m"])));
    assert!(!one_m(false, &HeaderMap::new()));
    assert_eq!(safety(200_000, ONE_M_WINDOW), Some(180_000));
}

fn rw(line: &str, u: Usage, w: u64) -> Option<String> {
    rewrite_usage(line.as_bytes(), u, w).map(|b| String::from_utf8(b).unwrap())
}

#[test]
fn unit_rewrite_usage() {
    // (a) The fixture's message_delta line: only the two cache-read numbers change.
    let f = fixture("p1-endturn-full-iter.sse");
    let line = f.lines().find(|l| l.starts_with("data:") && l.contains("message_delta")).unwrap();
    let u = Usage { input: 1000, output: 5, cache_read: 999_000, cache_write: 0 };
    let want = line.replace("\"cache_read_input_tokens\": 999000", "\"cache_read_input_tokens\": 1999000");
    assert_ne!(want, line);
    assert_eq!(rw(line, u, 2_000_000).as_deref(), Some(want.as_str()));

    // (b) Absent fields are inserted after `{`.
    let u = Usage { input: 120, output: 42, cache_read: 30, cache_write: 7 };
    let pre = r#"data: {"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":"#;
    let l = |usage: &str| format!("{pre}{usage}}}");
    assert_eq!(
        rw(&l(r#"{"output_tokens":42}"#), u, 200_000),
        Some(l(r#"{"input_tokens":120,"cache_creation_input_tokens":7,"cache_read_input_tokens":199873,"output_tokens":42}"#))
    );
    // (b2) An escaped `type` value is the same event to the agent's parser.
    assert_eq!(
        rw(r#"data: {"type":"message\u005fdelta","usage":{"output_tokens":42}}"#, u, 200_000),
        Some(r#"data: {"type":"message\u005fdelta","usage":{"input_tokens":120,"cache_creation_input_tokens":7,"cache_read_input_tokens":199873,"output_tokens":42}}"#.to_string())
    );
    // (c) An empty object.
    assert_eq!(
        rw(&l("{}"), u, 200_000),
        Some(l(r#"{"input_tokens":120,"cache_creation_input_tokens":7,"cache_read_input_tokens":199873}"#))
    );
    // (d) Only the last iterations entry changes.
    let it = r#"{"input_tokens":1,"cache_creation_input_tokens":2,"cache_read_input_tokens":3,"output_tokens":4}"#;
    let it2 = r#"{"input_tokens":120,"cache_creation_input_tokens":7,"cache_read_input_tokens":199873,"output_tokens":4}"#;
    assert_eq!(
        rw(&l(&format!(r#"{{"output_tokens":42,"iterations":[{it},{it}]}}"#)), u, 200_000),
        Some(l(&format!(
            r#"{{"input_tokens":120,"cache_creation_input_tokens":7,"cache_read_input_tokens":199873,"output_tokens":42,"iterations":[{it},{it2}]}}"#
        )))
    );
    // (e) A window below input + cache_write keeps the cumulative cache_read.
    assert_eq!(
        rw(&l(r#"{"output_tokens":42}"#), u, 100),
        Some(l(r#"{"input_tokens":120,"cache_creation_input_tokens":7,"cache_read_input_tokens":30,"output_tokens":42}"#))
    );
    // (f) Not a message_delta with usage, or not UTF-8.
    assert_eq!(rw(r#"data: {"type":"message_start","message":{"usage":{"input_tokens":1}}}"#, u, 200_000), None);
    assert_eq!(rw(r#"data: {"type":"message_delta","delta":{"stop_reason":"end_turn"}}"#, u, 200_000), None);
    assert_eq!(rewrite_usage(b"data: \xff", u, 200_000), None);
    // (g) An escaped key counts as present and keeps its text.
    assert_eq!(
        rw(&l(r#"{"cache_read_input_tokens":30,"output_tokens":42}"#), u, 200_000),
        Some(l(r#"{"input_tokens":120,"cache_creation_input_tokens":7,"cache_read_input_tokens":199873,"output_tokens":42}"#))
    );
    // (h) A repeated key in an edited object.
    assert_eq!(rw(&l(r#"{"output_tokens":42,"output_tokens":43}"#), u, 200_000), None);
    assert_eq!(rw(&l(&format!(r#"{{"output_tokens":42,"iterations":[{it},{{"output_tokens":1,"output_tokens":2}}]}}"#)), u, 200_000), None);
}

// ---- B. The stage and the three triggers ----

#[allow(non_snake_case)]
fn U15() -> Value {
    u4(1000, 10000, 140000, 5)
}

#[allow(non_snake_case)]
fn R15() -> Value {
    u4(1000, 10000, 189000, 5)
}

/// Posts `role` with `extra` headers after scripting `body` (SSE) at `p` on `t.up`; asserts 200.
fn run(t: &T, g: &SessionGrant, p: &str, body: &str, role: &str, extra: &[(&str, &str)]) -> (HeaderMap, String) {
    t.up.script(p, vec![sse_reply(body)]);
    let (s, h, b) = post(t, g, role, extra);
    assert_eq!(s, 200, "{}", String::from_utf8_lossy(&b));
    (h, String::from_utf8(b).unwrap())
}

#[test]
fn soft_end_turn_main_rewritten() {
    let t = setup("claude-t-1");
    let g = grant(&t, Agent::ClaudeCode);
    let (h, b) = run(&t, &g, "/c", &reply(S0(), &TEXT(), "end_turn", U15()), "lead", &[MAIN]);
    assert_eq!(b, reply(S0(), &TEXT(), "end_turn", R15()));
    assert!(h.get("content-length").is_none());
    assert!(!hv(&h, "x-zygo-adjustments").contains("context_"), "{}", hv(&h, "x-zygo-adjustments"));
    let r = last(&t, &g);
    assert_eq!(tokens(&r), (1000, 5, 140000, 10000));
    assert_eq!(codes(&r), ["context_signal"]);

    // Two message_delta events: both rewritten, one code.
    let two = |u: Value| {
        let start = json!({"message":{"id":"msg_1","type":"message","role":"assistant","content":[],"usage":S0()}});
        let d = json!({"delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":u});
        sse(&[("message_start", start), ("message_delta", d.clone()), ("message_delta", d), ("message_stop", json!({}))])
    };
    let (_, b) = run(&t, &g, "/c", &two(U15()), "lead", &[MAIN]);
    assert_eq!(b, two(R15()));
    assert_eq!(codes(&last(&t, &g)), ["context_signal"]);
}

#[test]
fn soft_iterations_last_entry_rewritten() {
    let t = setup("claude-t-1");
    let g = grant(&t, Agent::ClaudeCode);
    let with_iter = |top: Value, second: Value| {
        let mut first = u4(500, 0, 0, 2);
        first["type"] = json!("message");
        let mut second = second;
        second["type"] = json!("message");
        let mut u = top;
        u["iterations"] = json!([first, second]);
        u
    };
    let (_, b) = run(&t, &g, "/c", &reply(S0(), &TEXT(), "end_turn", with_iter(U15(), U15())), "lead", &[MAIN]);
    assert_eq!(b, reply(S0(), &TEXT(), "end_turn", with_iter(R15(), R15())));
    assert_eq!(codes(&last(&t, &g)), ["context_signal"]);
}

#[test]
fn soft_fixture_bytes_kept() {
    let t = setup("claude-t-1");
    let g = grant(&t, Agent::ClaudeCode);
    let f = fixture("p1-endturn-full-iter.sse");
    let want = f.replace("\"cache_read_input_tokens\": 999000", "\"cache_read_input_tokens\": 1999000");
    assert_ne!(want, f);
    let (_, b) = run(&t, &g, "/b", &f, "triage", &[MAIN]);
    assert_eq!(b, want);
    assert_eq!(codes(&last(&t, &g)), ["context_signal"]);
}

#[test]
fn soft_not_triggered_byte_identical() {
    let t = setup("claude-t-1");
    let g = grant(&t, Agent::ClaudeCode);
    let compaction: &[(&str, &str)] = &[("x-claude-code-request-class", "compaction"), ("x-claude-code-compaction", "auto")];
    let cases: Vec<(String, &[(&str, &str)])> = vec![
        (reply(S0(), &TEXT(), "tool_use", U15()), &[MAIN]),
        (reply(S0(), &TEXT(), "end_turn", U15()), &[SUB]),
        (reply(S0(), &TEXT(), "end_turn", U15()), compaction),
        (reply(S0(), &TEXT(), "end_turn", u4(1000, 10000, 139000, 5)), &[MAIN]),
        (reply(S0(), &TEXT(), "end_turn", U15()), &[]),
    ];
    for (i, (body, extra)) in cases.iter().enumerate() {
        let (_, b) = run(&t, &g, "/c", body, "lead", extra);
        assert_eq!(&b, body, "case {i}");
        assert!(codes(&last(&t, &g)).is_empty(), "case {i}");
    }
}

#[test]
fn soft_override_and_off() {
    let t = setup("claude-t-1");
    let body = reply(S0(), &TEXT(), "end_turn", U15());
    // claude-ns has no soft window: nothing until an override.
    let g = grant(&t, Agent::ClaudeCode);
    let (_, b) = run(&t, &g, "/ns", &body, "plan", &[MAIN]);
    assert_eq!(b, body);
    compact(&t, &g, json!(100000));
    let (_, b) = run(&t, &g, "/ns", &body, "plan", &[MAIN]);
    assert_eq!(b, reply(S0(), &TEXT(), "end_turn", R15()));
    assert_eq!(codes(&last(&t, &g)), ["context_signal"]);
    // On lead (soft 150,000): an override above the prompt, above the window, and off.
    for w in [json!(160000), json!(600000), json!("off")] {
        let g = grant(&t, Agent::ClaudeCode);
        compact(&t, &g, w.clone());
        let (_, b) = run(&t, &g, "/c", &body, "lead", &[MAIN]);
        assert_eq!(b, body, "{w}");
        assert!(codes(&last(&t, &g)).is_empty(), "{w}");
    }
}

#[test]
fn soft_passthrough_armed() {
    let t = setup("claude-t-1");
    let g = grant_with(&t, Agent::ClaudeCode, true);
    t.anth.script("", vec![sse_reply(&reply(S0(), &TEXT(), "end_turn", U15()))]);
    let mut hd = pt(&g);
    hd.extend([("authorization", OAT_BEARER), MAIN]);
    t.n.set(t.n.get() + 1);
    let (s, _, b) = send(&t.h, "POST", M, &hd, Some(req_body("claude-t-1").to_string().into_bytes()));
    assert_eq!(s, 200, "{}", String::from_utf8_lossy(&b));
    assert_eq!(String::from_utf8(b).unwrap(), reply(S0(), &TEXT(), "end_turn", R15()));
    let r = last(&t, &g);
    assert_eq!(r.role_served.as_deref(), Some("subscription"));
    assert_eq!(codes(&r), ["context_signal"]);
}

#[test]
fn compact_call_rewritten_any_size() {
    let t = setup("claude-t-1");
    let start = json!({"input_tokens":120,"cache_read_input_tokens":30,"cache_creation_input_tokens":7,"output_tokens":1});
    let body = reply(start.clone(), &tool("mcp__zygo__compact"), "tool_use", json!({"output_tokens":42}));
    let want = reply(
        start,
        &tool("mcp__zygo__compact"),
        "tool_use",
        json!({"input_tokens":120,"cache_creation_input_tokens":7,"cache_read_input_tokens":199873,"output_tokens":42}),
    );
    let check = |g: &SessionGrant, p: &str, role: &str, extra: &[(&str, &str)]| {
        let (_, b) = run(&t, g, p, &body, role, extra);
        assert_eq!(b, want, "{role} {extra:?}");
        let r = last(&t, g);
        assert_eq!(codes(&r), ["context_compact"]);
        assert_eq!(tokens(&r), (120, 42, 30, 7));
    };
    let g = grant(&t, Agent::ClaudeCode);
    check(&g, "/c", "lead", &[MAIN]);
    check(&g, "/c", "lead", &[SUB]);
    check(&g, "/ns", "plan", &[MAIN]);
    let g = grant(&t, Agent::ClaudeCode);
    compact(&t, &g, json!("off"));
    check(&g, "/c", "lead", &[MAIN]);
    // A compaction request is never armed.
    let (_, b) = run(&t, &g, "/c", &body, "lead", &[MAIN, ("x-claude-code-compaction", "auto")]);
    assert_eq!(b, body);
    assert!(codes(&last(&t, &g)).is_empty());
}

#[test]
fn compact_name_matched_by_parts() {
    let t = setup("claude-t-1");
    let g = grant(&t, Agent::ClaudeCode);
    let usage = json!({"output_tokens":42});
    let text = vec![
        ("content_block_start", json!({"index":0,"content_block":{"type":"text","text":"mcp__zygo__compact"}})),
        ("content_block_stop", json!({"index":0})),
    ];
    let mut bodies: Vec<String> = ["mcp__zygo__compactx", "mcp__zygox__compact", "mcp__other__compact", "mcp____compact", "zygo__compact", "compact"]
        .iter()
        .map(|n| reply(S0(), &tool(n), "tool_use", usage.clone()))
        .collect();
    bodies.push(reply(S0(), &text, "end_turn", usage.clone()));
    for body in &bodies {
        let (_, b) = run(&t, &g, "/c", body, "lead", &[MAIN]);
        assert_eq!(&b, body);
        assert!(codes(&last(&t, &g)).is_empty());
    }
}

/// (override, class header, stop reason, codes when over the safety window)
type Case = (Option<Value>, (&'static str, &'static str), &'static str, &'static [&'static str]);

#[test]
fn safety_window_native() {
    let t = setup("claude-t-1");
    let over = u4(1, 0, 360000, 5);
    let under = u4(1, 0, 359999, 5);
    let cases: [Case; 4] = [
        (None, MAIN, "end_turn", &["context_safety"]),
        (Some(json!("off")), MAIN, "end_turn", &["context_safety"]),
        (Some(json!(2000000)), MAIN, "end_turn", &["context_safety", "context_signal"]),
        (None, SUB, "tool_use", &["context_safety"]),
    ];
    for (w, class, stop, want) in cases {
        for (u, rewritten) in [(&over, true), (&under, false)] {
            let g = grant(&t, Agent::ClaudeCode);
            if let Some(w) = &w {
                compact(&t, &g, w.clone());
            }
            let body = reply(S0(), &TEXT(), stop, u.clone());
            let (_, b) = run(&t, &g, "/g", &body, "code", &[class]);
            let r = last(&t, &g);
            if rewritten {
                assert_eq!(b, reply(S0(), &TEXT(), stop, u4(1, 0, 999999, 5)), "{w:?} {class:?}");
                assert_eq!(codes(&r), want, "{w:?} {class:?}");
            } else {
                assert_eq!(b, body, "{w:?} {class:?}");
                assert!(codes(&r).is_empty(), "{w:?} {class:?}");
            }
        }
    }
}

/// The `anthropic-beta` value Claude Code 2.1.328 sent for an alias (Q7 capture): `fable` (`design[1m]`) or `opus`.
fn q7_beta(alias: &str) -> String {
    let p = format!("{}/../../fixtures/claude-code/q7/q7-claude.json", env!("CARGO_MANIFEST_DIR"));
    let v: Value = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
    v["aliases (ANTHROPIC_DEFAULT_*_MODEL)"][alias]["anthropic-beta"].as_str().unwrap().to_string()
}

/// §10.2.2 note (N15-1M): a `[1m]` request (Claude Code's `context-1m-` beta) to a 200k Claude model:
/// `agent_window` 1M, so `safety` = 180,000 and the rewrite target is 1M.
#[test]
fn one_m_beta_safety_200k() {
    let t = setup("claude-t-1");
    let (fable, opus) = (q7_beta("fable"), q7_beta("opus"));
    let (fable, opus) = (fable.as_str(), opus.as_str());
    let over = u4(1, 0, 180_000, 5); // prompt 180,001
    let at = u4(1, 0, 179_999, 5); // prompt 180,000
    // With the 1M beta, over the safety window: rewritten to window 1M, code context_safety.
    let g = grant(&t, Agent::ClaudeCode);
    let (_, b) = run(&t, &g, "/ns", &reply(S0(), &TEXT(), "end_turn", over.clone()), "plan", &[MAIN, ("anthropic-beta", fable)]);
    assert_eq!(b, reply(S0(), &TEXT(), "end_turn", u4(1, 0, 999_999, 5)));
    assert_eq!(codes(&last(&t, &g)), ["context_safety"]);
    // At exactly 180,000: not over `safety`, unchanged.
    let body = reply(S0(), &TEXT(), "end_turn", at);
    let (_, b) = run(&t, &g, "/ns", &body, "plan", &[MAIN, ("anthropic-beta", fable)]);
    assert_eq!(b, body);
    assert!(codes(&last(&t, &g)).is_empty());
    // Same request without the 1M token (the opus alias's beta): agent_window 200k, no safety, unchanged.
    let g = grant(&t, Agent::ClaudeCode);
    let body = reply(S0(), &TEXT(), "end_turn", over);
    let (_, b) = run(&t, &g, "/ns", &body, "plan", &[MAIN, ("anthropic-beta", opus)]);
    assert_eq!(b, body);
    assert!(codes(&last(&t, &g)).is_empty());
}

/// Stage 2's remembered `[1m]` (`Resolved.one_m`) is the same signal without the beta.
#[test]
fn one_m_suffix_in_name() {
    let t = setup("claude-t-1");
    let g = grant(&t, Agent::ClaudeCode);
    let (_, b) = run(&t, &g, "/ns", &reply(S0(), &TEXT(), "end_turn", u4(1, 0, 180_000, 5)), "plan[1m]", &[MAIN]);
    assert_eq!(b, reply(S0(), &TEXT(), "end_turn", u4(1, 0, 999_999, 5)));
    assert_eq!(codes(&last(&t, &g)), ["context_safety"]);
}

/// Responses SSE: one message "ok", then `response.completed` with `usage`.
fn responses_text(usage: Value) -> String {
    sse(&[
        ("response.created", json!({"response":{"id":"resp_3","model":"gpt-t-2","status":"in_progress","output":[],"usage":null}})),
        ("response.output_item.added", json!({"output_index":0,"item":{"type":"message","id":"msg_1","role":"assistant","content":[]}})),
        ("response.output_text.delta", json!({"output_index":0,"content_index":0,"delta":"ok"})),
        ("response.content_part.done", json!({"output_index":0,"content_index":0,"part":{"type":"output_text","text":"ok"}})),
        ("response.output_item.done", json!({"output_index":0,"item":{"type":"message"}})),
        ("response.completed", json!({"response":{"id":"resp_3","status":"completed","model":"gpt-t-2","usage":usage}})),
    ])
}

#[test]
fn safety_window_translated() {
    let t = setup("claude-t-1");
    let g = grant(&t, Agent::ClaudeCode);
    let u = |i: u64| json!({"input_tokens":i,"input_tokens_details":{"cached_tokens":0},"output_tokens":5});
    let (_, b) = run(&t, &g, "/o", &responses_text(u(360001)), "review", &[SUB]);
    assert_eq!(
        delta_usage(b.as_bytes()),
        json!({"input_tokens":360001,"cache_read_input_tokens":639999,"cache_creation_input_tokens":0,"output_tokens":5})
    );
    let r = last(&t, &g);
    assert_eq!(tokens(&r), (360001, 5, 0, 0));
    assert_eq!(codes(&r), ["context_safety"]);
    let (_, b) = run(&t, &g, "/o", &responses_text(u(360000)), "review", &[SUB]);
    assert_eq!(delta_usage(b.as_bytes())["cache_read_input_tokens"], json!(0));
    assert!(codes(&last(&t, &g)).is_empty());
}

#[test]
fn safety_p1_tool_use_fixtures() {
    let t = setup("claude-t-1");
    let g = grant(&t, Agent::ClaudeCode);
    for name in ["p1-tooluse-full-iter.sse", "p1-tooluse-full-noiter.sse"] {
        let f = fixture(name);
        let sent = f.replace("\"cache_read_input_tokens\": 999000", "\"cache_read_input_tokens\": 400000");
        assert_ne!(sent, f);
        let (_, b) = run(&t, &g, "/g", &sent, "code", &[SUB]);
        assert_eq!(b, f, "{name}");
        assert_eq!(codes(&last(&t, &g)), ["context_safety"], "{name}");
    }
}

/// The `content` the events build.
fn assemble(evs: &[SseEvent]) -> Value {
    let mut blocks: Vec<Value> = Vec::new();
    let mut partial: Vec<String> = Vec::new();
    for e in evs {
        let d = &e.data;
        let i = d["index"].as_u64().unwrap_or(0) as usize;
        match e.event.as_str() {
            "content_block_start" => {
                blocks.push(d["content_block"].clone());
                partial.push(String::new());
            }
            "content_block_delta" if d["delta"]["type"] == json!("input_json_delta") => {
                partial[i] += d["delta"]["partial_json"].as_str().unwrap();
            }
            "content_block_stop" if blocks[i]["type"] == json!("tool_use") && !partial[i].is_empty() => {
                blocks[i]["input"] = serde_json::from_str(&partial[i]).unwrap();
            }
            _ => {}
        }
    }
    Value::Array(blocks)
}

fn wide_tool() -> Value {
    let mut p = serde_json::Map::new();
    p.insert("n".into(), json!({"type":"integer","minimum":1}));
    for i in 0..25 {
        p.insert(format!("p{i}"), json!({"type":"string"}));
    }
    json!({"name":"get","input_schema":{"type":"object","properties":p,"required":["n"]}})
}

fn tool_ab() -> Value {
    json!({"name":"get","description":"d","input_schema":{"type":"object","properties":{"a":{"type":"string"},"b":{"type":"integer"}},"required":["b"]}})
}

#[test]
fn safety_restore_native() {
    let t = setup("claude-t-1");
    let g = grant(&t, Agent::ClaudeCode);
    let up = sse(&[
        ("message_start", json!({"message":{"id":"msg_2","type":"message","role":"assistant","model":"gpt-t-1","content":[],"stop_reason":null,"stop_sequence":null,"usage":S0()}})),
        ("content_block_start", json!({"index":0,"content_block":{"type":"tool_use","id":"toolu_1","name":"get","input":{}}})),
        ("content_block_delta", json!({"index":0,"delta":{"type":"input_json_delta","partial_json":"{\"p24\":nu"}})),
        ("content_block_delta", json!({"index":0,"delta":{"type":"input_json_delta","partial_json":"ll,\"p0\":\"x\",\"n\":2}"}})),
        ("content_block_stop", json!({"index":0})),
        ("message_delta", json!({"delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":u4(1, 0, 360000, 5)})),
        ("message_stop", json!({})),
    ]);
    t.up.script("/g", vec![sse_reply(&up)]);
    let mut body = req_body("code");
    body["tools"] = json!([wide_tool()]);
    let (s, _, b) = post_body(&t, &g, &body, &[MAIN]);
    assert_eq!(s, 200, "{}", String::from_utf8_lossy(&b));
    assert_eq!(assemble(&events(&b))[0]["input"], json!({"p0":"x","n":2}));
    assert_eq!(delta_usage(&b), u4(1, 0, 999999, 5));
    assert_eq!(codes(&last(&t, &g)), ["context_safety"]);
}

#[test]
fn safety_restore_translated() {
    let t = setup("claude-t-1");
    let g = grant(&t, Agent::ClaudeCode);
    let up = sse(&[
        ("response.created", json!({"response":{"id":"resp_2","model":"gpt-t-2","status":"in_progress","output":[],"usage":null}})),
        ("response.output_item.added", json!({"output_index":0,"item":{"type":"function_call","id":"fc_1","call_id":"call_9","name":"get","arguments":""}})),
        ("response.function_call_arguments.delta", json!({"output_index":0,"delta":"{\"a\":nu"})),
        ("response.function_call_arguments.delta", json!({"output_index":0,"delta":"ll,\"b\":2}"})),
        ("response.output_item.done", json!({"output_index":0,"item":{"type":"function_call","call_id":"call_9","name":"get","arguments":"{\"a\":null,\"b\":2}"}})),
        ("response.completed", json!({"response":{"id":"resp_2","status":"completed","model":"gpt-t-2","usage":{"input_tokens":360001,"output_tokens":3}}})),
    ]);
    t.up.script("/o", vec![sse_reply(&up)]);
    let mut body = req_body("review");
    body["tools"] = json!([tool_ab()]);
    let (s, _, b) = post_body(&t, &g, &body, &[MAIN]);
    assert_eq!(s, 200, "{}", String::from_utf8_lossy(&b));
    assert_eq!(assemble(&events(&b))[0]["input"], json!({"b":2}));
    assert_eq!(
        delta_usage(&b),
        json!({"input_tokens":360001,"cache_read_input_tokens":639999,"cache_creation_input_tokens":0,"output_tokens":3})
    );
    assert_eq!(codes(&last(&t, &g)), ["context_safety"]);
}

// ---- C. Bounds and C7 ----

#[test]
fn bound_split_lines_and_large_line() {
    let t = setup("claude-t-1");
    let g = grant(&t, Agent::ClaudeCode);
    // (a) 7-byte chunks: lines are held until their newline.
    let mut r = sse_reply(&reply(S0(), &TEXT(), "end_turn", U15()));
    r.chunk = 7;
    t.up.script("/c", vec![r]);
    let (s, _, b) = post(&t, &g, "lead", &[MAIN]);
    assert_eq!(s, 200);
    assert_eq!(String::from_utf8(b).unwrap(), reply(S0(), &TEXT(), "end_turn", R15()));
    // (b) A line over HOLD_MAX passes unchanged; the later message_delta is still rewritten.
    let blocks = || {
        vec![
            ("content_block_start", json!({"index":0,"content_block":{"type":"text","text":""}})),
            ("content_block_delta", json!({"index":0,"delta":{"type":"text_delta","text":"x".repeat(HOLD_MAX)}})),
            ("content_block_stop", json!({"index":0})),
        ]
    };
    let mut r = sse_reply(&reply(S0(), &blocks(), "end_turn", U15()));
    r.chunk = 65536;
    t.up.script("/c", vec![r]);
    let (s, _, b) = post(&t, &g, "lead", &[MAIN]);
    assert_eq!(s, 200);
    assert!(b == reply(S0(), &blocks(), "end_turn", R15()).as_bytes(), "large line");
    assert_eq!(codes(&last(&t, &g)), ["context_signal"]);
}

#[test]
fn bound_armed_drops_content_length() {
    let t = setup("claude-t-1");
    let body = reply(S0(), &TEXT(), "end_turn", U15());
    let cl = |b: &str| {
        let mut r = sse_reply(b);
        r.cl = true;
        r
    };
    let g = grant(&t, Agent::ClaudeCode);
    t.up.script("/c", vec![cl(&body)]);
    let (_, h, b) = post(&t, &g, "lead", &[MAIN]);
    assert!(h.get("content-length").is_none());
    assert_eq!(String::from_utf8(b).unwrap(), reply(S0(), &TEXT(), "end_turn", R15()));
    // OpenCode is never armed (§10.2.5).
    let g = grant(&t, Agent::OpenCode);
    t.up.script("/c", vec![cl(&body)]);
    let (s, h, b) = post(&t, &g, "lead", &[]);
    assert_eq!(s, 200);
    assert_eq!(String::from_utf8(b).unwrap(), body);
    assert_eq!(hv(&h, "content-length"), body.len().to_string());
    assert!(codes(&last(&t, &g)).is_empty());
}

#[test]
fn bound_no_context_window_never_armed() {
    let t = setup("claude-t-1");
    let g = grant(&t, Agent::ClaudeCode);
    let body = reply(S0(), &tool("mcp__zygo__compact"), "tool_use", U15());
    let mut r = sse_reply(&body);
    r.cl = true;
    t.up.script("/n", vec![r]);
    let (s, h, b) = post(&t, &g, "read", &[MAIN]);
    assert_eq!(s, 200);
    assert_eq!(String::from_utf8(b).unwrap(), body);
    assert_eq!(hv(&h, "content-length"), body.len().to_string());
    assert!(codes(&last(&t, &g)).is_empty());
}

#[test]
fn bound_non_streamed_never_rewritten() {
    let t = setup("claude-t-1");
    let g = grant(&t, Agent::ClaudeCode);
    // (a) An untranslated JSON 2xx.
    let m = json!({"id":"msg_1","type":"message","role":"assistant","model":"gpt-t-1",
        "content":[{"type":"tool_use","id":"toolu_1","name":"mcp__zygo__compact","input":{}}],"stop_reason":"tool_use","stop_sequence":null,
        "usage":{"input_tokens":1,"cache_creation_input_tokens":0,"cache_read_input_tokens":360000,"output_tokens":5}});
    t.up.script("/g", vec![json_reply(m.clone())]);
    let (s, _, b) = post(&t, &g, "code", &[MAIN]);
    assert_eq!(s, 200);
    assert_eq!(String::from_utf8(b).unwrap(), m.to_string());
    assert!(codes(&last(&t, &g)).is_empty());
    // (b) A translated JSON 2xx sent to a streaming client as SSE.
    let resp = json!({"id":"resp_1","object":"response","status":"completed","model":"up-o",
        "output":[{"type":"message","id":"msg_1","role":"assistant","content":[{"type":"output_text","text":"ok"}]}],
        "usage":{"input_tokens":360001,"input_tokens_details":{"cached_tokens":0},"output_tokens":5}});
    t.up.script("/o", vec![json_reply(resp)]);
    let (s, h, b) = post(&t, &g, "review", &[MAIN]);
    assert_eq!(s, 200, "{}", String::from_utf8_lossy(&b));
    assert!(hv(&h, "content-type").starts_with("text/event-stream"));
    assert_eq!(delta_usage(&b)["cache_read_input_tokens"], json!(0));
    assert!(codes(&last(&t, &g)).is_empty());
}

#[test]
fn c7_window_never_told() {
    let t = setup("claude-t-1");
    for w in [None, Some(123457)] {
        let g = grant(&t, Agent::ClaudeCode);
        if let Some(w) = w {
            compact(&t, &g, json!(w));
        }
        // The catalog id: the request is forwarded as received (a role name gets its model rewritten, §7.5).
        let req = req_body("claude-t-1").to_string();
        t.up.script("/c", vec![sse_reply(&reply(S0(), &TEXT(), "end_turn", U15()))]);
        t.n.set(t.n.get() + 1);
        let mut hd = auth(&g);
        hd.push(MAIN);
        let (s, h, b) = send(&t.h, "POST", M, &hd, Some(req.clone().into_bytes()));
        assert_eq!(s, 200);
        assert_eq!(t.up.at("/c").body, req.as_bytes(), "{w:?}");
        let text = String::from_utf8(b.clone()).unwrap();
        for n in ["150000", "123457"] {
            assert!(!text.contains(n), "{w:?} body {n}");
            for v in h.values() {
                assert!(!v.to_str().unwrap_or("").contains(n), "{w:?} header {n}");
            }
        }
        let u = delta_usage(&b);
        let sum: u64 = ["input_tokens", "cache_creation_input_tokens", "cache_read_input_tokens"].iter().map(|k| u[k].as_u64().unwrap()).sum();
        assert_eq!(sum, 200_000, "{w:?}");
        assert_eq!(codes(&last(&t, &g)), ["context_signal"], "{w:?}");
    }
}

// ---- D. Status: used and effective context ----

/// Once every row is written: (model, context_window, effective_window, threshold) of the status (AU29).
fn windows(t: &T, g: &SessionGrant) -> (Option<String>, Option<u64>, Option<u64>, Option<u64>) {
    rows(t, g, t.n.get());
    let s = t.h.session(&g.session).unwrap().status();
    (s.model, s.context_window, s.effective_window, s.threshold)
}

/// Posts `role` with `extra` headers and closes the connection once the body holds `line` and its "\n".
///
/// The tee reads usage only from whole lines, and it reads each upstream chunk before it forwards that chunk. So
/// when the client has the "\n" of `line`, the proxy has read `line`. A shorter needle (a part of a line, or the
/// `event:` line before the `data:` line) lets the client close before the proxy receives the rest of the line: the
/// fake upstream sends its body in three chunks, and the outcome then depends on the timing of the OS.
fn read_then_drop(t: &T, g: &SessionGrant, role: &str, extra: &[(&str, &str)], line: &str) {
    assert!(line.starts_with("data: ") && !line.contains('\n'), "a whole data line: {line:?}");
    let needle = format!("{line}\n");
    let mut hd = auth(g);
    hd.extend_from_slice(extra);
    t.n.set(t.n.get() + 1);
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let mut rb = http_client().post(t.h.url().join(M).unwrap()).header("content-type", "application/json");
        for (k, v) in &hd {
            rb = rb.header(*k, *v);
        }
        let mut r = rb.body(req_body(role).to_string()).send().await.unwrap();
        assert_eq!(r.status().as_u16(), 200);
        let mut seen = Vec::new();
        while !String::from_utf8_lossy(&seen).contains(&needle) {
            seen.extend_from_slice(&r.chunk().await.unwrap().expect("the body ended before the needle"));
        }
    }); // the response, its client and the runtime drop here: the connection closes
}

/// The first `data:` line of an SSE body, without its "\n".
fn data_line(body: &str) -> &str {
    body.lines().find(|l| l.starts_with("data: ")).expect("a data line")
}

#[test]
fn status_used_and_effective() {
    let st = |t: &T, g: &SessionGrant| {
        // Every row is written: used_context is set before its row.
        rows(t, g, t.n.get());
        let s = t.h.session(&g.session).unwrap().status();
        (s.used_context, s.effective_window)
    };
    let t = setup("claude-t-1");
    let g = grant(&t, Agent::ClaudeCode);
    assert_eq!(st(&t, &g), (None, Some(150000)));
    assert_eq!(windows(&t, &g), (Some("claude-t-1".into()), Some(200000), Some(150000), Some(150000)));
    run(&t, &g, "/c", &reply(S0(), &TEXT(), "end_turn", U15()), "lead", &[MAIN]);
    assert_eq!(st(&t, &g).0, Some(151000));
    let small = reply(S0(), &TEXT(), "end_turn", u4(1, 0, 5000, 1));
    run(&t, &g, "/c", &small, "lead", &[SUB]);
    assert_eq!(st(&t, &g).0, Some(151000));
    run(&t, &g, "/c", &small, "lead", &[("x-claude-code-request-class", "compaction"), ("x-claude-code-compaction", "auto")]);
    assert_eq!(st(&t, &g).0, Some(151000));
    compact(&t, &g, json!(600000));
    assert_eq!(st(&t, &g).1, Some(600000));
    compact(&t, &g, json!("off"));
    assert_eq!(st(&t, &g).1, None);
    let o = grant(&t, Agent::OpenCode);
    run(&t, &o, "/c", &reply(S0(), &TEXT(), "end_turn", U15()), "lead", &[]);
    assert_eq!(st(&t, &o).0, Some(151000));

    // gpt-t-1 as the main role's model: the safety window applies.
    let t = setup("gpt-t-1");
    let g = grant(&t, Agent::ClaudeCode);
    assert_eq!(st(&t, &g).1, Some(360000));
    assert_eq!(windows(&t, &g), (Some("gpt-t-1".into()), Some(400000), Some(360000), None));
    compact(&t, &g, json!(2000000));
    assert_eq!(st(&t, &g).1, Some(360000));
    compact(&t, &g, json!(300000));
    assert_eq!(st(&t, &g).1, Some(300000));
    compact(&t, &g, json!("off"));
    assert_eq!(st(&t, &g).1, Some(360000));
}

#[test]
fn unit_soft_threshold_public() { // H§7.2 `threshold`
    let (t1, ns) = (row("claude-t-1"), row("claude-ns"));
    assert_eq!(soft_threshold(None, &t1, None), Some(150_000));
    assert_eq!(soft_threshold(None, &ns, None), None);
    assert_eq!(soft_threshold(Some(WindowArg::Tokens(190_000)), &ns, Some(180_000)), Some(180_000));
    assert_eq!(soft_threshold(Some(WindowArg::Off), &t1, Some(180_000)), None);
}

#[test]
fn status_follows_one_m_request() { // AU29 `[1m]` name; AU28 example: 190k clamped by safety 180k → 180k
    let t = setup("claude-t-1");
    let g = grant(&t, Agent::ClaudeCode);
    let small = reply(S0(), &TEXT(), "end_turn", u4(1, 0, 5000, 1));
    run(&t, &g, "/ns", &small, "plan[1m]", &[MAIN]);
    assert_eq!(windows(&t, &g), (Some("claude-ns".into()), Some(200000), Some(180000), None));
    compact(&t, &g, json!(190000));
    assert_eq!(windows(&t, &g), (Some("claude-ns".into()), Some(200000), Some(180000), Some(180000)));
    run(&t, &g, "/ns", &small, "plan", &[MAIN]);
    assert_eq!(windows(&t, &g), (Some("claude-ns".into()), Some(200000), Some(190000), Some(190000)));
}

#[test]
fn status_follows_one_m_beta() { // AU29: the `context-1m-` beta form of `[1m]` (Q7 capture)
    let t = setup("claude-t-1");
    let g = grant(&t, Agent::ClaudeCode);
    let fable = q7_beta("fable");
    run(&t, &g, "/ns", &reply(S0(), &TEXT(), "end_turn", u4(1, 0, 5000, 1)), "plan", &[MAIN, ("anthropic-beta", fable.as_str())]);
    assert_eq!(windows(&t, &g), (Some("claude-ns".into()), Some(200000), Some(180000), None));
}

#[test]
fn status_follows_other_role() { // AU29: `/model` picks another role; a subagent request stores nothing
    let t = setup("claude-t-1");
    let g = grant(&t, Agent::ClaudeCode);
    run(&t, &g, "/b", &reply(S0(), &TEXT(), "end_turn", U15()), "triage", &[MAIN]);
    assert_eq!(windows(&t, &g), (Some("claude-big".into()), Some(2000000), Some(900000), Some(900000)));
    run(&t, &g, "/ns", &reply(S0(), &TEXT(), "end_turn", u4(1, 0, 5000, 1)), "plan", &[SUB]);
    assert_eq!(windows(&t, &g), (Some("claude-big".into()), Some(2000000), Some(900000), Some(900000)));
}

#[test]
fn status_ignores_compaction_request() { // AU29: class `compaction` stores nothing
    let t = setup("claude-t-1");
    let g = grant(&t, Agent::ClaudeCode);
    run(&t, &g, "/b", &reply(S0(), &TEXT(), "end_turn", U15()), "triage", &[MAIN]);
    let before = windows(&t, &g);
    let compaction = [("x-claude-code-request-class", "compaction"), ("x-claude-code-compaction", "auto")];
    run(&t, &g, "/ns", &reply(S0(), &TEXT(), "end_turn", u4(1, 0, 5000, 1)), "plan", &compaction);
    assert_eq!(windows(&t, &g), before);
    assert_eq!(t.h.session(&g.session).unwrap().status().used_context, Some(151000));
}

#[test]
fn status_ignores_non_2xx_reply() { // AU29: only a 2xx main-loop reply stores
    let t = setup("claude-t-1");
    let g = grant(&t, Agent::ClaudeCode);
    run(&t, &g, "/b", &reply(S0(), &TEXT(), "end_turn", U15()), "triage", &[MAIN]);
    let before = windows(&t, &g);
    // The 400 body carries usage, so the prompt is > 0 and only the 2xx test keeps it out.
    let mut r = json_reply(json!({"type":"error","error":{"type":"invalid_request_error","message":"bad"},"usage":{"input_tokens":5000}}));
    r.status = 400;
    t.up.script("/ns", vec![r]);
    let (s, _, b) = post(&t, &g, "plan", &[MAIN]);
    assert_eq!(s, 400, "{}", String::from_utf8_lossy(&b));
    assert_eq!(windows(&t, &g), before);
    let row = last(&t, &g);
    assert_eq!((row.status, tokens(&row).0), (400, 5000));
    assert_eq!(t.h.session(&g.session).unwrap().status().used_context, Some(151000));
}

#[test]
fn status_ignores_count_tokens() { // AU29: requests on other routes store nothing
    let t = setup("claude-t-1");
    let g = grant(&t, Agent::ClaudeCode);
    run(&t, &g, "/b", &reply(S0(), &TEXT(), "end_turn", U15()), "triage", &[MAIN]);
    let before = windows(&t, &g);
    // The reply carries usage, so the prompt is > 0 and only the route keeps it out.
    t.up.script("/ns", vec![json_reply(json!({"input_tokens":5000,"usage":{"input_tokens":5000}}))]);
    let mut hd = auth(&g);
    hd.push(MAIN);
    t.n.set(t.n.get() + 1);
    let body = json!({"model":"plan","messages":[{"role":"user","content":"hi"}]}).to_string().into_bytes();
    let (s, _, b) = send(&t.h, "POST", "anthropic/v1/messages/count_tokens", &hd, Some(body));
    assert_eq!(s, 200, "{}", String::from_utf8_lossy(&b));
    assert_eq!(windows(&t, &g), before);
    let r = last(&t, &g);
    assert_eq!((r.route.as_str(), tokens(&r).0), ("/anthropic/v1/messages/count_tokens", 5000));
    assert_eq!(t.h.session(&g.session).unwrap().status().used_context, Some(151000));
}

#[test]
fn status_follows_non_streamed_request() { // AU29: the condition does not test streaming
    let t = setup("claude-t-1");
    let g = grant(&t, Agent::ClaudeCode);
    let m = json!({"id":"msg_1","type":"message","role":"assistant","model":"gpt-t-1",
        "content":[{"type":"text","text":"ok"}],"stop_reason":"end_turn","stop_sequence":null,
        "usage":{"input_tokens":1,"cache_creation_input_tokens":0,"cache_read_input_tokens":5000,"output_tokens":5}});
    t.up.script("/g", vec![json_reply(m)]);
    let body = json!({"model":"code","max_tokens":64,"stream":false,"messages":[{"role":"user","content":"hi"}]});
    let (s, _, b) = post_body(&t, &g, &body, &[MAIN]);
    assert_eq!(s, 200, "{}", String::from_utf8_lossy(&b));
    assert_eq!(windows(&t, &g), (Some("gpt-t-1".into()), Some(400000), Some(360000), None));
    assert_eq!(t.h.session(&g.session).unwrap().status().used_context, Some(5001));
}

#[test]
fn status_follows_dropped_stream() { // AU29: stored under the used_context condition, with no message_delta
    let t = setup("claude-t-1");
    let g = grant(&t, Agent::ClaudeCode);
    let start = json!({"message":{"id":"msg_1","type":"message","role":"assistant","content":[],"usage":u4(1, 0, 5000, 1)}});
    let body = sse(&[("message_start", start)]);
    let mut r = sse_reply(&body);
    r.pings = true;
    t.up.script("/ns", vec![r]);
    // The client closes once it has the whole `message_start` data line: the proxy has read its usage.
    read_then_drop(&t, &g, "plan", &[MAIN], data_line(&body));
    assert_eq!(windows(&t, &g), (Some("claude-ns".into()), Some(200000), None, None));
    assert_eq!(t.h.session(&g.session).unwrap().status().used_context, Some(5001));
    assert_eq!(last(&t, &g).status, 200);
}

#[test]
fn status_follows_opencode_request() { // AU29: OpenCode stores its model with agent_window(row)
    let t = setup("claude-t-1");
    let o = grant(&t, Agent::OpenCode);
    assert_eq!(windows(&t, &o), (Some("claude-t-1".into()), Some(200000), Some(150000), Some(150000)));
    run(&t, &o, "/g", &reply(S0(), &TEXT(), "end_turn", U15()), "code", &[]);
    assert_eq!(windows(&t, &o), (Some("gpt-t-1".into()), Some(400000), Some(360000), None));
}

/// A passthrough Messages request for `name`, answered by the Anthropic fake upstream.
fn passthrough_request(t: &T, g: &SessionGrant, name: &str) {
    t.anth.script("", vec![sse_reply(&reply(S0(), &TEXT(), "end_turn", u4(1, 0, 5000, 1)))]);
    let mut hd = pt(g);
    hd.extend([("authorization", OAT_BEARER), MAIN]);
    t.n.set(t.n.get() + 1);
    let (s, _, b) = send(&t.h, "POST", M, &hd, Some(req_body(name).to_string().into_bytes()));
    assert_eq!(s, 200, "{}", String::from_utf8_lossy(&b));
}

#[test]
fn status_passthrough_no_model_until_served() { // AU29, H§7.2 `model`
    let t = setup("claude-t-1");
    let g = grant_with(&t, Agent::ClaudeCode, true);
    assert_eq!(windows(&t, &g), (None, None, None, None));
    passthrough_request(&t, &g, "claude-t-1");
    assert_eq!(windows(&t, &g), (Some("claude-t-1".into()), Some(200000), Some(150000), Some(150000)));
}

#[test]
fn status_model_name_capped() { // author's call (HUD-5 review): at most 128 bytes, cut on a char boundary
    let t = setup("claude-t-1");
    let g = grant_with(&t, Agent::ClaudeCode, true);
    passthrough_request(&t, &g, &format!("x{}", "é".repeat(100))); // 201 bytes, no catalog row
    let w = windows(&t, &g);
    assert_eq!(w, (Some(format!("x{}", "é".repeat(63))), None, None, None)); // byte 128 is inside an `é`: 127 bytes
    assert_eq!(w.0.unwrap().len(), 127);
}

#[test]
fn status_ignores_zero_prompt_reply() { // AU29 (HUD-5 round 2, F3): a 2xx main-loop reply with prompt 0 stores nothing
    let t = setup("claude-t-1");
    let g = grant(&t, Agent::ClaudeCode);
    run(&t, &g, "/b", &reply(S0(), &TEXT(), "end_turn", U15()), "triage", &[MAIN]);
    let before = windows(&t, &g);
    assert_eq!(before, (Some("claude-big".into()), Some(2000000), Some(900000), Some(900000)));
    // message_start and message_delta with zero usage: the prompt is 0.
    run(&t, &g, "/ns", &reply(u4(0, 0, 0, 0), &TEXT(), "end_turn", u4(0, 0, 0, 0)), "plan", &[MAIN]);
    assert_eq!(windows(&t, &g), before);
    assert_eq!(last(&t, &g).status, 200);
    assert_eq!(t.h.session(&g.session).unwrap().status().used_context, Some(151000));
    // A stream dropped before any usage: only `ping` events reach the client.
    let body = sse(&[("ping", json!({}))]);
    let mut r = sse_reply(&body);
    r.pings = true;
    t.up.script("/ns", vec![r]);
    read_then_drop(&t, &g, "plan", &[MAIN], data_line(&body));
    assert_eq!(windows(&t, &g), before);
    let row = last(&t, &g);
    assert_eq!((row.status, tokens(&row).0), (200, 0));
    assert_eq!(t.h.session(&g.session).unwrap().status().used_context, Some(151000));
}

// ---- HUD-8: H§14 #8, proxy part ----

#[test]
fn h14_08_status_windows_follow_the_served_request() { // H§14 #8 (proxy part): AU28, AU29, S3
    let t = setup("claude-t-1");
    let used = |g: &SessionGrant| t.h.session(&g.session).unwrap().status().used_context;
    // A prompt of 150,000 tokens.
    let u150 = reply(S0(), &TEXT(), "end_turn", u4(1000, 9000, 140000, 5));

    // Passthrough before the first request: no `model` in the status.
    let p = grant_with(&t, Agent::ClaudeCode, true);
    assert_eq!(windows(&t, &p), (None, None, None, None));

    // `[1m]`: role `plan` on a 200k Claude model with no soft window, called as `plan[1m]`, used 150,000. The
    // status has `context_window` 200,000 and `effective_window` 180,000 (the safety window for agent window
    // 1,000,000), not none, and no `threshold`: the bar fraction is 150/180.
    let g = grant(&t, Agent::ClaudeCode);
    run(&t, &g, "/ns", &u150, "plan[1m]", &[MAIN]);
    assert_eq!(windows(&t, &g), (Some("claude-ns".into()), Some(200000), Some(180000), None));
    assert_eq!(used(&g), Some(150000));
    // The same with an `anthropic-beta` token `context-1m-…`.
    let b = grant(&t, Agent::ClaudeCode);
    let beta = q7_beta("fable");
    run(&t, &b, "/ns", &u150, "plan", &[MAIN, ("anthropic-beta", beta.as_str())]);
    assert_eq!(windows(&t, &b), (Some("claude-ns".into()), Some(200000), Some(180000), None));
    assert_eq!(used(&b), Some(150000));

    // Soft 190,000 with safety 180,000 (model window 200,000): `threshold` 180,000.
    compact(&t, &g, json!(190000));
    assert_eq!(windows(&t, &g), (Some("claude-ns".into()), Some(200000), Some(180000), Some(180000)));
    // S3: after `zygo compact 150k` with no new request, the next status has `threshold` 150,000.
    let before = t.n.get();
    compact(&t, &g, json!(150000));
    assert_eq!(windows(&t, &g), (Some("claude-ns".into()), Some(200000), Some(150000), Some(150000)));
    assert_eq!(t.n.get(), before);
    // The override `off`: no `threshold`; the safety window alone is the effective window.
    compact(&t, &g, json!("off"));
    assert_eq!(windows(&t, &g), (Some("claude-ns".into()), Some(200000), Some(180000), None));

    // AU29: after a main-loop request served by another role's model (window 400,000), the status `model` and
    // `context_window` are that model's.
    run(&t, &g, "/g", &u150, "code", &[MAIN]);
    assert_eq!(windows(&t, &g), (Some("gpt-t-1".into()), Some(400000), Some(360000), None));

    // A non-streamed main-loop request stores model and agent window like a streamed one.
    let m = json!({"id":"msg_1","type":"message","role":"assistant","model":"claude-big",
        "content":[{"type":"text","text":"ok"}],"stop_reason":"end_turn","stop_sequence":null,
        "usage":{"input_tokens":1,"cache_creation_input_tokens":0,"cache_read_input_tokens":5000,"output_tokens":5}});
    t.up.script("/b", vec![json_reply(m)]);
    let body = json!({"model":"triage","max_tokens":64,"stream":false,"messages":[{"role":"user","content":"hi"}]});
    let (s, _, bytes) = post_body(&t, &g, &body, &[MAIN]);
    assert_eq!(s, 200, "{}", String::from_utf8_lossy(&bytes));
    // The override is still `off`, and the agent window of this request is the model window: no safety window.
    let big = (Some("claude-big".to_string()), Some(2000000), None, None);
    assert_eq!(windows(&t, &g), big);
    assert_eq!(used(&g), Some(5001));

    // A request on another route leaves the stored values unchanged.
    t.up.script("/g", vec![json_reply(json!({"input_tokens":5000,"usage":{"input_tokens":5000}}))]);
    let mut hd = auth(&g);
    hd.push(MAIN);
    t.n.set(t.n.get() + 1);
    let count = json!({"model":"code","messages":[{"role":"user","content":"hi"}]}).to_string().into_bytes();
    let (s, _, bytes) = send(&t.h, "POST", "anthropic/v1/messages/count_tokens", &hd, Some(count));
    assert_eq!(s, 200, "{}", String::from_utf8_lossy(&bytes));
    assert_eq!(windows(&t, &g), big);
    assert_eq!(last(&t, &g).route, "/anthropic/v1/messages/count_tokens");

    // An OpenCode request on `/anthropic/v1/messages` stores its model with `agent_window(row)`.
    let o = grant(&t, Agent::OpenCode);
    run(&t, &o, "/g", &u150, "code", &[]);
    assert_eq!(windows(&t, &o), (Some("gpt-t-1".into()), Some(400000), Some(360000), None));
}
