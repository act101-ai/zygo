//! N11: proxy request path (§7.3 stages 2, 4, 5 and the proxy step; §7.4, §7.5, §7.7, §10.4, §11.4, §14).

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fmt;
use std::io::{Read, Write};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::extract::Request;
use axum::response::Response;
use bytes::Bytes;
use http::HeaderMap;
use serde_json::{json, Value};
use tempfile::TempDir;
use zygo_config::keys::Secret;
use zygo_config::profile::Profile;
use zygo_config::{load, CustomRole, Dialect, KeyRef, LoadContext, Merged, Provider, ProxyConfig};
use zygo_core::ids::ulid;
use zygo_core::paths::Paths;
use zygo_core::Agent;
use zygo_proxy::forward::{outbound_headers, response_headers, upstream_url, BUSY_WAIT, INFLIGHT_FACTOR};
use zygo_proxy::pipeline::Adjustment;
use zygo_proxy::provider::{upstream_allowed, Candidate, KeyResolver};
use zygo_proxy::redact_stage::RedactLimits;
use zygo_proxy::resolve::resolve;
use zygo_proxy::server::StartOpts;
use zygo_proxy::session::{Clock, ManualClock, SessionGrant, SessionRequest};
use zygo_proxy::tee::{Tee, Usage, HOLD_MAX, USAGE_OFF_NOTE};
use zygo_proxy::{Proxy, ProxyHandle};
use zygo_redact::DeadlinePolicy;
use zygo_usage::{RequestRow, Store};

// ---- helpers ----

const M: &str = "anthropic/v1/messages";
const CT: &str = "anthropic/v1/messages/count_tokens";
const HOUR: Duration = Duration::from_secs(3600);
const SSE: &str = concat!(
    "event: message_start\n",
    r#"data: {"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","content":[],"usage":{"input_tokens":120,"cache_read_input_tokens":30,"cache_creation_input_tokens":7,"output_tokens":1}}}"#,
    "\n\n",
    "event: message_delta\n",
    r#"data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":42}}"#,
    "\n\n",
    "event: message_stop\n",
    r#"data: {"type":"message_stop"}"#,
    "\n\n",
);
const U: Usage = Usage { input: 120, output: 42, cache_read: 30, cache_write: 7 };
const ERR: &str = r#"{"type":"error","error":{"type":"invalid_request_error","message":"upstream"}}"#;

#[derive(Clone)]
enum Reply {
    /// 200 text/event-stream, SSE in 3 chunks; with headers the proxy must drop or keep.
    Sse,
    /// The status with body ERR.
    Status(u16),
    /// 200 application/json.
    Json(Value),
    /// 200 text/event-stream: the first event, then nothing.
    Hang,
}

#[derive(Clone)]
struct Got {
    /// With the query.
    path: String,
    headers: HeaderMap,
    body: Vec<u8>,
}

/// Path and header names only.
impl fmt::Debug for Got {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Got").field("path", &self.path).field("headers", &self.headers.keys().collect::<Vec<_>>()).finish()
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

async fn upstream_handler(st: Arc<UpState>, req: Request) -> Response {
    let path = req.uri().path_and_query().map(|p| p.as_str().to_string()).unwrap_or_default();
    let headers = req.headers().clone();
    let body = axum::body::to_bytes(req.into_body(), usize::MAX).await.unwrap().to_vec();
    let p = prefix(&path);
    st.got.lock().unwrap().push(Got { path, headers, body });
    let reply = st.script.lock().unwrap().get_mut(&p).and_then(|q| q.pop_front()).unwrap_or(Reply::Sse);
    let b = Response::builder();
    match reply {
        Reply::Sse => {
            let parts: Vec<Result<Bytes, std::io::Error>> =
                [&SSE[..40], &SSE[40..200], &SSE[200..]].iter().map(|s| Ok(Bytes::copy_from_slice(s.as_bytes()))).collect();
            b.status(200)
                .header("content-type", "text/event-stream")
                .header("x-request-id", "r1")
                .header("openai-organization", "org-test")
                .header("location", "http://upstream.test/x")
                .header("set-cookie", "s=1")
                .header("x-zygo-model", "forged")
                .body(Body::from_stream(futures_util::stream::iter(parts)))
                .unwrap()
        }
        Reply::Status(s) => b.status(s).header("content-type", "application/json").body(Body::from(ERR)).unwrap(),
        Reply::Json(v) => b.status(200).header("content-type", "application/json").body(Body::from(to_vec(&v))).unwrap(),
        Reply::Hang => {
            let first = SSE.split("\n\n").next().unwrap().to_string() + "\n\n";
            let s = futures_util::stream::iter([Ok::<_, std::io::Error>(Bytes::from(first))])
                .chain(futures_util::stream::pending());
            b.status(200).header("content-type", "text/event-stream").body(Body::from_stream(s)).unwrap()
        }
    }
}

use futures_util::StreamExt;

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
    fn port(&self) -> u16 {
        self.addr.port()
    }
    fn script(&self, prefix: &str, r: Vec<Reply>) {
        self.st.script.lock().unwrap().insert(prefix.to_string(), r.into());
    }
    fn got(&self) -> Vec<Got> {
        self.st.got.lock().unwrap().clone()
    }
    fn count(&self, p: &str) -> usize {
        self.got().iter().filter(|g| prefix(&g.path) == p).count()
    }
    /// The last request under `p`.
    fn at(&self, p: &str) -> Got {
        self.got().into_iter().filter(|g| prefix(&g.path) == p).last().unwrap_or_else(|| panic!("no request at {p:?}"))
    }
}

/// A port bound on 127.0.0.1 and released: a connect is refused at once. (A bound socket that does not listen is
/// refused on Linux, but on macOS its SYN is dropped and the connect times out after about 8 s.)
fn refused_port() -> u16 {
    std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap().local_addr().unwrap().port()
}

/// Accepts, reads the head and `content-length` bytes, counts, closes without a reply.
fn closing_upstream() -> (u16, Arc<AtomicUsize>) {
    let l = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = l.local_addr().unwrap().port();
    let n = Arc::new(AtomicUsize::new(0));
    let n2 = n.clone();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { return };
            let mut buf = Vec::new();
            let mut b = [0u8; 4096];
            let head_end = loop {
                let k = s.read(&mut b).unwrap_or(0);
                if k == 0 {
                    break None;
                }
                buf.extend_from_slice(&b[..k]);
                if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    break Some(i + 4);
                }
            };
            let Some(end) = head_end else { continue };
            let head = String::from_utf8_lossy(&buf[..end]).to_ascii_lowercase();
            let len: usize = head
                .lines()
                .find_map(|l| l.strip_prefix("content-length:"))
                .map(|v| v.trim().parse().unwrap())
                .unwrap_or(0);
            while buf.len() - end < len {
                let k = s.read(&mut b).unwrap_or(0);
                if k == 0 {
                    break;
                }
                buf.extend_from_slice(&b[..k]);
            }
            n2.fetch_add(1, Ordering::SeqCst);
            let _ = s.flush();
            drop(s);
        }
    });
    (port, n)
}

/// Per connection: reads the head and `content-length` bytes, waits `delay`, writes `resp` raw, closes.
fn raw_upstream(resp: Vec<u8>, delay: Duration) -> (u16, Arc<AtomicUsize>) {
    let l = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = l.local_addr().unwrap().port();
    let n = Arc::new(AtomicUsize::new(0));
    let n2 = n.clone();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { return };
            let (resp, n3) = (resp.clone(), n2.clone());
            std::thread::spawn(move || {
                let head = read_head(&mut s);
                n3.fetch_add(1, Ordering::SeqCst);
                if head.is_some() {
                    std::thread::sleep(delay);
                    let _ = s.write_all(&resp);
                    let _ = s.flush();
                }
            });
        }
    });
    (port, n)
}

/// Per connection: reads the head and `content-length` body bytes, writes `head`, then each part with a flush and a
/// `gap` sleep after it, then closes.
fn drip_upstream(head: &'static str, parts: Vec<Vec<u8>>, gap: Duration) -> u16 {
    let l = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { return };
            let parts = parts.clone();
            std::thread::spawn(move || {
                if read_head(&mut s).is_none() {
                    return;
                }
                let _ = s.write_all(head.as_bytes());
                for p in parts {
                    let _ = s.write_all(&p).and_then(|()| s.flush());
                    std::thread::sleep(gap);
                }
            });
        }
    });
    port
}

/// The request head (lower case) after reading the head and `content-length` body bytes; `None` on EOF first.
fn read_head(s: &mut std::net::TcpStream) -> Option<String> {
    let mut buf = Vec::new();
    let mut b = [0u8; 65536];
    let end = loop {
        let k = s.read(&mut b).unwrap_or(0);
        if k == 0 {
            return None;
        }
        buf.extend_from_slice(&b[..k]);
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
    };
    let head = String::from_utf8_lossy(&buf[..end]).to_string();
    let len: usize =
        head.to_ascii_lowercase().lines().find_map(|l| l.strip_prefix("content-length:")).map(|v| v.trim().parse().unwrap()).unwrap_or(0);
    while buf.len() - end < len {
        let k = s.read(&mut b).unwrap_or(0);
        if k == 0 {
            break;
        }
        buf.extend_from_slice(&b[..k]);
    }
    Some(String::from_utf8_lossy(&buf).to_string())
}

/// A fake forward proxy: records each request (head and body), answers 502 and closes.
fn fake_proxy() -> (String, Arc<Mutex<Vec<String>>>) {
    let l = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let url = format!("http://127.0.0.1:{}", l.local_addr().unwrap().port());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let s2 = seen.clone();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { return };
            if let Some(h) = read_head(&mut s) {
                s2.lock().unwrap().push(h);
            }
            let _ = s.write_all(b"HTTP/1.1 502 Bad Gateway\r\ncontent-length: 0\r\nconnection: close\r\n\r\n");
        }
    });
    (url, seen)
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

