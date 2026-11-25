//! WD-17: `zygo doctor` lines for the workspace files, the pool and the member maximum (R§10 `zygo doctor`, R§9.5,
//! R-C6; R§18 items 13 and 23).
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};
use zygo::cmd::doctor::definition_lines;
use zygo_config::repo_def::{approve_workspace_in, check_default, GitRunner, SystemGit, Use};
use zygo_config::{load, LoadContext, Merged};
use zygo_core::paths::Paths;
use zygo_work::repo::checkout;
use zygo_workspace::identity::workspace_id;

const MIN: &str = "version = 1\n";
const WS: &str = ".zygo/workspace.toml";
const INVALID: &str = "version = 1\n[members.a]\nargs = [\"x\"]\n";
const INVALID_LINE: &str =
    "workspace files: invalid (.zygo/workspace.toml: members.a.args: not allowed in a repository file)";

/// `git -C <dir> <args>` with no user or system config, no signing, a fixed identity, `core.autocrlf=false` and
/// `core.protectNTFS=false`.
fn git_cmd(dir: &Path, args: &[&str]) -> Command {
    let empty = if cfg!(windows) { "NUL" } else { "/dev/null" };
    let mut c = Command::new("git");
    c.env("GIT_CONFIG_GLOBAL", empty).env("GIT_CONFIG_NOSYSTEM", "1")
        .args(["-c", "commit.gpgsign=false", "-c", "user.name=t", "-c", "user.email=t@example.invalid",
               "-c", "core.autocrlf=false", "-c", "core.protectNTFS=false", "-C"]).arg(dir).args(args);
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
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().expect("git on PATH");
    child.stdin.take().unwrap().write_all(bytes).unwrap();
    let o = child.wait_with_output().unwrap();
    assert!(o.status.success(), "hash-object: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8(o.stdout).unwrap().trim().to_string()
}

/// Adds each regular file to the index of `dir` (nothing on disk), then `commit -q -m c`.
fn commit_files(dir: &Path, ents: &[(&str, &[u8])]) {
    for (path, bytes) in ents {
        let oid = blob(dir, bytes);
        git(dir, &["update-index", "--add", "--cacheinfo", &format!("100644,{oid},{path}")]);
    }
    git(dir, &["commit", "-q", "-m", "c"]);
}

struct Fx { _t: tempfile::TempDir, c: PathBuf }

impl Fx {
    fn root(&self) -> &Path { self._t.path() }
    fn approvals(&self) -> PathBuf { self._t.path().join("approvals.toml") }
}

/// `o.git` (bare), `w` with a `README` seed commit and then `ents` (not empty), pushed; `c` cloned `--no-checkout`.
fn fx(ents: &[(&str, &[u8])]) -> Fx {
    let t = tempfile::tempdir().unwrap();
    let (o, w) = (t.path().join("o.git"), t.path().join("w"));
    git(t.path(), &["init", "-q", "--bare", "-b", "main", o.to_str().unwrap()]);
    git(t.path(), &["init", "-q", "-b", "main", w.to_str().unwrap()]);
    commit_files(&w, &[("README", b"seed")]);
    commit_files(&w, ents);
    git(&w, &["remote", "add", "origin", "../o.git"]);
    git(&w, &["push", "-q", "origin", "main"]);
    git(t.path(), &["clone", "-q", "--no-checkout", "o.git", "c"]);
    let c = t.path().join("c");
    Fx { _t: t, c }
}

fn paths(root: &Path) -> Paths { Paths { config_file: root.join("config.toml"), cache_dir: root.join("cache"),
    data_dir: root.join("data"), sessions_dir: root.join("cache/sessions"), marketplaces_dir: root.join("cache/marketplaces"),
    plugin_store: root.join("data/plugins"), approvals_file: root.join("data/approvals.toml"), usage_db: root.join("data/usage.db") } }

/// Writes `root/config.toml` and loads it as the user layer.
fn merged(root: &Path, user_toml: &str) -> Merged {
    std::fs::write(root.join("config.toml"), user_toml).unwrap();
    load(&LoadContext { paths: paths(root), repo: None, team: None }).unwrap()
}

/// `root/pool` with the directories `a` and `b`, a file `c` and a hidden directory `.x`.
fn pool(root: &Path) -> PathBuf {
    let p = root.join("pool");
    for d in ["a", "b", ".x"] {
        std::fs::create_dir_all(p.join(d)).unwrap();
    }
    std::fs::write(p.join("c"), b"c").unwrap();
    p
}

/// A TOML literal string, so a Windows path needs no escape.
fn pool_toml(p: &Path) -> String { format!("[workspace]\npool = '{}'\n", p.display()) }

fn id(c: &Path) -> String { workspace_id(&checkout(c, Instant::now() + Duration::from_secs(60)).unwrap().common_dir) }

fn head(c: &Path) -> String { git(c, &["rev-parse", "refs/remotes/origin/main"]) }

/// The digest of the files on the default branch, not stored.
fn digest(f: &Fx) -> String {
    match check_default(&f.c, &id(&f.c), &f.approvals(), &SystemGit) {
        Use::NotApproved { digest, .. } => digest,
        other => panic!("not NotApproved: {other:?}"),
    }
}

/// Stores the approval of the files on the default branch; returns their digest.
fn approve(f: &Fx) -> String {
    let d = digest(f);
    approve_workspace_in(&f.approvals(), &id(&f.c), &d).unwrap();
    d
}

fn appr(f: &Fx, d: &str) -> String {
    format!("workspace files: refs/remotes/origin/main {} digest {}: approved", &head(&f.c)[..12], &d[..12])
}

fn lines(f: &Fx, m: &Merged) -> Vec<String> { definition_lines(&f.c, m, &f.approvals(), &SystemGit) }

/// `ls-tree` fails as when the tool is gone; every other command runs.
struct FailLsTree;

impl GitRunner for FailLsTree {
    fn git(&self, top: &Path, args: &[&str], deadline: Instant) -> Result<Output, String> {
        if args.first() == Some(&"ls-tree") {
            return Err("tool not found".into());
        }
        SystemGit.git(top, args, deadline)
    }
}

#[test]
fn approved_line() {
    let f = fx(&[(WS, MIN.as_bytes())]);
    let d = approve(&f);
    let m = merged(f.root(), "");
    assert_eq!(lines(&f, &m), vec![appr(&f, &d)]);
}

#[test]
fn not_approved_line_and_no_shared_lines() {
    let f = fx(&[(WS, MIN.as_bytes())]);
    let p = pool(f.root());
    let d = digest(&f);
    let m = merged(f.root(), &pool_toml(&p));
    assert_eq!(lines(&f, &m), vec![
        format!("workspace files: refs/remotes/origin/main {} digest {}: not approved", &head(&f.c)[..12], &d[..12]),
        // `up` would plan `solo`: no shared line.
        format!("pool: {}: 2 directories", p.display()),
    ]);
}

#[test]
fn invalid_line() {
    let f = fx(&[(WS, INVALID.as_bytes())]);
    let m = merged(f.root(), "");
    assert_eq!(lines(&f, &m), vec![INVALID_LINE.to_string()]);
}

#[test]
fn no_files_line() {
    let f = fx(&[("src/x", b"x")]);
    let m = merged(f.root(), "");
    assert_eq!(lines(&f, &m), vec!["workspace files: none".to_string()]);
}

/// R§18 item 23.
#[test]
fn unread_when_ls_tree_fails() {
    let f = fx(&[(WS, MIN.as_bytes())]);
    let m = merged(f.root(), "");
    assert_eq!(definition_lines(&f.c, &m, &f.approvals(), &FailLsTree),
               vec!["workspace files: unread (git ls-tree: tool not found)".to_string()]);
}

#[test]
fn pool_lines_outside_a_repository() {
    let t = tempfile::tempdir().unwrap();
    let (root, cwd) = (t.path().join("root"), t.path().join("cwd"));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&cwd).unwrap();
    let approvals = root.join("approvals.toml");
    let p = pool(&root);
    let run = |toml: &str| definition_lines(&cwd, &merged(&root, toml), &approvals, &SystemGit);
    assert_eq!(run(&pool_toml(&p)), vec![format!("pool: {}: 2 directories", p.display())]);
    let gone = root.join("gone");
    assert_eq!(run(&pool_toml(&gone)), vec![format!("pool: {}: not a directory", gone.display())]);
    assert_eq!(run("[workspace]\npool = \"rel/x\"\n"), vec!["pool: rel/x: must be absolute after ~ expansion".to_string()]);
    assert_eq!(run(""), Vec::<String>::new());
}

