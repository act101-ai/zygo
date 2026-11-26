//! RS-14: on an untranslated pair (Messages → Anthropic, Responses → openai) the provider key never reaches the
//! client (K§6.4.4): a non-2xx body and each in-stream error event have the key replaced with `[REDACTED]`; an error
//! `data` line over HOLD_MAX ends the client body before it; every other byte passes. Mock upstream on 127.0.0.1.

#[path = "support/responses.rs"]
mod support;

use serde_json::{json, Value};
use support::{codex_request, Harness, MODEL};
use zygo_config::Dialect;
use zygo_core::Agent;
use zygo_proxy::tee::{COPY_MAX, HOLD_MAX};

const R: &str = "openai/v1/responses";
const M: &str = "anthropic/v1/messages";
const SSE: &str = "text/event-stream";
const JSON: &str = "application/json";
const RED: &[u8] = b"[REDACTED]";

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// `event: <type>\ndata: <compact json>\n\n` per event.
fn sse(evs: &[Value]) -> Vec<u8> {
    evs.iter().flat_map(|e| format!("event: {}\ndata: {e}\n\n", e["type"].as_str().unwrap()).into_bytes()).collect()
}

/// `b` with each occurrence of `key` replaced by `[REDACTED]`.
fn redacted(b: &[u8], key: &str) -> Vec<u8> {
    String::from_utf8(b.to_vec()).unwrap().replace(key, "[REDACTED]").into_bytes()
}

/// (harness, client path, client body) of the untranslated pair toward upstream `d`.
fn pair(d: Dialect, agent: Agent) -> (Harness, &'static str, Value) {
    let h = Harness::start(d, agent, "");
    match d {
        Dialect::Anthropic => {
            (h, M, json!({"model":MODEL,"max_tokens":64,"stream":true,"messages":[{"role":"user","content":"hi"}]}))
        }
        _ => (h, R, codex_request("turn-with-reasoning")),
    }
}

fn message_start() -> Value {
    json!({"type":"message_start","message":{"id":"msg_up","type":"message","role":"assistant","model":"up-rs","content":[],
           "stop_reason":null,"usage":{"input_tokens":7,"output_tokens":1}}})
}

fn created() -> Value {
    json!({"type":"response.created","sequence_number":0,
           "response":{"id":"resp_up","object":"response","model":"up-rs","status":"in_progress","output":[],"usage":null}})
}

/// The first event of a 2xx stream from upstream `d`.
fn first(d: Dialect) -> Value {
    if d == Dialect::Anthropic {
        message_start()
    } else {
        created()
    }
}

fn failed(message: &str) -> Value {
    json!({"type":"response.failed","sequence_number":2,"response":{"id":"resp_up","object":"response","model":"up-rs",
           "status":"failed","output":[],"error":{"code":"server_error","message":message},"usage":null}})
}

/// K§6.4.4 (1): a 401 JSON body with the key reaches the client with `[REDACTED]` in its place; status and
/// `content-type` kept; `content-length` of the new body.
#[test]
fn non_2xx_body_has_no_key() {
    for d in [Dialect::Anthropic, Dialect::Openai] {
        let (h, path, body) = pair(d, Agent::ClaudeCode);
        let k = h.provider_key().to_string();
        let up = if d == Dialect::Anthropic {
            json!({"type":"error","error":{"type":"authentication_error","message":format!("invalid x-api-key: {k}")}})
        } else {
            json!({"error":{"type":"invalid_request_error","message":format!("Incorrect API key provided: {k}"),"param":null,"code":"invalid_api_key"}})
        };
        let up = serde_json::to_vec(&up).unwrap();
        h.reply_with(401, JSON, up.clone());
        let (s, hd, b) = h.post(path, &body);
        assert_eq!(s.as_u16(), 401, "{d:?}");
        assert_eq!(hd["content-type"].to_str().unwrap(), JSON, "{d:?}");
        assert_eq!(hd["content-length"].to_str().unwrap(), b.len().to_string(), "{d:?}");
        assert!(!contains(&b, k.as_bytes()), "{d:?}");
        assert!(contains(&b, RED), "{d:?}");
        assert_eq!(&b[..], &redacted(&up, &k)[..], "{d:?}");
    }
}

/// K§6.4.4 (1): a non-2xx SSE body is read whole too; `content-type` kept.
#[test]
fn non_2xx_sse_body_has_no_key() {
    let (h, path, body) = pair(Dialect::Openai, Agent::ClaudeCode);
    let k = h.provider_key().to_string();
    let up = sse(&[json!({"type":"error","sequence_number":0,"code":"invalid_api_key","message":format!("bad key {k}"),"param":null})]);
    h.reply_with(400, SSE, up.clone());
    let (s, hd, b) = h.post(path, &body);
    assert_eq!(s.as_u16(), 400);
    assert!(hd["content-type"].to_str().unwrap().starts_with(SSE));
    assert_eq!(hd["content-length"].to_str().unwrap(), b.len().to_string());
    assert!(!contains(&b, k.as_bytes()));
    assert_eq!(&b[..], &redacted(&up, &k)[..]);
}

/// K§6.4.4 (1): a non-2xx body over COPY_MAX is `502 zygo_upstream_too_large`; the key never reaches the client.
#[test]
fn non_2xx_over_copy_max_is_502() {
    for d in [Dialect::Anthropic, Dialect::Openai] {
        let (h, path, body) = pair(d, Agent::ClaudeCode);
        let k = h.provider_key().to_string();
        let mut up = format!("{k} ").into_bytes();
        up.resize(COPY_MAX + 1, b'x');
        h.reply_with(401, JSON, up);
        let (s, _, b) = h.post(path, &body);
        assert_eq!(s.as_u16(), 502, "{d:?}");
        assert!(contains(&b, b"zygo_upstream_too_large"), "{d:?}");
        assert!(!contains(&b, k.as_bytes()), "{d:?}");
    }
}

