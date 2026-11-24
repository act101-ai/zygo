# OS-9 — `[results] shape` in config and the session view

**Spec:** `docs/specs/2026-11-24-output-shaping-design.md` O§9, O§11 row "`[results] shape`"
**Files:** `crates/zygo-config/src/schema.rs`, `crates/zygo-config/tests/config_layers.rs`, `crates/zygo-core/src/session_view.rs`, `crates/zygo-core/tests/core_basics.rs`, `crates/zygo-launch/src/launcher.rs`, `crates/zygo-launch/tests/launcher.rs`, `crates/zygo-hooks/src/trim.rs`
**Consumes:** `trim::trim_in_with` (OS-4)
**Produces:** `ResultsConfig.shape: bool` (default `true`), `SessionView.shape: bool` (`#[serde(default = "default_true")]`); the launcher copies `c.results.shape` into the view
**Acceptance:** `test "$(cargo test -q -p zygo-config --test config_layers -- results_shape --list 2>/dev/null | grep -c ': test$')" -ge 2 && test "$(cargo test -q -p zygo-core --test core_basics -- view_shape --list 2>/dev/null | grep -c ': test$')" -ge 1 && test "$(cargo test -q -p zygo-launch --test launcher -- view_shape --list 2>/dev/null | grep -c ': test$')" -ge 1 && cargo test -p zygo-config --test config_layers && cargo test -p zygo-core --test core_basics && cargo test -p zygo-launch --test launcher && cargo test -p zygo-hooks --test hooks_basic` → every count holds (at least 2 named `results_shape…` in zygo-config `config_layers`; at least 1 named `view_shape…` in zygo-core `core_basics`; at least 1 named `view_shape…` in zygo-launch `launcher`; a count of 0 fails before any run), then all pass, including new tests: no `[results]` table gives `shape == true`; `shape = false` in the user file gives `false`; a `view.json` without `shape` reads `true`; a launch with `shape = false` writes `"shape": false` to the view

## Plan
