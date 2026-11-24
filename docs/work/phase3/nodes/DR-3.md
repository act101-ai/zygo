# DR-3 — `zygo decision` verb: `ls`, `show`, `deps`, `check` and the slash form

**Spec:** `docs/specs/2026-11-24-decision-records-design.md` D§10.1 (rows `ls`, `show`, `deps`, `check`; the exit codes; output through `zygo_core::esc`; the slash form), D§7, D§6.7 (`show` lists the trailer commits), D§6.8 (the directory texts), D§14 (columns `ls, show, deps` and `check`), D§13 item 5, D-C2
**Files:** `crates/zygo-work/src/decisions/mod.rs`, `crates/zygo-work/src/decisions/history.rs`, `crates/zygo-work/src/decisions/record.rs`, `crates/zygo-core/src/verbs.rs`, `crates/zygo/src/cmd/mod.rs`, `crates/zygo/src/cmd/verbs.rs`, `crates/zygo/src/cmd/decision.rs`, `crates/zygo/tests/cli_surface.rs`, `crates/zygo/tests/decision_cli.rs`, `crates/zygo/tests/slash_parity.rs`, `crates/zygo-plugins/tests/snapshot.rs`, `docs/specs/2026-11-18-zygo-design.md` (§17, §17.1, one change-log line)
**Consumes:** `read_dir` (DR-2), `set_faults` (DR-2), `RecordSet` (DR-2), `DirError` (DR-2), `dependents` (DR-2), `dependent_count` (DR-2), `Record` (DR-1), `valid_id` (DR-1)
**Produces:** `decisions::history::trailer_commits(top: &Path, id: &str, limit: usize) -> Result<Vec<(String, String)>, String>` (full id and subject, newest first, from `git log --grep '^Zygo-Decision: <id>$'`, 2 s bound; `Err` is the stderr line); `zygo::cmd::decision::DecisionIo<'a> { pub input: &'a mut dyn BufRead, pub out: &'a mut dyn Write, pub err: &'a mut dyn Write, pub tty: bool }`; `zygo::cmd::decision::run_with(cwd: &Path, env: &dyn Fn(&str) -> Option<OsString>, args: &[String], io: &mut DecisionIo) -> i32` (one arm per subcommand; later nodes add arms); `zygo::cmd::decision::verb(ctx: &VerbCtx, args: &[String]) -> anyhow::Result<String>` (slash: `run in your shell: zygo decision <args>`, reads no file); the `VERB_SPECS` entry `decision`
**Acceptance:** `cargo test -p zygo --locked --test decision_cli --test cli_surface` → all pass. `decision_cli` covers spec Acceptance #3 (`check` exits 1 with one line each for a `depends_on` cycle, a missing `depends_on` id, a file name ≠ `id`, `DDR-bad.md`, a 65 KiB file; `ls` exits 0 and names each on stderr; `README.md`, `x.tmp` and a subdirectory are ignored; a linked `docs/decisions` makes `ls` and `check` exit 1, `#[cfg(unix)]`), #4 (`deps` prints 4 lines with depths 1, 2, 3, 3 and the shared record once), #15 (`/zygo:decision ls` replies `run in your shell: zygo decision ls` and opens no file), exit 2 outside a git repository, `show` and `deps` exit 1 for an id that is not a valid record, a record title with an ESC character printed as `\u{1b}`, and an absent `docs/decisions/` → empty `ls` exit 0, `check` exit 0.

## Plan

Implementer notes. Paths are relative to the repository root. "The spec" is `docs/specs/2026-11-24-decision-records-design.md`; D§n is a section of it; "base §n" is `docs/specs/2026-11-18-zygo-design.md`. This node adds the CLI verb `zygo decision` with `ls`, `show`, `deps`, `check`, and the slash verb `/zygo:decision`, which reads no file (D-C2).

Reused code (do not rebuild): `zygo_core::esc` (`crates/zygo-core/src/text.rs:4`); `zygo_work::repo::checkout` (`crates/zygo-work/src/repo.rs:213`, as `crates/zygo/src/cmd/status.rs:21` calls it); `repo::git_run`, `repo::BOUND`, `RunError::reason` (`repo.rs:177`, `:14`, `:35`; `pub(crate)`, so `history.rs` in `zygo-work` uses them); `cmd::cwd_or_exit` (`crates/zygo/src/cmd/mod.rs:145`); `common::scratch_home` (`crates/zygo/tests/common/mod.rs:10`); the fixtures `C40`, `BODY`, `id`, `file`, `mkfifo` (copy from `crates/zygo-work/tests/decision_set.rs:8-62`).

Consumes (as in code now):

