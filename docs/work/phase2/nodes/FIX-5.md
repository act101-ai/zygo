# FIX-5 — HUD fit width: the status area is the terminal width less 4 (PH4 fallback; changes HUD-8)

**Spec:** `docs/specs/2026-11-21-statusline-hud-design.md` H§6.7, H§12 row PH4 (fallback column: "subtract the difference found by P-HUD-1"), H§14 #20; LIV-1 contract (a failed P-HUD-1 line creates a node for a DONE node before DOC-1); `docs/live-checks.md` row P-HUD-1/PH4; `fixtures/claude-code/hud/p-hud-1-fit.json`, `p-hud-1-width.txt`, `p-hud-1-compact-19.txt`
**Files:** `crates/zygo-hooks/src/statusline/width.rs`, `crates/zygo-hooks/src/statusline/render.rs`, `crates/zygo-hooks/tests/statusline_render.rs`, `docs/specs/2026-11-21-statusline-hud-design.md`
**Consumes:** —
**Produces:** `statusline::width::status_width(env: &dyn Fn(&str) -> Option<OsString>, tty: &dyn Fn() -> Option<u16>) -> u16`, `statusline::width::STATUS_INDENT: u16`, `statusline::width::STATUS_RIGHT_GAP: u16`
**Acceptance:** `cargo test -p zygo-hooks --locked && cargo test -p zygo --locked --test compact_cmd` → 0 failed, with `ph4_status_width_is_the_terminal_less_the_margins ... ok`, `h14_20_compact_drops_segments_in_order_and_width_sources ... ok` and `h14_01_golden_files_hold_each_render_byte_for_byte ... ok`; no file under `crates/zygo-hooks/tests/golden/` changed

## Plan

Work in the node's checkout. Run each command from the repository root. Each commit message ends, after one empty line, with the two adjacent lines `Zygo-Session: 01M80K6VKVFKHN736B0QCWTTKZ` and `Zygo-Work: FIX-5`. In each commit, tick (`- [x]`) the steps of this file that are done. Stage only the files of the contract and this file.

Facts of the live code (read 2026-11-22, main at 0be75eb):

- `terminal_width` (`width.rs:14-18`) returns the first source in the range 20 to 1000: `tty()`, then `COLUMNS`, else `DEFAULT_WIDTH` = 120 (`:8`).
- `RenderEnv::from_env` sets `width: terminal_width(env, &tty_columns)` (`render.rs:109`); `render.rs:11` imports it. `fit` (`render.rs:777-787`) drops segments until the row has `env.width` cells or fewer.
- `data::width_source` (`data.rs:704-718`) calls `terminal_width` and names the source that gives the result. `log_width` writes `statusline: width source <s>, width <n>`. This node does not change `data.rs`: the log keeps the terminal width, which `scripts/premises/live-hud.sh` reads (LIV-1 D2).
- Test helpers: `renv_from` (`statusline_render.rs:121-123`) uses `terminal_width`; `at` in `h14_20_…` (`:521-525`) sets `COLUMNS`; the width-source block is at `:555-570`; `from_env_reads_everything_through_its_closure` asserts `(20..=1000).contains(&e.width)` (`:742`).
- Golden rows (rendered at `COLUMNS=200`, `fixture.json:26`) have 109 cells or fewer, so W = 196 does not change them.
- Live evidence (P-HUD-1, Claude Code 2.1.330): `/dev/tty` does not answer the hook; `COLUMNS` is the terminal width; the status line starts 2 cells from the left edge and is cut with `…` 2 cells before the right edge.

