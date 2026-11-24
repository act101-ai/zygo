# Fragment SX — session text for each subagent and after a compaction (P3-14, P3-15)

Authority: `docs/specs/2026-11-24-session-text-design.md` (approved 2026-11-24), cited below as T§n. Node contracts: `docs/work/phase3/nodes/SX-*.md`. Code citations checked at `origin/phase-3` `768bf48`.

## 1. Rows

| ID | Node | Depends | TODO | Capability | — |
|----|------|---------|------|------------|---|
| SX-1 | Claude Code subagent branch of `hook session-start`: mechanics text on `SubagentStart` for every non-`zygo-` subagent (T§6.3 item 2) | — | TODO | implementation | — |
| SX-2 | OpenCode `session-start --event subagent`: mechanics text as plain text, no member part (T§7.2 item 2) | SX-1 | TODO | implementation | — |
| SX-3 | `hook pre-compact` sets `reinject/<agent_id>` for a non-`zygo-` subagent (T§9.2 item 1) | — | TODO | implementation | — |
| SX-4 | `hook trim` takes the flag and adds `additionalContext` = mechanics text, once (T§9.2 item 2) | SX-1, SX-3 | TODO | judgment | — |
| SX-5 | Claude Code launch registers `SubagentStart` → `hook session-start` on every launch (T§6.3 item 1) | — | TODO | implementation | — |
| SX-6 | OpenCode plugin transform: `--event subagent` for non-main `zygo/` models (T§7.2 item 1) | SX-2 | TODO | implementation | — |
| SX-7 | Base-spec amendments §8.5, §8.6 item 7, §8.6 last paragraph, §8.7 N33, §10.6 (T§13) | SX-4, SX-5, SX-6 | TODO | implementation | — |
| SX-8 | `docs/live-checks.md` section P3-15 (T§12.2) | — | TODO | mechanical | — |
| SX-9 | Live check P3-15, run by hand by the operator: evidence for ST1-ST4 | SX-8 | TODO | lookup | — |
| SX-10 | Record ST1-ST4: fixtures, premise rows, `check-premises.sh` ST arm, code change per result (T§12.1 items 1-5) | SX-7, SX-9 | TODO | judgment | — |

## 2. Files per node

| Node | Files |
|---|---|
| SX-1 | `crates/zygo-hooks/src/session_start.rs`, `crates/zygo-hooks/tests/subagent_text.rs` (new) |
| SX-2 | `crates/zygo-hooks/src/session_start.rs`, `crates/zygo-hooks/tests/subagent_text.rs` |
| SX-3 | `crates/zygo-core/src/session_dir.rs`, `crates/zygo-hooks/src/pre_compact.rs`, `crates/zygo-hooks/tests/reinject.rs` (new) |
| SX-4 | `crates/zygo-hooks/src/trim.rs`, `crates/zygo-hooks/tests/reinject.rs` |
| SX-5 | `crates/zygo-launch/src/claude_code.rs`, `crates/zygo-launch/tests/claude_code.rs` |
| SX-6 | `crates/zygo-launch/assets/opencode-plugin.js`, `crates/zygo-launch/tests/opencode.rs` |
| SX-7 | `docs/specs/2026-11-18-zygo-design.md` |
| SX-8 | `docs/live-checks.md` |
| SX-9 | — (the operator writes the three fixture files in T§12.2 step 6; SX-10 commits them) |
| SX-10 | `fixtures/claude-code/live/st-hooks.jsonl`, `fixtures/claude-code/live/st-evidence.txt`, `fixtures/opencode/live/st-transform.jsonl`, `fixtures/claude-code/hooks/SubagentStart-plugin-sub.json` (new), `docs/premises.md`, `scripts/check-premises.sh`, `docs/specs/2026-11-24-session-text-design.md` (failed-note only), `docs/specs/2026-11-18-zygo-design.md` (§8.7 ST4 sentence), `crates/zygo-hooks/tests/subagent_text.rs`; by result: `crates/zygo-launch/src/opencode.rs`, `crates/zygo-hooks/src/session_start.rs`, `crates/zygo-launch/assets/opencode-plugin.js`, `crates/zygo-launch/tests/opencode.rs` |

Intra-track shared files all have a path: `session_start.rs` and `subagent_text.rs` SX-1 → SX-2 → SX-6 → SX-7 → SX-10; `reinject.rs` SX-3 → SX-4; `opencode-plugin.js` and `tests/opencode.rs` SX-6 → SX-7 → SX-10; base spec SX-7 → SX-10.

## 3. Cross-track needs

