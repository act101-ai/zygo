# FX-3 — Test stdin helpers tolerate a child that exits before it reads stdin (statusline BrokenPipe flake)

**Spec:** `docs/specs/2026-11-21-statusline-hud-design.md` H§6.8 (:358, no `ZYGO_SESSION_DIR`: the hook prints nothing), AU24 (:119, a detached thread discards the rest of stdin; the hook never waits for it), H§7.1 (:409, `reads_stdin: false` on purpose), PH11 (:588, a status line command may exit before it reads all of stdin); review focus 75 (no new test reads a clock). The spec does not change.
**Files:** `crates/zygo/tests/common/mod.rs`, `crates/zygo/tests/compact_cmd.rs`, `crates/zygo/tests/statusline_cmd.rs`, `crates/zygo/tests/statusline_git.rs`
**Consumes:** —
**Produces:** —
**Acceptance:** `env -u ZYGO_SESSION_DIR cargo test -p zygo --locked --test compact_cmd --test statusline_cmd --test statusline_git` → all pass; no `write_all(..).unwrap()` on child stdin remains in those three files; the new test uses no sleep and no clock.

## Plan

### Defect (found in CI, execute-work §5.4)

`statusline_without_a_session_dir_prints_nothing` (`crates/zygo/tests/compact_cmd.rs:408-412`) flakes on ubuntu CI with a panic at `compact_cmd.rs:153:50`, `Os { code: 32, kind: BrokenPipe }` (CI runs 45533949611 and 45533953994, on PRs #41 and #42). Without `ZYGO_SESSION_DIR`, `zygo hook statusline` exits without reading stdin: its table entry has `reads_stdin: false` (`crates/zygo/src/cmd/hook.rs:24`), so the dispatcher skips the shared read (`hook.rs:117-119`), and `statusline_hud` returns `Ok(HookOutput::None)` (`hook.rs:46-48`) before its only stdin reader (`hook.rs:54-55`). The helper `zygo()` writes `{}` after spawn with `write_all(..).unwrap()`; when the child has exited first, the write gets EPIPE (Rust ignores SIGPIPE), so it returns `BrokenPipe` and the helper panics. The product is correct by H§6.8, AU24 and PH11, so the fix goes in the test helpers.

### Premises checked against the live code (branch `phase3/FX-3` at `b36b0eb`)

- `crates/zygo/tests/compact_cmd.rs` (blob `f7c4a52`): `use std::io::Write;` at line 3, used only by the one `write_all` at line 153; `#[derive(Default, Clone)] struct Env` at line 92; `fn zygo(home, args, env, stdin)` at line 108; Command setup lines 109-151; spawn with three piped streams at line 152; `child.stdin.take().unwrap().write_all(stdin).unwrap();` at line 153; `wait_with_output` at line 154. No `mod common;` yet; `mod member_fixture` (lines 20-21) declares no `common`.
- `crates/zygo/tests/statusline_cmd.rs` (blob `340fa63`): `mod common;` line 7, `use std::io::Write;` line 9 (its only `write_all` is line 76), `Fx::run()` at line 54, the write at line 76.
- `crates/zygo/tests/statusline_git.rs` (blob `08d6cf5`): `mod common;` line 7, `use std::io::Write;` line 10 (its only `write_all` is line 82), `Fx::hud()` at line 54, the write at line 82.
- `crates/zygo/tests/common/mod.rs` (blob `6748c11`): `#![allow(dead_code)]` at line 1, so a test binary that does not call `feed_stdin` gets no warning.
- `crates/zygo/src/cmd/hook.rs` (blob `f461b8a`): lines 24, 46-48, 54-55 as in the defect paragraph.
- `std::process::Child::wait` closes `self.stdin` before it waits, so the new test must `take()` stdin first to keep it open across `wait()`. After `wait()`, `wait_with_output()` returns the cached status and reads stdout and stderr.
- Windows: a write to a pipe with no reader fails with `ERROR_NO_DATA`, which std maps to `ErrorKind::BrokenPipe`, so `feed_stdin` behaves the same on all three CI images.
- Other helpers left alone: every other `write_all` on child stdin in `crates/zygo/tests/` feeds a hook or command that reads stdin to EOF before any exit path (`reads_stdin: true` hooks read at `hook.rs:119`, before `workspace` checks `--event`), or feeds `hook token` empty bytes (`hook_trim.rs:76`, no write syscall), so none can race.

### Steps

- [x] **1. Extract `spawn_zygo`** in `crates/zygo/tests/compact_cmd.rs`. Move lines 109-152 (the `Command` setup through the `spawn()`) into
  ```rust
  /// `zygo <args>` in `home` with `env`, all three streams piped; the caller feeds stdin.
  fn spawn_zygo(home: &Path, args: &[&str], env: &Env) -> std::process::Child
  ```
  which returns the child. `zygo()` becomes `let mut child = spawn_zygo(home, args, env);`, then the unchanged line 153 write and line 154 `wait_with_output`. Check: `env -u ZYGO_SESSION_DIR cargo test -p zygo --locked --test compact_cmd` → all pass (no behavior change).
- [x] **2. Add the failing test** right after `statusline_without_a_session_dir_prints_nothing`, still with the raw write:
  ```rust
  /// Without `ZYGO_SESSION_DIR` the hook may exit before it reads stdin (H§6.8, AU24, PH11), so a write after its exit
  /// gets a broken pipe. The order is fixed with no clock: stdin stays open until the child has exited.
  #[test]
  fn statusline_without_a_session_dir_exits_before_it_reads_stdin() {
      let d = home();
      let env = Env { hud: true, ..Default::default() };
      let mut child = spawn_zygo(d.path(), &["hook", "statusline"], &env);
      let mut si = child.stdin.take().unwrap(); // taken first: `wait` closes a stdin it still holds
      assert_eq!(child.wait().unwrap().code(), Some(0));
      si.write_all(b"{}").unwrap(); // step 4 replaces this with `common::feed_stdin(si, b"{}")`
      let out = child.wait_with_output().unwrap();
      assert_eq!((String::from_utf8(out.stdout).unwrap(), String::from_utf8(out.stderr).unwrap()), (String::new(), String::new()));
  }
  ```
- [x] **3. See it fail.** `env -u ZYGO_SESSION_DIR cargo test -p zygo --locked --test compact_cmd -- --exact statusline_without_a_session_dir_exits_before_it_reads_stdin` → FAILED, panic at the `si.write_all` line with `kind: BrokenPipe`. Run it three times; it fails each time (`--exact` runs it alone, so no concurrent spawn holds the pipe's read end).
- [x] **4. Add `feed_stdin`** to `crates/zygo/tests/common/mod.rs`:
  ```rust
  /// Writes `bytes` to a child's stdin and closes it. A child may exit before it reads stdin (statusline: H§6.8, AU24, PH11), so a broken pipe is not an error.
  pub fn feed_stdin(mut stdin: std::process::ChildStdin, bytes: &[u8]) {
      use std::io::Write;
      if let Err(e) = stdin.write_all(bytes) {
          assert_eq!(e.kind(), std::io::ErrorKind::BrokenPipe, "write to child stdin: {e}");
      }
  }
  ```
  `stdin` drops at the end, which closes it.
- [x] **5. Use it in `compact_cmd.rs`.** Add `mod common;` before `#[cfg(unix)] mod member_fixture;` (line 20). In the new test, replace `si.write_all(b"{}").unwrap();` with `common::feed_stdin(si, b"{}");` (and drop `mut` from `si`). In `zygo()`, replace line 153 with `common::feed_stdin(child.stdin.take().unwrap(), stdin);`. Remove `use std::io::Write;` (line 3), now an orphan.
- [x] **6. See it pass.** The step 3 command → ok. Then `env -u ZYGO_SESSION_DIR cargo test -p zygo --locked --test compact_cmd` → all pass.
- [x] **7. Use it in the two statusline files.** `crates/zygo/tests/statusline_cmd.rs:76` → `common::feed_stdin(child.stdin.take().unwrap(), stdin);`; remove `use std::io::Write;` (line 9). `crates/zygo/tests/statusline_git.rs:82` → `common::feed_stdin(child.stdin.take().unwrap(), stdin.as_bytes());`; remove `use std::io::Write;` (line 10). Build with no warning: `cargo test -p zygo --locked --test statusline_cmd --test statusline_git --no-run 2>&1 | grep -c warning` → `0`.
- [x] **8. Run the acceptance.** `env -u ZYGO_SESSION_DIR cargo test -p zygo --locked --test compact_cmd --test statusline_cmd --test statusline_git` → all pass. `grep -nE 'stdin\.take\(\)\.unwrap\(\)\.write_all|\.write_all\(' crates/zygo/tests/compact_cmd.rs crates/zygo/tests/statusline_cmd.rs crates/zygo/tests/statusline_git.rs` → no output. The new test body contains no `sleep`, `Instant`, `SystemTime` or `Duration` (review focus 75). `git diff --stat` lists only the four test files and this node file.
- [x] **9. Commit.** `FX-3: test stdin helpers tolerate a child that exits before it reads stdin`, with the brief's `Zygo-Session` and `Zygo-Work` trailers, no attribution. Push the node branch.
