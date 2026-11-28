//! The repository file `.zygo.toml`, read from the default branch (§13.5).

use std::ffi::OsStr;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use toml::{Table, Value};

use crate::schema::Config;

#[derive(Debug, Clone, PartialEq)]
pub enum RepoFile {
    Absent,
    /// A `git` call did not finish: `why` is `git <call>: <reason>` (§13.5 note 2026-11-19). Counts as absent.
    Unread { why: String },
    Invalid { branch: String, why: String },
    Found { branch: String, table: Table, ignored: Vec<String> },
}

/// A multi-line error as one line.
pub(crate) fn one_line(e: &dyn std::fmt::Display) -> String {
    e.to_string().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A TOML syntax error of `text` as `TOML parse error at line L, column C: <message>`. The error's own text quotes the
/// source line, which can hold a secret (C3); only its message and position are kept.
pub fn syntax_error(text: &str, message: &str, span: Option<std::ops::Range<usize>>) -> String {
    let msg = one_line(&message);
    let Some(at) = span.map(|r| r.start.min(text.len())) else { return format!("TOML parse error: {msg}") };
    let before = text.get(..at).unwrap_or(text);
    let line = before.matches('\n').count() + 1;
    let column = before.rsplit('\n').next().unwrap_or("").chars().count() + 1;
    format!("TOML parse error at line {line}, column {column}: {msg}")
}

const KEPT: [&str; 4] = ["guards", "navigation", "limits", "work"];
const GIT_BOUND: Duration = Duration::from_secs(2);
/// §13.5: the largest `.zygo.toml` zygo reads (64 KiB, as `.zygo/workspace.toml` in R§6.1 step 4).
pub const REPO_FILE_MAX: usize = 65536;

/// The stderr messages (under `LC_ALL=C`) of a `git` call that exits non-zero because the thing asked for is not there:
/// no repository, no `origin/HEAD` symbolic ref, no such branch, no `.zygo.toml` in it. Any other failure is a failed read.
pub const GIT_ABSENT: [&str; 5] = [
    "not a git repository (or any",
    "is not a symbolic ref",
    "invalid object name",
    "does not exist in",
    "exists on disk, but not in",
];

/// git's second line when repository discovery stops at a mount point (the first: `not a git repository (or any parent
/// up to mount point <dir>)`).
const BOUNDARY: &str = "Stopping at filesystem boundary (GIT_DISCOVERY_ACROSS_FILESYSTEM not set).";

/// True when `stderr` of a non-zero `git` exit is exactly one line holding one of [`GIT_ABSENT`], or exactly the
/// mount-point pair (that first line, then [`BOUNDARY`]). Any other line (e.g. `warning: ignoring broken ref …`) makes
/// it a failed read (fail closed).
pub fn git_absent(stderr: &str) -> bool {
    let text = stderr.trim_end_matches(['\n', '\r']);
    match text.split_once('\n') {
        None => GIT_ABSENT.iter().any(|m| text.contains(m)),
        Some((first, second)) => {
            first.contains("not a git repository (or any parent up to mount point ") && second.trim_end_matches('\r') == BOUNDARY
        }
    }
}

/// The caller's repository-selecting variables: a `git` child never gets them, so `-C <dir>` alone picks the repository.
pub const GIT_ENV_CLEARED: [&str; 7] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_NAMESPACE",
    "GIT_COMMON_DIR",
];

/// `git` with [`GIT_ENV_CLEARED`] and every `GIT_CONFIG*` variable removed and `LC_ALL=C` (the [`GIT_ABSENT`] messages
/// are matched in English); environment on the child only.
pub fn git_command() -> Command {
    git_command_for(OsStr::new("git"))
}

/// [`git_command`] with `program` in place of `git` (a test's fake `git`).
pub fn git_command_for(program: &OsStr) -> Command {
    let mut c = Command::new(program);
    for k in GIT_ENV_CLEARED {
        c.env_remove(k);
    }
    for (k, _) in std::env::vars_os() {
        if k.to_str().is_some_and(|k| k.starts_with("GIT_CONFIG")) {
            c.env_remove(k);
        }
    }
    c.env("LC_ALL", "C");
    c
}

