//! DR-16: an unreadable `docs/decisions` has its own error and line (D§6.8, D§9.5, D§14).
#![cfg(unix)]
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use zygo::cmd::decision::{run_in, DecisionIo, Repo};
use zygo_work::decisions::set::{read_dir, DirError};
use zygo_work::report::report;

const CANNOT: &str = "zygo decision: cannot read docs/decisions\n";
const NEW: [&str; 11] = ["new", "--kind", "decision", "--title", "T", "--unit", "phase-3:N12", "--round", "1", "--role", "code"];

/// Sets mode 0o755 again on drop, also on a panic.
struct Restore(PathBuf);
impl Drop for Restore {
    fn drop(&mut self) { let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o755)); }
}
/// `top/docs/decisions` with one file, then `top/rel` at mode 000. `None` (after a printed reason) when
/// `docs/decisions` can still be listed (root).
fn locked(top: &Path, rel: &str) -> Option<Restore> {
    let d = top.join("docs/decisions");
    fs::create_dir_all(&d).unwrap();
    fs::write(d.join("README.md"), "x").unwrap();
    let p = top.join(rel);
    fs::set_permissions(&p, fs::Permissions::from_mode(0o000)).unwrap();
    let g = Restore(p);
    if fs::read_dir(&d).is_ok() {
        eprintln!("skipped: {} can still be listed with {rel} at mode 000 (running as root)", d.display());
        return None;
    }
    Some(g)
}
struct Ran { code: i32, out: String, err: String }
fn run(top: &Path, args: &[&str]) -> Ran {
    let list = |_: &Path, _: &str, _: usize| Ok(vec![]);
    let checkout = || Some((top.to_path_buf(), top.join(".git")));
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let mut input = std::io::empty();
    let mut io = DecisionIo { input: &mut input, out: &mut out, err: &mut err, tty: false };
    let repo = Repo { checkout: &checkout, commits: &list, head: &|_| None, today: &String::new, usage_db: Path::new("") };
    let code = run_in(&repo, &|_| None, &args, &mut io);
    Ran { code, out: String::from_utf8(out).unwrap(), err: String::from_utf8(err).unwrap() }
}
fn assert_cannot_read(rel: &str) {
    let t = tempfile::tempdir().unwrap();
    let top = t.path().join("repo");
    let Some(g) = locked(&top, rel) else { return };
    assert_eq!(read_dir(&top), Err(DirError::Unreadable));
    for a in [&["ls"][..], &["check"], &NEW[..]] {
        let r = run(&top, a);
        assert_eq!((r.code, r.out.as_str(), r.err.as_str()), (1, "", CANNOT), "{a:?}");
    }
    assert_eq!(report(&t.path().join("absent.db"), Some(&top), None, None).unwrap(),
        "no work units\n\ndecision records: cannot read docs/decisions");
    drop(g); // the mode is back, so the listing below can be read
    let names: Vec<_> = fs::read_dir(top.join("docs/decisions")).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(names, ["README.md"], "`new` wrote nothing");
}

#[test] fn decisions_at_mode_000_cannot_be_read() { assert_cannot_read("docs/decisions"); } // D§6.8 listing error
#[test] fn docs_at_mode_000_cannot_be_read() { assert_cannot_read("docs"); }                // D§6.8 lstat error
/// The operator verbs first lstat `docs/decisions` and stop only on `NotFound` (D§14 row 1). With `docs` at mode 000
/// that lstat fails with a permission error, which must reach `cannot read` and not `no open records`.
fn assert_operator_verbs_cannot_read(rel: &str) {
    const ID: &str = "DDR-01JDQ2Y7Z8K4M5N6P7Q8R00001";
    let t = tempfile::tempdir().unwrap();
    let top = t.path().join("repo");
    let Some(_g) = locked(&top, rel) else { return };
    for a in [&["ratify", ID, "--yes"][..], &["overturn", ID, "--reason", "r", "--yes"]] {
        let r = run(&top, a);
        assert_eq!((r.code, r.out.as_str(), r.err.as_str()), (1, "", CANNOT), "{a:?}");
    }
}
#[test] fn operator_verbs_with_decisions_at_mode_000() { assert_operator_verbs_cannot_read("docs/decisions"); }
#[test] fn operator_verbs_with_docs_at_mode_000() { assert_operator_verbs_cannot_read("docs"); } // first lstat fails
#[test] fn regular_file_is_still_not_a_directory() {                                       // D§6.8 case 1
    let t = tempfile::tempdir().unwrap();
    let top = t.path().join("repo");
    fs::create_dir_all(top.join("docs")).unwrap();
    fs::write(top.join("docs/decisions"), "x").unwrap();
    assert_eq!(read_dir(&top), Err(DirError::NotDirectory));
    let r = run(&top, &["ls"]);
    assert_eq!((r.code, r.out.as_str(), r.err.as_str()), (1, "", "zygo decision: docs/decisions is not a directory\n"));
}
