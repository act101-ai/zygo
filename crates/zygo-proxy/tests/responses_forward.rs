//! RS-4: the response side of `POST /openai/v1/responses` toward Anthropic and chat upstreams (K§6.4.2, K§6.4.3
//! stream obligations 6 and 8, K§6.7, K§11 items 14 and 18): Responses SSE after the tee, the whole-answer Response
//! object, errors after the first event, non-2xx answers, and the usage row on the Messages form. Mock upstream on
//! 127.0.0.1; no agent binary, no network.

#[path = "support/responses.rs"]
mod support;

use http::HeaderMap;
use regex::Regex;
use serde_json::{json, Value};
use support::{codex_request, Harness, PROVIDER, UPSTREAM_MODEL};
use zygo_config::Dialect::{Anthropic as A, OpenaiChat as C};
use zygo_core::Agent;
use zygo_proxy::tee::COPY_MAX;
use zygo_proxy::translate::reasoning::decode_thinking;
use zygo_proxy::translate::Target;

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

    // A JSON answer to a streaming request: SSE with the same items (the server tool block dropped by the encoder).
    h.reply_with(200, JSON, serde_json::to_vec(&MSG_JSON(true)).unwrap());
    let (s, hd, b) = h.post(R, &with_stream(body, true));
    assert_eq!(s, 200);
    assert!(hv(&hd, "content-type").starts_with(SSE), "{hd:?}");
    let evs = events(&b);
    assert_eq!(terminals(&evs).len(), 1);
    assert_eq!(norm(Value::Array(done_items(&evs))), streamed);

    let rows = h.usage_rows();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[2].adjustments.iter().filter(|a| *a == "server_tool_dropped").count(), 1, "{:?}", rows[2].adjustments);
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

#[test] // stream obligation 6: a read error after the start → one response.failed, last, with the read error's message
fn read_error_mid_stream_one_failed() {
    let h = start(A);
    h.reply_cut(200, SSE, msg_sse(&[message_start(), text_start(0), text_delta(0, "par")]));
    let (s, _, b) = h.post(R, &codex_request("after-custom-tool-call-output"));
    assert_eq!(s, 200);
    let evs = events(&b);
    let n = names(&evs);
    assert_eq!(terminals(&evs).len(), 1, "{n:?}");
    let delta = n.iter().position(|e| *e == "response.output_text.delta").expect("the text delta");
    assert_eq!(evs[delta].1["delta"], "par");
    let (last, d) = evs.last().unwrap();
    assert_eq!(last, "response.failed");
    assert!(delta < evs.len() - 1);
    assert_eq!(d["response"]["error"], json!({"code":"api_error","message":"upstream response failed"}));
}

#[test] // review round 1 F3: a read error before the first whole event → the stream starts, then fails with the read error
fn read_error_before_first_event_failed() {
    let h = start(A);
    let mut part = msg_sse(&[message_start()]);
    part.truncate(30);
    h.reply_cut(200, SSE, part);
    let (s, _, b) = h.post(R, &codex_request("after-custom-tool-call-output"));
    assert_eq!(s, 200);
    let evs = events(&b);
    assert_eq!(names(&evs), ["response.created", "response.in_progress", "response.failed"]);
    for (i, (_, d)) in evs.iter().enumerate() {
        assert_eq!(d["sequence_number"], json!(i), "{d}");
    }
    assert_eq!(evs[2].1["response"]["error"], json!({"code":"api_error","message":"upstream response failed"}));
}

#[test] // review round 1 F4: after the encoder's terminal event the upstream is read to its end for the usage row
fn encoder_failure_row_has_final_usage() {
    let h = start(A);
    let big = "x".repeat(5 << 20);
    let evs = [
        message_start(),
        text_start(0),
        text_delta(0, &big),
        stop(0),
        json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":4242}}),
        json!({"type":"message_stop"}),
    ];
    h.reply_with(200, SSE, msg_sse(&evs));
    let (s, _, b) = h.post(R, &codex_request("after-custom-tool-call-output"));
    assert_eq!(s, 200);
    let evs = events(&b);
    assert_eq!(terminals(&evs).len(), 1, "{:?}", names(&evs));
    assert_eq!(evs.last().unwrap().0, "response.failed");
    let rows = h.usage_rows();
    assert_eq!(counts(&rows[0]), (100, 30, 5, 4242));
}

