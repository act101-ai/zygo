//! Feature toggles (§15.4). Every toggle defaults to `true`, but `workspace_tool` defaults to `false` (workspaces design AU65).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Features {
    pub results: bool,
    pub work_units: bool,
    pub compact_tool: bool,
    /// The MCP tool `workspace_down` (workspaces design AU65). Default `false`. `profile::validate` refuses
    /// `workspace_tool` in a profile (`not a feature key`), and the MCP server applies no profile: it reads this value
    /// from the merged config only. `Merged::apply_profile` itself does not refuse the key.
    pub workspace_tool: bool,
    pub redaction: RedactionFeatures,
}

impl Default for Features {
    fn default() -> Self {
        Features {
            results: true,
            work_units: true,
            compact_tool: true,
            workspace_tool: false,
            redaction: RedactionFeatures::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RedactionFeatures {
    pub secrets: bool,
    pub content: bool,
}

impl Default for RedactionFeatures {
    fn default() -> Self {
        RedactionFeatures { secrets: true, content: true }
    }
}
