# Fragment: decision records (DR), P3-8, P3-9, P3-10

Spec: `docs/specs/2026-11-24-decision-records-design.md` (approved 2026-11-24). Node files: `docs/work/phase3/nodes/DR-*.md`. Planned at `origin/phase-3` `768bf48`.

## 1. Rows

| ID | Node | Depends | State | Capability | Refs |
|----|------|---------|------|------------|---|
| DR-1 | Record format: parse and validate one DDR file, and `docs/decisions/` in place (README with a valid example) | — | TODO | judgment | — |
| DR-2 | Record set: directory reader, set checks and the dependency graph | DR-1 | TODO | implementation | — |
| DR-3 | `zygo decision` verb: `ls`, `show`, `deps`, `check` and the slash form | DR-2 | TODO | implementation | — |
| DR-4 | `zygo decision new`: write a record template | DR-3 | TODO | implementation | — |
| DR-5 | Operator verbs `ratify` and `overturn`: byte-preserving outcome writes | DR-4 | TODO | judgment | — |
| DR-6 | `zygo decision review`: the phase-end review | DR-5 | TODO | implementation | — |
| DR-7 | `zygo work report`: the two record lines | DR-2, DR-3 | TODO | implementation | — |
| DR-8 | Session view `repo_root`, written by the launcher | — | TODO | mechanical | — |
| DR-9 | The `dispositions` guard key, default `enforce` | — | TODO | mechanical | — |
| DR-10 | Agent transitions: the D§6.6 rule for one record change | DR-1 | TODO | judgment | — |
| DR-11 | Verdict git reader: four bounded git processes behind an injectable trait (PD1) | DR-10 | TODO | judgment | — |
| DR-12 | Keep check: the D§9.4 steps over the node range | DR-11 | TODO | judgment | — |
| DR-13 | `work_verdict`: `head`, `base`, `dispositions` input and the `dispositions` guard | DR-7, DR-8, DR-9, DR-12 | TODO | judgment | — |
| DR-14 | End-to-end gate: the stop path, the merged-node keep and no stored text | DR-5, DR-13 | TODO | implementation | — |
| DR-15 | Live check: PD2 and PD3 on the pinned Claude Code (operator, by hand) | — | TODO | lookup | — |

Edges for a shared file only (no name consumed): DR-7 → DR-3 and DR-13 → DR-7 (`docs/specs/2026-11-18-zygo-design.md`); DR-11 → DR-10 (`crates/zygo-work/src/gate/mod.rs`); DR-5 → DR-4 (`crates/zygo/src/cmd/decision.rs`, `crates/zygo-work/src/decisions/mod.rs`).

## 2. Files per node

- DR-1: `crates/zygo-work/Cargo.toml`, `crates/zygo-work/src/lib.rs`, `crates/zygo-work/src/decisions/mod.rs`, `crates/zygo-work/src/decisions/record.rs`, `crates/zygo-work/tests/decision_record.rs`, `docs/decisions/README.md`
- DR-2: `crates/zygo-work/src/decisions/mod.rs`, `crates/zygo-work/src/decisions/set.rs`, `crates/zygo-work/src/decisions/graph.rs`, `crates/zygo-work/tests/decision_set.rs`
- DR-3: `crates/zygo-work/src/decisions/mod.rs`, `crates/zygo-work/src/decisions/history.rs`, `crates/zygo-core/src/verbs.rs`, `crates/zygo/src/cmd/mod.rs`, `crates/zygo/src/cmd/verbs.rs`, `crates/zygo/src/cmd/decision.rs`, `crates/zygo/tests/cli_surface.rs`, `crates/zygo/tests/decision_cli.rs`, `docs/specs/2026-11-18-zygo-design.md`
- DR-4: `crates/zygo-work/src/decisions/mod.rs`, `crates/zygo-work/src/decisions/new.rs`, `crates/zygo/src/cmd/decision.rs`, `crates/zygo/tests/decision_new.rs`
- DR-5: `crates/zygo-work/src/decisions/mod.rs`, `crates/zygo-work/src/decisions/edit.rs`, `crates/zygo/src/cmd/decision.rs`, `crates/zygo/tests/decision_verbs.rs`
- DR-6: `crates/zygo-work/src/decisions/mod.rs`, `crates/zygo-work/src/decisions/review.rs`, `crates/zygo/src/cmd/decision.rs`, `crates/zygo/tests/decision_review.rs`
- DR-7: `crates/zygo-work/src/report.rs`, `crates/zygo/src/cmd/work.rs`, `crates/zygo-work/tests/collection.rs`, `crates/zygo-work/tests/decision_report.rs`, `docs/specs/2026-11-18-zygo-design.md`
- DR-8: `crates/zygo-core/src/session_view.rs`, `crates/zygo-launch/src/launcher.rs`
- DR-9: `crates/zygo-config/src/schema.rs`, `crates/zygo-work/src/policy.rs`, `crates/zygo-work/tests/policy.rs`, `crates/zygo/src/cmd/guards.rs`, `crates/zygo/tests/slash_parity.rs`, `crates/zygo/tests/serve_cli.rs`
- DR-10: `crates/zygo-work/src/lib.rs`, `crates/zygo-work/src/gate/mod.rs`, `crates/zygo-work/src/gate/transition.rs`, `crates/zygo-work/tests/decision_transition.rs`
- DR-11: `crates/zygo-work/src/gate/mod.rs`, `crates/zygo-work/src/gate/git.rs`, `crates/zygo-work/src/repo.rs`, `crates/zygo-work/tests/decision_git.rs`
- DR-12: `crates/zygo-work/src/gate/mod.rs`, `crates/zygo-work/src/gate/check.rs`, `crates/zygo-work/tests/decision_verdict.rs`
- DR-13: `crates/zygo-mcp/src/tools/work.rs`, `crates/zygo-mcp/tests/work_verdict.rs`, `crates/zygo-work/src/guards.rs`, `docs/specs/2026-11-18-zygo-design.md`
- DR-14: `crates/zygo/tests/decision_gate.rs`
- DR-15: —