impl std::ops::Index<&str> for TestKeys {
    type Output = Secret;
    fn index(&self, n: &str) -> &Secret {
        &self.0[n]
    }
}

/// Built at run time (no credential-shaped literal).
fn keys() -> TestKeys {
    TestKeys(["k-code", "k-alt", "k-oai", "k-or"].iter().map(|n| (n.to_string(), Secret::new(format!("test-key-{n}-{}", ulid())))).collect())
}

/// One `[[model.provider]]` block; `more` adds lines (flags, enabled, forward_headers).
fn prov(name: &str, url: &str, upstream: &str, key: Option<&str>, more: &str) -> String {
    let key = key.map(|k| format!("key = \"keychain:{k}\"\n")).unwrap_or_default();
    format!(
        "[[model.provider]]\nname = \"{name}\"\nbase_url = \"{url}\"\ndialects = [\"anthropic\"]\nupstream_model = \"{upstream}\"\n{key}{more}\n"
    )
}

fn model(id: &str, provs: &[String]) -> String {
    format!("[[model]]\nid = \"{id}\"\n{}\n", provs.concat())
}

/// User-layer config: role `code` → test-code; test-alt (with_alt), test-same, test-oai, test-none, test-fb.
fn base(u: &str, with_alt: bool) -> String {
    let prices = "[model.provider.price_usd_per_mtok]\ninput = 4.0\noutput = 20.0\ncache_read = 0.2\ncache_write = 8.0\n";
    let mut t = String::from("[role.code]\nmodel = \"test-code\"\n\n");
    t += &model("test-code", &[prov("p-code", u, "up-code", Some("k-code"), prices)]);
    if with_alt {
        t += &model("test-alt", &[prov("p-alt", &format!("{u}/alt"), "up-alt", Some("k-alt"), "")]);
    }
    t += &model("test-same", &[prov("p-same", u, "test-same", None, "")]);
    t += &model(
        "test-oai",
        &[prov("p-oai", &format!("{u}/oai"), "up-oai", Some("k-oai"), "").replace("[\"anthropic\"]", "[\"openai\"]")],
    );
    t += "[[model]]\nid = \"test-none\"\n\n";
    t += &model(
        "test-fb",
        &["a", "b", "c"].map(|n| prov(n, &format!("{u}/{n}"), "up-fb", None, "")),
    );
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

fn opts(d: &TempDir, c: Arc<ManualClock>, k: &TestKeys) -> StartOpts {
    let hosts = ["openrouter.ai", "eu.openrouter.ai", "notopenrouter.ai"];
    StartOpts {
        clock: c,
        keys: Arc::new(TestKeys(k.0.clone())),
        usage_db: Some(d.path().join("data/usage.db")),
        upstream_test: Some(hosts.iter().map(|h| (h.to_string(), SocketAddr::from(([127, 0, 0, 1], 0)))).collect()),
        redact_limits: Some(generous()),
        ..Default::default()
    }
}

/// A pass that must finish never hits its deadline (a debug build under load must not trip the 200 ms default).
fn generous() -> RedactLimits {
    RedactLimits {
        secret: DeadlinePolicy { base: Duration::from_secs(60), per_mib: Duration::ZERO, expire_after_scans: None },
        content_budget: Duration::from_secs(60),
        ..Default::default()
    }
}

fn start(cfg: Merged, o: StartOpts) -> ProxyHandle {
    Proxy::start_local_with(Arc::new(cfg), o).unwrap()
}

struct T {
    d: TempDir,
    up: Upstream,
    c: Arc<ManualClock>,
    k: TestKeys,
    h: ProxyHandle,
}

/// `base(up, true)` + `extra(up.url(), up.port())`; `tweak` edits the options.
fn setup_with(extra: impl FnOnce(&str, u16) -> String, tweak: impl FnOnce(&mut StartOpts, &TempDir)) -> T {
    let d = TempDir::new().unwrap();
    let up = Upstream::start();
    let c = ManualClock::new();
    let k = keys();
    let toml = base(&up.url(), true) + &extra(&up.url(), up.port());
    let mut o = opts(&d, c.clone(), &k);
    tweak(&mut o, &d);
    let h = start(cfg(&d, &toml), o);
    T { d, up, c, k, h }
}

fn setup(extra: impl FnOnce(&str, u16) -> String) -> T {
    setup_with(extra, |_, _| {})
}

fn none(_: &str, _: u16) -> String {
    String::new()
}

fn session(h: &ProxyHandle, agent: Agent, profile: Option<Profile>) -> SessionGrant {
    h.create_session(SessionRequest {
        agent,
        agent_version: "2.1.328".into(),
        profile,
        passthrough: false,
        repo: None,
        work: Some("ENG-42".into()),
        stored_values: Vec::new(),
        known_plugins: Default::default(),
    })
    .unwrap()
}

fn p1() -> Profile {
    Profile {
        name: "p1".into(),
        overrides: BTreeMap::from([("code".to_string(), "test-alt".to_string())]),
        disabled: vec!["design".into()],
        roles: BTreeMap::from([(
            "bench".to_string(),
            CustomRole {
                model: "test-alt".into(),
                description: "Runs benchmarks.".into(),
                prompt: "You run benchmarks.".into(),
                writes: false,
            },
        )]),
        ..Default::default()
    }
}

fn msg(model: &str) -> Vec<u8> {
    to_vec(&json!({"model":model,"max_tokens":64,"stream":true,"messages":[{"role":"user","content":"hello"}]}))
}

fn big(n: usize) -> Vec<u8> {
    to_vec(&json!({"model":"code","max_tokens":64,"stream":true,"messages":[{"role":"user","content":"x".repeat(n)}]}))
}

fn to_vec(v: &Value) -> Vec<u8> {
    serde_json::to_vec(v).unwrap()
}

fn body_json(g: Got) -> Value {
    serde_json::from_slice(&g.body).unwrap()
}

fn http_client() -> reqwest::Client {
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::builder().no_proxy().build().unwrap()
}

/// A request to the proxy with exactly `headers` (plus content-type when a body is sent).
fn send(h: &ProxyHandle, method: &str, path: &str, headers: &[(&str, &str)], body: Option<reqwest::Body>) -> (u16, HeaderMap, Vec<u8>) {
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

fn post(h: &ProxyHandle, g: &SessionGrant, path: &str, body: Vec<u8>, extra: &[(&str, &str)]) -> (u16, HeaderMap, Vec<u8>) {
    let mut hd = auth(g);
    hd.extend_from_slice(extra);
    send(h, "POST", path, &hd, Some(body.into()))
}

/// Content-length set.
fn post_cl(h: &ProxyHandle, g: &SessionGrant, body: Vec<u8>) -> (u16, HeaderMap, Vec<u8>) {
    post(h, g, M, body, &[])
}

/// No content-length: 64 KiB chunks.
fn post_chunked(h: &ProxyHandle, g: &SessionGrant, body: Vec<u8>) -> (u16, HeaderMap, Vec<u8>) {
    let chunks: Vec<Result<Bytes, std::io::Error>> = body.chunks(64 << 10).map(|c| Ok(Bytes::copy_from_slice(c))).collect();
    send(h, "POST", M, &auth(g), Some(reqwest::Body::wrap_stream(futures_util::stream::iter(chunks))))
}

/// Asserts the Anthropic error shape; (status, error.type).
fn st(r: (u16, HeaderMap, Vec<u8>)) -> (u16, String) {
    let v: Value = serde_json::from_slice(&r.2).unwrap_or_else(|_| panic!("not JSON: {}", String::from_utf8_lossy(&r.2)));
    assert_eq!(v["type"], "error", "{v}");
    assert!(v["error"]["message"].is_string(), "{v}");
    (r.0, v["error"]["type"].as_str().unwrap().to_string())
}

fn rows(t: &T, g: &SessionGrant, n: u64) -> Vec<RequestRow> {
    assert!(t.h.usage_written(n, Duration::from_secs(10)), "usage rows not written");
    Store::open(&t.d.path().join("data/usage.db")).unwrap().requests_for_session(&g.session).unwrap()
}

fn names(v: &[Provider]) -> Vec<&str> {
    v.iter().map(|p| p.name.as_str()).collect()
}

fn sorted_names(h: &HeaderMap) -> Vec<String> {
    let mut v: Vec<String> = h.keys().map(|k| k.as_str().to_string()).collect();
    v.sort();
    v
}

fn hv<'a>(h: &'a HeaderMap, n: &str) -> &'a str {
    h.get(n).unwrap_or_else(|| panic!("no header {n}")).to_str().unwrap()
}

fn pin(t: &T, g: &SessionGrant, model: &str) -> Option<String> {
    t.h.session(&g.session).unwrap().pins.lock().unwrap().get(model).cloned()
}

fn models(t: &T, g: &SessionGrant) -> Value {
    let r = send(&t.h, "GET", "anthropic/v1/models", &auth(g), None);
    assert_eq!(r.0, 200);
    serde_json::from_slice(&r.2).unwrap()
}

fn idx(v: &Value, id: &str) -> usize {
    v["data"].as_array().unwrap().iter().position(|m| m["id"] == id).unwrap_or_else(|| panic!("no {id}"))
}

fn item<'a>(v: &'a Value, id: &str) -> &'a Value {
    &v["data"][idx(v, id)]
}

fn ids(v: &Value) -> Vec<String> {
    v["data"].as_array().unwrap().iter().map(|m| m["id"].as_str().unwrap().to_string()).collect()
}

/// The client headers of test 4: every kind the proxy must drop or keep.
fn client_headers(bearer: &str) -> HeaderMap {
    let mut c = HeaderMap::new();
    for (k, v) in [
        ("authorization", bearer),
        ("x-api-key", "client-key"),
        ("x-zygo-session", "s1"),
        ("x-claude-code-request-class", "main"),
        ("anthropic-beta", "b1"),
        ("anthropic-version", "2023-06-01"),
        ("content-type", "application/json"),
        ("accept", "application/json"),
        ("x-stainless-os", "x"),
    ] {
        c.insert(k, v.parse().unwrap());
    }
    c
}

