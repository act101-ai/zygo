# zygo phase 5 roadmap

**Status:** started 2026-11-25. Items only; no spec yet. Next: one spec per track (see "Plan of work").
**Base specs:** `docs/specs/2026-11-18-zygo-design.md` (`§n`), `docs/specs/2026-11-21-statusline-hud-design.md` (`H§n`, decision ids `H OPn`, `H AUn`), `docs/specs/2026-11-22-workspace-mail-design.md` (`M§n`), `docs/specs/2026-11-24-session-runtime-design.md` (`T§n`), `docs/specs/2026-11-25-client-model-design.md` on `origin/phase-4-spec-client` (`K§n`, proposed, not approved).

Code references are `file:line` at commit `bf3726bd` (branch `phase-4`, equal to `main`). Phase 3 references are to `docs/work/phase3/graph.md` on `origin/phase-3`. Phase 4 item ids (`P4-n`) are those of `docs/roadmap/phase-4.md` on `origin/phase-4-spec-client`.

## Intent

The operator's request (2026-11-25) **(operator)**: "start a phase 5 roadmap that includes: statusline customization (but not the logo), a plugin model for tools - migrate internal tools to the plugin model."

Phase 5 has two tracks:

1. **Status line customization (operator).** The user can change the HUD. The logo is excluded **(operator)**: it stays fixed and always shows (H OP11, `docs/specs/2026-11-21-statusline-hud-design.md:92`; H AU8, `:103`).
2. **Tool plugins (operator).** Each zygo MCP tool gets a plugin model. All internal tools move to that model **(operator)**, with no change of behavior for the user.

### Status line: what exists

The HUD is fixed by an operator decision: "The HUD is fixed. No segment config. Exceptions: OP6 and OP7." (H OP4, `docs/specs/2026-11-21-statusline-hud-design.md:85`; non-goal H§3, `:43`; rejected alternative, `:528`). The segment order and the gaps are fixed (H AU9, `:104`; H§6.2). The operator's 2026-11-25 request replaces H OP4. The track spec amends H OP4, H§3, H AU9 and H§6.7, and it says so in its base-spec changes.

What a user can change today:

- **Style.** `[statusline] style = "compact" | "verbose"`, user config only (`crates/zygo-config/src/schema.rs:628-643`). The launcher writes it into the session view (`crates/zygo-core/src/session_view.rs:32-33`). The session verb `zygo statusline <style> [--save]` writes `overrides.json` (`crates/zygo/src/cmd/statusline.rs:57`). The hook reads `overrides.json`, then the view, then the default (`crates/zygo-hooks/src/statusline/mod.rs:25-34`).
- **Icon set.** Only the environment: `ZYGO_NERD_FONT=1|0`, else `TERM` and the platform select it (`crates/zygo-hooks/src/statusline/render.rs:126-138`; H AU2, `:97`). No config key exists.
- **Color.** Only the environment: `NO_COLOR` turns color off; `COLORTERM`, `TERM` and `WT_SESSION` select 256 or 16 colors (`render.rs:140-150`). No config key exists.

What a user cannot change today: the segments that show, their order and groups (constants `COMPACT`, `ROW_1`, `ROW_2`, `render.rs:428-449`), the width-drop order (constants `COMPACT_DROPS`, `ROW_1_DROPS`, `ROW_2_DROPS`, `render.rs:451-476`), the colors of a segment, and the maximum widths (`render.rs:479-484`).

Customization covers: which segments show, their order and groups, compact or verbose, the icon set, the colors, and the width-drop order. It also covers the segments that phases 3 and 4 add:

- Member name and unread mail (Phase 3 ML-15; M§9a, `docs/specs/2026-11-22-workspace-mail-design.md:136`). These are never removed for width (`docs/work/phase3/graph.md:50` on `origin/phase-3`).
- Session run time (Phase 3 RT-3; T§2.4, `docs/specs/2026-11-24-session-runtime-design.md:48`).
- Update notice, segment 16 (P4-7; K§6.5.5, `docs/specs/2026-11-25-client-model-design.md:556`, `:560` on `origin/phase-4-spec-client`).

