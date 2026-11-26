//! The mail tools mail_send, mail_read, mail_list (M§8, M§8a item 2, M§9).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use serde_json::{json, Map, Value};
use zygo_config::{sync_for, LoadContext, MailConfig};
use zygo_core::paths::Paths;
use zygo_core::session_view::{Member, SessionView};
use zygo_mail_sync::address::{resolve_to, To};
use zygo_mail_sync::env::{Env, NoSeam, Spawner, SystemClock, TmuxWake};
use zygo_mail_sync::outbox::send_remote_with;
use zygo_mail_sync::public::LsRemote;
use zygo_mail_sync::registry::read_cache;
use zygo_mail_sync::spawn::{trigger, ChildSpawner};
use zygo_mail_sync::state::SyncPaths;
use zygo_redact::DeadlinePolicy;
use zygo_workspace::mail::{self, names, store, wake};
use zygo_workspace::state::{Phase, Store, Workspace};

use crate::tool::{McpCtx, Tool, ToolDef, ToolError};

/// The last sentence of each mail tool description (M§8a item 2).
pub const NO_POLL: &str = "Do not poll; zygo wakes you when mail arrives.";

/// The `max` of `mail_read` when the call gives none.
const READ_DEFAULT: usize = 10;
/// The error of `mail_send` while the workspace stops (M§9, AU86).
const STOPPING: &str = "zygo: workspace is stopping";
/// The error of an addressed send when the config does not load (MS-19 D2; the text of the sync gate line).
const NO_CONFIG: &str = "zygo: mail sync cannot run: the config does not load";

/// The send time: `SystemTime::now` in `tools`, a fixed time in the tests.
#[doc(hidden)]
pub type Clock = Arc<dyn Fn() -> SystemTime + Send + Sync>;
/// The wake after a send (M§7.3) with the keys sent to: `wake::after_send` in `tools`, a recorder in the tests.
#[doc(hidden)]
pub type Waker = Arc<dyn Fn(&Paths, &Workspace, &[String], &MailConfig) + Send + Sync>;

/// The three mail tools, only for a workspace member with mail on (A1, AU58). The user config gives `[mail]`; a config
/// that cannot be loaded lists no tool.
pub fn tools(ctx: &McpCtx) -> Vec<Box<dyn Tool>> {
    if gate(ctx).is_none() {
        return Vec::new();
    }
    let paths = Paths::from_env();
    let Ok(merged) = zygo_config::load(&zygo_config::LoadContext { paths: paths.clone(), repo: None, team: None }) else {
        return Vec::new();
    };
    let wake: Waker = Arc::new(|p: &Paths, ws: &Workspace, keys: &[String], cfg: &MailConfig| {
        wake::after_send(p, ws, &tmux_program(), keys, cfg)
    });
    tools_with(ctx, paths, merged.config.mail, Arc::new(SystemTime::now), wake)
}

/// `tools` with the paths, the `[mail]` config, the clock and the waker given.
#[doc(hidden)]
pub fn tools_with(ctx: &McpCtx, paths: Paths, cfg: MailConfig, clock: Clock, wake: Waker) -> Vec<Box<dyn Tool>> {
    tools_with_sync(ctx, paths, cfg, clock, wake, Box::new(production_spawner()), DeadlinePolicy::default())
}

/// The spawner of `tools_with`: the sync child is `<zygo_program()> mail sync --ws-id <id> --background` (MS-19 D6).
fn production_spawner() -> ChildSpawner {
    ChildSpawner { zygo: zygo_program() }
}

