//! RS-11: `text.format` `json_schema` through `POST /openai/v1/responses` (K§6.4.3 request obligation 11, stream
//! obligation 2): the Codex thread-title requests toward an Anthropic upstream (native and prompted structured output)
//! and a chat upstream, streamed and as one JSON body. Mock upstream on 127.0.0.1; no agent binary, no network.

#[path = "support/responses.rs"]
mod support;

use std::path::PathBuf;

use http::HeaderMap;
use serde_json::{json, Value};
use support::{Harness, PROVIDER, UPSTREAM_MODEL};
use zygo_config::Dialect::{self, Anthropic as A, OpenaiChat as C};
use zygo_core::Agent;
use zygo_proxy::translate::facts::SCHEMA_PROMPT;

const R: &str = "openai/v1/responses";
const SSE: &str = "text/event-stream";
const JSON: &str = "application/json";
/// PC-5's Codex thread-title requests.
const TITLE_FIXTURES: [&str; 2] = ["compaction/requests/pc8-tui-high-0028.json", "compaction/requests/pc8-tui-low-0025.json"];
const ANSWER: &str = "{\"title\":\"Fix the build\"}";

// ---- helpers (no logic under test) ----

fn start(d: Dialect) -> Harness {
    Harness::start(d, Agent::ClaudeCode, "")
}

/// `fixtures/codex/<rel>`: its `body` value.
fn record(rel: &str) -> Value {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/codex").join(rel);
    let v: Value = serde_json::from_slice(&std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))).unwrap();
    v["body"].clone()
}

fn with_stream(mut body: Value, stream: bool) -> Value {
    body["stream"] = json!(stream);
    body
}

fn hv(h: &HeaderMap, n: &str) -> String {
    h.get(n).map(|v| v.to_str().unwrap().to_string()).unwrap_or_default()
}

fn adjustments(h: &HeaderMap) -> Vec<String> {
    hv(h, "x-zygo-adjustments").split(',').filter(|s| !s.is_empty()).map(str::to_string).collect()
}

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

/// The items of the `response.output_item.done` events, without their generated `id`.
fn done_items(b: &[u8]) -> Vec<Value> {
    let evs = events(b);
    let terminals: Vec<&str> =
        evs.iter().map(|(n, _)| n.as_str()).filter(|n| matches!(*n, "response.completed" | "response.incomplete" | "response.failed")).collect();
    assert_eq!(terminals, ["response.completed"], "{}", String::from_utf8_lossy(b));
    evs.into_iter().filter(|(n, _)| n == "response.output_item.done").map(|(_, d)| without_id(d["item"].clone())).collect()
}

fn without_id(mut item: Value) -> Value {
    item.as_object_mut().unwrap().remove("id");
    item
}

/// The one output item of a whole Response object.
fn json_items(b: &[u8]) -> Vec<Value> {
    let v: Value = serde_json::from_slice(b).unwrap_or_else(|_| panic!("not JSON: {}", String::from_utf8_lossy(b)));
    assert_eq!(v["object"], "response", "{v}");
    v["output"].as_array().unwrap().iter().cloned().map(without_id).collect()
}

/// The assistant message item that carries `text` as its one `output_text` part.
fn text_item(text: &str) -> Value {
    json!({"type":"message","role":"assistant","status":"completed",
        "content":[{"type":"output_text","text":text,"annotations":[]}]})
}

fn msg_events(text: &str) -> Vec<Value> {
    let (a, b) = text.split_at(text.len() / 2);
    vec![
        json!({"type":"message_start","message":{"id":"msg_up","type":"message","role":"assistant","model":UPSTREAM_MODEL,
            "content":[],"usage":{"input_tokens":10,"output_tokens":1}}}),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":a}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":b}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":5}}),
        json!({"type":"message_stop"}),
    ]
}

fn msg_json(text: &str) -> Value {
    json!({"id":"msg_up","type":"message","role":"assistant","model":UPSTREAM_MODEL,
        "content":[{"type":"text","text":text}],"stop_reason":"end_turn","stop_sequence":null,
        "usage":{"input_tokens":10,"output_tokens":5}})
}

