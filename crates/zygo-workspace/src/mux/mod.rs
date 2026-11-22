//! Multiplexer trait and the tmux test gate. Owner: node W3 (W§13).

pub mod tmux;

/// The tmux test gate and the private test server (W§19). Only the `test-hooks` feature builds it.
#[cfg(feature = "test-hooks")]
pub mod testing {
    use std::io::Write;
    use std::path::{Path, PathBuf};

    #[cfg(unix)]
    use super::Multiplexer;
    #[cfg(unix)]
    use super::{tmux::Tmux, MuxError, MIN_VERSION};

    /// The start of the line that a skipped tmux test writes to stderr.
    pub const SKIP_PREFIX: &str = "tmux tests skipped: ";

    /// `Ok(program)` when `program` is tmux 3.2 or later; else the reason to skip (W3 D12).
    #[cfg(unix)]
    pub fn tmux_gate(program: &Path) -> Result<PathBuf, String> {
        let p = program.display();
        match Tmux::new(program.to_path_buf(), None).version() {
            Ok(v) if v >= MIN_VERSION => Ok(program.to_path_buf()),
            Ok((major, minor)) => {
                Err(format!("{p} is tmux {major}.{minor}; the minimum is {}.{}", MIN_VERSION.0, MIN_VERSION.1))
            }
            Err(MuxError::Spawn { message }) => Err(format!("{p}: cannot run: {message}")),
            Err(MuxError::Failed { message }) if message.starts_with("unknown tmux version: ") => {
                Err(format!("{p}: {message}"))
            }
            Err(e) => Err(format!("{p}: tmux -V failed: {e}")),
        }
    }

    /// The tmux tests run on unix only (W3 D15).
    #[cfg(not(unix))]
    pub fn tmux_gate(_program: &Path) -> Result<PathBuf, String> { Err("tmux tests run on unix only".into()) }

    /// Writes the skip line. `write_all` on stderr, not `eprintln!`, because the test harness captures only the print
    /// macros.
    fn skip(reason: &str) {
        let _ = std::io::stderr().write_all(format!("{SKIP_PREFIX}{reason}\n").as_bytes());
    }

    /// [`tmux_gate`], with the skip line on stderr when it refuses.
    pub fn require_tmux_at(program: &Path) -> Option<PathBuf> {
        match tmux_gate(program) {
            Ok(p) => Some(p),
            Err(reason) => {
                skip(&reason);
                None
            }
        }
    }

    /// The first `tmux` on `PATH` (a file, with an execute bit on unix), through [`require_tmux_at`]. When there is
    /// none it writes `tmux tests skipped: tmux not found on PATH` and gives `None`.
    pub fn require_tmux() -> Option<PathBuf> {
        let path = std::env::var_os("PATH").unwrap_or_default();
        let found = std::env::split_paths(&path).map(|d| d.join("tmux")).find(|p| runnable(p));
        match found {
            Some(p) => require_tmux_at(&p),
            None => {
                skip("tmux not found on PATH");
                None
            }
        }
    }

    #[cfg(unix)]
    fn runnable(p: &Path) -> bool {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }

    #[cfg(not(unix))]
    fn runnable(p: &Path) -> bool { p.is_file() }

    /// A private tmux server for one test (W§19, W3 D11): its own mode-0700 folder in `/tmp`, the socket `<dir>/s`,
    /// and a wrapper `<dir>/tmux` that passes `-S <socket> -f /dev/null`. No call reaches the default server.
    #[cfg(unix)]
    #[derive(Debug)]
    pub struct PrivateServer {
        dir: PathBuf,
        socket: PathBuf,
        program: PathBuf,
        wrapper: PathBuf,
    }

