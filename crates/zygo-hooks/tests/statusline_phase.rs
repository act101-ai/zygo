//! H2: the workspace phase of the status line HUD (H§7.6, AU25; H§14 #36). Each test passes its environment as a
//! closure and its own `Paths` under a temp directory: no test changes the process environment.

use std::ffi::OsString;
use std::io::Cursor;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use tempfile::TempDir;
use zygo_core::paths::Paths;
use zygo_core::session_dir::SessionDir;
use zygo_core::session_view::{Member, SessionView, ViewWorkspace};
use zygo_hooks::statusline::data::collect;
use zygo_hooks::statusline::phase::{read, valid_ws_id};
use zygo_hooks::statusline::render::{render, ColorMode, HudData, IconSet, RenderEnv, Style};

const ID: &str = "0123456789abcdef";
const DRAINING: &[u8] = br#"{"version":1,"phase":"draining"}"#;

/// A session directory `<tmp>/sessions/S1` with a token and no `view.json`.
struct Fx {
    t: TempDir,
    dir: PathBuf,
}

impl Fx {
    fn new() -> Fx {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path().join("sessions").join("S1");
        let sd = SessionDir::new(dir.clone());
        sd.create().unwrap();
        sd.write_token("tok").unwrap();
        Fx { t, dir }
    }

    /// `workspaces_dir()` is `<tmp>/data/state/workspaces`.
    fn paths(&self) -> Paths {
        let (cache_dir, data_dir) = (self.t.path().join("cache"), self.t.path().join("data"));
        Paths {
            config_file: self.t.path().join("config.toml"),
            sessions_dir: self.t.path().join("sessions"),
            marketplaces_dir: cache_dir.join("marketplaces"),
            plugin_store: data_dir.join("plugins"),
            approvals_file: data_dir.join("approvals.toml"),
            usage_db: data_dir.join("usage.db"),
            cache_dir,
            data_dir,
        }
    }

    /// Writes `view.json`: a member of `ws_id`, or no membership marker.
    fn view(&self, ws_id: Option<&str>) {
        let workspace = ws_id
            .map(|id| ViewWorkspace { member: Member { ws_id: id.into(), pane: "kabcd2345".into() }, resume_note: None });
        SessionView { session: "S1".into(), workspace, ..Default::default() }
            .save(&SessionDir::new(self.dir.clone()))
            .unwrap();
    }

    /// Writes `<workspaces_dir>/<id>/workspace.json` with no check of `id`; gives the file path.
    fn state(&self, id: &str, body: &[u8]) -> PathBuf {
        let dir = self.paths().workspaces_dir().join(id);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("workspace.json");
        std::fs::write(&path, body).unwrap();
        path
    }

    fn env(&self) -> impl Fn(&str) -> Option<OsString> + use<> {
        let dir = self.dir.clone();
        move |key: &str| (key == "ZYGO_SESSION_DIR").then(|| dir.clone().into_os_string())
    }

    fn read(&self) -> Option<String> {
        read(&self.paths(), &self.env())
    }
}

