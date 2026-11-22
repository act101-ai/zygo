use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{json, Value};
use zygo_core::{session_dir::SessionDir, session_view::SessionView, Agent};
use zygo_usage::{Filter, Store};

const ZSUB: &str = "acfd66ba917fc3494";

fn fixture(name: &str) -> Value {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/claude-code/hooks").join(name);
    serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap()
}
fn session(home: &Path) -> SessionDir {
    std::fs::write(home.join("config.toml"), "").unwrap();
    let sd = SessionDir::new(home.join("s"));
    sd.create().unwrap();
    SessionView { session: "S1".into(), agent: Agent::ClaudeCode, main_role: "lead".into(), effective_role: None, role_prompt: None,
        user_prompt: None, profile: None, profile_prompt: None, work_item: None, legend: vec![], disabled_roles: vec![],
        work_units: true, results: true, compact_tool: true, navigation: false,
        trim_threshold_bytes: 16384, head_lines: 60, tail_lines: 20, ..Default::default() }.save(&sd).unwrap();
    sd
}
fn hook(home: &Path, sd: &SessionDir, event: Option<&str>, stdin: &[u8]) -> (Option<i32>, String, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_zygo"));
    cmd.args(["hook", "work"]);
    if let Some(e) = event { cmd.args(["--event", e]); }
    cmd.env("HOME", home).env("LOCALAPPDATA", home).env("APPDATA", home).env("ZYGO_CONFIG", home.join("config.toml"))
        .env("ZYGO_SESSION_DIR", &sd.root).env_remove("ZYGO_AGENT").current_dir(home);
    let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(stdin).unwrap();
    let out = child.wait_with_output().unwrap();
    (out.status.code(), String::from_utf8(out.stdout).unwrap(), String::from_utf8(out.stderr).unwrap())
}
fn usage_db(home: &Path) -> PathBuf {
    if cfg!(windows) { home.join("zygo").join("usage.db") } else { home.join(".local/share/zygo/usage.db") }
}
fn agent(mut v: Value, home: &Path) -> Value {
    v["agent_id"] = json!(ZSUB);
    v["agent_type"] = json!("zygo-code");
    v["cwd"] = json!(home.to_str().unwrap());
    v
}

#[test]
fn binary_records_begin_call_and_stop() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path();
    let sd = session(home);
    let mut begin = agent(fixture("PreToolUse-Bash-sub.json"), home);
    begin["tool_name"] = json!("mcp__zygo__work_begin");
    begin["tool_input"] = json!({"unit": "beads:bd-a1b2", "round": 1, "check": "cargo test -p demo", "files": ["src/"]});
    let quiet = (Some(0), String::new(), String::new());
    assert_eq!(hook(home, &sd, Some("begin"), begin.to_string().as_bytes()), quiet);
    let post = agent(fixture("PostToolUse-Bash-sub.json"), home);
    assert_eq!(hook(home, &sd, None, post.to_string().as_bytes()), quiet);
    let stop = fixture("SubagentStop-zygo-code-sub.json");
    assert_eq!(hook(home, &sd, None, stop.to_string().as_bytes()), quiet);
    let log = std::fs::read_to_string(sd.work().join(format!("{ZSUB}.jsonl"))).unwrap();
    let kinds: Vec<String> = log.lines().map(|l| serde_json::from_str::<Value>(l).unwrap()["t"].as_str().unwrap().to_string()).collect();
    assert_eq!(kinds, ["begin", "levels", "call", "guard"]);
    let rows = Store::open(&usage_db(home)).unwrap().work_units(&Filter::default()).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0].unit.as_str(), rows[0].session.as_str(), rows[0].agent_id.as_str(), rows[0].tool_calls), ("beads:bd-a1b2", "S1", ZSUB, 1));
}

#[test]
fn binary_exits_0_with_one_stderr_line_on_bad_input() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path();
    let sd = session(home);
    for (ev, bytes) in [(None, &b"not json"[..]), (Some("begin"), &b""[..])] {
        let (code, out, err) = hook(home, &sd, ev, bytes);
        assert_eq!((code, out.as_str()), (Some(0), ""));
        assert_eq!(err.lines().count(), 1);
        assert!(err.starts_with("zygo hook: "), "{err}");
    }
    assert!(std::fs::read_dir(sd.work()).map(|d| d.count() == 0).unwrap_or(true));
}

#[test]
fn binary_unparseable_begin_json_is_one_stderr_line() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path();
    let sd = session(home);
    std::fs::create_dir_all(sd.work()).unwrap();
    std::fs::write(sd.work().join(format!("{ZSUB}.begin.json")), b"{garbage").unwrap();
    let post = agent(fixture("PostToolUse-Bash-sub.json"), home);
    let (code, out, err) = hook(home, &sd, None, post.to_string().as_bytes());
    assert_eq!((code, out.as_str()), (Some(0), ""));
    assert_eq!(err, format!("zygo hook: work: {ZSUB}.begin.json does not parse; treated as no open unit\n"));
    let log = std::fs::read_to_string(sd.work().join(format!("{ZSUB}.jsonl"))).unwrap();
    assert_eq!(log.lines().map(|l| serde_json::from_str::<Value>(l).unwrap()["t"].as_str().unwrap().to_string()).collect::<Vec<_>>(), ["call"]);
}

#[test]
fn binary_denies_work_verdict_inside_a_subagent() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path();
    let sd = session(home);
    let mut v = agent(fixture("PreToolUse-Bash-sub.json"), home);
    v["tool_name"] = json!("mcp__zygo__work_verdict");
    v["tool_input"] = json!({"unit": "beads:bd-a1b2", "round": 1, "verdict": "keep"});
    let (code, out, err) = hook(home, &sd, Some("verdict"), v.to_string().as_bytes());
    assert_eq!((code, err.as_str()), (Some(0), ""));
    let out: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(out["hookSpecificOutput"]["permissionDecision"], "deny");
    assert_eq!(out["hookSpecificOutput"]["permissionDecisionReason"],
               "zygo: work_verdict is for the lead only. Report your result to the lead; the lead records the verdict.");
    let o = v.as_object_mut().unwrap();
    o.remove("agent_id");
    o.remove("agent_type");
    assert_eq!(hook(home, &sd, Some("verdict"), v.to_string().as_bytes()), (Some(0), String::new(), String::new()));
}
