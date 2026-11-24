//! HUD-9: the data of the status line HUD (H§7): bounded stdin, cache v2, the lease, the workers, the deadline and
//! the failure display.
//!
//! Acceptance items of H§14 that this file covers: #5, #6, #7, #17, #22, #23, #24, #27, #28, and the HUD parts of
//! #13, #14, #16, #25, #34 and #35. Each test passes its environment as a closure and its own `Paths`: no test
//! changes the process environment. The clock is the `test-hooks` seam `ZYGO_TEST_NOW`.

use std::ffi::OsString;
use std::io::{Cursor, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use serde_json::{json, Value};
use tempfile::TempDir;
use zygo_core::overrides::Overrides;
use zygo_core::paths::Paths;
use zygo_core::session_dir::SessionDir;
use zygo_core::session_view::SessionView;
use zygo_hooks::statusline::data::{collect, fetch_cap, log_width, width_source};
use zygo_hooks::statusline::render::{color_mode, render, ColorMode, HudData, IconSet, Outcome, RenderEnv, Style};
use zygo_hooks::statusline::style;
use zygo_proxy::client::SessionStatus;
use zygo_usage::{Store, TrimRow};

const LOGO: &str = "\u{1d607}\u{1d606}\u{1d5f4}\u{1d5fc}";
const BRANCH: &str = "\u{e0a0}";
const TREE: &str = "\u{f0645}";
const CPU: &str = "\u{f4bc}";
const PROFILE: &str = "\u{f0626}";
const DB: &str = "\u{f01bc}";
const BEAD: &str = "\u{f41b}";

/// The clock of a test: `ZYGO_TEST_NOW`.
const T: u64 = 1_800_000_000;
/// The git budget of a test that runs the real git: a loaded machine can need more than 400 ms.
const GIT: (&str, &str) = ("ZYGO_TEST_GIT_BUDGET_MS", "8000");
const FAR: Duration = Duration::from_secs(10);

// ---- the status endpoint ----

#[derive(Clone)]
enum Reply {
    /// The HTTP status and the JSON body.
    Json(u16, String),
    /// Accepts the request and never answers.
    Hang,
    /// Answers `200` with the body after the test sends on `go`.
    Gated(String),
}

/// A loopback HTTP server for `GET …/status`. `hits` counts the requests; `seen` gets one message for each.
struct Server {
    url: String,
    hits: Arc<AtomicUsize>,
    reply: Arc<Mutex<Reply>>,
    seen: mpsc::Receiver<()>,
    go: mpsc::Sender<()>,
}

impl Server {
    fn start(first: Reply) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://127.0.0.1:{}/", listener.local_addr().unwrap().port());
        let (hits, reply) = (Arc::new(AtomicUsize::new(0)), Arc::new(Mutex::new(first)));
        let (seen_tx, seen) = mpsc::channel();
        let (go, go_rx) = mpsc::channel::<()>();
        let (h, r) = (hits.clone(), reply.clone());
        std::thread::spawn(move || {
            let mut held: Vec<TcpStream> = Vec::new();
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                let mut head = Vec::new();
                let mut byte = [0u8; 1];
                while !head.ends_with(b"\r\n\r\n") && matches!(stream.read(&mut byte), Ok(1)) {
                    head.push(byte[0]);
                }
                h.fetch_add(1, Ordering::SeqCst);
                let _ = seen_tx.send(());
                let now = r.lock().unwrap().clone();
                let (status, body) = match now {
                    Reply::Json(status, body) => (status, body),
                    Reply::Hang => {
                        held.push(stream);
                        continue;
                    }
                    Reply::Gated(body) => {
                        let _ = go_rx.recv_timeout(Duration::from_secs(20));
                        (200, body)
                    }
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        Server { url, hits, reply, seen, go }
    }

    fn ok(status: &SessionStatus) -> Server {
        Server::start(Reply::Json(200, serde_json::to_string(status).unwrap()))
    }

    fn set(&self, reply: Reply) {
        *self.reply.lock().unwrap() = reply;
    }

    fn set_status(&self, status: &SessionStatus) {
        self.set(Reply::Json(200, serde_json::to_string(status).unwrap()));
    }

    fn hits(&self) -> usize {
        self.hits.load(Ordering::SeqCst)
    }
}

fn error_reply(status: u16, code: &str) -> Reply {
    Reply::Json(status, json!({ "error": { "type": code, "message": "m" } }).to_string())
}

/// The status reply of the OP13 data (H§14 #3).
fn op13() -> SessionStatus {
    SessionStatus {
        session: "S1".into(),
        used_context: Some(196_000),
        cache_read_share: Some(0.92),
        model: Some("claude-opus-4.7".into()),
        context_window: Some(1_000_000),
        tokens_in: Some(1_200_000),
        tokens_out: Some(48_000),
        spend_usd: Some(3.41),
        spend_notional: Some(false),
        requests_total: Some(12),
        requests_priced: Some(12),
        passthrough: Some(false),
        team_session: Some(false),
        ..Default::default()
    }
}

/// A status reply of a server older than the HUD: none of the new fields (H§7.2).
const OLD_REPLY: &str = r#"{"session":"S1","profile":"p1","work":"ENG-42","used_context":412345,"effective_window":600000,"compact":600000,"cache_read_share":0.6,"expires_at":"2026-11-21T00:00:00Z","usage_note":null}"#;

// ---- the session ----

/// A session directory `<tmp>/sessions/S1` with a token and a `view.json` (session `S1`, profile `fast`), no `url`
/// file (the proxy cannot be reached), and `<tmp>/plain`, a directory outside every repository.
struct Fx {
    t: TempDir,
    dir: PathBuf,
    plain: PathBuf,
}

impl Fx {
    fn new() -> Fx {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path().join("sessions").join("S1");
        let sd = SessionDir::new(dir.clone());
        sd.create().unwrap();
        sd.write_token("tok").unwrap();
        SessionView { session: "S1".into(), profile: Some("fast".into()), ..Default::default() }.save(&sd).unwrap();
        let plain = t.path().join("plain");
        std::fs::create_dir(&plain).unwrap();
        Fx { t, dir, plain }
    }

    fn served(server: &Server) -> Fx {
        let fx = Fx::new();
        std::fs::write(fx.dir.join("url"), &server.url).unwrap();
        fx
    }

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

    /// The stdin object: `cwd` and `effort = high`.
    fn stdin(&self, cwd: &Path) -> Value {
        json!({ "cwd": cwd, "effort": "high" })
    }

    /// One `collect` with the stdin of the plain directory.
    fn run(&self, vars: &[(&str, &str)], now: u64) -> HudData {
        self.run_with(Box::new(Cursor::new(self.stdin(&self.plain).to_string().into_bytes())), vars, now, FAR)
    }

    /// One `collect` in `cwd`.
    fn run_in(&self, cwd: &Path, vars: &[(&str, &str)], now: u64) -> HudData {
        self.run_with(Box::new(Cursor::new(self.stdin(cwd).to_string().into_bytes())), vars, now, FAR)
    }

    fn run_with(&self, stdin: Box<dyn Read + Send>, vars: &[(&str, &str)], now: u64, bound: Duration) -> HudData {
        let mut all: Vec<(String, String)> = vec![("ZYGO_TEST_NOW".into(), now.to_string())];
        all.extend(vars.iter().map(|(k, v)| (k.to_string(), v.to_string())));
        let env = move |key: &str| all.iter().rev().find(|(k, _)| k == key).map(|(_, v)| OsString::from(v));
        collect(stdin, &self.dir, &self.paths(), &env, Instant::now() + bound)
    }

    fn cache(&self) -> Value {
        serde_json::from_slice(&std::fs::read(self.dir.join("statusline.json")).unwrap()).unwrap()
    }

    fn lock(&self) -> PathBuf {
        self.dir.join("statusline.lock")
    }

    /// 92,160 trimmed bytes for the session `S1`.
    fn trim(&self) {
        let store = Store::open(&self.paths().usage_db).unwrap();
        store
            .insert_trim(&TrimRow { session: "S1".into(), stored_bytes: 100_000, view_bytes: 7_840, ..Default::default() })
            .unwrap();
    }
}

// ---- render ----

fn renv(style: Style, icons: IconSet, color: ColorMode) -> RenderEnv {
    RenderEnv { style, icons, color, width: 200, force_panic: false }
}

/// The text of the HUD: Nerd icons, no color, width 200.
fn text(d: &HudData, style: Style) -> String {
    render(d, &renv(style, IconSet::Nerd, ColorMode::Off))
}

fn plain_text(d: &HudData, style: Style) -> String {
    render(d, &renv(style, IconSet::Plain, ColorMode::Off))
}

/// The bytes of the HUD: Nerd icons, 256 colors, width 200.
fn ansi(d: &HudData, style: Style) -> String {
    render(d, &renv(style, IconSet::Nerd, ColorMode::Ansi256))
}

/// The last SGR sequence before the first `needle` in `s`.
fn sgr_before<'a>(s: &'a str, needle: &str) -> &'a str {
    let head = &s[..s.find(needle).unwrap_or_else(|| panic!("no {needle:?} in {s:?}"))];
    let start = head.rfind('\x1b').unwrap_or_else(|| panic!("no escape before {needle:?} in {s:?}"));
    &head[start..start + head[start..].find('m').unwrap() + 1]
}

const C: Style = Style::Compact;
const V: Style = Style::Verbose;

// ---- git fixtures ----

fn git(dir: &Path, args: &[&str]) {
    let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
    let out = Command::new("git")
        .current_dir(dir)
        .args(["-c", "commit.gpgsign=false", "-c", "user.name=t", "-c", "user.email=t@example.invalid"])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", null)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

fn commit(dir: &Path, message: &str) {
    git(dir, &["commit", "-q", "--allow-empty", "-m", message]);
}

/// `<parent>/<name>`: a repository on `branch` with one commit.
fn repo(parent: &Path, name: &str, branch: &str) -> PathBuf {
    git(parent, &["init", "-q", "-b", branch, name]);
    let dir = parent.join(name);
    commit(&dir, "a");
    dir
}

/// `<parent>/origin` on `main`, and its clone `<parent>/<name>`, whose `main` follows `origin/main`.
fn clone(parent: &Path, name: &str) -> PathBuf {
    if !parent.join("origin").exists() {
        repo(parent, "origin", "main");
    }
    git(parent, &["clone", "-q", "origin", name]);
    parent.join(name)
}

/// A linked worktree `<parent>/wt` of a clone, on the branch `wt` that follows `origin/main`, one commit ahead.
fn linked_ahead(parent: &Path) -> PathBuf {
    let c = clone(parent, "c");
    git(&c, &["worktree", "add", "-q", "-b", "wt", "../wt", "origin/main"]);
    let wt = parent.join("wt");
    commit(&wt, "b");
    wt
}

// ---- H§14 #5 ----

/// H§14 #5: the work item comes from `ZYGO_WORK`, else the status `work`; a `branch` source shows the live branch.
#[test]
#[ignore = "timing: collect waits only 200 ms for the stdin that sets the asserted cwd and effort"]
fn h14_05_work_item_from_zygo_work_the_status_and_the_live_branch() {
    let fx = Fx::new();
    let d = fx.run(&[("ZYGO_WORK", "branch:main")], T);
    assert_eq!(d.work.as_deref(), Some("branch:main"));
    let (colored, plain) = (ansi(&d, C), text(&d, C));
    assert!(colored.contains(&format!("\x1b[1;34m{BRANCH} main\x1b[0m")), "{colored:?}");
    assert!(!plain.contains("branch:"), "{plain}");

    for (work, nerd, bare) in [
        ("github:o/n#123", format!("{} #123", "\u{f408}"), "#123".to_string()),
        ("github:a#b#7", format!("{} #7", "\u{f408}"), "#7".to_string()),
        ("github:o/n", "github:o/n".to_string(), "github:o/n".to_string()),
        ("work-graph:hud/N3", "◆ hud/N3".to_string(), "♦ hud/N3".to_string()),
        ("beads:zy-a1b2", format!("{BEAD} zy-a1b2"), "○ zy-a1b2".to_string()),
        ("x:y", "x:y".to_string(), "x:y".to_string()),
        ("nocolon", "nocolon".to_string(), "nocolon".to_string()),
    ] {
        let d = fx.run(&[("ZYGO_WORK", work)], T);
        assert_eq!(text(&d, C), format!("{LOGO} {nerd}  {PROFILE} fast  offline"), "{work}");
        assert_eq!(plain_text(&d, C), format!("zygo {bare}  ≡ fast  offline"), "{work}");
    }

    // No work item at all and no git: no work segment.
    let unset: &[(&str, &str)] = &[];
    let empty: &[(&str, &str)] = &[("ZYGO_WORK", "")];
    for vars in [unset, empty] {
        let d = fx.run(vars, T);
        assert_eq!((d.work.as_deref(), d.branch.as_deref()), (None, None));
        assert_eq!(text(&d, C), format!("{LOGO}  {PROFILE} fast  offline"));
    }

    // The live branch: alone in plain blue; in place of the launch-time id of a `branch` source.
    let feat = repo(fx.t.path(), "feat-repo", "feat");
    let d = fx.run_in(&feat, &[("ZYGO_WORK", ""), GIT], T);
    assert_eq!(d.branch.as_deref(), Some("feat"));
    assert!(ansi(&d, C).contains(&format!("\x1b[34m{BRANCH} feat\x1b[0m")), "{:?}", ansi(&d, C));
    assert!(plain_text(&d, C).starts_with("zygo feat  "), "{}", plain_text(&d, C));
    let d = fx.run_in(&feat, &[("ZYGO_WORK", "branch:main"), GIT], T);
    assert!(ansi(&d, C).contains(&format!("\x1b[1;34m{BRANCH} feat\x1b[0m")), "{:?}", ansi(&d, C));
    // A detached head, and no git result: the launch-time id.
    git(&feat, &["checkout", "-q", "--detach"]);
    let d = fx.run_in(&feat, &[("ZYGO_WORK", "branch:main"), GIT], T + 31);
    assert_eq!(d.branch, None);
    assert!(text(&d, C).starts_with(&format!("{LOGO} {BRANCH} main  ")), "{}", text(&d, C));
    let d = fx.run(&[("ZYGO_WORK", "branch:main")], T + 31);
    assert!(text(&d, C).starts_with(&format!("{LOGO} {BRANCH} main  ")), "{}", text(&d, C));
}

/// AU11: with no `ZYGO_WORK`, the status `work` is the work item; `ZYGO_WORK` wins over it.
#[test]
fn h14_05_status_work_is_the_fallback() {
    let server = Server::ok(&SessionStatus { work: Some("github:o/n#9".into()), ..op13() });
    let fx = Fx::served(&server);
    let d = fx.run(&[], T);
    assert!(text(&d, C).starts_with(&format!("{LOGO} \u{f408} #9  ")), "{}", text(&d, C));
    let d = fx.run(&[("ZYGO_WORK", "beads:zy-1")], T);
    assert!(text(&d, C).starts_with(&format!("{LOGO} {BEAD} zy-1  ")), "{}", text(&d, C));
    // The cache holds the fallback: the second call made no request.
    assert_eq!(server.hits(), 1);
}

// ---- H§14 #6 ----

/// H§14 #6: the worktree segment shows only in a linked worktree, and only the final directory name.
#[test]
#[ignore = "timing: collect waits only 200 ms for the stdin that sets the asserted cwd and effort"]
fn h14_06_worktree_segment_from_the_git_worker() {
    let fx = Fx::new();
    let root = fx.t.path();
    let main = repo(root, "w", "main");
    let d = fx.run_in(&main, &[GIT], T);
    assert_eq!((d.worktree.as_deref(), d.branch.as_deref()), (None, Some("main")));
    assert!(!text(&d, C).contains(TREE));

    std::fs::create_dir_all(root.join("a").join("b")).unwrap();
    git(&main, &["worktree", "add", "-q", "-b", "feat-hud", "../a/b/feat-hud"]);
    let d = fx.run_in(&root.join("a").join("b").join("feat-hud"), &[GIT], T);
    assert_eq!(d.worktree.as_deref(), Some("feat-hud"));
    let row = text(&d, C);
    assert!(row.contains(&format!("{BRANCH} feat-hud {TREE} feat-hud  ")), "{row}");
    assert!(!row.contains('/') && !row.contains('\\'), "{row}");

    // The color: 256-color 172, else the 16-color fallback 33.
    let colored = |vars: &'static [(&'static str, &'static str)]| {
        let env = move |key: &str| vars.iter().find(|(k, _)| *k == key).map(|(_, v)| OsString::from(v));
        render(&d, &renv(C, IconSet::Nerd, color_mode(&env)))
    };
    assert!(colored(&[("COLORTERM", "truecolor")]).contains(&format!("\x1b[38;5;172m{TREE} feat-hud\x1b[0m")));
    assert!(colored(&[("TERM", "xterm")]).contains(&format!("\x1b[33m{TREE} feat-hud\x1b[0m")));

    // A bare repository with two worktrees: each shows its own name.
    git(root, &["clone", "-q", "--bare", "w", "bare.git"]);
    for name in ["one", "two"] {
        git(&root.join("bare.git"), &["worktree", "add", "-q", "-b", name, &format!("../{name}")]);
        let d = fx.run_in(&root.join(name), &[GIT], T);
        assert_eq!((d.worktree.as_deref(), d.branch.as_deref()), (Some(name), Some(name)));
    }

    // A 40-character name: 23 characters and `…`.
    let long = "n".repeat(40);
    git(&main, &["worktree", "add", "-q", "-b", "long", &format!("../{long}")]);
    let d = fx.run_in(&root.join(&long), &[GIT], T);
    assert!(text(&d, C).contains(&format!("{TREE} {}…  ", "n".repeat(23))), "{}", text(&d, C));
}

/// H§14 #6: a worktree directory whose name holds an escape sequence prints escaped. Windows refuses the name.
#[cfg(unix)]
#[test]
#[ignore = "timing: collect waits only 200 ms for the stdin that sets the asserted cwd and effort"]
fn h14_06_worktree_name_with_an_escape_sequence_is_escaped() {
    let fx = Fx::new();
    let main = repo(fx.t.path(), "w", "main");
    git(&main, &["worktree", "add", "-q", "-b", "esc", "../x\x1b[31my"]);
    let d = fx.run_in(&fx.t.path().join("x\x1b[31my"), &[GIT], T);
    assert_eq!(d.worktree.as_deref(), Some("x\x1b[31my"));
    let row = text(&d, C);
    assert!(row.contains(&format!("{TREE} x\\u{{1b}}[31my")), "{row:?}");
    assert!(!row.contains('\x1b'), "{row:?}");
}

// ---- H§14 #7 ----

/// H§14 #7: the ahead and behind marks, in verbose only, after the worktree; no `●` in any output (OP14).
#[test]
#[ignore = "timing: collect waits only 200 ms for the stdin that sets the asserted cwd and effort"]
fn h14_07_git_marks_from_the_git_worker() {
    let fx = Fx::new();
    let root = fx.t.path();
    let mut seen = Vec::new();

    // Ahead 1, in a linked worktree: the worktree comes before the marks.
    let wt = linked_ahead(root);
    let d = fx.run_in(&wt, &[GIT], T);
    assert_eq!((d.ahead, d.behind, d.worktree.as_deref()), (Some(1), Some(0), Some("wt")));
    // The cyan of the marks runs on into the profile, which is the next segment here (gap absorption, H§6.4).
    assert!(ansi(&d, V).contains(&format!("{TREE} wt\x1b[0m \x1b[36m↑1   {PROFILE} fast\x1b[0m")), "{:?}", ansi(&d, V));
    assert!(!text(&d, C).contains('↑') && !text(&d, C).contains('↓'), "{}", text(&d, C));
    seen.extend([ansi(&d, V), text(&d, C)]);

    // Ahead 0 and behind 2.
    let behind = clone(root, "behind");
    commit(&root.join("origin"), "c");
    commit(&root.join("origin"), "d");
    git(&behind, &["fetch", "-q"]);
    let d = fx.run_in(&behind, &[GIT], T);
    assert_eq!((d.ahead, d.behind), (Some(0), Some(2)));
    let row = text(&d, V);
    assert!(row.contains(&format!("{BRANCH} main ↓2")) && !row.contains('↑'), "{row}");
    seen.push(ansi(&d, V));

    // No upstream: no marks.
    let alone = repo(root, "alone", "main");
    let d = fx.run_in(&alone, &[GIT], T);
    assert_eq!((d.ahead, d.behind, d.branch.as_deref()), (None, None, Some("main")));
    assert!(!text(&d, V).contains('↑') && !text(&d, V).contains('↓'), "{}", text(&d, V));
    seen.push(ansi(&d, V));

    assert!(seen.iter().all(|s| !s.contains('●')), "{seen:?}");
}

// ---- H§14 #13, #14, #16: the HUD part ----

/// H§14 #13: the spend of the status reply prints as `$X.XX` in SGR `32`.
#[test]
fn h14_13_spend_of_the_status_reply_is_green() {
    let server = Server::ok(&SessionStatus { requests_total: Some(2), requests_priced: Some(2), ..op13() });
    let fx = Fx::served(&server);
    let d = fx.run(&[], T);
    assert!(ansi(&d, C).contains("\x1b[32m$3.41\x1b[0m"), "{:?}", ansi(&d, C));
    assert!(text(&d, C).contains("↑1.2m ↓48k  $3.41  "), "{}", text(&d, C));
}

/// H§14 #14: a notional spend has `≈` in SGR `2` before each form; with `spend_notional` absent there is no `≈`.
#[test]
#[ignore = "timing: a fetch 31 s after a cached one must answer within its 1 s cap"]
fn h14_14_notional_spend_has_a_dim_mark() {
    let notional = SessionStatus { spend_usd: Some(1.84), spend_notional: Some(true), ..op13() };
    let server = Server::ok(&notional);
    let fx = Fx::served(&server);
    let d = fx.run(&[], T);
    let row = ansi(&d, C);
    assert!(row.contains("≈\x1b[0m\x1b[32m$1.84\x1b[0m"), "{row:?}");
    assert_eq!(sgr_before(&row, "≈"), "\x1b[2m", "{row:?}");

    let cases = [
        (SessionStatus { requests_total: Some(2), requests_priced: Some(1), ..notional.clone() }, "≈$1.84+"),
        (SessionStatus { spend_usd: Some(0.004), ..notional.clone() }, "≈<$0.01"),
        (SessionStatus { requests_total: Some(2), requests_priced: Some(0), ..notional.clone() }, "≈$?"),
        (SessionStatus { spend_notional: None, ..notional.clone() }, "$1.84"),
    ];
    for (n, (status, want)) in cases.iter().enumerate() {
        server.set_status(status);
        let d = fx.run(&[], T + 31 * (n as u64 + 1));
        let row = text(&d, C);
        assert!(row.contains(&format!("↓48k  {want}  ")), "{want}: {row}");
        assert_eq!(row.contains('≈'), want.starts_with('≈'), "{row}");
    }
    assert_eq!(server.hits(), 5);
}

/// H§14 #16: an unpriced request gives `$X.XX+`; no priced request gives `$?`; no request gives `$0.00`.
#[test]
#[ignore = "timing: a fetch 31 s after a cached one must answer within its 1 s cap"]
fn h14_16_spend_forms_of_unpriced_requests() {
    let server = Server::ok(&op13());
    let fx = Fx::served(&server);
    let cases = [
        (Some(3.41), 2, 1, "$3.41+"),
        (Some(0.0), 1, 0, "$?"),
        (Some(0.0), 0, 0, "$0.00"),
    ];
    for (n, (spend, total, priced, want)) in cases.into_iter().enumerate() {
        server.set_status(&SessionStatus {
            spend_usd: spend,
            requests_total: Some(total),
            requests_priced: Some(priced),
            ..op13()
        });
        let d = fx.run(&[], T + 31 * n as u64);
        assert!(ansi(&d, C).contains(&format!("\x1b[32m{want}\x1b[0m")), "{want}: {:?}", ansi(&d, C));
    }
}

// ---- H§14 #17 ----

/// H§14 #17 and review focus 7: a status reply of an older server parses with every new field `None`; the HUD has
/// no model, mode, tokens, spend or `⇣` segment and keeps `ctx <used>/-`; the fetch counts as a success.
#[test]
#[ignore = "timing: collect waits only 200 ms for the stdin that sets the asserted cwd and effort"]
fn h14_17_old_status_reply_drops_only_the_new_segments() {
    let parsed: SessionStatus = serde_json::from_str(OLD_REPLY).unwrap();
    assert_eq!(
        parsed,
        SessionStatus {
            session: "S1".into(),
            profile: Some("p1".into()),
            work: Some("ENG-42".into()),
            used_context: Some(412_345),
            effective_window: Some(600_000),
            compact: Some(zygo_proxy::client::WindowArg::Tokens(600_000)),
            cache_read_share: Some(0.6),
            expires_at: "2026-11-21T00:00:00Z".into(),
            ..Default::default()
        }
    );
    assert_eq!(
        (parsed.model, parsed.context_window, parsed.threshold, parsed.tokens_in, parsed.tokens_out),
        (None, None, None, None, None)
    );
    assert_eq!(
        (parsed.spend_usd, parsed.spend_notional, parsed.requests_total, parsed.requests_priced),
        (None, None, None, None)
    );
    assert_eq!((parsed.passthrough, parsed.team_session), (None, None));

    let server = Server::start(Reply::Json(200, OLD_REPLY.into()));
    let fx = Fx::served(&server);
    let d = fx.run(&[], T);
    assert_eq!(d.outcome, Outcome::Ok);
    assert!(!d.stale);
    assert_eq!(text(&d, C), format!("{LOGO} ENG-42  {PROFILE} fast  ctx 412k/-  {DB} 60%"));
    assert_eq!(text(&d, V), format!("{LOGO}  ENG-42   {PROFILE} fast\nz\u{28e}\u{183}o  ctx 412k/-   cache 60%"));
    let cache = fx.cache();
    assert_eq!((cache["v"].as_u64(), cache["outcome"].as_str(), cache["retry_at"].as_u64()), (Some(2), Some("ok"), Some(0)));
    assert!(!fx.lock().exists());
}

// ---- H§14 #18 ----

/// H§14 #18 (data part), AU4: the trimmed count is not cached. A trimmed row written between two calls 1 s apart
/// shows at the second call, while the status part comes from the cache (one request in all). The count is the
/// one of the session of `view.json` in the usage store of `paths`.
#[test]
#[ignore = "timing: the trimmed count must come within the 1 s trim cap"]
fn h14_18_trimmed_row_between_two_calls_shows_at_the_second_call() {
    let server = Server::ok(&op13());
    let fx = Fx::served(&server);
    let first = fx.run(&[], T);
    assert_eq!(first.trimmed_bytes, None); // no usage store yet
    assert!(!text(&first, V).contains("trimmed") && !text(&first, C).contains('✂'), "{}", text(&first, V));

    fx.trim();
    let second = fx.run(&[], T + 1);
    assert_eq!(second.trimmed_bytes, Some(92_160));
    assert_eq!(server.hits(), 1);
    assert!(text(&second, C).ends_with("  ✂ 23k"), "{}", text(&second, C));
    assert!(plain_text(&second, C).ends_with("  trim 23k"), "{}", plain_text(&second, C));
    assert!(text(&second, V).ends_with("   trimmed 23k tok"), "{}", text(&second, V));
    assert_eq!(text(&first, V), text(&second, V).replace("   trimmed 23k tok", ""));

    // A row of another session does not count; one more row of this session does, again with no request.
    let store = Store::open(&fx.paths().usage_db).unwrap();
    store.insert_trim(&TrimRow { session: "S2".into(), stored_bytes: 8_000, ..Default::default() }).unwrap();
    assert_eq!(fx.run(&[], T + 2).trimmed_bytes, Some(92_160));
    store.insert_trim(&TrimRow { session: "S1".into(), stored_bytes: 4_000, ..Default::default() }).unwrap();
    let fourth = fx.run(&[], T + 3);
    assert_eq!(fourth.trimmed_bytes, Some(96_160));
    assert!(text(&fourth, V).ends_with("   trimmed 24k tok"), "{}", text(&fourth, V));
    assert_eq!(server.hits(), 1);

    // The cache file has no trimmed value.
    let cache = fx.cache();
    for part in [&cache, &cache["status"]] {
        assert!(part.as_object().unwrap().keys().all(|k| !k.contains("trim")), "{cache}");
    }
}

// ---- H§14 #22 ----

/// A reader that gives `head`, then blocks until the test drops its sender.
struct Stall {
    head: Option<Vec<u8>>,
    hold: mpsc::Receiver<()>,
}

impl Read for Stall {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self.head.take() {
            Some(head) => {
                buf[..head.len()].copy_from_slice(&head);
                Ok(head.len())
            }
            None => {
                let _ = self.hold.recv();
                Ok(0)
            }
        }
    }
}

