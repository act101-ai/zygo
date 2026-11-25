# PC-6 — Codex resume and update captures: PC10, PC16

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.3.7 rows PC10, PC16; K-D22 (the shared session store), K-D25 (premises against the mock, with no zygo between), K§6.5.4 (the fact PC16 measures; the fact itself is set in P4-9); K§11 items 9 and 10 (for these fixtures)
**Files:** `scripts/premises/codex/resume.sh` (new), `fixtures/codex/workspace/` (new), `fixtures/codex/update/` (new), `fixtures/codex/runs/resume/` (new)
**Consumes:** `run_codex` (PC-2), `codex_env` (PC-2), `codex_record` (PC-2)
**Produces:** `fixtures/codex/workspace/` (PC10: `SessionStart` `session_id` against the rollout id; `codex resume <id>` with a new scratch `CODEX_HOME` whose `sessions` links to a shared scratch store), `fixtures/codex/update/` (PC16 as P4-3 can measure it: `check_for_update_on_startup` at its default and the `CONNECT` record of the release-source request, refused by the mock), and in this node file under "Observed" one line per id: `- PCn: <verified|failed> <YYYY-MM-DD>: <fixture paths>`
**Acceptance:** `sh scripts/premises/run-codex.sh resume && N2_FORBIDDEN_NAMES="$(id -un)" sh scripts/check-premises.sh && test "$(for d in workspace update; do find fixtures/codex/$d -type f -size +2c -print -quit; done | grep -c .)" -eq 2 && grep -qx 'home=scratch' fixtures/codex/runs/resume/isolation.txt && grep -qx 'codex_home=scratch' fixtures/codex/runs/resume/isolation.txt && test -f fixtures/codex/runs/resume/connect.txt && test -z "$(grep -v ' refused$' fixtures/codex/runs/resume/connect.txt)" && ! grep -qx 'Written when the node runs.' docs/work/phase4/nodes/PC-6.md && test "$(grep -cE '^- (PC10|PC16): (verified|failed) [0-9]{4}-[0-9]{2}-[0-9]{2}: fixtures/codex/' docs/work/phase4/nodes/PC-6.md)" -eq 2` → every command exits 0: each of the two folders holds a nontrivial capture; the run used scratch `HOME` and `CODEX_HOME`, and every `CONNECT` (the release-source request among them) was refused; "Observed" has no placeholder and one dated result line for each of PC10 and PC16

## Planning notes

