# LIV-2 — Windows part without a Windows machine: PH8/PH10 fallback (plain icons on Windows), W-SHELL part 2 text, not-run records

**Spec:** `docs/specs/2026-11-21-statusline-hud-design.md` AU2 (as changed 2026-11-22), H§12 PH8 and PH10 (fallback cells and their 2026-11-22 notes), H§14 #31 and #38, H§9 row `docs/live-checks.md:77`; change-log row 2026-11-22 "author's call, PH8/PH10/W-SHELL: no Windows machine available to the executor; fallback applied by default". The executor has no Windows machine and no Git Bash; it never stops for this node. Code: `icon_set_on` (`crates/zygo-hooks/src/statusline/render.rs:126-138`, doc comment `:126-128`, body `:129-138`) gives the Nerd set on Windows when `WT_SESSION` or `TERM_PROGRAM` is set (`:136`); after this node, with `ZYGO_NERD_FONT` unset or any value other than `1` and `0`, a Windows build (`windows = true`) gives the plain set whatever `WT_SESSION` and `TERM_PROGRAM` hold; `ZYGO_NERD_FONT=1` still gives Nerd and `0` plain; the `TERM` rule and non-Windows builds do not change; `color_mode` (AU23, `WT_SESSION` selects 256 colors) does not change. The doc comment of `icon_set_on` follows AU2. The `zygo doctor` icon line names the set the status line uses, so its test case with `TERM_PROGRAM` expects plain on a Windows build and Nerd elsewhere (`cfg!(windows)`). `docs/live-checks.md:77` gets the H§9 text ("shows the stripe, the logo (`𝘇𝘆𝗴𝗼` or `zygo`) and `ctx`"); the W-SHELL row keeps its 2026-11-20 result for part 1 and gains "part 2 with the new text: not run: no Windows machine (2026-11-22)". The stop report and the PR list PH8, PH10 and W-SHELL part 2 as "not run: no Windows machine". The windows-latest CI jobs (`cargo test --workspace`, `pwsh scripts/test-install.ps1`, `.github/workflows/ci.yml`) run at delivery. Also writes the `docs/live-checks.md` entry for `scripts/premises/live-workspace.sh` (P2), with its date, method and request cost from P2's results. Edge to PRB-1 for `docs/live-checks.md` and the HUD spec; edge to P2 for the `docs/live-checks.md` entry of `live-workspace.sh`; edge M1 → LIV-2 for `crates/zygo/tests/usage_doctor.rs`
**Files:** `crates/zygo-hooks/src/statusline/render.rs`, `crates/zygo-hooks/src/statusline/data.rs`, `crates/zygo-hooks/tests/statusline_render.rs`, `crates/zygo/tests/usage_doctor.rs`, `docs/live-checks.md`, `docs/specs/2026-11-21-statusline-hud-design.md`
**Consumes:** —
**Produces:** —
**Acceptance:** `cargo test -p zygo-hooks --locked --test statusline_render && cargo test -p zygo --locked --test usage_doctor && cargo test -p zygo --test forbidden_content && cargo test --workspace --locked --no-run` → 0 failed, including H§14 #31 with the new Windows cases: `icon_set_on` with `windows = true` gives plain for `WT_SESSION=x`, for `TERM_PROGRAM=vscode` and for both, Nerd for `ZYGO_NERD_FONT=1` with `WT_SESSION=x`, plain for `ZYGO_NERD_FONT=yes` with `TERM_PROGRAM=vscode`; with `windows = false` the old results; the doctor icon-line test passes on this build and states the Windows expectation by `cfg!(windows)`; `docs/live-checks.md:77` holds the new text and the W-SHELL row the not-run record for part 2

## Plan

### Findings against the live code (read 2026-11-22, branch head cfb5ec1)

