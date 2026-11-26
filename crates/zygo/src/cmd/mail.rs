//! zygo mail send|read|ls|watch and /zygo:mail (M§8; owner: nodes ML-12, ML-13); the command-start mail sweep (M§5, AU71);
//! zygo mail sync (mail sync design S§8.1-S§8.3; owner: node MS-17).

use std::ffi::OsStr;
use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use anyhow::{anyhow, bail};
use clap::{Args, Subcommand};
use zygo_config::SyncOff;
use zygo_core::esc;
use zygo_core::verbs::MAIL_SLASH_ARGS;
use zygo_core::paths::Paths;
use zygo_mail_sync::env::{Env, NoSeam, SystemClock, TmuxWake};
use zygo_mail_sync::public::LsRemote;
use zygo_mail_sync::spawn::ChildSpawner;
use zygo_mail_sync::sync::{run_with, Mode, Opts, Report, SyncError};
use zygo_redact::secret::DeadlinePolicy;
use zygo_workspace::identity::{valid_ws_id, workspace_id};
use zygo_workspace::mail::{self, mail_dir, names, store, wake, MailError, BODY_MAX};
use zygo_workspace::state::{list_all, Store, Workspace};
use zygo_workspace::up::{find_checkout, GIT_BOUND};

use super::verbs::VerbCtx;
use super::{
    cwd_error, cwd_or_exit, finish, op5, op5_exit, print_err, print_out, tmux_program, user_config, user_config_or_exit,
    UsageError,
};

/// The text when neither `ZYGO_WORKSPACE` nor the checkout of the current folder names a workspace (as `zygo workspace`).
const NO_WORKSPACE: &str = "no workspace for this session or folder";

/// The git bound of the `/zygo:mail` checkout lookup: the bound of `admin::status`, so `/zygo:mail` fits the 4.5 s
/// hook bound as `/zygo:workspace` does (`crates/zygo-workspace/src/admin.rs:29-30`, W§12). The CLI keeps `GIT_BOUND`.
const SLASH_GIT_BOUND: Duration = Duration::from_secs(2);

/// The poll of `zygo mail watch` (A6): a send is seen within 1 s (M§13 #7).
const WATCH_POLL: Duration = Duration::from_millis(500);

/// S§8.3 step 3, D4: the CLI line when another sync of the workspace still holds the lock after `LOCK_WAIT`.
const BUSY_LINE: &str = "zygo: another mail sync of this workspace holds the lock; try again";

#[derive(Args)]
pub struct MailArgs {
    #[command(subcommand)]
    pub sub: Option<MailCmd>,
}

#[derive(Subcommand)]
pub enum MailCmd {
    /// Send a message to a member name, a pane key or all
    Send {
        to: String,
        #[arg(long)]
        subject: Option<String>,
        /// Read the body from this file
        #[arg(long, conflicts_with = "stdin")]
        file: Option<PathBuf>,
        /// `-`: read the body from stdin
        #[arg(value_parser = ["-"])]
        stdin: Option<String>,
    },
    /// Read the new messages of this pane's member, or of --name
    Read {
        #[arg(long)]
        name: Option<String>,
        #[arg(long, default_value_t = 10)]
        max: usize,
    },
    /// Unread counts per member, or the new messages of one
    Ls { name: Option<String> },
    /// Wait for new mail of this pane's member, or of <name>; print one line per message
    Watch {
        name: Option<String>,
        #[arg(long, default_value = "25m")]
        timeout: String,
    },
    /// Sync this workspace's mail with the side branch of its remote
    Sync(SyncArgs),
}

// `zygo mail sync` (S§8.1 row 4); a plain comment, so the about text is the doc of `MailCmd::Sync`.
#[derive(Args)]
pub struct SyncArgs {
    /// Workspace name; default: the workspace of this session or folder
    #[arg(long)]
    pub name: Option<String>,
    /// Prune the side branch in this sync
    #[arg(long)]
    pub prune: bool,
    /// The hidden child form (S§8.1 child row, AU44): the workspace id the hook passes.
    #[arg(long, hide = true, requires = "background")]
    pub ws_id: Option<String>,
    /// The hidden child form (S§8.1 child row, AU44): silent, `Mode::Child`.
    #[arg(long, hide = true, requires = "ws_id", conflicts_with_all = ["name", "prune"])]
    pub background: bool,
}