fn chat_chunks(text: &str) -> Vec<Value> {
    let (a, b) = text.split_at(text.len() / 2);
    let chunk = |delta: Value, finish: Value| {
        json!({"id":"chatcmpl-1","object":"chat.completion.chunk","model":UPSTREAM_MODEL,
            "choices":[{"index":0,"delta":delta,"finish_reason":finish}]})
    };
    vec![
        chunk(json!({"role":"assistant","content":a}), Value::Null),
        chunk(json!({"content":b}), Value::Null),
        chunk(json!({}), json!("stop")),
        json!({"id":"chatcmpl-1","object":"chat.completion.chunk","model":UPSTREAM_MODEL,"choices":[],
            "usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}}),
    ]
}

fn chat_json(text: &str) -> Value {
    json!({"id":"chatcmpl-1","object":"chat.completion","model":UPSTREAM_MODEL,
        "choices":[{"index":0,"message":{"role":"assistant","content":text},"finish_reason":"stop"}],
        "usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}})
}

/// Posts `body` streamed (`stream = true`) or not, with the upstream answering `text` in the Messages form.
fn post_anthropic(h: &Harness, body: &Value, stream: bool, text: &str) -> (HeaderMap, Vec<Value>) {
    if stream {
        h.reply_with(200, SSE, msg_sse(&msg_events(text)));
    } else {
        h.reply_with(200, JSON, msg_json(text).to_string().into_bytes());
    }
    let (s, hd, b) = h.post(R, &with_stream(body.clone(), stream));
    assert_eq!(s, 200, "{}", String::from_utf8_lossy(&b));
    let items = if stream { done_items(&b) } else { json_items(&b) };
    (hd, items)
}

fn post_chat(h: &Harness, body: &Value, stream: bool, text: &str) -> (HeaderMap, Vec<Value>) {
    if stream {
        h.reply_with(200, SSE, chat_sse(&chat_chunks(text)));
    } else {
        h.reply_with(200, JSON, chat_json(text).to_string().into_bytes());
    }
    let (s, hd, b) = h.post(R, &with_stream(body.clone(), stream));
    assert_eq!(s, 200, "{}", String::from_utf8_lossy(&b));
    let items = if stream { done_items(&b) } else { json_items(&b) };
    (hd, items)
}

// ---- tests ----

#[test] // obligation 11: Anthropic, structured output native; the string lengths move into `description`
fn text_format_json_schema_route_anthropic_native() {
    let h = start(A);
    for rel in TITLE_FIXTURES {
        let body = record(rel);
        let mut want_schema = body["text"]["format"]["schema"].clone();
        want_schema["properties"]["title"] = json!({"type":"string","description":"{\"minLength\":1,\"maxLength\":36}"});
        for stream in [true, false] {
            let (hd, items) = post_anthropic(&h, &body, stream, ANSWER);
            assert_eq!(h.upstream_paths().last().map(String::as_str), Some("/v1/messages"), "{rel}");
            let up = h.upstream_bodies().pop().unwrap();
            assert_eq!(up["output_config"]["format"], json!({"type":"json_schema","schema":want_schema}), "{rel} {stream}");
            assert!(up.get("tools").is_none() && up.get("tool_choice").is_none() && up.get("text").is_none(), "{rel}: {up}");
            let adj = adjustments(&hd);
            assert!(adj.iter().any(|a| a == "schema:string_length:1"), "{rel} {stream}: {adj:?}");
            assert!(adj.iter().any(|a| a == "text_format_dropped:strict"), "{rel} {stream}: {adj:?}");
            assert_eq!(items, [text_item(ANSWER)], "{rel} {stream}");
        }
    }
    let rows = h.usage_rows();
    assert_eq!(rows.len(), 4);
    assert!(rows.iter().all(|r| r.status == 200 && r.provider == PROVIDER), "{rows:?}");
}

#[test] // obligation 11: chat upstream; `response_format` carries the schema; injected nulls are removed on the way back
fn text_format_json_schema_route_chat() {
    let h = start(C);
    for rel in TITLE_FIXTURES {
        let body = record(rel);
        let schema = body["text"]["format"]["schema"].clone();
        for stream in [true, false] {
            let (_, items) = post_chat(&h, &body, stream, ANSWER);
            assert_eq!(h.upstream_paths().last().map(String::as_str), Some("/v1/chat/completions"), "{rel}");
            let up = h.upstream_bodies().pop().unwrap();
            assert_eq!(up["response_format"], json!({"type":"json_schema","json_schema":{"name":"output","schema":schema}}), "{rel}");
            assert_eq!(items, [text_item(ANSWER)], "{rel} {stream}");
        }
    }
    // An optional property: the OpenAI rules make it required and nullable; the null the model sends is removed.
    let mut body = record(TITLE_FIXTURES[0]);
    body["text"]["format"]["schema"] = json!({"type":"object","properties":{"title":{"type":"string"},"note":{"type":"string"}},
        "required":["title"],"additionalProperties":false});
    for stream in [true, false] {
        let (hd, items) = post_chat(&h, &body, stream, "{\"title\":\"x\",\"note\":null}");
        let up = h.upstream_bodies().pop().unwrap();
        let sent = &up["response_format"]["json_schema"]["schema"];
        assert_eq!(sent["required"], json!(["title", "note"]), "{up}");
        let adj = adjustments(&hd);
        assert!(adj.iter().any(|a| a.starts_with("schema:optional_property")), "{adj:?}");
        assert_eq!(items, [text_item("{\"title\":\"x\"}")], "{stream}");
    }
}

#[test] // obligation 11 and §7.6 `structured_output = none`: the schema goes into the system text
fn text_format_json_schema_route_prompted() {
    // The harness serves one model with `structured_output = "native"`. A second harness adds a model with
    // `structured_output = "none"` whose provider is the first harness's mock upstream (its port from the `host`
    // header that the first request recorded).
    let up = start(A);
    up.post(R, &record(TITLE_FIXTURES[0]));
    let host = hv(&up.upstream_headers().pop().unwrap(), "host");
    let extra = format!(
        "[role.rs11]\nmodel = \"gpt-none\"\n\n[[model]]\nid = \"gpt-none\"\n[model.facts]\nreasoning = \"always\"\n\
         effort_levels = [\"low\", \"medium\", \"high\"]\nmax_output_tokens = 32000\ncontext_window = 200000\n\
         sampling = true\nvision = true\nstructured_output = \"none\"\nforced_tool_choice = true\n\n\
         [[model.provider]]\nname = \"p-none\"\nbase_url = \"http://{host}\"\ndialects = [\"anthropic\"]\n\
         upstream_model = \"up-none\"\nkey = \"keychain:k-rs\"\n"
    );
    let h = Harness::start(A, Agent::ClaudeCode, &extra);
    for rel in TITLE_FIXTURES {
        let mut body = record(rel);
        body["model"] = json!("gpt-none");
        let schema = body["text"]["format"]["schema"].clone();
        for stream in [true, false] {
            if stream {
                up.reply_with(200, SSE, msg_sse(&msg_events(ANSWER)));
            } else {
                up.reply_with(200, JSON, msg_json(ANSWER).to_string().into_bytes());
            }
            let (s, hd, b) = h.post(R, &with_stream(body.clone(), stream));
            assert_eq!(s, 200, "{}", String::from_utf8_lossy(&b));
            let sent = up.upstream_bodies().pop().unwrap();
            assert_eq!(sent["model"], "up-none", "{sent}");
            assert!(sent.get("output_config").and_then(|o| o.get("format")).is_none(), "{rel}: {sent}");
            let system = sent["system"].as_str().unwrap_or_else(|| panic!("{rel}: system {sent}"));
            assert!(system.ends_with(&format!("{SCHEMA_PROMPT}{schema}")), "{rel}: {system}");
            let adj = adjustments(&hd);
            assert!(adj.iter().any(|a| a == "structured_output_prompted"), "{rel}: {adj:?}");
            let items = if stream { done_items(&b) } else { json_items(&b) };
            assert_eq!(items, [text_item(ANSWER)], "{rel} {stream}");
        }
    }
    assert!(h.upstream_bodies().is_empty(), "the second harness's own upstream is unused");
}
