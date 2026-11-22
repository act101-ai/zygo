use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};
use tempfile::TempDir;
use zygo_work::repo::{checkout, Checkout};
use zygo_workspace::identity::*;

#[test] // W§5: first 16 hex characters of SHA-256 of the path bytes
fn workspace_id_is_16_hex_of_sha256() {
    assert_eq!(workspace_id(Path::new("/srv/repo/.git")), "4b45de8d0544bbd0");
    assert_ne!(workspace_id(Path::new("/srv/repo2/.git")), "4b45de8d0544bbd0");
}

#[test] // AU2 slug rule, W§18 #2
fn slug_follows_au2() {
    assert_eq!(slug("My Repo!!"), "my-repo");
    assert_eq!(slug("---"), "ws");
    assert_eq!(slug(""), "ws");
    assert_eq!(slug("a__b..c"), "a-b-c");
    assert_eq!(slug("\u{212A}"), "ws"); // KELVIN SIGN: only ASCII letters are lower-cased
    assert_eq!(slug(&"a".repeat(50)), "a".repeat(32));
    // the cut at 32 ends on `-`, which is trimmed again
    assert_eq!(slug(&format!("{}-{}", "a".repeat(31), "b".repeat(18))), "a".repeat(31));
}

#[test] // AU2 collision suffix, W§18 #2
fn unique_name_adds_a_suffix_within_32() {
    assert_eq!(unique_name("app", &|_| false), "app");
    assert_eq!(unique_name("app", &|n| n == "app"), "app-2");
    assert_eq!(unique_name("app", &|n| n == "app" || n == "app-2"), "app-3");
    let long = "a".repeat(32);
    assert_eq!(unique_name(&long, &|n| n == long), format!("{}-2", "a".repeat(30)));
    assert_eq!(unique_name(&long, &|n| !n.ends_with("-10")), format!("{}-10", "a".repeat(29)));
    // the cut slug part ends on `-`: trimmed before the suffix
    let dash = format!("{}-bb", "a".repeat(29));
    assert_eq!(unique_name(&dash, &|n| n == dash), format!("{}-2", "a".repeat(29)));
}

#[test] // AU84, with the hostile values of the spec
fn validators_follow_au84() {
    assert!(valid_pane_key("kabcd2345"));
    for bad in ["", "k$(x)", "#{pane_id}", "kabcd234", "kabcd23456", "xabcd2345", "kABCD2345", "kabcd0189", "kabcd2345\n", " kabcd2345"] {
        assert!(!valid_pane_key(bad), "{bad:?}");
    }
    assert!(valid_ws_id("0123456789abcdef"));
    for bad in ["", "abc", "0123456789abcde", "0123456789abcdef0", "0123456789ABCDEF", "0123456789abcdeg", "0123456789abcdef\n"] {
        assert!(!valid_ws_id(bad), "{bad:?}");
    }
    let (a32, a33) = ("a".repeat(32), "a".repeat(33));
    for good in ["a", "my-repo", "app-2", "-", a32.as_str()] {
        assert!(valid_name(good), "{good:?}");
    }
    for bad in ["", "a b", "A", "a_b", "a.b", "a/b", "a\n", "\u{e9}", a33.as_str()] {
        assert!(!valid_name(bad), "{bad:?}");
    }
    assert!(valid_tmux_session("zygo-my-repo", "my-repo"));
    assert!(valid_tmux_session(&format!("zygo-{a32}"), &a32));
    let s33 = format!("zygo-{a33}");
    for (s, name) in [("other", "my-repo"), ("zygo-other", "my-repo"), ("my-repo", "my-repo"), ("zygo-", ""), ("zygo-a b", "a b"),
        ("zygo-my-repo\n", "my-repo"), ("ZYGO-my-repo", "my-repo"), (s33.as_str(), a33.as_str())] {
        assert!(!valid_tmux_session(s, name), "{s:?} {name:?}");
    }
}

#[test] // W§5: `k` plus 8 random characters of lower-case base32
fn new_pane_key_is_valid_and_random() {
    let keys: std::collections::BTreeSet<String> = (0..64).map(|_| new_pane_key()).collect();
    assert!(keys.iter().all(|k| valid_pane_key(k)));
    assert_eq!(keys.len(), 64);
}

const GIT: Duration = Duration::from_secs(10);

fn git(dir: &Path, args: &[&str]) {
    let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
    let out = Command::new("git").current_dir(dir)
        .args(["-c", "commit.gpgsign=false", "-c", "user.name=t", "-c", "user.email=t@example.invalid"]).args(args)
        .env("GIT_CONFIG_GLOBAL", null).env("GIT_CONFIG_NOSYSTEM", "1")
        .output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}
