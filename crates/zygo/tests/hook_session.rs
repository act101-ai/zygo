use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::Value;
use zygo_core::{session_dir::SessionDir, session_view::SessionView, Agent};

const KEEP: &str = "Keep in the summary:";

fn fixture(name: &str) -> Vec<u8> {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/claude-code/hooks").join(name);
    std::fs::read(p).unwrap()
}
fn session(home: &Path) -> SessionDir {
    std::fs::write(home.join("config.toml"), "").unwrap();
    let sd = SessionDir::new(home.join("s"));
    sd.create().unwrap();
    SessionView {
        session: "S1".into(),
        agent: Agent::ClaudeCode,
        main_role: "lead".into(),
        effective_role: None,
        role_prompt: Some("ROLE".into()),
        user_prompt: None,
        profile: None,
        profile_prompt: None,
        work_item: None,
        legend: vec![],
        disabled_roles: vec![],
        work_units: true,
        results: true,
        compact_tool: true,
        navigation: false,
        trim_threshold_bytes: 16384,
        head_lines: 60,
        tail_lines: 20,
    }
    .save(&sd)
    .unwrap();
    sd
}
fn run(home: &Path, name: &str, sd: Option<&SessionDir>, stdin: &[u8]) -> (Option<i32>, String, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_zygo"));
    cmd.args(["hook", name]);
    cmd.env("HOME", home).env("LOCALAPPDATA", home).env("APPDATA", home).env("ZYGO_CONFIG", home.join("config.toml"))
        .env_remove("ZYGO_AGENT").current_dir(home);
    match sd {
        Some(s) => cmd.env("ZYGO_SESSION_DIR", &s.root),
        None => cmd.env_remove("ZYGO_SESSION_DIR"),
    };
    let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(stdin).unwrap();
    let out = child.wait_with_output().unwrap();
    (out.status.code(), String::from_utf8(out.stdout).unwrap(), String::from_utf8(out.stderr).unwrap())
}
fn assert_context(out: (Option<i32>, String, String)) {
    assert_eq!(out.0, Some(0));
    let v: Value = serde_json::from_str(&out.1).unwrap();
    assert!(!v["hookSpecificOutput"]["additionalContext"].as_str().unwrap().is_empty());
    assert_eq!(out.2, "");
}
fn assert_keep(out: (Option<i32>, String, String)) -> String {
    assert_eq!(out.0, Some(0));
    assert!(out.1.starts_with(KEEP), "{}", out.1);
    assert_eq!(out.2, "");
    out.1
}

#[test]
fn binary_session_hooks_never_break_the_agent() {
    let start = fixture("SessionStart-main.json");
    let compact = fixture("PreCompact-main.json");

    // good session
    let d = tempfile::tempdir().unwrap();
    let sd = session(d.path());
    assert_context(run(d.path(), "session-start", Some(&sd), &start));
    assert_keep(run(d.path(), "pre-compact", Some(&sd), &compact));

    // broken view.json
    let d = tempfile::tempdir().unwrap();
    let sd = session(d.path());
    std::fs::write(sd.view_json(), "{broken").unwrap();
    let (code, out, err) = run(d.path(), "session-start", Some(&sd), &start);
    assert_eq!((code, out.as_str()), (Some(0), ""));
    assert_eq!(err.lines().count(), 1);
    assert!(err.starts_with("zygo hook: ") && err.ends_with('\n'), "{err}");
    assert_keep(run(d.path(), "pre-compact", Some(&sd), &compact));

    // broken begin.json
    let d = tempfile::tempdir().unwrap();
    let sd = session(d.path());
    std::fs::create_dir_all(sd.work()).unwrap();
    std::fs::write(sd.work().join("main.begin.json"), "{broken").unwrap();
    let t = assert_keep(run(d.path(), "pre-compact", Some(&sd), &compact));
    assert!(!t.contains("every open unit"));
    assert_context(run(d.path(), "session-start", Some(&sd), &start));

    // work is a regular file
    let d = tempfile::tempdir().unwrap();
    let sd = session(d.path());
    std::fs::write(sd.work(), "x").unwrap();
    assert_eq!(
        run(d.path(), "pre-compact", Some(&sd), &compact),
        (Some(0), String::new(), "zygo hook: cannot read the work directory\n".to_string())
    );
    assert_context(run(d.path(), "session-start", Some(&sd), &start));

    // no session directory
    let d = tempfile::tempdir().unwrap();
    session(d.path());
    assert_eq!(run(d.path(), "session-start", None, &start), (Some(0), String::new(), String::new()));
    assert_keep(run(d.path(), "pre-compact", None, &compact));

    // subagent input
    let d = tempfile::tempdir().unwrap();
    let sd = session(d.path());
    assert_eq!(
        run(d.path(), "session-start", Some(&sd), &fixture("SessionStart-sub.json")),
        (Some(0), String::new(), String::new())
    );
}
