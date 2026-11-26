//! The start of the sync child and its rate limit (mail sync design S§8.1, S§8.4, S§15). The hook path starts a
//! child when `now - last_start >= sync_gap` and writes `mail/sync/last_start` before the spawn; a send starts one
//! with no gap and writes no `last_start`. Nothing here waits for the child or runs git.
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::env::{Env, Spawner};
use crate::state::{read_last_start, write_last_start, SyncPaths};

/// S§8.1: the production [`Spawner`]: `<zygo> mail sync --ws-id <id> --background` as a detached child.
#[derive(Debug, Clone)]
pub struct ChildSpawner {
    pub zygo: PathBuf,
}

impl ChildSpawner {
    /// The command of the child: stdin, stdout and stderr to `/dev/null`, on unix in `/` and in a new session
    /// (`setsid`).
    pub fn command(&self, ws_id: &str) -> Command {
        let mut cmd = Command::new(&self.zygo);
        cmd.args(["mail", "sync", "--ws-id", ws_id, "--background"]);
        cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.current_dir("/");
            // SAFETY: the closure runs in the child between fork and exec and calls only `setsid`, which is
            // async-signal-safe; it allocates nothing.
            unsafe {
                cmd.pre_exec(|| if libc::setsid() == -1 { Err(std::io::Error::last_os_error()) } else { Ok(()) });
            }
        }
        cmd
    }
}

impl Spawner for ChildSpawner {
    // The caller does not wait for the child: it ends soon after, and `init` reaps the child (S§8.1).
    #[allow(clippy::zombie_processes)]
    fn spawn_sync(&self, ws_id: &str) -> std::io::Result<()> {
        self.command(ws_id).spawn().map(|_| ())
    }
}

/// The whole seconds of `now` since the Unix epoch; `0` before it.
fn unix_secs(now: SystemTime) -> u64 {
    now.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// S§8.4: `true` when no `last_start` is read, when `now - last_start >= gap`, or when `last_start` is later than
/// `now` (a clock set back, or a corrupt file).
pub fn due(sp: &SyncPaths, now: SystemTime, gap: Duration) -> bool {
    let Some(last) = read_last_start(sp) else { return true };
    unix_secs(now).checked_sub(last).is_none_or(|d| d >= gap.as_secs())
}

/// S§8.1, S§8.4: starts the sync child of `ws_id`. With `Some(gap)` (the hook path) it starts one only when [`due`],
/// and writes `last_start` first; a failed write gives one `log` line and no child. With `None` (a send) it starts
/// one with no gap and reads and writes no `last_start` (S§15). A child that cannot start gives one `log` line.
pub fn trigger(env: &Env, ws_id: &str, gap: Option<Duration>, log: &dyn Fn(&str)) {
    let sp = match SyncPaths::new(&env.paths, ws_id) {
        Ok(sp) => sp,
        Err(e) => return log(&format!("cannot start the mail sync child: {e}")),
    };
    if let Some(gap) = gap {
        let now = env.clock.now();
        if !due(&sp, now, gap) {
            return;
        }
        if let Err(e) = write_last_start(&sp, unix_secs(now)) {
            return log(&format!("cannot write mail/sync/last_start: {e}"));
        }
    }
    if let Err(e) = env.spawner.spawn_sync(ws_id) {
        log(&format!("cannot start the mail sync child: {e}"));
    }
}
