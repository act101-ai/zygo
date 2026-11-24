# MS-1 — Bounded git runner: whole mode and stream mode, reachable outside zygo-work

**Spec:** `docs/specs/2026-11-24-mail-sync-design.md` S§8.7; S§17 row `crates/zygo-work/src/repo.rs:61-75`
**Files:** `crates/zygo-work/src/repo.rs`, `crates/zygo-work/tests/git_runner.rs`
**Consumes:** —
**Produces:** `zygo_work::repo::RunError` (now `pub`), `zygo_work::repo::Whole { pub ok: bool, pub out: Vec<u8>, pub err: Vec<u8> }`, `zygo_work::repo::run_whole(cmd: std::process::Command, deadline: std::time::Instant) -> Result<Whole, RunError>`, `zygo_work::repo::Flow` (enum: `Go`, `Stop`), `zygo_work::repo::Streamed` (enum: `Done { ok: bool, err: Vec<u8> }`, `Stopped`), `zygo_work::repo::run_stream(cmd: std::process::Command, deadline: std::time::Instant, each: &mut dyn FnMut(&[u8]) -> Flow) -> Result<Streamed, RunError>`
**Acceptance:** `cargo test -p zygo-work --test git_runner && cargo test -p zygo-work` → all pass: a non-zero exit returns its stdout and at most 64 KiB of stderr; a zero exit returns stdout and empty stderr; `run_stream` passes each NUL-ended entry of `git ls-tree -z` once, and after `Flow::Stop` returns `Stopped` with the git process gone (unix: its process group killed); a deadline already past returns `TimedOut` with no spawn; both modes run with stdin null and (unix) in their own process group; every existing zygo-work test still passes. No test waits on wall-clock time.

## Plan

**Premises checked against the live code (branch `phase3/MS-1` at `737892a`):**

