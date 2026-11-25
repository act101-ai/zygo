# zygo phase 5 roadmap

**Status:** started 2026-11-25. Items only; no spec yet. Next: one spec per track (see "Plan of work").
**Base specs:** `docs/specs/2026-11-18-zygo-design.md` (`§n`), `docs/specs/2026-11-20-workspaces-design.md` (`W§n`), `docs/specs/2026-11-21-statusline-hud-design.md` (`H§n`, decision ids `H OPn`, `H AUn`), `docs/specs/2026-11-22-workspace-mail-design.md` (`M§n`), `docs/specs/2026-11-24-session-runtime-design.md` (`T§n`), `docs/specs/2026-11-25-client-model-design.md` on `origin/phase-4-spec-client` (`K§n`, proposed, not approved).

Code references are `file:line` at commit `bf3726bd` (branch `phase-4`, equal to `main`). Phase 3 references are to `docs/work/phase3/graph.md` on `origin/phase-3`. The Phase 4 roadmap (`docs/roadmap/phase-4.md` on `origin/phase-4-spec-client`) is being renumbered. Thus this roadmap names its rows by name, not by number: the **conformance row** (capability registry, rubric and grades, K§6.2), the **client contract build row** (descriptor and adapter registries, K§6.1) and the **background update row** (update notices, K§6.5).

## Intent

The operator's request (2026-11-25) **(operator)**: "start a phase 5 roadmap that includes: statusline customization (but not the logo), a plugin model for tools - migrate internal tools to the plugin model."

The operator's addition of the same day **(operator)**: "a way for an agent working on a plan to report status to the second statusline. we'll drop the compact/expanded, keep compact on top. look at our graph work loops. easy to report on done/total rows, and a brief status report. improve on that if you want - you get the vision. line 1 is the it report. line 2 is the project manager report."

Phase 5 has three tracks:

1. **Two-line HUD and line 1 customization (operator).** The HUD has two lines with fixed roles **(operator)**. Line 1, the "IT report", is today's compact line and stays on top **(operator)**. Line 2 is the "project manager report" **(operator)**. The compact/verbose style choice is removed **(operator)**. The user can change line 1. The logo is excluded **(operator)**: it stays fixed and always shows (H OP11, `docs/specs/2026-11-21-statusline-hud-design.md:92`; H AU8, `:103`).
2. **Plan report on line 2 (operator vision).** An agent that runs a plan (a work graph) reports to line 2: done/total rows and a brief status sentence that the agent writes **(operator)**. The operator invites improvements on that base **(operator)**; the item list below is the author's proposal.
3. **Tool plugins (operator).** Each zygo MCP tool gets a plugin model. All internal tools move to that model **(operator)**, with no change of behavior for the user.

### Status line: what exists

The HUD is fixed by an operator decision: "The HUD is fixed. No segment config. Exceptions: OP6 and OP7." (H OP4, `docs/specs/2026-11-21-statusline-hud-design.md:85`; non-goal H§3, `:43`; rejected alternative, `:528`). H OP7 gives two styles (`:88`). The segment order and the gaps are fixed (H AU9, `:104`; H§6.2). The operator's 2026-11-25 decisions replace H OP4 and H OP7. The track spec amends H OP4, H OP7, H AU1, H AU9, H§3, H§6, H§8 and H§6.7, and it says so in its base-spec changes.

What a user can change today:

- **Style.** `[statusline] style = "compact" | "verbose"`, user config only (`crates/zygo-config/src/schema.rs:628-643`). The launcher writes it into the session view (`crates/zygo-core/src/session_view.rs:32-33`). The session verb `zygo statusline <style> [--save]` writes `overrides.json` (`crates/zygo/src/cmd/statusline.rs:57`). The hook reads `overrides.json`, then the view, then the default (`crates/zygo-hooks/src/statusline/mod.rs:25-34`). Compact prints one row; verbose prints two rows (`crates/zygo-hooks/src/statusline/render.rs:798-804`).
- **Icon set.** Only the environment: `ZYGO_NERD_FONT=1|0`, else `TERM` and the platform select it (`render.rs:126-138`; H AU2, `:97`). No config key exists.
- **Color.** Only the environment: `NO_COLOR` turns color off; `COLORTERM`, `TERM` and `WT_SESSION` select 256 or 16 colors (`render.rs:140-150`). No config key exists.

What a user cannot change today: the segments that show, their order and groups (constants `COMPACT`, `ROW_1`, `ROW_2`, `render.rs:428-449`), the width-drop order (constants `COMPACT_DROPS`, `ROW_1_DROPS`, `ROW_2_DROPS`, `render.rs:451-476`), the colors of a segment, and the maximum widths (`render.rs:479-484`).

