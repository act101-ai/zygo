//! RS-2: Messages SSE (and JSON) → Responses SSE (and JSON) for a Responses client (K§6.4.3 stream obligations 1-9,
//! K§11 items 13-16). Bytes and values only; no clock, no network.

use std::collections::BTreeMap;

use regex::Regex;
use serde_json::{json, Value};
use zygo_config::Dialect;
use zygo_proxy::pipeline::Adjustment;
use zygo_proxy::tee::HOLD_MAX;
use zygo_proxy::translate::reasoning::decode_thinking;
use zygo_proxy::translate::responses_client::{request, response, Calls, Encoder};
use zygo_proxy::translate::sse::SseEvent;
use zygo_proxy::translate::{translate_response, Target};

fn tgt() -> Target {
    Target { provider: "p-ant".into(), upstream_model: "up-ant".into() }
}

fn other() -> Target {
    Target { provider: "p-oai".into(), upstream_model: "up-oai".into() }
}

/// Each event as `event: <type>\ndata: <json>\n\n`.
fn sse(events: &[Value]) -> String {
    events.iter().map(|e| format!("event: {}\ndata: {}\n\n", e["type"].as_str().unwrap(), e)).collect()
}

fn message_start() -> Value {
    json!({"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","model":"up-ant",
        "content":[],"stop_reason":null,"usage":{"input_tokens":10,"cache_read_input_tokens":5,
        "cache_creation_input_tokens":2,"output_tokens":1}}})
}

fn start(i: u64, block: Value) -> Value {
    json!({"type":"content_block_start","index":i,"content_block":block})
}

fn delta(i: u64, d: Value) -> Value {
    json!({"type":"content_block_delta","index":i,"delta":d})
}

fn stop(i: u64) -> Value {
    json!({"type":"content_block_stop","index":i})
}

fn text_block(i: u64, deltas: &[&str]) -> Vec<Value> {
    let mut v = vec![start(i, json!({"type":"text","text":""}))];
    v.extend(deltas.iter().map(|d| delta(i, json!({"type":"text_delta","text":d}))));
    v.push(stop(i));
    v
}

fn tool_block(i: u64, id: &str, name: &str, parts: &[&str]) -> Vec<Value> {
    let mut v = vec![start(i, json!({"type":"tool_use","id":id,"name":name,"input":{}}))];
    v.extend(parts.iter().map(|p| delta(i, json!({"type":"input_json_delta","partial_json":p}))));
    v.push(stop(i));
    v
}

fn end(reason: &str) -> Vec<Value> {
    vec![
        json!({"type":"message_delta","delta":{"stop_reason":reason,"stop_sequence":null},"usage":{"output_tokens":7}}),
        json!({"type":"message_stop"}),
    ]
}

/// Thinking, redacted thinking, text and a tool call, stop reason `tool_use`.
fn fixture() -> Vec<Value> {
    let mut v = vec![
        message_start(),
        start(0, json!({"type":"thinking","thinking":"","signature":""})),
        delta(0, json!({"type":"thinking_delta","thinking":"t1"})),
        delta(0, json!({"type":"signature_delta","signature":"sig1"})),
        stop(0),
        start(1, json!({"type":"redacted_thinking","data":"R1"})),
        stop(1),
    ];
    v.extend(text_block(2, &["Hel", "lo"]));
    v.extend(tool_block(3, "toolu_1", "exec_command", &["{\"cmd\":", "\"ls\"}"]));
    v.extend(end("tool_use"));
    v
}

fn enc() -> Encoder {
    Encoder::new("role-x", &tgt(), Calls::default())
}

/// The whole input in one push.
fn run(e: &mut Encoder, events: &[Value]) -> Vec<SseEvent> {
    e.push(sse(events).as_bytes())
}

/// Every `id` and `item_id` key removed, recursively.
fn norm(v: &Value) -> Value {
    match v {
        Value::Object(m) => {
            Value::Object(m.iter().filter(|(k, _)| *k != "id" && *k != "item_id").map(|(k, v)| (k.clone(), norm(v))).collect())
        }
        Value::Array(a) => Value::Array(a.iter().map(norm).collect()),
        x => x.clone(),
    }
}

fn norm_all(evs: &[SseEvent]) -> Vec<(String, Value)> {
    evs.iter().map(|e| (e.event.clone(), norm(&e.data))).collect()
}

fn types(evs: &[SseEvent]) -> Vec<String> {
    evs.iter().map(|e| e.event.clone()).collect()
}

const TERMINAL: [&str; 3] = ["response.completed", "response.incomplete", "response.failed"];

fn terminals(evs: &[SseEvent]) -> Vec<&SseEvent> {
    evs.iter().filter(|e| TERMINAL.contains(&e.event.as_str())).collect()
}

/// `sequence_number` rises by one from 0; `data.type` is the event name.
fn assert_sequence(evs: &[SseEvent]) {
    for (i, e) in evs.iter().enumerate() {
        assert_eq!(e.data["sequence_number"], json!(i), "event {i}: {}", e.event);
        assert_eq!(e.data["type"], json!(e.event), "event {i}");
    }
}

fn items_done(evs: &[SseEvent]) -> Vec<Value> {
    evs.iter().filter(|e| e.event == "response.output_item.done").map(|e| e.data["item"].clone()).collect()
}

fn adj(c: &str) -> Adjustment {
    Adjustment(c.into())
}

// ---- the stream ----

