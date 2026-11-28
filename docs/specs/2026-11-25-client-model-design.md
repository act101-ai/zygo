# zygo: client model, conformance score, Codex and update notices — design

**Status:** approved by the operator 2026-11-25 (instruction to plan and build)
**Date:** 2026-11-25
**Release:** phase 4 (roadmap items P4-1 to P4-11, `docs/roadmap/phase-4.md`)
**Base specs:** `docs/specs/2026-11-18-zygo-design.md` (`§n`; C3, C5, C6, C9, §3, §7.6, §8, §10.2, §12.2). Also cited: `docs/specs/2026-11-20-workspaces-design.md` (`W§n`, `AUn`), `docs/specs/2026-11-21-auto-update-design.md` (`U`), `docs/specs/2026-11-21-statusline-hud-design.md` (`H§n`), `docs/specs/2026-11-24-decision-records-design.md` (the DDR rule, P3-8). `K§n` is a section of this document.

Each decision has a mark. **(operator)**: the operator made the decision (the request of 2026-11-25, the two additions of the same day, and the decisions of 2026-11-25 on approval and row order, quoted in K§1). **(lead)**: the lead ruled it after the audit of this spec (2026-11-25). **(author)**: the author made it; the operator can change it. Code references are `file:line` at commit `bf3726bd` (branch `phase-4`, equal to `main`).

---

## 1. Intent

The operator's request, verbatim **(operator)**:

> "i would like to add codex support to zygo. with adding codex support, we need two things. we need to have a clear architectural model for client implementations so they are straightforward to implement, update, and grade. we will maintain a feature matrix conformance score. for every feature that any zygo feature a client supports, all clients get graded against their support for that client, and a rubric is built, clients are measured and scored and found wanting. but sufficient. :)"

Reading **(author)**:

- A **client** is a coding agent that zygo wraps. Today the clients are Claude Code and OpenCode. The base spec calls them "agents"; the code type is `zygo_core::Agent`.
- The **feature matrix** has one row per zygo capability and one column per client. When any client supports a zygo feature, that feature is a capability, and each client gets a graded cell.
- **"Found wanting, but sufficient"**: the score shows each gap. A separate sufficiency bar says whether a client may ship.

Two additions of 2026-11-25 **(operator)**:

> "we need to produce a feature matrix, a rollup, and a score for each client."

> "we want background update checks and post to the hud when there's an update."

Decisions of 2026-11-25 **(operator)**: the spec is approved, with the instruction to plan and build. Row order: (a) one plan for the rows that start now (conformance, Codex premises, proxy Responses route); (b) a plan for the client contract; (c) the client-contract build, after phase 3 merges; (d) the Codex client gets its own design document, designed against the built contract; (e) a plan for the Codex client; (f) its build; (g) the background update row is opportunistic and is dispatched in parallel as soon as phase 3 is complete. K§6.9 and the roadmap give the rows.

On the updates of the wrapped clients **(operator)**:

> "don't they announce their own updates? i know claude does... we have a flag per client in the abstraction that says whether to announce client updates, and we set the default based on the default of whether the client does by default."

The gap today, from the code:

1. The client type is `Agent { ClaudeCode, OpenCode }` (`crates/zygo-core/src/lib.rs:20-53`). The trait `AgentAdapter` (`crates/zygo-launch/src/adapter.rs:20-55`) covers the launch only. Its `Plugins` enum (`adapter.rs:74-77`) has one variant per client.
2. Outside the two adapter files, client behavior is a branch on the client kind or a client-id literal. 35 files under `crates/*/src` name a client variant or a client id (grep at `bf3726bd`, inline test modules included); 2 of them are the adapter files. Examples:
   - hook input: `crates/zygo-core/src/hook.rs:23` (OpenCode normalization, `hook_opencode.rs`);
   - hook output: `crates/zygo-hooks/src/session_start.rs:42-49`, `crates/zygo-hooks/src/workspace.rs:621-644`;
   - session text: `crates/zygo-hooks/src/session_text.rs:83`;
   - prompt block while stopping: `crates/zygo-hooks/src/workspace.rs:290`;
   - MCP tool names: `crates/zygo-work/src/hooks.rs:369-373`; the compact tool: `crates/zygo-mcp/src/tools/mod.rs:21`;
   - plugin delivery: `crates/zygo-launch/src/launcher.rs:519-547`, `crates/zygo-plugins/src/snapshot.rs:264-266`;
   - argument grammar and session flags: `crates/zygo-launch/src/launcher.rs:733-736`, `crates/zygo-workspace/src/args.rs:356-360`, `:402-406`, `:507`, `:595-611`, `:652-656`;
   - workspace: `crates/zygo-workspace/src/state.rs:209-218` (`PaneKind`), `up.rs:677-679`, `snapshot.rs:262-264`; `crates/zygo/src/cmd/ws.rs:207-245`, `cmd/member.rs:65`, `:76-79`, `:509-514`, `:688-691`;
   - proxy: `crates/zygo-proxy/src/window.rs:131` (rewrite armed for Claude Code only), `passthrough.rs:33-43` (credential row), `cache.rs:34` (marker default per client id);
   - redaction: `crates/zygo-redact/src/content.rs:262`, `harness.rs:45`;
   - work join: `crates/zygo-work/src/join.rs:16`;
   - config: `crates/zygo-config/src/builtin.rs:50-55` (panics when a client has no row in `data/agents.toml`), `schema.rs:468`, `:680`, `check.rs:171`;
   - CLI: `crates/zygo/src/cmd/mod.rs:253-256`, `:558-559`, `config.rs:82-86`, `doctor.rs:192`, `compact.rs:61`, `hook.rs:103-106`, `loadout.rs:328`.
3. The §8.8 checklist ("Adding an agent") omits the proxy dialect, the workspace flag tables and the CLI subcommand.
4. `docs/conformance.md` is two hand-written tables. Each cell is free text. `scripts/check-conformance.sh:18-24` names the two clients, and `:54-62` expects exactly two rows. `scripts/check-premises.sh:32-44`, `:82` names the client versions. No CI job and no Rust test runs either script (`.github/workflows/ci.yml:45-51` runs `cargo test` and the install tests only).
5. The proxy serves clients the Anthropic Messages dialect only (`crates/zygo-proxy/src/routes.rs:45-47`). `/openai/v1/responses`, `/openai/v1/chat/completions` and `/openai/v1/models` answer `404 zygo_route_not_in_v1` (`routes.rs:48-50`, `:64-67`). Translation goes one way, Messages to the upstream dialect (`translate/mod.rs:40-57`, `forward.rs:181-188`).

Groundwork that exists: `RequestCtx.dialect` is "the client's dialect" (`crates/zygo-proxy/src/pipeline.rs:39-40`); the content-redaction pass reads a Responses `input` body (`crates/zygo-redact/src/content.rs:27-32`, `:195-205`; selected at `crates/zygo-proxy/src/redact_stage.rs:191`); the error body has an OpenAI shape (`crates/zygo-proxy/src/errors.rs:34-41`); Responses and chat SSE already translate to Messages SSE (`translate/sse.rs:1-2`).

## 2. Goals

- **K-G1** One client contract. Each client-specific behavior is in one place per client. A check fails when a client branch appears elsewhere.
- **K-G2** A new client is: one module per layer, one fixtures folder, one grade file. The compiler and the checks list each missing piece.
- **K-G3** One capability registry with a rubric of checkable criteria. Each client has a grade for each criterion. Each "met" grade cites evidence that a check finds.
- **K-G4** A weighted score per client, with a per-tier breakdown, and a sufficiency bar. Both are derived, not written.
- **K-G5** The generator gives three outputs for each client: a feature matrix, a rollup and a score (K§6.2.6) **(operator)**. They are in `docs/conformance.md` and in a machine-readable `docs/conformance.json`. CI fails when either file is out of date.
- **K-G6** `zygo codex` launches Codex CLI through the zygo proxy, with the zygo session features that Codex allows. Each gap is a graded criterion.
- **K-G7** The proxy serves the OpenAI Responses API as a client dialect, with streaming, to each upstream dialect.
- **K-G8** A live zygo session checks for a new zygo release in the background and shows it on the HUD until the running version is current **(operator)**. For client releases, zygo never doubles a client's own notice by default **(operator; wording by the lead)**: zygo's flag is off for a client that announces its own updates. A user who turns off the client's own notice can turn on zygo's flag for that client (K§6.5.4).

## 3. Non-goals