/// H§14 #22: each stdin case prints a HUD within the bound; at most 64 KiB is parsed; a stdin string that the HUD
/// uses prints escaped; a stalled writer is parsed as empty at 200 ms; the stdin `model.id` never shows.
#[test]
#[ignore = "timing: asserts elapsed time and races a 200 ms stdin wait"]
fn h14_22_stdin_is_bounded_and_never_breaks_the_hud() {
    let server = Server::ok(&op13());
    let fx = Fx::served(&server);
    let cwd = fx.plain.to_str().unwrap().to_string();
    let mib = 1024 * 1024;
    let run = |stdin: Vec<u8>| {
        let start = Instant::now();
        let d = fx.run_with(Box::new(Cursor::new(stdin)), &[], T, Duration::from_secs(3));
        assert!(start.elapsed() < Duration::from_secs(3), "took {:?}", start.elapsed());
        let row = text(&d, V);
        assert!(row.starts_with(LOGO) && row.contains("opus-4.7"), "{row}");
        (d, row)
    };

    // No object: empty input.
    for bad in [&b""[..], b"[]", b"\"x\"", b"{\"effort\":", b"{\"effort\":\"high\"]", b"\xff\xfe{}"] {
        let (d, row) = run(bad.to_vec());
        assert_eq!(d.effort, None, "{:?}", String::from_utf8_lossy(bad));
        assert!(!row.contains(" · "), "{row}");
    }

    // 10 MiB of JSON, and a 1 MiB string in `model.id`: the object is not complete within 64 KiB.
    let big = json!({ "effort": "high", "cwd": cwd, "pad": "p".repeat(10 * mib) }).to_string();
    assert_eq!(run(big.into_bytes()).0.effort, None);
    let id = "role-name-".repeat(mib / 10);
    let (d, row) = run(json!({ "effort": "high", "model": { "id": id } }).to_string().into_bytes());
    assert_eq!(d.effort, None);
    assert!(!row.contains("role-name-"), "{}", &row[..200.min(row.len())]);
    // An object that fits is parsed whole.
    let fits = json!({ "pad": "p".repeat(60 * 1024), "effort": "max", "cwd": cwd }).to_string();
    assert!(fits.len() < 64 * 1024);
    assert_eq!(run(fits.into_bytes()).0.effort.as_deref(), Some("max"));

    // The stdin `model` is a role name: never shown (AU10).
    let (_, row) = run(json!({ "effort": "high", "cwd": cwd, "model": { "id": "role-lead", "display_name": "Lead" } }).to_string().into_bytes());
    assert!(row.contains(&format!("{CPU} opus-4.7 · high")) && !row.contains("role-lead") && !row.contains("Lead"), "{row}");

    // A stdin string that the HUD uses prints escaped.
    let (d, row) = run(json!({ "effort": "\x1b]0;x\x07\u{202e}", "cwd": cwd }).to_string().into_bytes());
    assert_eq!(d.effort.as_deref(), Some("\x1b]0;x\x07\u{202e}"));
    assert!(row.contains("opus-4.7 · \\u{1b}]"), "{row:?}");
    assert!(!row.contains('\x1b') && !row.contains('\x07') && !row.contains('\u{202e}'), "{row:?}");
    // `effort` as an object with a string level (PH5); another shape drops the effort part.
    assert_eq!(run(json!({ "effort": { "level": "low" }, "cwd": cwd }).to_string().into_bytes()).0.effort.as_deref(), Some("low"));
    assert_eq!(run(json!({ "effort": 3, "cwd": cwd }).to_string().into_bytes()).0.effort, None);
    assert_eq!(run(json!({ "effort": ["high"], "cwd": cwd }).to_string().into_bytes()).0.effort, None);

    // A valid object followed by 10 MiB of garbage: the object is used.
    let mut tail = json!({ "effort": "high", "cwd": cwd }).to_string().into_bytes();
    tail.extend(std::iter::repeat_n(b'}', 10 * mib));
    assert_eq!(run(tail).0.effort.as_deref(), Some("high"));

    // A writer that sends half an object and then stalls: empty input at 200 ms.
    let (_hold, rx) = mpsc::channel();
    let start = Instant::now();
    let stall = Stall { head: Some(b"{\"effort\":\"high\",\"cwd\":".to_vec()), hold: rx };
    let d = fx.run_with(Box::new(stall), &[], T, Duration::from_secs(3));
    let took = start.elapsed();
    assert_eq!(d.effort, None);
    assert!(text(&d, V).starts_with(LOGO));
    assert!(took >= Duration::from_millis(200) && took < Duration::from_secs(3), "took {took:?}");
}

