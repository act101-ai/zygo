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

- **S-C1** M-C1 to M-C6 apply. C6 applies to each new hook branch: a sync error never changes a hook decision.
- **S-C2** Mail is content (M-C2). No subject, body, address, host name, member name or path that holds one goes into a log, `workspace.json`, a usage row, a commit message or a ref name **(lead)**. A log line names ULIDs, pattern ids, check names, blob ids and counts only.
- **S-C3** zygo writes only `refs/heads/zygo/<x>` on the remote. A force push goes only to that branch, only with the lease of S§9 **(operator)**.
- **S-C4** Every git call is built by the hardened builder `git_command` (`crates/zygo-plugins/src/sync.rs:32-58`: no repository hooks, no fsmonitor, no LFS filter, no `ext::` transport, `GIT_TERMINAL_PROMPT=0`, `ssh -o BatchMode=yes`), plus `-c gc.auto=0 -c maintenance.auto=false` on each call **(lead)**. Each call runs under the runner of S§8.7, with a deadline.
- **S-C5** All content from the remote is untrusted: names, paths, modes, sizes, JSON and identities. zygo validates it before use, and never writes a file at a path that the remote chose (S§6.3).
- **S-C6** Only user config turns sync on (M§11, AU10, AU22). `.zygo.toml` and the team layer cannot.
- **S-C7** Trust boundary: each person with write access to the remote can send mail to, and wake, a member that syncs, and can delete or forge files on the side branch. Each person with read access can read all synced mail. The opt-in (S-G6) is the operator's consent to both.
- **S-C8** No machine deletes, or decides about, another host's files by its own clock or its own config. Another host's registry, acknowledgment and mail are removed only by that host's own acknowledgment (S§9) or by an explicit operator command (S§7.6) **(lead)**.
- **S-C9** Tests never use wall-clock timing (project rule) and never run a real `claude` or `opencode` (stub agents, `crates/zygo-launch/tests/stub_agent/`). The clock, the spawn of the sync child, the public probe and a hook between named steps are test seams.

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
- **Remove.** Only by path: `git worktree remove --force <path>`, at `zygo ws forget` (S§14).
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
- **Limit.** Webhooks fire for each push. A GitHub workflow on another event (for example `pull_request_target`), a GitLab policy that disables the skip, and a CI system that does not read the marker still run. The operator excludes `zygo/**` in such CI configs. `zygo mail sync` prints this once, at the first sync of a workspace.
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

A send to a remote address is refused:

- when the sender is a member with no name, or the CLI with no `ZYGO_PANE`: `zygo: remote mail needs a sender name; give this member a name in the loadout or with --name` **(lead)**;
- when sync is off: `zygo: mail sync is off for this workspace`;
- when the last public check said `public` (S§8.2);
- when the address is not in the member cache (S§7.3), with the known addresses (as M§10).

A member with no name has no address and gets no remote mail. `zygo mail read`, `zygo mail ls <name>` and `zygo mail watch <name>` take local members only; a local address works there as its bare name; a remote address is refused.

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

- **Owner.** The workspace whose identity is in it. A pass adds each ULID that it delivered (S§8.5) and removes each ULID whose file is no longer under `*@<host>/` at the tip (a prune removed it). Thus the list holds only messages that are still on the branch.
- **Meaning.** "This host imported this message; a prune may remove it." It is the only input that lets a prune remove a message (S§9).
- **Validation.** As the registry: mode, size at most 8 MiB, `version` 1, `host` equals the file name, a valid identity, each entry a valid ULID, at most 200 000 entries. An invalid ack counts as empty: the prune removes nothing for that host.
- **Size.** When this workspace's own ack would pass 100 000 entries, the pass runs a prune after its push (S§9), which takes the acknowledged files and so the entries off.

### 7.5 Identity, collision and the own hosts

**(author)** `mail/sync/identity` holds the identity, made at the first sync (random, `0600`). It is per workspace, not per machine: two clones of one repository on one machine are two workspaces (W§5), and each needs its own host.

