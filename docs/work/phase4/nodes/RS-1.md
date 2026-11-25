# RS-1 — Responses → Messages request translator and the reasoning mark codec

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.4.3 (top-level field lists; request obligations 1-10), K§6.7 (Responses requests), K§10 (field mapping and the mark are the builder's, inside the obligations), K§11 items 11 and 12; K-D21, K-D31, K-D49
**Files:** `crates/zygo-proxy/src/translate/responses_client.rs` (new), `crates/zygo-proxy/src/translate/mod.rs`, `crates/zygo-proxy/tests/responses_request.rs` (new)
**Consumes:** `fixtures/codex/requests/` (PC-3)
**Produces:** `translate::responses_client::check_fields(req: &Value) -> Result<(), ZygoError>` (the three top-level lists; refused and unknown fields → `400 zygo_bad_request` naming the field), `translate::responses_client::request(req: &Value, t: &Target) -> Result<(Value, Vec<Adjustment>), ZygoError>` (Responses body → Messages body for target `t`), `translate::responses_client::mark_reasoning(provider: &str, model: &str, block: &Value) -> String`, `translate::responses_client::unmark_reasoning(v: &str) -> Option<(String, String, Value)>` (bounded; `None` on any fault)
**Acceptance:** `test "$(cargo test -q -p zygo-proxy --test responses_request -- --list 2>/dev/null | grep -c ': test$')" -ge 16 && cargo test -p zygo-proxy --locked --test responses_request` → at least 16 tests (a count of 0 fails), then all pass on ubuntu, macOS and Windows CI: each `fixtures/codex/requests/*.json` body translates to a Messages request that parses as one (K§11 item 11); one case per request obligation 1-10; an unknown top-level field → 400 naming it; each ignore-list field accepted and absent from the Messages body; each refused field → the stated 400 (K§11 item 12); a marked reasoning value that does not decode, is over the bound, or names another provider or model is dropped with an adjustment code

## Planning notes

- Pure functions over `serde_json::Value`, beside `translate/responses.rs` (Messages → Responses upstream) and `translate/chat.rs`. Interfaces now: `Target::of(c: &Candidate) -> Target` (`translate/mod.rs:33`), `translate_request(req: &Value, to: Dialect, target: &Target)` (`:40`), `Adjustment(pub String)` (`pipeline.rs:26`), `ZygoError::bad_request` (`errors.rs:31`).
- The Messages body this node makes is then the input of the existing Messages → chat translator for a chat upstream (K§6.4.3); RS-3 chains them.
- The `model` field stays as the client sent it here; resolution and `upstream_model` are the forward step's (RS-3).

## Plan

Written when the node starts.