#[test]
fn stream_sequence_text_tool_thinking_redacted() {
    let mut e = enc();
    let evs = run(&mut e, &fixture());
    let want: Vec<String> = [
        "response.created",
        "response.in_progress",
        "output_item.added",
        "reasoning_summary_part.added",
        "reasoning_summary_text.delta",
        "reasoning_summary_text.done",
        "reasoning_summary_part.done",
        "output_item.done",
        "output_item.added",
        "output_item.done",
        "output_item.added",
        "content_part.added",
        "output_text.delta",
        "output_text.delta",
        "output_text.done",
        "content_part.done",
        "output_item.done",
        "output_item.added",
        "function_call_arguments.delta",
        "function_call_arguments.delta",
        "function_call_arguments.done",
        "output_item.done",
        "response.completed",
    ]
    .iter()
    .map(|t| if t.starts_with("response.") { t.to_string() } else { format!("response.{t}") })
    .collect();
    assert_eq!(types(&evs), want);
    assert_sequence(&evs);

    let r = &evs[0].data["response"];
    let id = r["id"].as_str().unwrap();
    assert!(Regex::new("^resp_[0-9A-Z]{26}$").unwrap().is_match(id), "{id}");
    assert_eq!(r["model"], "role-x");
    assert_eq!(r["status"], "in_progress");
    assert_eq!(r["object"], "response");
    assert_eq!(r["output"], json!([]));
    assert_eq!(evs[1].data["response"]["id"], id);

    // Each item's events share output_index and item id.
    let mut ids: BTreeMap<u64, String> = BTreeMap::new();
    for ev in &evs[2..evs.len() - 1] {
        let oi = ev.data["output_index"].as_u64().unwrap_or_else(|| panic!("{}: no output_index", ev.event));
        let iid = ev.data.get("item_id").or_else(|| ev.data["item"].get("id")).and_then(Value::as_str).unwrap();
        assert_eq!(ids.entry(oi).or_insert_with(|| iid.to_string()), iid, "{}", ev.event);
    }
    assert_eq!(ids.keys().copied().collect::<Vec<_>>(), vec![0, 1, 2, 3]);

    let done: Vec<&SseEvent> = evs.iter().filter(|e| e.event == "response.output_item.done").collect();
    for (i, d) in done.iter().enumerate() {
        assert_eq!(d.data["output_index"], json!(i));
    }
    let items = items_done(&evs);
    assert_eq!(items[0]["type"], "reasoning");
    assert_eq!(items[0]["summary"], json!([{"type":"summary_text","text":"t1"}]));
    assert_eq!(
        decode_thinking(items[0]["encrypted_content"].as_str().unwrap(), &tgt()),
        Some(json!({"type":"thinking","thinking":"t1","signature":"sig1"}))
    );
    assert_eq!(items[1]["type"], "reasoning");
    assert_eq!(items[1]["summary"], json!([]));
    assert_eq!(
        decode_thinking(items[1]["encrypted_content"].as_str().unwrap(), &tgt()),
        Some(json!({"type":"redacted_thinking","data":"R1"}))
    );
    assert_eq!(items[2]["type"], "message");
    assert_eq!(items[2]["role"], "assistant");
    assert_eq!(items[2]["content"], json!([{"type":"output_text","text":"Hello","annotations":[]}]));
    assert_eq!(
        norm(&items[3]),
        json!({"type":"function_call","call_id":"toolu_1","name":"exec_command","arguments":"{\"cmd\":\"ls\"}",
            "status":"completed"})
    );
    assert!(items[3]["id"].as_str().unwrap().starts_with("fc_"));

    let last = &evs.last().unwrap().data["response"];
    assert_eq!(last["id"], id);
    assert_eq!(last["status"], "completed");
    assert_eq!(last["output"], json!([]));
    assert_eq!(
        last["usage"],
        json!({"input_tokens":17,"input_tokens_details":{"cached_tokens":5},"output_tokens":7,"total_tokens":24})
    );
    assert!(e.ended());
    assert_eq!(e.adjustments(), vec![]);
    assert_eq!(e.held(), 0);
}

#[test]
fn stream_chunked_equals_whole() {
    let whole = norm_all(&run(&mut enc(), &fixture()));
    let text = sse(&fixture());
    let crlf = text.replace('\n', "\r\n");
    for (bytes, n) in [(text.as_bytes(), 1), (crlf.as_bytes(), 7)] {
        let mut e = enc();
        let evs: Vec<SseEvent> = bytes.chunks(n).flat_map(|c| e.push(c)).collect();
        assert_eq!(norm_all(&evs), whole, "chunk {n}");
        assert!(e.ended());
    }
}

#[test]
fn stream_server_tool_dropped() {
    let mut ev = vec![message_start()];
    ev.extend(text_block(0, &["a"]));
    ev.push(start(1, json!({"type":"server_tool_use","id":"srvtoolu_1","name":"web_search","input":{}})));
    ev.push(delta(1, json!({"type":"input_json_delta","partial_json":"{\"query\":\"x\"}"})));
    ev.push(stop(1));
    ev.push(start(2, json!({"type":"web_search_tool_result","tool_use_id":"srvtoolu_1","content":[]})));
    ev.push(stop(2));
    ev.extend(text_block(3, &["b"]));
    ev.extend(end("end_turn"));
    let mut e = enc();
    let evs = run(&mut e, &ev);
    assert_sequence(&evs);
    let items = items_done(&evs);
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["content"][0]["text"], "a");
    assert_eq!(items[1]["content"][0]["text"], "b");
    let added: Vec<&SseEvent> = evs.iter().filter(|e| e.event == "response.output_item.added").collect();
    assert_eq!(added.len(), 2);
    let oi: Vec<Value> = evs.iter().filter_map(|e| e.data.get("output_index").cloned()).collect();
    assert!(oi.iter().all(|v| *v == json!(0) || *v == json!(1)));
    assert_eq!(evs.last().unwrap().event, "response.completed");
    assert_eq!(e.adjustments(), vec![adj("server_tool_dropped")]);
}

#[test]
fn stream_max_tokens_incomplete() {
    let mut ev = vec![message_start()];
    ev.extend(text_block(0, &["cut"]));
    ev.extend(end("max_tokens"));
    let mut e = enc();
    let evs = run(&mut e, &ev);
    assert_sequence(&evs);
    let last = evs.last().unwrap();
    assert_eq!(last.event, "response.incomplete");
    let r = &last.data["response"];
    assert_eq!(r["status"], "incomplete");
    assert_eq!(r["incomplete_details"], json!({"reason":"max_output_tokens"}));
    assert!(r["usage"].is_object());
    assert_eq!(terminals(&evs).len(), 1);
    assert!(e.ended());
}

