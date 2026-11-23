use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use anyhow::anyhow;
use zygo_config::keys::default_store;
use zygo_config::{load, Level, LoadContext, Merged};
use zygo_core::overrides::Overrides;
use zygo_core::paths::Paths;
use zygo_core::session_dir::SessionDir;
use zygo_core::verbs::{VerbSpec, VERB_SPECS};
use zygo_hooks::statusline::render::Style;
use zygo_hooks::workspace::{read_session_file, record_of, start_snapshot_child, verb_member};
use zygo_workspace::mux::tmux::Tmux;
use zygo_workspace::snapshot;
use zygo_workspace::state::{CompactOverride, Store};
use zygo_plugins::check::esc;
use zygo_plugins::select::set_user_key;
use zygo_proxy::client::{ClientError, SessionClient};
use zygo_launch::serve_client::{from_config, read_team, Unreachable};
use zygo_proxy::session::SessionSettings;

use super::compact::client_timeout;
use super::UsageError;

pub struct VerbCtx {
    pub slash: bool,
    pub save: bool,
    pub session_dir: Option<SessionDir>,
    pub paths: Paths,
}

pub type VerbFn = fn(&VerbCtx, &[String]) -> anyhow::Result<String>;

pub struct Verb {
    pub spec: &'static VerbSpec,
    pub handler: VerbFn,
}

/// One entry per `VERB_SPECS` entry, same order.
pub static VERBS: &[Verb] = &[
    Verb { spec: &VERB_SPECS[0], handler: super::help::verb },
    Verb { spec: &VERB_SPECS[1], handler: super::status::verb },
    Verb { spec: &VERB_SPECS[2], handler: super::usage::verb },
    Verb { spec: &VERB_SPECS[3], handler: super::compact::verb },
    Verb { spec: &VERB_SPECS[4], handler: super::results::verb },
    Verb { spec: &VERB_SPECS[5], handler: super::redaction::verb },
    Verb { spec: &VERB_SPECS[6], handler: super::guards::verb },
    Verb { spec: &VERB_SPECS[7], handler: super::units::verb },
    Verb { spec: &VERB_SPECS[8], handler: super::work::verb },
    Verb { spec: &VERB_SPECS[9], handler: super::plugins::verb },
    Verb { spec: &VERB_SPECS[10], handler: super::marketplace::verb },
    Verb { spec: &VERB_SPECS[11], handler: super::profile::verb },
    Verb { spec: &VERB_SPECS[12], handler: super::compact_tool::verb },
    Verb { spec: &VERB_SPECS[13], handler: super::statusline::verb },
    Verb { spec: &VERB_SPECS[14], handler: super::down::verb },
    Verb { spec: &VERB_SPECS[15], handler: super::ls::verb },
    Verb { spec: &VERB_SPECS[16], handler: super::workspace::verb },
];

/// One session setting (§17.1, §6.1 layer 7).
#[derive(Debug, Clone, PartialEq)]
pub enum Setting {
    Results(bool),
    Threshold(u64),
    Units(bool),
    RedactionContent(bool),
    Guard { guard: String, level: String },
    /// The status line style, `compact` or `verbose` (statusline HUD design H§8, AU1).
    Statusline(Style),
}

/// The reply reads `<verb> <words> …`.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionChange {
    pub verb: &'static str,
    pub words: String,
    pub setting: Setting,
}

impl Setting {
    fn apply(&self, o: &mut Overrides) {
        match self {
            Setting::Results(b) => o.results = Some(*b),
            Setting::Threshold(n) => o.results_threshold = Some(*n),
            Setting::Units(b) => o.work_units = Some(*b),
            Setting::RedactionContent(b) => o.redaction_content = Some(*b),
            Setting::Guard { guard, level } => {
                o.guards.insert(guard.clone(), level.clone());
            }
            Setting::Statusline(s) => o.statusline = Some(super::statusline::name(*s).to_string()),
        }
    }

    fn save(&self, config_file: &Path) -> anyhow::Result<()> {
        match self {
            Setting::Results(b) => set_user_key(config_file, &["features", "results"], *b),
            Setting::Threshold(n) => {
                set_user_key(config_file, &["results", "trim_threshold_bytes"], i64::try_from(*n).unwrap_or(i64::MAX))
            }
            Setting::Units(b) => set_user_key(config_file, &["features", "work_units"], *b),
            Setting::RedactionContent(b) => set_user_key(config_file, &["features", "redaction", "content"], *b),
            Setting::Guard { guard, level } => set_user_key(config_file, &["guards", guard], level.as_str()),
            Setting::Statusline(s) => set_user_key(config_file, &["statusline", "style"], super::statusline::name(*s)),
        }
    }
}

