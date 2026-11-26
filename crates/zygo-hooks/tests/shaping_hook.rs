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
    env_at(shape, SessionView::default().trim_threshold_bytes, 60, 20)
}

/// As `env`, with the trim threshold, head and tail lines given.
fn env_at(shape: bool, threshold: u64, head_lines: u32, tail_lines: u32) -> Env {
    let t = tempfile::tempdir().unwrap();
    let dir = SessionDir::new(t.path().join("s"));
    dir.create().unwrap();
    SessionView { session: "sess-1".into(), agent: Agent::ClaudeCode, results: true, shape, trim_threshold_bytes: threshold, head_lines, tail_lines, ..Default::default() }
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
    let o = TrimOpts::default();
    assert_eq!(o.stderr_cleared, trim::PO2_STDERR_CLEARED);
    assert_eq!(o.db_bound, zygo_usage::store::HOOK_DB_BOUND);
    assert!(!o.shape_panic); // ZYGO_TEST_SHAPE_PANIC unset in CI
}

/// The PO1 capture with its `error` text `Exit code 3\n` then `out-line\nerr-line\n` repeated to at least 20 KiB.
fn po1_big() -> (Value, String) {
    let mut v = fixture("claude-code/hooks/PostToolUseFailure-Bash-output-main.json");
    let mut text = String::from("Exit code 3\n");
    while text.len() < 20 * 1024 {
        text.push_str("out-line\nerr-line\n");
    }
    v["error"] = json!(text);
    (v, text)
}

#[test]
fn po1_holds_equals_the_premise_row() {
    // O§7.13, O§17 PO1: the constant equals the PO1 row; the failing branch records its limit
    let p = std::fs::read_to_string(shaping::repo().join("docs/premises.md")).unwrap();
    let row = p.lines().find(|l| l.starts_with("| PO1 |")).unwrap();
    let status = row.split(" | ").nth(5).unwrap();
    assert_eq!(status, "failed");
    assert_eq!(trim::PO1_HOLDS, status == "verified");
    assert!(row.contains("a failed Bash call on Claude Code passes unchanged"), "{row}");
}

#[test]
fn po1_failed_bash_capture_gives_no_output_and_no_row() {
    // O§7.13 "If PO1 fails", O§10 "PO1 fails", O§16 item 18 (fail)
    let (big, text) = po1_big();
    assert!(text.len() >= 20 * 1024);
    // The capture has no `tool_response`; give it the one a passing Bash call has, so only the event filter stops it
    let mut with_response = big.clone();
    with_response["tool_response"] = bash(big["tool_input"]["command"].clone(), &text, "")["tool_response"].clone();
    assert!(with_response["tool_response"].is_object());
    assert_eq!(with_response["hook_event_name"], "PostToolUseFailure");
    for shape in [true, false] {
        let e = env(shape);
        let raw = fixture("claude-code/hooks/PostToolUseFailure-Bash-output-main.json");
        assert_eq!(raw["error"], "Exit code 3\nout-line\nerr-line");
        for input in [&raw, &big, &with_response] {
            assert_eq!(run(&e, Agent::ClaudeCode, input, &TrimOpts::default()), "");
            assert_eq!(run(&e, Agent::ClaudeCode, input, &TrimOpts { stderr_cleared: false, ..TrimOpts::default() }), "");
        }
        assert!(rows(&e).is_empty() && ids(&e).is_empty(), "shape = {shape}");
    }
}

#[test]
fn po1_silent_failed_bash_gives_no_output() {
    // O§16 item 18: the silent command
    let e = env(true);
    let v = fixture("claude-code/live/p-n24-1-exit.json");
    assert_eq!(v["error"], "Exit code 3");
    assert_eq!(run(&e, Agent::ClaudeCode, &v, &TrimOpts::default()), "");
    assert!(rows(&e).is_empty() && ids(&e).is_empty());
}

