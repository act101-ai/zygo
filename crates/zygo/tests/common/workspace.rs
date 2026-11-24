#![allow(dead_code)] // each test binary uses a part of this module

//! The workspace test harness (W§18 preamble): a scratch home whose state folder the test process and the `zygo`
//! binary name alike, a `bin` folder with the stub agents and a `tmux`, a test `$SHELL`, and, with tmux 3.2 or later,
//! a private tmux server. Bounded child waits (D15) and the ETXTBSY-safe first run of a written executable (D14).

use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::time::{Duration, Instant};

use zygo_core::paths::Paths;
use zygo_core::Agent;
use zygo_workspace::mux::testing::{require_tmux, require_tmux_at, PrivateServer};
use zygo_workspace::state::{Phase, Store, Workspace};

/// The workspace id of the tests.
pub const WS_ID: &str = "0123456789abcdef";

/// The longest time a test waits for one child process (a test bound).
pub const CHILD_BOUND: Duration = Duration::from_secs(60);

/// Waits for `child`: `try_wait` every 20 ms; after [`CHILD_BOUND`] it kills the child, reaps it and fails the test
/// with `<what> did not exit within 60s; killed` (D15).
pub fn wait_child(child: &mut Child, what: &str) -> ExitStatus {
    let t0 = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(st)) => return st,
            Ok(None) => {}
            Err(e) => panic!("{what}: wait failed: {e}"),
        }
        if t0.elapsed() >= CHILD_BOUND {
            let _ = child.kill();
            let _ = child.wait();
            panic!("{what} did not exit within {}s; killed", CHILD_BOUND.as_secs());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

type Reader = Option<std::thread::JoinHandle<Vec<u8>>>;

/// Reads `r` to its end on its own thread.
fn reader(r: Option<impl Read + Send + 'static>) -> Reader {
    r.map(|mut r| {
        std::thread::spawn(move || {
            let mut b = Vec::new();
            let _ = r.read_to_end(&mut b);
            b
        })
    })
}

fn joined(h: Reader) -> Vec<u8> {
    h.map(|h| h.join().unwrap_or_default()).unwrap_or_default()
}

/// [`wait_child`], with stdout and stderr read on two threads, so a full pipe cannot block the child (D15).
pub fn output_child(mut child: Child, what: &str) -> Output {
    let out = reader(child.stdout.take());
    let err = reader(child.stderr.take());
    let status = wait_child(&mut child, what);
    Output { status, stdout: joined(out), stderr: joined(err) }
}

/// A child that leads its own process group, held until the test reaps it (M2 review rounds 2 and 3, findings 6 and
/// 8). On every path (a normal exit, the [`CHILD_BOUND`] timeout, and `Drop` when a failed assert left the child) the
/// whole group gets SIGKILL first, so a process that the child left in its group (the agent) goes too, and only then
/// is the leader reaped. Until that reap the leader's pid, which is the group id, cannot name another process; no
/// signal is sent after it.
pub struct Guarded(Option<Child>);

/// SIGKILL to the process group `leader` leads. The caller has not reaped `leader`.
fn kill_group(leader: &Child) {
    // SAFETY: `kill` takes plain integers. `leader` is a child of this process that is not reaped yet (running or a
    // zombie), so its pid, the id of the group it leads, names no other process or group.
    unsafe {
        libc::kill(-(leader.id() as libc::pid_t), libc::SIGKILL);
    }
}

/// Whether `child` has exited, read without reaping it (`waitid` with `WNOWAIT`), so it stays a zombie whose pid
/// still names its group.
fn exited(child: &Child, what: &str) -> bool {
    // SAFETY: `info` is zeroed plain data that `waitid` fills; `WNOWAIT` leaves the child unreaped.
    unsafe {
        let mut info: libc::siginfo_t = std::mem::zeroed();
        let r = libc::waitid(libc::P_PID, child.id() as libc::id_t, &mut info, libc::WEXITED | libc::WNOHANG | libc::WNOWAIT);
        assert_eq!(r, 0, "{what}: waitid: {}", std::io::Error::last_os_error());
        info.si_pid() != 0
    }
}

impl Guarded {
    /// Spawns `c` as the leader of a new process group.
    pub fn spawn(mut c: Command) -> Guarded {
        use std::os::unix::process::CommandExt;
        c.process_group(0);
        Guarded(Some(c.spawn().unwrap()))
    }

    /// The pid of the child.
    pub fn id(&self) -> u32 {
        self.child().id()
    }

    fn child(&self) -> &Child {
        self.0.as_ref().expect("the child is reaped")
    }

    /// The stdin pipe of the child, when it was spawned with one.
    pub fn stdin(&mut self) -> Option<std::process::ChildStdin> {
        self.0.as_mut().and_then(|c| c.stdin.take())
    }

    /// Waits for the child (every 20 ms, at most [`CHILD_BOUND`]) without reaping it; then SIGKILL to its group; then
    /// the reap. After the bound the test fails with `<what> did not exit within 60s; killed`. The child counts as
    /// reaped after it.
    pub fn wait(&mut self, what: &str) -> ExitStatus {
        let c = self.0.as_mut().expect("the child is reaped");
        let t0 = Instant::now();
        let timed_out = loop {
            if exited(c, what) {
                break false;
            }
            if t0.elapsed() >= CHILD_BOUND {
                break true;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        kill_group(c);
        let st = c.wait();
        self.0 = None;
        let st = st.unwrap_or_else(|e| panic!("{what}: wait failed: {e}"));
        if timed_out {
            panic!("{what} did not exit within {}s; killed", CHILD_BOUND.as_secs());
        }
        st
    }

    /// [`output_child`] on the guarded child, with [`Guarded::wait`].
    pub fn output(mut self, what: &str) -> Output {
        let c = self.0.as_mut().expect("the child is reaped");
        let out = reader(c.stdout.take());
        let err = reader(c.stderr.take());
        let status = self.wait(what);
        Output { status, stdout: joined(out), stderr: joined(err) }
    }
}

impl Drop for Guarded {
    fn drop(&mut self) {
        if let Some(mut c) = self.0.take() {
            kill_group(&c);
            let _ = c.wait();
        }
    }
}

/// Makes an executable that this process just wrote safe to run (D14): runs `path --version` with stdin null; on a
/// spawn error `ETXTBSY` (another thread's fork still holds the file open for writing) it sleeps 10 ms and tries again
/// while less than 2 s passed; any other result ends the loop. The child is waited for through [`wait_child`].
pub fn ready_exe(path: &Path) {
    let t0 = Instant::now();
    loop {
        let spawned = Command::new(path)
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        match spawned {
            Ok(mut c) => {
                wait_child(&mut c, &format!("{} --version", path.display()));
                return;
            }
            Err(e) if e.raw_os_error() == Some(libc::ETXTBSY) && t0.elapsed() < Duration::from_secs(2) => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(_) => return,
        }
    }
}

/// The `zygo` binary of the package tests. `option_env!`, not `env!`: `crates/zygo-launch/tests/stub_member.rs` also
/// compiles this module (`#[path]` to `common/mod.rs`), and cargo defines `CARGO_BIN_EXE_zygo` only for the tests of
/// package `zygo`. The tests of `zygo-launch` never call the harness, so there the `None` arm is not reached.
pub(crate) fn zygo_exe() -> &'static str {
    match option_env!("CARGO_BIN_EXE_zygo") {
        Some(exe) => exe,
        None => panic!("the workspace harness runs only in the tests of package zygo"),
    }
}

/// `s` as one single-quoted shell word.
pub(crate) fn quote(s: &OsStr) -> String {
    format!("'{}'", s.to_string_lossy().replace('\'', r"'\''"))
}

fn write_mode(path: &Path, body: &str, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, body).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

/// The test `$SHELL`: records its cwd, its environment and, on request, its `/proc` status; on request it sends itself
/// SIGHUP.
const SHELL: &str = r#"#!/bin/sh
[ "$1" = --version ] && exit 0
PATH=/usr/bin:/bin
mkdir -p "$HOME/shell"; pwd > "$HOME/shell/cwd"; env > "$HOME/shell/env"
if [ -n "$M2_SHELL_STATUS" ]; then cat "/proc/$$/status" > "$HOME/shell/status"; fi
if [ -n "$M2_SHELL_HUP" ]; then kill -HUP $$; echo alive > "$HOME/shell/alive"; fi
"#;

/// One scratch home (D12: `root` is canonical).
pub struct Harness {
    pub home: tempfile::TempDir,
    pub root: PathBuf,
    pub bin: PathBuf,
    pub paths: Paths,
    pub server: Option<PrivateServer>,
}

impl Harness {
    /// A harness with a private tmux server, or `None` with the printed skip reason when no tmux 3.2 or later is on
    /// `PATH`.
    pub fn new() -> Option<Harness> {
        let p = require_tmux()?;
        Some(Harness::with_tmux(&p))
    }

    /// [`Harness::new`] with the tmux `tmux`.
    pub fn new_at(tmux: &Path) -> Option<Harness> {
        let p = require_tmux_at(tmux)?;
        Some(Harness::with_tmux(&p))
    }

    fn with_tmux(p: &Path) -> Harness {
        let h = Harness::base(Some(PrivateServer::start_with(p)));
        std::os::unix::fs::symlink(p, h.bin.join("tmux")).unwrap();
        h
    }

    /// A harness with no tmux server; `bin/tmux` fails every call (W§18 #72).
    pub fn no_tmux() -> Harness {
        let h = Harness::base(None);
        let t = h.bin.join("tmux");
        write_mode(&t, "#!/bin/sh\nexit 1\n", 0o755);
        ready_exe(&t);
        h
    }

    fn base(server: Option<PrivateServer>) -> Harness {
        let home = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(home.path()).unwrap();
        let bin = root.join("bin");
        std::fs::create_dir(&bin).unwrap();
        std::fs::write(root.join("config.toml"), "").unwrap();
        for name in ["claude", "opencode"] {
            let p = bin.join(name);
            std::fs::copy(super::stub_agent(), &p).unwrap();
            ready_exe(&p);
        }
        std::fs::create_dir(root.join("repo")).unwrap();
        let shell = root.join("shell.sh");
        write_mode(&shell, SHELL, 0o755);
        ready_exe(&shell);
        // The `Paths` of `HOME = root`, as `Paths::from_env` builds them on unix.
        let cache_dir = root.join(".cache/zygo");
        let data_dir = root.join(".local/share/zygo");
        let paths = Paths {
            config_file: root.join("config.toml"),
            sessions_dir: cache_dir.join("sessions"),
            marketplaces_dir: cache_dir.join("marketplaces"),
            plugin_store: data_dir.join("plugins"),
            approvals_file: data_dir.join("approvals.toml"),
            usage_db: data_dir.join("usage.db"),
            cache_dir,
            data_dir,
        };
        // In the test process the state folder is `<data_dir>/state` (its `HOME` is not `root`); the link makes the
        // binary with `HOME = root` name the same folder.
        let state = paths.data_dir.join("state");
        std::fs::create_dir_all(&state).unwrap();
        std::fs::create_dir_all(root.join(".local/state")).unwrap();
        std::os::unix::fs::symlink(&state, root.join(".local/state/zygo")).unwrap();
        Harness { home, root, bin, paths, server }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `root/repo`.
    pub fn repo(&self) -> PathBuf {
        self.root.join("repo")
    }

    /// The server socket, else `root/tmux/s`.
    pub fn socket(&self) -> PathBuf {
        match &self.server {
            Some(s) => s.socket().to_path_buf(),
            None => self.root.join("tmux/s"),
        }
    }

    /// The environment of every `zygo` that a test runs, with the update check off (auto-update design, Opt-out), so no
    /// pane and no test command reaches the release server; a test that needs the check removes the name.
    pub fn env(&self) -> Vec<(OsString, OsString)> {
        let tmp = self.root.join("tmux");
        std::fs::create_dir_all(&tmp).unwrap();
        let r = self.root.clone().into_os_string();
        vec![
            ("HOME".into(), r.clone()),
            ("APPDATA".into(), r.clone()),
            ("LOCALAPPDATA".into(), r),
            ("ZYGO_CONFIG".into(), self.root.join("config.toml").into()),
            ("PATH".into(), self.bin.clone().into()),
            ("TMUX_TMPDIR".into(), tmp.into()),
            ("SHELL".into(), self.root.join("shell.sh").into()),
            ("ZYGO_NO_UPDATE_CHECK".into(), "1".into()),
        ]
    }

    /// `zygo <args>` with only [`Harness::env`] and then `extra` (a later name wins), cwd `root`, stdin null, stdout and
    /// stderr piped.
    pub fn zygo(&self, args: &[&str], extra: &[(&str, &OsStr)]) -> Command {
        let mut c = Command::new(zygo_exe());
        c.args(args).env_clear().envs(self.env());
        for (k, v) in extra {
            c.env(k, v);
        }
        c.current_dir(&self.root).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        c
    }

    /// Writes `root/<name>.sh` (mode 0644): it waits until `root/<name>.go` exists, then runs `zygo <args>` with only
    /// [`Harness::env`] and `extra`. Returns the pane command `/bin/sh '<root>/<name>.sh'`, so the script is never
    /// exec'd (D14).
    pub fn script(&self, name: &str, args: &[&str], extra: &[(&str, &OsStr)]) -> String {
        let go = self.root.join(format!("{name}.go"));
        let mut vars = self.env();
        for (k, v) in extra {
            vars.retain(|(n, _)| n != OsStr::new(k));
            vars.push((OsString::from(k), v.to_os_string()));
        }
        let assigns: Vec<String> =
            vars.iter().map(|(k, v)| format!("{}={}", k.to_string_lossy(), quote(v))).collect();
        let words: Vec<String> = args.iter().map(|a| quote(OsStr::new(a))).collect();
        let body = format!(
            "while [ ! -e {} ]; do sleep 0.05; done\nexec env -i {} {} {}\n",
            quote(go.as_os_str()),
            assigns.join(" "),
            quote(OsStr::new(zygo_exe())),
            words.join(" ")
        );
        let path = self.root.join(format!("{name}.sh"));
        write_mode(&path, &body, 0o644);
        format!("/bin/sh {}", quote(path.as_os_str()))
    }

    /// `Store::open(&self.paths, ws_id)`.
    pub fn store(&self, ws_id: &str) -> Store {
        Store::open(&self.paths, ws_id)
    }

    /// One `Store::update` (5 s): the base fields of a workspace `app` that is up, then `f`.
    pub fn seed(&self, ws_id: &str, f: &mut dyn FnMut(&mut Workspace)) -> Workspace {
        let repo = self.repo();
        let socket = self.socket();
        self.store(ws_id)
            .update(Duration::from_secs(5), &mut |w| {
                w.name = "app".into();
                w.tmux_session = "zygo-app".into();
                w.common_dir = repo.join(".git");
                w.main_root = repo.clone();
                w.loadout = "solo".into();
                w.phase = Phase::Up;
                w.tmux_socket = Some(socket.clone());
                f(w);
                Ok(())
            })
            .unwrap()
    }

    /// A call to the private server; panics with the error.
    pub fn tmux(&self, args: &[&str]) -> String {
        let s = self.server.as_ref().expect("Harness::tmux needs a server (Harness::new)");
        s.run(args).unwrap_or_else(|e| panic!("tmux {args:?}: {e}"))
    }

    /// The stub output folder `root/out/<name>` and the stub environment for `kind`.
    pub fn stub(&self, kind: Agent, name: &str) -> (PathBuf, Vec<(&'static str, OsString)>) {
        let out = self.root.join("out").join(name);
        let env = vec![
            ("ZYGO_STUB_OUT", out.clone().into_os_string()),
            ("ZYGO_STUB_VERSION", zygo_config::builtin::min_agent_version(kind).to_string().into()),
        ];
        (out, env)
    }
}
