//! RS-5: a Responses client toward an `openai` upstream (K§6.4.3 Responses → Responses): the body passes except
//! `model`, `store` and `include`, the stream passes byte for byte, and the usage row has the counts of
//! `response.completed`, `response.incomplete` or the JSON body's `usage` (K§6.4.2 C3, K§11 items 17 and 19). Mock
//! upstream on 127.0.0.1; no agent binary, no network.

#[path = "support/responses.rs"]
mod support;

use http::HeaderMap;
use serde_json::{json, Value};
use support::{codex_request, Harness, MODEL, PROVIDER, UPSTREAM_MODEL};
use zygo_config::Dialect;
use zygo_core::Agent;
use zygo_usage::RequestRow;

const R: &str = "openai/v1/responses";
const M: &str = "anthropic/v1/messages";
const SSE: &str = "text/event-stream";
const JSON: &str = "application/json";

fn start() -> Harness {
    Harness::start(Dialect::Openai, Agent::ClaudeCode, "")
}

/// (input, cache read, cache write, output) of a usage row.
fn counts(r: &RequestRow) -> (u64, u64, u64, u64) {
    (r.input_tokens, r.cache_read_tokens, r.cache_write_tokens, r.output_tokens)
}

/// An OpenAI error body: an object with `error` and no top-level `type`; returns `error`.
fn oai_err(b: &[u8]) -> Value {
    let v: Value = serde_json::from_slice(b).unwrap_or_else(|_| panic!("not JSON: {}", String::from_utf8_lossy(b)));
    let o = v.as_object().expect("an object");
    assert!(o.contains_key("error"), "{v}");
    assert!(!o.contains_key("type"), "{v}");
    v["error"].clone()
}

fn hv(h: &HeaderMap, n: &str) -> String {
    h.get(n).map(|v| v.to_str().unwrap().to_string()).unwrap_or_default()
}

