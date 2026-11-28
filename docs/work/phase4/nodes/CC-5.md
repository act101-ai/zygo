# CC-5 — MCP tool naming and the `compact` tool listing (concern 5) read the descriptor

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.1.2 concern 5, K§9 (P4-6: `zygo-work/src/hooks.rs`, `zygo-mcp/src/tools/mod.rs`), K-D3
**Files:** `crates/zygo-core/src/client/mod.rs`, `crates/zygo-core/src/client/claude_code.rs`, `crates/zygo-core/src/client/opencode.rs`, `crates/zygo-work/src/hooks.rs`, `crates/zygo-mcp/src/tools/mod.rs`, `crates/zygo-proxy/src/window.rs` (from the CC-1 inventory at `b324e1dc`, round 1), `crates/zygo-proxy/tests/window.rs` (from the CC-1 inventory at `b324e1dc`, round 2), `crates/zygo-core/tests/client_tools.rs` (new), `crates/zygo-mcp/tests/client_tools.rs` (new), `crates/zygo-work/tests/nav_client.rs` (new), `crates/zygo-proxy/tests/client_tool_names.rs` (new), `docs/work/phase4/nodes/CC-5.md` (this node file) (CC-5 plan)
**Consumes:** `Agent::spec` (CC-3), `ClientSpec` (CC-3), `inventory` (CC-1), `check-test-edits.sh` (CC-2)
**Produces:** `ClientSpec` concern 5 fields: a pure function that takes a model-visible tool name and an MCP tool name and gives the MCP server when the name is that tool of that server in the client's form (K§6.1.2 concern 5, rewritten 2026-11-28 by the CC-5 plan), and whether the `compact` tool is listed (names: the builder's, K§10)
**Acceptance:** `test -z "$(for f in crates/zygo-mcp/src/tools/mod.rs crates/zygo-work/src/hooks.rs; do test ! -e "$f" || awk 'FNR==1{t=0;a=0} a && /^(pub(\([^)]*\))? )?mod [A-Za-z_][A-Za-z0-9_]* \{/{t=1} {a=0} /^#\[cfg\((test|all\(test, [^"]*\)|all\([^"]*, test\))\)\]/{a=1} !t && !/^[[:space:]]*\/\// && /Agent::(ClaudeCode|OpenCode)/{print FILENAME":"FNR}' "$f"; done)" && cargo test -p zygo-work -p zygo-mcp -p zygo-core --locked --no-fail-fast && S=$(git show "$(sed -n 's/^Parent: \([0-9a-f]\{40\}\)$/\1/p' docs/work/phase4/nodes/CC-5.md):scripts/check-test-edits.sh") && printf '%s\n' "$S" | sh -s -- "$(sed -n 's/^Parent: \([0-9a-f]\{40\}\)$/\1/p' docs/work/phase4/nodes/CC-5.md)" docs/work/phase4/nodes/CC-5.md` → every command exits 0, and `cargo test --workspace --locked --no-fail-fast` is green on ubuntu, macOS and Windows CI The first command (CC-1 round 3 F3): no production line names `Agent::ClaudeCode` or `Agent::OpenCode` in the files whose last production variant-path row at `b324e1dc` is this node's (3 lines there); a production line is a line before the file's test region (the inline `mod <name> {` that directly follows a column-0 `#[cfg(test)]`, `#[cfg(all(test, …))]` or `#[cfg(all(…, test))]`) and not a `//` comment; a file that no longer exists passes. A file that also holds a later node's variant-path rows is checked by that later node (`crates/zygo-launch/src/launcher.rs` CC-9, `crates/zygo/src/cmd/member.rs` and `ws.rs` CC-10).

## Planning notes

- File list: from the inventory at `c21b32e0` (`crates/zygo-work/src/hooks.rs:371-372`, `crates/zygo-mcp/src/tools/mod.rs:21`; `crates/zygo-mcp/src/tools/work.rs:137` is a test site). Before the plan is written, confirm it against the CC-1 rows that name CC-5, and add each file a row names.
- The tool-name parse moves into `client/<client>.rs` as a pure function on `&str` (K§6.1.1: no new `zygo-core` dependency).
- The `compact` tool stays listed only for Claude Code with the `compact_tool` feature on (no behavior change, K-D3).

