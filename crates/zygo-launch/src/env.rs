//! The child environment (§8.4): the denylist, then the variables `zygo` sets.

use std::ffi::{OsStr, OsString};

use zygo_config::{KeyRef, Merged};

/// Name and value rows. No `Debug`: values may be secrets.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Env(pub Vec<(OsString, OsString)>);

impl Env {
    pub fn from_process() -> Env {
        Env(std::env::vars_os().collect())
    }

    /// The first row named `name` (ASCII case fold on Windows).
    pub fn get(&self, name: &str) -> Option<&OsStr> {
        self.0.iter().find(|(k, _)| same(k, name, cfg!(windows))).map(|(_, v)| v.as_os_str())
    }
}

const PREFIXES: [&str; 3] = ["ANTHROPIC_", "OPENAI_", "OPENROUTER_"];
const EXACT: [&str; 9] = [
    "CLAUDE_CODE_OAUTH_TOKEN",
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_USE_VERTEX",
    "CLAUDE_CODE_USE_FOUNDRY",
    "ZYGO_TEAM_TOKEN",
    "CLAUDECODE",
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_CODE_BRIDGE_SESSION_ID",
];
/// Set by a member launch (§8.4 "Set"); removed from every other child (workspaces design AU58).
const MEMBER: [&str; 2] = ["ZYGO_WORKSPACE", "ZYGO_PANE"];

fn same(name: &OsStr, other: &str, fold: bool) -> bool {
    match name.to_str() {
        Some(n) if fold => n.eq_ignore_ascii_case(other),
        Some(n) => n == other,
        None => false,
    }
}

/// The parent rows without the denied names and without the names of `set`, then `set`; the second value is the
/// denied names that are not in `set`, sorted (names only, for the session log).
pub fn child_env(parent: &Env, cfg: &Merged, set: &[(String, String)]) -> (Env, Vec<String>) {
    filter(parent, cfg, set, cfg!(windows))
}

/// [`child_env`] with the case rule given (`true` = Windows).
#[cfg(feature = "test-hooks")]
pub fn child_env_with(parent: &Env, cfg: &Merged, set: &[(String, String)], fold_case: bool) -> (Env, Vec<String>) {
    filter(parent, cfg, set, fold_case)
}

fn filter(parent: &Env, cfg: &Merged, set: &[(String, String)], fold: bool) -> (Env, Vec<String>) {
    // Provider keys and the team token's variable (§8.4, R3).
    let providers: Vec<&str> = cfg
        .config
        .catalog
        .0
        .iter()
        .flat_map(|m| &m.provider)
        .filter_map(|p| p.key.as_ref())
        .chain(&cfg.config.server.token)
        .filter_map(|k| match k {
            KeyRef::Env(n) => Some(n.as_str()),
            KeyRef::Keychain(_) => None,
        })
        .collect();
    let denied = |k: &OsStr| {
        let Some(n) = k.to_str() else { return false };
        let up = if fold { n.to_ascii_uppercase() } else { n.to_string() };
        PREFIXES.iter().any(|p| up.starts_with(p))
            || EXACT.iter().chain(&MEMBER).chain(&providers).any(|e| same(k, e, fold))
    };
    let mut out = Vec::with_capacity(parent.0.len() + set.len());
    let mut removed = Vec::new();
    for (k, v) in &parent.0 {
        let is_set = set.iter().any(|(s, _)| same(k, s, fold));
        if denied(k) {
            if !is_set {
                removed.push(k.to_string_lossy().into_owned());
            }
        } else if !is_set {
            out.push((k.clone(), v.clone()));
        }
    }
    out.extend(set.iter().map(|(k, v)| (OsString::from(k), OsString::from(v))));
    removed.sort();
    (Env(out), removed)
}
