use serde::{Deserialize, Serialize};

use crate::session_dir::{write_atomic, SessionDir};
use crate::Agent;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionView {
    pub session: String,
    pub agent: Agent,
    pub main_role: String,
    pub effective_role: Option<String>,
    pub role_prompt: Option<String>,
    pub user_prompt: Option<String>,
    pub profile: Option<String>,
    pub profile_prompt: Option<String>,
    pub work_item: Option<WorkItemView>,
    pub legend: Vec<LegendEntry>,
    pub disabled_roles: Vec<String>,
    pub work_units: bool,
    pub results: bool,
    #[serde(default = "default_true")]
    pub compact_tool: bool,
    pub navigation: bool,
    #[serde(default = "default_trim_threshold")]
    pub trim_threshold_bytes: u64,
    #[serde(default = "default_head_lines")]
    pub head_lines: u32,
    #[serde(default = "default_tail_lines")]
    pub tail_lines: u32,
    /// The status line style the launcher took from the user config (`statusline.style`):
    /// `compact` or `verbose`. A `view.json` without the field reads as `compact`.
    #[serde(default = "default_statusline_style")]
    pub statusline_style: String,
}

/// Each field that has a serde default holds that default; `agent` is Claude Code; every other field is empty or `false`.
impl Default for SessionView {
    fn default() -> Self {
        SessionView {
            session: String::new(),
            agent: Agent::ClaudeCode,
            main_role: String::new(),
            effective_role: None,
            role_prompt: None,
            user_prompt: None,
            profile: None,
            profile_prompt: None,
            work_item: None,
            legend: Vec::new(),
            disabled_roles: Vec::new(),
            work_units: false,
            results: false,
            compact_tool: default_true(),
            navigation: false,
            trim_threshold_bytes: default_trim_threshold(),
            head_lines: default_head_lines(),
            tail_lines: default_tail_lines(),
            statusline_style: default_statusline_style(),
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_trim_threshold() -> u64 {
    16384
}

fn default_head_lines() -> u32 {
    60
}

fn default_tail_lines() -> u32 {
    20
}

fn default_statusline_style() -> String {
    "compact".into()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkItemView {
    pub unit: String,
    pub title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LegendEntry {
    pub role: String,
    pub description: String,
    pub custom: bool,
}

impl SessionView {
    pub fn load(dir: &SessionDir) -> anyhow::Result<Self> {
        Ok(serde_json::from_slice(&std::fs::read(dir.view_json())?)?)
    }

    /// Unique temp file, then rename.
    pub fn save(&self, dir: &SessionDir) -> anyhow::Result<()> {
        write_atomic(&dir.view_json(), &serde_json::to_vec(self)?)
    }
}
