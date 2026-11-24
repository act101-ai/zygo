//! Member fixtures for the AU105 mirror tests (M4). Unix only: workspaces need tmux (OP5).
#![cfg(unix)]
#![allow(dead_code)] // each test binary uses a part of this module

use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use zygo_core::paths::Paths;
use zygo_core::session_dir::SessionDir;
use zygo_core::session_view::{Member, SessionView, ViewWorkspace};
use zygo_core::Agent;
use zygo_workspace::state::{MemberRecord, MemberState, Phase, Store};

pub const WS_ID: &str = "0123456789abcdef";
pub const WS_KEY: &str = "kaaaaaaaa";
pub const OTHER_KEY: &str = "kbbbbbbbb";
pub const SID: &str = "01JTESTSESSIONAAAAAAAAAAAA";
pub const OTHER: &str = "01JOTHERSESSIONAAAAAAAAAAA";
pub const NOT_SAVED: &str = "; not saved for the workspace";

/// `view.json` in `dir` (made when missing) for zygo session `sid`, with the marker `{ws_id, pane}` (AU58).
pub fn view(dir: &Path, sid: &str, ws_id: &str, pane: &str) {
    let sd = SessionDir::new(dir.to_path_buf());
    sd.create().unwrap();
    let member = Member { ws_id: ws_id.into(), pane: pane.into(), ..Default::default() };
    SessionView {
        session: sid.into(),
        workspace: Some(ViewWorkspace { member, resume_note: None, ..Default::default() }),
        ..Default::default()
    }
    .save(&sd)
    .unwrap();
}

/// A record in state `running` whose committed zygo session is `sid`.
pub fn running(sid: &str) -> MemberRecord {
    MemberRecord { zygo_session: Some(sid.into()), state: MemberState::Running, ..MemberRecord::new(Agent::ClaudeCode) }
}

/// The state of workspace `app` (`WS_ID`) in phase `up` with `records` and `tmux_socket = socket`. The test process
/// names the state folder `<data_dir>/state` (its `HOME` is not `home`); the link `home/.local/state/zygo` makes the
/// binary (`HOME = home`) name the same files (FIX-17), as `member_home` in `cli_surface.rs`. Once per home.
pub fn state(paths: &Paths, home: &Path, records: &[(&str, MemberRecord)], socket: Option<&Path>) -> Store {
    std::fs::create_dir_all(paths.data_dir.join("state")).unwrap();
    std::fs::create_dir_all(home.join(".local/state")).unwrap();
    std::os::unix::fs::symlink(paths.data_dir.join("state"), home.join(".local/state/zygo")).unwrap();
    let store = Store::open(paths, WS_ID);
    store
        .update(Duration::from_secs(5), &mut |w| {
            w.name = "app".into();
            w.tmux_session = "zygo-app".into();
            w.common_dir = "/srv/app/.git".into();
            w.main_root = "/srv/app".into();
            w.loadout = "solo".into();
            w.phase = Phase::Up;
            w.tmux_socket = socket.map(Path::to_path_buf);
            w.members = records.iter().map(|(k, r)| (k.to_string(), r.clone())).collect();
            Ok(())
        })
        .unwrap();
    store
}

/// The record `key` as `store` reads it now.
pub fn record(store: &Store, key: &str) -> MemberRecord {
    store.read().unwrap().unwrap().members[key].clone()
}

