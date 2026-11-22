# FIX-14 — state tests: the `hold` helper retries the lock

**Spec:** `docs/specs/2026-11-20-workspaces-design.md` W§7.3 lock rules (AU41); found by W6 round 3 (R3-D5). Defect: the test helper `hold` (`crates/zygo-workspace/tests/state.rs:342-347`) takes the lock with one `fs4::FileExt::try_lock` and unwraps it. In the first full W6 round-3 run, the same single `try_lock` pattern in `tests/snapshot.rs` failed with `WouldBlock` in a parallel run; alone the test passed. Likely cause, not proven: a live test in another thread forks tmux, and the child holds a copy of every open file description until its `exec`, so a flock that this process just released can stay held for that moment. Fix: `hold` retries `try_lock` on `WouldBlock` every 5 ms for at most 5 s, then panics with the error, as W6's `hold` in `tests/snapshot.rs:62-73` does; any other error panics at once. No test changes what it asserts
**Files:** `crates/zygo-workspace/tests/state.rs`
**Consumes:** `state::Store::update` (W2)
**Produces:** state tests hold helper retries
**Acceptance:** `cargo test -p zygo-workspace --locked --test state` → all pass, with `hold` retrying `try_lock` on `WouldBlock` for at most 5 s
