# CC-5 — MCP tool naming and the `compact` tool listing (concern 5) read the descriptor

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.1.2 concern 5, K§9 (P4-6: `zygo-work/src/hooks.rs`, `zygo-mcp/src/tools/mod.rs`), K-D3
**Files:** `crates/zygo-core/src/client/mod.rs`, `crates/zygo-core/src/client/claude_code.rs`, `crates/zygo-core/src/client/opencode.rs`, `crates/zygo-work/src/hooks.rs`, `crates/zygo-mcp/src/tools/mod.rs`, `scripts/test-edit-rules/CC-5.rules` (new, if a test line is rewritten)
**Consumes:** `Agent::spec` (CC-3), `ClientSpec` (CC-3), `inventory` (CC-1), `base` (CC-1), `check-test-edits.sh` (CC-2)
**Produces:** `ClientSpec` concern 5 fields: a pure function from a model-visible tool name to `(server, tool)`, and whether the `compact` tool is listed (names: the builder's, K§10)
**Acceptance:** `cargo test -p zygo-work -p zygo-mcp -p zygo-core --locked --no-fail-fast && sh scripts/check-test-edits.sh "$(sed -n 's/^Base: \([0-9a-f]\{40\}\)$/\1/p' docs/work/phase4/nodes/CC-1.md)"` → every command exits 0, and `cargo test --workspace --locked --no-fail-fast` is green on ubuntu, macOS and Windows CI

## Planning notes

- File list: from the inventory at `c21b32e0` (`crates/zygo-work/src/hooks.rs:371-372`, `crates/zygo-mcp/src/tools/mod.rs:21`; `crates/zygo-mcp/src/tools/work.rs:137` is a test site). Before the plan is written, confirm it against the CC-1 rows that name CC-5, and add each file a row names.
- The tool-name parse moves into `client/<client>.rs` as a pure function on `&str` (K§6.1.1: no new `zygo-core` dependency).
- The `compact` tool stays listed only for Claude Code with the `compact_tool` feature on (no behavior change, K-D3).

## Plan

Written when the node starts.
