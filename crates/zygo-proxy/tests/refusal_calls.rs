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
use zygo_proxy::translate::sse::INPUT_DELTA_MAX;
use zygo_proxy::translate::{StreamMapper, Target};

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

/// The `refusal_call_dropped:` codes of the last usage row.
fn row_codes(h: &Harness) -> Vec<String> {
    let rows = h.usage_rows();
    let a = &rows.last().expect("a usage row").adjustments;
    a.iter().filter(|x| x.starts_with("refusal_call_dropped:")).cloned().collect()
}

/// The last usage row holds `refusal_call_dropped:1` exactly once, and no other `refusal_call_dropped:` code.
fn assert_row_code(h: &Harness) {
    assert_eq!(row_codes(h), ["refusal_call_dropped:1"]);
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
    let mut evs = responses_open_call();
    evs.push(last);
    sse(&evs)
}

/// The events of `responses_cut_call` before `last`.
fn responses_open_call() -> Vec<Value> {
    vec![
        json!({"type":"response.created","sequence_number":0,"response":{"id":"resp_up","object":"response","model":"up-rs","status":"in_progress","output":[],"usage":null}}),
        json!({"type":"response.output_item.added","sequence_number":1,"output_index":0,"item":{"type":"message","id":"msg_up","role":"assistant","content":[]}}),
        json!({"type":"response.content_part.added","sequence_number":2,"output_index":0,"content_index":0,"item_id":"msg_up","part":{"type":"output_text","text":""}}),
        json!({"type":"response.output_text.delta","sequence_number":3,"output_index":0,"content_index":0,"item_id":"msg_up","delta":"hel"}),
        json!({"type":"response.output_item.done","sequence_number":4,"output_index":0,"item":{"type":"message","id":"msg_up","role":"assistant","content":[{"type":"output_text","text":"hel"}]}}),
        json!({"type":"response.output_item.added","sequence_number":5,"output_index":1,"item":{"type":"function_call","id":"fc_up","call_id":"call_1","name":"exec_command","arguments":""}}),
        json!({"type":"response.function_call_arguments.delta","sequence_number":6,"output_index":1,"item_id":"fc_up","delta":CUT}),
    ]
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

/// A Responses mapper for a Messages client, as `forward.rs` builds it.
fn mapper() -> StreamMapper {
    StreamMapper::new(O, &Target { provider: "p-oai".into(), upstream_model: "up-oai".into() }).calls_dropped_key(false)
}

/// `response.created` and a `function_call` item `call_1` named `name` at output index 0.
fn open_call(name: &str) -> Vec<Value> {
    vec![
        json!({"type":"response.created","sequence_number":0,"response":{"id":"resp_up","object":"response","model":"up-rs","status":"in_progress","output":[],"usage":null}}),
        json!({"type":"response.output_item.added","sequence_number":1,"output_index":0,
               "item":{"type":"function_call","id":"fc_up","call_id":"call_1","name":name,"arguments":""}}),
    ]
}

fn arg_delta(d: &str) -> Value {
    json!({"type":"response.function_call_arguments.delta","output_index":0,"item_id":"fc_up","delta":d})
}

#[test] // F1: empty argument deltas add nothing to a held call: the held bytes and the pending call stay flat
fn responses_held_call_empty_deltas_stay_flat() {
    let mut m = mapper();
    let first = m.push(&sse(&open_call("exec_command")));
    assert_eq!(first.iter().map(|e| e.event.as_str()).collect::<Vec<_>>(), ["message_start"]);
    let (base, pending) = m.held_state();
    assert_eq!(pending, 1);
    let empty: Vec<Value> = (0..100_000).map(|_| arg_delta("")).collect();
    assert!(m.push(&sse(&empty)).is_empty());
    // No argument byte is held: the count is the call's fixed cost alone, and one call waits, not 100,000 events.
    assert_eq!(m.held_state(), (base, 1));
    let out = m.push(&sse(&[
        json!({"type":"response.output_item.done","output_index":0,"item":{"type":"function_call","call_id":"call_1","name":"exec_command","arguments":""}}),
        json!({"type":"response.completed","response":{"status":"completed","usage":{"input_tokens":7,"output_tokens":3}}}),
    ]));
    let names: Vec<&str> = out.iter().map(|e| e.event.as_str()).collect();
    assert_eq!(names, ["content_block_start", "content_block_stop", "message_delta", "message_stop"]);
    assert_eq!(m.held_state(), (0, 0));
}

#[test] // F1: many 1-byte argument deltas that take a held call past HOLD_MAX end the stream; nothing of the call is sent
fn responses_held_call_one_byte_deltas_over_hold_max_errors() {
    // The name holds all but 10,000 bytes of HOLD_MAX, so that 1-byte deltas cross the bound in a few thousand events.
    let name = "n".repeat(HOLD_MAX - 10_000);
    let mut evs = open_call(&name);
    evs.extend((0..20_000).map(|_| arg_delta("a")));
    let mut m = mapper();
    let out = m.push(&sse(&evs));
    let names: Vec<&str> = out.iter().map(|e| e.event.as_str()).collect();
    assert_eq!(names, ["message_start", "error"]);
    assert_eq!(out[1].data["error"]["message"], "upstream tool call too long");
    assert!(m.ended());
}

#[test] // F2: a JSON body at content_filter keeps a completed call and drops a call still in progress
fn messages_client_responses_upstream_content_filter_json_keeps_completed_call() {
    let h = start(O);
    h.reply_with(
        200,
        JSON,
        serde_json::to_vec(&json!({"id":"resp_up","object":"response","model":"up-rs","status":"incomplete",
            "incomplete_details":{"reason":"content_filter"},
            "output":[
                {"type":"function_call","id":"fc_0","call_id":"call_0","name":"exec_command","arguments":"{\"cmd\":\"ls\"}","status":"completed"},
                {"type":"function_call","id":"fc_up","call_id":"call_1","name":"exec_command","arguments":CUT,"status":"in_progress"}
            ],
            "usage":{"input_tokens":7,"output_tokens":3,"total_tokens":10}}))
        .unwrap(),
    );
    let (s, _, b) = h.post(M, &client_body(false));
    let text = String::from_utf8_lossy(&b);
    assert_eq!(s, 200, "{text}");
    assert!(!contains(&b, KEY) && !contains(&b, b"call_1"), "{text}");
    let v: Value = serde_json::from_slice(&b).unwrap();
    assert_eq!(v["stop_reason"], "refusal", "{text}");
    assert_eq!(v["content"], json!([{"type":"tool_use","id":"call_0","name":"exec_command","input":{"cmd":"ls"}}]), "{text}");
    assert_row_code(&h);
}

/// A Messages SSE body that ends in an `error` event with `message`, after the text "hel", with nothing of `call_1`.
fn assert_stream_error(b: &[u8], message: &str) {
    let text = String::from_utf8_lossy(b);
    assert!(contains(b, b"\"hel\""), "{text}");
    assert!(!contains(b, b"tool_use") && !contains(b, b"input_json_delta") && !contains(b, b"call_1"), "{text}");
    let evs = events(b);
    let (name, data) = evs.last().expect("an event");
    assert_eq!((name.as_str(), &data["error"]["message"]), ("error", &json!(message)), "{text}");
}

#[test] // F4: an upstream error event while a call is held: the call is not sent, then the error
fn messages_client_responses_upstream_error_while_held_drops_call() {
    let h = start(O);
    h.reply_with(200, SSE, responses_cut_call(json!({"type":"error","sequence_number":7,"code":"server_error","message":"boom"})));
    let (s, _, b) = h.post(M, &client_body(true));
    assert_eq!(s, 200, "{}", String::from_utf8_lossy(&b));
    assert_stream_error(&b, "boom");
}

#[test] // F4: the upstream stream ends while a call is held: the call is not sent, then the error
fn messages_client_responses_upstream_early_end_while_held_drops_call() {
    let h = start(O);
    h.reply_with(200, SSE, sse(&responses_open_call()));
    let (s, _, b) = h.post(M, &client_body(true));
    assert_eq!(s, 200, "{}", String::from_utf8_lossy(&b));
    assert_stream_error(&b, "upstream stream ended early");
}

#[test] // F4: a call closed by its output_item.done before content_filter stays sent, whole
fn messages_client_responses_upstream_closed_call_before_content_filter_is_sent() {
    let h = start(O);
    let mut evs = responses_open_call();
    evs.push(json!({"type":"response.function_call_arguments.delta","sequence_number":7,"output_index":1,"item_id":"fc_up","delta":"\"ls\"}"}));
    evs.push(json!({"type":"response.output_item.done","sequence_number":8,"output_index":1,
        "item":{"type":"function_call","id":"fc_up","call_id":"call_1","name":"exec_command","arguments":"{\"cmd\":\"ls\"}","status":"completed"}}));
    evs.push(json!({"type":"response.incomplete","sequence_number":9,
        "response":{"id":"resp_up","object":"response","model":"up-rs","status":"incomplete",
        "incomplete_details":{"reason":"content_filter"},"output":[],"usage":{"input_tokens":7,"output_tokens":3,"total_tokens":10}}}));
    h.reply_with(200, SSE, sse(&evs));
    let (s, _, b) = h.post(M, &client_body(true));
    let text = String::from_utf8_lossy(&b);
    assert_eq!(s, 200, "{text}");
    assert!(!contains(&b, KEY), "{text}");
    let evs = events(&b);
    let start = evs.iter().find(|(n, d)| n == "content_block_start" && d["content_block"]["type"] == "tool_use").expect("a tool_use block");
    assert_eq!(start.1["content_block"]["id"], "call_1", "{text}");
    let args: String = evs
        .iter()
        .filter(|(_, d)| d["delta"]["type"] == "input_json_delta")
        .map(|(_, d)| d["delta"]["partial_json"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(args, "{\"cmd\":\"ls\"}", "{text}");
    let md = evs.iter().find(|(n, _)| n == "message_delta").expect("a message_delta");
    assert_eq!(md.1["delta"]["stop_reason"], "refusal", "{text}");
    assert!(row_codes(&h).is_empty(), "{:?}", row_codes(&h));
}

/// `output_item.done` for the `function_call` item `call_1` at output index `oi`, with `args` and `status` (none when
/// `None`).
fn call_done(oi: u64, args: &str, status: Option<&str>) -> Value {
    let mut item = json!({"type":"function_call","id":"fc_up","call_id":"call_1","name":"exec_command","arguments":args});
    if let Some(st) = status {
        item["status"] = json!(st);
    }
    json!({"type":"response.output_item.done","output_index":oi,"item":item})
}

fn incomplete_content_filter() -> Value {
    json!({"type":"response.incomplete","response":{"status":"incomplete","incomplete_details":{"reason":"content_filter"},
        "usage":{"input_tokens":7,"output_tokens":3}}})
}

fn completed() -> Value {
    json!({"type":"response.completed","response":{"status":"completed","usage":{"input_tokens":7,"output_tokens":3}}})
}

fn names(out: &[zygo_proxy::translate::SseEvent]) -> Vec<&str> {
    out.iter().map(|e| e.event.as_str()).collect()
}

/// The `tool_use` content_block_start blocks of `out`.
fn tool_starts(out: &[zygo_proxy::translate::SseEvent]) -> Vec<Value> {
    out.iter()
        .filter(|e| e.event == "content_block_start" && e.data["content_block"]["type"] == "tool_use")
        .map(|e| e.data["content_block"].clone())
        .collect()
}

/// The `partial_json` pieces of `out`'s input_json_delta events.
fn json_pieces(out: &[zygo_proxy::translate::SseEvent]) -> Vec<String> {
    out.iter()
        .filter(|e| e.data["delta"]["type"] == "input_json_delta")
        .map(|e| e.data["delta"]["partial_json"].as_str().unwrap().to_owned())
        .collect()
}

/// G1: a call done with status `status` (not `completed`), then content_filter: the call stays held, then is dropped
/// and counted; no tool_use is sent.
fn assert_not_completed_done_dropped(status: Option<&str>) {
    let mut m = mapper();
    let mut out = m.push(&sse(&open_call("exec_command")));
    out.extend(m.push(&sse(&[arg_delta(CUT)])));
    let done = m.push(&sse(&[call_done(0, CUT, status)]));
    assert!(done.is_empty(), "{status:?}: {done:?}");
    assert_eq!(m.held_state().1, 1, "{status:?}");
    out.extend(m.push(&sse(&[incomplete_content_filter()])));
    assert_eq!(names(&out), ["message_start", "message_delta", "message_stop"], "{status:?}");
    assert_eq!(out[1].data["delta"]["stop_reason"], "refusal");
    assert!(tool_starts(&out).is_empty() && json_pieces(&out).is_empty(), "{status:?}");
    assert_eq!(m.calls_dropped(), 1, "{status:?}");
}

#[test] // round 2 review G1: done with status `incomplete`, then content_filter: dropped and counted, as the JSON body does
fn responses_call_done_incomplete_then_content_filter_is_dropped() {
    assert_not_completed_done_dropped(Some("incomplete"));
}

#[test] // round 2 review G1: done with no status, then content_filter: dropped and counted
fn responses_call_done_without_status_then_content_filter_is_dropped() {
    assert_not_completed_done_dropped(None);
}

#[test] // round 2 review G1: done with status `incomplete` (arguments only in the done), then a normal end: sent, whole
fn responses_call_done_incomplete_then_normal_end_is_sent() {
    let mut m = mapper();
    let mut out = m.push(&sse(&open_call("exec_command")));
    out.extend(m.push(&sse(&[call_done(0, "{\"cmd\":\"ls\"}", Some("incomplete")), completed()])));
    let starts = tool_starts(&out);
    assert_eq!(starts.len(), 1, "{out:?}");
    assert_eq!((&starts[0]["id"], &starts[0]["name"]), (&json!("call_1"), &json!("exec_command")));
    assert_eq!(json_pieces(&out).concat(), "{\"cmd\":\"ls\"}");
    assert_eq!(m.calls_dropped(), 0);
    assert_eq!(names(&out).last(), Some(&"message_stop"));
}

#[test] // round 2 review G2: a done and a delta for a call already sent (another item started) emit nothing
fn responses_late_done_and_delta_for_sent_call_are_ignored() {
    let mut m = mapper();
    let mut evs = open_call("exec_command");
    evs.push(arg_delta("{\"cmd\":"));
    evs.push(json!({"type":"response.output_item.added","output_index":1,"item":{"type":"message","id":"msg_up","role":"assistant","content":[]}}));
    evs.push(json!({"type":"response.content_part.added","output_index":1,"content_index":0,"part":{"type":"output_text","text":""}}));
    evs.push(json!({"type":"response.output_text.delta","output_index":1,"content_index":0,"delta":"hel"}));
    evs.push(call_done(0, "{\"cmd\":\"ls\"}", Some("completed")));
    evs.push(arg_delta("\"ls\"}"));
    evs.push(completed());
    let out = m.push(&sse(&evs));
    let starts = tool_starts(&out);
    assert_eq!(starts.len(), 1, "{out:?}");
    assert_eq!((&starts[0]["id"], &starts[0]["name"]), (&json!("call_1"), &json!("exec_command")));
    assert_eq!(json_pieces(&out).concat(), "{\"cmd\":");
    assert_eq!(names(&out).last(), Some(&"message_stop"));
}

#[test] // round 2 review G3: empty deltas, then the arguments only in output_item.done: the block carries them
fn responses_empty_deltas_then_done_arguments_are_sent() {
    let mut m = mapper();
    let mut evs = open_call("exec_command");
    evs.extend((0..3).map(|_| arg_delta("")));
    evs.push(call_done(0, "{\"cmd\":\"ls\"}", Some("completed")));
    evs.push(completed());
    let out = m.push(&sse(&evs));
    assert_eq!(tool_starts(&out).len(), 1, "{out:?}");
    assert_eq!(json_pieces(&out).concat(), "{\"cmd\":\"ls\"}");
}

#[test] // round 2 review G4, K§6.4.3 stream obligation 8: held arguments over 64 KiB go out in pieces cut on a char boundary
fn responses_held_call_long_arguments_cut_on_char_boundary() {
    // `é` (2 bytes) starts at byte INPUT_DELTA_MAX - 1, so that a byte cut at INPUT_DELTA_MAX would split it.
    let args = format!("{}é{}", "a".repeat(INPUT_DELTA_MAX - 1), "b".repeat(INPUT_DELTA_MAX));
    let mut m = mapper();
    let mut evs = open_call("exec_command");
    evs.push(arg_delta(&args));
    evs.push(call_done(0, &args, Some("completed")));
    evs.push(completed());
    let out = m.push(&sse(&evs));
    let pieces = json_pieces(&out);
    assert!(pieces.len() > 1, "{}", pieces.len());
    assert_eq!(pieces.concat(), args);
    assert_eq!(pieces[0].len(), INPUT_DELTA_MAX - 1);
    assert!(pieces.iter().all(|p| !p.is_empty() && p.len() <= INPUT_DELTA_MAX));
    // Each piece is a whole `str` of `args` at a char boundary.
    let mut at = 0;
    for p in &pieces {
        assert!(args.is_char_boundary(at) && args.is_char_boundary(at + p.len()), "{at}");
        at += p.len();
    }
}
