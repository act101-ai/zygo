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

const MSG_PATH: &str = "bob@b/01ARZ3NDEKTSV4RRFFQ69G5FAV.json";

/// The bootstrap root `rt` and `c1` = `rt` plus a message and a registry file, with `* text eol=crlf` in the
/// repository's attributes.
fn two_commits(m: &Machine) -> (String, String) {
    std::fs::create_dir_all(m.root.join(".git/info")).unwrap();
    std::fs::write(m.root.join(".git/info/attributes"), "* text eol=crlf\n").unwrap();
    let rt = bootstrap(&m.root, &m.wt, far()).unwrap();
    let c1 = commit(&m.wt, &rt, Some(&rt), &[
        Change::Add { path: MSG_PATH.into(), bytes: b"{\"a\":1}\n".to_vec() },
        Change::Add { path: "registry/a.json".into(), bytes: b"r\r\n".to_vec() },
    ], far()).unwrap();
    (rt, c1)
}

/// The recursive file paths of `c`.
fn files(cwd: &std::path::Path, c: &str) -> Vec<String> {
    git(cwd, &["ls-tree", "-r", "--name-only", c]).lines().map(String::from).collect()
}

#[test]
fn commit_applies_adds_and_removes_on_base() {
    let r = Remote::new();
    let m = Machine::new(&r);
    let (rt, c1) = two_commits(&m);
    assert_eq!(files(&m.root, &c1), ["ZYGO-MAIL", MSG_PATH, "registry/a.json"]);
    for (p, want) in [(MSG_PATH, &b"{\"a\":1}\n"[..]), ("registry/a.json", &b"r\r\n"[..])] {
        let id = git(&m.root, &["rev-parse", &format!("{c1}:{p}")]);
        assert_eq!(cat(&m.wt, &id, 100, far()), Ok(want.to_vec()), "{p}");
    }
    assert_eq!(git(&m.root, &["rev-list", "--parents", "-n1", &c1]), format!("{c1} {rt}"));
    assert_eq!(git(&m.wt, &["rev-parse", "HEAD"]), c1);
    let c2 = commit(&m.wt, &c1, Some(&c1), &[Change::Remove { path: "registry/a.json".into() }], far()).unwrap();
    assert_eq!(files(&m.root, &c2), ["ZYGO-MAIL", MSG_PATH]);
    assert_eq!(git(&m.wt, &["rev-parse", "HEAD"]), c2);
}

#[test]
fn commit_without_parent_is_a_root() {
    let r = Remote::new();
    let m = Machine::new(&r);
    let (_, c1) = two_commits(&m);
    let c3 = commit(&m.wt, &c1, None, &[], far()).unwrap();
    assert_eq!(git(&m.root, &["rev-list", "--parents", "-n1", &c3]), c3);
    assert_eq!(git(&m.root, &["rev-parse", &format!("{c3}^{{tree}}")]),
        git(&m.root, &["rev-parse", &format!("{c1}^{{tree}}")]));
}

#[test]
fn commit_author_and_message_are_fixed() {
    let r = Remote::new();
    let m = Machine::new(&r);
    let (rt, c1) = two_commits(&m);
    let c2 = commit(&m.wt, &c1, Some(&c1), &[Change::Remove { path: "registry/a.json".into() }], far()).unwrap();
    let c3 = commit(&m.wt, &c1, None, &[], far()).unwrap();
    for c in [rt, c1, c2, c3] {
        assert_eq!(who_and_message(&m.root, &c), FIXED, "{c}");
    }
}

#[test]
fn commit_neither_signs_nor_prompts() {
    // Control: the global `commit.gpgSign = true` of the test HOME is live for a plain `git commit`.
    let ctl = tempfile::TempDir::new().unwrap();
    git(ctl.path(), &["init", "-q"]);
    let n0 = gpg_calls();
    let o = plain(ctl.path()).args(["-c", "user.name=t", "-c", "user.email=t@t.invalid", "commit", "--allow-empty",
        "-m", "x"]).stdin(std::process::Stdio::null()).output().unwrap();
    assert!(!o.status.success());
    assert_eq!(gpg_calls(), n0 + 1);
    let r = Remote::new();
    let m = Machine::new(&r);
    let n = gpg_calls();
    let rt = bootstrap(&m.root, &m.wt, far()).unwrap();
    let c = commit(&m.wt, &rt, Some(&rt), &[Change::Add { path: "registry/a.json".into(), bytes: b"r\n".to_vec() }],
        far()).unwrap();
    assert_eq!(gpg_calls(), n);
    for id in [rt, c] {
        let raw = git(&m.root, &["cat-file", "commit", &id]);
        assert!(!raw.lines().any(|l| l.starts_with("gpgsig")), "{raw}");
    }
}

