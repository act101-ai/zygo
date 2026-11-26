# TF-1 — Tests: a child that exits before reading stdin does not fail the test (BrokenPipe)

**Spec:** none (test-harness defect); Global constraints "No test … may pass or fail by wall-clock time"
**Files:** `crates/zygo/tests/common/mod.rs`, `crates/zygo/tests/common/e2e.rs`, `crates/zygo/tests/compact_cmd.rs`, `crates/zygo/tests/results_sweep.rs`, `crates/zygo/tests/mcp_stdio.rs`, `crates/zygo/tests/cli_surface.rs`, `crates/zygo/tests/session_verbs.rs`, `crates/zygo/tests/cli_pipe.rs`, `crates/zygo/tests/statusline_cmd.rs`, `crates/zygo/tests/statusline_git.rs`, `crates/zygo/tests/slash_parity.rs`, `crates/zygo/tests/hook_session.rs`, `crates/zygo/tests/guard_hook.rs`, `crates/zygo/tests/hook_trim.rs`, `crates/zygo/tests/work_hook.rs`, `crates/zygo/tests/workspace_run.rs`, `crates/zygo/tests/update.rs`, `crates/zygo/tests/workspace_restore.rs`
**Consumes:** —
**Produces:** `common::write_stdin`, a stdin write that ignores `ErrorKind::BrokenPipe` and only that error; every child-stdin write in `crates/zygo/tests` goes through it
**Acceptance:** first, a test that fails first: `write_stdin` called with a child that has already exited must not panic; then `CARGO_INCREMENTAL=0 cargo test -p zygo --locked --test compact_cmd` → exit 0; no sleeps or timing assertions

## Planning notes

- CI run 45806142357, job ubuntu-latest, PR #192: `crates/zygo/tests/compact_cmd.rs` test `statusline_without_a_session_dir_prints_nothing` panicked at `compact_cmd.rs:153:50` with ``called `Result::unwrap()` on an `Err` value: Os { code: 32, kind: BrokenPipe }``. macOS and Windows passed on the same run.
- Cause: the helper `zygo` (`compact_cmd.rs:108-156`) spawns `zygo` with piped stdin and runs `child.stdin.take().unwrap().write_all(stdin).unwrap()` (`:153`). A `zygo` command that does not read stdin can exit before the write; the write then gets `EPIPE` (the Rust runtime ignores `SIGPIPE`, so it is an `Err`, not a signal). The write races the exit.
- Sites with the same pattern (child stdin, `write_all(...).unwrap()`), from `grep -rn "write_all" crates/zygo/tests` at origin/phase-4 1aebffbf:
  - fire and forget (write, drop, wait): `compact_cmd.rs:153`, `results_sweep.rs:50`, `mcp_stdio.rs:31`, `cli_surface.rs:168`, `:319`, `:369`, `:516`, `session_verbs.rs:84`, `cli_pipe.rs:30`, `statusline_cmd.rs:76`, `statusline_git.rs:82`, `slash_parity.rs:111`, `hook_session.rs:53`, `guard_hook.rs:29`, `:105`, `:207`, `:450`, `hook_trim.rs:20`, `work_hook.rs:32`, `workspace_run.rs:904`;
  - an answer to a child that waits for it (`Guard::stdin()` of `common/workspace.rs:114`): `update.rs:761`, `:933`, `:959`, `:998`, `:1106`, `workspace_restore.rs:758`, `:784`, `common/e2e.rs:871`, `workspace_run.rs:223`.
- Not this pattern: `session_verbs.rs:476` (a FIFO), `workspace_session.rs:205` and `common/mod.rs:59` (files), `update.rs:70`, `member_fixture/mod.rs:172`, `workspace_restore.rs:106`, `:981`, `:983` (already `let _ =`).
- Shared support module: `crates/zygo/tests/common/mod.rs` (`#![allow(dead_code)]`), declared by `mcp_stdio`, `cli_surface`, `statusline_cmd`, `statusline_git`, `slash_parity`, `update`, `workspace_restore`, `workspace_run` and others. `compact_cmd`, `results_sweep`, `session_verbs`, `cli_pipe`, `hook_session`, `guard_hook`, `hook_trim` and `work_hook` do not declare it yet.

## Plan

Capability: `mechanical` (one helper, one test, the same one-line change at each site).

### Interfaces