/// `zygo mail`: the OP5 check first; bare `zygo mail` is `zygo mail ls` (D1).
pub fn run(a: MailArgs) -> i32 {
    if let Some(code) = op5_exit() {
        return code;
    }
    if let Some(MailCmd::Sync(s)) = a.sub {
        return sync_run(s);
    }
    let paths = Paths::from_env();
    let cwd = match cwd_or_exit() {
        Ok(c) => c,
        Err(code) => return code,
    };
    let ws = match workspace_of(&paths, &cwd, std::env::var_os("ZYGO_WORKSPACE").as_deref(), GIT_BOUND) {
        Ok(w) => w,
        Err(e) => return finish(Err(e)),
    };
    let pane = std::env::var("ZYGO_PANE").ok().filter(|k| !k.is_empty());
    match a.sub {
        None => finish(ls_text(&paths, &ws, None)),
        Some(MailCmd::Ls { name }) => finish(ls_text(&paths, &ws, name.as_deref())),
        Some(MailCmd::Read { name, max }) => {
            let keys = match (name, pane) {
                (Some(n), _) => match names::resolve(&ws, &n, None) {
                    Ok(t) => t.into_iter().map(|t| t.key).collect(),
                    Err(e) => return finish(Err(e.into())),
                },
                (None, Some(k)) => vec![k],
                (None, None) => {
                    return finish(Err(UsageError("zygo mail read: no member for this pane; pass --name".into()).into()))
                }
            };
            let (text, failed) = read_text(&paths, &ws, &keys, max);
            if !text.is_empty() {
                print_out(&format!("{text}\n"));
            }
            match failed {
                Some(e) => {
                    print_err(&format!("{e}\n"));
                    1
                }
                None => 0,
            }
        }
        Some(MailCmd::Watch { name, timeout }) => {
            let Some(d) = zygo_config::parse_duration(&timeout) else {
                print_err(&format!("zygo: bad --timeout value {}: use <n>s, <n>m, <n>h or <n>d\n", esc(&timeout)));
                return 2;
            };
            let key = match (name, pane) {
                (Some(n), _) => match names::resolve(&ws, &n, None) {
                    Ok(mut t) if t.len() == 1 => t.remove(0).key,
                    Ok(_) => return finish(Err(UsageError(format!("zygo mail watch: {n} is not one member")).into())),
                    Err(e) => return finish(Err(e.into())),
                },
                (None, Some(k)) => k,
                (None, None) => {
                    return finish(Err(UsageError("zygo mail watch: no member for this pane; pass a name".into()).into()))
                }
            };
            watch(&mail_dir(&paths, &ws.id), &key, d, WATCH_POLL, &Instant::now, &mut std::io::stdout().lock())
        }
        Some(MailCmd::Sync(_)) => unreachable!("taken above"),
        Some(MailCmd::Send { to, subject, file, stdin }) => {
            let cfg = match user_config_or_exit(&paths, "mail") {
                Ok(c) => c,
                Err(code) => return code,
            };
            let body = match body_of(file.as_deref(), stdin.is_some(), &mut std::io::stdin().lock()) {
                Ok(b) => b,
                Err(e) => {
                    print_err(&format!("{e}\n"));
                    return 1;
                }
            };
            let subject = subject.unwrap_or_default();
            let r = match mail::send(&paths, &ws, pane.as_deref(), &to, &subject, &body, &cfg.mail, SystemTime::now()) {
                Ok(r) => r,
                Err(e) => {
                    print_err(&format!("{e}\n"));
                    return 1;
                }
            };
            let mut text = String::new();
            for s in &r.sent {
                let label = ws.members.get(&s.key).and_then(|m| m.valid_name()).unwrap_or(&s.key);
                text.push_str(&format!("sent {} to {}\n", esc(&s.id), esc(label)));
            }
            if !text.is_empty() {
                print_out(&text);
            }
            let keys: Vec<String> = r.sent.iter().map(|s| s.key.clone()).collect();
            wake::after_send(&paths, &ws, &tmux_program(), &keys, &cfg.mail);
            for x in &r.refused {
                print_err(&format!("{}; to {}\n", x.error, member(&ws, &x.key)));
            }
            if r.refused.is_empty() { 0 } else { 1 }
        }
    }
}

