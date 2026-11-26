//! RS-7: the soft-window rewrite for a Responses client (K§6.4.5, K§11 item 21): armed, the client's
//! `response.usage` follows §10.2.2 for an Anthropic or chat upstream (rewrite on the Messages `message_delta` before
//! the encoder) and for a Responses upstream (rewrite on `response.completed`); unarmed, the bytes pass unchanged.
//! Mock upstream on 127.0.0.1; no agent binary, no network.

#[path = "support/responses.rs"]
mod support;

use serde_json::{json, Value};
use support::{codex_request, Harness};
use zygo_config::Dialect;
use zygo_core::Agent;
use zygo_proxy::tee::Usage;
use zygo_proxy::window::rewrite_responses_usage;
use zygo_usage::RequestRow;

const R: &str = "openai/v1/responses";
const SSE: &str = "text/event-stream";

// ---- helpers (no logic under test) ----

/// `event: <type>\ndata: <compact json>\n\n` per event.
fn sse(evs: &[Value]) -> Vec<u8> {
    evs.iter().flat_map(|e| format!("event: {}\ndata: {e}\n\n", e["type"].as_str().unwrap()).into_bytes()).collect()
}

/// (input, cache read, cache write, output) of a usage row.
fn counts(r: &RequestRow) -> (u64, u64, u64, u64) {
    (r.input_tokens, r.cache_read_tokens, r.cache_write_tokens, r.output_tokens)
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

fn terminals(evs: &[(String, Value)]) -> Vec<&(String, Value)> {
    evs.iter().filter(|(n, _)| matches!(n.as_str(), "response.completed" | "response.incomplete" | "response.failed")).collect()
}

fn chunk(delta: Value, finish: Value) -> Value {
    json!({"id":"chatcmpl-1","object":"chat.completion.chunk","model":"up-rs",
        "choices":[{"index":0,"delta":delta,"finish_reason":finish}]})
}

/// The client's usage after a rewrite to window 1000000 with upstream input 40000, cached 150000, output 30.
fn rewritten() -> Value {
    json!({"input_tokens":1000000,"input_tokens_details":{"cached_tokens":960000},"output_tokens":30,"total_tokens":1000030})
}

/// The `context_*` adjustments of a row.
fn codes(r: &RequestRow) -> Vec<&str> {
    r.adjustments.iter().map(String::as_str).filter(|a| a.starts_with("context_")).collect()
}

/// Anthropic SSE: message_start with `start_usage`; one text block "hi", or one tool_use block named `tool` when
/// `Some`; message_delta {"stop_reason": stop} with usage {"output_tokens":30}; message_stop.
fn anthropic_stream(start_usage: Value, tool: Option<&str>, stop: &str) -> Vec<u8> {
    let block = match tool {
        Some(name) => json!({"type":"tool_use","id":"toolu_1","name":name,"input":{}}),
        None => json!({"type":"text","text":""}),
    };
    let mut evs = vec![
        json!({"type":"message_start","message":{"id":"msg_up","type":"message","role":"assistant","model":"up-rs",
            "content":[],"usage":start_usage}}),
        json!({"type":"content_block_start","index":0,"content_block":block}),
    ];
    if tool.is_none() {
        evs.push(json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hi"}}));
    }
    evs.push(json!({"type":"content_block_stop","index":0}));
    evs.push(json!({"type":"message_delta","delta":{"stop_reason":stop,"stop_sequence":null},"usage":{"output_tokens":30}}));
    evs.push(json!({"type":"message_stop"}));
    sse(&evs)
}

/// `sse(&resp_events(terminal, status, usage))`, with each item of `calls` sent as one `response.output_item.added`
/// before the terminal event and appended to the terminal `response.output`.
fn responses_stream(terminal: &str, status: &str, calls: Vec<Value>, usage: Value) -> Vec<u8> {
    let mut evs = resp_events(terminal, status, usage);
    let mut last = evs.pop().unwrap();
    for (i, c) in calls.into_iter().enumerate() {
        let n = i as u64 + 1;
        evs.push(json!({"type":"response.output_item.added","sequence_number":6 + n,"output_index":n,"item":c}));
        last["response"]["output"].as_array_mut().unwrap().push(c);
    }
    evs.push(last);
    sse(&evs)
}

/// `sse(&resp_events("response.completed", "completed", usage))` with an empty terminal `response.output`; with
/// `Some((event, item))`, `item` sent as one `event` (`response.output_item.added` or `.done`) before the terminal event.
fn streamed_only(call: Option<(&str, Value)>, usage: Value) -> Vec<u8> {
    let mut evs = resp_events("response.completed", "completed", usage);
    let mut last = evs.pop().unwrap();
    last["response"]["output"] = json!([]);
    if let Some((ev, item)) = call {
        evs.push(json!({"type":ev,"sequence_number":6,"output_index":1,"item":item}));
        last["sequence_number"] = json!(7);
    }
    evs.push(last);
    sse(&evs)
}

/// Rows of the session once there are `n`. A missing row hangs the test (no timing assertion).
fn rows_when(h: &Harness, n: usize) -> Vec<RequestRow> {
    loop {
        let rows = h.usage_rows();
        if rows.len() >= n {
            return rows;
        }
    }
}

/// `authorization: Bearer <token>` and `x-zygo-session: <session>`, then `extra`.
fn auth<'a>(h: &'a Harness, bearer: &'a str, extra: &[(&'a str, &'a str)]) -> Vec<(&'a str, &'a str)> {
    let mut v = vec![("authorization", bearer), ("x-zygo-session", h.session_id())];
    v.extend_from_slice(extra);
    v
}

fn usage_up() -> Value {
    json!({"input_tokens":190000,"input_tokens_details":{"cached_tokens":150000},"output_tokens":30,"output_tokens_details":{"reasoning_tokens":10},"total_tokens":190030})
}

fn usage_down() -> Value {
    json!({"input_tokens":1000000,"input_tokens_details":{"cached_tokens":960000},"output_tokens":30,"output_tokens_details":{"reasoning_tokens":10},"total_tokens":1000030})
}

/// The terminal event's `response.usage` of a client stream (exactly one terminal event).
fn terminal_usage(b: &[u8]) -> Value {
    let evs = events(b);
    let t = terminals(&evs);
    assert_eq!(t.len(), 1, "{}", String::from_utf8_lossy(b));
    t[0].1["response"]["usage"].clone()
}

// ---- tests ----

#[test]
fn rewrite_responses_usage_sets_three_numbers_only() {
    let u = Usage { input: 40000, output: 30, cache_read: 150000, cache_write: 0 };
    let line = |ty: &str, usage: &str| {
        format!(r#"data: {{"type":"{ty}","sequence_number":6,"response":{{"id":"resp_up","status":"completed","output":[],"usage":{usage}}}}}"#)
    };
    let (up, down) = (usage_up().to_string(), usage_down().to_string());
    for ty in ["response.completed", "response.incomplete"] {
        assert_eq!(rewrite_responses_usage(line(ty, &up).as_bytes(), u, 1_000_000), Some(line(ty, &down).into_bytes()), "{ty}");
    }
    for ty in ["response.failed", "response.created", "message_delta"] {
        assert_eq!(rewrite_responses_usage(line(ty, &up).as_bytes(), u, 1_000_000), None, "{ty}");
    }
    for usage in [
        "null",
        r#"{"input_tokens":1,"input_tokens":2}"#,
        r#"{"input_tokens":1,"input_tokens_details":7}"#,
        r#"{"input_tokens":1,"input_tokens_details":{"cached_tokens":1,"cached_tokens":2}}"#,
    ] {
        assert_eq!(rewrite_responses_usage(line("response.completed", usage).as_bytes(), u, 1_000_000), None, "{usage}");
    }
    let bare = line("response.completed", &up);
    assert_eq!(rewrite_responses_usage(bare.trim_start_matches("data: ").as_bytes(), u, 1_000_000), None);
    // Missing fields are inserted; the order of the members is not fixed.
    let usage_of = |b: Vec<u8>| serde_json::from_slice::<Value>(&b[5..]).unwrap()["response"]["usage"].clone();
    let u2 = Usage { input: 190000, output: 30, cache_read: 0, cache_write: 0 };
    let no_details = r#"{"input_tokens":190000,"output_tokens":30,"total_tokens":190030}"#;
    assert_eq!(
        usage_of(rewrite_responses_usage(line("response.completed", no_details).as_bytes(), u2, 1_000_000).unwrap()),
        json!({"input_tokens":1000000,"input_tokens_details":{"cached_tokens":810000},"output_tokens":30,"total_tokens":1000030})
    );
    assert_eq!(
        usage_of(rewrite_responses_usage(line("response.completed", "{}").as_bytes(), u2, 1_000_000).unwrap()),
        json!({"input_tokens":1000000,"input_tokens_details":{"cached_tokens":810000},"total_tokens":1000030})
    );
}

#[test]
fn armed_anthropic_and_chat_upstreams_rewrite_before_the_encoder() {
    for d in [Dialect::Anthropic, Dialect::OpenaiChat] {
        let h = Harness::start(d, Agent::ClaudeCode, "");
        let reply = match d {
            Dialect::Anthropic => anthropic_stream(
                json!({"input_tokens":40000,"cache_read_input_tokens":150000,"cache_creation_input_tokens":0,"output_tokens":1}),
                None,
                "end_turn",
            ),
            _ => chat_sse(&[
                chunk(json!({"role":"assistant","content":"hi"}), Value::Null),
                chunk(json!({}), json!("stop")),
                json!({"id":"chatcmpl-1","object":"chat.completion.chunk","model":"up-rs","choices":[],
                    "usage":{"prompt_tokens":190000,"completion_tokens":30,"total_tokens":190030,"prompt_tokens_details":{"cached_tokens":150000}}}),
            ]),
        };
        h.reply_with(200, SSE, reply);
        let (s, _, b) = h.post(R, &codex_request("turn-with-reasoning"));
        assert_eq!(s.as_u16(), 200, "{d:?}");
        let evs = events(&b);
        let t = terminals(&evs);
        assert_eq!(t.len(), 1, "{d:?}");
        assert_eq!(t[0].0, "response.completed", "{d:?}");
        assert_eq!(t[0].1["response"]["usage"], rewritten(), "{d:?}");
        let rows = h.usage_rows();
        assert_eq!(counts(&rows[0]), (40000, 150000, 0, 30), "{d:?}"); // upstream counts, before the rewrite
        assert_eq!(codes(&rows[0]), ["context_safety"], "{d:?}");
    }
}

#[test]
fn armed_responses_upstream_rewrites_completed_usage() {
    let h = Harness::start(Dialect::Openai, Agent::ClaudeCode, "");
    let up = responses_stream("response.completed", "completed", vec![], usage_up());
    h.reply_with(200, SSE, up.clone());
    let (s, _, b) = h.post(R, &codex_request("turn-with-reasoning"));
    assert_eq!(s.as_u16(), 200);
    let want = String::from_utf8(up).unwrap().replace(&usage_up().to_string(), &usage_down().to_string());
    assert_eq!(String::from_utf8(b.to_vec()).unwrap(), want);
    let rows = h.usage_rows();
    assert_eq!(counts(&rows[0]), (40000, 150000, 0, 30));
    assert_eq!(codes(&rows[0]), ["context_safety"]);
}

#[test]
fn unarmed_bytes_pass_unchanged() {
    let up = responses_stream("response.completed", "completed", vec![], usage_up());
    // (a) Not Claude Code, Responses upstream.
    let h = Harness::start(Dialect::Openai, Agent::OpenCode, "");
    h.reply_with(200, SSE, up.clone());
    let (s, _, b) = h.post(R, &codex_request("turn-with-reasoning"));
    assert_eq!(s.as_u16(), 200);
    assert_eq!(&b[..], &up[..]);
    assert!(codes(&h.usage_rows()[0]).is_empty());
    // (b) Not Claude Code, Anthropic upstream.
    let h = Harness::start(Dialect::Anthropic, Agent::OpenCode, "");
    h.reply_with(
        200,
        SSE,
        anthropic_stream(
            json!({"input_tokens":40000,"cache_read_input_tokens":150000,"cache_creation_input_tokens":0,"output_tokens":1}),
            None,
            "end_turn",
        ),
    );
    let (s, _, b) = h.post(R, &codex_request("turn-with-reasoning"));
    assert_eq!(s.as_u16(), 200);
    assert_eq!(
        terminal_usage(&b),
        json!({"input_tokens":190000,"input_tokens_details":{"cached_tokens":150000},"output_tokens":30,"total_tokens":190030})
    );
    assert!(codes(&h.usage_rows()[0]).is_empty());
    // (c) Claude Code, a compaction request.
    let h = Harness::start(Dialect::Openai, Agent::ClaudeCode, "");
    h.reply_with(200, SSE, up.clone());
    let bearer = format!("Bearer {}", h.token());
    let (s, _, b) = h.post_as(R, &auth(&h, &bearer, &[("x-claude-code-compaction", "1")]), &codex_request("turn-with-reasoning"));
    assert_eq!(s.as_u16(), 200);
    assert_eq!(&b[..], &up[..]);
}

#[test]
fn compact_call_rewrites_on_both_forms() {
    let compact_usage = json!({"input_tokens":1000000,"input_tokens_details":{"cached_tokens":999000},"output_tokens":30,"total_tokens":1000030});
    // (a) Anthropic upstream: a compact tool_use block.
    let h = Harness::start(Dialect::Anthropic, Agent::ClaudeCode, "");
    h.reply_with(
        200,
        SSE,
        anthropic_stream(
            json!({"input_tokens":1000,"cache_read_input_tokens":0,"cache_creation_input_tokens":0,"output_tokens":1}),
            Some("mcp__zygo__compact"),
            "tool_use",
        ),
    );
    let (s, _, b) = h.post(R, &codex_request("turn-with-reasoning"));
    assert_eq!(s.as_u16(), 200);
    assert_eq!(terminal_usage(&b), compact_usage);
    assert_eq!(codes(&h.usage_rows()[0]), ["context_compact"]);
    // (b) Responses upstream: a namespaced compact function_call.
    let call = |ns: &str| json!({"type":"function_call","id":"fc_1","call_id":"call_1","name":"compact","namespace":ns,"arguments":"{}","status":"completed"});
    let usage = json!({"input_tokens":1000,"input_tokens_details":{"cached_tokens":0},"output_tokens":30,"total_tokens":1030});
    let h = Harness::start(Dialect::Openai, Agent::ClaudeCode, "");
    h.reply_with(200, SSE, responses_stream("response.completed", "completed", vec![call("mcp__zygo")], usage.clone()));
    let (s, _, b) = h.post(R, &codex_request("turn-with-reasoning"));
    assert_eq!(s.as_u16(), 200);
    assert_eq!(terminal_usage(&b), compact_usage);
    assert_eq!(codes(&h.usage_rows()[0]), ["context_compact"]);
    // (c) Another server's tool: unchanged.
    let h = Harness::start(Dialect::Openai, Agent::ClaudeCode, "");
    let up = responses_stream("response.completed", "completed", vec![call("mcp__zygox")], usage);
    h.reply_with(200, SSE, up.clone());
    let (s, _, b) = h.post(R, &codex_request("turn-with-reasoning"));
    assert_eq!(s.as_u16(), 200);
    assert_eq!(&b[..], &up[..]);
    assert!(codes(&h.usage_rows()[0]).is_empty());
}

#[test]
fn soft_window_fires_at_end_of_turn_on_responses_form() {
    let h = Harness::start(Dialect::Openai, Agent::ClaudeCode, "");
    let bearer = format!("Bearer {}", h.token());
    let (s, _, b) = h.post_as(&format!("v1/zygo/sessions/{}/compact", h.session_id()), &auth(&h, &bearer, &[]), &json!({"window":100000}));
    assert_eq!(s.as_u16(), 200, "{}", String::from_utf8_lossy(&b));
    let usage = json!({"input_tokens":120000,"input_tokens_details":{"cached_tokens":100000},"output_tokens":30,"total_tokens":120030});
    let body = codex_request("turn-with-reasoning");
    let main = [("x-claude-code-request-class", "main")];
    // (a) The main class at the end of a turn: rewritten.
    h.reply_with(200, SSE, responses_stream("response.completed", "completed", vec![], usage.clone()));
    let (s, _, b) = h.post_as(R, &auth(&h, &bearer, &main), &body);
    assert_eq!(s.as_u16(), 200);
    assert_eq!(
        terminal_usage(&b),
        json!({"input_tokens":1000000,"input_tokens_details":{"cached_tokens":980000},"output_tokens":30,"total_tokens":1000030})
    );
    assert_eq!(codes(&rows_when(&h, 1)[0]), ["context_signal"]); // RS-7 round 1 review F3
    // (b) A function call ends the stream: not the end of the turn.
    let call = json!({"type":"function_call","id":"fc_1","call_id":"call_1","name":"exec_command","arguments":"{}","status":"completed"});
    let up = responses_stream("response.completed", "completed", vec![call], usage.clone());
    h.reply_with(200, SSE, up.clone());
    let (s, _, b) = h.post_as(R, &auth(&h, &bearer, &main), &body);
    assert_eq!(s.as_u16(), 200);
    assert_eq!(&b[..], &up[..]);
    // (c) No request class: unchanged.
    let up = responses_stream("response.completed", "completed", vec![], usage);
    h.reply_with(200, SSE, up.clone());
    let (s, _, b) = h.post_as(R, &auth(&h, &bearer, &[]), &body);
    assert_eq!(s.as_u16(), 200);
    assert_eq!(&b[..], &up[..]);
}

#[test] // RS-7 round 1 review F1, F3: a call item seen in the stream ends no turn, though the terminal output is empty
fn end_of_turn_uses_streamed_call_items() {
    let h = Harness::start(Dialect::Openai, Agent::ClaudeCode, "");
    let bearer = format!("Bearer {}", h.token());
    let (s, _, b) = h.post_as(&format!("v1/zygo/sessions/{}/compact", h.session_id()), &auth(&h, &bearer, &[]), &json!({"window":100000}));
    assert_eq!(s.as_u16(), 200, "{}", String::from_utf8_lossy(&b));
    let usage = json!({"input_tokens":120000,"input_tokens_details":{"cached_tokens":100000},"output_tokens":30,"total_tokens":120030});
    let body = codex_request("turn-with-reasoning");
    let main = [("x-claude-code-request-class", "main")];
    let calls = [
        ("response.output_item.added", json!({"type":"function_call","id":"fc_1","call_id":"call_1","name":"exec_command","arguments":"","status":"in_progress"})),
        ("response.output_item.added", json!({"type":"custom_tool_call","id":"ctc_1","call_id":"call_2","name":"apply_patch","input":""})),
        ("response.output_item.done", json!({"type":"local_shell_call","id":"lsc_1","call_id":"call_3","action":{"type":"exec","command":["ls"]},"status":"completed"})),
    ];
    for (ev, item) in calls {
        let ty = item["type"].as_str().unwrap().to_string();
        let up = streamed_only(Some((ev, item)), usage.clone());
        h.reply_with(200, SSE, up.clone());
        let (s, _, b) = h.post_as(R, &auth(&h, &bearer, &main), &body);
        assert_eq!(s.as_u16(), 200, "{ty}");
        assert_eq!(String::from_utf8_lossy(&b), String::from_utf8_lossy(&up), "{ty}");
    }
    // Plain text: the end of the turn.
    h.reply_with(200, SSE, streamed_only(None, usage));
    let (s, _, b) = h.post_as(R, &auth(&h, &bearer, &main), &body);
    assert_eq!(s.as_u16(), 200);
    assert_eq!(
        terminal_usage(&b),
        json!({"input_tokens":1000000,"input_tokens_details":{"cached_tokens":980000},"output_tokens":30,"total_tokens":1000030})
    );
}

#[test] // RS-7 round 1 review F2: a number the rewrite reads that is not a u64 integer leaves the line unchanged
fn non_integer_usage_leaves_the_line_unchanged() {
    let u = Usage { input: 40000, output: 0, cache_read: 150000, cache_write: 0 };
    for usage in [
        r#"{"input_tokens":190000,"input_tokens_details":{"cached_tokens":150000},"output_tokens":3e1,"total_tokens":190030}"#,
        r#"{"input_tokens":1.9e5,"input_tokens_details":{"cached_tokens":150000},"output_tokens":30,"total_tokens":190030}"#,
        r#"{"input_tokens":190000,"input_tokens_details":{"cached_tokens":150000.0},"output_tokens":30,"total_tokens":190030}"#,
    ] {
        let line = format!(r#"data: {{"type":"response.completed","sequence_number":6,"response":{{"id":"resp_up","output":[],"usage":{usage}}}}}"#);
        assert_eq!(rewrite_responses_usage(line.as_bytes(), u, 1_000_000), None, "{usage}");
    }
}

#[test] // RS-7 round 1 review F3: `response.incomplete` usage is rewritten end to end on both forms
fn incomplete_usage_rewritten_on_both_forms() {
    // (a) Anthropic upstream: a `max_tokens` stop through the encoder.
    let h = Harness::start(Dialect::Anthropic, Agent::ClaudeCode, "");
    h.reply_with(
        200,
        SSE,
        anthropic_stream(
            json!({"input_tokens":40000,"cache_read_input_tokens":150000,"cache_creation_input_tokens":0,"output_tokens":1}),
            None,
            "max_tokens",
        ),
    );
    let (s, _, b) = h.post(R, &codex_request("turn-with-reasoning"));
    assert_eq!(s.as_u16(), 200);
    let evs = events(&b);
    let t = terminals(&evs);
    assert_eq!(t.len(), 1, "{}", String::from_utf8_lossy(&b));
    assert_eq!(t[0].0, "response.incomplete");
    assert_eq!(t[0].1["response"]["usage"], rewritten());
    assert_eq!(codes(&h.usage_rows()[0]), ["context_safety"]);
    // (b) Responses upstream.
    let h = Harness::start(Dialect::Openai, Agent::ClaudeCode, "");
    let up = responses_stream("response.incomplete", "incomplete", vec![], usage_up());
    h.reply_with(200, SSE, up.clone());
    let (s, _, b) = h.post(R, &codex_request("turn-with-reasoning"));
    assert_eq!(s.as_u16(), 200);
    let want = String::from_utf8(up).unwrap().replace(&usage_up().to_string(), &usage_down().to_string());
    assert_eq!(String::from_utf8(b.to_vec()).unwrap(), want);
    assert_eq!(codes(&h.usage_rows()[0]), ["context_safety"]);
}

#[test] // RS-7 round 1 review F3: trigger 1, Responses client, Anthropic upstream: `end_turn` fires, `tool_use` does not
fn soft_window_on_anthropic_upstream_fires_at_end_turn_only() {
    let h = Harness::start(Dialect::Anthropic, Agent::ClaudeCode, "");
    let bearer = format!("Bearer {}", h.token());
    let (s, _, b) = h.post_as(&format!("v1/zygo/sessions/{}/compact", h.session_id()), &auth(&h, &bearer, &[]), &json!({"window":100000}));
    assert_eq!(s.as_u16(), 200, "{}", String::from_utf8_lossy(&b));
    let start = json!({"input_tokens":20000,"cache_read_input_tokens":100000,"cache_creation_input_tokens":0,"output_tokens":1});
    let body = codex_request("turn-with-reasoning");
    let main = [("x-claude-code-request-class", "main")];
    h.reply_with(200, SSE, anthropic_stream(start.clone(), None, "end_turn"));
    let (s, _, b) = h.post_as(R, &auth(&h, &bearer, &main), &body);
    assert_eq!(s.as_u16(), 200);
    assert_eq!(
        terminal_usage(&b),
        json!({"input_tokens":1000000,"input_tokens_details":{"cached_tokens":980000},"output_tokens":30,"total_tokens":1000030})
    );
    h.reply_with(200, SSE, anthropic_stream(start, Some("exec_command"), "tool_use"));
    let (s, _, b) = h.post_as(R, &auth(&h, &bearer, &main), &body);
    assert_eq!(s.as_u16(), 200);
    assert_eq!(
        terminal_usage(&b),
        json!({"input_tokens":120000,"input_tokens_details":{"cached_tokens":100000},"output_tokens":30,"total_tokens":120030})
    );
}