#[test]
fn stream_error_after_text_one_failed() {
    let ev = vec![
        message_start(),
        start(0, json!({"type":"text","text":""})),
        delta(0, json!({"type":"text_delta","text":"par"})),
        json!({"type":"error","error":{"type":"overloaded_error","message":"busy"}}),
        json!({"type":"message_stop"}),
    ];
    let mut e = enc();
    let evs = run(&mut e, &ev);
    assert_sequence(&evs);
    let t = terminals(&evs);
    assert_eq!(t.len(), 1);
    assert_eq!(evs.last().unwrap().event, "response.failed");
    let r = &evs.last().unwrap().data["response"];
    assert_eq!(r["status"], "failed");
    assert_eq!(r["error"], json!({"code":"overloaded_error","message":"busy"}));
    assert!(e.ended());
    assert!(e.push(sse(&[json!({"type":"message_stop"})]).as_bytes()).is_empty());
    assert!(e.fail("x", "y").is_empty());
    assert!(e.finish().is_empty());

    let ev = vec![message_start(), json!({"type":"error","error":{"type":"Bad Type!","message":"m"}})];
    let mut e = enc();
    let evs = run(&mut e, &ev);
    assert_sequence(&evs);
    assert_eq!(evs.last().unwrap().data["response"]["error"], json!({"code":"api_error","message":"m"}));
}

#[test]
fn stream_ping_and_unknown_dropped() {
    let plain = norm_all(&run(&mut enc(), &fixture()));
    let mut noisy = vec![json!({"type":"ping"}), json!({"type":"future_event"})];
    for f in fixture() {
        noisy.push(f);
        noisy.push(json!({"type":"ping"}));
        noisy.push(json!({"type":"future_event","x":1}));
    }
    let mut e = enc();
    let evs = run(&mut e, &noisy);
    assert_sequence(&evs);
    assert_eq!(norm_all(&evs), plain);
}

#[test]
fn stream_unparsable_after_start_fails() {
    let mut e = enc();
    let mut evs = run(&mut e, &[message_start()]);
    evs.extend(e.push(b"event: content_block_delta\ndata: {not json\n\n"));
    assert_sequence(&evs);
    assert_eq!(types(&evs), vec!["response.created", "response.in_progress", "response.failed"]);
    assert_eq!(
        evs[2].data["response"]["error"],
        json!({"code":"api_error","message":"upstream event is not JSON"})
    );
    assert!(e.ended());
    assert_eq!(e.held(), 0);
}

#[test]
fn stream_error_before_start() {
    let mut e = enc();
    let evs = run(&mut e, &[json!({"type":"error","error":{"type":"api_error","message":"no"}})]);
    assert_eq!(types(&evs), vec!["response.created", "response.in_progress", "response.failed"]);
    assert_sequence(&evs);
    assert!(e.ended());
}

#[test]
fn stream_partial_line_over_hold_max_fails() {
    let mut e = enc();
    let mut evs = run(&mut e, &[message_start()]);
    evs.extend(e.push(b"data: "));
    let chunk = vec![b'x'; 64 << 10];
    for c in std::iter::repeat_n(&chunk, HOLD_MAX / chunk.len() + 2) {
        evs.extend(e.push(c));
    }
    assert_sequence(&evs);
    let t = terminals(&evs);
    assert_eq!(t.len(), 1);
    assert_eq!(t[0].event, "response.failed");
    assert_eq!(t[0].data["response"]["error"]["message"], "upstream stream line too long");
    assert_eq!(e.held(), 0);
    assert!(e.ended());
    assert!(e.push(&chunk).is_empty());
    assert_eq!(e.held(), 0);
}

#[test]
fn stream_tool_args_over_hold_max_fails() {
    let mut e = enc();
    let mut evs = run(&mut e, &[message_start(), start(0, json!({"type":"tool_use","id":"t","name":"n","input":{}}))]);
    let part = "x".repeat(64 << 10);
    let one = sse(&[delta(0, json!({"type":"input_json_delta","partial_json":part}))]);
    for _ in 0..HOLD_MAX / part.len() + 2 {
        evs.extend(e.push(one.as_bytes()));
    }
    assert_sequence(&evs);
    let t = terminals(&evs);
    assert_eq!(t.len(), 1);
    assert_eq!(t[0].event, "response.failed");
    assert_eq!(t[0].data["response"]["error"], json!({"code":"api_error","message":"upstream tool call too long"}));
    assert_eq!(e.held(), 0);
    assert!(e.ended());
}

#[test] // RS-13 round 2 review G4: the first delta clears the start's input; only the delta counts toward HOLD_MAX
fn stream_large_start_input_then_small_delta_completes() {
    let input = json!({"a":"x".repeat(HOLD_MAX - 1024)});
    let args = format!("{{\"b\":\"{}\"}}", "y".repeat(4096));
    let mut ev = vec![message_start(), start(0, json!({"type":"tool_use","id":"t","name":"n","input":input}))];
    ev.push(delta(0, json!({"type":"input_json_delta","partial_json":args})));
    ev.push(stop(0));
    ev.extend(end("tool_use"));
    let mut e = enc();
    let evs = run(&mut e, &ev);
    assert_sequence(&evs);
    assert_eq!(evs.last().unwrap().event, "response.completed", "{:?}", terminals(&evs).last().map(|t| &t.data));
    let items = items_done(&evs);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["arguments"], json!(args));
}

#[test]
fn stream_one_chunk_of_small_blocks_over_hold_max_completes() {
    // The bound counts held bytes, not the size of one push: the lines already read are not held.
    let text = "y".repeat(64 << 10);
    let n = HOLD_MAX / text.len() + 2;
    let mut ev = vec![message_start()];
    for i in 0..n as u64 {
        ev.extend(text_block(i, &[&text]));
    }
    ev.extend(end("end_turn"));
    let mut e = enc();
    let evs = run(&mut e, &ev);
    assert_eq!(evs.last().unwrap().event, "response.completed");
    assert_eq!(items_done(&evs).len(), n);
    assert_eq!(e.held(), 0);
}