Design calls: (a) the source order does not change (H§12 PH4: a failed source is skipped); (b) when no source answers, W = 120 (H§6.7 step 3; 120 is an assumed status-area width, so nothing is subtracted); (c) `terminal_width` and the width-source log keep the terminal width T; (d) two named constants hold the margins, with the evidence in their comments; (e) the margins apply to the `tty` source as well as to `COLUMNS` (they belong to Claude Code's status area, not to a source).

### Task 1: the failing tests

- [x] **1.1** In `crates/zygo-hooks/tests/statusline_render.rs`, change the import at `:12` to `use zygo_hooks::statusline::width::{cells, status_width, terminal_width, truncate, STATUS_INDENT, STATUS_RIGHT_GAP};`, and change `renv_from` (`:122`) to `width: status_width(env, &no_tty)`.
- [x] **1.2** In `h14_20_compact_drops_segments_in_order_and_width_sources`, replace the closure `at` (`:521-525`) with:

```rust
    // The row for `COLUMNS` = n (H§6.7: W = n - 4).
    let cols = |n: usize| {
        let columns = n.to_string();
        let env = env_of(&v, &[("COLUMNS", columns.as_str())]);
        strip(&render(&d, &renv_from(&env, C)))
    };
    // The row for the fit width w.
    let at = |w: usize| cols(w + usize::from(STATUS_INDENT + STATUS_RIGHT_GAP));
```

Replace `assert_eq!(at(200), steps[0].1);` with:

```rust
    assert_eq!(cols(200), steps[0].1);
    // PH4: the status area is 4 cells narrower than the terminal: the 115-cell row needs `COLUMNS` 119.
    assert_eq!(cols(119), steps[0].1);
    assert_eq!(cols(118), steps[1].1);
```

Replace `assert_eq!(at(20), format!("▌▌▌ {LOGO} {BRANCH} main"));` and `assert_eq!(cells(&at(20)), 16);` with `assert_eq!(cols(20), format!("▌▌▌ {LOGO} {BRANCH} main"));` and `assert_eq!(cells(&cols(20)), 16);`. In the draining loop, change `assert!(at(columns).starts_with(&head), "{columns}");` to `assert!(cols(columns).starts_with(&head), "{columns}");`. Do not change the `terminal_width` assertions at `:555-570`.
- [x] **1.3** Add this test after `h14_20_…`:

```rust
/// H§6.7, PH4 (P-HUD-1 on Claude Code 2.1.330): the status area starts 2 cells in and ends 2 cells before the right
/// edge, so W is the terminal width less 4. With no source, W stays 120. The terminal width does not change.
#[test]
fn ph4_status_width_is_the_terminal_less_the_margins() {
    assert_eq!((STATUS_INDENT, STATUS_RIGHT_GAP), (2, 2));
    let tty = |n: u16| move || Some(n);
    assert_eq!(status_width(&vars(&[("COLUMNS", "110")]), &tty(90)), 86);
    assert_eq!(status_width(&vars(&[("COLUMNS", "110")]), &no_tty), 106);
    assert_eq!(status_width(&vars(&[]), &no_tty), 120);
    assert_eq!(status_width(&vars(&[("COLUMNS", "5")]), &tty(5000)), 120);
    assert_eq!(status_width(&vars(&[("COLUMNS", "abc")]), &no_tty), 120);
    assert_eq!(status_width(&vars(&[("COLUMNS", "20")]), &tty(19)), 16);
    assert_eq!(status_width(&vars(&[("COLUMNS", "1001")]), &tty(1000)), 996);
    assert_eq!(terminal_width(&vars(&[("COLUMNS", "110")]), &no_tty), 110);
}
```

- [x] **1.4** In `from_env_reads_everything_through_its_closure`, change `assert!((20..=1000).contains(&e.width));` to `assert!((16..=996).contains(&e.width));`, and the comment above it to `// The width is the terminal's (when the test run has one), else COLUMNS, less 4 (H§6.7).`
- [x] **1.5** Run `cargo test -p zygo-hooks --locked --test statusline_render`. Expected: FAIL to compile, `unresolved imports` for `status_width`, `STATUS_INDENT` and `STATUS_RIGHT_GAP`.

### Task 2: the fit width

- [x] **2.1** In `crates/zygo-hooks/src/statusline/width.rs`, after `RANGE` (`:10`), add:

```rust
/// Claude Code draws the status line this many cells from the left edge of the terminal (P-HUD-1 PH4, Claude Code
/// 2.1.330, 2026-11-22: `fixtures/claude-code/hud/p-hud-1-fit.json` `"indent": 2`; `p-hud-1-compact-19.txt`).
pub const STATUS_INDENT: u16 = 2;
/// Claude Code cuts the status line with `…` this many cells before the right edge of the terminal (P-HUD-1 PH4,
/// 2026-11-22: `p-hud-1-fit.json` `"gap": 2` at 19 columns).
pub const STATUS_RIGHT_GAP: u16 = 2;
```

Replace `terminal_width` (`:12-18`) with:

```rust
/// The terminal width: the terminal (`tty`), else `COLUMNS` (through `env`), else 120. A source that gives a value
/// outside 20 to 1000, or no number, is skipped. The width-source log names this value (P-HUD-1).
pub fn terminal_width(env: &dyn Fn(&str) -> Option<OsString>, tty: &dyn Fn() -> Option<u16>) -> u16 {
    measured(env, tty).unwrap_or(DEFAULT_WIDTH)
}

/// The assumed width W of Claude Code's status area (H§6.7): the terminal width less `STATUS_INDENT` and
/// `STATUS_RIGHT_GAP` when a source answers; 120 when none does.
pub fn status_width(env: &dyn Fn(&str) -> Option<OsString>, tty: &dyn Fn() -> Option<u16>) -> u16 {
    measured(env, tty).map_or(DEFAULT_WIDTH, |n| n - STATUS_INDENT - STATUS_RIGHT_GAP)
}

/// The first source with a value in 20 to 1000: the terminal, then `COLUMNS`.
fn measured(env: &dyn Fn(&str) -> Option<OsString>, tty: &dyn Fn() -> Option<u16>) -> Option<u16> {
    let columns = || env("COLUMNS")?.to_str()?.trim().parse::<u16>().ok();
    let usable = |n: &u16| RANGE.contains(n);
    tty().filter(usable).or_else(|| columns().filter(usable))
}
```

- [x] **2.2** In `render.rs`, change the import at `:11` to `use super::width::{cells, status_width, truncate, tty_columns};`, change `:109` to `width: status_width(env, &tty_columns),`, and change the doc comment at `:101` to say "the width (`status_width` with the terminal probe `tty_columns`)".
- [x] **2.3** Run `cargo test -p zygo-hooks --locked --test statusline_render`. Expected: `0 failed`.
- [x] **2.4** Commit `width.rs`, `render.rs`, `statusline_render.rs` and this file: `fix(hooks): the HUD fits its rows to Claude Code's status area, 4 cells narrower than the terminal (PH4)`.

### Task 3: the spec

- [x] **3.1** In the HUD spec, replace the whole of H§6.7 with the text "H§6.7" under "Spec texts for step 3.1" below, the H§12 row PH4 with the text "H§12 row PH4", and H§14 item 20 with the text "H§14 item 20". Add the change-log row of that heading as the last row of the change log. Commit the spec and this file: `docs(spec): H§6.7 fit width is the terminal width less 4 (PH4 fallback)`.

### Task 4: verification

- [ ] **4.1** Run `cargo test -p zygo-hooks --locked && cargo test -p zygo --locked --test compact_cmd`. Expected: `0 failed`, with the three tests of the contract. Run `git status --short crates/zygo-hooks/tests/golden`. Expected: no output.
- [ ] **4.2** Commit this file with all steps ticked: `docs(work): FIX-5 plan steps done`.

### Spec texts for step 3.1

H§6.7 (the whole section, final text):

```markdown
### 6.7 Width

A hook cannot see the terminal width. The HUD first finds the terminal width T:

1. Unix: the size of the controlling terminal, `TIOCGWINSZ` on `/dev/tty` (no write; skipped when `/dev/tty` cannot be opened or the call fails). Inside tmux this is the pane's size, so tmux needs no step of its own. Under Claude Code 2.1.330 this source does not answer (PH4);
2. `COLUMNS` from the environment. Claude Code 2.1.330 sets it to the terminal width for the status line command (PH4);
3. else no source answers.

A value outside 20 to 1000 from any source is skipped. Each source is a premise (PH4).

Claude Code draws the status line 2 cells from the left edge of the terminal and cuts it with `…` 2 cells before the right edge (PH4, P-HUD-1 2026-11-22). The HUD fits each row to the width W of that status area: W = T − 4 when a source answers (the indent 2 and the right gap 2 are named constants in the code); W = 120 when no source answers (120 is an assumed status-area width, so nothing is subtracted). The session log names the source and T, not W (P-HUD-1).

Width counting uses Unicode East Asian width (the `unicode-width` crate). The logo counts 4 cells (PH9). Each Nerd private-use glyph counts as 2 cells, which is safe for both "Mono" and proportional Nerd Font variants. Escape bytes count 0.

When a row is wider than W, the HUD removes whole segments, first to last in this order, until the row fits:

- **Compact:** trimmed → cache → tokens → threshold `⇣` → spend → profile → model → bar cells (the numbers stay) → `stale` → worktree → ctx numbers → work item.
- **Verbose row 1:** mode → profile → effort → ahead/behind → model → worktree → work item. When row 1 drops a segment, row 2 does not change: the logo and the reflection keep their column.
- **Verbose row 2:** trimmed → cache → tokens → spend → percent → threshold → bar cells → `stale` → ctx numbers.

The stripe, the logo and its reflection, the workspace phase and `offline`/`suspended`/`ended` are never removed. A row that still does not fit is printed as it is; Claude Code then cuts or wraps it (premise PH3).
```

H§12 row PH4 (final text; only the last sentence of the note changes):

```markdown
| PH4 | The width sources reach the hook: (a) `/dev/tty` opens and `TIOCGWINSZ` gives the terminal (or tmux pane) width, so the hook has a controlling terminal; (b) `COLUMNS` is exported. Also: Claude Code's status area is the full terminal width. | None. `COLUMNS` is a shell variable that shells often do not export. *(Note 2026-11-22, LIV-1, P-HUD-1: failed on Claude Code 2.1.330, tmux 3.7c, Linux. (a) width source in tmux at 200 columns: COLUMNS 200; on a pseudo-terminal of 100 columns: COLUMNS 100; (b) COLUMNS in the hook: tmux 200, pseudo-terminal 100; (c) status area = pane width - 4 at 19 columns (indent 2, right gap 2), row at 70 columns: whole. `docs/live-checks.md` row P-HUD-1/PH4. FIX-5 builds the fallback: H§6.7 fits each row to T − 4.)* | A source that fails is skipped; with none, W = 120, which is acceptable (the full compact row of the OP13 data is about 100 cells). If the status area is narrower than the terminal, subtract the difference found by P-HUD-1. |
```

H§14 item 20 (final text):

```markdown
20. Fixture `width-all`: one status fixture with status data 100 s old (so `stale` shows), a `threshold` of 600k below a 1m window, a linked worktree, and every droppable segment present (work item, worktree, model, profile, ctx with bar, `⇣`, tokens, spend, cache, trimmed); its full compact row is 115 cells. The fit width W is `COLUMNS` − 4 (H§6.7). `COLUMNS=200` → all segments; `COLUMNS=119` → all segments; `COLUMNS=118` → no trimmed; then each smaller `COLUMNS` removes segments in the H§6.7 order, one test step per segment (the row before a step is whole at `COLUMNS` = its cells + 4 and loses the segment at one less); `COLUMNS=20` → exactly `▌▌▌ 𝘇𝘆𝗴𝗼 <branch icon> main` (stripe, logo, work item; 16 cells). With phase `draining` added, the phase stays at every `COLUMNS`. The stripe and the logo are present at every `COLUMNS`. Width sources: with a test seam for each, `/dev/tty` 90 wins over `COLUMNS` 110; without `/dev/tty`, `COLUMNS`; none → 120; any source giving 5 or 5000 is skipped; `COLUMNS=abc` → skipped. Fit width: `/dev/tty` 90 → 86; `COLUMNS` 110 → 106; `COLUMNS` 20 → 16; no source → 120.
```

Change-log row (append as the last row):

```markdown
| 2026-11-22 | H§6.7, H§12 row PH4 (note), H§14 #20 | H§12 PH4 fallback (FIX-5); author's call for: the source order is kept, no source keeps W = 120, the log keeps T | P-HUD-1 on Claude Code 2.1.330: /dev/tty does not answer the hook, COLUMNS is the terminal width, and the status area starts 2 cells in and ends 2 cells before the right edge; the HUD fits its rows to T − 4. |
```
