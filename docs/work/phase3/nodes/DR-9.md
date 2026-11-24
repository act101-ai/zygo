# DR-9 — The `dispositions` guard key, default `enforce`

**Spec:** `docs/specs/2026-11-24-decision-records-design.md` D§9.3 (default level `enforce`, added at `crates/zygo-config/src/schema.rs` beside the other work-guard defaults), D§13 items 2 and 3, D-C7
**Files:** `crates/zygo-config/src/schema.rs`, `crates/zygo-work/src/policy.rs`, `crates/zygo-work/tests/policy.rs`, `crates/zygo/src/cmd/guards.rs`, `crates/zygo/tests/slash_parity.rs`, `crates/zygo/tests/serve_cli.rs`
**Consumes:** —
**Produces:** `zygo_config::GUARDS` with `"dispositions"` (7 entries); `zygo_config::GuardsConfig.dispositions: Level` (default `Level::Enforce`; a valid `[guards]` key, clamped by `min_level`); `zygo_work::policy::Guard::Dispositions` (in `Guard::CONFIGURED`, name `"dispositions"`, level from `[guards]`)
**Acceptance:** `test "$(cargo test -q -p zygo-work --test policy -- dispositions --list 2>/dev/null | grep -c ': test$')" -ge 3 && cargo test -p zygo-config -p zygo-work -p zygo --locked --no-fail-fast` → every count holds (at least 3 named `dispositions…` in zygo-work `policy`; a count of 0 fails before any run), then all pass, including a new test in `crates/zygo-work/tests/policy.rs` that the default policy gives `Guard::Dispositions` the level `enforce`, that `.zygo.toml` `[guards] dispositions = "notify"` gives `notify`, and that `min_level = "notify"` lifts a `monitor` setting to `notify`; the `describe` line and the `zygo guards` output list `dispositions enforce`.

## Plan
