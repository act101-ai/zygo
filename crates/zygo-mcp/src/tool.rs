use std::fmt;
use std::io::Write;
use std::path::PathBuf;

use serde::Serialize;
use zygo_config::Features;
use zygo_core::overrides::Overrides;
use zygo_core::paths::Paths;
use zygo_core::session_dir::SessionDir;
use zygo_core::session_view::SessionView;
use zygo_core::Agent;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: serde_json::Value,
}

#[derive(Debug, PartialEq)]
pub enum ToolError {
    InvalidParams(String),
    Failed(String),
}

impl fmt::Display for ToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ToolError::InvalidParams(m) | ToolError::Failed(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for ToolError {}

impl From<anyhow::Error> for ToolError {
    fn from(e: anyhow::Error) -> Self {
        ToolError::Failed(format!("{e:#}"))
    }
}

pub trait Tool: Send + Sync {
    fn def(&self) -> ToolDef;
    fn call(&self, args: serde_json::Value) -> Result<serde_json::Value, ToolError>;
}

#[derive(Debug, Clone)]
pub struct McpCtx {
    pub agent: Agent,
    pub features: Features,
    pub session_dir: Option<SessionDir>,
}

impl McpCtx {
    pub fn resolve(agent: Agent, paths: &Paths, repo: Option<PathBuf>, session_dir: Option<SessionDir>) -> anyhow::Result<McpCtx> {
        Self::resolve_with(agent, paths, repo, session_dir, &mut std::io::stderr())
    }
    pub fn resolve_with(agent: Agent, paths: &Paths, repo: Option<PathBuf>, session_dir: Option<SessionDir>, warn: &mut dyn Write) -> anyhow::Result<McpCtx> {
        let merged = zygo_config::load(&zygo_config::LoadContext { paths: paths.clone(), repo, team: None })?;
        let mut features = merged.config.features.clone();
        if let Some(d) = &session_dir {
            match SessionView::load(d) {
                Ok(v) => {
                    features.results = v.results;
                    features.compact_tool = v.compact_tool;
                    features.work_units = v.work_units;
                }
                Err(_) if !d.view_json().exists() => {}
                Err(_) => {
                    let _ = writeln!(warn, "zygo mcp: the session view cannot be read; using the config values");
                }
            }
            let o = Overrides::load(d);
            if let Some(r) = o.results {
                features.results = r;
            }
            if let Some(w) = o.work_units {
                features.work_units = w;
            }
        }
        Ok(McpCtx { agent, features, session_dir })
    }
}
