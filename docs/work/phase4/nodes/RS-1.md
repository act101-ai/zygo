# RS-1 — Responses → Messages request translator and the zygo mark on reasoning items

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.4.3 (top-level field lists; request obligations 1-10), K§6.7 (Responses requests), K§10 (field mapping and the mark are the builder's, inside the obligations), K§11 items 11 and 12; K-D21, K-D31, K-D49
**Files:** `crates/zygo-proxy/src/translate/responses_client.rs` (new), `crates/zygo-proxy/src/translate/reasoning.rs`, `crates/zygo-proxy/src/translate/mod.rs`, `crates/zygo-proxy/tests/responses_request.rs` (new)
**Consumes:** `fixtures/codex/requests/` (PC-3)
**Produces:** `translate::responses_client::check_fields(req: &Value) -> Result<(), ZygoError>` (the three top-level lists; refused and unknown fields → `400 zygo_bad_request` naming the field), `translate::responses_client::request(req: &Value, t: &Target) -> Result<(Value, Vec<Adjustment>), ZygoError>` (Responses body → Messages body for target `t`), `translate::reasoning::encode_thinking(t: &Target, block: &Value) -> String` (a `thinking` or `redacted_thinking` block as the zygo-marked `encrypted_content` of a Responses `reasoning` item), `translate::reasoning::decode_thinking(v: &str, t: &Target) -> Option<Value>` (the block back; `None` unless it decodes, fits the size bound and names `t`'s provider and upstream model)
**Acceptance:** `test "$(cargo test -q -p zygo-proxy --test responses_request -- --list 2>/dev/null | grep -c ': test$')" -ge 16 && cargo test -p zygo-proxy --locked --test responses_request && cargo test -p zygo-proxy --locked --test translate` → at least 16 tests in `responses_request` (a count of 0 fails), then both targets pass on ubuntu, macOS and Windows CI: each `fixtures/codex/requests/*.json` body translates to a valid Messages request (K§11 item 11; "valid" as defined below); one case per request obligation 1-10; an unknown top-level field → 400 naming it; each ignore-list field accepted and absent from the Messages body; each refused field → the stated 400 (K§11 item 12); a marked value that does not decode, is over the bound, or names another provider or model is dropped with an adjustment code; the existing reasoning round trip (`translate` target) is unchanged

## Interfaces now

- `translate/reasoning.rs`: `PREFIX: &str = "zygo1:"` (`:12`), `encode(t: &Target, id: &str, encrypted_content: &str) -> String` (`:35`), `decode(sig: &str) -> Option<Signature>` (`:46`). Today's codec carries a Responses reasoning item inside an Anthropic `thinking` signature (§7.6). The new mark is the reverse direction and reuses this codec's form: a prefix plus base64 of a JSON payload, decoded with the same bounded, fail-to-`None` rule. It uses its own prefix (or a kind field), so a value of one direction never decodes as the other.
- `Target::of(c: &Candidate) -> Target` (`translate/mod.rs:33`), `translate_request(req: &Value, to: Dialect, target: &Target) -> Result<(Value, Vec<Adjustment>), ZygoError>` (`:40`), `Adjustment(pub String)` (`pipeline.rs:26`), `ZygoError::bad_request` (`errors.rs:31`).

## Planning notes

- "Valid Messages request" (K§11 item 11): the proxy has no typed Messages request struct (`grep` at `ec17582b`); its Messages input path is `translate_request` (`translate/mod.rs:40`), which reads the body through `messages(req)` (`:353`) and the block helpers. A translated body is valid when it has `model`, `max_tokens` and a non-empty `messages` array whose roles alternate starting with `user`, every content block has a type that path knows, and `translate_request(&body, Dialect::Openai, &t)` and `translate_request(&body, Dialect::OpenaiChat, &t)` both return `Ok` with no `facts_missing` code. A plain `serde_json` parse is not the check.
- The Messages body this node makes is then the input of the existing Messages → chat translator for a chat upstream (K§6.4.3); RS-3 chains them.
- `model` stays as the client sent it; resolution and `upstream_model` are the forward step's (RS-3).

## Plan

Written when the node starts.
