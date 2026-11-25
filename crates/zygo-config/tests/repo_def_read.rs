//! WD-1: the git read of `.zygo/` (R§6.1 steps 2-5, R§7.4 pinned read; R§18 items 1, 2, 3, 18).
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};
use zygo_config::repo_def::{self, Files, GitRunner, Read, SystemGit};

const MAIN: &str = "refs/remotes/origin/main";

/// `git -C <dir> <args>` with no user or system config, no signing, a fixed identity, `core.autocrlf=false` and
/// `core.protectNTFS=false` (Git for Windows refuses an index path with a control character otherwise; the
/// fixtures never write such a path to disk: `w` holds index-only commits and `c` is cloned `--no-checkout`).
fn git_cmd(dir: &Path, args: &[&str]) -> Command {
    let empty = if cfg!(windows) { "NUL" } else { "/dev/null" };
    let mut c = Command::new("git");
    c.env("GIT_CONFIG_GLOBAL", empty).env("GIT_CONFIG_NOSYSTEM", "1")
        .args(["-c", "commit.gpgsign=false", "-c", "user.name=t", "-c", "user.email=t@example.invalid",
               "-c", "core.autocrlf=false", "-c", "core.protectNTFS=false", "-C"]).arg(dir).args(args);
    c
}

/// Runs it, asserts success, returns stdout trimmed.
fn git(dir: &Path, args: &[&str]) -> String {
    let o = git_cmd(dir, args).stdin(Stdio::null()).output().expect("git on PATH");
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8(o.stdout).unwrap().trim().to_string()
}

/// `hash-object -w --stdin` of `bytes` in `dir`; returns the blob id.
fn blob(dir: &Path, bytes: &[u8]) -> String {
    let mut child = git_cmd(dir, &["hash-object", "-w", "--stdin"])
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().expect("git on PATH");
    child.stdin.take().unwrap().write_all(bytes).unwrap();
    let o = child.wait_with_output().unwrap();
    assert!(o.status.success(), "hash-object: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8(o.stdout).unwrap().trim().to_string()
}

enum E<'a> { File(&'a str, &'a [u8]), Exec(&'a str, &'a [u8]), Link(&'a str, &'a str), Gitlink(&'a str) }

/// Adds `ents` to the index of `dir` with `update-index --add --cacheinfo <mode>,<oid>,<path>` (100644, 100755,
/// 120000 with the target as blob, 160000 with the commit `HEAD`), nothing on disk; then `commit -q -m c`.
fn commit_index(dir: &Path, ents: &[E]) {
    for e in ents {
        let (mode, oid, path) = match e {
            E::File(p, b) => ("100644", blob(dir, b), *p),
            E::Exec(p, b) => ("100755", blob(dir, b), *p),
            E::Link(p, t) => ("120000", blob(dir, t.as_bytes()), *p),
            E::Gitlink(p) => ("160000", git(dir, &["rev-parse", "HEAD"]), *p),
        };
        git(dir, &["update-index", "--add", "--cacheinfo", &format!("{mode},{oid},{path}")]);
    }
    git(dir, &["commit", "-q", "-m", "c"]);
}

struct Fx { _t: tempfile::TempDir, w: PathBuf, c: PathBuf }

/// `o.git`: `init -q --bare -b main`. `w`: `init -q -b main`, a first commit holding `README` (`seed`, via
/// `commit_index`), then `commit_index(ents)`, `remote add origin ../o.git`, `push -q origin main`.
/// `c`: `clone -q --no-checkout o.git c` (so `refs/remotes/origin/HEAD` -> `refs/remotes/origin/main`).
fn fx(ents: &[E]) -> Fx {
    let t = tempfile::tempdir().unwrap();
    let (o, w, c) = (t.path().join("o.git"), t.path().join("w"), t.path().join("c"));
    git(t.path(), &["init", "-q", "--bare", "-b", "main", o.to_str().unwrap()]);
    git(t.path(), &["init", "-q", "-b", "main", w.to_str().unwrap()]);
    commit_index(&w, &[E::File("README", b"seed")]);
    commit_index(&w, ents);
    git(&w, &["remote", "add", "origin", "../o.git"]);
    git(&w, &["push", "-q", "origin", "main"]);
    git(t.path(), &["clone", "-q", "--no-checkout", "o.git", "c"]);
    Fx { _t: t, w, c }
}

