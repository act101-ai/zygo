# zygo: workspace mail — design

**Status:** approved by the operator 2026-11-22
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

- Mail between workspaces or OS users. Mail between machines is the mail sync design (`docs/specs/2026-11-24-mail-sync-design.md`, phase-3 items P3-2 to P3-4): a side branch of the repository's remote **(operator, 2026-11-22)**.
- Network delivery, read receipts, threads, priorities, or an order other than the ULID order.
- A workflow (a fixed lead/worker protocol, merge rules, review steps). Mail is a primitive.
- Triggers and automatic actions on mail, other than the wake of M§7.
- Native Windows (OP5).

## 4. Constraints

- **M-C1** W-C1 to W-C9 apply. C6 applies to each new hook branch: a mail error never breaks the agent.
- **M-C2** Mail is content. It is an exception to W-C7: it lives only under `mail/` (M§5), never in `workspace.json`, never in `activity.json`, a log or a usage row. C3 is not changed: the proxy stores no body; mail is written by the mail tool and the CLI, not by the proxy.
- **M-C3** Every write is atomic (W-C4): write and `fsync` in `tmp/`, then hard-link into `new/` (a link refuses to replace an existing name), `fsync` `new/`, then remove the `tmp/` name.
- **M-C4** Only the OS user that owns the workspace state can read or write mail (dir `0700`, files `0600`, as AU2). This holds for `mail/`. Synced mail also exists in the repository's object store and on the remote (mail sync design S§13 limit 3, S-C7).
- **M-C5** No mail text reaches a tmux argument (W-C8). The wake text is fixed (M§7).
- **M-C6** A cloned repository cannot send mail. Member names come from user config, or from approved repository files (workspace definition design R§7). The side branch delivers mail only after the user-config opt-in (mail sync design S-G6).

## 5. Store

**(author)** One maildir per member under the workspace state directory (W§7.1):

```
~/.local/state/zygo/workspaces/<id>/mail/<pane key>/tmp/<ULID>.json
                                                    /new/<ULID>.json
                                                    /cur/<ULID>.json
```

- The folder is keyed by the pane key (W§5), which stays the same after a restore. The name (M§6) maps to the pane key.
- A message file (version 1): `{ "version": 1, "id": <ULID>, "from": <name or "cli">, "to": <name>, "sent_at": <RFC 3339>, "subject": <text, at most 200 bytes>, "body": <text, at most 1 MiB> }`. Fields that a reader does not know are kept (AU78). With mail sync between machines (mail sync design S§6.2), `from` and `to` can hold a full address `<name>@<host>`: `to` is the address of the target and `from` the address of the sender; the file format and its version do not change. The file format and the ULID names are fixed so that phase 3 can carry the same files on a side branch with no change (`docs/roadmap/phase-3.md`, P3-2): a message file is never edited after its link into `new/`, and the move to `cur/` is local read state, not part of the message.
- Send: write `tmp/<ULID>.json` (`0600`) and `fsync` it, hard-link it to `new/<ULID>.json` (a link refuses to replace an existing name), `fsync` `new/`, then remove the `tmp/` name. A send to `all` writes one file per member, sender excluded.
- Read: move `new/<ULID>.json` to `cur/<ULID>.json` the same way (hard-link into `cur/`, then remove the `new/` name), then return it. A link that fails because the file is gone means another reader took it: skip it.
- Retention: the sweep (AU69) deletes `cur/` files older than `[mail] keep = "30d"`. A maildir exists only for a pane key that has a member record. Every path that removes a member record removes `mail/<key>/`: `zygo ws forget-pane <key>`, `zygo up --reset`, and every other path that drops a record. `zygo ws forget` deletes `mail/`. A delivery does not create a maildir for a key that has no member record. A delivery takes `mail/lock` shared, reads the member records under it, and holds it until its last link into `new/`; it delivers only to a key that has a record in that read, and `zygo mail send` and `mail_send` resolve `to` on that read. A removal of `mail/<key>/` takes `mail/lock` exclusive, reads the member records under it, and removes `mail/<key>/` only when the key has no record. A missing `mail/` needs no lock and is not made. Neither waits more than 2 s: a delivery that cannot take the lock is refused with `zygo: mail is busy; try again`; a removal that cannot take it leaves the maildir, prints one line, and the next removal (`forget-pane`, `up`, `repair`) removes it. So that such a maildir does not stay, the repair step of `up` and `down` in phase `up` or `down`, and `up` on a running session, also remove every maildir with no record, best effort: an error or a held lock is ignored, with no line, and leaves it to the next removal. `up` removes the maildirs of the records that it dropped after its `phase = up` write or after its undo write, never before, because the undo puts `plan.forget` records back. A read takes `mail/lock` shared for its listing of `new/` and its moves into `cur/`, so no removal runs between them; it waits at most 2 s and is then refused with `zygo: mail is busy; try again`, and a missing `mail/` needs no lock. A read never makes `mail/` or `mail/<key>/` (only `cur/`, when `new/` has a message) and follows no link at `mail/`, `mail/<key>/`, `new/` or `cur/`: a link there is refused.
- Limits: a send that would put more than `[mail] max_unread = 500` files in the target's `new/` is refused with `zygo: <name> has 500 unread messages`.

