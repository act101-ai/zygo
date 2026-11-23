# zygo phase 3 roadmap: distributed workloads

**Status:** started 2026-11-22 by the operator's direction; items are not designed yet unless a spec is named.
**Base specs:** `docs/specs/2026-11-18-zygo-design.md` (`§n`), `docs/specs/2026-11-20-workspaces-design.md` (`W§n`), `docs/specs/2026-11-22-workspace-mail-design.md` (`M§n`).

## Intent

Phase 2 makes a workspace local to one machine and one OS user. Phase 3 lets the members of one repository work on more than one machine, and coordinate through the repository's own remote, with no zygo server and no daemon (W-G6 holds).

The operator's direction (2026-11-22): coordination data that must cross machines goes on a **side branch** of the repository. It never touches `main` or any work branch, and it is pushed to the remote like any branch.

## Items

| Id | Item | Depends | Spec |
|---|---|---|---|
| P3-1 | Workspace mail, local store (one machine) | phase 2 (W3-W10, H1) | `2026-11-22-workspace-mail-design.md` |
| P3-2 | Mail sync on a side branch: carry `mail/` messages between machines through the remote | P3-1 | to write |
| P3-3 | Member identity across machines: `<name>@<host>` addresses, a member registry on the side branch | P3-2 | to write |
| P3-4 | Wake of a remote member: the receiving machine's hooks deliver after a fetch (M§7) | P3-2, P3-3 | to write |

## P3-2 design notes (input for its spec)

- **Branch.** One orphan branch per repository, for example `zygo/mail`, created on first use. It shares no history with `main` and is never merged. zygo works on it in its own detached worktree under the workspace cache directory, never in a user checkout, so no user branch, index or working tree changes.
- **Format.** The M§5 message files, unchanged: one JSON file per message, named by ULID, under `<to>/<ULID>.json`. Files are append-only and never edited, so two machines never change one file and every merge is conflict-free.
- **Read state stays local.** `new/` → `cur/` is per machine; the branch carries messages only. A machine imports a message once (by ULID) into the local maildir and delivers it by M§7.
- **Sync, no daemon.** `zygo mail sync` (fetch, add, commit, push with a retry on a rejected push) runs from the sender's `mail_send`, from member hooks with a rate limit, and on demand. A wake on a remote machine needs one of its hooks or a watcher to run the fetch (P3-4).
- **Content and secrets.** Mail is content (M-C2), and a push publishes it to everyone with read access to the remote. Before a commit, the secret pass (§16.1) runs on each message and a message it cannot clear is refused. Sync is opt-in per workspace (`[mail] sync = "zygo/mail"`), and zygo refuses it on a remote that it can see is public.
- **Retention.** History on the side branch grows. A prune rewrites only that branch (a new orphan commit with the unread messages, then a force push of `zygo/mail` alone). This is the one force push zygo does, and only to its own branch.
- **Rejected for now:** a zygo-hosted relay (adds a server); mail in `main` (pollutes history and races between worktrees); git notes (poor tooling for many small files).

## Not in phase 3 unless the operator adds them

- Moving the work itself between machines (worktrees, builds, locks).
- Shared session state, overrides or results across machines.
- Native Windows workspaces (OP5).
