//! `zygo decision` (decision records design D§10.1): delegated decision records in `docs/decisions/`.
use std::ffi::OsString;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use zygo_work::decisions::history::trailer_commits;
use zygo_work::repo;

/// The streams of one `zygo decision` run.
pub struct DecisionIo<'a> {
    pub input: &'a mut dyn BufRead,
    pub out: &'a mut dyn Write,
    pub err: &'a mut dyn Write,
    /// Whether stdin is a terminal.
    pub tty: bool,
}

/// The repository facts `run_in` reads, as values so tests run no git.
pub struct Repo<'a> {
    /// The repository top of the current directory; `None` outside a git repository. Called after the arguments parse.
    pub top: &'a dyn Fn() -> Option<PathBuf>,
    /// `history::trailer_commits` outside tests.
    pub commits: &'a dyn Fn(&Path, &str, usize) -> Result<Vec<(String, String)>, String>,
}

/// One arm per subcommand; gives the exit code.
pub fn run_in(_repo: &Repo, _env: &dyn Fn(&str) -> Option<OsString>, _args: &[String], _io: &mut DecisionIo) -> i32 {
    2
}

/// [`run_in`] with the repository of `cwd` (2 s bound) and the real `git log`.
pub fn run_with(cwd: &Path, env: &dyn Fn(&str) -> Option<OsString>, args: &[String], io: &mut DecisionIo) -> i32 {
    let top = || repo::checkout(cwd, Instant::now() + Duration::from_secs(2)).map(|c| c.top);
    run_in(&Repo { top: &top, commits: &trailer_commits }, env, args, io)
}
