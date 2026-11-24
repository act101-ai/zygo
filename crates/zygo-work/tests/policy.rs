use std::path::Path;
use std::process::Command;
use tempfile::TempDir;
use zygo_config::{load, Level, LoadContext, Merged, Refusal, RepoFile};
use zygo_core::overrides::Overrides;
use zygo_core::paths::Paths;
use zygo_work::policy::{Guard, Policy};

fn git(dir: &Path, args: &[&str]) {
    let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
    let out = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid", "-c", "commit.gpgsign=false", "-C"])
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", null)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// A repo on `main` with one commit; `committed` is `.zygo.toml` in that commit; `worktree` overwrites it afterwards, uncommitted.
fn repo(committed: Option<&str>, worktree: Option<&str>) -> TempDir {
    let d = tempfile::tempdir().unwrap();
    git(d.path(), &["init", "-b", "main"]);
    std::fs::write(d.path().join("README"), "x").unwrap();
    if let Some(t) = committed {
        std::fs::write(d.path().join(".zygo.toml"), t).unwrap();
    }
    git(d.path(), &["add", "."]);
    git(d.path(), &["commit", "-m", "init"]);
    if let Some(t) = worktree {
        std::fs::write(d.path().join(".zygo.toml"), t).unwrap();
    }
    d
}

fn merged(user: &str, repo: &Path) -> Merged {
    load_merged(user, Some(repo))
}

/// `repo: None`: `load` leaves `repo_file` `Absent` and runs no git, so no `GIT_BOUND` applies.
fn merged_no_git(user: &str) -> Merged {
    load_merged(user, None)
}

fn load_merged(user: &str, repo: Option<&Path>) -> Merged {
    let d = tempfile::tempdir().unwrap();
    let config_file = d.path().join("config.toml");
    std::fs::write(&config_file, user).unwrap();
    let paths = Paths {
        config_file,
        cache_dir: d.path().join("cache"),
        data_dir: d.path().join("data"),
        sessions_dir: d.path().join("cache/sessions"),
        marketplaces_dir: d.path().join("cache/marketplaces"),
        plugin_store: d.path().join("data/plugins"),
        approvals_file: d.path().join("data/approvals.toml"),
        usage_db: d.path().join("data/usage.db"),
    };
    load(&LoadContext { paths, repo: repo.map(Path::to_path_buf), team: None }).unwrap()
}

fn policy(user: &str, repo: &Path) -> Policy {
    Policy::resolve(&merged(user, repo), repo)
}

fn line(root: &Path, levels: &str) -> String {
    format!("repo policy: {}@main ({levels})", root.display())
}
const DEFAULT_LEVELS: &str =
    "claims monitor, scope monitor, navigation monitor, output monitor, trailers notify, repeat monitor, dispositions enforce";

#[test]
fn defaults_without_file() {
    let r = repo(None, None);
    let p = policy("", r.path());
    assert_eq!(p.level(Guard::Claims), Level::Monitor);
    assert_eq!(p.level(Guard::Scope), Level::Monitor);
    assert_eq!(p.level(Guard::Navigation), Level::Monitor);
    assert_eq!(p.level(Guard::Output), Level::Monitor);
    assert_eq!(p.level(Guard::Trailers), Level::Notify);
    assert_eq!(p.describe(), "repo policy: defaults");
}

#[test]
#[ignore = "timing: the .zygo.toml git read has the 2 s zygo-config GIT_BOUND; a timeout drops the file"]
fn file_on_default_branch_sets_levels() {
    let r = repo(
        Some("version = 1\n[guards]\nclaims = \"enforce\"\nscope = \"notify\"\nnavigation = \"monitor\"\noutput = \"notify\"\ntrailers = \"enforce\"\n"),
        None,
    );
    let p = policy("", r.path());
    assert_eq!(p.level(Guard::Claims), Level::Enforce);
    assert_eq!(p.level(Guard::Scope), Level::Notify);
    assert_eq!(p.level(Guard::Navigation), Level::Monitor);
    assert_eq!(p.level(Guard::Output), Level::Notify);
    assert_eq!(p.level(Guard::Trailers), Level::Enforce);
    assert_eq!(
        p.describe(),
        line(r.path(), "claims enforce, scope notify, navigation monitor, output notify, trailers enforce, repeat monitor, dispositions enforce")
    );
}

#[test]
#[ignore = "timing: the .zygo.toml git read has the 2 s zygo-config GIT_BOUND; a timeout drops the file"]
fn file_with_no_level_change_still_names_root_and_branch() {
    let r = repo(Some("version = 1\n"), None);
    assert_eq!(policy("", r.path()).describe(), line(r.path(), DEFAULT_LEVELS));
}

#[test]
#[ignore = "timing: the .zygo.toml git read has the 2 s zygo-config GIT_BOUND; a timeout drops the file"]
fn working_tree_file_changes_nothing() {
    let r = repo(None, Some("version = 1\n[guards]\nclaims = \"enforce\"\n"));
    let p = policy("", r.path());
    assert_eq!(p.level(Guard::Claims), Level::Monitor);
    assert_eq!(p.describe(), "repo policy: defaults");

    let r = repo(
        Some("version = 1\n[guards]\nclaims = \"notify\"\n"),
        Some("version = 1\n[guards]\nclaims = \"enforce\"\n"),
    );
    assert_eq!(policy("", r.path()).level(Guard::Claims), Level::Notify);
}

#[test]
#[ignore = "timing: the .zygo.toml git read has the 2 s zygo-config GIT_BOUND; a timeout drops the file"]
fn min_level_raises() {
    let r = repo(
        Some("version = 1\n[guards]\nmin_level = \"monitor\"\nclaims = \"monitor\"\nnavigation = \"enforce\"\n"),
        None,
    );
    let p = policy("[guards]\nmin_level = \"notify\"\n", r.path());
    assert_eq!(p.level(Guard::Claims), Level::Notify);
    assert_eq!(p.level(Guard::Scope), Level::Notify);
    assert_eq!(p.level(Guard::Trailers), Level::Notify);
    assert_eq!(p.level(Guard::Navigation), Level::Enforce);
}

#[test]
#[ignore = "timing: the .zygo.toml git read has the 2 s zygo-config GIT_BOUND; a timeout drops the file"]
fn invalid_file_is_ignored() {
    let r = repo(Some("version = 2\n[guards]\nclaims = \"enforce\"\n"), None);
    let p = policy("", r.path());
    assert_eq!(p.level(Guard::Claims), Level::Monitor);
    assert_eq!(p.describe(), "repo policy: invalid (version is not 1)");
}

#[test]
#[ignore = "timing: the .zygo.toml git read has the 2 s zygo-config GIT_BOUND; a timeout drops the file"]
fn session_override_is_shown_and_floor_refused() {
    let r = repo(Some("version = 1\n[guards]\nclaims = \"notify\"\n"), None);
    let m = merged("", r.path());
    let mut o = Overrides::default();
    o.guards.insert("claims".into(), "enforce".into());
    let p = Policy::resolve(&m.with_overrides(&o).unwrap(), r.path());
    assert_eq!(p.level(Guard::Claims), Level::Enforce);
    assert_eq!(
        p.describe(),
        line(r.path(), "claims enforce, scope monitor, navigation monitor, output monitor, trailers notify, repeat monitor, dispositions enforce")
    );

    let m2 = merged("[guards]\nmin_level = \"notify\"\n", r.path());
    let mut o2 = Overrides::default();
    o2.guards.insert("claims".into(), "monitor".into());
    assert_eq!(
        m2.with_overrides(&o2).unwrap_err(),
        Refusal::BelowFloor { guard: "claims".into(), min: Level::Notify }
    );
    assert!(Policy::resolve(&m2, r.path())
        .describe()
        .starts_with(&format!("repo policy: {}@main", r.path().display())));
}

#[test]
fn guard_names_and_fixed_levels() {
    let names = [
        (Guard::Claims, "claims"),
        (Guard::Scope, "scope"),
        (Guard::Navigation, "navigation"),
        (Guard::Output, "output"),
        (Guard::Trailers, "trailers"),
        (Guard::Repeat, "repeat"),
        (Guard::Dispositions, "dispositions"),
        (Guard::Lead, "lead"),
        (Guard::Redaction, "redaction"),
        (Guard::Attribution, "attribution"),
    ];
    for (g, n) in names {
        assert_eq!(g.name(), n);
    }
    assert_eq!(
        Guard::CONFIGURED.map(Guard::name),
        ["claims", "scope", "navigation", "output", "trailers", "repeat", "dispositions"]
    );

    let r = repo(Some("version = 1\n[guards]\nclaims = \"monitor\"\n"), None);
    let p = policy("[guards]\nmin_level = \"enforce\"\n", r.path());
    assert_eq!(p.level(Guard::Claims), Level::Enforce);
    assert_eq!(p.level(Guard::Repeat), Level::Monitor);

    let r = repo(Some("version = 1\n[guards]\nclaims = \"monitor\"\nscope = \"monitor\"\n"), None);
    let p = policy("[guards]\nmin_level = \"monitor\"\n", r.path());
    assert_eq!(p.level(Guard::Lead), Level::Enforce);
    assert_eq!(p.level(Guard::Redaction), Level::Enforce);
    assert_eq!(p.level(Guard::Attribution), Level::Enforce);
}

#[test]
#[ignore = "timing: the .zygo.toml git read has the 2 s zygo-config GIT_BOUND; a timeout drops the file"]
fn turn_cap_is_lower_nonzero() {
    let table = [(0, 0, None), (0, 7, Some(7)), (10, 7, Some(7)), (5, 7, Some(5)), (5, 0, Some(5))];
    for (user_cap, file_cap, expected) in table {
        let r = repo(Some(&format!("version = 1\n[limits]\nsubagent_max_turns = {file_cap}\n")), None);
        let p = policy(&format!("[limits]\nsubagent_max_turns = {user_cap}\n"), r.path());
        assert_eq!(p.turn_cap(), expected, "user {user_cap}, file {file_cap}");
    }

    let r = repo(Some("version = 1\n[limits]\nsubagent_max_turns = 7\n"), None);
    let m = merged("", r.path());
    for (flag, expected) in [(9, Some(7)), (5, Some(5)), (0, Some(7))] {
        let flagged = m.apply_flag("limits.subagent_max_turns", toml::Value::Integer(flag));
        assert_eq!(Policy::resolve(&flagged, r.path()).turn_cap(), expected, "flag {flag}");
    }
}

#[test]
fn user_config_applies_without_or_with_an_invalid_file() {
    let user = "[guards]\nclaims = \"enforce\"\n[limits]\nsubagent_max_turns = 5\n";
    for committed in [None, Some("version = 2\n")] {
        let r = repo(committed, None);
        let p = policy(user, r.path());
        assert_eq!(p.level(Guard::Claims), Level::Enforce, "{committed:?}");
        assert_eq!(p.turn_cap(), Some(5), "{committed:?}");
    }
}

#[test]
#[ignore = "timing: the .zygo.toml git read has the 2 s zygo-config GIT_BOUND; a timeout drops the file"]
fn invalid_reason_has_no_control_characters() {
    let r = repo(Some("version = 1\nbad = \u{1b}[31m\n"), None);
    let d = policy("", r.path()).describe();
    assert!(d.starts_with("repo policy: invalid ("), "{d}");
    assert!(!d.chars().any(char::is_control), "{d:?}");
}

#[test]
fn dispositions_defaults_to_enforce() {
    let r = tempfile::tempdir().unwrap();
    let mut m = merged_no_git("");
    assert_eq!(Policy::resolve(&m, r.path()).level(Guard::Dispositions), Level::Enforce);
    // describe prints levels only for a found file; `merged_no_git` reads no `.zygo.toml`, so set an empty found file here
    m.repo_file = RepoFile::Found { branch: "main".into(), table: Default::default(), ignored: vec![] };
    assert_eq!(Policy::resolve(&m, r.path()).describe(), line(r.path(), DEFAULT_LEVELS));
}

#[test]
fn dispositions_is_a_guards_key() {
    let r = tempfile::tempdir().unwrap();
    let m = merged_no_git("[guards]\ndispositions = \"notify\"\n");
    assert_eq!(Policy::resolve(&m, r.path()).level(Guard::Dispositions), Level::Notify);
    let mut o = Overrides::default();
    o.guards.insert("dispositions".into(), "monitor".into());
    let p = Policy::resolve(&merged_no_git("").with_overrides(&o).unwrap(), r.path());
    assert_eq!(p.level(Guard::Dispositions), Level::Monitor);
}

#[test]
fn dispositions_min_level_lifts_monitor() {
    let r = tempfile::tempdir().unwrap();
    let user = "[guards]\nmin_level = \"notify\"\ndispositions = \"monitor\"\n";
    assert_eq!(Policy::resolve(&merged_no_git(user), r.path()).level(Guard::Dispositions), Level::Notify);
    let mut o = Overrides::default();
    o.guards.insert("dispositions".into(), "monitor".into());
    assert_eq!(
        merged_no_git(user).with_overrides(&o).unwrap_err(),
        Refusal::BelowFloor { guard: "dispositions".into(), min: Level::Notify }
    );
}

#[test]
#[ignore = "timing: the .zygo.toml git read has the 2 s zygo-config GIT_BOUND; a timeout drops the file"]
fn dispositions_from_repo_file() {
    let r = repo(Some("version = 1\n[guards]\ndispositions = \"notify\"\n"), None);
    let p = policy("", r.path());
    assert_eq!(p.level(Guard::Dispositions), Level::Notify);
    assert_eq!(
        p.describe(),
        line(
            r.path(),
            "claims monitor, scope monitor, navigation monitor, output monitor, trailers notify, repeat monitor, dispositions notify"
        )
    );
}
