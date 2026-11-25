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
