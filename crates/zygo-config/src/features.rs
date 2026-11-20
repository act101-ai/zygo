//! Feature toggles (§15.4); every toggle defaults to `true`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Features {
    pub results: bool,
    pub work_units: bool,
    pub compact_tool: bool,
    pub redaction: RedactionFeatures,
}

impl Default for Features {
    fn default() -> Self {
        Features { results: true, work_units: true, compact_tool: true, redaction: RedactionFeatures::default() }
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
