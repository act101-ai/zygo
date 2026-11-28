# DR-4 — `zygo decision new`: write a record template

**Spec:** `docs/specs/2026-11-24-decision-records-design.md` D§10.2, D§10.1 (row `new`), D§6.1, D§6.2, D§6.3 (keys that `new` fills), D§6.8 (directory text)
**Files:** `crates/zygo-work/src/decisions/mod.rs`, `crates/zygo-work/src/decisions/new.rs`, `crates/zygo/src/cmd/decision.rs`, `crates/zygo/tests/decision_new.rs`, `crates/zygo/tests/decision_cli.rs`, `crates/zygo-work/src/decisions/record.rs`
**Consumes:** `run_with` (DR-3), `DecisionIo` (DR-3), `read_dir` (DR-2), `Kind` (DR-1), `Class` (DR-1), `valid_id` (DR-1), `FILL_MARKER` (DR-1), `DIR` (DR-1)
**Produces:** `decisions::new::NewArgs { pub kind: Kind, pub title: String, pub unit: String, pub round: u32, pub role: String, pub class: Class, pub depends_on: Vec<String>, pub supersedes: Option<String> }`; `decisions::new::create(top: &Path, a: &NewArgs, model: &str, commit: &str, date: &str) -> Result<PathBuf, String>` (writes `<id>.tmp`, renames it to `<id>.md`, creates `docs/decisions/` when absent, does not commit); `new` (the subcommand `zygo decision new --kind <k> --title <t> --unit <u> --round <n> --role <r> [--class <c>] [--depends-on <id>]… [--supersedes <id>]`, prints the path)
**Acceptance:** `cargo test -p zygo --locked --test decision_new` → all pass. Covers spec Acceptance #1 (`check` reports the new file as malformed until every `<!-- zygo: fill -->` line is replaced; then `check` exits 0) and #2 (two records created in two linked checkouts get different ids; merging both branches gives no conflict); `model` is `unknown` outside a session and is the newest `requests` row's `model` for `role_served = <r>` inside one; each D§10.2 argument refusal (a bad `--unit`, `--round 0`, a role not in the session legend and not `lead`, a `--depends-on` id that is not a valid record); `docs/decisions` a regular file → exit 1 with `zygo decision: docs/decisions is not a directory`.

## Plan

### Premises checked against the live code (phase-3 at `055a24e`)