#[test]
fn po1_same_text_after_a_passing_call_is_shaped() {
    // O§7.13 "A passing run is still shaped": the same text as `PostToolUse` stdout is stored and replaced
    let (big, text) = po1_big();
    let e = env(true);
    let out = run(&e, Agent::ClaudeCode, &bash(big["tool_input"]["command"].clone(), &text, ""), &TrimOpts::default());
    assert!(!out.is_empty());
    assert_eq!(ids(&e).len(), 1);
    assert_eq!(rows(&e).len(), 1);
    assert_eq!(stored(&e, &ids(&e)[0]), text.as_bytes());
}

fn fixture(rel: &str) -> Value {
    serde_json::from_slice(&std::fs::read(shaping::repo().join("fixtures").join(rel)).unwrap()).unwrap()
}

fn stored(e: &Env, id: &str) -> Vec<u8> {
    zygo_results::Store::get(&e.dir.results(), id).unwrap().data
}

#[test]
fn bash_fixtures_give_no_output_and_no_row() {
    // O§16 item 5
    let e = env(true);
    for n in ["PostToolUse-Bash-main", "PostToolUse-Bash-sub"] {
        assert_eq!(run(&e, Agent::ClaudeCode, &fixture(&format!("claude-code/hooks/{n}.json")), &TrimOpts::default()), "", "{n}");
    }
    let oc = fixture("opencode/plugin-hooks/tool.execute.after-bash.json");
    assert_eq!(run(&e, Agent::OpenCode, &oc, &TrimOpts::default()), "");
    assert!(rows(&e).is_empty() && ids(&e).is_empty());
}

#[test]
fn file_printers_get_the_size_view_of_stdout_only() {
    // O§7.1, O§16 item 7: a file printer or an unsure parse takes the shape-off path
    let out = colored(707);
    assert!(out.len() >= 20 * 1024);
    for cmd in [
        "cat f.txt",
        "cat f.txt && true",
        "true; head f.txt",
        "sh -c 'sed -n 1,9p f'",
        "bash -c \"cat f\"",
        "(cat f)",
        "echo $(cat f)",
        "ls | xargs cat",
        r"find . -name x -exec cat {} \;",
        "cargo test 2>&1 | tail -n 80",
        "echo \"unclosed",
    ] {
        let e = env(true);
        let input = bash(json!(cmd), &out, "warn\n");
        let got = run(&e, Agent::ClaudeCode, &input, &TrimOpts::default());
        let id = ids(&e).first().cloned().unwrap_or_else(|| panic!("{cmd}: nothing stored"));
        assert_eq!((masked(&e, &got), Some(stored(&e, &id))), off(&input), "{cmd}");
        assert_eq!(resp(&got)["stderr"], "warn\n", "{cmd}");
        assert!(resp(&got)["stdout"].as_str().unwrap().contains('\x1b'), "{cmd}");
        assert_eq!(rows(&e)[0].4.as_deref(), Some("size"), "{cmd}");
    }
}

#[test]
fn command_names_are_recorded() {
    // O§7.2, O§16 item 8 (the shaper is OS-30's)
    let e = env(true);
    let so = (0..1000).map(|i| format!("test tests_{i:04} ... ok\n")).collect::<String>()
        + "\ntest result: ok. 1000 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n";
    assert!(!run(&e, Agent::ClaudeCode, &bash(json!("cargo test || true"), &so, ""), &TrimOpts::default()).is_empty());
    assert_eq!(rows(&e)[0].3.as_deref(), Some("cargo test"));
    let e = env(true);
    let d = "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -0,0 +1,2000 @@\n".to_string() + &(0..2000).map(|i| format!("+line {i:05}\n")).collect::<String>();
    assert!(!run(&e, Agent::ClaudeCode, &bash(json!("cd x && git diff"), &d, ""), &TrimOpts::default()).is_empty());
    assert_eq!(rows(&e)[0].3.as_deref(), Some("git diff"));
}

