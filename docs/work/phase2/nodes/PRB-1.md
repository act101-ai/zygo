# PRB-1 — Probe: temporary statusLine command for PH1-PH7, PH9, PH11-PH13

**Spec:** `docs/specs/2026-11-21-statusline-hud-design.md` H§12 (probe first), H§6.3 (the two OP13 `printf` commands), H§17 note 4. Needs a live login: the executor runs the live check from its own session on this machine, with passthrough and the operator's Claude Code login (operator, 2026-11-21: "you can test with passthrough"); pinned Claude Code in a terminal; a pinned version is a minimum (operator, 2026-11-21: Claude Code 2.1.330 meets the 2.1.328 pin). Adds PH3, PH6, PH7 (visible in the same capture) and PH13 (OpenCode help) to the H§12 list; PH8 and PH10 need Windows and Git Bash, so LIV-2 records them. Results are dated notes in the H§12 table. PH4 includes the status area width, read from the probe capture.
**Files:** `scripts/premises/live-hud-probe.sh`, `scripts/premises/live-hud-probe_test.sh`, `scripts/premises/hud_probe.py`, `scripts/premises/hud_probe_test.py`, `scripts/premises/ver_test.sh`, `scripts/premises/live-claude.sh`, `scripts/premises/live-deadline.sh`, `scripts/premises/live-opencode.sh`, `scripts/premises/live-passthrough.sh`, `scripts/premises/live-strict.sh`, `scripts/premises/live-16b.sh`, `scripts/premises/live-window.sh`, `scripts/premises/live-rt.sh`, `fixtures/claude-code/hud/`, `docs/specs/2026-11-21-statusline-hud-design.md`, `docs/live-checks.md`
**Consumes:** —
**Produces:** —
**Acceptance:** `sh scripts/premises/live-hud-probe.sh && cargo test -p zygo --test forbidden_content` → exit 0 with one `LIVE <id> verified|failed|observed` line each for PH1, PH2, PH3, PH4, PH5, PH6, PH7, PH9, PH11, PH12, PH13; stdin and environment logs written as fixtures through `scripts/premises/scrub.py`; a dated note per id in H§12. A `failed` line still passes this node; its H§12 fallback applies before LIV-2 starts. LIV-2 (TODO) is re-planned when the fallback changes its acceptance; the DONE nodes whose behaviour the fallback changes are recorded in the graph Log, and the executor adds one new fix node for them: PH1 → fix node for HUD-8, HUD-9, HUD-10, H2, LIV-1, and re-plan LIV-2; PH2 → fix node for HUD-8, HUD-11, LIV-1, H2; PH3 → fix node for HUD-8; PH9 → fix node for HUD-8, and re-plan LIV-2; PH4, PH5, PH11 → fix node for HUD-8, HUD-9 (PH4: FIX-5 already applied the fallback); PH7 → fix node for HUD-8, HUD-12; PH6, PH12, PH13 → none

## Plan

Re-plan of 2026-11-22 (round 2). It replaces the draft of 2026-11-21: that draft predates PRB-2 (`ver.sh`, `agent_ok`, the `results.txt` appends), follows the OpenCode HUD design (not authority; operator: "same as Claude Code"), asks the operator four visual questions, and had 22 open review findings. What was dropped from it and why: the `ver_ge` and `pinned_agents` steps (PRB-2 merged them, `scripts/premises/ver.sh`, `scripts/premises/live.sh:56-89`); the operator steps and the y/n questions (the executor runs live checks from its own session, operator 2026-11-21); the edits of the H§12 intro, the "Probe first" paragraph and H§17 note 4 (spec edits are dated notes in H§12 only, controller 2026-11-22); the `padding: 0` key (the zygo HUD sets none, `crates/zygo-launch/src/claude_code.rs:288`, so a probe with it would measure another status area); the byte search of the OpenCode executable for PH13 (it matches HTTP and TUI code, see "Facts").

Work in the node's checkout and run each command from the repository root; paths are relative to it. Each commit message ends with the two lines `Zygo-Session: 01M80K6VKVFKHN736B0QCWTTKZ` and `Zygo-Work: PRB-1` (given with `--trailer`). In each commit, tick (`- [x]`) the steps of this file that are done. Change only these paths: `scripts/premises/hud_probe.py`, `scripts/premises/hud_probe_test.py`, `scripts/premises/live-hud-probe.sh`, `scripts/premises/live-hud-probe_test.sh`, `scripts/premises/ver_test.sh`, the eight `live-*.sh` of step 1.2, `fixtures/claude-code/hud/prb-1-*`, `docs/specs/2026-11-21-statusline-hud-design.md` (H§12 notes and one change-log row only), `docs/live-checks.md` (see the note in "Findings for the controller", F1) and this file. Do not edit `graph.md`, another node file, `lib.sh`, `live.sh`, `live-hud.sh` or code. Do not touch a `p-hud-1-*` fixture.

Tasks 1 to 4 start no agent. Task 5 and step 7.4 are the two live runs. No step needs a person at the keyboard.

Builds on: `ver_ge` (`scripts/premises/ver.sh`), `agent_ver`, `agent_ok`, `agent_show`, `pinned_agents` with `LIVE_CC_VER` and `LIVE_OC_VER`, `n2_scratch_end` (PRB-2); the driving code of `scripts/premises/live-hud.sh` (LIV-1), copied into PRB-1's own files; FIX-5 and FIX-7 run first (graph Depends).
Plan produces: `scripts/premises/hud_probe.py`, `scripts/premises/hud_probe_test.py`, `scripts/premises/live-hud-probe.sh`, `scripts/premises/live-hud-probe_test.sh`, one case in `scripts/premises/ver_test.sh`, eight `LIVE_VER` lines, the fixtures `fixtures/claude-code/hud/prb-1-*`, eleven rows and one section in `docs/live-checks.md`, twelve dated notes and one change-log row in H§12 of the HUD spec.

### Facts of the code and of this machine (read 2026-11-22)

Framework (the script uses these helpers and adds none to `lib.sh` or `live.sh`):

- `scripts/premises/lib.sh`: `set -eu` (`:7`); `PREMISES_DIR`, `REPO_DIR` (`:8-9`); `CLAUDE_BIN` and `OPENCODE_BIN`, each `exit 1` with `premises: <agent> is not on PATH` when absent (`:11-12`); with no `N2_SCRATCH`, lib.sh makes one (`mktemp -d`, so under `TMPDIR`), removes it after exit 0 and keeps it after an error (`:13-44`); `TOOL_PATH` (`:46`); `new_run <name>` sets `RUN`, `H`, `R` and makes `R` a git repository with one commit (`:49-67`); `scrub <file>` = `python3 -I scripts/premises/scrub.py "$RUN" <file>` (`:123-125`); `tmux_own <session>` (`:131-136`); `tmux_send <session> <text>` (`:148`).
- `scripts/premises/live.sh`: `ver_ge` from `ver.sh` (`:5`); `PIN_CC`, `PIN_OC` from `docs/premises.md` (`:8-9`); `LIVE_PIDS` (`:14`); `live_end <status>` kills `$LIVE_PIDS` and the sessions of `$LIVE_TMUX`, removes `$N2_SCRATCH/live-$$`, then `n2_scratch_end` (`:23-43`; FIX-7 changes it so that a caller-set `N2_SCRATCH` keeps `live-<pid>`: the plan never sets `N2_SCRATCH`, so both forms remove the tree); `live_traps` (`:47-54`); `pinned_agents` checks both agents at or above their minimum and sets `LIVE_CC_VER`, `LIVE_OC_VER`, `LIVE_VER` (`:78-89`); `live_run <name>` (`:99`); `result <id> <word> <text>` prints `LIVE <id> <word> (agent ${LIVE_VER}): <text>` and appends it to `$N2_SCRATCH/results.txt` (`:226-229`); `live_done` (`:232-234`).
- The `LIVE_VER` sites that still fall back to a pin: `live-claude.sh:40`, `live-deadline.sh:30`, `live-16b.sh:78`, `live-window.sh:74`, `live-passthrough.sh:180`, `live-rt.sh:47` (`:-$PIN_CC`), `live-opencode.sh:9` (`LIVE_VER=$PIN_OC`), `live-strict.sh:150` (`LIVE_VER=$PIN_CC`). Each of the eight scripts calls `pinned_agents` before that line (`live-claude.sh:9`, `live-deadline.sh:11`, `live-16b.sh:23`, `live-window.sh:16`, `live-passthrough.sh:151`, `live-rt.sh:14`, `live-opencode.sh:8`, `live-strict.sh:19`), so `LIVE_CC_VER` and `LIVE_OC_VER` are set there. `live-strict.sh:62` (`LIVE_PIN_CC=$PIN_CC`, the version a stub prints) stays.
- `scripts/check-premises.sh:125-128` scans `fixtures/` and `scripts/premises/live*.sh` (so also `live-hud-probe.sh` and `live-hud-probe_test.sh`) for forbidden content, the names of `N2_FORBIDDEN_NAMES` and the host names. `tests/forbidden_content.rs` (test target `forbidden_content`, `crates/zygo/Cargo.toml` `[[test]]`) scans every tracked and new file.

Claude Code as LIV-1 found it (`docs/work/phase2/nodes/LIV-1.md`, "Live run findings"; Claude Code 2.1.330, tmux 3.7c):

- D1: the folder trust question starts with the pointer on `No, exit`; `Enter` alone ends Claude Code. The answer: `Down` until the pane shows `❯` on `Yes, I trust this folder` in two readings 0.5 s apart, then `Enter` (`scripts/premises/live-hud.sh:467-497`). The answer is kept for the path, so a later session on the same path gets no question.
- D2: `/dev/tty` does not answer the status line command, in tmux and on a pseudo-terminal; Claude Code sets `COLUMNS` to the terminal width.
- D4: Claude Code draws the status line 2 cells from the left and cuts a row with `…` 2 cells before the right edge. In one pane it wrote its own notice at the right end of the first status row; so the probe measures on row 2 only.
- D5: the status line shows before the first prompt; the login works under `env -i`; `/exit` ends a session; a pseudo-terminal that answers no terminal query still gets the status line.
- D7: Claude Code keeps the trust answer and a project folder for the scratch path in the caller's Claude Code config folder.
- D8: a session with no prompt sent one auxiliary request (HTTP 429) and no other request.
- `fixtures/claude-code/hud/p-hud-1-dump-pty.json`: before any prompt the stdin already holds `effort` (`{"level": "medium"}`), so PH5 needs no model turn.
- `crates/zygo-launch/src/claude_code.rs:381-384`: `zygo claude` with a user `--settings` prints a warning and logs `degraded: --settings`. A probe status line through zygo is a degraded launch, and zygo's own `statusLine` (`:288`) is in the same settings layer.

Measured by the planner on 2026-11-22 (no agent, no model request; each tmux command on a private socket with `-f /dev/null`):

- tmux 3.7c, `#{w:@option}` of a session user option (`set-option -t "=<session>:"`; the form `-t "=<session>"` answered `no such session`): U+F0645 1, U+2702 1, U+1D607 1, U+3042 2, U+2702 U+FE0F 1.
- OpenCode 1.19.6 (`$HOME/.opencode/bin/opencode`), temporary `HOME` and `XDG_*`, `OPENCODE_CONFIG_CONTENT` with one key, `opencode debug config --pure`: exit 0, no stderr; `model` and `username` are kept in the printed JSON; `statusLine`, `statusline`, `status_line`, `zygoProbeUnknownKey` and `tui` are dropped. `opencode --help` and `opencode debug --help` hold no `status line` form.
- The OpenCode executable holds `statusLine` 5 times (HTTP response code: `let statusLine = \`HTTP/1.1 ${this.statusCode}…`) and `statusline` 11 times (a TUI layout object `statusline:{showActivityMeta,…}`), so a byte search cannot decide PH13.
- The four new files below ran in a copied tree with stub `claude` and `opencode` programs: `hud_probe tests: ok (14)`, `live-hud-probe tests: ok (12)`; with the eight `LIVE_VER` edits `ver tests: ok (51)`, `lib tests: ok (27)`, `live selftest: ok (54)`, `check-premises: ok (P1 P2 P3 P4 P5 P6a P6b Q1 Q2 Q3 Q4 Q5 Q6 Q7 PW6 PW11 PW15)` (with and without `N2_FORBIDDEN_NAMES`). Not run there: the real Claude Code, the real OpenCode `debug config` inside the script, `cargo test`.

### Findings for the controller

- **F1 (contract Files lack `docs/live-checks.md`).** The plan rules ask for dated rows in `docs/live-checks.md` (as LIV-1); the contract Files do not name it. Steps 6.1 and 6.2 edit it. If the controller does not add the path, skip 6.1 and 6.2 and leave `docs/live-checks.md` out of steps 6.7 and 7.5.
- **F2 (stale contract text).** The Spec line says "The executor stops and asks the operator to run this node on their machine" and "PH4 … so HUD-8 is planned with it"; the operator decided that the executor runs live checks from its session (graph Log 2026-11-21), and HUD-8 is DONE. The Acceptance mapping names DONE nodes (HUD-8, HUD-9, HUD-10, HUD-11, HUD-12, H2, LIV-1) that cannot be re-planned "before that node starts". The plan records a failed line and names the nodes; the controller applies execute-work §5.4 (a new node for a DONE node, as the LIV-1 contract says) and re-plans LIV-2 (TODO).
- **F3 (Files that the plan does not change).** `scripts/premises/live.sh` needs no change (PRB-2 did `pinned_agents`). `ver_test.sh` gets one static case (L1) for the `LIVE_VER` sites.
- **F4 (Spec "Adds PH3, PH6, PH7 and PH13 to the H§12 list").** Done as a dated note under the LIV-1 note of H§12 (step 6.4), not by an edit of the "Probe first" paragraph or of H§17 note 4 (controller: spec edits are dated notes in H§12 only).
- **F5 (PH1, PH2, PH3, PH4, PH5, PH9, PH12 again).** LIV-1 checked them on the built HUD. The contract still asks a line for each; the probe checks them on the probe bytes in the same session, with no extra model request.
- **F6 (PH4 expected `failed`).** From D2 and D4: `/dev/tty` errno in both sessions, `COLUMNS` 200 and 100, status area = pane width - 4. FIX-5 already builds that fallback.

### Design

One shell script, `scripts/premises/live-hud-probe.sh` (Block C), and one Python helper, `scripts/premises/hud_probe.py` (Block A). The driving code is LIV-1's: the private tmux server (`tmux -u -L zygo-prb-<pid> -f /dev/null`), `env -i` with the real `HOME`, the caller's `CLAUDE_CONFIG_DIR`, `TOOL_PATH`, `TERM=xterm-256color`, `LANG=C.UTF-8`, `DISABLE_AUTOUPDATER=1`; the trust answer, the start wait, `/exit`, the trap that ends the private server, the pseudo-terminal driver. Copied functions: `tmux` wrapper, `hud_end` → `prb_end`, `trust_line`, `hud_trust` → `prb_trust`, `hud_wait` → `prb_wait`, `hud_stop` → `prb_stop`, `wait_until` (`live-hud.sh:393-407`, `:462-507`); `attrs`, `cmd_sgr` → `an_sgr`, `cmd_two` → `an_two`, `block`, `hud_pty.py` → `cmd_pty` (`live-hud.sh:31-124`, `:231-296`).

Author's calls:

- Claude Code runs directly, as `claude --settings <run>/settings.json` with the operator's stored login (the same login and environment as LIV-1's passthrough sessions), not through zygo: the probe checks Claude Code's status line, and zygo degrades a launch with a user `--settings` (Facts). The settings file holds only `{"statusLine": {"type": "command", "command": "'<python3>' -I '<hud_probe.py>' statusline '<dir>'", "refreshInterval": 2}}`. A `--settings` value comes before the user settings, so an operator `statusLine` is not used; LIV-1's zygo HUD came through the same layer.
- No prompt is typed. Each run starts Claude Code two times: session `t` in tmux (200 x 50) and session `p` on a pseudo-terminal (100 x 50, outside tmux, for PH4), both in the run's git repository `$R`. Session `p` starts after `t`, so the trust answer of `t` is kept for the path (D1).
- The probe command reads the phase from `<dir>/phase` at its start; the script writes it with a temporary file and `mv`. The phases of session `t`, in this order:

  | Phase | Command output (refreshInterval 2 s) | Script action | For |
  |---|---|---|---|
  | `verbose` | the H§6.3 verbose `printf` bytes (2 rows) | wait 60 s at most for the logo (trust answer on the way), 3 s, `capture-pane -e` | PH1, PH2, PH9 |
  | `compact` | the H§6.3 compact `printf` bytes (1 row, ends `✂ 23k`) | wait 10 s at most for `✂`, 1 s, `capture-pane -e` | PH7 (no U+FE0F) |
  | `wideA` … `wideD` | row 1 `P|wide<X> probe`; row 2 `<X>|` + glyphs + RULER: A none, B 10 × U+F0645, C 10 × U+2702, D 5 × `𝘇𝘆𝗴𝗼` (20 letters) | wait 10 s at most for row 1, 1 s, `capture-pane` | PH3, PH4 (c), PH6, PH7, PH9 |
  | `early` | closes stdin unread at once, prints `PH11 early <pid>` | wait 10 s at most, captures every 0.5 s for 8 s | PH11 |
  | `slow` | reads stdin, beats every 0.5 s for 5 s, then `PH12 slow <pid>` | captures every 0.5 s for 20 s | PH12 |
  | `done` | `PROBE DONE` | wait 10 s at most, one capture, then `/exit` | PH11 (went on) |

  RULER = 30 blocks, block k = dots then the number 10k in 10 cells (300 ASCII cells; the number 10k ends at ruler cell 10k). Claude Code cuts row 2 at its own count of the area, so R_A − R_X = (cells Claude Code counts for the glyphs of row X) − 0. With each glyph counted 1 cell, R_A − R_B = 10, R_A − R_C = 10, R_A − R_D = 20.