```rust
// zygo_work::decisions::record
pub const DIR: &str = "docs/decisions";
pub struct Record { pub id: String, pub kind: Kind, pub status: Status, pub class: Class, pub title: String, pub unit: String,
                    pub promoted_to: Option<String>, pub promoted_by: Option<String>, /* and the other D§6.3 fields */ pub body: String }
pub fn valid_id(s: &str) -> bool;
pub fn parse_record(stem: &str, bytes: &[u8]) -> Result<Record, Vec<String>>;
// Kind, Status, Class: pub fn parse(s: &str) -> Option<Self>; pub fn name(self) -> &'static str
// zygo_work::decisions::set
pub struct RecordSet { pub valid: Vec<Record>, pub malformed: Vec<(String, Vec<String>)> }
pub enum DirError { NotDirectory, TooMany }
pub fn read_dir(top: &Path) -> Result<RecordSet, DirError>;
pub fn set_faults(set: &RecordSet) -> Vec<String>;
// zygo_work::decisions::graph
pub struct Dependents { pub rows: Vec<(u32, String)>, pub cycle_at: Option<String> }
pub fn dependents(records: &[Record], id: &str) -> Dependents;
pub fn dependent_count(records: &[Record], id: &str, skip_superseded: bool) -> usize;
```

Produces:

```rust
// crates/zygo-work/src/decisions/history.rs
pub fn trailer_commits(top: &Path, id: &str, limit: usize) -> Result<Vec<(String, String)>, String>;
// crates/zygo-work/src/decisions/record.rs (split out of read_record; read_record = read_bytes, then parse_record)
pub fn read_bytes(path: &Path) -> Result<Vec<u8>, Vec<String>>;
// crates/zygo/src/cmd/decision.rs
pub struct DecisionIo<'a> { pub input: &'a mut dyn BufRead, pub out: &'a mut dyn Write, pub err: &'a mut dyn Write, pub tty: bool }
pub struct Repo<'a> {
    /// The repository top of the current directory; `None` outside a git repository. Called after the arguments parse.
    pub top: &'a dyn Fn() -> Option<PathBuf>,
    /// `history::trailer_commits` outside tests.
    pub commits: &'a dyn Fn(&Path, &str, usize) -> Result<Vec<(String, String)>, String>,
}
pub fn run_in(repo: &Repo, env: &dyn Fn(&str) -> Option<OsString>, args: &[String], io: &mut DecisionIo) -> i32; // one arm per subcommand
pub fn run_with(cwd: &Path, env: &dyn Fn(&str) -> Option<OsString>, args: &[String], io: &mut DecisionIo) -> i32;
pub fn run(args: &[String]) -> i32;                                        // the CLI arm: process cwd, env, stdin/stdout/stderr
pub fn verb(ctx: &VerbCtx, args: &[String]) -> anyhow::Result<String>;     // the slash form
// crates/zygo-core/src/verbs.rs: SPECS gains entry 17 (18 entries)
VerbSpec { name: "decision", args: "ls|show|deps|check …", summary: "Decision records; /zygo:decision replies with the shell command", scope: Scope::None }
```

Checked against the consumers: `DR-4.md`, `DR-5.md`, `DR-6.md` consume `run_with` and `DecisionIo` (they add arms to the `match` in `run_in`, and may add fields to `Repo`); `DR-6.md` consumes `trailer_commits` and `DecisionIo.tty`. No other node names `run_with` from this crate (`MS-14`, `MS-15`, `MS-17` name `zygo_mail_sync::sync::run_with`, a different item).

Why `Repo` and `run_in`: every git call runs under the 2 s production bound, and tests whose git calls run under that bound are `#[ignore = "timing: …"]` (`crates/zygo-config/tests/config_layers.rs:492`; graph.md Global constraints). `run_in` takes the top and the commit list as values, so the acceptance tests run no git and read no clock. `run_with` is `run_in` with `top = || repo::checkout(cwd, Instant::now() + Duration::from_secs(2)).map(|c| c.top)` and `commits = trailer_commits`. Only the end-to-end git tests are ignored.

Fixed values (D§10.1, D§6.8, D§7; the texts the spec does not give are fixed here). Every line goes through `zygo_core::esc` once, written as `esc(line) + "\n"`; write errors are ignored.

| Case | Stream | Line | Exit |
|---|---|---|---|
| any argument error (no subcommand, unknown subcommand or option, missing or extra operand, a missing option value, a `--status`/`--kind` value that `Status::parse`/`Kind::parse` refuses) | err | `zygo decision: usage: zygo decision ls [--status <s>] [--kind <k>] [--unit <prefix>] [--pending] \| show <id> \| deps <id> \| check` | 2 |
| `top()` is `None` (after the arguments parse) | err | `zygo decision: not in a git repository` | 2 |
| `DirError::NotDirectory` | err | `zygo decision: docs/decisions is not a directory` | 1 |
| `DirError::TooMany` | err | `zygo decision: more than 10,000 files in docs/decisions/` | 1 |
| a malformed file (`ls`, `show`, `deps`) | err | `zygo decision: <name>: <faults joined by "; ">` | — |
| a set fault (`ls` only) | err | `zygo decision: <fault>` | — |
| `ls` row | out | `<id> <kind> <status> <class> <dependent_count(valid, id, false)> <title>` | 0 |
| `show`/`deps` with an id not in `set.valid` (or not valid on re-read) | err | `zygo decision: <id> is not a valid record` | 1 |
| `show` | out | each line of the file (bytes from `read_bytes`, valid by `parse_record`; split on `\n` after one trailing `\n` is removed, so a `\r` is printed as `\r`), then `dependents: <depth-1 rows> direct, <all rows> total`, then on `Ok`: `commits: <n>` and `<hash> <subject>` per commit | 0 |
| `show`, commits `Err(line)` | err | `line` (the commits part is left out) | 0 |
| `deps` row; cycle | out; err | `<depth> <id> <kind> <status> <title>`; `zygo decision: cycle at <id>` | 0 |
| `check` | out | per malformed file, per fault: `<name>: <fault>`; then each `set_faults` line | 1 if any line, else 0 |

