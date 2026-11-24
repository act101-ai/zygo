# DR-5 — Operator verbs `ratify` and `overturn`: byte-preserving outcome writes

**Spec:** `docs/specs/2026-11-24-decision-records-design.md` D§11.1, D§11.3 (Ratify, Promote, Overturn; the re-read before each write; temporary file and rename), D§10.1 (rows `ratify`, `overturn`; `--promote` without `--as`; the refusal texts), D§6.6 (operator rows; overturn of a promoted record refused), D§6.2 (`toml_edit`: unknown keys, comments and body byte-equal), D-C5, D§14 (column `review, ratify, overturn`)
**Files:** `crates/zygo-work/src/decisions/mod.rs`, `crates/zygo-work/src/decisions/edit.rs`, `crates/zygo/src/cmd/decision.rs`, `crates/zygo/tests/decision_verbs.rs`, `crates/zygo-work/Cargo.toml`, `Cargo.lock`, `crates/zygo/tests/decision_cli.rs`, `crates/zygo/tests/decision_new.rs`
**Consumes:** `run_with` (DR-3), `DecisionIo` (DR-3), `read_dir` (DR-2), `parse_record` (DR-1), `Record` (DR-1), `Status` (DR-1)
**Produces:** `decisions::edit::Promotion` (`Adr | Spec | Rule`); `decisions::edit::Outcome` (`Ratify { note: String, promote: Option<(Promotion, String)> } | Overturn { reason: String }`); `decisions::edit::EditError` (`Changed | Refused(String) | Io(String)`); `decisions::edit::apply(path: &Path, shown: &[u8], outcome: &Outcome, today: &str) -> Result<(), EditError>` (re-reads the file; `Changed` when its bytes differ from `shown`; writes a temporary file in `docs/decisions/` and renames it); `decisions::edit::allowed(r: &Record, outcome: &Outcome) -> Result<(), String>`; `ratify` and `overturn` (the subcommands `zygo decision ratify <id> [--note <text>] [--promote adr|spec|rule --as <text>] [--yes]` and `zygo decision overturn <id> --reason <text> [--yes]`)
**Acceptance:** `cargo test -p zygo --locked --test decision_verbs` → all pass. Covers spec Acceptance #14 (`ratify --promote rule --as <text> --yes` writes `status`, `reviewed`, `promoted_to`, `promotion` and keeps unknown keys, comments and the body byte-equal; `overturn` of that record exits 1 with `zygo decision: <id> is promoted; remove the promotion by hand first` and changes nothing); `ratify` and `overturn` exit 2 with `zygo decision <verb>: run it in your own shell, not in an agent session` when `ZYGO_SESSION_DIR` is set; `ratify` of a non-`open` record exits 1 with `zygo decision: <id> is <status>`; `--promote` without `--as` exits 2; `overturn` without a non-empty `--reason` is refused; without `--yes`, `n` at the `[y/N]` prompt changes nothing; `apply` returns `Changed` and leaves the file byte-equal when the file changed after it was shown.

## Plan

### Premises checked against the live code (phase-3 at `0b1f22e`)

- Consumed, as they exist now:
  - `zygo::cmd::decision::run_with(cwd: &Path, env: &dyn Fn(&str) -> Option<OsString>, args: &[String], io: &mut DecisionIo) -> i32` (`crates/zygo/src/cmd/decision.rs:316`), and the test seam `run_in(repo: &Repo, env: &dyn Fn(&str) -> Option<OsString>, args: &[String], io: &mut DecisionIo) -> i32` (`:174`).
  - `DecisionIo { input: &mut dyn BufRead, out: &mut dyn Write, err: &mut dyn Write, tty: bool }` (`:28`).
  - `Repo { top, commits, head, today: &dyn Fn() -> String, usage_db }` (`:37`). `today` is already injected, so the tests read no clock.
  - `decisions::set::read_dir(top: &Path) -> Result<RecordSet, DirError>` (`set.rs:35`). It returns an empty set for an absent `docs/decisions`. A file or a link gives `NotDirectory`, which `run_in` already turns into exit 1 (`decision.rs:197-206`).
  - `decisions::record::parse_record(stem: &str, bytes: &[u8]) -> Result<Record, Vec<String>>` (`record.rs:242`). `Record` (`:67`) has `status: Status`, `promoted_to: Option<String>` and `body: String` (the text after the closing `+++` line).
  - `Status::{Open, Ratified, Overturned, Fixed, Superseded}` with `name(self) -> &'static str` (`:43`).
- Reused, not consumed by name:
  - `record::read_bytes(path) -> Result<Vec<u8>, Vec<String>>`: the lstat-first open (`record.rs:363`).
  - `record::DIR`.
  - `zygo_core::ids::ulid()`.
  - `decision.rs` helpers `session_dir`, `line`, `find`, `wants_help`, and the `show` print loop (`:131-170`, `:257-270`).
  - The temporary-file pattern of `new::create` (`new.rs`: `create_new`, `write_all`, `sync_all`, `rename`).
  - The decor-copy pattern of `zygo-config/src/profile.rs:302`.
- `toml_edit` 0.25.19 (workspace `Cargo.toml:67`) was checked in a scratch crate:
  - (a) `DocumentMut::to_string` turns every CRLF into LF.
  - (b) `doc.insert(k, v)` on an existing key drops the comment lines above that key, because they are key decor. The value must be replaced in place through `get_mut(k)…as_value_mut()`, with its old decor copied over.
  - (c) New keys go after the last root key-value. They go before a trailing comment block and before any `[table]`.
  - (d) `Value::from("a\nb")` writes a multi-line `"""` string, so new values are built from an escaped one-line basic string.

**Drift (Files line):** this node must touch four files that are not on its Files line.
1. `crates/zygo-work/Cargo.toml` gets `toml_edit.workspace = true`. `zygo-work` does not depend on `toml_edit` yet, and D§6.2 requires it.
2. `Cargo.lock` gets the one new dependency line under `zygo-work`. Without it, `--locked` fails.
3. `crates/zygo/tests/decision_cli.rs:101` and `crates/zygo/tests/decision_new.rs` (`const USAGE`) assert the usage line, which now names `ratify` and `overturn`. DR-4 made the same change to `decision_cli.rs`.
4. Not changed: `crates/zygo/src/cmd/mod.rs:361`, the help text "Decision records: ls, show, deps, check". DR-4 left it as it is, and this node does the same.

