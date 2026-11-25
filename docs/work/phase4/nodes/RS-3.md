# RS-3 — `POST /openai/v1/responses` route and forward for Anthropic and chat upstreams

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.4.1 (first item; identity and error shape), K§6.4.2 (stage table rows identity, resolution, redaction, usage, attribution; the stream-order table rows Anthropic and chat), K§6.4.3 (Responses → Anthropic, Responses → chat; stream obligations 6 and 8), K§6.4.4, K§6.7, K§11 items 14, 18 and 20; K-D47, K-D50
**Files:** `crates/zygo-proxy/src/routes.rs`, `crates/zygo-proxy/src/pipeline.rs`, `crates/zygo-proxy/src/forward.rs`, `crates/zygo-proxy/src/errors.rs`, `crates/zygo-proxy/src/redact_stage.rs`, `crates/zygo-proxy/tests/support/responses.rs` (new), `crates/zygo-proxy/tests/responses_route.rs` (new)
**Consumes:** `check_fields` (RS-1), `request` (RS-1), `Encoder::new` (RS-2), `response` (RS-2), `fixtures/codex/errors/` (PC-3)
**Produces:** `pipeline::ModelRoute::Responses`, `support::Harness::start(upstream: Dialect, agent: Agent, extra_toml: &str) -> Harness` (proxy on a `ManualClock`, one session of `agent`, one mock upstream of dialect `upstream`), `Harness::post(&self, path: &str, body: &Value) -> (StatusCode, HeaderMap, Bytes)`, `Harness::get(&self, path: &str) -> (StatusCode, HeaderMap, Bytes)`, `Harness::upstream_bodies(&self) -> Vec<Value>`, `Harness::reply_with(&self, status: u16, content_type: &str, body: Vec<u8>)`, `Harness::usage_rows(&self) -> Vec<RequestRow>`
**Acceptance:** `test "$(cargo test -q -p zygo-proxy --test responses_route -- --list 2>/dev/null | grep -c ': test$')" -ge 8 && cargo test -p zygo-proxy --locked` → at least 8 tests in `responses_route` (a count of 0 fails), then every `zygo-proxy` test target passes on ubuntu, macOS and Windows CI: a captured Codex request streamed through a Claude Code session to an Anthropic upstream returns Responses SSE and records usage; an upstream `529` before the first byte → `529`, an OpenAI error body and no SSE bytes, and an upstream error after the first event → one `response.failed` (K§11 item 14); Responses → chat reaches the chat mock and returns Responses SSE with usage recorded (K§11 item 18); redaction acts on a Responses `input` body (K§11 item 20); a non-streaming request returns one Response object; an over-long prompt reaches the client in the PC14 shape; identity and body errors on the route use the OpenAI error shape

## Planning notes

- Route by dialect: a Responses request from any session is served (K-D47). Tests use a Claude Code session (K§6.4.2).
- Stream order per K§6.4.2: upstream Messages SSE → tee → `Encoder`; chat SSE → existing `StreamMapper` (`translate/sse.rs:48`) → tee → `Encoder`. Today the mapper output goes into the tee at `forward.rs:1098-1110`; the encoder goes after the tee.
- Per-candidate body at forward time: `attempt_body` (`forward.rs:779-831`) translates when the candidate dialect differs from the client's; for this client it chains `request` (RS-1) then, for chat, the existing Messages → chat translator. The same-dialect branch (Responses upstream) is RS-4; until RS-4 it may refuse with a `zygo_` error, with no test claiming it.
- Redaction already picks `Layout::Input` for `Dialect::Openai` (`redact_stage.rs:191`); this node proves it on the route and edits the stage only if the route needs it.
- `upstream_path(route, d)` (`forward.rs:182-188`) and `ModelRoute::client_path` (`:167-172`) gain the `Responses` arm.
- `tests/support/responses.rs` is shared by RS-4 to RS-7, which only read it: a helper one of them needs goes in its own test file.
- Test hooks: the proxy's `test-hooks` feature (`crates/zygo-proxy/Cargo.toml:40-43`) and `ManualClock`; no sleep, no wall-clock bound (Global constraints).

## Plan

Written when the node starts.
