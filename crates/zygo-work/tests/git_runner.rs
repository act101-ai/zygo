//! MS-1: the bounded git runner, whole mode and stream mode (S§8.7). No test waits on wall-clock time.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tempfile::TempDir;
use zygo_work::repo::{run_whole, RunError, Whole};

const NULL: &str = if cfg!(windows) { "NUL" } else { "/dev/null" };
const EMPTY_BLOB: &[u8] = b"e69de29bb2d1d6434b8b29ae775ad8c2e48c5391\n";

/// A deadline one hour away: a test not about time never times out under machine load.
fn far() -> Instant { Instant::now() + Duration::from_secs(3600) }

/// `git -C <dir> <args>` with no global or system config, a fixed identity and `LC_ALL=C`.
fn git_in(dir: &Path, args: &[&str]) -> Command {
    let mut c = Command::new("git");
    c.arg("-C").arg(dir).args(["-c", "commit.gpgsign=false", "-c", "user.name=t", "-c", "user.email=t@t"]).args(args);
    c.env("GIT_CONFIG_GLOBAL", NULL).env("GIT_CONFIG_NOSYSTEM", "1").env("LC_ALL", "C");
    c
}

/// A setup git call that must succeed.
fn git(dir: &Path, args: &[&str]) {
    let out = git_in(dir, args).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// `<tmp>/w` on `main` with one commit that holds the files `a`, `b c` and `d`.
fn repo() -> (TempDir, PathBuf) {
    let t = TempDir::new().unwrap();
    git(t.path(), &["init", "-q", "-b", "main", "w"]);
    let w = t.path().join("w");
    for (name, body) in [("a", "1\n"), ("b c", "2\n"), ("d", "3\n")] {
        fs::write(w.join(name), body).unwrap();
    }
    git(&w, &["add", "-A"]);
    git(&w, &["commit", "-q", "-m", "a"]);
    (t, w)
}

fn contains(hay: &[u8], needle: &[u8]) -> bool { hay.windows(needle.len()).any(|w| w == needle) }

#[test]
fn whole_zero_exit_gives_stdout_and_empty_stderr() {
    let (_t, w) = repo();
    let head = run_whole(git_in(&w, &["rev-parse", "HEAD"]), far()).unwrap();
    assert!(head.ok);
    assert_eq!(head.out.len(), 41);
    assert_eq!(head.out[40], b'\n');
    assert!(head.err.is_empty());
    // `checkout -b` writes "Switched to a new branch 'x'" on stderr and exits 0: that stderr is not kept.
    let co = run_whole(git_in(&w, &["checkout", "-b", "x"]), far()).unwrap();
    assert_eq!(co, Whole { ok: true, out: Vec::new(), err: Vec::new() });
}

#[test]
fn whole_non_zero_exit_gives_stdout_and_stderr() {
    let (t, w) = repo();
    let bare = t.path().join("r.git");
    git(t.path(), &["init", "-q", "--bare", "-b", "main", "r.git"]);
    git(&w, &["push", "-q", bare.to_str().unwrap(), "main"]);
    git(&w, &["commit", "-q", "--amend", "-m", "b"]); // not a fast-forward of the pushed tip
    let r = run_whole(git_in(&w, &["push", "--porcelain", bare.to_str().unwrap(), "main"]), far()).unwrap();
    assert!(!r.ok);
    assert!(contains(&r.out, b"!\trefs/heads/main:refs/heads/main\t[rejected] (non-fast-forward)"));
    assert!(contains(&r.err, b"failed to push some refs"));
    assert!(r.err.len() <= 64 * 1024);
}

#[cfg(unix)]
#[test]
fn whole_non_zero_exit_keeps_64_kib_of_stderr() {
    let mut c = Command::new("sh");
    c.args(["-c", "printf out; head -c 70000 /dev/zero | tr '\\0' e >&2; exit 3"]);
    let r = run_whole(c, far()).unwrap();
    assert!(!r.ok);
    assert_eq!(r.out, b"out");
    assert_eq!(r.err.len(), 64 * 1024);
    assert!(r.err.iter().all(|&b| b == b'e'));
}

#[test]
fn whole_past_deadline_times_out_with_no_spawn() {
    // The program does not exist: a spawn would give `NotRun`, so `TimedOut` proves that no spawn was tried.
    let r = run_whole(Command::new("zygo-ms1-no-such-program"), Instant::now());
    assert_eq!(r, Err(RunError::TimedOut));
}

#[test]
fn whole_runs_with_stdin_null() {
    let (t, w) = repo();
    let x = t.path().join("x");
    fs::write(&x, "x").unwrap();
    let mut c = git_in(&w, &["hash-object", "--stdin"]);
    c.stdin(Stdio::from(fs::File::open(&x).unwrap())); // the runner must replace this with null
    assert_eq!(run_whole(c, far()).unwrap(), Whole { ok: true, out: EMPTY_BLOB.to_vec(), err: Vec::new() });
}

/// `sh -c` that prints `<pid> <pgid>` of the shell, NUL-ended.
#[cfg(unix)]
const PGID: &str = "printf '%s %s\\000' $$ $(ps -o pgid= -p $$)";

#[cfg(unix)]
fn pid_and_pgid(b: &[u8]) -> (u32, u32) {
    let s = std::str::from_utf8(b).unwrap().trim_end_matches('\0');
    let mut n = s.split(' ').map(|v| v.parse().unwrap());
    (n.next().unwrap(), n.next().unwrap())
}

#[cfg(unix)]
#[test]
fn whole_runs_in_its_own_process_group() {
    let mut c = Command::new("sh");
    c.args(["-c", PGID]);
    let r = run_whole(c, far()).unwrap();
    assert!(r.ok);
    let (pid, pgid) = pid_and_pgid(&r.out);
    assert_eq!(pid, pgid); // the shell leads its own group
}