- ChatGPT-login passthrough for Codex. The `chatgpt` row of §12.2 stays a v2 row "to be verified" **(author)**.
- The Grok CLI and Pi **(author)**.
- A change to a capability standard that makes a client score higher **(author)**. The split of a standard into criteria (K§6.2.2) restates the standard; it adds no requirement.
- The `POST /openai/v1/chat/completions` client route and the `openai-chat` client pair of §7.6 v2. Codex 0.172.0 accepts only `wire_api = "responses"` (K§8, alternative 7). The route keeps its `404 zygo_route_not_in_v1` **(author)**.
- A change of behavior for Claude Code or OpenCode in P4-6 (K-D3).
- An install during a session. A background check never prompts and never installs (U "Prompt": hooks run the absolute binary path; at `bf3726bd` that is `crates/zygo-launch/src/claude_code.rs:76` (`current_exe`) and `:463`, `:528` (the quoted path in each hook command); U's `:38` is stale).
- A change to the start check of the auto-update design (U). It keeps its prompt, its skips and its 24 h rule.

## 4. Constraints

- **K-C1** C3: the proxy stores and logs no request or response body. The Responses dialect keeps no conversation state on the proxy.
- **K-C2** C5: each pass over a body is O(body size) and bounded. A chained translation (K§6.4.3) is two linear passes.
- **K-C3** C6: a hook never breaks the agent. This holds for Codex hook input.
- **K-C4** C9: a client has a grade for each criterion before its launcher is built. K-D5 enforces this with the compiler and the conformance test.
- **K-C5** Tests never run a real `claude`, `opencode` or `codex` binary. Launch tests use the stub agents. No test uses the operator's OpenAI or ChatGPT login, and no step runs `codex login` **(operator rule, project memory; K-D14)**.
- **K-C6** No CI test measures wall-clock time or has a deadline **(project rule)**.
- **K-C7** P4-6 changes no behavior. The proof is the existing test suites, checked for each P4-6 node against its own diff: `scripts/check-test-edits.sh <parent> <node file>`, where `<parent>` is the parent of the node's layer (the top of the stack below it), not a fixed base. In each test file and each `#[cfg(test)]` module that exists at `<parent>`, the check fails when a test function is removed or renamed, when `#[ignore]` is added, or when the multiset of string, char and number literals changes, unless the node file lists that exact change under a "Test logic edits" heading with its spec reason. Type paths, imports and constructors change identifiers, not literals, so they pass with no rule table. The review gate checks each listed edit **(lead, 2026-11-25, amended)**. Reason for the amendment: the first rule ("tests change only by declared mechanical rewrites") cannot be met: at the scale of P4-6 (hundreds of type-path uses), rewrite rules that compose cannot be ordered, substring patterns collide, import lines are new lines, and K§9 itself requires test logic edits (check item 7 of K§6.2.5 reads the descriptor; `minimum_agent_versions_match_premises` reads `Agent::ALL`).
- **K-C8** The minimum Codex version is 0.172.0 **(author)**.

## 5. Terms

- **Client.** A coding agent that zygo wraps. One variant of `Agent`.
- **Client contract.** The data and behavior that a client supplies: the descriptor (K§6.1.2) and the adapter (K§6.1.3).
- **Capability.** A zygo feature that a client can support. Id `A-n` or `WA-n`.
- **Criterion.** One checkable statement of a capability's standard. Id `<capability>.<n>`, for example `A-7.2`.
- **Grade.** The status of one criterion for one client: `met`, `not-met` or `n/a`.
- **Cell.** One capability for one client. Its value is derived from the grades (K-D7).
- **Tier.** `core`, `standard` or `extra`. It sets the weight of a capability and whether the bar reads it.
- **Bar.** The sufficiency rule of K-D9.
- **Premise.** A dated, fixture-backed observation of a pinned client version (`docs/premises.md`, `docs/live-checks.md`).

---

## 6. Decided approach

### 6.1 Client contract (P4-6)

The `file:line` sites below are at `bf3726bd`. P4-6 takes the inventory again at its own base commit (K§6.9).

#### 6.1.1 Layers and crate layout

The crate dependency direction at `bf3726bd` (from each `crates/*/Cargo.toml` `[dependencies]`):

```
zygo-core            (no zygo dependency)
zygo-config          -> core
zygo-usage           -> core
zygo-redact          -> core, config
zygo-work            -> core, config, usage
zygo-plugins         -> core, config
zygo-results         -> core, config
zygo-proxy           -> core, config, redact, usage
zygo-workspace       -> core, config, work, proxy
zygo-hooks           -> core, config, proxy, results, usage, work, workspace
zygo-mcp             -> core, config, results, work, usage, workspace
zygo-launch          -> core, config, proxy, plugins, work, usage, redact, hooks, workspace
zygo (binary)        -> all of the above; the only crate with clap
```

Client facts are read in each of these crates. Only `zygo-core` is below all of them. Client behavior that needs config, plugins, proxy or workspace types can live only in `zygo-launch` or above. So the contract has two layers **(author)**:

```
 zygo (binary)
   cmd/client/<client>.rs ........ CLI shim: clap options -> LaunchOpts -> launch
        |
 zygo-launch
   client/mod.rs ................. BEHAVIOR REGISTRY: adapter_for(Agent) (exhaustive match)
   client/<client>/ .............. AgentAdapter impl: session config files, plugin delivery,
        |                          preflight, refusal, resume preparation, env rows
        |
 zygo-hooks  zygo-mcp  zygo-workspace  zygo-proxy  zygo-redact  zygo-work  zygo-plugins  zygo-config
        |      (no client branch: each reads the descriptor through Agent::spec())
        |
 zygo-core
   client/mod.rs ................. DATA REGISTRY: enum Agent, Agent::ALL, Agent::spec() (exhaustive match)
   client/<client>.rs ............ ClientSpec constant + pure functions (hook in/out, arg grammar,
                                   tool-name parse, harness spans)
```

- **Lower layer** `crates/zygo-core/src/client/`. `mod.rs` holds the client kind, `Agent::ALL`, `Agent::spec() -> &'static ClientSpec`, and `FromStr`/`as_str` built from the specs. Each `<client>.rs` holds one `ClientSpec` constant and the pure functions it points to. These functions take and give `serde_json::Value`, `&str` and `&[String]` only, so `zygo-core` needs no new dependency.
- **The kind is opaque outside `zygo_core::client` (compiler enforcement)** **(lead)**. `Agent` becomes a public struct around a private enum (`pub struct Agent(Kind)`; `enum Kind` is private to the module). Outside the module there is no variant path and no per-client constant. Code gets a client from `Agent::ALL`, from `Agent::from_str(id)` (config, environment, state files, tests) or from a value it was given. `Agent == Agent` compares two clients; nothing outside the module can name one. So a branch on a specific client does not compile outside the module. The only way left to name a client is its id as a string, and the check of K§6.1.4 covers that. The only matches on `Kind` are in `client/mod.rs`. Serde keeps the id strings, so state files, `ZYGO_AGENT` and the usage store keep their values.
- **Upper layer** `crates/zygo-launch/src/client/`. `mod.rs` holds `adapter_for(agent, zygo) -> Box<dyn AgentAdapter>` as a table keyed by client id, because it cannot match the private kind. A test fails when a client of `Agent::ALL` has no row or two rows. Each `<client>/` folder holds that client's adapter, moved from `claude_code.rs` and `opencode.rs`, and its assets (`assets/opencode-plugin.js` moves to `client/opencode/`).
- **CLI shim** `crates/zygo/src/cmd/client/<client>.rs`, moved from `cmd/claude.rs` and `cmd/opencode.rs`. It is the clap options struct and its conversion to `LaunchOpts`. It stays in the binary crate because clap is a dependency of that crate only (`crates/zygo/Cargo.toml:24`).
- **Stub agent** `crates/zygo-launch/tests/stub_agent/`: each client adds its stub behavior (what it records of its environment, arguments and config files), which the A-21.1 launch test runs.

`Dialect` (`crates/zygo-config/src/schema.rs:182-188`) moves to `zygo-core`, and `zygo-config` re-exports it with the same serde names, so a descriptor can name its proxy dialect **(author)**.

`Agent` keeps its name and its serialized ids **(author)**: a rename to `Client` adds churn at each use site for no gain.

#### 6.1.2 The descriptor (`ClientSpec`)

The descriptor answers the concerns that the trait does not cover today. Each concern below replaces the cited branches. Field names are left to the builder (K§10); the content is decided.

| # | Concern | Descriptor content | Replaces |
|---|---|---|---|
| 1 | Identity and updates | id (`claude-code`), display name, program name, CLI verb, install hint, minimum version (= the pinned version of the grade file), `supported` flag (K-D9), `announces_own_updates` fact and release source (K§6.5.4), status surface (`statusLine` command, or none) | `lib.rs:28-53`, `builtin.rs:50-55` and `data/agents.toml`, `member.rs:509-514`, `config.rs:82-86`, `doctor.rs:192` |
| 2 | Hook input | a function that turns the client's hook JSON into the common form (Claude Code: identity) | `hook.rs:23`, `hook_opencode.rs` |
| 3 | Hook output | a function that renders a common reply (context text, deny, block) in the client's shape | `session_start.rs:42-49`, `workspace.rs:630-643` |
| 4 | Session text delivery | the mode (at session start by hook; per request by plugin; at launch in config), the model alias table for the alias sentence (§8.6 item 5), whether a prompt can be blocked while stopping | `session_text.rs:83`, `workspace.rs:290`, `:621` |
| 5 | MCP tool naming | a function from a model-visible tool name to (server, tool); whether the `compact` tool is listed | `zygo-work/src/hooks.rs:369-373`, `zygo-mcp/src/tools/mod.rs:21` |
| 6 | Plugin delivery form | the form name only; the delivery itself is an adapter method (K§6.1.3) | `launcher.rs:519-547`, `snapshot.rs:264-266`, `adapter.rs:74-77` |
| 7 | Argument grammar and session flags | the flag table, the subcommand rule, the session-flag names, the insertion point of session arguments, the "bare" rule | `args.rs:356-360`, `:402-406`, `:507`, `:595-611`, `:652-656`, `launcher.rs:733-736`, `member.rs:189`, `:688-691` |
| 8 | Workspace resume and turn events | the resume kind (pre-assign or capture), whether a config folder is saved for resume (AU61), the turn events that the client gives, the pane-kind id | `member.rs:65`, `:76-79`, `state.rs:209-218`, `up.rs:677-679`, `snapshot.rs:262-264`, `ws.rs:207-245`, `schema.rs:680`, `check.rs:171`, `loadout.rs:328` |
| 9 | Proxy client facts | the client dialect, the request-class and compaction header names (none for a client without them), whether the window rewrite is armed, the passthrough credential type name, the cache-marker default, the harness span rule for redaction, whether requests carry a per-agent id | `window.rs:131`, `passthrough.rs:33-43`, `schema.rs:468`, `content.rs:262`, `harness.rs:45`, `join.rs:16` |
| 10 | Compaction | whether the window can change in a live session, and the refusal text when it cannot | `compact.rs:61` |
| 11 | Environment | the client's own denylist rows, added to the common rows of §8.4 | `env.rs:22-33` (today one list for each child) |

`PaneKind` (`state.rs:209-218`) becomes `Agent(Agent) | Shell` with the same serialized strings, so state files keep their form **(author)**. The defaults `Agent::ClaudeCode` at `session_view.rs:50`, `cmd/hook.rs:106` and `cmd/loadout.rs:328` become one registry constant `Agent::DEFAULT` **(author)**.

#### 6.1.3 The adapter (`AgentAdapter`)

The trait keeps its methods (`adapter.rs:20-55`). It gains the behavior that needs upper-crate types **(author)**:

- `deliver_plugins(...)`: the session's plugins in the client's form. It replaces the `match` at `launcher.rs:519-547` and the `Plugins` enum. Each adapter keeps its own delivered value inside its own state.
- `prepare_resume(...)`: the resume preparation of `ws.rs:218-245` (the saved config folder of AU61, the program folder of OpenCode).
- `env_rows(...)`: the client's set rows, after the common denylist and the descriptor's denylist rows.

#### 6.1.4 The boundary check **(lead: compiler first; author: the scanner rules)**

With the kind opaque (K§6.1.1), the compiler refuses a branch on a specific client outside `zygo_core::client`. One test, `crates/zygo-core/tests/client_boundary.rs`, covers the one way left: a client named by a string.

- It parses each `crates/*/src/**/*.rs` file with `syn` (a dev-dependency of `zygo-core`) and walks each token, also the token streams inside macro calls (`format!`, `json!`, `matches!`), so a literal inside a macro is seen. Comments are not tokens, so they are never flagged.
- It flags each string literal (normal, raw or byte string) whose whole value equals a registered name: each client's id, program name, CLI verb and config folder name, read from `Agent::ALL` at test time. A new client's names are so covered with no edit to the test. A literal that only contains a name (`"zygo claude: …"`, `"Claude attribution"`) is not flagged.
- Literals are allowed only in the descriptor files `crates/zygo-core/src/client/` (the fields that hold the id, program, verb and folder names), in `crates/zygo-launch/src/client/` and in `crates/zygo/src/cmd/client/`. Each other site reads the descriptor field (for example the OpenCode program folder link of `crates/zygo/src/cmd/ws.rs:282`, `:470`).
- Skipped: each item with a `#[cfg(test)]` attribute (any item, not only a `mod`), and `crates/*/tests/`.
- **Exception list** **(operator, 2026-11-25)**. `docs/conformance/boundary-exceptions.toml` lists the places where a literal equals a registered name but does not branch on a client. Each entry names a file, or a file and an item, with a reason and its approval; the test does not flag a literal inside an entry's file or item. A change to the list stops for the operator, as a bar exception does (K§6.2.1). The first entry is the no-attribution guard, `crates/zygo-work/src/guards.rs` item `claude_attribution` (the `"claude"` literals at about `:1047`-`:1059` at `c21b32e0`): they match attribution text and do not branch on a client.
- Wire formats are not client branches. A `zygo-proxy` stage that keys on `Dialect` (the usage parse in the tee, the window-rewrite format, cache-marker placement) is allowed. Client policy (`insert_markers.<client>`, redaction spans, which clients get the window rewrite) reads the descriptor (K§6.4.2) **(lead)**.
- Output: each violation as `file:line:column` and the literal.

### 6.2 Conformance: registry, grades, score, bar (P4-2)

#### 6.2.1 Files and owners

| File | Content | Owner (the one writer) |
|---|---|---|
| `docs/conformance/capabilities.toml` | the capability registry: id, name, standard text, tier, criteria, and the bar exceptions | edited by hand in a PR; a change of standard text, tier or bar exception stops for the operator (DDR class, P3-8) |
| `docs/conformance/<client-id>.toml` | one grade file per client: pinned version, one grade per criterion, one documentation URL per capability; no bar exception | edited by hand in the PR that changes the client |
| `docs/conformance.md`, `docs/conformance.json` | the generated feature matrix, rollup and score | written only by the generator (K-D10); a hand edit fails the check |

#### 6.2.2 Registry form

```toml
[[capability]]
id = "A-7"
name = "Result trimming"
tier = "standard"            # core | standard | extra
standard = "Large tool results go through the result store (§10.1)."

[[capability.criterion]]
id = "A-7.1"
text = "Shell tool output over the threshold reaches the model as the §10.1 view."
```

The migration of A-1 to A-21, WA-1 and WA-2 **(author)**. Each criterion restates part of the §8.1 standard or the base section it cites. Tiers and the reasons for the core set are in K-D9.

| Cap. | Tier | Criteria |
|---|---|---|
| A-1 Transport | core | .1 Each model request goes to the proxy at the client dialect's prefix. .2 The credential is the session token, got through the client's key helper; the token is in no argument, no environment row and no file except the session's `token` file (and the `0600` passthrough files of §12.3); passthrough excepted (§12.3). .3 The client's own login and provider selection are off: no model request goes to a vendor host and no login prompt shows (passthrough excepted). .4 The key-helper command runs in each shell that the client uses on Linux, macOS and Windows. |
| A-2 Attribution | core | .1 `X-Zygo-Session` on each request. .2 `X-Zygo-Agent-Version` on each request. .3 `X-Zygo-Repo` on each request when the session has a repository id. .4 `X-Zygo-Work` on each request when a work item is set. |
| A-3 Model list | standard | .1 The picker shows the session's role list. .2 The default model is a role. .3 Each model alias of the client resolves to a role. |
| A-4 Subagent roles | standard | .1 Each built-in subagent maps to a role. .2 One `zygo-<role>` subagent exists per enabled role, with model `<role>`. |
| A-5 Per-session config | core | .1 Each zygo setting reaches the client through a per-session mechanism. .2 zygo writes no file of the user's own client config. .3 Each setting that zygo writes is in effect: no user or project file overrides it. |
| A-6 Local services | core | .1 `zygo mcp` is registered as a stdio MCP server and starts in the session. |
| A-7 Result trimming | standard | .1 Shell tool output over the threshold reaches the model as the §10.1 view. .2 File-read text output over the threshold, the same. .3 Search and listing tool output, the same. .4 Web-fetch output, the same (`n/a` for a client with no such tool). .5 The view takes the place of the tool's output in the tool's own result, not of an error or a feedback message. |
| A-8 Session context | core | .1 The §8.6 session text reaches the main loop at each session start (new, cleared, resumed). .2 The mechanics text (§8.6 item 7) reaches each `zygo-<role>` subagent. |
| A-9 Status | extra | .1 to .5: the profile, the work item, the context use, the compaction override and the session savings show on the client's status surface, or in `zygo status` when the client has no surface (the OpenCode precedent: its A-9.1 to A-9.5 grades in `docs/conformance/opencode.toml`). .6 Each status command that zygo writes runs in each shell the client uses. |
| A-10 Secrets hygiene | core | .1 The §8.4 denylist, with the client's rows, is applied to the client's environment. .2 The session token is not in the client's environment, passthrough excepted (§12.3). |
| A-11 Skills and plugins | standard | .1 The skills of the session's plugin snapshot load. .2 The other parts of each snapshot plugin (commands, agents, hooks, MCP servers) load in the client's form. |
| A-12 Model profile | standard | .1 `--profile <name>` applies through the session (§11). |
| A-13 Version | core | .1 The descriptor holds a minimum version. .2 `zygo doctor` reports the installed version against it. |
| A-14 Role prompts | standard | .1 The main role's prompt opens the session text. .2 Each `zygo-<role>` subagent's prompt opens with its role prompt. |
| A-15 Lead guard | standard | .1 When the main loop runs on `main_role`, its file-edit tools are refused. .2 Subagents keep their file-edit tools. |
| A-16 Compaction window | standard | .1 The client compacts at the session's soft window (§10.2 signal or a client setting). .2 A window change in a live session takes effect with no relaunch. .3 `zygo compact` works from the client's shell. |
| A-17 Model-triggered compaction | extra | .1 The `compact` tool is listed to the model. .2 A call of it compacts the calling conversation (§10.2.3 trigger 2). |
| A-18 Compaction instructions | extra | .1 zygo's keep/drop text (§10.6) reaches the client's compaction. .2 The text names the session's open units at the time of the compaction. |
| A-19 Work units and guards | standard | .1 `work_begin` is recorded with the calling agent's id (main loop or subagent). .2 Each tool call appends its §13.3 lines. .3 A subagent stop runs the claims guard. .4 A guard can refuse a tool call. |
| A-20 Role tool restriction | standard | .1 A `writes = false` role cannot use the edit tools, by definition, not by prompt. .2 A role can have a turn cap. |
| A-21 Tests | core | .1 A stub-agent launch test on Linux, macOS and Windows asserts the environment, the flags and the generated config. .2 A premise test finds each setting key, flag and environment row that zygo writes in the pinned `--help`, schema or capture. |
| WA-1 Resume | standard | .1 zygo has the client's session id (pre-assigned or captured) before the first turn ends. .2 A resume by that id reopens the conversation in a new zygo session. |
| WA-2 Turn signals | standard | .1 Turn start. .2 Turn end, also for a turn that the user interrupts. .3 Subagent start, for each subagent. .4 Subagent end, for each subagent. .5 A dispatch block can refuse a subagent call. |
| A-22 Update notices (new, K-D5 union rule: K§6.5; added to the registry by P4-10; A-22.2 and A-22.3 are `not-met` with a reason until P4-11) | extra | .1 A newer zygo release that the background check found shows on the client's status surface, or in `zygo status` and the launcher's exit line when the client has no surface. .2 The descriptor's `announces_own_updates` fact is measured by a premise under a zygo launch. .3 A newer client release reaches the user from exactly one party: the client (fact true) or zygo (flag on). |

A-22 is `extra`, not part of A-13 (core) **(author)**: the operator named A-13 as an example, but a notice is not needed to run a session, and a core criterion that waits for a live check would put both shipped clients below the bar.

#### 6.2.3 Grade file and evidence form

```toml
client = "claude-code"               # a client id of Agent::ALL
name = "Claude Code"                 # the display name in the docs/premises.md and docs/live-checks.md headers
version = "2.1.328"                  # the pinned minimum

[[cell]]
capability = "A-1"
url = "https://code.claude.com/docs/en/llm-gateway"

[[grade]]
criterion = "A-1.2"
status = "met"
evidence = ["premise:docs/premises.md#Q7", "test:crates/zygo-launch/tests/stub_launch.rs::<fn>"]

[[grade]]
criterion = "A-1.4"
status = "not-met"
reason = "PowerShell needs & before a quoted path; zygo writes the double-quoted form"
```

A bar exception is a registry entry, never a grade-file entry **(lead)**. A registry change already stops for the operator.

```toml
# docs/conformance/capabilities.toml
[[bar_exception]]
client = "claude-code"
criterion = "A-1.4"
reason = "Windows without Git Bash runs the key helper in PowerShell, which needs & before a quoted path; zygo writes the double-quoted path, and CI runs the command only in Git Bash"
approved = "operator 2026-11-25, docs/specs/2026-11-25-client-model-design.md K-D18"
```

Evidence kinds **(author; lead rulings I4, I5)**:

- `test:<path>::<fn>`: the file exists under `crates/` and, parsed with `syn`, holds `fn <fn>` with `#[test]`, `#[tokio::test]` or `#[tokio::test(…)]` among its attributes. A test with `#[ignore]` (any form) is not evidence. A test under a `#[cfg(…)]` (on the function or an enclosing module) counts, and the generator records its platforms (for example `unix`, `windows`) next to the evidence. Timing tests are never in CI (project rule), so a criterion that only an ignored or timing test proves uses premise evidence.
- `fixture:<path>`: a regular file under `fixtures/`, not a symbolic link, more than 2 bytes and not `{}` or `[]` (the `nontrivial` rule of `scripts/check-premises.sh:20-27`).
- `premise:<file>#<id>`: `<file>` is `docs/premises.md` or `docs/live-checks.md`; that file has exactly one row `| <id> |`; its result is exactly `verified` (`docs/premises.md`) or `pass` (`docs/live-checks.md`). Any other result (`failed`, `observed`, `not run: …`) does not count. The file name removes id collisions between the two files.

#### 6.2.4 Derived values

- **Cell (K-D7).** Cell value = criteria met ÷ applicable criteria (criteria that are not `n/a`). All applicable criteria met: `full`. Some met: `partial`, and the text of each unmet criterion is the "what is missing". None met: `none`, and the reasons are the "why". All criteria `n/a`: the cell is `n/a`.
- **Score (K-D8)** **(lead)**. Tier score = the mean of the cell values over the tier's cells that have one applicable criterion or more. A tier with no such cell shows `n/a` and leaves the weights. Total score = Σ(weight × tier score) ÷ Σ(weight) over the tiers that are not `n/a`; weights core 3, standard 2, extra 1. Each score shows as a percent rounded down to an integer; the rounding is the last step, on unrounded values.
- **Bar (K-D9).** A client meets the bar when each criterion of each core capability is `met`, or is `not-met` with a registry bar exception for that client. A core criterion is never `n/a`. A **failing core capability** is a core capability with a criterion that is neither `met` nor `not-met` with a registry bar exception for that client.
- **Rollup.** For each client, for each tier and for the total: the count of `full`, `partial`, `none` and `n/a` cells; the count of criteria `met` and of criteria applicable; and the bar verdict, `meets bar` or `below bar` with the id of each failing core capability.

#### 6.2.5 The check and the generator

One Rust integration test, `crates/zygo-core/tests/conformance.rs`, is the check and the generator **(author)**. `zygo-core` gets `toml` and `syn` as dev-dependencies only. It runs in `cargo test --workspace`, so on the three CI systems (`.github/workflows/ci.yml:34-45`) and in the release job (`release.yml:30`), with no new CI step. The test fails when:

1. a client of `Agent::ALL` has no grade file, or a grade file names no client of `Agent::ALL`;
2. a grade file lacks a grade for a criterion of the registry, or has a grade for an unknown criterion, or has one twice (K-D5);
3. a `met` grade has no evidence that counts by K§6.2.3 (an ignored test, a missing function, a missing fixture, or a premise row that is absent or not `verified`/`pass` does not count);
4. a `not-met` or `n/a` grade has no reason, or a core criterion is `n/a`;
5. a cell lacks an `https://` URL;
6. a registry bar exception names a client not in `Agent::ALL`, a criterion that is `met` for that client, a criterion of a non-core capability, or has no `approved` text; or a grade file holds a `bar_exception` key (an unknown key);
7. for a client of `Agent::ALL`, the grade file `version` differs from the client's built-in minimum version (`crates/zygo-config/data/agents.toml` until P4-6 moves it into the descriptor), from the `<name>: <version>` header of `docs/premises.md`, or from the `<name>: <version> or later` header of `docs/live-checks.md`. The existing test `minimum_agent_versions_match_premises` (`crates/zygo-config/tests/builtin.rs:223`) already checks the first two for both clients; P4-2 keeps it, and P4-6 makes it read `Agent::ALL`;
8. a supported client does not meet the bar (K-D9). Until P4-6 adds the descriptor's `supported` flag, each client of `Agent::ALL` counts as supported (each has a CLI subcommand today);
9. a fixtures folder `fixtures/<client-id>/` of a client of `Agent::ALL` is missing or empty;
10. `docs/conformance.md` or `docs/conformance.json` differs from the generated text. Line ends are compared after CR LF becomes LF.

With `ZYGO_WRITE_CONFORMANCE=1` in its environment, the test writes both files and passes item 10. The Markdown file starts with a line that names the generator and says that a hand edit fails the check; the JSON file has the same text in a `generated_by` key.

Input rules for the files, which the test treats as hostile **(author)**: TOML with unknown keys refused; an id that does not match `^(A|WA)-[0-9]+(\.[0-9]+)?$` refused; an evidence path that is absolute, holds `..`, or leaves the repository after link resolution refused; a reason or URL with a line break, or over 300 characters, refused; the generator escapes `|` in each text it puts in a table.

`scripts/check-conformance.sh` and `scripts/test-check-conformance.sh` are deleted.

`scripts/check-premises.sh` becomes driven by data, not by a fixed list or by the client enum **(lead)**. It takes the names and versions from the header lines of `docs/premises.md` itself (each `<Name>: <version>` line before the first `##` heading), in place of `:32-44` and the `case` at `:82`. Each header name uses the minimum rule, except `act-lite` (exact) and `tmux` (minimum with one letter), as today. It checks each row of the table, not a fixed id list, and it still requires the ids of its lists today (`P1` … `Q7`, `PW1` … `PW16`), so it never passes on a table that lost rows. A `Codex: 0.172.0` header and the PC rows so pass before the Codex client exists. The conformance test compares versions only for clients of `Agent::ALL` (item 7).

#### 6.2.6 The three outputs: feature matrix, rollup, score

The generator gives three named outputs **(operator)**, each from the K§6.2.4 rules. Each is in both files.

1. **Feature matrix.** Capability × client. One row per capability, grouped by tier (core, then standard, then extra), registry order inside a tier. One column per client, in `Agent::ALL` order. A cell is the derived grade and the URL: `full <url>`; `partial (missing: <id> <text>; …) <url>`; `none (<id>: <reason>; …) <url>`; `n/a (<reason>)`. Rows per capability keep the table readable as clients are added (today `docs/conformance.md` has two tables with one row per client: 22 columns for A-1 to A-21 at `:8-11`, and 3 columns for WA-1 and WA-2 at `:17-20`).
2. **Rollup.** One table per client, with one row per tier and one total row: full, partial, none, n/a, criteria met / applicable. Under it, the bar verdict: `meets bar` or `below bar: <failing core capability ids>`; the bar exceptions with reason and approval; and the release state (`supported`, `meets bar, not released`, `below bar`).
3. **Score.** One table: client, total score, core, standard and extra tier scores, each a percent by K§6.2.4, or `n/a` for a tier with no applicable cell.

After the three outputs, `docs/conformance.md` lists, per client, each unmet criterion with its reason, each `n/a` with its reason, and the evidence of each met criterion.

**Machine-readable form** **(author)**: `docs/conformance.json`, written by the same generator and checked by the same test (item 10). CI and later tools read it, not the Markdown. Its form:

```json
{
  "generated_by": "crates/zygo-core/tests/conformance.rs (ZYGO_WRITE_CONFORMANCE=1); do not edit",
  "weights": {"core": 3, "standard": 2, "extra": 1},
  "clients": [{
    "id": "claude-code", "name": "Claude Code", "version": "2.1.328", "supported": true,
    "matrix": {"A-1": {"tier": "core", "grade": "partial", "value": 0.75, "url": "https://code.claude.com/docs/en/llm-gateway",
                       "missing": [{"criterion": "A-1.4", "text": "…", "reason": "…"}], "na": []}},
    "rollup": {"core":     {"full": 7, "partial": 1, "none": 0, "na": 0, "met": 19, "applicable": 20},
               "standard": {"…": "…"}, "extra": {"…": "…"}, "total": {"…": "…"},
               "bar": {"verdict": "meets bar", "failing": [], "exceptions": ["A-1.4"]},
               "release": "supported"},
    "score": {"total": 89, "core": 96, "standard": 75, "extra": 94}
  }]
}
```

The numbers are an example that follows K§6.2.4: core = (7 × 1 + 0.75) ÷ 8 = 0.96875 → 96; standard = 0.75 (12 cells whose values sum to 9) → 75; extra = (5/6 + 1 + 1) ÷ 3 = 0.944 → 94; total = (3 × 0.96875 + 2 × 0.75 + 1 × 0.944) ÷ 6 = 0.8918 → 89. Each cell also carries its `value`, so a reader can recompute each score.

The JSON file is a repository file, not a `zygo` command output, because the grade files are not in the binary and the data changes only with a commit **(author)**. Keys are written in a fixed order, so the file is byte-stable.

#### 6.2.7 Release gate for a client below the bar (P4-6)

A client with `supported = false` in its descriptor **(author; lead ruling I14: at launch, for each entry point)**:

- The gate is in the launcher, before the proxy session is created, so it holds for each entry point that starts a client: `zygo <verb>`, `zygo ws run` (a member pane, so also `zygo up`). The loadout check (`crates/zygo-config/src/check.rs`, run by `zygo config check` and `zygo up`) reports a pane of that kind as an error with the same text. The launch exits 2 (a pane takes the `ws run` failure path) with `zygo: <name> is below the conformance bar; see docs/conformance.md`, unless `ZYGO_PREVIEW_CLIENT=<client-id>` is set.
- The client is hidden from `zygo --help` and is not listed by `zygo doctor` (§8.1 A-13 report), `docs/install.md` or the installers.
- The gate function takes the descriptor and its `supported` value as input, so its tests use a real client (Codex) with both values and never pass on an empty set.

Check item 8 forbids `supported = true` below the bar. A client that meets the bar can stay unreleased; the document shows `meets bar, not released`.

### 6.3 Codex client: requirements for its own design (P4-7)

The Codex client gets its own design document (P4-7), designed against the built client contract **(operator)**. This section and the Codex parts of K§6.4 (the Codex client policy rows of K§6.4.2 and the arming rule of K§6.4.5) are the input to that design and stay its requirements. The premises of K§6.3.7 are built now (P4-3); the rest is built after the Codex design (P4-9).

#### 6.3.1 Launch: `zygo codex`

`zygo codex [--profile <p>] [--server <url>] [--compact <w>] [--yolo] [-- <codex args>]` runs the §8.2 sequence. The adapter adds this **(author unless marked)**:

1. **Preflight.** It first finds the effective working directory: the value of `-C`/`--cd` in the user's arguments (the last one, as Codex reads them), else the launch directory **(lead)**. From there up to the repository root it reads the project Codex files (`.codex/config.toml` and each hook file that PC4 names) by the safe-read rule of §8.4. It records each hook that zygo did not write, for K-D13.
2. **Per-session `CODEX_HOME`** = `$ZYGO_SESSION_DIR/codex-home/` (`0700`) **(K-D11)**. It holds:
   - `config.toml`: `model_provider = "zygo"`; `[model_providers.zygo]` with `wire_api = "responses"`, `base_url = "<proxy>/openai/v1"`, `auth.command` / `auth.args` = `zygo hook token` (the key helper; `ModelProviderAuthInfo` in the pinned schema), `http_headers` with the A-2 headers, `supports_websockets = false`, and `capabilities.remote_compaction = "unsupported"` (so Codex compacts by a normal model request; the pinned schema's `RemoteCompactionSupport` is a string, and Codex 0.172.0 refuses `false` under `--strict-config`: "invalid type: unit variant, expected string only", PC-4 `fixtures/codex/config/strict-config.txt`); `model = "<main_role>"`; `model_context_window` and `model_auto_compact_token_limit` (§10.2.5 rule); `[mcp_servers.zygo]` = `zygo mcp --agent codex` with `default_tools_approval_mode = "approve"` (without it Codex 0.172.0 under `approval_policy = "never"` refuses every call of the server's tools: "MCP tool call requires approval, but approval policy is never", after a `PermissionRequest` hook; PC-4 `fixtures/codex/hooks/outputs.json` `mcp_without_approval_mode`, `fixtures/codex/hooks/PermissionRequest.json`; the key passes `--strict-config`, PC-4 `fixtures/codex/config/strict-config.txt`); `shell_environment_policy` that keeps the §8.4 denylist out of tool shells; `developer_instructions` (K§6.3.3); `compact_prompt` (A-18); the status-line items (A-9); the skills paths of the plugin snapshot (A-11). zygo does not set `check_for_update_on_startup`, so Codex keeps its own update notice (K§6.5.4);
   - `agents/zygo-<role>.toml`: one per enabled role except `main_role`, with `model = "<role>"`, the description, and `developer_instructions` = role prompt + one space + the mechanics text (§8.5 rule); and the overrides of the built-in roles: `explorer` → `read`, `worker` → `code`, `default` → `code` (§8.5: a subagent with no model is `code`);
   - the model catalog file (A-3) with one entry per role. Each entry carries `base_instructions` set to Codex's own base prompt (not empty) and `apply_patch_tool_type = "freeform"`; the other ModelInfo fields are as in `fixtures/codex/models/catalog-patch.json`. Codex sends a catalog entry's `base_instructions` in place of its own base prompt and offers no `apply_patch` tool without `apply_patch_tool_type` (PC-3 G2, F1). Two open premises, which the Codex launch work (P4-9) measures before it relies on them: (a) the source of Codex's own base prompt for the catalog (PC-3 run `p` copied it at run time from a captured request); (b) whether Codex reads `base_instructions: ""` as "use my default" or as an empty prompt. With this file Codex sends no `/models` request (PC7, `fixtures/codex/models/result.txt` `d: models_request=0`), so the K§6.4.1 models route is no substitute for it;
   - `hooks.json` (K§6.3.2), and the trust records of K-D13 when PC4 allows them;
   - `sessions`: a link (a symbolic link; a directory junction on Windows) to the shared store `<state_dir>/codex/sessions/` (K-D22).
3. **Precedence** **(lead)**. Every key that zygo owns is passed as a `-c key=value` argument, not only the routing keys, so a project `.codex/config.toml` cannot override any of them (CLI overrides outrank project config: PC3). `config.toml` in `CODEX_HOME` holds the same keys, so the session folder shows the whole config. Settings that are files (the agent files, `hooks.json`, the model catalog) cannot go by `-c`; PC3 measures whether a project file overrides them, and A-5.3 is graded by what is true. No argument holds the token. On Windows, when the command line would pass 32,000 characters (the `developer_instructions` text is the large one), the launch exits 2 with `zygo codex: the session text is too long for the Windows command line`.
4. **Environment.** The common denylist (§8.4), plus the Codex rows: each `CODEX_*` variable (K-D29). Then `CODEX_HOME` and the §8.4 `ZYGO_*` rows, with `ZYGO_AGENT=codex`.
5. **Run** `codex` with the user's arguments after the inserted ones. A resume inserts `resume <id>` first (descriptor concern 7).
6. **Exit.** As §8.2. When a Codex session id is known, zygo prints `zygo codex -- resume <id>`.

The user's own `~/.codex` is not read in a zygo session (K-D32). A-5.2 holds: zygo writes no file in it.

#### 6.3.2 Hooks

`hooks.json` registers `zygo hook <name> --agent codex` for: `SessionStart` (session text, session-id capture), `PreToolUse` (work-unit begin, guards), `PostToolUse` (result store, §13.3 lines), `UserPromptSubmit` (`/zygo:` verbs, turn start), `Stop` and `Interrupt` (turn end), `SubagentStart` and `SubagentStop` (WA-2, claims guard). The exact event names, matchers and payload fields come from PC5.

The Codex hook-input function (descriptor concern 2) maps the Codex payload to the common form: the event name, the tool name (each Codex shell tool to `Bash`, `apply_patch` to the edit class, MCP tool names by concern 5), the tool input, `session_id`, and `agent_id` when present. The hook-output function (concern 3) renders the PC5 shapes.

The lead guard (A-15) is not installed on Codex while no hook payload tells the main loop from a subagent (K-D24). The session text then does not say that edits are refused.

#### 6.3.3 Session text

With hooks on, `SessionStart` gives the §8.6 text as additional context, as for Claude Code (A-8.1). The `zygo-<role>` agent files carry the mechanics text (A-8.2). With hooks off (K-D13 last branch), the main `developer_instructions` in `config.toml` carries the §8.6 text as built at launch, and the resume note of §8.6 item 8 is not given.

#### 6.3.4 Compaction

`model_context_window` is the served model's `context_window`; `model_auto_compact_token_limit` is its soft window, or the `--compact` value (§10.2.5 rule) **(author)**. The proxy arms the window rewrite for Codex (K§6.4.5) only when PC8 shows that a raised `usage` in `response.completed` makes Codex compact before its next request; else the descriptor has it off and A-16.2 is `not-met`. `zygo compact` in a Codex session then exits 2 with the concern-10 text.

#### 6.3.5 Workspace members

- **WA-1.** Resume kind: capture. The `SessionStart` hook records the Codex `session_id` in the member record, as `record_session` does for Claude Code (`crates/zygo-hooks/src/workspace.rs:388`). `resume_args(id)` = `resume <id>`. `has_session_arg` is true when the user's arguments hold the `resume` or `fork` subcommand. The shared session store (K-D22) makes the resume work in the new zygo session's `CODEX_HOME`.
- **WA-2.** `UserPromptSubmit` starts a turn; `Stop` and `Interrupt` end it; `SubagentStart` and `SubagentStop` give the subagent events; a `PreToolUse` deny on the subagent spawn tool (name from PC5) is the dispatch block.

#### 6.3.6 Hook trust (K-D13)

zygo never trusts a hook that it did not write. The order:

1. When PC4 shows that trust records written into the per-session `CODEX_HOME` before launch make Codex run zygo's hooks, the adapter writes one record for each zygo hook command, for that command's hash only. PC4 (Codex 0.172.0, PC-4 round 3): Codex keeps hook trust in CODEX_HOME's `config.toml` as `[hooks.state."<hook source file>:<event, snake case>:<group index>:<handler index>"]` with `trusted_hash = "sha256:<hex>"`, written when the user picks "Trust all and continue" in the TUI's "Hooks need review" prompt; such records written before launch are honored (replay), and a record with a wrong hash is not. The hash rule is not found: the hash changes with the event, the matcher and the handler (a `timeout`), not with the source file (`fixtures/codex/trust/hash-rule.json`), and no candidate of 59,383 per record matched. zygo cannot compute a record for its own command, so branch 1 is not yet open; the adapter takes branch 2 or 3.
2. Else, when the preflight found no hook that zygo did not write (no project hook file, no hook in a project `.codex/config.toml`), the adapter passes `--dangerously-bypass-hook-trust`.
3. Else the adapter writes no hooks. It prints one line: `codex: zygo hooks off: the project has its own Codex hooks`. The cells that need hooks (A-7, A-8.1 at start, A-15, A-19, WA-1, WA-2) are graded from what remains; the grade file states the branch.

The grade file grades branch 1 or 2 as the normal launch. Branch 3 is a known limit in the generated document.

#### 6.3.7 Premises (P4-3, K-D14)

Each provisional Codex grade becomes measured by a premise row in `docs/premises.md` (`PCn`) with fixtures under `fixtures/codex/`. The method is the existing one (`docs/premises.md` "Method"): Codex runs under `env -i` with a temporary `HOME` and `CODEX_HOME`, a dummy key built at run time, and `base_url` = `scripts/premises/mock_upstream.py` directly, with no zygo between. `mock_upstream.py` gains a `/v1/responses` SSE answer and a `/v1/models` answer. `HTTPS_PROXY`, `HTTP_PROXY` and `ALL_PROXY` point at the mock, which refuses and records each `CONNECT` and each absolute-URI request, so a request to any other host is seen and fails. No run uses an OpenAI or ChatGPT login. No step runs `codex login`.

| Id | What it measures | Feeds |
|---|---|---|
| PC1 | The provider table routes each model request to `base_url` with the key-helper bearer; the paths Codex calls; each other host it tries (with `CONNECT` records); processes left after exit | A-1, A-10 |
| PC2 | `http_headers` and `env_http_headers` reach each request | A-2 |
| PC3 | `CODEX_HOME` replaces `~/.codex`; `-c` outranks a project `.codex/config.toml` for each key zygo owns; whether project agent files or hooks override zygo's files; `-C`/`--cd` sets the directory whose `.codex/` Codex reads | A-5, K-D13 |
| PC4 | Where hook trust is kept; whether records written before launch are honored; which hook sources load; the effect of `--dangerously-bypass-hook-trust` | K-D13 |
| PC5 | Hook events, matchers, payload fields (`session_id`, `agent_id`, tool names for shell, `apply_patch`, MCP and subagent spawn) and output shapes (context, deny, block, result replacement) | A-7, A-8, A-15, A-19, WA-2 |
| PC6 | Agent TOML files load; the model name each sends; built-in role overrides; whether a subagent request carries a marker | A-4, A-14, A-20, K§6.4.5 |
| PC7 | The model catalog file and the `/models` shape; picker content | A-3 |
| PC8 | `model_context_window`, `model_auto_compact_token_limit`; the effect of a raised `usage` in `response.completed`; the compaction request shape; `compact_prompt` | A-16, A-17, A-18 |
| PC9 | `mcp_servers` stdio start; the model-visible name of a zygo tool | A-6, A-17 |
| PC10 | `session_id` from `SessionStart` equals the rollout id; `codex resume <id>` with a new `CODEX_HOME` whose `sessions` is a link to a shared store | WA-1 |
| PC11 | `tui.status_line` items | A-9 |
| PC12 | `shell_environment_policy` effect in tool shells | A-10 |
| PC13 | The user-role items that Codex injects (environment context, project instructions), for the redaction span rule | concern 9 |
| PC14 | The error shape that Codex treats as "context too long" | K§6.4.4 |
| PC15 | `skills` paths load a skill folder | A-11 |
| PC16 | Codex's own default, against the mock with no zygo between (K-D25): with no `check_for_update_on_startup` key in `config.toml`, the key is in effect at its default (`true`, the pinned schema), and a TUI start requests Codex's release source: `CONNECT` records that the mock refuses, counted per host: a host that the TUI start reaches more often than a control TUI run with the key `false`, by the same difference in every other key-default TUI run and with no such excess in the `exec` runs (a `CONNECT` shows the host, not the path); the host is recorded against the K§6.5.4 proposal (`registry.npmjs.org`). The measurement under a zygo launch is P4-9's (the Codex launch keeps the key unset, K§6.3.1) and P4-11's (`announces_own_updates`); the owner-run live check LC-CX1 sees the notice text against a real newer release | K§6.5.4 (A-22.2 cites the P4-11 premise row for Codex under a zygo launch, K§11 item 52) |
| PC17 | Codex accepts the Responses event sequence of zygo's encoder (`crates/zygo-proxy/src/translate/responses_client.rs`, `Encoder`), served from the mock as the encoder makes it at run time: the extra `output_item.added`, `content_part.*`, `*.delta` and `*.done` events and a `response.completed` whose `output` is `[]`; text, text after a dropped server tool, a `function_call`, two `function_call`s in one response, a `custom_tool_call` of the freeform `apply_patch`, text then a `function_call`, and `reasoning` items (with the zygo mark; from `redacted_thinking`; with no `encrypted_content`) each complete the turn, run the tools and come back in the next request as sent (Codex may add only `content` and `encrypted_content` on a reasoning item, each `null` and absent from the sent item: absent and null mean the same) | K§6.4.3 stream obligations 1-5 and 7 (`ping` dropped); RS-4 wires the encoder in on this row |
| PC18 | With `request_max_retries` and `stream_max_retries` 0, Codex ends the turn on the encoder's `response.incomplete` (`max_output_tokens`) and on its `response.failed` (after an upstream error; after a half-open `function_call`, with no tool run) after exactly 1 request, with a non-zero exit and its exact `turn.failed` message. With Codex's default retries (no keys, as a zygo launch) the request count per ending is recorded, per `result.txt`: `max_output_tokens` 1, `response.failed` after an upstream error 6, after a half-open call 6; each Codex retry is a new upstream request. Obligation 9's endings (`upstream ... too long`) are not served and were not measured against real Codex. The refusal ending is PC19 | K§6.4.3 stream obligations 5, 6 |
| PC19 | With Codex's default retries (no retry keys, as a zygo launch), the encoder's ending for `stop_reason` `refusal` ends the turn after exactly 1 upstream request, Codex shows the refusal text once, and a resumed next turn (`codex exec resume --last`) completes with the refused turn in its request. Recorded beside it, per `result.txt`, four candidate endings served after one base response: `response.incomplete` `content_filter` (RS-2's mapping), `response.completed` after a `message` item with a `refusal` part, the same with an `output_text` part, and `response.failed` with code `invalid_prompt`; for each, the request count, what Codex shows and whether the next turn works. The row covers the text-only refusal; the ending of a refusal after a call item (`response.failed` `invalid_prompt`) is inferred from the `rfd` candidate, which was measured text-only | K§6.4.3 stream obligation 5; RS-12 chose the mapping on this row |

`docs/premises.md` and `docs/live-checks.md` get the header line `Codex: 0.172.0` (and `Codex: 0.172.0 or later`). The premise check is driven by data (K§6.2.5), so the header and the PC rows pass before the Codex client exists; the grade file `docs/conformance/codex.toml` comes with the Codex client (P4-9), in the same row as its `Agent` entry (check item 1). The premise test (`crates/zygo-launch/tests/premise.rs`) gains a Codex part: each key zygo writes is in `fixtures/codex/config.schema.json` (from the pinned schema, `rust-v0.172.0` tag), each flag in `fixtures/codex/help.txt`. The premise harness also runs the generated config once with `--strict-config`. The launch does not pass `--strict-config`, because a project config with a key newer than the pin would then stop the launch **(author)**.

#### 6.3.8 Provisional Codex grades

From research on Codex CLI 0.172.0 (its `--help`, `codex features list`, the pinned config schema and the vendor configuration, hooks and subagent pages). Not measured. Each grade is replaced by the PC premises before P4-9.

| Cell | Provisional | Basis |
|---|---|---|
| A-1 | full | provider table, `wire_api = "responses"`, key-helper command |
| A-2 | full | `http_headers` |
| A-3 | partial | `-m`, model catalog file; picker content not seen |
| A-4 | full | agent TOML files with `model`; built-in roles can be overridden |
| A-5 | full | `CODEX_HOME`, `-c` |
| A-6 | full | stdio `mcp_servers` |
| A-7 | partial | `PostToolUse` can replace a result with feedback text (A-7.5 open); MCP output replacement not supported |
| A-8 | full | `SessionStart` context; `developer_instructions` |
| A-9 | full by the fallback rule, else partial | built-in status items; the provider item is experimental; `zygo status` |
| A-10 | full | denylist, `shell_environment_policy` |
| A-11 | partial | skills paths; plugins only from a marketplace |
| A-12 | full | proxy-side |
| A-13 | full | `codex --version` |
| A-14 | full | `developer_instructions` main and per agent |
| A-15 | none | no agent id on tool hooks (K-D24) |
| A-16 | partial | launch-time window; live change only after PC8 |
| A-17 | partial | tool reachable; trigger depends on PC8 and PC9 |
| A-18 | partial | `compact_prompt` is static, so A-18.2 is not met |
| A-19 | partial | subagent events with `agent_id`; no agent id on tool hooks (A-19.1) |
| A-20 | none or partial | no per-agent tool removal or turn cap seen; PC6 decides |
| A-21 | full | stub launch test and premise test (built in P4-9) |
| WA-1 | full or partial | capture, `codex resume <id>`; PC10 decides |
| WA-2 | partial | `Interrupt` gives a turn end on Esc; spawn tool name from PC5 |
| A-22 | partial | no custom status-line text (A-22.1 by `zygo status` and the exit line); `check_for_update_on_startup` defaults to `true` (PC16: Codex's own default, no zygo between); A-22.2 and A-22.3 after the P4-11 premise row for Codex under a zygo launch |

### 6.4 Proxy: the Responses client dialect (P4-4, K-D12)

#### 6.4.1 Routes

- `POST /openai/v1/responses`: the model call, streaming and non-streaming.
- `GET /openai/v1/models`: the session's role list in the shape that PC7 records. Each entry's `base_instructions` is empty and it has no `apply_patch_tool_type`. Codex sends a non-empty `base_instructions` in place of its own base prompt (PC-3 G2), and how it reads `""` is open (K§6.3.1 item 2, premise (b)), so the Codex launch supplies both values through the catalog file (K§6.3.1 item 2: Codex's own base prompt and `apply_patch_tool_type = "freeform"`) and must not rely on this route; with that file Codex sends no `/models` request (PC7, `fixtures/codex/models/result.txt` `d: models_request=0`).
- Each other path under `/openai/v1/` that PC1 shows Codex calls: served when its function is in this section, else `404` in the OpenAI error shape. `POST /openai/v1/chat/completions` keeps `404 zygo_route_not_in_v1` (K§3).

Stage 1 is the existing identity rule (`crates/zygo-proxy/src/identity.rs:22-35`): `X-Zygo-Session` and the bearer token. Errors on these routes use the OpenAI shape (`errors.rs:34-41`).

#### 6.4.2 Pipeline

The proxy accepts a route by dialect, not by client **(lead)**. A Responses request from any session is served; so the Responses-route tests in P4-4 use an existing client's session (a Claude Code session), and the Codex client policy rows come with the Codex build (P4-9).

The stages run on the client's body, with `ctx.dialect = Openai`. Translation is per candidate at forward time (`forward.rs:784-800`). Two kinds of rule apply **(lead)**:

- **Wire format, keyed by `Dialect`** (a proxy concept; K§6.1.4 allows it): the usage parse in the tee, the window-rewrite format, and cache-marker placement in the body built for the upstream.
- **Client policy, keyed by client through the descriptor**: `[cache] insert_markers.<client>`, the redaction harness spans, and whether the window rewrite is armed for the client.

| Stage | Responses client obligation |
|---|---|
| Identity, body limit | unchanged |
| Resolution (§7.4) | `model` is a role, custom role or model id, as for Messages |
| Redaction (§16) | the secret pass on each text; the content pass on `input` (`Layout::Input`, `crates/zygo-redact/src/content.rs:195-205`); harness spans by client policy (Codex: PC13, in P4-9) |
| Cache markers (§10.4) | placed at forward time on the Messages body built for the upstream (`responses_attempt`, `forward.rs:988-1020`, after the facts and the schema rewrite), only for an Anthropic upstream, by `insert_markers.<client>` (Codex default `true`, in P4-9: Codex sends no Anthropic markers). The placement is §10.4 with one addition for this body: `responses_client::request` builds `system` as one string (the instructions and the leading developer text), so the marker first makes that string one text block, `[{"type":"text","text":<the string>,"cache_control":{"type":"ephemeral","ttl":"1h"}}]`; without it only the tools would be in the cached prefix. A body that already holds a marker where Anthropic accepts one (on a `system` block, on a content block of a `messages` entry, or at the top level of a `tools` entry) gets none; a `cache_control` key inside a user value (a tool parameter schema, the `output_config.format` schema, a `tool_use` input) is not a marker and does not stop the markers. A Responses upstream gets no markers: OpenAI caches prompts by itself, and `prompt_cache_key` passes. A chat upstream gets none, as today. This is a change for the Responses client only: the Anthropic client keeps its stage at request time (`cache.rs:58-71`), and its string `system` stays unmarked (§10.4) |
| Window (§10.2) | the rewrite edits the usage of the last usage event in the stream that the tee sees (K§6.4.5), when armed by client policy |
| Usage (§14) | the tee parses usage per upstream stream dialect (C3 below) |
| Attribution (A-2) | unchanged |

**The tee and the stream order (C3)** **(lead)**. Today the tee parses only Anthropic events (`message_start`, `message_delta`: `crates/zygo-proxy/src/tee.rs:71-83`). That is enough today, because a non-Anthropic upstream stream is first mapped to Messages events and then goes through the tee (`forward.rs:534`, `:1469-1479`). The order per pair for a Responses client:

| Upstream | Stream order | What the tee parses and rewrites |
|---|---|---|
| Anthropic | upstream Messages SSE → tee → Messages-to-Responses encoder → client | Messages events (as today) |
| chat | upstream chat SSE → existing chat-to-Messages mapper → tee → encoder → client | Messages events (as today) |
| Responses | upstream Responses SSE → tee → client (same dialect, no encoder) | the `response.usage` of the Responses `response.completed`, `response.incomplete` or `response.failed` event (new) |

So the tee gains one parser: the `response.usage` of a Responses `response.completed`, `response.incomplete` or `response.failed` event. Each ends a turn with its counts, and the tee reads the usage of each of the three, whatever its reason: `response.incomplete` with `max_output_tokens` and with `content_filter` (an OpenAI upstream sends `content_filter`, and this same-dialect path passes it through unchanged, stream obligation 6), and `response.failed`. A 2xx stream in the Responses form that ends with no usage read (no terminal event, or a terminal line over `HOLD_MAX`, which the tee does not read) records zero counts and the adjustment code `usage_unread` on its usage row, so the zero is visible. The counts are read by the §14 rule of the translated paths: `input_tokens` less `input_tokens_details.cached_tokens` as input, that cached count as cache reads, no cache writes, `output_tokens` as output. The forward step tells the tee and the response stages which form they see: the Responses form only for a Responses upstream answering a Responses client (no mapper, no encoder), the Messages form for every other pair. Without it the same-dialect path records no usage. A non-streaming same-dialect response is measured from the `usage` object of the Responses JSON body, by the same rule; the translated non-streaming paths are measured as today, on the Messages form. The encoder always runs after the tee, so a rewrite made on a Messages `message_delta` reaches the client inside `response.completed`.

#### 6.4.3 Translation pairs

Messages is the pivot **(author, K-D21)**:

- **Responses client → Responses upstream: same dialect, no pivot.** The body goes as received, except: `model` := the candidate's `upstream_model`; `store` := `false`; `include` := the client's array with `reasoning.encrypted_content` appended when it is not in it (absent or `null` → `["reasoning.encrypted_content"]`; another JSON type → `400 zygo_bad_request` "include must be an array", for a request whose candidate list has an `openai` candidate, before any candidate is tried; with no `openai` candidate, `include` is safe to ignore); the stage edits. Custom tools (the freeform `apply_patch`), `namespace` tools and reasoning items pass natively: request obligations 1-10 below are for the Messages pivot only, so no tool is flattened. Reasoning bytes pass unchanged, with no zygo mark **(lead)**. A candidate change between turns can so send another provider's reasoning item; the upstream's error then reaches Codex by K§6.4.4.
- **Responses client → Anthropic upstream.** New request translator Responses → Messages. New response translator Messages SSE (and JSON) → Responses SSE (and JSON).
- **Responses client → chat upstream.** Responses → Messages (new) → chat (existing `translate/chat.rs`); chat SSE → Messages SSE (existing `translate/sse.rs`) → Responses SSE (new). Two linear passes (K-C2).

Candidates of a Responses client **(lead)**: per provider, the first dialect it speaks of `openai` (the same-dialect pair above), `anthropic`, then `openai-chat`. A model with no candidate answers `400 zygo_no_provider`.

Top-level request fields **(lead)**. Each top-level field is in exactly one list; an unknown field gives `400 zygo_bad_request` that names it:

- **Mapped:** `model`, `instructions`, `input`, `tools`, `tool_choice`, `parallel_tool_calls`, `max_output_tokens`, `temperature`, `top_p`, `reasoning`, `stream`, `text` (`text.format` of type `text` or `json_schema` (request obligation 11), and `text.verbosity`; another format type → 400 toward a non-Responses upstream; another `text` key → 400 that names it). `text.verbosity` must be `low`, `medium` or `high`; toward a Messages or chat upstream it is checked and not sent, as Messages has no counterpart.
- **Safe to ignore** (used by the proxy or dropped toward a non-Responses upstream; passed toward a Responses upstream, with `store` forced to `false`): `store`, `include`, `prompt_cache_key`, `metadata`, `client_metadata` (Codex's request metadata: thread, turn and window ids, installation id, sandbox and workspace state; every PC-3 request carries it), `service_tier`, `user`, `safety_identifier`, `stream_options`, `truncation` (only `"disabled"`; another value → 400 that names `truncation`).
- **Refused** (K-D31): `previous_response_id`, `background`, `conversation`, `prompt` (a stored prompt) → `400 zygo_bad_request` "zygo keeps no response state; send the full input with store=false".

Request obligations (Responses → Messages). Exact field mapping is left to the builder; acceptance tests check each line:

1. `instructions` and the leading `developer`/`system` items (those before the first item of another kind) become the system text, joined with a blank line. A later `developer`/`system` item keeps its place between turns as a `text` block of a `user` message; no developer text is dropped.
2. `message` items map by role (`user`, `assistant`); items that map to the same role in a row join one message, so roles alternate; in a user message `tool_result` blocks come first. `input_text` and the assistant's `output_text` and `refusal` map to text blocks; `input_image` to an image block (a `data:` URL → base64 source, another URL → url source); `input_file` whose `file_data` is a base64 PDF to a document block, any other file to the text `[document omitted: the model cannot read it here]` with `documents_dropped:<n>` (§7.6 document rule). Another part type, or an `input_image` with no `image_url` → 400 that names it. A string `input` is one user text message. An empty text block is dropped everywhere (user, assistant, a later developer item, a tool output's parts), and a message left with no block is dropped. The translated conversation starts with a `user` message; an empty one, or one that starts with the assistant → 400.
3. `function_call` items become `tool_use` blocks (consecutive calls in one assistant message); `function_call_output` items become `tool_result` blocks (consecutive outputs in one user message). `call_id` is kept as the tool-use id. A `function_call` item with a string `namespace` (a call of a tool inside a `namespace` tool, PC-5: `fixtures/codex/agents/requests/pc6-a-0003.json`) becomes a `tool_use` of the flat name (obligation 6); a `namespace` of another JSON type → 400. Each call's `call_id` is unique; each output answers an earlier unanswered call of its kind (`function_call` or `custom_tool_call`) with the same `call_id`; each call has its output before the next non-empty `user` message and before the end of `input` (a `developer` item between them is allowed). Otherwise → 400. The Messages body keeps tool adjacency: an assistant message with `tool_use` blocks has no thinking block after its first `tool_use`, and the next message is a user message with a `tool_result` for each of them (other user content after); an order that does not fit → 400.
4. `custom_tool_call` and its output map the same way, with input `{"input": "<text>"}`. A `custom` tool definition becomes a function tool with one required string parameter `input`; a `grammar` format's `definition` becomes that parameter's `description`; adjustment code `custom_tool_as_function`.
5. `reasoning` items with a zygo mark (made by the encoder of this path) decode to `thinking` or `redacted_thinking` blocks for the same provider and model; a decoded block keeps only its known keys (`thinking`: `type`, `thinking`, `signature`; `redacted_thinking`: `type`, `data`). Others are dropped with `reasoning_dropped:<n>`.
6. Tool definitions: `function` maps to a Messages tool (`parameters` → `input_schema`); `custom` by item 4; a `namespace` tool's nested `function` tools become function tools under the flat name `<namespace>__<name>` when that is 1-64 characters of `[A-Za-z0-9_-]`, else that string with each other character replaced by `_`, cut to 47 characters, then `_` and the first 16 hex digits of SHA-256 of `<len(namespace)>:<namespace><len(name)>:<name>` (lengths in UTF-8 bytes, decimal); a nested tool of another type is dropped with `tools_dropped:<type>:<n>`; a `namespace` tool with no string `name` or no `tools` array → 400. Every other type (hosted tools such as `web_search`) is dropped with `tools_dropped:<type>:<n>`. A flat name equal to another sent tool's name is a duplicate. One name stands for one tool across `tools` and the call items of `input`: a name of a nested tool (sent, or the flat name of a namespaced `function_call` item) that is also the name of a function or `custom` tool, or of another (namespace, name) pair → 400 (a call of it could not return under one kind). One exception: a `function_call` item with no `namespace` whose name is the flat name of a nested tool sent in `tools` is a call of that tool (Codex before 0.172 named MCP tools `mcp__<server>__<tool>`; a session resumed across an upgrade holds such calls): accepted, its `tool_use` keeps that name, and a model call of it still returns with `name` and `namespace` (stream obligation 2). Two sent tools with the same name → 400. Each `tool_use` name of the history that no sent tool defines is declared as a tool with description `history only` and input schema `{"type":"object"}`, with `tools_from_history:<n>`; when no tool was left to send, `tool_choice` is then `none`. Hosted-tool input items (`web_search_call`, `file_search_call`, `computer_call`, `computer_call_output`, `local_shell_call`, `local_shell_call_output`, `code_interpreter_call`, `image_generation_call`, `mcp_call`, `mcp_list_tools`, `mcp_approval_request`, `mcp_approval_response`) are dropped with `items_dropped:<type>:<n>`. In a code, a type of 1-32 characters from `[a-z0-9_]` is kept; any other type is named `other`.
7. `tool_choice`, `parallel_tool_calls`, `max_output_tokens`, `temperature`, `top_p`, `reasoning.effort` map by the §7.6 model facts. `max_tokens` is the client's `max_output_tokens`, else the catalog's `facts.max_output_tokens`; with neither (no facts, or 0) → 400 that names `max_output_tokens`. With no tool left to send, `tool_choice` and `parallel_tool_calls` are omitted, and a `tool_choice` of `required` → 400. A `tool_choice` `{type: function|custom, name}` names the sent tool of that name; a name that no sent tool has (a nested tool's bare name too) → 400. `{type: function, name, namespace}` names the sent nested tool (namespace, name) by its flat name; when no sent nested tool is that pair (the flat name is a plain or `custom` tool's, or the pair is only in `input`) → 400. A `namespace` that is not a string or `null` → 400. `{type: custom, name, namespace}` with a string `namespace` → 400 (a `namespace` tool holds only function tools). A mapped field of the wrong JSON type (`stream`, `parallel_tool_calls`: boolean; `temperature`, `top_p`: number; `reasoning`, `text`, `text.format`: object; `reasoning.effort`: string) → 400 that names it; `null` is absent.
8. The top-level field lists above apply.
9. An `input` item of an unknown `type`, or a known item with a missing required field → `400 zygo_bad_request` that names the type. It is never dropped in silence. An item with no `type` and a `role` is a `message`; a `type` that is not a string → 400. A 400 that echoes a client field or type name cuts it to 64 characters and replaces each control, format, line separator or paragraph separator character (Unicode Cc, Cf, Zl, Zp) with `?`.
10. Ignored fields never reach an Anthropic or chat upstream.
11. `text.format` of type `json_schema` becomes the Messages output format `output_config.format` = `{type: json_schema, schema}`, beside `output_config.effort` when that is sent. Codex 0.172.0 sends one on its thread-title request (`name` `codex_output_schema`, a `title` string of 1-36 characters; PC-5: `fixtures/codex/compaction/requests/pc8-tui-high-0028.json`, `pc8-tui-low-0025.json`). The §7.6 model facts and strict-schema rules then apply to it as to a Messages client's output schema: `structured_output = none` puts the schema in the system text (`structured_output_prompted`), and a `refuse` rule rewrites the schema (toward Anthropic, `string_length` moves `minLength`/`maxLength` into the `description`). Toward a chat upstream, the Messages → chat translator sends `response_format` = `{type: json_schema, json_schema: {name: "output", schema}}`. `schema` must be an object (absent or `null` → 400 `text.format: missing schema`). `name` and `description` (strings) and `strict` (boolean) are checked and not sent: Messages has no counterpart for them. Toward a chat upstream, `response_format` carries no `strict` key (`crates/zygo-proxy/src/translate/chat.rs:114`), so the upstream's default applies. A `description`, and a `strict` that is not `null`, are recorded as `text_format_dropped:description` and `text_format_dropped:strict`. Another key of a `json_schema` format, or a key other than `type` of a `text` format → 400 that names it. For either type, a key whose value is `null` counts as absent. The answer comes back as a `text` block and goes to the client as an `output_text` part (stream obligation 2; obligation 8 for a JSON body), with the §7.6 injected nulls removed. No tool is added, so client tools, `tool_choice` and the tool table of stream obligation 2 stay as they are. A forced tool is not used: the output-format path reuses the §7.6 facts and strict-schema rules that already exist for a Messages client's output schema; a forced tool's `tool_use` answer would have to be mapped back to `output_text`; and `crates/zygo-config/data/catalog.toml` sets `forced_tool_choice = false` for Claude Fable 5.2, Claude Opus 5.5 and Claude Sonnet 5.5, so the §7.6 facts layer (`crates/zygo-proxy/src/translate/facts.rs:48-57`) would widen a forced `tool_choice` `tool` to `any` **(author, RS-11)**.

Stream obligations (Messages SSE → Responses SSE):

1. The stream starts with `response.created` then `response.in_progress`, with a proxy-made id (`resp_<ULID>`), `model` = the name the client sent, `status = "in_progress"`.
2. Each content block becomes one output item: text → a `message` item with an `output_text` part (`output_item.added`, `content_part.added`, `output_text.delta`…, `output_text.done`, `content_part.done`, `output_item.done`); `tool_use` → a `function_call` item (`output_item.added`, `function_call_arguments.delta`…, `function_call_arguments.done`, `output_item.done` with the whole `arguments` string); a `tool_use` whose name is the flat name of a nested tool (from `tools`, or from a namespaced `function_call` item of `input`) → a `function_call` item with the nested `name` and the `namespace`, as PC-5 records (`fixtures/codex/mcp/requests/pc9-call-0033.json`); the proxy finds the pair in a table built from the request, never by splitting the name; a `tool_use` of a tool the client sent as `custom` (a `custom` entry of `tools`, or the name of a `custom_tool_call` item of `input`, which request obligation 6 declares from the history) → a `custom_tool_call` item (`output_item.added`, `custom_tool_call_input.delta`, `custom_tool_call_input.done`, `output_item.done` with `input` = the tool input's `input` string, else its JSON text); Codex matches the item kind to the tool kind (PC-3); `thinking` → a `reasoning` item with a summary and a zygo-marked `encrypted_content` that holds the thinking text and signature; `redacted_thinking` → a `reasoning` item with no summary and a zygo-marked `encrypted_content` that holds the redacted data **(lead)**. A mark longer than `MARK_MAX` (K§6.7) would not decode: the item is sent with no `encrypted_content`, with adjustment code `reasoning_unmarked:<n>`.
3. `server_tool_use` and `*_tool_result` blocks: the request translation removes server tools (§7.6), so an upstream should not send them; if one comes, it is dropped from the stream with the adjustment code `server_tool_dropped` **(lead)**.
4. `sequence_number` rises by one per event from 0. `output_index` and item ids are consistent across an item's events.
5. `message_delta` and `message_stop` become one terminal event: `response.completed` with `usage` (`input_tokens` = input + cache read + cache write; `input_tokens_details.cached_tokens` = cache read; `output_tokens`; `total_tokens`), or `response.incomplete` with reason `max_output_tokens` for `stop_reason` `max_tokens` or `model_context_window_exceeded`; every other stop reason (`end_turn`, `tool_use`, `stop_sequence`, `pause_turn`, whose server tools obligation 3 drops, and a text-only `refusal`) gives `response.completed`. `refusal` has two endings, by whether the response holds a call item:
   - **Text-only refusal** (no `function_call` or `custom_tool_call` item in the response) → ending `rfc`: after the done events of an open block, one more `message` item carries the text `The upstream model refused this request (stop_reason: refusal).` as one `output_text` part, with its `output_item.added`, `content_part.added`, `response.output_text.delta`, `response.output_text.done`, `content_part.done` and `output_item.done` events, then `response.completed`; a non-streamed body ends its `output` with the same item and has `status` `completed`. Codex 0.172.0 shows that text once and does not send the request again (PC19, measured); `response.incomplete` with `content_filter` made a default-configured Codex send it 6 times **(author, RS-12)**.
   - **Refusal with a call item** (a `function_call` or `custom_tool_call` item, closed or still open at the refusal) → ending `rfd`: a call item still open at the refusal (its arguments may be cut) gets no done events; it is dropped as on every other failure path (PC18 `xt` measured no tool run). Another open block gets its done events first. Calls closed before the refusal stay as sent: their done events already reached the client. No refusal text item; then `response.failed` with `usage` and error code `invalid_prompt`, message the same refusal text. A non-streamed body has `status` `failed` and that `error`, its `output` the closed calls and other items without the refusal text item; it holds no open or cut call (a Messages body holds whole blocks only). A `response.completed` here would let Codex run a call whose arguments may be cut. This applies on every translated upstream (Anthropic, chat and a JSON body); a same-dialect Responses upstream passes through (obligation 6). On chat, `finish_reason` `content_filter` maps to `refusal` (`translate/mod.rs` `chat_stop`); the mapper and the chat body translation (`chat::response`) drop every held tool call (their arguments may be cut), send no event or block of them, and put their count in the key `zygo_calls_dropped` of `message_delta` (of the Messages body for a JSON body; `message_events` moves it to `message_delta`). The encoder and `response()` count such a drop as a call item, so the ending is `rfd` with no call item sent, and record adjustment `refusal_call_dropped:<n>`. A Messages client gets no `tool_use` block for those calls **(lead, RS-12 review rounds 1 F1, 2 G1 and 3 H1, H2)** and never gets the key `zygo_calls_dropped`, which only the encoder reads: for a Messages client the forward step (`forward.rs`) builds the mapper with the key off and removes the key from a translated JSON body, and it records the count on the usage row as adjustment `refusal_call_dropped:<n>`, as the Responses path does. A Responses upstream reaches the mapper (its Responses arm, `translate/sse.rs`) only for a Messages client, and the same rule holds there. A `function_call` item is held as one call: its call id and name once, and its argument deltas appended into one string (an empty or missing delta adds nothing). The hold starts at `response.output_item.added` and ends at the first of: its `response.output_item.done` with `status` `completed`; the start of another output item (OpenAI emits items one at a time); the end of the response. A done with another or no `status` does not end the hold; its `arguments` are kept when no delta came (`responses::response` keeps only a `completed` call). When the hold ends at a done or at another item, the call is sent whole: `content_block_start`, its arguments in `input_json_delta` pieces of at most 64 KiB (obligation 8), and `content_block_stop`. A later done or argument delta that carries the `output_index` of a call already sent is ignored; an event with no `output_index` is not checked against the calls already sent. At `response.incomplete` with reason `content_filter` (stop reason `refusal`), a call still held is dropped, not sent, and counted the same way. At any other end, a held call is sent and closed. An error event, or an upstream stream that ends early, ends the stream with the held call not sent. Accepted limit (DDR-01M8CE0JMGSVYFYKASBVRHY5VX): a call whose done was not `completed` is sent when another output item starts, so a later `content_filter` cannot drop it, while the JSON body (below) drops it; for this sequence the stream and JSON paths differ. Only a non-conforming upstream sends it: OpenAI sends `status` `completed` on each `function_call` done and no item after a cut. To hold such a call past later items would reorder the block indexes that the Messages client gets **(author's call, RS-16; review rounds 1 F1, F3, 2 G1, G2 and 3 H1, H2)**. A non-streamed Responses body (the Responses body translation, `responses::response`, beside `chat::response`) at `incomplete` with reason `content_filter` drops each `function_call` item whose `status` is not `completed` and counts it the same way; a `completed` call is kept, as the stream keeps a closed call **(author's call, RS-16 review round 1 F2)**. On Anthropic, calls closed before the refusal are sent as received: a `tool_use` block that gets `content_block_stop` before `message_delta` `refusal` is closed and sent before the stop reason is known. Whether Anthropic sends a cut `tool_use` with `content_block_stop` before a refusal is not known (DDR-01M8CAJ3FPS180544MPTP1V0EC) **(lead, RS-12 review round 3 H3)**. Inferred, not measured: run 2, the committed `fixtures/codex/encoder/result.txt` (line `rfd`), measured `rfd` text-only, as 1 upstream request with the next turn ok, the same results as run 1 (83e04d6c); that Codex runs no tool from a failed response is not measured for a refusal.

   The terminal event's `response.output` is empty: each item reached the client in its `output_item.done`.
6. An upstream error **before the first byte** to the client keeps its HTTP status, with an OpenAI error body (K§6.4.4). An upstream error **after the stream started** gives `response.failed` with an error `code` and `message` **(lead)**. An upstream error's `message` has the provider key replaced with `[REDACTED]`, then is cut and its control, format and separator characters replaced as a 400 echo (request obligation 9); an over-long prompt keeps the K§6.4.4 rule instead. A read error on the upstream body gives `response.failed` with code `api_error` and message `upstream response failed`; before the first whole upstream event, the stream starts first (`response.created`, `response.in_progress`). Each stream ends with exactly one terminal event, and the client body ends at that event; the client gets nothing more. On the Anthropic route the rest of the upstream body is then read in a detached task, through the tee, until it ends, a read error occurs or the per-read idle timeout passes. On the chat route a terminal or error event of the mapper sets the stream done, and the upstream is read no further. The usage row is written once, when the stream ends or the detached task ends, with the counts read up to then **(lead, RS-4 review rounds 1 and 3)**. Measured against Codex 0.172.0 with its default retries (no retry keys, as a zygo launch; PC18 and PC19, per `fixtures/codex/encoder/result.txt`): a turn that ends in `response.failed`, after an upstream error or after a half-open `function_call`, gets 6 upstream requests (1 + 5 retries); `response.incomplete` `max_output_tokens` gets 1; `response.incomplete` `content_filter` gets 6 (case `rfa`, recorded), so the encoder does not send it (obligation 5); `response.failed` `invalid_prompt` gets 1 (case `rfd`, recorded). Each retry is a new upstream request. Scope: this holds for the encoder, that is for Anthropic and chat upstreams and JSON bodies. A same-dialect `openai` upstream (Responses → Responses, no encoder) passes its own `response.incomplete` `content_filter` through unchanged, and Codex then retries as it does against OpenAI directly; zygo does not rewrite a passthrough ending. This is recorded here as accepted behaviour, not a defect, and needs no node **(lead, RS-12 review round 1 F3)**.
7. `ping` is dropped. An upstream event of unknown type is dropped. An upstream event whose JSON does not parse ends a started stream with `response.failed`.
8. A non-streaming request gets one Response object with the same content, or the upstream status with an OpenAI error body. A streaming request whose upstream answers 2xx with a JSON body (not SSE) also gets the same content. The body, read whole within `COPY_MAX` (`crates/zygo-proxy/src/tee.rs`, equal to `HOLD_MAX`), becomes Messages SSE events (`message_events`, `crates/zygo-proxy/src/translate/sse.rs`) that pass the tee and the encoder as an upstream stream does. A `tool_use` block becomes `content_block_start` with `input: {}`, then its input serialized once as compact JSON, in `input_json_delta` events of at most 64 KiB of `partial_json` each, cut on a character boundary, then `content_block_stop`. So each event line stays under about 128 KiB plus framing (a second JSON escape at most doubles a piece), and the encoder holds as arguments only the compact input, which is shorter than the body that held it (numbers keep their written form, `arbitrary_precision`; a chat upstream's `arguments` string is parsed first). The encoder bounds a tool call's arguments alone: the held arguments plus the decoded `partial_json` piece stay within `HOLD_MAX`. The raw event line that carries a piece is bounded alone, by the line and event bound of obligation 9, and is not added to the held arguments. Such a body never ends the stream with an obligation 9 `response.failed`. The same 64 KiB pieces (`INPUT_DELTA_MAX`, one cutting function) carry a chat upstream's held tool-call `arguments` out of the chat mapper (`StreamMapper`, `translate/sse.rs`), and the arguments that the null-restore filter (`RestoreFilter`, `translate/schema.rs`) rebuilds after it removes an injected null. Text goes out the same way: the same function cuts it into `text_delta` pieces of at most 64 KiB of `text`, on a character boundary, for a text block of `message_events`, each text delta of the chat mapper and a text block that the null-restore filter rebuilds. `message_events` sends a thinking block's text in `thinking_delta` pieces the same way. A text of at most 64 KiB stays one delta; an empty text or thinking block of `message_events` keeps its one empty delta; the concatenated text of each block is unchanged. Each sender's longest event is below, before its framing (the `event:` line, the JSON envelope and the index, at most about 150 bytes). serde_json writes the shortest escape, so a string decoded and written again is never longer than its written form, and compact JSON holds no raw control character, so its second escape at most doubles it. Decoded text can hold a control character, which escapes as `\u00XX` (6 bytes) unless it is `\b`, `\f`, `\n`, `\r` or `\t`, so a piece of decoded text escapes to at most 6 × its bytes:
   - `message_events`: an `input_json_delta` at most 2 × 64 KiB (compact input); a `text_delta` or `thinking_delta` at most 6 × 64 KiB = 384 KiB (decoded text); a `signature_delta`, or a block sent whole in its start, at most that signature or block as written in the body, within `COPY_MAX`.
   - The chat mapper: an `input_json_delta` (`calls_out`, and `close` for a held Responses `function_call`, `translate/sse.rs`) cuts the raw upstream `arguments`, not compact JSON, and a control character in them escapes again as `\u00XX`, so at most 6 × 64 KiB = 384 KiB; a `text_delta` cuts the decoded `content` of one upstream chunk, so at most 6 × 64 KiB = 384 KiB too.
   - The null-restore filter: a rebuilt `input_json_delta` or `text_delta` at most 2 × 64 KiB (compact JSON), on raw bytes and on events.
   - The encoder checks each line alone against `HOLD_MAX` (obligation 9), and an event over it ends the stream with `upstream stream line too long`. No delta piece above comes near it. Only an event that `message_events` sends whole can: a `signature_delta`, or a block sent whole in its start, of a JSON body near `COPY_MAX`.
9. The §10.2.6 bounds hold: no buffer grows with the response. Each buffer below has a fixed bound (`HOLD_MAX`, `crates/zygo-proxy/src/tee.rs:22`; `COPY_MAX`, equal to it). Each bound holds between two upstream reads; inside one read a buffer can pass it by up to one chunk, because the chat mapper (`StreamMapper::push` and its tool-call count, `translate/sse.rs`) and the restore filter's raw path (`RestoreFilter::push_raw`, `translate/schema.rs`) check after the chunk is appended. The peak per stream is the sum of the bounds of the buffers on its path, in bytes held between two upstream reads; the output of one read is passed on and freed in the same step.
   - The tee's partial line, in line mode only (an SSE upstream): at most `HOLD_MAX`. It keeps its capacity; a longer line passes through unheld.
   - The error-event buffer before the encoder (`Streamed.held`, `crates/zygo-proxy/src/forward.rs`), in line mode only: the `data` lines of an upstream `error` event split over lines, to its blank line, at most `HOLD_MAX`. Over it the stream fails with `upstream event too long`.
   - The encoder's held bytes: its partial line, the current event's `data` lines and the open item's text, thinking or arguments. While a text, thinking or redacted block (or no block) is open, their sum stays within `HOLD_MAX`. While a tool call is open, its arguments stay within `HOLD_MAX` alone (only the kept bytes count: a call's first delta replaces the start's `input`), and the partial line plus the `data` lines stay within `HOLD_MAX` alone, so the encoder holds at most 2 × `HOLD_MAX`. Over a bound the stream ends with `response.failed`, whose message names the largest held part (`upstream stream line too long`, `upstream event too long`, `upstream content block too long`, `upstream tool call too long`).
   - The chat mapper's held tool calls (64 + id + name + arguments bytes each, until the choice finishes) plus its partial line and `data` lines: at most `HOLD_MAX` (`StreamMapper.held`, `translate/sse.rs`). Over it: an `upstream tool call too long` or `upstream stream line too long` error event. The Responses arm holds one `function_call` the same way (64 + call id + name + the bytes of its joined arguments, until its `response.output_item.done` or until another output item starts; obligation 5) within the same bound, and a held block over it ends the stream with the same `upstream tool call too long` error event **(author's call, RS-16)**. A `response.output_item.done` whose `status` is not `completed` does not end the hold. The Responses arm also keeps `sent` (`StreamMapper.sent`), the set of the `output_index` of each call already sent: one entry per call sent, bounded by the calls in one response, not by `HOLD_MAX` **(author's call, RS-16 review round 3 H3)**.
   - The null-restore filter's held block (`RestoreFilter`, `Hold`, `translate/schema.rs`), only when injected nulls are armed (§7.6). On raw bytes (an Anthropic upstream), the block's held event bytes (`bytes`), the current event's lines (`event`) and the partial line stay within `HOLD_MAX` together. Three more buffers copy parts of those: the decoded text (`text`) and the held non-delta events (`others`) copy parts of `bytes`, and the current event's `data` lines (`data`, `translate/schema.rs:534`, filled at `:714-721`) copy part of `event`. The copies are of disjoint parts of the first sum, so together they are at most `HOLD_MAX` too, and the filter holds at most 2 × `HOLD_MAX`. On events (after the chat mapper), the decoded text and held pings stay within `HOLD_MAX`; the held events carry the same text again, so about 2 × `HOLD_MAX`. Over the bound the held bytes pass on unchanged, and the rest of the block passes.
   - The peak per stream, by path:
     - Anthropic SSE upstream: the tee (1), `Streamed.held` (1) and the encoder (2): about 4 × `HOLD_MAX`; with the restore filter (2), about 6 × `HOLD_MAX`.
     - Chat SSE upstream: the chat mapper (1) and the encoder (1). The mapper sends whole events only (obligation 8: a text or tool-argument event at most 384 KiB plus framing), so the tee, `Streamed.held` and the encoder's partial line hold nothing between reads, and its `data` holds at most one event during a read. About 2 × `HOLD_MAX`; with the restore filter (2), about 4 × `HOLD_MAX`.
     - JSON body (obligation 8): the body is read whole within `COPY_MAX` and translated before the first byte. `message_events` builds all its Messages SSE events at once, and they go through the tee (not in line mode: it holds nothing, and `Streamed.held` is not used) to the encoder as one chunk, which is freed when it is sent. The event bytes are the body's block content (a tool input at most 2 ×, after its second escape; any other string at most as written) plus a fixed overhead per block times the number of blocks, plus the `message_start`, `message_delta` and `message_stop` events. The overhead per block, from `message_events` and `SseEvent::to_bytes` (`translate/sse.rs`), with d the digits of the block's index: 163 + 2d bytes for a block sent whole in its start (a block of another type, such as `{}`); 302 + 3d for text, and 114 + d more for each further 64 KiB piece; 333 + 3d for thinking, 122 + d more for each further 64 KiB piece, and 124 + d more with a signature; 337 + 3d for `tool_use`, and 128 + d more for each further 64 KiB piece. The worst case per `COPY_MAX` body is 1,398,101 `{}` blocks: 248,037,859 event bytes, about 59 × `COPY_MAX`; empty text blocks give about 12 ×, and empty `tool_use` blocks about 7 ×. This is an accepted limit (DDR-01M8BACPSY0WNJ0DK83YN82PAC). The encoder holds the open item within `HOLD_MAX` and one event's `data`. The peak is the body (`COPY_MAX`), the event chunk and the encoder's `HOLD_MAX`.

#### 6.4.4 Errors before the stream

An upstream error before the first byte to the client keeps its HTTP status and gets the OpenAI error body (`errors.rs:34-41`) **(lead)**. An over-long prompt reaches a Responses client in the shape that PC14 records, so Codex runs its own recovery. Over-long means a refusal that §7.6 maps to `prompt is too long`, Anthropic's own `prompt is too long: …` text, or Anthropic's full `` input length and `max_tokens` exceed context limit: <a> + <b> > <c> `` form (three integers) **(lead)**. Each of these forms (the `prompt is too long` form and the `max_tokens` limit form) gets the PC14 shape for the Responses client only, with the upstream message cleaned: each control, format, line separator or paragraph separator character (Unicode Cc, Cf, Zl, Zp) replaced with `?`, and at most 512 characters, so the numbers survive; no token count is made up. The same cleaning and cap apply to both forms before the stream (the non-streamed or pre-stream error body) and inside a started stream (`response.failed`). A message that only contains the phrase is an ordinary upstream error. Every other client receives the `max_tokens` limit form unchanged, before and inside the stream: Claude Code parses it to retry with a smaller `max_tokens` (K§12). `response.failed` is used only after the stream started (K§6.4.3 stream obligation 6). Inside a started stream the same forms get the PC14 code too: an upstream `error` event of type `invalid_request_error` whose message is an over-long form above (an Anthropic event, or the chat mapping of §7.6) gives `response.failed` with code `context_length_exceeded` and the message by the rule above, not cut as an echo, so a default Codex runs its recovery and does not retry the stream **(lead, RS-4 review round 1)**.

The provider key never reaches the client, on every client and upstream pair, translated or not **(author's call, RS-5 review F5)**. The provider key is the key `zygo` sends for the candidate provider (`Candidate.key`); a request that forwards the client's own credential (§7.3 passthrough) has none, and nothing is replaced. When a key is set, these rules apply to the upstream response (`run` in `crates/zygo-proxy/src/forward.rs`). The key they look for is the configured value with ASCII whitespace removed from both ends; that trimmed key is a part of the untrimmed one, so an echo of either form is found. (1) **Headers, every path.** Each upstream response header whose value holds the key, or whose name holds it in lower case (header names arrive in lower case), is dropped, translated or not, 2xx or not; the response then has the adjustment code `key_header_dropped`. (2) **Encoded bodies, every path.** An upstream response with a `content-encoding` other than `identity` is refused, as the key cannot be found in encoded bytes: a 2xx with `502` `zygo_upstream_encoded`; a non-2xx keeps the upstream status, with the zygo error body of type `zygo_upstream_encoded` (`errors.rs`) in place of the encoded body, and the upstream `retry-after` header when it has one (a 429 or 5xx). `zygo` never forwards `accept-encoding` (§7.3), so a provider encodes a body only on its own initiative. (3) **Translated non-2xx bodies.** `translate_error` replaces the key in the error it builds, before the stream; stream obligation 6 does so inside a started stream. (4) **Bodies.** Each occurrence of the key in the body bytes is replaced with `[REDACTED]`, the text that `scrub` (`crates/zygo-proxy/src/translate/mod.rs`) puts in its place. The replacement does not read lines or events: it works whatever the framing, line endings or event names, SSE or JSON. Every other byte passes unchanged, also bytes that are not UTF-8. *Every 2xx body, translated or not, but the one below:* the key is replaced in the upstream bytes as they are read (`KeyScrub`, `forward.rs`), before any other step reads them: the translator, the mapper, the restore filter, the Responses encoder, the tee and the line stages. So the exact key bytes never reach those steps. A step that decodes or joins text can still make the key again from bytes that do not hold it: the two residual forms of (5). *The untranslated 2xx JSON body with injected nulls* (§7.6) is the one body done in the other order: it is read whole within `COPY_MAX`, its nulls are removed (which parses the JSON, so a `\u` escape is decoded, and serializes it again with no escape of `[A-Za-z0-9_-]`), and then the key is replaced in the serialized bytes, so on this path an escaped key is found too; it is sent without `content-length`. *A translated 2xx body read whole* (a JSON upstream body) is read within `COPY_MAX` counted after the replacement; over it, `502` `zygo_upstream_too_large`. *An untranslated non-2xx body* is read whole within `COPY_MAX` (`crates/zygo-proxy/src/tee.rs`), counted on the upstream bytes; over it, `502` `zygo_upstream_too_large`, as on a translated body; the key is then replaced; its status and `content-type` stay, and its `content-length` is that of the new body. *An untranslated 2xx body that is not SSE* (and has no injected nulls) is read whole within `COPY_MAX`, counted after the replacement, and sent with its new `content-length`; a longer one goes on as a stream. Every other 2xx body (SSE, translated, or the rest of a longer body) is sent without `content-length`. The streamed replacement holds back, of the last `key length − 1` bytes read, the longest tail that is a start of the key, where a key split across reads can start; it sends that tail with the next read, or at the body end. Its memory is the key length; no line is held or cut, and an event end is not delayed. A read error ends the body without the held bytes. *Memory:* a path that reads a body whole holds about 2 × `COPY_MAX` per request: the body read and the body made from it (key replaced, nulls removed or translated); on the restore and translated paths the parsed JSON value comes in addition. (5) **Limits.** The guarantee holds for keys in the configured providers' alphabet `[A-Za-z0-9_-]` (Anthropic `sk-ant-…`, OpenAI `sk-…`) that are at least 20 bytes long. *Residual forms (accepted limit, DDR-01M8BHQNT1DP5RKYVJBCZ01HDF):* the replacement finds only the key's exact bytes. Two forms that do not hold those bytes become the key in a later step and so reach the client: (a) *an escaped echo*: the key written with a JSON `\u` escape in a 2xx body, which a translator (`translate_response`, the stream mapper, the Responses encoder) or the streamed restore filter (`RestoreFilter`, `crates/zygo-proxy/src/translate/schema.rs`, which parses a held block's text) decodes and writes again in its exact bytes; (b) *a key split across stream deltas*: each delta holds a part of the key, with event bytes between the parts, and the Responses encoder joins the deltas in its `*.done` events (`response.output_text.done`, `response.function_call_arguments.done`; `translate/responses_client.rs:1378`, `:1404`), and the restore filter joins a held block's deltas into new deltas. Only an upstream echo or the model's own output can make either form. JSON and URL encoders do not escape the characters `[A-Za-z0-9_-]`, so a real encoder writes such a key in its exact bytes. A key with other characters is not covered in its escaped forms (a JSON `\u` escape, URL percent-encoding). *Short keys:* each replacement writes the 10 bytes of `[REDACTED]`, so a key shorter than 10 bytes makes a body longer, up to 10 × for a one-byte key, and a key that overlaps `[REDACTED]` can match again in the replaced text. `zygo` does not refuse such keys (author's call, RS-14 round 2 review G5).

#### 6.4.5 Window rewrite for a Responses client

When client policy arms the rewrite, the tee works in whole SSE lines and the window stage rewrites the usage the tee parses (K§6.4.2), in the form that `ResponseCtx.lines` names. Until P4-6 moves arming to the descriptor, the rule is the §10.2.2 common conditions with the route `/openai/v1/responses` added: a Claude Code session is armed, an OpenCode session is not, a compaction request (`x-claude-code-compaction`) is not, and the served model needs a context window. The Codex rule is K§6.3.4, in P4-9.

- **Messages form** (Anthropic and chat upstreams): the `message_delta` usage, by §10.2.2 as on the Messages route. The encoder then carries it into the terminal event: `response.completed`, `response.incomplete` for a `max_tokens` stop, or `response.failed` for a `refusal` after a call item (stream obligation 5). On this path the forward step reads each line first for an upstream error (K§6.4.4) and then gives it to the line stages, but only when the window stage armed line mode at request time.
- **Responses form** (a Responses upstream): the `response.usage` of a `response.completed` or `response.incomplete` line, by §10.2.2 on the §14 translated-path counts (input = `input_tokens` − `input_tokens_details.cached_tokens`, cache read = `cached_tokens`, cache write 0). The three numbers are `input_tokens_details.cached_tokens := max(window − input − cache_write, cached)`, `input_tokens := input + cache_write + cached_tokens` and `total_tokens := input_tokens + output_tokens`. So the client gets the same usage that the encoder gives on the Messages path (`responses_client.rs` `response_usage`). A field that the object does not have is inserted, and `input_tokens_details` is inserted as an object that holds `cached_tokens`. The line stays unchanged when an edited object repeats a key, when `usage` or `input_tokens_details` is not an object, or when a present `input_tokens`, `input_tokens_details.cached_tokens` or `output_tokens` is not a u64 JSON integer (for example `3e1`): the tee reads 0 for such a number, so the edited numbers would be wrong. `output_tokens` and every other byte stay as received. A `response.failed` line is not rewritten. A terminal line over `HOLD_MAX` passes unchanged (§10.2.6).

Trigger 0 (safety) acts on each terminal event. Trigger 2 (the `compact` tool) matches the PC9 name in a tool call. In the Messages form, that is a `tool_use` `content_block_start`. In the Responses form, it is a `response.output_item.added` `function_call` item: its `name`, joined to its `namespace` by `flat_name` (`<namespace>__<name>`, K§6.4.3) when the item has a namespace, is the §10.2.3 compact tool name. Trigger 1 (soft window at end of turn) reads the `x-claude-code-request-class: main` header, as on the Messages route. Its end of turn is `stop_reason = "end_turn"` in the Messages form. In the Responses form, it is a `response.completed` when no call item came in the stream of that response, neither in a `response.output_item.added` or `response.output_item.done` event nor in the terminal `response.output`. A call item is an item of type `function_call`, `custom_tool_call`, `local_shell_call`, `computer_call`, `mcp_call` or `mcp_approval_request`. The streamed events count because a terminal event can carry an empty or no `output` (PC17). For Codex, trigger 1 needs a main-loop marker: it is armed only when PC6 finds a request marker for subagent calls; else A-16.1 relies on the launch-time limit. Codes and the usage row follow §10.2.2: each trigger's code is recorded once per response, and the row keeps the upstream counts.

### 6.5 Update notices (P4-10)

#### 6.5.1 What exists

The start check of U runs before a launch in `zygo claude`, `zygo opencode` and `zygo ws run` (`crates/zygo/src/cmd/update.rs:294-371`). It is skipped in a zygo session, in a workspace pane, off a TTY, with `-p`, with `[update] check = false` or `ZYGO_NO_UPDATE_CHECK`, and within 24 h of the last check (`update.rs:303-322`). It keeps its state in `<cache_dir>/update.json` (`checked_at`, `latest`, `skip`; `update.rs:231-275`). `[update]` is user config only (`crates/zygo-config/src/layers.rs:68`). Nothing checks during a session, so a session that runs for days never learns of a release, and a member session never checks at all.

#### 6.5.2 Background check of zygo's own releases **(operator: background check, HUD post; author: the mechanics)**

- **Where.** One thread in the launcher process. That process lives for the whole session and hosts the in-process proxy, so no new process starts and W-G6 holds. The thread starts once per launcher process, at the one point where `zygo <client>` and `zygo ws run` both pass before `Launcher::run` (the builder picks that one call site). It ends with the process.
- **When.** First tick 60 s after the child starts, then one tick per hour. A tick checks only when the shared throttle allows it (24 h since `checked_at`, the U rule, a clock that went back does not count). So a session that runs for days checks about once a day, and each live session shares one check.
- **Skips.** `[update] check = false` and a non-empty `ZYGO_NO_UPDATE_CHECK`, read at launch. The U skips for a zygo session, a workspace pane, a non-TTY and `-p` do not apply: they exist because the start check prompts, and the background check never prompts. So member sessions check too.
- **The check.** The U check: one GET for the release's public VERSION file, 2 s limit, no system proxy (`update.rs:219-229`). The check function moves out of the binary crate so the launcher can call it (K§9).
- **Result.** The thread writes `checked_at` and, on success, `latest` through the state owner (K§6.5.3). It never prompts and never installs. A failed check writes `checked_at` (as U does) and one session-log line `update check: failed: <reason>`; it shows nothing on the HUD.
- **Start check unchanged.** A later `zygo claude` within 24 h of a background check does not prompt, as U says today. The HUD and the exit line carry the notice in that window.

#### 6.5.3 The one owner of the throttle state **(author; lead ruling I12)**

A module `zygo_core::update_state` owns two files and one lock. The start check, the background check and each reader use only this module.

- `<cache_dir>/update.json`: the U file, with its U fields (`checked_at`, `latest`, `skip`). No new field goes in it, because an older binary rewrites it with the U fields only (`update.rs:251-275`).
- `<cache_dir>/update-clients.json`: the client results (`<client-id>.checked_at`, `<client-id>.latest`). Only the new module writes it, so an older binary cannot erase it.
- `<cache_dir>/update.lock`: a blocking lock (`fs4`, already a `zygo-core` dependency) with a 500 ms timeout. A writer that does not get it in time skips that write. The lock is never held during a GET or a prompt. A writer reads, does the GET (or asks), then takes the lock, reads the file again, changes only its own keys, keeps each key it does not know, writes (the U temporary file and rename form) and lets the lock go.
- Reads are bounded to 4 KiB; a missing, large or unparsable file is an empty state.

#### 6.5.4 New releases of the wrapped clients **(operator: the flag and its default; author: the rest)**

- **Fact.** Each descriptor declares `announces_own_updates`: under a zygo launch, the client shows its own update notice by default. It is a measured premise with a fixture, per client, because a zygo setting could hide the notice. At `bf3726bd` zygo sets no update option for any client (no `autoupdate`, `DISABLE_AUTOUPDATER` or `check_for_update_on_startup` in `crates/zygo-launch/src`; grep). Expected values, not yet measured: Claude Code `true`; OpenCode `true` (its `autoupdate` defaults on); Codex `true` (`check_for_update_on_startup`, "Defaults to `true`", pinned schema; K§6.3.1 keeps it unset). If zygo's launch hides a client's notice, that client's fact is `false`.
- **Flag.** zygo's per-client flag `announce_client_updates` defaults to NOT `announces_own_updates`, so zygo fills a gap and never doubles a notice by default **(author's reading of the operator)**. A user who turns off the client's own notice can turn on zygo's flag for that client. The user config overrides it per client: `[update.clients.<client-id>] announce = true | false` (user config only, under the `[update]` table that `layers.rs:68` already drops from team and repository layers) **(author)**.
- **Release source.** A descriptor may declare one release source: one HTTPS URL and the JSON field or text rule that gives the latest version. Proposed, each verified by its client's premise before use: Claude Code `https://registry.npmjs.org/@anthropic-ai/claude-code/latest` (field `version`); OpenCode `https://registry.npmjs.org/opencode-ai/latest` (field `version`); Codex `https://registry.npmjs.org/@openai/codex/latest` (field `version`). Measured for Codex (PC16, P4-3): its own update check at a TUI start is inferred as `api.github.com:443` from a controlled count difference (one extra `CONNECT` with `check_for_update_on_startup` at its default; the path is not visible); `registry.npmjs.org` was not seen. P4-11 verifies Codex's release source before use. A flag that is on for a client with no release source is a `zygo config check` error, and no check runs for that client.
- **Check.** When a client's flag is on, the K§6.5.2 thread also checks that client's release source, with the same tick, the same 24 h throttle (per client, in `update-clients.json`), the same lock, the same opt-outs, the same 2 s limit, no system proxy, no header but a generic `User-Agent`, and a body read bounded to 64 KiB. A version that does not parse as semver is ignored and logged.
- **Notice.** `<client> <new> available (zygo graded <pinned>)`, where `<pinned>` is the descriptor's minimum version (= the grade file's pin, check item 7).
- **Ungraded notice, with no network.** Whatever the flag, when the running client version (the launcher's `agent_version`, recorded in the session view) is newer than the pinned version, zygo can say `<client> <running> ungraded`. This is a local comparison. It shows in `zygo status`, `zygo doctor` and the verbose HUD only; the compact HUD leaves it out, because a pin is a minimum and most users run a newer version, so a permanent compact segment would be noise **(author)**.

#### 6.5.5 Where the notice shows (per client, through the contract)

The status surface is a descriptor fact (concern 1); A-9 and A-22 grade it.

| Surface | zygo update | client update (flag on) | ungraded |
|---|---|---|---|
| HUD (Claude Code `statusLine` → `zygo hook statusline`, `crates/zygo-launch/src/claude_code.rs:534`) | segment 16, compact `update <new>`, verbose `update <new> — run zygo update` | segment 16, `<client> <new> available (zygo graded <pinned>)` | verbose only |
| `zygo status` (each client; the only in-session surface for OpenCode and Codex) | one line | one line | one line |
| Launcher exit line (each client, when stderr is a TTY) | `zygo <new> is available (you have <cur>); run zygo update` | `<client> <new> is available (zygo graded <pinned>)` | none |

- **HUD segment 16** (an addition to H§6.2) **(author; lead ruling M17)**: after segment 15, color `33`, escaped (H-C6). Compact: on the compact line. Verbose: at the end of row 1 (the work and model row). It shows while `update.json` `latest` is newer than the running zygo version and is not the `skip` version, or while a client update is pending (flag on). "Running" is the launcher's version, which the launcher writes into the session view as an additive field; a hook binary replaced by `zygo update` in another terminal still compares against the session that runs. When both a zygo update and a client update are pending, compact shows the zygo update only (`update <new>`), and verbose shows both, zygo first: `update <new> — run zygo update · <client> <new> available (zygo graded <pinned>)`. Under width pressure it is the first segment removed (each H§6.7 drop list gains it first; verbose removes the client part before the zygo part). The hook reads both files through `zygo_core::update_state` (bounded file reads, no network), inside the H-C2 deadline of 3 s, on each call like the trimmed count (H§7.7).

- **Codex.** Codex 0.172.0 shows only built-in status-line items, and its status-line provider is experimental (`codex features list`: `tui_extensions_statusline experimental false`). So a Codex user sees zygo's notice in `zygo status` and in the exit line. The session text is not used: it is model context, and an update notice is for the user (§8.6, C7 spirit) **(author)**.

#### 6.5.6 Base-spec amendment (§3)

§3 "Nothing phones home" gets a wider exception: "… and, for each client whose `announce_client_updates` flag is on, one GET to that client's release source named in its descriptor (Claude Code, OpenCode and Codex: the npm registry `latest` document of their package), sending nothing else; the same opt-outs apply." With the expected default facts (all `true`), every flag is off, and no client check runs until a user turns one on.

### 6.6 Owners of state

| State | Owner | Created | Removed |
|---|---|---|---|
| Capability registry | the registry file; changed by PR; standard or tier change stops for the operator | P4-2 | never |
| Grade file per client | the client's PRs | with the client's `Agent` variant | with the variant |
| `docs/conformance.md`, `docs/conformance.json` | the conformance test (generator) | `ZYGO_WRITE_CONFORMANCE=1` run | never by hand |
| Per-session `CODEX_HOME` | the Codex adapter | at launch, in the session folder | with the session folder, by the §10.1 sweep |
| Codex hook trust records | the Codex adapter, inside `CODEX_HOME` | at launch | with `CODEX_HOME` |
| Shared Codex session store `<state_dir>/codex/sessions/` | Codex writes it; zygo makes the folder and the link | first `zygo codex` launch | never by zygo (the user's conversations, as `~/.codex/sessions`) |
| Codex session id of a member | the member record (existing `agent_session`) | `SessionStart` hook | `ws forget`, `ws forget-pane` |
| `<cache_dir>/update.json` (U fields), `update-clients.json` (client results), `update.lock` | `zygo_core::update_state` (K§6.5.3); writers: the start check and the background check | first check | never (cache files; deleting them only resets the throttle) |
| Running zygo version and client version of a session | the launcher, in the session view (additive fields) | at launch | with the session folder |

### 6.7 Hostile input at the new boundaries

- **Responses requests.** The body limit and the idle bound of `read_body` (`crates/zygo-proxy/src/routes.rs:74-76`) apply. `serde_json` depth limit applies. Unknown top-level fields, unknown item types and missing fields fail closed (K§6.4.3). On the translated paths, a zygo-marked reasoning value from the client is untrusted: it must decode, fit a size bound, and name a provider and model of the session's candidates, else it is dropped with an adjustment code; it never selects a provider. `previous_response_id` is refused. Header values other than identity are attribution only.
- **Upstream streams.** Bounded holds; unknown events dropped; an unparsable event ends a started stream with `response.failed`; an error before the first byte keeps its status (K§6.4.4).
- **Hook input from Codex.** Read under the existing hook bounds. A body that is not a JSON object, an unknown event, or an unknown tool name gives no output (C6). Paths in tool input go through the existing guard rules.
- **Project Codex files.** Read by the §8.4 safe-read rule at preflight. Their content only decides K-D13; zygo never copies them.
- **Grade and registry files.** K§6.2.5 input rules.
- **`update.json`, `update-clients.json`.** Local files that another process may write. Read bounded to 4 KiB; a file that is large, not JSON, or holds a version that is not semver is an empty state. The HUD escapes each value (H-C6).
- **Release sources.** Each answer is untrusted: 2 s limit, body bounded to 64 KiB, the version must parse as semver (the U `VERSION_RULE`, `update.rs:13`), else the answer is ignored and logged. A source answer never selects a download; `zygo update` keeps its own checksum path (U).

### 6.8 Flows walked

1. **`zygo codex` launch.** CLI shim → preview gate (K§6.2.7) → §8.2 steps 1-2 (config, profile, proxy session, session folder, `token` and `url` files) → `adapter_for(Agent::Codex)` → preflight (project files, trust branch) → build (`CODEX_HOME`, `-c` keys, environment) → plugin delivery (skills paths) → run `codex` → exit (session end, resume line).
2. **A streamed request to an Anthropic upstream.** Codex runs `zygo hook token` (key helper), sends `POST <proxy>/openai/v1/responses` with the bearer and `X-Zygo-*` headers → identity → body read → resolution of `model` (a role) → redaction on `input` → candidates → forward: Responses → Messages, cache markers, `POST /v1/messages` → the tee measures usage from the Messages SSE → encoder Messages SSE → Responses SSE (rewrite of `response.completed` when armed) → Codex.
3. **A hook call.** Codex runs `zygo hook <name> --agent codex` with JSON on stdin → `crates/zygo/src/cmd/hook.rs` takes the agent → `HookInput::parse(Agent::Codex, …)` calls the descriptor's input function → the common handler → the descriptor's output function → stdout. Any fault: one stderr line, exit 0 (`crates/zygo-core/src/hook.rs:76-92`, C6).
4. **Resume.** Plain: `zygo codex -- resume <id>` → a new zygo session and `CODEX_HOME` whose `sessions` links to the shared store → Codex finds the rollout. Member: `zygo up` → `ws run` → `adapter_for` → `has_session_arg` false → `resume_args(id)` from the member record → the same path.
5. **Background update notice.** `zygo claude` (or `ws run`) starts the child → the update thread waits 60 s → tick: opt-outs off, 24 h passed by a read of `update.json` → one GET (2 s, no lock held) → lock taken (500 ms bound), `update.json` read again, `checked_at` and `latest` written, lock released → the next `zygo hook statusline` run reads `update.json` and the session view, sees `latest` > running version and not `skip` → segment 16 → at child exit the launcher prints the exit line. A Codex or OpenCode user runs `zygo status` and sees the same line. A failed GET: one session-log line, no segment.
6. **A fourth client, from this document only.** (a) Add the private `Kind` variant and `client/<id>.rs` in `zygo-core`; the exhaustive match in `Agent::spec()` fails to compile until the descriptor exists. (b) Add `client/<id>/` in `zygo-launch`; `adapter_for` fails to compile until the adapter exists. (c) Add `cmd/client/<id>.rs` and the `Cmd` variant. (c2) Add the client's stub behavior to `crates/zygo-launch/tests/stub_agent/` and its stub launch test (A-21.1); the `adapter_for` table test fails until the adapter row exists. (d) `cargo test` fails: no grade file, no fixtures folder, version headers missing (K§6.2.5 items 1, 7, 9). (e) Write premises and fixtures, then the grade file with evidence. (f) Run the generator. (g) Set `supported = true` only when the bar is met. The boundary test fails if any step put a branch outside the client modules.

### 6.9 Order against phase 3 and row order **(operator: row order; lead: the phase 3 facts)**

Phase 3 still runs on `origin/phase-3` and is not in `main`; `phase-4` branched from `main` (`bf3726bd`). `phase-4` takes phase 3 from `origin/phase-3` directly, at a pinned commit, in a merge commit (node CC-0); it takes `main` again in a normal merge when phase 3 reaches `main`, before `phase-4` goes to `main`. The merge base of `phase-4` and `origin/phase-3` is `0c9ee387` (checked 2026-11-27). Phase 3 changes many of the client-branch sites of K§1 (`session_start.rs`, the status-line `render.rs` and `phase.rs`, `opencode-plugin.js`, `cmd/member.rs`, the workspace hooks, the Claude Code launch, the trim hook per client). Outside those sites it changes one file under `crates/zygo-proxy/src` (`serve.rs`: serve checks its config with `check_with(.., RepositoryLoadout::NotRead)`, FX-29) and `tests/serve.rs`; it adds rows and Method lines to `docs/premises.md`, whose output-shaping rows PO1-PO4 (O§17) use two ids that the OC-1 rows of `phase-4` also use (PO1, PO2), so the merge gives the OC-1 rows new ids; and it adds sections to `docs/live-checks.md`. It does not change `scripts/check-premises.sh`, `docs/conformance.md` or `docs/conformance/`.

| Row | Work | Depends | When |
|---|---|---|---|
| P4-1 | One plan for the rows that start now: P4-2, P4-3, P4-4 | approval | now |
| P4-2 | Conformance: registry, rubric, grade files for Claude Code and OpenCode, generator, the three outputs, bar | P4-1 | now, parallel with phase 3 |
| P4-3 | Codex premises and fixtures (K§6.3.7), with the data-driven premise check | P4-1 | now, parallel with phase 3 |
| P4-4 | Proxy Responses client route (K§6.4), tested with an existing client's session | P4-1, P4-3 (captured Codex requests) | now, parallel with phase 3 |
| P4-5 | Plan the client contract | approval | now; its inventory is taken again by P4-6 |
| P4-6 | Build the client contract (K§6.1, K§6.2.7); add capabilities or criteria for the client-facing features of phase 3 (output shaping, mail wake and delivery, the run-time status line) by the union rule **(lead, I13)** | phase 3 taken by `phase-4` at each phase-3 head; phase 3 and phase 4 reach `main` together through the `phase-4` pull request; P4-2, P4-5 | after `phase-4` takes `origin/phase-3` |
| P4-7 | Codex client design document, against the built contract; K§6.3 and the Codex parts of K§6.4 are its requirements | P4-3, P4-4, P4-6 | after P4-6 |
| P4-8 | Plan the Codex client | P4-7 | |
| P4-9 | Build the Codex client: its `Agent` entry, grade file `codex.toml`, Codex client-policy rows in the proxy, the bar decision, release listing when it meets the bar | P4-8 | |
| P4-10 | Background zygo update check, HUD segment 16, `zygo status` and exit lines, `update_state` (K§6.5.2, K§6.5.3, K§6.5.5 for zygo's own releases); A-22 added with A-22.1 graded | phase 3 complete (shared status-line segment order); P4-2 | opportunistic: dispatched in parallel as soon as phase 3 is complete **(operator)** |
| P4-11 | Client update announcements (K§6.5.4): `announces_own_updates` facts and premises, the flag, release sources; A-22.2 and A-22.3 graded | P4-6 (descriptor), P4-10 | after both |

P4-10 is split from P4-11 **(author)** so the opportunistic row needs no client contract: zygo's own update check reads no client fact. PC16 (P4-3) measures Codex's own default with no zygo between; the Codex fact is set in the Codex descriptor in P4-9 and measured under a zygo launch by the P4-11 premise row for Codex.

The inventory of K§1 is evidence of the pattern, not the work list. P4-6 starts by taking the inventory again at its base commit (the grep of K§1 item 2), because phase 3 adds client-specific branches. Each site found is moved behind the contract or named in the P4-6 plan with its concern.

---

## 7. Decisions

| Id | Decision | Mark | Reason |
|---|---|---|---|
| K-D0 | Add Codex support; a clear client model; a maintained feature-matrix conformance score; each client graded on each feature any client supports; a rubric; measured, scored, "found wanting, but sufficient" | operator | the request of 2026-11-25 |
| K-D1 | One client contract in two layers (K§6.1.1); the kind is opaque outside `zygo_core::client`, so the compiler refuses a branch on a specific client; the literal check of K§6.1.4 covers names in strings | lead (compiler first); author (layers, concerns) | the crate direction forces two layers; a compile error cannot be skipped |
| K-D2 | A client = a descriptor module, an adapter folder, a CLI shim, its stub-agent behavior, a fixtures folder, a grade file. §8.8 is superseded by K§6.8 flow 6 | author | the compiler and the tests list each missing piece; §8.8 missed three pieces |
| K-D3 | P4-6 moves Claude Code and OpenCode onto the contract with no behavior change; the proof is K-C7: each node's own diff is checked for removed or renamed tests, added `#[ignore]` and changed literals, and each test logic edit is listed in its node with its spec reason and checked at the review gate | lead (2026-11-25, amended) | the first rule ("tests change only by declared mechanical rewrites") cannot be met: at the scale of P4-6 (hundreds of type-path uses), rewrite rules that compose cannot be ordered, substring patterns collide, import lines are new lines, and K§9 itself requires test logic edits (check item 7 of K§6.2.5 reads the descriptor; `minimum_agent_versions_match_premises` reads `Agent::ALL`) |
| K-D4 | One registry file with criteria (K§6.2.2) | author | machine-checkable rubric |
| K-D5 | Union rule: a zygo feature that any client supports is a capability with a rubric, and each client gets a grade; a client with a missing grade fails the check. A new descriptor concern or adapter method also needs a capability that names it | operator intent; mechanics author | "all clients get graded" |
| K-D6 | Per-client grade file with measured evidence (K§6.2.3): `premise:<file>#<id>` counts only when `verified` or `pass`; an ignored test is not evidence; a cfg-limited test counts with its platforms | author; lead (I4, I5) | evidence that a test can find, with no id collision |
| K-D7 | Cell grade derived from criteria | author | no free-text grade |
| K-D8 | Score: cell value = met ÷ applicable; tier score = mean of cell values; total = weighted mean of the tiers (core 3, standard 2, extra 1), an empty tier `n/a`; rounded down last | author; lead (formula) | a transport gap weighs more than a status gap; rounding down never inflates |
| K-D9 | Bar: each core criterion met (or excepted). Core set: A-1, A-2, A-5, A-6, A-8, A-10, A-13, A-21. A-2 is added to the brief's set because the proxy refuses a request with no `X-Zygo-Session` (`crates/zygo-proxy/src/identity.rs:31`). Enforcement: the `supported` flag and check item 8 (K§6.2.7) | author | the simplest gate that the existing `cargo test` runs; no build features |
| K-D10 | The generator and check is one Rust test (K§6.2.5); it replaces `check-conformance.sh`; `check-premises.sh` becomes driven by data | author; lead (premise check by data) | TOML in shell is fragile; the scripts run in no CI job today; a premises header must not need an enum client |
| K-D11 | `zygo codex` with a per-session `CODEX_HOME` (K§6.3.1) | author | A-5 needs a per-session mechanism; hooks and agents need files |
| K-D12 | The proxy serves the Responses API as a client dialect (K§6.4) | author | Codex speaks only Responses |
| K-D13 | Hook trust order (K§6.3.6) | author | zygo never trusts a hook it did not write |
| K-D14 | Each provisional Codex grade is measured by a premise against the mock; no login; minimum 0.172.0 | author | measured grades |
| K-D15 | Non-goals of K§3 | author | scope |
| K-D16 | Codex ships when it meets the bar; workspace support graded | author | "found wanting, but sufficient" |
| K-D17 | `Dialect` moves to `zygo-core`, re-exported by `zygo-config` | author | the descriptor names the proxy dialect; core is the only crate below the proxy |
| K-D18 | Bar exceptions live in the registry, not in grade files; a `not-met` core criterion with an operator-approved exception counts for the bar, not for the score. Two exceptions are approved, each with its reason in the registry (`docs/conformance/capabilities.toml`): Claude Code A-1.4, approved with this spec (Windows without Git Bash runs the key helper in PowerShell, which needs `&` before the quoted path that zygo writes); OpenCode A-1.4, approved by the operator 2026-11-25 (OpenCode has no key-helper command). Reading of A-1: the "key helper" of A-1.2 is the client's mechanism that reads the session `token` file (Claude Code: `apiKeyHelper`; OpenCode: the `{file:}` reference in `provider.zygo.options.apiKey`); A-1.4 needs a command that runs in each shell, which OpenCode does not have | operator (approval of this spec; OpenCode A-1.4 2026-11-25); lead (place); lead (A-1 key-helper reading, 2026-11-25) | a registry change already stops for the operator; Claude Code ships today with that gap |
| K-D19 | `supported` flag and a gate at launch for each entry point (K§6.2.7) | author; lead (each entry point) | a below-bar client can be built and tested before release, and no path starts it by mistake |
| K-D20 | The CLI shim stays in the `zygo` crate under `cmd/client/` | author | clap is a dependency of the binary crate only |
| K-D21 | Messages is the translation pivot; same-dialect pairs pass through, reasoning bytes unchanged and unmarked | author; lead (no mark) | two new translators, not four; native custom tools and reasoning to OpenAI upstreams |
| K-D22 | Codex rollouts go to one shared zygo-owned store through a link in each `CODEX_HOME`; when PC10 fails, WA-1.2 is `not-met` and zygo prints no resume line | author | a resume needs the rollout in the new session's `CODEX_HOME` |
| K-D23 | Every key zygo owns is passed by `-c`; the preflight reads the directory that `-C`/`--cd` names | lead | a project config must not override any zygo key (A-5.3 graded by what is true) |
| K-D24 | No lead guard on Codex while hooks cannot tell the main loop from a subagent | author | a guard that refuses subagent edits breaks §8.3 |
| K-D25 | Codex premises run against `mock_upstream.py` directly, as the existing method | author | the premises need no proxy work, so P4-3 runs before P4-4; it tests the client, not zygo |
| K-D26 | P4-2 grades the criteria as written; each derived cell that differs from today's hand grade is listed in the PR with its reason, and that PR stops for the operator | author | the new rubric may raise or lower a grade; the operator sees each change |
| K-D27 | Client environment denylist rows are a descriptor concern; Codex denies `CODEX_*` for its own child only | author | removing `CODEX_*` from a Claude Code child would change its behavior |
| K-D28 | Evidence `test:` items name the file and the function | author | an exact, cheap check |
| K-D29 | The machine-readable output is `docs/conformance.json`, a generated repository file | author | the grade files are not in the binary; the data changes only with a commit; a file needs no command |
| K-D30 | The generated matrix has one row per capability and one column per client | author | the table stays readable as clients are added |
| K-D31 | `previous_response_id`, `background` and `conversation` are refused | author | C3: the proxy keeps no response state |
| K-D32 | The user's `~/.codex` is not read in a zygo session | author | user hooks and MCP servers would load with their own trust; A-5 |
| K-D33 | The generator gives a feature matrix, a rollup and a score per client (K§6.2.6) | operator | the addition of 2026-11-25 |
| K-D34 | A background check during a session; a HUD post when an update exists | operator | the addition of 2026-11-25 |
| K-D35 | The background check is a thread in the launcher process, first tick at 60 s, then hourly, gated by the shared 24 h throttle; no prompt, no install | author | the launcher lives for the session (W-G6 holds); hooks run the absolute binary path, so an install mid-session would mix versions |
| K-D36 | `zygo_core::update_state` owns `update.json`, `update-clients.json` and `update.lock`: a blocking lock with a 500 ms timeout, never held during a GET or a prompt; writers keep unknown keys | lead | concurrent sessions never lose a write; older binaries cannot erase client data |
| K-D37 | The background check ignores the U skips for a session, a pane, a non-TTY and `-p`; it keeps the two opt-outs | author | those skips exist because the start check prompts; member sessions must also learn of updates |
| K-D38 | A per-client `announce_client_updates` flag in the contract; default = NOT `announces_own_updates` | operator (the flag and the default rule); author (the reading "fill the gap, never double") | the addition on client updates |
| K-D39 | `[update.clients.<id>] announce` overrides the flag, user config only | author | the `[update]` table is already user-only (`layers.rs:68`) |
| K-D40 | `announces_own_updates` is a measured premise per client under a zygo launch; zygo sets no client update option | author | a zygo setting could hide the notice |
| K-D41 | `<client> <running> ungraded` from a local comparison only; verbose HUD, `zygo status`, `zygo doctor`; not the compact HUD | author | no network; a pin is a minimum, so the compact HUD would show it nearly always |
| K-D42 | Update notices are capability A-22 (extra), not criteria of A-13 (core) | author | a notice is not needed to run a session; a core criterion waiting on a live check would put the shipped clients below the bar |
| K-D43 | Codex and OpenCode users see the notice in `zygo status` and the launcher exit line, never in the session text | author | the session text is model context; the notice is for the user |
| K-D45 | The spec is approved, with the instruction to plan and build; rows in the order of K§6.9 | operator | the decision of 2026-11-25 |
| K-D46 | The Codex client gets its own design document against the built contract; K§6.3 and the Codex parts of K§6.4 are its requirements | operator | the decision of 2026-11-25 |
| K-D47 | The proxy accepts a route by dialect; wire format keys on `Dialect`, client policy on the descriptor | lead | build order: the route and its tests come before the Codex client exists |
| K-D48 | The tee parses usage per upstream stream dialect; cache markers are placed at forward time, only for an Anthropic upstream | lead | the same-dialect path would record no usage; markers belong to the body sent |
| K-D49 | Unknown top-level Responses fields are refused, except a named ignore list; `redacted_thinking` and `server_tool_use` have rules | lead | no field is dropped in silence |
| K-D50 | An upstream error before the first byte keeps its status with an OpenAI body; `response.failed` only after the stream started | lead | the client sees the real status |
| K-D51 | P4-6 adds capabilities or criteria for the client-facing features of phase 3 by the union rule | lead | phase 3 adds client features that no capability grades yet |
| K-D52 | P4-10 (zygo's own updates) is split from P4-11 (client updates) | author | the opportunistic row needs no client contract |
| K-D44 | The HUD compares `latest` with the launcher's version, recorded in the session view | author | a binary replaced in another terminal must not hide the notice for a session that still runs the old version |
| K-D53 | The boundary check has an exception list (K§6.1.4); a change to it stops for the operator; the first entry is the no-attribution guard | operator, 2026-11-25 | the guard matches attribution text and does not branch on a client |

---

## 8. Alternatives considered and rejected

1. **Keep the scattered branches and update the §8.8 checklist.** Rejected. 35 files hold a client branch today, and the checklist already misses three pieces. A checklist is not checked.
2. **The whole contract in `zygo-core` as one trait.** Rejected. Launch behavior needs config, plugins, proxy and workspace types; `zygo-core` depends on none of them, and the reverse direction would make a cycle.
3. **A new bottom crate `zygo-client`.** Rejected. It sits where `zygo-core` sits and adds one dependency to each crate, for no gain.
4. **The descriptor as a runtime data file.** Rejected. Hook normalization and tool-name parsing are functions, and a data file gives no compile error for a missing piece.
5. **Free-text grades (today's cells).** Rejected. A free-text cell cannot be checked, scored, or compared across clients.
6. **Equal weights.** Rejected. A missing status item would weigh as much as a broken transport.
7. **Chat Completions for Codex.** Rejected. Codex 0.172.0 accepts only `wire_api = "responses"`; chat support was removed.
8. **A numeric gate (score ≥ n%) in place of a core-tier bar.** Rejected. A client could pass with no transport and a high extra-tier score.
9. **A strict bar with no exceptions.** Rejected. Claude Code would be below the bar for a Windows PowerShell gap that it ships with today. An exception keeps the gap in the score and the document.
10. **Always pass `--dangerously-bypass-hook-trust`.** Rejected. It also trusts project hooks that the user never reviewed.
11. **Keep the user's `~/.codex` and pass only `-c` overrides.** Rejected. Hooks and agent files need files; the user's own hooks and MCP servers would load in the session.
12. **Translate Responses → Responses through the Messages pivot.** Rejected. It loses custom tools (the freeform `apply_patch`) and native reasoning.
13. **Shell scripts as generator, or a `cargo xtask`.** Rejected. TOML in shell is fragile, and the scripts run in no CI job. An xtask adds a workspace member and a CI step; the Rust test runs in the existing CI on three systems.
14. **A cargo feature per below-bar client.** Rejected. It splits the CI test matrix; the runtime gate is simpler.
15. **Capture Codex premises through the zygo proxy.** Rejected. It makes premises depend on P4-4 and mixes zygo's behavior into a client observation.
16. **A daemon or a timer service for update checks.** Rejected. W-G6: no daemon; the launcher already lives for the session.
17. **A network check in the status-line hook.** Rejected. The hook has a 3 s deadline (H-C2) and runs every 30 s; a file read is enough.
18. **The update notice in the session text or in a `UserPromptSubmit` context.** Rejected. That text reaches the model, not the user, and costs tokens each turn.
19. **Install in the background, apply at the next launch.** Rejected. U decided a prompt before an install; a silent install changes that approved design.
20. **Disable each client's own update notice and let zygo announce all.** Rejected. It changes client behavior that users know, and zygo would need each client's release source for every user.
21. **`zygo doctor --conformance` for the machine-readable form.** Rejected. The binary does not hold the grade files; a generated JSON file next to the Markdown needs no new command.
22. **A text-only scanner as the main boundary check.** Rejected. It has misses (macros, raw strings, `cfg(test)` on items that are not modules) and false positives; an opaque kind makes the compiler refuse the branch.
23. **A try-lock, or a lock held over the GET.** Rejected. A try-lock loses writes when two sessions write together; a lock held over a 2 s GET or a prompt blocks the other writers.
24. **A zygo mark on same-dialect reasoning.** Rejected. It changes bytes that the upstream made, for a case (a provider change between turns) that the upstream already reports as an error.

## 9. Files to create or change

Planning rows (P4-1, P4-5, P4-8) write work-graph files under `docs/work/` only.

P4-2 (conformance):

- `docs/conformance/capabilities.toml` (new): the registry, with the Claude Code A-1.4 and OpenCode A-1.4 bar exceptions (K-D18).
- `docs/conformance/claude-code.toml`, `docs/conformance/opencode.toml` (new): grades, from today's `docs/conformance.md` evidence.
- `crates/zygo-core/tests/conformance.rs` (new): check and generator; `crates/zygo-core/Cargo.toml`: `toml` and `syn` dev-dependencies.
- `crates/zygo-core/src/lib.rs`: `Agent::ALL` (P4-6 moves it to `client/mod.rs`).
- `docs/conformance.md`, `docs/conformance.json`: generated.
- `scripts/check-conformance.sh`, `scripts/test-check-conformance.sh`: deleted.
- `docs/specs/2026-11-18-zygo-design.md`: the §8.1 note (the registry is the list).

P4-3 (Codex premises):

- `scripts/check-premises.sh`: driven by data (K§6.2.5).
- `scripts/premises/run-codex.sh` (new), `scripts/premises/mock_upstream.py` (Responses answers and `CONNECT` records).
- `fixtures/codex/` (new): help, schema, requests, hooks, workspace and update captures.
- `docs/premises.md`, `docs/live-checks.md`: `Codex` header lines, PC rows, LC-CX1.

P4-4 (proxy):

- `crates/zygo-proxy/src/routes.rs`: the two routes.
- `crates/zygo-proxy/src/translate/responses_client.rs` (new): Responses → Messages request; Messages → Responses response and SSE.
- `crates/zygo-proxy/src/tee.rs`: the Responses `response.completed` usage parser and rewrite.
- `crates/zygo-proxy/src/forward.rs`: per-pair stream order (K§6.4.2), markers on the Anthropic upstream body for a Responses client, errors before the first byte.
- `crates/zygo-proxy/src/pipeline.rs`, `cache.rs`, `window.rs`, `redact_stage.rs`, `errors.rs`: the per-pair rules of K§6.4.2.
- `crates/zygo-proxy/src/models.rs`: the OpenAI models list.
- `crates/zygo-proxy/tests/` (new tests): the P4-4 acceptance items.

P4-6 (client contract, after phase 3; file list taken again at its base commit, K§6.9):

- `crates/zygo-core/src/client/mod.rs` (new): opaque `Agent`, private `Kind`, `ALL`, `DEFAULT`, `spec()`, `FromStr`, `as_str`.
- `crates/zygo-core/src/client/claude_code.rs`, `opencode.rs` (new): descriptors and pure functions, `supported = true`; `hook_opencode.rs` moves into `opencode.rs`.
- `crates/zygo-core/src/lib.rs`, `hook.rs`, `session_view.rs`: re-exports, normalization through the descriptor, `Agent::DEFAULT`.
- `crates/zygo-core/tests/client_boundary.rs` (new): K§6.1.4.
- `crates/zygo-core/tests/conformance.rs`: items 7 and 8 read the descriptor; `crates/zygo-config/tests/builtin.rs`: `minimum_agent_versions_match_premises` reads `Agent::ALL`.
- `crates/zygo-config/src/schema.rs`, `builtin.rs`, `check.rs`: `Dialect` re-export, defaults and kinds from the registry, the loadout gate; delete `crates/zygo-config/data/agents.toml`.
- `crates/zygo-redact/src/content.rs`, `harness.rs`; `crates/zygo-proxy/src/window.rs`, `passthrough.rs`, `cache.rs`; `crates/zygo-plugins/src/snapshot.rs`; `crates/zygo-work/src/hooks.rs`, `join.rs`; `crates/zygo-workspace/src/args.rs`, `state.rs`, `up.rs`, `snapshot.rs`, `loadout.rs`; `crates/zygo-hooks/src/session_start.rs`, `session_text.rs`, `workspace.rs`; `crates/zygo-mcp/src/tools/mod.rs`: read the descriptor.
- `crates/zygo-launch/src/client/mod.rs` (new): the `adapter_for` table and its test; `client/claude_code/`, `client/opencode/` (moved, with `assets/opencode-plugin.js`).
- `crates/zygo-launch/src/adapter.rs`, `launcher.rs`, `env.rs`: new methods, the launch gate, no client branch; remove `Plugins`.
- `crates/zygo/src/cmd/client/claude_code.rs`, `opencode.rs` (moved); `crates/zygo/src/cmd/mod.rs`, `ws.rs`, `member.rs`, `config.rs`, `doctor.rs`, `compact.rs`, `hook.rs`, `loadout.rs`: registry calls, hidden help for an unsupported client.
- Test files: the edits that K-C7 allows, and the test logic edits listed in their node (check item 7 reads the descriptor; `minimum_agent_versions_match_premises` reads `Agent::ALL`); `scripts/check-test-edits.sh` (new).
- `docs/conformance/capabilities.toml`, each grade file: the capabilities or criteria for the phase 3 client features (K-D51).
- `docs/specs/2026-11-18-zygo-design.md`: the §8.4 and §8.8 notes (K§12).

P4-7 (Codex design):

- `docs/specs/<date>-codex-client-design.md` (new): designed against the built contract, with K§6.3 and the Codex parts of K§6.4 as requirements.

P4-9 (Codex client):

- `crates/zygo-core/src/client/codex.rs` (new), `crates/zygo-launch/src/client/codex/` (new), `crates/zygo/src/cmd/client/codex.rs` (new), `crates/zygo/src/cmd/mod.rs` (`Cmd::Codex`).
- Codex client policy: `insert_markers.codex` default, redaction spans, window arming.
- `crates/zygo-launch/tests/stub_agent/`, `stub_launch.rs`, `premise.rs`, `codex.rs` (new): Codex parts.
- `docs/conformance/codex.toml` (new) in the same commit as the `Agent` entry; `supported` set by the bar.
- `docs/install.md`, `scripts/install.sh`, `scripts/install.ps1`: Codex listed when supported.
- `docs/conformance.md`, `docs/conformance.json`: generated.

P4-10 (zygo's own updates):

- `crates/zygo-core/src/update_state.rs` (new): K§6.5.3.
- `crates/zygo/src/cmd/update.rs`: the start check uses `update_state`; the release check (`check_latest`, `latest`, `release_base`) moves to `crates/zygo-launch/src/update_check.rs` (new), with the background thread.
- `crates/zygo-launch/src/launcher.rs`: records the running zygo and client versions in the session view; prints the exit line.
- `crates/zygo-core/src/session_view.rs`: two additive optional fields.
- `crates/zygo-hooks/src/statusline/data.rs`, `render.rs`, `width.rs`: segment 16 and its drop order.
- `crates/zygo/src/cmd/status.rs`, `doctor.rs`: the update line.
- `docs/conformance/capabilities.toml`, each grade file: A-22.
- `docs/specs/2026-11-18-zygo-design.md` (§3 unchanged in P4-10), `docs/specs/2026-11-21-statusline-hud-design.md` (H§6.2, H§6.7), `docs/specs/2026-11-21-auto-update-design.md`: amendment notes.

P4-11 (client updates):

- `crates/zygo-core/src/client/*.rs`: `announces_own_updates`, release source.
- `crates/zygo-launch/src/update_check.rs`: client checks into `update-clients.json`.
- `crates/zygo-config/src/schema.rs`, `check.rs`: `[update.clients.<id>] announce`; the error for a client with no release source.
- `crates/zygo/src/cmd/status.rs`, `doctor.rs`, statusline: the client and ungraded lines.
- `scripts/premises/`, `fixtures/claude-code/update/`, `fixtures/opencode/update/` (new): PU1, PU2; `docs/premises.md`, `docs/live-checks.md`: PU rows, LC-CC1, LC-OC1.
- `docs/specs/2026-11-18-zygo-design.md`: the §3 amendment (K§6.5.6).

## 10. Left to the builder

- Field and method names of `ClientSpec` and the new adapter methods.
- The exact JSON field mapping of K§6.4.3, inside its obligations.
- The zygo mark on reasoning items, inside K§6.7.
- The hook trust record format and the exact hook event and key names (from PC4, PC5). The record's key form and place are measured (K§6.3.6 branch 1, PC-4 round 3); the `trusted_hash` rule is still open (not found), and branch 1 waits for it.
- The Codex flag table (from the pinned `--help`).
- The text layout of `docs/conformance.md`, inside K§6.2.6; the JSON key order, once fixed.
- The one call site of the update thread (K§6.5.2) and the exact session-view field names.
- The exact npm package names, after each client's premise confirms them.
- The order of the files in each move, if each step keeps the suite green.

## 11. Acceptance

Each item is decided by a test or a command.

P4-2 (conformance):

1. `conformance` passes; with a planted fault it fails for each item 1-10 of K§6.2.5 (one unit case per item), including: an evidence test with `#[ignore]`; a `premise:` row whose result is `failed`; a premise id that is in the other file only; a `bar_exception` key in a grade file.
2. A `#[cfg(unix)]` evidence test counts, and the generated document shows `unix` next to it; a `#[tokio::test(flavor = "multi_thread")]` test counts.
3. `docs/conformance.md` and `docs/conformance.json` equal the generator output; CI fails when either does not.
4. **Feature matrix.** For a fixture registry and two fixture grade files, the matrix rows come grouped core, standard, extra, in registry order inside a tier; each cell text is `full <url>`, `partial (missing: …) <url>`, `none (…) <url>` or `n/a (…)`; the JSON `matrix` holds the same grades and values.
5. **Rollup.** For the same fixtures, each tier row and the total row give the expected full / partial / none / n/a counts and met / applicable counts; a client with one unmet core criterion and no registry exception shows `below bar` with that capability id; with an exception it shows `meets bar`.
6. **Score.** For the same fixtures, each tier score is the mean of its cell values and the total is the weighted mean of the tiers, rounded down last, equal to values computed by hand in the test; a tier with no applicable cell shows `n/a` and leaves the total's weights; the K§6.2.6 example numbers come out as written.
7. `minimum_agent_versions_match_premises` (`crates/zygo-config/tests/builtin.rs:223`) passes unchanged.

P4-3 (Codex premises):

8. `sh scripts/check-premises.sh` passes with the `Codex: 0.172.0` header and the PC rows, with no Codex client in `Agent::ALL`; it fails when a required existing id (for example `Q7`) is removed.
9. Each PC row has a date, a result and fixture paths that pass the `nontrivial` rule.
10. No fixture holds a token, a key form, a user name, a home path or an e-mail address (`scrub.py` pass). An address `<local>@example.invalid` at exactly the reserved domain (RFC 2606) is exempt, where `<local>` is only letters, digits, `.`, `_` and `-` and is not part of a percent hack or bang path: it can never resolve or belong to anyone. A verbatim upstream file (the Claude Code settings schema, the Codex `config.schema.json`) is exempt, at its exact path (case-sensitive), from the home-path rules (macOS and Windows), the bearer rule and the `N2_FORBIDDEN_NAMES` name scan only, and only while it is a regular file whose sha256 equals the value pinned in `scripts/check-premises.sh` and its source file gives the same value; a new upstream copy needs a reviewed change to the check. The host-name scan and every other rule still cover the verbatim files. Every scanned file is read as text, also a binary one. A path in the scanned scope that holds whitespace or `:` fails, because the scan could not read it or name it.

P4-4 (proxy; a Claude Code session; Codex request bodies from `fixtures/codex/requests/`; a mock upstream):

11. Each captured Codex request translates to a valid Messages request; each request obligation 1-11 of K§6.4.3 has a case.
12. An unknown top-level field gives 400 that names it; each field of the ignore list is accepted; each refused field gives the stated 400.
13. A Messages SSE fixture with text, tool use, thinking and redacted thinking gives the event sequence of stream obligations 1-5, with rising `sequence_number`; a `server_tool_use` block is dropped with `server_tool_dropped`.
14. An upstream `529` before the first byte gives `529` with an OpenAI error body and no SSE; an upstream error after the first event gives one `response.failed` and no other terminal event.
15. A tool call round trip (call, output, next request) keeps `call_id`.
16. A thinking round trip to the same provider keeps the signature; to another provider it is dropped with its code.
17. Responses → Responses: the upstream body equals the client body except `model`, `store`, `include` and stage edits; reasoning items are byte-equal; the usage store row has the token counts of the upstream `response.completed` (non-zero in the fixture), and a non-streaming request records the counts of the JSON body's `usage`.
18. Responses → chat: a captured request reaches the chat mock, and its stream returns as Responses SSE with usage recorded.
19. Responses → Anthropic with `insert_markers` on for the session's client: the upstream body has the markers; Responses → Responses: no `cache_control` anywhere.
20. Redaction acts on a Responses `input` body.
21. With the rewrite armed for the test session, `response.completed` usage follows §10.2.2 for an Anthropic upstream and for a Responses upstream; unarmed, the bytes pass unchanged.
22. `GET /openai/v1/models` gives the role list; `POST /openai/v1/chat/completions` stays `404 zygo_route_not_in_v1`.

P4-6 (client contract):

23. `cargo test --workspace --locked` passes on Linux, macOS and Windows CI at the top of each P4-6 layer.
24. For each P4-6 node, `scripts/check-test-edits.sh <parent> <node file>` passes, `<parent>` the parent of the node's layer: in each test file and each `#[cfg(test)]` module that exists at `<parent>`, no test function is removed or renamed, no `#[ignore]` is added, and the multiset of string, char and number literals is unchanged, except each change that the node file lists under "Test logic edits" with its spec reason. The review gate checks each listed edit (K-C7).
25. A file outside `zygo_core::client` that names a client variant does not compile (a `trybuild` compile-fail case).
26. `client_boundary` passes on the tree, and fails on test inputs that hold `"opencode"` as a plain literal, inside `format!`, as a raw string, and in a non-module item without `#[cfg(test)]`; it does not fail on `"zygo claude: …"` or a comment.
27. The `adapter_for` table test fails when a client has no row.
28. `stub_launch` asserts byte-equal generated settings and inline config for both clients against the output at the P4-6 base commit (the existing assertions).
29. A state file with `"kind": "claude-code"` reads into the pane kind of that client and writes back the same bytes.
30. The registry has at least one capability or criterion for each of output shaping, mail wake and delivery, and the run-time status line, each graded for each client (K-D51); `conformance` passes.

P4-9 (Codex client; at the commit that adds Codex with `supported = false`, and as permanent tests that set the flag):

31. With `supported = false`: `zygo codex`, `zygo ws run` for a Codex pane and the loadout check each refuse with the K§6.2.7 text; with `ZYGO_PREVIEW_CLIENT=codex` each runs; the gate test runs the Codex descriptor with both flag values.
32. The Codex stub launch test on the three systems asserts: the `CODEX_HOME` tree and modes; each owned key both in `config.toml` and as `-c`; no token in arguments, environment or files except `token`; no `CODEX_*` from the parent environment; the hooks and agent files; the `sessions` link.
33. A planted `-C <dir>` with `<dir>/.codex/hooks.json` selects K-D13 branch 3; without `-C` the same file in another directory has no effect; the three branches are chosen from planted files as stated.
34. The premise test finds each written key in `fixtures/codex/config.schema.json` and each flag in `fixtures/codex/help.txt`.
35. Each Codex hook fixture parses to the common form; each reply renders in the PC5 shape; a non-object body and an unknown event give no output and exit 0.
36. A workspace stub test captures the Codex session id and builds `resume <id>`.
37. `codex.toml` is in the same commit as the Codex `Agent` entry; each Codex grade cites counting evidence; `client_boundary` and `conformance` pass.
38. If the bar is met: `supported = true`, `zygo doctor` lists Codex, `docs/install.md` names it. If not: the generated document shows `below bar`, and the row ends with the unmet core criteria listed for the operator.

P4-10 (zygo's own updates; fixture server as in `crates/zygo/tests/update.rs`; the tick and the clock are injected, K-C6):

39. One tick with a stale throttle makes one GET and writes `checked_at` and `latest`; a second tick within 24 h makes none.
40. Two writers: one holds the lock; the other waits and then writes, or skips after 500 ms; neither holds the lock during its GET (a GET that the fixture server holds open does not block the other writer's lock); a key that the module does not know survives a write.
41. An older-form write of `update.json` (the U fields only) leaves `update-clients.json` unchanged.
42. The tick runs with `ZYGO_SESSION` and `ZYGO_WORKSPACE` set, off a TTY and with `-p`; it does not run with `[update] check = false` or `ZYGO_NO_UPDATE_CHECK=1`.
43. The tick never prompts and never writes the binary (the test binary path is unchanged after a tick that finds a newer version).
44. A failed GET writes `checked_at`, one session-log line, and no segment.
45. The status line with `latest` newer than the session view's version shows segment 16: compact `update <new>`, verbose at the end of row 1; equal, older, or equal to `skip`: no segment; a 5 KiB or non-JSON file: no segment and no error output.
46. Under width pressure, segment 16 is removed first.
47. `zygo status` prints the update line; the launcher prints the exit line when stderr is a TTY and not otherwise.

P4-11 (client updates):

48. With a client flag on, a fixture release source makes one GET per 24 h per client; the HUD and `zygo status` show `<client> <new> available (zygo graded <pinned>)`; with the flag off (the default when the fact is `true`) no client GET happens.
49. With a zygo update and a client update both pending: compact shows the zygo update only; verbose shows both, zygo first; under width pressure the client part goes first.
50. `[update.clients.codex] announce = true` in user config turns the flag on; the same key in a repository or team layer has no effect (`layers.rs:68`); `announce = true` for a client with no release source is a `zygo config check` error.
51. A session view with a client version newer than the descriptor pin gives `<client> <running> ungraded` in `zygo status`, `zygo doctor` and the verbose HUD, not in the compact HUD; the fixture server sees no request.
52. `grep -rnE 'autoupdate|DISABLE_AUTOUPDATER|check_for_update_on_startup' crates/zygo-launch/src` finds nothing; each PU premise row is `verified` and A-22.2 cites it; for Codex, the P4-11 premise row for Codex under a zygo launch is `verified` and A-22.2 cites it, not PC16 (PC16 measures Codex's own default with no zygo between, K-D25).

## 12. Base-spec changes

- **§3** (zygo design): the "Nothing phones home" exception widens as K§6.5.6 states (P4-11).
- **§8.1**: the capability list is `docs/conformance/capabilities.toml` (P4-2); A-22 is added (P4-10); the phase 3 client features are added (P4-6); the cell form and C9 stand.
- **§8.4**: `ZYGO_AGENT` takes each registry id (`claude-code`, `opencode`, `codex`); each client adds its own denylist rows.
- **§8.8**: superseded by K§6.8 flow 6.
- **§7.6 v2**: the Responses client route is built (K§6.4); the chat client route and the `openai-chat` client pair stay v2.
- **§7.6 context-limit errors** (RS-3): Anthropic's `` input length and `max_tokens` exceed context limit: <a> + <b> > <c> `` text is not mapped to `prompt is too long` for an `anthropic` client: it reaches that client unchanged (Claude Code parses it); only the Responses client maps it (K§6.4.4).
- **§7.2, §8.4, §8.7** (OC-1, operator 2026-11-25): OpenCode's provider key is a `{file:…}` reference to the session's `token` file, the inline config holds no token, and it sets `enabled_providers = ["zygo"]` and a `disabled_providers` that keeps every earlier list without `zygo`.
- **H§6.2, H§6.7** (HUD design): segment 16 and its place first in each drop list.
- **U** (auto-update design): the throttle state has one owner, `zygo_core::update_state`, shared with the background check; U's prompt, skips and rule are unchanged.

## Change log

- 2026-11-25: first version.
- 2026-11-25: the operator's two additions: the three conformance outputs (K§6.2.6) and update notices (K§6.5, then P4-7), with the operator's answer on client updates.
- 2026-11-25: order against phase 3 (K§6.9); items renumbered in causal order (then: conformance P4-1, Codex premises P4-2, proxy P4-3, client contract P4-4, Codex client P4-5, Codex grade P4-6, update notices P4-7).
- 2026-11-25: approved by the operator (instruction to plan and build); new row order P4-1 to P4-11 (K§6.9); the Codex client gets its own design (P4-7); the fixes of the spec audit (lead rulings C1-C3, I4-I15, M16, M17).
- 2026-11-25: K-C7, K-D3 and K§11 items 23-24 amended (lead): the test-edit proof is per node against its layer's parent, with listed test logic edits, because declared mechanical rewrites cannot be met at P4-6's scale. K§6.1.4 gets an exception list (operator, K-D53).
- 2026-11-25: K§6.2.4 amended (lead): failing core capability matches the bar sentence: an ungraded or n/a core criterion fails (CF-2 review F4).
- 2026-11-25: K§12 gets the OC-1 base-spec change (operator: OpenCode A-1.2, A-1.3, A-10.2 fixed in code before CF-4).
- 2026-11-25: the K§12 OC-1 line names `disabled_providers = []` (OC-1 review round 2).
- 2026-11-25: the K§12 OC-1 line: the inline `disabled_providers` keeps the user's lists without `zygo` (OC-1 review round 3).
- 2026-11-25: K§6.2.2 A-1.2, A-10.2 — passthrough excepted (§12.3); approved by the operator 2026-11-25.
- 2026-11-25: K-D18, K§9 — OpenCode A-1.4 bar exception, approved by the operator 2026-11-25.
- 2026-11-25: K-D18 — reading of A-1 "key helper" (OpenCode {file:} reference meets A-1.2; A-1.4 needs a command); author's call, for the operator at the CF-4 merge (K-D26).
- 2026-11-25: K§6.2.2 A-9 — the OpenCode precedent points to its A-9.1 to A-9.5 grades in `docs/conformance/opencode.toml`, not to `docs/conformance.md` (pointer correction); author's call.
- 2026-11-25: K§6.2.3 — the bar exception example's reason is the registry's text and cites no hand note in `docs/conformance.md`; author's call.
- 2026-11-25: K§6.3.7 amended (operator): the proxy variables: HTTP_PROXY and ALL_PROXY too, so plain-HTTP and SOCKS-aware clients also reach the mock.
- 2026-11-25: K§11 item 10 amended (author's call): the e-mail rule exempts the reserved domain example.invalid (RFC 2606); the live scripts use it as a git test identity.
- 2026-11-25: K§11 item 10 amended (author's call): the verbatim files' sha256 values are pinned in the check; the example.invalid exemption takes only a plain local part; a scanned path that holds whitespace or `:` fails (PC-8 review round 2).
- 2026-11-25: K§11 item 10 amended (author's call): the verbatim-file exemption names its rules exactly (home paths, bearer, `N2_FORBIDDEN_NAMES`) and is exact-path and case-sensitive; binary files are scanned as text (PC-8 review round 3).
- 2026-11-25: K-D18 — the Claude Code A-1.4 reason points to the registry (`docs/conformance/capabilities.toml`), not to the `docs/conformance.md` "Windows shell for the generated commands" note, which generation removes (pointer correction); author's call.
- 2026-11-25: K§6.4.3 — `client_metadata` in the safe-to-ignore list (every captured Codex request sends it, PC-3); obligations 1, 2, 4-7 name the place of a later developer item, the assistant text parts, the grammar text, the tool-definition types (`namespace` dropped), the hosted item types, the adjustment codes and `max_tokens` with no catalog value (RS-1 premise check); author's call.
- 2026-11-25: K§6.4.3 — obligations 2, 3, 5, 6, 7 and 9: empty text blocks dropped and a user-first conversation required; tool-call pairing; decoded thinking blocks keep known keys only; safe type names in adjustment codes; `tool_choice` checked against the sent tools; typed mapped fields; a non-string item type; echoed names cut and without control characters (RS-1 review round 1); author's call.
- 2026-11-25: K§6.4.3 — `text.verbosity` is checked and not sent toward Messages (no counterpart) and another `text` key is refused; obligation 3 adds Messages tool adjacency and pairs calls past empty user messages; obligation 6 adds unique tool names and tools declared from the call history (`tools_from_history`, a PC-5 compaction request sends `tools: []` with calls in its history); obligation 9 echoes no Cf, Zl or Zp character (RS-1 review round 2); author's call.
- 2026-11-25: the K§6.3.7 PC16 row states what P4-3 measures (author's call, PC-6 review round 1 F3): Codex's own default against the mock with no zygo between (K-D25), the release-source request against a control run with the key `false` and the `exec` runs; the measurement under a zygo launch is P4-9's and P4-11's.
- 2026-11-25: K§11 item 52, the K§6.3.8 A-22 row, the PC16 row's Feeds and the K§6.9 P4-10/P4-11 note amended (author's call, PC-6 review round 2 G2): for Codex, A-22.2 cites the P4-11 premise row for Codex under a zygo launch, not PC16, which measures Codex's own default with no zygo between (K-D25); the PU rows and A-22.2's criterion text are unchanged.
- 2026-11-25: K§6.5.4 release source gains the measured result for Codex (author's call, PC-6 review round 2 G3): inferred as `api.github.com:443` from PC16's controlled count difference, path not visible; `registry.npmjs.org` not seen; P4-11 verifies before use.
- 2026-11-25: K§6.4.3 stream obligations 2, 5, 9 — custom tool calls return as custom_tool_call items; the terminal event carries no output copy; the encoder's held bytes (RS-2 premise check); author's call.
- 2026-11-25: K§6.4.3 stream obligations 2, 5, 6, 9 — custom tools named by `custom_tool_call` history items return as custom_tool_call items; a reasoning mark over MARK_MAX is not sent (`reasoning_unmarked`); `model_context_window_exceeded` and `refusal` give `response.incomplete`, `pause_turn` completes; an upstream error message is cut as a 400 echo; the held-bytes bound holds inside one chunk and the failure names its cause (RS-2 review round 1); author's call.
- 2026-11-25: K§6.4.3 candidates of a Responses client (`anthropic`, then `openai-chat`; `openai` from RS-5), K§6.4.4 over-long forms and the K§12 §7.6 context-limit line (lead, RS-3 review round 1 F1, F2).
- 2026-11-25: K§6.4.4 and the K§12 §7.6 line: Anthropic's `max_tokens` limit form maps to the PC14 shape for the Responses client only and reaches other clients unchanged (lead, RS-3 review round 2 G1-G3).
- 2026-11-25: K§6.4.1 `GET /openai/v1/models` — `base_instructions` empty and no `apply_patch_tool_type`; the Codex launch supplies both through the K§6.3.1 catalog file and must not rely on this route (lead, RS-8 planning).
- 2026-11-25: K§6.3.1 item 2 — each catalog entry carries Codex's own base prompt as `base_instructions` and `apply_patch_tool_type = "freeform"`, the other fields as `catalog-patch.json`; two open premises for P4-9 (the source of the base prompt; how Codex reads `base_instructions: ""`); K§6.4.1 points to it (author's call, RS-8 review round 1 F1).
- 2026-11-25: K§6.4.3 request obligations 3, 6, 7 and stream obligation 2 — `namespace` tools flatten to `<namespace>__<name>` function tools (a hashed form for other characters or over 64), namespaced history calls use the flat name, a model call maps back to `function_call` with `name` and `namespace` as PC-5 records (RS-9 premise check); author's call.
- 2026-11-25: K§6.4.3 request obligations 6 and 7 — one name stands for one tool across `tools` and the call items of `input` (a namespaced call's flat name that is also a plain tool's or another pair's name → 400; a plain call of a sent pair's flat name, Codex's pre-0.172 MCP naming, binds to that pair); a `tool_choice` with a `namespace` names only a sent pair, and a `custom` one with a `namespace` → 400; the flat name's hash input is length-prefixed; a `tool_choice` `namespace` that is not a string → 400 (RS-9 review round 1); author's call.
- 2026-11-25: K§6.4.3 — stream obligation 9 states both bounded buffers of a Responses stream and the peak of about 2 × `HOLD_MAX` per stream; author's call (RS-4 round 2 review G4: the tee line buffer is a second bounded buffer).
- 2026-11-25: K§6.4.3 obligation 6 and K§6.4.4 — author's call, RS-4 round 3 review H5/H6: spec matches the drain and the PC14 cleaning.
- 2026-11-26: K§6.4.3 stream obligation 6 and K§6.4.4: in-stream over-long prompts get the PC14 code `context_length_exceeded`; in-stream error messages lose the provider key before the cut; a read error gives `upstream response failed`, started first when no event came; after the terminal event the upstream is read to its end for the usage row (lead, RS-4 review round 1 F3-F5).
- 2026-11-26: the K§6.3.7 PC table gains PC17 (Codex accepts the encoder's Responses event sequence: extra events, empty terminal `output`, text, function_call, custom_tool_call and marked reasoning items) and PC18 (`response.incomplete` and `response.failed` end the turn), measured by node PC-12 before RS-4 (lead's decision, PC-12 plan); author's call on the wording.
- 2026-11-26: the K§6.3.7 PC17 and PC18 rows name the shapes PC-12 serves after its review round 1 (server tool dropped, message then call, redacted and unmarked reasoning, two calls, `content_filter`, a half-open call) and PC18's retry condition (0 retries; Codex's default retries recorded); obligations 1-5 and 7 for PC17, 5, 6, 9 for PC18; PC17's echo allows `content` and `encrypted_content` as `null` when absent from the sent item; obligation 9 states that a default-configured Codex retries a turn that ends in `response.failed` up to 5 times (lead's decisions, PC-12 review round 1).
- 2026-11-26: K§6.4.3 stream obligation 6 holds the measured retry counts of a default-configured Codex per ending (moved from obligation 9, whose endings PC-12 does not serve); the PC18 row covers obligations 5 and 6, checks 1 request, a non-zero exit and the exact message under 0 retries, and records the default-retry counts per ending (PC-12 review round 2, G1 and G5); author's call on the wording.
- 2026-11-25: K§6.4.3 top-level `text` rule and new request obligation 11 — author's call, RS-11 planning: Codex thread-title requests send `text.format` `json_schema`; it maps to Messages `output_config.format` (the §7.6 facts, schema rules and chat `response_format` already handle that), not to a forced tool.
- 2026-11-25: K§6.4.2 C3 and K§6.4.3 (Responses → Responses, candidates of a Responses client) as final text: the tee also reads `response.incomplete` usage, Responses counts by the §14 translated-path rule, the forward step names the form the tee sees, `include` of another JSON type → 400, `namespace` tools pass unflattened, `openai` is the first candidate dialect; author's call (RS-5 premise check: a `max_output_tokens` ending recorded no usage; the `include` type and the RS-9 passthrough case were open; stale line citations).
- 2026-11-25: K§6.4.3 stream obligation 8: a tool_use input from a JSON body reaches the encoder in `input_json_delta` pieces of at most 64 KiB, within `HOLD_MAX`; author's call, "RS-10 review F3: a tool_use input from a JSON body went past HOLD_MAX on the streamed path".
- 2026-11-25: K§6.4.3 stream obligations 8 and 9: a tool call's arguments are bounded alone within `HOLD_MAX`, and the partial line and event that carry its deltas alone; the encoder peak while a tool call is open is about 2 × `HOLD_MAX`; author's call, "RS-13 round 1 review F1: tool arguments bounded by held text, not by the raw delta line".
- 2026-11-25: K§6.4.3 obligations 8 and 9; author's call, "RS-13 round 2 review: chat tool arguments in 64 KiB pieces; every stream buffer and the per-path peak stated".
- 2026-11-25: K§6.4.3 obligations 8 and 9; author's call, "RS-13 round 3 review: per-sender event bounds, between-read buffer bounds, JSON-body amplification recorded (DDR-01M8BACPSY0WNJ0DK83YN82PAC)".
- 2026-11-25: K§6.4.4 (the provider key never reaches the client, translated or not: untranslated non-2xx body and in-stream error events scrubbed, success bytes unchanged); author's call, "RS-5 review F5: untranslated passthrough echoed the provider key".
- 2026-11-25: K§6.4.2 C3 — author's call, RS-5 round 1 review: response.failed usage read; a stream with no readable usage is marked.
- 2026-11-25: K§6.4.3; author's call, "RS-5 round 2 review G1: include type check only with an openai candidate".
- 2026-11-25: K§6.4.3 obligation 11, author's call, RS-11 round 1 review: claims limited to what the repo shows; dropped format fields recorded.
- 2026-11-25: K§6.4.3 obligation 11, author's call, "RS-11 round 2 review: a null format key counts as absent".
- 2026-11-25: K§6.4.2 stage table, row cache markers; author's call, "RS-6 planning: the translated Messages body carries `system` as a string, which §10.4 leaves unmarked; the marker makes it one text block so the instructions are cached; citation moved to `responses_attempt`".
- 2026-11-25: K§6.4.2; author's call, "RS-6 round 1 review F3: an upstream marker guard looks only where Anthropic accepts a marker".
- 2026-11-25: K§6.4.5 as final text; author's call, "RS-7 premise check: forward.rs gave a translated Responses stream's lines to the error rewrite only, never to the window stage; the Responses-form edits (total_tokens kept equal to input + output), response.incomplete, and the trigger 1 and 2 rules on Responses lines were open".
- 2026-11-25: K§6.4.5; author's call, "RS-7 round 1 review: end of turn uses streamed call items; non-integer usage left unchanged".
- 2026-11-25: K§6.4.3 stream obligations 8 and 9 as final text; author's call, "RS-13 round 3 review: text deltas in 64 KiB pieces. RS-15 planning: a decoded text piece escapes to at most 6 × 64 KiB, not 2 ×; `message_events` cuts thinking the same way; an empty text or thinking block keeps its one delta; per-piece overhead added to the JSON-body count".
- 2026-11-25: K§6.4.4 (untranslated passthrough) as final text; author's call, "RS-14 premise check: line mode on the Messages route is armed only by the window stage, so the key arms it; an over-long or split `data` line cannot be read for its JSON `type`, so an event is classed by its `event` field and an unnamed event's unreadable line fails closed; no line is held, because success bytes stay unchanged; the rest of a cut body is read for the usage row; non-UTF-8 bytes are read as on the translated path".
- 2026-11-26: K§6.4.4; author's call, "RS-14 round 1 review: line classing could not fail closed; the key is replaced in all body bytes, headers holding it are dropped, an encoded body with a key set is refused".
- 2026-11-26: K§6.4.4; author's call, "RS-14 round 2 review: key scrubbed on every 2xx upstream body, translated too; trimmed key; encoded non-2xx keeps its status; key limits and memory stated".
- 2026-11-26: K§6.4.4; author's call, "RS-14 round 3 review: residual escaped and split-delta forms stated and recorded (DDR-01M8BHQNT1DP5RKYVJBCZ01HDF); trimmed key in the encoder; retry-after kept".
- 2026-11-25: K§6.3.1 item 2 — `capabilities.remote_compaction = "unsupported"` in place of `false`: Codex 0.172.0 refuses `false` under `--strict-config` ("invalid type: unit variant, expected string only") and accepts `"unsupported"` (PC-4 strict-config evidence); author's call.
- 2026-11-26: K§6.3.1 item 2 — `[mcp_servers.zygo]` carries `default_tools_approval_mode = "approve"`: without it Codex 0.172.0 under `approval_policy = "never"` refuses every MCP tool call (PC-4 round 3 capture); the key passes `--strict-config` (lead's decision, PC-4 review round 2 G1).
- 2026-11-26: K§6.3.6 branch 1 and K§10 (hook trust record): the measured record form and place, replay honored, a wrong hash refused, the `trusted_hash` rule not found, so branch 1 is not yet open (PC-4 review round 2 G3: PC4 `partial: replay only`); author's call.
- 2026-11-26: K§6.4.3 stream obligations 5 and 6, K§6.4.2 (the tee sentence), K§6.4.5 (the Messages-form terminal event) and the K§6.3.7 PC18 and new PC19 rows: a `refusal` stop maps to `response.completed` after one more `message` item whose `output_text` part is the refusal text, in place of `response.incomplete` `content_filter`, which a default-configured Codex 0.172.0 sent 6 times; chosen from four measured candidates by the first that passes in the order `refusal` part, `output_text` part, `response.failed` `invalid_prompt`; author's call, RS-12.
- 2026-11-26: K§6.4.3 stream obligations 5 and 6, K§6.4.2 (the tee sentence), K§6.4.5 and the PC19 row: a `refusal` in a response that holds a `function_call` or `custom_tool_call` item ends in `response.failed` `invalid_prompt` (ending `rfd`, inferred) on every translated upstream, so Codex runs no call whose arguments may be cut; a text-only refusal keeps `rfc`. The tee sentence names each Responses terminal event it reads, `content_filter` passthrough included; obligation 6 records the same-dialect `content_filter` passthrough as accepted behaviour; lead's decision, RS-12 review round 1.
- 2026-11-26: K§6.4.3 stream obligation 5: at a refusal (ending `rfd`), a call item still open gets no done events and is dropped as on every other failure path (PC18 `xt`); calls closed before the refusal stay as sent. The `rfd` source is run 2 (`result.txt`), equal to run 1; lead's decision, RS-12 review round 2.
- 2026-11-26: K§6.4.3 stream obligation 5; author's call, "RS-12 round 3 review: chat content_filter drops held calls; Anthropic closed-before-refusal recorded (DDR-01M8CAJ3FPS180544MPTP1V0EC)".
- 2026-11-26: K§6.4.3 stream obligations 5 and 9: a Messages client never gets `zygo_calls_dropped` (the row gets `refusal_call_dropped:<n>`); the mapper's Responses arm holds a `function_call` block to its `response.output_item.done`, drops it at `content_filter`, and bounds it by `HOLD_MAX`; author's call, "RS-16 premise check: the Responses arm sent a call's block as it streamed, so no held call existed to drop at content_filter".
- 2026-11-26: K§6.4.3 stream obligations 5 and 9: the Responses arm holds a `function_call` as one call (id, name, joined arguments), sent in 64 KiB `input_json_delta` pieces, held until its `response.output_item.done` or until another output item starts; `responses::response` drops at `content_filter` only a `function_call` whose `status` is not `completed`; author's call, RS-16 review round 1 F1-F3.
- 2026-11-26: K§6.4.3 stream obligations 5 and 9: only a `response.output_item.done` with `status` `completed` ends the hold of a Responses `function_call` (else it stays held and is dropped at `content_filter`, as `responses::response` drops it, unless another output item starts first: then it is sent, an accepted limit, DDR-01M8CE0JMGSVYFYKASBVRHY5VX, corrected in RS-16 round 3); a late done or argument delta for a call already sent is ignored; author's call, RS-16 review round 2 G1, G2.
- 2026-11-26: K§6.4.3 obligations 5 and 9; author's call, "RS-16 round 3 review: not-completed call sent when another item starts recorded (DDR-01M8CE0JMGSVYFYKASBVRHY5VX); sent set".
- 2026-11-27: K§6.9 (intro and the P4-6 row); author's call: phase 3 (`origin/phase-3`) is taken by `phase-4` directly, and `main` is taken again when phase 3 reaches `main`, because phase-3 → `main` waits on operator live checks in the phase-3 graph (MS-23, WD-21, WD-22) and CC-1 to CC-16 need only the phase 3 code; the intro states the measured phase 3 changes outside the client sites (`serve.rs`, the PO1/PO2 id clash in `docs/premises.md`).
- 2026-11-27: K§6.9 P4-6; operator: phase 3 and phase 4 reach `main` together through the `phase-4` pull request (#241); `phase-4` takes each new phase-3 head in a merge layer of its own, and phase 3 is not merged into `main` on its own.