1. The contract cited `icon_set_on` at `render.rs:128-137`. In the live code the doc comment is `render.rs:126-128` and the function is `:129-138`; the `legacy` line is `:136` as cited. The Spec line above now says `:126-138`.
2. The doctor and the HUD already share one helper: `doctor_report` pushes `icon_line(icon_set(env.env))` (`crates/zygo/src/cmd/doctor.rs:214`, import `:16`). So `crates/zygo/src/cmd/doctor.rs` needs no change, and the contract's Files list is correct without it.
3. `h14_32_doctor_icon_line_holds_the_nerd_glyphs_and_names_the_set` (`crates/zygo/tests/usage_doctor.rs:704-730`) calls `doctor_report`, which uses `cfg!(windows)` through `icon_set`. On this Linux build its `TERM_PROGRAM` case gives `nerd` before and after the change, so that test cannot fail first here. The failing-first test is `h14_31_icon_set_selection`, which injects `windows = true`. The Windows branch of the doctor test runs in the `windows-latest` job of `.github/workflows/ci.yml:12,22` at delivery.
4. After this node, no HUD rule reads `TERM_PROGRAM`, but `HUD_ENV` (`crates/zygo-hooks/src/statusline/data.rs:461-473`, doc "The environment variables that the HUD reads (H§7)") still lists it. Its only user is `dump` (`data.rs:478`), the `ZYGO_TEST_HUD_DUMP` seam. Author's call (review M8): `data.rs` joins the contract Files; the list keeps `TERM_PROGRAM` as evidence for a later Windows run of PH10, and step 2.2 corrects the doc comment.
5. `docs/live-checks.md` is in the scan scope of `scripts/check-premises.sh` (`:140-145`), which the contract's Acceptance does not run. Step 3.8 runs it.
6. The HUD spec cites `docs/live-checks.md` by line numbers that moved. AU30 (`2026-11-21-statusline-hud-design.md:125`) and the PH8 row (`:585`) cite `docs/live-checks.md:104` for the W-SHELL result, which is now the W-SHELL row at `docs/live-checks.md:141`. H-C4 (`:55`) cites `docs/live-checks.md:93` for P-N28-1, which is now at `:130`. The `:77` citations (`:517`, `:675`, `:695`) stay correct, because no step adds a line before line 77. The HUD spec joins the contract Files (it was in the Spec line only), and step 3.7 cites the rows by name.
7. The P2 cost and side effect come from P2's recorded results, not from the script header: 12 real requests over 6 full runs (2 for each full run) on the operator's login, and 6 empty `projects/<scratch repository>/memory` folders left in place in the operator's Claude config folder (`docs/work/phase2/graph.md:199`; `nodes/P2.md:1029`, Round 2 M2).
8. Note 5.6 (`live-workspace.sh:428-430`) tests whether the project folder `<config dir>/projects/<scratch repository path>` exists (`:428`, not its `memory` subfolder) and whether a config file has a `projects` entry for the repository (`:429`). The `docs/live-checks.md` text says exactly that.
9. The line-range fix of the Spec line (finding 1) and the two new Files entries (findings 4 and 6) are the contract corrections of this plan.

**Builds on:** (current signatures, unchanged by this node)
- `pub fn icon_set_on(env: &dyn Fn(&str) -> Option<OsString>, windows: bool) -> IconSet` (`crates/zygo-hooks/src/statusline/render.rs:129`)
- `pub fn icon_set(env: &dyn Fn(&str) -> Option<OsString>) -> IconSet` (`render.rs:122`, body `icon_set_on(env, cfg!(windows))`)
- `pub fn color_mode(env: &dyn Fn(&str) -> Option<OsString>) -> ColorMode` (`render.rs:142`)
- `fn icon_line(active: IconSet) -> String` (`crates/zygo/src/cmd/doctor.rs:224`)
- test helpers `fn vars(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<OsString>` (`crates/zygo-hooks/tests/statusline_render.rs:112`) and `fn icon_lines(p: &Paths, vars: &[(&str, &str)]) -> Vec<String>` (`crates/zygo/tests/usage_doctor.rs:690`)

**Plan produces:** `pub fn icon_set_on(env: &dyn Fn(&str) -> Option<OsString>, windows: bool) -> IconSet` with the same signature and the new AU2 rule (with `windows = true` and `ZYGO_NERD_FONT` not `1`: plain); the new H§14 #31 assertions; the doctor test with its `cfg!(windows)` expectation; the `HUD_ENV` doc comment of step 2.2; the `docs/live-checks.md` texts of steps 3.1-3.6; the HUD spec citations of step 3.7.

### Design decisions