#[test]
fn reuse_text_loses_only_escapes() {
    // O§7.3, O§7.4, O§16 item 9
    let e = env(true);
    let mut raw = "\x1b[1mdiff --git a/f.txt b/f.txt\x1b[m\n\x1b[1m--- a/f.txt\x1b[m\n\x1b[1m+++ b/f.txt\x1b[m\n\x1b[36m@@ -0,0 +1,104 @@\x1b[m\n".to_string();
    for i in 0..100 {
        raw += &format!("\x1b[32m+line {i:03}\x1b[m\n");
    }
    raw += "\x1b[32m+crlf line\x1b[m\r\n";
    for _ in 0..3 {
        raw += "\x1b[32m+same line\x1b[m\n";
    }
    let clean = raw.replace("\x1b[1m", "").replace("\x1b[36m", "").replace("\x1b[32m", "").replace("\x1b[m", "");
    let got = run(&e, Agent::ClaudeCode, &bash(json!("git diff --color=always"), &raw, ""), &TrimOpts::default());
    let id = ids(&e)[0].clone();
    assert_eq!(resp(&got)["stdout"], format!("{}\n{clean}", shaped_marker(&raw, "color codes removed", &id)));
    assert_eq!(stored(&e, &id), raw.as_bytes());
    // JSON is reuse text: nothing changes, so no view.
    let e = env(true);
    let line = format!("  \"{}\",\n", "x".repeat(300));
    let json = format!("[\n{line}{line}{line}  1\n]\n");
    assert_eq!(run(&e, Agent::ClaudeCode, &bash(json!("curl -s localhost/x"), &json, ""), &TrimOpts::default()), "");
    assert!(rows(&e).is_empty() && ids(&e).is_empty());
    // Control: the same lines in text are counted.
    let got = run(&e, Agent::ClaudeCode, &bash(json!("curl -s localhost/x"), &format!("note\n{json}"), ""), &TrimOpts::default());
    assert!(resp(&got)["stdout"].as_str().unwrap().contains(" [repeated 3 times]"));
}

/// 320 colored libtest lines (10240 bytes) and 192 build lines (6144 bytes).
fn streams() -> (String, String) {
    let so: String = (0..320).map(|i| format!("test tests_{i:04} ... \x1b[32mok\x1b[0m\n")).collect();
    let se: String = (0..192).map(|i| format!("   Compiling crate_{i:04} v0.10.0\n")).collect();
    assert_eq!((so.len(), se.len()), (10240, 6144));
    (so, se)
}

#[test]
fn stderr_cleared_branch_counts_both_streams() {
    // O§7.2, O§7.8, O§16 item 10
    let e = env(true);
    let (so, se) = streams();
    let raw = format!("{so}[stderr]\n{se}");
    let got = run(&e, Agent::ClaudeCode, &bash(json!("cargo test"), &so, &se), &TrimOpts::default());
    let id = ids(&e)[0].clone();
    let r = resp(&got);
    assert_eq!(r["stdout"], format!("{}\n{}", shaped_marker(&raw, "color codes removed", &id), raw.replace("\x1b[32m", "").replace("\x1b[0m", "")));
    assert_eq!(r["stderr"], "");
    assert_eq!((raw.len(), raw.lines().count()), (16393, 513));
    assert_eq!(stored(&e, &id), raw.as_bytes());
    assert_eq!(rows(&e), [(16393, 13705, Some(id), Some("cargo test".to_string()), Some("clean".to_string()))]);
}

#[test]
fn stderr_kept_branch_counts_stdout_only() {
    // O§7.8 "When PO2 fails", O§16 item 10
    let e = env(true);
    let (so, se) = streams();
    let raw = format!("{so}[stderr]\n{se}");
    let opts = TrimOpts { stderr_cleared: false, ..Default::default() };
    let got = run(&e, Agent::ClaudeCode, &bash(json!("cargo test"), &so, &se), &opts);
    let id = ids(&e)[0].clone();
    let r = resp(&got);
    assert_eq!(r["stdout"], format!("{}\n{}", shaped_marker(&raw, "color codes removed", &id), so.replace("\x1b[32m", "").replace("\x1b[0m", "")));
    assert_eq!(r["stderr"], se.as_str());
    assert_eq!(stored(&e, &id), raw.as_bytes());
    assert_eq!(rows(&e), [(10240, 7552, Some(id), Some("cargo test".to_string()), Some("clean".to_string()))]);
}

