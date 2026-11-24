# MS-7 — Secret pass on one message: clear, finding, could not run

**Spec:** `docs/specs/2026-11-24-mail-sync-design.md` S§8.6 ("Pattern set", "Run", "Three results"), S-G4; S§19 #4 and #5 (the pass result only)
**Files:** `crates/zygo-mail-sync/src/secret.rs`, `crates/zygo-mail-sync/tests/secret.rs`
**Consumes:** —
**Produces:** `zygo_mail_sync::secret::Outcome` (enum: `Clear`, `Finding(Vec<String>)`, `CouldNotRun(&'static str)`), `zygo_mail_sync::secret::check(msg: &serde_json::Value, redaction: &zygo_config::schema::RedactionConfig, main_root: &Path, policy: &zygo_redact::secret::DeadlinePolicy) -> Outcome`
**Acceptance:** `cargo test -p zygo-mail-sync --test secret` → all pass: a body with a built-in AWS key pattern gives `Finding` with that pattern id; a clean message gives `Clear` and the input value is not changed; `expire_after_scans = Some(0)` (`crates/zygo-redact/src/secret.rs:18-19`) gives `CouldNotRun`; an act-lite store file that exists and does not parse gives `CouldNotRun`; no store file gives `Clear` for a clean message (regex patterns only); `Outcome` carries pattern ids and check names only. zygo-redact source does not change.
**Uses:** zygo_redact::patterns::compile_with_store (existing, crates/zygo-redact/src/patterns.rs:36), zygo_redact::act_lite_store::read (existing, crates/zygo-redact/src/act_lite_store.rs:62-64), zygo_redact::secret::redact, DeadlinePolicy, SessionCache (existing, crates/zygo-redact/src/secret.rs:13-26, :74-76; crates/zygo-redact/src/cache.rs:122-129)

## Plan
