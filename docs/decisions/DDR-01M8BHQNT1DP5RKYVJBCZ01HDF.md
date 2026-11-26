+++
id = "DDR-01M8BHQNT1DP5RKYVJBCZ01HDF"
kind = "accepted-limit"
status = "open"
class = "none"
title = "The provider key can reach the client in two residual forms: a \\u-escaped echo, or a key split across stream deltas"
unit = "phase4:RS-14"
round = 3
role = "code"
model = "claude-opus-5-5"
commit = "d275d739b9c85771eb9fbd822a6b87b76b96b23d"
date = "2026-11-26"
reverse_cost = "low"
+++

## Finding

RS-14 round 3 review H2. K§6.4.4 (4) said: "The key's characters are never escaped (5), so a translated body made from
these bytes holds no key either." That is false in two cases. The replacement (`KeyScrub`,
`crates/zygo-proxy/src/forward.rs:1254`, applied to each 2xx body as it is read at `:570`) finds only the key's exact
bytes. Two forms that do not hold those bytes become the key in a later step:

- **A `\u`-escaped echo.** The key written with a JSON `\u` escape (for example `test-key-…` for `test-key-…`) in
  a 2xx body passes the replacement. A translator (`translate_response`, the stream mapper, the Responses encoder) or
  the streamed restore filter (`RestoreFilter`, `crates/zygo-proxy/src/translate/schema.rs:571`, which parses a held
  block's text at `:133`) decodes it and writes the key again in its exact bytes into the body sent to the client.
- **A key split across stream deltas.** Each delta holds a part of the key, with event bytes between the parts, so no
  run of upstream bytes holds the key. The Responses encoder joins the deltas of a block into its `*.done` events
  (`response.output_text.done` at `crates/zygo-proxy/src/translate/responses_client.rs:1378`,
  `response.function_call_arguments.done` at `:1404`), and the restore filter joins a held block's deltas into new
  deltas (`schema.rs:571-580`). The joined text holds the key.

The one untranslated path that parses before it replaces, an untranslated 2xx JSON body with injected nulls
(`forward.rs:566`, `restored_body` at `:1344`), replaces after the restore, so an escaped key is found there.

## Choice

Keep one replacement on the exact key bytes of the upstream body. K§6.4.4 (4) and (5) now name both residual forms
(`docs/specs/2026-11-25-client-model-design.md`).

It is accepted now because:

- Only an upstream echo or the model's own output can make either form. The proxy never writes the key into a body;
  a provider echoes it in an error, and a model can write it only when it is in the model's input.
- The exact key bytes are always removed, on every path: headers (dropped), encoded bodies (refused), 2xx and non-2xx
  bodies, translated or not, and upstream error messages (`error_data`, `forward.rs:1518`, which parses the message
  first, so an escaped key in an error event is found).
- Real encoders do not escape `[A-Za-z0-9_-]`, the alphabet of the configured providers' keys (Anthropic `sk-ant-…`,
  OpenAI `sk-…`). A provider's JSON encoder writes such a key in its exact bytes; a `\u`-escaped echo needs an encoder
  that escapes plain ASCII letters on purpose.
- A split across deltas needs the model to emit the key, which needs the key in its input. `zygo` sends the key only
  in the request header (`outbound_headers`, `forward.rs:74`), never in the body.

## Alternatives rejected

- A second replacement on decoded and joined text in every translator and in the restore filter now: it adds a scan
  of each delta, each `*.done` text and each rebuilt block, in four places, for forms that no supported provider was
  seen to send.
- Decode `\u` escapes in the byte replacement (`KeyScrub`): the replacement then reads JSON string syntax, which it
  does not do now by design (it works whatever the framing), and it would still miss a key split across deltas.
- Refuse a stream whose joined text holds the key: the key is then found only after parts of it were sent in deltas.

## Evidence

- `crates/zygo-proxy/src/forward.rs:570`: each 2xx body (but the restore path) goes through `key_scrubbed` before any
  other step.
- `crates/zygo-proxy/src/forward.rs:1254-1293`: `KeyScrub`, an exact byte match, with a held tail for a key split
  across reads (not across deltas).
- `crates/zygo-proxy/src/translate/responses_client.rs:1378`, `:1404`: the encoder's `*.done` events carry the joined
  block text.
- `crates/zygo-proxy/src/translate/schema.rs:133`, `:571-580`: the restore filter parses and joins a held block's text.
- `injected_nulls_restored_and_key_replaced` (`crates/zygo-proxy/tests/passthrough_key.rs`): a `\u`-escaped key in a
  whole body with injected nulls is replaced; it fails when the replacement runs before the restore (RS-14 round 3
  H1).
- `trimmed_key_escaped_in_translated_error_event_is_replaced` (`crates/zygo-proxy/tests/passthrough.rs`): a
  `\u`-escaped key in an error event of a translated stream is replaced (RS-14 round 3 H3).

Revisit when either trigger occurs:

- an upstream echo of the key is observed in an escaped form or split across deltas, or
- a client is found that logs the joined text of the `*.done` events.

The fix then: a second replacement of the key on the decoded and joined text, in the Responses encoder (each `*.done`
text and each delta) and in the restore filter (each rebuilt block).

## Cost to reverse

Low: one node. `scrub` (`crates/zygo-proxy/src/translate/mod.rs`) is applied to the joined text in the encoder's
`close` and to the rebuilt text in `RestoreFilter::rebuilt`, with the trimmed key passed to both, and one test per
form. K§6.4.4 (5) loses its residual-forms sentence. No public contract or stored format changes.
