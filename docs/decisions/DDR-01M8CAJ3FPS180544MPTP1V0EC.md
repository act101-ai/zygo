+++
id = "DDR-01M8CAJ3FPS180544MPTP1V0EC"
kind = "accepted-limit"
status = "open"
class = "none"
title = "On Anthropic, a tool_use closed before a refusal is sent to a Responses client before the stop reason is known"
unit = "phase4:RS-12"
round = 3
role = "code"
model = "claude-opus-5-5"
commit = "7ca569345f2fd8d69efd63fa103343f5d11e2461"
date = "2026-11-26"
reverse_cost = "low"
+++

## Finding

RS-12 round 3 review H3 (important, inferred). On an Anthropic upstream, the Responses encoder sends a call's done
events (`response.function_call_arguments.done`, `response.output_item.done`) at the call's `content_block_stop`
(`crates/zygo-proxy/src/translate/responses_client.rs:1297`, `close` at `:1438`). The stop reason comes later, in
`message_delta`, and the ending is chosen at `message_stop` (`:1312`, `finished` at `:886`). If Anthropic cuts a
`tool_use` for a refusal and still sends its `content_block_stop` before `message_delta` `refusal`, the client gets
that call whole, with cut arguments, before the stream ends in `response.failed` (ending `rfd`). Nothing measures
whether Anthropic sends a cut `tool_use` with `content_block_stop` before a refusal.

A call still open at the refusal is dropped (RS-12 round 2 G1), and on chat the held calls are dropped at
`content_filter` (RS-12 round 3 H1, H2). This gap is only the Anthropic call closed before the refusal.

## Choice

Keep sending each call's done events at its `content_block_stop`. K§6.4.3 stream obligation 5 says that calls closed
before the refusal are sent as received, and that whether Anthropic sends a cut `tool_use` with `content_block_stop`
before a refusal is not known (`docs/specs/2026-11-25-client-model-design.md`).

It is accepted now because:

- To fix it, the encoder must hold every call's done events until `message_delta`. That delays the tool start on
  every normal turn, for a case that was never observed.
- The stream then ends in `rfd` (`response.failed` `invalid_prompt`). By inference from PC18 `xt` (a `response.failed`
  after a call; no tool ran), Codex does not run a tool of a failed response.

## Alternatives rejected

- Hold each call's done events until `message_delta`, then send them for any stop but `refusal`: delays tool start on
  every turn with a call, for an unobserved case.
- Hold only the last call before `message_delta`: Anthropic can cut any call that is last before the stop, so it
  delays the same turns and covers the same case.

## Evidence

- `crates/zygo-proxy/src/translate/responses_client.rs:1297`: `content_block_stop` closes the open block and sends
  its done events.
- `crates/zygo-proxy/src/translate/responses_client.rs:1312`: at `message_stop` only a call still open at a refusal
  is dropped; `finished` (`:886`) then gives `rfd` because a call item was opened.
- `rfd_refusal_mid_tool_use_drops_the_open_call` and `rfd_refusal_after_complete_tool_use`
  (`crates/zygo-proxy/tests/responses_stream.rs`): an open call is dropped; a closed call stays as sent.
- PC18 `xt` (`fixtures/codex/encoder/result.txt`): after a `response.failed` that followed a half-open call, Codex ran
  no tool. This is for an upstream error, not a refusal.

Revisit when either trigger occurs:

- an Anthropic refusal is observed in the middle of a `tool_use`, or
- a measured Codex run shows Codex running a tool from a failed response.

## Cost to reverse

Low: one node. The encoder holds each closed call's done events until `message_delta` (bounded by `HOLD_MAX`, so
obligation 9 changes too), sends them for any stop but `refusal`, and drops them at a refusal. Tests change in
`responses_stream.rs` and `responses_forward.rs`; K§6.4.3 obligations 2, 5 and 9 change. No stored format changes.
