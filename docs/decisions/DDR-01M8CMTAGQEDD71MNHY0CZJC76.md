+++
id = "DDR-01M8CMTAGQEDD71MNHY0CZJC76"
kind = "decision"
status = "open"
class = "none"
title = "Clean: a charset designation takes one character, never a line end"
unit = "phase3:FX-16"
round = 1
role = "code"
model = "unknown"
commit = "306106f3f9a57dbb3659a918964bc7b77d5ffc9c"
date = "2026-11-26"
depends_on = []
reverse_cost = "low"
+++

## Choice

A charset designation is `ESC (` or `ESC )` plus the one character after it. Clean removes the `ESC`, the `(` or `)` and that whole character (3 bytes for an ASCII character, 4 to 6 bytes for a non-ASCII character), so the cut stays on a UTF-8 boundary. Before a line end (`\n` or `\r`), another `ESC` or the end of the text, clean removes only the 2 bytes `ESC (` or `ESC )`; the line end stays, and the next `ESC` starts its own sequence. The code is the `Some(b'(' | b')')` arm of `esc_len` in `crates/zygo-results/src/shape/clean.rs`.

## Alternatives rejected

- Literal 3 bytes (the O§7.4 text before this record: "plus one character, 3 bytes in all"). Rejected: for a non-ASCII character after `ESC (` (for example `é`, 2 bytes, or `😀`, 4 bytes), a 3-byte cut ends inside the character, and the slice `&text[kept_from..]` (or `&text[kept_from..i]`) then panics on a byte index that is not a char boundary.
- Remove `ESC (` plus a following `\n` or `\r` as "the one character". Rejected: it removes a line end, so a line goes (or the `\r` of a `\r\n` that reuse keeps), against O-C9 and O§7.4 "Clean removes no line".

## Evidence

- Spec O§7.4 bullet 1, `docs/specs/2026-11-24-output-shaping-design.md:240` (the charset-designation clause, rewritten to this choice) and `:242` ("Clean removes no line").
- O-C9, same file `:54`: when a shaper is unsure, it keeps the text. O§15 item 2, same file `:544`: a form is fixed only toward hiding less, and the fix is recorded as a decision record.
- Change log, same file: the 2026-11-25 O§7.4 row (OS-12 review F4/F5) and the 2026-11-26 O§7.4 row (FX-16 review F2).
- Code: `crates/zygo-results/src/shape/clean.rs:56-69` (`esc_len`; the designation arm at `:64-69`), `:42` and `:52` (the slices `&text[kept_from..i]` and `&text[kept_from..]` that a cut inside a character would panic on).
- Findings: OS-12 review F4 (`ESC ( B` kept `B`; 62 `okB` lines in `fixtures/shaping/clean/cargo-test-color`) and F5 (`ESC ESC [31m` kept `[31m`), recorded in `docs/work/phase3/graph.md` (the OS-12 DONE row) and `docs/work/phase3/nodes/FX-16.md`.
- Tests: `crates/zygo-results/tests/shape_clean.rs` `charset_designation_is_removed`, `charset_designation_never_takes_a_line_end_or_an_esc`, `charset_designation_takes_one_whole_character` and `charset_designation_before_a_line_end_esc_or_text_end`.

## Cost to reverse

One match arm in `esc_len` and the tests named above, plus the O§7.4 clause and the re-blessed `fixtures/shaping/clean/cargo-test-color/view`. Only FX-16 changes this code; OS-15 calls `clean` and does not depend on the designation length.
