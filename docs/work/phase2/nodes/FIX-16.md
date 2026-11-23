# FIX-16 — workspace_down passes the name after --

**Spec:** `docs/specs/2026-11-20-workspaces-design.md` W§9.3 step 2 and W§12 (row `zygo down`) as amended 2026-11-22 (W10 plan review: the detached process gets the name after `--`), AU65, AU84 (`-` is allowed at the start of a name); found by W10 plan review round 1 (B3, W10 D8, controller item C5). Defect: `WorkspaceDown::call` refuses a workspace name that starts with `-` (`crates/zygo-mcp/src/tools/workspace_down.rs:98-104`), because `zygo down` had no `--` handling, and runs `<zygo> down <name> [--now]`. Fix: `WorkspaceDown::call` runs `<zygo> down [--now] -- <name>` and no longer refuses a name that starts with `-`; a test with the workspace name `-x` checks the argument list. Edge to M1: M1's `zygo down` parser accepts `[--] [<name>]` (M1 Spec, W10 controller item C4), so the tool's argument list reaches `down` as a name
**Files:** `crates/zygo-mcp/src/tools/workspace_down.rs`, `crates/zygo-mcp/tests/mcp_server.rs`
**Consumes:** `tools::workspace_down::WorkspaceDown::new` (W11), `down::run` (W10)
**Produces:** workspace_down passes the name after --
**Acceptance:** `cargo test -p zygo-mcp --locked --test mcp_server` → all pass, including a call for the workspace name `-x` whose stub receives argv `down -- -x` (no refusal), and the W§18 #78 cases with argv `down -- <name>` and `down --now -- <name>` with `now = true`
