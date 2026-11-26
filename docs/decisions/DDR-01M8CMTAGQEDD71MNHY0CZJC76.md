+++
id = "DDR-01M8CMTAGQEDD71MNHY0CZJC76"
kind = "decision"
status = "open"
class = "none"
title = "Clean: a charset designation takes only a final byte 0x30-0x7E"
unit = "phase3:FX-16"
round = 1
role = "code"
model = "unknown"
commit = "b328900841fd66ed2de02682cc3e2b6d61459a5c"
date = "2026-11-26"
depends_on = []
reverse_cost = "low"
+++

## Choice

A charset designation is `ESC (` or `ESC )` plus one final byte in `0x30-0x7E` (the ECMA-35 designation final bytes), 3 bytes in all. When the byte after `ESC (` or `ESC )` is not such a byte, clean removes only the 2 bytes `ESC (` or `ESC )` and keeps what follows: a non-ASCII character, `U+009B` (which then starts its own 8-bit CSI), a byte below `0x30` such as a space, a line end (`\n` or `\r`), an `ESC` (which then starts its own sequence) or the end of the text. A cut of 2 or 3 bytes ends on an ASCII byte, so it stays on a UTF-8 boundary. The code is the `Some(b'(' | b')')` arm of `esc_len` in `crates/zygo-results/src/shape/clean.rs`. A lone `ESC` is removed alone at the end of the text, before a line end and before another `ESC` (the `None | Some(b'\n' | b'\r' | &ESC) => 1` arm of the same function).

## Alternatives rejected

- Literal "plus one character": remove `ESC (` and the whole character after it, whatever it is (the rule of this record before review round 2). Rejected: a non-ASCII character after `ESC (` (for example `é` or `😀`) is real text, not a designation final byte, and that rule removed it; a cut of a fixed 3 bytes would instead end inside such a character and split its UTF-8, and the slice `&text[kept_from..]` would panic. Both remove text that clean does not recognize, against O-C9 "when a shaper is unsure, it keeps".
- Take the line end as "the one character" after `ESC (` (`\n` or `\r`). Rejected: it removes a line end, so a line goes (or the `\r` of a `\r\n` that reuse keeps), against O§7.4 "Clean removes no line".

## Evidence

- Spec O§7.4 bullet 1, `docs/specs/2026-11-24-output-shaping-design.md:240` (the charset-designation clause and the lone `ESC` sentence, rewritten to this choice) and `:242` ("Clean removes no line").
- O-C9, same file `:54`: "When a shaper is unsure, it keeps the line or gives \"no change\"". O§15 item 2, same file `:544`: a form is fixed only toward hiding less, and the fix is recorded as a decision record.
- ECMA-35 (ISO/IEC 2022): a designation escape sequence ends with one final byte in `0x30-0x7E`; a byte outside that range is not part of the sequence.
- Change log, same file: the 2026-11-25 O§7.4 row (OS-12 review F4/F5) and the 2026-11-26 O§7.4 row (FX-16 review F2, review round 2 F1/F5/F7).
- Code: `crates/zygo-results/src/shape/clean.rs:59-86` (`esc_len`; the lone `ESC` arm at `:62`, the designation arm at `:65-68`), `:42` and `:52` (the slices `&text[kept_from..i]` and `&text[kept_from..]`, which a cut inside a character would panic on).
- Findings: OS-12 review F4 (`ESC ( B` kept `B`; 62 `okB` lines in `fixtures/shaping/clean/cargo-test-color`) and F5 (`ESC ESC [31m` kept `[31m`), recorded in `docs/work/phase3/graph.md` (the OS-12 DONE row); FX-16 review round 2 F1/F5 (the "one whole character" rule removed real text), recorded in `docs/work/phase3/nodes/FX-16.md`.
- Tests: `crates/zygo-results/tests/shape_clean.rs` `charset_designation_is_removed`, `charset_designation_never_takes_a_line_end_or_an_esc`, `charset_designation_takes_only_a_final_byte` and `charset_designation_before_a_line_end_esc_or_text_end`.

## Cost to reverse

One match arm in `esc_len` and the tests named above, plus the O§7.4 clause and the blessed `fixtures/shaping/clean/cargo-test-color/view`. Only FX-16 changes this code; OS-15 calls `clean` and does not depend on the designation length.