**Own hosts.** The current host, plus each host whose `ack/<host>.json` at the tip holds this identity (an older host name of this workspace, S§7.6 "Rename"). The pass imports from the address folders of all own hosts.

**Collision.** The current host's `registry/<host>.json` or `ack/<host>.json` at the tip holds another identity. The pass stops (exit 1 for the CLI, one `errors.log` line for a child) with:

`zygo: the mail host name is taken on the side branch by another identity. If that is another machine, set another [mail] host. If it is this workspace from before (a moved repository, a ws forget, a restored backup), run zygo mail sync --take-host.`

When a local workspace state (another `<id>` under `workspaces/`) holds that identity, the line also names that workspace: the repository moved (W§5: a new common dir gives a new workspace id), and the old state still exists.

**Twin.** Two workspaces with one identity: a state folder that was copied (a backup restored on a second machine, a home folder moved while the first machine still runs). The pass records in `mail/sync/state.json` the blob id of each own registry and ack file that it last pushed, and the blob id that it is about to push (before the push). A tip whose own file holds this identity and has a blob id that is neither of those two was written by another workspace with this identity. The pass stops with `zygo: another workspace writes with this mail identity (copied state?); run zygo mail sync --new-identity here, and set another [mail] host` and imports nothing. `zygo doctor` prints the same line while the condition holds.

- **Limit.** A twin is found only at the first pass after the other copy writes one of these files. Until then, both copies can deliver the same message (S§13 limit 5).

### 7.6 Operator commands

Each command that can lose mail prints the count and asks; `--yes` answers in scripts (as §15.7) **(lead)**.

- **`zygo mail sync --take-host`.** For the collision line. It writes `registry/<host>.json` and `ack/<host>.json` with this identity. It keeps the ack's entries, so no message that the old owner acknowledged is imported again. No mail is lost; it does not ask. If the old owner still runs, its next pass stops with the collision line.
- **`zygo mail sync --new-identity`.** For a twin. It makes a new identity and clears the recorded blob ids. Then the pass runs as a first sync: with a new host (the old one belongs to the twin) it registers; with the same host it stops with the collision line. It deletes nothing.
- **Rename.** A changed `host` in config. The old registry file (this identity) is deleted at the next pass, so no new mail goes to the old addresses. The old `ack/<old>.json` stays, so the old host stays an own host: the pass still imports mail that waits under `*@<old>/`. When the tip has no `*@<old>/` file, the pass deletes `ack/<old>.json`. No mail is lost.
- **`zygo mail sync --drop-host <host>`.** For a host that will never sync again. It counts the files under `*@<host>/` that its ack does not list, prints `zygo: <n> messages wait for host <host>; they will be deleted. Delete them? [y/N]`, and on yes deletes `registry/<host>.json`, `ack/<host>.json` and each `*@<host>/` file in one commit. A dropped host that runs again registers again at its next pass; the deleted mail is gone. This is the only path by which a machine deletes another host's files without that host's ack (S-C8).
- **`zygo ws forget`.** When the outbox is not empty: `zygo: <n> unsent messages will be deleted. Forget the workspace? [y/N]`. It also deletes the imported log; S§7.5 "Collision" covers the next sync of a new workspace for the same repository.

## 8. Sync (P3-2)

### 8.1 Entry points

| Entry | Runs | Rate limit |
|---|---|---|
| `mail_send` and `zygo mail send` to a remote address | the sync child, after the outbox write | none (the `again` mark merges bursts, S§8.3) |
| Member hook events `session` (main loop), `prompt`, `stop`, `tool-end` (W§9.1, `crates/zygo-hooks/src/workspace.rs:36-37`) | the sync child | `[mail] sync_gap` (S§8.4) |
| `zygo mail watch` with sync on | a sync in its own process, at start and then each `sync_gap` until it returns | `sync_gap` |
| `zygo mail sync [--name <workspace>] [--prune] [--take-host] [--new-identity] [--drop-host <host>] [--yes]` | a sync in its own process, with output | none |

The hook starts the child as it starts the snapshot child today (`start_snapshot_child`, `crates/zygo-hooks/src/workspace.rs:484-513`): stdin, stdout and stderr to the null device, cwd `/`, a new session (`setsid`), no wait. A child that does not start gives one `errors.log` line. The hook prints nothing more and changes no decision (S-C1).