/// `tools_with` with the spawn of the sync child and the deadline policy of the send-time secret pass given
/// (mail sync design S§8.1, S§8.6).
#[doc(hidden)]
pub fn tools_with_sync(
    ctx: &McpCtx,
    paths: Paths,
    cfg: MailConfig,
    clock: Clock,
    wake: Waker,
    spawner: Box<dyn Spawner>,
    secret_policy: DeadlinePolicy,
) -> Vec<Box<dyn Tool>> {
    let Some(member) = gate(ctx) else { return Vec::new() };
    let sync = Env {
        paths: paths.clone(),
        clock: Box::new(SystemClock),
        probe: Box::new(LsRemote),
        seam: Box::new(NoSeam),
        spawner,
        wake: Box::new(TmuxWake { tmux: tmux_program() }),
        secret_policy,
    };
    let m = Arc::new(Mail { paths, ws_id: member.ws_id, pane: member.pane, cfg, clock, wake, sync });
    vec![Box::new(MailSend(Arc::clone(&m))), Box::new(MailRead(Arc::clone(&m))), Box::new(MailList(m))]
}

/// The member marker when `[features] mail` is on, the session view reads, has `mail` on and has a marker (A1).
fn gate(ctx: &McpCtx) -> Option<Member> {
    if !ctx.features.mail {
        return None;
    }
    let view = SessionView::load(ctx.session_dir.as_ref()?).ok()?;
    if !view.mail {
        return None;
    }
    view.workspace.map(|w| w.member)
}

/// The first `<entry>/tmux` over the absolute entries of `PATH` that is an executable file, else `/dev/null/tmux`
/// (rule D2 of `crates/zygo/src/cmd/mod.rs`).
fn tmux_program() -> PathBuf {
    let found = std::env::var_os("PATH").and_then(|v| {
        std::env::split_paths(&v).filter(|p| p.is_absolute()).map(|p| p.join("tmux")).find(|p| executable(p))
    });
    found.unwrap_or_else(|| PathBuf::from("/dev/null/tmux"))
}

/// The zygo program that starts the sync child: the current executable (`zygo mcp`), canonical when it can be,
/// else an empty path, whose start then fails with one `errors.log` line (MS-19 D6).
fn zygo_program() -> PathBuf {
    match std::env::current_exe() {
        Ok(p) => std::fs::canonicalize(&p).unwrap_or(p),
        Err(_) => PathBuf::new(),
    }
}

fn executable(p: &std::path::Path) -> bool {
    let Ok(meta) = std::fs::metadata(p) else { return false };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.is_file() && meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    meta.is_file()
}

struct Mail {
    paths: Paths,
    ws_id: String,
    pane: String,
    cfg: MailConfig,
    clock: Clock,
    wake: Waker,
    /// The seams of the sync child start after a remote send (S§8.1 row 1).
    sync: Env,
}

/// Where `mail_send` sends: a local `to` for the M§5 send, or the result of a remote send (S§7.2).
enum Route {
    Local(String),
    Sent(Value),
}

impl Mail {
    /// `workspace.json` of this member's workspace (A3).
    fn workspace(&self) -> Result<Workspace, ToolError> {
        match Store::open(&self.paths, &self.ws_id).read() {
            Ok(Some(ws)) => Ok(ws),
            Ok(None) => Err(ToolError::Failed(format!("zygo: workspace {} has no state", self.ws_id))),
            Err(e) => Err(ToolError::Failed(format!("zygo: workspace {}: {e}", self.ws_id))),
        }
    }

    fn dir(&self) -> PathBuf {
        mail::mail_dir(&self.paths, &self.ws_id)
    }

    /// The merged config, read at each call; `None` when it does not load (MS-19 D2).
    fn config(&self) -> Option<zygo_config::Config> {
        zygo_config::load(&LoadContext { paths: self.paths.clone(), repo: None, team: None }).ok().map(|m| m.config)
    }

    /// S§10.4: the addresses of the member cache, each once in host order; none when the config does not load or
    /// sync is off for this workspace.
    fn remote(&self, ws: &Workspace) -> Vec<String> {
        let Some(cfg) = self.config() else { return Vec::new() };
        if sync_for(&cfg, &ws.name).is_err() {
            return Vec::new();
        }
        let Ok(sp) = SyncPaths::new(&self.paths, &self.ws_id) else { return Vec::new() };
        let mut out: Vec<String> = Vec::new();
        for a in read_cache(&sp).addresses() {
            if !out.contains(&a) {
                out.push(a);
            }
        }
        out
    }

