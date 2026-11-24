# Fragment: output shaping and saving accounting (OS track, P3-5 to P3-7)

Authority: `docs/specs/2026-11-24-output-shaping-design.md` (`O§n`), approved by the operator 2026-11-24. Base spec: `docs/specs/2026-11-18-zygo-design.md` (`§n`). Node contracts: `docs/work/phase3/nodes/OS-1.md` to `OS-39.md`.

## 1. Rows

Depends lists only nodes of this track. The assembler adds the DR track's first node to the Depends of each row with `—` here (OS-1, OS-6, OS-10, OS-16).

| ID | Node | Depends | State | Capability | Refs |
|----|------|---------|-------|------------|------|
| OS-1 | Result meta records the session that stored it | — | TODO | implementation | — |
| OS-2 | Usage store: `readbacks` table and three additive `trims` columns | OS-1 | TODO | judgment | — |
| OS-3 | Bounded usage-store open for the hook and the MCP server | OS-2 | TODO | judgment | — |
| OS-4 | Trim hook writes `result_id` and `shaper`, opens the store bounded | OS-3 | TODO | implementation | — |
| OS-5 | MCP result tools record one read-back row per successful reply | OS-3 | TODO | implementation | — |
| OS-6 | Carry computation for one event over its context's requests | — | TODO | judgment | — |
| OS-7 | Savings read: net figures, per-context carry, breakdown and ratio | OS-2, OS-6 | TODO | judgment | — |
| OS-8 | `zygo usage` prints the net figures, breakdown and ratio | OS-7 | TODO | implementation | — |
| OS-9 | `[results] shape` in config and the session view | OS-4 | TODO | mechanical | — |
| OS-10 | `shape` module: step names, shaper names and byte bounds | — | TODO | mechanical | — |
| OS-11 | Command parse: segments, file printers, stored name, runners, reuse text | OS-10 | TODO | judgment | — |
| OS-12 | Clean step: ANSI escapes and carriage-return progress | OS-11 | TODO | implementation | — |
| OS-13 | Dedup step: runs of equal consecutive lines, lossless | OS-12 | TODO | implementation | — |
| OS-14 | Size view over any text, and the shaped marker | OS-1, OS-10 | TODO | implementation | — |
| OS-15 | Shaping pipeline: step order, reuse rule, byte bound | OS-13 | TODO | judgment | — |
| OS-16 | Shaping fixture harness: byte-exact files, capture script, format and secret checks | — | TODO | implementation | — |
| OS-17 | Capture clean and dedup fixtures from real command output | OS-16 | TODO | implementation | — |
| OS-18 | Capture diff, json and log fixtures from real command output | OS-16 | TODO | implementation | — |
| OS-19 | Capture libtest fixtures from a scratch crate | OS-16 | TODO | implementation | — |
| OS-20 | Capture pytest fixtures from a scratch project | OS-16 | TODO | implementation | — |
| OS-21 | Capture go test fixtures from a scratch module | OS-16 | TODO | implementation | — |
| OS-22 | Premise script for PO1 to PO4 | OS-16 | TODO | implementation | — |
| OS-23 | Premise PO1: where a failed Bash call's output arrives, and what replaces it (operator, live Claude Code) | OS-22 | TODO | lookup | — |
| OS-24 | Premise PO2: an empty `stderr` in `updatedToolOutput` removes the stderr (operator, live Claude Code) | OS-23 | TODO | lookup | — |
| OS-25 | Premise PO3: OpenCode `output.output` holds stdout and stderr (operator, live OpenCode) | OS-24 | TODO | lookup | — |
| OS-26 | Premise PO4: the model sees a `result_view` reply plus at most a fixed wrapper (operator, both agents) | OS-25 | TODO | lookup | — |
| OS-27 | Shape-off baseline: hook output and stored file of the `97d8767` behavior | OS-9, OS-17, OS-18, OS-19, OS-20, OS-21 | TODO | implementation | — |
| OS-28 | Trim hook runs the shaping pipeline on Bash output | OS-14, OS-15, OS-27 | TODO | judgment | — |
| OS-29 | Kind views for diff, json and log (pipeline step 4) | OS-15, OS-18 | TODO | judgment | — |
| OS-30 | Runner views: dispatch and the libtest view (pipeline step 3) | OS-29, OS-19 | TODO | judgment | — |
| OS-31 | pytest runner view | OS-30, OS-20 | TODO | judgment | — |
| OS-32 | go test runner view with interleaved parallel output | OS-31, OS-21 | TODO | judgment | — |
| OS-33 | Failed Bash calls on Claude Code, as PO1 decided | OS-28, OS-23 | TODO | judgment | — |
| OS-34 | Set the stderr branch as PO2 decided | OS-33, OS-24 | TODO | mechanical | — |
| OS-35 | Fixture views through the hook: goldens, floors, recovery, bless guard | OS-34, OS-32 | TODO | judgment | — |
| OS-36 | The never-worse check over every case and the built inputs | OS-35 | TODO | judgment | — |
| OS-37 | On-demand latency test, ignored in CI | OS-35 | TODO | implementation | — |
| OS-38 | Measure the latency budget on the operator's machine (operator, release build) | OS-37 | TODO | lookup | — |
| OS-39 | Base spec text for output shaping and saving accounting | OS-8, OS-26, OS-34 | TODO | mechanical | — |

