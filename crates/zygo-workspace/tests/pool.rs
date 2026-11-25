use std::path::{Path, PathBuf};

use zygo_workspace::pool::*;

const DEFAULT_SET: [&str; 5] = ["orchestrator", "worker-1", "worker-2", "worker-3", "worker-4"];

fn names(n: &[&str]) -> Vec<Option<String>> { n.iter().map(|s| Some(s.to_string())).collect() }

/// A pool with directories `a` and `b`, a file `c` and a directory `.x`.
fn pool_ab() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    for s in ["a", "b", ".x"] {
        std::fs::create_dir(d.path().join(s)).unwrap();
    }
    std::fs::write(d.path().join("c"), "").unwrap();
    d
}

#[test]
fn list_keeps_visible_directories_in_name_order() {
    let d = pool_ab();
    assert_eq!(list(d.path()).unwrap(), vec![d.path().join("a"), d.path().join("b")]);
}

#[test]
fn list_refuses_a_missing_path_a_file_and_no_subdirectory() {
    let d = tempfile::tempdir().unwrap();
    assert_eq!(list(&d.path().join("missing")), Err("not a directory".into()));
    std::fs::write(d.path().join("f"), "").unwrap();
    assert_eq!(list(&d.path().join("f")), Err("not a directory".into()));
    std::fs::create_dir(d.path().join(".hidden")).unwrap();
    assert_eq!(list(d.path()), Err("no subdirectory".into())); // holds only a file and a dot directory
}

#[test]
fn expand_follows_the_config_dir_tilde_rule() {
    let h = tempfile::tempdir().unwrap();
    assert_eq!(expand("~", Some(h.path())), Ok(h.path().to_path_buf()));
    assert_eq!(expand("~/", Some(h.path())), Ok(h.path().to_path_buf()));
    assert_eq!(expand("~/.claude-interns", Some(h.path())), Ok(h.path().join(".claude-interns")));
    let abs = h.path().to_string_lossy().into_owned();
    assert_eq!(expand(&abs, None), Ok(PathBuf::from(&abs)));
    assert_eq!(expand("~/x", None), Err("no home directory".into()));
    assert_eq!(tilde("rel", Some(h.path())), None);
}

#[test]
fn expand_refuses_a_relative_path() {
    let h = tempfile::tempdir().unwrap();
    assert_eq!(expand("pool", Some(h.path())), Err("must be absolute after ~ expansion".into()));
    assert_eq!(expand("~x/pool", Some(h.path())), Err("must be absolute after ~ expansion".into()));
}

#[test]
fn default_set_on_two_directories_alternates() {
    let d = pool_ab();
    let dirs = list(d.path()).unwrap();
    let got: Vec<(&str, &Path)> =
        DEFAULT_SET.iter().zip(assign(&names(&DEFAULT_SET), dirs.len())).map(|(n, i)| (*n, dirs[i].as_path())).collect();
    let (a, b) = (d.path().join("a"), d.path().join("b"));
    assert_eq!(got, vec![("orchestrator", &*a), ("worker-1", &*b), ("worker-2", &*a), ("worker-3", &*b), ("worker-4", &*a)]);
}

#[test]
fn order_puts_named_panes_by_name_then_unnamed_in_pane_order() {
    let n = vec![Some("b".into()), None, Some("a".into()), None];
    assert_eq!(order(&n), vec![2, 0, 1, 3]);
    assert_eq!(assign(&n, 2), vec![1, 0, 0, 1]);
}

#[test]
fn shared_lines_name_each_shared_directory_once() {
    let (a, b) = (PathBuf::from("/p/a"), PathBuf::from("/p/b"));
    let dir_of: Vec<(String, PathBuf)> =
        [(0, &a), (1, &b), (2, &a), (3, &b), (4, &a)].iter().map(|(i, p)| (DEFAULT_SET[*i].to_string(), (*p).clone())).collect();
    assert_eq!(
        shared_lines(&dir_of),
        vec![
            format!("pool: {} is shared by orchestrator, worker-2, worker-4", a.display()),
            format!("pool: {} is shared by worker-1, worker-3", b.display()),
        ]
    );
    assert_eq!(shared_lines(&dir_of[..2]), Vec::<String>::new()); // k = n: nothing shared
}

/// R§18 item 13: unix only; Windows has no portable way to make a file unreadable to its owner.
#[cfg(unix)]
#[test]
fn mode_000_files_change_nothing() {
    use std::os::unix::fs::PermissionsExt;
    let d = pool_ab();
    for f in [d.path().join("a/.credentials.json"), d.path().join("a/settings.json"), d.path().join("z")] {
        std::fs::write(&f, "{}").unwrap();
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o000)).unwrap();
    }
    assert_eq!(list(d.path()).unwrap(), vec![d.path().join("a"), d.path().join("b")]);
}

// Fix round 1.

