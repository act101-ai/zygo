//! SX-3: `zygo hook pre-compact` sets the per-subagent re-inject flag (session text design T§9.2 item 1, T§14
//! item 2, T§17 check 7). No test spawns `claude` or `opencode` or reads a clock; the only child process is this
//! test binary.

use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::{json, Value};
use zygo_core::{
    hook::{guarded, HookInput},
    session_dir::SessionDir,
    Agent,
};
use zygo_hooks::pre_compact;

/// The §10.6 text with no open unit (`docs/specs/2026-11-18-zygo-design.md:520`), byte for byte as
/// `tests/session_text.rs:31`.
const KEEP_NO_UNIT: &str = "Keep in the summary: the plan or tracker path and the current step; the pending briefs and what each subagent reported; the result ids zygo stored (result_view reads them again). Drop: tool output that is in the result store; file contents already edited and committed.";
const ID: &str = "a1a80dd100f197242";
const CHILD: &str = "ZYGO_SX3_C6_CHILD";

fn fixture(name: &str) -> Value {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../fixtures/claude-code/hooks/{name}.json"));
    serde_json::from_slice(&std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))).unwrap()
}

fn with(mut v: Value, key: &str, val: Value) -> Value {
    v.as_object_mut().unwrap().insert(key.into(), val);
    v
}

fn input(v: &Value) -> HookInput {
    HookInput::parse(Agent::ClaudeCode, &serde_json::to_vec(v).unwrap()).unwrap()
}

fn sd() -> (tempfile::TempDir, SessionDir) {
    let t = tempfile::tempdir().unwrap();
    let d = SessionDir::new(t.path().join("s"));
    d.create().unwrap();
    (t, d)
}

fn run(d: &SessionDir, v: &Value) -> String {
    pre_compact::run_with(&input(v), Some(d)).unwrap().to_stdout()
}

#[test]
fn pre_compact_sub_prints_text_and_creates_flag() {
    // T§17 check 7
    let (_t, d) = sd();
    let sub = fixture("PreCompact-sub");
    assert_eq!((&sub["agent_id"], &sub["agent_type"]), (&json!(ID), &json!("claude")));
    assert_eq!(d.reinject(), d.root.join("reinject"));
    assert_eq!(run(&d, &sub), KEEP_NO_UNIT);
    let f = d.root.join("reinject").join(ID);
    let m = std::fs::metadata(&f).unwrap();
    assert!(m.is_file());
    assert_eq!(m.len(), 0);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(m.permissions().mode() & 0o777, 0o600);
        assert_eq!(std::fs::metadata(d.reinject()).unwrap().permissions().mode() & 0o777, 0o700);
    }
    // A second compaction of the same subagent: same text, still one flag.
    assert_eq!(run(&d, &sub), KEEP_NO_UNIT);
    assert_eq!(std::fs::read_dir(d.reinject()).unwrap().count(), 1);
}

#[test]
fn pre_compact_main_creates_no_flag() {
    let (_t, d) = sd();
    assert_eq!(run(&d, &fixture("PreCompact-main")), KEEP_NO_UNIT);
    assert!(!d.reinject().exists());
}

#[test]
fn pre_compact_zygo_type_creates_no_flag() {
    let (_t, d) = sd();
    assert_eq!(run(&d, &with(fixture("PreCompact-sub"), "agent_type", json!("zygo-read"))), KEEP_NO_UNIT);
    assert!(!d.reinject().exists());
}

#[test]
fn pre_compact_bad_agent_id_creates_no_file() {
    let (_t, d) = sd();
    for id in [json!("../x"), json!(""), json!("a".repeat(65)), json!("a/b"), json!(7), Value::Null] {
        assert_eq!(run(&d, &with(fixture("PreCompact-sub"), "agent_id", id.clone())), KEEP_NO_UNIT, "{id}");
        assert!(!d.reinject().exists(), "{id}");
        assert!(!d.root.join("x").exists(), "{id}");
    }
}

#[test]
fn pre_compact_flag_path_checks_the_id() {
    let (_t, d) = sd();
    for ok in ["a", ID, "A-z_9", &"a".repeat(64)] {
        assert_eq!(pre_compact::flag_path(&d, ok), Some(d.reinject().join(ok)), "{ok}");
    }
    for bad in ["", "../x", "..", "a/b", "a\\b", "a b", "a.b", "é", &"a".repeat(65)] {
        assert_eq!(pre_compact::flag_path(&d, bad), None, "{bad}");
    }
}

#[test]
fn pre_compact_flag_create_failure_still_prints_text() {
    // T§14 item 2: a regular file where `reinject/` must be makes the create fail on every OS.
    let (t, d) = sd();
    std::fs::write(d.reinject(), "").unwrap();
    let o = Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "child_flag_blocked", "--nocapture", "--test-threads=1"])
        .env(CHILD, t.path())
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(o.status.success(), "{o:?}");
    assert!(String::from_utf8(o.stdout).unwrap().contains("child_flag_blocked ... ok"));
    let err = String::from_utf8(o.stderr).unwrap();
    let flag = d.reinject().join(ID);
    assert!(err.starts_with(&format!("zygo hook: pre-compact: cannot create {}: ", flag.display())), "{err:?}");
    assert!(err.ends_with('\n') && err.lines().count() == 1, "{err:?}");
    assert!(std::fs::metadata(d.reinject()).unwrap().is_file());
}

/// Child half of `pre_compact_flag_create_failure_still_prints_text`; returns at once without `CHILD`.
#[test]
#[ignore]
fn child_flag_blocked() {
    let Some(root) = std::env::var_os(CHILD) else { return };
    let d = SessionDir::new(Path::new(&root).join("s"));
    let stdin = serde_json::to_vec(&fixture("PreCompact-sub")).unwrap();
    let got = guarded(&|b| pre_compact::run_with(&HookInput::parse(Agent::ClaudeCode, b)?, Some(&d)), &stdin);
    assert_eq!(got, (KEEP_NO_UNIT.to_string(), 0));
}