// ---- tee (§14, §10.2.6) ----

#[test]
fn tee_passes_chunks_and_reads_usage() {
    for size in [1, 7, 4096] {
        let mut t = Tee::new(true);
        let (mut out, mut n) = (vec![], 0);
        for c in SSE.as_bytes().chunks(size) {
            out.extend(t.push(Bytes::copy_from_slice(c), &mut |_| n += 1));
        }
        out.extend(t.finish(&mut |_| n += 1));
        assert_eq!((out.as_slice(), n, t.usage()), (SSE.as_bytes(), 0, U), "chunk size {size}");
    }
    let mut t = Tee::new(false);
    let b = r#"{"usage":{"input_tokens":5,"output_tokens":6,"cache_read_input_tokens":7,"cache_creation_input_tokens":8}}"#;
    assert_eq!(t.push(Bytes::from(b), &mut |_| {}), Bytes::from(b));
    t.finish(&mut |_| {});
    assert_eq!(t.usage(), Usage { input: 5, output: 6, cache_read: 7, cache_write: 8 });
    // `message_delta` usage: per-field maxima, a lower value does not replace a higher one (§14).
    let lower = SSE.replace(r#""usage":{"output_tokens":42}"#, r#""usage":{"input_tokens":3,"cache_read_input_tokens":0,"output_tokens":42}"#);
    assert_ne!(lower, SSE);
    let mut t = Tee::new(true);
    t.push(Bytes::from(lower), &mut |_| {});
    t.finish(&mut |_| {});
    assert_eq!(t.usage(), U);
}

#[test]
fn tee_line_mode_whole_lines_and_large_line() {
    let mut t = Tee::new(true);
    t.arm_line_mode();
    assert!(t.line_mode());
    let (mut lines, mut out) = (vec![], vec![]);
    for c in SSE.as_bytes().chunks(7) {
        out.extend(t.push(Bytes::copy_from_slice(c), &mut |l| lines.push(l.0.clone())));
    }
    out.extend(t.finish(&mut |_| {}));
    assert_eq!(out, SSE.as_bytes());
    assert_eq!(t.usage(), U);
    assert_eq!(lines, SSE.split_terminator('\n').map(|l| l.as_bytes().to_vec()).collect::<Vec<_>>());

    let mut t = Tee::new(true);
    t.arm_line_mode();
    let mut out = vec![];
    let mut rw = |l: &mut zygo_proxy::tee::SseLine| {
        if l.0 == br#"data: {"type":"message_stop"}"# {
            l.0 = b"data: {}".to_vec();
        }
    };
    for c in SSE.as_bytes().chunks(5) {
        out.extend(t.push(Bytes::copy_from_slice(c), &mut rw));
    }
    out.extend(t.finish(&mut rw));
    assert!(out.windows(9).any(|w| w == b"data: {}\n"));
    assert_eq!(t.usage(), U);

    let mut input = vec![b'x'; HOLD_MAX + 1];
    input.extend_from_slice(b"\ndata: y\n");
    let mut t = Tee::new(true);
    t.arm_line_mode();
    let (mut lines, mut out) = (vec![], vec![]);
    for c in input.chunks(64 << 10) {
        out.extend(t.push(Bytes::copy_from_slice(c), &mut |l| lines.push(l.0.clone())));
    }
    out.extend(t.finish(&mut |l| lines.push(l.0.clone())));
    assert!(out == input, "large line changed");
    assert_eq!(lines, vec![b"data: y".to_vec()]);

    let mut n = Tee::new(false);
    n.arm_line_mode();
    assert!(!n.line_mode());
}

// ---- resolution (§7.4, §11.3, §10.4) ----

#[test]
fn stale_override_falls_back_to_role_model() {
    let t = setup(none);
    let s = t.h.session(&session(&t.h, Agent::ClaudeCode, Some(p1())).session).unwrap();
    let r = resolve("code", Dialect::Anthropic, &s, &s.cfg).unwrap();
    assert_eq!((r.role.as_str(), r.model.as_str(), r.adjustments.len()), ("code", "test-alt", 0));
    let d2 = TempDir::new().unwrap();
    let r = resolve("code", Dialect::Anthropic, &s, &cfg(&d2, &base(&t.up.url(), false))).unwrap();
    assert_eq!((r.model.as_str(), r.adjustments.clone()), ("test-code", vec![Adjustment("profile_override_stale".into())]));
    let r = resolve("code[1m]", Dialect::Anthropic, &s, &s.cfg).unwrap();
    assert_eq!((r.requested.as_str(), r.one_m), ("code", true));
    s.pins.lock().unwrap().insert("test-fb".into(), "b".into());
    assert_eq!(names(&resolve("test-fb", Dialect::Anthropic, &s, &s.cfg).unwrap().candidates), ["b", "a", "c"]);
}

// ---- headers (§7.3, §20 #13a function part, C3) ----

#[test]
fn headers_unit_openai_dialect() {
    let k = Secret::new(format!("test-key-unit-{}", ulid()));
    let bearer = format!("Bearer {}", ulid());
    let c = client_headers(&bearer);
    let p = Provider { name: "p".into(), dialects: vec![Dialect::Anthropic], ..Default::default() };
    let p_oai = Provider { name: "o".into(), dialects: vec![Dialect::Openai], ..Default::default() };
    let o = outbound_headers(&c, &p_oai, Dialect::Openai, Some(&k));
    assert_eq!(sorted_names(&o), ["accept", "authorization", "content-type"]);
    assert_eq!(hv(&o, "authorization"), format!("Bearer {}", k.expose()));
    assert!(o["authorization"].is_sensitive());
    let a = outbound_headers(&c, &p, Dialect::Anthropic, Some(&k));
    assert_eq!(sorted_names(&a), ["accept", "anthropic-beta", "anthropic-version", "content-type", "x-api-key"]);
    assert_eq!(hv(&a, "x-api-key"), k.expose());
    assert!(a["x-api-key"].is_sensitive());
    assert!(outbound_headers(&c, &p, Dialect::Anthropic, None).get("x-api-key").is_none());
    assert!(!format!("{:?}", Candidate { provider: p, dialect: Dialect::Anthropic, key: Some(k.clone()) }).contains(k.expose()));
}

#[test]
fn headers_unit_url_and_transport() {
    let u = |b: &str, q: Option<&str>| upstream_url(b, "/v1/messages", q).unwrap().as_str().to_string();
    assert_eq!(u("http://127.0.0.1:9", Some("beta=true")), "http://127.0.0.1:9/v1/messages?beta=true");
    assert_eq!(u("https://h.test/p/?x=1", Some("beta=true")), "https://h.test/p/v1/messages?x=1&beta=true");
    assert_eq!(u("https://h.test/p?x=1", None), "https://h.test/p/v1/messages?x=1");
    for ok in ["https://api.example.test", "http://127.0.0.1:9", "http://127.8.0.1", "http://[::1]:9", "http://localhost:9"] {
        assert!(upstream_allowed(ok), "{ok}");
    }
    for no in ["http://10.0.0.5:9", "http://api.example.test", "http://127.0.0.1.example.test", "ftp://127.0.0.1"] {
        assert!(!upstream_allowed(no), "{no}");
    }
}

#[test]
fn headers_unit_response_dropped() {
    let mut u = HeaderMap::new();
    for (k, v) in [
        ("connection", "close, x-hop"),
        ("x-hop", "v"),
        ("keep-alive", "timeout=5"),
        ("proxy-connection", "keep-alive"),
        ("te", "trailers"),
        ("trailer", "x-t"),
        ("transfer-encoding", "chunked"),
        ("upgrade", "h2c"),
        ("location", "http://provider.test/x"),
        ("content-location", "http://provider.test/v1/messages"),
        ("link", "<http://provider.test/x>; rel=next"),
        ("refresh", "0; url=http://provider.test/"),
        ("proxy-authenticate", "Basic realm=x"),
        ("proxy-authentication-info", "x"),
        ("set-cookie", "s=1"),
        ("x-zygo-model", "forged"),
        ("openai-organization", "org-test"),
        ("openai-project", "proj-test"),
        ("anthropic-organization-id", "org-id-test"),
        ("content-length", "2"),
        ("content-type", "application/json"),
        ("x-request-id", "r1"),
    ] {
        u.append(k, v.parse().unwrap());
    }
    let acct = ["anthropic-organization-id", "content-length", "content-type", "openai-organization", "openai-project", "x-request-id"];
    assert_eq!(sorted_names(&response_headers(&u, false, false)), acct);
    assert_eq!(sorted_names(&response_headers(&u, true, false)), ["anthropic-organization-id", "content-type", "openai-organization", "openai-project", "x-request-id"]);
    assert_eq!(sorted_names(&response_headers(&u, false, true)), ["content-length", "content-type", "x-request-id"]);
    assert_eq!(sorted_names(&response_headers(&u, true, true)), ["content-type", "x-request-id"]);
}

/// A configured system proxy is never used for a loopback upstream (C3: the key would reach the proxy in clear
/// text); a remote upstream goes through it (CONNECT).
#[test]
fn upstream_loopback_bypasses_system_proxy() {
    let (purl, seen) = fake_proxy();
    let t = setup_with(
        |_, _| model("test-rhttps", &[prov("rhttps", "https://provider.invalid", "x", Some("k-code"), "")]),
        |o, _| {
            o.upstream_test = None;
            o.upstream_proxy = Some(purl.clone());
        },
    );
    let g = session(&t.h, Agent::ClaudeCode, None);
    assert_eq!(post(&t.h, &g, M, msg("code"), &[]).0, 200);
    assert_eq!(hv(&t.up.at("").headers, "x-api-key"), t.k["k-code"].expose());
    assert!(seen.lock().unwrap().is_empty(), "the proxy saw a loopback request");
    assert_eq!(st(post(&t.h, &g, M, msg("test-rhttps"), &[])), (503, "zygo_provider_unavailable".into()));
    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 1);
    assert!(seen[0].starts_with("CONNECT provider.invalid:443 "), "{:?}", seen[0].lines().next());
    assert!(!seen[0].contains(t.k["k-code"].expose()));
}