    /// S§7.2: a `to` without `@` is local (M§8). A `to` with `@` resolves against the host in force with sync on;
    /// a remote address goes through the S§7.2 rules and the S§8.6 send-time pass into the outbox, then the sync
    /// child starts with no gap (S§8.1 row 1).
    fn route(&self, ws: &Workspace, to: &str, subject: &str, body: &str) -> Result<Route, ToolError> {
        if !to.contains('@') {
            return Ok(Route::Local(to.into()));
        }
        let Some(cfg) = self.config() else { return Err(ToolError::Failed(NO_CONFIG.into())) };
        let settings = sync_for(&cfg, &ws.name);
        let own: Vec<String> = settings.iter().map(|s| s.host.clone()).collect();
        let a = match resolve_to(to, &own) {
            Err(e) => return Err(ToolError::Failed(e.line().into())),
            Ok(To::Local(n)) => return Ok(Route::Local(n)),
            Ok(To::All) => return Ok(Route::Local("all".into())),
            Ok(To::Remote(a)) => a,
        };
        let sp = SyncPaths::new(&self.paths, &self.ws_id).map_err(|e| failed(mail::MailError::Io(e)))?;
        let from = ws.members.get(&self.pane).and_then(|r| r.valid_name());
        let id = send_remote_with(
            &sp,
            settings.as_ref(),
            from,
            &a,
            subject,
            body,
            &cfg.redaction,
            &ws.main_root,
            (self.clock)(),
            &self.sync.secret_policy,
        )
        .map_err(|e| ToolError::Failed(e.line()))?;
        let store = Store::open(&self.paths, &self.ws_id);
        trigger(&self.sync, &self.ws_id, None, &|t| store.log_error(&format!("mail_send: {t}")));
        Ok(Route::Sent(json!({"sent": [{"id": id.to_string(), "to": format!("{}@{}", a.name, a.host)}], "refused": []})))
    }
}

fn failed(e: mail::MailError) -> ToolError {
    ToolError::Failed(e.to_string())
}

/// The argument object; a key outside `keys` is `InvalidParams` naming it (A6).
fn object<'a>(args: &'a Value, keys: &[&str]) -> Result<&'a Map<String, Value>, ToolError> {
    let Some(obj) = args.as_object() else {
        return Err(ToolError::InvalidParams("arguments must be an object".into()));
    };
    if let Some(k) = obj.keys().find(|k| !keys.contains(&k.as_str())) {
        return Err(ToolError::InvalidParams(format!("unknown key `{k}`")));
    }
    Ok(obj)
}

fn string<'a>(obj: &'a Map<String, Value>, key: &str) -> Result<&'a str, ToolError> {
    match obj.get(key) {
        Some(Value::String(s)) => Ok(s),
        Some(_) => Err(ToolError::InvalidParams(format!("{key} must be a string"))),
        None => Err(ToolError::InvalidParams(format!("{key} is required"))),
    }
}

fn def(name: &str, text: &str, input_schema: Value) -> ToolDef {
    ToolDef { name: name.into(), description: format!("{text}{NO_POLL}"), input_schema }
}

struct MailSend(Arc<Mail>);
struct MailRead(Arc<Mail>);
struct MailList(Arc<Mail>);

impl Tool for MailSend {
    fn def(&self) -> ToolDef {
        def(
            "mail_send",
            "Send mail to members of this workspace. to is a member name, a pane key, or all (every other member). To a member on another machine, use `<name>@<host>`. Returns the message ids. ",
            json!({"type":"object","properties":{"to":{"type":"string"},"subject":{"type":"string"},"body":{"type":"string"}},"required":["to","subject","body"],"additionalProperties":false}),
        )
    }