`ls` filters are joined by AND: `--status` equal, `--kind` equal, `--unit` a prefix of `unit`, `--pending` = `overturned`, or `ratified` with `promoted_to` and no `promoted_by`. A repeated option: the last value counts. Order: `set.valid` order (id). `trailer_commits` runs `git log -n <limit> --grep=^Zygo-Decision: <id>$ --format=%H%x09%s --` through `git_run` with deadline `Instant::now() + BOUND`, splits each line at the first tab, and maps `Err(e)` to `zygo decision: commits left out: git log <e.reason()>`; an id that fails `valid_id` gives `Err("zygo decision: commits left out: bad id")` with no git call. `show` calls it with limit 50. The slash reply is `run in your shell: zygo decision <esc(args joined by " ")>`, trailing space trimmed; it ignores `ctx` and reads nothing.

Readings of the spec that this plan fixes:
- `show` prints "the record as written": the bytes are read again with `read_bytes` (the `lstat`-first open of `read_record`), so a link or FIFO swapped in after `read_dir` is still never opened, and what is printed is what `parse_record` accepted.
- "Outside a git repository" is `repo::checkout` returning `None`, which also covers a `git` that fails or passes its bound; both exit 2.
- Argument errors are found before git runs (exit 2 either way); this lets one CI test reach the binary's arm without git.
- `ls` prints the set faults on stderr too, so it "names each" of the five cases of D§17 #3 while it exits 0.
- `env` and `DecisionIo.input`/`tty` are unused by these four subcommands; they are for DR-4 to DR-6.

### Steps

- [x] **1. Write the failing history tests.** Create `crates/zygo/tests/decision_cli.rs` with this header and helpers, and the two tests.

```rust
//! DR-3: `zygo decision ls|show|deps|check` and `/zygo:decision` (D§10.1, D§7, D§6.7, D§6.8, D§14, D-C2).
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use zygo::cmd::decision::{run_in, DecisionIo, Repo};
use zygo_work::decisions::history::trailer_commits;

mod common;

const C40: &str = "6e7fe2f0c1d2e3f4a5b6c7d8e9f0011223344556";
const BODY: &str = "\n## Choice\n\nC.\n\n## Alternatives rejected\n\nA.\n\n## Evidence\n\nE.\n\n## Cost to reverse\n\nR.\n";
fn id(n: u32) -> String { format!("DDR-01JDQ2Y7Z8K4M5N6P7Q8R{n:05}") }
fn md(n: u32) -> String { format!("{}.md", id(n)) }
/// A valid record text; `title` is a TOML basic-string body, `extra` more front-matter lines.
fn text(rid: &str, kind: &str, status: &str, title: &str, unit: &str, deps: &[u32], extra: &str, body: &str) -> String {
    let deps: Vec<String> = deps.iter().map(|d| id(*d)).collect();
    format!("+++\nid = \"{rid}\"\nkind = \"{kind}\"\nstatus = \"{status}\"\nclass = \"none\"\ntitle = \"{title}\"\nunit = \"{unit}\"\nround = 1\nrole = \"code\"\nmodel = \"unknown\"\ncommit = \"{C40}\"\ndate = \"2026-11-24\"\ndepends_on = {deps:?}\nreverse_cost = \"low\"\n{extra}+++\n{body}")
}
fn dir(top: &Path) -> PathBuf { let d = top.join("docs/decisions"); fs::create_dir_all(&d).unwrap(); d }
fn put(top: &Path, n: u32, deps: &[u32]) -> String { // an open decision titled `T`; returns its text
    let t = text(&id(n), "decision", "open", "T", "phase-3:N12", deps, "", BODY);
    fs::write(dir(top).join(md(n)), &t).unwrap();
    t
}
fn put_text(top: &Path, name: &str, t: &str) { fs::write(dir(top).join(name), t).unwrap(); }
type Commits = Result<Vec<(String, String)>, String>;
struct Ran { code: i32, out: String, err: String }
fn go(top: &dyn Fn() -> Option<PathBuf>, commits: Commits, args: &[&str]) -> Ran {
    let list = move |_: &Path, _: &str, limit: usize| { assert_eq!(limit, 50); commits.clone() };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let mut io = DecisionIo { input: &mut std::io::empty(), out: &mut out, err: &mut err, tty: false };
    let code = run_in(&Repo { top, commits: &list }, &|_| None, &args, &mut io);
    Ran { code, out: String::from_utf8(out).unwrap(), err: String::from_utf8(err).unwrap() }
}
fn run(top: &Path, args: &[&str]) -> Ran { go(&|| Some(top.to_path_buf()), Ok(vec![]), args) }
fn git(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git").args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"]).args(args)
        .current_dir(dir).output().unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8(o.stdout).unwrap().trim_end().to_string()
}
#[cfg(unix)]
fn mkfifo(path: &Path) { /* copy of crates/zygo-work/tests/decision_set.rs:57-62 */ }

#[test] fn trailer_commits_err_is_one_stderr_line() { // D§14 "git process fails"
    let t = tempfile::tempdir().unwrap();
    let e = trailer_commits(&t.path().join("missing"), &id(1), 50).unwrap_err();
    assert!(e.starts_with("zygo decision: commits left out: git log "), "{e}");
    assert_eq!(trailer_commits(t.path(), "nope", 50), Err("zygo decision: commits left out: bad id".to_string()));
}
#[test]
#[ignore = "timing: git calls run under the 2 s production bound (BOUND)"]
fn trailer_commits_match_the_trailer_line_newest_first() { // D§6.7
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    git(r, &["init", "-q"]);
    let commit = |subject: &str, body: &str| { git(r, &["commit", "-q", "--allow-empty", "-m", subject, "-m", body]); git(r, &["rev-parse", "HEAD"]) };
    let h1 = commit("one", &format!("Zygo-Decision: {}", id(1)));
    commit("two", &format!("see Zygo-Decision: {}", id(1)));
    let h3 = commit("three", &format!("Zygo-Decision: {}", id(2)));
    let h4 = commit("four", &format!("Zygo-Session: s\nZygo-Decision: {}", id(1)));
    let pair = |h: &str, s: &str| (h.to_string(), s.to_string());
    assert_eq!(trailer_commits(r, &id(1), 50), Ok(vec![pair(&h4, "four"), pair(&h1, "one")]));
    assert_eq!(trailer_commits(r, &id(1), 1), Ok(vec![pair(&h4, "four")]));
    assert_eq!(trailer_commits(r, &id(2), 50), Ok(vec![pair(&h3, "three")]));
}
```

