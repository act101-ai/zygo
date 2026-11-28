# CC-5 — MCP tool naming and the `compact` tool listing (concern 5) read the descriptor

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.1.2 concern 5, K§9 (P4-6: `zygo-work/src/hooks.rs`, `zygo-mcp/src/tools/mod.rs`), K-D3
**Files:** `crates/zygo-core/src/client/mod.rs`, `crates/zygo-core/src/client/claude_code.rs`, `crates/zygo-core/src/client/opencode.rs`, `crates/zygo-work/src/hooks.rs`, `crates/zygo-mcp/src/tools/mod.rs`, `crates/zygo-proxy/src/window.rs` (from the CC-1 inventory at `b324e1dc`, round 1), `crates/zygo-proxy/tests/window.rs` (from the CC-1 inventory at `b324e1dc`, round 2)
**Consumes:** `Agent::spec` (CC-3), `ClientSpec` (CC-3), `inventory` (CC-1), `check-test-edits.sh` (CC-2)
**Produces:** `ClientSpec` concern 5 fields: a pure function from a model-visible tool name to `(server, tool)`, and whether the `compact` tool is listed (names: the builder's, K§10)
**Acceptance:** `test -z "$(for f in crates/zygo-mcp/src/tools/mod.rs crates/zygo-work/src/hooks.rs; do test ! -e "$f" || awk 'FNR==1{t=0;a=0} a && /^(pub(\([^)]*\))? )?mod [A-Za-z_][A-Za-z0-9_]* \{/{t=1} {a=0} /^#\[cfg\((test|all\(test, [^"]*\)|all\([^"]*, test\))\)\]/{a=1} !t && !/^[[:space:]]*\/\// && /Agent::(ClaudeCode|OpenCode)/{print FILENAME":"FNR}' "$f"; done)" && cargo test -p zygo-work -p zygo-mcp -p zygo-core --locked --no-fail-fast && S=$(git show "$(sed -n 's/^Parent: \([0-9a-f]\{40\}\)$/\1/p' docs/work/phase4/nodes/CC-5.md):scripts/check-test-edits.sh") && printf '%s\n' "$S" | sh -s -- "$(sed -n 's/^Parent: \([0-9a-f]\{40\}\)$/\1/p' docs/work/phase4/nodes/CC-5.md)" docs/work/phase4/nodes/CC-5.md` → every command exits 0, and `cargo test --workspace --locked --no-fail-fast` is green on ubuntu, macOS and Windows CI The first command (CC-1 round 3 F3): no production line names `Agent::ClaudeCode` or `Agent::OpenCode` in the files whose last production variant-path row at `b324e1dc` is this node's (3 lines there); a production line is a line before the file's test region (the inline `mod <name> {` that directly follows a column-0 `#[cfg(test)]`, `#[cfg(all(test, …))]` or `#[cfg(all(…, test))]`) and not a `//` comment; a file that no longer exists passes. A file that also holds a later node's variant-path rows is checked by that later node (`crates/zygo-launch/src/launcher.rs` CC-9, `crates/zygo/src/cmd/member.rs` and `ws.rs` CC-10).

## Planning notes

- File list: from the inventory at `c21b32e0` (`crates/zygo-work/src/hooks.rs:371-372`, `crates/zygo-mcp/src/tools/mod.rs:21`; `crates/zygo-mcp/src/tools/work.rs:137` is a test site). Before the plan is written, confirm it against the CC-1 rows that name CC-5, and add each file a row names.
- The tool-name parse moves into `client/<client>.rs` as a pure function on `&str` (K§6.1.1: no new `zygo-core` dependency).
- The `compact` tool stays listed only for Claude Code with the `compact_tool` feature on (no behavior change, K-D3).

## Test logic edits

None.

## Plan

Written when the node starts. Its first line is `Parent: <40-hex>`, the commit this node's layer starts from.