The status surface differs per client. Claude Code has the HUD through `statusLine` (`crates/zygo-launch/src/claude_code.rs:534`). OpenCode has no status surface and uses `zygo status` (H§3, `docs/specs/2026-11-21-statusline-hud-design.md:47`). Codex shows only built-in status items (K§6.5.5, `docs/specs/2026-11-25-client-model-design.md:561`). Capability A-9 grades the status surface (K§6.2.2, `:230`). The status surface is a descriptor fact of the Phase 4 client contract (K§6.1.2 concern 1, `:163`). Thus the client contract (P4-4) owns this difference, and customization reaches each client through it.

### Tools: what exists

The `zygo mcp` server lists these tools (`crates/zygo-mcp/src/tools/`):

| Tool | File | Gate in `build_with` (`crates/zygo-mcp/src/tools/mod.rs:15-37`) |
|---|---|---|
| `compact` | `tools/compact.rs:9-23` | Claude Code only and `[features] compact_tool` (`mod.rs:21`; §10.2.3, `docs/specs/2026-11-18-zygo-design.md:500` "Listed for Claude Code only") |
| `result_view`, `result_search`, `result_tree` | `tools/results.rs:8-12` | `[features] results` (`mod.rs:24`) and a session directory (`results.rs:9`) |
| `work_begin`, `work_verdict` | `tools/work.rs:11-14` | `[features] work_units` (`mod.rs:27`) and a session directory (`work.rs:12`) |
| `workspace_down` | `tools/workspace_down.rs:38-46` | `[features] workspace_tool`, a session directory and a workspace membership marker (`workspace_down.rs:39-44`) |
| `mail_send`, `mail_read`, `mail_list` | Phase 3 ML-11 (`docs/work/phase3/graph.md:248` on `origin/phase-3`) | `[features] mail`, member only (`graph.md:171`; M§8, `docs/specs/2026-11-22-workspace-mail-design.md:92`) |

How a tool is registered: each tool is a Rust type that implements the `Tool` trait (`def`, `call`; `crates/zygo-mcp/src/tool.rs:43-46`). `build_with` puts the tools in one list with hard-coded gates and refuses two tools of one name (`mod.rs:31-35`). The server answers `tools/list` and `tools/call` from that list (`crates/zygo-mcp/src/server.rs:107-108`). The gates read `McpCtx` (`tool.rs:49-53`): the client, the `[features]` toggles (`crates/zygo-config/src/features.rs:7-15`) and the session directory. The launcher registers the server as `zygo mcp --agent <client>` (Claude Code `crates/zygo-launch/src/claude_code.rs:613-619`; OpenCode `crates/zygo-launch/src/opencode.rs:150-151`).

The `compact` gate is a client branch (`mod.rs:21`). The Phase 4 client contract moves it into the descriptor (K§6.1.2 concern 5, `docs/specs/2026-11-25-client-model-design.md:167`).

Code outside `zygo-mcp` also depends on the tool names:

- The proxy finds a `compact` call by its name `mcp__<server>__compact` (`crates/zygo-proxy/src/window.rs:93-99`).
- The Claude Code launch adds `PreToolUse` hooks for `mcp__zygo__work_begin` and `mcp__zygo__work_verdict` (`crates/zygo-launch/src/claude_code.rs:483-484`). The OpenCode plugin runs the `work` hook after each tool (`crates/zygo-launch/assets/opencode-plugin.js:183`).
- The session text names `result_view`, `result_search` and `result_tree` (`crates/zygo-hooks/src/session_text.rs:9`); the pre-compact text names `result_view` (`crates/zygo-hooks/src/pre_compact.rs:76`).
- The model-visible name form differs per client: Claude Code `mcp__<server>__<tool>`, OpenCode `<server>_<tool>` (`docs/conformance.md:31`, `:34`).
- `compact` keeps its name, because act-lite's skills call a tool of that name (§15.4, `docs/specs/2026-11-18-zygo-design.md:745`).

The plugin system for skills and plugins (§15, `docs/specs/2026-11-18-zygo-design.md:708-781`; crate `crates/zygo-plugins`):

- Marketplaces with a pinned commit (§15.1, `:710`); selection by `zygo plugins enable|disable` and profiles (§15.2, `:724`); a per-session snapshot (§15.3, `:730`; `crates/zygo-plugins/src/snapshot.rs:237`).
- Feature toggles for built-in features, which include the three tool toggles (§15.4, `:743`).
- The embedded `zygo` plugin, compiled into the binary, with commands and one skill and no code (§15.5, `:747`; `crates/zygo-plugins/src/embedded.rs:1-2`).
- A plugin check (§15.6, `:751`; `crates/zygo-plugins/src/check.rs:148`).
- Code approval by code digest, stored in `approvals.toml`; built-in digests pre-approved in the binary (§15.7, `:761-766`; `crates/zygo-plugins/src/approve.rs:1`, `crates/zygo-plugins/src/builtin.rs:1-2`).

