# zygo phase 5 roadmap

**Status:** started 2026-11-25; amended 2026-11-27 (plugin host, feature modules, module metrics). Items only; no spec yet. Next: the host contract spec, then the status line spec and the feature module spec (see "Plan of work").
**Base specs:** `docs/specs/2026-11-18-zygo-design.md` (`§n`), `docs/specs/2026-11-20-workspaces-design.md` (`W§n`), `docs/specs/2026-11-21-statusline-hud-design.md` (`H§n`, decision ids `H OPn`, `H AUn`), `docs/specs/2026-11-22-workspace-mail-design.md` (`M§n`), `docs/specs/2026-11-24-session-runtime-design.md` (`T§n`), `docs/specs/2026-11-25-client-model-design.md` (`K§n`; approved by the operator 2026-11-25, `:3`), `docs/specs/2026-11-24-decision-records-design.md` (`D§n`), `docs/specs/2026-11-24-mail-sync-design.md` (`S§n`), `docs/specs/2026-11-24-output-shaping-design.md` (`O§n`).

Code and spec references are `file:line` at commit `33fd7bc5` (branch `phase-4`). A reference that says "on `origin/phase-3`" is at commit `4ed521d7` of that branch: the phase 3 work graph (`docs/work/phase3/`), the phase 3 code that `phase-4` does not have yet, and the output shaping spec. This roadmap names the Phase 4 rows by name: the **conformance row** (P4-2: capability registry, rubric and grades, K§6.2; `docs/roadmap/phase-4.md:25`), the **client contract build row** (P4-6: descriptor and adapter registries, K§6.1; `:29`) and the **background update row** (P4-10: update notices, K§6.5; `:33`).

## Intent

The operator's request (2026-11-25) **(operator)**: "start a phase 5 roadmap that includes: statusline customization (but not the logo), a plugin model for tools - migrate internal tools to the plugin model."

The operator's addition of the same day **(operator)**: "a way for an agent working on a plan to report status to the second statusline. we'll drop the compact/expanded, keep compact on top. look at our graph work loops. easy to report on done/total rows, and a brief status report. improve on that if you want - you get the vision. line 1 is the it report. line 2 is the project manager report."

The operator's statements of 2026-11-27 **(operator)**:

1. "my goal for phase 5 is a fully plugin-driven architecture for every part that we foresee extension."
2. "plugin unit is how we deliver the features. this is looking more and more like a plugin host architecturally. we should consider adding ponytail style metrics so we can evaluate tool performance. different clients are encapsulated - i think that's in phase 4. clients stay compiled. this isn't plugin in the sense of external installation. it's that we are architected to be modular with contracts so it's easy to extend."

What these statements decide **(operator)**:

- **The unit is the feature.** A feature is delivered as one module. The unit is not the single tool.
- **zygo is a plugin host.** There is one host. The modules have contracts.
- **A module is compiled in.** "Plugin" here means a module that is compiled into the binary behind a contract. It does not mean external installation. Thus these are not in Phase 5: third-party code, a module that runs out of process, a new manifest file in a §15 marketplace plugin, and a new approval flow. The §15 system (plugins in the Claude Code format, delivered to the client; `docs/specs/2026-11-18-zygo-design.md:708`, `:720`) is a different thing and does not change.
- **Clients stay compiled.** A client is compiled Rust. The Phase 4 client contract encapsulates it (K§6.1, `docs/specs/2026-11-25-client-model-design.md:107`).
- **Metrics.** Metrics "ponytail style", to evaluate tool performance, are to be considered.

Terms (author; the words are the author's, the decisions above are the operator's):

- **Feature module**, short **module**: one internal feature, compiled into the binary, that reaches the host only through the host contract. The operator calls it a "plugin". This roadmap says "feature module" so that it is not read as a §15 plugin.
- **§15 plugin**: a plugin in the Claude Code marketplace format that zygo delivers to the client (§15, `docs/specs/2026-11-18-zygo-design.md:708-781`). Phase 5 does not change it.
- **Host**: the part of zygo that holds the registry and calls the modules.
- **Contribution**: one thing that a module gives to the host: an MCP tool, a hook handler, a verb, a HUD segment, a session-text sentence, a doctor check, a config check, a guard, or a metric.
- **Seam**: a place in the code where a set of entries can grow.

Phase 5 has four tracks and one more item:

0. **Host contract (operator: the direction; author: the items).** One feature-module contract and one registry (P5-15). The contract of each contribution kind follows from it.
1. **Two-line HUD and line 1 customization (operator).** The HUD has two lines with fixed roles **(operator)**. Line 1, the "IT report", is today's compact line and stays on top **(operator)**. Line 2 is the "project manager report" **(operator)**. The compact/verbose style choice is removed **(operator)**. The user can change line 1. The logo is excluded **(operator)**: it stays fixed and always shows (H OP11, `docs/specs/2026-11-21-statusline-hud-design.md:92`; H AU8, `:103`).
2. **Plan report on line 2 (operator vision).** An agent that runs a plan (a work graph) reports to line 2: done/total rows and a brief status sentence that the agent writes **(operator)**. The operator invites improvements on that base **(operator)**; the item list below is the author's proposal.
3. **Feature modules (operator).** Until 2026-11-27 this track was "Tool plugins": a model for each MCP tool. The unit is now the feature **(operator)**. Each internal feature becomes one module, and its tools, hook handlers, verbs, HUD segments, session-text sentences, checks and guards are contributions of that module. All internal tools move to that model **(operator, 2026-11-25)**, with no change of behavior for the user. The seams that hold a closed set today get a registry (P5-21 to P5-26; the list is the author's proposal).

The one more item is **module metrics** (P5-27) **(operator: to be considered)**.

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
  - Update notice, segment 16 (background update row; K§6.5.5, `docs/specs/2026-11-25-client-model-design.md:632`, `:636`).

Claude Code shows a two-row status line output as two rows (premise PH2, verified on Claude Code 2.1.330, `docs/specs/2026-11-21-statusline-hud-design.md:579`). So line 2 needs no new premise on Claude Code.

The status surface differs per client. Claude Code has the HUD through `statusLine` (`crates/zygo-launch/src/claude_code.rs:534`). OpenCode has no status surface and uses `zygo status` (H§3, `docs/specs/2026-11-21-statusline-hud-design.md:47`). Codex shows only built-in status items (K§6.5.5, `docs/specs/2026-11-25-client-model-design.md:638`). Capability A-9 grades the status surface (K§6.2.2, `:237`). The status surface is a descriptor fact of the client contract (K§6.1.2 concern 1, `:167`). Thus the client contract build row owns this difference, and both lines reach each client through it.

### Plan report: what exists

- **The work graph format.** A plan is `docs/work/<program>/graph.md` with the marker `<!-- work-graph -->`, a `**Run:**` line (for example `parallel=3`) and a `## Nodes` table with the columns `ID | Node | Depends | State | Capability | Refs` (`docs/work/phase3/graph.md:3-7`, `:190-192` on `origin/phase-3`). Each node has `nodes/<id>.md`; closed rows move to `history.md`.
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
| `mail_send`, `mail_read`, `mail_list` | Phase 3 ML-11 (`docs/work/phase3/history.md:91` on `origin/phase-3`) | `[features] mail`, member only (`docs/work/phase3/graph.md:171` on `origin/phase-3`; M§8, `docs/specs/2026-11-22-workspace-mail-design.md:92`) |

Phase 5 adds one internal tool, `plan_status` (P5-7).

How a tool is registered: each tool is a Rust type that implements the `Tool` trait (`def`, `call`; `crates/zygo-mcp/src/tool.rs:43-46`). `build_with` puts the tools in one list with hard-coded gates and refuses two tools of one name (`mod.rs:31-35`). The server answers `tools/list` and `tools/call` from that list (`crates/zygo-mcp/src/server.rs:107-108`). The gates read `McpCtx` (`tool.rs:49-53`): the client, the `[features]` toggles (`crates/zygo-config/src/features.rs:7-15`) and the session directory. The launcher registers the server as `zygo mcp --agent <client>` (Claude Code `crates/zygo-launch/src/claude_code.rs:613-619`; OpenCode `crates/zygo-launch/src/opencode.rs:172`).

The `compact` gate is a client branch (`mod.rs:21`). The client contract moves it into the descriptor (K§6.1.2 concern 5, `docs/specs/2026-11-25-client-model-design.md:171`).

Code outside `zygo-mcp` also depends on the tool names:

- The proxy finds a `compact` call by its name `mcp__<server>__compact` (`crates/zygo-proxy/src/window.rs:94-100`).
- The Claude Code launch adds `PreToolUse` hooks for `mcp__zygo__work_begin` and `mcp__zygo__work_verdict` (`crates/zygo-launch/src/claude_code.rs:483-484`). The OpenCode plugin runs the `work` hook after each tool (`crates/zygo-launch/assets/opencode-plugin.js:183`).
- The session text names `result_view`, `result_search` and `result_tree` (`crates/zygo-hooks/src/session_text.rs:9`); the pre-compact text names `result_view` (`crates/zygo-hooks/src/pre_compact.rs:76`).
- The model-visible name form differs per client: Claude Code `mcp__<server>__<tool>`, OpenCode `<server>_<tool>` (Claude Code: `docs/specs/2026-11-18-zygo-design.md:480`; OpenCode: premise P5, `docs/premises.md:34`, tool `zygo_probe_tool`).
- `compact` keeps its name, because act-lite's skills call a tool of that name (§15.4, `docs/specs/2026-11-18-zygo-design.md:745`).

The plugin system for skills and plugins (§15, `docs/specs/2026-11-18-zygo-design.md:708-781`; crate `crates/zygo-plugins`):

- Marketplaces with a pinned commit (§15.1, `:710`); selection by `zygo plugins enable|disable` and profiles (§15.2, `:724`); a per-session snapshot (§15.3, `:730`; `crates/zygo-plugins/src/snapshot.rs:237`).
- Feature toggles for built-in features, which include the three tool toggles (§15.4, `:743`).
- The embedded `zygo` plugin, compiled into the binary, with commands and one skill and no code (§15.5, `:747`; `crates/zygo-plugins/src/embedded.rs:1-2`).
- A plugin check (§15.6, `:751`; `crates/zygo-plugins/src/check.rs:148`).
- Code approval by code digest, stored in `approvals.toml`; built-in digests pre-approved in the binary (§15.7, `:761-766`; `crates/zygo-plugins/src/approve.rs:1`, `crates/zygo-plugins/src/builtin.rs:1-2`).

A plugin can already ship an MCP server through `.mcp.json` (§15.1, `:720`), and that server is code under §15.7. zygo's own tools are not in this system: they are compiled into `zygo mcp` and gated by `[features]`.

A feature module gives a tool: a declaration (name, description, input schema); the clients it supports; the enable key; and the session state it needs. It gives no approval step: a module is compiled into the binary **(operator, 2026-11-27)**, as the tools are today.

### Modularity: what exists

zygo consumes no plugin contribution today. A §15 plugin is in the Claude Code marketplace format (`docs/specs/2026-11-18-zygo-design.md:720`), and zygo copies its parts to the store for the client (`PARTS`, `crates/zygo-plugins/src/check.rs:142-145`). The hook table and the MCP tool list are static (rows below).

The seams, each with its entries today:

| Seam | Defined at | Entries today | How an entry is added today |
|---|---|---|---|
| Feature toggle | `struct Features` (`crates/zygo-config/src/features.rs:7-16`), `RedactionFeatures` (`:32-35`) | `results`, `work_units`, `compact_tool`, `workspace_tool`, `redaction.secrets`, `redaction.content`; phase 3 adds `mail` (`docs/work/phase3/graph.md:49` on `origin/phase-3`) | a new field, then a gate at each site that the feature uses (next table) |
| MCP tool | trait `Tool` (`crates/zygo-mcp/src/tool.rs:43-46`); list in `build_with` (`crates/zygo-mcp/src/tools/mod.rs:15-37`) | 7 tools in 4 groups ("Tools: what exists") | a new type, then a line and a gate in `build_with` |
| Hook handler | static `TABLE` (`crates/zygo/src/cmd/hook.rs:22-33`) | 10 handlers | a row in `TABLE`; a line in the Claude Code wiring, which code builds (`crates/zygo-launch/src/claude_code.rs:462-524`); a line in the OpenCode plugin (`crates/zygo-launch/assets/opencode-plugin.js:147-195`) |
| Verb | `SPECS` (`crates/zygo-core/src/verbs.rs:19-42`), exported as `VERB_SPECS` (`:44`); `VERBS` (`crates/zygo/src/cmd/verbs.rs:42-60`); clap `enum Cmd` (`crates/zygo/src/cmd/mod.rs:252-378`) and its dispatch (`:553-589`) | 17 verbs | a row in `SPECS`; a row in `VERBS` at the same index; a `Cmd` variant; a dispatch arm. The command files of the embedded `zygo` plugin already come from `VERB_SPECS` (`crates/zygo-plugins/src/embedded.rs:1`). Owner rule O14: "Every internal feature has a `/zygo:` command" (`docs/specs/2026-11-18-zygo-design.md:897`) |
| HUD segment | `enum Seg` (`crates/zygo-hooks/src/statusline/render.rs:405-425`); layout constants (`:428-449`); width-drop constants (`:451-476`) | 17 variants | a variant, a place in each constant, and its render code |
| Session-text sentence | 8 constants (`crates/zygo-hooks/src/session_text.rs:6-14`); fixed order in `mechanics` (`:18-30`) and `build` (`:33-62`) | 5 mechanics sentences and 3 alias parts | a constant and a line at a fixed place |
| Guard | `enum Guard` (`crates/zygo-work/src/policy.rs:8-19`); one inline chain, `guard_tool` (`crates/zygo-work/src/guards.rs:58-120`) | 9 guards | a variant and a block in the chain |
| Work-item source | trait `Source` (`crates/zygo-work/src/sources.rs:20-30`); `match` in `by_name` (`:32-40`) | 4 sources | an implementation and a `match` arm. The base spec: "Adding a source (Jira, Linear, GitLab) is a new implementation and a new name in `sources`; no other code changes" (`docs/specs/2026-11-18-zygo-design.md:631`) |
| Doctor check | one function, `report` (`crates/zygo/src/cmd/doctor.rs:190-243`) | not separate entries | lines in that function |
| Config check | one function, `check` (`crates/zygo-config/src/check.rs:34-148`) | not separate entries | lines in that function |
| Proxy stage | trait `Stage` (`crates/zygo-proxy/src/pipeline.rs:123-126`); a fixed `vec!` (`:157`) | 5 stages | a type, a place in the `vec!`, and a parameter of `Pipeline::with` (`:155`) |
| Dialect translator | `match` on `Dialect` in `translate_request` and `translate_response` (`crates/zygo-proxy/src/translate/mod.rs:41-57`) | 2 upstream dialects | a `match` arm in each function |
| Client | `enum Agent`, declared by the `agents!` macro (`crates/zygo-core/src/lib.rs:20-42`); trait `AgentAdapter` (`crates/zygo-launch/src/adapter.rs:20-55`) | 2 clients | Phase 4 builds the data registry and `adapter_for` (K§6.1.1, `docs/specs/2026-11-25-client-model-design.md:138-153`). The client contract nodes CC-1 to CC-16 have not started, and CC-0 is `IN-PROGRESS` (`docs/work/phase4/graph.md:92-108`) |
| Multiplexer | trait `Multiplexer` (`crates/zygo-workspace/src/mux/mod.rs:308-384`); one implementer (`crates/zygo-workspace/src/mux/tmux.rs:282`) | 1 backend | no construction site is shared: 26 `Tmux::new` calls in 8 files under `crates/*/src` (for example `crates/zygo-workspace/src/up.rs:276`, `crates/zygo/src/cmd/member.rs:307`); test files are not counted |
| Output shaper and runner (phase 3 only) | `enum Shaper` (`crates/zygo-results/src/shape/mod.rs:127-137`) and `enum Runner` (`crates/zygo-results/src/shape/command.rs:343-347`), both on `origin/phase-3` | 9 shapers, 3 runners | a variant |

A feature crosses many sites. The `[features]` keys show it:

| Key | MCP tools | Hook | Session text | Other sites |
|---|---|---|---|---|
| `results` | `result_view`, `result_search`, `result_tree` (`crates/zygo-mcp/src/tools/mod.rs:24`) | `trim` (`crates/zygo/src/cmd/hook.rs:25`; wired at `crates/zygo-launch/src/claude_code.rs:476` and `crates/zygo-launch/assets/opencode-plugin.js:179`; gate in the hook, `crates/zygo-hooks/src/trim.rs:104`) | the `STORE` sentence (`crates/zygo-hooks/src/session_text.rs:20-22`) | the verb `results` (`crates/zygo-core/src/verbs.rs:24`); the output guard (`crates/zygo-work/src/guards.rs:108`); the pre-compact text (`crates/zygo-hooks/src/pre_compact.rs:76`); the HUD segment `Trimmed` (`crates/zygo-hooks/src/statusline/render.rs:422`) |
| `work_units` | `work_begin`, `work_verdict` (`mod.rs:27`) | `work` (`hook.rs:30`; registered at `claude_code.rs:482-488` and `opencode-plugin.js:183`) | the `TRAILER` sentence (`session_text.rs:26-28`) | the verb `units` (`verbs.rs:27`) |
| `compact_tool` | `compact` (`mod.rs:21`) | none | none | the proxy match on the tool name (`crates/zygo-proxy/src/window.rs:94-100`); the verb `compact-tool` (`verbs.rs:32`); the view field (`crates/zygo-core/src/session_view.rs:21-22`) |
| `workspace_tool` | `workspace_down` (`crates/zygo-mcp/src/tools/workspace_down.rs:38-46`) | none | none | not a profile key (`crates/zygo-config/src/features.rs:11-14`) |

Thus a new feature edits many files today, and no one place lists what a feature owns.

Seams that are data already and need no change: the catalog, the roles, the marketplaces and the schema rules (`crates/zygo-config/src/builtin.rs:23-47`), the redaction patterns (`crates/zygo-config/src/builtin_patterns.rs:9`), and the loadouts (`[loadout.<name>]`, `crates/zygo-config/src/schema.rs:700`).

The operator deferred user-defined filter files in phase 3 (non-goal, `docs/specs/2026-11-24-output-shaping-design.md:38`; rejected alternative, `:504`; both on `origin/phase-3`). Phase 5 does not open that again: P5-23 only gives the built-in shapers a registry.

### Module metrics: what exists

The reference **(operator)**: "ponytail style metrics so we can evaluate tool performance".

The author's reading of the reference, which the operator can correct: Ponytail's `ponytail-gain` prints a scoreboard of what a tool saved across recent sessions (lines, tokens, cost, time), and its benchmark compares arms with the tool and without the tool.

What zygo records today:

- **The usage store.** Table `requests`: tokens, `estimated_usd` and `latency_ms` for each request (`crates/zygo-usage/src/schema.rs:6-34`). Table `trims`: `tool`, `kind`, `stored_bytes` and `view_bytes` for each trimmed result (`:35-43`). Table `work_units`: `tool_calls`, `failed_calls`, `guard_events` and `verdict` for each unit and round (`:44-60`). No table and no column is keyed by a module or by an MCP tool of zygo (`:5-78`).
- **`zygo usage`** (`crates/zygo/src/cmd/usage.rs:72`) prints `bytes trimmed` and `tokens trimmed (estimate)` (`:104`) from the sum of `stored_bytes - view_bytes` over the `trims` rows (`crates/zygo-usage/src/query.rs:183-187`).
- **`zygo work report`** prints one block for each unit (`crates/zygo-work/src/report.rs:41`).
- **The HUD** shows the trimmed figure (segment `Trimmed`, `crates/zygo-hooks/src/statusline/render.rs:422`).
- **Phase 3 saving accounting** (O§6, `docs/specs/2026-11-24-output-shaping-design.md:67` on `origin/phase-3`; the references of this bullet are to that file). Two figures that stay apart **(operator)**: "tokens kept out of context" and "estimated spend avoided", both net of read-backs (O§6.2, `:78-85`). A new `readbacks` table and the columns `result_id`, `command` and `shaper` on `trims` (O§6.5, `:131-152`). New `zygo usage` lines with a breakdown by tool and command (O§6.6, `:154`). A minimum saving for each shaper, which a CI test checks (O-G5, `:33`). Before phase 3: "No benchmark and no measured hook latency exist" (`:23`).

Thus zygo measures one feature today, the result store, and measures it per tool that was trimmed. It has no figure for a module, no use count for a zygo MCP tool, and no figure for the overhead that a feature adds.

## Items

The ids are stable, because other text refers to them. A new item gets the next id. The `Depends` column gives the order.

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
| P5-10 | Feature module contract for tools: a tool is a contribution of its module, with a declaration (name, description, input schema, capability declaration, supported clients, enable key, required session state); the registry of P5-15 takes the place of the hard-coded gates of `build_with` (`crates/zygo-mcp/src/tools/mod.rs:15-37`) | P5-15; client contract build row | to write (feature modules) |
| P5-11 | Module enable and disable: through the §15.4 toggles, in the config layers and profiles that set them today (`docs/specs/2026-11-18-zygo-design.md:745`); P5-19 holds the rules. No approval step and no digest, because a module is compiled into the binary **(operator, 2026-11-27)** | P5-10 | to write (feature modules) |
| P5-12 | Migration of each internal tool to the model: `compact`, `result_view`, `result_search`, `result_tree`, `work_begin`, `work_verdict`, `workspace_down`, `mail_send`, `mail_read`, `mail_list`, and `plan_status` when P5-7 came first. No change for the user: the same names, the same `tools/list` per client and per feature set, the same replies. This row is the tool part of the feature-module migration (P5-28) | P5-10, P5-11; Phase 3 ML-11 merged | to write (feature modules) |
| P5-13 | Tool-name users read the registry: the proxy `compact` match (`crates/zygo-proxy/src/window.rs:94-100`), the work hook matchers (`crates/zygo-launch/src/claude_code.rs:483-484`), the session text and pre-compact text (`crates/zygo-hooks/src/session_text.rs:9`, `crates/zygo-hooks/src/pre_compact.rs:76`). These sites become contributions of the module that owns the tool (P5-16, P5-18, P5-28) | client contract build row; P5-12 | to write (feature modules) |
| P5-14 | Module conformance, tools: the client support of each module's tools is in the capability registry and is graded per client (today A-6, A-17, A-19); the feature matrix shows it. P5-30 widens this row to the other contribution kinds | conformance row; P5-12 | to write (feature modules) |
| P5-15 | Host contract: one feature-module contract and one registry. A module declares its id, its enable key, the session state it needs, the clients it supports (through the Phase 4 descriptor), and its contributions: MCP tools, hook handlers, verbs, HUD segments, session-text sentences, doctor checks, config checks, guards and metrics. The host owns the order, the gates, the deadlines and the refusal of two entries of one name | client contract build row | to write (host contract) |
| P5-16 | Hook handler contribution: the hook table (`crates/zygo/src/cmd/hook.rs:22-33`) and the wiring for each client (`crates/zygo-launch/src/claude_code.rs:462-524`, `crates/zygo-launch/assets/opencode-plugin.js:147-195`) come from the registry | P5-15 | to write (feature modules) |
| P5-17 | Verb contribution: the CLI verb, the slash verb and the command file of the embedded `zygo` plugin come from one registration (O14, `docs/specs/2026-11-18-zygo-design.md:897`) | P5-15 | to write (feature modules) |
| P5-18 | Session-text contribution: a module owns its sentence (`crates/zygo-hooks/src/session_text.rs:6-14`). The words do not change | P5-15 | to write (feature modules) |
| P5-19 | Feature toggles derived from the registry. The rules that hold today stay: `workspace_tool` is not a profile key (`crates/zygo-config/src/features.rs:11-14`), and `redaction.secrets` "cannot be turned off by a profile or repository" (`docs/specs/2026-11-18-zygo-design.md:745`) | P5-15 | to write (feature modules) |
| P5-20 | Doctor check and config check contribution: a module gives its lines of `zygo doctor` (`crates/zygo/src/cmd/doctor.rs:190-243`) and its rules of `zygo config check` (`crates/zygo-config/src/check.rs:34-148`) | P5-15 | to write (feature modules) |
| P5-21 | Guard registry: each guard (`crates/zygo-work/src/policy.rs:8-19`) is one entry, in place of the inline chain (`crates/zygo-work/src/guards.rs:58-120`) | P5-15 | to write (feature modules) |
| P5-22 | Work-item source registry, in place of the `match` in `by_name` (`crates/zygo-work/src/sources.rs:32-40`). Author recommendation: the source contract (`:20-30`) gets a "progress" method, so that the plan reader (P5-6) is not bound to one format; P5-6 keeps `work-graph` as the first implementer | P5-15; P5-6 | to write (feature modules) |
| P5-23 | Output shaper and runner registry, with the built-in entries only (`enum Shaper` and `enum Runner` on `origin/phase-3`, "Modularity: what exists"). User-defined filter files stay out **(operator, phase 3)** | P5-15; phase 3 output shaping (P3-6) taken by `phase-4` | to write (feature modules) |
| P5-24 | Proxy stage registry (`crates/zygo-proxy/src/pipeline.rs:157`) and dialect translator registry (`crates/zygo-proxy/src/translate/mod.rs:41-57`), with the built-in entries. The constraints C3, C4 and C5 hold (`docs/specs/2026-11-18-zygo-design.md:54-56`) | P5-15 | to write (feature modules) |
| P5-25 | Client registry alignment: the Phase 4 registries (`adapter_for` and the descriptor registry, K§6.1.1, `docs/specs/2026-11-25-client-model-design.md:138-153`) and the Phase 5 host registry have one shape. Clients stay compiled **(operator)** | P5-15; client contract build row | to write (host contract) |
| P5-26 | Multiplexer factory: one construction site for the backend, in place of the direct `Tmux::new` calls ("Modularity: what exists") | P5-15 | to write (feature modules) |
| P5-27 | Module metrics **(operator: "ponytail style metrics so we can evaluate tool performance"; to be considered)**. The form is the author's proposal: each module reports its measured effect through the host contract, and one report shows, for each module and each tool, the use count, the saving (tokens, cost, time) and the overhead that it adds | P5-15; phase 3 saving accounting (P3-5) taken by `phase-4` | to write (host contract; see Plan of work, item 1) |
| P5-28 | Migration of each internal feature to a module, with no change for the user. Results: 3 tools, the `trim` hook, the `STORE` sentence, the verb `results`, the output guard, the pre-compact text, the segment `Trimmed`. Work units: 2 tools, the `work` hook and its matchers, the `TRAILER` sentence, the verb `units`. Compact: the tool `compact`, the proxy match, the verb `compact-tool`. Workspace tool: `workspace_down`. Mail (phase 3): 3 tools (`docs/work/phase3/history.md:91`), the member name and unread mail segments (`docs/work/phase3/graph.md:50`), the `mail` key (`:49`), all on `origin/phase-3`. Plan report: P5-6 to P5-8. "Modularity: what exists" gives the references of the sites | P5-10, P5-11, P5-16 to P5-20; the fixtures of P5-29; Phase 3 ML-11 merged | to write (feature modules) |
| P5-29 | Proof. A no-change fixture for each seam, made before the migration: `tools/list` and the replies per client and per feature set, the generated hook wiring per client, the session text, the HUD golden files, the verb list. And a boundary check that refuses a reference to a part of a module from outside the registry, modeled on the Phase 4 boundary check (K§6.1.4, `docs/specs/2026-11-25-client-model-design.md:189-199`) | the fixtures: none; the boundary check: P5-15 | to write (host contract) |
| P5-30 | Conformance, all contribution kinds: the client support that a module declares is in the capability registry (K§6.2) for each contribution kind, not only for tools. Widens P5-14 | conformance row; P5-28 | to write (feature modules) |

Phase 4 dependencies, by row name:

- **Client contract build row.** P5-9: the status surface is descriptor concern 1 (K§6.1.2, `docs/specs/2026-11-25-client-model-design.md:167`). P5-10 and P5-13: the MCP tool name form and the `compact` listing are descriptor concern 5 (`:171`). P5-15 and P5-25: the registries of K§6.1.1 (`:138-153`). That row waits until `phase-4` takes phase 3 (`docs/roadmap/phase-4.md:29`; node CC-0, `docs/work/phase4/graph.md:92`).
- **Conformance row.** P5-9 grades A-9 (`:237`); P5-14 grades A-6 (`:234`), A-17 (the `compact` tool listed and working, `:245`) and A-19 (`:247`); P5-30 adds the other contribution kinds.
- **Background update row.** P5-1: segment 16 is in the registry (K§6.5.5, `:632`, `:636`).

Phase 3 dependencies: P5-1 (ML-15, RT-3), P5-12 and P5-28 (ML-11), P5-23 (output shaping, P3-6) and P5-27 (saving accounting, P3-5). Phase 3 reaches `phase-4` through node CC-0, which is `IN-PROGRESS` (`docs/work/phase4/graph.md:92`).

## Stays closed

These sets are fixed by the operator or by a spec. Phase 5 gives them no registry.

- **Decision record kinds, statuses and stop classes (operator).** D§6.4, D§6.5 and D§8.2 (`docs/specs/2026-11-24-decision-records-design.md:156-168`, `:258-260`; the file is on `phase-4` at the commit of this roadmap, and `origin/phase-3` has a different version).
- **The mail sync transport (operator).** A non-goal: "A zygo server or relay; network delivery other than `git fetch` and `git push`" (`docs/specs/2026-11-24-mail-sync-design.md:35`).
- **The config layers.** §6.1 (`docs/specs/2026-11-18-zygo-design.md:103-105`).
- **The logo (operator).** H OP11 (`docs/specs/2026-11-21-statusline-hud-design.md:92`); the 2026-11-25 request excludes it.
- **The update source.** The update check reads the public VERSION file of the release (`docs/specs/2026-11-18-zygo-design.md:48`).
- **User-defined output filter files (operator, phase 3).** `docs/specs/2026-11-24-output-shaping-design.md:38`, `:504` on `origin/phase-3`.
- **Third-party modules and externally installed modules (operator, 2026-11-27).** See "Intent".

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
13. **Link to the feature module track.** If a feature module can add a HUD segment. **Decided:** yes. This follows from the operator's statement 1 of 2026-11-27 ("every part that we foresee extension"). The form is the author's: a module contributes its segments through the host contract (P5-15), and the segment registry (P5-1) holds them.

Line 2, the plan report:

14. **Which plan.** How zygo finds the active plan: the `work-graph` work item (`ZYGO_WORK` = `<program>/<node>`), the program that the last report names, or a config key. A session can have no plan, or a repository can hold more than one graph.
15. **Workspace.** In a workspace, if each member's line 2 shows the orchestrator's plan, or the member's own plan.
16. **No plan.** Line 2 shows nothing, or a short default. Author recommendation: nothing, so the HUD stays one line.
17. **Where the sentence is stored.** The usage store holds no free text (`crates/zygo-mcp/src/tools/work.rs:59`, §14), and the workspace state holds no content (W-C7, `docs/specs/2026-11-20-workspaces-design.md:61`). The spec names the place (for example a file in the session directory) and amends the rule it touches, as M-C2 did for mail. The size limit, the secret pass (§16.1), escaping (H-C6), and the age at which a report counts as stale.
18. **Tool or verb.** An MCP tool (`plan_status`), a verb (`zygo work status` collides with `zygo work [<id>|clear]`, §17, `docs/specs/2026-11-18-zygo-design.md:825`, when a branch is named `status`), or both. Who may report: the lead only, as `work_verdict` ("Lead only", `work.rs:49`), or each agent.
19. **Gate round and open pull requests.** The gate round can come from the usage store (`round`, `crates/zygo-usage/src/schema.rs:46`). An open pull request count needs a forge call; the HUD makes no network call today but the local proxy fetch, and §3 says nothing phones home (`docs/specs/2026-11-18-zygo-design.md:48`). If the count is in scope, and from the agent's report or from zygo.
20. **Cost.** The derived read in the 3 s deadline (H-C2): a bounded read, a cache like the H AU4 status cache, or a read only when the graph files change.
21. **Derived and reported disagree.** For example the report says "3 running" and the graph has 2 rows `IN-PROGRESS`. Which one line 2 shows.

## Open questions for the feature module spec

Until 2026-11-27 this section was "Open questions for the tool plugin spec". The spec decides the questions that have no decision mark. This roadmap does not.

1. **In process or out of process.** Internal tools as compiled Rust types in `zygo mcp`, or as separate processes; the same answer for third-party tools or not. **DECIDED (operator, 2026-11-27):** in process. A module is compiled Rust in the zygo binary. Third-party tools are not in Phase 5 (question 8).
2. **Relation to §15.** A tool plugin as a new part of a §15 plugin, as a new kind of plugin, or as an entry of the embedded `zygo` plugin (§15.5). **DECIDED (operator, 2026-11-27):** separate from §15. A feature module is none of the three. §15 does not change.
3. **Client support.** How a tool declares the clients it supports, and how that links to the descriptor (K§6.1.2 concern 5) and to the conformance registry (K§6.2). Today the `compact` gate is the only one (`crates/zygo-mcp/src/tools/mod.rs:21`).
4. **Capability declaration.** What a tool can touch (session directory, usage store, a child process: `workspace_down` runs `zygo down`, `crates/zygo-mcp/src/tools/workspace_down.rs:97-104`), and how the approval shows it. After the decision of question 7, no approval reads the declaration. The question that stays: if a module declares what it touches, and which check reads that declaration.
5. **Hooks and proxy coupling.** The hooks of a feature are contributions of its module (P5-16). The question that stays is the form: if the module declaration names the hooks that a tool needs (the work hooks, `crates/zygo-launch/src/claude_code.rs:483-484`) and the proxy behavior it triggers (`compact`, `crates/zygo-proxy/src/window.rs:94-100`).
6. **Enable and disable.** How the `[features]` keys `results`, `work_units`, `compact_tool`, `workspace_tool` and `mail` map to the new model, and the rules that hold today: `workspace_tool` is not a profile key (`crates/zygo-config/src/features.rs:11-14`).
7. **Approval of internal tools.** Pre-approved by digest in the binary as §15.7 does for the built-in marketplace, or no approval because the binary is trusted. **DECIDED (operator, 2026-11-27):** no approval. A module is compiled into the binary, and the binary is trusted.
8. **Third-party tool plugins.** If they are in scope for phase 5. **DECIDED (operator, 2026-11-27):** not in Phase 5.
9. **No-change proof.** The check that the migration changes nothing for the user: for example a `tools/list` and reply fixture per client and per feature set, made before the migration. P5-29 widens this proof to each seam.

## Open questions for the host contract spec

The spec decides these. This roadmap does not.

1. **Static registration form.** A compiled list that names each module, or collection at link time. Author recommendation: an explicit compiled list, because the compiler then names a missing piece, as K-G2 requires for clients ("The compiler and the checks list each missing piece", `docs/specs/2026-11-25-client-model-design.md:62`).
2. **Crate layout.** Where the contract lives, so that the crates do not form a cycle: the contributions are in `zygo-mcp`, `zygo-hooks`, `zygo-work`, `zygo-proxy` and the binary crate today ("Modularity: what exists").
3. **Session state.** How a module reaches the session state that it declares (the session directory, the session view, the usage store).
4. **Deadlines.** The deadline for each contribution kind. Today the HUD hook has 3 s for its whole run (H-C2, `docs/specs/2026-11-21-statusline-hud-design.md:53`; `BOUND`, `crates/zygo-hooks/src/statusline/mod.rs:23`), and the prompt hook ends within 5 s and waits 4.5 s for the verb (`crates/zygo-hooks/src/prompt.rs:15-16`).
5. **Order.** The rule for each seam where the order matters: the hook chain for each event (`crates/zygo-launch/src/claude_code.rs:471-508`), the proxy stages (`crates/zygo-proxy/src/pipeline.rs:157`), the session text (`crates/zygo-hooks/src/session_text.rs:33-62`) and the place of a HUD segment (`crates/zygo-hooks/src/statusline/render.rs:428-449`).
6. **A contribution to the seam of another module.** If one module may contribute to a seam that another module owns.
7. **Toggle keys.** How the `[features]` keys map to module ids (P5-19).
8. **Registry-only seams.** If the seams that are only a registry (P5-21, P5-23, P5-24, P5-26) each get their own small contract, or the common one.

## Open questions for module metrics

The spec decides these. This roadmap does not.

1. **Storage.** Where the numbers are stored. Two rules apply: "The HUD adds no column" to the usage store (`docs/specs/2026-11-21-statusline-hud-design.md:48`), and the usage store never holds bodies, prompts or tool inputs (§14 "Never", `docs/specs/2026-11-18-zygo-design.md:701`).
2. **Saving without a baseline run.** How a saving is measured: a measured counterfactual (for example the trimmed bytes, as phase 3 does), or benchmark arms with the module off.
3. **Overhead.** Which overheads count: the tokens of a tool description in each request, the tokens of the session text, the latency of a hook, the latency of a proxy stage.
4. **Report.** The verb that prints the report, and if line 2 or `zygo doctor` shows a part of it.
5. **Constraints that hold.** The agent is not told its window (C7, `docs/specs/2026-11-18-zygo-design.md:58`). Provider names and prices never leave the server (H-C5, `docs/specs/2026-11-21-statusline-hud-design.md:56`).

## Plan of work (operator rules as in phases 3 and 4; given by the lead, 2026-11-25; items 1, 5 and 6 amended and item 7 added by the author, 2026-11-27)

1. **Three specs.** The host contract spec is first: P5-15, P5-25, P5-29 and the rules that all contribution kinds share. Then the status line spec: P5-1 to P5-9 (two lines, line 1 customization and the plan report), with the segment contribution. And the feature module spec: P5-10 to P5-14, P5-16 to P5-24, P5-26, P5-28 and P5-30. Author recommendation: P5-27 is in the host contract spec and not in its own short spec, because a metric is a contribution kind. "Both specs" in item 2 now means the three specs.
2. **One work graph.** One work graph covers all of Phase 5. It is made after the operator approves both specs. A spec does not plan nodes.
3. **A review gate per node.** Each work node gets its own review gate of up to 3 rounds, as in phase 3. The P3-8 disposition rule applies at each gate: after round 3, a node passes only when each finding has exactly one disposition (fixed with a commit, rejected with a reason, or recorded with a DDR id), and no finding is of a class that stops for the operator.
4. **A pull request after every row.** Each row of the work graph, planning included, is delivered in its own pull request into the rendezvous branch `phase-5`. The lead merges a row after its gate and green CI. `phase-5` reaches `main` only on the operator's word.
5. **Order against phases 3 and 4.** The specs can start now. P5-15 and P5-9 wait for the client contract build row, and each row that depends on P5-15 waits with it. P5-1 also waits for Phase 3 ML-15 and RT-3 and for the background update row; P5-12 and P5-28 also wait for Phase 3 ML-11; P5-23 and P5-27 also wait for phase 3 output shaping and saving accounting. P5-6 and the fixtures of P5-29 wait for nothing.
6. **Order of `plan_status`.** When P5-10 is done before P5-7 starts, `plan_status` is born as a contribution of a feature module. Otherwise P5-7 builds it as the other internal tools are built today, and P5-12 migrates it.
7. **Timing against the client contract (author recommendation).** The client contract nodes are not built: CC-0 is `IN-PROGRESS` and CC-1 to CC-16 have not started (`docs/work/phase4/graph.md:92-108`). CC-9 builds the `adapter_for` registry (`:101`). Thus the host contract spec should fix the registry shape before CC-9 is built. Otherwise P5-25 changes a registry that was built a short time before. This roadmap does not change the Phase 4 graph; the operator decides if CC-9 waits.
