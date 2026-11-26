# RS-1 — Responses → Messages request translator and the zygo mark on reasoning items

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.4.3 (top-level field lists; request obligations 1-10), K§6.7 (Responses requests), K§10 (field mapping and the mark are the builder's, inside the obligations), K§11 items 11 and 12; K-D21, K-D31, K-D49
**Files:** `crates/zygo-proxy/src/translate/responses_client.rs` (new), `crates/zygo-proxy/src/translate/reasoning.rs`, `crates/zygo-proxy/src/translate/mod.rs`, `crates/zygo-proxy/tests/responses_request.rs` (new), `crates/zygo-proxy/tests/data/compaction-request.json` (new, round 3: a verbatim copy of PC-5's `fixtures/codex/compaction/requests/pc8-live-high-0022.json` from `origin/phase-4--PC-5`), `crates/zygo-proxy/src/errors.rs` (contract comment, rounds 2-3), `docs/specs/2026-11-25-client-model-design.md` (K§6.4.3 and its change-log line, step 1)
**Consumes:** `fixtures/codex/requests/` (PC-3)
**Produces:** `translate::responses_client::check_fields(req: &Value) -> Result<(), ZygoError>` (the three top-level lists; refused and unknown fields → `400 zygo_bad_request` naming the field), `translate::responses_client::request(req: &Value, t: &Target, facts: Option<&Facts>) -> Result<(Value, Vec<Adjustment>), ZygoError>` (Responses body → Messages body for target `t`; `facts` = the resolved model's catalog facts, the source of `max_tokens` when the client gives none), `translate::reasoning::encode_thinking(t: &Target, block: &Value) -> String` (a `thinking` or `redacted_thinking` block as the zygo-marked `encrypted_content` of a Responses `reasoning` item), `translate::reasoning::decode_thinking(v: &str, t: &Target) -> Option<Value>` (the block back; `None` unless it decodes, fits the size bound and names `t`'s provider and upstream model)
**Acceptance:** `test "$(cargo test -q -p zygo-proxy --test responses_request -- --list 2>/dev/null | grep -c ': test$')" -ge 16 && cargo test -p zygo-proxy --locked --test responses_request && cargo test -p zygo-proxy --locked --test translate` → at least 16 tests in `responses_request` (a count of 0 fails), then both targets pass on ubuntu, macOS and Windows CI: each `fixtures/codex/requests/*.json` body translates to a valid Messages request (K§11 item 11; "valid" as defined below); one case per request obligation 1-10; an unknown top-level field → 400 naming it; each ignore-list field accepted and absent from the Messages body; each refused field → the stated 400 (K§11 item 12); a marked value that does not decode, is over the bound, or names another provider or model is dropped with an adjustment code; the existing reasoning round trip (`translate` target) is unchanged

## Interfaces now

- `translate/reasoning.rs`: `PREFIX: &str = "zygo1:"` (`:12`), `encode(t: &Target, id: &str, encrypted_content: &str) -> String` (`:35`), `decode(sig: &str) -> Option<Signature>` (`:46`). Today's codec carries a Responses reasoning item inside an Anthropic `thinking` signature (§7.6). The new mark is the reverse direction and reuses this codec's form: a prefix plus base64 of a JSON payload, decoded with the same bounded, fail-to-`None` rule. It uses its own prefix (or a kind field), so a value of one direction never decodes as the other.
- `Target::of(c: &Candidate) -> Target` (`translate/mod.rs:33`), `translate_request(req: &Value, to: Dialect, target: &Target) -> Result<(Value, Vec<Adjustment>), ZygoError>` (`:40`), `Adjustment(pub String)` (`pipeline.rs:26`), `ZygoError::bad_request` (`errors.rs:31`).

## Planning notes

- "Valid Messages request" (K§11 item 11): the proxy has no typed Messages request struct (`grep` at `ec17582b`); its Messages input path is `translate_request` (`translate/mod.rs:40`), which reads the body through `messages(req)` (`:353`) and the block helpers. A translated body is valid when it has `model`, `max_tokens` and a non-empty `messages` array whose roles alternate starting with `user`, every content block has a type that path knows, and `translate_request(&body, Dialect::Openai, &t)` and `translate_request(&body, Dialect::OpenaiChat, &t)` both return `Ok` with no `blocks_dropped:` code (`translate_request` never makes `facts_missing`: only `attempt_body` does, `forward.rs:793`). A plain `serde_json` parse is not the check.
- The Messages body this node makes is then the input of the existing Messages → chat translator for a chat upstream (K§6.4.3); RS-3 chains them.
- `model` stays as the client sent it; resolution and `upstream_model` are the forward step's (RS-3).
- Lead's decision (namespace tools): RS-1 drops a `namespace` tool with `tools_dropped:namespace:<n>`, as planned. A later node RS-9 (depends on RS-1 and PC-5, whose fixtures capture Codex's namespaced `spawn_agent` calls) flattens them into function tools and maps their calls back. The drop is one match arm in `responses_client::tools` (the `other =>` arm), so RS-9 can replace it.
- Lead's decision (remote-compaction input items): not handled by RS-1 (no fixture shows one); noted for P4-9. Such an item is an unknown input item type today, so it gets the obligation 9 400.
- Execution (round 1): behaviour the plan left open, decided in the code: an input item with no `type` and a `role` is a `message` (the Responses short form), with neither → `"input item: missing type"`; a message role other than `user`/`assistant`/`developer`/`system` → `"message: unknown role: <role>"`; a `message` with no content → `"message: missing content"`; a part with no `type` → `"content part: missing type"`; a leading `developer`/`system` message with a non-text part is not leading (it keeps its place as user content); a tool with no `type` → `"tool: missing type"`, a `function`/`custom` tool with no `name` → `"<type> tool: missing name"`; `max_output_tokens` not a positive integer → `"max_output_tokens must be a positive integer"`; `instructions` not a string → `"instructions must be a string"`; `model` not a string → `"model must be a string"`; `custom_tool_as_function` is also set by a `custom_tool_call` item; `parallel_tool_calls: false` does not add `disable_parallel_tool_use` to a `none` tool choice.

## Plan

Base: `origin/phase-4` at 0d665799 (PC-3 merged). Branch `phase-4--RS-1`. Every commit ends with the lines `Zygo-Session: <session>` and `Zygo-Work: phase4:RS-1`; no attribution.

### Premise check (2026-11-25, against 0d665799)

- Fixtures: the four `fixtures/codex/requests/*.json` are mock records; the Responses body is the record's `body` key. Top-level body fields, the same in all four: `model`, `stream`, `input`, `tools`, `tool_choice`, `parallel_tool_calls`, `reasoning`, `store`, `include`, `prompt_cache_key`, `client_metadata`. All are in a K§6.4.3 list except `client_metadata` (thread, turn and window ids, installation id, `x-codex-turn-metadata` with sandbox and workspace state). No fixture sends `instructions` or `max_output_tokens`; every one sends `reasoning: {"summary":"auto"}` (no `effort`).
- Tools in every fixture: `function` tools, one `namespace` tool (`multi_agent_v1`, five nested `function` tools) and one `web_search` tool; `after-custom-tool-call-output.json` also has `custom` `apply_patch` with `format: {"type":"grammar","syntax":"lark",…}`. K§6.4.3 does not name the `namespace` type, and no fixture shows a call to a tool inside one.
- Input items: `message` (roles `developer`, `user`, `assistant`; parts `input_text`, `output_text`), `function_call`, `function_call_output`, `custom_tool_call`, `custom_tool_call_output`, `reasoning` (unmarked `encrypted_content` such as `pc3-opaque-reasoning-1`). The first items are two `developer` messages (PC13, `requests/injected-items.txt`). The assistant `output_text` part is not in obligation 2's list.
- Item ids are unscrubbed (`msg_<uuid>`, `fco_<uuid>`, `ctco_<uuid>`; PC-11 re-scrubs them). No test reads an `id` value; `item_ids_do_not_change_translation` proves the output does not depend on them.
- `max_tokens`: no fixture gives `max_output_tokens`, and `request(req, t)` had no catalog value; facts live in `forward.rs:403` (`rw.facts`), and `facts::apply` (`translate/facts.rs:15`) only lowers a `max_tokens` that is present. So `request` takes `facts: Option<&Facts>` (Produces changed above). RS-2 and RS-3 name `request (RS-1)` without a signature; RS-3's harness config needs `[model.facts] max_output_tokens` for a captured Codex body to translate.
- No existing function maps Responses `input` items to Messages: `translate/responses.rs:126` (`response`) maps Responses `output` items only. Reused helpers: `input_of` (`translate/mod.rs:188`), `s` (`:232`), `DOC_OMITTED` (`:23`); `facts::apply` (`facts.rs:15`) in the tests only.
- K§6.7 checks the mark against "the session's candidates"; translation is per candidate (`forward.rs:784-800`), so `decode_thinking` checks the one target `t`: a mark of another candidate is dropped on this attempt, and no mark selects a provider (review focus 8).
- `ZygoError` messages are "fixed texts (no header, token or body value)" (`errors.rs:9`); K§6.4.3 requires the field name in the 400. The echoed name or type is cut to its first 64 characters (K§6.7 hostile input).

### Interfaces

Consumed (as in the code at 0d665799):

- `pub struct Target { pub provider: String, pub upstream_model: String }` (`translate/mod.rs:27`)
- `pub fn translate_request(req: &Value, to: Dialect, target: &Target) -> Result<(Value, Vec<Adjustment>), ZygoError>` (`translate/mod.rs:40`)
- `pub struct Adjustment(pub String)` (`pipeline.rs:26`); `ZygoError::bad_request(message: impl Into<String>) -> ZygoError` (`errors.rs:31`), fields `status`, `code`, `message` (`errors.rs:12-16`)
- `pub const PREFIX: &str = "zygo1:"`, `pub fn encode(t: &Target, id: &str, encrypted_content: &str) -> String`, `pub fn decode(sig: &str) -> Option<Signature>` (`translate/reasoning.rs:12, 35, 46`)
- `pub fn facts::apply(req: &mut Value, f: &Facts) -> Vec<Adjustment>` (`translate/facts.rs:15`); `zygo_config::Facts` (`Default`; field `max_output_tokens: u64`; `zygo-config/src/schema.rs:125`)
- `pub(crate) fn input_of(args: Option<&str>) -> Value` (`translate/mod.rs:188`)

Produced:

- `translate::responses_client::check_fields(req: &Value) -> Result<(), ZygoError>`
- `translate::responses_client::request(req: &Value, t: &Target, facts: Option<&Facts>) -> Result<(Value, Vec<Adjustment>), ZygoError>`
- `translate::reasoning::encode_thinking(t: &Target, block: &Value) -> String`
- `translate::reasoning::decode_thinking(v: &str, t: &Target) -> Option<Value>`
- `translate::reasoning::THINKING_PREFIX: &str = "zygo1t:"`; `translate::reasoning::MARK_MAX: usize = 4 << 20`

### Steps

- [x] 1. **Spec.** In `docs/specs/2026-11-25-client-model-design.md` K§6.4.3 (keep heading and numbering), rewrite as final text:
  - the "Safe to ignore" bullet's list: `store`, `include`, `prompt_cache_key`, `metadata`, `client_metadata` (Codex's request metadata: thread, turn and window ids, installation id, sandbox and workspace state; every PC-3 request carries it), `service_tier`, `user`, `safety_identifier`, `stream_options`, `truncation` (only `"disabled"`; another value → 400 that names `truncation`).
  - obligation 1: "`instructions` and the leading `developer`/`system` items (those before the first item of another kind) become the system text, joined with a blank line. A later `developer`/`system` item keeps its place between turns as a `text` block of a `user` message; no developer text is dropped."
  - obligation 2: "`message` items map by role (`user`, `assistant`); items that map to the same role in a row join one message, so roles alternate; in a user message `tool_result` blocks come first. `input_text` and the assistant's `output_text` and `refusal` map to text blocks; `input_image` to an image block (a `data:` URL → base64 source, another URL → url source); `input_file` whose `file_data` is a base64 PDF to a document block, any other file to the text `[document omitted: the model cannot read it here]` with `documents_dropped:<n>` (§7.6 document rule). Another part type, or an `input_image` with no `image_url` → 400 that names it. A string `input` is one user text message."
  - obligation 4, last sentence: "A `custom` tool definition becomes a function tool with one required string parameter `input`; a `grammar` format's `definition` becomes that parameter's `description`; adjustment code `custom_tool_as_function`."
  - obligation 5, end: "…; others are dropped with `reasoning_dropped:<n>`."
  - obligation 6: "Tool definitions: `function` maps to a Messages tool (`parameters` → `input_schema`); `custom` by item 4; every other type (hosted tools such as `web_search`, and `namespace`: no premise records how Codex calls a tool inside one) is dropped with `tools_dropped:<type>:<n>`. Hosted-tool input items (`web_search_call`, `file_search_call`, `computer_call`, `computer_call_output`, `local_shell_call`, `local_shell_call_output`, `code_interpreter_call`, `image_generation_call`, `mcp_call`, `mcp_list_tools`, `mcp_approval_request`, `mcp_approval_response`) are dropped with `items_dropped:<type>:<n>`."
  - obligation 7, second sentence: "`max_tokens` is the client's `max_output_tokens`, else the catalog's `facts.max_output_tokens`; with neither (no facts, or 0) → 400 that names `max_output_tokens`."
  - `## Change log`, new last line: ``- 2026-11-25: K§6.4.3 — `client_metadata` in the safe-to-ignore list (every captured Codex request sends it, PC-3); obligations 1, 2, 4-7 name the place of a later developer item, the assistant text parts, the grammar text, the tool-definition types (`namespace` dropped), the hosted item types, the adjustment codes and `max_tokens` with no catalog value (RS-1 premise check); author's call.``
  - Check: `grep -c client_metadata docs/specs/2026-11-25-client-model-design.md` ≥ 2; the diff touches only K§6.4.3 and the change log. Commit `docs(spec): K§6.4.3 field and tool classes for Codex requests`.

