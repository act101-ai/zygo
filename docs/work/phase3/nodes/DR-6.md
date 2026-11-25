# DR-6 — `zygo decision review`: the phase-end review

**Spec:** `docs/specs/2026-11-24-decision-records-design.md` D§11.2 (steps 1 to 6), D§11.1 (session and terminal refusals), D§11.3 (Skip; re-read before each write), D§10.1 (row `review`; `zygo_core::esc`), D§14 (column `review, ratify, overturn`)
**Files:** `crates/zygo-work/src/decisions/mod.rs`, `crates/zygo-work/src/decisions/review.rs`, `crates/zygo/src/cmd/decision.rs`, `crates/zygo/tests/decision_review.rs`
**Consumes:** `apply` (DR-5), `Outcome` (DR-5), `Promotion` (DR-5), `EditError` (DR-5), `run_with` (DR-3), `DecisionIo` (DR-3), `trailer_commits` (DR-3), `read_dir` (DR-2), `dependent_count` (DR-2), `Record` (DR-1), `Kind` (DR-1)
**Produces:** `decisions::review::Queue`; `decisions::review::Queue::new(records: &[Record], kind: Option<Kind>, unit_prefix: Option<&str>) -> Queue` (open records by transitive dependent count over the non-`superseded` records, largest first, then id); `decisions::review::Queue::next(&mut self) -> Option<String>`; `decisions::review::Queue::overturned(&mut self, id: &str, records: &[Record])` (moves that record's open dependents to the front, in the same order); `review` (the subcommand `zygo decision review [--kind <k>] [--unit <prefix>]`)
**Acceptance:** `cargo test -p zygo --locked --test decision_review` → all pass. Covers spec Acceptance #12 (`review` exits 2 with `ZYGO_SESSION_DIR` set, and exits 2 with `zygo decision review: needs a terminal; use ratify or overturn with --yes in scripts` without a terminal; records come in dependents order, largest first; after an overturn the open dependents of that record come next, each with `depends on overturned <id>`; each outcome is in its file before the next prompt; a second `review` shows only the records still open) and #13 (a record changed on disk while `review` shows it is skipped with `<id> changed since it was shown; skipped` and the file is unchanged; a record title with an ESC character prints as `\u{1b}`); `q` prints the counts per outcome, the changed files and `commit these files; add a Zygo-Decision: line per id`; no `docs/decisions/` → `no open records`, exit 0. The terminal is the `DecisionIo.tty` flag set by the test, not a real terminal.

## Plan

### Premises checked against the live code (phase-3 at `8587e0b`)

Interfaces, copied from the code as they are now:

```rust
// crates/zygo-work/src/decisions/edit.rs (DR-5)
pub enum Promotion { Adr, Spec, Rule }                       // :12; parse(&str) -> Option<Self> :20, name(self) :29
pub enum Outcome { Ratify { note: String, promote: Option<(Promotion, String)> }, Overturn { reason: String } } // :40
pub enum EditError { Changed, Refused(String), Io(String) }   // :46
pub fn allowed(r: &Record, outcome: &Outcome) -> Result<(), String>                                         // :59
pub fn apply(path: &Path, lock: &Path, shown: &[u8], outcome: &Outcome, today: &str) -> Result<(), EditError> // :133
pub fn lock_path(common_dir: &Path) -> PathBuf                                                              // :142
// crates/zygo/src/cmd/decision.rs (DR-3, DR-5)
pub struct DecisionIo<'a> { pub input: &'a mut dyn BufRead, pub out: &'a mut dyn Write, pub err: &'a mut dyn Write, pub tty: bool } // :31
pub struct Repo<'a> { pub checkout: &'a dyn Fn() -> Option<(PathBuf, PathBuf)>, pub commits: &'a CommitsFn,
    pub head: &'a dyn Fn(&Path) -> Option<String>, pub today: &'a dyn Fn() -> String, pub usage_db: &'a Path } // :40
pub type CommitsFn = dyn Fn(&Path, &str, usize) -> Result<Vec<(String, String)>, String>;                   // :55
pub fn run_in(repo: &Repo, env: &dyn Fn(&str) -> Option<OsString>, args: &[String], io: &mut DecisionIo) -> i32 // :219
pub fn run_with(cwd: &Path, env: &dyn Fn(&str) -> Option<OsString>, args: &[String], io: &mut DecisionIo) -> i32 // :423
// crates/zygo-work/src/decisions/history.rs (DR-3)
pub fn trailer_commits(top: &Path, id: &str, limit: usize) -> Result<Vec<(String, String)>, String>          // :12
// crates/zygo-work/src/decisions/set.rs, graph.rs (DR-2)
pub struct RecordSet { pub valid: Vec<Record>, pub malformed: Vec<(String, Vec<String>)> }                   // set.rs:15
pub fn read_dir(top: &Path) -> Result<RecordSet, DirError>                                                  // set.rs:35
pub fn dependents(records: &[Record], id: &str) -> Dependents      // graph.rs:87; Dependents { rows: Vec<(u32, String)>, cycle_at }
pub fn inverse_of(records: &[Record], skip_superseded: bool) -> Inverse<'_>                                  // graph.rs:95
pub fn count_in(inv: &Inverse, id: &str) -> usize                                                           // graph.rs:100
pub fn dependent_count(records: &[Record], id: &str, skip_superseded: bool) -> usize                         // graph.rs:115
// crates/zygo-work/src/decisions/record.rs (DR-1)
pub struct Record { pub id, pub kind: Kind, pub status: Status, pub class: Class, pub title, pub unit, pub round: u32,
    pub role, pub model, pub date, pub depends_on: Vec<String>, pub body: String, .. }                       // :67
pub enum Kind { Assumption, Decision, AcceptedLimit, Compromise, OpenFinding }                              // :32
pub fn parse_record(stem: &str, bytes: &[u8]) -> Result<Record, Vec<String>>                                // :242
pub fn read_bytes(path: &Path) -> Result<Vec<u8>, Vec<String>>                                              // :363
// crates/zygo-core/src/text.rs
pub fn esc(s: &str) -> String                                                                               // :4
```

Reused in `decision.rs`: `session_dir` (:176), `line` (:181, prints through `esc`), `print_lines` (:343), the D§11.1 session refusal (:239-244), the D§14 row 1 check (:249-255), and `decide`'s prompt pattern (:364-375). `wants_help` (:202) already skips the values of `--kind` and `--unit`.

**Drift.**
1. `DecisionIo.tty` is documented and set as "stdin is a terminal" only (`decision.rs:35-36`, `:437`). D§11.1 asks for a terminal on stdin and stdout. Step 7 sets `tty = stdin.is_terminal() && stdout.is_terminal()` and changes the doc comment. `ratify` and `overturn` do not read `tty`, so they do not change.
2. `USAGE` (`decision.rs:23-26`) is asserted byte for byte in `crates/zygo/tests/decision_cli.rs:102`, `decision_new.rs:17` and `decision_verbs.rs:192`, which are not on the Files line. This plan leaves `USAGE` and the clap help text (`crates/zygo/src/cmd/mod.rs:361`) unchanged. To add `| review [--kind <k>] [--unit <prefix>]`, the lead must add those three test files to Files.
3. `graph::Inverse`'s map is private (`graph.rs:92`), so `review.rs` cannot walk it. `Queue::overturned` gets the dependents from `dependents()` over a copy of the records that are not `superseded`.

### Interface (Produces, final)

`crates/zygo-work/src/decisions/review.rs` (`pub mod review;` in `decisions/mod.rs`):

```rust
/// The open records of one `zygo decision review` in the D§11.2 order.
pub struct Queue {
    order: VecDeque<String>,            // the ids that `next` has not returned yet, front first
    rank: HashMap<String, usize>,       // each id's step 2 position
    count: HashMap<String, usize>,      // each id's dependent count (step 2)
    cause: HashMap<String, String>,     // id -> the overturned record that last moved it to the front
}
impl Queue {
    pub fn new(records: &[Record], kind: Option<Kind>, unit_prefix: Option<&str>) -> Queue;
    pub fn next(&mut self) -> Option<String>;
    pub fn overturned(&mut self, id: &str, records: &[Record]);
    /// The ids not yet returned (the `N` of `[i/N]` at the start).
    pub fn remaining(&self) -> usize;
    /// The step 2 count of a queued id; 0 for another id.
    pub fn dependents(&self, id: &str) -> usize;
    /// The overturned record that last moved `id` to the front.
    pub fn cause(&self, id: &str) -> Option<&str>;
}
```

Algorithms (the tests check the results, not the method):
- `new`: `inv = inverse_of(records, true)`. Keep `status == Open`, `kind.is_none_or(|k| r.kind == k)`, `unit_prefix.is_none_or(|p| r.unit.starts_with(p))`. Sort the `(count_in(&inv, id), id)` pairs by count descending, then id ascending. `rank` is the index after the sort.
- `overturned`: `live` = the records whose status is not `Superseded`, cloned. `deps` = the ids of `dependents(&live, id).rows`. `moved` = the queued ids in `deps`, sorted by `rank`. `order` = `moved` followed by the other queued ids in their current order. Set `cause[m] = id` for each moved id.

`crates/zygo/src/cmd/decision.rs`: `Sub::Review { kind: Option<Kind>, unit: Option<String> }`, parsed from `("review", rest)`: `--kind <k>` (`Kind::parse`), `--unit <prefix>`, a later flag replaces an earlier one, and anything else is `None` (USAGE, exit 2). `fn review(repo: &Repo, top: &Path, lock: &Path, set: &RecordSet, kind: Option<Kind>, unit: Option<&str>, io: &mut DecisionIo) -> i32`.

Output of `review` (spec-fixed text is quoted in the spec; the rest is an author's call, see the end):
```
[<i>/<N>] <id> <kind> class <class> dependents <n>
<title>
unit <unit> round <round> role <role> model <model> date <date>
depends on overturned <cause id>          (only for a record that an overturn moved)
depends_on <dep id> <status>              (one per depends_on id; `(overturned)` instead of the status; `missing` for no valid record)
<body lines>
commits: <k>
<hash> <subject>                          (at most 20, `repo.commits(top, id, 20)`; an Err prints its line on stderr)
r ratify · p promote · o overturn · s skip · q quit                  (prompt, no newline)
```
Sub-prompts (no newline): after `r`, `note (empty for none): `; after `p`, `promote to adr, spec or rule: `, then `promotion text: `; after `o`, `reason: `. An unknown action asks the action prompt again. A bad target or an empty text or reason asks the same sub-prompt again. EOF or a read error at any prompt acts as `q`. Every line of record or git text goes through `line`/`print_lines` (`esc`).

The end, at the queue's end or at `q`: `ratified <a>, promoted <b>, overturned <c>, skipped <d>` on stdout, then one `docs/decisions/<id>.md` line per written file in the order written, then `commit these files; add a Zygo-Decision: line per id`. Exit 0.

### Steps

- [x] **1. Write the failing `Queue` tests** in the new `crates/zygo/tests/decision_review.rs`. Header `//! DR-6: zygo decision review (D§11.1, D§11.2, D§11.3, D§10.1, D§14, D§17 #12, #13).` Fixtures: `C40`, `BODY`, `id(n)` and `text(rid, kind, status, title, unit, deps, extra, body)` as in `decision_cli.rs:9-18`; `TODAY = "2026-12-01"`; `RATIFIED = "reviewed = \"2026-11-30\"\nreview_note = \"\"\n"`. `fn graph(top: &Path) -> Vec<Record>` writes fixture G and returns `read_dir(top).unwrap().valid`:

  | n | kind | status | unit | depends_on | step 2 count |
  |---|---|---|---|---|---|
  | 1 | assumption | open | phase-3:N1 | — | 3 (2, 3, 4) |
  | 2 | decision | open | phase-3:N1 | 1 | 1 |
  | 3 | decision | open | phase-3:N1 | 2 | 0 |
  | 4 | decision | open | phase-3:N2 | 1 | 0 |
  | 5 | decision | open | phase-3:N1 | — | 1 (7; 6 is superseded, 8 is reached only through 6) |
  | 6 | decision | superseded | phase-3:N1 | 5 | — |
  | 7 | decision | ratified + `RATIFIED` | phase-3:N1 | 5 | — |
  | 8 | decision | open | phase-3:N1 | 6 | 0 |

  Tests:
  - `queue_orders_by_dependents_then_id`: draining `Queue::new(&g, None, None)` gives ids `[1, 2, 5, 3, 4, 8]`. Before the drain, `remaining() == 6`, `dependents(&id(1)) == 3`, `dependents(&id(5)) == 1`.
  - `queue_filters_by_kind_and_unit_and_counts_over_all_records`: `Kind::Assumption` drains `[1]` with `dependents(&id(1)) == 3`. `Some("phase-3:N2")` drains `[4]`.
  - `overturned_moves_the_open_dependents_to_the_front_in_step_2_order`: `next` gives 1 and 2. `overturned(&id(2), &g)` makes the drain start with 3. On a new queue: `next` 1, `next` 2, `overturned(&id(1), &g)` drains `[3, 4, 5, 8]`; `cause(&id(3)) == Some(id(1))`, `cause(&id(5)) == None`. Overturning 5 moves nothing, because 7 is not queued and 8 depends only on a superseded record.
- [x] **2. Run** `cargo test -p zygo --locked --test decision_review`. Check: it fails to compile, because `decisions::review` does not exist.
- [x] **3. Implement** `review.rs` (Interface and algorithms above) and add `pub mod review;` to `decisions/mod.rs`.
- [x] **4. Run** the same command. Check: the three tests pass.
- [x] **5. Write the failing CLI tests** in the same file. Helpers: `go(top, env, tty, input: &mut dyn BufRead, args) -> Ran` as `decision_verbs.rs:195-203`, but with `tty` as given, `checkout` giving `(top, top/.git)` (created), and `commits` that asserts `limit == 20` and returns `[("a".repeat(40), "first")]` for `id(1)` and no commits for the others. `review(top, input: &str) -> Ran` = `go(top, &|_| None, true, &mut input.as_bytes(), &["review"])`. `headers(out)` = the lines that start with `[`. `Script { answers: VecDeque<String>, probe: PathBuf, seen: Vec<String>, edit: Option<(PathBuf, String)> }`: a `Read` that returns one answer per call (EOF when none are left). At each call it pushes the `status` of the record at `probe` (the `parse_record` of its bytes) to `seen`. At its first call it writes `edit`, if set. It is wrapped in `BufReader`, so that each prompt reads one answer.
  - `review_exits_2_in_an_agent_session`: env `ZYGO_SESSION_DIR=<tmp>/session`, `tty` true. Result: `(2, "", "zygo decision review: run it in your own shell, not in an agent session\n")`. Every fixture file is byte-equal.
  - `review_without_a_terminal_exits_2`: `tty` false. Result: `(2, "", "zygo decision review: needs a terminal; use ratify or overturn with --yes in scripts\n")`.
  - `the_binary_refuses_without_a_terminal_and_in_a_session`: `Command::new(env!("CARGO_BIN_EXE_zygo"))`, `common::scratch_home`, `decision review`, `stdin(Stdio::null())`, `env_remove("ZYGO_SESSION_DIR")`. Result: exit 2, stderr is the terminal line. The same with `.env("ZYGO_SESSION_DIR", <tmp>)` (set on purpose) gives exit 2 and the session line. `mod common;`.
  - `review_shows_records_in_dependents_order`: fixture G, input `"s\n"` × 6. `headers` == `[1/6] <id1> assumption class none dependents 3`, `[2/6] <id2> decision class none dependents 1`, `[3/6] <id5> … dependents 1`, `[4/6] <id3> … 0`, `[5/6] <id4> … 0`, `[6/6] <id8> … 0`. The output holds `depends_on <id1> open`, `commits: 1\n<aaaa…> first\n` once, and `r ratify · p promote · o overturn · s skip · q quit` 6 times. It ends with `ratified 0, promoted 0, overturned 0, skipped 6\ncommit these files; add a Zygo-Decision: line per id\n`. Exit 0. The files are byte-equal.
  - `after_an_overturn_its_open_dependents_come_next`: input `"o\nwrong\n"`, then `"s\n"` × 5. `headers` ids are `[1, 2, 3, 4, 5, 8]`. `depends on overturned <id1>` occurs 3 times, once after each of the headers of 2, 3 and 4. The record 2 block holds `depends_on <id1> (overturned)`. File 1 has `status = "overturned"` and `review_note = "wrong"`, and `reviewed` is TODAY.
  - `each_outcome_is_in_its_file_before_the_next_prompt`: `Script` with the probe on file 1 and the answers `"r\n"`, `"ok\n"`, `"s\n"`. `seen == ["open", "open", "ratified"]`. File 1 has `status = "ratified"` and `review_note = "ok"`. A second `review(top, "s\n" × 5)` has no header for id 1, and its first header starts with `[1/5] <id2>`.
  - `q_prints_counts_files_and_the_commit_line`: input `"r\n\np\nrule\nAdd it\nq\n"`. Exit 0. The output ends with `ratified 1, promoted 1, overturned 0, skipped 0\ndocs/decisions/<id1>.md\ndocs/decisions/<id2>.md\ncommit these files; add a Zygo-Decision: line per id\n`. File 2 has `promoted_to = "rule"` and `promotion = "Add it"`. File 5 is byte-equal.
  - `unknown_answers_ask_again_and_eof_quits`: fixture G, input `"x\no\n\nwrong\n"`, then EOF. The action prompt occurs 3 times (twice for record 1, once for record 2, where EOF quits), and `reason: ` occurs twice. File 1 is overturned with `wrong`. The output ends with `ratified 0, promoted 0, overturned 1, skipped 0\ndocs/decisions/<id1>.md\n…commit line`.
  - `a_record_changed_while_shown_is_skipped`: one open record 1. `Script` edits file 1 at the first read (title `T` → `T2`) and answers `"r\n"`, `"\n"`. stderr is `<id1> changed since it was shown; skipped\n`. File 1 == the edited text. The output ends with `ratified 0, promoted 0, overturned 0, skipped 1\ncommit these files; add a Zygo-Decision: line per id\n`.
  - `an_esc_in_a_title_prints_as_an_escape`: record 1 with the TOML title `"a\\u001bb"`, input `"q\n"`. The output holds the line `a\u{1b}b` (the escape as text), and `!out.contains('\u{1b}')`.
  - `failure_rows_of_d14`: (a) no `docs/decisions` → `(0, "no open records\n", "")`; (b) only records 6 and 7 → the same; (c) fixture G with `--kind compromise` → the same; (d) `docs/decisions` as a file → `(1, _, "zygo decision: docs/decisions is not a directory\n")`; (e) record 1 and a malformed `<id9>.md` (no `## Evidence`), input `"s\n"` → stderr is `zygo decision: <id9>.md: cannot be reviewed; zygo decision check names the fault\n` and there is one header; (f) `review --kind bogus` and `review --unit` → exit 2 with the USAGE line on stderr.
  - `a_write_failure_exits_1_and_keeps_earlier_outcomes` (`#[cfg(unix)]`, skipped as root as in `decision_verbs.rs:333`): records 1 and 5. Run 1 with input `"r\n\nq\n"` ratifies record 1. The test then sets `docs/decisions` to mode `0o500`. Run 2 with input `"o\nx\n"` shows record 5 first. Then the mode goes back to `0o700`. Run 2 result: exit 1, stderr starts with `zygo decision: cannot write ` and contains `docs/decisions`, and the output ends with `ratified 0, promoted 0, overturned 0, skipped 0\ncommit these files; add a Zygo-Decision: line per id\n`. File 1 stays ratified and file 5 is byte-equal.
- [x] **6. Run** `cargo test -p zygo --locked --test decision_review`. Check: the step 5 tests fail (`review` gives USAGE, exit 2). The step 1 tests pass.
- [x] **7. Implement** in `decision.rs`:
  - `Sub::Review` and its parse (Interface).
  - The session refusal covers `Sub::Review` with the verb `review`. Directly after it, `Sub::Review` with `!io.tty` prints the terminal line and exits 2.
  - The D§14 row 1 check (`:250`) covers `Sub::Review` too.
  - The match arm `Sub::Review { kind, unit } => review(repo, &top, &lock_path(&common_dir), &set, kind, unit.as_deref(), io)`.
  - `review`: one stderr line per malformed file. Then `Queue::new(&set.valid, kind, unit)`. If it is empty, print `no open records` and exit 0. `statuses: HashMap<id, Status>` comes from `set.valid` and is updated after each write. Loop on `next()`: re-read with `read_bytes` + `parse_record` (that is `shown`). If it fails, print `zygo decision: <id> is not a valid record` on stderr and go on. If the status is no longer `open`, print `zygo decision: <id> is <status>` and go on. Else print the block (Interface), ask, build the `Outcome`, and call `apply(&path, lock, &shown, &o, &(repo.today)())`. `Ok` counts the outcome and records the path; after an overturn it calls `queue.overturned(&id, &set.valid)`. `Changed` prints `<id> changed since it was shown; skipped` and counts as skipped. `Refused(e)` prints `e` and counts as skipped. `Io(e)` prints `e`, then the end lines, and exits 1. Flush `io.out` before each read.
  - `run`: `tty = stdin.is_terminal() && stdout.is_terminal()`, and the `tty` doc becomes `/// Whether stdin and stdout are terminals.`
- [x] **8. Run** `cargo test -p zygo --locked --test decision_review`. Check: all pass. Then run `cargo clippy -p zygo -p zygo-work --all-targets --locked -- -D warnings`. Check: it is clean.
- [x] **9. Commit** the four Files with the message `DR-6: zygo decision review` and the trailers `Zygo-Session: <session>` and `Zygo-Work: phase3:DR-6:code`.
- [ ] **10. Final verification:** `cargo test -p zygo --locked --test decision_review` → all pass. `cargo test -p zygo-work --locked` → all pass. Also run `cargo test -p zygo --locked --test decision_verbs --test decision_cli --test decision_new`. Check: all pass, because `USAGE` did not change.

### Implementation notes

1. `Script` records the probe status only at a read that returns an answer. The EOF read at the end is not counted, so `seen` has the three entries that step 5 names.
2. Prompts end with no newline, so in captured output a header line can start with the previous prompt's text. The test helper `headers` takes the text from the `[i/N]` marker, not from the start of the line.
3. `ask` prints a newline at EOF, so the end lines start on their own line. The action prompt ends with one space (`… q quit `), as the `[y/N] ` prompt of `decide` does.
4. A promotion writes an empty `review_note`. The `p` path asks for no note.
5. `Queue::next` has `#[allow(clippy::should_implement_trait)]`, because Produces gives the name `next`.
6. `review_shows_records_in_dependents_order` also asserts `depends_on <id6> superseded`.
7. Step 8 clippy: `cargo clippy -p zygo -p zygo-work --all-targets --locked -- -D warnings` fails on code outside the Files: `crates/zygo-work/src/gate/transition.rs:10` (`large_enum_variant`), `:158` (`collapsible_if`) and `crates/zygo-work/tests/decision_record.rs:297` (`sliced_string_as_bytes`), with rustc 1.98.1. The DR-6 changes do not touch these files. With those three lints allowed, clippy is clean.

### Coverage

| Acceptance item | Test |
|---|---|
| #12 exit 2 with `ZYGO_SESSION_DIR` set | `review_exits_2_in_an_agent_session`, `the_binary_refuses_…` |
| #12 exit 2 and the terminal line without a terminal (`DecisionIo.tty`) | `review_without_a_terminal_exits_2`, `the_binary_refuses_…` |
| #12 dependents order, largest first, then id; superseded records leave the count | `queue_orders_by_dependents_then_id`, `review_shows_records_in_dependents_order` |
| #12 after an overturn, its open dependents come next with `depends on overturned <id>` | `overturned_moves_…`, `after_an_overturn_its_open_dependents_come_next` |
| #12 each outcome in its file before the next prompt; a second `review` shows only the open records | `each_outcome_is_in_its_file_before_the_next_prompt` |
| #13 changed on disk → `<id> changed since it was shown; skipped`, the file unchanged | `a_record_changed_while_shown_is_skipped` |
| #13 ESC in a title prints as `\u{1b}` | `an_esc_in_a_title_prints_as_an_escape` |
| `q`: the counts, the files, `commit these files; add a Zygo-Decision: line per id` | `q_prints_counts_files_and_the_commit_line` |
| No `docs/decisions/` → `no open records`, exit 0 (and the rest of the D§14 column) | `failure_rows_of_d14`, `a_write_failure_exits_1_…` |
| `--kind`, `--unit` filters | `queue_filters_…`, `failure_rows_of_d14` (c) |

### Author's calls (the spec does not decide these)

1. `Queue` has three methods that the Produces line does not name: `remaining`, `dependents` and `cause`. They keep the counts and the `depends on overturned` cause in one place. `next` keeps the signature that Produces gives.
2. The output layout, the sub-prompt texts, `depends_on <id> <status>` with `(overturned)` in place of the status, the summary line `ratified a, promoted b, overturned c, skipped d`, and the changed files as `docs/decisions/<id>.md` (relative, ready for `git add`).
3. Spec step 6 does not make the end lines conditional, so they are printed even when no file changed.
4. The malformed-file line is `zygo decision: <name>: cannot be reviewed; zygo decision check names the fault`. This follows step 1, which says that `check` names the fault, so the faults are not printed.
5. `Changed` and `Refused` count as `skipped`, and the review goes on. An `Io` failure stops the review, prints the end lines, and exits 1 (D§14: earlier outcomes are kept). `q` and EOF do not count as skipped.
6. A record that is re-read and is no longer valid or `open` is not shown, does not take an `i`, and is not counted.
7. The terminal check comes after the session check and before the repository lookup. Without a terminal, an absent `docs/decisions` therefore exits 2, not 0.
8. `N` is the size of the queue at the start, and moved records keep it.
