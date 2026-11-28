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
/// `KEEP_NO_UNIT` in `tests/session_text.rs`.
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

/// The command that runs `case` of `child_case` in a child process with `CHILD` = `root` (stdin null).
fn child_cmd(root: &Path, case: &str) -> Command {
    let mut cmd = Command::new(std::env::current_exe().unwrap());
    cmd.args(["--ignored", "--exact", "child_case", "--nocapture", "--test-threads=1"])
        .env(CHILD, root)
        .env(CASE, case)
        .stdin(Stdio::null());
    cmd
}

/// Asserts that the child test passed; returns its stderr.
fn child_stderr(case: &str, o: std::process::Output) -> String {
    assert!(o.status.success(), "{case}: {o:?}");
    assert!(String::from_utf8(o.stdout).unwrap().contains("child_case ... ok"), "{case}");
    String::from_utf8(o.stderr).unwrap()
}

/// Runs `case` of `child_case` in a child process with `CHILD` = `root`; returns its stderr.
fn child(root: &Path, case: &str) -> String {
    child_stderr(case, child_cmd(root, case).output().unwrap())
}

/// `child` within a 60 s bound (FX-37): a hook that waits, as on a FIFO, is killed and fails the test instead of
/// hanging the suite. A deadline: only the `#[ignore = "timing: …"]` tests use it (Global constraints).
#[cfg(unix)]
fn child_bounded(root: &Path, case: &str) -> String {
    let bound = std::time::Duration::from_secs(60);
    let c = child_cmd(root, case).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let pid = c.id();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(c.wait_with_output());
    });
    match rx.recv_timeout(bound) {
        Ok(o) => child_stderr(case, o.unwrap()),
        Err(_) => {
            // SAFETY: `kill` takes a plain pid and signal; `pid` is this test's own child.
            unsafe {
                libc::kill(pid as libc::pid_t, libc::SIGKILL);
            }
            panic!("the hook did not return within {} s", bound.as_secs());
        }
    }
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
/// `gone`; returns at once without `CHILD`. Case `no_text`: the input `sub`, and the hook prints nothing (the read of
/// the work folder fails). Case `entry`: the input `sub` through `pre_compact::run` (session folder from
/// `ZYGO_SESSION_DIR`), its output then written to stdout and flushed as `zygo hook` does.
#[test]
#[ignore]
fn child_case() {
    let (Some(root), Some(case)) = (std::env::var_os(CHILD), std::env::var(CASE).ok()) else { return };
    let stdin = serde_json::to_vec(&case_input("sub")).unwrap();
    if case == "entry" {
        let (out, code) = guarded(&|b| pre_compact::run(&HookInput::parse(Agent::ClaudeCode, b)?), &stdin);
        let mut so = std::io::stdout();
        std::io::Write::write_all(&mut so, out.as_bytes()).unwrap();
        std::io::Write::flush(&mut so).unwrap();
        assert_eq!(code, 0);
        return;
    }
    let (sub, input) = match case.as_str() {
        "gone" => ("gone", "sub"),
        "no_text" => ("s", "sub"),
        other => ("s", other),
    };
    let d = SessionDir::new(Path::new(&root).join(sub));
    let stdin = serde_json::to_vec(&case_input(input)).unwrap();
    let got = guarded(&|b| pre_compact::run_with(&HookInput::parse(Agent::ClaudeCode, b)?, Some(&d)), &stdin);
    let want = if case == "no_text" { "" } else { KEEP_NO_UNIT };
    assert_eq!(got, (want.to_string(), 0));
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

/// A FIFO at `path`, mode 0600.
#[cfg(unix)]
fn mkfifo(path: &Path) {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    // SAFETY: `c` is a valid NUL-terminated path.
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0, "mkfifo {}", path.display());
}

#[cfg(unix)]
fn mode(p: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).unwrap().permissions().mode() & 0o777
}

/// Asserts that `err` is the one stderr line of a flag of `ID` that cannot be created.
#[cfg(unix)]
fn one_flag_error(d: &SessionDir, err: &str) {
    let flag = d.reinject().join(ID);
    assert!(err.starts_with(&format!("zygo hook: pre-compact: cannot create {}: ", flag.display())), "{err:?}");
    assert!(err.ends_with('\n') && err.lines().count() == 1, "{err:?}");
}

