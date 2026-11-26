//! RS-6: cache markers at forward time on the Anthropic upstream body of a Responses client (K§6.4.2 row cache
//! markers, K§11 item 19, K-D48, base §10.4). Mock upstream on 127.0.0.1; no agent binary, no network.

#[path = "support/responses.rs"]
mod support;

use serde_json::{json, Value};
use support::{codex_request, Harness, UPSTREAM_MODEL};
use zygo_config::Dialect::{Anthropic as A, Openai as O, OpenaiChat as C};
use zygo_core::Agent;
use zygo_proxy::translate::facts::SCHEMA_PROMPT;

const R: &str = "openai/v1/responses";
const ON: &str = "[cache]\ninsert_markers.claude-code = true\n";
const SSE: &str = "text/event-stream";
const JSON: &str = "application/json";

fn mark() -> Value {
    json!({"type":"ephemeral","ttl":"1h"})
}

/// Occurrences of `pat` in `raw`.
fn count(raw: &[u8], pat: &[u8]) -> usize {
    raw.windows(pat.len()).filter(|w| *w == pat).count()
}

/// The upstream body (parsed, raw) of one `first-turn` request through `h`. The mock's default reply (400) is not
/// under test.
fn sent(h: &Harness) -> (Value, Vec<u8>) {
    h.post(R, &codex_request("first-turn"));
    (h.upstream_bodies().remove(0), h.upstream_raw().remove(0))
}

/// K§11 item 19: on for the session's client → the string system is one marked text block, the last tool is marked,
/// every other value is the unmarked body's; two markers, not more (review focus 12).
#[test]
fn anthropic_upstream_markers_when_on() {
    let (off, _) = sent(&Harness::start(A, Agent::ClaudeCode, ""));
    let (on, raw) = sent(&Harness::start(A, Agent::ClaudeCode, ON));
    let mark = json!({"type":"ephemeral","ttl":"1h"});
    let mut want = off.clone();
    want["system"] = json!([{"type":"text","text": off["system"].as_str().expect("first-turn has developer text"),
                             "cache_control": mark}]);
    let n = want["tools"].as_array().expect("first-turn has tools").len();
    want["tools"][n - 1]["cache_control"] = mark;
    assert_eq!(on, want);
    assert_eq!(count(&raw, b"\"cache_control\""), 2);
}

/// Off (Claude Code's default) → no marker.
#[test]
fn anthropic_upstream_no_markers_when_off() {
    let (_, raw) = sent(&Harness::start(A, Agent::ClaudeCode, ""));
    assert_eq!(count(&raw, b"cache_control"), 0);
}

/// The key is read per client: OpenCode's default `true` marks.
#[test]
fn opencode_default_marks_anthropic_upstream() {
    let (on, raw) = sent(&Harness::start(A, Agent::OpenCode, ""));
    assert_eq!(on["system"][0]["cache_control"], json!({"type":"ephemeral","ttl":"1h"}));
    assert_eq!(count(&raw, b"\"cache_control\""), 2);
}

/// A chat upstream gets none, even with the key on (OpenCode's default).
#[test]
fn chat_upstream_no_markers() {
    let (_, raw) = sent(&Harness::start(C, Agent::OpenCode, ""));
    assert_eq!(count(&raw, b"cache_control"), 0);
}

/// Round 1 review F2 (a), §7.3: the `openai` candidate answers 5xx; its body has no marker, the `anthropic` fallback's
/// body has both.
#[test]
fn fallback_to_anthropic_gets_markers() {
    let second = format!(
        "[[model.provider]]\nname = \"p-rs-a\"\nbase_url = \"{{UPSTREAM}}\"\ndialects = [\"anthropic\"]\n\
         upstream_model = \"{UPSTREAM_MODEL}\"\nkey = \"keychain:k-rs\"\n\n{ON}"
    );
    let h = Harness::start(O, Agent::ClaudeCode, &second);
    h.reply_seq(vec![(500, JSON, br#"{"error":{"type":"server_error","message":"busy"}}"#.to_vec())]);
    h.post(R, &codex_request("first-turn"));
    assert_eq!(h.upstream_paths(), ["/v1/responses", "/v1/messages"]);
    let (ups, raw) = (h.upstream_bodies(), h.upstream_raw());
    assert_eq!(count(&raw[0], b"cache_control"), 0);
    assert_eq!(ups[1]["system"][0]["cache_control"], mark(), "{}", ups[1]);
    assert_eq!(count(&raw[1], b"\"cache_control\""), 2);
}

/// Round 1 review F2 (b): a streamed request answered 200 with Messages events gets the markers upstream.
#[test]
fn streamed_request_gets_markers() {
    let h = Harness::start(A, Agent::ClaudeCode, ON);
    let evs = [
        json!({"type":"message_start","message":{"id":"msg_up","type":"message","role":"assistant","model":UPSTREAM_MODEL,
            "content":[],"usage":{"input_tokens":10,"output_tokens":1}}}),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hi"}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":5}}),
        json!({"type":"message_stop"}),
    ];
    let sse: Vec<u8> = evs.iter().flat_map(|e| format!("event: {}\ndata: {e}\n\n", e["type"].as_str().unwrap()).into_bytes()).collect();
    h.reply_with(200, SSE, sse);
    let mut req = codex_request("first-turn");
    req["stream"] = json!(true);
    let (s, _, b) = h.post(R, &req);
    assert_eq!(s.as_u16(), 200, "{}", String::from_utf8_lossy(&b));
    let (up, raw) = (h.upstream_bodies().remove(0), h.upstream_raw().remove(0));
    assert_eq!(up["stream"], json!(true));
    assert_eq!(up["system"][0]["cache_control"], mark(), "{up}");
    assert_eq!(count(&raw, b"\"cache_control\""), 2);
}

/// Round 1 review F2 (c), §7.6 `structured_output = "none"`: the schema text is inside the one marked system block.
#[test]
fn prompted_schema_inside_marked_system_block() {
    let extra = format!(
        "[[model]]\nid = \"gpt-none\"\n[model.facts]\nreasoning = \"always\"\n\
         effort_levels = [\"low\", \"medium\", \"high\"]\nmax_output_tokens = 32000\ncontext_window = 200000\n\
         sampling = true\nvision = true\nstructured_output = \"none\"\nforced_tool_choice = true\n\n\
         [[model.provider]]\nname = \"p-none\"\nbase_url = \"{{UPSTREAM}}\"\ndialects = [\"anthropic\"]\n\
         upstream_model = \"up-none\"\nkey = \"keychain:k-rs\"\n\n{ON}"
    );
    let h = Harness::start(A, Agent::ClaudeCode, &extra);
    let schema = json!({"type":"object","properties":{"title":{"type":"string"}},"required":["title"],"additionalProperties":false});
    let mut req = codex_request("first-turn");
    req["model"] = json!("gpt-none");
    req["text"] = json!({"format":{"type":"json_schema","name":"t","schema":schema,"strict":true}});
    h.post(R, &req);
    let (up, raw) = (h.upstream_bodies().remove(0), h.upstream_raw().remove(0));
    assert_eq!(up["model"], "up-none", "{up}");
    let system = up["system"].as_array().unwrap_or_else(|| panic!("system {up}"));
    assert_eq!(system.len(), 1, "{up}");
    assert!(system[0]["text"].as_str().unwrap().ends_with(&format!("{SCHEMA_PROMPT}{schema}")), "{up}");
    assert_eq!(system[0]["cache_control"], mark());
    assert_eq!(count(&raw, b"\"cache_control\""), 2);
}
