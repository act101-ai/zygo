# zygo: workspace mail — design

**Status:** draft for the operator's approval
**Date:** 2026-11-22
**Release:** after phase 2 (needs the workspace of `docs/specs/2026-11-20-workspaces-design.md`)
**Base specs:** `docs/specs/2026-11-18-zygo-design.md` (`§n`) and `docs/specs/2026-11-20-workspaces-design.md` (`W§n`, decisions `OPn`/`AUn`). `M§n` is a section of this document.

Each decision has a mark. **(operator)**: the operator made the decision. **(author)**: the author made it; the operator can change it before approval. Code references are `file:line` at commit `2d34269`.

---

## 1. Intent

Phase 2 leaves messages between agents out of scope (W§3: "Messages between agents, and wake of one agent by another"). Elephant-goldfish teams still need them. A lead and its workers in one workspace today send messages with an external script that:

1. writes report files to a folder in the system temp directory, which a reboot deletes;
2. types each message into the target pane with `tmux send-keys` to a tmux pane id (`%N`), which changes at each restore (W§5, "Pane key");
3. cuts the start of a long typed message, so long messages go to a file plus a short typed pointer;
4. makes the lead watch the folder with a bounded poll that it must start again after each timeout.

The operator asked for a durable mailbox in zygo **(operator, 2026-11-22)**. Workspace mail gives each member a durable inbox in the workspace state, addressed by a stable name, delivered by the hooks that phase 2 already registers, with no daemon.

## 2. Goals

- **M-G1** A message survives `zygo down`, `zygo up`, a reboot and a crash of the tmux server.
- **M-G2** A member sends to another member by name. No sender knows a tmux pane id.
- **M-G3** A busy member reads new mail before its turn ends. An idle member is woken.
- **M-G4** No size limit from the terminal: a message body never goes through `send-keys`.
- **M-G5** A watcher (a lead agent under a bounded tool, or a person) blocks until mail arrives, with no poll in the model.
- **M-G6** No daemon (W-G6). tmux stays the only long-lived process.

## 3. Non-goals

- Mail between workspaces, machines or OS users.
- Network delivery, read receipts, threads, priorities, or an order other than the ULID order.
- A workflow (a fixed lead/worker protocol, merge rules, review steps). Mail is a primitive.
- Triggers and automatic actions on mail, other than the wake of M§7.
- Native Windows (OP5).

## 4. Constraints

- **M-C1** W-C1 to W-C9 apply. C6 applies to each new hook branch: a mail error never breaks the agent.
- **M-C2** Mail is content. It is an exception to W-C7: it lives only under `mail/` (M§5), never in `workspace.json`, never in `activity.json`, a log or a usage row. C3 is not changed: the proxy stores no body; mail is written by the mail tool and the CLI, not by the proxy.
- **M-C3** Every write is atomic (W-C4): write in `tmp/`, then rename into `new/`.
- **M-C4** Only the OS user that owns the workspace state can read or write mail (dir `0700`, files `0600`, as AU2).
- **M-C5** No mail text reaches a tmux argument (W-C8). The wake text is fixed (M§7).
- **M-C6** A cloned repository cannot send mail or name a member (W-C5): names come from user config only.

## 5. Store

**(author)** One maildir per member under the workspace state directory (W§7.1):

```
~/.local/state/zygo/workspaces/<id>/mail/<pane key>/tmp/<ULID>.json
                                                    /new/<ULID>.json
                                                    /cur/<ULID>.json
```

- The folder is keyed by the pane key (W§5), which stays the same after a restore. The name (M§6) maps to the pane key.
- A message file (version 1): `{ "version": 1, "id": <ULID>, "from": <name or "cli">, "to": <name>, "sent_at": <RFC 3339>, "subject": <text, at most 200 bytes>, "body": <text, at most 1 MiB> }`. Fields that a reader does not know are kept (AU78).
- Send: write `tmp/<ULID>.json` (`0600`), `fsync`, rename to `new/<ULID>.json`. A send to `all` writes one file per member, sender excluded.
- Read: rename `new/<ULID>.json` to `cur/<ULID>.json`, then return it. A rename that fails because the file is gone means another reader took it: skip it.
- Retention: the sweep (AU69) deletes `cur/` files older than `[mail] keep = "30d"`. `zygo ws forget` deletes `mail/`. `zygo ws forget-pane <key>` deletes `mail/<key>/`.
- Limits: a send that would put more than `[mail] max_unread = 500` files in the target's `new/` is refused with `zygo: <name> has 500 unread messages`.

## 6. Names

**(author)** A loadout pane gets an optional `name` (W§11 schema addition): `^[a-z][a-z0-9-]{0,31}$`, unique in the loadout. `up` copies it into the `panes[]` entry and the member record. A hand-started member (AU9) takes `--name <n>` on `zygo claude` / `zygo opencode`, else it has no name and gets mail by pane key only. `all` and `cli` are reserved. Names are checked at each read, as AU84.

A rename by `zygo ws name <key> <name>` changes the name; mail stays in the pane-key folder.

## 7. Delivery

### 7.1 Busy member: the Stop hook

**(author)** The `Stop` hook of a member (W§9.1) reads the count of `mail/<own key>/new/`. When it is not zero and the input's `stop_hook_active` is `false` (`fixtures/claude-code/hooks/Stop-main.json:11`), it prints `{"decision":"block","reason":"zygo: <n> new messages; read them with mail_read"}`, so the agent reads the mail before its turn ends. With `stop_hook_active = true` it prints nothing (no loop; Claude Code also caps consecutive Stop blocks at 8, `fixtures/claude-code/settings-schema.json:1247-1249`). `busy` is cleared as in AU43 only when the hook does not block.