/// `zygo mail watch` (M-G5, M§8): each `poll`, the count of files in `new/` of `key` under `dir`; when a file has
/// landed, one line `<id> <from> <subject>` per message that parses on `out` (none when no file parses), exit 0,
/// nothing moved. Exit 1 when `timeout` has passed since the first `now()` with no file (no line), on a read error
/// (its text on stderr), or when the write to `out` fails (A7). The count comes before the clock, so mail already in
/// `new/` returns 0 even with a zero timeout (A4).
pub fn watch(dir: &Path, key: &str, timeout: Duration, poll: Duration, now: &dyn Fn() -> Instant, out: &mut dyn Write) -> i32 {
    let start = now();
    loop {
        match store::unread(dir, key).and_then(|n| if n > 0 { store::list(dir, key).map(Some) } else { Ok(None) }) {
            Err(e) => {
                print_err(&format!("{e}\n"));
                return 1;
            }
            Ok(Some(l)) => {
                let text: String =
                    l.iter().map(|s| format!("{} {} {}\n", esc(&s.id), esc(&s.from), esc(&s.subject))).collect();
                if let Err(e) = out.write_all(text.as_bytes()).and_then(|()| out.flush()) {
                    print_err(&format!("zygo mail watch: cannot write: {}\n", esc(&e.to_string())));
                    return 1;
                }
                return 0;
            }
            Ok(None) => {}
        }
        if now().duration_since(start) >= timeout {
            return 1;
        }
        std::thread::sleep(poll);
    }
}

/// `/zygo:mail [ls [<name>]]` (D2): lists only; any other args are a usage error and read nothing.
pub fn verb(ctx: &VerbCtx, args: &[String]) -> anyhow::Result<String> {
    op5()?;
    let name = match args {
        [] => None,
        [l] if l == "ls" => None,
        [l, n] if l == "ls" => Some(n.as_str()),
        _ => return Err(UsageError(format!("usage: /zygo:mail {MAIL_SLASH_ARGS}")).into()),
    };
    let cwd = std::env::current_dir().map_err(|e| anyhow!(cwd_error(&e)))?;
    let ws = workspace_of(&ctx.paths, &cwd, std::env::var_os("ZYGO_WORKSPACE").as_deref(), SLASH_GIT_BOUND)?;
    ls_text(&ctx.paths, &ws, name)
}

/// The command-start mail sweep (M§5, AU71): `cur/` files older than `[mail] keep` in each workspace. A user config
/// that does not load or a `keep` that does not parse gives no sweep (Open 2). A workspace folder or a `mail/` folder
/// that is a link is not opened (as `admin.rs:410`). Every error is ignored.
pub(crate) fn sweep(paths: &Paths, now: SystemTime) {
    let Ok(cfg) = user_config(paths) else { return };
    let Some(keep) = zygo_config::parse_duration(&cfg.mail.keep) else { return };
    let Ok(entries) = std::fs::read_dir(paths.workspaces_dir()) else { return };
    let real_dir = |p: &Path| std::fs::symlink_metadata(p).is_ok_and(|m| m.is_dir());
    for e in entries.flatten() {
        let name = e.file_name();
        let Some(id) = name.to_str().filter(|s| valid_ws_id(s)) else { continue };
        let dir = mail_dir(paths, id);
        if !real_dir(&paths.workspaces_dir().join(id)) || !real_dir(&dir) {
            continue;
        }
        store::sweep(&dir, keep, now);
    }
}

/// The production mail sync seams of the CLI (S§18; MS-18, MS-23): the system clock, the `git ls-remote` probe, no
/// seam, the `<zygo> mail sync --ws-id <id> --background` child of this binary (the binary rule of `ws run`), the
/// tmux wake and the default deadline policy of the secret pass (S§8.6).
pub fn sync_env(paths: Paths) -> Env {
    Env {
        paths,
        clock: Box::new(SystemClock),
        probe: Box::new(LsRemote),
        seam: Box::new(NoSeam),
        spawner: Box::new(ChildSpawner {
            zygo: super::member::zygo_binary(&std::env::current_exe().and_then(std::fs::canonicalize)),
        }),
        wake: Box::new(TmuxWake { tmux: tmux_program() }),
        secret_policy: DeadlinePolicy::default(),
    }
}

