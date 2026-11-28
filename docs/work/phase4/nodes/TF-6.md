# TF-6 — Tests: every child-stdin write in `crates/zygo/tests` goes through `common::write_stdin`

**Spec:** none (test-harness rule), as TF-1; TF-1's Produces (`nodes/TF-1.md:6`): "`common::write_stdin`, a stdin write that ignores `ErrorKind::BrokenPipe` and only that error; every child-stdin write in `crates/zygo/tests` goes through it"; found by the CC-0 merge (round 1 F2, `nodes/CC-0.md` "Round 1 review")
**Files:** `crates/zygo/tests/common/mod.rs` (`feed_stdin` removed), `crates/zygo/tests/compact_cmd.rs`, `crates/zygo/tests/decision_gate.rs`, `crates/zygo/tests/definition_hooks.rs`, `crates/zygo/tests/mail_cli.rs`, `crates/zygo/tests/mail_sync_cli.rs`, `crates/zygo/tests/mail_sync_forget.rs`, `crates/zygo/tests/mail_sync_no_content.rs`, `crates/zygo/tests/workspace_approve.rs`, `crates/zygo/tests/decision_cli.rs`, `crates/zygo/tests/definition_doctor.rs`, `crates/zygo/tests/definition_surfaces.rs` (each: the stdin write sites, their `use` lines, and in `definition_doctor.rs` a `mod common;` line)
**Consumes:** `common::write_stdin` (TF-1)
**Produces:** `crates/zygo/tests` with one child-stdin helper, `common::write_stdin`; no `feed_stdin`
**Acceptance:** from the repository root: `grep -rn feed_stdin crates/zygo/tests` → prints nothing; `grep -rn write_all crates/zygo/tests` → prints only the sites that are not a child's stdin (Planning notes, "Not this pattern") and the one inside `write_stdin`; `TMPDIR=/tmp CARGO_INCREMENTAL=0 cargo test -p zygo --locked --no-fail-fast` → exit 0 (`TMPDIR` outside any git checkout).

## Planning notes

- The rule is TF-1's (Spec line). CC-0 merged phase 3 (`74748ae7`) into `phase-4`; phase 3 brought its own helper `common::feed_stdin` (`crates/zygo/tests/common/mod.rs:90-96`) and raw child-stdin writes. CC-0 round 1 F2 changed only the conflicted call in `compact_cmd.rs` `zygo(..)` and left the rest to this node.
- `feed_stdin(mut stdin: ChildStdin, bytes)` and `write_stdin(mut w: impl Write, bytes)` (`common/mod.rs:79-84`) do the same: `write_all`, a `BrokenPipe` error is ignored, any other error panics (`feed_stdin` by `assert_eq!` on the kind, `write_stdin` by `panic!`), and the handle drops at the end (EOF to the child). So each change is a rename at the call; no assertion changes.
- Sites, from `grep -rn feed_stdin crates/zygo/tests` and `grep -rn write_all crates/zygo/tests` on `phase-4` at `b324e1dc`:
  - `feed_stdin` calls (13): `compact_cmd.rs:455`; `decision_gate.rs:476`, `:519`; `definition_hooks.rs:700`, `:994`, `:1053`; `mail_cli.rs:68`; `mail_sync_cli.rs:97`; `mail_sync_forget.rs:93`; `mail_sync_no_content.rs:170`, `:225`, `:1994`; `workspace_approve.rs:173`.
  - `feed_stdin` imports (5): `definition_hooks.rs:263` and `mail_cli.rs:21` (`use crate::common::feed_stdin;`, in an inner module); `mail_sync_cli.rs:22`, `mail_sync_forget.rs:27`, `mail_sync_no_content.rs:41` (`use common::feed_stdin;`).
  - The definition: `common/mod.rs:90-96` (doc comment and function).
  - Raw child-stdin writes (4): `decision_cli.rs:314` (`std::io::Write::write_all(&mut child.stdin.take().unwrap(), …).unwrap()`); `definition_doctor.rs:67`, `definition_surfaces.rs:80`, `workspace_approve.rs:83` (`child.stdin.take().unwrap().write_all(bytes).unwrap()`, each in `blob`, the stdin of `git hash-object -w --stdin`, followed by `wait_with_output` and a status assert).
  - Not this pattern (unchanged): `session_verbs.rs:478` (a FIFO), `workspace_session.rs:205` and `common/mod.rs:72` (files), `workspace_restore.rs:106`, `:981`, `:983` (stderr, `let _ =`), `update.rs:71` (a socket, `let _ =`), `member_fixture/mod.rs:172` (a socket, `let _ =`), `common/mod.rs:80` (inside `write_stdin`).
