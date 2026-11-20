//! N12: redaction stage (§7.3 stage 6, §16, §17.1). Fake upstream on 127.0.0.1, no network.
//! Secret samples are built at run time; flagged words are the tracer `plimsoq` from a test deny list.
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use axum::extract::Request;
use axum::response::Response;
use bytes::Bytes;
use http::HeaderMap;
use serde_json::{json, Value};
use tempfile::TempDir;
use zygo_config::{load, Dialect, LoadContext};
use zygo_core::{ids::ulid, paths::Paths, Agent};
use zygo_proxy::errors::ZygoError;
use zygo_proxy::pipeline::{ModelRoute, RequestCtx, Stage};
use zygo_proxy::redact_stage::{RedactLimits, RedactPass, RedactStage};
use zygo_proxy::server::StartOpts;
use zygo_proxy::session::{SessionGrant, SessionRequest};
use zygo_proxy::{Proxy, ProxyHandle};
use zygo_redact::act_lite_store::StoredValue;
use zygo_redact::DeadlinePolicy;
use zygo_usage::{RedactionCount, RequestRow, Store};

const M: &str = "anthropic/v1/messages";
const CT: &str = "anthropic/v1/messages/count_tokens";
const TRACER: &str = "plimsoq"; // deny-list word; no real word list
const REPL: &str = "🤬";          // §16.2 default replacement

type Got = Arc<Mutex<Vec<(String, Vec<u8>)>>>;

struct Upstream { addr: SocketAddr, got: Got, _stop: tokio::sync::oneshot::Sender<()> }
impl Upstream {
    /// 127.0.0.1:0, own runtime thread; every request is recorded (path with query, body) and answered 200 JSON.
    fn start() -> Upstream {
        let got: Got = Arc::default();
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
                    let g3 = g2.clone();
                    async move {
                        let path = req.uri().path_and_query().map(|p| p.as_str().to_string()).unwrap_or_default();
                        let body = axum::body::to_bytes(req.into_body(), usize::MAX).await.unwrap().to_vec();
                        g3.lock().unwrap().push((path, body));
                        Response::builder()
                            .status(200)
                            .header("content-type", "application/json")
                            .body(axum::body::Body::from(
                                r#"{"id":"m","type":"message","role":"assistant","content":[],"usage":{"input_tokens":1,"output_tokens":1}}"#,
                            ))
                            .unwrap()
                    }
                });
                tokio::select! {
                    _ = async { axum::serve(l, app).await } => {}
                    _ = rx => {}
                }
            });
        });
        Upstream { addr, got, _stop: tx }
    }
    fn url(&self) -> String { format!("http://127.0.0.1:{}", self.addr.port()) }
    fn bodies(&self) -> Vec<Vec<u8>> { self.got.lock().unwrap().iter().map(|g| g.1.clone()).collect() }
    fn paths(&self) -> Vec<String> { self.got.lock().unwrap().iter().map(|g| g.0.clone()).collect() }
}

struct T { d: TempDir, up: Upstream, h: ProxyHandle }

/// A catalog model `test-same` (upstream_model equal to its id, so an unchanged body is sent as received, §7.3), `test-none`
/// (no provider), a block pattern `tracer_block`, deny list `["plimsoq"]`; `extra` is more TOML (appended last).
fn setup(extra: &str, limits: RedactLimits) -> T {
    let d = TempDir::new().unwrap();
    let up = Upstream::start();
    let p = d.path();
    std::fs::write(p.join("config.toml"), BASE.replace("{URL}", &up.url()) + extra).unwrap();
    let paths = Paths { config_file: p.join("config.toml"), cache_dir: p.join("cache"), data_dir: p.join("data"), sessions_dir: p.join("cache/sessions"),
                        marketplaces_dir: p.join("cache/marketplaces"), plugin_store: p.join("data/plugins"), approvals_file: p.join("data/approvals.toml"),
                        usage_db: p.join("data/usage.db") };
    let cfg = load(&LoadContext { paths, repo: None, team: None }).unwrap();
    let o = StartOpts { usage_db: Some(p.join("data/usage.db")), redact_limits: Some(limits), ..Default::default() };
    let h = Proxy::start_local_with(Arc::new(cfg), o).unwrap();
    T { d, up, h }
}
/// The bytes of every file under `dir` except the input `config.toml` (the usage database and its journal included).
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
const BASE: &str = r#"
[[model]]
id = "test-same"
[[model.provider]]
name = "p"
base_url = "{URL}"
dialects = ["anthropic"]
upstream_model = "test-same"

[[model]]
id = "test-none"

[[redaction.pattern]]
id = "tracer_block"
pattern = "TRACERBLOCK_[0-9]{6}"
action = "block"

[redaction.content]
deny = ["plimsoq"]
"#;

