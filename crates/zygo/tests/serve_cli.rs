//! N37: the CLI against a team server (§17 `zygo doctor`, §14 `zygo usage --sync`, §6.1 layer 7 owner note, N30 and
//! N49 notes). Loopback only; team tokens from `team::new_token()` at run time; every variable goes to a child.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use tempfile::TempDir;
use zygo_config::{load_with_builtin, LoadContext};
use zygo_core::paths::Paths;
use zygo_proxy::serve::{self, ServeHandle, ServeOpts};
use zygo_proxy::server::StartOpts;
use zygo_proxy::team::new_token;
use zygo_usage::{RequestRow, Store};

struct Home {
    d: TempDir,
}

impl Home {
    fn new() -> Home {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("bin")).unwrap();
        std::fs::write(d.path().join("config.toml"), "").unwrap();
        Home { d }
    }
    fn path(&self) -> &Path {
        self.d.path()
    }
    fn config(&self, text: &str) {
        std::fs::write(self.path().join("config.toml"), text).unwrap();
    }
    /// The usage database `Paths::from_env` gives for this home.
    fn db(&self) -> Store {
        #[cfg(unix)]
        let p = self.path().join(".local/share/zygo/usage.db");
        #[cfg(windows)]
        let p = self.path().join("zygo").join("usage.db");
        Store::open(&p).unwrap()
    }
}

struct Srv {
    _d: TempDir,
    root: PathBuf,
    h: ServeHandle,
}

/// The built-in layer without its marketplace (no network).
fn builtin() -> toml::Table {
    let mut t = toml::Table::try_from(zygo_config::builtin::defaults()).unwrap();
    t.remove("marketplace");
    t
}

/// A team server on 127.0.0.1:0 with a token for alice, then `extra`; returns alice's token.
fn server(extra: &str) -> (Srv, String) {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().to_path_buf();
    let (alice, ah) = new_token();
    let config_file = root.join("config.toml");
    std::fs::write(&config_file, format!("[serve]\ntokens = [{{ label = \"alice\", sha256 = \"{ah}\" }}]\n{extra}")).unwrap();
    let paths = Paths {
        config_file: config_file.clone(),
        cache_dir: root.join("cache"),
        data_dir: root.join("data"),
        sessions_dir: root.join("cache/sessions"),
        marketplaces_dir: root.join("cache/marketplaces"),
        plugin_store: root.join("data/plugins"),
        approvals_file: root.join("data/approvals.toml"),
        usage_db: root.join("data/usage.db"),
    };
    let ctx = LoadContext { paths, repo: None, team: None };
    let h = serve::start(ServeOpts {
        listen: "127.0.0.1:0".into(),
        tls: None,
        config_file,
        load: Arc::new(move || Ok(load_with_builtin(&ctx, builtin())?)),
        report: Arc::new(|_: &str| {}),
        pid_file: None,
        signals: false,
        start: StartOpts { usage_db: Some(root.join("data/usage.db")), ..Default::default() },
    })
    .unwrap();
    (Srv { _d: d, root, h }, alice)
}

fn closed_url() -> String {
    let l = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = l.local_addr().unwrap().port();
    drop(l);
    format!("http://127.0.0.1:{port}/")
}

fn command(home: &Path, args: &[&str], token: Option<&str>) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_zygo"));
    c.args(args)
        .env("HOME", home)
        .env("APPDATA", home)
        .env("LOCALAPPDATA", home)
        .env("ZYGO_CONFIG", home.join("config.toml"))
        .env("PATH", home.join("bin"))
        .current_dir(home)
        .env_remove("ZYGO_SESSION")
        .env_remove("ZYGO_SESSION_DIR")
        .env_remove("ZYGO_TEAM_TOKEN")
        .env_remove("ZYGO_N62_KEY");
    if let Some(t) = token {
        c.env("ZYGO_TEAM_TOKEN", t);
    }
    c
}

fn zygo(home: &Path, args: &[&str], token: Option<&str>) -> Output {
    command(home, args, token).output().unwrap()
}

