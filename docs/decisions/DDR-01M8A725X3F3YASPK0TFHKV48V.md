+++
id = "DDR-01M8A725X3F3YASPK0TFHKV48V"
kind = "decision"
status = "open"
class = "none"
title = "Clean: an OSC stops at a carriage return"
unit = "phase3:OS-12"
round = 2
role = "code"
model = "unknown"
commit = "6e1b93871c25291406265becc8ce223e2d498450"
date = "2026-11-25"
depends_on = []
reverse_cost = "low"
+++

## Choice

The OSC scan stops at `\r` as it stops at `\n`. An `ESC ]` whose body reaches a `\r` before `BEL` or `ESC \` has no terminator, so only `ESC ]` is removed and the `\r` stays. With `reuse = true` (O§7.3 text that the model may reuse), each `\r` therefore stays, as the round 1 doc comment already said ("an escape never takes `\n` or `\r`").

## Alternatives rejected

- Let an OSC run across `\r` (the round 1 code). Rejected: with `reuse = true`, `clean("a\x1b]0;t\rb\x07c\r\n", true)` gave `"ac\r\n"`. It deleted a `\r` and the text `b` in a mode that must change only escapes.
- Stop only when `reuse = true`. Rejected: then the two modes would remove different escapes, and the `\r` rule (O§7.4 bullet 2) already handles `\r` when `reuse = false`.

## Evidence

- Spec O§7.3 "Text that the model may reuse", `docs/specs/2026-11-24-output-shaping-design.md:231`: only ANSI escape sequences go, no `\r` change.
- O§7.4 lines 239-240; O-C9 line 54; O§15 item 2 line 531.
- Code: `crates/zygo-results/src/shape/clean.rs:96-106` (`osc_stop`, the `\r` arm at `:100`).
- Test: `crates/zygo-results/tests/shape_clean.rs:194` (the input above, with `reuse` true and false).

## Cost to reverse

One match arm in `osc_stop` and one test. Only OS-12 uses it.
