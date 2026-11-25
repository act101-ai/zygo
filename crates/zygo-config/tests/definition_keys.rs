use std::path::PathBuf;

use zygo_config::layers::load_with_repo_reader;
use zygo_config::*;
use zygo_core::paths::Paths;

const IGNORED: &str = ".zygo.toml: limits.max_members ignored; only user or team config may set it";
const LO: &str = "[loadout.pair]\n[[loadout.pair.window]]\n[[loadout.pair.window.pane]]\n";
const PANE: &str = "[[loadout.pair.window.pane]]\n";
const BAD_JOB: &str = "must be \"orchestrator\" or \"worker\"";

#[must_use = "the directory is removed when this value drops; bind it"]
struct Cx {
    _d: tempfile::TempDir,
    c: LoadContext,
}

fn ctx(user: &str, team: Option<&str>, repo: Option<PathBuf>) -> Cx {
    let d = tempfile::Builder::new().prefix("zygo-wd5-").tempdir().unwrap();
    let p = d.path();
    std::fs::write(p.join("config.toml"), user).unwrap();
    let paths = Paths {
        config_file: p.join("config.toml"),
        cache_dir: p.join("cache"),
        data_dir: p.join("data"),
        sessions_dir: p.join("cache/sessions"),
        marketplaces_dir: p.join("cache/marketplaces"),
        plugin_store: p.join("data/plugins"),
        approvals_file: p.join("data/approvals.toml"),
        usage_db: p.join("data/usage.db"),
    };
    Cx { c: LoadContext { paths, repo, team: team.map(|t| toml::from_str(t).unwrap()) }, _d: d }
}

fn loaded(user: &str) -> Merged {
    load(&ctx(user, None, None).c).unwrap()
}

/// The one `config check` violation of `user`, as `<severity> <path>: <message>`.
fn one(user: &str) -> String {
    let v = check(&loaded(user).config);
    assert_eq!(v.len(), 1, "{v:?}");
    format!("{:?} {}", v[0].severity, v[0])
}

fn none(user: &str) {
    assert_eq!(check(&loaded(user).config), vec![]);
}

/// The merged config when the `.zygo.toml` read returns `repo` as found (no git call, no time bound).
fn with_repo_file(user: &str, team: Option<&str>, repo: &str) -> Merged {
    let cx = ctx(user, team, Some(PathBuf::from("repo")));
    let table: toml::Table = toml::from_str(repo).unwrap();
    let builtin = toml::Table::try_from(builtin::defaults()).unwrap();
    load_with_repo_reader(&cx.c, builtin, |_| RepoFile::Found { branch: "main".into(), table, ignored: vec![] }).unwrap()
}

#[test]
fn constants_and_defaults() { // R§9.3, R§9.4
    assert_eq!(JOBS, ["orchestrator", "worker"]);
    assert_eq!(MAX_MEMBERS_DEFAULT, 8);
    assert_eq!(LimitsConfig::default(), LimitsConfig { subagent_max_turns: 0, max_members: 8 });
    assert_eq!(WorkspaceConfig::default().pool, None);
    let p = LoadoutPane::default();
    assert_eq!((p.job, p.pool), (None, false));
    let m = loaded("");
    assert_eq!((m.config.limits.max_members, m.source_of("limits.max_members")), (8, Layer::Builtin));
    assert_eq!(check(&m.config), vec![]);
}

#[test]
fn keys_parse_from_user_config() { // R§13
    let user = format!(
        "[workspace]\npool = \"~/.claude-interns\"\n[limits]\nmax_members = 12\n{LO}kind = \"claude-code\"\nname = \"elena\"\njob = \"worker\"\npool = true\n"
    );
    let m = loaded(&user);
    assert_eq!(m.warnings, Vec::<String>::new());
    assert_eq!(m.config.workspace.pool.as_deref(), Some("~/.claude-interns"));
    assert_eq!((m.config.limits.max_members, m.source_of("limits.max_members")), (12, Layer::User));
    let p = &m.config.loadout["pair"].window[0].pane[0];
    assert_eq!((p.name.as_deref(), p.job.as_deref(), p.pool), (Some("elena"), Some("worker"), true));
    assert_eq!(check(&m.config), vec![]);
}

#[test]
fn user_pane_without_job_has_no_job() { // R§9.3 (lead: ruling 9)
    let m = loaded(&format!("{LO}kind = \"claude-code\"\nname = \"elena\"\n"));
    let p = &m.config.loadout["pair"].window[0].pane[0];
    assert_eq!((p.job.as_deref(), p.pool), (None, false));
    assert_eq!(check(&m.config), vec![]);
}