/// `zygo` inside a session: `ZYGO_SESSION_DIR` = `dir`, no token, no server.
fn zygo_in(home: &Path, args: &[&str], dir: &Path) -> Output {
    command(home, args, None).env("ZYGO_SESSION_DIR", dir).env("ZYGO_SESSION", "01TESTSESSION").output().unwrap()
}

/// (code, stdout, stderr)
fn out(o: &Output) -> (Option<i32>, String, String) {
    (o.status.code(), String::from_utf8_lossy(&o.stdout).into_owned(), String::from_utf8_lossy(&o.stderr).into_owned())
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64
}

fn row(id: &str, label: &str) -> RequestRow {
    RequestRow {
        ts: now(),
        session: id.into(),
        token_label: Some(label.into()),
        route: "/anthropic/v1/messages".into(),
        model: "m".into(),
        provider: "p-x".into(),
        status: 200,
        estimated_usd: 2.5,
        ..Default::default()
    }
}

fn serve_cfg(url: &str) -> String {
    format!("[server]\nurl = \"{url}\"\ntoken = \"env:ZYGO_TEAM_TOKEN\"\n")
}

#[test]
fn doctor_reports_server_reachability() { // §17
    let h = Home::new();
    let (s, alice) = server("");
    let url = s.h.url().to_string();
    h.config(&serve_cfg(&url));
    let (code, stdout, _) = out(&zygo(h.path(), &["doctor"], Some(&alice)));
    assert_eq!(code, Some(0));
    assert!(stdout.lines().any(|l| l == format!("server: {url} reachable")), "{stdout}");
    s.h.shutdown();
    let (_, stdout, _) = out(&zygo(h.path(), &["doctor"], Some(&alice)));
    assert!(stdout.lines().any(|l| l.starts_with(&format!("server: {url} unreachable ("))), "{stdout}");
} // mutation: the bare url kept

#[test]
fn doctor_prints_the_server_url_without_userinfo() { // N5 (round 2), U1 (round 3)
    let h = Home::new();
    let userinfo = ["n37user", "n37pass"].join(":");
    let rule = "not allowed: the URL must not hold a user name or password (C8)";
    for (url, shown) in [
        (format!("http://{userinfo}@127.0.0.1:1/"), "server: http://127.0.0.1:1/"),
        (format!("http://{userinfo}@10.0.0.5:8443/"), "server: http://10.0.0.5:8443/"),
    ] {
        h.config(&format!("[server]\nurl = \"{url}\"\n"));
        let (_, stdout, stderr) = out(&zygo(h.path(), &["doctor"], None));
        assert!(stdout.lines().any(|l| l == format!("{shown} {rule}")), "{stdout}");
        assert!(!stdout.contains("n37user") && !stdout.contains("n37pass") && !stderr.contains("n37pass"), "{stdout}{stderr}");
    }
} // mutation: the url printed as configured

#[test]
fn doctor_prints_no_part_of_a_server_url_that_does_not_parse() { // U2 (round 3)
    let h = Home::new();
    let userinfo = ["n37user", "n37pass"].join(":");
    h.config(&format!("[server]\nurl = \"http://{userinfo}@127.0.0.1:99999/\"\n"));
    let (_, stdout, stderr) = out(&zygo(h.path(), &["doctor"], None));
    assert!(stdout.lines().any(|l| l == "server: [server] url is not a URL"), "{stdout}");
    assert!(!(stdout.clone() + &stderr).contains("n37user") && !(stdout + &stderr).contains("n37pass"));
} // mutation: the configured string printed

