# zygo: mail sync between machines — design

**Status:** approved by the operator 2026-11-24
**Date:** 2026-11-24
**Release:** phase 3, track B (roadmap items P3-2, P3-3 and P3-4, `docs/roadmap/phase-3.md`)
**Base specs:** `docs/specs/2026-11-22-workspace-mail-design.md` (`M§n`; approved, not built yet), `docs/specs/2026-11-18-zygo-design.md` (`§n`), `docs/specs/2026-11-20-workspaces-design.md` (`W§n`, decisions `AUn`). The build follows `docs/specs/2026-11-24-decision-records-design.md`. `S§n` is a section of this document.

Each decision has a mark. **(operator)**: the operator made the decision (the roadmap's "P3-2 design notes", its item rows and its "Plan of work (operator, 2026-11-24)"). **(lead)**: the lead of this design work ruled it after an independent audit of the first draft (2026-11-24). **(author)**: the author made it; the operator can change it before approval. Code references are `file:line` at commit `8289868` (branch `phase-3`). Git references are to the manual pages of git 2.50 (`git help worktree`, `git help fetch`). P3-1 is not built. This spec extends the text of M§n, not code.

---

## 1. Intent

P3-1 gives each member of one workspace a durable inbox on one machine (M§1). Phase 3 lets the members of one repository work on more than one machine and coordinate through the repository's own remote, with no zygo server and no daemon (roadmap "Intent"; W-G6).

The operator's direction **(operator, 2026-11-22)**: data that must cross machines goes on a side branch of the repository. It never touches `main` or a work branch. zygo pushes it to the remote like any branch.

This spec covers three items:

- **P3-2** Carry M§5 message files between machines on a side branch.
- **P3-3** Give each member an address that is unique across machines, `<name>@<host>`, and keep a member registry on the side branch.
- **P3-4** Deliver and wake on the receiving machine after a fetch, with the M§7 rules.

## 2. Goals

- **S-G1** A member on machine A sends to a member on machine B by address. The message arrives in B's local maildir (M§5) with its bytes unchanged.
- **S-G2** No user branch, index, working tree, `HEAD` or `FETCH_HEAD` changes, and no git maintenance runs in the user's repository because of zygo. zygo writes only its own branch.
- **S-G3** No daemon (W-G6). A sync runs only when a send, a hook, a watcher or a person starts it.
- **S-G4** No message that the secret pass (§16.1) finds a secret in leaves the machine.
- **S-G5** No message is lost and none is delivered twice: not when two machines sync at the same time, not after a crash between two steps, not when a host is idle for months. Mail is lost only by an explicit operator command that states the count and asks first **(lead)**.
- **S-G6** Sync is off until the operator turns it on for one workspace on one machine, and names the host.

## 3. Non-goals

- A zygo server or relay; network delivery other than `git fetch` and `git push` **(operator)**.
- Mail in `main` or in git notes **(operator)**.
- A wake of a machine where no zygo process runs. S§10.3 states this limit.
- Proof of the sender. The `from` field is not authenticated (S-C7).
- `all` across machines. `all` stays the members of this machine (S§7.2).
- Native Windows (OP5).
- Moving work, worktrees, sessions or results between machines (roadmap "Not in phase 3").

## 4. Constraints

- **S-C1** M-C1 to M-C6 (M-C6 as amended in workspace definition design R§14) apply. C6 applies to each new hook branch: a sync error never changes a hook decision.
- **S-C2** Mail is content (M-C2). No subject, body, address, host name, member name or path that holds one goes into a log, a usage row, a commit message or a ref name **(lead)**. `workspace.json` holds none of them either, with one exception: the M§6 names of this workspace's own members, which the workspace record stores (`members.<key>.name`, `panes[].name`, W§7.2); no name of another machine's member goes there. The sync worktree (S§6.3) and its git index are the synced tree: by design they hold its paths, which name hosts and members (`<name>@<host>/`, `registry/<host>.json`, `ack/<host>.json`), and its message files; no subject or body goes into a path. A log line names ULIDs, pattern ids, check names, blob ids and counts only.
- **S-C3** zygo writes only `refs/heads/zygo/<x>` on the remote. A force push goes only to that branch, only with the lease of S§9 **(operator)**.
- **S-C4** Every git call is built by the hardened builder `git_command` (`crates/zygo-plugins/src/sync.rs:32-58`: no repository hooks, no fsmonitor, no LFS filter, no `ext::` transport, `GIT_TERMINAL_PROMPT=0`, `ssh -o BatchMode=yes`), plus `-c gc.auto=0 -c maintenance.auto=false -c fetch.writeCommitGraph=false` on each call **(lead)**. Each call runs under the runner of S§8.7, with a deadline.
- **S-C5** All content from the remote is untrusted: names, paths, modes, sizes, JSON and identities. zygo validates it before use, and never writes a file at a path that the remote chose (S§6.3).
- **S-C6** Only user config turns sync on (M§11, AU10, AU22). `.zygo.toml` and the team layer cannot.
- **S-C7** Trust boundary: each person with write access to the remote can send mail to, and wake, a member that syncs, and can delete or forge files on the side branch. Each person with read access can read all synced mail. The opt-in (S-G6) is the operator's consent to both.
- **S-C8** No machine deletes, or decides about, another host's files by its own clock or its own config. Another host's registry, acknowledgment and mail are removed only by that host's own acknowledgment (S§9) or by an explicit operator command (S§7.6) **(lead)**.
- **S-C9** Tests never use wall-clock timing (project rule) and never run a real `claude` or `opencode` (stub agents, `crates/zygo-launch/tests/stub_agent/`). The clock, the spawn of the sync child, the public probe, a hook between named steps and the pass deadline are test seams. The pass deadline seam is `sync::run_with_deadline`: it gives each pass another deadline of S§8.3 step 4, on both the `Instant` and the injected clock. Only the `test-hooks` feature builds it. Test builds and the operator's live-premise build (`cargo build -p zygo --features test-hooks` for `scripts/premises/live-hud.sh`) turn that feature on; a normal or release build never does. Production calls `sync::run_with`, with 120 s.

## 5. Terms

| Term | Meaning |
|---|---|
| Side branch | The branch named by `sync` (S§11), for example `zygo/mail`. It shares no history with any other branch and is never merged **(operator)**. |
| Host | The name of one workspace on the side branch. Form `^[a-z0-9][a-z0-9-]{0,31}$`. The operator sets it (S§7.1). |
| Address | `<name>@<host>`. `<name>` is the M§6 member name. |
| Local address | An address whose host is one of this workspace's hosts (S§7.5). It means the same member as the bare `<name>`. |
| Identity | 32 random lower-case hex characters, made once per workspace. It shows which workspace owns a registry or acknowledgment file. Not a secret. |
| Ack | An acknowledgment file, `ack/<host>.json`: the ULIDs that the host has imported (S§7.4). |
| Outbox | `mail/outbox/` in the workspace state: messages to remote addresses that wait for a push. |
| Imported log | `mail/sync/imported`: the local, append-only list of each ULID that this workspace has delivered (S§8.5). |
| Tip | The commit of the side branch that the last fetch got. |
| Pass | Steps 4 to 15 of S§8.3. A sync is one or more passes. |
| Sync child | A detached `zygo mail sync --ws-id <id> --background` process, started by a send or a hook. |

## 6. The side branch

### 6.1 Layout

**(operator)** The files are the M§5 message files, one per message, named by ULID, under `<to>/<ULID>.json`. Files are append-only and never edited. **(author)** The tree:

```
ZYGO-MAIL                        marker: the bytes "1\n"
registry/<host>.json             S§7.3; one file per host; its owner writes it
ack/<host>.json                  S§7.4; one file per host; its owner writes it
<name>@<host>/<ULID>.json        one message to that address
```

- `ZYGO-MAIL` marks the branch as zygo's. A pass refuses a tip that does not have it (S§8.3 step 7).
- A message file never changes after its first commit. No import deletes it **(lead)**. Only a prune removes it, and only after its host acknowledged it (S§9), or an explicit `--drop-host` (S§7.6).
- `registry/<host>.json` and `ack/<host>.json` are written only by the workspace whose identity is in them. Thus each path has one writer, and no commit needs a merge.
- Read state (`new/` to `cur/`) stays local **(operator)**. The ack is not read state: it says "imported", not "read".

### 6.2 Message file on the branch

The M§5 file, version 1, with no new field. Two values change form **(author)**: `to` is the full address of the target, and `from` is the full address of the sender (`<name>@<host>`). M§5 already keeps unknown fields (AU78). The importer writes the blob bytes into the local maildir unchanged (S-G1).

### 6.3 Worktree

**(operator)** zygo works on the side branch in its own detached worktree under the workspace cache directory, never in a user checkout. **(author)** The path is `$HOME/.cache/zygo/workspaces/<id>/mail-sync/` (W§7.1 cache root).

- **Make.** `git worktree add --force --detach --no-checkout <path> <commit>`, run in the main worktree (`main_root`, W§7.2). `--force` makes the worktree again when the user deleted its folder: `git help worktree` (`--force`): "add refuses to create a new worktree when … <path> is already assigned to some worktree but is missing … This option overrides these safeguards." zygo never runs `git worktree prune`, because it acts on every worktree of the repository **(lead)**.
- **Use.** Every later git call runs with the worktree as its current folder. zygo uses only the worktree's own index and `HEAD`: `read-tree`, `update-index --add --cacheinfo` and `--remove`, `write-tree`, `commit-tree`, `update-ref --no-deref HEAD`. It never checks out a file. Reason: a checkout writes paths that the remote chose; a link in the tree could redirect a later write (S-C5).
- **Per-worktree state.** `git help worktree` (REFS): "In general, all pseudo refs are per-worktree and all refs starting with refs/ are shared." Thus the worktree's `HEAD` and index are its own. `FETCH_HEAD` is a pseudo ref too, but zygo also fetches with `--no-write-fetch-head` (`git help fetch`), so no `FETCH_HEAD` is written in any worktree.
- **Remove.** Only at `zygo ws forget` (S§7.6, S§14), only by path, and never through a link. When `<cache>/<id>` or `<cache>/<id>/mail-sync` is a link, zygo removes only that link and runs no git, so git never acts on a worktree that the link names. When `mail-sync` is a real folder and the main worktree (`main_root`) is a folder, zygo runs `git worktree remove --force -- <path>` in the main worktree, under the S§8.7 runner with a 10 s deadline. What git does not remove (no main worktree folder, so no git; git fails or times out, for example on a folder that is not a working tree, whose admin folder is gone, or whose `main_root` is no repository; a regular file at `mail-sync`) zygo removes with the filesystem, as the sync's own recovery does (S§6.3 Make, `ensure_worktree`): that one path inside `<cache>/<id>/`, no link followed: right before that removal zygo reads `<cache>/<id>` and `mail-sync` again with no link followed, and a link put at either while git ran is removed as a link only. Only a failure of that filesystem removal refuses the forget (S§7.6). git then keeps an entry for the removed folder and lists it as `prunable`, as for a folder that the user deleted, and with no worktree folder zygo runs no git: the next `worktree add --force` at that path (Make) takes the entry over; the workspace id, and so the path, is the same for a new workspace of the same repository (W§5). A moved repository keeps a `prunable` entry for the old path. zygo never runs `git worktree prune`, here too.
- The worktree is listed by `git worktree list` in each checkout, detached. W§11 `branch:` resolution does not match it, because it has no branch.

### 6.4 Bootstrap (first use)

**(lead, author)** When the fetch finds no branch on the remote (step 6 of S§8.3: "couldn't find remote ref", matched without case):

1. In the main worktree: `git hash-object -w --stdin` with the bytes `1\n` gives the marker blob; `git mktree` with the one entry `100644 blob <blob>\tZYGO-MAIL` gives tree `T`; `git commit-tree T` (no parent) gives the root `R`.
2. If the worktree does not exist, make it at `R` (S§6.3). If it exists, `update-ref --no-deref HEAD R` in it.
3. The pass continues with `R` as the tip. Its commit (step 12) has `R` as parent; its push makes the branch.

Two machines that make the branch at the same time: the second push is rejected (the branch now exists and does not contain its `R`). Its next attempt fetches the new branch and builds on it; its own `R` is no longer used. A branch that a person deleted on the remote is made again the same way. The mail that was on it is gone (S-C7, S§13 limit 4).

### 6.5 Commits and refs

- `commit-tree` with `-c user.name=zygo -c user.email=zygo@zygo.invalid -c commit.gpgSign=false`. Message: `zygo mail [skip ci]` (S§6.6). It holds no count, address or host (S-C2).
- Fetch: `git fetch --no-write-fetch-head <remote> +refs/heads/<branch>:refs/remotes/<remote>/<branch>`. This is the ref that the user's own default fetch also writes. zygo makes no other ref. The fetched objects go into the repository's object store (S§13 limit 3).

### 6.6 Forge cost

**(lead)** Each pass that commits makes one push. A push can start CI and webhooks on the forge.

- The commit message holds `[skip ci]`. GitHub Actions skips `push` and `pull_request` workflow runs for a commit whose message holds `[skip ci]` (https://docs.github.com/en/actions/managing-workflow-runs-and-deployments/managing-workflow-runs/skipping-workflow-runs). GitLab skips the pipeline for a commit message with `[skip ci]`, in any case (https://docs.gitlab.com/ci/pipelines/, "Skip a pipeline"); GitLab still makes an empty pipeline with the status `skipped`.
- **Limit.** Webhooks fire for each push. A GitHub workflow on another event (for example `pull_request_target`), a GitLab policy that disables the skip, and a CI system that does not read the marker still run. The operator excludes `zygo/**` in such CI configs. `zygo mail sync` prints this once per workspace, at the end of its first sync that ends without a stop. The pass of that sync that finds no `again` mark under the lock (S§8.3 step 17) records the note as `ci_note_shown` in `mail/sync/state.json` (S§15), and the sync prints the note when it returns without a stop. When a mark comes after that look and a later pass of the same sync stops inside the lock, that pass sets `ci_note_shown` back to false under the lock, and the sync does not print the note. When a later pass of the same sync goes to another holder of the lock, the sync ends without a stop and prints the note that it recorded. If a later lock attempt of that sync fails with an error, the note recorded by the earlier pass is not printed and stays recorded; resetting it would need the lock (S§15). A sync whose first pass goes to another holder of the lock, and a sync child, print nothing of the note and do not record it, so a child sync before the first `zygo mail sync` does not use the note up.
- The rate limit (S§8.4) and one commit per pass bound the count of pushes.

## 7. Addresses, registry and acknowledgments (P3-3)

### 7.1 Host name

**(lead, author)** The host is `[mail.workspace.<name>] host`, else `[mail] host`. It has no default. Sync stays off until one is set: `zygo: set [mail] host to turn on mail sync; zygo does not publish the system host name` (config check error, and the sync line of `zygo mail ls`). Reason: the system host name can name a person or a place; the operator chooses what the branch shows.

### 7.2 Address grammar

**(author)** A `to` value is one of:

- `<name>`: a member of this machine (M§6). A bare name never means a remote member. Reason: a bare name that is local on one machine and remote on another would reach a member that the sender does not mean.
- `<name>@<host>`: with a local host, the same as `<name>`; with another host, a remote member (S§8).
- `all`: the members of this machine, as M§5. Not sent to other hosts.

`cli` and `zygo` are reserved names (M§6 reserves `cli` and `all`; this spec adds `zygo` for S§8.6). A send to `cli@<host>` or `zygo@<host>` is refused: they have no inbox.

**(lead)** At send (`mail_send`, `zygo mail send`), the local host is the host in force (S§7.1) when sync is on for the workspace; with sync off there is no local host. A send reads config and files only and runs no git (S§7.3), and an older own host (S§7.5, S§7.6 "Rename") is known only at the tip, so at send `<name>@<older host>` is a remote address, and the member cache rule below refuses it (the cache never holds a registry of this workspace's identity). A `to` value with no `@` follows M§8 and M§10. A `to` value with `@` that is not an address is refused with one line, the same for the tool and the CLI:

- a reserved name (`all@<host>`, `cli@<host>`, `zygo@<host>`): `zygo: all, cli and zygo have no address; send to <name> or <name>@<host>`;
- any other form: `zygo: not an address; send to a member name, a pane key, all, or <name>@<host>`.

A send to a remote address is refused:

- when the sender is a member with no name, or the CLI with no `ZYGO_PANE`: `zygo: remote mail needs a sender name; give this member a name in the loadout or with --name` **(lead)**;
- when sync is off: `zygo: mail sync is off for this workspace`;
- when the last public check said `public` (S§8.2);
- when the address is not in the member cache (S§7.3), with the known addresses (as M§10).

A member with no name has no address and gets no remote mail. The sender of `zygo mail send` is the member of `ZYGO_PANE`; with no `ZYGO_PANE` it is `cli`, which has no address (M§8). `zygo mail read --name <n>`, `zygo mail ls <n>` and `zygo mail watch <n>` take local members only: a value with no `@` follows M§8; `<name>@<local host>` (the local host as at send) works as `<name>`; any other value with `@` (a remote address, a reserved name, a bad form, or any address when sync is off or the config does not load) is refused with `zygo: <value> is not a local address; zygo mail read, ls and watch take a member of this machine` (exit 1). These verbs read config and files only and run no git.

### 7.3 Registry

**(author)** Each host has one file, `registry/<host>.json`:

```json
{ "version": 1, "host": "studio", "identity": "<32 hex>", "members": ["alice", "bob"] }
```

- **Owner.** Only the workspace whose identity is in the file writes it. A pass writes it when it is absent or when its member names differ from this workspace's names.
- **Content.** Member names only. No pane key, path, session id, user name or time.
- **Validation (S-C5).** A file is used only when: blob mode `100644`; size at most 64 KiB; valid JSON; `version` 1; `host` equals the file name and passes the host form; `identity` passes `^[0-9a-f]{32}$`; `members` has at most 256 names, each passes M§6. Any other file is skipped with one `errors.log` line that names the check and the blob id.
- **Member cache.** After each fetch, the pass writes `mail/sync/members.json` (atomic, `0600`): each valid registry other than this workspace's own, with its names. `mail_list`, the address check of S§7.2 and the session text (S§10.4) read this file. They never run git. No host is ever left out because of its age (S-C8).

### 7.4 Acknowledgment

**(lead, author)** Each host has one file, `ack/<host>.json`:

```json
{ "version": 1, "host": "studio", "identity": "<32 hex>", "imported": ["<ULID>", "…"] }
```

- **Owner.** The workspace whose identity is in it. The list holds ULIDs, while an entry on the branch is a path `<name>@<host>/<ULID>.json`, and two folders (two names, or an old and a new host) can hold one ULID. The ack is keyed by ULID by design. For each own host `<h>` (S§7.5), a pass writes the list (the ULIDs it lists ∪ each ULID that this pass acknowledged in the folders of any own host ∪ each ULID of the acks of all own hosts at the tip) ∩ the ULIDs of the `<ULID>.json` entries at the tip in the folders of `<h>` (`sync::own_changes`). The folders of a host are the top-level folders `<name>@<h>/` whose name is an address that `address::parse` accepts; the member name is not checked, so it need not be a member of this workspace (`sync::own_files`). Thus a ULID with no entry left in those folders (a prune removed it) goes out, and the list holds only ULIDs that are still on the branch under that host.
- **Meaning.** The list holds the ULIDs of the entries in that host's folders that this identity acknowledged: it imported them (delivered or refused, S§8.5), or found them already in the imported log. After a `--take-host` (S§7.6) it can also hold ULIDs that another identity acknowledged, because the command keeps the old owner's list. A ULID leaves the list when no entry with that ULID is left in that host's folders ("Owner"). The ack is the only input that lets a prune remove a message file (S§9 "Tree"). Two things stop a second delivery of a ULID. First, the own-ack filter: `import::candidates` skips each entry whose ULID is in the ack of any own host (the union of their lists) or that the pass already took (S§8.5 "Candidates"). Second, the imported log: `import::run` does not deliver a candidate whose ULID is in the log (`seen`); it only acknowledges it, or leaves it while its staged file is still in `mail/sync/incoming/`. Thus an identity delivers each ULID at most once, and an entry in a folder of an own host ("Owner": a folder whose name is an address that `address::parse` accepts) whose ULID this identity acknowledged in any own host can never be delivered, whatever its member name, sender or kind. An entry in a top-level folder whose name `address::parse` refuses is in no folder of a host: no pass reads it (S§7.6 "Rename", S§13 limit 2).
- **Validation.** As the registry: mode, size at most 8 MiB, `version` 1, `host` equals the file name, a valid identity, each entry a valid ULID, at most 200 000 entries. An invalid ack counts as empty: the prune removes nothing for that host.
- **Size.** When this workspace's own ack would pass 100 000 entries, the pass runs a prune after its push (S§9), which takes the acknowledged files and so the entries off.

### 7.5 Identity, collision and the own hosts

**(author)** `mail/sync/identity` holds the identity, made at the first sync (random, `0600`). It is per workspace, not per machine: two clones of one repository on one machine are two workspaces (W§5), and each needs its own host.

**Own hosts.** The current host, plus each host whose `ack/<host>.json` at the tip holds this identity (an older host name of this workspace, S§7.6 "Rename"). The pass imports from the address folders of all own hosts.

**Collision.** The current host's `registry/<host>.json` or `ack/<host>.json` at the tip holds another identity. The pass stops (exit 1 for the CLI, one `errors.log` line for a child) with:

`zygo: the mail host name is taken on the side branch by another identity. If that is another machine, set another [mail] host. If it is this workspace from before (a moved repository, a ws forget, a restored backup), run zygo mail sync --take-host.`

When a local workspace state (another `<id>` under `workspaces/`) holds that identity, the line also names that workspace: the repository moved (W§5: a new common dir gives a new workspace id), and the old state still exists.

**Twin.** Two workspaces with one identity: a state folder that was copied (a backup restored on a second machine, a home folder moved while the first machine still runs). The pass records in `mail/sync/state.json` the blob id of each own registry and ack file that it last pushed, and the blob id that it is about to push (before the push). It keeps these records for the own hosts only: the state write that ends a pass, and the state write after the push of `--take-host` or `--drop-host` (S§7.6), removes the path of each host that is not an own host at that tip, unless a file of this identity is still at that path there. A tip whose own file holds this identity and has a blob id that is neither of those two was written by another workspace with this identity. The pass stops with `zygo: another workspace writes with this mail identity (copied state?); run zygo mail sync --new-identity here, and set another [mail] host` and imports nothing. `zygo doctor` prints the same line while `state.json` `last_error` is `twin` (the check name of this stop) for the workspace of the session or folder (`ZYGO_WORKSPACE`, else the checkout of the current folder, as the mail verbs); a pass that ends without a stop clears `last_error`. `zygo doctor` runs no sync and no fetch for this line.

- **Limit.** A twin is found only at the first pass after the other copy writes one of these files. Until then, both copies can deliver the same message (S§13 limit 5).

### 7.6 Operator commands

Each command that can lose mail prints the count and asks; `--yes` answers in scripts (as §15.7) **(lead)**.

In a twin state (S§7.5) `--take-host` and `--drop-host` stop with the twin line and write nothing; the twin line's fix is `--new-identity`.

- **`zygo mail sync --take-host`.** For the collision line. It writes `registry/<host>.json` and `ack/<host>.json` with this identity. It keeps the ack's entries, so no message that the old owner acknowledged is imported again. No mail is lost; it does not ask. If the old owner still runs, its next pass stops with the collision line.
- **`zygo mail sync --new-identity`.** For a twin. It makes a new identity and clears the recorded blob ids. Then the pass runs as a first sync: with a new host (the old one belongs to the twin) it registers; with the same host it stops with the collision line. It deletes nothing.
- **Rename.** A changed `host` in config. The old registry file (this identity) is deleted at the next pass, so no new
  mail goes to the old addresses. The old `ack/<old>.json` stays, so the old host stays an own host: the pass still
  imports mail that waits under `*@<old>/`. When the new registry would pass 256 members, the pass does not write it and
  does not delete the old registry; while the old registry stays, the old ack stays too, so the mail that senders push
  to the old addresses is imported. The pass deletes `ack/<old>.json` when all of these hold: (a) no
  `*@<old>/<ULID>.json` entry at the tip is a message file (S§9 "Tree"): the folders never held one, or a prune has
  taken each one; (b) the list that this pass computes for `<old>` (S§7.4 "Owner") holds the ULID of every
  `*@<old>/<ULID>.json` entry at the tip (when the ack stays, the pass writes this list; when it deletes the ack, it
  writes no ack); (c) the pass deletes the old registry in the same commit, or the tip has no old registry of this
  identity. (a) and (b) look only at an entry with a canonical `<ULID>.json` name (`address::ulid_file`: 26 upper-case
  Crockford characters, then `.json`) directly in a top-level folder whose name is an address (`address::parse`: a
  member name that is not reserved, then `@`, then a host in the S§5 form), as every reader does: the own files
  (`sync::own_files`), import (`import::candidates`), the prune (`prune::run`) and `--drop-host` (`ops::host_folders`,
  `ops::waiting`). So an entry that (a) and (b) leave is of one of three kinds. (1) A link, a submodule or a tree in an
  address folder, at a `<ULID>.json` name whose ULID this identity acknowledged: it can never be delivered (S§7.4
  "Meaning") and no prune removes it. (2) An entry in an address folder with any other name, such as a regular file with
  another name or a `<ULID>.json` name in lower case: it is never delivered, never pruned and never counted by
  `--drop-host`. (3) An entry of any name or kind in a top-level folder `<x>@<old>/` whose name is not an address, such
  as a reserved name (`all@<old>/`) or a name with upper case (`Bob@<old>/`): it is never delivered, never pruned and
  never counted, and no `--take-host` or `--drop-host` touches it. No kind keeps the old ack, so no kind keeps the old
  host an own host. Kinds (1) and (2) stay on the branch until the operator removes them, or until a `--drop-host <old>`
  deletes the address folders of `<old>` with every entry in them. Kind (3) stays until the operator removes it with
  git: no mail-sync command removes it (S§13 limit 2). An entry in an address folder whose ULID no pass acknowledged (a
  full inbox, a name this workspace does not have) keeps the old ack. A sender whose member cache still lists
  `<name>@<old>` pushes new mail there while `ack/<old>.json` stays, and the pass imports it; after the ack is deleted,
  the sender's pass moves that mail to `mail/outbox/refused/` and sends it a notice (S§8.3 step 11). No mail is lost.
- **`zygo mail sync --drop-host <host>`.** For a host that will never sync again. The folders of the host
  (`ops::host_folders`) are the top-level trees of the tip whose name is an address of that host (`address::parse`: a
  member name that is not reserved, then `@`, then a host in the S§5 form). It counts the message files (S§9 "Tree")
  with a canonical `<ULID>.json` name directly in those folders whose ULID the valid `ack/<host>.json` does not list (a
  missing or invalid ack lists none); a link, a submodule, a tree or an entry with another name is not a message and is
  not counted (`ops::waiting`). It prints `zygo: <n> messages wait for host <host>; they will be deleted. Delete them?
  [y/N]`, and on yes deletes `registry/<host>.json`, `ack/<host>.json` and each of those folders with every entry in it,
  at any depth and of any name or kind (`ops::drop_host`), in one commit. A top-level folder `<x>@<host>/` whose name is
  not an address, such as `all@<host>/` or `Bob@<host>/`, is not a folder of the host: it is not counted and not
  deleted; only the operator can remove it, with git (S§13 limit 2). A dropped host that runs again registers again at
  its next pass; the deleted mail is gone. This is the only path by which a machine deletes another host's files without
  that host's ack (S-C8).
- **`zygo ws forget [--yes]`.** These steps come after the W§12 checks (`op.lock`, live launcher, session) and before anything is deleted, in this order:
  1. `forget` takes `mail/sync/lock` at once (no wait). When `mail/`, `mail/sync/` or the lock file is missing, `forget` makes it (folders `0700`, the file `0600`) and follows no link, so a sync that starts meanwhile waits for this lock and then finds the workspace gone (S§8.3 step 3). When `mail/` or `mail/sync/` is a link or not a folder, the workspace has no mail: `forget` takes no lock and makes nothing through the link. When the lock file is a link or not a regular file, no sync can hold it (a sync opens it with no link followed and refuses an entry that is not a regular file, with the stop `lock`), so `forget` takes no lock and goes on. `Taken::NotRegular` assumes a lock file does not become non-regular while it is held; no zygo path replaces a lock file, and only a same-uid actor can, which is outside the threat model (same-uid limit, M§5 R1-1) **(author)**. Any other error of the lock refuses with `zygo: cannot forget <name>: <error>` (exit 1, nothing deleted). A lock that a sync holds refuses with `zygo: a mail sync runs for <name>; try again` (exit 1, nothing deleted). `forget` keeps the lock until the state and the cache are gone, so no pass uses the worktree meanwhile.
  2. It counts the messages in `mail/outbox/` (`<n>`) and in `mail/outbox/refused/` (`<r>`). A `mail/` that is a link or not a folder is not followed and counts 0 for both (step 1: no mail). An outbox or refused folder that cannot be read refuses with `zygo: cannot forget <name>: the outbox cannot be read: <error>` (exit 1, nothing deleted): unsent mail is never taken for none. When either count is not 0, it asks on stderr, on one line, `zygo: <n> unsent messages and <r> refused messages will be deleted. Forget the workspace? [y/N]`; `--yes` answers yes and prints the question followed by ` y (--yes)`. Any answer but `y` or `yes`, and the end of input, prints `zygo: <name> not forgotten` and exits 0 with nothing deleted. An empty outbox with no refused message asks nothing.
  3. It removes the sync worktree (S§6.3): a link as a link, a folder with `git worktree remove --force`, and what git does not remove with the filesystem. Only a failed filesystem removal refuses, with `zygo: cannot forget <name>: the mail sync worktree <path> was not removed: <error>` (exit 1, nothing deleted).
  4. It takes `mail/lock` exclusive (M§5), waiting at most 2 s, and holds it until step 5 has renamed the state folder out of the workspace list. So while the folder goes, no delivery (M§5, S§8.5, S§8.6) is between its record read and its last link, and no remote send (S§8.1) is between its lock and its link into `mail/outbox/`. Under the lock, right before the rename, it counts `mail/outbox/` and `mail/outbox/refused/` again as in step 2 (`forget_recount`, FX-28). When either count is higher than the count of step 2, mail came in after that count: the forget refuses with `zygo: cannot forget <name>: the outbox changed after the count: <n> unsent messages and <r> refused messages now; try again` (exit 1). The state stays, the next sync makes again the sync worktree that step 3 removed (S§6.3), and a new `zygo ws forget` asks with the new count. The forget does not ask again and has no override, so this refusal has a cost: while members keep sending remote mail, each forget attempt can refuse at this count, after the question of step 2 and after step 3 removed the sync worktree, which the next sync makes again. The operator stops the senders, or tries again **(author)**. An outbox or refused folder that cannot be read at this count refuses as in step 2. So a remote send that returns `Ok` is in a count that the operator saw, or the forget deletes nothing **(author)**. This holds because every writer that can raise a count in this window holds `mail/lock` shared across its write: the remote send (`outbox::send_remote_with`, S§8.1). The pass writers of `mail/outbox/` and `mail/outbox/refused/` (`outbox::refuse`, `sync::refuse_unread`, the deletes of pushed files) run only under `mail/sync/lock`, which the forget holds from step 1, and the local sweep (S§9) only deletes. The forget does not ask again under the lock, because a question with no time bound must not hold `mail/lock`. It releases the lock before `op.lock`, the reverse of the lock order (`op.lock` < `mail/sync/lock` < `mail/lock` < the state lock). The rename moves the locked file with the state folder, so every taker of `mail/lock` (a delivery, a read, a removal of a maildir, a remote send) checks, right after it gets the lock, that the locked file is still the file at `<state>/mail/lock` (the same device and inode, the path read with no link followed). A taker that waited for the forget fails that check: it releases the lock and finds no workspace (a delivery or a send writes nothing, a read returns nothing, a removal removes nothing), whatever a `zygo up` of the same id wrote after the forget released `op.lock`. When `mail/` is missing it is made (`0700`, no link followed) as in step 1; when `mail/` is a link or not a folder the workspace has no mail (step 1) and no lock is taken. When `mail/lock` is a link or not a regular file, no delivery, read, removal or send can open it (each refuses to open such a file), so no lock is taken and the forget goes on with no lock; any other error of the lock refuses with `zygo: cannot forget <name>: <error>` (exit 1). This rests on the premise of step 1 (FX-24): a lock file is not replaced while it is held. A holder that opened the regular file before the same user replaced it with a link or a non-regular entry still holds a lock on the old file, so it is not stopped by this step and can still link into `mail/outbox/` after the count again; that message is then deleted with the state folder, in no count the operator saw. This is the same-user tampering limit that FX-24 records (no zygo path replaces a lock file; only a same-uid actor can, which is outside the threat model, M§5 R1-1) **(author)** (FX-28 review F4). A lock not taken in 2 s refuses with `zygo: cannot forget <name>: mail is busy; try again` (exit 1): the state stays, and the next sync makes again the sync worktree that step 3 removed (S§6.3). The lock comes after steps 2 and 3, so no question and no git call runs while deliveries and sends wait for it.
  5. W§12 as before: the state folder (with `mail/`, so the outbox and the imported log go with it), then the workspace cache.

  A state that cannot be read (W§12) gets the same steps; the main worktree is the `main_root` of its raw JSON when that is an absolute path, else none (no git). S§7.5 "Collision" covers the next sync of a new workspace for the same repository.

## 8. Sync (P3-2)

### 8.1 Entry points

| Entry | Runs | Rate limit |
|---|---|---|
| `mail_send` and `zygo mail send` to a remote address | the sync child, after the outbox write. The write holds `mail/lock` shared (M§5, as a delivery: `outbox::send_remote_with` through `Delivery::begin`) from before the first outbox folder is made until after the link into `mail/outbox/` (S§7.6 step 4, FX-28). Each failure of `Delivery::begin` refuses the send; `zygo mail send` prints the line on stderr and exits 1, and `mail_send` returns it as the tool error. A lock not taken in 2 s: `zygo: mail is busy; try again`. No `workspace.json`, no state folder, or a locked file that is no longer `mail/lock` (a forget ran while the send waited): `zygo: the workspace has no state; nothing sent`. A corrupt `workspace.json`: `zygo: mail: <state>/workspace.json: <reason>`. A newer `workspace.json`: `zygo: mail: workspace state is from a newer zygo; run zygo update`. A state folder that is a link: `zygo: mail: <state> is a link; it is not followed`. A state path that is not a folder: `zygo: mail: <state> is not a folder`. A `mail/` that is a link or not a folder: `zygo: mail: <state>/mail is not a folder`. A `mail/lock` that is not a regular file (an I/O error): `zygo: mail: <state>/mail/lock: is a link; it is not followed` for a link, and `zygo: mail: <state>/mail/lock: is not a regular file` for a folder, a FIFO or another entry. The sync state and the member cache are in the state folder, so a state folder that is missing or not a folder when the send reads them refuses before `Delivery::begin`; the two lines of those cases above come when the state goes after those reads. Both entries read the state before the send, so a state that is missing or does not read at that read refuses first, with that entry's line: `zygo mail send` refuses in `workspace_of` with `zygo: no workspace with id <id>` (`ZYGO_WORKSPACE` set; the line of `zygo ws` and `zygo down`) or `zygo: no workspace for this session or folder` (no `ZYGO_WORKSPACE`); `/zygo:mail` gives the same lines, so the CLI and slash replies stay the same (N42) (FX-28 step 10), and `mail_send` with `zygo: workspace <id> has no state`. A refusal makes no state folder, no `mail/outbox/` folder and no outbox file, and starts no child. When the state folder is a real folder and `mail/` is not a link, the refusal can leave `mail/` (`0700`) and `mail/lock` (`0600`), which `Delivery::begin` makes when they are missing, before it reads the state. | none (the `again` mark merges bursts, S§8.3) |
| Member hook events `session` (main loop), `prompt`, `stop`, `tool-end` (W§9.1, `crates/zygo-hooks/src/workspace.rs:36-37`) | the sync child | `[mail] sync_gap` (S§8.4) |
| `zygo mail watch` with sync on | the sync child, asked for at the start of the watch and before each look at `new/` under the S§8.4 rate limit (`mail/sync/last_start`, `sync_gap`), until it returns: a watch that starts within `sync_gap` of another start (a hook, another watch) starts no child, as that child already syncs. The watch never waits for the child, so no sync delays a look or the `--timeout`; mail that the child imports shows at a later look. A child that does not start, or a `last_start` that cannot be written, gives one `errors.log` line. A config that does not load, or sync off, starts no child | `[mail] sync_gap` (S§8.4) |
| `zygo mail sync [--name <workspace>] [--prune] [--take-host] [--new-identity] [--drop-host <host>] [--yes]` | a sync in its own process, with output | none |

The hook starts the child as it starts the snapshot child today (`start_snapshot_child`, `crates/zygo-hooks/src/workspace.rs:484-513`): stdin, stdout and stderr to the null device, cwd `/`, a new session (`setsid`), no wait. A child that does not start gives one `errors.log` line. The hook prints nothing more and changes no decision (S-C1).

Reason for `tool-end` **(author)**: on a machine where one member is in a turn of one hour, no `stop` or `prompt` comes for an hour. Without `tool-end`, the idle members of that machine get no remote mail for that hour. The cost is one read of a timestamp per tool call.

### 8.2 Public check

**(operator)** zygo refuses sync on a remote that it can see is public. **(author)** "Can see" means: an anonymous read of the repository succeeds.

1. Get the fetch URL and every push URL of the remote (`git remote get-url` and `git remote get-url --push --all`: each `pushurl`, or each `url` when the remote has no `pushurl`, which is the set that `git push` sends to). Check each distinct URL.
2. Map the URL to an HTTPS URL: `https://host/path` as is, without user info; `ssh://[user@]host[:port]/path` and the scp form `[user@]host:path` to `https://host/path`. A local path or `file://` URL is `local`. Any other scheme is `unknown`.
3. Run `git -c credential.helper= -c core.askPass= ls-remote --heads <https url>` with no credentials **(lead)**: `GIT_CONFIG_GLOBAL` and `GIT_CONFIG_SYSTEM` set to the null device, `GIT_CONFIG_NOSYSTEM=1`, `HOME` set to an empty temporary folder (no `.netrc`), `GIT_ASKPASS` and `SSH_ASKPASS` removed, `GIT_TERMINAL_PROMPT=0`, deadline 10 s. Proxy variables in the environment stay. The runner keeps the whole stderr up to 64 KiB (S§8.7).
4. Verdict: exit 0 is `public`. A failure whose stderr holds one of the builder's list of "authentication needed" or "repository not found" messages, matched without case, is `private`. Any other failure (network, TLS, timeout) is `unknown`.

- `public` for any URL: the pass stops with `zygo: the remote can be read without a login; mail sync refused` (exit 1). Nothing is fetched or pushed. A remote send is refused (S§7.2).
- `private` or `local`: the pass continues.
- `unknown`: the pass continues, and `zygo mail sync` prints `zygo: cannot check if the remote is public: <reason>` at the end of a sync that is not stopped. A sync that stops later prints only its stop line; its next sync prints the `unknown` line again, from the cache (reason `the last check, under 1 h ago, could not tell`) or from a new check. Reason: an SSH-only server has no HTTPS form; a refusal would turn sync off for good on such servers, and the operator note refuses only what zygo can see.
- The verdict is cached in `mail/sync/state.json` with the SHA-256 of each URL without user info (W-C7: no URL is stored), for 1 h of the injected clock. A changed URL, or a cache older than 1 h, runs the check again.
- Each probe ends at 10 s or at the pass deadline (S§8.3 step 4), whichever comes first. A check that reaches the pass deadline before it probed every URL is `unknown` with the reason `the pass deadline passed` and is not cached **(author, FX-40)**.
- A URL is printed only by `zygo mail sync`, through `zygo_config::display::scrub_userinfo` (as `crates/zygo-plugins/src/sync.rs:95`). It never goes into a log.

### 8.3 The sync

One sync of one workspace. The lock rule closes the lost-wakeup race **(lead)**:

1. **Gate.** Sync is on for this workspace (S§11), the host is set and valid (S§7.1), the branch name is valid (S§11). Else exit 0 with no action (child), or exit 1 with the reason (CLI).
2. **Mark.** Write `mail/sync/again` (empty file). The sync makes `mail/` and each folder in it only in a folder that exists, never the workspace state folder `<workspaces_dir>/<id>/`: with no state folder, `zygo ws forget` ran after step 1 (S§7.6), and the sync writes nothing and ends as a forgotten workspace (step 3).
3. **Lock.** Try an fs4 lock on `mail/sync/lock`. Not taken: exit 0 (child); the CLI waits for it up to 30 s, then exits 1. The holder sees the mark of step 2 (step 17). A lock file that cannot be made because the state folder is gone is a forgotten workspace too. Each time the lock is taken, and before any write, the workspace record is read again as at step 1: when it is gone, or it is not the record of step 1 (another id, name, common folder or main worktree, or, on unix, a state folder `<workspaces_dir>/<id>/` that is not the one step 1 read it in: a forget and a new workspace of the same id), or (unix) the locked file is no longer the file at `mail/sync/lock` (a forget unlinked the file this sync waited on), `zygo ws forget` ran while this sync waited (S§7.6), so the sync lets the lock go and writes nothing (no identity, no worktree, no `errors.log` line, no `state.json`): exit 0 with no line (child), or exit 1 with `zygo: the workspace was forgotten; nothing synced` (CLI). Nothing is synced; the next sync reads the new record at its step 1. A change of the record that keeps the workspace (its phase, its snapshot count, its members) is not a forget. A record that cannot be read at this check (for example a newer `version`) is not a forget either: it is the step 1 stop `zygo: mail sync cannot run: the workspace record cannot be read` (check `workspace`; exit 0 with no line for a child, exit 1 for the CLI), and nothing is written. The operator commands of S§7.6 that take this lock do the same.
4. **Clear.** Delete `mail/sync/again`. Then run one pass, steps 5 to 16, with a deadline of 120 s for that pass; each git call gets the time that is left.
5. **Finish incoming** (S§8.5, crash recovery). Each file that this step moves into a member's `new/` joins the delivered set of step 10 (FX-30). At the first pass of a sync, also the public check (S§8.2); `public` stops the sync here, before any fetch.
6. **Fetch** (S§6.5). No branch: bootstrap (S§6.4).
7. **Tip checks.** The tip tree has `ZYGO-MAIL` with the bytes `1\n`, else stop: `zygo: the side branch is not a zygo mail branch` (exit 1). Then the collision and twin checks (S§7.5).
8. **Member cache** (S§7.3).
9. **Import** (S§8.5).
10. **Count and wake** (S§10.1) **(author, FX-30)**. Before the first write of each delivery of step 9 (S§8.5 "Delivery", before step 1), the import records its pane key and ULID in `pending_wake` of `state.json`. The delivered set of the pass is, each ULID once: the files that step 5 moved into `new/`; the deliveries of step 9; and, in the first round of the pass, each `pending_wake` entry of an earlier run whose message is in `mail/<pane key>/new/` or `cur/`. The pass adds the size of the set to its delivered count, so a message is counted once also when its member read it before the pass. It wakes (S§10.1) only the members of the messages of the set that are in `new/` (unread mail); a message in `cur/` is counted and wakes no one. Right after the wake, one state write (it loads `state.json`, changes only `pending_wake` and saves it, under the sync lock of the pass, S§15) removes from `pending_wake` each entry that the pass counted, and each entry whose message neither arrived (not in `new/` or `cur/`) nor is staged (no `incoming/<pane key>.<ULID>.json`); step 9 delivers that message again when it is on the branch. An entry whose staged file is still in `incoming/` is kept: the step 5 that moves it into `new/` gives it to the delivered set, in this pass or, when that pass stops before its first round, in the next pass through the entry. A pass that stops before the wake keeps every entry, and the next pass counts and wakes those messages. Thus every message that a pass or a crash recovery puts into a member's `new/` is counted once, and its member is woken while the message is unread. Limit: the only second count and wake of a message comes from a crash (or a failed write) between the wake and that one state write: the next pass counts and wakes the message again. The M§7.3 `wake_gap` still holds.
11. **Outbox.** For each file in `mail/outbox/` in ULID order, at most 1000 readable files per pass:
    - it cannot be read (a link, a FIFO or another entry that is not a regular file, larger than 8 MiB, not JSON, or a `to` that is not an address): move it to `mail/outbox/refused/` as it is, without following a link, with one `errors.log` line that names its ULID and the check `read`. It takes no place in the 1000;
    - no machine reads its `to` host at the tip: the tip has neither a valid `registry/<host>.json` (S§7.3) nor a valid `ack/<host>.json` (S§7.4; an ack keeps an old host an own host of its renamed workspace, S§7.6 "Rename", S§8.5). This is a sender whose member cache is older than a rename. It takes no place in the 1000. If its path `<to>/<ULID>.json` is at the tip, it was pushed before (a crash after a push): delete it from the outbox. Else write one `errors.log` line that names its ULID and the check `no-host`, and refuse it as S§8.6 "At commit" does (the notice first, then the move to `mail/outbox/refused/` as it is; a notice that a later pass can deliver and that was not delivered leaves it in the outbox for the next pass, and a notice that no pass can deliver moves it with the line `notice: <reason>`), with the notice body `zygo did not send message <ULID> to <address>: host <host> has no registry on the side branch. The message is kept on this machine in mail/outbox/refused/.` As for the secret pass, the pass deadline (step 4) is checked before each such move: past it, the pass stops (`deadline`) and the files not reached stay in the outbox. The check uses the tip that this attempt fetched in step 6; a refusal stays when a rejected push (step 15) leads to a new fetch, as a secret-pass refusal does. A tip made by bootstrap in step 6 is not fetched and holds no registry yet: this check does not run (S§19 #8). Reason: no machine reads or deletes a file under an address folder of such a host, so a push would strand it (S-G5);
    - its path `<to>/<ULID>.json` is at the tip: it was pushed before (a crash after a push); delete it from the outbox;
    - with the split flag set (step 15), a file whose path is at the tip (this case and the same case of the `no-host` bullet) is not deleted here: it is kept and deleted only after an accepted push of this pass, as a pushed file;
    - else run the secret pass (S§8.6): clear, add the blob at that path; a finding, send the notice and then move it to `mail/outbox/refused/` (S§8.6 "At commit"); could not run, leave it in the outbox.
12. **Own files.** Write the own registry (S§7.3) and the ack of each own host (S§7.4 "Owner") when they change; delete an old own registry, and the ack of an old own host whose folders hold no message file and only ULIDs that its new list holds, when the pass deletes that host's registry in the same commit or the tip has no registry of this identity for that host (S§7.6 "Rename" (a) to (c)).
13. **Commit.** No change and the split flag not set: go to step 16. No change and the split flag set: the push of step 14 is of the tip itself. Else `read-tree` the tip, apply the adds and deletes, `write-tree`, `commit-tree` with the tip as parent (S§6.5). Record the blob ids of the own files that it holds as "about to push" (S§7.5).
14. **Push** `<commit>:refs/heads/<branch>` with `--porcelain`, no force.
15. **Result.** Before the push, zygo counts the push URLs of the remote (`git remote get-url --push --all`); `git push` sends to each and prints one porcelain ref line of the branch ref for each URL that it reaches. The push is:
    - accepted only when git exits 0, there is a ref line for every push URL, and every line accepts it;
    - "the branch moved" only when there is a ref line for every push URL and every line is a "branch moved" rejection;
    - else "any other error". This holds when one URL accepted and another did not (a rejection of any kind, or a URL that gave no ref line): the pass stops as below, records no own blob as pushed and keeps `pending`, the outbox stays, and the pass first sets the split flag (`push_split` in `mail/sync/state.json`, written under the sync lock with the atomic write). Reason **(author, FX-40)**: step 6 fetches only the fetch URL, which may now hold this pass's commit; a new attempt would find its files there and end without an error while another push URL never got the commit. A remote whose push URLs git cannot give is "any other error" with no push.
    - **Split flag (author, FX-40).** While it is set, a pass deletes no outbox file as pushed before (step 11), runs no prune (S§9), and always pushes: its commit, or the tip itself when it has no change (step 13), to every push URL. An accepted push (every URL took it) clears the flag; a split again keeps it and stops with the `push` stop; "the branch moved" tries again as in step 15. A `state.json` without the field reads as not set. The operator makes a URL that keeps refusing take the branch again (or removes it) with git.
    - Accepted: record the own blob ids as pushed; delete the pushed outbox files; set `refs/remotes/<remote>/<branch>` to the commit; this commit is now the tip of this pass.
    - Rejected because the branch moved (`fetch first`, `non-fast-forward`, matched without case in the porcelain line of the ref): go to step 6. At most 5 attempts per pass. After the fifth: exit 1, `zygo: the side branch moved 5 times during the sync; try again`. The outbox stays.
    - Any other error (authentication, network, a server rule): stop the pass, exit 1, one `errors.log` line with the check name and no URL. The outbox stays.
16. **Prune** when due (S§9), from the tip of this pass.
17. **Release and look again.** Release the lock. If `mail/sync/again` exists, go to step 3. Else exit.

Why no wakeup is lost: each starter writes the mark (step 2) before it tries the lock (step 3). A starter that fails the lock did so while a holder held it, so the holder's release (step 17) comes after the mark was written, and the holder's look after the release sees it. A starter that writes its mark after the holder's look takes the free lock itself.

The second and later passes have their own 120 s deadline. A sync runs passes while marks keep coming; it ends at the first look that finds no mark.

`mail/sync/state.json` records `last_ok`, `last_error` (check name only), the public-check cache, and the own blob ids, for `zygo mail ls`, S§8.2 and S§7.5. One rule set holds at read and at write: the value rules that keep URLs, credentials and content out of the file (S-C2, W-C7), as the writer applies them (`check_state`, `crates/zygo-mail-sync/src/state.rs`). At read, a missing file is the empty state; a link, a read error, a file that is not a JSON object, or a file that holds any value outside those rules is an error, never a default, so the S§7.5 twin check never runs on a guessed state; the file is left as it is. At write, a value outside those rules is refused and nothing is written. Thus every state that is read can be written back.

Reasons **(author)**:

- Step 13 builds each attempt on the newest tip. It adds new message paths and changes only own files, so a rebuild never conflicts, and no `git merge` or `rebase` runs.
- The retry needs no wait: each attempt starts with a fetch of the newer tip.

### 8.4 Rate limit

**(author)** A hook or a `zygo mail watch` starts the sync child only when `now - last_start` is `[mail] sync_gap` (default `60s`) or more, or when `last_start` is later than `now` (a clock set back, or a corrupt file). `last_start` is the Unix time in its own file, `mail/sync/last_start`; the starter (the hook or the watch) reads it and writes the new value (atomic) before the spawn. A separate file keeps the hook and the sync from writing one file. Two starters (hooks or watches) that read at the same time can both start a child; the lock and mark of S§8.3 make the second one a mark or a second pass. The clock is a parameter (S-C9).

### 8.5 Import (P3-4 receive side)

**(lead, author)** Each message is delivered once, by ULID. Two records say "delivered": the imported log (local, this workspace) and the own ack (on the branch).

**Candidates.** For each own host (S§7.5) and each local member with a name: stream `git ls-tree -z -l <tip> -- <name>@<host>/` (a literal path; the address form has no glob character). An entry is a candidate when its ULID is in no ack of an own host of this identity (the own ack: the union of their lists) and the pass has not taken that ULID already, in any own host or folder. Thus each ULID is a candidate at most once, and an identity delivers each ULID at most once (S§7.4 "Meaning"). Each ULID that the pass acknowledges goes into the ack of every own host whose folders (S§7.4 "Owner": the folder name is an address that `address::parse` accepts; the member name is not checked) hold an entry with that ULID, and so does each ULID of the own ack (S§7.4 "Owner"). A candidate whose ULID is already in the imported log (a sender added it again after a prune, S§14) is not delivered; it is only added to the own ack, so a prune removes it again. When its staged file `mail/sync/incoming/<pane key>.<ULID>.json` is still there (a crash after step 2, or a record that was out, see step 5), it is not acknowledged in this pass: step 5 of a later pass delivers that file, and the pass after that acknowledges the ULID. Reading a folder also stops at its target's inbox room: `[mail] max_unread` less the files in the target's `new/`, shared by the target's folders on all own hosts. A candidate already in the imported log takes no room, and the folder of a target with no room is not read. Thus one full inbox does not stop import for the other members. Reading stops at 500 candidates in the pass, or at the deadline; git is then killed (S§8.7). The pass is all its push attempts (S§8.3 step 15): the ULIDs taken as candidates are one set for the pass, an attempt after a rejected push reads no ULID of that set again and reads a new one only while the set holds fewer than 500, and each ULID that an earlier attempt acknowledged goes into the ack of each later attempt's commit. The rest waits for the next pass. Thus a large or hostile backlog still gives at most 500 deliveries per pass **(lead)**.

**Checks (S-C5).** A candidate is delivered only when:

- mode `100644`, type `blob` (no link `120000`, no submodule `160000`, no tree);
- the name matches `^[0-9A-HJKMNP-TV-Z]{26}\.json$` (Crockford ULID);
- size at most 8 MiB (a 1 MiB body can grow to 6 MiB with JSON escapes);
- the JSON is an object with `version` 1, `id` equal to the file's ULID, `to` equal to the folder name, `from` a valid address, `subject` at most 200 bytes, `body` at most 1 MiB, `sent_at` RFC 3339;
- the target's `new/` has fewer than `[mail] max_unread` files (M§5). A full inbox leaves the message on the branch, not acknowledged, for a later pass. It is not lost.

A candidate that fails a check other than the full inbox is not delivered. It is added to the imported log and the ack, so that it is not read again and a prune can remove it. One `errors.log` line names the ULID and the check. No time check is made: a message is never refused for its age (S-C8).

**Delivery, crash-safe.** Steps 1 to 3 are one M§5 delivery **(author)**: before step 1 of each candidate the pass takes `mail/lock` shared and reads the member records under it (M§5), and it holds the lock until step 3 is done, so no git call runs under it and a removal of `mail/<key>/` waits for one candidate at most. The member name of the candidate's folder maps to its pane key on that read, not on the record of step 1, and the inbox check uses that key; the wake after the import (S§10.1) uses the last such read. A candidate whose name has no record in that read is not staged, logged or acknowledged: its message stays on the branch, as the mail of a folder that this workspace does not have (S§13 limit 2). A delivery that cannot begin stops the import of this pass with one `errors.log` line `mail sync: import <ULID>: <kind>`, where `<kind>` is `busy` (the lock not taken in 2 s), `no-workspace` (no state folder or no `workspace.json`), `state` (a state folder that is a link or not a folder, or a state that does not read as valid), `mail` (`mail/` that cannot be made or checked: a link or an entry that is not a folder at `mail/`) or `io` (any other error); the delivery gives its source as a distinct error, so the kind is never a guess from the error code. The rest waits for the next pass.

1. Make `mail/<pane key>/` and its folders (`0700`, M§5) when missing, then write `mail/sync/incoming/<pane key>.<ULID>.json` (`0600`, the blob bytes), `fsync`. Thus every staged file has its maildir (step 5).
2. Append `<ULID>` to the imported log, `fsync`.
3. Rename the incoming file to `mail/<pane key>/new/<ULID>.json` (M§5).
4. Add the ULID to the own ack (pushed in step 13 of S§8.3).

Step 5 of S§8.3 finishes each file left in `incoming/` by a crash, each as one M§5 delivery: `mail/lock` held shared, with the member records read under it, from before the log check to the move into `new/`. A file whose pane key has no record in that read stays in `incoming/` while `mail/<pane key>/` exists, and the log is not changed: a record can be out for a short time (`zygo up` step 8 removes it and its undo write puts it back), and M§5 removes a maildir only after the write that removed its record for good (the `up` write or the undo write), so the maildir is the lasting sign. When `mail/<pane key>/` is missing, the file is deleted and the log is not changed: M§5 removed the mail of that member, and the branch copy stays (when a member of that name has a record again, a later pass imports it, or, when its ULID is already in the log, only acknowledges it, as in "Candidates"). A lock not taken in 2 s leaves the file and ends step 5 for this pass. A file whose ULID is not in the log can be torn by a crash during step 1: unless its bytes parse as a message whose `id` is the file's ULID, it is deleted and not logged, and the next pass imports the branch copy again. Otherwise step 5 appends the ULID to the log when it is not there, then renames the file into `new/`. A file whose ULID is in the log was synced in step 1, before step 2. `incoming/` is not swept. Thus a crash at any point gives one delivery: before step 2 the next pass completes it; after step 2 the log stops a second import.

A folder for a name that this workspace does not have is left alone. Its mail waits (S§13 limit 2).

The imported log grows by 27 bytes per delivered message and is never trimmed while the workspace exists (S§13 limit 6). Reason: a sender that crashed after its push can add a message again after a prune removed it (S§14); only a permanent local record stops a second delivery then.

### 8.6 Secret pass

**(operator)** The secret pass (§16.1) runs on each message before a commit; a message that it cannot clear is refused. **(lead, author)**

- **Pattern set.** The merged `[redaction]` patterns of the workspace's config, plus the act-lite store values of the main worktree, compiled with `zygo_redact::patterns::compile_with_store` (`crates/zygo-redact/src/patterns.rs:36`). Store values come from `zygo_redact::act_lite_store::read` (`crates/zygo-redact/src/act_lite_store.rs:62-64`). No store file is not a failure (regex patterns only, as §16.1). The values stay in process memory (C3).
- **Run.** `zygo_redact::secret::redact` (`crates/zygo-redact/src/secret.rs:74-76`) on the whole message object, with a fresh session cache and the default deadline policy (`crates/zygo-redact/src/secret.rs:22-26`: 200 ms plus 50 ms per MiB).
- **Three results.**
  - *Clear*: `Ok` with `changed = false` (`crates/zygo-redact/src/secret.rs:37-42`).
  - *Finding*: a `redact` finding or `Blocked`. The message is refused. zygo never pushes a redacted copy: the operator note says "refused".
  - *Could not run*: `Timeout`, a store file that exists but cannot be read or parsed, or patterns that do not compile.
- **At send.** `mail_send` and `zygo mail send` run the pass before they write a remote message to the outbox. A finding: `zygo: message not sent: it holds a secret (<ids>)`. Could not run: `zygo: message not sent: the secret check could not run (<check name>); send it again`. Nothing is written.
- **At commit** (S§8.3 step 11). The pass runs again, because the patterns or the store can change between send and sync.
  - Could not run: the message stays in the outbox for the next pass **(lead)**. One `errors.log` line with the ULID and the check name.
  - Finding: one `errors.log` line with the ULID and the pattern ids. The sender gets a notice **(lead, author)**, then the file moves to `mail/outbox/refused/`, in that order **(author)**. The notice is a local M§5 message from `zygo` to the sending member, subject `not sent: <ULID>`, body `zygo did not send message <ULID> to <address>: it holds a secret (<ids>). The message is kept on this machine in mail/outbox/refused/.` It is delivered and woken by M§7 like any local mail; the wake uses the member records of the notice's delivery. It is an M§5 delivery: its target is the sender's name resolved on the member records read under `mail/lock` held shared **(author)**, in three cases. (1) The record of step 1 and that read both have the name: when it maps in both to the same pane keys, the notice goes to those keys; when the keys differ (the sender was renamed, or another member holds the name now), the member under the lock is not the sender, and no one gets the notice (`moved`, below). (2) The record of step 1 does not have the name and that read has it (the member renamed itself to that name and kept its key, or joined during the run): the notice goes to the keys of that read. (3) That read does not have the name: no one gets the notice (`no-record`, below). Only a notice whose delivery does not begin is one that a later pass can deliver **(author)**: the lock not taken in 2 s, an I/O error of the lock, a state that does not read as valid, a `mail/` that is a link or not a folder, or no workspace. It leaves the message in the outbox, with one `errors.log` line `mail sync: outbox <ULID>: notice`; the next pass runs the secret pass on it again, and sends the notice and moves the file then. A notice that fails after its delivery began is one that no pass can deliver: it does not keep the message, and is delivered to no one: the file moves to `refused/` with no notice and one `errors.log` line `mail sync: outbox <ULID>: notice: <reason>`, where `<reason>` is `full` (the sender's `new/` holds `[mail] max_unread` files, and only the sender's reads clear it), `no-record` (that read does not have the sender's name: its record went during the pass; it gets no maildir, as `cli`), `moved` (the record of step 1 and that read both have the name and map it to other keys), `maildir` (the write into the sender's maildir met an entry that is not a real folder, for example a link or a file at `mail/<pane key>/new`) or `io` (any other error of that write). Later passes write nothing more about that message. When the move fails after the notice was delivered, the file stays in the outbox with the `notice` line, and the next pass sends a second notice: that is the accepted cost, as a lost notice would leave the sender with no word of the refusal. A `refused/<ULID>.json` that holds another message stops the refusal before the notice, and the file stays in the outbox. The S§8.3 step 11 `no-host` notice is sent the same way. It is content in the maildir, not a log line. A message sent by the CLI as `cli` has no inbox; `zygo mail ls` shows the refused count.
- Local mail (to this machine) gets no pass. It does not leave the machine; M§5 does not change.

### 8.7 The git runner

**(lead)** The plugin runner does not fit: on a non-zero exit, `run_bounded` keeps only the first stderr line and drops stdout (`crates/zygo-plugins/src/sync.rs:91-95`). The sync needs:

- **Whole mode.** Exit status and stdout on every exit, success or not, and up to 64 KiB of stderr on a non-zero exit. `push --porcelain` reports a rejected ref on stdout with a non-zero exit; the public check needs the stderr text. `run_bounded_out` and its `Ran` (`crates/zygo-work/src/repo.rs:61-75`) already return exit status, stdout and up to 64 KiB of stderr (read on a non-zero exit); the builder makes it reachable from the mail module, or copies it.
- **Stream mode.** For `ls-tree -z`: each NUL-ended entry goes to a callback, which can say stop; then the process group is killed. Memory stays bounded by one entry.
- Both modes: stdin null, own process group, killed at the deadline (as `crates/zygo-work/src/repo.rs:71-75`).
- Each git message that zygo tests for is matched without case.

## 9. Prune

**(operator)** A prune rewrites only the side branch: a new orphan commit with the messages that are not imported, then a force push of that branch alone. This is the one force push that zygo does. **(lead, author)**

- **When.** At step 16 of a pass, when `git rev-list --count <tip>` is 1000 or more, when the own ack has more than 100 000 entries (S§7.4), or when `zygo mail sync --prune` runs.
- **Base.** The tip of this pass: the commit of this pass's accepted push, if there was one, else the fetched tip **(lead)**. A prune never uses a fetch from before this pass's own push.
- **Tree.** The base tree, less each message file `<name>@<host>/<ULID>.json` whose ULID is in the valid
  `ack/<host>.json` of the base tree. A message file is a blob of mode `100644` or `100755` (`prune::removable`); a link
  (`120000`), a submodule (`160000`) or a tree at such a path is not one and stays. The prune looks only at a canonical
  `<ULID>.json` name (`address::ulid_file`) directly in a top-level folder whose name is an address (`address::parse`: a
  member name that is not reserved, then `@`, then a host in the S§5 form). A folder whose name is not an address, such
  as `all@<host>/` or `Bob@<host>/`, is not looked in, so no entry in it is removed (S§13 limit 2). The member name is
  not checked against the host's registry: an entry whose ULID the ack lists can never be delivered (S§7.4 "Meaning").
  Nothing else is removed: no registry, no ack, no message that its host has not acknowledged. No clock and no `keep` is
  used (S-C8).
- **Commit.** `commit-tree` with no parent, message `zygo mail [skip ci]`.
- **Push.** `git push --porcelain --force-with-lease=refs/heads/<branch>:<base> <remote> <commit>:refs/heads/<branch>`, its result read as in S§8.3 step 15. The lease is the base. A push from another machine after the base makes the lease fail at every push URL ("the branch moved"); the prune is then skipped in this pass; the next pass tries again. A push that one push URL took and another did not (a lease that failed at one URL only, or any other mix) has rewritten the branch at some URLs only: it stops the pass with the `push` stop of S§8.3 step 15 (exit 1, one `errors.log` line), never as a lost lease, and it sets the split flag of S§8.3 step 15 **(author, FX-40)**. While the split flag is set, no prune runs: the pass pushes its tip to every push URL first (S§8.3 step 15), and only a later pass, after an accepted push cleared the flag, prunes. Any other failure of the prune's git work (the commit count, the tree and commit, or the push: a rejection other than the lease, a git error or the deadline) also skips the prune and does not fail the pass: the pass ends as without a prune, and the sync report gets the note `prune skipped (<check>)`; the next pass tries again. A failed check of "Only its own branch" stops the pass. Thus a prune never removes a message that it did not see in its base, and the messages of this pass's own push are in its base and are not acknowledged yet, so they stay.
- **Only its own branch.** The branch name must pass the `sync` form of S§11 (prefix `zygo/`), and the base must have `ZYGO-MAIL`. Both are checked again just before the force push.
- **After a prune**, other machines fetch with the `+` refspec (S§6.5); their next pass builds on the new root. Each owner removes the pruned ULIDs from its own ack (S§7.4). The old objects stay on the server and in local object stores until each one runs its own garbage collection.

Local retention: the M§5 sweep also deletes files in `mail/outbox/refused/` older than `[mail] keep`, and files in `mail/outbox/tmp/` older than 1 h (as M§10). It never touches `mail/sync/incoming/`.

## 10. Delivery on the receiving machine (P3-4)

### 10.1 Wake after import

**(author)** The sync process acts as the sender's process of M§7.3 for each member in the delivered set of step 10 of S§8.3 (FX-30): the members that got mail in step 9, in the crash recovery of step 5, or in a run that stopped after its delivery and before its wake (the `pending_wake` entries). Of these, it wakes only a member whose message is in its `new/`, unread mail: a recovered message that the member already moved to `cur/` is counted and wakes no one. It takes the member records of the last delivery of step 9, or, when step 9 delivered nothing, a read under `mail/lock` held shared (M§5); a read that cannot begin writes the `errors.log` line `mail sync: wake: read` and wakes no one, and the messages stay counted. It wakes the member when it is idle by AU26 and its launcher is live (AU74), with the M§7.3 steps (the `@zygo-pane` and `@zygo-ws` checks, the fixed wake text, `wake_gap`). A busy member reads the mail at its `Stop` (M§7.1). A member whose launcher is not live gets the line at its next `SessionStart` or `UserPromptSubmit` (M§7.2).

### 10.2 Which process fetches

A fetch on machine B runs only in a sync on B (S§8.1). Thus remote mail reaches B when one of these happens on B:

1. a member of B sends mail to a remote address;
2. a member of B has a hook event `session`, `prompt`, `stop` or `tool-end`, and `sync_gap` has passed since the last sync started;
3. a `zygo mail watch` runs on B (a lead under a bounded tool, or a person);
4. a person runs `zygo mail sync` on B.

### 10.3 The limit, stated plainly

**(author)** When every member on B is idle, no watcher runs on B, and no person types on B, nothing on B fetches. Mail for B waits on the remote, with no upper bound, until one of the four events happens on B. It is kept: a prune does not remove mail that B has not acknowledged (S§9). zygo adds no daemon, timer, `cron` entry or tmux status command to close this gap (W-G6, S§12). A team that needs B to answer while it is idle keeps a `zygo mail watch` running on B (for example a lead under a bounded Monitor tool, M§8), or sends a person to B.

A host that is idle for months keeps its mail on the branch, and the branch grows by that mail (S§13 limit 2). `zygo mail ls` with no name (and `/zygo:mail`) ends with one sync line for this workspace. With sync on: `mail sync: last sync <time>, <n> in the outbox, <m> refused`, then `, last error <check>` while `state.json` holds one; `<time>` is `last_ok` in RFC 3339 UTC, `never` when there is none; a value that cannot be read is `-`. With `sync` set and no host: the S§7.1 line. With another S§11 form error: the off line that `zygo mail sync` prints for it. With no `sync` key for the workspace, or a config that does not load: no line. The operator removes a host that will not return with `--drop-host` (S§7.6).

### 10.4 Agent text

M§8a item 8 gains one clause when sync is on for the workspace and the member cache is not empty, after the member list, before "Send with": `Other machines: <address>, <address>, …; mail them by the full address.` Each address is `<name>@<host>`, in host order, then names as stored, each address once; the addresses are joined with `, `. At most 64 addresses; when there are more, the last of the 64 is followed by `, and <n> more; mail_list lists all`, before `; mail them by the full address.`, where `<n>` is the number of addresses left out. Sync off, a config that does not load, or an empty cache gives no clause. `mail_list` returns the remote addresses from the member cache as `remote`: each address once, in the clause order, with no limit; sync off or a config that does not load gives an empty list. The tool description of `mail_send` adds: "To a member on another machine, use `<name>@<host>`." No message text goes into this text (M§8a).

### 10.5 Output of remote values

A remote `from` or `subject` reaches a terminal in `zygo mail watch`, `zygo mail ls <name>` and `zygo mail read`, which also prints the body. Each prints each such value, and each body line, through `zygo_core::esc` (C0, C1 and bidi controls escaped). Reason: remote text is untrusted (S-C5); a raw escape sequence could change the terminal.

## 11. Configuration

User config only (M§11, AU10, AU22). The team layer drops `[mail]` with one warning, as the `USER_ONLY` tables (`crates/zygo-config/src/layers.rs:66-68`); add `mail` there if the P3-1 build did not. `.zygo.toml` drops it by its kept list (`crates/zygo-config/src/repo_file.rs:39`).

```toml
[mail]
host = "studio"          # S§7.1; no default; required for sync
sync_gap = "60s"         # S§8.4; <n>s | <n>m | <n>h

[mail.workspace.zygo]    # the workspace name (W§5, AU2)
sync = "zygo/mail"       # S-G6: absent = off; the side branch
remote = "origin"        # optional; default "origin"
host = "studio-2"        # optional; overrides [mail] host for this workspace
```

- **Per workspace (operator: "opt-in per workspace").** **(author)** The key `sync` sits in a table per workspace name, because the user config is shared by all workspaces of the user. Each machine opts in on its own, so each machine's operator consents (S-C7).
- **`sync`** must match `^zygo/[a-z0-9][a-z0-9-]{0,31}$`. Anything else is a config check error and sync stays off. Reason: the force push of S§9 must never reach a branch that is not zygo's (S-C3).
- **`remote`** must name a remote that `git remote` lists. Else the pass stops with `zygo: no such remote`.
- `zygo config check` validates the forms; `zygo config show` prints them with their source.
- `[mail] keep` (M§11) does not act on the side branch (S-C8).

## 12. Alternatives rejected

- **A zygo relay server** **(operator)**: adds a server; the repository remote already has the access control.
- **Mail in `main`** **(operator)**: fills history and races between worktrees.
- **Git notes** **(operator)**: poor tooling for many small files.
- **Delete on import** (first draft) **(lead)**: breaks "append-only, never edited".
- **Drop by age** (first draft) **(lead)**: loses mail of an idle host, and lets one machine's `keep` decide for another host (S-C8).
- **A checked-out worktree** **(author)**: a checkout writes paths that the remote chose (S-C5). The index-only worktree gives the same isolation with no file written.
- **`git worktree prune`** **(lead)**: acts on every worktree of the user's repository.
- **`git merge` or `rebase` on a rejected push** **(author)**: the rebuild on the tip (S§8.3 step 13) never conflicts, because each path has one writer.
- **Push a redacted copy** **(author)**: the operator note says "refused"; a redacted copy changes the message with no word to the sender.
- **A periodic sync from the tmux status line (`#(...)` in `status-right`)** **(author)**: it is a timer by another name (W-G6), and it changes the user's status line.
- **A forge API check for "public"** (GitHub `GET /repos/...`) **(author)**: works for one forge only. The anonymous `ls-remote` works for each HTTPS git server.
- **Refuse sync when the public check is `unknown`** **(author)**: an SSH-only server could never sync; the operator note refuses only a remote that zygo can see is public.
- **Ack by a ULID watermark** **(author)**: messages from different senders arrive out of ULID order, so "all before X" is false.
- **Trim the imported log by age** **(author)**: a re-add after a prune (S§14) could then deliver twice.
- **The system host name as default host** **(lead)**: publishes a local name that the operator did not choose.
- **Per-machine identity** **(author)**: two clones on one machine would share one host.
- **`all` across hosts** **(author)**: a broadcast to every machine of a repository is a larger change of reach than P3-2 asks for. A sender names each remote address.

## 13. Accepted limits

1. **No proof of the sender.** A person with write access to the remote can write any `from`, forge an ack (and so make a prune remove mail), or delete the branch (S-C7). Commit signing is not checked.
2. **Idle host, unknown names and entries that are not messages.** Mail for a host that does not sync, or for a name
   that its host does not have, stays on the branch until the host imports it or the operator drops the host (S§7.6). A
   message file in such an address folder whose ULID the host acknowledged in another folder goes with the next prune
   (S§9 "Tree"); this holds only for a folder whose name is an address (`address::parse`). A link, a submodule or a tree
   at a message path stays until the operator drops its host: no prune removes it, and after a rename it does not keep
   the old ack once its ULID is acknowledged (S§7.6 "Rename"). An entry in an address folder `*@<host>/` with no
   canonical `<ULID>.json` name (`address::ulid_file`), such as a regular file with another name or a `<ULID>.json` name
   in lower case, is never delivered, never pruned and never counted by `--drop-host`, and after a rename it does not
   keep the old ack. It stays until the operator removes it, or until `--drop-host` deletes its folder with every entry
   in it. An entry of any name or kind in a top-level folder `<x>@<host>/` whose name is not an address, such as a
   reserved name (`all@<host>/`) or a name with upper case (`Bob@<host>/`), is never delivered, never pruned and never
   counted, and after a rename it does not keep the old ack. No prune, `--take-host` or `--drop-host` touches it, and no
   mail-sync command removes it: only the operator can remove it, with git. The branch grows by that mail and those
   entries.
3. **Local copies.** The fetched blobs, including mail to other hosts, are in the repository's object store with the repository's own file modes. M-C4 (`0700`/`0600`) holds for `mail/`, not for the object store.
4. **Branch deleted on the remote.** The mail on it is gone. The next pass makes the branch again (S§6.4).
5. **Twin window.** Two copies of one workspace state can deliver one message on two machines until the first pass after one of them writes its registry or ack (S§7.5).
6. **Imported log.** 27 bytes per delivered message, kept while the workspace exists. It is read at each pass.
7. **Public flip.** A repository made public is found within 1 h of syncs (S§8.2).
8. **Forge CI and webhooks.** S§6.6.
9. **Large pushes.** A writer can push large files to the side branch, and each fetch downloads them, as for any branch. S§8.5 bounds only what zygo reads.
10. **Host name taken again.** When another machine registers a host name after its old owner renamed or was dropped, a sender whose member cache is older than that still sends to `<name>@<host>`, and the new owner gets that mail, because the member cache holds no identity (S§8.3 step 11).

## 14. Flows

Each row is one case, walked end to end.

| Case | What happens |
|---|---|
| Normal | A: send → outbox → child → pass: fetch, push. B: hook → child → pass: fetch, import (incoming, log, rename, ack), wake, push the ack. Any pass after 1000 commits: prune removes the acknowledged file. B's next pass removes the ULID from its ack. |
| Crash after A's push, before the outbox delete | A's next pass finds the path at the tip and deletes the outbox file. If a prune removed it first, A adds it again; B finds the ULID in its imported log, does not deliver it, and adds it to its ack again (S§8.5 "Candidates"); a later prune removes it. One delivery. |
| Crash during B's delivery | S§8.5 "Delivery, crash-safe". One delivery. |
| Crash after B's delivery, before its ack push | The next pass finds the ULID in the imported log, skips it, and pushes the ack. |
| Crash during a commit | Index only. The next pass reads the tip into the index again. |
| Crash during a prune | The lease push is atomic on the server: the branch is the base or the new root. |
| A and B push at the same time | One is rejected, fetches, builds on the new tip, and pushes (S§8.3 step 15). |
| A and B prune at the same time | The second lease fails; it skips. |
| A prunes while B pushes | B's push after A's base makes A's lease fail; or A's prune lands first and B's push is rejected, then B builds on the new root. |
| Two syncs on one machine | The lock and the `again` mark (S§8.3). |
| B idle for months | Its mail stays on the branch; prunes keep it (S§10.3). |
| B's inbox is full | The message is not acknowledged and stays for a later pass (S§8.5). |
| Host renamed while mail waits | S§7.6 "Rename". |
| Different `keep` on A and B | No effect on the branch (S-C8). |
| Moved repository, `ws forget`, restored backup | S§7.5 "Collision" and "Twin"; S§7.6. |
| Hostile branch | No marker: stop (S§8.3 step 7). Bad registry or ack: skipped or empty (S§7.3, S§7.4). Bad message: not delivered, logged by ULID (S§8.5). A forged ack for another identity in this host's name: collision stop (S-C7). |
| No network, authentication fails | The outbox stays; the next pass tries again. |
| Secret found at commit | S§8.6: refused, the sender gets the notice. |
| Secret check could not run at commit | The message stays in the outbox (S§8.6). |
| Sync child does not start | One `errors.log` line. The next trigger tries again. |
| Worktree folder deleted | `worktree add --force` makes it again (S§6.3). |
| `zygo ws forget` | The W§12 checks; the sync lock; the question when the outbox or its refused folder is not empty (S§7.6); `git worktree remove --force -- <path>` (S§6.3); `mail/lock` exclusive (M§5), held across the count again and the state rename; a higher count refuses (S§7.6 step 4, FX-28); then the state and the cache as W§12. The worktree goes before the cache: the cache removal deletes the worktree folder, and git would then still list the worktree. |

## 15. Owners of state

| State | Owner (the only writer) |
|---|---|
| `refs/heads/<branch>` on the remote | each syncing workspace, by fast-forward push; a prune, by lease |
| `<name>@<host>/<ULID>.json` | added by the sender's pass; removed only by a prune after `ack/<host>.json` lists it, or by `--drop-host` |
| `registry/<host>.json`, `ack/<host>.json` | the workspace whose identity is in them; `--drop-host` |
| `ZYGO-MAIL` | the pass that makes the branch |
| Sync worktree, its index and `HEAD` | the pass that holds `mail/sync/lock`; `zygo ws forget` removes the worktree while it holds that lock (S§7.6) |
| `mail/outbox/` | `mail_send` and `zygo mail send` add, under `mail/lock` shared (S§7.6 step 4, FX-28); the pass under the lock deletes or moves to `refused/`; the sweep deletes old `refused/` and `tmp/` files |
| `mail/sync/incoming/`, `mail/sync/imported` | the pass under the lock |
| `mail/sync/state.json`, `mail/sync/members.json` | the pass under the lock |
| `mail/sync/last_start` | the hook path that starts a sync child (S§8.4) |
| `mail/sync/again` | each starter writes it; the lock holder deletes it |
| `mail/sync/identity` | the first sync; `--new-identity` |
| `mail/<pane key>/new/` | M§5 writers and the import (S§8.5) |

## 16. Base-spec changes

- **M§3** The non-goal "Mail between … machines" is now this spec. Mail between OS users and between workspaces stays a non-goal.
- **M§5** `from` and `to` can hold a full address (S§6.2). The file format and its version do not change. The import and the notices of a pass are M§5 deliveries under `mail/lock` (S§8.5, S§8.6), and `zygo ws forget` holds `mail/lock` exclusive across its count again and its state rename, and a remote send holds it shared across its outbox write (S§7.6 step 4, FX-28).
- **M§6** Gains the address grammar (S§7.2) and the reserved name `zygo`.
- **M-C4** Holds for `mail/`. Synced mail also exists in the repository's object store and on the remote (S§13 limit 3, S-C7).
- **M-C6** Holds: a cloned repository cannot send mail. Member names come from user config or from approved repository files (workspace definition design R§7). The side branch delivers mail only after the user-config opt-in (S-G6).
- **M§8, M§8a** `mail_send` takes addresses; `mail_list` returns remote addresses; item 8 gains one clause (S§10.4). `zygo mail sync` is a new verb; `zygo mail ls` shows a sync line and the outbox and refused counts; `zygo mail watch` starts the sync child (S§8.1).
- **M§11** Gains `host`, `sync_gap` and `[mail.workspace.<name>]` (S§11).
- **W§7.1** Gains the cache path `$HOME/.cache/zygo/workspaces/<id>/mail-sync/` and the state paths `mail/outbox/` and `mail/sync/`.
- **W§12** `zygo ws forget` asks when the outbox or its refused folder is not empty and removes the sync worktree (S§7.6, S§14).

## 17. Files expected to change

P3-1 is not built. "The mail module" is the place where the P3-1 build puts the M§5 store.

| File | Change |
|---|---|
| The mail module | New parts: address grammar (S§7.2), outbox and the send-time pass (S§8.6), sync and passes (S§8.3), bootstrap (S§6.4), import (S§8.5), registry, ack and member cache (S§7.3, S§7.4), identity checks (S§7.5), operator commands (S§7.6), public check (S§8.2), prune (S§9). |
| `crates/zygo-plugins/src/sync.rs:32-58` | Reuse `git_command`, or move it to a crate that the mail module can use. Its behavior does not change. |
| `crates/zygo-work/src/repo.rs:61-75` | Make `run_bounded_out` reachable from the mail module (or copy it), and add the stream mode (S§8.7). |
| `crates/zygo-redact` | No change. Called from the mail module (S§8.6). |
| `crates/zygo-hooks/src/workspace.rs` | The rate-limited spawn of the sync child on `session`, `prompt`, `stop` and `tool-end` (S§8.1, S§8.4), on the pattern of `start_snapshot_child` (`:484-513`). |
| `crates/zygo-hooks/src/mail.rs` | The item 8 clause (S§10.4). |
| `crates/zygo-mcp/src/tools/` (mail tools of P3-1) | Addresses, the send-time pass, remote addresses in `mail_list`, the description sentence. |
| `crates/zygo/src/cmd/` (mail command of P3-1) | `zygo mail sync` and its options, the hidden `--ws-id <id> --background` form (AU44), the start of the sync child in `watch`, the sync line in `ls`, `esc` for remote values. |
| `crates/zygo/src/cmd/mod.rs` | The results sweep is skipped for `mail sync --background`, as for `ws` (AU71): it runs on hook events. |
| `crates/zygo/src/cmd/doctor.rs` | The twin line (S§7.5). |
| `crates/zygo-workspace/src/admin.rs` | `ws forget`: the question, the worktree removal (S§7.6, S§14). |
| `crates/zygo-config/src/schema.rs`, `check.rs`, `layers.rs` | S§11 keys, checks, `mail` in `USER_ONLY`. |
| `docs/premises.md` | SP1 to SP6 (S§20). |

## 18. Left to the builder

- The list of git stderr messages that mean `private` (S§8.2 step 4) and "branch moved" (S§8.3 step 15), for the git version that the build pins, as lists in code with a test per message. CI installs no git. The pinned version is the `git:` header line of `docs/premises.md`, a minimum: SP3 to SP6 (S§20) are run on it.
- Where the moved builder and runner live (S§17).
- The `errors.log` text of each line, inside S-C2.
- The test seams of S-C9. Their form is free; production code takes the real ones.

## 19. Acceptance

Each check is a test or a command. A "machine" in a test is one `HOME` with its own clone of one bare repository, used as remote by a `file://` URL. Clocks and the public probe are injected. The wake uses the multiplexer stub (W§13). "Seam" means a test hook between two named steps. No test runs a real agent or depends on wall-clock time (S-C9).

1. **Normal.** Machine A (host `a`, member `alice`) and machine B (host `b`, member `bob`). `alice` sends to `bob@b`. A sync on A, then on B: `bob`'s `new/` holds a file whose bytes equal the blob that A pushed, with `from` `alice@a` and `to` `bob@b`. The tip still holds the file, and `ack/b.json` lists its ULID.
2. **User repository untouched.** Each user checkout of A and B has `gc.auto = 1` in its repository config. After test 1: `HEAD`, the branch list, `git status --porcelain`, the index and `FETCH_HEAD` (absent, or the same bytes) are the same as before; `git worktree list` shows the user's worktrees as before plus one detached `mail-sync` entry; no new pack file and no `gc.log` exist in the repository.
3. **Push race.** A push of B lands between A's fetch and A's push (seam). A's first push is rejected, its second attempt succeeds, and the tip holds both messages. With the seam forcing 5 rejections, A exits 1 and its outbox still holds the message.
4. **Secret.** A send whose body holds a built-in AWS key pattern is refused at send with the pattern id, and nothing is written to the outbox. A message that passes at send and meets a new pattern before the pass moves to `outbox/refused/`, is not in the tip tree, its `errors.log` line holds the ULID and the id and no body or address, and `alice` has a notice from `zygo` in `new/`.
5. **Secret check could not run at commit.** With `expire_after_scans = 0` (the existing test hook, `crates/zygo-redact/src/secret.rs:18-19`), and in a second case with an act-lite store file that does not parse: the message stays in the outbox, is not on the tip, and no notice is sent. The next pass with a working check pushes it.
6. **Public check.** The probe answers `public`: the sync exits 1 and the bare repository's refs do not change; a remote send is then refused. `unknown`: the sync runs and prints the line. A table test maps the URL forms of S§8.2 step 2. With the injected clock 59 minutes later the cache is used; 61 minutes later the probe runs again. The probe command has `credential.helper` and `core.askPass` empty, and no `GIT_ASKPASS` or `SSH_ASKPASS` in its environment.
7. **Own branch only.** The bare repository has `zygo/mail` without `ZYGO-MAIL`: the sync exits 1 and pushes nothing. `sync = "main"` is a config check error and no sync runs. With no `host` set, no sync runs, and the config check names `[mail] host`.
8. **Bootstrap.** No branch on the remote and no worktree: the first sync makes the branch with `ZYGO-MAIL` and the message. Two machines that bootstrap between one fetch and one push (seam): one branch, both messages, one root.
9. **Collision and take-host.** Two machines with host `x`: the second exits 1 with the collision line and imports nothing. `--take-host` then succeeds; a message that the first machine acknowledged is not delivered on the second; the first machine's next sync exits 1.
10. **Moved repository.** A workspace state under another id holds the identity of `registry/x.json`: the collision line names that workspace.
11. **Twin.** A copy of A's state folder runs on machine C and writes the ack. A's next pass exits 1 with the twin line, and `zygo doctor` on A prints it. `--new-identity` with a new host then syncs.
12. **Hostile tip**, each case in `bob@b/`: a link entry, a submodule entry, a 9 MiB blob, `id` not equal to the name, `to` not equal to the folder, bad JSON. None is delivered; no file is written outside `bob`'s maildir and `mail/sync/` other than the `errors.log` lines and the git files of the pass's fetch, commit and push: the ref `refs/remotes/<remote>/<branch>` (S§6.5, S§8.3 step 15) and the sync worktree's own `HEAD` and index (S§6.3), each with the reflog that git keeps for it (a git object store is not counted); each is in `ack/b.json` after the pass, and `errors.log` has one line each with a ULID and a check name. A registry with 257 names, or with `host` not equal to its name, is not in the member cache.
13. **Import cap.** With 1200 messages for `bob@b` and `max_unread = 5000`, one pass delivers 500, and three passes deliver all, each once.
14. **No second delivery.** A crash seam after A's push and before its outbox delete, then a prune, then A's next pass: the file is on the tip again, and B delivers it zero more times. Crash seams after each step of S§8.5 "Delivery": one delivery each time.
15. **Inbox full.** `bob` has `max_unread` files in `new/`: the message is not delivered and not acknowledged; a prune keeps it; after `bob` reads, the next pass delivers it.
16. **Lost wakeup.** A seam holds the holder between the end of its pass and its release; a second child writes `again` and fails the lock; the holder runs a second pass, and a message that the second sender wrote to the outbox is on the tip at the end. A seam between the holder's release and its look, with a starter that writes its mark there: either the holder or the starter runs the next pass.
17. **Prune after push.** A pass pushes a message, then prunes in the same pass (1000 commits): the new root holds that message. With a push of another machine between this pass's push and its prune (seam), the lease fails and the tip is that machine's commit.
18. **Prune keeps the unacknowledged.** With A's `keep = "1d"`, B's `keep = "90d"`, and the injected clock a year later, a prune on A removes only files that the acks list. B, idle the whole time, then delivers each waiting message once.
19. **Host rename while mail waits.** `bob@b` has 3 waiting messages; B changes host to `b2`. B's next pass delivers the 3, deletes `registry/b.json`, and deletes `ack/b.json` after a prune removes the 3. A send to `bob@b` after the first pass is refused; a send to `bob@b2` arrives.
20. **Drop host.** `--drop-host b` with 3 waiting messages prints the count of 3 and asks; no deletes without yes; with `--yes` the files are gone from the tip. `ws forget` with 2 outbox files prints 2 and asks.
21. **Remote send rules.** A member with no name sending to `bob@b` gets the S§7.2 line; a send to `cli@b` is refused.
22. **Rate limit.** With the clock fixed, ten `tool-end` events start one sync child (spawn seam count 1); after the clock moves 60 s, the next event starts a second. A send starts a child with no gap.
23. **Wake.** After B's import, an idle `bob` with a live launcher gets the M§7.3 wake text through the multiplexer stub, once within `wake_gap`; a busy `bob` gets none, and its `Stop` hook blocks with the M§7.1 line.
24. **No content in logs.** The walks of `crates/zygo/tests/mail_sync_no_content.rs` run on `file://` remotes. They run the real `zygo` binary (`zygo mail sync`, `zygo doctor`, `zygo ws forget`, `zygo config check`, `zygo hook trim`) with its production clock, spawner, public probe and wake, and in process the S-C9 stub seams and the multiplexer stub. Every pass that they run in process has a deadline of an hour through the S-C9 seam `sync::run_with_deadline`. The real binary runs keep the production limits, and these limits apply to them: 120 s for each pass (S§8.3 step 4, the `--drop-host` pass among them), 10 s for the public probe (S§8.2 step 3; only the #6 runs, whose push URL is `https`), 30 s for the sync lock (`LOCK_WAIT`, S§8.3 step 3), and in `zygo ws forget` 2 s for `mail/lock` (`MAIL_LOCK_WAIT`, S§7.6 step 4), 5 s for the state lock and 10 s for `git worktree remove` (S§6.3 Remove), and in `zygo doctor` 2 s for each version probe. None of them decides a result. Each run does little work (at most a few messages to import, send or delete, and a probe that the `PATH` stub answers at once); no other process holds a lock that a run waits for, as each lock is in the scratch home of its machine and the test starts no thread and runs nothing while it waits for a binary run; and no assertion depends on how long a run takes. A run that goes past a limit gives a stop line (exit 1) or an `unknown` note (S§8.2), or, at a lock, a busy line (exit 1), or, at `git worktree remove`, a worktree entry that git keeps; the assertions of each binary run fail in each case: each run asserts its exit code, the sync runs that must stop assert their whole stderr, the `private` run of #6 asserts that its stderr is empty and that its one probe ran to its end, the `unknown` run of #6 asserts that its stderr is exactly its note and that its one probe ran to its end, and #26 asserts the `git worktree list` after the forget; a doctor version probe past its limit changes no twin line, which is what #11 asserts. So a limit can fail a walk but cannot make it pass. They walk tests 1 and 2, 3, 4 (the refused message gives exactly one new file in the sender's `new/`: the notice from `zygo` to the sender's name, with the subject `not sent: <ULID>` and the body of `outbox::refuse`), 5 (the sender's `new/` gets no new file: no notice), 6 (the `public` verdict and the send it refuses; `unknown`; the URL forms; the cache at 59 and 61 minutes; the production probe `public::LsRemote` through the binary, with a stub `git` on its `PATH` that answers `ls-remote` with a ref line (`public`: the sync is refused, and the probe is its only network call, with no fetch) and with `Repository not found.` (`private`: the sync goes on, with exit 0, the summary line as its only stdout, an empty stderr, and one probe that ran to its end) and with `remote: no idea` and exit 128 (`unknown`: the sync goes on, with exit 0, the summary line as its only stdout, a stderr that is exactly the note `zygo: cannot check if the remote is public: git ls-remote failed: remote: no idea`, the probe and then a fetch as its network calls, and one probe that ran to its end), and that records the arguments and the environment that `probe_command` gives, with `HTTPS_PROXY`, `HTTP_PROXY`, `ALL_PROXY` and `NO_PROXY` and their lower-case forms `https_proxy`, `http_proxy`, `all_proxy` and `no_proxy` kept among them, and that refuses any push or fetch to an `https` URL of that remote, so no network host is asked), 7, 8, 9 (a message to a name that both machines have, which the first machine delivered and acknowledged before the `--take-host`, is not delivered on the second: not in its `new/`, not in its imported log, and `0 delivered` in the `--take-host` run and in the pass after it), 10, 11 (the stop, its `errors.log` line, `last_error` and the `zygo doctor` line, then `--new-identity`), 12 (the six entries and the two registries: the member's `new/` holds the same ULIDs after the pass; every path of the home that the pass adds, changes or removes, object stores left out, is in that member's maildir, in `mail/sync/`, `errors.log` (an append of exactly the eight lines) or one of the git files that #12 names), 13 (each pass with a deadline of an hour through the S-C9 seam `sync::run_with_deadline`, so the cap of 500 decides each count and the speed of the machine does not), 14 (a crash after each step of S§8.5 "Delivery", then the recovery through the next pass: the message is in `new/` once and in the imported log once, `incoming/` is empty, the recovery pass counts the message once (`delivered` 1) and wakes the idle `bob` with a live launcher once, through the multiplexer stub, and the pass after delivers nothing; FX-30), 15 (a `--prune` pass while the inbox is full makes a new root that keeps the message not acknowledged and drops the acknowledged one; after `bob` reads, the next pass delivers it), 16, 17, 18, 19, 20 (with `ws forget` answered no, and answered yes on another machine), 21, 22 (the rate limit through the hook path, a send with no gap, a sync child that does not start after a CLI send, an MCP send, a `zygo mail watch` step and a hook), 23 (a wake through the multiplexer stub, once within `wake_gap`; none for a busy member and its `Stop` block; a wake that fails: a `wake_gap` that does not parse, a `wake.json` that cannot be written, a pane that takes no text), 25, 26, 27 and 28. After them, no subject, body, address, host name or member name appears in `errors.log`, `session.log`, the usage store, a commit message or a ref name. `workspace.json` holds no subject, body, address or host name, and no member name other than its own members' names (S-C2). `mail/sync/state.json` holds no subject, body, address or member name, and a host name only inside the own file paths `registry/<host>.json` and `ack/<host>.json` that S§7.5 records (the keys of `pushed` and `pending`), each for a host of this workspace. In each machine's scratch home, a file that holds one of these is in the S-C2 set: the index of the sync worktree, the maildir, the outbox with `refused/`, `incoming/`, `members.json`, the config files, `workspace.json`, `state.json` and the top of the results store. The check reads every file of the home except a git object store (a folder `objects` in `.git/` or in `<name>.git/`, whose commits and tags the commit check reads), and fails on any folder or file that it cannot read, with the path of that folder or file; every other read of the scan fails in the same way: `errors.log` (also when it is not UTF-8; a missing one only where the walk expects none), `workspace.json`, `state.json`, the session logs and the list of `sess-*` folders, the usage store and its `-wal`, the sync files and the linked-worktree admin files. It also checks the name of every file, folder and link in the home and the target of every link (`sweep_machine`; `sweep_remote` for the folder of a bare repository): a name or a link target that holds one is a finding anywhere, also inside the sync worktree folder. This is stricter than S-C2, which allows the paths of the synced tree there: S§6.3 makes the sync worktree with `--no-checkout` and zygo never checks a file out (SP3), so production writes no entry in that folder other than its `.git` file. It also fails when it did not read the files that the walk knows exist, or no file in each folder kind that the walk knows holds one: the maildir of each member that got mail, the outbox or `refused/` with a message in it, each `sess-*` folder, and `results/`. In the folder of each bare repository, only the #25 `seed/.zygo.toml` of the seed clone holds one, no file of the bare repository does, and no name or link target does. Each commit that zygo writes has the message `zygo mail [skip ci]` and the author and committer `zygo <zygo@zygo.invalid>` (S§6.5). One part is not walked: the SP2 premise (S§20), what a real `https` host (github.com, gitlab.com) answers to the anonymous probe, which only a live network host can check.
25. **Config layers.** `[mail]` in `.zygo.toml` is ignored with the existing line; in team config it is dropped with one warning.
26. **Forget.** `zygo ws forget` on A: `git worktree list` in A's checkout no longer lists the sync worktree, and the other worktrees are unchanged.
27. **Agent text.** After a sync on A that sees `b`'s registry, the session text of a member on A lists `bob@b` in the item 8 clause, and `mail_list` returns it.
28. **Terminal escape.** `zygo mail watch` prints a remote subject that holds `\x1b[2J` as escaped text.

## 20. Premises the first build verifies

- **SP1** A child that a member hook starts with `setsid` (as `start_snapshot_child`) outlives the hook in Claude Code 2.1.328 and in OpenCode 1.19.6, and its `git` calls run with the user's SSH agent and credential helper.
- **SP2** With no credentials (S§8.2 step 3), `git ls-remote --heads` over HTTPS succeeds for a public repository and fails for a private one on github.com and gitlab.com, and the private failure text matches the builder's list.
- **SP3** `git worktree add --force --detach --no-checkout` plus the index-only steps of S§6.3 write no file in the worktree folder other than its `.git` file, and change nothing in another checkout.
- **SP4** `commit-tree` with `-c commit.gpgSign=false` does not sign or prompt when the user's config has `commit.gpgSign = true`.
- **SP5** A user's plain `git fetch` (default refspec) after a prune updates `refs/remotes/<remote>/<branch>` with no error, and `git pull` on a work branch is not affected.
- **SP6** `-c gc.auto=0 -c maintenance.auto=false` on `fetch`, `commit-tree` and `push` stops each automatic `gc` and `maintenance run --auto` in the pinned git version (test 2 shows it).

## Change log

- 2026-11-24 · S-C4 · author's call (MS-2 review) · fetch.writeCommitGraph=true in user config made a sync fetch write commit-graph files, against S-G2; the override list gains -c fetch.writeCommitGraph=false.
- 2026-11-25 · S§8.3 · author's call · load/save disagree on a bad entry: a state.json with one value outside the rules loaded, and then every later save failed, so the pass could never write its state again; the read now refuses such a file as it refuses a non-object.
- 2026-11-25 · S§8.5 · author's call · a file staged by a crash during the write is deleted and imported again, not delivered.
- 2026-11-25 · S§8.5 · author's call · reading a folder stops at its target's inbox room, so one full inbox does not stop import for the others.
- 2026-11-25 · S§8.4 · author's call · a last_start later than now no longer stops hook starts until the clock passes it (MS-16 review F2).
- 2026-11-25 · S§8.3 · author's call (lead) · MS-13 review F2: an outbox file that cannot be read stayed in the outbox and took a place in the 1000 of each pass, so 1000 such files stopped every later message; it now moves to `mail/outbox/refused/` with one `read` line and takes no place.
- 2026-11-25 · S§9 · author's call (lead) · a prune failure other than a lost lease skips the prune and does not fail the pass (MS-14 review F1).
- 2026-11-25 · S§10.4, S§17 · author's call (lead) · Clause form fixed to the M§8a text of MS-20; each address once; clause code in mail.rs (ML-9 put item 8 there).
- 2026-11-26 · S§8.3 step 11 · author's call (lead) · MS-15 review F1: a sender with a stale member cache after a rename pushed mail to the old host's address folder at the tip, where no machine reads or deletes it (S-G5); a file whose `to` host has no registry at the fetched tip now moves to `mail/outbox/refused/` with one `no-host` line and a notice, and takes no place in the 1000; the tip check runs first, so a file pushed before a crash and before the rename is deleted from the outbox, not refused with a wrong notice. No-host review: a valid ack also keeps the host (an old host stays an own host while its ack is on the branch, S§19 #19); only a valid registry counts; the pass deadline stops these refusals; a no-host file takes no place in the 1000, at the tip or not; the check uses the tip of the attempt, and a refusal stays after a rejected push; S§13 item 10 added (a host name taken again).
- 2026-11-26 · S-C1, S§16 · author's call (lead) · M-C6 restated to follow workspace definition design R§14 (WD-18).
- 2026-11-26 · S§7.6 Rename · author's call (lead) · MS-15 review round 2 F1: with a new registry over 256 members the old registry stayed valid while the old ack was deleted, so mail sent to the old addresses was never imported, refused or pruned (S-G5); the old ack now goes only in the same commit as the old registry, or when there is no old registry of this identity.
- 2026-11-26 · S§7.6 · author's call (lead) · MS-15 review round 3 R2-1: --take-host in a twin state pushed the shared identity's registry, stopped the other machine and blocked --new-identity with a collision; operator commands now run the S§8.3 step 7 twin check before any write.
- 2026-11-26 · S§6.6 · author's call (lead) · MS-17 plan: `ci_note_shown` (`state.rs`) had no writer, and only the pass under the lock may write `state.json` (S§15); a child sync, which prints nothing, would have used the once-only note up before any `zygo mail sync`; the note now belongs to the first `zygo mail sync` whose first pass ends, and that pass records it.
- 2026-11-26 · S§6.6 · author's call (lead) · MS-17 review round 1 (F2): a sync whose first pass ended and whose later pass stopped recorded the note and did not print it; the note now belongs to the first `zygo mail sync` that ends without a stop, its pass that finds no mark records it, and a later stop sets it back.
- 2026-11-26 · S§8.2 · author's call (lead) · MS-17 plan: `run_with` (`sync.rs`) returns the notes only in the `Report` of a sync that is not stopped, and `SyncError::Stopped` carries one line; the `unknown` line is now printed at the end of a sync that is not stopped, and a stopped sync's next sync prints it again from the cache or a new check.
- 2026-11-26 · S§6.6 · author's call (lead) · MS-17 review round 2: note paragraph states each multi-pass case as built; the lock-error case keeps the note recorded (R1-A recorded).
- 2026-11-26 · S§7.2 · author's call (lead) · MS-19 plan: a send runs no git (S§7.3) and an older own host is known only at the tip, so at send the local host is the host in force and `<name>@<older host>` meets the member cache rule; the two refusal lines of a `to` with `@` that is not an address are fixed, so the tool and the CLI print the same text.
- 2026-11-26 · S§10.4 · author's call (lead) · MS-19 plan: the `mail_list` sentence named no key and no sync-off case; `mail_list` returns `remote`, each address once, empty with sync off or a config that does not load, as the item 8 clause.
- 2026-11-26 · S§7.2 · author's call (lead) · MS-18 plan: the read, ls and watch refusal of a value with `@` had no line and no rule for sync off; one fixed line now covers every value with `@` that is not a local address, and the CLI sender rule (`ZYGO_PANE`, else `cli`) is stated here, as S§7.2 refusal 1 uses it.
- 2026-11-26 · S§8.1 · author's call (lead) · MS-18 plan: the stdout of `zygo mail watch` carries message lines for a bounded tool; a `Mode::Cli` sync would print its lines and the once-only S§6.6 note there (MS-17 interface note) and wait 30 s for the lock; the watch sync runs as the child does, before a look, start to start.
- 2026-11-26 · S§7.5 · author's call (lead) · MS-18 plan: `zygo doctor` runs no fetch, so "while the condition holds" had no test; the condition is now the recorded result of the last pass (`last_error` `twin`, cleared by a pass that ends without a stop, `sync.rs`) for the workspace of the session or folder.
- 2026-11-26 · S§10.3 · author's call (lead) · MS-18 plan: the `zygo mail ls` sync line had no fixed form and no off cases; the line, its values and each off case are fixed (S§7.1 line with no host; no line with sync not set, so a workspace without sync lists as before).
- 2026-11-26 · S§10.5 · author's call (lead) · MS-18 plan: `zygo mail read` also prints a remote `from` and `subject`, and the body; it escapes each (ML-12, `read_text`), so the section names it.
- 2026-11-26 · S§8.1 · author's call (lead) · MS-18 review F1/F3: the watch ran the sync in its own process between looks, so a hanging fetch held the look loop up to the pass deadline (120 s) per pass, against M§13 #7 and `--timeout`, and a panic printed its text and exited 101; the watch now starts the sync child at its start and each `sync_gap`, does not wait for it, and imported mail shows at the next look.
- 2026-11-26 · S§8.1 · author's call (lead) · MS-18 review round 2 F1: per-workspace rate limit. The watch kept its own last start, so watches started again or in several panes started children far more often than the S§8.4 rate limit intends; the watch now asks for the child under the S§8.4 rule (`last_start`, `sync_gap`), shared with the hooks, at its start and before each look.
- 2026-11-26 · S§8.4 · author's call (lead) · names the watch as a rate-limited starter: S§8.1 row 3 asks for the child under this rule, so the rule, the `last_start` read and write, and the two-reader case each name a hook or a watch.
- 2026-11-26 · S§6.3 · author's call (lead) · MS-21 plan: the forget's cache removal deletes the worktree folder, so the removal must come first, and it must not need git when there is nothing to remove; Remove now names its conditions (worktree folder exists, main worktree is a folder), its deadline and its failure, and why a stale `prunable` entry is harmless (`worktree add --force` at the same path; git 2.56.1).
- 2026-11-26 · S§7.6 · author's call (lead) · MS-21 plan: the forget bullet named only the question; it now gives the step order after the W§12 checks, the sync lock, `--yes`, the "no" answer (exit 0, nothing deleted), the unreadable-outbox and removal refusals, and the state that cannot be read.
- 2026-11-26 · S§14 · author's call (lead) · MS-21 plan: the row put the question before the W§12 checks; the checks come first (no question for a workspace that runs), and the worktree goes before the cache.
- 2026-11-26 · S§15 · author's call (lead) · MS-21 plan: `zygo ws forget` removes the sync worktree under `mail/sync/lock`, so the row names it.
- 2026-11-26 · S§8.3 step 3 · author's call (lead) · MS-21 review F1: a `zygo mail sync` that passed its gate and waited for the lock of a forget ran a pass after the forget and made the identity, `mail/sync/` and the sync worktree again; each lock taken (every pass, every mode, and the operator commands) now reads the workspace record again first, and a forgotten workspace writes nothing (child: exit 0, no line; CLI: exit 1, `zygo: the workspace was forgotten; nothing synced`).
- 2026-11-26 · S§7.6 step 1 · author's call (lead) · MS-21 review F1: with no `mail/sync/` the forget took no lock, so a first sync could start during the removal; the forget now makes `mail/`, `mail/sync/` and the lock file (no link followed) and holds the lock.
- 2026-11-26 · S§7.6 step 1 · author's call (lead) · MS-21 review F3: a `mail/` link made the forget take its lock through the link; `mail/` or `mail/sync/` that is a link or not a folder is now no mail, and nothing is made through it.
- 2026-11-26 · S§6.3 · author's call (lead) · MS-21 review F2: a link at `<cache>/<id>/mail-sync` or `<cache>/<id>` made `git worktree remove --force` remove the other worktree of the repository that the link named; Remove now removes only such a link and runs no git.
- 2026-11-26 · S§7.6 step 2 · author's call (lead) · MS-21 review F3: a `mail/` link or file made the outbox count refuse the forget; such a `mail/` is now not followed and counts 0, and the forget goes on as before MS-21.
- 2026-11-26 · S§6.3, S§7.6 step 3 · author's call (lead) · MS-21 review F4: a worktree folder that git could not remove (not a working tree, admin folder gone, a regular file, `main_root` no repository) refused the forget for good; Remove now removes it with the filesystem, as the sync's own recovery, inside `<cache>/<id>/` with no link followed, and refuses only when that removal fails, naming the folder.
- 2026-11-26 · S§7.6 step 2, S§14, S§16 · author's call (lead) · MS-21 review F5: refused messages in `mail/outbox/refused/` were deleted with no question; the forget now counts them too, the question gives both counts (`zygo: <n> unsent messages and <r> refused messages will be deleted. Forget the workspace? [y/N]`), and only an empty outbox with no refused message asks nothing.
- 2026-11-26 · S§8.3 steps 2-3 · author's call (lead) · MS-21 review R2-1: a sync that passed its gate before a forget made `<workspaces_dir>/<id>/mail/sync/{again,lock}` again through a folder make that made parents; the sync now makes `mail/` and its folders only in a folder that exists, and a missing state folder at the mark or the lock is a forgotten workspace (child: exit 0, no line; CLI: the forgotten line), with nothing written.
- 2026-11-26 · S§8.3 step 3 · author's call (lead) · MS-21 review R2-2: the check under the lock found only that a record of this id existed, so a new workspace of the same id written during the wait passed, and a waiting CLI could lock the unlinked old lock file while another sync locked the new one; the check now stops as forgotten when the record is not the one of step 1 (id, name, common folder, main worktree, and on unix the state folder) or (unix) the locked file is not the file at `mail/sync/lock`. A record change that keeps the workspace does not stop the sync.
- 2026-11-26 · S§8.3 step 3 · author's call (lead) · MS-21 review R2-3: the check under the lock took every record read error for a forget; a record that cannot be read there (for example a newer `version`) is now the step 1 record stop (check `workspace`), and only no record or another workspace's record is a forget.
- 2026-11-26 · S§6.3 · author's call (lead) · MS-21 review R2-4: after a failed `git worktree remove`, the filesystem removal used what it read before git ran, so a link put at `<cache>/<id>` during git made it remove the folder the link named; Remove now reads `<cache>/<id>` and `mail-sync` again right before that removal and removes a link there as a link only.
- 2026-11-26 · S-C2 · author's call (lead) · MS-22 plan: the workspace record stores the M§6 names of its own members (`members.<key>.name`, `panes[].name`; `crates/zygo-workspace/src/state.rs:91-93`, `:128-130`), so "no member name in `workspace.json`" could not hold; the rule now names that one exception and keeps every other mail value, and every name of another machine's member, out of the file.
- 2026-11-26 · S§19 #24 · author's call (lead) · MS-22 plan: S§7.5 records the own file paths `registry/<host>.json` and `ack/<host>.json` in `state.json` (`pushed`, `pending`; `crates/zygo-mail-sync/src/state.rs:234-237`, `sync.rs:492-498`, `:774`), so the own host name is in that file by design, and `workspace.json` holds the own member names (S-C2); the test states both exceptions; "each commit message" now means each commit zygo writes, as a test that plays a hostile remote writes its own commits.
- 2026-11-26 · S§7.5 Twin · author's call (lead) · WD/MS-22 review F1: stale host names kept with no limit; `pushed` and `pending` kept the paths of every host the workspace ever had (only added, never removed), so `state.json` kept old host names after a rename or a take-host; the state write that ends a pass, and the one after the push of `--take-host` or `--drop-host`, now removes the path of each host that is not an own host at that tip, unless a file of this identity is still at that path.
- 2026-11-26 · S§19 #24 · author's call (lead) · MS-22 review F2: "after tests 1 to 23" named scenarios that the test does not walk; the test now states the walk it runs (tests 1, 3, 4, 5, 6 `public`, 9, 11, 12 with the registry half, 15, 19, 20 with `ws forget` no and yes, 22 and 23 with a failing child start and a failing wake), and the scenarios not walked are left to their own tests.
- 2026-11-26 · S-C2 · author's call (lead) · MS-22 review F3: the sync worktree and its git index hold the paths of the synced tree, which name hosts and members, by design, and S-C2 did not say so; it now states that exception, and the test scans that index for subject and body text only.
- 2026-11-26 · S§8.5 · author's call (FX-24 plan) · FX-13 P2 (d2, d3): the import staged and moved files into `mail/<key>/new/` for the keys of the step 1 record with no `mail/lock`, so a record removed during the pass got its maildir again; steps 1-3 and the finish of `incoming/` are now M§5 deliveries, taken per candidate so no git call runs under the lock; a key with no record is left on the branch (import) or its staged file deleted with the log unchanged (finish), as M§5 removes mail with its record.
- 2026-11-26 · S§8.6, S§8.3 step 11 · author's call (FX-24 plan) · FX-13 P2 (d1): the refused-message notice resolved the sender on the step 1 record and delivered with no `mail/lock`; it now resolves and delivers under the lock, and a notice it cannot send writes the `notice` line.
- 2026-11-26 · S§7.6, S§14, S§16 · author's call (FX-24 plan) · FX-13 P2 (d4): the forget renamed the state folder with no `mail/lock`, and an in-flight delivery made `<id>/mail/<key>/` again; step 4 now holds `mail/lock` exclusive across the rename, after the question and the worktree removal so neither runs under it.
- 2026-11-26 · S§8.5 · author's call · FX-24 review F1: a short record gap during up must not lose a staged message. Step 5 deleted a staged file whose key had no record, and a later pass then acknowledged its logged ULID with no delivery, while `zygo up` step 8 had the record out only until its undo write; step 1 now makes the maildir before it stages, step 5 deletes such a file only when the maildir is also gone and else leaves it for the next pass, and a logged ULID whose staged file is still in `incoming/` is not acknowledged until that file is delivered.
- 2026-11-26 · S§8.5, S§8.6, S§10.1 · author's call · FX-24 review F2: the import delivered to the pane key that the step 1 record gave a folder name, and both wakes used the step 1 record, so a name moved to another key during the pass went to the old key; the name now maps to its key on the read under `mail/lock`, and the wakes use the reads of the deliveries.
- 2026-11-26 · S§8.6, S§8.3 step 11 · author's call · FX-24 review F5: a busy lock must not lose the refusal notice. The refusal moved the message to `refused/` before the notice, so a notice that met a held `mail/lock` was never sent; the notice now comes first, a notice not delivered leaves the message in the outbox for the next pass, and a move that fails after a delivered notice gives a second notice on the next pass (the accepted cost).
- 2026-11-26 · S§8.5 · author's call · FX-24 review F6: every import delivery that could not begin wrote the line `lock`, so a forgotten workspace or a bad state read as a busy lock; the line now names the kind: `busy`, `no-workspace`, `state` or `io`.
- 2026-11-26 · S§7.6 step 4 · author's call · FX-24 review F7: the forget released `mail/lock` after `op.lock`, so a delivery that waited for it could read a new state that a `zygo up` of the same id wrote in between; it now releases `mail/lock` before `op.lock`.
- 2026-11-26 · S§8.6, S§8.3 step 11 · author's call · FX-24 review R2 F1: a lasting refusal of the notice kept the message in the outbox for ever. A sender inbox at `max_unread` refused the notice on every pass, so the message stayed queued and each pass logged it again; a notice that no pass can deliver (`full`, `no-record`) now moves the message to `refused/` with the line `notice: <reason>`, and only a notice that a later pass can deliver (a busy lock, an I/O error) keeps it.
- 2026-11-26 · S§8.5 · author's call · FX-24 review R2 F2: a link or an entry that is not a folder at `mail/` was logged as the kind `state`, because the kind was a guess from the error code; the delivery now gives the source of its error, and such a `mail/` is the new kind `mail`, apart from `state` (the state folder or `workspace.json`).
- 2026-11-26 · S§7.6 step 4 · author's call · FX-24 review R2 F3: the release of `mail/lock` before `op.lock` did not keep a waiting delivery from a new state: one scheduled late read the state after `op.lock` was released and a `zygo up` of the same id wrote a new one, and then wrote into it under the lock of the moved file. Every taker of `mail/lock` now checks that its locked file is still the file at `<state>/mail/lock` (device and inode, no link followed), as the sync does for `mail/sync/lock`, and else finds no workspace and writes nothing.
- 2026-11-26 · S§7.6 steps 1 and 4 · author's call · FX-24 review R2 F4: a link or a folder at `mail/lock` or `mail/sync/lock` made every `zygo ws forget` refuse, so the workspace could never be forgotten, while no delivery or sync can hold such a file; the forget now goes on without that lock and removes the state folder, and the sync refuses a lock file that is not a regular file (not only a link or a folder), so that holds for every such entry. Any other lock error still refuses.
- 2026-11-26 · S§8.6 · author's call · FX-24 review R2 F5: the notice resolved the sender's name only on the read under `mail/lock`, so after a rename the notice of `alice`'s refused message went to a new member now named `alice`, and woke it; the notice now goes to the sender only when the name maps to the same key there as in the record of step 1, and else to no one, with the line `notice: moved`, and the message moves to `refused/`.
- 2026-11-26 · S§8.6 · author's call · FX-24 review R3 F1: a sender that renamed itself after the pass gate and kept its key got no notice, because a name missing from the record of step 1 counted as `moved`; the target is now the name resolved in the read under `mail/lock`, `moved` only when both reads have the name and the keys differ, and `no-record` when that read does not have it.
- 2026-11-26 · S§8.6 · author's call · FX-24 review R3 F3: a write error in the sender's maildir (a link at `mail/<sender>/new`) kept the message in the outbox for ever and logged it on every pass; only an error of the delivery's begin now keeps it, and a write error moves it to `refused/` with `notice: maildir` or `notice: io`.
- 2026-11-26 · S§7.6 step 1 · author's call · FX-24 review R3 F4: the forget's `Taken::NotRegular` path rests on an unstated premise, that a lock file does not become non-regular while it is held; the step now states it and why it holds (no zygo path replaces a lock file; only a same-uid actor can, outside the threat model, M§5 R1-1). No code change.
- 2026-11-26 · S§7.4 Owner and Meaning, S§7.6 Rename and `--drop-host`, S§8.3 step 12, S§8.5 Candidates, S§9 Tree, S§13 limit 2 · author's call (FX-27 re-plan) · FX-27: an acknowledged link, submodule or tree in a renamed host's old folder kept `ack/<old>.json` for ever, as no prune removes it and the ack went only when no `*@<old>/` entry was left. The ack is keyed by ULID: import skips each ULID of the acks of all own hosts and each ULID it took in the pass, so an identity delivers each ULID at most once, and an entry in an own host's folder whose ULID it acknowledged can never be delivered, whatever its sender or kind. So: each own ack is (old ∪ acknowledged in this pass ∪ the own acks) ∩ the ULIDs in its folders (R1); the old ack goes when its folders hold no message file, its new list holds every ULID there, and its registry goes (R2); the prune is unchanged (R3); `--drop-host` counts only message files that the ack does not list (R4). S§9 now states MS-14 review F6: a message file is a blob of mode `100644` or `100755` (`prune::removable`). For the same reason, these earlier review findings are not defects: "a non-member file that shares an acked ULID is pruned without delivery" and "a ULID acked in another folder lets the ack list it" (such an entry can never be delivered), and "a non-member link that shares an acked ULID lets the old ack go" (a link is not a message).
- 2026-11-26 · S§7.4 Meaning, S§7.6 Rename and `--drop-host`, S§8.3 step 12, S§13 limit 2 · author's call
  (FX-27 re-plan review F3–F5) · FX-27 review round 1. F3: S§7.6 "Rename" named only links, submodules and trees
  as the entries that stay, but every reader (`sync::own_files`, `import::candidates`, `prune::run`,
  `ops::waiting`) looks only at canonical `<ULID>.json` names (`address::ulid_file`), so an entry with any other
  name is never delivered, pruned or counted and does not keep the old ack either; the text and S§13 limit 2 now
  state it, and `--drop-host` states what `ops::drop_host` does: it deletes each `*@<host>/` folder with every
  entry in it. F4: step 12 said "when its registry goes too", which did not cover an old host that has no registry
  of this identity at the tip; it now states S§7.6 (c) as the code does (`reg_stays`). F5: "Meaning" stated the ack
  only as "imported once" and the own-ack filter as the only guard; it now states what the list holds (also the
  ULIDs found in the imported log, and after `--take-host` the old owner's), when a ULID leaves it, and both guards
  against a second delivery: the own-ack filter of `import::candidates` and the imported log (`seen`) of
  `import::run`.
- 2026-11-26 · S§7.6 Rename and `--drop-host`, S§9 Tree, S§13 limit 2 · author's call (FX-27 re-plan review round 2
  F1–F2) · FX-27 review round 2. F1: every reader (`sync::own_files`, `prune::run`, `import::candidates`,
  `ops::host_folders`) skips a top-level folder whose name `address::parse` refuses (a reserved name such as
  `all@<host>/`, upper case such as `Bob@<host>/`), so "Rename" named two kinds of entries that stay where there are
  three, and `--drop-host` said it deletes each `*@<host>/` folder; the code stays (author's call: change the spec, not
  the code, as such a folder holds no mail a sender can address), and the text now states the third kind: it does not
  keep the old ack, no prune, `--take-host` or `--drop-host` touches it, and only the operator removes it, with git;
  `--drop-host` states the folders that `ops::host_folders` selects. F2: S§9 "Tree" said "the folder name is not
  checked", but the prune checks it with `address::parse`; it does not check the member name, and S§13 limit 2 says that
  a duplicate goes with the next prune only from a folder whose name is an address. Rename (a) now also covers folders
  that never held a message file, and (b) names the list the pass computes, as in the delete case the pass writes no
  ack.
- 2026-11-27 · S§19 #24 · author's call · FX-26: the walk named only part of S§19 (MS-22 "Not walked"); the
  first round (PR #233) added walks for the items of the MS-22 "Not walked" list, with the S-C9 seams and the
  multiplexer stub, and also scanned `session.log`; the production probe of a real https remote (`public::LsRemote`) asks a network host, so it is not
  walked and is left to SP2.
- 2026-11-27 · S§19 #24 · author's call (FX-26 review F1/F6) · FX-26 review round 1. The text said "stub seams only",
  but the walks also run the real `zygo` binary with its production clock, spawner, probe and wake. The text now names
  them. The #6 probe is now walked: `public::LsRemote` runs through the binary with a stub `git` on its `PATH`, for the
  `public` and the `private` verdicts, and the stub records the arguments and the environment of `probe_command`. A
  stub on `PATH` reaches the production code path without a network host, so "needs a network host" was true only of
  the answer of a real host. That answer (the SP2 premise) is now the only part of #6 not walked. The text also names the
  full-folder check of the scratch homes (FX-26 review F5).
- 2026-11-27 · S§19 #24 · author's call (FX-26 review round 2) · FX-26 review round 2. The #14 walk finished the
  staged delivery with a direct `import::finish_incoming` call, not the path that production runs; the crash seams
  now recover through the next pass (S§8.3 step 5). That pass drops the count of the finish and wakes no one for a
  recovered message, so #24 names that part as not walked and the defect as FX-30 (F-1, F-2). The #13 counts
  depended on the 120 s pass deadline of the wall clock; they now run with a far deadline through the S-C9 seam
  `sync::run_with_deadline` (F-3). The scratch-home check left out every folder named `objects`; it now leaves out
  only git object stores and fails when it read none of the files the walk knows exist, and the remote folder allows
  only `seed/.zygo.toml` (F-4, F-5). The #6 probe stub also records that a proxy variable stays (S§8.2 step 3) and
  refuses a push or fetch to the `https` URL (F-7, F-8).
- 2026-11-27 · S-C9 · author's call (FX-26): the pass deadline is a test seam. `sync::run_with_deadline` (FX-26 review
  round 2 F-3) was a seam that S-C9 did not name, and production code could call it; it is now in the seam list,
  behind the `test-hooks` feature (FX-26 review round 3, R3-5 and R3-6).
- 2026-11-27 · S§19 #24 · author's call (FX-26 re-plan) · FX-26 review round 3. The scratch-home check dropped each
  folder that it could not read, and its floor held files only; it now fails on any folder or file that it cannot
  read, and its floor holds each folder kind that the walk knows exists (R3-1). Only the #13 passes had the far
  deadline; every in-process pass now has it through the S-C9 seam (R3-11). The #6 stub's record kept `HTTPS_PROXY`
  only; it now keeps `HTTPS_PROXY`, `HTTP_PROXY`, `ALL_PROXY` and `NO_PROXY` (R3-12).
- 2026-11-27 · S§19 #24 · author's call (FX-26 re-plan review round 1) · The #6 stub's record now also keeps the
  lower-case `https_proxy`, `http_proxy`, `all_proxy` and `no_proxy` (F5). A file that the scratch-home check cannot
  read fails it with the path, and the other scans drop no walk error either (F6). The check also reads the name of
  every file, folder and link and every link target, and a marker there is a finding (F7). The binary runs keep the
  production limits (120 s per pass, 10 s for the probe); #24 now states that they apply and why they do not decide a
  result (F9). This is not a deadline test in the sense of the graph's global constraint "No wall-clock or deadline
  test runs in CI": no assertion is about a limit or a time.
- 2026-11-27 · S§19 #24 · author's call (FX-26 re-plan review round 2) · The #6 `private` run passed with an `unknown`
  verdict, which only adds a note, so a probe that failed or went past its limit could not fail it; #24 now gives its
  whole result (F1). The binary runs meet more limits than #24 named (the sync lock and the three waits of `ws
  forget`), and the `ws forget` runs leave unsent messages, so "an empty outbox" was not true; #24 now names each
  limit, the doctor probe among them, and why none decides a result (F1, F5). The name check now allows a marked name
  inside the sync worktree folder (not its `.git`) below a top folder `<name>@<host>` when the path holds no subject or
  body, and at `registry/<host>.json` and `ack/<host>.json` with a valid host; every other marked name stays a finding
  (F3). Every read of the scan now fails with its path (F4).
- 2026-11-27 · S§19 #24 · author's call (FX-26 re-plan 2) · FX-26 re-plan review round 3 (F1, F2, F4). The name
  check allowed a marked name inside the sync worktree folder below a top folder `<name>@<host>` and at
  `registry/<host>.json` and `ack/<host>.json`; planted cases did not test each boundary, and the rule allowed more
  than S§6.1 gives. S§6.3 makes the sync worktree with `--no-checkout` and zygo never checks a file out (SP3), so
  production writes no entry there, and an allowance only weakens the check: `sweep_machine` now reports a marked name
  or link target anywhere, which is stricter than S-C2. #24 also said that the SP2 premise was the only part not
  walked, while its #14 clause left the count and the wake of the recovered message to FX-30; it now names both.
- 2026-11-27 · S-C9, S§19 #24 · author's call (FX-26 re-plan 2 review round 1) · The #4 walk counted the files in the
  sender's `new/`, which already held another message, so it passed with no notice; it now asserts the one new file
  and its from, to, subject and body (F1). #5, #9 and #15 were walked only in part; #24 now names the walked result of
  each: no new file in the sender's `new/` for #5, a message acknowledged by the first machine and not delivered or
  counted on the second for #9, and a prune that keeps the unacknowledged message for #15 (F2). S-C9 said that only
  tests turn on the `test-hooks` feature; the operator's live-premise build (`scripts/premises/live-hud.sh`) does
  too, and S-C9 now says so (F5).
- 2026-11-27 · S§19 #12, S§19 #24 · author's call (FX-26 re-plan 2 review round 2) · The #12 walk asserted the
  `errors.log` lines, the ack and the member cache, but not "None is delivered" nor "no file is written outside
  `bob`'s maildir and `mail/sync/`" (F1). It now asserts both: bob's `new/` is the same after the pass, and every
  path of B's home that the pass adds, changes or removes (object stores left out) is in an allowed place. #12 said
  "no file outside" and also asks for `errors.log` lines and an ack that the pass commits and pushes, so it now
  names the files that this needs: the `errors.log` lines, `refs/remotes/<remote>/<branch>` (S§6.5, S§8.3 step 15),
  the sync worktree's own `HEAD` and index (S§6.3), and the reflog that git keeps for each. #24 names the walked
  result.
- 2026-11-27 · S§19 #24 · author's call (FX-26 re-plan 2 review round 3) · The #6 walk through the binary ran the
  `public` and `private` verdicts only, so a CLI that dropped the `unknown` note (`print_report` in `cmd/mail.rs`)
  passed it; #24 now names the `unknown` run (a stub answer that is not in `public::PRIVATE_TEXT`, exit 128) and its
  whole result: exit 0, the summary line as its only stdout, a stderr that is exactly the note of
  `sync::public_check`, and the probe and then a fetch as its network calls (F1). The first FX-26 change-log line
  said "every S§19 test"; it now says what that round did (F8).
- 2026-11-27 · S§7.6 step 4, S§8.1, S§14, S§15, S§16 · author's call · FX-28: a remote send that linked into `mail/outbox/` after the forget's count (step 2) and before its rename (step 5) was deleted with the state folder. It was in no count the operator saw, and its sender was told that it was sent. Rule (B): the send holds `mail/lock` shared across its outbox write (`outbox::send_remote_with`, through `Delivery::begin`, from before its first outbox folder until after its link); the forget counts again under `mail/lock` exclusive right before the rename (`forget_recount`) and refuses on a higher count. It does not ask again and has no override; step 4 states the cost. Rule (A), `mail/lock` across the question, was not taken: it reverses the step 4 rule that no question and no git call runs while deliveries wait.
- 2026-11-27 · S§8.1 · author's call · FX-28: row 1 names each `Delivery::begin` refusal of a remote send with its line, and what a refusal leaves (no state folder, no `mail/outbox/` and no outbox file; `mail/` and `mail/lock` can be made). A folder at `mail/lock` gives `<path>: is not a regular file` (the store open of a lock file looks at the path again after a failed open that is not `NotFound`, as `take_lock` does). The `zygo mail send` line of `workspace_of` is given as it is today (`no workspace with id <id>`, or `no workspace for this session or folder`): a `zygo: ` prefix on it is a change of the N42 parity of the CLI and slash replies, which FX-28 does not make.
- 2026-11-27 · S§8.1 · author's call (FX-28 step 10) · The two no-workspace lines of `workspace_of` (`zygo mail send`, `zygo mail ls`, `zygo mail read`, `zygo mail sync`, `/zygo:mail`) had no `zygo: ` prefix, unlike `zygo: no workspace with id <id>` of `zygo ws` and `zygo down`. The prefix is now in the line at its source, `zygo: no workspace with id <id>` and `zygo: no workspace for this session or folder`, not added by `finish`, so the slash form gives the same line and N42 holds. This replaces the earlier FX-28 line's statement that the prefix is not made.
- 2026-11-27 · S§7.6 step 4 · author's call (FX-28 review F4) · When `mail/lock` is not a regular file the forget goes on with no lock, so a holder that opened the regular file before the same user replaced it can still link into `mail/outbox/` after the count again, and that message is deleted uncounted. Step 4 now states this as the same-user tampering limit that FX-24 records (step 1, M§5 R1-1). No code change.
- 2026-11-27 · S§8.3 step 5, S§8.3 step 10, S§10.1, S§19 #24 · author's call (FX-30) · pending-wake record in the sync state: the import records each delivery (pane key, ULID) in `pending_wake` of `state.json` before its first write; the step 10 delivered set is step 5's moves, step 9's deliveries and the carried entries whose message is in `new/` or `cur/`, each ULID once; only a message in `new/` wakes its member. One state write right after the wake removes the entries that the pass counted and those neither arrived nor staged; an entry still staged in `incoming/` is kept; a pass that stops before the wake keeps every entry. A crash-recovered delivery (after `after_incoming`, `after_log`, `after_new` or `after_ack`) is now counted once and wakes its member. Recorded limit: a second count and wake only from a crash between the wake and that one state write. #24 walks the #14 count and wake.
- 2026-11-27 · S§18 · author's call (MS-25 review F2) · CI installs no git; the pinned version is the git: header of docs/premises.md, a minimum, and SP3-SP6 are run on it.
- 2026-11-28 · S§8.2 step 1, S§8.3 step 15, S§8.5 · author's call (FX-40) · the public check read only the first push URL while git push sends to every one (a public mirror got the branch), a push accepted by one URL and rejected by another took the first ref line, and the 500-candidate cap was counted per push attempt, so one pass could deliver 5 × 500; every push URL is checked, a push is accepted only when every URL accepts, and the cap is one set for the pass.
- 2026-11-28 · S§8.2 step 3, S§8.3 steps 11, 13 and 15, S§9 Push · author's call (FX-40 review F1-F4) · a push or a lease prune that one push URL took and another refused was read as "moved" or as a lost lease, so a pass ended without an error while a URL lacked the commit or the branch was rewritten at one URL only, a URL with no ref line was hidden, and each probe had its own 10 s past the pass deadline; the push URLs are counted, only an all-URL accept or all-URL "moved" is that result, any mix is the `push` stop and sets a split flag in state.json (while set: no outbox file deleted as pushed, no prune, the tip pushed to every URL until all take it), and each probe ends by the pass deadline.
