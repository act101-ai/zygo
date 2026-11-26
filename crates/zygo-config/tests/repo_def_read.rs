//! WD-1: the git read of `.zygo/` (R§6.1 steps 2-5, R§7.4 pinned read; R§18 items 1, 2, 3, 18).
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Output, Stdio};
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

/// The output that a call returns through `edit` in place of git's.
type Edit = Box<dyn Fn(&[&str], Output) -> Output>;

/// Records each call's args and deadline; fails the calls whose subcommand is `fail` (`"*"`: all) with
/// `Err("tool not found")`; else runs `SystemGit` with `Instant::now() + Duration::from_secs(600)`; with
/// `short_blob`, drops the last stdout byte of `cat-file blob`; then passes the output through `edit`.
#[derive(Default)]
struct Rec { calls: RefCell<Vec<(Vec<String>, Instant)>>, fail: Option<&'static str>, short_blob: bool, edit: Option<Edit> }

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
        Ok(match &self.edit { Some(e) => e(args, o), None => o })
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

/// A process exit with `code`.
fn status(code: i32) -> ExitStatus {
    #[cfg(unix)]
    return std::os::unix::process::ExitStatusExt::from_raw(code << 8);
    #[cfg(windows)]
    return std::os::windows::process::ExitStatusExt::from_raw(code as u32);
}

fn out(code: i32, stdout: &[u8], stderr: &[u8]) -> Output { Output { status: status(code), stdout: stdout.to_vec(), stderr: stderr.to_vec() } }

/// A `Rec` whose calls that `when` matches return `o`.
fn patched(when: fn(&[&str]) -> bool, o: Output) -> Rec {
    Rec { edit: Some(Box::new(move |a, real| if when(a) { o.clone() } else { real })), ..Rec::default() }
}

/// Removes the loose object `oid` from the repository at `dir` (the fixture's objects are loose: fewer than
/// `transfer.unpackLimit` were pushed, and a local clone links them). Git writes them read-only, which Windows
/// refuses to remove.
fn drop_object(dir: &Path, oid: &str) {
    let p = dir.join(".git/objects").join(&oid[..2]).join(&oid[2..]);
    #[cfg(windows)]
    #[allow(clippy::permissions_set_readonly_false)] // Windows only: clears the read-only attribute
    {
        let mut perm = std::fs::metadata(&p).unwrap().permissions();
        perm.set_readonly(false);
        std::fs::set_permissions(&p, perm).unwrap();
    }
    std::fs::remove_file(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    assert!(!git_cmd(dir, &["cat-file", "-e", oid]).status().unwrap().success(), "fixture: {oid} still there");
}

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
    assert_eq!(a.len(), 10);
    assert_eq!(a[0], ["symbolic-ref", "--quiet", "refs/remotes/origin/HEAD"]);
    assert_eq!(a[1], ["show-ref", "--verify", "--quiet", "refs/remotes/origin/main"]);
    assert_eq!(a[2], ["show-ref", "--verify", "--hash", "refs/remotes/origin/main"]);
    assert_eq!(a[3], ["rev-parse", "--verify", "--quiet", format!("{c}^{{commit}}").as_str()]);
    assert_eq!(a[4], ["ls-tree", "-z", "--long", "--full-tree", c.as_str(), "--", ".zygo"]);
    assert_eq!(a[5], ["ls-tree", "-z", "--long", "--full-tree", c.as_str(), "--", ".zygo/"]);
    assert_eq!(a[6], ["ls-tree", "-z", "--long", "--full-tree", c.as_str(), "--", ".zygo/profiles/"]);
    assert_eq!(a[7], ["cat-file", "blob", oid(".zygo/workspace.toml").as_str()]);
    assert_eq!(a[8], ["cat-file", "blob", oid(".zygo/profiles/go.toml").as_str()]);
    assert_eq!(a[9], ["cat-file", "blob", oid(".zygo/profiles/rust.toml").as_str()]);
    for arg in a.iter().flatten() {
        assert!(!["main", "HEAD", "origin/HEAD", "origin/main"].contains(&arg.as_str()), "short name {arg}");
    }
    // A full ref goes only to an exact-match lookup (git resolves a name given to `rev-parse` through
    // `refs/tags/<name>` too); `rev-parse` gets an object id only.
    let hex = |s: &str| (s.len() == 40 || s.len() == 64) && s.bytes().all(|b| b.is_ascii_hexdigit());
    for call in &a {
        if call.iter().any(|x| x.starts_with("refs/")) {
            assert!(call[0] == "symbolic-ref" || call[..2] == ["show-ref", "--verify"], "ref name outside an exact lookup: {call:?}");
        }
        if call[0] == "rev-parse" {
            let last = call.last().unwrap();
            assert!(last.strip_suffix("^{commit}").is_some_and(hex), "rev-parse of a name: {call:?}");
        }
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
    // F1: a tag named `refs/heads/main` (`refs/tags/refs/heads/main`) is never read in place of the branch.
    git(&f.c, &["update-ref", "refs/tags/refs/heads/main", "refs/remotes/origin/other"]);
    git(&f.c, &["update-ref", "-d", "refs/heads/main"]);
    assert_eq!(repo_def::read_default(&f.c, &Rec::default()), Read::Absent { full_ref: Some("refs/heads/main".into()) });
}

#[test] // F1, F7: a dangling origin/HEAD is absent, also when a tag has the target's name
fn dangling_origin_head_is_absent_not_the_tag() {
    let f = base();
    let absent = Read::Absent { full_ref: Some("refs/remotes/origin/gone".into()) };
    git(&f.w, &["checkout", "-q", "-b", "other"]);
    commit_index(&f.w, &[E::File(".zygo/workspace.toml", b"version = 9\n")]);
    git(&f.w, &["push", "-q", "origin", "other"]);
    git(&f.c, &["fetch", "-q", "origin"]); // before the dangling ref: a fetch can repair origin/HEAD
    git(&f.c, &["symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/gone"]);
    assert_eq!(repo_def::read_default(&f.c, &Rec::default()), absent);
    git(&f.c, &["update-ref", "refs/tags/refs/remotes/origin/gone", "refs/remotes/origin/other"]);
    assert_eq!(repo_def::read_default(&f.c, &Rec::default()), absent, "the tag was read");
}

#[test] // F1: a tag named `refs/remotes/origin/HEAD` changes nothing
fn origin_head_tag_changes_nothing() {
    let f = base();
    let first = found(repo_def::read_default(&f.c, &Rec::default()));
    git(&f.w, &["checkout", "-q", "-b", "other"]);
    commit_index(&f.w, &[E::File(".zygo/workspace.toml", b"version = 9\n")]);
    git(&f.w, &["push", "-q", "origin", "other"]);
    git(&f.c, &["fetch", "-q", "origin"]);
    git(&f.c, &["update-ref", "refs/tags/refs/remotes/origin/HEAD", "refs/remotes/origin/other"]);
    assert_eq!(found(repo_def::read_default(&f.c, &Rec::default())), first, "origin/HEAD present");
    git(&f.c, &["symbolic-ref", "--delete", "refs/remotes/origin/HEAD"]);
    let files = found(repo_def::read_default(&f.c, &Rec::default()));
    assert_eq!(files.full_ref.as_deref(), Some("refs/heads/main"), "origin/HEAD missing");
    assert_eq!(files.workspace, first.workspace);
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
                   Read::Unread { why: format!("pinned commit {} is not an object id", repo_def::short(bad)) });
        assert!(rec.args().is_empty(), "{bad}: git called");
    }
}

