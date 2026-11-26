//! RS-3: `POST /openai/v1/responses` (K§6.4.1-6.4.4, K§6.7, K§11 items 12, 14, 20): identity and the OpenAI error
//! shape, the field lists at the route, the request side toward Anthropic and chat upstreams, upstream errors before
//! the first byte, redaction and the usage row. Mock upstream on 127.0.0.1; no agent binary, no network.

#[path = "support/responses.rs"]
mod support;

use std::collections::BTreeSet;

use serde_json::{json, Value};
use support::{codex_request, Harness, MAX_OUTPUT, MODEL, PROVIDER, UPSTREAM_MODEL};
use zygo_config::Dialect::{Anthropic as A, OpenaiChat as C};
use zygo_core::Agent;
use zygo_proxy::translate::responses_client::{IGNORED, NO_STATE, REFUSED};

const R: &str = "openai/v1/responses";
const FIXTURES: [&str; 4] = ["first-turn", "after-function-call-output", "after-custom-tool-call-output", "turn-with-reasoning"];

fn start(d: zygo_config::Dialect) -> Harness {
    Harness::start(d, Agent::ClaudeCode, "")
}

/// An OpenAI error body: an object with `error` and no top-level `type`; returns `error`.
fn oai_err(b: &[u8]) -> Value {
    let v: Value = serde_json::from_slice(b).unwrap_or_else(|_| panic!("not JSON: {}", String::from_utf8_lossy(b)));
    let o = v.as_object().expect("an object");
    assert!(o.contains_key("error"), "{v}");
    assert!(!o.contains_key("type"), "{v}");
    v["error"].clone()
}

fn hv(h: &http::HeaderMap, n: &str) -> String {
    h.get(n).map(|v| v.to_str().unwrap().to_string()).unwrap_or_default()
}

fn no_sse(b: &[u8]) {
    let has = |n: &[u8]| b.windows(n.len()).any(|w| w == n);
    assert!(!has(b"event:") && !has(b"data:"), "{}", String::from_utf8_lossy(b));
}

/// No `facts_missing` and no `blocks_dropped:*` in `x-zygo-adjustments`.
fn clean_adjustments(h: &http::HeaderMap) {
    let a = hv(h, "x-zygo-adjustments");
    for item in a.split(',').filter(|s| !s.is_empty()) {
        assert!(item != "facts_missing" && !item.starts_with("blocks_dropped:"), "{a}");
    }
}

#[test]
fn identity_missing_session_header_openai_shape() {
    let h = start(A);
    let bearer = format!("Bearer {}", h.token());
    let (s, _, b) = h.post_as(R, &[("authorization", &bearer)], &codex_request("first-turn"));
    assert_eq!(s, 401);
    assert_eq!(oai_err(&b)["type"], "zygo_token_invalid");
    assert!(h.upstream_bodies().is_empty());
}

#[test]
fn identity_wrong_bearer_or_session_openai_shape() {
    let h = start(A);
    let body = codex_request("first-turn");
    let (s, _, b) = h.post_as(R, &[("authorization", "Bearer wrong-token"), ("x-zygo-session", h.session_id())], &body);
    assert_eq!((s.as_u16(), oai_err(&b)["type"].clone()), (401, json!("zygo_token_invalid")));
    let bearer = format!("Bearer {}", h.token());
    let (s, _, b) = h.post_as(R, &[("authorization", &bearer), ("x-zygo-session", "no-such-session")], &body);
    assert_eq!((s.as_u16(), oai_err(&b)["type"].clone()), (401, json!("zygo_session_unknown")));
    assert!(h.upstream_bodies().is_empty());
}

#[test]
fn unknown_field_400_before_upstream() {
    let h = start(A);
    let mut body = codex_request("first-turn");
    body["rs3_field"] = json!(1);
    let (s, _, b) = h.post(R, &body);
    assert_eq!(s, 400);
    let e = oai_err(&b);
    assert_eq!((e["type"].clone(), e["message"].clone()), (json!("zygo_bad_request"), json!("unknown field: rs3_field")));
    assert!(h.upstream_bodies().is_empty());
}

#[test]
fn refused_fields_400_before_upstream() {
    let h = start(A);
    for f in REFUSED {
        let mut body = codex_request("first-turn");
        body[f] = json!("x");
        let (s, _, b) = h.post(R, &body);
        assert_eq!(s, 400, "{f}");
        let e = oai_err(&b);
        assert_eq!((e["type"].clone(), e["message"].clone()), (json!("zygo_bad_request"), json!(format!("{f}: {NO_STATE}"))));
    }
    assert!(h.upstream_bodies().is_empty());
}

#[test]
fn proxy_error_after_identity_openai_shape() {
    let h = start(A);
    let mut body = codex_request("first-turn");
    body["model"] = json!("no-such-model");
    let (s, _, b) = h.post(R, &body);
    assert_eq!((s.as_u16(), oai_err(&b)["type"].clone()), (400, json!("zygo_unknown_model")));
    assert!(h.upstream_bodies().is_empty());
}

#[test]
fn captured_requests_reach_anthropic_as_messages() {
    let h = start(A);
    for f in FIXTURES {
        let (s, rh, _) = h.post(R, &codex_request(f));
        assert_eq!(s, 400, "{f}: the mock's status");
        assert_eq!(h.upstream_paths().last().map(String::as_str), Some("/v1/messages"), "{f}");
        let v = h.upstream_bodies().pop().unwrap();
        assert_eq!(v["model"], UPSTREAM_MODEL, "{f}");
        assert_eq!(v["max_tokens"], MAX_OUTPUT, "{f}");
        let m = v["messages"].as_array().unwrap_or_else(|| panic!("{f}: messages {v}"));
        assert!(!m.is_empty() && m[0]["role"] == "user", "{f}: {v}");
        assert!(v.get("input").is_none(), "{f}");
        for k in IGNORED {
            assert!(v.get(k).is_none(), "{f}: {k}");
        }
        let uh = h.upstream_headers().pop().unwrap();
        assert_eq!(hv(&uh, "anthropic-version"), "2023-06-01", "{f}");
        assert!(uh.get("authorization").is_none(), "{f}");
        assert!(!uh.keys().any(|k| k.as_str().starts_with("x-zygo-")), "{f}");
        clean_adjustments(&rh);
    }
}

