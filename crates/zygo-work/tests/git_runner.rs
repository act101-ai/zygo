//! MS-1: the bounded git runner, whole mode and stream mode (S§8.7). No test waits on wall-clock time.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tempfile::TempDir;
use zygo_work::repo::{run_stream, run_whole, Flow, RunError, Streamed, Whole};

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

/// Every entry `run_stream` passes, with `Flow::Go`.
fn stream_all(cmd: Command) -> (Result<Streamed, RunError>, Vec<Vec<u8>>) {
    let mut seen = Vec::new();
    let r = run_stream(cmd, far(), &mut |e| {
        seen.push(e.to_vec());
        Flow::Go
    });
    (r, seen)
}

#[test]
fn stream_passes_each_nul_ended_entry_of_ls_tree_once() {
    let (_t, w) = repo();
    let (r, seen) = stream_all(git_in(&w, &["ls-tree", "-z", "HEAD"]));
    assert_eq!(r, Ok(Streamed::Done { ok: true, err: Vec::new() }));
    assert_eq!(seen.len(), 3);
    let names: Vec<&[u8]> = seen.iter().map(|e| e.rsplit(|&b| b == b'\t').next().unwrap()).collect();
    assert_eq!(names, [&b"a"[..], &b"b c"[..], &b"d"[..]]);
    for e in &seen {
        assert!(e.starts_with(b"100644 blob "));
        assert!(!e.contains(&0)); // the NUL ends the entry and is not passed
    }
}

#[test]
fn stream_non_zero_exit_gives_stderr() {
    let (_t, w) = repo();
    let (r, seen) = stream_all(git_in(&w, &["ls-tree", "-z", "no-such-rev"]));
    let Ok(Streamed::Done { ok: false, err }) = &r else { panic!("{r:?}") };
    assert!(!err.is_empty() && err.len() <= 64 * 1024);
    assert!(seen.is_empty());
}

#[test]
fn stream_stop_returns_stopped_after_one_entry() {
    let (_t, w) = repo();
    let mut calls = 0;
    let r = run_stream(git_in(&w, &["ls-tree", "-z", "HEAD"]), far(), &mut |_| {
        calls += 1;
        Flow::Stop
    });
    assert_eq!(r, Ok(Streamed::Stopped));
    assert_eq!(calls, 1);
}

#[test]
fn stream_past_deadline_times_out_with_no_spawn() {
    let mut calls = 0;
    let r = run_stream(Command::new("zygo-ms1-no-such-program"), Instant::now(), &mut |_| {
        calls += 1;
        Flow::Go
    });
    assert_eq!(r, Err(RunError::TimedOut)); // a spawn would give `NotRun`
    assert_eq!(calls, 0);
}

#[test]
fn stream_runs_with_stdin_null() {
    let (t, w) = repo();
    fs::write(w.join(".gitattributes"), "a foo=bar\n").unwrap();
    let paths = t.path().join("paths");
    fs::write(&paths, b"a\0").unwrap();
    let mut c = git_in(&w, &["check-attr", "--stdin", "-z", "-a"]);
    c.stdin(Stdio::from(fs::File::open(&paths).unwrap())); // if read, git would print `a`, `foo`, `bar`
    let (r, seen) = stream_all(c);
    assert_eq!(r, Ok(Streamed::Done { ok: true, err: Vec::new() }));
    assert!(seen.is_empty());
}

#[cfg(unix)]
#[test]
fn stream_runs_in_its_own_process_group() {
    let mut c = Command::new("sh");
    c.args(["-c", PGID]);
    let (r, seen) = stream_all(c);
    assert_eq!(r, Ok(Streamed::Done { ok: true, err: Vec::new() }));
    assert_eq!(seen.len(), 1);
    let (pid, pgid) = pid_and_pgid(&seen[0]);
    assert_eq!(pid, pgid);
}

