# LIV-1 — Live check P-HUD-1 on Linux or macOS

**Spec:** `docs/specs/2026-11-21-statusline-hud-design.md` H§12 (P-HUD-1), H§17 note 4. Needs the operator's machine (live login); Linux or macOS. Builds the binary with `cargo build -p zygo --features test-hooks` to use the HUD-9 seams (stdin/environment dump, width-source log, fetch delay). Windows parts are LIV-2. The executor stops and asks the operator to run this node on their machine
**Files:** `scripts/premises/live-hud.sh`, `docs/live-checks.md`, `fixtures/claude-code/hud/`, `docs/specs/2026-11-21-statusline-hud-design.md`
**Consumes:** `ZYGO_TEST_HUD_DUMP` (HUD-9), `ZYGO_TEST_FETCH_DELAY_MS` (HUD-9), `ZYGO_TEST_REFRESH_INTERVAL` (HUD-9)
**Produces:** —
**Acceptance:** `cargo build -p zygo --features test-hooks && sh scripts/premises/live-hud.sh --zygo target/debug/zygo && cargo test -p zygo --test forbidden_content` → exit 0 with `LIVE` lines for P-HUD-1 (H§14 #37); scrubbed fixtures; dated rows in `docs/live-checks.md` and dated notes in H§12. A failed P-HUD-1 line changes the contract of each node it names: PH1 (colors) → HUD-8, HUD-9, HUD-10, H2, LIV-2; PH2 (two rows) → HUD-8, HUD-11, H2; PH3 (cut or wrap at 70 columns) → HUD-8; PH4 (width source, status area width) → HUD-8; PH5 (stdin and environment dump) → HUD-9; PH9 (logo and reflection column) → HUD-8, H2; PH12 (cancellation, observation) → HUD-9; C7 (HUD text in a recorded upstream request) → HUD-9, the hook entry in `cmd/hook.rs` that is the only writer of HUD text. For a DONE node the failure creates a new node (execute-work §5.4) before DOC-1 runs; a TODO node (H2, LIV-2) is re-planned before it starts

## Plan

Work in the node's checkout. Run each command from the repository root. Paths are relative to it. Each commit message ends, after one empty line, with the line `Zygo-Session: <the value of $ZYGO_SESSION in your shell>` (leave this line out when the variable is empty) and then the line `Zygo-Work: LIV-1` as the last line. In each commit, tick (`- [x]`) the steps of this file that are done. Change only the four paths of the contract and this file. Do not edit `graph.md`, another node file, `lib.sh`, `live.sh` or code.

Tasks 1 and 2 start no agent. Task 3 is the live run: it uses the operator's claude.ai login through `zygo claude --passthrough` (operator, 2026-11-21: "you can test with passthrough"; no API key, no configured provider). No step needs a person at the keyboard.

**Builds on:** `ZYGO_TEST_HUD_DUMP`, `ZYGO_TEST_FETCH_DELAY_MS`, `ZYGO_TEST_REFRESH_INTERVAL` (HUD-9); `ver_ge`, `agent_ver`, `agent_ok`, `agent_show`, `n2_scratch_end` (PRB-2); the `statusline` style of user config (HUD-3, HUD-7, HUD-11).
**Plan produces:** `scripts/premises/live-hud.sh`; the fixtures `fixtures/claude-code/hud/p-hud-1-*`; eight rows in `docs/live-checks.md`; eight dated notes and one change-log row in the HUD spec.

### Facts of the live code and scripts (read 2026-11-21)

Framework (the script uses only these helpers; it adds none to `lib.sh` or `live.sh`):

- `scripts/premises/lib.sh`: `set -eu` (`:7`); `REPO_DIR` (`:9`); `CLAUDE_BIN` (`:11`); `OPENCODE_BIN`, with `exit 1` and `premises: opencode is not on PATH` when `opencode` is absent (`:12`); `N2_SCRATCH`, made by `mktemp -d` when unset (`:33-44`); `TOOL_PATH` (`:46`); `new_run <name>` sets `RUN`, `H`, `R` and makes `R` a git repository with one commit (`:49-67`); `start_mock` (`:70`), `stop_mock` (`:80`); `scrub <file>` = `python3 -I scripts/premises/scrub.py "$RUN" <file>` (`:123-125`); `tmux_own <session>` (`:131-136`); `tmux_send <session> <text>` (`:148`).
- `scripts/premises/live.sh`: `ZYGO_BIN` keeps a value that is already set (`:6`); `PIN_CC` (`:8`); `LIVE_PIDS` (`:14`); `live_end <status>` kills `MOCK_PID`, `WEB_PID`, `$LIVE_PIDS` and the sessions of `$LIVE_TMUX`, removes `$N2_SCRATCH/live-$$`, then calls `n2_scratch_end` (`:23-43`), which removes a scratch dir that `lib.sh` made after status 0 and keeps it otherwise (`lib.sh:15-26`); `live_traps` (`:47-54`); `ver_ge` from `ver.sh` (`:5`); `agent_ver` (`:58`), `agent_ok` (`:65`), `agent_show` (`:74`); `pinned_agents` also needs the OpenCode version (`:78-89`), so the script does not call it; `live_run <name>` = `new_run "live-$$/<name>"` (`:99`); `tmux_zygo <session> <args>` starts zygo in a 200 x 50 detached session with a temporary `HOME` and `$LIVE_VARS` (`:140-149`); `tmux_wait_text` (`:152`); `mock_config <file>` (`:161`); `body_has <regex> <json files>` (`:211`); `result <id> <word> <text>` prints `LIVE <id> <word> (agent <version>): <text>` and appends it to `$N2_SCRATCH/results.txt` (`:226-229`); `live_done <fixture dir>` (`:232`).
- `scripts/premises/live-passthrough.sh:160-165` is the one passthrough run of today: real `HOME`, `ZYGO_CONFIG` = an empty file of the run, `DISABLE_AUTOUPDATER=1`, cwd = the run's repository, `"$ZYGO_BIN" claude --passthrough -- -p "Reply with the single word ready."`. It selects no model. It records no upstream request: it reads usage rows only. A status line shows only in an interactive session, so P-HUD-1 cannot use `-p`.
- `scripts/premises/live-claude.sh:124-143` (`part_n28`, P-N28-1) is the model for C7: mock upstream, `tmux_zygo n28 claude --compact 777k`, one typed prompt, then `body_has` over `$RUN/rec/[0-9]*.json`.
- `scripts/check-premises.sh:125-128` scans `fixtures`, `scripts/premises/live*.sh` and `docs/live-checks.md` for forbidden content (`:142-159`), for the names of `N2_FORBIDDEN_NAMES` (`:172`) and for the host names (`:183-192`). It does not read the P-HUD-1 rows and it does not read the HUD spec. So it covers three of the four paths of this node for content only.
- `tests/forbidden_content.rs` (the test target `forbidden_content` of `crates/zygo/Cargo.toml`) scans every tracked and every new file of the checkout, line by line: a letter run of 3 to 40 letters whose SHA-256 is in its list of 22 digests (the words are not readable), and credential forms (`sk-ant-…`, `sk-…` of 32 or more characters, `AKIA…`, `gh?_…`, `xox?-…`, private-key headers, and the built-in secret patterns: JWT, zygo token, and others). The new script, fixtures and docs must hold none of them.
- `scripts/premises/live-selftest.sh` and `lib_test.sh` have no list of live scripts. `live-selftest.sh:150` and `:237-238` copy `scripts/premises` and `fixtures` into a temporary tree and run `check-premises.sh` there, so the new files are scanned there too. `run-all.sh:13` runs the N2 experiments only. Nothing must be added to those three files.

HUD seams (all need `cargo build -p zygo --features test-hooks`; `crates/zygo/Cargo.toml:8`):

- `ZYGO_TEST_HUD_DUMP=<file>`: each run of `zygo hook statusline` writes `{"stdin_keys": [...], "effort": <the stdin value>, "env": {<name>: <value or null>}}` to `<file>`, atomically, right after the bounded stdin read (`crates/zygo-hooks/src/statusline/data.rs:451`, `:475-488`, `:578-580`). The `env` names are `ZYGO_SESSION_DIR`, `ZYGO_WORK`, `ZYGO_URL`, `NO_COLOR`, `ZYGO_NERD_FONT`, `TERM`, `COLORTERM`, `WT_SESSION`, `TERM_PROGRAM`, `COLUMNS` (`data.rs:462-473`).
- `ZYGO_TEST_FETCH_DELAY_MS=<n>`: the status fetch sleeps `<n>` ms before it starts (`data.rs:447`, `:634-636`). With 5000 and no cache, the run waits for the fetch to its bound and lives about 3 s (`BOUND` = 3 s, `crates/zygo-hooks/src/statusline/mod.rs:23`). The result is `offline`, and the next fetch is 30 s later (`data.rs:654-657`), so there is one slow run in each 30 s.
- `ZYGO_TEST_REFRESH_INTERVAL=<seconds>`: read from the launch environment; it replaces `refreshInterval: 30` in the settings that zygo gives Claude Code (`crates/zygo-launch/src/claude_code.rs:237-244`, `:288`).
- Not a seam, always on: each run appends `statusline: width source <tty|COLUMNS|default>, width <n>` to `<session dir>/session.log` when the pair changes (`data.rs:704-735`; called at `crates/zygo/src/cmd/hook.rs:51-52`). zygo removes the session directory at exit, so the script copies the log while the session lives.

HUD facts that the procedures use:

- Styles: `compact` (default) and `verbose`; the launch reads `[statusline] style` from the user config, which is the `ZYGO_CONFIG` file (`crates/zygo-config/src/schema.rs:631-643`; H§8).
- Width: `/dev/tty` `TIOCGWINSZ`, else `COLUMNS`, else 120; a value outside 20 to 1000 is skipped (`crates/zygo-hooks/src/statusline/width.rs:8-18`). The HUD fits its row to that width (H§6.7).
- Icon set: `ZYGO_NERD_FONT=1` forces the Nerd set (`render.rs:129-138`); only that set has the logo `𝘇𝘆𝗴𝗼` (U+1D607 U+1D606 U+1D5F4 U+1D5FC) and the reflection `zʎƃo` (H§6.5). Colors: 256 colors when `TERM` holds `256color` (`render.rs:142-150`). The one `38;5;n` color is the worktree name, `38;5;172`, and it shows only in a linked git worktree (H§6.2 row 4).
- The status line command is `'<path of the zygo binary>' hook statusline`; the path is `std::env::current_exe()` (`claude_code.rs:38`, `:288`).
- The launcher gives the child every variable of its own environment except the deny list (`crates/zygo-launch/src/env.rs:22-35`), so the `ZYGO_TEST_*` variables reach Claude Code and its status line command.
- The update check at start is skipped with `ZYGO_NO_UPDATE_CHECK` set (`crates/zygo/src/cmd/update.rs:305-308`).
- In passthrough, zygo refuses `--bare` and a `--model` that names a role, and it stops with `…; zygo cannot launch with it` when a settings file of Claude Code sets a credential key or `apiKeyHelper` (`claude_code.rs:396-430`, `launcher.rs:565-568`).
- `docs/live-checks.md:11`: a debug build can answer the first main request with 503 (the 200 ms redaction deadline); Claude Code sends it again. The acceptance uses `target/debug/zygo`, so the script's waits allow for one retry.
- `fixtures/claude-code/live/p-n28-1-pane.txt:17`: Claude Code 2.1.328 draws the status line 2 cells from the left edge of the pane.

This machine (2026-11-21): Linux, `tmux 3.7c`, `claude --version` = `2.1.330 (Claude Code)` (the minimum in `docs/premises.md:5` is 2.1.328), `python3` 3.14, `jq`, `git`, `ps`. OpenCode 1.19.6 is installed in `$HOME/.opencode/bin`, which is not on the default `PATH`. `dash` is not installed (`/bin/sh` is bash). `CLAUDE_CONFIG_DIR` is set in the executor's session, and not to `~/.claude`.

### Findings for the controller

- **B1 (the caller supplies OpenCode).** `scripts/premises/lib.sh:12` needs `opencode` on `PATH` for every script that sources it. The script adds no directory to `PATH`; each step that runs it puts `$HOME/.opencode/bin` first on `PATH` (step 2.1).
- **F2 (C7 is not a passthrough part).** A passthrough request goes from zygo to the vendor over TLS, and zygo has no seam that records it (`crates/zygo-proxy/src/passthrough.rs` has none). H§12 says "as P-N28-1", and P-N28-1 records at the mock. So the C7 part runs on the mock upstream with a temporary `HOME`, as `live-claude.sh:124-143`. It sends no model request.
- **F3 (PH3 at 70 columns).** In a 70-column pane the hook reads 70 from `/dev/tty` and fits its row to 70 cells, so no row is wider than the pane there. The script captures the 70-column pane, as H§12 says, and then makes the pane 19 columns wide. 19 is below the 20-column limit, so the terminal source is skipped and the hook prints a row for another width (120 when `COLUMNS` does not answer). That row is wider than the area, and the pane shows cut or wrap. The same capture gives the status area width of PH4.
- **F4 (PH4 will probably print `failed`).** PH4 has three parts: the terminal source answers, `COLUMNS` is exported, and the status area is the full width. Claude Code 2.1.328 indents the status line by 2 cells, and a shell does not export `COLUMNS`. The line text names each part, so the controller can see that only the "subtract the difference" fallback of H§12 applies.
- **F5 (PH9, font).** P-HUD-1 compares columns in the tmux grid. It cannot see whether the font of a terminal has the logo glyphs; that needs a person. The line text says so.
- **F6 (which login).** `live-passthrough.sh` drops `CLAUDE_CONFIG_DIR`. This script keeps the caller's `CLAUDE_CONFIG_DIR` when it is set, because the executor's session runs on that login. It is an author's call.
- **F7 (requests that the script cannot bound).** The operator's own plugins, hooks and instruction files load in the passthrough sessions, as in `live-passthrough.sh`. They can add model requests to the one turn.

### Design

One script, `scripts/premises/live-hud.sh`. Its full text is Block A at the end of this plan. The author ran Block A on 2026-11-21 in a copied tree with stub `claude` and `opencode` programs and a fake zygo (no agent, no network): `sh -n`, `--selfcheck`, the private tmux server, the trust answer, the two resizes, the hand runs, the pseudo-terminal run, the traps and the fixture files worked. Not run there: the real Claude Code, the real zygo, the process samples of PH12 and the requests of C7.

Structure, in this order:

1. `hud_py <dir>` writes three Python helpers: `hud_an.py` (the analyzer: `sgr`, `two`, `fit`, `ph12`, `expect`, `rows`), `hud_poll.py` (process samples) and `hud_pty.py` (the pseudo-terminal driver).
2. `--selfcheck`: before `lib.sh` is sourced, so it needs no agent program. It runs the analyzer on built-in samples and prints `live-hud selfcheck: ok (17)`.
3. `--zygo <path>`: checks the file and the tools `tmux`, `python3`, `jq`, `git`, `ps`; makes the path real (`os.path.realpath`); keeps `CLAUDE_CONFIG_DIR`; unsets `TMUX` and `TMUX_PANE`.
4. Sources `lib.sh`, then `live.sh`; calls `live_traps`.
5. Defines the shell function `tmux` = `command tmux -u -L zygo-hud-$$ -f /dev/null "$@"`. So `tmux_own`, `tmux_send`, `tmux_wait_text`, `tmux_zygo` and `live_end` all use the private server, with no tmux config file. No global option or hook is set.
6. Replaces the five traps with `hud_end <status>`: `tmux -L zygo-hud-$$ kill-server` (this ends every agent session of the script; zygo gets `SIGHUP` and removes its session directory), then it removes the socket file of that server, then `live_end <status>`.
7. Preconditions, each `exit 1` with a `live: …` line before any agent starts: `claude` at or above the minimum (`agent_ver`, `agent_ok`, `agent_show`); tmux 3.2 or later (`ver_ge`); no blank or quote in `HOME`, `CLAUDE_CONFIG_DIR`, `TOOL_PATH`, the scratch dir or the zygo path; the binary has the seams (one hand run of `zygo hook statusline` with `ZYGO_TEST_HUD_DUMP` writes the file).
8. Five sessions, then eight `LIVE` lines through `result`, then the fixtures through `scrub`, then `live_done`.

Scratch layout (all under `$N2_SCRATCH/live-$$`, removed by `live_end` on every exit path):

- `hud/` (`live_run hud`): `repo/` (the main repository of `new_run`), `wt/hud-work/` (a linked worktree on branch `hud-b`: the cwd of every passthrough session, so the HUD shows the worktree name in `38;5;172`), `zygo.toml` (empty: compact), `zygo-verbose.toml` (`[statusline]` `style = "verbose"`), `py/` (the helpers), `tmux-<session>.sh`, `dump-<session>.json`, `pane-*`, `session-*.log`, `expect-*.txt`, `events.json`, `fx-*`, `lines.txt`, `run.json`. `home/` is not used: the passthrough sessions run on the real `HOME`.
- `c7/` (`live_run c7`): `home/`, `repo/`, `rec/` (the mock's records), `zygo.toml`, `pane.txt`.

The command line of every passthrough session (sessions a, b, c in tmux; d on a pseudo-terminal), with cwd = `…/hud/wt/hud-work`:

```sh
env -i HOME=$HOME [CLAUDE_CONFIG_DIR=<the caller's value, when set>] PATH=$TOOL_PATH TERM=xterm-256color LANG=C.UTF-8 \
  DISABLE_AUTOUPDATER=1 ZYGO_NO_UPDATE_CHECK=1 ZYGO_CONFIG=<…/hud/zygo.toml or zygo-verbose.toml> ZYGO_NERD_FONT=1 \
  ZYGO_TEST_HUD_DUMP=<…/hud/dump-<session>.json> [ZYGO_TEST_REFRESH_INTERVAL=2] [ZYGO_TEST_FETCH_DELAY_MS=5000] \
  <zygo> claude --passthrough
```

No `--yolo`, no `--model` (as `live-passthrough.sh`; the login's default model also keeps the stdin `effort` that PH5 reads), no agent argument. In tmux the line is in `tmux-<session>.sh`, followed by `; echo hud-exit:$?; sleep 3`, and runs as `tmux new-session -d -s <session> -x <columns> -y 50 -c <worktree> "/bin/sh …/tmux-<session>.sh"`.

How a session is driven with no person:

- Start: `hud_wait <session>` polls `capture-pane -p` every 0.5 s for the logo, 60 s at most. When the pane holds `Yes, I trust` (the accept option of the folder trust question of Claude Code for the new worktree) and no logo, `hud_trust` does one key step in each poll: when the pointer `❯` is not on the accept line it sends `Down`; when two readings of the pane, 0.5 s apart, show the pointer on the accept line it sends `Enter`. (Claude Code 2.1.330 starts with the pointer on `No, exit`; see "Live run findings" D1.) When the pane holds `hud-exit:`, zygo ended: the reason is the first `zygo: ` line, scrubbed. On timeout the reason is `timeout: no HUD row in the pane within 60 s`. Each id of that session then prints `failed` with the reason, and the script goes on.
- Prompt: `tmux_send a "Reply with the single word ready. Use no tool."` (session a only).
- End: `hud_stop <session>` sends `/exit`, waits 10 s at most, then `kill-session`.

| Session | Style, size | Seams | Model turns | What the script does | Ids |
|---|---|---|---|---|---|
| a | compact, 200 x 50 | dump, refresh 2 | 1 | Prompt; wait 120 s at most for `ctx <digit>` in the HUD row (on timeout: a stderr note, go on); `capture-pane -e -p` → `pane-a-200.ans`; copy the dump; read the session directory from the dump; `resize-window -x 70`; wait 20 s at most for a line `width source <any>, width 70` in `session.log`; capture → `pane-a-70.txt`; count the width lines; `resize-window -x 19`; wait 20 s at most for one more width line (it names the source that answers at 19 columns); capture → `pane-a-19.txt`; copy `session.log`; run the hook by hand, with no terminal, for the expected row at each width (`hud_an expect`) | PH1, PH3, PH4, PH5 |
| b | verbose, 200 x 50 | dump | 0 | `capture-pane -e -p` → `pane-b-200.ans` | PH2, PH9 |
| c | compact, 200 x 50 | dump, refresh 2, fetch delay 5000 | 0 | `hud_poll.py` samples every 0.05 s for 45 s, from before the session starts: `ps -axo pid=,args=` (each process whose arguments start with `<zygo> hook statusline`) and the dump file of the session (each run of the hook replaces it once, so each new file is one run; `ps` seldom sees a run of a few ms) | PH12 |
| d | compact, pseudo-terminal 100 x 50, outside tmux | dump | 0 | `hud_pty.py` (`pty.fork`, `TIOCSWINSZ`): waits 60 s at most for the dump file, copies `session.log`, writes `/exit`, waits 10 s, then `SIGHUP` and `SIGKILL` to the session's process group. It does not answer the trust question (session a did, for the same path): when the question shows, it ends the session with status 5 and the PH4 line names it | PH4 |
| c7 | verbose, 200 x 50, mock upstream, temporary `HOME` | none | 0 (the mock answers) | `start_mock`; `mock_config`; `tmux_zygo c7 claude --compact 777k` with `LIVE_VARS="ZYGO_NERD_FONT=1 ZYGO_NO_UPDATE_CHECK=1"`; `tmux_wait_text c7 <logo> 60`; `tmux_send c7 "tracer-hud say hi"`; wait 90 s at most for a recorded request with `tracer-hud`; capture; `stop_mock` | C7 |

Expected model requests on the login: 1 main-loop request (the one turn of session a). Claude Code also sends its own small start and title requests in the four passthrough sessions; the script does not control them (see F7). The C7 part sends none.

Each premise of P-HUD-1 (H§12 puts PH1, PH2, PH3, PH4, PH5, PH9, PH12 and C7 in it; PH6, PH7, PH11 and PH13 are PRB-1's; PH8 and PH10 are LIV-2's):

| Id | Evidence | Pass condition in the script | `verified` text | `failed` text |
|---|---|---|---|---|
| PH1 | `hud_an sgr pane-a-200.ans`: the SGR state of each cell of the HUD row | `[ "$(j "$SGR" .ok)" = true ]`: stripe foregrounds 1, 3, 6 (`31`, `33`, `36`); first logo cell bold with foreground 15 (`1;97`); first cell of `hud-work` foreground 172 (`38;5;172`) | `capture-pane -e (tmux <v>): stripe fg 1,3,6, logo bold fg 15, worktree name fg 172 (38;5;n)` | `16-color part <true or false>, 256-color part <true or false>: <the JSON>` |
| PH2 | `hud_an two pane-b-200.ans` | `[ "$(j "$TWO" .rows)" = 2 ]`: the line after the logo line holds the stripe and the reflection, and the logo line does not hold the reflection | `verbose shows two rows: …` | `verbose shows <n> row(s): <the JSON>` |
| PH3 | `hud_an fit pane-a-19.txt expect-19.txt 19` (and the same for 70) | `[ "$C19" = cut ]` | `cut: at 70 columns the row is <class> (…); at 19 columns (width source <s>) a row of <n> cells is cut` | `wrapped: …` when the class is `wrapped`; else `not observed: …` |
| PH4 | The first width line of session a and of session d; `.env.COLUMNS` of both dumps; `indent + gap` of the 19-column fit | `[ "$WA" = "tty 200" ] && [ "$WP" = "tty 100" ] && [ "$CA" != unset ] && [ "$CP" != unset ] && [ "$GAP" = 0 ]` | `(a) width source in tmux at 200 columns: tty 200; on a pseudo-terminal of 100 columns: tty 100; (b) COLUMNS in the hook: …; (c) status area = pane width - 0 …` | the same text with the values seen |
| PH5 | `dump-a.keep.json` | the `jq -e` test below the table: `effort` is a string, or an object whose `level` is a string | `stdin effort is <value> (<type>); stdin keys: <keys>` | the same text (a `null` value means that stdin had no `effort`) |
| PH9 | `hud_an two pane-b-200.ans` | rows = 2 and `logo_col` ≥ 0 and `logo_col` = `refl_col` | `logo and reflection start at column <n> of the pane (tmux <v>); the glyphs of the font are not checked` | `logo column <n>, reflection column <m>, rows <k>` |
| PH12 | `hud_an ph12 events.json 3`: the first run that lives 1 s or more (from the `ps` samples); the next start is the first later `ps` run or dump write; `kills` when it lives less than 2.5 s; else `overlaps` when another run starts more than 0.2 s before its end; else `waits` | the class is `kills`, `waits` or `overlaps` (an observation exists) | `observation, refreshInterval 2 s: the first slow run lived <t> s of its 3 s bound; the next run started <u> s after its start: <what Claude Code does>; <n> runs in 45 s` | `not observed: no status line run of 1 s or more in 45 s (<n> runs seen)` |
| C7 | `body_has` over `c7/rec/[0-9]*.json` | `[ "$SHOWN" = yes ] && [ "$N_TR" -gt 0 ] && [ -z "$LEAK" ]`: the pane holds the logo; at least one recorded request holds `tracer-hud`; no recorded request holds the logo, the reflection, `777k` or `777000` | `HUD shown (verbose, --compact 777k); the logo, the reflection, 777k and 777000 are in none of <n> recorded requests (<m> hold the tracer prompt); mock upstream` | `HUD shown: …; recorded requests: …; with the tracer prompt: …; requests that hold HUD text: …; mock upstream` |

The PH5 test:

```sh
jq -e '(.effort | type) as $t | $t == "string" or ($t == "object" and (.effort.level | type) == "string")' "$HUD/dump-a.keep.json" >/dev/null
```

Every line has the form `LIVE <id> verified|failed (agent <Claude Code version>): <text>`. The script always prints the eight lines and then exits 0. A failed line does not fail the node.

Fixtures (`fixtures/claude-code/hud/`, removed and written again by each run; each one is `scrub <file> >fixtures/claude-code/hud/<name>`, which is `python3 -I scripts/premises/scrub.py "$RUN" <file>`; a file is written only when its source has content):

| Fixture | Content |
|---|---|
| `p-hud-1-results.txt` | The eight `LIVE` lines |
| `p-hud-1-run.json` | `date`, `claude_code`, `tmux`, `os` of the run |
| `p-hud-1-compact-200.txt` | The HUD row of session a at 200 columns, with the SGR codes of `capture-pane -e` |
| `p-hud-1-verbose-200.txt` | The two HUD rows of session b, with the SGR codes |
| `p-hud-1-compact-70.txt`, `p-hud-1-compact-19.txt` | The pane from the HUD row down (4 and 8 lines at most), plain |
| `p-hud-1-expect-70.txt`, `p-hud-1-expect-19.txt` | The row that the hook prints by hand for that width |
| `p-hud-1-dump-tmux.json`, `p-hud-1-dump-pty.json` | The dump files; `ZYGO_SESSION_DIR` and `ZYGO_URL` are replaced by `[set]` before the scrub |
| `p-hud-1-width.txt` | The width-source lines of sessions a and d |
| `p-hud-1-fit.json` | The analyzer results `sgr`, `two`, `70`, `19` |
| `p-hud-1-ph12.json` | The process samples (start and end seconds, no process id), the times of the dump writes, and the class |
| `p-hud-1-c7.json`, `p-hud-1-c7-pane.txt` | The request counts and marker hits; the two HUD rows of the C7 pane |

No whole pane is kept: only the HUD rows and the lines under them. A fixture must not hold a session id, a token, the user name, the home path, an account e-mail address or the host name. `scrub.py` replaces the run paths, the home path, the user name, the host names, UUIDs, e-mail addresses and key forms; the script replaces the session directory itself.

A failed line (the contract's mapping). The implementer writes the row and the note of the id with `failed` and the line text, and names the nodes in the report to the controller. The implementer changes no other node file, no code and not `graph.md`:

| Id | Nodes that the contract names |
|---|---|
| PH1 | HUD-8, HUD-9, HUD-10, H2, LIV-2 |
| PH2 | HUD-8, HUD-11, H2 |
| PH3 | HUD-8 |
| PH4 | HUD-8 |
| PH5 | HUD-9 |
| PH9 | HUD-8, H2 |
| PH12 | HUD-9 |
| C7 | HUD-9, and the hook entry in `crates/zygo/src/cmd/hook.rs` |

A `failed` text that starts with `timeout:`, `not observed:` or `zygo claude ended:` is a failure of the run, not an observation of the premise. The implementer records it the same way and says so in the report; the controller decides between a second run and a node.

### Live run findings (2026-11-21 and 2026-11-22; Claude Code 2.1.330, tmux 3.7c, Linux)

The real Claude Code differs from the plan in these points. The script (and Block A) is corrected for each. No premise id, pass condition or `LIVE` line format changed.

- **D1 (folder trust question).** The question starts with the pointer on `No, exit`, so `Enter` alone ends Claude Code. Pane of session a, run 1 (plain text of `capture-pane`; the path is the scratch worktree):

  ```text
   Accessing workspace:

   <scratch>/live-<pid>/hud/wt/hud-work

   Quick safety check: Is this a project you created or one you trust? (Like your own code, a well-known open source project, or work from your team). If not, take a moment to review what's in this
   folder first.

   Claude Code'll be able to read, edit, and execute files here.

   Security guide

   ❯ No, exit
     Yes, I trust this folder

   Enter to confirm · Esc to cancel

  Claude Code didn't start because this folder isn't trusted.
  Run claude again and choose Yes, or run it in another folder.
  hud-exit:1
  ```

  In session b of run 1 the one `Enter` was sent 0.5 s after the pane was empty and Claude Code did not take it: the question stayed for 60 s (`timeout: no HUD row in the pane within 60 s`). Correction: `hud_trust` (`Down` until the pane shows `❯` on the line `Yes, I trust this folder`, read two times, then `Enter`; a key that is not taken is sent again). The pseudo-terminal driver no longer sends `Enter` to the question. In runs 2 and 3 the question showed in session a only; sessions b, c and d started with no question (Claude Code kept the answer for the path).
- **D2 (width source).** The plan waited for `width source tty, width 70`. `/dev/tty` does not answer the hook under Claude Code 2.1.330, in tmux and on a pseudo-terminal; Claude Code sets `COLUMNS` to the terminal width. `session.log` of session a, run 3: `width source COLUMNS, width 200`, then `COLUMNS, width 70`, then `default, width 120` (at 19 columns `COLUMNS` is below the limit of 20). Session d: `COLUMNS, width 100`. In run 2 the 70-column capture was skipped (the wait for `tty` ended at its bound), and the first `COLUMNS 200` line was taken for the 19-column source. Correction: the wait at 70 accepts any source; the wait at 19 is for one more width line than before the resize, and that line gives the source for the expected row.
- **D3 (PH12, short runs).** A status line run with a warm cache lives a few ms; the `ps` samples (every 0.05 s) saw 9 runs in run 2 and 11 of 22 runs in run 3, so in run 2 "the next run started 6.18 s after its start" was the next run that `ps` saw, not the next run. Correction: the sampler also records each replacement of the dump file (one for each run). Run 3: a dump write each 2 s (22 in 45 s); each slow run ends about 2.1 s after its start, at the next refresh, before its 3 s bound; the next slow run comes 6 s later (the lease of the run that was ended stays for `LOCK_RETRY_AFTER` = 5 s, `crates/zygo-hooks/src/statusline/data.rs:48`), not 30 s later, because the ended run does not write the cache.
- **D4 (status row).** Claude Code draws the status line 2 cells from the left edge and cuts it with `…` 2 cells before the right edge (19-column pane: `  ▌▌▌ 𝘇𝘆𝗴𝗼 <branch glyph> hud…`). At 70 columns the HUD row for width 70 is 65 cells and shows whole. In the C7 pane (200 columns) Claude Code also writes its own notice (`● high · /effort`) at the right end of the first status row.
- **D5 (not in the plan's list, no change needed).** The status line shows before the first prompt. The login works under the reduced environment (`env -i`). `/exit` ends each tmux session. A pseudo-terminal that answers no terminal query still gets the status line. The reduced `PATH` gives one start-up line of a plugin of the operator (`plugin act101:act-lite: act-lite not found`); it does not stop the session.
- **D6 (step 5.2).** The `LIVE` texts hold measured values of the run (cells, seconds, run count, cost in the expected row), so the results file of a second run differs in those numbers. The records of Task 4 are written again from the acceptance run.
- **Runs.** Run 1 (2026-11-21): sessions a and c ended at the trust question (D1), b timed out, d gave no dump file; no model request; no result kept. Run 2 (2026-11-22): D2 and D3 found; one model turn; no result kept. Run 3: the corrected script; one model turn; the records of Task 4. The acceptance run (step 5.1) is run 4.

### Task 1: the script (no agent)

- [x] **1.1** Create `scripts/premises/live-hud.sh` with exactly the text of Block A. Check: `sh -n scripts/premises/live-hud.sh` prints nothing and exits 0.
- [x] **1.2** Run `sh scripts/premises/live-hud.sh --selfcheck`. Expected: 17 lines that start with `ok:`, then `live-hud selfcheck: ok (17)`, exit 0. (16 before the live-run corrections; see "Live run findings".)
- [x] **1.3** Run `sh scripts/premises/live-hud.sh`. Expected: `usage: sh scripts/premises/live-hud.sh --zygo <path> | --selfcheck`, exit 1.
- [x] **1.4** Run `sh scripts/check-premises.sh`. Expected: the last line is `check-premises: ok (P1 P2 P3 P4 P5 P6a P6b Q1 Q2 Q3 Q4 Q5 Q6 Q7)` (the new script is in its scan scope).
- [x] **1.5** Commit `scripts/premises/live-hud.sh` and this file: `test(premises): live-hud.sh, the P-HUD-1 live check of the status line HUD`.

### Task 2: the gate before the live run

- [x] **2.1** Run `OC_DIR=$HOME/.opencode/bin; "$OC_DIR/opencode" --version`. Expected: one version at or above 1.18.16 (1.19.6 on 2026-11-21), exit 0. When the file is absent or the version is lower: stop here and report the output. Do not add a stub and do not edit `lib.sh`. Each later step that needs OpenCode sets `OC_DIR` again in its own command (a shell variable does not stay between steps).
- [x] **2.2** Run `claude --version` and `tmux -V`. Expected: Claude Code 2.1.328 or later; tmux 3.2 or later.

### Task 3: the live run

- [x] **3.1** Run `cargo build -p zygo --features test-hooks`. Expected: exit 0 and the file `target/debug/zygo`.
- [x] **3.2** Run `OC_DIR=$HOME/.opencode/bin; PATH="$OC_DIR:$PATH" sh scripts/premises/live-hud.sh --zygo target/debug/zygo`. It takes 5 to 8 minutes and needs no input. Expected: eight lines `LIVE PH1 …`, `LIVE PH2 …`, `LIVE PH3 …`, `LIVE PH4 …`, `LIVE PH5 …`, `LIVE PH9 …`, `LIVE PH12 …`, `LIVE C7 …`, each with `verified` or `failed`; then `live: fixtures in fixtures/claude-code/hud; scratch removed on exit`; exit 0. When the script exits 1 with a `live: …` line, a precondition failed and no agent ran: report the line and stop. When more than four lines say `timeout:` or `zygo claude ended:`, report the lines and stop before Task 4 (the sessions did not start; the rows would hold no observation).
- [x] **3.3** Run `pgrep -fl "tmux .*-L zygo-hud-"` and `ls "${TMUX_TMPDIR:-/tmp}/tmux-$(id -u)"`. Expected: `pgrep` prints nothing and exits 1; the listing has no `zygo-hud-` name. (The traps ended the private tmux server and removed its socket file.) Note: `pgrep -f` also matches the shell that runs this command line (the pattern is in its arguments); a line that names only that shell counts as nothing.
- [x] **3.4** Run `ls fixtures/claude-code/hud`. Expected: `p-hud-1-results.txt` and `p-hud-1-run.json`, and the other `p-hud-1-*` files of the fixture table for each session that ran.
- [x] **3.5** Run `grep -rn -i -F -e "$HOME" -e "$(id -un)" -e "$(hostname)" fixtures/claude-code/hud`. Expected: no output, exit 1. Then run `grep -rn -E -e '[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}' -e 'sessions/' -e '@' fixtures/claude-code/hud`. Expected: no output, exit 1, or only lines with the placeholder `00000000-0000-4000-8000-…`. When a line matches, do not edit the fixture by hand: report the line number and the file (not the matched text) and stop.
- [x] **3.6** Read every `p-hud-1-*` fixture once. Expected: only HUD rows, the lines under them at 70 and 19 columns, numbers and the texts of the fixture table; no prompt text of the operator, no account name, no organization name. When a file holds one, report the file and stop.

### Task 4: the records

The values: `<DATE>`, `<CC>` and `<TMUX>` are `.date`, `.claude_code` and `.tmux` of `fixtures/claude-code/hud/p-hud-1-run.json`. For each id, `<RESULT-id>` is the word after the id in its line of `fixtures/claude-code/hud/p-hud-1-results.txt`, and `<TEXT-id>` is the text after `): ` in that line. In a table cell write `\|` for each `|` of a text.

- [ ] **4.1** In `docs/live-checks.md`, line 3: replace `the N24, N25, N14, N15, N28, N33, N34, N35 notes.` with `the N24, N25, N14, N15, N28, N33, N34, N35 notes; `docs/specs/2026-11-21-statusline-hud-design.md` H§12 (P-HUD-1).` (the inserted path is in backticks).
- [ ] **4.2** In `docs/live-checks.md`, bullet "Isolation": replace `every run except #28 gets a temporary` with `every run except #28 and the passthrough parts of P-HUD-1 gets a temporary`. Bullet "Upstream per row": replace `#28 uses the claude.ai login Claude Code already stores` with `#28 and P-HUD-1 (all parts but C7, which uses the mock) use the claude.ai login Claude Code already stores`.
- [ ] **4.3** In `docs/live-checks.md`, add this section directly above the line `## Results` (the text between the two lines of four backticks; the section holds one `sh` code block):

````markdown
## P-HUD-1 (status line HUD)

Node LIV-1; `docs/specs/2026-11-21-statusline-hud-design.md` H§12. From the checkout, on Linux or macOS, with tmux 3.2 or later, `python3`, `jq` and `git`, Claude Code logged in, and `claude` and `opencode` on `PATH` (`lib.sh` checks both; the script starts no OpenCode):

```sh
cargo build -p zygo --features test-hooks
sh scripts/premises/live-hud.sh --zygo target/debug/zygo
```

- The build needs the `test-hooks` feature: the script uses the seams `ZYGO_TEST_HUD_DUMP`, `ZYGO_TEST_FETCH_DELAY_MS` and `ZYGO_TEST_REFRESH_INTERVAL`. `sh scripts/premises/live-hud.sh --selfcheck` tests the script's analyzer with no agent.
- It prints one `LIVE <id> verified|failed (agent <version>): …` line for each of PH1, PH2, PH3, PH4, PH5, PH9, PH12 and C7, writes `fixtures/claude-code/hud/p-hud-1-*` and exits 0. A failed line does not change the exit status.
- Parts PH1 to PH12 run `zygo claude --passthrough` four times on the stored login: three sessions in a private tmux server (`tmux -L zygo-hud-<pid>`, no tmux config file, 200 x 50; one of them is then made 70 and 19 columns wide) and one on a pseudo-terminal of 100 columns. They use the real `HOME` and the caller's `CLAUDE_CONFIG_DIR`. The cwd is a linked git worktree in the scratch folder. The script answers the folder trust question of Claude Code with yes. Claude Code keeps a `projects` entry and a transcript folder for that path; the script does not delete them. zygo keeps the usage rows.
- Spend: one short answer on the login (`Reply with the single word ready. Use no tool.`), and Claude Code's own start requests in the four sessions. No tool runs with a permission of the script (no `--yolo`).
- Part C7 runs on the mock upstream with a temporary `HOME`, as P-N28-1: a passthrough request cannot be recorded.
- Bounds: 60 s for each session start, 120 s for the answer, 20 s for each width change, 45 s of process samples, 90 s for the mock request, 10 s for each session end. A bound that passes gives `failed` with the reason for the ids of that part; the script goes on.
- The traps end the private tmux server, the pseudo-terminal session, the mock and the background processes, and remove the scratch tree.
````

- [ ] **4.4** In `docs/live-checks.md`, add these eight rows at the end of the table of `## Results`, in this order, with the values in place:

```markdown
| P-HUD-1/PH1 | The status line shows SGR colors, 16-color and `38;5;n` (HUD design H§12 PH1) | Claude Code | <CC> | <DATE> | <RESULT-PH1> | tmux <TMUX>, Linux; `scripts/premises/live-hud.sh`, `zygo claude --passthrough` on the stored login. <TEXT-PH1>. `fixtures/claude-code/hud/p-hud-1-compact-200.txt`, `fixtures/claude-code/hud/p-hud-1-fit.json`; H§12 |
| P-HUD-1/PH2 | A two-row status line shows as two rows (H§12 PH2) | Claude Code | <CC> | <DATE> | <RESULT-PH2> | tmux <TMUX>, Linux; passthrough. <TEXT-PH2>. `fixtures/claude-code/hud/p-hud-1-verbose-200.txt`; H§12 |
| P-HUD-1/PH3 | A row wider than the status area is cut, not wrapped (H§12 PH3) | Claude Code | <CC> | <DATE> | <RESULT-PH3> | tmux <TMUX>, Linux; passthrough. <TEXT-PH3>. `fixtures/claude-code/hud/p-hud-1-compact-70.txt`, `fixtures/claude-code/hud/p-hud-1-compact-19.txt`, `fixtures/claude-code/hud/p-hud-1-expect-19.txt`; H§12 |
| P-HUD-1/PH4 | The width sources reach the hook, and the status area is the full terminal width (H§12 PH4) | Claude Code | <CC> | <DATE> | <RESULT-PH4> | tmux <TMUX>, Linux; passthrough, in tmux and on a pseudo-terminal. <TEXT-PH4>. `fixtures/claude-code/hud/p-hud-1-width.txt`, `fixtures/claude-code/hud/p-hud-1-dump-tmux.json`, `fixtures/claude-code/hud/p-hud-1-dump-pty.json`, `fixtures/claude-code/hud/p-hud-1-fit.json`; H§12 |
| P-HUD-1/PH5 | The stdin `effort` is a string, or an object with a string level (H§12 PH5) | Claude Code | <CC> | <DATE> | <RESULT-PH5> | tmux <TMUX>, Linux; passthrough. <TEXT-PH5>. `fixtures/claude-code/hud/p-hud-1-dump-tmux.json`; H§12 |
| P-HUD-1/PH9 | The reflection starts at the column of the logo (H§12 PH9) | Claude Code | <CC> | <DATE> | <RESULT-PH9> | tmux <TMUX>, Linux; passthrough. <TEXT-PH9>. `fixtures/claude-code/hud/p-hud-1-verbose-200.txt`; H§12 |
| P-HUD-1/PH12 | Observation: a status line run that still goes at the next refresh (H§12 PH12) | Claude Code | <CC> | <DATE> | <RESULT-PH12> | tmux <TMUX>, Linux; passthrough, `refreshInterval` 2 s, status fetch 5 s late. <TEXT-PH12>. `fixtures/claude-code/hud/p-hud-1-ph12.json`; H§12 |
| P-HUD-1/C7 | The HUD text is in no upstream request (H§12, C7) | Claude Code | <CC> | <DATE> | <RESULT-C7> | tmux <TMUX>, Linux; mock upstream, as P-N28-1. <TEXT-C7>. `fixtures/claude-code/hud/p-hud-1-c7.json`, `fixtures/claude-code/hud/p-hud-1-c7-pane.txt`; H§12 + C7 |
```

  Leave a fixture path out of a row when step 3.4 did not list that file.
- [ ] **4.5** In `docs/specs/2026-11-21-statusline-hud-design.md`, H§12 table: for each of the rows PH1, PH2, PH3, PH4, PH5, PH9, PH12, append this to the end of the cell "Evidence so far" (one space before it), with the values of that id:

```markdown
*(Note <DATE>, LIV-1, P-HUD-1: <RESULT-id> on Claude Code <CC>, tmux <TMUX>, Linux. <TEXT-id>. `docs/live-checks.md` row P-HUD-1/<id>.)*
```

  For an id whose result is `failed`, put this sentence before the closing `)*`: ` The fallback of this row is not built by LIV-1; the LIV-1 contract names the nodes: <the nodes of that id in the table "A failed line">.`
- [ ] **4.6** In the same file, add this paragraph directly below the paragraph that starts `Live check **P-HUD-1**` (one empty line between them):

```markdown
*(Note <DATE>, LIV-1: P-HUD-1 ran with `scripts/premises/live-hud.sh` on Claude Code <CC>, tmux <TMUX>, Linux. Parts PH1 to PH5, PH9 and PH12 ran through `zygo claude --passthrough` on the stored login. The C7 part ran on the mock upstream, as P-N28-1, because a passthrough request cannot be recorded: <RESULT-C7>. <TEXT-C7>. In a 70-column pane the HUD fits its row to the pane (H§6.7 step 1), so no row is wider than the pane there; the script also makes the pane 19 columns wide, below the 20-column limit of H§6.7, so the hook uses another width source and prints a row that is wider than the area (PH3, and the status area width of PH4). The "outside tmux" part of PH4 ran on a pseudo-terminal of 100 columns. PH9 compares columns in the tmux grid; the glyphs of a font are not checked. Rows: `docs/live-checks.md` P-HUD-1/PH1 to P-HUD-1/C7.)*
```

- [ ] **4.7** In the same file, add this row at the end of the table of `## Change log`:

```markdown
| <DATE> | H§12 (rows PH1, PH2, PH3, PH4, PH5, PH9, PH12; note below the P-HUD-1 paragraph) | LIV-1 (live check) | P-HUD-1 ran on Claude Code <CC>, tmux <TMUX>, Linux; each result is a dated note. |
```

- [ ] **4.8** Commit `fixtures/claude-code/hud/`, `docs/live-checks.md`, `docs/specs/2026-11-21-statusline-hud-design.md` and this file: `docs(live): P-HUD-1 results on Claude Code <CC>, tmux <TMUX>; fixtures and H§12 notes`.

### Task 5: verification

- [ ] **5.1** Acceptance, as the contract states it, with the OpenCode directory on `PATH` for the script. Run `OC_DIR=$HOME/.opencode/bin; cargo build -p zygo --features test-hooks && PATH="$OC_DIR:$PATH" sh scripts/premises/live-hud.sh --zygo target/debug/zygo && cargo test -p zygo --test forbidden_content`. Expected: exit 0; the eight `LIVE` lines; `test result: ok` with `0 failed` for `forbidden_content`. This is the second live run of the plan (one more short answer); it writes the fixtures again. When `forbidden_content` names a line of a new file, report the file and the line number and stop; do not edit the test.
- [ ] **5.2** Run `git diff --stat -- fixtures/claude-code/hud/p-hud-1-results.txt`. Expected: no output (the lines of the second run equal the lines of the first). When the file changed: write the cells of step 4.4 and the notes of steps 4.5 and 4.6 again from the new lines (result word and text of each changed id), and name each id whose result word changed in the report.
- [ ] **5.3** Run `N2_FORBIDDEN_NAMES="$(id -un)" sh scripts/check-premises.sh`. Expected: the last line is `check-premises: ok (P1 P2 P3 P4 P5 P6a P6b Q1 Q2 Q3 Q4 Q5 Q6 Q7)`. (It scans `fixtures/`, `scripts/premises/live-hud.sh` and `docs/live-checks.md` for forbidden content, the user name and the host name. It does not check the P-HUD-1 rows or the HUD spec.)
- [ ] **5.4** Run `OC_DIR=$HOME/.opencode/bin; PATH="$OC_DIR:$PATH" sh scripts/premises/live-selftest.sh`. Expected: the last line is `live selftest: ok (54)`, exit 0 (it copies the new script and fixtures into its test tree; it starts no agent).
- [ ] **5.5** Run `git status --short`. Expected: only `scripts/premises/live-hud.sh` (when changed after step 1.5), `fixtures/claude-code/hud/`, `docs/live-checks.md` and `docs/specs/2026-11-21-statusline-hud-design.md` (when changed after step 4.8) and this file.
- [ ] **5.6** Commit those paths and this file with all steps ticked: `docs(work): LIV-1 plan steps done; P-HUD-1 fixtures of the acceptance run`.
- [ ] **5.7** Report to the controller: the eight `LIVE` lines; for each `failed` id the nodes of the table "A failed line"; each line that is a failure of the run (`timeout:`, `not observed:`, `zygo claude ended:`); the two side effects on the operator's Claude Code folder (the `projects` entry and the transcript folder of the scratch worktree path).

### Block A: `scripts/premises/live-hud.sh`

````sh
#!/bin/sh
# Live check P-HUD-1 (node LIV-1; HUD design H§12, H§14 #37): the built status line HUD in the real
# Claude Code, on Linux or macOS. Run by hand only; never from CI or a Rust test.
# Usage: sh scripts/premises/live-hud.sh --zygo <path>   (<path>: cargo build -p zygo --features test-hooks)
#        sh scripts/premises/live-hud.sh --selfcheck      (the analyzer on built-in samples; no agent, no tmux)
# Prints eight lines, LIVE <id> verified|failed (agent <version>): <text>, for PH1 PH2 PH3 PH4 PH5 PH9
# PH12 C7. Exit 0 when the eight lines are printed (a failed line does not change the exit status);
# exit 1 when a precondition fails, before any agent starts.
#
# Parts PH1 to PH12 run `zygo claude --passthrough` on the claude.ai login that Claude Code already
# stores (as live-passthrough.sh; no key, no mock). What they do to the operator's machine:
# - They use the real HOME, and the caller's CLAUDE_CONFIG_DIR when it is set, so Claude Code reads the
#   operator's settings, hooks, plugins and MCP servers. The script edits none of them and adds no hook.
#   DISABLE_AUTOUPDATER=1 and ZYGO_NO_UPDATE_CHECK=1 are set.
# - Claude Code asks whether it trusts the run's temporary worktree; the script answers yes (it moves
#   the pointer to "Yes, I trust this folder", reads the pane to see it there, then sends Enter).
#   Claude Code then keeps an entry for that path under "projects" in its .claude.json, and a transcript
#   folder under <config dir>/projects/. The script does not delete them; the operator may.
# - They spend the operator's claude.ai usage: one short answer (session a), and Claude Code's own
#   start requests in four sessions.
# - zygo writes its state under ~/.local/share/zygo (usage rows stay; the plugin snapshot is refreshed)
#   and ~/.cache/zygo (each session folder, removed by zygo at exit). ZYGO_CONFIG is a file of the run.
# - Every session runs in a private tmux server (tmux -L zygo-hud-<pid>, no tmux config file) or on a
#   pseudo-terminal of the script; the traps end them. No global tmux option or hook is set.
# Part C7 runs on the mock upstream with a temporary HOME, as P-N28-1 (live-claude.sh): a passthrough
# request cannot be recorded.

# hud_py <dir>: writes the three Python helpers of this script into <dir>.
hud_py() {
  cat >"$1/hud_an.py" <<'PY'
import json, os, re, subprocess, sys, unicodedata

LOGO = "\U0001d607\U0001d606\U0001d5f4\U0001d5fc"
REFL = "zʎƃo"
STRIPE = "▌"
CSI = re.compile(r"\x1b\[[0-9;:?]*[A-Za-z]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)")


def plain(s):
    return CSI.sub("", s)


def cells(s):
    return sum(0 if unicodedata.combining(c) else 2 if unicodedata.east_asian_width(c) in "WF" else 1 for c in s)


def block(path):
    """The pane lines from the last line that holds the logo to the last non-empty line, SGR kept."""
    lines = open(path, encoding="utf-8", errors="replace").read().split("\n")
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


def cmd_sgr(pane):
    b = block(pane)
    if not b:
        return {"ok": False, "why": "no HUD row"}
    a = attrs(b[0])
    text = "".join(c for c, *_ in a)
    s, l, w = text.find(STRIPE * 3), text.find(LOGO), text.find("hud-work")
    stripe = [a[s + n][1] for n in range(3)] if s >= 0 else []
    logo = [a[l][1], a[l][2]] if l >= 0 else [None, False]
    tree = a[w][1] if w >= 0 else None
    r = {"stripe": stripe, "logo_fg": logo[0], "logo_bold": logo[1], "worktree_fg": tree,
         "stripe_dim": bool(s >= 0 and a[s][3])}
    r["ok16"] = stripe == [1, 3, 6] and logo[0] == 15 and logo[1] is True
    r["ok256"] = tree == 172
    r["ok"] = r["ok16"] and r["ok256"]
    return r


def cmd_two(pane):
    b = [plain(l) for l in block(pane)]
    if not b:
        return {"rows": 0, "logo_col": -1, "refl_col": -1}
    lc = b[0].find(LOGO)
    rc = b[1].find(REFL) if len(b) > 1 else -1
    same = REFL in b[0]
    return {"rows": 2 if rc >= 0 and STRIPE in b[1] and not same else 1, "logo_col": lc, "refl_col": rc}


def cmd_fit(pane, expected, cols):
    b = [plain(l) for l in block(pane)]
    e = plain(open(expected, encoding="utf-8", errors="replace").read()).split("\n")[0].rstrip()
    if not b or not e:
        return {"class": "none", "why": "no HUD row in the pane" if not b else "no expected row"}
    indent = len(b[0]) - len(b[0].lstrip(" "))
    s0 = b[0].strip()
    r = {"indent": indent, "shown_cells": cells(s0), "expected_cells": cells(e), "cols": int(cols)}
    if s0 == e:
        r["class"] = "whole"
        return r
    t = s0[:-1].rstrip() if s0.endswith("…") else s0
    if not t or not e.startswith(t):
        r["class"] = "other"
        return r
    rest = e[len(t):].lstrip()
    w = b[1].strip() if len(b) > 1 else ""
    r["class"] = "wrapped" if w and rest and (rest.startswith(w) or w.startswith(rest)) else "cut"
    if t == s0:
        # The pane does not show blanks at the end of a line: count the blanks of the expected row that follow.
        blanks = len(e[len(t):]) - len(rest)
        r["shown_cells"] = min(cells(s0) + blanks, int(cols) - indent)
    r["gap"] = int(cols) - indent - r["shown_cells"]
    return r


def cmd_ph12(events, bound):
    d = json.load(open(events))
    if isinstance(d, list):
        d = {"runs": d, "dumps": []}
    runs = sorted(d["runs"], key=lambda x: x["start"])
    dumps = sorted(d.get("dumps", []))
    bound = float(bound)
    # Each run writes the dump file once, so the dump count is the run count; ps seldom sees a run of a few ms.
    n = len(dumps) if dumps else len(runs)
    long = [x for x in runs if x["end"] - x["start"] >= 1.0]
    if not long:
        return {"class": "none", "runs": n, "seen_by_ps": len(runs)}
    first = long[0]
    life = round(first["end"] - first["start"], 2)
    # The starts after the slow run: the runs that ps saw, and the dump writes (the slow run writes its own
    # dump right after its stdin read, at most 0.5 s after its start).
    later = [x["start"] for x in runs if x["start"] > first["start"]] + [t for t in dumps if t > first["start"] + 0.5]
    nxt = round(min(later) - first["start"], 2) if later else None
    if life < bound - 0.5:
        c = "kills"
    elif nxt is not None and nxt < life - 0.2:
        c = "overlaps"
    else:
        c = "waits"
    return {"class": c, "life": life, "next_start": nxt, "runs": n, "seen_by_ps": len(runs)}


def cmd_expect(dump, zygo, cwd, columns, out):
    d = json.load(open(dump))
    env = {k: v for k, v in d.get("env", {}).items() if v is not None and k != "COLUMNS"}
    env.update({"HOME": os.environ["HOME"], "PATH": os.environ["PATH"]})
    if columns != "none":
        env["COLUMNS"] = columns
    p = subprocess.run([zygo, "hook", "statusline"], stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                       stderr=subprocess.DEVNULL, env=env, cwd=cwd, start_new_session=True, timeout=10)
    open(out, "wb").write(p.stdout)
    return {"bytes": len(p.stdout)}


def cmd_rows(pane, count, form):
    """The first <count> lines of the HUD block as text; form: raw (SGR kept) or plain."""
    b = block(pane)[:int(count)]
    return "".join((l if form == "raw" else plain(l)).rstrip() + "\n" for l in b)


if __name__ == "__main__":
    r = globals()["cmd_" + sys.argv[1]](*sys.argv[2:])
    sys.stdout.write(r if isinstance(r, str) else json.dumps(r, ensure_ascii=False) + "\n")
PY
  cat >"$1/hud_poll.py" <<'PY'
import json, os, subprocess, sys, time

# hud_poll.py <out> <zygo path> <seconds> [<dump file>]: samples every 0.05 s. "runs": each process whose
# arguments start with "<zygo path> hook statusline" (first and last sample that saw it). "dumps": the time
# of each new dump file (each run of the hook replaces it once), so a run of a few ms counts too.
out, prefix, seconds = sys.argv[1], sys.argv[2] + " hook statusline", float(sys.argv[3])
dump = sys.argv[4] if len(sys.argv) > 4 else None
t0, seen, dumps, last = time.monotonic(), {}, [], None
while time.monotonic() - t0 < seconds:
    now = round(time.monotonic() - t0, 2)
    if dump:
        try:
            st = os.stat(dump)
            key = (st.st_ino, st.st_mtime_ns)
        except OSError:
            key = None
        if key is not None and key != last:
            dumps.append(now)
            last = key
    ps = subprocess.run(["ps", "-axo", "pid=,args="], capture_output=True, text=True).stdout
    for line in ps.splitlines():
        pid, _, args = line.strip().partition(" ")
        if args.strip().startswith(prefix):
            seen.setdefault(pid, [now, now])[1] = now
    time.sleep(0.05)
runs = [{"n": i + 1, "start": a, "end": b} for i, (a, b) in enumerate(sorted(seen.values()))]
json.dump({"runs": runs, "dumps": dumps}, open(out, "w"))
PY
  cat >"$1/hud_pty.py" <<'PY'
import fcntl, json, os, pty, re, select, shutil, signal, struct, sys, termios, time

CSI = re.compile(rb"\x1b\[[0-9;:?]*[A-Za-z]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)")
dump, log_copy, cols, wait, cwd = sys.argv[1], sys.argv[2], int(sys.argv[3]), float(sys.argv[4]), sys.argv[5]
argv = sys.argv[6:]
signal.signal(signal.SIGTERM, lambda *_: sys.exit(143))
pid, fd = pty.fork()
if pid == 0:
    os.chdir(cwd)
    os.execv(argv[0], argv)
fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 50, cols, 0, 0))
buf, code = b"", 3


def pump(seconds):
    """Reads the terminal output for `seconds`; False when the child closed the terminal."""
    global buf
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        if select.select([fd], [], [], 0.2)[0]:
            try:
                data = os.read(fd, 65536)
            except OSError:
                return False
            if not data:
                return False
            buf = (buf + data)[-65536:]
    return True


try:
    end = time.monotonic() + wait
    while time.monotonic() < end and pump(0.5):
        if os.path.exists(dump) and os.path.getsize(dump) > 0:
            code = 0
            break
        # The folder trust question: not answered here (Enter alone means "No, exit" in Claude Code
        # 2.1.330). A tmux session of the script answers it before this session starts.
        if b"Itrustthisfolder" in re.sub(rb"\s+", b"", CSI.sub(b"", buf)):
            code = 5
            break
    if code == 0:
        pump(1)
        try:
            d = json.load(open(dump)).get("env", {}).get("ZYGO_SESSION_DIR")
            shutil.copyfile(os.path.join(d, "session.log"), log_copy)
        except (OSError, ValueError, TypeError):
            code = 4
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
sys.exit(code)
PY
}

if [ "${1:-}" = --selfcheck ]; then
  set -eu
  PY=$(mktemp -d)
  trap 'rm -rf "$PY"' EXIT
  hud_py "$PY"
  cat >"$PY/hud_an_test.py" <<'PY'
import json, os, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
T = HERE
E = "\x1b"
LOGO = "\U0001d607\U0001d606\U0001d5f4\U0001d5fc"
REFL = "zʎƃo"
S = "▌"


def run(*a):
    return json.loads(subprocess.run([sys.executable, "-I", os.path.join(HERE, "hud_an.py"), *a], capture_output=True, text=True, check=True).stdout)


def w(name, text):
    p = os.path.join(T, name)
    open(p, "w", encoding="utf-8").write(text)
    return p


n = 0


def ok(name, got, want):
    global n
    assert got == want, (name, got, want)
    n += 1
    print("ok:", name)


# S1: tmux -e form of the compact row, 2-space indent, after a rule line.
row = f"  {E}[31m{S}{E}[33m{S}{E}[36m{S}{E}[39m {E}[1m{E}[97m{LOGO}{E}[0m {E}[1m{E}[34m hud-b{E}[0m {E}[38;5;172m\U000f0645 hud-work{E}[39m  ctx 12k/{E}[2m200k{E}[0m"
p = w("s1.txt", "x\n" + "─" * 40 + "\n" + row + "\n  ⏸ manual mode on\n\n\n")
r = run("sgr", p)
ok("S1 sgr", (r["stripe"], r["logo_fg"], r["logo_bold"], r["worktree_fg"], r["ok"]), ([1, 3, 6], 15, True, 172, True))
# S2: no 256-color: the worktree name in 33.
p = w("s2.txt", row.replace("38;5;172", "33") + "\n")
r = run("sgr", p)
ok("S2 sgr 16 only", (r["ok16"], r["ok256"], r["worktree_fg"]), (True, False, 3))
# S3: two rows, same column.
v1 = f"  {S}{S}{S} {LOGO}  main   local"
v2 = f"  {S}{S}{S} {REFL}  ctx -/-"
ok("S3 two rows", run("two", w("s3.txt", v1 + "\n" + v2 + "\n")), {"rows": 2, "logo_col": 6, "refl_col": 6})
# S4: joined into one row.
ok("S4 one row", run("two", w("s4.txt", v1 + "   " + REFL + " ctx\n"))["rows"], 1)
# S5: fit classes.
exp = w("exp.txt", f"{E}[31m{S}{S}{S}{E}[0m {LOGO} main  ctx 12k/200k  tokens 1k 2k  $0.01")
e = f"{S}{S}{S} {LOGO} main  ctx 12k/200k  tokens 1k 2k  $0.01"
ok("S5 whole", run("fit", w("f1.txt", "  " + e + "\n  footer\n"), exp, "70")["class"], "whole")
r = run("fit", w("f2.txt", "  " + e[:17] + "\n  footer\n"), exp, "19")
ok("S5 cut", (r["class"], r["indent"], r["shown_cells"], r["gap"]), ("cut", 2, 17, 0))
r = run("fit", w("f3.txt", "  " + e[:16] + "…\n"), exp, "19")
ok("S5 cut with ellipsis", (r["class"], r["shown_cells"]), ("cut", 17))
r = run("fit", w("f4.txt", "  " + e[:17] + "\n  " + e[17:34] + "\n  " + e[34:] + "\n"), exp, "19")
ok("S5 wrapped", r["class"], "wrapped")
r = run("fit", w("f7.txt", "  " + e[:14] + "\n"), exp, "19")
ok("S5 cut at a blank", (r["class"], r["shown_cells"], r["gap"]), ("cut", 15, 2))
ok("S5 other", run("fit", w("f5.txt", "  " + e.replace("12k", "13k") + "\n"), exp, "70")["class"], "other")
ok("S5 none", run("fit", w("f6.txt", "nothing\n"), exp, "70")["class"], "none")
# S6: PH12 classes (bound 3).
ev = lambda rows: w("ev.json", json.dumps([{"n": i, "start": a, "end": b} for i, (a, b) in enumerate(rows)]))
ok("S6 overlaps", run("ph12", ev([(1.0, 3.95), (3.0, 3.1), (5.0, 5.1)]), "3")["class"], "overlaps")
ok("S6 waits", run("ph12", ev([(1.0, 3.95), (4.0, 4.1), (6.0, 6.1)]), "3")["class"], "waits")
ok("S6 kills", run("ph12", ev([(1.0, 3.0), (3.0, 3.1)]), "3")["class"], "kills")
ok("S6 none", run("ph12", ev([(1.0, 1.1), (3.0, 3.1)]), "3")["class"], "none")
r = run("ph12", w("ev2.json", json.dumps({"runs": [{"n": 1, "start": 1.0, "end": 3.1}, {"n": 2, "start": 7.0, "end": 9.0}], "dumps": [1.2, 3.05, 5.0, 7.1]})), "3")
ok("S6 kills, starts from the dump writes", (r["class"], r["life"], r["next_start"], r["runs"], r["seen_by_ps"]), ("kills", 2.1, 2.05, 4, 2))
out = subprocess.run([sys.executable, "-I", os.path.join(HERE, "hud_an.py"), "rows", w("r.txt", "a\n" + v1 + "\n" + v2 + "\n  footer\n\n"), "2", "plain"], capture_output=True, text=True, check=True).stdout
ok("S7 rows", out, v1 + "\n" + v2 + "\n")
print(f"live-hud selfcheck: ok ({n})")
PY
  python3 -I "$PY/hud_an_test.py"
  exit 0
fi

[ "${1:-}" = --zygo ] && [ -n "${2:-}" ] && [ $# -eq 2 ] || { echo "usage: sh scripts/premises/live-hud.sh --zygo <path> | --selfcheck"; exit 1; }
[ -f "$2" ] && [ -x "$2" ] || { echo "live: $2 is not an executable file"; exit 1; }
for t in tmux python3 jq git ps; do command -v "$t" >/dev/null || { echo "live: $t is not on PATH"; exit 1; }; done
# The path as std::env::current_exe gives it: the status line command of the session starts with it.
ZYGO_BIN=$(python3 -I -c 'import os, sys; print(os.path.realpath(sys.argv[1]))' "$2")
HUD_CC_DIR=${CLAUDE_CONFIG_DIR:-}
unset TMUX TMUX_PANE
. "$(dirname "$0")/lib.sh"
. "$PREMISES_DIR/live.sh"
live_traps
TERM=xterm-256color

# Every tmux call of this script and of lib.sh and live.sh goes to a private server.
HUD_SOCK="zygo-hud-$$"
tmux() { command tmux -u -L "$HUD_SOCK" -f /dev/null "$@"; }
# hud_end <status>: end the private tmux server (and so every agent session in it), remove its socket
# file, then live_end.
hud_end() {
  trap - EXIT INT TERM HUP PIPE
  command tmux -L "$HUD_SOCK" kill-server 2>/dev/null || true
  rm -f "${TMUX_TMPDIR:-/tmp}/tmux-$(id -u)/$HUD_SOCK" 2>/dev/null || true
  live_end "$1"
}
trap 'hud_end $?' EXIT
trap 'hud_end 129' HUP
trap 'hud_end 130' INT
trap 'hud_end 141' PIPE
trap 'hud_end 143' TERM

v=$("$CLAUDE_BIN" --version 2>/dev/null || true)
LIVE_CC_VER=$(agent_ver "$v")
agent_ok "$LIVE_CC_VER" "$PIN_CC" || {
  echo "live: claude is $(agent_show "$v"); the minimum is $PIN_CC (docs/premises.md); put a version at or above it first on PATH"; exit 1; }
LIVE_VER=$LIVE_CC_VER
TMUX_VER=$(command tmux -V | sed 's/^tmux //')
ver_ge "$(printf '%s' "$TMUX_VER" | sed 's/[^0-9.].*$//')" 3.2 || { echo "live: tmux is $TMUX_VER; the minimum is 3.2"; exit 1; }
case "$HOME$HUD_CC_DIR$TOOL_PATH$N2_SCRATCH$ZYGO_BIN" in
  *[[:space:]\']*) echo "live: a path (HOME, CLAUDE_CONFIG_DIR, PATH, the scratch dir or the zygo path) holds a blank or a quote"; exit 1 ;;
esac

FX="$REPO_DIR/fixtures/claude-code/hud"
mkdir -p "$FX"
LOGO=$(printf '\360\235\230\207\360\235\230\206\360\235\227\264\360\235\227\274')   # U+1D607 U+1D606 U+1D5F4 U+1D5FC
REFL=$(printf 'z\312\216\306\203o')                                                 # z U+028E U+0183 o
PTR=$(printf '\342\235\257')                                                        # U+276F, the pointer of a Claude Code menu
TRUST_YES="Yes, I trust"                                                            # the accept option of the folder trust question

live_run hud
HUD=$RUN
WT="$HUD/wt/hud-work"
git -C "$R" worktree add -q -b hud-b "$WT"
: >"$HUD/zygo.toml"
printf '[statusline]\nstyle = "verbose"\n' >"$HUD/zygo-verbose.toml"
: >"$HUD/lines.txt"
PY="$HUD/py"
mkdir -p "$PY" "$HUD/probe"
hud_py "$PY"
hud_an() { python3 -I "$PY/hud_an.py" "$@"; }

# The binary has the HUD-9 seams: one hand run of the hook writes the dump file.
(cd "$HUD" && env -i HOME="$HOME" PATH="$TOOL_PATH" ZYGO_SESSION_DIR="$HUD/probe" ZYGO_TEST_HUD_DUMP="$HUD/probe.json" \
  "$ZYGO_BIN" hook statusline </dev/null >/dev/null 2>&1) || true
[ -s "$HUD/probe.json" ] || { echo "live: $ZYGO_BIN has no HUD test seams; build it with: cargo build -p zygo --features test-hooks"; exit 1; }
rm -f "$FX"/p-hud-1-*

# line <id> verified|failed <text>: the LIVE line (live.sh result), also kept for the results fixture.
line() { result "$@" | tee -a "$HUD/lines.txt"; }
# hud_env <config file> <dump file> [NAME=value ...]: the environment words of one passthrough launch.
hud_env() {
  _c=$1 _d=$2; shift 2
  echo "HOME=$HOME ${HUD_CC_DIR:+CLAUDE_CONFIG_DIR=$HUD_CC_DIR} PATH=$TOOL_PATH TERM=xterm-256color LANG=C.UTF-8 DISABLE_AUTOUPDATER=1 ZYGO_NO_UPDATE_CHECK=1 ZYGO_CONFIG=$_c ZYGO_NERD_FONT=1 ZYGO_TEST_HUD_DUMP=$_d $*"
}
# hud_start <session> <columns> <config file> <dump file> [NAME=value ...]: `zygo claude --passthrough`
# in a detached session of the private tmux server, <columns> x 50, in the linked worktree.
hud_start() {
  _s=$1 _w=$2; shift 2
  tmux_own "$_s"
  echo "cd $WT && env -i $(hud_env "$@") $ZYGO_BIN claude --passthrough; echo hud-exit:\$?; sleep 3" >"$HUD/tmux-$_s.sh"
  tmux new-session -d -s "$_s" -x "$_w" -y 50 -c "$WT" "/bin/sh $HUD/tmux-$_s.sh"
}
pane() { tmux capture-pane -p -t "=$1:" 2>/dev/null || true; }
# wait_until <seconds> <command>: poll every 0.5 s; 1 on timeout.
wait_until() {
  _n=$(($1 * 2)); shift
  _i=0
  until "$@"; do _i=$((_i + 1)); [ "$_i" -lt "$_n" ] || return 1; sleep 0.5; done
}
# trust_line <session>: the pane line of the accept option of the folder trust question (empty: no question).
trust_line() { pane "$1" | grep -F "$TRUST_YES" | tail -n 1; }
# hud_trust <session>: one key step on the folder trust question. Claude Code 2.1.330 starts with the
# pointer on "No, exit", so Enter alone ends the session. The pointer is not on the accept line: Down.
# The pointer is on the accept line in two readings of the pane, 0.5 s apart: Enter. A key that Claude
# Code did not take is sent again by the next step. Prints the number of 0.5 s polls that the step used.
hud_trust() {
  case "$(trust_line "$1")" in
    "") echo 0 ;;
    *"$PTR"*)
      sleep 0.5
      case "$(trust_line "$1")" in *"$PTR"*) tmux send-keys -t "=$1:" Enter || true; sleep 1; echo 3 ;; *) echo 1 ;; esac ;;
    *) tmux send-keys -t "=$1:" Down || true; sleep 1; echo 2 ;;
  esac
}
# hud_wait <session>: the HUD row is in the pane within 60 s. Answers the folder trust question with
# yes (hud_trust). 1 when there is no HUD row; HUD_WHY then says why.
hud_wait() {
  _i=0
  until pane "$1" | grep -qF "$LOGO"; do
    _p=$(pane "$1")
    case "$_p" in
      *hud-exit:*)
        HUD_WHY="zygo claude ended: $(printf '%s\n' "$_p" | grep -E '^zygo: |hud-exit:' | sed -n 1p | python3 -I "$PREMISES_DIR/scrub.py" "$HUD" /dev/stdin | cut -c1-160)"
        return 1 ;;
      *"$TRUST_YES"*) _i=$((_i + $(hud_trust "$1"))) ;;
    esac
    _i=$((_i + 1)); [ "$_i" -lt 120 ] || { HUD_WHY="timeout: no HUD row in the pane within 60 s"; return 1; }
    sleep 0.5
  done
}
# hud_stop <session>: /exit, at most 10 s for the end, then kill the session.
hud_stop() {
  tmux_send "$1" /exit 2>/dev/null || true
  _i=0
  while tmux has-session -t "=$1" 2>/dev/null && ! pane "$1" | grep -qF hud-exit:; do
    _i=$((_i + 1)); [ "$_i" -lt 20 ] || break
    sleep 0.5
  done
  tmux kill-session -t "=$1" 2>/dev/null || true
}
# wsrc [<file>]: "<source> <width>" of each width-source line of a session log.
wsrc() { sed -n 's/.*width source \([^,]*\), width \([0-9]*\).*/\1 \2/p' "$@"; }
has_data() { pane "$1" | grep -F "$LOGO" | tail -n 1 | grep -qE 'ctx [0-9]'; }
log_has() { grep -q "$1" "$SD/session.log" 2>/dev/null; }
# wcount: the number of width-source lines in the session log; log_more <n>: there are more than <n>.
wcount() { wsrc "$SD/session.log" 2>/dev/null | grep -c . || true; }
log_more() { [ "$(wcount)" -gt "$1" ]; }