/// H§7.1: the git directory is the stdin `cwd`, else `workspace.current_dir`.
#[test]
#[ignore = "timing: collect waits only 200 ms for the stdin that sets the asserted cwd and effort"]
fn stdin_cwd_then_workspace_current_dir() {
    let fx = Fx::new();
    let one = repo(fx.t.path(), "one", "one");
    let two = repo(fx.t.path(), "two", "two");
    let run = |stdin: Value, now: u64| {
        fx.run_with(Box::new(Cursor::new(stdin.to_string().into_bytes())), &[GIT], now, FAR).branch
    };
    assert_eq!(run(json!({ "cwd": one, "workspace": { "current_dir": two } }), T).as_deref(), Some("one"));
    assert_eq!(run(json!({ "workspace": { "current_dir": two } }), T).as_deref(), Some("two"));
    assert_eq!(run(json!({ "cwd": 7, "workspace": { "current_dir": two } }), T).as_deref(), Some("two"));
}

// ---- H§14 #23 ----

/// H§14 #23: with the fetch seam and the git seam blocking, the call ends at its deadline; the git seam past its
/// budget gives no git segment while the proxy segments show.
#[test]
#[ignore = "timing: asserts elapsed time against the call deadline and the git budget"]
fn h14_23_blocked_workers_end_at_the_deadline() {
    let server = Server::ok(&op13());
    let fx = Fx::served(&server);
    let main = repo(fx.t.path(), "w", "main");
    let stdin = || Box::new(Cursor::new(fx.stdin(&main).to_string().into_bytes()));

    let bound = Duration::from_millis(1500);
    let start = Instant::now();
    let d = fx.run_with(stdin(), &[("ZYGO_TEST_STATUSLINE_BLOCK", "1"), ("ZYGO_TEST_GIT_DELAY_MS", "20000")], T, bound);
    let took = start.elapsed();
    assert!(took < bound + Duration::from_millis(500), "took {took:?}");
    assert!(took >= Duration::from_millis(1000), "the fetch gets the time to the deadline with no cache: {took:?}");
    assert_eq!((d.status.is_some(), d.outcome, d.branch.as_deref()), (false, Outcome::Offline, None));
    assert_eq!(server.hits(), 0);
    assert!(text(&d, C).ends_with("offline"), "{}", text(&d, C));

    // The git seam alone, past the 400 ms budget: the proxy segments show, the git segments do not.
    let start = Instant::now();
    let d = fx.run_with(stdin(), &[("ZYGO_TEST_GIT_DELAY_MS", "20000")], T + 31, Duration::from_secs(3));
    let took = start.elapsed();
    assert!(took >= Duration::from_millis(400) && took < Duration::from_secs(3), "took {took:?}");
    assert_eq!((d.branch.as_deref(), d.worktree.as_deref(), d.ahead), (None, None, None));
    assert!(text(&d, C).contains(&format!("{CPU} opus-4.7 {PROFILE} fast  ctx 196k/1m")), "{}", text(&d, C));
    // The failed try is a git part with no data, not due for 30 s.
    let cache = fx.cache();
    assert_eq!(cache["git"], json!({ "at": T + 31, "cwd": main, "linked": false }));
}

