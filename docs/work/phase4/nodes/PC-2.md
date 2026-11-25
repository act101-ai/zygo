# PC-2 — Codex premise harness: mock and scrub changes for every Codex part, codex-only `lib.sh` with an isolation record, `run-codex.sh`, help and schema captures

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.3.7 (method paragraph; the schema and help captures), K§4 K-C8 (the pin is a minimum), K§9 (P4-3), K§11 item 10; K-D14, K-D25; `docs/premises.md:3` (a pinned version is a minimum) and "Method"
**Files:** `scripts/premises/mock_upstream.py`, `scripts/premises/mock_upstream_test.py` (new), `scripts/premises/lib.sh`, `scripts/premises/lib_test.sh`, `scripts/premises/scrub.py`, `scripts/premises/scrub_test.py`, `scripts/premises/run-codex.sh` (new), `scripts/premises/codex/base.sh` (new), `scripts/premises/run-all.sh`, `fixtures/codex/help.txt` (new), `fixtures/codex/config.schema.json` (new), `fixtures/codex/config.schema.source.txt` (new), `fixtures/codex/runs/base/` (new)
**Consumes:** `scripts/check-premises.sh` (PC-1)
**Produces:** `run-codex.sh [<part>]` (runs `scripts/premises/codex/<part>.sh`, or every `codex/*.sh` in name order; a part needs no edit of `run-codex.sh`), `codex_env` and `run_codex` in `lib.sh` (env -i, scratch `HOME` and `CODEX_HOME` under the run folder, dummy key built at run time, `HTTPS_PROXY`, `HTTP_PROXY` and `ALL_PROXY` at the mock, provider `base_url` at the mock; asserts both folders are under the run folder before `codex` starts), `codex_record <part>` in `lib.sh` (writes `fixtures/codex/runs/<part>/isolation.txt` with the lines `home=scratch`, `codex_home=scratch`, `codex_version=<x.y.z>`, and `fixtures/codex/runs/<part>/connect.txt` with one line `<host>:<port> refused` per `CONNECT` and one line `<host>:<port> <METHOD> refused` per absolute-URI request), the mock deliverables listed below, the scrub deliverables listed below, `fixtures/codex/help.txt`, `fixtures/codex/config.schema.json`
**Acceptance:** `python3 -I scripts/premises/mock_upstream_test.py && python3 -I scripts/premises/scrub_test.py && sh scripts/premises/lib_test.sh && sh scripts/premises/run-codex.sh base && N2_FORBIDDEN_NAMES="$(id -un)" sh scripts/check-premises.sh && test "$(wc -c < fixtures/codex/help.txt)" -gt 2 && test "$(wc -c < fixtures/codex/config.schema.json)" -gt 2 && grep -qx 'home=scratch' fixtures/codex/runs/base/isolation.txt && grep -qx 'codex_home=scratch' fixtures/codex/runs/base/isolation.txt && grep -Eqx 'codex_version=[0-9]+\.[0-9]+\.[0-9]+' fixtures/codex/runs/base/isolation.txt && test -f fixtures/codex/runs/base/connect.txt && test -z "$(grep -vE '^(127\.0\.0\.1|localhost|\[::1\]):[0-9]+ ' fixtures/codex/runs/base/connect.txt)"` → every command exits 0: the mock, scrub and lib tests pass with one case per deliverable below; the base run used scratch `HOME` and `CODEX_HOME`, recorded the Codex version it ran, and its `CONNECT` record holds no non-loopback host

## Deliverables for the parallel capture nodes (PC-3 to PC-6 edit none of these files)

`mock_upstream.py`, each with a case in `mock_upstream_test.py`:

1. `POST /v1/responses` rule answers: an SSE file, a JSON body, or a status with a body (PC1, PC2, PC13, PC14 context-too-long error).
2. A rule sequence: the n-th request that matches a rule gets the n-th reply (multi-turn and tool-call turns, PC1; the turn sequence before and after `codex resume`, PC10).
3. A `response.completed` usage set by the rule, so a run can raise `usage` (PC8 compaction) and see the compaction request.
4. `GET /v1/models` answer from a rule file (PC7).
5. `CONNECT` handling: each `CONNECT` is refused and recorded as `<host>:<port> refused` (PC1 other hosts; PC16 update request). A plain-HTTP proxied request (absolute-URI request line) is refused and recorded as `<host>:<port> <METHOD> refused`.

`scrub.py`, each with a case in `scrub_test.py`: the Codex rollout file name and rollout id form, the Codex session and turn id forms, and the `CODEX_HOME` paths (run paths already map to `/tmp/run`), if the existing UUID and path rules do not cover them.

