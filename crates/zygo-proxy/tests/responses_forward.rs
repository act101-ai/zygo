//! RS-4: the response side of `POST /openai/v1/responses` toward Anthropic and chat upstreams (K§6.4.2, K§6.4.3
//! stream obligations 6 and 8, K§6.7, K§11 items 14 and 18): Responses SSE after the tee, the whole-answer Response
//! object, errors after the first event, non-2xx answers, and the usage row on the Messages form. Mock upstream on
//! 127.0.0.1; no agent binary, no network.

#[path = "support/responses.rs"]
mod support;

use http::HeaderMap;
use regex::Regex;
use serde_json::{json, Value};
use support::{codex_request, Harness, PROVIDER};
use zygo_config::Dialect::{Anthropic as A, OpenaiChat as C};
use zygo_core::Agent;

const R: &str = "openai/v1/responses";
const SSE: &str = "text/event-stream";
const JSON: &str = "application/json";

fn start(d: zygo_config::Dialect) -> Harness {
    Harness::start(d, Agent::ClaudeCode, "")
}

// ---- helpers (no logic under test) ----

/// `event: <type>\ndata: <compact json>\n\n` per event.
fn msg_sse(evs: &[Value]) -> Vec<u8> {
    evs.iter().flat_map(|e| format!("event: {}\ndata: {e}\n\n", e["type"].as_str().unwrap()).into_bytes()).collect()
}

/// `data: <json>\n\n` per chunk, then `data: [DONE]\n\n`.
fn chat_sse(chunks: &[Value]) -> Vec<u8> {
    let mut out: Vec<u8> = chunks.iter().flat_map(|c| format!("data: {c}\n\n").into_bytes()).collect();
    out.extend_from_slice(b"data: [DONE]\n\n");
    out
}

/// The `(event, data)` pairs of an SSE body.
fn events(b: &[u8]) -> Vec<(String, Value)> {
    let text = String::from_utf8_lossy(b).replace("\r\n", "\n");
    text.split("\n\n")
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

fn names(evs: &[(String, Value)]) -> Vec<&str> {
    evs.iter().map(|(n, _)| n.as_str()).collect()
}

fn terminals(evs: &[(String, Value)]) -> Vec<&(String, Value)> {
    evs.iter().filter(|(n, _)| matches!(n.as_str(), "response.completed" | "response.incomplete" | "response.failed")).collect()
}

fn done_items(evs: &[(String, Value)]) -> Vec<Value> {
    evs.iter().filter(|(n, _)| n == "response.output_item.done").map(|(_, d)| d["item"].clone()).collect()
}

/// Each generated id (`<prefix>_<ULID>`) as `<prefix>_X`.
fn norm(v: Value) -> Value {
    let re = Regex::new(r"\b(resp|msg|rs|fc|ctc)_[0-9A-HJKMNP-TV-Z]{26}").unwrap();
    serde_json::from_str(&re.replace_all(&v.to_string(), "${1}_X")).unwrap()
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

fn with_stream(mut body: Value, stream: bool) -> Value {
    body["stream"] = json!(stream);
    body
}

fn counts(r: &zygo_usage::RequestRow) -> (u64, u64, u64, u64) {
    (r.input_tokens, r.cache_read_tokens, r.cache_write_tokens, r.output_tokens)
}

/// The `response.completed` usage of the Messages fixtures.
fn msg_usage() -> Value {
    json!({"input_tokens":135,"input_tokens_details":{"cached_tokens":30},"output_tokens":42,"total_tokens":177})
}

fn chat_usage() -> Value {
    json!({"input_tokens":120,"input_tokens_details":{"cached_tokens":20},"output_tokens":7,"total_tokens":127})
}

// ---- fixtures ----

const PATCH: &str = "*** Begin Patch\n*** End Patch\n";

fn message_start() -> Value {
    json!({"type":"message_start","message":{"id":"msg_up","type":"message","role":"assistant","model":"up-rs",
        "content":[],"usage":{"input_tokens":100,"cache_read_input_tokens":30,"cache_creation_input_tokens":5,"output_tokens":1}}})
}

fn text_start(i: u64) -> Value {
    json!({"type":"content_block_start","index":i,"content_block":{"type":"text","text":""}})
}

fn text_delta(i: u64, t: &str) -> Value {
    json!({"type":"content_block_delta","index":i,"delta":{"type":"text_delta","text":t}})
}

fn stop(i: u64) -> Value {
    json!({"type":"content_block_stop","index":i})
}

#[allow(non_snake_case)]
fn MSG_EVENTS() -> Vec<Value> {
    vec![
        message_start(),
        json!({"type":"ping"}),
        text_start(0),
        text_delta(0, "hel"),
        text_delta(0, "lo"),
        stop(0),
        json!({"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_1","name":"exec_command","input":{}}}),
        json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"cmd\":\"ls\"}"}}),
        stop(1),
        json!({"type":"content_block_start","index":2,"content_block":{"type":"tool_use","id":"toolu_2","name":"apply_patch","input":{}}}),
        json!({"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta",
            "partial_json":json!({"input":PATCH}).to_string()}}),
        stop(2),
        json!({"type":"content_block_start","index":3,"content_block":{"type":"server_tool_use","id":"srvtoolu_1","name":"web_search","input":{}}}),
        stop(3),
        json!({"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":42}}),
        json!({"type":"message_stop"}),
    ]
}