## 6. Names

**(author)** A loadout pane gets an optional `name` (W§11 schema addition): `^[a-z][a-z0-9-]{0,31}$`, unique in the loadout. `up` copies it into the `panes[]` entry and the member record. A hand-started member (AU9) takes `--name <n>` on `zygo claude` / `zygo opencode`, else it has no name and gets mail by pane key only. `all`, `cli`, `zygo` and every name of pane-key form `^k[a-z2-7]{8}$` (W§5) are reserved, so `mail_send`'s `to` (a name or a pane key) is never ambiguous. `zygo` is the sender of the mail sync notice for a refused message (mail sync design S§8.6); like `cli`, it has no inbox. Names are checked at each read, as AU84.

A rename by `zygo ws name <key> <name>` changes the name; mail stays in the pane-key folder.

**Addresses (mail sync design S§5, S§7.2).** An address is `<name>@<host>`: `<name>` is a member name as above, `<host>` is the name of one workspace on the side branch, form `^[a-z0-9][a-z0-9-]{0,31}$`. A `to` value is one of:

- `<name>` or a pane key: a member of this machine. A bare name never means a remote member.
- `<name>@<host>`: with a host of this workspace (mail sync design S§7.5), the same as `<name>`; with another host, a remote member (mail sync design S§8).
- `all`: the members of this machine, as M§5. Not sent to other hosts.

A send to an address whose name is reserved (`cli@<host>`, `zygo@<host>`) is refused: they have no inbox.

## 7. Delivery

### 7.1 Busy member: the Stop hook

**(author)** The `Stop` hook of a member with `[features] mail` on (W§9.1) reads the count of `mail/<own key>/new/`. When it is not zero, the input's `stop_hook_active` is `false` and the workspace phase is not `draining` or `stopping` (`fixtures/claude-code/hooks/Stop-main.json:11`), it prints `{"decision":"block","reason":"zygo: <n> new messages; read them with mail_read"}`, so the agent reads the mail before its turn ends. With `stop_hook_active = true` it prints nothing (no loop; Claude Code also caps consecutive Stop blocks at 8, `fixtures/claude-code/settings-schema.json:1247-1249`). While the phase is `draining` or `stopping` it does not block, so `down` does not wait for unread mail (M§9). `busy` is cleared as in AU43 only when the hook does not block. A member with `[features] mail` off has no mail tools: its `Stop` hook does not read `mail/` and does not block.

### 7.2 New turn: UserPromptSubmit and SessionStart