A plugin can already ship an MCP server through `.mcp.json` (§15.1, `:720`), and that server is code under §15.7. zygo's own tools are not in this system: they are compiled into `zygo mcp` and gated by `[features]`.

A tool plugin gives a tool: a manifest; a capability declaration; the clients it supports; enable and disable; and approval as §15.7 gives for plugin code.

## Items

| Id | Item | Depends | Spec |
|---|---|---|---|
| P5-1 | Segment registry: each HUD segment (H§6.2 segments 0 to 15, member name, mail, run time, update notice 16) is one entry with an id, a default place, a default color, a drop rank and a "never removed" flag. The current fixed HUD is the default layout, byte for byte (the H§14 golden files do not change) | Phase 3 ML-15, RT-3 merged; P4-7 | to write (status line) |
| P5-2 | Layout config: which segments show, their order and groups for compact, verbose row 1 and verbose row 2, and the width-drop order; `zygo config check` refuses a bad layout. The stripe and the logo stay fixed and first **(operator: the logo)** | P5-1 | to write (status line) |
| P5-3 | Icon set and colors in config: an icon set key next to `ZYGO_NERD_FONT`, and the color of each segment; `NO_COLOR` and the 16/256-color rule (H AU23) still apply | P5-1 | to write (status line) |
| P5-4 | Scope and verb: the config layers that can set the layout, the icons and the colors; the session form of `zygo statusline`; a preview in `zygo doctor` | P5-2, P5-3 | to write (status line) |
| P5-5 | Per-client status surface: the customization reaches each client through the client contract (Claude Code HUD; `zygo status` for OpenCode and Codex); the A-9 criteria cover it | P4-1, P4-4, P5-4 | to write (status line) |
| P5-6 | Tool plugin model: a manifest for each tool (name, description, input schema, capability declaration, supported clients, enable key, required session state); one registry takes the place of the hard-coded gates of `build_with` (`crates/zygo-mcp/src/tools/mod.rs:15-37`) | P4-4 | to write (tool plugins) |
| P5-7 | Tool selection and approval: enable and disable through §15.2 selection and the §15.4 toggles; approval of tool code as §15.7; the internal tools pre-approved as the built-in digests are | P5-6 | to write (tool plugins) |
| P5-8 | Migration of each internal tool to the model: `compact`, `result_view`, `result_search`, `result_tree`, `work_begin`, `work_verdict`, `workspace_down`, `mail_send`, `mail_read`, `mail_list`. No change for the user: the same names, the same `tools/list` per client and per feature set, the same replies | P5-6, P5-7; Phase 3 ML-11 merged | to write (tool plugins) |
| P5-9 | Tool-name users read the registry: the proxy `compact` match (`crates/zygo-proxy/src/window.rs:93-99`), the work hook matchers (`crates/zygo-launch/src/claude_code.rs:483-484`), the session text and pre-compact text (`crates/zygo-hooks/src/session_text.rs:9`, `crates/zygo-hooks/src/pre_compact.rs:76`) | P4-4, P5-8 | to write (tool plugins) |
| P5-10 | Tool conformance: each tool's client support is in the capability registry and is graded per client (today A-6, A-17, A-19); the feature matrix shows it | P4-1, P5-8 | to write (tool plugins) |

Phase 4 dependencies, by item id:

- **P4-4 (client contract).** P5-5: the status surface is descriptor concern 1 (K§6.1.2, `docs/specs/2026-11-25-client-model-design.md:163`). P5-6 and P5-9: the MCP tool name form and the `compact` listing are descriptor concern 5 (`:167`). P4-4 itself waits for Phase 3 to merge (`docs/roadmap/phase-4.md`, P4-4 row, on `origin/phase-4-spec-client`).
- **P4-1 (capability registry).** P5-5 grades A-9 (`:230`); P5-10 grades A-6, A-17 (the `compact` tool listed and working, `:238`) and A-19 (`:240`).
- **P4-7 (update notices).** P5-1: segment 16 is in the registry (K§6.5.5, `:556`, `:560`).

