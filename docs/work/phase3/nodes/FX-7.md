# FX-7 — Exec-failure relaunch test runs in a child process (environ swap breaks parallel PATH lookups)

**Spec:** — (test harness defect in the `zygo` lib tests; production code does not change. `cmd::ws::tests::relaunch_returns_the_exec_error` (`crates/zygo/src/cmd/ws.rs:622-627`) calls `super::relaunch(Path::new("/nonexistent/zygo-m5"), …)` in the shared test process. `relaunch()` (`ws.rs:339-342`) is `Command::new(..).args(..).env(RELAUNCHED, "1").exec()`. Because the command sets an environment variable, std points the process-wide `environ` at a temporary heap copy for the `exec`, restores it when the `exec` fails, and frees the copy. A parallel test thread that spawns `git` by `PATH` in that window (helpers `crates/zygo/src/cmd/work.rs:237-241`, `crates/zygo/src/cmd/plugins.rs:202-207`) reads a broken `PATH` and fails with `ENOENT` or `InvalidInput: nul byte found in provided data`. Evidence: the full lib suite with `--skip relaunch_returns_the_exec_error`, 0 failures in 200 runs; only that test plus `cmd::work cmd::plugins`, 97 failures in 200 runs; a `posix_spawnp` shim saw `getenv("PATH")` return a heap pointer during the failures; seen in this graph's floor runs as `ws p=2977 f=1`); review focus 75 (no new test reads a clock). The spec does not change.
**Files:** `crates/zygo/src/cmd/ws.rs`, `crates/zygo/src/cmd/down.rs`
**Consumes:** —
**Produces:** —
**Acceptance:** `env -u ZYGO_SESSION_DIR cargo test -p zygo --locked --lib -- cmd::ws cmd::work cmd::plugins` → all pass, and `cmd::ws::tests::relaunch_exec_error_in_child` stays ignored in this plain run. Checked in review, not by this command: no lib test calls `exec()` in the test process; the change adds no sleep and no retry.

## Plan

Cause: `relaunch_returns_the_exec_error` is the only lib test that reaches an `exec()`. The two `exec()` calls in `crates/zygo/src` are `ws.rs:325` (`exec_shell`) and `ws.rs:341` (`relaunch`). `exec_shell` is called only from `run_cmd` (`ws.rs:150-195`), `run_cmd` only from `dispatch` (`ws.rs:104`), `dispatch` only from `cmd::run` (`crates/zygo/src/cmd/mod.rs:563`), and `cmd::run` only from `crates/zygo/src/main.rs:5`; no lib test calls any of them. `relaunch` is called from `run_cmd` (`ws.rs:137`) and from the test (`ws.rs:625`).

Fix: the existing child-process pattern of `crates/zygo/src/cmd/down.rs:303-334` (`slash_down_does_not_need_the_current_folder` runs `slash_down_in_a_removed_folder`). The test body moves unchanged to an ignored child test, `relaunch_exec_error_in_child`. The parent test keeps the name `relaunch_returns_the_exec_error` and runs the test binary (`std::env::current_exe()`) with `--exact cmd::ws::tests::relaunch_exec_error_in_child --ignored --test-threads=1`. In the child process no other test thread runs, so the `environ` swap cannot break a parallel `PATH` lookup. The parent spawns with no environment change, so its own spawn does not swap `environ`. `relaunch`, `relaunch_target` and every non-test line of `ws.rs` do not change. No sleep, no retry, no clock.

Commits: each commit message starts with `FX-7:` and ends with the `Zygo-Session` and `Zygo-Work` trailers that the brief gives, each on its own line. No Claude attribution.

- [x] 1. Record the flake on the base (evidence, no code change). Run `for i in $(seq 1 20); do env -u ZYGO_SESSION_DIR cargo test -q -p zygo --locked --lib -- cmd::ws cmd::work cmd::plugins >/dev/null 2>&1 || echo fail; done | wc -l`. Expected: at least 1 failure (triage: 97 in 200). If 0, record the count and continue; the triage evidence stands.

- [x] 2. Write the failing test. In `crates/zygo/src/cmd/ws.rs` `mod tests`, replace the body of `relaunch_returns_the_exec_error` (`ws.rs:622-627`) with the parent. Do not add the child yet:

```rust
    /// D4: the exec failure path. `relaunch` sets `ZYGO_RELAUNCHED`, so std points the process-wide `environ` at a
    /// temporary copy during the `exec` and frees it after the failure; a parallel test that spawns `git` by `PATH`
    /// then reads a freed `PATH` (FX-7). The child test runs the `exec` in its own process, with one test thread.
    #[cfg(unix)]
    #[test]
    fn relaunch_returns_the_exec_error() {
        let o = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "cmd::ws::tests::relaunch_exec_error_in_child", "--ignored", "--nocapture", "--test-threads=1"])
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();
        let text = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
        assert!(o.status.success(), "{text}");
        assert!(text.contains("test result: ok. 1 passed"), "{text}");
    }
```

  Run `env -u ZYGO_SESSION_DIR cargo test -p zygo --locked --lib -- --exact cmd::ws::tests::relaunch_returns_the_exec_error`. Expected: FAIL at the second assertion; the child run matches no test, so its output holds `test result: ok. 0 passed` and not `1 passed`. A compile error is not the red result.

- [x] 3. Add the child test, directly after the parent, with the old body unchanged:

```rust
    /// The child of `relaunch_returns_the_exec_error`: the failed `exec` swaps `environ`, so it runs alone.
    #[cfg(unix)]
    #[test]
    #[ignore = "child process of relaunch_returns_the_exec_error"]
    fn relaunch_exec_error_in_child() {
        let e = super::relaunch(Path::new("/nonexistent/zygo-m5"), &[OsString::from("ws")]);
        assert_eq!(e.kind(), std::io::ErrorKind::NotFound);
    }
```

  Run the command of step 2. Expected: PASS. Then run `env -u ZYGO_SESSION_DIR cargo test -p zygo --locked --lib -- cmd::ws::tests::relaunch`. Expected: `relaunch_returns_the_exec_error ... ok` and `relaunch_exec_error_in_child ... ignored`. Commit `FX-7: run the exec-failure relaunch test in a child process`.

- [x] 4. Check that the child still proves the exec failure path (a mutation check, not committed). Change `ErrorKind::NotFound` in the child to `ErrorKind::PermissionDenied`, run the command of step 2, and expect FAIL with the child's assertion text in the parent's message. Revert the change; `git diff` is empty.

- [x] 5. Check that no lib test calls `exec()` in the test process. `grep -rn '\.exec()' crates/zygo/src` gives only `ws.rs:325` and `ws.rs:341`. `grep -n 'exec_shell(\|relaunch(' crates/zygo/src/cmd/ws.rs` gives calls only in `run_cmd` and in `relaunch_exec_error_in_child`. `grep -rn 'run_cmd\|ws::dispatch\|cmd::run()' crates/zygo/src` gives no call inside a `mod tests`. `git diff origin/phase-3 -- crates/zygo/src/cmd/ws.rs` changes lines only inside `mod tests`, and holds no `sleep`, no retry loop and no `Instant`/`SystemTime`.

- [x] 6. Repeat the loop of step 1. Expected: 0 failures in 20 runs.

- [x] 7. Run the Acceptance command as written: `env -u ZYGO_SESSION_DIR cargo test -p zygo --locked --lib -- cmd::ws cmd::work cmd::plugins`. All tests pass, and `relaunch_exec_error_in_child` shows as ignored. Run `cargo clippy -p zygo --locked --all-targets -- -D warnings` and `cargo fmt --check -p zygo`; both are clean. Push `phase3/FX-7`.

Deviations: step 7, `cargo clippy -p zygo --locked --all-targets -- -D warnings` fails on base code outside Files (`crates/zygo-work/src/gate/transition.rs:10` large_enum_variant, `:158` collapsible_if; this branch does not change `zygo-work`), and `cargo fmt --check -p zygo` reports diffs across the crate that predate FX-7 (base `ws.rs` already has 44; the new test lines add none). Neither was fixed here. Evidence: step 1, 7 failures in 20; step 6, 0 failures in 20.

## Fix round 1

- F1 (fixed): under `--include-ignored` the child tests ran in the shared test process: `relaunch_exec_error_in_child` swapped `environ` with its failed `exec`, and `cmd::down::tests::slash_down_in_a_removed_folder` removed the process cwd. Each parent now sets `ZYGO_TEST_CHILD` to the child's full test path on the child `Command`; each child returns at once, doing nothing, unless `ZYGO_TEST_CHILD` equals its own path. Each child prints `child ran: <name>` after its assertions, and its parent asserts that line, because a child that returns early still reports `1 passed`.
- Counts, 20 runs of `env -u ZYGO_SESSION_DIR cargo test -q -p zygo --locked --lib -- --include-ignored cmd::ws cmd::work cmd::plugins`: 2 failures before, 0 after. Plain run (step 6): 0 in 20.
- Mutation: with the `.env("ZYGO_TEST_CHILD", …)` line removed from both parents, both parents fail at the `child ran:` assertion; restored.
- Deviation: the brief asked for the env guard only, but with the guard alone the mutation survived (the early-returning child still reports `test result: ok. 1 passed`). The `child ran:` marker and its parent assertion were added so that the parent proves the child body ran. The new lines follow the file's line width, not default rustfmt (see the step 7 deviation).