**(author)** For a member with `[features] mail` on, when `new/` is not empty, the member's `UserPromptSubmit` hook adds one line of context, `zygo: <n> new messages; read them with mail_read`, and the main-loop `SessionStart` adds the same line after the resume note (W§9.4). Neither hook puts mail text into the context. A member with `[features] mail` off gets no line, and its hooks do not read `mail/`.

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
| `mail_send` | `to` (name, pane key, `all`, or `<name>@<host>`, mail sync design S§7.2), `subject`, `body` | M§5 send, then M§7.3 wake; to a remote address: the S§8.6 send-time pass, the outbox write, then the sync child (mail sync design S§8.1). Returns the ids. |
| `mail_read` | `max` (default 10) | M§5 read, oldest first. Returns the messages. |
| `mail_list` | — | Unread count and the subjects of `new/`, without a read; `remote`: the addresses of the member cache (mail sync design S§10.4). |

CLI (W§12 additions):

| Command | Does |
|---|---|
| `zygo mail send <to> [--subject <s>] [--file <path> \| -]` | Send as `cli` (or as the member when `ZYGO_PANE` is set). |
| `zygo mail read [--name <n>] [--max <k>]` | Read a member's mail (default: the member of this pane). |
| `zygo mail ls [<name>]` | Unread counts per member, or subjects for one. |
| `zygo mail watch [<name>] [--timeout <dur>]` | Block until a file lands in `new/`, print one line per message (`<id> <from> <subject>`), exit 0; exit 1 at timeout (default 25m). For a lead under a bounded tool (for example a Monitor tool). It does not move the file. |

Slash forms: `/zygo:mail` (list) only, per O14; send and read stay tools, because their text is content.

`mail_list` also returns the current member names, so an agent can find a member that joined after its session started.

### 8a. Instructions to the agent

**(author)** An agent uses mail with no prompt from a person. Three places carry the instructions:

1. **Session text.** For a member with `[features] mail` on, the session text (§8.6) gains item 8, after the mechanics text, built at each main-loop `SessionStart` (so a resume gets the current names):
   > Workspace mail: you are `<name>` in workspace `<workspace name>`. Members: `<name>, <name>, …`. Send with mail_send (to a name, or `all`); read with mail_read; mail_list shows unread mail and the current members. zygo tells you when mail arrives: before your turn ends, at your next prompt, or with a wake line. Do not poll or wait for mail: end your turn, and you will be woken.

   A member with no name gets `you have no mail name; others reach you by pane <key>` in place of the first clause. The item is not appended to subagent prompts (unlike item 7): mail is between members, and a subagent reports to its member through its result.

   With mail sync on for the workspace (mail sync design S§11) and a member cache (`mail/sync/members.json`, S§7.3) that is not empty, item 8 gains one clause after the member list, before "Send with": `Other machines: <address>, <address>, …; mail them by the full address.` Each address is `<name>@<host>`, in host order, then names as stored, each address once. At most 64 addresses; when there are more, the last of the 64 is followed by `, and <n> more; mail_list lists all`, where `<n>` is the number of addresses left out. Sync off, a config that does not load, or an empty cache gives no clause. The clause holds addresses only (S§10.4).

   With a job (workspace definition design R§9.3), the member list shows each member's job in brackets, and a member with no job by its name alone: ``Members: `orchestrator (orchestrator), worker-1 (worker), elena` ``. Item 8 then ends with one sentence for the member's own job. The orchestrator: `Your job: orchestrator. Give work to the workers with mail_send, collect their results with mail_read, and check each result against its brief.` A worker: `Your job: worker. Take your work from <orchestrator> by mail, do it through your zygo-<role> subagents, and send the result to <orchestrator> with mail_send.`, where `<orchestrator>` is the orchestrator's name (its pane key when it has none), or `the member that gave you the work` with no orchestrator. A value other than `orchestrator` or `worker` counts as no job, in the member list and for the sentence. No job gives no sentence. With mail off there is no item 8, so no job sentence.
2. **Tool descriptions.** `mail_send`, `mail_read` and `mail_list` each end with the same rule: "Do not poll; zygo wakes you when mail arrives."
3. **Delivery lines** (M§7.1-7.3) name the tool to call: `read them with mail_read`.

The text holds names and tool names only, no message text (M-C2), and nothing about a context window (C7).

## 9. Interplay with phase 2