/// `event: <type>\ndata: <compact json>\n\n` per event.
fn sse(evs: &[Value]) -> Vec<u8> {
    evs.iter().flat_map(|e| format!("event: {}\ndata: {e}\n\n", e["type"].as_str().unwrap()).into_bytes()).collect()
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

fn usage_up() -> Value {
    json!({"input_tokens":120,"input_tokens_details":{"cached_tokens":20},"output_tokens":30,"output_tokens_details":{"reasoning_tokens":10},"total_tokens":150})
}

fn r0() -> Value {
    json!({"id":"resp_up","object":"response","model":"up-rs","status":"in_progress","output":[],"usage":null})
}

fn msg_done() -> Value {
    json!({"type":"message","id":"msg_up","role":"assistant","status":"completed","content":[{"type":"output_text","text":"hi"}]})
}

fn resp_events(terminal: &str, status: &str, usage: Value) -> Vec<Value> {
    let mut last = r0();
    last["status"] = json!(status);
    last["output"] = json!([msg_done()]);
    last["usage"] = usage;
    if terminal == "response.incomplete" {
        last["incomplete_details"] = json!({"reason":"max_output_tokens"});
    }
    vec![
        json!({"type":"response.created","sequence_number":0,"response":r0()}),
        json!({"type":"response.output_item.added","sequence_number":1,"output_index":0,
               "item":{"type":"message","id":"msg_up","role":"assistant","status":"in_progress","content":[]}}),
        json!({"type":"response.content_part.added","sequence_number":2,"output_index":0,"content_index":0,"item_id":"msg_up",
               "part":{"type":"output_text","text":""}}),
        json!({"type":"response.output_text.delta","sequence_number":3,"output_index":0,"content_index":0,"item_id":"msg_up","delta":"hi"}),
        json!({"type":"response.content_part.done","sequence_number":4,"output_index":0,"content_index":0,"item_id":"msg_up",
               "part":{"type":"output_text","text":"hi"}}),
        json!({"type":"response.output_item.done","sequence_number":5,"output_index":0,"item":msg_done()}),
        json!({"type":terminal,"sequence_number":6,"response":last}),
    ]
}

fn completed() -> Vec<u8> {
    sse(&resp_events("response.completed", "completed", usage_up()))
}

fn resp_json() -> Value {
    json!({"id":"resp_up","object":"response","model":"up-rs","status":"completed","output":[msg_done()],
           "usage":{"input_tokens":50,"input_tokens_details":{"cached_tokens":10},"output_tokens":5,"total_tokens":55}})
}

fn ns_tool() -> Value {
    json!({"type":"namespace","name":"multi_agent_v1","description":"agents",
           "tools":[{"type":"function","name":"spawn_agent","description":"spawn","parameters":{"type":"object","properties":{}}}]})
}

fn chat_sse() -> Vec<u8> {
    let evs = [
        json!({"id":"c1","object":"chat.completion.chunk","model":"up-rs","choices":[{"index":0,"delta":{"role":"assistant","content":"hi"},"finish_reason":null}]}),
        json!({"id":"c1","object":"chat.completion.chunk","model":"up-rs","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}),
        json!({"id":"c1","object":"chat.completion.chunk","model":"up-rs","choices":[],
               "usage":{"prompt_tokens":120,"completion_tokens":30,"total_tokens":150,"prompt_tokens_details":{"cached_tokens":20}}}),
    ];
    let mut out: Vec<u8> = evs.iter().flat_map(|e| format!("data: {e}\n\n").into_bytes()).collect();
    out.extend_from_slice(b"data: [DONE]\n\n");
    out
}

fn with_include(include: Value) -> Value {
    let mut b = codex_request("turn-with-reasoning");
    b["include"] = include;
    b
}

/// K§11 item 17, K§6.4.3, RS-9: the body passes except `model`, `store` and `include`; reasoning items keep their
/// bytes; a `namespace` tool is not flattened; no `cache_control`.
#[test]
fn body_passes_except_model_store_include() {
    let h = start();
    let mut b = codex_request("turn-with-reasoning");
    b["store"] = json!(true);
    b.as_object_mut().unwrap().remove("include");
    b["tools"].as_array_mut().unwrap().push(ns_tool());
    h.reply_with(200, SSE, completed());
    let (s, _, _) = h.post(R, &b);
    assert_eq!(s.as_u16(), 200);
    assert_eq!(h.upstream_paths(), ["/v1/responses"]);
    let up = &h.upstream_bodies()[0];
    let mut want = b.clone();
    want["model"] = json!(UPSTREAM_MODEL);
    want["store"] = json!(false);
    want["include"] = json!(["reasoning.encrypted_content"]);
    assert_eq!(up, &want);
    let raw = &h.upstream_raw()[0];
    for item in b["input"].as_array().unwrap().iter().filter(|i| i["type"] == "reasoning") {
        assert!(contains(raw, serde_json::to_string(item).unwrap().as_bytes()), "{item}");
    }
    assert!(up["tools"].as_array().unwrap().contains(&ns_tool()));
    assert!(!contains(raw, b"cache_control"));
}

/// K§6.4.3: the client's `include` entries stay; `reasoning.encrypted_content` is appended once.
#[test]
fn include_keeps_client_entries() {
    let h = start();
    h.reply_with(200, SSE, completed());
    for inc in [json!(["message.output_text.logprobs"]), json!(["reasoning.encrypted_content", "message.output_text.logprobs"]), Value::Null] {
        let (s, _, _) = h.post(R, &with_include(inc));
        assert_eq!(s.as_u16(), 200);
    }
    let ups = h.upstream_bodies();
    assert_eq!(ups[0]["include"], json!(["message.output_text.logprobs", "reasoning.encrypted_content"]));
    assert_eq!(ups[1]["include"], json!(["reasoning.encrypted_content", "message.output_text.logprobs"]));
    assert_eq!(ups[2]["include"], json!(["reasoning.encrypted_content"]));
    assert!(ups.iter().all(|u| u["store"] == json!(false)));
}

/// K§6.4.3: an `include` of another JSON type is a 400 before any upstream request.
#[test]
fn include_not_array_400() {
    let h = start();
    let (s, _, b) = h.post(R, &with_include(json!("reasoning.encrypted_content")));
    assert_eq!(s.as_u16(), 400);
    assert_eq!(oai_err(&b), json!({"type":"zygo_bad_request","message":"include must be an array"}));
    assert!(h.upstream_bodies().is_empty());
}

/// K§11 item 19 second half, K-D48: no cache marker toward a Responses upstream, even with OpenCode's
/// `insert_markers.opencode = true` default.
#[test]
fn no_cache_markers_toward_responses_upstream() {
    let h = Harness::start(Dialect::Openai, Agent::OpenCode, "");
    h.reply_with(200, SSE, completed());
    let (s, _, _) = h.post(R, &codex_request("first-turn"));
    assert_eq!(s.as_u16(), 200);
    assert!(!contains(&h.upstream_raw()[0], b"cache_control"));
}

/// K§11 item 17, K§6.4.2 C3: the stream passes byte for byte; the row has the `response.completed` counts.
#[test]
fn stream_passes_bytes_and_records_completed_usage() {
    let h = start();
    let up = completed();
    h.reply_with(200, SSE, up.clone());
    let (s, hd, b) = h.post(R, &codex_request("turn-with-reasoning"));
    assert_eq!(s.as_u16(), 200);
    assert!(hv(&hd, "content-type").starts_with(SSE));
    assert_eq!(&b[..], &up[..]);
    let rows = h.usage_rows();
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0].status, rows[0].provider.as_str(), rows[0].route.as_str()), (200, PROVIDER, "/openai/v1/responses"));
    assert_eq!(counts(&rows[0]), (100, 20, 0, 30));
}