- **D1. Windows detection: compile time, `cfg!(windows)`, in `icon_set` only (no change).** Each zygo binary is built for one target, so a runtime check (`std::env::consts::OS`) gives the same value and adds nothing. `icon_set_on` keeps its injected `windows: bool`. That is the test seam: the Linux tests cover the Windows branch with `windows = true`. Nothing in this node needs a Windows machine.
- **D2. One helper.** The doctor keeps `icon_set` (finding 2). The HUD and the doctor cannot differ, so the doctor test states only the build expectation by `cfg!(windows)`, as the contract asks.
- **D3. The rule.** `legacy` is replaced by `windows`: on a Windows build every automatic choice is plain, whatever `WT_SESSION` and `TERM_PROGRAM` hold. `ZYGO_NERD_FONT=1`/`0` and the `TERM` rule come first and do not change. `color_mode` is not touched (AU23: `WT_SESSION` still selects 256 colors). The existing assertion `statusline_render.rs:746` already guards this, so the plan adds none.
- **D4. `TERM_PROGRAM` stays in `HUD_ENV`, and its doc comment says why** (finding 4, step 2.2).
- **D5. The P2 entry in `docs/live-checks.md`** becomes a full section, in the same form as the P-HUD-1 and PRB-1 sections (command, offline test, output, parts, side effect, spend, bounds, traps, last run). It also gets one Results row with the id `P2/PW`: the other node rows use `<check>/<id>`, and a bare `P2` would clash with premise P2. The rows for each id stay in `docs/premises.md` and are not copied. Cost and side effect: finding 7. Bounds: every bound of `scripts/premises/live-workspace.sh`, read again at cfb5ec1. The doc text names no script line numbers, because they move; the plan keeps them as the source.
- **D6. W-SHELL row.** The date stays 2026-11-20 (the date of the result that it holds). The Result cell names both parts. The Evidence cell says that its part 2 status line is the pre-HUD one.
- **D7. Spec citations name the row, not a line** (finding 6). A change-log row records the edit.

Work in the node's checkout and run each command from the repository root. Stage only the six files of the contract's Files line and this file. Each commit message ends, after one empty line, with the executor's own `Zygo-Session:` trailer line and `Zygo-Work: LIV-2`.

### Task 1: the tests first

- [x] **1.1** In `crates/zygo-hooks/tests/statusline_render.rs`, `h14_31_icon_set_selection`: replace the two lines `:714-715` (`WT_SESSION` and `TERM_PROGRAM` with `true` → `IconSet::Nerd`) with the block below. Then replace the `icon_set` loop `:723-726` with the second block. Check: the file has no `icon_set_on(&vars(&[("WT_SESSION", "x")]), true), IconSet::Nerd` text left.

```rust
    // Windows: `WT_SESSION` and `TERM_PROGRAM` select no set (the PH8 and PH10 fallback, AU2 2026-11-22).
    assert_eq!(icon_set_on(&vars(&[("WT_SESSION", "x")]), true), IconSet::Plain);
    assert_eq!(icon_set_on(&vars(&[("TERM_PROGRAM", "vscode")]), true), IconSet::Plain);
    assert_eq!(icon_set_on(&vars(&[("WT_SESSION", "x"), ("TERM_PROGRAM", "vscode")]), true), IconSet::Plain);
    assert_eq!(icon_set_on(&vars(&[("WT_SESSION", "x"), ("TERM", "xterm-256color")]), true), IconSet::Plain);
    assert_eq!(icon_set_on(&vars(&[("ZYGO_NERD_FONT", "1"), ("WT_SESSION", "x")]), true), IconSet::Nerd);
    assert_eq!(icon_set_on(&vars(&[("ZYGO_NERD_FONT", "yes"), ("TERM_PROGRAM", "vscode")]), true), IconSet::Plain);
    assert_eq!(icon_set_on(&vars(&[("ZYGO_NERD_FONT", "0"), ("WT_SESSION", "x")]), true), IconSet::Plain);
    // Not Windows: the same variables give the old results.
    assert_eq!(icon_set_on(&vars(&[("WT_SESSION", "x")]), false), IconSet::Nerd);
    assert_eq!(icon_set_on(&vars(&[("TERM_PROGRAM", "vscode")]), false), IconSet::Nerd);
    assert_eq!(icon_set_on(&vars(&[("WT_SESSION", "x"), ("TERM_PROGRAM", "vscode")]), false), IconSet::Nerd);
    assert_eq!(icon_set_on(&vars(&[("ZYGO_NERD_FONT", "yes"), ("TERM_PROGRAM", "vscode")]), false), IconSet::Nerd);
```

