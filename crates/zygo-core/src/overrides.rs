use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::session_dir::{write_atomic, SessionDir};

/// Session overrides (layer 7); every field optional.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Overrides {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub results: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub results_threshold: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redaction_content: Option<bool>,
    pub guards: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub work_units: Option<bool>,
}

impl Overrides {
    /// A missing or invalid file gives the default.
    pub fn load(dir: &SessionDir) -> Overrides {
        std::fs::read(dir.overrides_json())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    /// Unique temp file, then rename.
    pub fn save(&self, dir: &SessionDir) -> anyhow::Result<()> {
        write_atomic(&dir.overrides_json(), &serde_json::to_vec(self)?)
    }
}