#[cfg(unix)]
#[test]
fn pre_compact_reinject_link_is_not_followed() {
    // FX-37 SX-F1: `reinject` a link to another folder: that folder keeps its mode and gets no flag.
    use std::os::unix::fs::PermissionsExt;
    let (t, d) = sd();
    let other = t.path().join("other");
    std::fs::create_dir(&other).unwrap();
    std::fs::set_permissions(&other, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::os::unix::fs::symlink(&other, d.reinject()).unwrap();
    assert_eq!(run(&d, &fixture("PreCompact-sub")), KEEP_NO_UNIT);
    assert_eq!(mode(&other), 0o755);
    assert!(!other.join(ID).exists());
    one_flag_error(&d, &child(t.path(), "sub"));
    assert_eq!(mode(&other), 0o755);
    assert_eq!(std::fs::read_dir(&other).unwrap().count(), 0);
    assert!(std::fs::symlink_metadata(d.reinject()).unwrap().file_type().is_symlink());
}

#[cfg(unix)]
#[test]
fn pre_compact_flag_link_is_not_followed() {
    // FX-37 SX-F1: the flag name a link to a file of mode 0644: the file keeps its mode and its bytes.
    use std::os::unix::fs::PermissionsExt;
    let (t, d) = sd();
    let target = t.path().join("target");
    std::fs::write(&target, "kept").unwrap();
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o644)).unwrap();
    std::fs::create_dir(d.reinject()).unwrap();
    let flag = d.reinject().join(ID);
    std::os::unix::fs::symlink(&target, &flag).unwrap();
    assert_eq!(run(&d, &fixture("PreCompact-sub")), KEEP_NO_UNIT);
    assert_eq!(mode(&target), 0o644);
    one_flag_error(&d, &child(t.path(), "sub"));
    assert_eq!(mode(&target), 0o644);
    assert_eq!(std::fs::read(&target).unwrap(), b"kept");
    assert!(std::fs::symlink_metadata(&flag).unwrap().file_type().is_symlink());
}

#[cfg(unix)]
#[test]
#[ignore = "timing: the hook must return inside the 60 s bound of `child_bounded` on a FIFO"]
fn pre_compact_flag_fifo_does_not_block() {
    // FX-37 SX-F1: a FIFO at the flag name: the hook returns within the bound with the text (the child asserts
    // `(KEEP_NO_UNIT, 0)`) and one stderr line; the FIFO is left as it is.
    use std::os::unix::fs::FileTypeExt;
    let (t, d) = sd();
    std::fs::create_dir(d.reinject()).unwrap();
    let flag = d.reinject().join(ID);
    mkfifo(&flag);
    one_flag_error(&d, &child_bounded(t.path(), "sub"));
    assert!(std::fs::symlink_metadata(&flag).unwrap().file_type().is_fifo());
}

#[cfg(unix)]
#[test]
fn pre_compact_flag_socket_is_not_a_flag() {
    // FX-37 review 4: a socket at the flag name (an open that cannot wait): no flag, the text, one stderr line; the
    // socket stays.
    use std::os::unix::fs::FileTypeExt;
    let (t, d) = sd();
    std::fs::create_dir(d.reinject()).unwrap();
    let flag = d.reinject().join(ID);
    let _l = std::os::unix::net::UnixListener::bind(&flag).unwrap();
    assert_eq!(run(&d, &fixture("PreCompact-sub")), KEEP_NO_UNIT);
    one_flag_error(&d, &child(t.path(), "sub"));
    assert!(std::fs::symlink_metadata(&flag).unwrap().file_type().is_socket());
}

#[cfg(unix)]
#[test]
fn pre_compact_flag_hard_link_is_not_a_flag() {
    // FX-37 review 3: the flag name a hard link to a file of mode 0644 (two links): the shared file keeps its mode.
    use std::os::unix::fs::PermissionsExt;
    let (t, d) = sd();
    let target = t.path().join("target");
    std::fs::write(&target, "").unwrap();
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o644)).unwrap();
    std::fs::create_dir(d.reinject()).unwrap();
    let flag = d.reinject().join(ID);
    std::fs::hard_link(&target, &flag).unwrap();
    assert_eq!(run(&d, &fixture("PreCompact-sub")), KEEP_NO_UNIT);
    assert_eq!(mode(&target), 0o644);
    one_flag_error(&d, &child(t.path(), "sub"));
    assert_eq!(mode(&target), 0o644);
}

