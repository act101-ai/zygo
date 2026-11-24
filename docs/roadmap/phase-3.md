# zygo phase 3 roadmap

**Status:** started 2026-11-22 (operator); P3-1 approved (spec named); P3-2 to P3-4 not designed yet. Output track added 2026-11-23 (operator); P3-5 to P3-7 not designed yet. Decision-record track added 2026-11-23 (operator); P3-8 to P3-10 not designed yet. Workspace-definition items added 2026-11-23 (operator); for P3-11 to P3-13 the spec conflicts were resolved with the operator on 2026-11-23; the specs are to write. Review-finding items added 2026-11-23 (operator); P3-14 and P3-15 not designed yet. Plan of work set 2026-11-24 (operator): six specs in a fixed order, then one work graph for the phase (see "Plan of work").
**Base specs:** `docs/specs/2026-11-18-zygo-design.md` (`§n`), `docs/specs/2026-11-20-workspaces-design.md` (`W§n`), `docs/specs/2026-11-22-workspace-mail-design.md` (`M§n`), `docs/specs/2026-11-21-statusline-hud-design.md` (`H§n`).

## Intent

Phase 2 makes a workspace local to one machine and one OS user. Phase 3 lets the members of one repository work on more than one machine, and coordinate through the repository's own remote, with no zygo server and no daemon (W-G6 holds).

The operator's direction (2026-11-22): coordination data that must cross machines goes on a **side branch** of the repository. It never touches `main` or any work branch, and it is pushed to the remote like any branch.

