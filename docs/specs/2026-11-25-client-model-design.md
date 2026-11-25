# zygo: client model, conformance score, Codex and update notices — design

**Status:** proposed 2026-11-25
**Date:** 2026-11-25
**Release:** phase 4 (roadmap items P4-1 to P4-7, `docs/roadmap/phase-4.md`)
**Base specs:** `docs/specs/2026-11-18-zygo-design.md` (`§n`; C3, C5, C6, C9, §3, §7.6, §8, §10.2, §12.2). Also cited: `docs/specs/2026-11-20-workspaces-design.md` (`W§n`, `AUn`), `docs/specs/2026-11-21-auto-update-design.md` (`U`), `docs/specs/2026-11-21-statusline-hud-design.md` (`H§n`), `docs/specs/2026-11-24-decision-records-design.md` (the DDR rule, P3-8). `K§n` is a section of this document.

Each decision has a mark. **(operator)**: the operator made the decision (the request of 2026-11-25 and the two additions of the same day, quoted in K§1). **(author)**: the author made it; the operator can change it before approval. Code references are `file:line` at commit `bf3726bd` (branch `phase-4`, equal to `main`).

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
- **K-G8** A live zygo session checks for a new zygo release in the background and shows it on the HUD until the running version is current **(operator)**. For each client, exactly one party tells the user about a new client release: the client or zygo **(operator)** (K§6.5).

## 3. Non-goals

