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

    /// FX-37 SX-F2: only a regular file is read. A folder, and on unix a link to valid overrides and a FIFO, give the
    /// default; the FIFO load does not wait (it returns within the 60 s bound of the test, not a timing check).
    #[test]
    fn load_uses_only_a_regular_file() {
        let d = tempfile::tempdir().unwrap();
        let sd = SessionDir::new(d.path().join("s"));
        sd.create().unwrap();
        let valid = br#"{"results":false}"#;
        std::fs::create_dir(sd.overrides_json()).unwrap();
        assert_eq!(Overrides::load(&sd), Overrides::default());
        std::fs::remove_dir(sd.overrides_json()).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            let target = d.path().join("target.json");
            std::fs::write(&target, valid).unwrap();
            std::os::unix::fs::symlink(&target, sd.overrides_json()).unwrap();
            assert_eq!(Overrides::load(&sd), Overrides::default());
            std::fs::remove_file(sd.overrides_json()).unwrap();
            let c = std::ffi::CString::new(sd.overrides_json().as_os_str().as_bytes()).unwrap();
            // SAFETY: `c` is a valid NUL-terminated path.
            assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
            let (tx, rx) = std::sync::mpsc::channel();
            let fifo = sd.clone();
            std::thread::spawn(move || {
                let _ = tx.send(Overrides::load(&fifo));
            });
            let got = rx.recv_timeout(std::time::Duration::from_secs(60)).expect("Overrides::load waited on a FIFO");
            assert_eq!(got, Overrides::default());
            std::fs::remove_file(sd.overrides_json()).unwrap();
        }
        std::fs::write(sd.overrides_json(), valid).unwrap();
        assert_eq!(Overrides::load(&sd).results, Some(false));
    }
}