#[test]
fn tilde_strips_every_leading_separator() {
    let h = tempfile::tempdir().unwrap();
    assert_eq!(tilde("~//x", Some(h.path())), Some(Ok(h.path().join("x"))));
    assert_eq!(tilde("~///", Some(h.path())), Some(Ok(h.path().to_path_buf())));
    assert_eq!(tilde("~/", Some(h.path())), Some(Ok(h.path().to_path_buf())));
    assert_eq!(tilde("~", None), Some(Err("no home directory".into())));
}

#[cfg(windows)]
#[test]
fn tilde_refuses_a_rest_with_a_prefix_or_root() {
    let h = tempfile::tempdir().unwrap();
    assert_eq!(tilde("~/\\x", Some(h.path())), Some(Ok(h.path().join("x"))));
    assert_eq!(tilde("~/C:\\x", Some(h.path())), Some(Err("must be absolute after ~ expansion".into())));
    assert_eq!(tilde("~/C:x", Some(h.path())), Some(Err("must be absolute after ~ expansion".into())));
}

#[test]
fn home_uses_an_absolute_variable_else_the_os_home() {
    let h = tempfile::tempdir().unwrap();
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    let abs = h.path().as_os_str().to_os_string();
    assert_eq!(home_in(|k| (k == var).then(|| abs.clone())), Some(h.path().to_path_buf()));
    assert_eq!(home_in(|k| (k == var).then(|| "rel".into())), std::env::home_dir());
    assert_eq!(home(), home_in(|k| std::env::var_os(k)));
}

#[test]
fn list_is_in_byte_order_of_name() {
    let d = tempfile::tempdir().unwrap();
    let want = ["10", "9", "Cap", "a", "\u{e9}"];
    for s in ["a", "\u{e9}", "Cap", "9", "10"] {
        std::fs::create_dir(d.path().join(s)).unwrap();
    }
    assert_eq!(list(d.path()).unwrap(), want.iter().map(|s| d.path().join(s)).collect::<Vec<_>>());
}

#[test]
fn shared_lines_escape_the_directory_and_the_names() {
    let p = PathBuf::from("/p/a\nb");
    let dir_of = vec![("w\u{1b}[31m".to_string(), p.clone()), ("w2".to_string(), p)];
    assert_eq!(shared_lines(&dir_of), vec!["pool: /p/a\\nb is shared by w\\u{1b}[31m, w2".to_string()]);
}

/// Restores a mode on drop, so the temp directory can be removed.
#[cfg(unix)]
struct Mode(PathBuf, u32);

#[cfg(unix)]
impl Drop for Mode {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(self.1));
    }
}

#[cfg(unix)]
fn chmod(p: &Path, m: u32) -> Mode {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(m)).unwrap();
    Mode(p.to_path_buf(), 0o755)
}

#[cfg(unix)]
#[test]
fn a_root_without_search_permission_is_not_readable() {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir(d.path().join("a")).unwrap();
    let _m = chmod(d.path(), 0o444);
    assert_eq!(list(d.path()), Err("not readable (PermissionDenied)".into()));
}

#[cfg(unix)]
#[test]
fn a_pool_under_a_mode_000_parent_is_not_readable() {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join("q/pool/a")).unwrap();
    let _m = chmod(&d.path().join("q"), 0o000);
    let r = list(&d.path().join("q/pool"));
    assert!(r.as_ref().is_err_and(|e| e.starts_with("not readable")), "{r:?}");
}

#[cfg(unix)]
#[test]
fn links_to_directories_are_followed_and_broken_links_skipped() {
    use std::os::unix::fs::symlink;
    let d = tempfile::tempdir().unwrap();
    let p = d.path();
    std::fs::create_dir(p.join("a")).unwrap();
    symlink(p.join("a"), p.join("l")).unwrap();
    symlink(p.join("nowhere"), p.join("dangling")).unwrap();
    symlink(p.join("loop1"), p.join("loop2")).unwrap();
    symlink(p.join("loop2"), p.join("loop1")).unwrap();
    std::fs::write(p.join("f"), "").unwrap();
    symlink(p.join("f"), p.join("g")).unwrap();
    assert_eq!(list(p).unwrap(), vec![p.join("a"), p.join("l")]);
}

#[cfg(unix)]
#[test]
fn two_links_to_one_directory_are_shared() {
    use std::os::unix::fs::symlink;
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir(d.path().join("a")).unwrap();
    symlink(d.path().join("a"), d.path().join("l")).unwrap();
    let dirs = list(d.path()).unwrap();
    let n = names(&["m1", "m2"]);
    let dir_of: Vec<(String, PathBuf)> =
        n.iter().zip(assign(&n, dirs.len())).map(|(m, i)| (m.clone().unwrap(), dirs[i].clone())).collect();
    assert_eq!(dir_of[1].1, d.path().join("l"));
    assert_eq!(shared_lines(&dir_of), vec![format!("pool: {} is shared by m1, m2", d.path().join("a").display())]);
}
