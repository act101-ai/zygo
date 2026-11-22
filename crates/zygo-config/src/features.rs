//! Feature toggles (§15.4). Every toggle defaults to `true`, but `workspace_tool` defaults to `false` (workspaces design AU65).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Features {
    pub results: bool,
    pub work_units: bool,
    pub compact_tool: bool,
    /// The MCP tool `workspace_down` (workspaces design AU65). Default `false`. A profile cannot set it: it is not in
    /// `profile::features_ok` or in the proxy's `profile_features`.
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
