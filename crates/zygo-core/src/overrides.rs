use std::collections::BTreeMap;
use std::path::Path;

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
    /// A missing or invalid file gives the default. Only a regular file is read (FX-37), by the rule of the setting
    /// verbs' reader `zygo_hooks::workspace::read_session_file`: on unix the open is `O_NOFOLLOW | O_NONBLOCK`, so a
    /// link is not followed and a FIFO does not make the read wait; a link, a FIFO, a folder or another entry that is
    /// not a regular file, and an open or read error, give the default. There is no size cap (as FIX-22).
    pub fn load(dir: &SessionDir) -> Overrides {
        Overrides::parse(read_regular(&dir.overrides_json()).as_deref())
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

/// The bytes of the regular file at `path`, by the rule of `Overrides::load`; `None` for any other entry or error.
fn read_regular(path: &Path) -> Option<Vec<u8>> {
    let mut o = std::fs::OpenOptions::new();
    o.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC);
    }
    let mut f = o.open(path).ok()?;
    if !f.metadata().ok()?.is_file() {
        return None;
    }
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut f, &mut bytes).ok()?;
    Some(bytes)
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

    /// FX-37 SX-F2: only a regular file is read. A folder, and on unix a link to valid overrides and a socket, give the
    /// default; a regular file is read. None of these opens can wait, so the test has no deadline.
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
            let target = d.path().join("target.json");
            std::fs::write(&target, valid).unwrap();
            std::os::unix::fs::symlink(&target, sd.overrides_json()).unwrap();
            assert_eq!(Overrides::load(&sd), Overrides::default());
            std::fs::remove_file(sd.overrides_json()).unwrap();
            drop(std::os::unix::net::UnixListener::bind(sd.overrides_json()).unwrap());
            assert_eq!(Overrides::load(&sd), Overrides::default());
            std::fs::remove_file(sd.overrides_json()).unwrap();
        }
        std::fs::write(sd.overrides_json(), valid).unwrap();
        assert_eq!(Overrides::load(&sd).results, Some(false));
    }

    /// FX-37 SX-F2: a FIFO at `overrides.json` gives the default and does not make the load wait.
    #[cfg(unix)]
    #[test]
    #[ignore = "timing: the FIFO load must end inside a 60 s recv_timeout"]
    fn load_does_not_wait_on_a_fifo() {
        use std::os::unix::ffi::OsStrExt;
        let d = tempfile::tempdir().unwrap();
        let sd = SessionDir::new(d.path().join("s"));
        sd.create().unwrap();
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
    }
}
