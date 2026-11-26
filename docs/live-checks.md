# Live checks (node N38)

Checks on the real pinned agents, one dated result per check. A pinned agent version is a minimum (operator, 2026-11-21). Spec: `docs/specs/2026-11-18-zygo-design.md` §20 #16b, #21, #28, #40; §19 P1; §7.6; the N24, N25, N14, N15, N28, N33, N34, N35 notes; `docs/specs/2026-11-21-statusline-hud-design.md` H§12 (P-HUD-1, and the PRB-1 probe); `docs/specs/2026-11-20-workspaces-design.md` W§17 (node P2); `docs/specs/2026-11-24-session-text-design.md` T§12.2 (P3-15); `docs/specs/2026-11-24-decision-records-design.md` D§18 (PD2, PD3); `docs/specs/2026-11-22-workspace-mail-design.md` M§13 #12 (M12).

Claude Code: 2.1.328 or later
OpenCode: 1.18.16 or later

## Method

- Harness: `scripts/premises/live-*.sh`, on the N2 harness (`lib.sh`, `mock_upstream.py`, `steps.py`, `hook_log.py`, `scrub.py`) plus `live.sh`. They run by hand only: no CI job and no Rust test calls them, and `run-all.sh` does not.
- Setup, once per shell: `cargo build -p zygo --release` (the scripts use `target/release/zygo`; a debug build misses the 200 ms secret-redaction deadline on an interactive Claude Code body and answers 503: `sh scripts/premises/live-deadline.sh`, mock upstream, 2026-11-19, `fixtures/claude-code/live/deadline-debug.json` shows the first `main` request at 503 after 210 ms, then a second `main` request at 200 after 80 ms, and `fixtures/claude-code/live/deadline-release.json` the same `main` request at 200 after 15 ms), then Claude Code 2.1.328 or later first on `PATH` (the commands below link 2.1.328): `export N2_SCRATCH=$(mktemp -d)`; `mkdir -p "$N2_SCRATCH/bin" && ln -sf ~/.local/share/claude/versions/2.1.328 "$N2_SCRATCH/bin/claude" && export PATH="$N2_SCRATCH/bin:$PATH"` (when that version is gone: `npm install --prefix "$N2_SCRATCH/npm" @anthropic-ai/claude-code@2.1.328` and link `"$N2_SCRATCH/npm/node_modules/.bin/claude"`). Each script stops unless `claude --version` and `opencode --version` are at or above the minimum versions above.
- Isolation: every run except #28, the passthrough parts of P-HUD-1, the Claude Code sessions of PRB-1 and part `c5l` of P2 (the real `HOME` and the caller's Claude Code config folder) gets a temporary `HOME` (`CLAUDE_CONFIG_DIR`, `XDG_*` under it), `ZYGO_CONFIG` = the run's own config file, and an environment emptied of every variable except the provider keys the check names. The P2 tmux panes are an exception: they get the temporary `HOME` and an emptied environment, but no `ZYGO_CONFIG`, since no zygo runs in them. The Claude Code panes get `CLAUDE_CONFIG_DIR`, `XDG_CONFIG_HOME`, `XDG_DATA_HOME` and `XDG_CACHE_HOME` under that `HOME` and no `XDG_STATE_HOME`, and they pass `ANTHROPIC_BASE_URL` (the mock), the dummy `ANTHROPIC_API_KEY`, `CLAUDE_CODE_GATEWAY_HINT_HEADERS=1` and `DISABLE_AUTOUPDATER=1`. The OpenCode TUI pane gets `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_CACHE_HOME` and `XDG_STATE_HOME` under that `HOME`, `OPENCODE_DISABLE_AUTOUPDATE=1` and the plugin's log path, and reaches the mock through its config file. The P2 print-mode runs on the mock and the OpenCode commands use the isolated environment of this bullet, the print-mode runs with the mock URL, the dummy key and `CLAUDE_CODE_GATEWAY_HINT_HEADERS=1` added. Auto-update is off (`DISABLE_AUTOUPDATER`, `OPENCODE_DISABLE_AUTOUPDATE`).
- Upstream per row: **mock** = `mock_upstream.py` behind zygo, answering only a dummy key built at run time, with authored replies (not model output); the P2 parts other than `c5l` point the agents at the mock directly, with no zygo between (PW7's API error: an upstream of the script that answers HTTP 400); **real** = the vendor API with the owner's key; #28, P-HUD-1 (all parts but C7, which uses the mock), PRB-1 and part `c5l` of P2 use the claude.ai login Claude Code already stores; N35-D reaches GitHub only.
- Keys: only in the environment, read at a hidden prompt; never on a command line, never in a file. zygo reads them as `env:` references; the strict and effort checks hand them to `curl` on stdin (`-H @-`). An interactive (tmux) run refuses to start when a key is named.
- Cleanup: each script removes its scratch tree (raw requests, hook logs, panes, debug files, usage copies) on exit, after the scrubbed fixtures are written. The traps (EXIT, INT, TERM, HUP, PIPE) also kill the mock and web servers, the background processes and tmux sessions the script started, and the whole process group of a command under the wall-clock bound; tmux targets are exact (`=name`), and a session the script did not start is left alone. These older cleanup paths were tested under `dash`: `dash scripts/premises/live-selftest.sh` (local only, no agent, no key) checks Ctrl-C, TERM and HUP during a bounded command, the `--scan-only` failure cases of #28 (links outside the zygo dirs, the strict liveness probe), the scan cases below, and the tmux targets on a private tmux server. It puts stub `claude`, `opencode` and `zygo` programs first on `PATH` and a dead loopback port in `MOCK_URL`, and stops unless the stubs resolve, so it cannot start a real agent; run it only on the scripts of the same checkout, never on older versions. The node PRB-2 changes ran under bash only (`dash` was not installed): the minimum-version rule (`sh scripts/premises/ver_test.sh`), and the scratch dir that `lib.sh` makes, removed after exit 0 and kept after an error, INT, TERM, HUP or PIPE, also through `live_end`, with a failed removal named on stderr (`sh scripts/premises/lib_test.sh`).
- Fixtures pass `scrub.py` (run paths, home, user name, host name, e-mail addresses, header credentials and plain-text `x-api-key` values, bearer words, key forms, organization and project ids, zygo tokens, GitHub tokens, JWTs, Windows user paths, names with spaces included); `python3 -I scripts/premises/scrub_test.py` tests each rule. `scripts/check-premises.sh` scans `fixtures/`, the live scripts and their helpers, this file, and the spec and conformance lines that name N38. Keys, ids, tokens, the user name and host names are also found right after a JSON backslash escape (`\n`, `\t`, `\uXXXX`) in nested JSON text; host names come from `hostname` and, on macOS, `scutil` (`LocalHostName`, `ComputerName`); a host name shorter than 4 characters or `localhost` is skipped with a note.

## Owner-run checks

Each needs a provider key. Enter keys at a hidden prompt, never as a literal on the command line:

```sh
printf 'Anthropic key: '; read -rs ANTHROPIC_API_KEY; echo; export ANTHROPIC_API_KEY
printf 'OpenAI key: '; read -rs OPENAI_API_KEY; echo; export OPENAI_API_KEY
```

Then, from the checkout, after the setup above (each prints `LIVE <id> <result> (agent <version>): …` lines to paste into the table, and writes its fixtures):

- **21, P1**: `sh scripts/premises/live-window.sh`. Environment: the Anthropic API key. Pass: 21: a `main` row over 40,000 prompt tokens carries `context_signal`, the next non-auxiliary row has `compaction = 1`, and a `PreCompact` hook ran with `trigger: auto` (no crossing row: `not-crossed`, not a fail). P1: 21 passes, and the `mcp__zygo__compact` call gives a `context_compact` row, then a compaction row, then a `main` row. Fixtures `21-usage.json`, `p1-tooluse-usage.json`.
- **16b**: `LIVE_SMALL_MODEL=<model id> sh scripts/premises/live-16b.sh`. Environment: the OpenAI API key; `LIVE_SMALL_MODEL` = an OpenAI-dialect model whose real input limit is at most 128,000 tokens; optional `LIVE_SMALL_BASE_URL` (default `https://api.openai.com`). The `zygo-code` subagent reads 26 files of about 30,000 characters (at least 130,104 tokens in all, the fewest files that cross 128,000). Pass: exit 0; `prompt is too long` in Claude Code's debug file (read once, deleted at exit); one subagent's rows show a 400, then `compaction = 1`, then a 200. Fixture `16b-usage.json`.
- **N14-S, N14-E**: `sh scripts/premises/live-strict.sh`. Environment: both keys. Pass: N14-S: every rewritten schema (one per built-in `refuse` rule, captured at the mock) gets HTTP 200 in strict mode from its vendor: Messages (`claude-haiku-5-5`); Responses and chat (`gpt-6.2-astra`). N14-E: Responses `reasoning.effort` and chat `reasoning_effort` get HTTP 200 for `low`, `medium`, `high`, `xhigh`, `max`. Fixtures `n14-strict.json`, `n14-effort.json`. `sh scripts/premises/live-strict.sh --capture-only` runs the mock capture alone, with no key and no network, and prints `live: captured <n> of 18 rewritten schemas:` and the `<family>-<code>.<api>` name of each schema found by its `(family-code)` marker (2026-11-19: 18 of 18).
- **R-RT**: `sh scripts/premises/live-rt.sh`. Environment: both keys. Pass: per vendor (`gpt-6.2-sol` translated, then `claude-haiku-5-5` native): exit 0, at least three `main` rows with status 200, no row with `thinking_dropped`. Fixture `rt-usage.json`.

Spend bounds and cost estimates (prices from `crates/zygo-config/data/catalog.toml`, per million tokens; Claude Code's own prompt and tools are taken as about 20,000 tokens a request; no prompt caching counted, so the real cost is lower):

| Owner script | Bound | Estimate |
|---|---|---|
| `live-window.sh` | `--max-turns 4` and 600 s per run (two runs) | `claude-haiku-5-5` ($0.10 input, $0.50 output): about 250,000 input and 10,000 output tokens, about $0.03 |
| `live-16b.sh` | lead `--max-turns 3`; 26 subagent turns (one per file); 1800 s | the owner's model: about 1,800,000 input tokens (the subagent's context grows to the 128,000 limit over about 24 turns, then a few short turns after compaction) and 10,000 output tokens; about $3.70 at $2.00/$10.00 (the price of `gpt-6.2-sol`) |
| `live-strict.sh` | no agent; 46 single requests, `max_tokens` 64 or 32, curl `-m 60` each | 16 Messages requests on `claude-haiku-5-5` (under $0.01) and 30 Responses or chat requests on `gpt-6.2-astra` ($8.00 input, $40.00 output, about $0.12) |
| `live-rt.sh` | `--max-turns 5` and 600 s per vendor | `gpt-6.2-sol` ($2.00, $10.00): about 100,000 input and 2,000 output tokens, about $0.22; `claude-haiku-5-5`: about $0.01 |

Without its key each script prints `live: <NAME> is not set; this check calls the real provider` and exits 1 before any network call.

When done, remove the keys from the shell and delete the scratch folder (the pinned link, `results.txt`):

```sh
unset ANTHROPIC_API_KEY OPENAI_API_KEY LIVE_SMALL_MODEL LIVE_SMALL_BASE_URL
rm -rf "$N2_SCRATCH"
```

## Windows checks

For a Windows tester (x86_64 or arm64); N38 closes without them. Every command is PowerShell 7 (`pwsh`), run from the zygo checkout. The checks use `claude-haiku-5-5` for every role ($0.10 input, $0.50 output per million tokens); all three cost under $0.05.

1. Install Rust (`rustup`), Node 20 or later, Git for Windows, SQLite (`winget install SQLite.SQLite`, gives `sqlite3`) and Claude Code 2.1.328 or later (the pinned version: `npm install -g @anthropic-ai/claude-code@2.1.328`) (this gives the `claude.cmd` shim). Open a new PowerShell after the installs.
2. Build zygo as a release build and put it first on `PATH` for this session:
   ```powershell
   cargo build -p zygo --release
   $env:Path = "$PWD\target\release;" + $env:Path
   zygo --version; claude --version   # claude prints 2.1.328 (Claude Code) or a later version
   ```
3. **W-SHELL, part 1** (no zygo, and no key in the session: a key in the environment outranks `apiKeyHelper`, spec §8.4, so the helper would not run): `Remove-Item Env:ANTHROPIC_API_KEY -ErrorAction SilentlyContinue`. Write `probe.json`. Its two commands parse under bash, cmd, Windows PowerShell 5.1 and PowerShell 7 (no `&&`: the pipe `|` separates the two parts in all four) and print one marker per shell; the helper appends them to `zygo-probe-key.txt` in the current folder and prints `probe-key`:
   ```json
   {"apiKeyHelper": "echo bash=$0 cmd=%COMSPEC% ps=$($PSVersionTable.PSVersion) >> zygo-probe-key.txt | echo probe-key",
    "statusLine": {"type": "command", "command": "echo bash=$0 cmd=%COMSPEC% ps=$($PSVersionTable.PSVersion)"}}
   ```
   What each shell prints: bash: `bash=` shows `bash` or a path, `cmd=%COMSPEC%` stays literal, `ps=` is empty; cmd: `cmd=` shows the path of `cmd.exe`, `bash=$0` stays literal; PowerShell: `ps=` shows the version (`5.1.…` for Windows PowerShell, `7.…` for PowerShell 7), and each word may be on its own line. Save the path: `$saved = $env:Path`. Run `claude --settings probe.json` (interactive). With no key in the session, Claude Code runs the helper and gets `probe-key`; it is not a real key, so the first request fails, which is expected. Read the status line, quit with `/exit`, then `Get-Content zygo-probe-key.txt`. Record `Get-Command bash` (Git for Windows present or not). Then remove Git for Windows from `PATH`: `$env:Path = ($env:Path -split ';' | Where-Object { $_ -notmatch '\\Git\\' }) -join ';'`; if Claude Code still starts, `Remove-Item zygo-probe-key.txt`, repeat and record. Restore Git for Windows: `$env:Path = $saved`.
4. Set the Anthropic API key for the session from a hidden prompt, never as a literal: `$env:ANTHROPIC_API_KEY = Read-Host -MaskInput 'Anthropic key'`.
5. Cheap-model config, kept out of the tester's own zygo config:
   ```powershell
   $roles = 'lead','design','plan','triage','review','code','read'
   ($roles | ForEach-Object { "[role.$_]`nmodel = `"claude-haiku-5-5`"`n" }) -join "`n" | Set-Content -Encoding utf8 live-win.toml
   $env:ZYGO_CONFIG = "$PWD\live-win.toml"
   zygo config check
   ```
6. **W-SHELL, part 2**: `zygo claude` (interactive) shows the stripe, the logo (`𝘇𝘆𝗴𝗼` or `zygo`) and `ctx` in its status line. On Windows the plain icon set is the default (AU2), so the logo is `zygo` unless `ZYGO_NERD_FONT=1`. Pass (parts 1 and 2): in each setup of part 1 the shell is bash or cmd (zygo's double-quoted path runs there), and the status line of part 2 shows the stripe, the logo (`𝘇𝘆𝗴𝗼` or `zygo`) and `ctx`. Fail: PowerShell (a quoted path needs `&` there; conformance A-1, A-9).
7. **W-SHIM**: `zygo claude -- -p --max-turns 1 "Reply with the single word ready."` gives a reply, exit 0 (`$LASTEXITCODE`), and `sqlite3 "$env:LOCALAPPDATA\zygo\usage.db" "select status, agent_version from requests order by ts desc limit 1"` prints `200|2.1.328`.
8. **W-AGENTS**: `zygo claude -- -p --max-turns 1 "List the names of the subagents you can use, one per line."`: the reply names `zygo-code` and `zygo-review`.
9. Clean up: `Remove-Item Env:ANTHROPIC_API_KEY, Env:ZYGO_CONFIG; Remove-Item probe.json, zygo-probe-key.txt, live-win.toml`.
10. Report three lines `W-<id> pass|fail: <shell or output>` with the date to the owner; the rows below are then filled.

## P-HUD-1 (status line HUD)

Node LIV-1; `docs/specs/2026-11-21-statusline-hud-design.md` H§12. From the checkout, on Linux or macOS, with tmux 3.2 or later, `python3`, `jq` and `git`, Claude Code logged in, and `claude` and `opencode` on `PATH` (`lib.sh` checks both; the script starts no OpenCode):

```sh
cargo build -p zygo --features test-hooks
sh scripts/premises/live-hud.sh --zygo target/debug/zygo
```

- The build needs the `test-hooks` feature: the script uses the seams `ZYGO_TEST_HUD_DUMP`, `ZYGO_TEST_FETCH_DELAY_MS` and `ZYGO_TEST_REFRESH_INTERVAL`. `sh scripts/premises/live-hud.sh --selfcheck` tests the script's analyzer with no agent.
- It prints one `LIVE <id> verified|failed (agent <version>): …` line for each of PH1, PH2, PH3, PH4, PH5, PH9, PH12 and C7, writes `fixtures/claude-code/hud/p-hud-1-*` and exits 0. A failed line does not change the exit status.
- Parts PH1 to PH12 run `zygo claude --passthrough` four times on the stored login: three sessions in a private tmux server (`tmux -L zygo-hud-<pid>`, no tmux config file, 200 x 50; one of them is then made 70 and 19 columns wide) and one on a pseudo-terminal of 100 columns. They use the real `HOME` and the caller's `CLAUDE_CONFIG_DIR`. The cwd is a linked git worktree in the scratch folder. The script answers the folder trust question of Claude Code with yes. Claude Code 2.1.330 keeps the answer as a `projects` entry for the main repository of that worktree (`<scratch>/live-<pid>/hud/repo`) in its `.claude.json`, a transcript folder for the worktree path and a `memory` folder for the repository path under `<config dir>/projects/`; the script does not delete them. zygo keeps the usage rows.
- Spend: one short answer on the login (`Reply with the single word ready. Use no tool.`), and Claude Code's own start requests in the four sessions. No tool runs with a permission of the script (no `--yolo`).
- Part C7 runs on the mock upstream with a temporary `HOME`, as P-N28-1: a passthrough request cannot be recorded.
- Bounds: 60 s for each session start, 120 s for the answer, 20 s for each width change, 45 s of process samples, 90 s for the mock request, 10 s for each session end. A bound that passes gives `failed` with the reason for the ids of that part; the script goes on.
- The traps end the private tmux server, the pseudo-terminal session, the mock and the background processes, and remove the scratch tree.

## PRB-1 (status line probe)

Node PRB-1; `docs/specs/2026-11-21-statusline-hud-design.md` H§12 ("Probe first"). From the checkout, on Linux or macOS, with tmux 3.2 or later, `python3` and `jq`, Claude Code logged in, and `claude` and `opencode` on `PATH`:

```sh
sh scripts/premises/live-hud-probe.sh
```

- No agent and no login: `python3 -I scripts/premises/hud_probe_test.py` tests the probe command and its analyzer; `sh scripts/premises/live-hud-probe_test.sh` runs the whole script with stub `claude` and `opencode` programs.
- It prints one `LIVE <id> verified|failed|observed (agent <version>): …` line for each of PH1, PH2, PH3, PH4, PH5, PH6, PH7, PH9, PH11, PH12 and PH13, writes `fixtures/claude-code/hud/prb-1-*` and exits 0. A failed line does not change the exit status.
- Claude Code runs as `claude --settings <run file>`, not through zygo, two times on the stored login: in a private tmux server (`tmux -L zygo-prb-<pid>`, no tmux config file, 200 x 50) and on a pseudo-terminal of 100 columns. The settings file holds only the probe `statusLine` command (`scripts/premises/hud_probe.py`, `refreshInterval` 2), whose output changes by phase: the two H§6.3 `printf` outputs, four ruler rows, an early exit, a slow run. They use the real `HOME` and the caller's `CLAUDE_CONFIG_DIR`; the cwd is a git repository in the scratch folder. The script answers the folder trust question of Claude Code with yes; Claude Code keeps the answer for that path in its config folder (2.1.330, no prompt typed: a trust entry in its config file, no project folder), and the script does not delete it.
- Spend: no prompt is typed; Claude Code sends only its own start requests (one auxiliary request each start on 2.1.330).
- PH13 runs `opencode --help`, `opencode debug --help` and `opencode debug config --pure` with a temporary `HOME`; no model request.
- Bounds: 60 s for each session start, 10 s for each phase change, 12 s and 20 s of captures for the early and slow phases, 60 s for the pseudo-terminal session, 30 s for each OpenCode call, 10 s for each session end. A bound that passes gives `failed` with the reason for the ids of that part; the script goes on.
- The traps end the private tmux server, the pseudo-terminal session and the background processes, and remove the scratch tree.

## Workspace agent premises (node P2)

Node P2; `docs/specs/2026-11-20-workspaces-design.md` W§17 (PW1, PW2, PW3, PW4, PW5, PW7, PW9, PW10, PW16). From the checkout, with tmux, `jq`, `python3` and `perl` (it ran on Linux; macOS not run), Claude Code logged in, and `claude` and `opencode` on `PATH` (OpenCode installs to `$HOME/.opencode/bin`, which the caller adds). PW3 also needs a second Claude Code at or above the pinned version and below the one on `PATH`: `PW3_OLD_CLAUDE=<path>`, else the highest such version file beside the program on `PATH`.

```sh
PATH="$HOME/.opencode/bin:$PATH" sh scripts/premises/live-workspace.sh
```

- No agent: `sh scripts/premises/workspace_test.sh` tests the record helpers (`workspace_lib.sh`) and the analyses (`workspace_analyze.py`) on stub data.
- It prints one `LIVE <id> verified|failed (agent <version>): …` line for each of the nine ids, writes `fixtures/claude-code/workspace/` and `fixtures/opencode/workspace/` at the end of the full run. Exit status: 0, the run finished (a failed line does not change it); 3, a request at the mock carried a credential other than the dummy key (the mock writes `ABORT` and stops); 1, a tool or agent is missing or below its minimum version, a path holds a quote or a space, the mock did not start, or the fixtures could not be copied to the repository; 2, an unknown part name; 129, 130, 141 or 143 after HUP, INT, PIPE or TERM. With part names (`c1 c2 c5 c5l c7 oc`) it runs only those parts; the fixtures then stay in the scratch folder and the lines are not results.
- Parts `c1`, `c2`, `c5` and `c7` start Claude Code as an interactive TUI in panes of a tmux server of the script's own (`-S <socket in a private folder>`, `-f /dev/null`, 200 x 50) and type the prompts, `/clear`, `/exit` and Escape with `send-keys`. Part `c1` also makes four print-mode runs (PW16 from another cwd and with another config folder; PW3 on the older and then the newer Claude Code). Part `oc` runs OpenCode `run` and `session list` commands and one OpenCode TUI pane. These parts use a temporary `HOME`, the dummy key and the mock upstream (PW7's API error: an upstream of the script that answers HTTP 400): no login is read and no model request leaves the machine. OpenCode fetches its provider package from the package registry into the temporary `HOME`. PW9 sends SIGTERM to the idle agent process itself.
- Part `c5l` (sub-check 5.5) runs one print-mode turn of Claude Code on the stored login, with the real `HOME` and the caller's `CLAUDE_CONFIG_DIR` (else `~/.claude`), `--setting-sources project`, `--strict-mcp-config` and `--no-session-persistence`. Known side effect: each run of part `c5l` (a full run or a part run) leaves one empty folder `<config dir>/projects/<scratch repository path>/memory` (Claude Code 2.1.330) for the run's temporary repository (`<scratch>/live-<pid>/c5l/repo`). The path differs in each run, so the folders add up. P2 on 2026-11-22 left 6 such folders, named `projects/-tmp-<scratch>-live-<pid>-c5l-repo/memory`, in two config folders: 5 in `~/.claude/projects/` from the round 1 runs, which passed no `CLAUDE_CONFIG_DIR`, and 1 in the operator's own config folder (`CLAUDE_CONFIG_DIR`) from the round 2 run. A read-only listing of both `projects/` folders on 2026-11-22 showed the 6 folders, each `memory` folder empty. They stay in place. The script does not change or delete anything in the config folder. Note 5.6 of `fixtures/claude-code/workspace/pw5.txt` records two facts after the turn: whether the project folder `<config dir>/projects/<scratch repository path>` exists (it does not test the `memory` folder inside it), and whether the config file has a `projects` entry for the repository (2026-11-22: the folder yes, the entry no).
- Spend: part `c5l` only, one prompt on the login's default model: about 2 model requests per `c5l` run; each extra denied dispatch adds one. They are billed to whatever login is stored in the config folder; a Console API-key login is billed. P2 made 6 runs of part `c5l` (full runs and part runs) on 2026-11-22: 12 requests on the operator's login in all. The other parts send no model request out of the machine.
- Bounds. Claude Code panes: 60 s for each session start (the first `SessionStart` input) and for each plain turn; 120 s for the PW2 turn with a subagent; 30 s for the second `SessionStart` after `/clear` and for the `UserPromptSubmit` input of the blocked prompt; 10 s for each control request to reach the mock, and a window of 10 s in which no request with the blocked prompt may come; 60 s for the Bash command of the Esc turn to start, then 15 s for a `Stop` or `StopFailure` input; 15 s for the process to end after SIGTERM; 10 s for PW7's error upstream to start; 5 s for each pane program to end before `KILL`. Print mode on the mock: 90 s for each run. Part `c5l`: 180 s for the turn. OpenCode: 180 s for each `run`, 60 s for each `session list`, 60 s for the TUI to load the plugin, 90 s for the request of the prompt typed into the TUI, 15 s for the TUI to end after SIGTERM. A bound that passes makes the sub-checks that depend on it `FAIL` with the bound in the text, and the script goes on. These bounds write a `note` line instead, which is not part of the verdict: 120 s for the `SubagentStop` input of the PW2 subagent (note 2.5); in PW7, 90 s for the turn with the foreground subagent, 60 s for the turn that starts the background subagent, 60 s for two `SubagentStop` inputs, 60 s for the full-window turn and 90 s for the turn after it (notes 7.7 to 7.11); 15 s for Claude Code to end after `/exit` (note 9.4); 30 s for the OpenCode idle event in the TUI (note 4.6). A print-mode or OpenCode command over its bound gets `TERM`, then `KILL` 2 s later.
- The traps end the private tmux server, the mock and every agent and helper process, and remove the private folder.
- Last run: 2026-11-22, Linux, Claude Code 2.1.330 (PW3: first launch on 2.1.329) and OpenCode 1.19.6: PW1, PW2, PW3, PW4, PW5, PW9 and PW10 verified; PW7 and PW16 failed. Rows: `docs/premises.md` PW1 to PW16, and row P2/PW below.

## FIX-21 (no Claude attribution)

Node FIX-21; spec §8.5 (the `attribution` and `includeCoAuthoredBy` settings keys), §13.4 (the attribution guard); operator, 2026-11-22: there is never Claude attribution. From the checkout, with `python3` and `git`, Claude Code logged in, and `claude` and `opencode` on `PATH` (`lib.sh` checks both; the script starts no OpenCode):

```sh
cargo build -p zygo --release
PATH="$HOME/.opencode/bin:$PATH" sh scripts/premises/live-attribution.sh [login bare mock]
```

- Part `login` runs `zygo claude --passthrough` in print mode with `--model haiku` in a new git repository of the scratch folder and asks Claude Code to create a file and commit it. Pass: one new commit whose message holds none of the §13.4 attribution forms. It uses the real `HOME` and no `CLAUDE_CONFIG_DIR`, as #28.
- Part `bare` asks the same with `--bare`. zygo refuses `--passthrough` with `--bare` (§12.3); the part records that refusal, then runs Claude Code alone with `--bare` and a settings file that holds only zygo's two attribution keys, on the same login. Claude Code in bare mode reads no claude.ai login, so this run cannot sign in.
- Part `mock` uses no login and no key, with a temporary `HOME`: Claude Code alone on the mock upstream (the control, with Claude Code's default attribution), Claude Code alone with `--bare` (recorded only), then `zygo claude` (not passthrough) on the mock, without and with `--bare`. It counts the recorded requests that hold Claude Code's own commit attribution text (the Anthropic noreply address in angle brackets, or the generated line with its link; zygo's session sentence names both forms without the brackets and the link). Pass: the control has such a request, and no zygo request has one.
- Tools: `Write`, and `Bash` only for `git add` and `git commit` (no `--yolo`). Spend: at most 8 turns on haiku in each of the two sign-in runs (the `bare` run stops before its first request).
- It prints one `LIVE <id> pass|fail|recorded (agent <version>): …` line for each of FIX21-LOGIN, FIX21-BARE-ZYGO, FIX21-BARE and FIX21-MOCK, and writes `fixtures/claude-code/live/fix21-login.json`, `fix21-bare.json` and `fix21-mock.json`.

## P3-15 (session text after a compaction and in subagents)

Node SX-8; `docs/specs/2026-11-24-session-text-design.md` T§12.2 (ST1, ST2, ST3, ST4 of T§12.1). The text below is a copy of T§12.2; T§12.2 is the authority, and a change there is copied here. Results go to the `docs/premises.md` "Results" table (rows ST1 to ST4) as T§12.1 "How a result enters the repository" says, and a dated row goes to the "Results" table of this file, as for the other checks.

Run on your own machine with your normal Claude Code and OpenCode logins. It runs no zygo and no CI job; from this repository it uses only `scripts/premises/scrub.py`. Claude Code runs once in print mode on Haiku with no compaction: about 5 model requests, about 80,000 input tokens in all (each request sends the whole context again; most of it is a cache read). OpenCode runs three short turns and one compaction on the models you pick.

What it leaves on your machine, and how to remove it (step 8): Claude Code writes its transcripts to `~/.claude/projects/<folder name>/`, and it can add an entry for the scratch folder to `~/.claude.json`; OpenCode keeps the scratch session in its own data folder. It does not load your user settings or MCP servers (`--setting-sources project,local`, `--strict-mcp-config`), and it does not change your config files.

**Claude Code (ST1, ST2)**

1. Make a scratch folder with the probe hook, its settings file and a one-agent plugin:

```sh
S=$(mktemp -d "${TMPDIR:-/tmp}/p315.XXXXXX") && cd "$S" && git init -q
cat > probe.py <<'EOF'
import json, sys
d = json.load(sys.stdin)
open(sys.argv[1], "a").write(json.dumps(d) + "\n")
ev = d.get("hook_event_name", "")
if ev in ("SessionStart", "SubagentStart"):
    who = "sub" if "agent_id" in d else "main"
    tag = "CANARY-%s-%s-%s" % (ev, d.get("source", "na"), who)
    print(json.dumps({"hookSpecificOutput": {"hookEventName": ev, "additionalContext": "\n" + tag}}))
