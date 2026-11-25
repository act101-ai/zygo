+++
id = "DDR-01M8A725NWWETPA4N3PZEX8J5E"
kind = "decision"
status = "open"
class = "none"
title = "Clean: an OSC terminator must start within 4096 body bytes"
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

An OSC (`ESC ]` to `BEL` or `ESC \`) is removed whole only when its terminator starts within 4096 bytes after `ESC ]` (body length 4096 or less). When no terminator starts within that bound, the OSC counts as "with no terminator" and only `ESC ]` is removed; the body, and a terminator that comes later, stay as text. The bound is `OSC_MAX_BODY` in `crates/zygo-results/src/shape/clean.rs`.

## Alternatives rejected

- No bound (the round 1 code): an OSC of any length that ends somewhere on the line is removed. Rejected: the O§7.4 4 KiB rule had no effect, and a stray `ESC ]` could hide up to a full line of real text (for example 100,000 bytes before a late `BEL`). That hides more than O-C9 allows.
- Bound on the whole OSC length, terminator included. Rejected: it differs from the 4096/4097 body-length reading only by one or two bytes, and the body-length reading is what the review gave.

## Evidence

- Spec O§7.4, `docs/specs/2026-11-24-output-shaping-design.md:239`: "an OSC of more than 4 KiB with no terminator: remove only `ESC ]`".
- O-C9, same file line 54: when a shaper is unsure, it keeps the text. O§15 item 2, line 531: fix a form only toward hiding less.
- Code: `crates/zygo-results/src/shape/clean.rs:7` (`OSC_MAX_BODY`), `:70` (bound check).
- Tests: `crates/zygo-results/tests/shape_clean.rs:172` (body 4095 and 4096: removed whole), `:182` (body 4097, 5000, and a 100,000-byte `V` run: only `ESC ]` goes).

## Cost to reverse

One constant and one match arm in `clean.rs`, plus two tests. Only OS-12 uses this code now; OS-15 calls `clean` and does not depend on the bound.