- [x] **2. Run it and see it fail.** `cargo test -p zygo --locked --test decision_cli` → does not compile (`decision_cli` imports `zygo::cmd::decision` and `decisions::history`, which do not exist).
- [x] **3. Implement `read_bytes` and `trailer_commits`.** In `crates/zygo-work/src/decisions/record.rs`, move the body of `read_record` up to `read_to_end` into `pub fn read_bytes(path: &Path) -> Result<Vec<u8>, Vec<String>>` with `read_record`'s doc comment; `read_record` becomes `read_bytes(path)?`, then `parse_record(stem, &bytes)`. Create `crates/zygo-work/src/decisions/history.rs` with `trailer_commits` as in Fixed values; add `pub mod history;` to `decisions/mod.rs`. Create `crates/zygo/src/cmd/decision.rs` with the `DecisionIo`, `Repo`, `run_in`, `run_with` signatures of Produces and `run_in` returning 2, and `pub mod decision;` in `cmd/mod.rs`, so the test file compiles.
- [x] **4. Run it and see it pass.** `cargo test -p zygo --locked --test decision_cli trailer_commits` → 1 passed, 1 ignored; `cargo test -p zygo --locked --test decision_cli -- --ignored trailer_commits` → 1 passed; `cargo test -p zygo-work --locked --test decision_record --test decision_set` → all pass (the split changes no behavior).
- [x] **5. Commit.** `git add crates/zygo-work/src/decisions/record.rs crates/zygo-work/src/decisions/history.rs crates/zygo-work/src/decisions/mod.rs crates/zygo/src/cmd/decision.rs crates/zygo/src/cmd/mod.rs crates/zygo/tests/decision_cli.rs`, message `feat(work): decision trailer commits (DR-3)` with the implementer's `Zygo-Session:` and `Zygo-Work:` trailers.

- [ ] **6. Write the failing subcommand tests.** Append to `decision_cli.rs`:

```rust
const USAGE: &str = "zygo decision: usage: zygo decision ls [--status <s>] [--kind <k>] [--unit <prefix>] [--pending] | show <id> | deps <id> | check\n";

#[test] fn outside_a_repository_exits_2() { // D§10.1
    for a in [&["ls"][..], &["show", &id(1)], &["deps", &id(1)], &["check"]] {
        let r = go(&|| None, Ok(vec![]), a);
        assert_eq!((r.code, r.out.as_str(), r.err.as_str()), (2, "", "zygo decision: not in a git repository\n"), "{a:?}");
    }
}
#[test] fn argument_errors_exit_2_before_git() {
    let cases: [&[&str]; 10] = [&[], &["frob"], &["show"], &["show", "a", "b"], &["deps"], &["check", "x"],
                                &["ls", "--status", "nope"], &["ls", "--kind"], &["ls", "--bogus"], &["ls", "x"]];
    for a in cases {
        let r = go(&|| -> Option<PathBuf> { panic!("top() called for {a:?}") }, Ok(vec![]), a);
        assert_eq!((r.code, r.out.as_str(), r.err.as_str()), (2, "", USAGE), "{a:?}");
    }
}
#[test] fn absent_directory_is_empty() { // D§14 row 1
    let t = tempfile::tempdir().unwrap();
    for a in [&["ls"][..], &["check"]] {
        let r = run(t.path(), a);
        assert_eq!((r.code, r.out.as_str(), r.err.as_str()), (0, "", ""), "{a:?}");
    }
}
#[test] fn check_and_ls_name_each_fault() { // D§17 #3
    let t = tempfile::tempdir().unwrap();
    let top = t.path();
    put(top, 1, &[2]); put(top, 2, &[1]); put(top, 3, &[9]);
    put_text(top, &md(4), &text(&id(5), "decision", "open", "T", "phase-3:N12", &[], "", BODY));
    put_text(top, "DDR-bad.md", &text(&id(6), "decision", "open", "T", "phase-3:N12", &[], "", BODY));
    put_text(top, &md(7), &"x".repeat(65 * 1024));
    put_text(top, "README.md", "x"); put_text(top, "x.tmp", "x");
    fs::create_dir(dir(top).join("sub")).unwrap();
    fs::write(dir(top).join("sub").join(md(8)), "x").unwrap();
    let faults = [format!("{}: id does not equal the file stem", md(4)), format!("{}: over 64 KiB", md(7)),
                  "DDR-bad.md: bad record name".to_string(), format!("{}: depends_on {} is not a record", id(3), id(9)),
                  format!("depends_on cycle at {}", id(1))];
    let r = run(top, &["check"]);
    assert_eq!((r.code, r.err.as_str()), (1, ""));
    assert_eq!(r.out, faults.iter().map(|f| format!("{f}\n")).collect::<String>());
    let r = run(top, &["ls"]);
    assert_eq!(r.code, 0);
    assert_eq!(r.out, format!("{} decision open none 1 T\n{} decision open none 1 T\n{} decision open none 0 T\n", id(1), id(2), id(3)));
    assert_eq!(r.err, faults.iter().map(|f| format!("zygo decision: {f}\n")).collect::<String>());
}
#[cfg(unix)]
#[test] fn linked_directory_exits_1() { // D§6.8, D§14 row 2, review focus 42
    let t = tempfile::tempdir().unwrap();
    let real = t.path().join("real"); fs::create_dir(&real).unwrap();
    fs::write(real.join(md(1)), text(&id(1), "decision", "open", "T", "phase-3:N12", &[], "", BODY)).unwrap();
    fs::create_dir(t.path().join("docs")).unwrap();
    std::os::unix::fs::symlink(&real, t.path().join("docs/decisions")).unwrap();
    for a in [&["ls"][..], &["check"]] {
        let r = run(t.path(), a);
        assert_eq!((r.code, r.out.as_str(), r.err.as_str()), (1, "", "zygo decision: docs/decisions is not a directory\n"), "{a:?}");
    }
}
#[cfg(unix)]
#[test] fn fifo_and_link_records_are_malformed_and_not_opened() { // D§6.8, review focus 42
    let t = tempfile::tempdir().unwrap();
    mkfifo(&dir(t.path()).join(md(1)));
    let elsewhere = t.path().join("r.md");
    fs::write(&elsewhere, text(&id(2), "decision", "open", "T", "phase-3:N12", &[], "", BODY)).unwrap();
    std::os::unix::fs::symlink(&elsewhere, dir(t.path()).join(md(2))).unwrap();
    let lines = [format!("{}: is not a regular file; not read", md(1)), format!("{}: is a link; not read", md(2))];
    let r = run(t.path(), &["ls"]);
    assert_eq!((r.code, r.out.as_str()), (0, ""));
    assert_eq!(r.err, lines.iter().map(|l| format!("zygo decision: {l}\n")).collect::<String>());
    let r = run(t.path(), &["check"]);
    assert_eq!((r.code, r.out), (1, lines.iter().map(|l| format!("{l}\n")).collect::<String>()));
}
#[test] fn ls_filters_and_pending() { // D§10.1 `ls`
    let t = tempfile::tempdir().unwrap();
    let w = |n: u32, kind: &str, status: &str, unit: &str, extra: &str| put_text(t.path(), &md(n), &text(&id(n), kind, status, "T", unit, &[], extra, BODY));
    w(1, "assumption", "open", "phase-3:N12", "");
    w(2, "decision", "ratified", "phase-3:N12", "reviewed = \"2026-11-24\"\npromoted_to = \"rule\"\npromotion = \"P\"\n");
    w(3, "decision", "overturned", "phase-3:N12", "reviewed = \"2026-11-24\"\nreview_note = \"R\"\n");
    w(4, "decision", "ratified", "phase-4:N1", &format!("reviewed = \"2026-11-24\"\npromoted_to = \"rule\"\npromotion = \"P\"\npromoted_by = \"{C40}\"\n"));
    let ids = |a: &[&str]| { let r = run(t.path(), a); assert_eq!((r.code, r.err.as_str()), (0, ""), "{a:?}");
                             r.out.lines().map(|l| l.split(' ').next().unwrap().to_string()).collect::<Vec<_>>() };
    assert_eq!(ids(&["ls"]), [id(1), id(2), id(3), id(4)]);
    assert_eq!(ids(&["ls", "--pending"]), [id(2), id(3)]);
    assert_eq!(ids(&["ls", "--status", "ratified"]), [id(2), id(4)]);
    assert_eq!(ids(&["ls", "--status", "ratified", "--pending"]), [id(2)]);
    assert_eq!(ids(&["ls", "--kind", "assumption"]), [id(1)]);
    assert_eq!(ids(&["ls", "--unit", "phase-4"]), [id(4)]);
    assert_eq!(run(t.path(), &["ls", "--pending"]).out.lines().next().unwrap(), format!("{} decision ratified none 0 T", id(2)));
}
#[test] fn deps_prints_breadth_first_once() { // D§7, D§17 #4
    let t = tempfile::tempdir().unwrap();
    put_text(t.path(), &md(1), &text(&id(1), "assumption", "open", "T", "phase-3:N12", &[], "", BODY));
    put(t.path(), 2, &[1]); put(t.path(), 3, &[2]); put(t.path(), 4, &[3]); put(t.path(), 5, &[3, 4]);
    let r = run(t.path(), &["deps", &id(1)]);
    assert_eq!((r.code, r.err.as_str()), (0, ""));
    assert_eq!(r.out, format!("1 {} decision open T\n2 {} decision open T\n3 {} decision open T\n3 {} decision open T\n", id(2), id(3), id(4), id(5)));
}
#[test] fn deps_cuts_a_cycle() { // D§7
    let t = tempfile::tempdir().unwrap();
    put(t.path(), 1, &[]); put(t.path(), 2, &[1, 3]); put(t.path(), 3, &[2]);
    let r = run(t.path(), &["deps", &id(1)]);
    assert_eq!((r.code, r.err), (0, format!("zygo decision: cycle at {}\n", id(2))));
    assert_eq!(r.out, format!("1 {} decision open T\n2 {} decision open T\n", id(2), id(3)));
}
#[test] fn show_and_deps_refuse_an_id_that_is_not_a_valid_record() { // D§10.1
    let t = tempfile::tempdir().unwrap();
    put(t.path(), 1, &[]);
    put_text(t.path(), &md(4), "x");
    for sub in ["show", "deps"] {
        for bad in [id(9), id(4), "nope".to_string()] {
            let r = run(t.path(), &[sub, &bad]);
            assert_eq!((r.code, r.out.as_str()), (1, ""), "{sub} {bad}");
            assert!(r.err.starts_with(&format!("zygo decision: {}: ", md(4))), "{sub} {bad}: {}", r.err);
            assert!(r.err.ends_with(&format!("zygo decision: {bad} is not a valid record\n")), "{sub} {bad}: {}", r.err);
        }
    }
}
#[test] fn show_prints_the_record_dependents_and_commits() { // D§10.1 `show`, D§6.7
    let t = tempfile::tempdir().unwrap();
    let one = put(t.path(), 1, &[]); put(t.path(), 2, &[1]); put(t.path(), 3, &[2]);
    let h = "a".repeat(40);
    let r = go(&|| Some(t.path().to_path_buf()), Ok(vec![(h.clone(), "fix it".into())]), &["show", &id(1)]);
    assert_eq!((r.code, r.err.as_str()), (0, ""));
    assert_eq!(r.out, format!("{one}dependents: 1 direct, 2 total\ncommits: 1\n{h} fix it\n"));
    let gone = "zygo decision: commits left out: git log timed out".to_string();
    let r = go(&|| Some(t.path().to_path_buf()), Err(gone.clone()), &["show", &id(1)]);
    assert_eq!((r.code, r.out, r.err), (0, format!("{one}dependents: 1 direct, 2 total\n"), format!("{gone}\n")));
}
#[test] fn record_text_and_commit_subjects_are_printed_escaped() { // D§10.1, review focus 41
    let t = tempfile::tempdir().unwrap();
    put(t.path(), 1, &[]);
    let body = BODY.replace("\nC.\n", "\nC\u{1b}[31m\u{202e}x\u{2028}y\r\n");
    put_text(t.path(), &md(2), &text(&id(2), "decision", "open", "A\\u001bB\\u202eC\\u2028D", "phase-3:N12", &[1], "", &body));
    let title = "A\\u{1b}B\\u{202e}C\\u{2028}D";
    let raw = |s: &str| s.contains(['\u{1b}', '\r', '\u{202e}', '\u{2028}']);
    let r = run(t.path(), &["ls"]);
    assert!(r.out.ends_with(&format!("{} decision open none 0 {title}\n", id(2))) && !raw(&r.out), "{}", r.out);
    let r = run(t.path(), &["deps", &id(1)]);
    assert_eq!(r.out, format!("1 {} decision open {title}\n", id(2)));
    let r = go(&|| Some(t.path().to_path_buf()), Ok(vec![("b".repeat(40), "s\u{1b}]0;x\u{7}".into())]), &["show", &id(2)]);
    assert!(r.out.contains("\nC\\u{1b}[31m\\u{202e}x\\u{2028}y\\r\n") && r.out.contains("s\\u{1b}]0;x\\u{07}\n") && !raw(&r.out), "{}", r.out);
}
#[test] fn cli_arm_reaches_run_with() { // the binary's `decision` arm; argument errors run no git
    let h = tempfile::tempdir().unwrap();
    let mut c = Command::new(env!("CARGO_BIN_EXE_zygo"));
    common::scratch_home(&mut c, h.path());
    let o = c.args(["decision", "frob"]).current_dir(h.path()).stdin(std::process::Stdio::null()).output().unwrap();
    assert_eq!((o.status.code(), String::from_utf8(o.stderr).unwrap()), (Some(2), USAGE.to_string()));
}
#[test]
#[ignore = "timing: git calls run under the 2 s production bound (BOUND)"]
fn binary_in_and_out_of_a_git_repository() { // D§10.1 exit 2; D§10.1 `show` with real `git log`
    let h = tempfile::tempdir().unwrap();
    let root = h.path().canonicalize().unwrap();
    let zygo = |cwd: &Path, a: &[&str]| { let mut c = Command::new(env!("CARGO_BIN_EXE_zygo")); common::scratch_home(&mut c, &root);
        c.args(["decision"]).args(a).current_dir(cwd).env("GIT_CEILING_DIRECTORIES", &root).stdin(std::process::Stdio::null()).output().unwrap() };
    let w = root.join("w"); fs::create_dir(&w).unwrap();
    let o = zygo(&w, &["ls"]);
    assert_eq!((o.status.code(), String::from_utf8(o.stderr).unwrap()), (Some(2), "zygo decision: not in a git repository\n".to_string()));
    let r = root.join("r"); fs::create_dir(&r).unwrap();
    git(&r, &["init", "-q"]);
    put(&r, 1, &[]);
    git(&r, &["add", "."]);
    git(&r, &["commit", "-q", "-m", "add record", "-m", &format!("Zygo-Decision: {}", id(1))]);
    let head = git(&r, &["rev-parse", "HEAD"]);
    let o = zygo(&r, &["ls"]);
    assert_eq!((o.status.code(), String::from_utf8(o.stdout).unwrap()), (Some(0), format!("{} decision open none 0 T\n", id(1))));
    let o = zygo(&r, &["show", &id(1)]);
    assert!(String::from_utf8(o.stdout).unwrap().ends_with(&format!("dependents: 0 direct, 0 total\ncommits: 1\n{head} add record\n")));
}
```