Reason for `tool-end` **(author)**: on a machine where one member is in a turn of one hour, no `stop` or `prompt` comes for an hour. Without `tool-end`, the idle members of that machine get no remote mail for that hour. The cost is one read of a timestamp per tool call.

### 8.2 Public check

**(operator)** zygo refuses sync on a remote that it can see is public. **(author)** "Can see" means: an anonymous read of the repository succeeds.

1. Get the fetch URL and the push URL of the remote (`git remote get-url` and `git remote get-url --push`). Check each distinct URL.
2. Map the URL to an HTTPS URL: `https://host/path` as is, without user info; `ssh://[user@]host[:port]/path` and the scp form `[user@]host:path` to `https://host/path`. A local path or `file://` URL is `local`. Any other scheme is `unknown`.
3. Run `git -c credential.helper= -c core.askPass= ls-remote --heads <https url>` with no credentials **(lead)**: `GIT_CONFIG_GLOBAL` and `GIT_CONFIG_SYSTEM` set to the null device, `GIT_CONFIG_NOSYSTEM=1`, `HOME` set to an empty temporary folder (no `.netrc`), `GIT_ASKPASS` and `SSH_ASKPASS` removed, `GIT_TERMINAL_PROMPT=0`, deadline 10 s. Proxy variables in the environment stay. The runner keeps the whole stderr up to 64 KiB (S§8.7).
4. Verdict: exit 0 is `public`. A failure whose stderr holds one of the builder's list of "authentication needed" or "repository not found" messages, matched without case, is `private`. Any other failure (network, TLS, timeout) is `unknown`.

- `public` for any URL: the pass stops with `zygo: the remote can be read without a login; mail sync refused` (exit 1). Nothing is fetched or pushed. A remote send is refused (S§7.2).
- `private` or `local`: the pass continues.
- `unknown`: the pass continues, and `zygo mail sync` prints `zygo: cannot check if the remote is public: <reason>`. Reason: an SSH-only server has no HTTPS form; a refusal would turn sync off for good on such servers, and the operator note refuses only what zygo can see.
- The verdict is cached in `mail/sync/state.json` with the SHA-256 of each URL without user info (W-C7: no URL is stored), for 1 h of the injected clock. A changed URL, or a cache older than 1 h, runs the check again.
- A URL is printed only by `zygo mail sync`, through `zygo_config::display::scrub_userinfo` (as `crates/zygo-plugins/src/sync.rs:95`). It never goes into a log.

### 8.3 The sync

One sync of one workspace. The lock rule closes the lost-wakeup race **(lead)**:

1. **Gate.** Sync is on for this workspace (S§11), the host is set and valid (S§7.1), the branch name is valid (S§11). Else exit 0 with no action (child), or exit 1 with the reason (CLI).
2. **Mark.** Write `mail/sync/again` (empty file).
3. **Lock.** Try an fs4 lock on `mail/sync/lock`. Not taken: exit 0 (child); the CLI waits for it up to 30 s, then exits 1. The holder sees the mark of step 2 (step 17).
4. **Clear.** Delete `mail/sync/again`. Then run one pass, steps 5 to 16, with a deadline of 120 s for that pass; each git call gets the time that is left.
5. **Finish incoming** (S§8.5, crash recovery). At the first pass of a sync, also the public check (S§8.2); `public` stops the sync here, before any fetch.
6. **Fetch** (S§6.5). No branch: bootstrap (S§6.4).
7. **Tip checks.** The tip tree has `ZYGO-MAIL` with the bytes `1\n`, else stop: `zygo: the side branch is not a zygo mail branch` (exit 1). Then the collision and twin checks (S§7.5).
8. **Member cache** (S§7.3).
9. **Import** (S§8.5).
10. **Wake** (S§10.1) the members that got mail in step 9.
11. **Outbox.** For each file in `mail/outbox/` in ULID order, at most 1000 per pass:
    - its path `<to>/<ULID>.json` is at the tip: it was pushed before (a crash after a push); delete it from the outbox;
    - else run the secret pass (S§8.6): clear, add the blob at that path; a finding, move it to `mail/outbox/refused/` and send the notice; could not run, leave it in the outbox.
