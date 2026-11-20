pub mod adapter;
pub mod claude_code;
pub mod env;
pub mod launcher;
pub mod opencode;
pub mod serve_client;
pub mod session_dir;
pub mod signals;

pub use adapter::{AgentAdapter, ChildSpec, LaunchCtx, Plugins, Preflight};
pub use launcher::{LaunchOpts, Launcher};