/// After `Flow::Stop` the whole process group is gone, proven with no timer: the leader and a grandchild hold the
/// only write ends of a pipe (fd 3), and a blocking read of the pipe ends (EOF) only when both are dead. A runner that
/// kills only the leader makes the read hang until the CI job timeout; it cannot pass. An anonymous pipe, not a FIFO:
/// on macOS a reader blocked on a FIFO can miss the wakeup when the last writer closes, and hang with no writer left.
/// `sleep 1000000` is the blocker that Stop must kill, not a wait of the test.
#[cfg(unix)]
#[test]
fn stream_stop_kills_the_process_group() {
    use std::io::Read;
    use std::os::unix::io::AsRawFd;
    use std::os::unix::process::CommandExt;
    let (mut rd, wr) = std::io::pipe().unwrap(); // both ends close-on-exec
    let w = wr.as_raw_fd();
    let mut c = Command::new("sh");
    // The grandchild prints `go` once it runs; then it and the leader block, each holding fd 3.
    c.args(["-c", "( printf 'go\\000'; exec sleep 1000000 ) & exec sleep 1000000"]);
    // SAFETY: only async-signal-safe calls between fork and exec.
    unsafe {
        c.pre_exec(move || {
            let r = if w == 3 { libc::fcntl(3, libc::F_SETFD, 0) } else { libc::dup2(w, 3) };
            if r == -1 { Err(std::io::Error::last_os_error()) } else { Ok(()) }
        });
    }
    let mut seen = Vec::new();
    let r = run_stream(c, far(), &mut |e| {
        seen.push(e.to_vec());
        Flow::Stop
    });
    drop(wr); // the test's own write end, else the read below never ends
    assert_eq!(r, Ok(Streamed::Stopped));
    assert_eq!(seen, [b"go".to_vec()]);
    let mut rest = Vec::new();
    assert_eq!(rd.read_to_end(&mut rest).unwrap(), 0);
}

/// Review F1: the deadline holds while entries keep coming. The callback runs past the deadline, so the run must end
/// `TimedOut` with no entry handed over after the deadline. A runner that hands one over gets `Stop` and fails at
/// once. The 250 ms only lets the first entry arrive before the deadline (so the check has power); correct code passes
/// whatever the machine load.
#[cfg(unix)]
#[test]
fn stream_deadline_while_entries_flow() {
    let mut c = Command::new("sh");
    c.args(["-c", "while :; do printf 'a\\000'; done"]);
    let deadline = Instant::now() + Duration::from_millis(250);
    let mut late = false;
    let r = run_stream(c, deadline, &mut |_| {
        if Instant::now() >= deadline {
            late = true;
            return Flow::Stop;
        }
        while Instant::now() < deadline {}
        Flow::Go
    });
    assert_eq!(r, Err(RunError::TimedOut));
    assert!(!late, "an entry was handed over after the deadline");
}

/// Review F2: as in whole mode, the end of the leader ends the run, although a background child still holds stdout.
/// A runner that waits for EOF fails at the deadline (an error, not a hang); `sleep 30` is the holder, not a wait.
#[cfg(unix)]
#[test]
fn stream_leader_exit_with_child_holding_stdout() {
    let mut c = Command::new("sh");
    c.args(["-c", "printf 'a\\000'; sleep 30 & exit 0"]);
    let mut seen = Vec::new();
    let r = run_stream(c, Instant::now() + Duration::from_secs(20), &mut |e| {
        seen.push(e.to_vec());
        Flow::Go
    });
    assert!(matches!(r, Ok(Streamed::Done { ok: true, .. })), "{r:?}");
    assert_eq!(seen, [b"a".to_vec()]);
}

/// Review F3: a callback that panics still leaves no process: the run kills and reaps the group while it unwinds, so
/// the PID is gone (ESRCH) at once. `sleep 1000` is the process that must be killed, not a wait of the test.
#[cfg(unix)]
#[test]
fn stream_callback_panic_kills_group() {
    let mut c = Command::new("sh");
    c.args(["-c", "printf '%s\\000' $$; exec sleep 1000"]);
    let mut pid: i32 = 0;
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_stream(c, far(), &mut |e| {
            pid = std::str::from_utf8(e).unwrap().parse().unwrap();
            panic!("the callback panics");
        })
    }));
    assert!(r.is_err());
    assert!(pid > 0);
    assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
    assert_eq!(std::io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
}

/// Review F4: a last piece with no NUL is not an entry.
#[cfg(unix)]
#[test]
fn stream_drops_last_piece_without_nul() {
    let mut c = Command::new("sh");
    c.args(["-c", "printf 'a\\000b'"]);
    let (r, seen) = stream_all(c);
    assert!(matches!(r, Ok(Streamed::Done { ok: true, .. })), "{r:?}");
    assert_eq!(seen, [b"a".to_vec()]);
}