With the operator's addition:

- Line 1 is today's compact row (`COMPACT`, `render.rs:428-437`; H AU9 compact groups). The default line 1 is that row, byte for byte.
- Today's verbose rows (`ROW_1`, `ROW_2`, `render.rs:439-449`) are no longer a style. Their segments that compact does not show are: git ahead/behind, effort, mode, the ctx percent (H§6.2, `:130-164`) and the logo reflection (H OP11, `:92`). Where they go is a question for the spec.
- Customization covers line 1: which segments show, their order and groups, the icon set, the colors, and the width-drop order. It also covers the segments that phases 3 and 4 add:
  - Member name and unread mail (Phase 3 ML-15; M§9a, `docs/specs/2026-11-22-workspace-mail-design.md:136`). These are never removed for width (`docs/work/phase3/graph.md:50` on `origin/phase-3`).
  - Session run time (Phase 3 RT-3; T§2.4, `docs/specs/2026-11-24-session-runtime-design.md:48`).
  - Update notice, segment 16 (background update row; K§6.5.5, `docs/specs/2026-11-25-client-model-design.md:556`, `:560` on `origin/phase-4-spec-client`).

Claude Code shows a two-row status line output as two rows (premise PH2, verified on Claude Code 2.1.330, `docs/specs/2026-11-21-statusline-hud-design.md:579`). So line 2 needs no new premise on Claude Code.

The status surface differs per client. Claude Code has the HUD through `statusLine` (`crates/zygo-launch/src/claude_code.rs:534`). OpenCode has no status surface and uses `zygo status` (H§3, `docs/specs/2026-11-21-statusline-hud-design.md:47`). Codex shows only built-in status items (K§6.5.5, `docs/specs/2026-11-25-client-model-design.md:561`). Capability A-9 grades the status surface (K§6.2.2, `:230`). The status surface is a descriptor fact of the client contract (K§6.1.2 concern 1, `:163`). Thus the client contract build row owns this difference, and both lines reach each client through it.

### Plan report: what exists

- **The work graph format.** A plan is `docs/work/<program>/graph.md` with the marker `<!-- work-graph -->`, a `**Run:**` line (for example `parallel=3`) and a `## Nodes` table with the columns `ID | Node | Depends | State | Capability | Refs` (`docs/work/phase3/graph.md:3-5`, `:190-192` on `origin/phase-3`). Each node has `nodes/<id>.md`; closed rows move to `history.md`.
- **States.** `TODO`, `PLANNED`, `IN-PROGRESS`, `DONE`, `BLOCKED(<reason>)`, `DROPPED(<evidence>)` (act-lite `work_graph.py`, `STATE_RE`, line 36).
- **What the script derives.** `status` prints the count per state, the ready rows, the schedule, the stuck rows and the history count (`work_graph.py:588-608`). A ready row is `TODO` or `PLANNED` with each dependency `DONE` (`:542-547`). The schedule is the `IN-PROGRESS` rows, then ready rows up to `parallel`, with no two rows that share a file (`:550-566`). A stuck row is `BLOCKED`, or depends on a `BLOCKED` or `DROPPED` row (`:569-585`). All rows include `history.md` (`:100-101`), so done/total counts archived rows too.
- **What zygo reads today.** The `work-graph` work-item source (§13.2, `docs/specs/2026-11-18-zygo-design.md:626`) reads the `## Nodes` table of each `docs/work/*/graph.md` (`crates/zygo-work/src/sources.rs:75-122`) and selects the `IN-PROGRESS` row whose Refs name the checkout (`sources.rs:146-188`). The item is `<program>/<node>` in `ZYGO_WORK`, and line 1 shows it as `◆ program/node` (H§6.2 work item markers). zygo does not read `history.md`, `nodes/`, the states other than `IN-PROGRESS`, or the `Run` line.
- **Gate rounds.** `work_verdict` records a verdict per unit and round in the usage store (`crates/zygo-mcp/src/tools/work.rs:45-79`; `round` column, `crates/zygo-usage/src/schema.rs:46`).
- **Free text from the model.** zygo stores none in the usage store: `work_verdict` accepts `findings` and does not store it (`work.rs:59`, "§14 stores no free text"). The workspace state holds no content (W-C7, `docs/specs/2026-11-20-workspaces-design.md:61`). Mail is the one exception, and it lives only under `mail/` (M-C2, `docs/specs/2026-11-22-workspace-mail-design.md:43`). Thus a stored status sentence needs a place that these rules allow.

