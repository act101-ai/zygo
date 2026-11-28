+++
id = "DDR-01M8BACPSY0WNJ0DK83YN82PAC"
kind = "accepted-limit"
status = "open"
class = "none"
title = "A streamed JSON 2xx body becomes Messages SSE events at a cost per block and per 64 KiB piece, up to about 59 x COPY_MAX"
unit = "phase4:RS-13"
round = 3
role = "code"
model = "claude-opus-5-5"
commit = "7c8d1f3a90db4a4d387a7478788ac2d1c33eba0a"
date = "2026-11-25"
reverse_cost = "low"
+++

## Finding

RS-13 round 3 review H1. When a client asks to stream and the upstream answers 2xx with a JSON body, the proxy reads
the body whole within `COPY_MAX` (4 MiB, `crates/zygo-proxy/src/tee.rs:23`; `crates/zygo-proxy/src/forward.rs:1096-1102`)
and turns it into Messages SSE events in one step: `message_events(&v)`, joined into one chunk
(`crates/zygo-proxy/src/forward.rs:566`). `message_events` (`crates/zygo-proxy/src/translate/sse.rs:444`) writes for
each content block a `content_block_start`, its deltas and a `content_block_stop`, each with its own `event:` line,
JSON envelope and `index`. That text costs a fixed number of bytes per block beyond the block's own content (its JSON
when sent whole; else its text, thinking, signature, id, name or input):

- a block of another type (sent whole in its start, no delta), for example `{}`: 163 bytes + 2 x the digits of the index;
- a text block: 302 bytes + 3 x the digits, and 114 bytes + the digits for each further 64 KiB piece of its text
  (since RS-15);
- a thinking block: 333 bytes + 3 x the digits, and 122 bytes + the digits for each further 64 KiB piece of its
  thinking (since RS-15), and 124 + the digits more with a signature;
- a tool_use block: 337 bytes + 3 x the digits, and 128 bytes + the digits for each further 64 KiB piece of its input.

So the event bytes are about the block content (at most 2 x for a tool input, which is escaped once more) plus that
overhead times the number of blocks and pieces, and are not bounded by a small multiple of `COPY_MAX`. K§6.4.3 stream obligation
9 said "at most about 2 x `COPY_MAX`" with a peak of "about 3 x `HOLD_MAX`"; that was false. Per byte of body, the
reviewer measured over 100,000 blocks: `{}` blocks 58x, empty text blocks 12x, empty tool_use blocks 7.3x. For a body
of `COPY_MAX`, from the format strings: 1,398,101 `{}` blocks give 248,037,859 event bytes (59.1 x `COPY_MAX`, about
237 MiB); empty text blocks 12.2 x; empty tool_use blocks 7.2 x.

## Choice

Keep `message_events` as it is: the events of the whole body are built at once, with no cap on the number of blocks.
K§6.4.3 stream obligation 9 now states the true bound for this path.

It is accepted now because:

- The upstream is a provider that the user configured. A real model answer has a few blocks; the large factors need
  hundreds of thousands of empty blocks, which no supported provider sends.
- The path is only a JSON 2xx answer to a streamed request. Every SSE upstream goes through the bounded line path of
  obligation 9.
- The memory is held for one step only: the chunk is sent to the tee and the encoder, and freed when it is sent
  (`futures_util::stream::once`, `crates/zygo-proxy/src/forward.rs:571`). It does not grow with the time of the stream.
- The behaviour predates RS-13: `message_events` built the whole event list before RS-13 (since zygo 0.1.0, commit
  `da682dd8`). RS-13 changed how a tool_use input is cut, which adds 128 bytes + the digits of the index per further
  64 KiB piece. RS-15 cuts text and thinking the same way (K§6.4.3 stream obligation 9), which adds 114 bytes + the
  digits per further 64 KiB piece of text and 122 bytes + the digits per further piece of thinking. These piece costs
  are small next to 64 KiB of content, so the worst case stays the `{}` blocks: about 59 x `COPY_MAX`.

## Alternatives rejected

- Build the events lazily (an iterator of events, sent as a stream of chunks): it changes the one-chunk body of the
  JSON branch and its tests for an input that no configured provider sends.
- Cap the number of blocks (for example, fail with `502` over a limit): it adds a new error to a valid answer, and no
  limit is measured yet.
- Bound the event bytes by `COPY_MAX` and fail over it: the same new error, and a large valid tool input near
  `COPY_MAX` (2 x after the second escape) would fail (`message_events_tool_input_near_copy_max_completes` in
  `crates/zygo-proxy/tests/translate.rs`).

## Evidence

- `crates/zygo-proxy/src/translate/sse.rs:444-472`: `message_events`, one start, the deltas and one stop per block.
- `crates/zygo-proxy/src/translate/sse.rs:19-21`: `SseEvent::to_bytes`, `event: <name>\ndata: <compact JSON>\n\n`.
- RS-15 (at commit `3a48877a`): `deltas` and `block_deltas` (`crates/zygo-proxy/src/translate/sse.rs:481-493`) cut a
  text or thinking block into pieces of at most `INPUT_DELTA_MAX` (64 KiB); each piece is one `content_block_delta`.
  The 114 + d and 122 + d piece costs come from that event's format string.
- `crates/zygo-proxy/src/forward.rs:563-571`: the streamed JSON branch builds all events into one chunk.
- The per-block counts above come from those format strings; the reviewer's 58x, 12x and 7.3x come from 100,000-block
  bodies (RS-13 round 3 review H1).
- K§6.4.3 stream obligation 9 (`docs/specs/2026-11-25-client-model-design.md`), JSON body path, states this bound.

Revisit when either trigger occurs:

- an upstream that is not under the user's control can answer a streamed request (a shared or third-party
  endpoint), or
- a memory incident on this path is measured.

The fix then: build the events lazily, or cap the number of blocks.

## Cost to reverse

Low: one node. `message_events` returns an iterator, or the JSON branch in `crates/zygo-proxy/src/forward.rs` checks
the block count before it builds the events, with one test. K§6.4.3 stream obligation 9 gets one sentence. No public
contract or stored format changes.
