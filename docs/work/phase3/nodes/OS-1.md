# OS-1 — Result meta records the session that stored it

**Spec:** `docs/specs/2026-11-24-output-shaping-design.md` O§6.5 item 1, O§6.4 "Key"
**Files:** `crates/zygo-results/src/store.rs`, `crates/zygo-results/tests/results.rs`, `crates/zygo-hooks/src/trim.rs`
**Consumes:** —
**Produces:** `Store::put(dir: &Path, tool: &str, session: Option<&str>, bytes: &[u8]) -> anyhow::Result<Stored>`, `Stored.session: Option<String>` (from `Meta.session`, `#[serde(default)]`)
**Acceptance:** `cargo test -p zygo-results --test results && cargo test -p zygo-hooks --test hooks_basic` → all pass, including new tests: a put with `Some("S1")` gives `Store::get(..).session == Some("S1")`; a meta file without `session` (the `97d8767` form) reads as `None`; the trim hook stores its session in the meta

## Plan