### 7.2 New turn: UserPromptSubmit and SessionStart

**(author)** When `new/` is not empty, the member's `UserPromptSubmit` hook adds one line of context, `zygo: <n> new messages; read them with mail_read`, and the main-loop `SessionStart` adds the same line after the resume note (W§9.4). Neither hook puts mail text into the context.

### 7.3 Idle member: the wake

**(author)** After a send, the sender's process wakes each target member that is idle by the idle rule of AU26 and whose launcher is live (AU74):

1. Read `@zygo-pane` of the target pane and check that it equals the target pane key, and `@zygo-ws` of the session (AU63). A mismatch: no wake.
2. `send-keys -t <pane id> -l "zygo: you have mail; read it with mail_read"`, then `send-keys -t <pane id> Enter` (two calls; the text is fixed, M-C5).
3. Record `woken_at` for the pane in `mail/<key>/wake.json` under the mail lock. A second wake within `[mail] wake_gap = "30s"` is skipped: the first one already queued a turn.

A busy member is not woken; M§7.1 delivers to it. A member whose launcher is not live is not woken; M§7.2 delivers at its next start. OpenCode: the wake is the same; the Stop branch uses the plugin's idle event (AU27, AU104) when PW4 passes.

## 8. Agent tools and CLI

**(author)** MCP tools on the zygo MCP server, added in `crates/zygo-mcp/src/tools/` and gated by `[features] mail` (default on in a workspace member, off otherwise), registered in `build_with` (`crates/zygo-mcp/src/tools/mod.rs:14-35`):

| Tool | Input | Effect |
|---|---|---|
| `mail_send` | `to` (name, pane key or `all`), `subject`, `body` | M§5 send, then M§7.3 wake. Returns the ids. |
| `mail_read` | `max` (default 10) | M§5 read, oldest first. Returns the messages. |
| `mail_list` | — | Unread count and the subjects of `new/`, without a read. |

CLI (W§12 additions):

| Command | Does |
|---|---|
| `zygo mail send <to> [--subject <s>] [--file <path> \| -]` | Send as `cli` (or as the member when `ZYGO_PANE` is set). |
| `zygo mail read [--name <n>] [--max <k>]` | Read a member's mail (default: the member of this pane). |
| `zygo mail ls [<name>]` | Unread counts per member, or subjects for one. |
| `zygo mail watch [<name>] [--timeout <dur>]` | Block until a file lands in `new/`, print one line per message (`<id> <from> <subject>`), exit 0; exit 1 at timeout (default 25m). For a lead under a bounded tool (for example a Monitor tool). It does not move the file. |

Slash forms: `/zygo:mail` (list) only, per O14; send and read stay tools, because their text is content.

## 9. Interplay with phase 2

- `down` does not wait for unread mail (it is durable). During `stopping`, `mail_send` returns `zygo: workspace is stopping` (AU86).
- The resume note (W§9.4) adds `<n> unread messages; read them with mail_read.` when `new/` is not empty.
- A detached member (W§7.5) keeps its mail; `up` lists its unread count with the detached line.
- `zygo workspace` and `zygo ls` show unread counts per member.
- A wake never counts as a `UserPromptSubmit` for AU26 until the agent's turn starts.

## 10. Failure handling

| Case | Behavior |
|---|---|
| Target name unknown | `mail_send` and `zygo mail send` exit 1 / return an error naming the known names. |
| Write fails partway | The file stays in `tmp/`; the sweep deletes `tmp/` files older than 1 h. |
| tmux unreachable at wake | No wake; the message stays in `new/`; M§7.1-7.2 deliver later. One `errors.log` line. |
| Hook cannot read `mail/` | C6: no block, no context line, one stderr line. |
| Pane reused by another program | M§7.3 step 1 refuses the wake. |

## 11. Configuration

User config only (AU10, AU22):

```toml
[mail]
keep = "30d"
max_unread = 500
wake_gap = "30s"

[[loadout.team.window.pane]]
kind = "claude-code"
name = "elena"          # M§6
```

## 12. Alternatives rejected

- **A supervisor daemon that owns messages and wakes** (an earlier zygo draft): rejected by W-G6 and A2; the hooks and the sender's own process cover delivery.
- **Typing the message into the pane** (today's script): cuts long text, races with the agent's input, and breaks on restore.
- **Mail inside `workspace.json`**: breaks W-C7 and makes every snapshot write carry content.
- **A poll in the model**: the lead goes idle when a task outruns its poll, and nothing wakes it (the deadlock that motivated phase 2's drain design).

## 13. Acceptance

1. A message sent before `zygo down` is read after `zygo up` and after a reboot (tmux server killed, `up --all`).
2. A send by name reaches the right member after a restore that changed every `%N`.
3. A 1 MiB body arrives byte-equal.
4. A member busy in a turn reads mail sent during the turn before the turn ends (Stop block, once).
5. An idle member is woken within 2 s of a send; a second send within 30 s does not type again.
6. A wake to a pane whose `@zygo-pane` does not match is refused.
7. `zygo mail watch` returns within 1 s of a send and exits 1 at its timeout.
8. No mail text appears in `workspace.json`, `activity.json`, any log or the usage store.
9. A hook with an unreadable `mail/` exits 0 and changes no decision.

## 14. Premises the first build verifies

- **PM1** A Stop block with `stop_hook_active = false` makes Claude Code 2.1.328 run one more turn that can call an MCP tool.
- **PM2** `send-keys -l` text plus a separate `Enter` submits a prompt in an idle Claude Code and in an idle OpenCode.
- **PM3** `UserPromptSubmit` and `SessionStart` context lines reach the model in both agents.
