//! MS-10 test harness: one test `HOME` per process, local bare remotes and their clones. No helper waits on
//! wall-clock time.
#![allow(dead_code)]

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tempfile::TempDir;
use zygo_core::paths::Paths;
use zygo_mail_sync::state::SyncPaths;

/// The workspace id of every test machine.
pub const WS_ID: &str = "0123456789abcdef";

/// The git variables that would make a child git read another repository, another config or another author.
const CLEARED: &[&str] = &[
    "GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_COMMON_DIR", "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES", "GIT_CONFIG", "GIT_CONFIG_GLOBAL", "GIT_CONFIG_SYSTEM", "GIT_CONFIG_NOSYSTEM",
    "GIT_CONFIG_COUNT", "GIT_CONFIG_PARAMETERS", "GIT_AUTHOR_NAME", "GIT_AUTHOR_EMAIL", "GIT_COMMITTER_NAME",
    "GIT_COMMITTER_EMAIL",
];

/// A deadline one hour away: a test not about time never times out under machine load.
pub fn far() -> Instant { Instant::now() + Duration::from_secs(3600) }

/// The test `HOME` of this process: its `.gitconfig` turns on `commit.gpgSign` and `push.gpgSign` with a spy gpg
/// program that records each call and fails, and sets `push.default = matching` (a push with no refspec connects). The first call sets `HOME`, `XDG_CONFIG_HOME` and `GIT_SSH_COMMAND` (a
/// spy ssh that records each call and fails; the builder keeps a set `GIT_SSH_COMMAND`) and removes the git author
/// and config variables from the process environment, so the code under test (which starts git with the process
/// environment) reads only this config.
pub fn home() -> &'static Path {
    static HOME: OnceLock<TempDir> = OnceLock::new();
    HOME.get_or_init(|| {
        let t = tempfile::Builder::new().prefix("home").tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
        let h = t.path().canonicalize().unwrap();
        let spy = h.join("gpg-spy");
        fs::write(&spy, format!("#!/bin/sh\necho ran >> '{}'\nexit 1\n", h.join("gpg-ran").display())).unwrap();
        fs::set_permissions(&spy, fs::Permissions::from_mode(0o755)).unwrap();
        fs::write(h.join(".gitconfig"), format!(
            "[commit]\n\tgpgSign = true\n[push]\n\tgpgSign = true\n\tdefault = matching\n[gpg]\n\tprogram = {}\n[init]\n\tdefaultBranch = main\n",
            spy.display(),
        )).unwrap();
        let ssh = h.join("ssh-spy");
        fs::write(&ssh, format!("#!/bin/sh\necho ran >> '{}'\nexit 1\n", h.join("ssh-ran").display())).unwrap();
        fs::set_permissions(&ssh, fs::Permissions::from_mode(0o755)).unwrap();
        fs::create_dir_all(h.join(".config")).unwrap();
        // SAFETY: every test calls `home()` (directly or through `git`, `Remote::new` or `Machine::new`) before it
        // spawns a process, and `OnceLock` blocks the other callers until the variables are set.
        unsafe {
            std::env::set_var("HOME", &h);
            std::env::set_var("XDG_CONFIG_HOME", h.join(".config"));
            std::env::set_var("GIT_SSH_COMMAND", &ssh);
            for k in CLEARED {
                std::env::remove_var(k);
            }
        }
        t
    }).path()
}

/// The count of gpg calls in this process so far.
pub fn gpg_calls() -> usize {
    fs::read_to_string(home().join("gpg-ran")).map(|s| s.lines().count()).unwrap_or(0)
}

/// The count of ssh calls in this process so far.
pub fn ssh_calls() -> usize {
    fs::read_to_string(home().join("ssh-ran")).map(|s| s.lines().count()).unwrap_or(0)
}

/// A plain `git` in `cwd` with the test `HOME`, without the git variables of [`CLEARED`].
pub fn plain(cwd: &Path) -> Command {
    home();
    let mut c = Command::new("git");
    c.current_dir(cwd);
    for k in CLEARED {
        c.env_remove(k);
    }
    c
}

/// `git -c commit.gpgSign=false -c push.gpgSign=false -c user.name=t -c user.email=t@t.invalid <args>` in `cwd`;
/// asserts success and returns stdout without its last `\n`.
pub fn git(cwd: &Path, args: &[&str]) -> String {
    let o = plain(cwd)
        .args(["-c", "commit.gpgSign=false", "-c", "push.gpgSign=false", "-c", "user.name=t", "-c", "user.email=t@t.invalid"])
        .args(args).stdin(Stdio::null()).output().unwrap();
    assert!(o.status.success(), "git {args:?} in {}: {}", cwd.display(), String::from_utf8_lossy(&o.stderr));
    let mut s = String::from_utf8(o.stdout).unwrap();
    if s.ends_with('\n') {
        s.pop();
    }
    s
}