- Each probe run writes `log/run-<ns>-<pid>-<phase>.json` (phase; `tty`: `/dev/tty` opened `O_RDONLY|O_NOCTTY`, `TIOCGWINSZ` → cols and rows, else the errno name; `env`: the values of `COLUMNS LINES TERM TERM_PROGRAM TERM_PROGRAM_VERSION COLORTERM NO_COLOR FORCE_COLOR WT_SESSION LANG LC_ALL LC_CTYPE CLAUDECODE CLAUDE_CODE_ENTRYPOINT`, `TMUX`/`TMUX_PANE` as `set`; `env_names`: every name; `stdin`: the parsed JSON, read for at most 2 s and 1 MiB, or `not read`) and events `start`, `beat`, `signal` (TERM, INT, HUP, PIPE; exit 128 + n), `stdout-error`, `end` in `events.jsonl`. These files stay in the scratch tree; a fixture keeps only the stdin keys and `effort`, the kept values and the names.
- PH13 is decided by OpenCode itself: the help texts, and `opencode debug config --pure` with each candidate key in `OPENCODE_CONFIG_CONTENT`, with two controls (a known key is kept, an unknown key is dropped). A temporary `HOME` under the scratch tree; 30 s bound for each call.
- Each `LIVE` text holds only values that do not change between two runs of the same Claude Code (cells, columns, classes, signal names), so step 7.4 can compare the results file byte for byte. Times, run counts and process lifetimes go into `prb-1-events.json` and `prb-1-analysis.json`.
- Bounds: 60 s for the start of each session; 10 s for each phase change; the capture times of the phase table; 60 s for session `p`; 10 s for each session end; 30 s for each OpenCode call; 2 s and 1 MiB for each stdin read. A bound that passes prints a note on stderr and the ids of that part print `failed` with the reason; the script goes on.
- Traps (EXIT, INT, TERM, HUP, PIPE): `prb_end` kills the private tmux server (and so Claude Code in session `t` and the width session `w`), removes its socket file, then `live_end` kills `LIVE_PIDS` (the pseudo-terminal driver, whose TERM handler sends `SIGHUP` then `SIGKILL` to session `p`'s process group) and removes the scratch tree.
- Fixtures: `fixtures/claude-code/hud/prb-1-*`, each through `scrub`, written only when the source has content; the script removes the old `prb-1-*` files first and never touches `p-hud-1-*`.

Each premise (H§12; the contract's list):

| Id | Probe | Pass condition (shell test in Block C) | `LIVE` word and text |
|---|---|---|---|
| PH1 | `an_sgr` of the `verbose` capture with escapes | `[ "$(j .sgr.ok)" = true ]`: stripe fg 1, 3, 6; first logo cell bold fg 15; `hud-work` fg 172 (`38;5;172`) | `verified`: `capture-pane -e (tmux <v>), the H§6.3 verbose printf bytes: stripe fg 1,3,6, logo bold fg 15, worktree name fg 172 (38;5;n)`; `failed`: `16-color part <b>, 256-color part <b>: <json>` |
| PH2 | `an_two` of the same capture | `[ "$(j .two.rows)" = 2 ]` | `verified`: `the two rows of the H§6.3 verbose printf show as two rows (tmux <v>, 200 columns)` |
| PH3 | `an_wide A` of the `wideA` capture | `[ "$C3" = cut ]` (ruler shorter than 300 and the next line is not the rest of the ruler) | `verified`: `cut: a row of 302 cells in a 200-column pane shows <n> cells (ruler <r>, ends in …: <b>); …`; `failed`: `wrapped: …` or `not observed: …` |
| PH4 | first run log of `t` and of `p`; `an_wide A` indent and right gap | `[ "$TT" = 200 ] && [ "$TP" = 100 ] && dig "$CT" && dig "$CP" && [ "$LOSS" = 0 ]` | `(a) /dev/tty in the command: tmux 200 columns: <cols or errno>, pseudo-terminal 100 columns: <…>; (b) COLUMNS: tmux <v>, pseudo-terminal <v>; (c) status area = pane width - <loss> at 200 columns (indent <i>, right gap <g>)` |
| PH5 | every run log with parsed stdin, both sessions (`stdin.json`) | `jq -e 'length > 0 and all(.[]; (.effort \| type) as $t \| $t == "string" or ($t == "object" and (.effort.level \| type) == "string"))'` | `stdin effort is <json> (<type>) in every logged run (tmux and pseudo-terminal); stdin keys of the first run: <keys>` |
| PH6 | `wideA` vs `wideB` | `[ "$(j .wide.B.diff)" = 10 ]` | `Claude Code counts U+F0645 as <c> cell(s): the row with 10 glyphs shows <d> fewer ruler cells than the ASCII row (<rA> and <rB>)` |
| PH7 | `wideA` vs `wideC`; tmux width; `compact` capture | `[ "$(j .wide.C.diff)" = 10 ] && [ "$(j .tw.C)" = 1 ] && [ "$(j .compact.scissors)" = true ] && [ "$(j .compact.vs16)" = false ]` | the PH6 form for U+2702, `; tmux <v> width <w>; U+FE0F after it in the compact row: <b>; the terminal font's presentation is not checked` |
| PH9 | `wideA` vs `wideD`; tmux width; `an_two` columns | `[ "$(j .wide.D.diff)" = 20 ] && [ "$(j .tw.D)" = 1 ] && [ "$(j .two.rows)" = 2 ] && [ "$(j .two.logo_col)" -ge 0 ] && [ "$(j .two.logo_col)" = "$(j .two.refl_col)" ]` | the PH6 form for each logo letter (20 letters), tmux width, `logo and reflection start at column <a> and <b>; the font's glyphs are not checked` |
| PH11 | `early` events and captures; the `done` capture | `[ "$(j .ph11.shown)" -ge 3 ] && [ "$(j .ph11.lost)" = 0 ] && [ "$(j .ph11.error)" = false ] && [ "$(j .ph11.after)" = true ]` (lost: an ended early run whose line no capture showed, ended 2.5 s or more before the last capture; error: `EPIPE` or a status line error text in a capture; after: `PROBE DONE` shows later) | `verified`: `a command that closes stdin unread and exits at once: each early run's line showed (3 or more runs), none lost, no error text, and the status line went on after the phase` |
| PH12 | `slow` events and captures | observation: `[ -n "$T12" ] && [ "$(j .ph12.starts)" -ge 2 ]` → `observed`; else `failed` `not observed: …` | `refreshInterval 2 s, a command that sleeps 5 s: <kills: Claude Code ends a running command when the next refresh comes \| waits \| overlaps>; signals seen: <names or none>; at most <n> command(s) at a time; slow line shown: <b>` |
| PH13 | `hud_probe.py ph13` | `[ "$(o '.help_hits \| length')" = 0 ] && [ "$(o .control.username)" = kept ] && [ "$(o .control.zygoProbeUnknownKey)" = dropped ] && [ "$(o '[.keys[] \| select(. != "dropped")] \| length')" = 0 ]` | `OpenCode <v>: no status line in --help or debug --help; debug config --pure drops the inline keys: statusLine dropped, statusline dropped, status_line dropped; control: username kept, zygoProbeUnknownKey dropped` (agent field: the OpenCode version) |

When session `t` does not start (`prb_wait` fails), every id of `t` prints `failed` with the reason (`timeout: …` or `claude ended: …`); PH4 and PH5 use what exists. A `failed` text that starts with `timeout:`, `claude ended:` or `not observed:` is a failure of the run, not an observation of the premise; record it the same way and say so in the report.

Model requests: each run starts Claude Code two times with no prompt: 0 main-loop requests; from D8 about one auxiliary start request (HTTP 429) for each start, which the script does not control (the operator's plugins and hooks load as in LIV-1). OpenCode sends none. Two runs (step 5.1 and the Acceptance, step 7.4): 0 main-loop requests and about 4 auxiliary requests in all.

A failed line (the contract's mapping). Write the row and the note with `failed` and the line text, and name the nodes in the report; change no other node file, no code and not `graph.md`:

| Id | Nodes that the contract names |
|---|---|
| PH1 | HUD-8, HUD-9, HUD-10, H2, LIV-1, LIV-2 |
| PH2 | HUD-8, HUD-11, LIV-1, H2 |
| PH3 | HUD-8 |
| PH9 | HUD-8, LIV-2 |
| PH4, PH5, PH11 | HUD-8, HUD-9 |
| PH7 | HUD-8, HUD-12 |
| PH6, PH12, PH13 | none |

### Task 1: the `LIVE_VER` sites (no agent)

- [x] **1.1** Record the baselines. Run `sh scripts/premises/ver_test.sh`, `sh scripts/premises/lib_test.sh`, `PATH="$HOME/.opencode/bin:$PATH" sh scripts/premises/live-selftest.sh` and `sh scripts/check-premises.sh`. Expected: last lines `ver tests: ok (50)`, `lib tests: ok (<L>)`, `live selftest: ok (<S>)`, `check-premises: ok (<IDS>)`, each exit 0. Write `<L>`, `<S>` and `<IDS>` into the commit message of step 1.4 (later steps compare with them). When one fails: stop and report its output (the tree is not at a known state).
- [x] **1.2** Add the case of Block E to `scripts/premises/ver_test.sh`, directly above the line `if [ "$bad" -eq 0 ]; then echo "ver tests: ok ($n)"; exit 0; fi`. Run `sh scripts/premises/ver_test.sh; echo "rc=$?"`. Expected: `FAIL: L1 no LIVE_VER site falls back to PIN_CC or PIN_OC (in: live-16b.sh live-claude.sh live-deadline.sh live-opencode.sh live-passthrough.sh live-rt.sh live-strict.sh live-window.sh)`, `ver tests: 1 failed`, `rc=1`.
- [x] **1.3** Make each LIVE line name the version that `pinned_agents` found. Replace exactly one line in each file:

  | File | Old line | New line |
  |---|---|---|
  | `scripts/premises/live-claude.sh` | `set_ver() { v=$(usage_version); LIVE_VER=${v:-$PIN_CC}; }` | `set_ver() { v=$(usage_version); LIVE_VER=${v:-$LIVE_CC_VER}; }` |
  | `scripts/premises/live-deadline.sh` | `  LIVE_VER=$(usage_version); LIVE_VER=${LIVE_VER:-$PIN_CC}` | `  LIVE_VER=$(usage_version); LIVE_VER=${LIVE_VER:-$LIVE_CC_VER}` |
  | `scripts/premises/live-16b.sh` | `LIVE_VER=$(usage_version); LIVE_VER=${LIVE_VER:-$PIN_CC}` | `LIVE_VER=$(usage_version); LIVE_VER=${LIVE_VER:-$LIVE_CC_VER}` |
  | `scripts/premises/live-window.sh` | `LIVE_VER=$(usage_version); LIVE_VER=${LIVE_VER:-$PIN_CC}` | `LIVE_VER=$(usage_version); LIVE_VER=${LIVE_VER:-$LIVE_CC_VER}` |
  | `scripts/premises/live-passthrough.sh` | `LIVE_VER=${LIVE_VER:-$PIN_CC}` | `LIVE_VER=${LIVE_VER:-$LIVE_CC_VER}` |
  | `scripts/premises/live-rt.sh` | `LIVE_VER=${LIVE_VER:-$PIN_CC}` | `LIVE_VER=${LIVE_VER:-$LIVE_CC_VER}` |
  | `scripts/premises/live-opencode.sh` | `LIVE_VER=$PIN_OC` | `LIVE_VER=$LIVE_OC_VER` |
  | `scripts/premises/live-strict.sh` | `LIVE_VER=$PIN_CC` | `LIVE_VER=$LIVE_CC_VER` |

  Check: `for f in scripts/premises/live*.sh; do sh -n "$f" || echo "syntax: $f"; done` prints nothing; `sh scripts/premises/ver_test.sh` → last line `ver tests: ok (51)`, exit 0; `PATH="$HOME/.opencode/bin:$PATH" sh scripts/premises/live-selftest.sh` → `live selftest: ok (<S>)`, exit 0 (it copies the changed scripts into its test tree). `lib.sh`, `live.sh` and `ver.sh` do not change, so `lib_test.sh` is not run here.
- [x] **1.4** Commit `scripts/premises/ver_test.sh`, the eight scripts of step 1.3 and this file: `git commit -m "fix(premises): LIVE lines name the agent version found, not the pin" -m "Baselines: lib <L>, selftest <S>, check-premises <IDS>." --trailer "Zygo-Session: 01M80K6VKVFKHN736B0QCWTTKZ" --trailer "Zygo-Work: PRB-1"`. Check: `git log -1 --format=%B` ends with those two lines.

### Task 2: the probe helper (no agent)

- [x] **2.1** Create `scripts/premises/hud_probe_test.py` with exactly the text of Block B. Run `python3 -I scripts/premises/hud_probe_test.py; echo "rc=$?"`. Expected: a `FileNotFoundError` traceback for `hud_probe.py`, `rc=1`.
- [x] **2.2** Create `scripts/premises/hud_probe.py` with exactly the text of Block A. Run `python3 -I scripts/premises/hud_probe_test.py`. Expected: 14 lines that start with `ok: H`, then `hud_probe tests: ok (14)`, exit 0.
- [x] **2.3** Commit the two files and this file: `test(premises): hud_probe.py, the PRB-1 probe status line command and analyzer` (with the two trailers). Check: `git log -1 --format=%B` ends with the two trailer lines.

### Task 3: the live script (no agent)

- [x] **3.1** Create `scripts/premises/live-hud-probe_test.sh` with exactly the text of Block D. Run `sh scripts/premises/live-hud-probe_test.sh; echo "rc=$?"`. Expected: `FAIL: L1 exit 0 and eleven LIVE lines …` and other `FAIL:` lines, `live-hud-probe tests: <n> failed`, `rc=1` (no `live-hud-probe.sh`; the L8 case waits 20 s).
- [x] **3.2** Create `scripts/premises/live-hud-probe.sh` with exactly the text of Block C. Run `sh -n scripts/premises/live-hud-probe.sh` (no output, exit 0), then `sh scripts/premises/live-hud-probe_test.sh` (about 3 minutes; stubs only, a private `TMUX_TMPDIR` under its temporary dir). Expected: 12 lines that start with `ok: L`, then `live-hud-probe tests: ok (12)`, exit 0.
- [x] **3.3** Run `sh scripts/premises/live-hud-probe.sh a; echo "rc=$?"`. Expected: `usage: sh scripts/premises/live-hud-probe.sh`, `rc=1`, and no `premises: scratch` line (the usage check comes before `lib.sh`).
- [x] **3.4** Check that the script writes no settings file of the user: `grep -nE '>>? *"?\$\{?(HOME|PRB_CC_DIR|CLAUDE_CONFIG_DIR)' scripts/premises/live-hud-probe.sh; echo "rc=$?"` → `rc=1`; `grep -n 'settings.json' scripts/premises/live-hud-probe.sh` → only lines with `$T/settings.json` or `$Q/settings.json`.
- [x] **3.5** Run `sh scripts/check-premises.sh` and `N2_FORBIDDEN_NAMES="$(id -un)" sh scripts/check-premises.sh`. Expected: both end with `check-premises: ok (<IDS>)`, exit 0 (the two new `live*.sh` are in its scan scope).
- [x] **3.6** Commit the two files and this file: `test(premises): live-hud-probe.sh, the PRB-1 live probe of the status line` (with the two trailers). Check: `git log -1 --format=%B` ends with the two trailer lines; `git status --short` prints nothing.

### Task 4: the gate before the live run (no agent)

- [x] **4.1** Run `"$HOME/.opencode/bin/opencode" --version`, `claude --version`, `tmux -V` and `command -v jq python3`. Expected: OpenCode 1.18.16 or later (1.19.6 on 2026-11-22); Claude Code 2.1.328 or later (2.1.330); tmux 3.2 or later (3.7c); two paths. A pin is a minimum. When a program is absent or below its minimum: stop and report the output; do not add a stub and do not edit `lib.sh`.
- [x] **4.2** Run `ls "${TMUX_TMPDIR:-/tmp}/tmux-$(id -u)"`. Expected: no name that starts with `zygo-prb-` (a name left by an earlier run would be reused only by a process with the same id; report it if present, do not delete the operator's other sockets).

### Task 5: the live run (two Claude Code starts, no prompt)

- [x] **5.1** Run `PATH="$HOME/.opencode/bin:$PATH" sh scripts/premises/live-hud-probe.sh; echo "rc=$?"` (do not set `N2_SCRATCH`). It takes 2 to 5 minutes and needs no input. Expected: `premises: scratch <dir> …` on stderr; eleven lines `LIVE PH1 …`, `LIVE PH2 …`, `LIVE PH3 …`, `LIVE PH4 …`, `LIVE PH5 …`, `LIVE PH6 …`, `LIVE PH7 …`, `LIVE PH9 …`, `LIVE PH11 …`, `LIVE PH12 …`, `LIVE PH13 …`, each with `verified`, `failed` or `observed`; `live: fixtures in fixtures/claude-code/hud; scratch removed on exit`; `rc=0`. When the script exits 1 with a `live: …` or `premises: …` line, a precondition failed: report the line and stop. When six or more lines say `timeout:` or `claude ended:`, report the lines and stop before Task 6 (session `t` did not start; the rows would hold no observation).
- [x] **5.2** Run `pgrep -fl "tmux .*-L zygo-prb-"` and `ls "${TMUX_TMPDIR:-/tmp}/tmux-$(id -u)"`. Expected: `pgrep` prints nothing (a line that names only the shell of this command line counts as nothing); no `zygo-prb-` name in the listing.
- [x] **5.3** Run `ls fixtures/claude-code/hud`. Expected: the fifteen `p-hud-1-*` files of LIV-1, unchanged (`git status --short fixtures/claude-code/hud` names no `p-hud-1-` file), and `prb-1-analysis.json prb-1-compact.txt prb-1-early.txt prb-1-env-pty.json prb-1-env-tmux.json prb-1-events.json prb-1-ph13.json prb-1-results.txt prb-1-run.json prb-1-stdin-pty.json prb-1-stdin-tmux.json prb-1-verbose.txt prb-1-wide.txt`, and `prb-1-slow.txt` only when a slow line showed. A missing file is allowed only for a session that did not start; name it in the report.
- [x] **5.4** Search the new fixtures: `grep -rn -i -F -e "$HOME" -e "$(id -un)" -e "$(hostname)" -e "${CLAUDE_CONFIG_DIR:-$HOME/.claude}" fixtures/claude-code/hud/prb-1-*` → no output, exit 1. Then `grep -rn -E -e '[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}' -e 'sk-' -e 'eyJ[A-Za-z0-9_-]{5,}' -e '@' -e 'Bearer' -e 'sessions/' fixtures/claude-code/hud/prb-1-*` → no output, exit 1, or only lines with the placeholder `00000000-0000-4000-8000-…`. When a line matches: do not edit the fixture by hand; report the file and the line number (not the matched text) and stop.
- [x] **5.5** Read every `prb-1-*` fixture once. Expected: status rows, the probe rows, numbers, the stdin key names and `effort`, the kept environment values of the allowlist and the other environment names (names only), the OpenCode key classes; no prompt text, no account name, no organization name, no e-mail address. When a file holds one: report the file and stop.

### Task 6: the records

The values: `<DATE>`, `<CC>`, `<OC>` and `<TMUX>` are `.date`, `.claude_code`, `.opencode` and `.tmux` of `fixtures/claude-code/hud/prb-1-run.json`. For each id, `<RESULT-id>` is the word after the id in its line of `fixtures/claude-code/hud/prb-1-results.txt`, and `<TEXT-id>` is the text after `): ` in that line. In a table cell write `\|` for each `|` of a text.

- [x] **6.1** In `docs/live-checks.md` (F1): line 3, replace `H§12 (P-HUD-1).` with `H§12 (P-HUD-1, and the PRB-1 probe).`; bullet "Isolation", replace `every run except #28 and the passthrough parts of P-HUD-1 gets a temporary` with `every run except #28, the passthrough parts of P-HUD-1 and the Claude Code sessions of PRB-1 gets a temporary`; bullet "Upstream per row", replace `#28 and P-HUD-1 (all parts but C7, which uses the mock) use the claude.ai login` with `#28, P-HUD-1 (all parts but C7, which uses the mock) and PRB-1 use the claude.ai login`. Then add this section directly above the line `## Results` (the text between the two lines of four backticks):

````markdown
## PRB-1 (status line probe)

Node PRB-1; `docs/specs/2026-11-21-statusline-hud-design.md` H§12 ("Probe first"). From the checkout, on Linux or macOS, with tmux 3.2 or later, `python3` and `jq`, Claude Code logged in, and `claude` and `opencode` on `PATH`:

```sh
sh scripts/premises/live-hud-probe.sh
```

- No agent and no login: `python3 -I scripts/premises/hud_probe_test.py` tests the probe command and its analyzer; `sh scripts/premises/live-hud-probe_test.sh` runs the whole script with stub `claude` and `opencode` programs.
- It prints one `LIVE <id> verified|failed|observed (agent <version>): …` line for each of PH1, PH2, PH3, PH4, PH5, PH6, PH7, PH9, PH11, PH12 and PH13, writes `fixtures/claude-code/hud/prb-1-*` and exits 0. A failed line does not change the exit status.
- Claude Code runs as `claude --settings <run file>`, not through zygo, two times on the stored login: in a private tmux server (`tmux -L zygo-prb-<pid>`, no tmux config file, 200 x 50) and on a pseudo-terminal of 100 columns. The settings file holds only the probe `statusLine` command (`scripts/premises/hud_probe.py`, `refreshInterval` 2), whose output changes by phase: the two H§6.3 `printf` outputs, four ruler rows, an early exit, a slow run. They use the real `HOME` and the caller's `CLAUDE_CONFIG_DIR`; the cwd is a git repository in the scratch folder. The script answers the folder trust question of Claude Code with yes; Claude Code keeps the answer and a project folder for that path in its config folder, and the script does not delete them.
- Spend: no prompt is typed; Claude Code sends only its own start requests (one auxiliary request each start on 2.1.330).
- PH13 runs `opencode --help`, `opencode debug --help` and `opencode debug config --pure` with a temporary `HOME`; no model request.
- Bounds: 60 s for each session start, 10 s for each phase change, 8 s and 20 s of captures for the early and slow phases, 60 s for the pseudo-terminal session, 30 s for each OpenCode call, 10 s for each session end. A bound that passes gives `failed` with the reason for the ids of that part; the script goes on.
- The traps end the private tmux server, the pseudo-terminal session and the background processes, and remove the scratch tree.
````

  Check: `grep -c '^## PRB-1 (status line probe)$' docs/live-checks.md` → `1`.
- [x] **6.2** In `docs/live-checks.md` (F1), add these eleven rows at the end of the table of `## Results`, in this order, with the values in place. In each row leave out a fixture path that step 5.3 did not list.

```markdown
| PRB-1/PH1 | The status line shows SGR colors, 16-color and `38;5;n` (HUD design H§12 PH1) | Claude Code | <CC> | <DATE> | <RESULT-PH1> | tmux <TMUX>, Linux; `scripts/premises/live-hud-probe.sh`, probe status line command, `claude --settings` on the stored login. <TEXT-PH1>. `fixtures/claude-code/hud/prb-1-verbose.txt`, `fixtures/claude-code/hud/prb-1-analysis.json`; H§12 |
| PRB-1/PH2 | A two-row status line shows as two rows (H§12 PH2) | Claude Code | <CC> | <DATE> | <RESULT-PH2> | tmux <TMUX>, Linux; probe. <TEXT-PH2>. `fixtures/claude-code/hud/prb-1-verbose.txt`; H§12 |
| PRB-1/PH3 | A row wider than the status area is cut, not wrapped (H§12 PH3) | Claude Code | <CC> | <DATE> | <RESULT-PH3> | tmux <TMUX>, Linux; probe. <TEXT-PH3>. `fixtures/claude-code/hud/prb-1-wide.txt`; H§12 |
| PRB-1/PH4 | The width sources reach the status line command, and the status area is the full terminal width (H§12 PH4) | Claude Code | <CC> | <DATE> | <RESULT-PH4> | tmux <TMUX>, Linux; probe, in tmux and on a pseudo-terminal. <TEXT-PH4>. `fixtures/claude-code/hud/prb-1-env-tmux.json`, `fixtures/claude-code/hud/prb-1-env-pty.json`, `fixtures/claude-code/hud/prb-1-wide.txt`, `fixtures/claude-code/hud/prb-1-analysis.json`; H§12 |
| PRB-1/PH5 | The stdin `effort` is a string, or an object with a string level (H§12 PH5) | Claude Code | <CC> | <DATE> | <RESULT-PH5> | tmux <TMUX>, Linux; probe. <TEXT-PH5>. `fixtures/claude-code/hud/prb-1-stdin-tmux.json`, `fixtures/claude-code/hud/prb-1-stdin-pty.json`; H§12 |
| PRB-1/PH6 | Claude Code counts a Nerd private-use glyph as one cell (H§12 PH6) | Claude Code | <CC> | <DATE> | <RESULT-PH6> | tmux <TMUX>, Linux; probe. <TEXT-PH6>. `fixtures/claude-code/hud/prb-1-wide.txt`, `fixtures/claude-code/hud/prb-1-analysis.json`; H§12 |
| PRB-1/PH7 | `✂` U+2702 shows in text presentation, one cell (H§12 PH7) | Claude Code | <CC> | <DATE> | <RESULT-PH7> | tmux <TMUX>, Linux; probe. <TEXT-PH7>. `fixtures/claude-code/hud/prb-1-wide.txt`, `fixtures/claude-code/hud/prb-1-compact.txt`, `fixtures/claude-code/hud/prb-1-analysis.json`; H§12 |
| PRB-1/PH9 | Each logo letter is one cell; the reflection starts at the column of the logo (H§12 PH9) | Claude Code | <CC> | <DATE> | <RESULT-PH9> | tmux <TMUX>, Linux; probe. <TEXT-PH9>. `fixtures/claude-code/hud/prb-1-wide.txt`, `fixtures/claude-code/hud/prb-1-verbose.txt`, `fixtures/claude-code/hud/prb-1-analysis.json`; H§12 |
| PRB-1/PH11 | A status line command that exits before it reads stdin causes no error and no lost line (H§12 PH11) | Claude Code | <CC> | <DATE> | <RESULT-PH11> | tmux <TMUX>, Linux; probe, `refreshInterval` 2 s. <TEXT-PH11>. `fixtures/claude-code/hud/prb-1-early.txt`, `fixtures/claude-code/hud/prb-1-events.json`; H§12 |
| PRB-1/PH12 | Observation: a status line run that still goes at the next refresh (H§12 PH12) | Claude Code | <CC> | <DATE> | <RESULT-PH12> | tmux <TMUX>, Linux; probe, `refreshInterval` 2 s, a command that sleeps 5 s. <TEXT-PH12>. `fixtures/claude-code/hud/prb-1-events.json`, `fixtures/claude-code/hud/prb-1-analysis.json`, `fixtures/claude-code/hud/prb-1-slow.txt`; H§12 |
| PRB-1/PH13 | OpenCode has no command status line that zygo could set (H§12 PH13) | OpenCode | <OC> | <DATE> | <RESULT-PH13> | Linux; `opencode --help`, `opencode debug --help`, `opencode debug config --pure` with a temporary `HOME`. <TEXT-PH13>. `fixtures/claude-code/hud/prb-1-ph13.json`; H§12 |
```

  Check: `grep -c '^| PRB-1/PH' docs/live-checks.md` → `11`.
- [x] **6.3** In `docs/specs/2026-11-21-statusline-hud-design.md`, H§12 table: for each of the rows PH1, PH2, PH3, PH4, PH5, PH6, PH7, PH9, PH11, PH12, PH13, append this at the end of the third cell ("Evidence so far"), one space before it, directly before the ` | ` that opens the Fallback cell (the cell may hold a LIV-1 note; keep it; FIX-5 may have changed the Fallback cell of PH4; do not change it):

```markdown
*(Note <DATE>, PRB-1: <RESULT-id> on Claude Code <CC>, tmux <TMUX>, Linux, with the probe status line command of `scripts/premises/live-hud-probe.sh` (not the zygo HUD). <TEXT-id>. `docs/live-checks.md` row PRB-1/<id>.)*
```

  For PH13 write `on OpenCode <OC>, Linux,` in place of `on Claude Code <CC>, tmux <TMUX>, Linux,`. For an id whose result is `failed`, put this sentence before the closing `)*`: ` The fallback of this row is not built by PRB-1; the PRB-1 contract names: <the nodes of that id in the table "A failed line">.` Check: `grep -c 'PRB-1: \(verified\|failed\|observed\) on ' docs/specs/2026-11-21-statusline-hud-design.md` → `11`; `grep -c '^| PH[0-9]* |' docs/specs/2026-11-21-statusline-hud-design.md` → `13` (no row was split).
- [x] **6.4** In the same file, add this paragraph directly below the paragraph that starts `*(Note 2026-11-22, LIV-1: P-HUD-1 ran with` (one empty line between them):

```markdown
*(Note <DATE>, PRB-1: the probe of "Probe first" ran after the build, with `scripts/premises/live-hud-probe.sh` on Claude Code <CC>, tmux <TMUX>, Linux, and OpenCode <OC>. It ran Claude Code directly with a `--settings` file that holds only the probe `statusLine` command (refreshInterval 2), with no prompt, in tmux at 200 columns and on a pseudo-terminal of 100 columns; the command printed the two OP13 `printf` outputs, then ruler rows that measure Claude Code's cell count of U+F0645, U+2702 and the logo letters, then an early exit and a slow run. Beside the ids of this paragraph it also checked PH3, PH6, PH7 and PH13 (PRB-1 contract). PH7 and PH9 compare cells in Claude Code's cut and in tmux; the glyphs and the presentation of a terminal font are not checked. Results: a dated PRB-1 note in each row; `docs/live-checks.md` rows PRB-1/PH1 to PRB-1/PH13.)*
```

  Check: `grep -c '^\*(Note .*, PRB-1: the probe of "Probe first"' docs/specs/2026-11-21-statusline-hud-design.md` → `1`.
- [x] **6.5** In the same file, add this row at the end of the table of `## Change log`:

```markdown
| <DATE> | H§12 (rows PH1, PH2, PH3, PH4, PH5, PH6, PH7, PH9, PH11, PH12, PH13; note below the LIV-1 note) | PRB-1 (live probe) | The probe ran on Claude Code <CC>, tmux <TMUX>, and OpenCode <OC>; each result is a dated note. |
```

  Check: `grep -c '| PRB-1 (live probe) |' docs/specs/2026-11-21-statusline-hud-design.md` → `1`; `git diff --stat -- docs/specs/2026-11-21-statusline-hud-design.md` shows one file; `git diff -- docs/specs/2026-11-21-statusline-hud-design.md` changes only lines of H§12 and the change log.
- [x] **6.6** Run `cargo test -p zygo --test forbidden_content`. Expected: `test result: ok.` with `0 failed`. When it names a line of a new or changed file: report the file and the line number and stop; do not edit the test.
- [x] **6.7** Commit `fixtures/claude-code/hud/prb-1-*`, `docs/live-checks.md`, `docs/specs/2026-11-21-statusline-hud-design.md` and this file: `docs(live): PRB-1 probe results on Claude Code <CC>, tmux <TMUX>, OpenCode <OC>; fixtures and H§12 notes` (with the two trailers). Check: `git status --short` prints nothing.

### Task 7: verification

- [x] **7.1** Run `N2_FORBIDDEN_NAMES="$(id -un)" sh scripts/check-premises.sh`. Expected: the last line is `check-premises: ok (<IDS>)`, exit 0.
- [x] **7.2** Run `python3 -I scripts/premises/hud_probe_test.py` and `sh scripts/premises/live-hud-probe_test.sh`. Expected: `hud_probe tests: ok (14)` and `live-hud-probe tests: ok (12)`, each exit 0.
- [x] **7.3** Run `sh scripts/premises/ver_test.sh` and `PATH="$HOME/.opencode/bin:$PATH" sh scripts/premises/live-selftest.sh`. Expected: `ver tests: ok (51)` and `live selftest: ok (<S>)`, each exit 0.
- [ ] **7.4** Acceptance, as the contract states it, with the OpenCode directory on `PATH`. Run `PATH="$HOME/.opencode/bin:$PATH" sh scripts/premises/live-hud-probe.sh && cargo test -p zygo --test forbidden_content; echo "rc=$?"`. Expected: the eleven `LIVE` lines (one each for PH1, PH2, PH3, PH4, PH5, PH6, PH7, PH9, PH11, PH12, PH13, each `verified`, `failed` or `observed`), `live: fixtures in fixtures/claude-code/hud; scratch removed on exit`, `test result: ok.` with `0 failed`, `rc=0`. This is the second live run (two more Claude Code starts, no prompt). Then run `git diff --quiet -- fixtures/claude-code/hud/prb-1-results.txt; echo "same=$?"`. Expected: `same=0` (the same eleven lines as step 5.1; the texts hold no times or counts). When `same=1`: do not change the records; report the ids whose line changed, with both lines (a premise that gives two answers on the same machine is a finding for the controller). Then run `git status --short`. Expected: nothing, or only `fixtures/claude-code/hud/prb-1-*` files other than `prb-1-results.txt` (measured times and counts of the second run); commit those and this file with all steps ticked: `docs(work): PRB-1 plan steps done; probe fixtures of the acceptance run` (with the two trailers), and check that `git status --short` then prints nothing.

After step 7.4, report to the controller: the eleven `LIVE` lines; for each `failed` id the nodes of the table "A failed line" (a DONE node gets a new node by execute-work §5.4; LIV-2 is re-planned); each line that is a failure of the run (`timeout:`, `claude ended:`, `not observed:`); the PH4 values for FIX-5; the side effects on the operator's Claude Code config folder (two trust entries and project folders for the two scratch repository paths); the auxiliary requests that zygo did not see (no zygo in these runs, so none is counted in zygo's usage rows).

### Block A: `scripts/premises/hud_probe.py`

````python
#!/usr/bin/env python3
"""Probe status line command and analyzer of node PRB-1 (HUD design H§12 "Probe first").

Run by scripts/premises/live-hud-probe.sh; tested by scripts/premises/hud_probe_test.py. No network.

  hud_probe.py settings <python> <this file> <dir> <refresh> <out>   the --settings JSON for Claude Code
  hud_probe.py statusline <dir>                                    the status line command (phase from <dir>/phase)
  hud_probe.py capture <socket> <session> <dir> <phase> <seconds> <escapes 0|1>
  hud_probe.py widths <socket> <session> <out>                     tmux's own cell width of each probe glyph
  hud_probe.py pty <log dir> <cols> <wait> <cwd> <argv...>         one session on a pseudo-terminal
  hud_probe.py ph13 <opencode> <work dir> <out>                    OpenCode help and inline config keys
  hud_probe.py analyze <tmux dir> <pty dir> <pane cols> <pty cols> <widths json> <stdin out>   JSON on stdout
  hud_probe.py fixtures <tmux dir> <pty dir> <analysis json> <ph13 json> <out dir>
"""
import errno
import fcntl
import glob
import json
import os
import re
import select
import signal
import struct
import subprocess
import sys
import termios
import time
import unicodedata

E = "\x1b"
LOGO = "\U0001d607\U0001d606\U0001d5f4\U0001d5fc"
REFL = "zʎƃo"
STRIPE = "▌"
PUA = "\U000f0645"
SCISSORS = "✂"
VS16 = "️"
# The two OP13 printf commands of H§6.3, as bytes (\e -> ESC, %% -> %).
VERBOSE = (f"{E}[31m▌{E}[33m▌{E}[36m▌{E}[0m {E}[1;97m{LOGO}{E}[0m  {E}[1;34m main{E}[0m {E}[38;5;172m{PUA} hud-work{E}[0m {E}[36m↑1{E}[0m   {E}[35m opus-4.7 · high{E}[0m   {E}[36m\U000f0626 fast{E}[0m   {E}[2mlocal{E}[0m\n"
           f"{E}[31m▌{E}[33m▌{E}[36m▌{E}[0m {E}[2;37m{REFL}{E}[0m  ctx 196k/{E}[2m1m{E}[0m {E}[32m▰▰{E}[2m▱▱▱▱▱▱{E}[0m 20%   {E}[2m↑ 1.2m in  ↓ 48k out{E}[0m   {E}[32m$3.41{E}[0m   {E}[2mcache 92%   trimmed 23k tok{E}[0m\n")
COMPACT = (f"{E}[31m▌{E}[33m▌{E}[36m▌{E}[0m {E}[1;97m{LOGO}{E}[0m {E}[1;34m main{E}[0m {E}[38;5;172m{PUA} hud-work{E}[0m  {E}[35m opus-4.7{E}[0m {E}[36m\U000f0626 fast{E}[0m  ctx 196k/{E}[2m1m{E}[0m {E}[32m▰{E}[2m▱▱▱▱{E}[0m  {E}[2m↑1.2m ↓48k{E}[0m  {E}[32m$3.41{E}[0m  {E}[2m\U000f01bc 92%  {SCISSORS} 23k{E}[0m\n")
# 300 ASCII cells: block k (1..30) is the number 10k right-aligned in 10 cells after dots, so the
# number 10k ends at ruler cell 10k.
RULER = "".join("." * (10 - len(str(10 * k))) + str(10 * k) for k in range(1, 31))
# Wide phases: row 1 is a short title, row 2 the measured row (row 2 has the same area in each phase).
WIDE = {"A": ("", 0), "B": (PUA, 10), "C": (SCISSORS, 10), "D": (LOGO, 5)}
CELLS = {"A": 0, "B": 10, "C": 10, "D": 20}   # the glyph cells of row 2 when each code point is 1 cell
ENV_KEEP = ("COLUMNS", "LINES", "TERM", "TERM_PROGRAM", "TERM_PROGRAM_VERSION", "COLORTERM", "NO_COLOR",
            "FORCE_COLOR", "WT_SESSION", "LANG", "LC_ALL", "LC_CTYPE", "CLAUDECODE", "CLAUDE_CODE_ENTRYPOINT")
CSI = re.compile(r"\x1b\[[0-9;:?]*[A-Za-z]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)")


def wide_rows(x):
    g, n = WIDE[x]
    return f"P|wide{x} probe\n{x}|{g * n}{RULER}\n"


def out_for(phase, pid):
    if phase == "verbose":
        return VERBOSE
    if phase == "compact":
        return COMPACT
    if phase == "early":
        return f"PH11 early {pid}\n"
    if phase.startswith("wide") and phase[4:] in WIDE:
        return wide_rows(phase[4:])
    if phase == "slow":
        return f"PH12 slow {pid}\n"
    if phase == "done":
        return "PROBE DONE\n"
    return f"PROBE {phase}\n"


# ---- the status line command ------------------------------------------------------------------

def event(d, **kv):
    kv["t"] = round(time.time(), 3)
    line = (json.dumps(kv, sort_keys=True) + "\n").encode()
    fd = os.open(os.path.join(d, "events.jsonl"), os.O_WRONLY | os.O_CREAT | os.O_APPEND, 0o600)
    try:
        os.write(fd, line)
    finally:
        os.close(fd)


def tty_size():
    try:
        fd = os.open("/dev/tty", os.O_RDONLY | os.O_NOCTTY)
    except OSError as e:
        return {"error": errno.errorcode.get(e.errno, str(e.errno))}
    try:
        rows, cols, _, _ = struct.unpack("HHHH", fcntl.ioctl(fd, termios.TIOCGWINSZ, b"\0" * 8))
        return {"cols": cols, "rows": rows}
    except OSError as e:
        return {"error": errno.errorcode.get(e.errno, str(e.errno))}
    finally:
        os.close(fd)


def read_stdin(limit=1 << 20, wait=2.0):
    """At most <limit> bytes until end of file, for at most <wait> seconds."""
    buf, end = b"", time.monotonic() + wait
    while len(buf) < limit:
        left = end - time.monotonic()
        if left <= 0 or not select.select([0], [], [], left)[0]:
            return buf, "deadline"
        chunk = os.read(0, 65536)
        if not chunk:
            return buf, "eof"
        buf += chunk
    return buf[:limit], "limit"


def cmd_statusline(d):
    try:
        phase = open(os.path.join(d, "phase"), encoding="utf-8").read().strip() or "verbose"
    except OSError:
        phase = "verbose"
    pid = os.getpid()
    if phase == "early":
        os.close(0)   # PH11: stdin is closed unread, before anything else
    event(d, pid=pid, phase=phase, event="start")

    def on_signal(n, _f):
        event(d, pid=pid, phase=phase, event="signal", sig=signal.Signals(n).name[3:])
        os._exit(128 + n)

    for s in (signal.SIGTERM, signal.SIGINT, signal.SIGHUP, signal.SIGPIPE):
        signal.signal(s, on_signal)
    if phase == "early":
        stdin, how = "not read", "closed"
    else:
        raw, how = read_stdin()
        try:
            stdin = json.loads(raw.decode("utf-8", "replace"))
        except ValueError:
            stdin = {"unparsed": raw[:200].decode("utf-8", "replace")}
    env = {k: os.environ[k] for k in ENV_KEEP if k in os.environ}
    for k in ("TMUX", "TMUX_PANE"):
        if k in os.environ:
            env[k] = "set"
    rec = {"t": round(time.time(), 3), "pid": pid, "phase": phase, "tty": tty_size(), "env": env,
           "env_names": sorted(os.environ), "stdin": stdin, "stdin_end": how}
    os.makedirs(os.path.join(d, "log"), exist_ok=True)
    name = os.path.join(d, "log", f"run-{time.time_ns()}-{pid}-{phase}.json")
    with open(name + ".tmp", "w", encoding="utf-8") as f:
        json.dump(rec, f)
    os.replace(name + ".tmp", name)
    if phase == "slow":
        for _ in range(10):   # 5 s, one beat each 0.5 s
            event(d, pid=pid, phase=phase, event="beat")
            time.sleep(0.5)
    try:
        sys.stdout.write(out_for(phase, pid))
        sys.stdout.flush()
    except BrokenPipeError:
        event(d, pid=pid, phase=phase, event="stdout-error")
        os._exit(1)
    event(d, pid=pid, phase=phase, event="end")
    return 0


# ---- driving helpers --------------------------------------------------------------------------

def tmux(sock, *args):
    return subprocess.run(["tmux", "-u", "-L", sock, "-f", "/dev/null", *args], capture_output=True,
                          text=True, timeout=10)


def cmd_capture(sock, session, d, phase, seconds, escapes):
    """Captures the pane every 0.5 s for <seconds> (at least once) into <d>/cap/<phase>-NNN.txt."""
    cap = os.path.join(d, "cap")
    os.makedirs(cap, exist_ok=True)
    n = max(1, int(float(seconds) * 2))
    flags = ["-e", "-p"] if escapes == "1" else ["-p"]
    with open(os.path.join(d, "captures.jsonl"), "a", encoding="utf-8") as idx:
        for i in range(n):
            t = time.time()
            p = tmux(sock, "capture-pane", *flags, "-t", f"={session}:")
            if p.returncode != 0:
                break
            f = os.path.join(cap, f"{phase}-{i:03d}.txt")
            open(f, "w", encoding="utf-8").write(p.stdout)
            idx.write(json.dumps({"phase": phase, "file": f, "t": round(t, 3)}) + "\n")
            if i < n - 1:
                time.sleep(max(0.0, t + 0.5 - time.time()))
    return 0


def cmd_widths(sock, session, out):
    """tmux's own width of each glyph (#{w:} of a user option on the probe's own session)."""
    r = {}
    for k, g in (("B", PUA), ("C", SCISSORS), ("D", LOGO[0]), ("ellipsis", "…")):
        a = tmux(sock, "set-option", "-t", f"={session}:", "@zygo_probe_glyph", g)
        b = tmux(sock, "display-message", "-p", "-t", f"={session}:", "#{w:@zygo_probe_glyph}")
        v = b.stdout.strip()
        r[k] = int(v) if a.returncode == 0 and b.returncode == 0 and v.isdigit() else None
    json.dump(r, open(out, "w"))
    return 0


def cmd_pty(logdir, cols, wait, cwd, *argv):
    """Starts argv on a pseudo-terminal of <cols> x 50 in <cwd>; waits <wait> s for a run log in
    <logdir>, then /exit. Exit 0: a run log; 3: none; 5: the folder trust question showed (not
    answered here: Enter alone means "No, exit" in Claude Code 2.1.330)."""
    import pty
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(143))
    pid, fd = pty.fork()
    if pid == 0:
        os.chdir(cwd)
        os.execv(argv[0], list(argv))
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 50, int(cols), 0, 0))
    raw = re.compile(rb"\x1b\[[0-9;:?]*[A-Za-z]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)")
    state = {"buf": b""}

    def pump(seconds):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            if select.select([fd], [], [], 0.2)[0]:
                try:
                    data = os.read(fd, 65536)
                except OSError:
                    return False
                if not data:
                    return False
                state["buf"] = (state["buf"] + data)[-65536:]
        return True

    code = 3
    try:
        end = time.monotonic() + float(wait)
        while time.monotonic() < end and pump(0.5):
            if glob.glob(os.path.join(logdir, "run-*.json")):
                code = 0
                break
            if b"Itrustthisfolder" in re.sub(rb"\s+", b"", raw.sub(b"", state["buf"])):
                code = 5
                break
        if code == 0:
            pump(1)
            os.write(fd, b"/exit")
            pump(0.5)
            os.write(fd, b"\r")
            end = time.monotonic() + 10
            while time.monotonic() < end and os.waitpid(pid, os.WNOHANG)[0] == 0:
                pump(0.2)
    finally:
        for sig in (signal.SIGHUP, signal.SIGKILL):
            try:
                if os.waitpid(pid, os.WNOHANG)[0] != 0:
                    break
                os.killpg(pid, sig)
                time.sleep(2)
            except (ProcessLookupError, ChildProcessError):
                break
    return code


def cmd_ph13(oc, work, out):
    """PH13: no status line in OpenCode's help texts, and no status line key in its inline config:
    `opencode debug config --pure` keeps a known key (username) and drops an unknown one; each
    candidate key is then dropped (no such key) or kept (OpenCode has that key)."""
    os.makedirs(work, exist_ok=True)
    home = os.path.join(work, "home")
    env = {"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "HOME": home,
           "XDG_CONFIG_HOME": os.path.join(home, ".config"), "XDG_DATA_HOME": os.path.join(home, ".local/share"),
           "XDG_CACHE_HOME": os.path.join(home, ".cache"), "XDG_STATE_HOME": os.path.join(home, ".local/state"),
           "OPENCODE_DISABLE_AUTOUPDATE": "1"}
    for k in ("XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME", "XDG_STATE_HOME"):
        os.makedirs(env[k], exist_ok=True)
    rx = re.compile(r"(?i)status[ _-]?line")

    def run(args, extra=None):
        try:
            p = subprocess.run([oc, *args], capture_output=True, text=True, timeout=30, cwd=work,
                               env=dict(env, **(extra or {})), stdin=subprocess.DEVNULL)
            return p.returncode, p.stdout, p.stderr
        except subprocess.TimeoutExpired:
            return 124, "", "timeout after 30 s"

    r = {"version": run(["--version"])[1].strip()}
    hits = []
    for args in (["--help"], ["debug", "--help"]):
        rc, o, e = run(args)
        hits += [" ".join(args) + ": " + m.group(0) for m in rx.finditer(o + e)]
    r["help_hits"] = hits

    def key(k, value):
        rc, o, e = run(["debug", "config", "--pure"], {"OPENCODE_CONFIG_CONTENT": json.dumps({k: value})})
        try:
            d = json.loads(o)
        except ValueError:
            d = None
        if rc == 0 and isinstance(d, dict):
            return "kept" if k in d else "dropped"
        return "rejected" if rc != 0 and k in (o + e) else f"error (exit {rc})"

    cmd = {"type": "command", "command": "true"}
    r["control"] = {"username": key("username", "zygo-probe"), "zygoProbeUnknownKey": key("zygoProbeUnknownKey", cmd)}
    r["keys"] = {k: key(k, cmd) for k in ("statusLine", "statusline", "status_line")}
    json.dump(r, open(out, "w"), indent=1)
    return 0


# ---- analysis ---------------------------------------------------------------------------------

def plain(s):
    return CSI.sub("", s)


def cells(s):
    return sum(0 if unicodedata.combining(c) else 2 if unicodedata.east_asian_width(c) in "WF" else 1 for c in s)


def captures(d, phase):
    out = []
    try:
        for line in open(os.path.join(d, "captures.jsonl"), encoding="utf-8"):
            c = json.loads(line)
            if c["phase"] == phase and os.path.exists(c["file"]):
                out.append((c["t"], open(c["file"], encoding="utf-8", errors="replace").read()))
    except OSError:
        pass
    return out


def block(text):
    """The pane lines from the last line that holds the logo to the last non-empty line, SGR kept."""
    lines = text.split("\n")
    at = [i for i, l in enumerate(lines) if LOGO in plain(l)]
    if not at:
        return []
    out = lines[at[-1]:]
    while out and not plain(out[-1]).strip():
        out.pop()
    return out


def attrs(line):
    """[(char, fg, bold, dim)] of one capture-pane -e line; fg is a palette index or None."""
    out, fg, bold, dim, i = [], None, False, False, 0
    while i < len(line):
        m = CSI.match(line, i)
        if not m:
            out.append((line[i], fg, bold, dim))
            i += 1
            continue
        i = m.end()
        if not m.group(0).endswith("m") or m.group(0).startswith("\x1b]"):
            continue
        p = [int(x) if x else 0 for x in re.split("[;:]", m.group(0)[2:-1])]
        k = 0
        while k < len(p):
            c = p[k]
            if c == 0:
                fg, bold, dim = None, False, False
            elif c == 1:
                bold = True
            elif c == 2:
                dim = True
            elif c == 22:
                bold = dim = False
            elif 30 <= c <= 37:
                fg = c - 30
            elif 90 <= c <= 97:
                fg = c - 90 + 8
            elif c == 39:
                fg = None
            elif c == 38 and k + 2 < len(p) and p[k + 1] == 5:
                fg = p[k + 2]
                k += 2
            elif c == 38 and k + 4 < len(p) and p[k + 1] == 2:
                fg = "rgb"
                k += 4
            k += 1
    return out


def an_sgr(text):
    b = block(text)
    if not b:
        return {"ok": False, "ok16": False, "ok256": False, "why": "no logo row"}
    a = attrs(b[0])
    t = "".join(c for c, *_ in a)
    s, l, w = t.find(STRIPE * 3), t.find(LOGO), t.find("hud-work")
    stripe = [a[s + n][1] for n in range(3)] if s >= 0 else []
    logo = [a[l][1], a[l][2]] if l >= 0 else [None, False]
    tree = a[w][1] if w >= 0 else None
    r = {"stripe": stripe, "logo_fg": logo[0], "logo_bold": logo[1], "worktree_fg": tree}
    r["ok16"] = stripe == [1, 3, 6] and logo[0] == 15 and logo[1] is True
    r["ok256"] = tree == 172
    r["ok"] = r["ok16"] and r["ok256"]
    return r


def an_two(text):
    b = [plain(l) for l in block(text)]
    if not b:
        return {"rows": 0, "logo_col": -1, "refl_col": -1}
    lc = b[0].find(LOGO)
    rc = b[1].find(REFL) if len(b) > 1 else -1
    return {"rows": 2 if rc >= 0 and STRIPE in b[1] and REFL not in b[0] else 1, "logo_col": lc, "refl_col": rc}


def an_compact(text):
    t = text
    return {"scissors": SCISSORS in plain(t), "vs16": (SCISSORS + VS16) in t}


def an_wide(x, text, cols):
    """Row 2 of a wide phase: the ruler cells it shows after its glyphs, and cut or wrap."""
    g, n = WIDE[x]
    lines = [plain(l) for l in text.split("\n")]
    at = [i for i, l in enumerate(lines) if l.lstrip(" ").startswith(f"{x}|")]
    if not at:
        return {"class": "none", "why": f"no {x}| row in the pane"}
    i = at[-1]
    line = lines[i].rstrip()
    indent = len(line) - len(line.lstrip(" "))
    rest = line.lstrip(" ")[2:]
    if not rest.startswith(g * n):
        return {"class": "none", "why": f"the {x}| row does not start with its {n} glyphs", "indent": indent}
    t = rest[len(g * n):]
    ell = t.endswith("…")
    t = (t[:-1] if ell else t).rstrip()
    if not RULER.startswith(t):
        return {"class": "other", "why": f"the {x}| row does not show a ruler prefix", "indent": indent}
    nxt = lines[i + 1].strip() if i + 1 < len(lines) else ""
    wrapped = len(nxt) >= 3 and RULER[len(t):].startswith(nxt[:10])
    r = {"ruler": len(t), "ellipsis": ell, "indent": indent, "shown": len(line) - indent, "cols": int(cols)}
    r["class"] = "wrapped" if wrapped else "cut" if len(t) < len(RULER) else "whole"
    if x == "A":
        r["gap"] = int(cols) - len(line)
        r["loss"] = indent + r["gap"]
    return r


def runs(d):
    out = []
    for f in sorted(glob.glob(os.path.join(d, "log", "run-*.json"))):
        try:
            out.append(json.load(open(f)))
        except (OSError, ValueError):
            pass
    return sorted(out, key=lambda r: r["t"])


def events(d):
    out = []
    try:
        for line in open(os.path.join(d, "events.jsonl"), encoding="utf-8"):
            try:
                out.append(json.loads(line))
            except ValueError:
                pass
    except OSError:
        pass
    return sorted(out, key=lambda e: e["t"])


def by_run(evs, phase):
    rs = {}
    for e in evs:
        if e.get("phase") != phase:
            continue
        r = rs.setdefault(e["pid"], {"pid": e["pid"], "start": None, "end": None, "signal": None, "last": None})
        if e["event"] == "start":
            r["start"] = e["t"]
        elif e["event"] == "end":
            r["end"] = e["t"]
        elif e["event"] == "signal":
            r["signal"] = (e["t"], e.get("sig"))
        r["last"] = e["t"]
    return sorted((r for r in rs.values() if r["start"] is not None), key=lambda r: r["start"])


def an_ph11(d):
    evs = by_run(events(d), "early")
    caps = captures(d, "early")
    texts = [plain(t) for _, t in caps]
    seen = {int(m.group(1)) for t in texts for m in re.finditer(r"PH11 early (\d+)", t)}
    last = caps[-1][0] if caps else 0
    ended = [r for r in evs if r["end"] is not None]
    lost = [r["pid"] for r in ended if r["end"] <= last - 2.5 and r["pid"] not in seen]
    err = any(re.search(r"(?i)EPIPE|status ?line[^\n]*(error|fail)", t) for t in texts)
    after = any("PROBE DONE" in plain(t) for _, t in captures(d, "done"))
    return {"runs": len(evs), "ended": len(ended), "shown": len(seen), "lost": len(lost), "error": err,
            "after": after, "captures": len(caps)}


def an_ph12(d):
    rs = by_run(events(d), "slow")
    caps = captures(d, "slow")
    shown = any("PH12 slow" in plain(t) for _, t in caps)
    sigs = sorted({r["signal"][1] for r in rs if r["signal"]})
    stop = []
    for r in rs:
        if r["end"] is not None:
            stop.append((r["end"], "end"))
        elif r["signal"]:
            stop.append((r["signal"][0], "signal"))
        else:
            stop.append((r["last"], "lost"))
    cls = "none"
    for i in range(len(rs) - 1):
        t_stop, how = stop[i]
        nxt = rs[i + 1]["start"]
        if how == "end":
            cls = "overlaps" if nxt < t_stop - 0.2 else "waits"
        else:
            cls = "kills" if t_stop - rs[i]["start"] < 4.5 else "waits"
        break
    pts = sorted([(r["start"], 1) for r in rs] + [(s[0], -1) for s in stop], key=lambda p: (p[0], p[1]))
    cur = maxc = 0
    for _, dlt in pts:
        cur += dlt
        maxc = max(maxc, cur)
    lives = [round(stop[i][0] - r["start"], 2) for i, r in enumerate(rs)]
    return {"starts": len(rs), "ends": sum(1 for s in stop if s[1] == "end"), "signals": sigs,
            "no_end": sum(1 for s in stop if s[1] == "lost"), "max_concurrent": maxc, "class": cls,
            "shown": shown, "lives": lives}


def an_stdin(t_runs, p_runs):
    out = []
    for sess, rs in (("tmux", t_runs), ("pty", p_runs)):
        for r in rs:
            if isinstance(r.get("stdin"), dict) and "unparsed" not in r["stdin"]:
                out.append({"session": sess, "phase": r["phase"], "keys": list(r["stdin"]),
                            "effort": r["stdin"].get("effort")})
    return out


def cmd_analyze(td, pd, cols, pcols, widths, stdin_out):
    t_runs, p_runs = runs(td), runs(pd)
    st = an_stdin(t_runs, p_runs)
    json.dump(st, open(stdin_out, "w"))
    v = captures(td, "verbose")
    c = captures(td, "compact")
    r = {"sgr": an_sgr(v[-1][1]) if v else {"ok": False, "ok16": False, "ok256": False, "why": "no verbose capture"},
         "two": an_two(v[-1][1]) if v else {"rows": 0, "logo_col": -1, "refl_col": -1},
         "compact": an_compact(c[-1][1]) if c else {"scissors": False, "vs16": False}}
    try:
        r["tw"] = json.load(open(widths))
    except (OSError, ValueError):
        r["tw"] = {}
    wide = {}
    for x in "ABCD":
        caps = captures(td, f"wide{x}")
        wide[x] = an_wide(x, caps[-1][1], cols) if caps else {"class": "none", "why": f"no wide{x} capture"}
    ra = wide["A"].get("ruler")
    for x in "BCD":
        rx = wide[x].get("ruler")
        if ra is not None and rx is not None:
            wide[x]["diff"] = ra - rx
            q, rem = divmod(ra - rx, CELLS[x])
            wide[x]["c"] = q if rem == 0 else round((ra - rx) / CELLS[x], 2)
    r["wide"] = wide

    def first(rs, k):
        return rs[0][k] if rs else None

    tt, pt = first(t_runs, "tty"), first(p_runs, "tty")
    te, pe = first(t_runs, "env") or {}, first(p_runs, "env") or {}
    show = lambda t: "none" if t is None else str(t["cols"]) if "cols" in t else t["error"]
    r["ph4"] = {"tty_t": show(tt), "tty_p": show(pt), "col_t": te.get("COLUMNS", "unset"),
                "col_p": pe.get("COLUMNS", "unset"), "indent": wide["A"].get("indent"),
                "gap": wide["A"].get("gap"), "loss": wide["A"].get("loss")}
    eff = [s["effort"] for s in st]
    r["ph5"] = {"runs": len(st), "first_keys": st[0]["keys"] if st else [],
                "efforts": sorted({json.dumps(e, sort_keys=True) for e in eff})}
    r["ph11"] = an_ph11(td)
    r["ph12"] = an_ph12(td)
    r["runs"] = {"tmux": len(t_runs), "pty": len(p_runs)}
    json.dump(r, sys.stdout, ensure_ascii=False)
    sys.stdout.write("\n")
    return 0


def cmd_fixtures(td, pd, an, ph13, out):
    """Writes the fixture sources (not yet scrubbed) into <out>: no whole stdin, no pid, no path."""
    os.makedirs(out, exist_ok=True)
    a = json.load(open(an))

    def rows(text, n, keep):
        b = block(text)[:n]
        return "".join((l if keep else plain(l)).rstrip() + "\n" for l in b)

    v, c = captures(td, "verbose"), captures(td, "compact")
    if v:
        open(os.path.join(out, "verbose.txt"), "w").write(rows(v[-1][1], 2, True))
    if c:
        open(os.path.join(out, "compact.txt"), "w").write(rows(c[-1][1], 1, True))
    w = []
    for x in "ABCD":
        caps = captures(td, f"wide{x}")
        if not caps:
            continue
        lines = [plain(l).rstrip() for l in caps[-1][1].split("\n")]
        at = [i for i, l in enumerate(lines) if l.lstrip(" ").startswith(f"P|wide{x}")]
        if at:
            w.append(f"# wide{x}\n" + "\n".join(lines[at[-1]:at[-1] + 3]) + "\n")
    if w:
        open(os.path.join(out, "wide.txt"), "w").write("".join(w))
    for ph, pat in (("early", r"PH11 early \d+"), ("slow", r"PH12 slow \d+")):
        seen, nums = [], {}
        for _, t in captures(td, ph):
            for m in re.finditer(pat, plain(t)):
                k = m.group(0)
                if k not in nums:
                    nums[k] = len(nums) + 1
                    seen.append(re.sub(r"\d+$", f"<run {nums[k]}>", k))
        if seen:
            open(os.path.join(out, f"{ph}.txt"), "w").write("\n".join(seen) + "\n")
    t0 = None
    evs = []
    pids = {}
    for e in events(td):
        t0 = e["t"] if t0 is None else t0
        pids.setdefault(e["pid"], len(pids) + 1)
        evs.append({"t": round(e["t"] - t0, 2), "run": pids[e["pid"]], "phase": e["phase"], "event": e["event"],
                    **({"sig": e["sig"]} if "sig" in e else {})})
    json.dump(evs, open(os.path.join(out, "events.json"), "w"))
    for sess, d in (("tmux", td), ("pty", pd)):
        rs = runs(d)
        if not rs:
            continue
        r0 = rs[0]
        json.dump({"phase": r0["phase"], "keys": list(r0["stdin"]) if isinstance(r0["stdin"], dict) else r0["stdin"],
                   "effort": r0["stdin"].get("effort") if isinstance(r0["stdin"], dict) else None,
                   "stdin_end": r0.get("stdin_end"), "runs": len(rs)},
                  open(os.path.join(out, f"stdin-{sess}.json"), "w"), indent=1, ensure_ascii=False)
        json.dump({"tty": r0["tty"], "env": r0["env"],
                   "env_names": [n for n in r0["env_names"] if n not in r0["env"]]},
                  open(os.path.join(out, f"env-{sess}.json"), "w"), indent=1)
    json.dump(a, open(os.path.join(out, "analysis.json"), "w"), indent=1, ensure_ascii=False)
    try:
        json.dump(json.load(open(ph13)), open(os.path.join(out, "ph13.json"), "w"), indent=1)
    except (OSError, ValueError):
        pass
    return 0


def cmd_settings(python, me, d, refresh, out):
    cmd = f"'{python}' -I '{me}' statusline '{d}'"
    json.dump({"statusLine": {"type": "command", "command": cmd, "refreshInterval": int(refresh)}}, open(out, "w"))
    return 0


if __name__ == "__main__":
    sys.exit(globals()["cmd_" + sys.argv[1]](*sys.argv[2:]))
````

### Block B: `scripts/premises/hud_probe_test.py`

````python
#!/usr/bin/env python3
"""Tests of scripts/premises/hud_probe.py (node PRB-1). No agent, no network, no tmux.
Usage: python3 -I scripts/premises/hud_probe_test.py   ->  "hud_probe tests: ok (<n>)", exit 0"""
import importlib.util
import json
import os
import re
import signal
import subprocess
import sys
import tempfile
import time

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
PROBE = os.path.join(HERE, "hud_probe.py")
spec = importlib.util.spec_from_file_location("hud_probe", PROBE)
hp = importlib.util.module_from_spec(spec)
spec.loader.exec_module(hp)
E = "\x1b"
n = bad = 0


def check(name, cond, why=""):
    global n, bad
    n += 1
    if cond:
        print("ok:", name)
    else:
        bad += 1
        print(f"FAIL: {name} ({why})")


def probe(*args, **kw):
    return subprocess.run([sys.executable, "-I", PROBE, *args], capture_output=True, **kw)


def rundir():
    d = tempfile.mkdtemp(dir=T)
    os.makedirs(os.path.join(d, "log"))
    return d


def setphase(d, p):
    open(os.path.join(d, "phase"), "w").write(p + "\n")


def logs(d):
    return [json.load(open(os.path.join(d, "log", f))) for f in sorted(os.listdir(os.path.join(d, "log")))]


def evs(d):
    return [json.loads(l) for l in open(os.path.join(d, "events.jsonl"))]


def cap(d, phase, texts, t0=1000.0, step=0.5):
    os.makedirs(os.path.join(d, "cap"), exist_ok=True)
    with open(os.path.join(d, "captures.jsonl"), "a") as idx:
        for i, t in enumerate(texts):
            f = os.path.join(d, "cap", f"{phase}-{i:03d}.txt")
            open(f, "w").write(t)
            idx.write(json.dumps({"phase": phase, "file": f, "t": t0 + i * step}) + "\n")


def ev(d, rows):
    with open(os.path.join(d, "events.jsonl"), "a") as f:
        for r in rows:
            f.write(json.dumps(r) + "\n")


T = tempfile.mkdtemp()
try:
    # H1: the probe bytes equal the two printf commands of H§6.3.
    text = open(os.path.join(REPO, "docs/specs/2026-11-21-statusline-hud-design.md"), encoding="utf-8").read()
    sec = text[text.index("### 6.3 Mockups"):text.index("### 6.4 Colors")]
    pf = re.findall(r"^printf '(.*)'$", sec, re.M)
    conv = [p.replace("\\e", E).replace("\\n", "\n").replace("%%", "%") for p in pf]
    check("H1 VERBOSE and COMPACT equal the H§6.3 printf bytes", len(conv) == 2 and conv[0] == hp.VERBOSE and conv[1] == hp.COMPACT,
          f"{len(conv)} printf lines")

    # H2: verbose phase: output bytes, stdin logged, env kept and omitted, no terminal.
    d = rundir()
    setphase(d, "verbose")
    tok = "probe" + "-secret-" + "4f1c"
    env = {"PATH": os.environ["PATH"], "COLUMNS": "88", "PROBE_SECRET": tok, "TMUX": "x"}
    p = probe("statusline", d, input=json.dumps({"effort": "high", "session_id": "s1"}).encode(), env=env,
              start_new_session=True, timeout=10)
    r = logs(d)[0]
    blob = "".join(open(os.path.join(r0, f), errors="replace").read() for r0, _, fs in os.walk(d) for f in fs)
    check("H2 verbose: bytes, stdin, env allowlist, no terminal",
          p.returncode == 0 and p.stdout.decode() == hp.VERBOSE and r["stdin"] == {"effort": "high", "session_id": "s1"}
          and r["env"].get("COLUMNS") == "88" and "PROBE_SECRET" not in r["env"] and "PROBE_SECRET" in r["env_names"]
          and r["env"].get("TMUX") == "set" and "error" in r["tty"] and tok not in blob and r["stdin_end"] == "eof",
          f"rc {p.returncode} rec {r}")

    # H3: early phase: stdin closed unread (the writer's pipe stays open), at once.
    d = rundir()
    setphase(d, "early")
    t0 = time.monotonic()
    pr = subprocess.Popen([sys.executable, "-I", PROBE, "statusline", d], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                          start_new_session=True)
    out, _ = pr.communicate(timeout=10)
    pr.stdin = None
    dt = time.monotonic() - t0
    r = logs(d)[0]
    check("H3 early: stdin not read, one line, under 2 s",
          pr.returncode == 0 and out.decode() == f"PH11 early {pr.pid}\n" and r["stdin"] == "not read" and dt < 2,
          f"rc {pr.returncode} out {out!r} dt {dt:.2f}")

    # H4: slow phase, SIGTERM after 1 s: exit 143, events start, beat and signal TERM, no end.
    d = rundir()
    setphase(d, "slow")
    pr = subprocess.Popen([sys.executable, "-I", PROBE, "statusline", d], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                          start_new_session=True)
    pr.stdin.write(b"{}")
    pr.stdin.close()
    time.sleep(1.0)
    pr.send_signal(signal.SIGTERM)
    pr.wait(timeout=10)
    kinds = [e["event"] for e in evs(d)]
    sig = [e.get("sig") for e in evs(d) if e["event"] == "signal"]
    check("H4 slow + SIGTERM: exit 143, start, beat, signal TERM, no end",
          pr.returncode == 143 and kinds[0] == "start" and "beat" in kinds and sig == ["TERM"] and "end" not in kinds,
          f"rc {pr.returncode} events {kinds}")

    # H5: wide phases: two rows, the measured row is <X>| + glyphs + the 300-cell ruler.
    d = rundir()
    ok = len(hp.RULER) == 300 and hp.RULER.endswith("300") and hp.RULER[:20] == "........10........20"
    for x in "ABCD":
        setphase(d, "wide" + x)
        o = probe("statusline", d, input=b"{}", timeout=10).stdout.decode().split("\n")
        g, k = hp.WIDE[x]
        ok = ok and len(o) == 3 and o[0] == f"P|wide{x} probe" and o[1] == f"{x}|{g * k}{hp.RULER}" and o[2] == ""
    check("H5 wide rows and ruler", ok)

    # H6: analysis of a cut capture with 1-cell glyphs: cut, c = 1, loss = indent + gap.
    d = rundir()
    W = 120
    area = W - 4

    def row(x, c=1):
        g, k = hp.WIDE[x]
        glyph_cells = hp.CELLS[x] * c
        r = area - 2 - glyph_cells - 1
        return f"  {x}|{g * k}{hp.RULER[:r]}…"

    for x in "ABCD":
        cap(d, "wide" + x, [f"x\n  P|wide{x} probe\n{row(x)}\n  ? for shortcuts\n"])
    a = {x: hp.an_wide(x, open(os.path.join(d, "cap", f"wide{x}-000.txt")).read(), W) for x in "ABCD"}
    check("H6 cut, ruler cells, loss 4", a["A"]["class"] == "cut" and a["A"]["ruler"] == area - 3 and a["A"]["loss"] == 4
          and a["A"]["indent"] == 2 and a["A"]["gap"] == 2 and all(a[x]["class"] == "cut" for x in "BCD")
          and a["A"]["ruler"] - a["B"]["ruler"] == 10 and a["A"]["ruler"] - a["D"]["ruler"] == 20, str(a))

    # H7: a glyph counted 2 cells gives diff 20 (B); a wrapped row is "wrapped".
    b2 = hp.an_wide("B", f"  P|wideB probe\n{row('B', 2)}\n", W)
    full = f"A|{hp.RULER}"
    wr = hp.an_wide("A", "  P|wideA probe\n  " + full[:W - 4] + "\n  " + full[W - 4:W - 4 + 50] + "\n", W)
    check("H7 2-cell glyph and wrap", a["A"]["ruler"] - b2["ruler"] == 20 and wr["class"] == "wrapped", f"{b2} {wr}")

    # H8: SGR of the verbose bytes as tmux shows them; 16-color only fails the 256-color part.
    pane = "\n".join("  " + l for l in hp.VERBOSE.split("\n"))
    s = hp.an_sgr(pane)
    s2 = hp.an_sgr(pane.replace("38;5;172", "33"))
    t2 = hp.an_two(pane)
    check("H8 sgr and two rows", s["ok"] and not s2["ok256"] and s2["ok16"] and t2 == {"rows": 2, "logo_col": 6, "refl_col": 6},
          f"{s} {s2} {t2}")

    # H9: compact: scissors present, no VS16; with VS16 found.
    check("H9 compact scissors and VS16", hp.an_compact(hp.COMPACT) == {"scissors": True, "vs16": False}
          and hp.an_compact(hp.COMPACT.replace(hp.SCISSORS, hp.SCISSORS + hp.VS16))["vs16"] is True)

    # H10: PH11: 5 early runs, each shown; later the done phase shows its row.
    d = rundir()
    rows = []
    for i in range(5):
        rows += [{"t": 1000 + 2 * i, "pid": 100 + i, "phase": "early", "event": "start"},
                 {"t": 1000.1 + 2 * i, "pid": 100 + i, "phase": "early", "event": "end"}]
    ev(d, rows)
    cap(d, "early", [f"  PH11 early {100 + i // 4}\n" for i in range(20)], t0=1000.3)
    cap(d, "done", ["  PROBE DONE\n"], t0=1040)
    r = hp.an_ph11(d)
    lost_d = rundir()
    ev(lost_d, rows)
    cap(lost_d, "early", ["  PH11 early 100\n"] * 20, t0=1000.3)
    r2 = hp.an_ph11(lost_d)
    check("H10 PH11 shown, lost, after", r["shown"] == 5 and r["lost"] == 0 and r["after"] and not r["error"]
          and r2["lost"] >= 3 and not r2["after"], f"{r} {r2}")

    # H11: PH12 classes: kills (signal at 2.1 s), waits (end, then start), overlaps (start before end).
    def ph12(rows):
        dd = rundir()
        ev(dd, [dict(e, phase="slow") for e in rows])
        return hp.an_ph12(dd)

    k = ph12([{"t": 0, "pid": 1, "event": "start"}, {"t": 2.1, "pid": 1, "event": "signal", "sig": "TERM"},
              {"t": 2.15, "pid": 2, "event": "start"}, {"t": 4.2, "pid": 2, "event": "signal", "sig": "TERM"}])
    w = ph12([{"t": 0, "pid": 1, "event": "start"}, {"t": 5.1, "pid": 1, "event": "end"},
              {"t": 6, "pid": 2, "event": "start"}, {"t": 11, "pid": 2, "event": "end"}])
    o = ph12([{"t": 0, "pid": 1, "event": "start"}, {"t": 2, "pid": 2, "event": "start"},
              {"t": 5.1, "pid": 1, "event": "end"}, {"t": 7.1, "pid": 2, "event": "end"}])
    check("H11 PH12 kills, waits, overlaps", k["class"] == "kills" and k["signals"] == ["TERM"] and k["max_concurrent"] == 1
          and w["class"] == "waits" and o["class"] == "overlaps" and o["max_concurrent"] == 2, f"{k} {w} {o}")

    # H12: PH13 with stub opencode programs: a strict stub (verified case) and a stub that keeps statusLine.
    def stub(path, keep):
        open(path, "w").write(f"""#!{sys.executable}
import json, os, sys
a = sys.argv[1:]
if a == ["--version"]: print("1.19.6")
elif a and a[-1] == "--help": print("Commands:\\n  opencode run   run opencode")
elif a[:2] == ["debug", "config"]:
    c = json.loads(os.environ.get("OPENCODE_CONFIG_CONTENT", "{{}}"))
    known = {keep!r}
    out = {{"agent": {{}}, "username": "unknown"}}
    out.update({{k: v for k, v in c.items() if k in known}})
    print(json.dumps(out))
""")
        os.chmod(path, 0o755)

    stub(os.path.join(T, "oc1"), ["username", "model"])
    stub(os.path.join(T, "oc2"), ["username", "statusLine"])
    probe("ph13", os.path.join(T, "oc1"), os.path.join(T, "w1"), os.path.join(T, "p1.json"), timeout=60)
    probe("ph13", os.path.join(T, "oc2"), os.path.join(T, "w2"), os.path.join(T, "p2.json"), timeout=60)
    p1, p2 = json.load(open(os.path.join(T, "p1.json"))), json.load(open(os.path.join(T, "p2.json")))
    check("H12 PH13 dropped keys, control, and a kept key",
          p1["version"] == "1.19.6" and p1["help_hits"] == [] and p1["control"] == {"username": "kept", "zygoProbeUnknownKey": "dropped"}
          and set(p1["keys"].values()) == {"dropped"} and p2["keys"]["statusLine"] == "kept", f"{p1} {p2}")

    # H13: analyze and fixtures end to end on a synthetic run: no pid, no stdin value but effort and keys.
    td, pd = rundir(), rundir()
    rec = lambda d, ph, cols, tty: open(os.path.join(d, "log", f"run-1-1-{ph}.json"), "w").write(json.dumps(
        {"t": 1, "pid": 4242, "phase": ph, "tty": tty, "env": {"COLUMNS": cols}, "env_names": ["COLUMNS", "HOME"],
         "stdin": {"effort": {"level": "medium"}, "session_id": "s-77", "cwd": "/x"}, "stdin_end": "eof"}))
    rec(td, "verbose", "120", {"error": "ENXIO"})
    rec(pd, "verbose", "100", {"error": "ENXIO"})
    cap(td, "verbose", [pane])
    cap(td, "compact", ["  " + hp.COMPACT])
    for x in "ABCD":
        cap(td, "wide" + x, [f"  P|wide{x} probe\n{row(x)}\n"], t0=2000)
    ev(td, [{"t": 3000, "pid": 4242, "phase": "slow", "event": "start"}])
    json.dump({"B": 1, "C": 1, "D": 1, "ellipsis": 1}, open(os.path.join(T, "tw.json"), "w"))
    out = probe("analyze", td, pd, str(W), "100", os.path.join(T, "tw.json"), os.path.join(T, "stdin.json"), timeout=30)
    an = json.loads(out.stdout)
    json.dump(an, open(os.path.join(T, "an.json"), "w"))
    probe("fixtures", td, pd, os.path.join(T, "an.json"), os.path.join(T, "p1.json"), os.path.join(T, "fx"), timeout=30)
    fx = {f: open(os.path.join(T, "fx", f)).read() for f in os.listdir(os.path.join(T, "fx"))}
    st = json.load(open(os.path.join(T, "stdin.json")))
    check("H13 analyze and fixtures",
          an["ph4"] == {"tty_t": "ENXIO", "tty_p": "ENXIO", "col_t": "120", "col_p": "100", "indent": 2, "gap": 2, "loss": 4}
          and an["wide"]["B"]["diff"] == 10 and an["wide"]["D"]["c"] == 1 and an["wide"]["A"]["shown"] == W - 4
          and an["sgr"]["ok"] and len(st) == 2
          and all(s["effort"] == {"level": "medium"} for s in st)
          and {"verbose.txt", "compact.txt", "wide.txt", "events.json", "stdin-tmux.json", "env-tmux.json",
               "stdin-pty.json", "env-pty.json", "analysis.json", "ph13.json"} <= set(fx)
          and not any("4242" in v or "s-77" in v for v in fx.values()), f"{an} {sorted(fx)}")

    # H14: settings JSON: the command, refreshInterval, no padding key.
    probe("settings", "/usr/bin/python3", PROBE, "/tmp/run/t", "2", os.path.join(T, "s.json"), timeout=10)
    sj = json.load(open(os.path.join(T, "s.json")))
    check("H14 settings", sj == {"statusLine": {"type": "command", "refreshInterval": 2,
                                                "command": f"'/usr/bin/python3' -I '{PROBE}' statusline '/tmp/run/t'"}}, str(sj))
finally:
    subprocess.run(["rm", "-rf", T])

if bad:
    print(f"hud_probe tests: {bad} failed")
    sys.exit(1)
print(f"hud_probe tests: ok ({n})")
````

### Block C: `scripts/premises/live-hud-probe.sh`

````sh
#!/bin/sh
# Live probe PRB-1 (HUD design H§12 "Probe first"): a temporary statusLine command (hud_probe.py) in the
# real Claude Code, on Linux or macOS, for PH1 PH2 PH3 PH4 PH5 PH6 PH7 PH9 PH11 PH12; PH13 on OpenCode.
# Run by hand only; never from CI or a Rust test.
# Usage: sh scripts/premises/live-hud-probe.sh   (claude and opencode on PATH; tmux 3.2 or later, python3, jq)
# Prints eleven lines, LIVE <id> verified|failed|observed (agent <version>): <text>. Exit 0 when the eleven
# lines are printed (a failed line does not change the exit status); exit 1 when a precondition fails,
# before any agent starts.
#
# What it does to the operator's machine:
# - Claude Code runs as `claude --settings <run file>` (no zygo), twice: in a private tmux server
#   (tmux -L zygo-prb-<pid>, no tmux config file, 200 x 50) and on a pseudo-terminal of 100 columns. The
#   settings file holds only the statusLine command; a --settings statusLine comes before the user's.
#   No settings file of the user is written.
# - Both runs use the real HOME and the caller's CLAUDE_CONFIG_DIR, so Claude Code reads the operator's
#   login, settings, hooks, plugins and MCP servers. DISABLE_AUTOUPDATER=1 is set. No prompt is typed:
#   Claude Code sends only its own start requests.
# - Claude Code asks whether it trusts the run's temporary repository; the script answers yes (it moves the
#   pointer to "Yes, I trust this folder", reads the pane to see it there, then sends Enter). Claude Code
#   keeps that answer and a project folder for the path in its config folder; the script does not delete them.
# - PH13 runs `opencode --help`, `opencode debug --help` and `opencode debug config --pure` with a temporary
#   HOME; OpenCode sends no model request there.
# - The traps end the private tmux server (and so Claude Code in it), the pseudo-terminal session and the
#   background processes, and remove the scratch tree.

for t in tmux python3 jq; do command -v "$t" >/dev/null || { echo "live: $t is not on PATH"; exit 1; }; done
[ $# -eq 0 ] || { echo "usage: sh scripts/premises/live-hud-probe.sh"; exit 1; }
PRB_CC_DIR=${CLAUDE_CONFIG_DIR:-}
unset TMUX TMUX_PANE
. "$(dirname "$0")/lib.sh"
. "$PREMISES_DIR/live.sh"
live_traps

# Every tmux call of this script and of lib.sh and live.sh goes to a private server with no config file.
PRB_SOCK="zygo-prb-$$"
tmux() { command tmux -u -L "$PRB_SOCK" -f /dev/null "$@"; }
# prb_end <status>: end the private tmux server (and every agent session in it), remove its socket file,
# then live_end (which kills LIVE_PIDS, removes the scratch tree and exits with <status>).
prb_end() {
  trap - EXIT INT TERM HUP PIPE
  command tmux -L "$PRB_SOCK" kill-server 2>/dev/null || true
  rm -f "${TMUX_TMPDIR:-/tmp}/tmux-$(id -u)/$PRB_SOCK" 2>/dev/null || true
  live_end "$1"
}
trap 'prb_end $?' EXIT
trap 'prb_end 129' HUP
trap 'prb_end 130' INT
trap 'prb_end 141' PIPE
trap 'prb_end 143' TERM

pinned_agents
TMUX_VER=$(command tmux -V | sed 's/^tmux //')
ver_ge "$(printf '%s' "$TMUX_VER" | sed 's/[^0-9.].*$//')" 3.2 || { echo "live: tmux is $TMUX_VER; the minimum is 3.2"; exit 1; }
case "$HOME$PRB_CC_DIR$TOOL_PATH$N2_SCRATCH$PREMISES_DIR" in
  *[[:space:]\']*) echo "live: a path (HOME, CLAUDE_CONFIG_DIR, PATH, the scratch dir or the checkout) holds a blank or a quote"; exit 1 ;;
esac

FX="$REPO_DIR/fixtures/claude-code/hud"
mkdir -p "$FX"
LOGO=$(printf '\360\235\230\207\360\235\230\206\360\235\227\264\360\235\227\274')   # U+1D607 U+1D606 U+1D5F4 U+1D5FC
PTR=$(printf '\342\235\257')                                                        # U+276F, the pointer of a Claude Code menu
SCISSORS=$(printf '\342\234\202')                                                   # U+2702
TRUST_YES="Yes, I trust"                                                            # the accept option of the folder trust question
PY="$PREMISES_DIR/hud_probe.py"
PYTHON=$(command -v python3)
hp() { python3 -I "$PY" "$@"; }

live_run probe
P=$RUN
T="$P/tmux"
Q="$P/pty"
mkdir -p "$T/log" "$Q/log" "$P/fx" "$P/oc"
hp settings "$PYTHON" "$PY" "$T" 2 "$T/settings.json"
hp settings "$PYTHON" "$PY" "$Q" 2 "$Q/settings.json"
printf 'verbose\n' >"$T/phase"
printf 'verbose\n' >"$Q/phase"
: >"$P/lines.txt"
rm -f "$FX"/prb-1-*

# line <id> <word> <text>: the LIVE line (live.sh result), also kept for the results fixture.
line() { result "$@" | tee -a "$P/lines.txt"; }
# cc_env: the environment words of each Claude Code launch (the login, settings and plugins of the caller).
cc_env() { echo "HOME=$HOME ${PRB_CC_DIR:+CLAUDE_CONFIG_DIR=$PRB_CC_DIR} PATH=$TOOL_PATH TERM=xterm-256color LANG=C.UTF-8 DISABLE_AUTOUPDATER=1"; }
pane() { tmux capture-pane -p -t "=$1:" 2>/dev/null || true; }
shows() { pane "$1" | grep -qF -- "$2"; }
# wait_until <seconds> <command>: poll every 0.5 s; 1 on timeout.
wait_until() {
  _n=$(($1 * 2)); shift
  _i=0
  until "$@"; do _i=$((_i + 1)); [ "$_i" -lt "$_n" ] || return 1; sleep 0.5; done
}
# trust_line <session>: the pane line of the accept option of the folder trust question (empty: no question).
trust_line() { pane "$1" | grep -F "$TRUST_YES" | tail -n 1; }
# prb_trust <session>: one key step on the folder trust question (LIV-1, live run finding D1: Claude Code
# 2.1.330 starts with the pointer on "No, exit", so Enter alone ends the session). The pointer is not on
# the accept line: Down. The pointer is on the accept line in two readings 0.5 s apart: Enter. A key that
# Claude Code did not take is sent again by the next step. Prints the number of 0.5 s polls it used.
prb_trust() {
  case "$(trust_line "$1")" in
    "") echo 0 ;;
    *"$PTR"*)
      sleep 0.5
      case "$(trust_line "$1")" in *"$PTR"*) tmux send-keys -t "=$1:" Enter || true; sleep 1; echo 3 ;; *) echo 1 ;; esac ;;
    *) tmux send-keys -t "=$1:" Down || true; sleep 1; echo 2 ;;
  esac
}
# prb_wait <session> <text>: the text is in the pane within 60 s; answers the folder trust question with
# yes. 1 when it is not; PRB_WHY then says why.
prb_wait() {
  _i=0
  until shows "$1" "$2"; do
    _p=$(pane "$1")
    case "$_p" in
      *prb-exit:*)
        PRB_WHY="claude ended: $(printf '%s\n' "$_p" | grep -E 'prb-exit:' | sed -n 1p | cut -c1-80)"
        return 1 ;;
      *"$TRUST_YES"*) _i=$((_i + $(prb_trust "$1"))) ;;
    esac
    _i=$((_i + 1)); [ "$_i" -lt 120 ] || { PRB_WHY="timeout: no probe status line in the pane within 60 s"; return 1; }
    sleep 0.5
  done
}
# prb_stop <session>: /exit, at most 10 s for the end, then kill the session.
prb_stop() {
  tmux_send "$1" /exit 2>/dev/null || true
  _i=0
  while tmux has-session -t "=$1" 2>/dev/null && ! shows "$1" prb-exit:; do
    _i=$((_i + 1)); [ "$_i" -lt 20 ] || break
    sleep 0.5
  done
  tmux kill-session -t "=$1" 2>/dev/null || true
}
# phase <name>: the phase file that the probe command reads at its start (temporary file, then rename).
phase() { printf '%s\n' "$1" >"$T/phase.tmp" && mv "$T/phase.tmp" "$T/phase"; }
# cap <phase> <seconds> <escapes 0|1>: captures of session t every 0.5 s.
cap() { hp capture "$PRB_SOCK" t "$T" "$1" "$2" "$3" || true; }
# note <text>: a line on stderr; the run goes on.
note() { echo "live-hud-probe: $*" >&2; }

