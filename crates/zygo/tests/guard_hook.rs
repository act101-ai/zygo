mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{json, Value};
use zygo_core::{session_dir::SessionDir, session_view::SessionView, Agent};

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
/// `zygo hook guard` with the environment set on the child only.
fn hook(home: &Path, sd: &SessionDir, stdin: &[u8]) -> (Option<i32>, String, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_zygo"));
    cmd.args(["hook", "guard"]);
    cmd.env("HOME", home).env("LOCALAPPDATA", home).env("APPDATA", home).env("ZYGO_CONFIG", home.join("config.toml"))
        .env("ZYGO_SESSION_DIR", &sd.root).env_remove("ZYGO_AGENT").current_dir(home);
    let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    common::write_stdin(child.stdin.take().unwrap(), stdin);
    let out = child.wait_with_output().unwrap();
    (out.status.code(), String::from_utf8(out.stdout).unwrap(), String::from_utf8(out.stderr).unwrap())
}

#[test] // §20 #29b through the binary; C6 wrapper (Review focus 5)
fn binary_guard_denies_redaction_and_exits_0_on_bad_input() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path();
    let sd = session(home);
    let mut w = fixture("PreToolUse-Write-sub.json");
    w["cwd"] = json!(home.to_str().unwrap());
    w["tool_input"]["content"] = json!("[REDACTED:openai_key]");
    let (code, out, err) = hook(home, &sd, w.to_string().as_bytes());
    assert_eq!((code, err.as_str()), (Some(0), ""));
    assert_eq!(serde_json::from_str::<Value>(&out).unwrap(), json!({"hookSpecificOutput": {"hookEventName": "PreToolUse",
        "permissionDecision": "deny", "permissionDecisionReason": "`[REDACTED:<id>]` marks a secret zygo removed; never write it into a file."}}));
    w["tool_input"]["content"] = json!("alpha\n");
    assert_eq!(hook(home, &sd, w.to_string().as_bytes()), (Some(0), String::new(), String::new()));
    let mut newer = w.clone();
    newer["future_field"] = json!({"k": [1]});
    newer["hook_event_name"] = json!("PreToolUseV2");
    assert_eq!(hook(home, &sd, newer.to_string().as_bytes()), (Some(0), String::new(), String::new()));
    for bytes in [&b"not json"[..], b"", b"[]"] {
        let (code, out, err) = hook(home, &sd, bytes);
        assert_eq!((code, out.as_str(), err.lines().count()), (Some(0), "", 1), "{}", String::from_utf8_lossy(bytes));
    }
}

