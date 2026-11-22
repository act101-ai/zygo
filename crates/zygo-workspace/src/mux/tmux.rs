//! tmux implementation of the multiplexer trait. Owner: node W3 (W§13).

use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

#[cfg(unix)]
use zygo_config::repo_file::{exited_unreaped, kill_group};
use zygo_config::repo_file::kill_tree;

use super::{
    format_escape, is_gone_message, is_no_target_message, parse_version, session_target, window_target, HookScope,
    Multiplexer, MuxError, NewSession, NewWindow, SessionAnswer, Split, BOUND, PANE_FORMAT,
};

/// How often the runner checks for the exit.
const POLL: Duration = Duration::from_millis(5);

/// The refusal of an argument list that tmux would run through `sh -c` (W3 D7).
const SHORT_ARGV: &str = "tmux needs an argument list of 2 or more items";

/// The result of [`run_bounded`]. `status` is `None` when the kernel reaped the child (SIGCHLD ignored).
#[derive(Debug)]
pub(crate) struct Ran {
    pub status: Option<ExitStatus>,
    pub stdout: String,
    pub stderr: String,
}

/// How the child ended, as [`wait_exit`] saw it.
enum Exit {
    /// The child has exited and is not reaped yet, so its group can still be killed safely.
    #[cfg(unix)]
    Unreaped,
    /// The child is reaped (status known), or the kernel reaped it (`None`). No kill may be sent after this.
    Reaped(Option<ExitStatus>),
}

/// Runs `cmd` in its own process group (unix), with stdin from `/dev/null` and stdout and stderr piped (W3 D9). It
/// waits at most `limit` for the exit and then for EOF on both pipes.
///
/// No exit within the limit: the group is killed with `kill_tree` and the call gives [`MuxError::TimedOut`]. An exit,
/// but a pipe still held at the limit: the group is killed while the child is unreaped, then the child is reaped; if
/// the kernel already reaped it, no kill is sent. A reader thread that a process outside the group keeps blocked is
/// abandoned, never joined. The output is what was read, made into text with `from_utf8_lossy`.
pub(crate) fn run_bounded(mut cmd: Command, limit: Duration) -> Result<Ran, MuxError> {
    let deadline = Instant::now() + limit;
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    let mut child = cmd.spawn().map_err(|e| MuxError::Spawn { message: e.to_string() })?;
    let (Some(out), Some(err)) = (child.stdout.take(), child.stderr.take()) else {
        kill_tree(&mut child);
        return Err(MuxError::Spawn { message: "no pipes".into() });
    };
    let (tx, rx) = mpsc::channel::<()>();
    let out_buf = read_all(out, tx.clone());
    let err_buf = read_all(err, tx);
    let exit = wait_exit(&mut child, deadline)?;
    let mut eof = 0;
    while eof < 2 && rx.recv_timeout(deadline.saturating_duration_since(Instant::now())).is_ok() {
        eof += 1;
    }
    let status = match exit {
        #[cfg(unix)]
        Exit::Unreaped => {
            if eof < 2 {
                kill_group(&child);
            }
            child.wait().ok()
        }
        Exit::Reaped(s) => s,
    };
    let text = |b: &Mutex<Vec<u8>>| String::from_utf8_lossy(&b.lock().unwrap_or_else(|e| e.into_inner())).into_owned();
    Ok(Ran { status, stdout: text(&out_buf), stderr: text(&err_buf) })
}

/// Waits until `deadline` for the exit of `child`. On unix it first polls `exited_unreaped` (WNOWAIT), so the child
/// stays unreaped and its group can still be killed. The first `waitid` error (ECHILD when the kernel reaps by itself)
/// falls through to the `try_wait` loop, with no group kill after it. At the deadline the group is killed with
/// `kill_tree` and the call gives [`MuxError::TimedOut`].
fn wait_exit(child: &mut Child, deadline: Instant) -> Result<Exit, MuxError> {
    #[cfg(unix)]
    loop {
        match exited_unreaped(child) {
            Ok(true) => return Ok(Exit::Unreaped),
            Ok(false) if Instant::now() < deadline => std::thread::sleep(POLL),
            Ok(false) => {
                kill_tree(child);
                return Err(MuxError::TimedOut);
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
                return Err(MuxError::TimedOut);
            }
            // The kernel reaped the child: it has exited with an unknown status. No kill, because its pid may already
            // be reused.
            #[cfg(unix)]
            Err(e) if e.raw_os_error() == Some(libc::ECHILD) => return Ok(Exit::Reaped(None)),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(MuxError::Failed { message: format!("tmux: {e}") });
            }
        }
    }
}

