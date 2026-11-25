//! Loadout resolution. Owner: node W5 (W§11).
//!
//! A loadout names the windows and panes that `zygo up` starts. `resolve` turns one loadout of the user config (or
//! the built-in `solo`) into a [`Plan`]: each pane has its kind, its directory and its launch fields. It reads no file
//! and starts no program.

use std::fmt;
use std::path::{Path, PathBuf};

use zygo_config::{BUILTIN_LOADOUT, Config, LOADOUT_KINDS, Loadout, LoadoutPane, LoadoutWindow};
use zygo_core::{Agent, esc};

use crate::identity::Worktree;

/// Where a loadout name comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The built-in `solo` ([`builtin_solo`]).
    Builtin,
    /// `[loadout.<name>]` of the user config.
    User,
}

/// What a pane runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneKind {
    /// `zygo claude` or `zygo opencode`.
    Agent(Agent),
    /// The user's shell.
    Shell,
}

/// One pane of a [`Plan`]. A field the loadout does not set has the default of `zygo claude` / `zygo opencode` (AU45).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanPane {
    pub kind: PaneKind,
    /// The member name (M§6): none for a shell pane (AU53); for an agent pane, only when zygo_core::ids::valid_member_name
    /// passes (AU84) and no earlier agent pane of the plan took it (the first wins; `zygo up` does not run `config check`).
    pub name: Option<String>,
    /// The pane's directory: the main worktree when the loadout sets no `cwd`.
    pub cwd: PathBuf,
    pub profile: Option<String>,
    pub args: Vec<String>,
    pub yolo: bool,
    pub passthrough: bool,
    pub server: Option<String>,
    pub compact: Option<String>,
    pub config_dir: Option<String>,
}

/// One window of a [`Plan`], with its panes in loadout order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanWindow {
    pub name: Option<String>,
    pub layout: Option<String>,
    pub panes: Vec<PlanPane>,
}

/// A resolved loadout (W§11).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// The name of the loadout.
    pub loadout: String,
    pub windows: Vec<PlanWindow>,
    /// One line for each different `branch:` that no worktree has checked out.
    pub warnings: Vec<String>,
}

/// Why a loadout does not resolve. `window` and `pane` are indexes in loadout order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadoutError {
    Unknown { name: String },
    NoWindow { name: String },
    NoPane { name: String, window: usize },
    Kind { name: String, window: usize, pane: usize },
}

impl fmt::Display for LoadoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadoutError::Unknown { name } => write!(f, "unknown loadout `{}`", esc(name)),
            LoadoutError::NoWindow { name } => write!(f, "loadout {}: must have at least one window", esc(name)),
            LoadoutError::NoPane { name, window } => {
                write!(f, "loadout {}: window[{window}] must have at least one pane", esc(name))
            }
            LoadoutError::Kind { name, window, pane } => {
                let [claude_code, opencode, shell] = LOADOUT_KINDS;
                write!(
                    f,
                    "loadout {}: window[{window}].pane[{pane}].kind must be \"{claude_code}\", \"{opencode}\" or \"{shell}\"",
                    esc(name)
                )
            }
        }
    }
}

impl std::error::Error for LoadoutError {}

/// The built-in loadout `solo` (W§11, AU45): one window with one `claude-code` pane in the main worktree.
pub fn builtin_solo() -> Loadout {
    let pane = LoadoutPane { kind: "claude-code".into(), ..LoadoutPane::default() };
    Loadout { window: vec![LoadoutWindow { name: None, layout: None, pane: vec![pane] }] }
}

/// Each loadout name with its source, in name order. A user `[loadout.solo]` replaces the built-in one.
pub fn list(cfg: &Config) -> Vec<(String, Source)> {
    let mut all: Vec<(String, Source)> = cfg.loadout.keys().map(|k| (k.clone(), Source::User)).collect();
    if !cfg.loadout.contains_key(BUILTIN_LOADOUT) {
        all.push((BUILTIN_LOADOUT.to_string(), Source::Builtin));
    }
    all.sort_by(|a, b| a.0.cmp(&b.0));
    all
}

