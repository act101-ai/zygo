+++
id = "DDR-01M8CE0JMGSVYFYKASBVRHY5VX"
kind = "accepted-limit"
status = "open"
class = "none"
title = "A Responses function_call done without status completed is sent when another item starts, so a later content_filter cannot drop it"
unit = "phase4:RS-16"
round = 3
role = "code"
model = "claude-opus-5-5"
commit = "405b938264ecef39b218cad6ecc91ca8cb2eaedb"
date = "2026-11-26"
reverse_cost = "low"
+++

## Finding

RS-16 round 3 review H1 (important). In the mapper's Responses arm (`crates/zygo-proxy/src/translate/sse.rs`), a
`function_call` whose `response.output_item.done` has a `status` other than `completed`, or none, stays held
(`responses`, the `Some("function_call")` arm at `:319`). When another output item starts, `open` (`:444`) calls
`close` (`:488`), which sends the held call whole to the Messages client and puts its `output_index` in `sent`
(`:497`). If the upstream then ends with `response.incomplete` reason `content_filter`, `drop_held` (`:505`) finds no
held call, so the call with possibly cut arguments stays sent. The JSON path, `responses::response`
(`crates/zygo-proxy/src/translate/responses.rs:151`), drops every `function_call` that is not `completed` at
`content_filter`. For this event sequence the stream and JSON paths differ.

Only a non-conforming upstream sends this sequence. OpenAI sends `status: completed` on each `function_call` done,
and sends no item after a cut.

## Choice

Keep sending a held call when another output item starts. K§6.4.3 stream obligation 5 states the gap
(`docs/specs/2026-11-25-client-model-design.md`).

It is accepted now because:

- Only a non-conforming upstream is affected: a conforming one never sends a not-completed done followed by more items.
- To fix it, the mapper must hold the call past later items. The later items then reach the Messages client first, so
  the block indexes change order against the upstream output order.

## Alternatives rejected

- Hold a not-completed call past later items, and send it at the end: the Messages client gets blocks out of the
  upstream order, for a case that a conforming upstream does not make.
- Drop a not-completed call when another item starts: it drops a call that a normal end would send, and the JSON path
  keeps such a call at every end but `content_filter`.

## Evidence

- `crates/zygo-proxy/src/translate/sse.rs:319`: a done that is not `completed` does not close the held block.
- `crates/zygo-proxy/src/translate/sse.rs:444`, `:488`, `:497`: `open` closes the held block, `close` sends it and
  records it in `sent`.
- `crates/zygo-proxy/src/translate/sse.rs:505`, `:520`: at a refusal `end` calls `drop_held`, which acts only on a held
  block.
- `crates/zygo-proxy/src/translate/responses.rs:151`: the JSON path drops each not-completed call at `content_filter`.
- `responses_call_done_incomplete_then_content_filter_is_dropped` and `responses_late_done_and_delta_for_sent_call_are_ignored`
  (`crates/zygo-proxy/tests/refusal_calls.rs`): a not-completed call still held at `content_filter` is dropped; a call
  sent when another item starts stays sent.

Revisit when this trigger occurs:

- an OpenAI-compatible upstream is observed sending a `function_call` done whose `status` is not `completed`, followed
  by more output items.

## Cost to reverse

Low: one node. The mapper holds a not-completed call past later items (or drops it when another item starts), with a
test for the sequence; K§6.4.3 stream obligations 5 and 9 change. No stored format changes.