#[test]
fn stream_fail_and_finish() {
    let mut e = enc();
    assert!(e.fail("upstream_error", "gone").is_empty());
    assert!(!e.ended());
    let mut evs = run(&mut e, &[message_start()]);
    evs.extend(e.fail("upstream_error", "gone"));
    assert_sequence(&evs);
    assert_eq!(types(&evs), vec!["response.created", "response.in_progress", "response.failed"]);
    assert_eq!(evs[2].data["response"]["error"], json!({"code":"upstream_error","message":"gone"}));
    assert!(e.ended());
    assert!(e.fail("upstream_error", "again").is_empty());

    let f = fixture();
    let mut e = enc();
    let mut evs = run(&mut e, &f[..f.len() - 1]);
    evs.extend(e.finish());
    assert_sequence(&evs);
    let t = terminals(&evs);
    assert_eq!(t.len(), 1);
    assert_eq!(t[0].data["response"]["error"], json!({"code":"api_error","message":"upstream stream ended early"}));
    assert!(e.ended());

    let mut e = enc();
    let evs = e.finish();
    assert_eq!(types(&evs), vec!["response.created", "response.in_progress", "response.failed"]);
    assert_sequence(&evs);
}

// ---- round trips through `request` ----

fn user(text: &str) -> Value {
    json!({"type":"message","role":"user","content":[{"type":"input_text","text":text}]})
}

#[test]
fn tool_call_round_trip_keeps_call_id() {
    let items = items_done(&run(&mut enc(), &fixture()));
    let req = json!({"model":"role-x","max_output_tokens":100,
        "tools":[{"type":"function","name":"exec_command","parameters":{"type":"object"}}],
        "input":[user("hi"), items[3], {"type":"function_call_output","call_id":"toolu_1","output":"ok"}]});
    let (m, _) = request(&req, &tgt(), None).unwrap();
    let msgs = m["messages"].as_array().unwrap();
    let call = &msgs[1]["content"][0];
    assert_eq!(msgs[1]["role"], "assistant");
    assert_eq!(call["type"], "tool_use");
    assert_eq!(call["id"], "toolu_1");
    assert_eq!(call["input"], json!({"cmd":"ls"}));
    let res = &msgs[2]["content"][0];
    assert_eq!(res["type"], "tool_result");
    assert_eq!(res["tool_use_id"], "toolu_1");
}

#[test]
fn custom_tool_call_round_trip() {
    let patch = "*** Begin Patch\n*** End Patch\n";
    let tools = json!([{"type":"custom","name":"apply_patch"}]);
    let calls = Calls::of(&json!({"tools":tools}));
    let mut ev = vec![message_start()];
    let args = json!({"input":patch}).to_string();
    ev.extend(tool_block(0, "toolu_p", "apply_patch", &[&args]));
    ev.extend(end("tool_use"));
    let mut e = Encoder::new("role-x", &tgt(), calls);
    let evs = run(&mut e, &ev);
    assert_sequence(&evs);
    assert_eq!(
        types(&evs[2..evs.len() - 1]),
        vec![
            "response.output_item.added",
            "response.custom_tool_call_input.delta",
            "response.custom_tool_call_input.done",
            "response.output_item.done",
        ]
    );
    assert_eq!(evs[3].data["delta"], patch);
    assert_eq!(evs[4].data["input"], patch);
    let item = items_done(&evs).remove(0);
    assert_eq!(
        norm(&item),
        json!({"type":"custom_tool_call","call_id":"toolu_p","name":"apply_patch","input":patch,"status":"completed"})
    );
    assert!(item["id"].as_str().unwrap().starts_with("ctc_"));

    let req = json!({"model":"role-x","max_output_tokens":100,"tools":tools,
        "input":[user("fix it"), item, {"type":"custom_tool_call_output","call_id":"toolu_p","output":"done"}]});
    let (m, _) = request(&req, &tgt(), None).unwrap();
    let call = &m["messages"][1]["content"][0];
    assert_eq!(call["type"], "tool_use");
    assert_eq!(call["id"], "toolu_p");
    assert_eq!(call["input"], json!({"input":patch}));

    // A custom entry with no string name, and a function entry of that name, give a function call.
    let calls = Calls::of(&json!({"tools":[{"type":"custom"},{"type":"custom","name":5},
        {"type":"function","name":"apply_patch"}]}));
    let msg = json!({"content":[{"type":"tool_use","id":"toolu_q","name":"apply_patch","input":{"input":"x"}}],
        "stop_reason":"tool_use","usage":{}});
    let (r, _) = response(&msg, "role-x", &tgt(), &calls);
    assert_eq!(r["output"][0]["type"], "function_call");
    assert_eq!(r["output"][0]["arguments"], "{\"input\":\"x\"}");
}

fn thinking_input() -> Value {
    let items = items_done(&run(&mut enc(), &fixture()));
    json!({"model":"role-x","max_output_tokens":100,"input":[user("hi"), items[0], items[1],
        {"type":"message","role":"assistant","content":[{"type":"output_text","text":"Hello"}]}, user("next")]})
}

#[test]
fn thinking_round_trip_same_provider_keeps_signature() {
    let (m, codes) = request(&thinking_input(), &tgt(), None).unwrap();
    let a = &m["messages"][1];
    assert_eq!(a["role"], "assistant");
    assert_eq!(a["content"][0], json!({"type":"thinking","thinking":"t1","signature":"sig1"}));
    assert_eq!(a["content"][1], json!({"type":"redacted_thinking","data":"R1"}));
    assert!(codes.iter().all(|c| !c.0.starts_with("reasoning_dropped")), "{codes:?}");
}

#[test]
fn thinking_round_trip_other_provider_dropped() {
    let (m, codes) = request(&thinking_input(), &other(), None).unwrap();
    let a = &m["messages"][1];
    assert_eq!(a["role"], "assistant");
    assert!(a["content"].as_array().unwrap().iter().all(|b| b["type"] != "thinking" && b["type"] != "redacted_thinking"));
    assert!(codes.contains(&adj("reasoning_dropped:2")), "{codes:?}");
}

// ---- the JSON form (obligation 8) ----