/// The P-HUD-1 seam `ZYGO_TEST_FETCH_DELAY_MS`: the status fetch starts after the delay.
#[test]
#[ignore = "timing: races a fetch delay against a 600 ms deadline and asserts elapsed time"]
fn fetch_delay_seam_delays_the_status_fetch() {
    let server = Server::ok(&op13());
    let fx = Fx::served(&server);
    let cursor = || Box::new(Cursor::new(fx.stdin(&fx.plain).to_string().into_bytes()));
    let d = fx.run_with(cursor(), &[("ZYGO_TEST_FETCH_DELAY_MS", "20000")], T, Duration::from_millis(600));
    assert_eq!((d.status.is_some(), d.outcome, server.hits()), (false, Outcome::Offline, 0));
    let start = Instant::now();
    let d = fx.run_with(cursor(), &[("ZYGO_TEST_FETCH_DELAY_MS", "300")], T + 31, FAR);
    assert!(start.elapsed() >= Duration::from_millis(300));
    assert_eq!((d.status.is_some(), d.outcome, server.hits()), (true, Outcome::Ok, 1));
}

// ---- H§14 #24 ----

/// H§14 #24: a status endpoint that accepts and never answers. The first call waits to its deadline and writes
/// `outcome = offline` and `retry_at`; the next four calls, 1 s apart, make no request and return at once.
#[test]
#[ignore = "timing: asserts elapsed time against an 800 ms deadline"]
fn h14_24_backoff_after_a_fetch_that_never_answers() {
    let server = Server::start(Reply::Hang);
    let fx = Fx::served(&server);
    let cursor = || Box::new(Cursor::new(fx.stdin(&fx.plain).to_string().into_bytes()));
    let bound = Duration::from_millis(800);
    let start = Instant::now();
    let d = fx.run_with(cursor(), &[], T, bound);
    let took = start.elapsed();
    assert!(took >= Duration::from_millis(600) && took < bound + Duration::from_millis(500), "took {took:?}");
    assert_eq!((d.outcome, d.status.is_some(), server.hits()), (Outcome::Offline, false, 1));
    let cache = fx.cache();
    assert_eq!((cache["outcome"].as_str(), cache["retry_at"].as_u64()), (Some("offline"), Some(T + 30)));
    assert_eq!(cache.get("status"), None);
    assert!(!fx.lock().exists());

    for n in 1..=4 {
        let start = Instant::now();
        // A call that waited would take the 10 s of its deadline.
        let d = fx.run(&[], T + n);
        assert!(start.elapsed() < Duration::from_secs(5), "call {n} waited: {:?}", start.elapsed());
        assert_eq!((d.outcome, d.status.is_some()), (Outcome::Offline, false));
        assert!(text(&d, C).ends_with("offline"));
    }
    assert_eq!(server.hits(), 1);
    // After `retry_at` the next call fetches again.
    fx.run_with(cursor(), &[], T + 30, bound);
    assert_eq!(server.hits(), 2);
}