## Test logic edits

- literals crates/zygo-proxy/tests/window.rs: +"claude-code" x1 — K§6.1.2 concern 5: `compact_tool` takes the client whose MCP tool-name form it reads; `unit_windows` passes Claude Code's descriptor, parsed from its id, and keeps each asserted name and result

## Plan

Parent: d93fbc0963fdc3094ea7f9e43f6ae0c77b9046c2

Step 1 writes the first line of this section: `Parent: <40-hex>`, the commit this node's layer starts from. This file has no `Parent:` line until step 1.

Planning base: `phase-4` at `1140f668` (2026-11-28; CC-4 merged at `a081c87b`; `1140f668` changes only `docs/work/phase4/`). Since the inventory base `b324e1dc`, only `crates/zygo-proxy/src/window.rs` of this node's code changed (CC-4: the `use zygo_core::Agent;` line `:13` is removed, so the base rows `:95` and `:97` are now `:94` and `:96`; `:163` and `:206` are unchanged). `git diff --stat b324e1dc a081c87b -- crates/zygo-work/src/hooks.rs crates/zygo-mcp/src/tools/mod.rs crates/zygo-proxy/tests/window.rs` is empty, so those rows hold at their base lines.

Premise results (live code at `a081c87b`):

- File list (planning note 1): holds. The CC-1 rows that name CC-5 are `crates/zygo-mcp/src/tools/mod.rs:22`; `crates/zygo-proxy/src/window.rs:95`, `:97`, `:163`, `:206` (now `:94`, `:96`, `:163`, `:206`); `crates/zygo-proxy/tests/window.rs:29`, `:452`, `:453`, `:455`; `crates/zygo-work/src/hooks.rs:371`, `:372`. Each file is on the Files line. `crates/zygo-mcp/src/tools/work.rs` has no CC-5 row (its row `:322` is CC-13, in the test range `264-328`), so the planning note's `work.rs:137` is not this node's.
- Tool-name parse (planning note 2): drifted. `nav_tool` (`hooks.rs:367-376`) does not split a name into `(server, tool)`: for each known tool `t` of `NAV_TOOLS` (`:285`) it strips `t` from the end, then the client's frame (Claude Code: prefix `mcp__`, suffix `__`; OpenCode: suffix `_`), and tests the server for the end `act-lite`. OpenCode's form is `<server>_<tool>`, and both parts can hold `_`: `fixtures/opencode/load-result.json` lists `zygo_probe_tool` and `user-mcp_probe_tool` (server `zygo`, tool `probe_tool`); OpenCode servers from plugins are `<plugin>-<name>` (`crates/zygo-launch/src/opencode.rs:409`, `:418`) with names from `.mcp.json`. So a name alone has no one `(server, tool)` split, and a split at the first or the last `_` changes the result for `symbols_batch`, `repo_outline` or a server with `_` (K-D3). Action (author's call): the descriptor function takes the model-visible name and the MCP tool name and gives the server; K§6.1.2 row 5 is rewritten as final text with a `## Change log` line, and this file's Produces line follows it. It is still a pure function on `&str` in `client/<client>.rs` (K§6.1.1: no new `zygo-core` dependency).
- `compact` listing (planning note 3): holds. `crates/zygo-mcp/src/tools/mod.rs:22` lists `compact::Compact` when `ctx.agent == Agent::ClaudeCode && ctx.features.compact_tool`. Tests `compact_listed_for_claude_code_and_returns_compaction_requested` and `compact_absent_for_opencode` (`crates/zygo-mcp/tests/mcp_server.rs:297`, `:309`) pin both clients.
- `window.rs` (CC-4 round 1 observation: a textual conflict with CC-5): does not apply. CC-4 is merged into `phase-4` (`a081c87b`), and this layer branches after it. `compact_tool` (`window.rs:93-99`) is Claude Code's form (`mcp__<server>__compact`, server ending in `zygo`, §10.2.3 trigger 2); its callers are `messages_line` (`:163`) and `responses_line` (`:206`, after `flat_name` for a namespaced item, K§6.4.5). Both run only in line mode (`on_response_line`, `:136-148`), which `on_request` (`:130-136`) arms only for a client with `window_rewrite` (CC-4: Claude Code only). So the session client is always Claude Code where it is called today.
- `hooks.rs:371-372` (CC-4 note: CC-5 or CC-6): CC-1 gives both rows to CC-5. CC-6's `hooks.rs` rows are `:171`, `:196`, `:197`, `:445` (concerns 2 and 3); CC-6 does not touch `nav_tool`. After this node, `hooks.rs` and `tools/mod.rs` have no production line with `Agent::ClaudeCode` or `Agent::OpenCode` (the only ones are `hooks.rs:371-372` and `mod.rs:22`).
- No other production code parses an MCP tool name: the searches `mcp__`, `ends_with("zygo")`, `ends_with("act-lite")` over `crates/*/src` find only `window.rs:93-98`, `hooks.rs:371-374`, the Claude Code adapter's hook matchers (`crates/zygo-launch/src/claude_code.rs:483-484`, CC-9's file, adapter-owned) and a doc line of `translate/responses_client.rs:708` (wire format, K§6.4.3).
- Spec: one section changed (K§6.1.2 row 5, above). K§9 lists `zygo-work/src/hooks.rs` and `zygo-mcp/src/tools/mod.rs`; `window.rs` is added to the row's replaced sites from the CC-1 inventory.