fn canon(p: &Path) -> PathBuf { p.canonicalize().unwrap() }
/// `<parent>/<name>`: a repository on `main` with one commit; `parent` is made first. The path is not canonical
/// (a canonical Windows path has the `\\?\` prefix; the `git` helper gets plain paths, as in `hud_git.rs`).
fn repo(parent: &Path, name: &str) -> PathBuf {
    std::fs::create_dir_all(parent).unwrap();
    git(parent, &["init", "-q", "-b", "main", name]);
    git(&parent.join(name), &["commit", "-q", "--allow-empty", "-m", "a"]);
    parent.join(name)
}
fn co(dir: &Path) -> Checkout { checkout(dir, Instant::now() + GIT).unwrap() }
/// Beside `w`: worktree `a` on branch `feat` and worktree `b` detached.
fn two_linked(w: &Path) -> (PathBuf, PathBuf) {
    git(w, &["worktree", "add", "-q", "-b", "feat", "../a"]);
    git(w, &["worktree", "add", "-q", "--detach", "../b"]);
    let parent = w.parent().unwrap();
    (parent.join("a"), parent.join("b"))
}

#[test] // W§18 #1
fn one_repository_has_one_id_and_two_apps_get_two_names() {
    let t = TempDir::new().unwrap();
    let (app1, app2) = (repo(&t.path().join("x"), "app"), repo(&t.path().join("y"), "app"));
    let (a, b) = two_linked(&app1);
    let id = workspace_id(&co(&app1).common_dir);
    assert!(valid_ws_id(&id), "{id}");
    assert_eq!(workspace_id(&co(&a).common_dir), id);
    assert_eq!(workspace_id(&co(&b).common_dir), id);
    assert_ne!(workspace_id(&co(&app2).common_dir), id);
    let name = |dir: &Path| {
        let list = worktrees(&co(dir)).unwrap();
        slug(main_worktree(&list).unwrap().path.file_name().unwrap().to_str().unwrap())
    };
    let first = unique_name(&name(&a), &|_| false); // from a linked worktree: the main worktree gives the name
    assert_eq!(first, "app");
    assert_eq!(unique_name(&name(&app2), &|n| n == first), "app-2");
}

#[test] // AU77: main worktree first, branch without `refs/heads/`, detached has none
fn worktrees_lists_the_main_worktree_first() {
    let t = TempDir::new().unwrap();
    let w = repo(t.path(), "w");
    let (a, b) = two_linked(&w);
    let mut list = worktrees(&co(&a)).unwrap();
    assert_eq!(list[0], Worktree { path: canon(&w), branch: Some("main".into()), bare: false });
    assert_eq!(main_worktree(&list), Some(&list[0]));
    list[1..].sort_by(|x, y| x.path.cmp(&y.path));
    assert_eq!(list[1..], [
        Worktree { path: canon(&a), branch: Some("feat".into()), bare: false },
        Worktree { path: canon(&b), branch: None, bare: false },
    ]);
}

#[test] // AU77: the bare entry is skipped; with no other entry there is no main worktree
fn bare_repository_main_worktree_is_the_first_non_bare_entry() {
    let t = TempDir::new().unwrap();
    repo(t.path(), "w");
    git(t.path(), &["clone", "-q", "--bare", "w", "repo.git"]);
    let bare = canon(&t.path().join("repo.git"));
    let only = Checkout { top: bare.clone(), common_dir: bare.clone(), main_root: bare.clone(), deadline: Instant::now() + GIT };
    let list = worktrees(&only).unwrap();
    assert_eq!(list, [Worktree { path: bare.clone(), branch: None, bare: true }]);
    assert_eq!(main_worktree(&list), None);
    git(&t.path().join("repo.git"), &["worktree", "add", "-q", "../main", "main"]);
    let c = co(&t.path().join("main"));
    assert_eq!(c.main_root, bare); // the AU77 premise: `Checkout.main_root` is the bare directory
    let list = worktrees(&c).unwrap();
    assert!(list[0].bare);
    let main = Worktree { path: canon(&t.path().join("main")), branch: Some("main".into()), bare: false };
    assert_eq!(main_worktree(&list), Some(&main));
}

#[test] // the git call is bounded by `Checkout.deadline`; a git failure is an error
fn worktrees_fails_after_the_deadline_and_when_git_fails() {
    let t = TempDir::new().unwrap();
    let w = repo(t.path(), "w");
    assert!(worktrees(&Checkout { deadline: Instant::now(), ..co(&w) }).is_err());
    let gone = t.path().join("gone"); // no such directory: `git -C` exits non-zero
    let c = Checkout { top: gone.clone(), common_dir: gone.clone(), main_root: gone, deadline: Instant::now() + GIT };
    assert!(worktrees(&c).is_err());
}
