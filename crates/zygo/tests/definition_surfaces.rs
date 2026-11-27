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
    for args in [&["config", "show"][..], &["config", "check"], &["profile", "list"]] {
        let o = zygo(&h, &h, args);
        let se = err(&o);
        assert_eq!(o.status.code(), Some(0), "{args:?}: {se}");
        assert!(def_lines(&se).is_empty(), "{args:?}: {se}");
    }
}

/// OP5: `zygo loadout` exits 2 on Windows before any read.
#[cfg(unix)]
mod unix {
    use super::*;

    const USER_REPOSITORY: &str = "[[loadout.repository.window]]\n[[loadout.repository.window.pane]]\nkind = \"claude-code\"\n";

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
