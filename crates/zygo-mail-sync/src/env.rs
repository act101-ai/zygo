//! The test seams of S-C9: the clock, the public probe, the spawn of the sync child and a hook between named
//! steps. Production code takes the real ones (S§18).

use std::time::SystemTime;
use zygo_core::paths::Paths;

/// S-C9: the clock, injected so no test depends on wall-clock time.
pub trait Clock { fn now(&self) -> SystemTime; }

/// S-C9: the production clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock { fn now(&self) -> SystemTime { SystemTime::now() } }

/// S§8.2: the result of the public probe; `Unknown` carries the reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict { Public, Private, Local, Unknown(String) }

/// S§8.2, S-C9: the public probe of a remote's `https` URL.
pub trait Probe { fn probe(&self, https_url: &str) -> Verdict; }

/// S§8.1, S-C9: the spawn of the sync child for a workspace.
pub trait Spawner { fn spawn_sync(&self, ws_id: &str) -> std::io::Result<()>; }

/// S-C9: the named point at which a test seam stopped the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Crash(pub &'static str);

/// S-C9: a hook between named steps; a test returns `Err` to stop the run at that point.
pub trait Seam { fn at(&self, point: &'static str) -> Result<(), Crash>; }

/// S-C9: the production seam, which never stops the run.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoSeam;

impl Seam for NoSeam { fn at(&self, _point: &'static str) -> Result<(), Crash> { Ok(()) } }

/// S§18: the paths and the seams that one sync run uses.
pub struct Env {
    pub paths: Paths,
    pub clock: Box<dyn Clock>,
    pub probe: Box<dyn Probe>,
    pub seam: Box<dyn Seam>,
    pub spawner: Box<dyn Spawner>,
}