#[test] // review round 2 G1: the client body ends at the encoder's terminal event; the rest of the upstream drains detached
fn client_released_at_completed() {
    let h = start(A);
    let evs = [
        message_start(),
        text_start(0),
        text_delta(0, "hi"),
        stop(0),
        json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":77}}),
        json!({"type":"message_stop"}),
    ];
    let release = h.reply_held(200, SSE, msg_sse(&evs));
    // A regression: this read never ends (the upstream is still open).
    let (s, _, b) = h.post(R, &codex_request("after-custom-tool-call-output"));
    assert_eq!(s, 200);
    let evs = events(&b);
    assert_eq!(terminals(&evs).len(), 1, "{:?}", names(&evs));
    assert_eq!(evs.last().unwrap().0, "response.completed");
    release.send(()).unwrap();
    assert_eq!(counts(&h.usage_rows()[0]), (100, 30, 5, 77));
}

#[test] // review round 2 G1: an encoder failure (a non-JSON event after `message_start`) ends the client body at once
fn client_released_at_encoder_failure() {
    let h = start(A);
    let mut body = msg_sse(&[message_start()]);
    body.extend_from_slice(b"event: content_block_start\ndata: not json\n\n");
    body.extend(msg_sse(&[
        json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":88}}),
        json!({"type":"message_stop"}),
    ]));
    let release = h.reply_held(200, SSE, body);
    let (s, _, b) = h.post(R, &codex_request("after-custom-tool-call-output"));
    assert_eq!(s, 200);
    let evs = events(&b);
    assert_eq!(terminals(&evs).len(), 1, "{:?}", names(&evs));
    assert_eq!(evs.last().unwrap().0, "response.failed");
    release.send(()).unwrap();
    assert_eq!(counts(&h.usage_rows()[0]), (100, 30, 5, 88));
}

/// An Anthropic stream that fails after its first event with `error`.
fn failing(error: Value) -> Vec<u8> {
    msg_sse(&[message_start(), text_start(0), text_delta(0, "par"), json!({"type":"error","error":error})])
}

#[test] // review round 1 F5, K§6.4.4: an in-stream context-limit error gets the PC14 code and the PC14 message rule
fn in_stream_context_error_pc14_code() {
    let h = start(A);
    let limit = "input length and `max_tokens` exceed context limit: 190000 + 32000 > 200000";
    let long = "prompt is too long: 210000 tokens > 200000 maximum";
    for m in [limit, long] {
        h.reply_with(200, SSE, failing(json!({"type":"invalid_request_error","message":m})));
        let (s, _, b) = h.post(R, &codex_request("after-custom-tool-call-output"));
        assert_eq!(s, 200);
        let evs = events(&b);
        assert_eq!(terminals(&evs).len(), 1);
        let (last, d) = evs.last().unwrap();
        assert_eq!(last, "response.failed");
        assert_eq!(d["response"]["error"], json!({"code":"context_length_exceeded","message":m}));
    }
    // A message that only contains the phrase is an ordinary error.
    let other = "note: prompt is too long for the cache";
    h.reply_with(200, SSE, failing(json!({"type":"invalid_request_error","message":other})));
    let (_, _, b) = h.post(R, &codex_request("after-custom-tool-call-output"));
    assert_eq!(events(&b).last().unwrap().1["response"]["error"]["code"], "invalid_request_error");

    // A chat upstream's context error (the §7.6 mapping) too.
    let h = start(C);
    let chunks = [
        chunk(json!({"role":"assistant","content":"par"}), Value::Null),
        json!({"error":{"message":"This model's maximum context length is 1000 tokens. However, your messages resulted in 1200 tokens.","code":"context_length_exceeded"}}),
    ];
    h.reply_with(200, SSE, chat_sse(&chunks));
    let (_, _, b) = h.post(R, &codex_request("first-turn"));
    let evs = events(&b);
    assert_eq!(terminals(&evs).len(), 1);
    let e = &evs.last().unwrap().1["response"]["error"];
    assert_eq!(*e, json!({"code":"context_length_exceeded","message":"prompt is too long: 1200 tokens > 1000 maximum"}));
}

