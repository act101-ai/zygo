//! OS-28: the trim hook runs the shaping pipeline on Bash output (O§7.1-O§7.3, O§7.8-O§7.10, O§10, O§16).
mod common {
    pub mod shaping;
}
use common::shaping;
use serde_json::{json, Value};
use std::path::PathBuf;
use zygo_core::{
    Agent,
    hook::{HookInput, guarded},
    session_dir::SessionDir,
    session_view::SessionView,
};
use zygo_hooks::trim::{self, TrimOpts};

/// The masked result id.
const ZERO_ID: &str = "00000000000000000000000000";

struct Env {
    _t: tempfile::TempDir,
    dir: SessionDir,
    db: PathBuf,
}

/// A session with `results = true`, the default threshold, head and tail, and `shape`; the usage db is ready.
fn env(shape: bool) -> Env {
    let t = tempfile::tempdir().unwrap();
    let dir = SessionDir::new(t.path().join("s"));
    dir.create().unwrap();
    SessionView { session: "sess-1".into(), agent: Agent::ClaudeCode, results: true, shape, ..Default::default() }
        .save(&dir)
        .unwrap();
    let db = t.path().join("usage.db");
    drop(zygo_usage::Store::open(&db).unwrap());
    Env { _t: t, dir, db }
}

/// The hook stdout of `input` through `guarded`; the exit code is 0.
fn run(e: &Env, agent: Agent, input: &Value, opts: &TrimOpts) -> String {
    let b = serde_json::to_vec(input).unwrap();
    let (out, code) = guarded(&|b| trim::trim_in_with(&HookInput::parse(agent, b)?, &e.dir, &e.db, opts), &b);
    assert_eq!(code, 0);
    out
}

/// A Claude Code `PostToolUse` Bash input with `command`, `stdout` and `stderr`.
fn bash(command: Value, stdout: &str, stderr: &str) -> Value {
    shaping::hook_input(&shaping::Case {
        dir: PathBuf::new(),
        shaper: "none".into(),
        name: "built".into(),
        meta: json!({"command": command, "exit": 0, "event": "PostToolUse", "agent": "claude-code"}),
        stdout: stdout.as_bytes().to_vec(),
        stderr: stderr.as_bytes().to_vec(),
        must_keep: vec![],
    })
}

/// The stored result ids.
fn ids(e: &Env) -> Vec<String> {
    std::fs::read_dir(e.dir.results())
        .map(|d| d.map(|f| f.unwrap().file_name().to_string_lossy().into_owned()).filter(|n| zygo_results::is_id(n)).collect())
        .unwrap_or_default()
}

type Row = (i64, i64, Option<String>, Option<String>, Option<String>);

/// (stored_bytes, view_bytes, result_id, command, shaper) of each `trims` row, by rowid.
fn rows(e: &Env) -> Vec<Row> {
    let c = rusqlite::Connection::open(&e.db).unwrap();
    let mut q = c.prepare("SELECT stored_bytes, view_bytes, result_id, command, shaper FROM trims ORDER BY rowid").unwrap();
    q.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))).unwrap().map(Result::unwrap).collect()
}

/// Checks the envelope; returns `updatedToolOutput`.
fn resp(out: &str) -> Value {
    let v: Value = serde_json::from_str(out).unwrap();
    assert_eq!(v["hookSpecificOutput"]["hookEventName"], "PostToolUse");
    v["hookSpecificOutput"]["updatedToolOutput"].clone()
}

/// `out` with the one stored id replaced by 26 `0`.
fn masked(e: &Env, out: &str) -> String {
    let ids = ids(e);
    assert_eq!(ids.len(), 1, "{ids:?}");
    assert!(out.contains(&ids[0]));
    out.replace(&ids[0], ZERO_ID)
}

/// The same input with `shape = false`: (hook stdout with its stored id masked, stored bytes).
fn off(input: &Value) -> (String, Option<Vec<u8>>) {
    let e = env(false);
    let out = run(&e, Agent::ClaudeCode, input, &TrimOpts::default());
    match ids(&e).first() {
        Some(id) => (masked(&e, &out), Some(std::fs::read(e.dir.results().join(id)).unwrap())),
        None => (out, None),
    }
}

/// `n` lines `"\x1b[32mline {i:05}\x1b[0m xxxxxxxx\n"`, 29 bytes each, all distinct.
fn colored(n: usize) -> String {
    (0..n).map(|i| format!("\x1b[32mline {i:05}\x1b[0m xxxxxxxx\n")).collect()
}

/// The O§7.8 shaped marker of the stored raw text `raw`.
fn shaped_marker(raw: &str, steps: &str, id: &str) -> String {
    format!(
        "[zygo shaped this output: {steps}. The full output ({} lines, {} bytes) is saved as result {id}; read it with result_view, result_search or result_tree.]",
        raw.lines().count(),
        raw.len()
    )
}

#[test]
fn trim_opts_default_branch_is_po2_constant() {
    assert!(trim::PO2_STDERR_CLEARED); // true until OS-34
    let o = TrimOpts::default();
    assert_eq!(o.stderr_cleared, trim::PO2_STDERR_CLEARED);
    assert_eq!(o.db_bound, zygo_usage::store::HOOK_DB_BOUND);
    assert!(!o.shape_panic); // ZYGO_TEST_SHAPE_PANIC unset in CI
}
