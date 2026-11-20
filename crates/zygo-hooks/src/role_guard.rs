use serde_json::json;
use zygo_core::hook::{HookInput, HookOutput};
use zygo_config::Level;
use zygo_core::session_dir::SessionDir;
use zygo_work::log::{self, Kind, LogLine};
use zygo_work::policy::Guard;

const EDIT_TOOLS: [&str; 3] = ["Edit", "Write", "NotebookEdit"];

const DENY_REASON: &str = "zygo: the lead does not edit files. Delegate this change to the zygo-code subagent with the files, the change and how to check it.";

/// §8.3: the lead (no `agent_id` key) may not edit files; a subagent and every other tool pass.
/// Each refusal first appends a `guard` line to `main.jsonl` (§13.4 note, §20 #36); a write error is a C6 error.
pub fn run(input: &HookInput) -> anyhow::Result<HookOutput> { guard_in(input, SessionDir::from_env().as_ref()) }

/// `run` with the session directory given, so tests do not read the environment.
pub fn guard_in(input: &HookInput, dir: Option<&SessionDir>) -> anyhow::Result<HookOutput> {
    let edits = input.tool_name.as_deref().is_some_and(|t| EDIT_TOOLS.iter().any(|e| e.eq_ignore_ascii_case(t)));
    if input.has_agent_id || !edits {
        return Ok(HookOutput::None);
    }
    if let Some(dir) = dir {
        let kind = Kind::Guard { guard: Guard::Lead.name().into(), level: Level::Enforce.name().into() };
        let ts = time::OffsetDateTime::now_utc().unix_timestamp();
        // The lead's log is `main` (`agent_of`). `Ok(None)` is a full log: the refusal stands. `Err` is a C6 error.
        log::lock(&dir.work(), "main")?.append(&LogLine { n: 0, ts, kind })?;
    }
    Ok(HookOutput::Json(json!({"hookSpecificOutput": {
        "hookEventName": "PreToolUse",
        "permissionDecision": "deny",
        "permissionDecisionReason": DENY_REASON,
    }})))
}