/// Kills `child`'s process group (unix: `libc::kill(-(child.id() as i32), libc::SIGKILL)`; the child must have been
/// started with `process_group(0)` and not yet reaped) or its process tree (Windows: `taskkill /T /F /PID <id>`,
/// stdout and stderr null; a descendant that left the tree survives). Does not reap.
pub fn kill_group(child: &std::process::Child) {
    #[cfg(unix)]
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/T", "/F", "/PID", &child.id().to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

/// [`kill_group`], then `child.kill()` and `child.wait()`, both results ignored.
pub fn kill_tree(child: &mut std::process::Child) {
    kill_group(child);
    let _ = child.kill();
    let _ = child.wait();
}

/// Whether `child` has exited, without reaping it: with `info` zeroed, `waitid(P_PID, child.id(), &mut info,
/// WEXITED | WNOHANG | WNOWAIT)`. A return of 0 gives `Ok(info.si_pid() != 0)`. A return of -1 gives
/// `Err(std::io::Error::last_os_error())`, e.g. `ECHILD` when SIGCHLD is ignored and the kernel reaps by itself.
#[cfg(unix)]
pub fn exited_unreaped(child: &std::process::Child) -> std::io::Result<bool> {
    // SAFETY: `siginfo_t` is plain data, valid when zeroed; `waitid` writes only into `info`.
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    let r = unsafe {
        libc::waitid(libc::P_PID, child.id() as libc::id_t, &mut info, libc::WEXITED | libc::WNOHANG | libc::WNOWAIT)
    };
    if r == -1 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: `waitid` returned 0, so `info` is either zeroed (no state change) or filled for a child.
    Ok(unsafe { info.si_pid() } != 0)
}

/// Why a `git` call gave no stdout: the thing is absent (git's one stderr line), the read failed (a fixed reason word),
/// or stdout went past the cap (its first `cap + 1` bytes).
enum GitFail {
    Absent(String),
    Read(String),
    Larger(Vec<u8>),
}

/// A finished `git` call: exit status, stdout and up to 64 KiB of stderr.
pub(crate) struct GitOut {
    pub(crate) status: std::process::ExitStatus,
    pub(crate) out: Vec<u8>,
    pub(crate) err: Vec<u8>,
}

/// A `git` call run to its end, or stopped past its stdout cap with the first `cap + 1` bytes of stdout.
pub(crate) enum Ran {
    Done(GitOut),
    Larger(Vec<u8>),
}

/// Runs one `git` call of the repository-file read (`args`, `deadline`, stdout `cap`), as [`run_capped`] does: the
/// real `git`, or a test's fake.
type RunGit<'a> = &'a dyn Fn(&[&str], Instant, Option<usize>) -> Result<Ran, String>;

/// One call through `run`: stdout on success; [`GitFail::Absent`] for a non-zero exit with a [`GIT_ABSENT`] message;
/// [`GitFail::Larger`] past `cap`; else [`GitFail::Read`].
fn git(run: RunGit, args: &[&str], deadline: Instant, cap: Option<usize>) -> Result<String, GitFail> {
    let o = match run(args, deadline, cap).map_err(GitFail::Read)? {
        Ran::Done(o) => o,
        Ran::Larger(head) => return Err(GitFail::Larger(head)),
    };
    if o.status.code() != Some(0) {
        let err = String::from_utf8_lossy(&o.err).into_owned();
        return Err(if git_absent(&err) { GitFail::Absent(err) } else { GitFail::Read("failed".into()) });
    }
    String::from_utf8(o.out).map_err(|_| GitFail::Read("output is not UTF-8".into()))
}

/// The reason of a call whose stdout is above `cap` bytes.
pub(crate) fn output_larger(cap: usize) -> String {
    format!("output larger than {cap} bytes")
}

/// `<git> -C <repo> <args>` (`git`: [`git_command`], or a caller's with more environment set), run to its end: a reason
/// when it does not start, does not finish by `deadline` (the child is killed and its pipe readers, which a grandchild
/// may hold open, are left behind) or its stdout cannot be read. With `cap`, zygo keeps at most `cap` bytes of stdout:
/// past that it stops the read, kills and reaps the child and gives [`output_larger`] (R§6.1 step 4).
pub(crate) fn git_out_with(
    git: Command,
    repo: &Path,
    args: &[&str],
    deadline: Instant,
    cap: Option<usize>,
) -> Result<GitOut, String> {
    match run_capped(git, repo, args, deadline, cap)? {
        Ran::Done(o) => Ok(o),
        Ran::Larger(head) => Err(output_larger(head.len() - 1)),
    }
}

/// [`git_out_with`], with a stdout past `cap` given as [`Ran::Larger`] and its first `cap + 1` bytes.
pub(crate) fn run_capped(
    mut git: Command,
    repo: &Path,
    args: &[&str],
    deadline: Instant,
    cap: Option<usize>,
) -> Result<Ran, String> {
    let timed_out = || "timed out".to_string();
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        return Err(timed_out());
    }
    let mut child = git
        .arg("-C")
        .arg(repo)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| "tool not found".to_string())?;
    let (Some(stdout), Some(mut stderr)) = (child.stdout.take(), child.stderr.take()) else {
        let _ = child.kill();
        let _ = child.wait();
        return Err("failed".into());
    };
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut b = Vec::new();
        // One byte past the cap tells a stdout above it; the read stops there.
        let limit = cap.map_or(u64::MAX, |c| c as u64 + 1);
        let _ = tx.send(stdout.take(limit).read_to_end(&mut b).map(|_| b));
    });
    let (etx, erx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = (&mut stderr).take(64 * 1024).read_to_end(&mut b);
        let _ = std::io::copy(&mut stderr, &mut std::io::sink());
        let _ = etx.send(b);
    });
    let kill = |child: &mut std::process::Child| {
        let _ = child.kill();
        let _ = child.wait();
    };
    let Ok(out) = rx.recv_timeout(left) else {
        kill(&mut child);
        return Err(timed_out());
    };
    let out = match (cap, out) {
        (Some(c), Ok(b)) if b.len() > c => {
            kill(&mut child);
            return Ok(Ran::Larger(b));
        }
        (_, out) => out,
    };
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(5)),
            _ => {
                kill(&mut child);
                return Err(timed_out());
            }
        }
    };
    let err = erx.recv_timeout(deadline.saturating_duration_since(Instant::now())).map_err(|_| timed_out())?;
    Ok(Ran::Done(GitOut { status, out: out.map_err(|_| "failed".to_string())?, err }))
}

