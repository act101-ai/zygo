# SX-8 — `docs/live-checks.md` section P3-15 (the operator's live check)

**Spec:** docs/specs/2026-11-24-session-text-design.md §12.2, §16 row `docs/live-checks.md`
**Files:** `docs/live-checks.md`
**Consumes:** —
**Produces:** `docs/live-checks.md#P3-15`
**Acceptance:** `N2_FORBIDDEN_NAMES="$(id -un)" sh scripts/check-premises.sh` → exit 0 (the script scans `docs/live-checks.md` for the account name read at run time; nothing personal is written in the repository). Review: steps 1-8 and "Reading the result" of T§12.2 are in the `## P3-15` section without change in meaning (or the section points to T§12.2 for them).

## Plan

Docs only. One file changes: `docs/live-checks.md`. No code, no test file, no change to `docs/work/phase3/graph.md`, the spec, `docs/premises.md` or `scripts/check-premises.sh` (T§12.1 items 1-5 belong to SX-10, after the operator runs the check in SX-9).

"T§n" below is a section of `docs/specs/2026-11-24-session-text-design.md` (at `768bf48`). T§12.2 is the block from the line after the heading `### 12.2 Live check P3-15 (operator, by hand)` (line 267) up to the line before `## 13. Base-spec changes` (line 377): spec lines 269-375.

**Decision: copy, not pointer.** The section holds the full text of T§12.2 (introduction, steps 1-8 with their code blocks, the OpenCode note after step 8, and "Reading the result"). Reason: the operator runs the check from `docs/live-checks.md`, as for every other section there, and SX-9 consumes `docs/live-checks.md#P3-15`. T§12.2 stays the authority; the section says so. A trial copy of spec lines 269-375 into a scratch copy of the repository passed `N2_FORBIDDEN_NAMES="$(id -un)" sh scripts/check-premises.sh` with exit 0 (2026-11-24), so the verbatim text holds no forbidden pattern.

**Interfaces.** Consumes: none. Produces: `docs/live-checks.md#P3-15`, a section whose heading line starts with `## P3-15 `; SX-9 (`docs/work/phase3/nodes/SX-9.md`) reads its steps 6-7 output paths `fixtures/claude-code/live/st-hooks.jsonl`, `fixtures/claude-code/live/st-evidence.txt`, `fixtures/opencode/live/st-transform.jsonl` and its "Reading the result".

- [x] 1. In `docs/live-checks.md`, line 3 (the intro sentence that lists each section's spec), insert before its final period, after ``W§17 (node P2)``, this text:
  > ; `docs/specs/2026-11-24-session-text-design.md` T§12.2 (P3-15)

  Check: ``grep -c 'T§12.2 (P3-15)' docs/live-checks.md`` prints `1`.
- [x] 2. Insert a new section between the end of `## FIX-21 (no Claude attribution)` and the line `## Results`, with one blank line before `## Results`. Heading: `## P3-15 (session text after a compaction and in subagents)`. First paragraph, in the style of the other sections (node, then spec), exactly:
  > Node SX-8; `docs/specs/2026-11-24-session-text-design.md` T§12.2 (ST1, ST2, ST3, ST4 of T§12.1). The text below is a copy of T§12.2; T§12.2 is the authority, and a change there is copied here. Results go to the `docs/premises.md` "Results" table (rows ST1 to ST4) as T§12.1 "How a result enters the repository" says; this file gets no row.

  Check: `grep -n '^## ' docs/live-checks.md` lists `## P3-15 (session text after a compaction and in subagents)` directly after `## FIX-21 (no Claude attribution)` and directly before `## Results`.
- [x] 3. Under that paragraph, paste spec lines 269-375 unchanged, with exactly these three edits of form (no edit of meaning): (a) step 3, `[Subagents]` becomes `https://code.claude.com/docs/en/sub-agents` (the spec defines the label at its line 11; this file has no such label); (b) "Reading the result", `(the lead, T§12.1 "How a result enters the repository")` becomes `(the lead, `docs/specs/2026-11-24-session-text-design.md` T§12.1 "How a result enters the repository")`; (c) its last bullet, `in this spec` becomes `in the session-text spec`. The headings `**Claude Code (ST1, ST2)**`, `**OpenCode (ST3, ST4)**`, `**Capture**` and `**Reading the result**` stay bold lines, not `###` headings, as in T§12.2. Check: `diff <(sed -n '269,375p' docs/specs/2026-11-24-session-text-design.md) <(awk '/^## P3-15 /{f=1;next} /^## Results/{f=0} f' docs/live-checks.md)` shows only the added lines (the blank line after the heading, the first paragraph, the blank line after it, and the blank line before `## Results`) and the three changed lines of edits (a), (b), (c); every step 1-8 code block is byte-equal.
- [x] 4. Check the section holds no personal data: no personal name, account name, home path or e-mail address. Check: `awk '/^## P3-15 /{f=1;next} /^## Results/{f=0} f' docs/live-checks.md | grep -nE "/Users/|/home/|@[A-Za-z0-9-]+\.[A-Za-z]{2,}|$(id -un)"` prints nothing (exit 1). `~/.claude`, `~/.claude.json`, `$S` and `<provider>/<MAIN>` are generic and allowed.
- [x] 5. Verification: `N2_FORBIDDEN_NAMES="$(id -un)" sh scripts/check-premises.sh` exits 0 and prints `check-premises: ok (P1 P2 P3 P4 P5 P6a P6b Q1 Q2 Q3 Q4 Q5 Q6 Q7 PW6 PW11 PW15 PW1 PW2 PW3 PW4 PW5 PW7 PW9 PW10 PW16)` (the scan covers `docs/live-checks.md` through `SCOPE`, `scripts/check-premises.sh`; the name scan reads the account name at run time).
- [x] 6. Check only one file changed: `git diff --name-only origin/phase-3` prints exactly two paths: `docs/live-checks.md` and `docs/work/phase3/nodes/SX-8.md` (this plan and its checkboxes).
- [x] 7. Commit `docs/live-checks.md` alone, message `SX-8: live-checks section P3-15 (T§12.2)`, with the trailers `Zygo-Session:` (followed by the implementer's own session id) and `Zygo-Work: phase3:SX-8`, each on its own line; no Claude attribution.

### Review round 1

- F1 fixed: the P3-15 intro of `docs/live-checks.md` sends results to the `docs/premises.md` Results table (ST1 to ST4) and a dated row to this file's `## Results` table.
- F2 fixed: step 1 uses `mktemp -d "${TMPDIR:-/tmp}/p315.XXXXXX"` (works on GNU and BSD); the step 8 `*p315*` and `"p315" in k` filters still match, in T§12.2 and the copy.
- F3 fixed: step 7 also sends the MAIN and SUB ids of step 4 and whether the subagent's answer was shown in step 5, in T§12.2 and the copy.
- F4 fixed: step 8 says to delete `~/.claude.json.bak-p315` once Claude Code starts normally, in T§12.2 and the copy; the spec change log records F2-F4.
- F5 refuted: `#P3-15` is the plan's id for "heading starts with `## P3-15 `", not a rendered link.