/// `git <args>` in `cwd` with `input` on stdin; asserts success and returns stdout without its last `\n`.
pub fn git_in(cwd: &Path, args: &[&str], input: &[u8]) -> String {
    let mut ch = plain(cwd).args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped())
        .spawn().unwrap();
    ch.stdin.take().unwrap().write_all(input).unwrap();
    let o = ch.wait_with_output().unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
    let mut s = String::from_utf8(o.stdout).unwrap();
    if s.ends_with('\n') {
        s.pop();
    }
    s
}

/// The blob id of `bytes`, written into the repository at `cwd`.
pub fn blob(cwd: &Path, bytes: &[u8]) -> String { git_in(cwd, &["hash-object", "-w", "--stdin"], bytes) }

/// A tree of the entries `(mode, name, hex id)` in the given order, written without git's checks.
pub fn tree_literally(cwd: &Path, entries: &[(&str, &[u8], &str)]) -> String {
    let mut b = Vec::new();
    for (mode, name, id) in entries {
        b.extend_from_slice(mode.as_bytes());
        b.push(b' ');
        b.extend_from_slice(name);
        b.push(0);
        b.extend_from_slice(&hex::decode(id).unwrap());
    }
    git_in(cwd, &["hash-object", "-t", "tree", "-w", "--literally", "--stdin"], &b)
}

/// `git commit-tree <tree> [-p <parent>] -m t`.
pub fn commit_tree(cwd: &Path, tree: &str, parent: Option<&str>) -> String {
    let mut a = vec!["commit-tree", tree];
    if let Some(p) = parent {
        a.extend(["-p", p]);
    }
    a.extend(["-m", "t"]);
    git(cwd, &a)
}

/// The sorted entry names of `dir`.
pub fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    v.sort();
    v
}

/// A canonical temp folder.
fn canonical_dir() -> (TempDir, PathBuf) {
    home();
    let t = TempDir::new().unwrap();
    let p = t.path().canonicalize().unwrap();
    (t, p)
}

/// The zygo paths of a test machine, under `p`.
fn paths(p: &Path) -> Paths {
    Paths {
        config_file: p.join("config.toml"),
        cache_dir: p.join("cache"),
        data_dir: p.join("data"),
        sessions_dir: p.join("cache/sessions"),
        marketplaces_dir: p.join("cache/marketplaces"),
        plugin_store: p.join("data/plugins"),
        approvals_file: p.join("data/approvals.toml"),
        usage_db: p.join("data/usage.db"),
    }
}

/// A local bare repository with a `main` branch, used by a `file://` URL.
pub struct Remote {
    pub dir: TempDir,
    pub bare: PathBuf,
    pub url: String,
}

impl Remote {
    pub fn new() -> Remote {
        let (dir, p) = canonical_dir();
        git(&p, &["init", "-q", "--bare", "bare.git"]);
        let bare = p.join("bare.git");
        let url = format!("file://{}", bare.display());
        git(&p, &["clone", "-q", &url, "seed"]);
        let seed = p.join("seed");
        fs::write(seed.join("README"), "x\n").unwrap();
        git(&seed, &["add", "README"]);
        git(&seed, &["commit", "-q", "-m", "seed"]);
        git(&seed, &["push", "-q", "origin", "HEAD:refs/heads/main"]);
        Remote { dir, bare, url }
    }

    /// The commit of `refs/heads/<branch>` on the remote, or `None`.
    pub fn tip(&self, branch: &str) -> Option<String> {
        let o = plain(&self.bare).args(["rev-parse", "--verify", "-q", &format!("refs/heads/{branch}")])
            .output().unwrap();
        o.status.success().then(|| String::from_utf8(o.stdout).unwrap().trim_end().to_string())
    }
}

/// One clone of a [`Remote`] (`root`) and the path of its side-branch worktree (`wt`, not made).
pub struct Machine {
    pub dir: TempDir,
    pub root: PathBuf,
    pub wt: PathBuf,
}

impl Machine {
    pub fn new(remote: &Remote) -> Machine {
        let (dir, p) = canonical_dir();
        git(&p, &["clone", "-q", &remote.url, "user"]);
        let wt = SyncPaths::new(&paths(&p), WS_ID).unwrap().worktree;
        Machine { dir, root: p.join("user"), wt }
    }
}
