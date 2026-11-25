//! The verdict git reader (D§9.4): one `git cat-file --batch`, one `git merge-base`, one `git rev-list` and one
//! `git log`, each bounded, behind an injectable runner (D§17).
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use crate::repo::{RunError, Whole};

/// The deadline of one git process.
pub const GIT_STEP: Duration = Duration::from_secs(2);
/// The deadline of all git processes of one verdict, from the start of the first.
pub const GIT_TOTAL: Duration = Duration::from_secs(10);
/// The git processes one verdict may start.
pub const GIT_PROCESSES: usize = 4;

/// Why a git read gave no answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitFail {
    Failed { step: &'static str, why: String },
    TooMuch,
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
    fn run(&mut self, _cmd: Command, _deadline: Instant) -> Result<Whole, RunError> {
        todo!()
    }
    fn batch(&mut self, _cmd: Command, _deadline: Instant) -> Result<Box<dyn BatchIo>, RunError> {
        todo!()
    }
    fn now(&self) -> Instant {
        todo!()
    }
}

/// The git reader of one verdict in the checkout `root`.
pub struct ProcessGit {
    root: PathBuf,
    runner: Box<dyn GitRunner>,
    first: Option<Instant>,
    started: usize,
    batch: Option<Result<Box<dyn BatchIo>, GitFail>>,
    objects: usize,
    bytes: u64,
}

impl ProcessGit {
    pub fn new(root: &Path) -> ProcessGit {
        ProcessGit::with_runner(root, Box::new(ProcessRunner))
    }

    pub fn with_runner(root: &Path, runner: Box<dyn GitRunner>) -> ProcessGit {
        ProcessGit { root: root.to_path_buf(), runner, first: None, started: 0, batch: None, objects: 0, bytes: 0 }
    }
}

impl VerdictGit for ProcessGit {
    fn commit(&mut self, _rev: &str) -> Result<Option<String>, GitFail> {
        todo!()
    }
    fn record_blob(&mut self, _rev: &str, _name: &str) -> Result<Blob, GitFail> {
        todo!()
    }
    fn merge_base(&mut self, _a: &str, _b: &str) -> Result<Option<String>, GitFail> {
        todo!()
    }
    fn rev_list(&mut self, _base: &str, _head: &str, _max: usize) -> Result<Option<Vec<String>>, GitFail> {
        todo!()
    }
    fn log(&mut self, _base: &str, _head: &str) -> Result<Vec<LogCommit>, GitFail> {
        todo!()
    }
}
