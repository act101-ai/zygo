//! `workspace_down` (workspaces design AU65): the fallback of W§17 PW15. The zygo MCP server runs outside the agent's
//! sandbox and lives for the whole agent session. Thus the detached `down` (AU18) that it starts outlives the tool call.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use zygo_config::repo_file::kill_tree;
#[cfg(unix)]
use zygo_config::repo_file::{exited_unreaped, kill_group};
use zygo_core::paths::Paths;
use zygo_core::session_view::SessionView;
use zygo_work::hooks;
use zygo_workspace::state::Store;

use crate::tool::{McpCtx, Tool, ToolDef, ToolError};

pub const DESCRIPTION: &str = "Stop the workspace of this session. It runs zygo down for this workspace and returns at once. The workspace stops after your turn, so end your turn after the call. Set now to true to stop at once, without waiting for running subagents.";

/// The longest wait for the exit of `down` and for EOF on its stdout and stderr (AU65).
const LIMIT: Duration = Duration::from_secs(5);
const TIMED_OUT: &str = "zygo: down did not return within 5 s";
/// The note line when `down` exited but a process of its group still held stdout or stderr at the limit, and the
/// group was killed.
const HELD_STOPPED: &str = "zygo: a process of down still held its output after 5 s and was stopped";
/// The note line for the same case when the kernel had already reaped `down` (SIGCHLD ignored): no kill is sent.
const HELD_LEFT: &str = "zygo: a process of down still held its output after 5 s and was left running";
/// The most bytes kept of each of stdout and stderr (as `zygo_work::repo`); the rest is read and dropped.
const CAP: usize = 64 * 1024;
/// The time between two checks for the exit.
const POLL: Duration = Duration::from_millis(10);

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
        // This tool passes the name to `zygo down` with no `--` before it, so the clap parser of `zygo down`
        // (`crates/zygo/src/cmd/down.rs`) reads a name that starts with `-` as a flag. AU84 allows `-` anywhere in a
        // name (`valid_name`, `crates/zygo-workspace/src/identity.rs:69`, which `validate` in `state.rs` calls), so
        // `Store::read` gives such a name; this check is the only guard, so refuse it here.
        if name.starts_with('-') {
            return Err(ToolError::Failed(format!("zygo: workspace name `{name}` starts with `-`; zygo down would read it as a flag")));
        }
        let mut cmd = Command::new(&self.program);
        cmd.arg("down").arg(&name);
        if now {
            cmd.arg("--now");
        }
        cmd.env("ZYGO_SESSION", &self.session_id);
        let ran = run_bounded(cmd, LIMIT)?;
        match ran.status {
            // An unknown status (the kernel reaped `down`, SIGCHLD ignored) counts as success, as in `zygo_work::repo`.
            None => Ok(Value::String(ran.text)),
            Some(s) if s.success() => Ok(Value::String(ran.text)),
            Some(s) if ran.text.is_empty() => Err(ToolError::Failed(format!("zygo: down exited with {s}"))),
            Some(_) => Err(ToolError::Failed(ran.text)),
        }
    }
}

/// The end of a run of `down` that exited within the limit.
struct Ran {
    /// The exit status; `None` when the kernel reaped the child itself (SIGCHLD ignored), so the status is unknown.
    status: Option<ExitStatus>,
    /// Stdout, then stderr, each cut at [`CAP`]; then the note line when a process still held a pipe at the limit.
    text: String,
}

/// How the wait for the exit ended.
enum Exit {
    /// The child has exited and is not reaped yet: its pid, which is also its group id, is still reserved, so a group
    /// kill reaches only its own group.
    #[cfg(unix)]
    Unreaped,
    /// The child is reaped (status known), or the kernel reaped it (`None`). No kill may be sent after this: its pid
    /// may already be reused.
    Reaped(Option<ExitStatus>),
}

