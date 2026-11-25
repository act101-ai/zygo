# CC-2 — `scripts/check-test-edits.sh`: the per-node test-edit check

**Spec:** docs/specs/2026-11-25-client-model-design.md K§4 K-C7 (amended, lead, 2026-11-25), K-D3, K§9 (P4-6, "Test files"), K§11 item 24
**Files:** `scripts/check-test-edits.sh` (new), `scripts/test-check-test-edits.sh` (new), `tools/test-edits/` (new: `Cargo.toml`, `src/main.rs`; crate `zygo-test-edits`, `publish = false`), `Cargo.toml` (workspace member), `Cargo.lock`
**Consumes:** `phase_3_in_phase_4` (CC-0)
**Produces:** `check-test-edits.sh <parent> <node file>` (exit 0 when the K-C7 check passes; exit 1 with each offending unit and the change; exit 2 when `<parent>` is empty or not a commit, or the node file is missing), the "Test logic edits" entry forms: `- removed <file>::<fn> — <reason>`, `- renamed <file>::<old> -> <new> — <reason>`, `- ignore <file>::<fn> — <reason>`, `- literals <file>: <delta> — <reason>` (`<delta>` is the exact multiset change as the tool prints it, for example `+"claude-code" x3, -"x" x1`)
**Acceptance:** `test "$(sh scripts/test-check-test-edits.sh | grep -c '^ok ')" -ge 12 && sh scripts/test-check-test-edits.sh && cargo test -p zygo-test-edits --locked` → at least 12 cases print `ok <case>` and both commands exit 0. Each case is a planted commit pair in a throw-away repository: a changed string literal fails; a changed number literal fails; a removed test function fails; a renamed test function fails; an added `#[ignore]` fails; a literal change listed with its exact delta passes; a listed delta that differs from the real one fails; a listed removal passes; a type-path rename (`Agent::ClaudeCode` to another identifier path) passes; an added `use` line passes; a `#[cfg(test)]` module moved to another `src` file of the same crate with identifier edits only passes; an edited doc comment passes; an empty `<parent>` exits 2

## Planning notes

- The check per K-C7 (amended): each node is checked against its own diff, from `<parent>`, the parent of its layer (the top of the stack below it). No rule table: the `scripts/test-edit-rules/*.rules` mechanism of the first plan is dropped.
- Units: each file under `crates/*/tests/` and each `#[cfg(test)]` item of `crates/*/src/` that exists at `<parent>`. New files and new `#[cfg(test)]` items are not checked.
- Bounds of a `#[cfg(test)]` module: a POSIX shell cannot find them reliably (nested braces, braces in strings and macros). The helper `zygo-test-edits` parses each file at `<parent>` and at `HEAD` (read with `git show`) with `syn` (already a workspace dependency from CF-1), and takes each item whose attributes hold a `cfg` predicate that names `test` (`cfg(test)`, `cfg(all(test, unix))`). The script is a thin wrapper: `cargo run -q --locked -p zygo-test-edits -- "$@"`.
- Test functions: each `fn` with an attribute whose last path segment is `test` (`#[test]`, `#[tokio::test(...)]`), named by its module path. Literals: every string, byte-string, char and number literal token in the unit, also inside macro token streams; `#[doc]` attributes (doc comments) are skipped, so a comment edit passes.
- Matching a unit at `HEAD`: a test file by `git diff -M` rename detection; a `#[cfg(test)]` module by its module path in any `src` file of the same crate (the moves of CC-3 and CC-9).
- Node file: the entries under its `## Test logic edits` heading. A node's acceptance passes the parent from the `Parent: <40-hex>` line that the node writes as the first line of its `## Plan` when it starts.
- The test script builds repositories under `mktemp -d`, with no wall-clock wait (Global constraints). The check is not added to CI: the review gate of each P4-6 node runs it (K§11 item 24; Verification floor).

## Test logic edits

None.

## Plan

Written when the node starts.
