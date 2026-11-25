//! WD-6: the repository loadout from [members], with the default set (R§9.1, R§9.2, R§9.3 effect 1, R§18 item 10).

use std::collections::BTreeMap;

use zygo_config::repo_def::{self, Definition, Files, REPOSITORY_LOADOUT};
use zygo_config::LoadoutPane;

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

fn window(ws: &str) -> zygo_config::LoadoutWindow {
    let l = repo_def::repository_loadout(&def(Some(ws), &[])).unwrap();
    assert_eq!(l.window.len(), 1);
    l.window.into_iter().next().unwrap()
}

fn names(w: &zygo_config::LoadoutWindow) -> Vec<&str> {
    w.pane.iter().map(|p| p.name.as_deref().unwrap()).collect()
}

fn agent(name: &str, job: &str, pool: bool) -> LoadoutPane {
    LoadoutPane {
        kind: "claude-code".into(),
        name: Some(name.into()),
        job: Some(job.into()),
        pool,
        ..LoadoutPane::default()
    }
}

#[test]
fn name_is_repository() {
    assert_eq!(REPOSITORY_LOADOUT, "repository");
}

#[test]
fn default_set_from_version_alone() {
    for text in ["version = 1\n", "version = 1\n[members]\n"] {
        let w = window(text);
        assert_eq!(w.name, None);
        assert_eq!(w.layout.as_deref(), Some("main-vertical"));
        assert_eq!(
            w.pane,
            vec![
                agent("orchestrator", "orchestrator", true),
                agent("worker-1", "worker", true),
                agent("worker-2", "worker", true),
                agent("worker-3", "worker", true),
                agent("worker-4", "worker", true),
            ]
        );
    }
}

#[test]
fn orchestrator_first_then_byte_order() {
    let w =
        window("version = 1\n[members.b]\n[members.a]\n[members.boss]\njob = \"orchestrator\"\n");
    assert_eq!(names(&w), ["boss", "a", "b"]);
    assert_eq!(w.layout.as_deref(), Some("main-vertical"));
    assert_eq!(w.name, None);
}

#[test]
fn no_orchestrator_gives_tiled() {
    let w = window("version = 1\n[members.b]\n[members.a]\n");
    assert_eq!(names(&w), ["a", "b"]);
    assert_eq!(w.layout.as_deref(), Some("tiled"));
}

#[test]
fn set_layout_wins() {
    let w = window(
        "version = 1\nlayout = \"even-horizontal\"\n[members.boss]\njob = \"orchestrator\"\n",
    );
    assert_eq!(w.layout.as_deref(), Some("even-horizontal"));
    let w = window("version = 1\nlayout = \"main-horizontal\"\n[members.a]\n");
    assert_eq!(w.layout.as_deref(), Some("main-horizontal"));
    let w = window("version = 1\nlayout = \"tiled\"\n");
    assert_eq!(w.layout.as_deref(), Some("tiled"));
}

#[test]
fn member_without_job_is_worker() {
    let w = window("version = 1\n[members.a]\n");
    assert_eq!(w.pane[0].job.as_deref(), Some("worker"));
}

#[test]
fn member_fields_and_unset_defaults() {
    let w = window(
        "version = 1\n[members.a]\nkind = \"opencode\"\nprofile = \"rust\"\ncwd = \"branch:feature-x\"\n\
         [members.b]\npool = true\ncwd = \"src/app\"\n",
    );
    assert_eq!(
        w.pane,
        vec![
            LoadoutPane {
                kind: "opencode".into(),
                name: Some("a".into()),
                job: Some("worker".into()),
                profile: Some("rust".into()),
                cwd: Some("branch:feature-x".into()),
                ..LoadoutPane::default()
            },
            LoadoutPane {
                kind: "claude-code".into(),
                name: Some("b".into()),
                job: Some("worker".into()),
                pool: true,
                cwd: Some("src/app".into()),
                ..LoadoutPane::default()
            },
        ]
    );
    for p in &w.pane {
        assert!(p.args.is_empty());
        assert!(!p.yolo);
        assert!(!p.passthrough);
        assert_eq!(p.server, None);
        assert_eq!(p.compact, None);
        assert_eq!(p.config_dir, None);
    }
}

#[test]
fn no_workspace_file_gives_none() {
    assert_eq!(
        repo_def::repository_loadout(&def(None, &[("rust", "disabled = [\"design\"]\n")])),
        None
    );
    assert_eq!(repo_def::repository_loadout(&def(None, &[])), None);
}
