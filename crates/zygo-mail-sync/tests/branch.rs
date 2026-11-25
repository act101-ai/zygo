//! MS-10: the side branch plumbing (mail sync design S§6.1, S§6.3-S§6.6, S-C3, S-C4, S-G2, S§18; SP3, SP4) on
//! local bare repositories. No test depends on wall-clock time.
#![cfg(unix)]

mod common;

use common::*;
use std::time::Instant;
use zygo_mail_sync::branch::*;
use zygo_work::repo::{Flow, RunError};

#[test]
fn constants_have_the_spec_values() {
    assert_eq!(MARKER, b"1\n");
    assert_eq!(COMMIT_MESSAGE, "zygo mail [skip ci]");
    assert_eq!(MOVED_TEXT, &["fetch first", "non-fast-forward", "stale info"]);
}

/// The answer to an object-id argument that is not 40 or 64 lower-case hex.
fn refused<T>() -> Result<T, BranchError> { Err(BranchError::Refused("object id")) }

#[test]
fn object_id_arguments_refuse_option_like_values() {
    let r = Remote::new();
    let m = Machine::new(&r);
    let c = git(&m.root, &["rev-parse", "HEAD"]);
    let bad = "--output=/tmp/x";
    assert_eq!(ls(&m.wt, bad, None, far(), &mut |_| Flow::Go), refused());
    assert_eq!(cat(&m.root, bad, 10, far()), refused());
    assert_eq!(has_marker(&m.root, bad, far()), refused());
    assert_eq!(commit(&m.root, bad, None, &[], far()), refused());
    assert_eq!(push(&m.root, "origin", "zygo/mail", bad, None, far()), refused());
    assert_eq!(push(&m.root, "origin", "zygo/mail", &c, Some(bad), far()), refused());
    assert_eq!(set_tracking(&m.root, "origin", "zygo/mail", bad, far()), refused());
    assert_eq!(ensure_worktree(&m.root, &m.wt, bad, far()), refused());
    assert!(!m.wt.exists());
}

#[test]
fn past_deadline_times_out_before_spawn() {
    let r = Remote::new();
    let m = Machine::new(&r);
    assert_eq!(fetch(&m.root, "origin", "zygo/mail", Instant::now()), Err(BranchError::Git(RunError::TimedOut, "fetch")));
}

/// A script under `dir` that touches `<dir>/spy-ran` and then runs `prog` with its arguments.
fn spy(dir: &std::path::Path, prog: &str) -> String {
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join("spy");
    std::fs::write(&p, format!("#!/bin/sh\ntouch '{}'\nexec {prog} \"$@\"\n", dir.join("spy-ran").display())).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p.display().to_string()
}

/// A root commit with the one file `name` = `bytes`, pushed by plain git from `m` to `refs/heads/zygo/mail`
/// (forced).
fn push_plain_root(m: &Machine, name: &[u8], bytes: &[u8]) -> String {
    let b = blob(&m.root, bytes);
    let t = tree_literally(&m.root, &[("100644", name, &b)]);
    let c = commit_tree(&m.root, &t, None);
    git(&m.root, &["push", "-q", "-f", "origin", &format!("{c}:refs/heads/zygo/mail")]);
    c
}

#[test]
fn fetch_of_a_missing_branch_is_no_branch_and_writes_no_fetch_head() {
    let r = Remote::new();
    let m = Machine::new(&r);
    assert_eq!(fetch(&m.root, "origin", "zygo/mail", far()), Ok(Fetched::NoBranch));
    assert!(!m.root.join(".git/FETCH_HEAD").exists());
}

#[test]
fn fetch_of_a_branch_gives_its_tip_and_the_tracking_ref() {
    let r = Remote::new();
    let a = Machine::new(&r);
    let b = Machine::new(&r);
    let c1 = push_plain_root(&b, b"ZYGO-MAIL", b"1\n");
    assert_eq!(fetch(&a.root, "origin", "zygo/mail", far()), Ok(Fetched::Tip(c1.clone())));
    assert_eq!(git(&a.root, &["rev-parse", "refs/remotes/origin/zygo/mail"]), c1);
    let c2 = push_plain_root(&b, b"other", b"2\n");
    assert_eq!(fetch(&a.root, "origin", "zygo/mail", far()), Ok(Fetched::Tip(c2)));
    assert!(!a.root.join(".git/FETCH_HEAD").exists());
}