/// A valid state file of exactly `len` bytes.
fn padded(len: usize) -> Vec<u8> {
    let (head, tail) = (r#"{"version":1,"phase":"draining","pad":""#, r#""}"#);
    format!("{head}{}{tail}", "x".repeat(len - head.len() - tail.len())).into_bytes()
}

#[test] // WS§5, WS AU84: the cases of `validators_follow_au84` in `crates/zygo-workspace/tests/identity.rs`
fn ws_id_rule_equals_the_w1_rule() {
    assert!(valid_ws_id("0123456789abcdef"));
    for bad in ["", "abc", "0123456789abcde", "0123456789abcdef0", "0123456789ABCDEF", "0123456789abcdeg", "0123456789abcdef\n"] {
        assert!(!valid_ws_id(bad), "{bad:?}");
    }
    // The values of H§14 #36, path parts, and 16 bytes that are not 16 characters.
    for bad in ["../x", "ABC", "..", "/", "0123456789abcde/", "0123456789abcd\u{e9}"] {
        assert!(!valid_ws_id(bad), "{bad:?}");
    }
}

#[test] // H§7.6; AU25: read fresh on each call
fn member_session_gives_the_phase_of_the_file() {
    let fx = Fx::new();
    fx.view(Some(ID));
    for phase in ["building", "up", "draining", "stopping", "down", "other"] {
        fx.state(ID, format!(r#"{{"version":1,"phase":"{phase}"}}"#).as_bytes());
        assert_eq!(fx.read().as_deref(), Some(phase));
    }
}

#[test] // WS AU78: a field that the reader does not know does not stop the read
fn extra_fields_are_ignored() {
    let fx = Fx::new();
    fx.view(Some(ID));
    let full = serde_json::json!({
        "version": 1, "id": ID, "name": "my-repo", "common_dir": "/r/.git", "main_root": "/r",
        "tmux_session": "zygo-my-repo", "loadout": "default", "phase": "stopping", "phase_since": 1_800_000_000u64,
        "down_all": false, "windows": [], "panes": [], "members": {}, "future": { "a": [1, 2] }
    });
    fx.state(ID, full.to_string().as_bytes());
    assert_eq!(fx.read().as_deref(), Some("stopping"));
}

#[test]
fn non_member_session_gives_none() {
    let fx = Fx::new();
    fx.state(ID, DRAINING);
    // No `view.json`.
    assert_eq!(fx.read(), None);
    // A `view.json` with no membership marker.
    fx.view(None);
    assert_eq!(fx.read(), None);
    // A member, but `ZYGO_SESSION_DIR` is not set or is empty.
    fx.view(Some(ID));
    assert_eq!(read(&fx.paths(), &|_: &str| None), None);
    assert_eq!(read(&fx.paths(), &|key: &str| (key == "ZYGO_SESSION_DIR").then(OsString::new)), None);
    assert_eq!(fx.read().as_deref(), Some("draining"));
}

#[test] // AU25, WS AU84: the id check comes before the path
fn hostile_ws_id_gives_none_and_reads_no_file() {
    let fx = Fx::new();
    fx.state(ID, DRAINING);
    fx.view(Some(ID));
    assert_eq!(fx.read().as_deref(), Some("draining"));
    let abs = fx.t.path().join("abs");
    let hostile = [
        "..",
        "../x",
        "",
        "ABC",
        "0123456789abcdef0",
        "0123456789ABCDEF",
        "0123456789abcdef/../0123456789abcdef",
        abs.to_str().unwrap(),
    ];
    for id in hostile {
        // The file that a join of `id` with no check reads.
        fx.state(id, DRAINING);
        fx.view(Some(id));
        assert_eq!(fx.read(), None, "{id:?}");
    }
}

#[test]
fn missing_file_gives_none() {
    let fx = Fx::new();
    fx.view(Some(ID));
    assert_eq!(fx.read(), None);
    assert!(!fx.paths().workspaces_dir().exists());
}

#[test]
fn bad_content_gives_none() {
    let fx = Fx::new();
    fx.view(Some(ID));
    for body in [
        "",
        "not json",
        "[]",
        "\"x\"",
        r#"{"version":1}"#,
        r#"{"phase":"draining"}"#,
        r#"{"version":1,"phase":7}"#,
        r#"{"version":1,"phase":"draining""#,
    ] {
        fx.state(ID, body.as_bytes());
        assert_eq!(fx.read(), None, "{body:?}");
    }
    // `workspace.json` is a directory.
    let path = fx.state(ID, DRAINING);
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert_eq!(fx.read(), None);
}

#[cfg(unix)]
#[test] // C6: a `workspace.json` that is a FIFO with no writer is not opened, so the reader does not wait
fn fifo_gives_none_and_does_not_wait() {
    let fx = Fx::new();
    fx.view(Some(ID));
    let path = fx.state(ID, DRAINING);
    std::fs::remove_file(&path).unwrap();
    let c_path = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
    // SAFETY: `c_path` is a valid NUL-terminated string that lives for the whole call.
    assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) }, 0);
    let (paths, env) = (fx.paths(), fx.env());
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || tx.send(read(&paths, &env)));
    // An open of the FIFO waits for a writer, and this test has none: a reader that opens it gives no answer.
    assert_eq!(rx.recv_timeout(std::time::Duration::from_secs(5)), Ok(None));
}

