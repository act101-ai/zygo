# Fragment RT — session run time (P3-13)

Authority: `docs/specs/2026-11-24-session-runtime-design.md` (`T§n`, approved 2026-11-24). Base specs it amends: `docs/specs/2026-11-21-statusline-hud-design.md` (`H§n`), `docs/specs/2026-11-20-workspaces-design.md` (`W§n`), `docs/specs/2026-11-18-zygo-design.md` (`§n`). Code anchors checked at `origin/phase-3` `768bf48`.

## 1. Rows

| ID | Node | Depends | TODO | Capability | — |
|----|------|---------|------|------------|---|
| RT-1 | Member record `first_started_at`, set by each commit of a pending start | — | TODO | implementation | — |
| RT-2 | Run-time start rule, `<form>` text, and the `runtime:` line of `zygo status` | — | TODO | implementation | — |
| RT-3 | Run-time HUD segment in compact and verbose, with its place in the width-removal order | RT-2 | TODO | implementation | — |

RT-1 and RT-2 are both roots of this track. The assembler adds the DR-track dependency to the root(s) it chooses.

## 2. Files per node

| Node | Files |
|---|---|
| RT-1 | `crates/zygo-workspace/src/state.rs`, `crates/zygo-workspace/tests/state.rs`, `crates/zygo-hooks/src/workspace.rs`, `crates/zygo-hooks/tests/workspace.rs`, `crates/zygo/src/cmd/member.rs`, `docs/specs/2026-11-20-workspaces-design.md` |
| RT-2 | `crates/zygo-core/src/ids.rs`, `crates/zygo-hooks/src/statusline/phase.rs`, `crates/zygo-hooks/tests/statusline_phase.rs`, `crates/zygo-hooks/src/statusline/render.rs`, `crates/zygo-hooks/tests/statusline_render.rs`, `crates/zygo/src/cmd/status.rs`, `docs/specs/2026-11-18-zygo-design.md` |
| RT-3 | `crates/zygo-hooks/src/statusline/data.rs`, `crates/zygo-hooks/src/statusline/render.rs`, `crates/zygo-hooks/tests/statusline_render.rs`, `crates/zygo-hooks/tests/statusline_phase.rs`, `docs/specs/2026-11-21-statusline-hud-design.md` |

RT-1 shares no file with RT-2 or RT-3, so it runs in parallel with them. RT-2 and RT-3 share `render.rs`, `statusline_render.rs` and `statusline_phase.rs`; RT-3 depends on RT-2.

## 3. Cross-track needs

- **ML (P3-1, M§9a HUD segments): no functional dependency in either direction.** T§9 bullet 2 states the T§3.1 group lists hold the M§9a segments "so that the two amendments agree in whichever order they are built". RT adds `[run time]` as its own group at the end of compact and of verbose row 1; M§9a adds segments to the first group and a new row-1 group before the work item (M§9a table, "Place"). Neither consumes a name of the other.
- **Shared files with ML (the assembler must put a dependency path between the nodes that touch each one, graph-format §4 "Files"):**
  - `crates/zygo-hooks/src/statusline/render.rs` (RT-2, RT-3; ML adds `Seg` variants, `COMPACT`/`ROW_1` groups, `Glyphs` entries, M§9a "HUD changes").
  - `crates/zygo-hooks/src/statusline/data.rs` (RT-3; ML fills `HudData` with the name and the mail count).
  - `crates/zygo-hooks/src/statusline/phase.rs` (RT-2; ML may read the member name here or beside it, M§9a "follow the rules of the workspace phase segment").
  - `crates/zygo-hooks/tests/statusline_render.rs`, `crates/zygo-hooks/tests/statusline_phase.rs` (RT-2, RT-3; likely ML).
  - `docs/specs/2026-11-21-statusline-hud-design.md` (RT-3 writes the T§3.1 amendments, whose H AU9 lists already name the M§9a segments; ML writes the M§9a rows of H§6.2 and H§6.5).
  - `crates/zygo-workspace/src/state.rs` (RT-1; ML adds the member `name`, M§6 "`up` copies it into … the member record"; WD also changes it, `docs/specs/2026-11-24-workspace-definition-design.md:439`).
  - `crates/zygo-hooks/src/workspace.rs` (RT-1; ML changes the member `Stop`, `UserPromptSubmit` and `SessionStart` hooks, M§7.1-7.2).
  - `crates/zygo/src/cmd/member.rs` (RT-1; ML adds `--name` on a hand start, M§6).
  - `docs/specs/2026-11-20-workspaces-design.md` (RT-1 amends W§7.2 and W AU92; ML amends W§11 and W§7.2 for `name`).
  - `crates/zygo/src/cmd/status.rs` (RT-2 only, as far as the approved specs say; no other Phase 3 spec cites it).
- **Order the author suggests:** ML's HUD node before RT-3, so that the H AU9 text RT-3 writes matches the code at every commit; RT-1 after or before the ML state/hook node, by whichever keeps the critical path short. Either order is correct by T§9.
- **DR:** the assembler makes the first RT node(s) depend on the DR track's first node (brief). RT needs nothing from DR.

## 4. Global constraints

