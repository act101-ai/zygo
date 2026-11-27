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
* `zygo up` starts the workspace of this repository and attaches to it (`--loadout <name>`; with no flag, the user `[workspace] default_loadout`, else the approved workspace files, else `solo`; `--reset` to start from the loadout and not from the snapshot, `--no-attach`, `--all` to restore every workspace that was up).
* `zygo down` waits until each agent has ended its turn and its subagents, saves the snapshot, then stops the agents (`--now` to skip the wait, `--timeout <n>s|<n>m|<n>h`, default 10m, `--wait`, `--all`). A drain that times out stops nothing: the workspace is up again, and `zygo down` names each busy pane and exits 1. From inside Claude Code, run `/zygo:down`.
* After `zygo down`, a reboot or a crash of the tmux server, `zygo up` brings back the layout and resumes each agent in its own conversation, with its launch flags, its session settings and its compaction window. Shell panes come back as a shell in their folder; zygo never runs a saved shell command again. Prompt text and inline flag values (for example inline JSON for `--mcp-config`) are not restored; `zygo up` names each flag that it did not restore.
* `zygo ls` lists the workspaces, `zygo attach` attaches to a running one, `zygo workspace` shows the members of this one, `zygo ws forget <name>` deletes a workspace that is not up, and `zygo ws forget-pane <key>` deletes one pane's record.
* Loadouts are `[loadout.<name>]` tables in the user config (`~/.config/zygo/config.toml`), and the `repository` loadout that approved workspace files add (see [Workspace files](#workspace-files-zygo)); a repository's `.zygo.toml` cannot set them. The built-in `solo` loadout is one Claude Code pane. Panes are `claude-code`, `opencode` or `shell`. An agent pane can also set `name`, `job` (`orchestrator` or `worker`) and `pool = true` (`claude-code` only, not with `config_dir`; see [docs/passthrough.md](docs/passthrough.md#a-pool-of-logins-for-workspace-members)). `zygo loadout list` and `zygo loadout show <name>` show them.
* Workspaces need tmux 3.2 or newer (`zygo doctor` shows the version). On Windows, run zygo in WSL.

### Workspace files (`.zygo/`)

A repository can define its workspace members and model profiles in files, so each person and each machine starts the same workspace.

* `.zygo/workspace.toml` defines the members. Each `.zygo/profiles/<name>.toml` defines one model profile.
* zygo reads these files from the default branch (the target of `origin/HEAD`, else `main`), from git objects only. An edit in the working tree, on a work branch or in a worktree has no effect.
* Nothing in them applies on a machine until you approve them there with `zygo workspace approve`.

`.zygo/workspace.toml`:

```toml
version = 1

[members.lead]
job = "orchestrator"

[members.ana]
profile = "rust"
pool = true

[members.ben]
pool = true
cwd = "branch:docs"
```

`.zygo/profiles/rust.toml`:

```toml
overrides = { review = "claude-opus-5-5", code = "claude-sonnet-5-5" }
disabled = ["design"]
```

| Key | Value |
|---|---|
| `version` | Required, `1`. |
| `layout` | `even-horizontal`, `even-vertical`, `main-horizontal`, `main-vertical` or `tiled`. Default: `main-vertical` when there is an orchestrator, else `tiled`. |
| `[members.<name>]` | One table per member. The name is 1-32 characters of `a-z`, `0-9` and `-`, starts with a letter, and is not `all`, `cli` or `zygo`. At most 32 members. |
| `kind` | `claude-code` (default) or `opencode`. |
| `job` | `orchestrator` or `worker` (default). At most one orchestrator. |
| `profile` | A profile name: 1-32 characters of `a-z`, `0-9` and `-`, not `none`. |
| `pool` | `true`: the member takes a login from your pool and runs in passthrough ([docs/passthrough.md](docs/passthrough.md#a-pool-of-logins-for-workspace-members)). `claude-code` only. With no pool configured, no effect. |
| `cwd` | `branch:<name>` (the worktree of that branch), or a path relative to the repository. Printable ASCII only; no absolute path, `~`, `\`, `.` or `..`. |

A profile file sets `overrides` (a role to a model id of your catalog, at most 16) and `disabled` (roles hidden from the lead, at most 16); at least one of them. Its name is the file name without `.toml`.

Any other key makes the files invalid as a whole, and nothing from them applies: a repository file cannot set a provider, a key, an endpoint, `args`, `config_dir`, `yolo`, `passthrough`, `server`, a prompt or a shell pane. `.zygo/workspace.toml` can be at most 64 KiB, each profile file at most 16 KiB, and `.zygo/profiles/` can hold at most 32 profile files.

With no `[members]`, the members are `orchestrator` and `worker-1` to `worker-4`, each `claude-code` with `pool = true`. The orchestrator is the first pane; the other members follow in name order. `[limits] max_members` in user or team config (1-32, default 8) caps the agents that the files start; `.zygo.toml` cannot set it. Above it, `zygo up` starts nothing: `zygo: this repository's workspace files start <n> agents; [limits] max_members is <m>; nothing started`.

**Approve.** `zygo workspace approve` shows the folder, the ref, the commit, the digest, each file line by line (each character other than printable ASCII as `\u{…}`, a tab as `\t`), the entries of `.zygo/` that zygo does not read, and a summary, then asks `approve these workspace files? [y/N]`:

```
$ zygo workspace approve --yes
folder: /home/me/src/app
ref: refs/remotes/origin/main
commit: 120e99a97b01ea58dddd1852435219f65a84d9ba
digest: c63b23b7c352fb19fb2dca3a3ba408acb4f922c5820dae47ef4d905450a5dead
.zygo/workspace.toml (140 bytes):
    version = 1
    …
.zygo/profiles/rust.toml (93 bytes):
    overrides = { review = "claude-opus-5-5", code = "claude-sonnet-5-5" }
    disabled = ["design"]
ignored: none
summary:
  agents: 3
  member lead: kind claude-code, job orchestrator, profile -, pool false, cwd -
  member ana: kind claude-code, job worker, profile rust, pool true, cwd -
  member ben: kind claude-code, job worker, profile -, pool true, cwd "branch:docs"
  profile rust: overrides code = "claude-sonnet-5-5", review = "claude-opus-5-5"; disabled design
  ana, ben run in passthrough on pool /home/me/.claude-interns; profile rust of ana does not apply in passthrough
approved c63b23b7c352fb19fb2dca3a3ba408acb4f922c5820dae47ef4d905450a5dead
to start it: zygo up (a workspace that exists already: zygo down, then zygo up --reset)
```

* `--yes` approves with no question and prints the listing on stdout, for scripts. `--digest <hex>` approves only when the files still have that digest; else it prints `zygo: the workspace files changed: their digest is <digest>, not <hex>; nothing approved` and exits 1, so a script approves exactly what it read.
* The last summary line shows only when your user config sets `[workspace] pool` and a member takes a pool directory.
* Files that are approved already: `workspace files already approved (<digest>)`. Absent or invalid files: one line, exit 1, nothing approved. Any answer other than `y` or `yes`: `not approved; run zygo workspace approve`.
* It works in your own shell only, native Windows included. Inside an agent session it exits 1 with `zygo: approval is not available inside an agent session; run it in your own shell: zygo workspace approve`, and `/zygo:workspace approve` replies `run in your shell: zygo workspace approve`.
* The approval is for these exact bytes, in this repository, on this machine. It is a `[[workspace]]` entry (`id`, `digest`) in `approvals.toml` (`~/.local/share/zygo/`; on Windows `%LOCALAPPDATA%\zygo\`). A new commit that changes `.zygo/` needs a new approval; files that you approved before apply again with no question. To withdraw an approval, delete its entry.

**`zygo up`.** With no `--loadout`, `zygo up` uses the user `[workspace] default_loadout` when your user config sets it, else the `repository` loadout of approved workspace files, else `solo`. A user `[loadout.repository]` is used in place of the files' loadout. When the files are not approved, `zygo up` starts `solo` and prints `zygo: this repository's workspace files (refs/remotes/origin/main, digest <12>) are not approved on this machine; nothing from them was applied; see zygo workspace approve` (invalid files: `… are invalid (<why>); nothing from them was applied`). When you choose `repository` yourself (`--loadout repository`, or `default_loadout = "repository"`), the same cases exit 1 and start nothing.

Each member keeps the files it started with: a resume or a restore uses the same commit, even after the default branch changes. New files apply after `zygo down`, then `zygo up --reset`. When that commit is gone or no longer approved, the member does not start, and zygo prints `zygo: profile <n> of this member comes from the workspace files at commit <12> (digest <12>), which this repository no longer has or no longer approves; the agent was not started; to start from the current files: zygo down, then zygo up --reset`. A restore and `zygo up --all` read no workspace files.

**Profiles.** An approved repository profile is one more profile: choose it with `--profile <name>`, `zygo profile use <name>`, or a member's `profile`. A user profile or a team preset of the same name wins. When the files are not approved, `zygo claude --profile rust` prints `zygo: profile rust is in this repository's workspace files, which are not approved on this machine (digest <12>); launching without it; see zygo workspace approve` and starts without it.

**What zygo shows.** With the files above approved:

```
$ zygo profile list
  rust (workspace-files)
$ zygo loadout list
repository  workspace-files
solo  built-in
$ zygo config show --layer workspace-files
loadout.repository.window = [ … ]  # workspace-files .zygo/workspace.toml
profile.rust.disabled = ["design"]  # workspace-files .zygo/profiles/rust.toml
profile.rust.overrides.code = "claude-sonnet-5-5"  # workspace-files .zygo/profiles/rust.toml
profile.rust.overrides.review = "claude-opus-5-5"  # workspace-files .zygo/profiles/rust.toml
$ zygo doctor
…
workspace files: refs/remotes/origin/main 120e99a97b01 digest c63b23b7c352: approved
```

* Not approved: `zygo profile list`, `zygo config show` and `zygo loadout show repository` show nothing from the files and print `zygo: this repository's workspace files (refs/remotes/origin/main, digest <12>) are not approved on this machine; not shown; see zygo workspace approve` (`zygo loadout show repository` then exits 1). `zygo loadout list` shows `repository  workspace-files, not approved`; invalid files give `repository  workspace-files, invalid`; a user `[loadout.repository]` gives `repository  workspace-files, shadowed` after the `repository  user` row. A user profile of the same name gives `rust (workspace-files, shadowed)` after the `rust (user)` row.
* `zygo config check` checks the files, approved or not. A violation is one line, for example `warning: workspace files: .zygo/workspace.toml: members.a.args: not allowed in a repository file`, and the exit status does not change.
* `zygo doctor` prints `workspace files: <ref> <commit> digest <12>: approved` (or `not approved`), `invalid (<why>)`, `none` or `unread (<why>)`, and `workspace files: <n> agents; [limits] max_members is <m>` when the files start too many. It never fails because of them.

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

Passthrough (`zygo claude --passthrough`) runs Claude Code on your claude.ai login, in local mode only. The vendor's consumer terms restrict routing subscription credentials through third-party software; your own account carries that risk. See [docs/passthrough.md](docs/passthrough.md). Workspace members can each run on their own login from a pool of config directories: see [docs/passthrough.md](docs/passthrough.md#a-pool-of-logins-for-workspace-members).

Build: `cargo build --release`

License: MIT
