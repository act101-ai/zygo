# PC-3 — Codex proxy-facing captures: PC1, PC2, PC7, PC13, PC14

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.3.7 rows PC1, PC2, PC7, PC13, PC14; K§6.4.1 (the paths PC1 shows), K§6.4.4 (PC14), K§6.9 (P4-4 needs the captured Codex requests); K§11 items 9 and 10 (for these fixtures)
**Files:** `scripts/premises/codex/proxy.sh` (new), `fixtures/codex/requests/` (new), `fixtures/codex/transport/` (new), `fixtures/codex/models/` (new), `fixtures/codex/errors/` (new), `fixtures/codex/runs/proxy/` (new), `fixtures/codex/runs/base/`, `fixtures/codex/help.txt` (lead's call, 2026-11-25: PC-2's base record predates PC-10 and lacks the TZ and `daemon=` lines; this node reruns `run-codex.sh base` once before its own capture)
**Consumes:** `run_codex` (PC-2), `codex_env` (PC-2), `codex_record` (PC-2)
**Produces:** `fixtures/codex/requests/` (captured Codex `POST /v1/responses` bodies and headers, scrubbed: a first turn; a turn after a `function_call_output`; a turn after a `custom_tool_call_output` of `apply_patch`; a turn whose `input` carries a `reasoning` item), `fixtures/codex/models/` (the `/models` request and the shape Codex accepts, PC7), `fixtures/codex/errors/` (the context-too-long shape Codex recovers from, PC14), `fixtures/codex/transport/` (paths called, processes after exit, PC1), and in this node file under "Observed" one line per id: `- PCn: <verified|failed> <YYYY-MM-DD>: <fixture paths>`
**Acceptance:** `sh scripts/premises/run-codex.sh proxy && N2_FORBIDDEN_NAMES="$(id -un)" sh scripts/check-premises.sh && test "$(find fixtures/codex/requests -type f -name '*.json' -size +2c | wc -l)" -ge 4 && grep -qx 'home=scratch' fixtures/codex/runs/proxy/isolation.txt && grep -qx 'codex_home=scratch' fixtures/codex/runs/proxy/isolation.txt && test -f fixtures/codex/runs/proxy/connect.txt && test -z "$(grep -v ' refused$' fixtures/codex/runs/proxy/connect.txt)" && ! grep -qx 'Written when the node runs.' docs/work/phase4/nodes/PC-3.md && test "$(grep -cE '^- (PC1|PC2|PC7|PC13|PC14): (verified|failed) [0-9]{4}-[0-9]{2}-[0-9]{2}: fixtures/codex/' docs/work/phase4/nodes/PC-3.md)" -eq 5 && grep -E '^- (PC1|PC2|PC7|PC13|PC14): ' docs/work/phase4/nodes/PC-3.md | cmp -s - fixtures/codex/runs/proxy/observed.txt` → every command exits 0: at least 4 captured requests; the run used scratch `HOME` and `CODEX_HOME`, and every `CONNECT` was refused; "Observed" has no placeholder and one dated result line for each of PC1, PC2, PC7, PC13, PC14, equal to `fixtures/codex/runs/proxy/observed.txt`

## Planning notes

