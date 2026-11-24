# Fragment MS — mail sync between machines (P3-2, P3-3, P3-4)

Authority: `docs/specs/2026-11-24-mail-sync-design.md` (S§n; approved 2026-11-24). Base: `docs/specs/2026-11-22-workspace-mail-design.md` (M§n; P3-1, cut by the ML planner, not built). Node contracts: `docs/work/phase3/nodes/MS-*.md`. Cut at `origin/phase-3` `768bf48`.

## 1. Rows

| ID | Node | Depends | State | Capability | Refs |
|----|------|---------|-------|------------|------|
| MS-1 | Bounded git runner: whole and stream modes, public in zygo-work | — | TODO | judgment | — |
| MS-2 | zygo-mail-sync crate: hardened git calls and test seams | MS-1 | TODO | implementation | — |
| MS-3 | Mail sync config keys, forms and per-workspace resolution | — | TODO | implementation | — |
| MS-4 | Address grammar and the ULID file form | MS-2, MS-3 | TODO | implementation | — |
| MS-5 | Sync state files: paths, identity, state.json, imported log, marks | MS-2 | TODO | judgment | — |
| MS-6 | Registry and ack files, validation, member cache | MS-4, MS-5 | TODO | judgment | — |
| MS-7 | Secret pass on one message | MS-2 | TODO | implementation | — |
| MS-8 | Public check: URL map, anonymous probe, verdict lists, cache | MS-5 | TODO | judgment | — |
| MS-9 | Outbox: send-time rules, refusal and notice, counts, local retention | MS-6, MS-7, MS-8 | TODO | judgment | — |
| MS-10 | Side branch plumbing: index-only worktree, fetch, bootstrap, commit, push | MS-5 | TODO | judgment | — |
| MS-11 | Tip metadata, own hosts, collision and twin checks | MS-6, MS-10 | TODO | judgment | — |
| MS-12 | Import: candidates, checks, crash-safe delivery | MS-6, MS-10 | TODO | judgment | — |
| MS-13 | The sync: gate, mark, lock, passes, push retry, wake | MS-9, MS-11, MS-12 | TODO | judgment | — |
| MS-14 | Prune: orphan rewrite of acknowledged files, lease push | MS-13 | TODO | judgment | — |
| MS-15 | Operator commands: take-host, new-identity, rename, drop-host | MS-14 | TODO | judgment | — |
| MS-16 | Sync child start and rate limit; member hooks start it | MS-3, MS-5 | TODO | implementation | — |
| MS-17 | `zygo mail sync` verb, options, hidden background form | MS-15, MS-16 | TODO | implementation | — |
| MS-18 | CLI send/read/ls/watch with addresses; escaped text; doctor twin line | MS-17 | TODO | implementation | — |
| MS-19 | MCP mail tools: addresses, send-time pass, remote mail_list | MS-17 | TODO | implementation | — |
| MS-20 | Session text: "Other machines" clause of item 8 | MS-6, MS-16 | TODO | mechanical | — |
| MS-21 | `zygo ws forget`: outbox question and sync worktree removal | MS-17 | TODO | implementation | — |
| MS-22 | No mail content in logs, state, usage rows, commits or refs | MS-18, MS-19, MS-21 | TODO | implementation | — |
| MS-23 | Live premise SP1: hook-started child outlives the hook | MS-17 | TODO | lookup | — |
| MS-24 | Live premise SP2: anonymous probe on github.com and gitlab.com | MS-8 | TODO | lookup | — |
| MS-25 | Record premises SP1-SP6 and the pinned git version | MS-14, MS-23, MS-24 | TODO | mechanical | — |

First nodes (no in-track dependency): MS-1, MS-3. The assembler adds the DR track's first node to both.

## 2. Files per node

