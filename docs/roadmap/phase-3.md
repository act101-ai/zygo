# zygo phase 3 roadmap

**Status:** started 2026-11-22 (operator); P3-1 approved (spec named); P3-2 to P3-4 not designed yet. Output track added 2026-11-23 (operator); P3-5 to P3-7 not designed yet. Decision-record track added 2026-11-23 (operator); P3-8 to P3-10 not designed yet.
**Base specs:** `docs/specs/2026-11-18-zygo-design.md` (`§n`), `docs/specs/2026-11-20-workspaces-design.md` (`W§n`), `docs/specs/2026-11-22-workspace-mail-design.md` (`M§n`).

## Intent

Phase 2 makes a workspace local to one machine and one OS user. Phase 3 lets the members of one repository work on more than one machine, and coordinate through the repository's own remote, with no zygo server and no daemon (W-G6 holds).

The operator's direction (2026-11-22): coordination data that must cross machines goes on a **side branch** of the repository. It never touches `main` or any work branch, and it is pushed to the remote like any branch.

Phase 3 has a second track (operator, 2026-11-23): it also decreases the tokens that tool output puts in the context, and it makes the reported saving correct. The track comes from a comparison with rtk (https://github.com/rtk-ai/rtk), a CLI proxy that rewrites shell commands and filters their output per command.

The gap today: the trim hook (`crates/zygo-hooks/src/trim.rs`, §10.1) acts on output size only (`[results] trim_threshold_bytes`, default 16384). It does not read the Bash command, and it does not measure or store Bash stderr. Bash output below the threshold (git, test runners, linters) goes to the model in full. Kind sniffing (`crates/zygo-results/src/kind.rs`: json, jsonl, diff, log, tree, text) is used only by `result_tree`, not by the view. The saving figure is `SUM(stored_bytes - view_bytes)` ÷ 4 (`crates/zygo-usage/src/query.rs`, `crates/zygo/src/cmd/usage.rs`). It counts each trimmed byte one time, although the model pays for that byte on each later request. The `trims` table has `tool` and `kind` columns that `zygo usage` does not print. No benchmark and no measured hook latency exist.

The direction: zygo shapes output in the `PostToolUse` trim hook, after the tool has run. It does not rewrite commands. It keeps the raw output in the result store for each shaped view.

Phase 3 has a third track (operator, 2026-11-23): delegated decision records. The agents that author work (the `zygo-<role>` subagents) make decisions and assumptions that the operator did not make. A review gate runs 3 rounds at most. Today, a finding that is open after round 3 needs the operator. The direction: each such decision gets a record in the same way as an ADR, but after the fact. Then the work can pass the 3-round gate and continue with no operator action. The operator audits the records at the end of a phase and makes categorical changes when an assumption was incorrect. No finding stays without a record and a disposition.

P3-8 comes first in the order of work, because each node that runs without it makes decisions that are not recorded.

## Items

| Id | Item | Depends | Spec |
|---|---|---|---|
| P3-1 | Workspace mail, local store (one machine) | phase 2 (W3-W10, H1) | `2026-11-22-workspace-mail-design.md` |
| P3-2 | Mail sync on a side branch: carry `mail/` messages between machines through the remote | P3-1 | to write |
| P3-3 | Member identity across machines: `<name>@<host>` addresses, a member registry on the side branch | P3-2 | to write |
| P3-4 | Wake of a remote member: the receiving machine's hooks deliver after a fetch (M§7) | P3-2, P3-3 | to write |
| P3-5 | Saving accounting with a carry factor: two figures ("tokens kept out of context", "estimated spend avoided"), net of read-backs, counted per context; per-tool and per-command breakdown in `zygo usage`; view to read-back ratio | none | to write |
| P3-6 | Command-aware output shaping in the trim hook: ANSI and progress removal, repeated-line dedup with counts, failures-only views for test runners, kind-aware views (diff, json, log), Bash stderr measured and stored | P3-5 | to write |
| P3-7 | Shaping tests: fixtures from real command output, a minimum saving for each filter, a never-worse check, a measured hook latency budget | P3-6 | to write |
| P3-8 | Delegated decision record (DDR): record format, kinds, statuses, the disposition rule for review findings, the classes that stop for the operator | none | to write |
| P3-9 | DDR enforcement in zygo: the verdict step refuses a pass while a finding has no disposition; `zygo work report` counts open records; a `Zygo-Decision:` commit trailer | P3-8 | to write |
| P3-10 | Phase-end ratification: the operator gets each open record to ratify, overturn or promote | P3-8 | to write |

## P3-2 design notes (input for its spec)

- **Branch.** One orphan branch per repository, for example `zygo/mail`, created on first use. It shares no history with `main` and is never merged. zygo works on it in its own detached worktree under the workspace cache directory, never in a user checkout, so no user branch, index or working tree changes.
- **Format.** The M§5 message files, unchanged: one JSON file per message, named by ULID, under `<to>/<ULID>.json`. Files are append-only and never edited, so two machines never change one file and every merge is conflict-free.
- **Read state stays local.** `new/` → `cur/` is per machine; the branch carries messages only. A machine imports a message once (by ULID) into the local maildir and delivers it by M§7.
- **Sync, no daemon.** `zygo mail sync` (fetch, add, commit, push with a retry on a rejected push) runs from the sender's `mail_send`, from member hooks with a rate limit, and on demand. A wake on a remote machine needs one of its hooks or a watcher to run the fetch (P3-4).
- **Content and secrets.** Mail is content (M-C2), and a push publishes it to everyone with read access to the remote. Before a commit, the secret pass (§16.1) runs on each message and a message it cannot clear is refused. Sync is opt-in per workspace (`[mail] sync = "zygo/mail"`), and zygo refuses it on a remote that it can see is public.
- **Retention.** History on the side branch grows. A prune rewrites only that branch (a new orphan commit with the unread messages, then a force push of `zygo/mail` alone). This is the one force push zygo does, and only to its own branch.
- **Rejected for now:** a zygo-hosted relay (adds a server); mail in `main` (pollutes history and races between worktrees); git notes (poor tooling for many small files).

## P3-5 design notes (input for its spec)

- **Carry factor.** A trim keeps `S` tokens out of the context. With `N` later requests in the same context before a compaction or a clear: token-turns avoided = `S × (N + 1)`; spend avoided ≈ `S × (w + r × N)` in input-price units. `w` is the cache-write multiplier (1.25 for the 5-minute cache, 2 for the 1-hour cache) and `r` is the cache-read multiplier (0.1). The multipliers come from the catalog, not from constants in code.
- **Cache expiry.** A request that follows a cache expiry writes the prefix again; that request counts at `w`, not `r`. The `requests` table records cache-write tokens (`cache_write_tokens`), so this is measurable.
- **Read-backs.** Each `result_view`, `result_search` and `result_tree` reply goes into the context and is carried too. The net figure subtracts them with the same factor.
- **Scope is the context, not the session.** A trim in a subagent is carried only for the life of that subagent. The `requests` and `trims` tables each have an `agent_id` column (`crates/zygo-usage/src/schema.rs`). Open point to verify in the spec: that the column is filled for each subagent request and trim; if it is not, the spec adds that.
- **Two figures, kept apart.** "Tokens kept out of context" stays the one-time figure. "Estimated spend avoided" is the multiplied figure and has the `≈` mark like other notional spend. Multiplied tokens are never shown as a context-size figure.
- **Not counted.** A smaller context reaches the soft window later and compacts less frequently. The spec states this effect and puts no number on it.

## P3-6 design notes (input for its spec)

- **After the tool, not before.** Shaping runs in `PostToolUse` on the finished output, with the command from the hook input. No command is rewritten, so permission rules see the user's real command, and output that the shell pipes to another program is not changed.
- **Always recoverable.** Each shaped view stores the raw output and carries the result id marker; a shaped view below the size threshold is stored too. The existing "no view larger than the text" rule holds.
- **Order of work.** First the low-risk steps (ANSI and progress removal, repeated-line dedup). Then kind-aware views. Then command-specific views (test runners first).
- **Fail open.** A shaper that cannot parse its input returns the plain size-based view (C6).
- **Rejected for now:** a `PreToolUse` command rewrite to a wrapper binary (changes what permission rules see; makes output unfit for scripts); user-defined filter files (needs a trust gate; decide after the built-in shapers are measured).

## P3-8 design notes (input for its spec)

- **Name.** Delegated decision record, DDR. The agent decided under delegated authority and the operator has not ratified the decision. "PDR" is not used, because it is a known term (preliminary design review).
- **One record type, with a kind.** The kinds are `assumption`, `decision`, `accepted-limit`, `compromise` and `open-finding`.
- **Fields.** Id; work unit; role and model; commit; date; the choice; the alternatives rejected; the evidence; the assumptions it depends on, by id; the cost to reverse; the later records that build on it; status.
- **Statuses.** `open`, `ratified`, `overturned`, `fixed`, `superseded`.
- **Assumptions are first-class.** A decision cites an assumption by id. When an assumption is found incorrect, one query lists each record that depends on it. This is the base for a categorical change.
- **Disposition rule.** Each review finding ends in exactly one disposition: fixed (commit), rejected (reason), or recorded (DDR id). After round 3 the gate lets the work continue only when no finding is without a disposition.
- **Classes that stop for the operator.** An agent cannot record these and continue: a failed acceptance check; a security finding; data loss or an action that cannot be reversed; a change to a public contract (CLI, config schema, MCP tool shape, stored format); a deviation from a constraint in an approved spec; a new rule or a permission change.
- **Who signs.** The author does not accept its own compromise. The reviewer or the lead signs the disposition.
- **Storage.** One file per record in the repository, under `docs/decisions/`, in the pull request that caused it.
- **No numeric limit on open records at first.** Phase 3 measures the count and the dependency depth; the first phase-end review decides if a limit is necessary.
- **Outside this repository.** The plan, execute and deliver skills are in the act101 plugin. They need these changes: the plan step states what an author can decide alone; the execute step writes records at the verification stage; the deliver step lists the records in the pull request body. P3-8 names these changes; the plugin makes them.

## P3-10 design notes (input for its spec)

- **Outcomes per record.** Ratify; overturn (this makes a work node); promote to an ADR, a spec change or a rule.
- **Order.** Records are shown sorted by the number of records that build on them, largest first.
- **Rules need the operator.** A record never becomes a rule without this review.

## Not in phase 3 unless the operator adds them

- Moving the work itself between machines (worktrees, builds, locks).
- Shared session state, overrides or results across machines.
- Native Windows workspaces (OP5).
- User-defined output filters and a scan of past session transcripts for missed savings.