Files other specs also name (assembler: check for overlap and add edges): `crates/zygo-core/src/session_view.rs` (DR-8; session-runtime, output-shaping), `crates/zygo-launch/src/launcher.rs` (DR-8; session-runtime, workspace-definition), `crates/zygo-config/src/schema.rs` (DR-9; mail-sync, output-shaping, workspace-definition), `crates/zygo/src/cmd/mod.rs` (DR-3; mail-sync), `crates/zygo-core/src/verbs.rs`, `crates/zygo/src/cmd/verbs.rs`, `crates/zygo/tests/cli_surface.rs` (DR-3; any track that adds a verb; `VERB_SPECS` is a fixed-size array and `cli_surface.rs` lists every verb in order), `docs/specs/2026-11-18-zygo-design.md` (DR-3, DR-7, DR-13; any track with base-spec changes).

## 3. Cross-track needs

- Consumes from other tracks: none.
- Produces for other tracks: the record format and `docs/decisions/` (DR-1). The session-text, mail-sync, output-shaping and workspace-definition specs each say their build follows this spec (line 6 of each). Operator order: "P3-8 comes first in the order of work" (`docs/roadmap/phase-3.md:20`). Each other track's first node depends on DR-1. `zygo decision new` (DR-4) and `zygo decision check` (DR-3) come later; until then records are written by hand from the README example and checked by `cargo test -p zygo-work --test decision_record` only for that example.
- The `dispositions` guard (DR-13) refuses a `keep` with no `dispositions` at the default level. D§12 says the skill changes (items 3 to 12) ship before a zygo release that has this guard. Another track's nodes that run under a zygo binary built after DR-13 will have their `keep` refused until the act-lite/act101 skills call `work_verdict` with `head` and `dispositions` (D§9.3, 4th bullet). The skill changes are in other repositories and are not nodes here.

## 4. Global constraints (every DR node inherits these)

- No record text, finding text, label or disposition reason in the usage store, a log or a session file (D-C1, D§3).
- No hook reads a record file; the slash form reads none; the verdict check is in the MCP tool (D-C2, D§10.1).
- The record file is the only owner of a record's state; dependents, depth, counts and the node range are computed at each use; no index (D-C3).
- A record is never deleted or renamed (D-C4). An agent never writes an operator key or status (D-C5).
- Bounds (D-C6): each git process 2 s; one verdict at most 4 git processes and 10 s in all; `--batch` at most 2,048 objects and 16 MiB; node range at most 10,000 commits; at most 1,000 record changes read; record file at most 64 KiB, front matter closed within 8 KiB; directory at most 10,000 entries; `work_verdict` arguments at most 256 KiB, `dispositions` at most 256 entries, `finding` 1–200 bytes, `reason` 1–2,000 bytes; `title` 1–120 bytes; `unit` at most 128 bytes; `depends_on` at most 64 ids; `review_note` at most 2,000 bytes; `promotion` at most 500 bytes; `<problems>` at most 10 then `; and <n> more`, at most 4 KiB (D§9.3); `show` at most 50 commits, `review` at most 20 (D§10.1, D§11.2).
- The new guard follows the §13.4 levels and §13.5 resolution (D-C7, D§9.3).
- Record id `^DDR-[0-9A-HJKMNP-TV-Z]{26}$`; file `docs/decisions/<id>.md`; TOML front matter between `+++` lines; edits by zygo use `toml_edit` and keep unknown keys, comments and the body byte-equal (D§6.1, D§6.2).
- Text from a record or from git is printed one line at a time through `zygo_core::esc` (D§10.1).
- Not a security boundary against an agent that runs as the same OS user; no code or text claims more (D-C9).
- Tests: verdict tests use a git fixture repository with linked checkouts; git failures come from an injected git runner; no test measures wall-clock time (D§17 preamble).

