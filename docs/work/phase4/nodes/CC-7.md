# CC-7 — Argument grammar and session flags (concern 7) read the descriptor

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.1.2 concern 7, K§9 (P4-6: `args.rs`, `launcher.rs`), K-D3
**Files:** `crates/zygo-core/src/client/mod.rs`, `crates/zygo-core/src/client/claude_code.rs`, `crates/zygo-core/src/client/opencode.rs`, `crates/zygo-workspace/src/args.rs`, `crates/zygo-launch/src/launcher.rs`, `crates/zygo-workspace/tests/args.rs` (from the CC-1 inventory at `b324e1dc`, round 2)
**Consumes:** `Agent::spec` (CC-3), `ClientSpec` (CC-3), `inventory` (CC-1), `check-test-edits.sh` (CC-2)
**Produces:** `ClientSpec` concern 7 fields: the flag table, the subcommand rule, the session-flag names, the insertion point of session arguments, the "bare" rule (names: the builder's, K§10); kept unchanged: the signatures of `zygo_workspace::args::{occurrences, flags, has_session_arg}` at `b324e1dc` (the CC-9 adapter rows call them, so CC-7 makes no adapter edit; CC-1 round 1, F8)
**Acceptance:** `test -z "$(for f in crates/zygo-workspace/src/args.rs; do test ! -e "$f" || awk 'FNR==1{t=0;a=0} a && /^(pub(\([^)]*\))? )?mod [A-Za-z_][A-Za-z0-9_]* \{/{t=1} {a=0} /^#\[cfg\((test|all\(test, [^"]*\)|all\([^"]*, test\))\)\]/{a=1} !t && !/^[[:space:]]*\/\// && /Agent::(ClaudeCode|OpenCode)/{print FILENAME":"FNR}' "$f"; done)" && cargo test -p zygo-workspace -p zygo-launch -p zygo-core --locked --no-fail-fast && git show "$(sed -n 's/^Parent: \([0-9a-f]\{40\}\)$/\1/p' docs/work/phase4/nodes/CC-7.md):scripts/check-test-edits.sh" | sh -s -- "$(sed -n 's/^Parent: \([0-9a-f]\{40\}\)$/\1/p' docs/work/phase4/nodes/CC-7.md)" docs/work/phase4/nodes/CC-7.md` → every command exits 0, and `cargo test --workspace --locked --no-fail-fast` is green on ubuntu, macOS and Windows CI The first command (CC-1 round 3 F3): no production line names `Agent::ClaudeCode` or `Agent::OpenCode` in the files whose last production variant-path row at `b324e1dc` is this node's (9 lines there); a production line is a line before the file's test region (the inline `mod <name> {` that directly follows a column-0 `#[cfg(test)]`, `#[cfg(all(test, …))]` or `#[cfg(all(…, test))]`) and not a `//` comment; a file that no longer exists passes. A file that also holds a later node's variant-path rows is checked by that later node (`crates/zygo-launch/src/launcher.rs` CC-9, `crates/zygo/src/cmd/member.rs` and `ws.rs` CC-10).

## Planning notes

- File list: from the K§1 inventory at `bf3726bd` (`args.rs:356-360`, `:402-406`, `:507`, `:595-611`, `:652-656`; `launcher.rs:733-736`). The concern 7 sites in `crates/zygo/src/cmd/member.rs` (`:189`, `:688-691`) are CC-10, which owns that file. Before the plan is written, confirm the list against the CC-1 rows that name CC-7, and add each file a row names.
- The grammar functions take `&[String]` and give indexes or `Vec<String>` (K§6.1.1: no new `zygo-core` dependency). `zygo_workspace::args::opencode_subcommand` (called at `launcher.rs:735`) becomes a descriptor function.
- Signatures kept (CC-1 round 1, F8): `zygo_workspace::args::occurrences(agent: Agent, args: &[String]) -> Vec<Occurrence>`, `flags(agent: Agent, args: &[String]) -> Vec<&'static str>` and `has_session_arg(agent: Agent, args: &[String]) -> bool` (at `b324e1dc`: `args.rs:701`, `:695`, `:586`) keep their signatures. The CC-9 adapter rows call them (`crates/zygo-launch/src/claude_code.rs:90`, `:95`, `:101`, `:701`; `opencode.rs:512`), so CC-7 makes no adapter edit; each body reads the descriptor of the `agent` it is given.
- The argument vectors stay equal for both clients (K-D3); `crates/zygo-workspace/tests/args.rs` and `stub_launch` are the proof.

## Test logic edits

None.

## Plan

Written when the node starts. Its first line is `Parent: <40-hex>`, the commit this node's layer starts from.