/// R§18 item 13.
#[test]
fn shared_lines_of_the_default_set() {
    let f = fx(&[(WS, MIN.as_bytes())]);
    let p = pool(f.root());
    let d = approve(&f);
    let m = merged(f.root(), &pool_toml(&p));
    assert_eq!(lines(&f, &m), vec![
        appr(&f, &d),
        format!("pool: {}: 2 directories", p.display()),
        format!("pool: {} is shared by orchestrator, worker-2, worker-4", p.join("a").display()),
        format!("pool: {} is shared by worker-1, worker-3", p.join("b").display()),
    ]);
    // A user `default_loadout` wins over the repository loadout.
    let m = merged(f.root(), &(pool_toml(&p) + "default_loadout = \"solo\"\n"));
    assert_eq!(lines(&f, &m), vec![appr(&f, &d), format!("pool: {}: 2 directories", p.display())]);
}

#[test]
fn member_maximum_line() {
    const MAX: &str = "workspace files: 5 agents; [limits] max_members is 4";
    let f = fx(&[(WS, MIN.as_bytes())]);
    let four = "[limits]\nmax_members = 4\n";
    // Not approved: the operator learns it before approving.
    assert!(lines(&f, &merged(f.root(), four)).contains(&MAX.to_string()));
    let d = approve(&f);
    assert_eq!(lines(&f, &merged(f.root(), four)), vec![appr(&f, &d), MAX.to_string()]);
    assert!(!lines(&f, &merged(f.root(), "[limits]\nmax_members = 5\n")).contains(&MAX.to_string()));
    // A user `[loadout.repository]` is used in place of the repository one.
    let user = format!("{four}[[loadout.repository.window]]\n[[loadout.repository.window.pane]]\nkind = \"claude-code\"\n");
    assert!(!lines(&f, &merged(f.root(), &user)).iter().any(|l| l.starts_with("workspace files: 5 agents")));
}