/// A directory outside every production temp root (§13.4: the system temp dir, `/tmp`, `/var/tmp`, canonicalised), as
/// computed in this test process: `out_of_scope` exempts any path under a temp root, so the scope deny cannot be tested
/// there (e.g. `CARGO_TARGET_DIR` under `/tmp`). Tries `CARGO_TARGET_TMPDIR`, then this workspace's `target/tmp`; panics
/// if both are under a temp root. This set can be larger than the child's (its TMPDIR is the test's `scratch` dir), so
/// the helper is conservative.
fn outside_temp_roots() -> PathBuf {
    let roots = zygo_work::guards::temp_roots(&std::env::temp_dir());
    let candidates = [PathBuf::from(env!("CARGO_TARGET_TMPDIR")), Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp")];
    for c in &candidates {
        if std::fs::create_dir_all(c).is_err() { continue; }
        let Ok(canon) = c.canonicalize() else { continue };
        // Windows canonical paths carry `\\?\`; the child keeps getting the plain path there, as before N52.
        if !roots.iter().any(|r| canon.starts_with(r)) { return if cfg!(windows) { c.clone() } else { canon }; }
    }
    panic!("no directory outside the temp roots {roots:?}: {candidates:?}")
}

#[test] // Round 2: a configurable guard (scope at enforce) denies through the binary; the production temp roots (env_ctx)
#[ignore = "timing: real git inside the 2 s repo lookup bound of the guard/work hook"]
fn binary_guard_scope_enforce_denies_and_exempts_tmpdir() {
    // Outside every production temp root (§13.4), found by `outside_temp_roots`; the child's TMPDIR is a directory inside
    // this test dir, so only the production temp-root list exempts it.
    let base = outside_temp_roots();
    let d = tempfile::tempdir_in(&base).unwrap();
    let home = d.path();
    let sd = session(home);
    std::fs::write(home.join("config.toml"), "[guards]\nscope = \"enforce\"\n").unwrap();
    for dir in ["src", "scratch"] { std::fs::create_dir_all(home.join(dir)).unwrap(); }
    // A real (unborn) repository: a `.git` entry git refuses is a failed read with a line (N53 round 2).
    let init = Command::new("git").args(["init", "-q", "-b", "main"]).current_dir(home).env("GIT_CEILING_DIRECTORIES", &base)
        .output().unwrap();
    assert!(init.status.success(), "{}", String::from_utf8_lossy(&init.stderr));
    let sub = |tool: &str, tin: Value| {
        let mut v = fixture("PreToolUse-Bash-sub.json");
        v["agent_id"] = json!("acfd66ba917fc3494");
        v["agent_type"] = json!("zygo-code");
        v["cwd"] = json!(home.to_str().unwrap());
        v["tool_name"] = json!(tool);
        v["tool_input"] = tin;
        v
    };
    let run = |args: &[&str], v: &Value| {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_zygo"));
        cmd.args(args).env("HOME", home).env("LOCALAPPDATA", home).env("APPDATA", home).env("ZYGO_CONFIG", home.join("config.toml"))
            .env("ZYGO_SESSION_DIR", &sd.root).env("TMPDIR", home.join("scratch")).env("TMP", home.join("scratch"))
            .env("TEMP", home.join("scratch")).env("GIT_CEILING_DIRECTORIES", &base).env_remove("ZYGO_AGENT").current_dir(home);
        let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        common::write_stdin(child.stdin.take().unwrap(), v.to_string().as_bytes());
        let out = child.wait_with_output().unwrap();
        (out.status.code(), String::from_utf8(out.stdout).unwrap(), String::from_utf8(out.stderr).unwrap())
    };
    let begin = sub("mcp__zygo__work_begin", json!({"unit": "beads:bd-a1b2", "round": 1, "check": "cargo test -p demo", "files": ["src/lib.rs"]}));
    assert_eq!(run(&["hook", "work", "--event", "begin"], &begin), (Some(0), String::new(), String::new()));
    let edit = |p: &Path| sub("Edit", json!({"file_path": p, "old_string": "a", "new_string": "b"}));
    let (code, out, err) = run(&["hook", "guard"], &edit(&home.join("src/main.rs")));
    assert_eq!((code, err.as_str()), (Some(0), ""));
    assert_eq!(serde_json::from_str::<Value>(&out).unwrap(), json!({"hookSpecificOutput": {"hookEventName": "PreToolUse",
        "permissionDecision": "deny",
        "permissionDecisionReason": "zygo: `src/main.rs` is not in this unit's files; if the change is needed, report it. Ask the lead to widen the brief."}}));
    assert_eq!(run(&["hook", "guard"], &edit(&home.join("src/lib.rs"))), (Some(0), String::new(), String::new()));
    assert_eq!(run(&["hook", "guard"], &edit(&home.join("scratch/x.rs"))), (Some(0), String::new(), String::new()));
}

// N53: a failed read of `.zygo.toml` is one stderr line (C6, §13.5 note 2026-11-19); an invalid one too (§13.5).

/// The first `git` file on this process's `PATH`.
#[cfg(unix)]
fn real_git() -> PathBuf {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path).map(|d| d.join("git")).find(|g| g.is_file()).expect("git on PATH")
}
#[cfg(unix)]
const ENFORCE: &str = "version = 1\n[guards]\nscope = \"enforce\"\n";
/// Past both 2 s bounds.
#[cfg(unix)]
const SLEEP: &str = "sleep 3";

