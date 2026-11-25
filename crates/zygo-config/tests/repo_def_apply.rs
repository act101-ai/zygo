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

// ---- Merged::with_definition ----

#[test]
fn user_profile_shadows_on_merged() {
    let (_c, m) = merged(Some("[profile.rust]\nprompt = \"u\"\n"), None);
    let w = m.with_definition(&def(None, &[("rust", RUST)]));
    assert_eq!(w.config.profile["rust"], m.config.profile["rust"]);
    assert_eq!(w.shadowed_profiles(), ["rust"]);
    assert_eq!(w.definition_file_of("profile.rust.prompt"), None);
    assert!(w.show(None, &|_| true).contains("profile.rust.prompt = \"u\"  # user\n"));
    assert!(!w.show(None, &|_| true).contains("workspace-files"));
}

#[test]
fn team_preset_shadows_on_merged() {
    let (_c, m) = merged(Some(""), Some("[profile.rust]\ndisabled = [\"review\"]\n"));
    let w = m.with_definition(&def(None, &[("rust", RUST)]));
    assert_eq!(w.config.profile["rust"].disabled, ["review"]);
    assert!(w.config.profile["rust"].overrides.is_empty());
    assert_eq!(w.shadowed_profiles(), ["rust"]);
    assert_eq!(w.source_of("profile.rust"), Layer::Team);
}

fn with_ws_and_rust() -> Merged {
    let (_c, m) = merged(None, None);
    m.with_definition(&def(Some("version = 1\n"), &[("rust", RUST)]))
}

#[test]
fn show_labels_workspace_files() {
    let w = with_ws_and_rust();
    let all = w.show(None, &|_| true);
    assert!(all.contains("profile.rust.disabled = [\"design\"]  # workspace-files .zygo/profiles/rust.toml\n"), "{all}");
    assert!(all.contains("profile.rust.overrides.review = \"gpt-6-sol\"  # workspace-files .zygo/profiles/rust.toml\n"), "{all}");
    assert!(
        all.lines().any(|l| l.starts_with("loadout.repository.window = ") && l.ends_with("  # workspace-files .zygo/workspace.toml")),
        "{all}"
    );
    assert_eq!(w.definition_file_of("profile.rust.overrides.review"), Some(".zygo/profiles/rust.toml"));
    assert_eq!(w.definition_file_of("loadout.repository.window"), Some(".zygo/workspace.toml"));
    assert_eq!(w.definition_file_of("results.head_lines"), None);
    assert!(w.shadowed_profiles().is_empty());
}

#[test]
fn layer_filter_hides_workspace_files() {
    let d = tmp("repo");
    let found = RepoFile::Found {
        branch: "main".into(),
        table: toml::from_str("version = 1\n[navigation]\nlines = 7\n").unwrap(),
        ignored: vec![],
    };
    let m = load_with_repo_reader(&ctx(None, Some(d.to_path_buf()), None), toml::Table::new(), |_| found).unwrap();
    let w = m.with_definition(&def(Some("version = 1\n"), &[("rust", RUST)]));
    for l in [Layer::Builtin, Layer::Team, Layer::User, Layer::Repository, Layer::Profile, Layer::Flags, Layer::Session] {
        let s = w.show(Some(l), &|_| true);
        assert!(!s.contains("workspace-files"), "{l}: {s}");
        assert!(!s.lines().any(|x| x.starts_with("profile.rust.") || x.starts_with("loadout.repository.")), "{l}: {s}");
    }
    // R§10: `--layer repository` stays `.zygo.toml`.
    let repo = w.show(Some(Layer::Repository), &|_| true);
    assert_eq!(repo, "navigation.lines = 7  # repository\n");
}

#[test]
fn show_workspace_files_exact() {
    let (_c, m) = merged(Some("[results]\ntrim_threshold_bytes = 1000\n"), None);
    assert_eq!(m.show_workspace_files(), "");
    assert_eq!(
        m.with_definition(&def(None, &[("rust", RUST)])).show_workspace_files(),
        "profile.rust.disabled = [\"design\"]  # workspace-files .zygo/profiles/rust.toml\nprofile.rust.overrides.review = \"gpt-6-sol\"  # workspace-files .zygo/profiles/rust.toml\n"
    );
}

#[test]
fn user_loadout_repository_shadows_on_merged() {
    let (_c, m) = merged(Some(USER_LOADOUT), None);
    let w = m.with_definition(&def(Some("version = 1\n"), &[]));
    assert_eq!(w.config.loadout[REPOSITORY_LOADOUT], m.config.loadout[REPOSITORY_LOADOUT]);
    assert_eq!(w.definition_file_of("loadout.repository.window"), None);
    assert_eq!(w.show_workspace_files(), "");
}

#[test]
fn layer_has_no_workspace_files_variant() {
    assert!("workspace-files".parse::<Layer>().is_err());
    // No wildcard: a new variant fails to compile.
    let n = |l: Layer| match l {
        Layer::Builtin | Layer::Team | Layer::User | Layer::Repository | Layer::Profile | Layer::Flags | Layer::Session => 7,
    };
    assert_eq!(n(Layer::User), 7);
}