/// S§8.3 step 1 (D3): the CLI line of sync off for workspace `ws_name`.
fn off_line(off: SyncOff, ws_name: &str) -> String {
    let name = esc(ws_name);
    match off {
        SyncOff::NotOn => {
            format!("zygo: mail sync is off for workspace {name}; set sync in [mail.workspace.{name}] of the user config")
        }
        SyncOff::NoHost => "zygo: mail sync is off: no mail host; set host in [mail] of the user config".into(),
        SyncOff::BadHost => {
            "zygo: mail sync is off: the mail host is not 1 to 32 of a-z, 0-9 and -, starting with a letter or a digit".into()
        }
        SyncOff::BadBranch => {
            format!("zygo: mail sync is off: sync in [mail.workspace.{name}] is not zygo/ and 1 to 32 of a-z, 0-9 and -")
        }
        SyncOff::BadGap => "zygo: mail sync is off: sync_gap in [mail] is not <n>s, <n>m or <n>h".into(),
    }
}

/// D6: the readable workspace whose name is `name`.
fn workspace_named(paths: &Paths, name: &str) -> anyhow::Result<Workspace> {
    list_all(paths)
        .into_iter()
        .filter_map(|(_, r)| r.ok())
        .find(|w| w.name == name)
        .ok_or_else(|| anyhow!("zygo: no workspace named {}", esc(name)))
}

/// `zygo mail sync` (S§8.1 row 4): the workspace of `--name`, else of this session or folder; then the sync.
fn sync_run(a: SyncArgs) -> i32 {
    if a.background {
        return sync_background(Paths::from_env(), a.ws_id.as_deref().unwrap_or_default());
    }
    let paths = Paths::from_env();
    let ws = match &a.name {
        Some(n) => workspace_named(&paths, n),
        None => {
            let cwd = match cwd_or_exit() {
                Ok(c) => c,
                Err(code) => return code,
            };
            workspace_of(&paths, &cwd, std::env::var_os("ZYGO_WORKSPACE").as_deref(), GIT_BOUND)
        }
    };
    let ws = match ws {
        Ok(w) => w,
        Err(e) => return finish(Err(e)),
    };
    let (stdin, stdout, stderr) = (std::io::stdin(), std::io::stdout(), std::io::stderr());
    sync_cli(&sync_env(paths), &ws, &a, &mut stdin.lock(), &mut stdout.lock(), &mut stderr.lock())
}

/// `zygo mail sync --ws-id <id> --background` (S§8.1 child row, S§8.3; AU44, D7): one `Mode::Child` sync that
/// prints nothing on any path. A panic prints nothing either: the hook in place first writes the one `errors.log` line
/// `mail sync: stopped: panic` (C6, S-C2) when `ws_id` is a workspace id whose state folder is a real folder, else
/// nothing. An id that fails `valid_ws_id` stops at the gate (check `workspace`): exit 0, nothing written. The exit
/// code is that of `SyncError::exit_code(Mode::Child)`.
fn sync_background(paths: Paths, ws_id: &str) -> i32 {
    let real = valid_ws_id(ws_id)
        && std::fs::symlink_metadata(paths.workspaces_dir().join(ws_id)).is_ok_and(|m| m.is_dir());
    let store = real.then(|| Store::open(&paths, ws_id));
    std::panic::set_hook(Box::new(move |_| {
        if let Some(s) = &store {
            s.log_error("mail sync: stopped: panic");
        }
    }));
    #[cfg(feature = "test-hooks")]
    if std::env::var_os("ZYGO_TEST_PANIC_SYNC").is_some() {
        panic!("test panic in mail sync --background");
    }
    match run_with(&sync_env(paths), ws_id, Mode::Child, &Opts::default()) {
        Ok(_) => 0,
        Err(e) => e.exit_code(Mode::Child),
    }
}

/// S§8.2, S§8.3 (D1, D2): one `Mode::Cli` sync of `ws`, its lines on `out` and `err`; the exit code.
fn sync_cli(env: &Env, ws: &Workspace, a: &SyncArgs, _input: &mut dyn BufRead, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    match run_with(env, &ws.id, Mode::Cli, &Opts { prune: a.prune }) {
        Ok(r) => {
            print_report(&r, out, err);
            0
        }
        Err(e) => {
            print_error(&e, &ws.name, err);
            e.exit_code(Mode::Cli)
        }
    }
}