## 2. Files per node

| Node | Files |
|---|---|
| OS-1 | `crates/zygo-results/src/store.rs`, `crates/zygo-results/tests/results.rs`, `crates/zygo-hooks/src/trim.rs` |
| OS-2 | `crates/zygo-usage/src/schema.rs`, `crates/zygo-usage/src/store.rs`, `crates/zygo-usage/src/query.rs`, `crates/zygo-usage/tests/store.rs`, `crates/zygo/tests/usage_doctor.rs`, `crates/zygo-hooks/src/trim.rs` |
| OS-3 | `crates/zygo-usage/src/store.rs`, `crates/zygo-usage/tests/store.rs` |
| OS-4 | `crates/zygo-hooks/src/trim.rs`, `crates/zygo-hooks/tests/hooks_basic.rs` |
| OS-5 | `crates/zygo-mcp/src/tools/results.rs`, `crates/zygo-mcp/tests/readbacks.rs` |
| OS-6 | `crates/zygo-usage/src/carry.rs`, `crates/zygo-usage/src/lib.rs`, `crates/zygo-usage/tests/carry.rs` |
| OS-7 | `crates/zygo-usage/src/savings.rs`, `crates/zygo-usage/src/lib.rs`, `crates/zygo-usage/tests/savings.rs` |
| OS-8 | `crates/zygo/src/cmd/usage.rs`, `crates/zygo/tests/usage_doctor.rs` |
| OS-9 | `crates/zygo-config/src/schema.rs`, `crates/zygo-config/tests/config_layers.rs`, `crates/zygo-core/src/session_view.rs`, `crates/zygo-core/tests/core_basics.rs`, `crates/zygo-launch/src/launcher.rs`, `crates/zygo-launch/tests/launcher.rs`, `crates/zygo-hooks/src/trim.rs` |
| OS-10 | `crates/zygo-results/src/shape/mod.rs`, `crates/zygo-results/src/lib.rs` |
| OS-11 | `crates/zygo-results/src/shape/command.rs`, `crates/zygo-results/src/shape/mod.rs`, `crates/zygo-results/tests/shape_command.rs` |
| OS-12 | `crates/zygo-results/src/shape/clean.rs`, `crates/zygo-results/src/shape/mod.rs`, `crates/zygo-results/tests/shape_clean.rs` |
| OS-13 | `crates/zygo-results/src/shape/dedup.rs`, `crates/zygo-results/src/shape/mod.rs`, `crates/zygo-results/tests/shape_dedup.rs` |
| OS-14 | `crates/zygo-results/src/view.rs`, `crates/zygo-results/tests/results.rs` |
| OS-15 | `crates/zygo-results/src/shape/mod.rs`, `crates/zygo-results/tests/shape_pipeline.rs` |
| OS-16 | `.gitattributes`, `scripts/fixtures/capture-shaping.sh`, `crates/zygo-hooks/tests/common/shaping.rs`, `crates/zygo-hooks/tests/shaping_fixtures.rs` |
| OS-17 | `fixtures/shaping/clean/{cargo-test-color,git-clone-progress,git-diff-color-crlf}/`, `fixtures/shaping/dedup/{build-repeated-warning,already-repeated}/` |
| OS-18 | `fixtures/shaping/diff/git-diff-repo/`, `fixtures/shaping/json/cargo-metadata/`, `fixtures/shaping/log/errors-in-middle/` |
| OS-19 | `fixtures/shaping/libtest/{passing,failing-nocapture,signal-crash,compile-error}/` |
| OS-20 | `fixtures/shaping/pytest/{passing,failing-verbose,collection-error}/` |
| OS-21 | `fixtures/shaping/gotest/{passing-verbose,failing-parallel,panic}/` |
| OS-22 | `scripts/premises/live-shaping.sh` |
| OS-23 | `fixtures/claude-code/hooks/PostToolUseFailure-Bash-output-main.json`, `fixtures/shaping/failure/{short,cargo-test-failing}/`, `docs/premises.md` |
| OS-24 | `docs/premises.md`, `fixtures/claude-code/requests/po2-result.json` |
| OS-25 | `docs/premises.md`, `fixtures/opencode/plugin-hooks/tool.execute.after-bash-stderr.json` |
| OS-26 | `docs/premises.md`, `fixtures/claude-code/requests/po4-readback.json`, `fixtures/opencode/requests/po4-readback.json` |
| OS-27 | `crates/zygo-hooks/tests/shaping_off.rs`, `crates/zygo-hooks/tests/golden/shape_off/` |
| OS-28 | `crates/zygo-hooks/src/trim.rs`, `crates/zygo-hooks/tests/shaping_hook.rs` |
| OS-29 | `crates/zygo-results/src/shape/kind_view.rs`, `crates/zygo-results/src/shape/mod.rs`, `crates/zygo-results/tests/shape_kind_view.rs`, `crates/zygo-results/tests/shape_pipeline.rs` |
| OS-30 | `crates/zygo-results/src/shape/runner.rs`, `crates/zygo-results/src/shape/mod.rs`, `crates/zygo-results/tests/shape_runner_libtest.rs`, `crates/zygo-results/tests/shape_pipeline.rs` |
| OS-31 | `crates/zygo-results/src/shape/runner.rs`, `crates/zygo-results/tests/shape_runner_pytest.rs` |
| OS-32 | `crates/zygo-results/src/shape/runner.rs`, `crates/zygo-results/tests/shape_runner_gotest.rs` |
| OS-33 | `crates/zygo-hooks/src/trim.rs`, `crates/zygo-hooks/tests/shaping_hook.rs`, `crates/zygo-launch/src/claude_code.rs`, `crates/zygo-launch/tests/claude_code.rs` |
| OS-34 | `crates/zygo-hooks/src/trim.rs`, `crates/zygo-hooks/tests/shaping_hook.rs` |
| OS-35 | `crates/zygo-hooks/tests/shaping.rs`, `fixtures/shaping/**/view` |
| OS-36 | `crates/zygo-hooks/tests/shaping.rs` |
| OS-37 | `crates/zygo-hooks/tests/shaping_latency.rs` |
| OS-38 | — |
| OS-39 | `docs/specs/2026-11-18-zygo-design.md` |