#[cfg(unix)]
#[derive(Clone, Copy)]
struct Probe<'a> {
    /// `.zygo.toml` committed on `main`; `None`: no repository.
    policy: Option<&'a str>,
    /// `(word, sh)`: a `git` stub first on the child's PATH runs `sh` when `word` is an argument.
    stub: Option<(&'a str, &'a str)>,
    /// False: no session view, so `units_on` loads the config a second time in the run.
    view: bool,
    /// More environment on the hook children.
    env: &'a [(&'a str, &'a str)],
    /// Delete the object file of this revision (e.g. `HEAD:.zygo.toml`, `HEAD^{tree}`, `HEAD`) after the commit.
    drop: Option<&'a str>,
    /// Write `(path under the checkout, content)` after the commit.
    write: Option<(&'a str, &'a str)>,
}
#[cfg(unix)]
const P: Probe = Probe { policy: Some(ENFORCE), stub: None, view: true, env: &[], drop: None, write: None };

/// The N52 D2 probe: `zygo hook work --event begin`, then `zygo hook guard` on an out-of-scope `Edit`.
#[cfg(unix)]
fn d2(p: Probe) -> [(Option<i32>, String, String); 2] {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir_in(outside_temp_roots()).unwrap();
    let base = d.path().canonicalize().unwrap();
    let home = base.join("r");
    for dir in ["src", "scratch", "bin"] { std::fs::create_dir_all(home.join(dir)).unwrap(); }
    let sd = session(&home);
    if !p.view { std::fs::remove_file(sd.view_json()).unwrap(); }
    let git = real_git();
    let g = |args: &[&str]| {
        let out = Command::new(&git).args(args).current_dir(&home).env("GIT_CEILING_DIRECTORIES", &base).output().unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8(out.stdout).unwrap()
    };
    if let Some(policy) = p.policy {
        g(&["init", "-q", "-b", "main"]);
        std::fs::write(home.join(".zygo.toml"), policy).unwrap();
        g(&["add", ".zygo.toml"]);
        g(&["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false", "commit", "-q", "-m", "policy"]);
        if let Some(rev) = p.drop {
            let id = g(&["rev-parse", rev]);
            let id = id.trim();
            std::fs::remove_file(home.join(".git/objects").join(&id[..2]).join(&id[2..])).unwrap();
        }
        if let Some((path, content)) = p.write {
            std::fs::write(home.join(path), content).unwrap();
        }
    }
    if let Some((word, sh)) = p.stub {
        let stub = home.join("bin/git");
        std::fs::write(&stub, format!("#!/bin/sh\ncase \" $* \" in *\" {word} \"*) {sh};; esac\nexec '{}' \"$@\"\n", git.display())).unwrap();
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let path = format!("{}:{}", home.join("bin").display(), std::env::var("PATH").unwrap_or_default());
    let sub = |tool: &str, tin: Value| {
        let mut v = fixture("PreToolUse-Bash-sub.json");
        v["agent_id"] = json!("acfd66ba917fc3494");
        v["agent_type"] = json!("zygo-code");
        v["cwd"] = json!(home.to_str().unwrap());
        v["tool_name"] = json!(tool);
        v["tool_input"] = tin;
        v
    };
    let run = |args: &[&str], v: &Value| {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_zygo"));
        cmd.args(args).env("HOME", &home).env("LOCALAPPDATA", &home).env("APPDATA", &home).env("ZYGO_CONFIG", home.join("config.toml"))
            .env("ZYGO_SESSION_DIR", &sd.root).env("TMPDIR", home.join("scratch")).env("TMP", home.join("scratch"))
            .env("TEMP", home.join("scratch")).env("PATH", &path).env("GIT_CEILING_DIRECTORIES", &base)
            .env_remove("ZYGO_AGENT").current_dir(&home);
        for (k, v) in p.env { cmd.env(k, v); }
        let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        common::write_stdin(child.stdin.take().unwrap(), v.to_string().as_bytes());
        let out = child.wait_with_output().unwrap();
        (out.status.code(), String::from_utf8(out.stdout).unwrap(), String::from_utf8(out.stderr).unwrap())
    };
    let begin = sub("mcp__zygo__work_begin", json!({"unit": "beads:bd-a1b2", "round": 1, "check": "cargo test -p demo", "files": ["src/lib.rs"]}));
    let b = run(&["hook", "work", "--event", "begin"], &begin);
    let edit = sub("Edit", json!({"file_path": home.join("src/main.rs"), "old_string": "a", "new_string": "b"}));
    [b, run(&["hook", "guard"], &edit)]
}

/// With and without a session view: begin and guard each allow (user level `monitor`) and write exactly `lines`.
#[cfg(unix)]
fn assert_lines_once(p: Probe, lines: &str) {
    for view in [true, false] {
        for (i, r) in d2(Probe { view, ..p }).into_iter().enumerate() {
            assert_eq!(r, (Some(0), String::new(), lines.to_string()), "view {view}, run {i}");
        }
    }
}
#[cfg(unix)]
fn unread(why: &str) -> String {
    format!("zygo hook: .zygo.toml not read ({why}); user and team levels apply\n")
}

#[cfg(unix)]
#[test]
#[ignore = "timing: real git inside the 2 s repo lookup bound of the guard/work hook"]
fn binary_guard_repo_file_enforce_denies_with_real_git() {
    let [b, (code, out, err)] = d2(P);
    assert_eq!(b, (Some(0), String::new(), String::new()));
    assert_eq!((code, err.as_str()), (Some(0), ""));
    assert_eq!(serde_json::from_str::<Value>(&out).unwrap(), json!({"hookSpecificOutput": {"hookEventName": "PreToolUse",
        "permissionDecision": "deny",
        "permissionDecisionReason": "zygo: `src/main.rs` is not in this unit's files; if the change is needed, report it. Ask the lead to widen the brief."}}));
}

#[cfg(unix)]
#[test]
fn binary_guard_slow_rev_parse_says_unread_once() {
    assert_lines_once(Probe { stub: Some(("rev-parse", SLEEP)), ..P }, &unread("git rev-parse: timed out"));
}

#[cfg(unix)]
#[test]
#[ignore = "timing: real git inside the 2 s repo lookup bound of the guard/work hook"]
fn binary_guard_slow_worktree_list_says_unread_once() {
    assert_lines_once(Probe { stub: Some(("worktree", SLEEP)), ..P }, &unread("git worktree list: timed out"));
}

#[cfg(unix)]
#[test]
#[ignore = "timing: real git inside the 2 s repo lookup bound of the guard/work hook"]
fn binary_guard_slow_default_branch_says_unread_once() {
    assert_lines_once(Probe { stub: Some(("refs/remotes/origin/HEAD", SLEEP)), ..P }, &unread("git symbolic-ref: timed out"));
}

#[cfg(unix)]
#[test]
#[ignore = "timing: real git inside the 2 s repo lookup bound of the guard/work hook"]
fn binary_guard_slow_show_says_unread_once() {
    assert_lines_once(Probe { stub: Some(("show", SLEEP)), ..P }, &unread("git show: timed out"));
}

#[cfg(unix)]
#[test]
#[ignore = "timing: real git inside the 2 s repo lookup bound of the guard/work hook"]
fn binary_guard_dubious_ownership_is_a_failed_read() {
    let line = unread("git rev-parse: failed");
    // zygo's git keeps the host's system config (it removes only `GIT_CONFIG*` variables), so a host whose system
    // config sets `safe.directory` (CI runners) never refuses; the stub below proves the refusal path there.
    let system_safe = Command::new(real_git()).args(["config", "--system", "--get-all", "safe.directory"]).output()
        .is_ok_and(|o| o.status.success());
    if !system_safe {
        for r in d2(Probe { env: &[("GIT_TEST_ASSUME_DIFFERENT_OWNER", "1")], ..P }) {
            assert_eq!(r, (Some(0), String::new(), line.clone()));
        }
    }
    let refuse = "echo \"fatal: detected dubious ownership in repository at '/r'\" >&2; exit 128";
    assert_lines_once(Probe { stub: Some(("rev-parse", refuse)), ..P }, &line);
}

#[cfg(unix)]
#[test]
#[ignore = "timing: real git inside the 2 s repo lookup bound of the guard/work hook"]
fn binary_guard_bad_object_is_a_failed_read() {
    assert_lines_once(Probe { drop: Some("HEAD:.zygo.toml"), ..P }, &unread("git show: failed"));
}

// N53 round 2: corruption that git reports with an "absent" message is a failed read (fail closed).

#[cfg(unix)]
#[test]
#[ignore = "timing: real git inside the 2 s repo lookup bound of the guard/work hook"]
fn binary_guard_missing_tree_is_a_failed_read() {
    assert_lines_once(Probe { drop: Some("HEAD^{tree}"), ..P }, &unread("git show: failed"));
}

#[cfg(unix)]
#[test]
#[ignore = "timing: real git inside the 2 s repo lookup bound of the guard/work hook"]
fn binary_guard_missing_commit_is_a_failed_read() {
    assert_lines_once(Probe { drop: Some("HEAD"), ..P }, &unread("git show: failed"));
}

#[cfg(unix)]
#[test]
#[ignore = "timing: real git inside the 2 s repo lookup bound of the guard/work hook"]
fn binary_guard_ref_to_missing_object_is_a_failed_read() {
    let id = format!("{}\n", "1234567890".repeat(4));
    assert_lines_once(Probe { write: Some((".git/refs/heads/main", &id)), ..P }, &unread("git show: failed"));
}

#[cfg(unix)]
#[test]
#[ignore = "timing: real git inside the 2 s repo lookup bound of the guard/work hook"]
fn binary_guard_garbage_ref_is_a_failed_read() {
    assert_lines_once(Probe { write: Some((".git/refs/heads/main", "garbage\n")), ..P }, &unread("git show: failed"));
}

#[cfg(unix)]
#[test] // a `.git` entry git calls "not a git repository": a broken repository
#[ignore = "timing: real git inside the 2 s repo lookup bound of the guard/work hook"]
fn binary_guard_garbage_head_is_a_failed_read() {
    assert_lines_once(Probe { write: Some((".git/HEAD", "garbage\n")), ..P }, &unread("git rev-parse: failed"));
}

#[cfg(unix)]
#[test] // only the E2 reason words reach a line
#[ignore = "timing: real git inside the 2 s repo lookup bound of the guard/work hook"]
fn binary_guard_absent_worktree_list_is_failed() {
    let absent = "echo 'fatal: not a git repository (or any of the parent directories): .git' >&2; exit 128";
    assert_lines_once(Probe { stub: Some(("worktree", absent)), ..P }, &unread("git worktree list: failed"));
}

/// A `git` stub body: 192 KiB (three times the 64 KiB pipe buffer and the 64 KiB the readers keep) written to stderr by
/// shell builtins only, no child process: a 1 KiB literal joined 192 times, then one `printf`. The reader keeps 64 KiB
/// and the pipe holds 64 KiB, so a writer nobody drains is cut off by `SIGPIPE` before it can finish.
#[cfg(unix)]
fn pad_stderr() -> String {
    format!("k={}; x=; i=0; while [ $i -lt 192 ]; do x=$x$k; i=$((i+1)); done; printf %s \"$x\" >&2", "x".repeat(1024))
}

#[cfg(unix)]
#[test] // 192 KiB of stderr and exit 128: read within the bound, a failed read (not "timed out")
#[ignore = "timing: real git inside the 2 s repo lookup bound of the guard/work hook"]
fn binary_guard_large_stderr_is_a_failed_read() {
    let big = format!("{}; exit 128", pad_stderr());
    assert_lines_once(Probe { stub: Some(("rev-parse", &big)), ..P }, &unread("git rev-parse: failed"));
    assert_lines_once(Probe { stub: Some(("show", &big)), ..P }, &unread("git show: failed"));
}

#[cfg(unix)]
#[test] // the stderr past the 64 KiB kept is drained: the child finishes and its own result stands (no line, deny)
#[ignore = "timing: real git inside the 2 s repo lookup bound of the guard/work hook"]
fn binary_guard_large_stderr_does_not_change_a_successful_read() {
    for word in ["rev-parse", "show"] {
        let [b, (code, out, err)] = d2(Probe { stub: Some((word, &pad_stderr())), ..P });
        assert_eq!(b, (Some(0), String::new(), String::new()), "{word}");
        assert_eq!((code, err.as_str()), (Some(0), ""), "{word}");
        assert_eq!(serde_json::from_str::<Value>(&out).unwrap()["hookSpecificOutput"]["permissionDecision"], "deny", "{word}: {out}");
    }
}

#[cfg(unix)]
#[test] // N53 round 3: the caller's `GIT_DIR` never selects the repository `.zygo.toml` is read from
#[ignore = "timing: real git inside the 2 s repo lookup bound of the guard/work hook"]
fn binary_guard_repo_file_ignores_caller_git_dir() {
    let other = tempfile::tempdir_in(outside_temp_roots()).unwrap();
    let b = other.path().canonicalize().unwrap();
    for args in [&["init", "-q", "-b", "main"][..], &["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false",
        "commit", "-q", "--allow-empty", "-m", "b"]] {
        let out = Command::new(real_git()).args(args).current_dir(&b).env("GIT_CEILING_DIRECTORIES", other.path()).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    }
    let git_dir = b.join(".git");
    let [begin, (code, out, err)] = d2(Probe { env: &[("GIT_DIR", git_dir.to_str().unwrap())], ..P });
    assert_eq!(begin, (Some(0), String::new(), String::new()));
    assert_eq!((code, err.as_str()), (Some(0), ""));
    assert_eq!(serde_json::from_str::<Value>(&out).unwrap()["hookSpecificOutput"]["permissionDecision"], "deny", "{out}");
}

#[cfg(unix)]
#[test] // git runs with `LC_ALL=C`: a translated "absent" message would be a failed read
#[ignore = "timing: real git inside the 2 s repo lookup bound of the guard/work hook"]
fn binary_guard_git_gets_c_locale() {
    let sh = "if [ \"$LC_ALL\" = C ]; then echo 'fatal: ref refs/remotes/origin/HEAD is not a symbolic ref' >&2; \
        else echo 'fatal: Referenz refs/remotes/origin/HEAD ist keine symbolische Referenz' >&2; fi; exit 128";
    let [b, (code, out, err)] = d2(Probe { stub: Some(("refs/remotes/origin/HEAD", sh)), env: &[("LC_ALL", "de_DE.UTF-8")], ..P });
    assert_eq!(b, (Some(0), String::new(), String::new()));
    assert_eq!((code, err.as_str()), (Some(0), ""));
    assert_eq!(serde_json::from_str::<Value>(&out).unwrap()["hookSpecificOutput"]["permissionDecision"], "deny");
}

#[cfg(unix)]
#[test] // §13.5 "an invalid file is ignored with one stderr line"; C3: no file value in it
#[ignore = "timing: real git inside the 2 s repo lookup bound of the guard/work hook"]
fn binary_guard_invalid_repo_file_says_so_once() {
    assert_lines_once(Probe { policy: Some("version = 2\n[guards]\nscope = \"enforce\"\n"), ..P },
        "zygo hook: .zygo.toml: invalid (version is not 1); ignored\n");
    let secret = format!("sk-{}", "n53secret");
    let policy = format!("version = 1\n[guards]\nscope = \"{secret}\"\n");
    for view in [true, false] {
        for r in d2(Probe { policy: Some(&policy), view, ..P }) {
            assert_eq!((r.0, r.1.as_str(), r.2.lines().count()), (Some(0), "", 1), "{}", r.2);
            assert!(r.2.starts_with("zygo hook: .zygo.toml: invalid ("), "{}", r.2);
            assert!(!r.2.contains("n53secret"), "{}", r.2);
        }
    }
}

#[cfg(unix)]
#[test]
#[ignore = "timing: real git inside the 2 s repo lookup bound of the guard/work hook"]
fn binary_guard_warning_with_newline_is_one_line() {
    assert_lines_once(Probe { policy: Some("version = 1\n[\"a\\nb\"]\nx = 1\n"), ..P },
        "zygo hook: .zygo.toml: [a\\nb] ignored; the repository file may set only [guards], [navigation], [limits], [work]\n");
}

#[cfg(unix)]
#[test]
fn binary_guard_not_a_repository_writes_no_line() {
    for r in d2(Probe { policy: None, ..P }) {
        assert_eq!(r, (Some(0), String::new(), String::new()));
    }
}

#[cfg(unix)]
#[test]
fn binary_guard_no_git_says_unread_only_in_a_repository() {
    // The system temp dir: no `.git` above it on a normal machine, so the first run is "not a repository".
    let d = tempfile::tempdir().unwrap();
    let home = d.path();
    let marker = home.canonicalize().unwrap().ancestors().find(|a| a.join(".git").exists()).map(Path::to_path_buf);
    assert!(marker.is_none(), "precondition: no `.git` above the system temp dir {home:?}; found one in {marker:?}");
    let sd = session(home);
    let mut w = fixture("PreToolUse-Write-sub.json");
    w["cwd"] = json!(home.to_str().unwrap());
    let run = || {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_zygo"));
        cmd.args(["hook", "guard"]).env("HOME", home).env("LOCALAPPDATA", home).env("APPDATA", home)
            .env("ZYGO_CONFIG", home.join("config.toml")).env("ZYGO_SESSION_DIR", &sd.root).env("PATH", "")
            .env_remove("ZYGO_AGENT").current_dir(home);
        let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        common::write_stdin(child.stdin.take().unwrap(), w.to_string().as_bytes());
        let out = child.wait_with_output().unwrap();
        (out.status.code(), String::from_utf8(out.stdout).unwrap(), String::from_utf8(out.stderr).unwrap())
    };
    assert_eq!(run(), (Some(0), String::new(), String::new()));
    std::fs::create_dir(home.join(".git")).unwrap();
    assert_eq!(run(), (Some(0), String::new(), unread("git rev-parse: tool not found")));
}

#[test] // N66: in a session the guard hook reads `$ZYGO_SESSION_DIR/team.toml` (§14 note 2026-11-19); one it cannot read is one stderr line and the user levels apply (C6, §13.5 note)
fn binary_guard_reads_the_session_team_file() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path();
    let sd = session(home);
    let mut v = fixture("PreToolUse-Bash-main.json");
    v["cwd"] = json!(home.to_str().unwrap());
    v["tool_input"]["command"] = json!("cargo test | head -5");
    let input = v.to_string();
    assert_eq!(hook(home, &sd, input.as_bytes()), (Some(0), String::new(), String::new()));   // output at its default, monitor
    std::fs::write(sd.root.join("team.toml"), "[guards]\nmin_level = \"enforce\"\n").unwrap();
    let (code, out, err) = hook(home, &sd, input.as_bytes());
    assert_eq!((code, err.as_str()), (Some(0), ""));
    assert_eq!(serde_json::from_str::<Value>(&out).unwrap(), json!({"hookSpecificOutput": {"hookEventName": "PreToolUse",
        "permissionDecision": "deny",
        "permissionDecisionReason": "zygo: do not shorten output with `| head`/`| tail`; zygo stores large results and gives you `result_search`."}}));
    std::fs::remove_file(sd.root.join("team.toml")).unwrap();
    std::fs::create_dir(sd.root.join("team.toml")).unwrap();
    let (code, out, err) = hook(home, &sd, input.as_bytes());
    assert_eq!((code, out.as_str()), (Some(0), ""));
    assert!(err.starts_with("zygo hook: team.toml not read (") && err.ends_with("); user levels apply\n") && err.lines().count() == 1, "{err}");
} // mutations: `env_config` keeps `team: None` (→ no deny); the session dir taken from elsewhere than `ZYGO_SESSION_DIR`
  // (→ no deny); a read error drops the whole config or exits non-zero (→ C6 broken); the line not written (→ empty stderr)