EOF
H="python3 -I $S/probe.py $S/hooks.jsonl"
E='[{"hooks":[{"type":"command","command":"'"$H"'"}]}]'
printf '{"hooks":{"SessionStart":%s,"SubagentStart":%s}}\n' "$E" "$E" > settings.json
mkdir -p plug/p315/.claude-plugin plug/p315/agents
printf '{"name":"p315","version":"0.0.1"}\n' > plug/p315/.claude-plugin/plugin.json
printf -- '---\nname: probe\ndescription: Replies DONE.\n---\nReply with the word DONE and nothing else.\n' > plug/p315/agents/probe.md
echo "$S"
```

2. Run Claude Code once. It waits for each subagent (no background subagents, no fork mode) and exits after its reply:

```sh
cd "$S" && CLAUDE_CODE_DISABLE_BACKGROUND_TASKS=1 CLAUDE_CODE_FORK_SUBAGENT=0 claude -p --model haiku --setting-sources project,local --strict-mcp-config --settings "$S/settings.json" --plugin-dir "$S/plug/p315" "Call the general-purpose subagent with the task 'Reply with the word DONE.' and wait for its result. Then call the p315:probe subagent with the task 'Reply with the word DONE.' and wait for its result. Then reply with both results."
```

3. Read the evidence from the subagent transcripts (`~/.claude/projects/<project>/<session>/subagents/agent-<id>.jsonl`, https://code.claude.com/docs/en/sub-agents). Only entries that are not `assistant` entries count, so a model's own words are never evidence:

```sh
python3 -I - "$S/hooks.jsonl" > "$S/evidence.txt" <<'EOF'
import json, os, sys
hooks = [json.loads(l) for l in open(sys.argv[1])]
main = next(h for h in hooks if h.get("hook_event_name") == "SessionStart" and "agent_id" not in h)
base = main["transcript_path"][: -len(".jsonl")]
for h in hooks:
    if h.get("hook_event_name") != "SubagentStart":
        continue
    p = os.path.join(base, "subagents", "agent-%s.jsonl" % h["agent_id"])
    kinds = []
    if os.path.exists(p):
        for line in open(p):
            e = json.loads(line)
            if e.get("type") != "assistant" and "CANARY-SubagentStart" in line:
                kinds.append(str(e.get("type")))
    print(h.get("agent_type"), "transcript" if os.path.exists(p) else "no-transcript", "canary:" + (",".join(kinds) or "none"))
