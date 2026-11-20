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
