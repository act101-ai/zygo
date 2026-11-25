# PC-8 — check-premises.sh: the name scan exempts the verbatim schemas; an indented table row is not a premise row

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.2.5 (the premise check by data), K§11 item 10 (the fixture scan)
**Files:** `scripts/check-premises.sh`, `scripts/test-check-premises.sh`
**Consumes:** `scripts/check-premises.sh` (PC-1), `scripts/test-check-premises.sh` (PC-1)
**Produces:** `scripts/check-premises.sh` with (a) the forbidden-name scan (N2_FORBIDDEN_NAMES) skipping the same verbatim upstream schema files the script already exempts elsewhere (today fixtures/claude-code/settings-schema.json contains the upstream example path /Users/alice/…, so N2_FORBIDDEN_NAMES=alice sh scripts/check-premises.sh exits 1 on line 22 of that file); (b) a line indented four or more spaces (or a tab) not read as a Results table row or header line, as Markdown renders it as a code block. `scripts/test-check-premises.sh` gains one case for each, failing first.
**Acceptance:** `sh scripts/test-check-premises.sh && N2_FORBIDDEN_NAMES=alice sh scripts/check-premises.sh && sh scripts/check-premises.sh` → every command exits 0.

## Planning notes

- Found by the PC-1 round 3 implementer (2026-11-25); the behavior predates round 3.

## Plan