#[test]
fn usage_sync_replaces_pending_rows() { // §14, B2, R5
    let h = Home::new();
    let (s, alice) = server("");
    h.config(&serve_cfg(s.h.url().as_str()));
    let remote = Store::open(&s.root.join("data/usage.db")).unwrap();
    remote.insert_request(&row("A1", "alice")).unwrap();
    remote.insert_request(&row("A1", "alice")).unwrap();
    h.db().insert_requests(&[row("A1", "alice")]).unwrap();
    h.db().mark_sync_pending("A1").unwrap();
    assert_eq!(out(&zygo(h.path(), &["usage", "--sync"], Some(&alice))), (Some(0), "synced 1 of 1 sessions\n".into(), String::new()));
    let a = h.db().requests_for_session("A1").unwrap();
    assert_eq!(a.len(), 2);
    assert!(a.iter().all(|r| r.provider.is_empty() && r.estimated_usd == 0.0));
    assert!(h.db().sync_pending().unwrap().is_empty());
    assert_eq!(out(&zygo(h.path(), &["usage", "--sync"], Some(&alice))).1, "synced 0 of 0 sessions\n");
    h.config("");
    assert_eq!(
        out(&zygo(h.path(), &["usage", "--sync"], None)),
        (Some(1), String::new(), "zygo usage --sync: no server configured ([server] url)\n".into())
    );
    h.config(&serve_cfg(&closed_url()));
    h.db().mark_sync_pending("A1").unwrap();
    let (code, _, stderr) = out(&zygo(h.path(), &["usage", "--sync"], Some(&alice)));
    assert_eq!(code, Some(12), "{stderr}");
    assert!(stderr.starts_with("zygo usage --sync: session A1: server unreachable: "), "{stderr}");
    assert!(stderr.ends_with("synced 0 of 1 sessions\n"), "{stderr}");
    assert_eq!(h.db().sync_pending().unwrap(), ["A1"]);
    s.h.shutdown();
} // mutations: the not-built bail kept; insert instead of replace; no clear; unreachable → 0 or 1

#[test]
fn setting_verbs_read_the_team_layer() { // §6.1 layer 7 owner note, N30 and N49 notes
    let a = "[[marketplace]]\nname = \"team\"\nsource = \"github:example/agent-plugins\"\nref = \"v1.4.0\"\ncommit = \"COMMIT\"\nrequired = [\"lint\"]\n\
             [features]\nresults = true\ncompact_tool = true\n[features.redaction]\ncontent = true\n[guards]\nclaims = \"enforce\"\n"
        .replace("COMMIT", &"4f2c".repeat(10));
    let h = Home::new();
    let (s, alice) = server(&a);
    h.config(&serve_cfg(s.h.url().as_str()));
    for (args, needle) in [
        (&["plugins", "disable", "team:lint"][..], "refused: team:lint is required by the team config"),
        (&["compact-tool", "off"], "refused: `features.compact_tool` is set by the team config"),
        (&["results", "off", "--save"], "refused: `features.results` is set by the team config"),
        (&["redaction", "content", "off", "--save"], "refused: `features.redaction.content` is set by the team config"),
        (&["guards", "claims", "notify", "--save"], "refused: `guards.claims` is set by the team config"),
    ] {
        let (code, _, stderr) = out(&zygo(h.path(), args, Some(&alice)));
        assert_eq!(code, Some(1), "{args:?}: {stderr}");
        assert!(stderr.contains(needle), "{args:?}: {stderr}");
    }
    let (code, stdout, stderr) = out(&zygo(h.path(), &["units", "off", "--save"], Some(&alice)));
    assert_eq!((code, stdout.as_str()), (Some(0), "units off saved to user config\n"), "{stderr}"); // a default stays open
    s.h.shutdown();
    let h = Home::new();
    let (s, alice) = server("[features]\nwork_units = true\n");
    h.config(&serve_cfg(s.h.url().as_str()));
    let (code, _, stderr) = out(&zygo(h.path(), &["units", "off", "--save"], Some(&alice)));
    assert_eq!(code, Some(1), "{stderr}");
    assert!(stderr.contains("refused: `features.work_units` is set by the team config"), "{stderr}");
    h.config(&serve_cfg(&closed_url()));
    let (code, _, stderr) = out(&zygo(h.path(), &["compact-tool", "off"], Some(&alice)));
    assert_eq!(code, Some(12), "{stderr}");
    assert!(stderr.starts_with("zygo compact-tool: server unreachable: "), "{stderr}");
    s.h.shutdown();
} // mutations: any verb passes None; defaults sent as team keys; unreachable → 1

