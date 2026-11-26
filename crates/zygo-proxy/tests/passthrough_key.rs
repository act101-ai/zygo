//! RS-14: on an untranslated pair (Messages → Anthropic, Responses → openai) the provider key never reaches the
//! client (K§6.4.4): every occurrence of the key in the body bytes is replaced with `[REDACTED]`, 2xx or not, SSE or
//! JSON, whatever the framing; an upstream header that holds the key is dropped; an encoded body is refused. Every
//! other byte passes. Mock upstream on 127.0.0.1.

#[path = "support/responses.rs"]
mod support;

use serde_json::{json, Value};
use support::{codex_request, Harness, MODEL};
use zygo_config::Dialect;
use zygo_core::Agent;
use zygo_proxy::tee::{COPY_MAX, HOLD_MAX};

const R: &str = "openai/v1/responses";
const M: &str = "anthropic/v1/messages";
const CT: &str = "anthropic/v1/messages/count_tokens";
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

/// A content type that also writes `extra` as more reply header lines: the mock writes the content type into the head
/// as given.
fn with_headers(ctype: &str, extra: &[(&str, &str)]) -> String {
    extra.iter().fold(ctype.to_string(), |s, (n, v)| format!("{s}\r\n{n}: {v}"))
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

/// A 2xx text stream from upstream `d` whose text delta is `text`.
fn text_stream(d: Dialect, text: &str) -> Vec<u8> {
    if d == Dialect::Anthropic {
        sse(&[
            message_start(),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":text}}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"message_stop"}),
        ])
    } else {
        sse(&[
            created(),
            json!({"type":"response.output_text.delta","sequence_number":1,"output_index":0,"content_index":0,"item_id":"msg_up","delta":text}),
        ])
    }
}

/// Posts to the untranslated pair toward `d` with the upstream reply `up` (status 200, `text/event-stream`); asserts
/// a 200 whose body is `up` with the key replaced and holds no key.
fn stream_is_redacted(d: Dialect, agent: Agent, up: impl Fn(&str) -> Vec<u8>) {
    let (h, path, body) = pair(d, agent);
    let k = h.provider_key().to_string();
    let up = up(&k);
    assert!(contains(&up, k.as_bytes()));
    h.reply_with(200, SSE, up.clone());
    let (s, hd, b) = h.post(path, &body);
    assert_eq!(s.as_u16(), 200, "{d:?} {agent:?}");
    assert!(!contains(&b, k.as_bytes()), "{d:?} {agent:?}: {}", String::from_utf8_lossy(&b));
    assert!(hd.get("content-length").is_none_or(|v| v.to_str().unwrap() == b.len().to_string()), "{d:?} {agent:?}");
    if agent == Agent::OpenCode {
        assert_eq!(String::from_utf8_lossy(&b), String::from_utf8_lossy(&redacted(&up, &k)), "{d:?}");
    }
}

/// K§6.4.4: a 401 JSON body with the key reaches the client with `[REDACTED]` in its place; status and `content-type`
/// kept; `content-length` of the new body.
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

/// K§6.4.4: a non-2xx SSE body is read whole too; `content-type` kept.
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

/// K§6.4.4: a non-2xx body over COPY_MAX is `502 zygo_upstream_too_large`; the key never reaches the client.
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

/// K§6.4.4: a count_tokens non-2xx body has the key replaced.
#[test]
fn count_tokens_non_2xx_has_no_key() {
    let h = Harness::start(Dialect::Anthropic, Agent::ClaudeCode, "");
    let k = h.provider_key().to_string();
    let up = serde_json::to_vec(
        &json!({"type":"error","error":{"type":"authentication_error","message":format!("invalid x-api-key: {k}")}}),
    )
    .unwrap();
    h.reply_with(401, JSON, up.clone());
    let (s, _, b) = h.post(CT, &json!({"model":MODEL,"messages":[{"role":"user","content":"hi"}]}));
    assert_eq!(s.as_u16(), 401);
    assert!(!contains(&b, k.as_bytes()));
    assert_eq!(&b[..], &redacted(&up, &k)[..]);
}

/// K§6.4.4: an Anthropic `error` event in a 2xx stream has the key replaced, with the window stage armed (Claude Code)
/// and not (OpenCode).
#[test]
fn messages_stream_error_event_has_no_key() {
    for agent in [Agent::ClaudeCode, Agent::OpenCode] {
        stream_is_redacted(Dialect::Anthropic, agent, |k| {
            sse(&[
                message_start(),
                json!({"type":"error","error":{"type":"overloaded_error","message":format!("upstream key {k} rejected")}}),
            ])
        });
    }
}

/// K§6.4.4: Responses `error` and `response.failed` events in a 2xx stream have the key replaced.
#[test]
fn responses_stream_error_and_failed_have_no_key() {
    for agent in [Agent::ClaudeCode, Agent::OpenCode] {
        stream_is_redacted(Dialect::Openai, agent, |k| {
            sse(&[
                created(),
                json!({"type":"error","sequence_number":1,"code":"invalid_api_key","message":format!("key {k} revoked"),"param":null}),
                failed(&format!("key {k} revoked")),
            ])
        });
    }
}

/// Round 1 F1: an unnamed event whose `data` is split in two lines, each whole JSON, the second without a `type`.
#[test]
fn split_data_in_an_unnamed_event_has_no_key() {
    for d in [Dialect::Anthropic, Dialect::Openai] {
        stream_is_redacted(d, Agent::OpenCode, |k| {
            let mut up = sse(&[first(d)]);
            up.extend_from_slice(format!("data: {{\"type\":\"error\"}}\ndata: {{\"message\":\"{k}\"}}\n\n").as_bytes());
            up
        });
    }
}

/// Round 1 F2: `event: error` after the event's `data` line.
#[test]
fn event_name_after_data_has_no_key() {
    for d in [Dialect::Anthropic, Dialect::Openai] {
        stream_is_redacted(d, Agent::OpenCode, |k| {
            let mut up = sse(&[first(d)]);
            up.extend_from_slice(
                format!("data: {{\"error\":{{\"type\":\"overloaded_error\",\"message\":\"{k}\"}}}}\nevent: error\n\n").as_bytes(),
            );
            up
        });
    }
}

/// Round 1 F3: an error event with bare-CR line endings.
#[test]
fn bare_cr_lines_have_no_key() {
    for d in [Dialect::Anthropic, Dialect::Openai] {
        stream_is_redacted(d, Agent::OpenCode, |k| {
            let mut up = sse(&[first(d)]);
            up.extend_from_slice(
                format!("event: error\rdata: {{\"type\":\"error\",\"error\":{{\"message\":\"{k}\"}}}}\r\r").as_bytes(),
            );
            up
        });
    }
}

/// Round 1 F10: a stream with CRLF line endings that holds the key, in an error event and a text delta.
#[test]
fn crlf_lines_have_no_key() {
    for d in [Dialect::Anthropic, Dialect::Openai] {
        stream_is_redacted(d, Agent::OpenCode, |k| {
            let mut up = text_stream(d, &format!("echo {k}"));
            up.extend_from_slice(
                format!("event: error\ndata: {{\"type\":\"error\",\"error\":{{\"message\":\"{k}\"}}}}\n\n").as_bytes(),
            );
            String::from_utf8(up).unwrap().replace('\n', "\r\n").into_bytes()
        });
    }
}

/// Round 1 F10: the key in the last bytes of the body, with no final newline, is replaced (the replacement's flush
/// at the body end).
#[test]
fn key_at_the_end_without_final_newline_is_replaced() {
    for d in [Dialect::Anthropic, Dialect::Openai] {
        stream_is_redacted(d, Agent::OpenCode, |k| {
            let mut up = sse(&[first(d)]);
            up.extend_from_slice(format!("data: {k}").as_bytes());
            up
        });
    }
}

/// K§6.4.4: the key in a 2xx success text delta is replaced (round 1 F8).
#[test]
fn success_text_delta_has_no_key() {
    for d in [Dialect::Anthropic, Dialect::Openai] {
        for agent in [Agent::ClaudeCode, Agent::OpenCode] {
            stream_is_redacted(d, agent, |k| text_stream(d, &format!("echo {k}")));
        }
    }
}

/// K§6.4.4: a 2xx stream that does not hold the key passes byte for byte.
#[test]
fn success_stream_without_key_passes_byte_for_byte() {
    for d in [Dialect::Anthropic, Dialect::Openai] {
        let (h, path, body) = pair(d, Agent::OpenCode);
        let up = text_stream(d, "echo test-key-k-rs partial");
        h.reply_with(200, SSE, up.clone());
        let (s, _, b) = h.post(path, &body);
        assert_eq!(s.as_u16(), 200, "{d:?}");
        assert_eq!(&b[..], &up[..], "{d:?}");
    }
}

/// K§6.4.4: a 2xx JSON body has the key replaced; `content-length`, when sent, is the new body's.
#[test]
fn success_json_body_has_no_key() {
    for d in [Dialect::Anthropic, Dialect::Openai] {
        let (h, path, mut body) = pair(d, Agent::OpenCode);
        body["stream"] = json!(false);
        let k = h.provider_key().to_string();
        let up = if d == Dialect::Anthropic {
            json!({"id":"msg_up","type":"message","role":"assistant","model":"up-rs","content":[{"type":"text","text":format!("echo {k}")}],
                   "stop_reason":"end_turn","usage":{"input_tokens":7,"output_tokens":3}})
        } else {
            json!({"id":"resp_up","object":"response","model":"up-rs","status":"completed",
                   "output":[{"type":"message","id":"msg_up","role":"assistant","content":[{"type":"output_text","text":format!("echo {k}")}]}],
                   "usage":{"input_tokens":7,"output_tokens":3,"total_tokens":10}})
        };
        let up = serde_json::to_vec(&up).unwrap();
        h.reply_with(200, JSON, up.clone());
        let (s, hd, b) = h.post(path, &body);
        assert_eq!(s.as_u16(), 200, "{d:?}");
        assert!(!contains(&b, k.as_bytes()), "{d:?}");
        assert_eq!(&b[..], &redacted(&up, &k)[..], "{d:?}");
        assert!(hd.get("content-length").is_none_or(|v| v.to_str().unwrap() == b.len().to_string()), "{d:?}");
    }
}

/// K§6.4.4: an error event's `data` line over HOLD_MAX has the key replaced; the usage row is still written (round 1
/// F9: no cut).
#[test]
fn error_line_over_hold_max_has_no_key() {
    for d in [Dialect::Anthropic, Dialect::Openai] {
        let (h, path, body) = pair(d, Agent::OpenCode);
        let k = h.provider_key().to_string();
        let message = format!("{} {k}", "x".repeat(HOLD_MAX));
        let err = if d == Dialect::Anthropic {
            json!({"type":"error","error":{"type":"api_error","message":message}})
        } else {
            failed(&message)
        };
        let up = sse(&[first(d), err, json!({"type":"message_stop"})]);
        h.reply_with(200, SSE, up.clone());
        let (s, _, b) = h.post(path, &body);
        assert_eq!(s.as_u16(), 200, "{d:?}");
        assert!(!contains(&b, k.as_bytes()), "{d:?}");
        assert!(b[..] == redacted(&up, &k)[..], "{d:?}");
        assert_eq!(h.usage_rows().len(), 1, "{d:?}");
    }
}

/// K§6.4.4: a success event's `data` line over HOLD_MAX passes: unchanged without the key, with the key replaced.
#[test]
fn success_line_over_hold_max_passes() {
    for with_key in [false, true] {
        let (h, path, body) = pair(Dialect::Anthropic, Agent::OpenCode);
        let k = h.provider_key().to_string();
        let text = format!("{} {}", "x".repeat(HOLD_MAX), if with_key { k.as_str() } else { "end" });
        let up = text_stream(Dialect::Anthropic, &text);
        h.reply_with(200, SSE, up.clone());
        let (s, _, b) = h.post(path, &body);
        assert_eq!(s.as_u16(), 200);
        assert!(b[..] == redacted(&up, &k)[..], "{with_key}");
    }
}

/// K§6.4.4 (round 1 F4): an upstream response header that holds the key is dropped, translated or not, 2xx or not;
/// the other headers pass; the code `key_header_dropped` is recorded.
#[test]
fn header_with_key_is_dropped() {
    // (upstream, client path): untranslated, then translated.
    for (d, path) in [(Dialect::Anthropic, M), (Dialect::Openai, R), (Dialect::Openai, M)] {
        for status in [200u16, 401] {
            let h = Harness::start(d, Agent::OpenCode, "");
            let k = h.provider_key().to_string();
            let body = if path == M {
                json!({"model":MODEL,"max_tokens":64,"stream":true,"messages":[{"role":"user","content":"hi"}]})
            } else {
                codex_request("turn-with-reasoning")
            };
            let ctype = with_headers(SSE, &[("x-echo-key", &format!("Bearer {k}")), ("x-other", "kept")]);
            let up = if status == 200 {
                text_stream(d, "hello")
            } else {
                br#"{"error":{"type":"invalid_request_error","message":"no"}}"#.to_vec()
            };
            h.reply_with(status, &ctype, up);
            let (s, hd, b) = h.post(path, &body);
            let tag = format!("{d:?} {path} {status}");
            assert_eq!(s.as_u16(), status, "{tag}: {}", String::from_utf8_lossy(&b));
            assert!(hd.get("x-echo-key").is_none(), "{tag}");
            assert_eq!(hd.get("x-other").map(|v| v.to_str().unwrap()), Some("kept"), "{tag}");
            assert!(hd.iter().all(|(_, v)| !contains(v.as_bytes(), k.as_bytes())), "{tag}");
            let adj = hd.get("x-zygo-adjustments").map(|v| v.to_str().unwrap().to_string()).unwrap_or_default();
            assert!(adj.split(',').any(|a| a == "key_header_dropped"), "{tag}: {adj}");
        }
    }
}

/// K§6.4.4 (round 1 F5): an upstream body with a `content-encoding` other than identity is
/// `502 zygo_upstream_encoded`, 2xx or not; identity passes.
#[test]
fn encoded_body_is_502() {
    for d in [Dialect::Anthropic, Dialect::Openai] {
        for status in [200u16, 401] {
            let (h, path, body) = pair(d, Agent::OpenCode);
            let k = h.provider_key().to_string();
            let ctype = with_headers(SSE, &[("content-encoding", "gzip")]);
            h.reply_with(status, &ctype, text_stream(d, &format!("echo {k}")));
            let (s, _, b) = h.post(path, &body);
            assert_eq!(s.as_u16(), 502, "{d:?} {status}");
            assert!(contains(&b, b"zygo_upstream_encoded"), "{d:?} {status}");
            assert!(!contains(&b, k.as_bytes()), "{d:?} {status}");
        }
        let (h, path, body) = pair(d, Agent::OpenCode);
        let up = text_stream(d, "hello");
        h.reply_with(200, &with_headers(SSE, &[("content-encoding", "identity")]), up.clone());
        let (s, _, b) = h.post(path, &body);
        assert_eq!(s.as_u16(), 200, "{d:?}");
        assert_eq!(&b[..], &up[..], "{d:?}");
    }
}
