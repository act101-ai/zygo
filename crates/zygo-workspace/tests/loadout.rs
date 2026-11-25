use std::path::PathBuf;
use zygo_config::{Config, Loadout, LoadoutPane, LoadoutWindow, WorkspaceConfig, BUILTIN_LOADOUT, LOADOUT_KINDS};
use zygo_core::Agent;
use zygo_workspace::identity::Worktree;
use zygo_workspace::loadout::*;

fn root() -> PathBuf { std::env::temp_dir().join("w5-main") }
fn pane(kind: &str) -> LoadoutPane { LoadoutPane { kind: kind.into(), ..LoadoutPane::default() } }
fn cwd(kind: &str, cwd: &str) -> LoadoutPane { LoadoutPane { cwd: Some(cwd.into()), ..pane(kind) } }
fn window(panes: Vec<LoadoutPane>) -> LoadoutWindow { LoadoutWindow { name: None, layout: None, pane: panes } }
fn cfg(default: &str, loadouts: Vec<(&str, Vec<LoadoutWindow>)>) -> Config {
    Config {
        workspace: WorkspaceConfig { default_loadout: default.into(), ..WorkspaceConfig::default() },
        loadout: loadouts.into_iter().map(|(n, w)| (n.to_string(), Loadout { window: w })).collect(),
        ..Config::default()
    }
}
fn plain(kind: PaneKind) -> PlanPane { // the defaults of `zygo claude` and `zygo opencode` (AU45)
    PlanPane { kind, name: None, cwd: root(), profile: None, args: vec![], yolo: false, passthrough: false, server: None, compact: None, config_dir: None }
}
fn trees() -> Vec<Worktree> {
    vec![
        Worktree { path: root(), branch: Some("main".into()), bare: false },
        Worktree { path: root().join("wt-x"), branch: Some("feature-x".into()), bare: false },
    ]
}

#[test]
fn builtin_solo_is_one_claude_code_pane_in_the_main_worktree() {
    assert_eq!(BUILTIN_LOADOUT, "solo");
    assert_eq!(builtin_solo(), Loadout { window: vec![window(vec![pane("claude-code")])] });
    let solo = Plan {
        loadout: "solo".into(),
        windows: vec![PlanWindow { name: None, layout: None, panes: vec![plain(PaneKind::Agent(Agent::ClaudeCode))] }],
        warnings: vec![],
    };
    assert_eq!(resolve(&Config::default(), None, &root(), &[]), Ok(solo.clone()));
    assert_eq!(resolve(&cfg("pair", vec![("pair", vec![window(vec![pane("shell")])])]), Some("solo"), &root(), &trees()), Ok(solo));
    // a user [loadout.solo] replaces the built-in one
    let p = resolve(&cfg("solo", vec![("solo", vec![window(vec![pane("shell")])])]), None, &root(), &[]).unwrap();
    assert_eq!(p.windows[0].panes, [plain(PaneKind::Shell)]);
}

#[test]
fn unset_pane_fields_take_the_launch_defaults_and_set_fields_are_kept() {
    let set = LoadoutPane {
        kind: "claude-code".into(), name: None, profile: Some("cheap".into()), args: vec!["--model".into(), "lead".into()], yolo: true,
        passthrough: true, server: Some("https://zygo.example".into()), compact: Some("600k".into()),
        config_dir: Some("~/.claude-work".into()), cwd: None,
    };
    let w = LoadoutWindow { name: Some("agents".into()), layout: Some("main-vertical".into()), pane: vec![pane("claude-code"), pane("opencode"), pane("shell"), set] };
    let p = resolve(&cfg("pair", vec![("pair", vec![w])]), None, &root(), &[]).unwrap();
    assert_eq!((p.loadout.as_str(), p.windows.len(), &p.warnings), ("pair", 1, &vec![]));
    assert_eq!((p.windows[0].name.as_deref(), p.windows[0].layout.as_deref()), (Some("agents"), Some("main-vertical")));
    assert_eq!(p.windows[0].panes[..3], [plain(PaneKind::Agent(Agent::ClaudeCode)), plain(PaneKind::Agent(Agent::OpenCode)), plain(PaneKind::Shell)]);
    assert_eq!(p.windows[0].panes[3], PlanPane {
        kind: PaneKind::Agent(Agent::ClaudeCode), name: None, cwd: root(), profile: Some("cheap".into()), args: vec!["--model".into(), "lead".into()],
        yolo: true, passthrough: true, server: Some("https://zygo.example".into()), compact: Some("600k".into()), config_dir: Some("~/.claude-work".into()),
    });
    for k in LOADOUT_KINDS {
        assert!(resolve(&cfg("x", vec![("x", vec![window(vec![pane(k)])])]), None, &root(), &[]).is_ok(), "{k}");
    }
}

#[test] // M§6, AU84: resolve copies a valid pane name; a name that fails the grammar is not copied
fn member_name_resolve_copies_the_pane_name() {
    let named = |kind: &str, n: &str| LoadoutPane { name: Some(n.into()), ..pane(kind) };
    let w = window(vec![named("claude-code", "lead"), pane("opencode"), named("opencode", "Bad"), named("claude-code", "all"), named("claude-code", "kabcdefgh")]);
    let p = resolve(&cfg("team", vec![("team", vec![w])]), None, &root(), &[]).unwrap();
    let names: Vec<Option<&str>> = p.windows[0].panes.iter().map(|pp| pp.name.as_deref()).collect();
    assert_eq!(names, [Some("lead"), None, None, None, None]);
    assert_eq!(p.windows[0].panes[0], PlanPane { name: Some("lead".into()), ..plain(PaneKind::Agent(Agent::ClaudeCode)) });
}