#[allow(non_snake_case)]
fn MSG_JSON(server_tool: bool) -> Value {
    let mut content = vec![
        json!({"type":"text","text":"hello"}),
        json!({"type":"tool_use","id":"toolu_1","name":"exec_command","input":{"cmd":"ls"}}),
        json!({"type":"tool_use","id":"toolu_2","name":"apply_patch","input":{"input":PATCH}}),
    ];
    if server_tool {
        content.push(json!({"type":"server_tool_use","id":"srvtoolu_1","name":"web_search","input":{}}));
    }
    json!({"id":"msg_up","type":"message","role":"assistant","model":"up-rs","content":content,
        "stop_reason":"tool_use","stop_sequence":null,
        "usage":{"input_tokens":100,"cache_read_input_tokens":30,"cache_creation_input_tokens":5,"output_tokens":42}})
}

fn chunk(delta: Value, finish: Value) -> Value {
    json!({"id":"chatcmpl-1","object":"chat.completion.chunk","model":"up-rs",
        "choices":[{"index":0,"delta":delta,"finish_reason":finish}]})
}

fn call(args: &str) -> Value {
    json!({"index":0,"id":"call_1","type":"function","function":{"name":"exec_command","arguments":args}})
}

fn chat_usage_raw() -> Value {
    json!({"prompt_tokens":120,"completion_tokens":7,"total_tokens":127,"prompt_tokens_details":{"cached_tokens":20}})
}

#[allow(non_snake_case)]
fn CHAT_CHUNKS(args: &str) -> Vec<Value> {
    vec![
        chunk(json!({"role":"assistant","content":"hel"}), Value::Null),
        chunk(json!({"content":"lo"}), Value::Null),
        chunk(json!({"tool_calls":[call(args)]}), Value::Null),
        chunk(json!({}), json!("tool_calls")),
        json!({"id":"chatcmpl-1","object":"chat.completion.chunk","model":"up-rs","choices":[],"usage":chat_usage_raw()}),
    ]
}

#[allow(non_snake_case)]
fn CHAT_JSON(args: &str) -> Value {
    let mut c = call(args);
    c.as_object_mut().unwrap().remove("index");
    json!({"id":"chatcmpl-1","object":"chat.completion","model":"up-rs",
        "choices":[{"index":0,"message":{"role":"assistant","content":"hello","tool_calls":[c]},"finish_reason":"tool_calls"}],
        "usage":chat_usage_raw()})
}

const LS: &str = "{\"cmd\":\"ls\"}";

fn args_of(items: &[Value]) -> Value {
    let fc = items.iter().find(|i| i["type"] == "function_call").expect("a function_call item");
    serde_json::from_str(fc["arguments"].as_str().unwrap()).unwrap()
}