### Tools: what exists

The `zygo mcp` server lists these tools (`crates/zygo-mcp/src/tools/`):

| Tool | File | Gate in `build_with` (`crates/zygo-mcp/src/tools/mod.rs:15-37`) |
|---|---|---|
| `compact` | `tools/compact.rs:9-23` | Claude Code only and `[features] compact_tool` (`mod.rs:21`; §10.2.3, `docs/specs/2026-11-18-zygo-design.md:500` "Listed for Claude Code only") |
| `result_view`, `result_search`, `result_tree` | `tools/results.rs:8-12` | `[features] results` (`mod.rs:24`) and a session directory (`results.rs:9`) |
| `work_begin`, `work_verdict` | `tools/work.rs:11-14` | `[features] work_units` (`mod.rs:27`) and a session directory (`work.rs:12`) |
| `workspace_down` | `tools/workspace_down.rs:38-46` | `[features] workspace_tool`, a session directory and a workspace membership marker (`workspace_down.rs:39-44`) |
| `mail_send`, `mail_read`, `mail_list` | Phase 3 ML-11 (`docs/work/phase3/graph.md:248` on `origin/phase-3`) | `[features] mail`, member only (`graph.md:171`; M§8, `docs/specs/2026-11-22-workspace-mail-design.md:92`) |

Phase 5 adds one internal tool, `plan_status` (P5-7).

How a tool is registered: each tool is a Rust type that implements the `Tool` trait (`def`, `call`; `crates/zygo-mcp/src/tool.rs:43-46`). `build_with` puts the tools in one list with hard-coded gates and refuses two tools of one name (`mod.rs:31-35`). The server answers `tools/list` and `tools/call` from that list (`crates/zygo-mcp/src/server.rs:107-108`). The gates read `McpCtx` (`tool.rs:49-53`): the client, the `[features]` toggles (`crates/zygo-config/src/features.rs:7-15`) and the session directory. The launcher registers the server as `zygo mcp --agent <client>` (Claude Code `crates/zygo-launch/src/claude_code.rs:613-619`; OpenCode `crates/zygo-launch/src/opencode.rs:150-151`).

The `compact` gate is a client branch (`mod.rs:21`). The client contract moves it into the descriptor (K§6.1.2 concern 5, `docs/specs/2026-11-25-client-model-design.md:167`).

Code outside `zygo-mcp` also depends on the tool names:

- The proxy finds a `compact` call by its name `mcp__<server>__compact` (`crates/zygo-proxy/src/window.rs:93-99`).
- The Claude Code launch adds `PreToolUse` hooks for `mcp__zygo__work_begin` and `mcp__zygo__work_verdict` (`crates/zygo-launch/src/claude_code.rs:483-484`). The OpenCode plugin runs the `work` hook after each tool (`crates/zygo-launch/assets/opencode-plugin.js:183`).
- The session text names `result_view`, `result_search` and `result_tree` (`crates/zygo-hooks/src/session_text.rs:9`); the pre-compact text names `result_view` (`crates/zygo-hooks/src/pre_compact.rs:76`).
- The model-visible name form differs per client: Claude Code `mcp__<server>__<tool>`, OpenCode `<server>_<tool>` (Claude Code: `docs/specs/2026-11-18-zygo-design.md:480`; OpenCode: premise P5, `docs/premises.md:29`, tool `zygo_probe_tool`).
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

Numbered in causal order.

