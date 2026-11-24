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
#[allow(clippy::too_many_arguments)]
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
    let mut input = std::io::empty();
    let mut io = DecisionIo { input: &mut input, out: &mut out, err: &mut err, tty: false };
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
fn mkfifo(path: &Path) { // as crates/zygo-work/tests/decision_set.rs:57
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    // SAFETY: `c` is a valid NUL-terminated path.
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0, "mkfifo {}", path.display());
}

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
