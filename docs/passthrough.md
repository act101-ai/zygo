# Passthrough

`zygo claude --passthrough` runs Claude Code on your claude.ai subscription login and model choice. Redaction, the result store, the soft context window, cache handling, hooks, MCP tools, plugins, work units and the session text still apply (spec §12.1).

## Risk and local mode only

The vendor's consumer terms restrict routing subscription credentials through third-party software. Your own account carries that risk.

Passthrough works in local mode only. The login goes from Claude Code to the in-process proxy on loopback, and from there to the vendor. It never passes through a shared server. Serve mode refuses a passthrough session with `403 zygo_passthrough_local_only`.

## Credential type

| Type | Agent | Upstream host | Forwarded credential (allowlist) |
|---|---|---|---|
| `claude-ai` | Claude Code | `api.anthropic.com` | `Authorization: Bearer sk-ant-oat…` only |

The proxy reads this row; it holds no other values for the type.

## Proxy behaviour

- **Identity.** On `/anthropic/*`, when the `X-Zygo-Token` header is present, it carries the session token, with `X-Zygo-Session`. The session must be a passthrough session. `Authorization` is not read as identity. An invalid or empty token gives `401 zygo_passthrough_token_invalid` "relaunch zygo claude --passthrough".
- **Login check.** The request must carry exactly one `Authorization` header and no `x-api-key`. Its value is `Bearer`, one space and one token that starts with `sk-ant-oat`, with more after the prefix and no space or comma. Anything else (no login, `x-api-key`, a `zyg_` token, any other bearer) gives `401 zygo_passthrough_no_login` "passthrough needs a claude.ai login (/login)". Nothing is sent upstream.
- **Model.** No role resolution. The model is sent as received, `[1m]` included. The first catalog model with a native Anthropic provider whose `upstream_model` equals the name (without `[1m]`) supplies the model facts. The role is recorded as `subscription`.
- **Upstream.** One candidate: the matched model's provider on `api.anthropic.com` port 443, else a built-in provider for that host with no key. The login is sent only to `api.anthropic.com` on port 443; the same host on another port does not get it. No provider key is read; a provider with `enabled = false` is still dropped, and `require_zdr` still applies.
- **Headers.** The client's `Authorization` and `anthropic-beta` are forwarded unchanged. No `x-api-key` is added. Every `x-zygo-*` and `x-claude-code-*` header is removed.
- **Usage.** The usage row records the tokens, `role = subscription` and a cost estimate of 0. The login is never logged or stored.

## Limits

- Claude Code reads the session token (in `ANTHROPIC_CUSTOM_HEADERS`) once at start. The session has no idle expiry: its token stays valid while its launcher runs.
- A `/model` switch mid-session does not move the lead guard.
- `GET /anthropic/v1/models` still lists the role names. Role names do not work in passthrough: use a vendor model id.

## Several logins on one machine

Each Claude Code config directory holds its own claude.ai login. `--config-dir` sets `CLAUDE_CONFIG_DIR` for the launch, so one machine can run several subscription logins side by side:

```
zygo claude --passthrough --config-dir ~/.claude-interns/<name>
```

The directory must exist. Run `/login` once in each directory. The flag wins over an inherited `CLAUDE_CONFIG_DIR`.

## A pool of logins for workspace members

A pool is a folder in user config whose subdirectories are Claude Code config directories, each with its own login. Workspace members that take a directory from the pool run in passthrough, each on the login of its directory.

```toml
[workspace]
pool = "~/.claude-interns"   # user config only; `~` expands as for --config-dir; must be absolute after that
```

Each entry of the pool folder whose name does not start with `.` and that is a directory (links followed) is one config directory. Run `/login` once in each, for example with `zygo claude --passthrough --config-dir ~/.claude-interns/<name>`.

- **Which members.** A `claude-code` pane with `pool = true`, in `.zygo/workspace.toml` ([workspace-files.md](workspace-files.md)) or in a user loadout. Defining the pool is your consent to passthrough for these members; there is no other key. A member without `pool = true` is a normal zygo member with role routing. With no pool configured, `pool = true` has no effect. OpenCode has no pool: `pool` on an `opencode` pane is invalid, and `zygo config check` also refuses `pool` together with `config_dir`.
- **Assignment.** At a fresh build (`zygo up` of a new workspace, or `zygo up --reset`), the pool members in name order (members with no name after them, in pane order) take the directories in name order, in turn: with directories `alice` and `bob`, members `ana`, `ben` and `cy` get `alice`, `bob`, `alice`. Each member saves its directory and passthrough, so a resume or a restore keeps them, even after the pool changes or is removed. The next `zygo up --reset` assigns again.
- **Shown to you.** `zygo up` prints, once per fresh build: `zygo: ana, ben run in passthrough on pool /home/me/.claude-interns; profile rust of ana does not apply in passthrough` (a member with no name is `pane <n>`). `zygo workspace approve` shows the same text in its summary, or the refusal of a pool that `zygo up` would refuse because its path cannot be expanded or `[server] url` is set, ending `; zygo up starts nothing`. A pool that you set after the approval is not in that summary; the pool members still run in passthrough on it.
- **Profiles.** A profile, from any source, does not apply in passthrough: passthrough sends no role names. `zygo claude --passthrough --profile <n>` prints `zygo: profile <n> does not apply in passthrough`.
- **Local mode only.** With `[server] url` set, `zygo up` refuses a plan with a pool member.
- **Refusals.** When the plan has a pool member and the pool does not work, `zygo up` exits 1 and starts nothing: `zygo: pool <dir>: <reason>; nothing started`. When the `pool` path cannot be expanded, `<reason>` is `must be absolute after ~ expansion` or `no home directory`, and `<dir>` is the `pool` text as your config writes it (for example `zygo: pool rel/pool: must be absolute after ~ expansion; nothing started`). Else `<dir>` is the expanded path, and `<reason>` is `passthrough is local mode only, and [server] url is set`, `not a directory`, `no subdirectory`, `not readable (<kind>)` or `a directory name is not UTF-8`. A plan with no pool member does not read the pool.
- **`zygo doctor`.** `pool: <dir>: <k> directories`, or `pool: <dir>: <reason>`. With fewer directories than members, one line per shared directory for the loadout that `zygo up` would choose, for example `pool: /home/me/.claude-interns/alice is shared by ana, ben`.
- **What zygo reads.** zygo reads the names and types of the pool's entries only, and sets `CLAUDE_CONFIG_DIR` for the member. As for any config directory, the launch check reads its `settings.json`: a credential key or a provider switch there stops that member's launch.
