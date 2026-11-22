# FIX-14 — state tests: the `hold` helper retries the lock

**Spec:** `docs/specs/2026-11-20-workspaces-design.md` W§7.3 lock rules (AU41); found by W6 round 3 (R3-D5). Defect: the test helper `hold` (`crates/zygo-workspace/tests/state.rs:342-347`) takes the lock with one `fs4::FileExt::try_lock` and unwraps it. In the first full W6 round-3 run, the same single `try_lock` pattern in `tests/snapshot.rs` failed with `WouldBlock` in a parallel run; alone the test passed. Likely cause, not proven: a live test in another thread forks tmux, and the child holds a copy of every open file description until its `exec`, so a flock that this process just released can stay held for that moment. Fix: `hold` retries `try_lock` on `WouldBlock` every 5 ms for at most 5 s, then panics with the error, as W6's `hold` in `tests/snapshot.rs:62-73` does; any other error panics at once. No test changes what it asserts
**Files:** `crates/zygo-workspace/tests/state.rs`
**Consumes:** `state::Store::update` (W2)
**Produces:** state tests hold helper retries
**Acceptance:** `cargo test -p zygo-workspace --locked --test state` → all pass, with `hold` retrying `try_lock` on `WouldBlock` for at most 5 s

## Plan

The fix is the body of `hold` in `crates/zygo-workspace/tests/state.rs:342-347`; it becomes the loop of `hold` in `crates/zygo-workspace/tests/snapshot.rs:62-73`, with the same doc comment. `S5` (`state.rs:12`), `Instant` and `Duration` are in scope already.

Decisions (author's call):
- D1. The defect is a rare race with a forked child, so the test does not fork. It makes the same condition in-process: a flock is per open file description, so a lock that a second handle of the same file holds gives `WouldBlock` to `hold`, as the child's copy does.
- D2. The new test is `hold_retries_a_lock_that_is_released_soon`, after `hold`. It takes the lock with `hold` (the lock is free, so the first `try_lock` succeeds), moves that handle to a thread that sleeps 50 ms and drops it, then calls `hold` on the same path and asserts that it returns, that the wait is at least 25 ms (so the lock was held when `hold` started) and less than `S5`. 25 ms, not 50 ms, because the thread can start its sleep before the main thread reads the start time.
- D3. No existing test changes; the Acceptance stays as written.

Steps:

- [ ] Write the failing test `hold_retries_a_lock_that_is_released_soon` in `state.rs` after `hold` (D2).
- [ ] Run it and see it fail: `cargo test -p zygo-workspace --locked --test state hold_retries_a_lock_that_is_released_soon`. Expected: 1 failed, a panic at `state.rs:345` with `WouldBlock`.
- [ ] Implement: replace the body of `hold` with the retry loop (5 ms sleep, at most `S5`, any other error panics with `hold: {e:?}`) and add the doc comment.
- [ ] Run it and see it pass: the same command → 1 passed.
- [ ] Run the Acceptance: `cargo test -p zygo-workspace --locked --test state` → all pass (23: 22 before, plus the new test). Then `cargo clippy -p zygo-workspace --locked --tests -- -D warnings` → exit 0.
- [ ] Commit `crates/zygo-workspace/tests/state.rs` and this file, ticked, in the FIX-14 checkout: "fix(workspace): FIX-14 state hold retries the lock".
