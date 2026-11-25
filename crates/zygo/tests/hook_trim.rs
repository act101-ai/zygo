use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use serde_json::{json, Value};
use zygo_core::{session_dir::SessionDir, session_view::SessionView, Agent};

fn session(d: &Path) -> SessionDir {
    let sd = SessionDir::new(d.join("s")); sd.create().unwrap();
    SessionView { session: "s".into(), agent: Agent::ClaudeCode, main_role: "lead".into(), effective_role: None,
        role_prompt: None, user_prompt: None, profile: None, profile_prompt: None, work_item: None, legend: vec![],
        disabled_roles: vec![], work_units: true, results: true, compact_tool: true, navigation: false,
        trim_threshold_bytes: 16384, head_lines: 60, tail_lines: 20, ..Default::default() }.save(&sd).unwrap();
    sd
}
fn zygo(d: &Path, sd: &SessionDir, args: &[&str], input: &[u8]) -> Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_zygo")).args(args)
        .env("HOME", d).env("LOCALAPPDATA", d.join("local")).env("APPDATA", d.join("roaming"))
        .env("ZYGO_SESSION_DIR", &sd.root).env_remove("ZYGO_AGENT").current_dir(d)
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    c.stdin.take().unwrap().write_all(input).unwrap();
    c.wait_with_output().unwrap()
}
fn post_bash(stdout: &str) -> Vec<u8> {     // fixture shape of PostToolUse-Bash-main.json
    serde_json::to_vec(&json!({"hook_event_name": "PostToolUse", "tool_name": "Bash", "tool_input": {"command": "x"},
        "tool_response": {"stdout": stdout, "stderr": "", "interrupted": false, "isImage": false, "noOutputExpected": false}})).unwrap()
}
fn big() -> String { (0..5000).map(|i| format!("line {i:06} xxxxxxxx\n")).collect() }
/// The data directory the usage database lives in, per Paths::from_env.
fn data_dir(d: &Path) -> std::path::PathBuf { if cfg!(windows) { d.join("local").join("zygo") } else { d.join(".local/share/zygo") } }
/// The usage database as the launcher leaves it (O§6.5 item 3).
fn usage_ready(d: &Path) { drop(zygo_usage::Store::open(&data_dir(d).join("usage.db")).unwrap()); }

#[test] fn trim_binary_trims_bash_over_threshold() {
    let d = tempfile::tempdir().unwrap(); let sd = session(d.path()); usage_ready(d.path());
    let o = zygo(d.path(), &sd, &["hook", "trim"], &post_bash(&big()));
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&o.stderr), "");
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["hookSpecificOutput"]["hookEventName"], "PostToolUse");
    assert!(v["hookSpecificOutput"]["updatedToolOutput"]["stdout"].as_str().unwrap().contains("[zygo trimmed this output: 5000 lines, 105000 bytes;"));
    assert_eq!(std::fs::read_dir(sd.results()).unwrap().count(), 2);
    assert!(data_dir(d.path()).join("usage.db").is_file());
}
#[test] fn trim_binary_passes_small_output() {   // (reg)
    let d = tempfile::tempdir().unwrap(); let sd = session(d.path());
    let o = zygo(d.path(), &sd, &["hook", "trim"], &post_bash("small"));
    assert_eq!((o.status.code(), o.stdout.len(), o.stderr.len()), (Some(0), 0, 0));
}
#[test] fn trim_binary_store_failure_prints_one_fixed_line() {
    let d = tempfile::tempdir().unwrap(); let sd = session(d.path());
    std::fs::write(sd.results(), b"in the way").unwrap();
    let o = zygo(d.path(), &sd, &["hook", "trim"], &post_bash(&format!("SENTINEL-BODY-TEXT\n{}", big())));
    assert_eq!(o.status.code(), Some(0));
    assert!(o.stdout.is_empty());
    assert_eq!(String::from_utf8_lossy(&o.stderr), "zygo hook: cannot store the tool result\n");
}
#[test] fn trim_binary_usage_failure_keeps_view_with_one_fixed_line() {
    let d = tempfile::tempdir().unwrap(); let sd = session(d.path());
    let data = data_dir(d.path());
    std::fs::create_dir_all(data.parent().unwrap()).unwrap();
    std::fs::write(&data, b"in the way").unwrap();                    // the data directory is a file: usage open fails
    let o = zygo(d.path(), &sd, &["hook", "trim"], &post_bash(&big()));
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&o.stderr), "zygo hook: usage store unavailable\n");
    assert!(String::from_utf8_lossy(&o.stdout).contains("[zygo trimmed this output:"));
}
#[test] fn trim_binary_work_log_failure_keeps_view_with_one_fixed_line() {
    let d = tempfile::tempdir().unwrap(); let sd = session(d.path()); usage_ready(d.path());
    std::fs::write(sd.work(), b"in the way").unwrap();                 // the work path is a file: append fails
    let o = zygo(d.path(), &sd, &["hook", "trim"], &post_bash(&big()));
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&o.stderr), "zygo hook: work log unavailable\n");
    assert!(String::from_utf8_lossy(&o.stdout).contains("[zygo trimmed this output:"));
}
#[test] fn token_binary_prints_the_token() {
    let d = tempfile::tempdir().unwrap(); let sd = session(d.path());
    sd.write_token("session-token-for-tests\n").unwrap();
    let o = zygo(d.path(), &sd, &["hook", "token"], b"");
    assert_eq!((o.status.code(), String::from_utf8_lossy(&o.stdout).into_owned(), o.stderr.len()), (Some(0), "session-token-for-tests".to_string(), 0));
}
