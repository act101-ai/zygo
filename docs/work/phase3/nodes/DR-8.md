# DR-8 — Session view `repo_root`, written by the launcher

**Spec:** `docs/specs/2026-11-24-decision-records-design.md` D§13 item 6
**Files:** `crates/zygo-core/src/session_view.rs`, `crates/zygo-launch/src/launcher.rs`, `crates/zygo-core/tests/core_basics.rs`, `crates/zygo-launch/tests/launcher.rs`
**Consumes:** —
**Produces:** `zygo_core::session_view::SessionView.repo_root: Option<String>` (`#[serde(default)]`; the repository top level the launcher holds, `self.top`)
**Acceptance:** `test "$(cargo test -q -p zygo-core --test core_basics -- repo_root --list 2>/dev/null | grep -c ': test$')" -ge 2 && test "$(cargo test -q -p zygo-launch --test launcher -- repo_root --list 2>/dev/null | grep -c ': test$')" -ge 2 && cargo test -p zygo-core --locked --test core_basics repo_root && cargo test -p zygo-launch --locked --test launcher repo_root` → every count holds (at least 2 named `repo_root…` in zygo-core `core_basics`; at least 2 named `repo_root…` in zygo-launch `launcher`; a count of 0 fails before any run), then all pass, with at least two tests run: a `view.json` without `repo_root` loads as `None` and a view with it round-trips; a launch in a repository writes the top level into `view.json`, a launch outside one writes `None`.

## Plan