```rust
    // `icon_set` is the rule for this build: on Windows the terminal variables give the plain set.
    for env in [vars(&[]), vars(&[("TERM", "linux")]), vars(&[("WT_SESSION", "x")]), vars(&[("TERM_PROGRAM", "vscode")])] {
        assert_eq!(icon_set(&env), icon_set_on(&env, cfg!(windows)));
    }
    let terminal = vars(&[("WT_SESSION", "x"), ("TERM_PROGRAM", "vscode")]);
    assert_eq!(icon_set(&terminal), if cfg!(windows) { IconSet::Plain } else { IconSet::Nerd });
```

- [x] **1.2** Run `cargo test -p zygo-hooks --locked --test statusline_render h14_31_icon_set_selection`. Check: 1 failed. The panic is at the first new line (`icon_set_on(&vars(&[("WT_SESSION", "x")]), true)`), with `left: Nerd` and `right: Plain`.
- [x] **1.3** In `crates/zygo/tests/usage_doctor.rs`, `h14_32_doctor_icon_line_holds_the_nerd_glyphs_and_names_the_set`: replace the comment and the `cases` array (`:712-721`) with the block below. Keep the loop and the rest of the test.

```rust
    // The set is the one the status line uses (AU2). `xterm` selects no set by itself on any platform. On a Windows
    // build `WT_SESSION` and `TERM_PROGRAM` select no set either (the PH8 and PH10 fallback), so there the
    // automatic set is plain; elsewhere it is nerd.
    let auto = if cfg!(windows) { &plain } else { &nerd };
    let cases: [(&[(&str, &str)], &String); 8] = [
        (&[("ZYGO_NERD_FONT", "1")], &nerd),
        (&[("ZYGO_NERD_FONT", "1"), ("TERM", "dumb")], &nerd),
        (&[("ZYGO_NERD_FONT", "0")], &plain),
        (&[("ZYGO_NERD_FONT", "0"), ("TERM", "xterm"), ("WT_SESSION", "1")], &plain),
        (&[("TERM", "linux")], &plain),
        (&[("TERM", "dumb"), ("ZYGO_NERD_FONT", "yes")], &plain),
        (&[("TERM", "xterm"), ("TERM_PROGRAM", "x"), ("NO_COLOR", "1")], auto),
        (&[("TERM", "xterm"), ("WT_SESSION", "1")], auto),
    ];
```

- [x] **1.4** Run `cargo test -p zygo --locked --test usage_doctor h14_32_doctor_icon_line_holds_the_nerd_glyphs_and_names_the_set`. Check: 1 passed, 0 failed. This is expected on Linux (finding 3). The Windows branch runs in CI at delivery.

### Task 2: the rule

- [x] **2.1** In `crates/zygo-hooks/src/statusline/render.rs`, replace the doc comment and the body of `icon_set_on` (`:126-138`) with the block below. The signature does not change. Check: `cargo check -p zygo-hooks --locked` finishes with no error and no warning.

```rust
/// AU2. `ZYGO_NERD_FONT=1` forces the Nerd set and `0` the plain set. Any other value, or none: plain when `TERM`
/// is `linux` or `dumb`, or on Windows (`windows`) whatever `WT_SESSION` and `TERM_PROGRAM` hold (the PH8 and PH10
/// fallback, 2026-11-22: no Windows run showed that they reach the hook through Git Bash); else Nerd. `NO_COLOR`
/// has no part in it.
pub fn icon_set_on(env: &dyn Fn(&str) -> Option<OsString>, windows: bool) -> IconSet {
    match var(env, "ZYGO_NERD_FONT").as_deref() {
        Some("1") => return IconSet::Nerd,
        Some("0") => return IconSet::Plain,
        _ => {}
    }
    let console = matches!(var(env, "TERM").as_deref(), Some("linux" | "dumb"));
    if console || windows { IconSet::Plain } else { IconSet::Nerd }
}
```