# ---- session a: compact, 200 columns, one short turn; then 70 columns; then 19 columns ----
A_WHY="" SD="" SGR='{"ok":false,"why":"no capture"}' R70='{"class":"none"}' R19='{"class":"none"}' WA="" W19=""
hud_start a 200 "$HUD/zygo.toml" "$HUD/dump-a.json" ZYGO_TEST_REFRESH_INTERVAL=2
if hud_wait a; then
  tmux_send a "Reply with the single word ready. Use no tool." || true
  wait_until 120 has_data a || echo "live-hud: no ctx numbers in the HUD within 120 s; the run goes on" >&2
  sleep 3
  tmux capture-pane -e -p -t "=a:" >"$HUD/pane-a-200.ans" 2>/dev/null || true
  SGR=$(hud_an sgr "$HUD/pane-a-200.ans" 2>/dev/null) || SGR='{"ok":false,"why":"analyzer error"}'
  cp "$HUD/dump-a.json" "$HUD/dump-a.keep.json" 2>/dev/null || true
  SD=$(jq -r '.env.ZYGO_SESSION_DIR // empty' "$HUD/dump-a.keep.json" 2>/dev/null || true)
  if [ -n "$SD" ] && [ -e "$SD/session.log" ]; then
    WA=$(wsrc "$SD/session.log" | sed -n 1p)
    tmux resize-window -t "=a:" -x 70 -y 50 || true
    # The hook logs a width line when the source or the width changes. The source at 70 columns is
    # the one that answers (Claude Code 2.1.330: COLUMNS; /dev/tty does not answer the hook).
    if wait_until 20 log_has 'width source [^,]*, width 70$'; then sleep 3; pane a >"$HUD/pane-a-70.txt"; fi
    N19=$(wcount)
    tmux resize-window -t "=a:" -x 19 -y 50 || true
    # 19 is below the 20-column limit of the hook, so a new line names the source that answers then.
    if wait_until 20 log_more "$N19"; then W19=$(wsrc "$SD/session.log" | sed -n "$((N19 + 1))p"); sleep 3; pane a >"$HUD/pane-a-19.txt"; fi
    cp "$SD/session.log" "$HUD/session-a.log" 2>/dev/null || true
    # The expected rows: the hook run by hand with no terminal, on the session's own data.
    if [ -s "$HUD/pane-a-70.txt" ]; then
      hud_an expect "$HUD/dump-a.keep.json" "$ZYGO_BIN" "$WT" 70 "$HUD/expect-70.txt" >/dev/null 2>&1 || true
      R70=$(hud_an fit "$HUD/pane-a-70.txt" "$HUD/expect-70.txt" 70 2>/dev/null) || R70='{"class":"none"}'
    fi
    if [ -s "$HUD/pane-a-19.txt" ]; then
      case "$W19" in "COLUMNS "*) _c=${W19#COLUMNS } ;; *) _c=none ;; esac
      hud_an expect "$HUD/dump-a.keep.json" "$ZYGO_BIN" "$WT" "$_c" "$HUD/expect-19.txt" >/dev/null 2>&1 || true
      R19=$(hud_an fit "$HUD/pane-a-19.txt" "$HUD/expect-19.txt" 19 2>/dev/null) || R19='{"class":"none"}'
    fi
  fi
else
  A_WHY=$HUD_WHY
fi
hud_stop a

# ---- session b: verbose, 200 columns, no turn ----
B_WHY="" TWO='{"rows":0,"logo_col":-1,"refl_col":-1}'
hud_start b 200 "$HUD/zygo-verbose.toml" "$HUD/dump-b.json"
if hud_wait b; then
  sleep 3
  tmux capture-pane -e -p -t "=b:" >"$HUD/pane-b-200.ans" 2>/dev/null || true
  TWO=$(hud_an two "$HUD/pane-b-200.ans" 2>/dev/null) || TWO='{"rows":0,"logo_col":-1,"refl_col":-1}'
else
  B_WHY=$HUD_WHY
fi
hud_stop b

# ---- session c: refreshInterval 2 s and a status fetch that starts 5 s late; 45 s of process samples ----
python3 -I "$PY/hud_poll.py" "$HUD/events.json" "$ZYGO_BIN" 45 "$HUD/dump-c.json" &
LIVE_PIDS=$!
hud_start c 200 "$HUD/zygo.toml" "$HUD/dump-c.json" ZYGO_TEST_REFRESH_INTERVAL=2 ZYGO_TEST_FETCH_DELAY_MS=5000
hud_wait c || true
wait "$LIVE_PIDS" || true
LIVE_PIDS=""
hud_stop c
P12=$(hud_an ph12 "$HUD/events.json" 3 2>/dev/null) || P12='{"class":"none","runs":0}'

# ---- session d: outside tmux, on a pseudo-terminal of 100 columns, no turn ----
WP=""
# shellcheck disable=SC2046
python3 -I "$PY/hud_pty.py" "$HUD/dump-d.json" "$HUD/session-d.log" 100 60 "$WT" \
  "$(command -v env)" -i $(hud_env "$HUD/zygo.toml" "$HUD/dump-d.json") "$ZYGO_BIN" claude --passthrough &
LIVE_PIDS=$!
D_RC=0
wait "$LIVE_PIDS" || D_RC=$?
LIVE_PIDS=""
case "$D_RC" in
  0) D_WHY="" ;;
  3) D_WHY="pseudo-terminal session: no dump file from the hook within 60 s" ;;
  4) D_WHY="pseudo-terminal session: no copy of session.log" ;;
  5) D_WHY="pseudo-terminal session: Claude Code showed the folder trust question, which the script does not answer there" ;;
  *) D_WHY="pseudo-terminal session: the driver ended with status $D_RC" ;;