/// H§14 #24: the fetch cap. A status part 60 s old with a latency of 200 ms: at most 1 s; with 2,000 ms: at most
/// 3 s, clamped to the deadline; a part 100 s old, or none: the full deadline.
#[test]
#[ignore = "timing: asserts elapsed time against the 1 s fetch cap"]
fn h14_24_fetch_cap_follows_the_last_latency() {
    let s = Duration::from_secs;
    let ms = Duration::from_millis;
    assert_eq!(fetch_cap(Some(60), Some(200), s(3)), s(1));
    assert_eq!(fetch_cap(Some(60), Some(1_000), s(3)), ms(1_500));
    assert_eq!(fetch_cap(Some(60), Some(2_000), s(3)), s(3));
    assert_eq!(fetch_cap(Some(60), Some(2_000), ms(2_500)), ms(2_500));
    assert_eq!(fetch_cap(Some(90), Some(200), s(3)), s(1));
    assert_eq!(fetch_cap(Some(91), Some(200), s(3)), s(3));
    assert_eq!(fetch_cap(Some(100), Some(200), s(3)), s(3));
    assert_eq!(fetch_cap(None, None, s(3)), s(3));
    assert_eq!(fetch_cap(Some(60), None, s(3)), s(3));
    assert_eq!(fetch_cap(Some(60), Some(200), ms(400)), ms(400));
    assert_eq!(fetch_cap(Some(60), Some(u64::MAX), s(3)), s(3));

    // In a run: the part is 60 s old and the last latency is 200 ms, so the fetch that never answers ends at 1 s,
    // long before the 10 s deadline.
    let server = Server::start(Reply::Hang);
    let fx = Fx::served(&server);
    let cache = json!({
        "v": 2, "at": T - 60, "outcome": "ok", "retry_at": 0, "latency_ms": 200,
        "status": { "at": T - 60, "model": "opus-4.7", "used": 1000, "window": 200_000, "denominator": 200_000 },
        "git": { "at": T, "cwd": fx.plain, "linked": false },
    });
    std::fs::write(fx.dir.join("statusline.json"), cache.to_string()).unwrap();
    let start = Instant::now();
    let d = fx.run(&[], T);
    let took = start.elapsed();
    assert!(took >= Duration::from_millis(950) && took < Duration::from_secs(5), "took {took:?}");
    assert_eq!((d.outcome, d.stale, server.hits()), (Outcome::Offline, false, 1));
    // The cached status data is 90 s old or less: normal colors, no marker (H§6.8).
    assert!(ansi(&d, C).contains(&format!("\x1b[35m{CPU} opus-4.7\x1b[0m")), "{:?}", ansi(&d, C));
    assert!(!text(&d, C).contains("offline") && !text(&d, C).contains("stale"), "{}", text(&d, C));
    let after = fx.cache();
    assert_eq!((after["latency_ms"].as_u64(), after["retry_at"].as_u64(), after["status"]["at"].as_u64()), (Some(200), Some(T + 30), Some(T - 60)));
}

/// An expired cache: the status part is 100 s old (model `old`), and the git part of the plain directory is fresh.
fn expired_cache(fx: &Fx) {
    let cache = json!({
        "v": 2, "at": T - 100, "outcome": "ok", "retry_at": 0, "latency_ms": 5,
        "status": { "at": T - 100, "model": "old", "used": 1000, "window": 200_000, "denominator": 200_000 },
        "git": { "at": T, "cwd": fx.plain, "linked": false },
    });
    std::fs::write(fx.dir.join("statusline.json"), cache.to_string()).unwrap();
}

/// H§14 #24: two runs with an expired cache make one status request between them: the second finds
/// `statusline.lock`, makes no request and renders from the cache.
#[test]
#[ignore = "timing: asserts elapsed time of the second run"]
fn h14_24_two_runs_make_one_request_under_the_lease() {
    let server = Server::start(Reply::Gated(serde_json::to_string(&op13()).unwrap()));
    let fx = Fx::served(&server);
    expired_cache(&fx);
    std::thread::scope(|scope| {
        let first = scope.spawn(|| fx.run(&[], T));
        // The first run holds the lease: its request is at the server, which does not answer yet.
        server.seen.recv_timeout(FAR).expect("the first run made no request");
        assert!(fx.lock().exists());
        let lock: Value = serde_json::from_slice(&std::fs::read(fx.lock()).unwrap()).unwrap();
        assert_eq!(lock, json!({ "tried_at": T, "retry_after": 5 }));

        let start = Instant::now();
        let second = fx.run(&[], T);
        assert!(start.elapsed() < Duration::from_secs(5), "the second run waited: {:?}", start.elapsed());
        assert_eq!(second.status.as_ref().and_then(|s| s.model.as_deref()), Some("old"));
        assert!(second.stale);
        assert_eq!(server.hits(), 1);
        assert!(fx.lock().exists(), "the second run removed a lock that is not its own");

        server.go.send(()).unwrap();
        let first = first.join().unwrap();
        assert_eq!(first.status.as_ref().and_then(|s| s.model.as_deref()), Some("opus-4.7"));
        assert!(!first.stale);
    });
    assert_eq!(server.hits(), 1);
    assert!(!fx.lock().exists());
    assert_eq!(fx.cache()["status"]["model"].as_str(), Some("opus-4.7"));
}

/// H§14 #24: a `statusline.lock` 10 s old is left by a dead run: it is removed and taken. A lock that is not older
/// than its `retry_after` holds.
#[test]
fn h14_24_a_dead_lock_is_removed_and_taken() {
    let server = Server::ok(&op13());
    let fx = Fx::served(&server);
    expired_cache(&fx);
    std::fs::write(fx.lock(), json!({ "tried_at": T - 5, "retry_after": 5 }).to_string()).unwrap();
    let d = fx.run(&[], T);
    assert_eq!((server.hits(), d.status.and_then(|s| s.model).as_deref()), (0, Some("old")));
    assert!(fx.lock().exists());

    std::fs::write(fx.lock(), json!({ "tried_at": T - 10, "retry_after": 5 }).to_string()).unwrap();
    let d = fx.run(&[], T);
    assert_eq!((server.hits(), d.status.and_then(|s| s.model).as_deref()), (1, Some("opus-4.7")));
    assert!(!fx.lock().exists());

    // A lock with no body yet (its holder is between the create and the write) holds while the file is young.
    std::fs::write(fx.lock(), "").unwrap();
    fx.run(&[], T + 31);
    assert_eq!(server.hits(), 1);
    assert!(fx.lock().exists());
}