/// Client headers of test 4 + framing and routing headers, over HTTP.
fn wire_headers<'a>(g: &'a SessionGrant, bearer: &'a str) -> Vec<(&'a str, &'a str)> {
    vec![
        ("x-api-key", g.token.as_str()),
        ("x-zygo-session", g.session.as_str()),
        ("authorization", bearer),
        ("x-claude-code-request-class", "main"),
        ("anthropic-beta", "b1"),
        ("anthropic-version", "2023-06-01"),
        ("accept", "application/json"),
        ("x-stainless-os", "x"),
        ("cookie", "c=1"),
        ("accept-encoding", "gzip"),
        ("proxy-authorization", "Basic x"),
        ("te", "trailers"),
    ]
}

fn no_client_identity(h: &HeaderMap) {
    for n in ["authorization", "cookie", "accept-encoding", "proxy-authorization", "te"] {
        assert!(h.get(n).is_none(), "{n} forwarded");
    }
    assert!(h.keys().all(|k| !k.as_str().starts_with("x-zygo-") && !k.as_str().starts_with("x-claude-code-")), "{h:?}");
}

#[test]
fn headers_outbound_anthropic_provider() {
    let t = setup(none);
    let g = session(&t.h, Agent::ClaudeCode, None);
    let bearer = format!("Bearer {}", g.token);
    let r = send(&t.h, "POST", &format!("{M}?beta=true"), &wire_headers(&g, &bearer), Some(msg("code").into()));
    assert_eq!(r.0, 200);
    let got = t.up.at("");
    assert_eq!(got.path, "/v1/messages?beta=true");
    let h = &got.headers;
    assert_eq!(hv(h, "x-api-key"), t.k["k-code"].expose());
    assert_ne!(hv(h, "x-api-key"), g.token);
    no_client_identity(h);
    assert!(h.get("x-stainless-os").is_none());
    assert_eq!(hv(h, "anthropic-beta"), "b1");
    assert_eq!(hv(h, "anthropic-version"), "2023-06-01");
    assert_eq!(hv(h, "content-type"), "application/json");
    assert_eq!(hv(h, "accept"), "application/json");
    assert_eq!(hv(h, "host"), format!("127.0.0.1:{}", t.up.port()));
    assert_eq!(hv(h, "content-length"), got.body.len().to_string());
}

#[test]
fn headers_forward_headers_list() {
    let t = setup(|u, _| {
        let list = r#"forward_headers = ["content-type", "x-stainless-os", "authorization", "x-zygo-session", "x-claude-code-request-class", "host", "content-length", "transfer-encoding", "connection", "te", "upgrade", "proxy-authorization", "cookie", "accept-encoding"]"#;
        model("test-fh", &[prov("p-fh", &format!("{u}/fh"), "up-fh", Some("k-code"), &format!("{list}\n"))])
    });
    let g = session(&t.h, Agent::ClaudeCode, None);
    let bearer = format!("Bearer {}", g.token);
    let mut hd = wire_headers(&g, &bearer);
    hd.push(("host", "client.test"));
    let r = send(&t.h, "POST", M, &hd, Some(msg("test-fh").into()));
    assert_eq!(r.0, 200);
    let got = t.up.at("/fh");
    let h = &got.headers;
    assert_eq!(hv(h, "content-type"), "application/json");
    assert_eq!(hv(h, "x-stainless-os"), "x");
    assert_eq!(hv(h, "x-api-key"), t.k["k-code"].expose());
    no_client_identity(h);
    for n in ["anthropic-beta", "upgrade", "transfer-encoding"] {
        assert!(h.get(n).is_none(), "{n} forwarded");
    }
    // `accept` is not in the list: the client's value is not forwarded. reqwest sets its own `*/*` default
    // (reqwest 0.13.5 ClientBuilder::new), so the plan's "no accept" is checked as "not the client's value".
    assert_ne!(h.get("accept").map(|v| v.to_str().unwrap()), Some("application/json"));
    assert_eq!(hv(h, "host"), format!("127.0.0.1:{}", t.up.port()));
    assert_eq!(hv(h, "content-length"), got.body.len().to_string());
}

// ---- routing (§20 #7, #8, #23; §7.3; §14) ----

#[test]
fn route_role_reaches_role_model_provider() {
    let t = setup(none);
    let g = session(&t.h, Agent::ClaudeCode, None);
    let (s, hd, b) = post(
        &t.h,
        &g,
        M,
        msg("code"),
        &[("x-claude-code-request-class", "main"), ("x-claude-code-agent-id", "ag-1"), ("x-claude-code-agent-type", "custom")],
    );
    assert_eq!((s, b.as_slice()), (200, SSE.as_bytes()));
    assert_eq!(hd.get_all("x-zygo-model").iter().collect::<Vec<_>>(), ["test-code"]);
    assert_eq!((hv(&hd, "x-zygo-role"), hv(&hd, "x-request-id")), ("code", "r1"));
    assert!(["location", "set-cookie", "x-zygo-adjustments", "x-zygo-redacted"].iter().all(|n| hd.get(*n).is_none()), "{hd:?}");
    let mut want: Value = serde_json::from_slice(&msg("code")).unwrap();
    want["model"] = json!("up-code");
    assert_eq!(t.up.at("").body, to_vec(&want));
    assert_eq!(hv(&t.up.at("").headers, "x-api-key"), t.k["k-code"].expose());
    let r = &rows(&t, &g, 1)[0];
    assert_eq!(
        (r.role_requested.as_deref(), r.role_served.as_deref(), r.model.as_str(), r.provider.as_str()),
        (Some("code"), Some("code"), "test-code", "p-code")
    );
    assert_eq!((r.input_tokens, r.output_tokens, r.cache_read_tokens, r.cache_write_tokens), (120, 42, 30, 7));
    assert!((r.estimated_usd - 0.001382).abs() < 1e-12, "{}", r.estimated_usd); // (120×4 + 42×20 + 30×0.2 + 7×8) / 1e6
    assert_eq!(
        (r.status, r.route.as_str(), r.agent.as_str(), r.agent_version.as_deref(), r.token_label.as_deref(), r.profile.as_deref(), r.passthrough, r.compaction),
        (200, "/anthropic/v1/messages", "claude-code", Some("2.1.328"), None, None, false, false)
    );
    assert_eq!(
        (r.request_class.as_deref(), r.agent_id.as_deref(), r.agent_type.as_deref(), r.work_item.as_deref()),
        (Some("main"), Some("ag-1"), Some("custom"), Some("ENG-42"))
    );
    assert_eq!(r.ts, t.c.utc().unix_timestamp());
    let s = t.h.session(&g.session).unwrap();
    assert_eq!(s.status().cache_read_share, Some(30.0 / 157.0));
    assert_eq!(pin(&t, &g, "test-code").as_deref(), Some("p-code"));
    let dbg = format!("{r:?}");
    assert!(!dbg.contains(&g.token) && !dbg.contains(t.k["k-code"].expose()));
}

#[test]
fn route_unchanged_body_byte_for_byte() {
    let t = setup(none);
    let raw = r#"{ "model" : "test-same",
  "max_tokens": 1.0e2, "stream": true, "messages": [{"role":"user","content":"café \/ x"}] }"#
        .as_bytes()
        .to_vec();
    for agent in [Agent::ClaudeCode, Agent::OpenCode] {
        let g = session(&t.h, agent, None);
        assert_eq!(post(&t.h, &g, M, raw.clone(), &[]).0, 200);
        assert_eq!(t.up.got().last().unwrap().body, raw, "{agent}");
    }
}

#[test]
fn route_profile_override_serves() {
    let t = setup(none);
    let g = session(&t.h, Agent::ClaudeCode, Some(p1()));
    assert_eq!(g.profile_warning, None);
    let (s, hd, _) = post(&t.h, &g, M, msg("code"), &[]);
    assert_eq!(s, 200);
    assert_eq!((hv(&hd, "x-zygo-model"), hv(&hd, "x-zygo-role")), ("test-alt", "code"));
    let got = t.up.at("/alt");
    assert_eq!(body_json(got.clone())["model"], "up-alt");
    assert_eq!(hv(&got.headers, "x-api-key"), t.k["k-alt"].expose());
    let r = &rows(&t, &g, 1)[0];
    assert_eq!((r.profile.as_deref(), r.model.as_str(), r.provider.as_str()), (Some("p1"), "test-alt", "p-alt"));
}

#[test]
fn route_disabled_role_and_unknown_name() {
    let t = setup(none);
    let g = session(&t.h, Agent::ClaudeCode, Some(p1()));
    assert_eq!(st(post(&t.h, &g, M, msg("design"), &[])), (400, "zygo_role_disabled".into()));
    assert_eq!(st(post(&t.h, &g, M, msg("nope"), &[])), (400, "zygo_unknown_model".into()));
    assert!(t.up.got().is_empty());
    let r = rows(&t, &g, 2);
    assert_eq!(r.iter().map(|r| r.status).collect::<Vec<_>>(), [400, 400]);
    assert_eq!(r.iter().map(|r| r.role_requested.as_deref()).collect::<Vec<_>>(), [Some("design"), Some("nope")]);
    assert!(r.iter().all(|r| r.model.is_empty() && r.provider.is_empty() && r.agent_version.as_deref() == Some("2.1.328")));
}

#[test]
fn route_custom_role_served() {
    let t = setup(none);
    let g = session(&t.h, Agent::ClaudeCode, Some(p1()));
    let (s, hd, _) = post(&t.h, &g, M, msg("bench"), &[]);
    assert_eq!(s, 200);
    assert_eq!((hv(&hd, "x-zygo-role"), hv(&hd, "x-zygo-model")), ("bench", "test-alt"));
    assert_eq!(rows(&t, &g, 1)[0].role_served.as_deref(), Some("bench"));
}