esac
[ -s "$HUD/session-d.log" ] && WP=$(wsrc "$HUD/session-d.log" | sed -n 1p)

# ---- the LIVE lines of the passthrough parts ----
j() { printf '%s' "$1" | jq -r "$2"; }
if [ -n "$A_WHY" ]; then
  line PH1 failed "$A_WHY"
elif [ "$(j "$SGR" .ok)" = true ]; then
  line PH1 verified "capture-pane -e (tmux $TMUX_VER): stripe fg $(j "$SGR" '.stripe | join(",")'), logo bold fg 15, worktree name fg 172 (38;5;n)"
else
  line PH1 failed "16-color part $(j "$SGR" '.ok16 // false'), 256-color part $(j "$SGR" '.ok256 // false'): $SGR"
fi
if [ -n "$B_WHY" ]; then
  line PH2 failed "$B_WHY"
elif [ "$(j "$TWO" .rows)" = 2 ]; then
  line PH2 verified "verbose shows two rows: the logo row, then the reflection row (tmux $TMUX_VER, 200 columns)"
else
  line PH2 failed "verbose shows $(j "$TWO" .rows) row(s): $TWO"
fi
C19=$(j "$R19" .class)
T3="at 70 columns the row is $(j "$R70" .class) ($(j "$R70" '.shown_cells // "-"') of $(j "$R70" '.expected_cells // "-"') cells shown); at 19 columns (width source ${W19:-none}) a row of $(j "$R19" '.expected_cells // "-"') cells is $C19"
if [ -n "$A_WHY" ]; then
  line PH3 failed "$A_WHY"