| Id | Item | Depends | Spec |
|---|---|---|---|
| P5-1 | Segment registry: each HUD segment (H§6.2 segments 0 to 15, member name, mail, run time, update notice 16) is one entry with an id, a default place, a default color, a drop rank and a "never removed" flag | Phase 3 ML-15, RT-3 merged; background update row | to write (status line) |
| P5-2 | Two-line HUD **(operator)**: line 1 (IT report) is today's compact row, byte for byte (the H§14 compact golden files do not change), and stays on top; line 2 (project manager report) is a slot under it that prints nothing when it has no content, so the HUD stays one line. The compact/verbose style is removed **(operator)**: the migration of `[statusline] style` and of `zygo statusline <style>` | P5-1 | to write (status line) |
| P5-3 | Line 1 layout config: which segments show, their order and groups, and the width-drop order; `zygo config check` refuses a bad layout. The stripe and the logo stay fixed and first **(operator: the logo)** | P5-2 | to write (status line) |
| P5-4 | Icon set and colors in config, for both lines: an icon set key next to `ZYGO_NERD_FONT`, and the color of each segment; `NO_COLOR` and the 16/256-color rule (H AU23) still apply | P5-1 | to write (status line) |
| P5-5 | Scope and verb: the config layers that can set the layout, the icons and the colors; the session form of `zygo statusline`; a preview in `zygo doctor` | P5-3, P5-4 | to write (status line) |
| P5-6 | Plan reader (derived source): zygo reads the active work graph itself, with no agent action, in the `work_graph.py` format: program, rows by state (`history.md` included), `IN-PROGRESS` ids, ready ids, stuck ids, `parallel`. Bounded read; a malformed graph shows nothing (C6) | none | to write (status line) |
| P5-7 | Plan report (reported source): the agent writes a short status sentence through a new internal MCP tool (for example `plan_status`) or a CLI verb; zygo stores it with its time, a size limit and the secret pass; the line 2 report age comes from that time | P5-10 if it lands first (see Plan of work) | to write (status line) |
| P5-8 | Line 2 content and width: program, done/total with a small bar, running ids, blocked and operator-wait ids, gate round, next ready ids, the reported sentence, its age; one terminal line with its own width-drop order | P5-2, P5-6, P5-7 | to write (status line) |
| P5-9 | Per-client status surface: line 1 customization and line 2 reach each client through the client contract (Claude Code HUD; `zygo status` for OpenCode and Codex); the A-9 criteria cover both lines | conformance row; client contract build row; P5-5, P5-8 | to write (status line) |
| P5-10 | Tool plugin model: a manifest for each tool (name, description, input schema, capability declaration, supported clients, enable key, required session state); one registry takes the place of the hard-coded gates of `build_with` (`crates/zygo-mcp/src/tools/mod.rs:15-37`) | client contract build row | to write (tool plugins) |
| P5-11 | Tool selection and approval: enable and disable through §15.2 selection and the §15.4 toggles; approval of tool code as §15.7; the internal tools pre-approved as the built-in digests are | P5-10 | to write (tool plugins) |
| P5-12 | Migration of each internal tool to the model: `compact`, `result_view`, `result_search`, `result_tree`, `work_begin`, `work_verdict`, `workspace_down`, `mail_send`, `mail_read`, `mail_list`, and `plan_status` when P5-7 came first. No change for the user: the same names, the same `tools/list` per client and per feature set, the same replies | P5-10, P5-11; Phase 3 ML-11 merged | to write (tool plugins) |
| P5-13 | Tool-name users read the registry: the proxy `compact` match (`crates/zygo-proxy/src/window.rs:93-99`), the work hook matchers (`crates/zygo-launch/src/claude_code.rs:483-484`), the session text and pre-compact text (`crates/zygo-hooks/src/session_text.rs:9`, `crates/zygo-hooks/src/pre_compact.rs:76`) | client contract build row; P5-12 | to write (tool plugins) |
| P5-14 | Tool conformance: each tool's client support is in the capability registry and is graded per client (today A-6, A-17, A-19); the feature matrix shows it | conformance row; P5-12 | to write (tool plugins) |

Phase 4 dependencies, by row name:

- **Client contract build row.** P5-9: the status surface is descriptor concern 1 (K§6.1.2, `docs/specs/2026-11-25-client-model-design.md:163`). P5-10 and P5-13: the MCP tool name form and the `compact` listing are descriptor concern 5 (`:167`). That row waits for Phase 3 to merge (`docs/roadmap/phase-4.md` on `origin/phase-4-spec-client`).
- **Conformance row.** P5-9 grades A-9 (`:230`); P5-14 grades A-6, A-17 (the `compact` tool listed and working, `:238`) and A-19 (`:240`).
- **Background update row.** P5-1: segment 16 is in the registry (K§6.5.5, `:556`, `:560`).

## Open questions for the status line spec

The spec decides these. This roadmap does not.

Two lines and line 1:

