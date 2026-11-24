# MS-1 — Bounded git runner: whole mode and stream mode, reachable outside zygo-work

**Spec:** `docs/specs/2026-11-24-mail-sync-design.md` S§8.7; S§17 row `crates/zygo-work/src/repo.rs:61-75`
**Files:** `crates/zygo-work/src/repo.rs`, `crates/zygo-work/tests/git_runner.rs`
**Consumes:** —
**Produces:** `zygo_work::repo::RunError` (now `pub`), `zygo_work::repo::Whole { pub ok: bool, pub out: Vec<u8>, pub err: Vec<u8> }`, `zygo_work::repo::run_whole(cmd: std::process::Command, deadline: std::time::Instant) -> Result<Whole, RunError>`, `zygo_work::repo::Flow` (enum: `Go`, `Stop`), `zygo_work::repo::Streamed` (enum: `Done { ok: bool, err: Vec<u8> }`, `Stopped`), `zygo_work::repo::run_stream(cmd: std::process::Command, deadline: std::time::Instant, each: &mut dyn FnMut(&[u8]) -> Flow) -> Result<Streamed, RunError>`
**Acceptance:** `cargo test -p zygo-work --test git_runner && cargo test -p zygo-work` → all pass: a non-zero exit returns its stdout and at most 64 KiB of stderr; a zero exit returns stdout and empty stderr; `run_stream` passes each NUL-ended entry of `git ls-tree -z` once, and after `Flow::Stop` returns `Stopped` with the git process gone (unix: its process group killed); a deadline already past returns `TimedOut` with no spawn; both modes run with stdin null and (unix) in their own process group; every existing zygo-work test still passes. No test waits on wall-clock time.

## Plan