elif [ "$C19" = cut ]; then
  line PH3 verified "cut: $T3"
elif [ "$C19" = wrapped ]; then
  line PH3 failed "wrapped: $T3"
else
  line PH3 failed "not observed: $T3"
fi
CA=$(jq -r '.env.COLUMNS // "unset"' "$HUD/dump-a.keep.json" 2>/dev/null || echo unset)
CP=$(jq -r '.env.COLUMNS // "unset"' "$HUD/dump-d.json" 2>/dev/null || echo unset)
GAP=$(j "$R19" 'if .gap == null then "unknown" else (.indent + .gap | tostring) end')
T4="(a) width source in tmux at 200 columns: ${WA:-none}; on a pseudo-terminal of 100 columns: ${WP:-none}; (b) COLUMNS in the hook: tmux $CA, pseudo-terminal $CP; (c) status area = pane width - $GAP at 19 columns (indent $(j "$R19" '.indent // "-"'), right gap $(j "$R19" '.gap // "-"')), row at 70 columns: $(j "$R70" .class)"
if [ "$WA" = "tty 200" ] && [ "$WP" = "tty 100" ] && [ "$CA" != unset ] && [ "$CP" != unset ] && [ "$GAP" = 0 ]; then
  line PH4 verified "$T4"
else
  line PH4 failed "${A_WHY:+$A_WHY; }${D_WHY:+$D_WHY; }$T4"