Files that this track changes several times, with the path that orders them: `crates/zygo-hooks/src/trim.rs` (OS-1 → OS-2 → OS-3 → OS-4 → OS-9 → OS-27 → OS-28 → OS-33 → OS-34); `crates/zygo-results/src/shape/mod.rs` (OS-10 → OS-11 → OS-12 → OS-13 → OS-15 → OS-29 → OS-30); `crates/zygo-results/src/shape/runner.rs` (OS-30 → OS-31 → OS-32); `docs/premises.md` (OS-23 → OS-24 → OS-25 → OS-26); `crates/zygo/tests/usage_doctor.rs` (OS-2 → OS-7 → OS-8).

## 3. Cross-track needs

- **DR track first node:** the assembler adds it to OS-1, OS-6, OS-10 and OS-16 (the rows with `—`). Each node of this track follows `docs/specs/2026-11-24-decision-records-design.md`: a regular-expression change toward hiding less (O§15 item 2), a fixture case choice (O§15 item 4) and each author's call of section 7 below that a builder changes is a DDR under `docs/decisions/` (D§6.1).
- **Session text track (SX):** shares `crates/zygo-hooks/src/trim.rs` (OS-1, OS-2, OS-4, OS-9, OS-28, OS-33, OS-34) and `crates/zygo-launch/src/claude_code.rs` (OS-33; the hook registration at `claude_code.rs:476-503`). OS-33 inserts the `trim` entry first under `PostToolUseFailure`; an SX change to that list must keep that order. The assembler must put a path between each SX node on these files and the OS nodes above.
- **Shared candidates with other tracks (assembler to check):** `crates/zygo-config/src/schema.rs`, `crates/zygo-core/src/session_view.rs`, `crates/zygo-launch/src/launcher.rs`, `crates/zygo-launch/tests/launcher.rs`, `crates/zygo-core/tests/core_basics.rs`, `crates/zygo-config/tests/config_layers.rs` (OS-9); `docs/premises.md` (OS-23 to OS-26); `docs/specs/2026-11-18-zygo-design.md` (OS-39); `.gitattributes` (OS-16); `crates/zygo-usage/src/store.rs`, `crates/zygo-usage/src/schema.rs` (OS-2, OS-3); `crates/zygo-mcp/src/tools/results.rs` (OS-5); `crates/zygo/src/cmd/usage.rs` (OS-8).
- **Operator time:** OS-23 to OS-26 and OS-38 run by hand on the operator's machine (live Claude Code 2.1.328 or later, OpenCode 1.18.16 or later, release build). OS-33, OS-34, OS-35, OS-39 wait for them.

