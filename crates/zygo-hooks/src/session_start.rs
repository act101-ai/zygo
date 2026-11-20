//! §8.6: the session text as additional context at session start.

use serde_json::json;
use zygo_core::hook::{HookInput, HookOutput};
use zygo_core::overrides::Overrides;
use zygo_core::session_dir::SessionDir;
use zygo_core::session_view::SessionView;
use zygo_core::Agent;

use crate::session_text::build;

pub fn run(input: &HookInput) -> anyhow::Result<HookOutput> {
    let Some(d) = SessionDir::from_env() else { return Ok(HookOutput::None) };
    run_in(input, &d)
}

pub fn run_in(input: &HookInput, dir: &SessionDir) -> anyhow::Result<HookOutput> {
    // Subagents get the role prompt and the mechanics through `--agents` (§8.5).
    if input.has_agent_id {
        return Ok(HookOutput::None);
    }
    let mut v = SessionView::load(dir)?;
    let ov = Overrides::load(dir);
    v.results = ov.results.unwrap_or(v.results);
    v.work_units = ov.work_units.unwrap_or(v.work_units);
    let text = build(&v);
    Ok(match v.agent {
        Agent::ClaudeCode => HookOutput::Json(
            json!({"hookSpecificOutput": {"hookEventName": "SessionStart", "additionalContext": text}}),
        ),
        _ => HookOutput::Text(text),
    })
}
