//! CC-4 round 2 D2: the harness rule of the content pass (§16.2) is each client's descriptor fact; the result for
//! each client is the result at the base (K-D3). Uses only the public `filter`, so it also runs at the base.

use serde_json::{json, Value};
use std::time::Duration;
use zygo_config::{ContentConfig, RedactionConfig};
use zygo_core::Agent;
use zygo_redact::cache::{CachePool, SESSION_CAP_BYTES};
use zygo_redact::content::{filter, Layout, Shape};
use zygo_redact::{compile, Compiled, SessionCache};

const PREFIXES: [&str; 6] = ["<system-reminder>", "<bash-stdout>", "<bash-stderr>", "<system-update>", "Stop hook feedback:", "SubagentStop hook feedback:"];

fn compiled() -> Compiled {
    let content = ContentConfig { replacement: "🤬".into(), allow: vec![], deny: vec!["plimsoq".into()] };
    compile(&RedactionConfig { pattern: vec![], content }).unwrap()
}
/// The body after the content pass, and whether it changed.
fn run(mut body: Value, layout: Layout, agent: Agent) -> (Value, bool) {
    let mut cache = SessionCache::new(CachePool::new(256 << 20, SESSION_CAP_BYTES));
    let f = filter(&mut body, Shape { layout, agent }, &compiled(), &mut cache, Duration::from_secs(60));
    (body, f.changed)
}
fn user(content: Value) -> Value {
    json!({"messages": [{"role": "user", "content": content}]})
}
fn text_blocks(texts: &[&str]) -> Value {
    user(Value::Array(texts.iter().map(|t| json!({"type": "text", "text": t})).collect()))
}

#[test]
fn harness_prefixes_skip_the_block_for_each_client() {
    for id in ["claude-code", "opencode"] {
        let a: Agent = id.parse().unwrap();
        for p in PREFIXES {
            let t = format!("{p} plimsoq");
            for (body, layout) in [(user(json!(t)), Layout::Messages), (text_blocks(&[&t]), Layout::Messages), (json!({"input": t}), Layout::Input)] {
                assert_eq!(run(body.clone(), layout, a), (body, false), "{id} {p} {layout:?}");
            }
        }
        // Only at the start of the text: a prefix after a space is user text.
        assert_eq!(run(user(json!(" <system-reminder> plimsoq")), Layout::Messages, a), (user(json!(" <system-reminder> 🤬")), true), "{id}");
        assert_eq!(run(user(json!("say plimsoq")), Layout::Messages, a), (user(json!("say 🤬")), true), "{id}");
    }
}

#[test]
fn harness_span_inside_a_command_message_for_each_client() {
    for id in ["claude-code", "opencode"] {
        let a: Agent = id.parse().unwrap();
        let cmd = "<command-message>m</command-message> <command-name>/x plimsoq</command-name> plimsoq";
        assert_eq!(run(text_blocks(&[cmd]), Layout::Messages, a),
                   (text_blocks(&["<command-message>m</command-message> <command-name>/x plimsoq</command-name> 🤬"]), true), "{id}");
        // Outside a `<command-message>` block the element is user text.
        assert_eq!(run(text_blocks(&["<command-name>/x plimsoq</command-name> plimsoq"]), Layout::Messages, a),
                   (text_blocks(&["<command-name>/x 🤬</command-name> 🤬"]), true), "{id}");
    }
}