    fn call(&self, args: Value) -> Result<Value, ToolError> {
        let obj = object(&args, &["to", "subject", "body"])?;
        let (to, subject, body) = (string(obj, "to")?, string(obj, "subject")?, string(obj, "body")?);
        let m = &self.0;
        let ws = m.workspace()?;
        // AU86 with AU83: stopping blocks only while `down` holds `op.lock`.
        if ws.phase == Phase::Stopping && !Store::open(&m.paths, &m.ws_id).op_lock_free() {
            return Err(ToolError::Failed(STOPPING.into()));
        }
        let to = match m.route(&ws, to, subject, body)? {
            Route::Sent(v) => return Ok(v),
            Route::Local(n) => n,
        };
        let report = mail::send(&m.paths, &ws, Some(&m.pane), &to, subject, body, &m.cfg, (m.clock)()).map_err(failed)?;
        let refused: Vec<String> = report.refused.iter().map(|r| r.error.to_string()).collect();
        if report.sent.is_empty() && !refused.is_empty() {
            return Err(ToolError::Failed(refused.join("\n")));
        }
        let targets = names::members(&ws);
        let label = |key: &str| {
            targets.iter().find(|t| t.key == key).and_then(|t| t.name.clone()).unwrap_or_else(|| key.to_string())
        };
        let sent: Vec<Value> = report.sent.iter().map(|s| json!({"id": s.id, "to": label(&s.key)})).collect();
        let keys: Vec<String> = report.sent.into_iter().map(|s| s.key).collect();
        if !keys.is_empty() {
            (m.wake)(&m.paths, &ws, &keys, &m.cfg);
        }
        Ok(json!({"sent": sent, "refused": refused}))
    }
}

impl Tool for MailRead {
    fn def(&self) -> ToolDef {
        def(
            "mail_read",
            "Read your new mail, oldest first: at most max messages (default 10). The messages returned are marked read. ",
            json!({"type":"object","properties":{"max":{"type":"integer","minimum":1}},"additionalProperties":false}),
        )
    }

    fn call(&self, args: Value) -> Result<Value, ToolError> {
        let obj = object(&args, &["max"])?;
        let max = match obj.get("max") {
            None => READ_DEFAULT,
            Some(v) => match v.as_u64() {
                Some(n) if n >= 1 => usize::try_from(n).unwrap_or(usize::MAX),
                _ => return Err(ToolError::InvalidParams("max must be an integer of at least 1".into())),
            },
        };
        let m = &self.0;
        m.workspace()?;
        let msgs = store::read(&m.dir(), &m.pane, max).map_err(failed)?;
        Ok(json!({"messages": msgs}))
    }
}

impl Tool for MailList {
    fn def(&self) -> ToolDef {
        def(
            "mail_list",
            "Show the count and the subjects of your unread mail, and the current members of this workspace, without marking anything read. ",
            json!({"type":"object","additionalProperties":false}),
        )
    }

    fn call(&self, args: Value) -> Result<Value, ToolError> {
        object(&args, &[])?;
        let m = &self.0;
        let ws = m.workspace()?;
        let dir = m.dir();
        let unread = store::unread(&dir, &m.pane).map_err(failed)?;
        let messages: Vec<Value> = store::list(&dir, &m.pane)
            .map_err(failed)?
            .into_iter()
            .map(|s| json!({"id": s.id, "from": s.from, "subject": s.subject}))
            .collect();
        let members: Vec<Value> = names::members(&ws).into_iter().map(|t| json!({"key": t.key, "name": t.name})).collect();
        Ok(json!({"unread": unread, "messages": messages, "members": members, "remote": m.remote(&ws)}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// MS-19 review F2: in the test process `zygo_program` is the canonical current executable.
    #[test]
    fn zygo_program_is_the_canonical_current_exe() {
        let want = std::env::current_exe().expect("current_exe").canonicalize().expect("canonicalize");
        assert_eq!(zygo_program(), want);
    }

    /// MS-19 review F2: the spawner of `tools_with` starts `<zygo_program()> mail sync --ws-id <id> --background`.
    #[test]
    fn production_spawner_starts_mail_sync_of_the_workspace() {
        let sp = production_spawner();
        assert_eq!(sp.zygo, zygo_program());
        let cmd = sp.command("ws-1");
        assert_eq!(cmd.get_program(), zygo_program().as_os_str());
        let args: Vec<&std::ffi::OsStr> = cmd.get_args().collect();
        assert_eq!(args, ["mail", "sync", "--ws-id", "ws-1", "--background"]);
    }
}
