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
    /// The session's status line style. A value other than `compact` or `verbose` is ignored by its reader.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub statusline: Option<String>,
}

impl Overrides {
    /// A missing or invalid file gives the default.
    pub fn load(dir: &SessionDir) -> Overrides {
        Overrides::parse(std::fs::read(dir.overrides_json()).ok().as_deref())
    }

    /// The overrides of the bytes of `overrides.json`, by the rule of `load`: no bytes (no file) or bytes that are not
    /// valid overrides JSON give the default. For a caller that has read the file already (M4 review round 3).
    pub fn parse(bytes: Option<&[u8]>) -> Overrides {
        bytes.and_then(|b| serde_json::from_slice(b).ok()).unwrap_or_default()
    }

    /// Unique temp file, then rename.
    pub fn save(&self, dir: &SessionDir) -> anyhow::Result<()> {
        write_atomic(&dir.overrides_json(), &serde_json::to_vec(self)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// M4 review round 3: `parse` is the rule of `load`: no file and invalid JSON give the default; valid JSON parses.
    #[test]
    fn parse_is_the_rule_of_load() {
        let d = tempfile::tempdir().unwrap();
        let sd = SessionDir::new(d.path().to_path_buf());
        assert_eq!(Overrides::load(&sd), Overrides::default());
        assert_eq!(Overrides::parse(None), Overrides::default());
        for bytes in [&b"{"[..], b"", b"[]", br#"{"results":"no"}"#] {
            std::fs::write(sd.overrides_json(), bytes).unwrap();
            assert_eq!(Overrides::load(&sd), Overrides::default(), "{bytes:?}");
            assert_eq!(Overrides::parse(Some(bytes)), Overrides::default(), "{bytes:?}");
        }
        let valid = br#"{"results":false,"guards":{"claims":"enforce"}}"#;
        std::fs::write(sd.overrides_json(), valid).unwrap();
        let want = Overrides {
            results: Some(false),
            guards: BTreeMap::from([("claims".to_string(), "enforce".to_string())]),
            ..Default::default()
        };
        assert_eq!(Overrides::load(&sd), want);
        assert_eq!(Overrides::parse(Some(valid)), want);
    }
}