#[test]
fn po2_stderr_cleared_equals_the_premise_row() {
    // O§7.8 "Output, `PostToolUse`" and "When PO2 fails", O§10 "PO2 fails", O§16 item 10, O§17 PO2
    let p = std::fs::read_to_string(shaping::repo().join("docs/premises.md")).unwrap();
    let row = p.lines().find(|l| l.starts_with("| PO2 |")).unwrap();
    let status = row.split(" | ").nth(5).unwrap();
    assert_eq!(status, "verified");
    assert_eq!(trim::PO2_STDERR_CLEARED, status == "verified");
    assert!(row.contains("O§7.8 'Output, `PostToolUse`' holds."), "{row}");
    // the default TrimOpts takes the verified branch of item 10
    let o = TrimOpts::default();
    assert_eq!(o.stderr_cleared, trim::PO2_STDERR_CLEARED);
    let e = env(true);
    let (so, se) = streams();
    let raw = format!("{so}[stderr]\n{se}");
    let r = resp(&run(&e, Agent::ClaudeCode, &bash(json!("cargo test"), &so, &se), &o));
    let id = ids(&e)[0].clone();
    assert!(r["stdout"].as_str().unwrap().starts_with(&shaped_marker(&raw, "color codes removed", &id)));
    assert_eq!(r["stderr"], "");
    assert_eq!(stored(&e, &id), raw.as_bytes());
    assert_eq!(rows(&e), [(16393, 13705, Some(id), Some("cargo test".to_string()), Some("clean".to_string()))]);
}

#[test]
fn stderr_after_stdout_without_newline() {
    // O§7.2: a `\n` before `[stderr]` when stdout does not end in one
    let e = env(true);
    let mut so = colored(70);
    so.pop();
    run(&e, Agent::ClaudeCode, &bash(json!("echo x"), &so, "e\n"), &TrimOpts::default());
    assert_eq!(stored(&e, &ids(&e)[0]), format!("{so}\n[stderr]\ne\n").as_bytes());
}

#[test]
fn command_missing_shapes_and_records_other() {
    // O§10 "Command missing or not a string"
    let e = env(true);
    let got = run(&e, Agent::ClaudeCode, &bash(json!(12345), &colored(70), ""), &TrimOpts::default());
    assert!(resp(&got)["stdout"].as_str().unwrap().starts_with("[zygo shaped this output: color codes removed."));
    assert_eq!(rows(&e)[0].3.as_deref(), Some("other"));
}

/// A 2000-byte, 1-line raw text whose shaped view saves `saving` bytes: one CSI that clean removes.
fn saving_input(saving: usize) -> String {
    let p = zygo_results::Stored { id: ZERO_ID.into(), tool: "Bash".into(), bytes: 2000, lines: 1, kind: zygo_results::Kind::Text, created_at: 0, session: None, data: vec![] };
    let e = saving + zygo_results::view::shaped_view(&[zygo_results::shape::Step::ColorCodesRemoved], "", &p).len();
    format!("\x1b[{}m{}\n", "0".repeat(e - 3), "a".repeat(2000 - e - 1))
}

#[test]
fn saving_of_255_gives_nothing_and_256_is_used() {
    // O§7.8 use rule, O§10 "View saves < 256 bytes"
    let e = env(true);
    let raw = saving_input(255);
    assert_eq!(raw.len(), 2000);
    assert_eq!(run(&e, Agent::ClaudeCode, &bash(json!("echo x"), &raw, ""), &TrimOpts::default()), "");
    assert!(ids(&e).is_empty() && rows(&e).is_empty());
    assert!(std::fs::read_dir(e.dir.results()).map(|d| d.count() == 0).unwrap_or(true));
    let raw = saving_input(256);
    assert!(!run(&e, Agent::ClaudeCode, &bash(json!("echo x"), &raw, ""), &TrimOpts::default()).is_empty());
    let id = ids(&e)[0].clone();
    assert_eq!(rows(&e), [(2000, 1744, Some(id), Some("other".to_string()), Some("clean".to_string()))]);
}

