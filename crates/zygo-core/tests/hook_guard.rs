use serde_json::json;
use zygo_core::hook::{guarded, HookInput, HookOutput};
use zygo_core::Agent;

fn parse_then_json(b: &[u8]) -> anyhow::Result<HookOutput> {
    HookInput::parse(Agent::ClaudeCode, b)?;
    Ok(HookOutput::Json(json!({"x":1})))
}

#[test]
fn not_json() {
    assert_eq!(guarded(&parse_then_json, b"not json"), (String::new(), 0));
}

#[test]
fn empty() {
    assert_eq!(guarded(&parse_then_json, b""), (String::new(), 0));
}

#[test]
fn newer_version() {
    let i = HookInput::parse(
        Agent::ClaudeCode,
        br#"{"hook_event_name":"PreToolUse","tool_name":"Edit","future_field":{"a":1}}"#,
    )
    .unwrap();
    assert_eq!(i.tool_name.as_deref(), Some("Edit"));
    assert!(!i.has_agent_id);
}

#[test]
fn agent_id_null() {
    let i = HookInput::parse(Agent::ClaudeCode, br#"{"agent_id":null}"#).unwrap();
    assert!(i.has_agent_id);
    assert_eq!(i.agent_id, None);
}

#[test]
fn panic_is_contained() {
    assert_eq!(guarded(&|_| panic!("boom"), b"{}"), (String::new(), 0));
}
