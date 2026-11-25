//! DR-11: the verdict git reader (D§9.4): four bounded git processes behind an injectable runner. No test waits on
//! wall-clock time: stub tests move a stub clock, and real-git tests run with every deadline an hour away.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::time::{Duration, Instant};
use tempfile::TempDir;
use zygo_work::gate::git::{
    BatchIo, Blob, GitFail, GitRunner, LogCommit, ProcessGit, VerdictGit, GIT_PROCESSES, GIT_STEP, GIT_TOTAL,
};
use zygo_work::repo::{run_piped, RunError, Whole};

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

/// The record file name `DDR-0…0<c>.md` (26 characters after `DDR-`).
fn rec(c: char) -> String { format!("DDR-{}{c}.md", "0".repeat(25)) }

/// `c` 40 times: an object id.
fn hex(c: char) -> String { c.to_string().repeat(40) }

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

// ---- the stub runner: a test-set clock, canned replies, and a record of every run ----

#[derive(Default)]
struct Calls {
    /// The step (`cat-file`, `merge-base`, `rev-list` or `log`) and deadline of each process started.
    runs: Vec<(&'static str, Instant)>,
    /// Every request written to the `--batch` process.
    sent: Vec<Vec<u8>>,
}

struct Stub {
    clock: Rc<Cell<Instant>>,
    log: Rc<RefCell<Calls>>,
    whole: VecDeque<Result<Whole, RunError>>,
    canned: Vec<u8>,
}

fn step_of(cmd: &Command) -> &'static str {
    for a in cmd.get_args() {
        for s in ["cat-file", "merge-base", "rev-list", "log"] {
            if a == s {
                return s;
            }
        }
    }
    panic!("no git step in {cmd:?}")
}

impl GitRunner for Stub {
    fn run(&mut self, cmd: Command, deadline: Instant) -> Result<Whole, RunError> {
        self.log.borrow_mut().runs.push((step_of(&cmd), deadline));
        self.whole.pop_front().expect("a canned whole reply")
    }
    fn batch(&mut self, cmd: Command, deadline: Instant) -> Result<Box<dyn BatchIo>, RunError> {
        self.log.borrow_mut().runs.push((step_of(&cmd), deadline));
        Ok(Box::new(Canned { log: self.log.clone(), data: std::mem::take(&mut self.canned), at: 0 }))
    }
    fn now(&self) -> Instant { self.clock.get() }
}

/// A `--batch` process that gives canned stdout and `Failed` at its end.
struct Canned {
    log: Rc<RefCell<Calls>>,
    data: Vec<u8>,
    at: usize,
}

impl BatchIo for Canned {
    fn send(&mut self, line: &[u8]) -> Result<(), RunError> {
        self.log.borrow_mut().sent.push(line.to_vec());
        Ok(())
    }
    fn read_line(&mut self, max: usize) -> Result<Vec<u8>, RunError> {
        let rest = &self.data[self.at..];
        let i = rest[..rest.len().min(max + 1)].iter().position(|&b| b == b'\n').ok_or(RunError::Failed)?;
        self.at += i + 1;
        Ok(rest[..i].to_vec())
    }
    fn read_exact(&mut self, n: usize) -> Result<Vec<u8>, RunError> {
        let rest = &self.data[self.at..];
        if rest.len() < n {
            return Err(RunError::Failed);
        }
        self.at += n;
        Ok(rest[..n].to_vec())
    }
}

struct Rig {
    g: ProcessGit,
    t0: Instant,
    clock: Rc<Cell<Instant>>,
    log: Rc<RefCell<Calls>>,
}

/// A `ProcessGit` on the stub. `t0` is one `Instant::now()`, used only as the origin of the stub clock.
fn rig(whole: Vec<Result<Whole, RunError>>, canned: Vec<u8>) -> Rig {
    let t0 = Instant::now();
    let clock = Rc::new(Cell::new(t0));
    let log = Rc::new(RefCell::new(Calls::default()));
    let stub = Stub { clock: clock.clone(), log: log.clone(), whole: whole.into(), canned };
    Rig { g: ProcessGit::with_runner(Path::new("/zygo-dr11-no-such-dir"), Box::new(stub)), t0, clock, log }
}

fn ok(out: &str) -> Result<Whole, RunError> { Ok(Whole { ok: true, out: out.as_bytes().to_vec(), err: Vec::new() }) }

