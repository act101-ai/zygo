# OS-8 — `zygo usage` prints the net figures, breakdown and ratio

**Spec:** `docs/specs/2026-11-24-output-shaping-design.md` O§6.6, O§6.2 (names), O§6.3 rule "Prices", O-C5, O§10 row "Config does not load in `zygo usage`", O§16 item 15 (`zygo usage` part)
**Files:** `crates/zygo/src/cmd/usage.rs`, `crates/zygo/tests/usage_doctor.rs`
**Consumes:** `Store::savings` (OS-7), `Savings` (OS-7), `SavingsKey` (OS-7), `Rates` (OS-6)
**Produces:** —
**Acceptance:** `test "$(cargo test -q -p zygo --test usage_doctor -- savings --list 2>/dev/null | grep -c ': test$')" -ge 6 && cargo test -p zygo --test usage_doctor` → every count holds (at least 6 named `savings…` in zygo `usage_doctor`; a count of 0 fails before any run), then all pass: the existing H§14 item 18 test is unchanged; on a fixture database the lines after `tokens trimmed (estimate)` equal the O§6.6 block line for line; a negative amount prints `≈-$`; unpriced requests add ` (<n> requests had no price)`; a config that does not load prints `estimated spend avoided: unknown (config does not load)` and every other line; a database built from the `97d8767` schema exits 0 and prints `bytes read back: 0` and `-` keys; a tool, command or model name with a control character passes through `esc`

## Plan