- MS-1: `crates/zygo-work/src/repo.rs`, `crates/zygo-work/tests/git_runner.rs`
- MS-2: `Cargo.toml`, `Cargo.lock`, `crates/zygo-mail-sync/Cargo.toml`, `crates/zygo-mail-sync/src/lib.rs`, `crates/zygo-mail-sync/src/git.rs`, `crates/zygo-mail-sync/src/env.rs`, `crates/zygo-mail-sync/tests/git_builder.rs`, and the empty module files `crates/zygo-mail-sync/src/address.rs`, `crates/zygo-mail-sync/src/state.rs`, `crates/zygo-mail-sync/src/registry.rs`, `crates/zygo-mail-sync/src/secret.rs`, `crates/zygo-mail-sync/src/public.rs`, `crates/zygo-mail-sync/src/outbox.rs`, `crates/zygo-mail-sync/src/branch.rs`, `crates/zygo-mail-sync/src/identity.rs`, `crates/zygo-mail-sync/src/import.rs`, `crates/zygo-mail-sync/src/sync.rs`, `crates/zygo-mail-sync/src/prune.rs`, `crates/zygo-mail-sync/src/ops.rs`, `crates/zygo-mail-sync/src/spawn.rs`
- MS-3: `crates/zygo-config/src/schema.rs`, `crates/zygo-config/src/check.rs`, `crates/zygo-config/src/layers.rs`, `crates/zygo-config/tests/mail_sync_config.rs`
- MS-4: `crates/zygo-mail-sync/src/address.rs`, `crates/zygo-mail-sync/tests/address.rs`
- MS-5: `crates/zygo-mail-sync/src/state.rs`, `crates/zygo-mail-sync/tests/state.rs`
- MS-6: `crates/zygo-mail-sync/src/registry.rs`, `crates/zygo-mail-sync/tests/registry.rs`
- MS-7: `crates/zygo-mail-sync/src/secret.rs`, `crates/zygo-mail-sync/tests/secret.rs`
- MS-8: `crates/zygo-mail-sync/src/public.rs`, `crates/zygo-mail-sync/tests/public.rs`
- MS-9: `crates/zygo-mail-sync/src/outbox.rs`, `crates/zygo-mail-sync/tests/outbox.rs`, the P3-1 sweep file (ML: M§5 retention)
- MS-10: `crates/zygo-mail-sync/src/branch.rs`, `crates/zygo-mail-sync/tests/branch.rs`, `crates/zygo-mail-sync/tests/common/mod.rs`
- MS-11: `crates/zygo-mail-sync/src/identity.rs`, `crates/zygo-mail-sync/tests/identity.rs`
- MS-12: `crates/zygo-mail-sync/src/import.rs`, `crates/zygo-mail-sync/tests/import.rs`
- MS-13: `crates/zygo-mail-sync/src/sync.rs`, `crates/zygo-mail-sync/tests/sync_pass.rs`, `crates/zygo-mail-sync/tests/common/mod.rs`
- MS-14: `crates/zygo-mail-sync/src/prune.rs`, `crates/zygo-mail-sync/src/sync.rs`, `crates/zygo-mail-sync/tests/prune.rs`
- MS-15: `crates/zygo-mail-sync/src/ops.rs`, `crates/zygo-mail-sync/src/sync.rs`, `crates/zygo-mail-sync/tests/operator.rs`
- MS-16: `crates/zygo-mail-sync/src/spawn.rs`, `crates/zygo-hooks/src/workspace.rs`, `crates/zygo-hooks/Cargo.toml`, `Cargo.lock`, `crates/zygo-hooks/tests/mail_sync_trigger.rs`
- MS-17: `crates/zygo/src/cmd/mail.rs`, `crates/zygo/src/cmd/mod.rs`, `crates/zygo/Cargo.toml`, `Cargo.lock`, `crates/zygo/tests/mail_sync_cli.rs`
- MS-18: `crates/zygo/src/cmd/mail.rs`, `crates/zygo/src/cmd/doctor.rs`, `crates/zygo/tests/mail_remote_cli.rs`
- MS-19: `crates/zygo-mcp/src/tools/mail.rs`, `crates/zygo-mcp/Cargo.toml`, `Cargo.lock`, `crates/zygo-mcp/tests/mail_remote.rs`
- MS-20: `crates/zygo-hooks/src/session_text.rs`, `crates/zygo-hooks/tests/mail_sync_text.rs`
- MS-21: `crates/zygo-workspace/src/admin.rs`, `crates/zygo-workspace/tests/admin.rs`, `crates/zygo/src/cmd/ws.rs`, `crates/zygo/tests/mail_sync_forget.rs`
- MS-22: `crates/zygo/tests/mail_sync_no_content.rs`
- MS-23: —
- MS-24: —
- MS-25: `docs/premises.md`, `scripts/check-premises.sh`

In-track shared files and their paths: `Cargo.lock` (MS-2 → MS-5 → MS-16 → MS-17 → MS-19); `src/sync.rs` (MS-2 → … → MS-13 → MS-14 → MS-15); `tests/common/mod.rs` (MS-10 → MS-12 → MS-13); `crates/zygo/src/cmd/mail.rs` (MS-17 → MS-18); the empty module files (MS-2 before each owner).

## 3. Cross-track needs

**P3-1 names consumed (assembler maps each to its ML node and adds the edge):**