12. **Own files.** Write the own registry (S§7.3) and the own acks (S§7.4) when they change; delete an old own registry or an empty old ack (S§7.6 "Rename").
13. **Commit.** No change: go to step 16. Else `read-tree` the tip, apply the adds and deletes, `write-tree`, `commit-tree` with the tip as parent (S§6.5). Record the blob ids of the own files that it holds as "about to push" (S§7.5).
14. **Push** `<commit>:refs/heads/<branch>` with `--porcelain`, no force.
15. **Result.**
    - Accepted: record the own blob ids as pushed; delete the pushed outbox files; set `refs/remotes/<remote>/<branch>` to the commit; this commit is now the tip of this pass.
    - Rejected because the branch moved (`fetch first`, `non-fast-forward`, matched without case in the porcelain line of the ref): go to step 6. At most 5 attempts per pass. After the fifth: exit 1, `zygo: the side branch moved 5 times during the sync; try again`. The outbox stays.
    - Any other error (authentication, network, a server rule): stop the pass, exit 1, one `errors.log` line with the check name and no URL. The outbox stays.
16. **Prune** when due (S§9), from the tip of this pass.
17. **Release and look again.** Release the lock. If `mail/sync/again` exists, go to step 3. Else exit.

Why no wakeup is lost: each starter writes the mark (step 2) before it tries the lock (step 3). A starter that fails the lock did so while a holder held it, so the holder's release (step 17) comes after the mark was written, and the holder's look after the release sees it. A starter that writes its mark after the holder's look takes the free lock itself.

The second and later passes have their own 120 s deadline. A sync runs passes while marks keep coming; it ends at the first look that finds no mark.

`mail/sync/state.json` records `last_ok`, `last_error` (check name only), the public-check cache, and the own blob ids, for `zygo mail ls`, S§8.2 and S§7.5.

Reasons **(author)**:

- Step 13 builds each attempt on the newest tip. It adds new message paths and changes only own files, so a rebuild never conflicts, and no `git merge` or `rebase` runs.
- The retry needs no wait: each attempt starts with a fetch of the newer tip.

### 8.4 Rate limit

**(author)** A hook starts the sync child only when `now - last_start` is `[mail] sync_gap` (default `60s`) or more. `last_start` is the Unix time in its own file, `mail/sync/last_start`; the hook reads it and writes the new value (atomic) before the spawn. A separate file keeps the hook and the sync from writing one file. Two hooks that read at the same time can both start a child; the lock and mark of S§8.3 make the second one a mark or a second pass. The clock is a parameter (S-C9).

### 8.5 Import (P3-4 receive side)

**(lead, author)** Each message is delivered once, by ULID. Two records say "delivered": the imported log (local, this workspace) and the own ack (on the branch).

**Candidates.** For each own host (S§7.5) and each local member with a name: stream `git ls-tree -z -l <tip> -- <name>@<host>/` (a literal path; the address form has no glob character). An entry is a candidate when its ULID is not in the own ack. A candidate whose ULID is already in the imported log (a sender added it again after a prune, S§14) is not delivered; it is only added to the own ack, so a prune removes it again. Reading stops at 500 candidates in the pass, or at the deadline; git is then killed (S§8.7). The rest waits for the next pass. Thus a large or hostile backlog still gives 500 deliveries per pass **(lead)**.

**Checks (S-C5).** A candidate is delivered only when:

- mode `100644`, type `blob` (no link `120000`, no submodule `160000`, no tree);
- the name matches `^[0-9A-HJKMNP-TV-Z]{26}\.json$` (Crockford ULID);
- size at most 8 MiB (a 1 MiB body can grow to 6 MiB with JSON escapes);
- the JSON is an object with `version` 1, `id` equal to the file's ULID, `to` equal to the folder name, `from` a valid address, `subject` at most 200 bytes, `body` at most 1 MiB, `sent_at` RFC 3339;
- the target's `new/` has fewer than `[mail] max_unread` files (M§5). A full inbox leaves the message on the branch, not acknowledged, for a later pass. It is not lost.

