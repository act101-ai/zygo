//! WD-4: checked use (R§7.3, R§7.4; R§18 items 6, 8).
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};
use zygo_config::repo_def::{self, Files, GitRunner, Pin, PinnedUse, SystemGit, Use};

const MAIN: &str = "refs/remotes/origin/main";
const ID: &str = "3f9a1c0d5e7b2a46";
const ID2: &str = "0123456789abcdef";
/// A and B have the same length, so a swapped blob passes the size check of R§6.1 step 5.
const A: &str = "version = 1\nlayout = \"main-vertical\"\n";
const B: &str = "version = 1\nlayout = \"even-vertical\"\n";

/// `git -C <dir> <args>` with no user or system config, no signing, a fixed identity, `core.autocrlf=false` and
/// `core.protectNTFS=false`.
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

/// Adds each regular file to the index of `dir` (nothing on disk), then `commit -q -m c`.
fn commit_files(dir: &Path, ents: &[(&str, &[u8])]) {
    for (path, bytes) in ents {
        let oid = blob(dir, bytes);
        git(dir, &["update-index", "--add", "--cacheinfo", &format!("100644,{oid},{path}")]);
    }
    git(dir, &["commit", "-q", "-m", "c"]);
}

struct Fx { _t: tempfile::TempDir, w: PathBuf, c: PathBuf }

/// `o.git` (bare), `w` with a `README` seed commit and then `ents` (not empty), pushed; `c` cloned `--no-checkout`.
fn fx(ents: &[(&str, &[u8])]) -> Fx {
    let t = tempfile::tempdir().unwrap();
    let (o, w, c) = (t.path().join("o.git"), t.path().join("w"), t.path().join("c"));
    git(t.path(), &["init", "-q", "--bare", "-b", "main", o.to_str().unwrap()]);
    git(t.path(), &["init", "-q", "-b", "main", w.to_str().unwrap()]);
    commit_files(&w, &[("README", b"seed")]);
    commit_files(&w, ents);
    git(&w, &["remote", "add", "origin", "../o.git"]);
    git(&w, &["push", "-q", "origin", "main"]);
    git(t.path(), &["clone", "-q", "--no-checkout", "o.git", "c"]);
    Fx { _t: t, w, c }
}

fn push_change(f: &Fx, ents: &[(&str, &[u8])]) {
    commit_files(&f.w, ents);
    git(&f.w, &["push", "-q", "origin", "main"]);
    git(&f.c, &["fetch", "-q", "origin"]);
}

fn head(c: &Path) -> String {
    git(c, &["rev-parse", MAIN])
}

fn ws(text: &str) -> Files {
    Files {
        full_ref: None,
        commit: "a".repeat(40),
        workspace: Some(text.as_bytes().to_vec()),
        profiles: BTreeMap::new(),
        ignored: Vec::new(),
    }
}

fn d(text: &str) -> String {
    repo_def::digest(&ws(text))
}

fn parsed(text: &str) -> repo_def::Definition {
    repo_def::parse(&ws(text)).unwrap()
}

fn store(f: &Fx) -> PathBuf {
    f._t.path().join("approvals.toml")
}