/// K§6.4.4 (2): an Anthropic `error` event in a 2xx stream has the key replaced, with the window stage armed
/// (Claude Code) and not (OpenCode); without the window stage every other byte is unchanged.
#[test]
fn messages_stream_error_event_has_no_key() {
    for agent in [Agent::ClaudeCode, Agent::OpenCode] {
        let (h, path, body) = pair(Dialect::Anthropic, agent);
        let k = h.provider_key().to_string();
        let err = json!({"type":"error","error":{"type":"overloaded_error","message":format!("upstream key {k} rejected")}});
        let up = sse(&[message_start(), err.clone()]);
        h.reply_with(200, SSE, up.clone());
        let (s, _, b) = h.post(path, &body);
        assert_eq!(s.as_u16(), 200, "{agent:?}");
        assert!(!contains(&b, k.as_bytes()), "{agent:?}");
        assert!(contains(&b, &redacted(&sse(&[err]), &k)), "{agent:?}");
        if agent == Agent::OpenCode {
            assert_eq!(&b[..], &redacted(&up, &k)[..]);
        }
    }
}

/// K§6.4.4 (2): Responses `error` and `response.failed` events in a 2xx stream have the key replaced.
#[test]
fn responses_stream_error_and_failed_have_no_key() {
    for agent in [Agent::ClaudeCode, Agent::OpenCode] {
        let (h, path, body) = pair(Dialect::Openai, agent);
        let k = h.provider_key().to_string();
        let err = json!({"type":"error","sequence_number":1,"code":"invalid_api_key","message":format!("key {k} revoked"),"param":null});
        let fail = failed(&format!("key {k} revoked"));
        let up = sse(&[created(), err.clone(), fail.clone()]);
        h.reply_with(200, SSE, up.clone());
        let (s, _, b) = h.post(path, &body);
        assert_eq!(s.as_u16(), 200, "{agent:?}");
        assert!(!contains(&b, k.as_bytes()), "{agent:?}");
        assert!(contains(&b, &redacted(&sse(&[err]), &k)), "{agent:?}");
        assert!(contains(&b, &redacted(&sse(&[fail]), &k)), "{agent:?}");
        if agent == Agent::OpenCode {
            assert_eq!(&b[..], &redacted(&up, &k)[..]);
        }
    }
}

/// K§6.4.4 (3): a 2xx stream with no error event passes byte for byte, also when a text event holds the key.
#[test]
fn success_stream_passes_byte_for_byte() {
    for d in [Dialect::Anthropic, Dialect::Openai] {
        let (h, path, body) = pair(d, Agent::OpenCode);
        let k = h.provider_key().to_string();
        let up = if d == Dialect::Anthropic {
            sse(&[
                message_start(),
                json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
                json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":format!("echo {k}")}}),
                json!({"type":"content_block_stop","index":0}),
                json!({"type":"message_stop"}),
            ])
        } else {
            sse(&[
                created(),
                json!({"type":"response.output_text.delta","sequence_number":1,"output_index":0,"content_index":0,"item_id":"msg_up","delta":format!("echo {k}")}),
            ])
        };
        h.reply_with(200, SSE, up.clone());
        let (s, _, b) = h.post(path, &body);
        assert_eq!(s.as_u16(), 200, "{d:?}");
        assert_eq!(&b[..], &up[..], "{d:?}");
    }
}

/// K§6.4.4 (2): an error event's `data` line over HOLD_MAX does not reach the client: the body ends before it, and
/// the usage row is still written.
#[test]
fn error_line_over_hold_max_ends_the_body() {
    for d in [Dialect::Anthropic, Dialect::Openai] {
        let (h, path, body) = pair(d, Agent::OpenCode);
        let k = h.provider_key().to_string();
        let message = format!("{} {k}", "x".repeat(HOLD_MAX));
        let (err, name) = if d == Dialect::Anthropic {
            (json!({"type":"error","error":{"type":"api_error","message":message}}), "error")
        } else {
            (failed(&message), "response.failed")
        };
        let head = sse(&[first(d)]);
        let mut up = head.clone();
        up.extend_from_slice(&sse(&[err, json!({"type":"message_stop"})]));
        h.reply_with(200, SSE, up);
        let (s, _, b) = h.post(path, &body);
        assert_eq!(s.as_u16(), 200, "{d:?}");
        let mut want = head;
        want.extend_from_slice(format!("event: {name}\n").as_bytes());
        assert_eq!(&b[..], &want[..], "{d:?}");
        assert_eq!(h.usage_rows().len(), 1, "{d:?}");
    }
}

/// K§6.4.4 (3): a success event's `data` line over HOLD_MAX passes unchanged.
#[test]
fn success_line_over_hold_max_passes() {
    let (h, path, body) = pair(Dialect::Anthropic, Agent::OpenCode);
    let k = h.provider_key().to_string();
    let text = format!("{} {k}", "x".repeat(HOLD_MAX));
    let up = sse(&[
        message_start(),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":text}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"message_stop"}),
    ]);
    h.reply_with(200, SSE, up.clone());
    let (s, _, b) = h.post(path, &body);
    assert_eq!(s.as_u16(), 200);
    assert_eq!(&b[..], &up[..]);
}
