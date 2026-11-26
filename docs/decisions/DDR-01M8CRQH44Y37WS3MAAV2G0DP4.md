+++
id = "DDR-01M8CRQH44Y37WS3MAAV2G0DP4"
kind = "decision"
status = "open"
class = "none"
title = "Clean: an nF escape takes 1 to 4 intermediates and one final byte"
unit = "phase3:FX-21"
round = 1
role = "code"
model = "unknown"
commit = "569e61a97fb3a736407aad21a995b33f69873a52"
date = "2026-11-26"
depends_on = []
reverse_cost = "low"
+++

## Choice

Clean removes an ECMA-35 nF escape whole: `ESC`, then 1 to 4 intermediate bytes `0x20-0x2F`, then one final byte `0x30-0x7E` (run + 2 bytes). Every charset designation is of this form: `ESC ( B`, `ESC ) 0`, `ESC * B`, `ESC + B`, `ESC - A`, `ESC $ B`, `ESC $ ( B`, `ESC ( SP @`, `ESC $ ( SP @`, and also `ESC % G`. When the intermediates are not followed by a final byte (a line end `\n` or `\r`, `ESC`, the first byte of `U+009B`, another non-ASCII byte, `0x7F`, a control byte, or the end of the text), or the run is longer than 4, clean removes only the `ESC` (1 byte) and keeps the intermediates and what follows as text (O-C9); a following `ESC` or `U+009B` then starts its own sequence. The bound is 4 because the longest ECMA-35 designation, a multi-byte dynamically redefinable set (`ESC $ ( SP F`), has 3 intermediates; 4 keeps one spare, and a longer run of spaces and punctuation is more likely text, which an unbounded run would remove with the next letter. Every cut byte is ASCII, so a cut ends on a UTF-8 boundary, and no cut takes `\n` or `\r`. The rule replaces the FX-16 `ESC (` / `ESC )` arm (DDR-01M8CMTAGQEDD71MNHY0CZJC76): `(` and `)` are intermediate bytes. The lead decided the rule; this record is the O§15 item 2 record of the form change. The code is the `Some(0x20..=0x2F)` arm of `esc_len` and the constant `NF_MAX_INTERMEDIATES` in `crates/zygo-results/src/shape/clean.rs`.

## Alternatives rejected

- Keep the FX-16 rule (`ESC (` / `ESC )` plus one final byte; every other form `ESC` plus its next character). Rejected: `ESC *`, `ESC +`, `ESC $ (` and intermediates before the final byte leave residue (`aBb`, `a(Bb`, ` @x`), which is escape bytes shown as text.
- Keep the FX-16 2-byte cut (`ESC (` / `ESC )`) when no final byte follows, and remove `ESC` plus the run for the other intermediates. Rejected: without a final byte the bytes after `ESC` are not recognized as an escape, so removing them removes text clean does not recognize, against O-C9 "when a shaper is unsure, it keeps"; one rule for every intermediate is simpler and hides less.
- No bound on the intermediate run. Rejected: `ESC` followed by a long run of spaces or punctuation and then a letter would lose all of them and the letter, which are most likely text.
- Bound 3 (exactly the longest ECMA-35 designation). Rejected in favor of 4 (the lead's example bound): one spare intermediate costs little, and a private or later nF form with one more intermediate is still removed whole.

## Evidence

- Spec O§7.4 bullet 1, `docs/specs/2026-11-24-output-shaping-design.md:240` (rewritten to this rule) and `:242` ("Clean removes no line"); O-C9, `:54` ("When a shaper is unsure, it keeps the line or gives \"no change\""); O§15 item 2, `:544` (a form is fixed only toward hiding less, and the fix is recorded as a decision record); the 2026-11-26 O§7.4 FX-21 change-log row.
- ECMA-35 (ISO/IEC 2022): an nF escape sequence is `ESC`, one or more intermediate bytes `02/00-02/15`, and one final byte `03/00-07/14`; a designation is an nF escape.
- Probe at FX-16 (`docs/work/phase3/nodes/FX-21.md` Spec line): `a ESC * B b` gave `aBb`, `a ESC + B b` gave `aBb`, `a ESC $ ( B b` gave `a(Bb`, `ESC ( SP @x` gave ` @x`. FX-16 review round 1 F4 and round 3 F2 in `docs/work/phase3/nodes/FX-16.md`.
- Code: `crates/zygo-results/src/shape/clean.rs:58` (`NF_MAX_INTERMEDIATES`), `:63-82` (`esc_len`, the nF arm at `:70-81`), `:42` and `:52` (the slices that a cut inside a character would panic on; every cut here ends on an ASCII byte).
- Tests: `crates/zygo-results/tests/shape_clean.rs` `nf_escape_is_removed_whole`, `nf_intermediates_with_no_final_byte_lose_only_esc`, `nf_run_over_the_bound_loses_only_esc`, and the FX-16 tests whose expectations FX-21 changed (`charset_designation_never_takes_a_line_end_or_an_esc`, `charset_designation_takes_only_a_final_byte`, `charset_designation_final_byte_range_ends`, `charset_designation_before_a_line_end_esc_or_text_end`). Failing-first run: 25 passed, 7 failed; after the change 32 passed.
- Fixtures: the only nF escape in `fixtures` and `crates` is `ESC ( B` in `fixtures/shaping/clean/cargo-test-color/stdout` (122 times), removed whole before and after this change.
- Commit that carries out this decision: ba8be534 (code and tests).

## Cost to reverse

One match arm and one constant in `esc_len`, the three `nf_*` tests and the four changed FX-16 expectations in `crates/zygo-results/tests/shape_clean.rs`, and the O§7.4 bullet 1 clause and change-log row. No blessed `view` depends on it. Only FX-21 changes this code.