/// Reads `pipe` to EOF on a new thread into the returned buffer, which the caller can read at any time. At EOF or on
/// a read error the thread sends one message on `done` and ends.
fn read_all(mut pipe: impl Read + Send + 'static, done: mpsc::Sender<()>) -> Arc<Mutex<Vec<u8>>> {
    let buf = Arc::new(Mutex::new(Vec::new()));
    let keep = Arc::clone(&buf);
    std::thread::spawn(move || {
        let mut chunk = [0u8; 8192];
        loop {
            match pipe.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => keep.lock().unwrap_or_else(|e| e.into_inner()).extend_from_slice(&chunk[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
        let _ = done.send(());
    });
    buf
}

/// The error of a failed call, from its stderr (W3 D1): gone, no target, or failed.
pub(crate) fn classify(ran: &Ran) -> MuxError {
    let message = ran.stderr.trim().to_string();
    if is_gone_message(&message) {
        MuxError::Gone { message }
    } else if is_no_target_message(&message) {
        MuxError::NoTarget { message }
    } else if message.is_empty() {
        let status = ran.status.map_or_else(|| "an unknown status".to_string(), |s| s.to_string());
        MuxError::Failed { message: format!("tmux failed with {status} and printed no error") }
    } else {
        MuxError::Failed { message }
    }
}

/// The stdout of a call that succeeded: exit 0, or a status that the kernel took (`None`) with empty stderr.
pub(crate) fn finish(ran: Ran) -> Result<String, MuxError> {
    let ok = match ran.status {
        Some(s) => s.success(),
        None => ran.stderr.trim().is_empty(),
    };
    if ok { Ok(ran.stdout) } else { Err(classify(&ran)) }
}

/// One line of output without its line end.
fn line(out: String) -> String {
    let s = out.strip_suffix('\n').unwrap_or(&out);
    s.strip_suffix('\r').unwrap_or(s).to_string()
}

/// The tmux implementation of [`Multiplexer`]. Every call passes `-S <socket>` when a socket is set (AU62).
#[derive(Debug, Clone)]
pub struct Tmux {
    program: PathBuf,
    socket: Option<PathBuf>,
}

impl Tmux {
    /// `program` is the tmux program. `socket` is passed as `-S` to every call; `None` passes no `-S`, so tmux (or a
    /// wrapper's own `-S`) chooses the server.
    pub fn new(program: PathBuf, socket: Option<PathBuf>) -> Tmux { Tmux { program, socket } }

    pub fn socket(&self) -> Option<&Path> { self.socket.as_deref() }

    /// The program, then `-S <socket>` when set, then `-u` when `utf8`, with `LC_ALL=C` when `c_locale`.
    fn command(&self, c_locale: bool, utf8: bool) -> Command {
        let mut c = Command::new(&self.program);
        if let Some(s) = &self.socket {
            c.arg("-S").arg(s);
        }
        if utf8 {
            c.arg("-u");
        }
        if c_locale {
            c.env("LC_ALL", "C");
        }
        c
    }

    /// One bounded call with `LC_ALL=C` and `-u`; its stdout, or the classified error.
    fn call<S: AsRef<OsStr>>(&self, args: &[S]) -> Result<String, MuxError> {
        let mut c = self.command(true, true);
        c.args(args);
        finish(run_bounded(c, BOUND)?)
    }

    /// [`Tmux::call`] for a command whose output is not used.
    fn run(&self, args: &[&str]) -> Result<(), MuxError> { self.call(args).map(|_| ()) }

    /// [`Tmux::call`] for `show-options`: empty output gives `None`.
    fn show(&self, args: &[&str]) -> Result<Option<String>, MuxError> {
        let v = line(self.call(args)?);
        Ok(if v.is_empty() { None } else { Some(v) })
    }
}

/// The `-c` value of a start directory: `#` doubled (AU55, W3 D6). A directory that is not UTF-8 is refused.
fn start_dir_arg(dir: &Path) -> Result<String, MuxError> {
    let s = dir
        .to_str()
        .ok_or_else(|| MuxError::Failed { message: format!("start directory is not UTF-8: {}", dir.to_string_lossy()) })?;
    Ok(format_escape(s))
}

/// Refuses an argument list of fewer than 2 items, which tmux would run through `sh -c` (W3 D7).
fn check_argv(argv: &[OsString]) -> Result<(), MuxError> {
    if argv.len() < 2 {
        return Err(MuxError::Failed { message: SHORT_ARGV.into() });
    }
    Ok(())
}

fn window_index(out: String) -> Result<u32, MuxError> {
    let v = line(out);
    v.parse().map_err(|_| MuxError::Failed { message: format!("tmux printed no window index: {v}") })
}

impl Multiplexer for Tmux {
    fn version(&self) -> Result<(u32, u32), MuxError> {
        let out = self.call(&["-V"])?;
        parse_version(&out).ok_or_else(|| MuxError::Failed {
            message: format!("unknown tmux version: {}", out.lines().next().unwrap_or("")),
        })
    }

    fn socket_path(&self, session: &str) -> Result<PathBuf, MuxError> {
        let v = line(self.call(&["display-message", "-p", "-t", &session_target(session), "#{socket_path}"])?);
        if v.is_empty() {
            return Err(MuxError::Failed { message: format!("tmux printed no socket path for {session}") });
        }
        Ok(PathBuf::from(v))
    }

    fn session_exists(&self, session: &str) -> SessionAnswer {
        match self.run(&["has-session", "-t", &session_target(session)]) {
            Ok(()) => SessionAnswer::Exists,
            Err(MuxError::Gone { .. }) => SessionAnswer::Gone,
            Err(e) => SessionAnswer::Error { message: e.to_string() },
        }
    }

    fn new_session(&self, spec: &NewSession<'_>) -> Result<u32, MuxError> {
        check_argv(spec.argv)?;
        let dir = start_dir_arg(spec.start_dir)?;
        let mut c = self.command(false, true);
        c.env_clear().envs(spec.env.iter().map(|(k, v)| (k, v)));
        c.args(["new-session", "-d", "-P", "-F", "#{window_index}", "-s", spec.name, "-c", &dir]);
        c.args(["-x", &spec.width.to_string(), "-y", &spec.height.to_string(), "--"]);
        c.args(spec.argv);
        window_index(finish(run_bounded(c, BOUND)?)?)
    }

    fn resize_window(&self, session: &str, window: u32, width: u32, height: u32) -> Result<(), MuxError> {
        let (w, h) = (width.to_string(), height.to_string());
        self.run(&["resize-window", "-t", &window_target(session, window), "-x", &w, "-y", &h])
    }

    fn unset_window_option(&self, session: &str, window: u32, name: &str) -> Result<(), MuxError> {
        self.run(&["set-option", "-w", "-u", "-t", &window_target(session, window), name])
    }

    fn set_window_option(&self, window: &str, name: &str, value: &str) -> Result<(), MuxError> {
        self.run(&["set-option", "-w", "-t", window, name, value])
    }

    fn set_env(&self, session: &str, name: &str, value: &str) -> Result<(), MuxError> {
        self.run(&["set-environment", "-t", &session_target(session), name, value])
    }

    fn hide_env(&self, session: &str, name: &str) -> Result<(), MuxError> {
        self.run(&["set-environment", "-r", "-t", &session_target(session), name])
    }

    fn set_session_option(&self, session: &str, name: &str, value: &str) -> Result<(), MuxError> {
        self.run(&["set-option", "-t", &session_target(session), name, value])
    }

    fn show_session_option(&self, session: &str, name: &str) -> Result<Option<String>, MuxError> {
        self.show(&["show-options", "-qv", "-t", &session_target(session), name])
    }

    fn set_hook(&self, session: &str, scope: HookScope, event: &str, command: &str) -> Result<(), MuxError> {
        match scope {
            HookScope::Session => self.run(&["set-hook", "-t", &session_target(session), event, command]),
            HookScope::Window(w) => self.run(&["set-hook", "-w", "-t", &window_target(session, w), event, command]),
        }
    }

    fn new_window(&self, spec: &NewWindow<'_>) -> Result<u32, MuxError> {
        check_argv(spec.argv)?;
        let dir = start_dir_arg(spec.start_dir)?;
        let mut args: Vec<OsString> =
            ["new-window", "-d", "-P", "-F", "#{window_index}", "-t"].iter().map(OsString::from).collect();
        args.push(session_target(spec.session).into());
        if let Some(n) = spec.name {
            args.push("-n".into());
            args.push(format_escape(n).into());
        }
        args.extend(["-c".into(), dir.into(), "--".into()]);
        args.extend(spec.argv.iter().cloned());
        window_index(self.call(&args)?)
    }

    fn split_window(&self, spec: &Split<'_>) -> Result<String, MuxError> {
        check_argv(spec.argv)?;
        let mut args: Vec<OsString> =
            ["split-window", "-d", "-P", "-F", "#{pane_id}", "-t", spec.target].iter().map(OsString::from).collect();
        if let Some(d) = spec.start_dir {
            args.push("-c".into());
            args.push(start_dir_arg(d)?.into());
        }
        args.push("--".into());
        args.extend(spec.argv.iter().cloned());
        let v = line(self.call(&args)?);
        if !v.starts_with('%') {
            return Err(MuxError::Failed { message: format!("tmux printed no pane id: {v}") });
        }
        Ok(v)
    }

    fn select_layout(&self, session: &str, window: u32, layout: &str) -> Result<(), MuxError> {
        self.run(&["select-layout", "-t", &window_target(session, window), layout])
    }

    fn select_window(&self, session: &str, window: u32) -> Result<(), MuxError> {
        self.run(&["select-window", "-t", &window_target(session, window)])
    }

    fn select_pane(&self, pane: &str) -> Result<(), MuxError> { self.run(&["select-pane", "-t", pane]) }

    fn zoom(&self, pane: &str) -> Result<(), MuxError> { self.run(&["resize-pane", "-Z", "-t", pane]) }

    fn list_panes(&self, session: &str) -> Result<Vec<String>, MuxError> {
        let out = self.call(&["list-panes", "-s", "-t", &session_target(session), "-F", PANE_FORMAT])?;
        Ok(out.lines().map(str::to_string).collect())
    }

    fn show_pane_option(&self, pane: &str, name: &str) -> Result<Option<String>, MuxError> {
        self.show(&["show-options", "-p", "-qv", "-t", pane, name])
    }

    fn set_pane_option(&self, pane: &str, name: &str, value: &str) -> Result<(), MuxError> {
        self.run(&["set-option", "-p", "-t", pane, name, value])
    }

    fn display(&self, target: &str, format: &str) -> Result<String, MuxError> {
        Ok(line(self.call(&["display-message", "-p", "-t", target, format])?))
    }

    fn kill_session(&self, session: &str) -> Result<(), MuxError> {
        self.run(&["kill-session", "-t", &session_target(session)])
    }

    fn attach(&self, session: &str) -> Result<i32, MuxError> {
        let mut c = self.command(false, false);
        c.args(["attach-session", "-t", &session_target(session)]);
        let status = c.status().map_err(|e| MuxError::Spawn { message: e.to_string() })?;
        Ok(status.code().unwrap_or(1))
    }

    fn switch_client(&self, session: &str) -> Result<(), MuxError> {
        self.run(&["switch-client", "-t", &session_target(session)])
    }
}