fn failed(step: &'static str, why: &str) -> GitFail { GitFail::Failed { step, why: why.into() } }

const S: fn(u64) -> Duration = Duration::from_secs;

#[test]
fn constants_hold_spec_values() {
    assert_eq!((GIT_STEP, GIT_TOTAL, GIT_PROCESSES), (S(2), S(10), 4));
    assert_eq!(failed("git log", "timed out").to_string(), "cannot check: git log timed out");
    assert_eq!(GitFail::TooMuch.to_string(), "too much to read");
}

#[test]
fn each_call_gets_min_of_step_and_total_deadline() {
    let Rig { mut g, t0, clock, log } = rig(vec![ok(&format!("{}\n", hex('a'))), ok(""), ok("")], Vec::new());
    assert_eq!(g.merge_base("a", "b"), Ok(Some(hex('a'))));
    clock.set(t0 + S(3));
    assert_eq!(g.rev_list("a", "b", 10), Ok(Some(vec![])));
    clock.set(t0 + S(9));
    assert_eq!(g.log("a", "b"), Ok(vec![]));
    assert_eq!(log.borrow().runs, vec![("merge-base", t0 + S(2)), ("rev-list", t0 + S(5)), ("log", t0 + S(10))]);
    clock.set(t0 + S(10));
    assert_eq!(g.commit("a"), Err(failed("git cat-file", "over 10 s in all")));
    assert_eq!(log.borrow().runs.len(), 3);
}

#[test]
fn fifth_process_is_refused_with_no_run() {
    let base = format!("{}\n", hex('a'));
    let Rig { mut g, log, .. } = rig(vec![ok(&base), ok(&base), ok(&base), ok(&base)], Vec::new());
    for _ in 0..4 {
        assert_eq!(g.merge_base("a", "b"), Ok(Some(hex('a'))));
    }
    assert_eq!(g.merge_base("a", "b"), Err(failed("git merge-base", "more than 4 git processes")));
    assert_eq!(log.borrow().runs.len(), 4);
}

#[test]
fn runner_timeout_names_the_step() {
    let Rig { mut g, .. } = rig(vec![Err(RunError::TimedOut)], Vec::new());
    assert_eq!(g.log("a", "b").unwrap_err().to_string(), "cannot check: git log timed out");
}

#[test]
fn merge_base_exit_status_is_judged_with_its_output() {
    let w = |ok: bool, err: &[u8]| Ok(Whole { ok, out: Vec::new(), err: err.to_vec() });
    let Rig { mut g, .. } = rig(vec![w(false, b""), w(true, b""), w(false, b"fatal: Not a valid object name nope\n")], Vec::new());
    assert_eq!(g.merge_base("a", "b"), Ok(None));
    assert_eq!(g.merge_base("a", "b"), Err(failed("git merge-base", "bad output")));
    assert_eq!(g.merge_base("a", "b"), Err(failed("git merge-base", "failed: fatal: Not a valid object name nope")));
}

#[test]
fn bad_revision_or_name_runs_nothing() {
    let Rig { mut g, log, .. } = rig(Vec::new(), Vec::new());
    assert_eq!(g.commit("-x"), Ok(None));
    assert_eq!(g.record_blob("a", "sub/x.md"), Ok(Blob::Missing));
    assert_eq!(g.merge_base("a", "-x"), Err(failed("git merge-base", "bad revision")));
    assert!(log.borrow().runs.is_empty());
}

#[test]
fn log_output_parses_into_commits() {
    let (a, b, m, p1, p2) = (hex('a'), hex('b'), hex('c'), hex('d'), hex('e'));
    let out = format!("{a}\0{p1}\0del\n\0\nD\0docs/decisions/A.md\0{m}\0{p1} {p2}\0merge\n\0{b}\0\0\0\nM\0docs/decisions/B.md\0A\0docs/other\0");
    let Rig { mut g, .. } = rig(vec![ok(&out), ok(&out[1..])], Vec::new());
    assert_eq!(g.log("x", "y").unwrap(), vec![
        LogCommit { id: a, parents: vec![p1.clone()], message: "del\n".into(), paths: vec![('D', "docs/decisions/A.md".into())] },
        LogCommit { id: m, parents: vec![p1, p2], message: "merge\n".into(), paths: vec![] },
        LogCommit { id: b, parents: vec![], message: String::new(), paths: vec![('M', "docs/decisions/B.md".into())] },
    ]);
    assert_eq!(g.log("x", "y"), Err(failed("git log", "bad output")));
}
