//! zygo mail send|read|ls|watch and /zygo:mail (M§8; owner: nodes ML-12, ML-13); the command-start mail sweep (M§5, AU71);
//! zygo mail sync (mail sync design S§8.1-S§8.3; owner: node MS-17).

use std::ffi::OsStr;
use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime};

use anyhow::{anyhow, bail};
use clap::{Args, Subcommand};
use zygo_config::{sync_for, Config, SyncOff};
use zygo_core::esc;
use zygo_core::verbs::MAIL_SLASH_ARGS;
use zygo_core::paths::Paths;
use zygo_mail_sync::address::{resolve_to, To};
use zygo_mail_sync::env::{Env, NoSeam, SystemClock, TmuxWake};
use zygo_mail_sync::ops;
use zygo_mail_sync::outbox::{count, refused_count, send_remote_with};
use zygo_mail_sync::public::LsRemote;
use zygo_mail_sync::spawn::{trigger, ChildSpawner};
use zygo_mail_sync::state::{load as load_sync_state, SyncPaths};
use zygo_mail_sync::sync::{run_with, Mode, Opts, Report, SyncError};
use zygo_redact::secret::DeadlinePolicy;
use zygo_workspace::identity::{valid_ws_id, workspace_id};
use zygo_workspace::mail::{self, mail_dir, names, store, MailError, BODY_MAX};
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

/// S§7.1: the `zygo mail ls` line when sync is set for the workspace and `[mail] host` is not (the config check holds
/// the same text without `zygo: `, `crates/zygo-config/src/check.rs:15`).
const NO_HOST_LINE: &str = "zygo: set [mail] host to turn on mail sync; zygo does not publish the system host name";

/// S§8.3 step 3, D4: the CLI line when another sync of the workspace still holds the lock after `LOCK_WAIT`.
const BUSY_LINE: &str = "zygo: another mail sync of this workspace holds the lock; try again";

/// S§6.6 Limit, D4: the once-per-workspace note at the first `zygo mail sync` that ends without a stop.
const CI_NOTE: &str = "zygo: each mail sync push can start CI and webhooks on the forge. The commits hold [skip ci]; webhooks, other workflow events and CI systems that do not read [skip ci] still run. Exclude zygo/** in such CI configs.";

#[derive(Args)]
pub struct MailArgs {
    #[command(subcommand)]
    pub sub: Option<MailCmd>,
}

#[derive(Subcommand)]
pub enum MailCmd {
    /// Send a message to a member name, a pane key, all, or <name>@<host>
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
#[command(group(clap::ArgGroup::new("op").multiple(false).args(["prune", "take_host", "new_identity", "drop_host"])))]
pub struct SyncArgs {
    /// Workspace name; default: the workspace of this session or folder
    #[arg(long)]
    pub name: Option<String>,
    /// Prune the side branch in this sync
    #[arg(long)]
    pub prune: bool,
    /// Take the mail host name for this workspace (after the collision line)
    #[arg(long)]
    pub take_host: bool,
    /// Make a new mail identity for this workspace (after the twin line)
    #[arg(long)]
    pub new_identity: bool,
    /// Delete the waiting mail, registry and ack of a host that will never sync again
    #[arg(long, value_name = "host")]
    pub drop_host: Option<String>,
    /// Answer yes to the question of --drop-host
    #[arg(long, requires = "drop_host")]
    pub yes: bool,
    /// The hidden child form (S§8.1 child row, AU44): the workspace id the hook passes.
    #[arg(long, hide = true, requires = "background")]
    pub ws_id: Option<String>,
    /// The hidden child form (S§8.1 child row, AU44): silent, `Mode::Child`.
    #[arg(long, hide = true, requires = "ws_id", conflicts_with_all = ["name", "prune", "take_host", "new_identity", "drop_host", "yes"])]
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
                (Some(n), _) => match member_arg(&paths, &ws, &n).and_then(|m| names::resolve(&ws, &m, None).map_err(Into::into)) {
                    Ok(t) => t.into_iter().map(|t| t.key).collect(),
                    Err(e) => return finish(Err(e)),
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
                (Some(n), _) => match member_arg(&paths, &ws, &n).and_then(|m| names::resolve(&ws, &m, None).map_err(Into::into)) {
                    Ok(mut t) if t.len() == 1 => t.remove(0).key,
                    Ok(_) => return finish(Err(UsageError(format!("zygo mail watch: {n} is not one member")).into())),
                    Err(e) => return finish(Err(e)),
                },
                (None, Some(k)) => k,
                (None, None) => {
                    return finish(Err(UsageError("zygo mail watch: no member for this pane; pass a name".into()).into()))
                }
            };
            // S§8.1 row 3 (review F1/F3): the sync child of this binary at start and each `sync_gap`; no wait.
            let env = sync_env(paths.clone());
            let mut step = watch_sync_step(&env, &ws);
            watch_with(&mail_dir(&paths, &ws.id), &key, d, WATCH_POLL, &Instant::now, &mut std::io::stdout().lock(), &mut step)
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
            let (mut o, mut e) = (Vec::new(), Vec::new());
            let subject = subject.unwrap_or_default();
            let code = send_cli(&sync_env(paths.clone()), &ws, pane.as_deref(), &to, &subject, &body, &cfg, &mut o, &mut e);
            if !o.is_empty() {
                print_out(&String::from_utf8_lossy(&o));
            }
            if !e.is_empty() {
                print_err(&String::from_utf8_lossy(&e));
            }
            code
        }
    }
}