### Interface (Produces, final)

`crates/zygo-work/src/decisions/edit.rs` (`pub mod edit;` in `decisions/mod.rs`):

```rust
/// The promotion target of `ratify --promote` (D§11.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Promotion { Adr, Spec, Rule }
impl Promotion {
    /// `adr`, `spec`, `rule`; exact, case-sensitive.
    pub fn parse(s: &str) -> Option<Self>;
    pub fn name(self) -> &'static str;
}
/// One operator outcome (D§11.3, D§6.6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome { Ratify { note: String, promote: Option<(Promotion, String)> }, Overturn { reason: String } }
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    /// The file's bytes differ from `shown`; nothing written.
    Changed,
    /// The transition is not allowed, or the file is not a valid record; the stderr line; nothing written.
    Refused(String),
    /// A read or write failed; the stderr line, naming the file.
    Io(String),
}
/// D§6.6 operator rows. Ratify: `status` must be `open`. Overturn: `status` must be `open` or `ratified`, and
/// `promoted_to` absent. Err lines: `zygo decision: <id> is <status>` and
/// `zygo decision: <id> is promoted; remove the promotion by hand first` (the status test first).
pub fn allowed(r: &Record, outcome: &Outcome) -> Result<(), String>;
/// Re-reads `path` with `read_bytes`; `Changed` when the bytes differ from `shown`; else checks `parse_record` and
/// `allowed`, sets the outcome keys with `toml_edit`, checks the result with `parse_record`, writes
/// `<dir>/<id>.<ULID>.tmp` (create_new, write_all, sync_all) and renames it to `path`.
pub fn apply(path: &Path, shown: &[u8], outcome: &Outcome, today: &str) -> Result<(), EditError>;
```

`allowed` is an addition to the Produces line. The CLI refuses before the prompt, and `apply` (and DR-6) refuse again after the re-read; one function owns that rule.

Keys that `apply` writes, in this order. A key that exists is replaced in place with its decor kept; a key that is absent is appended:
- Ratify: `status = "ratified"`, `reviewed = <today>`, `review_note = <note>` (may be `""`); with a promotion also `promoted_to = <adr|spec|rule>` and `promotion = <text>`.
- Overturn: `status = "overturned"`, `reviewed = <today>`, `review_note = <reason>`.

The algorithm of `apply` after the checks (the tests do not decide it):
1. `text = str::from_utf8(&bytes)`. `parse_record` has accepted it.
2. `open = text.find('\n') + 1`.
3. `front_end = text.len() - r.body.len()`.
4. `close = text[..front_end].trim_end_matches(['\n', '\r']).len() - 3`.
5. `front = &text[open..close]`.
6. Parse `front` as `toml_edit::DocumentMut` and set each key with `set(doc, k, v)`. If `doc.get_mut(k).and_then(Item::as_value_mut)` is `Some(old)`: keep `d = old.decor().clone()`, assign `*old = basic(v)`, then `*old.decor_mut() = d`. Else: `doc.insert(k, Item::Value(basic(v)))`.
7. `basic(v)` parses `"` + escaped `v` + `"` as a `toml_edit::Value`. The escapes: `\` → `\\`, `"` → `\"`, LF → `\n`, CR → `\r`, TAB → `\t`, and any other char below U+0020 or U+007F → `\uXXXX`.
8. `rendered = doc.to_string()`. When `&text[..open] == "+++\r\n"`: `rendered = rendered.replace("\r\n", "\n").replace('\n', "\r\n")`.
9. `new = text[..open] + rendered + text[close..]`. The two `+++` lines and the body are copied as bytes.
10. `parse_record(stem, new)` Err(faults) → `Refused("zygo decision: <id>: the outcome would make the record malformed: <faults joined by "; ">")`.

Error lines:
- `Io("zygo decision: cannot read <path>: <faults joined by "; ">")` for the re-read.
- `Io("zygo decision: cannot write <path>: <io error>")` for the temporary file (its path) or for the rename (`path`). On a write failure the temporary file is removed (best effort).
- `Refused("zygo decision: <id> is not a valid record")` when the re-read bytes do not parse.

`crates/zygo/src/cmd/decision.rs`:
- `Sub` gains `Decide(DecideArgs)`, with `struct DecideArgs { verb: &'static str, id: String, outcome: Outcome, yes: bool }` (`verb` is `"ratify"` or `"overturn"`).
- `parse` gains `("ratify", [id, rest @ ..])` and `("overturn", [id, rest @ ..])` through `fn parse_decide(verb: &'static str, id: &str, rest: &[String]) -> Option<DecideArgs>`. A later flag replaces an earlier one. `parse_decide` gives `None` (`USAGE`, exit 2) when:
  - there is an unknown flag or a missing value;
  - `--promote` is not `adr|spec|rule`;
  - `--promote` comes without `--as`, or `--as` without `--promote`;
  - `--as ""`;
  - `overturn` has no `--reason`, or has `--reason ""`.
- `wants_help` also skips the values of `--note`, `--promote`, `--as` and `--reason`.
- `USAGE` becomes `zygo decision: usage: zygo decision new --kind <k> --title <t> --unit <u> --round <n> --role <r> [--class <c>] [--depends-on <id>]... [--supersedes <id>] | ls [--status <s>] [--kind <k>] [--unit <prefix>] [--pending] | show <id> | deps <id> | check | ratify <id> [--note <text>] [--promote adr|spec|rule --as <text>] [--yes] | overturn <id> --reason <text> [--yes]`.
- The order in `run_in`:
  1. help, then parse.
  2. `Decide` with `session_dir(env).is_some()` → `zygo decision <verb>: run it in your own shell, not in an agent session`, exit 2.
  3. `top()`.
  4. `Decide` with `fs::symlink_metadata(top.join(DIR))` giving NotFound → stdout `no open records`, exit 0 (D§14 row 1).
  5. `read_dir` (as now).
  6. `fn decide(repo: &Repo, top: &Path, set: &RecordSet, a: &DecideArgs, io: &mut DecisionIo) -> i32`.
