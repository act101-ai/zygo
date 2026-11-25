# RS-8 — `GET /openai/v1/models` in the PC7 shape, other PC1 paths, chat route stays 404

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.4.1 (second and third items), K§3 (the chat/completions route stays 404), K§11 item 22
**Files:** `crates/zygo-proxy/src/routes.rs`, `crates/zygo-proxy/src/models.rs`, `crates/zygo-proxy/tests/responses_models.rs` (new)
**Consumes:** `Harness::start` (RS-3), `fixtures/codex/models/` (PC-3), `fixtures/codex/transport/` (PC-3)
**Produces:** `models::list_openai(s: &SessionState, cfg: &Merged) -> Value` (the session's role list in the shape PC7 records)
**Acceptance:** `test "$(cargo test -q -p zygo-proxy --test responses_models -- --list 2>/dev/null | grep -c ': test$')" -ge 3 && cargo test -p zygo-proxy --locked` → at least 3 tests in `responses_models` (a count of 0 fails), then every `zygo-proxy` test target passes on ubuntu, macOS and Windows CI: `GET /openai/v1/models` gives the role list in the PC7 shape; `POST /openai/v1/chat/completions` stays `404 zygo_route_not_in_v1`; each other `/openai/v1/` path that PC1 records is served when K§6.4 gives its function, else `404` in the OpenAI error shape (K§11 item 22)

## Planning notes

- `models::list(s, cfg, d)` (`models.rs:21`) gives the Anthropic list shape for any dialect today; the route at `routes.rs:50` answers `not_in_v1`. The identity rule of the Anthropic models route (`routes.rs:269-273`) applies.

## Plan

Written when the node starts.