#[test]
fn route_direct_id_and_1m_suffix() {
    let t = setup(none);
    let g = session(&t.h, Agent::ClaudeCode, None);
    let (s, hd, _) = post(&t.h, &g, M, msg("test-code[1m]"), &[]);
    assert_eq!(s, 200);
    assert_eq!((hv(&hd, "x-zygo-role"), hv(&hd, "x-zygo-model")), ("direct", "test-code"));
    assert_eq!(body_json(t.up.at(""))["model"], "up-code");
    let r = &rows(&t, &g, 1)[0];
    assert_eq!((r.role_requested.as_deref(), r.role_served.as_deref()), (Some("test-code"), Some("direct")));
}

#[test]
fn route_kill_switch_and_empty_catalog_row() {
    let t = setup(|u, _| {
        model("test-off", &[prov("p-off1", &format!("{u}/off1"), "x", None, "enabled = false\n")])
            + &model(
                "test-half",
                &[prov("p-off", &format!("{u}/off"), "x", None, "enabled = false\n"), prov("p-on", &format!("{u}/on"), "x", None, "")],
            )
            + &model(
                "test-dk",
                &[prov("p-off", &format!("{u}/off"), "x", None, "enabled = false\n"), prov("p-mk", &format!("{u}/mk"), "x", Some("k-missing"), "")],
            )
    });
    let g = session(&t.h, Agent::ClaudeCode, None);
    assert_eq!(st(post(&t.h, &g, M, msg("test-none"), &[])), (400, "zygo_no_provider".into()));
    assert_eq!(st(post(&t.h, &g, M, msg("test-off"), &[])), (503, "zygo_provider_unavailable".into()));
    assert_eq!(post(&t.h, &g, M, msg("test-half"), &[]).0, 200);
    assert_eq!((t.up.count("/off"), t.up.count("/on")), (0, 1));
    // §7.4: candidates are the enabled providers; every one dropped for a missing key (§6.2).
    assert_eq!(st(post(&t.h, &g, M, msg("test-dk"), &[])), (503, "zygo_provider_key_missing".into()));
    assert_eq!(t.up.count("/mk"), 0);
}

#[test]
fn route_bad_requests_and_identity() {
    let t = setup(none);
    let g = session(&t.h, Agent::ClaudeCode, None);
    let mut errs = vec![];
    let r = send(&t.h, "POST", M, &[("x-zygo-session", g.session.as_str())], Some(msg("code").into()));
    errs.push(r.2.clone());
    assert_eq!(st(r), (401, "zygo_token_invalid".into()));
    let wrong = format!("zyg_s_{}", ulid());
    let r = send(&t.h, "POST", M, &[("x-api-key", wrong.as_str()), ("x-zygo-session", g.session.as_str())], Some(msg("code").into()));
    errs.push(r.2.clone());
    assert_eq!(st(r), (401, "zygo_token_invalid".into()));
    let other = ulid();
    let r = send(&t.h, "POST", M, &[("x-api-key", g.token.as_str()), ("x-zygo-session", other.as_str())], Some(msg("code").into()));
    errs.push(r.2.clone());
    assert_eq!(st(r), (401, "zygo_session_unknown".into()));
    for b in [b"not json".to_vec(), br#"{"messages":[]}"#.to_vec(), br#"{"model":5}"#.to_vec()] {
        let r = post(&t.h, &g, M, b, &[]);
        errs.push(r.2.clone());
        assert_eq!(st(r), (400, "zygo_bad_request".into()));
    }
    assert!(errs.iter().all(|e| !String::from_utf8_lossy(e).contains(&g.token)));
    assert!(t.up.got().is_empty());
    let r = rows(&t, &g, 3);
    assert_eq!(r.iter().map(|r| r.status).collect::<Vec<_>>(), [400, 400, 400]);
}

#[test]
fn route_accept_refreshes_ttl_and_sweeps() {
    let t = setup(none);
    let (old, g) = (session(&t.h, Agent::ClaudeCode, None), session(&t.h, Agent::ClaudeCode, None));
    t.c.advance(23 * HOUR);
    assert_eq!(st(post(&t.h, &g, M, msg("nope"), &[])), (400, "zygo_unknown_model".into()));
    t.c.advance(2 * HOUR);
    assert_eq!(st(post(&t.h, &g, M, msg("code"), &[])), (401, "zygo_session_unknown".into()));
    assert!(t.h.session(&old.session).is_none());
    let g2 = session(&t.h, Agent::ClaudeCode, None);
    t.c.advance(23 * HOUR);
    assert_eq!(post(&t.h, &g2, M, msg("code"), &[]).0, 200);
    t.c.advance(2 * HOUR);
    assert_eq!(post(&t.h, &g2, M, msg("code"), &[]).0, 200);
}

// ---- fallback and pinning (§20 #11, §7.3, §10.4) ----

#[test]
fn fallback_500_moves_pin() {
    let t = setup(none);
    let g = session(&t.h, Agent::ClaudeCode, None);
    t.up.script("/a", vec![Reply::Status(500)]);
    let (s, _, b) = post(&t.h, &g, M, msg("test-fb"), &[]);
    assert_eq!((s, b.as_slice()), (200, SSE.as_bytes()));
    assert_eq!((t.up.count("/a"), t.up.count("/b"), t.up.count("/c")), (1, 1, 0));
    // The model is restored after an attempt: the next provider (same upstream_model) gets it rewritten too.
    assert_eq!(body_json(t.up.at("/a"))["model"], "up-fb");
    assert_eq!(body_json(t.up.at("/b"))["model"], "up-fb");
    assert_eq!(pin(&t, &g, "test-fb").as_deref(), Some("b"));
    assert_eq!(rows(&t, &g, 1)[0].provider, "b");
    assert_eq!(post(&t.h, &g, M, msg("test-fb"), &[]).0, 200);
    assert_eq!((t.up.count("/a"), t.up.count("/b")), (1, 2));
}

#[test]
fn fallback_429_and_connect_refused() {
    let dead = refused_port();
    let t = setup(|u, _| {
        model(
            "test-fb2",
            &[
                prov("dead", &format!("http://127.0.0.1:{dead}"), "x", None, ""),
                prov("a", &format!("{u}/a"), "x", None, ""),
                prov("b", &format!("{u}/b"), "x", None, ""),
            ],
        )
    });
    let g = session(&t.h, Agent::ClaudeCode, None);
    t.up.script("/a", vec![Reply::Status(429)]);
    assert_eq!(post(&t.h, &g, M, msg("test-fb2"), &[]).0, 200);
    assert_eq!((t.up.count("/a"), t.up.count("/b")), (1, 1));
    assert_eq!(pin(&t, &g, "test-fb2").as_deref(), Some("b"));
}

#[test]
fn fallback_not_on_4xx_or_send_error() {
    let (cl, closed) = closing_upstream();
    let t = setup(|u, _| {
        model("test-cl", &[prov("cl", &format!("http://127.0.0.1:{cl}"), "x", None, ""), prov("b", &format!("{u}/b"), "x", None, "")])
    });
    let g = session(&t.h, Agent::ClaudeCode, None);
    t.up.script("/a", vec![Reply::Status(400)]);
    let (s, _, b) = post(&t.h, &g, M, msg("test-fb"), &[]);
    assert_eq!((s, b.as_slice()), (400, ERR.as_bytes()));
    assert_eq!(t.up.count("/b"), 0);
    assert_eq!(pin(&t, &g, "test-fb").as_deref(), Some("a"));
    assert_eq!(rows(&t, &g, 1)[0].status, 400);
    assert_eq!(st(post(&t.h, &g, M, msg("test-cl"), &[])), (503, "zygo_provider_unavailable".into()));
    assert_eq!(closed.load(Ordering::SeqCst), 1);
    assert_eq!(t.up.count("/b"), 0);
    assert_eq!(pin(&t, &g, "test-cl"), None);
}

#[test]
fn redirect_not_followed_and_provider_urls_dropped() {
    let other = Upstream::start();
    let resp = format!(
        "HTTP/1.1 307 Temporary Redirect\r\nlocation: {}/redir/v1/messages\r\ncontent-location: http://provider.test/v1/messages\r\n\
         link: <http://provider.test/x>; rel=next\r\nrefresh: 0\r\nproxy-authenticate: Basic realm=x\r\nconnection: close, x-hop\r\n\
         x-hop: v\r\nkeep-alive: timeout=5\r\ncontent-type: application/json\r\ncontent-length: 2\r\n\r\n{{}}",
        other.url()
    );
    let (port, n) = raw_upstream(resp.into_bytes(), Duration::ZERO);
    let t = setup(move |_, _| model("test-r", &[prov("r", &format!("http://127.0.0.1:{port}"), "up-r", None, "")]));
    let g = session(&t.h, Agent::ClaudeCode, None);
    let (s, hd, b) = post(&t.h, &g, M, msg("test-r"), &[]);
    assert_eq!((s, b.as_slice()), (307, b"{}".as_slice()));
    assert_eq!((n.load(Ordering::SeqCst), other.got().len()), (1, 0));
    for h in ["location", "content-location", "link", "refresh", "proxy-authenticate", "x-hop", "keep-alive"] {
        assert!(hd.get(h).is_none(), "{h} reached the client");
    }
}

#[test] // §7.3 note (c) is serve mode only
fn local_keeps_provider_account_headers() {
    let t = setup(none);
    let g = session(&t.h, Agent::ClaudeCode, None);
    let (s, hd, _) = post(&t.h, &g, M, msg("code"), &[]);
    assert_eq!((s, hv(&hd, "openai-organization")), (200, "org-test"));
}