#[test]
fn json_response_same_content() {
    let stream_items = items_done(&run(&mut enc(), &fixture()));
    let mut msg = json!({"id":"msg_1","type":"message","role":"assistant","model":"up-ant","content":[
        {"type":"thinking","thinking":"t1","signature":"sig1"},
        {"type":"redacted_thinking","data":"R1"},
        {"type":"text","text":"Hello"},
        {"type":"server_tool_use","id":"srvtoolu_1","name":"web_search","input":{"query":"x"}},
        {"type":"tool_use","id":"toolu_1","name":"exec_command","input":{"cmd":"ls"}}],
        "stop_reason":"tool_use",
        "usage":{"input_tokens":10,"cache_read_input_tokens":5,"cache_creation_input_tokens":2,"output_tokens":7}});
    let (r, codes) = response(&msg, "role-x", &tgt(), &Calls::default());
    assert!(r["id"].as_str().unwrap().starts_with("resp_"));
    assert_eq!(r["object"], "response");
    assert_eq!(r["model"], "role-x");
    assert_eq!(r["status"], "completed");
    assert_eq!(norm(&r["output"]), norm(&Value::Array(stream_items)));
    assert_eq!(
        r["usage"],
        json!({"input_tokens":17,"input_tokens_details":{"cached_tokens":5},"output_tokens":7,"total_tokens":24})
    );
    assert_eq!(codes, vec![adj("server_tool_dropped")]);

    msg["stop_reason"] = json!("max_tokens");
    let (r, _) = response(&msg, "role-x", &tgt(), &Calls::default());
    assert_eq!(r["status"], "incomplete");
    assert_eq!(r["incomplete_details"]["reason"], "max_output_tokens");
}

// ---- review round 1 findings ----

/// The one terminal event of `evs`, asserted to be `response.failed` with `message`; the encoder holds nothing.
fn assert_failed(e: &Encoder, evs: &[SseEvent], message: &str) {
    assert_sequence(evs);
    let t = terminals(evs);
    assert_eq!(t.len(), 1, "{:?}", types(evs));
    assert_eq!(t[0].event, "response.failed");
    assert_eq!(t[0].data["response"]["error"], json!({"code":"api_error","message":message}));
    assert_eq!(evs.last().unwrap().event, "response.failed");
    assert_eq!(e.held(), 0);
    assert!(e.ended());
}

/// Whole delta events of 64 KiB each on block 0 until their sum passes `HOLD_MAX`, one push each.
fn push_deltas_past_hold_max(e: &mut Encoder, d: impl Fn(&str) -> Value) -> Vec<SseEvent> {
    let part = "x".repeat(64 << 10);
    let one = sse(&[delta(0, d(&part))]);
    (0..HOLD_MAX / part.len() + 2).flat_map(|_| e.push(one.as_bytes())).collect()
}

#[test]
fn f5_custom_tool_from_history_returns_as_custom_tool_call() {
    // PC-5's compaction request sends `tools: []`; its history here also holds an `apply_patch` custom call.
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/compaction-request.json");
    let rec: Value = serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap();
    let mut body = rec["body"].clone();
    assert_eq!(body["tools"], json!([]));
    body["max_output_tokens"] = json!(100);
    let input = body["input"].as_array_mut().unwrap();
    let last = input.pop().unwrap();
    input.push(json!({"type":"custom_tool_call","call_id":"call_p0","name":"apply_patch","input":"*** Begin Patch\n"}));
    input.push(json!({"type":"custom_tool_call_output","call_id":"call_p0","output":"ok"}));
    input.push(last);
    let (m, codes) = request(&body, &tgt(), None).unwrap();
    assert!(m["tools"].as_array().unwrap().iter().any(|t| t["name"] == "apply_patch"), "{}", m["tools"]);
    assert!(codes.iter().any(|c| c.0.starts_with("tools_from_history:")), "{codes:?}");

    let calls = Calls::of(&body);
    let mut ev = vec![message_start()];
    ev.extend(tool_block(0, "toolu_p", "apply_patch", &[&json!({"input":"x"}).to_string()]));
    ev.extend(end("tool_use"));
    let items = items_done(&run(&mut Encoder::new("role-x", &tgt(), calls.clone()), &ev));
    assert_eq!(items[0]["type"], "custom_tool_call");
    assert_eq!(items[0]["input"], "x");
    let msg = json!({"content":[{"type":"tool_use","id":"toolu_p","name":"apply_patch","input":{"input":"x"}}],
        "stop_reason":"tool_use","usage":{}});
    assert_eq!(response(&msg, "role-x", &tgt(), &calls).0["output"][0]["type"], "custom_tool_call");
    // A function call in the history declares no custom tool.
    let msg = json!({"content":[{"type":"tool_use","id":"toolu_e","name":"exec_command","input":{}}],
        "stop_reason":"tool_use","usage":{}});
    assert_eq!(response(&msg, "role-x", &tgt(), &calls).0["output"][0]["type"], "function_call");
}

#[test]
fn f1_reasoning_mark_over_mark_max_is_not_sent() {
    // ~3.3 MiB of thinking: held within HOLD_MAX, its base64 mark over MARK_MAX.
    let part = "t".repeat(64 << 10);
    let n = 53;
    let mut ev = vec![message_start(), start(0, json!({"type":"thinking","thinking":"","signature":""}))];
    ev.extend((0..n).map(|_| delta(0, json!({"type":"thinking_delta","thinking":part}))));
    ev.push(delta(0, json!({"type":"signature_delta","signature":"sig1"})));
    ev.push(stop(0));
    ev.extend(text_block(1, &["ok"]));
    ev.extend(end("end_turn"));
    let mut e = enc();
    let evs = run(&mut e, &ev);
    assert_sequence(&evs);
    assert_eq!(evs.last().unwrap().event, "response.completed");
    let items = items_done(&evs);
    assert_eq!(items[0]["type"], "reasoning");
    assert!(items[0].get("encrypted_content").is_none(), "mark sent");
    assert_eq!(items[0]["summary"][0]["text"].as_str().unwrap().len(), n * part.len());
    assert_eq!(e.adjustments(), vec![adj("reasoning_unmarked:1")]);

    let thinking = part.repeat(n);
    let msg = json!({"content":[{"type":"thinking","thinking":thinking,"signature":"s"},
        {"type":"redacted_thinking","data":"R1"}],"stop_reason":"end_turn","usage":{}});
    let (r, codes) = response(&msg, "role-x", &tgt(), &Calls::default());
    assert!(r["output"][0].get("encrypted_content").is_none());
    assert!(r["output"][1]["encrypted_content"].is_string());
    assert_eq!(codes, vec![adj("reasoning_unmarked:1")]);
}

