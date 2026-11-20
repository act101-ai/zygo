use anyhow::Context;
use zygo_core::hook::{HookInput, HookOutput};
use zygo_core::session_dir::SessionDir;

/// §8.4: print the session token from `$ZYGO_SESSION_DIR/token`.
pub fn run(_input: &HookInput) -> anyhow::Result<HookOutput> {
    token_in(&SessionDir::from_env().context("ZYGO_SESSION_DIR is not set")?)
}

pub fn token_in(dir: &SessionDir) -> anyhow::Result<HookOutput> {
    Ok(HookOutput::Text(dir.read_token()?))
}
