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
    BatchIo, Blob, GitFail, GitRunner, LogCommit, ProcessGit, ProcessRunner, VerdictGit, GIT_PROCESSES, GIT_STEP,
    GIT_TOTAL,
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

/// Dropping `Piped` kills the whole process group, proven with no timer as in git_runner.rs
/// `stream_stop_kills_the_process_group`: the leader and a grandchild hold the only write ends of a pipe (fd 3), and a
/// blocking read ends (EOF) only when both are dead. An anonymous pipe, not a FIFO: on macOS a reader blocked on a FIFO
/// can miss the wakeup when the last writer closes, and hang with no writer left.
#[cfg(unix)]
#[test]
fn piped_drop_kills_the_process_group() {
    use std::io::Read;
    use std::os::unix::io::AsRawFd;
    use std::os::unix::process::CommandExt;
    let (mut rd, wr) = std::io::pipe().unwrap(); // both ends close-on-exec
    let w = wr.as_raw_fd();
    let mut c = Command::new("sh");
    // The grandchild prints `go` once it runs; then it and the leader block, each holding fd 3.
    c.args(["-c", "( printf 'go\\n'; exec sleep 1000000 ) & exec sleep 1000000"]);
    // SAFETY: only async-signal-safe calls between fork and exec.
    unsafe {
        c.pre_exec(move || {
            let r = if w == 3 { libc::fcntl(3, libc::F_SETFD, 0) } else { libc::dup2(w, 3) };
            if r == -1 { Err(std::io::Error::last_os_error()) } else { Ok(()) }
        });
    }
    let mut p = run_piped(c, far()).unwrap();
    assert_eq!(p.read_line(16).unwrap(), b"go");
    drop(p);
    drop(wr); // the test's own write end, else the read below never ends
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
    /// The `--batch` processes dropped.
    drops: usize,
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

impl Drop for Canned {
    fn drop(&mut self) { self.log.borrow_mut().drops += 1; }
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
    let canned = format!("{} commit 0\n\n", hex('a')).into_bytes();
    let Rig { mut g, log, .. } = rig(vec![ok(&base), ok(""), ok("")], canned);
    assert_eq!(g.commit("a"), Ok(Some(hex('a'))));
    assert_eq!(g.merge_base("a", "b"), Ok(Some(hex('a'))));
    assert_eq!(g.rev_list("a", "b", 10), Ok(Some(vec![])));
    assert_eq!(g.log("a", "b"), Ok(vec![]));
    assert_eq!(g.merge_base("a", "b"), Err(failed("git merge-base", "more than 4 git processes")));
    assert_eq!(g.log("a", "b"), Err(failed("git log", "more than 4 git processes")));
    assert_eq!(log.borrow().runs.len(), 4);
}

#[test]
fn second_merge_base_is_refused_with_no_run() {
    let base = format!("{}\n", hex('a'));
    let Rig { mut g, log, .. } = rig(vec![ok(&base), ok(&base)], Vec::new());
    assert_eq!(g.merge_base("a", "b"), Ok(Some(hex('a'))));
    assert_eq!(g.merge_base("a", "b"), Err(failed("git merge-base", "more than 4 git processes")));
    assert_eq!(log.borrow().runs.len(), 1);
}

#[test]
fn over_10_s_in_all_refuses_merge_base_rev_list_and_log() {
    let canned = format!("{} commit 0\n\n", hex('a')).into_bytes();
    let Rig { mut g, t0, clock, log } = rig(Vec::new(), canned);
    assert_eq!(g.commit("a"), Ok(Some(hex('a'))));
    clock.set(t0 + S(10));
    assert_eq!(g.merge_base("a", "b"), Err(failed("git merge-base", "over 10 s in all")));
    assert_eq!(g.rev_list("a", "b", 10), Err(failed("git rev-list", "over 10 s in all")));
    assert_eq!(g.log("a", "b"), Err(failed("git log", "over 10 s in all")));
    assert_eq!(log.borrow().runs.len(), 1);
}

#[test]
fn runner_timeout_names_the_step() {
    let Rig { mut g, .. } = rig(vec![Err(RunError::TimedOut)], Vec::new());
    assert_eq!(g.log("a", "b").unwrap_err().to_string(), "cannot check: git log timed out");
}

#[test]
fn merge_base_exit_status_is_judged_with_its_output() {
    let w = |ok: bool, err: &[u8]| Ok(Whole { ok, out: Vec::new(), err: err.to_vec() });
    let one = |r| rig(vec![r], Vec::new()).g.merge_base("a", "b");
    assert_eq!(one(w(false, b"")), Ok(None));
    assert_eq!(one(w(true, b"")), Err(failed("git merge-base", "bad output")));
    assert_eq!(one(w(false, b"fatal: Not a valid object name nope\n")), Err(failed("git merge-base", "failed: fatal: Not a valid object name nope")));
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
fn range_shaped_revision_runs_nothing() {
    let Rig { mut g, log, .. } = rig(Vec::new(), Vec::new());
    assert_eq!(g.rev_list("x.", "y", 10), Err(failed("git rev-list", "bad revision")));
    assert_eq!(g.log("x", "y."), Err(failed("git log", "bad revision")));
    assert_eq!(g.merge_base("a..b", "c"), Err(failed("git merge-base", "bad revision")));
    assert_eq!(g.commit("a..b"), Ok(None));
    assert_eq!(g.record_blob("a...b", "A.md"), Ok(Blob::Missing));
    assert!(log.borrow().runs.is_empty());
}

#[test]
fn log_output_parses_into_commits() {
    let (a, b, m, p1, p2) = (hex('a'), hex('b'), hex('c'), hex('d'), hex('e'));
    let out = format!("{a}\0{p1}\0del\n\0\nD\0docs/decisions/A.md\0{m}\0{p1} {p2}\0merge\n\0{b}\0\0\0\nM\0docs/decisions/B.md\0A\0docs/other\0");
    let Rig { mut g, .. } = rig(vec![ok(&out)], Vec::new());
    assert_eq!(g.log("x", "y").unwrap(), vec![
        LogCommit { id: a, parents: vec![p1.clone()], message: "del\n".into(), paths: vec![('D', "docs/decisions/A.md".into())] },
        LogCommit { id: m, parents: vec![p1, p2], message: "merge\n".into(), paths: vec![] },
        LogCommit { id: b, parents: vec![], message: String::new(), paths: vec![('M', "docs/decisions/B.md".into())] },
    ]);
    assert_eq!(rig(vec![ok(&out[1..])], Vec::new()).g.log("x", "y"), Err(failed("git log", "bad output")));
}

#[test]
fn log_output_without_the_closing_nul_is_bad_output() {
    let out = format!("{}\0{}\0msg\0\nA\0docs/decisions/DD", hex('a'), hex('b'));
    assert_eq!(rig(vec![ok(&out)], Vec::new()).g.log("x", "y"), Err(failed("git log", "bad output")));
    let out = format!("{}\0{}\0msg", hex('a'), hex('b'));
    assert_eq!(rig(vec![ok(&out)], Vec::new()).g.log("x", "y"), Err(failed("git log", "bad output")));
}

#[test]
fn the_2049th_object_read_is_too_much() {
    let canned = format!("{} commit 0\n\n", hex('a')).repeat(2_048).into_bytes();
    let Rig { mut g, log, .. } = rig(Vec::new(), canned);
    for _ in 0..2_048 {
        assert_eq!(g.commit("a"), Ok(Some(hex('a'))));
    }
    assert_eq!(g.commit("a"), Err(GitFail::TooMuch));
    assert_eq!(log.borrow().sent.len(), 2_048); // the 2,049th request is not sent
}

#[test]
fn a_read_past_16_mib_in_all_is_too_much() {
    let big = 16 * 1024 * 1024 - 1;
    let mut canned = format!("{} commit {big}\n", hex('a')).into_bytes();
    canned.resize(canned.len() + big, b'x');
    canned.extend_from_slice(format!("\n{a} commit 1\nx\n{a} commit 1\n", a = hex('a')).as_bytes());
    let Rig { mut g, .. } = rig(Vec::new(), canned);
    assert_eq!(g.commit("a"), Ok(Some(hex('a'))));
    assert_eq!(g.commit("a"), Ok(Some(hex('a')))); // exactly 16 MiB in all
    assert_eq!(g.commit("a"), Err(GitFail::TooMuch));
}

#[test]
fn a_failed_batch_stays_failed_with_one_process() {
    let Rig { mut g, log, .. } = rig(Vec::new(), Vec::new());
    let e: Result<Option<String>, GitFail> = Err(failed("git cat-file", "failed"));
    assert_eq!(g.commit("a"), e);
    assert_eq!(g.record_blob("a", &rec('A')), e.map(|_: Option<String>| Blob::Missing));
    assert_eq!(log.borrow().runs.iter().filter(|r| r.0 == "cat-file").count(), 1);
    assert_eq!(log.borrow().drops, 1);
}

#[test]
fn too_much_drops_the_batch_and_later_reads_start_no_process() {
    let canned = format!("{} commit {}\n", hex('a'), 16 * 1024 * 1024 + 1).into_bytes();
    let Rig { mut g, log, .. } = rig(Vec::new(), canned);
    assert_eq!(g.commit("a"), Err(GitFail::TooMuch));
    assert_eq!(log.borrow().drops, 1);
    assert_eq!(g.commit("b"), Err(GitFail::TooMuch));
    assert_eq!(g.record_blob("a", "A.md"), Err(GitFail::TooMuch));
    assert_eq!(log.borrow().runs.len(), 1);
    assert_eq!(log.borrow().sent.len(), 1);
}

/// A raw tree with regular entries `A.md` (id `aa…`) and `B.md` (id `bb…`).
fn tree_ab() -> Vec<u8> {
    let mut t = b"100644 A.md\0".to_vec();
    t.extend_from_slice(&[0xaa; 20]);
    t.extend_from_slice(b"100644 B.md\0");
    t.extend_from_slice(&[0xbb; 20]);
    t
}

/// The `--batch` reply of one object.
fn reply(id: char, kind: &str, body: &[u8]) -> Vec<u8> {
    let mut r = format!("{} {kind} {}\n", hex(id), body.len()).into_bytes();
    r.extend_from_slice(body);
    r.push(b'\n');
    r
}

#[test]
fn record_blob_reads_and_counts_each_tree_once() {
    let tree = tree_ab();
    // tree + A + B = exactly 16 MiB; a second count of the tree goes past it.
    let a = vec![b'x'; 16 * 1024 * 1024 - tree.len() - 1];
    let mut canned = reply('c', "tree", &tree);
    canned.extend(reply('a', "blob", &a));
    canned.extend(reply('b', "blob", b"y"));
    // 3 objects read; 2,045 more fit in 2,048, and the next is too much.
    canned.extend(format!("{} commit 0\n\n", hex('d')).repeat(2_045).into_bytes());
    let Rig { mut g, log, .. } = rig(Vec::new(), canned);
    assert_eq!(g.record_blob("r", "A.md"), Ok(Blob::Data(a)));
    assert_eq!(g.record_blob("r", "B.md"), Ok(Blob::Data(b"y".to_vec())));
    assert_eq!(&log.borrow().sent[..3], [b"r:docs/decisions\n".to_vec(), format!("{}\n", hex('a')).into_bytes(),
        format!("{}\n", hex('b')).into_bytes()]);
    for _ in 0..2_045 {
        assert_eq!(g.commit("d"), Ok(Some(hex('d'))));
    }
    assert_eq!(g.commit("d"), Err(GitFail::TooMuch));
}

#[test]
fn record_blob_malformed_tree_is_bad_output() {
    let Rig { mut g, .. } = rig(Vec::new(), reply('c', "tree", b"100644 A.md"));
    assert_eq!(g.record_blob("r", "A.md"), Err(failed("git cat-file", "bad output")));
}

#[test]
fn record_blob_id_that_is_not_a_blob_is_bad_output() {
    let mut canned = reply('c', "tree", &tree_ab());
    canned.extend(reply('a', "tree", b""));
    let Rig { mut g, .. } = rig(Vec::new(), canned);
    assert_eq!(g.record_blob("r", "A.md"), Err(failed("git cat-file", "bad output")));
}

#[test]
fn record_blob_tree_bytes_count_toward_16_mib() {
    let tree = tree_ab();
    let mut canned = reply('c', "tree", &tree);
    canned.extend(format!("{} blob {}\n", hex('a'), 16 * 1024 * 1024 - tree.len() + 1).into_bytes());
    let Rig { mut g, log, .. } = rig(Vec::new(), canned);
    assert_eq!(g.record_blob("r", "A.md"), Err(GitFail::TooMuch));
    assert_eq!(log.borrow().drops, 1);
}

// ---- Piped: line, end-of-output and closed-stdin failures (no wall-clock wait) ----

#[cfg(unix)]
fn sh(script: &str) -> zygo_work::repo::Piped {
    let mut c = Command::new("sh");
    c.args(["-c", script]);
    run_piped(c, far()).unwrap()
}

#[cfg(unix)]
#[test]
fn piped_read_line_past_max_fails() {
    let mut p = sh("printf 'abcdefgh\\n'");
    assert!(matches!(p.read_line(4), Err(RunError::Failed)));
}

#[cfg(unix)]
#[test]
fn piped_end_of_output_fails() {
    let mut p = sh("printf 'ab\\nc'");
    assert_eq!(p.read_line(16).unwrap(), b"ab");
    assert!(matches!(p.read_line(16), Err(RunError::Failed)));
    assert!(matches!(p.read_exact(2), Err(RunError::Failed)));
}

#[cfg(unix)]
#[test]
fn piped_send_after_the_child_closed_stdin_fails() {
    // The child closes its stdin, then says so; it exits at once.
    let mut p = sh("exec 0<&-; printf 'closed\\n'");
    assert_eq!(p.read_line(16).unwrap(), b"closed");
    let r = p.send(b"x\n").and_then(|()| p.read_line(16).map(drop));
    assert!(matches!(r, Err(RunError::Failed)));
}

#[cfg(unix)]
#[test]
fn piped_send_then_read_fails_with_a_foreign_reader() {
    // A background process holds the stdin read end, so the write succeeds; the read then sees the exit.
    let mut p = sh("sleep 30 <&0 >/dev/null 2>&1 & exec 0<&-; printf 'closed\\n'");
    assert_eq!(p.read_line(16).unwrap(), b"closed");
    let r = p.send(b"x\n").and_then(|()| p.read_line(16).map(drop));
    assert!(matches!(r, Err(RunError::Failed)));
}

// ---- real git: PD1 and the tree entry modes (D§6.8) ----

/// The real runner with its clock one hour ahead, so every deadline is an hour away and machine load cannot fail a test.
struct Far(ProcessRunner);

impl GitRunner for Far {
    fn run(&mut self, cmd: Command, deadline: Instant) -> Result<Whole, RunError> { self.0.run(cmd, deadline) }
    fn batch(&mut self, cmd: Command, deadline: Instant) -> Result<Box<dyn BatchIo>, RunError> { self.0.batch(cmd, deadline) }
    fn now(&self) -> Instant { Instant::now() + S(3600) }
}

/// [`Far`] with `HOME`, `XDG_CONFIG_HOME` (and `USERPROFILE`) set to an empty temp dir on each command, so no
/// developer `~/.gitconfig` reaches a real-git test.
struct Iso {
    far: Far,
    home: TempDir,
}

impl Iso {
    fn isolate(&self, mut cmd: Command) -> Command {
        cmd.env("HOME", self.home.path()).env("XDG_CONFIG_HOME", self.home.path());
        if cfg!(windows) {
            cmd.env("USERPROFILE", self.home.path());
        }
        cmd
    }
}

impl GitRunner for Iso {
    fn run(&mut self, cmd: Command, deadline: Instant) -> Result<Whole, RunError> {
        let cmd = self.isolate(cmd);
        self.far.run(cmd, deadline)
    }
    fn batch(&mut self, cmd: Command, deadline: Instant) -> Result<Box<dyn BatchIo>, RunError> {
        let cmd = self.isolate(cmd);
        self.far.batch(cmd, deadline)
    }
    fn now(&self) -> Instant { self.far.now() }
}

fn far_git(root: &Path) -> ProcessGit {
    ProcessGit::with_runner(root, Box::new(Iso { far: Far(ProcessRunner), home: TempDir::new().unwrap() }))
}

struct Fx {
    base: String,
    c1: String,
    c2: String,
}

/// `<tmp>/w` on `main` at `base` (`docs/decisions/README.md`), and the linked checkout `<tmp>/n` on `node` with `c1`
/// (records A and X, X with mode 100755) and `c2` (A changed, L a symlink entry, S a submodule entry). `w` stays at
/// `base`: `c1` and `c2` exist only on `node`.
fn fixture() -> (TempDir, PathBuf, Fx) {
    let t = TempDir::new().unwrap();
    git(t.path(), &["init", "-q", "-b", "main", "w"]);
    let w = t.path().join("w");
    fs::create_dir_all(w.join("docs/decisions")).unwrap();
    fs::write(w.join("docs/decisions/README.md"), "readme\n").unwrap();
    git(&w, &["add", "-A"]);
    git(&w, &["commit", "-q", "-m", "base"]);
    let base = out(&w, &["rev-parse", "HEAD"]);
    git(&w, &["worktree", "add", "-q", "../n", "-b", "node"]);
    let n = t.path().join("n");
    let d = |c: char| format!("docs/decisions/{}", rec(c));
    fs::write(n.join(d('A')), "a1\n").unwrap();
    fs::write(n.join(d('X')), "x\n").unwrap();
    git(&n, &["add", &d('A'), &d('X')]);
    git(&n, &["update-index", "--chmod=+x", &d('X')]);
    git(&n, &["commit", "-q", "-m", "c1"]);
    let c1 = out(&n, &["rev-parse", "HEAD"]);
    fs::write(n.join(d('A')), "a2\n").unwrap();
    git(&n, &["add", &d('A')]);
    let readme = out(&n, &["hash-object", "-w", "docs/decisions/README.md"]);
    git(&n, &["update-index", "--add", "--cacheinfo", &format!("120000,{readme},{}", d('L'))]);
    git(&n, &["update-index", "--add", "--cacheinfo", &format!("160000,{base},{}", d('S'))]);
    git(&n, &["commit", "-q", "-m", "c2\n\nZygo-Session: s"]);
    let c2 = out(&n, &["rev-parse", "HEAD"]);
    (t, w, Fx { base, c1, c2 })
}

#[test]
fn pd1_main_checkout_reads_linked_checkout_commits_before_merge() {
    let (_t, w, fx) = fixture();
    let mut g = far_git(&w);
    let d = |c: char| format!("docs/decisions/{}", rec(c));
    assert_eq!(out(&w, &["rev-parse", "HEAD"]), fx.base);
    assert_eq!(g.commit(&fx.c2[..7]), Ok(Some(fx.c2.clone())));
    assert_eq!(g.record_blob(&fx.c2, &rec('A')), Ok(Blob::Data(b"a2\n".to_vec())));
    assert_eq!(g.merge_base(&fx.base, &fx.c2), Ok(Some(fx.base.clone())));
    assert_eq!(g.rev_list(&fx.base, &fx.c2, 10_000), Ok(Some(vec![fx.c2.clone(), fx.c1.clone()])));
    assert_eq!(g.log(&fx.base, &fx.c2), Ok(vec![
        LogCommit { id: fx.c2.clone(), parents: vec![fx.c1.clone()], message: "c2\n\nZygo-Session: s\n".into(),
                    paths: vec![('M', d('A')), ('A', d('L')), ('A', d('S'))] },
        LogCommit { id: fx.c1.clone(), parents: vec![fx.base.clone()], message: "c1\n".into(),
                    paths: vec![('A', d('A')), ('A', d('X'))] },
    ]));
}

#[test]
fn record_blob_tells_regular_link_submodule_and_missing() {
    let (_t, w, fx) = fixture();
    let mut g = far_git(&w);
    assert_eq!(g.record_blob(&fx.c2, &rec('X')), Ok(Blob::Data(b"x\n".to_vec())));
    assert_eq!(g.record_blob(&fx.c2, &rec('L')), Ok(Blob::NotRegular));
    assert_eq!(g.record_blob(&fx.c2, &rec('S')), Ok(Blob::NotRegular));
    assert_eq!(g.record_blob(&fx.c2, &rec('Z')), Ok(Blob::Missing));
    assert_eq!(g.record_blob(&fx.base, &rec('A')), Ok(Blob::Missing));
    assert_eq!(g.record_blob("nope", "README.md"), Ok(Blob::Missing));
}

#[test]
fn docs_decisions_as_a_blob_is_missing() {
    let t = TempDir::new().unwrap();
    git(t.path(), &["init", "-q", "-b", "main", "w"]);
    let w = t.path().join("w");
    fs::create_dir_all(w.join("docs")).unwrap();
    fs::write(w.join("docs/decisions"), "x\n").unwrap();
    git(&w, &["add", "-A"]);
    git(&w, &["commit", "-q", "-m", "a"]);
    let mut g = far_git(&w);
    assert_eq!(g.record_blob("HEAD", &rec('A')), Ok(Blob::Missing));
}

#[test]
fn rev_list_over_max_is_none() {
    let (_t, w, fx) = fixture();
    assert_eq!(far_git(&w).rev_list(&fx.base, &fx.c2, 1), Ok(None));
    assert_eq!(far_git(&w).rev_list(&fx.base, &fx.c2, 2).unwrap().unwrap().len(), 2);
}

#[test]
fn merge_base_of_unknown_object_fails_naming_merge_base() {
    let (_t, w, fx) = fixture();
    let mut g = far_git(&w);
    let e = g.merge_base("nope", &fx.base).unwrap_err();
    assert!(matches!(&e, GitFail::Failed { step: "git merge-base", .. }), "{e:?}");
    assert!(e.to_string().starts_with("cannot check: git merge-base failed: "), "{e}");
}

#[test]
fn merge_base_of_unrelated_histories_is_none() {
    let (_t, w, fx) = fixture();
    let o = out(&w, &["commit-tree", &format!("{}^{{tree}}", fx.base), "-m", "o"]);
    let mut g = far_git(&w);
    assert_eq!(g.merge_base(&fx.base, &o), Ok(None));
}

#[test]
fn log_lists_an_added_gitlink_under_ignore_submodules_all() {
    let (_t, w, fx) = fixture();
    git(&w, &["config", "diff.ignoreSubmodules", "all"]);
    let mut g = far_git(&w);
    let c2 = g.log(&fx.c1, &fx.c2).unwrap();
    assert_eq!(c2.len(), 1);
    assert!(c2[0].paths.contains(&('A', format!("docs/decisions/{}", rec('S')))), "{:?}", c2[0].paths);
}

#[test]
fn log_parses_under_a_utf16_log_output_encoding() {
    let (_t, w, fx) = fixture();
    git(&w, &["config", "i18n.logOutputEncoding", "UTF-16"]);
    let mut g = far_git(&w);
    let c = g.log(&fx.base, &fx.c2).unwrap();
    assert_eq!(c.iter().map(|c| c.message.as_str()).collect::<Vec<_>>(), ["c2\n\nZygo-Session: s\n", "c1\n"]);
}

#[test]
fn log_lists_the_paths_of_a_merge_against_its_first_parent() {
    let (t, w, fx) = fixture();
    let n = t.path().join("n");
    git(&w, &["checkout", "-q", "-b", "side"]);
    fs::write(w.join("side.txt"), "s\n").unwrap();
    git(&w, &["add", "-A"]);
    git(&w, &["commit", "-q", "-m", "side"]);
    let side = out(&w, &["rev-parse", "HEAD"]);
    git(&w, &["checkout", "-q", "main"]);
    git(&n, &["merge", "-q", "--no-ff", "--no-commit", "side"]);
    let d = format!("docs/decisions/{}", rec('M'));
    fs::write(n.join(&d), "m\n").unwrap();
    git(&n, &["add", &d]); // only the new file: the fixture's index-only entries of c2 stay as they are
    git(&n, &["commit", "-q", "-m", "merge"]);
    let m = out(&n, &["rev-parse", "HEAD"]);
    assert_eq!(far_git(&w).log(&fx.c2, &m), Ok(vec![
        LogCommit { id: m, parents: vec![fx.c2.clone(), side], message: "merge\n".into(), paths: vec![('A', d)] },
    ]));
}