#[test]
fn marketplace_reads_the_team_layer() { // S2 (round 2), D11, N40.md:102
    let a = "[[marketplace]]\nname = \"team\"\nsource = \"github:example/agent-plugins\"\nref = \"v1.4.0\"\ncommit = \"COMMIT\"\n"
        .replace("COMMIT", &"4f2c".repeat(10));
    let h = Home::new();
    let (s, alice) = server(&a);
    h.config(&serve_cfg(s.h.url().as_str()));
    let (code, stdout, stderr) = out(&zygo(h.path(), &["marketplace", "list"], Some(&alice)));
    assert_eq!(code, Some(0), "{stderr}");
    assert!(stdout.lines().any(|l| l.starts_with("team ") && l.contains(" team plugins: ")), "{stdout}");
    let (code, _, stderr) = out(&zygo(h.path(), &["marketplace", "remove", "team"], Some(&alice)));
    assert_eq!(code, Some(1), "{stderr}");
    assert!(stderr.contains("marketplace team is from the team config and cannot be removed"), "{stderr}");
    s.h.shutdown();
} // mutation: `marketplace::verb` passes `None` → `team` not listed, remove says "is not configured"

#[test]
fn verbs_in_a_session_read_the_team_file() { // team-file decision
    let h = Home::new();
    let dir = h.path().join("s/01TESTSESSION");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("team.toml"), "[features]\ncompact_tool = true\n").unwrap();
    let run = || out(&zygo_in(h.path(), &["compact-tool", "off"], &dir));
    let (code, _, stderr) = run();
    assert_eq!(code, Some(1), "{stderr}");
    assert!(stderr.contains("refused: `features.compact_tool` is set by the team config"), "{stderr}");
    std::fs::remove_file(dir.join("team.toml")).unwrap();
    std::fs::create_dir(dir.join("team.toml")).unwrap();
    let (code, _, stderr) = run();
    assert_eq!(code, Some(1), "{stderr}");
    assert!(stderr.starts_with("zygo compact-tool: cannot read team.toml: "), "{stderr}");
} // mutation: in a session the verb fetches with the token

#[test]
fn profile_reads_the_team_layer() { // N62, §14 note (N37 review), §6.1 layer 2
    let h = Home::new();
    let (s, alice) = server("[profile.teamp]\nprompt = \"T\"\n");
    h.config(&serve_cfg(s.h.url().as_str()));
    let (code, stdout, stderr) = out(&zygo(h.path(), &["profile", "list"], Some(&alice)));
    assert_eq!((code, stdout.as_str()), (Some(0), "  teamp (team)\n"), "{stderr}");
    let (code, stdout, stderr) = out(&zygo(h.path(), &["profile", "show", "teamp"], Some(&alice)));
    assert_eq!(code, Some(0), "{stderr}");
    assert!(stdout.starts_with("profile teamp (team)\n") && stdout.contains("prompt = \"T\""), "{stdout}");
    let (wrong, _) = new_token();                                        // a token the server does not know: 401
    let (code, stdout, stderr) = out(&zygo(h.path(), &["profile", "list"], Some(&wrong)));
    assert_eq!((code, stdout.as_str()), (Some(1), ""), "{stderr}");
    assert!(stderr.starts_with("zygo profile: team config refused: "), "{stderr}");
    s.h.shutdown();
    let (code, stdout, stderr) = out(&zygo(h.path(), &["profile", "list"], Some(&alice)));
    assert_eq!((code, stdout.as_str()), (Some(12), ""), "{stderr}");
    assert!(stderr.starts_with("zygo profile: server unreachable: "), "{stderr}");
} // mutations: `loaded` passes `team: None` (→ "no profiles", "profile teamp: not found"); a team error falls back to
  // the local config (→ exit 0 for the refused and the unreachable case); a refusal mapped to 12 (→ 12, not 1)