#[test]
fn apply_profile_on_repository_profile() {
    let (_c, m) = merged(None, None);
    let w = m.with_definition(&def(None, &[("rust", RUST)]));
    let p = w.apply_profile("rust").unwrap();
    assert_eq!(p.config.profile, w.config.profile);
    assert_eq!(p.config.features, w.config.features);
    assert_eq!(p.show(None, &|_| true), w.show(None, &|_| true));
}

#[test]
fn definition_survives_rebuild() {
    let (_c, m) = merged(Some("[profile.u]\nfeatures = { work_units = false }\n"), None);
    let w = m.with_definition(&def(Some("version = 1\n"), &[("rust", RUST)]));
    for x in [
        w.apply_profile("u").unwrap(),
        w.apply_flag("results.head_lines", toml::Value::Integer(5)),
        w.with_overrides(&Overrides { results: Some(false), ..Overrides::default() }).unwrap(),
    ] {
        assert!(x.config.profile.contains_key("rust"));
        assert!(x.config.loadout.contains_key(REPOSITORY_LOADOUT));
        assert_eq!(x.show_workspace_files(), w.show_workspace_files());
    }
}

/// R-C8: `load` does not read `.zygo/`.
#[test]
fn load_does_not_read_zygo_dir() {
    let d = tmp("rc8");
    git(&d, &["init", "-b", "main"]);
    std::fs::create_dir_all(d.join(".zygo/profiles")).unwrap();
    std::fs::write(d.join(".zygo/workspace.toml"), "version = 1\n").unwrap();
    std::fs::write(d.join(".zygo/profiles/rust.toml"), RUST).unwrap();
    git(&d, &["add", "."]);
    git(&d, &["commit", "-m", "x"]);
    let mut reads = 0;
    let m = load_with_repo_reader(&ctx(None, Some(d.to_path_buf()), None), toml::Table::new(), |_| {
        reads += 1;
        RepoFile::Absent
    })
    .unwrap();
    assert_eq!(reads, 1);
    assert!(!m.config.profile.contains_key("rust"));
    assert!(!m.config.loadout.contains_key(REPOSITORY_LOADOUT));
    assert_eq!(m.show_workspace_files(), "");
    assert!(m.shadowed_profiles().is_empty());
}

// ---- review fixes ----

/// A second `with_definition` replaces the first: A then B is B alone.
#[test]
fn second_definition_replaces_first() {
    const OTHER: &str = "overrides = { review = \"other\" }\n";
    let (_c, m) = merged(None, None);
    let b = def(None, &[("rust", OTHER)]);
    let ab = m.with_definition(&def(Some("version = 1\n"), &[("rust", RUST)])).with_definition(&b);
    let only_b = m.with_definition(&b);
    assert!(ab.shadowed_profiles().is_empty(), "{:?}", ab.shadowed_profiles());
    assert_eq!(ab.config.profile["rust"].overrides["review"], "other");
    assert!(ab.config.profile["rust"].disabled.is_empty());
    assert!(!ab.config.loadout.contains_key(REPOSITORY_LOADOUT));
    assert_eq!(ab.definition_file_of("loadout.repository.window"), None);
    assert_eq!(
        ab.show_workspace_files(),
        "profile.rust.disabled = []  # workspace-files .zygo/profiles/rust.toml\nprofile.rust.overrides.review = \"other\"  # workspace-files .zygo/profiles/rust.toml\n"
    );
    assert_eq!(ab.show_workspace_files(), only_b.show_workspace_files());
    assert_eq!(ab.config, only_b.config);
    assert_eq!(ab.show(None, &|_| true), only_b.show(None, &|_| true));
    // And after a rebuild.
    let x = ab.apply_flag("results.head_lines", toml::Value::Integer(5));
    assert_eq!(x.config.profile["rust"].overrides["review"], "other");
    assert!(!x.config.loadout.contains_key(REPOSITORY_LOADOUT));
}

/// A flag key under a definition profile is labelled `flags`.
#[test]
fn flag_under_definition_profile_is_flags() {
    let w = with_ws_and_rust().apply_flag("profile.rust.prompt", toml::Value::String("x".into()));
    assert_eq!(w.config.profile["rust"].prompt.as_deref(), Some("x"));
    assert_eq!(w.definition_file_of("profile.rust.prompt"), None);
    assert_eq!(w.definition_file_of("profile.rust.overrides.review"), Some(".zygo/profiles/rust.toml"));
    assert_eq!(w.show(Some(Layer::Flags), &|_| true), "profile.rust.prompt = \"x\"  # flags\n");
    let all = w.show(None, &|_| true);
    assert!(all.contains("profile.rust.prompt = \"x\"  # flags\n"), "{all}");
    assert!(!w.show_workspace_files().contains("prompt"));
}

/// The file in the `workspace-files <file>` label is escaped like the key and the value.
#[test]
fn workspace_files_label_escaped() {
    let (_c, m) = merged(None, None);
    let out = m.with_definition(&def(None, &[("a\nb", RUST)])).show_workspace_files();
    assert_eq!(out.lines().count(), 2, "{out}");
    assert!(out.contains("  # workspace-files .zygo/profiles/a\\nb.toml\n"), "{out}");
}
