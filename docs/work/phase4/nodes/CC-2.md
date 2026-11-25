# CC-2 — `scripts/check-test-edits.sh`: test edits only by declared rewrite rules

**Spec:** docs/specs/2026-11-25-client-model-design.md K§4 K-C7, K-D3, K§9 (P4-6, "Test files"), K§11 item 24
**Files:** `scripts/check-test-edits.sh` (new), `scripts/test-check-test-edits.sh` (new), `scripts/test-edit-rules/` (new; one `<node-id>.rules` file per P4-6 node that rewrites test lines)
**Consumes:** `phase_3_in_phase_4` (CC-0)
**Produces:** `check-test-edits.sh <base>` (exit 0 when each test edit since `<base>` is a rewrite by a rule of `scripts/test-edit-rules/*.rules`; else exit 1 with each offending `file:line`; prints each edited test line), the rule line form `<identifier path> => <replacement>`
**Acceptance:** `test "$(sh scripts/test-check-test-edits.sh | grep -c '^ok ')" -ge 11 && sh scripts/test-check-test-edits.sh && sh scripts/check-test-edits.sh "$(git rev-parse HEAD~1)"` → at least 11 cases print `ok <case>` and the test script exits 0. The cases: a rewrite-only edit passes; a changed expected value fails; a changed assertion macro fails; a rule pattern with a digit, with a quote, with an operator is refused (three cases); a renamed test file with rewrite-only edits passes; a `#[cfg(test)]` module moved to another `src` file with rewrite-only edits passes; a deleted test function fails; a new test file passes and is listed; an added line in a test file that exists at `<base>` fails

## Planning notes

- Test lines: each line under `crates/*/tests/`, and each line inside a `#[cfg(test)]` item of `crates/*/src/` (K§11 item 24).
- The rewrite table is the union of `scripts/test-edit-rules/*.rules` (author): one file per node, so stacked nodes declare their own rules with no shared file. The pull request of each P4-6 node lists its rules file and the script's list of edited lines.
- New tests go in new files only (author): K-C7 allows no other change to a test that exists at the base, and an added line there can change that test. So CC-3, CC-8, CC-9, CC-11, CC-13 and CC-14 put new tests in new files or in new `src` modules.
- The script runs in a POSIX shell on Linux and macOS with `git` only. It is not added to CI: K§11 item 24 runs it at the review gate of each P4-6 node; the Verification floor names it.
- The test script builds throw-away git repositories under a `mktemp -d` folder, with no wall-clock wait (Global constraints).

## Plan

Written when the node starts.