1. **Style migration.** What happens to a user config that sets `[statusline] style` (`crates/zygo-config/src/schema.rs:628-643`), to `statusline` in `overrides.json`, and to `zygo statusline <style>` (`crates/zygo/src/cmd/statusline.rs:57`): refuse, ignore with a notice, or map. If the verb goes away or gets a new meaning.
2. **Verbose segments.** Where the verbose-only segments go (git ahead/behind, effort, mode, ctx percent): dropped, or available to the line 1 customization. The default is line 1 = today's compact row, byte for byte.
3. **Logo reflection.** The reflection `zʎƃo` is a verbose row 2 feature (H OP11, `docs/specs/2026-11-21-statusline-hud-design.md:92`; H AU8, `:103`). If line 2 shows it, and at which column.
4. **Config form.** A segment list, a format string, or named presets.
5. **Layers.** `[statusline]` is user config only today (H§8). If a profile, the team config or the repository file can set the layout, the icons or the colors.
6. **Fixed parts.** The stripe and the logo are fixed **(operator: the logo)**. If a user can hide or move the other segments that are never removed today: the workspace phase, the `offline`, `suspended` and `ended` markers (H§6.7), the member name and the mail count (M§9a).
7. **Colors.** Per-segment SGR codes or a named theme; the 16-color fallback of a 256-color value (H AU23).
8. **Icon set.** The precedence of a config key against `ZYGO_NERD_FONT` (H AU2); custom glyphs per segment or not.
9. **Width-drop order.** Its own list, or derived from the layout.
10. **Session scope.** If the session verb can change the layout.
11. **Bad config.** The hook falls back to the default layout and writes one stderr line (C6, H-C3), or `config check` alone refuses it.
12. **Constraints that hold.** The 3 s deadline (H-C2), escaping (H-C6), and the window kept from the agent (H-C4, C7), on both lines.
13. **Link to the tool track.** If a tool plugin can add a HUD segment.

Line 2, the plan report:

14. **Which plan.** How zygo finds the active plan: the `work-graph` work item (`ZYGO_WORK` = `<program>/<node>`), the program that the last report names, or a config key. A session can have no plan, or a repository can hold more than one graph.
15. **Workspace.** In a workspace, if each member's line 2 shows the orchestrator's plan, or the member's own plan.
16. **No plan.** Line 2 shows nothing, or a short default. Author recommendation: nothing, so the HUD stays one line.
17. **Where the sentence is stored.** The usage store holds no free text (`crates/zygo-mcp/src/tools/work.rs:59`, §14), and the workspace state holds no content (W-C7, `docs/specs/2026-11-20-workspaces-design.md:61`). The spec names the place (for example a file in the session directory) and amends the rule it touches, as M-C2 did for mail. The size limit, the secret pass (§16.1), escaping (H-C6), and the age at which a report counts as stale.
18. **Tool or verb.** An MCP tool (`plan_status`), a verb (`zygo work status` collides with `zygo work [<id>|clear]`, §17, `docs/specs/2026-11-18-zygo-design.md:825`, when a branch is named `status`), or both. Who may report: the lead only, as `work_verdict` ("Lead only", `work.rs:49`), or each agent.
19. **Gate round and open pull requests.** The gate round can come from the usage store (`round`, `crates/zygo-usage/src/schema.rs:46`). An open pull request count needs a forge call; the HUD makes no network call today but the local proxy fetch, and §3 says nothing phones home (`docs/specs/2026-11-18-zygo-design.md:48`). If the count is in scope, and from the agent's report or from zygo.
20. **Cost.** The derived read in the 3 s deadline (H-C2): a bounded read, a cache like the H AU4 status cache, or a read only when the graph files change.
21. **Derived and reported disagree.** For example the report says "3 running" and the graph has 2 rows `IN-PROGRESS`. Which one line 2 shows.

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

1. **One spec per track area.** The status line spec covers P5-1 to P5-9 (two lines, line 1 customization and the plan report). The tool plugin spec covers P5-10 to P5-14.
2. **One work graph.** One work graph covers all of Phase 5. It is made after the operator approves both specs. A spec does not plan nodes.
3. **A review gate per node.** Each work node gets its own review gate of up to 3 rounds, as in phase 3. The P3-8 disposition rule applies at each gate: after round 3, a node passes only when each finding has exactly one disposition (fixed with a commit, rejected with a reason, or recorded with a DDR id), and no finding is of a class that stops for the operator.
4. **A pull request after every row.** Each row of the work graph, planning included, is delivered in its own pull request into the rendezvous branch `phase-5`. The lead merges a row after its gate and green CI. `phase-5` reaches `main` only on the operator's word.
5. **Order against phases 3 and 4.** The specs can start now. The tool track (P5-10 on) and P5-9 wait for the client contract build row. P5-1 also waits for Phase 3 ML-15 and RT-3 and for the background update row; P5-12 also waits for Phase 3 ML-11. P5-6 waits for nothing.
6. **Order of `plan_status`.** When P5-10 is done before P5-7 starts, `plan_status` is born in the tool plugin model. Otherwise P5-7 builds it as the other internal tools are built today, and P5-12 migrates it.
