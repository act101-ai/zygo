+++
id = "DDR-01M8A726CCFEFSJDC2SS0KJ2G0"
kind = "decision"
status = "open"
class = "none"
title = "Clean: an OSC stops at the line end"
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

An escape never takes `\n` (plan Decision 2). An `ESC` before `\n` or `\r` removes only the `ESC`. An OSC body that reaches `\n` (or `\r`, see the carriage-return record) before `BEL` or `ESC \` has no terminator, so only `ESC ]` is removed. So clean removes no line, and the output line count equals the input line count.

## Alternatives rejected

- Let an OSC run to a `BEL` or `ESC \` on a later line (the "OSC to `BEL` or `ESC \`" form read alone). Rejected: it removes `\n`s and every line between, against O§7.4 bullet 3 "Clean removes no line" (line 241), and it hides text that was never part of a title or a link.
- Let an `ESC` take a following `\n` as "its next character". Rejected: it joins two lines, against the same bullet and against the acceptance "output line count equals input line count".

## Evidence

- Spec O§7.4, `docs/specs/2026-11-24-output-shaping-design.md:239` (escape forms) and the "Clean removes no line" bullet at line 241; O-C9 line 54; O§15 item 2 line 531.
- Node acceptance, `docs/work/phase3/nodes/OS-12.md:7`.
- Code: `crates/zygo-results/src/shape/clean.rs:60` (`ESC` before `\n` or `\r`), `:96-106` (`osc_stop` stops at `\n`).
- Tests: `crates/zygo-results/tests/shape_clean.rs:124` (`an_escape_never_takes_a_newline`); the `run` helper at `:8` checks the line count on every input.

## Cost to reverse

The match arms in `esc_len` and `osc_stop`, and two tests. Only OS-12 uses it. A reversal lets clean remove lines, which O§7.4 forbids.