- `crates/zygo-work/src/repo.rs` (365 lines) is reachable as `zygo_work::repo` (`crates/zygo-work/src/lib.rs:10`, `pub mod repo;`). `RunError` is `pub(crate)` (`repo.rs:25-32`; variants `NotRun`, `TimedOut`, `Failed`, `Absent`; derives `Debug, Clone, Copy, PartialEq, Eq`); `RunError::reason` is `pub(crate)` (`repo.rs:35`) and stays so (DR-3 uses it inside the crate).
- The runner is private: `fn run_bounded_out(mut cmd: Command, deadline: Instant, cut: Option<u64>) -> Result<Ran, RunError>` (`repo.rs:75`), returning `struct Ran { ok: bool, out: Vec<u8>, cut: bool, err: Vec<u8> }` (`repo.rs:64-69`). Its callers: `run_bounded_err` (`repo.rs:54`, with `cut: None`) and `hud_git_with` (`repo.rs:265`, with `Some(HUD_CUT)`). `run_bounded` (`repo.rs:46`) is called from `crates/zygo-work/src/sources.rs:52`. None of these signatures change.
- **Premise drift (spec S§8.7 vs code).** S§8.7 says both modes run "own process group, killed at the deadline (as `repo.rs:71-75`)". In the code only the cut mode (`cut: Some(_)`) does that: `process_group(0)` at `repo.rs:80-83`, the group kill before reaping at `repo.rs:117-136`, the Windows tree kill at `repo.rs:146-147`. The plain mode (`cut: None`, used by `run_bounded_err`) has no group and kills only the child (`repo.rs:149-150`). So `run_whole` calls `run_bounded_out` in cut mode with a cut of `u64::MAX` (no byte is ever cut: the reader's limit is `u64::MAX.saturating_add(1) == u64::MAX`, `repo.rs:92`, and `buf.len() as u64 > u64::MAX` is never true, `repo.rs:97`).
- **Second drift, kept.** In cut mode with SIGCHLD ignored the kernel reaps the child, the status is unknown and `ok` is `true` (`repo.rs:112`, `:144`, `:159-160`). `run_whole` and `run_stream` take this rule as it is: zygo does not ignore SIGCHLD, and consumers read stdout (`push --porcelain`) before `ok`.
- Process-group helpers exist and are reused, not rebuilt (`crates/zygo-config/src/repo_file.rs`): `pub fn kill_group(child: &std::process::Child)` (`:104`; unix `kill(-pid, SIGKILL)`, Windows `taskkill /T /F`; does not reap), `pub fn kill_tree(child: &mut std::process::Child)` (`:120`; `kill_group`, `kill`, `wait`), `#[cfg(unix)] pub fn exited_unreaped(child: &std::process::Child) -> std::io::Result<bool>` (`:130`). The `zygo-workspace` and `zygo-mcp` runners named `run_bounded` (`crates/zygo-workspace/src/mux/tmux.rs:60`, `crates/zygo-mcp/src/tools/workspace_down.rs:149`) are tmux/tool runners with other error types; not used.
- `libc` is a `cfg(unix)` dependency of zygo-work (`crates/zygo-work/Cargo.toml`), so the integration test can call `libc::fcntl`; `tempfile` is a dev-dependency. No `[lints]` table and no clippy step in CI (`.github/workflows/ci.yml:45` runs `cargo test --workspace --locked --no-fail-fast` on ubuntu, macos, windows).
- Facts the tests rely on, checked with git on this machine: `git ls-tree -z HEAD` ends every entry with NUL (`100644 blob <id>\t<name>\0`); `git push --porcelain` of a non-fast-forward to a local bare repository prints `!\trefs/heads/main:refs/heads/main\t[rejected] (non-fast-forward)` on stdout, `error: failed to push some refs to '<path>'` on stderr, exit 1; `git checkout -b x` prints `Switched to a new branch 'x'` on stderr only and exits 0; `git hash-object --stdin` with empty stdin prints `e69de29bb2d1d6434b8b29ae775ad8c2e48c5391`; `git check-attr --stdin -z -a` given `a\0` with `.gitattributes` `a foo=bar` prints `a\0foo\0bar\0`, given empty stdin prints nothing. The FIFO proof of step 13 was tried: after a group kill the blocking read gives EOF; after a kill of the leader alone it hangs.

**Consumers found** (`grep -ln "run_whole\|run_stream\|Streamed\|RunError\|Flow::" docs/work/phase3/nodes/*.md`): MS-2 (`run_whole`, `run_stream`, `Whole`, `Flow`, `Streamed`, `RunError`; wraps them as `git::run` / `git::stream` with the same callback type `&mut dyn FnMut(&[u8]) -> Flow`), MS-8 (`Whole`, `RunError`; `classify(w: &Whole)` reads `ok`, `out`, `err`), MS-10 (`RunError`, `Flow`, `Streamed`, `Whole`; `BranchError::Git(RunError, &'static str)` needs `RunError: Debug + Clone`), DR-11 (`run_whole`, `Whole`, `RunError`; its stub `GitRunner` builds `Whole` values and adds its own stdin form in `repo.rs`, not this node). DR-3 names `RunError::reason` as `pub(crate)`; unchanged.

**Decisions (author):**

- `Whole` and `Streamed` derive `Debug, Clone, PartialEq, Eq`; `Flow` derives `Debug, Clone, Copy, PartialEq, Eq`. Tests compare whole values; MS-8 and DR-11 build `Whole` literals.
- The stream callback gets each entry **without** its NUL. A last piece that has no NUL (output that does not end in NUL) is not passed: S§8.7 passes NUL-ended entries only.
- `Streamed::Done.err` follows the whole mode: up to 64 KiB of stderr on a non-zero exit, empty on success.
- The shared parts of `run_bounded_out` (spawn with pipes, stderr drain, the wait loop) move to three private functions that both runners call, so no code is copied. `hud_git` tests and `find_checkout_tells_timeout_from_not_a_repository` guard the move.
- No test of the stream deadline while running: it needs a wall-clock wait. The deadline check before spawn is tested (an `Instant` already past); the in-run deadline path uses the same `stop` as `Flow::Stop`.

**Interfaces.** Consumes: — (reused existing code: `run_bounded_out`, `Ran` at `repo.rs:64-75`; `zygo_config::repo_file::{kill_group, kill_tree, exited_unreaped}` above). Produces (`crates/zygo-work/src/repo.rs`, signatures as in the contract):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunError { NotRun, TimedOut, Failed, Absent }          // was pub(crate); variants unchanged

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Whole { pub ok: bool, pub out: Vec<u8>, pub err: Vec<u8> }
pub fn run_whole(cmd: std::process::Command, deadline: std::time::Instant) -> Result<Whole, RunError>

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow { Go, Stop }
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Streamed { Done { ok: bool, err: Vec<u8> }, Stopped }
pub fn run_stream(cmd: std::process::Command, deadline: std::time::Instant,
                  each: &mut dyn FnMut(&[u8]) -> Flow) -> Result<Streamed, RunError>
```

Spec-fixed values: stderr kept up to 64 KiB (`64 * 1024`, as `repo.rs:108`), stdin null, own process group on unix (`process_group(0)`), group killed on `Flow::Stop` and at the deadline, `TimedOut` with no spawn when `Instant::now() >= deadline`.

### Steps

All commands run in `/Users/kagemartin/Work/zygo/.worktrees/phase3/MS-1`. Each commit message starts with `MS-1:` and ends with the trailers your brief gives, each on its own line; no Claude attribution.

**Part A — move the shared parts of the runner (no behaviour change).**

- [ ] **Step 1 — baseline.** Run `cargo test -p zygo-work`. Result: all pass. Write down the pass count.
- [ ] **Step 2 — extract three private functions in `crates/zygo-work/src/repo.rs`.** Move code out of `run_bounded_out` (`repo.rs:75-163`) into these, and make `run_bounded_out` call them; its signature and results do not change. Add `use std::process::{Child, ChildStderr, ChildStdout, ExitStatus};` to the `std::process` import.
  - `fn spawn_piped(mut cmd: Command, deadline: Instant, group: bool) -> Result<(Child, ChildStdout, ChildStderr), RunError>`: the body of `repo.rs:76-89` (deadline check before spawn → `TimedOut`; stdin null, stdout and stderr piped; `#[cfg(unix)] if group { process_group(0) }`; spawn error → `NotRun`; missing pipe → kill, wait, `Failed`). Add `#[cfg(not(unix))] let _ = group;` so Windows builds with no unused-variable warning. `run_bounded_out` calls it with `group = cut.is_some()`.
  - `fn drain_err(mut err: ChildStderr) -> mpsc::Receiver<Vec<u8>>`: the thread of `repo.rs:105-111` (keeps `64 * 1024` bytes, drains the rest, sends the buffer).
  - `fn wait_bounded(child: &mut Child, deadline: Instant, group: bool) -> Result<Option<ExitStatus>, RunError>`: the `'wait` block of `repo.rs:112-156`, with `cut.is_some()` replaced by `group`, `break 'wait Some(s)` replaced by `return Ok(Some(s))`, the plain loop's `break Some(s)` / `break None` replaced by `return Ok(Some(s))` / `return Ok(None)`, and `kill_tree(&mut child)` replaced by `kill_tree(child)`. The `'wait` label and its `#[cfg_attr(not(unix), allow(unused_labels))]` go away; the `Err(_) => break` of the unix loop still leaves that inner loop for the plain loop. Keep the comments of the moved lines.
  - Check: `cargo build -p zygo-work` has no warning that names `repo.rs`.
- [ ] **Step 3 — run the guard.** Run `cargo test -p zygo-work`. Result: all pass, same count as step 1.
- [ ] **Step 4 — commit.** `git add crates/zygo-work/src/repo.rs && git commit -m "MS-1: move spawn, stderr drain and wait out of run_bounded_out"` (plus trailers).

**Part B — whole mode.**

- [ ] **Step 5 — write the failing whole-mode tests.** Create `crates/zygo-work/tests/git_runner.rs` with exactly this content:

```rust
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
```

- [ ] **Step 6 — see it fail.** Run `cargo test -p zygo-work --test git_runner`. Result: compile error `unresolved imports` / `private` for `run_whole`, `RunError`, `Whole`.
- [ ] **Step 7 — implement whole mode in `crates/zygo-work/src/repo.rs`.**
  - `pub(crate) enum RunError` (`repo.rs:26`) → `pub enum RunError`. Leave `pub(crate) fn reason` as it is.
  - After `struct Ran` add:
    ```rust
    /// The end of a [`run_whole`] that exited: exit success, the whole stdout, and up to 64 KiB of stderr on a
    /// non-zero exit (else empty).
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Whole { pub ok: bool, pub out: Vec<u8>, pub err: Vec<u8> }

    /// The cut of [`run_whole`]: no byte is ever cut; the value selects the process-group mode of `run_bounded_out`.
    const WHOLE: u64 = u64::MAX;

    /// Runs `cmd` (S§8.7 whole mode) with stdin null, in its own process group on unix (its tree on Windows), until it
    /// exits or `deadline`: `TimedOut` before any spawn when the deadline is past, and at the deadline (group killed).
    pub fn run_whole(cmd: Command, deadline: Instant) -> Result<Whole, RunError> {
        run_bounded_out(cmd, deadline, Some(WHOLE)).map(|Ran { ok, out, err, .. }| Whole { ok, out, err })
    }
    ```
- [ ] **Step 8 — see it pass.** Run `cargo test -p zygo-work --test git_runner`. Result: 4 tests pass on any OS, 6 on unix (`whole_zero_exit_gives_stdout_and_empty_stderr`, `whole_non_zero_exit_gives_stdout_and_stderr`, `whole_non_zero_exit_keeps_64_kib_of_stderr`, `whole_past_deadline_times_out_with_no_spawn`, `whole_runs_with_stdin_null`, `whole_runs_in_its_own_process_group`). Then `cargo test -p zygo-work`: all pass.
- [ ] **Step 9 — commit.** `git add crates/zygo-work/src/repo.rs crates/zygo-work/tests/git_runner.rs && git commit -m "MS-1: pub RunError, Whole and run_whole"` (plus trailers).

**Part C — stream mode.**

- [ ] **Step 10 — write the failing stream-mode tests.** In `crates/zygo-work/tests/git_runner.rs`, change the import line to `use zygo_work::repo::{run_stream, run_whole, Flow, RunError, Streamed, Whole};` and append:

```rust
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

/// After `Flow::Stop` the whole process group is gone, proven with no timer: a grandchild holds the only write end
/// of a FIFO, and a blocking read of the FIFO ends (EOF) only when that grandchild is dead. A runner that kills only
/// the leader makes the read hang until the CI job timeout; it cannot pass. `sleep 1000000` is the blocker that Stop
/// must kill, not a wait of the test.
#[cfg(unix)]
#[test]
fn stream_stop_kills_the_process_group() {
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
    c.args(["-c", "( exec 3>\"$1\"; printf 'go\\000'; exec sleep 1000000 ) & exec sleep 1000000", "sh"]).arg(&fifo);
    let mut seen = Vec::new();
    let r = run_stream(c, far(), &mut |e| {
        seen.push(e.to_vec());
        Flow::Stop
    });
    assert_eq!(r, Ok(Streamed::Stopped));
    assert_eq!(seen, [b"go".to_vec()]);
    assert_eq!(unsafe { libc::fcntl(rd.as_raw_fd(), libc::F_SETFL, 0) }, 0); // now blocking
    let mut rest = Vec::new();
    assert_eq!(rd.read_to_end(&mut rest).unwrap(), 0);
}
```

- [ ] **Step 11 — see it fail.** Run `cargo test -p zygo-work --test git_runner`. Result: compile error `unresolved imports` for `run_stream`, `Flow`, `Streamed`.
- [ ] **Step 12 — implement stream mode in `crates/zygo-work/src/repo.rs`.** Add `use std::io::BufRead;`. Add after `run_whole`:
  ```rust
  /// What the [`run_stream`] callback wants next.
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub enum Flow { Go, Stop }

  /// The end of a [`run_stream`]: the process exited (exit success; up to 64 KiB of stderr on a non-zero exit, else
  /// empty), or the callback said stop and the process (unix: its process group) was killed and reaped.
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub enum Streamed { Done { ok: bool, err: Vec<u8> }, Stopped }

  /// Kills `child` with its process group (unix) or tree (Windows) and reaps it. Unix: when `waitid` fails (SIGCHLD
  /// ignored, the kernel reaps), only the child is killed, because its PID may be reused.
  fn stop(child: &mut Child)

  /// Runs `cmd` (S§8.7 stream mode) with stdin null, in its own process group on unix: each NUL-ended stdout entry,
  /// without its NUL, goes to `each`; a last piece with no NUL is not passed. `Flow::Stop` kills the group and gives
  /// `Stopped`. `TimedOut` before any spawn when the deadline is past, and at the deadline (group killed). Memory is
  /// bounded by a few entries.
  pub fn run_stream(cmd: Command, deadline: Instant, each: &mut dyn FnMut(&[u8]) -> Flow) -> Result<Streamed, RunError>
  ```
  `stop` body: `#[cfg(unix)] if zygo_config::repo_file::exited_unreaped(child).is_err() { let _ = child.kill(); let _ = child.wait(); return; }` then `zygo_config::repo_file::kill_tree(child);`.
  `run_stream` algorithm (the callback is not `Send`, so it runs on the calling thread):
  1. `let (mut child, out, err) = spawn_piped(cmd, deadline, true)?; let erx = drain_err(err);`
  2. Reader thread with `let (tx, rx) = mpsc::sync_channel::<Vec<u8>>(1);`: on `std::io::BufReader::new(out)`, loop `read_until(0, &mut entry)`; `Ok(0)` or `Err(_)` → end; an entry that ends in `0` → pop the NUL and `tx.send(entry)`, end when the send fails (receiver dropped after Stop); an entry with no trailing NUL → end without sending.
  3. Loop on `rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))`: `Ok(e)` → if `each(&e) == Flow::Stop` { `stop(&mut child); return Ok(Streamed::Stopped)` }; `Err(RecvTimeoutError::Timeout)` → `stop(&mut child); return Err(RunError::TimedOut)`; `Err(RecvTimeoutError::Disconnected)` → leave the loop.
  4. `let status = wait_bounded(&mut child, deadline, true)?; let ok = status.is_none_or(|s| s.success());` then `err` as `run_bounded_out` does (`erx.recv_timeout(left).unwrap_or_default()` only when `!ok`); return `Ok(Streamed::Done { ok, err })`.
- [ ] **Step 13 — see it pass.** Run `cargo test -p zygo-work --test git_runner`. Result: all pass — 9 tests on any OS, 13 on unix (step 8's plus `stream_passes_each_nul_ended_entry_of_ls_tree_once`, `stream_non_zero_exit_gives_stderr`, `stream_stop_returns_stopped_after_one_entry`, `stream_past_deadline_times_out_with_no_spawn`, `stream_runs_with_stdin_null`, `stream_runs_in_its_own_process_group`, `stream_stop_kills_the_process_group`). If `stream_stop_kills_the_process_group` hangs, the group was not killed: fix `stop`, do not add a timeout. Then run `pgrep -f "sleep 1000000"`: no process.
- [ ] **Step 14 — acceptance.** Run `cargo test -p zygo-work --test git_runner && cargo test -p zygo-work`. Result: all pass; the zygo-work pass count is step 1's plus the new tests. Run `cargo build --workspace`: no error (callers of `run_bounded`, `git_run` and `hud_git_with` still compile). For each Windows target that `rustup target list --installed` lists, also run `cargo check -p zygo-work --tests --target` with that target; with none listed, the windows-latest CI job is the check.
- [ ] **Step 15 — commit and push.** `git add crates/zygo-work/src/repo.rs crates/zygo-work/tests/git_runner.rs && git commit -m "MS-1: Flow, Streamed and run_stream"` (plus trailers), then `git push`.