`lib.sh`, with cases in `lib_test.sh`: `codex_env`, `run_codex`, `codex_record`; a runner states the agents it needs, so a Codex-only run needs only `codex` on `PATH` (`lib.sh:11-12` require `claude` and `opencode` today).

## Planning notes

- The pinned version is a minimum (K-C8; `docs/premises.md:3`): the harness checks `codex --version` ≥ 0.172.0 with `ver_ge` (`scripts/premises/ver.sh`) and records the version it ran; it does not refuse a later version. `codex-cli 0.172.0` is on the lead's machine (`codex --version`, 2026-11-25).
- The schema is the pinned schema of the `rust-v0.172.0` tag (K§6.3.7), fetched once from the public repository and kept verbatim with its source URL beside it (the `fixtures/claude-code/settings-schema.source.txt` precedent). This fetch is the only network use of the node, and it carries no credential. PC-1 owns its scan exemption.
- `run-all.sh` gains the Codex runner, so it still "runs every experiment" (`docs/premises.md` "Method").

## Plan

Base: `origin/phase-4--PC-1` at 4c0de3f9 (PC-1 round 2 may still change `scripts/check-premises.sh`; this plan uses only its `N2_FORBIDDEN_NAMES` input and its `VERBATIM` entry, `scripts/check-premises.sh:139`). Premise checked 2026-11-25 against the live code:

- `scripts/premises/lib.sh:11-12` require `claude` and `opencode`; `:45` `DUMMY_KEY`; `:46` `TOOL_PATH` uses both binaries; `new_run` `:49`, `start_mock` `:70`, `check_abort` `:87`, `EXTRA_ENV` `:106`, `scrub` `:123`.
- `scripts/premises/mock_upstream.py`: rules are tried only for `POST …/v1/messages` (`:148`); any other method gets 404 (`:167`); `main()` requires `r["sse"]` in every rule (`:209-210`); there is no `CONNECT` handler (the base class answers 501). The existing `times` key already gives a rule sequence (first rule with uses left wins), so deliverable 2 needs a test, not code.
- `scripts/premises/scrub.py`: the existing UUID rule and run-path rules already cover the Codex rollout file name, session and turn ids (UUIDv7, lowercase) and `CODEX_HOME` paths. Checked by running `scrub.py` on samples: `<run>/home/.codex/sessions/2026/11/25/rollout-2026-11-25T10-20-30-<uuid>.jsonl` → `/home/user/.codex/sessions/2026/11/25/rollout-2026-11-25T10-20-30-00000000-0000-4000-8000-000000000001.jsonl`. So `scrub.py` gets test cases and no code change.
- `scripts/premises/run-all.sh:13` lists the runners; `scripts/premises/lib_test.sh:296` copies that list and `:313`, `:321`, `:330` expect 12 runs; `rall` (`:293-299`) copies `run-all.sh` and `lib.sh` only.
- Schema source exists: `https://raw.githubusercontent.com/openai/codex/rust-v0.172.0/codex-rs/core/config.schema.json` (HTTP 200, 284300 bytes, valid JSON, sha256 `52fb96b1ff79a51bf9ce31695a4a22d4e80f57591d91bad5747b657f4e612bac`, fetched with `curl -q` under `env -i`, 2026-11-25). Put in `fixtures/codex/` of a scratch copy, it passes `N2_FORBIDDEN_NAMES="$(id -un)" sh scripts/check-premises.sh`.
- **Drift found (isolation):** on the lead's machine `codex` on `PATH` is `~/.local/bin/codex`, a link to `~/.codex/packages/standalone/current/bin/codex` (a native binary with sibling folders `codex-resources`, `codex-path` and `codex-package.json`). The binary lives inside the owner's `~/.codex`. Scratch `HOME` and `CODEX_HOME` alone leave its executable path, and a `PATH` entry, inside `~/.codex`. The plan adds one rule: when the resolved binary is under the owner's `~/.codex`, the harness copies its package folder into the scratch folder and runs the copy (step 9, case X8).

Safety:
- Steps 1-12 use stub programs only. Do not run the real `codex` before step 13 passes.
- Never run `codex login`; never set `OPENAI_API_KEY` or a ChatGPT token; never read, copy or list the contents of the owner's `~/.codex` except the one package copy of step 9 (binary and its siblings, no config, no `auth.json`).
- Do not run any other `run-*.sh`, `live*.sh` or `run-all.sh` (they start Claude Code or OpenCode). Never run an older version of a script.

### Interfaces