- `decide`:
  1. `find` → exit 1.
  2. `shown = read_bytes(<top>/docs/decisions/<id>.md)`. When it fails, or `parse_record(id, shown)` fails → `zygo decision: <id> is not a valid record`, exit 1.
  3. `allowed(&rec, &a.outcome)` Err → that line, exit 1.
  4. Unless `--yes`: print `shown` with the `show` loop (each line through `esc`), then write `<verb> <id>? [y/N] ` (no newline) to stdout and flush. Read one line. Unless the trimmed answer is `y` or `yes` (ASCII case-insensitive) → stderr `zygo decision: <id> not changed`, exit 1. EOF counts as no.
  5. `apply(path, &shown, &a.outcome, &(repo.today)())`:
     - Ok → stdout `esc(path)`, exit 0.
     - `Changed` → stderr `<id> changed since it was shown; skipped`, exit 1.
     - `Refused(s)` or `Io(s)` → stderr `s`, exit 1.
- The `show` print loop moves into `fn print_lines(w: &mut dyn Write, bytes: &[u8])`. `show` and `decide` both call it.

### Steps

- [x] **1. Dependency.** Add `toml_edit.workspace = true` to `[dependencies]` of `crates/zygo-work/Cargo.toml`, after `toml.workspace = true`, then run `cargo check -p zygo-work`. Check: it builds, and `git diff Cargo.lock` adds only the line `"toml_edit",` to the `zygo-work` dependency list.

- [x] **2. Write the failing `apply` tests** in the new `crates/zygo/tests/decision_verbs.rs`. Header and fixtures:

```rust
//! DR-5: `zygo decision ratify` and `overturn` (D§11.1, D§11.3, D§10.1, D§6.6, D§6.2, D§14, D§17 #14).
use std::ffi::OsString;
use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use zygo::cmd::decision::{run_in, DecisionIo, Repo};
use zygo_work::decisions::edit::{allowed, apply, EditError, Outcome, Promotion};
use zygo_work::decisions::record::parse_record;

const C40: &str = "6e7fe2f0c1d2e3f4a5b6c7d8e9f0011223344556";
const ID: &str = "DDR-01JDQ2Y7Z8K4M5N6P7Q8R00001";
const TODAY: &str = "2026-12-01";
const BODY: &str = "\n## Choice\n\nC.\n\n## Alternatives rejected\n\nA.\n\n## Evidence\n\nE.\n\n## Cost to reverse\n\nR.\n\n## Unknown section\n\nKept   \n";
/// D§17 #14: a comment above a key, a comment after a value, an unknown inline key, a closing comment, an unknown table, an unknown section.
fn rich(status: &str) -> String {
    format!("+++\nid = \"{ID}\"\n# kind is frozen\nkind = \"decision\"\nstatus = \"{status}\"  # set by the operator\nclass = \"none\"\ntitle = \"T\"\nunit = \"phase-3:N12\"\nround = 1\nrole = \"code\"\nmodel = \"unknown\"\ncommit = \"{C40}\"\ndate = \"2026-11-24\"\ndepends_on = []\nreverse_cost = \"low\"\nzz_unknown = {{ a = 1 }}   # kept\n# closing comment\n\n[extra]\nnote = \"kept\"\n+++\n{BODY}")
}
const PROMOTED: &str = "reviewed = \"2026-12-01\"\nreview_note = \"\"\npromoted_to = \"rule\"\npromotion = \"Add the rule\"\n";
/// `rich` after `ratify --promote rule --as "Add the rule"` on TODAY.
fn rich_promoted() -> String {
    rich("ratified").replace("zz_unknown = { a = 1 }   # kept\n", &format!("zz_unknown = {{ a = 1 }}   # kept\n{PROMOTED}"))
}
/// A valid record with `status` and `extra` root key lines before the closing `+++`.
fn plain(status: &str, extra: &str) -> String {
    format!("+++\nid = \"{ID}\"\nkind = \"decision\"\nstatus = \"{status}\"\nclass = \"none\"\ntitle = \"T\"\nunit = \"phase-3:N12\"\nround = 1\nrole = \"code\"\nmodel = \"unknown\"\ncommit = \"{C40}\"\ndate = \"2026-11-24\"\ndepends_on = []\nreverse_cost = \"low\"\n{extra}+++\n{BODY}")
}
const RATIFIED: &str = "reviewed = \"2026-11-30\"\nreview_note = \"\"\n";
fn dir(top: &Path) -> PathBuf { let d = top.join("docs/decisions"); fs::create_dir_all(&d).unwrap(); d }
fn put(top: &Path, text: &str) -> PathBuf { let p = dir(top).join(format!("{ID}.md")); fs::write(&p, text).unwrap(); p }
fn promote() -> Outcome { Outcome::Ratify { note: String::new(), promote: Some((Promotion::Rule, "Add the rule".into())) } }
fn overturn(reason: &str) -> Outcome { Outcome::Overturn { reason: reason.into() } }
```