/// The binary probes `claude` on PATH; PATH is git's exec-path, which holds git and no agent.
#[cfg(unix)]
#[test]
fn doctor_exits_0_with_invalid_files_and_a_broken_pool() {
    let f = fx(&[(WS, INVALID.as_bytes())]);
    let root = f.root().join("home");
    std::fs::create_dir_all(&root).unwrap();
    let gone = root.join("gone");
    std::fs::write(root.join("config.toml"), pool_toml(&gone)).unwrap();
    let exec_path = git(f.root(), &["--exec-path"]);
    let o = Command::new(env!("CARGO_BIN_EXE_zygo"))
        .arg("doctor")
        .current_dir(&f.c)
        .env("HOME", &root)
        .env("LOCALAPPDATA", &root)
        .env("APPDATA", &root)
        .env("ZYGO_CONFIG", root.join("config.toml"))
        .env("PATH", exec_path)
        .env_remove("ZYGO_SESSION")
        .env_remove("ZYGO_SESSION_DIR")
        .env_remove("CLAUDE_CONFIG_DIR")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&o.stdout);
    assert_eq!(o.status.code(), Some(0), "stdout: {stdout}\nstderr: {}", String::from_utf8_lossy(&o.stderr));
    assert!(stdout.lines().any(|l| l == INVALID_LINE), "{stdout}");
    let pool_line = format!("pool: {}: not a directory", gone.display());
    assert!(stdout.lines().any(|l| l == pool_line), "{stdout}");
}
