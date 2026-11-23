# zygo

zygo is one native binary that wraps Claude Code and OpenCode: it routes each agent role to a chosen model through a local proxy, saves tokens, redacts secrets from requests, tracks work units, and delivers plugins from approved marketplaces, with act-lite and other token-saving tools built in.

Version 0.1.0.

---

## Agentic coding like you've never experienced before

### A status line that tells you want you want to know

<img width="1072" height="93" alt="image" src="https://github.com/user-attachments/assets/6365701c-305f-418b-a85d-6f00fb35b464" />

## Role-based model routing

- Follows [elephants-and-goldfish](https://research.google/pubs/elephants-goldfish-and-the-new-golden-age-of-software-engineering/) model for improved generation and token efficiency.

- Create your own model profiles to use any model natively for a chosen role

### AST-based Code Navigation

- Anti-pattern: Full file reads to find one method pollutes the context window with irrelevant code
- Anti-pattern: Random file chunk reads pollute the context window less, but repeated misses cause repeated LLM submissions, wasting tokens

* Solution: Navigate your codebase and documentation following the structural tree.

### Tool Results Store

- Anti-pattern: Agent context gets polluted with long tool results (eg build output)
- Anti-pattern: Agent runs tools with `| tail` and misses key results requiring re-running

* Solution: Save the full results from tool calls and provide recall and navigation tools

### Cache Annotation

- Anti-pattern: Coding agent doesn't mark all segments for caching resulting in higher costs

- Solution: zygo ensures that all context segments are marked for caching, resulting in higher cache hit rates and lower token costs.

### Inline DLP

- Anti-pattern: Agent reads secrets into context but doesn't realize until inference time, when it's too late

* Solution: DLP always redacts secrets from LLM calls

### Soft Context Management

- Anti-pattern: Auto-compacting in the middle of tasks resulting in information loss and poor results
- Anti-pattern: Context quality degradation as context window grows resulting in poor results

* Solution: Lifecycle hooks allow auto-compacting between tasks in a long-running plan

### Skills for Autonomous Delivery

- Plan and build software using a graph methodology for autonomous agentic execution.

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