/// The team layer of a verb (§6.1 layer 2): in a session, its `team.toml`; outside, fetched from the configured server
/// with the team token; `None` without a server. An unreachable server is exit 12.
pub(crate) fn team(ctx: &VerbCtx, verb: &str) -> anyhow::Result<Option<toml::Table>> {
    if let Some(sd) = &ctx.session_dir {
        return read_team(&sd.root).map_err(|e| anyhow!("zygo {verb}: cannot read team.toml: {}", esc(&e.to_string())));
    }
    let m = base(ctx, verb, None, None)?;
    let env = |n: &str| std::env::var_os(n);
    let Some(c) = from_config(None, &m.config, &default_store(&ctx.paths), &env).map_err(|r| anyhow!("zygo {verb}: {r}"))? else {
        return Ok(None);
    };
    match c.team_table() {
        Ok(t) => Ok(Some(t)),
        Err(ClientError::Unreachable(d)) => Err(Unreachable(format!("zygo {verb}: server unreachable: {}", esc(&d))).into()),
        Err(e) => Err(anyhow!("zygo {verb}: team config refused: {}", esc(&e.to_string()))),
    }
}

/// The config without session overrides; `team` from [`team`].
pub(crate) fn base(ctx: &VerbCtx, verb: &str, team: Option<toml::Table>, repo: Option<&Path>) -> anyhow::Result<Merged> {
    load(&LoadContext { paths: ctx.paths.clone(), repo: repo.map(Path::to_path_buf), team })
        .map_err(|e| anyhow!("zygo {verb}: {e}"))
}

/// The read forms' config (F1): each stored session setting applied on its own, as the hook policy does
/// (`zygo-work/src/guards.rs` `policy`). A guard below `min_level` is lifted to it; a refused setting is skipped and
/// never hides the others. Also returns the guards lifted to the floor.
pub(crate) fn with_session(b: Merged, dir: Option<&SessionDir>) -> (Merged, BTreeSet<String>) {
    let mut lifted = BTreeSet::new();
    let Some(dir) = dir else { return (b, lifted) };
    let o = Overrides::load(dir);
    let mut cur = b;
    let toggles = [
        Overrides { results: o.results, ..Default::default() },
        Overrides { results_threshold: o.results_threshold, ..Default::default() },
        Overrides { redaction_content: o.redaction_content, ..Default::default() },
        Overrides { work_units: o.work_units, ..Default::default() },
    ];
    for one in toggles {
        if let Ok(next) = cur.with_overrides(&one) {
            cur = next;
        }
    }
    for (g, l) in o.guards {
        let Ok(l) = l.parse::<Level>() else { continue };
        let min = cur.config.guards.min_level;
        let one = Overrides { guards: BTreeMap::from([(g.clone(), l.max(min).name().to_string())]), ..Default::default() };
        if let Ok(next) = cur.with_overrides(&one) {
            cur = next;
            if l < min {
                lifted.insert(g);
            }
        }
    }
    (cur, lifted)
}

/// `old` back in place of `overrides.json` (temp file plus rename), or no file when there was none.
fn restore(dir: &SessionDir, old: Option<Vec<u8>>) -> std::io::Result<()> {
    let target = dir.overrides_json();
    let Some(old) = old else {
        return match std::fs::remove_file(&target) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    };
    let tmp = dir.root.join(format!(".overrides.json.{}.tmp", zygo_core::ids::ulid()));
    let res = std::fs::write(&tmp, old).and_then(|()| std::fs::rename(&tmp, &target));
    if res.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    res
}

/// The value that `mirror` writes into the member record (AU105).
pub(crate) enum Mirror {
    /// `member.overrides`: the whole `overrides.json`, read under the state lock.
    Overrides,
    /// `member.compact`.
    Compact(CompactOverride),
}

/// The text added at the end of a reply or error text when the mirror did not write (AU105).
pub(crate) const NOT_SAVED: &str = "; not saved for the workspace";

/// The time the slash form keeps for its reply and the process exit after the mirror (inside the 4.5 s bound).
pub(crate) const SLASH_MARGIN: Duration = Duration::from_millis(500);

/// The lock wait of the mirror: `HOOK_WAIT` for the CLI form, and for the slash form also at most the time left less
/// `SLASH_MARGIN`; `None` when no time is left (the mirror is skipped).
pub(crate) fn lock_bound(slash: bool, left: Option<Duration>) -> Option<Duration> {
    match (slash, left) {
        (false, _) | (true, None) => Some(snapshot::HOOK_WAIT),
        (true, Some(left)) => {
            let b = left.saturating_sub(SLASH_MARGIN).min(snapshot::HOOK_WAIT);
            (!b.is_zero()).then_some(b)
        }
    }
}

