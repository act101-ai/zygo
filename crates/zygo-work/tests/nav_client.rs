use std::path::Path;

use serde_json::{json, Value};
use zygo_config::Merged;
use zygo_core::hook::HookInput;
use zygo_core::session_dir::SessionDir;
use zygo_core::Agent;
use zygo_work::hooks::{self, HookCtx};
use zygo_work::{log, WorkItem};

fn no_config(_: &Path) -> Option<Merged> { None }
fn no_item(_: &Path, _: &Merged) -> Option<WorkItem> { None }
fn no_warn(_: &str) {}

/// True when one `PostToolUse` call of `tool` with `{"path": "src/lib.rs"}`, in a session of the client `id`,
/// writes a `navigated` line to the lead's work log.
fn navigated(id: &str, tool: &str) -> bool {
    let t = tempfile::tempdir().unwrap();
    let sd = SessionDir::new(t.path().join("S1"));
    sd.create().unwrap();
    let ctx = HookCtx { session_dir: &sd, session: "S1".into(), usage_db: t.path().join("usage.db"), now: 1000,
        config: &no_config, item: &no_item, warn: &no_warn, temp_roots: vec![] };
    let v = json!({"session_id": "S1", "hook_event_name": "PostToolUse", "tool_name": tool,
        "tool_input": {"path": "src/lib.rs"}, "tool_response": {}, "cwd": t.path().to_str().unwrap()});
    let input = HookInput::parse(id.parse::<Agent>().unwrap(), v.to_string().as_bytes()).unwrap();
    hooks::post_tool_with(&input, &ctx).unwrap();
    std::fs::read_to_string(log::log_path(&sd.work(), "main")).unwrap_or_default().lines()
        .any(|l| serde_json::from_str::<Value>(l).unwrap()["t"] == "navigated")
}

#[test]
fn navigation_reads_the_client_tool_names() {
    for (id, tool, want) in [
        ("claude-code", "mcp__act-lite__skeleton", true),
        ("claude-code", "mcp__plugin_act101_act-lite__symbols_batch", true),
        ("claude-code", "act-lite_skeleton", false),
        ("opencode", "act-lite_skeleton", true),
        ("opencode", "act101-act-lite_symbols_batch", true),
        ("opencode", "act-lite_repo_outline", true),
        ("opencode", "mcp__act-lite__skeleton", false),
        ("opencode", "act-lite__skeleton", false),
        ("opencode", "other_skeleton", false),
    ] {
        assert_eq!(navigated(id, tool), want, "{id} {tool}");
    }
}
