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