#[test] // review round 2 G2, K§6.4.3 obligation 6: a `prompt is too long` message gets the 512-character cap and cleaning
fn in_stream_prompt_too_long_capped_and_cleaned() {
    let h = start(A);
    let m = format!("prompt is too long: 1 tokens > 0 maximum \u{1b}[2J\u{202e}{}", "z".repeat(5000));
    h.reply_with(200, SSE, failing(json!({"type":"invalid_request_error","message":m})));
    let (_, _, b) = h.post(R, &codex_request("after-custom-tool-call-output"));
    let evs = events(&b);
    let (last, d) = evs.last().unwrap();
    assert_eq!(last, "response.failed");
    let e = &d["response"]["error"];
    assert_eq!(e["code"], "context_length_exceeded");
    let msg = e["message"].as_str().unwrap();
    assert!(msg.chars().count() <= 512, "{} characters", msg.chars().count());
    assert!(!msg.contains('\u{1b}') && !msg.contains('\u{202e}'), "{msg:?}");
}

#[test] // review round 1 F5: the provider key never reaches the client in an in-stream error, whole or cut
fn in_stream_error_message_without_key() {
    let h = start(A);
    let key = h.provider_key().to_string();
    h.reply_with(200, SSE, failing(json!({"type":"authentication_error","message":format!("bad key {key}")})));
    let (_, _, b) = h.post(R, &codex_request("after-custom-tool-call-output"));
    let evs = events(&b);
    assert_eq!(evs.last().unwrap().1["response"]["error"], json!({"code":"authentication_error","message":"bad key [REDACTED]"}));
    // The key across the encoder's 64-character cut: no prefix of it either.
    h.reply_with(200, SSE, failing(json!({"type":"api_error","message":format!("{}{key}", "y".repeat(50))})));
    let (_, _, b) = h.post(R, &codex_request("after-custom-tool-call-output"));
    let text = String::from_utf8_lossy(&b);
    assert!(!text.contains(&key[..10]), "{text}");
    assert_eq!(events(&b).last().unwrap().0, "response.failed");
}

#[test] // review round 2 G3: an error event whose JSON spans two data lines never carries the provider key to the client
fn split_error_event_without_key() {
    let h = start(A);
    let key = h.provider_key().to_string();
    let mut body = msg_sse(&[message_start(), text_start(0), text_delta(0, "par")]);
    let e = format!("event: error\ndata: {{\"type\":\"error\",\ndata: \"error\":{{\"type\":\"api_error\",\"message\":\"bad {key}\"}}}}\n\n");
    body.extend_from_slice(e.as_bytes());
    h.reply_with(200, SSE, body);
    let (s, _, b) = h.post(R, &codex_request("after-custom-tool-call-output"));
    assert_eq!(s, 200);
    let text = String::from_utf8_lossy(&b);
    assert!(!text.contains(&key), "{text}");
    assert_eq!(events(&b).last().unwrap().0, "response.failed");
}

/// A Messages `error` event whose `data` is split over two lines after its `"type"` field.
fn split_error(error: &str) -> Vec<u8> {
    let mut body = msg_sse(&[message_start(), text_start(0), text_delta(0, "par")]);
    body.extend_from_slice(format!("event: error\ndata: {{\"type\":\"error\",\ndata: \"error\":{error}}}\n\n").as_bytes());
    body
}

#[test] // review round 3 H1: a split error event whose key has its first byte JSON-escaped never carries the key
fn split_error_event_escaped_key() {
    let h = start(A);
    let key = h.provider_key().to_string();
    let esc = format!("\\u0074{}", &key[1..]);
    h.reply_with(200, SSE, split_error(&format!("{{\"type\":\"api_error\",\"message\":\"bad {esc}\"}}")));
    let (s, _, b) = h.post(R, &codex_request("after-custom-tool-call-output"));
    assert_eq!(s, 200);
    let text = String::from_utf8_lossy(&b);
    assert!(!text.contains(&key[1..]), "{text}");
    let evs = events(&b);
    assert_eq!(terminals(&evs).len(), 1, "{:?}", names(&evs));
    let (last, d) = evs.last().unwrap();
    assert_eq!(last, "response.failed");
    assert_eq!(d["response"]["error"], json!({"code":"api_error","message":"bad [REDACTED]"}));
}

