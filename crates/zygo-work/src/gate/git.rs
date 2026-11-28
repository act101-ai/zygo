//! The verdict git reader (D§9.4): one `git cat-file --batch`, one `git merge-base`, one `git rev-list` and one
//! `git log`, each bounded, behind an injectable runner (D§17).
use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use zygo_core::esc;

use crate::repo::{RunError, Whole};

/// The deadline of one git process.
pub const GIT_STEP: Duration = Duration::from_secs(2);
/// The deadline of all git processes of one verdict, from the start of the first.
pub const GIT_TOTAL: Duration = Duration::from_secs(10);
/// The git processes one verdict may start.
pub const GIT_PROCESSES: usize = 4;

const CAT_FILE: &str = "git cat-file";
const MERGE_BASE: &str = "git merge-base";
const REV_LIST: &str = "git rev-list";
const LOG: &str = "git log";
/// The object reads of one `--batch` process (D§9.4).
const BATCH_OBJECTS: usize = 2_048;
/// The object bytes read by one `--batch` process, in all (D§9.4).
const BATCH_BYTES: u64 = 16 * 1024 * 1024;
/// The longest `--batch` reply header read.
const HEADER_MAX: usize = 600;
/// The modes of a regular file tree entry.
const REGULAR: [&[u8]; 2] = [b"100644", b"100755"];

/// One `--batch` reply.
enum Reply {
    Missing,
    Object { id: String, kind: String, body: Vec<u8> },
}

/// The bytes of stderr kept in a `failed: <line>` reason.
const WHY_MAX: usize = 200;

/// Why a git read gave no answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitFail {
    Failed { step: &'static str, why: String },
    TooMuch,
}

impl std::fmt::Display for GitFail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GitFail::Failed { step, why } => write!(f, "cannot check: {step} {why}"),
            GitFail::TooMuch => f.write_str("too much to read"),
        }
    }
}

/// A record file in one commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Blob {
    Missing,
    NotRegular,
    Data(Vec<u8>),
}

/// One commit of `git log`, with its paths under `docs/decisions/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogCommit {
    pub id: String,
    pub parents: Vec<String>,
    pub message: String,
    pub paths: Vec<(char, String)>,
}

/// The git reads of one verdict.
pub trait VerdictGit {
    /// The full id of `<rev>^{commit}`; `None` when there is no such commit.
    fn commit(&mut self, rev: &str) -> Result<Option<String>, GitFail>;
    /// The entry `name` of the tree `<rev>:docs/decisions`.
    fn record_blob(&mut self, rev: &str, name: &str) -> Result<Blob, GitFail>;
    /// The merge base of `a` and `b`; `None` when they have none.
    fn merge_base(&mut self, a: &str, b: &str) -> Result<Option<String>, GitFail>;
    /// The commits of `<base>..<head>`, newest first; `None` when there are more than `max`.
    fn rev_list(&mut self, base: &str, head: &str, max: usize) -> Result<Option<Vec<String>>, GitFail>;
    /// The commits of `<base>..<head>` with their paths under `docs/decisions/`.
    fn log(&mut self, base: &str, head: &str) -> Result<Vec<LogCommit>, GitFail>;
}

/// A running request-and-reply process.
pub trait BatchIo {
    fn send(&mut self, line: &[u8]) -> Result<(), RunError>;
    fn read_line(&mut self, max: usize) -> Result<Vec<u8>, RunError>;
    fn read_exact(&mut self, n: usize) -> Result<Vec<u8>, RunError>;
    /// The deadline of every later wait for stdout (D§9.4: each object read has its own bound).
    fn set_deadline(&mut self, deadline: Instant);
}

impl BatchIo for crate::repo::Piped {
    fn send(&mut self, line: &[u8]) -> Result<(), RunError> {
        crate::repo::Piped::send(self, line)
    }
    fn read_line(&mut self, max: usize) -> Result<Vec<u8>, RunError> {
        crate::repo::Piped::read_line(self, max)
    }
    fn read_exact(&mut self, n: usize) -> Result<Vec<u8>, RunError> {
        crate::repo::Piped::read_exact(self, n)
    }
    fn set_deadline(&mut self, deadline: Instant) {
        crate::repo::Piped::set_deadline(self, deadline)
    }
}

/// How git processes run, and the clock their deadlines come from.
pub trait GitRunner {
    fn run(&mut self, cmd: Command, deadline: Instant) -> Result<Whole, RunError>;
    fn batch(&mut self, cmd: Command, deadline: Instant) -> Result<Box<dyn BatchIo>, RunError>;
    fn now(&self) -> Instant;
}

/// The real runner.
pub struct ProcessRunner;

