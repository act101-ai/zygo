# CC-4 — Proxy, redaction, usage and work-join client facts (concern 9) read the descriptor

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.1.2 concern 9, K§6.1.4 (wire format keys on `Dialect`; client policy reads the descriptor), K§6.4.2 (the two kinds of rule), K§9 (P4-6: `content.rs`, `harness.rs`, `window.rs`, `passthrough.rs`, `cache.rs`, `join.rs`, `schema.rs`), K-D3, K-D47
**Files:** `crates/zygo-core/src/client/mod.rs`, `crates/zygo-core/src/client/claude_code.rs`, `crates/zygo-core/src/client/opencode.rs`, `crates/zygo-proxy/src/window.rs`, `crates/zygo-proxy/src/passthrough.rs`, `crates/zygo-proxy/src/cache.rs`, `crates/zygo-redact/src/content.rs`, `crates/zygo-redact/src/harness.rs`, `crates/zygo-work/src/join.rs`, `crates/zygo-usage/src/savings.rs`, `crates/zygo-config/src/schema.rs`, `scripts/test-edit-rules/CC-4.rules` (new, if a test line is rewritten)
**Consumes:** `Agent::spec` (CC-3), `ClientSpec` (CC-3), `inventory` (CC-1), `base` (CC-1), `check-test-edits.sh` (CC-2)
**Produces:** `ClientSpec` concern 9 fields: client dialect (`Dialect`), request-class and compaction header names, window rewrite armed, passthrough credential type name, cache-marker default, harness span rule for redaction, per-agent request id (names: the builder's, K§10)
**Acceptance:** `cargo test -p zygo-proxy -p zygo-redact -p zygo-usage -p zygo-work -p zygo-config --locked --no-fail-fast && sh scripts/check-test-edits.sh "$(sed -n 's/^Base: \([0-9a-f]\{40\}\)$/\1/p' docs/work/phase4/nodes/CC-1.md)"` → every command exits 0 (the RS tests of K§11 items 19 and 21 among them), and `cargo test --workspace --locked --no-fail-fast` is green on ubuntu, macOS and Windows CI

## Planning notes

- File list: from the K§1 and K§9 inventory at `bf3726bd`, checked at `c21b32e0` (`window.rs:131`, `passthrough.rs:35`, `content.rs:262`, `join.rs:16`, `schema.rs:468`) and `b7e9d45d` (`savings.rs:77`). Before the plan is written, confirm it against the CC-1 rows that name CC-4, and add each file a row names.
- Depends on RS-6 and RS-7: they change `cache.rs` and `window.rs`, and K-D47 leaves the per-client policy sites there for this node to move. The wire-format stages that key on `Dialect` stay as RS-5 to RS-7 built them (K§6.1.4).
- Client policy only: `[cache] insert_markers.<client>` default (`schema.rs:468`), the redaction harness spans (`content.rs:262`), whether the window rewrite is armed (`window.rs:131`), the passthrough credential row, the per-agent id of `join.rs:16` and `savings.rs:77`. No value changes (K-D3): the defaults stay Claude Code `false`, OpenCode `true`.
- The proxy and redaction carry K-C1 (no body stored or logged); this node moves policy reads and changes no body path.

## Plan

Written when the node starts.
