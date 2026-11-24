# zygo: mail sync between machines — design

**Status:** proposed 2026-11-24
**Date:** 2026-11-24
**Release:** phase 3, track B (roadmap items P3-2, P3-3 and P3-4, `docs/roadmap/phase-3.md`)
**Base specs:** `docs/specs/2026-11-22-workspace-mail-design.md` (`M§n`; approved, not built yet), `docs/specs/2026-11-18-zygo-design.md` (`§n`), `docs/specs/2026-11-20-workspaces-design.md` (`W§n`, decisions `AUn`). The build follows `docs/specs/2026-11-24-decision-records-design.md`. `S§n` is a section of this document.

Each decision has a mark. **(operator)**: the operator made the decision (the roadmap's "P3-2 design notes", its item rows and its "Plan of work (operator, 2026-11-24)"). **(author)**: the author made it; the operator can change it before approval. Code references are `file:line` at commit `97d8767` (branch `phase-3`). P3-1 is not built. This spec extends the text of M§n, not code.

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
- **S-G2** No user branch, index, working tree or `HEAD` changes. zygo writes only its own branch.
- **S-G3** No daemon (W-G6). A sync runs only when a send, a hook, a watcher or a person starts it.
- **S-G4** No message that the secret pass (§16.1) cannot clear leaves the machine.
- **S-G5** Two machines that sync at the same time lose no message and never conflict.
- **S-G6** Sync is off until the operator turns it on for one workspace on one machine.

## 3. Non-goals

- A zygo server or relay; network delivery other than `git fetch` and `git push` **(operator)**.
- Mail in `main` or in git notes **(operator)**.
- A wake of a machine where no zygo process runs. S§10.3 states this limit.
- Proof of the sender. The `from` field is not authenticated (S§4, S-C7).
- `all` across machines. `all` stays the members of this machine (S§7.2).
- Native Windows (OP5).
- Moving work, worktrees, sessions or results between machines (roadmap "Not in phase 3").

## 4. Constraints

- **S-C1** M-C1 to M-C6 apply. C6 applies to each new hook branch: a sync error never changes a hook decision.
- **S-C2** Mail is content (M-C2). No subject, body or address list goes into a log, `workspace.json`, a usage row, a commit message or a git ref name. Logs name ULIDs, pattern ids and counts only.
- **S-C3** zygo writes only `refs/heads/zygo/<x>` on the remote. A force push goes only to that branch, only with a lease (S§9) **(operator)**.
- **S-C4** Every git call uses the hardened builder of `crates/zygo-plugins/src/sync.rs:32-58`: no repository hooks (`core.hooksPath` is the null device), no fsmonitor, no LFS filter, no `ext::` transport, no terminal prompt (`GIT_TERMINAL_PROMPT=0`), `ssh -o BatchMode=yes`. Each call has a deadline (`run_bounded`, `crates/zygo-plugins/src/sync.rs:62-98`).
- **S-C5** All content from the remote is untrusted: names, paths, modes, sizes, JSON and timestamps. zygo validates it before use, and never writes a file at a path that the remote chose (S§7.4).
- **S-C6** Only user config turns sync on (M§11, AU10, AU22). `.zygo.toml` and the team layer cannot.
- **S-C7** Trust boundary: each person with write access to the remote can send mail to, and wake, a member that syncs. Each person with read access can read all synced mail. The opt-in (S-G6) is the operator's consent to both.
- **S-C8** Tests never use wall-clock timing (project rule) and never run a real `claude` or `opencode` (stub agents, `crates/zygo-launch/tests/stub_agent/`). The clock, the spawn of the sync child, the public probe and the push are test seams.

## 5. Terms

| Term | Meaning |
|---|---|
| Side branch | The branch named by `sync` (S§11), default `zygo/mail`. One per repository remote. It shares no history with any other branch and is never merged **(operator)**. |
| Host | The name of one machine's workspace on the side branch. Form `^[a-z0-9][a-z0-9-]{0,31}$`. |
| Address | `<name>@<host>`. `<name>` is the M§6 member name. |
| Local address | An address whose host is this workspace's host. It means the same member as the bare `<name>`. |
| Identity | 32 random lower-case hex characters, made once per workspace. It proves that a registry file is this workspace's own. Not a secret. |
| Outbox | `mail/outbox/` in the workspace state: messages to remote addresses that wait for a push. |
| Tip | The commit of the side branch that the last fetch got. |
| Sync | One run of S§8. |
| Sync child | A detached `zygo mail sync --ws-id <id> --background` process, started by a send or a hook. |

## 6. The side branch

### 6.1 Layout

**(operator)** The files are the M§5 message files, one per message, named by ULID, under `<to>/<ULID>.json`. **(author)** The tree:

```
ZYGO-MAIL                        marker: the bytes "1\n"
registry/<host>.json             S§7.3; one file per host
<name>@<host>/<ULID>.json        one message to that address
```

- `ZYGO-MAIL` marks the branch as zygo's. A sync refuses a tip that does not have it (S§8 step 6). This stops a typo in `sync` from writing to a branch that is not a mail branch.
- A message file never changes after its first commit. Only the host of the address deletes it, after it imports it (S§8 step 8). Thus no two machines ever change one path, and no commit needs a merge.
- The branch holds messages that are not imported yet. Read state (`new/` to `cur/`) stays local **(operator)**.

### 6.2 Message file on the branch

The M§5 file, version 1, with no new field. Two values change form **(author)**: `to` is the full address of the target, and `from` is the full address of the sender (`<name>@<host>`, or `cli@<host>` for a CLI send). M§5 already keeps unknown fields (AU78). The importer writes the file bytes into the local maildir unchanged (S-G1).

### 6.3 Worktree

**(operator)** zygo works on the side branch in its own detached worktree under the workspace cache directory, never in a user checkout. **(author)** The path is `$HOME/.cache/zygo/workspaces/<id>/mail-sync/` (W§7.1 cache root). zygo makes it with `git worktree add --detach --no-checkout <path> <commit>`, from the main worktree (`main_root`, W§7.2).

- zygo uses only the worktree's own index and `HEAD`: `read-tree`, `update-index --add --cacheinfo` and `--remove`, `write-tree`, `commit-tree`, `update-ref --no-deref HEAD`. It never checks out files. Reason: a checkout would write paths that the remote chose; a hostile tree could hold a symbolic link where zygo later writes (S-C5).
- A missing worktree folder (the user cleared the cache) is made again after `git worktree prune`.
- The worktree is listed by `git worktree list` in each checkout of the repository, detached, at the tip. W§11 `branch:` resolution does not match it, because it has no branch.
- Commits: `commit-tree` with `-c user.name=zygo -c user.email=zygo@zygo.invalid -c commit.gpgSign=false`. The message is `zygo mail: +<added> -<removed>` (counts only, S-C2). The first commit of a new branch has no parent and holds only `ZYGO-MAIL` and the new files.

### 6.4 Refs

Fetch uses the refspec `+refs/heads/<branch>:refs/remotes/<remote>/<branch>`, the ref that the user's own default fetch also writes. zygo makes no other ref in the repository. The fetched objects go into the repository's object store (S§13, limit 3).

## 7. Addresses and the registry (P3-3)

### 7.1 Host name

**(author)** The host is, in order: `[mail.workspace.<name>] host`, then `[mail] host`, then the slug of the system host name. The slug takes the part before the first `.` and applies the AU2 slug rule (W§5), cut to 32 characters. An empty slug, or a value that fails the host form, turns sync off with `zygo: mail host <value> is not valid; set [mail] host` (config check error, and the sync line of `zygo mail ls`).

### 7.2 Address grammar

**(author)** A `to` value is one of:

- `<name>`: a member of this machine (M§6). A bare name never means a remote member. Reason: a bare name that is local on one machine and remote on another would reach a different member than the sender means.
- `<name>@<host>`: with a local host, the same as `<name>`; with another host, a remote member (S§8).
- `all`: the members of this machine, as M§5. Not sent to other hosts.

A member with no name (M§6) has no address and gets no remote mail. `zygo mail read`, `zygo mail ls <name>` and `zygo mail watch <name>` take local members only; a local address works there as its bare name, and a remote address is refused.

`cli` is reserved as a name (M§6). A send to `cli@<host>` is refused: `cli` has no inbox. A send to a remote address is refused when sync is off (`zygo: mail sync is off for this workspace`), when the last public check said `public` (S§8.2), or when the address is not in the member cache (S§7.3), with the list of known addresses (as M§10).

### 7.3 Registry

**(author)** Each host has one file, `registry/<host>.json`:

```json
{ "version": 1, "host": "studio", "identity": "<32 hex>", "members": ["alice", "bob"], "updated_at": "<RFC 3339>" }
```

- **Owner.** Only the workspace whose identity is in the file writes or deletes it. A sync writes its own file when the member names of this workspace differ from the file, when the file is absent, or when `updated_at` is older than half of `[mail] keep` (15 days by default). The last rule keeps a live host from looking stale.
- **Content.** Member names only. No pane key, path, session id or user name.
- **Collision.** A tip whose `registry/<host>.json` has another identity stops the sync: `zygo: mail host <host> belongs to another machine on <remote>/<branch>; set [mail] host, or run zygo mail sync --take-host if that machine is gone` (exit 1). Two machines that make the same file at the same time: one push wins; the other is rejected, fetches, and finds the collision. Thus a collision is always found before a message to it is imported.
- **`--take-host`.** The operator states that the old machine is gone (a reinstall, a new disk). The sync writes the file with this identity. If the old machine is alive, its next sync stops with the collision line.
- **Host rename.** A registry file with this identity and another host is deleted at the next sync. Messages that wait under the old host's addresses are not imported. They are removed by the age rule (S§9). Rename a host only when no mail waits for it.
- **Stale members and hosts.** A member that `ws forget-pane` deletes, or that loses its name, leaves the file at the next sync. A host whose `updated_at` is older than `[mail] keep` is stale: it is not in the member cache, a send to it is refused, and the prune removes its file and its folders (S§9).
- **Validation (S-C5).** A registry file is used only when: blob mode `100644`; size at most 64 KiB; valid JSON; `version` 1; `host` equals the file name and passes the host form; `identity` passes `^[0-9a-f]{32}$`; `members` has at most 256 names, each passes M§6. Any other file is skipped with one `errors.log` line that names the path.
- **Member cache.** After each fetch, the sync writes `mail/sync/members.json` (atomic, `0600`): the valid, not stale hosts other than this one, each with its names. `mail_list`, the address check of S§7.2 and the session text (S§10.4) read this file. They never run git.

### 7.4 Identity

**(author)** `mail/sync/identity` holds the identity, made at the first sync (random, `0600`). It is per workspace, not per machine. Reason: two clones of one repository on one machine are two workspaces (W§5); each needs its own host, and the collision rule tells the second one to set it. `zygo ws forget` deletes it with `mail/`.

## 8. Sync (P3-2)

### 8.1 Entry points

| Entry | Runs | Rate limit |
|---|---|---|
| `mail_send` and `zygo mail send` to a remote address | the sync child, after the outbox write | none (the lock merges bursts, S§8.3) |
| Member hook events `session` (main loop), `prompt`, `stop`, `tool-end` (W§9.1, `crates/zygo-hooks/src/workspace.rs:36-37`) | the sync child | `[mail] sync_gap` (S§8.4) |
| `zygo mail watch` with sync on | a sync in its own process, at start and then each `sync_gap` until it returns | `sync_gap` |
| `zygo mail sync [--name <workspace>] [--prune] [--take-host]` | a sync in its own process, with output | none |

The hook starts the child as it starts the snapshot child today (`start_snapshot_child`, `crates/zygo-hooks/src/workspace.rs:484-513`): stdin, stdout and stderr to the null device, cwd `/`, a new session (`setsid`), no wait. A child that does not start gives one `errors.log` line. The hook prints nothing more and changes no decision (S-C1).

Reason for `tool-end` **(author)**: on a machine where one member is in a turn of one hour, no `stop` or `prompt` comes for an hour. Without `tool-end`, the idle members of that machine get no remote mail for that hour. The cost is one read of a timestamp per tool call.

### 8.2 Public check

**(operator)** zygo refuses sync on a remote that it can see is public. **(author)** "Can see" means: an anonymous read of the repository succeeds.

1. Get the fetch URL and the push URL of the remote (`git remote get-url` and `git remote get-url --push`). Check each distinct URL.
2. Map the URL to an HTTPS URL: `https://host/path` as is, without user info; `ssh://[user@]host[:port]/path` and the scp form `[user@]host:path` to `https://host/path`. A local path or `file://` URL is `local`. Any other scheme is `unknown`.
3. Run `git ls-remote --heads <https url>` with no credentials: `GIT_CONFIG_GLOBAL` and `GIT_CONFIG_SYSTEM` set to the null device, `GIT_CONFIG_NOSYSTEM=1`, `HOME` set to an empty temporary folder (so no `.netrc` and no credential helper), `GIT_TERMINAL_PROMPT=0`, deadline 10 s. Proxy variables in the environment stay.
4. Verdict: exit 0 is `public`. A failure whose stderr says that authentication is needed or the repository is not found (the builder lists the messages of the git version that the first build pins) is `private`. Any other failure (network, TLS, timeout) is `unknown`.

- `public` for any URL: the sync stops with `zygo: <remote> can be read without a login; mail sync refused` (exit 1). Nothing is fetched or pushed. A remote send is refused (S§7.2).
- `private` or `local`: the sync continues.
- `unknown`: the sync continues, and `zygo mail sync` prints `zygo: cannot check if <remote> is public: <reason>`. Reason: an SSH-only server has no HTTPS form; a refusal would turn sync off for good on such servers, and the operator note refuses only what zygo can see.
- The verdict is cached in `mail/sync/state.json` with the SHA-256 of each URL without user info (W-C7: no URL is stored), for 1 h. A changed URL, or a cache older than 1 h, runs the check again. Thus a repository made public is found within 1 h of syncs.
- Output: each URL is printed through `zygo_config::display::scrub_userinfo` (as `crates/zygo-plugins/src/sync.rs:95`).

### 8.3 The run

One sync of one workspace. Deadline: 120 s for the whole run; each git call gets the time that is left.

1. **Gate.** Sync is on for this workspace (S§11), the host is valid (S§7.1), the branch name is valid (S§11). Else exit 0 with no action (child), or exit 1 with the reason (CLI).
2. **Lock.** Try an fs4 lock on `mail/sync/lock`. Held: the child and the hook path write `mail/sync/again` and exit 0; the CLI waits for the lock up to 30 s, then exits 1. Reason: two syncs in one worktree would write one index.
3. **Public check** (S§8.2).
4. **Worktree** (S§6.3): make it when missing.
5. **Fetch** the branch. "Couldn't find remote ref" means no branch: the tip is none.
6. **Tip checks.** The tip tree has `ZYGO-MAIL` with the bytes `1\n`, else stop: `zygo: <remote>/<branch> is not a zygo mail branch` (exit 1). The registry collision check (S§7.3).
7. **Member cache** (S§7.3).
8. **Import** (S§8.5). Collect the paths to delete: each file under this host's address folders that is now in the local maildir, and each file there that fails validation.
9. **Wake** (S§10.1) the members that got mail in step 8.
10. **Outbox.** For each file in `mail/outbox/` in ULID order (at most 1000 per run; the rest waits for the next run): run the secret pass (S§8.6). A file that it clears is added at `<to>/<ULID>.json`. A file that it refuses moves to `mail/outbox/refused/`.
11. **Registry** (S§7.3): add, update or delete this workspace's own files.
12. **Commit.** No change: go to step 15. Else `read-tree` the tip (or an empty index, plus `ZYGO-MAIL`, when there is no tip), apply the adds and deletes, `write-tree`, `commit-tree` with the tip as parent.
13. **Push** `<commit>:refs/heads/<branch>` with `--porcelain`. No force.
14. **Result.**
    - Accepted: delete the pushed outbox files, then update the ref `refs/remotes/<remote>/<branch>` to the commit.
    - Rejected because the branch moved (`fetch first`, `non-fast-forward`): go to step 5. At most 5 attempts. After the fifth: exit 1, `zygo: <remote>/<branch> moved 5 times during the sync; try again`. The outbox stays.
    - Any other error (authentication, network, a server rule): stop, exit 1, one `errors.log` line (the first stderr line, user info removed). The outbox stays.
15. **Prune** when due (S§9).
16. **Again.** If `mail/sync/again` exists: delete it and run steps 5 to 15 one more time. Then release the lock.

`mail/sync/state.json` records `last_ok`, `last_error` (fixed text, no content) and the public-check cache, for `zygo mail ls` and S§8.2.

Reasons **(author)**:

- Step 12 builds each attempt on the newest tip. It adds new paths and deletes own paths only, so a rebuild never conflicts, and no `git merge` or `rebase` runs.
- The retry needs no wait: each attempt starts with a fetch of the newer tip.
- Step 16 prevents a message that was written to the outbox while a sync ran (after its step 10) from waiting for an unrelated later sync.

### 8.4 Rate limit

**(author)** A hook starts the sync child only when `now - last_start` is `[mail] sync_gap` (default `60s`) or more. `last_start` is the Unix time in its own file, `mail/sync/last_start`; the hook reads it and writes the new value (atomic) before the spawn. A separate file keeps the hook and the sync from writing one file. Two hooks that read at the same time can both start a child; the lock of step 2 makes the second one an `again` mark. The clock is a parameter (S-C8).

### 8.5 Import (P3-4 receive side)

For each local member with a name: `git ls-tree -l <tip> -- <name>@<host>/` (a literal path; the address form has no glob character). The output is cut at 4 MiB per member (the cut mode of `crates/zygo-work/src/repo.rs:71-75`); entries past the cut wait for the next sync, after this sync deletes the ones it imported.

An entry is imported only when all of these hold (S-C5):

- mode `100644`, type `blob` (no link `120000`, no submodule `160000`, no tree);
- the name matches `^[0-9A-HJKMNP-TV-Z]{26}\.json$` (Crockford ULID);
- size at most 8 MiB (a 1 MiB body can grow to 6 MiB with JSON escapes);
- the JSON is an object with `version` 1, `id` equal to the file's ULID, `to` equal to the folder name, `from` a valid address, `subject` at most 200 bytes, `body` at most 1 MiB, `sent_at` RFC 3339;
- the ULID time is not more than 24 h after now (clock skew allowance), and not older than `[mail] keep`;
- the target's `new/` has fewer than `[mail] max_unread` files (M§5). A full inbox leaves the message on the branch for a later sync; this is not a failure;
- neither `new/<ULID>.json` nor `cur/<ULID>.json` exists in the target maildir. This is the duplicate check.

Delivery is the M§5 send: write `tmp/<ULID>.json` (`0600`, the blob bytes), `fsync`, rename into `new/`. A file that fails a check other than the full inbox is not delivered, is deleted from the branch (it is in this host's own folder), and gives one `errors.log` line with the path and the failed check. A folder for a name that this workspace does not have is left alone; the age rule removes it (S§9).

The age rule and the duplicate check together prevent a second delivery: a delivered file stays in `new/` or `cur/` until the M§5 sweep deletes it after `keep`, and by then its ULID is older than `keep` and the import refuses it. The one gap: a sender clock up to 24 h ahead, and a delete push that failed for `keep`. S§13 states it.

### 8.6 Secret pass

**(operator)** The secret pass (§16.1) runs on each message before a commit; a message that it cannot clear is refused. **(author)**

- **Pattern set.** The merged `[redaction]` patterns of the workspace's config, plus the act-lite store values of the main worktree, compiled with `zygo_redact::patterns::compile_with_store` (`crates/zygo-redact/src/patterns.rs:36`). Store values come from `zygo_redact::act_lite_store::read` (`crates/zygo-redact/src/act_lite_store.rs:62-64`). The values stay in the process memory (C3).
- **Run.** `zygo_redact::secret::redact` (`crates/zygo-redact/src/secret.rs:74-76`) on the whole message object, with a fresh session cache and the default deadline policy (`crates/zygo-redact/src/secret.rs:22-26`: 200 ms plus 50 ms per MiB).
- **Clear** means: `Ok` with `changed = false` (`crates/zygo-redact/src/secret.rs:37-42`). Any `redact` finding, a `Blocked` error, or a `Timeout` is a refusal. zygo never pushes a redacted copy: a copy with holes is not what the sender wrote, and the operator note says "refused".
- **At send.** `mail_send` and `zygo mail send` run the same pass before they write a remote message to the outbox. A refusal returns `zygo: message not sent: it holds a secret (<ids>)` or `zygo: message not sent: the secret check did not finish; send it again`. Reason: the sender learns at once; a refusal found only in a later sync child reaches no one.
- **At commit** (S§8.3 step 10). The pass runs again, because the patterns or the store can change between send and sync. A refusal moves the file to `mail/outbox/refused/` and writes one `errors.log` line: the ULID, the address and the pattern ids. `zygo mail ls` shows the refused count.
- Local mail (to this machine) gets no pass. It does not leave the machine; M§5 does not change.

## 9. Prune and retention

**(operator)** A prune rewrites only the side branch: a new orphan commit with the messages that are not imported, then a force push of that branch alone. This is the one force push that zygo does. **(author)**

- **When.** At step 15 of a sync, when `git rev-list --count <tip>` is 1000 or more, or when `zygo mail sync --prune` runs.
- **Tree.** The tip tree, less: each message file whose ULID time is older than `[mail] keep`; each registry file of a stale host (S§7.3); each address folder whose host has no registry file after that. `ZYGO-MAIL` stays.
- **Commit.** `commit-tree` with no parent, message `zygo mail: prune`.
- **Push.** `git push --force-with-lease=refs/heads/<branch>:<tip> <remote> <commit>:refs/heads/<branch>`. The lease is the tip that this sync fetched. A push from another machine after that fetch makes the lease fail. Then the prune is skipped (no retry in this run); the next sync tries again. Thus a prune never removes a message that another machine pushed.
- **Only its own branch.** The branch name must pass the `sync` form of S§11 (prefix `zygo/`), and the tip must have `ZYGO-MAIL` (S§8.3 step 6). Both are checked again just before the force push.
- **After a prune**, other machines fetch with the `+` refspec (S§6.4); their next sync builds on the new root. The old objects stay on the server and in local object stores until each one runs its own garbage collection.

Local retention: the M§5 sweep also deletes files in `mail/outbox/refused/` older than `keep`, and files in `mail/outbox/tmp/` older than 1 h (as M§10).

## 10. Delivery on the receiving machine (P3-4)

### 10.1 Wake after import

**(author)** The sync process acts as the sender's process of M§7.3 for each member that got mail in S§8.3 step 8: it wakes the member when it is idle by AU26 and its launcher is live (AU74), with the M§7.3 steps (the `@zygo-pane` and `@zygo-ws` checks, the fixed wake text, `wake_gap`). A busy member reads the mail at its `Stop` (M§7.1). A member whose launcher is not live gets the line at its next `SessionStart` or `UserPromptSubmit` (M§7.2).

### 10.2 Which process fetches

A fetch on machine B runs only in a sync on B (S§8.1). Thus remote mail reaches B when one of these happens on B:

1. a member of B sends mail to a remote address;
2. a member of B has a hook event `session`, `prompt`, `stop` or `tool-end`, and `sync_gap` has passed since the last sync started;
3. a `zygo mail watch` runs on B (a lead under a bounded tool, or a person);
4. a person runs `zygo mail sync` on B.

### 10.3 The limit, stated plainly

**(author)** When every member on B is idle, no watcher runs on B, and no person types on B, nothing on B fetches. Mail for B waits on the remote, with no upper bound, until one of the four events happens on B. zygo adds no daemon, timer, `cron` entry or tmux status command to close this gap (W-G6, S§12). A team that needs B to answer while it is idle keeps a `zygo mail watch` running on B (for example a lead under a bounded Monitor tool, M§8), or sends a person to B.

`zygo mail ls` shows `last sync <time>` on each machine, so a person can see a machine that does not sync.

### 10.4 Agent text

M§8a item 8 gains one clause when sync is on and the member cache is not empty, after the member list: `Other machines: <address>, <address>, …; mail them by the full address.` At most 64 addresses, then `and <n> more; mail_list lists all`. `mail_list` returns the remote addresses from the member cache. The tool descriptions of `mail_send` add: "To a member on another machine, use `<name>@<host>`." No message text goes into this text (M§8a).

### 10.5 Output of remote values

A remote `from` or `subject` reaches a terminal in `zygo mail watch` and `zygo mail ls <name>`. Both print each such value through `zygo_core::esc` (C0, C1 and bidi controls escaped). Reason: remote text is untrusted (S-C5); a raw escape sequence could change the terminal.

## 11. Configuration

User config only (M§11, AU10, AU22). The team layer drops `[mail]` with one warning, as the `USER_ONLY` tables (`crates/zygo-config/src/layers.rs:66-68`); add `mail` there if the P3-1 build did not. `.zygo.toml` drops it by its kept list (`crates/zygo-config/src/repo_file.rs:39`).

```toml
[mail]
host = "studio"          # S§7.1; default: slug of the system host name
sync_gap = "60s"         # S§8.4; <n>s | <n>m | <n>h

[mail.workspace.zygo]    # the workspace name (W§5, AU2)
sync = "zygo/mail"       # S-G6: absent = off; the side branch
remote = "origin"        # optional; default "origin"
host = "studio-2"        # optional; overrides [mail] host for this workspace
```

- **Per workspace (operator: "opt-in per workspace").** **(author)** The key `sync` sits in a table per workspace name, because the user config is shared by all workspaces of the user. Each machine opts in on its own, so each machine's operator consents (S-C7).
- **`sync`** must match `^zygo/[a-z0-9][a-z0-9-]{0,31}$`. Anything else is a config check error and sync stays off. Reason: the force push of S§9 must never reach a branch that is not zygo's (S-C3).
- **`remote`** must name a remote that `git remote` lists. Else the sync stops with `zygo: no remote <name>`.
- `zygo config check` validates the three forms; `zygo config show` prints them with their source.

## 12. Alternatives rejected

- **A zygo relay server** **(operator)**: adds a server; the repository remote already has the access control.
- **Mail in `main`** **(operator)**: fills history and races between worktrees.
- **Git notes** **(operator)**: poor tooling for many small files.
- **A checked-out worktree** **(author)**: a checkout writes paths that the remote chose; a symbolic link in the tree could redirect a later write (S-C5). The index-only worktree gives the same isolation with no file written.
- **`git merge` or `rebase` on a rejected push** **(author)**: the rebuild on the tip (S§8.3 step 12) never conflicts, because each path has one writer.
- **Push a redacted copy** **(author)**: the operator note says "refused"; a redacted copy changes the message with no word to the sender.
- **A periodic sync from the tmux status line (`#(...)` in `status-right`)** **(author)**: it is a timer by another name (W-G6), and it changes the user's status line.
- **A forge API check for "public"** (GitHub `GET /repos/...`) **(author)**: works for one forge only and sends the repository name to a third party in a new way. The anonymous `ls-remote` works for each HTTPS git server.
- **Refuse sync when the public check is `unknown`** **(author)**: an SSH-only server could never sync; the operator note refuses only a remote that zygo can see is public.
- **A seen-set file for the duplicate check** **(author)**: the maildir names and the age rule (S§8.5) give the same result with no new state.
- **Per-machine identity** **(author)**: two clones on one machine would share one host and flap the registry (S§7.4).
- **`all` across hosts** **(author)**: a broadcast to every machine of a repository is a larger change of reach than P3-2 asks for. A sender names each remote address.

## 13. Accepted limits

1. **No proof of the sender.** A person with write access to the remote can write any `from` (S-C7). Commit signing is not checked.
2. **Idle machine.** S§10.3.
3. **Local copies.** The fetched blobs, including mail to other hosts, are in the repository's object store with the repository's own file modes. M-C4 (`0700`/`0600`) holds for `mail/`, not for the object store. Any OS user who can read the repository's `.git` folder can read synced mail.
4. **Large pushes.** A writer of the remote can push large files to the side branch, and each fetch downloads them, as for any branch. S§8.5 bounds only what zygo reads and imports.
5. **Second delivery.** Possible only when a sender's clock is up to 24 h ahead and the delete push failed for `keep` (S§8.5).
6. **Lost outbox.** `zygo ws forget` deletes `mail/`, and with it unsent remote messages. It prints `zygo: <n> unsent messages deleted` when `n > 0`.
7. **Public flip.** A repository made public is found within 1 h of syncs (S§8.2).

## 14. Failure handling

| Case | Behavior |
|---|---|
| No network, authentication fails | Exit 1 (CLI) or one `errors.log` line (child). The outbox stays. The next sync tries again. |
| Push rejected 5 times | S§8.3 step 14. The outbox stays. |
| Crash after the push, before the outbox delete | The next sync adds the same bytes at the same path. Git sees no change, or the receiver already deleted the file; then it comes back once, and the duplicate check (S§8.5) skips it and deletes it again. |
| Crash after delivery, before the delete push | The next sync finds the file in `new/` or `cur/`, skips it and deletes it from the branch. |
| Crash during a prune | The lease push is atomic on the server: the branch is the old tip or the new root. |
| Lock held | `again` mark (S§8.3 step 2). |
| Not a mail branch, host collision, public remote, bad config | Sync stops with the line of its section. Remote sends are refused while it stays so. |
| Secret found at commit | S§8.6. |
| Sync child does not start | One `errors.log` line. The next trigger tries again. |
| Worktree folder deleted | `git worktree prune`, then make it again (S§6.3). |
| `zygo ws forget` | Runs `git worktree remove --force` on the sync worktree before it deletes the cache, so no stale worktree entry stays in the repository. Then S§13 limit 6. |

## 15. Owners of state

| State | Owner (the only writer) |
|---|---|
| `refs/heads/<branch>` on the remote | every syncing workspace, by fast-forward push; a prune, by lease |
| `<name>@<host>/<ULID>.json` on the branch | added by the sender's sync; deleted only by the sync of `<host>`, or by a prune |
| `registry/<host>.json` | the workspace whose identity is in it (S§7.3); a prune for a stale host |
| `ZYGO-MAIL` | the sync that makes the branch |
| Sync worktree, its index and `HEAD` | the sync that holds `mail/sync/lock` |
| `mail/outbox/` | `mail_send` and `zygo mail send` add; the sync under the lock deletes or moves to `refused/`; the sweep deletes old `refused/` and `tmp/` files |
| `mail/sync/state.json` | the sync |
| `mail/sync/last_start` | the hook path that starts a sync child (S§8.4) |
| `mail/sync/members.json` | the sync |
| `mail/sync/identity` | the first sync |
| `mail/<pane key>/new/` | M§5 writers and the import (S§8.5) |

## 16. Base-spec changes

- **M§3** The non-goal "Mail between … machines" is now this spec. Mail between OS users and between workspaces stays a non-goal.
- **M§5** `from` and `to` can hold a full address (S§6.2). The file format and its version do not change.
- **M§6** Gains the address grammar (S§7.2). `cli` stays reserved.
- **M-C4** Holds for `mail/`. Synced mail also exists in the repository's object store and on the remote (S§13 limit 3, S-C7).
- **M-C6** Holds: no file of a checkout sends mail or names a local member. The side branch delivers mail only after the user-config opt-in (S-G6).
- **M§8, M§8a** `mail_send` takes addresses; `mail_list` returns remote addresses; item 8 gains one clause (S§10.4). `zygo mail sync` is a new verb; `zygo mail ls` shows a sync line and the outbox and refused counts; `zygo mail watch` syncs (S§8.1).
- **M§11** Gains `host`, `sync_gap` and `[mail.workspace.<name>]` (S§11).
- **W§7.1** Gains the cache path `$HOME/.cache/zygo/workspaces/<id>/mail-sync/` and the state paths `mail/outbox/` and `mail/sync/`.
- **W§12** `zygo ws forget` removes the sync worktree (S§14).

## 17. Files expected to change

P3-1 is not built. "The mail module" is the place where the P3-1 build puts the M§5 store.

| File | Change |
|---|---|
| The mail module | New parts: address grammar (S§7.2), outbox (S§8.6 at send), sync run (S§8.3), import (S§8.5), registry and member cache (S§7.3), identity (S§7.4), public check (S§8.2), prune (S§9). |
| `crates/zygo-plugins/src/sync.rs:32-98` | Reuse `git_command` and `run_bounded`, or move them to a crate that the mail module can use. The behavior does not change. |
| `crates/zygo-work/src/repo.rs:71-75` | Reuse the cut mode for `ls-tree` output, or copy it next to the moved builder. |
| `crates/zygo-redact` | No change. Called from the mail module (S§8.6). |
| `crates/zygo-hooks/src/workspace.rs` | The rate-limited spawn of the sync child on `session`, `prompt`, `stop` and `tool-end` (S§8.1, S§8.4), on the pattern of `start_snapshot_child` (`:484-513`). |
| `crates/zygo-hooks/src/session_text.rs` | The item 8 clause (S§10.4). |
| `crates/zygo-mcp/src/tools/` (mail tools of P3-1) | Addresses, the send-time pass, remote addresses in `mail_list`, the description sentence. |
| `crates/zygo/src/cmd/` (mail command of P3-1) | `zygo mail sync`, the hidden `--ws-id <id> --background` form (AU44), the sync in `watch`, the sync line in `ls`, `esc` for remote values. |
| `crates/zygo/src/cmd/mod.rs` | The results sweep is skipped for `mail sync --background`, as for `ws` (AU71): it runs on hook events. |
| `crates/zygo-workspace/src/admin.rs` | `ws forget`: the worktree removal and the unsent count (S§14). |
| `crates/zygo-config/src/schema.rs`, `check.rs`, `layers.rs` | S§11 keys, checks, `mail` in `USER_ONLY`. |
| `docs/premises.md` | SP1 to SP5 (S§20). |

## 18. Left to the builder

- The exact stderr messages that mean `private` in S§8.2 step 4, for the git version that the build pins, as a list in code with a test per message.
- Where the moved git builder lives (S§17).
- The parse of `git push --porcelain` output for "moved" against "other error" (S§8.3 step 14).
- The `errors.log` text of each line, inside S-C2.
- The test seams of S-C8: a clock, a spawn function, a public probe and a hook between fetch and push. Their form is free; production code takes the real ones.

## 19. Acceptance

Each check is a test or a command. A "machine" in a test is one `HOME` with its own clone of one bare repository, used as remote by a `file://` URL. Clocks and the public probe are injected. The wake uses the multiplexer stub (W§13). No test runs a real agent or depends on wall-clock time (S-C8).

1. Machine A (host `a`, member `alice`) and machine B (host `b`, member `bob`), sync on for both. `alice` sends to `bob@b`. A sync on A, then a sync on B: `bob`'s `new/` holds a file whose bytes equal the blob that A pushed, with `from` `alice@a` and `to` `bob@b`. After B's sync the tip tree has no `bob@b/` file.
2. After 1, in each user checkout of A and B: `HEAD`, the branch list, `git status --porcelain` and the index are the same as before the syncs.
3. A push of B lands between A's fetch and A's push (the seam). A's first push is rejected, its second attempt succeeds, and the tip holds both messages. With the seam forcing 5 rejections, A exits 1 and its outbox still holds the message.
4. A send whose body holds a built-in AWS key pattern is refused at send with the pattern id, and nothing is written to the outbox. A message that passes at send and meets a new pattern before the sync moves to `outbox/refused/`, is not in the tip tree, and its `errors.log` line holds the ULID and the id and no body text.
5. The probe answers `public`: the sync exits 1 with the public line, and the bare repository's refs do not change; a remote send is then refused. `unknown`: the sync runs and prints the line. A table test maps the URL forms of S§8.2 step 2. A clock 59 minutes later uses the cache; 61 minutes later runs the probe again.
6. The bare repository has `zygo/mail` without `ZYGO-MAIL`: the sync exits 1 and pushes nothing. `sync = "main"` is a config check error and no sync runs.
7. Two machines with host `x`: the second sync exits 1 with the collision line and imports nothing. `--take-host` then succeeds, and the first machine's next sync exits 1.
8. Hostile tip, each case in `bob@b/`: a link entry, a submodule entry, a 9 MiB blob, `id` not equal to the name, `to` not equal to the folder, bad JSON, a ULID 25 h in the future, a ULID older than `keep`. None is delivered, no file is written outside `bob`'s maildir, each is deleted from the branch, and `errors.log` has one line each. A registry file with 257 names, or with `host` not equal to its name, is not in the member cache.
9. Duplicate: a message already in `bob`'s `cur/` and still on the tip is not delivered again and is deleted from the branch. A crash seam between A's push and its outbox delete gives one delivery on B.
10. Rate limit: with the clock fixed, ten `tool-end` events start one sync child (spawn seam count 1); after the clock moves 60 s, the next event starts a second one. A send starts a child with no gap.
11. Lock: a sync that finds the lock held writes `again`; the holder runs steps 5 to 15 once more, and a message written to the outbox during the first pass is on the tip at the end.
12. Wake: after B's import, an idle `bob` with a live launcher gets the M§7.3 wake text through the multiplexer stub, once within `wake_gap`; a busy `bob` gets none and the `Stop` hook blocks with the M§7.1 line.
13. Prune: with 1000 commits and the clock past `keep` for some messages, the sync makes one commit with no parent that holds `ZYGO-MAIL`, the registry, and the messages younger than `keep`. With a push of another machine between the fetch and the prune (seam), the lease fails and the tip is that machine's commit.
14. Stale host: a registry file whose `updated_at` is older than `keep` is not in the member cache, a send to its addresses is refused, and a prune removes its file and folders.
15. No content: after tests 1 to 14, no subject or body text appears in `errors.log`, `workspace.json`, `mail/sync/state.json`, the usage store, a commit message, or a ref name.
16. `[mail]` in `.zygo.toml` is ignored with the existing line; in team config it is dropped with one warning.
17. `zygo ws forget` on A: `git worktree list` in A's checkout no longer lists the sync worktree, and the unsent line is printed when the outbox was not empty.
18. After a sync on A that sees `b`'s registry, the session text of a member on A lists `bob@b` in the item 8 clause, and `mail_list` returns it.
19. `zygo mail watch` prints a remote subject that holds `\x1b[2J` as escaped text.

## 20. Premises the first build verifies

- **SP1** A child that a member hook starts with `setsid` (as `start_snapshot_child`) outlives the hook in Claude Code 2.1.328 and in OpenCode 1.19.6, and its `git` calls run with the user's SSH agent and credential helper.
- **SP2** With no credentials (S§8.2 step 3), `git ls-remote --heads` over HTTPS succeeds for a public repository and fails for a private one on github.com and gitlab.com, and the private failure text matches the builder's list.
- **SP3** `git worktree add --detach --no-checkout` plus the index-only steps of S§6.3 write no file in the worktree folder other than its `.git` file, and change nothing in another checkout.
- **SP4** `commit-tree` with `-c commit.gpgSign=false` does not sign or prompt when the user's config has `commit.gpgSign = true`.
- **SP5** A user's plain `git fetch` (default refspec) after a prune updates `refs/remotes/<remote>/<branch>` with no error, and `git pull` on a work branch is not affected.
