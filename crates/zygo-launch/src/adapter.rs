//! What each agent supplies to the launcher (§8.2 step 4): its program, its config builder and its preflight.

use std::path::PathBuf;
use std::sync::Arc;

use zygo_config::profile::Profile;
use zygo_config::Merged;
use zygo_core::paths::Paths;
use zygo_core::session_dir::SessionDir;
use zygo_core::session_view::SessionView;
use zygo_core::Agent;
use zygo_plugins::snapshot::{Delivered, Snapshot};
use zygo_proxy::client::WindowArg;
use zygo_proxy::session::SessionConfig;
use zygo_proxy::Url;
use zygo_work::WorkItem;

use crate::env::Env;

pub trait AgentAdapter {
    fn agent(&self) -> Agent;
    /// The program name looked up on `PATH` (`claude`, `opencode`).
    fn program(&self) -> &str;
    /// Printed verbatim on exit 13.
    fn install_hint(&self) -> &str;
    /// The role the user's model flag picks, the agent's aliases resolved to role names (§8.5: `--model opus` → `lead`);
    /// any other value as given; `None` when no model flag is present.
    fn picked_model(&self, args: &[String]) -> Option<String>;
    /// [`Self::picked_model`] with an alias whose role the session lacks redirected as the agent redirects it.
    fn picked_role(&self, args: &[String], _cfg: &SessionConfig) -> Option<String> {
        self.picked_model(args)
    }
    fn build(&self, ctx: &LaunchCtx) -> anyhow::Result<ChildSpec>;
    fn preflight(&self, ctx: &LaunchCtx) -> Result<(), Preflight>;
    /// A launch the agent's arguments or settings refuse with exit 2 and this text (§12.3); checked before build.
    fn refusal(&self, _ctx: &LaunchCtx) -> Option<String> {
        None
    }
}

/// The agent's arguments, its environment rows (added after the denylist), stderr lines and session-log lines.
pub struct ChildSpec {
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub lines: Vec<String>,
    pub log: Vec<String>,
}

/// A settings file that sets a key the launch cannot run with (§8.4): exit 2.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preflight {
    pub file: PathBuf,
    pub key: String,
}

/// Claude Code: the session snapshot (§15.3). OpenCode: the delivered plugins, `None` when the store lock was not
/// obtained; `Delivered` holds the shared store lock until the context is dropped.
pub enum Plugins {
    Snapshot(Snapshot),
    Delivered(Option<Delivered>),
}

/// Everything an adapter reads to build the agent's session config. No `Debug`: it holds the session token.
pub struct LaunchCtx {
    pub agent: Agent,
    pub program: PathBuf,
    pub agent_version: String,
    pub cfg: Arc<Merged>,
    pub profile: Option<Profile>,
    pub session: String,
    pub token: String,
    pub session_dir: SessionDir,
    pub proxy_url: Url,
    pub config: SessionConfig,
    pub view: SessionView,
    pub effective_role: Option<String>,
    pub repo: Option<String>,
    pub work: Option<WorkItem>,
    pub plugins: Plugins,
    pub args: Vec<String>,
    pub passthrough: bool,
    pub yolo: bool,
    pub compact: Option<WindowArg>,
    pub cwd: PathBuf,
    /// The git checkout top of cwd, when there is one.
    pub repo_root: Option<PathBuf>,
    pub env: Env,
    pub paths: Paths,
}