/// Opens `path` (read, write, create, no truncate) and takes its fs4 lock until the file is dropped.
///
/// The lock can be busy for a short time although no test holds it: `Store::update` (in `state`) opens the same
/// `lock` file with O_CLOEXEC and takes a flock, and a child that another test spawns on a parallel thread while that
/// fd is open shares the open file description, so the child keeps the flock until its exec. Thus a `WouldBlock`
/// is tried again every 1 ms for at most 2 s; after 2 s, or on any other error, the call panics.
pub fn hold(path: PathBuf) -> std::fs::File {
    const LIMIT: Duration = Duration::from_secs(2);
    let f = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&path).unwrap();
    let t0 = Instant::now();
    loop {
        match fs4::FileExt::try_lock(&f) {
            Ok(()) => return f,
            Err(fs4::TryLockError::WouldBlock) => {
                let waited = t0.elapsed();
                assert!(waited < LIMIT, "lock {} still busy after {waited:?}", path.display());
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(fs4::TryLockError::Error(e)) => panic!("lock {}: {e}", path.display()),
        }
    }
}

/// Drops `held` when `d` has passed since `t0`: held for the whole verb run and for `d` at least.
pub fn release_after(held: std::fs::File, t0: Instant, d: Duration) {
    std::thread::sleep(d.saturating_sub(t0.elapsed()));
    drop(held);
}

/// `overrides.json` of `dir` as JSON.
pub fn ovr_json(dir: &Path) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(dir.join("overrides.json")).unwrap()).unwrap()
}

/// A session folder `sessions/<id>` with token `t` and the proxy url `url`.
fn session_at(sessions: &Path, id: &str, url: &str) -> PathBuf {
    let dir = sessions.join(id);
    let sd = SessionDir::new(dir.clone());
    sd.create().unwrap();
    sd.write_token("t").unwrap();
    std::fs::write(sd.url(), url).unwrap();
    dir
}

/// A session folder whose url is a closed loopback port.
pub fn dead_session_at(sessions: &Path, id: &str) -> PathBuf {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    drop(l);
    session_at(sessions, id, &format!("http://127.0.0.1:{port}/"))
}