- `down` does not wait for unread mail (it is durable). During `stopping`, `mail_send` returns `zygo: workspace is stopping` (AU86).
- When `new/` is not empty, the main-loop `SessionStart` adds the M§7.2 line after the resume note (W§9.4). The resume note adds no second line.
- A detached member (W§7.5) keeps its mail; `up` lists its unread count with the detached line.
- `zygo workspace` and `zygo ls` show unread counts per member.
- A wake never counts as a `UserPromptSubmit` for AU26 until the agent's turn starts.

## 9a. Status line (HUD)

**(operator, 2026-11-22)** Two fixed segments join the HUD of `docs/specs/2026-11-21-statusline-hud-design.md` (`H§n`). Both are fixed segments: HUD OP4 (no segment list, no format string) does not change. Both follow the rules of the workspace phase segment (H AU25): shown only for a workspace member, read fresh on each call, never cached, hidden on any read error, no dependency of the hook on `zygo-workspace`.

| Segment | Text (Nerd / plain) | Shown when | Source | Color | Place | Width |
|---|---|---|---|---|---|---|
| Member name | `<name>` / `<name>` | the member has a name (M§6), and the name is not equal to the worktree segment's name (H AU21) | the member's `name`, written by the launcher into the session view's workspace marker: a new field `name: Option<String>` on `Member` (`crates/zygo-core/src/session_view.rs:103-106`, today `ws_id` and `pane` only), added with `#[serde(default)]` so older views still load | bold default, `1` | first group, right after the workspace phase; verbose row 1, own group before the work item | 32 cells (H AU19) |
| Unread mail | `✉ <n>` / `mail <n>` | `n` ≥ 1 | the store's unread count (M§5): the regular files named `<ULID>.json` (`^[0-9A-HJKMNP-TV-Z]{26}$` before `.json`) in `mail/<pane key>/new/`; a link, a folder or any other name is not counted, and a missing `new/` counts 0. This is the same number `zygo workspace` shows. The pane key from `view.workspace.member.pane` | yellow `33` | first group, after the member name | `99+` above 99 |

- Neither segment is dropped for width (as the phase segment, H AU25).
- The hook validates the pane key (`^k[a-z2-7]{8}$`, WS AU84) before it builds the path, reads the directory with the H§7.7 deadline, takes each entry's type from the directory read (no link is followed), and counts only the regular `<ULID>.json` files, with its own copy of the ULID check (no dependency of the hook on `zygo-workspace`); it never opens a message (M-C2). The status line reaches only the user (H-C4), so the count tells the agent nothing.
- The name passes `zygo_core::esc` (H-C6), although M§6 already limits it to `[a-z0-9-]`.
- Use: an idle pane whose unread count grows shows a failed wake (M§7.3).

HUD changes: two rows in the segment table (H§6.2), the compact group `[logo, workspace phase, member name, mail, work item, worktree]`, the verbose row-1 groups `[workspace phase], [member name, mail], [work item, worktree, ahead/behind], …` (H AU9), and `✉` U+2709 in the Nerd icon set with `mail` in the plain set (H§6.5).

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

Mail sync between machines adds `[mail] host`, `[mail] sync_gap` and the per-workspace table `[mail.workspace.<name>]` (keys `sync`, `remote`, `host`); their forms and defaults are in `docs/specs/2026-11-24-mail-sync-design.md` S§11.

## 12. Alternatives rejected

- **A supervisor daemon that owns messages and wakes** (an earlier zygo draft): rejected by W-G6 and A2; the hooks and the sender's own process cover delivery.
- **Mail in git for phase 2** (in `main`, or a side branch at once): one machine needs no sync, and a commit on `main` fills history and races between worktrees. The side branch is the phase-3 transport between machines (P3-2), on top of this local store.
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
10. The HUD shows `✉ 3` for a member with three unread messages, nothing at zero, and `99+` above 99; no message file is opened (M§9a).
11. The HUD shows the member name for a named member outside a worktree, and shows it once when it equals the worktree name.
12. With no prompt about mail, a member started from a loadout sends a message to another member by name when its task needs one, and ends its turn without a poll; the session text of a resumed member lists a member that joined while it was down (M§8a).

## 14. Premises the first build verifies