/// `git show` said `.zygo.toml` or the branch is absent (`said`, git's one stderr line); a corrupt object or ref can say
/// the same, so a second call under the same deadline confirms it. `None`: absent; `Some(reason)`: a failed read.
fn confirm_absent(run: RunGit, branch: &str, said: &str, deadline: Instant) -> Option<String> {
    let (args, absent): (Vec<&str>, fn(&GitOut) -> bool) = if said.contains("invalid object name") {
        // No such ref: exit 1, nothing printed. A ref to a missing object resolves (exit 0); a broken one warns.
        (vec!["rev-parse", "--verify", "--quiet", branch], |o| o.status.code() == Some(1) && o.out.is_empty() && o.err.is_empty())
    } else if said.contains("does not exist in") || said.contains("exists on disk, but not in") {
        // The branch's tree is readable and has no `.zygo.toml`: exit 0, nothing printed.
        (vec!["ls-tree", "--name-only", branch, "--", ".zygo.toml"], |o| o.status.code() == Some(0) && o.out.is_empty() && o.err.is_empty())
    } else {
        // `not a git repository`: `repo` is no repository (the hooks find the checkout first and judge a `.git` entry).
        return None;
    };
    match run(&args, deadline, None) {
        Ok(Ran::Done(o)) if absent(&o) => None,
        Ok(_) => Some("failed".into()),
        Err(w) => Some(w),
    }
}

/// The `git` calls share one 2 s budget.
pub fn read_default_branch(repo: &Path) -> RepoFile {
    read_with(&|args, deadline, cap| run_capped(git_command(), repo, args, deadline, cap), Instant::now() + GIT_BOUND)
}

