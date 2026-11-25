+++
id = "DDR-01M8A7264VP3Y3X177D8SRFK4A"
kind = "decision"
status = "open"
class = "none"
title = "Clean: a trailing carriage return overwrites nothing"
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

In `overwrite_cr`, a `\r` with nothing after it on the line overwrites nothing. Each line first loses its trailing `\r`s; then only the text after the last remaining `\r` stays. So `"progress 100%\r"` gives `"progress 100%"`, `"line one\r\r\nline two\r\n"` gives `"line one\nline two\n"`, and `"a\rb\r\n"` gives `"b\n"`.

## Alternatives rejected

- Keep the text after the last `\r`, even when it is empty (the round 1 code, the literal O§7.4 text). Rejected: a progress line that ends in `\r` (common in tools that redraw a line) became empty, and the final status text was lost. The fixtures and the review show this form to be wrong; O§15 item 2 lets a form be fixed toward hiding less.
- Drop every `\r` without overwrite. Rejected: the progress frames before the last `\r` would stay, and O§7.4 asks to remove them.

## Evidence

- Spec O§7.4 bullet 2, `docs/specs/2026-11-24-output-shaping-design.md:240`: "a `\r` overwrites, so only the text after the last `\r` is kept".
- O-C9 line 54; O§15 item 2 line 531.
- Code: `crates/zygo-results/src/shape/clean.rs:110-122` (`trim_end_matches` at `:118`).
- Test: `crates/zygo-results/tests/shape_clean.rs:205`.

## Cost to reverse

One line in `overwrite_cr` and one test. Only OS-12 uses it.