#[test] // AU25: a `version` other than 1 is not read (WS AU29)
fn other_version_gives_none() {
    let fx = Fx::new();
    fx.view(Some(ID));
    for version in ["2", "0", "\"1\"", "1.5", "-1", "4294967297"] {
        fx.state(ID, format!(r#"{{"version":{version},"phase":"draining"}}"#).as_bytes());
        assert_eq!(fx.read(), None, "{version}");
    }
}

#[test] // The bound is 64 KiB, the value of `STDIN_CAP` (AU24)
fn file_over_64_kib_gives_none() {
    let fx = Fx::new();
    fx.view(Some(ID));
    fx.state(ID, &padded(64 * 1024));
    assert_eq!(fx.read().as_deref(), Some("draining"));
    fx.state(ID, &padded(64 * 1024 + 1));
    assert_eq!(fx.read(), None);
    fx.state(ID, &padded(10 * 1024 * 1024));
    assert_eq!(fx.read(), None);
}

// ---- H§14 #36: `collect`, then `render` ----

const STRIPE: &str = "\x1b[31m▌\x1b[33m▌\x1b[36m▌\x1b[0m ";
const LOGO: &str = "\u{1d607}\u{1d606}\u{1d5f4}\u{1d5fc}";
const BRANCH: &str = "\u{e0a0}";

/// One `collect`: stdin `cwd` = `<tmp>/plain` (outside every repository), `ZYGO_WORK=branch:main`, no proxy.
fn hud(fx: &Fx) -> HudData {
    let plain = fx.t.path().join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    let stdin = serde_json::json!({ "cwd": plain }).to_string().into_bytes();
    let dir = fx.dir.clone();
    let env = move |key: &str| match key {
        "ZYGO_SESSION_DIR" => Some(dir.clone().into_os_string()),
        "ZYGO_WORK" => Some(OsString::from("branch:main")),
        _ => None,
    };
    collect(Box::new(Cursor::new(stdin)), &fx.dir, &fx.paths(), &env, Instant::now() + Duration::from_secs(10))
}

/// Nerd icons, with color.
fn ansi(d: &HudData, style: Style, width: u16) -> String {
    render(d, &RenderEnv { style, icons: IconSet::Nerd, color: ColorMode::Ansi256, width, force_panic: false })
}

/// `s` without its SGR sequences.
fn strip(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('\x1b') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        rest = &rest[rest.find('m').map_or(rest.len(), |m| m + 1)..];
    }
    out + rest
}

#[test] // H§14 #36, first sentence
fn h14_36_member_session_shows_the_phase_after_the_logo() {
    let fx = Fx::new();
    fx.view(Some(ID));
    fx.state(ID, DRAINING);
    let d = hud(&fx);
    assert_eq!(d.phase.as_deref(), Some("draining"));

    let compact = ansi(&d, Style::Compact, 200);
    assert_eq!(strip(&compact).find(&format!("▌▌▌ {LOGO} ws draining {BRANCH} main")), Some(0), "{compact:?}");
    assert!(
        compact.starts_with(&format!("{STRIPE}\x1b[1;97m{LOGO}\x1b[0m \x1b[31mws draining\x1b[0m \x1b[1;34m{BRANCH} main")),
        "{compact:?}"
    );
    let verbose = ansi(&d, Style::Verbose, 200);
    assert_eq!(strip(&verbose).find(&format!("▌▌▌ {LOGO}  ws draining   {BRANCH} main")), Some(0), "{verbose:?}");
    assert!(
        verbose.starts_with(&format!("{STRIPE}\x1b[1;97m{LOGO}\x1b[0m  \x1b[31mws draining\x1b[0m   \x1b[1;34m{BRANCH} main")),
        "{verbose:?}"
    );

    // In both styles, at every `COLUMNS`.
    for width in [200, 115, 100, 80, 60, 40, 30, 20] {
        let compact = ansi(&d, Style::Compact, width);
        assert!(strip(&compact).starts_with(&format!("▌▌▌ {LOGO} ws draining")), "{width}: {compact:?}");
        assert!(compact.contains(&format!("{LOGO}\x1b[0m \x1b[31mws draining")), "{width}: {compact:?}");
        let verbose = ansi(&d, Style::Verbose, width);
        assert!(strip(&verbose).starts_with(&format!("▌▌▌ {LOGO}  ws draining")), "{width}: {verbose:?}");
        assert!(verbose.contains(&format!("{LOGO}\x1b[0m  \x1b[31mws draining")), "{width}: {verbose:?}");
    }
}

#[test] // H§14 #36, from the second sentence
fn h14_36_phase_colors_and_hidden_cases() {
    let fx = Fx::new();
    fx.view(Some(ID));
    let state = |phase: &str| fx.state(ID, format!(r#"{{"version":1,"phase":"{phase}"}}"#).as_bytes());

    // `building`, `stopping`, `down` → SGR `33`, `1;31`, `2`.
    for (phase, sgr) in [("building", "33"), ("stopping", "1;31"), ("down", "2")] {
        state(phase);
        let d = hud(&fx);
        assert_eq!(d.phase.as_deref(), Some(phase));
        let compact = ansi(&d, Style::Compact, 200);
        assert!(compact.contains(&format!("{LOGO}\x1b[0m \x1b[{sgr}mws {phase}")), "{compact:?}");
        let verbose = ansi(&d, Style::Verbose, 200);
        assert!(verbose.contains(&format!("{LOGO}\x1b[0m  \x1b[{sgr}mws {phase}")), "{verbose:?}");
    }

    // The render of a non-member session: the reference for each hidden case.
    fx.view(None);
    state("draining");
    let none = hud(&fx);
    assert_eq!(none.phase, None);
    let reference = (ansi(&none, Style::Compact, 200), ansi(&none, Style::Verbose, 200));
    assert!(strip(&reference.0).starts_with(&format!("▌▌▌ {LOGO} {BRANCH} main")), "{:?}", reference.0);
    assert!(!reference.0.contains("ws ") && !reference.1.contains("ws "));
    let hidden = |label: &str| {
        let d = hud(&fx);
        assert_eq!((ansi(&d, Style::Compact, 200), ansi(&d, Style::Verbose, 200)), reference, "{label}");
        d.phase
    };

    // `up` and `other`: the value is read, and no segment shows.
    fx.view(Some(ID));
    for phase in ["up", "other"] {
        state(phase);
        assert_eq!(hidden(phase).as_deref(), Some(phase));
    }
    // `version = 2`.
    fx.state(ID, br#"{"version":2,"phase":"draining"}"#);
    assert_eq!(hidden("version 2"), None);
    // Bad JSON.
    fx.state(ID, b"not json");
    assert_eq!(hidden("bad JSON"), None);
    // A missing file.
    std::fs::remove_file(fx.paths().workspaces_dir().join(ID).join("workspace.json")).unwrap();
    assert_eq!(hidden("missing file"), None);
    // A marker `ws_id` of `../x` or `ABC`: the file that a join with no check reads is there, and is not read.
    for id in ["../x", "ABC"] {
        fx.state(id, DRAINING);
        fx.view(Some(id));
        assert_eq!(hidden(id), None);
    }
}