#[test]
fn fallback_all_failed_removes_pin() {
    let d1 = refused_port();
    let d2 = refused_port();
    let t = setup(|_, _| {
        model(
            "test-dead",
            &[prov("d1", &format!("http://127.0.0.1:{d1}"), "x", None, ""), prov("d2", &format!("http://127.0.0.1:{d2}"), "x", None, "")],
        )
    });
    let g = session(&t.h, Agent::ClaudeCode, None);
    assert_eq!(post(&t.h, &g, M, msg("test-fb"), &[]).0, 200);
    assert_eq!(pin(&t, &g, "test-fb").as_deref(), Some("a"));
    t.up.script("/a", vec![Reply::Status(500)]);
    t.up.script("/b", vec![Reply::Status(503)]);
    t.up.script("/c", vec![Reply::Status(500)]);
    let (s, _, b) = post(&t.h, &g, M, msg("test-fb"), &[]);
    assert_eq!((s, b.as_slice()), (500, ERR.as_bytes()));
    assert_eq!(pin(&t, &g, "test-fb"), None);
    let r = rows(&t, &g, 2);
    assert_eq!((r[1].status, r[1].provider.as_str()), (500, "c"));
    t.h.session(&g.session).unwrap().pins.lock().unwrap().insert("test-dead".into(), "d1".into());
    assert_eq!(st(post(&t.h, &g, M, msg("test-dead"), &[])), (503, "zygo_provider_unavailable".into()));
    assert_eq!(pin(&t, &g, "test-dead"), None);
}

// ---- stage 5 (§6.2, §7.5, C3) ----

#[test]
fn provider_missing_key() {
    let t = setup(|u, _| {
        model("test-k1", &[prov("k1", &format!("{u}/k1"), "x", Some("k-missing"), "")])
            + &model(
                "test-k2",
                &[prov("miss", &format!("{u}/miss"), "x", Some("k-missing"), ""), prov("ok", &format!("{u}/ok"), "x", Some("k-code"), "")],
            )
    });
    let g = session(&t.h, Agent::ClaudeCode, None);
    assert_eq!(st(post(&t.h, &g, M, msg("test-k1"), &[])), (503, "zygo_provider_key_missing".into()));
    assert_eq!(post(&t.h, &g, M, msg("test-k2"), &[]).0, 200);
    assert_eq!((t.up.count("/miss"), t.up.count("/ok")), (0, 1));
}

#[test]
fn provider_zdr_openrouter() {
    let t = setup(|u, port| {
        let zdr = "[model.provider.flags]\nzdr = true\n";
        model("test-or", &[prov("or", &format!("http://openrouter.ai:{port}/or"), "test-or", Some("k-or"), zdr)])
            + &model("test-or2", &[prov("or2", &format!("http://eu.openrouter.ai:{port}/or2"), "test-or2", None, zdr)])
            + &model("test-nor", &[prov("nor", &format!("http://notopenrouter.ai:{port}/nor"), "test-nor", None, zdr)])
            + &model("test-lz", &[prov("lz", &format!("{u}/lz"), "test-lz", None, zdr)])
            + &model(
                "test-orfb",
                &[prov("orf", &format!("http://openrouter.ai:{port}/orf"), "test-orfb", None, zdr), prov("plain", &format!("{u}/plain"), "test-orfb", None, "")],
            )
            + &model(
                "test-orfb2",
                &[prov("orf2", &format!("http://openrouter.ai:{port}/orf2"), "x-or", None, zdr), prov("plain2", &format!("{u}/plain2"), "x-plain", None, "")],
            )
    });
    let g = session(&t.h, Agent::ClaudeCode, None);
    let p = |v: Value| post(&t.h, &g, M, to_vec(&v), &[]);
    assert_eq!(p(json!({"model":"test-or","max_tokens":8,"messages":[],"provider":{"order":["x"],"zdr":false}})).0, 200);
    assert_eq!(body_json(t.up.at("/or"))["provider"], json!({"order":["x"],"zdr":true}));
    assert_eq!(hv(&t.up.at("/or").headers, "x-api-key"), t.k["k-or"].expose());
    assert_eq!(p(json!({"model":"test-or2","max_tokens":8,"messages":[]})).0, 200);
    assert_eq!(body_json(t.up.at("/or2"))["provider"], json!({"zdr":true}));
    for (m, path) in [("test-nor", "/nor"), ("test-lz", "/lz")] {
        let raw = to_vec(&json!({"model":m,"max_tokens":8,"messages":[]}));
        let (s, hd, _) = post(&t.h, &g, M, raw.clone(), &[]);
        assert_eq!(s, 200);
        assert_eq!(t.up.at(path).body, raw);
        assert_eq!(hv(&hd, "x-zygo-adjustments"), "zdr_host_unsupported");
    }
    t.up.script("/orf", vec![Reply::Status(500)]);
    let raw = to_vec(&json!({"model":"test-orfb","max_tokens":8,"messages":[]}));
    assert_eq!(post(&t.h, &g, M, raw.clone(), &[]).0, 200);
    assert_eq!(body_json(t.up.at("/orf"))["provider"], json!({"zdr":true}));
    assert_eq!(t.up.at("/plain").body, raw);
    // The `provider` field is restored after a zdr attempt: the fallback (other upstream_model) gets the client's.
    for client in [None, Some(json!({"order":["q"]}))] {
        t.up.script("/orf2", vec![Reply::Status(500)]);
        let mut v = json!({"model":"test-orfb2","max_tokens":8,"messages":[]});
        if let Some(c) = &client {
            v["provider"] = c.clone();
        }
        // A new session each time: no pin from the last round.
        assert_eq!(post(&t.h, &session(&t.h, Agent::ClaudeCode, None), M, to_vec(&v), &[]).0, 200);
        let mut want_or = v.clone();
        want_or["model"] = json!("x-or");
        want_or["provider"] = client.clone().map_or(json!({"zdr":true}), |_| json!({"order":["q"],"zdr":true}));
        assert_eq!(t.up.at("/orf2").body, to_vec(&want_or));
        let mut want = v.clone();
        want["model"] = json!("x-plain");
        assert_eq!(t.up.at("/plain2").body, to_vec(&want));
    }
}

#[test]
fn provider_require_zdr() {
    let t = setup(|u, port| {
        let zdr = "[model.provider.flags]\nzdr = true\n";
        String::from("[proxy]\nrequire_zdr = true\n\n")
            + &model(
                "test-rz",
                &[prov("plain", &format!("{u}/plain"), "x", None, ""), prov("rz", &format!("http://openrouter.ai:{port}/rz"), "x", None, zdr)],
            )
            + &model(
                "test-mix",
                &[prov("nz", &format!("{u}/nz"), "x", Some("k-code"), ""), prov("mz", &format!("http://openrouter.ai:{port}/mz"), "x", Some("k-missing"), zdr)],
            )
    });
    let g = session(&t.h, Agent::ClaudeCode, None);
    assert_eq!(post(&t.h, &g, M, msg("test-rz"), &[]).0, 200);
    assert_eq!((t.up.count("/plain"), t.up.count("/rz")), (0, 1));
    assert_eq!(st(post(&t.h, &g, M, msg("test-code"), &[])), (503, "zygo_provider_unavailable".into()));
    assert_eq!(st(post(&t.h, &g, M, msg("test-mix"), &[])), (503, "zygo_provider_unavailable".into()));
}

#[test]
fn provider_plain_http_remote_refused() {
    let t = setup(|u, _| {
        model("test-rem", &[prov("rem", "http://10.0.0.5:9", "x", Some("k-code"), "")])
            + &model("test-rem2", &[prov("rem", "http://10.0.0.5:9", "x", Some("k-code"), ""), prov("ok", &format!("{u}/ok"), "x", None, "")])
    });
    let g = session(&t.h, Agent::ClaudeCode, None);
    let t0 = Instant::now();
    assert_eq!(st(post(&t.h, &g, M, msg("test-rem"), &[])), (503, "zygo_provider_unavailable".into()));
    assert!(t0.elapsed() < Duration::from_secs(5));
    assert_eq!(post(&t.h, &g, M, msg("test-rem2"), &[]).0, 200);
    assert_eq!(t.up.count("/ok"), 1);
}

// ---- body numbers (§7.3: parsed once, re-serialized only when changed) ----

#[test]
fn body_numbers_unchanged_after_rewrite() {
    let t = setup(none);
    let g = session(&t.h, Agent::ClaudeCode, None);
    let body = |m: &str| {
        format!(
            r#"{{"model":"{m}","max_tokens":64,"messages":[{{"role":"assistant","content":[{{"type":"tool_use","id":"t","name":"n","input":{{"big":123456789012345678901234567890,"e":1e5,"f":0.1,"neg0":-0.0}}}}]}}],"metadata":{{"n":1e400}}}}"#
        )
    };
    assert_eq!(post(&t.h, &g, M, body("code").into_bytes(), &[]).0, 200);
    // Every number keeps its digits; serde_json `arbitrary_precision` writes an unsigned exponent with "+"
    // (serde_json 1.0.154 de.rs `scan_exponent`): the same JSON number (RFC 8259 §6).
    let want = body("up-code").replace("1e5", "1e+5").replace("1e400", "1e+400");
    assert_eq!(String::from_utf8(t.up.at("").body).unwrap(), want);
    // Not re-serialized: the received bytes.
    assert_eq!(post(&t.h, &g, M, body("test-same").into_bytes(), &[]).0, 200);
    assert_eq!(String::from_utf8(t.up.at("").body).unwrap(), body("test-same"));
}

// ---- cache markers (§20 #18, §10.4) ----