/// The counting runner: records each call; with `fail`, every call is `Err("tool not found")`; else `SystemGit`.
/// With `swap = Some((n, bytes))`, once `n` reads have started (`roots()`), a `cat-file blob` call gives `bytes`.
#[derive(Default)]
struct Cnt { calls: RefCell<Vec<Vec<String>>>, fail: bool, swap: Option<(usize, &'static [u8])> }

impl Cnt {
    /// One `symbolic-ref` call per `read_default`.
    fn reads(&self) -> usize {
        self.calls.borrow().iter().filter(|c| c[0] == "symbolic-ref").count()
    }

    /// One `ls-tree ... -- .zygo` call per read that reaches R§6.1 step 3, by `read_default` or `read_at`.
    fn roots(&self) -> usize {
        self.calls.borrow().iter().filter(|c| c[0] == "ls-tree" && c.last().map(String::as_str) == Some(".zygo")).count()
    }

    fn blobs(&self) -> usize {
        self.calls.borrow().iter().filter(|c| c[0] == "cat-file" && c.get(1).map(String::as_str) == Some("blob")).count()
    }
}

impl GitRunner for Cnt {
    fn git(&self, top: &Path, args: &[&str], _deadline: Instant) -> Result<Output, String> {
        self.calls.borrow_mut().push(args.iter().map(|a| a.to_string()).collect());
        if self.fail {
            return Err("tool not found".into());
        }
        let mut o = SystemGit.git(top, args, Instant::now() + Duration::from_secs(600))?;
        if let Some((n, bytes)) = self.swap {
            if self.roots() >= n && args.len() >= 2 && args[0] == "cat-file" && args[1] == "blob" {
                o.stdout = bytes.to_vec();
            }
        }
        Ok(o)
    }
}

#[test] // R§6.1 results pass through; a parse failure is Invalid at the read commit
fn absent_unread_invalid() {
    let f = fx(&[("src/x", b"x")]);
    let s = store(&f);
    assert_eq!(repo_def::check_default(&f.c, ID, &s, &Cnt::default()), Use::Absent { full_ref: MAIN.into() });
    let fail = Cnt { fail: true, ..Cnt::default() };
    assert_eq!(repo_def::check_default(&f.c, ID, &s, &fail), Use::Unread { why: "git symbolic-ref: tool not found".into() });
    let f = fx(&[(".zygo", b"x")]);
    assert_eq!(repo_def::check_default(&f.c, ID, &s, &Cnt::default()),
               Use::Invalid { full_ref: Some(MAIN.into()), commit: head(&f.c), why: ".zygo is not a directory".into() });
    let f = fx(&[(".zygo/workspace.toml", b"version = 2\n")]);
    let why = repo_def::parse(&ws("version = 2\n")).unwrap_err();
    assert_eq!(repo_def::check_default(&f.c, ID, &store(&f), &Cnt::default()),
               Use::Invalid { full_ref: Some(MAIN.into()), commit: head(&f.c), why });
}

#[test] // R§18 item 6; R§7.3: one read, one digest, one lookup; the def applied is the read checked
fn one_read_per_use_and_never_a_mix() {
    let f = fx(&[(".zygo/workspace.toml", A.as_bytes())]);
    let s = store(&f);
    repo_def::approve_workspace_in(&s, ID, &d(A)).unwrap();
    let r = Cnt { swap: Some((2, B.as_bytes())), ..Cnt::default() };
    let Use::Approved { full_ref, pin, files, def } = repo_def::check_default(&f.c, ID, &s, &r) else { panic!() };
    assert_eq!((r.reads(), r.blobs()), (1, 1));
    assert_eq!((full_ref.as_str(), pin.clone()), (MAIN, Pin { commit: head(&f.c), digest: d(A) }));
    assert_eq!(files.workspace.as_deref(), Some(A.as_bytes()));
    assert_eq!(def, parsed(A));
    let Use::NotApproved { digest, def, files, .. } = repo_def::check_default(&f.c, ID, &s, &r) else { panic!() };
    assert_eq!((r.reads(), r.blobs()), (2, 2));
    assert_eq!((digest, def, files.workspace.as_deref()), (d(B), parsed(B), Some(B.as_bytes())));
    // The inverse: only B is approved. The first use (A) is NotApproved, the second (B) is Approved with B's def.
    let s2 = f._t.path().join("approvals-b.toml");
    repo_def::approve_workspace_in(&s2, ID, &d(B)).unwrap();
    let r = Cnt { swap: Some((2, B.as_bytes())), ..Cnt::default() };
    assert!(matches!(repo_def::check_default(&f.c, ID, &s2, &r), Use::NotApproved { .. }));
    let Use::Approved { pin, def, .. } = repo_def::check_default(&f.c, ID, &s2, &r) else { panic!() };
    assert_eq!((pin.digest, def), (d(B), parsed(B)));
    assert_eq!(r.reads(), 2);
}

#[test] // R§18 item 8; R§6.1 step 1: the key is the workspace id
fn same_files_in_another_repository_are_not_approved() {
    let (f1, f2) = (fx(&[(".zygo/workspace.toml", A.as_bytes())]), fx(&[(".zygo/workspace.toml", A.as_bytes())]));
    let s = store(&f1);
    repo_def::approve_workspace_in(&s, ID, &d(A)).unwrap();
    assert!(matches!(repo_def::check_default(&f1.c, ID, &s, &Cnt::default()), Use::Approved { .. }));
    let Use::NotApproved { digest, .. } = repo_def::check_default(&f2.c, ID2, &s, &Cnt::default()) else { panic!() };
    assert_eq!(digest, d(A));
}

#[test] // R§7.4: the pinned read, not the default branch
fn pinned_approved_reads_the_pin() {
    let f = fx(&[(".zygo/workspace.toml", A.as_bytes())]);
    let s = store(&f);
    repo_def::approve_workspace_in(&s, ID, &d(A)).unwrap();
    let Use::Approved { pin, .. } = repo_def::check_default(&f.c, ID, &s, &Cnt::default()) else { panic!() };
    push_change(&f, &[(".zygo/workspace.toml", B.as_bytes())]);
    repo_def::approve_workspace_in(&s, ID, &d(B)).unwrap();
    let r = Cnt::default();
    assert_eq!(repo_def::check_pinned(&f.c, ID, &pin, &s, &r), PinnedUse::Approved { def: parsed(A) });
    assert_eq!((r.reads(), r.roots(), r.blobs()), (0, 1, 1));
    assert!(r.calls.borrow().iter().all(|c| c.iter().all(|a| !a.starts_with("refs/"))));
    // R§7.2 ruling 4: the user removes the entry by hand.
    std::fs::write(&s, format!("[[workspace]]\nid = \"{ID}\"\ndigest = \"{}\"\n", d(B))).unwrap();
    assert_eq!(repo_def::check_pinned(&f.c, ID, &pin, &s, &Cnt::default()), PinnedUse::NotApproved);
}

#[test] // R§7.4: gone, unread, and never Approved for a digest other than pin.digest
fn pinned_gone_unread_and_other_digest() {
    let f = fx(&[(".zygo/workspace.toml", A.as_bytes())]);
    let s = store(&f);
    repo_def::approve_workspace_in(&s, ID, &d(A)).unwrap();
    repo_def::approve_workspace_in(&s, ID, &d(B)).unwrap();
    let c = head(&f.c);
    // A commit that the clone does not have, as after a force-push and gc.
    commit_files(&f.w, &[(".zygo/workspace.toml", B.as_bytes())]);
    let gone = git(&f.w, &["rev-parse", "HEAD"]);
    assert_eq!(repo_def::check_pinned(&f.c, ID, &Pin { commit: gone, digest: d(B) }, &s, &Cnt::default()), PinnedUse::Gone);
    let fail = Cnt { fail: true, ..Cnt::default() };
    assert_eq!(repo_def::check_pinned(&f.c, ID, &Pin { commit: c.clone(), digest: d(A) }, &s, &fail),
               PinnedUse::Unread { why: "git ls-tree: tool not found".into() });
    // Both digests are stored; the read at c gives d(A), so a pin of d(B) at c is not approved.
    assert_eq!(repo_def::check_pinned(&f.c, ID, &Pin { commit: c.clone(), digest: d(B) }, &s, &Cnt::default()), PinnedUse::NotApproved);
    // The seed commit has no .zygo/ (Read::Absent): never Approved.
    let seed = git(&f.c, &["rev-parse", &format!("{MAIN}~1")]);
    assert_eq!(repo_def::check_pinned(&f.c, ID, &Pin { commit: seed, digest: d(A) }, &s, &Cnt::default()), PinnedUse::NotApproved);
    // The other id is not approved.
    assert_eq!(repo_def::check_pinned(&f.c, ID2, &Pin { commit: c, digest: d(A) }, &s, &Cnt::default()), PinnedUse::NotApproved);
}

#[test] // R§18 item 6; R§7.4: one read per pinned use; the def applied is the read checked
fn pinned_one_read_per_use_and_never_a_mix() {
    let f = fx(&[(".zygo/workspace.toml", A.as_bytes())]);
    let s = store(&f);
    repo_def::approve_workspace_in(&s, ID, &d(A)).unwrap();
    repo_def::approve_workspace_in(&s, ID, &d(B)).unwrap();
    let pin = Pin { commit: head(&f.c), digest: d(A) };
    let r = Cnt { swap: Some((2, B.as_bytes())), ..Cnt::default() };
    assert_eq!(repo_def::check_pinned(&f.c, ID, &pin, &s, &r), PinnedUse::Approved { def: parsed(A) });
    assert_eq!((r.roots(), r.blobs()), (1, 1));
    // The second read gives B (same length); its digest is not pin.digest, so B's def is never applied.
    assert_eq!(repo_def::check_pinned(&f.c, ID, &pin, &s, &r), PinnedUse::NotApproved);
    assert_eq!((r.roots(), r.blobs()), (2, 2));
}

#[test] // R§7.4: an invalid .zygo/ at the pinned commit is never Approved
fn pinned_invalid_is_not_approved() {
    let f = fx(&[(".zygo", b"x")]);
    let s = store(&f);
    repo_def::approve_workspace_in(&s, ID, &d(A)).unwrap();
    let pin = Pin { commit: head(&f.c), digest: d(A) };
    assert_eq!(repo_def::check_pinned(&f.c, ID, &pin, &s, &Cnt::default()), PinnedUse::NotApproved);
}

#[test] // R§7.4: a pinned commit that is not an object id is Unread, with no git call
fn pinned_commit_not_an_oid_is_unread() {
    let f = fx(&[(".zygo/workspace.toml", A.as_bytes())]);
    let s = store(&f);
    repo_def::approve_workspace_in(&s, ID, &d(A)).unwrap();
    let r = Cnt::default();
    let pin = Pin { commit: "not-a-sha".into(), digest: d(A) };
    assert!(matches!(repo_def::check_pinned(&f.c, ID, &pin, &s, &r), PinnedUse::Unread { .. }));
    assert!(r.calls.borrow().is_empty());
}

#[test] // R§7.2: a missing or invalid approvals file approves nothing
fn default_missing_or_invalid_store_is_not_approved() {
    let f = fx(&[(".zygo/workspace.toml", A.as_bytes())]);
    let s = store(&f);
    let Use::NotApproved { full_ref, files, digest, def } = repo_def::check_default(&f.c, ID, &s, &Cnt::default()) else { panic!() };
    assert_eq!((full_ref.as_str(), files.commit, digest, def), (MAIN, head(&f.c), d(A), parsed(A)));
    std::fs::write(&s, "[[workspace\nid = ").unwrap();
    let Use::NotApproved { digest, .. } = repo_def::check_default(&f.c, ID, &s, &Cnt::default()) else { panic!() };
    assert_eq!(digest, d(A));
}
