//! WD-16: `zygo config show [--layer <name>]`, `zygo config check`, `zygo profile list`, `zygo profile show <n>`,
//! `zygo loadout list` and `zygo loadout show repository` show the repository files (workspace definition design R§10
//! rows; R§18 items 9 and 22; R-C2: nothing of unapproved files is shown; R-C7: every value through `esc`). The config
//! and profile tests are not `cfg(unix)`: they start nothing and have no OP5 check; `zygo loadout` exits 2 on Windows
//! (OP5), so its tests are in `mod unix`. No network: the user config turns the built-in marketplace off
//! (`common::NO_MARKETPLACE`), and each run asserts that no marketplace sync ran (`common::assert_no_marketplace_sync`).
//! No agent starts, so no stub agent. Every test git call has no user or system config and no template.
mod common;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use zygo_config::repo_def::{approve_workspace_in, check_default, SystemGit, Use};
use zygo_work::repo::checkout;
use zygo_workspace::identity::workspace_id;

const WS: &str = ".zygo/workspace.toml";
const PROF: &str = ".zygo/profiles/rust.toml";
const MIN: &str = "version = 1\n";
const RUST: &str = "overrides = { review = \"gpt-6-sol\" }\ndisabled = [\"design\"]\n";
const INVALID: &str = "version = 1\n[members.a]\nargs = [\"x\"]\n";
const WHY: &str = ".zygo/workspace.toml: members.a.args: not allowed in a repository file";
const REF: &str = "refs/remotes/origin/main";
const DISABLED: &str = "profile.rust.disabled = [\"design\"]  # workspace-files .zygo/profiles/rust.toml";
const REVIEW: &str = "profile.rust.overrides.review = \"gpt-6-sol\"  # workspace-files .zygo/profiles/rust.toml";
const PROF_FILE: &str = "  # workspace-files .zygo/profiles/rust.toml";
const WS_FILE: &str = "  # workspace-files .zygo/workspace.toml";

/// The file that turns user and system git config off.
fn no_config() -> &'static str {
    if cfg!(windows) { "NUL" } else { "/dev/null" }
}

/// `git -C <dir> <args>` with no user or system config, no template, no signing, a fixed identity,
/// `core.autocrlf=false` and `core.protectNTFS=false`; no `GIT_DIR`-like variable of the caller.
fn git_cmd(dir: &Path, args: &[&str]) -> Command {
    let mut c = Command::new("git");
    c.env("GIT_CONFIG_GLOBAL", no_config())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TEMPLATE_DIR", "")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR")
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.invalid",
            "-c",
            "core.autocrlf=false",
            "-c",
            "core.protectNTFS=false",
            "-C",
        ])
        .arg(dir)
        .args(args);
    c
}

/// Runs it, asserts success, returns stdout trimmed.
fn git(dir: &Path, args: &[&str]) -> String {
    let o = git_cmd(dir, args).stdin(Stdio::null()).output().expect("git on PATH");
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8(o.stdout).unwrap().trim().to_string()
}

