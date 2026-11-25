# PC-2 — Codex premise harness: mock and scrub changes for every Codex part, codex-only `lib.sh` with an isolation record, `run-codex.sh`, help and schema captures

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.3.7 (method paragraph; the schema and help captures), K§4 K-C8 (the pin is a minimum), K§9 (P4-3), K§11 item 10; K-D14, K-D25; `docs/premises.md:3` (a pinned version is a minimum) and "Method"
**Files:** `scripts/premises/mock_upstream.py`, `scripts/premises/mock_upstream_test.py` (new), `scripts/premises/lib.sh`, `scripts/premises/lib_test.sh`, `scripts/premises/scrub.py`, `scripts/premises/scrub_test.py`, `scripts/premises/run-codex.sh` (new), `scripts/premises/codex/base.sh` (new), `scripts/premises/run-all.sh`, `fixtures/codex/help.txt` (new), `fixtures/codex/config.schema.json` (new), `fixtures/codex/config.schema.source.txt` (new), `fixtures/codex/runs/base/` (new)
**Consumes:** `scripts/check-premises.sh` (PC-1)
**Produces:** `run-codex.sh [<part>]` (runs `scripts/premises/codex/<part>.sh`, or every `codex/*.sh` in name order; a part needs no edit of `run-codex.sh`), `codex_env` and `run_codex` in `lib.sh` (env -i, scratch `HOME` and `CODEX_HOME` under the run folder, dummy key built at run time, `HTTPS_PROXY` at the mock, provider `base_url` at the mock; asserts both folders are under the run folder before `codex` starts), `codex_record <part>` in `lib.sh` (writes `fixtures/codex/runs/<part>/isolation.txt` with the lines `home=scratch`, `codex_home=scratch`, `codex_version=<x.y.z>`, and `fixtures/codex/runs/<part>/connect.txt` with one line `<host>:<port> refused` per `CONNECT`), the mock deliverables listed below, the scrub deliverables listed below, `fixtures/codex/help.txt`, `fixtures/codex/config.schema.json`
**Acceptance:** `python3 -I scripts/premises/mock_upstream_test.py && python3 -I scripts/premises/scrub_test.py && sh scripts/premises/lib_test.sh && sh scripts/premises/run-codex.sh base && N2_FORBIDDEN_NAMES="$(id -un)" sh scripts/check-premises.sh && test "$(wc -c < fixtures/codex/help.txt)" -gt 2 && test "$(wc -c < fixtures/codex/config.schema.json)" -gt 2 && grep -qx 'home=scratch' fixtures/codex/runs/base/isolation.txt && grep -qx 'codex_home=scratch' fixtures/codex/runs/base/isolation.txt && grep -Eqx 'codex_version=[0-9]+\.[0-9]+\.[0-9]+' fixtures/codex/runs/base/isolation.txt && test -f fixtures/codex/runs/base/connect.txt && test -z "$(grep -vE '^(127\.0\.0\.1|localhost|\[::1\]):[0-9]+ ' fixtures/codex/runs/base/connect.txt)"` → every command exits 0: the mock, scrub and lib tests pass with one case per deliverable below; the base run used scratch `HOME` and `CODEX_HOME`, recorded the Codex version it ran, and its `CONNECT` record holds no non-loopback host

## Deliverables for the parallel capture nodes (PC-3 to PC-6 edit none of these files)

`mock_upstream.py`, each with a case in `mock_upstream_test.py`:

1. `POST /v1/responses` rule answers: an SSE file, a JSON body, or a status with a body (PC1, PC2, PC13, PC14 context-too-long error).
2. A rule sequence: the n-th request that matches a rule gets the n-th reply (multi-turn and tool-call turns, PC1; the turn sequence before and after `codex resume`, PC10).
3. A `response.completed` usage set by the rule, so a run can raise `usage` (PC8 compaction) and see the compaction request.
4. `GET /v1/models` answer from a rule file (PC7).
5. `CONNECT` handling: each `CONNECT` is refused and recorded as `<host>:<port> refused` (PC1 other hosts; PC16 update request).

`scrub.py`, each with a case in `scrub_test.py`: the Codex rollout file name and rollout id form, the Codex session and turn id forms, and the `CODEX_HOME` paths (run paths already map to `/tmp/run`), if the existing UUID and path rules do not cover them.

`lib.sh`, with cases in `lib_test.sh`: `codex_env`, `run_codex`, `codex_record`; a runner states the agents it needs, so a Codex-only run needs only `codex` on `PATH` (`lib.sh:11-12` require `claude` and `opencode` today).

## Planning notes

- The pinned version is a minimum (K-C8; `docs/premises.md:3`): the harness checks `codex --version` ≥ 0.172.0 with `ver_ge` (`scripts/premises/ver.sh`) and records the version it ran; it does not refuse a later version. `codex-cli 0.172.0` is on the lead's machine (`codex --version`, 2026-11-25).
- The schema is the pinned schema of the `rust-v0.172.0` tag (K§6.3.7), fetched once from the public repository and kept verbatim with its source URL beside it (the `fixtures/claude-code/settings-schema.source.txt` precedent). This fetch is the only network use of the node, and it carries no credential. PC-1 owns its scan exemption.
- `run-all.sh` gains the Codex runner, so it still "runs every experiment" (`docs/premises.md` "Method").

## Plan

Written when the node starts.
