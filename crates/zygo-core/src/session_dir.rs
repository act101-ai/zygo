use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::Context;

/// The launcher's lock file in its session directory: its process id (for people), locked by the launcher from
/// before `token` is written until its exit ends (N37 U3, §14 note).
pub const PID_FILE: &str = "pid";

/// A session is live while its directory holds `token` and another holder has the lock on `pid` (N37 R1, U3); a
/// missing `pid` counts as dead. A lock ends with its holder, so a reused process id does not matter.
pub fn live(dir: &Path) -> bool {
    dir.join("token").exists()
        && fs::File::open(dir.join(PID_FILE))
            .is_ok_and(|f| matches!(fs4::FileExt::try_lock(&f), Err(fs4::TryLockError::WouldBlock)))
}

#[derive(Debug, Clone)]
pub struct SessionDir {
    pub root: PathBuf,
}

impl SessionDir {
    pub fn new(root: PathBuf) -> Self {
        SessionDir { root }
    }

    /// From `ZYGO_SESSION_DIR`.
    pub fn from_env() -> Option<Self> {
        std::env::var_os("ZYGO_SESSION_DIR")
            .filter(|v| !v.is_empty())
            .map(|v| SessionDir::new(PathBuf::from(v)))
    }

    /// Creates the directory and any missing parents, mode 0700 on unix.
    pub fn create(&self) -> anyhow::Result<()> {
        let mut b = fs::DirBuilder::new();
        b.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            b.mode(0o700);
        }
        b.create(&self.root)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&self.root, fs::Permissions::from_mode(0o700))?;
        }
        Ok(())
    }

    pub fn token(&self) -> PathBuf {
        self.root.join("token")
    }
    /// The proxy base URL, written by the launcher next to `token` (§8.2 step 2).
    pub fn url(&self) -> PathBuf {
        self.root.join("url")
    }
    pub fn settings_json(&self) -> PathBuf {
        self.root.join("settings.json")
    }
    pub fn view_json(&self) -> PathBuf {
        self.root.join("view.json")
    }
    /// The session log (§8.4), next to `token`; removed at exit with the directory, §8.2 step 6.
    pub fn log(&self) -> PathBuf {
        self.root.join("session.log")
    }
    pub fn overrides_json(&self) -> PathBuf {
        self.root.join("overrides.json")
    }
    pub fn results(&self) -> PathBuf {
        self.root.join("results")
    }
    pub fn work(&self) -> PathBuf {
        self.root.join("work")
    }

    pub fn read_token(&self) -> anyhow::Result<String> {
        let t = fs::read_to_string(self.token()).context("read session token")?;
        Ok(t.trim().to_string())
    }

    /// Writes the token, mode 0600 on unix; the mode is set before any byte is written.
    pub fn write_token(&self, t: &str) -> anyhow::Result<()> {
        let mut opts = fs::OpenOptions::new();
        opts.write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(self.token())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            f.set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        f.set_len(0)?;
        f.write_all(t.as_bytes())?;
        Ok(())
    }
}

/// Writes `bytes` to a unique temp file next to `target` (mode 0600 on unix), syncs it, then renames it over `target`.
pub(crate) fn write_atomic(target: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let dir = match target.parent() {
        Some(d) if !d.as_os_str().is_empty() => d,
        _ => Path::new("."),
    };
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tmp.as_file().set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    tmp.write_all(bytes)?;
    tmp.as_file().sync_all()?;
    tmp.persist(target)?;
    Ok(())
}
