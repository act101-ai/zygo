# DR-8 — Session view `repo_root`, written by the launcher

**Spec:** `docs/specs/2026-11-24-decision-records-design.md` D§13 item 6
**Files:** `crates/zygo-core/src/session_view.rs`, `crates/zygo-launch/src/launcher.rs`
**Consumes:** —
**Produces:** `zygo_core::session_view::SessionView.repo_root: Option<String>` (`#[serde(default)]`; the repository top level the launcher holds, `self.top`)
**Acceptance:** `cargo test -p zygo-core -p zygo-launch --locked repo_root` → all pass, with at least two tests run: a `view.json` without `repo_root` loads as `None` and a view with it round-trips; a launch in a repository writes the top level into `view.json`, a launch outside one writes `None`.

## Plan
