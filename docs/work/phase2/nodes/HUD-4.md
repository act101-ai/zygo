# HUD-4 — Prices: long_context tier, cache_write_5m, Prices::cost, price sanity

**Spec:** `docs/specs/2026-11-21-statusline-hud-design.md` H§7.3 steps 1-2, AU5, AU32, AU33, H§9 rows §6.3; catalog data per H§11 `catalog.toml` row and §19a. Edge to HUD-3 for the shared `schema.rs` and `check.rs`. Updates the `verified_on` assert at `crates/zygo-config/tests/builtin.rs:72` with the catalog's new `verified_on`
**Files:** `crates/zygo-config/src/schema.rs`, `crates/zygo-config/src/check.rs`, `crates/zygo-config/data/catalog.toml`, `crates/zygo-config/tests/prices.rs`, `crates/zygo-config/tests/builtin.rs`
**Consumes:** —
**Produces:** `schema::LongContextPrices { above: u64, input: f64, output: f64, cache_read: f64, cache_write: f64, cache_write_5m: Option<f64> }`, `Prices.long_context: Option<LongContextPrices>`, `Prices.cache_write_5m: Option<f64>`, `schema::TokenUsage { input: u64, output: u64, cache_read: u64, cache_write: u64, cache_write_5m: Option<u64>, cache_write_1h: Option<u64> }`, `Prices::cost(&self, u: &TokenUsage) -> Option<f64>`
**Acceptance:** `cargo test -p zygo-config --test prices && cargo test --workspace --locked --no-run && cargo test -p zygo-config --test builtin` → all pass and the workspace builds, including H§14 #11, the AU32 `config check` rejections, a non-finite cost counted as not priced, and the AU33 split rates

## Plan

- [x] Read the contract and H§7.3 steps 1-2, AU5, AU32, AU33, H§9 rows §6.3, H§11, H§13, H§14 #11-#12.
- [x] Check the prices on the provider's pages on 2026-11-21: `https://platform.claude.com/docs/en/about-claude/pricing` (5m cache writes: Fable 5.2 $10, Opus 5.5 $5, Sonnet 5.5 $2.50, Haiku 5.5 $0.125; Haiku 5.5 "for prompts over 100,000 tokens": $0.50 input, $2.50 output, $0.05 cache read, $1 1h write, $0.625 5m write; "A request's prompt length counts all of its input tokens, including cache reads and cache writes") and the four `facts.source` overview pages (same rates; context window 1M; max output 128K).
- [x] Write `crates/zygo-config/tests/prices.rs` first (27 tests); see it fail to compile (no `TokenUsage`, `LongContextPrices`, `Prices::cost`, `cache_write_5m`, `long_context`).
- [x] `schema.rs`: add `Prices.cache_write_5m`, `Prices.long_context`, `LongContextPrices`, `TokenUsage`, `Prices::cost`; see the cost tests on hand-made tables pass and the `config check` and catalog tests fail (14 failed).
- [x] `check.rs`: refuse a negative, NaN or infinite rate (base tier and `long_context`) and `long_context.above = 0`.
- [x] `catalog.toml`: `cache_write_5m` on the four Anthropic rows; the Haiku comment becomes `long_context` data (with its own `cache_write_5m = 0.625`, from the price page); `verified_on = 2026-11-21` on the four Anthropic rows. The two OpenAI rows were not checked again and keep `2026-11-18`.
- [x] `tests/builtin.rs`: the `verified_on` assert expects `2026-11-21` for the Anthropic rows and `2026-11-18` for the OpenAI rows.
- [x] Run `cargo test -p zygo-config` (all targets pass), `cargo clippy -p zygo-config --all-targets --locked -- -D warnings` (clean), then the three Acceptance commands.

Design calls (author of the node):

- A long tier with no `cache_write_5m` prices 5-minute writes at the tier's `cache_write`, not at the base 5-minute rate ("all rates come from `long_context`").
- `TokenUsage.cache_write` is the total. Write tokens outside the reported split use `cache_write`. A split larger than the total is priced, and counted in the prompt, as reported.
- The token sums saturate; the cost is computed in `f64`.
- `LongContextPrices` has `#[serde(default)]` as `Prices` has, so an absent `above` is 0 and `config check` refuses it.