#[test]
fn fetch_never_reads_the_remote_as_an_option() {
    let r = Remote::new();
    let m = Machine::new(&r);
    let spy = spy(r.dir.path(), "git-upload-pack");
    assert_eq!(fetch(&m.root, &format!("--upload-pack={spy}"), "zygo/mail", far()),
        Err(BranchError::Git(RunError::Failed, "fetch")));
    assert!(!r.dir.path().join("spy-ran").exists());
}

/// The entries that `ls` passes for `tip` under `prefix`.
fn entries(wt: &std::path::Path, tip: &str, prefix: Option<&str>) -> Vec<Entry> {
    let mut v = Vec::new();
    let end = ls(wt, tip, prefix, far(), &mut |e| {
        v.push(e);
        Flow::Go
    });
    assert!(matches!(end, Ok(zygo_work::repo::Streamed::Done { ok: true, .. })), "{end:?}");
    v
}

/// The author, committer and message of commit `c`, as `%an <%ae>|%cn <%ce>|%B`.
fn who_and_message(cwd: &std::path::Path, c: &str) -> String {
    git(cwd, &["log", "-1", "--format=%an <%ae>|%cn <%ce>|%B", c])
}

const FIXED: &str = "zygo <zygo@zygo.invalid>|zygo <zygo@zygo.invalid>|zygo mail [skip ci]\n";

#[test]
fn bootstrap_makes_one_marker_root_and_the_worktree() {
    let r = Remote::new();
    let m = Machine::new(&r);
    let rt = bootstrap(&m.root, &m.wt, far()).unwrap();
    let b = blob(&m.root, b"1\n");
    assert_eq!(entries(&m.wt, &rt, None), vec![Entry {
        mode: "100644".into(), kind: "blob".into(), id: b.clone(), size: Some(2), path: "ZYGO-MAIL".into(),
    }]);
    assert_eq!(cat(&m.wt, &b, 2, far()), Ok(b"1\n".to_vec()));
    assert_eq!(git(&m.root, &["rev-list", "--parents", "-n1", &rt]), rt);
    let mktree = git_in(&m.root, &["mktree"], format!("100644 blob {b}\tZYGO-MAIL\n").as_bytes());
    assert_eq!(git(&m.root, &["rev-parse", &format!("{rt}^{{tree}}")]), mktree);
    assert_eq!(who_and_message(&m.root, &rt), FIXED);
    assert_eq!(git(&m.wt, &["rev-parse", "HEAD"]), rt);
    let list = git(&m.root, &["worktree", "list", "--porcelain"]);
    assert!(list.contains(&format!("worktree {}\nHEAD {rt}\ndetached", m.wt.display())), "{list}");
    // An existing worktree gets `update-ref`. Two bootstraps in one second make the same commit, so the worktree
    // is first moved away from it, with no timing assumption.
    let c = git(&m.root, &["rev-parse", "HEAD"]);
    assert_eq!(ensure_worktree(&m.root, &m.wt, &c, far()), Ok(()));
    assert_eq!(git(&m.wt, &["rev-parse", "HEAD"]), c);
    let rt2 = bootstrap(&m.root, &m.wt, far()).unwrap();
    assert_eq!(git(&m.wt, &["rev-parse", "HEAD"]), rt2);
    assert_eq!(names(&m.wt), [".git"]);
}

#[test]
fn ensure_worktree_makes_a_deleted_folder_again() {
    let r = Remote::new();
    let m = Machine::new(&r);
    let c = git(&m.root, &["rev-parse", "HEAD"]);
    assert_eq!(ensure_worktree(&m.root, &m.wt, &c, far()), Ok(()));
    std::fs::remove_dir_all(&m.wt).unwrap();
    assert_eq!(ensure_worktree(&m.root, &m.wt, &c, far()), Ok(()));
    assert!(std::fs::symlink_metadata(m.wt.join(".git")).unwrap().is_file());
    assert_eq!(git(&m.wt, &["rev-parse", "HEAD"]), c);
    assert_eq!(names(&m.wt), [".git"]);
}