    #[cfg(unix)]
    impl PrivateServer {
        /// Makes the folder and the wrapper; starts no server (the first call that needs one starts it through the
        /// wrapper). Panics when [`require_tmux`] gives none: a test calls it first.
        pub fn start() -> PrivateServer {
            use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
            use std::sync::atomic::{AtomicU64, Ordering};
            use std::time::{Duration, Instant};
            static NEXT: AtomicU64 = AtomicU64::new(0);

            let Some(program) = require_tmux() else {
                panic!("PrivateServer::start: no tmux 3.2 or later on PATH; call require_tmux first");
            };
            assert!(
                !program.as_os_str().as_encoded_bytes().contains(&b'\''),
                "PrivateServer::start: the tmux path {} holds a quote",
                program.display()
            );
            let n = NEXT.fetch_add(1, Ordering::Relaxed);
            let dir = PathBuf::from(format!("/tmp/zygo-mux-{}-{n}-{:08x}", std::process::id(), rand::random::<u32>()));
            std::fs::DirBuilder::new().mode(0o700).create(&dir).expect("PrivateServer::start: create the folder");
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).expect("PrivateServer::start: chmod");
            let socket = dir.join("s");
            assert!(socket.as_os_str().len() <= 100, "PrivateServer::start: socket path {} is too long", socket.display());
            let wrapper = dir.join("tmux");
            let body = format!(
                "#!/bin/sh\nexec '{}' -S '{}' -f /dev/null \"$@\"\n",
                program.display(),
                socket.display()
            );
            std::fs::write(&wrapper, body).expect("PrivateServer::start: write the wrapper");
            std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755))
                .expect("PrivateServer::start: chmod the wrapper");
            let server = PrivateServer { dir, socket, program, wrapper };

            // The wrapper was just written: another thread's fork can still hold it open for writing (ETXTBSY).
            let busy = std::io::Error::from_raw_os_error(libc::ETXTBSY).to_string();
            let t0 = Instant::now();
            loop {
                let mut c = std::process::Command::new(&server.wrapper);
                c.arg("-V");
                match super::tmux::run_bounded(c, super::BOUND).and_then(super::tmux::finish) {
                    Ok(_) => break,
                    Err(MuxError::Spawn { message }) if message == busy && t0.elapsed() < Duration::from_secs(2) => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(e) => panic!("PrivateServer::start: {} -V failed: {e}", server.wrapper.display()),
                }
            }
            server
        }

        /// The socket path, `<dir>/s`.
        pub fn socket(&self) -> &Path { &self.socket }

        /// The real tmux program.
        pub fn program(&self) -> &Path { &self.program }

        /// The wrapper `<dir>/tmux`, which passes `-S <socket> -f /dev/null`.
        pub fn wrapper(&self) -> &Path { &self.wrapper }

        /// `Tmux::new(wrapper, Some(socket))`.
        pub fn tmux(&self) -> Tmux { Tmux::new(self.wrapper.clone(), Some(self.socket.clone())) }

        /// Runs the real program with `-S <socket> -f /dev/null -u` and `LC_ALL=C`, bounded. Exit 0 gives stdout
        /// with trailing white space removed; other results are classified as the trait's calls are.
        pub fn run(&self, args: &[&str]) -> Result<String, MuxError> {
            let ran = super::tmux::run_bounded(self.command(args), super::BOUND)?;
            super::tmux::finish(ran).map(|s| s.trim_end().to_string())
        }

        /// `kill-server`, then waits at most 5 s until `list-sessions` fails.
        pub fn kill_server(&self) {
            use std::time::{Duration, Instant};
            let _ = self.run(&["kill-server"]);
            let t0 = Instant::now();
            while self.run(&["list-sessions"]).is_ok() && t0.elapsed() < Duration::from_secs(5) {
                std::thread::sleep(Duration::from_millis(20));
            }
        }

        fn command(&self, args: &[&str]) -> std::process::Command {
            let mut c = std::process::Command::new(&self.program);
            c.arg("-S").arg(&self.socket).args(["-f", "/dev/null", "-u"]).args(args).env("LC_ALL", "C");
            c
        }
    }

    #[cfg(unix)]
    impl Drop for PrivateServer {
        /// `kill-server` (result ignored), then removes the folder.
        fn drop(&mut self) {
            let _ = super::tmux::run_bounded(self.command(&["kill-server"]), super::BOUND);
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Where `set_hook` puts a hook (W3 D14).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookScope {
    /// `set-hook -t =<s>:`.
    Session,
    /// `set-hook -w -t =<s>:<w>`.
    Window(u32),
}

/// The arguments of `new_session`. `start_dir` is raw: the implementation escapes `#` (W3 D6).
#[derive(Debug, Clone, Copy)]
pub struct NewSession<'a> {
    pub name: &'a str,
    pub start_dir: &'a Path,
    pub width: u32,
    pub height: u32,
    /// The whole environment of the tmux client that creates the session (the cleaned caller environment).
    pub env: &'a [(OsString, OsString)],
    /// The first pane's command, 2 or more items.
    pub argv: &'a [OsString],
}

/// The arguments of `new_window`. `name` and `start_dir` are raw: the implementation escapes `#` (W3 D6). A name with
/// a control character is refused ([`CONTROL_NAME_MESSAGE`]).
#[derive(Debug, Clone, Copy)]
pub struct NewWindow<'a> {
    pub session: &'a str,
    pub name: Option<&'a str>,
    pub start_dir: &'a Path,
    /// The new pane's command, 2 or more items.
    pub argv: &'a [OsString],
}

