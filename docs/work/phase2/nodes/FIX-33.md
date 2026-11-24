# FIX-33 — CI failures of PR #14 (run 45338846931): macOS and Windows test builds, inherited descriptors in the detached down

**Spec:** `docs/specs/2026-11-20-workspaces-design.md` W§9.3 step 2 (the detached `down` keeps no descriptor of the caller), AU65, W§17 PW15. Created 2026-11-23 in delivery: CI run 45338846931 on PR #14 failed on all three runners.
**Files:** `crates/zygo-workspace/tests/up.rs`, `crates/zygo-workspace/tests/activity.rs`, `crates/zygo-workspace/tests/down.rs`, `crates/zygo-workspace/src/down.rs`, `crates/zygo-workspace/tests/mux.rs`, `crates/zygo/tests/session_verbs.rs`, `docs/specs/2026-11-20-workspaces-design.md`
**Consumes:** —
**Produces:** —
**Acceptance:** `gh pr checks 14` → CI green on PR #14 (ubuntu, macos, windows); locally `cargo test -p zygo-workspace --locked --no-fail-fast`, `cargo test --workspace --locked --no-run`, `cargo clippy --workspace --all-targets --locked -- -D warnings` and `sh scripts/test-install.sh` pass, and `cargo check -p zygo-workspace --tests --locked` passes for `x86_64-apple-darwin` and `x86_64-pc-windows-msvc`
**Capability:** implementation

## Plan

The three failures of run 45338846931:
1. macos-latest: `crates/zygo-workspace/tests/up.rs:321` does not compile. On Apple, `openpty` takes `*mut termios` and `*mut winsize`; the call passes `std::ptr::null()` and `&ws`.
2. windows-latest: `crates/zygo-workspace/tests/activity.rs:487-488` does not compile. The `Mode` guard uses `std::os::unix::fs::PermissionsExt` with no `cfg(unix)` gate.
3. ubuntu-latest: `detached_child_holds_no_caller_descriptor` (`crates/zygo-workspace/tests/down.rs:1690`) found descriptors 142 and 145 (`pipe:[…]`) in the detached child, besides 0-2. The runner gave the test process pipes without `FD_CLOEXEC`; `spawn_detached` (`crates/zygo-workspace/src/down.rs`) closes nothing above 2, so they reached the detached `down`. Product defect.

- [x] Fix 1: pass `std::ptr::null_mut()` and `&mut ws` (a `*mut` coerces to `*const` on Linux). Check every other `openpty`, `forkpty` and `login_tty` call in `crates/`.
- [x] Fix 2: gate `Mode` with `#[cfg(unix)]`. Check every test file for `std::os::unix` or `PermissionsExt` without a `cfg(unix)` gate.
- [x] Fix 3, test first: in `detach_helper`, before `down::down`, open a pipe and clear `FD_CLOEXEC` on both ends. Run → FAIL with the extra descriptors listed.
- [x] In the `pre_exec` of `spawn_detached`, close every descriptor above 2: `close_range(3, u32::MAX, 0)` on Linux, with a fallback loop to `sysconf(_SC_OPEN_MAX)` (cap 65536); the loop on other Unix. Run → PASS; run the whole `down` file. (Done as close-on-exec marking; see Results, Fix 3.)
- [x] W§9.3 step 2: one sentence; change-log row.
- [x] Local checks and cross-checks (see Acceptance).
- [ ] Commit, push, watch CI; record each round below.

## Results

### Fix 1: macOS `openpty` (`crates/zygo-workspace/tests/up.rs`, `pty_client`)

- The call is now `libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null_mut(), &raw mut ws)`, with `let mut ws`.
- Not as the brief expected: the brief gave `&mut ws`. On Linux, clippy denies it (`unnecessary_mut_passed`: "the function `libc::openpty` doesn't need a mutable reference"). `&raw mut ws` is a `*mut winsize`: it fits Apple's `*mut` and coerces to the `*const` of Linux, and clippy accepts it.
- A search of `crates/` for `openpty`, `forkpty` and `login_tty` found only this call.

### Fix 2: Windows (`crates/zygo-workspace/tests/activity.rs`)

- The `Mode` guard (struct and `Drop`) is `#[cfg(unix)]`. Its only user, `update_in_a_read_only_folder_is_a_file_condition`, is already `#[cfg(unix)]`.
- The import `std::process::{Child, Command, Stdio}` is `#[cfg(unix)]` too: on Windows it was unused (the CI log warned), because only `cfg(unix)` tests use it.
- The search for ungated `std::os::unix` or `PermissionsExt` in test files was done by the compiler: `cargo check --workspace --tests --target x86_64-pc-windows-msvc` passes (see Cross-checks). So every test target of every package compiles for Windows.

### Fix 3: the detached `down` inherited the caller's descriptors (`crates/zygo-workspace/src/down.rs`, `spawn_detached`)