fi
if [ ! -s "$HUD/dump-a.keep.json" ]; then
  line PH5 failed "${A_WHY:-no dump file from the hook}"
elif jq -e '(.effort | type) as $t | $t == "string" or ($t == "object" and (.effort.level | type) == "string")' "$HUD/dump-a.keep.json" >/dev/null; then
  line PH5 verified "stdin effort is $(jq -c .effort "$HUD/dump-a.keep.json") ($(jq -r '.effort | type' "$HUD/dump-a.keep.json")); stdin keys: $(jq -r '.stdin_keys | join(" ")' "$HUD/dump-a.keep.json")"
else
  line PH5 failed "stdin effort is $(jq -c .effort "$HUD/dump-a.keep.json") ($(jq -r '.effort | type' "$HUD/dump-a.keep.json")); stdin keys: $(jq -r '.stdin_keys | join(" ")' "$HUD/dump-a.keep.json")"
fi
if [ -n "$B_WHY" ]; then
  line PH9 failed "$B_WHY"
elif [ "$(j "$TWO" .rows)" = 2 ] && [ "$(j "$TWO" .logo_col)" -ge 0 ] && [ "$(j "$TWO" .logo_col)" = "$(j "$TWO" .refl_col)" ]; then
  line PH9 verified "logo and reflection start at column $(j "$TWO" .logo_col) of the pane (tmux $TMUX_VER); the glyphs of the font are not checked"
