# CC-9 — Launch layer: the `adapter_for` registry, the adapters moved under `client/`, plugin delivery and environment rows (concerns 6 and 11)

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.1.1 (upper layer), K§6.1.2 concerns 6 and 11, K§6.1.3, K§9 (P4-6: `zygo-launch/src/client/`, `adapter.rs`, `launcher.rs`, `env.rs`, `zygo-plugins/src/snapshot.rs`), K§11 items 27 and 28, K-D2, K-D3, K-D5, K-D27
**Files:** `crates/zygo-core/src/client/mod.rs`, `crates/zygo-core/src/client/claude_code.rs`, `crates/zygo-core/src/client/opencode.rs`, `crates/zygo-launch/src/client/mod.rs` (new), `crates/zygo-launch/src/client/claude_code/` (new; moved from `src/claude_code.rs`), `crates/zygo-launch/src/client/opencode/` (new; moved from `src/opencode.rs` and `assets/opencode-plugin.js`), `crates/zygo-launch/src/claude_code.rs` (deleted), `crates/zygo-launch/src/opencode.rs` (deleted), `crates/zygo-launch/assets/opencode-plugin.js` (deleted), `crates/zygo-launch/src/adapter.rs`, `crates/zygo-launch/src/launcher.rs`, `crates/zygo-launch/src/env.rs`, `crates/zygo-launch/src/lib.rs`, `crates/zygo-plugins/src/snapshot.rs`, `crates/zygo-workspace/src/env.rs` (at the P4-6 base `b324e1dc`, checked at CC-1 planning), `crates/zygo-launch/tests/` and `crates/zygo/tests/` (identifier edits only, K-C7)
**Consumes:** `Agent::spec` (CC-3), `ClientSpec` (CC-3), `inventory` (CC-1), `check-test-edits.sh` (CC-2)
**Produces:** `zygo_launch::client::adapter_for(agent: Agent, zygo: PathBuf) -> Box<dyn AgentAdapter>`, `AgentAdapter::deliver_plugins(..)`, `AgentAdapter::prepare_resume(..)`, `AgentAdapter::env_rows(..)` (parameters: the builder's, K§10); `ClientSpec` concern 6 field (plugin delivery form name) and concern 11 field (the client's own denylist rows); `zygo_launch::Plugins` removed
**Acceptance:** `test "$(cargo test -q -p zygo-launch --lib --locked -- client:: --list 2>/dev/null | grep -c ': test$')" -ge 2 && cargo test -p zygo-launch --lib --locked -- client:: && test ! -e crates/zygo-launch/src/claude_code.rs && test ! -e crates/zygo-launch/src/opencode.rs && test ! -e crates/zygo-launch/assets/opencode-plugin.js && ! grep -rnE 'enum Plugins' crates/zygo-launch/src && cargo test -p zygo-launch --test stub_launch --locked && cargo test -p zygo-launch -p zygo-plugins -p zygo --locked --no-fail-fast && sh scripts/check-test-edits.sh "$(sed -n 's/^Parent: \([0-9a-f]\{40\}\)$/\1/p' docs/work/phase4/nodes/CC-9.md)" docs/work/phase4/nodes/CC-9.md` → at least 2 `client::` table tests (the table check fails on a table with a client of `Agent::ALL` missing, and on a client with two rows), all pass; the old files are gone; `stub_launch` passes with its assertions unchanged (K§11 item 28); every command exits 0, and `cargo test --workspace --locked --no-fail-fast` is green on ubuntu, macOS and Windows CI

## Planning notes

- File list: from the K§1 inventory at `bf3726bd` (`launcher.rs:519-547`, `snapshot.rs:264-266`, `adapter.rs:74-77`, `env.rs:22-33`). The adapter call sites in `crates/zygo/src/cmd/` (`claude.rs:21`, `opencode.rs:21`, `ws.rs:218-245` at `c21b32e0`) are CC-10, which calls `adapter_for` and `prepare_resume`. Phase 3 changes the Claude Code launch and `opencode-plugin.js` (K§6.9). Before the plan is written, confirm the list against the CC-1 rows that name CC-9, and add each file a row names.
- `adapter_for` is a table keyed by client id, because the launch crate cannot match the kind (K§6.1.1). The table test lives in `client/mod.rs` (a new module is not a checked unit, CC-2).
- `crates/zygo-workspace/src/env.rs` (`REMOVE_INSIDE_AGENT`, `:30-43` at `c21b32e0` and `origin/phase-3`): its `CLAUDE_CODE_*` and `OPENCODE_*` rows are client environment rows (concern 11). They come from each descriptor's rows plus the common rows, in the same order, so the list stays equal (K-D3).
- K§11 item 28: `crates/zygo-launch/tests/stub_launch.rs` keeps each assertion and expected value. The `Plugins::` uses (`crates/zygo-launch/tests/opencode.rs:239`, `:853`, `:1003`, `:1048` at `bf3726bd`) change only identifiers; any literal change is listed below.
- K-D27: the denylist rows of each client stay as at the base for its own child; no row is added to or removed from either child's environment.
- K-D5: each new adapter method maps to an existing capability (`deliver_plugins` A-11, `prepare_resume` WA-1, `env_rows` A-10), so no registry change here.

## Test logic edits

None planned. Any edit that the K-C7 check reports is listed here, in the CC-2 entry form, with its spec reason, before the review gate.

## Plan

Written when the node starts. Its first line is `Parent: <40-hex>`, the commit this node's layer starts from.