/// The arguments of `split_window`. `target` is a pane id `%N` or a window target; `start_dir` is raw (W3 D6).
#[derive(Debug, Clone, Copy)]
pub struct Split<'a> {
    pub target: &'a str,
    pub start_dir: Option<&'a Path>,
    /// The new pane's command, 2 or more items.
    pub argv: &'a [OsString],
}

/// The refusal of a window name that holds a control character (W3 round 2 B1): tmux 3.7c refuses such a name, and
/// tmux 3.2a prints it raw, so a newline would split a `list-panes` line.
pub const CONTROL_NAME_MESSAGE: &str = "window name holds a control character";

/// `Err(Failed)` with [`CONTROL_NAME_MESSAGE`] when `name` holds a character for which `char::is_control` is true:
/// C0 (newline, tab and the rest), DEL and C1.
pub(crate) fn check_window_name(name: &str) -> Result<(), MuxError> {
    if name.chars().any(char::is_control) {
        return Err(MuxError::Failed { message: CONTROL_NAME_MESSAGE.into() });
    }
    Ok(())
}

/// `Err(Failed)` unless `name` is a tmux option, hook or environment name that tmux cannot read as a flag: not empty,
/// not starting with `-`, and only `[A-Za-z0-9_@-]` (W3 round 2 M2).
pub(crate) fn check_name(name: &str) -> Result<(), MuxError> {
    let ok = !name.is_empty()
        && !name.starts_with('-')
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'@' | b'-'));
    if !ok {
        return Err(MuxError::Failed { message: format!("invalid tmux option or environment name: {name:?}") });
    }
    Ok(())
}

/// `Err(Failed)` unless `session` is `zygo-<name>` with `<name>` valid for `identity::valid_name` (AU2, AU84; W3
/// round 2 M3), so a target holds no `#`, `:`, `.` or flag.
pub(crate) fn check_session(session: &str) -> Result<(), MuxError> {
    if !session.strip_prefix("zygo-").is_some_and(crate::identity::valid_name) {
        return Err(MuxError::Failed { message: format!("invalid tmux session name: {session:?}") });
    }
    Ok(())
}

/// The exact target of a session: `=<session>:`.
pub fn session_target(session: &str) -> String { format!("={session}:") }

/// The exact target of a window: `=<session>:<window>`.
pub fn window_target(session: &str, window: u32) -> String { format!("={session}:{window}") }