#[test] // R§6.5 `short`; review F5: at most 12 characters, also when byte 12 is inside a character
fn short_is_at_most_12_characters() {
    // Byte 12 is inside `é` (bytes 11 and 12).
    let s = format!("{}é{}", "a".repeat(11), "x".repeat(100_000));
    let got = repo_def::short(&s);
    assert!(got.chars().count() <= 12, "short gave {} characters", got.chars().count());
    assert_eq!(got, format!("{}é", "a".repeat(11)));
    assert_eq!(repo_def::short("abc"), "abc");
    assert_eq!(repo_def::short(&"f".repeat(64)), "f".repeat(12));
}

#[test] // review F5: the unread reason of a non-id pin prints at most 12 characters of it
fn read_at_of_a_huge_non_id_has_a_short_reason() {
    let f = base();
    let bad = "z".repeat(1 << 20);
    let rec = Rec::default();
    let Read::Unread { why } = repo_def::read_at(&f.c, &bad, &rec) else { panic!("not unread") };
    assert!(why.len() < 1024, "the reason has {} bytes", why.len());
    assert_eq!(why, format!("pinned commit {} is not an object id", "z".repeat(12)));
    assert!(rec.args().is_empty(), "git called");
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

#[test] // F2: a blobless clone is read with no fetch; a blob that is not here is gone (R§7.4), whether pruned or promised
fn partial_clone_is_read_without_a_fetch() {
    let f = base();
    let t = f.c.parent().unwrap();
    git(&t.join("o.git"), &["config", "uploadpack.allowFilter", "true"]);
    git(t, &["clone", "-q", "--no-local", "--filter=blob:none", "--no-checkout", "o.git", "p"]);
    let p = t.join("p");
    let ws = git(&p, &["rev-parse", &format!("{}:.zygo/workspace.toml", git(&p, &["rev-parse", MAIN]))]);
    let here = |p: &Path| git_cmd(p, &["cat-file", "-e", &ws]).env("GIT_NO_LAZY_FETCH", "1").status().unwrap().success();
    assert!(!here(&p), "fixture: blob fetched by the clone");
    let c = head(&p);
    assert_eq!(repo_def::read_default(&p, &Rec::default()), Read::Unread { why: format!("commit {c} or one of its objects is missing") });
    assert_eq!(repo_def::read_at(&p, &c, &Rec::default()), Read::Gone { commit: c.clone() });
    assert!(!here(&p), "the read fetched the blob");
}

#[test] // F2, F4, F5: the environment of each SystemGit call
fn system_git_command_environment() {
    let c = SystemGit::command();
    let envs: BTreeMap<String, Option<String>> = c.get_envs()
        .map(|(k, v)| (k.to_string_lossy().into_owned(), v.map(|v| v.to_string_lossy().into_owned()))).collect();
    for (k, v) in [("GIT_NO_LAZY_FETCH", "1"), ("GIT_TERMINAL_PROMPT", "0"), ("GIT_NO_REPLACE_OBJECTS", "1"), ("LC_ALL", "C")] {
        assert_eq!(envs.get(k), Some(&Some(v.to_string())), "{k}");
    }
    for k in ["GIT_ICASE_PATHSPECS", "GIT_GLOB_PATHSPECS", "GIT_NOGLOB_PATHSPECS", "GIT_LITERAL_PATHSPECS", "GIT_DIR"] {
        assert_eq!(envs.get(k), Some(&None), "{k} not removed");
    }
}

#[test] // F4: a replace ref does not change the read
fn replace_refs_are_ignored() {
    let f = base();
    let first = found(repo_def::read_default(&f.c, &Rec::default()));
    let ws = git(&f.c, &["rev-parse", &format!("{}:.zygo/workspace.toml", first.commit)]);
    let other = blob(&f.c, b"version = 8\n");
    git(&f.c, &["replace", &ws, &other]);
    assert_eq!(git(&f.c, &["cat-file", "blob", &ws]), "version = 8", "fixture: replace not in effect");
    assert_eq!(found(repo_def::read_default(&f.c, &Rec::default())), first);
}

#[test] // F6: the commit is here, its root tree is not
fn read_at_with_the_root_tree_gone_is_gone() {
    let f = base();
    let c = head(&f.c);
    drop_object(&f.c, &git(&f.c, &["rev-parse", &format!("{c}^{{tree}}")]));
    assert_eq!(repo_def::read_at(&f.c, &c, &Rec::default()), Read::Gone { commit: c.clone() });
}

#[test] // F7: a subtree or a blob gone while the commit stays
fn read_at_with_a_subtree_or_blob_gone_is_gone() {
    for path in [".zygo", ".zygo/profiles", ".zygo/workspace.toml", ".zygo/profiles/rust.toml"] {
        let f = base();
        let c = head(&f.c);
        drop_object(&f.c, &git(&f.c, &["rev-parse", &format!("{c}:{path}")]));
        assert_eq!(repo_def::read_at(&f.c, &c, &Rec::default()), Read::Gone { commit: c.clone() }, "{path}");
    }
}

#[test] // F7: R§6.1 has no gone result; read_default gives unread
fn read_default_maps_gone_to_unread() {
    let f = base();
    let c = head(&f.c);
    drop_object(&f.c, &git(&f.c, &["rev-parse", &format!("{c}:.zygo/workspace.toml")]));
    assert_eq!(repo_def::read_default(&f.c, &Rec::default()),
               Read::Unread { why: format!("commit {c} or one of its objects is missing") });
}

#[test] // F7: only a quiet exit 1 of `cat-file -e` is a missing object
fn exit_1_with_stderr_is_not_gone() {
    let f = base();
    let c = head(&f.c);
    let rec = Rec { edit: Some(Box::new(|a, real| match a[0] {
        "ls-tree" => out(128, b"", b"fatal: x\n"),
        "cat-file" if a[1] == "-e" => out(1, b"", b"error: x\n"),
        _ => real,
    })), ..Rec::default() };
    assert_eq!(repo_def::read_at(&f.c, &c, &rec), Read::Unread { why: "git ls-tree: failed".into() });
}

#[test] // F7: a non-zero git exit, not a runner error, is unread
fn non_zero_exit_is_unread() {
    let f = base();
    let sym = patched(|a| a[0] == "symbolic-ref", out(128, b"", b"fatal: x\n"));
    assert_eq!(repo_def::read_default(&f.c, &sym), Read::Unread { why: "git symbolic-ref: failed".into() });
    let lst = patched(|a| a[0] == "ls-tree", out(128, b"", b"fatal: x\n"));
    assert_eq!(repo_def::read_at(&f.c, &head(&f.c), &lst), Read::Unread { why: "git ls-tree: failed".into() });
    let blob = patched(|a| a[..2] == ["cat-file", "blob"], out(128, b"", b"fatal: x\n"));
    assert_eq!(repo_def::read_at(&f.c, &head(&f.c), &blob), Read::Unread { why: "git cat-file: failed".into() });
}

#[test] // F7: an `ls-tree` listing of any other shape is unread
fn odd_ls_tree_output_is_unread() {
    let f = base();
    let c = head(&f.c);
    let z = git(&f.c, &["rev-parse", &format!("{c}:.zygo")]);
    let w = git(&f.c, &["rev-parse", &format!("{c}:.zygo/workspace.toml")]);
    let top = |a: &[&str]| a[0] == "ls-tree" && a.last() == Some(&".zygo");
    let sub = |a: &[&str]| a[0] == "ls-tree" && a.last() == Some(&".zygo/");
    type When = fn(&[&str]) -> bool;
    let cases: Vec<(&str, When, String)> = vec![
        ("no NUL", top, format!("040000 tree {z}      -\t.zygo")),
        ("three fields", top, format!("040000 tree {z}\t.zygo\0")),
        ("oid not hex", top, format!("040000 tree {}      -\t.zygo\0", "g".repeat(40))),
        ("size BAD for a blob that is here", sub, format!("100644 blob {w}     BAD\t.zygo/workspace.toml\0")),
        ("path outside the prefix", sub, format!("100644 blob {w}      12\tother/workspace.toml\0")),
        ("path one level deeper", sub, format!("100644 blob {w}      12\t.zygo/a/workspace.toml\0")),
        ("two records for .zygo", top, format!("040000 tree {z}      -\t.zygo\0040000 tree {z}      -\t.zygo\0")),
    ];
    for (name, when, stdout) in cases {
        let rec = patched(when, out(0, stdout.as_bytes(), b""));
        assert_eq!(repo_def::read_at(&f.c, &c, &rec), Read::Unread { why: "git ls-tree: failed".into() }, "{name}");
    }
}

#[test] // F7: R§6.1 step 5, the byte count both ways
fn long_content_and_short_profile_are_invalid() {
    let f = fx(&[E::File(".zygo/workspace.toml", b"version = 1\n")]);
    let long = Rec { edit: Some(Box::new(|a, mut real| {
        if a[..2] == ["cat-file", "blob"] { real.stdout.push(b'\n') }
        real
    })), ..Rec::default() };
    assert_eq!(repo_def::read_default(&f.c, &long), invalid(&f, ".zygo/workspace.toml: read 13 bytes, listed 12"));
    let f = fx(&[E::File(".zygo/profiles/rust.toml", b"[overrides]\n")]);
    let short = Rec { short_blob: true, ..Rec::default() };
    assert_eq!(repo_def::read_default(&f.c, &short), invalid(&f, ".zygo/profiles/rust.toml: read 11 bytes, listed 12"));
}

#[test] // F11: ignored paths in byte order
fn ignored_is_in_byte_order() {
    let f = fx(&[E::File(".zygo/workspace.toml", b"version = 1\n"), E::File(".zygo/README", b"r"),
                 E::File(".zygo/profiles/notes.md", b"n"), E::File(".zygo/zzz", b"z")]);
    let files = found(repo_def::read_default(&f.c, &Rec::default()));
    assert_eq!(files.ignored, [".zygo/README", ".zygo/profiles/notes.md", ".zygo/zzz"]);
}