#[test]
fn ls_passes_hostile_names_as_data() {
    let r = Remote::new();
    let m = Machine::new(&r);
    let root = &m.root;
    let b = blob(root, b"1\n");
    let long = [b'x'; 300];
    let inner = tree_literally(root, &[("100644", b"..", &b), ("100644", b"a\tb", &b), ("100644", b"a\nb", &b),
        ("100644", &long, &b), ("100644", b"\xff\xfe", &b)]);
    let top = tree_literally(root, &[("100644", b"ZYGO-MAIL", &b), ("40000", b"bob@b", &inner)]);
    let c = commit_tree(root, &top, None);
    assert_eq!(ensure_worktree(root, &m.wt, &c, far()), Ok(()));
    let paths: Vec<String> = entries(&m.wt, &c, Some("bob@b/")).into_iter().map(|e| e.path).collect();
    assert_eq!(paths, ["bob@b/..".to_string(), "bob@b/a\tb".into(), "bob@b/a\nb".into(),
        format!("bob@b/{}", "x".repeat(300))]);
    assert_eq!(names(&m.wt), [".git"]);
}

#[test]
fn ls_lists_the_top_level_and_stops_on_request() {
    let r = Remote::new();
    let m = Machine::new(&r);
    let rt = bootstrap(&m.root, &m.wt, far()).unwrap();
    let mark = blob(&m.root, b"1\n");
    let msg = blob(&m.root, b"{}\n");
    let dir = tree_literally(&m.root, &[("100644", b"01ARZ3NDEKTSV4RRFFQ69G5FAV.json", &msg)]);
    let top = tree_literally(&m.root, &[("100644", b"ZYGO-MAIL", &mark), ("40000", b"bob@b", &dir)]);
    let c = commit_tree(&m.root, &top, Some(&rt));
    let all = entries(&m.wt, &c, None);
    assert_eq!(all.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(), ["ZYGO-MAIL", "bob@b"]);
    assert_eq!((all[1].kind.as_str(), all[1].size), ("tree", None));
    let mut calls = 0;
    let end = ls(&m.wt, &c, None, far(), &mut |_| {
        calls += 1;
        Flow::Stop
    });
    assert_eq!(end, Ok(zygo_work::repo::Streamed::Stopped));
    assert_eq!(calls, 1);
    let missing = "0123456789abcdef0123456789abcdef01234567";
    assert_eq!(ls(&m.wt, missing, None, far(), &mut |_| Flow::Go), Err(BranchError::Git(RunError::Failed, "ls-tree")));
}

#[test]
fn cat_returns_bytes_up_to_max() {
    let r = Remote::new();
    let m = Machine::new(&r);
    let b = blob(&m.root, b"0123456789");
    assert_eq!(cat(&m.root, &b, 10, far()), Ok(b"0123456789".to_vec()));
    assert_eq!(cat(&m.root, &b, 9, far()), Err(BranchError::Refused("too large")));
}

#[test]
fn has_marker_needs_the_marker_blob() {
    let r = Remote::new();
    let m = Machine::new(&r);
    let rt = bootstrap(&m.root, &m.wt, far()).unwrap();
    assert_eq!(has_marker(&m.wt, &rt, far()), Ok(true));
    let root = &m.root;
    let one = blob(root, b"1\n");
    let two = blob(root, b"2\n");
    let extra = blob(root, b"1\n\n");
    let sub = tree_literally(root, &[("100644", b"x", &one)]);
    let trees = [
        tree_literally(root, &[("100644", b"other", &one)]),
        tree_literally(root, &[("100644", b"ZYGO-MAIL", &two)]),
        tree_literally(root, &[("100644", b"ZYGO-MAIL", &extra)]),
        tree_literally(root, &[("120000", b"ZYGO-MAIL", &one)]),
        tree_literally(root, &[("40000", b"ZYGO-MAIL", &sub)]),
    ];
    for t in trees {
        let c = commit_tree(root, &t, None);
        assert_eq!(has_marker(&m.wt, &c, far()), Ok(false), "tree {t}");
    }
}