/// The terminal multiplexer operations of W§7.4, W§8 and W§9 (W§13). Every call except `attach` has the bound
/// [`BOUND`]. A session argument is a session name; the implementation makes the exact target.
///
/// Errors, for every operation unless its line says more:
/// - `Failed` before any tmux call for a refused argument: a session name that is not `zygo-<name>` (AU84), an
///   option, hook or environment name outside `[A-Za-z0-9_@-]` or starting with `-`, a window name with a control
///   character, a start directory that is not UTF-8, or an argument list of fewer than 2 items.
/// - `Gone`: tmux said that the session or its server is gone ([`is_gone_message`]).
/// - `NoTarget`: the window or pane does not exist and the session still does, or is not known. An operation whose
///   target names the session (`=<s>:<w>`) checks `has-session` after a `NoTarget` and gives `Gone` when the session
///   is gone. A pane id `%N` names no session, so a pane operation keeps `NoTarget`; the caller checks
///   [`Multiplexer::session_exists`].
/// - `Failed` with tmux's stderr for any other tmux error; `TimedOut` after [`BOUND`]; `Spawn` when tmux cannot run;
///   `Failed` when tmux exits but a process outside its group still holds its output at the bound.
pub trait Multiplexer {
    /// `tmux -V`, read with [`parse_version`]. Output without a version gives `Failed`.
    fn version(&self) -> Result<(u32, u32), MuxError>;
    /// `has-session -t =<s>:`, then `display-message -p -t =<s>: -- #{socket_path}`. A missing session gives `Gone`
    /// (the display alone prints the server's path for any session name).
    fn socket_path(&self, session: &str) -> Result<PathBuf, MuxError>;
    /// `has-session -t =<s>:`, with the AU87 answer kinds; a refused name gives `Error`.
    fn session_exists(&self, session: &str) -> SessionAnswer;
    /// `new-session -d -P -F #{window_index} -s <s> -c <dir> -x <w> -y <h> -- <argv>`, with exactly `spec.env`;
    /// returns the first window index.
    fn new_session(&self, spec: &NewSession<'_>) -> Result<u32, MuxError>;
    /// `resize-window -t =<s>:<w> -x <width> -y <height>`. A missing window: `NoTarget`, or `Gone` with the session.
    fn resize_window(&self, session: &str, window: u32, width: u32, height: u32) -> Result<(), MuxError>;
    /// `set-option -w -u -t =<s>:<w> -- <name>`. A missing window: `NoTarget`, or `Gone` with the session.
    fn unset_window_option(&self, session: &str, window: u32, name: &str) -> Result<(), MuxError>;
    /// `set-option -w -t <window> -- <name> <value>`; `window` is a window target `=<s>:<w>` (a missing window gives
    /// `NoTarget`, or `Gone` with the session) or a pane id (`NoTarget`).
    fn set_window_option(&self, window: &str, name: &str, value: &str) -> Result<(), MuxError>;
    /// `set-environment -t =<s>: -- <name> <value>`.
    fn set_env(&self, session: &str, name: &str, value: &str) -> Result<(), MuxError>;
    /// `set-environment -r -t =<s>: -- <name>`: the name is marked as removed for new panes.
    fn hide_env(&self, session: &str, name: &str) -> Result<(), MuxError>;
    /// `set-option -t =<s>: -- <name> <value>`.
    fn set_session_option(&self, session: &str, name: &str, value: &str) -> Result<(), MuxError>;
    /// `show-options -qv -t =<s>: -- <name>`. `None` means unset or no such session: a missing session gives no
    /// error.
    fn show_session_option(&self, session: &str, name: &str) -> Result<Option<String>, MuxError>;
    /// `set-hook -t =<s>: -- <event> <command>`, or `set-hook -w -t =<s>:<w> -- …` for [`HookScope::Window`] (a
    /// missing window: `NoTarget`, or `Gone` with the session).
    fn set_hook(&self, session: &str, scope: HookScope, event: &str, command: &str) -> Result<(), MuxError>;
    /// `new-window -d -P -F #{window_index} -t =<s>: -c <dir> -- <argv>`, then, with a name,
    /// `rename-window -t =<s>:<index> -- <name>`; returns the new window index. tmux stores a renamed window's name
    /// vis-encoded on 3.2a and 3.7c, so [`parse_pane_line`] reads it back exactly. A failed rename returns its error
    /// and leaves the window with tmux's own name.
    fn new_window(&self, spec: &NewWindow<'_>) -> Result<u32, MuxError>;
    /// `split-window -d -P -F #{pane_id} -t <target> [-c <dir>] -- <argv>`; returns the new pane id. A missing window
    /// target: `NoTarget`, or `Gone` with the session; a missing pane id: `NoTarget`.
    fn split_window(&self, spec: &Split<'_>) -> Result<String, MuxError>;
    /// `select-layout -t =<s>:<w> -- <layout>`. A missing window: `NoTarget`, or `Gone` with the session.
    fn select_layout(&self, session: &str, window: u32, layout: &str) -> Result<(), MuxError>;
    /// `select-window -t =<s>:<w>`. A missing window: `NoTarget`, or `Gone` with the session.
    fn select_window(&self, session: &str, window: u32) -> Result<(), MuxError>;
    /// `select-pane -t <pane>`. A missing pane: `NoTarget`.
    fn select_pane(&self, pane: &str) -> Result<(), MuxError>;
    /// `resize-pane -Z -t <pane>`: toggles the zoom of the pane's window. A missing pane: `NoTarget`.
    fn zoom(&self, pane: &str) -> Result<(), MuxError>;
    /// `list-panes -s -t =<s>: -F` [`PANE_FORMAT`]: one line per pane (stdout split on `\n` only), parsed with
    /// [`parse_pane_line`].
    fn list_panes(&self, session: &str) -> Result<Vec<String>, MuxError>;
    /// `show-options -p -qv -t <pane> -- <name>`. `None` means unset or no such pane: a missing pane gives no error.
    fn show_pane_option(&self, pane: &str, name: &str) -> Result<Option<String>, MuxError>;
    /// `set-option -p -t <pane> -- <name> <value>`. A missing pane: `NoTarget`.
    fn set_pane_option(&self, pane: &str, name: &str, value: &str) -> Result<(), MuxError>;
    /// `display-message -p -t <target> -- <format>`; the output without its last `\n`. A missing target gives no
    /// error: a missing session or pane gives empty fields, and a missing window gives the session's current window's
    /// value.
    fn display(&self, target: &str, format: &str) -> Result<String, MuxError>;
    /// `kill-session -t =<s>:`.
    fn kill_session(&self, session: &str) -> Result<(), MuxError>;
    /// `attach-session -t =<s>:`, with the terminal and the caller's environment and no bound; returns tmux's exit
    /// code.
    fn attach(&self, session: &str) -> Result<i32, MuxError>;
    /// `switch-client -t =<s>:`.
    fn switch_client(&self, session: &str) -> Result<(), MuxError>;
}

/// The oldest tmux that workspaces support (AU7).
pub const MIN_VERSION: (u32, u32) = (3, 2);

/// The bound of every tmux call except `attach` (W§13).
pub const BOUND: Duration = Duration::from_secs(2);

/// The answer to "does this tmux session exist" (AU87 (1)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionAnswer {
    Exists,
    /// tmux said explicitly that the session or its server is gone ([`is_gone_message`]).
    Gone,
    /// Any other tmux error: a timeout, a permission error, a spawn error or another message.
    Error { message: String },
}

/// The error of a tmux call (W3 D1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MuxError {
    /// The session or its server is gone ([`is_gone_message`]).
    Gone { message: String },
    /// The window or pane does not exist ([`is_no_target_message`]); the session can still exist.
    NoTarget { message: String },
    /// Any other failure, with tmux's stderr (trimmed) or the refusal text.
    Failed { message: String },
    /// The call did not end within [`BOUND`].
    TimedOut,
    /// tmux could not be started.
    Spawn { message: String },
}

impl MuxError {
    pub fn is_gone(&self) -> bool { matches!(self, MuxError::Gone { .. }) }