/// D1: each note on `err` as it is (fixed text, or text `public::classify` already scrubbed and escaped), then the
/// summary on `out`. Write errors are ignored.
fn print_report(r: &Report, out: &mut dyn Write, err: &mut dyn Write) {
    for n in &r.notes {
        let _ = writeln!(err, "{n}");
    }
    let _ = writeln!(out, "zygo: mail sync: {} sent, {} delivered", r.pushed, r.delivered);
}

/// D2: the one stderr line of a sync that did not run to its end. Write errors are ignored.
fn print_error(e: &SyncError, ws_name: &str, err: &mut dyn Write) {
    let line = match e {
        SyncError::Off(o) => off_line(*o, ws_name),
        SyncError::Busy => BUSY_LINE.to_string(),
        SyncError::Stopped { line, .. } => line.clone(),
    };
    let _ = writeln!(err, "{line}");
}

/// The workspace of `ZYGO_WORKSPACE`, else of the checkout of `cwd`, with the error texts of `zygo workspace`
/// (`admin::status`). A state folder that is a link reads as none.
fn workspace_of(paths: &Paths, cwd: &Path, zygo_workspace: Option<&OsStr>, git_bound: Duration) -> anyhow::Result<Workspace> {
    let (id, by_env) = match zygo_workspace {
        Some(v) => {
            let id = v
                .to_str()
                .filter(|s| valid_ws_id(s))
                .ok_or_else(|| anyhow!("invalid workspace id {}", esc(&v.to_string_lossy())))?;
            (id.to_string(), true)
        }
        None => {
            let (co, _, _) = find_checkout(cwd, git_bound).map_err(|_| anyhow!(NO_WORKSPACE))?;
            (workspace_id(&co.common_dir), false)
        }
    };
    let real = std::fs::symlink_metadata(paths.workspaces_dir().join(&id)).is_ok_and(|m| m.is_dir());
    let read = if real { Store::open(paths, &id).read().map_err(|e| anyhow!("{e}"))? } else { None };
    match read {
        Some(ws) => Ok(ws),
        None if by_env => bail!("no workspace with id {id}"),
        None => bail!(NO_WORKSPACE),
    }
}

/// `ls` (D3, D4): per member in key order `<key> <name or -> <unread>`, or with `name` each new message of its
/// targets as `<id> <from> <subject>`; nothing is moved.
fn ls_text(paths: &Paths, ws: &Workspace, name: Option<&str>) -> anyhow::Result<String> {
    let dir = mail_dir(paths, &ws.id);
    let mut lines = Vec::new();
    match name {
        None => {
            for t in names::members(ws) {
                let unread = store::unread(&dir, &t.key).map_or_else(|_| "-".to_string(), |n| n.to_string());
                lines.push(format!("{} {} {unread}", esc(&t.key), t.name.as_deref().map_or_else(|| "-".to_string(), esc)));
            }
            if lines.is_empty() {
                return Ok("no members".into());
            }
        }
        Some(n) => {
            for t in names::resolve(ws, n, None)? {
                for s in store::list(&dir, &t.key)? {
                    lines.push(format!("{} {} {}", esc(&s.id), esc(&s.from), esc(&s.subject)));
                }
            }
            if lines.is_empty() {
                return Ok("no new messages".into());
            }
        }
    }
    Ok(lines.join("\n"))
}

/// `<key> (<name>)`, or `<key>` for a member with no valid name.
fn member(ws: &Workspace, key: &str) -> String {
    match ws.members.get(key).and_then(|m| m.valid_name()) {
        Some(n) => format!("{} ({})", esc(key), esc(n)),
        None => esc(key),
    }
}

