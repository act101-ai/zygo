//! §10.1 Shaping, O§9: with `[results] shape = false`, a Bash output at or below the trim threshold passes unchanged
//! on each client: the hook prints nothing, stores nothing and writes no `trims` row, while `shape = true` shapes the
//! same text. Evidence for conformance criterion A-7.6 (CC-12 round 3 F1).
mod common {
    pub mod shaping;
}
use common::shaping;
use serde_json::json;
use std::path::PathBuf;
use zygo_core::{
    Agent,
    hook::{HookInput, guarded},
    session_dir::SessionDir,
    session_view::SessionView,
};
use zygo_hooks::trim::{self, TrimOpts};

/// A session of `agent` with `results = true`, the default trim threshold and `shape`, and a ready usage db.
struct Env {
    _t: tempfile::TempDir,
    dir: SessionDir,
    db: PathBuf,
}

fn env(agent: Agent, shape: bool) -> Env {
    let t = tempfile::tempdir().unwrap();
    let dir = SessionDir::new(t.path().join("s"));
    dir.create().unwrap();
    SessionView { session: "sess-1".into(), agent, results: true, shape, ..Default::default() }.save(&dir).unwrap();
    assert_eq!(SessionView::load(&dir).unwrap().shape, shape);
    let db = t.path().join("usage.db");
    drop(zygo_usage::Store::open(&db).unwrap());
    Env { _t: t, dir, db }
}

/// The hook stdout of the Bash input of `agent` with `stdout`; the exit code is 0.
fn run(e: &Env, agent: Agent, stdout: &str) -> String {
    let client = if agent == Agent::OpenCode { "opencode" } else { "claude-code" };
    let input = shaping::hook_input(&shaping::Case {
        dir: PathBuf::new(),
        shaper: "none".into(),
        name: "shape-off".into(),
        meta: json!({"command": "make build", "exit": 0, "event": "PostToolUse", "agent": client}),
        stdout: stdout.as_bytes().to_vec(),
        stderr: vec![],
        must_keep: vec![],
    });
    let b = serde_json::to_vec(&input).unwrap();
    let (out, code) = guarded(&|b| trim::trim_in_with(&HookInput::parse(agent, b)?, &e.dir, &e.db, &TrimOpts::default()), &b);
    assert_eq!(code, 0);
    out
}

/// The number of stored results.
fn stored(e: &Env) -> usize {
    std::fs::read_dir(e.dir.results())
        .map(|d| d.filter(|f| zygo_results::is_id(&f.as_ref().unwrap().file_name().to_string_lossy())).count())
        .unwrap_or(0)
}

/// The number of `trims` rows.
fn rows(e: &Env) -> i64 {
    let c = rusqlite::Connection::open(&e.db).unwrap();
    c.query_row("SELECT count(*) FROM trims", [], |r| r.get(0)).unwrap()
}

#[test]
fn shape_off_output_below_the_threshold_passes_unchanged_on_each_client() {
    // 200 equal colored lines: clean and dedup apply, and the text is below the default threshold.
    let text = "\x1b[32mbuilding module core\x1b[0m\n".repeat(200);
    assert!((text.len() as u64) < SessionView::default().trim_threshold_bytes);
    for agent in [Agent::ClaudeCode, Agent::OpenCode] {
        let on = env(agent, true);
        let shaped = run(&on, agent, &text);
        assert!(shaped.contains("[zygo shaped this output"), "{agent:?}: shape = true does not shape: {shaped}");
        let off = env(agent, false);
        assert_eq!(run(&off, agent, &text), "", "{agent:?}: shape = false changed the output");
        assert_eq!(stored(&off), 0, "{agent:?}: shape = false stored a result");
        assert_eq!(rows(&off), 0, "{agent:?}: shape = false wrote a trims row");
    }
}
