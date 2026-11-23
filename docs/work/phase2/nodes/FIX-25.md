# FIX-25 — up tests do not race on the tmux wrapper script (Text file busy)

**Spec:** test harness defect. `crates/zygo-workspace/tests/up.rs` `restore_first_window_name_resumes_and_reports` failed once on the base branch (`d715b11`, 2026-11-23) with `zygo: tmux: /tmp/zygo-mux-<pid>-<n>-<hex>/tmux: line 2: /usr/bin/tmux: Text file busy` (exit 1 vs 0) and passed on the next run: a test writes a wrapper script and executes it while another thread's forked child still holds a write handle (ETXTBSY). The helper that writes the `zygo-mux-*` wrapper is `PrivateServer::start` (`crates/zygo-workspace/src/mux/mod.rs`, test support module).
**Files:** `crates/zygo-workspace/tests/up.rs`, `crates/zygo-workspace/src/mux/mod.rs`
**Consumes:** —
**Produces:** the wrapper is written, closed and synced before any spawn can inherit its handle (or written to a temp name and renamed into place), so no test run gets ETXTBSY
**Acceptance:** `sh -c 'for i in $(seq 20); do cargo test -p zygo-workspace --locked --test up || exit 1; done' && cargo test -p zygo-workspace --locked --test down` → exit 0: `--test up` passes 20 runs in a row (no added timeout), then `--test down` passes

## Plan

Not planned yet.