## 4. Global constraints

- O-C1: a shaper fault, a store fault or a usage-store fault never makes the agent's output worse; a fault gives the §10.1 size view or no change — `docs/specs/2026-11-24-output-shaping-design.md` O§4 O-C1; base C6
- O-C2: the usage store holds no tool input and no tool output; the command is stored only as a name from the closed list of O§7.2, else `other` — O§4 O-C2, O§7.2; base §14
- O-C3: raw output lives only in the session's results directory, mode `0600` (directory `0700`, `crates/zygo-results/src/store.rs:20-41`) — O§4 O-C3; base §10.1
- O-C4: a view is used only when `raw_bytes − view_bytes ≥ MIN_SAVING_BYTES` = 256, with the row's numbers, marker included — O§4 O-C4, O§7.8
- O-C5: price multipliers come from the catalog price rows, never from constants in code — O§4 O-C5 (operator)
- O-C6: CI tests are deterministic; no CI test measures wall-clock time or has a deadline; the latency budget runs only on demand (`#[ignore = "timing: shaping latency budget, run on demand"]`) — O§4 O-C6, O§8.5; commit `cb7d13c`; operator memory "No timing tests in CI"
- O-C7: tests never run a real `claude` or `opencode`; hook tests feed hook-input JSON to `trim_in`; launcher tests use the stub agents — O§4 O-C7, O§8.4; operator memory "Stub agents in harness tests"
- O-C8: each new stored field is additive; `SCHEMA_VERSION` stays 1 (`crates/zygo-usage/src/store.rs:62`) — O§4 O-C8, O§6.5
- O-C9: a shaped view hides only lines that a shaper positively recognizes as noise; no view drops the text that says why something failed; unsure means keep the line or no change — O§4 O-C9 (lead)
- O-C10: the trim hook never makes a Bash call wait more than `HOOK_DB_BOUND` = 100 ms on a busy usage database — O§4 O-C10, O§7.11
- Constants, not config: `SHAPE_MAX_BYTES` = 4 MiB (4194304), `MIN_SAVING_BYTES` = 256, `HOOK_DB_BOUND` = 100 ms, `VIEW_CAP_BYTES` = 8 KiB (`crates/zygo-results/src/view.rs:5`), `BYTES_PER_TOKEN` = 4 (`crates/zygo-core/src/tokens.rs:4`), threshold default 16384 bytes — O§9, O§7.3, O§7.7, O§6.2
- `[results] shape = false` gives hook output and stored file byte-equal to those of `97d8767` — O§9
- Shaping is for the Bash tool only; `Read`, `Grep`, `Glob`, `WebFetch` keep the §10.1 size view — O§7.1, O§3
- "Trimmed" always means the gross figure; the net figure is always "kept out of context"; the status line and `zygo status` do not change — O§6.2, O§6.7
- Each fixture is real output, scrubbed; at most 256 KiB per case and 4 MiB for `fixtures/shaping/` — O§8.1
- Every phase 3 node runs on branch `phase-3`, with a review gate of up to 3 rounds and the P3-8 disposition rule — `docs/roadmap/phase-3.md` "Plan of work (operator, 2026-11-24)" items 3-4