/// `zygo mail send` after the body is read (M§8; mail sync design S§7.2, S§8.1 row 1, S§8.6 "At send"). A `to` with
/// `@` is resolved against the host in force (none with sync off): a refused form is its line; `<name>@<own host>`
/// is the local send of `<name>`; a remote address goes through the S§7.2 rules and the send-time secret pass into
/// the outbox, then one sync child starts with no gap. A `to` with no `@` is the local send (ML-4), and its wake goes
/// through `env.wake`. The lines go to `out` and `err`; the result is the exit code.
#[allow(clippy::too_many_arguments)]
pub fn send_cli(
    env: &Env,
    ws: &Workspace,
    pane: Option<&str>,
    to: &str,
    subject: &str,
    body: &str,
    cfg: &Config,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let now = env.clock.now();
    let mut to = to.to_string();
    if to.contains('@') {
        let settings = sync_for(cfg, &ws.name);
        let own: Vec<String> = settings.iter().map(|s| s.host.clone()).collect();
        match resolve_to(&to, &own) {
            Err(e) => {
                let _ = writeln!(err, "{}", e.line());
                return 1;
            }
            Ok(To::Local(n)) => to = n,
            Ok(To::All) => to = "all".into(),
            Ok(To::Remote(a)) => {
                let sp = match SyncPaths::new(&env.paths, &ws.id) {
                    Ok(sp) => sp,
                    Err(e) => {
                        let _ = writeln!(err, "{}", MailError::Io(e));
                        return 1;
                    }
                };
                let from = pane.and_then(|k| ws.members.get(k)).and_then(|m| m.valid_name());
                let sent = send_remote_with(
                    &sp,
                    settings.as_ref(),
                    from,
                    &a,
                    subject,
                    body,
                    &cfg.redaction,
                    &ws.main_root,
                    now,
                    &env.secret_policy,
                );
                return match sent {
                    Err(e) => {
                        let _ = writeln!(err, "{}", e.line());
                        1
                    }
                    Ok(id) => {
                        let _ = writeln!(out, "sent {id} to {}@{}", esc(&a.name), esc(&a.host));
                        let store = Store::open(&env.paths, &ws.id);
                        trigger(env, &ws.id, None, &|t| store.log_error(&format!("mail send: {t}")));
                        0
                    }
                };
            }
        }
    }
    let r = match mail::send(&env.paths, ws, pane, &to, subject, body, &cfg.mail, now) {
        Ok(r) => r,
        Err(e) => {
            let _ = writeln!(err, "{e}");
            return 1;
        }
    };
    for s in &r.sent {
        let label = ws.members.get(&s.key).and_then(|m| m.valid_name()).unwrap_or(&s.key);
        let _ = writeln!(out, "sent {} to {}", esc(&s.id), esc(label));
    }
    let keys: Vec<String> = r.sent.iter().map(|s| s.key.clone()).collect();
    if !keys.is_empty() {
        env.wake.wake(&env.paths, ws, &keys, &cfg.mail, now);
    }
    for x in &r.refused {
        let _ = writeln!(err, "{}; to {}", x.error, member(ws, &x.key));
    }
    if r.refused.is_empty() { 0 } else { 1 }
}