- Mock replies are authored (not model output), as the existing method says; they go under this node's own fixture folders. Every mock and scrub change this node needs is a PC-2 deliverable; this node edits neither file.
- PC13 (the user-role items Codex injects) is read from the same captured requests; the redaction span rule that uses it is P4-9 (K§6.4.2).
- PC-7 writes the `docs/premises.md` rows from "Observed"; this node does not edit `docs/premises.md`.
- Acceptance placeholder clause anchored to the line (it matched its own text); lead's call 2026-11-25.
- Acceptance clause added in round 2 (lead's call): the Observed lines here equal `fixtures/codex/runs/proxy/observed.txt` (`grep … | cmp -s - …`, POSIX).

## Observed

- PC1: verified 2026-11-25: fixtures/codex/transport/result.txt fixtures/codex/transport/paths.txt fixtures/codex/transport/key-helper.txt fixtures/codex/transport/other-hosts.txt fixtures/codex/transport/processes.txt
- PC2: verified 2026-11-25: fixtures/codex/requests/http-headers.txt
- PC7: verified 2026-11-25: fixtures/codex/models/result.txt fixtures/codex/models/picker-b.jsonl fixtures/codex/models/picker-c.jsonl fixtures/codex/models/picker-d.jsonl fixtures/codex/models/catalog-shape.json fixtures/codex/models/openai-shape.json fixtures/codex/models/catalog-file.json fixtures/codex/models/request.json
- PC13: verified 2026-11-25: fixtures/codex/requests/injected-items.txt fixtures/codex/requests/first-turn.json
- PC14: verified 2026-11-25: fixtures/codex/errors/result.txt fixtures/codex/errors/context-400.json fixtures/codex/errors/context-failed.sse fixtures/codex/errors/e1-exec.txt fixtures/codex/errors/e2-exec.txt

## Review round 1 findings (PR #137, 2026-11-25)

Round 2 changes `scripts/premises/codex/proxy.sh`; where they differ from step 2 below, they replace it. Test of
F1-F6: a synthetic-record test (scratch, not committed) runs the analysis heredoc on authored records; on the
round 1 code it failed 15 of 28 checks, on the round 2 code it passes 28 of 28. F7, F8: stub runs.

- F1 (important): Codex offered no `apply_patch` tool (`gpt-mock` got the default model metadata), and the class
  rule took Codex's rejection `unsupported custom tool call: apply_patch` as the apply_patch class. Fix: the turn
  runs (`a`, `b`, `c`) get `model_catalog_json = $RUN/json/catalog-turns.json`, one entry `gpt-mock` with the
  required ModelInfo fields of step 2.2 and `apply_patch_tool_type = "freeform"` (ModelInfo field,
  `ApplyPatchToolType` snake_case `freeform`|`function`, `codex-rs/protocol/src/openai_models.rs`; not in the
  pinned config schema, and not checked offline: the installed program is under `~/.codex`, which this node never
  reads; the real run confirms it by `apply_patch=custom` in `transport/tools.txt`). Published as
  `models/catalog-turns.json`. Class `after-custom-tool-call-output` now needs a `custom` tool named `apply_patch`
  in the request's `tools` and a `custom_tool_call_output` (`call_pc3_2`) whose output is not an "unsupported
  custom tool call" error. If the real run still offers none, the class is missing (exit 1). Test: rejected output
  → class missing; no custom tool → class missing; good case → class found.
- F2 (important): PC13 checked only the AGENTS.md tracer. Fix: `injected-items.txt` lists every part of every
  item before the prompt (`<item>.<part> <type> <role> <part type> <first line, at most 120 characters>`), then
  `environment_context_in=<i.j|none>` and `agents_tracer_in=<i.j|none>`; PC13 is verified only when both are found.
  Test: both parts listed with their indexes; no `<environment_context>` → PC13 failed.
- F3 (important): PC7's line omitted the picker files; `result.txt` did not tell provider models from bundled ones;
  `models_requests` counted exec run `h`. Fix: the PC7 line cites `models/picker-b.jsonl`, `picker-c.jsonl`,
  `picker-d.jsonl`; `models_requests` counts the picker runs (`b`, `c`, `d`) only, `exec_models_requests=<n>` the
  rest; each variant line ends `served=<listed ids from that variant's authored source> bundled=<other listed ids>`;
  `models/request.json` is the first picker-run `/models` request. Test: counts 3 and 1; `b` served and bundled
  split; `c` bundled only.
- F5 (nit): Fix: each helper variant has its own `key-helper-<v>.sh`, which logs its variant; `helper_runs` counts
  run `h` only (`helper_runs_other=<n>` the rest); PC1 needs every run-`h` request with the bearer and every
  `CONNECT` line refused; `transport/result.txt` = `connect_lines=<n> refused=<n> hosts=<host:port,…>`, cited by
  PC1. The distinct token came with PC-10 (`MOCK_ALT_KEY` in `mock_upstream.py` and `start_mock`): the part
  builds `MOCK_ALT_KEY` at run time (a shell variable, never exported or written to the repo), each helper prints
  it, and PC1 needs every run-`h` request with `"credential": "alt"` (`with_helper_token` = `requests`). Test: no
  run-`h` helper → PC1 failed; run `h` with the primary key → PC1 failed; a CONNECT not refused → PC1 failed;
  counts and hosts line.
- F6 (nit): PC14 now needs both `e1` and `e2` with Codex's "context window" words and `requests=1` each. Test:
  `e2` without the words → PC14 failed.
- F7 (nit): a failed run deleted the committed fixtures first. Fix: the analysis files, the scrubbed copies and
  `codex_record`'s `runs/proxy/` are built in `$RUN/fx`; only when no class is missing are `requests`,
  `transport`, `models`, `errors` and `runs/proxy` moved over the fixture folders. A failed run prints
  `codex proxy: fixtures not replaced; this run's are in <RUN>/fx`, then the missing-classes line, and exits 1.
  Test: stub dry run, `CODEX_FX` stays empty.
- F8 (nit): Fix: `n2_trap_chain p3_rpc_kill` (kills the driver, the app-server job and this run's Codex copies);
  `app-server` runs as a background job the shell waits on for at most 150 s (then the job and the copies are
  killed, status `bound`; harness hygiene, no CI timing test); the job writes `exec-models-<v>.rc`, which is also
  the driver's `<done>`; the driver opens the FIFO with `O_NONBLOCK` (retried while no reader, at most 60 s, ends
  once `<done>` exists), so neither side blocks for ever. Tests: SIGTERM during a hanging `app-server` stub → no
  process left; a hanging `app-server` stub → each model run ends at the bound, no process left; the driver alone
  with no reader ends once `<done>` exists.
- Not this node: the fixtures of round 1 hold the host time zone and the user-agent OS text; PC-10 (`TZ=UTC` in
  `codex_env`, user-agent OS scrub, double-slash path scrub) merges before the real rerun.

### Merge of PC-10 (a1c0ce8a) into this branch

- `run_codex` now refuses (prints `premises: …` and exits) while another `codex app-server` runs or a daemon
  socket exists; that message went to the run's redirected stdout and the part ended with no word. Each run is
  now a subshell (`(run_codex …)`), and `p3_refused <stdout>` prints such a line and exits 1, then
  `check_abort`. `CODEX_VERSION` for the last line comes from the isolation record. Test: a stub `codex
  app-server` started outside the run folder → exit 1 with the `premises: a codex app-server runs outside this
  run folder …` line.
- `n2_daemon_flag`: every call here is `exec …` (also `exec … resume --last …`) or `app-server`; all pass.
- `codex_record`'s new scans: the FIFOs are skipped (not regular files); `codex_record` runs in the part's own
  shell; the driver and the app-server job are waited for before it.

### Round 2 steps

- [x] R1. Synthetic-record test written first; on the round 1 code: 15 of 28 checks fail (F1, F2, F3, F5, F6).
- [x] R2. Fixes in `proxy.sh`; `sh -n` and `shellcheck -s sh` exit 0; synthetic test 28 of 28.
- [x] R3. Stub dry run (`stub-bin` first on PATH, `command -v codex` = the stub): exit 1, last line
  `codex proxy: missing request classes: first-turn after-function-call-output after-custom-tool-call-output
  turn-with-reasoning`; `CODEX_FX` has no file; `<scratch>/codex-proxy/fx/runs/proxy/isolation.txt` as step 4;
  `connect.txt` empty; `stub-args.txt` = `app-server`; `observed.txt` five `failed` lines citing the picker files
  and `transport/result.txt`; no process holds the scratch folder; `git status --porcelain fixtures` empty.
- [x] R4. F8 stub tests: trap (SIGTERM during a hanging `app-server` stub): part exit 143, the mock, the driver and the Codex copy gone; bound (hanging `app-server` stub): `b`, `c`, `d` each `exec-models-<v>.rc` = `bound`, driver 1, `processes.txt` none, no process left, `CODEX_FX` empty; the driver alone with no reader ends (1) once `<done>` exists.
- [x] R5. Real `run-codex.sh base` (2026-11-25: exit 0; `isolation.txt` gains `TZ`, `daemon=none`; `help.txt` and the schema unchanged), then real rerun (2026-11-25: exit 0; `apply_patch=custom`, Codex applied the authored patch; PC13 `environment_context_in=2.1`, `agents_tracer_in=2.0`; all five verified) (the lead releases it, after PC-10): steps 5-7 checks, and `pgrep -fl 'app-server'` empty before; `daemon=none` and `TZ` in `env_names`; no `America/`, `Mac OS`, `Los_Angeles` in the fixtures; exit 0; `transport/tools.txt`
  `apply_patch=custom`; the four classes found; five dated lines.
- [x] R6. (2026-11-25: Acceptance exit 0, its real run under the same checks) "Observed" replaced with the new `observed.txt` lines, verbatim; Acceptance exit 0; commit, push (PR #137).

## Plan

Base: `origin/phase-4` at 5cb6a387 (PC-1, PC-2, OC-1 merged). Branch `phase-4--PC-3`. Premise checked 2026-11-25 against the live code:

- `scripts/premises/run-codex.sh:9-19`: `run-codex.sh proxy` runs `sh scripts/premises/codex/proxy.sh` with `N2_SCRATCH` exported; a new part needs no edit there. Part header as `scripts/premises/codex/base.sh:9-11`.
- `scripts/premises/lib.sh`: `new_run` `:82-101` (sets `RUN`, `H`, `R`, `CX_HOME=$H/.codex`), `start_mock [rules.json]` `:105-117`, `stop_mock` `:153-158`, `scrub <file>` `:196-198`, `codex_env` `:332-366`, `run_codex` `:370-380`, `codex_record <part>` `:400-476`, `publish_sse` `:539-543`, `EXTRA_ENV_ALLOW="RUST_LOG RUST_BACKTRACE"` `:216`. `codex_env` writes `$CX_HOME/config.toml` only when it is missing and refuses one whose single `base_url = "…"` line is not `$MOCK_URL/v1` (`:345-355`), so a part may write its own provider config after `start_mock`. `codex_record` replaces `$CODEX_FX/runs/<part>/` whole (`:403-406`). There is no `CODEX_EXTRA_ALLOW` anywhere in `scripts/`; the allow-list is `EXTRA_ENV_ALLOW`.
- `scripts/premises/mock_upstream.py`: rules with `path` match `POST …/v1/responses` and `GET …/v1/models` (`:113-121`, `:166-171`); `header` and `body` keys (`:172-188`); `last` reads `body.messages` only (`:183-186`), so it never matches a Responses body: this plan uses `body` and `header`; `times` sequences (`:324-333`); the reply file is opened at request time (`:343`), so a part may write a reply file after `start_mock` and before the request; `json` + `status` (`:345-348`); default Responses SSE (`:356-358`); no `/v1/models` rule → `404` (`:359-361`); `CONNECT` and absolute-URI lines in `<rec>/connect.txt` (`:205-273`); bodies are matched against `json.dumps(body)` (default separators: `"type": "function_call_output"`).
- `scripts/premises/scrub.py:157-170`: a record's `body.system` and each tool `description` → `[omitted]`; `instructions` and `input` are kept; ids → numbered UUIDs; `authorization` → `[omitted]`.
- `scripts/check-premises.sh:161-165`: an evidence path must be a regular file of more than 2 bytes, not `{}`/`[]`; `:206-246` forbidden-content scan over `fixtures/`.
- `fixtures/codex/config.schema.json` (pinned 0.172.0): provider `auth` = `ModelProviderAuthInfo` (`:3220`, a command printing the bearer: this is K§6.3.7 PC1's "key-helper"), `env_http_headers` (`:3354`, header name → environment variable; unset or empty → no header), `http_headers` (`:3381`), `request_max_retries`, `stream_max_retries`, `model_catalog_json` (`:1570`), feature `apply_patch_freeform` (`:846`).
- `fixtures/codex/help.txt`: `codex exec` has `--json`, `-s workspace-write`, `--enable`, `exec resume --last [PROMPT]`; top-level `app-server`. There is no picker outside the TUI; this plan reads picker content from `codex app-server`'s `model/list` (the list the picker shows; author's call, K§6.3.7 PC7).
- Consumers: `RS-1.md:7` translates **each** `fixtures/codex/requests/*.json` body, so only the four turn records are `*.json` in `requests/`; every other file there is `.txt`. `RS-3` reads `errors/`, `RS-8` reads `models/` and `transport/`. `PC-7.md` reads the "Observed" lines; each path in them must pass `nontrivial`.
- No `EXTRA_ENV` name is needed: `env_http_headers` is measured with `TERM`, which `codex_env` already sets to `xterm-256color` (`lib.sh:356`). `lib.sh` is not in Files and is not changed.

Safety (operator's rules for real Codex runs):
- Codex runs only through `run_codex` in this part, under `run-codex.sh proxy`. Never run `codex login`; no real token; no network but `127.0.0.1`. Never read, list or copy anything in `~/.codex` (the harness copies only the program package).
- Steps 1-4 use a stub program only. The real run (steps 5-7) is executed by the lead, one node's real run at a time (PC-3 to PC-6 never run real Codex together).
- No CI test is added; nothing here asserts wall-clock time in CI. The part's own waits (FIFO driver) are premise-script waits, run by hand.

### Interfaces

- Consumes (exact, `scripts/premises/lib.sh` at 5cb6a387):
  - `new_run <name>` → `RUN`, `H`, `R`, `CX_HOME=$H/.codex`.
  - `start_mock [rules.json]` → `MOCK_URL`, `MOCK_PID`; `stop_mock`; `scrub <file>` (stdout); `publish_sse <dest dir>` (scrubbed copies of `$RUN/sse/*.sse`).
  - `codex_env` (PC-2): needs `new_run`, `start_mock`; sets `CODEX_EXE`, `CODEX_VERSION`, `CODEX_ENV`; writes `$CX_HOME/config.toml` only when missing.
  - `run_codex <codex args>` (PC-2): `codex_env`, a record in `$RUN/cx-used.d/`, then `(cd "$R" && $CODEX_ENV $EXTRA_ENV "$CODEX_EXE" "$@")`; returns Codex's status.
  - `codex_record <part>` (PC-2): after `stop_mock`; writes `$CODEX_FX/runs/<part>/isolation.txt` and `connect.txt`; exit 1 on any isolation failure.
  - `CODEX_FX` (default `$REPO_DIR/fixtures/codex`), `CODEX_VERSION`, `DUMMY_KEY`, `MOCK_URL`.
- Produces (all under `$CODEX_FX`, scrubbed; written whole on each run):
  - `requests/first-turn.json`, `requests/after-function-call-output.json`, `requests/after-custom-tool-call-output.json`, `requests/turn-with-reasoning.json` (mock records: `method`, `path`, `headers`, `body`), `requests/injected-items.txt` (PC13), `requests/http-headers.txt` (PC2).
  - `transport/result.txt` (round 2), `transport/paths.txt`, `transport/key-helper.txt`, `transport/other-hosts.txt`, `transport/processes.txt`, `transport/tools.txt`, `transport/replies/*.sse` (the authored turn replies) (PC1).
  - `models/request.json` (when Codex sent one), `models/catalog-shape.json`, `models/openai-shape.json`, `models/catalog-file.json`, `models/catalog-turns.json` (round 2), `models/picker-b.jsonl`, `models/picker-c.jsonl`, `models/picker-d.jsonl`, `models/result.txt` (PC7).
  - `errors/context-400.json`, `errors/context-failed.sse`, `errors/e1-exec.txt`, `errors/e2-exec.txt`, `errors/result.txt` (PC14).
  - `runs/proxy/isolation.txt`, `runs/proxy/connect.txt` (`codex_record`), `runs/proxy/observed.txt` (the five "Observed" lines).

### Steps

- [x] 1. Check the base: `git merge-base --is-ancestor 5cb6a387 HEAD` → exit 0.
- [x] 2. Write `scripts/premises/codex/proxy.sh` (new; POSIX `sh`; part header as `base.sh:9-11`; a header comment naming PC1, PC2, PC7, PC13, PC14 and the fixtures). In this order:
  1. `new_run codex-proxy`. Write `$R/AGENTS.md` = `pc3-agents-tracer: project instructions for the PC13 capture.`
  2. Authored files (values fixed here; nothing is model output). In `$RUN/json/`: `context-400.json` = `{"error":{"message":"pc3 input is over the limit","type":"invalid_request_error","param":"input","code":"context_length_exceeded"}}`; `catalog-shape.json` = `{"models":[M("gpt-mock"),M("gpt-mock-listed")]}`; `catalog-file.json` = `{"models":[M("gpt-mock"),M("gpt-mock-file")]}`, where `M(s)` = `{"slug":s,"display_name":"PC3 "+s,"description":"authored for PC-3","supported_reasoning_levels":[{"effort":"low","description":"low"}],"default_reasoning_level":"low","shell_type":"shell_command","visibility":"list","supported_in_api":true,"priority":1,"support_verbosity":false,"truncation_policy":{"mode":"bytes","limit":10000},"experimental_supported_tools":[],"base_instructions":"PC3 base instructions."}` (the required fields of `ModelInfo` at the `rust-v0.172.0` tag, `codex-rs/protocol/src/openai_models.rs`); `openai-shape.json` = `{"object":"list","data":[{"id":"gpt-mock-listed","object":"model","created":0,"owned_by":"pc3"}]}`. In `$RUN/sse/`: `context-failed.sse` = events `response.created` (`{"type":"response.created","response":{"id":"resp_pc3_e2","object":"response","model":"{{MODEL}}","status":"in_progress","output":[]}}`) then `response.failed` (`{"type":"response.failed","response":{"id":"resp_pc3_e2","object":"response","model":"{{MODEL}}","status":"failed","output":[],"error":{"code":"context_length_exceeded","message":"pc3 input is over the limit"}}}`). The authored texts never say "context window".
  3. `$RUN/rules.json` (first match wins): `e1` `{"path":"/v1/responses","body":"pc3-turn-e1","times":1,"status":400,"json":"json/context-400.json"}`; `e2` `{"path":"/v1/responses","body":"pc3-turn-e2","times":1,"sse":"sse/context-failed.sse"}`; `final` `{"path":"/v1/responses","body":"\"type\": \"custom_tool_call_output\"","sse":"sse/turn-final.sse"}`; `patch` `{"path":"/v1/responses","body":"\"type\": \"function_call_output\"","sse":"sse/turn-patch.sse"}`; `fc` `{"path":"/v1/responses","body":"pc3-turn-b","sse":"sse/turn-fc.sse"}`; `models-b` `{"path":"/v1/models","header":{"x-pc3-variant":"^b$"},"json":"json/catalog-shape.json"}`; `models-c` the same with `^c$` and `json/openai-shape.json`. `start_mock "$RUN/rules.json"`.
  4. `cx_config <variant> <dir> [helper|catalog]`: writes `<dir>/config.toml` whole (mkdir first): `model = "gpt-mock"`, `model_provider = "mock"`, with `catalog` also `model_catalog_json = "$RUN/json/catalog-file.json"`; table `[model_providers.mock]`: `name = "mock"`, `base_url = "$MOCK_URL/v1"`, `wire_api = "responses"`, `request_max_retries = 0`, `stream_max_retries = 0`, `http_headers = { "X-Pc3-Static" = "pc3-static-value", "X-Pc3-Variant" = "<variant>" }`, `env_http_headers = { "X-Pc3-Env" = "TERM", "X-Pc3-Unset" = "PC3_UNSET_VAR" }`, and `env_key = "MOCK_API_KEY"`; with `helper`, no `env_key` and a table `[model_providers.mock.auth]` `command = "$RUN/key-helper.sh"`. `$RUN/key-helper.sh` (mode 755): `#!/bin/sh`, appends one line to `$RUN/key-helper.ran`, prints `$DUMMY_KEY` (value written at run time).
  5. `ps_note <label>`: appends to `$RUN/processes.txt` the line `<label>:` then every line of `ps -A -o pid=,ppid=,command=` that holds `$RUN/codex-pkg` or `pc3-tracer` (none → `<label>: none`).
  6. Runs, each with `</dev/null`, stdout and stderr to `$RUN/exec-<x>.out` / `.err`, status kept (`|| rc=$?`, never fatal), then `ps_note <x>`. Common turn flags `T` = `exec --json -s workspace-write --enable apply_patch_freeform`:
     - `a` (probe): `cx_config t "$H/.codex"`; `run_codex $T pc3-turn-a`.
     - Replies from the probe (python3 -I heredoc): from the first record whose body holds `pc3-turn-a`, the shell tool is the first `type: function` tool named `exec_command`, `shell_command` or `shell` (arguments `{"cmd":"echo pc3-tracer"}`, `{"command":"echo pc3-tracer"}`, `{"command":["echo","pc3-tracer"]}`); none (or no record) → name `pc3-no-shell-tool`, arguments `{}`. Write `$RUN/transport/tools.txt` (`tools=<type:name,…>`, `shell_tool=<name|none>`, `apply_patch=<custom|function|none>`) and `$RUN/sse/turn-fc.sse` (`response.created`; `response.output_item.done` with `{"type":"function_call","id":"fc_pc3_1","call_id":"call_pc3_1","name":<tool>,"arguments":<JSON text>,"status":"completed"}`; `response.completed` with that item and usage `{"input_tokens":10,"output_tokens":1,"total_tokens":11}`), `turn-patch.sse` (items: reasoning `{"type":"reasoning","id":"rs_pc3_1","summary":[{"type":"summary_text","text":"pc3 reasoning one"}],"encrypted_content":"pc3-opaque-reasoning-1"}`, then `{"type":"custom_tool_call","id":"ctc_pc3_1","call_id":"call_pc3_2","name":"apply_patch","input":"*** Begin Patch\n*** Add File: pc3.txt\n+pc3 patch line\n*** End Patch\n","status":"completed"}`), `turn-final.sse` (reasoning `rs_pc3_2` / `pc3-opaque-reasoning-2`, then message `{"type":"message","id":"msg_pc3_final","role":"assistant","status":"completed","content":[{"type":"output_text","text":"pc3 done","annotations":[]}]}`). Each event as `event: <type>\ndata: <json>\n\n`, `output_index` per item, `{{MODEL}}` for the model.
     - `b` (turns): `run_codex $T pc3-turn-b`. `c` (resume): `run_codex $T resume --last pc3-turn-c`.
     - `h` (key helper): `CX_HOME=$RUN/cx-helper`; `cx_config h "$CX_HOME" helper`; `run_codex exec --json pc3-turn-h`.
     - `e1`, `e2` (context error): `CX_HOME=$RUN/cx-<e>`; `cx_config <e> "$CX_HOME"`; `run_codex exec --json pc3-turn-<e>`.
     - `b`, `c`, `d` model lists: `CX_HOME=$RUN/cx-models-<v>`; `cx_config <v> "$CX_HOME"` (`b`, `c`: `helper`; `d`: `catalog`; changed after the first real run: only the provider with an auth command sent `GET /v1/models`, so with `env_key` the `models-b` and `models-c` rules could never answer); `mkfifo $RUN/rpc-<v>.in`; start the driver `python3 -I $RUN/rpc.py $RUN/rpc-<v>.in $RUN/rpc-<v>.out $RUN/rpc-<v>.done &` (pid kept); `run_codex app-server <$RUN/rpc-<v>.in >$RUN/rpc-<v>.out 2>$RUN/rpc-<v>.err`; then `: >$RUN/rpc-<v>.done`; `wait <driver pid>` (never a bare `wait`; its status goes to `$RUN/rpc-<v>.status`). `rpc.py <fifo> <out> <done>` (heredoc): opens the FIFO for writing; writes `{"id":1,"method":"initialize","params":{"clientInfo":{"name":"zygo-premise","title":"zygo premise","version":"0.0.0"}}}`; polls `<out>` (0.2 s, at most 60 s) for a JSON line whose top-level `id` is 1 (a nested id never counts), and stops early with 1 after one last read once `<done>` exists (the server has exited: no answer can come); writes `{"method":"initialized"}` and `{"id":2,"method":"model/list","params":{}}`; polls for id 2 the same way; closes the FIFO (EOF ends the server); exit 0 when id 2 came, else 1; a `BrokenPipeError` ends it with 1.
  7. `stop_mock`; `codex_record proxy`.
  8. `rm -rf "$CODEX_FX/requests" "$CODEX_FX/transport" "$CODEX_FX/models" "$CODEX_FX/errors"`. Analysis (python3 -I heredoc; args `$RUN`, `$RUN/out`, today's date) writes the files of Produces under `$RUN/out/` from `$RUN/rec/*.json`, `rec/connect.txt`, the `exec-*` outputs, `rpc-*.out`, `processes.txt`, `key-helper.ran`, `transport/tools.txt`; then each file is copied with `scrub` to the same relative path under `$CODEX_FX` (`publish_sse "$CODEX_FX/transport/replies"` for the turn replies, then `transport/replies/context-failed.sse` removed, since `publish_sse` copies every `$RUN/sse/*.sse`; `errors/context-failed.sse` by `scrub`). Rules (each record's variant is its `x-pc3-variant` header):
     - Request classes (POST `/v1/responses`, first by `seq`): `first-turn` = a user message holding `pc3-turn-b` and no `function_call_output` item; `after-function-call-output` = an item `function_call_output` with `call_id` `call_pc3_1` and no `custom_tool_call_output`; `after-custom-tool-call-output` = an item `custom_tool_call_output` with `call_id` `call_pc3_2` and no message holding `pc3-turn-c`; `turn-with-reasoning` = a user message holding `pc3-turn-c` and an item of type `reasoning`.
     - `requests/injected-items.txt`: for each `input` item of `first-turn` before the item holding `pc3-turn-b`: `<index> <type> <role> <first line, at most 120 characters>` (the run paths get scrub's names `/repo`, `/home/user`, `/tmp/run` before the cut: a cut path escapes scrub's rule, as the second real run showed); then `agents_tracer_in=<index|none>`.
     - `requests/http-headers.txt`: one line per record `<seq> <method> <path> variant=<v> static=<value|absent> env=<value|absent> unset=<present|absent>`; then `requests=<n> static_ok=<n> env_ok=<n> unset_absent=<n>` (ok = `pc3-static-value`, `xterm-256color`).
     - `transport/paths.txt`: `<count> <METHOD> <path without query> <sorted query keys or ->` per distinct triple. `transport/other-hosts.txt`: `<count> <line>` per distinct `connect.txt` line, or `none`. `transport/processes.txt`: `$RUN/processes.txt`. `transport/key-helper.txt`: `helper_runs=<lines of key-helper.ran>`, `requests=<n of variant h>`, `with_bearer=<n of variant h whose credential_headers is ["authorization"] and reply is not 401-no-credential>`, `no_credential=<n of all records with reply 401-no-credential>`.
     - `models/request.json`: the first GET record whose path holds `/models` (absent when none). `models/picker-<v>.jsonl`: `rpc-<v>.out`. `models/result.txt`: `models_requests=<n> <distinct paths with query>`, then per `b`, `c`, `d`: `<v>: models_request=<n of variant v> listed=<yes|no> driver=<status>` (`listed` = a line with id 2 and a `result` whose text holds `gpt-mock-listed`, for `d` `gpt-mock-file`). The authored model files are copied as written.
     - `errors/e<n>-exec.txt`: `exec-e<n>.out` then `exec-e<n>.err`. `errors/result.txt` per variant: `e<n>: exit=<rc> codex_words=<yes|no> requests=<n of variant e<n>>` (`codex_words` = the exec output holds `context window`, any case: Codex's own words, since the authored texts never say it).
     - `runs/proxy/observed.txt`, five lines `- PCn: <verified|failed> <date>: <paths>` with the Produces paths per id: PC1 `transport/paths.txt transport/key-helper.txt transport/other-hosts.txt transport/processes.txt` — verified when `helper_runs` ≥ 1, `requests` ≥ 1, `with_bearer` = `requests`, `no_credential` = 0; PC2 `requests/http-headers.txt` — verified when `requests` > 0 and the three ok counts equal it; PC7 `models/result.txt models/catalog-shape.json models/openai-shape.json models/catalog-file.json` (+ `models/request.json` when written) — verified when `models_requests` ≥ 1, `b` or `c` is `listed=yes`, and `d` is `listed=yes`; PC13 `requests/injected-items.txt requests/first-turn.json` — verified when at least one item precedes the prompt and `agents_tracer_in` is an index; PC14 `errors/result.txt errors/context-400.json errors/context-failed.sse errors/e1-exec.txt errors/e2-exec.txt` — verified when a variant has `codex_words=yes`. Each path is written as `fixtures/codex/<path>`.
     - Exit 1 with `codex proxy: missing request classes: <names>` when a class has no record (after every file is written); else the part prints `codex proxy: ok (codex $CODEX_VERSION)`.
- [x] 3. `sh -n scripts/premises/codex/proxy.sh` → exit 0; `shellcheck -s sh scripts/premises/codex/proxy.sh` → exit 0 (directives only with a reason, as in `base.sh:19`: SC2034 on `N2_AGENTS=codex`, which `lib.sh` reads, and SC1091 on the `lib.sh` source line; `base.sh` has the same two findings).
- [x] 4. Stub dry run (no real binary). `S=$(mktemp -d)`; build `S/bin/codex` from the stub source of `scripts/premises/lib_test.sh:26-46` with `cc -DVER='"0.172.0"'`; `mkdir -p S/fx`; run `N2_SCRATCH=$S/scr CODEX_FX=$S/fx PATH="$S/bin:$PATH" sh scripts/premises/run-codex.sh proxy`. Pass: exit 1 and the last line is `codex proxy: missing request classes: first-turn after-function-call-output after-custom-tool-call-output turn-with-reasoning` (the stub sends no request); `S/fx/runs/proxy/isolation.txt` holds `home=scratch`, `codex_home=scratch`, `codex_version=0.172.0`, `run_versions=` nine times `0.172.0`, `codex_exe=scratch`, `env_names=ALL_PROXY CODEX_HOME HOME HTTPS_PROXY HTTP_PROXY MOCK_API_KEY PATH TERM TMPDIR XDG_CACHE_HOME XDG_CONFIG_HOME XDG_DATA_HOME XDG_STATE_HOME`, `proxy=mock`; `S/fx/runs/proxy/connect.txt` is empty; `S/scr/codex-proxy/home/stub-args.txt` is `app-server`; `S/fx/runs/proxy/observed.txt` has five lines, each `failed`; `ps -A -o command=` shows no line holding `$S`; `git status --porcelain fixtures` prints nothing. Then `rm -rf "$S"`. Do not go on unless every check passes.
- [x] 5. Real run, precondition (the lead runs steps 5-7): `pgrep -fl '\.codex/packages'` prints nothing (else stop and report; never kill). `stamp=$(mktemp)`.
- [x] 6. `sh scripts/premises/run-codex.sh proxy` → exit 0, last line `codex proxy: ok (codex <x.y.z>)`, `x.y.z` ≥ 0.172.0. A non-zero exit: stop and report the output and the kept scratch folder; do not filter a record.
- [x] 7. After the run: `find "$HOME/.codex" -newer "$stamp" -print` prints nothing; `pgrep -fl 'codex-pkg|mock_upstream|\.codex/packages'` prints nothing; `rm -f "$stamp"`. `fixtures/codex/runs/proxy/isolation.txt` has `home=scratch`, `codex_home=scratch`, `codex_exe=scratch`, `proxy=mock`; every line of `connect.txt` ends in ` refused` (review focus 6).
- [x] 8. Read the captures: `find fixtures/codex/requests -name '*.json' | wc -l` is 4; each holds `"method": "POST"` and a `body.input`; `transport/tools.txt` names the shell tool and `apply_patch=custom` (else report it: the turn replies then name a tool Codex lacks); `runs/proxy/observed.txt` has five lines.
- [x] 9. In this file, replace `Written when the node runs.` under "Observed" with the five lines of `fixtures/codex/runs/proxy/observed.txt`, verbatim.
- [x] 10. Acceptance (the node's command, exactly as in **Acceptance**) → every command exits 0. First run 2026-11-25: exit 1, only on the unanchored placeholder clause (it matched its own text); after the lead's change to `! grep -qx 'Written when the node runs.'`, rerun 2026-11-25: exit 0.
- [x] 11. Commit `scripts/premises/codex/proxy.sh`, `fixtures/codex/requests/`, `fixtures/codex/transport/`, `fixtures/codex/models/`, `fixtures/codex/errors/`, `fixtures/codex/runs/proxy/` and this file: `feat(premises): Codex proxy-facing captures (PC-3)`, trailers `Zygo-Session: <session>` and `Zygo-Work: phase4:PC-3`, no attribution. Push `phase-4--PC-3`.

### Author's checks (2026-11-25)

1. Spec coverage: K§6.3.7 PC1 (base_url routing, key-helper bearer, paths, other hosts, processes) → steps 2.6 `h`, 2.8 transport; PC2 → `cx_config` headers, `http-headers.txt`; PC7 → model variants `b`, `c`, `d`, `models/`; PC13 → `AGENTS.md` tracer, `injected-items.txt`; PC14 → `e1`, `e2`, `errors/`; K§6.4.1 paths → `paths.txt`; K§6.9 requests → the four classes; K§11 item 9 → step 2.8 paths, step 10; item 10 → `scrub` on every fixture, `check-premises.sh` in step 10.
2. Step scan: each step names one file or one command with its pass result; the analysis rules give the classification and verdict rules, which the signature does not decide.
3. Interface match: `run_codex`, `codex_env`, `codex_record` as `lib.sh:332-476`; RS-1 reads only the four `requests/*.json` records; RS-3 `errors/`; RS-8 `models/`, `transport/`; PC-7 reads the "Observed" lines in the contract's form, paths unquoted after `: `.
4. Review focus 6 → steps 4 and 7 (isolation lines, refused `CONNECT` only, no write to `~/.codex`). Focus 10 (PC14 shape) is RS-3's test; this node gives `errors/result.txt`.
5. Proportion: the plan gives values, rules and pass results; no script body is written here.
6. Live code: every cited path, line and name opened at 5cb6a387. No existing helper classifies Responses records or drives `app-server`; `publish_sse` and `scrub` are reused.