A candidate that fails a check other than the full inbox is not delivered. It is added to the imported log and the ack, so that it is not read again and a prune can remove it. One `errors.log` line names the ULID and the check. No time check is made: a message is never refused for its age (S-C8).

**Delivery, crash-safe.**

1. Write `mail/sync/incoming/<pane key>.<ULID>.json` (`0600`, the blob bytes), `fsync`.
2. Append `<ULID>` to the imported log, `fsync`.
3. Rename the incoming file to `mail/<pane key>/new/<ULID>.json` (M§5).
4. Add the ULID to the own ack (pushed in step 13 of S§8.3).

Step 5 of S§8.3 finishes each file left in `incoming/` by a crash: it appends the ULID to the log when it is not there, then renames the file into `new/`. `incoming/` is not swept. Thus a crash at any point gives one delivery: before step 2 the next pass completes it; after step 2 the log stops a second import.

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
  - Finding: the file moves to `mail/outbox/refused/`. One `errors.log` line with the ULID and the pattern ids. The sender gets a notice **(lead, author)**: a local M§5 message from `zygo` to the sending member, subject `not sent: <ULID>`, body `zygo did not send message <ULID> to <address>: it holds a secret (<ids>). The message is kept on this machine in mail/outbox/refused/.` The notice is delivered and woken by M§7 like any local mail. It is content in the maildir, not a log line. A message sent by the CLI as `cli` has no inbox; `zygo mail ls` shows the refused count.
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
- **Tree.** The base tree, less each message file `<name>@<host>/<ULID>.json` whose ULID is in the valid `ack/<host>.json` of the base tree. Nothing else is removed: no registry, no ack, no message that its host has not acknowledged. No clock and no `keep` is used (S-C8).
- **Commit.** `commit-tree` with no parent, message `zygo mail [skip ci]`.
- **Push.** `git push --porcelain --force-with-lease=refs/heads/<branch>:<base> <remote> <commit>:refs/heads/<branch>`. The lease is the base. A push from another machine after the base makes the lease fail; the prune is then skipped in this pass; the next pass tries again. Thus a prune never removes a message that it did not see in its base, and the messages of this pass's own push are in its base and are not acknowledged yet, so they stay.
- **Only its own branch.** The branch name must pass the `sync` form of S§11 (prefix `zygo/`), and the base must have `ZYGO-MAIL`. Both are checked again just before the force push.
- **After a prune**, other machines fetch with the `+` refspec (S§6.5); their next pass builds on the new root. Each owner removes the pruned ULIDs from its own ack (S§7.4). The old objects stay on the server and in local object stores until each one runs its own garbage collection.

Local retention: the M§5 sweep also deletes files in `mail/outbox/refused/` older than `[mail] keep`, and files in `mail/outbox/tmp/` older than 1 h (as M§10). It never touches `mail/sync/incoming/`.

## 10. Delivery on the receiving machine (P3-4)

### 10.1 Wake after import

**(author)** The sync process acts as the sender's process of M§7.3 for each member that got mail in step 9 of S§8.3: it wakes the member when it is idle by AU26 and its launcher is live (AU74), with the M§7.3 steps (the `@zygo-pane` and `@zygo-ws` checks, the fixed wake text, `wake_gap`). A busy member reads the mail at its `Stop` (M§7.1). A member whose launcher is not live gets the line at its next `SessionStart` or `UserPromptSubmit` (M§7.2).

### 10.2 Which process fetches

A fetch on machine B runs only in a sync on B (S§8.1). Thus remote mail reaches B when one of these happens on B:

1. a member of B sends mail to a remote address;
2. a member of B has a hook event `session`, `prompt`, `stop` or `tool-end`, and `sync_gap` has passed since the last sync started;
3. a `zygo mail watch` runs on B (a lead under a bounded tool, or a person);
4. a person runs `zygo mail sync` on B.

### 10.3 The limit, stated plainly

