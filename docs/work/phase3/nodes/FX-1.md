# FX-1 — Process-group kill tests: anonymous pipe in place of a FIFO (Darwin FIFO lost wakeup)

**Spec:** `docs/specs/2026-11-24-mail-sync-design.md` S§8.7 (stream mode: after the callback says stop, the process group is killed; both modes: own process group; review focus 46; proven by `stream_stop_kills_the_process_group`, MS-1); `docs/specs/2026-11-24-decision-records-design.md` D§9.4 opening paragraph (bounded git processes; `Piped` drop kills the group, proven by `piped_drop_kills_the_process_group`, DR-11) and D§17 (no wall-clock test); review focus 75 (no new test reads a clock)
**Files:** `crates/zygo-work/tests/git_runner.rs`, `crates/zygo-work/tests/decision_git.rs`
**Consumes:** —
**Produces:** —
**Acceptance:** `cargo test -p zygo-work --locked --test git_runner --test decision_git` → all pass on ubuntu, macOS and Windows; no FIFO (`mkfifo`) remains in either file; neither test waits on a timer or sleeps (the only clock read is the existing `far()` deadline, one hour away); both tests stay `#[cfg(unix)]`.

## Plan

### Defect (found in the RT-1 floor, execute-work §5.4)

Both tests kill a process group, then do a blocking read on a named FIFO whose only writer was a killed grandchild. Darwin's FIFO code has a lost-wakeup race: when the reader is asleep as the last writer closes, the reader is never woken and the test hangs forever. Seen: one 24-minute hang of `stream_stop_kills_the_process_group` in a local floor; stress ~1 in 1,500-3,000 runs; a plain-C repro hung 9 times in 15,600 runs. Anonymous pipes do not have the race (0 hangs in 35,000). CI runs `macos-latest` with no `timeout-minutes` (`.github/workflows/ci.yml`), so one hang costs up to 360 min. Production code is not affected: it reads anonymous pipes, and `stop` kills the whole group.

### Premises checked against the live code (branch `phase3/FX-1` at `33085c9`)

- `crates/zygo-work/tests/git_runner.rs` (blob `fc15fbb`): `stream_stop_kills_the_process_group` at lines 204-232 makes a FIFO with `mkfifo` (line 216), opens the read end `O_NONBLOCK` (line 218), the grandchild opens it as fd 3 (`exec 3>"$1"`), the test sets it blocking with `fcntl` (line 229) and asserts `read_to_end` gives 0.
- `crates/zygo-work/tests/decision_git.rs`: `piped_drop_kills_the_process_group` at lines 83-103, the same FIFO pattern (`mkfifo` line 91); the grandchild prints `go\n`, the test reads it with `p.read_line(16)`, then `drop(p)`.
- Kill paths, both through `fn stop(child: &mut Child)` (`crates/zygo-work/src/repo.rs:106-114`, blob `2ebadab`; group kill by `zygo_config::repo_file::kill_tree` at line 113): run_stream's `Flow::Stop` calls `run.stop()` (`repo.rs:156-161`); `Piped` holds a `Running` whose `Drop` calls `stop` (`repo.rs:131-133`). So one mutant of line 113 tests both.
- `spawn_piped` sets `process_group(0)` (`repo.rs:277`). Adding `pre_exec` makes std use fork+exec in place of posix_spawn; `process_group(0)` still applies.
- Toolchain 1.98 (`rust-toolchain.toml`), edition 2024: `std::io::pipe()` is stable (since 1.87); it returns `(PipeReader, PipeWriter)`, both close-on-exec, so other tests' children running in parallel do not inherit the write end.
- After the change `fs`, `TempDir` and `libc` stay used in both files (other tests use them); no import becomes an orphan except the per-test `use` lines named below.

### Steps

- [ ] **1. Change `stream_stop_kills_the_process_group`** (`crates/zygo-work/tests/git_runner.rs:204-232`). Apply the triage diff (`proposed-fix.diff` in the triage scratchpad; it applies to blob `fc15fbb`). The result:
  ```rust
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
  ```
  `w == 3` needs `F_SETFD 0` because `dup2(3, 3)` does not clear close-on-exec.
- [ ] **2. See it pass.** `cargo test -p zygo-work --locked --test git_runner` → all pass.
- [ ] **3. Prove it still catches a leak (manual, not a CI test; leaves no change).** In `crates/zygo-work/src/repo.rs:113` replace `zygo_config::repo_file::kill_tree(child);` with `let _ = child.kill(); let _ = child.wait(); // MUTANT: leader only` (the triage `proposed-fix-with-mutant.diff` hunk). Build first (`cargo test -p zygo-work --locked --test git_runner --no-run`), then run `cargo test -p zygo-work --locked --test git_runner -- --exact stream_stop_kills_the_process_group` in the background. Expected: the test does not finish, and `pgrep -fl "sleep 1000000"` shows the grandchild alive (the leader is gone). Then kill the test process, `pkill -f "sleep 1000000"`, `git checkout -- crates/zygo-work/src/repo.rs`, and confirm `git diff --exit-code crates/zygo-work/src` exits 0 and `pgrep -f "sleep 1000000"` finds nothing. Record the result in this file under "Mutant checks".
- [ ] **4. Change `piped_drop_kills_the_process_group`** (`crates/zygo-work/tests/decision_git.rs:83-103`) with the same pattern. The result:
  ```rust
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
  ```
  The old test had no doc comment; the new one gets one so the reason for the pipe stays next to the code.
- [ ] **5. See it pass.** `cargo test -p zygo-work --locked --test decision_git` → all pass.
- [ ] **6. Prove it still catches a leak (manual; leaves no change).** Same mutant as step 3 (`Piped`'s drop reaches `stop` through `Running::drop`, `repo.rs:131-133`). Run `cargo test -p zygo-work --locked --test decision_git -- --exact piped_drop_kills_the_process_group` in the background; expect no finish and the grandchild alive in `pgrep -fl "sleep 1000000"`. Kill the test and the grandchild, restore `repo.rs` with `git checkout --`, confirm `git diff --exit-code crates/zygo-work/src` exits 0 and no `sleep 1000000` is left. Record under "Mutant checks".
- [ ] **7. Run the acceptance.** `cargo test -p zygo-work --locked --test git_runner --test decision_git` → all pass. `grep -c mkfifo crates/zygo-work/tests/git_runner.rs crates/zygo-work/tests/decision_git.rs` → `0` for both. `git diff --stat` lists only the two test files and this node file. Windows: both tests keep `#[cfg(unix)]`, so `pre_exec`, `libc` and `CommandExt` do not compile there; the windows-latest CI job is the check.
- [ ] **8. Commit.** `FX-1: anonymous pipe in place of a FIFO in the process-group kill tests`, with the brief's `Zygo-Session` and `Zygo-Work` trailers, no attribution. Push the node branch.

### Mutant checks

(filled in at steps 3 and 6)
