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
    /// Per-subagent re-inject flags (session text design T§9.2): directory 0700, files 0600.
    pub fn reinject(&self) -> PathBuf {
        self.root.join("reinject")
    }
    /// Turn and subagent state of a workspace member (workspaces design W§9.1).
    pub fn activity_json(&self) -> PathBuf {
        self.root.join("activity.json")
    }
    /// The fs4 lock for changes to `activity.json` (AU56).
    pub fn activity_lock(&self) -> PathBuf {
        self.root.join("activity.lock")
    }

    /// Appends one line to the session log (§8.4): the one writer of `session.log` (H1 D21). Each `\n` and `\r` in
    /// `line` becomes a space, then one `\n` ends the line. Created with mode 0600 on unix and opened with
    /// `O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC`, so a link is not followed and a FIFO does not make the writer wait;
    /// only a regular file is written. Every error is ignored.
    pub fn append_log(&self, line: &str) {
        let mut o = fs::OpenOptions::new();
        o.append(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            o.mode(0o600).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC);
        }
        let Ok(mut f) = o.open(self.log()) else { return };
        if !f.metadata().is_ok_and(|m| m.file_type().is_file()) {
            return;
        }
        let mut text = line.replace(['\n', '\r'], " ");
        text.push('\n');
        let _ = f.write_all(text.as_bytes());
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
pub fn write_atomic(target: &Path, bytes: &[u8]) -> anyhow::Result<()> {
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
