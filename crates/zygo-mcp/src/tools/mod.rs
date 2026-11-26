pub mod compact;
pub mod mail;
pub mod results;
pub mod work;
pub mod workspace_down;

use zygo_core::Agent;

use crate::tool::{McpCtx, Tool};

pub fn build(ctx: &McpCtx) -> anyhow::Result<Vec<Box<dyn Tool>>> {
    build_with(ctx, results::tools, work::tools)
}

#[doc(hidden)]
pub fn build_with(
    ctx: &McpCtx,
    results: impl FnOnce(&McpCtx) -> Vec<Box<dyn Tool>>,
    work: impl FnOnce(&McpCtx) -> Vec<Box<dyn Tool>>,
) -> anyhow::Result<Vec<Box<dyn Tool>>> {
    let mut v: Vec<Box<dyn Tool>> = Vec::new();
    if ctx.agent == Agent::ClaudeCode && ctx.features.compact_tool {
        v.push(Box::new(compact::Compact));
    }
    if ctx.features.results {
        v.extend(results(ctx));
    }
    if ctx.features.work_units {
        v.extend(work(ctx));
    }
    v.extend(workspace_down::tools(ctx));
    v.extend(mail::tools(ctx));
    let mut seen = std::collections::BTreeSet::new();
    for t in &v {
        let name = t.def().name;
        anyhow::ensure!(seen.insert(name.clone()), "zygo mcp: two tools are named `{name}`");
    }
    Ok(v)
}
