# WD-3 — Workspace approvals in `approvals.toml`

**Spec:** `docs/specs/2026-11-24-workspace-definition-design.md` R§7.2, R§11 rows "Approvals file invalid" and "Approval write fails", R§18 items 8 (the store key) and 21
**Files:** `crates/zygo-config/src/repo_def/mod.rs`, `crates/zygo-config/src/repo_def/store.rs` (new), `crates/zygo-config/tests/repo_def_store.rs` (new), `crates/zygo-plugins/tests/approve_shared.rs` (new)
**Consumes:** `digest` (WD-2)
**Produces:** `repo_def::is_workspace_approved_in(file: &Path, ws_id: &str, digest: &str) -> bool`, `repo_def::approve_workspace_in(file: &Path, ws_id: &str, digest: &str) -> anyhow::Result<Stored>`, `repo_def::Stored { Added, AlreadyPresent }`
**Acceptance:** `cargo test -p zygo-config --test repo_def_store && cargo test -p zygo-plugins --test approve_shared` → all pass. The tests cover: an entry `[[workspace]]` with `id` (16 hex) and `digest` (64 hex) appended with every earlier byte kept (lock on `<file>.lock`, temp file, `fsync`, rename, as `crates/zygo-plugins/src/approve.rs:38-77`); a second approve of the same pair is `AlreadyPresent` and the file is byte-equal; another `ws_id` with the same digest is not approved; an invalid approvals file approves nothing, `approve_workspace_in` returns `Err` and the file is byte-equal; a failed write leaves no temp file (`cfg(unix)` read-only directory case; the invalid-file case runs everywhere); `zygo_plugins::approve::is_approved_in` finds each plugin approval in a file that also holds `[[workspace]]` entries, and `zygo_plugins::approve::approve_in` keeps those entries byte for byte (R§18 item 21)

## Plan