- [ ] **7. Run it and see it fail.** `cargo test -p zygo --locked --test decision_cli` → the step-6 tests fail (`run_in` returns 2 with no output; `zygo decision` is not a subcommand, so clap exits 2 with its own text).
- [ ] **8. Implement the subcommands.** In `crates/zygo/src/cmd/decision.rs`: `run_in` parses `args` (first word `ls`, `show`, `deps`, `check`; anything else gives the usage line and 2), then calls `(repo.top)()`, then `read_dir`, and gives the lines and exits of Fixed values; `run_with` as in "Why `Repo` and `run_in`"; `run(args)` takes the cwd from `cwd_or_exit()` (its `Err(code)` is returned), locks stdin, stdout and stderr, sets `tty = std::io::IsTerminal::is_terminal(&std::io::stdin())`, and calls `run_with(&cwd, &|k| std::env::var_os(k), args, &mut io)`. In `cmd/mod.rs`: the variant `/// Decision records: ls, show, deps, check` `Decision { #[arg(trailing_var_arg = true, allow_hyphen_values = true)] args: Vec<String> }` in `Cmd`, and the arm `Cmd::Decision { args } => return decision::run(args),` beside `Cmd::Loadout`.
- [ ] **9. Run it and see it pass.** `cargo test -p zygo --locked --test decision_cli` → all pass, 2 ignored; `cargo test -p zygo --locked --test decision_cli -- --ignored` → 2 passed.
- [ ] **10. Commit.** `git add crates/zygo/src/cmd/decision.rs crates/zygo/src/cmd/mod.rs crates/zygo/tests/decision_cli.rs`, message `feat(work): zygo decision ls, show, deps, check (DR-3)` with the implementer's trailers.