/// Runs `cmd` in its own process group, with stdin from `/dev/null` and stdout and stderr piped. It waits at most
/// `limit` for the exit and for EOF on both pipes, and keeps at most [`CAP`] bytes of each pipe (the rest is read
/// and dropped). The text is stdout, then stderr; a newline is put between them when stdout does not end in one.
/// Trailing white space is removed.
///
/// No exit within the limit: the group is killed with `kill_tree` (the leader is not reaped yet) and the call fails
/// with the timeout text. An exit, but no EOF within the limit (a process still holds a pipe): on unix the group is
/// killed while the leader is still unreaped, then the leader is reaped; the result is the exit status with the
/// output read so far and the note line [`HELD_STOPPED`]. If the kernel already reaped the leader (SIGCHLD ignored),
/// no kill is sent and the note line is [`HELD_LEFT`].
///
/// The reader threads end at EOF. A process outside the group (for example one that called `setsid`) that holds a
/// pipe is not killed, so its reader thread stays blocked: it is abandoned, a bounded leak per call, never joined.
fn run_bounded(mut cmd: Command, limit: Duration) -> Result<Ran, ToolError> {
    let deadline = Instant::now() + limit;
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    let mut child = cmd.spawn().map_err(|e| ToolError::Failed(format!("zygo: down cannot start: {e}")))?;
    let (Some(out), Some(err)) = (child.stdout.take(), child.stderr.take()) else {
        kill_tree(&mut child);
        return Err(ToolError::Failed("zygo: down cannot start: no pipes".into()));
    };
    let (tx, rx) = mpsc::channel::<()>();
    let out_buf = read_capped(out, tx.clone());
    let err_buf = read_capped(err, tx);
    let exit = wait_exit(&mut child, deadline)?;
    let mut eof = 0;
    while eof < 2 && rx.recv_timeout(deadline.saturating_duration_since(Instant::now())).is_ok() {
        eof += 1;
    }
    let held = eof < 2;
    let (status, note) = match exit {
        #[cfg(unix)]
        Exit::Unreaped => {
            if held {
                kill_group(&child);
            }
            let s = child.wait().map_err(|e| ToolError::Failed(format!("zygo: down: {e}")))?;
            (Some(s), held.then_some(HELD_STOPPED))
        }
        Exit::Reaped(s) => (s, held.then_some(HELD_LEFT)),
    };
    let snapshot = |b: &Mutex<Vec<u8>>| String::from_utf8_lossy(&b.lock().unwrap_or_else(|e| e.into_inner())).into_owned();
    let mut text = snapshot(&out_buf);
    let err = snapshot(&err_buf);
    if !text.is_empty() && !text.ends_with('\n') && !err.is_empty() {
        text.push('\n');
    }
    text.push_str(&err);
    let mut text = text.trim_end().to_string();
    if let Some(note) = note {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(note);
    }
    Ok(Ran { status, text })
}

/// Waits until `deadline` for the exit of `child`. On unix it first polls `exited_unreaped` (WNOWAIT), so the child
/// stays unreaped and its group can still be killed safely. The first error of `waitid` (ECHILD when SIGCHLD is
/// ignored and the kernel reaps by itself) falls through to the `try_wait` loop, with no group kill after it. At the
/// deadline the group is killed with `kill_tree` while the child is unreaped, and the call fails.
fn wait_exit(child: &mut Child, deadline: Instant) -> Result<Exit, ToolError> {
    #[cfg(unix)]
    loop {
        match exited_unreaped(child) {
            Ok(true) => return Ok(Exit::Unreaped),
            Ok(false) if Instant::now() < deadline => std::thread::sleep(POLL),
            Ok(false) => {
                kill_tree(child);
                return Err(ToolError::Failed(TIMED_OUT.into()));
            }
            Err(_) => break,
        }
    }
    loop {
        match child.try_wait() {
            Ok(Some(s)) => return Ok(Exit::Reaped(Some(s))),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(POLL),
            Ok(None) => {
                kill_tree(child);
                return Err(ToolError::Failed(TIMED_OUT.into()));
            }
            // The kernel reaped the child (SIGCHLD ignored): it has exited with an unknown status. No kill, because
            // its pid may already be reused.
            #[cfg(unix)]
            Err(e) if e.raw_os_error() == Some(libc::ECHILD) => return Ok(Exit::Reaped(None)),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(ToolError::Failed(format!("zygo: down: {e}")));
            }
        }
    }
}

/// Reads `pipe` to EOF on a new thread. The first [`CAP`] bytes go into the returned buffer, which the caller can
/// read at any time; the rest is read and dropped, so the writer is never blocked. At EOF or on a read error the
/// thread sends one message on `done` and ends.
fn read_capped(mut pipe: impl Read + Send + 'static, done: mpsc::Sender<()>) -> Arc<Mutex<Vec<u8>>> {
    let buf = Arc::new(Mutex::new(Vec::new()));
    let keep = Arc::clone(&buf);
    std::thread::spawn(move || {
        let mut chunk = [0u8; 8192];
        loop {
            match pipe.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    let mut b = keep.lock().unwrap_or_else(|e| e.into_inner());
                    let room = CAP.saturating_sub(b.len());
                    b.extend_from_slice(&chunk[..n.min(room)]);
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
        let _ = done.send(());
    });
    buf
}