/// `read` (D3, D4): at most `max` messages in all over `keys`, each as `<id> <from> <sent_at> <subject>` and its
/// body lines indented by two spaces. A member that fails stops the read: the text has the messages already read
/// (moved to `cur/`), and the error names the member.
fn read_text(paths: &Paths, ws: &Workspace, keys: &[String], max: usize) -> (String, Option<String>) {
    let dir = mail_dir(paths, &ws.id);
    let mut lines = Vec::new();
    let mut left = max;
    let mut failed = None;
    for k in keys {
        if left == 0 {
            break;
        }
        let got = match store::read(&dir, k, left) {
            Ok(g) => g,
            Err(e) => {
                failed = Some(format!("{e}; mail of {}", member(ws, k)));
                break;
            }
        };
        left = left.saturating_sub(got.len());
        for m in got {
            lines.push(format!("{} {} {} {}", esc(&m.id), esc(&m.from), esc(&m.sent_at), esc(&m.subject)));
            lines.extend(m.body.lines().map(|l| format!("  {}", esc(l))));
        }
    }
    if lines.is_empty() && failed.is_none() {
        return ("no new messages".into(), None);
    }
    (lines.join("\n"), failed)
}

/// The body of `send` (D5): the file, else stdin when `stdin`, else empty. At most `BODY_MAX + 1` bytes are read; a
/// larger body is `BodyTooLarge`.
fn body_of(file: Option<&Path>, stdin: bool, input: &mut dyn Read) -> anyhow::Result<String> {
    let cap = BODY_MAX as u64 + 1;
    let mut buf = Vec::new();
    let (what, r) = match file {
        Some(p) => (esc(&p.to_string_lossy()), std::fs::File::open(p).and_then(|f| f.take(cap).read_to_end(&mut buf))),
        None if stdin => ("stdin".to_string(), input.take(cap).read_to_end(&mut buf)),
        None => return Ok(String::new()),
    };
    r.map_err(|e| anyhow!("zygo mail send: cannot read {what}: {}", esc(&e.to_string())))?;
    if buf.len() > BODY_MAX {
        return Err(MailError::BodyTooLarge.into());
    }
    String::from_utf8(buf).map_err(|_| anyhow!("zygo mail send: the body is not UTF-8"))
}

#[cfg(all(test, unix))]
mod tests {
    use super::verb;
    use crate::cmd::UsageError;

    fn v(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    /// M§8, O14: the slash form lists only; any other args are a usage error and read nothing.
    #[test]
    fn slash_takes_no_send_or_read_text() {
        let d = tempfile::TempDir::new().unwrap();
        let mut c = crate::cmd::compact::tests::ctx(d.path(), None);
        c.slash = true;
        for a in [&["send", "bob", "hi"][..], &["read"][..], &["ls", "a", "b"][..], &["bob"][..]] {
            let e = verb(&c, &v(a)).unwrap_err();
            assert!(e.downcast_ref::<UsageError>().is_some(), "{a:?}");
            assert_eq!(e.to_string(), "usage: /zygo:mail [ls [<name>]]", "{a:?}");
        }
    }

    /// M2: the slash checkout lookup has the 2 s bound of `admin::status`, so `/zygo:mail` fits the 4.5 s hook bound.
    #[test]
    fn slash_git_bound_is_that_of_workspace_status() {
        assert_eq!(super::SLASH_GIT_BOUND, std::time::Duration::from_secs(2));
        assert!(super::SLASH_GIT_BOUND < zygo_workspace::up::GIT_BOUND);
    }

    /// M§13 #7: a send is seen within 1 s, so the CLI poll is at most 1 s. Nothing waits.
    #[test]
    fn watch_poll_is_at_most_1s() {
        assert!(super::WATCH_POLL <= std::time::Duration::from_secs(1));
    }

    /// MS-18, MS-23: `sync_env` keeps the paths and the default secret-pass policy.
    #[test]
    fn sync_env_keeps_paths_and_default_policy() { // U3
        let p = zygo_core::paths::Paths::from_env();
        let e = super::sync_env(p.clone());
        assert_eq!(e.paths, p);
        assert_eq!(e.secret_policy, zygo_redact::secret::DeadlinePolicy::default());
    }
}

#[cfg(all(test, windows))]
mod op5_slash_mail {
    use super::verb;
    use crate::cmd::UsageError;

    #[test]
    fn op5_slash_mail() {
        let d = tempfile::TempDir::new().unwrap();
        let mut c = crate::cmd::compact::tests::ctx(d.path(), None);
        c.slash = true;
        let e = verb(&c, &[]).unwrap_err();
        assert!(e.downcast_ref::<UsageError>().is_some());
        assert_eq!(e.to_string(), "workspaces need tmux; run zygo in WSL");
    }
}
