//! SX-3: `zygo hook pre-compact` sets the per-subagent re-inject flag (session text design T§9.2 item 1, T§14
//! item 2, T§17 check 7). No test spawns `claude` or `opencode` or reads a clock; the only child process is this
//! test binary.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{json, Value};
use zygo_core::{
    hook::{guarded, HookInput},
    overrides::Overrides,
    session_dir::SessionDir,
    session_view::SessionView,
    Agent,
};
use zygo_hooks::{
    pre_compact,
    session_text::mechanics,
    trim::{self, TrimOpts},
};

/// The §10.6 text with no open unit (`docs/specs/2026-11-18-zygo-design.md` §10.6), byte for byte as
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

/// Runs `case` of `child_case` in a child process with `CHILD` = `root`; returns its stderr.
fn child(root: &Path, case: &str) -> String {
    let o = Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "child_case", "--nocapture", "--test-threads=1"])
        .env(CHILD, root)
        .env(CASE, case)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(o.status.success(), "{case}: {o:?}");
    assert!(String::from_utf8(o.stdout).unwrap().contains("child_case ... ok"), "{case}");
    String::from_utf8(o.stderr).unwrap()
}

const CASE: &str = "ZYGO_SX3_CASE";

/// The inputs of `child_case`, by case name.
fn case_input(case: &str) -> Value {
    match case {
        "main" => fixture("PreCompact-main"),
        "zygo" => with(fixture("PreCompact-sub"), "agent_type", json!("zygo-read")),
        "bad_id" => with(fixture("PreCompact-sub"), "agent_id", json!("../x")),
        "sub" => fixture("PreCompact-sub"),
        other => panic!("unknown case {other}"),
    }
}

/// Child half of the stderr tests: runs `case_input($CASE)` against `$CHILD/s`, or `$CHILD/gone` for the case
/// `gone`; returns at once without `CHILD`.
#[test]
#[ignore]
fn child_case() {
    let (Some(root), Some(case)) = (std::env::var_os(CHILD), std::env::var(CASE).ok()) else { return };
    let (sub, input) = if case == "gone" { ("gone", "sub") } else { ("s", case.as_str()) };
    let d = SessionDir::new(Path::new(&root).join(sub));
    let stdin = serde_json::to_vec(&case_input(input)).unwrap();
    let got = guarded(&|b| pre_compact::run_with(&HookInput::parse(Agent::ClaudeCode, b)?, Some(&d)), &stdin);
    assert_eq!(got, (KEEP_NO_UNIT.to_string(), 0));
}