- **DR first node → SX-1.** Brief: the assembler adds it (the build is governed by `docs/specs/2026-11-24-decision-records-design.md`, T header).
- **`crates/zygo-hooks/src/trim.rs` (OS).** SX-4 needs a dependency path with every OS node that edits `trim.rs`. Recommended order: SX-4 after the last OS node that changes `trim_in`'s output object (O§7.8 "Output, `PostToolUse`", O§ PostToolUseFailure branch), so the `additionalContext` merge goes into the final output shape and the flag check is placed before the final early returns once.
- **`crates/zygo-launch/src/claude_code.rs` `hooks()` (OS).** OS adds a `PostToolUseFailure` `trim` entry (output-shaping spec, "If PO1 holds" item 1); SX-5 adds a `SubagentStart` entry in the same function (`claude_code.rs:465-524`). Needs a path; either order works.
- **`crates/zygo-launch/tests/claude_code.rs`, `crates/zygo-launch/tests/opencode.rs`.** Any other node that edits these needs a path with SX-5 / SX-6 / SX-10.
- **`docs/specs/2026-11-18-zygo-design.md` (OS O§12, others).** SX-7 and SX-10 need a path with every node that amends the base spec.
- **`docs/premises.md` (OS rows PO1-PO4; mail-sync rows SP1-SP6) and `scripts/check-premises.sh`.** SX-10 needs a path with every node that adds premise rows or ID lists to the script; the `for id in $IDS $WIDS` line (`scripts/check-premises.sh:58`) and the failed-note `case` (`:104-108`) are edit points other tracks may also change.
- **`docs/live-checks.md` (workspace-definition spec line 485).** SX-8 needs a path with that track's live-checks node.
- **`crates/zygo-core/src/session_dir.rs`.** SX-3 adds `reinject()`; any other track node adding a session-folder path there needs a path with SX-3.
- No SX node edits `crates/zygo-hooks/tests/hooks_basic.rs` (author's call 2), so the OS trim tests there do not need ordering with SX.

## 4. Global constraints

- **T-C1 (C6)** Each new hook branch, on an internal error, prints nothing that changes a decision, writes one line to stderr, exits 0 (T§4; wrapper `crates/zygo-core/src/hook.rs:76`). Pre-compact flag failure: §10.6 text still printed (T§14).
- **T-C2** No change to an agent definition zygo did not write: user `--agents` merged as given (`crates/zygo-launch/src/claude_code.rs:418`), plugin and built-in prompts untouched (T§4).
- **T-C3** No CI test runs a real `claude` or `opencode`; stub agents and recorded fixtures only; no test reads a wall clock (T§4, T§17 check 15). Also operator rule: no wall-clock tests in CI.
- **T-C4** Added text names no context window, size or price (T§4).
- **T-C5** The added text is `mechanics(v)` (`crates/zygo-hooks/src/session_text.rs:18`) of the session view with the `overrides.json` `results` and `work_units` values applied, as `text_of` does (`crates/zygo-hooks/src/session_start.rs:33-39`). Default all-on M = 721 bytes (T§11, T§17 check 2).
- **T-C6** Where the Claude Code docs answer, follow them; a contradicting live result goes to the lead and changes no code by itself (T§4).
- **T-C-ZYGO** `zygo-` prefix rule = `is_zygo` (`crates/zygo-work/src/hooks.rs:159-161`); a missing or non-string `agent_type` is not `zygo-` (T§6.3 item 2).
- **T-C-NAME** Flag file name = `agent_id` passing the work-log agent-name rule: 1-64 bytes of `[A-Za-z0-9_-]` (`crates/zygo-work/src/log.rs:140-146`, `check_agent`, `pub(crate)` today). Directory `$ZYGO_SESSION_DIR/reinject/` mode `0700`, file `0600` (T§9.2 item 1).
- **T-C-TRIM** Flag re-inject rides only the existing trim entry, matcher `Bash|Read|Grep|Glob|WebFetch` (`crates/zygo-launch/src/claude_code.rs:476`); no new `PostToolUse` entry (T§15 item 8). The text is given only when the remove succeeds (T§9.2 item 2).
- **T-C-OC** Plugin hook calls keep the 10 s bound and `""` on failure (`crates/zygo-launch/assets/opencode-plugin.js:12-15`, `:27-28`; T§14).
- **T-C-PREM** Before ST rows exist the code is built for "no row yet": ST3 no change, ST4 `zygo-<role>` prompts keep the text and the transform branch exists; `check-premises.sh` has no `SIDS` (T§12.1). Tests never read `docs/premises.md`.
- **Hard rules (operator).** No Claude attribution in commits, PRs or files; commit trailer `Zygo-Session: <session>` and `Zygo-Work: <unit>` when a unit is named.

## 5. Verification floor additions

- `cargo test -p zygo-hooks --test subagent_text` and `--test reinject` on ubuntu, macOS, Windows (CI `cargo test --workspace --locked --no-fail-fast`, `.github/workflows/ci.yml:45`). File-mode assertions under `cfg(unix)` only.
- `cargo test -p zygo-launch --test opencode` node-driven plugin tests are `cfg(unix)` (`crates/zygo-launch/tests/opencode.rs:669`); they run on ubuntu and macOS only.
- `sh scripts/check-premises.sh` is not in CI; SX-10 runs it locally with `N2_FORBIDDEN_NAMES` set (T§12.1 item 1).

## 6. Review focus

1. **SX-4, T§9.2 item 2 / T§14:** `agent_id` from a `PostToolUse` input joined to `reinject/` without the name rule can remove a file outside `reinject/` (for example `../view.json`). The trim side must apply the same rule as pre-compact (via `flag_path`). Test with `agent_id` `../x`.
2. **SX-4, T§9.2 item 2:** the flag check runs before the trim's own early returns (`crates/zygo-hooks/src/trim.rs:20-25`, `:99-106`), including a non-hooked shape and results off; check both the under-threshold object and the merged over-threshold object, and a second run that prints nothing.
3. **SX-1, T§6.3 item 2:** both early returns (`session_start.rs:25-27` in `run_in`, `:67-69` in `run_member`) are replaced; the production path is `run_member` (`crates/zygo/src/cmd/hook.rs:26` → `session_start::run`). `agent_type` cases: `Explore`, `Plan`, `general-purpose`, `p315:probe`, number `1`, missing, `zygo-code` (T§17 check 3). A subagent `SessionStart` (`source: "compact"`) prints nothing (check 5).
4. **SX-1, T§9.1:** main-loop `source: "compact"` output equals `source: "startup"` output (check 6), with the input named as constructed in the test.
5. **SX-1, T§14 item 1:** a `SubagentStart` with an unparseable `view.json` → exit 0, empty stdout, one stderr line (check 12) — test through the C6 wrapper, not only the inner function.
6. **SX-2, T§7.2 item 2:** `--event subagent` returns before any member part: no resume note and no `view.json` write (check 11, byte-equal).
7. **SX-3, T§9.2 item 1:** `pre_compact::run` today ignores its input (`crates/zygo-hooks/src/pre_compact.rs:9`); the main input, `zygo-` types and bad ids create nothing; a create failure still prints the §10.6 text.
8. **SX-5, T§6.3 item 1:** member order `session-start` first, then `workspace --event subagent-start`; entry has no `--event` flag and no matcher.
9. **SX-6, T§7.2 item 1:** unset `ZYGO_MAIN_ROLE` with a `zygo` model goes to `--event subagent` (today it returns at `opencode-plugin.js:187`); a non-`zygo` provider calls nothing.
10. **SX-10, T§12.1 item 4:** the `ST*)` arm sits before the `P*)` arm (`scripts/check-premises.sh:106`) as the spec says; the ST failed-note pattern uses `P3-15`, not the `N2` of the shared `pat` (`:103`), and reads `$SSPEC`; the loop runs over `$IDS $WIDS $SIDS` (`:58`).
11. **All, T§17 check 15:** no new test spawns `claude`/`opencode` or reads a clock.

## 7. Author's calls

1. **Helper name.** SX-1 exposes `session_start::mechanics_with_overrides(dir, v)` so SX-2 and SX-4 share one T-C5 implementation (T§18 leaves names to the builder; the contract fixes this one for Consumes).
2. **New test files.** `crates/zygo-hooks/tests/subagent_text.rs` (SX-1, SX-2, SX-10) and `crates/zygo-hooks/tests/reinject.rs` (SX-3, SX-4) instead of `hooks_basic.rs`, which OS also edits (T§16 allows "or a new test file"; T§18 test layout).
3. **Name rule location.** `pre_compact::flag_path` holds the rule (repeated or shared from `zygo-work`, T§18); SX-4 consumes it so both sides use one check.
4. **SX-5 has no edge to SX-1.** Registering `SubagentStart` before the branch exists is harmless (today the hook returns nothing for `agent_id`); the graph still lands both before SX-7.
5. **SX-6 depends on SX-2.** Without SX-2, `session-start --event subagent` reaches `run_member` with `cli_event = "subagent"` and prints the full session text to subagents; the edge prevents that state on the branch.
6. **SX-7 depends on SX-4, SX-5, SX-6** so the base spec describes behavior that exists; the §8.7 ST4 sentence (T§13 last bullet) belongs to SX-10.
7. **SX-10 is one node** because T§12.1 items 2, 4 and 5 require the rows, the script change and the code change in the same commit. Its result-dependent files are listed in full so the assembler orders them now.
8. **SX-9 is the only edge to the live check.** No code node waits for it (T§12.1: code is built for "no row yet"); only SX-10 does.

## 8. Not plannable from the spec alone

- **SX-10 scope is unknown until SX-9:** which of ST3-F3, ST4-verified, ST4-failed applies decides the files actually changed (T§12.1 table, T§7.2 item 3, T§9.3). The Plan section must be written after the result.
- **SX-10 scratch-copy mutation checks:** `scripts/check-premises.sh` reads fixed repo paths (`:8`, `:11`), so check 13's negative cases need a copy of the repository (for example `git worktree add`); no test harness for this script exists today (`.github/workflows/ci.yml` does not run it).
- **SX-6, ST4 effect on the title request:** T§10 says the title request (`small_model`) gets the mechanics text if the transform runs for it; no test can separate it from a subagent request, by design (T§7.2).
