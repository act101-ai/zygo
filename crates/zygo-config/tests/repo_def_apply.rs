//! WD-7: the approved definition added to the merged config with the source workspace-files (R§8, R§9.1, R§10, R-C8).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use zygo_config::layers::load_with_repo_reader;
use zygo_config::repo_def::{self, Applied, Definition, Files, REPOSITORY_LOADOUT};
use zygo_config::*;
use zygo_core::overrides::Overrides;
use zygo_core::paths::Paths;

// ---- helpers ----

fn def(ws: Option<&str>, profiles: &[(&str, &str)]) -> Definition {
    repo_def::parse(&Files {
        full_ref: Some("refs/remotes/origin/main".into()),
        commit: "a".repeat(40),
        workspace: ws.map(|t| t.as_bytes().to_vec()),
        profiles: profiles
            .iter()
            .map(|(n, t)| (n.to_string(), t.as_bytes().to_vec()))
            .collect::<BTreeMap<_, _>>(),
        ignored: vec![],
    })
    .unwrap()
}

#[must_use = "the directory is removed when this value drops; bind it"]
struct Tmp(tempfile::TempDir);
impl std::ops::Deref for Tmp {
    type Target = Path;
    fn deref(&self) -> &Path {
        self.0.path()
    }
}

#[must_use = "the directory is removed when this value drops; bind it"]
struct Cx {
    _d: tempfile::TempDir,
    c: LoadContext,
}
impl std::ops::Deref for Cx {
    type Target = LoadContext;
    fn deref(&self) -> &LoadContext {
        &self.c
    }
}

fn tmp(name: &str) -> Tmp {
    Tmp(tempfile::Builder::new().prefix(&format!("zygo-wd7-{name}-")).tempdir().unwrap())
}

static CTX_N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn ctx(user: Option<&str>, repo: Option<PathBuf>, team: Option<&str>) -> Cx {
    let n = CTX_N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let d = tmp(&format!("ctx{n}"));
    let config_file = d.join("config.toml");
    if let Some(u) = user {
        std::fs::write(&config_file, u).unwrap();
    }
    let paths = Paths {
        config_file,
        cache_dir: d.join("cache"),
        data_dir: d.join("data"),
        sessions_dir: d.join("cache/sessions"),
        marketplaces_dir: d.join("cache/marketplaces"),
        plugin_store: d.join("data/plugins"),
        approvals_file: d.join("data/approvals.toml"),
        usage_db: d.join("data/usage.db"),
    };
    Cx { c: LoadContext { paths, repo, team: team.map(|t| toml::from_str(t).unwrap()) }, _d: d.0 }
}

fn git(dir: &Path, args: &[&str]) {
    let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
    let out = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid", "-c", "commit.gpgsign=false", "-C"])
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", null)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CEILING_DIRECTORIES", std::env::temp_dir())
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

const RUST: &str = "overrides = { review = \"gpt-6-sol\" }\ndisabled = [\"design\"]\n";
const USER_LOADOUT: &str = "[[loadout.repository.window]]\n[[loadout.repository.window.pane]]\nkind = \"claude-code\"\nname = \"mine\"\n";

fn merged(user: Option<&str>, team: Option<&str>) -> (Cx, Merged) {
    let c = ctx(user, None, team);
    let m = load_with_builtin(&c, toml::Table::new()).unwrap();
    (c, m)
}

// ---- apply_to_config ----

#[test]
fn repository_profile_added_without_features() {
    let mut cfg = Config::default();
    let a = repo_def::apply_to_config(&mut cfg, &def(None, &[("rust", RUST)]));
    assert_eq!(a, Applied { profiles: vec!["rust".into()], shadowed_profiles: vec![], loadout: false, loadout_shadowed: false });
    assert_eq!(
        cfg.profile["rust"],
        ProfileConfig {
            overrides: BTreeMap::from([("review".into(), "gpt-6-sol".into())]),
            disabled: vec!["design".into()],
            ..ProfileConfig::default()
        }
    );
    assert_eq!(cfg.profile["rust"].features, None);
    // R§8 "§11.2 selection": the repository cannot set a default profile.
    assert_eq!(cfg.user.default_profile, None);
}

#[test]
fn existing_profile_shadows_by_name() {
    let mut cfg = merged(Some("[profile.rust]\nprompt = \"u\"\n"), None).1.config;
    let before = cfg.profile["rust"].clone();
    let a = repo_def::apply_to_config(&mut cfg, &def(None, &[("rust", RUST), ("go", RUST)]));
    assert_eq!(a.profiles, ["go"]);
    assert_eq!(a.shadowed_profiles, ["rust"]);
    assert_eq!(cfg.profile["rust"], before);
    // No key-by-key merge.
    assert!(cfg.profile["rust"].overrides.is_empty() && cfg.profile["rust"].disabled.is_empty());
}

#[test]
fn repository_loadout_added() {
    let d = def(Some("version = 1\n[members.a]\nprofile = \"rust\"\n"), &[]);
    let mut cfg = Config::default();
    let a = repo_def::apply_to_config(&mut cfg, &d);
    assert!(a.loadout && !a.loadout_shadowed);
    assert_eq!(cfg.loadout[REPOSITORY_LOADOUT], repo_def::repository_loadout(&d).unwrap());
}

#[test]
fn user_loadout_repository_stays() {
    let mut cfg = merged(Some(USER_LOADOUT), None).1.config;
    let before = cfg.loadout[REPOSITORY_LOADOUT].clone();
    let a = repo_def::apply_to_config(&mut cfg, &def(Some("version = 1\n"), &[]));
    assert!(!a.loadout && a.loadout_shadowed);
    assert_eq!(cfg.loadout[REPOSITORY_LOADOUT], before);
}

#[test]
fn no_workspace_file_no_loadout() {
    let mut cfg = Config::default();
    let a = repo_def::apply_to_config(&mut cfg, &def(None, &[("rust", RUST)]));
    assert!(!a.loadout && !a.loadout_shadowed);
    assert!(!cfg.loadout.contains_key(REPOSITORY_LOADOUT));
}