# ---- tmux's own cell widths of the probe glyphs (session w, no agent) ----
tmux_own w
tmux new-session -d -s w -x 40 -y 5 "sleep 60"
hp widths "$PRB_SOCK" w "$P/tw.json"
tmux kill-session -t "=w" 2>/dev/null || true

# ---- session t: Claude Code with the probe status line, 200 x 50, in tmux; no prompt ----
T_WHY=""
tmux_own t
echo "cd $R && env -i $(cc_env) $CLAUDE_BIN --settings $T/settings.json; echo prb-exit:\$?; sleep 3" >"$P/tmux-t.sh"
tmux new-session -d -s t -x 200 -y 50 -c "$R" "/bin/sh $P/tmux-t.sh"
if prb_wait t "$LOGO"; then
  sleep 3
  cap verbose 1 1
  phase compact
  wait_until 10 shows t "$SCISSORS" || note "no compact row within 10 s"
  sleep 1
  cap compact 1 1
  for x in A B C D; do
    phase "wide$x"
    wait_until 10 shows t "P|wide$x probe" || note "no wide$x rows within 10 s"
    sleep 1
    cap "wide$x" 1 0
  done
  # early after the wide phases: a Claude Code that does not tolerate it ends there, after PH3 to PH9.
  phase early
  wait_until 10 shows t "PH11 early" || note "no early row within 10 s"
  cap early 8 0
  phase slow
  cap slow 20 0
  phase done
  wait_until 10 shows t "PROBE DONE" || note "no PROBE DONE row within 10 s"
  cap done 1 0