// ---- tests ----

#[test] // K§6.4.2 row Anthropic: Messages SSE → tee → encoder; the row keeps the Messages counts
fn anthropic_stream_returns_responses_sse_and_usage() {
    let h = start(A);
    h.reply_with(200, SSE, msg_sse(&MSG_EVENTS()));
    let body = codex_request("after-custom-tool-call-output");
    let (s, hd, b) = h.post(R, &body);
    assert_eq!(s, 200, "{}", String::from_utf8_lossy(&b));
    assert!(hv(&hd, "content-type").starts_with(SSE), "{hd:?}");
    assert_eq!(h.upstream_paths(), ["/v1/messages"]);
    let evs = events(&b);
    let n = names(&evs);
    assert_eq!(n[..2], ["response.created", "response.in_progress"], "{n:?}");
    for (i, (_, d)) in evs.iter().enumerate() {
        assert_eq!(d["sequence_number"], json!(i), "{d}");
    }
    let t = terminals(&evs);
    assert_eq!(t.len(), 1, "{n:?}");
    let (last, d) = evs.last().unwrap();
    assert_eq!(last, "response.completed");
    assert_eq!(d["response"]["output"], json!([]));
    assert_eq!(d["response"]["model"], body["model"]);
    assert_eq!(d["response"]["usage"], msg_usage());
    let items = done_items(&evs);
    let types: Vec<&str> = items.iter().map(|i| i["type"].as_str().unwrap()).collect();
    assert_eq!(types, ["message", "function_call", "custom_tool_call"]);
    // `Calls` comes from the client's body: upstream, `apply_patch` is a plain tool.
    assert_eq!(items[2]["name"], "apply_patch");
    assert_eq!(items[2]["call_id"], "toolu_2");
    assert_eq!(items[2]["input"], PATCH);
    assert_eq!(items[1]["call_id"], "toolu_1");
    assert_eq!(args_of(&items), json!({"cmd":"ls"}));
    assert!(!n.contains(&"ping"), "{n:?}");
    assert!(!evs.iter().any(|(_, d)| d.to_string().contains("server_tool_use")));
    let rows = h.usage_rows();
    assert_eq!(rows.len(), 1);
    let r = &rows[0];
    assert_eq!((r.status, r.provider.as_str()), (200, PROVIDER));
    assert_eq!(counts(r), (100, 30, 5, 42));
    assert_eq!(r.route, "/openai/v1/responses");
    assert!(r.adjustments.iter().any(|a| a == "server_tool_dropped"), "{:?}", r.adjustments);
}

#[test] // K§11 item 18: Responses → chat; chat SSE → mapper → tee → encoder
fn chat_stream_returns_responses_sse_and_usage() {
    let h = start(C);
    h.reply_with(200, SSE, chat_sse(&CHAT_CHUNKS(LS)));
    let (s, hd, b) = h.post(R, &codex_request("first-turn"));
    assert_eq!(h.upstream_paths(), ["/v1/chat/completions"]);
    assert_eq!(s, 200, "{}", String::from_utf8_lossy(&b));
    assert!(hv(&hd, "content-type").starts_with(SSE), "{hd:?}");
    let evs = events(&b);
    let t = terminals(&evs);
    assert_eq!(t.len(), 1, "{:?}", names(&evs));
    assert_eq!(t[0].0, "response.completed");
    assert_eq!(t[0].1["response"]["usage"], chat_usage());
    let items = done_items(&evs);
    let types: Vec<&str> = items.iter().map(|i| i["type"].as_str().unwrap()).collect();
    assert_eq!(types, ["message", "function_call"]);
    assert_eq!(items[0]["content"][0]["text"], "hello");
    let rows = h.usage_rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(counts(&rows[0]), (100, 20, 0, 7));
}