| M§ | What MS uses | Consumer nodes |
|---|---|---|
| M§5 | The mail store crate or module (Cargo dependency of `zygo-mail-sync`); maildir `mail/<pane key>/{tmp,new,cur}/`; the message file v1 type and atomic writer; the local send (used for the refusal notice); the sweep (AU69) | MS-2, MS-5, MS-9, MS-12, MS-13, MS-18 |
| M§5, M§11 | `[mail] max_unread`, `[mail] keep` | MS-9, MS-12 |
| M§6 | Member name form `^[a-z][a-z0-9-]{0,31}$`, reserved `cli` and `all`; `name` in the member record; name → pane key map | MS-4, MS-6, MS-12, MS-13 |
| M§7.1 | The Stop-hook block line (checked by S§19 #23); the hook-side member lookup in `crates/zygo-hooks/src/workspace.rs` | MS-13, MS-16 |
| M§7.3 | The wake (idle rule AU26, live launcher AU74, `@zygo-pane`/`@zygo-ws` checks, fixed text, `wake_gap`, `wake.json`) | MS-13 |
| M§8 | MCP tools `mail_send`, `mail_read`, `mail_list` in `crates/zygo-mcp/src/tools/` (MS assumes `mail.rs`) and the `[features] mail` gate; CLI `zygo mail send|read|ls|watch` in `crates/zygo/src/cmd/` (MS assumes `mail.rs`) and `Cmd::Mail` in `crates/zygo/src/cmd/mod.rs`; workspace lookup by `--name` | MS-17, MS-18, MS-19 |
| M§8a | Item 8 of the session text and its builder in `crates/zygo-hooks/src/session_text.rs` | MS-20 |
| M§10 | The `errors.log` writer for mail | MS-13, MS-16 |
| M§11 | The `[mail]` struct in `crates/zygo-config/src/schema.rs`; `mail` in `USER_ONLY` if P3-1 adds it | MS-3 |

If the ML cut names other paths for the MCP tool file, the CLI file or the sweep, the assembler replaces the MS paths with those and keeps the edges.

**Files shared with other tracks (each needs a path in the assembled graph):** `Cargo.toml`, `Cargo.lock` (every track that adds a crate or a dependency); `crates/zygo-config/src/schema.rs`, `check.rs`, `layers.rs` (ML M§11; workspace definition P3-11); `crates/zygo-hooks/src/workspace.rs` (ML M§7.1-7.2); `crates/zygo-hooks/src/session_text.rs` (ML M§8a; session text P3-14, P3-15); `crates/zygo/src/cmd/mod.rs` (ML `zygo mail`; any track that adds a verb, for example DR P3-10); `crates/zygo/src/cmd/doctor.rs`; `crates/zygo-workspace/src/admin.rs` and `crates/zygo/src/cmd/ws.rs` (workspace definition P3-11/P3-12 if it touches `ws`); `crates/zygo-work/src/repo.rs` (DR P3-9 if its verdict git reads, D§9.4, use this runner); `docs/premises.md`, `scripts/check-premises.sh` (ML PM1-PM3 and every track with premises).

**Produced for other tracks:** `zygo_work::repo::run_whole`, `run_stream`, `Whole`, `Flow`, `Streamed`, `RunError` now `pub` (MS-1); a DR node that needs a bounded git call with the whole stderr can consume them.

**DR:** MS-1 and MS-3 also depend on the DR track's first node (assembler).

## 4. Global constraints

- **S-C1** M-C1 to M-C6 hold; C6 for each new hook branch: a sync error never changes a hook decision — S§4 S-C1.
- **S-C2** No subject, body, address, host name, member name or path that holds one in a log, `workspace.json`, a usage row, a commit message or a ref name; a log line names ULIDs, pattern ids, check names, blob ids and counts only — S§4 S-C2.
- **S-C3** zygo writes only `refs/heads/zygo/<x>` on the remote; a force push only to that branch, only with the S§9 lease; branch form `^zygo/[a-z0-9][a-z0-9-]{0,31}$` — S§4 S-C3, S§11.
- **S-C4** Every git call is `git_command` (`crates/zygo-plugins/src/sync.rs:32-52`) plus `-c gc.auto=0 -c maintenance.auto=false`, under the S§8.7 runner, with a deadline — S§4 S-C4.
- **S-C5** All remote content is untrusted and validated before use; zygo never writes a file at a path the remote chose; no checkout of the side branch — S§4 S-C5, S§6.3.
- **S-C6** Only user config turns sync on; `.zygo.toml` and the team layer cannot — S§4 S-C6, S§11.
- **S-C7** Trust boundary as stated; the opt-in is the operator's consent — S§4 S-C7.
- **S-C8** No machine deletes or decides about another host's files by its own clock or config; only that host's ack or `--drop-host` — S§4 S-C8.
- **S-C9** No wall-clock timing in tests; no real `claude` or `opencode` (stub agents, `crates/zygo-launch/tests/stub_agent/`); clock, child spawn, public probe and named-step hooks are seams — S§4 S-C9; project rule (memory "No timing tests in CI"; commits `cb7d13c`, `96c1cbc`).
- Exact values: host `^[a-z0-9][a-z0-9-]{0,31}$` (S§5); identity `^[0-9a-f]{32}$` (S§5); ULID file `^[0-9A-HJKMNP-TV-Z]{26}\.json$` (S§8.5); marker `ZYGO-MAIL` = `1\n` (S§6.1); commit message `zygo mail [skip ci]`, author `zygo <zygo@zygo.invalid>`, `commit.gpgSign=false` (S§6.5); registry ≤ 64 KiB, ≤ 256 members (S§7.3); ack ≤ 8 MiB, ≤ 200 000 entries, own ack prune trigger > 100 000 (S§7.4); message ≤ 8 MiB, subject ≤ 200 bytes, body ≤ 1 MiB (S§8.5); 500 candidates per pass (S§8.5); 1000 outbox files per pass (S§8.3 step 11); pass deadline 120 s; 5 push attempts; CLI lock wait 30 s (S§8.3); probe deadline 10 s, public cache 1 h (S§8.2); `sync_gap` default `60s` (S§8.4, S§11); prune at `rev-list --count` ≥ 1000 (S§9); `outbox/tmp/` swept after 1 h (S§9); runner stderr ≤ 64 KiB (S§8.7); imported log 27 bytes per ULID, never trimmed (S§8.5); files `0600`, folders `0700` (M-C4).
- Native Windows is a non-goal (S§3, OP5): every crate builds on windows-latest; tests that use the workspace state, tmux, `setsid` or file modes are `#![cfg(unix)]`.
- Never run `git worktree prune` (S§6.3, S§12); never `git merge` or `rebase` (S§12).
- No Claude attribution in commits, pull requests or files (operator instruction; memory "No Claude attribution").

## 5. Verification floor additions

- All surfaces (ubuntu-latest, macos-latest, windows-latest): `cargo test --workspace --locked --no-fail-fast` now runs `zygo-mail-sync` and the new test files of `zygo-work`, `zygo-config`, `zygo-hooks`, `zygo-mcp`, `zygo-workspace` and `zygo`; they need `git` on `PATH` (present on all three runner images) and no network (`file://` remotes; injected probe) — `.github/workflows/ci.yml` job `test`; S§19 preamble.
- Premises: `sh scripts/check-premises.sh` covers SP1-SP6 and the `git:` header (MS-25) — S§20, S§17 row `docs/premises.md`.
- Live checks (manual, recorded by the operator): SP1 (MS-23), SP2 (MS-24) — S§20.

## 6. Review focus

1. A starter's mark written after the holder's look but before its release, and a starter that fails the lock in the gap between release and look — no lost pass — MS-13 (S§8.3 steps 2-4, 17; S§19 #16).
2. The prune base: never a fetch from before this pass's own push; the lease is that base; branch form and marker re-checked right before the force push — MS-14 (S§9).
3. Delivery crash windows: `finish_incoming` when the ULID is already in the log and the rename target already exists; a torn last line of the imported log — MS-12, MS-5 (S§8.5).
4. Own ack upkeep: a ULID leaves the ack only when no `*@<own host>/` file for it remains at the tip, across all own hosts after a rename — MS-13, MS-15 (S§7.4, S§7.6 "Rename").
5. `act_lite_store::read` returns an untyped error for a missing store (`crates/zygo-redact/src/act_lite_store.rs:72-73`): "no store" must give regex-only patterns, an unreadable or unparsable store must give `CouldNotRun`, with zygo-redact unchanged — MS-7 (S§8.6, S§17).
6. `run_bounded_out` sets a process group only in cut mode today (`crates/zygo-work/src/repo.rs:80-83`): both new modes need it, and the existing `run_bounded`/`run_bounded_err` callers must keep their behavior — MS-1 (S§8.7).
7. Hostile entry names in `ls-tree -z` output (tabs, newlines, `..`, very long names, names outside an address folder) never reach a filesystem path — MS-12, MS-10 (S-C5).
8. Twin check after a crash between recording "about to push" and the push: the pending id must not raise `Twin` — MS-11 (S§7.5).
9. The hook path: `trigger` reads config and `last_start` only, never runs git, and adds no output; a failed spawn changes no decision — MS-16 (S§8.1, S-C1).
10. Windows build: `pre_exec`/`setsid`, file modes and fs4 use are `cfg(unix)`; the crate still compiles and its tests compile to nothing — MS-2, MS-16 (OP5).
11. `git push --porcelain` exits non-zero on a rejected ref with the reason on stdout; another ref's line or a remote message must not be read as "moved" — MS-10 (S§8.3 step 15, S§8.7).
12. Collision line naming another local workspace goes to the CLI only, never to `errors.log` — MS-11, MS-13 (S§7.5, S-C2).
13. Subject and body limits are bytes, not characters; `sent_at` RFC 3339 parse of hostile values — MS-12 (S§8.5).
14. `zygo mail sync --background` must skip the results sweep and print nothing even on a panic — MS-17 (S§17 row `crates/zygo/src/cmd/mod.rs`, AU71, AU44).

## 7. Author's calls

1. **New crate `crates/zygo-mail-sync`** holds every S§17 "mail module" part. Reason: S§17 leaves the place open ("the place where the P3-1 build puts the M§5 store"), and a separate crate keeps MS files apart from ML files. Order: core → config → work, plugins, redact → workspace → P3-1 store → mail-sync → hooks, mcp, zygo. `zygo-workspace` cannot depend on it (cycle), so MS-21 gives `admin::forget` a `before_remove` callback that `crates/zygo/src/cmd/ws.rs` fills.
2. **Builder and runner (S§18):** `git_command` is reused from `zygo-plugins` unchanged (S§17 allows "reuse"); the runner is made public in `zygo-work` (MS-1) and gains the stream mode, not copied.
3. **Outbox layout:** flat `mail/outbox/<ULID>.json`, with `mail/outbox/tmp/` and `mail/outbox/refused/`; the branch path `<to>/<ULID>.json` comes from the validated `to` field. S§5 and S§8.3 do not fix the layout.
4. **Seams (S§18):** `Seam::at(&'static str)` on the pattern of `zygo_workspace::up::fire` (`crates/zygo-workspace/src/up.rs:168`); `Env` holds boxed `Clock`, `Probe`, `Seam`, `Spawner`; production builds it in the CLI (`sync_env`), MCP and hooks.
5. **The hook trigger starts no child when `sync_for` is not `Ok`** (only saves spawns; the child's own gate, S§8.3 step 1, still runs).
6. **`zygo doctor` twin line** reads `state.json` `last_error` (the twin check name); doctor runs no git.
7. **Wake** (S§10.1) is part of MS-13, because step 10 lives in the pass.
8. **Live premises:** MS-23 and MS-24 are `lookup` nodes that edit no file; MS-25 (`mechanical`) writes their results and the SP3-SP6 rows to `docs/premises.md`. This keeps "never `lookup` for a node that edits files".
9. **git pin (S§18, SP6):** the repository pins no git version (no `git:` header in `docs/premises.md`, none in `.github/workflows/ci.yml`). MS-25 adds a `git:` header with the version the CI tests and the operator's runs used; the minimum features in use are `fetch --no-write-fetch-head` and `maintenance.auto` (git 2.29) and the existing `rev-parse --path-format` (git 2.31).
10. **S§19 #27** is split: `mail_list` part in MS-19, session text part in MS-20; no single end-to-end test.
11. **`sync_for`** lives in `zygo-config` beside the keys, so hooks, MCP and CLI resolve sync the same way.
12. **S§16 base-spec changes** are not edited into the M or W specs by this track; S§16 states them. The assembler or operator decides if the base specs get a change note.

## 8. What a builder cannot plan from yet

- The P3-1 public API (crate or module path, the message type, local send, wake, maildir helpers, the MCP and CLI file names). Every MS node that names "(ML: M§n)" waits for the ML contracts; `docs/work/phase3/nodes/MS-2.md`, `MS-9.md`, `MS-12.md`, `MS-13.md`, `MS-16.md` to `MS-20.md`.
- The git version pin (S§18 "the git version that the build pins"; S§20 SP6): none exists in the repository; MS-25 sets it after the tests run.
- The CLI lock wait of 30 s (S§8.3 step 3) and the `watch` loop each `sync_gap` (S§8.1) have no CI test by the no-timing rule; MS-18 tests only the pure `watch_sync_due`; the 30 s wait is a review item of MS-13.