/// A porcelain push output with the one ref line `<flag>\tC:refs/heads/<to>\t<summary>`.
fn porcelain(flag: &str, to: &str, summary: &str) -> Vec<u8> {
    format!("To file:///x\n{flag}\tC:refs/heads/{to}\t{summary}\nDone\n").into_bytes()
}

#[test]
fn classify_moved_fetch_first_any_case() {
    assert_eq!(classify(false, &porcelain("!", "zygo/mail", "[REJECTED] (Fetch First)"), "zygo/mail"), Pushed::Moved);
}

#[test]
fn classify_moved_non_fast_forward_any_case() {
    assert_eq!(classify(false, &porcelain("!", "zygo/mail", "[rejected] (NON-FAST-FORWARD)"), "zygo/mail"), Pushed::Moved);
}

#[test]
fn classify_moved_stale_info_any_case() {
    assert_eq!(classify(false, &porcelain("!", "zygo/mail", "[Rejected] (STALE INFO)"), "zygo/mail"), Pushed::Moved);
}

#[test]
fn classify_reads_only_the_target_ref_line() {
    let failed = Pushed::Other("push failed".into());
    let rejected = Pushed::Other("push rejected".into());
    for flag in ["*", " ", "+", "="] {
        assert_eq!(classify(true, format!("{flag}\tC:refs/heads/zygo/mail\t[new branch]\nDone\n").as_bytes(), "zygo/mail"),
            Pushed::Accepted, "flag {flag:?}");
    }
    assert_eq!(classify(false, b"!\tC:refs/heads/zygo/other\t[rejected] (fetch first)\n", "zygo/mail"), failed);
    assert_eq!(classify(false, b"!\tC:refs/heads/zygo/mail-2\t[rejected] (fetch first)\n", "zygo/mail"), failed);
    assert_eq!(classify(false,
        b"remote: [rejected] (fetch first)\n!\tC:refs/heads/zygo/mail\t[remote rejected] (pre-receive hook declined)\n",
        "zygo/mail"), rejected);
    assert_eq!(classify(false, b"!\tC:refs/heads/zygo/mail\t[remote rejected] (fetch first)\n", "zygo/mail"), rejected);
    assert_eq!(classify(false, b"", "zygo/mail"), failed);
    assert_eq!(classify(false, b"*\tC:refs/heads/zygo/mail\t[new branch]\n", "zygo/mail"), failed);
}

/// The bootstrap root of `m` plus a commit that adds `registry/<who>.json`: distinct per machine even when two
/// roots are made in the same second.
fn first(m: &Machine, who: &str) -> String {
    let rt = bootstrap(&m.root, &m.wt, far()).unwrap();
    commit(&m.wt, &rt, Some(&rt), &[Change::Add { path: format!("registry/{who}.json"), bytes: b"{}\n".to_vec() }],
        far()).unwrap()
}

#[test]
fn push_of_a_new_branch_is_accepted_and_set_tracking_moves_the_ref() {
    let r = Remote::new();
    let m = Machine::new(&r);
    let c = first(&m, "a");
    assert_eq!(push(&m.wt, "origin", "zygo/mail", &c, None, far()), Ok(Pushed::Accepted));
    assert_eq!(r.tip("zygo/mail"), Some(c.clone()));
    assert_eq!(set_tracking(&m.wt, "origin", "zygo/mail", &c, far()), Ok(()));
    assert_eq!(git(&m.root, &["rev-parse", "refs/remotes/origin/zygo/mail"]), c);
}