/// [`read_default_branch`] with `run` running each `git` call, every call by the one `deadline`.
fn read_with(run: RunGit, deadline: Instant) -> RepoFile {
    let branch = match git(run, &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"], deadline, None) {
        Ok(s) if !s.trim().is_empty() => s.trim().to_string(),
        Ok(_) | Err(GitFail::Absent(_)) => "main".into(),
        Err(GitFail::Read(w)) => return RepoFile::Unread { why: format!("git symbolic-ref: {w}") },
        // No cap on this call: never reached.
        Err(GitFail::Larger(head)) => {
            return RepoFile::Unread { why: format!("git symbolic-ref: {}", output_larger(head.len() - 1)) };
        }
    };
    // §13.5: `git show` of a tree prints `tree <rev>\n\n` and the names in it (exit 0). No `.zygo.toml` file starts
    // with that line (it is not TOML), so the header tells a tree without another git call.
    let tree = format!("tree {branch}:.zygo.toml\n\n");
    let not_a_file = |branch: String| RepoFile::Invalid { branch, why: ".zygo.toml is not a file".into() };
    // §13.5: the read stops one byte past REPO_FILE_MAX and kills the call; the file is then invalid.
    let text = match git(run, &["show", &format!("{branch}:.zygo.toml")], deadline, Some(REPO_FILE_MAX)) {
        Ok(t) if t.starts_with(&tree) => return not_a_file(branch),
        Ok(t) => t,
        Err(GitFail::Larger(head)) if head.starts_with(tree.as_bytes()) => return not_a_file(branch),
        Err(GitFail::Larger(_)) => {
            return RepoFile::Invalid { branch, why: format!(".zygo.toml is larger than {REPO_FILE_MAX} bytes") };
        }
        Err(GitFail::Absent(said)) => match confirm_absent(run, &branch, &said, deadline) {
            None => return RepoFile::Absent,
            Some(w) => return RepoFile::Unread { why: format!("git show: {w}") },
        },
        Err(GitFail::Read(w)) => return RepoFile::Unread { why: format!("git show: {w}") },
    };
    from_text(branch, &text)
}

/// The repository file read from `branch` with contents `text`: parsed, filtered to the kept tables and validated.
pub fn from_text(branch: String, text: &str) -> RepoFile {
    let invalid = |why: String| RepoFile::Invalid { branch: branch.clone(), why };
    let parsed: Table = match text.parse() {
        Ok(t) => t,
        Err(e) => return invalid(syntax_error(text, e.message(), e.span())),
    };
    let mut table = Table::new();
    let mut ignored = Vec::new();
    for (k, v) in parsed {
        if k == "version" {
            if v.as_integer() != Some(1) {
                return invalid("version is not 1".into());
            }
            table.insert(k, v);
        } else if KEPT.contains(&k.as_str()) {
            table.insert(k, v);
        } else {
            ignored.push(k);
        }
    }
    let mut kept = table.clone();
    kept.remove("version");
    // `load` drops `guards.min_level` and `limits.max_members` with a warning; a bad value must not invalidate the file.
    if let Some(g) = kept.get_mut("guards").and_then(Value::as_table_mut) {
        g.remove("min_level");
    }
    if let Some(l) = kept.get_mut("limits").and_then(Value::as_table_mut) {
        l.remove("max_members");
    }
    if let Err(e) = crate::typed::from_value::<Config>(Value::Table(kept)) {
        return invalid(e.describe(Some(text)));
    }
    RepoFile::Found { branch, table, ignored }
}

#[cfg(all(test, unix))]
mod process_tests {
    use super::*;
    use std::io::BufRead;
    use std::os::unix::process::CommandExt;

    /// No process `pid`, or a zombie not yet reaped by init.
    fn gone(pid: &str) -> bool {
        let o = Command::new("ps").args(["-o", "stat=", "-p", pid]).output().unwrap();
        let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
        s.is_empty() || s.starts_with('Z')
    }