#[test]
fn over_4_mib_gets_only_the_size_view() {
    // O§7.3, O§10 "Raw text over 4 MiB"
    let e = env(true);
    let input = bash(json!("echo x"), &colored(5 * 1024 * 1024 / 29 + 1), "");
    let got = run(&e, Agent::ClaudeCode, &input, &TrimOpts::default());
    assert_eq!(masked(&e, &got), off(&input).0);
    assert!(resp(&got)["stdout"].as_str().unwrap().contains('\x1b'));
    assert_eq!(rows(&e)[0].4.as_deref(), Some("size"));
}

#[test]
fn store_failure_gives_fixed_error_and_no_row() {
    // O§10 "`Store::put` fails"
    let e = env(true);
    std::fs::write(e.dir.results(), b"in the way").unwrap();
    let input = bash(json!("echo x"), &colored(70), "");
    let parsed = HookInput::parse(Agent::ClaudeCode, &serde_json::to_vec(&input).unwrap()).unwrap();
    let err = trim::trim_in_with(&parsed, &e.dir, &e.db, &TrimOpts::default()).err().expect("an error");
    assert_eq!(format!("{err:#}"), "cannot store the tool result");
    assert_eq!(run(&e, Agent::ClaudeCode, &input, &TrimOpts::default()), "");
    assert!(rows(&e).is_empty());
}

#[test]
fn read_keeps_the_size_view() {
    // O§7.1: only Bash is shaped
    let e = env(true);
    let mut v = fixture("claude-code/hooks/PostToolUse-Read-main.json");
    v["tool_response"] = json!({"type": "text", "file": {"filePath": "/repo/f", "content": colored(707), "numLines": 707, "startLine": 1, "totalLines": 707}});
    let c = resp(&run(&e, Agent::ClaudeCode, &v, &TrimOpts::default()))["file"]["content"].as_str().unwrap().to_string();
    assert!(c.contains('\x1b') && c.contains("[zygo trimmed this output: 707 lines,"));
    assert_eq!((rows(&e)[0].3.as_deref(), rows(&e)[0].4.as_deref()), (None, Some("size")));
}

#[test]
fn shaper_panic_gives_size_view_of_raw_text() {
    // O§7.10, O§10 "Shaper panics", O§16 item 12
    let panic = TrimOpts { shape_panic: true, ..Default::default() };
    let e = env(true);
    let big = colored(3532);
    assert!(big.len() >= 100 * 1024);
    let input = bash(json!("echo x"), &big, "");
    let got = run(&e, Agent::ClaudeCode, &input, &panic);
    assert_eq!(masked(&e, &got), off(&input).0);
    assert!(resp(&got)["stdout"].as_str().unwrap().contains('\x1b'));
    assert_eq!(rows(&e)[0].4.as_deref(), Some("size"));
    let e = env(true);
    let small = bash(json!("echo x"), &colored(70), "");
    assert_eq!(run(&e, Agent::ClaudeCode, &small, &panic), "");
    assert!(ids(&e).is_empty() && rows(&e).is_empty());
    let got = run(&e, Agent::ClaudeCode, &small, &TrimOpts::default());
    assert!(resp(&got)["stdout"].as_str().unwrap().starts_with("[zygo shaped this output: color codes removed."));
}

// ---- Round 2 ----

/// The `kind` of each `trims` row, by rowid.
fn kinds(e: &Env) -> Vec<String> {
    let c = rusqlite::Connection::open(&e.db).unwrap();
    let mut q = c.prepare("SELECT kind FROM trims ORDER BY rowid").unwrap();
    q.query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect()
}

/// The §10.1 size view of `text` with the marker of the stored raw text `raw` (id masked), default head and tail.
fn size_view_of(text: &str, raw: &str) -> String {
    size_view_with(text, raw, 60, 20)
}