impl GitRunner for ProcessRunner {
    fn run(&mut self, cmd: Command, deadline: Instant) -> Result<Whole, RunError> {
        crate::repo::run_whole(cmd, deadline)
    }
    fn batch(&mut self, cmd: Command, deadline: Instant) -> Result<Box<dyn BatchIo>, RunError> {
        crate::repo::run_piped(cmd, deadline).map(|p| Box::new(p) as Box<dyn BatchIo>)
    }
    fn now(&self) -> Instant {
        Instant::now()
    }
}

/// The git reader of one verdict in the checkout `root`.
pub struct ProcessGit {
    root: PathBuf,
    runner: Box<dyn GitRunner>,
    first: Option<Instant>,
    /// The steps whose one process has started.
    started: Vec<&'static str>,
    batch: Option<Result<Box<dyn BatchIo>, GitFail>>,
    objects: usize,
    bytes: u64,
    /// The reply to `<rev>:docs/decisions`, by rev: each tree is read and counted once.
    trees: HashMap<String, Reply>,
}

impl ProcessGit {
    pub fn new(root: &Path) -> ProcessGit {
        ProcessGit::with_runner(root, Box::new(ProcessRunner))
    }

    pub fn with_runner(root: &Path, runner: Box<dyn GitRunner>) -> ProcessGit {
        ProcessGit {
            root: root.to_path_buf(),
            runner,
            first: None,
            started: Vec::new(),
            batch: None,
            objects: 0,
            bytes: 0,
            trees: HashMap::new(),
        }
    }

    /// The deadline of the next git process of `step`: `min(now + GIT_STEP, first start + GIT_TOTAL)`. Refused, with no
    /// process counted, when a process of `step` or `GIT_PROCESSES` have started, or the total time is over.
    fn deadline(&mut self, step: &'static str) -> Result<Instant, GitFail> {
        if self.started.contains(&step) || self.started.len() == GIT_PROCESSES {
            return Err(fail(step, "more than 4 git processes"));
        }
        let now = self.runner.now();
        let end = *self.first.get_or_insert(now) + GIT_TOTAL;
        if now >= end {
            return Err(fail(step, "over 10 s in all"));
        }
        self.started.push(step);
        Ok((now + GIT_STEP).min(end))
    }

    /// `git <args>` in `root`, with no promisor fetch.
    fn cmd(&self, args: &[&str]) -> Command {
        let mut c = crate::repo::git_cmd(OsStr::new("git"), &self.root, args);
        c.env("GIT_NO_LAZY_FETCH", "1");
        c
    }

    /// The one `--batch` process: spawned at the first object read, with the deadline it got then. Its first failure
    /// is kept, and every later read gives it with no new process.
    fn batch_io(&mut self) -> Result<&mut Box<dyn BatchIo>, GitFail> {
        if self.batch.is_none() {
            let spawned = self.deadline(CAT_FILE).and_then(|deadline| {
                let cmd = self.cmd(&["cat-file", "--batch"]);
                self.runner.batch(cmd, deadline).map_err(|e| fail(CAT_FILE, e.reason()))
            });
            self.batch = Some(spawned);
        }
        match self.batch.as_mut() {
            Some(Ok(io)) => Ok(io),
            Some(Err(e)) => Err(e.clone()),
            None => unreachable!("set above"),
        }
    }

    /// Reads `object` through the one `--batch` process. Every error is kept (the process is dropped), so later reads
    /// give it with no new process.
    fn ask(&mut self, object: &str) -> Result<Reply, GitFail> {
        let reply = self.read_object(object);
        if let Err(e) = &reply {
            self.batch = Some(Err(e.clone()));
        }
        reply
    }

    /// The object and byte caps are checked before any stdout of the object is read: the 2,049th request is not sent,
    /// and a header whose size takes the total past 16 MiB gives `TooMuch` with no body read.
    fn read_object(&mut self, object: &str) -> Result<Reply, GitFail> {
        self.batch_io()?;
        if self.objects == BATCH_OBJECTS {
            return Err(GitFail::TooMuch);
        }
        self.objects += 1;
        let Some(Ok(io)) = self.batch.as_mut() else { unreachable!("batch_io gave a process") };
        let run = |e: RunError| fail(CAT_FILE, e.reason());
        let bad = || fail(CAT_FILE, "bad output");
        io.send(format!("{object}\n").as_bytes()).map_err(run)?;
        let header = io.read_line(HEADER_MAX).map_err(run)?;
        let header = std::str::from_utf8(&header).map_err(|_| bad())?;
        match header.split(' ').collect::<Vec<_>>()[..] {
            [_, "missing" | "ambiguous"] => Ok(Reply::Missing),
            [id, kind, size] => {
                let id = hex_id(id.as_bytes()).ok_or_else(bad)?;
                let size: u64 = size.parse().map_err(|_| bad())?;
                let total = self.bytes.saturating_add(size);
                if total > BATCH_BYTES {
                    return Err(GitFail::TooMuch);
                }
                self.bytes = total;
                let body = io.read_exact(size as usize).map_err(run)?;
                if io.read_exact(1).map_err(run)? != b"\n" {
                    return Err(bad());
                }
                Ok(Reply::Object { id, kind: kind.to_string(), body })
            }
            _ => Err(bad()),
        }
    }

