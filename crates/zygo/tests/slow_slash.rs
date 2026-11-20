//! N50: the slash forms of the slow verbs reply with the shell command and change nothing (spec §17.1).

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use tempfile::TempDir;
use zygo::cmd::marketplace::run_io;
use zygo::cmd::plugins;
use zygo::cmd::verbs::VerbCtx;
use zygo::cmd::UsageError;
use zygo_core::paths::Paths;
use zygo_plugins::sync::SyncOpts;

const WIDE: Duration = Duration::from_secs(120);

const NULL: &str = if cfg!(windows) { "NUL" } else { "/dev/null" };

fn ctx(slash: bool) -> (VerbCtx, TempDir) {
    let t = tempfile::tempdir().unwrap();
    let r = t.path();
    let paths = Paths {
        config_file: r.join("config.toml"),
        cache_dir: r.join("cache"),
        data_dir: r.join("data"),
        sessions_dir: r.join("cache/sessions"),
        marketplaces_dir: r.join("cache/marketplaces"),
        plugin_store: r.join("data/plugins"),
        approvals_file: r.join("data/approvals.toml"),
        usage_db: r.join("data/usage.db"),
    };
    (with(&paths, slash), t)
}

fn with(paths: &Paths, slash: bool) -> VerbCtx { VerbCtx { slash, save: false, session_dir: None, paths: paths.clone() } }

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git").env("GIT_CONFIG_GLOBAL", NULL).env("GIT_CONFIG_NOSYSTEM", "1")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid", "-c", "commit.gpgsign=false", "-c", "tag.gpgsign=false"])
        .arg("-C").arg(dir).args(args).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