Tests (assertions as written; each ends with the file's bytes checked):

```rust
#[test] fn apply_promote_keeps_unknown_keys_comments_and_body_byte_equal() { // D§17 #14, D§6.2, review focus 39
    let t = tempfile::tempdir().unwrap();
    let p = put(t.path(), &rich("open"));
    let shown = fs::read(&p).unwrap();
    assert_eq!(apply(&p, &shown, &promote(), TODAY), Ok(()));
    assert_eq!(fs::read_to_string(&p).unwrap(), rich_promoted());
    let r = parse_record(ID, &fs::read(&p).unwrap()).unwrap();
    assert_eq!((r.promoted_to.as_deref(), r.promotion.as_deref(), r.reviewed.as_deref()), (Some("rule"), Some("Add the rule"), Some(TODAY)));
}
#[test] fn apply_keeps_a_crlf_record_byte_equal_and_writes_crlf_lines() { // review focus 39
    let t = tempfile::tempdir().unwrap();
    let crlf = |s: String| s.replace('\n', "\r\n");
    let p = put(t.path(), &crlf(rich("open")));
    let shown = fs::read(&p).unwrap();
    assert_eq!(apply(&p, &shown, &promote(), TODAY), Ok(()));
    assert_eq!(fs::read_to_string(&p).unwrap(), crlf(rich_promoted()));
}
#[test] fn apply_returns_changed_and_leaves_the_file_when_it_changed_after_it_was_shown() { // D§11.3, review focus 39
    let t = tempfile::tempdir().unwrap();
    let p = put(t.path(), &plain("open", ""));
    let shown = fs::read(&p).unwrap();
    let edited = plain("open", "").replace("title = \"T\"", "title = \"T2\"");
    fs::write(&p, &edited).unwrap();
    assert_eq!(apply(&p, &shown, &promote(), TODAY), Err(EditError::Changed));
    assert_eq!(fs::read_to_string(&p).unwrap(), edited);
}
#[test] fn apply_writes_a_new_file_and_renames_it() { // D§11.3 temporary file + rename
    let t = tempfile::tempdir().unwrap();
    let p = put(t.path(), &plain("open", ""));
    let old = t.path().join("old-link");
    fs::hard_link(&p, &old).unwrap();
    assert_eq!(apply(&p, &fs::read(&p).unwrap(), &overturn("wrong"), TODAY), Ok(()));
    assert_eq!(fs::read_to_string(&old).unwrap(), plain("open", ""));          // the old inode is untouched: a rename, not a write in place
    assert_eq!(fs::read_to_string(&p).unwrap(), plain("overturned", "reviewed = \"2026-12-01\"\nreview_note = \"wrong\"\n"));
    let names: Vec<String> = fs::read_dir(dir(t.path())).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    assert_eq!(names, vec![format!("{ID}.md")]);                              // no .tmp left
}
#[test] fn apply_overturn_of_a_ratified_record_replaces_the_operator_keys_in_place() { // D§6.6 row 3
    let t = tempfile::tempdir().unwrap();
    let p = put(t.path(), &plain("ratified", RATIFIED));
    assert_eq!(apply(&p, &fs::read(&p).unwrap(), &overturn("wrong"), TODAY), Ok(()));
    assert_eq!(fs::read_to_string(&p).unwrap(), plain("overturned", "reviewed = \"2026-12-01\"\nreview_note = \"wrong\"\n"));
}
#[test] fn apply_refuses_forbidden_transitions_and_changes_nothing() { // D§6.6, D§10.1
    let promoted = format!("{RATIFIED}promoted_to = \"rule\"\npromotion = \"P\"\n");
    let cases: Vec<(String, Outcome, String)> = vec![
        (plain("ratified", RATIFIED), promote(), format!("zygo decision: {ID} is ratified")),
        (plain("overturned", "reviewed = \"2026-11-30\"\nreview_note = \"no\"\n"), promote(), format!("zygo decision: {ID} is overturned")),
        (plain("fixed", &format!("fixed_by = \"{C40}\"\n")), overturn("x"), format!("zygo decision: {ID} is fixed")),
        (plain("superseded", ""), overturn("x"), format!("zygo decision: {ID} is superseded")),
        (plain("ratified", &promoted), overturn("x"), format!("zygo decision: {ID} is promoted; remove the promotion by hand first")),
    ];
    for (text, o, want) in cases {
        let t = tempfile::tempdir().unwrap();
        let p = put(t.path(), &text);
        assert_eq!(allowed(&parse_record(ID, text.as_bytes()).unwrap(), &o), Err(want.clone()));
        assert_eq!(apply(&p, text.as_bytes(), &o, TODAY), Err(EditError::Refused(want)));
        assert_eq!(fs::read_to_string(&p).unwrap(), text);
    }
}
#[test] fn apply_refuses_an_outcome_that_makes_the_record_malformed() { // D§6.3 review_note at most 2,000 bytes
    let t = tempfile::tempdir().unwrap();
    let p = put(t.path(), &plain("open", ""));
    let r = apply(&p, &fs::read(&p).unwrap(), &overturn(&"x".repeat(2001)), TODAY);
    assert!(matches!(&r, Err(EditError::Refused(s)) if s.starts_with(&format!("zygo decision: {ID}: the outcome would make the record malformed: "))), "{r:?}");
    assert_eq!(fs::read_to_string(&p).unwrap(), plain("open", ""));
}
#[cfg(unix)]
#[test] fn apply_write_failure_is_io_naming_the_file_and_changes_nothing() { // D§14 "the write of an outcome fails"
    use std::os::unix::fs::PermissionsExt;
    // SAFETY: geteuid has no preconditions.
    if unsafe { libc::geteuid() } == 0 { eprintln!("skipped: root ignores directory modes"); return; }
    let t = tempfile::tempdir().unwrap();
    let p = put(t.path(), &plain("open", ""));
    let d = dir(t.path());
    fs::set_permissions(&d, fs::Permissions::from_mode(0o500)).unwrap();
    let r = apply(&p, &fs::read(&p).unwrap(), &overturn("x"), TODAY);
    fs::set_permissions(&d, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(matches!(&r, Err(EditError::Io(s)) if s.starts_with("zygo decision: cannot write ") && s.contains("docs/decisions")), "{r:?}");
    assert_eq!(fs::read_to_string(&p).unwrap(), plain("open", ""));
}
```

- [x] **3. Run** `cargo test -p zygo --locked --test decision_verbs`. Check: it fails to compile with `could not find edit in decisions` (or the like).

- [x] **4. Implement** `crates/zygo-work/src/decisions/edit.rs` with the interface above: `Promotion` (`parse`, `name`), `Outcome`, `EditError`, `allowed`, `apply`, and the private `set` and `basic`. Add `pub mod edit;` to `crates/zygo-work/src/decisions/mod.rs`, in alphabetical order. Doc comments cite D§11.3, D§6.6 and D§6.2.

- [x] **5. Run** `cargo test -p zygo --locked --test decision_verbs`. Check: the 8 `apply_*` tests pass (on Windows 7; the write-failure test is unix only).

- [x] **6. Commit** `crates/zygo-work/Cargo.toml`, `Cargo.lock`, `crates/zygo-work/src/decisions/{mod.rs,edit.rs}` and `crates/zygo/tests/decision_verbs.rs` with the message `DR-5: decisions::edit::apply, byte-preserving outcome writes` and the trailers `Zygo-Session: <session>` and `Zygo-Work: phase3:DR-5`. Check: `git status` is clean.

- [x] **7. Write the failing CLI tests** in `crates/zygo/tests/decision_verbs.rs`. Also set the `USAGE` constant of `decision_cli.rs:101` and `decision_new.rs` to the new `USAGE` text, and add it here with its `\n`. Helper:

```rust
const USAGE: &str = "zygo decision: usage: zygo decision new --kind <k> --title <t> --unit <u> --round <n> --role <r> [--class <c>] [--depends-on <id>]... [--supersedes <id>] | ls [--status <s>] [--kind <k>] [--unit <prefix>] [--pending] | show <id> | deps <id> | check | ratify <id> [--note <text>] [--promote adr|spec|rule --as <text>] [--yes] | overturn <id> --reason <text> [--yes]\n";
struct Ran { code: i32, out: String, err: String }
/// `run_in` with `top`, today TODAY, stdin `input`, and `env` for ZYGO_SESSION_DIR; no git, no clock, no process env.
fn go(top: &Path, env: &dyn Fn(&str) -> Option<OsString>, input: &mut dyn BufRead, args: &[&str]) -> Ran {
    let list = |_: &Path, _: &str, _: usize| Ok(vec![]);
    let (t, today) = (|| Some(top.to_path_buf()), || TODAY.to_string());
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let mut io = DecisionIo { input, out: &mut out, err: &mut err, tty: false };
    let code = run_in(&Repo { top: &t, commits: &list, head: &|_| None, today: &today, usage_db: Path::new("") }, env, &args, &mut io);
    Ran { code, out: String::from_utf8(out).unwrap(), err: String::from_utf8(err).unwrap() }
}
fn run(top: &Path, args: &[&str]) -> Ran { go(top, &|_| None, &mut std::io::empty(), args) }
fn path_line(p: &Path) -> String { format!("{}\n", p.display()) }
/// Stdin that writes `bytes` to `path` at its first read, then answers `y`: an edit while the prompt waits.
struct EditOnRead { path: PathBuf, bytes: String, done: bool }
impl Read for EditOnRead {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.done { return Ok(0); }
        fs::write(&self.path, &self.bytes)?;
        self.done = true;
        buf[..2].copy_from_slice(b"y\n");
        Ok(2)
    }
}
```

Tests:

```rust
#[test] fn ratify_promote_rule_yes_writes_the_outcome_and_overturn_of_it_is_refused() { // D§17 #14
    let t = tempfile::tempdir().unwrap();
    let p = put(t.path(), &rich("open"));
    let r = run(t.path(), &["ratify", ID, "--promote", "rule", "--as", "Add the rule", "--yes"]);
    assert_eq!((r.code, r.out.clone(), r.err.as_str()), (0, path_line(&p), ""));
    assert_eq!(fs::read_to_string(&p).unwrap(), rich_promoted());
    let r = run(t.path(), &["overturn", ID, "--reason", "wrong", "--yes"]);
    assert_eq!((r.code, r.out.as_str(), r.err), (1, "", format!("zygo decision: {ID} is promoted; remove the promotion by hand first\n")));
    assert_eq!(fs::read_to_string(&p).unwrap(), rich_promoted());
}
#[test] fn ratify_and_overturn_write_their_keys() { // D§6.6 rows 1-3, D§11.3
    let t = tempfile::tempdir().unwrap();
    let p = put(t.path(), &plain("open", ""));
    assert_eq!(run(t.path(), &["ratify", ID, "--note", "ok", "--yes"]).code, 0);
    assert_eq!(fs::read_to_string(&p).unwrap(), plain("ratified", "reviewed = \"2026-12-01\"\nreview_note = \"ok\"\n"));
    assert_eq!(run(t.path(), &["overturn", ID, "--reason", "wrong", "--yes"]).code, 0);   // ratified without promoted_to
    assert_eq!(fs::read_to_string(&p).unwrap(), plain("overturned", "reviewed = \"2026-12-01\"\nreview_note = \"wrong\"\n"));
    put(t.path(), &plain("open", ""));
    assert_eq!(run(t.path(), &["ratify", ID, "--yes"]).code, 0);
    assert_eq!(fs::read_to_string(&p).unwrap(), plain("ratified", "reviewed = \"2026-12-01\"\nreview_note = \"\"\n"));
}
#[test] fn operator_verbs_exit_2_in_an_agent_session_and_change_nothing() { // D§11.1, D-C5
    let t = tempfile::tempdir().unwrap();
    let p = put(t.path(), &plain("open", ""));
    let sd = t.path().join("session");
    let env = |k: &str| (k == "ZYGO_SESSION_DIR").then(|| sd.clone().into_os_string());
    for (verb, args) in [("ratify", vec!["ratify", ID, "--yes"]), ("overturn", vec!["overturn", ID, "--reason", "x", "--yes"])] {
        let r = go(t.path(), &env, &mut std::io::empty(), &args);
        assert_eq!((r.code, r.out.as_str(), r.err), (2, "", format!("zygo decision {verb}: run it in your own shell, not in an agent session\n")));
        assert_eq!(fs::read_to_string(&p).unwrap(), plain("open", ""));
    }
}
#[test] fn ratify_of_a_record_that_is_not_open_exits_1() { // D§10.1
    for (status, extra) in [("ratified", RATIFIED.to_string()), ("overturned", "reviewed = \"2026-11-30\"\nreview_note = \"no\"\n".into()),
                            ("fixed", format!("fixed_by = \"{C40}\"\n")), ("superseded", String::new())] {
        let t = tempfile::tempdir().unwrap();
        let p = put(t.path(), &plain(status, &extra));
        let r = run(t.path(), &["ratify", ID, "--yes"]);
        assert_eq!((r.code, r.out.as_str(), r.err), (1, "", format!("zygo decision: {ID} is {status}\n")));
        assert_eq!(fs::read_to_string(&p).unwrap(), plain(status, &extra));
        if status != "ratified" {
            let r = run(t.path(), &["overturn", ID, "--reason", "x", "--yes"]);
            assert_eq!((r.code, r.err), (1, format!("zygo decision: {ID} is {status}\n")));
        }
    }
}
#[test] fn argument_errors_exit_2_with_the_usage_line() { // D§10.1 `--promote` without `--as`; empty `--reason`
    let t = tempfile::tempdir().unwrap();
    let p = put(t.path(), &plain("open", ""));
    for a in [vec!["ratify"], vec!["ratify", ID, "--promote", "rule", "--yes"], vec!["ratify", ID, "--as", "t", "--yes"],
              vec!["ratify", ID, "--promote", "law", "--as", "t"], vec!["ratify", ID, "--promote", "rule", "--as", ""],
              vec!["ratify", ID, "--note"], vec!["ratify", ID, "--bogus"], vec!["overturn", ID, "--yes"],
              vec!["overturn", ID, "--reason", "", "--yes"], vec!["overturn", ID, "--reason"]] {
        let r = run(t.path(), &a);
        assert_eq!((r.code, r.out.as_str(), r.err.as_str()), (2, "", USAGE), "{a:?}");
        assert_eq!(fs::read_to_string(&p).unwrap(), plain("open", ""));
    }
}
#[test] fn without_yes_the_record_is_shown_and_only_y_writes() { // D§11.1 `[y/N]`
    let t = tempfile::tempdir().unwrap();
    let p = put(t.path(), &plain("open", ""));
    let shown = format!("{}ratify {ID}? [y/N] ", plain("open", ""));
    for answer in ["n\n", "", "\n", "no\n"] {
        let r = go(t.path(), &|_| None, &mut answer.as_bytes(), &["ratify", ID]);
        assert_eq!((r.code, r.out.clone(), r.err), (1, shown.clone(), format!("zygo decision: {ID} not changed\n")), "{answer:?}");
        assert_eq!(fs::read_to_string(&p).unwrap(), plain("open", ""));
    }
    let r = go(t.path(), &|_| None, &mut "y\n".as_bytes(), &["ratify", ID]);
    assert_eq!((r.code, r.out, r.err.as_str()), (0, format!("{shown}{}", path_line(&p)), ""));
    assert_eq!(fs::read_to_string(&p).unwrap(), plain("ratified", "reviewed = \"2026-12-01\"\nreview_note = \"\"\n"));
}
#[test] fn a_record_changed_at_the_prompt_is_skipped_and_left_as_edited() { // D§11.3 re-read, D§14 two writers
    let t = tempfile::tempdir().unwrap();
    let p = put(t.path(), &plain("open", ""));
    let edited = plain("open", "").replace("title = \"T\"", "title = \"T2\"");
    let mut input = BufReader::new(EditOnRead { path: p.clone(), bytes: edited.clone(), done: false });
    let r = go(t.path(), &|_| None, &mut input, &["overturn", ID, "--reason", "wrong"]);
    assert_eq!((r.code, r.err), (1, format!("{ID} changed since it was shown; skipped\n")));
    assert_eq!(fs::read_to_string(&p).unwrap(), edited);
}
#[test] fn failure_rows_of_d14() { // D§14 column `review, ratify, overturn`
    let t = tempfile::tempdir().unwrap();
    for a in [vec!["ratify", ID, "--yes"], vec!["overturn", ID, "--reason", "x", "--yes"]] {
        let r = run(t.path(), &a);                                             // docs/decisions absent
        assert_eq!((r.code, r.out.as_str(), r.err.as_str()), (0, "no open records\n", ""), "{a:?}");
    }
    let t = tempfile::tempdir().unwrap();
    fs::create_dir_all(t.path().join("docs")).unwrap();
    fs::write(t.path().join("docs/decisions"), "").unwrap();                   // not a directory
    let r = run(t.path(), &["ratify", ID, "--yes"]);
    assert_eq!((r.code, r.err.as_str()), (1, "zygo decision: docs/decisions is not a directory\n"));
    let t = tempfile::tempdir().unwrap();
    let bad = plain("open", "").replace("\n## Evidence\n\nE.\n", "\n");          // malformed: missing section
    let p = put(t.path(), &bad);
    let r = run(t.path(), &["ratify", ID, "--yes"]);
    assert_eq!((r.code, r.err), (1, format!("zygo decision: {ID} is not a valid record\n")));
    assert_eq!(fs::read_to_string(&p).unwrap(), bad);
    let r = run(t.path(), &["overturn", "DDR-01JDQ2Y7Z8K4M5N6P7Q8R00009", "--reason", "x", "--yes"]);
    assert_eq!((r.code, r.err.as_str()), (1, "zygo decision: DDR-01JDQ2Y7Z8K4M5N6P7Q8R00009 is not a valid record\n"));
}
#[test] fn help_words_as_flag_values_are_values() { // D§10.1 `wants_help`
    let t = tempfile::tempdir().unwrap();
    let p = put(t.path(), &plain("open", ""));
    assert_eq!(run(t.path(), &["ratify", ID, "--note", "-h", "--yes"]).code, 0);
    assert_eq!(parse_record(ID, &fs::read(&p).unwrap()).unwrap().review_note.as_deref(), Some("-h"));
    let r = run(t.path(), &["ratify", ID, "-h"]);
    assert_eq!((r.code, r.out.as_str()), (0, USAGE));
}
#[cfg(unix)]
#[test] fn a_write_failure_exits_1_naming_the_file() { // D§14 "the write of an outcome fails"
    use std::os::unix::fs::PermissionsExt;
    // SAFETY: geteuid has no preconditions.
    if unsafe { libc::geteuid() } == 0 { eprintln!("skipped: root ignores directory modes"); return; }
    let t = tempfile::tempdir().unwrap();
    let p = put(t.path(), &plain("open", ""));
    let d = dir(t.path());
    fs::set_permissions(&d, fs::Permissions::from_mode(0o500)).unwrap();
    let r = run(t.path(), &["overturn", ID, "--reason", "x", "--yes"]);
    fs::set_permissions(&d, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!((r.code, r.out.as_str()), (1, ""));
    assert!(r.err.starts_with("zygo decision: cannot write ") && r.err.contains("docs/decisions"), "{}", r.err);
    assert_eq!(fs::read_to_string(&p).unwrap(), plain("open", ""));
}
```

No test runs the `zygo` binary. Every test calls `run_in`, which gets `ZYGO_SESSION_DIR` only from its `env` closure (as `decision_new.rs` does), so the process environment does not matter.

- [x] **8. Run** `cargo test -p zygo --locked --test decision_verbs`. Check: the 10 new CLI tests fail (`ratify` and `overturn` give `USAGE`, exit 2). The `apply_*` tests still pass.

- [x] **9. Implement** the CLI in `crates/zygo/src/cmd/decision.rs` as the Interface section states: `DecideArgs`, `Sub::Decide`, `parse_decide`, `wants_help` (four more value flags), `USAGE`, the session refusal and the absent-directory line in `run_in`, `decide`, and `print_lines` (with `show` calling it). Import `zygo_work::decisions::edit::{allowed, apply, EditError, Outcome, Promotion}`.

- [x] **10. Run** `cargo test -p zygo --locked --test decision_verbs --test decision_cli --test decision_new`. Check: all pass, including `decision_cli`'s help and usage tests with the new `USAGE`.

- [x] **11. Acceptance and neighbours.** Run:
  - `cargo test -p zygo --locked --test decision_verbs`: all pass.
  - `cargo test -p zygo-work --locked`: all pass.
  - `cargo test -p zygo --locked --test cli_surface`: passes, since the `decision` verb set is unchanged.
  - `cargo run -p zygo --locked -- decision check`: exit 0.

  Commit `crates/zygo/src/cmd/decision.rs`, `crates/zygo/tests/decision_verbs.rs`, `crates/zygo/tests/decision_cli.rs` and `crates/zygo/tests/decision_new.rs` with the message `DR-5: zygo decision ratify and overturn` and the two trailers. Check: `git status` is clean.

### Coverage

| Contract item | Test |
|---|---|
| #14 promote writes `status`, `reviewed`, `promoted_to`, `promotion`; unknown keys, comments and body byte-equal | `apply_promote_keeps_…`, `ratify_promote_rule_yes_…` |
| #14 overturn of that record exits 1 with the promoted text and changes nothing | `ratify_promote_rule_yes_…`, `apply_refuses_forbidden_transitions_…` |
| Session refusal, exit 2, both verbs | `operator_verbs_exit_2_in_an_agent_session_…` |
| `ratify` of a non-`open` record, exit 1, `zygo decision: <id> is <status>` | `ratify_of_a_record_that_is_not_open_exits_1` |
| `--promote` without `--as` exit 2; empty or missing `--reason` refused | `argument_errors_exit_2_with_the_usage_line` |
| `n` at `[y/N]` changes nothing | `without_yes_the_record_is_shown_and_only_y_writes` |
| `apply` → `Changed`, file byte-equal | `apply_returns_changed_…`, `a_record_changed_at_the_prompt_…` |
| Temporary file + rename in `docs/decisions/` | `apply_writes_a_new_file_and_renames_it` |
| Review focus 39: CRLF, comments between keys, unknown tables, the re-read race | `apply_keeps_a_crlf_record_…`, `apply_promote_keeps_…`, `apply_returns_changed_…`, `a_record_changed_at_the_prompt_…` |
| D§14 column `review, ratify, overturn` (absent, not a directory, malformed, write failure, two writers) | `failure_rows_of_d14`, `a_write_failure_exits_1_naming_the_file`, `apply_write_failure_is_io_…`, `a_record_changed_at_the_prompt_…` |
| D§6.6 operator rows (open→ratified, open→overturned, ratified-unpromoted→overturned) | `ratify_and_overturn_write_their_keys`, `apply_overturn_of_a_ratified_record_…` |

### Author's calls (the spec does not decide these)

1. `allowed` is public, which adds to the Produces line, so that the CLI refuses before the prompt and `apply` refuses again after the re-read.
2. A declined prompt (anything but `y` or `yes`, and EOF) exits 1 with `zygo decision: <id> not changed`. The prompt text is `<verb> <id>? [y/N] `.
3. Success prints the record's path on stdout (as `new` does), exit 0. `Changed` exits 1.
4. `--as` without `--promote`, an empty `--as`, and an empty `--reason` are argument errors: `USAGE`, exit 2.
5. The length limits (`review_note` at most 2,000 bytes, `promotion` at most 500) are enforced by `apply`'s check of the result with `parse_record`. It refuses with `zygo decision: <id>: the outcome would make the record malformed: <faults>`, exit 1, and adds no argument texts.
6. New string values are always one-line basic TOML strings (escaped LF, CR, TAB and control characters), so a note cannot break the line structure of a CRLF record.
7. The line ending of the rewritten front matter follows the opening `+++` line. A front matter with mixed line endings comes out all CRLF or all LF, and only that case loses byte-equality.
8. The temporary file is `<id>.<ULID>.tmp`, so a stale `<id>.tmp` or a second writer cannot collide with it. It is removed when the write fails (`new` leaves its `.tmp`). The rewritten file takes default permissions.
9. The session refusal comes after a successful parse, so bad arguments inside a session print `USAGE`. "Set" means non-empty, the same as the existing `session_dir`.
10. D§14 row 1 (`no open records`, exit 0) is checked before `read_dir`, with `symlink_metadata` NotFound on `docs/decisions`. An empty but present directory gives `<id> is not a valid record`, exit 1.
11. A malformed target prints the existing `find` or `show` line `zygo decision: <id> is not a valid record`. Other malformed files are not listed.
12. For overturn, the status test comes before the promotion test. An open record with `promoted_to` gets the promoted refusal.

### Review round 1

Findings:

1. Re-read-then-rename race (two `apply` calls both write; one outcome is lost): confirmed and fixed. `apply` takes an exclusive `File::lock` on the record before the re-read and holds it until after the rename. The re-read goes through the path, so a writer that waited reads the renamed file and gets `Changed`. Tests: `decisions::edit::tests::the_record_is_locked_from_the_re_read_to_the_rename` (zygo-work unit test, through the seam `apply_with`), `apply_with_a_stale_shown_after_a_first_apply_returns_changed_and_keeps_the_first_outcome`.
2. A front matter with mixed CRLF and LF lines is rewritten to the ending of line 1: confirmed and fixed. `apply` refuses it, writes nothing, and prints `zygo decision: <id>: the front matter mixes CRLF and LF line endings; make them one kind first`. Test: `apply_refuses_a_front_matter_that_mixes_crlf_and_lf_and_changes_nothing` (both repros).
3. The non-empty promotion text and reason are checked only in the CLI: confirmed and fixed. `allowed` refuses `zygo decision: <id>: the promotion text is empty` and `zygo decision: <id>: the reason is empty`; the CLI keeps its usage line and exit 2. Test: `apply_refuses_an_empty_promotion_text_or_reason_and_changes_nothing`.
4. A note within its limits is refused by the 8 KiB rule: refuted. D§6.8 gives this refusal, and the message names the reason.
5. An empty `ZYGO_SESSION_DIR` is not refused: refuted. This is the existing `session_dir` rule and author's call 9: "set" means non-empty, as in DR-4.
6. Test gaps: confirmed and fixed.
   - (a) No test of a failed rename. Test: `decisions::edit::tests::a_failed_rename_is_io_and_changes_nothing`.
   - (b) The prompt of a CRLF record shows a `\r` on each line. `print_lines` now strips one trailing CR per line. Test: `the_prompt_of_a_crlf_record_holds_no_cr`.
   - (c) `decide` checks `allowed` on the `read_dir` record, but shows bytes it reads later. It now runs `allowed` on the parsed shown bytes. No deterministic test: no callback runs between `read_dir` and the read of the shown bytes inside `run_in`, so a test cannot change the file in that window without a new public `Repo` hook only for tests.

Author's calls:

1. The lock handle is opened as `read_bytes` opens a record (`O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC` on unix, `FILE_FLAG_OPEN_REPARSE_POINT` on Windows), so a link is not followed and a FIFO does not block. A failed open or lock is `Io` with `zygo decision: cannot read <path>: <error>`.
2. The test seam is the crate-private `apply_with(path, shown, outcome, today, before_rename)`. The hook runs after the temporary file is written and synced, before the rename. `apply` calls it with a no-op.
3. The mixed-ending check covers `text[..front_end]`, from the opening `+++` line to the closing `+++` line and its line ending. The body is not checked: it is copied byte for byte. This replaces author's call 7.
4. The empty checks do not trim, as the CLI does not. They come after the status and promotion checks in `allowed`, so the existing refusals keep their order.
5. `show` strips the trailing CR too, because it shares `print_lines` with the prompt. The `decision_cli` test `record_text_and_commit_subjects_are_printed_escaped` now puts a CR in the middle of a line, which is still escaped as `\r`.

### Review round 2

CI failure: run 45507410749 on d17b1f0 passed on macOS and Ubuntu and failed on windows-latest. 15 of 20 `decision_verbs` tests and the 2 `decisions::edit` lib tests failed with `Io("zygo decision: cannot read …\DDR-….md: cannot read: The process cannot access the file because another process has locked a portion of the file. (os error 33)")`.

Root cause: `apply_with` locked the record (`lock(path)`, `File::lock`) and then re-read it by path with `read_bytes`, through a second handle. On Windows `File::lock` is `LockFileEx`, a mandatory byte-range lock, so a read through any other handle fails with `ERROR_LOCK_VIOLATION` (33), even in the same process. On unix `File::lock` is `flock`, which is advisory, so the read passed. The `cannot read: …` suffix is the open/read error text of `read_bytes`. The round 1 design had a second defect on all 3 OSes: the rename replaces the record, so two writers could lock different files. A writer that opened the old record before the rename and a writer that opened the new record after it both held "the" lock, both read the new bytes, and both could write. Then one outcome is lost.

Fix: `apply` locks `docs/decisions/.lock` (`edit::LOCK`) instead of the record. It opens or creates the file (read and write, no truncation, no link followed, a FIFO does not block), takes `File::lock`, and drops the handle after the rename. The record has no open zygo handle during the re-read (`read_bytes` opens and closes its own handle) or the rename.
- Windows: `LockFileEx` on `.lock` blocks I/O only on `.lock`, which nobody reads or writes. The re-read of the record and the `MoveFileEx`/`SetFileInformationByHandle` rename onto it do not touch a locked file. A waiting `apply` blocks in `LockFileEx` until the holder's handle closes.
- macOS and Linux: `flock` on `.lock`. The file is never renamed or removed, so every `apply` locks the same inode. This also closes the round 1 race above.
- On all 3 OSes the re-read, the compare, the temporary file and the rename happen while one process holds the lock. A writer that waited re-reads the renamed record and gets `Changed`.

Author's calls:

1. `.lock` stays in `docs/decisions/`. Removing it after the rename would bring back the two-files race: a waiter holds the removed file while a new `apply` creates and locks a new one. `set::read_dir` ignores it, because `name_kind` gives `Other` for a name that does not end in `.md` (`record.rs:130-136`), so `decision check`, `ls` and `show` do not see it. This follows the repository's `<file>.lock` sidecars (`zygo-config/src/profile.rs:269`, `zygo-plugins/src/approve.rs:44`). `git status` shows it as untracked until a `.gitignore` entry `docs/decisions/.lock` is added. That file is outside this row's files.
2. A failed open or lock of `.lock` is `Io` with `zygo decision: cannot write <dir>/.lock: <error>`, because creating it is a write in `docs/decisions/`. The read-only directory tests (`apply_write_failure_is_io_…`, `a_write_failure_exits_1_naming_the_file`) now fail at this step, with the same message prefix and an unchanged record.
3. One lock for the directory, not one for each record: one file is left behind, not one per record, and an `apply` holds the lock for only milliseconds.

Tests:
- `the_record_is_locked_from_the_re_read_to_the_rename` now checks, inside the `before_rename` seam, that a second handle's `try_lock` on `.lock` gets `WouldBlock` (`flock` on unix, `LockFileEx` with `LOCKFILE_FAIL_IMMEDIATELY` on Windows, both per handle, so this works in one process). It also checks that the record reads through the path while the lock is held, which is the read that failed on Windows. After `apply`, the lock is free and `.lock` is empty.
- `a_failed_rename_is_io_and_changes_nothing` and `apply_writes_a_new_file_and_renames_it` expect `.lock` beside the record. The second also runs `check` and `ls`, and checks that neither one reports `.lock`.
- Mutation check: with `lock` pointed back at the record, both lib tests fail.