#[test] // K§11 item 14, second half: an upstream error event after the first event → one response.failed
fn anthropic_error_after_first_event_one_failed() {
    let h = start(A);
    let evs = [
        message_start(),
        text_start(0),
        text_delta(0, "par"),
        json!({"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}),
        stop(0),
        json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":3}}),
        json!({"type":"message_stop"}),
    ];
    h.reply_with(200, SSE, msg_sse(&evs));
    let (s, _, b) = h.post(R, &codex_request("after-custom-tool-call-output"));
    assert_eq!(s, 200);
    let evs = events(&b);
    let deltas: Vec<&Value> = evs.iter().filter(|(n, _)| n == "response.output_text.delta").map(|(_, d)| d).collect();
    assert_eq!(deltas.len(), 1);
    assert_eq!(deltas[0]["delta"], "par");
    assert_eq!(terminals(&evs).len(), 1, "{:?}", names(&evs));
    let (last, d) = evs.last().unwrap();
    assert_eq!(last, "response.failed");
    assert_eq!(d["response"]["error"], json!({"code":"overloaded_error","message":"Overloaded"}));
    assert!(!names(&evs).contains(&"response.completed"));
}

#[test] // the chat half of K§11 item 14
fn chat_error_after_first_event_one_failed() {
    let h = start(C);
    let chunks = [
        chunk(json!({"role":"assistant","content":"par"}), Value::Null),
        json!({"error":{"message":"boom","type":"server_error"}}),
        chunk(json!({}), json!("stop")),
    ];
    h.reply_with(200, SSE, chat_sse(&chunks));
    let (s, _, b) = h.post(R, &codex_request("first-turn"));
    assert_eq!(s, 200);
    let evs = events(&b);
    assert_eq!(terminals(&evs).len(), 1, "{:?}", names(&evs));
    let (last, d) = evs.last().unwrap();
    assert_eq!(last, "response.failed");
    assert_eq!(d["response"]["error"], json!({"code":"api_error","message":"boom"}));
}

#[test] // stream obligation 8: one Response object with the stream's content
fn non_streaming_anthropic_same_content() {
    let h = start(A);
    let body = codex_request("after-custom-tool-call-output");
    h.reply_with(200, SSE, msg_sse(&MSG_EVENTS()));
    let (_, _, b) = h.post(R, &body);
    let evs = events(&b);
    let streamed = norm(Value::Array(done_items(&evs)));
    let usage = terminals(&evs)[0].1["response"]["usage"].clone();

    h.reply_with(200, JSON, serde_json::to_vec(&MSG_JSON(true)).unwrap());
    let (s, hd, b) = h.post(R, &with_stream(body.clone(), false));
    assert_eq!(s, 200, "{}", String::from_utf8_lossy(&b));
    assert!(hv(&hd, "content-type").starts_with(JSON), "{hd:?}");
    let r: Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(r["object"], "response");
    assert_eq!(r["status"], "completed");
    assert_eq!(norm(r["output"].clone()), streamed);
    assert_eq!(r["usage"], usage);
    assert!(hv(&hd, "x-zygo-adjustments").split(',').any(|a| a == "server_tool_dropped"), "{hd:?}");

    // A JSON answer to a streaming request: SSE with the same items (no server tool block: see the plan's lead notes).
    h.reply_with(200, JSON, serde_json::to_vec(&MSG_JSON(false)).unwrap());
    let (s, hd, b) = h.post(R, &with_stream(body, true));
    assert_eq!(s, 200);
    assert!(hv(&hd, "content-type").starts_with(SSE), "{hd:?}");
    let evs = events(&b);
    assert_eq!(terminals(&evs).len(), 1);
    assert_eq!(norm(Value::Array(done_items(&evs))), streamed);

    let rows = h.usage_rows();
    assert_eq!(rows.len(), 3);
    assert_eq!(counts(&rows[1]), (100, 30, 5, 42));
    assert_eq!(counts(&rows[2]), (100, 30, 5, 42));
}

#[test]
fn non_streaming_chat_same_content() {
    let h = start(C);
    h.reply_with(200, SSE, chat_sse(&CHAT_CHUNKS(LS)));
    let (_, _, b) = h.post(R, &codex_request("first-turn"));
    let streamed = norm(Value::Array(done_items(&events(&b))));

    h.reply_with(200, JSON, serde_json::to_vec(&CHAT_JSON(LS)).unwrap());
    let (s, hd, b) = h.post(R, &with_stream(codex_request("first-turn"), false));
    assert_eq!(s, 200, "{}", String::from_utf8_lossy(&b));
    assert!(hv(&hd, "content-type").starts_with(JSON), "{hd:?}");
    let r: Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(r["object"], "response");
    assert_eq!(norm(r["output"].clone()), streamed);
    assert_eq!(r["usage"], chat_usage());
    let rows = h.usage_rows();
    assert_eq!(counts(&rows[1]), (100, 20, 0, 7));
}

#[test] // stream obligation 8, non-2xx half; K-D50
fn non_2xx_non_streaming_keeps_status_openai_error() {
    let h = start(A);
    h.reply_with(500, JSON, br#"{"type":"error","error":{"type":"api_error","message":"boom"}}"#.to_vec());
    let (s, hd, b) = h.post(R, &with_stream(codex_request("after-custom-tool-call-output"), false));
    assert_eq!(s, 500);
    assert!(hv(&hd, "content-type").starts_with(JSON), "{hd:?}");
    assert_eq!(oai_err(&b), json!({"type":"api_error","message":"boom"}));
    assert!(!b.windows(5).any(|w| w == b"data:"));

    let h = start(C);
    h.reply_with(429, JSON, br#"{"error":{"message":"slow down","type":"rate_limit"}}"#.to_vec());
    let (s, _, b) = h.post(R, &with_stream(codex_request("first-turn"), false));
    assert_eq!(s, 429);
    assert_eq!(oai_err(&b), json!({"type":"rate_limit_error","message":"slow down"}));
}

#[test] // stream obligation 6: a read error after the start → one response.failed, last
fn read_error_mid_stream_one_failed() {
    let h = start(A);
    h.reply_cut(200, SSE, msg_sse(&[message_start(), text_start(0), text_delta(0, "par")]));
    let (s, _, b) = h.post(R, &codex_request("after-custom-tool-call-output"));
    assert_eq!(s, 200);
    let evs = events(&b);
    assert_eq!(terminals(&evs).len(), 1, "{:?}", names(&evs));
    let (last, d) = evs.last().unwrap();
    assert_eq!(last, "response.failed");
    assert_eq!(d["response"]["error"]["code"], "api_error");
}

#[test] // base §7.6: an injected null is removed on the way back to a Responses client
fn injected_null_removed_for_responses_client() {
    let h = start(C);
    let args = "{\"cmd\":\"ls\",\"workdir\":null}";
    h.reply_with(200, SSE, chat_sse(&CHAT_CHUNKS(args)));
    let (s, hd, b) = h.post(R, &codex_request("first-turn"));
    assert_eq!(s, 200, "{}", String::from_utf8_lossy(&b));
    let adj = hv(&hd, "x-zygo-adjustments");
    assert!(adj.split(',').any(|a| a.starts_with("schema:optional_property:")), "{adj}");
    assert_eq!(args_of(&done_items(&events(&b))), json!({"cmd":"ls"}));

    h.reply_with(200, JSON, serde_json::to_vec(&CHAT_JSON(args)).unwrap());
    let (s, _, b) = h.post(R, &with_stream(codex_request("first-turn"), false));
    assert_eq!(s, 200, "{}", String::from_utf8_lossy(&b));
    let r: Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(args_of(r["output"].as_array().unwrap()), json!({"cmd":"ls"}));
}

#[test] // a 2xx that is no Messages body never reaches the client (errors.rs upstream_bad_response)
fn bad_2xx_body_is_502_openai_shape() {
    let h = start(A);
    h.reply_with(200, JSON, br#"{"output":[]}"#.to_vec());
    let (s, _, b) = h.post(R, &with_stream(codex_request("after-custom-tool-call-output"), false));
    assert_eq!(s, 502);
    assert_eq!(
        oai_err(&b),
        json!({"type":"zygo_upstream_bad_response","message":"upstream response could not be translated"})
    );
}