- [x] 2. **Mark tests.** Create `crates/zygo-proxy/tests/responses_request.rs` with these helpers and tests (the file grows in steps 5 and 7):
  ```rust
  fn tgt() -> Target { Target { provider: "p-ant".into(), upstream_model: "up-ant".into() } }
  fn think() -> Value { json!({"type":"thinking","thinking":"t1","signature":"sig1"}) }
  fn redacted() -> Value { json!({"type":"redacted_thinking","data":"R1"}) }
  fn raw_mark(payload: &Value) -> String // THINKING_PREFIX + STANDARD base64 of the payload's JSON
  ```
  - `mark_round_trip`: for `b` in `[think(), redacted()]`: `let v = encode_thinking(&tgt(), &b); assert!(v.starts_with(THINKING_PREFIX)); assert_eq!(decode_thinking(&v, &tgt()), Some(b));`
  - `mark_undecodable_is_none`: each of `""`, `"zygo1t:"`, `"zygo1t:!!!"`, `raw_mark(&json!({}))`, `raw_mark(&json!({"provider":"p-ant","model":"up-ant","block":{"type":"text","text":"x"}}))`, `raw_mark(&json!({"provider":"p-ant","model":"up-ant","block":{"type":"thinking","thinking":"x"}}))` (no `signature`), `raw_mark(&json!({"provider":"p-ant","model":"up-ant","block":{"type":"redacted_thinking"}}))` (no `data`) → `decode_thinking(v, &tgt()) == None`.
  - `mark_over_bound_is_none`: `let b = json!({"type":"thinking","thinking":"x".repeat(MARK_MAX),"signature":"s"}); let v = encode_thinking(&tgt(), &b); assert!(v.len() > MARK_MAX); assert_eq!(decode_thinking(&v, &tgt()), None);`
  - `mark_other_provider_or_model_is_none`: `v = encode_thinking(&tgt(), &think())`; `decode_thinking(&v, &Target { provider: "other".into(), ..tgt() }) == None` and `decode_thinking(&v, &Target { upstream_model: "other".into(), ..tgt() }) == None`.
  - `marks_do_not_cross`: `decode(&encode_thinking(&tgt(), &think())) == None`; `decode_thinking(&encode(&tgt(), "rs_1", "ENC"), &tgt()) == None`.
  - Check: `cargo test -p zygo-proxy --test responses_request` fails to compile (no `encode_thinking`). Commit `test(proxy): RS-1 reasoning mark cases`.