/// `hash-object -w --stdin` of `bytes` in `dir`; returns the blob id.
fn blob(dir: &Path, bytes: &[u8]) -> String {
    let mut child = git_cmd(dir, &["hash-object", "-w", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("git on PATH");
    child.stdin.take().unwrap().write_all(bytes).unwrap();
    let o = child.wait_with_output().unwrap();
    assert!(o.status.success(), "hash-object: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8(o.stdout).unwrap().trim().to_string()
}

/// Adds each regular file to the index of `dir` (nothing on disk), then `commit -q -m c`.
fn commit_files(dir: &Path, ents: &[(&str, &str)]) {
    for (path, text) in ents {
        let oid = blob(dir, text.as_bytes());
        git(dir, &["update-index", "--add", "--cacheinfo", &format!("100644,{oid},{path}")]);
    }
    git(dir, &["commit", "-q", "-m", "c"]);
}

struct Fx {
    t: tempfile::TempDir,
    c: PathBuf,
}

/// `o.git` (bare), `w` with a `README` seed commit, then `ents` in a second commit when not empty, pushed; `c` cloned
/// `--no-checkout`.
fn fx(ents: &[(&str, &str)]) -> Fx {
    let t = tempfile::tempdir().unwrap();
    let (o, w) = (t.path().join("o.git"), t.path().join("w"));
    git(t.path(), &["init", "-q", "--bare", "-b", "main", o.to_str().unwrap()]);
    git(t.path(), &["init", "-q", "-b", "main", w.to_str().unwrap()]);
    commit_files(&w, &[("README", "seed")]);
    if !ents.is_empty() {
        commit_files(&w, ents);
    }
    git(&w, &["remote", "add", "origin", "../o.git"]);
    git(&w, &["push", "-q", "origin", "main"]);
    git(t.path(), &["clone", "-q", "--no-checkout", "o.git", "c"]);
    let c = t.path().join("c");
    Fx { t, c }
}

/// `f.t/home` with `config.toml` = `user_toml`, a newline, then [`common::NO_MARKETPLACE`] (its `[[marketplace]]`
/// header closes any open table, so it goes after the other keys). A second call writes the file again.
fn home(f: &Fx, user_toml: &str) -> PathBuf {
    let h = f.t.path().join("home");
    std::fs::create_dir_all(&h).unwrap();
    std::fs::write(h.join("config.toml"), format!("{user_toml}\n{}", common::NO_MARKETPLACE)).unwrap();
    h
}

/// `Paths.approvals_file` of `Paths::from_env` under `common::scratch_home`.
fn approvals(home: &Path) -> PathBuf {
    if cfg!(windows) { home.join("zygo").join("approvals.toml") } else { home.join(".local/share/zygo/approvals.toml") }
}

fn id(c: &Path) -> String {
    workspace_id(&checkout(c, Instant::now() + Duration::from_secs(60)).unwrap().common_dir)
}

/// The digest of the files on the default branch, not stored.
fn digest(f: &Fx) -> String {
    match check_default(&f.c, &id(&f.c), &f.t.path().join("none.toml"), &SystemGit) {
        Use::NotApproved { digest, .. } => digest,
        u => panic!("not NotApproved: {u:?}"),
    }
}

/// Stores the approval of the files on the default branch in the approvals file of `home`.
fn appr(f: &Fx, home: &Path) {
    approve_workspace_in(&approvals(home), &id(&f.c), &digest(f)).unwrap();
}

/// `zygo <args>` in `dir` with the scratch `home`, no session, no workspace, no tmux, no user or system git config and
/// stdin null; then asserts that no marketplace sync ran.
fn zygo(home: &Path, dir: &Path, args: &[&str]) -> Output {
    zygo_env(home, dir, args, &[])
}

/// [`zygo`] with `env` added to the child's environment only (this process's environment is not changed).
fn zygo_env(home: &Path, dir: &Path, args: &[&str], env: &[(&str, &std::ffi::OsStr)]) -> Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_zygo"));
    c.args(args).current_dir(dir);
    common::scratch_home(&mut c, home);
    c.env_remove("ZYGO_SESSION_DIR")
        .env_remove("ZYGO_SESSION")
        .env_remove("ZYGO_WORKSPACE")
        .env_remove("TMUX")
        .env("GIT_CONFIG_GLOBAL", no_config())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdin(Stdio::null());
    for (k, v) in env {
        c.env(k, v);
    }
    let o = c.output().expect("zygo binary");
    common::assert_no_marketplace_sync(home);
    o
}

fn out(o: &Output) -> String {
    String::from_utf8(o.stdout.clone()).unwrap()
}

fn err(o: &Output) -> String {
    String::from_utf8(o.stderr.clone()).unwrap()
}

/// The stderr lines that name the workspace files.
fn def_lines(err: &str) -> Vec<&str> {
    err.lines().filter(|l| l.contains("workspace files")).collect()
}

fn not_approved(d: &str) -> String {
    format!(
        "zygo: this repository's workspace files ({REF}, digest {}) are not approved on this machine; not shown; see zygo workspace approve",
        &d[..12]
    )
}

fn invalid_line() -> String {
    format!("zygo: this repository's workspace files ({REF}) are invalid ({WHY}); not shown")
}

#[test]
fn config_show_names_the_file_of_each_repository_key() { // R§18 item 22
    let f = fx(&[(WS, MIN), (PROF, RUST)]);
    let h = home(&f, "");
    appr(&f, &h);
    let o = zygo(&h, &f.c, &["config", "show"]);
    let (so, se) = (out(&o), err(&o));
    assert_eq!(o.status.code(), Some(0), "{se}");
    assert!(so.lines().any(|l| l == DISABLED), "{so}");
    assert!(so.lines().any(|l| l == REVIEW), "{so}");
    assert!(so.lines().any(|l| l.starts_with("loadout.repository.window = ") && l.ends_with(WS_FILE)), "{so}");
    assert!(def_lines(&se).is_empty(), "{se}");
}

#[test]
fn config_show_layer_repository_shows_no_repository_key() {
    let f = fx(&[(WS, MIN), (PROF, RUST)]);
    let h = home(&f, "");
    appr(&f, &h);
    let o = zygo(&h, &f.c, &["config", "show", "--layer", "repository"]);
    let so = out(&o);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert!(!so.contains("workspace-files") && !so.contains("profile.rust"), "{so}");
}

#[test]
fn config_show_layer_workspace_files_shows_only_repository_keys() {
    let f = fx(&[(WS, MIN), (PROF, RUST)]);
    let h = home(&f, "");
    appr(&f, &h);
    let o = zygo(&h, &f.c, &["config", "show", "--layer", "workspace-files"]);
    let so = out(&o);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert!(so.lines().any(|l| l == DISABLED), "{so}");
    assert!(so.lines().any(|l| l == REVIEW), "{so}");
    assert!(so.lines().all(|l| l.ends_with(PROF_FILE) || l.ends_with(WS_FILE)), "{so}");
}

#[test]
fn config_show_not_approved_shows_no_repository_key_and_one_line() { // R-C2
    let f = fx(&[(WS, MIN), (PROF, RUST)]);
    let h = home(&f, "");
    let d = digest(&f);
    for args in [&["config", "show"][..], &["config", "show", "--layer", "workspace-files"]] {
        let o = zygo(&h, &f.c, args);
        let (so, se) = (out(&o), err(&o));
        assert_eq!(o.status.code(), Some(0), "{args:?}: {se}");
        assert!(!so.contains("workspace-files"), "{args:?}: {so}");
        assert_eq!(def_lines(&se), [not_approved(&d)], "{args:?}");
        if args.len() > 2 {
            assert_eq!(so, "", "{args:?}");
        }
    }
}

#[test]
fn config_show_invalid_shows_no_repository_key_and_one_line() {
    let f = fx(&[(WS, INVALID)]);
    let h = home(&f, "");
    let o = zygo(&h, &f.c, &["config", "show"]);
    let (so, se) = (out(&o), err(&o));
    assert_eq!(o.status.code(), Some(0), "{se}");
    assert!(!so.contains("workspace-files"), "{so}");
    assert_eq!(def_lines(&se), [invalid_line()]);
}

#[test]
fn config_show_and_check_escape_the_violation() { // R-C7; R§18 item 7 last sentence
    let f = fx(&[(WS, "version = 1\n[members.\"a\u{202e}b\"]\n")]);
    let h = home(&f, "");
    let se = err(&zygo(&h, &f.c, &["config", "show"]));
    assert!(se.contains("\\u{202e}") && !se.contains('\u{202e}'), "{se:?}");
    let so = out(&zygo(&h, &f.c, &["config", "check"]));
    assert!(so.contains("\\u{202e}") && !so.contains('\u{202e}'), "{so:?}");
}

#[test]
fn config_check_warns_on_an_invalid_definition_and_keeps_its_exit_status() { // R§10 `config check`
    let f = fx(&[(WS, MIN)]);
    let h = home(&f, "");
    let o = zygo(&h, &f.c, &["config", "check"]);
    let c0 = o.status.code();
    assert!(!out(&o).lines().any(|l| l.contains("workspace files")), "{}", out(&o));
    let f = fx(&[(WS, INVALID)]);
    let h = home(&f, "");
    let o = zygo(&h, &f.c, &["config", "check"]);
    let so = out(&o);
    assert_eq!(o.status.code(), c0, "{so}");
    let lines: Vec<&str> = so.lines().filter(|l| l.contains("workspace files")).collect();
    let warning = format!("warning: workspace files: {WHY}");
    assert_eq!(lines, [warning.as_str()], "{so}");
    assert_eq!(so.lines().last(), Some(warning.as_str()), "{so}");
    let h = home(&f, "[role.x]\nmodel = \"nope\"\n");
    let o = zygo(&h, &f.c, &["config", "check"]);
    assert_eq!(o.status.code(), Some(11), "{}", err(&o));
    assert_eq!(out(&o), format!("error: role.x.model: unknown model\n{warning}\n"));
}

/// A user config that chooses the repository loadout (R§10 `zygo up` row: an explicit choice).
const DEFAULT_REPOSITORY: &str = "[workspace]\ndefault_loadout = \"repository\"\n";
const UNKNOWN_DEFAULT: &str = "error: workspace.default_loadout: unknown loadout";
/// FX-29 author's call: the line of `config check` when it read no workspace files.
const NOT_READ_DEFAULT: &str = "warning: workspace.default_loadout: no workspace files were read here; zygo up uses \"repository\" only from approved files that hold .zygo/workspace.toml";

/// The stdout lines of `config check` that name `workspace.default_loadout`.
fn default_lines(so: &str) -> Vec<&str> {
    so.lines().filter(|l| l.contains("workspace.default_loadout")).collect()
}

/// FX-29, R§10 `zygo config check` row: `default_loadout = "repository"` is accepted exactly where `zygo up` plans it
/// (`up.rs` `choose`): approved files that hold `.zygo/workspace.toml`, or a user `[loadout.repository]`. Files that are
/// not approved, and approved files with profiles only, make `up` refuse: the error and exit 11 stay.
#[test]
fn config_check_default_loadout_repository_follows_zygo_up() {
    let f = fx(&[(WS, MIN)]);
    let h = home(&f, DEFAULT_REPOSITORY);
    let o = zygo(&h, &f.c, &["config", "check"]);
    let so = out(&o);
    assert_eq!(o.status.code(), Some(11), "not approved: {so}{}", err(&o));
    assert_eq!(default_lines(&so), [UNKNOWN_DEFAULT], "not approved: {so}");
    appr(&f, &h);
    let o = zygo(&h, &f.c, &["config", "check"]);
    let so = out(&o);
    assert_eq!(o.status.code(), Some(0), "approved: {so}{}", err(&o));
    assert!(default_lines(&so).is_empty(), "approved: {so}");
    assert_eq!(so, "config ok\n");
    let p = fx(&[(PROF, RUST)]);
    let hp = home(&p, DEFAULT_REPOSITORY);
    appr(&p, &hp);
    let o = zygo(&hp, &p.c, &["config", "check"]);
    let so = out(&o);
    assert_eq!(o.status.code(), Some(11), "profiles only: {so}{}", err(&o));
    assert_eq!(default_lines(&so), [UNKNOWN_DEFAULT], "profiles only: {so}");
    // Control: a user `[loadout.repository]` is the user's loadout; the files do not matter.
    let n = fx(&[(WS, MIN)]);
    let hn = home(&n, &user_repository_table());
    let o = zygo(&hn, &n.c, &["config", "check"]);
    let so = out(&o);
    assert_eq!(o.status.code(), Some(0), "user table: {so}{}", err(&o));
    assert!(default_lines(&so).is_empty(), "user table: {so}");
}

/// [`DEFAULT_REPOSITORY`] with a user `[loadout.repository]` of one shell pane.
fn user_repository_table() -> String {
    format!("{DEFAULT_REPOSITORY}[loadout.repository]\n[[loadout.repository.window]]\n[[loadout.repository.window.pane]]\nkind = \"shell\"\n")
}

/// FX-29 review F2: the other states of the read. Invalid files, no files, and files with profiles only that are not
/// approved make `zygo up` refuse, so `config check` keeps the error and exit 11. A user `[loadout.repository]` with
/// approved files is the user's loadout: `config ok`, exit 0.
#[test]
fn config_check_default_loadout_repository_refuses_where_zygo_up_refuses() {
    let f = fx(&[(WS, INVALID)]);
    let h = home(&f, DEFAULT_REPOSITORY);
    let o = zygo(&h, &f.c, &["config", "check"]);
    assert_eq!(o.status.code(), Some(11), "invalid: {}{}", out(&o), err(&o));
    assert_eq!(out(&o), format!("{UNKNOWN_DEFAULT}\nwarning: workspace files: {WHY}\n"), "invalid");
    let a = fx(&[]);
    let ha = home(&a, DEFAULT_REPOSITORY);
    let o = zygo(&ha, &a.c, &["config", "check"]);
    assert_eq!(o.status.code(), Some(11), "absent: {}{}", out(&o), err(&o));
    assert_eq!(out(&o), format!("{UNKNOWN_DEFAULT}\n"), "absent");
    let p = fx(&[(PROF, RUST)]);
    let hp = home(&p, DEFAULT_REPOSITORY);
    let o = zygo(&hp, &p.c, &["config", "check"]);
    assert_eq!(o.status.code(), Some(11), "profiles only, not approved: {}{}", out(&o), err(&o));
    assert_eq!(out(&o), format!("{UNKNOWN_DEFAULT}\n"), "profiles only, not approved");
    let u = fx(&[(WS, MIN)]);
    let hu = home(&u, &user_repository_table());
    appr(&u, &hu);
    let o = zygo(&hu, &u.c, &["config", "check"]);
    assert_eq!(o.status.code(), Some(0), "user table, approved: {}{}", out(&o), err(&o));
    assert_eq!(out(&o), "config ok\n", "user table, approved");
}

/// FX-29 author's call: with no read (an unread read; outside a checkout) `config check` cannot see the files that
/// `zygo up` will find, so `default_loadout = "repository"` is one warning and the exit status stays that of the config.
#[test]
fn config_check_default_loadout_repository_warns_when_no_files_are_read() {
    let f = unread_fx();
    let h = home(&f, DEFAULT_REPOSITORY);
    let o = zygo(&h, &f.c, &["config", "check"]);
    assert_eq!(o.status.code(), Some(0), "unread: {}{}", out(&o), err(&o));
    assert_eq!(out(&o), format!("{NOT_READ_DEFAULT}\n"));
    assert_eq!(def_lines(&err(&o)), [unread_line(REFUSED)]);
    // Review F5 of WD-16: git looks for no repository above the temp folder, wherever `TMPDIR` is (child only).
    let env = [("GIT_CEILING_DIRECTORIES", f.t.path().as_os_str())];
    let o = zygo_env(&h, &h, &["config", "check"], &env);
    assert_eq!(o.status.code(), Some(0), "outside: {}{}", out(&o), err(&o));
    assert_eq!(out(&o), format!("{NOT_READ_DEFAULT}\n"));
    assert!(def_lines(&err(&o)).is_empty(), "outside: {}", err(&o));
}

#[test]
fn profile_list_shows_an_approved_repository_profile() { // R§18 item 9
    let f = fx(&[(WS, MIN), (PROF, RUST)]);
    let h = home(&f, "");
    appr(&f, &h);
    let o = zygo(&h, &f.c, &["profile", "list"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(out(&o), "  rust (workspace-files)\n");
    let h = home(&f, "[user]\ndefault_profile = \"rust\"\n");
    let o = zygo(&h, &f.c, &["profile", "list"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(out(&o), "* rust (workspace-files)\n");
}

#[test]
fn profile_list_marks_a_shadowed_repository_profile() { // R§18 item 9
    let f = fx(&[(WS, MIN), (PROF, RUST)]);
    let h = home(&f, "[profile.rust]\nprompt = \"R\"\n");
    appr(&f, &h);
    let o = zygo(&h, &f.c, &["profile", "list"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(out(&o), "  rust (user)\n  rust (workspace-files, shadowed)\n");
}

#[test]
fn profile_list_lists_nothing_from_unapproved_or_invalid_files() { // R-C2
    let f = fx(&[(WS, MIN), (PROF, RUST)]);
    let h = home(&f, "");
    let o = zygo(&h, &f.c, &["profile", "list"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(out(&o), "no profiles\n");
    assert_eq!(def_lines(&err(&o)), [not_approved(&digest(&f))]);
    let f = fx(&[(WS, INVALID)]);
    let h = home(&f, "");
    let o = zygo(&h, &f.c, &["profile", "list"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(out(&o), "no profiles\n");
    assert_eq!(def_lines(&err(&o)), [invalid_line()]);
}

#[test]
fn profile_show_prints_an_approved_repository_profile() {
    let f = fx(&[(WS, MIN), (PROF, RUST)]);
    let h = home(&f, "");
    let o = zygo(&h, &f.c, &["profile", "show", "rust"]);
    let se = err(&o);
    assert_eq!(o.status.code(), Some(1), "{se}");
    assert_eq!(out(&o), "");
    assert!(se.contains("profile rust: not found"), "{se}");
    assert!(se.contains(&not_approved(&digest(&f))), "{se}");
    appr(&f, &h);
    let o = zygo(&h, &f.c, &["profile", "show", "rust"]);
    let so = out(&o);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(so.lines().next(), Some("profile rust (workspace-files)"), "{so}");
    assert!(so.lines().any(|l| l == "disabled = [\"design\"]"), "{so}");
    assert!(so.lines().any(|l| l == "review = \"gpt-6-sol\""), "{so}");
}

#[test]
fn outside_a_repository_reads_nothing() { // plan call 7
    let f = fx(&[(WS, MIN), (PROF, RUST)]);
    let h = home(&f, "");
    // Review F5: git looks for no repository above the temp folder, wherever `TMPDIR` is (child only).
    let env = [("GIT_CEILING_DIRECTORIES", f.t.path().as_os_str())];
    for args in [&["config", "show"][..], &["config", "check"], &["profile", "list"]] {
        let o = zygo_env(&h, &h, args, &env);
        let se = err(&o);
        assert_eq!(o.status.code(), Some(0), "{args:?}: {se}");
        assert!(def_lines(&se).is_empty(), "{args:?}: {se}");
    }
}

/// The line of an unread read whose reason is `why`: a checkout was found and its read failed.
fn unread_line(why: &str) -> String {
    format!("zygo: this repository's workspace files were not read ({why}); not shown")
}

/// The line of a lookup that failed before zygo knew whether the folder is a repository (`find_checkout` exit 1, FX-25
/// close): it names no repository.
#[cfg(unix)]
fn lookup_failed_line(why: &str) -> String {
    format!("zygo: workspace files were not read ({why}); not shown")
}

/// `origin/HEAD` set as a direct ref: the read refuses it, so the read is unread (R§6.1 step 2).
fn unread_fx() -> Fx {
    let f = fx(&[(WS, MIN), (PROF, RUST)]);
    let oid = git(&f.c, &["rev-parse", "refs/remotes/origin/main"]);
    git(&f.c, &["update-ref", "--no-deref", "refs/remotes/origin/HEAD", &oid]);
    f
}

const REFUSED: &str = "origin/HEAD points outside refs/remotes/origin/";

#[test]
fn an_unread_read_is_one_line_and_shows_nothing() { // R§10 rows; review F5
    let f = unread_fx();
    let h = home(&f, "");
    for args in [&["config", "show"][..], &["config", "show", "--layer", "workspace-files"], &["profile", "list"]] {
        let o = zygo(&h, &f.c, args);
        let (so, se) = (out(&o), err(&o));
        assert_eq!(o.status.code(), Some(0), "{args:?}: {se}");
        assert_eq!(def_lines(&se), [unread_line(REFUSED)], "{args:?}");
        assert!(!so.contains("workspace-files"), "{args:?}: {so}");
    }
    let o = zygo(&h, &f.c, &["config", "check"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(out(&o), "config ok\n");
    assert_eq!(def_lines(&err(&o)), [unread_line(REFUSED)]);
    let o = zygo(&h, &f.c, &["profile", "show", "rust"]);
    let se = err(&o);
    assert_eq!(o.status.code(), Some(1), "{se}");
    assert!(se.contains("profile rust: not found"), "{se}");
    assert_eq!(def_lines(&se), [unread_line(REFUSED)]);
}

#[test]
fn profile_list_marks_a_repository_profile_that_a_team_preset_shadows() { // R§18 item 9; review F5
    let f = fx(&[(WS, MIN), (PROF, RUST)]);
    let h = home(&f, "");
    appr(&f, &h);
    let sd = f.t.path().join("session");
    std::fs::create_dir_all(&sd).unwrap();
    std::fs::write(sd.join("team.toml"), "[profile.rust]\ndisabled = [\"review\"]\n").unwrap();
    let env = [("ZYGO_SESSION_DIR", sd.as_os_str())];
    let o = zygo_env(&h, &f.c, &["profile", "list"], &env);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(out(&o), "  rust (team)\n  rust (workspace-files, shadowed)\n");
    let o = zygo_env(&h, &f.c, &["profile", "show", "rust"], &env);
    let so = out(&o);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(so.lines().next(), Some("profile rust (team)"), "{so}");
    assert!(so.lines().any(|l| l == "disabled = [\"review\"]"), "{so}");
    assert!(!so.contains("gpt-6-sol"), "{so}");
}

/// `zygo profile <args>` with the user config `cfg`, in a fixture repository with no `.zygo/` (review F5: the
/// `list` and `show` tests that were `profile.rs` unit tests, where the read took the test process's folder).
fn profile(cfg: &str, args: &[&str]) -> Output {
    let f = fx(&[]);
    let h = home(&f, cfg);
    zygo(&h, &f.c, &[&["profile"][..], args].concat())
}

const PROFILES: &str = "[user]\ndefault_profile = \"a\"\n[profile.a]\nprompt = \"A\"\n[profile.b]\nprompt = \"B\"\n";
const ESC_PROFILES: &str =
    "[user]\ndefault_profile = \"x\\u001b[2K\"\n[profile.\"a.b\"]\nprompt = \"B\"\n[profile.\"x\\u001b[2K\\n* forged\"]\nprompt = \"A\"\n";

#[test]
fn profile_list_marks_the_default_and_the_layer() {
    assert_eq!(out(&profile(PROFILES, &["list"])), "* a (user)\n  b (user)\n");
    assert_eq!(out(&profile("[user]\n", &["list"])), "no profiles\n");
    // The verb word, not `use list`.
    assert_eq!(out(&profile("[profile.list]\nprompt = \"L\"\n", &["list"])), "  list (user)\n");
}

#[test]
fn profile_show_prints_the_toml_and_the_layer() {
    let o = profile(PROFILES, &["show", "b"]);
    let so = out(&o);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert!(so.starts_with("profile b (user)\n") && so.contains("prompt = \"B\""), "{so}");
    let o = profile(PROFILES, &["show", "zz"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("profile zz: not found"), "{}", err(&o));
}

#[test]
fn profile_list_and_show_escape_config_text() { // §11.3
    assert_eq!(out(&profile(ESC_PROFILES, &["list"])), "  a.b (user)\n  x\\u{1b}[2K\\n* forged (user)\n");
    let so = out(&profile(ESC_PROFILES, &["show", "x\u{1b}[2K\n* forged"]));
    assert!(so.starts_with("profile x\\u{1b}[2K\\n* forged (user)\n"), "{so:?}");
    assert_eq!(out(&profile(ESC_PROFILES, &["show", "a.b"])).lines().next(), Some("profile a.b (user)"));
    let se = err(&profile(ESC_PROFILES, &["show", "z\u{1b}"]));
    assert!(se.contains("profile z\\u{1b}: not found") && !se.contains('\u{1b}'), "{se:?}");
    // Round 2 F1: the TOML body is escaped line by line; its line breaks stay.
    let cfg = "[profile.a]\nprompt = \"x\\u202e\"\n[profile.a.plugins]\n\"m:p\\u202e\\u001b[2K\" = true\n";
    let o = out(&profile(cfg, &["show", "a"]));
    assert!(!o.contains('\u{202e}') && !o.contains('\u{1b}'), "{o:?}");
    assert!(o.lines().any(|l| l == "prompt = \"x\\u{202e}\""), "{o:?}");
    assert!(o.contains("\"m:p\\u{202e}\\u001B[2K\" = true"), "{o:?}");
    assert!(o.starts_with("profile a (user)\n") && o.lines().count() > 3, "{o:?}");
}

/// OP5: `zygo loadout` exits 2 on Windows before any read. The stub `git` tests are here too: the stub is a `sh` script.
#[cfg(unix)]
mod unix {
    use super::*;
    use std::ffi::OsString;

    const USER_REPOSITORY: &str = "[[loadout.repository.window]]\n[[loadout.repository.window.pane]]\nkind = \"claude-code\"\n";

    /// `<f.t>/stub/git`: a `sh` script that runs `body` (`$@`: the git arguments), then the real `git` with the same
    /// arguments. Returns the `PATH` with that folder first, for a child's environment only.
    fn stub_git(f: &Fx, body: &str) -> OsString {
        use std::os::unix::fs::PermissionsExt;
        let path = std::env::var_os("PATH").expect("PATH");
        let real = std::env::split_paths(&path).map(|d| d.join("git")).find(|p| p.is_file()).expect("git on PATH");
        let dir = f.t.path().join("stub");
        std::fs::create_dir_all(&dir).unwrap();
        let script = format!("#!/bin/sh\n{body}\nexec '{}' \"$@\"\n", real.display());
        std::fs::write(dir.join("git"), script).unwrap();
        std::fs::set_permissions(dir.join("git"), std::fs::Permissions::from_mode(0o755)).unwrap();
        std::env::join_paths(std::iter::once(dir).chain(std::env::split_paths(&path))).unwrap()
    }

    /// The `git` subcommands of a definition read that no other lookup of these verbs runs (`read_default`).
    const READ_CALLS: [&str; 4] = ["symbolic-ref", "show-ref", "ls-tree", "cat-file"];

    #[test]
    fn config_show_layer_runs_no_read() { // R§10 `config show --layer <layer>`; review F5
        let f = fx(&[(WS, MIN), (PROF, RUST)]);
        let h = home(&f, "");
        appr(&f, &h);
        let path = stub_git(&f, "for a in \"$@\"; do case \"$a\" in ls-tree|show) echo stub >&2; exit 1;; esac; done");
        let env = [("PATH", path.as_os_str())];
        // Control: a read through the stub fails, and `config show` prints the unread line.
        let se = err(&zygo_env(&h, &f.c, &["config", "show"], &env));
        assert_eq!(def_lines(&se), [unread_line("git ls-tree: failed")], "{se}");
        for layer in ["builtin", "team", "user", "repository", "profile", "flags", "session"] {
            let o = zygo_env(&h, &f.c, &["config", "show", "--layer", layer], &env);
            let se = err(&o);
            assert_eq!(o.status.code(), Some(0), "{layer}: {se}");
            assert!(def_lines(&se).is_empty(), "{layer}: {se}");
            assert!(!out(&o).contains("workspace-files"), "{layer}");
        }
    }

    #[test]
    fn loadout_show_repository_with_a_user_loadout_runs_no_read() { // R§10 `loadout show repository`; review F5
        let f = fx(&[(WS, MIN)]);
        let log = f.t.path().join("git.log");
        let path = stub_git(&f, &format!("echo \"$*\" >> '{}'", log.display()));
        let env = [("PATH", path.as_os_str())];
        let read_calls = || -> Vec<String> {
            let text = std::fs::read_to_string(&log).unwrap_or_default();
            std::fs::write(&log, "").unwrap();
            text.lines().filter(|l| l.split(' ').any(|a| READ_CALLS.contains(&a))).map(str::to_owned).collect()
        };
        // Control: with no user `[loadout.repository]`, the stub logs the read.
        let h = home(&f, "");
        appr(&f, &h);
        let o = zygo_env(&h, &f.c, &["loadout", "show", "repository"], &env);
        assert_eq!(o.status.code(), Some(0), "{}", err(&o));
        assert!(!read_calls().is_empty());
        let h = home(&f, USER_REPOSITORY);
        let o = zygo_env(&h, &f.c, &["loadout", "show", "repository"], &env);
        assert_eq!(o.status.code(), Some(0), "{}", err(&o));
        assert!(out(&o).lines().any(|l| l == "kind = \"claude-code\""), "{}", out(&o));
        assert_eq!(read_calls(), Vec::<String>::new());
    }

    #[test]
    fn loadout_list_and_show_print_the_unread_line() { // R§10 `loadout` row: unread; review F5
        let f = unread_fx();
        let h = home(&f, "");
        let o = zygo(&h, &f.c, &["loadout", "list"]);
        assert_eq!(o.status.code(), Some(0), "{}", err(&o));
        assert_eq!(out(&o), "solo  built-in\n");
        assert_eq!(err(&o), format!("{}\n", unread_line(REFUSED)));
        let o = zygo(&h, &f.c, &["loadout", "show", "repository"]);
        assert_eq!(o.status.code(), Some(1));
        assert_eq!(out(&o), "");
        assert_eq!(err(&o), format!("{}\n", unread_line(REFUSED)));
    }

    #[test]
    fn a_failed_worktree_list_is_the_unread_line_with_its_reason() { // WD-16 review F1 binary form; FX-25 f6, close
        let f = fx(&[(WS, MIN), (PROF, RUST)]);
        let h = home(&f, "");
        appr(&f, &h);
        // A git that fails only `worktree list`, on the child's `PATH` only.
        let path = stub_git(&f, "case \" $* \" in *\" worktree list \"*) echo 'fatal: stub' >&2; exit 1;; esac");
        let env = [("PATH", path.as_os_str())];
        // A lookup failure (`find_checkout` exit 1): the line names no repository.
        let line = lookup_failed_line("git worktree list failed: fatal: stub");
        for args in [&["config", "show"][..], &["profile", "list"]] {
            let o = zygo_env(&h, &f.c, args, &env);
            let (so, se) = (out(&o), err(&o));
            assert_eq!(o.status.code(), Some(0), "{args:?}: {se}");
            assert_eq!(def_lines(&se), [line.as_str()], "{args:?}");
            assert!(!so.contains("workspace-files"), "{args:?}: {so}");
        }
        let o = zygo_env(&h, &f.c, &["loadout", "show", "repository"], &env);
        assert_eq!(o.status.code(), Some(1), "{}", err(&o));
        assert_eq!(out(&o), "");
        assert_eq!(err(&o), format!("{line}\n"));
        // Control: with the real git, the approved files apply.
        let o = zygo(&h, &f.c, &["loadout", "show", "repository"]);
        assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    }

    /// `zygo loadout list` in `f.c` with the user config `user`, approved or not: (stdout, stderr), exit 0 asserted.
    fn list(f: &Fx, user: &str, approve: bool) -> (String, String) {
        let h = home(f, user);
        if approve {
            appr(f, &h);
        }
        let o = zygo(&h, &f.c, &["loadout", "list"]);
        assert_eq!(o.status.code(), Some(0), "{}", err(&o));
        (out(&o), err(&o))
    }

    #[test]
    fn loadout_list_labels_the_repository_loadout_in_each_state() { // R§10 row as rewritten 2026-11-26
        let cases: [(Fx, &str, bool, &str); 7] = [
            (fx(&[(WS, MIN)]), "", true, "repository  workspace-files\nsolo  built-in\n"),
            (fx(&[(WS, MIN)]), "", false, "repository  workspace-files, not approved\nsolo  built-in\n"),
            (fx(&[(WS, INVALID)]), "", false, "repository  workspace-files, invalid\nsolo  built-in\n"),
            (
                fx(&[(WS, MIN)]),
                USER_REPOSITORY,
                true,
                "repository  user\nrepository  workspace-files, shadowed\nsolo  built-in\n",
            ),
            (fx(&[(PROF, RUST)]), "", false, "solo  built-in\n"),
            // Review F4: an invalid read with no `.zygo/workspace.toml` gives no row (the reviewer's probe); with one,
            // an invalid profile still gives the row.
            (fx(&[(PROF, "prompt = \"x\"\n")]), "", false, "solo  built-in\n"),
            (fx(&[(WS, MIN), (PROF, "prompt = \"x\"\n")]), "", false, "repository  workspace-files, invalid\nsolo  built-in\n"),
        ];
        for (f, user, approve, want) in &cases {
            let (so, se) = list(f, user, *approve);
            assert_eq!(&so, want, "{user} {approve}");
            assert!(def_lines(&se).is_empty(), "{se}");
        }
    }

    #[test]
    fn loadout_list_shows_an_invalid_row_for_a_listing_above_a_limit() { // R§10 `zygo loadout list` row (FX-39 R5); R§6.1 step 4
        // `.zygo/` with workspace.toml and 256 more entries: 257 in all, above the 256-entry listing limit.
        let mut ents = vec![(WS.to_string(), MIN.to_string())];
        ents.extend((0..256).map(|i| (format!(".zygo/x{i:03}"), "x".to_string())));
        let ents: Vec<(&str, &str)> = ents.iter().map(|(p, t)| (p.as_str(), t.as_str())).collect();
        let f = fx(&ents);
        let (so, se) = list(&f, "", false);
        assert_eq!(so, "repository  workspace-files, invalid\nsolo  built-in\n");
        assert!(def_lines(&se).is_empty(), "{se}");
    }

    #[test]
    fn loadout_show_repository_prints_only_when_approved() { // R-C2
        let f = fx(&[(WS, MIN)]);
        let h = home(&f, "");
        let o = zygo(&h, &f.c, &["loadout", "show", "repository"]);
        assert_eq!(o.status.code(), Some(1));
        assert_eq!(out(&o), "");
        assert_eq!(err(&o), format!("{}\n", not_approved(&digest(&f))));
        let h = home(&f, USER_REPOSITORY);
        let o = zygo(&h, &f.c, &["loadout", "show", "repository"]);
        assert_eq!(o.status.code(), Some(0), "{}", err(&o));
        assert!(out(&o).lines().any(|l| l == "kind = \"claude-code\""), "{}", out(&o));
        assert_eq!(err(&o), "");
        let h = home(&f, "");
        appr(&f, &h);
        let o = zygo(&h, &f.c, &["loadout", "show", "repository"]);
        let so = out(&o);
        assert_eq!(o.status.code(), Some(0), "{}", err(&o));
        for l in [
            "[[loadout.repository.window]]",
            "layout = \"main-vertical\"",
            "name = \"orchestrator\"",
            "job = \"orchestrator\"",
            "name = \"worker-4\"",
            "pool = true",
        ] {
            assert!(so.lines().any(|x| x == l), "{l}: {so}");
        }
        let f = fx(&[(WS, INVALID)]);
        let h = home(&f, "");
        let o = zygo(&h, &f.c, &["loadout", "show", "repository"]);
        assert_eq!(o.status.code(), Some(1));
        assert_eq!(err(&o), format!("{}\n", invalid_line()));
        let f = fx(&[]);
        let h = home(&f, "");
        let o = zygo(&h, &f.c, &["loadout", "show", "repository"]);
        assert_eq!(o.status.code(), Some(1));
        assert_eq!(err(&o), "zygo: unknown loadout `repository`\n");
    }
}