- [x] **2.2** In `crates/zygo-hooks/src/statusline/data.rs`, replace the one doc line above `const HUD_ENV` (`:461`, `/// The environment variables that the HUD reads (H§7).`) with the three lines below. The list does not change. Check: `cargo check -p zygo-hooks --locked --features test-hooks` finishes with no error and no warning, and `rg -n -F 'evidence for a later Windows run of PH10' crates/zygo-hooks/src/statusline/data.rs` prints one line.

```rust
/// The environment variables of H§7 that `dump` records (P-HUD-1). `TERM_PROGRAM` is read by no HUD rule since AU2
/// of 2026-11-22 (the PH8 and PH10 fallback: on Windows the plain set unless `ZYGO_NERD_FONT=1`); the dump records it
/// as evidence for a later Windows run of PH10.
```

- [x] **2.3** Run `cargo test -p zygo-hooks --locked --test statusline_render`. Check: 0 failed; `h14_31_icon_set_selection` passes.
- [x] **2.4** Run `cargo test -p zygo --locked --test usage_doctor`. Check: 0 failed.
- [x] **2.5** Commit `crates/zygo-hooks/src/statusline/render.rs`, `crates/zygo-hooks/src/statusline/data.rs`, `crates/zygo-hooks/tests/statusline_render.rs`, `crates/zygo/tests/usage_doctor.rs` and this file with steps 1.1-2.5 ticked. Message: `fix(statusline): plain icons on Windows unless ZYGO_NERD_FONT=1 (PH8, PH10 fallback)`.

### Task 3: `docs/live-checks.md`

- [x] **3.1** Line 77 (W-SHELL part 2). Replace the whole line with the line below. Check: `rg -o -n 'shows the stripe, the logo' docs/live-checks.md` prints exactly two lines, both `77:shows the stripe, the logo`, and `rg -n -F 'shows `zygo ◇`' docs/live-checks.md` prints nothing.

```text
6. **W-SHELL, part 2**: `zygo claude` (interactive) shows the stripe, the logo (`𝘇𝘆𝗴𝗼` or `zygo`) and `ctx` in its status line. On Windows the plain icon set is the default (AU2), so the logo is `zygo` unless `ZYGO_NERD_FONT=1`. Pass (parts 1 and 2): in each setup of part 1 the shell is bash or cmd (zygo's double-quoted path runs there), and the status line of part 2 shows the stripe, the logo (`𝘇𝘆𝗴𝗼` or `zygo`) and `ctx`. Fail: PowerShell (a quoted path needs `&` there; conformance A-1, A-9).
```