else
  line PH9 failed "logo column $(j "$TWO" .logo_col), reflection column $(j "$TWO" .refl_col), rows $(j "$TWO" .rows)"
fi
case "$(j "$P12" .class)" in
  kills) _t="Claude Code ends the run early" ;;
  waits) _t="Claude Code waits for the run before it starts the next" ;;
  overlaps) _t="Claude Code starts the next run while the first still goes, and does not end the first" ;;
  *) _t="" ;;
esac
if [ -n "$_t" ]; then
  line PH12 verified "observation, refreshInterval 2 s: the first slow run lived $(j "$P12" .life) s of its 3 s bound; the next run started $(j "$P12" '.next_start // "never"') s after its start: $_t; $(j "$P12" .runs) runs in 45 s"
else
  line PH12 failed "not observed: no status line run of 1 s or more in 45 s ($(j "$P12" .runs) runs seen)"
fi

# ---- fixtures of the passthrough parts (RUN is still the hud run, so scrub maps its paths) ----
# fx <file> <fixture name>: a scrubbed copy, when the file has content.
fx() { if [ -s "$1" ]; then scrub "$1" >"$FX/$2"; fi; }
mask='.env.ZYGO_SESSION_DIR |= (if . == null then null else "[set]" end) | .env.ZYGO_URL |= (if . == null then null else "[set]" end)'
if [ -s "$HUD/pane-a-200.ans" ]; then hud_an rows "$HUD/pane-a-200.ans" 1 raw >"$HUD/fx-compact-200.txt" || true; fi
if [ -s "$HUD/pane-b-200.ans" ]; then hud_an rows "$HUD/pane-b-200.ans" 2 raw >"$HUD/fx-verbose-200.txt" || true; fi
if [ -s "$HUD/pane-a-70.txt" ]; then hud_an rows "$HUD/pane-a-70.txt" 4 plain >"$HUD/fx-compact-70.txt" || true; fi
if [ -s "$HUD/pane-a-19.txt" ]; then hud_an rows "$HUD/pane-a-19.txt" 8 plain >"$HUD/fx-compact-19.txt" || true; fi
if [ -s "$HUD/dump-a.keep.json" ]; then jq "$mask" "$HUD/dump-a.keep.json" >"$HUD/fx-dump-tmux.json" || true; fi
if [ -s "$HUD/dump-d.json" ]; then jq "$mask" "$HUD/dump-d.json" >"$HUD/fx-dump-pty.json" || true; fi
{
  [ -e "$HUD/session-a.log" ] && wsrc "$HUD/session-a.log" | sed 's/^/tmux session a (200, then 70, then 19 columns): /'
  [ -e "$HUD/session-d.log" ] && wsrc "$HUD/session-d.log" | sed 's/^/pseudo-terminal, 100 columns: /'
  true
} >"$HUD/fx-width.txt"
jq -n --argjson sgr "$SGR" --argjson two "$TWO" --argjson c70 "$R70" --argjson c19 "$R19" \
  '{sgr: $sgr, two: $two, "70": $c70, "19": $c19}' >"$HUD/fx-fit.json" || true
