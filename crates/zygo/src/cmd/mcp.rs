use zygo_core::{exit::ExitCode, paths::Paths, session_dir::SessionDir, Agent};
use zygo_mcp::{serve_stdio, tools, McpCtx};

pub fn run(agent: Option<Agent>) -> i32 {
    let Some(agent) = agent else {
        super::print_err("zygo mcp: --agent claude-code|opencode is required\n");
        return ExitCode::Usage as i32;
    };
    let ctx = match McpCtx::resolve(agent, &Paths::from_env(), std::env::current_dir().ok(), SessionDir::from_env()) {
        Ok(c) => c,
        Err(e) => { super::print_err(&format!("zygo mcp: {e:#}\n")); return ExitCode::ConfigInvalid as i32; }
    };
    let served = tools::build(&ctx).and_then(|t| serve_stdio(agent, t));
    match served {
        Ok(()) => 0,
        Err(e) => { super::print_err(&format!("zygo mcp: {e:#}\n")); 1 }
    }
}