- [ ] **11. Write the failing slash and verb-table tests.** Append to `decision_cli.rs`:

```rust
#[test] fn slash_form_replies_with_the_shell_command_and_reads_nothing() { // D§17 #15, D-C2
    let v = zygo::cmd::verbs::VERBS.iter().find(|v| v.spec.name == "decision").expect("decision is a slash verb");
    let ctx = zygo::cmd::verbs::VerbCtx { slash: true, save: false, session_dir: None, paths: zygo_core::paths::Paths::from_env() };
    for (a, want) in [(&["ls"][..], "run in your shell: zygo decision ls"), (&["show", "DDR-X"], "run in your shell: zygo decision show DDR-X"),
                      (&["ls", "a\u{1b}b"], "run in your shell: zygo decision ls a\\u{1b}b"), (&[], "run in your shell: zygo decision")] {
        let a: Vec<String> = a.iter().map(|s| s.to_string()).collect();
        assert_eq!((v.handler)(&ctx, &a).unwrap(), want);
    }
}
```

In `crates/zygo/tests/cli_surface.rs`: add `"decision"` to the list of `help_lists_every_command`; add `|decision` to `every_command_exists`; in `verbs_match_spec` change `[(&str, &str); 17]` to `18`, append `("decision", "ls|show|deps|check …")` to `want` and `"decision"` to the `names` array. In `crates/zygo-plugins/tests/snapshot.rs`: both `17` in `embedded_has_one_command_per_verb` become `18`. In `crates/zygo/tests/slash_parity.rs`: `NOT_IN_SAME` becomes `&["down", "decision"]` (the slash reply differs from the CLI reply by design), and `"decision ls"` joins the line list of `shell_only_slash_forms_reply_with_the_command_and_change_nothing` (ignored; it checks through `zygo hook prompt` that no file changes).
- [ ] **12. Run it and see it fail.** `cargo test -p zygo --locked --test decision_cli --test cli_surface` → `slash_form_replies…` panics with `decision is a slash verb`; `verbs_match_spec` fails on the length.
- [ ] **13. Implement the slash verb.** In `crates/zygo-core/src/verbs.rs`: `SPECS: [VerbSpec; 18]` with the Produces entry last. In `cmd/verbs.rs`: `Verb { spec: &VERB_SPECS[17], handler: super::decision::verb },` last. In `decision.rs`: `verb` returns `Ok` of the slash reply of Fixed values. The embedded `commands/decision.md` comes from `VERB_SPECS` (`crates/zygo-plugins/src/embedded.rs:29`); no file is added.
- [ ] **14. Run it and see it pass.** `cargo test -p zygo --locked --test decision_cli --test cli_surface` → all pass (the node's acceptance check); `cargo test -p zygo --locked --test slash_parity --test compact_cmd` and `cargo test -p zygo-plugins --locked --test snapshot` → all pass; `cargo test -p zygo --locked --test slash_parity -- --ignored shell_only` → passes.
- [ ] **15. Commit.** `git add crates/zygo-core/src/verbs.rs crates/zygo/src/cmd/verbs.rs crates/zygo/src/cmd/decision.rs crates/zygo/tests/decision_cli.rs crates/zygo/tests/cli_surface.rs crates/zygo/tests/slash_parity.rs crates/zygo-plugins/tests/snapshot.rs`, message `feat(work): /zygo:decision replies with the shell command (DR-3)` with the implementer's trailers.

- [ ] **16. Write the base-spec change (D§13 item 5).** In `docs/specs/2026-11-18-zygo-design.md`: after the §17 row of `zygo work`, the row ``| `zygo decision new\|ls\|show\|deps\|check\|ratify\|overturn\|review` | Delegated decision records in `docs/decisions/` of the current repository (decision records design D§10.1). |``; after the §17.1 row of `workspace`, the row ``| `decision <args>` | Replies `run in your shell: zygo decision <args>` and reads no file: the slash form runs inside the hook (decision records design D§10.1, D-C2) | — |``; at the end of the change log, ``| 2026-11-24 | §17, §17.1 | decision records design D§13 item 5 (approved 2026-11-24; node DR-3) | The `zygo decision` verb and its slash form, which reads no record (D-C2). |``.
- [ ] **17. Check the spec rows.** `grep -c 'zygo decision' docs/specs/2026-11-18-zygo-design.md` → at least 3; `git diff --stat docs/specs/2026-11-18-zygo-design.md` → 3 insertions, 0 deletions.
- [ ] **18. Run the floor.** `cargo test --workspace --locked --no-fail-fast` → all pass; `cargo run -p zygo --locked -- decision check` → exit 0, no output (`docs/decisions/` holds only `README.md`; graph.md Verification floor, D§12 item 11).
- [ ] **19. Commit.** `git add docs/specs/2026-11-18-zygo-design.md`, message `docs(spec): zygo decision in §17 and §17.1 (DR-3)` with the implementer's trailers.

Review focus covered: 41 (ESC, CR, bidi, line separators in title, body and commit subject: `record_text_and_commit_subjects_are_printed_escaped`; a note is front-matter TOML, which cannot hold a raw control character, and `show` prints it with the same rule); 42 (`linked_directory_exits_1`, `fifo_and_link_records_are_malformed_and_not_opened`, both `#[cfg(unix)]`); 75 (no new test reads a clock or spawns an agent: the two git tests that run under the 2 s bound are `#[ignore = "timing: …"]`; the binary tests spawn only `zygo` and `git`).
