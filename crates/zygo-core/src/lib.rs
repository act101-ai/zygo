use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

pub mod exit;
pub mod hook;
pub mod hook_opencode;
pub mod ids;
pub mod overrides;
pub mod paths;
pub mod session_dir;
pub mod session_view;
pub mod text;
pub mod tokens;
pub mod verbs;

pub use text::esc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Agent {
    #[serde(rename = "claude-code")]
    ClaudeCode,
    #[serde(rename = "opencode")]
    OpenCode,
}

impl Agent {
    pub fn as_str(&self) -> &'static str {
        match self {
            Agent::ClaudeCode => "claude-code",
            Agent::OpenCode => "opencode",
        }
    }
}

impl fmt::Display for Agent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Agent {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> anyhow::Result<Self> {
        match s {
            "claude-code" => Ok(Agent::ClaudeCode),
            "opencode" => Ok(Agent::OpenCode),
            _ => anyhow::bail!("unknown agent `{s}` (claude-code or opencode)"),
        }
    }
}
