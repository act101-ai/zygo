# CC-8 — Pane kind, resume kind and turn events (concern 8) read the descriptor; `PaneKind` becomes `Agent(Agent) | Shell`

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.1.2 concern 8 and the `PaneKind` paragraph, K§4 K-C7 (64 `PaneKind::` test uses), K§9 (P4-6: `state.rs`, `up.rs`, `snapshot.rs`, `loadout.rs`, `schema.rs`, `check.rs`), K§11 item 29, K-D3
**Files:** `crates/zygo-core/src/client/mod.rs`, `crates/zygo-core/src/client/claude_code.rs`, `crates/zygo-core/src/client/opencode.rs`, `crates/zygo-workspace/src/state.rs`, `crates/zygo-workspace/src/up.rs`, `crates/zygo-workspace/src/snapshot.rs`, `crates/zygo-workspace/src/loadout.rs`, `crates/zygo-config/src/schema.rs`, `crates/zygo-config/src/check.rs`, `crates/zygo-config/src/repo_def/shape.rs` (at the P4-6 base `b324e1dc`, checked at CC-1 planning), `crates/zygo-workspace/tests/pane_kind.rs` (new), `crates/zygo-workspace/tests/` and `crates/zygo/tests/` (identifier edits only, K-C7), `crates/zygo-config/src/repo_def/loadout.rs`, `crates/zygo/src/cmd/workspace_approve.rs` (both from the CC-1 inventory at `b324e1dc`), `crates/zygo-config/tests/config_layers.rs`, `crates/zygo-config/tests/definition_keys.rs` (from the CC-1 inventory at `b324e1dc`, round 2)
**Consumes:** `Agent::spec` (CC-3), `ClientSpec` (CC-3), `inventory` (CC-1), `check-test-edits.sh` (CC-2)
**Produces:** `zygo_workspace::state::PaneKind { Agent(Agent), Shell }` (serialized `"claude-code"`, `"opencode"`, `"shell"` as today); `ClientSpec` concern 8 fields: the resume kind (pre-assign or capture), whether a config folder is saved for resume (AU61), the turn events the client gives, the pane-kind id (names: the builder's, K§10)
**Acceptance:** `test "$(cargo test -q -p zygo-workspace --test pane_kind --locked -- --list 2>/dev/null | grep -c ': test$')" -ge 4 && cargo test -p zygo-workspace --test pane_kind --locked && cargo test -p zygo-workspace -p zygo-config -p zygo --locked --no-fail-fast && sh scripts/check-test-edits.sh "$(sed -n 's/^Parent: \([0-9a-f]\{40\}\)$/\1/p' docs/work/phase4/nodes/CC-8.md)" docs/work/phase4/nodes/CC-8.md` → at least 4 `pane_kind` tests, all pass: a state file with `"kind": "claude-code"` reads into the Claude Code pane kind and writes back the same bytes; the same for `"opencode"` and `"shell"`; an unknown kind is refused as at the base; then every command exits 0, and `cargo test --workspace --locked --no-fail-fast` is green on ubuntu, macOS and Windows CI

## Planning notes

- File list: from the K§1 inventory at `bf3726bd` (`state.rs:209-218`, `up.rs:677-679`, `snapshot.rs:262-264`, `schema.rs:680`, `check.rs:171`) and at `c21b32e0` (`loadout.rs:99`). The concern 8 sites in `crates/zygo/src/cmd/` (`ws.rs:207-245`, `member.rs:65`, `:76-79`, `loadout.rs:328`) are CC-10. Phase 3 adds 4 lines to `state.rs` (`git diff --stat bf3726bd b7e9d45d`). Before the plan is written, confirm the list against the CC-1 rows that name CC-8, and add each file a row names.
- `LOADOUT_KINDS` (`schema.rs:680` at `c21b32e0`) is built from `Agent::ALL` plus `"shell"`, in the same order.
- `crates/zygo-config/src/repo_def/shape.rs` (phase 3; `:123` the member `kind` list `["claude-code", "opencode"]`, `:139` the pool rule "only a claude-code member", at `origin/phase-3` `b6b4bdac`): the kind list and the pool rule read the descriptor (concern 8). The error texts stay byte-equal (K-D3).
- The `PaneKind::ClaudeCode` and `PaneKind::OpenCode` test uses (64 under `crates/*/tests` at `bf3726bd`) become `PaneKind::Agent(Agent::ClaudeCode)` and `PaneKind::Agent(Agent::OpenCode)`: identifiers only, so the K-C7 check passes them with no listing. CC-13 later sweeps the `Agent::` variant paths.
- K§11 item 29 is the `pane_kind` test file (a new file is not a checked unit, CC-2).

## Test logic edits

None planned. Any edit that the K-C7 check reports is listed here, in the CC-2 entry form, with its spec reason, before the review gate.

## Plan

Written when the node starts. Its first line is `Parent: <40-hex>`, the commit this node's layer starts from.