/// The folder of a pane whose `cwd` is `cwd` (W§11; FIX-27 D6): `None` gives `main_root`; `branch:<b>` gives the path of
/// the worktree that has `<b>` checked out, else `main_root` and `Some(<b>)`; an absolute text gives itself; a relative
/// text gives `main_root.join(text)`. `~` is not expanded and no folder is checked: the function does no I/O.
pub fn pane_cwd(cwd: Option<&str>, main_root: &Path, worktrees: &[Worktree]) -> (PathBuf, Option<String>) {
    match cwd {
        None => (main_root.to_path_buf(), None),
        Some(text) => match text.strip_prefix("branch:") {
            Some(branch) => match worktrees.iter().find(|t| t.branch.as_deref() == Some(branch)) {
                Some(tree) => (tree.path.clone(), None),
                None => (main_root.to_path_buf(), Some(branch.to_string())),
            },
            None if Path::new(text).is_absolute() => (PathBuf::from(text), None),
            None => (main_root.join(text), None),
        },
    }
}

/// The plan of loadout `name`, or of `workspace.default_loadout` when `name` is `None` (W§11, AU10).
///
/// `main_root` is the main worktree; `worktrees` is the repository's worktree list, for `cwd = "branch:<name>"`. A
/// branch that no worktree has checked out gives the main worktree and one warning. `~` is not expanded and no
/// directory is checked: the function does no I/O.
pub fn resolve(cfg: &Config, name: Option<&str>, main_root: &Path, worktrees: &[Worktree]) -> Result<Plan, LoadoutError> {
    let name = name.unwrap_or(&cfg.workspace.default_loadout);
    let builtin;
    let loadout = match cfg.loadout.get(name) {
        Some(l) => l,
        None if name == BUILTIN_LOADOUT => {
            builtin = builtin_solo();
            &builtin
        }
        None => return Err(LoadoutError::Unknown { name: name.to_string() }),
    };
    if loadout.window.is_empty() {
        return Err(LoadoutError::NoWindow { name: name.to_string() });
    }
    let mut warnings: Vec<String> = Vec::new();
    let mut windows = Vec::with_capacity(loadout.window.len());
    let mut names: Vec<String> = Vec::new();
    for (wi, w) in loadout.window.iter().enumerate() {
        if w.pane.is_empty() {
            return Err(LoadoutError::NoPane { name: name.to_string(), window: wi });
        }
        let mut panes = Vec::with_capacity(w.pane.len());
        for (pi, p) in w.pane.iter().enumerate() {
            let kind = match p.kind.as_str() {
                "shell" => PaneKind::Shell,
                other => match other.parse::<Agent>() {
                    Ok(agent) => PaneKind::Agent(agent),
                    Err(_) => return Err(LoadoutError::Kind { name: name.to_string(), window: wi, pane: pi }),
                },
            };
            let (cwd, missing) = pane_cwd(p.cwd.as_deref(), main_root, worktrees);
            if let Some(branch) = missing {
                let warning = format!(
                    "loadout {}: no worktree has branch `{}` checked out; using the main worktree",
                    esc(name),
                    esc(&branch)
                );
                if !warnings.contains(&warning) {
                    warnings.push(warning);
                }
            }
            // M§6, AU53: only an agent pane has a name; a shell pane's name is dropped and takes no name from a later pane.
            let name = p.name.clone().filter(|n| {
                matches!(kind, PaneKind::Agent(_)) && zygo_core::ids::valid_member_name(n) && !names.contains(n)
            });
            names.extend(name.clone());
            panes.push(PlanPane {
                kind,
                name,
                cwd,
                profile: p.profile.clone(),
                args: p.args.clone(),
                yolo: p.yolo,
                passthrough: p.passthrough,
                server: p.server.clone(),
                compact: p.compact.clone(),
                config_dir: p.config_dir.clone(),
            });
        }
        windows.push(PlanWindow { name: w.name.clone(), layout: w.layout.clone(), panes });
    }
    Ok(Plan { loadout: name.to_string(), windows, warnings })
}
