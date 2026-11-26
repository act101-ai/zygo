//! RS-6: cache markers at forward time on the Anthropic upstream body of a Responses client (K§6.4.2 row cache
//! markers, K§11 item 19, K-D48, base §10.4). Mock upstream on 127.0.0.1; no agent binary, no network.

#[path = "support/responses.rs"]
mod support;

use serde_json::{json, Value};
use support::{codex_request, Harness};
use zygo_config::Dialect::{Anthropic as A, OpenaiChat as C};
use zygo_core::Agent;

const R: &str = "openai/v1/responses";
const ON: &str = "[cache]\ninsert_markers.claude-code = true\n";

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