    #[test] // kill_group ends the child's whole group and does not reap the child
    #[ignore = "timing: polls for the grandchild exit with a 2 s bound"]
    fn kill_group_kills_the_childs_group() {
        let mut c = Command::new("sh");
        c.args(["-c", "sleep 30 & echo $!; wait"]).stdin(Stdio::null()).stdout(Stdio::piped());
        c.process_group(0);
        let mut child = c.spawn().unwrap();
        let mut line = String::new();
        std::io::BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).unwrap();
        let grandchild = line.trim().to_string();
        assert!(!grandchild.is_empty() && !gone(&grandchild), "grandchild {grandchild:?} did not start");
        kill_group(&child);
        let status = child.wait().expect("kill_group must not reap the child");
        assert_eq!(std::os::unix::process::ExitStatusExt::signal(&status), Some(libc::SIGKILL));
        let until = Instant::now() + Duration::from_secs(2);
        while !gone(&grandchild) && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(gone(&grandchild), "grandchild {grandchild} still runs");
    }

    #[test] // R§6.1 step 4 (FX-39 D3): a call past the stdout cap is stopped with the reason; one at the cap is read
    fn git_out_with_stops_a_call_past_the_stdout_cap() {
        let far = Instant::now() + Duration::from_secs(600);
        let dir = std::env::temp_dir();
        // `sh -c <script> sh -C <dir> x`: the script runs, `-C <dir> x` are its unused arguments.
        let sh = |script: &str| {
            let mut c = Command::new("sh");
            c.args(["-c", script, "sh"]);
            c
        };
        // `yes` writes lines with no end: only the cap ends the call.
        assert_eq!(git_out_with(sh("yes"), &dir, &["x"], far, Some(4096)).err(), Some("output larger than 4096 bytes".into()));
        // Exactly the cap is read; one byte more is not.
        let o = git_out_with(sh("printf abc"), &dir, &["x"], far, Some(3)).unwrap();
        assert_eq!(o.out, b"abc");
        assert_eq!(git_out_with(sh("printf abc"), &dir, &["x"], far, Some(2)).err(), Some(output_larger(2)));
    }

    /// A fake `git` for [`read_with`]: `sh -c <script> sh -C <dir> <call> <arg>...` run by [`run_capped`] with `envs`
    /// set, so `$3` is the git call and `$4` its first argument.
    fn fake<'a>(script: &'a str, dir: &'a Path, envs: Vec<(&'a str, std::path::PathBuf)>) -> impl Fn(&[&str], Instant, Option<usize>) -> Result<Ran, String> + 'a {
        move |args: &[&str], deadline: Instant, cap: Option<usize>| {
            let mut c = Command::new("sh");
            c.args(["-c", script, "sh"]);
            for (k, v) in &envs {
                c.env(k, v);
            }
            run_capped(c, dir, args, deadline, cap)
        }
    }

    fn far() -> Instant {
        Instant::now() + Duration::from_secs(600)
    }

    fn larger() -> RepoFile {
        RepoFile::Invalid { branch: "main".into(), why: format!(".zygo.toml is larger than {REPO_FILE_MAX} bytes") }
    }

    #[test] // §13.5 (FX-43): a `.zygo.toml` above the cap is invalid; the read stops one byte past the cap and the
            // call is killed and reaped
    fn repo_file_read_stops_past_the_cap() {
        let dir = tempfile::tempdir().unwrap();
        let mark = dir.path().join("show-finished");
        let pid_file = dir.path().join("show-pid");
        // `show` writes its pid, ignores SIGPIPE, writes 16384 lines of 64 bytes (1 MiB), waits 1 s, then writes the
        // marker. Once the read stops, its writes fail and it goes on: only the kill keeps the marker from being written.
        let script = r#"case "$3" in
            symbolic-ref) echo main ;;
            show) echo $$ > "$FX43_PID"; trap '' PIPE
                  i=0; while [ $i -lt 16384 ]; do printf '%063d\n' 0; i=$((i+1)); done 2>/dev/null
                  sleep 1; : > "$FX43_MARK" ;;
            *) exit 2 ;;
        esac"#;
        let run = fake(script, dir.path(), vec![("FX43_MARK", mark.clone()), ("FX43_PID", pid_file.clone())]);
        assert_eq!(read_with(&run, far()), larger());
        let pid = std::fs::read_to_string(&pid_file).unwrap().trim().to_string();
        assert!(!pid.is_empty(), "the fake show wrote no pid");
        // Reaped: no /proc entry. A running call or an unreaped zombie still has one.
        #[cfg(target_os = "linux")]
        {
            let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"));
            assert!(stat.is_err(), "the show call {pid} was not killed and reaped: {stat:?}");
        }
        std::thread::sleep(Duration::from_millis(1500));
        assert!(!mark.exists(), "the show call ran to its end: it was not killed");
    }

    #[test] // §13.5 (FX-43): exactly REPO_FILE_MAX bytes of `.zygo.toml` are read as before; one byte more is invalid
    fn repo_file_cap_boundary() {
        let dir = tempfile::tempdir().unwrap();
        let blob = dir.path().join("blob");
        let script = r#"case "$3" in
            symbolic-ref) echo main ;;
            show) cat "$FX43_BLOB" ;;
            *) exit 2 ;;
        esac"#;
        let run = fake(script, dir.path(), vec![("FX43_BLOB", blob.clone())]);
        let head = "version = 1\n[guards]\nclaims = \"enforce\"\n";
        let pad = |len: usize| format!("{head}#{}\n", "x".repeat(len - head.len() - 2));
        std::fs::write(&blob, pad(REPO_FILE_MAX)).unwrap();
        assert_eq!(std::fs::metadata(&blob).unwrap().len(), REPO_FILE_MAX as u64);
        match read_with(&run, far()) {
            RepoFile::Found { branch, table, ignored } => {
                assert_eq!(branch, "main");
                assert_eq!(table["guards"]["claims"].as_str(), Some("enforce"));
                assert!(ignored.is_empty());
            }
            other => panic!("{other:?}"),
        }
        std::fs::write(&blob, pad(REPO_FILE_MAX + 1)).unwrap();
        assert_eq!(read_with(&run, far()), larger());
    }

    #[test] // §13.5 (FX-43): a tree at `.zygo.toml` is invalid as not a file, whatever the size of its listing
    fn repo_file_tree_is_not_a_file() {
        let dir = tempfile::tempdir().unwrap();
        // `git show <rev>` of a tree: `tree <rev>`, an empty line, one name per line; exit 0.
        let script = r#"case "$3" in
            symbolic-ref) echo main ;;
            show) printf 'tree %s\n\n' "$4"; i=0; while [ $i -lt "$FX43_N" ]; do printf 'entry-%057d\n' $i; i=$((i+1)); done ;;
            *) exit 2 ;;
        esac"#;
        let want = RepoFile::Invalid { branch: "main".into(), why: ".zygo.toml is not a file".into() };
        // 2 names (a small listing) and 4096 names of 64 bytes (256 KiB, past the cap).
        for n in ["2", "4096"] {
            let run = fake(script, dir.path(), vec![("FX43_N", n.into())]);
            assert_eq!(read_with(&run, far()), want, "{n} names");
        }
    }

    #[test] // §13.5 note 2026-11-19 (FX-43 review F1): every git call of the read gets the one deadline of the read
    fn repo_file_calls_share_one_deadline() {
        use std::os::unix::process::ExitStatusExt;
        let done = |code: i32, out: &str, err: &str| {
            Ok(Ran::Done(GitOut { status: std::process::ExitStatus::from_raw(code << 8), out: out.into(), err: err.into() }))
        };
        let seen = std::cell::RefCell::new(Vec::new());
        // No origin/HEAD target, no `.zygo.toml` in `main`, confirmed by `ls-tree`: three calls.
        let run = |args: &[&str], deadline: Instant, _cap: Option<usize>| {
            seen.borrow_mut().push((args[0].to_string(), deadline));
            match args[0] {
                "symbolic-ref" => done(0, "main\n", ""),
                "show" => done(128, "", "fatal: path '.zygo.toml' does not exist in 'main'\n"),
                "ls-tree" => done(0, "", ""),
                other => panic!("unexpected git call {other}"),
            }
        };
        let deadline = far();
        assert_eq!(read_with(&run, deadline), RepoFile::Absent);
        let seen = seen.into_inner();
        assert_eq!(seen.iter().map(|(c, _)| c.as_str()).collect::<Vec<_>>(), ["symbolic-ref", "show", "ls-tree"]);
        assert!(seen.iter().all(|(_, d)| *d == deadline), "a call got its own deadline: {seen:?}");
    }

    #[test] // §13.5 note 2026-11-19 (FX-43 review F1): the calls share the budget, so a slow first call leaves the
            // second call only the rest of it
    #[ignore = "timing: two fake git calls of 0.6 s each against one 1 s deadline"]
    fn repo_file_calls_share_one_budget() {
        let dir = tempfile::tempdir().unwrap();
        let script = r#"case "$3" in
            symbolic-ref) sleep 0.6; echo main ;;
            show) sleep 0.6; printf 'version = 1\n' ;;
            *) exit 2 ;;
        esac"#;
        let run = fake(script, dir.path(), Vec::new());
        // A fresh 1 s per call would give Found; the shared 1 s leaves `show` 0.4 s.
        let r = read_with(&run, Instant::now() + Duration::from_secs(1));
        assert_eq!(r, RepoFile::Unread { why: "git show: timed out".into() });
    }

    #[test] // exited_unreaped: false while the child runs, true after it exits, and the status stays for wait
    #[ignore = "timing: polls for the child exit with a 5 s bound"]
    fn exited_unreaped_tells_an_exit_without_reaping() {
        let mut running = Command::new("sleep").arg("30").spawn().unwrap();
        assert!(!exited_unreaped(&running).unwrap());
        let _ = running.kill();
        let _ = running.wait();

        let mut child = Command::new("sh").args(["-c", "exit 3"]).spawn().unwrap();
        let until = Instant::now() + Duration::from_secs(5);
        while !exited_unreaped(&child).unwrap() {
            assert!(Instant::now() < until, "child did not exit");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(exited_unreaped(&child).unwrap(), "a second look must still see the exit: not reaped");
        assert_eq!(child.try_wait().unwrap().and_then(|s| s.code()), Some(3));
    }
}