    /// A `Failed` error whose message holds `no space for` (a split that does not fit).
    pub fn is_no_space(&self) -> bool { matches!(self, MuxError::Failed { message } if message.contains("no space for")) }
}

impl std::fmt::Display for MuxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MuxError::Gone { message }
            | MuxError::NoTarget { message }
            | MuxError::Failed { message }
            | MuxError::Spawn { message } => f.write_str(message),
            MuxError::TimedOut => write!(f, "tmux timed out after {} s", BOUND.as_secs()),
        }
    }
}

impl std::error::Error for MuxError {}

/// Whether the first line of tmux's stderr says that the session or its server is gone (AU87 (1), W3 D2).
pub fn is_gone_message(stderr: &str) -> bool {
    let l = stderr.lines().next().unwrap_or("").trim_end();
    l.starts_with("can't find session")
        || l.starts_with("no such session:")
        || l.starts_with("no server running on ")
        || l == "server exited unexpectedly"
        || (l.starts_with("error connecting to ")
            && (l.ends_with(" (No such file or directory)") || l.ends_with(" (Connection refused)")))
}

/// Whether the first line of tmux's stderr says that a window or pane does not exist (W3 D1).
pub fn is_no_target_message(stderr: &str) -> bool {
    let l = stderr.lines().next().unwrap_or("");
    ["can't find window:", "no such window:", "can't find pane:", "no such pane:"].iter().any(|p| l.starts_with(p))
}