## Open questions for the status line spec

The spec decides these. This roadmap does not.

1. **Config form.** A segment list per style, a format string, or named presets.
2. **Layers.** `[statusline]` is user config only today (H§8). The spec states if a profile, the team config or the repository file can set the layout, the icons or the colors.
3. **Fixed parts.** The stripe and the logo are fixed **(operator: the logo)**. The spec states if a user can hide or move the other segments that are never removed today: the workspace phase, the `offline`, `suspended` and `ended` markers (H§6.7), the member name and the mail count (M§9a).
4. **Colors.** Per-segment SGR codes or a named theme; the 16-color fallback of a 256-color value (H AU23).
5. **Icon set.** The precedence of a config key against `ZYGO_NERD_FONT` (H AU2); custom glyphs per segment or not.
6. **Width-drop order.** Its own list, or derived from the layout.
7. **Session scope.** If the session verb can change the layout, or the style only (H AU1).
8. **Clients.** How the layout applies on a client with no HUD (`zygo status`) or with built-in items only (Codex).
9. **Bad config.** The hook falls back to the default layout and writes one stderr line (C6, H-C3), or `config check` alone refuses it.
10. **Constraints that hold.** The 3 s deadline (H-C2), escaping (H-C6), and the window kept from the agent (H-C4, C7). The spec states how each segment that a user adds or moves keeps them.
11. **Link to the tool track.** If a tool plugin can add a HUD segment.

## Open questions for the tool plugin spec

The spec decides these. This roadmap does not.

1. **In process or out of process.** Internal tools as compiled Rust types in `zygo mcp`, or as separate processes; the same answer for third-party tools or not.
2. **Relation to §15.** A tool plugin as a new part of a §15 plugin, as a new kind of plugin, or as an entry of the embedded `zygo` plugin (§15.5).
3. **Client support.** How a tool declares the clients it supports, and how that links to the descriptor (K§6.1.2 concern 5) and to the conformance registry (K§6.2). Today the `compact` gate is the only one (`crates/zygo-mcp/src/tools/mod.rs:21`).
4. **Capability declaration.** What a tool can touch (session directory, usage store, a child process: `workspace_down` runs `zygo down`, `crates/zygo-mcp/src/tools/workspace_down.rs:97-104`), and how the approval shows it.
5. **Hooks and proxy coupling.** If the manifest declares the hooks that a tool needs (the work hooks, `crates/zygo-launch/src/claude_code.rs:483-484`) and the proxy behavior it triggers (`compact`, `crates/zygo-proxy/src/window.rs:93-99`).
6. **Enable and disable.** How the `[features]` keys `results`, `work_units`, `compact_tool`, `workspace_tool` and `mail` map to the new model, and the rules that hold today: `workspace_tool` is not a profile key (`crates/zygo-config/src/features.rs:11-14`).
7. **Approval of internal tools.** Pre-approved by digest in the binary as §15.7 does for the built-in marketplace, or no approval because the binary is trusted.
8. **Third-party tool plugins.** If they are in scope for phase 5.
9. **No-change proof.** The check that the migration changes nothing for the user: for example a `tools/list` and reply fixture per client and per feature set, made before the migration.

## Plan of work (operator rules as in phases 3 and 4; given by the lead, 2026-11-25)

1. **One spec per track.** The status line spec covers P5-1 to P5-5. The tool plugin spec covers P5-6 to P5-10.
2. **One work graph.** One work graph covers all of Phase 5. It is made after the operator approves both specs. A spec does not plan nodes.
3. **A review gate per node.** Each work node gets its own review gate of up to 3 rounds, as in phase 3. The P3-8 disposition rule applies at each gate: after round 3, a node passes only when each finding has exactly one disposition (fixed with a commit, rejected with a reason, or recorded with a DDR id), and no finding is of a class that stops for the operator.
4. **A pull request after every row.** Each row of the work graph, planning included, is delivered in its own pull request into the rendezvous branch `phase-5`. The lead merges a row after its gate and green CI. `phase-5` reaches `main` only on the operator's word.
5. **Order against phases 3 and 4.** The specs can start now. The implementation of each track waits for P4-4. P5-1 also waits for Phase 3 ML-15 and RT-3 and for P4-7; P5-8 also waits for Phase 3 ML-11.