fn write(dir: &Path, rel: &str, text: &str) {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

/// A repository at `dir` with the code plugin `demo` at 1.0.0, tagged `v1.0.0`; its path.
fn market(dir: &Path) -> String {
    std::fs::create_dir_all(dir).unwrap();
    git(dir, &["init", "-q", "-b", "main"]);
    write(dir, ".claude-plugin/marketplace.json",
        r#"{"name":"team","owner":{"name":"Owner"},"plugins":[{"name":"demo","source":"./plugins/demo","description":"Demo","version":"1.0.0"}]}"#);
    write(dir, "plugins/demo/.claude-plugin/plugin.json", r#"{"name":"demo","version":"1.0.0","description":"Demo plugin","license":"MIT"}"#);
    write(dir, "plugins/demo/skills/hello/SKILL.md", "---\nname: hello\ndescription: Says hello\n---\nSay hello.\n");
    write(dir, "plugins/demo/scripts/run.sh", "echo hi\n");
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", "1.0.0"]);
    git(dir, &["tag", "v1.0.0"]);
    dir.display().to_string()
}

/// A git program that does not exist: any spawn fails.
fn no_git(dir: &Path) -> SyncOpts {
    SyncOpts { git_program: Some(dir.join("git-must-not-run")), ..Default::default() }
}

/// Every file under `dir` with its bytes, sorted, relative to `dir`.
fn snapshot(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut v: Vec<_> = walkdir::WalkDir::new(dir).sort_by_file_name().into_iter().filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .map(|e| (e.path().strip_prefix(dir).unwrap().to_path_buf(), std::fs::read(e.path()).unwrap())).collect();
    v.sort();
    v
}

fn cfg_team(paths: &Paths, src: &str) {
    std::fs::write(&paths.config_file,
        format!("[[marketplace]]\nname = \"team\"\nsource = {}\nref = \"v1.0.0\"\n", toml::Value::String(src.into()))).unwrap();
}

fn args(a: &[&str]) -> Vec<String> { a.iter().map(|s| s.to_string()).collect() }

fn mk(c: &VerbCtx, a: &[&str], git: &SyncOpts) -> anyhow::Result<String> {
    run_io(c, &args(a), None, Some(toml::Table::new()), &mut Cursor::new(String::new()), &mut vec![], git)
}

fn plug(c: &VerbCtx, a: &[&str]) -> anyhow::Result<String> {
    plugins::run_with(c, &args(a), None, Some(toml::Table::new()), &mut Cursor::new(String::new()))
}

/// Fills the store through the shell forms: marketplace `team` with `demo` enabled, not approved.
fn fill_store(paths: &Paths, src: &str, dir: &Path) {
    let empty = dir.join("empty-gitconfig");
    std::fs::write(&empty, "").unwrap();
    let opts = SyncOpts { per_marketplace: WIDE, git_config_global: Some(empty), ..Default::default() };
    let out = run_io(&with(paths, false), &args(&["add", src, "--enable", "demo"]), None, Some(toml::Table::new()),
        &mut Cursor::new("n\n".to_string()), &mut vec![], &opts).unwrap();
    assert!(out.contains("not approved"), "{out}");
}

#[test]
fn slash_marketplace_add_replies_with_the_shell_command() {
    let (c, t) = ctx(true);
    let src = market(&t.path().join("origin"));
    let before = snapshot(t.path());
    let out = mk(&c, &["add", &src, "--name", "n", "--enable", "all"], &no_git(t.path())).unwrap();
    assert_eq!(out, format!("run in your shell: zygo marketplace add {src} --name n --enable all"));
    assert_eq!(snapshot(t.path()), before);
    assert!(!c.paths.config_file.exists() && !c.paths.marketplaces_dir.exists() && !c.paths.plugin_store.exists());
}

#[test]
fn slash_marketplace_update_replies_with_the_shell_command() {
    let (c, t) = ctx(true);
    let src = market(&t.path().join("origin"));
    cfg_team(&c.paths, &src);
    let before = snapshot(t.path());
    for (a, want) in [(&["update"][..], "update"), (&["update", "team"], "update team"), (&["update", "--all", "--accept"], "update --all --accept")] {
        assert_eq!(mk(&c, a, &no_git(t.path())).unwrap(), format!("run in your shell: zygo marketplace {want}"));
        assert_eq!(snapshot(t.path()), before);
    }
}

#[test]
fn slash_plugins_sync_replies_with_the_shell_command() {
    let (c, t) = ctx(true);
    let src = market(&t.path().join("origin"));
    cfg_team(&c.paths, &src);
    let before = snapshot(t.path());
    assert_eq!(plug(&c, &["sync"]).unwrap(), "run in your shell: zygo plugins sync");
    assert!(!c.paths.marketplaces_dir.exists());
    assert_eq!(snapshot(t.path()), before);
}

#[test]
fn slash_plugins_enable_of_an_unstored_plugin_replies_with_the_shell_command() {
    let (c, t) = ctx(true);
    let src = market(&t.path().join("origin"));
    cfg_team(&c.paths, &src);
    let before = snapshot(t.path());
    assert_eq!(plug(&c, &["enable", "team:demo"]).unwrap(), "run in your shell: zygo plugins enable team:demo");
    assert_eq!(snapshot(t.path()), before);
    assert!(!c.paths.plugin_store.exists() && !c.paths.approvals_file.exists());
}

#[test]
fn slash_plugins_enable_of_a_stored_plugin_lists_the_code_without_syncing() {
    let (c, t) = ctx(true);
    let src = market(&t.path().join("origin"));
    fill_store(&c.paths, &src, t.path());
    let state = || { let mut v = snapshot(&c.paths.cache_dir); v.extend(snapshot(&c.paths.data_dir)); v };
    let before = state();
    let out = plug(&c, &["enable", "team:demo"]).unwrap();
    assert!(out.starts_with("enabled team:demo\n"), "{out}");
    assert!(out.contains("code file scripts/run.sh"), "{out}");
    assert!(out.ends_with("run: zygo plugins approve team:demo\napplies at next launch"), "{out}");
    assert_eq!(state(), before);
    assert!(!c.paths.approvals_file.exists());
}

#[cfg(unix)]
#[test]
fn slash_fetching_forms_start_no_git_child() {
    use std::os::unix::fs::PermissionsExt;
    let (c, t) = ctx(true);
    let src = market(&t.path().join("origin"));
    cfg_team(&c.paths, &src);
    let rec = t.path().join("rec.sh");
    std::fs::write(&rec, format!("#!/bin/sh\necho x >> {}/calls\nexit 1\n", t.path().display())).unwrap();
    std::fs::set_permissions(&rec, std::fs::Permissions::from_mode(0o755)).unwrap();
    let opts = SyncOpts { git_program: Some(rec), ..Default::default() };
    assert!(mk(&c, &["add", &src], &opts).unwrap().starts_with("run in your shell: zygo marketplace add "));
    assert_eq!(mk(&c, &["update"], &opts).unwrap(), "run in your shell: zygo marketplace update");
    assert!(!t.path().join("calls").exists());
}

#[test]
fn slash_plugins_approve_changes_nothing() {
    let (c, t) = ctx(true);
    let src = market(&t.path().join("origin"));
    cfg_team(&c.paths, &src);
    let before = snapshot(t.path());
    let e = plug(&c, &["approve", "team:demo"]).unwrap_err();
    assert_eq!(e.to_string(), "team:demo is not in the store; run zygo plugins sync");
    assert!(!c.paths.plugin_store.exists());
    assert_eq!(snapshot(t.path()), before);
    fill_store(&c.paths, &src, t.path());
    let before = snapshot(t.path());
    let out = plug(&c, &["approve", "team:demo"]).unwrap();
    assert!(out.contains("code file scripts/run.sh"), "{out}");
    assert!(out.ends_with("run: zygo plugins approve team:demo"), "{out}");
    assert_eq!(snapshot(t.path()), before);
    assert!(!c.paths.approvals_file.exists());
}

#[test]
fn slash_usage_errors_stay_usage_errors() {
    let (c, t) = ctx(true);
    for a in [&["add"][..], &["add", "s", "--bogus"], &["update", "a", "b"], &["update", "a", "--all"]] {
        let e = mk(&c, a, &no_git(t.path())).unwrap_err();
        assert!(e.downcast_ref::<UsageError>().is_some(), "{a:?}: {e}");
    }
}
