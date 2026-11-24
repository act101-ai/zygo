//! Repository policy: guard levels and the subagent turn cap (§13.4, §13.5).

use std::path::{Path, PathBuf};

use zygo_config::{GuardsConfig, Level, Merged, RepoFile};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Guard {
    Claims,
    Scope,
    Navigation,
    Output,
    Trailers,
    Repeat,
    /// D§9.3: checked in work_verdict; default enforce.
    Dispositions,
    Lead,
    Redaction,
    /// FIX-21: Claude attribution in a commit, tag, pull request, issue or release; fixed `enforce`, no `[guards]` key.
    Attribution,
}

impl Guard {
    /// The guards set in `[guards]`, in `zygo_config::GUARDS` order.
    pub const CONFIGURED: [Guard; 7] = [
        Guard::Claims,
        Guard::Scope,
        Guard::Navigation,
        Guard::Output,
        Guard::Trailers,
        Guard::Repeat,
        Guard::Dispositions,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Guard::Claims => "claims",
            Guard::Scope => "scope",
            Guard::Navigation => "navigation",
            Guard::Output => "output",
            Guard::Trailers => "trailers",
            Guard::Repeat => "repeat",
            Guard::Dispositions => "dispositions",
            Guard::Lead => "lead",
            Guard::Redaction => "redaction",
            Guard::Attribution => "attribution",
        }
    }
}

#[derive(Debug, Clone)]
enum Status {
    Defaults,
    Repo { root: PathBuf, branch: String },
    Invalid(String),
}

#[derive(Debug, Clone)]
pub struct Policy {
    guards: GuardsConfig,
    status: Status,
    cap: u32,
}

impl Policy {
    /// `cfg` must come from `zygo_config::load` with `LoadContext.repo = Some(root)`: `load` reads `.zygo.toml` from the
    /// default branch and merges it; `Merged.repo_file` and the levels and cap in `cfg.config` are only meaningful then.
    /// `repo` must be that same `root`; it is only printed by `describe`.
    pub fn resolve(cfg: &Merged, repo: &Path) -> Policy {
        let status = match &cfg.repo_file {
            RepoFile::Absent | RepoFile::Unread { .. } => Status::Defaults,
            RepoFile::Invalid { why, .. } => Status::Invalid(why.clone()),
            RepoFile::Found { branch, .. } => Status::Repo { root: repo.to_path_buf(), branch: branch.clone() },
        };
        Policy { guards: cfg.config.guards.clone(), status, cap: cfg.config.limits.subagent_max_turns }
    }

    pub fn level(&self, g: Guard) -> Level {
        match g {
            Guard::Lead | Guard::Redaction | Guard::Attribution => Level::Enforce,
            Guard::Repeat => Level::Monitor,
            _ => self.guards.get(g.name()).unwrap_or_default(),
        }
    }

    /// The subagent turn cap; 0 in the config means no cap.
    pub fn turn_cap(&self) -> Option<u32> {
        (self.cap != 0).then_some(self.cap)
    }

    /// The whole line, `repo policy: ` included.
    pub fn describe(&self) -> String {
        let levels = Guard::CONFIGURED
            .iter()
            .map(|g| format!("{} {}", g.name(), self.level(*g)))
            .collect::<Vec<_>>()
            .join(", ");
        match &self.status {
            Status::Defaults => "repo policy: defaults".to_string(),
            Status::Repo { root, branch } => format!("repo policy: {}@{branch} ({levels})", root.display()),
            // `why` can quote the repo file; a control character (e.g. a terminal escape) is shown as `?`
            Status::Invalid(why) => {
                format!("repo policy: invalid ({})", why.chars().map(|c| if c.is_control() { '?' } else { c }).collect::<String>())
            }
        }
    }
}