else
  T_WHY=$PRB_WHY
fi
prb_stop t

# ---- session p: outside tmux, on a pseudo-terminal of 100 columns; no prompt ----
# shellcheck disable=SC2046
python3 -I "$PY" pty "$Q/log" 100 60 "$R" "$(command -v env)" -i $(cc_env) "$CLAUDE_BIN" --settings "$Q/settings.json" &
LIVE_PIDS=$!
P_RC=0
wait "$LIVE_PIDS" || P_RC=$?
LIVE_PIDS=""
case "$P_RC" in
  0) P_WHY="" ;;
  3) P_WHY="pseudo-terminal session: no probe run within 60 s" ;;
  5) P_WHY="pseudo-terminal session: Claude Code showed the folder trust question, which the script does not answer there" ;;
  *) P_WHY="pseudo-terminal session: the driver ended with status $P_RC" ;;
esac

# ---- PH13: OpenCode help and inline config keys (temporary HOME) ----
hp ph13 "$OPENCODE_BIN" "$P/oc" "$P/ph13.json" || true

# ---- analysis and the LIVE lines ----
hp analyze "$T" "$Q" 200 100 "$P/tw.json" "$P/stdin.json" >"$P/an.json" || echo '{}' >"$P/an.json"
j() { jq -r "$1" "$P/an.json"; }
o() { jq -r "$1" "$P/ph13.json" 2>/dev/null || echo error; }
dig() { case "$1" in '' | *[!0-9]*) return 1 ;; esac; }

