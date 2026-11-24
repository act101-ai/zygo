# zygo

zygo is one native binary that wraps Claude Code and OpenCode (and other coding agents to come): it routes each agent role to a chosen model through a local proxy, decreases token waste, redacts secrets from requests, tracks work units, and delivers plugins from approved marketplaces. Code navigation and the delivery skills come from the act-lite plugin, which zygo delivers from its built-in `act101` marketplace and which is on by default.

---

## A status line that tells you what you want to know

<img width="1091" height="140" alt="image" src="https://github.com/user-attachments/assets/c8d3dc64-27cd-45c2-9f2d-4e641cfcb0dc" />

The status line is for Claude Code; in OpenCode, run `zygo status`.

### Icons: install a Nerd Font

The status line uses [Nerd Font](https://www.nerdfonts.com/) icons. Without a Nerd Font, the icons show as boxes or are missing. The examples below install JetBrainsMono Nerd Font; for other fonts, see https://www.nerdfonts.com/font-downloads.

macOS (Homebrew):

```sh
brew install --cask font-jetbrains-mono-nerd-font
```

Linux:

```sh
mkdir -p ~/.local/share/fonts
cd ~/.local/share/fonts
curl -OL https://github.com/ryanoasis/nerd-fonts/releases/latest/download/JetBrainsMono.tar.xz
tar -xf JetBrainsMono.tar.xz
fc-cache -f
```

Windows (Scoop):

```sh
scoop bucket add nerd-fonts
scoop install JetBrainsMono-NF
```

Or download `JetBrainsMono.zip` from the font downloads page, extract it, select the `.ttf` files, right-click and choose Install.

Then select the font in your terminal's settings: Terminal.app or iTerm2 on macOS, GNOME Terminal or Konsole on Linux, the profile's Appearance > Font face in Windows Terminal. In the VS Code terminal, set `terminal.integrated.fontFamily`. For WSL, install the font on the Windows side and set it in Windows Terminal.

zygo selects the icon set from `ZYGO_NERD_FONT`:

- `ZYGO_NERD_FONT=1` forces Nerd icons.
- `ZYGO_NERD_FONT=0` forces plain icons.
- Unset: plain icons when `TERM` is `linux` or `dumb`, and on Windows; Nerd icons otherwise.

On Windows, including Git Bash, set `ZYGO_NERD_FONT=1` after you install the font. To check the result, run `zygo doctor`; it prints an icon test line.

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

### Native Terminal Multiplexing (v0.2.1)

- All coding agents in a workspace are automatically enlisted in a tmux session.
- Workspace loadouts set layout and agent configuration.
- Workspaces have shared mailboxes for inter-agent communication — **COMING SOON**.
- Gracefully pause and restart your workspaces.
* A workspace is one git repository: the main checkout and all its worktrees. It has one tmux session, `zygo-<name>`.
* `zygo up` starts the workspace of this repository and attaches to it (`--loadout <name>`, `--reset` to start from the loadout and not from the snapshot, `--no-attach`, `--all` to restore every workspace that was up).
* `zygo down` waits until each agent has ended its turn and its subagents, saves the snapshot, then stops the agents (`--now` to skip the wait, `--timeout <n>s|<n>m|<n>h`, default 10m, `--wait`, `--all`). A drain that times out stops nothing: the workspace is up again, and `zygo down` names each busy pane and exits 1. From inside Claude Code, run `/zygo:down`.
* After `zygo down`, a reboot or a crash of the tmux server, `zygo up` brings back the layout and resumes each agent in its own conversation, with its launch flags, its session settings and its compaction window. Shell panes come back as a shell in their folder; zygo never runs a saved shell command again. Prompt text and inline flag values (for example inline JSON for `--mcp-config`) are not restored; `zygo up` names each flag that it did not restore.
* `zygo ls` lists the workspaces, `zygo attach` attaches to a running one, `zygo workspace` shows the members of this one, `zygo ws forget <name>` deletes a workspace that is not up, and `zygo ws forget-pane <key>` deletes one pane's record.
* Loadouts are `[loadout.<name>]` tables in the user config (`~/.config/zygo/config.toml`) only; a repository's `.zygo.toml` cannot set them. The built-in `solo` loadout is one Claude Code pane. Panes are `claude-code`, `opencode` or `shell`. `zygo loadout list` and `zygo loadout show <name>` show them.
* Workspaces need tmux 3.2 or newer (`zygo doctor` shows the version). On Windows, run zygo in WSL.

### Session Commands

- Change a session's settings while it runs: compaction window (`compact`), result trimming (`results`), the content redaction pass (`redaction`), guard levels (`guards`), work units (`units`) and status line style (`statusline`). On every verb except `compact`, `--save` also writes the change to user config.
* Run `zygo help` for every verb with its current value and scope; `zygo help <verb>` for one verb. In Claude Code the same verbs are `/zygo:<verb>`; in a shell, `zygo <verb>`.
* In a workspace, a session setting is restored with that agent at the next `zygo up`.

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

Requirements: Claude Code 2.1.328 or newer, or OpenCode 1.18.16 or newer; tmux 3.2 or newer for workspaces. `zygo doctor` shows each installed version against its minimum.

Update check: `zygo claude`, `zygo opencode` and `zygo up` check for a new release at most once in 24 hours and ask `Update now? [y/N/s]` (`s` skips that version). The check runs only in a terminal, and never inside a zygo session or a workspace pane. It reads the release's public `VERSION` file and sends nothing else. To turn it off, set `[update] check = false` in user config or `ZYGO_NO_UPDATE_CHECK=1`.

Passthrough (`zygo claude --passthrough`) runs Claude Code on your claude.ai login, in local mode only. The vendor's consumer terms restrict routing subscription credentials through third-party software; your own account carries that risk. See [docs/passthrough.md](docs/passthrough.md).

Build: `cargo build --release`

License: MIT