/// A session folder whose proxy reads each request (head and `content-length` body); then, with `rewrite`, it
/// writes that text to the folder's `overrides.json` and replies `200 {"redaction_content":false}`; without, it
/// replies `409 {"error":{"type":"zygo_refused","message":"no"}}`.
pub fn scripted_session_at(sessions: &Path, id: &str, rewrite: Option<&'static str>) -> PathBuf {
    serving_session_at(sessions, id, move |file| match rewrite {
        Some(text) => {
            std::fs::write(file, text).unwrap();
            ("200 OK", r#"{"redaction_content":false}"#)
        }
        None => ("409 Conflict", r#"{"error":{"type":"zygo_refused","message":"no"}}"#),
    })
}

/// A session folder whose proxy reads each request, runs `on_patch` with the folder's `overrides.json` path (after
/// the verb saved it, before the mirror reads it) and replies `200 {"redaction_content":false}`.
pub fn patching_session_at(sessions: &Path, id: &str, on_patch: fn(&Path)) -> PathBuf {
    serving_session_at(sessions, id, move |file| {
        on_patch(file);
        ("200 OK", r#"{"redaction_content":false}"#)
    })
}

/// A session folder whose proxy reads each request (head and `content-length` body), then replies with the status
/// and body that `respond` gives for the folder's `overrides.json` path.
fn serving_session_at(
    sessions: &Path, id: &str, respond: impl Fn(&Path) -> (&'static str, &'static str) + Send + 'static,
) -> PathBuf {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let dir = session_at(sessions, id, &format!("http://{}/", l.local_addr().unwrap()));
    let file = dir.join("overrides.json");
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(s) = s else { return };
            let mut r = std::io::BufReader::new(s);
            let mut len = 0usize;
            loop {
                let mut line = String::new();
                if r.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
                if let Some((k, v)) = line.split_once(':') && k.eq_ignore_ascii_case("content-length") {
                    len = v.trim().parse().unwrap_or(0);
                }
            }
            let _ = r.read_exact(&mut vec![0; len]);
            let (status, body) = respond(&file);
            let reply = format!(
                "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = r.get_mut().write_all(reply.as_bytes());
        }
    });
    dir
}

/// One call of the stand-in `tmux`.
#[derive(Debug)]
pub struct TmuxCall {
    /// `after` when `workspace.json` held an `"overrides"` key when tmux ran, else `before`.
    pub when: String,
    /// The pid and the process group of tmux's parent (the process that ran the snapshot).
    pub parent: i32,
    pub parent_pgid: i32,
    /// The command line of that parent (`ps -ww -o args=`).
    pub parent_args: String,
}

/// A stand-in `tmux` in `home/bin`. Each call appends one line `<when> <ppid> <pgid> <parent args>` to
/// `home/tmux-calls.log`, waits until `home/tmux-go` exists or 1.5 s pass (below the 2 s bound of a tmux call), adds
/// `end <ppid>`, and exits 1. Returns the variables for the child: `PATH` with `home/bin` first,
/// `ZYGO_TEST_TMUX_LOG`, `ZYGO_TEST_TMUX_GO`, `ZYGO_TEST_STATE_FILE` (= `state_file`).
pub fn fake_tmux(home: &Path, state_file: &Path) -> Vec<(&'static str, String)> {
    use std::os::unix::fs::PermissionsExt;
    let bin = home.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let tmux = bin.join("tmux");
    std::fs::write(
        &tmux,
        "#!/bin/sh\n\
         if grep -q '\"overrides\"' \"$ZYGO_TEST_STATE_FILE\"; then w=after; else w=before; fi\n\
         g=$(ps -ww -o pgid= -p \"$PPID\" | tr -d ' ')\n\
         echo \"$w $PPID $g $(ps -ww -o args= -p \"$PPID\")\" >> \"$ZYGO_TEST_TMUX_LOG\"\n\
         i=0; while [ ! -e \"$ZYGO_TEST_TMUX_GO\" ] && [ $i -lt 30 ]; do sleep 0.05; i=$((i+1)); done\n\
         echo \"end $PPID\" >> \"$ZYGO_TEST_TMUX_LOG\"\n\
         exit 1\n",
    )
    .unwrap();
    std::fs::set_permissions(&tmux, std::fs::Permissions::from_mode(0o755)).unwrap();
    vec![
        ("PATH", format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default())),
        ("ZYGO_TEST_TMUX_LOG", home.join("tmux-calls.log").display().to_string()),
        ("ZYGO_TEST_TMUX_GO", home.join("tmux-go").display().to_string()),
        ("ZYGO_TEST_STATE_FILE", state_file.display().to_string()),
    ]
}

/// The call lines of `home/tmux-calls.log` (not the `end` lines), as they are now.
pub fn tmux_calls(home: &Path) -> Vec<TmuxCall> {
    std::fs::read_to_string(home.join("tmux-calls.log"))
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.starts_with("end "))
        .map(|l| {
            let mut p = l.splitn(4, ' ');
            TmuxCall {
                when: p.next().unwrap().to_string(),
                parent: p.next().unwrap().parse().unwrap(),
                parent_pgid: p.next().unwrap().parse().unwrap(),
                parent_args: p.next().unwrap_or("").to_string(),
            }
        })
        .collect()
}

/// The calls once at least one is logged; panics when `d` passes first.
pub fn wait_for_calls(home: &Path, d: Duration) -> Vec<TmuxCall> {
    let t0 = Instant::now();
    loop {
        let calls = tmux_calls(home);
        if !calls.is_empty() {
            return calls;
        }
        assert!(t0.elapsed() < d, "no tmux call within {d:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// `true` while process `pid` exists (`kill(pid, 0)`).
pub fn alive(pid: i32) -> bool {
    // SAFETY: signal 0 checks the process and sends nothing.
    unsafe { libc::kill(pid, 0) == 0 }
}

/// Waits until process `pid` is gone; panics when `d` passes first.
pub fn wait_gone(pid: i32, d: Duration) {
    let t0 = Instant::now();
    while alive(pid) {
        assert!(t0.elapsed() < d, "process {pid} still runs after {d:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
}