/// The override mirror (OP12, AU105): writes `value` into the member record of the session folder `dir` when that
/// session is a workspace member. The marker is read by the hook rule (`verb_member`; the slash form first needs
/// `ZYGO_WORKSPACE`); a session that is not a member gets `Ok(())` with nothing read or written (W-C6, W§18 #89). The
/// write waits for the state lock at most `lock_bound`, changes only a state that exists (`update_existing`) and only
/// a record of this zygo session (`record_of`). After a write, and only then, the `snapshot.pending` check of W§7.3
/// runs: inline for the CLI form; for the slash form, which runs inside `zygo hook prompt`, as the detached `zygo ws
/// snapshot` child, as the hooks do. An `Err` means the record was not written.
pub(crate) fn mirror(ctx: &VerbCtx, dir: &SessionDir, value: Mirror) -> anyhow::Result<()> {
    let env = |k: &str| std::env::var_os(k);
    let gate = ctx.slash.then_some(&env as &dyn Fn(&str) -> Option<OsString>);
    let Some(m) = verb_member(dir, &ctx.paths, gate) else { return Ok(()) };
    let Some(bound) = lock_bound(ctx.slash, zygo_hooks::prompt::verb_time_left()) else {
        return Err(anyhow!("no time left in the hook bound"));
    };
    let store = Store::open(&ctx.paths, &m.ws_id);
    let written = store.update_existing(bound, &mut |ws| {
        let Some(rec) = ws.members.get_mut(&m.pane) else {
            return Err(anyhow!("no member record for pane {}", m.pane));
        };
        if !record_of(rec, &m.session) {
            return Err(anyhow!("member record of pane {} belongs to another session", m.pane));
        }
        match &value {
            Mirror::Overrides => {
                // Under the state lock: no link is followed and no FIFO is waited on (W§7.3, W-C7; FIX-22 rule).
                let path = dir.overrides_json();
                let Some(bytes) = read_session_file(&path)? else {
                    return Err(anyhow!("{}: missing", path.display()));
                };
                rec.overrides = Some(serde_json::from_slice(&bytes)?);
            }
            Mirror::Compact(c) => rec.compact = Some(c.clone()),
        }
        Ok(())
    });
    let written = match written {
        Ok(Some(ws)) => ws,
        Ok(None) => return Err(anyhow!("no workspace with id {}", m.ws_id)),
        Err(e) => return Err(anyhow!("{e}")),
    };
    if ctx.slash {
        if let Ok(zygo) = std::env::current_exe()
            && let Some(force) = snapshot::take_pending(&ctx.paths, &m.ws_id)
        {
            start_snapshot_child(&ctx.paths, &m.ws_id, &zygo, force, &|t| store.log_error(&format!("verb mirror: {t}")));
        }
    } else {
        let mux = Tmux::new(super::tmux_program(), written.tmux_socket.clone());
        let _ = snapshot::after_write(&ctx.paths, &mux, &m.ws_id, snapshot::HOOK_WAIT);
    }
    Ok(())
}