#[test]
fn check_job_is_orchestrator_or_worker() { // R§9.1
    let pane = |job: &str| format!("{LO}kind = \"claude-code\"\njob = \"{job}\"\n");
    none(&pane("orchestrator"));
    none(&pane("worker"));
    for bad in ["lead", "Worker", ""] {
        assert_eq!(one(&pane(bad)), format!("Error loadout.pair.window[0].pane[0].job: {BAD_JOB}"), "{bad:?}");
    }
}

#[test]
fn check_one_orchestrator_per_loadout() { // R§9.1
    let orch = "kind = \"claude-code\"\njob = \"orchestrator\"\n";
    let two = format!("{LO}{orch}{PANE}kind = \"claude-code\"\njob = \"worker\"\n{PANE}{orch}");
    assert_eq!(one(&two), "Error loadout.pair.window[0].pane[2].job: only one orchestrator per loadout");
    let two_windows = format!("{LO}{orch}[[loadout.pair.window]]\n{PANE}{orch}");
    assert_eq!(one(&two_windows), "Error loadout.pair.window[1].pane[0].job: only one orchestrator per loadout");
    none(&format!("{LO}{orch}[loadout.other]\n[[loadout.other.window]]\n[[loadout.other.window.pane]]\n{orch}"));
}

#[test]
fn check_pool_rules() { // R§9.1, R§9.5
    none(&format!("{LO}kind = \"claude-code\"\npool = true\n"));
    none(&format!("{LO}kind = \"claude-code\"\npool = false\nconfig_dir = \"~/.claude-work\"\n"));
    assert_eq!(
        one(&format!("{LO}kind = \"opencode\"\npool = true\n")),
        "Error loadout.pair.window[0].pane[0].pool: only with kind = \"claude-code\""
    );
    assert_eq!(
        one(&format!("{LO}kind = \"claude-code\"\npool = true\nconfig_dir = \"~/.claude-work\"\n")),
        "Error loadout.pair.window[0].pane[0].pool: not with config_dir"
    );
}

#[test]
fn check_workspace_pool_absolute_after_tilde() { // R§9.5
    let w = |p: &str| format!("[workspace]\npool = '{p}'\n");
    for ok in ["~", "~/.claude-interns"] {
        none(&w(ok));
    }
    none(&w(&std::env::temp_dir().display().to_string()));
    for bad in ["interns", "./interns", "~interns", ""] {
        assert_eq!(one(&w(bad)), "Error workspace.pool: must be absolute after ~ expansion", "{bad:?}");
    }
}

#[test]
fn check_max_members_is_1_to_32() { // R§9.4
    for ok in [1, 8, 32] {
        none(&format!("[limits]\nmax_members = {ok}\n"));
    }
    for bad in [0, 33] {
        assert_eq!(one(&format!("[limits]\nmax_members = {bad}\n")), "Error limits.max_members: must be 1-32", "{bad}");
    }
}

#[test]
fn repo_file_cannot_set_max_members() { // R§9.4, R§18 item 12 (.zygo.toml part)
    let repo = "version = 1\n[limits]\nmax_members = 30\nsubagent_max_turns = 12\n";
    let m = with_repo_file("[limits]\nmax_members = 4\n", None, repo);
    assert_eq!(m.warnings, [IGNORED]);
    assert_eq!((m.config.limits.max_members, m.source_of("limits.max_members")), (4, Layer::User));
    assert_eq!((m.config.limits.subagent_max_turns, m.source_of("limits.subagent_max_turns")), (12, Layer::Repository));
    let m = with_repo_file("", None, repo);
    assert_eq!(m.warnings, [IGNORED]);
    assert_eq!((m.config.limits.max_members, m.source_of("limits.max_members")), (8, Layer::Builtin));
    assert_eq!(with_repo_file("", None, "[limits]\nsubagent_max_turns = 12\n").warnings, Vec::<String>::new());
}

#[test]
fn team_layer_sets_max_members() { // R§9.4
    let m = load(&ctx("", Some("[limits]\nmax_members = 12\n"), None).c).unwrap();
    assert_eq!((m.config.limits.max_members, m.source_of("limits.max_members")), (12, Layer::Team));
    assert_eq!(m.warnings, Vec::<String>::new());
    assert!(m.team_keys.contains("limits.max_members"));
    let m = with_repo_file("", Some("[limits]\nmax_members = 12\n"), "[limits]\nmax_members = 30\n");
    assert_eq!(m.config.limits.max_members, 12);
    assert_eq!(m.warnings, [IGNORED]);
}
