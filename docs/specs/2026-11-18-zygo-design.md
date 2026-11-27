# zygo: design specification

**Status:** approved by the owner 2026-11-18. **Amendment A1 (2026-11-18), approved by the owner 2026-11-18:** name `zygo`; marketplaces and plugin approval (§15); code navigation and SDLC skills from act-lite (§10.5, §6.5, §8.6, §13.2, §13.4); act-lite store values in the secret pass (§16.1). Amended text is marked *(A1)*. **Amendment A2 (2026-11-18), approved by the owner 2026-11-18:** a slash command for every internal feature and `/zygo:help` (§17.1, §6.1 layer 7); marked *(A2)*.

**Verification (2026-11-18):** comprehension 3 runs (15, 15 and 6 gaps, all fixed); critic 3 runs (15, 12 and 7 findings, all fixed; the fixes after run 3 were not reviewed again); sufficiency 1 run (4 questions: 3 answered in §7.2, §7.6 and §19; 1 answered by the owner, O8).
**Date:** 2026-11-18
**Name:** `zygo`, from Greek *zygós*, "yoke" (owner, 2026-11-18).

This document is the authority for building `zygo` v1. It is self-contained: a builder needs no other document. Each decision is marked **(owner)** when the product owner made it and **(author)** when the author of this document made it; the owner can change any author's call before approval.

---

## 1. Intent

AI coding agents (Claude Code, OpenCode, Codex, …) waste most of their tokens in three places: a main conversation that does all the work itself and grows until its context is huge, tool outputs (test runs, logs, file listings) that are pasted whole into the context, and whole-file reads where a symbol outline would do. They are also tied to one vendor's models per agent.

`zygo` is a single binary that wraps a coding agent. It starts the agent against a model proxy that `zygo` controls, injects hooks, a local MCP server, skills and plugins, and enforces a working pattern called **elephant-goldfish**: the main conversation (the elephant) plans, briefs and checks; every piece of work runs in a short-lived subagent (a goldfish) with a fresh context, on the cheapest model that does it well. The proxy routes each **role** to a model, translates between API dialects so any model can serve any agent, keeps prompt caches warm, redacts secrets and profanity, and makes the agent compact its context earlier than the model's full window.

The result is an open-source, single-user-first tool that a developer runs on their own machine, and that a small team can also run as one shared server.

## 2. Goals