- Produces, in `crates/zygo/tests/common/mod.rs`: `pub fn write_stdin(mut w: impl std::io::Write, bytes: &[u8])` — `w.write_all(bytes)`; `Ok` returns; `Err(e)` with `e.kind() == ErrorKind::BrokenPipe` returns; any other `Err` panics with the error. `w` drops at the end (EOF to the child), as the drop of the temporary did before.

### Steps

- [x] 1. Write the test that fails first, in `crates/zygo/tests/compact_cmd.rs`: `stdin_write_to_an_exited_child_is_not_an_error`. Spawn the built `zygo` (`env!("CARGO_BIN_EXE_zygo")`) with `--version`, `Stdio::piped()` stdin and null stdout and stderr; take its stdin; `child.wait()` until it exits (the reader is then closed, so the write cannot race: no sleep, no timeout); then call `common::write_stdin(stdin, &vec![b'x'; 1 << 20])` (1 MiB, more than any pipe buffer). Add `mod common;` to the file. Run `CARGO_INCREMENTAL=0 cargo test -p zygo --locked --test compact_cmd stdin_write_to_an_exited_child`. Pass: it fails (does not compile: `write_stdin` is not defined).
- [x] 2. Prove the old write fails the same way: in a scratch copy of the test, replace the `write_stdin` call with `stdin.write_all(...).unwrap()`; run it. Pass: it panics with `BrokenPipe`. Discard the scratch copy.
- [x] 3. Make the stdin write ignore an `ErrorKind::BrokenPipe` error, and only that error: add `write_stdin` (Interfaces) to `crates/zygo/tests/common/mod.rs`. Run step 1's command. Pass: the test passes.
- [x] 4. Replace each site listed in Planning notes with `common::write_stdin(<stdin>, <bytes>)` (one shared helper, since the tests share `common`). Add `mod common;` to each listed file that lacks it; inside `common/e2e.rs` call `super::write_stdin`. Where a site keeps the `ChildStdin` for a later `drop` (`update.rs:933`), pass `&mut stdin` and keep the `drop`.
- [x] 5. Keep asserting the child's output and exit status as before: change no assertion, no expected value, no `wait_with_output` and no exit-code check. Run `git diff origin/phase-4 -- crates/zygo/tests`. Pass: outside the new test and `write_stdin`, each changed line is a write site or a `mod common;` line.
- [x] 6. Run `grep -rn "write_all" crates/zygo/tests`. Pass: no child-stdin `write_all(...).unwrap()` remains; only the excluded sites of Planning notes and the one inside `write_stdin`.
- [x] 7. Run the Acceptance check `CARGO_INCREMENTAL=0 cargo test -p zygo --locked --test compact_cmd`. Pass: exit 0. Then `CARGO_INCREMENTAL=0 cargo test -p zygo --locked --tests`. Pass: no new failure against origin/phase-4.
- [ ] 8. Commit `fix(tests): TF-1 ignore BrokenPipe on child stdin writes` with trailers `Zygo-Session: <session>` and `Zygo-Work: phase4:TF-1`; push the node branch.

### Execution notes

- Step 1 failed first (E0425: `write_stdin` not found in `common`). Step 2 panicked at the old write with `Os { code: 32, kind: BrokenPipe }`, the CI error. Step 3 passed.
- All 29 sites use `write_stdin`; none kept `unwrap`. The 9 answer sites (`update.rs`, `workspace_restore.rs`, `common/e2e.rs`, `workspace_run.rs:223`) each assert the child's exit status or output (or read a file that the child writes) after the write, so a child that exits without reading its answer still fails the test there.
- `common/e2e.rs` is not a child of `common`: each test declares it beside `common` with `#[path = "common/e2e.rs"] mod e2e;`, and it already imports `crate::common::…`. So it calls `crate::common::write_stdin`, not `super::write_stdin`.
- The change removed `use std::io::Write;` from 13 places (12 files and `mcp_call` in `common/e2e.rs`) where the replaced site was its only user (`-D warnings` rejects the unused import).
- Step 7: `--test compact_cmd` 5 passed, 19 ignored. `--tests`: 384 passed, 2 failed, 301 ignored; the 2 failures are the known `guard_hook` ones (`binary_guard_not_a_repository_writes_no_line`, `binary_guard_slow_rev_parse_says_unread_once`; `guard_hook.rs:73`, a checkout under `/private/tmp` has no directory outside the temp roots).