/// Records each call's args and deadline; fails the calls whose subcommand is `fail` (`"*"`: all) with
/// `Err("tool not found")`; else runs `SystemGit` with `Instant::now() + Duration::from_secs(600)`; with
/// `short_blob`, drops the last stdout byte of `cat-file blob`.
#[derive(Default)]
struct Rec { calls: RefCell<Vec<(Vec<String>, Instant)>>, fail: Option<&'static str>, short_blob: bool }

impl GitRunner for Rec {
    fn git(&self, top: &Path, args: &[&str], deadline: Instant) -> Result<Output, String> {
        self.calls.borrow_mut().push((args.iter().map(|a| a.to_string()).collect(), deadline));
        if self.fail.is_some_and(|f| f == "*" || args.first() == Some(&f)) {
            return Err("tool not found".into());
        }
        let mut o = SystemGit.git(top, args, Instant::now() + Duration::from_secs(600))?;
        if self.short_blob && args.starts_with(&["cat-file", "blob"]) {
            o.stdout.pop();
        }
        Ok(o)
    }
}

impl Rec {
    fn args(&self) -> Vec<Vec<String>> {
        self.calls.borrow().iter().map(|c| c.0.clone()).collect()
    }
    /// Calls that start with `cat-file blob`.
    fn content_reads(&self) -> usize {
        self.args().iter().filter(|a| a.len() >= 2 && a[0] == "cat-file" && a[1] == "blob").count()
    }
}

fn head(c: &Path) -> String { git(c, &["rev-parse", MAIN]) }
fn found(r: Read) -> Files { match r { Read::Found { files } => files, other => panic!("{other:?}") } }

fn base() -> Fx {
    fx(&[E::File(".zygo/workspace.toml", b"version = 1\n"), E::File(".zygo/profiles/rust.toml", b"[overrides]\n"),
         E::Exec(".zygo/profiles/go.toml", b"x"), E::File(".zygo/profiles/notes.md", b"n"), E::File(".zygo/README", b"r")])
}

#[test]
fn found_reads_the_default_branch_files() {
    let f = base();
    let r = repo_def::read_default(&f.c, &Rec::default());
    assert_eq!(r, Read::Found { files: Files {
        full_ref: Some(MAIN.into()), commit: head(&f.c), workspace: Some(b"version = 1\n".to_vec()),
        profiles: BTreeMap::from([("go".into(), b"x".to_vec()), ("rust".into(), b"[overrides]\n".to_vec())]),
        ignored: vec![".zygo/README".into(), ".zygo/profiles/notes.md".into()] } });
}

#[test] // review focus 29; R§6.1 step 2 "each later call names that commit id"; one budget
fn calls_name_the_commit_or_a_full_ref() {
    let f = base();
    let c = head(&f.c);
    let oid = |p: &str| git(&f.c, &["rev-parse", &format!("{c}:{p}")]);
    let rec = Rec::default();
    found(repo_def::read_default(&f.c, &rec));
    let a = rec.args();
    assert_eq!(a.len(), 8);
    assert_eq!(a[0], ["symbolic-ref", "--quiet", "refs/remotes/origin/HEAD"]);
    assert_eq!(a[1], ["rev-parse", "--verify", "--quiet", "refs/remotes/origin/main^{commit}"]);
    assert_eq!(a[2], ["ls-tree", "-z", "--long", "--full-tree", c.as_str(), "--", ".zygo"]);
    assert_eq!(a[3], ["ls-tree", "-z", "--long", "--full-tree", c.as_str(), "--", ".zygo/"]);
    assert_eq!(a[4], ["ls-tree", "-z", "--long", "--full-tree", c.as_str(), "--", ".zygo/profiles/"]);
    assert_eq!(a[5], ["cat-file", "blob", oid(".zygo/workspace.toml").as_str()]);
    assert_eq!(a[6], ["cat-file", "blob", oid(".zygo/profiles/go.toml").as_str()]);
    assert_eq!(a[7], ["cat-file", "blob", oid(".zygo/profiles/rust.toml").as_str()]);
    for arg in a.iter().flatten() {
        assert!(!["main", "HEAD", "origin/HEAD", "origin/main"].contains(&arg.as_str()), "short name {arg}");
    }
    let d: Vec<Instant> = rec.calls.borrow().iter().map(|x| x.1).collect();
    assert!(d.windows(2).all(|w| w[0] == w[1]), "one deadline per read");
}

#[test] // R§18 item 1
fn default_branch_only() {
    let f = base();
    git(&f.c, &["reset", "-q", "--hard"]);
    let first = found(repo_def::read_default(&f.c, &Rec::default()));
    let wt = f.c.parent().unwrap().join("wt");
    git(&f.c, &["worktree", "add", "-q", "-b", "side", wt.to_str().unwrap(), MAIN]);
    std::fs::write(wt.join(".zygo/workspace.toml"), "version = 2\n").unwrap();
    assert_eq!(found(repo_def::read_default(&f.c, &Rec::default())), first, "edit in a linked worktree");
    assert_eq!(found(repo_def::read_default(&wt, &Rec::default())), first, "read from the linked worktree");
    git(&wt, &["commit", "-q", "-am", "side"]);
    assert_eq!(found(repo_def::read_default(&f.c, &Rec::default())), first, "commit on another branch");
    std::fs::write(f.c.join(".zygo/workspace.toml"), "version = 3\n").unwrap();
    assert_eq!(found(repo_def::read_default(&f.c, &Rec::default())), first, "working-tree edit");
    git(&f.c, &["commit", "-q", "-am", "local"]);
    assert_eq!(found(repo_def::read_default(&f.c, &Rec::default())), first, "unpushed local commit");
    git(&f.c, &["push", "-q", "origin", "main"]);
    let after = found(repo_def::read_default(&f.c, &Rec::default()));
    assert_eq!(after.workspace.as_deref(), Some(&b"version = 3\n"[..]));
    assert_ne!(after.commit, first.commit);
}

fn invalid(f: &Fx, why: &str) -> Read { Read::Invalid { full_ref: Some(MAIN.into()), commit: head(&f.c), why: why.into() } }

#[test] // R§18 item 2; review focus 30
fn tree_entries_are_refused_before_any_content_read() {
    let pattern = "a profile file name must match ^[a-z0-9-]{1,32}\\.toml$";
    let long = format!(".zygo/profiles/{}.toml", "a".repeat(33));
    let cases: Vec<(Vec<E>, String)> = vec![
        (vec![E::Link(".zygo", "elsewhere")], ".zygo is not a directory".into()),
        (vec![E::Gitlink(".zygo")], ".zygo is not a directory".into()),
        (vec![E::File(".zygo", b"x")], ".zygo is not a directory".into()),
        (vec![E::Link(".zygo/workspace.toml", "../w.toml")], ".zygo/workspace.toml is not a regular file".into()),
        (vec![E::File(".zygo/workspace.toml/a", b"x")], ".zygo/workspace.toml is not a regular file".into()),
        (vec![E::File(".zygo/profiles", b"x")], ".zygo/profiles is not a directory".into()),
        (vec![E::Link(".zygo/profiles/x.toml", "../x")], ".zygo/profiles/x.toml is not a regular file".into()),
        (vec![E::File(".zygo/profiles/x.toml/a", b"x")], ".zygo/profiles/x.toml is not a regular file".into()),
        (vec![E::File(".zygo/profiles/Rust.toml", b"x")], format!(".zygo/profiles/Rust.toml: {pattern}")),
        (vec![E::File(".zygo/profiles/None.toml", b"x")], format!(".zygo/profiles/None.toml: {pattern}")),
        (vec![E::File(".zygo/profiles/.toml", b"x")], format!(".zygo/profiles/.toml: {pattern}")),
        (vec![E::File(&long, b"x")], format!("{long}: {pattern}")),
        (vec![E::File(".zygo/profiles/a\u{1b}b.toml", b"x")], format!(".zygo/profiles/a\\u{{1b}}b.toml: {pattern}")),
        (vec![E::File(".zygo/profiles/none.toml", b"x")], ".zygo/profiles/none.toml: the profile name none is reserved".into()),
    ];
    for (ents, why) in cases {
        let f = fx(&ents);
        let rec = Rec::default();
        assert_eq!(repo_def::read_default(&f.c, &rec), invalid(&f, &why), "{why}");
        assert_eq!(rec.content_reads(), 0, "content read before the check: {why}");
    }
}

#[test] // R§18 item 3; review focus 30
fn sizes_are_refused_before_any_content_read() {
    let big_ws = vec![b'a'; 65537];
    let big_profile = vec![b'a'; 16385];
    let names: Vec<String> = (0..33).map(|i| format!(".zygo/profiles/p{i:02}.toml")).collect();
    let many: Vec<E> = names.iter().map(|n| E::File(n, b"x")).collect();
    let cases: Vec<(Vec<E>, &str)> = vec![
        (vec![E::File(".zygo/workspace.toml", &big_ws)], ".zygo/workspace.toml is larger than 65536 bytes"),
        (vec![E::File(".zygo/profiles/big.toml", &big_profile)], ".zygo/profiles/big.toml is larger than 16384 bytes"),
        (many, ".zygo/profiles holds more than 32 profile files"),
    ];
    for (ents, why) in cases {
        let f = fx(&ents);
        let rec = Rec::default();
        assert_eq!(repo_def::read_default(&f.c, &rec), invalid(&f, why), "{why}");
        assert_eq!(rec.content_reads(), 0, "content read before the size check: {why}");
    }
}

#[test] // the limits themselves are allowed
fn limits_are_the_spec_values() {
    assert_eq!((repo_def::WORKSPACE_MAX, repo_def::PROFILE_MAX, repo_def::PROFILES_MAX), (65536, 16384, 32));
    let ws = vec![b'a'; 65536];
    let p = vec![b'b'; 16384];
    let names: Vec<String> = (0..32).map(|i| format!(".zygo/profiles/p{i:02}.toml")).collect();
    let mut ents: Vec<E> = names.iter().map(|n| E::File(n, &p)).collect();
    ents.push(E::File(".zygo/workspace.toml", &ws));
    let files = found(repo_def::read_default(&fx(&ents).c, &Rec::default()));
    assert_eq!(files.workspace.map(|w| w.len()), Some(65536));
    assert_eq!(files.profiles.len(), 32);
    assert!(files.profiles.values().all(|v| v.len() == 16384));
}

#[test] // R§6.1 step 4: nothing to read
fn absent_definitions() {
    let none = fx(&[E::File("src/lib.rs", b"")]);
    assert_eq!(repo_def::read_default(&none.c, &Rec::default()), Read::Absent { full_ref: Some(MAIN.into()) });
    let empty = fx(&[E::File(".zygo/README", b"r"), E::File(".zygo/profiles/notes.md", b"n")]);
    assert_eq!(repo_def::read_default(&empty.c, &Rec::default()), Read::Absent { full_ref: Some(MAIN.into()) });
}

#[test] // R§18 item 18; review focus 29
fn origin_head_outside_is_unread() {
    let f = base();
    let unread = Read::Unread { why: "origin/HEAD points outside refs/remotes/origin/".into() };
    git(&f.c, &["symbolic-ref", "refs/remotes/origin/HEAD", "refs/heads/x"]);
    assert_eq!(repo_def::read_default(&f.c, &Rec::default()), unread);
    git(&f.c, &["symbolic-ref", "--delete", "refs/remotes/origin/HEAD"]);
    git(&f.c, &["update-ref", "--no-deref", "refs/remotes/origin/HEAD", &head(&f.c)]);
    assert_eq!(repo_def::read_default(&f.c, &Rec::default()), unread, "a direct origin/HEAD");
}

#[test] // R§18 item 18: no origin/HEAD; a tag `main` never wins over the branch
fn no_origin_head_reads_the_branch_not_the_tag() {
    let f = base(); // clone `c` has local branch main = origin/main
    git(&f.w, &["checkout", "-q", "-b", "other"]);
    commit_index(&f.w, &[E::File(".zygo/workspace.toml", b"version = 9\n")]);
    git(&f.w, &["push", "-q", "origin", "other"]);
    git(&f.c, &["fetch", "-q", "origin"]);
    git(&f.c, &["symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/other"]);
    let moved = found(repo_def::read_default(&f.c, &Rec::default())); // origin/HEAD moved inside refs/remotes/origin/
    assert_eq!(moved.full_ref.as_deref(), Some("refs/remotes/origin/other"));
    assert_eq!(moved.workspace.as_deref(), Some(&b"version = 9\n"[..]));
    git(&f.c, &["tag", "main", "refs/remotes/origin/other"]);
    git(&f.c, &["symbolic-ref", "--delete", "refs/remotes/origin/HEAD"]);
    let files = found(repo_def::read_default(&f.c, &Rec::default()));
    assert_eq!(files.full_ref.as_deref(), Some("refs/heads/main"));
    assert_eq!(files.commit, git(&f.c, &["rev-parse", "refs/heads/main"]));
    assert_eq!(files.workspace.as_deref(), Some(&b"version = 1\n"[..]));
    git(&f.c, &["update-ref", "-d", "refs/heads/main"]);
    assert_eq!(repo_def::read_default(&f.c, &Rec::default()), Read::Absent { full_ref: Some("refs/heads/main".into()) });
}

#[test] // R§7.4: steps 3-5 at the saved commit, no default-branch lookup
fn read_at_reads_the_pinned_commit() {
    let f = base();
    let pinned = found(repo_def::read_default(&f.c, &Rec::default()));
    commit_index(&f.w, &[E::File(".zygo/workspace.toml", b"version = 5\n")]);
    git(&f.w, &["push", "-q", "origin", "main"]);
    git(&f.c, &["fetch", "-q", "origin"]);
    let rec = Rec::default();
    let at = found(repo_def::read_at(&f.c, &pinned.commit, &rec));
    assert_eq!(at, Files { full_ref: None, ..pinned.clone() });
    for call in rec.args() {
        assert!(call[0] == "ls-tree" || call[0] == "cat-file", "{call:?}");
        assert!(call.iter().all(|a| !a.starts_with("refs/")), "{call:?}");
    }
}

#[test] // R§7.4: commit gone after a force-push and gc
fn read_at_of_a_pruned_commit_is_gone() {
    let f = base();
    git(&f.c, &["reset", "-q", "--hard"]);
    git(&f.c, &["checkout", "-q", "-b", "side"]);
    std::fs::write(f.c.join(".zygo/workspace.toml"), "version = 7\n").unwrap();
    git(&f.c, &["commit", "-q", "-am", "side"]);
    let gone = git(&f.c, &["rev-parse", "HEAD"]);
    git(&f.c, &["checkout", "-q", "main"]);
    git(&f.c, &["branch", "-q", "-D", "side"]);
    git(&f.c, &["reflog", "expire", "--expire=now", "--all"]);
    git(&f.c, &["gc", "-q", "--prune=now"]);
    assert!(!git_cmd(&f.c, &["cat-file", "-e", &gone]).status().unwrap().success(), "fixture: commit still there");
    assert_eq!(repo_def::read_at(&f.c, &gone, &Rec::default()), Read::Gone { commit: gone.clone() });
}

#[test] // review focus 29: a pin is never passed to git unless it is an object id
fn read_at_refuses_a_non_id() {
    let f = base();
    let (upper, short) = ("A".repeat(40), "a".repeat(39));
    for bad in ["main", "-x", "", "refs/heads/main", upper.as_str(), short.as_str()] {
        let rec = Rec::default();
        assert_eq!(repo_def::read_at(&f.c, bad, &rec),
                   Read::Unread { why: format!("pinned commit {bad} is not an object id") });
        assert!(rec.args().is_empty(), "{bad}: git called");
    }
}

#[test] // R§6.1 results: a failed git call is unread, naming the call
fn failing_runner_is_unread() {
    let f = base();
    let all = Rec { fail: Some("*"), ..Rec::default() };
    assert_eq!(repo_def::read_default(&f.c, &all), Read::Unread { why: "git symbolic-ref: tool not found".into() });
    let c = head(&f.c);
    let lst = Rec { fail: Some("ls-tree"), ..Rec::default() };
    assert_eq!(repo_def::read_at(&f.c, &c, &lst), Read::Unread { why: "git ls-tree: tool not found".into() });
    assert!(lst.args().contains(&vec!["cat-file".to_string(), "-e".into(), c.clone()]), "Gone confirmed first");
}

#[test] // R§6.1 step 5
fn content_must_match_size_and_be_utf8() {
    let f = fx(&[E::File(".zygo/workspace.toml", &[0xff, 0xfe])]);
    assert_eq!(repo_def::read_default(&f.c, &Rec::default()), invalid(&f, ".zygo/workspace.toml is not UTF-8"));
    let f = fx(&[E::File(".zygo/workspace.toml", b"version = 1\n")]);
    let short = Rec { short_blob: true, ..Rec::default() };
    assert_eq!(repo_def::read_default(&f.c, &short), invalid(&f, ".zygo/workspace.toml: read 11 bytes, listed 12"));
}