## 5. Verification floor additions

- All CI surfaces (ubuntu-latest, macos-latest, windows-latest): `cargo test --workspace --locked --no-fail-fast` now also runs `zygo-usage` tests `carry`, `savings`; `zygo-results` tests `shape_*`; `zygo-mcp` test `readbacks`; `zygo-hooks` tests `shaping`, `shaping_hook`, `shaping_off`, `shaping_fixtures` — `.github/workflows/ci.yml:45`; O§16 items 1-16
- Fixture bless, by hand, reviewed in the diff: `ZYGO_BLESS=1 cargo test -p zygo-hooks --test shaping` (OS-35) and `ZYGO_BLESS=1 cargo test -p zygo-hooks --test shaping_off` (OS-27, before OS-28 only) — O§8.1 "Capture"
- On demand, not CI, operator's machine, release build: `cargo test -p zygo-hooks --release --test shaping_latency -- --ignored` (OS-38); the numbers go in the phase 3 pull request — O§8.5, O§16 item 17
- Live premises, by hand, recorded in `docs/premises.md`: PO1-PO4 through `scripts/premises/live-shaping.sh` (OS-23 to OS-26) — O§17

## 6. Review focus

- OS-11 (O§7.1): a file printer the parse misses (nested `-c`, `$( )` inside double quotes, `xargs -I{} cat`, `find -execdir`, `command cat`, `env FOO=1 cat`, a path such as `/bin/cat`) gets shaped text that the agent may copy into `Edit`.
- OS-30, OS-31, OS-32 (O§7.6, O-C9): a hidden form that also matches a failure line (`test x ... ok` text printed by a failing test, a pytest `-v` line of an `XPASS(strict)` failure, a go `--- SKIP` that wraps a `t.Fatal`); a go test line given to the wrong test after `=== NAME`.
- OS-28 (O§7.8, O§7.10): the use rule checked on a view without its marker; `catch_unwind` that leaves the size view off for a large output; stderr counted when PO2 failed; a `Store::put` error that still writes a row.
- OS-6, OS-7 (O§6.3, O§6.4): the cut compared with the wrong previous row; an expiry seen on a growing context; a read-back keyed by the reading session; OpenCode subagent rows carrying a main-loop trim; double count of an unpriced request.
- OS-2, OS-3 (O§6.5, O§7.11): the hook adding a column (write lock on every Bash call); the seven-column fallback hiding an error other than "no such column"; a bounded open that waits longer than `bound`.
- OS-5 (O§6.4, O-C1): a read-back row write that changes the reply text or its error on a busy store; a row for an error reply.
- OS-12 (O§7.4): CRLF changed in reuse text; an OSC with no end that eats the rest of the output.
- OS-13 (O§7.5): dedup that is not lossless at the end of text without a trailing newline.
- OS-27 (O§9): goldens blessed after shaping code exists, so `shape = false` is not checked against `97d8767`.
- OS-33 (O§7.13): `Exit code N` line hidden, cut or counted; `trim` not first under `PostToolUseFailure` for a member or with work units on.
- OS-16, OS-23 (O§8.1): a capture that keeps a home path, user name or host name; a CRLF capture normalized by git before `-text` is in `.gitattributes`.