#[cfg(unix)]
#[test]
fn pre_compact_unreadable_work_and_flag_failure_one_line() {
    // FX-37 review 5: the read of the work folder fails and the flag cannot be made (`reinject` a regular file): the
    // hook prints nothing, exits 0 and writes exactly one stderr line that holds both errors.
    use std::os::unix::fs::PermissionsExt;
    let (t, d) = sd();
    let work = d.work();
    std::fs::create_dir_all(&work).unwrap();
    std::fs::write(d.reinject(), "").unwrap();
    std::fs::set_permissions(&work, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::read_dir(&work).is_ok() {
        std::fs::set_permissions(&work, std::fs::Permissions::from_mode(0o700)).unwrap();
        eprintln!("skipped: a work folder of mode 000 can still be read by this user");
        return;
    }
    let err = child(t.path(), "no_text");
    std::fs::set_permissions(&work, std::fs::Permissions::from_mode(0o700)).unwrap();
    let flag = d.reinject().join(ID);
    assert!(err.starts_with("zygo hook: cannot read the work directory; "), "{err:?}");
    assert!(err.contains(&format!("; pre-compact: cannot create {}: ", flag.display())), "{err:?}");
    assert!(err.ends_with('\n') && err.lines().count() == 1, "{err:?}");
}

#[test]
fn pre_compact_error_line_follows_the_text() {
    // FX-37 review, T§14 item 2: through the hook entry, the stderr line comes after the §10.6 text is printed.
    // stdout and stderr of the child go to one file, so their order is kept.
    let (t, d) = sd();
    std::fs::write(d.reinject(), "").unwrap();
    let log = t.path().join("both.txt");
    let f = std::fs::File::create(&log).unwrap();
    let o = child_cmd(t.path(), "entry")
        .env("ZYGO_SESSION_DIR", &d.root)
        .stdout(f.try_clone().unwrap())
        .stderr(f)
        .status()
        .unwrap();
    assert!(o.success(), "{o:?}");
    let both = std::fs::read_to_string(&log).unwrap();
    // The child's own output sits between `test child_case ... ` and `ok`.
    assert!(both.contains("test result: ok. 1 passed"), "{both:?}");
    let text = both.find(KEEP_NO_UNIT).unwrap_or_else(|| panic!("no text: {both:?}"));
    let flag = d.reinject().join(ID);
    let line = both
        .find(&format!("zygo hook: pre-compact: cannot create {}: ", flag.display()))
        .unwrap_or_else(|| panic!("no line: {both:?}"));
    assert!(text < line, "{both:?}");
    assert_eq!(both.matches("zygo hook:").count(), 1, "{both:?}");
}

#[cfg(unix)]
#[test]
fn pre_compact_unreadable_work_folder_still_creates_the_flag() {
    // FX-37 SX-F3: the flag does not depend on the read of the work folder (T§9.2 item 1, T§14 item 2).
    use std::os::unix::fs::PermissionsExt;
    let (_t, d) = sd();
    let work = d.work();
    std::fs::create_dir_all(&work).unwrap();
    std::fs::set_permissions(&work, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::read_dir(&work).is_ok() {
        std::fs::set_permissions(&work, std::fs::Permissions::from_mode(0o700)).unwrap();
        eprintln!("skipped: a work folder of mode 000 can still be read by this user");
        return;
    }
    let got = pre_compact::run_with(&input(&fixture("PreCompact-sub")), Some(&d));
    std::fs::set_permissions(&work, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert!(got.is_err(), "the read of the work folder did not fail");
    let flag = d.reinject().join(ID);
    let m = std::fs::symlink_metadata(&flag).unwrap();
    assert!(m.is_file());
    assert_eq!(m.len(), 0);
    assert_eq!(mode(&flag), 0o600);
    assert_eq!(mode(&d.reinject()), 0o700);
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

/// Asserts that `err` is the one stderr line of a flag of `ID` that is not taken.
#[cfg(unix)]
fn one_trim_error(d: &SessionDir, err: &str) {
    let flag = d.reinject().join(ID);
    assert!(err.starts_with(&format!("zygo hook: trim: cannot remove {}: ", flag.display())), "{err:?}");
    assert!(err.ends_with('\n') && err.lines().count() == 1, "{err:?}");
}

#[cfg(unix)]
#[test]
fn trim_reinject_link_is_not_followed() {
    // FX-37 review 1: `reinject` a link to another folder that holds a file named `ID`: not a flag; no text, the file
    // outside the session folder stays, one stderr line.
    let (t, d, db) = tenv();
    let other = t.path().join("other");
    std::fs::create_dir(&other).unwrap();
    std::fs::write(other.join(ID), "").unwrap();
    std::os::unix::fs::symlink(&other, d.reinject()).unwrap();
    assert_eq!(trim_run(&d, &db, &fixture("PostToolUse-Bash-sub")), "");
    assert!(other.join(ID).exists());
    one_trim_error(&d, &trim_child(t.path(), "under", &[]));
    assert!(other.join(ID).exists());
}

/// FX-37 review 1: an entry at the flag name that is not a regular file is not a flag: no text, the entry stays, one
/// stderr line. `make` puts the entry at the flag path.
#[cfg(unix)]
fn trim_not_a_flag(make: &dyn Fn(&Path, &Path)) {
    let (t, d, db) = tenv();
    std::fs::create_dir(d.reinject()).unwrap();
    let flag = d.reinject().join(ID);
    make(t.path(), &flag);
    assert_eq!(trim_run(&d, &db, &fixture("PostToolUse-Bash-sub")), "");
    assert!(std::fs::symlink_metadata(&flag).is_ok());
    one_trim_error(&d, &trim_child(t.path(), "under", &[]));
    assert!(std::fs::symlink_metadata(&flag).is_ok());
}

#[cfg(unix)]
#[test]
fn trim_flag_link_is_not_a_flag() {
    trim_not_a_flag(&|t, flag| {
        std::fs::write(t.join("target"), "").unwrap();
        std::os::unix::fs::symlink(t.join("target"), flag).unwrap();
    });
}

#[cfg(unix)]
#[test]
fn trim_flag_fifo_is_not_a_flag() {
    // The flag is never opened, so a FIFO cannot make the hook wait; no deadline is needed.
    trim_not_a_flag(&|_, flag| mkfifo(flag));
}

#[cfg(unix)]
#[test]
fn trim_flag_socket_is_not_a_flag() {
    trim_not_a_flag(&|_, flag| {
        // The socket file stays after the listener is dropped.
        drop(std::os::unix::net::UnixListener::bind(flag).unwrap());
    });
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