#[test]
fn f2_data_lines_with_no_blank_line_fail_inside_one_push() {
    // Six 1 MiB data lines of one event, no blank line, in one chunk.
    let mib = "x".repeat(1 << 20);
    let mut chunk = String::from("data: {\"type\":\"future_event\",\"a0\":\"x\"\n");
    for i in 1..=6 {
        chunk.push_str(&format!("data: ,\"a{i}\":\"{mib}\"\n"));
    }
    let mut e = enc();
    let mut evs = run(&mut e, &[message_start()]);
    evs.extend(e.push(chunk.as_bytes()));
    assert_failed(&e, &evs, "upstream event too long");

    // Three such lines and the blank line: an unknown event, dropped, nothing held.
    let mut chunk = String::from("data: {\"type\":\"future_event\",\"a0\":\"x\"\n");
    for i in 1..=3 {
        chunk.push_str(&format!("data: ,\"a{i}\":\"{mib}\"\n"));
    }
    chunk.push_str("data: }\n\n");
    let mut e = enc();
    let evs = run(&mut e, &[message_start()]);
    assert!(e.push(chunk.as_bytes()).is_empty());
    assert_eq!(e.held(), 0);
    assert_eq!(evs.len(), 2);
    assert!(!e.ended());
}

#[test]
fn f2_one_line_over_hold_max_in_one_chunk_fails() {
    let mut e = enc();
    let mut evs = run(&mut e, &[message_start()]);
    let line = format!("data: {}\n\n", "x".repeat(HOLD_MAX + 1));
    evs.extend(e.push(line.as_bytes()));
    assert_failed(&e, &evs, "upstream event too long");
}

#[test]
fn f3_partial_line_during_tool_call_names_the_line() {
    let mut e = enc();
    let mut evs = run(&mut e, &[message_start()]);
    evs.extend(run(&mut e, &tool_block(0, "t", "n", &["{\"a\":1"])[..2]));
    evs.extend(e.push(b"data: "));
    let chunk = vec![b'x'; 64 << 10];
    for c in std::iter::repeat_n(&chunk, HOLD_MAX / chunk.len() + 2) {
        evs.extend(e.push(c));
    }
    assert_failed(&e, &evs, "upstream stream line too long");
}

#[test]
fn f3_f6_text_past_hold_max_fails() {
    let mut e = enc();
    let mut evs = run(&mut e, &[message_start(), start(0, json!({"type":"text","text":""}))]);
    evs.extend(push_deltas_past_hold_max(&mut e, |x| json!({"type":"text_delta","text":x})));
    assert_failed(&e, &evs, "upstream content block too long");
}

#[test]
fn f3_f6_thinking_past_hold_max_fails() {
    let mut e = enc();
    let mut evs = run(&mut e, &[message_start(), start(0, json!({"type":"thinking","thinking":"","signature":""}))]);
    evs.extend(push_deltas_past_hold_max(&mut e, |x| json!({"type":"thinking_delta","thinking":x})));
    assert_failed(&e, &evs, "upstream content block too long");
}

#[test]
fn f3_f6_custom_tool_input_past_hold_max_fails() {
    let calls = Calls::of(&json!({"tools":[{"type":"custom","name":"apply_patch"}]}));
    let mut e = Encoder::new("role-x", &tgt(), calls);
    let tool = start(0, json!({"type":"tool_use","id":"t","name":"apply_patch","input":{}}));
    let mut evs = run(&mut e, &[message_start(), tool]);
    evs.extend(push_deltas_past_hold_max(&mut e, |x| json!({"type":"input_json_delta","partial_json":x})));
    assert_failed(&e, &evs, "upstream tool call too long");
}

#[test]
fn f4_stop_reasons() {
    for (reason, event, status, detail) in [
        ("model_context_window_exceeded", "response.incomplete", "incomplete", Some("max_output_tokens")),
        ("refusal", "response.incomplete", "incomplete", Some("content_filter")),
        ("pause_turn", "response.completed", "completed", None),
        ("end_turn", "response.completed", "completed", None),
        ("stop_sequence", "response.completed", "completed", None),
    ] {
        let mut ev = vec![message_start()];
        ev.extend(text_block(0, &["a"]));
        ev.extend(end(reason));
        let evs = run(&mut enc(), &ev);
        assert_sequence(&evs);
        let last = evs.last().unwrap();
        assert_eq!(last.event, event, "{reason}");
        assert_eq!(last.data["response"]["status"], status, "{reason}");
        let msg = json!({"content":[{"type":"text","text":"a"}],"stop_reason":reason,"usage":{}});
        let (r, _) = response(&msg, "role-x", &tgt(), &Calls::default());
        assert_eq!(r["status"], status, "{reason}");
        for resp in [&last.data["response"], &r] {
            match detail {
                Some(d) => assert_eq!(resp["incomplete_details"], json!({"reason":d}), "{reason}"),
                None => assert!(resp.get("incomplete_details").is_none(), "{reason}"),
            }
        }
    }
}