#[test]
fn config_reads_the_team_layer() { // N62, §6.1 (`zygo config show` prints each key's source layer), controller N62-3
    let h = Home::new();
    let (s, alice) = server("[guards]\nclaims = \"enforce\"\n");
    h.config(&serve_cfg(s.h.url().as_str()));
    let team_line = "guards.claims = \"enforce\"  # team";
    let local = |stdout: &str| stdout.lines().any(|l| l.starts_with("guards.claims = ") && l.ends_with("  # builtin"));
    let not_read = |stderr: &str, why: &str| {
        stderr.lines().count() == 1 && stderr.starts_with(&format!("zygo config: team layer not read: {why}"))
    };
    let (code, stdout, stderr) = out(&zygo(h.path(), &["config", "show"], Some(&alice)));
    assert_eq!(code, Some(0), "{stderr}");
    assert!(stdout.lines().any(|l| l == team_line), "{stdout}");
    let (code, stdout, stderr) = out(&zygo(h.path(), &["config", "show", "--layer", "team"], Some(&alice)));
    assert_eq!(code, Some(0), "{stderr}");
    assert!(stdout.lines().any(|l| l == team_line), "{stdout}");
    let (wrong, _) = new_token();                                        // refused: 401
    let (code, stdout, stderr) = out(&zygo(h.path(), &["config", "show"], Some(&wrong)));
    assert_eq!(code, Some(0), "{stderr}");
    assert!(local(&stdout) && not_read(&stderr, "team config refused: "), "{stdout}{stderr}");
    let (code, stdout, stderr) = out(&zygo(h.path(), &["config", "show"], None));   // no team token
    assert_eq!(code, Some(0), "{stderr}");
    assert!(local(&stdout) && not_read(&stderr, "serve mode needs a team token"), "{stdout}{stderr}");
    s.h.shutdown();
    let (code, stdout, stderr) = out(&zygo(h.path(), &["config", "show"], Some(&alice)));
    assert_eq!(code, Some(0), "offline, show reports the local config: {stderr}");
    assert!(local(&stdout) && not_read(&stderr, "server unreachable: "), "{stdout}{stderr}");
    let (code, stdout, stderr) = out(&zygo(h.path(), &["config", "check"], Some(&alice)));
    assert_eq!((code, stdout.as_str()), (Some(0), "config ok\n"), "offline, check checks the local config: {stderr}");
    assert!(not_read(&stderr, "server unreachable: "), "{stderr}");
    let (code, _, stderr) = out(&zygo(h.path(), &["config", "show", "--layer", "bogus"], Some(&alice)));
    assert_eq!(code, Some(2), "{stderr}");
    assert!(!stderr.contains("team layer not read"), "the layer check comes before the team read: {stderr}");
} // mutations: `run()` passes `team: None` (→ `# builtin`, no `# team` line); a team error ends config with 12 or 1
  // (→ exit and empty stdout); the stderr line dropped (→ `not_read` false); the `zygo config: ` prefix not stripped
  // (→ "team layer not read: zygo config: …"); the team read before the layer check (→ stderr holds the line)

