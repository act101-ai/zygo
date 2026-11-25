# Phase 3 Work Graph

<!-- work-graph -->

**Goal:** Build Phase 3 of zygo (workspace mail, mail sync, output shaping and saving accounting, decision records, workspace definition, session run time and session text) as the seven approved specs specify.
**Authority:** `docs/specs/2026-11-22-workspace-mail-design.md` (M§n; P3-1), `docs/specs/2026-11-24-decision-records-design.md` (D§n; P3-8 to P3-10), `docs/specs/2026-11-24-mail-sync-design.md` (S§n; P3-2 to P3-4), `docs/specs/2026-11-24-output-shaping-design.md` (O§n; P3-5 to P3-7), `docs/specs/2026-11-24-workspace-definition-design.md` (R§n; P3-11, P3-12), `docs/specs/2026-11-24-session-runtime-design.md` (T§n in RT nodes; P3-13), `docs/specs/2026-11-24-session-text-design.md` (T§n in SX nodes; P3-14, P3-15), each approved by the operator (P3-1 on 2026-11-22, the others on 2026-11-24); base specs `docs/specs/2026-11-18-zygo-design.md` (§n), `docs/specs/2026-11-20-workspaces-design.md` (W§n), `docs/specs/2026-11-21-statusline-hud-design.md` (H§n); order of work `docs/roadmap/phase-3.md`
**Run:** parallel=3 · worktrees=.worktrees/phase3/ · disk-per-node=—
**Workflows:** (1) Each node gets its own review gate of up to 3 rounds; every finding ends as fixed (a commit in the node range), rejected (with a reason) or recorded (a DDR id), per `docs/specs/2026-11-24-decision-records-design.md` — operator, 2026-11-24 (`docs/roadmap/phase-3.md` "Plan of work" item 3). (2) Each node is delivered in its own pull request to the rendezvous branch `phase-3`; the controller merges each node's pull request into `phase-3` once its review gate passes and its CI is green, a node is DONE when that pull request is merged, and each dependent node branches from `phase-3` after that; `phase-3` reaches `main` only when the operator says so — operator, 2026-11-24 ("we PR after every row, including planning"; "we need a phase-3 rendezvous branch, each row merges to that branch so you can be autonomous"). (3) DDR first: every node with no other dependency depends on DR-1 (the record format), so DR-1 is the only root — operator, "P3-8 comes first in the order of work" (`docs/roadmap/phase-3.md` "Intent"). (4) The act-lite/act101 skill changes that the DDR spec names are made in their own repositories (`~/Work/act-lite`, `~/Work/act`, `~/Work/act101-marketplace`), not in this graph; no zygo release that contains DR-13's `dispositions` guard at `enforce` ships before those skill changes ship — `docs/specs/2026-11-24-decision-records-design.md` D§12.

To continue in any session: "resume the work in `docs/work/phase3/graph.md`".

## Global constraints

Phase 2 constraints that still bind code Phase 3 touches:

- Base constraints C1-C10 apply; C6: a hook never breaks the agent; every new hook branch, on an internal error, changes no decision, writes at most one line and exits 0 (mail, sync, shaping, session text) — `docs/specs/2026-11-20-workspaces-design.md` W§4 W-C1; `docs/specs/2026-11-22-workspace-mail-design.md` M§4 M-C1; `docs/specs/2026-11-24-mail-sync-design.md` S§4 S-C1; `docs/specs/2026-11-24-output-shaping-design.md` O§4 O-C1; `docs/specs/2026-11-24-session-text-design.md` T§4 T-C1
- No daemon; tmux is the only long-lived process; mail is delivered by hooks and the sender's process — `docs/specs/2026-11-20-workspaces-design.md` W§4 W-C2, W§2 W-G6; `docs/specs/2026-11-22-workspace-mail-design.md` M§2 M-G6
- tmux options and hooks only on zygo's own sessions; never a global option or hook — `docs/specs/2026-11-20-workspaces-design.md` W§4 W-C3
- Every state write is atomic (temporary file, then rename); every mail write is `tmp/`, `fsync`, rename into `new/` — `docs/specs/2026-11-20-workspaces-design.md` W§4 W-C4, W§7.3; `docs/specs/2026-11-22-workspace-mail-design.md` M§4 M-C3
- W-C5 as amended: a cloned repository cannot start an agent through a workspace until the operator approves its workspace files on this machine; loadouts come from user config, and from `.zygo/workspace.toml` after that approval. This amends the Phase 2 constraint "`[loadout]` and `[workspace]` come from user config only"; `[statusline]`, `[update]` and `[mail]` stay user-config only (the team layer and `.zygo.toml` drop them) — `docs/specs/2026-11-24-workspace-definition-design.md` R§4 R-C1, R§14 row W-C5; `docs/specs/2026-11-20-workspaces-design.md` W-C5, AU10, AU22; `docs/specs/2026-11-22-workspace-mail-design.md` M§11
- `zygo claude`/`zygo opencode` outside a workspace stay a clean v1 launch — `docs/specs/2026-11-20-workspaces-design.md` W§4 W-C6, OP9
- Workspace state holds no secret and no content; no user-controlled text reaches a tmux format argument unescaped; never resume a conversation whose launcher is live — `docs/specs/2026-11-20-workspaces-design.md` W§4 W-C7, W-C8, W-C9
- Native Windows: every workspace and mail verb exits 2 with `workspaces need tmux; run zygo in WSL`; every crate still builds on windows-latest; tests that use workspace state, tmux, `setsid` or file modes are `cfg(unix)` — `docs/specs/2026-11-20-workspaces-design.md` OP5; `docs/specs/2026-11-22-workspace-mail-design.md` M§3; `docs/specs/2026-11-24-mail-sync-design.md` S§3; `.github/workflows/ci.yml` (windows-latest)
- Minimum tmux 3.2; tmux-dependent tests run when tmux 3.2 or later is on `PATH` and skip with a printed reason otherwise; `cfg(unix)` on Windows; no CI install step — `docs/specs/2026-11-20-workspaces-design.md` AU7; operator, 2026-11-21 (`docs/work/phase2/graph.md` Global constraints)
- State dir `0700`, files `0600` (also `mail/`, `reinject/`, the results directory) — `docs/specs/2026-11-20-workspaces-design.md` AU2; `docs/specs/2026-11-22-workspace-mail-design.md` M§4 M-C4; `docs/specs/2026-11-24-output-shaping-design.md` O§4 O-C3; `docs/specs/2026-11-24-session-text-design.md` T§9.2 item 1
- Crate graph: `zygo-workspace` depends on `zygo-core`, `zygo-config`, `zygo-work`, `zygo-proxy` only (the mail store lives in `zygo-workspace::mail`; sync, which needs `zygo-redact` and git, lives in the new crate `zygo-mail-sync`, which `zygo-workspace` never depends on); `zygo-hooks` depends on `zygo-workspace`; the status-line module does not depend on `zygo-workspace`; `zygo-config` does not depend on `zygo-workspace` — `docs/specs/2026-11-20-workspaces-design.md` AU103, AU102, AU45; `docs/specs/2026-11-21-statusline-hud-design.md` H§7.6; `docs/specs/2026-11-22-workspace-mail-design.md` M§9a
- No wall-clock or deadline test runs in CI; such checks are `#[ignore = "timing: …"]` and run on demand; time rules are tested with an injected clock or `now` — operator (PR #15, commits `cb7d13c`, `96c1cbc`)
- CI and harness tests never run a real `claude` or `opencode` binary or a real agent config directory; they use the stub agents (`crates/zygo-launch/tests/stub_agent/`) and recorded fixtures; live checks run real agents only by the operator — operator instruction (owner login risk), held in the lead's session memory before 2026-11-24; applied in Phase 2 by the stub agent `crates/zygo-launch/tests/stub_agent/` (node L3, `docs/work/phase2/history.md:31`); `docs/live-checks.md` "Method"
- No Claude attribution in commits, pull requests, tags, release notes or files; commit trailers `Zygo-Session: <session>` and, when a unit is named, `Zygo-Work: <unit>` — `docs/specs/2026-11-18-zygo-design.md` §8.5 ("operator, 2026-11-22: there is never Claude attribution"); the session rule of each zygo session (the mechanics text, §8.6 item 7)

Decision records (D):

- No record text, finding text, label or disposition reason in the usage store, a log or a session file; no hook reads a record file; the slash form reads none; the verdict check is in the MCP tool — `docs/specs/2026-11-24-decision-records-design.md` D-C1, D-C2, D§3, D§10.1
- The record file is the only owner of a record's state (no index); a record is never deleted or renamed; an agent never writes an operator key or status — D-C3, D-C4, D-C5
- Record id `^DDR-[0-9A-HJKMNP-TV-Z]{26}$`; file `docs/decisions/<id>.md`; TOML front matter between `+++` lines; zygo edits with `toml_edit` and keeps unknown keys, comments and the body byte-equal — D§6.1, D§6.2
- Bounds: each git process 2 s; one verdict at most 4 git processes and 10 s; `--batch` at most 2,048 objects and 16 MiB; node range at most 10,000 commits; at most 1,000 record changes read; record file at most 64 KiB, front matter closed within 8 KiB; directory at most 10,000 entries; `work_verdict` arguments at most 256 KiB, `dispositions` at most 256 entries, `finding` 1–200 bytes, `reason` 1–2,000 bytes; `title` 1–120 bytes; `unit` at most 128 bytes; `depends_on` at most 64 ids; `review_note` at most 2,000 bytes; `promotion` at most 500 bytes; `<problems>` at most 10 then `; and <n> more`, at most 4 KiB; `show` at most 50 commits, `review` at most 20 — D-C6, D§9.3, D§10.1, D§11.2
- The `dispositions` guard follows the §13.4 levels and §13.5 resolution, default `enforce` — D-C7, D§9.3
- Text from a record or from git is printed one line at a time through `zygo_core::esc` — D§10.1
- Not a security boundary against an agent that runs as the same OS user; no code or text claims more — D-C9
- Verdict tests use a git fixture repository with linked checkouts; git failures come from an injected git runner — D§17 preamble

Workspace mail (M):

- Mail is content: only under `mail/`; never in `workspace.json`, `activity.json`, a log or a usage row; the proxy stores no body — M§4 M-C2
- No mail text reaches a tmux argument; the wake text is fixed: `zygo: you have mail; read it with mail_read` — M§4 M-C5, M§7.3
- Member names come from user config only (loadout, `--name`, `zygo ws name`) or approved repository files (M-C6 as amended in R§14); name grammar `^[a-z][a-z0-9-]{0,31}$`, unique in the loadout; `all`, `cli` (and, with sync, `zygo`) reserved; checked at each read (AU84) — M§4 M-C6, M§6; R§14; S§16
- Message file version 1, ULID names, never edited after the rename into `new/`; unknown fields kept — M§5 (AU78)
- Limits: subject at most 200 bytes, body at most 1 MiB, `[mail] max_unread = 500`, `[mail] keep = "30d"`, `[mail] wake_gap = "30s"`; `tmp/` files older than 1 h deleted — M§5, M§10, M§11
- `[features] mail`: on in a workspace member, off otherwise — M§8
- HUD segments: shown only for a member, read fresh, never cached, hidden on any read error, never dropped for width; pane key validated (`^k[a-z2-7]{8}$`) before the path; message files never opened — M§9a (H AU25)

Mail sync (S):

- No subject, body, address, host name, member name or path that holds one in a log, `workspace.json`, a usage row, a commit message or a ref name; a log line names ULIDs, pattern ids, check names, blob ids and counts only — S§4 S-C2
- zygo writes only `refs/heads/zygo/<x>` on the remote; a force push only to that branch, only with the S§9 lease; branch form `^zygo/[a-z0-9][a-z0-9-]{0,31}$` — S§4 S-C3, S§11
- Every git call is `git_command` (`crates/zygo-plugins/src/sync.rs:32`) plus `-c gc.auto=0 -c maintenance.auto=false`, under the S§8.7 runner, with a deadline; never `git worktree prune`, `git merge` or `rebase` — S§4 S-C4, S§6.3, S§12
- All remote content is untrusted and validated before use; zygo never writes a file at a path the remote chose; no checkout of the side branch — S§4 S-C5, S§6.3
- Only user config turns sync on; `.zygo.toml` and the team layer cannot; the opt-in is the operator's consent — S§4 S-C6, S-C7, S§11
- No machine deletes or decides about another host's files by its own clock or config; only that host's ack or `--drop-host` — S§4 S-C8
- Clock, child spawn, public probe and named-step hooks are seams — S§4 S-C9
- Exact values: host `^[a-z0-9][a-z0-9-]{0,31}$`; identity `^[0-9a-f]{32}$` (S§5); ULID file `^[0-9A-HJKMNP-TV-Z]{26}\.json$` (S§8.5); marker `ZYGO-MAIL` = `1\n` (S§6.1); commit message `zygo mail [skip ci]`, author `zygo <zygo@zygo.invalid>`, `commit.gpgSign=false` (S§6.5); registry ≤ 64 KiB, ≤ 256 members (S§7.3); ack ≤ 8 MiB, ≤ 200 000 entries, own ack prune trigger > 100 000 (S§7.4); message ≤ 8 MiB (S§8.5); 500 candidates per pass (S§8.5); 1000 outbox files per pass (S§8.3); pass deadline 120 s; 5 push attempts; CLI lock wait 30 s (S§8.3); probe deadline 10 s, public cache 1 h (S§8.2); `sync_gap` default `60s` (S§8.4, S§11); prune at `rev-list --count` ≥ 1000 (S§9); `outbox/tmp/` swept after 1 h (S§9); runner stderr ≤ 64 KiB (S§8.7); imported log 27 bytes per ULID, never trimmed (S§8.5)

Output shaping (O):

- The usage store holds no tool input and no tool output; the command is stored only as a name from the closed list of O§7.2, else `other`; raw output lives only in the session's results directory — O§4 O-C2, O-C3
- A view is used only when `raw_bytes − view_bytes ≥ MIN_SAVING_BYTES` = 256, marker included — O§4 O-C4, O§7.8
- Price multipliers come from the catalog price rows, never from constants in code — O§4 O-C5
- Each new stored field is additive; usage `SCHEMA_VERSION` stays 1 (`crates/zygo-usage/src/store.rs:62`) — O§4 O-C8, O§6.5
- A shaped view hides only lines a shaper positively recognizes as noise; no view drops the text that says why something failed; unsure means keep the line — O§4 O-C9
- The trim hook never makes a Bash call wait more than `HOOK_DB_BOUND` = 100 ms on a busy usage database — O§4 O-C10, O§7.11
- Constants, not config: `SHAPE_MAX_BYTES` = 4194304, `MIN_SAVING_BYTES` = 256, `HOOK_DB_BOUND` = 100 ms, `VIEW_CAP_BYTES` = 8 KiB, `BYTES_PER_TOKEN` = 4, threshold default 16384 bytes — O§9, O§7.3, O§7.7, O§6.2
- `[results] shape = false` gives hook output and stored file byte-equal to those of `97d8767`; shaping is for the Bash tool only — O§9, O§7.1, O§3
- "Trimmed" always means the gross figure; the net figure is always "kept out of context"; the status line and `zygo status` do not change for P3-5 — O§6.2, O§6.7
- Each fixture is real output, scrubbed; at most 256 KiB per case and 4 MiB for `fixtures/shaping/` — O§8.1

Workspace definition (R):

- Nothing from `.zygo/` applies on a machine before the operator approved its digest for that workspace id on that machine; a launch never prompts; only `zygo workspace approve` asks (`--yes` for scripts) — R§4 R-C2, R-C5, R§7.2, R§7.3
- Allow list only: `version` (= 1), `layout` (5 tmux preset names), `members.<name>.{kind, job, profile, pool, cwd}`; profiles `overrides` (≤ 16, keys 1-32 `[a-z0-9-]`, values 1-128 printable ASCII) and `disabled` (≤ 16); any other key at any level makes the whole definition invalid — R§4 R-C3, R§6.2-R§6.4
- Read from git objects of one commit only, never the working tree or index; one 2 s budget for the git calls of one read (`GIT_BOUND`, `crates/zygo-config/src/repo_file.rs:40`); full ref names and commit ids — R§4 R-C4, R§6.1
- Limits: `.zygo/workspace.toml` ≤ 64 KiB; each profile ≤ 16 KiB; ≤ 32 profile files; ≤ 32 members; ≤ 1 orchestrator; profile names `^[a-z0-9-]{1,32}\.toml$`, stem not `none`; `cwd` printable ASCII, parts 1-255 bytes, ≤ 1024 bytes, no `.`, `..`, `\`, absolute or `~` — R§6.1 step 4, R§6.2
- Digest: SHA-256 over path, `0x00`, decimal byte count, `0x00`, bytes, per file in R§6.4 order; 64 lower-case hex, 12 in one-line messages; store `[[workspace]] id digest` in `Paths.approvals_file` with the `approve_in` protocol; zygo never removes an entry — R§6.5, R§7.2
- zygo never reads, copies or logs files in a pool directory (names and types only) — R§4 R-C6, R§9.5
- Repository text on one line passes `zygo_core::esc`; the approve listing escapes every non-printable-ASCII character as `\u{<hex>}`; TOML errors print message and position only — R§4 R-C7, R§7.1 step 6
- No hook, the MCP server or the status line reads `.zygo/`; `load` does not read it — R§4 R-C8, R§8
- `[limits] max_members` 1-32, default 8, user and team layers only; a pinned member keeps its pin, pool directory and passthrough for life — R§9.4, R§7.4, R§9.5

Session run time (T, RT nodes):

- The status-line hook keeps its 3 s deadline; one `workspace.json` read serves phase and start; read rules of H AU25 for the start; `version` stays 1, the new field optional; 0 counts as unknown — `docs/specs/2026-11-24-session-runtime-design.md` T§2.2, T§2.6, T§3.2; `docs/specs/2026-11-21-statusline-hud-design.md` H-C2

Session text (T, SX nodes):

- No change to an agent definition zygo did not write; added text names no context window, size or price; the added text is `mechanics(v)` with the `overrides.json` values applied (default all-on 721 bytes) — `docs/specs/2026-11-24-session-text-design.md` T§4 T-C2, T-C4, T-C5, T§11
- `zygo-` prefix rule = `is_zygo` (`crates/zygo-work/src/hooks.rs:159`); flag file name = `agent_id` passing the work-log agent-name rule (1-64 bytes of `[A-Za-z0-9_-]`, `crates/zygo-work/src/log.rs:141`); re-inject rides only the existing trim entry; plugin calls keep the 10 s bound — T§6.3, T§9.2, T§14, T§15 item 8
- Before ST rows exist the code is built for "no row yet"; tests never read `docs/premises.md` — T§12.1

## Verification floor

- All surfaces (ubuntu-latest, macos-latest, windows-latest): `cargo test --workspace --locked --no-fail-fast`; it now also runs `zygo-mail-sync` and the new test files of every crate; the verdict and sync tests need `git` on `PATH` (present on all three images) and no network (`file://` remotes, injected probe) — `.github/workflows/ci.yml` job `test`; D§17 preamble; S§19 preamble; O§16; R§18; T§7 (RT); T§17 (SX)
- Install script, unix: `sh scripts/test-install.sh` — `.github/workflows/ci.yml` (non-Windows)
- Install script, Windows: `pwsh scripts/test-install.ps1` — `.github/workflows/ci.yml` (windows-latest)
- Local, unix with tmux 3.2 or later, on demand: `cargo test -p zygo-workspace --test mux --test mail_wake --test admin --test up -- --include-ignored` and `cargo test -p zygo --test workspace_mail --test workspace_member -- --include-ignored` — M§13 #1, #2, #5, #6, #7
- Stub self-test, local: `sh scripts/premises/live-selftest.sh` stays green with the `live-mail.sh` cases (stub agents only) — ML-16
- Premises: `sh scripts/check-premises.sh` exits 0 (with `N2_FORBIDDEN_NAMES` set at run time); after MS-25 it covers SP1-SP6 and the `git:` header; after SX-10 the ST rows — S§20; T§12.1 (SX)
- Decision records: after DR-3, delivery of the branch runs `cargo run -p zygo --locked -- decision check` → exit 0 — D§12 item 11
- HUD reference bytes: `cargo test --locked -p zygo-hooks --test statusline_render -- h14_01_golden_files_hold_each_render_byte_for_byte h14_04_emission_examples_1_and_3_are_golden --exact` → both pass with the reference files in `crates/zygo-hooks/tests/golden/` unchanged by RT-3 (other nodes' new golden files, such as OS-27's `shape_off/`, are not in these tests) — `docs/specs/2026-11-24-session-runtime-design.md` T§3.1 last row; `crates/zygo-hooks/tests/statusline_render.rs:249`, `:345`
- Fixture bless, by hand, reviewed in the diff: `ZYGO_BLESS=1 cargo test -p zygo-hooks --test shaping` (OS-35) and `ZYGO_BLESS=1 cargo test -p zygo-hooks --test shaping_off` (OS-27, before OS-28 only) — O§8.1
- On demand, not CI, operator's machine, release build: `cargo test -p zygo-hooks --release --test shaping_latency -- --ignored` (OS-38); the numbers go in OS-38's pull request — O§8.5, O§16 item 17
- Live checks (manual, recorded by the operator, never in CI, never by an agent): PD2, PD3 (DR-15); PM1-PM3 (ML-17), M12 (ML-19); SP1 (MS-23), SP2 (MS-24); PO1-PO4 (OS-23 to OS-26); PR1, PR2 (WD-21, WD-22); ST1-ST4 (SX-9) — D§18; M§13 #12, M§14; S§20; O§17; R§19; T§12.2 (SX)

## Review focus

1. A `PostToolUse` `agent_id` such as `../x` or `../view.json` joined to `reinject/` — the trim side applies the same agent-name rule as pre-compact (through `flag_path`) before any join, and removes nothing outside `reinject/` — test in SX-4 — `docs/specs/2026-11-24-session-text-design.md` T§9.2 item 2, T§14
2. The flag check placed after the trim's own early returns (non-hooked shape, results off) — the check runs first; under-threshold and merged over-threshold objects both carry `additionalContext`; a second run prints nothing — test in SX-4 — T§9.2 item 2 (SX)
3. Approval bypass: a use that checks one read and applies another, or applies a partial definition — one read, one digest, one lookup per use — tests in WD-4, WD-11 — `docs/specs/2026-11-24-workspace-definition-design.md` R§7.3, R§6.4, R§10 `up` row
4. Two readers and a sender on one maildir at once — each message read once, no partial file in `new/` — test in ML-2 — `docs/specs/2026-11-22-workspace-mail-design.md` M§5, M-C3
5. Wake safety: a pane whose `@zygo-pane` or session `@zygo-ws` differs, any text but `WAKE_TEXT`, two senders inside the gap — no wake, no other text reaches `send-keys`, the gap holds under the lock — tests in ML-6, ML-5 — M§7.3, M-C5
6. Stop loop — block only with `stop_hook_active = false`; `busy` kept while blocking and cleared otherwise — test in ML-10 — M§7.1 (AU43)
7. An unreadable `mail/` in any new hook branch — exit 0, no decision change (C6) — test in ML-10 — M-C1, M§10
8. Hostile entry names in `ls-tree -z` output (tabs, newlines, `..`, very long names, names outside an address folder) — never reach a filesystem path — tests in MS-12, MS-10 — `docs/specs/2026-11-24-mail-sync-design.md` S-C5
9. A starter's mark written after the holder's look but before its release, and a starter that fails the lock between release and look — no lost pass — test in MS-13 — S§8.3 steps 2-4, 17; S§19 #16
10. The prune base older than this pass's own push; branch form and marker not re-checked right before the force push — the lease is that base; both re-checked — test in MS-14 — S§9
11. `git cat-file --batch` on `<rev>:<path>` returns a symlink as an ordinary blob — the reader parses the tree entry mode and refuses a link or submodule — test in DR-11 — `docs/specs/2026-11-24-decision-records-design.md` D§6.8
12. A node already merged (`merge-base HEAD <head>` = `head`) — steps 5 to 9 are skipped, not reported — test in DR-12 — D§9.4 step 4
13. Proofs bound to the node: a `fixed` commit that is an ancestor of `base`, a record present at `base`, one record for two findings — each refused — test in DR-12 — D§8.1
14. A stop-class `recorded` entry in round 1 — gives the stop problem, not the round-limit problem — test in DR-12 — D§8.2, D§17 #7
15. Merge commits and commits without a `Zygo-Session:` trailer (the operator's ratify commit); `Zygo-Session:` only in body text — not read; the body-text case is decided in DR-12's plan — test in DR-12 — D§9.4 step 9
16. At `enforce` a refused `keep` — `work_units.verdict` unchanged (no `set_verdict` before the check); argument refusals come before the level is read; a missing `head`, `base` or `dispositions` is a guard problem; `monitor` makes no git call — test in DR-13 — D§9.2, D§9.3
17. Delivery crash windows: `finish_incoming` when the ULID is already in the log and the rename target exists; a torn last line of the imported log — test in MS-12, MS-5 — S§8.5
18. A shaper view that also hides a failure line (`test x ... ok` printed by a failing test, a pytest `-v` `XPASS(strict)` line, a go `--- SKIP` around `t.Fatal`, a go line given to the wrong test after `=== NAME`) — the failure text stays — tests in OS-30, OS-31, OS-32 — `docs/specs/2026-11-24-output-shaping-design.md` O§7.6, O-C9
19. A file printer the command parse misses (nested `-c`, `$( )` in double quotes, `xargs -I{} cat`, `find -execdir`, `command cat`, `env FOO=1 cat`, `/bin/cat`) — no shaped text the agent may copy into `Edit` — test in OS-11 — O§7.1
20. The use rule checked on a view without its marker; `catch_unwind` that leaves the size view off for large output; stderr counted when PO2 failed; a `Store::put` error that still writes a row — test in OS-28 — O§7.8, O§7.10
21. The hook adding a column (write lock on every Bash call); the seven-column fallback hiding an error other than "no such column"; a bounded open that waits longer than `bound` — test in OS-2, OS-3 — O§6.5, O§7.11
22. Carry: the cut compared with the wrong previous row; an expiry on a growing context; a read-back keyed by the reading session; OpenCode subagent rows carrying a main-loop trim; an unpriced request counted twice — tests in OS-6, OS-7 — O§6.3, O§6.4
23. A read-back row write on a busy store that changes the reply text or its error; a row for an error reply — test in OS-5 — O§6.4, O-C1
24. `Exit code N` line hidden, cut or counted; `trim` not first under `PostToolUseFailure` (for a member, with work units on, and after SX-5 adds `SubagentStart`) — test in OS-33 (order kept in SX-5) — O§7.13; T§6.3 item 1 (SX)
25. Shape-off goldens blessed after shaping code exists — blessed in OS-27 on the `97d8767` behavior — test in OS-27 — O§9
26. A capture that keeps a home path, user name or host name; a CRLF capture normalized by git before `-text` is in `.gitattributes` — tests in OS-16, OS-23 — O§8.1
27. CRLF changed in reuse text; an OSC with no end that eats the rest of the output — test in OS-12 — O§7.4
28. Dedup at the end of text without a trailing newline — lossless — test in OS-13 — O§7.5
29. Short ref names or option injection in git calls; `origin/HEAD` outside `refs/remotes/origin/` — refused — test in WD-1 — R§6.1 step 2
30. Content read before the size and mode checks; link and submodule modes — refused before any content read — test in WD-1 — R§6.1 step 4
31. A key outside the allow list at any depth (inline tables included); `cwd` look-alike characters — the whole definition invalid — test in WD-2 — R§6.2-R§6.4, R-C3
32. Approval keyed by digest alone; a plugin approvals file broken by `[[workspace]]` entries — keyed by id and digest; plugin approvals still parse — test in WD-3 — R§7.2, R§18 item 21
33. A resume that reads the default branch again, or starts without its pinned profile — the pin is used — test in WD-14 — R§7.4
34. Pool fallback to the default config directory; a pool file opened; a resume that reassigns — none of these — tests in WD-9, WD-12 — R§9.5, R-C6
35. Writes before the R§9.4/R§9.5 refusals (state file, tmux session) — nothing written — tests in WD-11, WD-12 — R§10 `up` row
36. Terminal sequences or bidi controls in the approve listing or a one-line message — escaped — test in WD-15 — R§7.1 step 6, R-C7
37. A user profile or user `[loadout.repository]` shadowed by the repository; `--layer repository` showing repository-file keys — test in WD-7 — R§8, R§10 `config show`
38. A hook or `load` that makes a new git call — none — tests in WD-19, WD-7 — R-C8
39. Byte-equal preservation with CRLF line ends, comments between keys and unknown tables; the re-read-then-rename race — tests in DR-5 — D§6.2, D§17 #14, D§11.3
40. A `class` change to `none` or a `kind` change after creation — refused as frozen — test in DR-10 — D§6.3
41. ESC, CR, bidi and line separators in a record title, body, note or commit subject — printed escaped — tests in DR-3, DR-6 — D§10.1
42. Links and FIFOs in `docs/decisions/` — refused; tests under `#[cfg(unix)]` as `crates/zygo-hooks/tests/workspace.rs:642` — tests in DR-1, DR-2, DR-3 — D§6.8
43. `/zygo:work report` inside the `UserPromptSubmit` hook — reads no `docs/decisions/` file — test in DR-7 — D-C2
44. A `new` write failure — leaves at most the `.tmp` file, which every reader ignores — test in DR-4 — D§10.2
45. `act_lite_store::read` returns an untyped error for a missing store — "no store" gives regex-only patterns; unreadable or unparsable gives `CouldNotRun`; `zygo-redact` unchanged — test in MS-7 — S§8.6, S§17
46. `run_bounded_out` sets a process group only in cut mode today (`crates/zygo-work/src/repo.rs:75`) — both new modes set it; existing `run_bounded`/`run_bounded_err` callers keep their behavior — test in MS-1 — S§8.7
47. Own ack upkeep across own hosts after a rename — a ULID leaves the ack only when no `*@<own host>/` file for it remains at the tip — tests in MS-13, MS-15 — S§7.4, S§7.6
48. A crash between recording "about to push" and the push — the pending id does not raise `Twin` — test in MS-11 — S§7.5
49. The hook path of the sync trigger — reads config and `last_start` only, runs no git, adds no output; a failed spawn changes no decision — test in MS-16 — S§8.1, S-C1
50. `git push --porcelain` exits non-zero on a rejected ref with the reason on stdout — another ref's line or a remote message is not read as "moved" — test in MS-10 — S§8.3 step 15, S§8.7
51. A collision line that names another local workspace — goes to the CLI only, never to `errors.log` — tests in MS-11, MS-13 — S§7.5, S-C2
52. Subject and body limits in bytes, not characters; a hostile `sent_at` — refused — test in MS-12 — S§8.5
53. `zygo mail sync --background` on a panic — skips the results sweep and prints nothing — test in MS-17 — S§17 row `crates/zygo/src/cmd/mod.rs`, AU71, AU44
54. Windows build of `zygo-mail-sync` (`pre_exec`/`setsid`, file modes, fs4) — compiles; tests compile to nothing — tests in MS-2, MS-16 — S§3 (OP5)
55. No mail content outside `mail/`: no subject or body in state, logs, usage, `errors.log`, HUD, commits or refs; the HUD opens no message — tests in ML-4, ML-15, ML-18, MS-22 — M-C2, M§9a; S-C2
56. Name trust: names only from user config, the CLI and approved repository files; AU84 check at each read; reserved names refused — tests in ML-1, ML-3, ML-7, ML-8 — M-C6, M§6
57. Atomic state edits: name copy in `up`, snapshot keeps `Pane.name`, `ws name` under `lock` — tests in ML-3, ML-8 — W§7.3, AU78
58. Feature gate: tools listed only for a member with `features.mail` on; item 8 only then — tests in ML-9, ML-11 — M§8, M§8a
59. The exit commit in `record_exit` (`crates/zygo/src/cmd/member.rs:472`) sets `first_started_at` only on the branch that takes `pending`; a hook-committed record keeps it; `Exit::NotStarted` never sets it — test in RT-1 — `docs/specs/2026-11-24-session-runtime-design.md` T§3.2 (W AU92)
60. `pending.started_at = 0` leaves the field absent; a later non-zero commit sets it; a replaced hand-start record starts without it, a kept one keeps it — test in RT-1 — T§3.2, T§4, T§5
61. A record whose `zygo_session` and `pending.zygo_session` both differ from `view.session` — the session's own ULID start — test in RT-2 — T§2.2 rule 3
62. A member whose `workspace.json` fails any read check — no start at all, no ULID fallback — test in RT-2 — T§2.2 rule 2
63. `u64::MAX` and unit floors in the run-time form — no overflow, two-digit second unit — test in RT-2 — T§2.3
64. The `runtime:` line placement — right after `session:` in the `Ok`, `unreachable` and `hide` (C7) forms — test in RT-2 — T§2.5
65. `collect` reads `workspace.json` once for phase and start — the old `phase::read` call is replaced, not kept beside it — test in RT-3 — T§2.6, H§7.6 amendment
66. The run time as a local segment — `stale` does not dim it, `ended` does not drop it; the Nerd clock glyph counts 2 cells in the fit — test in RT-3 — T§2.4, H AU20, H§6.7
67. Both early returns of `session_start` (`run_in`, `run_member`) replaced; `agent_type` `Explore`, `Plan`, `general-purpose`, `p315:probe`, `1`, missing, `zygo-code`; a subagent `SessionStart` with `source: "compact"` prints nothing — test in SX-1 — T§6.3 item 2, T§17 checks 3, 5
68. Main-loop `source: "compact"` output equals `source: "startup"` output — test in SX-1 — T§9.1, T§17 check 6
69. A `SubagentStart` with an unparseable `view.json`, through the C6 wrapper — exit 0, empty stdout, one stderr line — test in SX-1 — T§14 item 1, T§17 check 12
70. `--event subagent` — returns before any member part: no resume note, `view.json` byte-equal — test in SX-2 — T§7.2 item 2, T§17 check 11
71. `pre_compact::run` ignores its input today — the main input, `zygo-` types and bad ids create nothing; a create failure still prints the §10.6 text — test in SX-3 — T§9.2 item 1
72. `SubagentStart` member order — `session-start` first, then `workspace --event subagent-start`; no `--event` flag and no matcher — test in SX-5 — T§6.3 item 1
73. Unset `ZYGO_MAIN_ROLE` with a `zygo` model — goes to `--event subagent`; a non-`zygo` provider calls nothing — test in SX-6 — T§7.2 item 1
74. The `ST*)` arm of `scripts/check-premises.sh` — before the `P*)` arm, with the `P3-15` failed-note pattern and `$SSPEC`; the loop over `$IDS $WIDS $SIDS` — test in SX-10 — T§12.1 item 4
75. Any new test that spawns `claude`/`opencode` or reads a clock — none (review of the diff) — tests in SX-1 to SX-6, and in the review gate of each node that adds a test — T§17 check 15 (SX); O-C6, O-C7; S-C9; R-C9

## Nodes

| ID | Node | Depends | State | Capability | Refs |
|----|------|---------|-------|------------|------|
| DR-1 | Record format: parse and validate one DDR file, and `docs/decisions/` in place (README with a valid example) | — | DONE | judgment | 26be671 9612477 85e0c43 · decision_record 30/30 · PR #17 |
| DR-2 | Record set: directory reader, set checks and the dependency graph | DR-1 | DONE | implementation | 7e67ae4 d4007c2 168426f · decision_set 20/20 · PR #20 |
| DR-3 | `zygo decision` verb: `ls`, `show`, `deps`, `check` and the slash form | DR-2 | DONE | implementation | 683c800 46baa07 2334e49 f1ffb87 0b60e99 · decision_cli 17p/0f/6i, cli_surface 20p/0f/3i; floor green; decision check 0 · PR #22 |
| DR-4 | `zygo decision new`: write a record template | DR-3 | DONE | implementation | f0d049d beeaa1d 3965c61 · decision_new 12p/0f/1i; floor green · PR #25 |
| DR-5 | Operator verbs `ratify` and `overturn`: byte-preserving outcome writes | DR-4 | DONE | judgment | 397edc5 aba92f9 d3e3ff7 0db609f 4f86964 2b072a7 · decision_verbs 25p/0f; floor green · PR #29 |
| DR-6 | `zygo decision review`: the phase-end review | DR-5 | DONE | implementation | 732af80 82d2e65 cb609a4 · decision_review 21/0; floor green · PR #34 |
| DR-7 | `zygo work report`: the two record lines | DR-2, DR-3, RT-2 | TODO | implementation | — |
| DR-8 | Session view `repo_root`, written by the launcher | DR-1 | DONE | mechanical | c806e21 88441c1 · repo_root 2/2 core, 3/3 launcher · PR #19 |
| DR-9 | The `dispositions` guard key, default `enforce` | DR-1 | DONE | mechanical | 34a088d fb104db · dispositions 4 tests · PR #18 |
| DR-10 | Agent transitions: the D§6.6 rule for one record change | DR-1 | DONE | judgment | 9efee7e 281dd03 · decision_transition 45/45 · PR #21 |
| DR-11 | Verdict git reader: four bounded git processes behind an injectable trait (PD1) | DR-10, MS-1 | DONE | judgment | 52a5006 b306476 5e3b797 196e814 0add171 · decision_git 33/33; floor green · PR #35 |
| DR-12 | Keep check: the D§9.4 steps over the node range | DR-11 | DONE | judgment | e0e426b 05c4148 43c44e7 7acba21 · decision_verdict 41/41; floor green · PR #37 |
| DR-13 | `work_verdict`: `head`, `base`, `dispositions` input and the `dispositions` guard | DR-7, DR-8, DR-9, DR-12 | TODO | judgment | — |
| DR-14 | End-to-end gate: the stop path, the merged-node keep and no stored text | DR-5, DR-13 | TODO | implementation | — |
| DR-15 | Live check: PD2 and PD3 on the pinned Claude Code (operator, by hand) | SX-8 | TODO | mechanical | — |
| ML-1 | Mail config keys, the loadout pane `name` and the member-name grammar | DR-9, DR-8 | DONE | implementation | c3c7337 da263a1 · config_layers 90p/0f/16i, core_basics 24p/0f/3i; floor green · PR #23 |
| ML-2 | Maildir store: message file, atomic send, read by rename, limits, sweep | DR-1 | DONE | judgment | 2550471 e7fed8c 24c9124 · mail_store 38p/0f/0i; floor green · PR #24 |
| ML-16 | Live-check script for PM1-PM3 and acceptance #12 | DR-1 | DONE | implementation | a8add6a 3b33343 957db39 · live selftest ok (76); floor green · PR #26 |
| OS-1 | Result meta records the session that stored it | DR-1 | DONE | implementation | 7d285bd f733e6c 8a21ec2 · results 50p/0f, hooks_basic 55p/0f; floor green · PR #27 |
| OS-6 | Carry computation for one event over its context's requests | DR-1 | DONE | judgment | 32286a7 2ac0b35 · carry 19p/0f; floor green · PR #30 |
| OS-10 | `shape` module: step names, shaper names and byte bounds | DR-1 | DONE | mechanical | 9f2cd74 · shape tests 3 passed; floor green · PR #31 |
| OS-16 | Shaping fixture harness: byte-exact files, capture script, format and secret checks | DR-1 | DONE | implementation | a09ce9f ee1a671 3686f01 bca8cad def8f45 3b71a13 701d9e1 · shaping_fixtures 22 pass; floor green · PR #32 |
| MS-1 | Bounded git runner: whole and stream modes, public in zygo-work | DR-1 | DONE | judgment | e7d5023 821a65f 7017674 ab87c30 · git_runner 17/0, zygo-work ok; floor green · PR #33 |
| MS-3 | Mail sync config keys, forms and per-workspace resolution | ML-1, WD-5 | TODO | implementation | — |
| WD-1 | Git read of `.zygo/` from the default branch and at a pinned commit | DR-1 | DONE | judgment | 35ffa73 e7c51d9 · repo_def_read 27/27; floor green · PR #36 |
| WD-5 | Config keys: pane `job`/`pool`, `[workspace] pool`, `[limits] max_members`, `.zygo.toml` filter, `config check` rules | DR-3, ML-1, RT-1 | TODO | implementation | — |
| RT-1 | Member record `first_started_at`, set by each commit of a pending start | DR-1 | TODO | implementation | — |
| RT-2 | Run-time start rule, `<form>` text, and the `runtime:` line of `zygo status` | ML-1, WD-5 | TODO | implementation | — |
| SX-1 | Claude Code subagent branch of `hook session-start`: mechanics text on `SubagentStart` for every non-`zygo-` subagent (T§6.3 item 2) | DR-1 | DONE | implementation | 6a979de 346fa1a · subagent_text 10 pass (9 subagent_start, 1 main_compact); floor green · PR #39 |
| SX-3 | `hook pre-compact` sets `reinject/<agent_id>` for a non-`zygo-` subagent (T§9.2 item 1) | DR-1 | IN-PROGRESS | implementation | .worktrees/phase3/SX-3-node · PR #41 |
| SX-5 | Claude Code launch registers `SubagentStart` → `hook session-start` on every launch (T§6.3 item 1) | OS-33 | TODO | implementation | — |
| SX-8 | `docs/live-checks.md` section P3-15 (T§12.2) | DR-1 | TODO | mechanical | — |
| ML-17 | Live premises PM1-PM3 recorded (operator run) | ML-16 | TODO | mechanical | — |
| MS-2 | zygo-mail-sync crate: hardened git calls and test seams | MS-1, ML-2, DR-5 | TODO | implementation | — |
| OS-2 | Usage store: `readbacks` table and three additive `trims` columns | OS-1 | TODO | judgment | — |
| OS-11 | Command parse: segments, file printers, stored name, runners, reuse text | OS-10 | TODO | judgment | — |
| OS-14 | Size view over any text, and the shaped marker | OS-1, OS-10 | TODO | implementation | — |
| OS-17 | Capture clean and dedup fixtures from real command output | OS-16 | TODO | implementation | — |
| OS-18 | Capture diff, json and log fixtures from real command output | OS-16 | TODO | implementation | — |
| OS-19 | Capture libtest fixtures from a scratch crate | OS-16 | TODO | implementation | — |
| OS-20 | Capture pytest fixtures from a scratch project | OS-16 | TODO | implementation | — |
| OS-21 | Capture go test fixtures from a scratch module | OS-16 | TODO | implementation | — |
| OS-22 | Premise script for PO1 to PO4 | OS-16 | TODO | implementation | — |
| WD-2 | Typed shapes, allow-list validation and digest of the workspace files; PR3 | WD-1 | TODO | judgment | — |
| SX-2 | OpenCode `session-start --event subagent`: mechanics text as plain text, no member part (T§7.2 item 2) | SX-1 | TODO | implementation | — |
| SX-9 | Live check P3-15, run by hand by the operator: evidence for ST1-ST4 | SX-8 | TODO | mechanical | — |
| ML-3 | Member name in the plan, the member record and `panes[]` | ML-1, RT-1 | TODO | implementation | — |
| MS-7 | Secret pass on one message | MS-2 | TODO | implementation | — |
| OS-3 | Bounded usage-store open for the hook and the MCP server | OS-2 | TODO | judgment | — |
| OS-7 | Savings read: net figures, per-context carry, breakdown and ratio | OS-2, OS-6 | TODO | judgment | — |
| OS-12 | Clean step: ANSI escapes and carriage-return progress | OS-11 | TODO | implementation | — |
| OS-23 | Premise PO1: where a failed Bash call's output arrives, and what replaces it (operator, live Claude Code) | OS-22, ML-17 | TODO | mechanical | — |
| WD-3 | Workspace approvals in `approvals.toml` | WD-2 | TODO | judgment | — |
| SX-6 | OpenCode plugin transform: `--event subagent` for non-main `zygo/` models (T§7.2 item 1) | SX-2 | TODO | implementation | — |
| ML-4 | Address resolution and the send operation | ML-2, ML-3 | TODO | implementation | — |
| ML-5 | Multiplexer operations for the wake: literal keys and Enter | ML-3 | TODO | implementation | — |
| ML-7 | Member name in the session view marker, hand start `--name` | ML-3, DR-3, DR-8 | TODO | implementation | — |
| MS-5 | Sync state files: paths, identity, state.json, imported log, marks | MS-2, WD-5 | TODO | judgment | — |
| OS-4 | Trim hook writes `result_id` and `shaper`, opens the store bounded | OS-3 | TODO | implementation | — |
| OS-5 | MCP result tools record one read-back row per successful reply | OS-3 | TODO | implementation | — |
| OS-8 | `zygo usage` prints the net figures, breakdown and ratio | OS-7 | TODO | implementation | — |
| OS-13 | Dedup step: runs of equal consecutive lines, lossless | OS-12 | TODO | implementation | — |
| OS-24 | Premise PO2: an empty `stderr` in `updatedToolOutput` removes the stderr (operator, live Claude Code) | OS-23 | TODO | mechanical | — |
| WD-4 | Checked use: one read, one digest, one store lookup per use; `Pin` | WD-3 | TODO | judgment | — |
| WD-9 | Pool directory listing, assignment and shared lines | WD-5, ML-2 | TODO | judgment | — |
| ML-6 | Wake of an idle member after a send | ML-4, ML-5, ML-17 | TODO | judgment | — |
| ML-8 | `zygo ws name <key> <name>` | ML-5, ML-7 | TODO | judgment | — |
| ML-9 | Session text item 8 (workspace mail) at main-loop SessionStart | ML-4, ML-7, DR-8, RT-1 | TODO | implementation | — |
| ML-15 | HUD segments: member name and unread mail | ML-7, RT-2 | TODO | implementation | — |
| MS-4 | Address grammar and the ULID file form | MS-2, MS-3 | TODO | implementation | — |
| MS-8 | Public check: URL map, anonymous probe, verdict lists, cache | MS-5 | TODO | judgment | — |
| MS-10 | Side branch plumbing: index-only worktree, fetch, bootstrap, commit, push | MS-5 | TODO | judgment | — |
| OS-15 | Shaping pipeline: step order, reuse rule, byte bound | OS-13 | TODO | judgment | — |
| OS-25 | Premise PO3: OpenCode `output.output` holds stdout and stderr (operator, live OpenCode) | OS-24 | TODO | mechanical | — |
| WD-6 | The repository loadout from `[members]`, with the default set | WD-4, WD-5 | TODO | implementation | — |
| WD-8 | Member record fields `launch.job` and `launch.definition` | WD-4, ML-3 | TODO | mechanical | — |
| ML-10 | Delivery by hooks: Stop block, prompt line, SessionStart line, OpenCode per-request line | ML-9, ML-17 | TODO | judgment | — |
| ML-11 | MCP tools `mail_send`, `mail_read`, `mail_list` | ML-6, ML-7 | TODO | implementation | — |
| ML-12 | CLI `zygo mail send\|read\|ls`, slash `/zygo:mail`, mail sweep at command start | ML-6, ML-7, DR-3 | TODO | implementation | — |
| ML-14 | Unread counts in `zygo workspace`, `zygo ls` and `up`; `forget-pane` removes the maildir | ML-2, ML-8 | TODO | judgment | — |
| MS-6 | Registry and ack files, validation, member cache | MS-4, MS-5 | TODO | judgment | — |
| MS-24 | Live premise SP2: anonymous probe on github.com and gitlab.com | MS-8 | TODO | lookup | — |
| OS-9 | `[results] shape` in config and the session view | OS-4, ML-9, MS-3 | TODO | mechanical | — |
| OS-26 | Premise PO4: the model sees a `result_view` reply plus at most a fixed wrapper (operator, both agents) | OS-25 | TODO | mechanical | — |
| OS-29 | Kind views for diff, json and log (pipeline step 4) | OS-15, OS-18 | TODO | judgment | — |
| WD-7 | Approved definition added to the merged config with source `workspace-files` | WD-6, MS-3 | TODO | judgment | — |
| WD-10 | Plan panes carry `job`, `pool` and the pin; `Source::Repository` | WD-6, ML-3 | TODO | implementation | — |
| RT-3 | Run-time HUD segment in compact and verbose, with its place in the width-removal order | RT-2, ML-15 | TODO | implementation | — |
| ML-13 | `zygo mail watch` | ML-12 | TODO | implementation | — |
| MS-9 | Outbox: send-time rules, refusal and notice, counts, local retention | MS-6, MS-7, MS-8, ML-4 | TODO | judgment | — |
| MS-11 | Tip metadata, own hosts, collision and twin checks | MS-6, MS-10 | TODO | judgment | — |
| MS-12 | Import: candidates, checks, crash-safe delivery | MS-6, MS-10, ML-2, ML-4 | TODO | judgment | — |
| MS-16 | Sync child start and rate limit; member hooks start it | MS-3, MS-5, ML-10, DR-5 | TODO | implementation | — |
| OS-27 | Shape-off baseline: hook output and stored file of the `97d8767` behavior | OS-9, OS-17, OS-18, OS-19, OS-20, OS-21 | TODO | implementation | — |
| OS-30 | Runner views: dispatch and the libtest view (pipeline step 3) | OS-29, OS-19 | TODO | judgment | — |
| WD-11 | `zygo up`: loadout choice, checked read, pin, member maximum | WD-7, WD-8, WD-10, ML-14, MS-5 | TODO | judgment | — |
| WD-13 | Launcher: repository profile by a default-branch read (not pinned) | WD-7, DR-13, OS-9 | TODO | judgment | — |
| WD-17 | `zygo doctor` lines: workspace files, pool, member maximum | WD-7, WD-9 | TODO | implementation | — |
| ML-18 | End-to-end mail on tmux with stub agents | ML-10, ML-11, ML-13, ML-14 | TODO | implementation | — |
| MS-13 | The sync: gate, mark, lock, passes, push retry, wake | MS-9, MS-11, MS-12, ML-6 | TODO | judgment | — |
| OS-28 | Trim hook runs the shaping pipeline on Bash output | OS-14, OS-15, OS-27 | TODO | judgment | — |
| OS-31 | pytest runner view | OS-30, OS-20 | TODO | judgment | — |
| WD-12 | `zygo up`: pool check, assignment and passthrough at a fresh build | WD-9, WD-11 | TODO | judgment | — |
| ML-19 | Live acceptance #12: a member mails by name with no prompt about mail (operator run) | ML-18, SX-8, DR-15 | TODO | mechanical | — |
| MS-14 | Prune: orphan rewrite of acknowledged files, lease push | MS-13 | TODO | judgment | — |
| MS-20 | Session text: "Other machines" clause of item 8 | MS-6, MS-16, MS-13 | TODO | mechanical | — |
| OS-32 | go test runner view with interleaved parallel output | OS-31, OS-21 | TODO | judgment | — |
| OS-33 | Failed Bash calls on Claude Code, as PO1 decided | OS-28, OS-23 | TODO | judgment | — |
| MS-15 | Operator commands: take-host, new-identity, rename, drop-host | MS-14 | TODO | judgment | — |
| OS-34 | Set the stderr branch as PO2 decided | OS-33, OS-24 | TODO | mechanical | — |
| WD-18 | Session text: each member's job and the job sentence | WD-11, ML-9, ML-10, MS-20 | TODO | implementation | — |
| MS-17 | `zygo mail sync` verb, options, hidden background form | MS-15, MS-16, ML-12, ML-13 | TODO | implementation | — |
| OS-35 | Fixture views through the hook: goldens, floors, recovery, bless guard | OS-34, OS-32 | TODO | judgment | — |
| SX-4 | `hook trim` takes the flag and adds `additionalContext` = mechanics text, once (T§9.2 item 2) | SX-1, SX-3, OS-34 | TODO | judgment | — |
| MS-19 | MCP mail tools: addresses, send-time pass, remote mail_list | MS-17, ML-11, WD-18 | TODO | implementation | — |
| MS-23 | Live premise SP1: hook-started child outlives the hook | MS-17 | TODO | lookup | — |
| OS-36 | The never-worse check over every case and the built inputs | OS-35 | TODO | judgment | — |
| OS-37 | On-demand latency test, ignored in CI | OS-35 | TODO | implementation | — |
| WD-14 | Pinned launches: resume and `ws run` resolve the profile from the pin | WD-12, WD-13, MS-17 | TODO | judgment | — |
| MS-18 | CLI send/read/ls/watch with addresses; escaped text; doctor twin line | MS-17, ML-13, MS-19, WD-17 | TODO | implementation | — |
| MS-21 | `zygo ws forget`: outbox question and sync worktree removal | MS-17, WD-12, WD-14 | TODO | implementation | — |
| MS-25 | Record premises SP1-SP6 and the pinned git version | MS-14, MS-23, MS-24, OS-26 | TODO | mechanical | — |
| OS-38 | Measure the latency budget on the operator's machine (operator, release build) | OS-37 | TODO | lookup | — |
| WD-19 | Hooks, MCP server and status line read nothing from `.zygo/` | WD-14 | TODO | implementation | — |
| WD-21 | PR1 live check (operator): two pool members send two logins | WD-14, ML-19 | TODO | mechanical | — |
| WD-22 | PR2 live check (operator): per-session settings act the same on a pool directory | WD-14, WD-21 | TODO | mechanical | — |
| MS-22 | No mail content in logs, state, usage rows, commits or refs | MS-18, MS-19, MS-21 | TODO | implementation | — |
| WD-15 | `zygo workspace approve [--yes] [--digest <hex>]` | WD-14, DR-3, MS-21 | TODO | judgment | — |
| WD-16 | `config show/check`, `profile list/show`, `loadout list/show` show the repository files | WD-15 | TODO | implementation | — |
| WD-20 | User docs: workspace files, approval, members, pool, passthrough | WD-12, WD-15, WD-16, WD-17 | TODO | mechanical | — |
| SX-7 | Base-spec amendments §8.5, §8.6 item 7, §8.6 last paragraph, §8.7 N33, §10.6 (T§13) | SX-4, SX-5, SX-6, WD-16 | TODO | implementation | — |
| OS-39 | Base spec text for output shaping and saving accounting | OS-8, OS-26, OS-34, SX-7 | TODO | mechanical | — |
| SX-10 | Record ST1-ST4: fixtures, premise rows, `check-premises.sh` ST arm, code change per result (T§12.1 items 1-5) | SX-7, SX-9, MS-25, OS-39 | TODO | judgment | — |

## Log

| Date | Event |
|------|-------|
| 2026-11-24 | Graph assembled from seven track fragments (DR, ML, MS, OS, WD, RT, SX); 133 nodes; DR-1 root. |
| 2026-11-24 | Audit round 1 remediated (9 findings): golden test target, operator nodes mechanical with Files, byte-preserving mail delivery, SX-4 tools, test counts, ML-8/ML-14 judgment, DR-11 bounds, runnable live commands, citations. |
| 2026-11-24 | DR-1 done. Review round 1, 7 findings: F1, F2, F3, F6, F7 fixed (9612477); F4a, F4b, F5 rejected (reasons in nodes/DR-1.md). D§6.2 example commit fixed to 40 hex (3511ad5). Windows-only code compiles only in CI. |
| 2026-11-24 | DR-8 done: launcher writes `SessionView.repo_root`. Review round 1, 6 findings: 4 fixed (clock-free outside-repo test, TMPDIR independence, doc comment, non-UTF-8 test); 2 rejected (in-repo git test needs the 2 s clock bound; bad field fails the load like all fields). Floor green. |
| 2026-11-24 | DR-2 done: record set reader, set checks, dependency graph. D§17 #4 corrected to 4 dependents (depths 1, 2, 3, 3). Review round 1, 7 findings: 5 fixed (count speed, deepest_chain order, duplicate lines, non-UTF-8 names, linked docs); 2 rejected (DFS cycle_at; races). Floor green. |
| 2026-11-24 | DR-9 done: key `dispositions`, default `enforce`. Round 1, 5 findings: 1 fixed (CI tests read no git); 3 rejected (repo-file, serve_cli: ignored timing tests, pass locally; trailer per session rule); 1 out of scope (agents, repos lower the gate; D§9.3 allows; DR-13). Floor green. |
| 2026-11-24 | DR-10 done: agent_change, the D§6.6 rule for one record change. Review round 1, 8 findings, all fixed: malformed old refused, self-supersede, model/commit/date/supersedes frozen, name escaped, Writes column exhaustive, id match. Floor green. |
| 2026-11-24 | DR-3 done: zygo decision ls\|show\|deps\|check, trailer commits, slash reply. Review 7 findings: 6 fixed (git config independence, linear ls, unborn HEAD, --help, test split, slash args), 1 refuted (D-C9). PR #22. |
| 2026-11-24 | ML-1 done: [mail] keys, pane name, member-name grammar, d duration unit. Review 6 findings: 5 fixed (pane name scrubbed, pane-key-form names reserved (M§6 author's call), no name on shell panes, profile features.mail test, note), 1 left to ML-6/ML-12 (checked arithmetic). PR #23. |
| 2026-11-24 | ML-2 planned (016912f); Files += zygo-workspace/Cargo.toml, Cargo.lock for the ulid dep. PR #24. |
| 2026-11-24 | ML-2 done: maildir store, link send, link read to cur/, limits, sweep. Review 16 findings: 11 fixed (no-replace, O_NOFOLLOW, 8 MiB cap, 0700 root, cur mtime, fsync, tmp=Duplicate, sweep, unparsable, M-C3 link); 5 deferred (MS-17, ML-15, ML-14 notes; 500 race; ULID order). PR #24. |
| 2026-11-24 | OS-1 planned (149d9ed); Files += zygo-hooks/tests/hooks_basic.rs for the trim_session test. PR #27. |
| 2026-11-24 | DR-4 done: zygo decision new. Round 1, 6 confirmed: --help as title, fill marker in TOML comment, no ## Finding for stop class, 3 test gaps, git test isolation; top-level help (cmd/mod.rs:361) omits `new`, carried to first of ML-7/ML-12/MS-17/WD-14 to edit it. 3 rejected. PR #25. |
| 2026-11-24 | OS-1 done: result meta records the session (`Store::put` takes `session: Option<&str>`, trim passes it). Review 4 findings: 3 fixed; 1 rejected (non-string session; no writer produces one, O-C8 fixes the type). PR #27. |
| 2026-11-24 | DR-5 planned (d6efc16): 11 steps, two test-first cycles; Files widened (zygo-work Cargo.toml, Cargo.lock, decision_cli/decision_new USAGE); MS-2 and MS-16 now depend on DR-5 (Cargo.lock overlap). PR #29. |
| 2026-11-24 | OS-6 planned (9f6b378): 6 steps, 13 tests. PR #30. |
| 2026-11-24 | OS-6 done (32286a7 2ac0b35): carry 19p/0f; review 5 confirmed, fixed; floor green. PR #30. |
| 2026-11-24 | DR-5 done (397edc5 aba92f9 d3e3ff7): ratify and overturn, byte-preserving writes; decision_verbs 22p/0f. Review round 1, 6 findings: 4 fixed (record lock, mixed line endings refused, empty texts in allowed, test gaps); 2 refuted. Floor green. PR #29. |
| 2026-11-24 | OS-10 planned: 5 steps, 3 tests (Step/Shaper as_str, byte bounds); no collision in zygo-results · PR #31 |
| 2026-11-24 | OS-10 done: Step/Shaper/SHAPE_MAX_BYTES/MIN_SAVING_BYTES in zygo-results::shape; 0 confirmed findings (1 refuted); note to OS-28: cast before subtracting u64 bytes · PR #31 |
| 2026-11-24 | ML-16 done (a8add6a 3b33343 957db39): live selftest ok (76); review 10 confirmed, fixed; floor green. PR #26. |
| 2026-11-24 | DR-5 round 2 (0db609f 4f86964 2b072a7): Windows os error 33 fixed with a sidecar writer lock at <git common dir>/zygo-decisions.lock; lock span tested both ends; failure-path tests. Review: 4 fixed, 1 refuted (shared-repo write access, same as zygo-work). PR #29. |
| 2026-11-24 | MS-1 planned (454e92e): 15 steps; run_whole uses the cut mode with no cut for the process group · PR #33 |
| 2026-11-24 | MS-1 done (e7d5023 821a65f 7017674 ab87c30): run_whole/run_stream public in zygo-work; review 4 fixed (stream deadline, leader exit, panic guard, 2-entry memory), 2 refuted; floor green · PR #33 |
| 2026-11-24 | DR-6 planned (897ee78): 10 steps; tty checks stdin and stdout; USAGE line unchanged, review added later with new/ratify/overturn · PR #34 |
| 2026-11-24 | DR-6 done (732af80 82d2e65 cb609a4): zygo decision review; tty needs stdin and stdout; USAGE lists review; review 4 fixed (usage, over-limit text re-asked, cycle re-queue, HashSet), 3 refuted; floor green · PR #34 |
| 2026-11-24 | DR-11 planned (4e482f0); contract: one persistent `--batch` process (D§9.4), `run_piped` replaces the stdin form; author's call · PR #35 |
| 2026-11-24 | DR-11 done (52a5006 b306476 5e3b797 196e814 0add171): VerdictGit: 1 `--batch`, run_piped; review 8 fixed (tree cache/rev, 1 proc/kind, log `--ignore-submodules=none --encoding=UTF-8`, range revs, closing NUL, test HOME+tests); F5 (2 s bound runs idle): spec gap; floor green · PR #35 |
| 2026-11-24 | WD-1 planned (7e304a9): 6 steps; contract change Read::Absent { full_ref: Option<String> } (read_at needs it, R§7.4); lead's call: SystemGit reuses repo_file::git_out (now pub(crate)), GitOut widened to ExitStatus, no copy · PR #36 |
| 2026-11-24 | WD-1 done (repo definition reader; review round 1 fixed 8) · PR #36 |
| 2026-11-24 | DR-12 planned (1cffa68): 18 steps · PR #37 |
| 2026-11-24 | DR-12 in progress: node branch `phase3/DR-12--DR-12` at 3374eea, worktree `.worktrees/phase3/DR-12-node` · PR #37 |
| 2026-11-24 | DR-12 done: check_keep D§9.4 steps 1-9; review round 1 fixed 6 (merge paths, session line, stop problems); floor 2560/0 · PR #37 |
| 2026-11-24 | SX-1 planned (cf52550): 7 steps · PR #39 |
| 2026-11-24 | SX-1 in progress: node branch `phase3/SX-1--SX-1` at 0f7034f, worktree `.worktrees/phase3/SX-1-node` · PR #39 |
| 2026-11-24 | SX-1 done: mechanics text on SubagentStart for non-zygo subagents; review round 1 (3 confirmed, 1 refuted); floor 2570 passed 0 failed · PR #39 |
| 2026-11-24 | OS-16 planned (6ba122f): 6 test-first tasks; mod common { pub mod shaping; } per test binary; scripts/fixtures/ created; ZYGO_FIXTURES_DIR redirect; scrub order run dir, HOME, user, host. PR #32. |
| 2026-11-24 | OS-16 done: shaping fixture harness and capture script; review round 1 (10 confirmed); floor 2582 passed 0 failed · PR #32 |
| 2026-11-24 | SX-3 planned (65accaf): 7 steps · PR #41 |
| 2026-11-24 | SX-3 in progress: node branch `phase3/SX-3--SX-3` at d55d51c, worktree `.worktrees/phase3/SX-3-node` · PR #41 |
