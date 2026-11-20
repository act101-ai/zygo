//! The session directory at launch and at exit (§8.2 steps 2 and 6).

use std::fs::File;
use std::io::Write;
use std::path::Path;

use zygo_core::paths::Paths;
use zygo_core::session_dir::SessionDir;
use zygo_proxy::Url;
use zygo_usage::Store;
use zygo_work::{fold, log as work_log};

/// The session directory exists already: another session's, or the same name under another case (B-1).
#[derive(Debug)]
pub(crate) struct Taken;

impl std::fmt::Display for Taken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the session directory already exists")
    }
}

impl std::error::Error for Taken {}

/// Creates `<sessions>/<session>` (0700; refused with [`Taken`] when it exists), then `pid` (this process id) with its
/// lock taken (U3), then `token` (0600) and `url`. The session is live while the returned file is open; Rust opens it
/// close-on-exec, so the agent does not inherit the lock.
pub(crate) fn open(paths: &Paths, session: &str, token: &str, url: &Url) -> anyhow::Result<(SessionDir, File)> {
    SessionDir::new(paths.sessions_dir.clone()).create()?; // the parents, each new one 0700 (N-1)
    let sd = SessionDir::new(paths.sessions_dir.join(session));
    #[cfg(unix)]
    let b = {
        let mut b = std::fs::DirBuilder::new();
        std::os::unix::fs::DirBuilderExt::mode(&mut b, 0o700);
        b
    };
    #[cfg(not(unix))]
    let b = std::fs::DirBuilder::new();
    // Not recursive: an existing directory (or one that differs only in case) is another session's (B-1).
    b.create(&sd.root).map_err(|e| if e.kind() == std::io::ErrorKind::AlreadyExists { anyhow::Error::new(Taken) } else { e.into() })?;
    sd.create()?; // exists now; sets 0700 against the umask
    let mut pid = File::create(sd.root.join(crate::serve_client::PID_FILE))?;
    lock_or_log(&sd, fs4::FileExt::try_lock(&pid))?;
    write!(pid, "{}", std::process::id())?;
    sd.write_token(token)?;
    std::fs::write(sd.url(), url.as_str())?;
    Ok((sd, pid))
}

/// "Held by another" stops the launch; any other lock error (for example `ENOLCK`) is logged and the launch goes on
/// with `pid` not locked, so `zygo usage --sync` does not count the session live (V3).
fn lock_or_log(sd: &SessionDir, r: Result<(), fs4::TryLockError>) -> std::io::Result<()> {
    match r {
        Err(fs4::TryLockError::Error(e)) => {
            log(sd, &format!("pid not locked ({e}); zygo usage --sync does not count this session live"));
            Ok(())
        }
        r => r.map_err(std::io::Error::from),
    }
}

/// Appends one line to the session log (0600 on unix); errors are ignored.
pub(crate) fn log(sd: &SessionDir, line: &str) {
    let mut o = std::fs::OpenOptions::new();
    o.append(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    if let Ok(mut f) = o.open(sd.log()) {
        let _ = writeln!(f, "{line}");
    }
}

/// Folds every open unit (`<agent>.begin.json`) into its `work_units` row; `ended_at` = the unit's stop, else `now`.
/// Errors are ignored.
pub(crate) fn fold_units(sd: &SessionDir, usage_db: &Path, now: i64) {
    let dir = sd.work();
    let Ok(entries) = std::fs::read_dir(&dir) else { return };
    let mut rows = Vec::new();
    for e in entries.flatten() {
        let name = e.file_name();
        let Some(agent) = name.to_str().and_then(|n| n.strip_suffix(".begin.json")) else { continue };
        let Some(begin) = work_log::open_unit(&dir, agent) else { continue };
        let Some(mut row) = fold::fold_begin(&dir, agent, &begin) else { continue };
        row.ended_at = begin.stopped_at.or(Some(now));
        rows.push(row);
    }
    if rows.is_empty() {
        return;
    }
    let Ok(store) = Store::open(usage_db) else { return };
    for r in &rows {
        let _ = store.insert_work_unit(r);
    }
}

/// Removes every entry but `results/`; without `results/` the directory itself goes (§8.2 step 6, §10.1).
pub(crate) fn remove_except_results(sd: &SessionDir) {
    let Ok(entries) = std::fs::read_dir(&sd.root) else { return };
    for e in entries.flatten() {
        if e.file_name() == "results" {
            continue;
        }
        let p = e.path();
        if e.file_type().is_ok_and(|t| t.is_dir()) {
            let _ = std::fs::remove_dir_all(&p);
        } else {
            let _ = std::fs::remove_file(&p);
        }
    }
    let _ = std::fs::remove_dir(&sd.root);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lock_error_other_than_held_is_logged_and_the_launch_goes_on() { // V3 (round 4)
        let d = tempfile::tempdir().unwrap();
        let sd = SessionDir::new(d.path().to_path_buf());
        let no_locks = std::io::Error::other("no locks on this file system"); // as ENOLCK
        assert!(lock_or_log(&sd, Err(fs4::TryLockError::Error(no_locks))).is_ok());
        let log = std::fs::read_to_string(sd.log()).unwrap();
        assert!(log.starts_with("pid not locked ("), "{log}");
        assert!(lock_or_log(&sd, Err(fs4::TryLockError::WouldBlock)).is_err());
        assert!(lock_or_log(&sd, Ok(())).is_ok());
    } // mutations: every lock error stops the launch; WouldBlock ignored too
}