/// The first `<digits>.<digits>` after `tmux ` in `tmux -V` output: `tmux 3.2a` gives (3, 2), `tmux next-3.6` gives
/// (3, 6), and `tmux master` gives none (W3 D13).
pub fn parse_version(out: &str) -> Option<(u32, u32)> {
    let rest = out.lines().next()?.strip_prefix("tmux ")?;
    let b = rest.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if !b[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let major_end = i + b[i..].iter().take_while(|c| c.is_ascii_digit()).count();
        if b.get(major_end) == Some(&b'.') {
            let minor_len = b[major_end + 1..].iter().take_while(|c| c.is_ascii_digit()).count();
            if minor_len > 0 {
                let major = rest[i..major_end].parse().ok()?;
                let minor = rest[major_end + 1..major_end + 1 + minor_len].parse().ok()?;
                return Some((major, minor));
            }
        }
        i = major_end;
    }
    None
}

/// The `list-panes -F` format (W§19, AU99): 13 fields split by `|`. The window name and the pane key are read through
/// `q:`, so a `|` in them arrives as `\|`. The pane path is last and takes the rest of the line. Three `s///`
/// substitutions double each `\` of the path and turn LF and CR into `\n` and `\r`, so each pane is one line on tmux
/// 3.2a and later (`q:` escapes neither LF nor CR on 3.2a or 3.7c; W3 round 2 M1 probe).
pub const PANE_FORMAT: &str = "#{window_index}|#{q:window_name}|#{automatic-rename}|#{window_layout}|#{window_width}|#{window_height}|#{window_active}|#{window_zoomed_flag}|#{pane_id}|#{pane_index}|#{pane_active}|#{q:@zygo-pane}|#{s/\\\\/\\\\\\\\/;s/\n/\\\\n/;s/\r/\\\\r/:pane_current_path}";

/// The number of fields of [`PANE_FORMAT`].
pub const PANE_FIELDS: usize = 13;