**(author)** When every member on B is idle, no watcher runs on B, and no person types on B, nothing on B fetches. Mail for B waits on the remote, with no upper bound, until one of the four events happens on B. It is kept: a prune does not remove mail that B has not acknowledged (S§9). zygo adds no daemon, timer, `cron` entry or tmux status command to close this gap (W-G6, S§12). A team that needs B to answer while it is idle keeps a `zygo mail watch` running on B (for example a lead under a bounded Monitor tool, M§8), or sends a person to B.

A host that is idle for months keeps its mail on the branch, and the branch grows by that mail (S§13 limit 2). `zygo mail ls` shows `last sync <time>` for this machine. The operator removes a host that will not return with `--drop-host` (S§7.6).

### 10.4 Agent text

M§8a item 8 gains one clause when sync is on and the member cache is not empty, after the member list: `Other machines: <address>, <address>, …; mail them by the full address.` At most 64 addresses, then `and <n> more; mail_list lists all`. `mail_list` returns the remote addresses from the member cache. The tool description of `mail_send` adds: "To a member on another machine, use `<name>@<host>`." No message text goes into this text (M§8a).

### 10.5 Output of remote values

A remote `from` or `subject` reaches a terminal in `zygo mail watch` and `zygo mail ls <name>`. Both print each such value through `zygo_core::esc` (C0, C1 and bidi controls escaped). Reason: remote text is untrusted (S-C5); a raw escape sequence could change the terminal.

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
2. **Idle host and unknown names.** Mail for a host that does not sync, or for a name that its host does not have, stays on the branch until the host imports it or the operator drops the host (S§7.6). The branch grows by that mail.
3. **Local copies.** The fetched blobs, including mail to other hosts, are in the repository's object store with the repository's own file modes. M-C4 (`0700`/`0600`) holds for `mail/`, not for the object store.
4. **Branch deleted on the remote.** The mail on it is gone. The next pass makes the branch again (S§6.4).
5. **Twin window.** Two copies of one workspace state can deliver one message on two machines until the first pass after one of them writes its registry or ack (S§7.5).
6. **Imported log.** 27 bytes per delivered message, kept while the workspace exists. It is read at each pass.
7. **Public flip.** A repository made public is found within 1 h of syncs (S§8.2).
8. **Forge CI and webhooks.** S§6.6.
9. **Large pushes.** A writer can push large files to the side branch, and each fetch downloads them, as for any branch. S§8.5 bounds only what zygo reads.

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
| `zygo ws forget` | Asks when the outbox is not empty (S§7.6), then `git worktree remove --force <path>`, then W§12 as today. |

## 15. Owners of state

| State | Owner (the only writer) |
|---|---|
| `refs/heads/<branch>` on the remote | each syncing workspace, by fast-forward push; a prune, by lease |
| `<name>@<host>/<ULID>.json` | added by the sender's pass; removed only by a prune after `ack/<host>.json` lists it, or by `--drop-host` |
| `registry/<host>.json`, `ack/<host>.json` | the workspace whose identity is in them; `--drop-host` |
| `ZYGO-MAIL` | the pass that makes the branch |
| Sync worktree, its index and `HEAD` | the pass that holds `mail/sync/lock` |
| `mail/outbox/` | `mail_send` and `zygo mail send` add; the pass under the lock deletes or moves to `refused/`; the sweep deletes old `refused/` and `tmp/` files |
| `mail/sync/incoming/`, `mail/sync/imported` | the pass under the lock |
| `mail/sync/state.json`, `mail/sync/members.json` | the pass under the lock |
| `mail/sync/last_start` | the hook path that starts a sync child (S§8.4) |
| `mail/sync/again` | each starter writes it; the lock holder deletes it |
| `mail/sync/identity` | the first sync; `--new-identity` |
| `mail/<pane key>/new/` | M§5 writers and the import (S§8.5) |

## 16. Base-spec changes