if [ -n "$T_WHY" ]; then
  line PH1 failed "$T_WHY"
elif [ "$(j .sgr.ok)" = true ]; then
  line PH1 verified "capture-pane -e (tmux $TMUX_VER), the H§6.3 verbose printf bytes: stripe fg 1,3,6, logo bold fg 15, worktree name fg 172 (38;5;n)"
else
  line PH1 failed "16-color part $(j '.sgr.ok16 // false'), 256-color part $(j '.sgr.ok256 // false'): $(jq -c .sgr "$P/an.json")"
fi

if [ -n "$T_WHY" ]; then
  line PH2 failed "$T_WHY"
elif [ "$(j .two.rows)" = 2 ]; then
  line PH2 verified "the two rows of the H§6.3 verbose printf show as two rows (tmux $TMUX_VER, 200 columns)"
else
  line PH2 failed "the H§6.3 verbose printf shows as $(j .two.rows) row(s): $(jq -c .two "$P/an.json")"
fi

C3=$(j .wide.A.class)
T3="a row of 302 cells in a 200-column pane shows $(j '.wide.A.shown // "-"') cells (ruler $(j '.wide.A.ruler // "-"'), ends in …: $(j '.wide.A.ellipsis // "-"'))"
if [ -n "$T_WHY" ]; then
  line PH3 failed "$T_WHY"