/// D1: the one change path of the session setting verbs; `m` is the config without session overrides.
/// Order: O2, the limits of the new setting, `overrides.json` (and the PATCH for `redaction content`), the mirror of
/// `overrides.json` into the member record once the outcome is final (AU105), user config.
pub fn set_session(ctx: &VerbCtx, m: &Merged, c: SessionChange) -> anyhow::Result<String> {
    let SessionChange { verb, words, setting } = c;
    if ctx.session_dir.is_none() && !ctx.save {
        return Err(UsageError(format!("zygo {verb}: not in a zygo session; add --save to change user config")).into());
    }
    let mut only = Overrides::default();
    setting.apply(&mut only);
    m.with_overrides(&only)?;
    let Some(dir) = &ctx.session_dir else {
        setting.save(&ctx.paths.config_file).map_err(|e| anyhow!("zygo {verb}: saving to user config failed: {e}"))?;
        return Ok(format!("{verb} {words} saved to user config"));
    };
    // F6: a file that exists but cannot be read stops the verb before any change. It is read by the session-file rule
    // (`read_session_file`): a link, a FIFO or another entry that is not a regular file is such a file, and no read
    // waits on it (M4 review round 2). A missing file is `None`.
    let old = match read_session_file(&dir.overrides_json()) {
        Ok(b) => b,
        Err(e) => return Err(anyhow!("zygo {verb}: cannot read overrides.json: {e:#}")),
    };
    // The client is built before any write: a session dir it cannot use changes nothing.
    let client = match setting {
        Setting::RedactionContent(_) => Some(
            SessionClient::from_session_dir(&dir.root)
                .map_err(|e| anyhow!("redaction failed: {e}"))?
                .timeout(client_timeout(ctx.slash)),
        ),
        _ => None,
    };
    let mut next = Overrides::load(dir);
    setting.apply(&mut next);
    next.save(dir)?;
    let mut lost: Option<ClientError> = None;
    if let (Some(client), Setting::RedactionContent(b)) = (client, &setting) {
        match client.set_settings(&SessionSettings { redaction_content: Some(*b) }) {
            Ok(_) => {}
            // F5: only a clear refusal (an HTTP error status) is rolled back.
            Err(e @ ClientError::Reply { status, .. }) if !(200..300).contains(&status) => {
                return Err(match restore(dir, old) {
                    Ok(()) => anyhow!("redaction failed: {e}"),
                    Err(r) => anyhow!("redaction failed: {e}; restoring overrides.json failed: {r}"),
                });
            }
            // A lost PATCH keeps the new `overrides.json`, so it is mirrored (AU105).
            Err(e) => lost = Some(e),
        }
    }
    let note = if mirror(ctx, dir, Mirror::Overrides).is_err() { NOT_SAVED } else { "" };
    if let Some(e) = lost {
        return Err(anyhow!("redaction failed: {e}; the session may already use the new setting{note}"));
    }
    let done = format!("{verb} {words} for this session");
    if !ctx.save {
        return Ok(format!("{done}{note}"));
    }
    setting.save(&ctx.paths.config_file).map_err(|e| anyhow!("{done}; saving to user config failed: {e}{note}"))?;
    Ok(format!("{done}; saved to user config{note}"))
}

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::BTreeMap;

    use tempfile::TempDir;
    use zygo_config::{Level, Refusal};

    use super::*;
    use crate::cmd::compact::tests::{ctx, dead_session, paths, proxy};
    use crate::cmd::UsageError;

    pub(crate) fn v(s: &[&str]) -> Vec<String> {
        s.iter().map(|t| t.to_string()).collect()
    }

    pub(crate) fn usage_text(r: anyhow::Result<String>) -> String {
        let e = r.unwrap_err();
        assert!(e.downcast_ref::<UsageError>().is_some(), "{e}");
        e.to_string()
    }

    /// The text of an error that is not a `UsageError`.
    pub(crate) fn err_text(r: anyhow::Result<String>) -> String {
        let e = r.unwrap_err();
        assert!(e.downcast_ref::<UsageError>().is_none(), "{e}");
        e.to_string()
    }

    pub(crate) fn sd(d: &Path) -> std::path::PathBuf {
        let dir = d.join("s");
        SessionDir::new(dir.clone()).create().unwrap();
        dir
    }

    pub(crate) fn ov(dir: &Path) -> Overrides {
        Overrides::load(&SessionDir::new(dir.to_path_buf()))
    }

    pub(crate) fn cfg(d: &Path) -> toml::Table {
        cfg_at(&d.join("config.toml"))
    }

    fn change(verb: &'static str, words: &str, setting: Setting) -> SessionChange {
        SessionChange { verb, words: words.into(), setting }
    }

    fn units_off() -> SessionChange {
        change("units", "off", Setting::Units(false))
    }

    fn redaction_off() -> SessionChange {
        change("redaction", "content off", Setting::RedactionContent(false))
    }

    fn m0(c: &VerbCtx) -> Merged {
        base(c, "x", None, None).unwrap()
    }

    /// M4, decision 2026-11-23: the slash form's lock wait fits inside the hook bound; no time left skips the mirror.
    #[test]
    fn lock_bound_fits_the_hook_bound() {
        use std::time::Duration;
        let ms = Duration::from_millis;
        assert_eq!(lock_bound(false, None), Some(ms(1000)));
        assert_eq!(lock_bound(false, Some(ms(0))), Some(ms(1000)));
        assert_eq!(lock_bound(true, None), Some(ms(1000)));
        assert_eq!(lock_bound(true, Some(ms(4000))), Some(ms(1000)));
        assert_eq!(lock_bound(true, Some(ms(1200))), Some(ms(700)));
        assert_eq!(lock_bound(true, Some(ms(500))), None);
        assert_eq!(lock_bound(true, Some(ms(0))), None);
    }

    /// M4 review F2a: the slash mirror takes its lock wait from the deadline of the prompt hook (`verb_time_left`),
    /// not the 1 s `HOOK_WAIT`. The slash gate reads `ZYGO_WORKSPACE` from the process environment, so the case runs
    /// in a child process that has it; this process changes no variable.
    #[cfg(unix)]
    #[test]
    fn slash_mirror_waits_only_within_the_hook_deadline() {
        let o = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "cmd::verbs::tests::slash_mirror_with_the_lock_held", "--ignored", "--nocapture", "--test-threads=1"])
            .env("ZYGO_WORKSPACE", "0123456789abcdef")
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();
        let text = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
        assert!(o.status.success(), "{text}");
        assert!(text.contains("test result: ok. 1 passed"), "{text}");
    }

    /// The child of `slash_mirror_waits_only_within_the_hook_deadline`: a member whose `lock` the test holds runs a
    /// slash setting verb with 700 ms left in the hook bound. The lock wait is 700 ms less `SLASH_MARGIN`, so the verb
    /// replies within 900 ms; with the `HOOK_WAIT` bound (1 s) it could not.
    #[cfg(unix)]
    #[test]
    #[ignore = "child process of slash_mirror_waits_only_within_the_hook_deadline"]
    fn slash_mirror_with_the_lock_held() {
        use std::time::{Duration, Instant};
        use zygo_core::session_view::{Member, SessionView, ViewWorkspace};
        use zygo_workspace::state::{MemberRecord, MemberState, Phase};
        const WS: &str = "0123456789abcdef";
        const PANE: &str = "kaaaaaaaa";
        const SID: &str = "01JTESTSESSIONAAAAAAAAAAAA";
        assert_eq!(std::env::var("ZYGO_WORKSPACE").as_deref(), Ok(WS), "run by slash_mirror_waits_only_within_the_hook_deadline");
        let d = TempDir::new().unwrap();
        let dir = sd(d.path());
        let member = Member { ws_id: WS.into(), pane: PANE.into(), ..Default::default() };
        SessionView { session: SID.into(), workspace: Some(ViewWorkspace { member, ..Default::default() }), ..Default::default() }
            .save(&SessionDir::new(dir.clone()))
            .unwrap();
        let mut c = ctx(d.path(), Some(&dir));
        c.slash = true;
        let store = Store::open(&c.paths, WS);
        let rec = MemberRecord { zygo_session: Some(SID.into()), state: MemberState::Running, ..MemberRecord::new(zygo_core::Agent::ClaudeCode) };
        store
            .update(Duration::from_secs(5), &mut |w| {
                w.name = "app".into();
                w.tmux_session = "zygo-app".into();
                w.common_dir = "/srv/app/.git".into();
                w.main_root = "/srv/app".into();
                w.loadout = "solo".into();
                w.phase = Phase::Up;
                w.members = BTreeMap::from([(PANE.to_string(), rec.clone())]);
                Ok(())
            })
            .unwrap();
        let lock = std::fs::OpenOptions::new().read(true).write(true).open(store.dir().join("lock")).unwrap();
        fs4::FileExt::try_lock(&lock).unwrap();
        let t0 = Instant::now();
        let reply = zygo_hooks::prompt::with_verb_deadline(t0 + Duration::from_millis(700), || set_session(&c, &m0(&c), units_off()));
        let took = t0.elapsed();
        drop(lock);
        assert!(took < Duration::from_millis(900), "{took:?}");
        assert_eq!(reply.unwrap(), "units off for this session; not saved for the workspace");
        assert_eq!(store.read().unwrap().unwrap().members[PANE], rec);
    }

    #[test]
    fn session_change_writes_overrides_only() {
        let d = TempDir::new().unwrap();
        let dir = sd(d.path());
        let c = ctx(d.path(), Some(&dir));
        assert_eq!(set_session(&c, &m0(&c), units_off()).unwrap(), "units off for this session");
        assert_eq!(ov(&dir), Overrides { work_units: Some(false), ..Default::default() });
        assert!(!d.path().join("config.toml").exists());
    }

    #[test]
    fn keeps_earlier_overrides() {
        let d = TempDir::new().unwrap();
        let dir = sd(d.path());
        std::fs::write(dir.join("overrides.json"), r#"{"work_units":false}"#).unwrap();
        let c = ctx(d.path(), Some(&dir));
        set_session(&c, &m0(&c), change("results", "off", Setting::Results(false))).unwrap();
        assert_eq!(ov(&dir).results, Some(false));
        assert_eq!(ov(&dir).work_units, Some(false));
    }

    #[test]
    fn save_also_writes_user_config() {
        let d = TempDir::new().unwrap();
        let dir = sd(d.path());
        let c = VerbCtx { save: true, ..ctx(d.path(), Some(&dir)) };
        assert_eq!(
            set_session(&c, &m0(&c), change("results", "threshold 32768", Setting::Threshold(32768))).unwrap(),
            "results threshold 32768 for this session; saved to user config"
        );
        assert_eq!(ov(&dir).results_threshold, Some(32768));
        assert_eq!(cfg(d.path())["results"]["trim_threshold_bytes"].as_integer(), Some(32768));

        let out = TempDir::new().unwrap();
        let c = VerbCtx { save: true, ..ctx(out.path(), None) };
        let g = Setting::Guard { guard: "claims".into(), level: "enforce".into() };
        assert_eq!(set_session(&c, &m0(&c), change("guards", "claims enforce", g)).unwrap(), "guards claims enforce saved to user config");
        assert_eq!(cfg(out.path())["guards"]["claims"].as_str(), Some("enforce"));
        assert!(!paths(out.path()).sessions_dir.exists());
    }

    #[test]
    fn outside_a_session_needs_save() {
        let d = TempDir::new().unwrap();
        let c = ctx(d.path(), None);
        let e = set_session(&c, &m0(&c), units_off()).unwrap_err();
        assert!(e.downcast_ref::<UsageError>().is_some(), "{e}");
        assert_eq!(e.to_string(), "zygo units: not in a zygo session; add --save to change user config");
        assert!(!d.path().join("config.toml").exists());
    }

    #[test]
    fn refusal_writes_nothing() {
        let d = TempDir::new().unwrap();
        let floor = "[guards]\nmin_level = \"notify\"\n";
        std::fs::write(d.path().join("config.toml"), floor).unwrap();
        let dir = sd(d.path());
        let c = VerbCtx { save: true, ..ctx(d.path(), Some(&dir)) };
        let g = Setting::Guard { guard: "claims".into(), level: "monitor".into() };
        let e = set_session(&c, &m0(&c), change("guards", "claims monitor", g)).unwrap_err();
        assert_eq!(e.downcast_ref::<Refusal>(), Some(&Refusal::BelowFloor { guard: "claims".into(), min: Level::Notify }));
        assert_eq!(e.to_string(), "refused: guard claims cannot go below min_level notify");
        assert!(!dir.join("overrides.json").exists());
        assert_eq!(std::fs::read_to_string(d.path().join("config.toml")).unwrap(), floor);

        let team: toml::Table = "[features]\nwork_units = true\n".parse().unwrap();
        let m = base(&c, "x", Some(team), None).unwrap();
        let e = set_session(&c, &m, units_off()).unwrap_err();
        assert_eq!(e.to_string(), "refused: `features.work_units` is set by the team config");
        assert!(!dir.join("overrides.json").exists());
    }

    /// A session dir `id` whose proxy reads each request, then replies `409` with `body` or, for `None`, drops the
    /// connection.
    fn fake_session(d: &Path, id: &str, body: Option<&'static str>) -> std::path::PathBuf {
        use std::io::{BufRead, Read, Write};
        let dir = dead_session(d, id);
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        std::fs::write(SessionDir::new(dir.clone()).url(), format!("http://{}/", l.local_addr().unwrap())).unwrap();
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
                if let Some(b) = body {
                    let reply = format!(
                        "HTTP/1.1 409 Conflict\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{b}",
                        b.len()
                    );
                    let _ = r.get_mut().write_all(reply.as_bytes());
                }
            }
        });
        dir
    }

    fn no_tmp_files(dir: &Path) {
        for f in std::fs::read_dir(dir).unwrap() {
            let name = f.unwrap().file_name().to_string_lossy().into_owned();
            assert!(!name.ends_with(".tmp"), "{name}");
        }
    }

    const REFUSED: &str = r#"{"error":{"type":"zygo_refused","message":"no"}}"#;

    #[test]
    fn redaction_content_patches_the_session() {
        let d = TempDir::new().unwrap();
        let (h, id, dir) = proxy(d.path());
        let c = ctx(d.path(), Some(&dir));
        assert_eq!(set_session(&c, &m0(&c), redaction_off()).unwrap(), "redaction content off for this session");
        assert_eq!(h.session(&id).unwrap().settings().redaction_content, Some(false));
        assert_eq!(ov(&dir).redaction_content, Some(false));
    }

    #[test]
    fn patch_refused_restores_the_old_overrides() {
        let d = TempDir::new().unwrap();
        let s = fake_session(d.path(), "S3", Some(REFUSED));
        let old = br#"{"work_units":false}"#;
        std::fs::write(s.join("overrides.json"), old).unwrap();
        let c = ctx(d.path(), Some(&s));
        assert_eq!(err_text(set_session(&c, &m0(&c), redaction_off())), "redaction failed: zygo_refused: no");
        assert_eq!(std::fs::read(s.join("overrides.json")).unwrap(), old);
        no_tmp_files(&s);

        let none = fake_session(d.path(), "S4", Some(REFUSED));
        let c = ctx(d.path(), Some(&none));
        assert_eq!(err_text(set_session(&c, &m0(&c), redaction_off())), "redaction failed: zygo_refused: no");
        assert!(!none.join("overrides.json").exists());
    }

    #[test]
    fn patch_lost_keeps_the_new_overrides() {
        let d = TempDir::new().unwrap();
        for (s, what) in [(fake_session(d.path(), "S5", None), "dropped"), (dead_session(d.path(), "S6"), "closed port")] {
            std::fs::write(s.join("overrides.json"), br#"{"work_units":false}"#).unwrap();
            let c = VerbCtx { save: true, ..ctx(d.path(), Some(&s)) };
            let e = err_text(set_session(&c, &m0(&c), redaction_off()));
            assert!(e.starts_with("redaction failed: zygo proxy unreachable: "), "{what}: {e}");
            assert!(e.ends_with("; the session may already use the new setting"), "{what}: {e}");
            assert_eq!(ov(&s), Overrides { work_units: Some(false), redaction_content: Some(false), ..Default::default() });
            assert!(!d.path().join("config.toml").exists(), "{what}");
            no_tmp_files(&s);
        }
    }

    #[test]
    fn no_session_client_changes_nothing() {
        let d = TempDir::new().unwrap();
        let s = dead_session(d.path(), "S7");
        std::fs::remove_file(SessionDir::new(s.clone()).url()).unwrap();
        let c = ctx(d.path(), Some(&s));
        assert_eq!(
            err_text(set_session(&c, &m0(&c), redaction_off())),
            "redaction failed: session directory: cannot read the proxy url file"
        );
        assert!(!s.join("overrides.json").exists());
    }

    #[test]
    fn unreadable_overrides_changes_nothing() {
        let d = TempDir::new().unwrap();
        let s = fake_session(d.path(), "S8", Some(REFUSED));
        std::fs::create_dir(s.join("overrides.json")).unwrap();
        let c = VerbCtx { save: true, ..ctx(d.path(), Some(&s)) };
        for (verb, change) in [("units", units_off()), ("redaction", redaction_off())] {
            let e = err_text(set_session(&c, &m0(&c), change));
            assert!(e.starts_with(&format!("zygo {verb}: cannot read overrides.json: ")), "{e}");
            assert!(s.join("overrides.json").is_dir(), "{verb}");
            assert!(!d.path().join("config.toml").exists(), "{verb}");
        }
    }

    #[test]
    fn user_config_failure_after_the_session_change() {
        let d = TempDir::new().unwrap();
        let dir = sd(d.path());
        let m = m0(&ctx(d.path(), Some(&dir)));
        let bad = d.path().join("bad.toml");
        std::fs::write(&bad, "features = 3\n").unwrap();
        let c = VerbCtx { save: true, paths: Paths { config_file: bad.clone(), ..paths(d.path()) }, ..ctx(d.path(), Some(&dir)) };
        let e = set_session(&c, &m, units_off()).unwrap_err();
        assert_eq!(e.to_string(), "units off for this session; saving to user config failed: user config: features is not a table");
        assert!(e.downcast_ref::<UsageError>().is_none());
        assert_eq!(ov(&dir).work_units, Some(false));
        assert_eq!(std::fs::read_to_string(&bad).unwrap(), "features = 3\n");
    }

    #[test]
    fn limits_check_only_the_new_setting() {
        let d = TempDir::new().unwrap();
        std::fs::write(d.path().join("config.toml"), "[guards]\nmin_level = \"notify\"\n").unwrap();
        let dir = sd(d.path());
        std::fs::write(dir.join("overrides.json"), r#"{"guards":{"claims":"monitor"}}"#).unwrap();
        let c = ctx(d.path(), Some(&dir));
        assert_eq!(set_session(&c, &m0(&c), units_off()).unwrap(), "units off for this session");
        assert_eq!(ov(&dir).guards, BTreeMap::from([("claims".to_string(), "monitor".to_string())]));
        assert_eq!(ov(&dir).work_units, Some(false));
    }

    fn cfg_at(f: &Path) -> toml::Table {
        std::fs::read_to_string(f).unwrap().parse().unwrap()
    }

    #[test]
    fn set_user_key_keeps_the_comment_of_a_replaced_key() {
        let d = TempDir::new().unwrap();
        let f = d.path().join("config.toml");
        std::fs::write(&f, "[features]\n# above\nresults = true # keep me\n").unwrap();
        set_user_key(&f, &["features", "results"], false).unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "[features]\n# above\nresults = false # keep me\n");
        std::fs::write(&f, "features = { results = true }\n").unwrap();
        set_user_key(&f, &["features", "results"], false).unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "features = { results = false }\n");
    }

    #[test]
    fn set_user_key_keeps_the_line_ends() {
        let d = TempDir::new().unwrap();
        let f = d.path().join("config.toml");
        std::fs::write(&f, "[user]\r\nx = 1 # c\r\n").unwrap();
        set_user_key(&f, &["guards", "claims"], "enforce").unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "[user]\r\nx = 1 # c\r\n[guards]\r\nclaims = \"enforce\"\r\n");
        std::fs::write(&f, "[user]\nx = 1 # c\n").unwrap();
        set_user_key(&f, &["guards", "claims"], "enforce").unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "[user]\nx = 1 # c\n[guards]\nclaims = \"enforce\"\n");
    }

    #[test]
    fn set_user_key_writes_nested_keys_and_keeps_other_bytes() {
        let d = TempDir::new().unwrap();
        let f = d.path().join("config.toml");
        let head = "# keep\n[user]\ndefault_profile = \"p\"\n";
        std::fs::write(&f, head).unwrap();
        set_user_key(&f, &["features", "redaction", "content"], false).unwrap();
        assert!(std::fs::read_to_string(&f).unwrap().starts_with(head));
        assert_eq!(cfg_at(&f)["features"]["redaction"]["content"].as_bool(), Some(false));
        set_user_key(&f, &["guards", "claims"], "enforce").unwrap();
        set_user_key(&f, &["guards", "claims"], "notify").unwrap();
        assert_eq!(cfg_at(&f)["guards"]["claims"].as_str(), Some("notify"));
        assert_eq!(std::fs::read_to_string(&f).unwrap().matches("claims").count(), 1);

        let missing = d.path().join("new/config.toml");
        set_user_key(&missing, &["results", "trim_threshold_bytes"], 32768i64).unwrap();
        assert_eq!(cfg_at(&missing)["results"]["trim_threshold_bytes"].as_integer(), Some(32768));

        let bad = d.path().join("bad.toml");
        std::fs::write(&bad, "[user]\nprompt = 3\n").unwrap();
        let e = set_user_key(&bad, &["user", "prompt", "x"], true).unwrap_err();
        assert_eq!(e.to_string(), "user config: user.prompt is not a table");
        assert_eq!(std::fs::read_to_string(&bad).unwrap(), "[user]\nprompt = 3\n");

        let inline = d.path().join("inline.toml");
        std::fs::write(&inline, "features = { results = true }\n").unwrap();
        set_user_key(&inline, &["features", "work_units"], false).unwrap();
        assert_eq!(cfg_at(&inline)["features"]["results"].as_bool(), Some(true));
        assert_eq!(cfg_at(&inline)["features"]["work_units"].as_bool(), Some(false));
    }


    /// N72 (N70 review): a team-layer config error reaches `zygo status` (`base`) escaped, on one line.
    #[test]
    fn status_escapes_a_team_config_error() {
        let d = TempDir::new().unwrap();
        let dir = sd(d.path());
        for (team, want) in [
            ("[role.\"x\\u001b[2K\\nforged\"]\nmodel = 1\n", "role.\"x\\u{1b}[2K\\nforged\".model: "),
            ("[[model.\"x\\u001b[2K\\nforged\".provider]]\nname = \"a\"\n[[model.\"x\\u001b[2K\\nforged\".provider]]\nname = \"a\"\n",
                "model.\"x\\u{1b}[2K\\nforged\".provider[0] and model.\"x\\u{1b}[2K\\nforged\".provider[1] have the same name"),
        ] {
            std::fs::write(dir.join(zygo_config::layers::TEAM_FILE), team).unwrap();
            let e = crate::cmd::status::merged(&ctx(d.path(), Some(&dir)), None).unwrap_err().to_string();
            assert!(e.starts_with("zygo status: team config (team): ") && e.contains(want), "{e:?}");
            assert!(!e.contains('\u{1b}') && !e.contains('\n'), "{e:?}");
        }
    }
}