## 5. Verification floor additions

- D§17 preamble: the verdict tests (DR-11, DR-12, DR-13, DR-14) need `git` on `PATH` in CI on ubuntu, macOS and Windows; no new command beyond `cargo test --workspace --locked --no-fail-fast`.
- D§12 item 11: the deliver step runs `zygo decision check`. After DR-3 is merged, delivery of the phase-3 branch adds `cargo run -p zygo --locked -- decision check` → exit 0.

## 6. Review focus

- DR-11, D§6.8 "In git": `git cat-file --batch` on `<rev>:<path>` returns a symlink as an ordinary blob. Only the tree entry mode shows a link or a submodule, so the reader must parse the tree object. A plain path read accepts a linked record.
- DR-12, D§9.4 step 4: `merge-base HEAD <head>` equal to `head` (node already merged) must skip steps 5 to 9, not report them.
- DR-12, D§8.1: the proofs bind to the node. A `fixed` commit that is an ancestor of `base` is refused even when it exists; a record present at `base` is refused even when it is valid at `head`; one record serving two findings is refused.
- DR-12, D§8.2 and Acceptance #7: a stop-class `recorded` entry in round 1 must give the stop problem, not the round-limit problem (the round limit applies to class `none` only).
- DR-12, D§9.4 step 9: merge commits and commits without `Zygo-Session:` are not read (the operator's ratify commit). A message that contains `Zygo-Session:` only in its body text, not as a trailer line, is an open question for the plan.
- DR-10, D§6.3 "frozen": a change of `class` to `none` or of `kind` after creation is the specific mistake the lead ruled against.
- DR-5, D§6.2 and Acceptance #14: byte-equal preservation with CRLF line ends, comments between keys and unknown tables. Also the re-read-then-rename race (D§11.3).
- DR-13, D§9.2: argument refusals happen before the level is read (all three levels). The absence of `head`, `base` or `dispositions` is a guard problem, not an argument refusal. `monitor` makes no git call.
- DR-13, D§9.3: at `enforce` a refusal must leave `work_units.verdict` unchanged (no `set_verdict` call before the check).
- DR-3, DR-6, D§10.1: ESC, CR, bidi and line separators in a title, body, note or commit subject.
- DR-1, DR-2, DR-3: links and FIFOs under `#[cfg(unix)]`. Windows link creation needs a privilege, and the existing tests gate links the same way (`crates/zygo-hooks/tests/workspace.rs:642`).
- DR-7, D-C2: `/zygo:work report` runs inside the `UserPromptSubmit` hook, so the slash form must not read `docs/decisions/`.
- DR-4, D§10.2: a write failure leaves at most the `.tmp` file, which every reader ignores (`name_kind` → `Other`).

## 7. Author's calls

- The record code lives in `zygo-work` (`decisions::*` for P3-8/P3-10, `gate::*` for P3-9), beside `report.rs` and the `repo.rs` git runner. No new crate.
- DR-1 is the first node: the format and the README with a valid filled example. `new` and `check` are D§10 (no P3-8 mark in the spec), so they come in DR-4 and DR-3.
- `docs/decisions/README.md` holds the example. Readers ignore it by name (D§6.8).
- `toml` and `toml_edit` are added to `zygo-work` `[dependencies]` in DR-1 (`toml` is a dev-dependency there today; both are workspace dependencies, `Cargo.toml:66-67`).
- `ls` "dependents count" is the transitive count over all valid records (D§5 defines Dependent transitively). `review` leaves out `superseded` records as D§11.2 step 2 states.
- The slash form of `zygo work report` prints no record lines (D-C2). The report takes `records_in: Option<&Path>`.
- The base-spec edits of D§13 go with the node that implements them: §17/§17.1 → DR-3, §13.6 → DR-7, §13.3/§13.4/§13.5 → DR-13. DR-9 changes code only. Item 6 (session view) is code only (DR-8).
- The git reader is split from the check (DR-11 and DR-12), and the transition rule from both (DR-10), so that each can be rejected alone. DR-10 creates `gate/mod.rs`, so DR-11 depends on it.
- PD1 needs only `git` and is checked in CI (DR-11). PD2 and PD3 need a real Claude Code, so they are the operator-run lookup DR-15 with rows in `docs/live-checks.md`.
- The end-to-end stop path (Acceptance #7, #8, #16) is in its own node (DR-14) in the `zygo` crate, because it needs the `zygo` binary and the tool together.
- `review` takes its terminal state from `DecisionIo.tty`, so tests need no real terminal.
- Separate test files per node (`decision_cli`, `decision_new`, `decision_verbs`, `decision_review`) so that each acceptance is one test target.