elif [ "$C3" = cut ]; then
  line PH3 verified "cut: $T3; the next line does not go on with the row"
elif [ "$C3" = wrapped ]; then
  line PH3 failed "wrapped: $T3; the next line goes on with the row"
else
  line PH3 failed "not observed: $C3: $(j '.wide.A.why // ""')"
fi

TT=$(j .ph4.tty_t) TP=$(j .ph4.tty_p) CT=$(j .ph4.col_t) CP=$(j .ph4.col_p) LOSS=$(j .ph4.loss)
T4="(a) /dev/tty in the command: tmux 200 columns: $TT, pseudo-terminal 100 columns: $TP; (b) COLUMNS: tmux $CT, pseudo-terminal $CP; (c) status area = pane width - $LOSS at 200 columns (indent $(j .ph4.indent), right gap $(j .ph4.gap))"
if [ "$TT" = 200 ] && [ "$TP" = 100 ] && dig "$CT" && dig "$CP" && [ "$LOSS" = 0 ]; then
  line PH4 verified "$T4"
else
  line PH4 failed "${T_WHY:+$T_WHY; }${P_WHY:+$P_WHY; }$T4"
fi

if jq -e 'length > 0 and all(.[]; (.effort | type) as $t | $t == "string" or ($t == "object" and (.effort.level | type) == "string"))' "$P/stdin.json" >/dev/null 2>&1; then
  line PH5 verified "stdin effort is $(jq -c '.[0].effort' "$P/stdin.json") ($(jq -r '.[0].effort | type' "$P/stdin.json")) in every logged run (tmux and pseudo-terminal); stdin keys of the first run: $(jq -r '.[0].keys | join(" ")' "$P/stdin.json")"
else
  line PH5 failed "${T_WHY:+$T_WHY; }stdin effort shapes: $(j '.ph5.efforts | join(" ")') in $(j .ph5.runs) logged run(s)"
fi

# wide_text <X> <glyph text> <glyph count>: the measured cells of one wide row.
wide_text() { echo "Claude Code counts $2 as $(j ".wide.$1.c // \"-\"") cell(s): the row with $3 shows $(j ".wide.$1.diff // \"-\"") fewer ruler cells than the ASCII row ($(j '.wide.A.ruler // "-"') and $(j ".wide.$1.ruler // \"-\""))"; }
if [ -n "$T_WHY" ]; then
  line PH6 failed "$T_WHY"
elif [ "$(j .wide.B.diff)" = 10 ]; then
  line PH6 verified "$(wide_text B U+F0645 '10 glyphs')"
else
  line PH6 failed "$(wide_text B U+F0645 '10 glyphs')$(j '.wide.B.why // "" | if . == "" then "" else "; " + . end')"
fi

T7="$(wide_text C U+2702 '10 glyphs'); tmux $TMUX_VER width $(j '.tw.C // "-"'); U+FE0F after it in the compact row: $(j .compact.vs16); the terminal font's presentation is not checked"
if [ -n "$T_WHY" ]; then
  line PH7 failed "$T_WHY"
elif [ "$(j .wide.C.diff)" = 10 ] && [ "$(j .tw.C)" = 1 ] && [ "$(j .compact.scissors)" = true ] && [ "$(j .compact.vs16)" = false ]; then
  line PH7 verified "$T7"
else
  line PH7 failed "$T7"
fi

T9="$(wide_text D 'each logo letter' '20 letters'); tmux $TMUX_VER width $(j '.tw.D // "-"'); logo and reflection start at column $(j .two.logo_col) and $(j .two.refl_col); the font's glyphs are not checked"
if [ -n "$T_WHY" ]; then
  line PH9 failed "$T_WHY"
elif [ "$(j .wide.D.diff)" = 20 ] && [ "$(j .tw.D)" = 1 ] && [ "$(j .two.rows)" = 2 ] && [ "$(j .two.logo_col)" -ge 0 ] && [ "$(j .two.logo_col)" = "$(j .two.refl_col)" ]; then
  line PH9 verified "$T9"
else
  line PH9 failed "$T9"
fi

if [ -n "$T_WHY" ]; then
  line PH11 failed "$T_WHY"
elif [ "$(j .ph11.shown)" -ge 3 ] && [ "$(j .ph11.lost)" = 0 ] && [ "$(j .ph11.error)" = false ] && [ "$(j .ph11.after)" = true ]; then
  line PH11 verified "a command that closes stdin unread and exits at once: each early run's line showed (3 or more runs), none lost, no error text, and the status line went on after the phase"
else
  line PH11 failed "a command that closes stdin unread and exits at once: lines shown $(j .ph11.shown) of $(j .ph11.runs) runs, lost $(j .ph11.lost), error text $(j .ph11.error), went on after the phase $(j .ph11.after)"
fi

case "$(j .ph12.class)" in
  kills) T12="Claude Code ends a running command when the next refresh comes" ;;
  waits) T12="Claude Code waits for a running command before it starts the next" ;;
  overlaps) T12="Claude Code starts the next command while the first still runs" ;;
  *) T12="" ;;