    /// One whole-mode git process of `step`.
    fn whole(&mut self, step: &'static str, args: &[&str]) -> Result<Whole, GitFail> {
        let deadline = self.deadline(step)?;
        let cmd = self.cmd(args);
        self.runner.run(cmd, deadline).map_err(|e| fail(step, e.reason()))
    }
}

fn fail(step: &'static str, why: &str) -> GitFail {
    GitFail::Failed { step, why: why.to_string() }
}

/// `failed: <the first stderr line, escaped, at most 200 bytes>`, or `failed` with no stderr.
fn exit_why(err: &[u8]) -> String {
    let text = String::from_utf8_lossy(err);
    let line = esc(text.lines().next().unwrap_or(""));
    if line.is_empty() {
        return "failed".to_string();
    }
    let mut cut = line.len().min(WHY_MAX);
    while !line.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("failed: {}", &line[..cut])
}

/// A revision git may take as one argument: 1 to 256 printable ASCII bytes, no space, no leading `-`, and not a range:
/// no `..` and no trailing `.` (which would make `<base>..<head>` into `<base>...<head>`).
fn good_rev(rev: &str) -> bool {
    let b = rev.as_bytes();
    (1..=256).contains(&b.len())
        && b[0] != b'-'
        && b.iter().all(|c| (0x21..=0x7e).contains(c))
        && !rev.contains("..")
        && !rev.ends_with('.')
}

/// A file name in one tree: 1 to 255 bytes, no `/`, `\n` or NUL.
fn good_name(name: &str) -> bool {
    (1..=255).contains(&name.len()) && !name.bytes().any(|b| b == b'/' || b == b'\n' || b == 0)
}

/// 40 or 64 bytes of [0-9a-f].
fn hex_id(b: &[u8]) -> Option<String> {
    let ok = matches!(b.len(), 40 | 64) && b.iter().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'));
    ok.then(|| String::from_utf8_lossy(b).into_owned())
}

/// `--format=%H%x00%P%x00%B --name-status -z` output (git 2.50: `<H>\0<P>\0<B>\0\n<S>\0<path>\0…<H>\0…`; the first
/// status of a commit has a leading `\n`, and a commit with no paths is followed directly by the next id). Output
/// that does not end with its closing NUL is malformed.
fn parse_log(out: &[u8]) -> Option<Vec<LogCommit>> {
    let mut t = out.split(|&b| b == 0).peekable();
    let mut v = Vec::new();
    loop {
        let id = t.next()?;
        if id.is_empty() && t.peek().is_none() {
            break; // the NUL that ends the output
        }
        let id = hex_id(id)?;
        let parents = std::str::from_utf8(t.next()?)
            .ok()?
            .split(' ')
            .filter(|p| !p.is_empty())
            .map(|p| hex_id(p.as_bytes()))
            .collect::<Option<Vec<_>>>()?;
        let message = String::from_utf8_lossy(t.next()?).into_owned();
        let mut paths = Vec::new();
        while let Some(s) = t.peek().and_then(|s| status(s)) {
            t.next();
            let p = String::from_utf8_lossy(t.next()?).into_owned();
            if p.starts_with("docs/decisions/") {
                paths.push((s, p));
            }
        }
        v.push(LogCommit { id, parents, message, paths });
    }
    Some(v)
}

/// A name-status token: one ASCII upper-case letter, after one optional leading `\n`.
fn status(tok: &[u8]) -> Option<char> {
    match tok.strip_prefix(b"\n").unwrap_or(tok) {
        [c] if c.is_ascii_uppercase() => Some(*c as char),
        _ => None,
    }
}

/// The entry `name` of a raw tree: `Some(Some((mode, raw id)))`, `Some(None)` when absent, `None` when malformed.
fn find_entry<'a>(mut t: &'a [u8], name: &[u8], id_len: usize) -> Option<Option<(&'a [u8], &'a [u8])>> {
    while !t.is_empty() {
        let sp = t.iter().position(|&b| b == b' ')?;
        let nul = sp + 1 + t[sp + 1..].iter().position(|&b| b == 0)?;
        let end = nul + 1 + id_len;
        if end > t.len() {
            return None;
        }
        if &t[sp + 1..nul] == name {
            return Some(Some((&t[..sp], &t[nul + 1..end])));
        }
        t = &t[end..];
    }
    Some(None)
}