- PC16 "under a zygo launch" (K§6.3.7) cannot run before the Codex client exists: there is no `zygo codex` in P4-3. Here PC16 measures the client's own default against the mock directly (K-D25). The measurement under a zygo launch is owned by P4-9 (the Codex launch keeps the key unset, K§6.3.1) and P4-11 (the `announces_own_updates` fact, K§11 item 52). The "Observed" line for PC16 states this scope.
- The release-source request of PC16 is refused at the mock's `CONNECT`; no request reaches the npm registry or another real host. The text of a real notice is LC-CX1, owner-run (PC-7 adds the row as `not run`).
- The shared store and its link are in the scratch tree; the link is a symbolic link (the Windows junction of K§6.3.1 is P4-9).
- The turn sequence before and after `codex resume` comes from the PC-2 mock deliverable 2; every mock and scrub change this node needs is a PC-2 deliverable. PC-7 writes the rows from "Observed".
- Acceptance fix (author's call, lead, 2026-11-25): the placeholder check was `! grep -q 'Written when the node runs' …`, which matched its own text in the Acceptance line and so could never pass. It is now `! grep -qx 'Written when the node runs.' …`, anchored to the whole placeholder line under "Observed".

## Observed

Written when the node runs.

## Plan

Base: `origin/phase-4` at 5cb6a387 (PC-1, PC-2 and OC-1 merged). Premise checked 2026-11-25 against the live code:

- `scripts/premises/run-codex.sh:17-19` runs `sh codex/<part>.sh` for `run-codex.sh <part>`; a new part needs no edit there.
- `scripts/premises/lib.sh`: `new_run` `:82` (sets `RUN`, `H`, `R`, and `CX_HOME=$H/.codex` `:88`; `R` is a git repository with one commit), `start_mock [rules]` `:105`, `n2_trap_chain` `:126`, `stop_mock` `:153`, `check_abort` `:160` (exit 3 on `rec/ABORT`), `scrub` `:196`, `CODEX_FX` `:209`, `EXTRA_ENV_ALLOW` `:216` (`RUST_LOG RUST_BACKTRACE`), `codex_env` `:332` (`CX_HOME=${CX_HOME:-$H/.codex}` `:333`; `CX_HOME` must be under `$RUN` `:336`; an existing `$CX_HOME/config.toml` is accepted when its one `base_url` line is `"$MOCK_URL/v1"` `:345-347`, else it writes the base file `:351-353`), `run_codex` `:370` (one `cx-used.d` record per call, also from a background subshell; returns Codex's status), `codex_record` `:400`.
- `scripts/premises/mock_upstream.py`: with no rules, every `POST …/v1/responses` gets the default Responses SSE with text `ok` (`:79-95`, `:356-358`); each request is recorded as `rec/<nnnn>.json` with `seq`, `method`, `path`, `headers`, `body` (`:336-340`); a `CONNECT` is refused and appended to `rec/connect.txt` as `<host>:<port> refused` (`:219-220`, `:205-217`); a non-origin request-target as `<host>:<port> <METHOD> refused` (`:272`).
- `scripts/premises/scrub.py:9`: UUIDs become numbered placeholders per file, so two equal ids in one fixture file keep one placeholder (equality survives the scrub); run paths map to `/tmp/run`, `/home/user`, `/repo` (`:5`).
- `fixtures/codex/config.schema.json` (pinned): `check_for_update_on_startup` "Defaults to `true`"; `hooks.SessionStart` is an array of matcher groups `{matcher, hooks: [{type: "command", command, …}]}` (`HooksToml`, `MatcherGroup`, `HookHandlerConfig`); feature names `codex_hooks` and `hooks` both exist; `projects.<path>.trust_level` is `trusted|untrusted`.
- `fixtures/codex/help.txt`: `codex resume [OPTIONS] [SESSION_ID] [PROMPT]` is the interactive TUI (`:271-279`); `codex exec … resume` is the non-interactive form (`:148`); `--dangerously-bypass-hook-trust` exists on `codex`, `codex exec` and `codex resume` (`:102`, `:212`, `:349`); `codex exec --json` prints JSONL events (`:233`).
- `scripts/premises/lib_test.sh:22-47`: the compiled stub `codex` (`stub.c`, `mkcx <file> <version>`): `--version` prints `codex-cli <version>`; any other call writes `stub-env.txt`, `stub-exe.txt`, `stub-args.txt` under `$HOME` and prints `stub`.

Drift and decisions (author's calls; §5.4):

- **`codex resume <id>` is the TUI** and needs a terminal; `run_codex` (`lib.sh:377`) runs Codex with the caller's stdio. The part gives that run a pseudo-terminal: a Python helper (stdlib `os.openpty`) holds the master side and the part runs `run_codex … <"$PTY" >"$PTY" 2>&1 &`. No `lib.sh` change. The exec form (`codex exec resume <id>`) runs beside it as supporting evidence. PC10's result is decided by the TUI form, which is what WA-1's `resume_args(id) = resume <id>` runs.
- **PC16 is read from a plain TUI start** (`codex <prompt>`, the form a zygo launch runs), not from `exec`, because Codex checks for updates in the TUI. The scope line under "Observed" says that this is the client's default against the mock with no zygo between (contract planning note).
- **No mock rules.** The contract's planning note says the turn sequence comes from the PC-2 rule sequence. The default `ok` reply is enough here: the evidence that a resume loaded the rollout is turn 1's user tracer in the resumed run's request. So no rules file and no authored SSE are needed.
- **The task header names `CODEX_EXTRA_ALLOW`.** No such name exists. The allow-list is `EXTRA_ENV_ALLOW` (`lib.sh:216`). This node sets no `EXTRA_ENV`, so it needs no allow-list change and no file outside its Files.
- PC-7's Consumes names `fixtures/codex/workspace/` but not `fixtures/codex/update/`. PC-7 takes its paths from the "Observed" lines, so it needs no change.

Safety (the operator's rules for this node):

- Real Codex runs only through `run-codex.sh resume` (the PC-2 harness: `env -i`, scratch `HOME` and `CODEX_HOME`, proxies at the mock, only the package copy from `~/.codex/packages`). Never read, list or copy anything else in `~/.codex`. Never `codex login`. No real token. No network but 127.0.0.1. No new host.
- Steps 1-7 use the stub only. Step 8 (the real run) is run by the lead, alone (no other node's real run at the same time), and only after step 7 passes.
- The part kills only processes whose command line holds its own run folder (`$RUN/codex-pkg`), never an owner process.
- Wait bounds in the part (polls of 0.5 s up to a limit) are observation limits in a premise script that CI never runs (K-C5). They are not assertions and no CI test depends on them.

### Interfaces

- Consumes (PC-2, as in the code now):
  - `new_run <name>` → `RUN`, `H`, `R`, `CX_HOME=$H/.codex`; `start_mock` (no argument) → `MOCK_URL`, `MOCK_PID`; `stop_mock`; `check_abort`; `scrub <file>` → stdout; `n2_trap_chain <command>`; `DUMMY_KEY`; `CODEX_FX`; `PREMISES_DIR`.
  - `CX_HOME`: the part sets it to `$RUN/<name>` before each `run_codex`. `codex_env` accepts the part's own `config.toml` when its only `base_url` line is `base_url = "<MOCK_URL>/v1"`.
  - `run_codex <codex args>`: returns Codex's status; writes one `cx-used.d` record per call.
  - `codex_record <part>`: after `stop_mock`; writes `$CODEX_FX/runs/<part>/isolation.txt` and `connect.txt`; exit 1 on any isolation failure.
  - Mock records: `$RUN/rec/<nnnn>.json` (`seq`, `method`, `path`, `body`); `$RUN/rec/connect.txt` lines `<host>:<port> refused` or `<host>:<port> <METHOD> refused`.
- Produces:
  - `scripts/premises/codex/resume.sh`, run as `sh scripts/premises/run-codex.sh resume`; exit 0 when every run ran and every fixture was written, whatever PC10 and PC16 measured; exit 1 on a harness failure (message `codex resume: <why>`); exit 3 on a mock abort.
  - `fixtures/codex/workspace/` (PC10): `summary.txt`, `ids.txt`, `hooks.jsonl`, `rollout-meta.json`, `request-a.json` … `request-d.json` (each only when that run made a `/v1/responses` request), `codex-home-files.txt`, `tui-c.txt`.
  - `fixtures/codex/update/` (PC16): `summary.txt`, `connect-e.txt`, `config.toml`, `tui-e.txt`.
  - `fixtures/codex/runs/resume/isolation.txt`, `connect.txt` (from `codex_record resume`).
  - Under "Observed" in this file, the two lines that the part prints last, in PC-7's form `- PCn: <verified|failed> <YYYY-MM-DD>: <fixture paths>`, each followed by one `  Scope: …` line.

### The runs (one run folder `codex-resume`, one mock, in this order)

| Run | `CODEX_HOME` | `sessions` | Command (after `run_codex`) | Measures |
|---|---|---|---|---|
| a | `$RUN/cx-a` | link → `$STORE` | `exec --json --dangerously-bypass-hook-trust "Reply ok. $T1"` | the rollout id; `SessionStart` `session_id` |
| b | `$RUN/cx-b` | link → `$STORE` | `exec --json --dangerously-bypass-hook-trust resume "$ID" "Reply ok. $T2"` | exec resume (supporting) |
| c | `$RUN/cx-c` | link → `$STORE` | `resume --dangerously-bypass-hook-trust "$ID" "Reply ok. $T3"` (TUI, on the pty) | PC10: `codex resume <id>` in a new `CODEX_HOME` |
| d | `$RUN/cx-d` | none | `exec --json --dangerously-bypass-hook-trust resume "$ID" "Reply ok. $T4"` | control: with no link |
| e | `$RUN/cx-e` | none | `--dangerously-bypass-hook-trust "Reply ok. $T5"` (TUI, on the pty) | PC16: update check at start |

`STORE=$RUN/store/sessions`. Tracers: `T1=pc10-tracer-one`, `T2=pc10-tracer-two`, `T3=pc10-tracer-three`, `T4=pc10-tracer-four`, `T5=pc16-tracer-five`. `ID` is the UUID of the one rollout file that run a made under `$STORE`. If run a made none (the stub), `ID=00000000-0000-0000-0000-000000000000`, so every later run still runs (and fails as a measurement).

Each `config.toml` (written by the part before the run; the base text of `lib.sh:351-353` plus three sections; no `check_for_update_on_startup` key):

```toml
model = "gpt-mock"
model_provider = "mock"

[model_providers.mock]
name = "mock"
base_url = "<MOCK_URL>/v1"
env_key = "MOCK_API_KEY"
wire_api = "responses"

[features]
codex_hooks = true
hooks = true

[projects."<realpath of R>"]
trust_level = "trusted"

[[hooks.SessionStart]]
[[hooks.SessionStart.hooks]]
type = "command"
command = "<RUN>/hook.sh"
```

Both feature names are set because the pinned schema lists both. Which of them gates hooks is PC5's measurement (PC-5), so PC10 does not depend on that result. `$RUN/hook.sh` (mode 755): `#!/bin/sh`, then `cat >"$(mktemp "<RUN>/hooks/XXXXXX")"` (one file per hook call). It prints nothing and exits 0.

### Steps

- [x] 1. Build the stub for the dry runs: in `S=$(mktemp -d)` (prefix `pc6i-`), `mkdir -p "$S/stub-bin" "$S/fx"`; write `$S/stub.c` as `scripts/premises/lib_test.sh:26-45` (the text between `<<'C'` and `C`) and `cc -DVER='"0.172.0"' -o "$S/stub-bin/codex" "$S/stub.c"`; copy `fixtures/codex/config.schema.json` and `config.schema.source.txt` into `$S/fx`. Pass: `"$S/stub-bin/codex" --version` prints `codex-cli 0.172.0`; `file -b "$S/bin/codex"` starts with `Mach-O` and holds `executable`.
- [x] 2. Red: `N2_SCRATCH=$S/scr CODEX_FX=$S/fx PATH="$S/stub-bin:$PATH" sh scripts/premises/run-codex.sh resume` → exit 1, `run-codex: no part resume`.
- [x] 3. Write `scripts/premises/codex/resume.sh` (new), setup part. Part header (`run-codex.sh` comment form: what it measures, PC10 and PC16, K§6.3.7, K-D22, K-D25; it writes `fixtures/codex/workspace/`, `update/`, `runs/resume/`); `PREMISES_DIR=$(cd "$(dirname "$0")/.." && pwd)`, `N2_AGENTS=codex`, `. "$PREMISES_DIR/lib.sh"`; `new_run codex-resume`; `n2_trap_chain pc6_cleanup`; `start_mock` (no rules); `STORE=$RUN/store/sessions`; `mkdir -p "$STORE" "$RUN/hooks" "$RUN/out/workspace" "$RUN/out/update"`; write `$RUN/hook.sh`. Functions:
  - `pc6_cleanup`: kill `$PTY_PID` when set; `pkill -f "$RUN/codex-pkg" 2>/dev/null || true`.
  - `pc6_home <name> <link 0|1>`: `CX_HOME=$RUN/<name>`; `mkdir -p`; with 1, `ln -s "$STORE" "$CX_HOME/sessions"`; write `$CX_HOME/config.toml` (the text above).
  - `pc6_mark <name> <before|after>`: appends `<name> <before|after> <highest rec/<nnnn>.json seq, 0 when none> <line count of rec/connect.txt, 0 when missing>` to `$RUN/marks.txt`; with `after`, it moves the hook files in `$RUN/hooks/` to `$RUN/hooks.d/<name>/`, so each run's hook calls are its own (one `hook.sh` serves all runs). The mock writes one file per seq (`mock_upstream.py:277-282`, `:321-339`), so the highest seq equals the count; the seq stays right if a file is ever missing.
- [x] 4. In the same file: `pc6_exec <name> <codex args…>`: `pc6_mark <name> before`; `run_codex "$@" </dev/null >"$RUN/<name>.out" 2>"$RUN/<name>.err" || rc=$?`; `check_abort`; `pc6_mark <name> after`; append `<name> exit <rc>` to `$RUN/marks.txt`.
- [x] 5. In the same file: `pc6_tui <name> <tracer> <codex args…>`, a TUI run on a pseudo-terminal:
  - Start the helper in the background: `python3 -I - "$RUN/<name>.pty" "$RUN/<name>.raw" "$RUN/<name>.stop" <<'PY' &`, `PTY_PID=$!`. The helper: `m, s = os.openpty()`; `TIOCSWINSZ` 50 rows × 200 columns on `s`; writes `os.ttyname(s)` to `<name>.pty` (temporary file, then rename). Then it loops on `select([m], [], [], 0.2)`: appends each read to `<name>.raw`; answers `ESC [ 6 n` with `ESC [ 1 ; 1 R` and `ESC [ c` with `ESC [ ? 1 ; 2 c`, keeping the bytes split across reads in a 16-byte tail. When `<name>.stop` exists, it writes `\x03`, waits 1 s, writes `\x03`, drains for 3 s, closes `s` and `m` and exits 0. It also stops after 300 s. Codex's TUI queries the cursor position at start and fails when nothing answers.
  - Wait up to 10 s (0.5 s polls) for `<name>.pty`, else `codex resume: no pseudo-terminal`, exit 1. `pc6_mark <name> before`; `run_codex "$@" <"$PTY" >"$PTY" 2>&1 &`, `CX_PID=$!`.
  - Poll every 0.5 s, up to 180 times, until a `rec/*.json` newer than the mark has `path` ending `/v1/responses` and holds `<tracer>`, or `CX_PID` no longer runs. When the tracer was seen, poll up to 20 more times until `grep -rqF <tracer> "$CX_HOME/sessions/"` (the rollout write).
  - `touch <name>.stop`; poll up to 40 times for `CX_PID` to end, else `pkill -f "$RUN/codex-pkg"` and record `killed`. `wait "$CX_PID" || rc=$?`; `wait "$PTY_PID"`; `PTY_PID=`; `check_abort`; `pc6_mark <name> after`; append `<name> exit <rc|killed>` to `marks.txt`.
- [x] 6. In the same file: the sequence and the record.
  - `pc6_home cx-a 1; pc6_exec a exec --json --dangerously-bypass-hook-trust "Reply ok. $T1"`. Then `ID` = the UUID in the name of the one `rollout-*-<uuid>.jsonl` under `$STORE` (else the nil UUID).
  - b, c, d and e as the table says (c and e through `pc6_tui` with `$T3` and `$T5`).
  - Then one `python3 -I - "$RUN" "$ID" <<'PY'` that writes the raw outputs under `$RUN/out/workspace/` and `$RUN/out/update/` from `marks.txt`, `rec/`, `hooks/`, `$STORE` and the `cx-*` folders. A run's requests are the records whose `seq` lies in its before/after window, and its connect lines are those in its window.
    - `ids.txt`: `rollout_id=`, `rollout_meta_id=` (`payload.id` of the rollout's first line), `exec_thread_id=` (`thread_id` of run a's `thread.started` event), `session_start_<a|b|c|d|e>=` (each `session_id` of a `SessionStart` payload in that run's hook files, comma-separated, `none` when none).
    - `summary.txt`, one `key=value` per line: `session_start_fired_a=yes|no`; `session_id_equals_rollout_id=yes|no` (every `session_start_a` id = `rollout_id` = `rollout_meta_id`); `exec_resume_exit=<rc>`; `exec_resume_history=yes|no` (run b's first `/v1/responses` body holds `$T1` and `$T2`); `tui_resume_exit=<rc|killed>`; `tui_resume_history=yes|no` (same for run c with `$T1` and `$T3`); `tui_resume_in_store=yes|no` (a file under `$STORE` holds `$T3`); `links_kept=yes|no` (`cx-a`, `cx-b`, `cx-c` `sessions` are still symbolic links to `$STORE`); `store_rollouts=<count>`; `no_link_resume_exit=<rc>`; `no_link_resume_history=yes|no` (run d, `$T1` and `$T4`); `pc10=verified|failed`.
    - `pc10=verified` only when `session_id_equals_rollout_id`, `tui_resume_history`, `tui_resume_in_store` and `links_kept` are all `yes`.
    - `hooks.jsonl`: one line `{"run": "<x>", "payload": <payload>}` per hook file (`hooks.d/<x>/`).
    - `rollout-meta.json`: run a's rollout first line.
    - `request-<a..d>.json`: each run's first `/v1/responses` record.
    - `codex-home-files.txt`: for each of `cx-a` … `cx-e` and `store`, the sorted relative paths of its files and links, not followed into the link, each as `<folder>/<path>`, with `-> store` after a link.
    - `tui-c.txt` and `../update/tui-e.txt`: the `.raw` text with `\x1b\[[0-?]*[ -/]*[@-~]`, `\x1b\][^\x07\x1b]*(\x07|\x1b\\)` and any other `\x1b.` removed and `\r` dropped.
    - `../update/connect-e.txt`: run e's connect lines.
    - `../update/config.toml`: `cx-e/config.toml`.
    - `../update/summary.txt`: `config_key=absent` (no `check_for_update_on_startup` line in `cx-e/config.toml`), `schema_default=true` (read from the description text in `$CODEX_FX/config.schema.json`), `tui_start_exit=<rc|killed>`, `tui_start_connects=<the host:port of each run e connect line, comma-separated, none>`, `exec_connects=<the same for runs a, b and d>`, `release_source_request=seen|not-seen` (seen: a run e line whose host is not `127.0.0.1`, `localhost` or `[::1]`), `pc16=verified|failed` (verified only when `config_key=absent`, `schema_default=true` and `seen`).
  - Then `stop_mock`. Clear `$CODEX_FX/workspace` and `$CODEX_FX/update`, and write `scrub` of each `$RUN/out/<dir>/<file>` into them. Then `codex_record resume`.
  - Last, print the two Observed lines: `- PC10: <pc10> <date +%Y-%m-%d>: <paths>` and `- PC16: <pc16> <date>: <paths>`. The paths are `fixtures/codex/<dir>/<file>`, only files of more than 2 bytes that are not `{}` or `[]`, `summary.txt` first. Each line is followed by its scope line:
    - PC10: `  Scope: codex resume <id> (TUI, pseudo-terminal) in a new CODEX_HOME whose sessions links to a shared store; exec resume and a CODEX_HOME with no link recorded beside it.`
    - PC16: `  Scope: Codex's own default against the mock with no zygo between (K-D25); under a zygo launch: P4-9 and P4-11; the notice text: LC-CX1 (owner-run).`
- [x] 7. Stub dry run (no real binary; `command -v codex` names `$S/stub-bin/codex`): `sh -n scripts/premises/codex/resume.sh` and `shellcheck -s sh scripts/premises/codex/resume.sh` print nothing (the part carries `disable` lines, each with its reason: SC2034 `N2_AGENTS` and SC1091 the sourced `lib.sh`, which `base.sh` also draws, and SC2094 the pty as input and output). Then `N2_SCRATCH=$S/scr CODEX_FX=$S/fx PATH="$S/stub-bin:$PATH" sh scripts/premises/run-codex.sh resume`. Pass:
  - exit 0.
  - The last four lines are the `- PC10: failed <today>: fixtures/codex/workspace/summary.txt …` and `- PC16: failed <today>: fixtures/codex/update/summary.txt …` lines, each with its `  Scope:` line.
  - `$S/fx/runs/resume/isolation.txt` holds `home=scratch`, `codex_home=scratch`, `codex_exe=scratch`, `proxy=mock` and `run_versions=0.172.0,0.172.0,0.172.0,0.172.0,0.172.0` (five runs).
  - `$S/fx/runs/resume/connect.txt` is empty.
  - `$S/fx/workspace/summary.txt` has `pc10=failed` and `session_start_fired_a=no`; `$S/fx/update/summary.txt` has `config_key=absent`, `schema_default=true`, `release_source_request=not-seen`, `pc16=failed`.
  - `$S/scr/codex-resume/c.raw` and `e.raw` each hold `stub` (the pty path works).
  - `$S/scr/codex-resume/home/stub-args.txt` is `--dangerously-bypass-hook-trust Reply ok. pc16-tracer-five`.
  - `grep -rlF "$HOME" "$S/fx/workspace" "$S/fx/update"` and `grep -rliw "$(id -un)" "$S/fx/workspace" "$S/fx/update"` print nothing.
  - `pgrep -fl "$S/scr"` prints nothing.

  Then `rm -rf "$S"`. Do not go on unless every check passes.
- [ ] 8. Real run (the lead, alone). Precondition: `pgrep -fl '\.codex/packages'` prints nothing, else stop and report (never kill). `stamp=$(mktemp)`; `sh scripts/premises/run-codex.sh resume` → exit 0. Then:
  - `find "$HOME/.codex" -newer "$stamp" -print` prints nothing.
  - `pgrep -fl codex-pkg`, `pgrep -fl '\.codex/packages'` and `pgrep -fl mock_upstream` print nothing.
  - `rm -f "$stamp"`.

  If the find lists a file, a process is left, or `connect.txt` holds a loopback host, stop and report it as a finding; do not filter the record.
- [ ] 9. Check the captures:
  - `fixtures/codex/workspace/summary.txt` and `fixtures/codex/update/summary.txt` hold every key of step 6.
  - `fixtures/codex/workspace/ids.txt` shows one placeholder for `rollout_id`, `rollout_meta_id` and `session_start_a` when `session_id_equals_rollout_id=yes`.
  - `fixtures/codex/runs/resume/isolation.txt`: `run_versions` five times the real version.
  - Every line of `connect.txt` ends ` refused`.
  - Record a failed PC as found (a measured `failed` is a result, not a defect of this node). A harness error (exit 1) is a defect: fix it and run steps 7 and 8 again.
- [ ] 10. Replace `Written when the node runs.` under "## Observed" with the four lines that step 8 printed last, verbatim.
- [ ] 11. Acceptance (the node's command, exactly as the contract's **Acceptance** line) → exit 0.
- [ ] 12. Commit `scripts/premises/codex/resume.sh`, `fixtures/codex/workspace/`, `fixtures/codex/update/`, `fixtures/codex/runs/resume/` and this file with the message `feat(premises): Codex resume and update captures (PC-6)`, ending with `Zygo-Session: <session>` and `Zygo-Work: phase4:PC-6`, each on its own line. No Claude attribution.

### Author's checks (2026-11-25)

1. Spec coverage: PC10 `session_id` = rollout id → step 6 `session_id_equals_rollout_id`; `codex resume <id>` with a new `CODEX_HOME` linked to a shared store → runs b and c, `links_kept`, `tui_resume_in_store`; PC16 default in effect → `config_key=absent`, `schema_default=true`; release-source request with its `CONNECT` record → run e, `connect-e.txt`; K-D25 → no zygo, mock only; K§11 item 9 → step 6 prints only nontrivial paths with a date and result; item 10 → `scrub` and step 11 (`check-premises.sh`).
2. Step scan: each step names one file or one check. Only the pty answer rule and the ANSI strip rule carry a body, because the signatures do not decide them.
3. Interface match: `run_codex`, `codex_env` (through `CX_HOME` and its own `config.toml`) and `codex_record` as in `lib.sh` now. The Observed form is PC-7's.
4. Review focus 6 (`graph.md`, PC-3 to PC-6, each run): no real host and no read of `~/.codex`. Each of the five runs goes through `run_codex` (one `cx-used.d` record each). Step 7 checks the stub record. Steps 8 and 9 check `isolation.txt` (`home=scratch`, `codex_home=scratch`, `proxy=mock`), that every `connect.txt` line ends ` refused` (the PC16 release-source request among them), `find ~/.codex -newer` and that no process is left. Step 11 runs the same checks through the Acceptance command.
5. Proportion: the plan gives signatures, keys and pass values, not the script.
6. Live code: every cited line was opened on `origin/phase-4` 5cb6a387. No existing helper gives a pseudo-terminal, a per-run record window or a rollout id: `tmux_claude` (`lib.sh:491`) is Claude-only and would bypass `run_codex`. `hook_log.py` (`hook_settings`, `lib.sh:524`) writes Claude hook logs; the part's one-line `hook.sh` is smaller than adapting it.