- **H-C2:** the whole status-line hook run has a 3 s deadline (`docs/specs/2026-11-21-statusline-hud-design.md:53`). The run time adds no process and no file read beyond the phase read it shares (T§2.6 "Cost").
- **One `workspace.json` read for phase and start** (T§2.6; H§7.6 amendment in T§3.1). No `zygo-workspace` dependency from the status-line module (H§7.6, `crates/zygo-hooks/src/statusline/phase.rs:5-6`).
- **Read rules of H AU25** for the start: `ws_id` matches `^[0-9a-f]{16}$` before any path (W AU84), regular file checked on the opened handle, at most 64 KiB, `version` 1, never cached, any failure gives no start and no line (T§2.2 rule 2).
- **`version` stays 1** (W AU29 unchanged, T§3.2); the new field is optional with a serde default, and unknown fields survive through `extra` (W AU78, `crates/zygo-workspace/src/state.rs:128-129`).
- **0 counts as unknown** for every start value (T§2.2; `crates/zygo/src/cmd/member.rs:197` writes 0 before 1970).
- **No timing tests in CI:** every test injects `now` (`ZYGO_TEST_NOW` under `test-hooks`, `crates/zygo-hooks/src/statusline/data.rs:584`, or the `now: u64` argument of `status::run`); no test reads the wall clock or waits (T§7 preamble; operator memory "No timing tests in CI").
- **Acceptance runs on ubuntu, macOS and Windows CI** with `cargo test --workspace --locked --no-fail-fast` (`.github/workflows`, matrix `[ubuntu-latest, macos-latest, windows-latest]`).
- **Text through `esc`:** the `runtime:` value is built from digits and unit letters only; it needs no escape, but the line sits beside lines that pass `zygo_plugins::check::esc` (`crates/zygo/src/cmd/status.rs:52`).
- **No Claude attribution** in commits or files; trailer `Zygo-Session: <session>` and `Zygo-Work: <unit>` (operator instructions).

## 5. Verification floor additions

- `cargo test --locked -p zygo-workspace -p zygo-hooks -p zygo -p zygo-core` on all three CI platforms (RT-1, RT-2, RT-3).
- `git diff --exit-code $(git merge-base HEAD origin/phase-3) HEAD -- crates/zygo-hooks/tests/golden` → no output after RT-3 (T§3.1 last row: the reference bytes stay those of a HUD without the run time).
- No clippy or fmt step exists in `.github/workflows` today; RT adds none.

## 6. Review focus

1. **RT-1, T§3.2 W AU92 addition:** the exit commit in `record_exit` (`crates/zygo/src/cmd/member.rs:472-497`) sets the field only on the branch that takes `pending`; a record already committed by the hook keeps the set value; `Exit::NotStarted` never sets it.
2. **RT-1, T§3.2 / T§4:** `pending.started_at = 0` leaves the field absent; a later commit with a non-zero value then sets it ("absent or 0").
3. **RT-1, T§5 "Hand start":** the record replaced at `member.rs:805-808` starts without the field (it comes from `MemberRecord::new`); the kept record (`:803-812`) keeps it.
4. **RT-2, T§2.2 rule 3:** a record whose `zygo_session` and `pending.zygo_session` both differ from `view.session` gives the session's own ULID start, not that record's `first_started_at`.
5. **RT-2, T§2.2 rule 2:** a member whose `workspace.json` fails any read check gives no start at all; it must not fall back to the ULID start (the fallback is only for rules 3 and 6).
6. **RT-2, T§2.3:** floor at each unit, two-digit second unit, `u64::MAX` gives a form with no overflow (`d / 86400` etc., no `+` on `d`).
7. **RT-2, T§2.5:** the `runtime:` line sits right after the `session:` line in both the `Ok` and the `unreachable` branch, and inside the `hide` (C7) form too.
8. **RT-3, T§2.6 / H§7.6 amendment:** `collect` reads `workspace.json` once for the phase and the start; the old `phase::read(paths, env)` call at `data.rs:586` is replaced, not kept beside the new read.
9. **RT-3, T§2.4 "Kind" / H AU20:** the run time is a local segment: `stale` does not dim it and an `ended` outcome does not drop it.
10. **RT-3, T§2.4 "Width counting":** the Nerd clock glyph counts 2 cells in the H§6.7 fit.

## 7. Author's calls

- **Three nodes, not one:** the state change (RT-1) has no code link to the readers (the reader has its own serde struct, H§7.6), so it runs in parallel; the start rule and the `zygo status` line (RT-2) are one unit because `status.rs` is the first consumer of the public start function and the format function; the HUD segment (RT-3) is the biggest test surface (bytes, width) and owns the H amendments.
- **Capability `implementation` for RT-1**, not `judgment`: the migration is an optional serde field with a default, and W AU78 already keeps unknown fields; T§4 settles every case.
- **The format function lives in `render.rs`** (T§6 row `render.rs`, "the public T§2.3 format function"), as `spend_text` already does for both the HUD and `zygo status` (`render.rs:253`, `status.rs:8`).
- **Produced names are proposals** where T§6 gives none: `phase::MemberRead`, `phase::member_read`, `phase::start`, `render::run_time_text`, `HudData.run_time`. T§6 fixes `ulid_secs(&str) -> Option<u64>` and `status::run` taking `now: u64`.
- **`phase::read(paths, env)` stays** (its tests in `statusline_phase.rs` keep passing); it may become a thin wrapper over `member_read`. The builder of RT-2 decides.
- **Base §17 amendment goes in RT-2** (it ships the `runtime:` line); W amendments in RT-1; all H amendments in RT-3.