/// `zygo mail watch` (M-G5, M§8): each `poll`, the count of files in `new/` of `key` under `dir`; when a file has
/// landed, one line `<id> <from> <subject>` per message that parses on `out` (none when no file parses), exit 0,
/// nothing moved. Exit 1 when `timeout` has passed since the first `now()` with no file (no line), on a read error
/// (its text on stderr), or when the write to `out` fails (A7). The count comes before the clock, so mail already in
/// `new/` returns 0 even with a zero timeout (A4).
pub fn watch(dir: &Path, key: &str, timeout: Duration, poll: Duration, now: &dyn Fn() -> Instant, out: &mut dyn Write) -> i32 {
    watch_with(dir, key, timeout, poll, now, out, &mut || {})
}

/// [`watch`] with `sync()` before each look at `new/` (mail sync design S§8.1 row 3): in `zygo mail watch` the step
/// of [`watch_sync_step`], which starts the sync child at start and then each `sync_gap` when sync is on.
pub fn watch_with(
    dir: &Path,
    key: &str,
    timeout: Duration,
    poll: Duration,
    now: &dyn Fn() -> Instant,
    out: &mut dyn Write,
    sync: &mut dyn FnMut(),
) -> i32 {
    let start = now();
    loop {
        sync();
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

/// S§8.1 row 3 (MS-18 review F1/F3): the step of `zygo mail watch` for [`watch_with`]. With sync on for `ws` in the
/// user config of `env.paths`, it starts the sync child (`env.spawner`; `zygo mail sync --ws-id <id> --background` in
/// production) before the first look and then before the first look after each `sync_gap`, start to start, by
/// [`watch_sync_due`] on `env.clock`. It does not wait for the child, so a slow or hanging sync never holds a look,
/// and the child's import shows at a later look. It reads and writes no `mail/sync/last_start`: that file is the hook
/// rate limit (S§8.4), and `spawn::trigger` with a gap would skip the start of a watch after a recent hook start. A
/// child that does not start gives one `errors.log` line, as `trigger` does. A config that does not load, or sync off,
/// makes the step do nothing; the config is read once, when the step is made.
pub fn watch_sync_step<'a>(env: &'a Env, ws: &'a Workspace) -> impl FnMut() + 'a {
    let settings = user_config(&env.paths).ok().and_then(|c| sync_for(&c, &ws.name).ok());
    let store = Store::open(&env.paths, &ws.id);
    let mut last: Option<SystemTime> = None;
    move || {
        let Some(s) = &settings else { return };
        let now = env.clock.now();
        if watch_sync_due(last, now, s.sync_gap) {
            last = Some(now);
            if let Err(e) = env.spawner.spawn_sync(&ws.id) {
                store.log_error(&format!("mail watch: cannot start the mail sync child: {e}"));
            }
        }
    }
}

/// S§8.1 row 3, the S§8.4 rule: `true` when no sync ran in this watch (`last` is `None`), when `now - last >= gap`,
/// or when `now` is earlier than `last` (a clock set back).
pub fn watch_sync_due(last: Option<SystemTime>, now: SystemTime, gap: Duration) -> bool {
    last.is_none_or(|l| now.duration_since(l).map_or(true, |d| d >= gap))
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

/// The command-start mail sweep (M§5, AU71): `cur/` files older than `[mail] keep` in each workspace, and the outbox
/// sweep of mail sync (S§9 last paragraph, D9): `outbox/refused/` files older than `keep` and `outbox/tmp/` files older
/// than 1 h; queued outbox files are kept. A user config
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
        if let Ok(sp) = SyncPaths::new(paths, id) {
            zygo_mail_sync::outbox::sweep(&sp, keep, now);
        }
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

/// D6: the workspace whose name is `name`, resolved as `ws forget-pane --name` resolves it
/// (`zygo_workspace::admin::named`): a readable state in a real state folder; a shared name is its line (exit 1).
fn workspace_named(paths: &Paths, name: &str) -> anyhow::Result<Workspace> {
    let all = list_all(paths);
    match zygo_workspace::admin::named(paths, &all, name, true) {
        Ok(Some(ws)) => Ok(ws.clone()),
        Ok(None) => Err(anyhow!("zygo: no workspace named {}", esc(name))),
        Err(line) => Err(anyhow!(line)),
    }
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
    std::panic::set_hook(Box::new(move |_| panic_line(store.as_ref(), &PANIC_LOGGED)));
    #[cfg(feature = "test-hooks")]
    if std::env::var_os("ZYGO_TEST_PANIC_SYNC").is_some() {
        panic!("test panic in mail sync --background");
    }
    match run_with(&sync_env(paths), ws_id, Mode::Child, &Opts::default()) {
        Ok(_) => 0,
        Err(e) => e.exit_code(Mode::Child),
    }
}

/// D7, C6: set once the hook of the hidden form has written its line in this process.
static PANIC_LOGGED: AtomicBool = AtomicBool::new(false);

/// D7, C6: the body of the hidden form's panic hook: the one `errors.log` line `mail sync: stopped: panic` to
/// `store`, at most once while `written` is false (a panic in a `std::thread::scope` thread panics again on the main
/// thread, and both reach the hook). No store: nothing.
fn panic_line(store: Option<&Store>, written: &AtomicBool) {
    if let Some(s) = store
        && !written.swap(true, Ordering::SeqCst)
    {
        s.log_error("mail sync: stopped: panic");
    }
}

/// S§7.6, S§8.2, S§8.3 (D1, D2, D5): `--drop-host` asks on `err` and reads the answer from `input`, and runs no
/// sync; `--take-host` and `--new-identity` run their operation and the sync; else one `Mode::Cli` sync of `ws`.
/// The lines go to `out` and `err`; the result is the exit code.
fn sync_cli(env: &Env, ws: &Workspace, a: &SyncArgs, input: &mut dyn BufRead, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    if let Some(host) = &a.drop_host {
        let dropped = {
            let mut ask = |prompt: &str| -> bool {
                if a.yes {
                    let _ = writeln!(err, "{prompt} y (--yes)");
                    return true;
                }
                let _ = write!(err, "{prompt} ");
                let _ = err.flush();
                super::plugins::confirm(input)
            };
            ops::drop_host(env, &ws.id, host, &mut ask)
        };
        return match dropped {
            Ok(None) => {
                let _ = writeln!(out, "zygo: host {} not dropped", esc(host));
                0
            }
            Ok(Some(n)) => {
                let _ = writeln!(out, "zygo: host {} dropped; {n} messages deleted", esc(host));
                0
            }
            Err(e) => {
                print_error(&e, &ws.name, err);
                e.exit_code(Mode::Cli)
            }
        };
    }
    let done = if a.take_host {
        ops::take_host(env, &ws.id)
    } else if a.new_identity {
        ops::new_identity(env, &ws.id)
    } else {
        run_with(env, &ws.id, Mode::Cli, &Opts { prune: a.prune })
    };
    match done {
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

/// D1: the CI note on `out` when `r.ci_note` (S§6.6); each note on `err` as it is (fixed text, or text
/// `public::classify` already scrubbed and escaped); then the summary on `out`. Write errors are ignored.
fn print_report(r: &Report, out: &mut dyn Write, err: &mut dyn Write) {
    if r.ci_note {
        let _ = writeln!(out, "{CI_NOTE}");
    }
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
pub(crate) fn workspace_of(
    paths: &Paths,
    cwd: &Path,
    zygo_workspace: Option<&OsStr>,
    git_bound: Duration,
) -> anyhow::Result<Workspace> {
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
                lines.push("no members".into());
            }
            lines.extend(sync_line(paths, ws));
        }
        Some(n) => {
            for t in names::resolve(ws, &member_arg(paths, ws, n)?, None)? {
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

/// `secs` since the Unix epoch as RFC 3339 UTC (the `sent_at` form of M§5); `None` out of range.
fn rfc3339(secs: u64) -> Option<String> {
    time::OffsetDateTime::from_unix_timestamp(i64::try_from(secs).ok()?)
        .ok()?
        .format(&time::format_description::well_known::Rfc3339)
        .ok()
}

/// S§10.3, S§7.1 (P3): the last line of `zygo mail ls`. Sync set: `mail sync: last sync <time>, <n> in the outbox,
/// <m> refused[, last error <check>]`, with `<time>` RFC 3339 UTC or `never`, and `-` for a value that cannot be
/// read. No host: the S§7.1 line; another form error: its `off_line`. Sync not set, or a config that does not load:
/// no line. Reads config and files only (no git).
fn sync_line(paths: &Paths, ws: &Workspace) -> Option<String> {
    let cfg = user_config(paths).ok()?;
    match sync_for(&cfg, &ws.name) {
        Err(SyncOff::NotOn) => None,
        Err(SyncOff::NoHost) => Some(NO_HOST_LINE.into()),
        Err(o) => Some(off_line(o, &ws.name)),
        Ok(_) => {
            let sp = SyncPaths::new(paths, &ws.id).ok()?;
            let st = load_sync_state(&sp);
            let last = match &st {
                Ok(s) => match s.last_ok {
                    None => "never".to_string(),
                    Some(t) => rfc3339(t).unwrap_or_else(|| "-".into()),
                },
                Err(_) => "-".into(),
            };
            let n = count(&sp).map_or_else(|_| "-".to_string(), |n| n.to_string());
            let m = refused_count(&sp).map_or_else(|_| "-".to_string(), |n| n.to_string());
            let mut line = format!("mail sync: last sync {last}, {n} in the outbox, {m} refused");
            if let Ok(s) = &st
                && let Some(c) = &s.last_error
            {
                line.push_str(&format!(", last error {}", esc(c)));
            }
            Some(line)
        }
    }
}

/// S§7.2 (P1, D4): the member value of `read`, `ls` and `watch`. A value with no `@` is kept as it is; a value with
/// `@` is resolved against the host in force (none with sync off or a config that does not load), and only
/// `<name>@<own host>` is taken, as `<name>`; any other value with `@` is the not-local error. Only a value with `@`
/// loads the config, so a local listing never fails on config.
fn member_arg(paths: &Paths, ws: &Workspace, n: &str) -> anyhow::Result<String> {
    if !n.contains('@') {
        return Ok(n.to_string());
    }
    let own = user_config(paths).ok().and_then(|c| sync_for(&c, &ws.name).ok()).map(|s| vec![s.host]).unwrap_or_default();
    match resolve_to(n, &own) {
        Ok(To::Local(name)) => Ok(name),
        _ => Err(anyhow!("zygo: {n} is not a local address; zygo mail read, ls and watch take a member of this machine")),
    }
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

    /// D2, D4 (review round 1, T1): a CLI sync that finds the lock held after the wait prints `BUSY_LINE` and exits 1.
    #[test]
    fn busy_is_the_busy_line_and_exit_1() {
        use zygo_mail_sync::sync::{Mode, SyncError};
        let mut err = Vec::new();
        super::print_error(&SyncError::Busy, "app", &mut err);
        assert_eq!(String::from_utf8(err).unwrap(), format!("{}\n", super::BUSY_LINE));
        assert_eq!(super::BUSY_LINE, "zygo: another mail sync of this workspace holds the lock; try again");
        assert_eq!(SyncError::Busy.exit_code(Mode::Cli), 1);
    }

    /// D7, C6 (review round 1, F3): a panic in a scoped thread panics again on the main thread; the hook body writes
    /// one `errors.log` line for both.
    #[test]
    fn panic_line_is_written_at_most_once() {
        let d = tempfile::TempDir::new().unwrap();
        let data = d.path().join("data");
        let cache = d.path().join("cache");
        let paths = zygo_core::paths::Paths {
            config_file: d.path().join("config.toml"),
            sessions_dir: cache.join("sessions"),
            marketplaces_dir: cache.join("marketplaces"),
            plugin_store: data.join("plugins"),
            approvals_file: data.join("approvals.toml"),
            usage_db: data.join("usage.db"),
            cache_dir: cache,
            data_dir: data,
        };
        let store = zygo_workspace::state::Store::open(&paths, "0123456789abcdef");
        let written = std::sync::atomic::AtomicBool::new(false);
        super::panic_line(Some(&store), &written);
        super::panic_line(Some(&store), &written);
        let log = std::fs::read_to_string(store.dir().join("errors.log")).unwrap();
        assert_eq!(log.lines().count(), 1, "{log}");
        assert!(log.ends_with(" mail sync: stopped: panic\n"), "{log}");
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