/// One window, from a line of [`PANE_FORMAT`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowRow {
    pub index: u32,
    /// The name as the user typed it (`q:` escapes removed, then [`vis_decode`]).
    pub name: String,
    pub automatic_rename: bool,
    pub layout: String,
    pub width: u32,
    pub height: u32,
    pub active: bool,
    pub zoomed: bool,
}

/// One pane, from a line of [`PANE_FORMAT`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneRow {
    /// The pane id, `%N`.
    pub id: String,
    pub index: u32,
    pub active: bool,
    /// The `@zygo-pane` value (`q:` escapes removed); empty when unset.
    pub key: String,
    pub cwd: PathBuf,
}

/// Why a `list-panes` line does not parse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    TooFewFields { found: usize },
    BadField { field: &'static str },
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::TooFewFields { found } => write!(f, "list-panes line has {found} of {PANE_FIELDS} fields"),
            ParseError::BadField { field } => write!(f, "list-panes field {field} is not valid"),
        }
    }
}

impl std::error::Error for ParseError {}

/// Reverses tmux's vis encoding of a window name (`utf8_stravis` with `VIS_OCTAL|VIS_CSTYLE|VIS_TAB|VIS_NL`): `\\`
/// gives `\`, `\` and three octal digits give that byte, and `\a \b \t \n \v \f \r \0` give their control bytes.
/// Every other character stays unchanged. The bytes are made into text with `from_utf8_lossy`.
pub fn vis_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'\\' || i + 1 >= b.len() {
            out.push(b[i]);
            i += 1;
            continue;
        }
        let octal = b.get(i + 1..i + 4).filter(|d| d.iter().all(|c| (b'0'..=b'7').contains(c)));
        if let Some(d) = octal {
            let v = d.iter().fold(0u32, |a, c| a * 8 + u32::from(c - b'0'));
            if let Ok(byte) = u8::try_from(v) {
                out.push(byte);
                i += 4;
                continue;
            }
        }
        let control = match b[i + 1] {
            b'\\' => Some(b'\\'),
            b'a' => Some(0x07),
            b'b' => Some(0x08),
            b't' => Some(b'\t'),
            b'n' => Some(b'\n'),
            b'v' => Some(0x0b),
            b'f' => Some(0x0c),
            b'r' => Some(b'\r'),
            b'0' => Some(0),
            _ => None,
        };
        match control {
            Some(c) => {
                out.push(c);
                i += 2;
            }
            None => {
                out.push(b'\\');
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Removes the `q:` escapes: `\X` gives `X`.
fn q_unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\'
            && let Some(n) = chars.next()
        {
            out.push(n);
            continue;
        }
        out.push(c);
    }
    out
}

/// Reverses the path encoding of [`PANE_FORMAT`]: `\\` gives `\`, `\n` gives LF and `\r` gives CR. Every other
/// character stays unchanged.
fn path_decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            let decoded = match chars.peek() {
                Some('\\') => Some('\\'),
                Some('n') => Some('\n'),
                Some('r') => Some('\r'),
                _ => None,
            };
            if let Some(d) = decoded {
                out.push(d);
                chars.next();
                continue;
            }
        }
        out.push(c);
    }
    out
}

/// Splits a line into at most [`PANE_FIELDS`] fields. Each of the first 12 ends at the first `|` that no `\` escapes;
/// the last field is the rest of the line, raw.
fn split_fields(line: &str) -> Vec<&str> {
    let b = line.as_bytes();
    let mut fields = Vec::with_capacity(PANE_FIELDS);
    let mut start = 0;
    let mut i = 0;
    while fields.len() < PANE_FIELDS - 1 {
        if i >= b.len() {
            fields.push(&line[start..]);
            return fields;
        }
        match b[i] {
            b'\\' => i += 2,
            b'|' => {
                fields.push(&line[start..i]);
                i += 1;
                start = i;
            }
            _ => i += 1,
        }
    }
    fields.push(&line[start..]);
    fields
}

