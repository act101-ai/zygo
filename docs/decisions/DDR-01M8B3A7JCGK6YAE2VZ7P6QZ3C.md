+++
id = "DDR-01M8B3A7JCGK6YAE2VZ7P6QZ3C"
kind = "accepted-limit"
status = "open"
class = "none"
title = "Responses drain tasks have no total-time or count limit beyond the per-read idle timeout"
unit = "phase4:RS-4"
round = 3
role = "code"
model = "claude-opus-5-5"
commit = "13d49e9314fb27f81b2acbf2494f6521319d1e4e"
date = "2026-11-25"
reverse_cost = "low"
+++

## Finding

RS-4 round 3 review H4. After the Responses encoder's terminal event, `next_chunk` ends the client body and moves the
`Flow` into a detached task (`crates/zygo-proxy/src/forward.rs:1429`, `drain` at `:1441`). The task reads the rest
of the upstream body through the tee until the body ends, a read error occurs or one read waits longer than
`UPSTREAM_READ_IDLE` (600 s, `crates/zygo-proxy/src/server.rs:35`, set as the reqwest `read_timeout` at `:141`).
Nothing limits the total time of one task or the number of tasks. The tasks are outside `MAX_CONNECTIONS`
(`crates/zygo-proxy/src/serve.rs:36`), because the client connection is released first. An upstream that keeps
sending after its terminal event, with gaps under 600 s, keeps a task and its upstream connection open for as long as
it sends.

## Choice

Keep the drain as it is: no total-time limit and no task count limit.

It is accepted now because:

- An Anthropic upstream closes the body right after `message_stop`, so on the Anthropic route the task reads at most
  the few bytes between the encoder's terminal event and the end of the body.
- A chat upstream never drains: a terminal or error event of the chat mapper sets `done`, and the upstream is read no
  further (`crates/zygo-proxy/src/forward.rs:1474`). `next_chunk` spawns no task when `done` is set.
- The usage row needs the final counts: Anthropic sends the final `output_tokens` in `message_delta`, which can come
  after an encoder failure (`encoder_failure_row_has_final_usage` in `crates/zygo-proxy/tests/responses_forward.rs`).
  A drain cut short writes a row with low counts.

## Alternatives rejected

- A total-time limit on each drain task: it cuts the final counts of a slow but valid upstream, and no measured
  upstream needs it.
- Counting drain tasks against `MAX_CONNECTIONS`, or a separate semaphore: it adds shared state on the response path
  for a case that no supported upstream produces.
- No drain (drop the `Flow` at the terminal event): the row loses the final `message_delta` counts (round 1 F4).

## Evidence

- `crates/zygo-proxy/src/forward.rs:1421-1446`: `next_chunk` and `drain`.
- `crates/zygo-proxy/src/forward.rs:1474`: the chat mapper's terminal or error event sets `done`.
- `crates/zygo-proxy/src/server.rs:35`, `:141`: the 600 s per-read idle timeout on upstream reads.
- `encoder_failure_row_has_final_usage` fails when `tokio::spawn(drain(f))` is replaced by `drop(f)` (RS-4 round 3
  disposition H3, `docs/work/phase4/nodes/RS-4.md`).
- K§6.4.3 stream obligation 6 (`docs/specs/2026-11-25-client-model-design.md`) states the drain.

Revisit when either trigger occurs:

- an upstream is seen that streams after its terminal event (Anthropic route), or
- Codex retry behaviour after `response.failed` is measured (RS-12): a client that retries at once while each failed
  stream's drain still runs multiplies the open upstream connections.

## Cost to reverse

Low: one node. A limit goes into `drain` in `crates/zygo-proxy/src/forward.rs` (a `tokio::time::timeout` around the
loop, or a semaphore permit moved into the task) with one test. K§6.4.3 stream obligation 6 gets one sentence. No
public contract or stored format changes.