esac
SIGS=$(j '.ph12.signals | if length == 0 then "none" else join(",") end')
if [ -n "$T_WHY" ]; then
  line PH12 failed "$T_WHY"
elif [ -n "$T12" ] && [ "$(j .ph12.starts)" -ge 2 ]; then
  line PH12 observed "refreshInterval 2 s, a command that sleeps 5 s: $T12; signals seen: $SIGS; at most $(j .ph12.max_concurrent) command(s) at a time; slow line shown: $(j .ph12.shown)"
else
  line PH12 failed "not observed: $(j .ph12.starts) slow run(s) started in 20 s"
fi

LIVE_VER=$LIVE_OC_VER
K13="statusLine $(o .keys.statusLine), statusline $(o .keys.statusline), status_line $(o .keys.status_line); control: username $(o .control.username), zygoProbeUnknownKey $(o .control.zygoProbeUnknownKey)"
if [ "$(o '.help_hits | length')" = 0 ] && [ "$(o .control.username)" = kept ] && [ "$(o .control.zygoProbeUnknownKey)" = dropped ] &&
  [ "$(o '[.keys[] | select(. != "dropped")] | length')" = 0 ]; then
  line PH13 verified "OpenCode $LIVE_OC_VER: no status line in --help or debug --help; debug config --pure drops the inline keys: $K13"
else
  line PH13 failed "OpenCode $LIVE_OC_VER: help matches: $(o '.help_hits | join("; ")'); inline keys: $K13"
fi
LIVE_VER=$LIVE_CC_VER

# ---- fixtures (RUN is the probe run, so scrub maps its paths) ----
[ "$(grep -c '^LIVE PH' "$P/lines.txt")" = 11 ] || { echo "live: $(grep -c '^LIVE PH' "$P/lines.txt") LIVE lines, not 11"; exit 1; }
hp fixtures "$T" "$Q" "$P/an.json" "$P/ph13.json" "$P/fx" || true
python3 -I -c 'import json, sys; json.dump(dict(zip(["date", "claude_code", "opencode", "tmux", "os"], sys.argv[1:]), pane="200x50", pty="100x50", refresh_interval=2), open(sys.argv[6], "w"), indent=1)' \
  "$(date +%Y-%m-%d)" "$LIVE_CC_VER" "$LIVE_OC_VER" "$TMUX_VER" "$(uname -s)" "$P/fx/run.json"
cp "$P/lines.txt" "$P/fx/results.txt"
for f in "$P"/fx/*; do
  if [ -s "$f" ]; then scrub "$f" >"$FX/prb-1-${f##*/}"; fi
done
live_done "$FX"
````

### Block D: `scripts/premises/live-hud-probe_test.sh`

````sh
#!/bin/sh
# Offline test of scripts/premises/live-hud-probe.sh (node PRB-1): the script runs in a copied tree with
# stub claude and opencode programs (a stub Claude Code that runs the --settings statusLine command each
# refreshInterval and draws it, with the folder trust question first), a temporary HOME and a private
# TMUX_TMPDIR. No agent, no network, no login. Not run by CI.
# Usage: sh scripts/premises/live-hud-probe_test.sh   ->  "live-hud-probe tests: ok (<n>)", exit 0
set -u
unset TMUX TMUX_PANE CLAUDECODE CLAUDE_CONFIG_DIR N2_SCRATCH N2_SCRATCH_OWN
HERE=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$HERE/../.." && pwd)
T=$(mktemp -d)
export TMUX_TMPDIR="$T/tmux"
mkdir -p "$TMUX_TMPDIR"
cleanup() {
  for s in "$TMUX_TMPDIR"/tmux-*/*; do [ -S "$s" ] && tmux -S "$s" kill-server 2>/dev/null; done
  pkill -f "$T/" 2>/dev/null
  chmod -R u+rwx "$T" 2>/dev/null
  rm -rf "$T"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
n=0 bad=0
ok() { n=$((n + 1)); printf 'ok: %s\n' "$1"; }
no() { n=$((n + 1)); bad=$((bad + 1)); printf 'FAIL: %s (%s)\n' "$1" "$2"; }

# The copied tree: the premise scripts, docs/premises.md (the pins) and an empty fixture folder.
C="$T/repo"
mkdir -p "$C/scripts" "$C/docs" "$C/fixtures/claude-code/hud" "$T/bin" "$T/home/.claude"
cp -R "$REPO/scripts/premises" "$C/scripts/"
cp "$REPO/docs/premises.md" "$C/docs/"
printf '{"model": "x"}\n' >"$T/home/.claude/settings.json"
SUM=$(cksum <"$T/home/.claude/settings.json")

cat >"$T/bin/claude" <<'PY'
#!/usr/bin/env python3
# Stub Claude Code: --version; else the folder trust question (pointer on "No, exit") until a marker file
# exists, then the statusLine command of --settings each refreshInterval: stdin JSON, COLUMNS, a new
# session (no controlling terminal); a run still going at the next refresh gets SIGTERM. Draws the
# output 2 cells from the left and cuts a row at width - 4 with an ellipsis. "/exit" Enter ends it.
import json, os, select, signal, subprocess, sys, termios, time, tty, unicodedata
a = sys.argv[1:]
if a == ["--version"]:
    print(os.environ.get("STUB_CC_VER", "2.1.330") + " (Claude Code)")
    sys.exit(0)
s = json.load(open(a[a.index("--settings") + 1]))["statusLine"]
cmd, every = s["command"], s.get("refreshInterval", 2)
fd = sys.stdin.fileno()
old = termios.tcgetattr(fd)
tty.setraw(fd)
out = sys.stdout


def width():
    return os.get_terminal_size(out.fileno()).columns


def cells(t):
    return sum(2 if unicodedata.east_asian_width(c) in "WF" else 1 for c in t)


def cut(row, w):
    if "\x1b" in row or cells(row) <= w:
        return row
    r = ""
    for c in row:
        if cells(r + c) > w - 1:
            break
        r += c
    return r + "…"


def draw(lines):
    out.write("\x1b[H\x1b[2J" + "\r\n".join(lines) + "\r\n")
    out.flush()


def keys(timeout):
    if select.select([fd], [], [], timeout)[0]:
        return os.read(fd, 1024).decode("utf-8", "replace")
    return ""


mark = os.path.join(os.environ["HOME"], ".stub-trusted")
try:
    if not os.path.exists(mark):
        at = 0
        while True:
            draw([" Quick safety check", "", (" ❯ " if at == 0 else "   ") + "No, exit",
                  (" ❯ " if at == 1 else "   ") + "Yes, I trust this folder"])
            k = keys(5)
            if "\x1b[B" in k:
                at = 1
            elif "\x1b[A" in k:
                at = 0
            elif "\r" in k:
                if at == 0:
                    draw(["Claude Code didn't start because this folder isn't trusted."])
                    sys.exit(1)
                open(mark, "w").close()
                break
    typed, rows, prev, nxt = "", [], None, 0.0
    while True:
        if time.monotonic() >= nxt:
            nxt = time.monotonic() + every
            if prev is not None and prev.poll() is None:
                os.killpg(prev.pid, signal.SIGTERM)
            env = dict(os.environ, COLUMNS=str(width()))
            prev = subprocess.Popen(cmd, shell=True, stdin=subprocess.PIPE, stdout=subprocess.PIPE, env=env,
                                    start_new_session=True)
            try:
                prev.stdin.write(json.dumps({"session_id": "stub", "effort": {"level": "medium"}, "model": {"id": "m"}}).encode())
                prev.stdin.close()
            except BrokenPipeError:
                pass
            try:
                o, _ = prev.communicate(timeout=every * 0.8)
                rows = o.decode("utf-8", "replace").rstrip("\n").split("\n")
            except subprocess.TimeoutExpired:
                pass
        draw(["> " + typed, ""] + ["  " + cut(r, width() - 4) for r in rows] + ["  ? for shortcuts"])
        k = keys(0.2)
        for c in k:
            if c == "\r":
                if typed == "/exit":
                    sys.exit(0)
                typed = ""
            elif c.isprintable():
                typed += c
finally:
    termios.tcsetattr(fd, termios.TCSADRAIN, old)
PY
cat >"$T/bin/opencode" <<'PY'
#!/usr/bin/env python3
import json, os, sys
a = sys.argv[1:]
if a in (["--version"], ["-v"]):
    print("1.19.6")
elif a and a[-1] == "--help":
    print("Commands:\n  opencode run [message..]   run opencode with a message")
elif a[:2] == ["debug", "config"]:
    c = json.loads(os.environ.get("OPENCODE_CONFIG_CONTENT", "{}"))
    d = {"agent": {}, "mode": {}, "plugin": [], "command": {}, "username": "unknown"}
    d.update({k: v for k, v in c.items() if k in ("username", "model")})
    print(json.dumps(d))
PY
chmod +x "$T/bin/claude" "$T/bin/opencode"

# run <out file> [NAME=value ...]: the script in the copied tree, stubs first on PATH, no N2_SCRATCH (lib.sh
# makes its own scratch dir under TMPDIR = $T/tmp: removed after exit 0, kept after an error); sets rc.
run() {
  _o=$1; shift
  env HOME="$T/home" PATH="$T/bin:$PATH" TMPDIR="$T/tmp" "$@" sh "$C/scripts/premises/live-hud-probe.sh" >"$_o" 2>&1
  rc=$?
}
# clean_after <case> <exit status>: no private tmux server or socket, no live-<pid> tree, no stub process,
# no scratch dir after exit 0; the settings file of the fake HOME unchanged.
clean_after() {
  _l=$(ls "$TMUX_TMPDIR"/tmux-*/ 2>/dev/null | grep -c zygo-prb- || true)
  _s=$(find "$T/tmp" -mindepth 2 -maxdepth 2 -name 'live-*' | grep -c . || true)
  _d=$(find "$T/tmp" -mindepth 1 -maxdepth 1 | grep -c . || true)
  [ "$2" -ne 0 ] && _d=0
  _p=$(pgrep -f "$T/bin/claude" | grep -c . || true)
  if [ "$_l" = 0 ] && [ "$_s" = 0 ] && [ "$_d" = 0 ] && [ "$_p" = 0 ] && [ "$(cksum <"$T/home/.claude/settings.json")" = "$SUM" ]; then ok "$1 leaves nothing"
  else no "$1 leaves nothing" "sockets $_l, live trees $_s, scratch dirs $_d, stub processes $_p"; fi
  rm -rf "$T/tmp"; mkdir -p "$T/tmp"
}
mkdir -p "$T/tmp"

# L1: the full run with the stubs: eleven lines, the stub's expected results, the fixtures.
run "$T/l1.out"
if [ "$rc" -eq 0 ] && [ "$(grep -c '^LIVE PH' "$T/l1.out")" = 11 ]; then ok "L1 exit 0 and eleven LIVE lines"
else no "L1 exit 0 and eleven LIVE lines" "exit $rc: $(tail -n 5 "$T/l1.out" | tr '\n' '|')"; fi
want="PH1 verified|PH2 verified|PH3 verified|PH4 failed|PH5 verified|PH6 verified|PH7 verified|PH9 verified|PH11 verified|PH12 observed|PH13 verified"
got=$(sed -n 's/^LIVE \(PH[0-9]*\) \([a-z]*\) .*/\1 \2/p' "$T/l1.out" | tr '\n' '|' | sed 's/|$//')
if [ "$got" = "$want" ]; then ok "L2 results with the stub Claude Code"; else no "L2 results with the stub Claude Code" "$got"; fi
if grep -q 'LIVE PH4 failed .*tmux 200 columns: ENXIO.*COLUMNS: tmux 200, pseudo-terminal 100.*pane width - 4 at 200 columns (indent 2, right gap 2)' "$T/l1.out"; then ok "L3 PH4 text"
else no "L3 PH4 text" "$(grep 'LIVE PH4' "$T/l1.out")"; fi
if grep -q 'LIVE PH12 observed .*ends a running command when the next refresh comes; signals seen: TERM' "$T/l1.out"; then ok "L4 PH12 text"
else no "L4 PH12 text" "$(grep 'LIVE PH12' "$T/l1.out")"; fi
fx="analysis.json compact.txt early.txt env-pty.json env-tmux.json events.json ph13.json results.txt run.json slow.txt stdin-pty.json stdin-tmux.json verbose.txt wide.txt"
have=$(cd "$C/fixtures/claude-code/hud" && ls | sed 's/^prb-1-//' | tr '\n' ' ' | sed 's/ $//')
want_fx=$(printf '%s\n' $fx | grep -v '^slow.txt$' | tr '\n' ' ' | sed 's/ $//')
if [ "$have" = "$want_fx" ] || [ "$have" = "$(printf '%s\n' $fx | tr '\n' ' ' | sed 's/ $//')" ]; then ok "L5 fixture files"
else no "L5 fixture files" "$have"; fi
if grep -rqF -e "$T" -e "\"pid\"" "$C/fixtures/claude-code/hud"; then no "L6 no run path and no pid in a fixture" "found"
else ok "L6 no run path and no pid in a fixture"; fi
clean_after L1 "$rc"

# L7: Claude Code below the minimum: exit 1 before any tmux server starts.
run "$T/l7.out" STUB_CC_VER=2.1.327
if [ "$rc" -eq 1 ] && grep -q 'the minimum is 2.1.328' "$T/l7.out"; then ok "L7 claude below the minimum"
else no "L7 claude below the minimum" "exit $rc: $(tail -n 2 "$T/l7.out" | tr '\n' '|')"; fi
clean_after L7 "$rc"

# L8: TERM during session t: exit 143; the traps end the tmux server and remove the scratch tree.
rm -f "$T/home/.stub-trusted"
env HOME="$T/home" PATH="$T/bin:$PATH" TMPDIR="$T/tmp" sh "$C/scripts/premises/live-hud-probe.sh" >"$T/l8.out" 2>&1 &
pid=$!
i=0
until pgrep -f "$T/bin/claude --settings" >/dev/null || [ "$i" -ge 40 ]; do i=$((i + 1)); sleep 0.5; done
sleep 2
kill -TERM "$pid"
wait "$pid"
rc=$?
if [ "$rc" -eq 143 ]; then ok "L8 TERM exits 143"; else no "L8 TERM exits 143" "exit $rc"; fi
sleep 1
clean_after L8 "$rc"

# L9: a blank in the scratch path: exit 1 with the path line.
mkdir -p "$T/a b"
env HOME="$T/home" PATH="$T/bin:$PATH" N2_SCRATCH="$T/a b" sh "$C/scripts/premises/live-hud-probe.sh" >"$T/l9.out" 2>&1
rc=$?
if [ "$rc" -eq 1 ] && grep -q 'holds a blank or a quote' "$T/l9.out"; then ok "L9 blank in a path"
else no "L9 blank in a path" "exit $rc: $(tail -n 2 "$T/l9.out" | tr '\n' '|')"; fi

if [ "$bad" -eq 0 ]; then echo "live-hud-probe tests: ok ($n)"; exit 0; fi
echo "live-hud-probe tests: $bad failed"
exit 1
````

### Block E: the L case of `scripts/premises/ver_test.sh`

````sh
# --- L (node PRB-1): a LIVE line names the agent version that pinned_agents found, never the pin ----
# The fallback of each LIVE_VER site in the live scripts is LIVE_CC_VER or LIVE_OC_VER (both set by
# pinned_agents), not PIN_CC or PIN_OC (a minimum, operator 2026-11-21).
lhits=""
for f in "$HERE"/live-*.sh; do
  if grep -qE ':-\$PIN_(CC|OC)\}|LIVE_VER=\$PIN_(CC|OC)([^A-Za-z0-9_]|$)' "$f"; then lhits="$lhits ${f##*/}"; fi
done
if [ -z "$lhits" ]; then ok "L1 no LIVE_VER site falls back to PIN_CC or PIN_OC"
else no "L1 no LIVE_VER site falls back to PIN_CC or PIN_OC" "in:$lhits"; fi
````