#[test]
fn doctor_reports_the_team_layer() { // N62, §17 `zygo doctor`, §6.1 (the server's catalog is authoritative)
    let team = "[[marketplace]]\nname = \"team\"\nsource = \"github:example/agent-plugins\"\nref = \"v1.4.0\"\ncommit = \"COMMIT\"\nrequired = [\"lint\"]\n"
        .replace("COMMIT", &"4f2c".repeat(10));
    let own = "[[model]]\nid = \"m1\"\n[model.facts]\nreasoning = \"always\"\neffort_levels = [\"low\", \"high\"]\ncontext_window = 200000\nmax_output_tokens = 64000\n\
               [[model.provider]]\nname = \"n62own\"\nbase_url = \"https://p.example.invalid\"\nupstream_model = \"u\"\ndialects = [\"anthropic\"]\nkey = \"env:ZYGO_N62_KEY\"\n";
    let local_line = "provider n62own: env:ZYGO_N62_KEY unset";
    let h = Home::new();
    let (s, alice) = server(&team);
    h.config(&(serve_cfg(s.h.url().as_str()) + own));
    let (code, stdout, stderr) = out(&zygo(h.path(), &["doctor"], Some(&alice)));
    assert_eq!(code, Some(0), "{stderr}");
    assert!(stdout.lines().any(|l| l == "plugin team:lint: not in store; run zygo plugins sync"), "{stdout}");
    assert!(!stdout.lines().any(|l| l.starts_with("provider ")), "the server holds the provider keys: {stdout}");
    let (wrong, _) = new_token();                                        // refused: 401
    let (code, stdout, stderr) = out(&zygo(h.path(), &["doctor"], Some(&wrong)));
    assert_eq!(code, Some(0), "doctor reports, it does not exit 1: {stderr}");
    assert!(stderr.lines().count() == 1 && stderr.starts_with("zygo doctor: team config refused: "), "{stderr}");
    assert!(stdout.lines().any(|l| l == local_line), "the local config is reported: {stdout}");
    s.h.shutdown();
    let (code, stdout, stderr) = out(&zygo(h.path(), &["doctor"], Some(&alice)));
    assert_eq!(code, Some(0), "doctor reports, it does not exit 12: {stderr}");
    assert!(stderr.lines().count() == 1 && stderr.starts_with("zygo doctor: server unreachable: "), "{stderr}");
    assert!(stdout.lines().any(|l| l == local_line), "the local config is reported: {stdout}");
} // mutations: `run()` passes `team: None` (→ provider lines, no team:lint line); a team error ends doctor (→ 1 or
  // 12, no report); the team error not printed (→ empty stderr); the report after a team error drops the local
  // config (→ no `provider n62own:` line)

/// A server that answers every request `401` with `body`; returns its URL.
fn refusing(body: &'static str) -> String {
    use std::io::{Read, Write};
    let l = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let url = format!("http://127.0.0.1:{}/", l.local_addr().unwrap().port());
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(mut s) = s else { continue };
            let (mut buf, mut chunk) = (Vec::new(), [0u8; 1024]);
            while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                match s.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => buf.extend_from_slice(&chunk[..n]),
                }
            }
            let _ = write!(s, "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        }
    });
    url
}