- Consumes:
  - `scripts/check-premises.sh` (PC-1): no arguments; `N2_FORBIDDEN_NAMES` (space-separated words, run time); exit 0 with last line `check-premises: ok (<ids>)`, else exit 1 `check-premises: <reason>`; `fixtures/codex/config.schema.json` is in `VERBATIM` (path and bearer rules exempt).
  - `ver_ge <have> <min>` (`scripts/premises/ver.sh`): exit 0 when both are dotted numbers and `have >= min`, else 1.
  - `lib.sh` as it is: `new_run <name>` (sets `RUN`, `H`, `R`), `start_mock [rules.json]` (sets `MOCK_URL`, `MOCK_PID`), `stop_mock`, `check_abort`, `scrub <file>`, `DUMMY_KEY`, `EXTRA_ENV`, `REPO_DIR`, `PREMISES_DIR`, `N2_SCRATCH`.
- Produces (PC-3 to PC-6 use these and edit none of the files):
  - `N2_AGENTS` (shell or environment variable read when `lib.sh` is sourced): space-separated names from `claude`, `opencode`, `codex`; default `claude opencode`. For each name, in order, `lib.sh` sets `CLAUDE_BIN`, `OPENCODE_BIN` or `CODEX_BIN` from `command -v`, or prints `premises: <name> is not on PATH` and exits 1 before any scratch folder exists. Another name: `premises: unknown agent '<name>' in N2_AGENTS`, exit 1.
  - `CX_HOME`: the `CODEX_HOME` of the next `run_codex`; default `$H/.codex`, reset by `new_run`. A part may set it to another folder under `$RUN` (PC10).
  - `CODEX_FX`: fixture root; default `$REPO_DIR/fixtures/codex`.
  - `codex_env`: needs `new_run` and `start_mock` first. Checks (each prints its message and exits 1, before any `codex` process starts):
    - `premises: the run folder path holds a blank`
    - `premises: codex HOME is not under the run folder` (`realpath $H` is not under `realpath $RUN`/)
    - `premises: CODEX_HOME is not under the run folder` (same for `CX_HOME`)
    - `premises: the run folder is under ~/.codex` (`realpath $RUN` is under `realpath $OWNER_HOME/.codex`; `OWNER_HOME` is the caller's `HOME`, saved when `lib.sh` is sourced)
    - `premises: EXTRA_ENV sets HOME, CODEX_HOME or PATH`
    - `premises: codex_env needs start_mock first` (`MOCK_URL` empty)
    - `premises: the codex executable is under ~/.codex` (after the copy rule; a guard, never expected)

    Then it makes `$CX_HOME`, `$RUN/tmp`, `$H/.local/state`; writes `$CX_HOME/config.toml` only when it does not exist (text below); sets `CODEX_ENV` to the words
    `env -i PATH=<dir of CODEX_EXE>:/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin TERM=${TERM:-xterm-256color} HOME=$H CODEX_HOME=$CX_HOME XDG_CONFIG_HOME=$H/.config XDG_DATA_HOME=$H/.local/share XDG_CACHE_HOME=$H/.cache XDG_STATE_HOME=$H/.local/state TMPDIR=$RUN/tmp MOCK_API_KEY=$DUMMY_KEY HTTPS_PROXY=$MOCK_URL HTTP_PROXY=$MOCK_URL ALL_PROXY=$MOCK_URL`;
    and, once per process, sets `CODEX_VERSION` from `$CODEX_ENV $CODEX_EXE --version` (first line, last blank-separated word): not `x.y.z` digits → `premises: codex --version gave '<v>', not x.y.z`; `ver_ge "$CODEX_VERSION" "$CODEX_MIN"` fails → `premises: codex <v> is below the minimum 0.172.0`. `CODEX_MIN=0.172.0` (K-C8).
    Base `config.toml` (written verbatim with the mock URL):
    ```toml
    model = "gpt-mock"
    model_provider = "mock"

    [model_providers.mock]
    name = "mock"
    base_url = "<MOCK_URL>/v1"
    env_key = "MOCK_API_KEY"
    wire_api = "responses"
    ```
  - `CODEX_EXE`: set by `codex_env` on first use. If `realpath $CODEX_BIN` is under `realpath $OWNER_HOME/.codex`/: when its folder is named `bin`, copy the folder above it (`cp -R`) to `$N2_SCRATCH/codex-pkg` and use `$N2_SCRATCH/codex-pkg/bin/<name>`; else copy the file to `$N2_SCRATCH/codex-pkg/<name>`. A copy that exists is reused. Otherwise `CODEX_EXE=$CODEX_BIN`.
  - `run_codex <codex args>`: `codex_env`, then `(cd "$R" && $CODEX_ENV $EXTRA_ENV "$CODEX_EXE" "$@")`, then `check_abort`; returns codex's status.
  - `codex_record <part>`: part name matches `[A-Za-z0-9_-]+`, else `premises: bad part name '<part>'`, exit 1; no `codex_env` in this run → `premises: codex_record: no codex run in this run folder`, exit 1. Replaces `$CODEX_FX/runs/<part>/` with `isolation.txt` (exactly `home=scratch`, `codex_home=scratch`, `codex_version=$CODEX_VERSION`, one per line) and `connect.txt` (`scrub` of `$RUN/rec/connect.txt`; empty when the mock saw no `CONNECT`). Call it after `stop_mock`.
  - `scripts/premises/run-codex.sh [<part>]`: sets `N2_AGENTS=codex`, sources `lib.sh`, exports `N2_SCRATCH`; with `<part>` runs `sh codex/<part>.sh` (name `[A-Za-z0-9_-]+`, else `run-codex: bad part name '<part>'`, exit 1; missing file → `run-codex: no part <part>`, exit 1); with none runs every `codex/*.sh` in name order, printing `== codex/<name>.sh` before each; stops at the first failure with its status.
  - A part file `scripts/premises/codex/<part>.sh` starts with `PREMISES_DIR=$(cd "$(dirname "$0")/.." && pwd)`, `N2_AGENTS=codex`, `. "$PREMISES_DIR/lib.sh"`.
  - `mock_upstream.py` rule keys (additions; existing keys unchanged):
    - `path` (regex): a rule with no `path` keeps today's meaning and matches only `POST …/v1/messages`. Rules are tried for `POST …/v1/messages`, `POST …/v1/responses` and `GET …/v1/models` (query string allowed).
    - Reply: `sse` (file) or `json` (file), one of them required (`main()` exits `mock: rule <i> needs sse or json`); relative paths resolve against the rules file folder for both. `status` (int, default 200). `usage` (object): replaces `response.usage` in the `response.completed` event of an SSE reply.
    - `{{MODEL}}` and `{{MSGID}}` work in `sse` and `json` files.
    - No rule matched on `/v1/responses`: the default Responses SSE (`response.created`, `response.output_text.delta` with text `ok`, `response.output_item.done`, `response.completed` with usage `{"input_tokens": 10, "output_tokens": 1, "total_tokens": 11}`), whatever `stream` says. No rule matched on `/v1/models`: 404, as today.
    - `CONNECT <host>:<port>`: no credential check; one line `<host>:<port> refused` appended to `<dir>/connect.txt` (under `LOCK`); reply `403` with `connection: close`; no tunnel; no `<nnnn>.json`.
    - Absolute-URI request (request line target starts with `http://` or `https://`, any method; e.g. `GET http://example.invalid/x HTTP/1.1`): no credential check; one line `<host>:<port> <METHOD> refused` appended to the same `<dir>/connect.txt` (port 80 for `http`, 443 for `https` when the URI names none); same refusal as `CONNECT` (`403`, `connection: close`); no `<nnnn>.json`. An origin-form request (`/v1/...`) is unchanged.

### Steps

- [ ] 1. Write `scripts/premises/mock_upstream_test.py` (new; `python3 -I`, stdlib only; no network beyond 127.0.0.1). It starts `mock_upstream.py` as a child with `MOCK_KEY=dummy-key` built as `"dummy-" + "key"`, `--dir`, `--rules`, `--port-file` in a `tempfile.TemporaryDirectory`, waits for the port file (poll, at most 60 s), kills the child at the end. Requests use `http.client` with `authorization: Bearer <dummy>`; `CONNECT` uses a raw socket. Last line `mock tests: ok (10)`, exit 0; each failure prints `mock test fail: <case>: <why>`, exit 1. Rules file (in this order) and cases:

  | # | Rule(s) | Request | Assert |
  |---|---|---|---|
  | M1 | `{"name":"r-sse","path":"/v1/responses","body":"m1","sse":"a.sse","times":1}` | POST `/v1/responses` body `{"model":"m","stream":true,"input":"m1"}` | 200, `content-type: text/event-stream`, body == `a.sse` with `{{MODEL}}` → `m` |
  | M2 | `{"name":"r-json","path":"/v1/responses","body":"m2","json":"b.json"}` | POST body `{"model":"m","input":"m2"}` | 200, `application/json`, parsed body == `b.json` |
  | M3 | `{"name":"r-err","path":"/v1/responses","body":"m3","status":400,"json":"err.json"}` (`err.json` = `{"error":{"code":"context_length_exceeded","message":"too long"}}`) | POST body `…"input":"m3"` | 400, body == `err.json` |
  | M4 | `{"path":"/v1/responses","body":"m4","sse":"s1.sse","times":1}`, then the same with `s2.sse` | three POSTs `…"input":"m4"` | 1st body == `s1.sse`, 2nd == `s2.sse`, 3rd holds `event: response.completed` and `"output_tokens": 1` (default) |
  | M5 | `{"path":"/v1/responses","body":"m5","sse":"a.sse","usage":{"input_tokens":900000,"output_tokens":5,"total_tokens":900005}}` | POST `…"input":"m5"` | the `response.completed` data's `response.usage` == the rule's usage; every other event of the reply == `a.sse` |
  | M6 | `{"path":"/v1/models","json":"models.json"}` | GET `/v1/models?client_version=0.172.0` | 200, body == `models.json`; record `<nnnn>.json` has `"method": "GET"` |
  | M7 | — | raw `CONNECT example.invalid:443 HTTP/1.1` + `Host:` | status line `HTTP/1.1 403`, the server closes; `connect.txt` == `example.invalid:443 refused\n`; no new `<nnnn>.json` |
  | M8 | `{"name":"msg","sse":"m.sse"}` (no `path`) | POST `/v1/responses` `…"input":"m8"`, then POST `/v1/messages` `{"model":"m","stream":true,"messages":[]}` | first gets the default Responses SSE (rule not used on `/v1/responses`); second gets `m.sse` |
  | M10 | — | raw `GET http://example.invalid/x HTTP/1.1` + `Host: example.invalid` (no credential) | status line `HTTP/1.1 403`, the server closes; `connect.txt` gains the line `example.invalid:80 GET refused`; no new `<nnnn>.json` |
  | M9 | — | POST `/v1/responses` with `authorization: Bearer other` | 401; `ABORT` file exists in the record dir |

  Run M9 last (it stops the mock). `a.sse` is a three-event Responses stream (`response.created`, `response.output_text.delta`, `response.completed` with usage `{"input_tokens":1,"output_tokens":1,"total_tokens":2}`), written by the test.
- [ ] 2. Run `python3 -I scripts/premises/mock_upstream_test.py` → exit 1; M1-M8 and M10 fail (M9 may pass).
- [ ] 3. Change `scripts/premises/mock_upstream.py` to the rule keys, defaults, `CONNECT` and absolute-URI handling in Interfaces. New functions: `default_responses_sse(model: str) -> str`; `set_usage(sse: str, usage: dict) -> str` (rewrites only the `response.completed` event's `data:` JSON). `do_CONNECT` is its own method, not `handle_any`. Update the module docstring (title: mock Anthropic and OpenAI Responses upstream; the new keys; `connect.txt`).
- [ ] 4. Run the step 2 command → exit 0, `mock tests: ok (10)`.
- [ ] 5. In `scripts/premises/scrub_test.py` add:
  - to `CASES`: `("codex rollout file name", "rollout-2026-11-25T10-20-30-" + U1 + ".jsonl", "rollout-2026-11-25T10-20-30-00000000-0000-4000-8000-000000000001.jsonl", [U1])` and `("codex session and turn ids", '{"session_id":"' + U1 + '","turn_id":"' + U2 + '"}', '{"session_id":"00000000-0000-4000-8000-000000000001","turn_id":"00000000-0000-4000-8000-000000000002"}', [U1, U2])`, with `U1 = "0199b0c2-7c1d-7a3e-9f00-1234567890ab"`, `U2 = "0199b0c3-0000-7000-8000-00000000abcd"`;
  - after the host-name checks, a `CODEX_HOME` check with a known run folder `run`: the text `<realpath run>/home/.codex/sessions/2026/11/25/x.jsonl <realpath run>/cx/config.toml` scrubs to `/home/user/.codex/sessions/2026/11/25/x.jsonl /tmp/run/cx/config.toml` (fail text `codex home paths: got …`);
  - the ok count becomes `len(CASES) + 3`.
- [ ] 6. Run `python3 -I scripts/premises/scrub_test.py` → exit 0, `scrub tests: ok (<n>)`. The cases pass with no change to `scrub.py` (checked at planning). If one fails, add the rule to `scrub.py` that makes it pass and name it in the docstring.
- [ ] 7. In `scripts/premises/lib_test.sh`:
  - `mkbin`: also link `python3 git cut tr ls` (and any other tool the new `lib.sh` code calls); always write a stub `codex`: `--version` prints `codex-cli ${STUB_VER:-0.172.0}`; any other call writes `env` to `$HOME/stub-env.txt`, `$0` to `$HOME/stub-exe.txt` and its arguments to `$HOME/stub-args.txt`, prints `stub`, exits 0. Add `mkbin "$T/nocx" 1` then `rm "$T/nocx/codex"`.
  - `rall`: copy `ver.sh` too; add `codex` at the end of the id list; `R1`, `R3`, `R4` expect 13 runs (`R2` stays 4).
  - New cases. Each runs with `XENV` holding `N2_AGENTS=codex` (and `CODEX_FX=$T/fx-<case>`), in caller mode unless named, and with `PATH` from `mkbin` (no `claude`, no `opencode` where it says so):

  | Case | Setup and action | Expect |
  |---|---|---|
  | X1 | own dir; `PATH` with `codex` only (a `mkbin` dir with `claude` and `opencode` removed); `exit 0` | exit 0; own dir removed |
  | X2 | own dir; `PATH=$T/nocx`; `exit 0` | exit 1; stdout `premises: codex is not on PATH`; no dir made |
  | X3 | `new_run x; start_mock; run_codex exec hi; python3 -I -c '<raw CONNECT example.invalid:443 to the mock port>'; stop_mock; codex_record base; exit 0` | exit 0. `stub-env.txt` names, sorted (`LC_ALL=C`), are exactly `ALL_PROXY CODEX_HOME HOME HTTPS_PROXY HTTP_PROXY MOCK_API_KEY PATH TERM TMPDIR XDG_CACHE_HOME XDG_CONFIG_HOME XDG_DATA_HOME XDG_STATE_HOME`; `HOME` = `<RUN>/home` and `CODEX_HOME` = `<RUN>/home/.codex`, both under the kept dir; `HTTPS_PROXY` matches `^http://127\.0\.0\.1:[0-9]+$` and `HTTP_PROXY` and `ALL_PROXY` equal it; `MOCK_API_KEY=dummy-key`; no value holds the tester's `$HOME`; `config.toml` holds `base_url = "<HTTPS_PROXY>/v1"` and `env_key = "MOCK_API_KEY"`; `isolation.txt` is exactly the three lines with `codex_version=0.172.0`; `connect.txt` is exactly `example.invalid:443 refused` |
  | X4 | `STUB_VER=0.171.9`; `new_run x; start_mock; run_codex exec hi` | exit 1; stdout holds `premises: codex 0.171.9 is below the minimum 0.172.0`; no `stub-env.txt` |
  | X5 | `STUB_VER=0.173.0`; as X3 without the CONNECT | exit 0; `isolation.txt` line 3 is `codex_version=0.173.0` |
  | X6 | `new_run x; start_mock; H=$T/elsewhere; run_codex exec hi` | exit 1; `premises: codex HOME is not under the run folder`; no `stub-env.txt` anywhere |
  | X7 | `new_run x; start_mock; CX_HOME=$T/elsewhere; run_codex exec hi` | exit 1; `premises: CODEX_HOME is not under the run folder`; no `stub-env.txt` |
  | X8 | `HOME=$T/hX8`; stub `codex` at `$T/hX8/.codex/packages/standalone/current/bin/codex` with a sibling `codex-package.json`; `PATH` has `$T/hX8/.local/bin` holding a link `codex` to it (`current` itself a link to `releases/v`); action as X3 without the CONNECT | exit 0; `stub-exe.txt` is `<N2_SCRATCH>/codex-pkg/bin/codex`; `<N2_SCRATCH>/codex-pkg/codex-package.json` exists; no `PATH` entry in `stub-env.txt` resolves under `$T/hX8/.codex` |
  | X9 | `HOME=$T/hX9`, `N2_SCRATCH=$T/hX9/.codex/scr`; `new_run x; start_mock; run_codex exec hi` | exit 1; `premises: the run folder is under ~/.codex` |
  | X10 | `new_run x; start_mock; EXTRA_ENV="HOME=$T/elsewhere"; run_codex exec hi` | exit 1; `premises: EXTRA_ENV sets HOME, CODEX_HOME or PATH` |
  | X11 | `new_run x; run_codex exec hi` (no mock) | exit 1; `premises: codex_env needs start_mock first` |
  | X12 | `new_run x; codex_record base` | exit 1; `premises: codex_record: no codex run in this run folder` |
  | X13 | as X3, then `codex_record ../x` | exit 1; `premises: bad part name '../x'` |
  | K1 | copy `run-codex.sh`, `lib.sh`, `ver.sh` to `$T/K-pd` with `codex/b.sh` and `codex/a.sh`, each appending its name to `$T/K.seen`; run `run-codex.sh` | exit 0; `K.seen` is `a` then `b` |
  | K2 | same tree; `run-codex.sh b` | exit 0; `K.seen` is `b` |
  | K3 | same tree; `run-codex.sh zz` | exit 1; `run-codex: no part zz` |
  | K4 | same tree; `run-codex.sh ../a` | exit 1; `run-codex: bad part name '../a'` |
- [ ] 8. Run `sh scripts/premises/lib_test.sh` → exit 1; the X and K cases and R1, R3, R4 fail; the L, E and C cases pass.
- [ ] 9. Change `scripts/premises/lib.sh` to the Interfaces: the `N2_AGENTS` loop replaces `:11-12` (default order keeps the `claude` message of case L3); save `OWNER_HOME`; source `"$PREMISES_DIR/ver.sh"`; `TOOL_PATH` takes the folders of the binaries that are set; `new_run` resets `CX_HOME`; add `CODEX_MIN`, `CODEX_FX`, `codex_env`, `run_codex`, `codex_record`. Update the header comment (which agents are checked; Codex runs use `run_codex`).
- [ ] 10. Write `scripts/premises/run-codex.sh` (new) to the Interfaces. In `scripts/premises/run-all.sh` set `N2_AGENTS="claude opencode codex"` before sourcing `lib.sh`, add `codex` at the end of the loop list (`:13`), and name `codex` in the header comment's "Needs".
- [ ] 11. Write `scripts/premises/codex/base.sh` (new): part header; `mkdir -p "$CODEX_FX"`; `new_run codex-base; start_mock`; `run_codex --version >"$RUN/version.txt"`; `$RUN/help.txt` = for each of `--help`, `exec --help`, `resume --help`: a line `$ codex <args>`, then the output of `run_codex <args> 2>&1`, then a blank line; `stop_mock`; `scrub "$RUN/help.txt" >"$CODEX_FX/help.txt"`; when `$CODEX_FX/config.schema.json` is missing or empty: `env -i PATH=/usr/bin:/bin curl -q -sSfL --max-time 30 -o "$CODEX_FX/config.schema.json" "$URL"` with `URL` = the source above, a `python3 -I` JSON parse check, and `$CODEX_FX/config.schema.source.txt` = `source: <URL>`, `retrieved: <date +%Y-%m-%d>`, `sha256: <shasum -a 256>`, `note: verbatim (not scrubbed); the pinned schema of the rust-v0.172.0 tag (K§6.3.7).`; then `codex_record base`; last line `codex base: ok (codex <CODEX_VERSION>)`.
- [ ] 12. Run `sh scripts/premises/lib_test.sh` → exit 0, `lib tests: ok (<n>)` with every L, E, C, R, X and K case `ok`. Also `sh scripts/premises/ver_test.sh` → exit 0 (regression; stub programs only).
- [ ] 13. Isolation dry run with a stub `codex` first on `PATH` (still no real binary). In a `mktemp -d` folder `S`: write `S/bin/codex` as the step 7 stub; `mkdir -p S/fx`; `echo '{"dry":1}' >S/fx/config.schema.json` (so no fetch); run `N2_SCRATCH=$S/scr CODEX_FX=$S/fx PATH="$S/bin:$PATH" sh scripts/premises/run-codex.sh base`. Pass: exit 0; `S/fx/runs/base/isolation.txt` is the three lines with `codex_version=0.172.0`; `S/fx/runs/base/connect.txt` is empty; `cut -d= -f1 S/scr/codex-base/home/stub-env.txt | LC_ALL=C sort` is the 13 names of X3; `grep -c "$HOME" S/scr/codex-base/home/stub-env.txt` is 0; `S/scr/codex-base/home/stub-exe.txt` is `$S/bin/codex`. Then `rm -rf "$S"`. Do not go on unless every check passes.
- [ ] 14. Real run. Precondition: `pgrep -fl codex` prints nothing (no Codex process of the owner is open). Then `stamp=$(mktemp)`; `sh scripts/premises/run-codex.sh base` → exit 0 with last line `codex base: ok (codex <x.y.z>)`, `x.y.z` ≥ 0.172.0. Then `find "$HOME/.codex" -newer "$stamp" -print` → no output (the run changed nothing in the owner's `~/.codex`); `rm -f "$stamp"`. If the find lists a file, or `connect.txt` holds a host that is not loopback, stop and report it as a finding; do not filter the record.
- [ ] 15. Check the captures: `fixtures/codex/help.txt` holds the three `$ codex` lines and the flags `-c`, `-C` and `resume`; `fixtures/codex/config.schema.json` sha256 equals the one in `config.schema.source.txt`; `fixtures/codex/runs/base/isolation.txt` and `connect.txt` are as in step 13 with the real version.
- [ ] 16. Acceptance (the node's command, exactly): `python3 -I scripts/premises/mock_upstream_test.py && python3 -I scripts/premises/scrub_test.py && sh scripts/premises/lib_test.sh && sh scripts/premises/run-codex.sh base && N2_FORBIDDEN_NAMES="$(id -un)" sh scripts/check-premises.sh && test "$(wc -c < fixtures/codex/help.txt)" -gt 2 && test "$(wc -c < fixtures/codex/config.schema.json)" -gt 2 && grep -qx 'home=scratch' fixtures/codex/runs/base/isolation.txt && grep -qx 'codex_home=scratch' fixtures/codex/runs/base/isolation.txt && grep -Eqx 'codex_version=[0-9]+\.[0-9]+\.[0-9]+' fixtures/codex/runs/base/isolation.txt && test -f fixtures/codex/runs/base/connect.txt && test -z "$(grep -vE '^(127\.0\.0\.1|localhost|\[::1\]):[0-9]+ ' fixtures/codex/runs/base/connect.txt)"` → exit 0.
- [ ] 17. Commit the files of the node's **Files** list (`scrub.py` only if step 6 changed it) with the message `feat(premises): Codex premise harness, help and schema captures (PC-2)`. End it with `Zygo-Session: <session>` and `Zygo-Work: phase4:PC-2`, each on its own line. No Claude attribution.

Notes:
- `base` runs `--version` and `--help` only; it sends no model request. The first model request through the mock is PC-3's PC1 run. So the base `connect.txt` is empty, and a non-loopback line there is a real finding.
- `HTTPS_PROXY`, `HTTP_PROXY` and `ALL_PROXY` all point at the mock (K§6.3.7, operator, 2026-11-25). A `CONNECT` (https) and an absolute-URI request (plain http) to another host are both refused and recorded in `connect.txt`; a SOCKS-only client that reads `ALL_PROXY` reaches the mock, which does not speak SOCKS, so its request fails.
- `docs/premises.md:17` ("checks that `claude` and `opencode` are on `PATH`") becomes incomplete after step 9 (a runner now states its agents). PC-7 owns `docs/premises.md` and its Codex method lines; this node does not edit it.

### Author's checks (2026-11-25)

1. Spec coverage. K§6.3.7 method: `env -i`, temporary `HOME` and `CODEX_HOME`, dummy key at run time, `base_url` at the mock, `HTTPS_PROXY`, `HTTP_PROXY` and `ALL_PROXY` at the mock, refusing and recording `CONNECT` and absolute-URI requests → steps 3, 9 and cases X3, X6-X10, M7, M10. `/v1/responses` SSE and `/v1/models` answers → M1-M6. Schema from the `rust-v0.172.0` tag and help capture → steps 11, 15. K-C8 minimum, not exact → X4, X5. K-D25 (mock directly, no zygo) → step 9 (`base_url` is the mock). K§11 item 10 (scrub hygiene) → steps 5, 11, 16 (`check-premises.sh`). Deliverables 1-5 → M1-M3, M4, M5, M6, M7 and M10; scrub deliverables → step 5; `lib.sh` deliverables → X1-X13; `run-all.sh` → step 10, R cases; `run-codex.sh` → K1-K4.
2. Step scan: each step is one action. Steps 3, 9 and 10 give signatures, texts and rules; only `CODEX_EXE`'s copy rule and the env word list are given in full, because the tests do not fix them alone.
3. Interface match: PC-3 to PC-6 use `run_codex`, `codex_record <part>`, `CX_HOME`, the part header and the mock rule keys as written here. PC-1's `VERBATIM` holds `fixtures/codex/config.schema.json` (`scripts/check-premises.sh:139`). `ver_ge` matches `scripts/premises/ver.sh:5`.
4. Review focus 6: a real host is a refused `CONNECT` record → M7, X3, step 14; `~/.codex` not read → X6-X9, step 13, step 14's `find`; `isolation.txt` record → X3, X5, acceptance.
5. Proportion: the plan gives the interfaces, texts and cases; it holds no function bodies.
6. Live code: every line cited above was read at 4c0de3f9. No existing helper resolves a real path or checks the Codex version; `run-smoke.sh:31` resolves a path with `python3 -I -c 'os.path.realpath'`, and `codex_env` uses the same form.
