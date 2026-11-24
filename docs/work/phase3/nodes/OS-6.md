# OS-6 — Carry computation for one event over its context's requests

**Spec:** `docs/specs/2026-11-24-output-shaping-design.md` O§6.3 (steps 1-4, the rules, the worked example), O-C5
**Files:** `crates/zygo-usage/src/carry.rs`, `crates/zygo-usage/src/lib.rs`, `crates/zygo-usage/tests/carry.rs`
**Consumes:** —
**Produces:** `Rates { input: f64, cache_read: f64, write: f64 }` (USD per Mtok; `write` = `cache_write_5m`, absent: `cache_write`), `CtxRequest { rowid: i64, ts: i64, model: String, provider: String, input_tokens: u64, cache_read_tokens: u64, cache_write_tokens: u64, compaction: bool }`, `carry::carry(event_ts: i64, q: &[CtxRequest], first_only: bool, rates: &dyn Fn(&str, &str, u64) -> Option<Rates>) -> Carry`, `Carry { usd_per_token: f64, unpriced: Vec<i64> }` (`q` is Q_C in `ts`, `rowid` order; the closure takes model, provider and prompt tokens; `unpriced` holds rowids)
**Acceptance:** `cargo test -p zygo-usage --test carry` → all pass, including: the O§6.3 worked example gives `usd_per_token × 10_000 = 0.12` and, for the read-back after request 5, `usd_per_token × 1_000 = 0.0054` (each within 1e-12); a context whose cache-write tokens exceed its cache-read tokens on each row and whose cache-read tokens never fall prices each later row at `cache_read`; a first row whose prompt is under half of the prompt of the row before the event gives an empty carry; a `compaction` row is taken and ends the carry; an event in the same second as request A starts at A; `first_only` takes one row; a row with no cache tokens is priced at `input`; a row whose closure returns `None` adds 0 and its rowid is in `unpriced`; doubled rates give twice `usd_per_token`

## Plan