- **M§3** The non-goal "Mail between … machines" is now this spec. Mail between OS users and between workspaces stays a non-goal.
- **M§5** `from` and `to` can hold a full address (S§6.2). The file format and its version do not change.
- **M§6** Gains the address grammar (S§7.2) and the reserved name `zygo`.
- **M-C4** Holds for `mail/`. Synced mail also exists in the repository's object store and on the remote (S§13 limit 3, S-C7).
- **M-C6** Holds: no file of a checkout sends mail or names a local member. The side branch delivers mail only after the user-config opt-in (S-G6).
- **M§8, M§8a** `mail_send` takes addresses; `mail_list` returns remote addresses; item 8 gains one clause (S§10.4). `zygo mail sync` is a new verb; `zygo mail ls` shows a sync line and the outbox and refused counts; `zygo mail watch` syncs (S§8.1).
- **M§11** Gains `host`, `sync_gap` and `[mail.workspace.<name>]` (S§11).
- **W§7.1** Gains the cache path `$HOME/.cache/zygo/workspaces/<id>/mail-sync/` and the state paths `mail/outbox/` and `mail/sync/`.
- **W§12** `zygo ws forget` asks when the outbox is not empty and removes the sync worktree (S§7.6, S§14).

## 17. Files expected to change

P3-1 is not built. "The mail module" is the place where the P3-1 build puts the M§5 store.

| File | Change |
|---|---|
| The mail module | New parts: address grammar (S§7.2), outbox and the send-time pass (S§8.6), sync and passes (S§8.3), bootstrap (S§6.4), import (S§8.5), registry, ack and member cache (S§7.3, S§7.4), identity checks (S§7.5), operator commands (S§7.6), public check (S§8.2), prune (S§9). |
| `crates/zygo-plugins/src/sync.rs:32-58` | Reuse `git_command`, or move it to a crate that the mail module can use. Its behavior does not change. |
| `crates/zygo-work/src/repo.rs:61-75` | Make `run_bounded_out` reachable from the mail module (or copy it), and add the stream mode (S§8.7). |
| `crates/zygo-redact` | No change. Called from the mail module (S§8.6). |
| `crates/zygo-hooks/src/workspace.rs` | The rate-limited spawn of the sync child on `session`, `prompt`, `stop` and `tool-end` (S§8.1, S§8.4), on the pattern of `start_snapshot_child` (`:484-513`). |
| `crates/zygo-hooks/src/session_text.rs` | The item 8 clause (S§10.4). |
| `crates/zygo-mcp/src/tools/` (mail tools of P3-1) | Addresses, the send-time pass, remote addresses in `mail_list`, the description sentence. |
| `crates/zygo/src/cmd/` (mail command of P3-1) | `zygo mail sync` and its options, the hidden `--ws-id <id> --background` form (AU44), the sync in `watch`, the sync line in `ls`, `esc` for remote values. |
| `crates/zygo/src/cmd/mod.rs` | The results sweep is skipped for `mail sync --background`, as for `ws` (AU71): it runs on hook events. |
| `crates/zygo/src/cmd/doctor.rs` | The twin line (S§7.5). |
| `crates/zygo-workspace/src/admin.rs` | `ws forget`: the question, the worktree removal (S§7.6, S§14). |
| `crates/zygo-config/src/schema.rs`, `check.rs`, `layers.rs` | S§11 keys, checks, `mail` in `USER_ONLY`. |
| `docs/premises.md` | SP1 to SP6 (S§20). |

## 18. Left to the builder

- The list of git stderr messages that mean `private` (S§8.2 step 4) and "branch moved" (S§8.3 step 15), for the git version that the build pins, as lists in code with a test per message.
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
12. **Hostile tip**, each case in `bob@b/`: a link entry, a submodule entry, a 9 MiB blob, `id` not equal to the name, `to` not equal to the folder, bad JSON. None is delivered, no file is written outside `bob`'s maildir and `mail/sync/`, each is in `ack/b.json` after the pass, and `errors.log` has one line each with a ULID and a check name. A registry with 257 names, or with `host` not equal to its name, is not in the member cache.
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
24. **No content in logs.** After tests 1 to 23, no subject, body, address, host name or member name appears in `errors.log`, `workspace.json`, `mail/sync/state.json`, the usage store, a commit message or a ref name. Each commit message is `zygo mail [skip ci]`.
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