#[test] // review round 3 H2: a split `prompt is too long` event gets the PC14 code, the cap and the cleaning
fn split_prompt_too_long_pc14() {
    let h = start(A);
    let m = format!("prompt is too long: 1 tokens > 0 maximum \u{1b}[2J\u{202e}{}", "z".repeat(5000));
    h.reply_with(200, SSE, split_error(&json!({"type":"invalid_request_error","message":m}).to_string()));
    let (_, _, b) = h.post(R, &codex_request("after-custom-tool-call-output"));
    let evs = events(&b);
    assert_eq!(terminals(&evs).len(), 1, "{:?}", names(&evs));
    let (last, d) = evs.last().unwrap();
    assert_eq!(last, "response.failed");
    let e = &d["response"]["error"];
    assert_eq!(e["code"], "context_length_exceeded");
    let msg = e["message"].as_str().unwrap();
    assert!(msg.starts_with("prompt is too long: 1 tokens > 0 maximum"), "{msg:?}");
    assert!(msg.chars().count() <= 512, "{} characters", msg.chars().count());
    assert!(!msg.contains('\u{1b}') && !msg.contains('\u{202e}'), "{msg:?}");
}

#[test] // review round 3 H1: a held split error event over HOLD_MAX fails as the encoder fails an over-long event, with no echo of its text
fn split_error_event_over_hold_max() {
    let h = start(A);
    let part = "q".repeat(3 << 20);
    let mut body = msg_sse(&[message_start(), text_start(0), text_delta(0, "par")]);
    body.extend_from_slice(format!("event: error\ndata: {{\"type\":\"error\",\"pad\":\"{part}\",\ndata: \"more\":\"{part}\"}}\n\n").as_bytes());
    h.reply_with(200, SSE, body);
    let (s, _, b) = h.post(R, &codex_request("after-custom-tool-call-output"));
    assert_eq!(s, 200);
    assert!(!b.windows(64).any(|w| w == &part.as_bytes()[..64]), "held text echoed");
    let evs = events(&b);
    assert_eq!(terminals(&evs).len(), 1, "{:?}", names(&evs));
    let (last, d) = evs.last().unwrap();
    assert_eq!(last, "response.failed");
    assert_eq!(d["response"]["error"], json!({"code":"api_error","message":"upstream event too long"}));
}

#[test] // review round 3 H7, RS-9: an upstream call to a flat namespaced name returns with its name and namespace
fn namespaced_call_mapped_back() {
    let h = start(A);
    let body = codex_request("after-custom-tool-call-output");
    let flat = "multi_agent_v1__spawn_agent";
    let args = json!({"message":"go"});
    let want = |items: &[Value]| {
        let fc = items.iter().find(|i| i["type"] == "function_call").expect("a function_call item");
        assert_eq!((&fc["name"], &fc["namespace"]), (&json!("spawn_agent"), &json!("multi_agent_v1")), "{fc}");
        assert_eq!(serde_json::from_str::<Value>(fc["arguments"].as_str().unwrap()).unwrap(), args);
    };
    h.reply_with(200, SSE, msg_sse(&[
        message_start(),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_ns","name":flat,"input":{}}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":args.to_string()}}),
        stop(0),
        json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":42}}),
        json!({"type":"message_stop"}),
    ]));
    let (s, _, b) = h.post(R, &body);
    assert_eq!(s, 200, "{}", String::from_utf8_lossy(&b));
    let evs = events(&b);
    assert_eq!(terminals(&evs)[0].0, "response.completed");
    want(&done_items(&evs));

    h.reply_with(200, JSON, serde_json::to_vec(&json!({"id":"msg_up","type":"message","role":"assistant","model":"up-rs",
        "content":[{"type":"tool_use","id":"toolu_ns","name":flat,"input":args}],"stop_reason":"tool_use","stop_sequence":null,
        "usage":{"input_tokens":100,"cache_read_input_tokens":30,"cache_creation_input_tokens":5,"output_tokens":42}})).unwrap());
    let (s, _, b) = h.post(R, &with_stream(body, false));
    assert_eq!(s, 200, "{}", String::from_utf8_lossy(&b));
    let r: Value = serde_json::from_slice(&b).unwrap();
    want(r["output"].as_array().unwrap());
}

