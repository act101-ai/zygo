# Decision records

Each file `DDR-<ULID>.md` in this directory is one delegated decision record: a choice an agent made under delegated authority that the operator has not yet ratified. The rules are in `docs/specs/2026-11-24-decision-records-design.md` D§6. zygo reads only names that match `DDR-<26 Crockford base32 characters>.md`; it ignores this file and every other name. A record is never deleted or renamed; it leaves the open set by a status change.

## Example

```markdown
+++
id = "DDR-01JDQ2Y7Z8K4M5N6P7Q8R9S0TV"
kind = "decision"
status = "open"
class = "none"
title = "Store the pane name in the member record, not in the session view"
unit = "phase-3:N12"
round = 2
role = "code"
model = "claude-sonnet-4-5"
commit = "6e7fe2f0c1d2e3f4a5b6c7d8e9f0011223344556"
date = "2026-11-24"
depends_on = ["DDR-01JDQ2W0000000000000000000"]
reverse_cost = "low"
+++

## Choice

One paragraph: what the agent chose.

## Alternatives rejected

Each alternative, and why it was rejected.

## Evidence

The file:line, test output or document text that the choice rests on.

## Cost to reverse

What a reversal changes, and which later work it touches.
```
