# CC-6 — Hook output and session text delivery (concerns 3 and 4) read the descriptor

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.1.2 concerns 3 and 4, K§9 (P4-6: `session_start.rs`, `session_text.rs`, `workspace.rs`), K§4 K-C3 (C6), K-D3
**Files:** `crates/zygo-core/src/client/mod.rs`, `crates/zygo-core/src/client/claude_code.rs`, `crates/zygo-core/src/client/opencode.rs`, `crates/zygo-hooks/src/session_start.rs`, `crates/zygo-hooks/src/session_text.rs`, `crates/zygo-hooks/src/workspace.rs`, `crates/zygo-hooks/src/prompt.rs`, `crates/zygo-hooks/src/role_guard.rs`, `crates/zygo-hooks/src/trim.rs`, `crates/zygo-work/src/guards.rs`, `crates/zygo-work/src/hooks.rs` (from the CC-1 inventory at `b324e1dc`, round 1)
**Consumes:** `Agent::spec` (CC-3), `ClientSpec` (CC-3), `inventory` (CC-1), `check-test-edits.sh` (CC-2)
**Produces:** `ClientSpec` concern 3 field: a pure function that renders a common reply (context text, deny, block) as the client's hook output `serde_json::Value`; concern 4 fields: the session text delivery mode, the model alias table of the alias sentence, whether a prompt can be blocked while stopping (names: the builder's, K§10)
**Acceptance:** `test -z "$(for f in crates/zygo-hooks/src/session_start.rs crates/zygo-hooks/src/session_text.rs crates/zygo-hooks/src/workspace.rs; do test ! -e "$f" || awk 'FNR==1{t=0;a=0} a && /^(pub(\([^)]*\))? )?mod [A-Za-z_][A-Za-z0-9_]* \{/{t=1} {a=0} /^#\[cfg\((test|all\(test, [^"]*\)|all\([^"]*, test\))\)\]/{a=1} !t && !/^[[:space:]]*\/\// && /Agent::(ClaudeCode|OpenCode)/{print FILENAME":"FNR}' "$f"; done)" && cargo test -p zygo-hooks -p zygo-core --locked --no-fail-fast && sh scripts/check-test-edits.sh "$(sed -n 's/^Parent: \([0-9a-f]\{40\}\)$/\1/p' docs/work/phase4/nodes/CC-6.md)" docs/work/phase4/nodes/CC-6.md` → every command exits 0, and `cargo test --workspace --locked --no-fail-fast` is green on ubuntu, macOS and Windows CI The first command (CC-1 round 3 F3): no production line names `Agent::ClaudeCode` or `Agent::OpenCode` in the files whose last production variant-path row at `b324e1dc` is this node's (7 lines there); a production line is a line before the file's test region (the inline `mod <name> {` that directly follows a column-0 `#[cfg(test)]`, `#[cfg(all(test, …))]` or `#[cfg(all(…, test))]`) and not a `//` comment; a file that no longer exists passes. A file that also holds a later node's variant-path rows is checked by that later node (`crates/zygo-launch/src/launcher.rs` CC-9, `crates/zygo/src/cmd/member.rs` and `ws.rs` CC-10).

## Planning notes

- File list: from the K§1 inventory at `bf3726bd` (`session_start.rs:42-49`, `session_text.rs:83`, `workspace.rs:290`, `:621-644`) and the inventory at `c21b32e0` (`prompt.rs:173`, `:449`, maybe test sites). Phase 3 changes `session_start.rs` and the trim hook (K§6.9), so the lines move. Before the plan is written, confirm the list against the CC-1 rows that name CC-6, and add each file a row names.
- A hook never breaks the agent (K-C3): a render fault keeps the existing one-stderr-line, exit-0 path (`crates/zygo-core/src/hook.rs:76-92` at `bf3726bd`).
- The hook output bytes stay equal for both clients (K-D3); the existing `zygo-hooks` tests are the proof.

## Test logic edits

None planned. Any edit that the K-C7 check reports is listed here, in the CC-2 entry form, with its spec reason, before the review gate.

## Plan

Written when the node starts. Its first line is `Parent: <40-hex>`, the commit this node's layer starts from.