- Test first: `detach_helper` (`crates/zygo-workspace/tests/down.rs`) now opens a pipe before `down::down` and clears `FD_CLOEXEC` on both ends.
- Red: `detached_child_holds_no_caller_descriptor` failed as in CI. Keys `["0", "1", "2", "3", "4"]`, with `"3": "pipe:[82273731]", "4": "pipe:[82273731]"`.
- First fix, as the brief gave it: `pre_exec` closed every descriptor above 2 (`close_range(3, u32::MAX, 0)`, else a `close` loop). The descriptor test passed, but `detach_round_2` failed at `down.rs:2145`: `left: (0, "zygo: down started; end your turn\n")`, `right: (1, "")`.
  - That case ("the detached child cannot start") points `zygo` at a missing file.
  - The cause: std's `spawn` reports a failed `exec` to the parent through its own close-on-exec pipe, and that pipe is open in the child during `pre_exec`. If the pipe is closed, the parent reads EOF and treats the start as a success.
- Not as the brief expected: the fix marks every descriptor above 2 close-on-exec in `pre_exec`, so `exec` closes all of them. The error pipe still reports a failed `exec`.
  - Linux: `syscall(SYS_close_range, 3, u32::MAX, CLOSE_RANGE_CLOEXEC)`. On an error (a kernel before 5.11), it uses the loop.
  - Other Unix: a loop of `fcntl(fd, F_SETFD, FD_CLOEXEC)` from 3 to `sysconf(_SC_OPEN_MAX)`, capped at 65536, with errors ignored.
  - Only async-signal-safe calls, no allocation, no `/proc` read. The function is `cloexec_above_stderr`, and its doc comment cites both tests, W§9.3 step 2, AU65 and PW15.
- Green: both tests pass, and the whole `down` file passes (41).
- Spec: W§9.3 step 2 has one new sentence, and there is a change-log row (2026-11-23, author's call (node FIX-33)).

### Local checks

- `cargo test -p zygo-workspace --locked --no-fail-fast`: exit 0, 409 passed, 2 ignored, 0 failed (down 41, up 70, activity 41 + 1 ignored).
- `cargo test --workspace --locked --no-run`: exit 0.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: exit 0.
- `sh scripts/test-install.sh`: `test-install: all passed`.

### Cross-checks

- The `x86_64-apple-darwin` and `x86_64-pc-windows-msvc` targets are installed. A plain `cargo check` fails in the C build scripts of `ring` and `libsqlite3-sys`, because this host has no SDK headers:
  - MSVC with `clang-cl`: `'assert.h' file not found`;
  - Darwin with `clang`: `'TargetConditionals.h' file not found`.
- To check the Rust code anyway, I used a compiler wrapper that stays out of the tree (scratchpad `xcc/cc-msvc` and `xcc/cc-darwin`). It turns each C compile into an empty object for the target, and sends preprocessor probes to the real clang. `cargo check` links nothing, so rustc type-checks every Rust crate as on the target. C code is not checked: CI does that.
- `cargo check --workspace --tests --locked --target x86_64-pc-windows-msvc`: exit 0. Warnings only, all dead code or unused imports on Windows outside this node: `zygo-workspace` `state.rs:662`, `tests/state.rs:724`; `zygo-mcp` `workspace_down.rs:28`; `zygo` `cmd/member.rs:20,961,964`, `cmd/ws.rs:483-484`, `tests/loadout_secrets.rs:274,553,566`.
- `cargo check --workspace --tests --locked --target x86_64-apple-darwin`: exit 0. Warnings only: in `tests/down.rs`, values used only under `target_os = "linux"`.
- The checks are real: with the old `up.rs`, the darwin check failed with CI's error, `E0308` at `up.rs:321:23`.

### CI rounds

**Round 1: run 45342068421 (head e98247d).** ubuntu success, windows success, macos failure. The macOS test build compiled for the first time. Two test targets failed, and the old compile error had hidden both:
- `crates/zygo/tests/session_verbs.rs:596` (`au105_not_saved_text_goes_at_the_end`): the expected text named `/var/folders/.../cfg/config.toml.lock`, but `zygo` printed `/private/var/folders/...`. On macOS, `/var` is a symlink to `/private/var`. The product is right: `resolve` in `crates/zygo-config/src/profile.rs` puts the lock beside the resolved config file (a symlinked config locks its target). Test defect. Fix: the expected lock path is `canonicalize(cfg/config.toml)` + `.lock`, which is the same path on Linux.
- `crates/zygo-workspace/tests/mux.rs:387` (`stderr_maps_to_answers_and_errors`): the result was `Ok(())`, not `Err(Failed { "tmux exited, but a process outside its process group holds its output" })`. The stub ran `setsid sleep 30 &`, and macOS has no `setsid` command, so no process outside the group held stdout. Test defect. Fix: the stub runs `perl -MPOSIX -e 'POSIX::setsid(); exec @ARGV' sleep 30 &`. Perl is in the base system of macOS and Ubuntu, and `exec` keeps the pid that `$!` names.
- A search of `crates/` found no other use of the `setsid` command in a test script.
- Local: `mux` 50 passed + 1 ignored, and `session_verbs` 29 passed. `cargo clippy -p zygo -p zygo-workspace --all-targets --locked -- -D warnings` exits 0, and the darwin cross-check of both packages exits 0.