## 7. Author's calls

1. **Order inside the track.** Edges are real dependencies only. Pure shaper nodes (OS-10 to OS-15) and fixture captures (OS-16 to OS-21) may start before P3-5 ends; the roadmap order P3-5 → P3-6 → P3-7 holds where it is real: the hook integration OS-28 depends on the P3-5 store nodes (through OS-4), and the P3-7 test nodes OS-35 to OS-38 depend on every shaper. Captures are P3-7 material but run early because each shaper node is tested on real output. The O§7.3 build order (steps 1-2, then 4, then 3) is kept: OS-12, OS-13 → OS-29 → OS-30.
2. **Price row of a request.** A catalog model row has prices per provider (`crates/zygo-config/src/schema.rs:202`). OS-8 takes the price of the provider that the `requests` row names; when that provider has no price or the row names none (a team session, §14 N16), the first provider of the model with a price (the rule of the test helper `list_prices`, `crates/zygo-hooks/tests/statusline_team.rs:154-156`). The tier comes from `long_context.above`, as `Prices::cost` (`schema.rs:280-289`).
3. **Unpriced count.** "`<n>` requests had no price" counts distinct request rows (OS-6 returns rowids; OS-7 counts distinct ones).
4. **Shape-off baseline.** O§16 item 11 compares with `97d8767` output. The baseline goldens are blessed in OS-27 on the current code before OS-28 adds shaping. `trim.rs` is unchanged between `97d8767` and `origin/phase-3` (verified with `git diff 97d8767 HEAD -- crates/zygo-hooks/src/trim.rs`); OS-1 to OS-9 change no hook output or raw file byte. No old binary is built or run.
5. **Failure fixtures.** For a case with `event = PostToolUseFailure`, `stdout` holds the hook input's `error` string and `stderr` is empty; `common::shaping::hook_input` (OS-16) builds the `error` field from it.
6. **Bounded open on a missing database.** `open_bounded` creates no file and runs no `CREATE TABLE`; a missing database or table is an error, so the hook writes no row (O-C1). The launcher's full open at launch (`crates/zygo-launch/src/launcher.rs:307`) creates the schema first.
7. **Panic seam.** O§8.4 asks for a `#[cfg(test)]` hook; integration tests cannot see `cfg(test)` of the library, so OS-28 uses the existing `test-hooks` feature (`crates/zygo-hooks/Cargo.toml:8`, pattern of `statusline/render.rs:96-110`) with `ZYGO_TEST_SHAPE_PANIC=1`.
8. **PO2 before it is known.** OS-28 builds both O§7.8 branches behind `TrimOpts.stderr_cleared` and tests both; OS-34 only sets the default from the PO2 row. So no shaping work waits for the operator's PO2 run.
9. **Test files.** O§14 names `tests/shaping.rs` and `tests/shaping_latency.rs`; this plan adds `shaping_hook.rs`, `shaping_off.rs`, `shaping_fixtures.rs` and `tests/common/shaping.rs` in `zygo-hooks`, and one test file per shaper in `zygo-results`, so that nodes do not share a test file without a reason (O§15 item 3).
10. **Fixture case names** under `fixtures/shaping/<shaper>/` are this plan's (O§15 item 4 leaves the source projects to the builder).
11. **Premise script.** O§17 says the premises run through `scripts/premises/`; no script for PO1-PO4 exists, so OS-22 writes `scripts/premises/live-shaping.sh` on `lib.sh` and `scrub.py`. `scripts/check-premises.sh` is not changed: its id lists (`IDS`, `WIDS`) check base-spec §19 notes, and O§17 asks for none.