/// A pass that must finish never hits its deadline.
fn generous() -> RedactLimits {
    RedactLimits { secret: DeadlinePolicy { base: Duration::from_secs(60), per_mib: Duration::ZERO, expire_after_scans: None }, content_budget: Duration::from_secs(60), ..Default::default() }
}
fn grant_with(t: &T, agent: Agent, stored: Vec<StoredValue>) -> SessionGrant {
    t.h.create_session(SessionRequest {
        agent,
        agent_version: "2.1.328".into(),
        profile: None,
        passthrough: false,
        repo: None,
        work: None,
        stored_values: stored,
        known_plugins: Default::default(),
    })
    .unwrap()
}
fn grant(t: &T, stored: Vec<StoredValue>) -> SessionGrant { grant_with(t, Agent::ClaudeCode, stored) }
fn key_sample() -> String { ["sk-", "ant-api03-", &"Ab1_".repeat(24)].concat() }   // built-in `anthropic_api_key`
fn block_sample() -> String { format!("TRACERBLOCK_{}", 123456) }
fn body(messages: Value) -> Value { json!({"model": "test-same", "max_tokens": 64, "messages": messages}) }
/// The 8-distinct-string conversation of the cache and timeout tests; strings in scope: "test-same", "user", "q1", "assistant", "a1", "tool_result", "t1", <key text>.
fn conv(key_text: &str) -> Value {
    body(json!([{"role":"user","content":"q1"},{"role":"assistant","content":"a1"},
                {"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":key_text}]}]))
}
fn to_vec(v: &Value) -> Vec<u8> { serde_json::to_vec(v).unwrap() }
fn has(hay: &[u8], needle: &[u8]) -> bool { hay.windows(needle.len()).any(|w| w == needle) }
fn auth(g: &SessionGrant) -> Vec<(&str, &str)> { vec![("x-api-key", g.token.as_str()), ("x-zygo-session", g.session.as_str())] }
/// (status, headers, body); a request to the proxy with the session's credentials plus `extra`.
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
fn post(t: &T, g: &SessionGrant, path: &str, b: &Value, extra: &[(&str, &str)]) -> (u16, HeaderMap, Vec<u8>) {
    let mut hd = auth(g);
    hd.extend_from_slice(extra);
    send(t, "POST", path, &hd, Some(to_vec(b)))
}
/// (status, error.type); asserts the Anthropic error shape.
fn st(r: &(u16, HeaderMap, Vec<u8>)) -> (u16, String) {
    let v: Value = serde_json::from_slice(&r.2).unwrap_or_else(|_| panic!("not JSON"));
    assert_eq!(v["type"], "error");
    assert!(v["error"]["message"].is_string());
    (r.0, v["error"]["type"].as_str().unwrap().to_string())
}
/// The `error.message` of an error reply.
fn msg(r: &(u16, HeaderMap, Vec<u8>)) -> String {
    serde_json::from_slice::<Value>(&r.2).unwrap()["error"]["message"].as_str().unwrap().to_string()
}
/// `n` = requests this proxy has handled so far; use one request per session when asserting a row.
fn row(t: &T, g: &SessionGrant, n: u64) -> RequestRow {
    assert!(t.h.usage_written(n, Duration::from_secs(10)), "usage rows not written");
    let mut v = Store::open(&t.d.path().join("data/usage.db")).unwrap().requests_for_session(&g.session).unwrap();
    assert_eq!(v.len(), 1);
    v.remove(0)
}
fn up_json(t: &T, i: usize) -> Value { serde_json::from_slice(&t.up.bodies()[i]).unwrap() }
fn scans(t: &T, g: &SessionGrant) -> (u64, u64) {
    let s = t.h.session(&g.session).unwrap();
    // A pass that panicked held its lock: the poisoned guard is read all the same.
    let a = s.redaction.secret_cache.lock().unwrap_or_else(|e| e.into_inner()).scan_count();
    let b = s.redaction.content_cache.lock().unwrap_or_else(|e| e.into_inner()).scan_count();
    (a, b)
}
fn count(id: &str, n: u32) -> RedactionCount { RedactionCount { id: id.into(), count: n } }
/// A `RequestCtx` as stage 6 sees it (stages 2-5 skipped).
fn ctx(t: &T, g: &SessionGrant, b: Value, d: Dialect) -> RequestCtx {
    let s = t.h.session(&g.session).unwrap();
    RequestCtx { cfg: s.cfg.clone(), session: s, route: ModelRoute::Messages, dialect: d, headers: HeaderMap::new(), raw: Bytes::new(), body: b,
                 changed: false, resolved: None, candidates: vec![], adjustments: vec![], redactions: BTreeMap::new(), redacted: false, line_mode: false }
}