EOF
cat "$S/evidence.txt"
```

**OpenCode (ST3, ST4)**

4. Pick two different model ids of one OpenCode provider, `MAIN` and `SUB` (for example a Sonnet id and a Haiku id). `small_model` is set to `MAIN`, so a title request cannot look like a subagent request. Make the probe plugin and start OpenCode in the scratch folder:

```sh
mkdir -p "$S/oc/plugins" && cat > "$S/oc/plugins/probe.js" <<'EOF'
import { appendFileSync } from "node:fs";
const put = (o) => { try { appendFileSync(process.env.P315_LOG, JSON.stringify({ t: Date.now(), ...o }) + "\n"); } catch {} };
export const P315 = async () => ({
  "experimental.chat.system.transform": async (input) => put({ hook: "transform", model: input?.model?.id, session: input?.sessionID ?? null, keys: Object.keys(input ?? {}).sort() }),
  "experimental.session.compacting": async (input) => put({ hook: "compacting", session: input?.sessionID ?? null, keys: Object.keys(input ?? {}).sort() }),
});
EOF
cd "$S" && P315_LOG="$S/oc.jsonl" OPENCODE_CONFIG_DIR="$S/oc" OPENCODE_CONFIG_CONTENT='{"small_model":"<provider>/<MAIN>","agent":{"general":{"model":"<provider>/<SUB>"}}}' opencode -m <provider>/<MAIN>
```

5. Type: `Use the general subagent to read probe.py and tell me its first line.` Wait until its answer is shown. Type `/compact` and wait until the summary is shown. Type `Reply OK.` and wait for the reply. Exit OpenCode.

**Capture**

6. From the checkout of this repository:

```sh
python3 -I scripts/premises/scrub.py "$S" "$S/hooks.jsonl" > fixtures/claude-code/live/st-hooks.jsonl
cp "$S/evidence.txt" fixtures/claude-code/live/st-evidence.txt
python3 -I scripts/premises/scrub.py "$S" "$S/oc.jsonl" > fixtures/opencode/live/st-transform.jsonl
claude --version; opencode --version
```

7. Send the three files, the two version lines, the MAIN and SUB model ids chosen in step 4, and whether the subagent's answer was shown in step 5 (yes or no) to the lead.

8. Remove what the check left. Close every Claude Code session first, because `~/.claude.json` is shared:

```sh
rm -rf ~/.claude/projects/*p315*
cp ~/.claude.json ~/.claude.json.bak-p315
python3 -I -c 'import json,os; p=os.path.expanduser("~/.claude.json"); d=json.load(open(p)); r=[k for k in list(d.get("projects",{})) if "p315" in k]; [d["projects"].pop(k) for k in r]; json.dump(d,open(p,"w"),indent=2); print("removed", r)'
rm -rf "$S"
```

Once Claude Code starts normally, delete `~/.claude.json.bak-p315`.

The OpenCode session stays in OpenCode's data folder; delete it from OpenCode's session list if you want it gone.

**Reading the result** (the lead, `docs/specs/2026-11-24-session-text-design.md` T§12.1 "How a result enters the repository"):

- ST1 verified: the `general-purpose` line of `st-evidence.txt` says `transcript` and `canary:` with at least one entry. ST1 failed: it says `transcript` and `canary:none`. `no-transcript`: not decided; no row.
- ST2: the same for the `p315:probe` line.
- ST3 verified: `st-transform.jsonl` holds a `transform` line with model `MAIN` after the `compacting` line, with the same `session` when both lines have one. Failed: no such line, while the log shows the `Reply OK.` turn ran (a `transform` line before the `compacting` line exists).
- ST4 verified: it holds a `transform` line with model `SUB`. Failed: none, while the subagent's answer was shown in step 5.
- The `keys` lists go into the rows as information; they change no behavior in the session-text spec.

## Mail premises (node ML-17)

Node ML-17 (script: node ML-16); `docs/specs/2026-11-22-workspace-mail-design.md` M§14 (PM1, PM2, PM3). From the checkout, with tmux 3.2 or later, `jq`, `python3` and `perl`, and `claude` and `opencode` on `PATH` at or above the pinned versions (the caller adds the OpenCode folder to `PATH`). No login is read. Network needed: OpenCode fetches its provider package from the package registry into the temporary `HOME`.

```sh
N2_SCRATCH=/tmp/ml17-pm sh scripts/premises/live-mail.sh --pm
```

- It prints one `LIVE <id> verified|failed (agent <versions>): …` line for each of PM1, PM2 and PM3, also appended to `$N2_SCRATCH/results.txt`, and at the end of the run copies `fixtures/claude-code/mail/pm1.txt`, `pm2.txt`, `pm3.txt` and `fixtures/opencode/mail/pm2.txt`, `pm3.txt` into the checkout, a failed line included. Exit status: 0, the run finished (a failed line does not change it); 3, a request at the mock carried a credential other than the dummy key (the mock writes `ABORT` and stops); 1, a tool or agent is missing or below its minimum version, a path holds a quote or a space, the tmux socket path is longer than 100 characters (set `TMPDIR` to a short folder), or the fixtures could not be copied to the repository; 2, a usage error; 129, 130, 141 or 143 after a signal.
- Each agent runs in a 200 x 50 pane of a tmux server of the script's own (`-S <socket in a private folder>`, `-f /dev/null`); text is typed with `send-keys -l`, then `Enter` 0.5 s later. Every part uses a temporary `HOME`, the dummy key and the mock upstream, with no zygo between. PM1: Claude Code with a probe MCP server (`scripts/premises/mcp_probe.py`) and a `Stop` hook that answers `decision: block` once. PM2 and PM3, Claude Code: `--tools ""`, with `SessionStart` and `UserPromptSubmit` hooks that return `additionalContext` lines, then the wake line typed into the idle pane. PM2 and PM3, OpenCode: the TUI with a probe plugin that pushes a mail line in `experimental.chat.system.transform`, then the wake line typed into the idle pane.
- Bounds. Claude Code panes: 60 s for each session start (the first `SessionStart` input; the folder trust question is answered yes), then a 3 s pause. PM1: 60 s for the first `Stop` input, 5 s for its hook output, then 60 s each for the request with the block reason, the `PostToolUse` input of the probe tool, the request with the tool result and the second `Stop` input. PM2 and PM3, Claude Code: 60 s for a `Stop` input after the wake line (a timeout is note 2.4). OpenCode pane: 60 s for the plugin's first event, a 5 s pause, then 90 s for a request with the wake line. 5 s for each pane program to end after SIGTERM, then SIGKILL.
- Side effects: the scratch folder. With `N2_SCRATCH` set, `<N2_SCRATCH>/live-<pid>` stays, with the raw records (`ml-rec/PM<n>.out`, `ml-rec/PM<n>-oc.out`), the temporary `HOME`s, the recorded requests, the hook logs and the pane text, and `<N2_SCRATCH>/results.txt` keeps each `LIVE` line. Without `N2_SCRATCH`, `lib.sh` makes a temporary folder and deletes it, raw output included, after exit 0 (it keeps and names it after any other exit). Nothing under the real `HOME` is written. The traps end the private tmux server, the mock and every agent and helper process, and remove the private folder (`${TMPDIR:-/tmp}/zm.XXXXXX`).
- Last run: 2026-11-25, macOS, Claude Code 2.1.332, OpenCode 1.18.16, tmux 3.8: PM1, PM2 and PM3 verified. Rows: `docs/premises.md` PM1 to PM3.

## PD2, PD3 (decision records premises)

Node DR-15; `docs/specs/2026-11-24-decision-records-design.md` D§18 PD2, PD3; D§9.2; D§8.4.

**Design.** One print-mode Claude Code run proves both premises. The upstream is `scripts/premises/mock_upstream.py`: its authored replies make the main loop call `mcp__zygo__work_verdict` twice. Call 1 is small (`discard`). Call 2 is the PD2 object: exactly 262,144 bytes of compact UTF-8 JSON (`ARGS_MAX`, `crates/zygo-mcp/src/tools/work.rs:37`; D§9.2), schema-valid, with 256 `dispositions`, quotes, backslashes, tabs, newlines and non-ASCII text. The mock streams call 2 the way the real API streams a large tool input: as many `input_json_delta` events of at most 512 bytes each, split on character boundaries (`split.py` below; `scripts/premises/sse.py` writes the other events). The MCP server is a stub stdio server named `zygo` (`stub.py` below). It lists `work_verdict` with zygo's description and input schema (`work.rs:72`, `:176-196`), writes each `tools/call` line to a file exactly as received, and records nothing else. A `PreToolUse` hook on `mcp__zygo__work_verdict` (the matcher zygo registers, `crates/zygo-launch/src/claude_code.rs:484`) runs `scripts/premises/hook_log.py`, which saves each hook input.
- PD2 is `verified` when the arguments of call 2 that the stub received, written as compact JSON, are byte-equal to the 262,144 bytes that the mock sent, and the raw `tools/call` line that the stub received holds those 262,144 bytes unchanged (`raw_line_holds_sent_bytes` in `verify.py`). zygo measures the same compact form (`serde_json::to_vec`, `work.rs:201`).
- PD3 is `verified` when both `PreToolUse` inputs (both main-loop calls) have no `agent_id` key. zygo's hook takes the key's presence as "subagent" (`crates/zygo-core/src/hook.rs:35`) and denies only then (`crates/zygo-work/src/hooks.rs:184-185`).

**Why a mock and not the real API.** On this machine, `claude auth status` with a temporary `HOME` and `CLAUDE_CONFIG_DIR` gave `"loggedIn": false, "authMethod": "none"` (Claude Code 2.1.332, 2026-11-25). Thus `claude -p` with an isolated `HOME` cannot use the claude.ai login. Both premises concern only Claude Code: the path from an API `tool_use` block to the MCP server (PD2), and the hook input that Claude Code builds (PD3). Neither depends on the model, so authored replies are enough. The run uses no login and no key.

**What the run touches.**
- Reads: the Claude Code binary on `PATH`, and the repository's `scripts/premises/` files.
- Writes: only `/private/tmp/dr15-live/`. Step L4 also writes the two scrubbed fixtures into this worktree.
- Isolation: `env -i` with a temporary `HOME`, `CLAUDE_CONFIG_DIR` and `XDG_*`. `--strict-mcp-config` and `--settings` take only the run's files, and there are no managed settings (L1 checks).
- Network: the API goes to the mock on 127.0.0.1 with the dummy key. `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1` and `DISABLE_AUTOUPDATER=1` stop other traffic. The mock stops the run and writes `rec/ABORT` if any other credential arrives (`mock_upstream.py:125-134`).
- zygo: no zygo process runs, so no verdict reaches any `usage.db` or session ledger.
- The operator's state: the run does not touch the claude.ai login, `~/.claude.json`, or the lead's config folder. L4 checks that no transcript for the run is in the real config folders.

**Spend.** No real-API run. Model: none (the mock sends authored replies). Turn bound: `--max-turns 4` (3 assistant turns are scripted). Cost: $0. Wall-clock bound: the Bash tool timeout of 300,000 ms on L3.

L1. Preflight:

```sh
cd "$(git rev-parse --path-format=absolute --git-common-dir)/../.worktrees/phase3/DR-15-node" && test "$(git branch --show-current)" = phase3/DR-15--DR-15 && v=$(claude --version | awk '{print $1}') && echo "claude $v" && sh -c '. scripts/premises/ver.sh && ver_ge "$1" 2.1.328' sh "$v" && command -v python3 jq && test ! -e /private/tmp/dr15-live && test ! -e "/Library/Application Support/ClaudeCode/managed-settings.json" && echo "preflight ok"
```

Check: the output prints `claude <version>` (2.1.328 or later; the pinned version is a minimum), two tool paths and `preflight ok`. The shell is now in the worktree, and L2-L4 use it as `W`.

L2. Build the run folder (writes only `/private/tmp/dr15-live`):

```sh
W=$(git rev-parse --show-toplevel) && D=/private/tmp/dr15-live && PY=$(command -v python3) && mkdir "$D" && mkdir -p "$D/home/.claude" "$D/home/.config" "$D/home/.local/share" "$D/home/.cache" "$D/repo" "$D/rec" "$D/hooks" "$D/mcp" "$D/sse" "$D/payload" "$D/out" && claude --version > "$D/version.txt" && git -C "$D/repo" init -q && git -C "$D/repo" -c user.name=t -c user.email=t@t commit -q --allow-empty -m init || { echo "L2 run folder failed"; false; }
cat > "$D/gen.py" <<'EOF'
"""gen.py <dir>: the PD2 and PD3 work_verdict arguments as compact UTF-8 JSON (DR-15)."""
import json, os, sys

ARGS_MAX = 262144  # crates/zygo-mcp/src/tools/work.rs ARGS_MAX (D§9.2: 256 KiB)
CLASSES = ["none", "acceptance", "security", "irreversible", "contract", "spec", "rule"]
KINDS = ["fixed", "rejected", "recorded"]


def compact(o):
    return json.dumps(o, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def text(seed, n):
    unit = "dr15 %s é✂ \"q\" \\ \t|\n" % seed
    return (unit * (n // len(unit) + 1))[:n]


rows = []
for i in range(256):
    kind = KINDS[i % 3]
    e = {"finding": text("f%03d" % i, 200), "class": CLASSES[i % 7], "disposition": kind}
    if kind == "fixed":
        e["commit"] = "%040x" % (i + 1)
    elif kind == "rejected":
        e["reason"] = text("r%03d" % i, 1000)
    else:
        e["record"] = "DDR-%026d" % i
    rows.append(e)
big = {"unit": "phase3:DR-15:probe", "round": 1, "verdict": "keep", "findings": "",
       "head": "a" * 40, "base": "b" * 40, "dispositions": rows}
pad = ARGS_MAX - len(compact(big))
assert pad > 0, pad
big["findings"] = ("pd2 padding " * (pad // 12 + 1))[:pad]
data = compact(big)
assert len(data) == ARGS_MAX, len(data)
small = {"unit": "phase3:DR-15:probe", "round": 1, "verdict": "discard", "findings": "dr15 pd3 probe"}
open(os.path.join(sys.argv[1], "big.json"), "wb").write(data)
open(os.path.join(sys.argv[1], "small.json"), "wb").write(compact(small))
print("big.json %d bytes, small.json %d bytes" % (len(data), len(compact(small))))
EOF
cat > "$D/split.py" <<'EOF'
"""split.py <input json> <max bytes>: reads one sse.py tool reply on stdin and writes it with its single
input_json_delta replaced by many, each partial_json at most <max bytes> of UTF-8, cut on character boundaries (DR-15)."""
import json, sys

text = open(sys.argv[1], "rb").read().decode("utf-8")
limit = int(sys.argv[2])
chunks, cur, size = [], [], 0
for ch in text:
    n = len(ch.encode("utf-8"))
    if size + n > limit:
        chunks.append("".join(cur))
        cur, size = [], 0
    cur.append(ch)
    size += n
if cur:
    chunks.append("".join(cur))
assert "".join(chunks) == text
out = []
for ev in sys.stdin.read().split("\n\n"):
    if not ev.strip():
        continue
    name, data = ev.split("\n", 1)
    d = json.loads(data[len("data: "):])
    if d.get("type") == "content_block_delta" and d["delta"].get("type") == "input_json_delta":
        for c in chunks:
            e = {"type": "content_block_delta", "index": d["index"], "delta": {"type": "input_json_delta", "partial_json": c}}
            out.append("%s\ndata: %s\n\n" % (name, json.dumps(e)))
    else:
        out.append(ev + "\n\n")
sys.stdout.write("".join(out))
EOF
cat > "$D/stub.py" <<'EOF'
"""stub.py <log dir>: stdio MCP server named zygo in the run's --mcp-config; one tool, work_verdict, with zygo's
input schema (crates/zygo-mcp/src/tools/work.rs). Saves each tools/call line as received; records nothing else."""
import json, os, sys

REV = {"type": "string", "pattern": "^[0-9a-f]{7,64}$"}
SCHEMA = {"type": "object", "properties": {
    "unit": {"type": "string"},
    "round": {"type": "integer", "minimum": 1},
    "verdict": {"type": "string", "enum": ["keep", "discard", "rework"]},
    "findings": {"type": "string"},
    "head": REV,
    "base": REV,
    "dispositions": {"type": "array", "maxItems": 256, "items": {"type": "object", "properties": {
        "finding": {"type": "string", "minLength": 1, "maxLength": 200},
        "class": {"type": "string", "enum": ["none", "acceptance", "security", "irreversible", "contract", "spec", "rule"]},
        "disposition": {"type": "string", "enum": ["fixed", "rejected", "recorded"]},
        "commit": REV,
        "reason": {"type": "string", "minLength": 1, "maxLength": 2000},
        "record": {"type": "string", "pattern": "^DDR-[0-9A-HJKMNP-TV-Z]{26}$"}},
        "required": ["finding", "class", "disposition"]}}},
    "required": ["unit", "round", "verdict"]}
DESC = "Lead only. Record your verdict on a unit of work and round after its subagent has stopped: keep, discard or rework, with your findings. On keep, give `head` (the commit you judged) and `dispositions` (an empty list when the review had no findings): one entry for each review finding, fixed with its commit, rejected with its reason, or recorded with its DDR id; in one checkout, also give `base`."
n = 0
for raw in sys.stdin.buffer:
    try:
        msg = json.loads(raw)
    except ValueError:
        continue
    mid, method = msg.get("id"), msg.get("method")
    if method == "tools/call":
        n += 1
        with open(os.path.join(sys.argv[1], "call-%02d.raw" % n), "wb") as f:
            f.write(raw)
    if mid is None:
        continue
    if method == "initialize":
        out = {"result": {"protocolVersion": msg.get("params", {}).get("protocolVersion", "2024-11-05"),
                          "capabilities": {"tools": {}}, "serverInfo": {"name": "zygo", "version": "0.0.0-dr15"}}}
    elif method == "tools/list":
        out = {"result": {"tools": [{"name": "work_verdict", "description": DESC, "inputSchema": SCHEMA}]}}
    elif method == "tools/call":
        out = {"result": {"content": [{"type": "text", "text": "dr15-stub-ok call %d" % n}]}}
    else:
        out = {"error": {"code": -32601, "message": "not found"}}
    out.update({"jsonrpc": "2.0", "id": mid})
    sys.stdout.write(json.dumps(out) + "\n")
    sys.stdout.flush()
EOF
cat > "$D/verify.py" <<'EOF'
"""verify.py <run dir>: PD2 and PD3 results of the DR-15 run as <run>/out/pd2.json, pd3.json; prints two LIVE lines."""
import glob, hashlib, json, os, sys, time

D = sys.argv[1]
ver = open(os.path.join(D, "version.txt")).read().split()[0]


def compact(o):
    return json.dumps(o, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def digest(b):
    return {"bytes": len(b), "sha256": hashlib.sha256(b).hexdigest()}


sent = {k: open(os.path.join(D, "payload", k + ".json"), "rb").read() for k in ("big", "small")}
which = {"keep": "big", "discard": "small"}
hooks = {}
for p in sorted(glob.glob(os.path.join(D, "hooks", "*-PreToolUse-mcp__zygo__work_verdict-*.json"))):
    h = json.load(open(p))
    hooks.setdefault(which.get(h.get("tool_input", {}).get("verdict")), []).append((os.path.basename(p), h))
calls = {}
for p in sorted(glob.glob(os.path.join(D, "mcp", "call-*.raw"))):
    raw = open(p, "rb").read()
    args = json.loads(raw)["params"]["arguments"]
    calls.setdefault(which.get(args.get("verdict")), []).append((raw, args))
results = {}
for p in sorted(glob.glob(os.path.join(D, "rec", "[0-9]*.json"))):
    body = json.load(open(p)).get("body")
    if not isinstance(body, dict):
        continue
    for m in body.get("messages") or []:
        for c in m.get("content") if isinstance(m.get("content"), list) else []:
            if isinstance(c, dict) and c.get("type") == "tool_result" and c.get("tool_use_id", "").startswith("toolu_dr15_"):
                results[c["tool_use_id"]] = {"is_error": bool(c.get("is_error")), "content": c.get("content")}
listed = []
for p in sorted(glob.glob(os.path.join(D, "rec", "[0-9]*.json"))):
    body = json.load(open(p)).get("body")
    if isinstance(body, dict) and body.get("tools"):
        listed = [t.get("name") for t in body["tools"] if str(t.get("name", "")).startswith("mcp__")]
        break

init = {}
for line in open(os.path.join(D, "out", "stream.jsonl"), encoding="utf-8"):
    try:
        e = json.loads(line)
    except ValueError:
        continue
    if e.get("type") == "system" and e.get("subtype") == "init":
        init = e
        break
harness = {"mcp_servers": init.get("mcp_servers"), "work_verdict_listed": "mcp__zygo__work_verdict" in (init.get("tools") or []),
           "small_call_reached_stub": len(calls.get("small", [])) == 1}
day = time.strftime("%Y-%m-%d")
big_calls = calls.get("big", [])
recv = big_calls[0] if len(big_calls) == 1 else None
big_hooks = hooks.get("big", [])
hook_in = compact(big_hooks[0][1]["tool_input"]) if big_hooks else b""
pd2_ok = recv is not None and compact(recv[1]) == sent["big"] and sent["big"] in recv[0] and len(sent["big"]) == 262144
pd2 = {"premise": "PD2", "claude_code": ver, "date": day, "harness": harness, "result": "verified" if pd2_ok else "failed",
       "sent": digest(sent["big"]),
       "pre_tool_use_tool_input": dict(digest(hook_in), equal=hook_in == sent["big"]),
       "received": None if recv is None else dict(digest(compact(recv[1])), equal=compact(recv[1]) == sent["big"],
                                                  raw_line_bytes=len(recv[0]), raw_line_holds_sent_bytes=sent["big"] in recv[0]),
       "tools_calls_seen": {k: len(v) for k, v in calls.items()},
       "mcp_tools_in_first_request": listed,
       "tool_result": results.get("toolu_dr15_01")}
inputs = [(n, h) for k in ("small", "big") for n, h in hooks.get(k, [])]
pd3_ok = len(inputs) == 2 and all("agent_id" not in h for _, h in inputs)
small_in = hooks.get("small", [(None, None)])[0][1]
pd3 = {"premise": "PD3", "claude_code": ver, "date": day, "harness": harness, "result": "verified" if pd3_ok else "failed",
       "pre_tool_use_inputs": [{"file": n, "tool_name": h.get("tool_name"), "keys": sorted(h), "has_agent_id": "agent_id" in h,
                                "has_agent_type": "agent_type" in h} for n, h in inputs],
       "small_call_input": small_in,
       "tool_result": results.get("toolu_dr15_00")}
os.makedirs(os.path.join(D, "out"), exist_ok=True)
for name, o in (("pd2", pd2), ("pd3", pd3)):
    with open(os.path.join(D, "out", name + ".json"), "w") as f:
        json.dump(o, f, indent=2, ensure_ascii=False)
        f.write("\n")
r = pd2["received"] or {}
print("LIVE PD2 %s (agent %s): sent %d bytes sha256 %s; received %s bytes, equal %s; raw line holds sent bytes %s; tool_result is_error %s"
      % (pd2["result"], ver, pd2["sent"]["bytes"], pd2["sent"]["sha256"], r.get("bytes"), r.get("equal"),
         r.get("raw_line_holds_sent_bytes"), (pd2["tool_result"] or {}).get("is_error")))
print("LIVE PD3 %s (agent %s): %d PreToolUse inputs for mcp__zygo__work_verdict from the main loop; agent_id key in %d"
      % (pd3["result"], ver, len(inputs), sum(1 for _, h in inputs if "agent_id" in h)))
EOF
cat > "$D/rules.json" <<'EOF'
[{"name": "dr15-00", "sse": "sse/00.sse", "times": 1, "last": "dr15-probe go", "body": "\"tools\": \\[\\{", "no_header": ["x-claude-code-compaction", "x-claude-code-agent-id"]},
 {"name": "dr15-01", "sse": "sse/01.sse", "times": 1, "last": "toolu_dr15_00", "body": "\"tools\": \\[\\{", "no_header": ["x-claude-code-compaction", "x-claude-code-agent-id"]},
 {"name": "dr15-02", "sse": "sse/02.sse", "times": 1, "last": "toolu_dr15_01", "body": "\"tools\": \\[\\{", "no_header": ["x-claude-code-compaction", "x-claude-code-agent-id"]}]
EOF
python3 -I "$D/gen.py" "$D/payload" && python3 -I "$W/scripts/premises/sse.py" tool mcp__zygo__work_verdict toolu_dr15_00 "$(cat "$D/payload/small.json")" > "$D/sse/00.sse" && python3 -I "$W/scripts/premises/sse.py" tool mcp__zygo__work_verdict toolu_dr15_01 '{}' | python3 -I "$D/split.py" "$D/payload/big.json" 512 > "$D/sse/01.sse" && python3 -I "$W/scripts/premises/sse.py" text "dr15 done" > "$D/sse/02.sse" || { echo "L2 payload failed"; false; }
python3 -I -c 'import json, sys
parts = [json.loads(l[6:])["delta"]["partial_json"] for l in open(sys.argv[1], encoding="utf-8") if l.startswith("data: ") and "input_json_delta" in l]
joined = "".join(parts).encode("utf-8")
print("sse/01.sse input_json_delta events %d, largest %d bytes, equal to big.json %s" % (len(parts), max(len(p.encode("utf-8")) for p in parts), joined == open(sys.argv[2], "rb").read()))' "$D/sse/01.sse" "$D/payload/big.json"
printf '{"mcpServers":{"zygo":{"command":"%s","args":["-I","%s/stub.py","%s/mcp"]}}}\n' "$PY" "$D" "$D" > "$D/mcp.json"
printf '{"hooks":{"PreToolUse":[{"matcher":"mcp__zygo__work_verdict","hooks":[{"type":"command","command":"%s -I %s/scripts/premises/hook_log.py %s/hooks"}]}]}}\n' "$PY" "$W" "$D" > "$D/settings.json"
printf '{"hasCompletedOnboarding":true,"numStartups":5,"theme":"dark","customApiKeyResponses":{"approved":["dummy-key"],"rejected":[]},"projects":{"%s/repo":{"hasTrustDialogAccepted":true,"hasCompletedProjectOnboarding":true}}}\n' "$D" > "$D/home/.claude/.claude.json"
jq -e . "$D/rules.json" "$D/mcp.json" "$D/settings.json" "$D/home/.claude/.claude.json" > /dev/null && wc -c "$D/payload/big.json" "$D/payload/small.json"
```

Check: `gen.py` prints `big.json 262144 bytes, small.json 87 bytes`, and `wc` shows `262144` for `big.json`. The `sse/01.sse` line shows at least 512 `input_json_delta` events, the largest at most 512 bytes, and `equal to big.json True`: the concatenation of all `partial_json` values of `sse/01.sse` is byte-equal to `big.json`. (Offline test of these scripts and `sse.py`, 2026-11-25: a stdin replay of `initialize`, `tools/list` and the 256 KiB `tools/call` into `stub.py` answered all three and saved the line. `verify.py` gave `verified` on a complete synthetic run and `failed` with no call and no hook input. Offline test of `split.py`, 2026-11-25: 513 events, the largest 512 bytes, equal to `big.json`; `verify.py` gave `failed` on a synthetic run whose raw `tools/call` line escaped the non-ASCII text, although the parsed arguments were equal.)

L3. Run Claude Code once on the mock (Bash timeout 300000):

```sh
W=$(git rev-parse --show-toplevel) && D=/private/tmp/dr15-live && CC=$(command -v claude) || { echo "L3 setup failed"; false; }
MOCK_KEY=dummy-key python3 -I "$W/scripts/premises/mock_upstream.py" --dir "$D/rec" --rules "$D/rules.json" --port-file "$D/port" > "$D/mock.log" 2>&1 &
MP=$!
trap 'kill $MP 2>/dev/null' EXIT
python3 -I -c 'import os, sys, time
for _ in range(100):
    if os.path.exists(sys.argv[1]) and os.path.getsize(sys.argv[1]):
        sys.exit(0)
    time.sleep(0.1)
sys.exit("mock did not start")' "$D/port" || { echo "L3 mock start failed"; false; }
(cd "$D/repo" && env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin TERM=xterm-256color HOME="$D/home" CLAUDE_CONFIG_DIR="$D/home/.claude" XDG_CONFIG_HOME="$D/home/.config" XDG_DATA_HOME="$D/home/.local/share" XDG_CACHE_HOME="$D/home/.cache" ANTHROPIC_BASE_URL="http://127.0.0.1:$(cat "$D/port")" ANTHROPIC_API_KEY=dummy-key DISABLE_AUTOUPDATER=1 CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 ENABLE_TOOL_SEARCH=false "$CC" -p "dr15-probe go" --max-turns 4 --strict-mcp-config --mcp-config "$D/mcp.json" --settings "$D/settings.json" --allowedTools mcp__zygo__work_verdict --output-format stream-json --verbose < /dev/null > "$D/out/stream.jsonl" 2> "$D/out/stderr.txt"); echo "claude exit $?"
kill $MP; wait $MP 2>/dev/null
test ! -e "$D/rec/ABORT" && echo "no ABORT"
ls "$D/hooks" "$D/mcp"
jq -c 'select(.type == "system" and .subtype == "init") | {mcp_servers, work_verdict_listed: (.tools | index("mcp__zygo__work_verdict") != null)}' "$D/out/stream.jsonl"
jq -r 'select(.type == "result") | .result' "$D/out/stream.jsonl"
```

Check: `claude exit 0` and `no ABORT`. `hooks/` holds `0001-PreToolUse-mcp__zygo__work_verdict-main.json` and `0002-…-main.json`, and `mcp/` holds `call-01.raw` and `call-02.raw`. The init line shows the server `zygo` as `connected` and `work_verdict_listed: true`. The result line is `dr15 done`.

L4. Results and scrubbed fixtures:

```sh
W=$(git rev-parse --show-toplevel) && D=/private/tmp/dr15-live && python3 -I "$D/verify.py" "$D" | tee "$D/out/live.txt" && mkdir -p "$W/fixtures/claude-code/live" && python3 -I "$W/scripts/premises/scrub.py" "$D" "$D/out/pd2.json" > "$W/fixtures/claude-code/live/pd2.json" && python3 -I "$W/scripts/premises/scrub.py" "$D" "$D/out/pd3.json" > "$W/fixtures/claude-code/live/pd3.json" && jq -c '{premise, claude_code, date, result, harness}' "$W/fixtures/claude-code/live/pd2.json" "$W/fixtures/claude-code/live/pd3.json" && echo "dr15 transcripts in real config folders: $(ls "$HOME/.claude/projects" "${CLAUDE_CONFIG_DIR:-$HOME/.claude}/projects" 2>/dev/null | grep -c dr15-live)" && ls "$D/home/.claude/projects" && N2_FORBIDDEN_NAMES="$(id -un)" sh "$W/scripts/check-premises.sh"
```

Check:
- Two `LIVE PD2 …` and `LIVE PD3 …` lines are printed, each `verified` or `failed`. Their text goes to the implementer.
- Both fixtures exist in the worktree, with `small_call_reached_stub: true` in `harness`.
- `dr15 transcripts in real config folders: 0` is printed, and the run's transcript folder is under `$D/home/.claude/projects`.
- `check-premises: ok`.

## M12 (workspace mail acceptance #12)

Node ML-19 (script: node ML-16); `docs/specs/2026-11-22-workspace-mail-design.md` M§13 #12 (first half), M§8a. The second half (the session text of a resumed member lists a member that joined while it was down) is the test `mail_item_at_session_start_after_mechanics_before_resume_note` in `crates/zygo-hooks/tests/session_text.rs`, not a live check. From the checkout, with tmux 3.2 or later, `jq`, `python3` and `perl`, `claude` and `opencode` on `PATH` at or above the pinned versions (the script checks both), a release build of zygo (`cargo build -p zygo --release`, or `ZYGO_BIN=<path>`), and a Claude Code config folder with a stored claude.ai login (`CLAUDE_CONFIG_DIR` when set, else `~/.claude`). No key is entered: both members are passthrough.

```sh
N2_SCRATCH=<scratch folder> sh scripts/premises/live-mail.sh --m12
```

- It prints one `LIVE M12 verified|failed (agent <Claude Code version>): …` line, also appended to `$N2_SCRATCH/results.txt`, and at the end of the run copies `fixtures/claude-code/mail/m12.txt` into the checkout, a failed line included. Exit status: 0, the run finished (a failed line does not change it); 1, a tool or agent is missing or below its minimum version, `ZYGO_BIN` is not an executable file, a path holds a quote or a space, the tmux socket path is longer than 100 characters (set `TMPDIR` to a short folder), or the fixture could not be copied; 2, a usage error; 129, 130, 141 or 143 after a signal.
- The run: a git folder `zm12` in the scratch folder and a zygo config with the loadout `m12`, two `claude-code` panes `alpha` and `beta`, `passthrough = true`, `args = ["--setting-sources", "project", "--strict-mcp-config"]`; `zygo up --loadout m12 --no-attach` on a tmux server of the script's own (`-S <socket in a private folder>`, `-f /dev/null`); the folder trust question is answered yes. When both members have an agent session, after a 5 s pause, the task `Tell beta that the build is green.` (no word about mail) is typed into alpha's pane with `send-keys -l`, then `Enter` 0.5 s later.
- Sub-checks: 12.1 `zygo up` exit 0, both panes in `workspace.json`, both members have an agent session; 12.2 alpha's task turn, read from its transcript, calls `mcp__zygo__mail_send` with `beta` among the `to` values; 12.3 alpha becomes busy and then idle, and makes no `mail_read`, `mail_list` or `zygo mail watch` call after the last `mail_send` of the turn (a `mail_list` before the send is how a member finds the names, M§8a item 1); note 12.4 lists the tool names of the task turn in order. The fixture holds the head line (Claude Code, zygo and tmux versions) and these lines only: tool names and recipients, no subject or body (M-C2). A `12.1 FAIL` is a setup fault (no stored login, a workspace that did not start): fix it and run again; with `12.1 ok` the line is the result.
- Bounds: 120 s for both agent sessions, 60 s for alpha to become busy, 300 s for it to become idle again.
- Side effects: it spends the operator's claude.ai usage on a few turns (alpha's task turn, and beta's turn when the mail wakes it). It uses the real `HOME`: zygo writes its state, session folders and usage rows there, and stops and removes the workspace `zm12` at the end (`zygo down`, `zygo ws forget`). It writes the folder trust answer for one scratch folder into the Claude Code config, and the transcripts of alpha and beta stay in that config's `projects` folder (alpha's is read only). With `N2_SCRATCH` set, `<N2_SCRATCH>/live-<pid>` stays, with the raw record `ml-rec/M12.out` and the run folder `m12`.
- Last run: 2026-11-25, macOS, Claude Code 2.1.332, zygo 0.2.1, tmux 3.8: M12 verified. Row: `## Results` M12.

## Results

| Id | Check | Agent | Version | Date | Result | Evidence |
|---|---|---|---|---|---|---|
| 16b | Subagent on a small-window model overruns, gets the mapped `prompt is too long`, recovers by reactive compaction (§20 #16b) | Claude Code | — | — | not run: deferred after release (owner, 2026-11-19) | `scripts/premises/live-16b.sh` → `fixtures/claude-code/live/16b-usage.json`; §20 #16b |
| 21 | A low `soft_context_window` makes Claude Code compact after the crossing turn (§20 #21) | Claude Code | — | — | not run: deferred after release (owner, 2026-11-19) | `scripts/premises/live-window.sh` → `fixtures/claude-code/live/21-usage.json`; §20 #21 |
| P1 | P1 through zygo on the real Anthropic API: `end_turn` (from 21) and `tool_use` (zygo `compact` tool) (§19 P1) | Claude Code | — | — | not run: deferred after release (owner, 2026-11-19) | `scripts/premises/live-window.sh` → `fixtures/claude-code/live/p1-tooluse-usage.json`; §19 P1 |
| 28 | One `zygo claude --passthrough -p` answer on the stored claude.ai login; no login token and no session token in any zygo file (§20 #28) | Claude Code | 2.1.328 | 2026-11-19 | pass | `fixtures/claude-code/live/28-usage.json`: reply `ready`, exit 0, one row `role_served = subscription`, `passthrough = 1`, `status = 200`, `estimated_usd = 0.0`. Run leak scan (`grep -raF`, the claude.ai login token prefix and the zygo session token prefix) over the session's `.dump requests` lines, stdout, stderr and the scrubbed capture: found nothing. The recorded 2026-11-19 run cannot show that a `session.log` copy existed: the script of that day did not require one and kept no record of it, so the `session.log` scan is unproven for that run. The pass rests on the usage row, stdout, stderr, the capture and the whole-store scan. (The script now records a run without a copy as a `fail`; the copy is polled every 0.2 s while the session runs, so lines written in the last 0.2 s before zygo removes the folder at exit may be missing from it.) `fixtures/claude-code/live/28-store-scan.txt`: the same two prefixes over every file of `~/.local/share/zygo` (21 files) and `~/.cache/zygo` (117 files), raw `usage.db`, `usage.db-wal`, `usage.db-shm` included, symbolic links followed only when their target is inside these two dirs (any other link is refused, named as a failure and not read), read errors counted; run read-only after the #28 run with `live-passthrough.sh --scan-only`, which rewrites this file only when the scan passes; 0 read errors, 0 refused links, 0 hits (two earlier scans of the same day, one without link following or the read-error count and one that followed every link, also found 138 files and 0 hits). With a session id, another session's token file is not counted only when its `url` is exactly `http://127.0.0.1:<port>` or `http://[::1]:<port>` and `<url>/healthz` answers with no proxy; any other `url` counts its token file. Side effects on the owner's real `HOME`: the test row stays in the owner's usage store; zygo creates or refreshes `~/.local/share/zygo/plugins`; Claude Code writes a transcript folder under `~/.claude/projects/` (named after the run's temporary repository) and possibly a `projects` entry in `~/.claude.json` (the owner may delete the transcript folder; the script does not); the script now sets `DISABLE_AUTOUPDATER=1` and takes the session id from the run's usage row (the 2026-11-19 run had neither; its id came from the new session folder). §20 #28 |
| P-N24-1 | A `Bash` command with a non-zero exit fires `PostToolUseFailure`; an interrupted command's `tool_response.interrupted` is true (§13.3) | Claude Code | 2.1.328 | 2026-11-19 | fail | `fixtures/claude-code/live/p-n24-1-exit.json` (`exit 3` fires `PostToolUseFailure`: holds), `fixtures/claude-code/live/p-n24-1-interrupt.json` (a running command interrupted with Escape fires neither `PostToolUse` nor `PostToolUseFailure` within 30 s: no `interrupted` input exists); §13.3 |
| P-N25-1 | A `SubagentStop` hook's `additionalContext` (no `decision`) reaches the subagent or the lead (§13.4) | Claude Code | 2.1.328 | 2026-11-19 | pass | `fixtures/claude-code/live/p-n25-1.json`: the subagent's next request carries the text as a `<system-reminder>`; §13.4 |
| P-N28-1 | The status line is shown to the user and its text is in no upstream request (§17, C7) | Claude Code | 2.1.328 | 2026-11-19 | pass | `fixtures/claude-code/live/p-n28-1-pane.txt` (`zygo ◇ … ◇ compact 777k`); no recorded request holds `◇`, `777k` or `777000`; §17 + C7 |
| N15-1M | Window `/context` shows for `design[1m]` whose model is 200k (§10.2.2) | Claude Code | 2.1.328 | 2026-11-19 | fail | `fixtures/claude-code/live/n15-1m-context.txt`: `/context` shows `2.7k/1m tokens`; the `[1m]` suffix sizes the window at 1M, not the 200k of v1's formula. Control run (`live-claude.sh n15c`, `--model design`, same config, no `[1m]`): `fixtures/claude-code/live/n15-control-context.txt` shows `2.2k/200k tokens`, so the 1M comes from the suffix alone; §10.2.2 |
| A-20 | `disallowedTools` in an `--agents` entry removes the edit tools from that subagent (conformance A-20) | Claude Code | 2.1.328 | 2026-11-19 | pass | `fixtures/claude-code/live/a20-tools.json`: `zygo-review` has `disallowedTools` and its requests list no `Edit`, `Write` or `NotebookEdit`; `zygo-code` lists `Edit`; §8.1 A-20 |
| 40 | The pinned Claude Code loads the delivered snapshot's skill as `<plugin>:<skill>` (§20 #40) | Claude Code | 2.1.328 | 2026-11-19 | pass | `fixtures/claude-code/live/40-skills.json` (`zygo:zygo-cli`); §20 #40 |
| N35-D | `claude plugin install` plus zygo's selection gives the `zygo doctor` line; `installed_plugins.json` place and layout (§15.3) | Claude Code | 2.1.328 | 2026-11-19 | pass | `fixtures/claude-code/live/doctor-default.txt`, `fixtures/claude-code/live/doctor-custom-dir.txt`, `fixtures/claude-code/live/installed_plugins.json`: `<config dir>/plugins/installed_plugins.json`, top-level `version` (2) and `plugins` keyed `<plugin>@<marketplace>`; the act101-ai/act101 marketplace registers as `act101-marketplace`; §15.3 |
| N14-S | Each built-in `refuse` rule's rewritten schema is accepted in strict mode by its vendor (§7.6) | Claude Code | — | — | not run: deferred after release (owner, 2026-11-19) | `scripts/premises/live-strict.sh` → `fixtures/claude-code/live/n14-strict.json`; §7.6 |
| N14-E | `reasoning.effort` (Responses) and `reasoning_effort` (chat) are accepted for each catalog effort level (§7.6) | Claude Code | — | — | not run: deferred after release (owner, 2026-11-19) | `scripts/premises/live-strict.sh` → `fixtures/claude-code/live/n14-effort.json`; §7.6 |
| R-RT | Reasoning round trip in a tool loop against each vendor (§7.6) | Claude Code | — | — | not run: deferred after release (owner, 2026-11-19) | `scripts/premises/live-rt.sh` → `fixtures/claude-code/live/rt-usage.json`; §7.6 |
| OC-THROW | A guard deny thrown in `tool.execute.before` blocks the tool | OpenCode | 1.18.16 | 2026-11-19 | pass | `fixtures/opencode/live/oc-throw.json`: the write did not happen; the tool result is the redaction guard's text with `is_error: true` |
| OC-SHAPES | `tool.execute.before/after` payloads for `read`, `grep`, `glob`, `webfetch`, `edit`, `write` | OpenCode | 1.18.16 | 2026-11-19 | pass | `fixtures/opencode/live/tool.execute.before-edit.json` and the 11 other files: `edit` args `filePath`, `oldString`, `newString`; `write` args `filePath`, `content` (as `hook_opencode::normalize` expects). Run as role `code`: zygo removes `edit`/`write` from the main role |
| OC-AUTO | `opencode --auto run …` (flag before the subcommand) runs | OpenCode | 1.18.16 | 2026-11-19 | fail | `fixtures/opencode/live/oc-auto.txt`: `zygo opencode --yolo -- run …` starts `opencode --auto run …`, which prints the help and exits 1; `opencode run --auto …` runs |
| W-SHELL | Windows: the shell that runs `apiKeyHelper` and `statusLine`; zygo's double-quoted path works there | Claude Code | 2.1.328 | 2026-11-20 | part 1 pass; part 2 with the new text: not run: no Windows machine (2026-11-22) | Windows 11 x64 (build 26200), PowerShell 7.5.8, Git for Windows 2.51.1, zygo v0.1.0 release asset. Part 1: `statusLine` printed `bash=/usr/bin/bash cmd=%COMSPEC% ps=` (Git Bash: `uname -s` = `MINGW64_NT-10.0-26200`, `cygpath -w /usr/bin/bash` = `C:\Program Files\Git\usr\bin\bash.exe`); `apiKeyHelper` wrote `bash=$0 cmd=C:\WINDOWS\system32\cmd.exe ps=$($PSVersionTable.PSVersion)` (cmd). `Get-Command bash` = `C:\Windows\system32\bash.exe` (the WSL launcher; Claude Code does not use it). With `\Git\` removed from `PATH` the results are the same: Claude Code still finds Git Bash, so the no-Git-Bash (PowerShell) case was not reached. Part 2: `zygo claude` status line `zygo ◇ - ◇ - ◇ ctx -/- ◇ cache - ◇ trimmed 0` (the status line of 2026-11-20, before the HUD; the part 2 text changed with the HUD, HUD design H§9 and H§14 #38, and part 2 with the new text has not run); §8.1 A-1/A-9 |
| W-SHIM | Windows: the npm `claude.cmd` shim as the program | Claude Code | 2.1.328 | 2026-11-20 | pass | Same machine; Claude Code from `npm install --prefix` (`node_modules\.bin\claude.cmd`) first on `PATH`. Reply `ready`, `$LASTEXITCODE` 0; last `usage.db` row `200\|2.1.328`, model `claude-haiku-5-5` |
| W-AGENTS | Windows: inline `--agents` JSON reaches Claude Code intact on the shim path | Claude Code | 2.1.328 | 2026-11-20 | pass | Same machine and shim. The reply lists `zygo-code`, `zygo-design`, `zygo-plan`, `zygo-read`, `zygo-review`, `zygo-triage` beside the built-in agents; exit 0 |
| P-HUD-1/PH1 | The status line shows SGR colors, 16-color and `38;5;n` (HUD design H§12 PH1) | Claude Code | 2.1.330 | 2026-11-22 | verified | tmux 3.7c, Linux; `scripts/premises/live-hud.sh`, `zygo claude --passthrough` on the stored login. capture-pane -e (tmux 3.7c): stripe fg 1,3,6, logo bold fg 15, worktree name fg 172 (38;5;n). `fixtures/claude-code/hud/p-hud-1-compact-200.txt`, `fixtures/claude-code/hud/p-hud-1-fit.json`; H§12 |
| P-HUD-1/PH2 | A two-row status line shows as two rows (H§12 PH2) | Claude Code | 2.1.330 | 2026-11-22 | verified | tmux 3.7c, Linux; passthrough. verbose shows two rows: the logo row, then the reflection row (tmux 3.7c, 200 columns). `fixtures/claude-code/hud/p-hud-1-verbose-200.txt`; H§12 |
| P-HUD-1/PH3 | A row wider than the status area is cut, not wrapped (H§12 PH3) | Claude Code | 2.1.330 | 2026-11-22 | verified | tmux 3.7c, Linux; passthrough. cut: at 70 columns the row is whole (65 of 65 cells shown); at 19 columns (width source default 120) a row of 87 cells is cut. `fixtures/claude-code/hud/p-hud-1-compact-70.txt`, `fixtures/claude-code/hud/p-hud-1-compact-19.txt`, `fixtures/claude-code/hud/p-hud-1-expect-19.txt`; H§12 |
| P-HUD-1/PH4 | The width sources reach the hook, and the status area is the full terminal width (H§12 PH4) | Claude Code | 2.1.330 | 2026-11-22 | failed | tmux 3.7c, Linux; passthrough, in tmux and on a pseudo-terminal. (a) width source in tmux at 200 columns: COLUMNS 200; on a pseudo-terminal of 100 columns: COLUMNS 100; (b) COLUMNS in the hook: tmux 200, pseudo-terminal 100; (c) status area = pane width - 4 at 19 columns (indent 2, right gap 2), row at 70 columns: whole. `fixtures/claude-code/hud/p-hud-1-width.txt`, `fixtures/claude-code/hud/p-hud-1-dump-tmux.json`, `fixtures/claude-code/hud/p-hud-1-dump-pty.json`, `fixtures/claude-code/hud/p-hud-1-fit.json`; H§12 |
| P-HUD-1/PH5 | The stdin `effort` is a string, or an object with a string level (H§12 PH5) | Claude Code | 2.1.330 | 2026-11-22 | verified | tmux 3.7c, Linux; passthrough. stdin effort is {"level":"medium"} (object); stdin keys: session_id transcript_path cwd scratchpad_dir prompt_id effort session_name model workspace version output_style cost context_window exceeds_200k_tokens prompt_cache fast_mode thinking rate_limits. `fixtures/claude-code/hud/p-hud-1-dump-tmux.json`; H§12 |
| P-HUD-1/PH9 | The reflection starts at the column of the logo (H§12 PH9) | Claude Code | 2.1.330 | 2026-11-22 | verified | tmux 3.7c, Linux; passthrough. logo and reflection start at column 6 of the pane (tmux 3.7c); the glyphs of the font are not checked. `fixtures/claude-code/hud/p-hud-1-verbose-200.txt`; H§12 |
| P-HUD-1/PH12 | Observation: a status line run that still goes at the next refresh (H§12 PH12) | Claude Code | 2.1.330 | 2026-11-22 | verified | tmux 3.7c, Linux; passthrough, `refreshInterval` 2 s, status fetch 5 s late. observation, refreshInterval 2 s: the first slow run lived 2.16 s of its 3 s bound; the next run started 2.23 s after its start: Claude Code ends the run early; 22 runs in 45 s. `fixtures/claude-code/hud/p-hud-1-ph12.json`; H§12 |
| P-HUD-1/C7 | The HUD text is in no upstream request (H§12, C7) | Claude Code | 2.1.330 | 2026-11-22 | verified | tmux 3.7c, Linux; mock upstream, as P-N28-1. HUD shown (verbose, --compact 777k); the logo, the reflection, 777k and 777000 are in none of 2 recorded requests (2 hold the tracer prompt); mock upstream. `fixtures/claude-code/hud/p-hud-1-c7.json`, `fixtures/claude-code/hud/p-hud-1-c7-pane.txt`; H§12 + C7 |
| PRB-1/PH1 | The status line shows SGR colors, 16-color and `38;5;n` (HUD design H§12 PH1) | Claude Code | 2.1.330 | 2026-11-22 | verified | tmux 3.7c, Linux; `scripts/premises/live-hud-probe.sh`, probe status line command, `claude --settings` on the stored login. capture-pane -e on tmux 3.7c of the H§6.3 verbose printf bytes: stripe fg 1,3,6, logo bold fg 15, worktree name fg 172 as 38;5;n. `fixtures/claude-code/hud/prb-1-verbose.txt`, `fixtures/claude-code/hud/prb-1-analysis.json`; H§12 |
| PRB-1/PH2 | A two-row status line shows as two rows (H§12 PH2) | Claude Code | 2.1.330 | 2026-11-22 | verified | tmux 3.7c, Linux; probe. the two rows of the H§6.3 verbose printf show as two rows on tmux 3.7c at 200 columns. `fixtures/claude-code/hud/prb-1-verbose.txt`; H§12 |
| PRB-1/PH3 | A row wider than the status area is cut, not wrapped (H§12 PH3) | Claude Code | 2.1.330 | 2026-11-22 | verified | tmux 3.7c, Linux; probe. cut: a row of 302 cells in a 200-column pane shows 196 cells, ruler 193, ends in …: true; the next line does not go on with the row. `fixtures/claude-code/hud/prb-1-wide.txt`; H§12 |
| PRB-1/PH4 | The width sources reach the status line command, and the status area is the full terminal width (H§12 PH4) | Claude Code | 2.1.330 | 2026-11-22 | failed | tmux 3.7c, Linux; probe, in tmux and on a pseudo-terminal. a: /dev/tty in the command: tmux 200 columns: ENXIO, pseudo-terminal 100 columns: ENXIO; b: COLUMNS: tmux 200, pseudo-terminal 100; c: status area = pane width - 4 at 200 columns, indent 2, right gap 2. `fixtures/claude-code/hud/prb-1-env-tmux.json`, `fixtures/claude-code/hud/prb-1-env-pty.json`, `fixtures/claude-code/hud/prb-1-wide.txt`, `fixtures/claude-code/hud/prb-1-analysis.json`; H§12 |
| PRB-1/PH5 | The stdin `effort` is a string, or an object with a string level (H§12 PH5) | Claude Code | 2.1.330 | 2026-11-22 | verified | tmux 3.7c, Linux; probe. stdin effort is of type object, {"level":"medium"}, in every logged run of the tmux and pseudo-terminal sessions; stdin keys of the first run: session_id transcript_path cwd scratchpad_dir effort model workspace version output_style cost context_window exceeds_200k_tokens fast_mode thinking. `fixtures/claude-code/hud/prb-1-stdin-tmux.json`, `fixtures/claude-code/hud/prb-1-stdin-pty.json`; H§12 |
| PRB-1/PH6 | Claude Code counts a Nerd private-use glyph as one cell (H§12 PH6) | Claude Code | 2.1.330 | 2026-11-22 | verified | tmux 3.7c, Linux; probe. Claude Code counts U+F0645 at cell width 1: the row with 10 glyphs shows 10 fewer ruler cells than the ASCII row, 193 against 183. `fixtures/claude-code/hud/prb-1-wide.txt`, `fixtures/claude-code/hud/prb-1-analysis.json`; H§12 |
| PRB-1/PH7 | `✂` U+2702 shows in text presentation, one cell (H§12 PH7) | Claude Code | 2.1.330 | 2026-11-22 | verified | tmux 3.7c, Linux; probe. cell widths only: Claude Code counts U+2702 at cell width 1: the row with 10 glyphs shows 10 fewer ruler cells than the ASCII row, 193 against 183; tmux 3.7c cell width 1; U+FE0F after it in the compact row: false; text or emoji presentation in a terminal font: not checked. `fixtures/claude-code/hud/prb-1-wide.txt`, `fixtures/claude-code/hud/prb-1-compact.txt`, `fixtures/claude-code/hud/prb-1-analysis.json`; H§12 |
| PRB-1/PH9 | Each logo letter is one cell; the reflection starts at the column of the logo (H§12 PH9) | Claude Code | 2.1.330 | 2026-11-22 | verified | tmux 3.7c, Linux; probe. cell widths only: Claude Code counts each logo letter at cell width 1: the row with 20 letters shows 20 fewer ruler cells than the ASCII row, 193 against 173; tmux 3.7c cell width 1; logo and reflection start at column 6 and 6; glyph shapes in a terminal font: not checked. `fixtures/claude-code/hud/prb-1-wide.txt`, `fixtures/claude-code/hud/prb-1-verbose.txt`, `fixtures/claude-code/hud/prb-1-analysis.json`; H§12 |
| PRB-1/PH11 | A status line command that exits before it reads stdin causes no error and no lost line (H§12 PH11) | Claude Code | 2.1.330 | 2026-11-22 | verified | tmux 3.7c, Linux; probe, `refreshInterval` 2 s. a command that closes stdin unread and exits at once: each of 3 or more early runs that ended 2.5 s before the last capture showed its line, no run was lost to a stdout error or a missing end, no error text, and the status line went on after the phase; EPIPE likely not exercised: each logged stdin payload is smaller than a 64 KiB pipe buffer, so Claude Code's write likely ended before the command closed stdin. `fixtures/claude-code/hud/prb-1-early.txt`, `fixtures/claude-code/hud/prb-1-events.json`; H§12 |
| PRB-1/PH12 | Observation: a status line run that still goes at the next refresh (H§12 PH12) | Claude Code | 2.1.330 | 2026-11-22 | observed | tmux 3.7c, Linux; probe, `refreshInterval` 2 s, a command that sleeps 5 s. refreshInterval 2 s, a command that sleeps 5 s: Claude Code ends a running command when the next refresh comes; signals seen: TERM; concurrent commands at most: 1; slow line shown: false. `fixtures/claude-code/hud/prb-1-events.json`, `fixtures/claude-code/hud/prb-1-analysis.json`; H§12 |
| PRB-1/PH13 | OpenCode has no command status line that zygo could set (H§12 PH13) | OpenCode | 1.19.6 | 2026-11-22 | verified | Linux; `opencode --help`, `opencode debug --help`, `opencode debug config --pure` with a temporary `HOME`. OpenCode 1.19.6: --help and debug --help exit 0 and name no status line; debug config --pure with each of 3 top-level keys drops it: statusLine dropped, statusline dropped, status_line dropped; control: username kept, zygoProbeUnknownKey dropped; nested keys not tested. `fixtures/claude-code/hud/prb-1-ph13.json`; H§12 |
| P2/PW | Workspace agent premises: session ids, hooks, resume and SIGTERM on Claude Code and OpenCode (workspaces design W§17 PW1-PW5, PW7, PW9, PW10, PW16) | Claude Code + OpenCode | 2.1.330 + 1.19.6 | 2026-11-22 | 7 verified, 2 failed | Linux (macOS not run); `scripts/premises/live-workspace.sh`, mock upstream, and one turn on the stored login (sub-check 5.5; about 2 model requests per `c5l` run, 12 in all over the 6 runs of part `c5l` of P2, full runs and part runs). PW1, PW2, PW3, PW4, PW5, PW9, PW10 verified; PW7 failed (7.2, 7.3, 7.5); PW16 failed (16.2). Side effect: 6 empty `projects/<scratch repository>/memory` folders, 5 in `~/.claude/projects/` (round 1 runs, no `CLAUDE_CONFIG_DIR`) and 1 in the operator's own config folder (`CLAUDE_CONFIG_DIR`, round 2 run); they stay in place. `fixtures/claude-code/workspace/pw*.txt`, `fixtures/opencode/workspace/pw4.txt`; one row per id in `docs/premises.md`; W§17 |
| FIX21-LOGIN | `zygo claude --passthrough` (print mode, `--model haiku`) commits in a new repository; the message holds no Claude attribution (§8.5, §13.4) | Claude Code | 2.1.330 | 2026-11-22 | pass | Linux; `scripts/premises/live-attribution.sh login`, the stored login. Exit 0, one new commit "Add hello.txt containing a greeting" with a body and a `Zygo-Session` trailer, no attribution form. `fixtures/claude-code/live/fix21-login.json` |
| FIX21-BARE-ZYGO | `zygo claude --passthrough -- --bare` (§12.3) | Claude Code | 2.1.330 | 2026-11-22 | recorded | Exit 2: "zygo: --passthrough does not work with --bare: bare mode reads only API keys" |
| FIX21-BARE | Claude Code alone, `--bare` with zygo's two attribution keys in `--settings`, commits on the stored login | Claude Code | 2.1.330 | 2026-11-22 | fail: cannot sign in | Exit 1, stdout "Not logged in · Please run /login": bare mode reads only `ANTHROPIC_API_KEY` or an `apiKeyHelper` (`claude --help`). No request was sent. `fixtures/claude-code/live/fix21-bare.json`; FIX21-MOCK covers bare mode |
| FIX21-MOCK | Claude Code's commit attribution text is in no request of a zygo session, with or without `--bare` (§8.5) | Claude Code | 2.1.330 | 2026-11-22 | pass | Linux; mock upstream. Control (Claude Code alone): 1 of 2 requests, the main one (12 tools), holds the commit attribution reminder with the noreply address in angle brackets; control `--bare` (recorded): 0 of 2, its main request has Bash, Edit and Read; zygo: 0 of 1 (12 tools, Bash among them); zygo `--bare`: 0 of 1 (9 tools, Bash among them). `fixtures/claude-code/live/fix21-mock.json` |
| PD2 | A `work_verdict` argument object of 256 KiB (262,144 bytes of compact JSON, the D§9.2 bound) reaches the MCP server unchanged (decision records D§18 PD2) | Claude Code | 2.1.332 | 2026-11-25 | verified | macOS; print mode, mock upstream (no key, no model), temporary `HOME`, `--strict-mcp-config` with a stub stdio MCP server named `zygo` that lists zygo's `work_verdict` schema. The mock streamed the object as 513 `input_json_delta` events of at most 512 bytes each. Sent 262144 bytes, sha256 3bd49a86ddb3280422777d34f8acb1cf586d935656b79f4ec615aa914753bb6a; received 262144 bytes, sha256 3bd49a86ddb3280422777d34f8acb1cf586d935656b79f4ec615aa914753bb6a, equal: true; raw line holds the sent bytes: true; `PreToolUse` `tool_input` equal: true; tool result error: false. Limit: print mode, main loop only, no `--agents` and no zygo proxy; zygo's interactive launch path (`crates/zygo-launch/src/claude_code.rs:622`) was not exercised. D§18 names 2.1.328; 2.1.332 was used because the pinned version is a minimum (Method). `fixtures/claude-code/live/pd2.json`; D§18, D§9.2 |
| PD3 | The `PreToolUse` input for `work_verdict` from the main loop has no `agent_id` key, so the lead-only deny (`crates/zygo-work/src/hooks.rs:184-185`) lets the lead's call through (decision records D§18 PD3, D§8.4) | Claude Code | 2.1.332 | 2026-11-25 | verified | macOS; the PD2 run, print mode, main loop only. 2 `PreToolUse` inputs for `mcp__zygo__work_verdict`; `agent_id` key in 0; keys: `cwd`, `effort`, `hook_event_name`, `mcp_server`, `permission_mode`, `prompt_id`, `session_id`, `tool_input`, `tool_name`, `tool_use_id`, `transcript_path`. Limit: print mode, main loop only, no `--agents` and no zygo proxy; zygo's interactive launch path (`crates/zygo-launch/src/claude_code.rs:622`) was not exercised. D§18 names 2.1.328; 2.1.332 was used because the pinned version is a minimum (Method). `fixtures/claude-code/live/pd3.json`; D§18, D§8.4 |
| M12 | With no prompt about mail, a member started from a loadout sends a message to another member by name when its task needs one, and ends its turn without a poll (workspace mail M§13 #12, first half; M§8a) | Claude Code | 2.1.332 | 2026-11-25 | verified | macOS; `scripts/premises/live-mail.sh --m12`, two passthrough members on the stored claude.ai login, zygo 0.2.1, tmux 3.8, task "Tell beta that the build is green.". 3 of 3 sub-checks passed. Tool calls of alpha's task turn: ToolSearch, mcp__zygo__mail_send. `fixtures/claude-code/mail/m12.txt`; M§13 #12 |
