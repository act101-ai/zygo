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
