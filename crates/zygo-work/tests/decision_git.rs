//! DR-11: the verdict git reader (D§9.4): four bounded git processes behind an injectable runner. No test waits on
//! wall-clock time: stub tests move a stub clock, and real-git tests run with every deadline an hour away.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};
use tempfile::TempDir;
use zygo_work::repo::{run_piped, RunError};

const NULL: &str = if cfg!(windows) { "NUL" } else { "/dev/null" };

/// A deadline one hour away: a test not about time never times out under machine load.
fn far() -> Instant { Instant::now() + Duration::from_secs(3600) }

/// `git -C <dir> <args>` with no global or system config, a fixed identity and `LC_ALL=C`.
fn git_in(dir: &Path, args: &[&str]) -> Command {
    let mut c = Command::new("git");
    c.arg("-C").arg(dir).args(["-c", "commit.gpgsign=false"]).args(args);
    c.env("GIT_CONFIG_GLOBAL", NULL).env("GIT_CONFIG_NOSYSTEM", "1").env("LC_ALL", "C");
    c.env("GIT_AUTHOR_NAME", "t").env("GIT_AUTHOR_EMAIL", "t@t").env("GIT_COMMITTER_NAME", "t").env("GIT_COMMITTER_EMAIL", "t@t");
    c
}

/// A setup git call that must succeed.
fn git(dir: &Path, args: &[&str]) {
    let out = git_in(dir, args).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// The trimmed stdout of a setup git call that must succeed.
fn out(dir: &Path, args: &[&str]) -> String {
    let o = git_in(dir, args).output().unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8(o.stdout).unwrap().trim().to_string()
}

/// `<tmp>/w` on `main` with one commit that holds the file `a`.
fn repo() -> (TempDir, PathBuf) {
    let t = TempDir::new().unwrap();
    git(t.path(), &["init", "-q", "-b", "main", "w"]);
    let w = t.path().join("w");
    fs::write(w.join("a"), "1\n").unwrap();
    git(&w, &["add", "-A"]);
    git(&w, &["commit", "-q", "-m", "a"]);
    (t, w)
}

// ---- Piped: the request-and-reply form of the bounded runner ----

#[test]
fn piped_past_deadline_times_out_with_no_spawn() {
    assert!(matches!(run_piped(Command::new("zygo-dr11-no-such-program"), Instant::now()), Err(RunError::TimedOut)));
}

#[test]
fn piped_answers_cat_file_batch_requests_in_turn() {
    let (_t, w) = repo();
    let mut p = run_piped(git_in(&w, &["cat-file", "--batch"]), far()).unwrap();
    p.send(b"HEAD^{commit}\n").unwrap();
    let h = String::from_utf8(p.read_line(600).unwrap()).unwrap();
    let (id, rest) = h.split_once(' ').unwrap();
    assert_eq!(id, out(&w, &["rev-parse", "HEAD"]));
    let size: usize = rest.strip_prefix("commit ").unwrap().parse().unwrap();
    assert_eq!(p.read_exact(size + 1).unwrap().last(), Some(&b'\n'));
    p.send(b"nope\n").unwrap();
    assert_eq!(p.read_line(600).unwrap(), b"nope missing");
}

#[cfg(unix)]
#[test]
fn piped_drop_kills_the_process_group() {
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;
    use std::os::unix::io::AsRawFd;
    let t = TempDir::new().unwrap();
    let fifo = t.path().join("f");
    assert!(Command::new("mkfifo").arg(&fifo).status().unwrap().success());
    // Read end first and non-blocking, so the grandchild's open for writing does not wait.
    let mut rd = fs::OpenOptions::new().read(true).custom_flags(libc::O_NONBLOCK).open(&fifo).unwrap();
    let mut c = Command::new("sh");
    // The grandchild opens the FIFO, then prints `go`; then it and the leader block.
    c.args(["-c", "( exec 3>\"$1\"; printf 'go\\n'; exec sleep 1000000 ) & exec sleep 1000000", "sh"]).arg(&fifo);
    let mut p = run_piped(c, far()).unwrap();
    assert_eq!(p.read_line(16).unwrap(), b"go");
    drop(p);
    assert_eq!(unsafe { libc::fcntl(rd.as_raw_fd(), libc::F_SETFL, 0) }, 0); // now blocking
    let mut rest = Vec::new();
    assert_eq!(rd.read_to_end(&mut rest).unwrap(), 0);
}