#[test]
fn server_error_text_prints_as_one_line() { // N62 K1
    let h = Home::new();
    h.config(&serve_cfg(&refusing(r#"{"error":{"message":"x\nzygo config: forged\u001b[2J"}}"#)));
    let (token, _) = new_token();
    let (code, _, stderr) = out(&zygo(h.path(), &["config", "show"], Some(&token)));
    assert_eq!(code, Some(0), "{stderr}");
    assert!(stderr.lines().count() == 1 && !stderr.contains('\u{1b}'), "{stderr:?}");
    let (code, _, stderr) = out(&zygo(h.path(), &["profile", "list"], Some(&token)));
    assert_eq!(code, Some(1), "{stderr}");
    assert!(stderr.lines().count() == 1 && !stderr.contains('\u{1b}'), "{stderr:?}");
} // mutation: `esc` removed from `verbs::team` (→ two lines, ESC present)

const NULL: &str = if cfg!(windows) { "NUL" } else { "/dev/null" };

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git").env("GIT_CONFIG_GLOBAL", NULL).env("GIT_CONFIG_NOSYSTEM", "1")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid", "-c", "commit.gpgsign=false"])
        .arg("-C").arg(dir).args(args).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn status_reads_the_team_layer() { // N66, §14 note 2026-11-19 (every config-reading verb), §13.5 (`zygo status` prints the repo policy)
    const FLOOR: &str = "[guards]\nmin_level = \"enforce\"\n";
    let levels = "@main (claims enforce, scope enforce, navigation enforce, output enforce, trailers enforce, repeat monitor)";
    let h = Home::new();
    git(h.path(), &["init", "-q", "-b", "main"]);
    std::fs::write(h.path().join(".zygo.toml"), "version = 1\n").unwrap();
    git(h.path(), &["add", ".zygo.toml"]);
    git(h.path(), &["commit", "-q", "-m", "init"]);
    let path = std::env::var_os("PATH").unwrap_or_default(); // the child's git reads the repository file
    let policy = |o: &Output| -> String {
        let (code, stdout, stderr) = out(o);
        assert_eq!(code, Some(0), "{stderr}");
        stdout.lines().find(|l| l.starts_with("repo policy: ")).unwrap_or_default().to_string()
    };
    // Outside a session: the team layer from the server.
    let (s, alice) = server(FLOOR);
    h.config(&serve_cfg(s.h.url().as_str()));
    let line = policy(&command(h.path(), &["status"], Some(&alice)).env("PATH", &path).output().unwrap());
    assert!(line.ends_with(levels), "{line}");
    s.h.shutdown();
    let (code, stdout, stderr) = out(&command(h.path(), &["status"], Some(&alice)).env("PATH", &path).output().unwrap());
    assert_eq!((code, stdout.as_str()), (Some(12), ""), "{stderr}");
    assert!(stderr.starts_with("zygo status: server unreachable: "), "{stderr}");
    // In a session: the team layer from `team.toml`; no token, and the server is gone.
    let dir = h.path().join("s/01TESTSESSION");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("team.toml"), FLOOR).unwrap();
    let in_session = || {
        command(h.path(), &["status"], None).env("PATH", &path).env("ZYGO_SESSION_DIR", &dir).env("ZYGO_SESSION", "01TESTSESSION")
            .output().unwrap()
    };
    let line = policy(&in_session());
    assert!(line.ends_with(levels), "{line}");
    // H3: each session override on its own; the refused one (claims, team-set) does not hide scope.
    std::fs::write(dir.join("team.toml"), "[guards]\nclaims = \"notify\"\n").unwrap();
    std::fs::write(dir.join("overrides.json"), r#"{"guards": {"claims": "monitor", "scope": "enforce"}}"#).unwrap();
    let line = policy(&in_session());
    assert!(line.ends_with("(claims notify, scope enforce, navigation monitor, output monitor, trailers notify, repeat monitor)"), "{line}");
    std::fs::remove_file(dir.join("team.toml")).unwrap();
    std::fs::create_dir(dir.join("team.toml")).unwrap();
    let (code, _, stderr) = out(&in_session());
    assert_eq!(code, Some(1), "{stderr}");
    assert!(stderr.starts_with("zygo status: cannot read team.toml: "), "{stderr}");
} // mutations: `merged` loads with `team: None` (→ `claims monitor, …`, both modes); unreachable falls back to the
  // local config (→ 0); in a session the team fetched from the server (→ token refusal, exit 1, no policy line)

#[test]
fn status_and_help_escape_the_profile_name() { // N66 J2: a `team.toml` the agent wrote cannot print terminal control codes
    let (esc, bel) = (char::from(0x1b), char::from(0x07));
    let h = Home::new();
    let dir = h.path().join("s/01TESTSESSION");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("team.toml"), format!("[user]\ndefault_profile = \"p\\u{:04x}q\\u{:04x}\"\n", 0x1b, 0x07)).unwrap();
    for (args, line) in [(&["status"][..], "profile: p\\u{1b}q\\u{07}"), (&["help", "profile"][..], "now: p\\u{1b}q\\u{07}")] {
        let (code, stdout, stderr) = out(&zygo_in(h.path(), args, &dir));
        assert_eq!(code, Some(0), "{stderr}");
        assert!(!stdout.contains(esc) && !stdout.contains(bel), "{stdout:?}");
        assert!(stdout.lines().any(|l| l == line), "{stdout:?}");
    }
} // mutations: no `esc` on the `profile:` line (→ raw ESC in status); none on the help `now:` value (→ raw ESC in help)
