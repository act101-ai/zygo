# PC-2 — Codex premise harness: mock Responses, models and `CONNECT` answers; codex-only `lib.sh`; `run-codex.sh`; help and schema captures

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.3.7 (method paragraph; the schema and help captures), K§9 (P4-3), K§11 item 10; K-D14, K-D25; `docs/premises.md` "Method"
**Files:** `scripts/premises/mock_upstream.py`, `scripts/premises/mock_upstream_test.py` (new), `scripts/premises/lib.sh`, `scripts/premises/scrub.py`, `scripts/premises/scrub_test.py`, `scripts/premises/run-codex.sh` (new), `scripts/premises/codex/base.sh` (new), `scripts/premises/run-all.sh`, `fixtures/codex/help.txt` (new), `fixtures/codex/config.schema.json` (new), `fixtures/codex/config.schema.source.txt` (new)
**Consumes:** —
**Produces:** `run-codex.sh [<part>]` (runs `scripts/premises/codex/<part>.sh`, or every `codex/*.sh` in name order; a part needs no edit of `run-codex.sh`), `lib.sh` `codex_env` and `run_codex` (env -i, scratch `HOME` and `CODEX_HOME`, dummy key built at run time, `HTTPS_PROXY` at the mock, provider `base_url` at the mock), `mock_upstream.py` rules for `POST /v1/responses` (SSE file, JSON, or status with body) and `GET /v1/models`, and a `CONNECT` record file, `fixtures/codex/help.txt`, `fixtures/codex/config.schema.json`
**Acceptance:** `python3 -I scripts/premises/mock_upstream_test.py && python3 -I scripts/premises/scrub_test.py && sh scripts/premises/run-codex.sh base && N2_FORBIDDEN_NAMES="$(id -un)" sh scripts/check-premises.sh && test "$(wc -c < fixtures/codex/help.txt)" -gt 2 && test "$(wc -c < fixtures/codex/config.schema.json)" -gt 2` → every command exits 0: the mock test covers a `/v1/responses` SSE answer, a `/v1/models` answer, an error status answer and a refused, recorded `CONNECT`; `run-codex.sh base` runs `codex --version` = 0.172.0 and writes the help capture under the scratch `HOME`

## Planning notes

- `lib.sh:11-12` exits when `claude` or `opencode` is not on `PATH`. A Codex-only run needs only `codex` (author: each runner states the agents it needs; the existing runners keep their check).
- The schema is the pinned schema of the `rust-v0.172.0` tag (K§6.3.7), fetched once from the public repository and kept verbatim with its source URL beside it (the `fixtures/claude-code/settings-schema.source.txt` precedent). This fetch is the only network use, and it carries no credential.
- `codex` is `codex-cli 0.172.0` on the lead's machine (`codex --version`, 2026-11-25). The harness refuses another version.
- `scrub.py` gains the Codex id forms it meets (rollout and session ids) if the existing UUID rule does not cover them, with `scrub_test.py` cases.
- `run-all.sh` gains the Codex runner, so it still "runs every experiment" (`docs/premises.md` "Method").

## Plan

Written when the node starts.
