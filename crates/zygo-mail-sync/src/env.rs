//! The test seams of S-C9: the clock, the public probe, the spawn of the sync child and a hook between named
//! steps. Production code takes the real ones (S§18).

use std::path::PathBuf;
use std::time::SystemTime;
use zygo_config::MailConfig;
use zygo_core::paths::Paths;
use zygo_redact::secret::DeadlinePolicy;
use zygo_workspace::mail::wake::after_send_at;
use zygo_workspace::state::Workspace;

/// S-C9: the clock, injected so no test depends on wall-clock time.
pub trait Clock: Send + Sync { fn now(&self) -> SystemTime; }

/// S-C9: the production clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock { fn now(&self) -> SystemTime { SystemTime::now() } }

/// S§8.2: the result of the public probe; `Unknown` carries the reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict { Public, Private, Local, Unknown(String) }

/// S§8.2, S-C9: the public probe of a remote's `https` URL.
pub trait Probe: Send + Sync {
    fn probe(&self, https_url: &str) -> Verdict;
    /// The probe, ended by `deadline` at the latest (S§8.3 step 4, FX-40 review F4). A probe that runs no process
    /// (a test seam) gives [`Probe::probe`]; [`crate::public::LsRemote`] ends its `git ls-remote` at the deadline.
    fn probe_until(&self, https_url: &str, deadline: std::time::Instant) -> Verdict {
        let _ = deadline;
        self.probe(https_url)
    }
}

/// S§8.1, S-C9: the spawn of the sync child for a workspace.
pub trait Spawner: Send + Sync { fn spawn_sync(&self, ws_id: &str) -> std::io::Result<()>; }

/// S-C9: the named point at which a test seam stopped the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Crash(pub &'static str);

/// S-C9: a hook between named steps; a test returns `Err` to stop the run at that point.
pub trait Seam: Send + Sync { fn at(&self, point: &'static str) -> Result<(), Crash>; }

/// S-C9: the production seam, which never stops the run.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoSeam;

impl Seam for NoSeam { fn at(&self, _point: &'static str) -> Result<(), Crash> { Ok(()) } }

/// S§10.1, M§7.3, S-C9: the wake of the members `keys` after an import or a notice, at the injected time `now`.
pub trait Wake: Send + Sync {
    fn wake(&self, paths: &Paths, ws: &Workspace, keys: &[String], cfg: &MailConfig, now: SystemTime);
}

/// The production wake: the M§7.3 wake through the tmux program `tmux` and the AU26 idle rule, with the deferred
/// retry of a member in the quiet window (`after_send_at`, FX-35).
#[derive(Debug, Clone)]
pub struct TmuxWake {
    pub tmux: PathBuf,
}

impl Wake for TmuxWake {
    fn wake(&self, paths: &Paths, ws: &Workspace, keys: &[String], cfg: &MailConfig, now: SystemTime) {
        after_send_at(paths, ws, &self.tmux, keys, cfg, now);
    }
}

/// S§18: the paths and the seams that one sync run uses. `secret_policy` is the deadline policy of the secret pass
/// at commit (S§8.6); production code gives `DeadlinePolicy::default()`.
pub struct Env {
    pub paths: Paths,
    pub clock: Box<dyn Clock>,
    pub probe: Box<dyn Probe>,
    pub seam: Box<dyn Seam>,
    pub spawner: Box<dyn Spawner>,
    pub wake: Box<dyn Wake>,
    pub secret_policy: DeadlinePolicy,
}
