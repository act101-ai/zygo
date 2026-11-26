# PC-6 — Codex resume and update captures: PC10, PC16

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.3.7 rows PC10, PC16; K-D22 (the shared session store), K-D25 (premises against the mock, with no zygo between), K§6.5.4 (the fact PC16 measures; the fact itself is set in P4-9); K§11 items 9 and 10 (for these fixtures)
**Files:** `scripts/premises/codex/resume.sh` (new), `fixtures/codex/workspace/` (new), `fixtures/codex/update/` (new), `fixtures/codex/runs/resume/` (new), `docs/specs/2026-11-25-client-model-design.md` (the K§6.3.7 PC16 row and a change-log line, review round 1 F3; K§11 item 52, the K§6.3.8 A-22 row, the K§6.9 P4-10/P4-11 note and K§6.5.4's release source, review round 2 G2 and G3; lead's calls)
**Consumes:** `run_codex` (PC-2), `codex_env` (PC-2), `codex_record` (PC-2)
**Produces:** `fixtures/codex/workspace/` (PC10: `SessionStart` `session_id` against the rollout id; `codex resume <id>` with a new scratch `CODEX_HOME` whose `sessions` links to a shared scratch store), `fixtures/codex/update/` (PC16 as P4-3 can measure it, Codex's own default with no zygo between: `check_for_update_on_startup` at its default, and the release-source request inferred from per-host `CONNECT` counts of a key-default TUI start against a control TUI run with the key `false`, all refused by the mock), and in this node file under "Observed" one line per id: `- PCn: <verified|failed> <YYYY-MM-DD>: <fixture paths>`
**Acceptance:** `sh scripts/premises/run-codex.sh resume && N2_FORBIDDEN_NAMES="$(id -un)" sh scripts/check-premises.sh && test "$(for d in workspace update; do find fixtures/codex/$d -type f -size +2c -print -quit; done | grep -c .)" -eq 2 && grep -qx 'home=scratch' fixtures/codex/runs/resume/isolation.txt && grep -qx 'codex_home=scratch' fixtures/codex/runs/resume/isolation.txt && test -f fixtures/codex/runs/resume/connect.txt && test -z "$(grep -v ' refused$' fixtures/codex/runs/resume/connect.txt)" && ! grep -qx 'Written when the node runs.' docs/work/phase4/nodes/PC-6.md && test "$(grep -cE '^- (PC10|PC16): (verified|failed) [0-9]{4}-[0-9]{2}-[0-9]{2}: fixtures/codex/' docs/work/phase4/nodes/PC-6.md)" -eq 2` → every command exits 0: each of the two folders holds a nontrivial capture; the run used scratch `HOME` and `CODEX_HOME`, and every `CONNECT` (the release-source request among them) was refused; "Observed" has no placeholder and one dated result line for each of PC10 and PC16

## Planning notes

- PC16 "under a zygo launch" (K§6.3.7) cannot run before the Codex client exists: there is no `zygo codex` in P4-3. Here PC16 measures the client's own default against the mock directly (K-D25). The measurement under a zygo launch is owned by P4-9 (the Codex launch keeps the key unset, K§6.3.1) and P4-11 (the `announces_own_updates` fact, K§11 item 52). The "Observed" line for PC16 states this scope.
- The release-source request of PC16 is refused at the mock's `CONNECT`; no request reaches the npm registry or another real host. The text of a real notice is LC-CX1, owner-run (PC-7 adds the row as `not run`).
- The shared store and its link are in the scratch tree; the link is a symbolic link (the Windows junction of K§6.3.1 is P4-9).
- The turn sequence before and after `codex resume` comes from the PC-2 mock deliverable 2; every mock and scrub change this node needs is a PC-2 deliverable. PC-7 writes the rows from "Observed".
- Acceptance fix (author's call, lead, 2026-11-25): the placeholder check was `! grep -q 'Written when the node runs' …`, which matched its own text in the Acceptance line and so could never pass. It is now `! grep -qx 'Written when the node runs.' …`, anchored to the whole placeholder line under "Observed".

## Observed

- PC10: verified 2026-11-25: fixtures/codex/workspace/summary.txt fixtures/codex/workspace/codex-home-files.txt fixtures/codex/workspace/hooks.jsonl fixtures/codex/workspace/ids.txt fixtures/codex/workspace/request-a.json fixtures/codex/workspace/request-b.json fixtures/codex/workspace/request-c.json fixtures/codex/workspace/rollout-meta.json fixtures/codex/workspace/tui-c-warnings.txt fixtures/codex/workspace/tui-c.txt
  Scope: codex resume <id> (TUI, pseudo-terminal) in a new CODEX_HOME whose sessions links to a shared store; exec resume and a CODEX_HOME with no link recorded beside it.
- PC16: verified 2026-11-25: fixtures/codex/update/summary.txt fixtures/codex/update/config-control.toml fixtures/codex/update/config.toml fixtures/codex/update/connect-e.txt fixtures/codex/update/connect-f.txt fixtures/codex/update/tui-e-warnings.txt fixtures/codex/update/tui-e.txt fixtures/codex/update/tui-f-warnings.txt fixtures/codex/update/tui-f.txt
  Scope: Codex's own default against the mock with no zygo between (K-D25); release source inferred from a controlled count difference (one extra api.github.com:443 CONNECT when the key is on), path not visible; under a zygo launch: P4-9 and P4-11; the notice text: LC-CX1 (owner-run).

### Findings

- Loopback records (accepted as recorded, lead's call 2026-11-25): each TUI run (c, e, f) sent one `POST /mcp` via HTTP_PROXY to a 127.0.0.1 port other than the mock's (`127.0.0.1:<port> POST /mcp refused` in `fixtures/codex/runs/resume/connect.txt`, the path recorded since PC-10); refused by the mock; no NO_PROXY is set by the harness (lib.sh, outside this node).
- Release source (lead's count rule, 2026-11-25): per-host CONNECT counts, run e (key default) and run c (key default) `api.github.com:443=2`, control f (key `false`) `=1`, exec runs a and b `=1`, d `0`; all other hosts equal. So the release source is inferred from one extra api.github.com:443 CONNECT per key-default TUI run vs the control (one run per arm; a retry of the baseline request is not excluded); the path is not visible (CONNECT). It is not the K§6.5.4 proposal `registry.npmjs.org` (`release_source_matches_proposed=no`): a finding for K§6.5.4 and P4-11. raw.githubusercontent.com is reached by every TUI start, the control included, so it is not the update check.
- Warnings panel (F2 key): run e's panel shows "Warnings · 1 of 4 · Deprecation: `[features].codex_hooks` is deprecated. Use `[features].hooks` instead"; only the first of the warnings is captured.

## Review round 1 findings

Round 2, 2026-11-25. Tests: a scratch synthetic test runs the part's record block on made-up run folders (F1, F6, F7: 7 of 11 checks red before, 11 green after); scratch stub variants make a harness refusal in TUI run c (F2: before, exit 0 with `c exit 1` and `pc10=failed`; after, exit 1 `codex resume: run c: the harness refused it before Codex started: premises: codex 0.1.0 is below the minimum 0.172.0`) and an exec run d that hangs (F9: before, no bound; after, `d exit killed` and the part goes on).

- F1 (important), PC16 had no control: run e's hosts github.com, api.github.com, chatgpt.com and ab.chatgpt.com also come from the exec runs. Fixed: a control TUI run f, as run e but with `check_for_update_on_startup = false` in its `config.toml`. `release_source_hosts` (the code since the lead's count rule, 2026-11-25): each non-loopback host:port that run e reaches more often than run f, by the same difference in the other key-default TUI run c, with no exec run (a, b, d) above run f; `release_source_excess` gives the difference. `pc16=verified` needs at least one such host, the key absent in run e, `false` in run f, and the schema default `true`. The summary records `connect_counts_<c|e|f|a|b|d>`, `release_source_path=not visible (CONNECT)` and the K§6.5.4 proposal (`release_source_proposed=registry.npmjs.org`, `release_source_matches_proposed=yes|no`). (Round 2 first used a set rule, host absent from f and the exec runs; the recapture showed it cannot see a second request to a shared host; see the recapture item below.)
- F2 (important), a harness refusal in a background run became a measurement: `codex_env` exits in the background subshell, so the part recorded `c exit 1` and exited 0. Fixed: `pc6_wait` compares the `cx-used.d` record count (run_codex writes one after codex_env accepts the run) before and after each run; no new record → exit 1 naming the run and the `premises:` line.
- F3 (important), the K§6.3.7 PC16 row said "Under a zygo launch": rewritten (lead's call, §5.4) to state what is measured (Codex's own default against the mock with no zygo between, K-D25; the release-source request against a control and the exec runs; the host against the K§6.5.4 proposal) and to name P4-9 and P4-11 for the zygo-launch measurement; change-log line added. K§11 item 52 ("each PU and PC16 premise row is `verified` and A-22.2 cites it") stays consistent: unchanged.
- F5 (nit), the loopback POSTs and the TUI warnings: at stop, the pseudo-terminal helper sends F2 (`ESC O Q`), records the raw offset in `<run>.f2`, and then sends Esc and three Ctrl-C. The screen from F2 on is written as `tui-c-warnings.txt`, `tui-e-warnings.txt` and `tui-f-warnings.txt`. The stub cannot show the panel; the recapture records what it holds. The refused loopback path comes with PC-10's mock change.
- F6 (nit): `pc10=verified` also needs `exec_thread_id_equals_rollout_id=yes`, `session_start_c_equals_rollout_id=yes` and `tui_resume_exit=0`.
- F7 (nit): one UUID mapping (`uuid-NNNN`, in order of first appearance over workspace then update files, by name) is applied to every output before scrub; `uuid-NNNN` is not a UUID, so scrub's per-file numbering leaves it alone.
- F8 (nit), time zone and OS: PC-10's (TZ=UTC, the OS scrub); no part change.
- F9 (nit): exec runs run in the background with a bound (240 polls of 0.5 s), then this run folder's Codex is killed (`<run> exit killed`); harness hygiene, not a CI timing test.
- F10: already done; nothing to do.
- Recapture after PC-10 (2026-11-25, kept scratch, not committed): (1) F7 bug, fixed: a UUID in the run folder's own path (a session scratchpad) was mapped before scrub, so scrub no longer matched the path and TUI texts, requests, hooks and configs kept `/tmp/claude-501/…/uuid-NNNN/scratchpad/…`; the mapping now leaves the run path's UUIDs to scrub (synthetic case added, red then green). (2) PC16 measured `failed`: control run f (key `false`) made the same CONNECT hosts as run e (raw.githubusercontent.com, github.com, api.github.com, chatgpt.com, ab.chatgpt.com), so no host is unique to e. By count, every TUI run with the key at its default (c and e here, e in round 1) made two `api.github.com:443` CONNECTs and the control made one; Codex's log database does not name the request. A count-based comparison (run e's requests minus the control's and the exec runs', as multisets) would name `api.github.com:443`; the set rule cannot. The method choice is the lead's (F1). Lead's call (2026-11-25): per-request counts. For each host:port, run e minus the control f; the same positive difference in the other key-default TUI run c; no exec run (a, b, d) above f. `summary.txt` records `connect_counts_<c|e|f|a|b|d>`, `release_source_hosts`, `release_source_excess`, `release_source_path=not visible (CONNECT)` and the proposal; the K§6.3.7 PC16 row follows (synthetic cases from the real counts, red on the set rule, then green). (3) F7 gap, fixed: Codex's message item ids `msg_<uuid v7>` kept their raw UUIDs in `request-*.json` (`\b` does not bound a UUID after `_`, in the mapping and in scrub.py); the mapping now uses hex-only bounds (synthetic case, red then green). scrub.py has the same gap (outside this node; PC-11).
- Also: each TUI run (c, e, f) passes `--no-daemon` (`fixtures/codex/help.txt:132`, `:379`), so no shared per-uid background server outlives the run.

## Review round 2 findings

Round 3, 2026-11-25 (head 49763518 reviewed). Tests: six synthetic cases added to the scratch synthetic test (G1 3, G7 3), red on 49763518 (6 of 22 failed), green after (22 of 22). The Observed lines and fixtures above are round 2's (Acceptance run of 49763518); they are re-taken by the real rerun the lead releases after PC-5, with this round's part.

- G1 (important), fixed: the UUID mapping's lookbehind `(?<![0-9A-Fa-f-])` skipped a UUID after `-`, so rollout file names (`rollout-<date>T<hh>-<mm>-<ss>-<uuid>.jsonl`, in `hooks.jsonl` `transcript_path` and in `codex-home-files.txt`) kept scrub's per-file placeholders. The mapping now also starts after a `-` that follows a non-hex character or a rollout time stamp (`T\d\d-\d\d-\d\d-`), never inside the stamp's digits. Synthetic cases: `transcript_path` and `session_id` share one placeholder; `codex-home-files.txt` names the rollout with the `ids.txt` placeholder; the stamp digits are kept. Re-derivation: the committed fixtures are the Acceptance run's, whose scratch lib.sh removed on exit 0; the real rerun regenerates them.
- G2 (important), fixed (lead's call, §5.4): K§11 item 52 now has A-22.2 cite, for Codex, the P4-11 premise row for Codex under a zygo launch, not PC16 (PC16 measures Codex's own default with no zygo between, K-D25); the PU rows are unchanged. Aligned: the K§6.3.8 A-22 row ("A-22.2 and A-22.3 after the P4-11 premise row for Codex under a zygo launch"), the PC16 row's Feeds column, and the K§6.9 P4-10/P4-11 note (it said "The Codex fact (PC16) is measured in P4-3"). The round 1 change-log line no longer says "item 52 is unchanged"; a new line states the amendment. A-22.2's criterion text is unchanged.
- G3 (nit), fixed: K§6.5.4's release source gains the measured result for Codex (inferred as `api.github.com:443` from PC16's controlled count difference, path not visible; `registry.npmjs.org` not seen; P4-11 verifies before use); change-log line.
- G4 (nit), fixed: the Findings "Release source" line says the inference rests on one extra api.github.com:443 CONNECT per key-default TUI run vs the control, one run per arm, and that a retry of the baseline request is not excluded.
- G5 (nit), fixed: the Produces line, Plan steps 6 and 7 (the count rule's keys, the Scope line's inference clause, six runs, `daemon=none`, the run f stub arguments), the tracers (T6) and round 1 F1's text now match the code; the runs table already had run f.
- G6 (nit), recorded: `"sandbox":"seatbelt"` (the host OS's sandbox name in requests), `/private/tmp` and `marketplace-macos-*` (a host-OS folder name in `codex-home-files.txt`) are scrub gaps for PC-11 (the lead adds them to its scope; scrub.py is outside this node). Codex's base prompt is kept in full in the request fixtures on purpose: RS-1 translates it, and PC-3 keeps it for the same reason. UTC run times (`TZ=UTC`, PC-10) and loopback ports are not private.
- G7 (nit), fixed: a `tui-<c|e|f>-warnings.txt` is written only when the screen from F2 on holds the panel title `Warnings ·`; otherwise there is no file, so Observed does not cite it, and the summary says `warnings_panel=not captured` (`workspace/summary.txt` for run c; `update/summary.txt` `warnings_panel` for run e and `control_warnings_panel` for run f). Stub case: the key echo alone (12 bytes) gives no file.

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
  - `fixtures/codex/update/` (PC16): `summary.txt`, `connect-e.txt`, `config.toml`, `tui-e.txt`; round 2: `connect-f.txt`, `config-control.toml`, `tui-f.txt`, and `tui-<c|e|f>-warnings.txt` (`tui-c-warnings.txt` in `workspace/`; each only when it holds the panel, round 3 G7).
  - `fixtures/codex/runs/resume/isolation.txt`, `connect.txt` (from `codex_record resume`).
  - Under "Observed" in this file, the two lines that the part prints last, in PC-7's form `- PCn: <verified|failed> <YYYY-MM-DD>: <fixture paths>`, each followed by one `  Scope: …` line.

### The runs (one run folder `codex-resume`, one mock, in this order)

| Run | `CODEX_HOME` | `sessions` | Command (after `run_codex`) | Measures |
|---|---|---|---|---|
| a | `$RUN/cx-a` | link → `$STORE` | `exec --json --dangerously-bypass-hook-trust "Reply ok. $T1"` | the rollout id; `SessionStart` `session_id` |
| b | `$RUN/cx-b` | link → `$STORE` | `exec --json --dangerously-bypass-hook-trust resume "$ID" "Reply ok. $T2"` | exec resume (supporting) |
| c | `$RUN/cx-c` | link → `$STORE` | `resume --no-daemon --dangerously-bypass-hook-trust "$ID" "Reply ok. $T3"` (TUI, on the pty) | PC10: `codex resume <id>` in a new `CODEX_HOME` |
| d | `$RUN/cx-d` | none | `exec --json --dangerously-bypass-hook-trust resume "$ID" "Reply ok. $T4"` | control: with no link |
| e | `$RUN/cx-e` | none | `--no-daemon --dangerously-bypass-hook-trust "Reply ok. $T5"` (TUI, on the pty) | PC16: update check at start |
| f | `$RUN/cx-f` (`check_for_update_on_startup = false`) | none | `--no-daemon --dangerously-bypass-hook-trust "Reply ok. $T6"` (TUI, on the pty) | PC16 control (round 2, F1) |

`STORE=$RUN/store/sessions`. Tracers: `T1=pc10-tracer-one`, `T2=pc10-tracer-two`, `T3=pc10-tracer-three`, `T4=pc10-tracer-four`, `T5=pc16-tracer-five`, `T6=pc16-tracer-six` (run f, round 2). `ID` is the UUID of the one rollout file that run a made under `$STORE`. If run a made none (the stub), `ID=00000000-0000-0000-0000-000000000000`, so every later run still runs (and fails as a measurement).

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
  - b, c, d, e and f as the table says (c, e and f through `pc6_tui` with `$T3`, `$T5` and `$T6`; each TUI run passes `--no-daemon`; f's `config.toml` holds `check_for_update_on_startup = false`).
  - Then one `python3 -I - "$RUN" "$ID" <<'PY'` that writes the raw outputs under `$RUN/out/workspace/` and `$RUN/out/update/` from `marks.txt`, `rec/`, `hooks/`, `$STORE` and the `cx-*` folders. A run's requests are the records whose `seq` lies in its before/after window, and its connect lines are those in its window.
    - `ids.txt`: `rollout_id=`, `rollout_meta_id=` (`payload.id` of the rollout's first line), `exec_thread_id=` (`thread_id` of run a's `thread.started` event), `session_start_<a|b|c|d|e>=` (each `session_id` of a `SessionStart` payload in that run's hook files, comma-separated, `none` when none).
    - `summary.txt`, one `key=value` per line: `session_start_fired_a=yes|no`; `session_id_equals_rollout_id=yes|no` (every `session_start_a` id = `rollout_id` = `rollout_meta_id`); `exec_resume_exit=<rc>`; `exec_resume_history=yes|no` (run b's first `/v1/responses` body holds `$T1` and `$T2`); `tui_resume_exit=<rc|killed>`; `tui_resume_history=yes|no` (same for run c with `$T1` and `$T3`); `tui_resume_in_store=yes|no` (a file under `$STORE` holds `$T3`); `links_kept=yes|no` (`cx-a`, `cx-b`, `cx-c` `sessions` are still symbolic links to `$STORE`); `store_rollouts=<count>`; `no_link_resume_exit=<rc>`; `no_link_resume_history=yes|no` (run d, `$T1` and `$T4`); `pc10=verified|failed`.
    - `pc10=verified` only when `session_id_equals_rollout_id`, `tui_resume_history`, `tui_resume_in_store` and `links_kept` are all `yes`.
    - `hooks.jsonl`: one line `{"run": "<x>", "payload": <payload>}` per hook file (`hooks.d/<x>/`).
    - `rollout-meta.json`: run a's rollout first line.
    - `request-<a..d>.json`: each run's first `/v1/responses` record.
    - `codex-home-files.txt`: for each of `cx-a` … `cx-e` and `store`, the sorted relative paths of its files and links, not followed into the link, each as `<folder>/<path>`, with `-> store` after a link.
    - `tui-c.txt`, `../update/tui-e.txt` and `../update/tui-f.txt` (the screen before F2) and `tui-<c|e|f>-warnings.txt` (the screen from F2 on, written only when it holds `Warnings ·`; else no file and `warnings_panel=not captured`): the `.raw` text with `\x1b\[[0-?]*[ -/]*[@-~]`, `\x1b\][^\x07\x1b]*(\x07|\x1b\\)` and any other `\x1b.` removed and `\r` dropped.
    - `../update/connect-e.txt`: run e's connect lines.
    - `../update/config.toml`: `cx-e/config.toml`.
    - `../update/summary.txt`: `config_key=absent` (no `check_for_update_on_startup` line in `cx-e/config.toml`), `schema_default=true` (read from the description text in `$CODEX_FX/config.schema.json`), `tui_start_exit=<rc|killed>`, `tui_start_connects=<the host:port of each run e connect line, comma-separated, none>`, `control_config_key=false`, `control_exit`, `control_connects` (run f), `warnings_panel` and `control_warnings_panel` (`captured|not captured`), `exec_connects=<the same for runs a, b and d>`, `connect_counts_<c|e|f|a|b|d>=<host:port=n,…>` (loopback left out), `release_source_hosts` and `release_source_excess` (the count rule: run e minus run f per host, the same positive difference in run c, no exec run above run f), `release_source_request=seen|not-seen`, `release_source_path=not visible (CONNECT)`, `release_source_proposed=registry.npmjs.org`, `release_source_matches_proposed=yes|no`, `pc16=verified|failed` (verified only when `config_key=absent`, `control_config_key=false`, `schema_default=true` and at least one release-source host). `workspace/summary.txt` also has `warnings_panel` (run c).
  - Then `stop_mock`. Clear `$CODEX_FX/workspace` and `$CODEX_FX/update`, and write `scrub` of each `$RUN/out/<dir>/<file>` into them. Then `codex_record resume`.
  - Last, print the two Observed lines: `- PC10: <pc10> <date +%Y-%m-%d>: <paths>` and `- PC16: <pc16> <date>: <paths>`. The paths are `fixtures/codex/<dir>/<file>`, only files of more than 2 bytes that are not `{}` or `[]`, `summary.txt` first. Each line is followed by its scope line:
    - PC10: `  Scope: codex resume <id> (TUI, pseudo-terminal) in a new CODEX_HOME whose sessions links to a shared store; exec resume and a CODEX_HOME with no link recorded beside it.`
    - PC16: `  Scope: Codex's own default against the mock with no zygo between (K-D25); <inference>; under a zygo launch: P4-9 and P4-11; the notice text: LC-CX1 (owner-run).`, where `<inference>` is `release source inferred from a controlled count difference (one extra <host:port> CONNECT when the key is on), path not visible` (`<n> extra … CONNECTs` for n > 1), or `no controlled count difference (no release-source request seen)`.
- [x] 7. Stub dry run (no real binary; `command -v codex` names `$S/stub-bin/codex`): `sh -n scripts/premises/codex/resume.sh` and `shellcheck -s sh scripts/premises/codex/resume.sh` print nothing (the part carries `disable` lines, each with its reason: SC2034 `N2_AGENTS` and SC1091 the sourced `lib.sh`, which `base.sh` also draws, and SC2094 the pty as input and output). Then `N2_SCRATCH=$S/scr CODEX_FX=$S/fx PATH="$S/stub-bin:$PATH" sh scripts/premises/run-codex.sh resume`. Pass:
  - exit 0.
  - The last four lines are the `- PC10: failed <today>: fixtures/codex/workspace/summary.txt …` and `- PC16: failed <today>: fixtures/codex/update/summary.txt …` lines, each with its `  Scope:` line.
  - `$S/fx/runs/resume/isolation.txt` holds `home=scratch`, `codex_home=scratch`, `codex_exe=scratch`, `proxy=mock` and `run_versions=0.172.0,0.172.0,0.172.0,0.172.0,0.172.0,0.172.0` (six runs, round 2), `daemon=none` (PC-10).
  - `$S/fx/runs/resume/connect.txt` is empty.
  - `$S/fx/workspace/summary.txt` has `pc10=failed` and `session_start_fired_a=no`; `$S/fx/update/summary.txt` has `config_key=absent`, `schema_default=true`, `release_source_request=not-seen`, `pc16=failed`.
  - `$S/scr/codex-resume/c.raw` and `e.raw` each hold `stub` (the pty path works).
  - `$S/scr/codex-resume/home/stub-args.txt` is `--no-daemon --dangerously-bypass-hook-trust Reply ok. pc16-tracer-six` (run f is last, round 2).
  - `grep -rlF "$HOME" "$S/fx/workspace" "$S/fx/update"` and `grep -rliw "$(id -un)" "$S/fx/workspace" "$S/fx/update"` print nothing.
  - `pgrep -fl "$S/scr"` prints nothing.

  Then `rm -rf "$S"`. Do not go on unless every check passes.
- [x] 8. Real run (the lead, alone). Precondition: `pgrep -fl '\.codex/packages'` prints nothing, else stop and report (never kill). `stamp=$(mktemp)`; `sh scripts/premises/run-codex.sh resume` → exit 0. Then:
  - `find "$HOME/.codex" -newer "$stamp" -print` prints nothing.
  - `pgrep -fl codex-pkg`, `pgrep -fl '\.codex/packages'` and `pgrep -fl mock_upstream` print nothing.
  - `rm -f "$stamp"`.

  If the find lists a file or a process is left, stop and report it as a finding; do not filter the record. A refused loopback record in `connect.txt` is recorded (under "Findings") and reported, not a stop (lead's call 2026-11-25).
- [x] 9. Check the captures:
  - `fixtures/codex/workspace/summary.txt` and `fixtures/codex/update/summary.txt` hold every key of step 6.
  - `fixtures/codex/workspace/ids.txt` shows one placeholder for `rollout_id`, `rollout_meta_id` and `session_start_a` when `session_id_equals_rollout_id=yes`.
  - `fixtures/codex/runs/resume/isolation.txt`: `run_versions` five times the real version.
  - Every line of `connect.txt` ends ` refused`.
  - Record a failed PC as found (a measured `failed` is a result, not a defect of this node). A harness error (exit 1) is a defect: fix it and run steps 7 and 8 again.
- [x] 10. Replace `Written when the node runs.` under "## Observed" with the four lines that step 8 printed last, verbatim.
- [x] 11. Acceptance (the node's command, exactly as the contract's **Acceptance** line) → exit 0.
- [x] 12. Commit `scripts/premises/codex/resume.sh`, `fixtures/codex/workspace/`, `fixtures/codex/update/`, `fixtures/codex/runs/resume/` and this file with the message `feat(premises): Codex resume and update captures (PC-6)`, ending with `Zygo-Session: <session>` and `Zygo-Work: phase4:PC-6`, each on its own line. No Claude attribution.

### Author's checks (2026-11-25)

1. Spec coverage: PC10 `session_id` = rollout id → step 6 `session_id_equals_rollout_id`; `codex resume <id>` with a new `CODEX_HOME` linked to a shared store → runs b and c, `links_kept`, `tui_resume_in_store`; PC16 default in effect → `config_key=absent`, `schema_default=true`; release-source request with its `CONNECT` record → run e, `connect-e.txt`; K-D25 → no zygo, mock only; K§11 item 9 → step 6 prints only nontrivial paths with a date and result; item 10 → `scrub` and step 11 (`check-premises.sh`).
2. Step scan: each step names one file or one check. Only the pty answer rule and the ANSI strip rule carry a body, because the signatures do not decide them.
3. Interface match: `run_codex`, `codex_env` (through `CX_HOME` and its own `config.toml`) and `codex_record` as in `lib.sh` now. The Observed form is PC-7's.
4. Review focus 6 (`graph.md`, PC-3 to PC-6, each run): no real host and no read of `~/.codex`. Each of the six runs (five before round 2) goes through `run_codex` (one `cx-used.d` record each). Step 7 checks the stub record. Steps 8 and 9 check `isolation.txt` (`home=scratch`, `codex_home=scratch`, `proxy=mock`), that every `connect.txt` line ends ` refused` (the PC16 release-source request among them), `find ~/.codex -newer` and that no process is left. Step 11 runs the same checks through the Acceptance command.
5. Proportion: the plan gives signatures, keys and pass values, not the script.
6. Live code: every cited line was opened on `origin/phase-4` 5cb6a387. No existing helper gives a pseudo-terminal, a per-run record window or a rollout id: `tmux_claude` (`lib.sh:491`) is Claude-only and would bypass `run_codex`. `hook_log.py` (`hook_settings`, `lib.sh:524`) writes Claude hook logs; the part's one-line `hook.sh` is smaller than adapting it.