#[test] // review round 3 H7: a streamed request answered with a JSON 2xx ends with response.completed and its usage row
fn streamed_request_json_answer_completed_and_row() {
    let h = start(A);
    h.reply_with(200, JSON, serde_json::to_vec(&MSG_JSON(false)).unwrap());
    let (s, hd, b) = h.post(R, &with_stream(codex_request("after-custom-tool-call-output"), true));
    assert_eq!(s, 200, "{}", String::from_utf8_lossy(&b));
    assert!(hv(&hd, "content-type").starts_with(SSE), "{hd:?}");
    let evs = events(&b);
    let t = terminals(&evs);
    assert_eq!(t.len(), 1, "{:?}", names(&evs));
    assert_eq!(evs.last().unwrap().0, "response.completed");
    assert_eq!(t[0].1["response"]["usage"], msg_usage());
    let rows = h.usage_rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(counts(&rows[0]), (100, 30, 5, 42));
}

#[test] // RS-10: a JSON 2xx to a streamed request: message_events → encoder keeps redacted_thinking, drops server tools
fn streamed_request_json_answer_keeps_redacted_drops_server_tool() {
    let h = start(A);
    let red = json!({"type":"redacted_thinking","data":"EmwKAhgB"});
    let mut m = MSG_JSON(true);
    let content = m["content"].as_array_mut().unwrap();
    content.insert(0, red.clone());
    content.push(json!({"type":"web_search_tool_result","tool_use_id":"srvtoolu_1","content":[]}));
    h.reply_with(200, JSON, serde_json::to_vec(&m).unwrap());
    let (s, hd, b) = h.post(R, &with_stream(codex_request("after-custom-tool-call-output"), true));
    assert_eq!(s, 200, "{}", String::from_utf8_lossy(&b));
    assert!(hv(&hd, "content-type").starts_with(SSE), "{hd:?}");
    let evs = events(&b);
    assert_eq!(terminals(&evs).len(), 1, "{:?}", names(&evs));
    assert_eq!(evs.last().unwrap().0, "response.completed");
    let items = done_items(&evs);
    let types: Vec<&str> = items.iter().map(|i| i["type"].as_str().unwrap()).collect();
    assert_eq!(types, ["reasoning", "message", "function_call", "custom_tool_call"]);
    assert_eq!(items[0]["summary"], json!([]));
    let t = Target { provider: PROVIDER.into(), upstream_model: UPSTREAM_MODEL.into() };
    assert_eq!(decode_thinking(items[0]["encrypted_content"].as_str().unwrap(), &t), Some(red));
    assert_eq!(items[1]["content"][0]["text"], "hello");
    assert!(!evs.iter().any(|(_, d)| d.to_string().contains("srvtoolu_1")));
    let rows = h.usage_rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].adjustments.iter().filter(|a| *a == "server_tool_dropped").count(), 1, "{:?}", rows[0].adjustments);
}

/// The one terminal event of `evs`: `response.failed` `invalid_prompt` with the refusal text (RS-12, ending rfd).
fn assert_rfd(evs: &[(String, Value)]) {
    let t = terminals(evs);
    assert_eq!(t.len(), 1, "{:?}", names(evs));
    let (last, d) = evs.last().unwrap();
    assert_eq!(last, "response.failed", "{:?}", names(evs));
    assert_eq!(d["response"]["status"], "failed");
    assert_eq!(d["response"]["error"]["code"], "invalid_prompt");
    assert!(!names(evs).contains(&"response.completed"), "{:?}", names(evs));
}

#[test] // RS-12 round 1 review F1: chat content_filter after a tool call → response.failed invalid_prompt, call closed
fn chat_refusal_after_tool_call_fails() {
    let h = start(C);
    let mut chunks = CHAT_CHUNKS("{\"cmd\":");
    chunks[3] = chunk(json!({}), json!("content_filter"));
    h.reply_with(200, SSE, chat_sse(&chunks));
    let (s, _, b) = h.post(R, &codex_request("first-turn"));
    assert_eq!(s, 200, "{}", String::from_utf8_lossy(&b));
    let evs = events(&b);
    assert_rfd(&evs);
    let n = names(&evs);
    assert_eq!(n[n.len() - 3..n.len() - 1], ["response.function_call_arguments.done", "response.output_item.done"]);
    assert_eq!(done_items(&evs).last().unwrap()["arguments"], "{\"cmd\":");
}