#[test] // §20 #29 first part; §7.3 header; §14 row
fn secret_key_in_tool_result_redacted_and_header_set() {
    let t = setup("", generous()); let g = grant(&t, vec![]); let key = key_sample();
    let b = body(json!([{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":format!("env: {key} end")}]}]));
    let (s, h, _) = post(&t, &g, M, &b, &[]);
    assert_eq!((s, h.get("x-zygo-redacted").map(|v| v.to_str().unwrap())), (200, Some("1")));
    assert_eq!(up_json(&t, 0)["messages"][0]["content"][0]["content"], "env: [REDACTED:anthropic_api_key] end");
    assert!(!has(&t.up.bodies()[0], key.as_bytes()));
    assert_eq!(row(&t, &g, 1).redactions, vec![count("anthropic_api_key", 1)]);
}

#[test] // §16.1 block; N11 note (findings on the refusal row); readings 3, 5
fn secret_block_pattern_refuses_422_ids_only() {
    let t = setup("", generous()); let g = grant(&t, vec![]); let v = block_sample();
    // A tracer word in a user text: the content pass must not run after a block.
    let b = body(json!([{"role":"user","content":format!("{TRACER} {v}")}]));
    let r = post(&t, &g, M, &b, &[]);
    assert_eq!(st(&r), (422, "zygo_redaction_block".to_string()));
    assert_eq!(msg(&r), "blocked by redaction pattern: tracer_block");
    assert!(!has(&r.2, v.as_bytes()) && r.1.get("x-zygo-redacted").is_none());
    assert_eq!(t.up.bodies().len(), 0);
    assert_eq!(scans(&t, &g).1, 0);                              // content pass never ran
    let w = row(&t, &g, 1);
    assert_eq!((w.status, w.redactions), (422, vec![count("tracer_block", 1)]));
}

#[test] // §20 #29: binary payload by path not scanned; a 70 KiB base64-looking string with a key at its end is
fn secret_binary_payload_unscanned_long_string_scanned() {
    let t = setup("", generous()); let g = grant(&t, vec![]); let key = key_sample();
    let long = format!("{} {key}", "QUJD".repeat(17_920));       // 70 KiB
    let img = json!({"type":"image","source":{"type":"base64","media_type":"image/png","data":key}});
    let b = body(json!([{"role":"user","content":[img, {"type":"tool_result","tool_use_id":"t1","content":long}]}]));
    assert_eq!(post(&t, &g, M, &b, &[]).0, 200);
    let v = up_json(&t, 0);
    assert_eq!(v["messages"][0]["content"][0]["source"]["data"], key.as_str());
    let out = v["messages"][0]["content"][1]["content"].as_str().unwrap();
    assert!(out.ends_with(" [REDACTED:anthropic_api_key]") && out.len() == 71_680 + " [REDACTED:anthropic_api_key]".len());
    assert_eq!(row(&t, &g, 1).redactions, vec![count("anthropic_api_key", 1)]);
}

#[test] // §20 #29c first half (thinking unchanged); §16.1 scope (a); body otherwise unchanged → received bytes
fn secret_thinking_block_forwarded_unchanged() {
    let t = setup("", generous()); let g = grant(&t, vec![]);
    let pw = ["pass", "word=\"fixture-", "pass-123\""].concat();   // built at run time
    let sig = format!("s\u{ef}g\n\"q\" {}\u{1F600}", ulid());   // non-ASCII and escaped characters: a re-serialisation change shows
    let think = json!({"type":"thinking","thinking":pw,"signature":sig});
    // (1) only the thinking block holds the assignment: nothing changes, the received bytes go out.
    let only = body(json!([{"role":"user","content":"hi"},{"role":"assistant","content":[think.clone(), {"type":"text","text":"ok"}]}]));
    let raw = to_vec(&only);
    let (s, h, _) = send(&t, "POST", M, &auth(&g), Some(raw.clone()));
    assert_eq!((s, h.get("x-zygo-redacted").is_none()), (200, true));
    assert_eq!(t.up.bodies()[0], raw);
    // (2) the same text in a text block is redacted; the thinking block is still byte-identical.
    let both = body(json!([{"role":"assistant","content":[think.clone(), {"type":"text","text":pw}]}]));
    assert_eq!(post(&t, &g, M, &both, &[]).0, 200);
    let v = up_json(&t, 1);
    assert_eq!(v["messages"][0]["content"][0], think);
    assert!(has(&t.up.bodies()[1], &to_vec(&think)));            // and the same bytes
    assert_eq!(v["messages"][0]["content"][1]["text"], "password=\"[REDACTED:generic_secret_assignment]\"");
}

#[test] // §20 #29d local part: 6-byte rule is N10's; here the stage applies the exact-value pattern
fn secret_stored_value_redacted_short_value_not() {
    let t = setup("", generous());
    let (v, short) = (format!("v{}", ulid()), ulid()[..5].to_string());
    let g = grant(&t, vec![StoredValue::new(v.clone()), StoredValue::new(short.clone())]);
    let b = body(json!([{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":format!("a {v} b {short} c")}]}]));
    assert_eq!(post(&t, &g, M, &b, &[]).0, 200);
    assert_eq!(up_json(&t, 0)["messages"][0]["content"][0]["content"], format!("a [REDACTED:act_lite_secret] b {short} c"));
    assert_eq!(row(&t, &g, 1).redactions, vec![count("act_lite_secret", 1)]);
}

#[test] // §20 #30; §16.1 "a retry continues where the last attempt stopped"
fn secret_timeout_refuses_503_and_retry_continues() {
    let l = RedactLimits { secret: DeadlinePolicy { expire_after_scans: Some(5), ..generous().secret }, ..generous() };
    let t = setup("", l); let key = key_sample();
    let g = grant(&t, vec![]);
    let r = post(&t, &g, M, &conv(&format!("env: {key}")), &[]);
    assert_eq!((st(&r), msg(&r).as_str()), ((503, "zygo_redaction_unavailable".to_string()), "redaction did not finish in time"));
    assert!(r.1.get("x-zygo-redacted").is_none() && !has(&r.2, key.as_bytes()));
    assert_eq!((t.up.bodies().len(), scans(&t, &g).0), (0, 5));      // no upstream request; 5 strings scanned and cached
    assert_eq!(post(&t, &g, M, &conv(&format!("env: {key}")), &[]).0, 200);
    assert_eq!(scans(&t, &g).0, 8);                                  // the retry scanned only the remaining 3
    assert_eq!(up_json(&t, 0)["messages"][2]["content"][0]["content"], "env: [REDACTED:anthropic_api_key]");
}

#[test] // §16.1 "timeout or panic → 503"; the panic is raised while the cache lock is held
fn secret_panic_refuses_503_then_next_request_passes() {
    let t = setup("", RedactLimits { panic_once: Some(RedactPass::Secret), ..generous() }); let g = grant(&t, vec![]);
    let b = conv(&key_sample());
    let r = post(&t, &g, M, &b, &[]);
    assert_eq!((st(&r), msg(&r).as_str()), ((503, "zygo_redaction_unavailable".to_string()), "redaction failed"));
    assert_eq!(t.up.bodies().len(), 0);
    assert_eq!(post(&t, &g, M, &b, &[]).0, 200);                     // the poisoned cache lock is used again
    assert_eq!(up_json(&t, 0)["messages"][2]["content"][0]["content"], "[REDACTED:anthropic_api_key]");
}

#[test] // review focus 3, §20 #29a, §16.1 session cache
fn secret_second_pass_scans_only_new_strings() {
    let t = setup("", generous()); let g = grant(&t, vec![]); let key_text = format!("env: {}", key_sample());
    assert_eq!(post(&t, &g, M, &conv(&key_text), &[]).0, 200);
    assert_eq!(scans(&t, &g).0, 8);                                  // 8 distinct strings
    assert_eq!(post(&t, &g, M, &conv(&key_text), &[]).0, 200);
    assert_eq!(scans(&t, &g).0, 8);                                  // the same conversation again: 0 scans
    let mut next = conv(&key_text);                                  // the conversation plus 2 new strings
    next["messages"].as_array_mut().unwrap().extend([json!({"role":"assistant","content":"a2"}), json!({"role":"user","content":"q3"})]);
    assert_eq!(post(&t, &g, M, &next, &[]).0, 200);
    assert_eq!(scans(&t, &g).0, 10);                                 // "a2" and "q3" only
    let up = t.up.bodies();
    assert_eq!(up[0], up[1]);                                        // output bytes identical: the prompt cache is kept
    let (a, c) = (up_json(&t, 0), up_json(&t, 2));
    assert_eq!(c["messages"].as_array().unwrap()[..3], a["messages"].as_array().unwrap()[..]);
    assert_eq!(a["messages"][2]["content"][0]["content"], "env: [REDACTED:anthropic_api_key]");
}

#[test] // review focus 2 (N12 part): nothing changed → the received bytes, odd spacing included
fn secret_unchanged_body_forwarded_byte_for_byte() {
    let t = setup("", generous()); let g = grant(&t, vec![]);
    let raw = br#"{ "model" : "test-same",  "max_tokens":64, "messages":[ {"role":"user","content":"hello"} ] }"#.to_vec();
    let (s, h, _) = send(&t, "POST", M, &auth(&g), Some(raw.clone()));
    assert_eq!((s, h.get("x-zygo-redacted").is_none()), (200, true));
    assert_eq!(t.up.bodies()[0], raw);
    assert!(row(&t, &g, 1).redactions.is_empty());
}

#[test] // §16 intro "`count_tokens` is scanned too"
fn secret_count_tokens_is_scanned() {
    let t = setup("", generous()); let g = grant(&t, vec![]); let key = key_sample();
    let b = body(json!([{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":key}]}]));
    let (s, h, _) = post(&t, &g, CT, &b, &[]);
    assert_eq!((s, h.get("x-zygo-redacted").map(|v| v.to_str().unwrap())), (200, Some("1")));
    assert_eq!(t.up.paths(), vec!["/v1/messages/count_tokens".to_string()]);
    assert_eq!(up_json(&t, 0)["messages"][0]["content"][0]["content"], "[REDACTED:anthropic_api_key]");
}

#[test] // §7.3 note (a): a repeated key (top level, nested, escaped) is sent serialized from the scanned value
fn repeated_key_body_serialized_from_scanned_value() {
    let t = setup("", generous()); let g = grant(&t, vec![]);
    let pw = ["pass", "word=Abcdefghij", "klmnop1"].concat();
    let want = br#"{"model":"test-same","max_tokens":64,"messages":[{"role":"user","content":"hi"}]}"#.to_vec();
    let raws = [
        format!(r#"{{"model":"test-same","max_tokens":64,"messages":[{{"role":"user","content":"{pw}"}}],"messages":[{{"role":"user","content":"hi"}}]}}"#),
        format!(r#"{{"model":"test-same","max_tokens":64,"messages":[{{"role":"user","content":"{pw}","content":"hi"}}]}}"#),
        format!(r#"{{"model":"test-same","max_tokens":64,"\u006dessages":[{{"role":"user","content":"{pw}"}}],"messages":[{{"role":"user","content":"hi"}}]}}"#),
    ];
    assert!(raws[2].contains(r"\u006dessages") && raws[2] != raws[0]);  // the escaped first key reaches the proxy as written
    for (i, raw) in raws.iter().enumerate() {
        let (s, h, _) = send(&t, "POST", M, &auth(&g), Some(raw.clone().into_bytes()));
        assert_eq!((s, h.get("x-zygo-redacted").is_none()), (200, true), "{i}");
        let up = &t.up.bodies()[i];
        assert!(!has(up, pw.as_bytes()), "{i}");
        assert_eq!(up, &want, "{i}");
    }
}

#[test] // §7.3 note (a) on count_tokens (§16 intro: scanned too)
fn repeated_key_count_tokens_serialized() {
    let t = setup("", generous()); let g = grant(&t, vec![]);
    let pw = ["pass", "word=Abcdefghij", "klmnop1"].concat();
    let raw = format!(r#"{{"model":"test-same","messages":[{{"role":"user","content":"{pw}"}}],"messages":[{{"role":"user","content":"hi"}}]}}"#);
    assert_eq!(send(&t, "POST", CT, &auth(&g), Some(raw.into_bytes())).0, 200);
    assert_eq!(t.up.paths(), vec!["/v1/messages/count_tokens".to_string()]);
    assert_eq!(t.up.bodies()[0], br#"{"model":"test-same","messages":[{"role":"user","content":"hi"}]}"#.to_vec());
}

#[test] // §7.3 note (a), C4 (round 2 B1): a serde_json private key is refused 400 before any stage; nothing goes upstream
fn reserved_key_refused_400() {
    let t = setup("", generous()); let g = grant(&t, vec![]);
    let pw = ["pass", "word=Abcdefghij", "klmnop1"].concat();
    let msgs = r#""messages":[{"role":"user","content":"hi"}]"#;
    let mut nested = String::from("1");
    for _ in 0..16 {
        nested = format!("{}{{\"$serde_json::private::RawValue\":{}}}{}", "[".repeat(120), serde_json::to_string(&nested).unwrap(), "]".repeat(120));
    }
    let cases = [
        (M, format!(r#"{{"model":"test-same","max_tokens":64,{msgs},"metadata":{{"$serde_json::private::RawValue":"{{\"user_id\":\"{pw}\",\"user_id\":\"u\"}}"}}}}"#)),
        (CT, format!(r#"{{"model":"test-same","messages":{{"$serde_json::private::RawValue":"[{{\"role\":\"user\",\"content\":\"{pw}\",\"content\":\"hi\"}}]"}}}}"#)),
        (M, format!(r#"{{"model":"test-same","max_tokens":64,{msgs},"metadata":{{"\u0024serde_json::private::RawValue":"{{\"user_id\":\"{pw}\",\"user_id\":\"u\"}}"}}}}"#)),
        (M, format!(r#"{{"model":"test-same","max_tokens":64,{msgs},"metadata":{{"$serde_json::private::Number":"12345678"}}}}"#)),
        (M, format!(r#"{{"model":"test-same","max_tokens":64,{msgs},"metadata":{nested}}}"#)),
    ];
    assert!(cases[2].1.contains(r"\u0024serde_json") && !cases[2].1.contains("$serde_json"));
    for (i, (path, raw)) in cases.iter().enumerate() {
        let r = send(&t, "POST", path, &auth(&g), Some(raw.clone().into_bytes()));
        assert_eq!(st(&r), (400, "zygo_bad_request".to_string()), "{i}");
    }
    assert!(t.up.bodies().is_empty());
    assert!(read_all(t.d.path()).iter().all(|f| !has(f, pw.as_bytes())));
}

#[test] // round 3 R2-1, C4: a repeat before a serde_json private key does not hide it: 400, nothing upstream
fn repeat_before_reserved_key_refused_400() {
    let t = setup("", generous()); let g = grant(&t, vec![]);
    let pw = ["pass", "word=Abcdefghij", "klmnop1"].concat();
    let raw = format!(r#"{{"model":"test-same","max_tokens":64,"max_tokens":64,"messages":[{{"role":"user","content":"hi"}}],"metadata":{{"$serde_json::private::RawValue":"{{\"user_id\":\"{pw}\",\"user_id\":\"u\"}}"}}}}"#);
    assert_eq!(st(&send(&t, "POST", M, &auth(&g), Some(raw.into_bytes()))), (400, "zygo_bad_request".to_string()));
    assert!(t.up.bodies().is_empty());
    assert!(read_all(t.d.path()).iter().all(|f| !has(f, pw.as_bytes())));
}

#[test] // round 3 R2-1 (N2): nested private keys after a repeat are refused before the parse expands them
fn repeat_before_nested_reserved_key_refused_400() {
    let t = setup("", generous()); let g = grant(&t, vec![]);
    let mut nested = String::from("1");
    for _ in 0..10 {
        nested = format!("{}{{\"$serde_json::private::RawValue\":{}}}{}", "[".repeat(120), serde_json::to_string(&nested).unwrap(), "]".repeat(120));
    }
    let raw = format!(r#"{{"model":"test-same","max_tokens":64,"max_tokens":64,"messages":[{{"role":"user","content":"hi"}}],"metadata":{nested}}}"#);
    assert_eq!(st(&send(&t, "POST", M, &auth(&g), Some(raw.into_bytes()))), (400, "zygo_bad_request".to_string()));
    assert!(t.up.bodies().is_empty());
}

#[test] // review focus 2: no repeat in any one object → the received bytes
fn unique_keys_body_forwarded_byte_for_byte() {
    let t = setup("", generous()); let g = grant(&t, vec![]);
    let raw = br#"{ "model" : "test-same", "max_tokens": 1.0e2, "metadata": {"model": "x"},
  "messages": [ {"role":"user","content":"a \/ b"}, {"role":"user","content":"c"} ] }"#.to_vec();
    assert_eq!(send(&t, "POST", M, &auth(&g), Some(raw.clone())).0, 200);
    assert_eq!(t.up.bodies()[0], raw);
}

#[test] // §7.3 note (b): a name refused at stage 2 is still recorded redacted; no file holds it
fn role_requested_recorded_redacted() {
    let t = setup("", generous()); let g = grant(&t, vec![]);
    let pw = ["pass", "word=Abcdefghij", "klmnop1"].concat();
    let b = json!({"model": pw, "max_tokens": 64, "messages": [{"role":"user","content":"hi"}]});
    assert_eq!(st(&post(&t, &g, M, &b, &[])), (400, "zygo_unknown_model".to_string()));
    let r = row(&t, &g, 1);
    assert_eq!(r.role_requested.as_deref(), Some("password=[REDACTED:generic_secret_assignment]"));
    assert!(r.redactions.is_empty());
    assert!(read_all(t.d.path()).iter().all(|f| !has(f, pw.as_bytes())));
}

#[test] // §7.3 note (b): a name a `block` pattern matches is not recorded
fn role_requested_block_match_not_recorded() {
    let t = setup("", generous()); let g = grant(&t, vec![]);
    let b = json!({"model": block_sample(), "max_tokens": 64, "messages": [{"role":"user","content":"hi"}]});
    assert_eq!(st(&post(&t, &g, M, &b, &[])), (400, "zygo_unknown_model".to_string()));
    let r = row(&t, &g, 1);
    assert_eq!((r.status, r.role_requested), (400, None));
}

#[test] // §20 #31; §16.2 user-text paths and harness skips
fn content_user_text_only_command_args_included() {
    let t = setup("", generous());
    let (g1, g2) = (grant(&t, vec![]), grant(&t, vec![]));
    let b = body(json!([
        {"role":"user","content":[{"type":"text","text":format!("say {TRACER} now")}]},
        {"role":"assistant","content":[{"type":"text","text":format!("I will not say {TRACER}")}]},
        {"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":format!("file says {TRACER}")},
                                  {"type":"text","text":format!("<system-reminder>{TRACER}</system-reminder>")}]}]));
    let (s, h, _) = post(&t, &g1, M, &b, &[]);
    assert_eq!((s, h.get("x-zygo-redacted").map(|v| v.to_str().unwrap())), (200, Some("1")));
    let v = up_json(&t, 0)["messages"].clone();
    assert_eq!(v[0]["content"][0]["text"], "say 🤬 now");
    assert_eq!((&v[1], &v[2]), (&b["messages"][1], &b["messages"][2]));      // assistant, tool_result, system-reminder: unchanged
    assert_eq!(row(&t, &g1, 1).redactions, vec![count("content_profane", 1)]);
    // A slash command: the typed arguments are filtered in both blocks.
    let c = body(json!([{"role":"user","content":[
        {"type":"text","text":format!("<command-message>x</command-message>\n<command-args>{TRACER} zorp</command-args>")},
        {"type":"text","text":format!("All: {TRACER} zorp. First: {TRACER}.")}]}]));
    assert_eq!(post(&t, &g2, M, &c, &[]).0, 200);
    assert_eq!(up_json(&t, 1)["messages"][0]["content"][1]["text"], "All: 🤬 zorp. First: 🤬.");
    assert_eq!(row(&t, &g2, 2).redactions, vec![count("content_profane", 3)]);   // 1 in the arguments block, 2 in the expanded file (N9 `whole_arguments_and_positional_copies_are_both_filtered`)
}

#[test] // §16.2 gate, N2 note: main, compaction, auxiliary, absent run; subagent does not
fn content_request_class_gate() {
    let t = setup("", generous()); let g = grant(&t, vec![]);
    let b = body(json!([{"role":"user","content":format!("say {TRACER}")}]));
    let want = [(Some("main"), true), (Some("compaction"), true), (Some("auxiliary"), true), (None, true), (Some("subagent"), false)];
    for (i, (class, filtered)) in want.into_iter().enumerate() {
        let extra: Vec<(&str, &str)> = class.map(|c| ("x-claude-code-request-class", c)).into_iter().collect();
        assert_eq!(post(&t, &g, M, &b, &extra).0, 200);
        let sent = up_json(&t, i)["messages"][0]["content"].as_str().unwrap().to_string();
        assert_eq!(sent, if filtered { "say 🤬".to_string() } else { format!("say {TRACER}") }, "{class:?}");
    }
}

#[test] // §17.1 PATCH …/settings `redaction_content`; acceptance: content off, secret pass still on
fn content_off_per_session_via_patch() {
    let t = setup("", generous()); let g = grant(&t, vec![]); let key = key_sample();
    let b = body(json!([{"role":"user","content":format!("say {TRACER}")}]));
    let patch = |on: bool| {
        let path = format!("v1/zygo/sessions/{}/settings", g.session);
        let r = send(&t, "PATCH", &path, &auth(&g), Some(to_vec(&json!({"redaction_content": on}))));
        assert_eq!((r.0, serde_json::from_slice::<Value>(&r.2).unwrap()), (200, json!({"redaction_content": on})));
    };
    post(&t, &g, M, &b, &[]);
    assert_eq!(up_json(&t, 0)["messages"][0]["content"], "say 🤬");
    patch(false);
    post(&t, &g, M, &b, &[]);                                         // nothing changes: the received bytes
    assert_eq!(t.up.bodies()[1], to_vec(&b));
    let both = body(json!([{"role":"user","content":[{"type":"text","text":format!("say {TRACER}")},{"type":"tool_result","tool_use_id":"t1","content":key}]}]));
    post(&t, &g, M, &both, &[]);                                      // secret pass still runs, tracer stays
    let v = up_json(&t, 2);
    assert_eq!((v["messages"][0]["content"][0]["text"].clone(), v["messages"][0]["content"][1]["content"].clone()),
               (json!(format!("say {TRACER}")), json!("[REDACTED:anthropic_api_key]")));
    patch(true);
    post(&t, &g, M, &b, &[]);
    assert_eq!(up_json(&t, 3)["messages"][0]["content"], "say 🤬");
}

#[test] // §20 #32; §16.2 timeout fails open, `content_timeout` recorded; review focus 10 (HTTP half)
fn content_timeout_fails_open_and_is_recorded() {
    let t = setup("", RedactLimits { content_budget: Duration::ZERO, ..generous() }); let g = grant(&t, vec![]);
    let big = format!("{TRACER} {}", "word ".repeat(14_000));          // crosses one 64 KiB check
    let b = body(json!([{"role":"user","content":format!("first {TRACER}")},{"role":"user","content":big.clone()}]));
    let (s, h, _) = post(&t, &g, M, &b, &[]);
    assert_eq!((s, h.get("x-zygo-redacted").map(|v| v.to_str().unwrap())), (200, Some("1")));   // the request goes on
    let v = up_json(&t, 0)["messages"].clone();
    assert_eq!((v[0]["content"].as_str().unwrap(), &v[1]), ("first 🤬", &b["messages"][1]));    // the late string goes out unchanged
    assert_eq!(row(&t, &g, 1).redactions, vec![count("content_profane", 1), count("content_timeout", 1)]);
    // A timeout that redacted nothing: no header, the received bytes, the finding on the row (reading 3).
    let g2 = grant(&t, vec![]);
    let raw = to_vec(&body(json!([{"role":"user","content":big}])));
    let (s, h, _) = send(&t, "POST", M, &auth(&g2), Some(raw.clone()));
    assert_eq!((s, h.get("x-zygo-redacted").is_none(), t.up.bodies()[1].clone()), (200, true, raw));
    assert_eq!(row(&t, &g2, 2).redactions, vec![count("content_timeout", 1)]);
}

#[test] // §16.2 "A panic refuses 503"
fn content_panic_refuses_503_then_next_request_passes() {
    let t = setup("", RedactLimits { panic_once: Some(RedactPass::Content), ..generous() }); let g = grant(&t, vec![]);
    let b = body(json!([{"role":"user","content":format!("say {TRACER}")}]));
    let r = post(&t, &g, M, &b, &[]);
    assert_eq!((st(&r), msg(&r).as_str()), ((503, "zygo_redaction_unavailable".to_string()), "redaction failed"));
    assert_eq!(t.up.bodies().len(), 0);
    assert_eq!(scans(&t, &g).0, 3);                                   // the secret pass ran and left the hook for the content pass
    assert_eq!(post(&t, &g, M, &b, &[]).0, 200);
    assert_eq!(up_json(&t, 0)["messages"][0]["content"], "say 🤬");
}

#[test] // §15.4 toggle (reading 2): secret pass skipped, content pass still runs (needs step 6)
fn content_pass_still_runs_when_secrets_toggle_off() {
    let t = setup("\n[features.redaction]\nsecrets = false\n", generous()); let g = grant(&t, vec![]); let key = key_sample();
    let b = body(json!([{"role":"user","content":[{"type":"text","text":TRACER},{"type":"tool_result","tool_use_id":"t1","content":key}]}]));
    assert_eq!(post(&t, &g, M, &b, &[]).0, 200);
    let v = up_json(&t, 0);
    assert_eq!((v["messages"][0]["content"][1]["content"].as_str().unwrap(), v["messages"][0]["content"][0]["text"].as_str().unwrap()), (key.as_str(), REPL));
    assert_eq!(row(&t, &g, 1).redactions, vec![count("content_profane", 1)]);
}

#[test] // §16.2 "OpenCode only: <file …> spans are skipped"; the agent is the session's (reading 8; the layout is the same for both agents)
fn content_agent_reaches_the_pass() {
    let t = setup("", generous());
    let b = body(json!([{"role":"user","content":format!("see <file path=\"a.rs\">{TRACER}</file> and {TRACER}")}]));
    let (cc, oc) = (grant_with(&t, Agent::ClaudeCode, vec![]), grant_with(&t, Agent::OpenCode, vec![]));
    post(&t, &cc, M, &b, &[]);
    post(&t, &oc, M, &b, &[]);
    assert_eq!(up_json(&t, 0)["messages"][0]["content"], "see <file path=\"a.rs\">🤬</file> and 🤬");
    assert_eq!(up_json(&t, 1)["messages"][0]["content"], "see <file path=\"a.rs\">plimsoq</file> and 🤬");
}

#[test] // §7.3 stage order: stage 2 refuses before stage 6 looks at the body
fn order_resolution_error_comes_before_redaction() {
    let t = setup("", generous()); let g = grant(&t, vec![]);
    let b = json!({"model":"test-none","max_tokens":64,"messages":[{"role":"user","content":block_sample()}]});
    assert_eq!(st(&post(&t, &g, M, &b, &[])), (400, "zygo_no_provider".to_string()));
}

#[test] // §16.1 / §16.2 default values
fn stage_limits_default_to_spec_values() {
    let l = RedactLimits::default();
    assert_eq!((l.secret.base, l.secret.per_mib, l.secret.expire_after_scans, l.content_budget),
               (Duration::from_millis(200), Duration::from_millis(50), None, Duration::from_millis(200)));
    assert_eq!((l.panic_once, l.extra_wait), (None, Duration::ZERO));
}

#[test] // N8 round 2 / N11 note: `changed` only when a string changed
fn stage_changed_only_when_a_string_changed() {
    let t = setup("", generous()); let g = grant(&t, vec![]); let stage = RedactStage::with_limits(generous());
    let mut c = ctx(&t, &g, body(json!([{"role":"user","content":"hello"}])), Dialect::Anthropic);
    stage.on_request(&mut c).unwrap();
    assert_eq!((c.changed, c.redacted, c.redactions.is_empty()), (false, false, true));
    let mut c = ctx(&t, &g, body(json!([{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":key_sample()}]}])), Dialect::Anthropic);
    stage.on_request(&mut c).unwrap();
    assert_eq!((c.changed, c.redacted, c.redactions), (true, true, BTreeMap::from([("anthropic_api_key".to_string(), 1)])));
    let mut c = ctx(&t, &g, body(json!([{"role":"user","content":format!("say {TRACER}")}])), Dialect::Anthropic);
    stage.on_request(&mut c).unwrap();
    assert_eq!((c.changed, c.redacted, c.redactions), (true, true, BTreeMap::from([("content_profane".to_string(), 1)])));
}

#[test] // N11 note: findings set on a 422; `redacted` stays false (review 2026-11-19)
fn stage_block_sets_findings_and_flag() {
    let t = setup("", generous()); let g = grant(&t, vec![]); let stage = RedactStage::with_limits(generous()); let v = block_sample();
    let mut c = ctx(&t, &g, body(json!([{"role":"user","content":v.clone()}])), Dialect::Anthropic);
    let e: ZygoError = stage.on_request(&mut c).unwrap_err();
    assert_eq!((e.status.as_u16(), e.code, e.message.as_str()), (422, "zygo_redaction_block", "blocked by redaction pattern: tracer_block"));
    assert_eq!((c.redacted, c.redactions), (false, BTreeMap::from([("tracer_block".to_string(), 1)])));
    assert!(!e.message.contains(&v));
}

#[test] // §16.2 user-text paths by client dialect (Responses `input`, chat/completions `messages`)
fn stage_layout_follows_dialect() {
    let t = setup("", generous()); let g = grant(&t, vec![]); let stage = RedactStage::with_limits(generous());
    let run = |d: Dialect, b: Value| { let mut c = ctx(&t, &g, b, d); stage.on_request(&mut c).unwrap(); c.body };
    let arr = json!({"model":"m","input":[{"role":"user","content":[{"type":"input_text","text":format!("say {TRACER}")}]}]});
    assert_eq!(run(Dialect::Openai, arr)["input"][0]["content"][0]["text"], "say 🤬");
    assert_eq!(run(Dialect::Openai, json!({"model":"m","input":format!("{TRACER} x")}))["input"], "🤬 x");
    assert_eq!(run(Dialect::OpenaiChat, body(json!([{"role":"user","content":format!("say {TRACER}")}])))["messages"][0]["content"], "say 🤬");
    let same = json!({"model":"m","input":format!("{TRACER} x")});
    assert_eq!(run(Dialect::Anthropic, same.clone()), same);          // `input` is not an Anthropic user-text path
}

#[test] // review focus 10; §16.2 "go out unchanged in this request only (not cached), so the next request filters them"
fn stage_timeout_strings_uncached_then_filtered() {
    let t = setup("", generous()); let g = grant(&t, vec![]);
    let (zero, full) = (RedactStage::with_limits(RedactLimits { content_budget: Duration::ZERO, ..generous() }), RedactStage::with_limits(generous()));
    let big = format!("{TRACER} {}", "word ".repeat(14_000));
    let b = body(json!([{"role":"user","content":format!("first {TRACER}")},{"role":"user","content":big},{"role":"user","content":format!("third {TRACER}")}]));
    let mut c = ctx(&t, &g, b.clone(), Dialect::Anthropic);
    zero.on_request(&mut c).unwrap();
    assert_eq!((c.changed, c.redactions), (true, BTreeMap::from([("content_profane".to_string(), 1), ("content_timeout".to_string(), 1)])));
    assert_eq!((&c.body["messages"][1], &c.body["messages"][2]), (&b["messages"][1], &b["messages"][2]));   // both late strings unchanged
    assert_eq!(scans(&t, &g).1, 2);                                   // "first" and the big string were started; the third never was
    let mut c = ctx(&t, &g, b, Dialect::Anthropic);
    full.on_request(&mut c).unwrap();
    assert_eq!(scans(&t, &g).1, 4);                                   // the next request filters exactly the two that were not cached
    assert!(c.body["messages"][1]["content"].as_str().unwrap().starts_with("🤬 word word "));
    assert_eq!(c.body["messages"][2]["content"], "third 🤬");
    assert_eq!(c.redactions, BTreeMap::from([("content_profane".to_string(), 3)]));
}

#[test] // reading 2: a class header that is not UTF-8 is not admitted; an absent one is (§16.2 gate)
fn stage_non_utf8_class_is_not_admitted() {
    let t = setup("", generous()); let g = grant(&t, vec![]); let stage = RedactStage::with_limits(generous());
    let b = body(json!([{"role":"user","content":format!("say {TRACER}")}]));
    let mut c = ctx(&t, &g, b.clone(), Dialect::Anthropic);
    c.headers.insert("x-claude-code-request-class", http::HeaderValue::from_bytes(b"\xff").unwrap());
    stage.on_request(&mut c).unwrap();
    assert_eq!((c.body, c.changed), (b.clone(), false));
    let mut c = ctx(&t, &g, b, Dialect::Anthropic);
    stage.on_request(&mut c).unwrap();
    assert_eq!(c.body["messages"][0]["content"], "say 🤬");
}

#[test] // reading 4: one count per id, summed over strings (both passes)
fn stage_counts_sum_per_id_over_strings() {
    let t = setup("", generous()); let g = grant(&t, vec![]); let stage = RedactStage::with_limits(generous()); let key = key_sample();
    let b = body(json!([
        {"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":format!("a {key}")},
                                  {"type":"tool_result","tool_use_id":"t2","content":format!("b {key}")},
                                  {"type":"text","text":format!("one {TRACER}")}]},
        {"role":"user","content":format!("two {TRACER}")}]));
    let mut c = ctx(&t, &g, b, Dialect::Anthropic);
    stage.on_request(&mut c).unwrap();
    assert_eq!(c.redactions, BTreeMap::from([("anthropic_api_key".to_string(), 2), ("content_profane".to_string(), 2)]));
}

#[test] // §20 #33; C3. One proxy, five outcomes: 503 panic, 200 redact+content, 422 block, 503 timeout, 200 content timeout.
fn leak_nothing_written_contains_a_value() {
    let l = RedactLimits { secret: DeadlinePolicy { expire_after_scans: Some(10), ..generous().secret }, content_budget: Duration::ZERO,
                           panic_once: Some(RedactPass::Secret), extra_wait: Duration::ZERO };
    let t = setup("", l); let (key, blk) = (key_sample(), block_sample()); let g = grant(&t, vec![]);
    let small = body(json!([{"role":"user","content":format!("say {TRACER}")},{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":key.clone()}]}]));
    let many: Vec<Value> = (0..12).map(|i| json!({"role":"user","content":format!("s{i} {TRACER}")})).collect();   // 14 distinct strings > 10 scans
    let slow = body(json!([{"role":"user","content":format!("{TRACER} {}", "word ".repeat(14_000))}]));
    let runs = [small.clone(), small, body(json!([{"role":"user","content":blk.clone()}])), body(Value::Array(many)), slow];
    let mut seen: Vec<u8> = Vec::new();
    let codes: Vec<u16> = runs.iter().map(|b| { let r = post(&t, &g, M, b, &[]); for (k, v) in &r.1 { seen.extend_from_slice(k.as_str().as_bytes()); seen.extend_from_slice(v.as_bytes()); } seen.extend_from_slice(&r.2); r.0 }).collect();
    assert_eq!(codes, vec![503, 200, 422, 503, 200]);
    assert!(t.h.usage_written(5, Duration::from_secs(10)));
    // Every file of the state directory except the input config, plus every response header and body.
    let mut files: Vec<Vec<u8>> = read_all(t.d.path());
    files.push(seen);
    for needle in [key.as_bytes(), blk.as_bytes(), TRACER.as_bytes(), REPL.as_bytes()] {
        assert!(files.iter().all(|f| !has(f, needle)), "{}", String::from_utf8_lossy(&needle[..3]));   // never prints the value
    }
}

#[test] // §16.2 "the secret pass runs first regardless": the request-class gate is the content pass's only
fn secret_runs_for_every_request_class() {
    let t = setup("", generous()); let g = grant(&t, vec![]); let key = key_sample();
    let b = body(json!([{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":key.clone()}]}]));
    for (i, class) in ["subagent", "unknown-class", "main"].into_iter().enumerate() {
        assert_eq!(post(&t, &g, M, &b, &[("x-claude-code-request-class", class)]).0, 200);
        assert_eq!(up_json(&t, i)["messages"][0]["content"][0]["content"], "[REDACTED:anthropic_api_key]", "{class}");
        assert!(!has(&t.up.bodies()[i], key.as_bytes()));
    }
}

#[test] // §16.1, C5: a lock wait past the deadline refuses 503 (test hook `extra_wait`, no clock involved)
fn secret_lock_wait_counts_against_deadline() {
    let t = setup("", RedactLimits { extra_wait: Duration::from_secs(3600), ..generous() }); let g = grant(&t, vec![]);
    let r = post(&t, &g, M, &conv(&key_sample()), &[]);
    assert_eq!((st(&r), msg(&r).as_str()), ((503, "zygo_redaction_unavailable".to_string()), "redaction did not finish in time"));
    assert_eq!((t.up.bodies().len(), scans(&t, &g).0), (0, 1)); // every attempt scans one uncached string (§16.1 Bound note)
}

#[test] // §16.1 Bound note (N68): the lock wait comes off base plus per-MiB, not the base only
fn secret_lock_wait_reduces_the_whole_budget() {
    let secret = DeadlinePolicy { base: Duration::ZERO, per_mib: Duration::from_secs(1 << 30), expire_after_scans: None };
    let t = setup("", RedactLimits { secret, extra_wait: Duration::from_secs(1 << 40), ..generous() }); let g = grant(&t, vec![]);
    let r = post(&t, &g, M, &conv(&key_sample()), &[]);
    assert_eq!((st(&r), msg(&r).as_str()), ((503, "zygo_redaction_unavailable".to_string()), "redaction did not finish in time"));
    assert_eq!((t.up.bodies().len(), scans(&t, &g).0), (0, 1));
}

#[test] // §16.2: a lock wait past the budget fails open with `content_timeout` (secret pass off, so only the content pass waits)
fn content_lock_wait_counts_against_budget() {
    let l = RedactLimits { extra_wait: Duration::from_secs(3600), ..generous() };
    let t = setup("\n[features.redaction]\nsecrets = false\n", l); let g = grant(&t, vec![]);
    let raw = to_vec(&body(json!([{"role":"user","content":format!("{TRACER} {}", "word ".repeat(14_336))}])));   // 70 KiB
    let (s, h, _) = send(&t, "POST", M, &auth(&g), Some(raw.clone()));
    assert_eq!((s, h.get("x-zygo-redacted").is_none()), (200, true));
    assert_eq!(t.up.bodies()[0], raw);
    assert_eq!(row(&t, &g, 1).redactions, vec![count("content_timeout", 1)]);
}

#[test] // §16 intro: both passes run on `count_tokens`
fn content_count_tokens_is_filtered() {
    let t = setup("", generous()); let g = grant(&t, vec![]);
    let (s, h, _) = post(&t, &g, CT, &body(json!([{"role":"user","content":format!("say {TRACER}")}])), &[]);
    assert_eq!((s, h.get("x-zygo-redacted").map(|v| v.to_str().unwrap())), (200, Some("1")));
    assert_eq!(up_json(&t, 0)["messages"][0]["content"], "say 🤬");
}

#[test] // §16.1 note: the 422 lists every matched block id, ", "-joined; the row has one count per id
fn secret_two_block_patterns_both_listed() {
    let extra = "\n[[redaction.pattern]]\nid = \"tracer_second\"\npattern = \"TRACERTWO_[0-9]{6}\"\naction = \"block\"\n";
    let t = setup(extra, generous()); let g = grant(&t, vec![]);
    let (a, b) = (block_sample(), format!("TRACERTWO_{}", 654321));
    let r = post(&t, &g, M, &body(json!([{"role":"user","content":format!("{b} {a} {a}")},{"role":"user","content":b.clone()}])), &[]);
    assert_eq!((st(&r), msg(&r).as_str()), ((422, "zygo_redaction_block".to_string()), "blocked by redaction pattern: tracer_block, tracer_second"));
    assert!(!has(&r.2, a.as_bytes()) && !has(&r.2, b.as_bytes()));
    assert_eq!(row(&t, &g, 1).redactions, vec![count("tracer_block", 1), count("tracer_second", 1)]);
}

#[test] // N11 `cache_markers_opencode_only`: stage 4 changed the body and stage 6 found nothing: the changed body goes out
fn secret_keeps_an_earlier_stage_change() {
    let t = setup("", generous()); let g = grant_with(&t, Agent::OpenCode, vec![]);
    let b = json!({"model":"test-same","max_tokens":64,"system":[{"type":"text","text":"a"},{"type":"text","text":"b"}],
      "tools":[{"name":"t1","input_schema":{"type":"object"}},{"name":"t2","input_schema":{"type":"object"}}],"messages":[{"role":"user","content":"hi"}]});
    let mut want = b.clone();
    want["system"][1]["cache_control"] = json!({"type":"ephemeral","ttl":"1h"});
    want["tools"][1]["cache_control"] = json!({"type":"ephemeral","ttl":"1h"});
    assert_eq!(post(&t, &g, M, &b, &[]).0, 200);
    assert_eq!(t.up.bodies()[0], to_vec(&want));
}