#[cfg(unix)]
#[test]
fn pre_compact_existing_flag_gets_mode_0600() {
    // Review F2: T§9.2 item 1 sets the modes of a flag and directory that already exist.
    use std::os::unix::fs::PermissionsExt;
    let (_t, d) = sd();
    let f = d.reinject().join(ID);
    std::fs::create_dir(d.reinject()).unwrap();
    std::fs::set_permissions(d.reinject(), std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(&f, "").unwrap();
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(run(&d, &fixture("PreCompact-sub")), KEEP_NO_UNIT);
    assert_eq!(std::fs::metadata(&f).unwrap().permissions().mode() & 0o777, 0o600);
    assert_eq!(std::fs::metadata(d.reinject()).unwrap().permissions().mode() & 0o777, 0o700);
}

#[test]
fn pre_compact_missing_root_is_not_created() {
    // Review F3: a stale ZYGO_SESSION_DIR. The root is never created; the text is still printed with one stderr line.
    let t = tempfile::tempdir().unwrap();
    let d = SessionDir::new(t.path().join("gone"));
    assert_eq!(run(&d, &fixture("PreCompact-sub")), KEEP_NO_UNIT);
    assert!(!d.root.exists());
    let err = child(t.path(), "gone");
    assert!(!d.root.exists());
    let flag = d.reinject().join(ID);
    assert!(err.starts_with(&format!("zygo hook: pre-compact: cannot create {}: ", flag.display())), "{err:?}");
    assert!(err.ends_with('\n') && err.lines().count() == 1, "{err:?}");
}

#[test]
fn pre_compact_no_session_dir_creates_nothing() {
    // Review gap 1: with no session directory there is no flag path; the text is unchanged.
    let cwd = std::env::current_dir().unwrap();
    let before: Vec<_> = std::fs::read_dir(&cwd).unwrap().map(|e| e.unwrap().file_name()).collect();
    let out = pre_compact::run_with(&input(&fixture("PreCompact-sub")), None).unwrap().to_stdout();
    assert_eq!(out, KEEP_NO_UNIT);
    assert!(!cwd.join("reinject").exists());
    let after: Vec<_> = std::fs::read_dir(&cwd).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(before, after);
}

#[test]
fn pre_compact_no_flag_cases_write_no_stderr() {
    // Review gap 2: the main input, a `zygo-` type and a bad id write nothing to stderr and create nothing.
    for case in ["main", "zygo", "bad_id"] {
        let (t, d) = sd();
        assert_eq!(child(t.path(), case), "", "{case}");
        assert_eq!(std::fs::read_dir(&d.root).unwrap().count(), 0, "{case}");
        assert_eq!(std::fs::read_dir(t.path()).unwrap().count(), 1, "{case}");
    }
}

// SX-4: `zygo hook trim` gives the mechanics text once after a subagent's compaction (T§9.2 items 2-4, T§14 item 2,
// T§17 check 8).

/// Results, navigation and units on; shape off; threshold 16384, head 60, tail 20.
fn view() -> SessionView {
    SessionView {
        session: "S1".into(),
        agent: Agent::ClaudeCode,
        results: true,
        navigation: true,
        work_units: true,
        shape: false,
        trim_threshold_bytes: 16384,
        head_lines: 60,
        tail_lines: 20,
        ..Default::default()
    }
}

/// `sd()` with `view()` saved and the usage db at `<t>/usage.db` opened once.
fn tenv() -> (tempfile::TempDir, SessionDir, PathBuf) {
    let (t, d) = sd();
    view().save(&d).unwrap();
    let db = t.path().join("usage.db");
    drop(zygo_usage::Store::open(&db).unwrap());
    (t, d, db)
}

/// stdout of `trim::trim_in_with(input(v), d, db, &TrimOpts::default())` through `guarded`; asserts exit 0.
fn trim_run(d: &SessionDir, db: &Path, v: &Value) -> String {
    let stdin = serde_json::to_vec(v).unwrap();
    let (out, code) =
        guarded(&|b| trim::trim_in_with(&HookInput::parse(Agent::ClaudeCode, b)?, d, db, &TrimOpts::default()), &stdin);
    assert_eq!(code, 0);
    out
}

fn m_full() -> String {
    let m = mechanics(&view());
    assert_eq!(m.len(), 721);
    m
}

fn envelope(m: &str) -> String {
    format!(r#"{{"hookSpecificOutput":{{"hookEventName":"PostToolUse","additionalContext":{}}}}}"#, json!(m))
}

/// The flag of `ID`, set by `pre_compact::run_with` with `PreCompact-sub` (T§17 check 7); returns its path.
fn set_flag(d: &SessionDir) -> PathBuf {
    run(d, &fixture("PreCompact-sub"));
    let f = d.reinject().join(ID);
    assert!(f.is_file());
    f
}

/// `PostToolUse-Bash-sub` with `stdout` = "line 0\n" .. "line 2999\n" (over 16384 bytes, asserted).
fn big_bash() -> Value {
    let stdout: String = (0..3000).map(|i| format!("line {i}\n")).collect();
    assert!(stdout.len() > 16384);
    let mut v = fixture("PostToolUse-Bash-sub");
    v["tool_response"]["stdout"] = json!(stdout);
    v
}

/// The keys of `hookSpecificOutput` in the one-line output `out`, in order.
fn keys(out: &str) -> Vec<String> {
    assert_eq!(out.lines().count(), 1, "{out}");
    let v: Value = serde_json::from_str(out).unwrap();
    v["hookSpecificOutput"].as_object().unwrap().keys().cloned().collect()
}

/// set_flag, then `name` gives `envelope(m)` and the flag is gone, then a second run gives "".
fn once(d: &SessionDir, db: &Path, name: &str, m: &str) {
    let f = set_flag(d);
    assert_eq!(fixture(name)["agent_id"], json!(ID), "{name}");
    assert_eq!(trim_run(d, db, &fixture(name)), envelope(m), "{name}");
    assert!(!f.exists(), "{name}");
    assert_eq!(trim_run(d, db, &fixture(name)), "", "{name}");
}

#[test]
fn trim_bash_sub_after_pre_compact_gives_mechanics_once() {
    // T§17 check 8
    let (_t, d, db) = tenv();
    assert_eq!(envelope("M"), r#"{"hookSpecificOutput":{"hookEventName":"PostToolUse","additionalContext":"M"}}"#);
    once(&d, &db, "PostToolUse-Bash-sub", &m_full());
}

#[test]
fn trim_read_sub_gives_mechanics_once() {
    let (_t, d, db) = tenv();
    once(&d, &db, "PostToolUse-Read-sub", &m_full());
}

#[test]
fn trim_grep_sub_gives_mechanics_once() {
    let (_t, d, db) = tenv();
    once(&d, &db, "PostToolUse-Grep-sub", &m_full());
}

#[test]
fn trim_glob_sub_gives_mechanics_once() {
    let (_t, d, db) = tenv();
    once(&d, &db, "PostToolUse-Glob-sub", &m_full());
}

#[test]
fn trim_webfetch_sub_gives_mechanics_once() {
    let (_t, d, db) = tenv();
    once(&d, &db, "PostToolUse-WebFetch-sub", &m_full());
}

#[test]
fn trim_results_off_bash_sub_gives_mechanics_once() {
    // Review focus 115: results off is an early return of the trim; the flag is taken before it.
    let (_t, d, db) = tenv();
    Overrides { results: Some(false), ..Default::default() }.save(&d).unwrap();
    let m = mechanics(&SessionView { results: false, ..view() });
    assert_eq!(m.len(), 556);
    assert!(!m.contains("result_view"));
    once(&d, &db, "PostToolUse-Bash-sub", &m);
}

#[test]
fn trim_non_hooked_shape_gives_mechanics_once() {
    // Review focus 115: an image Read has no trim target; the flag is taken before that return.
    let (_t, d, db) = tenv();
    once(&d, &db, "PostToolUse-Read-image-sub", &m_full());
}

#[test]
fn trim_over_threshold_merges_view_and_mechanics() {
    let (_t, d, db) = tenv();
    let f = set_flag(&d);
    let out = trim_run(&d, &db, &big_bash());
    assert_eq!(keys(&out), ["hookEventName", "updatedToolOutput", "additionalContext"]);
    let v: Value = serde_json::from_str(&out).unwrap();
    let h = &v["hookSpecificOutput"];
    assert_eq!(h["additionalContext"], json!(m_full()));
    assert_ne!(h["updatedToolOutput"]["stdout"], big_bash()["tool_response"]["stdout"]);
    assert!(!f.exists());
    assert_eq!(keys(&trim_run(&d, &db, &big_bash())), ["hookEventName", "updatedToolOutput"]);
}

#[test]
fn trim_main_input_reads_no_flag() {
    let (_t, d, db) = tenv();
    let f = set_flag(&d);
    std::fs::write(d.reinject().join("main"), "").unwrap();
    assert!(fixture("PostToolUse-Bash-main").get("agent_id").is_none());
    assert_eq!(trim_run(&d, &db, &fixture("PostToolUse-Bash-main")), "");
    assert!(f.exists());
    assert!(d.reinject().join("main").exists());
}

#[test]
fn trim_bad_agent_id_removes_no_file() {
    // Review focus 114: an id that fails the name rule names no path.
    let (_t, d, db) = tenv();
    set_flag(&d);
    std::fs::write(d.root.join("x"), "keep").unwrap();
    for id in [json!("../x"), json!("../view.json"), json!("a/b"), json!(7)] {
        assert_eq!(trim_run(&d, &db, &with(fixture("PostToolUse-Bash-sub"), "agent_id", id.clone())), "", "{id}");
        assert!(d.root.join("x").exists(), "{id}");
        assert!(d.view_json().exists(), "{id}");
        assert!(d.reinject().join(ID).exists(), "{id}");
    }
}

#[test]
fn trim_flag_remove_failure_gives_no_text() {
    // T§14 item 2: a non-empty directory where the flag is makes `remove_file` fail on every OS.
    let (t, d, db) = tenv();
    let flag = d.reinject().join(ID);
    std::fs::create_dir_all(flag.join("x")).unwrap();
    let err = trim_child(t.path(), "under", &[]);
    assert!(err.starts_with(&format!("zygo hook: trim: cannot remove {}: ", flag.display())), "{err:?}");
    assert!(err.ends_with('\n') && err.lines().count() == 1, "{err:?}");
    assert_eq!(keys(&trim_run(&d, &db, &big_bash())), ["hookEventName", "updatedToolOutput"]);
    assert!(flag.is_dir());
}

/// Runs `case` of `trim_child_case` in a child process with `CHILD` = `root`; returns its stderr. `extra` sets more
/// variables of the child only.
fn trim_child(root: &Path, case: &str, extra: &[(&str, &Path)]) -> String {
    let mut c = Command::new(std::env::current_exe().unwrap());
    c.args(["--ignored", "--exact", "trim_child_case", "--nocapture", "--test-threads=1"])
        .env(CHILD, root)
        .env(CASE, case)
        .stdin(Stdio::null());
    for (k, v) in extra {
        c.env(k, v);
    }
    let o = c.output().unwrap();
    assert!(o.status.success(), "{case}: {o:?}");
    assert!(String::from_utf8(o.stdout).unwrap().contains("trim_child_case ... ok"), "{case}");
    String::from_utf8(o.stderr).unwrap()
}

/// Child half of the trim stderr and `run` tests; returns at once without `CHILD`. Case `under`: `trim_in_with` on
/// `PostToolUse-Bash-sub` prints nothing. Case `run_image`: `trim::run` (session folder from `ZYGO_SESSION_DIR`) on
/// `PostToolUse-Read-image-sub` prints `envelope(M)`.
#[test]
#[ignore]
fn trim_child_case() {
    let (Some(root), Some(case)) = (std::env::var_os(CHILD), std::env::var(CASE).ok()) else { return };
    let d = SessionDir::new(Path::new(&root).join("s"));
    let db = Path::new(&root).join("usage.db");
    match case.as_str() {
        "under" => {
            let stdin = serde_json::to_vec(&fixture("PostToolUse-Bash-sub")).unwrap();
            let got = guarded(
                &|b| trim::trim_in_with(&HookInput::parse(Agent::ClaudeCode, b)?, &d, &db, &TrimOpts::default()),
                &stdin,
            );
            assert_eq!(got, (String::new(), 0));
        }
        "run_image" => {
            let stdin = serde_json::to_vec(&fixture("PostToolUse-Read-image-sub")).unwrap();
            let got = guarded(&|b| trim::run(&HookInput::parse(Agent::ClaudeCode, b)?), &stdin);
            assert_eq!(got, (envelope(&m_full()), 0));
        }
        other => panic!("unknown case {other}"),
    }
}

#[test]
fn trim_run_entry_gives_mechanics_for_non_hooked_shape() {
    // Review F1: `trim::run` takes the flag for an input with no trim target, as `trim_in_with` does.
    let (t, d, _db) = tenv();
    let f = set_flag(&d);
    let err = trim_child(t.path(), "run_image", &[("ZYGO_SESSION_DIR", &d.root), ("HOME", t.path())]);
    assert_eq!(err, "");
    assert!(!f.exists());
}

#[cfg(unix)]
#[test]
fn trim_flag_stat_failure_gives_no_text() {
    // Review F3, T§14 item 2: a stat error other than NotFound is a flag that cannot be removed.
    use std::os::unix::fs::PermissionsExt;
    let (t, d, db) = tenv();
    let flag = set_flag(&d);
    std::fs::set_permissions(d.reinject(), std::fs::Permissions::from_mode(0o000)).unwrap();
    let err = trim_child(t.path(), "under", &[]);
    let big = trim_run(&d, &db, &big_bash());
    std::fs::set_permissions(d.reinject(), std::fs::Permissions::from_mode(0o700)).unwrap();
    assert!(err.starts_with(&format!("zygo hook: trim: cannot remove {}: ", flag.display())), "{err:?}");
    assert!(err.ends_with('\n') && err.lines().count() == 1, "{err:?}");
    assert_eq!(keys(&big), ["hookEventName", "updatedToolOutput"]);
    assert!(flag.exists());
}

#[test]
fn trim_without_flag_output_unchanged() {
    let (_t, d, db) = tenv();
    assert_eq!(trim_run(&d, &db, &fixture("PostToolUse-Bash-sub")), "");
    assert_eq!(keys(&trim_run(&d, &db, &big_bash())), ["hookEventName", "updatedToolOutput"]);
}

#[test]
fn trim_view_load_error_keeps_flag() {
    let (_t, d, db) = tenv();
    let f = set_flag(&d);
    std::fs::write(d.view_json(), "{").unwrap();
    assert_eq!(trim_run(&d, &db, &fixture("PostToolUse-Bash-sub")), "");
    assert!(f.exists());
}

#[test]
fn trim_store_failure_still_gives_mechanics() {
    let (_t, d, db) = tenv();
    let f = set_flag(&d);
    std::fs::write(d.results(), b"in the way").unwrap();
    assert_eq!(trim_run(&d, &db, &big_bash()), envelope(&m_full()));
    assert!(!f.exists());
}

#[test]
fn trim_failure_event_keeps_flag() {
    let (_t, d, db) = tenv();
    let f = set_flag(&d);
    assert_eq!(trim_run(&d, &db, &fixture("PostToolUseFailure-Read-sub")), "");
    assert!(f.exists());
}
