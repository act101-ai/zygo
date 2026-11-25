# CC-7 — Argument grammar and session flags (concern 7) read the descriptor

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.1.2 concern 7, K§9 (P4-6: `args.rs`, `launcher.rs`), K-D3
**Files:** `crates/zygo-core/src/client/mod.rs`, `crates/zygo-core/src/client/claude_code.rs`, `crates/zygo-core/src/client/opencode.rs`, `crates/zygo-workspace/src/args.rs`, `crates/zygo-launch/src/launcher.rs`, `scripts/test-edit-rules/CC-7.rules` (new, if a test line is rewritten)
**Consumes:** `Agent::spec` (CC-3), `ClientSpec` (CC-3), `inventory` (CC-1), `base` (CC-1), `check-test-edits.sh` (CC-2)
**Produces:** `ClientSpec` concern 7 fields: the flag table, the subcommand rule, the session-flag names, the insertion point of session arguments, the "bare" rule (names: the builder's, K§10)
**Acceptance:** `cargo test -p zygo-workspace -p zygo-launch -p zygo-core --locked --no-fail-fast && sh scripts/check-test-edits.sh "$(sed -n 's/^Base: \([0-9a-f]\{40\}\)$/\1/p' docs/work/phase4/nodes/CC-1.md)"` → every command exits 0, and `cargo test --workspace --locked --no-fail-fast` is green on ubuntu, macOS and Windows CI

## Planning notes

- File list: from the K§1 inventory at `bf3726bd` (`args.rs:356-360`, `:402-406`, `:507`, `:595-611`, `:652-656`; `launcher.rs:733-736`). The concern 7 sites in `crates/zygo/src/cmd/member.rs` (`:189`, `:688-691`) are CC-10, which owns that file. Before the plan is written, confirm the list against the CC-1 rows that name CC-7, and add each file a row names.
- The grammar functions take `&[String]` and give indexes or `Vec<String>` (K§6.1.1: no new `zygo-core` dependency). `zygo_workspace::args::opencode_subcommand` (called at `launcher.rs:735`) becomes a descriptor function.
- The argument vectors stay equal for both clients (K-D3); `crates/zygo-workspace/tests/args.rs` and `stub_launch` are the proof.

## Plan

Written when the node starts.