#[test]
fn captured_requests_reach_chat_as_chat() {
    let h = start(C);
    for f in FIXTURES {
        let (_, rh, _) = h.post(R, &codex_request(f));
        assert_eq!(h.upstream_paths().last().map(String::as_str), Some("/v1/chat/completions"), "{f}");
        let v = h.upstream_bodies().pop().unwrap();
        assert_eq!(v["model"], UPSTREAM_MODEL, "{f}");
        assert!(v["messages"].as_array().is_some_and(|m| !m.is_empty()), "{f}: {v}");
        assert_eq!(v["stream"], true, "{f}");
        assert!(v.get("input").is_none() && v.get("system").is_none(), "{f}");
        for k in IGNORED {
            assert!(v.get(k).is_none(), "{f}: {k}");
        }
        let uh = h.upstream_headers().pop().unwrap();
        let auth = hv(&uh, "authorization");
        assert!(!auth.is_empty() && auth != format!("Bearer {}", h.token()), "{f}");
        clean_adjustments(&rh);
    }
}

#[test]
fn upstream_529_before_first_byte_streaming() {
    let h = start(A);
    h.reply_with(529, "application/json", br#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#.to_vec());
    let body = codex_request("first-turn");
    assert_eq!(body["stream"], true);
    let (s, rh, b) = h.post(R, &body);
    assert_eq!(s.as_u16(), 529);
    assert!(hv(&rh, "content-type").starts_with("application/json"), "{rh:?}");
    assert_eq!(oai_err(&b), json!({"type":"overloaded_error","message":"Overloaded"}));
    no_sse(&b);
}

/// The PC14 shape: the keys, type, param and code of `context-400.json`'s error, and `message`.
fn pc14(b: &[u8], message: &str) {
    let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/codex/errors/context-400.json");
    let want: Value = serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap();
    let want = &want["error"];
    let e = oai_err(b);
    let keys = |v: &Value| v.as_object().unwrap().keys().cloned().collect::<BTreeSet<_>>();
    assert_eq!(keys(&e), keys(want), "{e}");
    for k in ["type", "param", "code"] {
        assert_eq!(e[k], want[k], "{k}");
    }
    assert_eq!(e["message"], message);
    no_sse(b);
}

#[test]
fn over_long_prompt_anthropic_pc14() {
    let h = start(A);
    h.reply_with(
        400,
        "application/json",
        br#"{"type":"error","error":{"type":"invalid_request_error","message":"prompt is too long: 250001 tokens > 200000 maximum"}}"#.to_vec(),
    );
    let (s, _, b) = h.post(R, &codex_request("first-turn"));
    assert_eq!(s, 400);
    pc14(&b, "prompt is too long: 250001 tokens > 200000 maximum");
}

#[test]
fn over_long_prompt_chat_pc14() {
    let h = start(C);
    h.reply_with(
        400,
        "application/json",
        json!({"error":{"message":"This model's maximum context length is 128000 tokens. However, your messages resulted in 130000 tokens.",
            "type":"invalid_request_error","param":"messages","code":"context_length_exceeded"}})
        .to_string()
        .into_bytes(),
    );
    let (s, _, b) = h.post(R, &codex_request("first-turn"));
    assert_eq!(s, 400);
    pc14(&b, "prompt is too long: 130000 tokens > 128000 maximum");
}

#[test]
fn redaction_acts_on_input() {
    let h = Harness::start(A, Agent::ClaudeCode, "[redaction.content]\ndeny = [\"plimsoq\"]\n");
    // Built at run time (no credential-shaped literal).
    let k = ["sk-", "ant-api03-", &"Ab1_".repeat(24)].concat();
    let mut body = codex_request("first-turn");
    body["input"].as_array_mut().unwrap().push(json!({"type":"message","role":"user",
        "content":[{"type":"input_text","text": format!("key {k} and plimsoq")}]}));
    let (_, rh, _) = h.post(R, &body);
    let sent = h.upstream_bodies().pop().expect("one upstream request").to_string();
    assert!(!sent.contains(&k), "the secret reached the upstream");
    assert!(!sent.contains("plimsoq"), "the denied word reached the upstream");
    assert_eq!(hv(&rh, "x-zygo-redacted"), "1");
}

#[test]
fn attribution_row_for_responses_route() {
    let h = start(A);
    let mut body = codex_request("first-turn");
    body["model"] = json!("code");
    let (s, rh, _) = h.post(R, &body);
    assert_eq!(s, 400);
    assert_eq!(h.upstream_bodies().pop().unwrap()["model"], UPSTREAM_MODEL);
    assert_eq!((hv(&rh, "x-zygo-role"), hv(&rh, "x-zygo-model")), ("code".to_string(), MODEL.to_string()));
    let rows = h.usage_rows();
    assert_eq!(rows.len(), 1);
    let r = &rows[0];
    assert_eq!(r.route, "/openai/v1/responses");
    assert_eq!(r.agent, "claude-code");
    assert_eq!(r.role_served.as_deref(), Some("code"));
    assert_eq!((r.model.as_str(), r.provider.as_str(), r.status), (MODEL, PROVIDER, 400));
}