fn number(s: &str, field: &'static str) -> Result<u32, ParseError> {
    if s.is_empty() || !s.bytes().all(|c| c.is_ascii_digit()) {
        return Err(ParseError::BadField { field });
    }
    s.parse().map_err(|_| ParseError::BadField { field })
}

fn flag(s: &str, field: &'static str) -> Result<bool, ParseError> {
    match s {
        "0" => Ok(false),
        "1" => Ok(true),
        _ => Err(ParseError::BadField { field }),
    }
}

/// Parses one line of `list-panes -F` [`PANE_FORMAT`] (AU99). Fields 1-12 are read from the left, and field 13, the
/// pane path, is the rest of the line, with the path encoding of [`PANE_FORMAT`] reversed. A line with fewer than 13
/// fields, or a typed field that does not parse, is refused.
pub fn parse_pane_line(line: &str) -> Result<(WindowRow, PaneRow), ParseError> {
    let f = split_fields(line);
    if f.len() < PANE_FIELDS {
        return Err(ParseError::TooFewFields { found: f.len() });
    }
    let window = WindowRow {
        index: number(f[0], "window_index")?,
        name: vis_decode(&q_unescape(f[1])),
        automatic_rename: flag(f[2], "automatic-rename")?,
        layout: f[3].to_string(),
        width: number(f[4], "window_width")?,
        height: number(f[5], "window_height")?,
        active: flag(f[6], "window_active")?,
        zoomed: flag(f[7], "window_zoomed_flag")?,
    };
    if !f[8].starts_with('%') {
        return Err(ParseError::BadField { field: "pane_id" });
    }
    let pane = PaneRow {
        id: f[8].to_string(),
        index: number(f[9], "pane_index")?,
        active: flag(f[10], "pane_active")?,
        key: q_unescape(f[11]),
        cwd: PathBuf::from(path_decode(f[12])),
    };
    Ok((window, pane))
}

/// The AU54 refusal text for a zygo path or a tmux socket path that a tmux command string cannot quote.
pub const UNSAFE_PATH_MESSAGE: &str =
    "zygo: the zygo path or tmux socket path holds a quote, a backslash, a $ or a control character; workspaces cannot use it";

/// A path that holds `'`, `"`, `\`, `$` or a control character, or is not UTF-8 (AU54).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnsafePath;

impl std::fmt::Display for UnsafePath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(UNSAFE_PATH_MESSAGE) }
}

impl std::error::Error for UnsafePath {}

/// The path as text, when it is UTF-8 and holds no `'`, `"`, `\`, `$` and no control character (AU54).
pub fn check_path(path: &Path) -> Result<&str, UnsafePath> {
    let s = path.to_str().ok_or(UnsafePath)?;
    if s.chars().any(|c| matches!(c, '\'' | '"' | '\\' | '$') || c.is_control()) {
        return Err(UnsafePath);
    }
    Ok(s)
}

/// Doubles each `#`, so tmux reads the value as literal text and expands no format in it (AU55, W-C8).
pub fn format_escape(s: &str) -> String { s.replace('#', "##") }

/// The tmux hook command of AU54: `run-shell -b "'<zygo>' ws snapshot --ws-id <id> --event <event>"`, with each `#`
/// of the path doubled. `Err(UnsafePath)` also when `ws_id` fails `identity::valid_ws_id` or `event` is not a tmux
/// hook name (`[a-z-]+`), because either one would be unquoted inside the command (W3 round 2 M7; checked in every
/// build).
pub fn hook_command(zygo: &Path, ws_id: &str, event: &str) -> Result<String, UnsafePath> {
    if !crate::identity::valid_ws_id(ws_id)
        || event.is_empty()
        || !event.bytes().all(|b| b.is_ascii_lowercase() || b == b'-')
    {
        return Err(UnsafePath);
    }
    let path = check_path(zygo)?;
    Ok(format!("run-shell -b \"'{}' ws snapshot --ws-id {ws_id} --event {event}\"", format_escape(path)))
}