#[test] // M§6, ML-3 review round 1: a name that an earlier pane of the same plan took reads as no name (the first wins)
fn member_name_resolve_first_pane_wins_a_repeated_name() {
    let named = |kind: &str, n: &str| LoadoutPane { name: Some(n.into()), ..pane(kind) };
    let w1 = window(vec![named("claude-code", "lead"), named("opencode", "lead"), named("opencode", "dev")]);
    let w2 = window(vec![named("claude-code", "dev"), named("claude-code", "ops")]);
    let p = resolve(&cfg("team", vec![("team", vec![w1, w2])]), None, &root(), &[]).unwrap();
    let names: Vec<Vec<Option<&str>>> =
        p.windows.iter().map(|w| w.panes.iter().map(|pp| pp.name.as_deref()).collect()).collect();
    assert_eq!(names, [vec![Some("lead"), None, Some("dev")], vec![None, Some("ops")]]);
}

#[test]
fn cwd_is_absolute_relative_or_a_branch_worktree() {
    let abs = root().join("elsewhere");
    let panes = vec![cwd("shell", abs.to_str().unwrap()), cwd("shell", "sub/dir"), cwd("shell", "branch:feature-x"), cwd("claude-code", "branch:main")];
    let p = resolve(&cfg("pair", vec![("pair", vec![window(panes)])]), None, &root(), &trees()).unwrap();
    let got: Vec<&PathBuf> = p.windows[0].panes.iter().map(|x| &x.cwd).collect();
    assert_eq!(got, [&abs, &root().join("sub/dir"), &root().join("wt-x"), &root()]);
    assert_eq!(p.warnings, Vec::<String>::new());
}

#[test]
fn branch_without_a_worktree_gives_the_main_worktree_and_one_warning() {
    let panes = vec![cwd("shell", "branch:gone"), cwd("claude-code", "branch:gone"), cwd("shell", "branch:feature-x"), cwd("shell", "branch:other")];
    let p = resolve(&cfg("pair", vec![("pair", vec![window(panes)])]), None, &root(), &trees()).unwrap();
    let got: Vec<&PathBuf> = p.windows[0].panes.iter().map(|x| &x.cwd).collect();
    assert_eq!(got, [&root(), &root(), &root().join("wt-x"), &root()]);
    assert_eq!(p.warnings, [
        "loadout pair: no worktree has branch `gone` checked out; using the main worktree",
        "loadout pair: no worktree has branch `other` checked out; using the main worktree",
    ]);
    // no worktree list at all
    let p = resolve(&cfg("pair", vec![("pair", vec![window(vec![cwd("shell", "branch:main")])])]), None, &root(), &[]).unwrap();
    assert_eq!((&p.windows[0].panes[0].cwd, p.warnings.len()), (&root(), 1));
}

#[test]
fn errors_name_the_loadout_and_the_place() {
    let c = cfg("nope", vec![
        ("empty", vec![]),
        ("hole", vec![window(vec![pane("shell")]), window(vec![])]),
        ("vim", vec![window(vec![pane("shell"), pane("vim")])]),
    ]);
    let r = |name| resolve(&c, name, &root(), &[]).unwrap_err();
    assert_eq!(r(None), LoadoutError::Unknown { name: "nope".into() });
    assert_eq!(r(Some("x")), LoadoutError::Unknown { name: "x".into() });
    assert_eq!(r(Some("empty")), LoadoutError::NoWindow { name: "empty".into() });
    assert_eq!(r(Some("hole")), LoadoutError::NoPane { name: "hole".into(), window: 1 });
    assert_eq!(r(Some("vim")), LoadoutError::Kind { name: "vim".into(), window: 0, pane: 1 });
    assert_eq!(r(Some("x")).to_string(), "unknown loadout `x`");
    assert_eq!(r(Some("empty")).to_string(), "loadout empty: must have at least one window");
    assert_eq!(r(Some("hole")).to_string(), "loadout hole: window[1] must have at least one pane");
    assert_eq!(r(Some("vim")).to_string(), "loadout vim: window[0].pane[1].kind must be \"claude-code\", \"opencode\" or \"shell\"");
    assert_eq!(LoadoutError::Unknown { name: "a\nb".into() }.to_string(), "unknown loadout `a\\nb`");
}

#[test]
fn list_gives_each_name_with_its_source_in_name_order() {
    let s = |n: &str, src| (n.to_string(), src);
    assert_eq!(list(&Config::default()), [s("solo", Source::Builtin)]);
    let w = || vec![window(vec![pane("shell")])];
    assert_eq!(list(&cfg("solo", vec![("pair", w()), ("alpha", w()), ("zed", w())])),
        [s("alpha", Source::User), s("pair", Source::User), s("solo", Source::Builtin), s("zed", Source::User)]);
    assert_eq!(list(&cfg("solo", vec![("solo", w())])), [s("solo", Source::User)]);
}

#[test] // FIX-27 D6; kills: a `pane_cwd` that differs from `resolve`
fn pane_cwd_is_the_folder_rule_of_resolve() {
    assert_eq!(pane_cwd(None, &root(), &trees()), (root(), None));
    for t in trees() {
        let Some(b) = t.branch.as_deref() else { continue };
        assert_eq!(pane_cwd(Some(&format!("branch:{b}")), &root(), &trees()), (t.path.clone(), None));
    }
    assert_eq!(pane_cwd(Some("branch:nope"), &root(), &trees()), (root(), Some("nope".to_string())));
    let tmp = std::env::temp_dir();
    assert_eq!(pane_cwd(Some(tmp.to_str().unwrap()), &root(), &trees()), (tmp.clone(), None));
    assert_eq!(pane_cwd(Some("sub/dir"), &root(), &trees()), (root().join("sub/dir"), None));
}
