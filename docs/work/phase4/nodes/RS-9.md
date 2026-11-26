# RS-9 — Codex namespace tools: flatten into function tools for Anthropic and chat upstreams, map their calls back

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.4.3 (request obligation 6, tool definitions, as RS-1 rewrites it; request obligation 3, `function_call` items; stream obligation 2, `tool_use` → a `function_call` item); K§6.4.4 is errors before the stream and does not apply
**Files:** `crates/zygo-proxy/src/translate/responses_client.rs`, `crates/zygo-proxy/tests/responses_request.rs`, `crates/zygo-proxy/tests/responses_stream.rs`, `docs/specs/2026-11-25-client-model-design.md` (K§6.4.3 and its change-log line)
**Consumes:** `request` (RS-1), `fixtures/codex/agents/` (PC-5)
**Produces:** a `namespace` tool's nested function tools become function tools named `<namespace>__<name>` (or the form the K§6.4.3 rewrite chooses; it must be a valid tool name on each upstream and reversible) on Anthropic and chat upstreams, with no `tools_dropped:namespace` adjustment; a model call to such a tool is translated back to a Codex `function_call` item with `name` and `namespace` as PC-5's captures show; K§6.4.3 rewritten as final text with a change-log line (author's call)
**Acceptance:** `test "$(cargo test -q -p zygo-proxy --test responses_request --test responses_stream -- --list 2>/dev/null | grep -cE 'namespace_tools_flatten_[a-z0-9_]*: test$')" -ge 1 && test "$(cargo test -q -p zygo-proxy --test responses_request --test responses_stream -- --list 2>/dev/null | grep -cE 'namespace_call_maps_back_[a-z0-9_]*: test$')" -ge 1 && cargo test -p zygo-proxy --locked --test responses_request --test responses_stream && cargo test --workspace --locked` → at least one `namespace_tools_flatten_*` and one `namespace_call_maps_back_*` test (a count of 0 fails), both test targets pass, and the workspace run exits 0 apart from the known `zygo --test guard_hook` environment failures: a PC-5 `fixtures/codex/agents/` request translated for an Anthropic and for a chat target carries each nested `multi_agent_v1` tool as a function tool under the flattened name and no `tools_dropped:namespace` code; a model `tool_use` of a flattened name gives a `function_call` item with `name` and `namespace` as the PC-5 captures show; a `function_call` input item with a `namespace` maps to a `tool_use` of the flattened name, so the call round-trips

## Planning notes

- Lead's decision 2026-11-25 (RS-1 plan): RS-1 drops `namespace` tools (`tools_dropped:namespace:<n>`) so its scope stays the captured bodies (PC-3), where no fixture shows a call to a tool inside one.
- Dropping loses Codex's subagent tools (`multi_agent_v1`: `spawn_agent` and the rest, five nested `function` tools in every PC-3 request) on Anthropic and chat upstreams.
- PC-5's fixtures (`fixtures/codex/agents/`, PC6) capture Codex's namespaced call items; read the `name`/`namespace` form from them before the spec rewrite.
- The call-back mapping lands in RS-2's `Encoder` (stream obligation 2) and `response` (JSON form), both in `responses_client.rs`; RS-9 so depends on RS-2 (shared files).

## Plan