- **PM1** A Stop block with `stop_hook_active = false` makes Claude Code 2.1.328 run one more turn that can call an MCP tool.
- **PM2** `send-keys -l` text plus a separate `Enter` submits a prompt in an idle Claude Code and in an idle OpenCode.
- **PM3** `UserPromptSubmit` and `SessionStart` context lines reach the model in both agents.

## Change log

| Date | Section | Decided by | Reason |
|---|---|---|---|
| 2026-11-24 | M§6 | author's call | A name of pane-key form is reserved so `mail_send`'s `to` stays unambiguous (ML-1 review). |
| 2026-11-24 | M-C3, M§5 | author's call | link, not rename, so a delivery or read never replaces an existing message (ML-2 review) |
| 2026-11-24 | M§11 | mail sync design S§16 | Gains `host`, `sync_gap` and `[mail.workspace.<name>]` (S§11). |
| 2026-11-25 | M§6 | mail sync design S§16 | Gains the address grammar (S§7.2) and the reserved name `zygo`, the sender of the S§8.6 refused-message notice, which has no inbox. |
| 2026-11-25 | M§5 | mail sync design S§16 | `from` and `to` can hold a full address `<name>@<host>` (S§6.2); the file format and its version do not change. |
| 2026-11-25 | M§9a | author's call | The HUD count is the store's count (regular <ULID>.json files; a missing new/ is 0), so the HUD and zygo workspace agree (ML-14 review; FX-12). |
| 2026-11-25 | M§5 | author's call | Retention states the record/maildir invariant: a send from an older state read recreated mail/<key>/ after forget-pane, and up --reset dropped records but kept their maildirs (ML-14 review; FX-13). |
| 2026-11-25 | M§7.1 | author's call (ML-10 review) | The Stop block is for a member with `[features] mail` on, which has `mail_read`, and it does not block in `draining` or `stopping`, so `down` does not wait for unread mail (M§9). |
| 2026-11-25 | M§7.2 | author's call (ML-10 review) | The prompt and SessionStart lines are for a member with `[features] mail` on, which has `mail_read` (M§8, M§8a). |
| 2026-11-25 | M§9 | author's call (ML-10 review) | The resume-note bullet said the note adds a second unread line; SessionStart gives only the M§7.2 line, after the note. |
| 2026-11-25 | M§3, M-C4, M-C6 | mail sync design S§16 | Mail between machines is now the mail sync design; M-C4 holds for `mail/`, and synced mail is also in the object store and on the remote; M-C6 holds: the side branch delivers only after the user-config opt-in. |
| 2026-11-25 | M§8a | author's call (lead) | Mail sync design S§16 and S§10.4: item 8 gains the "Other machines" clause after the member list, at most 64 addresses (MS-20). |
| 2026-11-26 | M-C6, M§8a | author's call (lead) | Workspace definition design R§14 and R§9.3: member names also come from approved repository files (R§7); item 8 lists each member's job in brackets and ends with the member's job sentence, after the mail tail, so the S§10.4 clause keeps its place (WD-18). M-C6 keeps the S-G6 side-branch sentence from the mail sync design S§16 row. |
| 2026-11-26 | M§8 | mail sync design S§16 | mail_send takes addresses and runs the send-time pass; mail_list returns the remote addresses (S§7.2, S§8.6, S§10.4; MS-19). |
| 2026-11-26 | M§5 | author's call (lead) | Deliveries and maildir removals take mail/lock (shared/exclusive) and read the member records under it, so a send from an older state read, or one in flight, cannot make mail/<key>/ again after a record removal; up removes dropped maildirs after phase = up or its undo (FX-13). |
| 2026-11-26 | M§5 | author's call (lead) | A read takes mail/lock shared for its list and moves, so a removal cannot run between them, and follows no link at mail/, mail/<key>/, new/ or cur/ (FX-13 review F1, F2). |
| 2026-11-26 | M§5 | author's call (lead) | The repair step in phase up or down and up on a running session also remove maildirs with no record, best effort, so one left by a failed removal does not stay (FX-13 review F3). |