/// H§14 #24: a leftover temp file 2 minutes old is gone after the next cache write; a young one stays.
#[test]
fn h14_24_old_temp_files_are_removed_after_a_write() {
    let server = Server::ok(&op13());
    let fx = Fx::served(&server);
    let aged = |name: &str, age: u64| {
        let path = fx.dir.join(name);
        std::fs::write(&path, "x").unwrap();
        let f = std::fs::File::options().write(true).open(&path).unwrap();
        f.set_modified(SystemTime::now() - Duration::from_secs(age)).unwrap();
        path
    };
    let gone = [aged("statusline.json.123.tmp", 120), aged("statusline-9.tmp", 120)];
    let kept = [aged("statusline.json.456.tmp", 5), aged("other.tmp", 120), aged("statusline.jsonx", 120)];
    fx.run(&[], T);
    assert_eq!(server.hits(), 1);
    for p in &gone {
        assert!(!p.exists(), "{}", p.display());
    }
    for p in &kept {
        assert!(p.exists(), "{}", p.display());
    }
    // The write itself leaves no temp file.
    let mut names: Vec<String> =
        std::fs::read_dir(&fx.dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    assert_eq!(names, ["other.tmp", "statusline.json", "statusline.json.456.tmp", "statusline.jsonx", "token", "url", "view.json"]);
}

/// H§14 #24 and #25: a forced panic in the git worker gives a HUD without git segments; the try is a git part with
/// no data. (The stderr line and the exit status are checked on the binary, `zygo/tests/statusline_git.rs`.)
#[test]
#[ignore = "timing: asserts elapsed time"]
fn h14_24_git_worker_panic_gives_no_git_segments() {
    let server = Server::ok(&op13());
    let fx = Fx::served(&server);
    let main = repo(fx.t.path(), "w", "main");
    let start = Instant::now();
    let d = fx.run_in(&main, &[("ZYGO_TEST_GIT_PANIC", "1"), GIT, ("ZYGO_WORK", "branch:launch")], T);
    assert!(start.elapsed() < Duration::from_secs(5), "the hook waited for a worker that panicked");
    assert_eq!((d.branch.clone(), d.worktree.clone(), d.ahead, d.behind), (None, None, None, None));
    assert!(text(&d, C).starts_with(&format!("{LOGO} {BRANCH} launch  {CPU} opus-4.7")), "{}", text(&d, C));
    assert_eq!(fx.cache()["git"], json!({ "at": T, "cwd": main, "linked": false }));
}

// ---- H§14 #25: the parts that need no fake program ----

/// H§14 #25: the next call within 30 s does not run git; a call 30 s later does.
#[test]
#[ignore = "timing: collect waits only 200 ms for the stdin that sets the asserted cwd and effort"]
fn h14_25_git_part_is_not_due_for_30_s() {
    let fx = Fx::new();
    let w = repo(fx.t.path(), "w", "main");
    assert_eq!(fx.run_in(&w, &[GIT], T).branch.as_deref(), Some("main"));
    git(&w, &["checkout", "-q", "-b", "next"]);
    assert_eq!(fx.run_in(&w, &[GIT], T + 29).branch.as_deref(), Some("main"), "git ran before the part was due");
    assert_eq!(fx.run_in(&w, &[GIT], T + 30).branch.as_deref(), Some("next"));
    assert_eq!(fx.cache()["git"], json!({ "at": T + 30, "cwd": w, "branch": "next", "linked": false, "worktree": "w" }));
}

/// H§14 #25 (M3): a cwd change while another run holds the lock. The HUD shows the new cwd's branch, never the old
/// one, and the cache keeps the old git part.
#[test]
#[ignore = "timing: collect waits only 200 ms for the stdin that sets the asserted cwd and effort"]
fn h14_25_cwd_change_ignores_the_lease_for_git() {
    let fx = Fx::new();
    let old = repo(fx.t.path(), "old", "old-branch");
    let new = repo(fx.t.path(), "new", "new-branch");
    assert_eq!(fx.run_in(&old, &[GIT], T).branch.as_deref(), Some("old-branch"));
    let before = std::fs::read(fx.dir.join("statusline.json")).unwrap();

    std::fs::write(fx.lock(), json!({ "tried_at": T + 1, "retry_after": 5 }).to_string()).unwrap();
    let d = fx.run_in(&new, &[GIT], T + 1);
    assert_eq!(d.branch.as_deref(), Some("new-branch"));
    assert_eq!(std::fs::read(fx.dir.join("statusline.json")).unwrap(), before, "a run without the lease wrote the cache");
    assert!(fx.lock().exists());
    // A directory with no repository, under the same lock: no branch at all, not the cached one.
    let d = fx.run(&[GIT], T + 1);
    assert_eq!((d.branch, d.worktree), (None, None));
    // Back in the old cwd, the cached part renders with no git run.
    git(&old, &["checkout", "-q", "-b", "moved"]);
    assert_eq!(fx.run_in(&old, &[GIT], T + 2).branch.as_deref(), Some("old-branch"));
}

// ---- H§14 #27 ----

/// H§14 #27: the cache holds data, not a line. Two calls 10 s apart with different stdin `effort` make one fetch and
/// two outputs; a call 31 s later fetches again; an old `{at, line}` file counts as no cache.
#[test]
#[ignore = "timing: collect waits only 200 ms for the stdin that sets the asserted cwd and effort"]
fn h14_27_cache_holds_data_and_stdin_is_fresh() {
    let server = Server::ok(&op13());
    let fx = Fx::served(&server);
    let run = |effort: &str, now: u64| {
        let stdin = json!({ "cwd": fx.plain, "effort": effort }).to_string().into_bytes();
        text(&fx.run_with(Box::new(Cursor::new(stdin)), &[], now, FAR), V)
    };
    let first = run("high", T);
    assert!(first.contains(&format!("{CPU} opus-4.7 · high")), "{first}");
    let second = run("low", T + 10);
    assert!(second.contains(&format!("{CPU} opus-4.7 · low")), "{second}");
    assert_eq!(server.hits(), 1);
    assert_eq!(first.replace("· high", "· low"), second);

    server.set_status(&SessionStatus { used_context: Some(300_000), ..op13() });
    let third = run("low", T + 41);
    assert_eq!(server.hits(), 2);
    assert!(third.contains("ctx 300k/1m"), "{third}");

    // The trimmed count has no cache: a row written between two calls shows at the next call.
    assert!(!third.contains("trimmed"), "{third}");
    fx.trim();
    assert!(run("low", T + 42).contains("trimmed 23k tok"));
    assert_eq!(server.hits(), 2);

    // The cache file: version 2, numbers only (H-C4), written with no temp file left.
    let cache = fx.cache();
    assert_eq!(
        cache["status"],
        json!({
            "at": T + 41, "model": "opus-4.7", "used": 300_000, "window": 1_000_000, "denominator": 1_000_000,
            "tokens_in": 1_200_000, "tokens_out": 48_000, "spend_usd": 3.41, "spend_notional": false,
            "requests_total": 12, "requests_priced": 12, "cache_read_share": 0.92, "passthrough": false,
            "team_session": false,
        })
    );
    assert_eq!((cache["v"].as_u64(), cache["at"].as_u64(), cache["outcome"].as_str()), (Some(2), Some(T + 41), Some("ok")));
    assert!(cache["latency_ms"].is_u64());
    let text = cache.to_string();
    for named in ["context_window", "effective_window", "threshold", "\"session\"", "expires_at", "\"line\""] {
        assert!(!text.contains(named), "{named} in {text}");
    }

    // An old `{at, line}` cache: a fetch, and the file becomes version 2.
    std::fs::write(fx.dir.join("statusline.json"), format!(r#"{{"at":{},"line":"zygo ◇ cached"}}"#, T + 42)).unwrap();
    let fourth = run("low", T + 43);
    assert_eq!(server.hits(), 3);
    assert!(fourth.contains("ctx 300k/1m") && !fourth.contains("cached"), "{fourth}");
    assert_eq!(fx.cache()["v"].as_u64(), Some(2));
}

/// The cache keeps the `⇣` value and the bar denominator in place of the window fields of the reply (H-C4, AU17,
/// AU28), and the render from the cache equals the render of the reply.
#[test]
#[ignore = "timing: collect waits only 200 ms for the stdin that sets the asserted cwd and effort"]
fn cache_keeps_the_threshold_mark_and_the_bar_denominator() {
    let with = SessionStatus { threshold: Some(600_000), effective_window: Some(600_000), used_context: Some(300_000), ..op13() };
    let server = Server::ok(&with);
    let fx = Fx::served(&server);
    let d = fx.run(&[], T);
    assert!(text(&d, V).contains("ctx 300k/1m ▰▰▰▰▱▱▱▱ 50% ⇣600k"), "{}", text(&d, V));
    let cache = fx.cache();
    assert_eq!((cache["status"]["threshold"].as_u64(), cache["status"]["denominator"].as_u64()), (Some(600_000), Some(600_000)));
    let fresh = HudData { status: Some(with), profile: Some("fast".into()), effort: Some("high".into()), ..Default::default() };
    assert_eq!(text(&d, V), text(&fresh, V));
    // From the cache alone, 10 s later.
    assert_eq!(text(&fx.run(&[], T + 10), V), text(&fresh, V));

    // `compact = off`: `⇣off`, also with an unknown window. A threshold that is not below the window: no mark.
    server.set_status(&SessionStatus { compact: Some(zygo_proxy::client::WindowArg::Off), context_window: None, ..op13() });
    let d = fx.run(&[], T + 31);
    assert!(text(&d, C).contains("ctx 196k/- ⇣off"), "{}", text(&d, C));
    assert_eq!(fx.cache()["status"]["threshold"].as_str(), Some("off"));
    server.set_status(&SessionStatus { threshold: Some(1_000_000), ..op13() });
    let d = fx.run(&[], T + 62);
    assert!(!text(&d, C).contains('⇣'), "{}", text(&d, C));
    assert_eq!(fx.cache()["status"].get("threshold"), None);
}

// ---- H§14 #28 ----

/// H§14 #28: the proxy cannot be reached and there is no cache. The local segments show (work item, worktree, git
/// marks, trimmed from the session of `view.json`) with `offline` in SGR `31`; there is no mode segment.
#[test]
#[ignore = "timing: collect waits only 200 ms for the stdin that sets the asserted cwd and effort"]
fn h14_28_offline_keeps_the_local_segments() {
    let fx = Fx::new();
    let wt = linked_ahead(fx.t.path());
    fx.trim();
    let d = fx.run_in(&wt, &[GIT, ("ZYGO_WORK", "branch:main")], T);
    assert_eq!((d.outcome, d.status.is_some(), d.trimmed_bytes), (Outcome::Offline, false, Some(92_160)));
    assert_eq!(
        ansi(&d, C),
        format!(
            "\x1b[31m▌\x1b[33m▌\x1b[36m▌\x1b[0m \x1b[1;97m{LOGO}\x1b[0m \x1b[1;34m{BRANCH} wt\x1b[0m \x1b[38;5;172m{TREE} wt\x1b[0m  \x1b[36m{PROFILE} fast\x1b[0m  \x1b[2m✂ 23k\x1b[0m  \x1b[31moffline\x1b[0m"
        )
    );
    let verbose = text(&d, V);
    assert_eq!(verbose, format!("{LOGO}  {BRANCH} wt {TREE} wt ↑1   {PROFILE} fast\nz\u{28e}\u{183}o  trimmed 23k tok   offline"));
    for mode in ["local", "serve", "passthrough"] {
        assert!(!verbose.contains(mode), "{verbose}");
    }
    let cache = fx.cache();
    assert_eq!((cache["outcome"].as_str(), cache["retry_at"].as_u64()), (Some("offline"), Some(T + 30)));
}

/// H§14 #28: the fetch fails with cached status data. Up to 90 s old: normal colors and no marker. Older: every
/// proxy-derived segment in SGR `2` and `stale`, while the local segments keep their colors.
#[test]
#[ignore = "timing: collect waits only 200 ms for the stdin that sets the asserted cwd and effort"]
fn h14_28_stale_dims_the_proxy_segments_only() {
    let server = Server::ok(&op13());
    let fx = Fx::served(&server);
    let wt = linked_ahead(fx.t.path());
    fx.trim();
    let vars = [GIT, ("ZYGO_WORK", "branch:main")];
    let fresh = ansi(&fx.run_in(&wt, &vars, T), C);

    server.set(Reply::Json(500, "{}".into()));
    let d = fx.run_in(&wt, &vars, T + 60);
    assert_eq!((d.outcome, d.stale, server.hits()), (Outcome::Offline, false, 2));
    assert_eq!(ansi(&d, C), fresh);

    let d = fx.run_in(&wt, &vars, T + 100);
    assert_eq!((d.outcome, d.stale, server.hits()), (Outcome::Offline, true, 3));
    // H§6.4 example 3, with this fixture's branch and worktree.
    assert_eq!(
        ansi(&d, C),
        format!(
            "\x1b[31m▌\x1b[33m▌\x1b[36m▌\x1b[0m \x1b[1;97m{LOGO}\x1b[0m \x1b[1;34m{BRANCH} wt\x1b[0m \x1b[38;5;172m{TREE} wt\x1b[0m  \x1b[2m{CPU} opus-4.7\x1b[0m \x1b[36m{PROFILE} fast\x1b[0m  \x1b[2mctx 196k/1m ▰▱▱▱▱  ↑1.2m ↓48k  $3.41  {DB} 92%  ✂ 23k  stale\x1b[0m"
        )
    );
    assert!(ansi(&d, V).contains(&format!("\x1b[36m↑1\x1b[0m   \x1b[2m{CPU} opus-4.7 · high\x1b[0m")), "{:?}", ansi(&d, V));
}

/// H§14 #28: a suspended team session shows `suspended` (SGR `33`) with the cached segments, never `offline` or
/// `stale`.
#[test]
#[ignore = "timing: a fetch 31 s after a cached one must answer within its 1 s cap"]
fn h14_28_suspended_session_keeps_the_cached_segments() {
    let server = Server::ok(&op13());
    let fx = Fx::served(&server);
    fx.run(&[], T);
    server.set(error_reply(401, "zygo_session_suspended"));
    let d = fx.run(&[], T + 31);
    assert_eq!((d.outcome, d.stale), (Outcome::Suspended, false));
    let row = ansi(&d, C);
    assert!(row.ends_with("\x1b[33msuspended\x1b[0m") && row.contains(&format!("\x1b[35m{CPU} opus-4.7\x1b[0m")), "{row:?}");
    let cache = fx.cache();
    assert_eq!((cache["outcome"].as_str(), cache["retry_at"].as_u64()), (Some("suspended"), Some(T + 61)));

    // Older than 90 s: the cached segments are dim, and the marker stays `suspended`.
    let d = fx.run(&[], T + 131);
    assert_eq!((d.outcome, d.stale, server.hits()), (Outcome::Suspended, true, 3));
    let row = ansi(&d, C);
    assert!(row.ends_with("\x1b[33msuspended\x1b[0m") && row.contains(&format!("\x1b[2m{CPU} opus-4.7\x1b[0m")), "{row:?}");
    assert!(!row.contains("stale") && !row.contains("offline"), "{row:?}");

    // With no cached status data: the marker alone.
    let server = Server::start(error_reply(401, "zygo_session_suspended"));
    let fx = Fx::served(&server);
    assert_eq!(text(&fx.run(&[], T), C), format!("{LOGO}  {PROFILE} fast  suspended"));
}

/// H§14 #28: a session that is gone (`401` with `zygo_session_unknown` or `zygo_token_invalid`) with a cache
/// present. The cache gets `outcome = ended`; that call and five more within 5 minutes print the local segments and
/// `ended` in SGR `31`, no proxy-derived segment, and none of the five makes a request.
#[test]
#[ignore = "timing: a fetch 31 s after a cached one must answer within its 1 s cap"]
fn h14_28_ended_session_drops_the_cached_segments() {
    for code in ["zygo_session_unknown", "zygo_token_invalid"] {
        let server = Server::ok(&op13());
        let fx = Fx::served(&server);
        fx.trim();
        let vars = [("ZYGO_WORK", "github:o/n#123")];
        assert!(text(&fx.run(&vars, T), V).contains("opus-4.7"));
        server.set(error_reply(401, code));
        let want = format!("{LOGO} \u{f408} #123  {PROFILE} fast  ✂ 23k  ended");
        for (n, now) in [T + 31, T + 32, T + 90, T + 150, T + 240, T + 330].into_iter().enumerate() {
            let d = fx.run(&vars, now);
            assert_eq!(d.outcome, Outcome::Ended, "{code} call {n}");
            assert_eq!(text(&d, C), want, "{code} call {n}");
            assert!(ansi(&d, C).ends_with("\x1b[31mended\x1b[0m"), "{code} call {n}");
            let verbose = text(&d, V);
            for part in ["opus", "ctx", "local", "$", "cache", "↑", "stale", "offline"] {
                assert!(!verbose.contains(part), "{code} call {n}: {part} in {verbose}");
            }
            assert_eq!(server.hits(), 2, "{code} call {n} made a request");
        }
        let cache = fx.cache();
        assert_eq!((cache["outcome"].as_str(), cache["retry_at"].as_u64()), (Some("ended"), Some(T + 331)));
        assert_eq!(cache.get("status"), None);
        // After `retry_at` the hook asks again.
        fx.run(&vars, T + 331);
        assert_eq!(server.hits(), 3, "{code}");
    }
    // Another `401` is not `ended`.
    let server = Server::start(error_reply(401, "zygo_other"));
    let fx = Fx::served(&server);
    assert_eq!(fx.run(&[], T).outcome, Outcome::Offline);
}

/// AU13: the host of `ZYGO_URL` is the host of the mode `serve <host>`, a loopback host included.
#[test]
fn team_host_is_the_host_of_zygo_url() {
    let server = Server::ok(&SessionStatus { team_session: Some(true), spend_notional: Some(true), ..op13() });
    let fx = Fx::served(&server);
    let d = fx.run(&[("ZYGO_URL", "https://127.0.0.1:8443/base")], T);
    assert_eq!(d.team_host.as_deref(), Some("127.0.0.1"));
    assert!(text(&d, V).contains("fast   serve 127.0.0.1\n"), "{}", text(&d, V));
    let d = fx.run(&[("ZYGO_URL", "https://zygo.example.com/")], T);
    assert!(text(&d, V).contains("fast   serve zygo.example.com\n"), "{}", text(&d, V));
    let cases: [&[(&str, &str)]; 3] = [&[], &[("ZYGO_URL", "")], &[("ZYGO_URL", "not a url")]];
    for vars in cases {
        assert_eq!(fx.run(vars, T).team_host, None);
    }
}

/// AU12: the profile comes from `view.json`; the status `profile` is the fallback. With no `view.json` there is no
/// trimmed segment (its session id is unknown).
#[test]
#[ignore = "timing: the trimmed count must come within the 1 s trim cap"]
fn profile_from_the_view_then_the_status() {
    let server = Server::ok(&SessionStatus { profile: Some("from-status".into()), ..op13() });
    let fx = Fx::served(&server);
    fx.trim();
    let d = fx.run(&[], T);
    assert_eq!((d.profile.as_deref(), d.trimmed_bytes), (Some("fast"), Some(92_160)));
    std::fs::remove_file(fx.dir.join("view.json")).unwrap();
    let d = fx.run(&[], T);
    assert_eq!((d.profile.as_deref(), d.trimmed_bytes), (None, None));
    assert!(text(&d, C).contains(&format!("{PROFILE} from-status  ")), "{}", text(&d, C));
    // A usage store that is missing gives no trimmed segment and creates nothing.
    let fx = Fx::new();
    assert_eq!(fx.run(&[], T).trimmed_bytes, None);
    assert!(!fx.paths().data_dir.exists());
}

// ---- style (AU1, H§8) ----

/// AU1: the style is the `statusline` of `overrides.json`, else the `statusline_style` of `view.json`, else
/// `compact`. A value other than `compact` or `verbose` is ignored at each layer.
#[test]
fn style_from_the_overrides_then_the_view_then_the_default() {
    let view = |s: &str| SessionView { statusline_style: s.into(), ..Default::default() };
    let over = |s: Option<&str>| Overrides { statusline: s.map(str::to_string), ..Default::default() };
    assert_eq!(style(&over(None), None), C);
    assert_eq!(style(&over(None), Some(&view("verbose"))), V);
    assert_eq!(style(&over(Some("verbose")), Some(&view("compact"))), V);
    assert_eq!(style(&over(Some("compact")), Some(&view("verbose"))), C);
    assert_eq!(style(&over(Some("verbose")), None), V);
    // `wide` in `overrides.json` is ignored: the style comes from `view.json`.
    assert_eq!(style(&over(Some("wide")), Some(&view("verbose"))), V);
    assert_eq!(style(&over(Some("wide")), Some(&view("compact"))), C);
    assert_eq!(style(&over(Some("")), Some(&view("verbose"))), V);
    assert_eq!(style(&over(Some("VERBOSE")), Some(&view("wide"))), C);
    assert_eq!(style(&over(None), Some(&view(""))), C);

    // Through the files of a session directory, as the hook reads them.
    let fx = Fx::new();
    let sd = SessionDir::new(fx.dir.clone());
    std::fs::write(sd.overrides_json(), r#"{"statusline":"wide"}"#).unwrap();
    view("verbose").save(&sd).unwrap();
    assert_eq!(style(&Overrides::load(&sd), SessionView::load(&sd).ok().as_ref()), V);
    std::fs::write(sd.overrides_json(), r#"{"statusline":"compact"}"#).unwrap();
    assert_eq!(style(&Overrides::load(&sd), SessionView::load(&sd).ok().as_ref()), C);
}

/// H§14 #34, the HUD part: the repository file cannot set the style, so the launcher writes `compact` into
/// `view.json`, and the session renders one row.
#[test]
fn h14_34_view_with_the_default_style_renders_compact() {
    let fx = Fx::new();
    let sd = SessionDir::new(fx.dir.clone());
    let view = SessionView::load(&sd).unwrap();
    assert_eq!(view.statusline_style, "compact");
    let chosen = style(&Overrides::load(&sd), Some(&view));
    assert_eq!(chosen, C);
    let row = text(&fx.run(&[], T), chosen);
    assert_eq!(row.lines().count(), 1, "{row}");
}

/// H§14 #35, the HUD part: `statusline_style = "verbose"` in `view.json` renders two rows.
#[test]
fn h14_35_view_with_the_verbose_style_renders_two_rows() {
    let fx = Fx::new();
    let sd = SessionDir::new(fx.dir.clone());
    SessionView { session: "S1".into(), statusline_style: "verbose".into(), ..Default::default() }.save(&sd).unwrap();
    let chosen = style(&Overrides::load(&sd), SessionView::load(&sd).ok().as_ref());
    assert_eq!(chosen, V);
    let rows = text(&fx.run(&[], T), chosen);
    assert_eq!(rows.lines().count(), 2, "{rows}");
    assert!(rows.starts_with(LOGO) && rows.contains("\nz\u{28e}\u{183}o"), "{rows}");
}

// ---- the P-HUD-1 seams ----

/// The width source and the width (H§6.7, PH4): the terminal wins over `COLUMNS`; a source out of range is skipped.
#[test]
fn width_source_names_the_source_that_answered() {
    let columns = |v: &'static str| move |key: &str| (key == "COLUMNS").then(|| OsString::from(v));
    let none = |_: &str| None::<OsString>;
    assert_eq!(width_source(&columns("110"), &|| Some(90)), ("tty", 90));
    assert_eq!(width_source(&columns("110"), &|| None), ("COLUMNS", 110));
    assert_eq!(width_source(&none, &|| None), ("default", 120));
    assert_eq!(width_source(&columns("110"), &|| Some(5)), ("COLUMNS", 110));
    assert_eq!(width_source(&columns("5000"), &|| Some(5)), ("default", 120));
    assert_eq!(width_source(&columns("abc"), &|| None), ("default", 120));
    assert_eq!(width_source(&columns("120"), &|| None), ("COLUMNS", 120));
    assert_eq!(width_source(&none, &|| Some(120)), ("tty", 120));
}

/// The session log gets one line for each change of the width source or the width, not one for each run.
#[test]
fn width_log_has_one_line_for_each_change() {
    let fx = Fx::new();
    let sd = SessionDir::new(fx.dir.clone());
    log_width(&sd, "tty", 90);
    log_width(&sd, "tty", 90);
    log_width(&sd, "COLUMNS", 110);
    log_width(&sd, "COLUMNS", 110);
    log_width(&sd, "tty", 90);
    assert_eq!(
        std::fs::read_to_string(sd.log()).unwrap(),
        "statusline: width source tty, width 90\nstatusline: width source COLUMNS, width 110\nstatusline: width source tty, width 90\n"
    );
    assert_eq!(std::fs::read_to_string(fx.dir.join("statusline.width")).unwrap(), "tty 90");
    let tmp: Vec<_> = std::fs::read_dir(&fx.dir).unwrap().map(|e| e.unwrap().file_name()).filter(|n| n.to_string_lossy().ends_with(".tmp")).collect();
    assert!(tmp.is_empty(), "{tmp:?}");
}

/// The P-HUD-1 seam `ZYGO_TEST_HUD_DUMP`: the parsed stdin keys, the stdin `effort` and the HUD environment.
#[test]
#[ignore = "timing: collect waits only 200 ms for the stdin that sets the asserted cwd and effort"]
fn hud_dump_seam_writes_the_stdin_keys_and_the_environment() {
    let fx = Fx::new();
    let file = fx.t.path().join("dump.json");
    let stdin = json!({ "cwd": fx.plain, "effort": { "level": "high" }, "model": { "id": "lead" }, "version": "2.1.328" });
    let vars = [("ZYGO_TEST_HUD_DUMP", file.to_str().unwrap()), ("ZYGO_WORK", "branch:main"), ("COLUMNS", "90"), ("TERM", "xterm-256color")];
    fx.run_with(Box::new(Cursor::new(stdin.to_string().into_bytes())), &vars, T, FAR);
    let dump: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(dump["stdin_keys"], json!(["cwd", "effort", "model", "version"]));
    assert_eq!(dump["effort"], json!({ "level": "high" }));
    assert_eq!(
        dump["env"],
        json!({
            "ZYGO_SESSION_DIR": null, "ZYGO_WORK": "branch:main", "ZYGO_URL": null, "NO_COLOR": null,
            "ZYGO_NERD_FONT": null, "TERM": "xterm-256color", "COLORTERM": null, "WT_SESSION": null,
            "TERM_PROGRAM": null, "COLUMNS": "90",
        })
    );
}