/// `rev-list` output: one id per line.
fn parse_ids(out: &[u8]) -> Option<Vec<String>> {
    if out.is_empty() {
        return Some(Vec::new());
    }
    out.strip_suffix(b"\n")?.split(|&b| b == b'\n').map(hex_id).collect()
}

impl VerdictGit for ProcessGit {
    fn commit(&mut self, rev: &str) -> Result<Option<String>, GitFail> {
        if !good_rev(rev) {
            return Ok(None);
        }
        match self.ask(&format!("{rev}^{{commit}}"))? {
            Reply::Object { id, .. } => Ok(Some(id)),
            Reply::Missing => Ok(None),
        }
    }

    fn record_blob(&mut self, rev: &str, name: &str) -> Result<Blob, GitFail> {
        if !good_rev(rev) || !good_name(name) {
            return Ok(Blob::Missing);
        }
        // The tree is read and its entry mode decides: a link or submodule is never read as a blob (D§6.8). Each
        // rev's tree is read, and counted toward the caps, once.
        if !self.trees.contains_key(rev) {
            let r = self.ask(&format!("{rev}:docs/decisions"))?;
            self.trees.insert(rev.to_string(), r);
        }
        let Some(Reply::Object { id, kind, body }) = self.trees.get(rev) else { return Ok(Blob::Missing) };
        if kind != "tree" {
            return Ok(Blob::Missing);
        }
        let bad = || fail(CAT_FILE, "bad output");
        let (mode, raw) = match find_entry(body, name.as_bytes(), id.len() / 2).ok_or_else(bad)? {
            None => return Ok(Blob::Missing),
            Some(entry) => entry,
        };
        if !REGULAR.contains(&mode) {
            return Ok(Blob::NotRegular);
        }
        let blob: String = raw.iter().map(|b| format!("{b:02x}")).collect();
        match self.ask(&blob)? {
            Reply::Object { kind, body, .. } if kind == "blob" => Ok(Blob::Data(body)),
            _ => Err(bad()),
        }
    }

    fn merge_base(&mut self, a: &str, b: &str) -> Result<Option<String>, GitFail> {
        if !good_rev(a) || !good_rev(b) {
            return Err(fail(MERGE_BASE, "bad revision"));
        }
        let w = self.whole(MERGE_BASE, &["merge-base", a, b])?;
        if w.ok {
            if let Some(id) = w.out.strip_suffix(b"\n").and_then(hex_id) {
                return Ok(Some(id));
            }
        } else if !w.err.is_empty() {
            return Err(GitFail::Failed { step: MERGE_BASE, why: exit_why(&w.err) });
        } else if w.out.is_empty() {
            return Ok(None); // exit 1 with no output: no common ancestor
        }
        Err(fail(MERGE_BASE, "bad output"))
    }

    fn rev_list(&mut self, base: &str, head: &str, max: usize) -> Result<Option<Vec<String>>, GitFail> {
        if !good_rev(base) || !good_rev(head) {
            return Err(fail(REV_LIST, "bad revision"));
        }
        let count = format!("--max-count={}", max.saturating_add(1));
        let range = format!("{base}..{head}");
        let w = self.whole(REV_LIST, &["rev-list", &count, &range, "--"])?;
        if !w.ok {
            return Err(GitFail::Failed { step: REV_LIST, why: exit_why(&w.err) });
        }
        let ids = parse_ids(&w.out).ok_or_else(|| fail(REV_LIST, "bad output"))?;
        Ok((ids.len() <= max).then_some(ids))
    }

    fn log(&mut self, base: &str, head: &str) -> Result<Vec<LogCommit>, GitFail> {
        if !good_rev(base) || !good_rev(head) {
            return Err(fail(LOG, "bad revision"));
        }
        let range = format!("{base}..{head}");
        let args = [
            "log",
            "--no-renames",
            "--diff-merges=first-parent",
            "--name-status",
            "-z",
            "--no-color",
            "--no-show-signature",
            "--ignore-submodules=none",
            "--encoding=UTF-8",
            "--format=%H%x00%P%x00%B",
            &range,
            "--",
            "docs/decisions/",
        ];
        let w = self.whole(LOG, &args)?;
        if !w.ok {
            return Err(GitFail::Failed { step: LOG, why: exit_why(&w.err) });
        }
        parse_log(&w.out).ok_or_else(|| fail(LOG, "bad output"))
    }
}
