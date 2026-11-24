# zygo

zygo is one native binary that wraps Claude Code and OpenCode (and other coding agents to come): it routes each agent role to a chosen model through a local proxy, decreases token waste, redacts secrets from requests, tracks work units, and delivers plugins from approved marketplaces. Code navigation and the delivery skills come from the act-lite plugin, which zygo delivers from its built-in `act101` marketplace and which is on by default.

---

## A status line that tells you what you want to know

<img width="1091" height="140" alt="image" src="https://github.com/user-attachments/assets/c8d3dc64-27cd-45c2-9f2d-4e641cfcb0dc" />

The status line is for Claude Code; in OpenCode, run `zygo status`.

### Role-based model routing

- Implements [elephants-and-goldfish](https://research.google/pubs/elephants-goldfish-and-the-new-golden-age-of-software-engineering/) model for improved generation and token efficiency in your coding agent.
- Execute tasks with the most capability-appropriate model for the task.
- Create your own model profiles to use any model natively for a chosen role

### AST-based Code Navigation

- Anti-pattern: Full file reads to find one method pollutes the context window with irrelevant code
- Anti-pattern: Random file chunk reads pollute the context window less, but repeated misses cause repeated LLM submissions, wasting tokens
* Solution: Navigate your codebase and documentation following the structural tree.
* Navigation is provided by the act-lite plugin (on by default). It needs the `act-lite` binary on `PATH`; the zygo installer runs act-lite's installer when it finds no navigation binary ([docs/install.md](docs/install.md#code-navigation)).

### Tool Results Store

- Anti-pattern: Agent context gets polluted with long tool results (eg build output)
- Anti-pattern: Agent runs tools with `| tail` and misses key results requiring re-running
* Solution: Save the full results from tool calls and provide recall and navigation tools

### Cache Annotation

- Anti-pattern: Coding agent doesn't mark all segments for caching resulting in higher inference costs.
- Anti-pattern: Coding agent sets cache expiry too short resulting in cache misses and higher costs.
* Solution: zygo pins each model in a session to the first provider that serves it, so the provider-side prompt cache stays on one provider.
* Solution: When a request carries no cache markers of its own, zygo marks the system prompt (last system block) and the tool definitions (last tool) with a 1-hour cache. This is on by default for OpenCode and off for Claude Code, which places its own markers (`[cache] insert_markers.<agent>`).

### Inline DLP

- Anti-pattern: Agent reads secrets into context but doesn't realize until inference time, when it's too late
- Anti-pattern: A file containing secrets is read by a tool and exposed to the inference endpoint.
* Solution: The proxy redacts secrets from each request before it goes to a provider. It matches built-in patterns and, in local mode, the exact values in the act-lite secret store.
* The secret pass is on by default and fails closed: a request it cannot clear in time is refused. No profile, repository file or session command can turn it off; only `[features] redaction.secrets` in user or team config can, and a team that sets it on keeps it on.

### Soft Context Management

- Anti-pattern: Auto-compacting in the middle of tasks resulting in information loss and poor results
- Anti-pattern: Context quality degradation as context window grows resulting in poor results
* Solution: At a soft context window set per model, zygo makes the agent compact at the end of a turn, not in the middle of a tool loop. The model also gets a `compact` tool to call between tasks in a long-running plan
* This works in full on Claude Code. On OpenCode, zygo sets the context limit at launch only (`zygo opencode --compact <tokens|off>`).

### Skills for Autonomous Delivery
- Anti-pattern: Coding agent fails to deliver software in cohesive, testable units.
- Anti-pattern: Coding agent drifts from plan/design across long-running sessions.
* Solution: Plan and build software using a proven graph methodology for autonomous agentic execution.
* These skills are delivered by the act-lite plugin (on by default).

### Native Terminal Multiplexing — **COMING SOON** (v0.1.3)

- All coding agents in a workspace are automatically enlisted in a tmux session.
- Workspace loadouts save layout and agent configuration.
- Workspaces have shared mailboxes for inter-agent communication.
- Gracefully pause and restart your workspaces.

---

## Install

macOS and Linux:

```sh
curl -fsSL https://raw.githubusercontent.com/act101-ai/zygo/main/scripts/install.sh | sh
```

Windows (PowerShell; see [docs/install.md](docs/install.md#windows)):

```powershell
irm https://raw.githubusercontent.com/act101-ai/zygo/main/scripts/install.ps1 | iex
```

No administrator rights are needed. Options, environment variables and `zygo update`: [docs/install.md](docs/install.md).

Passthrough (`zygo claude --passthrough`) runs Claude Code on your claude.ai login, in local mode only. The vendor's consumer terms restrict routing subscription credentials through third-party software; your own account carries that risk. See [docs/passthrough.md](docs/passthrough.md).

Build: `cargo build --release`

License: MIT