if [ -s "$HUD/events.json" ]; then
  jq -n --slurpfile ev "$HUD/events.json" --argjson r "$P12" '{events: $ev[0], result: $r}' >"$HUD/fx-ph12.json" || true
fi
fx "$HUD/fx-compact-200.txt" p-hud-1-compact-200.txt
fx "$HUD/fx-verbose-200.txt" p-hud-1-verbose-200.txt
fx "$HUD/fx-compact-70.txt" p-hud-1-compact-70.txt
fx "$HUD/fx-compact-19.txt" p-hud-1-compact-19.txt
fx "$HUD/expect-70.txt" p-hud-1-expect-70.txt
fx "$HUD/expect-19.txt" p-hud-1-expect-19.txt
fx "$HUD/fx-dump-tmux.json" p-hud-1-dump-tmux.json
fx "$HUD/fx-dump-pty.json" p-hud-1-dump-pty.json
fx "$HUD/fx-width.txt" p-hud-1-width.txt
fx "$HUD/fx-fit.json" p-hud-1-fit.json
fx "$HUD/fx-ph12.json" p-hud-1-ph12.json

# ---- C7: mock upstream and a temporary HOME, as P-N28-1; no model request ----
live_run c7
start_mock
mock_config "$RUN/zygo.toml"
printf '[statusline]\nstyle = "verbose"\n' >>"$RUN/zygo.toml"
LIVE_VARS="ZYGO_NERD_FONT=1 ZYGO_NO_UPDATE_CHECK=1"
tmux_zygo c7 claude --compact 777k
tmux_wait_text c7 "$LOGO" 60 || true
tmux_send c7 "tracer-hud say hi" || true
c7_seen() { [ -n "$(body_has 'tracer-hud' "$RUN"/rec/[0-9]*.json 2>/dev/null)" ]; }
wait_until 90 c7_seen || true
sleep 5
pane c7 >"$RUN/pane.txt"
tmux kill-session -t "=c7" 2>/dev/null || true
stop_mock
N_ALL=$(find "$RUN/rec" -name '[0-9]*.json' | grep -c . || true)
N_TR=$(body_has 'tracer-hud' "$RUN"/rec/[0-9]*.json | grep -c . || true)
LEAK=$(body_has "$LOGO|$REFL|777k|777000" "$RUN"/rec/[0-9]*.json | sed 's#.*/##' | tr '\n' ' ')
if grep -qF "$LOGO" "$RUN/pane.txt"; then SHOWN=yes; else SHOWN=no; fi
if [ "$SHOWN" = yes ] && [ "$N_TR" -gt 0 ] && [ -z "$LEAK" ]; then
  line C7 verified "HUD shown (verbose, --compact 777k); the logo, the reflection, 777k and 777000 are in none of $N_ALL recorded requests ($N_TR hold the tracer prompt); mock upstream"