/// K§6.4.2 C3: a `response.incomplete` ending carries the counts.
#[test]
fn incomplete_usage_recorded() {
    let h = start();
    let up = sse(&resp_events(
        "response.incomplete",
        "incomplete",
        json!({"input_tokens":200,"input_tokens_details":{"cached_tokens":0},"output_tokens":64,"total_tokens":264}),
    ));
    h.reply_with(200, SSE, up.clone());
    let (s, _, b) = h.post(R, &codex_request("turn-with-reasoning"));
    assert_eq!(s.as_u16(), 200);
    assert_eq!(&b[..], &up[..]);
    assert_eq!(counts(&h.usage_rows()[0]), (200, 0, 0, 64));
}

/// K§11 item 17: a non-streaming request records the JSON body's `usage`.
#[test]
fn non_streaming_records_json_usage() {
    let h = start();
    let mut req = codex_request("turn-with-reasoning");
    req["stream"] = json!(false);
    h.reply_with(200, JSON, serde_json::to_vec(&resp_json()).unwrap());
    let (s, hd, b) = h.post(R, &req);
    assert_eq!(s.as_u16(), 200);
    assert!(hv(&hd, "content-type").starts_with(JSON));
    assert_eq!(serde_json::from_slice::<Value>(&b).unwrap(), resp_json());
    assert_eq!(counts(&h.usage_rows()[0]), (40, 10, 0, 5));
}

/// K§6.4.3 candidates: a Responses client takes the `openai` dialect first, wherever the provider lists it.
#[test]
fn openai_first_candidate_dialect() {
    let h = Harness::start_dialects(&[Dialect::Anthropic, Dialect::OpenaiChat, Dialect::Openai], Agent::ClaudeCode, "");
    h.reply_with(200, SSE, completed());
    let (s, _, _) = h.post(R, &codex_request("first-turn"));
    assert_eq!(h.upstream_paths(), ["/v1/responses"]);
    assert_eq!(s.as_u16(), 200);
}

/// K§6.4.2 C3: a mapped stream (a Responses or chat upstream answering a Messages client) is read as Messages events.
#[test]
fn mapped_streams_parse_as_messages() {
    for (d, reply) in [(Dialect::Openai, completed()), (Dialect::OpenaiChat, chat_sse())] {
        let h = Harness::start(d, Agent::ClaudeCode, "");
        h.reply_with(200, SSE, reply);
        let body = json!({"model":MODEL,"max_tokens":64,"stream":true,"messages":[{"role":"user","content":"hi"}]});
        let (s, _, _) = h.post(M, &body);
        assert_eq!(s.as_u16(), 200, "{d:?}");
        let rows = h.usage_rows();
        assert_eq!((rows[0].route.as_str(), counts(&rows[0])), ("/anthropic/v1/messages", (100, 20, 0, 30)), "{d:?}");
    }
}

/// Produces: `responses_usage` reads only the `usage` object of a terminal Responses event.
#[test]
fn responses_usage_reads_terminal_lines_only() {
    use zygo_proxy::tee::responses_usage;
    let line = |v: Value| format!("data: {v}").into_bytes();
    assert_eq!(responses_usage(&line(json!({"type":"response.completed","response":{"usage":usage_up()}}))), Some(usage_up()));
    assert_eq!(responses_usage(&line(json!({"type":"response.incomplete","response":{"usage":usage_up()}}))), Some(usage_up()));
    assert_eq!(responses_usage(&line(json!({"type":"response.created","response":{"usage":null}}))), None);
    assert_eq!(responses_usage(&line(json!({"type":"response.completed","response":{"usage":null}}))), None);
    assert_eq!(responses_usage(&line(json!({"type":"message_delta","usage":{"output_tokens":3}}))), None);
    assert_eq!(responses_usage(b"event: response.completed"), None);
    assert_eq!(responses_usage(b"data: {not json usage"), None);
}

/// K§6.4.4: a non-2xx reply passes with its status and body.
#[test]
fn non_2xx_passes_status_and_body() {
    let h = start();
    let reply = br#"{"error":{"type":"invalid_request_error","message":"bad reasoning item","param":"input","code":null}}"#.to_vec();
    h.reply_with(400, JSON, reply.clone());
    let (s, _, b) = h.post(R, &codex_request("turn-with-reasoning"));
    assert_eq!(s.as_u16(), 400);
    assert_eq!(&b[..], &reply[..]);
    assert_eq!(h.usage_rows()[0].status, 400);
}