#[test]
fn f6_utf8_one_byte_chunks_equal_whole() {
    let mut ev = vec![
        message_start(),
        start(0, json!({"type":"thinking","thinking":"","signature":""})),
        delta(0, json!({"type":"thinking_delta","thinking":"思考 ü"})),
        delta(0, json!({"type":"signature_delta","signature":"sig"})),
        stop(0),
    ];
    ev.extend(text_block(1, &["héllo ", "日本", " 🎉"]));
    ev.extend(tool_block(2, "toolu_u", "exec_command", &["{\"cmd\":\"é", "cho 🎉\"}"]));
    ev.extend(end("tool_use"));
    let whole = run(&mut enc(), &ev);
    let bytes = sse(&ev).into_bytes();
    let mut e = enc();
    let one: Vec<SseEvent> = bytes.chunks(1).flat_map(|c| e.push(c)).collect();
    assert_eq!(norm_all(&one), norm_all(&whole));
    let items = items_done(&one);
    assert_eq!(items[0]["summary"][0]["text"], "思考 ü");
    assert_eq!(items[1]["content"][0]["text"], "héllo 日本 🎉");
    assert_eq!(items[2]["arguments"], "{\"cmd\":\"écho 🎉\"}");
}

#[test]
fn f6_signature_only_thinking() {
    let ev = vec![
        message_start(),
        start(0, json!({"type":"thinking","thinking":"","signature":""})),
        delta(0, json!({"type":"signature_delta","signature":"sigonly"})),
        stop(0),
        json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":1}}),
        json!({"type":"message_stop"}),
    ];
    let evs = run(&mut enc(), &ev);
    assert_sequence(&evs);
    assert_eq!(
        types(&evs),
        vec!["response.created", "response.in_progress", "response.output_item.added", "response.output_item.done",
            "response.completed"]
    );
    let item = &items_done(&evs)[0];
    assert_eq!(item["summary"], json!([]));
    assert_eq!(
        decode_thinking(item["encrypted_content"].as_str().unwrap(), &tgt()),
        Some(json!({"type":"thinking","thinking":"","signature":"sigonly"}))
    );
}

#[test]
fn note_error_message_cut_as_echo() {
    let long = format!("a\u{7}b\u{200B}c\u{2028}{}", "x".repeat(100));
    let ev = vec![message_start(), json!({"type":"error","error":{"type":"api_error","message":long}})];
    let evs = run(&mut enc(), &ev);
    let want: String = format!("a?b?c?{}", "x".repeat(58));
    assert_eq!(want.chars().count(), 64);
    assert_eq!(evs.last().unwrap().data["response"]["error"]["message"], want);
}

// ---- RS-9: namespaced calls map back (K§6.4.3 stream obligation 2) ----

/// The `body` of `fixtures/codex/<rel>`.
fn record(rel: &str) -> Value {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/codex").join(rel);
    let rec: Value = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
    rec["body"].clone()
}

/// A Messages JSON response with one `tool_use` named `name`.
fn tool_msg(id: &str, name: &str) -> Value {
    json!({"content":[{"type":"tool_use","id":id,"name":name,"input":{}}],"stop_reason":"tool_use","usage":{}})
}

#[test]
fn namespace_call_maps_back_stream() {
    let body = record("agents/requests/pc6-a-0002.json");
    let calls = Calls::of(&body);
    let mut ev = vec![message_start()];
    ev.extend(tool_block(0, "toolu_s", "multi_agent_v1__spawn_agent", &["{\"agent_type\":", "\"explorer\"}"]));
    ev.extend(end("tool_use"));
    let evs = run(&mut Encoder::new("role-x", &tgt(), calls), &ev);
    assert_sequence(&evs);
    assert_eq!(
        types(&evs[2..evs.len() - 1]),
        vec![
            "response.output_item.added",
            "response.function_call_arguments.delta",
            "response.function_call_arguments.delta",
            "response.function_call_arguments.done",
            "response.output_item.done",
        ]
    );
    assert_eq!(evs[2].data["item"]["name"], "spawn_agent");
    assert_eq!(evs[2].data["item"]["namespace"], "multi_agent_v1");
    let done = items_done(&evs).remove(0);
    assert_eq!(
        norm(&done),
        json!({"type":"function_call","call_id":"toolu_s","name":"spawn_agent","namespace":"multi_agent_v1",
            "arguments":"{\"agent_type\":\"explorer\"}","status":"completed"})
    );
    assert!(done["id"].as_str().unwrap().starts_with("fc_"));

    // Round trip: the item, sent back in history, is a call of the flat name.
    let mut req = body.clone();
    req["max_output_tokens"] = json!(100);
    let input = req["input"].as_array_mut().unwrap();
    input.push(done);
    input.push(json!({"type":"function_call_output","call_id":"toolu_s","output":"ok"}));
    let (m, codes) = request(&req, &tgt(), None).unwrap();
    let call = m["messages"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|x| x["content"].as_array().unwrap().clone())
        .find(|b| b["type"] == "tool_use" && b["id"] == "toolu_s")
        .unwrap();
    assert_eq!(call["name"], "multi_agent_v1__spawn_agent");
    assert!(!codes.iter().any(|c| c.0.starts_with("tools_from_history:")), "{codes:?}");
}

#[test]
fn namespace_call_maps_back_json_and_chat() {
    let mcp = Calls::of(&record("mcp/requests/pc9-call-0033.json"));
    let (r, _) = response(&tool_msg("toolu_c", "mcp__zygo__compact"), "role-x", &tgt(), &mcp);
    assert_eq!(r["output"][0]["type"], "function_call");
    assert_eq!(r["output"][0]["name"], "compact");
    assert_eq!(r["output"][0]["namespace"], "mcp__zygo");

    let agents = Calls::of(&record("agents/requests/pc6-a-0002.json"));
    let chat = json!({"id":"c1","model":"up","choices":[{"index":0,"message":{"role":"assistant","content":null,
        "tool_calls":[{"id":"call_1","type":"function","function":{"name":"multi_agent_v1__wait_agent","arguments":"{}"}}]},
        "finish_reason":"tool_calls"}],"usage":{"prompt_tokens":1,"completion_tokens":1}});
    let msg = translate_response(&chat, Dialect::OpenaiChat, &tgt());
    let (r, _) = response(&msg, "role-x", &tgt(), &agents);
    let item = r["output"].as_array().unwrap().iter().find(|x| x["type"] == "function_call").unwrap().clone();
    assert_eq!(item["name"], "wait_agent");
    assert_eq!(item["namespace"], "multi_agent_v1");
    assert_eq!(item["call_id"], "call_1");
}

