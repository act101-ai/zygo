# Phase 3 Work Graph

<!-- work-graph -->

**Goal:** Build Phase 3 of zygo (workspace mail, mail sync, output shaping and saving accounting, decision records, workspace definition, session run time and session text) as the seven approved specs specify.
**Authority:** `docs/specs/2026-11-22-workspace-mail-design.md` (M§n; P3-1), `docs/specs/2026-11-24-decision-records-design.md` (D§n; P3-8 to P3-10), `docs/specs/2026-11-24-mail-sync-design.md` (S§n; P3-2 to P3-4), `docs/specs/2026-11-24-output-shaping-design.md` (O§n; P3-5 to P3-7), `docs/specs/2026-11-24-workspace-definition-design.md` (R§n; P3-11, P3-12), `docs/specs/2026-11-24-session-runtime-design.md` (T§n in RT nodes; P3-13), `docs/specs/2026-11-24-session-text-design.md` (T§n in SX nodes; P3-14, P3-15), each approved by the operator (P3-1 on 2026-11-22, the others on 2026-11-24); base specs `docs/specs/2026-11-18-zygo-design.md` (§n), `docs/specs/2026-11-20-workspaces-design.md` (W§n), `docs/specs/2026-11-21-statusline-hud-design.md` (H§n); order of work `docs/roadmap/phase-3.md`
**Run:** parallel=3 · worktrees=.worktrees/phase3/ · disk-per-node=—
**Workflows:** (1) Each node gets its own review gate of up to 3 rounds; every finding ends as fixed (a commit in the node range), rejected (with a reason) or recorded (a DDR id), per `docs/specs/2026-11-24-decision-records-design.md` — operator, 2026-11-24 (`docs/roadmap/phase-3.md` "Plan of work" item 3). (2) Rows are delivered as GitHub stacked pull requests on the trunk `phase-3`: a row's branch is created on top of the open stack's top branch, so every dependency sits in the same layer or a lower one, and its pull request is one layer of that stack (`gh stack link --base phase-3 …`); the controller merges a stack, or its bottom part, into `phase-3` (`gh stack merge`) once every layer in it has passed its review gate and its CI is green; a node is DONE when its layer merges; `phase-3` reaches `main` only when the operator says so — operator, 2026-11-24 ("we PR after every row, including planning"; "we need a phase-3 rendezvous branch, each row merges to that branch so you can be autonomous") and 2026-11-25 ("in the future i prefer you use stacked prs instead of this slow process"; "graph header is fine"; GitHub "About stacked pull requests"). (3) DDR first: every node with no other dependency depends on DR-1 (the record format), so DR-1 is the only root — operator, "P3-8 comes first in the order of work" (`docs/roadmap/phase-3.md` "Intent"). (4) The act-lite/act101 skill changes that the DDR spec names are made in their own repositories (`~/Work/act-lite`, `~/Work/act`, `~/Work/act101-marketplace`), not in this graph; no zygo release that contains DR-13's `dispositions` guard at `enforce` ships before those skill changes ship — `docs/specs/2026-11-24-decision-records-design.md` D§12.

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
- Bounds: each git process 2 s; one verdict at most 4 git processes and 10 s; `--batch` at most 2,048 objects and 16 MiB; node range at most 10,000 commits; at most 1,000 record changes read; record file at most 64 KiB, front matter closed within 8 KiB; directory at most 10,000 entries; `work_verdict` arguments at most 256 KiB, `dispositions` at most 256 entries, `finding` 1–200 characters, `reason` 1–2,000 characters; `title` 1–120 bytes; `unit` at most 128 bytes; `depends_on` at most 64 ids; `review_note` at most 2,000 bytes; `promotion` at most 500 bytes; `<problems>` at most 10 then `; and <n> more`, at most 4 KiB; `show` at most 50 commits, `review` at most 20 — D-C6, D§9.3, D§10.1, D§11.2
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
| OS-43 | Cap every shaped view at 8 KiB, kind view or not | OS-41, SX-4, OS-46 | TODO | judgment | — |
| MS-15 | Operator commands: take-host, new-identity, rename, drop-host | MS-14 | TODO | judgment | — |
| MS-17 | `zygo mail sync` verb, options, hidden background form | MS-15, MS-16, ML-12, ML-13 | TODO | implementation | — |
| MS-19 | MCP mail tools: addresses, send-time pass, remote mail_list | MS-17, ML-11, WD-18 | TODO | implementation | — |
| MS-23 | Live premise SP1: hook-started child outlives the hook | MS-17 | TODO | lookup | — |
| OS-36 | The never-worse check over every case and the built inputs | OS-35 | DONE | judgment | 1fb09e17 · a4501591 · 65a63246 · acceptance pass (never_worse 22, shaping 36) · PR #201 |
| OS-37 | On-demand latency test, ignored in CI | OS-35 | DONE | implementation | 98d82be5 · b5c27cbd · 77c6726f · acceptance pass (shaping_latency 5 ignored) · PR #200 |
| WD-14 | Pinned launches: resume and `ws run` resolve the profile from the pin | WD-12, WD-13, MS-17, FX-7 | TODO | judgment | — |
| MS-18 | CLI send/read/ls/watch with addresses; escaped text; doctor twin line | MS-17, ML-13, MS-19, WD-17, FX-15 | TODO | implementation | — |
| MS-21 | `zygo ws forget`: outbox question and sync worktree removal | MS-17, WD-12, WD-14 | TODO | implementation | — |
| MS-25 | Record premises SP1-SP6 and the pinned git version | MS-14, MS-23, MS-24, OS-26, FX-19 | TODO | mechanical | — |
| OS-45 | Bound the usage-store wait in elapsed time | — | DONE | judgment | fb717aba · aecf74d2 · a0378951 · acceptance pass (busy_wait 7, zygo-usage, zygo-hooks, readbacks) · PR #203 |
| OS-38 | Measure the latency budget on the operator's machine (operator, release build) | OS-37, OS-45 | TODO | lookup | — |
| WD-19 | Hooks, MCP server and status line read nothing from `.zygo/` | WD-14 | TODO | implementation | — |
| WD-21 | PR1 live check (operator): two pool members send two logins | WD-14, ML-19 | TODO | mechanical | — |
| WD-22 | PR2 live check (operator): per-session settings act the same on a pool directory | WD-14, WD-21 | TODO | mechanical | — |
| MS-22 | No mail content in logs, state, usage rows, commits or refs | MS-18, MS-19, MS-21 | TODO | implementation | — |
| WD-15 | `zygo workspace approve [--yes] [--digest <hex>]` | WD-14, DR-3, MS-21 | TODO | judgment | — |
| WD-16 | `config show/check`, `profile list/show`, `loadout list/show` show the repository files | WD-15, WD-24 | TODO | implementation | — |
| WD-20 | User docs: workspace files, approval, members, pool, passthrough | WD-12, WD-15, WD-16, WD-17 | TODO | mechanical | — |
| SX-7 | Base-spec amendments §8.5, §8.6 item 7, §8.6 last paragraph, §8.7 N33, §10.6 (T§13) | SX-4, SX-5, SX-6, WD-16 | TODO | implementation | — |
| OS-39 | Base spec text for output shaping and saving accounting | OS-8, OS-26, OS-34, SX-7 | TODO | mechanical | — |
| OS-46 | Bound the hook's usage-store wait per Bash call | — | DONE | judgment | 75339a21 · d6be5653 · c727ff0e · 68f8e48c · acceptance pass (busy_wait_total/open_bounded_total 7, rollback_journal 1, zygo-usage, zygo-hooks, readbacks) · PR #205 |
| SX-10 | Record ST1-ST4: fixtures, premise rows, `check-premises.sh` ST arm, code change per result (T§12.1 items 1-5) | SX-7, SX-9, MS-25, OS-39 | TODO | judgment | — |
| FX-1 | Process-group kill tests: anonymous pipe in place of a FIFO (Darwin FIFO lost wakeup) | — | DONE | implementation | 38ca329 · git_runner 17 + decision_git 34 pass; floor green · PR #40 |
| FX-2 | Decision writer lock: unlock before the close (a forked child keeps a closed flock) | — | DONE | implementation | 8a6191d c4584c2 · decisions::edit 4 pass; floor green · PR #42 |
| FX-3 | Test stdin helpers tolerate a child that exits before it reads stdin (statusline BrokenPipe flake) | — | DONE | implementation | 1636973 · compact_cmd 5p + statusline_cmd 4p; floor green · PR #46 |
| FX-4 | Proxy rows-bound test waits for the written row before the complete-flag GET (Windows flake) | — | DONE | mechanical | bbe99ca · serve 51p; floor green · PR #50 |
| FX-5 | Piped send test asserts send-then-read fails (Linux foreign-reader flake) | — | DONE | mechanical | 460b7cf · decision_git 35p, piped_epipe 1p; floor green · PR #51 |
| FX-6 | Dead-port test helpers: a port no parallel test can take (CI run 45557591675) | — | DONE | implementation | 690ea805 · zygo lib 38/compact_cmd 5/session_verbs 11/serve_cli 4/workspace_member 2/serve_client 40 passed · PR #65 |
| FX-7 | Exec-failure relaunch test runs in a child process (environ swap breaks parallel PATH lookups) | — | DONE | implementation | 365436c6 · zygo lib ws/work/plugins 22p, down 8p; --include-ignored 54p; floor green · PR #97 |
| FX-8 | zygo usage reads summary and savings from one snapshot | OS-8 | DONE | implementation | 4636d2ee, 73cffb55 · report_snapshot 1/0, usage_doctor 45/0 ; floor green · PR #105 |
| FX-9 | OpenCode session text item 8 fixed at SessionStart, not rebuilt per request | ML-9, ML-10, MS-16, WD-18, SX-10 | TODO | judgment | — |
| FX-10 | HUD spec lists the member name and unread mail segments and the ✉ glyph | ML-15, RT-3 | DONE | mechanical | 8b19ed30, daf5faed · ✉ 4 ; floor green · PR #123 |
| FX-11 | zygo doctor icon line shows the ✉ mail glyph | ML-15, RT-3 | DONE | implementation | 1921d4ca, a7ac8924 · usage_doctor 45/0, statusline_render 37/0 ; floor green · PR #122 |
| FX-12 | HUD unread count uses the store's definition (regular <ULID>.json files; missing new/ is 0) | ML-15, ML-2, FX-11 | DONE | implementation | 233290fe, 9335d48e · statusline_mail 9/0 ; floor green · PR #124 |
| FX-13 | Member record and maildir lifecycle: no maildir without a record; every record removal removes it | ML-14, ML-2, ML-11, ML-12, WD-14 | TODO | judgment | — |
| FX-14 | `zygo up` team read: CLI test of `with_team`, team-layer warnings only, R§9.4 sentence, `verbs::team` server tests, `up --all` exit code | WD-11, WD-12, ML-12 | DONE | implementation | 64d9685c, 3dc62442, 34467d50 · up_definition, workspace_restore, serve_cli, cmd::up all pass ; floor green · PR #162 |
| FX-15 | `zygo doctor` pool lines skip the members `zygo up` skips | WD-12 | DONE | implementation | 0b4ee021, f046b2b7 · definition_doctor 17/0 ; floor green · PR #129 |
| FX-16 | Clean removes charset designations (`ESC ( B`) and the escape after a lone `ESC` (`ESC ESC [31m`) | OS-12 | TODO | implementation | — |
| FX-17 | `zygo up` refuses a `[limits] max_members` outside 1-32 from the team or user layer (`config check` rule, `check_jobs` path) | FX-14, WD-11 | DONE | implementation | plan 7a124777 · node da3062c7, e437fde9 · acceptance pass (workspace_restore 7 passed, cmd::up 16 passed) · PR #169 |
| FX-18 | live-mail.sh fixture head line doubles the word zygo | ML-19 | DONE | mechanical | plan 02a12d2d · node fad4e150 · acceptance pass (sh scripts/premises/live-selftest.sh: live selftest: ok (77)) · PR #187 |
| FX-19 | PO2 row states what its run observed | OS-34 | DONE | mechanical | plan b2c8ca97 · node b5fa4c27, efbcccae · acceptance pass (check-premises ok; po2_ 3 passed) · PR #188 |
| FX-20 | live-detach.sh PW15 record head line doubles the word zygo | — | DONE | mechanical | 96868305 · c98ea5fa · cc948a96 · acceptance pass (live selftest: ok (85)) · PR #194 |
| WD-23 | `config check`: `job` only on an agent pane (`claude-code`, `opencode`); R§9.1 text and change log | WD-5, MS-3 | DONE | implementation | 20cc6480 · definition_keys 14/14, config_layers 90/0 (16 ignored); floor green · PR #75 |
| WD-24 | `loadout show` and `config show` pass pane `job` through `hide_secrets` (W§12) | WD-5 | DONE | mechanical | 9b719fa8 · cmd::loadout+config 32p; floor green · PR #66 |
| DR-16 | Unreadable `docs/decisions`: own error and line (`cannot read docs/decisions`), not "not a directory"; D§6.8 | DR-2, DR-3, DR-7 | DONE | implementation | c50a3dc9 · acc 5/0; floor green · PR #84 |
| MS-26 | `state::load` refuses a state.json value that `save` refuses (S§8.3) | MS-5 | DONE | implementation | 2c17b835 · acc 20/0; floor green · PR #83 |

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
| 2026-11-24 | FX-1 added (b5acf60): found in the RT-1 floor, a 24-min hang of git_runner stream_stop_kills_the_process_group; root cause Darwin FIFO lost wakeup; git_runner and decision_git kill tests use an anonymous pipe; tests only; 8 steps |
| 2026-11-24 | FX-1 in progress: node branch `phase3/FX-1--FX-1`, worktree `.worktrees/phase3/FX-1-node` · PR #40 |
| 2026-11-24 | FX-1 done: pipe replaces FIFO in the two process-group kill tests; review round 1 (0 confirmed, 4 refuted); floor 2570 passed 0 failed · PR #40 |
| 2026-11-24 | SX-8 DONE: P3-15 section copied into docs/live-checks.md from T§12.2; round 1 fixed 4 findings (Results row, mktemp, step 7 ids, backup delete), T§12.2 rewritten (author's call) · PR #44 |
| 2026-11-24 | FX-3 added: CI BrokenPipe flake on PRs #41 and #42 (statusline_without_a_session_dir_prints_nothing); the hook exits before it reads stdin (H§6.8, AU24, PH11), so the test write gets EPIPE; fix in test helpers (common::feed_stdin); tests only; 9 steps |
| 2026-11-24 | FX-3 DONE: `feed_stdin` tolerates BrokenPipe in three zygo test helpers; the new test fails first, with no clock; review 0 confirmed (hang-on-regression trade-off kept under focus 75); floor green (ws 2593/0) · PR #46 |
| 2026-11-24 | FX-2 added (20347a0): found in RT-1's CI (ubuntu run 45530476489), edit.rs:297 lock still held after apply; a child forked by another thread keeps the flock past the close; Held guard unlocks on drop; fork test; no production impact; 5 steps |
| 2026-11-24 | FX-2 in progress: node branch `phase3/FX-2--FX-2`, worktree `.worktrees/phase3/FX-2-node` · PR #42 |
| 2026-11-24 | FX-2 done: decision lock unlocked before close so a child's inherited descriptor cannot keep it; review round 1 (2 confirmed, 3 refuted); floor 2593 passed 0 failed · PR #42 |
| 2026-11-24 | OS-14 DONE: `size_view` and `shaped_view` in zygo-results view.rs; review round 1 (0 confirmed; F1 empty steps and F2 empty text refuted, spec O§7.3 line 227 and O§7.8); floor green (ws 2596/0) · PR #45 |
| 2026-11-24 | SX-2 done: `subagent_text()` in crates/zygo-hooks/src/session_start.rs prints mechanics only for an opencode `--event subagent`; review round 1 (2 confirmed, fixed: CLI test, doc sentence); floor green · PR #47 |
| 2026-11-24 | SX-3 planned (65accaf): 7 steps · PR #41 |
| 2026-11-24 | SX-3 in progress: node branch `phase3/SX-3--SX-3` at d55d51c, worktree `.worktrees/phase3/SX-3-node` · PR #41 |
| 2026-11-24 | SX-3 done: pre-compact sets reinject/<agent_id> (0600 in 0700 dir) for a non-zygo subagent; review round 1 (4 confirmed, 4 refuted); floor 2602 passed 0 failed · PR #41 |
| 2026-11-24 | FX-4 DONE: rows-bound test waits for `usage_written(1, …)` before the complete-flag GET (Windows race, CI run 45543172599; product correct per the N37 note); review 0 confirmed; floor green · PR #50 |
| 2026-11-24 | FX-5 done: piped send test asserts send-then-read fails (Linux foreign-reader race, CI run 45544809983); dash-safe foreign-reader test; single-test `piped_epipe` binary keeps the EPIPE check; review round 1 (3 confirmed, fixed); floor green · PR #51 |
| 2026-11-24 | RT-1 planned (6d70796): 11 steps, 10 first_started_at tests · PR #38 |
| 2026-11-24 | RT-1 in progress: node worktree .worktrees/phase3/RT-1-node, branch phase3/RT-1--RT-1 · PR #38 |
| 2026-11-24 | RT-1 done (7efa8d9 441e170 ff23c48): member record first_started_at; review round 1: 1 confirmed, 3 refuted; floor 2529 passed, 0 failed · PR #38 |
| 2026-11-24 | MS-2 done: zygo-mail-sync crate with hardened git calls and test seams, 13 empty module files; review round 1 (5 confirmed, fixed; S-C4 rewritten for fetch.writeCommitGraph=false); floor green · PR #48 |
| 2026-11-24 | OS-2 DONE: `readbacks` table; three `trims` columns added on the full open; seven-column insert fallback; round 1 fixed 2 (`table_xinfo`, `open` doc), O§6.5 item 3 rewritten (author's call); floor green · PR #49 |
| 2026-11-24 | OS-17 DONE: five clean/dedup fixtures captured; spec O§8.1 clean-row commands corrected (author's call); review round 1: larger CRLF diff case, O§7.4 rewrite reverted to operator, must_keep without escape bytes · PR #52 |
| 2026-11-24 | OS-11 done: command parse (crates/zygo-results/src/shape/command.rs); review round 1 fixed 9 findings (A1-A5, C1, C2, C4; C3 refuted); floor: workspace 2657 passed, 0 failed · PR #53 |
| 2026-11-24 | SX-6 DONE: the OpenCode plugin transform calls `--event subagent` for non-main `zygo/` models (T§7.2 item 1); review round 1 fixed 4 (anthropic case, per-call stdin, malformed model, missing output.system); floor: workspace 2652 passed, 0 failed · PR #54 |
| 2026-11-24 | OS-19 done: 4 libtest cases (passing 120, failing-nocapture, signal-crash SIGABRT, compile-error); failing cases labeled opencode; O§8.2/O§8.3 item 4: a case with no runner summary is checked against the none row (author's call); PR #56 |
| 2026-11-24 | FX-6, WD-23, WD-24 added: FX-6 dead-port test helpers (CI run 45557591675); WD-23 config check refuses job on a non-agent pane (operator, R§9.1); WD-24 job through hide_secrets (W§12). Files-overlap edges: ML-7 on FX-6, WD-16 on WD-24, WD-23 on MS-3. |
| 2026-11-24 | OS-18 DONE: diff, json and log fixtures; operator ratified the O§8.1 diff must_keep rule; review round 1 fixed 6 findings; floor: workspace 2652 passed, 0 failed · PR #55 |
| 2026-11-24 | OS-20 done: 3 pytest cases (passing 12 files, failing-verbose -v 2 failed, collection-error with --continue-on-collection-errors); review round 1 fixed floor-reachability and must_keep; PR #57 |
| 2026-11-24 | WD-5 done: config keys job, pool, [workspace] pool, [limits] max_members (default 8); review round fixed .zygo.toml max_members invalidating the file (repo_file::from_text); floor green (ws 2681 passed); PR #58. |
| 2026-11-24 | OS-40 done; O§7.1/O§7.2 command forms widened (pipeline newline, backtick/`0<`/`$(<f)`, `eval --`, `builtin` wrapper, `-c` option scan, redirect-only file printer); review: 6 confirmed fixed, 1 kept as safe; floor green (ws 2695 passed) |
| 2026-11-24 | OS-22 done; scripts/premises/live-shaping.sh po1-po4 (mock upstream, scrub, stand-in MCP for PO4); publishes only the five OS-23..26 evidence files; review: 2 confirmed + 4 plausible fixed; not run live (operator only); floor green (ws 2670) |
| 2026-11-24 | WD-24 DONE — hide_secrets scrubs pane job; two older field tests now cover job and name; W§12 scrub list names pane job and name (author's call); review F1, F2 fixed, F3 (loadout name is a table key, outside WD-24) dropped; floor 2681p 0f. |
| 2026-11-24 | MS-5 DONE: mail-sync state files (paths, identity, state.json, imported log, marks); fix round 1 closed 9 review findings; floor green (ws 2724/0). PR #69. |
| 2026-11-24 | MS-3 done: mail sync config keys (host, sync_gap, mail.workspace.<n>.sync/remote), sync_for, form checks incl. W§5 workspace-name keys; review: 4 fixed, 2 dropped (unknown keys: crate norm; remote: pass-time per S§11, carried to MS-10). PR #68. |
| 2026-11-24 | RT-2 done: run time on the HUD and the status runtime: line (ulid_secs, T§2.2 start rule, run_time_text); review: 3 fixed (read keeps phase when members does not parse; rule-5 pending test; plan ignore count). PR #67. |
| 2026-11-25 | MS-8 DONE — public-repository check with 1 h cache (S§8.2); review round 1 fixed 8 findings (cache tests, esc of server text, `--` before remote, test env); floor green ws 2773/0. |
| 2026-11-24 | WD-9 DONE: pool listing, assignment and shared lines; shared ~ rule and pool::home used by --config-dir; fix round 1 closed 6 review findings; floor green (ws 2722/0). PR #70. |
| 2026-11-24 | OS-7 done: savings read (crates/zygo-usage/src/savings.rs) with 17 tests; fix round 1 closed 4 findings; floor green, ws 2723 passed, 0 failed · PR #71 |
| 2026-11-25 | WD-2 done — repo_def shapes, allow-list validation, digest; review round 1 fixed OS-independent cwd check and added pool/layout tests; 12 tests; floor green (2736 passed). |
| 2026-11-25 | WD-23 DONE — config check refuses job on a non-agent pane (R§9.1 rule 5, operator 2026-11-24); review found no defect; carry-forward: the launch-time job node must apply the same rule; floor green ws 2740/0. |
| 2026-11-25 | MS-4 DONE — address grammar and ULID file form; review round 1 reserved `zygo` in zygo-core RESERVED_MEMBER_NAMES (M§6, S§7.2; scope widened to zygo-core ids.rs and two tests); floor green ws 2766/0. |
| 2026-11-25 | MS-10 DONE — side-branch plumbing (fetch, bootstrap, commit, push); review round 1 fixed 7 (--no-tags, --force-remove, push.gpgSign off, stale worktree re-add without prune per S§6.3, local-only no-branch match, 0700 temp dir); floor green ws 2789/0. |
| 2026-11-25 | DR-7 DONE — `zygo work report` record lines (unit `decisions:` line, trailing `decision records:` line; CLI only, slash form reads none per D-C2); review round 1 fixed 3 (one `git rev-parse --show-toplevel` call, args parsed before lookup, wiring tests); floor green ws 2819/0. |
| 2026-11-24 | MS-7 done; zygo_mail_sync::secret::check/check_with (Clear, Finding, CouldNotRun); scans every key and string leaf; block+redact ids unioned; finding on any match count; review: 3 confirmed + 3 fixed; floor green (ws 2691). |
| 2026-11-24 | ML-3 DONE: member/pane `name` fields in zygo-workspace (plan, member record, `panes[]`); node head 863b6b05; floor green: workspace 2682 passed, 0 failed · PR #62 |
| 2026-11-24 | OS-3 DONE: bounded usage-store open `open_bounded` + `HOOK_DB_BOUND`; node head c403338f; review 3 findings fixed; floor green: workspace 2677 passed, 0 failed · PR #64 |
| 2026-11-24 | FX-6 DONE: dead-port helpers point at fixed port 1 via shared `tests/dead_port`; plan revised after macOS bound-not-listening connect timed out; review found the same race in update.rs, fixed; floor: workspace 2671 passed, 0 failed · PR #65 |
| 2026-11-25 | MS-26 done; state::load now refuses values check_state refuses (InvalidData, file unchanged); review: 2 test gaps closed (probe beside fresh cache; public without verdict); floor ws 2944/0. |
| 2026-11-25 | DR-16 DONE: unreadable docs/decisions has its own DirError::Unreadable and line "cannot read docs/decisions"; review: 2 test gaps closed (operator verbs with docs at 000; new writes nothing); floor ws 2949/0 · PR #84 |
| 2026-11-25 | DR-13 DONE: work_verdict head/base/dispositions + dispositions guard (monitor/notify/enforce); review: short id shadowed by a ref now refused, config warning shown, dispatch tests; bounds header bytes→characters (D§9.2); floor ws 2966/0 · PR #85 |
| 2026-11-25 | WD-3 DONE: workspace approvals in approvals.toml (repo_def::store; [[workspace]] id+digest; plugin approvals still parse); review: F2 fixed (failed rename drops temp file), F1 refuted (per-section read by design), F3-F5 to operator (approve.rs protocol); floor ws 2961/0 · PR #88 |
| 2026-11-25 | DR-14 DONE: end-to-end decision gate (in-process stop path + merged-node keep; binary flow over zygo mcp stdio; no stored text). Review: R1/R3/R4 fixed (no mcp stderr marker; Win \\?\ canon; one env strip list), R2/R6 accepted, R5 refuted; floor ws 2977/1 pre-existing flake · PR #90 |
| 2026-11-25 | ML-4 DONE: member names + mail::send by name/key/all (names::resolve; reserved cli/zygo). Review: F1 fixed (SendReport; delivers past a refused target), F3 fixed (limits once, before resolve), F5 fixed (reserved name test), F4 refuted, F2 consumers fixed; floor ws 2967/0 · PR #89 |
| 2026-11-25 | ML-5 DONE: wake sends literal keys then Enter. Review: 3 of 6 findings confirmed and fixed (F1, F4, F5: a final ";" sent as "\;", docs for the two-call leftover state, sentinel-ordered wake test); F2, F3, F6 refuted. Floor ws 2976 passed, 0 failed. |
| 2026-11-25 | OS-4 DONE: trim rows carry result_id and shaper; hook opens usage.db bounded. Review: F2 (Read row test), F3 (Store::open doc) fixed; F1 dropped (needs a timing test), F4 dropped (node file in scope). MS-16 now depends on OS-4 (Cargo.lock, zygo-hooks Cargo.toml). |
| 2026-11-25 | OS-5 DONE: result_view/search/tree write one read-back row per successful reply; store opened bounded, kept, reopened after a failure. Review: F2 (stderr line test), F3 (positive control), F4 (Store::open doc) fixed; F1 dropped (O§6.4 keeps the store). |
| 2026-11-25 | FX-7 added from the zygo lib flake triage (`ws p=2977 f=1`): a failed exec in the test process frees the swapped environ, so parallel git spawns lose PATH; fix runs it in a child process. Files-overlap edges: ML-7 and WD-14 on FX-7. |
| 2026-11-25 | FX-7 DONE: exec-failure relaunch test runs in a child process; flake 7/20 before, 0/20 after. Review F1 fixed: child tests in ws.rs and down.rs do nothing unless their parent ran them (--include-ignored 2/20 to 0/20); down.rs added to Files. |
| 2026-11-25 | WD-4 DONE: checked use of the workspace definition (repo_def::check; one read, digest, store lookup per use; Pin). Review: F1, F3 fixed with tests; F2 refuted (contract: NotApproved has no why); floor ws 3017/0 · PR #98 |
| 2026-11-25 | MS-6 DONE: registry and ack files, validation, member cache; review 5 minor: F1 fixed (typed parse, no Value tree), F3 part (limits pub), F5 tests; F4 refuted; F2 (members.json read uncapped) to operator; MS-13/MS-14 notes carried; floor ws 3035/0 · PR #99 |
| 2026-11-25 | OS-8 DONE: zygo usage prints savings lines (net figures, breakdown, ratio). Review: F1 fixed (amounts unknown when config does not load), F2 fixed (no sign on zero), F4 tests added, F3 to FX-8 (added: one read snapshot); floor ws 3016/0 · PR #101 |
| 2026-11-25 | ML-7 DONE: member name in the session view marker; hand start takes `--name`. Review: 5 confirmed findings, all fixed. The DR-4 help line fix (top-level help omits `decision new`) landed · PR #103 |
| 2026-11-25 | FX-8 DONE: zygo usage reads summary and savings in one read transaction via new `Store::report` (savings first). Review: F1 confirmed (test sees the defect only in savings-first order; documented on `Store::report`), F2 refuted (no seam to write mid-report without a timing test) |
| 2026-11-25 | ML-8 DONE (2fc1f821 64486e85 0bfa1c6d): zygo ws name; review 1 important (hand-start rename race, fixed in hand_join) + 2 nits fixed; lock-wait test ignored as timing; floor green · PR #107 |
| 2026-11-25 | WD-6 done: `repo_def::repository_loadout` builds the `repository` loadout from `[members]` with the default set; review added a loadout-check test and edge tests. |
| 2026-11-25 | ML-9 done (ed8193ef 4bed6315): session text item 8; review 1 important (no-record pane told it is reachable; item now omitted) + 2 test gaps, dup names fixed; FX-9 added (OpenCode per-request rebuild). Floor green. PR #106. |
| 2026-11-25 | ML-15 done (0128c7fa 63b94dd8): HUD member name + unread mail; review 2 important (mail scan starved git/status workers: fixed; width rule kept) + 2 test gaps fixed; FX-10 (HUD spec), FX-11 (doctor ✉) added. Floor green. PR #108. |
| 2026-11-25 | WD-8 done: `state::Launch` gains `job` and `definition`; review found `Pin` dropped unknown keys (AU78), fixed with `Pin.extra` and `Pin::new` · PR #110 |
| 2026-11-25 | MS-9 DONE (e7987782 f30700e9 bec4c680): outbox kept: send rules, secret pass, refuse with notice, counts, sweep; round-2 review fixes applied; outbox 29/0; floor green · PR #111 |
| 2026-11-25 | WD-7 DONE: `Merged::with_definition` adds the approved definition as the `workspace-files` source; a second call replaces the first. Review round 2: 5 findings fixed · PR #112 |
| 2026-11-25 | MS-11 DONE: identity registry/ack checks and moved-workspace scan; review: 7 findings fixed; F1 (no cap on registry/ack file count) to operator as a proposed rule; F9 (twin false positive after crash at tip) is a contract note for MS-13 · PR #113 |
| 2026-11-25 | MS-12 done: import checks (reader-equal json), crash-safe delivery via incoming/, torn staged files re-imported, per-target inbox room; S§8.5 step 5 and Candidates rewritten (author's call). |
| 2026-11-25 | WD-17 DONE: `zygo doctor` prints the workspace-files, pool, shared-pool and member-maximum lines; it finds the checkout the same way `up` does. 2 rounds, 9 findings: 5 fixed, 1 closed by a fix, 1 carried forward, 2 dropped · PR #119 |
| 2026-11-25 | WD-10 done: plan panes carry job, pool and the definition pin; Source::Repository; list labels repository as workspace-files only when the definition added it (CLI passes false until WD-16). WD-12 contract note: job/pool from user loadouts are unchecked. |
| 2026-11-25 | ML-14 done: unread counts in zygo workspace, zygo ls and up; forget-pane removes mail/<key>/ (file, folder or link); FX-12 (HUD count) and FX-13 (maildir lifecycle) added from review. Floor green · PR #116. |
| 2026-11-25 | OS-9 DONE (80d820d0 04a9d343): `[results] shape` (default true) in config, the session view and the launcher; trim passes it on; review round 2 fixed 2 test gaps; floor green · PR #117 |
| 2026-11-25 | RT-3 DONE (fab29ce1 72c3d03a da38c41a): run-time HUD segment in compact and verbose with its width order; collect reads workspace.json once via member_read; review round 2 fixed 2; FX-11 widened to add the 󰅐 clock glyph to the doctor table. Floor green · PR #118 |
| 2026-11-25 | FX-11 DONE: the doctor icon line holds 21 glyphs, adding mail (U+2709) and clock (U+F0150); mail follows reflection, clock sits between trimmed and truncation. FX-10 must add the H§6.5 Mail row after Reflection · PR #122 |
| 2026-11-25 | FX-10 DONE: the HUD spec names the member name and unread mail segments and the `✉` glyph (AU4, AU20, AU25; H§6.2, H§6.4, H§6.5, H§6.6, H§6.7, H§6.8) · PR #123 |
| 2026-11-25 | WD-11 DONE: `up` reads definitions; a team config read failure degrades to the user config `max_members` with a printed line; `up --all` with a repository default prints the solo line. Round-3 non-blocking findings to FX-14 · PR #120 |
| 2026-11-25 | WD-13 DONE: launcher reads a repo profile from the default branch only for a name in neither user nor team config; prints not-approved, invalid, not-read lines; passthrough: no profile from any source, one line. L6 (duplicate precedence sentence, base §11.1) left to WD-14 · PR #121 |
| 2026-11-25 | FX-12 DONE: HUD unread counts only regular <ULID>.json files in new/; a missing new/ is 0, same as the mail store; review round 2 added I/L/O alphabet fixtures and narrowed the module doc · PR #124 |
| 2026-11-25 | WD-12 DONE: `zygo up` checks and assigns the config-directory pool round-robin; members run in passthrough; user-loadout job/pool values pass config check. Review round 2 moved the pool line after the step-8 write and added tests. FX-15 added (doctor pool filter) · PR #125 |
| 2026-11-25 | FX-15 DONE: doctor pool lines use the `zygo up` member filter (server user info skipped via has_user_info) and its kept member names (agent kinds, valid_member_name, first holder); review F1 fixed in round 2; F2/F3 to operator report · PR #129 |
| 2026-11-25 | MS-24 DONE: SP2 verified on github.com and gitlab.com with well-known private repos; both private probes fail with `could not read Username` (in PRIVATE_TEXT); result for MS-25 · PR #130 |
| 2026-11-25 | OS-21 done: go test fixtures (passing-verbose, failing-parallel, panic) captured from a scratch module; round 2 kept the full panic trace in must_keep and a fail_fetch log line right after its CONT. |
| 2026-11-25 | OS-12 DONE: merged 6e1b9387 5ff2466e; clean removes CSI, 8-bit CSI, OSC (OSC 8 keeps text) and CR overwrites, line count kept. Review F4 (`ESC ( B` leaves `B`) and F5 (`ESC ESC [31m` leaves `[31m`) to FX-16; O§7.4 rewritten, pending ratification · PR #132 |
| 2026-11-25 | ML-17 DONE: PM1-PM3 verified live (Claude Code 2.1.332, OpenCode 1.18.16, macOS); fixtures and premises rows added; live-checks entry added; check-premises has no PM arm (needs operator approval); run by the lead under the operator's instruction · PR #134 |
| 2026-11-25 | DR-15 DONE: PD2 and PD3 verified live on Claude Code 2.1.332 (mock upstream, stub zygo MCP); review fixes: chunked delivery, raw-byte check, limits stated; run by the lead under the operator's instruction · PR #133 |
| 2026-11-25 | SX-9 DONE: live premises ST1-ST4 verified; review findings F1-F3 fixed in round 2; OpenCode ran on glm/Z.AI, not OpenRouter (deviation in nodes/SX-9.md) · PR #135 |
| 2026-11-25 | OS-27 DONE: 22 shaping-off goldens (19 hook_stdout, 3 stored); review F1–F4, F6 fixed; F5 rejected (.gitattributes outside node files, no golden holds `\r`) · PR #140 |
| 2026-11-25 | OS-13 DONE: dedup step folds runs of equal consecutive lines, lossless. Review round 2 added 5 edge-input tests; F1 (expand count limit) rejected, needs an operator-approved threshold; F3 rejected · PR #139 |
| 2026-11-25 | OS-15 DONE: shape::run pipeline (steps 1-2, reuse rule, 4 MiB bound, size_view); review R1, R3, R4 fixed in tests; R2 (sniff of colored diff/json) held for operator ratification of O§7.3 (lead) · PR #145 |
| 2026-11-25 | OS-23 DONE: PO1 failed live (Claude Code 2.1.332): error holds stdout before stderr; no hook output replaced the text; OS-33 takes the "If PO1 fails" branch of O§7.13. Failed-Bash fixture and 2 failure cases. Review: 5 findings, 4 fixed, 1 carried to report · PR #142 |
| 2026-11-25 | ML-6 DONE: wake of an idle member after a send (M§7.3); Files widened with state.rs (Store::live_session, plan drift D1); after_send_with for injected probe and clock (MS-13); keys woken in parallel; lock waits, no added gate. Review: 9 findings, 8 fixed, 1 in part · PR #143 |
| 2026-11-25 | ML-10 DONE: Stop block (once, not while draining/stopping), prompt and SessionStart line, OpenCode per-request line; all off when [features] mail is off; unreadable mail/ changes no decision. Spec M§7.1, M§7.2, M§9 rewritten (author's call). Review: 6 fixed · PR #144 |
| 2026-11-25 | ML-11 DONE: MCP tools mail_send, mail_read, mail_list; review H2 fixed (stopping block follows AU83), H1/M1/L2-L4 carried, M2/L1 refuted; PR #146. |
| 2026-11-25 | MS-16 DONE: sync child start and rate limit; member hooks start it. Review fixes: session-start trigger for Claude Code; a future last_start counts as due; S§8.4 rewritten (author's call) · PR #148 |
| 2026-11-25 | OS-29 DONE: kind views diff/json/log (step 4); review 7: F1 (view larger than input → None), F2 (plain-diff hunk counts), F4 (must_keep whole-line rule), F7 tests fixed; F3 (8 KiB incl. marker → OS-28), F5, F6 carried · PR #153 |
| 2026-11-25 | MS-13 DONE (PR #150): sync pass with outbox, lock, wake and secret check; review R1-R7 fixed; lock tests made deterministic (spawn guard: fork-held flock fds made the retake busy). |
| 2026-11-25 | OS-28 DONE (PR #154): trim hook runs the shaping pipeline on Bash output; review R1-R4 fixed (opencode test input, kind sniffed on stdout, non-string stderr kept, 4 MiB bound on raw text); new node OS-41 for marker room in kind views. |
| 2026-11-25 | ML-12 DONE: zygo mail send\|read\|ls, /zygo:mail, mail sweep at command start. Review fixes: the sweep skips linked folders, a partial read prints what it has, a 2 s slash bound, a list-only fallback body, help args · PR #149 |
| 2026-11-25 | OS-24 DONE: PO2 verified on Claude Code 2.1.332 (stderr arrives merged into stdout; the replaced stdout removed err-line); hook fixture + empty shape-off golden added; review 7: F1/F2/F4-F7 fixed (first reading "not shown" reversed), F3 refuted · PR #151 |
| 2026-11-25 | OS-30 DONE: libtest runner view and dispatch as pipeline step 3; review round 2 fixed F1-F4 (failure sections, binary boundary, dedup lines, ASCII/CR whitespace); F5 is FX-16; F6 fixed in the OS-36 contract. |
| 2026-11-25 | FX-14 DONE: `zygo up` prints only team-layer warnings; CLI test of `with_team`, R§9.4 sentence, `verbs::team` server tests, `up --all` exit code. 2 rounds; review F1, F2 fixed; F3 nit kept; F4 (max_members range) is new node FX-17 · PR #162 |
| 2026-11-25 | FX-17 added from FX-14 review F4: `zygo up` takes `[limits] max_members` from the team or user layer with no 1-32 range check; contract reuses the `config check` rule via the `check_jobs` path (check.rs:119-120, up.rs:797-808) |
| 2026-11-25 | MS-13 DONE (round 3): commit batching (dba2a089) and chunking by ARGV_BUDGET 16000 / BODY_BUDGET 64 MiB with the commit contract doc and tests T1/T2 (5dd04b76). Review: 6 findings (F1, F2/F3, F4, T1, T2) · PR #150 |
| 2026-11-25 | ML-13 DONE: zygo mail watch (exit 0 on any file in new/, 1 at timeout, default 25m, 500ms poll). Review R1-R7: wake on unparsable files, write error exits 1, help/cli_surface, duplicate-name and poll-bound tests · PR #159 |
| 2026-11-25 | OS-25 DONE: PO3 verified on OpenCode 1.18.16 (live-shaping.sh po3, mock upstream): output.output holds stdout then stderr. Review R1-R3: run-opencode.sh keeps fixtures it does not write; test name match exact; spec records PO3. F2 (no PO arm in check-premises) awaits operator OK. |
| 2026-11-25 | OS-26 DONE: PO4 verified on Claude Code 2.1.332 and OpenCode 1.18.16 (wrapper 0 bytes up to 5199-byte replies); review findings 5: 3 confirmed and fixed, 2 refuted; new node OS-44 for read-back bytes above the agent's cut limits · PR #170 |
| 2026-11-25 | OS-33 DONE (kept, round 1): PO1 failed, so the O§7.13 "If PO1 fails" branch was taken: no trim on failed Bash. Review 3, all fixed: F1 vacuous hook fixtures (a tool_response input added); F2 step 13 clippy note; F3 trailer text · PR #168 |
| 2026-11-25 | OS-41 DONE: shaped marker room 341 B reserved in kind views, Step::ALL holds all 8 steps, hook tests per kind. Review: 4 findings, 2 fixed, 1 refuted, 1 → OS-43 (shaped view with no kind view has no 8 KiB cap) · PR #167 |
| 2026-11-25 | OS-31 DONE: pytest runner view; review R2 fixed F1 (-s live output), F3 (last line), F4 (PASSES kept), F5 (change log); F2 is OS-42 · PR #166 |
| 2026-11-25 | OS-42 added: wrapper dispatch and unseen pytest live output (OS-31 review F2, R2 report); OS-35 now depends on it |
| 2026-11-25 | FX-17 DONE (kept, round 1). Review 4: F1 and F2 (test gaps: other check errors; team warnings before the range line) fixed; F3 and F4 refuted (spec R§9.4 :271 fallback; existing exit 11 load error) · PR #169 |
| 2026-11-25 | ML-18 DONE: end-to-end mail on tmux with stub agents. Review findings 6: 4 confirmed and fixed, 2 refuted · PR #171 |
| 2026-11-25 | MS-14 DONE, kept, round 1. Fixed F1 (non-lease failure: `prune skipped (<check>)`; S§9 Push rewritten, lead's call; D11), F2 (`pruned <n> files` / `prune skipped: the branch moved; run again`), F6 (regular files only), F7 (`--max-count=1000`). Refuted F3, F4 (D8 gap), F5 · PR #179 |
| 2026-11-25 | ML-19 DONE (live run by the lead, override "YOU BUILD YOU TEST"). R1: 9 findings; 6 confirmed (F1 Method bullets, F2 line 3 M§14, F4 plan deviations, F6 beta's turn stopped by cleanup, F8 bounds, F9 exit-1 list); F3 → FX-18; 2 refuted (F5 operator override, F7 new checks) · PR #178 |
| 2026-11-25 | MS-20 DONE: review round 1: 7 findings, 4 confirmed (F2 consumer contracts FX-9/WD-18, F4 duplicate remote addresses, F6 two tests, F7 S§10.4/S§17 text), 3 refuted (F1 mail_list remote is MS-19, F3 per-request rebuild is FX-9, F5 cache size cap would be a new rule) · PR #177 |
| 2026-11-26 | SX-5 DONE, kept, round 1. Fixed F2 (step 9 ran 0 tests: workspace_down all #[ignore] timing; recorded). Refuted F1 (SubagentStart without agent_id: spec T§6.3 item 2 covers agent_id input only; a guard is a new check), F3 (settings order only) · PR #181 |
| 2026-11-26 | OS-32 DONE, kept, round 1. Fixed F1 (panic/race/teardown after a passed test hidden; O-C9), F2 (unparsed mark), F3 (go test rules 1-3 rewritten, lead's call, O§15 decision record). Refuted F4 (fixed step set), F5, F6 (OS-36 seam) · PR #182 |
| 2026-11-26 | OS-34 DONE, kept, round 1. Fixed F1 (test follows the PO2 row to failed), F3 (no prose assertion). F2 → FX-19 (PO2 run had stderr merged into stdout). Refuted F4 (fixed acceptance; --exact used), F5 (no failure) · PR #184 |
| 2026-11-26 | FX-19 added: PO2 row states what its run observed (OS-34 review F2); OS-42 and MS-25 now depend on it (shared files: output-shaping spec, premises.md) |
| 2026-11-26 | FX-18 DONE: M12 head line names zygo once (`${ZV:-zygo ?}`), stub-agent selftest case; live selftest ok (77). FX-20 added: live-detach.sh PW15 record line doubles the word zygo (FX-18 review F3) · PR #187 |
| 2026-11-26 | SX-4 DONE: hook trim takes the subagent's re-inject flag first and adds additionalContext once (T§9.2 item 2); review F1 (run entry test), F3 (stat error) and a doc nit fixed; F2 refuted (any remove/print order has a kill window; at-most-once holds). |
| 2026-11-26 | FX-19 DONE: PO2 row and O§7.8/O§17 restated to what the 2.1.332 run observed (stderr merged into stdout; stderr "" removal of non-empty stderr not observed); review F1, F4 fixed; author's call (lead), awaiting operator ratification. |
| 2026-11-26 | FX-20 DONE: live-detach.sh PW15 record line and result text name zygo once (`launch ${ZV:-zygo ?}`); stub-agent selftest cases for a version and an empty version; live selftest ok (85). Review F1 refuted (clap prints "zygo <version>"), F2 fixed (empty-version case) · PR #194 |
| 2026-11-26 | WD-18 DONE: item 8 lists each member's job and ends with the member's job sentence (R§9.3); review F1 (mail sync M-C6 restatement) and F2 part 1 (non-JOBS value) fixed in spec text; F2 part 2 refuted (one orchestrator per loadout, check.rs:241) |
| 2026-11-26 | OS-42 DONE: no view hides another runner's failure section; a wrapper at any depth, a PYTEST_ADDOPTS word, -p no:capture or an -o addopts override turns the pytest view off; O§7.6 (lead) awaiting operator ratification · PR #196 |
| 2026-11-26 | OS-35 DONE: 20 fixture views through the hook, id masked; floors, row bytes, recovery and bless guard tested; PO1-failed cases checked as none, their failing floor on the runner view; review F1-F6, F8 fixed, F7, F9 refuted · PR #198 |
| 2026-11-26 | OS-36 DONE: O§8.3 never-worse check over every case and 11 built inputs; bless refuses a failing view; item 5 rejects hidden failure-section lines; review F1, F3, F4 fixed, F2 refuted (contract oracle) · PR #201 |
| 2026-11-26 | OS-37 DONE: 4 O§8.5 budget tests and the child process, ignored in CI; sized inputs keep each case's shaping path; shape-on and shape-off rows asserted; review F1-F3, F5 fixed, F4 wording, F5 timeout refuted · PR #200 |
| 2026-11-26 | OS-45 added: OS-38 run failed O§8.5 budget 4 (lock held p95 159.27 ms > 150 ms); SQLite busy_timeout counts requested sleep, macOS sleeps ~1.5x, breaking O-C10; OS-38 now depends on OS-45 |
| 2026-11-26 | OS-45 DONE: bounded open uses its own elapsed-time busy handler; lock-held hook p95 159.27 → 128.05 ms (release, macOS); review F1, F2, F4 fixed; F3 (O-C10 per call on a non-WAL db) open for a new row · PR #203 |
| 2026-11-26 | OS-46 added: OS-45 review F3; O-C10 bounds each Bash call but O§7.11 bounds each statement, so on a rollback-journal db the hook's read and write can each wait HOOK_DB_BOUND |
| 2026-11-26 | OS-46 DONE: hook uses open_bounded_total (one wait budget per Bash call); MCP keeps per-statement open; OS-43 now depends on OS-46 (both change trim.rs); review F1-F6, F8 fixed, F7 recorded here · PR #205 |
| 2026-11-26 | OS-44 DROPPED: kept_bytes modelled each agent's cut, but OpenCode keeps whole lines under user-config limits and Claude Code cuts by an API token count and keeps characters; no premise states what is kept; O§6.4 Size limit stated as permanent · PR #207 |