#[test] // RS-12 round 1 review F1: a JSON 2xx with a tool_use and stop_reason refusal → response.failed, streamed or not
fn json_refusal_after_tool_call_fails() {
    let h = start(A);
    let mut m = MSG_JSON(false);
    m["stop_reason"] = json!("refusal");
    let req = codex_request("after-custom-tool-call-output");
    h.reply_with(200, JSON, serde_json::to_vec(&m).unwrap());
    let (s, hd, b) = h.post(R, &with_stream(req.clone(), true));
    assert_eq!(s, 200, "{}", String::from_utf8_lossy(&b));
    assert!(hv(&hd, "content-type").starts_with(SSE), "{hd:?}");
    let evs = events(&b);
    assert_rfd(&evs);
    let types: Vec<String> = done_items(&evs).iter().map(|i| i["type"].as_str().unwrap().to_owned()).collect();
    assert_eq!(types, ["message", "function_call", "custom_tool_call"]);

    h.reply_with(200, JSON, serde_json::to_vec(&m).unwrap());
    let (s, _, b) = h.post(R, &with_stream(req, false));
    assert_eq!(s, 200, "{}", String::from_utf8_lossy(&b));
    let r: Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(r["status"], "failed");
    assert_eq!(r["error"]["code"], "invalid_prompt");
}

#[test] // RS-13, K§6.4.3 stream obligation 8: a JSON 2xx whose tool_use input is 1.5 MiB of `"` reaches a streamed request whole
fn streamed_json_answer_keeps_large_tool_input() {
    let h = start(A);
    let mut m = MSG_JSON(false);
    m["content"] = json!([{"type":"tool_use","id":"toolu_1","name":"exec_command","input":{"a":"\"".repeat(3 << 19)}}]);
    let up = serde_json::to_vec(&m).unwrap();
    assert!(up.len() <= COPY_MAX, "{}", up.len());
    let req = codex_request("after-custom-tool-call-output");

    h.reply_with(200, JSON, up.clone());
    let (s, _, b) = h.post(R, &with_stream(req.clone(), false));
    assert_eq!(s, 200, "{}", String::from_utf8_lossy(&b[..b.len().min(512)]));
    let r: Value = serde_json::from_slice(&b).unwrap();
    let fc = r["output"].as_array().unwrap().iter().find(|i| i["type"] == "function_call").expect("a function_call item");
    let want = fc["arguments"].clone();
    assert!(want.as_str().is_some_and(|a| a.len() > 3 << 20), "{}", want.as_str().map_or(0, str::len));

    h.reply_with(200, JSON, up);
    let (s, hd, b) = h.post(R, &with_stream(req, true));
    assert_eq!(s, 200);
    assert!(hv(&hd, "content-type").starts_with(SSE), "{hd:?}");
    let evs = events(&b);
    assert_eq!(terminals(&evs).len(), 1, "{:?}", names(&evs));
    assert_eq!(evs.last().unwrap().0, "response.completed", "{:?}", terminals(&evs));
    let items = done_items(&evs);
    let got = items.iter().find(|i| i["type"] == "function_call").expect("a function_call item");
    assert_eq!(got["arguments"], want);
}

#[test] // RS-13 round 1 review F1, K§6.4.3 stream obligation 8: a JSON 2xx of `"` as close to COPY_MAX as fits reaches a streamed request whole
fn streamed_json_answer_near_copy_max_keeps_tool_input() {
    let h = start(A);
    let body = |n: usize| {
        let mut m = MSG_JSON(false);
        m["content"] = json!([{"type":"tool_use","id":"toolu_1","name":"exec_command","input":{"a":"\"".repeat(n)}}]);
        serde_json::to_vec(&m).unwrap()
    };
    let base = body(0).len();
    let up = body((COPY_MAX - base) / 2);
    assert!(up.len() <= COPY_MAX && up.len() + 2 > COPY_MAX, "{}", up.len());
    let req = codex_request("after-custom-tool-call-output");

    h.reply_with(200, JSON, up.clone());
    let (s, _, b) = h.post(R, &with_stream(req.clone(), false));
    assert_eq!(s, 200, "{}", String::from_utf8_lossy(&b[..b.len().min(512)]));
    let r: Value = serde_json::from_slice(&b).unwrap();
    let fc = r["output"].as_array().unwrap().iter().find(|i| i["type"] == "function_call").expect("a function_call item");
    let want = fc["arguments"].clone();

    h.reply_with(200, JSON, up);
    let (s, _, b) = h.post(R, &with_stream(req, true));
    assert_eq!(s, 200);
    let evs = events(&b);
    assert_eq!(terminals(&evs).len(), 1, "{:?}", names(&evs));
    assert_eq!(evs.last().unwrap().0, "response.completed", "{:?}", terminals(&evs));
    let items = done_items(&evs);
    let got = items.iter().find(|i| i["type"] == "function_call").expect("a function_call item");
    assert_eq!(got["arguments"], want);
}