Phase 3 has a second track (operator, 2026-11-23): it also decreases the tokens that tool output puts in the context, and it makes the reported saving correct. The track comes from a comparison with rtk (https://github.com/rtk-ai/rtk), a CLI proxy that rewrites shell commands and filters their output per command.

The gap today: the trim hook (`crates/zygo-hooks/src/trim.rs`, §10.1) acts on output size only (`[results] trim_threshold_bytes`, default 16384). It does not read the Bash command, and it does not measure or store Bash stderr. Bash output below the threshold (git, test runners, linters) goes to the model in full. Kind sniffing (`crates/zygo-results/src/kind.rs`: json, jsonl, diff, log, tree, text) is used only by `result_tree`, not by the view. The saving figure is `SUM(stored_bytes - view_bytes)` ÷ 4 (`crates/zygo-usage/src/query.rs`, `crates/zygo/src/cmd/usage.rs`). It counts each trimmed byte one time, although the model pays for that byte on each later request. The `trims` table has `tool` and `kind` columns that `zygo usage` does not print. No benchmark and no measured hook latency exist.

The direction: zygo shapes output in the `PostToolUse` trim hook, after the tool has run. It does not rewrite commands. It keeps the raw output in the result store for each shaped view.

Phase 3 has a third track (operator, 2026-11-23): delegated decision records. The agents that author work (the `zygo-<role>` subagents) make decisions and assumptions that the operator did not make. A review gate runs 3 rounds at most. Today, a finding that is open after round 3 needs the operator. The direction: each such decision gets a record in the same way as an ADR, but after the fact. Then the work can pass the 3-round gate and continue with no operator action. The operator audits the records at the end of a phase and makes categorical changes when an assumption was incorrect. No finding stays without a record and a disposition.

P3-8 comes first in the order of work, because each node that runs without it makes decisions that are not recorded.

Phase 3 also has workspace-definition items (operator, 2026-11-23). A repository can define its workspace (config, model profiles and members) in files, so that each machine and each person starts the same workspace after the operator approves the files on that machine. The status line shows the session run time, because a workspace session runs for days.

Phase 3 also has review-finding items (operator, 2026-11-23). A review found two gaps in zygo. Each was then verified in the code and in a live Claude Code session (P3-14, P3-15).

The gaps today (verified 2026-11-23): a subagent that is not a `zygo-<role>` agent gets the trim marker, the `[REDACTED:<id>]` marker and the guards, but not the sentences that explain them. §8.6 item 7 says that the mechanics text is appended to each subagent prompt; the code does this for the `zygo-<role>` agents only. And no premise, fixture or test records which zygo text is in the context after a compaction.

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
| P3-11 | Workspace config and model profiles as repository files; no credentials | phase 2 (workspaces) | to write |
| P3-12 | Workspace member definition as a repository file: default one orchestrator and four elephant workers; optional config-directory pool for the members | P3-11 | to write |
| P3-13 | Session run time in the status line and in `zygo status`, with a format that holds for days | none | to write |
| P3-14 | Session rules for each subagent: the mechanics text (§8.6) also reaches a subagent that is not a `zygo-<role>` agent | none | to write |
| P3-15 | Session text after a compaction: a premise, a fixture and a test for each agent; a correction if the text is lost | none | to write |

## Plan of work (operator, 2026-11-24)

1. **One spec per track.** Each Phase 3 track gets one spec. The decision-record spec covers P3-8, P3-9 and P3-10 together.
2. **One work graph.** One work graph covers all of Phase 3. It is made after all track specs are approved. A spec does not plan nodes.
3. **A review gate per node.** Each work node gets its own review gate of up to 3 rounds (phase 2 had no per-node review). The P3-8 disposition rule applies at each gate: after round 3, a node passes only when each finding has exactly one disposition (fixed with a commit, rejected with a reason, or recorded with a DDR id), and no finding is of a class that stops for the operator.
4. **One branch, one pull request.** Phase 3 is built on the branch `phase-3` and delivered as one pull request at the end.

Spec order:

1. Decision records (P3-8 to P3-10): `docs/specs/2026-11-24-decision-records-design.md`.
2. Mail sync (P3-2 to P3-4).
3. Output shaping (P3-5 to P3-7).
4. Workspace definition (P3-11, P3-12).
5. Session run time (P3-13 alone).
6. Session text (P3-14, P3-15).

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

## P3-11 design notes (input for its spec)

- **Location.** A `.zygo/` directory in the repository: `.zygo/workspace.toml` and one file per profile under `.zygo/profiles/`. The workspace file is a loadout (W§11), not a new format (P3-12). `.zygo.toml` stays as it is (§13.5).
- **Read from the default branch.** The same rule as `.zygo.toml` (§13.5): a work branch cannot change the workspace it runs in.
- **The files are a template.** zygo applies nothing from them until the operator approves them on this machine.
- **Allowed content is a list, not "all but secrets".** The spec names each key a repository file can set. Never allowed: credentials, provider endpoints or base URLs, a change that turns off or weakens redaction, the update settings. A profile in the repository names roles and models from the user's catalog only; it cannot define a provider.
- **Reason for the limit.** A repository file that could set a provider endpoint could send each request to a host that the repository author controls.
- **Trust gate.** The approval is a named verb, `zygo workspace approve`. The verb shows the files and asks; `--yes` approves in scripts (§15.7). The approval is for a content digest and is stored as plugin approvals are (§15.7). A new digest needs a new approval.
- **Launch.** A launch never prompts (§15.7). `zygo up` without an approval applies nothing from the repository and prints one line.
- **Layer.** The §6.1 layer order is not changed. A repository profile is one more profile source. When a repository profile and a user profile have the same name, the user profile is used.
- **`zygo config show`** prints the repository file as the source of each key it sets (§6.1).
- **Resolved with the operator (2026-11-23).** The approved texts below did not agree with the first picks. Each line gives the resolution and the text that the spec amends or keeps.
  - **W-C5 is amended.** The old rule: a cloned repository cannot start an agent or a command through a workspace, and `[loadout]` and `[workspace]` come from user config only (W-C5, W AU10, W AU22, W§11). The new rule: a cloned repository cannot start an agent until the operator approves its workspace files on this machine. The spec amends W-C5, W AU10, W AU22 and the W§14 rejected alternative (loadouts in `.zygo.toml`), and it says so in its base-spec changes. `.zygo.toml` keeps its four tables (§6.1 layer 4, §13.5).
  - **§15.7 holds.** A launch never prompts. The approval verb asks, and `--yes` approves in scripts.
  - **§6.1 holds.** The layer order is not changed. A user profile is used before a repository profile of the same name; no layer moves for this.
  - **§11.1 gets one more profile source.** §11.1 puts profiles in user config and team config only. The spec adds the repository as a source, after the approval.
  - **§13.5 review boundary.** §13.5 calls the default-branch rule a review boundary, not a security boundary. That text holds. For these files the approval is the security boundary; the default-branch rule alone is not.
- **Open points for the spec.**
  - The exact allow list of keys for the repository files.
  - Profile content. A §11.1 profile can also hold a prompt, custom roles with prompts, a plugin selection and feature keys. The pick stays "roles and models only" until the spec decides otherwise. The spec states how §11.2 selection and §11.3 validation treat a repository profile.

## P3-12 design notes (input for its spec)

- **Terms.** A "member" is the W§5 member: the agent slot of one pane, with the optional name of M§6. "Orchestrator" and "worker" are not terms of the approved specs; M§1 says "a lead and its workers". An orchestrator is a member that gives work to the other members and collects their results through workspace mail (P3-1). An elephant worker is a member that is itself a lead session (§5 "Lead / elephant", G3): it holds context and delegates to `zygo-<role>` subagents. Each `zygo claude` member is already a lead session, so the two differ in their job, not in how zygo starts them. The word "team" is not used for the set of members, because it means the team config of serve mode (§6.1 layer 2).
- **Definition.** `.zygo/workspace.toml` is a loadout (W§11), not a new format. The table is `[members]`. Each member has a name (M§6), a `kind` and a profile, and one new key: `job = "orchestrator" | "worker"`. `kind` keeps its W§11 meaning, the agent kind (`claude-code`, `opencode`, `shell`). The default when no member is defined: one orchestrator and four elephant workers.
- **Not allowed in the repository file.** `config_dir`, `args`, `yolo`, `passthrough`, `server` and a shell pane.
- **Pool.** A config-directory pool is a directory whose subdirectories are agent config directories, for example `~/.claude-interns/<name>`. zygo starts each member with one of them as the agent's config directory, so that members can run on different agent accounts.
- **The pool path is user config, not repository content.** It is a path on one machine and it leads to credentials. The repository file can only say that a member takes its config directory from the pool.
- **Assignment.** Round-robin over the pool in name order, stable per member name across restarts. When the pool has fewer directories than the workspace has members, directories are shared and `zygo doctor` reports it. With no pool, each member uses the user's default config directory.
- **zygo does not read the pool's credentials.** It sets the config directory for the child only; it does not copy, read or log the files in it. The §8.4 rules hold.
- **The member count is bounded.** `[limits]` in user config sets a maximum member count that a repository file cannot exceed.
- **Pools return from a non-goal.** W§3 lists "pools and slots of agents" as a non-goal of phase 2 that can return in a later phase. This item returns the pool; the spec says so in its base-spec changes.
- **Already in the specs.** A loadout pane has `kind`, `profile` and `config_dir` (W§11), and `name` (M§6). The member record saves the effective `CLAUDE_CONFIG_DIR` and resume sets it again (W AU61, W AU92); PW16 expects a resume with another config directory to fail. Thus the member definition is a loadout with a job per member, and the pool only selects the `config_dir` value.
- **Resolved with the operator (2026-11-23).**
  - **W-C5 is amended.** The member definition in the repository starts five agents by default. It starts none until the operator approves the workspace files on this machine (P3-11). The spec amends W-C5, W AU10, W AU22 and the W§14 rejected alternative, and it says so. The definition stays in the repository; it does not move to user config.
  - **The file is a loadout.** W§11 holds as the format. The spec adds the `job` key to it and states the `[members]` table.
  - **`kind` holds.** In W§7.2 and W§11, `kind` is the agent kind. Orchestrator and worker use `job`.
  - **`config_dir` is user config only.** W§11 allows `config_dir` in a loadout pane of user config. The repository file cannot set it; a member there can only state that its config directory comes from the pool.
  - **The word "team".** It keeps its one meaning, the team config of serve mode (§6.1 layer 2). The table is `[members]`.
- **Open points for the spec.**
  - The rule that stops a repository `[limits]` from raising the member maximum. `.zygo.toml` can set `[limits]` (§6.1 layer 4, §13.5), and the turn cap takes the lower non-zero value of the layers. The member maximum is read from user config (and team config) only, or it follows the same "lower value" rule.
  - The pool on OpenCode. `config_dir` is for `claude-code` only (W§11). The spec states what a pool does for an `opencode` member.
  - The §8.5 launch interaction: how this meets the per-session settings file and `--plugin-dir` that zygo passes at launch (§8.5), the §8.4 preflight that reads the settings files of the config directory, and what Claude Code version a pool directory needs.
  - How the `[members]` table relates to the `[[loadout.<name>.window.pane]]` entries of W§11.

## P3-13 design notes (input for its spec)

- **What it shows.** Wall-clock time since the member's first start. It continues across a workspace snapshot and resume, so the start time is stored with the member and is not the process start. A session outside a workspace shows the time since its own start.
- **Format.** Two units at most: `47m`, `5h12m`, `2d04h`, `12d07h`. No seconds. Fixed width within each range so the line does not move.
- **Where.** Both HUD styles on Claude Code (`compact` and `verbose`, H§8); one line in `zygo status` for OpenCode and for scripts (A-9, H§3).
- **Cost.** One read of a stored timestamp; no new process and no change to the status line time budget (H-C2, 3 s). For a member, the HUD already reads `workspace.json` on each run (H§7.6).
- **Not shown at first.** Active time (time with a request in flight) is a different figure. The usage database can give it later; the first version shows wall-clock time only.
- **Already in the specs.** `member.last_run_at` is the time of the member's last committed start, and `member.pending.started_at` is the time of a start that is not committed (W§7.2, W AU92). Neither is the first start. The Claude Code stdin has a `cost` key that the HUD never reads (H§3, H§7.1); the spec does not take the run time from it.
- **Resolved with the operator (2026-11-23).**
  - **The run time belongs to the member.** §5 and W§5 hold: a session is one launch of one agent, and a resumed agent is a new zygo session with the same agent conversation. Thus the run time is not a property of the zygo session. The spec adds a `first_started_at` field to the member record (W§7.2). A session outside a workspace (W-C6) uses its own start.
  - **The HUD spec is amended.** H§6.2 fixes the segments and their order (H AU9), and H§6.7 fixes the order in which segments are removed at a small width. The spec amends the HUD spec to add the segment and its place in the removal order, as M§9a did for the member name and the mail count.
- **Open point for the spec.** The row, the group and the place in the removal order that the new segment gets.

## P3-14 design notes (input for its spec)

- **Verified in the code (2026-11-23).** A `zygo-<role>` subagent gets the role prompt and the mechanics text through the `--agents` prompt (`crates/zygo-launch/src/claude_code.rs`). The `SessionStart` hook returns nothing for a subagent (`crates/zygo-hooks/src/session_start.rs`). zygo registers no `SubagentStart` hook. A user `--agents` entry is merged as given, and a plugin agent keeps its own prompt.
- **Enforcement has no such limit.** The trim hook (`crates/zygo-hooks/src/trim.rs`), the guard hook (`crates/zygo-work/src/guards.rs`) and the secret pass (`crates/zygo-proxy/src/redact_stage.rs`) act on each subagent. Thus a subagent can get a trim marker or a `[REDACTED:<id>]` marker that nothing explained to it.
- **Verified in a live session (2026-11-23, Claude Code).** The built-in Explore agent and the plugin agent `act101:code-explorer` had none of the four sentences (redaction marker, result tools, no `head` or `tail`, commit trailers). The `zygo-read` agent had all four. All three had the `mcp__zygo__*` tool names.
- **Which agents.** The built-in Explore, Plan and general-purpose agents, plugin agents, and user `--agents` entries.
- **No work unit.** A `work_begin` call from a subagent that is not a zygo role is ignored (`crates/zygo-work/src/hooks.rs`), so its work is not in a work unit.
- **OpenCode has the same gap.** The built-in agents get a model and an edit permission only (`crates/zygo-launch/src/opencode.rs`). The system-prompt transform of the generated plugin does not run for subagents (§8.7).
- **Base-spec text.** §8.6 item 7 and the code do not agree. The spec for this item states which one changes.
- **Open points for the spec.**
  - The mechanism on OpenCode, which gives no agent id.
  - If the lead is steered away from agent types that are not zygo roles. Today only the session text does this; no `PreToolUse` hook matches the agent tool.
  - If the work of such a subagent becomes a work unit.
  - The `--agents` prompt is built at launch, so a `results` or `work_units` override made later does not change the mechanics text of a `zygo-<role>` agent.

## P3-15 design notes (input for its spec)

- **Verified in the code (2026-11-23).** zygo registers `SessionStart` with no matcher (`crates/zygo-launch/src/claude_code.rs`), and the hook does not read the `source` field (`crates/zygo-hooks/src/session_start.rs`). Thus the hook gives the session text for each main-loop `SessionStart` event, of any source.
- **Not recorded.** The only fixture with `source: compact` is the subagent fixture (`fixtures/claude-code/hooks/SessionStart-sub.json`); the main-loop fixture has `source: startup`. No premise in `docs/premises.md` and no test covers the main loop after a compaction. §8.6 says only that the text is injected at session start. The `PreCompact` text (§10.6) does not hold the session rules.
- **Not known.** If Claude Code sends a main-loop `SessionStart` with `source: compact` after an automatic compaction. If the session text from the start stays in the context through a compaction. If OpenCode calls the system-prompt transform on the first request after a compaction.
- **Work.** One premise for each agent, with a fixture (§19), and a test. If the text is lost, the hook gives it again. If the text is then in the context two times, the spec states that cost.
- **Launch-time values.** Each run of the hook reads the `results` and `work_units` overrides. The other parts of the text (role prompt, profile prompt, legend, work item) are launch-time values. This agrees with the next-launch scope of the `profile` and `plugins` verbs (§17.1). No change is planned.

## Not in phase 3 unless the operator adds them

- Moving the work itself between machines (worktrees, builds, locks).
- Shared session state, overrides or results across machines.
- Native Windows workspaces (OP5).
- User-defined output filters and a scan of past session transcripts for missed savings.