else
  line C7 failed "HUD shown: $SHOWN; recorded requests: $N_ALL; with the tracer prompt: $N_TR; requests that hold HUD text: ${LEAK:-none}; mock upstream"
fi
jq -n --argjson all "$N_ALL" --argjson tr "$N_TR" --arg leak "$LEAK" \
  '{requests: $all, tracer_requests: $tr, markers: ["logo (U+1D607 U+1D606 U+1D5F4 U+1D5FC)", "reflection (z U+028E U+0183 o)", "777k", "777000"], requests_with_a_marker: ($leak | split(" ") | map(select(. != "")))}' >"$RUN/fx-c7.json" || true
hud_an rows "$RUN/pane.txt" 2 plain >"$RUN/fx-c7-pane.txt" || true
fx "$RUN/fx-c7.json" p-hud-1-c7.json
fx "$RUN/fx-c7-pane.txt" p-hud-1-c7-pane.txt

# ---- the run record and the eight lines ----
jq -n --arg d "$(date +%Y-%m-%d)" --arg cc "$LIVE_CC_VER" --arg t "$TMUX_VER" --arg os "$(uname -s)" \
  '{date: $d, claude_code: $cc, tmux: $t, os: $os}' >"$HUD/run.json"
fx "$HUD/run.json" p-hud-1-run.json
fx "$HUD/lines.txt" p-hud-1-results.txt
live_done "$FX"
````