#[test] // RS-15, K§6.4.3 stream obligation 8: a restored JSON text block of a chat upstream goes out in 64 KiB pieces
fn streamed_chat_restore_keeps_large_text() {
    let h = start(C);
    // 3 MiB of `\"` in `b`: the held text is within HOLD_MAX; one rebuilt delta of it would escape to about 6 MiB.
    let b = "\"".repeat(3 << 19);
    let text = json!({"a":null,"b":b}).to_string();
    let want = json!({"b":b}).to_string();
    let mut chunks: Vec<Value> = text.as_bytes().chunks(60_000).map(|p| chunk(json!({"content":std::str::from_utf8(p).unwrap()}), Value::Null)).collect();
    chunks.push(chunk(json!({}), json!("stop")));
    h.reply_with(200, SSE, chat_sse(&chunks));
    let mut req = codex_request("first-turn");
    req["text"]["format"] = json!({"type":"json_schema","name":"out","schema":{"type":"object",
        "properties":{"a":{"type":"string"},"b":{"type":"string"}},"required":["b"],"additionalProperties":false}});
    let (s, _, body) = h.post(R, &req);
    assert_eq!(s, 200, "{}", String::from_utf8_lossy(&body[..body.len().min(512)]));
    // The null filter is armed on the output schema: `a` was made required (and nullable) upstream.
    let up = h.upstream_bodies().pop().unwrap();
    assert_eq!(up["response_format"]["json_schema"]["schema"]["required"], json!(["b", "a"]), "{}", up["response_format"]);
    let evs = events(&body);
    assert_eq!(terminals(&evs).len(), 1, "{:?}", names(&evs));
    assert_eq!(evs.last().unwrap().0, "response.completed", "{:?}", terminals(&evs));
    let items = done_items(&evs);
    let msg = items.iter().find(|i| i["type"] == "message").expect("a message item");
    assert_eq!(msg["content"][0]["text"], json!(want));
}

#[test] // RS-15, K§6.4.3 stream obligation 8: a JSON 2xx whose text is `"` as close to COPY_MAX as fits reaches a streamed request whole
fn streamed_json_answer_keeps_large_text() {
    let h = start(A);
    let body = |n: usize| {
        let mut m = MSG_JSON(false);
        m["content"] = json!([{"type":"text","text":"\"".repeat(n)}]);
        m["stop_reason"] = json!("end_turn");
        serde_json::to_vec(&m).unwrap()
    };
    let base = body(0).len();
    let n = (COPY_MAX - base) / 2;
    let up = body(n);
    assert!(up.len() <= COPY_MAX && up.len() + 2 > COPY_MAX, "{}", up.len());
    let req = codex_request("after-custom-tool-call-output");

    h.reply_with(200, JSON, up.clone());
    let (s, _, b) = h.post(R, &with_stream(req.clone(), false));
    assert_eq!(s, 200, "{}", String::from_utf8_lossy(&b[..b.len().min(512)]));
    let r: Value = serde_json::from_slice(&b).unwrap();
    let msg = r["output"].as_array().unwrap().iter().find(|i| i["type"] == "message").expect("a message item");
    let want = msg["content"][0]["text"].clone();
    assert_eq!(want.as_str().map(str::len), Some(n));

    h.reply_with(200, JSON, up);
    let (s, _, b) = h.post(R, &with_stream(req, true));
    assert_eq!(s, 200);
    let evs = events(&b);
    assert_eq!(terminals(&evs).len(), 1, "{:?}", names(&evs));
    assert_eq!(evs.last().unwrap().0, "response.completed", "{:?}", terminals(&evs));
    let items = done_items(&evs);
    let got = items.iter().find(|i| i["type"] == "message").expect("a message item");
    assert_eq!(got["content"][0]["text"], want);
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
