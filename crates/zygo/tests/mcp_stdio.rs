use std::io::Write;
use std::process::{Command, Stdio};
use serde_json::Value;
use zygo_core::{session_dir::SessionDir, session_view::SessionView, Agent};

mod common;

#[derive(Clone, Copy)]
enum View { None, CompactOff, Corrupt }

fn run_mcp(agent: &str, config: &str, view: View, input: &str) -> (Option<i32>, Vec<Value>, String) {
    let d = tempfile::tempdir().unwrap();
    let cfg = d.path().join("config.toml");
    std::fs::write(&cfg, config).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_zygo"));
    common::scratch_home(&mut cmd, d.path());
    cmd.args(["mcp", "--agent", agent]).env_remove("ZYGO_SESSION_DIR").current_dir(d.path());
    if !matches!(view, View::None) {
        let sd = SessionDir::new(d.path().join("s"));
        sd.create().unwrap();
        match view {
            View::CompactOff => SessionView { session: "s".into(), agent: Agent::ClaudeCode, main_role: "lead".into(), effective_role: None,
                role_prompt: None, user_prompt: None, profile: None, profile_prompt: None, work_item: None, legend: vec![],
                disabled_roles: vec![], work_units: true, results: true, compact_tool: false, navigation: false,
                trim_threshold_bytes: 16384, head_lines: 60, tail_lines: 20 }.save(&sd).unwrap(),
            _ => std::fs::write(sd.view_json(), b"{broken").unwrap(),
        }
        cmd.env("ZYGO_SESSION_DIR", &sd.root);
    }
    let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();   // dropped here: EOF
    let out = child.wait_with_output().unwrap();
    let lines = String::from_utf8(out.stdout).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    (out.status.code(), lines, String::from_utf8(out.stderr).unwrap())
}

const SESSION: &str = concat!(
    "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-06-18\"}}\n",
    "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n",
    "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}\n",
    "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/call\",\"params\":{\"name\":\"compact\",\"arguments\":{}}}\n",
);

#[test]
fn claude_code_binary_serves_compact_and_exits_zero_on_eof() {
    let (code, out, err) = run_mcp("claude-code", "", View::None, SESSION);
    assert_eq!(code, Some(0));
    assert_eq!(err, "");
    assert_eq!(out.len(), 3);
    assert_eq!(out[0]["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(out[1]["result"]["tools"][0]["name"], "compact");
    assert_eq!(out[2]["result"]["content"][0]["text"], "compaction requested");
}

#[test]
fn binary_honours_config_session_view_and_agent() {
    let (code, out, _) = run_mcp("claude-code", "[features]\ncompact_tool = false\n", View::None, SESSION);
    assert_eq!(code, Some(0));
    assert_eq!(out[1]["result"]["tools"], serde_json::json!([]));
    assert_eq!(out[2]["error"]["code"], -32602);
    let (_, out, _) = run_mcp("claude-code", "", View::CompactOff, SESSION);      // session view outranks config
    let names: Vec<_> = out[1]["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["result_view", "result_search", "result_tree", "work_begin", "work_verdict"]);     // view has results and work units on (N18, N24)
    let (_, out, _) = run_mcp("opencode", "", View::None, SESSION);
    assert_eq!(out[1]["result"]["tools"], serde_json::json!([]));
}

#[test]
fn binary_with_a_corrupt_session_view_warns_once_and_uses_config() {
    let (code, out, err) = run_mcp("claude-code", "", View::Corrupt, SESSION);
    assert_eq!(code, Some(0));
    assert_eq!(err.lines().count(), 1, "{err:?}");
    assert_eq!(out[1]["result"]["tools"][0]["name"], "compact");
}

#[test]
fn binary_without_agent_exits_2() {
    let h = tempfile::tempdir().unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_zygo"));
    common::scratch_home(&mut cmd, h.path());
    let s = cmd.arg("mcp").stdin(Stdio::null()).output().unwrap();
    assert_eq!(s.status.code(), Some(2));
    assert!(s.stdout.is_empty());
}