/// As `size_view_of`, with `head` and `tail` lines.
fn size_view_with(text: &str, raw: &str, head_lines: u32, tail_lines: u32) -> String {
    let s = zygo_results::Stored { id: ZERO_ID.into(), tool: "Bash".into(), bytes: raw.len() as u64, lines: raw.lines().count() as u64, kind: zygo_results::Kind::Text, created_at: 0, session: None, data: vec![] };
    let cfg = zygo_config::ResultsConfig { trim_threshold_bytes: 0, head_lines, tail_lines, shape: true };
    zygo_results::view::size_view(text, &s, &cfg)
}

#[test]
fn json_stdout_with_stderr_keeps_reuse_protection() {
    // R2, O§7.3: the kind is sniffed on stdout, so a JSON stdout is reuse text in both branches.
    // The JSON is under SNIFF_BYTES (4 KiB), so the stderr part is inside the sniffed head; a low threshold and
    // 3 head lines make the size view apply with no step 1-4 change.
    let line = format!("  \"{}\",\n", "x".repeat(1000));
    let json = format!("[\n{}  1\n]\n", line.repeat(3));
    assert!(json.len() < 4 * 1024);
    for cleared in [true, false] {
        let e = env_at(true, 100, 3, 1);
        let opts = TrimOpts { stderr_cleared: cleared, ..Default::default() };
        let got = run(&e, Agent::ClaudeCode, &bash(json!("curl localhost/x"), &json, "warn\n"), &opts);
        let raw = format!("{json}[stderr]\nwarn\n");
        let src = if cleared { raw.as_str() } else { json.as_str() };
        assert_eq!(masked(&e, &resp(&got)["stdout"].as_str().unwrap().to_string()), size_view_with(src, &raw, 3, 1), "cleared {cleared}");
        assert_eq!(kinds(&e), ["json"], "cleared {cleared}");
        assert_eq!(rows(&e)[0].4.as_deref(), Some("size"), "cleared {cleared}");
    }
}

#[test]
fn non_string_stderr_is_left_in_the_output() {
    // R3: only a string stderr is cleared
    for v in [json!(12345), json!(["warn"])] {
        let e = env(true);
        let mut input = bash(json!("echo x"), &colored(70), "");
        input["tool_response"]["stderr"] = v.clone();
        let r = resp(&run(&e, Agent::ClaudeCode, &input, &TrimOpts::default()));
        assert!(r["stdout"].as_str().unwrap().starts_with("[zygo shaped this output: color codes removed."), "{v}");
        assert_eq!(r["stderr"], v);
    }
}

#[test]
fn po2_fail_bound_is_on_the_raw_text() {
    // R4, O§7.3 line 221 and O§7.8 "When PO2 fails": raw over 4 MiB gets step 5 only, on stdout
    let so = colored(3 * 1024 * 1024 / 29);
    let se: String = (0..2 * 1024 * 1024 / 32).map(|i| format!("   Compiling crate_{i:06} v0.1.0\n")).collect();
    let raw = format!("{so}[stderr]\n{se}");
    assert!(so.len() <= 4 * 1024 * 1024 && raw.len() > 4 * 1024 * 1024);
    let e = env(true);
    let opts = TrimOpts { stderr_cleared: false, ..Default::default() };
    let r = resp(&run(&e, Agent::ClaudeCode, &bash(json!("echo x"), &so, &se), &opts));
    let view = r["stdout"].as_str().unwrap().to_string();
    assert_eq!(masked(&e, &view), size_view_of(&so, &raw));
    assert_eq!(r["stderr"], se.as_str());
    let id = ids(&e)[0].clone();
    assert_eq!(rows(&e), [(so.len() as i64, view.len() as i64, Some(id), Some("other".to_string()), Some("size".to_string()))]);
}

#[test]
fn opencode_shaped_output_gains_no_stderr_key() {
    // G1, O§7.2 OpenCode, O§7.8 output
    let e = env(true);
    let mut v = fixture("opencode/plugin-hooks/tool.execute.after-bash.json");
    v["output"]["output"] = json!(colored(707));
    v["output"]["metadata"]["output"] = json!(colored(707));
    let r = resp(&run(&e, Agent::OpenCode, &v, &TrimOpts::default()));
    assert!(r["stdout"].as_str().unwrap().starts_with("[zygo shaped this output: color codes removed."));
    assert!(r.get("stderr").is_none(), "{r}");
}

