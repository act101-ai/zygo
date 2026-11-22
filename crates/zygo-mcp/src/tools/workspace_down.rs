//! `workspace_down` (workspaces design AU65): the fallback of W§17 PW15. The zygo MCP server runs outside the agent's
//! sandbox and lives for the whole agent session. Thus the detached `down` (AU18) that it starts outlives the tool call.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use zygo_config::repo_file::kill_tree;
use zygo_core::paths::Paths;
use zygo_core::session_view::SessionView;
use zygo_work::hooks;
use zygo_workspace::state::Store;

use crate::tool::{McpCtx, Tool, ToolDef, ToolError};

pub const DESCRIPTION: &str = "Stop the workspace of this session. It runs zygo down for this workspace and returns at once. The workspace stops after your turn, so end your turn after the call. Set now to true to stop at once, without waiting for running subagents.";

/// The longest wait for the exit of `down` and for EOF on its stdout and stderr (AU65).
const LIMIT: Duration = Duration::from_secs(5);
const TIMED_OUT: &str = "zygo: down did not return within 5 s";

/// `workspace_down` only when `[features] workspace_tool` is on in the merged config and the session view has a
/// membership marker (AU58). A view that cannot be read, or a zygo binary path that cannot be found, lists no tool.
pub fn tools(ctx: &McpCtx) -> Vec<Box<dyn Tool>> {
    if !ctx.features.workspace_tool {
        return Vec::new();
    }
    let Some(sd) = ctx.session_dir.as_ref() else { return Vec::new() };
    let Some(ws) = SessionView::load(sd).ok().and_then(|v| v.workspace) else { return Vec::new() };
    let Ok(program) = std::env::current_exe() else { return Vec::new() };
    vec![Box::new(WorkspaceDown::new(program, Paths::from_env(), ws.member.ws_id, hooks::session_id(sd)))]
}

pub struct WorkspaceDown {
    program: PathBuf,
    paths: Paths,
    ws_id: String,
    session_id: String,
}

impl WorkspaceDown {
    /// `program` is the zygo binary: `std::env::current_exe()` in `tools`, a stub in the tests.
    pub fn new(program: PathBuf, paths: Paths, ws_id: String, session_id: String) -> WorkspaceDown {
        WorkspaceDown { program, paths, ws_id, session_id }
    }

    /// The workspace name from `workspace.json`; the error names the id.
    fn name(&self) -> Result<String, ToolError> {
        match Store::open(&self.paths, &self.ws_id).read() {
            Ok(Some(ws)) => Ok(ws.name),
            Ok(None) => Err(ToolError::Failed(format!("zygo: workspace {} has no state", self.ws_id))),
            Err(e) => Err(ToolError::Failed(format!("zygo: workspace {}: {e}", self.ws_id))),
        }
    }
}

/// `now` (optional boolean) is the only key.
fn parse_now(args: &Value) -> Result<bool, ToolError> {
    let Some(obj) = args.as_object() else {
        return Err(ToolError::InvalidParams("arguments must be an object".into()));
    };
    if let Some(k) = obj.keys().find(|k| k.as_str() != "now") {
        return Err(ToolError::InvalidParams(format!("unknown key `{k}`; the only key is `now`")));
    }
    match obj.get("now") {
        None => Ok(false),
        Some(Value::Bool(b)) => Ok(*b),
        Some(_) => Err(ToolError::InvalidParams("now must be true or false".into())),
    }
}

impl Tool for WorkspaceDown {
    fn def(&self) -> ToolDef {
        ToolDef {
            name: "workspace_down".into(),
            description: DESCRIPTION.into(),
            input_schema: json!({"type": "object", "properties": {"now": {"type": "boolean"}}, "additionalProperties": false}),
        }
    }

    fn call(&self, args: Value) -> Result<Value, ToolError> {
        let now = parse_now(&args)?;
        let name = self.name()?;
        let mut cmd = Command::new(&self.program);
        cmd.arg("down").arg(&name);
        if now {
            cmd.arg("--now");
        }
        cmd.env("ZYGO_SESSION", &self.session_id);
        let (status, text) = run_bounded(cmd, LIMIT)?;
        if status.success() {
            Ok(Value::String(text))
        } else if text.is_empty() {
            Err(ToolError::Failed(format!("zygo: down exited with {status}")))
        } else {
            Err(ToolError::Failed(text))
        }
    }
}

/// Runs `cmd` in its own process group, with stdin from `/dev/null` and stdout and stderr piped. It waits at most
/// `limit` for the exit and for EOF on both pipes. The text is stdout, then stderr; a newline is put between them
/// when stdout does not end in one. Trailing white space is removed. On the limit (no exit, or no EOF), the whole
/// group is killed with `kill_tree` and the child is reaped. This also ends the reader threads.
fn run_bounded(mut cmd: Command, limit: Duration) -> Result<(ExitStatus, String), ToolError> {
    let deadline = Instant::now() + limit;
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    let mut child = cmd.spawn().map_err(|e| ToolError::Failed(format!("zygo: down cannot start: {e}")))?;
    let stop = |child: &mut Child, why: String| {
        kill_tree(child);
        ToolError::Failed(why)
    };
    let (Some(mut out), Some(mut err)) = (child.stdout.take(), child.stderr.take()) else {
        return Err(stop(&mut child, "zygo: down cannot start: no pipes".into()));
    };
    let (tx, rx) = mpsc::channel::<(usize, Vec<u8>)>();
    let etx = tx.clone();
    std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = out.read_to_end(&mut b);
        let _ = tx.send((0, b));
    });
    std::thread::spawn(move || {
        let mut b = Vec::new();
        let _ = err.read_to_end(&mut b);
        let _ = etx.send((1, b));
    });
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            Ok(None) => return Err(stop(&mut child, TIMED_OUT.into())),
            Err(e) => return Err(stop(&mut child, format!("zygo: down: {e}"))),
        }
    };
    let mut parts = [Vec::new(), Vec::new()];
    for _ in 0..2 {
        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok((i, b)) => parts[i] = b,
            // The child has exited and been reaped, but a process of its group holds a pipe: kill the group.
            Err(_) => return Err(stop(&mut child, TIMED_OUT.into())),
        }
    }
    let mut text = String::from_utf8_lossy(&parts[0]).into_owned();
    let err = String::from_utf8_lossy(&parts[1]);
    if !text.is_empty() && !text.ends_with('\n') && !err.is_empty() {
        text.push('\n');
    }
    text.push_str(&err);
    Ok((status, text.trim_end().to_string()))
}
