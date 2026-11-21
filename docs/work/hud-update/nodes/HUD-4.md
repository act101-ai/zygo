# HUD-4 — Prices: long_context tier, cache_write_5m, Prices::cost, price sanity

**Spec:** `docs/specs/2026-11-21-statusline-hud-design.md` H§7.3 steps 1-2, AU5, AU32, AU33, H§9 rows §6.3; catalog data per H§11 `catalog.toml` row and §19a. Edge to HUD-3 for the shared `schema.rs` and `check.rs`
**Files:** `crates/zygo-config/src/schema.rs`, `crates/zygo-config/src/check.rs`, `crates/zygo-config/data/catalog.toml`, `crates/zygo-config/tests/prices.rs`
**Consumes:** —
**Produces:** `schema::LongContextPrices { above: u64, input: f64, output: f64, cache_read: f64, cache_write: f64 }`, `Prices.long_context: Option<LongContextPrices>`, `Prices.cache_write_5m: Option<f64>`, `schema::TokenUsage { input: u64, output: u64, cache_read: u64, cache_write: u64, cache_write_5m: Option<u64>, cache_write_1h: Option<u64> }`, `Prices::cost(&self, u: &TokenUsage) -> Option<f64>`
**Acceptance:** `cargo test -p zygo-config --test prices && cargo test --workspace --locked --no-run` → all pass and the workspace builds, including H§14 #11, the AU32 `config check` rejections, a non-finite cost counted as not priced, and the AU33 split rates

## Plan