- Consumed, as they exist now: `zygo::cmd::decision::run_with(cwd: &Path, env: &dyn Fn(&str) -> Option<OsString>, args: &[String], io: &mut DecisionIo) -> i32` and `run_in(repo: &Repo, _env: &dyn Fn(&str) -> Option<OsString>, args: &[String], io: &mut DecisionIo) -> i32` (`crates/zygo/src/cmd/decision.rs:115,227`); `DecisionIo { input: &mut dyn BufRead, out: &mut dyn Write, err: &mut dyn Write, tty: bool }` (`:21`); `Repo { top: &dyn Fn() -> Option<PathBuf>, commits: &CommitsFn }` (`:30`); `decisions::set::read_dir(top: &Path) -> Result<RecordSet, DirError>` (`set.rs:35`); `decisions::record::{Kind, Class}` with `parse(&str) -> Option<Self>` and `name(self) -> &'static str` (`record.rs:31,63`); `valid_id(s: &str) -> bool` (`record.rs:121`); `FILL_MARKER = "<!-- zygo: fill -->"`, `DIR = "docs/decisions"` (`record.rs:7,9`).
- Reused, not consumed by name: `zygo_work::hooks::valid_unit` (`hooks.rs:585`), `zygo_work::hooks::session_id(&SessionDir) -> String` (`hooks.rs:571`), `zygo_core::ids::ulid()` (`ids.rs:2`, upper-case Crockford), `zygo_core::session_view::SessionView::load` and `.legend[].role` (`session_view.rs:17,177`), `zygo_usage::Store::open_read_only` + `requests_for_session` (rows `ORDER BY ts, rowid`, `query.rs:62`), `zygo_core::paths::Paths::from_env().usage_db`, `zygo_work::repo::git` (`repo.rs:189`, `pub(crate)`) with `repo::BOUND` (2 s), the TOML string quoting `toml::Value::String(s).to_string()` (as `crates/zygo-plugins/src/builtin.rs:42`).
- `read_dir` already refuses a `docs/decisions` that is a file or a link before the subcommand runs, with `zygo decision: docs/decisions is not a directory`, exit 1 (`decision.rs:126-131`); the `new` arm inherits it.
- `parse_record` requires `reverse_cost` (`record.rs:279`) but D§10.2 gives `new` no `--reverse-cost`. The template writes it as a fill line (author's call 1), so the record stays malformed until that line is replaced, as Acceptance #1 needs.
- No serve-mode test is needed in code: in serve mode the local store has no row of the session until exit (`zygo-launch/src/serve_client.rs:390`), so the newest-row lookup finds none and gives `unknown` (D§10.2).

**Drift (Files line):** the contract's Files miss three files that this node must touch.
1. `crates/zygo/tests/decision_cli.rs`: its `go` helper builds `Repo { top, commits: &list }` (`decision_cli.rs:35`). New `Repo` fields break that literal. Its `USAGE` constant (`:100`) asserts the usage line, which now names `new`. DR-3's plan expects this ("may add fields to `Repo`", `nodes/DR-3.md:59`).
2. `crates/zygo-work/src/decisions/record.rs`: `valid_role` (`:226`) and `valid_hex` (`:217`) are private. `new.rs` needs them, so they become `pub(crate)`. The only alternative is to copy them.
3. (Not changed) `crates/zygo/src/cmd/mod.rs:361` help text "Decision records: ls, show, deps, check" leaves out `new`. This plan leaves the file as it is. The lead can add it to Files if wanted.

### Interface (Produces, final)

In `crates/zygo-work/src/decisions/new.rs` (`pub mod new;` in `decisions/mod.rs`):

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewArgs { pub kind: Kind, pub title: String, pub unit: String, pub round: u32, pub role: String,
                     pub class: Class, pub depends_on: Vec<String>, pub supersedes: Option<String> }
/// D§10.2 argument checks that need no repository: title (D§6.3: 1..=120 bytes, no `\n`/`\r`), unit (`valid_unit`),
/// round >= 1, role (`valid_role`, and in a session: in `legend` or `lead`), at most 64 `depends_on`. Err is the line.
pub fn check_args(a: &NewArgs, legend: Option<&[String]>) -> Result<(), String>;
/// The `model` of the newest `requests` row of `session` with `role_served = role`; `unknown` when none, when the
/// store cannot be opened read-only, or when the model is empty.
pub fn session_model(usage_db: &Path, session: &str, role: &str) -> String;
/// `git rev-parse --verify -q HEAD` of `top` under `repo::BOUND`; `Some` only for 40 or 64 lower-case hex.
pub fn head(top: &Path) -> Option<String>;
/// Today, UTC, `YYYY-MM-DD` (`time::OffsetDateTime::now_utc().date()`).
pub fn today_utc() -> String;
/// Writes `docs/decisions/<id>.tmp` (create_new, write, sync_all), renames it to `<id>.md`; creates `docs/decisions/`
/// when absent; does not commit. Err: `zygo decision: cannot write <path>: <io error>`; a failure after the open
/// leaves the `.tmp` file.
pub fn create(top: &Path, a: &NewArgs, model: &str, commit: &str, date: &str) -> Result<PathBuf, String>;
```

In `crates/zygo/src/cmd/decision.rs`, `Repo` gains three fields (doc comments: "as values so tests run no git and read no clock"):

```rust
pub head: &'a dyn Fn(&Path) -> Option<String>,   // new::head outside tests
pub today: &'a dyn Fn() -> String,               // new::today_utc outside tests
pub usage_db: &'a Path,                          // Paths::from_env().usage_db outside tests
```

The `new` subcommand behaves as follows:
- Order: parse → `check_args` (before `top()`, so argument refusals run no git) → `top()` → `read_dir` → each `--depends-on`/`--supersedes` id through the existing `find` → `head` → `model` → `create` → print `esc(path)`.
- Exit codes: a parse error (missing required flag, unknown flag, bad `--kind`/`--class`, a `--round` that is not a `u32`) prints `USAGE` and exits 2. A `check_args` refusal exits 2. An id that is not a valid record prints `zygo decision: <id> is not a valid record` and exits 1. A `head` of `None` prints `zygo decision: cannot read HEAD` and exits 1. A `create` Err prints its line and exits 1.
- `model`: when `env("ZYGO_SESSION_DIR")` is non-empty, `session_model(repo.usage_db, &session_id(&sd), role)`; else `unknown`.
- `legend`: `SessionView::load(&sd)` roles; `None` outside a session or when the view does not load.

Refusal lines (`check_args`):
- `zygo decision new: --title must be one line of 1 to 120 bytes`
- `zygo decision new: --unit <u> is not a unit id`
- `zygo decision new: --round must be 1 or more`
- `zygo decision new: --role <r> is not a role name`
- `zygo decision new: --role <r> is not a role of this session`
- `zygo decision new: more than 64 --depends-on`

The new `USAGE`:
`zygo decision: usage: zygo decision new --kind <k> --title <t> --unit <u> --round <n> --role <r> [--class <c>] [--depends-on <id>]... [--supersedes <id>] | ls [--status <s>] [--kind <k>] [--unit <prefix>] [--pending] | show <id> | deps <id> | check`

The template that `create` writes. `q(s)` is `toml::Value::String(s).to_string()`. Lines in `[…]` appear only when they apply:

```
+++
id = "<id>"
kind = "<kind>"
status = "open"
class = "<class>"
title = <q(title)>
unit = <q(unit)>
round = <round>
role = <q(role)>
model = <q(model)>
commit = <q(commit)>
date = <q(date)>
depends_on = [<q(id)>, …]
# <!-- zygo: fill --> reverse_cost = "low", "medium" or "high" (low: one node or less; high: a public contract, a stored format, a migration or data)
[supersedes = <q(id)>]
+++
[
## Finding

<!-- zygo: fill --> The reviewer's finding, with its location, failure and evidence.
]
## Choice

<!-- zygo: fill --> One paragraph: what the agent chose.            | open-finding: What stays open.

## Alternatives rejected

<!-- zygo: fill --> Each alternative, and why it was rejected.      | open-finding: The fixes considered, and why none was made.

## Evidence

<!-- zygo: fill --> The file:line, test output or document text that the choice rests on.

## Cost to reverse

<!-- zygo: fill --> What a reversal changes, and which later work it touches.
```

`## Finding` is present only for `accepted-limit`, `compromise` and `open-finding`. The body starts with `\n` after the closing `+++` line, as `BODY` in `decision_cli.rs:11`.

### Steps

Test file `crates/zygo/tests/decision_new.rs`. Its helpers follow `decision_cli.rs`: `mod common;`, `C40`, `id(n)`, `md(n)`, `text(…)`/`put(top, n, deps)` copied from `decision_cli.rs:10-25`, `git(dir, args)` from `:39`. Also:

```rust
struct Ran { code: i32, out: String, err: String }
/// `run_in` with `top`, HEAD `C40`, today `2026-11-24`, the usage store `db`, and `env` for ZYGO_SESSION_DIR.
fn go(top: &Path, env: &dyn Fn(&str) -> Option<OsString>, db: &Path, args: &[&str]) -> Ran;
const BASE: [&str; 11] = ["new", "--kind", "decision", "--title", "T", "--unit", "phase-3:N12", "--round", "1", "--role", "code"];
fn new(top: &Path, extra: &[&str]) -> Ran { /* go(top, &|_| None, &top.join("none.db"), BASE ++ extra) */ }
/// Each line that holds FILL_MARKER replaced: the reverse_cost line by `reverse_cost = "low"`, others by `Filled.`
fn fill(path: &Path);
fn session(sd: &Path, roles: &[&str]) // SessionView { session: "s1", legend: roles as LegendEntry, ..Default } .save
fn rows(db: &Path, rows: &[(i64, &str, &str, &str)]) // Store::open, insert_request(RequestRow { ts, session, role_served: Some, model, ..Default })
```

- [x] **1. Test: the template is malformed until filled, then valid (D§17 #1).** `new_writes_a_template_that_check_refuses_until_filled`:
  ```rust
  let r = new(top, &[]);                                   // tempdir with no docs/
  assert_eq!((r.code, r.err.as_str()), (0, ""));
  let p = PathBuf::from(r.out.trim_end());
  let name = p.file_name().unwrap().to_str().unwrap().to_string();
  assert_eq!(p, top.join("docs/decisions").join(&name));
  assert!(matches!(name_kind(&name), NameKind::Record(_)));
  ```
  Assert `run(top, ["check"])` gives `(1, "<name>: missing key reverse_cost\n<name>: empty section ## Choice\n<name>: empty section ## Alternatives rejected\n<name>: empty section ## Evidence\n<name>: empty section ## Cost to reverse\n")`. Then `fill(&p)` and assert `check` gives `(0, "", "")`. `read_dir(top).unwrap().valid[0]` equals: kind `Decision`, status `Open`, class `None`, title `T`, unit `phase-3:N12`, round 1, role `code`, model `unknown`, commit `C40`, date `2026-11-24`, `depends_on` empty, `reverse_cost` `low`, `supersedes` `None`, every operator key `None`.
- [x] **2. Test: finding kinds get `## Finding`; `--class`, `--depends-on`, `--supersedes` and a quoted title are written.** `new_fills_finding_class_links_and_title`. Put records 1 and 2 with `put`. Run `go(top, &|_| None, &top.join("none.db"), &["new", "--kind", "open-finding", "--title", "a \"b\" \\ c", "--unit", "phase-3:N12", "--round", "1", "--role", "code", "--class", "security", "--depends-on", &id(1), "--depends-on", &id(2), "--supersedes", &id(1)])`. Assert `check` lists `<name>: missing key reverse_cost` then `<name>: empty section ## Finding` first among the sections. After `fill`, the record parsed by `parse_record` has kind `OpenFinding`, class `Security`, `depends_on == [id(1), id(2)]`, `supersedes == Some(id(1))`, title `a "b" \ c`. The record's text contains `<!-- zygo: fill --> What stays open.` before the fill.
- [x] **3. Test: two linked checkouts (D§17 #2).** `two_linked_checkouts_get_different_ids_and_merge_cleanly`. Steps: `git init -q` in `a`; commit `--allow-empty`; `git worktree add -q ../b -b side`; `new(a)` and `new(b)` (each `go` with its own `top`); `git add . && git commit -q -m r` in each; in `a`, `git merge -q --no-edit side` succeeds (the test `git` helper asserts the exit status). Assert the two printed file names differ and `a/docs/decisions` holds both.
- [x] **4. Test: `model` (D§10.2).** `model_is_unknown_outside_a_session_and_the_newest_row_inside`.
  - Setup: `session(sd, &["code", "review"])`; `rows(db, &[(1,"s1","code","m-old"), (2,"s1","code","m-new"), (3,"s1","review","m-rev"), (5,"s2","code","m-other")])`; `env = |k| (k == "ZYGO_SESSION_DIR").then(|| sd.into())`.
  - Assert: `--role code` writes `model = "m-new"`; `--role lead` writes `model = "unknown"` (no row; this is also the serve-mode case); the same `--role code` with `env = |_| None` writes `model = "unknown"`; a session whose `db` path does not exist writes `unknown`.
  - Read `model` with `read_dir` after `fill`.
- [x] **5. Test: argument refusals (D§10.2).** `argument_refusals_exit_2_and_write_nothing`.
  - Table of `(args, code, err)`:
    - `--unit nounit` → `(2, "zygo decision new: --unit nounit is not a unit id\n")`;
    - `--round 0` → `(2, "zygo decision new: --round must be 1 or more\n")`;
    - `--round x`, a missing `--kind`, `--kind bogus`, `--class bogus` and `--frob` → `(2, USAGE)`;
    - `--title ""`, `--title "a\nb"` and a 121-byte title → `(2, "zygo decision new: --title must be one line of 1 to 120 bytes\n")`;
    - outside a session, `--role Bad` → `(2, "zygo decision new: --role Bad is not a role name\n")`;
    - in a session with legend `[code]`, `--role review` → `(2, "zygo decision new: --role review is not a role of this session\n")`, and `--role lead` → exit 0;
    - 65 × `--depends-on id(1)` → `(2, "zygo decision new: more than 64 --depends-on\n")`.
  - For each refused case, assert `out` is empty and `top.join("docs")` does not exist.
- [x] **6. Test: ids that are not valid records (D§10.2).** `depends_on_and_supersedes_must_name_valid_records`. Put a valid record `id(1)` and a malformed `md(2)` (`"x"`). For each of `--depends-on id(9)`, `--depends-on nope`, `--depends-on id(2)` and `--supersedes id(9)`, assert `(1, "", "zygo decision: <id> is not a valid record\n")`, where `<id>` is the argument as given. `fs::read_dir(top/docs/decisions)` still holds only `md(1)` and `md(2)`.
- [x] **7. Test: directory and write failures (D§6.8, D§10.2, review focus 44).**
  - `docs_decisions_a_file_exits_1`: `docs/decisions` is a regular file, so `new` gives `(1, "", "zygo decision: docs/decisions is not a directory\n")`.
  - `#[cfg(unix)] a_write_failure_exits_1_and_a_left_tmp_is_ignored`: skip with `eprintln!` when `libc::geteuid() == 0`. Make `docs/decisions` with mode `0o500`. `new` exits 1, with `err` starting `zygo decision: cannot write ` and `out` empty. The directory holds no `.md` file. Restore `0o700`. Then write `format!("{}.tmp", id(3))` with text `"+++\npartial"`, as a failed rename leaves it. `check` gives `(0, "", "")` and `ls` gives `(0, "", "")`.
- [x] **8. Test (ignored): the binary arm with real git.** `#[ignore = "timing: git calls run under the 2 s production bound (BOUND)"] binary_new_writes_head`. In a `git init` repo with one commit, run `CARGO_BIN_EXE_zygo decision new …BASE[1..]` with `common::scratch_home`, `.env_remove("ZYGO_SESSION_DIR")` and `GIT_CEILING_DIRECTORIES`. Assert exit 0 and that stdout names a file that exists. After `fill`, the record parses with `commit == git rev-parse HEAD` and `model == "unknown"`.
- [x] **9. Run the tests and see them fail.** `env -u ZYGO_SESSION_DIR cargo test -p zygo --locked --test decision_new` fails to compile: `zygo_work::decisions::new` and the `Repo` fields do not exist yet.
- [x] **10. Implement `decisions/new.rs`** with the signatures above. Add `pub mod new;` to `decisions/mod.rs`. In `record.rs`, make `valid_hex` and `valid_role` `pub(crate)`. `create` calls the private `fn template(id: &str, a: &NewArgs, model: &str, commit: &str, date: &str) -> String`, which writes the text fixed above.
- [x] **11. Implement the `new` arm in `cmd/decision.rs`.**
  - Add `Sub::New(NewArgs)` and `fn parse_new(rest: &[String]) -> Option<NewArgs>`. In `parse_new`, a later flag replaces an earlier one; `--depends-on` appends; `--class` defaults to `Class::None`.
  - Add the `Repo` fields, the `check_args` call before `top()`, `fn new(repo, env, top, set, a, io) -> i32` in the order fixed above, and the new `USAGE`.
  - Rename `_env` to `env` in `run_in`.
  - In `run_with`, pass `head: &new::head, today: &new::today_utc, usage_db: &Paths::from_env().usage_db`.
  - In `decision_cli.rs`, add `head: &|_| None, today: &String::new, usage_db: Path::new("")` to the `go` literal and replace its `USAGE` constant with the new line.
- [x] **12. Run the tests and see them pass.**
  - `env -u ZYGO_SESSION_DIR cargo test -p zygo --locked --test decision_new` → all pass; 1 ignored.
  - `env -u ZYGO_SESSION_DIR cargo test -p zygo --locked --test decision_cli` → all pass.
  - `env -u ZYGO_SESSION_DIR cargo test -p zygo-work --locked --test decision_record --test decision_set --test decision_transition` → all pass.
  - `env -u ZYGO_SESSION_DIR cargo test -p zygo --locked --test decision_new -- --ignored binary_new_writes_head` → pass. This runs on demand only; it is not part of CI.
- [x] **13. Commit** with the message `feat(work): decision new (DR-4)` and the trailers `Zygo-Session: <session>` and `Zygo-Work: <unit>`, each on its own line at the end.

### Author's checks

- **Spec coverage:**
  - D§10.2 fills: id (step 1, name), `status`, `date`, `commit` (1, 8), `model` (4).
  - D§10.2 validation: unit, round, role in and outside a session, ids (5, 6).
  - D§10.2 writes: `.tmp` then rename, `docs/decisions/` created (1, 7); no commit (nothing in the steps stages a file).
  - D§10.1 row `new` and USAGE (5, 11). D§6.1 name form (1). D§6.2 front matter and sections (1, 2). D§6.3 keys `new` fills (1, 2). D§6.8 directory text (7). D§17 #1 (1) and #2 (3).
- **Review focus:** item 44 (a write failure leaves at most the `.tmp`, which readers ignore) → step 7. Graph item 75 (no new test reads a clock or spawns an agent): date and HEAD are injected, and no test spawns `claude` or `opencode`. The one git-bound binary test is `#[ignore = "timing: …"]`.
- **Interface match:** DR-14 consumes only the subcommand `new` (`nodes/DR-14.md:5`), with the flags of D§10.1; `--class security` is covered in step 2. DR-5 and DR-6 add arms to `run_in` and can use `Repo.today`. No node consumes `NewArgs` or `create` by name.
- **Live code:** each cited symbol was opened at the line given above. `find`, `line`, `read_dir`, `valid_unit`, `session_id`, `ids::ulid`, `requests_for_session` and the `q` quoting are reused, not rewritten.

### Author's calls

1. **`reverse_cost`:** written as a TOML comment fill line. The key stays absent (`missing key reverse_cost`) until the agent replaces the line. D§10.2 gives no flag for it, and `parse_record` requires it.
2. **`## Finding`:** written only for the three finding kinds, not for a `decision` of a stop class. In a `decision` record that section is not checked, so an unreplaced marker there would break D§17 #1 ("malformed until every fill line is replaced").
3. **Exit codes:** argument refusals exit 2, as usage errors do. An id that is not a valid record exits 1 with the existing `show`/`deps` text. A missing HEAD (`cannot read HEAD`) exits 1. The refusal texts are new; the spec does not fix them.
4. **`--title` check:** `--title` is checked by D§6.3 (1 to 120 bytes, one line), and more than 64 `--depends-on` are refused. Without these checks, `new` would write a record that is malformed for a reason the agent did not cause.
5. **Unreadable view:** inside a session whose `view.json` does not load, `--role` is checked by the name regex only, not refused.
6. **Serve mode:** there is no serve-mode branch; no local row gives `unknown`.
7. **Printed path:** the path printed is the absolute path that `create` returns.
8. **Left `.tmp` file:** a failed rename leaves the `.tmp` file; D§10.2 allows it.

### Review round 1

1. Fixed in beeaa1d: `-h`/`--help` that is the value of a flag (`--title -h`) is that value, not a help request; the record is written.
2. Fixed in beeaa1d: `parse_record` refuses a front matter that still holds the fill marker (`unfilled hint in front matter`).
3. Fixed in beeaa1d: `new --kind decision` with a stop class writes `## Finding` with a hint that has no fill marker; `check` does not need it. This replaces author's call 2.
4. Rejected for this node: the top-level help line in `cmd/mod.rs:361` is outside the node's files; carried to the next row that changes `cmd/mod.rs`.
5. Fixed in beeaa1d (tests): unreadable HEAD exits 1 and writes nothing; `new` outside a repository exits 2; an empty session model gives `unknown`. No code change was needed. The `.tmp` left on a write failure has no injection seam and stays covered by the reader test only.
6. Rejected: D§10.2 asks only "present as a valid record"; a spec gap, not a break.
7. Rejected: matches spec; `new` does not commit.
8. Rejected: the only caller passes the process env; no failure.
9. Fixed in beeaa1d: the test git helper sets `GIT_CONFIG_GLOBAL` to an empty file, `GIT_CONFIG_NOSYSTEM=1`, `HOME` and `XDG_CONFIG_HOME` to a temp directory, and the identity by env.
