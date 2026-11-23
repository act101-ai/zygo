//! §8.6: the session text as additional context at session start; for a workspace member also the resume note and
//! the member record (workspaces design W§9.1, W§9.4; H1).

use std::io::Write;
use std::path::Path;

use serde_json::json;
use zygo_core::hook::{error_line, HookInput, HookOutput};
use zygo_core::overrides::Overrides;
use zygo_core::paths::Paths;
use zygo_core::session_dir::SessionDir;
use zygo_core::session_view::SessionView;
use zygo_core::Agent;

use crate::session_text::build;
use crate::workspace;

pub fn run(input: &HookInput) -> anyhow::Result<HookOutput> {
    let Some(d) = SessionDir::from_env() else { return Ok(HookOutput::None) };
    run_member(input, &d, &Paths::from_env(), &std::env::current_exe().unwrap_or_default(), &mut std::io::stdout())
}

pub fn run_in(input: &HookInput, dir: &SessionDir) -> anyhow::Result<HookOutput> {
    // Subagents get the role prompt and the mechanics through `--agents` (§8.5).
    if input.has_agent_id {
        return Ok(HookOutput::None);
    }
    let v = SessionView::load(dir)?;
    Ok(output(v.agent, text_of(dir, &v)))
}

/// The session text of `v` with the overrides of the session folder applied.
fn text_of(dir: &SessionDir, v: &SessionView) -> String {
    let mut v = v.clone();
    let ov = Overrides::load(dir);
    v.results = ov.results.unwrap_or(v.results);
    v.work_units = ov.work_units.unwrap_or(v.work_units);
    build(&v)
}

/// The hook output of `text` for `agent`.
fn output(agent: Agent, text: String) -> HookOutput {
    match agent {
        Agent::ClaudeCode => {
            HookOutput::Json(json!({"hookSpecificOutput": {"hookEventName": "SessionStart", "additionalContext": text}}))
        }
        _ => HookOutput::Text(text),
    }
}

/// `run_in` with the member part (H1 D3, D11). The view is loaded as `run_in` loads it. A launch that is not a member
/// gets the output or the error of `run_in` and no other effect. The Claude Code `SessionStart` entry of a member is
/// `session-start --event member` (`input.cli_event == Some("member")`): the session folder is checked first, and a
/// folder that `workspace::check_folder` refuses gives one stderr line (the D2 text), the session text of `run_in`'s
/// read with no note (nothing when that read fails) and no write (attempt-2 review round 1, F3); after the check a
/// load error writes one session-log line and prints nothing (a `view.json` that is a FIFO, a folder or not JSON is a
/// load error); a `view.json` that is a link to a regular view loads, then writes one session-log line and gives the
/// session text with no note and no other change, as does an entry that is no longer a regular file after the load.
/// Without the flag (a launch that is not a member, or the OpenCode per-request call) a load error is `run_in`'s
/// error and that check does not run. A session folder that `workspace::check_folder` refuses skips the member part.
/// Else `workspace::session_start` runs; a Claude Code member's output is printed to `out` there, so the result is
/// `HookOutput::None`. A `view.json` that is a FIFO is a load error of `SessionView::load` (`<path>: is a FIFO`), with
/// no wait, for every launch (FIX-22).
pub fn run_member(
    input: &HookInput, dir: &SessionDir, paths: &Paths, zygo: &Path, out: &mut dyn Write,
) -> anyhow::Result<HookOutput> {
    if input.has_agent_id {
        return Ok(HookOutput::None);
    }
    let member = input.cli_event.as_deref() == Some("member");
    if member && let Err(e) = workspace::check_folder(dir) {
        // D2 (attempt-2 review round 1, F3): nothing is written into a refused folder; the session text of today's
        // read is printed with no note, and the refusal is the one stderr line.
        error_line(&format!("{e:#}"));
        return Ok(match SessionView::load(dir) {
            Ok(v) => output(v.agent, text_of(dir, &v)),
            Err(_) => HookOutput::None,
        });
    }
    let v = match SessionView::load(dir) {
        Ok(v) => v,
        Err(e) if member => {
            dir.append_log(&format!("workspace hook session-start: {e:#}"));
            return Ok(HookOutput::None);
        }
        Err(e) => return Err(e),
    };
    let text = text_of(dir, &v);
    if v.workspace.is_none() || workspace::check_folder(dir).is_err() {
        return Ok(output(v.agent, text));
    }
    if member {
        let view = dir.view_json();
        let refused = match std::fs::symlink_metadata(&view) {
            Ok(m) if m.file_type().is_symlink() => Some("is a link"),
            Ok(m) if !m.is_file() => Some("is not a regular file"),
            Ok(_) => None,
            // Gone since the load: nothing to change in it.
            Err(_) => return Ok(output(v.agent, text)),
        };
        if let Some(why) = refused {
            dir.append_log(&format!("workspace hook session-start: {}: {why}; not read", view.display()));
            return Ok(output(v.agent, text));
        }
    }
    Ok(match workspace::session_start(&input.raw, dir, &v, paths, zygo, out, &text)? {
        None => HookOutput::None,
        Some(t) => output(v.agent, t),
    })
}