- [x] 3. **Mark code** in `crates/zygo-proxy/src/translate/reasoning.rs`: `pub const THINKING_PREFIX: &str = "zygo1t:";` (doc: the mark on a Responses reasoning item's `encrypted_content`, the reverse of `PREFIX`; a `zygo1t:` value never passes `strip_prefix("zygo1:")`), `pub const MARK_MAX: usize = 4 << 20;` (doc: the K§6.7 size bound on a marked value, in bytes; the value of `tee::HOLD_MAX`), `pub fn encode_thinking(t: &Target, block: &Value) -> String` (`THINKING_PREFIX` + base64 of `{"provider": t.provider, "model": t.upstream_model, "block": block}`), `pub fn decode_thinking(v: &str, t: &Target) -> Option<Value>` (`None` when `v.len() > MARK_MAX`, checked before decoding; the prefix is missing; base64 or JSON fails; `provider`/`model` differ from `t`; or `block` is neither a `thinking` with string `thinking` and `signature` nor a `redacted_thinking` with string `data`; else the block). Check: the 5 tests pass; `cargo test -p zygo-proxy --locked --test translate` passes. Commit `feat(proxy): zygo mark for thinking blocks in Responses reasoning items`.

- [x] 4. **Module.** `crates/zygo-proxy/src/translate/mod.rs`: add `pub mod responses_client;` after `pub mod responses;`. Create `crates/zygo-proxy/src/translate/responses_client.rs` with the doc `//! Responses client → Messages (K§6.4.3, K§6.7).`, these constants, and the two `responses_client` signatures of "Produced" with `todo!()` bodies:
  ```rust
  pub const MAPPED: [&str; 12] = ["model","instructions","input","tools","tool_choice","parallel_tool_calls","max_output_tokens","temperature","top_p","reasoning","stream","text"];
  pub const IGNORED: [&str; 10] = ["store","include","prompt_cache_key","metadata","client_metadata","service_tier","user","safety_identifier","stream_options","truncation"];
  pub const REFUSED: [&str; 4] = ["previous_response_id","background","conversation","prompt"];
  pub const NO_STATE: &str = "zygo keeps no response state; send the full input with store=false";
  pub const HOSTED_ITEMS: [&str; 12] = ["web_search_call","file_search_call","computer_call","computer_call_output","local_shell_call","local_shell_call_output","code_interpreter_call","image_generation_call","mcp_call","mcp_list_tools","mcp_approval_request","mcp_approval_response"];
  ```
  Check: `cargo build -p zygo-proxy --locked` exits 0.

- [x] 5. **Field-list tests** (K§11 item 12; obligation 8). Helper `fn err<T: std::fmt::Debug>(r: Result<T, ZygoError>) -> ZygoError` (the `Err`, else panic). Each 400 asserts `status == StatusCode::BAD_REQUEST`, `code == "zygo_bad_request"` and the exact `message`:
  - `unknown_top_level_field_400_names_it`: `check_fields(&json!({"model":"m","input":[],"frobnicate":1}))` → `"unknown field: frobnicate"`.
  - `unknown_field_name_is_cut_to_64`: key `"k".repeat(64) + &"z".repeat(500)` → `format!("unknown field: {}", "k".repeat(64))`.
  - `each_ignore_field_accepted`: for `f` in `IGNORED`, `check_fields` of `{"model":"m","input":[], f: v}` is `Ok`; `v` = `"disabled"` for `truncation`, `{}` for `metadata`/`client_metadata`/`stream_options`, `["reasoning.encrypted_content"]` for `include`, `false` for `store`, else `"x"`.
  - `truncation_other_than_disabled_400`: `"truncation":"auto"` → `"truncation: only \"disabled\" is accepted"`.
  - `each_refused_field_400`: for `f` in `REFUSED` → `format!("{f}: {NO_STATE}")`.
  - `body_not_object_400`: `check_fields(&json!([1]))` → `"request body must be a JSON object"`.
  - Check: these 6 fail (`todo!` panics). Commit `test(proxy): RS-1 top-level field lists`.

- [x] 6. **`check_fields`**: not an object → `"request body must be a JSON object"`; then each key in body order, first failure returned: in `REFUSED` → `"{key}: {NO_STATE}"`; `truncation` not `"disabled"` → `"truncation: only \"disabled\" is accepted"`; in `MAPPED` or `IGNORED` → accepted; else `"unknown field: <first 64 chars of key>"`. Check: the 6 tests pass. Commit `feat(proxy): Responses top-level field lists`.

- [x] 7. **Request tests** (K§11 item 11; obligations 1-10). Helpers:
  ```rust
  fn facts() -> Facts { Facts { max_output_tokens: 32000, ..Facts::default() } }
  /// The `body` of every `fixtures/codex/requests/*.json` record, with its file name; asserts at least 4.
  fn fixtures() -> Vec<(String, Value)> // Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/codex/requests")
  fn body(input: Value) -> Value { json!({"model":"m","input":input,"max_output_tokens":100}) }
  fn tr(b: &Value) -> (Value, Vec<String>) // request(b, &tgt(), Some(&facts())).unwrap(), codes as strings
  fn assert_valid(m: &Value)
  // user(t)/asst(t)/dev(t)/sys(t): a `message` of that role with one input_text (asst: output_text) part
  // fc(call_id, name, arguments) / fco(call_id, output): function_call / function_call_output items
  ```
  `assert_valid`: `m["model"]` is a string; `m["max_tokens"]` is a u64 > 0; `m["messages"]` is a non-empty array with roles `user`, `assistant`, `user`, … from the first; every block `type` is one of `text`, `image`, `document`, `tool_use`, `tool_result`, `thinking`, `redacted_thinking`; for `to` in `[Dialect::Openai, Dialect::OpenaiChat]`, `translate_request(m, to, &tgt())` is `Ok` and no code starts with `blocks_dropped:`.
  - `fixtures_translate_to_valid_messages`: each fixture: `let (m, _) = tr(&b); assert_valid(&m); assert_eq!(m["model"], b["model"]); assert_eq!(m["max_tokens"], 32000);`
  - `item_ids_do_not_change_translation`: each fixture, and a copy with every `input[*].id` set to `"x"`, give equal `tr` results.
  - `ob1_system_text_and_later_developer_item`: `{"model":"m","max_output_tokens":100,"instructions":"I","input":[dev("D1"), sys("S1"), user("u1"), asst("a1"), dev("D2"), user("u2")]}` → `m["system"] == "I\n\nD1\n\nS1"`; `m["messages"] == [{"role":"user","content":[{"type":"text","text":"u1"}]}, {"role":"assistant","content":[{"type":"text","text":"a1"}]}, {"role":"user","content":[{"type":"text","text":"D2"},{"type":"text","text":"u2"}]}]`.
  - `ob1_fixture_developer_items_in_system`: `first-turn.json`: each `input_text` text of `input[0]` and `input[1]` is a substring of `m["system"]`; no message block's text equals one of them.
  - `ob2_message_parts`: user parts `input_text "a"`, `input_image` `image_url "data:image/png;base64,QUJD"`, `input_image` `image_url "https://e.x/i.png"`, `input_file` `file_data "data:application/pdf;base64,UERG"`, `input_file` `file_id "file_1"`, then an assistant message with `output_text "b"`, `refusal "c"` → user content `[{"type":"text","text":"a"}, {"type":"image","source":{"type":"base64","media_type":"image/png","data":"QUJD"}}, {"type":"image","source":{"type":"url","url":"https://e.x/i.png"}}, {"type":"document","source":{"type":"base64","media_type":"application/pdf","data":"UERG"}}, {"type":"text","text":DOC_OMITTED}]`; assistant content `[{"type":"text","text":"b"},{"type":"text","text":"c"}]`; codes contain `"documents_dropped:1"`. `"input":"hi"` → messages `[{"role":"user","content":[{"type":"text","text":"hi"}]}]`.
  - `ob2_unknown_part_400`: user part `{"type":"input_audio"}` → `"unknown content part type: input_audio"`; `{"type":"input_image"}` → `"input_image: missing image_url"`.
  - `ob3_function_calls_grouped`: `[user("go"), fc("c1","f","{\"a\":1}"), fc("c2","g","{}"), fco("c1","r1"), fco("c2","r2"), user("next")]` → `[user [text "go"], assistant [{"type":"tool_use","id":"c1","name":"f","input":{"a":1}}, {"type":"tool_use","id":"c2","name":"g","input":{}}], user [{"type":"tool_result","tool_use_id":"c1","content":"r1"}, {"type":"tool_result","tool_use_id":"c2","content":"r2"}, text "next"]]`. `[user("go"), fc("c1","f","{}"), dev("D"), fco("c1","r")]` → last user content `[tool_result c1, text "D"]`.
  - `ob4_custom_tool_call_and_definition`: `after-custom-tool-call-output.json` → the tool named `apply_patch` has `input_schema == {"type":"object","properties":{"input":{"type":"string","description": <the fixture tool's format.definition>}},"required":["input"]}`; a `tool_use` with `id "call_pc3_2"`, `name "apply_patch"`, `input == {"input": <the fixture custom_tool_call's input>}`; a `tool_result` with `tool_use_id "call_pc3_2"`; codes contain `"custom_tool_as_function"`.
  - `ob5_marked_reasoning_in_place`: `[user("go"), reasoning(encode_thinking(&tgt(), &think())), reasoning(encode_thinking(&tgt(), &redacted())), fc("c1","f","{}"), fco("c1","r")]` (`reasoning(e)` = `{"type":"reasoning","summary":[],"encrypted_content":e}`) → assistant content `[think(), redacted(), tool_use c1]`; no code starts with `reasoning_dropped`.
  - `ob5_unmarked_or_foreign_reasoning_dropped`: `turn-with-reasoning.json` → no `thinking` block; codes contain `"reasoning_dropped:2"`. Items with `encrypted_content` = `encode_thinking(&Target { provider: "other".into(), ..tgt() }, &think())`, `"zygo1t:!!!"`, `encode(&tgt(), "rs", "E")`, and one with none → codes contain `"reasoning_dropped:4"`; no `thinking` block.
  - `ob6_tools_and_hosted_items_dropped`: `first-turn.json` → codes contain `"tools_dropped:namespace:1"` and `"tools_dropped:web_search:1"`; no `m["tools"]` name is `multi_agent_v1` or one of its nested names; each fixture `function` tool is there with `input_schema` = its `parameters`. `[user("go"), {"type":"web_search_call","id":"ws","status":"completed"}, asst("a")]` → codes contain `"items_dropped:web_search_call:1"`.
  - `ob7_mapped_fields_then_facts`: `{"model":"m","input":[user("go")],"tools":[{"type":"function","name":"f","parameters":{"type":"object"}}],"tool_choice":"required","parallel_tool_calls":false,"max_output_tokens":99999,"temperature":0.5,"top_p":0.9,"stream":true,"reasoning":{"effort":"xhigh","summary":"auto"}}` → `m["tool_choice"] == {"type":"any","disable_parallel_tool_use":true}`, `max_tokens 99999`, `temperature 0.5`, `top_p 0.9`, `stream true`, `output_config == {"effort":"xhigh"}`; then `facts::apply(&mut m, &Facts { reasoning: Reasoning::Always, effort_levels: vec![Effort::Low, Effort::Medium, Effort::High], max_output_tokens: 32000, sampling: false, ..Facts::default() })` contains `"effort_clamped:xhigh>high"`, `"max_output_clamped:99999>32000"`, `"sampling_dropped"`. `tool_choice` `"auto"` → `{"type":"auto"}`; `"none"` → `{"type":"none"}`; `{"type":"function","name":"f"}` and `{"type":"custom","name":"f"}` → `{"type":"tool","name":"f"}`; `{"type":"web_search"}` → 400 `"tool_choice: unknown form"`.
  - `ob7_max_tokens_from_catalog_or_400`: `{"model":"m","input":[user("go")]}` with `Some(&facts())` → `max_tokens == 32000`; with `None`, and with `Some(&Facts::default())` → 400 `"max_output_tokens is required: the model's catalog states no max_output_tokens"`.
  - `ob8_request_applies_field_lists`: `request` with `"frobnicate":1` → 400 `"unknown field: frobnicate"`; with `"previous_response_id":"r"` → 400 `format!("previous_response_id: {NO_STATE}")`.
  - `ob9_unknown_or_incomplete_item_400`: `{"type":"mystery_item"}` → `"unknown input item type: mystery_item"`; `{"type":"function_call_output","output":"x"}` → `"function_call_output: missing call_id"`; `{"type":"function_call","call_id":"c","arguments":"{}"}` → `"function_call: missing name"`; `{"type":"custom_tool_call","call_id":"c","name":"n"}` → `"custom_tool_call: missing input"`; `{"type":"message","content":[]}` → `"message: missing role"`.
  - `ob10_ignored_fields_absent`: each fixture body with every `IGNORED` field set (values of `each_ignore_field_accepted`) → the keys of `m` ⊆ `{"model","system","messages","max_tokens","temperature","top_p","stream","tools","tool_choice","output_config"}`.
  - `text_format_rules`: `"text":{"format":{"type":"text"},"verbosity":"low"}` → `Ok`, no key `text` in `m`; `"text":{"format":{"type":"json_schema","name":"o","schema":{}}}` → 400 `"text.format: only type text is accepted toward this upstream"`.
  - Check: these 17 fail (`todo!`). Commit `test(proxy): RS-1 request obligations and fixtures`.

- [x] 8. **`request`** in `responses_client.rs`. First `check_fields(req)?`. The output object is built new, so no ignored field is carried (obligation 10): `model` (as sent), `system` (only when the text is non-empty), `messages`, `max_tokens`, `temperature`, `top_p`, `stream` (each copied when present), `tools` (when non-empty), `tool_choice` (when set), `output_config: {"effort": <reasoning.effort>}` (when `reasoning.effort` is a string). Mapping: the K§6.4.3 text of step 1 with the exact values the step 7 tests fix; `function_call.arguments` through `input_of`; a `function_call_output`/`custom_tool_call_output` `output` string stays a string, an array maps its parts as user parts; `parallel_tool_calls: false` sets `disable_parallel_tool_use: true` on `tool_choice` (`{"type":"auto"}` when none is given). Other error text: `input` neither string nor array → `"input must be a string or an array"`; an item or part type in a message is cut to 64 characters. Codes in this order, none with a zero count: `custom_tool_as_function`, `reasoning_dropped:<n>`, `documents_dropped:<n>`, `tools_dropped:<type>:<n>` (types sorted), `items_dropped:<type>:<n>` (types sorted). Check: every `responses_request` test passes. Commit `feat(proxy): Responses → Messages request translator`.

- [x] 9. **Acceptance and floor.** Run alone: `test "$(cargo test -q -p zygo-proxy --test responses_request -- --list 2>/dev/null | grep -c ': test$')" -ge 16 && cargo test -p zygo-proxy --locked --test responses_request && cargo test -p zygo-proxy --locked --test translate` → exit 0 (28 tests listed). Then `cargo test --workspace --locked --no-fail-fast` → every target passes except the 2 known `zygo --test guard_hook` failures in a checkout under /private/tmp (an environment failure, not a defect). `git diff --stat origin/phase-4` lists only the Files line's paths and this node file (plan text corrected in round 1: the base moved to 92e342d7 and was merged, so a diff against 0d665799 also shows `graph.md`). Tick the boxes; commit `docs(work): RS-1 steps done`; push.

### Author's checks (2026-11-25)

1. Spec coverage: field lists → steps 5-6; obligations 1-10 → step 7 (`ob1`-`ob10`; obligation 8 also step 5); K§6.7 mark rules → steps 2-3 and `ob5_*`; K§11 item 11 → `fixtures_translate_to_valid_messages`; item 12 → step 5; the `text` rule → `text_format_rules`.
2. Step scan: each code step names its signature, file and fixed values; `request`'s body is decided by the step 1 text and the step 7 assertions.
3. Interface match: RS-2 consumes `encode_thinking`, `decode_thinking`, `request`; RS-3 consumes `check_fields`, `request` (its call passes `rw.facts`, `forward.rs:403`). Names as in Produces.
4. Review focus 7 → `ob9_unknown_or_incomplete_item_400`, `each_refused_field_400`, `text_format_rules`; review focus 8 → `mark_*`, `ob5_unmarked_or_foreign_reasoning_dropped`.
5. Proportion: no step carries a function body; the plan gives signatures, constants, messages and assertions.
6. Live code: every path, symbol and line cited above was opened at 0d665799.

## Review round 1 findings

Review of PR #152 at d9092120: 1 important, 8 nits. Round 2 merged origin/phase-4 at 06272761 (RS-9 added), then wrote a failing test and a fix for each finding. The K§6.4.3 text changed for each rule (`612cae4b`, one change-log line). Errors.rs's contract comment now allows the echoed name (F3).

- [x] **F1 (important)** — the conversation could be empty or start with the assistant, and such a request failed at the upstream with no fallback. Now: `400 "input: the conversation must start with a user message"`. Test `f1_conversation_must_start_with_user` (`[developer, assistant, user]`, `[developer, function_call, function_call_output]`, `[]`, `[developer]`). Commits `5bc47c47` (test), `f8c6df1d` (fix).
- [x] **F2** — a client tool type went into an adjustment code as sent. Now a type of 1-32 characters from `[a-z0-9_]` is kept and any other is `other` (`code_type`, used for `tools_dropped` and `items_dropped`). Test `f2_dropped_type_in_code_is_safe` (newline, comma and colon, non-ASCII, 33 characters). Commits `294f05d5`, `e411ef7f`.
- [x] **F3** — echoed names could hold control characters. `cut` now takes 64 characters and replaces each control character with `?`. The `errors.rs` contract comment states that a 400 may echo a client field or type name, cut and cleaned this way. Test `f3_echoed_names_have_no_control_characters` (the reviewer's `\u{1b}[31m\r\nX-Injected: 1` as a field name, item type, role and part type). Commits `0071c71f`, `5f689745`.
- [x] **F4** — `decode_thinking` returned the block with every key. It now rebuilds the block from its known keys only. Test `f4_decoded_block_keeps_known_keys_only` (`cache_control` and an extra key). Commits `b89073e3`, `b8ffa52c`.
- [x] **F5** — with no tool left to send, `tool_choice` and the parallel flag are omitted, and `required` gives `400 "tool_choice: required, but no tool is sent"`. A named tool that is not sent (also one inside a dropped namespace) gives `400 "tool_choice: the named tool is not among the sent tools"`. Test `f5_tool_choice_against_sent_tools`. `ob7_mapped_fields_then_facts`'s `choice` helper now sends tool `f`. Commits `f39cb1f9`, `ed03f7d7`.
- [x] **F6** — a mapped field of the wrong JSON type gives a typed 400: `stream`, `parallel_tool_calls` must be a boolean; `temperature`, `top_p` a number; `reasoning`, `text`, `text.format` an object; `reasoning.effort` a string. `null` counts as absent. Test `f6_mapped_field_of_wrong_type_400`. Commits `bc373def`, `b43cb126`.
- [x] **F7** — the message short form now applies only when `type` is absent; a non-string `type` gives `400 "input item: type must be a string"`. Test `f7_short_form_only_without_type`. Commits `ec8d97be`, `35f9c09e`.
- [x] **F8** — tool-call pairing (`Conv::call`, `Conv::output`, `Conv::all_answered`). These cases each give a 400:
  - a duplicate `call_id` among calls → `"<call type>: duplicate call_id"`;
  - an output with no earlier unanswered call of its kind with the same `call_id` → `"<output type>: no earlier matching call"`;
  - a call with no output before the next `user` message or the end of `input` → `"<call type>: no output before the next user message or the end of input"`.

  A `developer` item between a call and its output stays allowed (as in `ob3_function_calls_grouped`). Test `f8_tool_call_pairing`; the four fixtures still translate. Commits `da5628bd`, `d18aee65`.
- [x] **F9** — empty text blocks are dropped everywhere (message parts, a string `input`, tool-output parts), and a message left with no block is dropped, so F1 then applies. Tests:
  - `f9_empty_text_blocks_dropped` (fails before the fix);
  - added tests for cases the reviewer listed: `f9_output_array_maps_parts`, `f9_parallel_false_without_tool_choice`, `f9_item_and_part_errors` (the 64-character cut on a multi-byte item type and part type, `input item: missing type`, `message: unknown role: tool`), `f9_other_hosted_items_dropped` (`local_shell_call`, `mcp_call`).

  Commits `3f339d0a`, `827d5519`.

Round 2 results: Acceptance exit 0 (`responses_request` 41 tests, `translate` 48). `cargo test -p zygo-proxy --locked` passes. `cargo clippy -p zygo-proxy --locked --all-targets -- -D warnings` is clean. `cargo test --workspace --locked --no-fail-fast` fails only on the 2 known `zygo --test guard_hook` tests under /private/tmp.

## Review round 2 findings

Review at e3aaa925: 1 important, 5 nits. In round 3, origin/phase-4 had not moved (06272761 was already merged). Each finding was fixed with a failing test first; none is left open. The K§6.4.3 text and one change-log line are in `c08d4c94`, with the `errors.rs` contract comment.

- [x] **G1 (important)** — tool history with no tool left to send. Anthropic refuses `tool_use` blocks whose name no sent tool defines.
  - `history_tools` declares one tool per distinct `tool_use` name that no sent tool defines, in first-use order: `{"name", "description":"history only", "input_schema":{"type":"object"}}`, with adjustment code `tools_from_history:<n>`.
  - When the sent list was empty, `tool_choice` becomes `{"type":"none"}`. When tools were sent, `tool_choice` is unchanged.
  - The F5 rules still apply to the client's `tool_choice` against the tools the client sent. So `required` with only history tools stays a 400.
  - Tests:
    - `g1_compaction_request_declares_history_tools`, on PC-5's compaction body, copied to `tests/data/compaction-request.json`;
    - `g1_no_tools_with_history`;
    - `g1_dropped_namespace_tool_called_in_history`.
  - Commits `6792cc89` (test), `7cbeb809` (fix).
  - Side effect: `turn-with-reasoning.json` calls `apply_patch` and sends no `custom` tool, so it now declares `apply_patch` from history.
- [x] **G2** — `adjacency` checks the translated messages:
  - a thinking block after a `tool_use` in the same assistant message → `400 "input: a reasoning item follows a tool call in the same assistant turn"`;
  - an assistant message with `tool_use` blocks not followed at once by a user message holding a `tool_result` for each → `400 "input: each tool call's output must come in the next user turn, before other assistant content"`.

  Test `g2_tool_adjacency`:
  - 400 cases, the reviewer's orders: reasoning between a call and its output; reasoning between two outputs; assistant text between two outputs; a developer item between two parallel calls.
  - Still translate: parallel outputs in both orders; a developer item between a call and its output; a custom pair; custom and function calls in parallel; a hosted item between a call and its output; reasoning, a call and its output, then reasoning and assistant text.
  - Not counted as a 400: the reviewer's "assistant text between call and output" gives `A[tool_use, text] U[tool_result]`, which is valid Messages. It still translates.

  Commits `54b747cb`, `2dbdee08`.
- [x] **G3** — the user-message pairing check now runs after empty-message dropping. Test `g3_pairing_after_empty_message_dropped`. Commits `9493edd6`, `9e30692f`.
- [x] **G4** — two sent tools with the same name → `400 "tools: two tools have the same name"`. Test `g4_duplicate_tool_names_400`. Commits `f9057778`, `823f4896`.
- [x] **G5** — K§6.4.3 listed `text.verbosity` as mapped, but Messages has no counterpart. Corrected to "checked and not sent":
  - a value other than `low`, `medium`, `high` → `400 "text.verbosity must be one of low, medium, high"`; `null` is absent;
  - another `text` key → `400 "unknown field: text.<key>"`.

  Test `g5_text_sub_keys`. Commits `0b2e693f`, `119484b2`.
- [x] **G6** — `cut` also replaces Unicode Cf, Zl and Zp characters with `?`. The list is written by hand (std has no category lookup) and checked against Python's Unicode 13 database, plus the Unicode 14-15 additions U+0890-0891 and U+13439-1343F. Test `g6_echo_replaces_format_and_separator_characters` (U+202E, U+2028, U+2029, U+200B, U+FEFF). Commits `949ae268`, `8e79f962`.

Round 3 results: Acceptance exit 0 (`responses_request` 49 tests, `translate` 48). `cargo test -p zygo-proxy --locked` passes. `cargo clippy -p zygo-proxy --locked --all-targets -- -D warnings` is clean. `cargo test --workspace --locked --no-fail-fast` fails only on the 2 known `zygo --test guard_hook` tests under /private/tmp. Builds used `CARGO_INCREMENTAL=0`.