1. **G1 Wrap agents.** `zygo claude` and `zygo opencode` launch the agent with every `zygo` feature on and no manual configuration.
2. **G2 Route by role.** The agent asks for a role (`lead`, `design`, `plan`, `triage`, `review`, `code`, `read`); `zygo` decides the model. Any model in the catalog can serve any agent (dialect translation).
3. **G3 Elephant-goldfish.** The main conversation runs on the `lead` role and cannot edit files; each role is a subagent with its own prompt and tool limits.
4. **G4 Save tokens.** Result store, soft context window with compaction signal, model-triggered compaction, cache markers and provider pinning, code navigation tools, compaction instructions, inefficiency guards.
5. **G5 Model profiles.** Named sets of role→model overrides, custom roles, disabled roles, a profile prompt and a plugin selection.
6. **G6 Plugin delivery.** Skills and plugins from git marketplaces, checked, stored, and given to each session as a snapshot, for each agent in the form it reads.
7. **G7 Passthrough.** A user with a claude.ai subscription runs Claude Code on that subscription and still gets every `zygo` feature that does not depend on routing.
8. **G8 Redaction.** Secrets in any request string and profanity in the user's own typed text are replaced by the proxy before a request goes to a provider: on the user's machine in local mode, on the team server in serve mode (the request reaches the server over HTTPS, C8, and the server is the team's trust boundary) **(owner)**.
9. **G9 Work units.** Work is attributed to units whose ids come from pluggable work-item sources (Beads, GitHub issues, the git branch); optional guards judge claims, scope, navigation and output per unit.
10. **G10 Two modes, one binary.** Local mode (default): the proxy runs inside the launcher on loopback, no login. Serve mode: one long-running shared server for a team, authenticated by a shared token.
11. **G11 Workspaces.** `zygo up` creates or restores one tmux session per repository, with its layout and its agents; `zygo down` waits until each agent has ended its turn and its subagents (the drain), then saves the state and stops the agents; the next `zygo up`, also after a reboot or a crash of the tmux server, resumes each agent in its own conversation, with its saved launch flags and overrides. A loadout in user config sets the first layout and the agent panes; CLI verbs list, attach and forget workspaces; no daemon: tmux is the only long-lived process (workspaces design `docs/specs/2026-11-20-workspaces-design.md`, W-G1 to W-G6).

## 3. Non-goals

- User accounts, sign-in, single sign-on, groups, entitlements.
- Quotas, budgets, cost tiers, credits, boosters, spend limits, per-person usage reports, leaderboards.
- A policy bundle delivered through CI, signed configuration, minimum-version enforcement.
- A model admission or fitness gate.
- A web portal or admin UI.
- API keys for non-agent clients; any client other than an `zygo`-launched agent.
- Notifications, escalation of a session to a stronger role.
- Self-hosted model serving. `zygo` proxies to hosted providers (any OpenAI- or Anthropic-compatible endpoint, including a local one the user runs).
- Storing request or response content anywhere. `zygo` stores metadata only (§14).
- Multi-replica serve mode. One server process holds the session state in memory.
- Writing back to work-item sources (no status changes, no comments) in v1.
- Telemetry that leaves the machine. Nothing phones home, with one exception: the update check reads the release's public VERSION file and sends nothing else; opt-out `[update] check = false` in user config, or `ZYGO_NO_UPDATE_CHECK` set to any non-empty value (for example `ZYGO_NO_UPDATE_CHECK=1`) (§17 `zygo update`; auto-update design `docs/specs/2026-11-21-auto-update-design.md`). *(2026-11-22, auto-update design)*

## 4. Constraints

- **C1** One native binary per platform: macOS (arm64, x86_64), Linux (arm64, x86_64), Windows (x86_64, arm64); WSL counts as Linux. Rust is the implementation language **(author)**: one static binary, no runtime to install. *(Note 2026-11-20, owner decision at delivery: 0.1.0 publishes all six binaries so a Windows tester can test them. The Windows test job reports but does not block CI or the release until node N78 fixes the Windows failures of CI run 44783725171; `docs/install.md` names the known Windows defects.)* Workspaces (G11) need tmux 3.2 or later, and native Windows has no tmux: there every workspace verb exits 2 with `workspaces need tmux; run zygo in WSL` before any other step, and `zygo claude` and `zygo opencode` work as before (workspaces design OP5).
- **C2** Installs never need administrator rights. Install script → `~/.local/bin/zygo` (Windows `%LOCALAPPDATA%\zygo\bin\zygo.exe`), checksum verified.
- **C3** The proxy never stores a request or response body, never logs one, and never writes a redacted word or secret to any log, file or metric.
- **C4** Secret redaction fails closed (the request is refused if the pass cannot finish). Profanity redaction fails open at its time budget (the request goes on, the event is counted).
- **C5** The request path is bounded. Every pass over a body is O(body size) with a stated per-request deadline; bodies of several MiB are normal (long contexts).
- **C6** A hook never breaks the agent: on any internal error a hook prints nothing that changes a decision, writes one line to stderr, and exits 0.
- **C7** The agent is not told its compaction window. No prompt text, setting or refusal says the window is smaller than the model's (§9).
- **C8** Serve mode accepts only HTTPS on any non-loopback address, on both ends: `zygo serve` refuses to listen without TLS, and the client refuses a `--server`/`[server] url` that is not `https://` unless its host is loopback (exit 2). *(Note 2026-11-19, N37 review: the client also refuses a server URL that holds a user name or password (exit 2); the team token is the only credential it sends.)*
- **C9** Agent support is declared, not assumed: every agent has a conformance row for every capability in §8.1 before its launcher is built.
- **C10** v1 agents: Claude Code and OpenCode **(owner)**. v2: Codex and the Grok CLI; later: Pi **(owner)**.

## 5. Glossary

| Term | Meaning |
|---|---|
| Agent | A coding-agent program `zygo` launches (Claude Code, OpenCode). |
| Session | One launch of one agent. Has an id (ULID), a session token, a session directory, a profile and state in the proxy. |
| Proxy | The HTTP component that receives the agent's model requests and forwards them to providers. In-process in local mode; `zygo serve` in serve mode. |
| Dialect | An API shape: `anthropic` (Messages API), `openai` (Responses API) or `openai-chat` (chat/completions). A provider lists the dialects it speaks; a client's dialect is its route (§7.3). |
| Claude id | A catalog model id that starts with `claude-`. |
| Provider | An upstream endpoint for a model: base URL, dialects, key reference, prices, flags. |
| Catalog | The set of models `zygo` knows, each with providers and facts (§6.3). |
| Role | A named job (`lead`, `design`, …) mapped to a model, with a description, a prompt and a `writes` flag (§6.4). |
| Bundle role | A role defined in config (§6.4, built-in or overridden there), as opposed to a custom role a profile adds (§11). |
| Capability word | A word a skill may use instead of a role: `lookup` (find and read), `mechanical` (rote edits), `implementation` (a specified change), `judgment` (a decision, a review, a root cause). §8.6 maps each to subagents. |
| Lead / elephant | The main conversation. Runs on `lead`, plans and delegates, never edits files. |
| Goldfish | A subagent `zygo-<role>` with a fresh context, given one brief. |
| Model profile | A named set of overrides and additions to the roles (§11). |
| Result store | Local storage of large tool outputs, read back through MCP tools (§10.1). |
| Soft context window | Per-model token count above which `zygo` signals the agent to compact (§10.2). |
| Work item | An external task record (a Beads issue, a GitHub issue). |
| Work unit | The unit of attribution inside a session: `<source>:<id>[:<part>]` (§13). |
| Guard | A deterministic detector with a level: `monitor`, `notify` or `enforce` (§13.4). |
| Passthrough | A session whose upstream credential is the agent's own subscription login (§12). |
| Marketplace | A git repository or directory of plugins in the Claude Code marketplace layout (§15). |
| Adjustment code | A short code recorded when the proxy changes a request or response (`effort_clamped:high>medium`, `context_signal`, …). Never content. |
| Workspace | One git repository, identified by its git common dir: the main checkout and every linked worktree are one workspace (workspaces design OP2). |
| Workspace session | The tmux session of a workspace, named `zygo-<name>`; one per workspace (workspaces design OP3). |
| Workspace id | The first 16 hex characters of SHA-256 of the canonical common-dir path; form `^[0-9a-f]{16}$` (workspaces design AU2, AU84). |
| Pane key | A stable id that zygo gives each pane of a workspace session: `k` and 8 characters of lower-case base32, form `^k[a-z2-7]{8}$` (workspaces design AU84). It stays the same after a restore; tmux pane ids (`%N`) change. |
| Member | The agent slot of one pane key. It runs one session at a time; its record persists across sessions, so the next `zygo up` can resume it. A resumed agent is a new session with the same agent conversation. |
| Agent session id | The agent's own conversation id (Claude Code `session_id`, OpenCode `sessionID`); zygo resumes by it. Not the session id (ULID). |
| Live launcher | A launcher that holds the lock on the `pid` file of its session directory. |
| Loadout | A named layout and its panes, in user config (workspaces design OP10). "Profile" stays the model profile (§11). |
| Loadout pane | One `[[loadout.<name>.window.pane]]` entry (§6.1 layer 3): an agent pane or a shell pane. |
| Snapshot | The saved state of a workspace: layout, panes, cwds and member records. |
| Drain | The wait in `zygo down` until each member's turn and its subagents end. |
| Detached member | A member record whose pane key is not in the saved layout; `zygo up` does not recreate it. |

## 6. Configuration

### 6.1 Files and precedence

Lowest to highest; a higher layer replaces a key of a lower one, table by table, with two exceptions in §13.5: guard levels never go below `[guards] min_level`, and the turn cap is the lower non-zero value of the layers: *(Note 2026-11-20, owner decision (whole-branch review, config finding 1): the user, repository, profile and flag layers may replace a key the team config sets (session overrides still cannot, layer 7), with three exceptions: they cannot lower the team's `[guards] min_level`, turn off `redaction.secrets` when the team config sets it on, or remove a plugin the team marks `required`. The team value of these three is a floor the higher layers cannot go below.)*

1. **Built-in defaults** compiled into the binary: catalog (§6.3), roles and prompts (§6.4), redaction patterns (§16), thresholds.
2. **Team config** (serve mode only): the server's config, served to clients at session creation (`GET /v1/zygo/config`): roles, catalog ids, redaction, guard defaults, plugin marketplaces. Provider keys and URLs never leave the server. It cannot set `[loadout]` or `[workspace]`: a loadout starts programs with arguments, and a team server must not start programs on a client, so the client drops both tables from the team layer with one warning, `zygo: [loadout] and [workspace] are read from user config only; ignored` (workspaces design AU22).
3. **User config** `~/.config/zygo/config.toml` (Windows `%APPDATA%\zygo\config.toml`; `ZYGO_CONFIG` overrides the path). It is the only layer, with the built-in defaults, that sets `[workspace]` (`default_loadout`, default `solo`, the built-in loadout of one Claude Code pane; `drain_block_dispatch`, default `true`; `drain_timeout`, default `10m`; `pool`, the config-directory pool, no default, absolute after `~` expansion, whose members run in passthrough) and `[loadout.<name>]` (windows with a layout and agent or shell panes) (workspaces design W§11; workspace definition design R§9.5). With the built-in defaults and the team config, it sets `[limits] max_members`, the most agent panes a repository loadout may start, 1-32, default 8 (workspace definition design R§9.4).
4. **Repository file** `.zygo.toml` at the repository root, read from the default branch (§13.5). It may set only `[guards]`, `[navigation]`, `[limits]` and `[work]`; any other table is ignored with one stderr line. It cannot set `limits.max_members`: the load removes it with one stderr line, `.zygo.toml: limits.max_members ignored; only user or team config may set it` (workspace definition design R§9.4).
5. **Model profile** selected for the launch (§11).
6. **Launch flags** (`--profile`, `--passthrough`, `--server`, agent flags after `--`).
7. **Session overrides** *(A2)*: changes made during a session with a `/zygo:` command or its CLI form (§17.1), kept in `$ZYGO_SESSION_DIR/overrides.json` and, for proxy-side settings, in the session state; discarded at session end. For a workspace member, each change, and each change of the compaction override, is also saved in its member record, and `zygo up` applies the saved values at resume, so they survive `zygo down`/`zygo up`, crashes and reboots (workspaces design OP12, AU105). They cannot change a key the team config sets, lower a guard below `min_level`, disable a `required` plugin, or turn off `redaction.secrets`. *(Note 2026-11-19, owner decision at N37 planning: "a key the team config sets" means a key the server's config file names explicitly; `GET /v1/zygo/config` sends only those keys (not the defaults), and every other key stays open to the user and session layers within the limits above.)*

In serve mode the server's catalog and provider set are authoritative: a client's user config may add profiles, plugins and work sources, but it cannot add a model or provider the server does not have.

`[statusline] style = "compact" | "verbose"` (default `compact`) selects the status line style (§17; HUD design H§8). `[update] check = true | false` (default `true`) turns the start check of `zygo update` on or off (§3, §17; auto-update design). Only user config may set these two tables: the repository file ignores them (layer 4, one stderr line), and the team layer drops them with one warning, `config: [statusline] in team config ignored; only user config may set them` (the warning names each dropped table, `[statusline], [update]` when both are present). `zygo statusline` changes the style for a session (layer 7). *(2026-11-22, HUD design, auto-update design)*

`zygo config show [--layer <name>]` prints the merged config with each key's source layer; secrets are shown as `<set>`/`<unset>`. In a repository whose workspace files are approved on this machine, each key that they add (the repository profiles and `loadout.repository`) shows the source `workspace-files` and its file, for example `# workspace-files .zygo/profiles/rust.toml`. `workspace-files` is a source, not a layer: `--layer workspace-files` shows only those keys, and `--layer repository` shows only the keys of `.zygo.toml` (workspace definition design R§10).

### 6.2 Provider keys

- A provider entry names its key as `key = "env:ANTHROPIC_API_KEY"` or `key = "keychain:anthropic"`. `zygo keys set <name>` stores a key in the OS secret store (macOS Keychain, Windows Credential Manager, Secret Service on Linux, a `0600` file when none is available, reported by `zygo doctor`).
- Keys are read by the proxy only; they are removed from the agent's environment (§8.4).
- A provider whose key is missing is skipped at request time. No provider left: `503 zygo_provider_key_missing` when every candidate was dropped for a missing key, else `503 zygo_provider_unavailable`.

### 6.3 Catalog

```toml
[[model]]
id = "claude-opus-5-5"
soft_context_window = 400000         # optional (§10.2); ≤ facts.context_window

  [model.facts]                        # optional; required for translation features (§7.6)
  reasoning = "always"                 # always | switchable | none
  # reasoning_default_on = true        # only with switchable
  effort_levels = ["low", "medium", "high", "xhigh", "max"]  # subset of the ladder, lowest first
  max_output_tokens = 128000
  context_window = 1000000
  sampling = false                     # temperature/top_p accepted
  vision = true
  structured_output = "native"         # native | none
  forced_tool_choice = false           # tool_choice naming one tool accepted
  verified_on = 2026-11-18
  source = "https://…"                 # where the facts were checked

  [[model.provider]]
  name = "anthropic"
  base_url = "https://api.anthropic.com"
  dialects = ["anthropic"]
  upstream_model = "claude-opus-5-5"
  key = "env:ANTHROPIC_API_KEY"
  enabled = true                       # false = skipped (the kill switch)
  [model.provider.price_usd_per_mtok]  # the row cost (§14) and the local session spend (§7.2); a team session's spend uses the built-in list prices
  input = 4.0
  output = 20.0
  cache_read = 0.20
  cache_write = 8.0
  [model.provider.flags]               # optional (§7.5)
  zdr = false
```

- The effort ladder is `none < minimal < low < medium < high < xhigh < max`.
- Providers are tried in the listed order (§7.4), after the session's pinned provider.
- The built-in catalog ships rows for current Anthropic and OpenAI models with verified facts **(author)**. The model ids, prices and facts are data; the builder verifies each row against the provider's documentation and records `source` and `verified_on`.
- `price_usd_per_mtok` may hold `long_context = { above, input, output, cache_read, cache_write, cache_write_5m }` (`cache_write_5m` optional): when a request's `prompt` (§10.2.1) is above `above` tokens, all its rates come from `long_context`, else the base rates (HUD design H§7.3). *(2026-11-22, HUD design)*
- `price_usd_per_mtok` may hold `cache_write_5m`, the rate of 5-minute cache writes (absent: `cache_write`). `cache_write` prices every other cache write: 1-hour writes, and writes whose usage has no 5-minute/1-hour split. It is the provider's longer-retention write rate: the 1-hour rate in the built-in Anthropic rows, the 30-minute-retention rate in the built-in OpenAI rows. Each Anthropic row of the built-in catalog sets `cache_write_5m` = 1.25 × its `input` rate (HUD design AU33). *(2026-11-22, HUD design)*
- `zygo config check` validates: unique ids; each model has ≥ 1 provider; `soft_context_window` > 0 and ≤ `context_window`, and only with facts; `effort_levels` empty only with `reasoning = "none"`; every role's model exists; every `[[schema_rule]]` (§7.6) is well formed; every rate of `price_usd_per_mtok` and of its `long_context` (`input`, `output`, `cache_read`, `cache_write`, `cache_write_5m`) is a finite number ≥ 0, and `long_context.above` > 0 *(2026-11-22, HUD design)*. A failing user config refuses to launch with the first violation; a failing team config refuses to start the server.

### 6.4 Roles

```toml
main_role = "lead"

[role.lead]
model = "claude-opus-5-5"
description = "Orchestrates the session: plans, briefs subagents, checks their work."
prompt = "…"            # §6.5
writes = false          # the lead guard (§8.3) enforces it for the main loop
```

Default roles and models **(author)**, all on Anthropic (the first provider of the built-in catalog): `lead`, `plan`, `triage`, `review` → its strongest general model; `design` → the vendor's top reasoning model with the 1M window; `code` → a mid-price coding model; `read` → the cheapest fast model. `writes = false` for `lead`, `read`, `review`; `true` for the others. Each role also has `dialect_models = { openai = "<id>" }` optional: the model to use when the agent's dialect is `openai` and the role's model has no provider for it.

### 6.5 Role prompts (defaults; editable in config)

| Role | Prompt |
|---|---|
| `lead` | You are the lead of this session: the elephant of the elephant-goldfish model. You hold the context and the judgment; every task runs in a goldfish, an `zygo-<role>` subagent with a fresh context, on the cheapest role that does it well. Plan the work, write each subagent's brief (the files, the change, how to check it), dispatch it, and check what it returns against its brief. You do not edit files; zygo refuses the edit. Running a check to confirm a subagent's report is yours. |
| `design` | You decide architecture and design questions with long-term consequences. Give the decision, the alternatives you rejected and why, and what would make you change it. You do not write code. |
| `plan` | You turn a task into ordered steps, each with its check. Return the plan; do not do it. |
| `triage` | You find the root cause of a defect from the symptom, the reproduction and what is already ruled out. Prove the cause before you name it; report the cause, the evidence and the smallest fix. |
| `review` | You are a reviewer who has not seen the work being made. Find defects in the change you are given: correctness, security, missing tests. Report each with its location, the failure it causes and the evidence. Do not approve; do not fix. |
| `code` | You make the one well-specified change you are given: edit the code, write the tests, make the check pass. Stay within the brief; report what changed, the check's output and anything the brief did not foresee. |
| `read` | You do read-only and mechanical work: find, trace, summarize, classify, extract, reformat. Answer with what you found and where; do not change files. |

The prompts are also the vocabulary for skills: a skill names a role or a capability, never a model (§8.6).

## 7. Proxy

### 7.1 Topology

```
Local mode (default)                              Serve mode
┌────────────────────────────────────┐            ┌───────────────────────────┐
│ zygo claude                         │            │ zygo serve (team server)  │
│  ├─ proxy on 127.0.0.1:<random>     │──┐         │  proxy, HTTPS             │
│  ├─ session dir, hooks, MCP server  │  │         │  session state in memory  │
│  └─ child: claude (ANTHROPIC_BASE_  │  │         │  usage db (metadata)      │
│      URL = the local proxy)         │  │         └────────────▲──────────────┘
└────────────────────────────────────┘  │                      │ session token
                                         ▼                      │
                                 Providers (Anthropic,   zygo claude --server <url>
                                 OpenAI, OpenRouter, any  (hooks, MCP, result store,
                                 compatible endpoint)      plugins stay local)
```

- **Local mode.** The launcher starts the proxy in its own process on `127.0.0.1` with an OS-assigned port, before it starts the agent, and stops it after the agent exits. Each launch has its own proxy; concurrent sessions do not share one. **(author)** No background daemon. Workspaces add none: tmux holds the agent processes, and zygo runs only when a verb, a hook or a pane starts it (workspaces design AU1, W-C2).
- **Serve mode.** `zygo serve --listen <addr> [--tls-cert <pem> --tls-key <pem>]`. A non-loopback address without TLS is refused at start (C8). Clients use `--server <url>` or `[server] url` in user config, with `[server] token = "env:ZYGO_TEAM_TOKEN"` or `"keychain:zygo-team"` (`zygo keys set zygo-team` stores it); the agent then sends its requests to the server directly; hooks, the MCP server, the result store and plugin snapshots still run on the client.
- Both modes run the same request path (§7.3) and the same session API (§7.2).

### 7.2 Sessions and authentication

- **Serve-mode tokens.** `[serve] tokens = [{ label = "alice-laptop", sha256 = "<hex>" }]`. `zygo serve token new <label>` prints a new random token once (`zyg_t_` + 43 base62 characters) and appends its hash. The label is the only identity the server has; it is written on usage rows. Removing an entry revokes it at the next config reload (`SIGHUP` or `zygo serve reload`).
- **Team config.** In serve mode the launcher first calls `GET /v1/zygo/config` (`Authorization: Bearer <team token>`) and merges the reply as layer 2 of §6.1; the profile is resolved against the merged config. In local mode layer 2 is empty. The reply carries: `main_role`, roles (model, description, prompt, writes), team profile presets, redaction patterns and content settings, guard defaults and `min_level`, `[limits]`, `[features]`, marketplaces with refs, commits, required/default flags and team approvals (§15.3), `[results]` and `[navigation]` values, and the catalog **without providers**: per model `id`, `facts` (with `context_window`), `soft_context_window`, and the dialects it can be served on (directly or by translation). Provider names, URLs, keys, prices and flags never leave the server. The client uses this for the picker (`behavesAs`, §8.5), OpenCode's `limit.context` (§10.2.5), profile pre-validation and `zygo config check`; the server validates again at session creation. **(author)**
- **Session creation.** Then the client calls `POST /v1/zygo/sessions` (serve mode: `Authorization: Bearer <team token>`; local mode: in-process call, no HTTP). Body: `{ "agent": "claude-code" | "opencode", "agent_version", "profile": <the resolved profile, §11> | null, "passthrough": bool, "repo", "work" }`. The proxy validates the profile against its catalog (§11.3) and replies `{ "session": "<ulid>", "token": "zyg_s_<43 base62>", "config": <the session's view with the profile applied: enabled roles and custom roles with prompts, the model list, guard defaults>, "expires_at" }`.
- **Session token.** Sent by the agent on every model request: through the agent's key mechanism (Claude Code `apiKeyHelper` = `zygo hook token`, which prints it; OpenCode provider `apiKey`), or, in passthrough, in the header `X-Zygo-Token` (§12). The token is bound to the session id; the request must also carry `X-Zygo-Session` with that id. Mismatch or unknown → `401 zygo_token_invalid`. *(Note 2026-11-18, N10 planning: the session routes `…/compact`, `…/settings` and `…/status` take the same token (`x-api-key`, else `authorization: Bearer`) and `X-Zygo-Session`; the path id must equal the header id, else `401 zygo_token_invalid`. An `X-Zygo-Session` that names no live session → `401 zygo_session_unknown`.)*
- **Session state** (in memory, per session): agent, profile, passthrough flag, provider pin per model (§10.4), compaction override (§10.2), last heartbeat time. No session expires by idle time: a session ends when its launcher deletes it at exit, local mode by stopping its proxy, or when the server restarts; `expires_at` is always empty and is kept because older clients require the field. *(2026-11-22, FIX-18)* A server restart loses sessions: requests get `401 zygo_session_unknown`, and the client prints "zygo server restarted; relaunch the session". **(author)** *(Note 2026-11-18, N10 review: shutdown closes open connections at once.)*
- `DELETE /v1/zygo/sessions/{id}` at launcher exit.
- **Liveness (serve mode).** The launcher sends `POST /v1/zygo/sessions/{id}/heartbeat` every 60 s with the team token. After 5 min without a heartbeat the session is **suspended**: its token is refused (`401 zygo_session_suspended`) but its state is kept; the next heartbeat (a laptop waking up) resumes it. A suspended session is not deleted by time: it is kept until the next heartbeat resumes it, its launcher deletes it, or the server restarts. *(2026-11-22, FIX-18)* A session token copied by a tool therefore stops working within 5 min of the launcher stopping. **(author)** *(Note 2026-11-18, N10 planning: liveness applies to sessions created with a team token; creation counts as a heartbeat. A team token whose label did not create the session → `401 zygo_session_unknown` on its session routes.)*
- **Status.** `GET /v1/zygo/sessions/{id}/status` (session token) replies with the session's `session`, `profile`, `work`, `used_context`, `effective_window` (§10.2.2), `compact`, `cache_read_share`, `expires_at` and `usage_note`, and with the HUD fields of `docs/specs/2026-11-21-statusline-hud-design.md` H§7.2: `model`, `context_window`, `threshold`, `tokens_in`, `tokens_out`, `spend_usd`, `spend_notional`, `requests_total`, `requests_priced`, `passthrough`, `team_session`. Each HUD field is optional, so a reply from an older server parses. A local proxy prices `spend_usd` with the catalog prices of the route that served each request (notional prices in passthrough, with `spend_notional = true`); a team server prices it with the built-in list prices compiled into its binary and sends `spend_notional = true` (HUD design AU14). The "Team config" rule above holds: no configured price leaves the server. *(2026-11-22, HUD design)* The reply also has `requests_in_flight` (model requests accepted on the forward path and not yet ended, a streamed body included) and `last_request_end` (the RFC 3339 UTC time at which the last such request ended; absent when none ended), from a local proxy and a team server alike; both are optional, so an older client ignores them and a reply from an older server has neither. The `zygo down` drain reads them (workspaces design AU91, OP13).
- **Credentials per route.** Model routes, `…/compact`, `…/settings`, `…/status`: the session token. `POST /v1/zygo/sessions`, `…/heartbeat`, `DELETE …/sessions/{id}`, `GET …/requests`, `GET /v1/zygo/config`, `GET /v1/zygo/plugins`: the team token, and for a session route only a session created with the same token label.

*(Note 2026-11-19, N16 planning: a reload (`SIGHUP` or `zygo serve reload`) reads the server config again and runs `zygo config check`; on an error the running config stays and the server prints `zygo serve: reload failed (<file>); run zygo config check`, with no value from the file (C3). On success one swap replaces the team tokens, the config and the known plugin set: team routes and new sessions use them at once; a live session whose token label is gone is removed, so its session token is refused at once on every route (`401 zygo_session_unknown`); other live sessions keep their creation-time redaction patterns, session config (roles view, guards) and features until they end, while model resolution reads the current catalog (§11.3). The TLS certificate and key, `--listen`, `[proxy] max_inflight_mib` and `[proxy] redaction_cache_mib` take effect at restart. Usage rows past `retain_days` are pruned at start and at each successful reload. The known plugin set comes from config only (the server does not sync): marketplace `default_on` and `required`, `[plugins] enable`/`disable`, and every key of a configured profile's `plugins` map. `zygo serve` holds an exclusive lock on `<data_dir>/serve.pid` for its life; a second server exits 1 (`zygo serve is already running`); `zygo serve reload` signals only a pid whose file is locked, else prints `no running zygo serve` and exits 1; on Windows it exits 1 (`not available on Windows; restart zygo serve`). `zygo serve token new <label>` refuses a label the config already has (`label <l> exists; remove it from the config first`). `--listen` defaults to `127.0.0.1:8443`. A non-loopback listen without TLS exits 2; an invalid config exits 11.)*

*(Note 2026-11-19, owner decision "follow sensible standards" — timeouts and limits the sections above and below do not give:*
*- Upstream (proxy to provider): connect 10 s; read-idle 600 s between bytes, the default request timeout of the Anthropic and OpenAI SDKs; no total limit, so a long stream is not cut.*
*- `zygo serve`: request-header read 60 s, the nginx `client_header_timeout` default; at most 512 open client connections, the nginx `worker_connections` default; a connection over the limit waits in the accept queue.* *(Note 2026-11-19, N60 plan review: `zygo serve` also bounds the request body read at 60 s between reads, the nginx `client_body_timeout` default, so a stalled upload does not hold a connection slot forever. A client that stops reading the response is not bounded in v1.)* *(Note 2026-11-19, N60 review: `zygo serve` speaks HTTP/1.1 only, on plain HTTP and over TLS (ALPN `http/1.1`), so the 60 s header bound also covers an idle kept-open connection; the agents' HTTP clients use HTTP/1.1. The body bound covers every route. At start, `zygo serve` raises its soft open-file limit to the hard limit on unix, as Go's runtime does, so 512 clients and their upstream sockets fit.)*
*- Serve client control calls (config, plugins, session create and delete, heartbeat, rows fetch, `zygo doctor` health check, `zygo usage --sync`): connect 10 s, 30 s per call; a connect failure or timeout is "server unreachable" (exit 12 at launch).*
*- The server's wait for a session's queued usage rows before it answers the rows fetch: 25 s, then it answers with the rows written so far.*
*- The agent `--version` probe: 2 s, as the act-lite probe (§10.5).*
*- `zygo update`: connect 10 s, read-idle 30 s between bytes, no total limit; it ignores the system proxy (N36).)*

### 7.3 Request path

Routes: `POST /anthropic/v1/messages`, `POST /anthropic/v1/messages/count_tokens`, `GET /anthropic/v1/models`, `POST /openai/v1/responses`, `POST /openai/v1/chat/completions`, `GET /openai/v1/models`. Session routes: `/v1/zygo/sessions…`, `POST|DELETE /v1/zygo/sessions/{id}/compact`, `GET /v1/zygo/sessions/{id}/status`, `PATCH /v1/zygo/sessions/{id}/settings` *(A2: session overrides of proxy-side settings, §17.1)*, `GET /v1/zygo/config`, `GET /v1/zygo/plugins` (§15.3), `GET /v1/zygo/sessions/{id}/requests` (§14), `GET /healthz`.

Stages, in order:

| # | Stage | Does |
|---|---|---|
| 1 | Identity | Session token + session header (§7.2), or passthrough identity (§12). |
| 2 | Resolution | `[1m]` suffix stripped and remembered; name → role, custom role or catalog model id (§7.4); profile applied; candidate providers. |
| 3 | Meter | Arms the usage tee on the response (§14) and the compaction signal (§10.2). |
| 4 | Cache | Provider pin; cache markers (§10.4). |
| 5 | Provider | Drops disabled providers and providers without a key; applies flags (§7.5). |
| 6 | Redaction | Secret pass, then content pass (§16). Last stage before the body leaves. |
| — | Proxy | Model rewrite, translation (§7.6), forward, fallback to the next provider on connect error, 5xx or 429 before any byte was streamed; response headers. |

The body is parsed once (JSON) and passed between stages as a parsed value; a body that no stage changed is forwarded byte for byte. Request body limit: 32 MiB (`413 zygo_body_too_large`). In-flight request memory is bounded per process (`[proxy] max_inflight_mib = 512`), counted as 4 × the raw body size to cover the parsed value; a request that would exceed it waits up to 5 s, then gets `503 zygo_busy`.

**Outbound headers.** The proxy never forwards the client's `authorization`, `x-api-key`, any `x-zygo-*` header or the `x-claude-code-*` hint headers upstream; it sets the provider's own credential. It forwards `content-type`, `accept`, and, to a provider that speaks the `anthropic` dialect, `anthropic-version` and `anthropic-beta` (the body may carry fields that need the beta flag). A provider may narrow or widen this with `forward_headers = [...]` in its entry. Passthrough keeps the client's `authorization` (§12.4). *(Note 2026-11-18, N11 planning: `forward_headers` can never forward `authorization`, `x-api-key`, `x-zygo-*`, `x-claude-code-*`, nor the framing and routing headers `host`, `content-length`, `transfer-encoding`, `connection`, `keep-alive`, `te`, `trailer`, `upgrade`, `proxy-*`, `cookie`, `accept-encoding` (RFC 9110, RFC 9112). The provider credential header is marked sensitive. The upstream client uses the system proxy settings and follows no redirect; a provider `base_url` with `http://` is refused unless its host is loopback (C3: the key would travel in clear text). Fallback retries only a connect error, a 5xx or a 429; a send error after request bytes went out ends the attempts with `503 zygo_provider_unavailable`. A model whose providers are all disabled gets `503 zygo_provider_unavailable`; `400 zygo_no_provider` means the catalog row lists no provider for the dialect.)*

**`count_tokens`.** Forwarded when the served provider speaks `anthropic`; otherwise answered locally with an estimate (`ceil(UTF-8 bytes of the strings / 4)`) as `{ "input_tokens": <n> }`, recorded `count_estimated`.

Error bodies use the client dialect's error shape with `type` = `zygo_<code>`. *(Note 2026-11-18, N10 planning: the proxy's own errors on the session API: a refused body → `400 zygo_bad_request` (an unknown field is named, no value is echoed); a session change against a §6.1 layer-7 limit → `403 zygo_setting_refused` naming the limit; an unknown route → `404 zygo_not_found`; a wrong method → `405 zygo_method_not_allowed`; over the 32 MiB body limit → `413 zygo_body_too_large`. `/v1/zygo/…` and `/healthz` have no client dialect and use the Anthropic shape. Error messages are fixed texts and never echo a header or token.)*

Response headers added: `X-Zygo-Model` (the served model id), `X-Zygo-Role` (role served), `X-Zygo-Adjustments` (codes known when headers are sent), `X-Zygo-Redacted: 1` when a redaction happened. *(Note 2026-11-18, N11 planning: from the upstream response the proxy drops the hop-by-hop headers (RFC 9110 §7.6.1), `location`, `set-cookie` and every `x-zygo-*` before it sets its own, so no provider URL reaches the client (§7.2).)* *(Note 2026-11-19, N12 planning: `X-Zygo-Redacted: 1` is set when a redaction pass found something to replace (a content timeout alone does not set it). A refused request (`422 zygo_redaction_block`, `503 zygo_redaction_unavailable`) carries no `X-Zygo-*` header; its findings are on its usage row.)*

*(Note 2026-11-19, N16 planning: `GET /v1/zygo/config` replies with config key names (`main_role`, `role`, `profile`, `redaction`, `guards`, `limits`, `features`, `marketplace`, `results`, `navigation`, `model`); `GET /v1/zygo/plugins` → `{ "marketplace": [...] }`; `GET /v1/zygo/sessions/{id}/requests` → `{ "requests": [<row>…] }`, `[]` for a same-label session with no rows, `401 zygo_session_unknown` for an id that names no session and has no rows or for another label's session; no row cap; the usage database not opening → `503 zygo_usage_unavailable`. `POST /v1/zygo/sessions` with `"passthrough": true` in serve mode → `403 zygo_passthrough_local_only` "passthrough is local mode only".)*

*(Note 2026-11-19, whole-branch review: (a) a body with a repeated object key is never forwarded as the received bytes; it is serialized from the parsed value that both redaction passes scanned. (b) The usage row's `role_requested` never holds text the secret pass would change: it is recorded after the secret pass, redacted. (c) In serve mode, response headers that name the provider account (`openai-organization`, `openai-project`, `anthropic-organization-id`) do not reach the client (§7.2). (d) A request the translator cannot map is a client error (400), not 503.)* *(Note 2026-11-19, N69: a body holding an object key that unescapes to a serde_json private token (`$serde_json::private::RawValue`, `$serde_json::private::Number`) is refused `400 zygo_bad_request`, history included; such a key in an earlier message makes every later request of that conversation fail until it is removed. Accepted: the alternative expands the token before the scan.)*

### 7.4 Resolution

A request's `model` is resolved in this order:

1. A role name (`lead`, …) → the profile's override for that role if valid (§11.3), else the role's model; a role the profile disabled → `400 zygo_role_disabled`.
2. A custom role of the profile → its model.
3. A catalog model id → that model (a direct request; role recorded as `direct`).
4. Anything else → `400 zygo_unknown_model`.

The served model must have a provider that speaks the request's dialect, or one whose dialect `zygo` can translate to (§7.6); else the role's `dialect_models` entry is tried; else `400 zygo_no_provider`. Candidates are the model's enabled providers in config order, the pinned one first (§10.4).

`GET /<dialect>/v1/models` lists, for the session: each role (`id` = role name, `display_name` = role name, `description` = role description, plus ` · profile <name>: <model>` when overridden), each custom role, and each catalog model reachable on that dialect. Disabled roles are not listed.

### 7.5 Provider flags (optional)

- `zdr = true` on a provider whose host is `openrouter.ai` (or a subdomain): every request to it gets the top-level body field `provider.zdr = true` (an existing `provider` object keeps its keys; a client value `false` is overwritten). On other hosts `zdr` is recorded only.
- `[proxy] require_zdr = true` (default `false`): providers without `zdr = true` are dropped at stage 5; no provider left → `503 zygo_provider_unavailable`.
- The flags table is open for later keys; an unknown key is a `config check` warning, not an error.

### 7.6 Translation and model facts

- Supported pairs in v1: `anthropic` client → `openai` (Responses) upstream; `anthropic` client → `openai-chat` upstream (most local and self-hosted endpoints speak only chat/completions; the `openai` pair is preferred when a provider speaks both). Both v1 agents send `anthropic` (§8.5, §8.7).
- **v2 (with Codex):** the client routes `POST /openai/v1/responses`, `POST /openai/v1/chat/completions`, `GET /openai/v1/models`, the `openai-chat` client → `anthropic` upstream pair, and `dialect_models` (§6.4). v1 answers these routes `404 zygo_route_not_in_v1`. **(author; no v1 agent uses them)** Streaming both ways (SSE event mapping), tool calls, tool results, images, system text, usage fields. A request to a provider that speaks the client's dialect is not translated.
- **Reasoning round trip.** To a Responses upstream the proxy sends `store: false` and `include: ["reasoning.encrypted_content"]`. Each returned reasoning item becomes an Anthropic `thinking` block whose `signature` carries the provider name, the `upstream_model`, the item's id and its encrypted content (encoded `zygo1:<base64>`); on the next request such a block is decoded back into the reasoning item in its original position when the request goes to the same provider and `upstream_model`. On every path, translated or not, a `thinking` block whose signature does not belong to the request's target (an `zygo1:` block for another provider or model, or a native signature on a translated request) is dropped and recorded `thinking_dropped:<n>`; this covers fallback, a moved pin, a profile change and a `/model` switch. Toward a chat/completions upstream thinking is dropped the same way. **(author; the builder verifies the round trip in a tool loop against each vendor.)**
- **Block types without a counterpart.** Anthropic server tools (tool definitions with a `type` other than `custom`, and `server_tool_use` / `*_tool_result` blocks) are removed on a translated request, recorded `server_tools_dropped:<n>`. `document` blocks: a base64 PDF becomes a Responses `input_file`; any other document, and every document toward chat/completions, becomes the text `[document omitted: the model cannot read it here]`, recorded `documents_dropped:<n>`. Any other unknown block type is removed and recorded `blocks_dropped:<type>:<n>`. Nothing is silently lost.
- **Mid-conversation system messages.** A `messages` element with `"role": "system"` is mapped to the upstream's instruction form (Responses `developer` item, chat `system` message).
- **Context-limit errors.** An upstream refusal for an over-long prompt is returned to an `anthropic` client as `400` `invalid_request_error` with the message `prompt is too long: <n> tokens > <max> maximum`, the wording Claude Code's reactive compaction recognizes (https://code.claude.com/docs/en/errors). Other upstream errors keep their status and are mapped to the client dialect's error shape. *(Note 2026-11-19, N13 planning (author's call): the numbers are taken from the upstream error when it gives them (`n_prompt_tokens`, or the count after "resulted in" / "requested"; `n_ctx`, or the count after "maximum context length is"). Otherwise `n` = the request's token estimate (§7.3 `count_tokens`: ceil(UTF-8 bytes of the strings / 4)), and `max` = the upstream's limit when the error text gives one, else `n − 1`. *(Corrected 2026-11-19, N13 round-2 review: whenever `max` is known, `n` = the larger of that `n` and `max + 1`, so `n > max` always holds.)* The same mapping applies to an in-stream error event. In a mapped error message, the provider's key value is replaced with `[REDACTED]`.)*
- On a translated request the model facts apply, each change recorded as an adjustment code:

| Fact | Behavior | Code |
|---|---|---|
| Effort | Read the client's effort (Messages `output_config.effort`, `thinking.type = disabled` → `none`; chat `reasoning_effort`). Absent → send nothing. Else clamp to `effort_levels` (nearest by ladder; the lower on a tie). Write as Responses `reasoning.effort`, chat `reasoning_effort`, or Messages `thinking`/`output_config.effort` (`none` → `thinking.type = disabled` only for `switchable`). `reasoning = none` → nothing sent. | `effort_clamped:<from>><to>`, `effort_dropped` |
| `max_output_tokens` | Lower the request's maximum to it. | `max_output_clamped:<from>><to>` |
| `sampling = false` | Drop `temperature`, `top_p`. | `sampling_dropped` |
| `vision = false` | Replace image blocks with the text `[image omitted: the model does not accept images]`. | `images_dropped:<n>` |
| `forced_tool_choice = false` | A forced single tool becomes Responses `required` / Messages `auto`. | `tool_choice_widened` |
| `structured_output = none` | The output schema is not forwarded; it is appended to the system text as "Answer with one JSON object that fits this schema: …". | `structured_output_prompted` |
| no facts | Translate without these rules. | `facts_missing` |

*(Note 2026-11-19, N14 planning, Effort row: `thinking.type = disabled` takes precedence over `output_config.effort`; an effort value not on the ladder is recorded `effort_dropped`; with no facts, no effort is sent. A `max_output_tokens` of 0 (not stated) is not applied.)*

- **Strict schemas.** `[proxy] strict_schemas = false` (default). Strict mode on both vendors refuses some JSON Schema constructs. `[[schema_rule]]` entries (`family = "anthropic" | "openai"`, `code`, `severity = "refuse" | "warn"`, optional `limit`) name them; on a translated request (and on every request when `strict_schemas = true`) each `refuse` rule rewrites its construct in tool schemas and output schemas in a fixed order (`array_items_required`, `empty_schema`, `map_object`, `one_of`, `all_of`, `conditional`, `closed_object`, bounds and pattern moves, `optional_property`/`optional_properties_max`), recording `schema:<code>:<n>`; a rewritten optional property's injected `null` is removed from tool inputs and structured answers on the way back, so the client sees its own shape. The built-in rule set and its order are data **(author)**; the builder verifies each rule against both vendors' strict modes and records the evidence in the test. *(Note 2026-11-19, N14 planning: the rule family is the upstream vendor's (`anthropic` for an `anthropic` provider, `openai` for `openai` and `openai-chat`). Facts apply before the schema rewrite, so a prompted output schema is not rewritten. A bounds, pattern or conditional rule moves its keys into `description` as compact JSON; `one_of` renames to `anyOf`; `all_of` merges its branches recursively (object maps such as `properties` key by key, the same property's schemas with the same merge, `required` united, equal values kept), and an `allOf` whose branches give one key two values that cannot be merged is left unchanged and not counted; the two optional-property rules make the property required and nullable, except a property that already accepts null (`type` `"null"`, a `type` array with `"null"` whose `enum` and `const`, when present, also accept null, an `anyOf`/`oneOf` branch that accepts null, or, with no `type`, an `enum` that contains null or `const: null`; the schema `true`; the empty schema `{}`), which gets no injected null; a null path is recorded only where a null was injected. Only an injected null is removed on the way back, from tool inputs and from JSON text answers. On an untranslated stream every line passes unchanged except the deltas of a block from which a null was removed; a ping, comment or other non-delta event inside a held block is held in place, so a block with no removed null goes out byte for byte, and a rebuilt block keeps its non-delta events in order with the one rebuilt delta where the first delta was; a held block over 4 MiB (all held lines) is flushed unchanged. Codes with no rewrite (`array_items_required`, `empty_schema`, unknown codes) are not applied; optional properties under `$defs`/`definitions` are not rewritten; translated Responses tools keep `strict: false`.)*

### 7.7 Audit of adjustments

Each request's adjustment codes go to the usage row (§14) and, when known at header time, to `X-Zygo-Adjustments`. Codes carry names and counts, never content.

## 8. Launcher and agent wrapping

### 8.1 Capability list

Every agent has a conformance row with each capability marked **full**, **partial** (what is missing) or **none** (why). The row is written, with the agent's documentation URL per cell, before its launcher is built (C9), and published in the docs.

| Id | Capability | Standard |
|---|---|---|
| A-1 | Transport | Base URL → the proxy's dialect prefix; credential = the session token through the agent's key helper; the agent's own login and provider selection off (except passthrough). |
| A-2 | Attribution | `X-Zygo-Session`, `X-Zygo-Agent-Version`, `X-Zygo-Repo`, `X-Zygo-Work` (when set) on every request. |
| A-3 | Model list | The picker shows the session's role list (§7.4); the default model and every alias the agent has are roles (§8.5). |
| A-4 | Subagent roles | Every built-in agent/subagent the agent has maps to a role; an `zygo-<role>` subagent exists per enabled role with `model = <role>`. |
| A-5 | Per-session config | All `zygo` settings reach the agent through a per-session mechanism (a settings file passed by flag, an inline config env), never by editing the user's own files. |
| A-6 | Local services | `zygo mcp` registered as a stdio MCP server. |
| A-7 | Result trimming | Large tool results go through the result store (§10.1). |
| A-8 | Session context | The session text (§8.6) injected at session start. |
| A-9 | Status | Profile, work item, context use, compaction override and session savings on the agent's status surface, else `zygo status`. |
| A-10 | Secrets hygiene | The env denylist (§8.4). |
| A-11 | Skills and plugins | The session's plugin snapshot delivered in the agent's own form (§15.3). |
| A-12 | Model profile | `--profile <name>` applied through the session (§11). |
| A-13 | Version | A minimum agent version per agent in the built-in config; `zygo doctor` reports the installed version. |
| A-14 | Role prompts | The main role's prompt opens the session text; each `zygo-<role>` opens with its role's prompt. |
| A-15 | Lead guard | When the main loop runs on `main_role`, it cannot use the agent's file-edit tools (§8.3); subagents keep theirs. |
| A-16 | Compaction window | The compaction signal (§10.2) or an equivalent agent setting; `zygo compact` works from the agent's shell. |
| A-17 | Model-triggered compaction | The `compact` MCP tool (§10.3). |
| A-18 | Compaction instructions | `zygo`'s keep/drop text reaches the agent's pre-compaction mechanism (§10.6). |
| A-19 | Work units and guards | §13 hooks. |
| A-20 | Role tool restriction | A `writes = false` role cannot use the edit tools, by definition, not by prompt; an optional turn cap. |
| A-21 | Tests | A stub-agent launch test on Linux, macOS and Windows asserting the environment, flags and generated config; a premise test that reads the pinned agent's `--help` or schema for every setting key `zygo` writes. *(Note 2026-11-18, N2, Q7: on Claude Code 2.1.328 and OpenCode 1.18.16 every key, env row and flag zygo writes is in the pinned `--help` or schema, or in the binary's validation strings for `modelPicker`, and its effect was observed. The managed keys zygo only reads in the §8.4 preflight — `allowManagedHooksOnly: true` and a managed `disableAllHooks: true` — are in the schema but their effect was not observed: they act only from the system managed-settings file, which needs administrator rights, and 2.1.328 ignores `CLAUDE_CODE_MANAGED_SETTINGS_PATH`. Q7's `failed` means exactly this, cannot be observed without administrator rights, not a contradicting observation; the premise test for these keys checks the schema only.)* |
| WA-1 | Resume (workspace members) | Pre-assign or capture the agent session id; resume the conversation by that id (workspaces design AU28, W§8.2, W§8.3). |
| WA-2 | Turn signals (workspace members) | Turn start and end, subagent start and end, and the dispatch block, as hook or plugin events (workspaces design AU28, W§9.1). |

v1 target rows: Claude Code full on every capability. *(Note 2026-11-18, N39 planning: OpenCode A-19 is partial — its plugin API exposes no subagent-stop event and no agent id (§13.6), so the claims and scope guards cannot run on OpenCode.)* OpenCode: A-8 full via its plugin context hook; A-16 partial (agent setting, §10.2.5); A-17 none (no gateway-visible tool name convention verified; listed as a backlog premise); A-18 none unless its plugin API exposes a pre-compaction hook; A-20 partial (no turn cap). The builder verifies each cell against the pinned agent version before coding and corrects this table in the docs. *(Note 2026-11-18, N2, Q2: OpenCode 1.18.16 cells as observed — A-8 full: the plugin hook `experimental.chat.system.transform` adds text seen in the request's system. A-16 partial: a reply whose usage is over the model's `limit.context` made OpenCode send a compaction request; missing: no proxy signal, and the window is a launch-time setting. A-17 none in v1: MCP tools reach the request as `<server>_<tool>`, so a `zygo_compact` call is visible to the proxy, and OpenCode compacts on a usage over its window, but the SSE rewrite is not armed for OpenCode, §10.2.5. A-18 full: text pushed to `output.context` in `experimental.session.compacting` appears in the compaction request. A-20 full: `permission.edit: deny` removes the edit and write tools from the agent's request, and an agent's `steps` caps its tool iterations — at the cap OpenCode adds a "MAXIMUM STEPS REACHED … text only" message, the tools stay listed. `fixtures/opencode/`.)*

WA-1 and WA-2 do not continue the ids A-1 to A-21; `docs/conformance.md` has them in their own table, in the same cell form. Every cell is partial: Claude Code WA-1 (a resume with another `CLAUDE_CONFIG_DIR` finds no conversation, PW16; zygo saves and reuses the config folder), Claude Code WA-2 (a turn ended by Esc sends no `Stop` or `StopFailure`, and internal subagents send `SubagentStop` without `SubagentStart`, PW7), OpenCode WA-1 (the resume is verified with `opencode run` only) and OpenCode WA-2 (no subagent events; the busy status that starts a turn is not in a fixture); the OpenCode evidence is from OpenCode 1.19.6 (PW4) (workspaces design AU28, AU104).

### 8.2 Launch sequence (both agents)

1. Load config (§6.1; in serve mode, fetch the team config first, §7.2), resolve the profile (§11), resolve repository id and work item (§13.2). **Repository id:** the first `remote.origin.url` from `git config --get-all` (2 s bound; *Note 2026-11-18, N22 planning: read with `--local`, so a global config outside a repository gives none*), normalized: drop scheme, userinfo and port; read scp form `host:path` as `host/path`; drop a trailing `/` and `.git`; lower-case; it must match `^[a-z0-9.-]+(/[a-z0-9._-]+)+$` and be ≤ 128 bytes, else none. No remote or outside a repository → none (no path is ever sent). **Workspace member:** the CLI layer decides membership before the launch: a launch that `zygo up` starts in a workspace pane (`zygo ws run`) is a member, and a hand start in a workspace pane is one when it passes the membership checks (workspaces design AU102, W§8.4). For a Claude Code member with no session argument of the user's own, the launcher adds `--session-id=<new UUID v4>` (one token) before the user's arguments (W§8.3); a resume adds `--resume=<id>` for Claude Code or `--session=<id>` for OpenCode (W§8.2 step 3).
2. Create the session (§7.2); create the session directory `~/.cache/zygo/sessions/<session>/` (`0700`). *(Note 2026-11-18, N10 planning: the launcher writes the proxy base URL to `$ZYGO_SESSION_DIR/url`, next to the token file `token` (§8.4); `zygo` subcommands and hooks read both from the session directory.)* *(Note 2026-11-19, whole-branch review: the server's reply is untrusted. The launcher accepts the session id only when it is non-empty ASCII alphanumeric (the rule the later calls already apply) and the token only when it is a valid header value; otherwise it stops before it creates any file. SIGINT, SIGTERM and SIGHUP are handled from before the directory exists until step 6 ends, so a signal in steps 2–4 or 6 still runs the cleanup.)* After the directory exists and before step 2a, the launcher calls the session callback that the CLI layer sets: for a member it writes the member record, makes the `results` link (§10.1) and applies the saved overrides (workspaces design AU34, AU92, W§8.2 step 4). An error from the callback is logged and the launch goes on (C6); a hand-start callback that returns `NotMember` makes the launch a plain launch before the view, the settings and the agent environment are built (W§8.4). The session view holds the membership marker and the resume note inputs from `LaunchOpts` (AU58, AU72).
2a. With `--compact <n|off>`, set the session override (`POST /v1/zygo/sessions/{id}/compact`) before the agent starts.
3. Sync and snapshot plugins (§15).
4. Build the agent's per-session config (§8.5 Claude Code, §8.7 OpenCode) and child environment (§8.4); run the preflight (§8.4) and stop with exit 2 when it fails.
5. Start the agent as a child process with the user's arguments after `--`; wait. While the child runs, the launcher ignores SIGINT and SIGQUIT (Windows: `CTRL_C_EVENT`, `CTRL_BREAK_EVENT`), which the terminal already delivers to the child's process group, so Ctrl+C reaches the agent once and does not stop the in-process proxy; it forwards SIGTERM and SIGHUP to the child. *(Note 2026-11-19, N57 planning: a signal that is already ignored when the launcher starts (`nohup`, a background job) stays ignored and is neither caught nor forwarded, so the agent inherits it ignored, as it would without zygo.)* For a workspace member the wrapper then classifies the exit: a launch that received SIGTERM or SIGHUP, an agent ended by a signal, or a resume that failed fast (a non-zero exit less than 10 s after the spawn, with no signal) keeps the member resumable; any other exit after a spawn ends it; an exit before the spawn keeps the member as it was (workspaces design AU17, AU60, AU73, AU92, W§8.2 step 7).
6. At exit: end open work units (§13.3), delete the session (§7.2), remove the session directory except `results/` (swept after 24 h, §10.1), print nothing unless asked. Work units also end at exit for a member (workspaces design AU31). A member's `results` is a link into the workspace cache, so its session directory stays until the sweep removes it (§10.1; AU16, AU69).

Exit codes: `0`; `2` usage; `11` config invalid; `12` server unreachable; `13` agent binary not found (prints the install hint); else the agent's own code. *(Note 2026-11-19, N31 planning: a child killed by a signal gives 128 + signal; the launcher does not re-raise.)*

### 8.3 Lead guard

- When the main loop's effective role equals `main_role`, the main loop's file-edit tools are refused; subagents are not affected. Effective role: the role picked by the user's model flag (`--model <v>` for Claude Code, `-m <provider>/<v>` or `--agent <name>` for OpenCode) when it names a role or custom role; a direct model id → no role (guard off, no main prompt); nothing picked → `main_role`.
- Claude Code: `PreToolUse` hook on `Edit|Write|NotebookEdit` → `zygo hook role-guard`. If the hook input has an `agent_id` key (any value, `null` included: the call is inside a subagent) or the tool is another one: no output. Else: `{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"zygo: the lead does not edit files. Delegate this change to the zygo-code subagent with the files, the change and how to check it."}}`.
- OpenCode: the agent mapped to `main_role` (`build`) gets `"permission": { "edit": "deny" }`.
- Known limit, documented: shell writes (redirection, `sed -i`) and MCP write tools are not blocked; the guard removes the habitual edit path, not a determined one.
- `zygo` merges each readable user `--settings` value into its settings file and passes no user value on (§8.5 Flags), so the guard stays on. A value that `zygo` cannot read as a JSON object is not used. `zygo` prints one line for the merge and one line for each value it does not use.

### 8.4 Environment

- **Denylist** on the child environment, limited to model-provider credentials and agent routing so that the agent's normal work (`gh`, package managers, cloud tests) keeps its own tokens: `ANTHROPIC_*`, `OPENAI_*`, `OPENROUTER_*`, `CLAUDE_CODE_OAUTH_TOKEN`, `CLAUDE_CODE_USE_BEDROCK`, `CLAUDE_CODE_USE_VERTEX`, `CLAUDE_CODE_USE_FOUNDRY`, every variable a catalog provider names in `key = "env:…"`, `ZYGO_TEAM_TOKEN`, and the Claude Code session markers `CLAUDECODE`, `CLAUDE_CODE_CHILD_SESSION`, `CLAUDE_CODE_SESSION_ID` and `CLAUDE_CODE_BRIDGE_SESSION_ID` (workspaces design AU64) (except the variables `zygo` sets itself). Names match case-insensitively on Windows. The names removed (never values) go to the session log. *(Note 2026-11-19, N37 plan review: the denylist also removes the variable that `[server] token = "env:<NAME>"` names, so a team token under any name stays out of the agent's environment.)*
- **Preflight (Claude Code).** The launcher reads the user, project and local settings files (`~/.claude/settings.json`, `.claude/settings.json`, `.claude/settings.local.json`) and the managed settings file. `zygo` finds these files by itself, and a cloned repository can put anything at their paths, so each is read by a safe rule: links are followed; a target that is not a regular file (a FIFO, a socket, a device, a directory) is skipped with no line and is not opened; a regular file is opened, on unix with `O_NONBLOCK` and, when that open fails because it would block (a file lease), once more without it, with no loop; the open handle must again be a regular file, else the file is skipped; `O_NONBLOCK` is then cleared and the file is read whole, with no size limit and no time limit. A file that is absent, is skipped, cannot be read, is not JSON or holds JSON that is not an object is skipped with no line. The passthrough check of the managed file (§12.3) reads it the same way. Residual risks, with no retry loop and no time limit: a FIFO swapped in between the metadata read and the open is opened, but with `O_NONBLOCK`, and the check on the handle skips it, so the launch does not wait; after a lease makes the first open fail, the second open has no `O_NONBLOCK`, so a FIFO swapped in at that moment with no writer makes the launch wait until a writer opens it (this needs a lease holder and the right to rename in that folder; Linux `O_PATH` could close it, but it is not used because macOS has no `O_PATH`); on Windows the metadata read itself opens the target, with no access rights; a FUSE mount that does not answer makes the metadata read wait; on Windows the type check cannot tell a named pipe or a device from a regular file, so such a target found in a repository is opened and read, and a pipe server that does not write makes the preflight wait. If any sets, in `env` or as a key, `ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN`, `ANTHROPIC_BASE_URL`, `apiKeyHelper` (outside passthrough this is replaced by `zygo`'s file; it is listed for passthrough, §12.3), or a `CLAUDE_CODE_USE_*` provider switch, the launch stops with exit 2 naming the file and key: these outrank or bypass the session's key helper, so the agent would send another credential to the proxy or skip the proxy and its redaction. The session settings set `disableAllHooks: false`; a managed file that sets `disableAllHooks: true` or `allowManagedHooksOnly: true` also stops the launch (exit 2), because `zygo`'s hooks, guards, trimming and status line would silently not run (https://code.claude.com/docs/en/hooks). `--settings` is the highest non-managed scope, so `zygo`'s own `env` rows win for the keys it sets; the preflight covers the credential keys it cannot unset. **(author)** *(Note 2026-11-19, N32 planning: the user settings file is `$CLAUDE_CONFIG_DIR/settings.json` when that variable is set; the project and local files are read in the current directory and, when it differs, at the git repository root; setting keys match exactly, names inside `env` match without case on Windows. Only the managed settings file is read: a `managed-settings.d/` folder, MDM or registry policy and server-managed settings are not checked. A managed `apiKeyHelper` stops the launch in every mode, because managed settings outrank `--settings` and would replace zygo's key helper.)*
- **Set:** `ZYGO_SESSION`, `ZYGO_SESSION_DIR`, `ZYGO_AGENT` (`claude-code` | `opencode`), `ZYGO_URL` (proxy base URL), `ZYGO_REPO`, `ZYGO_WORK`, `ZYGO_MAIN_ROLE` (effective role, empty for a direct model id). For a workspace member also `ZYGO_WORKSPACE` (the workspace id) and `ZYGO_PANE` (the pane key); every launch that is not a member removes both from the child environment, and hooks take membership only from the session view's marker (workspaces design AU58). The session token is not in the environment: hooks and `zygo` subcommands read it from `$ZYGO_SESSION_DIR/token` (`0600`) **(author)**. Accepted limit: tools run as the same user and can read that file (and OpenCode's inline config, §8.7). The token reaches only the session's proxy, and in serve mode it stops working when the launcher stops (§7.2).

### 8.5 Claude Code

- **Environment:** `ANTHROPIC_BASE_URL = <proxy>/anthropic`; `ANTHROPIC_MODEL = <main_role>`; aliases `ANTHROPIC_DEFAULT_FABLE_MODEL = design[1m]`, `ANTHROPIC_DEFAULT_OPUS_MODEL = lead`, `ANTHROPIC_DEFAULT_SONNET_MODEL = code`, `ANTHROPIC_DEFAULT_HAIKU_MODEL = read`, `CLAUDE_CODE_SUBAGENT_MODEL = code`; `CLAUDE_CODE_GATEWAY_HINT_HEADERS = 1` (the agent then sends `x-claude-code-request-class`, `x-claude-code-agent-type`, `x-claude-code-agent-id`, `x-claude-code-compaction`); `ANTHROPIC_CUSTOM_HEADERS` = the A-2 headers; `DISABLE_LOGIN_COMMAND = 1`, `DISABLE_LOGOUT_COMMAND = 1`. When the profile disables the role an alias points at, the alias is redirected to the first enabled role in the order `lead`, `code`, `read`, `main_role` (`main_role` cannot be disabled, so one always exists).
- **Alias table** (docs): `fable`/`best` → `design`; `opus`/`opusplan` → `lead`; `sonnet` → `code`; `haiku` and background tasks → `read`; subagent without a model → `code`.
- **Per-session settings file** `$ZYGO_SESSION_DIR/settings.json`, passed with `--settings`: `apiKeyHelper = "zygo hook token"`; `model = <main_role>`; `availableModels` = the session's role list; `modelPicker = { "options": [ … ] }` with one row per role and custom role: `model` = role name, `label`, `description`, and `behavesAs = <the role's model id>` when that model is a Claude id *(Note 2026-11-18, N2, Q3: row shape observed on Claude Code 2.1.328. Observed in runs: rows with `model`, `label`, `description` and `behavesAs` are accepted and `behavesAs` takes effect; a row with `id` instead of `model` is ignored with a notice; a bare array instead of `{ "options": … }` is ignored. Seen only in the binary's validation strings, not exercised: `replaceBuiltInOptions` (boolean; without it the rows are added to the built-in lineup). Observed with `availableModels` set to the role list: `--model claude-haiku-4-5`, not in the list, is refused and the session uses the default model. `fixtures/claude-code/p2/`, `fixtures/claude-code/settings-binary-excerpt.txt`, `fixtures/claude-code/q7/q7-claude.json`.)*. Reason: Claude Code turns on effort levels and thinking, and sizes the context window, by recognizing the model id; behind a gateway a role name is not recognized, and `behavesAs` makes Claude Code treat it as that Claude model. A role whose model is not a Claude id has no `behavesAs`; Claude Code then assumes a 1M window (v2.1.323 and later) and the soft window covers it (§10.2.1); `statusLine = { "type": "command", "command": "zygo hook statusline", "refreshInterval": 30 }`; hooks: `PostToolUse` (`Bash|Read|Grep|Glob|WebFetch` → `zygo hook trim`), `SessionStart` → `zygo hook session-start`, `PreToolUse` `Edit|Write|NotebookEdit` → `zygo hook role-guard` (when §8.3 applies), `UserPromptSubmit` → `zygo hook prompt`, `PreCompact` → `zygo hook pre-compact`, the §13 work hooks when work units are on, and, for a workspace member only (workspaces design W§9.1), `SessionStart` → `zygo hook session-start --event member` in place of the plain entry, a second `UserPromptSubmit` entry → `zygo hook workspace --event prompt`, `PreToolUse` `*` → `zygo hook workspace --event pre-tool`, `PostToolUse` `*` and `PostToolUseFailure` `*` → `zygo hook workspace --event tool-end`, `Stop` and `StopFailure` (AU57) → `zygo hook workspace --event stop`, `SubagentStart` → `zygo hook workspace --event subagent-start`, `SubagentStop` → `zygo hook workspace --event subagent-stop` and `SessionEnd` → `zygo hook workspace --event session-end`; a member whose merged settings set `disableAllHooks: true`, or that is started with `--bare` or with `CLAUDE_CODE_SIMPLE` on (Flags), runs none of them, and the `zygo down` drain counts it as busy (AU26); `env` mirrors the environment rows above; in every mode, passthrough included, `attribution = { "commit": "", "pr": "", "sessionUrl": false }` and `includeCoAuthoredBy = false`, so Claude Code adds no co-author trailer, no "Generated with Claude Code" line and no Claude session trailer or link to a commit or a pull request (operator, 2026-11-22: there is never Claude attribution; `--settings` outranks the user, project and local settings, so a user or `--config-dir` setting cannot turn it back on, and a user `--settings` value is merged with these two keys kept, Flags). *(Note 2026-11-22, FIX-21 live check, Claude Code 2.1.330: without these keys Claude Code puts a reminder in the main request to end each commit message with a Claude co-author trailer that carries the Anthropic noreply address; in a zygo session, with or without `--bare`, no request holds it, and a commit made through `zygo claude --passthrough` holds no attribution. `--bare` drops the attribution text by itself (Claude Code alone with `--bare` sends none) and also skips every settings hook, the §13.4 guard included. `docs/live-checks.md` FIX21-LOGIN, FIX21-MOCK.)* *(Note 2026-11-19, N32 planning: every command zygo writes for the agent — hook commands, `apiKeyHelper`, `statusLine.command`, and the MCP server `command` — names the absolute path of the running zygo binary in place of the bare name `zygo`, so a zygo that is not on PATH still runs. In a shell command string the path is single-quoted on unix and double-quoted on Windows; the MCP `command` is the plain path.)*
- **Subagents:** `--agents` JSON with one `zygo-<role>` per enabled bundle role except `main_role` (no `zygo-lead`: the lead is the main loop) and per custom role: `description` = role description; `prompt` = role prompt + one space + the mechanics text (§8.6); `model` = role name; `disallowedTools = ["Edit","Write","NotebookEdit"]` when `writes = false`; `maxTurns` when a turn cap applies (§13.5). A user `--agents` value is merged; a user key with the same name wins. Each user `--agents` value must be a JSON object; with `--print`, a value that does not parse as JSON is read as a file path, relative to the working directory, the same way as a `--settings` file (§8.5 Flags), so a pipe such as `<(…)` can hold it, and the file must hold a JSON object. `zygo` reads each such file once per launch and uses that text for both the check and the merge, because a pipe can be read only once. A value that is neither stops the launch with exit 2 and `zygo: --agents: not a JSON object (or, with --print, a file that holds one)`.
- **Plugins:** `--plugin-dir $ZYGO_SESSION_DIR/plugins` (the snapshot, §15.3). The embedded `zygo` plugin (§15.5) is always in the snapshot.
- **MCP:** `--mcp-config` with `{ "mcpServers": { "zygo": { "command": "zygo", "args": ["mcp", "--agent", "claude-code"] } } }`.
- **Flags:** the user's arguments after `--` pass unchanged, except a user `--settings`. Flags that defeat a feature (`--settings`, `--bare`, `--model` with a direct model id) are recorded in the session log as degraded; other flags, `--dangerously-skip-permissions` included, are not. A start whose agent gets `CLAUDE_CODE_SIMPLE` on is bare mode by Claude Code's own rule (the value, lower-cased and trimmed, is `1`, `true`, `yes` or `on`, read from the agent's environment after the §8.4 denylist; `zygo` sets no `CLAUDE_CODE_SIMPLE`): the session log records `degraded: CLAUDE_CODE_SIMPLE`, as it records `degraded: --bare` (workspaces design AU26). Claude Code uses only the last `--settings` it is given (observed on 2.1.330), so `zygo` merges the user's values into its own file and passes only that file. Each user `--settings` value before `--` is read as Claude Code takes it: a value that parses as a JSON object is inline settings; any other value is a file path, relative to the working directory (observed on 2.1.330). A value is never judged by its first character. Known limit: the `serde_json` features `arbitrary_precision` and `raw_value` are on for the whole workspace, so in user settings, inline or in a file, a JSON object whose first key is `$serde_json::private::Number` or `$serde_json::private::RawValue` is read by `serde_json`'s rule for that key (as a number, or as the JSON text it holds), not as an object with a plain key; `zygo` does not change this. The values merge in order, a later key winning. In the merge, `attribution` and `includeCoAuthoredBy` stay `zygo`'s; per `hooks` event, the user's entries come first, then `zygo`'s (a user `hooks` value or event that is not an object or a list leaves `zygo`'s in place); per `env` name, a user name wins (a user `env` that is not an object leaves `zygo`'s in place); every other user key is kept and wins over `zygo`'s key of the same name, as a user `--agents` key does, except a user `disableAllHooks` that is not `true` or `false`, which `zygo` removes (see below). A user `disableAllHooks` of `true` or `false` is one of these other keys: a user `disableAllHooks: true` wins and stops every hook, `zygo`'s included (a `disableAllHooks: true` in `--settings` was observed to stop every hook on 2.1.328, the Q7 note after §19a), while `attribution` and `includeCoAuthoredBy` always stay `zygo`'s. A user `disableAllHooks` that is not `true` or `false` is not used: the settings schema types it as a boolean, settings that Claude Code could not load made it run with neither file (see below), and its handling of such a value was not observed. `zygo` removes it from that value before the merge, so a boolean `disableAllHooks` of another user value still counts by the merge order and, with none, `zygo`'s `false` stays; for each such value, in value order, `zygo` prints `zygo: your --settings disableAllHooks value is <type>, not true or false; it was removed`, where `<type>` is `a string`, `a number`, `an object`, `an array` or `null`. The value itself is never printed, because it can hold any text, for example a token. When at least one value is merged, `zygo` prints one merge line for the merged settings: `zygo: your --settings is merged into zygo's settings file; zygo's attribution keys and hooks stay in force`, or, in its place, when the merged `disableAllHooks` is `true` (a later user value `disableAllHooks: false` wins over an earlier `true`), `zygo: your --settings is merged into zygo's settings file; zygo's attribution keys stay in force; your disableAllHooks stops every hook and the status line, zygo's included` (the schema: "Disable all hooks and statusLine execution"). The merge line comes first, then the `disableAllHooks` lines, then the lines for values that could not be read, each group in value order. A value that is not a readable JSON object (no file at the path, a file that cannot be read, a file that is not JSON, JSON that is not an object) is not used and not passed on: the agent gets only `zygo`'s file, with the readable values merged, and for each such value `zygo` prints one line. The value text is in the line only when an entry exists at the path (`symlink_metadata` finds one), because a value with no entry (with a BOM or other prefix, an array, a JSON string, broken JSON of any length) can be inline settings that hold secrets. With no entry, when no file is found (`NotFound`, or on Windows a name the system refuses: `ERROR_INVALID_NAME`, `ERROR_BAD_PATHNAME`, or `ERROR_FILENAME_EXCED_RANGE` for a name that is too long), the line is `zygo: your --settings value (not shown) is not a JSON object and names no file; it was not used` on every platform; for any other error (for example a folder on the path that cannot be searched, a part of the path that is not a folder, a link loop in a folder part of the path, or on unix a name longer than the system limit) the line is `zygo: your --settings value (not shown) is not a JSON object and could not be opened as a file: <io error>; it was not used`, where `<io error>` is the system error text, which holds no path. With an entry (a link loop at the last part of the path and a dangling link are entries), `zygo` reads the path as it did before this rule: one blocking open that follows links, then the whole content, with no size limit and no time limit. So any type that the system can read is read, regular files, pipes and FIFOs (a `<(…)` path such as `/dev/fd/63`) and devices included; a writer that writes after the launch starts is read in full, and a FIFO with no writer makes the launch wait until a writer opens and closes it, because the value is the user's own input. When that read fails, the line is `zygo: your --settings value <value> could not be read as a JSON object and was not used: <error>`, where `<value>` is the value as typed, written as a compact JSON string in which every character that `zygo_core::esc` escapes is then written in `esc`'s form, and `<error>` is the system error text of the open or the read (for example for a directory or a file the user cannot read); `<category> error at line L column C` for a file that does not parse, where `<category>` is `syntax`, `data`, `eof` or `io` (`serde_json::Error::classify`); or `the file holds JSON that is not an object`. `<error>` is escaped the same way, so the line is always one line. `zygo` never prints the text of a `serde_json` error of user input: a data error can hold literals of the file, for example through the `$serde_json::private::Number` key. Reason: passed on after `zygo`'s file, such a value made Claude Code stop with "Settings file not found" (a missing file, or a value that is neither a JSON object nor a file) or, for a file that is not JSON, warn that those settings did not load and run with neither file, so the `attribution` keys and the hooks (the session text and the §13.4 attribution guard with them) were not in force (observed on 2.1.330; operator, 2026-11-22: there is never Claude attribution, and `zygo` must force it). A launch that a workspace starts with `--settings` stays a workspace member and gets the same merge (W AU59). `--yolo` before `--` maps to `--dangerously-skip-permissions`.

### 8.6 Session text

`zygo hook session-start` prints the session text as additional context. Order:

1. The effective role's prompt (`lead` by default; none for a direct model id).
2. The user prompt (§11.1), then the profile prompt, each verbatim, when set.
3. `Work item: <source>:<id> — <title>` when set.
4. The legend: one line per enabled subagent, `zygo-<role>: <description>`, custom roles after bundle roles.
5. Claude Code only, the alias sentence: "A skill names a role or a capability: pick the subagent by the legend. When a skill still names a model, use the subagent instead: `haiku` is `zygo-read`, `sonnet` is `zygo-code`, `opus` is the premium subagent that fits the task (`zygo-plan`, `zygo-triage` or `zygo-review`), `fable` is `zygo-design`. Do not set a model on the subagent call." Clauses for disabled roles are left out.
6. Capabilities: "`lookup` is `zygo-read`; `mechanical` and `implementation` are `zygo-code`; `judgment` is `zygo-design` for a design question and `zygo-plan`, `zygo-triage` or `zygo-review` for text." (disabled roles left out).
7. The mechanics text (also appended to each subagent prompt): the redaction sentence ("`[REDACTED:<id>]` marks a secret zygo removed; never write it into a file."), the attribution sentence, always, on both agents ("Never add Claude attribution to commits, pull request titles or bodies, tags, release notes or comments: no `Co-Authored-By: Claude` trailer, no `noreply@anthropic.com` address, no `Generated with Claude Code` line and no Claude session link."; operator, 2026-11-22), the result-store sentence ("Large tool outputs are stored by zygo; read them with result_view, result_search or result_tree using the id in the marker; do not shorten output with head or tail."), the navigation sentence when navigation is present (§10.5) ("Use act-lite's skeleton, symbols or repo_outline before reading a whole source file.") *(A1)*, the trailer sentence when work units are on ("Add the commit trailers `Zygo-Session: <session>` and, when your brief names a unit, `Zygo-Work: <unit>`, each on its own line at the end of the message.").
8. For a workspace member whose session view holds resume note inputs (workspaces design W§9.4, AU72), the resume note, after one empty line, one sentence per line: `zygo did not stop this session cleanly; the list of closed units was not saved.` when the stop was not clean (this sentence alone); else `zygo stopped this session while subagents <type> <id>, … were running; their results were lost; dispatch them again if still needed.` when subagents were interrupted, and `Units <unit>, … were closed when zygo stopped this session; call work_begin to open them again if work continues.` when units were closed. No note after a clean stop with both lists empty. Claude Code prints it once, at the first main-loop `SessionStart` of the session, then removes the inputs from the view; OpenCode adds it to every main-role request while the inputs are in the view (W§9.4, AU104).

Nothing in the text names a context window, a size or a price (C7).

### 8.7 OpenCode

- Config injected through `OPENCODE_CONFIG_CONTENT` (inline JSON) and `OPENCODE_CONFIG_DIR = $ZYGO_SESSION_DIR/opencode` (skills, plugin files). Premise, verified on the pinned OpenCode version before the launcher is built (A-21): the inline config outranks the user's and the project's `.opencode` configs for the keys it sets. Whatever the result, the preflight reads the project and user OpenCode configs and stops the launch (exit 2, naming the file) when one defines a provider named `zygo`, sets `model`, or sets an `agent.*.model` or `permission` key that `zygo` also sets. (OpenCode config docs: https://opencode.ai/docs/config/)
- Provider `zygo` with `npm = "@ai-sdk/anthropic"`, `options.baseURL = <proxy>/anthropic/v1`, `options.apiKey` = the session token, `options.headers` = the A-2 headers; `models` = each role and custom role, `limit.context` = the served model's `soft_context_window` when set, else its `context_window`, else 200,000 (§10.2.5); `model = zygo/<main_role>`.
- Agents: `build` → `lead` (with `permission.edit = "deny"`, §8.3), `plan` → `plan`, `general` → `code`, `explore` → `read`; `zygo-<role>` agents with `mode = "subagent"`, `prompt` (§8.6), `model = zygo/<role>`, `permission.edit = "deny"` when `writes = false`.
- `mcp.zygo = { "type": "local", "command": ["zygo", "mcp", "--agent", "opencode"] }`.
- Generated plugin `opencode/plugins/zygo.js`: `tool.execute.after` → result store (§10.1); session context hook → §8.6 text; `tool.execute.before` → guards (§13.4) for the tools it exposes. Each handler shells out to `zygo hook <name> --agent opencode` with the JSON on stdin and a 10 s timeout, and ignores failures (C6). *(Note 2026-11-19, N33 planning: the plugin also maps the session compaction event to `zygo hook pre-compact` and calls `zygo hook work` after each tool; it registers no `work_begin`/`work_verdict` hooks, since OpenCode gives no agent id. `hook_opencode::normalize` maps OpenCode tool names to the Claude Code names the hooks match (`bash` → `Bash`, `read` → `Read`, `edit` → `Edit`, `write` → `Write`, `grep` → `Grep`, `glob` → `Glob`). The session-start text is added only to requests whose model is `zygo/<effective role>` (`ZYGO_MAIN_ROLE`; none for a direct model id), so subagents and the title agent do not get it; as on Claude Code, no `zygo-<main_role>` agent is written, and `default_agent` names the built-in agent mapped to `main_role` when that agent is a primary one. The inline config sets `small_model` to `zygo/read` (else `zygo/<main_role>` when `read` is disabled), so title generation also goes through the proxy. Notify-level guard text has no OpenCode channel and is dropped. Every hook command, on both agents, uses the absolute path of the running zygo binary in place of the bare name `zygo`.)*
- For a workspace member (the plugin's gate: `ZYGO_WORKSPACE` set and not empty; the hooks decide from the session view's marker), the plugin also calls `zygo hook workspace --agent opencode --event <event>`, one call after another: `session` with no session id at load, which commits the pending start; `session` with the `sessionID` of each new main session (the known top session that is not deleted and was activated last; a session's kind comes only from the `info` of a `session.*` event); `prompt` when its turn set becomes non-empty and `stop` when it becomes empty; `pre-tool` with `tool_name` `Agent` for the `task` tool in every session, waited for (a deny throws, so the subagent does not start); `pre-tool` for every other tool of a session that is not a known child, and `tool-end` after each counted call (workspaces design W§9.1, AU27, AU104).
- No alias sentence (OpenCode has no model aliases; its built-in agents are the map).

### 8.8 Adding an agent (checklist, for v2 rows)

1. Write the conformance row (§8.1) with a documentation URL per cell. 2. Map aliases and built-in agents to roles. 3. Choose the per-session config mechanism and the env/flag table. 4. Hooks or plugin for A-7, A-8, A-15, A-18, A-19. 5. Stub-agent launch test and premise test. 6. Docs: alias table, conformance row.

## 9. Elephant-goldfish summary

The model is the combination of: `lead` as `main_role` (§6.4), role prompts (§6.5), the lead guard (§8.3), one `zygo-<role>` subagent per role with its own prompt, model and tool limits (§8.5, §8.7), the alias map so third-party skills that name models still land on roles (§8.6), `writes = false` roles (§6.4), the optional turn cap (§13.5), and the compaction instructions (§10.6). No feature depends on a particular vendor.

## 10. Token saving

### 10.1 Result store

- **Threshold.** A tool result over `trim_threshold_bytes` (default 16 KiB) is stored; the agent gets a view instead. On Claude Code the view is returned as `updatedToolOutput` in the tool's own output shape (the text field of `Bash` output, `Read` file content, `Grep`/`Glob` result text); a tool whose shape the premise test (A-21) has not confirmed on the pinned version is not hooked. `Read` is trimmed only when its output is text; image, PDF and notebook output pass unchanged. Hooked tools (Claude Code): `Bash`, `Read` (only for files over the threshold), `Grep`, `Glob`, `WebFetch`. OpenCode: the tools its `tool.execute.after` hook exposes.
- **Stored form.** `$ZYGO_SESSION_DIR/results/<id>` (raw bytes, `0600`) + `<id>.json` (tool, bytes, lines, kind, created_at); id is a ULID. The store holds the user's own tool output on the user's own machine, as the terminal does; it is the one exception to C3, limited to the session directory. Swept 24 h after the session ends, at the next `zygo` command. *(Note 2026-11-19, N71 plan review: "the next `zygo` command" is every command except `zygo hook …` and `zygo mcp`: a hook is time-bounded (the prompt hook finishes within 5 s, §17.1; the OpenCode plugin hook has a 10 s timeout, §8.7), `zygo hook token` is on the model-request path, and C6 says a hook never breaks the agent. A session that ended in a hook or MCP run is swept by the next other command.)*
- **View.** First 60 lines, then the marker line `[zygo trimmed this output: <n> lines, <bytes> bytes; only part of it is shown. The full output is saved as result <id>. To read the rest, call result_search, result_view or result_tree with id "<id>".]`, then the last 20 lines (none for `Read`). Lines over 400 characters are cut with `…`. The view never exceeds 8 KiB.
- **Kind** by sniffing the first 4 KiB: `json`, `jsonl`, `diff`, `log`, `tree`, else `text`.
- **MCP tools.** `result_view(id, lines?)` (a range, ≤ 400 lines per call), `result_search(id, pattern)` (Rust regex, ≤ 50 matches with line numbers and one line of context), `result_tree(id)` (JSON keys to depth 3 with array lengths; files of a diff; directory structure of a tree; headings of text; nothing for a log).
- **Measure.** Each trim records `bytes_trimmed = stored − view` in the usage store (§14).
- Config: `[results] trim_threshold_bytes`, `head_lines`, `tail_lines`. On/off is `[features] results` (§15.4), the only switch.
- **Workspace members.** A member session's `results` is a symbolic link to `~/.cache/zygo/workspaces/<id>/results/<pane key>/`, made by the session callback (§8.2 step 2), so a result id stays readable after `zygo up` resumes the member in a new session (workspaces design AU16). At exit the link stays, so the session directory stays too. The sweep removes an ended session directory whose `results` link points under the workspace cache by the same 24 h age rule, when its launcher is not live and is still not live just before the removal: the link first, then the directory. It never deletes a workspace results folder or anything in it; only `zygo ws forget` and `zygo ws forget-pane` delete one (AU69). The `zygo ws` verbs do not sweep (AU71).

### 10.2 Soft context window

**Purpose.** Make the agent compact before the model's full window, per model, without the model knowing (C7). The agent's own auto-compaction does the work; the proxy only triggers it.

#### 10.2.1 Definitions

- `prompt` = `input_tokens + cache_read_input_tokens + cache_creation_input_tokens` of a response (the context the model was given).
- `soft` = `soft_context_window` of the served model; absent → trigger 1 off.
- `safety` = 90 % of the served model's `context_window` when the agent's assumed window (`agent_window`, §10.2.2) is larger than it, else none. Without it the agent would compact only near its assumed window while the model refuses at its smaller one.
- `effective` = the session override (§10.2.4) when set, else `soft`; then `min(effective, safety)` when `safety` exists. `off` → trigger 1 off for the session except the safety window, which always applies. A numeric override applies also to a model without `soft` (it still needs `context_window`, a common condition below).
- A **compaction request** carries the header `x-claude-code-compaction` (any value).

#### 10.2.2 The rewrite

When a trigger fires on a response, the proxy rewrites the `usage` of that response's `message_delta` SSE event so the agent's measure equals the window:

```
window = max(context_window of the served model, agent_window)
agent_window = context_window of the catalog model the requested name resolves to
               (role, custom role or model id, profile applied) when that model's id is a
               Claude id (the picker's behavesAs, §8.5), else 1,000,000 (Claude Code's
               assumption for an unrecognized id since v2.1.323; the minimum supported
               Claude Code version is set at or above it, A-13)
usage.input_tokens               := the stream's cumulative input
usage.cache_creation_input_tokens := the stream's cumulative cache_write
usage.cache_read_input_tokens    := max(window − input − cache_write, cumulative cache_read)
```

If `usage` has an `iterations` array, the same three fields are also written in its last entry. `output_tokens` and every other byte stay as received. The cache-read field is the one raised because the agent prices it at about a tenth of input, so the phantom cost the agent shows for that one response stays small. The usage store records the upstream numbers, measured before the rewrite.

Claude Code sizes a role name by the picker's `behavesAs` (§8.5) or, without one, as 1M. If the served model differs from the one the agent assumes (a profile override, a fallback), the `max` above still forces the agent's measure to at least its own assumed window. *(Note 2026-11-19, N38, N15-1M: on Claude Code 2.1.328, `/context` for `design[1m]`, whose role model is a 200k Claude model, shows a 1M window (`2.7k/1m tokens`, `fixtures/claude-code/live/n15-1m-context.txt`): the `[1m]` suffix sets the agent's assumed window to 1M, which v1's formula (suffix ignored) does not follow.)* *(Note 2026-11-19, N67 planning: `agent_window` = 1,000,000 when the requested name carries `[1m]` or an `anthropic-beta` token of the request starts with `context-1m-` (Claude Code strips the suffix from `model` and sends that beta, `fixtures/claude-code/q7/q7-claude.json:40-41`).)*

Common conditions for both triggers: Claude Code session (A-16 row); the served model has `context_window`; a streamed 2xx response on `/anthropic/v1/messages`; not a compaction request.

*(Note 2026-11-19, N15 planning: the rewrite replaces only the three numbers in the received line; a field the object lacks is inserted after its `{`; a line whose edited object repeats a key is not rewritten. Codes are recorded only for a rewritten line, at most once per trigger per response, in the order `context_safety`, `context_signal`, `context_compact`. Only an SSE upstream response is armed: a translated non-SSE body sent to the client as SSE is not rewritten. The status `effective_window` is §10.2.1 `effective`, safety window included, computed for the model of the last main-loop request, else the main role's model; a passthrough session before its first main-loop request has no `effective_window` (HUD design AU29, 2026-11-22).)*

#### 10.2.3 Triggers

- **Trigger 0, safety window.** `safety` exists and `prompt > safety`, on any request class and any stop reason (a subagent's tool loop included): this prevents an upstream refusal, so it does not wait for a turn boundary. Adjustment code `context_safety`. If the agent still sends an over-long request, the upstream's refusal reaches it as `prompt is too long` (§7.6) and the agent's reactive compaction runs, for the main loop and for subagents alike.
- **Trigger 1, soft window.** All of: `effective` set and not `off`; `x-claude-code-request-class` is `main`; `stop_reason = "end_turn"` in the `message_delta` event; `prompt > effective`. Effect: the agent compacts before its next request, which is the user's next message, so the decision is made at response time and never inside a tool loop. Adjustment code `context_signal`.
- **Trigger 2, the model's call.** The response contains a `content_block_start` of `type: tool_use` whose name is the `zygo` MCP server's `compact` tool (§10.3) — matched by server and tool parts, not a fixed prefix. Any stop reason, request class, window or override. The conversation that called the tool compacts: a subagent's call compacts that subagent's own context, never the lead's. Adjustment code `context_compact`.

*(Note 2026-11-19, N15 planning, trigger 2: "server and tool parts" = the name is `mcp__<server>__compact` and `<server>` ends with `zygo`, the rule of the navigation guard (§10.5); `mcp__zygo__compact` and `mcp__plugin_x_zygo__compact` match, `mcp__zygo__compactx` and `mcp__zygox__compact` do not.)*

#### 10.2.4 Session override

- `POST /v1/zygo/sessions/{id}/compact` `{ "window": <n> | "off" }`, n from 100,000 to 10,000,000 (a value above the model's window is accepted and has no effect); `DELETE` clears it (back to the model's `soft`). Reply `{ "window": <n> | "off" | null }`.
- `zygo compact <tokens|on|off>`: `600000`, `600k`, `1m` forms; `on` = `DELETE`. Session from `--session` else `ZYGO_SESSION`. Exit 2 on a bad value, 12 when unreachable within 10 s. *(Note 2026-11-19, N28 planning: the slash form's session-API timeout is 4 s; with no `--session` and no `ZYGO_SESSION`, `zygo compact` exits 2; `zygo compact` has no `--save`.)*
- `/zygo:compact <arg>` typed in Claude Code: `zygo hook prompt` (`UserPromptSubmit`) matches `^/zygo:([a-z][a-z0-9-]*)(?:\s+(.*))?$`, calls the same request, and prints `{"decision":"block","reason":"zygo: compact <arg> for this session","hookSpecificOutput":{"hookEventName":"UserPromptSubmit","suppressOriginalPrompt":true}}` so the line never reaches the model. Unknown verb → block with the `/zygo:help` text *(A2)*; every other verb is in §17.1. Failure → block with `zygo: compact failed: <reason>`. The hook finishes within 5 s. The embedded `zygo` plugin provides `commands/compact.md` (`disable-model-invocation: true`) so the command is listed.
- The status line shows the soft or override threshold as `⇣600k` only when it is below a known model window, and `⇣off` for an override of `off`; a safety-only window, a threshold at or above the model window, and an unknown model window show no numeric `⇣` (HUD design H§6.1 AU28). *(2026-11-22, HUD design)*
- For a workspace member, `zygo compact` and `/zygo:compact` also write the change into the member record (`set <w>`, or `cleared` for `on`), and `zygo up` passes the saved value as `--compact` at resume (`cleared`: none; never changed: the launch value), so the override survives `zygo down`/`zygo up`, crashes and reboots (workspaces design OP12, AU32, AU105). When that write fails or is skipped, the reply ends with `; not saved for the workspace`, and the exit code does not change. The `compact` MCP tool changes no override.

#### 10.2.5 OpenCode

OpenCode compacts at its configured `limit.context`; `zygo` sets it to the soft window (§8.7). This is an agent setting the model is not shown (C7 holds). The SSE rewrite is not armed for OpenCode in v1. OpenCode reads `limit.context` only at start, so the window is set at launch: `zygo opencode --compact <tokens|off>` (`off` = the model's `context_window`). `zygo compact` inside an OpenCode session exits 2 with "OpenCode reads its window at start; relaunch with zygo opencode --compact <n>". **(author)** `zygo claude --compact <tokens|off>` sets the initial session override the same way.

#### 10.2.6 Implementation bounds

When armed, the response tee works in whole SSE lines (a partial line is held until its newline) so it can replace the `message_delta` line; a held line over 4 MiB is flushed unchanged. An unarmed response passes chunk by chunk. An armed response drops any upstream `content-length`. Non-streamed responses are never rewritten.

### 10.3 `compact` MCP tool

`compact()` — description: "Compact this conversation now. Call it at a boundary of your work, when what came before is no longer needed, for example before starting the next unit of a plan." Returns `compaction requested`. It makes no call: the proxy sees the tool call in the response stream (trigger 2). Listed for Claude Code only. Nothing in it names a window, size or price.

### 10.4 Cache markers and provider pinning

- **Pinning.** The first response for a model in a session pins the provider that served it; later requests for that model try the pinned provider first. A retry that lands elsewhere re-pins. All attempts failed → the pin is removed. This keeps the provider-side prompt cache on one provider.
- **Markers** (Anthropic dialect only, `[cache] insert_markers.<agent>`; default `false` for Claude Code, which places its own, `true` for OpenCode): if no `cache_control` key exists anywhere in the body, add `{"type":"ephemeral","ttl":"1h"}` to the last `system` block (only when `system` is an array) and to the last tool definition. Never on `count_tokens`. Every other byte as received.

### 10.5 Code navigation (act-lite) *(A1)*

Code navigation comes from act-lite (https://github.com/act101-ai/act-lite), delivered as the default-on plugin `act101:act-lite` of the built-in marketplace (§15.9); zygo ships no navigation tools of its own **(owner)**. act-lite's MCP server (`act-lite mcp serve`) serves `skeleton`, `symbols`, `symbols_batch`, `repo_outline`, `definition`, `references`, `callers`, `status` and its code-health and dependency analyzers, for 43 languages. act-lite's skills also bring the SDLC workflow (define, plan, execute, deliver, diagnose, review); zygo's role prompts (§6.5) name the role, not a process.

*(Note 2026-11-19, operator decision at N25 planning: the source-file language list is compiled into zygo from `fixtures/act-lite/languages.json`; the launcher writes no `nav_langs.json`, and nothing reads one.)*

- **Navigation present** for a session: `act101:act-lite` is selected (§15.2) and its MCP command is found on `PATH` at launch (§15.3). Only then does the session text carry the navigation sentence (§8.6) and the navigation guard (§13.4) run.
- **Source file** (for the navigation guard): a file whose extension (or, for the names act-lite lists, whose file name) is in the language list compiled into zygo for the pinned act-lite version. When the installed act-lite reports another version (`act-lite --version`, 2 s bound, once per launch), the guard still uses the compiled list, and `zygo doctor` reports the two versions; a re-pin (N41) updates the list. *(Note 2026-11-18, N2, P6a: act-lite 1.2.3 has no `act-lite status` command and its MCP `status` tool lists language names without extensions, so the machine-readable list this bullet first named does not exist; the list is the extension list of act-lite 1.2.3 in `fixtures/act-lite/languages.json`, taken from the release source, and "no list → the guard does not run" is replaced by the compiled list. The version-mismatch rule is the author's call, not observed behaviour.)*
- A **navigation call** is a call to the act-lite MCP server's `skeleton`, `symbols`, `symbols_batch` or `repo_outline`, matched by server and tool parts, as trigger 2 is (§10.2.3).
- Config: `[navigation] lines = 400` (used by the navigation guard; the repository file may set it).

### 10.6 Compaction instructions

`zygo hook pre-compact` (Claude Code `PreCompact`, both matchers) prints: "Keep in the summary: the plan or tracker path and the current step; every open unit of work with its id, round, check and files: <from the session's open units>; the pending briefs and what each subagent reported; the result ids zygo stored (result_view reads them again). Drop: tool output that is in the result store; file contents already edited and committed." No open unit → the same text without the clause "every open unit of work … open units;". Never names a window or size.

### 10.7 Savings report

`zygo usage [--session <id> | --since <date>]` prints, from the usage store (§14): requests, input/output/cache tokens by role and model, cache-read share, estimated cost at list price, `bytes_trimmed`, compaction signals sent and compactions observed (the next request's `x-claude-code-compaction`), guard events. `zygo usage` also prints `tokens trimmed (estimate): <n>` (bytes ÷ 4, rounded up) after the exact `bytes trimmed: <n>` line. `zygo status` in a session prints its savings in tokens (`savings: cache 92%, trimmed 23k tok`) and a `spend: $3.41` line with the session spend of §7.2 (`≈` when notional); outside a session it prints no spend line. The status line shows the session's cache-read share and trimmed tokens. *(2026-11-22, HUD design)*

## 11. Model profiles

### 11.1 Shape

Profiles come from three sources: user config, team config (shared presets), and, after the operator approves them on this machine, the repository's `.zygo/profiles/<name>.toml` files, read from the default branch (workspace definition design R§6, R§7, R§8). In user config (and in team config as shared presets):

```toml
[profile.rust]
overrides = { review = "gpt-6-sol", code = "claude-sonnet-5-5" }   # role → catalog model id
disabled = ["design"]                                               # bundle roles hidden from the lead
prompt = "Prefer small, reviewable commits."                        # ≤ 2048 bytes
plugins = { "team:my-skills" = true, "team:noisy-plugin" = false }  # §15.2, <marketplace>:<plugin>
features = { work_units = false }                                   # §15.4 keys; optional
  [profile.rust.roles.bench]                                        # ≤ 8 custom roles
  model = "claude-haiku-5-5"
  description = "Runs and summarizes benchmarks."                   # one line, ≤ 256 bytes
  prompt = "You run the benchmark you are given and report the numbers."   # ≤ 2048 bytes
  writes = false                                                    # default true

[user]
prompt = "Answer in British English."   # the user prompt, ≤ 2048 bytes, applies to every session
default_profile = "rust"                # optional
```

A repository profile file holds one profile, named by its file stem, with `overrides` and `disabled` only:

```toml
# .zygo/profiles/rust.toml
overrides = { review = "gpt-6-sol", code = "claude-sonnet-5-5" }   # role → catalog model id
disabled = ["design"]                                               # bundle roles hidden from the lead
```

It cannot set `prompt`, custom roles, `plugins` or `features`, and it cannot define a provider: it names roles and models from the user's catalog only. It is not a layer of §6.1: the approved repository profiles are added to the merged config after the layers, with the source `workspace-files`.

### 11.2 Selection

`--profile <name>` wins; `--profile none` disables; else `user.default_profile`; else none. Order of sources for one name: the user profile, then the team preset, then the repository profile (§11.1). A user profile with the same name as a team preset shadows it; a repository profile whose name a user profile or a team preset has is not used, and is never merged key by key with it. A workspace member resolves a repository profile from its pin, the read commit and digest saved in its member record (workspace definition design R§7.4): a later start of that member does not read the default branch, and a pin that is gone, not approved or not read stops the launch. `zygo profile list | show <name> | use <name>` (sets `default_profile`).

### 11.3 Validation (at session creation, by the proxy that will serve)

- Name 1–32 of `[a-z0-9-]`. A profile with no part set is invalid. *(Note 2026-11-18, N6 planning: the name `none` is reserved; it disables the profile.)* *(Note 2026-11-19, N42 review: `help` is not a profile verb argument: `zygo profile help` and `/zygo:profile help` print the profile verb's help, as `help profile` does, and change nothing.)*
- Each override: key is a bundle role; model is in the catalog and servable on the agent's dialect (directly or by translation).
- Each custom role: name follows the profile-name rule and is not a bundle role, a catalog id, `direct`, `subscription`, or a Claude Code alias word (`fable`, `best`, `opus`, `opusplan`, `sonnet`, `haiku`); `description` and `prompt` non-empty and without `{` or `}`; model as for overrides.
- `disabled`: bundle roles, each once; `main_role` cannot be disabled.
- `plugins`: ≤ 32 names, each `<marketplace>:<plugin>` (the form of §15.2) naming a plugin in a configured marketplace.
- `features`: keys of §15.4 only; `redaction.secrets` is refused (a profile cannot turn it off).
- Text fields: no control characters other than newline and tab.
- A repository profile (§11.1) gets the same rules. The shape rules of workspace definition design R§6.3 (its two keys, at most 16 entries each, the name pattern, printable ASCII values) apply at the read, before these rules.
- The launcher reads the default branch for a repository profile only when the requested name is in neither the user nor the team config, so a launch with no repository profile makes no extra git call. In place of `zygo: profile <n> not found; launching without it`, files that are not approved give `zygo: profile <n> is in this repository's workspace files, which are not approved on this machine (digest <12>); launching without it; see zygo workspace approve`, invalid files give `zygo: profile <n>: this repository's workspace files are invalid (<why>); launching without it`, and files that were not read give `zygo: profile <n>: this repository's workspace files were not read (<why>); launching without it`. The launch goes on without the profile (exit 0).
- A launch in passthrough sends no role names (§12.3), so no profile, from any source, applies to it. A requested profile name gives `zygo: profile <n> does not apply in passthrough`, nothing is read, and the launch goes on.
- Invalid → the launcher prints the first violation and launches **without** the profile (exit 0, one warning line). A stale override at request time (the model left the catalog after a server reload) falls back to the role's own model and records `profile_override_stale`.

### 11.4 Effects

- Resolution (§7.4); `/v1/models` annotations; subagents and the legend (§8.5–§8.6): disabled roles are left out, custom roles added with their own prompt; a custom role may be the main loop's model (`--model <custom>`): the session text opens with its prompt and the lead guard is off.
- The usage row records `profile` (name).

## 12. Passthrough (v1: claude.ai with Claude Code)

### 12.1 Purpose and risk

A user with a claude.ai subscription runs Claude Code on that subscription's login and model choice, and still gets redaction, the result store, the soft context window, cache handling, hooks, MCP tools, plugins, work units and the session text. **Local mode only (author):** the subscription credential goes from the agent to the in-process proxy on loopback and from there to the vendor; it never passes through a shared server. Serve mode refuses a passthrough session (`403 zygo_passthrough_local_only`). The README states that the vendor's consumer terms restrict routing subscription credentials through third-party software and that the user's own account carries that risk.

A workspace pool member (workspace definition design R§9.5) starts with `--passthrough` on the config directory that the pool gives it, and no profile applies to it **(operator, 2026-11-24)**. §12.3 and §12.4 do not change.

### 12.2 Credential types

| Type | Version | Agent | Upstream host | Forwarded credential (allowlist) |
|---|---|---|---|---|
| `claude-ai` | v1 | Claude Code | `api.anthropic.com` | `Authorization: Bearer sk-ant-oat…` only |
| `chatgpt` | v2 | Codex | to be verified | to be verified |
| `grok` | v2 | Grok CLI | to be verified | to be verified |

A v2 row is added by filling this table and its tests; the stages below read the row, not hard-coded values.

### 12.3 Launcher (`zygo claude --passthrough`)

- Not set: `ANTHROPIC_MODEL`, the four `ANTHROPIC_DEFAULT_*_MODEL` rows, `CLAUDE_CODE_SUBAGENT_MODEL`, `DISABLE_LOGIN_COMMAND`, `DISABLE_LOGOUT_COMMAND`; in the settings file: no `apiKeyHelper`, `availableModels` or `model`. `ANTHROPIC_BASE_URL` and the hint headers stay.
- `ANTHROPIC_CUSTOM_HEADERS` adds `X-Zygo-Token: <session token>`.
- Generated subagents have no `model` field (they run on the main loop's model); prompts and tool limits stay.
- The upstream credential is the login Claude Code stored with `/login`. A login token given through the environment (`CLAUDE_CODE_OAUTH_TOKEN`) is removed by the denylist (§8.4) and is not supported.
- A role name on any model path (`--model lead`, `--fallback-model code`, a user `--agents` entry with `"model": "code"`) → exit 2: "--passthrough sends model names to the vendor; use a vendor model id". `--bare` → exit 2 (bare mode reads only API keys). `CLAUDE_CODE_SIMPLE` on in the agent's environment (the rule of §8.5 Flags) → exit 2: "--passthrough does not work with CLAUDE_CODE_SIMPLE: bare mode reads only API keys" (workspaces design AU26).
- If a system-level managed settings file sets `apiKeyHelper` or an API key, exit 2: "passthrough needs your claude.ai login; a managed setting replaces it". *(Note 2026-11-19, N32 planning: passthrough also writes no `modelPicker`, whose rows offer role names. In passthrough a Claude Code model alias (`sonnet`, `opus`, …) on `--model` is a vendor model name, so the lead guard and the session text are off, as for a vendor model id.)*
- The lead guard and the session text use `main_role` unless `--model` names a vendor model id (then both are off, as for any direct model id).

### 12.4 Proxy

- **Stage 1.** On `/anthropic/*`: when `X-Zygo-Token` is present, it is the session token (verified as in §7.2, and the session must have `passthrough = true`); `Authorization` is not read as identity. Invalid → `401 zygo_passthrough_token_invalid` "relaunch zygo claude --passthrough". Then the upstream credential is checked against the type's allowlist: anything else (missing, `x-api-key`, an `zyg_` token, any other bearer) → `401 zygo_passthrough_no_login` "passthrough needs a claude.ai login (/login)"; nothing is sent upstream.
- **Stage 2.** No role resolution. `model` as sent (`[1m]` stripped for lookup). Catalog match = the first model with a native Anthropic provider whose `upstream_model` equals it; the match supplies facts (and so the soft window). Role recorded as `subscription`. One candidate: the match's provider on the type's upstream host, else a built-in provider for that host with no key.
- **Stage 3.** The tee and compaction signal run as usual. The usage row records tokens, `role = subscription`, cost estimate 0.
- **Stage 5.** No key file is read; `enabled = false` still applies.
- **Proxy.** The client's `Authorization` is forwarded unchanged; no `x-api-key` is added; every `x-zygo-*` header is removed; the `anthropic-beta` header is forwarded unchanged. The credential is never logged or stored.
- No translation, no other provider.

### 12.5 Limits (documented)

The session token in `ANTHROPIC_CUSTOM_HEADERS` is read once at agent start; a passthrough session has no idle expiry, so its token stays valid while its launcher runs (§7.2). *(2026-11-22, FIX-18)* A `/model` switch mid-session does not move the lead guard.

## 13. Work units and work-item sources

### 13.1 Model

- **Work item:** a record in an external tracker. **Work unit:** `<source>:<id>[:<part>]`; each segment 1–48 of `[A-Za-z0-9._-]`, starting alphanumeric; whole ≤ 128 bytes. *(Note 2026-11-18, N22 planning: a source id with other characters (`zygo-v1/N12`, `acme/widgets#42`) maps each of them to `-` in the unit id; `ZYGO_WORK` and lookups keep the source id.)* `part` names a stage (`plan`, `review`, `verify`). A round counts attempts from 1.
- **Facts of a unit** (metadata only): session, agent id (`main` for the lead), agent type, began/ended times, requests and tokens (joined from the usage store, §13.6), tool-call counts, failed calls, check ran/passed after the last edit, guard events by guard and level, the levels in force, the lead's verdict.
- Nothing is written back to the source in v1.

### 13.2 Sources

```toml
[work]
sources = ["work-graph", "beads", "github", "branch"]   # tried in order for detection *(A1: work-graph added)*
```

| Source | Id form | Detected from | Lookup (title, url, status) |
|---|---|---|---|
| `work-graph` *(A1)* | `<program>/<node>` (`zygo-v1/N12`) | a work graph `docs/work/<program>/graph.md` (act-lite's format, marker `<!-- work-graph -->`) with a node `IN-PROGRESS` whose Refs name the current checkout; a `zygo work <program>/<node>` argument | the node's title; its **Acceptance** check is the check hint and its **Files** are the files hint |
| `beads` | the Beads issue id (`bd-a1b2`) | `.beads/` in the repository and a branch name or `zygo work` argument matching the repository's Beads id prefix | `bd show <id> --json` (2 s bound); `acceptance_criteria` is offered to the lead as the unit's check hint |
| `github` | `<owner>/<repo>#<n>`, short form `gh-<n>` | branch names `<n>-…`, `gh-<n>-…`, `issue-<n>…`; a `#<n>` argument | `gh issue view <n> --json title,url,state` (2 s bound) |
| `branch` | the branch name | always (fallback) | none |

- A source is an internal trait with three methods: `detect(repo, branch) -> Option<id>`, `lookup(id) -> Option<Item { title, url, status, check_hint?, files_hint? }>`, `normalize(arg) -> Option<id>`. Adding a source (Jira, Linear, GitLab) is a new implementation and a new name in `sources`; no other code changes **(author)**.
- A lookup failure (tool missing, offline, timeout) keeps the id and drops title/url; one stderr line at launch.
- `zygo work [<id>]` shows or sets the session-independent current item for the repository (stored in `<git common dir>/zygo-work`, the directory `git rev-parse --git-common-dir` names, so it is never committed and linked worktrees share it); `zygo work clear`. At launch: explicit setting, else detection. The item goes to `ZYGO_WORK`, `X-Zygo-Work` and the session text.

### 13.3 Collection

- `zygo mcp` tool `work_begin(unit, round, check, files)`: `check` is one literal shell command (≤ 1,024 bytes, empty allowed), `files` ≤ 64 repository-relative paths (`dir/` = prefix). Description: "Record the unit of work you are starting: its id, its round, the one command that proves it done and the files it may change. Call it once, first." Reply `noted <unit> round <n>`. When the unit's source gives a check hint or a files hint, an empty `check` or `files` takes the hint *(A1)*.
- The record is made by the `PreToolUse` hook on that tool (`zygo hook work --event begin`), because only the hook sees `agent_id`: it appends a `begin` line to `$ZYGO_SESSION_DIR/work/<agent_id or main>.jsonl` and writes `<…>.begin.json`. A repeated `(unit, round)` is a no-op; a different open unit is ended `superseded` first. Subagents whose type does not start with `zygo-` record nothing.
- Each agent's log file exists from its first hook call, whether or not a unit is open; lines without an open unit are kept for the guards and ignored by the fold, which reads from the open unit's `begin` offset.
- `PostToolUse` on a navigation tool (§10.5) appends `navigated` with the paths from its input.
- `PostToolUse`/`PostToolUseFailure` (every tool) append one line each: `call`, `failed`, `check: passed|failed` (a `Bash` command that matches the check: whitespace-normalised equality, optionally prefixed by allowed `VAR=value` words and `cd <dir> && `, optionally suffixed ` 2>&1`, run inside the repository of the unit's last edit), `edit` (with the file's repository root), `repeat` (same tool+input hash among the last 256 calls, no edit since), `pushed`, `pr_opened`, `trimmed`. *(Note 2026-11-18, N24 planning: open premise — a `Bash` command with a non-zero exit fires `PostToolUseFailure` on the pinned Claude Code. No capture shows it (only `Read` failures are in `fixtures/claude-code/hooks/`), and the `Bash` `tool_response` has no exit code. Until a live check (N38) verifies it, `failed` and `check: failed` come from `PostToolUseFailure` or `tool_response.interrupted: true`; every other `PostToolUse` counts as passed.)* *(Note 2026-11-19, N38, P-N24-1: on Claude Code 2.1.328 a `Bash` command with a non-zero exit fires `PostToolUseFailure` (`fixtures/claude-code/live/p-n24-1-exit.json`). A running `Bash` command interrupted with Escape fires neither `PostToolUse` nor `PostToolUseFailure` (`fixtures/claude-code/live/p-n24-1-interrupt.json`), so no input with `tool_response.interrupted: true` reaches zygo; `docs/live-checks.md`.)*
- Lines ≤ 1 KiB, one append each (`O_APPEND`), ≤ 4,000 lines per file (then `overflow`: no more lines are appended, and the `navigation` and `repeat` guards no longer fire for that agent, because their history is incomplete — they fail open). A hook reads at most `begin.json` and the log's last 256 KiB (enough for the 256 calls the `repeat` and navigation guards look back over). *(Note 2026-11-18, N24 planning: the fold (`SubagentStop`, a superseding `begin`, launcher exit) reads from the open unit's `begin` offset to the end of the log, bounded by the 4,000-line × 1 KiB cap; every other hook path keeps the `begin.json` + last-256 KiB bound.)* *(Note 2026-11-18, N24 review: the `repeat` and navigation look-back is the last 256 calls that lie within the last 256 KiB; when fewer fit, the guards look back over fewer and fail open beyond.)* *(Note 2026-11-18, N24 review: the `begin` line carries unit and round; `check` and `files` are kept in `begin.json`, each path ≤ 4,096 bytes.)* *(Note 2026-11-18, N24 review: `begin.json` also keeps the repository root of the unit's last edit, which the check's repository test reads.)*
- `SubagentStop` (matcher `^zygo-`) and launcher exit fold the log into the unit's facts and append one row to the usage store's `work_units` table (§14).
- `zygo mcp` tool `work_verdict(unit, round, verdict, findings, head, base, dispositions)` (lead only): `verdict` ∈ `keep | discard | rework`; recorded on the unit row. On `keep`, the `dispositions` guard (§13.4) checks `head`, the optional `base` and one `dispositions` entry per review finding against the node range, as decision records design D§9.2 and D§9.4 specify; git runs in the session's repository root (`repo_root` in the session view), and a view without it gives the problem `cannot check: no repository root`. The argument refusals of D§9.2 (a wrong type; a value outside its enum, pattern, length or item limit; a key the entry's disposition does not take; an argument object over 256 KiB) are `InvalidParams` at every level, before the level is read. *(Note 2026-11-18, N24 review: "lead only" is enforced by the `PreToolUse` hook on that tool, `zygo hook work --event verdict`: an input with an `agent_id` key (any value) gets `permissionDecision: "deny"` with "zygo: work_verdict is for the lead only. Report your result to the lead; the lead records the verdict.")*

### 13.4 Guards

Levels: `monitor` (record), `notify` (record and tell the agent once, without refusing), `enforce` (record and refuse or continue with the reason). On Claude Code, `notify` prints only `hookSpecificOutput.additionalContext` with the text and **no** `permissionDecision` (an `allow` would skip the user's permission prompt, https://code.claude.com/docs/en/hooks); `enforce` prints `permissionDecision: "deny"` with the text as `permissionDecisionReason` (for `SubagentStop`, `decision: "block"` with `reason`). All guards except `lead`, `redaction` and `attribution` run only when `[features] work_units` is on. Defaults: all `monitor` except `trailers = "notify"` **(author)** and `dispositions = "enforce"` (decision records design D§9.3). `[guards] min_level` (user or team config) is the lowest level a repository may set.

*(Note 2026-11-19, operator decision at N25 planning: `notify` "once" means once per event: each guard event prints its text in that call; nothing remembers earlier notices. The claims guard fires nothing when the work log cannot show the whole unit (its begin line is outside the hook read window, or an `overflow` line exists), as the navigation and repeat guards. The `lead` guard also appends its event line (§20 #36), as every guard does. Scope: a path with no repository root is not in the unit's files (an event); "temp and scratch directories" are the canonical system temp directory, `/tmp` and `/var/tmp`. A guard-line write failure (an I/O error) is a C6 internal error for every guard; a full log (the line is refused at the §13.3 cap or after `overflow`) is not an error: navigation, repeat and claims then fire nothing, and every other guard still prints its decision *(corrected 2026-11-19, N25 round-2 review)*. Guard file reads use the §13.3 hook read bound (256 KiB): `-F` reads the last 256 KiB of a regular file; a source file over 256 KiB counts as over `navigation.lines`. Whether Claude Code passes `additionalContext` on `SubagentStop` to the agent is unverified: live check P-N25-1 (N38).)* *(Note 2026-11-19, N38, P-N25-1: verified on Claude Code 2.1.328: a `SubagentStop` hook that prints `hookSpecificOutput.additionalContext` (no `decision`) puts the text in the subagent's next request as a `<system-reminder>` (`fixtures/claude-code/live/p-n25-1.json`).)*

| Guard | Trigger | Event when | notify text / enforce action |
|---|---|---|---|
| claims | `SubagentStop` of `zygo-<role>` with an open unit whose check is non-empty (Claude Code: the handback `PreToolUse` where the agent provides one; the builder verifies which event can continue a subagent) | the check never ran, or an edit followed the last pass | "zygo: this subagent did not run its acceptance check after its last edit (`<unit>`)." / continue the subagent once: "zygo: run the acceptance check `<check>` alone, in the repository you changed, after your last edit, and quote its output in your report; say so if it fails." |
| scope | `PreToolUse` edit tools inside a subagent with an open unit listing `files` | the path (relative to its own repository root) is not listed and not under a listed prefix; temp and scratch directories never count | "zygo: `<path>` is not in this unit's files; if the change is needed, report it." / deny with that text + " Ask the lead to widen the brief." |
| navigation | `PreToolUse` `Read` without `limit` of a source file (§10.5) over `navigation.lines` lines, navigation present (§10.5) *(A1)* | no `skeleton`/`symbols`/`symbols_batch`/`repo_outline` on that path (or a parent dir) among this agent's last 256 `navigated` paths (in the open unit, or in the session when no unit is open) | "zygo: call skeleton or symbols on `<path>` before reading all of it." / deny with that text |
| output | `PreToolUse` `Bash`, result store on | a pipeline segment `head` or `tail` after `|` with no args, `-n <N>`, `-<N>` or `-c <N>` (not `-f`, not `-n +N`) | "zygo: do not shorten output with `| head`/`| tail`; zygo stores large results and gives you `result_search`." / deny with that text |
| trailers | `PreToolUse` `Bash` that is `git commit` with `-m`/`-F` | the message lacks `Zygo-Session:` (and `Zygo-Work:` when a unit is open) | the trailer sentence (§8.6) / deny with it |
| repeat | `PostToolUse` | the `repeat` fact | `monitor` only in v1 |
| dispositions | `work_verdict` with `verdict = "keep"`, checked in the MCP tool, not a hook (no `guard` line, no `guard_events` count) | any problem of decision records design D§9.4 | the verdict is recorded and the reply ends with `zygo: dispositions: <problems>` / the verdict is not recorded; the tool returns an error with `zygo: keep refused: <problems>`; at `monitor` the check does not run |
| lead | §8.3 | always `enforce` when registered | — |
| redaction | §16.1 | an edit writes `[REDACTED:` | always `enforce`; runs without work units |
| attribution | `PreToolUse` `Bash` with a message command: `git commit` (`--amend` included), `git tag`, `git merge`, `git commit-tree`, `git notes add`, `git notes append` (after `--ref <ref>`), `gh pr create`, `gh pr edit`, `gh pr comment`, `gh pr review`, `gh pr merge`, `gh issue create`, `gh issue edit`, `gh issue comment`, `gh release create` or `gh release edit` (`new` for `create`). The command may follow `VAR=value` words, a program path, and the wrappers `command` (`-p`), `time`, `env` (options and `VAR=value`), `exec`, `nohup`, `nice`, `stdbuf` and `timeout` (with its duration), each with its options; a `bash`, `sh`, `zsh`, `dash` or `ksh` `-c` string is parsed and searched as a command (up to 4 levels). | For each message command, and only there, in this order: its arguments as the shell gives them to the program (quotes removed, so `"Cl""aude"` is `Claude`); the heredoc bodies of that command and of every command before it in the line (any join: `;`, `&&`, `||`, newline, pipe); each message, body or notes file it names (`git` `-F`/`--file`; `gh` `-F`, `--body-file`, `--notes-file`; separate or attached value): when a command before it in the line writes that file (a `>`, `>>`, `>|`, `&>` or `&>>` redirection, with an optional descriptor number, or a `tee` operand), the words of the last such command and of the commands piped into it, and the file is not read; else the file, read whole, one line at a time (a regular file only, so a FIFO is never opened; not `-` or `/dev/stdin`). A relative file is resolved from the directory the command runs in: the hook `cwd`, then each `cd` or `pushd` before it (an absolute path, a relative path, `~`, `~/<path>`, or no directory for `HOME`), also inside a `{ … }` group; a `cd` inside a `( … )` subshell ends with the subshell; git's `-C` applies on top. The search finds a Claude attribution, matched without case, first match in this order: `noreply@anthropic.com`; `Co-Authored-By`, optional blanks, `:` or `=`, then a name (up to `<` or the line end) that holds `claude` as a whole word; `Generated with [Claude Code]`, or `Generated with Claude Code` without brackets, with or without a link (`Generated with` then optional blanks then `[Claude Code]`, or `Claude Code` as whole words); `Generated by Claude`, with or without `Code` (`Generated by` then optional blanks then `claude` as a whole word); `🤖` then optional blanks or U+FE0F then `Generated`; `Claude-Session:`; `https://claude.ai/code/` followed by a session path. A product name such as "Claude Code plugin" or "supports Claude Code" is not attribution, and text in the other commands of the line (a search for the text, an `echo`, a heredoc of a later command) is not searched, except the heredocs of the commands before the message command and the words of a command that writes a file it names. | always `enforce`; runs without work units: deny with "zygo: Claude attribution is never allowed in commits, tags, notes, pull requests, issues, comments, reviews or releases. `<match>` is in `<place>`; reword that text without it and run the command again.", `<match>` being the matched form as written in this cell, `<place>` being `a command argument`, `a heredoc`, ``the command that writes `<path>` `` or ``the file `<path>` `` (operator, 2026-11-22) |

Known limits of the attribution guard, documented: a message written in an editor; a message, body or notes read from standard input without a heredoc in the command; a message reused from an existing commit (`git commit -C <rev>` or `-c <rev>`, `git revert`); text the shell builds when it runs: `$'…'` escapes, shell variables and command substitutions; a file after a `cd` whose directory the guard cannot tell (`cd -`, `popd`, a `pushd` with no directory, a directory with `$` or a backquote); a message file that the line writes in another way than a redirection or `tee` (for example `cp`, `mv`, or a shell `-c` string), which is read from disk as it is before the line runs; `gh api`; and `sudo`, `xargs` or alias wrappers. These are not seen; the §8.5 settings keys and the §8.6 attribution sentence cover them.

A guard never fails the tool on its own error (C6). Guard texts are shown to the model locally and never stored.

### 13.5 Repository file

`.zygo.toml`, read from the default branch (`refs/remotes/origin/HEAD`, else `main`; `git show <branch>:.zygo.toml`), so a working-tree edit by an agent changes nothing until it is merged. This is a review boundary, not a security boundary. *(Note 2026-11-19, N52 diagnosis: when a git call that reads this file (`rev-parse`, the default-branch lookup, `git show`) passes its 2 s bound or fails, the file counts as absent and the user and team levels apply; this is an internal error under C6, so the hook writes one stderr line naming the repository file and the failed read, and changes no decision because of it.)*

```toml
version = 1
[guards]
claims = "enforce"
scope = "notify"
navigation = "monitor"
output = "notify"
trailers = "enforce"
dispositions = "enforce"
[navigation]
lines = 400
[limits]
subagent_max_turns = 0   # 0 = no cap; else maxTurns on every zygo-<role> subagent (Claude Code)
[work]
sources = ["beads", "branch"]
```

Per guard: the file's level, else the config default; then the higher of that and `min_level`. The `[guards]` keys are `claims`, `scope`, `navigation`, `output`, `trailers`, `repeat` and `dispositions` (decision records design D§13 item 3), and `min_level` (user or team config only). Turn cap in force: the lower non-zero of the file's and the config's `[limits] subagent_max_turns` (user or team config, default 0). Member maximum: `[limits] max_members` comes from the built-in, team and user layers only; the load removes it from the file with the line `.zygo.toml: limits.max_members ignored; only user or team config may set it` (workspace definition design R§9.4). An invalid file is ignored with one stderr line; `zygo doctor` and `zygo status` print `repo policy: <root>@<branch> (claims enforce, …)`, `defaults` or `invalid (<why>)`.

### 13.6 Join and report

Units of one session are cut per agent by `began_at`: unit *k* owns that agent's usage rows with time in (`began_at` of *k*, `began_at` of *k+1*]. Usage rows carry `agent_id` from `x-claude-code-agent-id` (null for the main loop). Premise (verified before the join is built, §19): the hooks' `agent_id` equals that header for the same subagent, is stable for the subagent's life, and the main loop sends none. OpenCode carries no agent id; its units are cut over the whole session. `zygo work report [<unit>] [--since]` prints per unit: rounds, tokens and estimated cost by role and model, check ran/passed, guard events, verdicts. The CLI form also counts the delegated decision records in `docs/decisions/` of the working tree of the repository at the current directory (`git rev-parse --show-toplevel`, 2 s bound; decision records design D§6.8, D§9.5). A unit block that has at least one valid record of that unit, or of that unit with a `:<part>`, gains `  decisions: <open> open, <total> total` after its `guard events:` line. After the last block, or after `no work units`, after one empty line, when the directory holds at least one record name: `decision records: <open> open, <total> total, deepest chain <d>`, then `, <m> malformed` when `m > 0`; a `docs/decisions` that is a link or not a directory prints `decision records: docs/decisions is not a directory`, and more than 10,000 entries print `decision records: over 10,000 files`. Outside a repository, when the git call fails, and in the slash form (which reads no record, D-C2), there are no record lines. A record never makes the report fail.

## 14. Usage store (metadata only)

- SQLite (WAL) at `~/.local/share/zygo/usage.db` (Windows `%LOCALAPPDATA%\zygo\usage.db`); serve mode keeps its own on the server. Tables:
  - `requests`: ts, session, token label (serve mode) or null, agent, agent_version, agent_id, agent_type, request_class, compaction header, route, role requested, role served, model, provider, profile, passthrough, status, latency_ms, input/output/cache_read/cache_write tokens, estimated_usd (list price × tokens, with the long-context tier of §6.3 when the price row has one; 0 for passthrough) *(2026-11-22, HUD design)*, adjustments (text list), redaction findings (`[{id, count}]`), repo, work item. *(Note 2026-11-19, N11 review: `status` 0 means no upstream response — the client left or the proxy stopped first.)*
  - `trims`: ts, session, agent_id, tool, kind, stored_bytes, view_bytes.
  - `work_units`: §13.1 facts.
  - `upstream_errors`: ts, session, request_class, model, status, error_type, error_message. *(Note 2026-11-22, FIX-10: LIV-1 D8, FIX-8. One row for each passthrough request whose upstream answer reaches the client with a status that is not 2xx; a refusal by zygo itself, a non-passthrough session and an error event inside a 2xx stream write no row. The response path copies at most the first 16 KiB of the relayed body and does no parse; the usage writer reads `error.type` and `error.message` from it (`error` as an object, else the body; when `error` is a string, the type is empty and the string is the message). Both are empty when the copy is not such JSON (not JSON, an SSE body, a body over 16 KiB) or when the body did not finish (an upstream read error or a client disconnect before its end: such a body is not trusted). Each field is cut to 2 KiB; the secret pass of §16.1 runs on it first, at the stage-6 deadline, so a token is matched whole (an empty field when the pass blocks or fails, or when the session's pass lock stays held past that deadline, also when `[features] redaction.secrets` is off); then the client's login is removed (whole, and each run of 8 or more characters shared with its first 512 characters); then each control, format (Cf), line separator and paragraph separator character becomes a space, and at most 300 characters are kept. Only the secret pass and the login removal run on the message: an upstream message that quotes request text keeps that text. At most 256 rows with a body copy wait for the usage writer; a row over that bound gets empty fields, and its `requests` row is still written. The row and its `requests` row are written in one transaction. The table is created with `CREATE TABLE IF NOT EXISTS` on every open, without a schema version change; `retain_days` and `zygo usage --sync` (which replaces a session's rows) apply as for `requests`. An older binary does not know the table: after a downgrade, its rows are not pruned until a binary that knows it opens the database again.)*
- Never: bodies, prompts, tool inputs, file paths outside the repository-relative `files` of a unit (kept only in the local session directory), redacted spans or words, credentials. One exception: the upstream error `type` and `message` of a non-2xx passthrough answer (`upstream_errors`), scrubbed and capped as that table's note says. *(2026-11-22, FIX-10)*
- Retention: `[usage] retain_days = 90`; older rows are deleted at launch.
- **Serve mode.** The server writes `requests` rows; `trims` and `work_units` are written on the client. So that `zygo usage` and `zygo work report` (§10.7, §13.6) read one local database, the launcher, before it deletes the session (§8.2 step 6), fetches the session's rows with `GET /v1/zygo/sessions/{id}/requests` (team token; the server answers only for sessions created with the same token label, also after the session is deleted) and inserts them into the local `requests` table. A failed fetch prints one line; the server keeps the rows for `retain_days`, and `zygo usage --sync` retries, with the team token, for local sessions whose rows are missing. **(author)** *(Note 2026-11-19, N37 planning: the server answers the rows request only after its usage writer has written every row queued for that session; a session whose fetch failed is recorded locally as pending sync, and `--sync` retries exactly those. In a session, slash verbs read the team layer from `$ZYGO_SESSION_DIR/team.toml` (written by the launcher, mode 0600, no secrets), because the team token is not in the agent's environment.)* *(Note 2026-11-19, N37 plan review: the rows reply carries `complete` (false when the 25 s wait of §7 ended with rows still queued); a session is marked pending sync when the server creates it and cleared only after a complete fetch, or when the server no longer knows the session; `--sync` replaces the rows of a session it retries. The pending mark is a table added without a schema version change, so older binaries and read-only opens are unaffected.)* *(Note 2026-11-19, N37 second plan review: `--sync` skips a session that is still live (its session directory still holds its token file); the launcher replaces the session's rows at exit.)* *(Note 2026-11-19, N37 review: "live" also needs the launcher to be alive: the launcher writes its process id to `pid` in the session directory, and a session whose launcher process is gone is retried. (N37 review round 2: the launcher holds an exclusive lock on `pid` for its life; a session is live while that lock is held.) In serve mode every config-reading verb, `zygo marketplace`, `profile`, `config` and `doctor` included, reads the team layer.)*
- Usage is recorded from a tee on the response stream: chunks go to the client as they arrive; a copy is parsed per SSE line that carries `usage` (Anthropic `message_start` then cumulative maxima from `message_delta`; OpenAI the final chunk or `response.completed`). Non-streamed bodies are copied up to 4 MiB for parsing. Usage is written after the stream ends or the client disconnects. The tee also reads `cache_creation.ephemeral_5m_input_tokens` and `ephemeral_1h_input_tokens` when present, for the cost: 5-minute writes at `cache_write_5m` (else `cache_write`), 1-hour writes and writes with no split at `cache_write` (§6.3); the row keeps one cache-write total (HUD design AU33). *(2026-11-22, HUD design)* *(Note 2026-11-19, N13 planning: for a translated response, the OpenAI numbers (`response.completed` usage, or the final chat chunk's usage) are read by the stream mapper. The mapper runs before the tee and writes them as Anthropic `message_delta` usage: `input_tokens` = input − cached, saturating; `cache_read_input_tokens` = cached; `cache_creation_input_tokens` = 0. The tee then records that usage, and the usage row gets the same numbers. The tee itself parses only the Anthropic form. The mapper holds a partial upstream event up to the 4 MiB bound of §10.2.6; over it, it ends the stream with an error event and does not flush the bytes unchanged (they are another dialect's). A translated non-streamed body is read up to 4 MiB; over that, the client gets `502 zygo_upstream_too_large`.)*

*(Note 2026-11-19, N16 planning: §7.2 "Provider names … never leave the server" wins over the `provider` column here: rows served by `GET /v1/zygo/sessions/{id}/requests` carry `provider = ""`; the server's own database keeps the name. Prices never leave the server either: those rows carry `estimated_usd = 0`; the server's own database keeps the cost.)*

## 15. Plugins and skills *(A1: rewritten)*

### 15.1 Marketplaces

```toml
[[marketplace]]
name = "team"
source = "github:example/agent-plugins"   # github:<owner>/<repo>, git+https://…, git+ssh://…, or a local path
ref = "v1.4.0"                             # tag or commit; branch names are refused (reproducible snapshots)
commit = "4f2c…"                           # written by zygo: the commit `ref` resolved to (the pin)
```

- **Layout:** the Claude Code marketplace format: `.claude-plugin/marketplace.json` (`name`, `owner.name`, `plugins[]` with `name`, `source`, `description`, `version`). A plugin `source` must be a relative path inside the marketplace (`./…`); any other source form (another repository, a package) is skipped with one line naming the plugin **(author)**. Per plugin: `.claude-plugin/plugin.json`, `skills/<skill>/SKILL.md`, and optionally `agents/`, `commands/`, `hooks/hooks.json`, `.mcp.json`, `.lsp.json`, `scripts/`, `bin/`, `opencode/plugin.js`. Manifests for other agents (`.codex-plugin/`, `.cursor-plugin/`) are ignored in v1; any other file is not copied to the store.
- **Origins**, merged by name: the built-in marketplace (§15.9), team config (layer 2), and user config (written by `zygo marketplace`, §15.8). A user entry replaces a team entry of the same name; the built-in `act101` entry can be moved to another `ref` by the user, not to another source.
- **Pin:** a fetch where `ref` resolves to a commit other than `commit` keeps `commit` and prints `marketplace <name>: tag <ref> moved; run zygo marketplace update <name> --accept`.

### 15.2 Selection

- `zygo plugins enable|disable <marketplace>:<plugin>` writes `[plugins] enable` / `disable` in user config.
- Order, lowest to highest: built-in `default_on` (§15.9), team `default_on`, user `enable`/`disable`, the profile's `plugins` map (§11.1; `true` adds, `false` removes). Team config may mark plugins `required`: a user or profile cannot disable them.
- A selected plugin is delivered only when its code is approved (§15.7).

### 15.3 Sync and snapshot

- `zygo plugins sync` (also at every launch, 5 s per marketplace, 20 s total): fetch each marketplace at its `commit` into `~/.cache/zygo/marketplaces/<name>/`, run the plugin check (§15.6) on each selected plugin, copy passing plugins to the store `~/.local/share/zygo/plugins/<marketplace>/<name>/<version>/`, write the manifest last (temp file + rename). A failure keeps the previous store; the launch continues with what the store has and prints one line (`plugins: offline` / `partial`).
- Serve mode: `GET /v1/zygo/plugins` lists the team's marketplaces, refs, commits, `required`/`default_on` flags and team approvals; the client fetches from the marketplaces itself. **(author)** The server does not host archives.
- **Missing commands:** a `.mcp.json` server whose `command` is neither found on `PATH` nor an existing absolute path is left out of the session's snapshot, and the launch prints one line: `plugin <marketplace>:<plugin>: <command> not found (<homepage>)`. zygo installs no program a plugin needs **(author)**. *(Note 2026-11-20, owner: the installers `scripts/install.sh` and `scripts/install.ps1` choose the navigation plugin. When act101's `act` is on `PATH`, they enable `act101:act101` and disable `act101:act-lite`. When only `act-lite` is on `PATH`, they enable `act101:act-lite` and disable `act101:act101`. When neither is found, they run act-lite's installer and then enable `act101:act-lite`. The zygo binary still installs no program. See `docs/install.md`.)*
- **Snapshot per session**, made from the store under a lock:
  - Claude Code → `$ZYGO_SESSION_DIR/plugins/<plugin>/`, passed by `--plugin-dir`. Claude Code loads the plugin's skills (namespaced `<plugin>:<skill>`), agents, commands, hooks and MCP servers itself.
  - OpenCode (no plugin namespace) → each skill to `$ZYGO_SESSION_DIR/opencode/skills/<plugin>-<skill>/` with its frontmatter `name` set to `<plugin>-<skill>`; each `opencode/plugin.js` to `$ZYGO_SESSION_DIR/opencode/plugins/plugin-<plugin>.js`; each `.mcp.json` server to an OpenCode `mcp` entry `<plugin>-<server>` (`type = "local"`, `command` = the command and its args, `environment` = its env); each agent to an OpenCode agent `<plugin>-<agent>` with `mode = "subagent"`; each command to an OpenCode command `<plugin>-<command>`. A part the pinned OpenCode cannot read is not delivered, and its conformance cell says so (§19 P5).
  - Nothing is written to `~/.claude` or the user's OpenCode config.
- `zygo plugins list` → `<marketplace>:<name> <version> (required | default | optional | disabled) skills: a, b · mcp: <servers> · code: approved | needs approval | none · missing: <commands>`. `zygo doctor` reports the store and manifest state, missing commands, and a selected plugin that is also installed in the agent's own config (it would load twice).

*(Note 2026-11-18, N30 planning: the launch prints one line per plugin that needs approval, one per missing MCP command, and the moved-tag, skipped-source and offline/partial lines; any other plugin condition (store copy changed, reserved name, duplicate name, lock busy) counts toward `plugins: partial` and is printed in full only by `zygo plugins sync` and `zygo plugins list`.)*

### 15.4 Feature toggles

Built-in features are switched in config, per user, team and profile; every toggle defaults to `true` **(author)**, except `workspace_tool`: `[features] results`, `work_units`, `compact_tool`, `redaction.secrets` (cannot be turned off by a profile or repository), `redaction.content`, and `workspace_tool`, default `false`, which lists the MCP tool `workspace_down` (it runs `zygo down` for the session's workspace, detached) in a workspace member session (workspaces design AU65). A profile may set `features` too, except `workspace_tool`. Navigation is switched by selecting or deselecting `act101:act-lite` (§10.5, §15.2). The `compact` tool keeps the name `compact`: act-lite's skills call a tool of that name when the session has one *(A1)*.

### 15.5 The embedded `zygo` plugin

Compiled into the binary and always in the snapshot: one `commands/<verb>.md` per §17.1 verb, each with `disable-model-invocation: true` *(A2)*, and the `zygo-cli` skill (how to use `zygo compact`, `zygo status`, `zygo work`, `zygo usage`, `zygo marketplace`, `zygo plugins`; "never state or guess the context window"). OpenCode gets the skill (it has no prompt hook for `/zygo:compact`).

### 15.6 Plugin check

`zygo plugin check <dir>` (marketplace or single plugin), run at sync and usable in CI:

- Names 1–64 of `[a-z0-9-]`, no leading/trailing/double `-`; a skill's `name` equals its directory; `description` 1–1024 characters; `version` semver; marketplace entry version equals `plugin.json`.
- No file over 1 MiB, plugin total ≤ 4 MiB (counting only the parts §15.1 copies), no symlinks, every path inside the plugin.
- It prints the plugin's code items and code digest (§15.7). A structure violation fails the check; code does not.

*(Note 2026-11-18, N30 planning: "no file over 1 MiB, plugin total ≤ 4 MiB" count raw bytes as stored; text code items are still hashed with CRLF read as LF, §15.7.)*

### 15.7 Plugin code and approval

- **Code** of a plugin: each `.mcp.json` and `.lsp.json` server (command, args, env names), each hook command (`hooks/hooks.json`, `plugin.json` `hooks`), `opencode/plugin.js`, each file under `scripts/` or `bin/`, each copied file with an executable bit or a script extension (`.sh .bash .zsh .py .js .mjs .ts .rb .pl .ps1 .bat .cmd`), each inline shell span in Markdown (`` !`…` `` or a ```` ```! ```` block), and the frontmatter keys `hooks` and `allowed-tools`. The **code digest** is SHA-256 over the sorted code items: each path with its bytes, and each command string. *(Note 2026-11-18, N29 planning: text code items are hashed with CRLF read as LF, so a checkout's line endings do not change the digest; a change that only adds a CR before a LF is therefore not a new digest.)* *(Note 2026-11-18, N29 review: a copied file that a code item names by path is also code.)* *(Note 2026-11-18, N29 review: the digest also covers a hook's event and matcher and a server's type, cwd and env values; values are not displayed.)*
- A plugin without code is delivered when selected.
- A plugin with code is delivered only when **approved** **(author)**: `zygo marketplace add`, `zygo marketplace update`, `zygo plugins enable` and `zygo plugins approve <marketplace>:<plugin>` show each code item and ask; approval stores `(marketplace, plugin, code digest)` in `~/.local/share/zygo/approvals.toml`. The same file holds `[[workspace]]` entries (`id`, `digest`): the approvals of a repository's workspace files, written by `zygo workspace approve` (workspace definition design R§7.2); the plugin reader does not read them. A new digest needs a new approval; until then the plugin is not delivered and the launch prints one line naming `zygo plugins approve`. A launch never prompts, except the update prompt (§17 `zygo update`, auto-update design) *(2026-11-22, auto-update design)*; `--yes` approves in scripts. *(Note 2026-11-19, N30 review, from this text: the in-session refusal guards against mistakes, not a hostile agent; an agent that can run shell commands can run `zygo plugins approve --yes` outside a session. A 40-hex `ref` resolves only as a commit.)*
- Pre-approved: the built-in marketplace's plugins at the commit the zygo release pins (their digests are compiled into the binary), and team approvals listed in team config for team marketplaces.

### 15.8 Managing marketplaces

- `zygo marketplace add <source> [--name <n>] [--ref <tag|commit>] [--enable all|<plugin>,…]`: fetches the source; without `--ref`, takes the highest semver tag (`v` prefix allowed; no tag → refused, naming `--ref`); records `ref` and `commit`; checks every plugin (§15.6); lists each plugin with its version, license and code; writes the entry to user config. The name defaults to the marketplace's `name`.
- `zygo marketplace list`: name, source, ref, commit, origin (built-in, team, user), plugins.
- `zygo marketplace update [<name> | --all] [--accept]`: moves to the highest semver tag above the current one (`--accept` also takes a moved tag), shows version changes and changed code, and asks for approvals (§15.7).
- `zygo marketplace remove <name>`: removes a user entry; the built-in and team entries are refused (disable their plugins instead).

*(Note 2026-11-19, N40 planning: the highest semver tag counts pre-releases; no semver tag is refused naming --ref; a commit ref has no current version, so update leaves it; a bare update means --all; update of a built-in or team entry writes a user entry; add asks only for plugins that --enable selects.)*

### 15.9 Built-in marketplace (owner)

- `act101` = `github:act101-ai/act101`, at the tag and commit the zygo release pins; each zygo release moves the pin, and `zygo marketplace update act101` moves ahead of it.
- `act101:act-lite` (MIT) is `default_on`: it provides code navigation (§10.5) and the SDLC skills. A user may disable it.
- `act101:act101` (licensed Commercial by its publisher) is off until the user enables it.

## 16. Redaction

Both passes run in stage 6, last before the body leaves, on the body as earlier stages left it, before the model rewrite and translation. `count_tokens` is scanned too.

### 16.1 Secret pass (fails closed)

- Patterns: `[[redaction.pattern]]` `{ id, pattern (Rust regex), action = "redact" | "block", description }`. Built-in set **(author)**: provider API keys (Anthropic, OpenAI, OpenRouter, Google), cloud keys (AWS access key id, Azure storage key, GCP service-account private key), GitHub/GitLab tokens, Slack tokens, private-key PEM blocks, JWTs, `zyg_` tokens, generic `password=`/`secret=` assignments with ≥ 12-character values. The builder checks each pattern against public samples and sets ids.
- Scope: every JSON string value at any depth, except (a) the model's own reasoning, which must go back unchanged or the vendor refuses the request: `thinking` and `signature` of `thinking` blocks, `data` of `redacted_thinking` blocks, Responses `reasoning` items (the model only ever saw redacted input); and (b) binary payloads identified by their JSON path, never by their content: Anthropic `source.data` of an `image` or `document` block with `source.type = "base64"`; OpenAI `image_url.url` and `input_image.image_url` values that start with `data:`; `input_file.file_data`. Object keys are not scanned. *(Note 2026-11-19, whole-branch review: the skip covers only a placeholder `[REDACTED:<id>]` whose `<id>` is a configured pattern id. Patterns run on the whole string; a match is dropped only when it lies wholly inside such a placeholder, so a placeholder inside a secret does not split it.)*
- **act-lite store values (owner) *(A1)*.** In local mode, at session creation, the launcher reads the act-lite secret store for the repository (location and per-project id as act-lite defines them, https://github.com/act101-ai/act-lite#act-lite-env; the store shared with act101's `act`) and gives each stored value of 6 bytes or more to the proxy as an exact-value pattern, id `act_lite_secret`, action `redact`, checked before the regex patterns. The values stay in the proxy's memory; they never go to a log, file, metric or usage row (C3), and the pattern-set version includes a hash of the value set. No store, or a read failure: no exact-value patterns and one session-log line. Serve mode: the client sends no stored values to the team server; only the regex patterns apply there **(author)**. *(Note 2026-11-18, N10 planning: stored values shorter than 6 bytes are dropped before the patterns are built. If the exact-value pattern does not build, it is dropped with one session-log line that names no value, and the session runs on the regex patterns; if the regex patterns do not build, session creation is refused `503 zygo_redaction_unavailable` (C4).)*
- Session cache: a request re-sends the earlier conversation, so each scanned string's result (by a 128-bit hash of the string and the pattern-set version) is kept in a per-session cache (at most 64 MiB of entries, least recently used dropped). A cached string is not scanned again; its stored replacement is applied. The cost of a request is then proportional to its new strings, and the replacement stays byte-identical across requests.
- Matching: `RegexSet` to find which patterns match, then each pattern's regex left to right, non-overlapping; the earlier pattern in config order wins an overlap. *(Note 2026-11-18, N8 review: overlapping matches are merged into one span labelled with the earliest pattern's id, so no part of a longer secret survives.)* `redact` → the span becomes `[REDACTED:<id>]`; `block` → `422 zygo_redaction_block` listing ids only.
- Deterministic: the same input always gives the same bytes, so a re-sent redacted turn keeps the prompt cache.
- Bound: runs off the async executor with a deadline of 200 ms plus 50 ms per MiB of uncached strings; timeout or panic → `503 zygo_redaction_unavailable`. Strings scanned before the deadline are cached, so a retry continues where the last attempt stopped and a large first request does not block every later one. The session caches of §16.1 and §16.2 share one process-wide cap (`[proxy] redaction_cache_mib = 256`, least recently used dropped across sessions); a dropped entry is scanned again, with the same result. *(Note 2026-11-19, whole-branch review: the deadline starts after the cache lookup, every attempt scans at least one uncached string, and the session-lock wait is taken off the whole budget (base plus per-MiB), never past it.)*
- Placeholders in files: the session text (§8.6) says "`[REDACTED:<id>]` marks a secret zygo removed; never write it into a file." The `redaction` guard (always `enforce`, independent of work units) denies an `Edit`, `Write` or `NotebookEdit` whose new content contains `[REDACTED:` with that sentence as the reason. A config whose patterns do not compile is refused at load.
- Findings `(id, count)` go to the usage row; never the span. *(Note 2026-11-19, N12 planning: a `block` match is refused `422 zygo_redaction_block` with the message `blocked by redaction pattern: <id>[, <id>…]` (ids sorted, no matched text); a secret-pass timeout, or a panic in either pass, is refused `503 zygo_redaction_unavailable` with the message `redaction did not finish in time` (timeout) or `redaction failed` (panic); nothing is sent upstream. The usage row of a blocked request carries one count of 1 per blocked id. The time a request waits for its session's cache lock counts against the §16.1 and §16.2 deadlines.)*

### 16.2 Content pass (fails open)

- Purpose: words the user typed that are profane, slurs or explicit sexual terms are replaced with `🤬` before the model sees them. What the agent reads on the user's behalf (files, tool results) is not filtered.
- Gate: the request's `x-claude-code-request-class` is `main` or absent (OpenCode sends none); the secret pass runs first regardless. *(Note 2026-11-18, N2 round 2 finding: `compaction` and `auxiliary` requests re-send the user's typed text (fixtures/claude-code/requests, premises Q5), so the gate also admits those two classes; `subagent` stays excluded.)*
- **User-text paths.** `messages` shape (Messages, chat/completions): for each element with `"role": "user"`, `content` when a string; when an array, the `text` of each direct `"type": "text"` element. `input` shape (Responses): `input` when a string; else each item of type `message` (or none) with `"role": "user"`: `content` string, or each `input_text`. Not filtered: other roles, `tool_result` blocks and their nested text, system text, tools, images and files, thinking blocks.
- Accepted limit: a user who types a harness prefix (below) at the start of a message skips the content pass for that block; the secret pass still runs.
- **Harness text skipped:** text blocks starting with `<system-reminder>`, `<bash-stdout>`, `<bash-stderr>`, `<system-update>`; the text block directly after a `<command-message>` block (an expanded command file) — except that the typed command arguments in it are filtered: the `$ARGUMENTS` substitution and a trailing `ARGUMENTS: …` line are located and filtered **(author; the builder confirms both forms against captures of the pinned Claude Code)**; OpenCode only: `<file …>…</file>` and `<shell …>…</shell>` spans whose close tag is present (a missing close tag opens no span). *(Note 2026-11-18, N9 review: also skipped — text blocks starting with `Stop hook feedback:` or `SubagentStop hook feedback:`, and the `<command-name>` element (only inside a `<command-message>` block) *(N9 review round 2, 2026-11-18)*; both are written by the agent, not typed.)* *(Note 2026-11-18, N9 review round 3: in the expanded command file, a token is tested only when it equals, case-insensitive, a token of the typed command arguments; this covers `$ARGUMENTS`, positional and `ARGUMENTS:` forms alike. A word of the file itself equal to such a typed word is filtered too.)*
- **Tokens:** runs of `[A-Za-z0-9_]`; a token is tested when it is 3–40 characters, letters only, and not inside a whitespace-free run longer than 64 bytes. Lookup order: `deny` list (flag) → `allow` list (pass) → per-request verdict cache → classifier.
- **Classifier (author):** a Rust profanity library with severity categories (for example `rustrict`, feature `censor` only): flag at profane/offensive moderate-or-higher, sexual severe; a 3-letter token only at severe; a whole-word test on the censored form.
- Replacement `[redaction.content] replacement = "🤬"` (≤ 64 bytes, non-empty); `allow`/`deny` lists of lower-case letters, 3–40 characters, no word in both. Built-in lists are empty: no word list is shipped in the repository.
- Session cache: as in §16.1, each user-text string's filtered result is cached per session (key: hash of the string and the classifier and list version), so a re-sent turn is not scanned again and its bytes do not change between requests. Cost per request is proportional to the new user text.
- Bound: own 200 ms budget for uncached strings, deadline checked every 256 classifications and every 64 KiB scanned or written; on timeout the pass stops, strings not yet filtered go out unchanged in this request only (they are not cached, so the next request filters them), the request goes on, and the finding `content_timeout` is recorded. A panic refuses `503 zygo_redaction_unavailable`.
- Findings: `content_profane`, `content_offensive`, `content_sexual` with counts.
- Tests use tracer words configured in a test `deny` list; no profane word appears in the repository, tests included.

## 17. CLI surface

| Command | Does |
|---|---|
| `zygo claude [--profile <n>] [--passthrough] [--server <url>] [--compact <n\|off>] [--yolo] [-- <args>]` | Launch Claude Code. |
| `zygo opencode [--profile <n>] [--server <url>] [--compact <n\|off>] [--yolo] [-- <args>]` | Launch OpenCode (`--yolo` → `--auto`). *(Note 2026-11-19, N65 planning: `--auto` goes where OpenCode 1.18.16 accepts it: right after `run`, or first for the TUI; a subcommand without `--auto` gets none, and the session log says so.)* |
| `zygo serve [--listen] [--tls-cert --tls-key]`, `zygo serve token new <label>`, `zygo serve reload` | Serve mode. |
| `zygo status` | Mode, server, profile, work item, repo policy, session status (inside a session: context use, override, savings in tokens, the `spend:` line of §10.7, and the `runtime:` line (session run time; P3-13); a status reply with no `spend_usd`, from an older server, gives no `spend:` line). *(2026-11-22, HUD design)* *(2026-11-24, session run time design T§3.3)* |
| `zygo doctor` | Agent binaries and versions, secret store, keys present per provider, server reachable, plugin store, missing plugin commands, plugins also installed in the agent's own config, repo policy, and one icon test line: the Nerd glyphs of the status line and the active icon set (HUD design AU2) *(2026-11-22, HUD design)*; and one tmux line for workspaces: `tmux: <x.y> (minimum 3.2)`, `tmux: <x.y> (minimum 3.2; below minimum)`, `tmux: not found`, `tmux: cannot run: <error>` or `tmux: unknown (minimum 3.2)` (workspaces design W§12, AU7). |
| `zygo compact <tokens\|on\|off>` | §10.2.4. |
| `zygo profile list\|show\|use` | §11. |
| `zygo work [<id>\|clear]`, `zygo work report` | §13. |
| `zygo decision new\|ls\|show\|deps\|check\|ratify\|overturn\|review` | Delegated decision records in `docs/decisions/` of the current repository (decision records design D§10.1). |
| `zygo usage` | §10.7. |
| `zygo statusline [compact\|verbose] [--save]` | Status line style: a session change; `--save` also writes `[statusline] style` to user config; with no argument, prints the style and its source. The verb works the same in an OpenCode session (operator decision, built by HUD-11). *(2026-11-22, HUD design)* |
| `zygo marketplace add\|list\|update\|remove` | §15.8. |
| `zygo plugins list\|sync\|enable\|disable\|approve`, `zygo plugin check <dir>` | §15.2, §15.3, §15.6, §15.7. |
| `zygo keys set\|rm\|list <provider>` | §6.2. |
| `zygo config show\|check` | §6. |
| `zygo up [--loadout <n>] [--reset] [--no-attach] [--all] [--] [<name>]` | Create or restore the workspace of this repository, or `<name>`, in its tmux session and resume its agents; `--all`: every workspace that was up (workspaces design W§8). |
| `zygo down [--now] [--timeout <dur>] [--wait] [--all] [--] [<name>]` | Drain, save and stop the workspace; `--now` skips the drain; detached when run from an agent or from a pane of that workspace, unless `--wait` (workspaces design W§9, AU18). Also `/zygo:down [--now]` (§17.1) and the MCP tool `workspace_down` when `[features] workspace_tool` is on (§15.4). |
| `zygo workspace` | Status of this session's workspace, else of the folder's: its phase and one line per member (workspaces design W§12). |
| `zygo workspace approve [--yes] [--digest <hex>]` | Show this repository's workspace files and approve them on this machine (workspace definition design R§7.1); runs on native Windows. |
| `zygo ls` | One line per workspace with its main worktree, state, agents, attached clients and unread messages: `<name>  <main worktree>  <state>  agents <r>/<s>/<d>  clients <c>  unread <n>` (workspaces design W§12). |
| `zygo attach [--] [<name>]` | Attach or switch to a running workspace (workspaces design W§12). |
| `zygo loadout list`, `zygo loadout show <n>` | Loadouts with their source; one loadout in TOML (workspaces design W§11, W§12). |
| `zygo ws forget [--] <name>`, `zygo ws forget-pane <key>` | Delete a workspace's state and caches; delete one member record and its caches (workspaces design W§12). |
| `zygo ws snapshot`, `zygo ws run`, `zygo hook workspace` | Internal and hidden; the `ws` verbs do not sweep (§10.1; workspaces design W§12, AU44). |
| `zygo update [--version <v>] [--check]` | Replace the binary from the latest release, checksum verified, atomic rename. *(Note 2026-11-19, N36: on Windows a running .exe cannot be replaced by a rename, so the binary is renamed to zygo.exe.old, the new file is renamed in, and .old is deleted best effort; on macOS and Linux the replace is one atomic rename.)* The start check: `zygo claude`, `zygo opencode` and `zygo up` run the update check before the launch (`zygo up`: one time for the whole workspace, `--all` included, before it reads the workspace), at most once in 24 h: one GET for the release's public VERSION file, 2 s limit, the system proxy not used, the time and result kept in `~/.cache/zygo/update.json` (Windows: `update.json` in the zygo cache directory); a failure is silent and the launch continues. A newer release that was not skipped is asked about: `zygo <new> is available (you have <old>). Update now? [y/N/s]`. The answer is read without case. `y` or `yes` installs it by the path above, prints `updated to <new>; relaunch zygo` and exits 0 without starting the agent (a failed install prints `zygo update: <reason>` and exits with its code); `s` or `skip` records the skip and asks again only for a newer release; `N`, the default, and any other answer launch and ask again after 24 h. When other zygo sessions are live, the prompt comes after the line `<n> zygo sessions are running; they keep the old version until you restart them (zygo down / zygo up)`; on Windows the update is then deferred and the launch continues. No check and no prompt when stdin or stderr is not a terminal, with `-p` or `--print`, inside a zygo session (`ZYGO_SESSION` set, an empty value too), in a workspace pane (`ZYGO_WORKSPACE` set, an empty value too: `zygo up` gives it to every pane it starts, the first pane of a new session through `new-session -e`, so the panes do not ask again), or with the opt-out of §3 (`ZYGO_NO_UPDATE_CHECK` with any non-empty value). `zygo serve` does not check. *(2026-11-22, auto-update design)* `zygo ws run` runs the check before it cleans its environment, so a pane with `ZYGO_WORKSPACE` skips it; when it does install an update, `ws run` prints the relaunch line and then runs the updated binary with the same arguments and `ZYGO_RELAUNCHED=1`, which does not check again, so the pane still starts its member; a failed relaunch prints `zygo: cannot relaunch zygo ws run: <error>` and the member starts from the running binary (workspaces design W§8.2 step 1a). |
| `zygo help [<verb>]`, `zygo results`, `zygo redaction`, `zygo guards`, `zygo units`, `zygo compact-tool` | §17.1 *(A2)*. |
| `zygo mcp`, `zygo hook <name>` | Internal; started by the agents. |

`zygo hook statusline`: the status line is the HUD of `docs/specs/2026-11-21-statusline-hud-design.md`. It reads `GET /v1/zygo/sessions/{id}/status` (cached 30 s in the session directory) and the session's trimmed bytes from the local usage store, opened read-only; the whole run has a 3 s deadline. *(2026-11-22, HUD design)* *(Note 2026-11-19, N38, P-N28-1: on Claude Code 2.1.328 the status line is shown (`zygo ◇ … ◇ compact 777k`, `fixtures/claude-code/live/p-n28-1-pane.txt`) and its text is in no request sent upstream (C7 holds).)*

### 17.1 Slash commands *(A2)*

- **One rule:** `/zygo:<verb> <args>` typed in Claude Code does the same as `zygo <verb> <args>` run in the session's shell; one implementation serves both. `zygo hook prompt` (`UserPromptSubmit`) matches `^/zygo:([a-z][a-z0-9-]*)(?:\s+(.*))?$`, runs the verb, and prints `{"decision":"block","reason":"zygo: <reply>","hookSpecificOutput":{"hookEventName":"UserPromptSubmit","suppressOriginalPrompt":true}}`, so the line and the reply never reach the model (C7). The hook finishes within 5 s; a verb that cannot finish in time replies with the CLI command to run. OpenCode has no prompt hook: OpenCode users run the CLI form. *(Note 2026-11-19, N28 planning: the hook waits 4.5 s for the verb; past that it blocks with `zygo: run in your shell: zygo <verb> <args>`. A verb that panics lets the prompt through (C6). C7: `zygo status` and `zygo help` run with `ZYGO_SESSION` set (the agent's shell, §8.4) print `compact: override set` (or `none`) and `context: <used>`, and no window number, because their output can reach the model; the slash forms and the status line reach only the user and print the value. This is the one exception to "same reply".)* *(Note 2026-11-19, N28 round 2: trailing whitespace, a final newline included, is trimmed from the prompt before the match, so `/zygo:compact 600k` followed by a newline dispatches.)*
- **Scope:** a **session** change lasts until the session ends (§6.1 layer 7); `--save` also writes it to user config. A setting the agent reads only at start (plugins, profile, the MCP tool list) replies "applies at next launch" and writes user config. *(Note 2026-11-19, N49 planning: in a session a setting verb replies `<verb> <words> for this session`, adding `; saved to user config` with `--save`; outside a session `--save` is required (else exit 2, `zygo <verb>: not in a zygo session; add --save to change user config`) and the reply is `<verb> <words> saved to user config`. Limits are checked for the new setting only. If the session change succeeds and the user-config write fails, the verb exits 1 with `<verb> <words> for this session; saving to user config failed: <e>`. `compact-tool` always writes user config. A refusal exits 1. If the proxy refuses the `redaction content` PATCH with an HTTP error status, `overrides.json` is restored and the verb exits 1 with `redaction failed: <e>`. On a transport error or timeout, `overrides.json` keeps the new setting and the verb exits 1 with `redaction failed: <e>; the session may already use the new setting`. In a session the verb reads the old `overrides.json` once, by the session-file rule (workspaces spec AU105): the open does not follow a link and does not wait on a FIFO, only a regular file is read, and it is read whole. A missing file gives the default overrides, and the new overrides are built from the bytes of that one read. A link, a FIFO or another entry that is not a regular file at that path, or a read that fails, stops the setting verb with exit 1 and one line, `zygo <verb>: cannot read <full path>: <reason>` (the reason is `is a link; not read` for a link, `is not a regular file; not read` for a FIFO or another entry that is not a regular file, else the error), with no PATCH and no save. A session directory the client cannot use also stops the verb before any change. `redaction off` and `redaction secrets off` are refused with the secret limit, inside or outside a session, before any other check; any other `secrets` argument is a usage error.)*
- **Limits** (§6.1 layer 7) hold for every verb; a refusal names the limit that stopped it.
- **Workspace members** (workspaces design AU105): a session setting verb and `compact`, in the CLI and in the slash form, also write the new value into the member record when the session is a workspace member, after the outcome is final, so `zygo up` applies it at resume (§6.1 layer 7). When that write fails or is skipped, the reply, or the error text, ends with `; not saved for the workspace`; the exit code does not change. A session that is not a member gets no write and its usual reply.

| Verb | Does | Scope |
|---|---|---|
| `help [<verb>]` | Every verb with its arguments and current value; with a verb, its details | — |
| `status` | As `zygo status` | — |
| `usage` | The session's savings (§10.7) | — |
| `compact <tokens\|on\|off>` | §10.2.4 | session |
| `results on\|off\|threshold <bytes>` | Result trimming (§10.1) | session, `--save` |
| `redaction [content on\|off]` | Shows both passes; switches the content pass; the secret pass is status only | session, `--save` |
| `guards [<guard> <level>]` | Levels with their sources; sets one guard's level, never below its floor (§13.5) | session, `--save` |
| `units on\|off` | Work units (§13) | session, `--save` |
| `statusline [compact\|verbose]` | Status line style (§17); the same in an OpenCode session (operator decision, HUD-11) *(2026-11-22, HUD design)* | session, `--save` |
| `work [<id>\|clear]` | Current work item (§13.2) | repository; session text at next launch |
| `plugins [list\|enable\|disable\|approve <m>:<p>]` | §15.2, §15.7 (act-lite navigation included) | next launch |
| `marketplace [list\|add\|update\|remove …]` | §15.8 | user config |
| `profile [<name>\|none]` | Default profile (§11.2) | next launch |
| `compact-tool on\|off` | The `compact` MCP tool (§10.3) | next launch |
| `down [--now]` | Drain, save and stop the workspace of this session's membership marker, never the folder's; detached, so it replies within the hook bound (§17 `zygo down`; workspaces design OP11, AU18). A session that is not a member: `zygo: this session is not in a workspace`, exit 1 | — |
| `ls` | As `zygo ls` (workspaces design OP11) | — |
| `workspace` | As `zygo workspace` (workspaces design OP11) | — |
| `decision <args>` | Replies `run in your shell: zygo decision <args>` and reads no file: the slash form runs inside the hook (decision records design D§10.1, D-C2) | — |

- **Code approval stays in the shell (author):** as a slash command, `marketplace add|update` and `plugins approve|enable` list the code and print the exact `zygo` command that approves it; the hook cannot ask a question. *(Note 2026-11-19, N50 planning: as a slash command, `marketplace add|update`, `plugins sync` and `plugins enable` of a plugin not in the store reply `run in your shell: zygo <verb> <args>` and change nothing; these slash forms do not list code, because listing needs a fetch that the 5 s hook bound does not allow, so the shell command lists the code and asks. `plugins enable` of a stored plugin and `plugins approve` list the code from the store and print `run: zygo plugins approve <id>`. These slash forms (`marketplace add|update`, `plugins sync|enable|approve`) start no git process; apart from the user-config write of `plugins enable|disable` (scope: next launch), they write no file. Session setting verbs write `overrides.json`, and user config with `--save`, by design (§17.1 Scope).)*
  *(Note 2026-11-18, N30 planning: approval is refused when the command runs inside an agent session (ZYGO_SESSION_DIR set); `zygo plugins sync` is not a slash verb.)*
- **Secret redaction (author):** no verb turns it off; only config files can (§15.4).
- Proxy-side settings (`redaction.content`) go through `PATCH /v1/zygo/sessions/{id}/settings`; hook-side settings (results, guards, units) are read from `overrides.json` on every hook call. *(Note 2026-11-18, N10 planning: `PATCH` merges: only the fields present change; the reply is the session's full settings.)*

## 18. Decisions and alternatives

### 18.1 Owner's calls

| Id | Decision |
|---|---|
| O1 | Remove users, quotas, policies, admission tests and the portal; keep passthrough, every token-saving feature, plugin delivery, elephant-goldfish and model profiles. |
| O2 | Provider flags such as ZDR are optional. |
| O3 | One binary, local mode by default, serve mode for teams. |
| O4 | Keep secret and profanity redaction. |
| O5 | Work units take hints from Beads, GitHub issues and similar sources. |
| O6 | Passthrough v1: claude.ai with Claude Code; v2: Codex and Grok. |
| O7 | Agents v1: Claude Code, OpenCode; v2: Codex, Grok CLI; later: Pi. |
| O8 | In serve mode, redaction runs on the team server; no local relay. |
| O9 | The name is `zygo` *(A1)*. |
| O10 | Users add external marketplaces (the act101 marketplace among them) and zygo delivers their plugins to the agents *(A1)*. |
| O11 | The act101 marketplace is built in; `act-lite` is on by default; the commercial `act101` plugin is off until enabled *(A1)*. |
| O12 | Code navigation and the SDLC skills come from act-lite; zygo keeps no overlapping feature *(A1)*. |
| O13 | zygo keeps its proxy secret pass and adds act-lite's stored values to it *(A1)*. |
| O14 | Every internal feature has a `/zygo:` command to enable, disable or configure it, and `/zygo:help` lists them *(A2)*. |
| OP1 | Workspaces: workspace, loadout, one tmux session per workspace, `zygo down`, `zygo up`, CLI verbs to manage sessions; no pause or resume verbs (workspaces design). |
| OP2 | A workspace is one repository, identified by the git common dir; the main checkout and every linked worktree are the same workspace (workspaces design). |
| OP3 | One tmux session per workspace (workspaces design). |
| OP4 | `zygo up` resumes each agent's own conversation by its saved agent session id (workspaces design). |
| OP5 | Native Windows has no workspace features: each workspace verb exits 2 with `workspaces need tmux; run zygo in WSL`; `zygo claude` works as in v1; the multiplexer is behind a trait, and tmux is the only implementation (workspaces design). |
| OP6 | `zygo down` drains by default and blocks new subagent dispatch during the drain; `[workspace] drain_block_dispatch = true` is the default (workspaces design). |
| OP7 | zygo snapshots the full workspace state, layout included, on every change while the workspace runs, not only at `zygo down` (workspaces design). |
| OP8 | A pane that ran a zygo agent session is resumed; every other pane comes back as a plain shell in its saved cwd; zygo never runs a saved non-agent command again (workspaces design). |
| OP9 | `zygo claude` and `zygo opencode` outside a workspace stay a clean v1 launch; only `zygo up` creates or restores a workspace (workspaces design). |
| OP10 | The name is "loadout"; "profile" stays the model profile (§11); a loadout pane can name a model profile (workspaces design). |
| OP11 | `down`, `ls` and `workspace` are also slash commands (`/zygo:down [--now]`, `/zygo:ls`, `/zygo:workspace`), per O14; `up` and `attach` stay CLI-only; `/zygo:down` takes the detached path (workspaces design). |
| OP12 | A member's session overrides and compaction override are saved on each change and always applied at resume, so a crash or reboot does not lose them (workspaces design). |
| OP13 | The session status gains `requests_in_flight` and `last_request_end`, on team servers too (workspaces design). |

### 18.2 Author's calls (the owner checks these)

| Id | Decision | Why |
|---|---|---|
| A1 | Passthrough is local-mode only. | The subscription credential never transits a shared server; lowers the terms-of-use exposure to the user's own machine. |
| A2 | Local mode runs the proxy inside the launcher, one per session; no daemon. Workspaces keep this: tmux holds the processes (workspaces design AU1). | Nothing to install or supervise; session state dies with the session, except the workspace state, which outlives sessions (workspaces design AU1). |
| A3 | Serve mode is single-instance with in-memory sessions; a restart ends sessions. | No database or cache dependency for a team tool. |
| A4 | The session token is stored in a `0600` file in the session directory, not in the environment (except `X-Zygo-Token` in passthrough, which the agent requires). | Tool subprocesses inherit the environment; the redaction pattern for `zyg_` tokens is the second line of defence. |
| A5 | Configuration is layered TOML; the repository file may set only guards, navigation, limits and work sources. | A cloned repository cannot change providers, keys or redaction. |
| A6 | Model profiles live in client config; serve mode validates them at session creation and stores nothing per user. | No user store. |
| A7 | Plugins come from git marketplaces pinned to tags or commits; serve mode lists marketplaces but does not host archives. | Reproducible snapshots without a registry. |
| A8 | *(A1)* A plugin's code (MCP and LSP servers, hooks, scripts, inline shell) is delivered only after the user approves that plugin's code digest; a new digest needs a new approval; built-in marketplace plugins come approved by the release. | Approval per plugin and per code version, not per marketplace: a marketplace with one plugin that needs code no longer has to be trusted as a whole, and an update that adds code cannot slip in. |
| A15 | Keep the response-time usage rewrite for compaction, with `behavesAs` picker rows and a 90 % safety window. | Claude Code's documented window settings are static per launch; the rewrite is the only way to honor a mid-session override and the model's own `compact` call. |
| A16 | Preflight refuses a launch when the user's or the project's agent settings would replace the session credential or bypass the proxy. | The agent's credential order puts those settings above the session key helper. |
| A17 | The env denylist covers model-provider credentials only. | Broader patterns removed `GH_TOKEN`, cloud profiles and package tokens the agent's own work needs. |
| A9 | *(A1)* Withdrawn: code navigation comes from act-lite (O12). | — |
| A18 | *(A1)* OpenCode gets plugin skills, agents, commands and MCP servers under `<plugin>-<name>`. | OpenCode has no plugin namespace; the prefix removes name clashes between plugins (act101 and act-lite share five skill names). |
| A19 | *(A1)* A plugin MCP server whose command is missing is left out with one line; zygo installs nothing. | Installing third-party programs is outside zygo's job and its trust model. |
| A20 | *(A1)* Plugin sources are relative paths inside the marketplace in v1. | Every plugin is then covered by the marketplace's pin. |
| A21 | *(A1)* In serve mode the client sends no act-lite stored values to the server. | Stored secrets the request does not contain never leave the machine. |
| A23 | *(A2)* No slash command turns off secret redaction. | A typed line inside a session should not remove the fail-closed protection. |
| A24 | *(A2)* Code approval from a slash command only lists the code and prints the CLI command. | The prompt hook cannot ask the user a question. |
| A22 | *(A1)* A `work-graph` work-item source reads unit ids, check and files from an active act-lite work graph. | The node contract already holds them; agents do not declare them twice. |
| A10 | OpenCode's soft window is its `limit.context`; the SSE rewrite is Claude Code only in v1. | The rewrite's premise (the agent measures context from `message_delta` usage) is verified for Claude Code only. |
| A11 | The usage store is local SQLite with 90-day retention; costs are estimates at list price. | Shows savings without a server or accounts. |
| A12 | Default guard levels `monitor`, trailers `notify`. | Guards collect data before they block anyone. |
| A13 | The profanity pass is on by default. | O4; a user turns it off with `redaction.content = false`. |
| A14 | The command-argument part of an expanded command block is filtered. | Otherwise a typed argument bypasses the content pass. |
| AU1-AU105 | The workspaces author's calls are the rows AU1 to AU105 of the workspaces design W§6.2 (`docs/specs/2026-11-20-workspaces-design.md`), kept there with their reasons and code references. Withdrawn ids keep their rows: AU21, AU23, AU33, AU40, AU47, AU49, AU50, AU51, AU66, AU81, AU82, AU101. | One record for each decision; the ids stay traceable to the reviews of that design. |

### 18.3 Alternatives rejected

- **A background daemon for local mode.** Adds install, upgrade and lifecycle work for no gain over a per-session proxy.
- **Passthrough through serve mode.** Moves the user's subscription credential through a shared server (A1).
- **Claude Code's documented window settings alone** (`autoCompactWindow` in `modelSettings`, `--autocompact`, `CLAUDE_CODE_AUTO_COMPACT_WINDOW`, `CLAUDE_CODE_MAX_CONTEXT_TOKENS`). They are read at launch and keyed by the model the agent picked, so they cannot follow a mid-session override, a profile that serves another model, or the model's own `compact` call. Used where they fit: OpenCode's `limit.context` (§10.2.5).
- **Hard refusal above the soft window.** Breaks a turn in progress; the response-time compaction signal does not.
- **Telling the model its smaller window.** Context pressure degrades results (C7).
- **A cost or quota price for the override.** Quotas are a non-goal; the override is a plain session setting.
- **Shipping a profanity word list.** A word list in the repository is the content the feature removes; a library plus empty config lists keeps the repository clean.
- **Plugins carrying their own MCP servers by default.** A plugin would run any program on every machine it reaches (A8).
- **Trust per marketplace** *(A1)*. One code-carrying plugin forced trust in the whole marketplace, and later updates were trusted unseen.
- **zygo's own navigation tools** *(A1)*. They duplicated act-lite (O12).
- **Routing by vendor model names in skills.** Skills name roles or capabilities; the alias sentence covers third-party skills.
- **Workspace alternatives** (workspaces design W§14, each with its reason there): a supervisor daemon; messages, wake, triggers, git modes, pools, slots, toasts and restart briefs; a fresh restart by default; a Windows Terminal backend; a one-agent Windows mode; auto-tmux on `zygo claude`; loadouts in `.zygo.toml`; periodic-only snapshots; tmux-resurrect or tmux-continuum; Claude Code background sessions; a shell string as the pane command; members keyed by tmux pane id (`%N`); `down` in the foreground when an agent runs it; a refused `down` while a shell pane runs a program; `up` holding the lock while it creates panes; a snapshot that deletes the record of each pane that is gone; a stable layout with candidates and promotion, and tombstones for `kill-pane`; launch arguments saved unfiltered; provider keys removed from tmux and `ws run`; one plugin sync per `up`; the `-L <label>` and `default` server forms; `results/` moved out at exit and back in at resume; overrides saved only at `down`; no slash commands for workspace verbs; a `resume_failed` state and `--retry-failed`; gate bypass flags on `up --all`; a drain idle rule from notifications and a quiet time; a `window-pane-changed` hook.

## 19. Premises the first work verifies

Each is checked against the pinned agent version before the work that depends on it is planned. A failed premise applies its fallback, recorded as a dated note in this document.

| Id | Premise | Evidence so far | Fallback |
|---|---|---|---|
| P1 | Claude Code compacts before its next request after a streamed response whose `message_delta` usage (and its `iterations` last entry) reports a full window, after `end_turn` and after `tool_use` (the tool result is appended, the conversation compacts, the loop continues). | Observed on Claude Code 2.1.319 in earlier work by the author: both stops, with the `iterations` entry required. | Trigger 0 and Trigger 2 rely on the reactive `prompt is too long` path (§7.6) only; the `compact` tool is removed (A-17 none). *(Note 2026-11-18, N2, P1: verified on Claude Code 2.1.328 for `end_turn`, `tool_use` and a subagent; the compaction follows with or without the `iterations` entry, so on this version the entry is not required — the rewrite still writes it when present.)* |
| P2 | `modelPicker` rows with `behavesAs` make Claude Code size a role's window and enable effort and thinking as for that Claude model. | Documented at https://code.claude.com/docs/en/model-config. | Set `CLAUDE_CODE_MAX_CONTEXT_TOKENS` per launch to the main role's window; accept no effort control for roles. |
| P3 | The hook's `agent_id` equals `x-claude-code-agent-id` (§13.6). | Not yet checked. | Cut units over the whole session. |
| P4 | OpenCode's inline config outranks project configs for the keys `zygo` sets (§8.7). | Not yet checked. | The preflight refuses every conflicting project config (already specified). |
| P5 *(A1)* | The pinned OpenCode reads `mcp` entries, subagents and commands from the inline config or `OPENCODE_CONFIG_DIR` (§15.3). | Not yet checked. | The parts it cannot read are not delivered; the OpenCode conformance row says so. |
| P6 *(A1)* | `act-lite status` reports its language list in machine-readable form, and act-lite's MCP tool names are `skeleton`, `symbols`, `symbols_batch`, `repo_outline` (§10.5). | act-lite 1.2.3 README lists the tools. | The navigation guard uses a language list compiled into zygo for the pinned act-lite version. *(Note 2026-11-18, N2, P6a: failed on act-lite 1.2.3 — it has no `status` subcommand, and its MCP `status` tool lists language names without extensions; fallback applied: the navigation guard uses the extension list of act-lite 1.2.3 compiled into zygo from `fixtures/act-lite/languages.json`, see §10.5. P6b verified: `act-lite mcp serve` lists `skeleton`, `symbols`, `symbols_batch`, `repo_outline`.)* |

The workspace premises PW1-PW17 are in the workspaces design W§17, each with its evidence, result and fallback, and their results are rows of `docs/premises.md`: verified PW1, PW2, PW3, PW4 (on OpenCode 1.19.6), PW5, PW9, PW10, PW11; failed, with the fallback in effect, PW6, PW7, PW15, PW16; withdrawn PW8, PW12, PW13, PW14, PW17.

## 19a. Left to the builder

- Crate layout, HTTP framework, SSE parser, SQLite library, secret-store library.
- Exact error message texts beyond those quoted; adjustment code names beyond those listed.
- The built-in catalog rows, default role models and secret patterns, each verified with a recorded source.
- Which Claude Code event can continue a subagent for the claims guard (§13.4), verified on the pinned version.
- The OpenCode config mechanism details (§8.7), verified on the pinned version.
- The exact `modelPicker` row fields on the pinned Claude Code version (§8.5; `behavesAs` is documented at https://code.claude.com/docs/en/model-config).
- The Claude Code tool output shapes for `updatedToolOutput` per hooked tool (§10.1).

*(Note 2026-11-18, N2, Q1: verified — `SubagentStop` with `decision: "block"` continues the same subagent once; the pinned version has no separate handback `PreToolUse` event.)*

*(Note 2026-11-18, N2, Q4: verified — `updatedToolOutput` is the tool's own `tool_response` object with its text field replaced: Bash `stdout`, Read `file.content`, Grep `filenames` or `content`, Glob `filenames`, WebFetch `result`. Counts: Read builds the line-numbered view from `file.content` alone, a stale `numLines` is not shown; Grep files mode prints `Found <numFiles> files`, so the hook sets `numFiles` to the number of names it returns. On 2.1.328 `Grep` and `Glob` are not in the default tool set — they exist only when `--tools` names them — so in a default session searches run through `Bash`. Shapes: `fixtures/claude-code/tool-output/`, results: `fixtures/claude-code/requests/q4-result.json`.)*

*(Note 2026-11-18, N2, Q5: verified without `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC`, as a zygo launch runs — request classes seen: `main`, `subagent`, `compaction`, `auxiliary`; no `background` class was seen. `auxiliary` covers session-title generation, which carries the user's typed text, prompt suggestions and WebFetch's secondary call. A compaction request has class `compaction`, not `main`, and re-sends the conversation. A `--agents` subagent sends `x-claude-code-agent-type: custom`; its hook `agent_type` is its name. §16.2's gate, class `main` or absent, therefore leaves user text in `compaction` and `auxiliary` requests unfiltered. `fixtures/claude-code/requests/q5-hint-headers.json`.)*

*(Note 2026-11-18, N2, Q7: failed, in the sense "cannot be observed without administrator rights" — every Claude Code and OpenCode key, env row and flag zygo writes is in the pinned `--help` or schema and shows its effect in a capture. Not observed: the managed keys zygo only reads in the §8.4 preflight, `allowManagedHooksOnly: true` and a managed `disableAllHooks: true`; Claude Code honors them only from the system managed-settings file, which needs administrator rights, and 2.1.328 ignores `CLAUDE_CODE_MANAGED_SETTINGS_PATH`. A `disableAllHooks: true` in `--settings` was observed to stop every hook. Fallback applied: zygo writes nothing for these keys; the preflight keeps its stop on them from the documentation and the schema, and the A-21 premise test checks the schema only for them, see the A-21 row. `docs/premises.md`.)*

## 20. Acceptance

Each item is a test or a command whose output decides it. "Mock upstream" = an HTTP server in the test that records what it receives and replies with fixture SSE.

**Launch and wrapping**
1. Stub-agent launch test (Linux, macOS, Windows) for each agent: the child environment has the §8.4/§8.5 (or §8.7) rows and none of the denylisted names; the generated settings, `--agents`, MCP and plugin arguments match fixtures.
2. A premise test reads the pinned agent's `--help` or schema and finds every setting key and flag `zygo` writes.
3. `zygo hook role-guard` on a fixture main-thread `Edit` input prints the deny JSON; with `agent_id` (also `null`) or a `Bash` input it prints nothing; on `not json` it exits 0 with empty stdout.
4. With `--model code`, no role-guard hook is registered; with `--model lead` it is.
5. The session text starts with the `lead` prompt, contains the legend and (Claude Code) the alias sentence; with `disabled = ["design"]` it has no `zygo-design` and no `fable` clause.
6. A `writes = false` role's subagent definition has the edit tools disallowed (Claude Code) / `permission.edit = "deny"` (OpenCode).

**Proxy**
7. Mock upstream: a request for role `code` reaches the provider of the role's model with `model` rewritten to `upstream_model`; the usage row has role `code`, the model, the token counts.
8. With a profile override for `code`, the override's model serves and `X-Zygo-Model` names it; a disabled role → `400 zygo_role_disabled`; an unknown name → `400 zygo_unknown_model`.
9. Translation: a Messages streaming request served by a Responses-only mock returns a valid Messages SSE stream with tool calls and usage; the effort clamp records `effort_clamped:<from>><to>`.
10. Strict-schema fixtures: each `refuse` rule rewrites its construct and the injected nulls are removed from the returned tool input.
11. Fallback: the first provider returns 500 before streaming; the second serves; the pin moves to the second.
12. `zdr = true` on an `openrouter.ai` provider: the upstream body has `provider.zdr = true`, other keys kept. `require_zdr = true` drops a provider without it.
13. Serve mode: a wrong or missing team token → `401`; a non-loopback listen without TLS refuses to start; a revoked token fails after `zygo serve reload`; a session without heartbeat for 5 min is suspended (`401 zygo_session_suspended`) and resumes with its state on the next heartbeat; the client refuses `--server http://10.0.0.5:8443` with exit 2.
13a. Mock upstream on a non-Anthropic host: no `authorization` from the client, no `x-api-key` from the client, no `x-zygo-*`, no `anthropic-beta`, no `x-claude-code-*` header arrives; the provider's own key does. *(Note 2026-11-18, N11 planning: "non-Anthropic host" = a provider whose dialects do not include `anthropic` (§7.3 Outbound headers); the request is translated, so the HTTP test is N13's; N11 tests the header rule at function level.)*
13b. Preflight: a fixture `~/.claude/settings.json` with `env.ANTHROPIC_API_KEY`, a project settings file with `CLAUDE_CODE_USE_BEDROCK`, and a project `.opencode` config defining provider `zygo` each stop the launch with exit 2 naming the file; `GH_TOKEN` in the parent environment reaches the child.
13c. Translation round trip: a two-turn tool loop through a Responses mock returns `thinking` blocks with `zygo1:` signatures, and the second request upstream carries the reasoning items; an upstream context-length error reaches the client as `400` with `prompt is too long`.

**Token saving**
14. A 100 KiB `Bash` output through `zygo hook trim` returns a view ≤ 8 KiB with the marker line; `result_view`, `result_search`, `result_tree` read the stored bytes; the trims table has the row.
15. Soft window, mock upstream: a `main` streamed response with `stop_reason = end_turn` and `prompt > soft` arrives at the client with the rewritten `message_delta` usage (and the `iterations` entry when present), every other byte unchanged; `tool_use` stop, a subagent class, a compaction request, or `prompt ≤ soft` arrive byte-identical.
16. A response containing a `tool_use` of the `zygo` server's `compact` tool is rewritten regardless of size.
16a. Safety window: a role served by a non-Claude model with `context_window = 400000` and no `soft_context_window` is signalled at a `prompt` above 360,000, also with the override `off` or `2m`, also on a `subagent` request with `stop_reason = tool_use`.
16b. Live check (manual, recorded): a subagent on a small-window model that overruns its window recovers through reactive compaction after the mapped `prompt is too long` error.
17. `/zygo:compact 600k` through `zygo hook prompt` prints the block JSON and the session's override is 600,000; `/zygo:compact off` disables trigger 1; `on` clears it; `/zygo:frob` blocks with the unknown-verb text.
17a. *(A2)* Each §17.1 verb typed as `/zygo:<verb>` gives the same reply and effect as its CLI form; the line is blocked and absent from the upstream request.
17b. *(A2)* A session change is gone in the next session; with `--save` it is in user config. A guard below `min_level`, a `required` plugin, a team-set key and `redaction off` are each refused with the limit named.
17c. *(A2)* `/zygo:help` lists every verb that has a `commands/<verb>.md` in the embedded plugin, and each of those works; an unknown verb blocks with the help text.
18. Cache markers are added for OpenCode only when no `cache_control` exists; never on `count_tokens`.
19. *(A1)* With `act101:act-lite` selected and a stub `act-lite` on `PATH`, the session text has the navigation sentence and the navigation guard at `enforce` denies a `Read` of a 500-line Rust file with no earlier act-lite `skeleton`; with `act-lite` absent from `PATH`, neither the sentence nor the guard is present and the launch prints the missing-command line.
20. `zygo hook pre-compact` with one open unit prints the keep/drop text naming the unit.
21. Live check (manual, recorded): a Claude Code session against the local proxy with `soft_context_window` set low compacts after the turn that crosses it (the next request carries `x-claude-code-compaction`).

**Status line** *(2026-11-22, HUD design)*
21a. The status line HUD passes the acceptance items #1 to #37 of `docs/specs/2026-11-21-statusline-hud-design.md` H§14: rendering (#1-#7); ctx and threshold (#8-#9); spend and tokens (#10-#17); trimmed tokens (#18-#19); width (#20-#21, H§6.7), where the fit width W is the terminal width less 4 and the terminal width comes from `/dev/tty`, else `COLUMNS`, and W is 120 when neither answers; input, deadline and failure (#22-#30); icons and style (#31-#35; except the OpenCode clause of #33: in an OpenCode session the `statusline` verb works as in a Claude Code session, operator decision); the workspace phase, with the workspaces build (#36); the live check P-HUD-1 (#37). Item #38 (W-SHELL part 2, Windows) is not run: no Windows machine is available to the executor.

**Workspaces**
21b. The acceptance items #1 to #91 of `docs/specs/2026-11-20-workspaces-design.md` W§18: identity, state and validation (#1-#12); `up` and restore (#13-#39); `down` and the drain (#40-#53); membership and isolation (#54-#61); config and platform (#62-#65, #64 on the Windows CI job); slash commands, saved overrides, OpenCode and the membership path (#66-#75); the live checks (#76; results in `docs/premises.md`, §19); the items added after the live checks and with later work (#77-#91).

**Profiles**
22. Each §11.3 violation yields its message and the launch continues without the profile.
23. A custom role appears in `/v1/models`, the legend and `--agents` with its prompt, and serves its model.

**Passthrough**
24. `zygo claude --passthrough` (stub agent): no model env rows, no `apiKeyHelper`/`availableModels`/`model` in settings, `X-Zygo-Token` in custom headers, subagents without `model`; `--model lead`, `--fallback-model code`, a user `--agents` with `"model": "code"`, and `--bare` each exit 2.
25. Mock upstream: `Authorization: Bearer sk-ant-oat-x` + valid `X-Zygo-Token` reaches upstream with that authorization, no `x-api-key`, no `x-zygo-*`, model unchanged; the usage row has `role = subscription`, cost 0.
26. Missing authorization, `x-api-key`, an `zyg_` bearer, or another bearer → `401 zygo_passthrough_no_login`, nothing upstream. A passthrough session request to serve mode → `403 zygo_passthrough_local_only`.
27. Secret redaction applies on the passthrough path as on the normal path.
28. Live check (manual, recorded, needs the owner's login): one `zygo claude --passthrough -p` answer.

**Redaction**
29. A body with an Anthropic key in a tool result arrives upstream with `[REDACTED:<id>]`; a `block` pattern refuses `422` listing ids only; an image block's `source.data` is not scanned, while a 70 KiB base64-looking tool-result string with a key at its end is redacted.
29d. *(A1)* A value from a fixture act-lite store appears in a tool result and reaches upstream as `[REDACTED:act_lite_secret]`; a stored value of 5 bytes is not redacted; in serve mode the client sends no stored value.
29c. A `thinking` block containing `password="fixture-pass-123"` reaches upstream byte-identical; an `zygo1:` block for another upstream model is dropped with `thinking_dropped:1`.
29a. The same conversation sent twice: the second request scans only its new strings (counter in a test hook) and the redacted bytes are identical.
29b. `zygo hook` for the redaction guard denies a `Write` whose content contains `[REDACTED:` and allows one that does not.
30. A secret-pass timeout (test hook forcing it) → `503 zygo_redaction_unavailable`; no upstream request.
31. A tracer word from a test `deny` list in a user text block becomes `🤬`; the same word in a `tool_result`, a `<system-reminder>` block or an assistant message passes; in a slash-command argument it is replaced.
32. A content-pass timeout (forced) sends the request and records `content_timeout`.
33. No usage row, log line or file written during tests 29–32 contains the tracer word, the secret or `🤬` (grep over the test's state directory and captured logs).

**Work units**
34. `work_begin` through the hook writes the `begin` line; a matching check run in the unit's repository records `check: passed`; an edit after it makes the claim stale; `SubagentStop` writes the `work_units` row with the folded facts.
35. Source detection: a branch `42-fix-x` with `github` gives `<owner>/<repo>#42`; a repository with `.beads/` and branch `bd-a1b2-…` gives `bd-a1b2`; a missing `bd` or `gh` keeps the id with no title.
36. Each guard at each level on fixture hook inputs prints the specified output (nothing, `allow` + context, `deny`/continue) and appends its event line.
37. `.zygo.toml` on the default branch sets levels; the same file only in the working tree does not; `min_level` raises a lower level.

**Plugins**
38. *(A1)* `zygo plugin check` fails each structure violation on fixtures (bad name, file over 1 MiB, plugin over 4 MiB, symlink, path outside the plugin, version mismatch) and, for one fixture per code kind (`.mcp.json` server, hook command, `scripts/`, executable bit, script extension, inline shell span, ```` ```! ```` block, `opencode/plugin.js`, frontmatter `hooks`), lists the item and a digest that changes when the item changes. A moved tag keeps the recorded commit until `zygo marketplace update --accept`.
38a. *(A1)* A selected plugin with code is not delivered until `zygo plugins approve`; after a change to one script it is not delivered again until re-approved; a plugin without code is delivered without approval; the built-in `act-lite` at the pinned commit is delivered with no prompt.
38b. *(A1)* `zygo marketplace add <fixture repo>` without `--ref` records the highest semver tag and its commit; with no tags it refuses; `update` moves to a higher tag; `remove act101` is refused.
39. Sync from a fixture git marketplace at a tag fills the store; a failed fetch keeps the previous store and the launch continues with `plugins: offline`.
40. The Claude Code snapshot loads `<plugin>:<skill>` (premise test with the pinned agent). *(A1)* The OpenCode snapshot places each skill as `<plugin>-<skill>` with its `name` rewritten, `plugin-<plugin>.js`, an `mcp` entry `<plugin>-<server>` per `.mcp.json` server, and agents and commands as `<plugin>-<name>`; a server whose command is missing is left out with its one line.
40a. *(A1)* With no user config, a Claude Code launch delivers `act101:act-lite` (skills and MCP server) and not `act101:act101`; after `zygo plugins enable act101:act101` and approval, both are delivered.

**Distribution and hygiene**
41. The install script installs without elevation, verifies the checksum and is idempotent; `--dry-run` prints the steps.
42. `grep -ri` over the repository for every word in the forbidden-content test list returns nothing (no profane words; no credentials).

---

## Change log

| Date | Section | Decided by | Reason |
|---|---|---|---|
| 2026-11-21 | §20 #11 (tests of §7.3 fallback and §10.4 pin removal); no text change | author's call | Found defect: the test helper refused_port released its port, another test's listener could take it, and fallback_all_failed_removes_pin failed once in three full runs. Node FIX-1 uses a fixed port below 1024. |
| 2026-11-22 | §3, §6.1, §6.3, §7.2, §10.2.2 (N15 note), §10.2.4, §10.7, §14, §15.7, §17 (status line paragraph, `zygo status`, `zygo statusline`, `zygo doctor`, `zygo update` rows), §17.1, §20 (21a) | operator (statusline HUD design H§9 and auto-update design "Base-spec changes", both approved 2026-11-21); texts by DOC-1, corrected against the code in its review round 2 | The two approved designs change these sections. Each section is rewritten in place, and its new text is marked *(2026-11-22, HUD design)* or *(2026-11-22, auto-update design)*. The `zygo up` start check and the W§15 rows are workspaces D1's. |
| 2026-11-22 | §14 (`upstream_errors`, "Never" line) | author's call (FIX-10, from FIX-8) | §14: table `upstream_errors` records the upstream error type and message of a non-2xx passthrough answer; LIV-1 D8, FIX-8, FIX-10. |
| 2026-11-22 | §7.2 | operator | no TTL was requested; sessions end with their launcher or a server restart |
| 2026-11-22 | §8.5, §8.6 item 7, §13.4 (attribution row, known limits) | operator ("THERE IS NEVER CLAUDE ATTRIBUTION. LAUNCH A SUBAGENT RIGHT NOW TO CONFIRM THAT ZYGO CORRECTLY FORCES NO-ATTRIBUTION"; earlier "NO CLAUDE ATTRIBUTION IN ANY COMMITS, OR PRS") | The check found zygo did not force it: on Claude Code 2.1.330 a commit in a session with zygo's settings file got a Claude co-author trailer. zygo now forces it in the settings file, the session text and an always-on guard (FIX-21). Round 2 (review, author's call filling the operator's "zygo must force it"; §8.3, §8.5 settings note and Flags, §13.4 attribution row and known limits): a user `--settings` value is merged into zygo's file with the attribution keys and zygo's hooks kept, and only an unreadable value keeps the replace path; the guard searches only each message command (its unquoted arguments, its heredocs and its files read whole), follows `cd`, `pushd`, groups and subshells, sees through `timeout`, `nice`, `nohup`, `exec`, `stdbuf` and shell `-c` strings, also covers `git merge`, `git commit-tree`, `git notes add|append` and `gh pr merge`, matches `=` and blanks before the co-author separator, a co-author name with Claude anywhere, the generated-by line, the robot line with U+FE0F and a Claude session link, and names where it found the text; the live check (`scripts/premises/live-attribution.sh`) found no attribution in a zygo session's requests or commit. |
| 2026-11-22 | §8.3 (last item), §8.5 Flags | controller decision (FIX-21 round 3) following the operator's rule ("there is NEVER Claude attribution; zygo must force it") | A user `--settings` value that `zygo` cannot read as a JSON object was passed on after `zygo`'s file, and Claude Code then stopped or ran with neither file, so the attribution keys and hooks were lost. `zygo` now passes only its own file, does not use such a value, and prints one line that names it; the replace line is gone. A user `disableAllHooks` still wins (stated next to the attribution keys, which always win). A workspace member with `--settings` stays a member (W AU59). |
| 2026-11-23 | §8.5 Flags (merge rule, merge line, non-boolean `disableAllHooks`, escaping of the unreadable-value line) | author's call (FIX-23, from the FIX-21 merge review, 2026-11-22); controller decisions 2026-11-23 (non-boolean value, status line, escaping through `esc`, split of the value-reading rules to FIX-26, the type-only `disableAllHooks` line, the merge-rule exception) | With a user `--settings` value that sets `disableAllHooks: true`, the merged file stops every hook and the status line, `zygo`'s included, but `zygo` printed that its attribution keys and hooks stay in force. When the merged `disableAllHooks` is `true`, `zygo` now prints `zygo: your --settings is merged into zygo's settings file; zygo's attribution keys stay in force; your disableAllHooks stops every hook and the status line, zygo's included` in place of that line; the line for other settings is unchanged. A user `disableAllHooks` that is not `true` or `false` is removed before the merge and not used, with one line that names only its JSON type; the merge-rule sentence states this exception. The lines come merge line, `disableAllHooks` lines, unreadable-value lines. The unreadable-value line writes the value through `zygo_core::esc`, which now also escapes U+061C, so all twelve Bidi_Control characters are escaped. The workspaces-spec texts AU59 and the W§16 row for `crates/zygo-launch/src/claude_code.rs` are FIX-24's. |
| 2026-11-23 | §8.4 Preflight (reading the settings files `zygo` finds), §8.5 Subagents (reading an `--agents` file), §8.5 Flags (the unreadable-value lines, `serde_json` limit) | controller decisions 2026-11-23 (re-plan of the BLOCKED FIX-23: the value-or-file rule with no first-character rule, value text only for an entry, the Windows invalid-name rule with its three errors, parse errors as category and position, the `serde_json` limit sentence; second plan review: a value the user gives keeps today's read semantics, the §8.4 files `zygo` finds skip anything that is not a regular file, the residual risks stated); author's call (FIX-26: the order of the checks, the line texts, one read of an `--agents` file per launch) | A `--settings` value that is not a JSON object was printed whole when `zygo` could not read it, so broken inline settings put secrets on stderr; the line gave no reason; the §8.4 settings files that `zygo` finds by itself were read with a blocking open, so a FIFO planted at one of their paths stopped the launch and a device was read; a `<(…)` pipe given as `--print --agents` was read twice, so its agents were dropped with no line. Changes: a value that parses as a JSON object is inline and any other value is a path, never judged by its first character; the value text is printed only when an entry exists at the path, with the reason; with no entry, `NotFound` (and on Windows `ERROR_INVALID_NAME`, `ERROR_BAD_PATHNAME`, `ERROR_FILENAME_EXCED_RANGE`) gives `zygo: your --settings value (not shown) is not a JSON object and names no file; it was not used` and any other error gives `zygo: your --settings value (not shown) is not a JSON object and could not be opened as a file: <io error>; it was not used`; a file that does not parse gives `<syntax|data|eof|io> error at line L column C`, never `serde_json`'s text; known limit: a `$serde_json::private::Number` or `$serde_json::private::RawValue` first key is read by `serde_json`'s rule. A `--settings` or `--print --agents` value the user gives is read as before (one blocking open that follows links; pipes, FIFOs and devices read; a FIFO with no writer waits). The `--print` `--agents` file is read once per launch; its refusal line and exit 2 are unchanged. The §8.4 settings files that `zygo` finds by itself, and the §12.3 passthrough check of the managed file, skip with no line and do not open a target that is not a regular file; a regular file is opened with `O_NONBLOCK` on unix and once more without it when that open would block, checked again on the handle and read whole; they still skip a file they cannot use with no line; their residual risks are stated in §8.4, including (review round 1, controller decision F2) that on Windows the type check cannot tell a named pipe or a device from a regular file, so such a target found in a repository is opened and read and a pipe server that does not write makes the preflight wait. The workspaces-spec text AU59 is FIX-24's. |
| 2026-11-23 | §17.1 (note on an `overrides.json` that cannot be read) | author's call (M4 review round 2; error text and one read, M4 review round 3) | It fixes a base-spec defect; it is not a workspace feature. The setting verbs read the old `overrides.json` with a plain blocking open, so a FIFO at that path made the verb wait with no end, and a link made it read and then write foreign JSON. The read now uses the session-file rule for all sessions, member or not: a link, a FIFO or a non-regular `overrides.json` stops the setting verb with exit 1 and one line, `zygo <verb>: cannot read <full path>: <reason>`, with no PATCH and no save. The file is opened once and the new overrides are built from those bytes. |
| 2026-11-23 | §2 | author's call (node D1) | W§15 §2: goal G11 Workspaces (W-G1 to W-G6). |
| 2026-11-23 | §4 C1 | author's call (node D1) | W§15 §4 C1: workspaces need tmux 3.2; on native Windows every workspace verb exits 2 (OP5; `crates/zygo/src/cmd/mod.rs:80-88`). |
| 2026-11-23 | §5 | author's call (node D1) | W§15 §5: the product terms of W§5; "Session" stays; the W§5 terms Builder and Brief name the design document's own reader and source, not product terms, so they are not added. |
| 2026-11-23 | §6.1 (layers 2, 3, 7) | author's call (node D1) | W§15 §6.1: the team layer drops `[loadout]` and `[workspace]` with one warning (AU22; `crates/zygo-config/src/layers.rs:70-90`); user config sets them (W§11; defaults from `crates/zygo-config/src/schema.rs:685-697`); member overrides are saved and applied at resume (OP12, AU105). Layers 4 and 5 unchanged, as W§15 says. |
| 2026-11-23 | §7.1 | author's call (node D1) | W§15 §7.1: no daemon stays; tmux holds the workspace processes (AU1). |
| 2026-11-23 | §7.2 | author's call (node D1) | W§15 §7.2: the status reply gains `requests_in_flight` and `last_request_end` (AU91, OP13; `crates/zygo-proxy/src/client.rs:85-90`). |
| 2026-11-23 | §8.1 | author's call (node D1) | W§15 §8.1: capabilities WA-1 Resume and WA-2 Turn signals (AU28), with the cells of `docs/conformance.md` (all partial). |
| 2026-11-23 | §8.2 (steps 1, 2, 5, 6) | author's call (node D1) | W§15 §8.2: membership before the launch, the session-id pre-assign and the resume argument; the session callback and the resume note in the view (`crates/zygo-launch/src/launcher.rs:427-442`, `:586`); exit classification; the `results` link keeps the session directory until the sweep. |
| 2026-11-23 | §8.4 (denylist, Set) | author's call (node D1) | W§15 §8.4: the four Claude Code session markers in the denylist (AU64); `ZYGO_WORKSPACE` and `ZYGO_PANE` for members, removed for every other launch (AU58; `crates/zygo-launch/src/env.rs:23-35`). |
| 2026-11-23 | §8.5 (settings file hooks, Flags) | author's call (node D1) | W§15 §8.5: the member hooks of W§9.1, `StopFailure` included (AU57; `crates/zygo-launch/src/claude_code.rs:478-521`); `degraded: CLAUDE_CODE_SIMPLE` (AU26; `claude_code.rs:40-51`, `:646-647`). |
| 2026-11-23 | §8.6 | author's call (node D1) | W§15 §8.6: the resume note as item 8; once on Claude Code, on every main-role request on OpenCode while its inputs are in the view, as the code does (`crates/zygo-hooks/src/workspace.rs:611-628`); W§15 row corrected. |
| 2026-11-23 | §8.7 | author's call (node D1) | W§15 §8.7: the member calls of the generated plugin, the tool count included, as the code does (`crates/zygo-launch/assets/opencode-plugin.js:152-178`); W§15 row corrected. |
| 2026-11-23 | §10.1 | author's call (node D1) | W§15 §10.1: the member `results` link (AU16), the sweep of linked session directories (AU69), no sweep for `zygo ws` (AU71; `crates/zygo/src/cmd/mod.rs:547-552`). |
| 2026-11-23 | §10.2.4 | author's call (node D1) | W§15 §10.2.4: a member's compaction override survives `down`/`up` and crashes (OP12, AU32, AU105; `crates/zygo/src/cmd/compact.rs:100-109`). |
| 2026-11-23 | §12.3 | author's call (node D1) | W§15 §12.3: `CLAUDE_CODE_SIMPLE` on is refused with `--passthrough`, as `--bare` is (`crates/zygo-launch/src/claude_code.rs:20`, `:674-676`). |
| 2026-11-23 | §15.4 | author's call (node D1) | W§15 §15.4: `[features] workspace_tool`, default `false`; a profile cannot set it (AU65; `crates/zygo-config/src/features.rs:1-27`). |
| 2026-11-23 | §17 (`zygo doctor` and `zygo update` rows, workspace rows) | author's call (node D1) | W§15 §17: the rows of W§12 and the doctor tmux line (`crates/zygo/src/cmd/doctor.rs:212-217`); auto-update design "Where" and "When the check is skipped": the `zygo up` check, the empty-value rule and the `ws run` relaunch (`crates/zygo/src/cmd/up.rs:37`, `update.rs:302-313`, `ws.rs:125-141`). |
| 2026-11-23 | §17.1 | author's call (node D1) | W§15 §17.1: verbs `down [--now]`, `ls`, `workspace` (OP11; `crates/zygo-core/src/verbs.rs:34-41`); setting verbs and `compact` also write the member record (AU105). |
| 2026-11-23 | §18.1 | author's call (node D1) | W§15 §18: the operator's calls OP1-OP13 of W§6.2. |
| 2026-11-23 | §18.2 | author's call (node D1) | W§15 §7.1, §18.2 A2 and §18: A2 keeps no daemon and the workspace state outlives sessions (AU1); AU1-AU105 are cited from W§6.2 with the withdrawn ids, not copied, so each decision has one record. |
| 2026-11-23 | §18.3 | author's call (node D1) | W§15 §18: the alternatives of W§14, by name, with their reasons in W§14. |
| 2026-11-23 | §19 | author's call (node D1) | W§15 §19: PW1-PW17 with their results from `docs/premises.md`; PW8, PW12, PW13, PW14, PW17 withdrawn. |
| 2026-11-23 | §20 | author's call (node D1) | W§15 §20: the checks of W§18, items #1 to #91, as item 21b. |
| 2026-11-23 | §13.4 (attribution row, known limits) | author's call (node FIX-32) | whole-branch review B4: the guard read a message file from disk although the same line wrote it first, so a trailer written by a heredoc passed and a stale file refused a clean commit; it now searches the writing command and the heredocs of every earlier command. |
| 2026-11-24 | §17, §17.1 | decision records design D§13 item 5 (approved 2026-11-24; node DR-3) | The `zygo decision` verb and its slash form, which reads no record (D-C2). |
| 2026-11-24 | §6.1 layer 3 | operator (workspace definition design R§9.4, R§9.5: user config holds the pool and the maximum); author's call for the team layer (R§9.4); node WD-5 | `[workspace] pool` and `[limits] max_members` are user config keys. |
| 2026-11-24 | §6.1 layer 4 | operator (workspace definition design R§9.4: a repository file cannot exceed the user maximum); author's call for the filter; node WD-5 | `.zygo.toml` cannot set `limits.max_members`; its four tables do not change. |
| 2026-11-24 | §13.5 | operator (workspace definition design R§9.4); author's call for the filter; node WD-5 | The repository file cannot set `limits.max_members`. |
| 2026-11-24 | §17 (`zygo status` row) | operator (session run time design T§2.5, T§3.3, approved 2026-11-24); node RT-2 | Inside a session, `zygo status` prints the `runtime:` line right after `session:`. |
| 2026-11-25 | §13.6 | decision records design D§13 item 4 (approved 2026-11-24; node DR-7) | `zygo work report` prints the `decisions:` line in each unit block and the `decision records:` line after the units (D§9.5); the slash form reads no record (D-C2). |
| 2026-11-25 | §13.3, §13.4, §13.5 | decision records design D§13 items 1, 2, 3 (approved 2026-11-24; node DR-13) | `work_verdict` takes `head`, `base` and `dispositions`; the `dispositions` guard (default `enforce`) checks a keep in the MCP tool; `dispositions` is a `[guards]` key. |
| 2026-11-25 | §17 | author's call | zygo ls rows end with the workspace unread count (ML-14). |
| 2026-11-25 | §11.1 | operator (workspace definition design R§8, R§14 row §11.1: the source); author's call (node WD-13) for the text | Profiles also come from the repository's approved `.zygo/profiles/`, with `overrides` and `disabled` only, added after the layers with the source `workspace-files`. |
| 2026-11-25 | §11.3 | workspace definition design R§8, R§14 row §11.3; author's call (node WD-13) for the text | A repository profile gets the same rules after the R§6.3 shape rules at read; the launcher reads only for a name not in user or team config, with the not-approved and invalid lines; passthrough applies no profile. |
| 2026-11-25 | §11.3 | author's call (node WD-13) | a named profile that does not apply is reported, as R§9.5 does for up; the not-read line of the launcher's read is listed with the other two; the not-found line is written in full. |
| 2026-11-26 | §11.1 | author's call (node WD-14) | The precedence sentence moves to §11.2 (WD-13 review L6). |
| 2026-11-26 | §11.2 | workspace definition design R§7.4, R§8, R§14 row §11.2; author's call (node WD-14) for the text | Order of sources for one name: user, team preset, repository; a member resolves a repository profile from its pin. |
| 2026-11-26 | §12.1 | operator (workspace definition design R§9.5, R§14 row §12) | A pool member starts with --passthrough; §12.3 and §12.4 do not change. |
| 2026-11-26 | §15.7 | workspace definition design R§14 row §15.7; author's call (node WD-15) for the text | approvals.toml also holds [[workspace]] entries. |
| 2026-11-26 | §17 | workspace definition design R§14 row §17; author's call (node WD-15) for the text | Row zygo workspace approve. |
| 2026-11-26 | §6.1 | workspace definition design R§14 row §6.1; author's call (node WD-16) for the text | config show names the source workspace-files and the file for the keys the repository files add. |