- [x] **3.2** W-SHELL row of the Results table (line 141). Replace `| 2026-11-20 | pass | Windows 11 x64 (build 26200)` with `| 2026-11-20 | part 1 pass; part 2 with the new text: not run: no Windows machine (2026-11-22) | Windows 11 x64 (build 26200)`. Replace the end of the same row, `` `zygo ◇ - ◇ - ◇ ctx -/- ◇ cache - ◇ trimmed 0`; §8.1 A-1/A-9 | ``, with `` `zygo ◇ - ◇ - ◇ ctx -/- ◇ cache - ◇ trimmed 0` (the status line of 2026-11-20, before the HUD; the part 2 text changed with the HUD, HUD design H§9 and H§14 #38, and part 2 with the new text has not run); §8.1 A-1/A-9 | ``. Check: `rg -n -F 'part 2 with the new text: not run: no Windows machine (2026-11-22)' docs/live-checks.md` prints one line, and that line starts `141:| W-SHELL |`.
- [x] **3.3** Line 3. Replace `H§12 (P-HUD-1, and the PRB-1 probe).` with ``H§12 (P-HUD-1, and the PRB-1 probe); `docs/specs/2026-11-20-workspaces-design.md` W§17 (node P2).``. Check: the file still has the W-SHELL part 2 text on line 77 (the step adds no line).
- [x] **3.4** Method bullets, lines 12-13 (the `c5l` part uses the stored login, the real `HOME` and the caller's Claude Code config folder; the other P2 parts point the agents at the mock directly, with no zygo between: `lib.sh:97-103` `agent_env`, `live-workspace.sh:155` `cc_print`, `:604-605` the OpenCode provider, `:528` the HTTP 400 upstream of PW7). In line 12, replace `every run except #28, the passthrough parts of P-HUD-1 and the Claude Code sessions of PRB-1 gets` with ``every run except #28, the passthrough parts of P-HUD-1, the Claude Code sessions of PRB-1 and part `c5l` of P2 (the real `HOME` and the caller's Claude Code config folder) gets``. In line 13, replace `with authored replies (not model output);` with ``with authored replies (not model output); the P2 parts other than `c5l` point the agents at the mock directly, with no zygo between (PW7's API error: an upstream of the script that answers HTTP 400);``, and replace `#28, P-HUD-1 (all parts but C7, which uses the mock) and PRB-1 use the claude.ai login` with ``#28, P-HUD-1 (all parts but C7, which uses the mock), PRB-1 and part `c5l` of P2 use the claude.ai login``. Check: `rg -o -n 'part `c5l` of P2' docs/live-checks.md` prints exactly two lines, `12:part `c5l` of P2` and `13:part `c5l` of P2`, and line 77 still holds the W-SHELL part 2 text (no line added).
- [x] **3.5** Replace the section `## Workspace agent premises (node P2): side effect` (its heading line, the empty line and its one bullet, which ends with `The full entry for the script is written by node LIV-2.`) with the text below. Check: `rg -n -F 'written by node LIV-2' docs/live-checks.md` prints nothing, and `rg -n '^## Workspace agent premises \(node P2\)$' docs/live-checks.md` prints one match.

````text
## Workspace agent premises (node P2)

Node P2; `docs/specs/2026-11-20-workspaces-design.md` W§17 (PW1, PW2, PW3, PW4, PW5, PW7, PW9, PW10, PW16). From the checkout, with tmux, `jq`, `python3` and `perl` (it ran on Linux; macOS not run), Claude Code logged in, and `claude` and `opencode` on `PATH` (OpenCode installs to `$HOME/.opencode/bin`, which the caller adds). PW3 also needs a second Claude Code at or above the pinned version and below the one on `PATH`: `PW3_OLD_CLAUDE=<path>`, else the highest such version file beside the program on `PATH`.

```sh
PATH="$HOME/.opencode/bin:$PATH" sh scripts/premises/live-workspace.sh
```

- No agent: `sh scripts/premises/workspace_test.sh` tests the record helpers (`workspace_lib.sh`) and the analyses (`workspace_analyze.py`) on stub data.
- It prints one `LIVE <id> verified|failed (agent <version>): …` line for each of the nine ids, writes `fixtures/claude-code/workspace/` and `fixtures/opencode/workspace/` at the end of the full run, and exits 0. A failed line does not change the exit status. With part names (`c1 c2 c5 c5l c7 oc`) it runs only those parts; the fixtures then stay in the scratch folder and the lines are not results.
- Parts `c1`, `c2`, `c5` and `c7` start Claude Code as an interactive TUI in panes of a tmux server of the script's own (`-S <socket in a private folder>`, `-f /dev/null`, 200 x 50) and type the prompts, `/clear`, `/exit` and Escape with `send-keys`. Part `c1` also makes four print-mode runs (PW16 from another cwd and with another config folder; PW3 on the older and then the newer Claude Code). Part `oc` runs OpenCode `run` and `session list` commands and one OpenCode TUI pane. These parts use a temporary `HOME`, the dummy key and the mock upstream (PW7's API error: an upstream of the script that answers HTTP 400): no login is read and no model request leaves the machine. OpenCode fetches its provider package from the package registry into the temporary `HOME`. PW9 sends SIGTERM to the idle agent process itself.
- Part `c5l` (sub-check 5.5) runs one print-mode turn of Claude Code on the stored login, with the real `HOME` and the caller's `CLAUDE_CONFIG_DIR` (else `~/.claude`), `--setting-sources project`, `--strict-mcp-config` and `--no-session-persistence`. Known side effect: each full run leaves one empty folder `<config dir>/projects/<scratch repository path>/memory` (Claude Code 2.1.330) for the run's temporary repository (`<scratch>/live-<pid>/c5l/repo`). The path differs in each run, so the folders add up: the six full runs of P2 on 2026-11-22 left 6 such folders in the operator's Claude config folder, and they stay in place. The script does not change or delete anything in the config folder. Note 5.6 of `fixtures/claude-code/workspace/pw5.txt` records two facts after the turn: whether the project folder `<config dir>/projects/<scratch repository path>` exists (it does not test the `memory` folder inside it), and whether the config file has a `projects` entry for the repository (2026-11-22: the folder yes, the entry no).
- Spend: part `c5l` only, one prompt on the login's default model: 2 model requests in each full run, counted against the login's usage and not billed to an API key. P2 made 6 full runs on 2026-11-22: 12 requests on the operator's login in all. The other parts send no model request out of the machine.
- Bounds. Claude Code panes: 60 s for each session start (the first `SessionStart` input) and for each plain turn; 120 s for the PW2 turn with a subagent; 30 s for the second `SessionStart` after `/clear` and for the `UserPromptSubmit` input of the blocked prompt; 10 s for each control request to reach the mock, and a window of 10 s in which no request with the blocked prompt may come; 60 s for the Bash command of the Esc turn to start, then 15 s for a `Stop` or `StopFailure` input; 15 s for the process to end after SIGTERM; 10 s for PW7's error upstream to start; 5 s for each pane program to end before `KILL`. Print mode on the mock: 90 s for each run. Part `c5l`: 180 s for the turn. OpenCode: 180 s for each `run`, 60 s for each `session list`, 60 s for the TUI to load the plugin, 90 s for the request of the prompt typed into the TUI, 15 s for the TUI to end after SIGTERM. A bound that passes makes the sub-checks that depend on it `FAIL` with the bound in the text, and the script goes on. These bounds write a `note` line instead, which is not part of the verdict: 120 s for the `SubagentStop` input of the PW2 subagent (note 2.5); in PW7, 90 s for the turn with the foreground subagent, 60 s for the turn that starts the background subagent, 60 s for two `SubagentStop` inputs, 60 s for the full-window turn and 90 s for the turn after it (notes 7.7 to 7.11); 15 s for Claude Code to end after `/exit` (note 9.4); 30 s for the OpenCode idle event in the TUI (note 4.6). A print-mode or OpenCode command over its bound gets `TERM`, then `KILL` 2 s later.
- The traps end the private tmux server, the mock and every agent and helper process, and remove the private folder.
- Last run: 2026-11-22, Linux, Claude Code 2.1.330 (PW3: first launch on 2.1.329) and OpenCode 1.19.6: PW1, PW2, PW3, PW4, PW5, PW9 and PW10 verified; PW7 and PW16 failed. Rows: `docs/premises.md` PW1 to PW16, and row P2/PW below.
````

Source of the Bounds bullet (`scripts/premises/live-workspace.sh` at cfb5ec1). Session start `:199`, `:223`, `:310`, `:360`, `:455`, `:532` (`cc_ready`, `:135-140`). Plain turns `:207`, `:234`, `:318`, `:364`, `:459`, `:536`. PW2 subagent turn `:332`. `/clear` `:322`. `UserPromptSubmit` `:387`. Control requests `:382`, `:392`, and the 10 s window `:391`. Esc `:464`, `:468`. SIGTERM `:212`. Error upstream `:525`. Pane end `:146`. Print mode `:255`, `:260`, `:283`, `:288` (`cc_print`, `:153-159`). `c5l` `:416`. OpenCode `run` `:610`, `:619`, `:630`, `:660`, `:701`; `session list` `:612`, `:694`; plugin load `:678`; TUI request `:683`; TUI end `:691`. Note lines (`note()`, `workspace_lib.sh:17`): `:335`, `:477-482`, `:248-249`, `:686-688`. `TERM` then `KILL`: `live.sh:131`. The 5 s HTTP timeout of one control request (`:170`) is inside the 10 s bound and is not listed.

- [x] **3.6** Add this row after the last row of the Results table (the row `| PRB-1/PH13 |`, the last line of the file). Check: `rg -n -F '| P2/PW |' docs/live-checks.md` prints one line, and its line number is the line count that `rg -c '' docs/live-checks.md` prints.

```text
| P2/PW | Workspace agent premises: session ids, hooks, resume and SIGTERM on Claude Code and OpenCode (workspaces design W§17 PW1-PW5, PW7, PW9, PW10, PW16) | Claude Code + OpenCode | 2.1.330 + 1.19.6 | 2026-11-22 | 7 verified, 2 failed | Linux (macOS not run); `scripts/premises/live-workspace.sh`, mock upstream, and one turn on the stored login (sub-check 5.5; 2 model requests in each full run, 12 in all over the 6 full runs of P2). PW1, PW2, PW3, PW4, PW5, PW9, PW10 verified; PW7 failed (7.2, 7.3, 7.5); PW16 failed (16.2). Side effect: 6 empty `projects/<scratch repository>/memory` folders left in the operator's Claude config folder. `fixtures/claude-code/workspace/pw*.txt`, `fixtures/opencode/workspace/pw4.txt`; one row per id in `docs/premises.md`; W§17 |
```

- [x] **3.7** In `docs/specs/2026-11-21-statusline-hud-design.md`, cite `docs/live-checks.md` rows by name, not by line (finding 6). In AU30 (`:125`) and in the PH8 row (`:585`), replace each `` (`docs/live-checks.md:104`) `` with `` (`docs/live-checks.md`, Results row W-SHELL) ``. In H-C4 (`:55`), replace `` (`docs/live-checks.md:93`) `` with `` (`docs/live-checks.md`, Results row P-N28-1) ``. After the last change-log row (`:733`), add the row below. Check: `rg -n -o 'live-checks.md:(93|104)' docs/specs/2026-11-21-statusline-hud-design.md` prints nothing; `rg -o -n 'Results row W-SHELL' docs/specs/2026-11-21-statusline-hud-design.md` prints exactly two lines, `125:` and `585:`; the last line of the file is the new row.

```text
| 2026-11-22 | H-C4, AU30, H§12 row PH8 | LIV-2 (citation fix) | The citations of `docs/live-checks.md` named lines 93 and 104, which moved when rows were added; they now name the Results rows P-N28-1 and W-SHELL. |
```

- [x] **3.8** Run `sh scripts/check-premises.sh` and `cargo test -p zygo --test forbidden_content`. Check: both exit 0. If `check-premises.sh` names a line of `docs/live-checks.md`, change that line of steps 3.1-3.6 until it passes, and keep its meaning.
- [x] **3.9** Commit `docs/live-checks.md`, `docs/specs/2026-11-21-statusline-hud-design.md` and this file with steps 3.1-3.9 ticked. Message: `docs(live-checks): W-SHELL part 2 text and not-run record; live-workspace.sh entry (P2); HUD spec row citations`.

### Task 4: acceptance and report

- [x] **4.1** Regression run of the packages that render or print the icon set: `cargo test -p zygo-hooks --locked` and `cargo test -p zygo --locked --test compact_cmd --test statusline_git --test statusline_cmd --test usage_doctor`. Check: both exit 0, 0 failed.
- [x] **4.2** Run the Acceptance: `cargo test -p zygo-hooks --locked --test statusline_render && cargo test -p zygo --locked --test usage_doctor && cargo test -p zygo --test forbidden_content && cargo test --workspace --locked --no-run`. Check: exit 0, 0 failed in each test run. Then `rg -o -n 'shows the stripe, the logo' docs/live-checks.md` prints exactly two lines, both starting `77:`, and `rg -n -F 'part 2 with the new text: not run: no Windows machine (2026-11-22)' docs/live-checks.md` prints one line, the W-SHELL row. Tick 4.1 and 4.2 and commit this file alone (message `docs(work): LIV-2 plan steps done`).
- [ ] **4.3** The node's stop report (and later the PR text) holds these three lines, word for word: `PH8: not run: no Windows machine (fallback applied: plain icons on Windows unless ZYGO_NERD_FONT=1)`, `PH10: not run: no Windows machine (fallback applied, the same rule)`, `W-SHELL part 2: not run: no Windows machine; part 1 keeps its 2026-11-20 pass`. The report also says that the Windows branch of `h14_32_doctor_icon_line_holds_the_nerd_glyphs_and_names_the_set` and of `icon_set` runs first in the `windows-latest` CI job at delivery. Check: the report text contains the three lines.