#[test] // N66 H2: with the team layer a user `[[model]]` is ignored (§6.1); hooks use no models, so that warning is no hook line (C6)
fn binary_guard_says_nothing_of_a_user_model_in_a_session() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path();
    let sd = session(home);
    std::fs::write(home.join("config.toml"), "[[model]]\nid = \"m\"\n[model.facts]\nreasoning = \"always\"\neffort_levels = [\"low\"]\n\
        context_window = 200000\nmax_output_tokens = 64000\n[[model.provider]]\nname = \"p\"\nbase_url = \"https://p.example.invalid\"\n\
        upstream_model = \"u\"\ndialects = [\"anthropic\"]\nkey = \"env:ZYGO_N66_TEST_KEY\"\n").unwrap();
    std::fs::write(sd.root.join("team.toml"), "[guards]\nmin_level = \"enforce\"\n").unwrap();
    let mut v = fixture("PreToolUse-Bash-main.json");
    v["cwd"] = json!(home.to_str().unwrap());
    v["tool_input"]["command"] = json!("cargo test | head -5");
    let (code, out, err) = hook(home, &sd, v.to_string().as_bytes());
    assert_eq!((code, err.as_str()), (Some(0), ""));
    assert_eq!(serde_json::from_str::<Value>(&out).unwrap()["hookSpecificOutput"]["permissionDecision"], "deny"); // the team layer loaded
} // mutation: the `[[model]]` warning not filtered (→ one `zygo hook: config: [[model]] …` line)

#[test] // FIX-21, §13.4: the attribution guard through the binary, on a pull request body; a product name passes
fn binary_guard_denies_claude_attribution_in_a_pr_body() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path();
    let sd = session(home);
    let mut v = fixture("PreToolUse-Bash-sub.json");
    v["cwd"] = json!(home.to_str().unwrap());
    v["tool_input"] = json!({"command": "gh pr create --title t --body \"Co-Authored-By: Claude <noreply@anthropic.com>\""});
    let (code, out, err) = hook(home, &sd, v.to_string().as_bytes());
    assert_eq!((code, err.as_str()), (Some(0), ""));
    assert_eq!(serde_json::from_str::<Value>(&out).unwrap(), json!({"hookSpecificOutput": {"hookEventName": "PreToolUse",
        "permissionDecision": "deny",
        "permissionDecisionReason": "zygo: Claude attribution is never allowed in commits, tags, notes, pull requests, issues, comments, reviews or releases. `noreply@anthropic.com` is in a command argument; reword that text without it and run the command again."}}));
    v["tool_input"] = json!({"command": "gh pr create --title t --body \"Adds the Claude Code host\""});
    assert_eq!(hook(home, &sd, v.to_string().as_bytes()), (Some(0), String::new(), String::new()));
}