#[test]
fn shaper_panic_with_stderr_gives_size_view_of_raw_text() {
    // G2, O§7.10
    let e = env(true);
    let panic = TrimOpts { shape_panic: true, ..Default::default() };
    let so = colored(3532);
    let raw = format!("{so}[stderr]\nwarn\n");
    let r = resp(&run(&e, Agent::ClaudeCode, &bash(json!("echo x"), &so, "warn\n"), &panic));
    assert_eq!(masked(&e, &r["stdout"].as_str().unwrap().to_string()), size_view_of(&raw, &raw));
    assert_eq!(r["stderr"], "");
    assert_eq!(stored(&e, &ids(&e)[0]), raw.as_bytes());
}

#[test]
fn empty_stdout_with_stderr_has_no_leading_newline() {
    // G3, O§7.2: no newline before `[stderr]` when stdout is empty
    let e = env(true);
    let se: String = (0..1000).map(|i| format!("warning {i:04}: unused\n")).collect();
    assert!(se.len() > 16384);
    let r = resp(&run(&e, Agent::ClaudeCode, &bash(json!("echo x"), "", &se), &TrimOpts::default()));
    let raw = format!("[stderr]\n{se}");
    assert_eq!(stored(&e, &ids(&e)[0]), raw.as_bytes());
    assert_eq!(masked(&e, &r["stdout"].as_str().unwrap().to_string()), size_view_of(&raw, &raw));
    assert_eq!(r["stderr"], "");
}

/// 81 distinct plain lines whose §10.1 size view saves exactly `saving` bytes (one middle line is dropped).
fn size_saving_input(saving: usize) -> String {
    let head: String = (0..60).map(|i| format!("head {i:02}\n")).collect();
    let tail: String = (0..20).map(|i| format!("tail {i:02}\n")).collect();
    (1..4000)
        .map(|m| format!("{head}{}\n{tail}", "m".repeat(m)))
        .find(|t| t.len() as i64 - size_view_of(t, t).len() as i64 == saving as i64)
        .unwrap()
}

#[test]
fn size_view_use_rule_boundary() {
    // G4, O§7.8 use rule on the size-view path (no step 1-4 applies)
    let raw = size_saving_input(255);
    let e = env_at(true, 100, 60, 20);
    assert_eq!(run(&e, Agent::ClaudeCode, &bash(json!("echo x"), &raw, ""), &TrimOpts::default()), "");
    assert!(ids(&e).is_empty() && rows(&e).is_empty());
    let raw = size_saving_input(256);
    let got = run(&e, Agent::ClaudeCode, &bash(json!("echo x"), &raw, ""), &TrimOpts::default());
    assert!(resp(&got)["stdout"].as_str().unwrap().contains("[zygo trimmed this output: 81 lines,"));
    let id = ids(&e)[0].clone();
    assert_eq!(rows(&e), [(raw.len() as i64, raw.len() as i64 - 256, Some(id), Some("other".to_string()), Some("size".to_string()))]);
}

#[test]
fn log_view_with_its_marker_fits_8_kib() {
    // O§7.7; O§8.2 row "diff, json, log": view ≤ 8 KiB, marker included (O§8.1)
    use zygo_results::shape::kind_view::kind_view;
    use zygo_results::view::{SHAPED_MARKER_ROOM, VIEW_CAP_BYTES};
    use zygo_results::Kind;
    let raw: String = (1..=1000).map(|i| format!("2026-11-25 10:00:00 ERROR line {i:05}\n")).collect();
    let marker = shaped_marker(&raw, "log errors kept", ZERO_ID);
    let full = kind_view(&raw, Kind::Log, VIEW_CAP_BYTES).unwrap().0;
    assert!(marker.len() + 1 + full.len() > VIEW_CAP_BYTES); // the old cap's kind view is within one marker length of 8 KiB
    let e = env(true);
    let out = run(&e, Agent::ClaudeCode, &bash(json!("echo x"), &raw, ""), &TrimOpts::default());
    let view = masked(&e, resp(&out)["stdout"].as_str().unwrap());
    let kv = kind_view(&raw, Kind::Log, VIEW_CAP_BYTES - SHAPED_MARKER_ROOM).unwrap().0;
    assert_eq!(view, format!("{marker}\n{kv}"));
    assert!(view.len() <= VIEW_CAP_BYTES, "{}", view.len());
    let id = ids(&e)[0].clone();
    assert_eq!(rows(&e), [(raw.len() as i64, view.len() as i64, Some(id), Some("other".to_string()), Some("log".to_string()))]);
}