#[test]
fn push_after_the_remote_moved_is_moved() {
    let r = Remote::new();
    let a = Machine::new(&r);
    let b = Machine::new(&r);
    let ca = first(&a, "a");
    let cb = first(&b, "b");
    assert_eq!(push(&b.wt, "origin", "zygo/mail", &cb, None, far()), Ok(Pushed::Accepted));
    assert_eq!(push(&a.wt, "origin", "zygo/mail", &ca, None, far()), Ok(Pushed::Moved));
    assert_eq!(r.tip("zygo/mail"), Some(cb.clone()));
    assert_eq!(fetch(&a.wt, "origin", "zygo/mail", far()), Ok(Fetched::Tip(cb.clone())));
    assert_eq!(push(&a.wt, "origin", "zygo/mail", &ca, None, far()), Ok(Pushed::Moved));
    assert_eq!(r.tip("zygo/mail"), Some(cb));
}

/// `a` pushes `c1`; `b` fetches it, commits `c2` on it and pushes. Returns `(c1, c2)`.
fn moved_under_a(a: &Machine, b: &Machine) -> (String, String) {
    let c1 = first(a, "a");
    assert_eq!(push(&a.wt, "origin", "zygo/mail", &c1, None, far()), Ok(Pushed::Accepted));
    assert_eq!(fetch(&a.wt, "origin", "zygo/mail", far()), Ok(Fetched::Tip(c1.clone())));
    assert_eq!(fetch(&b.root, "origin", "zygo/mail", far()), Ok(Fetched::Tip(c1.clone())));
    assert_eq!(ensure_worktree(&b.root, &b.wt, &c1, far()), Ok(()));
    let c2 = commit(&b.wt, &c1, Some(&c1), &[Change::Add { path: "registry/b.json".into(), bytes: b"{}\n".to_vec() }],
        far()).unwrap();
    assert_eq!(push(&b.wt, "origin", "zygo/mail", &c2, None, far()), Ok(Pushed::Accepted));
    (c1, c2)
}

#[test]
fn lease_push_after_the_remote_moved_is_moved_and_leaves_the_ref() {
    let r = Remote::new();
    let a = Machine::new(&r);
    let b = Machine::new(&r);
    let (c1, c2) = moved_under_a(&a, &b);
    let o = commit(&a.wt, &c1, None, &[], far()).unwrap();
    assert_eq!(push(&a.wt, "origin", "zygo/mail", &o, Some(&c1), far()), Ok(Pushed::Moved));
    assert_eq!(r.tip("zygo/mail"), Some(c2));
}

#[test]
fn lease_push_on_the_current_base_is_accepted() {
    let r = Remote::new();
    let a = Machine::new(&r);
    let b = Machine::new(&r);
    let (_, c2) = moved_under_a(&a, &b);
    assert_eq!(fetch(&a.wt, "origin", "zygo/mail", far()), Ok(Fetched::Tip(c2.clone())));
    let o2 = commit(&a.wt, &c2, None, &[], far()).unwrap();
    assert_eq!(push(&a.wt, "origin", "zygo/mail", &o2, Some(&c2), far()), Ok(Pushed::Accepted));
    assert_eq!(r.tip("zygo/mail"), Some(o2));
}

#[test]
fn push_declined_by_a_hook_is_other_even_with_moved_words() {
    use std::os::unix::fs::PermissionsExt;
    let r = Remote::new();
    let m = Machine::new(&r);
    let hook = r.bare.join("hooks/pre-receive");
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    std::fs::write(&hook, "#!/bin/sh\necho '[rejected] (fetch first)'\necho 'non-fast-forward'\nexit 1\n").unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    let c = first(&m, "a");
    assert_eq!(push(&m.wt, "origin", "zygo/mail", &c, None, far()), Ok(Pushed::Other("push rejected".into())));
    assert_eq!(r.tip("zygo/mail"), None);
}

#[test]
fn push_never_reads_the_remote_as_an_option() {
    let r = Remote::new();
    let m = Machine::new(&r);
    let spy = spy(r.dir.path(), "git-receive-pack");
    let c = first(&m, "a");
    assert_eq!(push(&m.wt, &format!("--receive-pack={spy}"), "zygo/mail", &c, None, far()),
        Ok(Pushed::Other("push failed".into())));
    assert!(!r.dir.path().join("spy-ran").exists());
}

#[test]
fn push_to_a_missing_remote_is_other() {
    let r = Remote::new();
    let m = Machine::new(&r);
    let c = first(&m, "a");
    assert_eq!(push(&m.wt, "nosuch", "zygo/mail", &c, None, far()), Ok(Pushed::Other("push failed".into())));
}
