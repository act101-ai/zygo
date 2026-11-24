# FIX-32 — Whole-branch review fixes: OpenCode short clusters, inline --settings, ws run live launcher, attribution writer, README drain line

**Spec:** `docs/specs/2026-11-20-workspaces-design.md` W-C7, W-C9, AU30; `docs/specs/2026-11-18-zygo-design.md` §13.4 (attribution guard row and its known limits). Created 2026-11-23 from the phase 2 whole-branch review (controller decisions: blocking findings B1-B4 and one README line R1; minors deferred, see "Deferred findings").
**Files:** `crates/zygo-workspace/src/args.rs`, `crates/zygo-workspace/tests/args.rs`, `crates/zygo/src/cmd/member.rs`, `crates/zygo/tests/workspace_run.rs`, `crates/zygo-launch/src/launcher.rs`, `crates/zygo-work/src/guards.rs`, `crates/zygo-work/tests/guards.rs`, `docs/specs/2026-11-20-workspaces-design.md`, `docs/specs/2026-11-18-zygo-design.md`, `README.md`
**Consumes:** —
**Produces:** —
**Acceptance:** `cargo test -p zygo-workspace -p zygo -p zygo-work --locked` → all pass, including the new tests of B1-B4 below
**Capability:** implementation

## Plan

Work test first: for each item, write the tests, run them red, fix, run them green.

**B1. OpenCode short clusters use the yargs-parser rule** (`crates/zygo-workspace/src/args.rs`, cluster walk).
- [ ] Confirm the yargs-parser short-flag loop in `/home/kagemartin/.opencode/bin/opencode` (read with `python3 -I`); record offset and quote below.
- [ ] Tests in `crates/zygo-workspace/tests/args.rs`: `-mc` (no value for `-m`, `-c` is `--continue`, `has_session_arg` true, restore adds no `--session`); `-ms ses_1` (`-s` takes `ses_1`, `has_session_arg` true); the `-sses_1` case re-derived from the confirmed rule; a Claude Code case that keeps today's rule.
- [ ] Test in `crates/zygo/tests/workspace_run.rs`: a saved OpenCode launch `-mc` with a resumable session is restored with no `--session`.
- [ ] Run → FAIL.
- [ ] Make the attach rule of a cluster letter depend on the agent: Claude Code keeps Commander's rule; OpenCode uses the confirmed yargs rule.
- [ ] Run → PASS.

**B2. An inline `--settings` JSON value is never saved** (W-C7, AU30).
- [ ] Tests: `--settings '{"env":{"K":"v"}}'` is removed and the label names `--settings`; `--settings=/abs/s.json` is kept; `--settings ./s.json` is kept as typed.
- [ ] Run → FAIL.
- [ ] Add the smallest mechanism for a class that depends on the value; rewrite the AU30 `--settings` sentence; change-log row.
- [ ] Run → PASS.

**B3. `ws run` refuses to resume while that conversation's launcher is live** (W-C9; `crates/zygo/src/cmd/member.rs`, the `None` branch of the session callback).
- [ ] Test in `crates/zygo/tests/workspace_run.rs`: a record whose launcher is live, then `ws run` for that pane: the W-C9 line, no agent start, an unchanged record.
- [ ] Run → FAIL.
- [ ] In the `update_existing` closure, before `record_start`, apply the live-launcher check of the hand-start path (`live_session`, `Conflict::Live`); the callback fails and the agent is not spawned.
- [ ] Run → PASS.

**B4. The attribution guard reads what the same command writes** (`crates/zygo-work/src/guards.rs`, `find_attribution`).
- [ ] Tests in `crates/zygo-work/tests/guards.rs`: heredoc writer with a trailer then `git commit -F msg.txt`: denied, heredoc named; clean heredoc writer while the file on disk holds a trailer: allowed; a pre-existing file with a trailer and no writer: denied.
- [ ] Run → FAIL.
- [ ] (a) a file named by `-F`, `--body-file` or `--notes-file` that an earlier segment writes (`>`, `>>`, `tee`) is not read from disk; that segment's heredocs and command text are searched; (b) the heredoc bodies of every earlier segment are searched. Update the §13.4 guard text; change-log row.
- [ ] Run → PASS.

**R1. README.** One sentence in the `zygo down` part: a drain that times out puts the workspace back up and stops nothing (`crates/zygo-workspace/src/down.rs`, `time_out`).
- [ ] Add the sentence.

- [ ] Measure: `cargo test -p zygo-workspace -p zygo -p zygo-work --locked --no-fail-fast`; `cargo clippy -p zygo-workspace -p zygo -p zygo-work --all-targets --locked -- -D warnings`; `cargo test --workspace --locked --no-run`; `sh scripts/check-premises.sh`.
- [ ] Commit.