#[test]
fn namespace_call_maps_back_from_history_only() {
    let mut body = record("mcp/requests/pc9-call-0033.json");
    body["tools"] = json!([]);
    let calls = Calls::of(&body);
    let (r, _) = response(&tool_msg("toolu_c", "mcp__zygo__compact"), "role-x", &tgt(), &calls);
    assert_eq!(r["output"][0]["name"], "compact");
    assert_eq!(r["output"][0]["namespace"], "mcp__zygo");
    let (r, _) = response(&tool_msg("toolu_e", "exec_command"), "role-x", &tgt(), &calls);
    assert_eq!(r["output"][0]["name"], "exec_command");
    assert!(r["output"][0].get("namespace").is_none(), "{}", r["output"][0]);
    let (r, _) = response(&tool_msg("toolu_s", "multi_agent_v1__spawn_agent"), "role-x", &tgt(), &Calls::default());
    assert_eq!(r["output"][0]["name"], "multi_agent_v1__spawn_agent");
    assert!(r["output"][0].get("namespace").is_none(), "{}", r["output"][0]);
}

// ---- RS-9 review round 1 findings ----

#[test]
fn namespace_call_maps_back_one_kind_for_events_and_item() {
    // F2: a custom tool `ns__x` sent and a history call of `ns`/`x`: `request` refuses the body, and the encoder's
    // events and item come from one lookup, so they agree on the kind.
    let body = json!({"model":"role-x","max_output_tokens":100,"tools":[{"type":"custom","name":"ns__x"}],
        "input":[user("go"),
            {"type":"function_call","call_id":"c1","name":"x","namespace":"ns","arguments":"{}"},
            {"type":"function_call_output","call_id":"c1","output":"r"}, user("next")]});
    let e = request(&body, &tgt(), None).unwrap_err();
    assert_eq!(e.message, "input: two tools have the same name");
    let mut ev = vec![message_start()];
    ev.extend(tool_block(0, "toolu_x", "ns__x", &["{\"input\":\"p\"}"]));
    ev.extend(end("tool_use"));
    let evs = run(&mut Encoder::new("role-x", &tgt(), Calls::of(&body)), &ev);
    assert_sequence(&evs);
    let added = &evs[2].data["item"];
    let done = items_done(&evs).remove(0);
    assert_eq!(added["type"], done["type"]);
    let prefix = if done["type"] == "custom_tool_call" { "response.custom_tool_call_input" } else { "response.function_call_arguments" };
    assert!(types(&evs[3..evs.len() - 2]).iter().all(|t| t.starts_with(prefix)), "{:?}", types(&evs));

    // F1: a plain sent function of the flat name: a model call of it never returns as namespaced.
    let body = json!({"tools":[{"type":"function","name":"mcp__zygo__compact"}],
        "input":[{"type":"function_call","call_id":"c1","name":"compact","namespace":"mcp__zygo","arguments":"{}"}]});
    let msg = json!({"content":[{"type":"tool_use","id":"t","name":"mcp__zygo__compact","input":{}}],
        "stop_reason":"tool_use","usage":{}});
    let (r, _) = response(&msg, "role-x", &tgt(), &Calls::of(&body));
    assert_eq!(r["output"][0]["name"], "mcp__zygo__compact");
    assert!(r["output"][0].get("namespace").is_none(), "{}", r["output"][0]);
}

#[test]
fn namespace_call_maps_back_after_resumed_plain_history_call() {
    // A plain history call of a sent pair's flat name (pre-0.172 MCP naming): a new model call of it still returns
    // with `name` and `namespace`.
    let body = json!({"model":"role-x","max_output_tokens":100,
        "tools":[{"type":"namespace","name":"mcp__zygo","tools":[{"type":"function","name":"compact"}]}],
        "input":[user("go"), {"type":"function_call","call_id":"c1","name":"mcp__zygo__compact","arguments":"{}"},
            {"type":"function_call_output","call_id":"c1","output":"r"}, user("next")]});
    assert!(request(&body, &tgt(), None).is_ok());
    let msg = json!({"content":[{"type":"tool_use","id":"t","name":"mcp__zygo__compact","input":{}}],
        "stop_reason":"tool_use","usage":{}});
    let (r, _) = response(&msg, "role-x", &tgt(), &Calls::of(&body));
    assert_eq!(r["output"][0]["name"], "compact");
    assert_eq!(r["output"][0]["namespace"], "mcp__zygo");
}

// ---- RS-11: a json_schema answer returns as output_text (K§6.4.3 stream obligation 2) ----

#[test]
fn text_format_json_schema_answer_returns_as_output_text() {
    let body = record("compaction/requests/pc8-tui-high-0028.json");
    let answer = "{\"title\":\"Fix the build\"}";
    let mut ev = vec![message_start()];
    ev.extend(text_block(0, &["{\"title\":", "\"Fix the build\"}"]));
    ev.extend(end("end_turn"));
    let evs = run(&mut Encoder::new("role-x", &tgt(), Calls::of(&body)), &ev);
    assert_sequence(&evs);
    let term = terminals(&evs);
    assert_eq!(term.len(), 1);
    assert_eq!(term[0].event, "response.completed");
    let want = json!({"type":"message","role":"assistant","status":"completed",
        "content":[{"type":"output_text","text":answer,"annotations":[]}]});
    assert_eq!(items_done(&evs).iter().map(norm).collect::<Vec<_>>(), vec![want.clone()]);
    // JSON form: Anthropic upstream, then chat upstream.
    let msg = json!({"content":[{"type":"text","text":answer}],"stop_reason":"end_turn","usage":{}});
    let (r, _) = response(&msg, "role-x", &tgt(), &Calls::of(&body));
    assert_eq!(norm(&r["output"]), json!([want.clone()]));
    let chat = json!({"id":"c1","model":"up","choices":[{"index":0,"message":{"role":"assistant","content":answer},
        "finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1}});
    let (r, _) = response(&translate_response(&chat, Dialect::OpenaiChat, &tgt()), "role-x", &tgt(), &Calls::of(&body));
    assert_eq!(norm(&r["output"]), json!([want]));
}
