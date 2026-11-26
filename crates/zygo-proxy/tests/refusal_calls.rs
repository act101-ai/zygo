//! RS-16: a Messages client (Claude Code) never gets the `zygo_calls_dropped` key, and a call open at a refusal or
//! `content_filter` is dropped, not sent with cut arguments, with adjustment `refusal_call_dropped:<n>` on the usage
//! row (K§6.4.3 stream obligations 5 and 9). The upstream is a mock on 127.0.0.1.

#[path = "support/responses.rs"]
mod support;

use serde_json::{json, Value};
use support::{Harness, MODEL};
use zygo_config::Dialect::{self, Openai as O, OpenaiChat as C};
use zygo_core::Agent;
use zygo_proxy::tee::HOLD_MAX;

const M: &str = "anthropic/v1/messages";
const SSE: &str = "text/event-stream";
const JSON: &str = "application/json";
/// Tool arguments cut by the filter.
const CUT: &str = "{\"cmd\":";
const KEY: &[u8] = b"zygo_calls_dropped";

fn start(d: Dialect) -> Harness {
    Harness::start(d, Agent::ClaudeCode, "")
}

fn client_body(stream: bool) -> Value {
    json!({"model":MODEL,"max_tokens":64,"stream":stream,"messages":[{"role":"user","content":"hi"}]})
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// `event: <type>\ndata: <compact json>\n\n` per event (a Responses stream).
fn sse(evs: &[Value]) -> Vec<u8> {
    evs.iter().flat_map(|e| format!("event: {}\ndata: {e}\n\n", e["type"].as_str().unwrap()).into_bytes()).collect()
}

/// `data: <json>\n\n` per chunk, then `data: [DONE]\n\n`.
fn chat_sse(chunks: &[Value]) -> Vec<u8> {
    let mut out: Vec<u8> = chunks.iter().flat_map(|c| format!("data: {c}\n\n").into_bytes()).collect();
    out.extend_from_slice(b"data: [DONE]\n\n");
    out
}

/// The `(event, data)` pairs of an SSE body: per blank-line block, the `event:` name and the joined `data:` JSON.
fn events(b: &[u8]) -> Vec<(String, Value)> {
    String::from_utf8_lossy(b)
        .split("\n\n")
        .filter(|blk| !blk.trim().is_empty())
        .map(|blk| {
            let (mut name, mut data) = (String::new(), String::new());
            for line in blk.lines() {
                if let Some(n) = line.strip_prefix("event:") {
                    name = n.trim().to_string();
                } else if let Some(d) = line.strip_prefix("data:") {
                    data.push_str(d.trim_start());
                }
            }
            (name, serde_json::from_str(&data).unwrap_or_else(|_| panic!("data not JSON: {data}")))
        })
        .collect()
}

fn chunk(delta: Value, finish: Value) -> Value {
    json!({"id":"chatcmpl-1","object":"chat.completion.chunk","model":"up-rs","choices":[{"index":0,"delta":delta,"finish_reason":finish}]})
}

fn call() -> Value {
    json!({"index":0,"id":"call_1","type":"function","function":{"name":"exec_command","arguments":CUT}})
}

fn usage_raw() -> Value {
    json!({"prompt_tokens":120,"completion_tokens":7,"total_tokens":127})
}

/// The last usage row holds `refusal_call_dropped:1`.
fn assert_row_code(h: &Harness) {
    let rows = h.usage_rows();
    let a = &rows.last().expect("a usage row").adjustments;
    assert!(a.contains(&"refusal_call_dropped:1".to_owned()), "{a:?}");
}

/// A Messages SSE body: no key, no tool_use block, no call id, stop_reason refusal.
fn assert_stream_dropped(b: &[u8]) {
    let text = String::from_utf8_lossy(b);
    assert!(!contains(b, KEY), "{text}");
    assert!(!contains(b, b"tool_use") && !contains(b, b"input_json_delta") && !contains(b, b"call_1"), "{text}");
    let evs = events(b);
    let md = evs.iter().find(|(n, _)| n == "message_delta").expect("a message_delta");
    assert_eq!(md.1["delta"]["stop_reason"], "refusal", "{text}");
    assert_eq!(evs.last().map(|(n, _)| n.as_str()), Some("message_stop"), "{text}");
}

/// A Messages JSON body: no key, stop_reason refusal, only the text block "hel".
fn assert_json_dropped(b: &[u8]) {
    let text = String::from_utf8_lossy(b);
    assert!(!contains(b, KEY), "{text}");
    assert!(!contains(b, b"tool_use") && !contains(b, b"call_1"), "{text}");
    let v: Value = serde_json::from_slice(b).unwrap();
    assert_eq!(v["stop_reason"], "refusal", "{text}");
    assert_eq!(v["content"], json!([{"type":"text","text":"hel"}]), "{text}");
}

/// A Responses stream: a `message` item with text "hel", then a `function_call` item `call_1` with one argument
/// delta `CUT` and no `output_item.done`, then `last`.
fn responses_cut_call(last: Value) -> Vec<u8> {
    sse(&[
        json!({"type":"response.created","sequence_number":0,"response":{"id":"resp_up","object":"response","model":"up-rs","status":"in_progress","output":[],"usage":null}}),
        json!({"type":"response.output_item.added","sequence_number":1,"output_index":0,"item":{"type":"message","id":"msg_up","role":"assistant","content":[]}}),
        json!({"type":"response.content_part.added","sequence_number":2,"output_index":0,"content_index":0,"item_id":"msg_up","part":{"type":"output_text","text":""}}),
        json!({"type":"response.output_text.delta","sequence_number":3,"output_index":0,"content_index":0,"item_id":"msg_up","delta":"hel"}),
        json!({"type":"response.output_item.done","sequence_number":4,"output_index":0,"item":{"type":"message","id":"msg_up","role":"assistant","content":[{"type":"output_text","text":"hel"}]}}),
        json!({"type":"response.output_item.added","sequence_number":5,"output_index":1,"item":{"type":"function_call","id":"fc_up","call_id":"call_1","name":"exec_command","arguments":""}}),
        json!({"type":"response.function_call_arguments.delta","sequence_number":6,"output_index":1,"item_id":"fc_up","delta":CUT}),
        last,
    ])
}

#[test] // chat stream, call held at content_filter
fn messages_client_chat_content_filter_streamed_has_no_dropped_key() {
    let h = start(C);
    h.reply_with(
        200,
        SSE,
        chat_sse(&[
            chunk(json!({"role":"assistant","content":"hel"}), Value::Null),
            chunk(json!({"tool_calls":[call()]}), Value::Null),
            chunk(json!({}), json!("content_filter")),
            json!({"id":"chatcmpl-1","object":"chat.completion.chunk","model":"up-rs","choices":[],"usage":usage_raw()}),
        ]),
    );
    let (s, _, b) = h.post(M, &client_body(true));
    assert_eq!(s, 200, "{}", String::from_utf8_lossy(&b));
    assert!(contains(&b, b"\"hel\""), "{}", String::from_utf8_lossy(&b));
    assert_stream_dropped(&b);
    assert_row_code(&h);
}

#[test] // chat JSON body, non-streamed request
fn messages_client_chat_content_filter_json_has_no_dropped_key() {
    let h = start(C);
    let mut c = call();
    c.as_object_mut().unwrap().remove("index");
    h.reply_with(
        200,
        JSON,
        serde_json::to_vec(&json!({"id":"chatcmpl-1","object":"chat.completion","model":"up-rs",
            "choices":[{"index":0,"message":{"role":"assistant","content":"hel","tool_calls":[c]},"finish_reason":"content_filter"}],
            "usage":usage_raw()}))
        .unwrap(),
    );
    let (s, _, b) = h.post(M, &client_body(false));
    assert_eq!(s, 200, "{}", String::from_utf8_lossy(&b));
    assert!(!contains(&b, KEY), "{}", String::from_utf8_lossy(&b));
    let v: Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(v["stop_reason"], "refusal");
    assert_eq!(v["content"], json!([{"type":"text","text":"hel"}]));
    assert_row_code(&h);
}

#[test] // Responses stream, function_call open at response.incomplete content_filter
fn messages_client_responses_upstream_content_filter_drops_open_call() {
    let h = start(O);
    h.reply_with(
        200,
        SSE,
        responses_cut_call(json!({"type":"response.incomplete","sequence_number":8,
            "response":{"id":"resp_up","object":"response","model":"up-rs","status":"incomplete",
            "incomplete_details":{"reason":"content_filter"},"output":[],"usage":{"input_tokens":7,"output_tokens":3,"total_tokens":10}}})),
    );
    let (s, _, b) = h.post(M, &client_body(true));
    assert_eq!(s, 200, "{}", String::from_utf8_lossy(&b));
    assert!(contains(&b, b"\"hel\""), "{}", String::from_utf8_lossy(&b));
    assert_stream_dropped(&b);
    assert_row_code(&h);
}

#[test] // K§6.4.3 obligation 9: a held function_call block over HOLD_MAX ends the stream; nothing of it is sent
fn messages_client_responses_upstream_held_call_over_hold_max_errors() {
    let h = start(O);
    let piece = "a".repeat(64 << 10);
    let mut evs = vec![
        json!({"type":"response.created","sequence_number":0,"response":{"id":"resp_up","object":"response","model":"up-rs","status":"in_progress","output":[],"usage":null}}),
        json!({"type":"response.output_item.added","sequence_number":1,"output_index":0,
               "item":{"type":"function_call","id":"fc_up","call_id":"call_1","name":"exec_command","arguments":""}}),
    ];
    for i in 0..HOLD_MAX / piece.len() + 1 {
        evs.push(json!({"type":"response.function_call_arguments.delta","sequence_number":2 + i,"output_index":0,"item_id":"fc_up","delta":piece}));
    }
    evs.push(json!({"type":"response.completed","sequence_number":999,"response":{"id":"resp_up","object":"response","model":"up-rs",
        "status":"completed","output":[],"usage":{"input_tokens":7,"output_tokens":3,"total_tokens":10}}}));
    h.reply_with(200, SSE, sse(&evs));
    let (s, _, b) = h.post(M, &client_body(true));
    assert_eq!(s, 200);
    let evs = events(&b);
    let names: Vec<&str> = evs.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["message_start", "error"], "{names:?}");
    assert_eq!(evs[1].1["error"]["message"], "upstream tool call too long");
    assert!(!contains(&b, b"tool_use") && !contains(&b, b"call_1"));
}

#[test] // Responses JSON body, non-streamed request: function_call item in a response incomplete at content_filter
fn messages_client_responses_upstream_content_filter_json_drops_call() {
    let h = start(O);
    h.reply_with(
        200,
        JSON,
        serde_json::to_vec(&json!({"id":"resp_up","object":"response","model":"up-rs","status":"incomplete",
            "incomplete_details":{"reason":"content_filter"},
            "output":[
                {"type":"message","id":"msg_up","role":"assistant","content":[{"type":"output_text","text":"hel"}]},
                {"type":"function_call","id":"fc_up","call_id":"call_1","name":"exec_command","arguments":CUT}
            ],
            "usage":{"input_tokens":7,"output_tokens":3,"total_tokens":10}}))
        .unwrap(),
    );
    let (s, _, b) = h.post(M, &client_body(false));
    assert_eq!(s, 200, "{}", String::from_utf8_lossy(&b));
    assert_json_dropped(&b);
    assert_row_code(&h);
}