#[test]
fn diff_view_with_its_marker_fits_8_kib() {
    // O§7.7; O§8.2 row "diff, json, log": view ≤ 8 KiB, marker included (O§8.1)
    use zygo_results::shape::kind_view::kind_view;
    use zygo_results::view::{SHAPED_MARKER_ROOM, VIEW_CAP_BYTES};
    use zygo_results::Kind;
    // 300 git file sections of 66 bytes each: the view lists all 300 files, then shows whole sections up to the cap.
    let raw: String = (1..=300).map(|i| format!("diff --git a/f{i:03} b/f{i:03}\n--- a/f{i:03}\n+++ b/f{i:03}\n@@ -1 +1 @@\n-a\n+b\n")).collect();
    let marker = shaped_marker(&raw, "diff files listed", ZERO_ID);
    let full = kind_view(&raw, Kind::Diff, VIEW_CAP_BYTES).unwrap().0;
    assert!(marker.len() + 1 + full.len() > VIEW_CAP_BYTES); // the old cap's kind view is within one marker length of 8 KiB
    let e = env(true);
    let out = run(&e, Agent::ClaudeCode, &bash(json!("echo x"), &raw, ""), &TrimOpts::default());
    let view = masked(&e, resp(&out)["stdout"].as_str().unwrap());
    let kv = kind_view(&raw, Kind::Diff, VIEW_CAP_BYTES - SHAPED_MARKER_ROOM).unwrap().0;
    assert_eq!(view, format!("{marker}\n{kv}"));
    assert!(view.len() <= VIEW_CAP_BYTES, "{}", view.len());
    let id = ids(&e)[0].clone();
    assert_eq!(rows(&e), [(raw.len() as i64, view.len() as i64, Some(id), Some("other".to_string()), Some("diff".to_string()))]);
}

#[test]
fn json_view_with_its_marker_fits_8_kib() {
    // O§7.7; O§8.2 row "diff, json, log": view ≤ 8 KiB, marker included (O§8.1)
    use zygo_results::shape::kind_view::kind_view;
    use zygo_results::view::{SHAPED_MARKER_ROOM, VIEW_CAP_BYTES};
    use zygo_results::Kind;
    // A pretty-printed array of 600 short lines: the view shows the outline, then raw lines up to the cap.
    let items: Vec<String> = (1..=600).map(|i| format!("  {{\"id\": {i}, \"name\": \"item {i:05}\"}}")).collect();
    let raw = format!("[\n{}\n]\n", items.join(",\n"));
    let marker = shaped_marker(&raw, "json outlined", ZERO_ID);
    let full = kind_view(&raw, Kind::Json, VIEW_CAP_BYTES).unwrap().0;
    assert!(marker.len() + 1 + full.len() > VIEW_CAP_BYTES); // the old cap's kind view is within one marker length of 8 KiB
    let e = env(true);
    let out = run(&e, Agent::ClaudeCode, &bash(json!("echo x"), &raw, ""), &TrimOpts::default());
    let view = masked(&e, resp(&out)["stdout"].as_str().unwrap());
    let kv = kind_view(&raw, Kind::Json, VIEW_CAP_BYTES - SHAPED_MARKER_ROOM).unwrap().0;
    assert_eq!(view, format!("{marker}\n{kv}"));
    assert!(view.len() <= VIEW_CAP_BYTES, "{}", view.len());
    let id = ids(&e)[0].clone();
    assert_eq!(rows(&e), [(raw.len() as i64, view.len() as i64, Some(id), Some("other".to_string()), Some("json".to_string()))]);
}
