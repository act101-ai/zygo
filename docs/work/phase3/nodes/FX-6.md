# FX-6 — Dead-port test helpers: a freed loopback port can be taken by a parallel test's server (CI flake)

**Spec:** — (test harness defect, CI run 45557591675: `cmd::compact::tests::unreachable_is_compact_failed` got `400 "one\ntwo\e[31m"` from a `canned_session` server bound on the port that `dead_session` (`crates/zygo/src/cmd/compact.rs:188-198`) had read and freed); review focus 75 (no new test reads a clock). The spec does not change.
**Files:** `crates/zygo/src/cmd/compact.rs`, `crates/zygo/tests/compact_cmd.rs`, `crates/zygo/tests/member_fixture/mod.rs`, `crates/zygo/tests/serve_cli.rs`, `crates/zygo/tests/workspace_member.rs`, `crates/zygo-launch/tests/serve_client.rs`
**Consumes:** —
**Produces:** —
**Acceptance:** `env -u ZYGO_SESSION_DIR cargo test -p zygo --locked --lib -- cmd::compact cmd::status cmd::verbs && env -u ZYGO_SESSION_DIR cargo test -p zygo --locked --test compact_cmd --test session_verbs --test serve_cli --test workspace_member && env -u ZYGO_SESSION_DIR cargo test -p zygo-launch --locked --test serve_client` → all pass (ignored tests stay ignored). Checked in review, not by this command: every "unreachable" URL or port these helpers make (`dead_session`, the three `closed_url`, `dead_session_at`, `closed_port`) can never be answered by another test while it is in use; no helper reads the port of a listener and then frees it; the change adds no sleep and no retry.

## Plan