- ChatGPT-login passthrough for Codex. The `chatgpt` row of §12.2 stays a v2 row "to be verified" **(author)**.
- The Grok CLI and Pi **(author)**.
- A change to a capability standard that makes a client score higher **(author)**. The split of a standard into criteria (K§6.2.2) restates the standard; it adds no requirement.
- The `POST /openai/v1/chat/completions` client route and the `openai-chat` client pair of §7.6 v2. Codex 0.172.0 accepts only `wire_api = "responses"` (K§8, alternative 7). The route keeps its `404 zygo_route_not_in_v1` **(author)**.
- A change of behavior for Claude Code or OpenCode in P4-4 (K-D3).
- An install during a session. A background check never prompts and never installs (U "Prompt": hooks run the absolute binary path; at `bf3726bd` that is `crates/zygo-launch/src/claude_code.rs:76` (`current_exe`) and `:463`, `:528` (the quoted path in each hook command); U's `:38` is stale).
- A change to the start check of the auto-update design (U). It keeps its prompt, its skips and its 24 h rule.

## 4. Constraints

- **K-C1** C3: the proxy stores and logs no request or response body. The Responses dialect keeps no conversation state on the proxy.
- **K-C2** C5: each pass over a body is O(body size) and bounded. A chained translation (K§6.4.3) is two linear passes.
- **K-C3** C6: a hook never breaks the agent. This holds for Codex hook input.
- **K-C4** C9: a client has a grade for each criterion before its launcher is built. K-D5 enforces this with the compiler and the conformance test.
- **K-C5** Tests never run a real `claude`, `opencode` or `codex` binary. Launch tests use the stub agents. No test uses the operator's OpenAI or ChatGPT login, and no step runs `codex login` **(operator rule, project memory; K-D14)**.
- **K-C6** No CI test measures wall-clock time or has a deadline **(project rule)**.
- **K-C7** P4-4 changes no behavior. The proof is the existing test suites, which pass with no edit except moved paths and imports (K-D3).
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

### 6.1 Client contract (P4-4)

The `file:line` sites below are at `bf3726bd`. P4-4 takes the inventory again at its own base commit (K§6.9).

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

- **Lower layer** `crates/zygo-core/src/client/`. `mod.rs` holds `Agent` (moved from `lib.rs:20-53`; `zygo_core::Agent` stays as a re-export), `Agent::ALL`, `Agent::spec() -> &'static ClientSpec`, and `FromStr`/`as_str` built from the specs. Each `<client>.rs` holds one `ClientSpec` constant and the pure functions it points to. These functions take and give `serde_json::Value`, `&str` and `&[String]` only, so `zygo-core` needs no new dependency.
- **Upper layer** `crates/zygo-launch/src/client/`. `mod.rs` holds `adapter_for(agent, zygo) -> Box<dyn AgentAdapter>`. Each `<client>/` folder holds that client's adapter, moved from `claude_code.rs` and `opencode.rs`, and its assets (`assets/opencode-plugin.js` moves to `client/opencode/`).
- **CLI shim** `crates/zygo/src/cmd/client/<client>.rs`, moved from `cmd/claude.rs` and `cmd/opencode.rs`. It is the clap options struct and its conversion to `LaunchOpts`. It stays in the binary crate because clap is a dependency of that crate only (`crates/zygo/Cargo.toml:24`).

`Dialect` (`crates/zygo-config/src/schema.rs:182-188`) moves to `zygo-core`, and `zygo-config` re-exports it with the same serde names, so a descriptor can name its proxy dialect **(author)**.

`Agent` keeps its name. A rename to `Client` would touch each of the 35 files and the serialized `ZYGO_AGENT` value (§8.4) for no gain **(author)**.

#### 6.1.2 The descriptor (`ClientSpec`)

The descriptor answers the concerns that the trait does not cover today. Each concern below replaces the cited branches. Field names are left to the builder (K§10); the content is decided.

| # | Concern | Descriptor content | Replaces |
|---|---|---|---|
| 1 | Identity and updates | id (`claude-code`), display name, program name, CLI verb, install hint, minimum version (= the pinned version of the grade file), `supported` flag (K-D9), `announces_own_updates` fact and release source (K§6.5.4), status surface (`statusLine` command, or none) | `lib.rs:28-53`, `builtin.rs:50-55` and `data/agents.toml`, `member.rs:688-691`, `config.rs:82-86`, `doctor.rs:192` |
| 2 | Hook input | a function that turns the client's hook JSON into the common form (Claude Code: identity) | `hook.rs:23`, `hook_opencode.rs` |
| 3 | Hook output | a function that renders a common reply (context text, deny, block) in the client's shape | `session_start.rs:42-49`, `workspace.rs:630-643` |
| 4 | Session text delivery | the mode (at session start by hook; per request by plugin; at launch in config), the model alias table for the alias sentence (§8.6 item 5), whether a prompt can be blocked while stopping | `session_text.rs:83`, `workspace.rs:290`, `:621` |
| 5 | MCP tool naming | a function from a model-visible tool name to (server, tool); whether the `compact` tool is listed | `zygo-work/src/hooks.rs:369-373`, `zygo-mcp/src/tools/mod.rs:21` |
| 6 | Plugin delivery form | the form name only; the delivery itself is an adapter method (K§6.1.3) | `launcher.rs:519-547`, `snapshot.rs:264-266`, `adapter.rs:74-77` |
| 7 | Argument grammar and session flags | the flag table, the subcommand rule, the session-flag names, the insertion point of session arguments, the "bare" rule | `args.rs:356-360`, `:402-406`, `:507`, `:595-611`, `:652-656`, `launcher.rs:733-736`, `member.rs:189`, `:509-514` |
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

#### 6.1.4 The boundary check

A Rust test, `crates/zygo-core/tests/client_boundary.rs`, scans `crates/*/src/**/*.rs` **(author)**. It fails on each of these outside the allowlist:

- a path `Agent::<Variant>` (a match arm, a comparison, a `matches!`, a construction);
- a string literal equal to a client id (`"claude-code"`, `"opencode"`, `"codex"`).

The allowlist is: `crates/zygo-core/src/client/`, `crates/zygo-launch/src/client/`, `crates/zygo/src/cmd/client/`, and the one `Cmd` enum and its dispatch in `crates/zygo/src/cmd/mod.rs` (one variant per client is the CLI surface). The scan of a file stops at its first line `#[cfg(test)]` that a `mod` line follows (the repository's form for inline test modules, for example `crates/zygo/src/cmd/member.rs:958`). Files under `crates/*/tests/` are not scanned. The test lists each violation as `file:line`.

### 6.2 Conformance: registry, grades, score, bar (P4-1)

#### 6.2.1 Files and owners

| File | Content | Owner (the one writer) |
|---|---|---|
| `docs/conformance/capabilities.toml` | the capability registry: id, name, standard text, tier, criteria | edited by hand in a PR; a change of standard text or tier stops for the operator (DDR class, P3-8) |
| `docs/conformance/<client-id>.toml` | one grade file per client: pinned version, one grade per criterion, one documentation URL per capability, bar exceptions | edited by hand in the PR that changes the client |
| `docs/conformance.md` | the generated matrix, scores and bar | written only by the generator (K-D10); a hand edit fails the check |

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
| A-1 Transport | core | .1 Each model request goes to the proxy at the client dialect's prefix. .2 The credential is the session token, got through the client's key helper; the token is in no argument, no environment row and no file except the session's `token` file (and the `0600` passthrough files of §12.3). .3 The client's own login and provider selection are off: no model request goes to a vendor host and no login prompt shows (passthrough excepted). .4 The key-helper command runs in each shell that the client uses on Linux, macOS and Windows. |
| A-2 Attribution | core | .1 `X-Zygo-Session` on each request. .2 `X-Zygo-Agent-Version` on each request. .3 `X-Zygo-Repo` on each request when the session has a repository id. .4 `X-Zygo-Work` on each request when a work item is set. |
| A-3 Model list | standard | .1 The picker shows the session's role list. .2 The default model is a role. .3 Each model alias of the client resolves to a role. |
| A-4 Subagent roles | standard | .1 Each built-in subagent maps to a role. .2 One `zygo-<role>` subagent exists per enabled role, with model `<role>`. |
| A-5 Per-session config | core | .1 Each zygo setting reaches the client through a per-session mechanism. .2 zygo writes no file of the user's own client config. .3 Each setting that zygo writes is in effect: no user or project file overrides it. |
| A-6 Local services | core | .1 `zygo mcp` is registered as a stdio MCP server and starts in the session. |
| A-7 Result trimming | standard | .1 Shell tool output over the threshold reaches the model as the §10.1 view. .2 File-read text output over the threshold, the same. .3 Search and listing tool output, the same. .4 Web-fetch output, the same (`n/a` for a client with no such tool). .5 The view takes the place of the tool's output in the tool's own result, not of an error or a feedback message. |
| A-8 Session context | core | .1 The §8.6 session text reaches the main loop at each session start (new, cleared, resumed). .2 The mechanics text (§8.6 item 7) reaches each `zygo-<role>` subagent. |
| A-9 Status | extra | .1 to .5: the profile, the work item, the context use, the compaction override and the session savings show on the client's status surface, or in `zygo status` when the client has no surface (the OpenCode precedent, `docs/conformance.md` evidence for A-9). .6 Each status command that zygo writes runs in each shell the client uses. |
| A-10 Secrets hygiene | core | .1 The §8.4 denylist, with the client's rows, is applied to the client's environment. .2 The session token is not in the client's environment. |
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
| A-22 Update notices (new, K-D5 union rule: K§6.5; added to the registry by P4-7, with the feature) | extra | .1 A newer zygo release that the background check found shows on the client's status surface, or in `zygo status` and the launcher's exit line when the client has no surface. .2 The descriptor's `announces_own_updates` fact is measured by a premise under a zygo launch. .3 A newer client release reaches the user from exactly one party: the client (fact true) or zygo (flag on). |

A-22 is `extra`, not part of A-13 (core) **(author)**: the operator named A-13 as an example, but a notice is not needed to run a session, and a core criterion that waits for a live check would put both shipped clients below the bar.

#### 6.2.3 Grade file form

```toml
client = "codex"                     # an Agent id
name = "Codex"                       # the display name in docs/premises.md and docs/live-checks.md headers
version = "0.172.0"                  # the pinned minimum

[[cell]]
capability = "A-1"
url = "https://…"                    # the client's documentation page for the feature

[[grade]]
criterion = "A-1.1"
status = "met"
evidence = ["premise:PC1", "fixture:fixtures/codex/requests/pc1-0001.json",
            "test:crates/zygo-launch/tests/stub_launch.rs::codex_base_url"]

[[grade]]
criterion = "A-15.1"
status = "not-met"
reason = "tool hooks carry no agent id, so the main loop cannot be told from a subagent"

[[bar_exception]]
criterion = "A-1.4"
reason = "…"
approved = "operator 2026-11-25, docs/specs/2026-11-25-client-model-design.md K-D18"
```

Evidence kinds **(author)**:

- `test:<path>::<fn>`: the file exists under `crates/` and holds `fn <fn>(` with `#[test]` or `#[tokio::test]` within the 3 lines above it. The path makes the check exact and cheap.
- `fixture:<path>`: a regular file under `fixtures/`, not a symbolic link, more than 2 bytes and not `{}` or `[]` (the `nontrivial` rule of `scripts/check-premises.sh:20-27`).
- `premise:<id>`: exactly one row `| <id> |` in `docs/premises.md` or `docs/live-checks.md`, with result `verified`.

#### 6.2.4 Derived values

- **Cell (K-D7).** All applicable criteria met: `full`. Some met: `partial`, and the text of each unmet criterion is the "what is missing". None met: `none`, and the reasons are the "why". All criteria `n/a`: the cell is `n/a` and leaves the score.
- **Score (K-D8).** Weight: core 3, standard 2, extra 1. Cell value = met ÷ applicable criteria. Score = Σ(weight × cell value) ÷ Σ(weight), over cells with one applicable criterion or more, as a percent rounded down to an integer. The same formula per tier gives the breakdown.
- **Bar (K-D9).** A client meets the bar when each criterion of each core capability is `met`, or is `not-met` with a `bar_exception`. A core criterion is never `n/a`. A **failing core capability** is a core capability with a criterion that is `not-met` and has no exception.
- **Rollup.** For each client, for each tier and for the total: the count of `full`, `partial`, `none` and `n/a` cells; the count of criteria `met` and of criteria applicable (not `n/a`); and the bar verdict, `meets bar` or `below bar` with the id of each failing core capability.

#### 6.2.5 The check and the generator

One Rust integration test, `crates/zygo-core/tests/conformance.rs`, is the check and the generator **(author)**. `zygo-core` gets `toml` as a dev-dependency only. It runs in `cargo test --workspace`, so on the three CI systems (`.github/workflows/ci.yml:34-45`) and in the release job (`release.yml:30`), with no new CI step. The test fails when:

1. a variant of `Agent::ALL` has no grade file, or a grade file names no variant;
2. a grade file lacks a grade for a criterion of the registry, or has a grade for an unknown criterion, or has one twice (K-D5);
3. a `met` grade has no evidence, or an evidence item is not found (K§6.2.3);
4. a `not-met` or `n/a` grade has no reason, or a core criterion is `n/a`;
5. a cell lacks an `https://` URL;
6. a `bar_exception` names a criterion that is `met`, or a criterion of a non-core capability, or has no `approved` text;
7. the grade file `version` differs from the client's built-in minimum version (`crates/zygo-config/data/agents.toml` until P4-4 moves it into the descriptor), from the `<name>: <version>` header of `docs/premises.md`, or from the `<name>: <version> or later` header of `docs/live-checks.md`;
8. a supported client does not meet the bar (K-D9). Until P4-4 adds the descriptor's `supported` flag, each variant of `Agent::ALL` counts as supported (each has a CLI subcommand today);
9. a fixtures folder `fixtures/<client-id>/` is missing or empty;
10. `docs/conformance.md` or `docs/conformance.json` differs from the generated text. Line ends are compared after CR LF becomes LF.

With `ZYGO_WRITE_CONFORMANCE=1` in its environment, the test writes both files and passes item 10. The Markdown file starts with a line that names the generator and says that a hand edit fails the check; the JSON file has the same text in a `generated_by` key.

Input rules for the files, which the test treats as hostile **(author)**: TOML with unknown keys refused; an id that does not match `^(A|WA)-[0-9]+(\.[0-9]+)?$` refused; an evidence path that is absolute, holds `..`, or leaves the repository after link resolution refused; a reason or URL with a line break, or over 300 characters, refused; the generator escapes `|` in each text it puts in a table.

`scripts/check-conformance.sh` and `scripts/test-check-conformance.sh` are deleted. `scripts/check-premises.sh` reads the client names and versions from the grade files (the `name =` and `version =` lines) in place of `:32-44` and the `case` at `:82` **(author)**.

#### 6.2.6 The three outputs: feature matrix, rollup, score

The generator gives three named outputs **(operator)**, each from the K§6.2.4 rules. Each is in both files.

1. **Feature matrix.** Capability × client. One row per capability, grouped by tier (core, then standard, then extra), registry order inside a tier. One column per client, in `Agent::ALL` order. A cell is the derived grade and the URL: `full <url>`; `partial (missing: <id> <text>; …) <url>`; `none (<id>: <reason>; …) <url>`; `n/a (<reason>)`. Rows per capability keep the table readable as clients are added (today's form has one row per client and 22 columns).
2. **Rollup.** One table per client, with one row per tier and one total row: full, partial, none, n/a, criteria met / applicable. Under it, the bar verdict: `meets bar` or `below bar: <failing core capability ids>`; the bar exceptions with reason and approval; and the release state (`supported`, `meets bar, not released`, `below bar`).
3. **Score.** One table: client, total score, core, standard and extra sub-scores, each a percent by K-D8.

After the three outputs, `docs/conformance.md` lists, per client, each unmet criterion with its reason, each `n/a` with its reason, and the evidence of each met criterion.

**Machine-readable form** **(author)**: `docs/conformance.json`, written by the same generator and checked by the same test (item 10). CI and later tools read it, not the Markdown. Its form:

```json
{
  "generated_by": "crates/zygo-core/tests/conformance.rs (ZYGO_WRITE_CONFORMANCE=1); do not edit",
  "tiers": {"core": 3, "standard": 2, "extra": 1},
  "clients": [{
    "id": "codex", "name": "Codex", "version": "0.172.0", "supported": false,
    "matrix": {"A-1": {"tier": "core", "grade": "partial", "url": "https://…",
                        "missing": [{"criterion": "A-1.4", "text": "…", "reason": "…"}], "na": []}},
    "rollup": {"core":  {"full": 7, "partial": 1, "none": 0, "na": 0, "met": 21, "applicable": 22},
               "standard": {…}, "extra": {…}, "total": {…},
               "bar": {"verdict": "below bar", "failing": ["A-1"], "exceptions": []},
               "release": "below bar"},
    "score": {"total": 74, "core": 95, "standard": 61, "extra": 50}
  }]
}
```

The JSON file is a repository file, not a `zygo` command output, because the grade files are not in the binary and the data changes only with a commit **(author)**. Keys are written in a fixed order, so the file is byte-stable.

#### 6.2.7 Release gate for a client below the bar (P4-4)

A client with `supported = false` in its descriptor **(author)**:

- is hidden from `zygo --help`, and its subcommand exits 2 with `zygo <verb>: <name> is below the conformance bar; see docs/conformance.md` unless `ZYGO_PREVIEW_CLIENT=<client-id>` is in the environment;
- is not listed by `zygo doctor` (§8.1 A-13 report), by `docs/install.md`, or by the installers.

Check item 8 forbids `supported = true` below the bar. A client that meets the bar can stay unreleased; the document shows `meets bar, not released`.

### 6.3 Codex client (P4-2, P4-5, P4-6)

#### 6.3.1 Launch: `zygo codex`

`zygo codex [--profile <p>] [--server <url>] [--compact <w>] [--yolo] [-- <codex args>]` runs the §8.2 sequence. The adapter adds this **(author unless marked)**:

1. **Preflight.** It reads the project Codex files (`.codex/config.toml` and each hook file that PC4 names, from the working directory up to the repository root) by the safe-read rule of §8.4. It records each hook that zygo did not write, for K-D13.
2. **Per-session `CODEX_HOME`** = `$ZYGO_SESSION_DIR/codex-home/` (`0700`) **(K-D11)**. It holds:
   - `config.toml`: `model_provider = "zygo"`; `[model_providers.zygo]` with `wire_api = "responses"`, `base_url = "<proxy>/openai/v1"`, `auth.command` / `auth.args` = `zygo hook token` (the key helper; `ModelProviderAuthInfo` in the pinned schema), `http_headers` with the A-2 headers, `supports_websockets = false`, and `capabilities.remote_compaction = false` (so Codex compacts by a normal model request); `model = "<main_role>"`; `model_context_window` and `model_auto_compact_token_limit` (§10.2.5 rule); `[mcp_servers.zygo]` = `zygo mcp --agent codex`; `shell_environment_policy` that keeps the §8.4 denylist out of tool shells; `developer_instructions` (K§6.3.3); `compact_prompt` (A-18); the status-line items (A-9); the skills paths of the plugin snapshot (A-11). zygo does not set `check_for_update_on_startup`, so Codex keeps its own update notice (K§6.5.4);
   - `agents/zygo-<role>.toml`: one per enabled role except `main_role`, with `model = "<role>"`, the description, and `developer_instructions` = role prompt + one space + the mechanics text (§8.5 rule); and the overrides of the built-in roles: `explorer` → `read`, `worker` → `code`, `default` → `code` (§8.5: a subagent with no model is `code`);
   - the model catalog file (A-3) with one entry per role;
   - `hooks.json` (K§6.3.2), and the trust records of K-D13 when PC4 allows them;
   - `sessions`: a link (a symbolic link; a directory junction on Windows) to the shared store `<state_dir>/codex/sessions/` (K-D22).
3. **Precedence.** The keys that route traffic (`model_provider`, the `model_providers.zygo` table, `mcp_servers.zygo`) are also passed as `-c key=value` arguments, so a project `.codex/config.toml` cannot outrank them (PC3) (K-D23). No argument holds the token.
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

1. When PC4 shows that trust records written into the per-session `CODEX_HOME` before launch make Codex run zygo's hooks, the adapter writes one record for each zygo hook command, for that command's hash only.
2. Else, when the preflight found no hook that zygo did not write (no project hook file, no hook in a project `.codex/config.toml`), the adapter passes `--dangerously-bypass-hook-trust`.
3. Else the adapter writes no hooks. It prints one line: `codex: zygo hooks off: the project has its own Codex hooks`. The cells that need hooks (A-7, A-8.1 at start, A-15, A-19, WA-1, WA-2) are graded from what remains; the grade file states the branch.

The grade file grades branch 1 or 2 as the normal launch. Branch 3 is a known limit in the generated document.

#### 6.3.7 Premises (P4-2, K-D14)

Each provisional Codex grade becomes measured by a premise row in `docs/premises.md` (`PCn`) with fixtures under `fixtures/codex/`. The method is the existing one (`docs/premises.md` "Method"): Codex runs under `env -i` with a temporary `HOME` and `CODEX_HOME`, a dummy key built at run time, and `base_url` = `scripts/premises/mock_upstream.py` directly, with no zygo between. `mock_upstream.py` gains a `/v1/responses` SSE answer and a `/v1/models` answer. `HTTPS_PROXY` points at the mock, which refuses and records each `CONNECT`, so a request to any other host is seen and fails. No run uses an OpenAI or ChatGPT login. No step runs `codex login`.

| Id | What it measures | Feeds |
|---|---|---|
| PC1 | The provider table routes each model request to `base_url` with the key-helper bearer; the paths Codex calls; each other host it tries (with `CONNECT` records); processes left after exit | A-1, A-10 |
| PC2 | `http_headers` and `env_http_headers` reach each request | A-2 |
| PC3 | `CODEX_HOME` replaces `~/.codex`; `-c` outranks a project `.codex/config.toml` for the K§6.3.1 step-3 keys | A-5 |
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
| PC16 | Under a zygo launch, `check_for_update_on_startup` is in effect at its default (`true`) and Codex requests its release source at start (a `CONNECT` record); the owner-run live check LC-CX1 sees the notice text against a real newer release | A-22.2, K§6.5.4 |

`docs/premises.md` and `docs/live-checks.md` get the header line `Codex: 0.172.0` (and `Codex: 0.172.0 or later`). The premise test (`crates/zygo-launch/tests/premise.rs`) gains a Codex part: each key zygo writes is in `fixtures/codex/config.schema.json` (from the pinned schema, `rust-v0.172.0` tag), each flag in `fixtures/codex/help.txt`. The premise harness also runs the generated config once with `--strict-config`. The launch does not pass `--strict-config`, because a project config with a key newer than the pin would then stop the launch **(author)**.

#### 6.3.8 Provisional Codex grades

From research on Codex CLI 0.172.0 (its `--help`, `codex features list`, the pinned config schema and the vendor configuration, hooks and subagent pages). Not measured. Each grade is replaced by the PC premises before P4-6.

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
| A-21 | full | stub launch test and premise test (built in P4-5) |
| WA-1 | full or partial | capture, `codex resume <id>`; PC10 decides |
| WA-2 | partial | `Interrupt` gives a turn end on Esc; spawn tool name from PC5 |
| A-22 | partial | no custom status-line text (A-22.1 by `zygo status` and the exit line); `check_for_update_on_startup` defaults to `true` (A-22.2, A-22.3 after PC16) |

### 6.4 Proxy: the Responses client dialect (P4-3, K-D12)

#### 6.4.1 Routes

- `POST /openai/v1/responses`: the model call, streaming and non-streaming.
- `GET /openai/v1/models`: the session's role list in the shape that PC7 records.
- Each other path under `/openai/v1/` that PC1 shows Codex calls: served when its function is in this section, else `404` in the OpenAI error shape. `POST /openai/v1/chat/completions` keeps `404 zygo_route_not_in_v1` (K§3).

Stage 1 is the existing identity rule (`crates/zygo-proxy/src/identity.rs:22-35`): `X-Zygo-Session` and the bearer token. Errors on these routes use the OpenAI shape (`errors.rs:34-41`).

#### 6.4.2 Pipeline

The stages run on the client's body, with `ctx.dialect = Openai`, as they run on an Anthropic body today. Translation is per candidate at forward time, as today (`forward.rs`). Each stage states which (client dialect, upstream dialect) pairs it acts on. A table test covers each pair for each stage. Each stage that cannot act on a pair is a graded criterion, not a hidden gap.

| Stage | Responses client obligation |
|---|---|
| Identity, body limit | unchanged |
| Resolution (§7.4) | `model` is a role, custom role or model id, as for Messages |
| Redaction (§16) | the secret pass on each text; the content pass on `input` (`Layout::Input`), with the Codex harness spans of PC13 skipped |
| Cache markers (§10.4) | toward an Anthropic upstream: on the translated Messages body, by `[cache] insert_markers.codex` (default `true`: Codex sends no Anthropic markers); toward a Responses upstream: `prompt_cache_key` passes |
| Window (§10.2) | the rewrite edits `response.completed` usage when armed (K§6.4.5) |
| Usage (§14) | measured from the upstream stream before any rewrite, as today |
| Attribution (A-2) | unchanged |

#### 6.4.3 Translation pairs

Messages is the pivot **(author, K-D21)**:

- **Responses client → Responses upstream: same dialect, no pivot.** The body goes as received, except: `model` := the candidate's `upstream_model`; `store` := `false`; `include` keeps `reasoning.encrypted_content`; the stage edits. Custom tools (the freeform `apply_patch`) and reasoning items pass natively. A reasoning item reaches only the provider and upstream model that made it; else it is dropped with an adjustment code. The proxy marks each returned reasoning item so it can check this on the next request (encoding left to the builder; the existing `zygo1:` form of §7.6 is the model).
- **Responses client → Anthropic upstream.** New request translator Responses → Messages. New response translator Messages SSE (and JSON) → Responses SSE (and JSON).
- **Responses client → chat upstream.** Responses → Messages (new) → chat (existing `translate/chat.rs`); chat SSE → Messages SSE (existing `translate/sse.rs`) → Responses SSE (new). Two linear passes (K-C2).

Request obligations (Responses → Messages). Exact field mapping is left to the builder; acceptance tests check each line:

1. `instructions` and leading `developer`/`system` items become the system text. A later `developer`/`system` item keeps its place between turns; no developer text is dropped.
2. `message` items map by role; `input_text`, `input_image`, `input_file` map to text, image and document blocks (§7.6 document rule).
3. `function_call` items become `tool_use` blocks (consecutive calls in one assistant message); `function_call_output` items become `tool_result` blocks (consecutive outputs in one user message). `call_id` is kept as the tool-use id.
4. `custom_tool_call` and its output map the same way, with input `{"input": "<text>"}`. A `custom` tool definition becomes a function tool with one string parameter `input`; adjustment code `custom_tool_as_function`.
5. `reasoning` items with a zygo mark decode to `thinking` blocks for the same provider and model; others are dropped with an adjustment code.
6. Hosted tools and their items (web search, local shell, and the like) are dropped with an adjustment code each.
7. `tool_choice`, `parallel_tool_calls`, `max_output_tokens`, `temperature`, `top_p`, `reasoning.effort` map by the §7.6 model facts. `max_tokens` is set from the catalog when the client gives none.
8. `previous_response_id` set, `background: true`, or a `conversation` reference → `400 zygo_bad_request` "zygo keeps no response state; send the full input with store=false" (K-C1, K-D31).
9. An `input` item of an unknown `type`, or a known item with a missing required field → `400 zygo_bad_request` that names the type. It is never dropped in silence.
10. `store`, `include`, `prompt_cache_key`, `metadata`, `service_tier`, `user` are used by the proxy or dropped; they never reach an Anthropic upstream.

Stream obligations (Messages SSE → Responses SSE):

1. The stream starts with `response.created` then `response.in_progress`, with a proxy-made id (`resp_<ULID>`), `model` = the name the client sent, `status = "in_progress"`.
2. Each content block becomes one output item: text → a `message` item with an `output_text` part (`output_item.added`, `content_part.added`, `output_text.delta`…, `output_text.done`, `content_part.done`, `output_item.done`); `tool_use` → a `function_call` item (`output_item.added`, `function_call_arguments.delta`…, `function_call_arguments.done`, `output_item.done` with the whole `arguments` string); `thinking` → a `reasoning` item with a summary and a zygo-marked `encrypted_content` that holds the thinking text and signature.
3. `sequence_number` rises by one per event from 0. `output_index` and item ids are consistent across an item's events.
4. `message_delta` and `message_stop` become one terminal event: `response.completed` with `usage` (`input_tokens` = input + cache read + cache write; `input_tokens_details.cached_tokens` = cache read; `output_tokens`; `total_tokens`), or `response.incomplete` with reason `max_output_tokens` for `stop_reason = "max_tokens"`.
5. An upstream error before or during the stream gives `response.failed` with an error `code` and `message`. Each stream ends with exactly one terminal event.
6. `ping` is dropped. An upstream event of unknown type is dropped. An upstream event whose JSON does not parse ends the stream with `response.failed`.
7. A non-streaming request gets one Response object with the same content.
8. The §10.2.6 bounds hold: a held partial event is bounded by `HOLD_MAX` (`crates/zygo-proxy/src/tee.rs:22`); no buffer grows with the response.

#### 6.4.4 Context-limit errors

An upstream refusal for an over-long prompt reaches a Responses client in the shape that PC14 records, so Codex runs its own recovery. Other upstream errors keep their status and get the OpenAI error shape (§7.6 rule, client side).

#### 6.4.5 Window rewrite for a Responses client

When the descriptor arms the rewrite (K§6.3.4), the tee works in whole SSE lines and rewrites the `usage` of `response.completed`, by the §10.2.2 formula with the Responses fields. Trigger 0 (safety) and trigger 2 (the `compact` tool, matched by the PC9 name in a `function_call` item) act on each request. Trigger 1 (soft window at end of turn) needs a main-loop marker: it is armed only when PC6 finds a request marker for subagent calls; else A-16.1 relies on the launch-time limit.

### 6.5 Update notices (P4-7)

#### 6.5.1 What exists

The start check of U runs before a launch in `zygo claude`, `zygo opencode` and `zygo ws run` (`crates/zygo/src/cmd/update.rs:294-371`). It is skipped in a zygo session, in a workspace pane, off a TTY, with `-p`, with `[update] check = false` or `ZYGO_NO_UPDATE_CHECK`, and within 24 h of the last check (`update.rs:303-322`). It keeps its state in `<cache_dir>/update.json` (`checked_at`, `latest`, `skip`; `update.rs:231-275`). `[update]` is user config only (`crates/zygo-config/src/layers.rs:68`). Nothing checks during a session, so a session that runs for days never learns of a release, and a member session never checks at all.

#### 6.5.2 Background check of zygo's own releases **(operator: background check, HUD post; author: the mechanics)**

- **Where.** One thread in the launcher process. That process lives for the whole session and hosts the in-process proxy, so no new process starts and W-G6 holds. The thread starts once per launcher process, at the one point where `zygo <client>` and `zygo ws run` both pass before `Launcher::run` (the builder picks that one call site). It ends with the process.
- **When.** First tick 60 s after the child starts, then one tick per hour. A tick checks only when the shared throttle allows it (24 h since `checked_at`, the U rule, a clock that went back does not count). So a session that runs for days checks about once a day, and each live session shares one check.
- **Skips.** `[update] check = false` and a non-empty `ZYGO_NO_UPDATE_CHECK`, read at launch. The U skips for a zygo session, a workspace pane, a non-TTY and `-p` do not apply: they exist because the start check prompts, and the background check never prompts. So member sessions check too.
- **The check.** The U check: one GET for the release's public VERSION file, 2 s limit, no system proxy (`update.rs:219-229`). The check function moves out of the binary crate so the launcher can call it (K§9).
- **Result.** The thread writes `checked_at` and, on success, `latest` through the state owner (K§6.5.3). It never prompts and never installs. A failed check writes `checked_at` (as U does) and one session-log line `update check: failed: <reason>`; it shows nothing on the HUD.
- **Start check unchanged.** A later `zygo claude` within 24 h of a background check does not prompt, as U says today. The HUD and the exit line carry the notice in that window.

#### 6.5.3 The one owner of the throttle state

`update.json` gets one owner: a module `zygo_core::update_state` **(author)**. It holds the file's path rule, its read (bounded to 4 KiB; a missing, large or unparsable file is an empty state), its atomic write (the U temporary-file and rename form, `update.rs:251-275`), and a try-lock on `<cache_dir>/update.lock` (`fs4`, already a `zygo-core` dependency). The start check, the background check and every reader use only this module. A writer that cannot take the lock skips that tick; it never waits. New fields are additive: `clients.<client-id>.checked_at` and `clients.<client-id>.latest` (K§6.5.4). The U fields keep their meaning.

#### 6.5.4 New releases of the wrapped clients **(operator: the flag and its default; author: the rest)**

- **Fact.** Each descriptor declares `announces_own_updates`: under a zygo launch, the client shows its own update notice by default. It is a measured premise with a fixture, per client, because a zygo setting could hide the notice. At `bf3726bd` zygo sets no update option for any client (no `autoupdate`, `DISABLE_AUTOUPDATER` or `check_for_update_on_startup` in `crates/zygo-launch/src`; grep). Expected values, not yet measured: Claude Code `true`; OpenCode `true` (its `autoupdate` defaults on); Codex `true` (`check_for_update_on_startup`, "Defaults to `true`", pinned schema; K§6.3.1 keeps it unset). If zygo's launch hides a client's notice, that client's fact is `false`.
- **Flag.** zygo's per-client flag `announce_client_updates` defaults to NOT `announces_own_updates`, so zygo fills a gap and never doubles a notice **(author's reading of the operator)**. The user config overrides it per client: `[update.clients.<client-id>] announce = true | false` (user config only, under the `[update]` table that `layers.rs:68` already drops from team and repository layers) **(author)**.
- **Release source.** A descriptor may declare one release source: one HTTPS URL and the JSON field or text rule that gives the latest version. Proposed, each verified by its client's premise before use: Claude Code `https://registry.npmjs.org/@anthropic-ai/claude-code/latest` (field `version`); OpenCode `https://registry.npmjs.org/opencode-ai/latest` (field `version`); Codex `https://registry.npmjs.org/@openai/codex/latest` (field `version`). A flag that is on for a client with no release source is a `zygo config check` error, and no check runs for that client.
- **Check.** When a client's flag is on, the K§6.5.2 thread also checks that client's release source, with the same tick, the same 24 h throttle (per client, in `clients.<id>`), the same lock, the same opt-outs, the same 2 s limit, no system proxy, no header but a generic `User-Agent`, and a body read bounded to 64 KiB. A version that does not parse as semver is ignored and logged.
- **Notice.** `<client> <new> available (zygo graded <pinned>)`, where `<pinned>` is the descriptor's minimum version (= the grade file's pin, check item 7).
- **Ungraded notice, with no network.** Whatever the flag, when the running client version (the launcher's `agent_version`, recorded in the session view) is newer than the pinned version, zygo can say `<client> <running> ungraded`. This is a local comparison. It shows in `zygo status`, `zygo doctor` and the verbose HUD only; the compact HUD leaves it out, because a pin is a minimum and most users run a newer version, so a permanent compact segment would be noise **(author)**.

#### 6.5.5 Where the notice shows (per client, through the contract)

The status surface is a descriptor fact (concern 1); A-9 and A-22 grade it.

| Surface | zygo update | client update (flag on) | ungraded |
|---|---|---|---|
| HUD (Claude Code `statusLine` → `zygo hook statusline`, `crates/zygo-launch/src/claude_code.rs:534`) | segment 16, compact `update <new>`, verbose `update <new> — run zygo update` | segment 16, `<client> <new> available (zygo graded <pinned>)` | verbose only |
| `zygo status` (each client; the only in-session surface for OpenCode and Codex) | one line | one line | one line |
| Launcher exit line (each client, when stderr is a TTY) | `zygo <new> is available (you have <cur>); run zygo update` | `<client> <new> is available (zygo graded <pinned>)` | none |

- **HUD segment 16** (an addition to H§6.2) **(author)**: after segment 15, color `33`, escaped (H-C6). It shows while `update.json` `latest` is newer than the running zygo version and is not the `skip` version. "Running" is the launcher's version, which the launcher writes into the session view as an additive field; a hook binary replaced by `zygo update` in another terminal still compares against the session that runs. Under width pressure it is the first segment removed (H§6.7 lists gain it first). The hook reads `update.json` through `zygo_core::update_state` (a bounded file read, no network), inside the H-C2 deadline of 3 s, read on each call like the trimmed count (H§7.7).
- **Codex.** Codex 0.172.0 shows only built-in status-line items, and its status-line provider is experimental (`codex features list`: `tui_extensions_statusline experimental false`). So a Codex user sees zygo's notice in `zygo status` and in the exit line. The session text is not used: it is model context, and an update notice is for the user (§8.6, C7 spirit) **(author)**.

#### 6.5.6 Base-spec amendment (§3)

§3 "Nothing phones home" gets a wider exception: "… and, for each client whose `announce_client_updates` flag is on, one GET to that client's release source named in its descriptor (Claude Code, OpenCode and Codex: the npm registry `latest` document of their package), sending nothing else; the same opt-outs apply." With the expected default facts (all `true`), every flag is off, and no client check runs until a user turns one on.

### 6.6 Owners of state

| State | Owner | Created | Removed |
|---|---|---|---|
| Capability registry | the registry file; changed by PR; standard or tier change stops for the operator | P4-1 | never |
| Grade file per client | the client's PRs | with the client's `Agent` variant | with the variant |
| `docs/conformance.md`, `docs/conformance.json` | the conformance test (generator) | `ZYGO_WRITE_CONFORMANCE=1` run | never by hand |
| Per-session `CODEX_HOME` | the Codex adapter | at launch, in the session folder | with the session folder, by the §10.1 sweep |
| Codex hook trust records | the Codex adapter, inside `CODEX_HOME` | at launch | with `CODEX_HOME` |
| Shared Codex session store `<state_dir>/codex/sessions/` | Codex writes it; zygo makes the folder and the link | first `zygo codex` launch | never by zygo (the user's conversations, as `~/.codex/sessions`) |
| Codex session id of a member | the member record (existing `agent_session`) | `SessionStart` hook | `ws forget`, `ws forget-pane` |
| `<cache_dir>/update.json` and `update.lock` (throttle, `latest`, `skip`, per-client results) | `zygo_core::update_state` (K§6.5.3); writers: the start check and the background check | first check | never (a cache file; deleting it only resets the throttle) |
| Running zygo version and client version of a session | the launcher, in the session view (additive fields) | at launch | with the session folder |

### 6.7 Hostile input at the new boundaries

- **Responses requests.** The body limit and the idle bound of `read_body` (`crates/zygo-proxy/src/routes.rs:74-76`) apply. `serde_json` depth limit applies. Unknown item types and missing fields fail closed (K§6.4.3 item 9). A zygo-marked reasoning value from the client is untrusted: it must decode, fit a size bound, and name a provider and model of the session's candidates, else it is dropped with an adjustment code; it never selects a provider. `previous_response_id` is refused. Header values other than identity are attribution only.
- **Upstream streams.** Bounded holds; unknown events dropped; unparsable events end the stream with `response.failed`.
- **Hook input from Codex.** Read under the existing hook bounds. A body that is not a JSON object, an unknown event, or an unknown tool name gives no output (C6). Paths in tool input go through the existing guard rules.
- **Project Codex files.** Read by the §8.4 safe-read rule at preflight. Their content only decides K-D13; zygo never copies them.
- **Grade and registry files.** K§6.2.5 input rules.
- **`update.json`.** A local file that another process may write. Read bounded to 4 KiB; a file that is large, not JSON, or holds a version that is not semver is an empty state. The HUD escapes each value (H-C6).
- **Release sources.** Each answer is untrusted: 2 s limit, body bounded to 64 KiB, the version must parse as semver (the U `VERSION_RULE`, `update.rs:13`), else the answer is ignored and logged. A source answer never selects a download; `zygo update` keeps its own checksum path (U).

### 6.8 Flows walked

1. **`zygo codex` launch.** CLI shim → preview gate (K§6.2.7) → §8.2 steps 1-2 (config, profile, proxy session, session folder, `token` and `url` files) → `adapter_for(Agent::Codex)` → preflight (project files, trust branch) → build (`CODEX_HOME`, `-c` keys, environment) → plugin delivery (skills paths) → run `codex` → exit (session end, resume line).
2. **A streamed request to an Anthropic upstream.** Codex runs `zygo hook token` (key helper), sends `POST <proxy>/openai/v1/responses` with the bearer and `X-Zygo-*` headers → identity → body read → resolution of `model` (a role) → redaction on `input` → candidates → forward: Responses → Messages, cache markers, `POST /v1/messages` → the tee measures usage from the Messages SSE → encoder Messages SSE → Responses SSE (rewrite of `response.completed` when armed) → Codex.
3. **A hook call.** Codex runs `zygo hook <name> --agent codex` with JSON on stdin → `crates/zygo/src/cmd/hook.rs` takes the agent → `HookInput::parse(Agent::Codex, …)` calls the descriptor's input function → the common handler → the descriptor's output function → stdout. Any fault: one stderr line, exit 0 (`hook.rs:77-92`, C6).
4. **Resume.** Plain: `zygo codex -- resume <id>` → a new zygo session and `CODEX_HOME` whose `sessions` links to the shared store → Codex finds the rollout. Member: `zygo up` → `ws run` → `adapter_for` → `has_session_arg` false → `resume_args(id)` from the member record → the same path.
5. **Background update notice.** `zygo claude` (or `ws run`) starts the child → the update thread waits 60 s → tick: opt-outs off, `update_state` try-lock taken, 24 h passed → one GET (2 s) → `update.json` written → the next `zygo hook statusline` run reads `update.json` and the session view, sees `latest` > running version and not `skip` → segment 16 → at child exit the launcher prints the exit line. A Codex or OpenCode user runs `zygo status` and sees the same line. A failed GET: one session-log line, no segment.
6. **A fourth client, from this document only.** (a) Add the variant and `client/<id>.rs` in `zygo-core`; the exhaustive match in `Agent::spec()` fails to compile until the descriptor exists. (b) Add `client/<id>/` in `zygo-launch`; `adapter_for` fails to compile until the adapter exists. (c) Add `cmd/client/<id>.rs` and the `Cmd` variant. (d) `cargo test` fails: no grade file, no fixtures folder, version headers missing (K§6.2.5 items 1, 7, 9). (e) Write premises and fixtures, then the grade file with evidence. (f) Run the generator. (g) Set `supported = true` only when the bar is met. The boundary test fails if any step put a branch outside the client modules.

### 6.9 Order against phase 3 **(author; input from the lead, 2026-11-25)**

Phase 3 still runs on `origin/phase-3` and is not in `main`; `phase-4` branched from `main` (`bf3726bd`). Phase 3 changes many of the client-branch sites of K§1 (`session_start.rs`, the status-line `render.rs` and `phase.rs`, `opencode-plugin.js`, `cmd/member.rs`, the workspace hooks, the Claude Code launch, the trim hook per client). It does not change `crates/zygo-proxy/src` (only `tests/serve.rs`), `scripts/check-conformance.sh`, `scripts/check-premises.sh` or `docs/conformance.md`; it adds a section to `docs/live-checks.md`.

- **Starts at approval, in parallel with phase 3, based on `phase-4`:** P4-1 (registry, rubric, grade files, generator, three outputs, bar), P4-2 (Codex premises and fixtures), P4-3 (proxy Responses client dialect). P4-1 needs no client contract: it reads the client list from `Agent::ALL`, the minimum versions from `crates/zygo-config/data/agents.toml`, and counts each variant as supported (K§6.2.5 items 7, 8). P4-2 changes `docs/live-checks.md` only by its own rows and header line.
- **Waits until phase 3 merges into `main` and `phase-4` takes it:** P4-4 (client contract), then P4-5 and P4-6 (Codex client and grade), and P4-7 (update notices: it shares the status-line segment order and drop lists with phase 3's HUD work).
- **The inventory of K§1 is evidence of the pattern, not the work list.** P4-4 starts by taking the inventory again at its base commit (the grep of K§1 item 2), because phase 3 adds client-specific branches. Each site found is moved behind the contract or named in the P4-4 plan with its concern.

---

## 7. Decisions

| Id | Decision | Mark | Reason |
|---|---|---|---|
| K-D0 | Add Codex support; a clear client model; a maintained feature-matrix conformance score; each client graded on each feature any client supports; a rubric; measured, scored, "found wanting, but sufficient" | operator | the request of 2026-11-25 |
| K-D1 | One client contract in two layers (K§6.1.1); the 11 concerns of K§6.1.2 plus the adapter of K§6.1.3; the boundary test of K§6.1.4 | author | the crate direction forces two layers; the test keeps branches from returning |
| K-D2 | A client = a descriptor module, an adapter folder, a CLI shim, a fixtures folder, a grade file. §8.8 is superseded by K§6.8 flow 6 | author | the compiler and the tests list each missing piece; §8.8 missed three pieces |
| K-D3 | P4-4 moves Claude Code and OpenCode onto the contract with no behavior change; the existing tests pass with only path and import edits | author | a refactor proven by unchanged tests |
| K-D4 | One registry file with criteria (K§6.2.2) | author | machine-checkable rubric |
| K-D5 | Union rule: a zygo feature that any client supports is a capability with a rubric, and each client gets a grade; a client with a missing grade fails the check. A new descriptor concern or adapter method also needs a capability that names it | operator intent; mechanics author | "all clients get graded" |
| K-D6 | Per-client grade file with measured evidence (K§6.2.3) | author | evidence that a test can find |
| K-D7 | Cell grade derived from criteria | author | no free-text grade |
| K-D8 | Weighted score: core 3, standard 2, extra 1; rounded down | author | a transport gap must weigh more than a status gap; rounding down never inflates |
| K-D9 | Bar: each core criterion met (or excepted). Core set: A-1, A-2, A-5, A-6, A-8, A-10, A-13, A-21. A-2 is added to the brief's set because the proxy refuses a request with no `X-Zygo-Session` (`crates/zygo-proxy/src/identity.rs:31`). Enforcement: the `supported` flag and check item 8 (K§6.2.7) | author | the simplest gate that the existing `cargo test` runs; no build features |
| K-D10 | The generator and check is one Rust test (K§6.2.5); it replaces `check-conformance.sh` and the hardcoded versions | author | TOML in shell is fragile; the scripts run in no CI job today |
| K-D11 | `zygo codex` with a per-session `CODEX_HOME` (K§6.3.1) | author | A-5 needs a per-session mechanism; hooks and agents need files |
| K-D12 | The proxy serves the Responses API as a client dialect (K§6.4) | author | Codex speaks only Responses |
| K-D13 | Hook trust order (K§6.3.6) | author | zygo never trusts a hook it did not write |
| K-D14 | Each provisional Codex grade is measured by a premise against the mock; no login; minimum 0.172.0 | author | measured grades |
| K-D15 | Non-goals of K§3 | author | scope |
| K-D16 | Codex ships when it meets the bar; workspace support graded | author | "found wanting, but sufficient" |
| K-D17 | `Dialect` moves to `zygo-core`, re-exported by `zygo-config` | author | the descriptor names the proxy dialect; core is the only crate below the proxy |
| K-D18 | Bar exceptions: a `not-met` core criterion with an operator-approved exception counts for the bar, not for the score. Approval of this spec approves one: Claude Code A-1.4 (Windows PowerShell: `docs/conformance.md` "Windows shell for the generated commands") | author; the operator approves with this spec | Claude Code ships today with that gap; a strict bar would mark it unsupported; an exception keeps the gap visible |
| K-D19 | `supported` flag and preview gate (K§6.2.7) | author | a below-bar client can be built and tested before release |
| K-D20 | The CLI shim stays in the `zygo` crate under `cmd/client/` | author | clap is a dependency of the binary crate only |
| K-D21 | Messages is the translation pivot; same-dialect pairs pass through | author | two new translators, not four; native custom tools and reasoning to OpenAI upstreams |
| K-D22 | Codex rollouts go to one shared zygo-owned store through a link in each `CODEX_HOME`; when PC10 fails, WA-1.2 is `not-met` and zygo prints no resume line | author | a resume needs the rollout in the new session's `CODEX_HOME` |
| K-D23 | Routing keys are also passed by `-c` | author | a project config must not outrank the transport (A-5.3) |
| K-D24 | No lead guard on Codex while hooks cannot tell the main loop from a subagent | author | a guard that refuses subagent edits breaks §8.3 |
| K-D25 | Codex premises run against `mock_upstream.py` directly, as the existing method | author | the premises need no proxy work, so P4-2 runs before P4-3; it tests the client, not zygo |
| K-D26 | P4-1 grades the criteria as written; each derived cell that differs from today's hand grade is listed in the PR with its reason, and that PR stops for the operator | author | the new rubric may raise or lower a grade; the operator sees each change |
| K-D27 | Client environment denylist rows are a descriptor concern; Codex denies `CODEX_*` for its own child only | author | removing `CODEX_*` from a Claude Code child would change its behavior |
| K-D28 | Evidence `test:` items name the file and the function | author | an exact, cheap check |
| K-D29 | The machine-readable output is `docs/conformance.json`, a generated repository file | author | the grade files are not in the binary; the data changes only with a commit; a file needs no command |
| K-D30 | The generated matrix has one row per capability and one column per client | author | the table stays readable as clients are added |
| K-D31 | `previous_response_id`, `background` and `conversation` are refused | author | C3: the proxy keeps no response state |
| K-D32 | The user's `~/.codex` is not read in a zygo session | author | user hooks and MCP servers would load with their own trust; A-5 |
| K-D33 | The generator gives a feature matrix, a rollup and a score per client (K§6.2.6) | operator | the addition of 2026-11-25 |
| K-D34 | A background check during a session; a HUD post when an update exists | operator | the addition of 2026-11-25 |
| K-D35 | The background check is a thread in the launcher process, first tick at 60 s, then hourly, gated by the shared 24 h throttle; no prompt, no install | author | the launcher lives for the session (W-G6 holds); hooks run the absolute binary path, so an install mid-session would mix versions |
| K-D36 | `zygo_core::update_state` is the one owner of `update.json`, with a try-lock | author | the start check, the background check and the HUD read one state; concurrent sessions never wait |
| K-D37 | The background check ignores the U skips for a session, a pane, a non-TTY and `-p`; it keeps the two opt-outs | author | those skips exist because the start check prompts; member sessions must also learn of updates |
| K-D38 | A per-client `announce_client_updates` flag in the contract; default = NOT `announces_own_updates` | operator (the flag and the default rule); author (the reading "fill the gap, never double") | the addition on client updates |
| K-D39 | `[update.clients.<id>] announce` overrides the flag, user config only | author | the `[update]` table is already user-only (`layers.rs:68`) |
| K-D40 | `announces_own_updates` is a measured premise per client under a zygo launch; zygo sets no client update option | author | a zygo setting could hide the notice |
| K-D41 | `<client> <running> ungraded` from a local comparison only; verbose HUD, `zygo status`, `zygo doctor`; not the compact HUD | author | no network; a pin is a minimum, so the compact HUD would show it nearly always |
| K-D42 | Update notices are capability A-22 (extra), not criteria of A-13 (core) | author | a notice is not needed to run a session; a core criterion waiting on a live check would put the shipped clients below the bar |
| K-D43 | Codex and OpenCode users see the notice in `zygo status` and the launcher exit line, never in the session text | author | the session text is model context; the notice is for the user |
| K-D44 | The HUD compares `latest` with the launcher's version, recorded in the session view | author | a binary replaced in another terminal must not hide the notice for a session that still runs the old version |

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
15. **Capture Codex premises through the zygo proxy.** Rejected. It makes premises depend on P4-3 and mixes zygo's behavior into a client observation.
16. **A daemon or a timer service for update checks.** Rejected. W-G6: no daemon; the launcher already lives for the session.
17. **A network check in the status-line hook.** Rejected. The hook has a 3 s deadline (H-C2) and runs every 30 s; a file read is enough.
18. **The update notice in the session text or in a `UserPromptSubmit` context.** Rejected. That text reaches the model, not the user, and costs tokens each turn.
19. **Install in the background, apply at the next launch.** Rejected. U decided a prompt before an install; a silent install changes that approved design.
20. **Disable each client's own update notice and let zygo announce all.** Rejected. It changes client behavior that users know, and zygo would need each client's release source for every user.
21. **`zygo doctor --conformance` for the machine-readable form.** Rejected. The binary does not hold the grade files; a generated JSON file next to the Markdown needs no new command.

## 9. Files to create or change

P4-1:

- `docs/conformance/capabilities.toml` (new): the registry.
- `docs/conformance/claude-code.toml`, `docs/conformance/opencode.toml` (new): grades, from today's `docs/conformance.md` evidence.
- `crates/zygo-core/tests/conformance.rs` (new): check and generator; `crates/zygo-core/Cargo.toml`: `toml` dev-dependency.
- `crates/zygo-core/src/lib.rs`: `Agent::ALL` (P4-4 moves it to `client/mod.rs`).
- `docs/conformance.md`, `docs/conformance.json`: generated.
- `scripts/check-conformance.sh`, `scripts/test-check-conformance.sh`: deleted.
- `scripts/check-premises.sh`: names and versions from the grade files.
- `docs/specs/2026-11-18-zygo-design.md`: notes on §8.1 (the registry is the list) and §8.8 (superseded by K§6.8 flow 6).

P4-2:

- `scripts/premises/run-codex.sh` (new), `scripts/premises/mock_upstream.py` (Responses and `CONNECT`).
- `fixtures/codex/` (new): help, schema, requests, hooks, workspace captures.
- `docs/premises.md`, `docs/live-checks.md`: `Codex` header lines, PC rows.

P4-3:

- `crates/zygo-proxy/src/routes.rs`: the two routes.
- `crates/zygo-proxy/src/translate/responses_client.rs` (new): Responses → Messages request; Messages → Responses response and SSE.
- `crates/zygo-proxy/src/pipeline.rs`, `forward.rs`, `cache.rs`, `window.rs`, `redact_stage.rs`, `tee.rs`, `errors.rs`: per-pair behavior of K§6.4.2.
- `crates/zygo-proxy/src/models.rs`: the OpenAI models list.
- `crates/zygo-proxy/tests/` (new tests): K§11 proxy items.

P4-4:

- `crates/zygo-core/src/client/mod.rs` (new): `Agent`, `ALL`, `DEFAULT`, `spec()`, `FromStr`, `as_str`.
- `crates/zygo-core/src/client/claude_code.rs`, `opencode.rs` (new): descriptors and pure functions; `hook_opencode.rs` moves into `opencode.rs`.
- `crates/zygo-core/src/lib.rs`: re-export `Agent` and `Dialect`; remove the moved code.
- `crates/zygo-core/src/hook.rs`: normalization through the descriptor.
- `crates/zygo-core/src/session_view.rs`: `Agent::DEFAULT`.
- `crates/zygo-core/tests/client_boundary.rs` (new): K§6.1.4.
- `crates/zygo-core/src/client/*.rs`: `supported = true` for both; `crates/zygo-core/tests/conformance.rs`: items 7 and 8 read the descriptor.
- `crates/zygo/src/cmd/mod.rs`, `doctor.rs`: the preview gate of K§6.2.7 and the doctor list from the registry.
- `crates/zygo-config/src/schema.rs`: `Dialect` re-export; marker defaults and `LOADOUT_KINDS` from the registry.
- `crates/zygo-config/src/builtin.rs`, `check.rs`: minimum version and kinds from the registry; delete `crates/zygo-config/data/agents.toml`.
- `crates/zygo-redact/src/content.rs`, `harness.rs`: span rule from the descriptor.
- `crates/zygo-proxy/src/window.rs`, `passthrough.rs`, `cache.rs`: proxy facts from the descriptor.
- `crates/zygo-plugins/src/snapshot.rs`: no client branch.
- `crates/zygo-work/src/hooks.rs`, `join.rs`: tool naming and per-agent id from the descriptor.
- `crates/zygo-workspace/src/args.rs`: flag tables move to the descriptors; the walk stays and reads them.
- `crates/zygo-workspace/src/state.rs`, `up.rs`, `snapshot.rs`, `loadout.rs`: `PaneKind::Agent(Agent)`.
- `crates/zygo-hooks/src/session_start.rs`, `session_text.rs`, `workspace.rs`: output and delivery from the descriptor.
- `crates/zygo-mcp/src/tools/mod.rs`: compact tool from the descriptor.
- `crates/zygo-launch/src/client/mod.rs` (new): `adapter_for`.
- `crates/zygo-launch/src/client/claude_code/`, `client/opencode/` (moved from `claude_code.rs`, `opencode.rs`, `assets/opencode-plugin.js`).
- `crates/zygo-launch/src/adapter.rs`: new methods; remove `Plugins`.
- `crates/zygo-launch/src/launcher.rs`, `env.rs`: no client branch.
- `crates/zygo/src/cmd/client/claude_code.rs`, `opencode.rs` (moved from `cmd/claude.rs`, `cmd/opencode.rs`).
- `crates/zygo/src/cmd/mod.rs`, `ws.rs`, `member.rs`, `config.rs`, `doctor.rs`, `compact.rs`, `hook.rs`, `loadout.rs`: registry calls.

P4-5:

- `crates/zygo-core/src/client/codex.rs` (new), `crates/zygo-launch/src/client/codex/` (new), `crates/zygo/src/cmd/client/codex.rs` (new), `crates/zygo/src/cmd/mod.rs` (`Cmd::Codex`).
- `crates/zygo-launch/tests/stub_agent/stub.rs`, `harness.rs`, `stub_launch.rs`, `premise.rs`: Codex parts; `crates/zygo-launch/tests/codex.rs` (new).
- `docs/conformance/codex.toml` (new): grades, `supported = false` until P4-6.

P4-6:

- `docs/conformance/codex.toml`: measured grades.
- `crates/zygo-core/src/client/codex.rs`: `supported = true` when the bar is met.
- `docs/install.md`, `scripts/install.sh`, `scripts/install.ps1`: Codex listed when supported.
- `docs/conformance.md`, `docs/conformance.json`: generated.

P4-7:

- `crates/zygo-core/src/update_state.rs` (new): the one owner of `update.json` and `update.lock` (K§6.5.3).
- `crates/zygo/src/cmd/update.rs`: the start check uses `update_state`; the release check (`check_latest`, `latest`, `release_base`) moves to `crates/zygo-launch/src/update_check.rs` (new), so the launcher can call it.
- `crates/zygo-launch/src/update_check.rs` (new): the background thread, the zygo check and the client checks.
- `crates/zygo-launch/src/launcher.rs`: records the running zygo and client versions in the session view; prints the exit line.
- `crates/zygo-core/src/session_view.rs`: two additive optional fields (running zygo version, client version).
- `crates/zygo-core/src/client/*.rs`: `announces_own_updates`, release source, status surface.
- `crates/zygo-config/src/schema.rs`, `check.rs`: `[update.clients.<id>] announce`; the check error for a client with no release source.
- `crates/zygo-hooks/src/statusline/data.rs`, `render.rs`, `width.rs`: segment 16 and its drop order.
- `crates/zygo/src/cmd/status.rs`, `doctor.rs`: the update and ungraded lines.
- `scripts/premises/`, `fixtures/claude-code/update/`, `fixtures/opencode/update/` (new): the `announces_own_updates` premises (PU1 Claude Code, PU2 OpenCode; Codex is PC16).
- `docs/premises.md`, `docs/live-checks.md`: PU rows; live checks LC-CC1, LC-OC1, LC-CX1 (owner-run: a real newer release shows the client's own notice).
- `docs/conformance/capabilities.toml` and each grade file: A-22.
- `docs/specs/2026-11-18-zygo-design.md` (§3), `docs/specs/2026-11-21-statusline-hud-design.md` (H§6.2, H§6.7), `docs/specs/2026-11-21-auto-update-design.md` (a note: the background check shares the state): amendment notes, K§12.

## 10. Left to the builder

- Field and method names of `ClientSpec` and the new adapter methods.
- The exact JSON field mapping of K§6.4.3, inside its obligations.
- The zygo mark on reasoning items, inside K§6.7.
- The hook trust record format and the exact hook event and key names (from PC4, PC5).
- The Codex flag table (from the pinned `--help`).
- The text layout of `docs/conformance.md`, inside K§6.2.6; the JSON key order, once fixed.
- The one call site of the update thread (K§6.5.2) and the exact session-view field names.
- The exact npm package names, after each client's premise confirms them.
- The order of the files in each move, if each step keeps the suite green.

## 11. Acceptance

Each item is decided by a test or a command.

P4-1:

1. `conformance` passes; with a planted fault it fails for each item 1-10 of K§6.2.5 (one unit case per item).
2. A unit case with a two-capability registry gives the K§6.2.4 score, tier breakdown and bar.
3. `docs/conformance.md` and `docs/conformance.json` equal the generator output; CI fails when either does not.
4. **Feature matrix.** For a fixture registry and two fixture grade files, the matrix rows come grouped core, standard, extra, in registry order inside a tier; each cell text is `full <url>`, `partial (missing: …) <url>`, `none (…) <url>` or `n/a (…)` as K§6.2.6 states; the JSON `matrix` holds the same grades.
5. **Rollup.** For the same fixtures, each tier row and the total row give the expected full / partial / none / n/a counts and met / applicable counts; a client with one unmet core criterion and no exception shows `below bar` with that capability id; with an exception it shows `meets bar`.
6. **Score.** For the same fixtures, the total and each tier sub-score equal the K-D8 values computed by hand in the test, rounded down; a capability with only `n/a` criteria leaves both numerator and denominator.
7. `sh scripts/check-premises.sh` passes with versions read from the grade files.

P4-2:

8. Each PC row in `docs/premises.md` has a date, result and fixture paths that pass the `nontrivial` rule; `check-premises.sh` passes.
9. No fixture holds a token, a key form, a user name or a home path (`scrub.py` pass).

P4-3 (proxy tests with a mock upstream; Codex request bodies from `fixtures/codex/requests/`):

10. Each captured Codex request translates to a valid Messages request; each obligation line 1-10 of K§6.4.3 has a case.
11. A Messages SSE fixture with text, tool use and thinking gives the event sequence of stream obligations 1-4, with rising `sequence_number`.
12. An upstream error mid-stream gives one `response.failed` and no other terminal event.
13. A tool call round trip (call, output, next request) keeps `call_id`.
14. A thinking round trip to the same provider keeps the signature; to another provider it is dropped with its code.
15. Responses → Responses: the upstream body equals the client body except `model`, `store`, `include` and stage edits.
16. Responses → chat: a captured request reaches the chat mock, and its stream returns as Responses SSE.
17. `previous_response_id`, an unknown item type, and a bad identity each give the stated status in the OpenAI error shape.
18. Redaction acts on a Responses `input` body and skips the PC13 spans.
19. With the rewrite armed, `response.completed` usage follows §10.2.2; with it off, the bytes pass unchanged.
20. `GET /openai/v1/models` gives the role list; `POST /openai/v1/chat/completions` stays `404 zygo_route_not_in_v1`.

P4-4:

21. `cargo test --workspace --locked` passes on Linux, macOS and Windows CI.
22. `git diff -M <P4-4 base> -- 'crates/*/tests'` shows only renames and `use`/path lines.
23. `client_boundary` passes on the tree, and fails on a test input that holds `Agent::ClaudeCode` in a non-allowlisted file and on one that holds the literal `"opencode"`.
24. `stub_launch` asserts byte-equal generated settings and inline config for both clients against the output at the P4-4 base commit (the existing assertions, unchanged).
25. A state file with `"kind": "claude-code"` reads into `PaneKind::Agent(Agent::ClaudeCode)` and writes back the same bytes.
26. The preview gate: `zygo codex` is absent until P4-5; a test client with `supported = false` exits 2 with the K§6.2.7 text and runs with `ZYGO_PREVIEW_CLIENT` set (CLI test with the stub).

P4-5:

27. The Codex stub launch test on the three systems asserts: the `CODEX_HOME` tree and modes; each `config.toml` key; the `-c` arguments; no token in arguments, environment or files except `token`; no `CODEX_*` from the parent environment; the hooks and agent files; the `sessions` link.
28. The premise test finds each written key in `fixtures/codex/config.schema.json` and each flag in `fixtures/codex/help.txt`.
29. Each Codex hook fixture parses to the common form; each reply renders in the PC5 shape; a non-object body and an unknown event give no output and exit 0.
30. A workspace stub test captures the Codex session id and builds `resume <id>`.
31. The K-D13 branch is chosen from planted project files (none, a hook file, a config hook) as stated.
32. `client_boundary` and `conformance` pass with Codex added.

P4-6:

33. Each Codex grade cites measured evidence; `conformance` passes.
34. If the bar is met: `supported = true`, `zygo doctor` lists Codex, `docs/install.md` names it. If not: the generated document shows `below bar`, and P4-6 ends with the unmet core criteria listed for the operator.

P4-7 (fixture server as in `crates/zygo/tests/update.rs`; no real clock wait: the tick and the clock are injected, K-C6):

35. One tick with a stale throttle makes one GET and writes `checked_at` and `latest`; a second tick within 24 h makes none; two launchers with one shared state make one GET (the second fails the try-lock or sees the fresh `checked_at`).
36. The tick runs with `ZYGO_SESSION` and `ZYGO_WORKSPACE` set, off a TTY and with `-p`; it does not run with `[update] check = false` or `ZYGO_NO_UPDATE_CHECK=1`.
37. The tick never prompts and never writes the binary (the test binary path is unchanged after a tick that finds a newer version).
38. A failed GET writes `checked_at`, one session-log line, and no segment.
39. The status line with `update.json` `latest` newer than the session view's version shows segment 16 (compact and verbose texts); equal, older, or equal to `skip`: no segment; an `update.json` of 5 KiB or non-JSON: no segment and no error output.
40. Under width pressure, segment 16 is removed first.
41. `zygo status` prints the update line; the launcher prints the exit line when stderr is a TTY and not otherwise.
42. With a client flag on, a fixture release source makes one GET per 24 h per client; the HUD and `zygo status` show `<client> <new> available (zygo graded <pinned>)`; with the flag off (default when the fact is `true`) no client GET happens.
43. `[update.clients.codex] announce = true` in user config turns the flag on; the same key in a repository or team layer has no effect (`layers.rs:68`); `announce = true` for a client with no release source is a `zygo config check` error.
44. A session view with a client version newer than the descriptor pin gives `<client> <running> ungraded` in `zygo status`, `zygo doctor` and the verbose HUD, not in the compact HUD; no network call happens (the fixture server sees no request).
45. `grep -rnE 'autoupdate|DISABLE_AUTOUPDATER|check_for_update_on_startup' crates/zygo-launch/src` finds nothing (zygo hides no client notice); each PU/PC16 premise row is `verified` and A-22.2 cites it.

## 12. Base-spec changes

- **§3** (zygo design): the "Nothing phones home" exception widens as K§6.5.6 states.
- **§8.1**: the capability list is `docs/conformance/capabilities.toml`; A-22 is added; the cell form and C9 stand.
- **§8.4**: `ZYGO_AGENT` takes each registry id (`claude-code`, `opencode`, `codex`); each client adds its own denylist rows.
- **§8.8**: superseded by K§6.8 flow 6.
- **§7.6 v2**: the Responses client route is built (K§6.4); the chat client route and the `openai-chat` client pair stay v2.
- **H§6.2, H§6.7** (HUD design): segment 16 and its place first in each drop list.
- **U** (auto-update design): the throttle state has one owner, `zygo_core::update_state`, shared with the background check; U's prompt, skips and rule are unchanged.

## Change log

- 2026-11-25: first version.
- 2026-11-25: the operator's two additions: the three conformance outputs (K§6.2.6) and update notices (K§6.5, P4-7), with the operator's answer on client updates.
- 2026-11-25: order against phase 3 (K§6.9); items renumbered in causal order (conformance P4-1, Codex premises P4-2, proxy P4-3, client contract P4-4, Codex client P4-5, Codex grade P4-6, update notices P4-7).
