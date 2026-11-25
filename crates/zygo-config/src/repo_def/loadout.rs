//! The repository loadout from [members] (R§9.1, R§9.2, R§9.3 effect 1).

use std::collections::BTreeMap;

use super::{Definition, MemberDef};
use crate::{Loadout, LoadoutPane, LoadoutWindow};

/// The name of the loadout that the workspace definition gives.
pub const REPOSITORY_LOADOUT: &str = "repository";

/// The repository loadout: one unnamed window, one pane per member, or `None` with no `workspace.toml`.
pub fn repository_loadout(def: &Definition) -> Option<Loadout> {
    let ws = def.workspace.as_ref()?;
    let members = if ws.members.is_empty() { default_set() } else { ws.members.clone() };
    let mut pane: Vec<LoadoutPane> = members
        .into_iter()
        .map(|(name, m)| LoadoutPane {
            kind: m.kind.unwrap_or_else(|| "claude-code".into()),
            name: Some(name),
            job: Some(m.job.unwrap_or_else(|| "worker".into())),
            profile: m.profile,
            pool: m.pool,
            cwd: m.cwd,
            ..LoadoutPane::default()
        })
        .collect();
    // Stable: the orchestrator first, the others keep the byte order of the map.
    pane.sort_by_key(|p| p.job.as_deref() != Some("orchestrator"));
    let orchestrator = pane.first().is_some_and(|p| p.job.as_deref() == Some("orchestrator"));
    let layout = ws.layout.clone().unwrap_or_else(|| (if orchestrator { "main-vertical" } else { "tiled" }).into());
    Some(Loadout { window: vec![LoadoutWindow { name: None, layout: Some(layout), pane }] })
}

/// R§9.2: `orchestrator` and `worker-1` to `worker-4`, each a claude-code member with a pool directory.
fn default_set() -> BTreeMap<String, MemberDef> {
    let member = |job: &str| MemberDef {
        kind: Some("claude-code".into()),
        job: Some(job.into()),
        profile: None,
        pool: true,
        cwd: None,
    };
    let mut set = BTreeMap::from([("orchestrator".to_string(), member("orchestrator"))]);
    for i in 1..=4 {
        set.insert(format!("worker-{i}"), member("worker"));
    }
    set
}