#[test]
fn cache_markers_opencode_only() {
    let t = setup(none);
    let oc = session(&t.h, Agent::OpenCode, None);
    let cc = session(&t.h, Agent::ClaudeCode, None);
    let b = json!({"model":"test-same","max_tokens":64,"system":[{"type":"text","text":"a"},{"type":"text","text":"b"}],
      "tools":[{"name":"t1","input_schema":{"type":"object"}},{"name":"t2","input_schema":{"type":"object"}}],"messages":[{"role":"user","content":"hi"}]});
    let mark = json!({"type":"ephemeral","ttl":"1h"});
    let mut want = b.clone();
    want["system"][1]["cache_control"] = mark.clone();
    want["tools"][1]["cache_control"] = mark.clone();
    let last = || t.up.got().last().unwrap().body.clone();
    assert_eq!(post(&t.h, &oc, M, to_vec(&b), &[]).0, 200);
    assert_eq!(last(), to_vec(&want));

    let mut marked = b.clone();
    marked["messages"][0]["content"] = json!([{"type":"text","text":"hi","cache_control":{"type":"ephemeral"}}]);
    assert_eq!(post(&t.h, &oc, M, to_vec(&marked), &[]).0, 200);
    assert_eq!(last(), to_vec(&marked));

    let mut s_text = b.clone();
    s_text["system"] = json!("s");
    let mut want2 = s_text.clone();
    want2["tools"][1]["cache_control"] = mark.clone();
    assert_eq!(post(&t.h, &oc, M, to_vec(&s_text), &[]).0, 200);
    assert_eq!(last(), to_vec(&want2));

    assert_eq!(post(&t.h, &cc, M, to_vec(&b), &[]).0, 200);
    assert_eq!(last(), to_vec(&b));

    t.up.script("", vec![Reply::Json(json!({"input_tokens":3}))]);
    assert_eq!(post(&t.h, &oc, CT, to_vec(&b), &[]).0, 200);
    let got = t.up.got().last().unwrap().clone();
    assert_eq!((got.path.as_str(), got.body), ("/v1/messages/count_tokens", to_vec(&b)));
}

// ---- count_tokens (§7.3) ----

#[test]
fn count_tokens_forwarded_on_anthropic() {
    let t = setup(none);
    let g = session(&t.h, Agent::ClaudeCode, None);
    t.up.script("", vec![Reply::Json(json!({"input_tokens":17}))]);
    let (s, hd, b) = post(&t.h, &g, CT, msg("code"), &[]);
    assert_eq!((s, serde_json::from_slice::<Value>(&b).unwrap()), (200, json!({"input_tokens":17})));
    let got = t.up.at("");
    assert_eq!(got.path, "/v1/messages/count_tokens");
    assert_eq!(body_json(got)["model"], "up-code");
    assert_eq!(hv(&hd, "x-zygo-model"), "test-code");
}

#[test]
fn count_tokens_estimated_locally() {
    let t = setup(none);
    let g = session(&t.h, Agent::ClaudeCode, None);
    let body = to_vec(&json!({"model":"test-oai","messages":[{"role":"user","content":"ééééé"}]}));
    let (s, hd, b) = post(&t.h, &g, CT, body, &[]);
    // 8 + 4 + 10 bytes = 22, ceil(22 / 4) = 6 (5 by characters).
    assert_eq!((s, serde_json::from_slice::<Value>(&b).unwrap()), (200, json!({"input_tokens":6})));
    assert_eq!(hv(&hd, "x-zygo-adjustments"), "count_estimated");
    assert!(t.up.got().is_empty());
    let r = &rows(&t, &g, 1)[0];
    assert_eq!((r.adjustments.clone(), r.provider.as_str(), r.status), (vec!["count_estimated".to_string()], "", 200));
}

// ---- /v1/models (§7.4, §11.4, §20 #23) ----

#[test]
fn models_list_annotations() {
    let t = setup(none);
    let g = session(&t.h, Agent::ClaudeCode, Some(p1()));
    let v = models(&t, &g);
    let all = ids(&v);
    for id in ["code", "lead", "bench", "test-code", "test-alt", "test-same", "test-oai"] {
        assert!(all.contains(&id.to_string()), "{id} missing: {all:?}");
    }
    for id in ["design", "test-none"] {
        assert!(!all.contains(&id.to_string()), "{id} listed");
    }
    let c = cfg(&TempDir::new().unwrap(), &base(&t.up.url(), true));
    assert_eq!(item(&v, "code")["display_name"], "code");
    assert_eq!(item(&v, "code")["description"], format!("{} · profile p1: test-alt", c.config.role["code"].description));
    assert_eq!(item(&v, "lead")["description"], c.config.role["lead"].description.as_str());
    assert_eq!(item(&v, "bench")["description"], "Runs benchmarks.");
    assert_eq!(item(&v, "test-code")["type"], "model");
    assert!(idx(&v, "code") < idx(&v, "bench") && idx(&v, "bench") < idx(&v, "test-code"));
    assert_eq!(v["has_more"], false);
    assert_eq!((v["first_id"].as_str().unwrap(), v["last_id"].as_str().unwrap()), (all[0].as_str(), all[all.len() - 1].as_str()));
    let r = send(&t.h, "GET", "anthropic/v1/models", &[("x-zygo-session", g.session.as_str())], None);
    assert_eq!(st(r), (401, "zygo_token_invalid".into()));
}

// ---- limits (§7.3) ----

#[test]
fn limits_body_over_32_mib() {
    let t = setup(none);
    let g = session(&t.h, Agent::ClaudeCode, None);
    let b = vec![b' '; (32 << 20) + 1];
    assert_eq!(st(post_cl(&t.h, &g, b.clone())), (413, "zygo_body_too_large".into()));
    assert_eq!(st(post_chunked(&t.h, &g, b)), (413, "zygo_body_too_large".into()));
    assert!(t.up.got().is_empty());
    assert_eq!(rows(&t, &g, 2).iter().map(|r| r.status).collect::<Vec<_>>(), [413, 413]);
}

#[test]
fn limits_inflight_waits_then_busy() {
    assert_eq!((BUSY_WAIT, StartOpts::default().busy_wait), (Duration::from_secs(5), Duration::from_secs(5)));
    assert_eq!((ProxyConfig::default().max_inflight_mib, INFLIGHT_FACTOR), (512, 4));
    let t = setup_with(|_, _| "[proxy]\nmax_inflight_mib = 1\n".into(), |o, _| o.busy_wait = Duration::from_millis(200));
    let g = session(&t.h, Agent::ClaudeCode, None);
    assert_eq!(post_cl(&t.h, &g, big(200 << 10)).0, 200); // ≈ 800 KiB ≤ 1 MiB
    let hold = t.h.hold_inflight(600 << 10);
    for send in [post_cl, post_chunked] {
        let t0 = Instant::now();
        assert_eq!(st(send(&t.h, &g, big(200 << 10))), (503, "zygo_busy".into()));
        assert!(t0.elapsed() >= Duration::from_millis(200));
    }
    drop(hold);
    assert_eq!(post_cl(&t.h, &g, big(200 << 10)).0, 200);
    assert_eq!(t.up.got().len(), 2);
    let t0 = Instant::now();
    assert_eq!(st(post_cl(&t.h, &g, big(300 << 10))), (503, "zygo_busy".into()));
    assert!(t0.elapsed() >= Duration::from_millis(200));
}

/// A busy reply to a client that sent its whole body reads the body first: the reply is followed by a clean close,
/// not a reset (on Windows a reset also discards the reply the client already received; Linux keeps it, so there this
/// test passes either way).
#[test]
fn limits_busy_reads_the_body_before_replying() {
    let t = setup_with(|_, _| "[proxy]\nmax_inflight_mib = 1\n".into(), |o, _| o.busy_wait = Duration::from_millis(200));
    let g = session(&t.h, Agent::ClaudeCode, None);
    let _hold = t.h.hold_inflight(900 << 10);
    let body = big(200 << 10);
    let mut s = std::net::TcpStream::connect(format!("127.0.0.1:{}", t.h.url().port().unwrap())).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    write!(s, "POST /{M} HTTP/1.1\r\nhost: x\r\nx-api-key: {}\r\nx-zygo-session: {}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n", g.token, g.session, body.len()).unwrap();
    // The body arrives after the busy wait ended: it sits unread unless the server drains it.
    std::thread::sleep(Duration::from_millis(400));
    s.write_all(&body).unwrap();
    let mut got = Vec::new();
    s.read_to_end(&mut got).expect("a clean close after the reply");
    let text = String::from_utf8_lossy(&got);
    assert!(text.starts_with("HTTP/1.1 503"), "{text}");
    assert!(text.contains("zygo_busy"), "{text}");
}

/// A chunked request holding permits never waits for more while holding them (no hold-and-wait): A (chunked, 2 ×
/// 100 KiB) and B (200 KiB, content-length, queued between A's chunks) both get 200 within a 1 MiB budget.
#[test]
fn limits_chunked_no_hold_and_wait() {
    let t = setup_with(|_, _| "[proxy]\nmax_inflight_mib = 1\n".into(), |o, _| o.busy_wait = Duration::from_secs(2));
    let g = session(&t.h, Agent::ClaudeCode, None);
    let addr = format!("127.0.0.1:{}", t.h.url().port().unwrap());
    let body = big(200 << 10);
    let (b1, b2) = body.split_at(100 << 10);
    let (b1, b2) = (b1.to_vec(), b2.to_vec());
    let (tok, sid) = (g.token.clone(), g.session.clone());
    let a = std::thread::spawn(move || {
        let mut s = std::net::TcpStream::connect(addr).unwrap();
        write!(s, "POST /{M} HTTP/1.1\r\nhost: x\r\nx-api-key: {tok}\r\nx-zygo-session: {sid}\r\ncontent-type: application/json\r\ntransfer-encoding: chunked\r\nconnection: close\r\n\r\n").unwrap();
        write!(s, "{:x}\r\n", b1.len()).unwrap();
        s.write_all(&b1).unwrap();
        s.write_all(b"\r\n").unwrap();
        s.flush().unwrap();
        std::thread::sleep(Duration::from_millis(400));
        write!(s, "{:x}\r\n", b2.len()).unwrap();
        s.write_all(&b2).unwrap();
        s.write_all(b"\r\n0\r\n\r\n").unwrap();
        s.flush().unwrap();
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        out.lines().next().unwrap_or("").to_string()
    });
    std::thread::sleep(Duration::from_millis(150));
    let b = post_cl(&t.h, &g, big(200 << 10));
    let a = a.join().unwrap();
    assert_eq!(b.0, 200, "{}", String::from_utf8_lossy(&b.2));
    assert!(a.starts_with("HTTP/1.1 200"), "A: {a}");
    assert_eq!(t.up.got().len(), 2);
}