Interfaces (live at `a081c87b`):

- Consumes `Agent::spec` (CC-3): `pub const fn spec(self) -> &'static ClientSpec` at `crates/zygo-core/src/client/mod.rs:102`.
- Consumes `ClientSpec` (CC-3, with CC-4's fields): `#[derive(Debug, Clone)] pub struct ClientSpec { pub id: &'static str, pub name: &'static str, pub program: &'static str, pub verb: &'static str, pub config_dir: &'static str, pub install_hint: &'static str, pub min_version: &'static str, pub supported: bool, pub status_line: bool, pub hook_input: fn(Value) -> Value, pub dialect: Dialect, pub request_class_header: Option<&'static str>, pub compaction_header: Option<&'static str>, pub agent_id_header: Option<&'static str>, pub agent_type_header: Option<&'static str>, pub window_rewrite: bool, pub passthrough_credential: Option<&'static str>, pub cache_markers: bool, pub redact_skip_tags: &'static [&'static str], pub redact_harness_prefixes: &'static [&'static str], pub redact_harness_spans: &'static [&'static str] }` at `crates/zygo-core/src/client/mod.rs:51-95`, re-exported as `zygo_core::ClientSpec` (`crates/zygo-core/src/lib.rs:13`). Its two literals: `crates/zygo-core/src/client/claude_code.rs:7-36`, `crates/zygo-core/src/client/opencode.rs:7-37`.
- Consumes `inventory` (CC-1): the 11 CC-5 rows above, each changed by a step: `mod.rs:22` (step 4), `hooks.rs:371`, `:372` (step 5), `window.rs:94`, `:96`, `:163`, `:206` and `tests/window.rs:29`, `:452`, `:453`, `:455` (step 6; `:29` is the import, which keeps its text).
- Consumes `check-test-edits.sh` (CC-2): `scripts/check-test-edits.sh <parent> <node file>` at the parent, run in the fail-closed form of step 7.
- Kept signatures: `fn nav_tool(name: &str, agent: Agent) -> bool` (`hooks.rs:367`, private); `pub fn build_with(ctx: &McpCtx, results: impl FnOnce(&McpCtx) -> Vec<Box<dyn Tool>>, work: impl FnOnce(&McpCtx) -> Vec<Box<dyn Tool>>) -> anyhow::Result<Vec<Box<dyn Tool>>>` (`tools/mod.rs:16-20`).
- Produces (exact): two public fields added to `ClientSpec` after `redact_harness_spans`; `ClientSpec` keeps `#[derive(Debug, Clone)]`:
  - `pub mcp_server: for<'a> fn(&'a str, &str) -> Option<&'a str>`: the MCP server of the model-visible tool name (first argument) when it names the MCP tool (second argument) in the client's form; `None` when it does not.
  - `pub lists_compact_tool: bool`: zygo's MCP server lists its `compact` tool to the client (when `features.compact_tool` is on).
  - Changed: `pub fn compact_tool(client: &ClientSpec, name: &str) -> bool` in `zygo_proxy::window` (was `pub fn compact_tool(name: &str) -> bool`).
  - New private functions: `pub(super) fn mcp_server<'a>(name: &'a str, tool: &str) -> Option<&'a str>` in `client/claude_code.rs` and in `client/opencode.rs`.

Descriptor values (K-D3: today's behavior):

| Field | `claude_code::SPEC` | `opencode::SPEC` | Source |
|---|---|---|---|
| `mcp_server` | `claude_code::mcp_server`: `name` with `tool` stripped from its end, then `mcp__` from its start, then `__` from its end (the base order) | `opencode::mcp_server`: `name` with `tool` stripped from its end, then `_` from its end | `hooks.rs:369-372`, `window.rs:94-97`; `fixtures/opencode/load-result.json` |
| `lists_compact_tool` | `true` | `false` | `tools/mod.rs:22`; base spec §10.2.3 |

Behavior per client (K-D3: no change; each case with its test):

- Navigation line (§10.5): a Claude Code session records `mcp__<server>__<t>` and an OpenCode session `<server>_<t>`, server ending in `act-lite`, `t` in `NAV_TOOLS`. Forged input: an OpenCode-form name in a Claude Code session (`act-lite_skeleton`) and a Claude Code-form name in an OpenCode session (`mcp__act-lite__skeleton`: OpenCode's frame gives the server `mcp__act-lite_`) record no navigation line, as at the base. Test `navigation_reads_the_client_tool_names` (step 2; it passes at the parent and at the head).
- `compact` listing (§10.2.3, K§6.1.2 concern 5): listed only for a client with `lists_compact_tool` and `features.compact_tool` on: Claude Code yes, OpenCode never. Tests `compact_listing_reads_the_descriptor` (step 2), `compact_listed_for_claude_code_and_returns_compaction_requested`, `compact_absent_for_opencode`, `feature_switches_remove_their_tools_and_skip_their_seams` (`mcp_server.rs`).
- Trigger 2 (§10.2.3, K§6.4.5): the stage reads the session client's form. Forged input: OpenCode's form `zygo_compact` in a Claude Code session is not a compact call (as at the base). An OpenCode session never reaches trigger 2, because its window is never armed (`window_rewrite` `false`, §10.2.5), so any compact name it sends changes nothing (as at the base); `compact_tool(<OpenCode>, "zygo_compact")` is `true` as a function value only. Tests `compact_tool_reads_the_client_form` (step 2), `unit_windows`, `compact_call_rewritten_any_size`, `compact_name_matched_by_parts`, `bound_armed_drops_content_length` (its OpenCode part) in `tests/window.rs`; `compact_call_rewrites_on_both_forms` and `unarmed_bytes_pass_unchanged` in `tests/responses_window.rs` (a namespaced Responses call, flattened to `mcp__zygo__compact`, in a Claude Code session).

Steps:

- [x] 1. **Parent line.** When the node starts, insert as the first line under `## Plan` the line `Parent: <40-hex>` = `git merge-base HEAD <layer base branch>`, where the layer base branch is the branch of the open layer directly below this one, or `origin/phase-4` when the stack below is empty (Workflows (2); CC-2 Definitions "Parent"). CC-12's layer is open but shares no file with this node (its Files are registry, grade and generated conformance files), so, as in CC-3 and CC-4, the layer branches from `origin/phase-4`. After every merge into the node branch, recompute the line and rewrite it in the merge commit. Check: `sed -n 's/^Parent: \([0-9a-f]\{40\}\)$/\1/p' docs/work/phase4/nodes/CC-5.md` prints exactly one hash, equal to `git merge-base HEAD <layer base branch>`. Commit `docs(work): CC-5 parent`.
- [x] 2. **Failing tests**, four new files (K-C7 does not check a file that is new in the node). No test names an `Agent` variant by path (CC-13, CC-14): a client comes from its id.
  - `crates/zygo-core/tests/client_tools.rs`:
    ```rust
    use zygo_core::{Agent, ClientSpec};

    fn spec(id: &str) -> &'static ClientSpec {
        id.parse::<Agent>().unwrap().spec()
    }

    #[test]
    fn client_specs_hold_the_concern_5_values() {
        let c = spec("claude-code");
        assert!(c.lists_compact_tool);
        assert_eq!((c.mcp_server)("mcp__zygo__compact", "compact"), Some("zygo"));
        assert_eq!((c.mcp_server)("mcp__plugin_x_zygo__compact", "compact"), Some("plugin_x_zygo"));
        assert_eq!((c.mcp_server)("mcp__act-lite__symbols_batch", "symbols_batch"), Some("act-lite"));
        assert_eq!((c.mcp_server)("mcp__act-lite__symbols_batch", "batch"), None);
        assert_eq!((c.mcp_server)("mcp____compact", "compact"), Some(""));
        for n in ["mcp__zygo__compactx", "zygo__compact", "compact", "zygo_compact", "mcp_zygo_compact"] {
            assert_eq!((c.mcp_server)(n, "compact"), None, "{n}");
        }
        let o = spec("opencode");
        assert!(!o.lists_compact_tool);
        assert_eq!((o.mcp_server)("zygo_probe_tool", "probe_tool"), Some("zygo"));
        assert_eq!((o.mcp_server)("user-mcp_probe_tool", "probe_tool"), Some("user-mcp"));
        assert_eq!((o.mcp_server)("act101-act-lite_symbols_batch", "symbols_batch"), Some("act101-act-lite"));
        assert_eq!((o.mcp_server)("mcp__zygo__compact", "compact"), Some("mcp__zygo_"));
        for n in ["zygocompact", "compact", "zygo_compactx"] {
            assert_eq!((o.mcp_server)(n, "compact"), None, "{n}");
        }
    }
    ```
  - `crates/zygo-mcp/tests/client_tools.rs`:
    ```rust
    use zygo_config::Features;
    use zygo_core::Agent;
    use zygo_mcp::{tools, McpCtx, Tool};

    #[test]
    fn compact_listing_reads_the_descriptor() {
        for a in Agent::ALL {
            for on in [true, false] {
                let ctx = McpCtx { agent: a, features: Features { compact_tool: on, ..Features::default() }, session_dir: None };
                let names: Vec<String> = tools::build_with(&ctx, |_| vec![], |_| vec![]).unwrap().iter().map(|t| t.def().name).collect();
                let want: Vec<String> = if on && a.spec().lists_compact_tool { vec!["compact".to_string()] } else { vec![] };
                assert_eq!(names, want, "{a} compact_tool={on}");
            }
        }
        assert!("claude-code".parse::<Agent>().unwrap().spec().lists_compact_tool);
        assert!(!"opencode".parse::<Agent>().unwrap().spec().lists_compact_tool);
    }
    ```
  - `crates/zygo-proxy/tests/client_tool_names.rs`:
    ```rust
    use zygo_core::Agent;
    use zygo_proxy::window::compact_tool;

    #[test]
    fn compact_tool_reads_the_client_form() {
        let cc = "claude-code".parse::<Agent>().unwrap().spec();
        let oc = "opencode".parse::<Agent>().unwrap().spec();
        assert!(compact_tool(cc, "mcp__zygo__compact"));
        assert!(compact_tool(cc, "mcp__plugin_x_zygo__compact"));
        assert!(!compact_tool(cc, "zygo_compact"));
        assert!(!compact_tool(cc, "mcp__zygox__compact"));
        assert!(compact_tool(oc, "zygo_compact"));
        assert!(compact_tool(oc, "plugin-zygo_compact"));
        assert!(!compact_tool(oc, "mcp__zygo__compact"));
        assert!(!compact_tool(oc, "zygo__compactx"));
    }
    ```
  - `crates/zygo-work/tests/nav_client.rs`, with the helpers `no_config` and `no_item` copied from `crates/zygo-work/tests/collection.rs:63`, `:65`:
    ```rust
    use std::path::Path;

    use serde_json::{json, Value};
    use zygo_config::Merged;
    use zygo_core::hook::HookInput;
    use zygo_core::session_dir::SessionDir;
    use zygo_core::Agent;
    use zygo_work::hooks::{self, HookCtx};
    use zygo_work::{log, WorkItem};

    fn no_config(_: &Path) -> Option<Merged> { None }
    fn no_item(_: &Path, _: &Merged) -> Option<WorkItem> { None }
    fn no_warn(_: &str) {}

    /// True when one `PostToolUse` call of `tool` with `{"path": "src/lib.rs"}`, in a session of the client `id`,
    /// writes a `navigated` line to the lead's work log.
    fn navigated(id: &str, tool: &str) -> bool {
        let t = tempfile::tempdir().unwrap();
        let sd = SessionDir::new(t.path().join("S1"));
        sd.create().unwrap();
        let ctx = HookCtx { session_dir: &sd, session: "S1".into(), usage_db: t.path().join("usage.db"), now: 1000,
            config: &no_config, item: &no_item, warn: &no_warn, temp_roots: vec![] };
        let v = json!({"session_id": "S1", "hook_event_name": "PostToolUse", "tool_name": tool,
            "tool_input": {"path": "src/lib.rs"}, "tool_response": {}, "cwd": t.path().to_str().unwrap()});
        let input = HookInput::parse(id.parse::<Agent>().unwrap(), v.to_string().as_bytes()).unwrap();
        hooks::post_tool_with(&input, &ctx).unwrap();
        std::fs::read_to_string(log::log_path(&sd.work(), "main")).unwrap_or_default().lines()
            .any(|l| serde_json::from_str::<Value>(l).unwrap()["t"] == "navigated")
    }

    #[test]
    fn navigation_reads_the_client_tool_names() {
        for (id, tool, want) in [
            ("claude-code", "mcp__act-lite__skeleton", true),
            ("claude-code", "mcp__plugin_act101_act-lite__symbols_batch", true),
            ("claude-code", "act-lite_skeleton", false),
            ("opencode", "act-lite_skeleton", true),
            ("opencode", "act101-act-lite_symbols_batch", true),
            ("opencode", "act-lite_repo_outline", true),
            ("opencode", "mcp__act-lite__skeleton", false),
            ("opencode", "act-lite__skeleton", false),
            ("opencode", "other_skeleton", false),
        ] {
            assert_eq!(navigated(id, tool), want, "{id} {tool}");
        }
    }
    ```
  - Check: `cargo test -p zygo-core --test client_tools --locked`, `cargo test -p zygo-mcp --test client_tools --locked` and `cargo test -p zygo-proxy --test client_tool_names --locked` fail to compile (no field `lists_compact_tool`, `mcp_server`; `compact_tool` takes 1 argument); `cargo test -p zygo-work --test nav_client --locked` passes (it pins today's result, K-D3). Commit `test(core): CC-5 concern 5 descriptor tests (failing)`.
  - Note (implementation): the checks hold: the `zygo-core`, `zygo-mcp` and `zygo-proxy` targets fail to compile (13, 3 and 8 errors: no field `lists_compact_tool`/`mcp_server`; `compact_tool` takes 1 argument); `nav_client` passes (1 passed). Plan error: the `zygo-mcp` test imported `Tool`, which is unused (a method call on `Box<dyn Tool>` needs no trait import; rustc warns `unused import: Tool`). The file imports `zygo_mcp::{tools, McpCtx}`.
- [ ] 3. **Descriptor fields.** In `crates/zygo-core/src/client/mod.rs`, add the two fields of "Produces (exact)" after `redact_harness_spans`, each with a one-line doc comment, and change the struct doc (`:48-50`) to name concern 5 (MCP tool naming). In `client/claude_code.rs` and `client/opencode.rs`, add `pub(super) fn mcp_server<'a>(name: &'a str, tool: &str) -> Option<&'a str>` with the rule of the table (a doc line names the client's form: `mcp__<server>__<tool>`, `<server>_<tool>`), and set `mcp_server` and `lists_compact_tool` in `SPEC` to the table values. Check: `cargo test -p zygo-core --locked --no-fail-fast` passes (`client_tools`, `client`, `client_proxy`); `cargo test -p zygo-mcp --test client_tools --locked` passes (the listing at `tools/mod.rs:22` already equals the descriptor values); `cargo build --workspace --locked` passes (no reader yet; `--all-targets` is not used, because `zygo-proxy --test client_tool_names` builds only after step 6). Commit `feat(core): ClientSpec concern 5 fields`.
- [ ] 4. **`compact` listing.** `crates/zygo-mcp/src/tools/mod.rs:22`: the condition is `ctx.agent.spec().lists_compact_tool && ctx.features.compact_tool`; remove `use zygo_core::Agent;` (`:7`, no other use). Check: `cargo test -p zygo-mcp --locked --no-fail-fast` passes (`client_tools`; `mcp_server` with `compact_listed_for_claude_code_and_returns_compaction_requested`, `compact_absent_for_opencode`, `feature_switches_remove_their_tools_and_skip_their_seams`, `duplicate_tool_names_are_refused_naming_the_name`). Commit `refactor(mcp): compact listing reads the descriptor`.
- [ ] 5. **Navigation tool names.** `crates/zygo-work/src/hooks.rs:367-376`: `nav_tool(name, agent)` keeps its signature; it is true when, for a `t` of `NAV_TOOLS`, `(agent.spec().mcp_server)(name, t)` is a server that ends with `act-lite`; the `match` on the variants is removed. Add a doc line: the act-lite navigation tools in the client's MCP tool-name form (`ClientSpec::mcp_server`, §10.5). Check: `cargo test -p zygo-work --locked --no-fail-fast` passes (`nav_client`; `collection` with `post_navigation_tool_appends_absolute_paths`, `post_overlong_line_is_dropped_alone`; `guards` (`:546`, a Claude Code navigation call)). Commit `refactor(work): navigation tool names read the descriptor`.
- [ ] 6. **Trigger 2 tool name.** One commit, so each commit builds:
  - `crates/zygo-proxy/src/window.rs:93-99`: `pub fn compact_tool(client: &ClientSpec, name: &str) -> bool` is true when `(client.mcp_server)(name, "compact")` is a server that ends with `zygo`; the doc line says "the `zygo` MCP server's `compact` tool in the client's MCP tool-name form (`ClientSpec::mcp_server`), its server ending in `zygo` (§10.2.3 trigger 2)". Add `use zygo_core::ClientSpec;`. `:163`: `b["name"].as_str().is_some_and(|n| compact_tool(ctx.session.agent.spec(), n))`; `:206`: `compact_tool(ctx.session.agent.spec(), &name)`. The `flat_name` join (`:202-205`, K§6.4.5 wire format) stays.
  - `crates/zygo-proxy/tests/window.rs:452-455` (`unit_windows`): before `:452` add `let cc = "claude-code".parse::<Agent>().unwrap().spec();`; each `compact_tool(<name>)` becomes `compact_tool(cc, <name>)`; every name and result is kept. `:29` keeps its text (`compact_tool` is still imported). This is the one `## Test logic edits` entry (`+"claude-code" x1`).
  - Check: `cargo test -p zygo-proxy --locked --no-fail-fast` passes: `client_tool_names`; `window` (`unit_windows`, `compact_call_rewritten_any_size`, `compact_name_matched_by_parts`, `bound_armed_drops_content_length`); `responses_window` (K§11 item 21: `compact_call_rewrites_on_both_forms`, `unarmed_bytes_pass_unchanged`); `responses_cache` (K§11 item 19). Commit `refactor(proxy): compact tool name reads the descriptor`.
- [ ] 7. **Acceptance**, the node's Acceptance line unchanged, run alone as one command; read its output. It passes when the variant-path check prints nothing for `crates/zygo-mcp/src/tools/mod.rs` and `crates/zygo-work/src/hooks.rs`, the `zygo-work`, `zygo-mcp` and `zygo-core` tests pass, and the K-C7 gate in its fail-closed form, `S=$(git show "<parent>:scripts/check-test-edits.sh") && printf '%s\n' "$S" | sh -s -- "<parent>" docs/work/phase4/nodes/CC-5.md` with `<parent>` from the `Parent:` line, exits 0 (Verification floor; a parent without the script fails `git show`, so the gate fails closed). `## Test logic edits` holds one entry, `literals crates/zygo-proxy/tests/window.rs: +"claude-code" x1` (step 6); the four other test files of this node are new at the parent, which K-C7 does not check. If the gate prints any other offence, the edit differs from this plan: make the edit match the plan; do not add an entry that copies the printed delta.
- [ ] 8. **Verification floor.** `cargo test --workspace --locked --no-fail-fast` → 0 failed (`zygo-proxy` runs here, since it is not in the Acceptance crates); `ZYGO_WRITE_CONFORMANCE=1 cargo test -p zygo-core --test conformance --locked`, then `git diff --exit-code docs/conformance.md docs/conformance.json` → exit 0; `N2_FORBIDDEN_NAMES=zzcinameq sh scripts/check-premises.sh` → exit 0; `sh scripts/test-install.sh` → `all passed`; `git status --porcelain` empty. Push; link the layer into the stack (`gh stack link --base phase-4 …`, Workflows (2)); CI `test` on ubuntu, macOS and Windows green on the layer head; review gate (Workflows (1)).

Review focus (graph): no line names CC-5. Line 13 (window rewrite on an unarmed session) holds through the RS-7 tests that step 6 runs unchanged (`responses_window`); line 14 (a test edit that changes behavior) is step 7's gate, and the one listed edit keeps every asserted name and result of `unit_windows`. Line 12 (cache markers) is not touched (no `cache.rs` or marker change). The wire-format stages of `window.rs` (`ctx.lines`, `flat_name`) stay keyed on `Dialect` (K§6.1.4, K-D47); this node changes no body path (K-C1).

Author's checks (2026-11-28):

- Spec coverage: K§6.1.2 concern 5 has two facts, each one field with its readers: the tool-name function (`hooks.rs` `nav_tool`, `window.rs` `compact_tool`) and the `compact` listing (`tools/mod.rs`). K§6.1.1: the functions are pure, on `&str`, in `client/<client>.rs`; `zygo-core` gets no dependency. K§9: `hooks.rs` and `tools/mod.rs` read the descriptor. K-D3: every value is today's (table sources), and each behavior per client, forged input included, has its test (above).
- Step scan: each step is one change with a check command that holds at that step; step 3 adds unread fields (the proxy test target builds only after step 6, so step 3 builds without `--all-targets`); step 6 changes `compact_tool`'s signature, its two callers and its one test caller in one commit.
- Interface match: CC-3's `Agent::spec` and CC-3/CC-4's `ClientSpec` are as live (above); this node adds fields after CC-4's last field. CC-6 (consumer of the same descriptor, not of a CC-5 product) adds its concern 3 and 4 fields to the same two `SPEC` literals and edits `hooks.rs` only at its rows `:171`, `:196`, `:197`, `:445`, which this plan does not touch; CC-6's Consumes line (`Agent::spec`, `ClientSpec`, `inventory`, `check-test-edits.sh`) holds.
- Proportion: two fields, two private functions, four call-site edits, one test edit; the existing tests of both clients carry the no-change proof, and four small new test files pin the descriptor values and the forged-input cases.
- Live code: every cited path and line was opened at `a081c87b` (`client/mod.rs`, `client/claude_code.rs`, `client/opencode.rs`, `hooks.rs`, `tools/mod.rs`, `tool.rs`, `features.rs`, `window.rs`, `tests/window.rs`, `tests/responses_window.rs`, `tests/mcp_server.rs`, `tests/collection.rs`, `fixtures/opencode/load-result.json`, `crates/zygo-launch/src/opencode.rs`). No existing function gives a client's MCP tool-name form; `nav_tool`, `compact_tool` and `build_with` are kept and changed in place.