- `definition_doctor.rs` does not declare `mod common;`; it gets one (`common/mod.rs` has `#![allow(dead_code)]`). `decision_cli.rs`, `definition_surfaces.rs` and `workspace_approve.rs` declare it.
- `use std::io::Write;` has no other user in `definition_doctor.rs` (`:3`), `definition_surfaces.rs` (`:10`) and `workspace_approve.rs` (`:10`) after the change (no other `write`, `write!`, `writeln!` or `flush` call in them); it is removed, as TF-1 removed it where the replaced site was its only user (`-D warnings` rejects an unused import).
- These test files came from phase 3. `phase-4` takes each new phase-3 head in a merge layer of its own (operator, 2026-11-27); a later phase-3 merge may conflict on these lines or bring new `feed_stdin` calls. That merge keeps `write_stdin` (and changes each new `feed_stdin` call to it); a merge layer never brings `feed_stdin` back.
- `feed_stdin` is also named in phase-3 work records (`docs/work/phase3/…`) and `nodes/CC-0.md`; they are records and stay as written.
- Files overlap: CC-8, CC-9, CC-10 and CC-13 list `crates/zygo/tests/`; CC-8 depends on TF-6 (CC-9, CC-10, CC-13 through CC-8), so CC-8 starts on the single helper (author's call, as CC-8 depends on TF-1). CC-0 shares files and is below TF-6.
- No registry standard text, tier, bar exception or boundary exception list changes; no operator stop. Not a P4-6 node, so `check-test-edits.sh` is not part of its check; it changes no literal and no test function name.

## Plan

Capability: `mechanical` (the same one-line change at each listed site; one function removed).

### Steps

- [ ] 1. In `.worktrees/phase4/TF-6` (branch `phase-4--TF-6` from `origin/phase-4`), run `grep -rn feed_stdin crates/zygo/tests` and `grep -rn write_all crates/zygo/tests`. Pass: the output lists exactly the sites of Planning notes (line numbers may differ only if `phase-4` moved past `b324e1dc`; then list the new ones under Execution notes).
- [ ] 2. Change each `feed_stdin(<stdin>, <bytes>)` call to `write_stdin(<stdin>, <bytes>)` with the same path prefix (`common::`, or none where the file imports it), and each `use … ::feed_stdin;` to `use … ::write_stdin;`.
- [ ] 3. Change the four raw writes to `common::write_stdin(child.stdin.take().unwrap(), <bytes>)`; add `mod common;` to `definition_doctor.rs` beside its other items at the top; remove `use std::io::Write;` from `definition_doctor.rs`, `definition_surfaces.rs` and `workspace_approve.rs`.
- [ ] 4. Remove `feed_stdin` and its doc comment from `common/mod.rs`. Pass: `grep -rn feed_stdin crates/zygo/tests` prints nothing; `grep -rn write_all crates/zygo/tests` prints only the "Not this pattern" sites and `common/mod.rs` `write_stdin`.
- [ ] 5. Keep each assertion: `git diff origin/phase-4 -- crates/zygo/tests` shows only call-site lines, `use` lines, the `mod common;` line and the removed function. Run `cargo clippy -p zygo --tests --locked -- -D warnings`. Pass: exit 0.
- [ ] 6. Acceptance: `TMPDIR=/tmp CARGO_INCREMENTAL=0 cargo test -p zygo --locked --no-fail-fast`. Pass: exit 0, 0 failed.
- [ ] 7. Commit `test(zygo): TF-6 one child-stdin helper (write_stdin)` with the trailers `Zygo-Session: <session>` and `Zygo-Work: phase4:TF-6`; no Claude attribution. Push the node branch (no force). Open the pull request into `phase-4`.

### Execution notes