// ---- usage rows (§14) ----

#[test]
fn usage_non_streamed_json() {
    let t = setup(none);
    let g = session(&t.h, Agent::ClaudeCode, None);
    let reply = json!({"id":"m","type":"message","usage":{"input_tokens":11,"output_tokens":12,"cache_read_input_tokens":13,"cache_creation_input_tokens":14}});
    t.up.script("", vec![Reply::Json(reply.clone())]);
    let (s, hd, b) = post(&t.h, &g, M, msg("code"), &[]);
    assert_eq!((s, b), (200, to_vec(&reply)));
    assert!(hd.get("content-length").is_some());
    let r = &rows(&t, &g, 1)[0];
    assert_eq!((r.input_tokens, r.output_tokens, r.cache_read_tokens, r.cache_write_tokens), (11, 12, 13, 14));
}

#[test]
fn usage_written_on_client_disconnect() {
    let t = setup(none);
    let g = session(&t.h, Agent::ClaudeCode, None);
    t.up.script("", vec![Reply::Hang]);
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let mut r = http_client()
            .post(t.h.url().join(M).unwrap())
            .header("x-api-key", &g.token)
            .header("x-zygo-session", &g.session)
            .body(msg("code"))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200);
        assert!(r.chunk().await.unwrap().is_some());
        drop(r);
    });
    drop(rt);
    assert!(t.h.usage_written(1, Duration::from_secs(10)));
    let r = &rows(&t, &g, 1)[0];
    assert_eq!((r.input_tokens, r.output_tokens, r.status), (120, 1, 200));
    assert_eq!(t.h.session(&g.session).unwrap().status().cache_read_share, Some(30.0 / 157.0));
}

/// No upstream response (the client left first): the row has status 0 and no provider (§14 note).
#[test]
fn usage_status_0_without_upstream_response() {
    let reply = format!("HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\n\r\n{SSE}", SSE.len());
    let (port, n) = raw_upstream(reply.into_bytes(), Duration::from_secs(2));
    let t = setup(move |_, _| model("test-slow", &[prov("slow", &format!("http://127.0.0.1:{port}"), "up", None, "")]));
    let g = session(&t.h, Agent::ClaudeCode, None);
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let r = rt.block_on(async {
        http_client()
            .post(t.h.url().join(M).unwrap())
            .header("x-api-key", &g.token)
            .header("x-zygo-session", &g.session)
            .timeout(Duration::from_millis(500))
            .body(msg("test-slow"))
            .send()
            .await
    });
    assert!(r.is_err_and(|e| e.is_timeout()));
    drop(rt);
    let r = &rows(&t, &g, 1)[0];
    assert_eq!((r.status, r.provider.as_str(), r.input_tokens), (0, "", 0));
    assert_eq!(n.load(Ordering::SeqCst), 1);
}

#[test]
fn usage_flushed_on_shutdown() {
    let T { d, up: _up, h, .. } = setup(none);
    let g = session(&h, Agent::ClaudeCode, None);
    assert_eq!(post(&h, &g, M, msg("code"), &[]).0, 200);
    assert_eq!(post(&h, &g, M, msg("code"), &[]).0, 200);
    h.shutdown();
    assert_eq!(Store::open(&d.path().join("data/usage.db")).unwrap().requests_for_session(&g.session).unwrap().len(), 2);
}

#[test]
fn usage_store_open_failure_fails_open() {
    let t = setup_with(none, |o, d| {
        std::fs::write(d.path().join("blocker"), b"file").unwrap();
        o.usage_db = Some(d.path().join("blocker/usage.db"));
    });
    let g = session(&t.h, Agent::ClaudeCode, None);
    assert_eq!(g.usage_note.as_deref(), Some(USAGE_OFF_NOTE));
    assert_eq!(post(&t.h, &g, M, msg("code"), &[]).0, 200);
    assert_eq!(t.h.session(&g.session).unwrap().status().usage_note.as_deref(), Some(USAGE_OFF_NOTE));
    let good = setup(none);
    assert_eq!(session(&good.h, Agent::ClaudeCode, None).usage_note, None);
}

// ---- upstream bounds (§7 note 2026-11-19) ----

#[test]
fn upstream_silent_before_headers_ends_at_the_read_idle_bound() {
    let reply = "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 2\r\n\r\n{}";
    let (port, _) = raw_upstream(reply.as_bytes().to_vec(), Duration::from_secs(20));
    let t = setup_with(
        move |_, _| model("test-silent", &[prov("silent", &format!("http://127.0.0.1:{port}"), "up", None, "")]),
        |o, _| o.upstream_read_idle = Duration::from_millis(300),
    );
    let g = session(&t.h, Agent::ClaudeCode, None);
    let t0 = Instant::now();
    assert_eq!(st(post(&t.h, &g, M, msg("test-silent"), &[])), (503, "zygo_provider_unavailable".into()));
    assert!(t0.elapsed() < Duration::from_secs(10), "{:?}", t0.elapsed());
} // mutation: no `.read_timeout(..)` (the reply comes at 20 s as a 200)

#[test]
fn upstream_stalled_body_ends_at_the_read_idle_bound() {
    let (first, rest) = SSE.split_once("\n\n").unwrap();
    let head = "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n";
    let port = drip_upstream(head, vec![format!("{first}\n\n").into_bytes(), rest.as_bytes().to_vec()], Duration::from_secs(20));
    let t = setup_with(
        move |_, _| model("test-stall", &[prov("stall", &format!("http://127.0.0.1:{port}"), "up", None, "")]),
        |o, _| o.upstream_read_idle = Duration::from_millis(300),
    );
    let g = session(&t.h, Agent::ClaudeCode, None);
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let t0 = Instant::now();
    rt.block_on(async {
        let r = http_client()
            .post(t.h.url().join(M).unwrap())
            .header("x-api-key", &g.token)
            .header("x-zygo-session", &g.session)
            .body(msg("test-stall"))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200);
        let _ = r.bytes().await; // an error or an early end; either is the cut
    });
    assert!(t0.elapsed() < Duration::from_secs(10), "{:?}", t0.elapsed());
} // mutation: no `.read_timeout(..)` (the body ends when the upstream closes, after its two 20 s gaps)

#[test]
fn upstream_stream_longer_than_the_idle_bound_is_not_cut() {
    let head = "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n";
    let parts = SSE.as_bytes().chunks(SSE.len().div_ceil(8)).map(<[u8]>::to_vec).collect();
    let port = drip_upstream(head, parts, Duration::from_millis(500));
    let t = setup_with(
        move |_, _| model("test-drip", &[prov("drip", &format!("http://127.0.0.1:{port}"), "up", None, "")]),
        |o, _| o.upstream_read_idle = Duration::from_secs(3),
    );
    let g = session(&t.h, Agent::ClaudeCode, None);
    let (s, _, b) = post(&t.h, &g, M, msg("test-drip"), &[]);
    assert_eq!(s, 200);
    assert!(String::from_utf8_lossy(&b).contains(r#"{"type":"message_stop"}"#));
} // mutation: `.timeout(idle)` (a total limit) in place of `.read_timeout(idle)` cuts the 4 s stream at 3 s; the gap is
  // 6 × under the bound

#[test]
fn local_mode_reads_a_body_with_no_idle_bound() { // §7 note 2026-11-19: the body bound is serve mode only
    let t = setup_with(none, |o, _| o.body_read_idle = Duration::from_secs(1));
    let g = session(&t.h, Agent::ClaudeCode, None);
    let b = msg("code");
    let parts = vec![b[..b.len() / 2].to_vec(), b[b.len() / 2..].to_vec()];
    let body = futures_util::stream::iter(parts.into_iter().enumerate()).then(|(i, p)| async move {
        if i == 1 {
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
        Ok::<_, std::io::Error>(Bytes::from(p))
    });
    assert_eq!(send(&t.h, "POST", M, &auth(&g), Some(reqwest::Body::wrap_stream(body))).0, 200);
} // mutation: local mode passes `Some(opts.body_read_idle)` (the 3 s pause is a 400)

#[cfg(target_os = "linux")]
#[test]
fn upstream_connect_bound_falls_back_to_the_next_provider() {
    use std::os::fd::FromRawFd;
    // SAFETY: plain socket calls on a new descriptor; the listener takes ownership of it.
    let l = unsafe {
        let fd = libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0);
        assert!(fd >= 0);
        let mut a: libc::sockaddr_in = std::mem::zeroed();
        a.sin_family = libc::AF_INET as libc::sa_family_t;
        a.sin_addr.s_addr = u32::from(std::net::Ipv4Addr::LOCALHOST).to_be();
        let len = std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t;
        assert_eq!(libc::bind(fd, &a as *const libc::sockaddr_in as *const libc::sockaddr, len), 0);
        assert_eq!(libc::listen(fd, 0), 0);
        std::net::TcpListener::from_raw_fd(fd)
    };
    let addr = l.local_addr().unwrap();
    // Linux drops further SYNs while the queue is full, so the next connect hangs.
    let _fill: Vec<_> = (0..2).filter_map(|_| std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(1)).ok()).collect();
    let t = setup_with(
        move |u, _| model("test-conn", &[prov("full", &format!("http://{addr}"), "x", None, ""), prov("b", &format!("{u}/b"), "x", None, "")]),
        |o, _| {
            o.upstream_connect = Duration::from_millis(300);
            o.upstream_read_idle = Duration::from_secs(60);
        },
    );
    let g = session(&t.h, Agent::ClaudeCode, None);
    let t0 = Instant::now();
    assert_eq!(post(&t.h, &g, M, msg("test-conn"), &[]).0, 200);
    assert_eq!(t.up.count("/b"), 1);
    assert!(t0.elapsed() < Duration::from_secs(10), "{:?}", t0.elapsed());
    drop(l);
} // mutation: no `.connect_timeout(..)` (the 60 s read bound then ends the connect as a non-connect error: a 503 after
  // 60 s). Linux only: macOS and Windows refuse or reset a connect to a full queue instead of dropping it.
