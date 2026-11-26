//! The "Other machines" clause of session text item 8 (mail sync design S§10.4, S§19 #27 session text part;
//! M§8a item 1). The cache is written with `write_cache`; no test runs git or the sync child.
use zygo_core::Agent;
use zygo_hooks::mail;
use zygo_workspace::state::{MemberRecord, Workspace};

const MAIL_TAIL: &str = "Send with mail_send (to a name, or `all`); read with mail_read; mail_list shows unread mail and the current members. zygo tells you when mail arrives: before your turn ends, at your next prompt, or with a wake line. Do not poll or wait for mail: end your turn, and you will be woken.";
const ONE: &str = "Other machines: bob@b; mail them by the full address.";
const ID: &str = "0123456789abcdef";
const KEY: &str = "kaaaaaaaa";

fn addrs(n: usize) -> Vec<String> {
    (0..n).map(|i| format!("m{i}@b")).collect()
}
fn first64(a: &[String]) -> String {
    a[..64].join(", ")
}
/// Item 8 of the member `elena` alone in `app`, with `clause` after the member list.
fn item(clause: Option<&str>) -> String {
    let c = clause.map_or(String::new(), |c| format!("{c} "));
    format!("Workspace mail: you are `elena` in workspace `app`. Members: `elena`. {c}{MAIL_TAIL}")
}

#[test] // S§10.4
fn other_machines_lists_each_address_then_the_rule() {
    assert_eq!(mail::other_machines(&["bob@b".to_string()]).as_deref(), Some(ONE));
    assert_eq!(
        mail::other_machines(&["bob@b".to_string(), "eve@c".to_string()]).as_deref(),
        Some("Other machines: bob@b, eve@c; mail them by the full address.")
    );
}

#[test] // S§10.4: at most 64 addresses, then `and <n> more; mail_list lists all`
fn other_machines_holds_at_most_64_addresses() {
    let a = addrs(65);
    assert_eq!(
        mail::other_machines(&a),
        Some(format!("Other machines: {}, and 1 more; mail_list lists all; mail them by the full address.", first64(&a)))
    );
    assert!(!mail::other_machines(&a).unwrap().contains("m64@b"));
    assert_eq!(mail::other_machines(&a[..64]), Some(format!("Other machines: {}; mail them by the full address.", first64(&a))));
    let a = addrs(300);
    assert_eq!(
        mail::other_machines(&a),
        Some(format!("Other machines: {}, and 236 more; mail_list lists all; mail them by the full address.", first64(&a)))
    );
}

#[test] // S§10.4: only when the member cache is not empty
fn other_machines_is_none_for_no_address() {
    assert_eq!(mail::other_machines(&[]), None);
}

#[test] // S§10.4: the clause comes after the member list; M§8a: no addresses give today's item
fn session_item_with_puts_the_clause_after_the_member_list() {
    let mut ws = Workspace::new(ID);
    ws.name = "app".into();
    ws.members.insert(KEY.into(), MemberRecord { name: Some("elena".into()), ..MemberRecord::new(Agent::ClaudeCode) });
    assert_eq!(mail::session_item_with(&ws, KEY, &["bob@b".to_string()]), Some(item(Some(ONE))));
    assert_eq!(mail::session_item_with(&ws, KEY, &[]), Some(item(None)));
    assert_eq!(mail::session_item_with(&ws, KEY, &[]), mail::session_item(&ws, KEY));
    assert_eq!(mail::session_item_with(&ws, "kbbbbbbbb", &["bob@b".to_string()]), None);
}

/// Item 8 at `SessionStart` from `mail/sync/members.json` (S§7.3, S§10.4, S§19 #27). Workspaces need tmux (OP5).
#[cfg(unix)]
mod at_session_start {
    use super::*;
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use serde_json::{json, Value};
    use tempfile::TempDir;
    use zygo_core::paths::Paths;
    use zygo_core::session_dir::SessionDir;
    use zygo_core::session_view::{Member, SessionView, ViewWorkspace};
    use zygo_hooks::workspace;
    use zygo_mail_sync::env::{Clock, Env, NoSeam, Probe, Spawner, TmuxWake, Verdict};
    use zygo_mail_sync::registry::{write_cache, MemberCache};
    use zygo_mail_sync::state::SyncPaths;
    use zygo_workspace::mail::mail_dir;
    use zygo_workspace::state::{Phase, Store};

    const SID: &str = "01JTESTSESSIONAAAAAAAAAAAA";
    const S5: Duration = Duration::from_secs(5);
    /// Sync on for the workspace `app`.
    const ON: &str = "[mail]\nhost = \"studio\"\n\n[mail.workspace.app]\nsync = \"zygo/mail\"\n";
    /// Mail on, sync off.
    const OFF: &str = "[mail]\nhost = \"studio\"\n";

    /// `data_dir` = `<p>/data` is not under `$HOME`, so the state folder is `<p>/data/state`.
    fn paths(p: &Path) -> Paths {
        Paths { config_file: p.join("config.toml"), cache_dir: p.join("cache"), data_dir: p.join("data"),
            sessions_dir: p.join("cache/sessions"), marketplaces_dir: p.join("cache/marketplaces"),
            plugin_store: p.join("data/plugins"), approvals_file: p.join("data/approvals.toml"), usage_db: p.join("data/usage.db") }
    }

    struct Fx {
        _t: TempDir,
        agent: Agent,
        paths: Paths,
        dir: SessionDir,
        zygo: PathBuf,
    }

    /// The member `KEY` named `elena`, alone in the workspace `app` (`ID`), phase `up`, whose record belongs to the
    /// session `SID`; the view has mail on; `config` is the user config.
    fn fx(agent: Agent, config: &str) -> Fx {
        let t = TempDir::new().unwrap();
        let paths = paths(t.path());
        let dir = SessionDir::new(paths.sessions_dir.join(SID));
        dir.create().unwrap();
        let marker = ViewWorkspace { member: Member { ws_id: ID.into(), pane: KEY.into(), ..Default::default() }, ..Default::default() };
        SessionView { session: SID.into(), agent, main_role: "lead".into(), workspace: Some(marker), mail: true, ..Default::default() }
            .save(&dir)
            .unwrap();
        let mut rec = MemberRecord::new(agent);
        rec.zygo_session = Some(SID.into());
        rec.name = Some("elena".into());
        let root = t.path().to_path_buf();
        Store::open(&paths, ID)
            .update(S5, &mut |w| {
                w.name = "app".into();
                w.tmux_session = "zygo-app".into();
                w.common_dir = root.join("app/.git");
                w.main_root = root.join("app");
                w.loadout = "solo".into();
                w.phase = Phase::Up;
                w.members.insert(KEY.into(), rec.clone());
                Ok(())
            })
            .unwrap();
        fs::write(&paths.config_file, config).unwrap();
        let zygo = t.path().join("missing");
        Fx { _t: t, agent, paths, dir, zygo }
    }

    /// The fixed clock.
    struct At(SystemTime);

    impl Clock for At {
        fn now(&self) -> SystemTime {
            self.0
        }
    }

    /// The hook path runs no git (S§7.3).
    struct NoGit;

    impl Probe for NoGit {
        fn probe(&self, _https_url: &str) -> Verdict {
            panic!("the hook path runs no git")
        }
    }

    /// A sync child that starts and does nothing.
    struct NoChild;

    impl Spawner for NoChild {
        fn spawn_sync(&self, _ws_id: &str) -> io::Result<()> {
            Ok(())
        }
    }

    fn env(f: &Fx) -> Env {
        Env { paths: f.paths.clone(), clock: Box::new(At(UNIX_EPOCH + Duration::from_secs(1_800_000_000))),
            probe: Box::new(NoGit), seam: Box::new(NoSeam), spawner: Box::new(NoChild),
            wake: Box::new(TmuxWake { tmux: "/dev/null/tmux".into() }), secret_policy: Default::default() }
    }

    /// Writes the member cache of `f` with `hosts`.
    fn cache(f: &Fx, hosts: &[(&str, &[&str])]) {
        let c = MemberCache {
            hosts: hosts.iter().map(|(h, ns)| (h.to_string(), ns.iter().map(|n| n.to_string()).collect())).collect(),
        };
        write_cache(&SyncPaths::new(&f.paths, ID).unwrap(), &c).unwrap();
    }

    /// The member part of `SessionStart` with the session text `TEXT`: `additionalContext` of the stdout JSON for
    /// Claude Code, the result for OpenCode.
    fn text(f: &Fx) -> String {
        let view = SessionView::load(&f.dir).unwrap();
        let mut out = Vec::new();
        let r = workspace::session_start_with_sync(&json!({}), &f.dir, &view, &f.paths, &f.zygo, &mut out, "TEXT", &env(f))
            .unwrap();
        match f.agent {
            Agent::ClaudeCode => {
                let v: Value = serde_json::from_slice(&out).unwrap();
                v["hookSpecificOutput"]["additionalContext"].as_str().unwrap().to_string()
            }
            _ => r.unwrap(),
        }
    }

    #[test] // S§19 #27, S§10.4: sync on and `b`'s registry with `bob` in the cache
    fn item_8_names_bob_at_b_after_the_member_list() {
        for agent in [Agent::ClaudeCode, Agent::OpenCode] {
            let f = fx(agent, ON);
            cache(&f, &[("b", &["bob"])]);
            assert_eq!(text(&f), format!("TEXT\n\n{}", item(Some(ONE))), "{agent:?}");
        }
    }

    #[test] // S§10.4: 64 addresses, then `and 1 more; mail_list lists all`
    fn item_8_holds_64_of_65_addresses() {
        let f = fx(Agent::ClaudeCode, ON);
        let names: Vec<String> = (0..65).map(|i| format!("m{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        cache(&f, &[("b", &refs)]);
        let c = format!(
            "Other machines: {}, and 1 more; mail_list lists all; mail them by the full address.",
            first64(&addrs(65))
        );
        assert_eq!(text(&f), format!("TEXT\n\n{}", item(Some(&c))));
    }

    #[test] // S§10.4: sync off, no cache or an empty cache add nothing
    fn no_clause_with_sync_off_or_an_empty_cache() {
        let off = fx(Agent::ClaudeCode, OFF);
        cache(&off, &[("b", &["bob"])]);
        assert_eq!(text(&off), format!("TEXT\n\n{}", item(None)));
        let none = fx(Agent::ClaudeCode, ON);
        assert_eq!(text(&none), format!("TEXT\n\n{}", item(None)));
        let empty = fx(Agent::ClaudeCode, ON);
        cache(&empty, &[]);
        assert_eq!(text(&empty), format!("TEXT\n\n{}", item(None)));
    }

    #[test] // S§10.4, M§8a, M-C2: addresses only, no message text
    fn the_clause_holds_addresses_only() {
        let f = fx(Agent::ClaudeCode, ON);
        cache(&f, &[("b", &["bob"])]);
        let new = mail_dir(&f.paths, ID).join(KEY).join("new");
        fs::create_dir_all(&new).unwrap();
        fs::write(new.join(format!("01J{:023}.json", 0)), r#"{"from":"bob@b","subject":"s-private","body":"b-private"}"#)
            .unwrap();
        let got = text(&f);
        assert_eq!(got, format!("TEXT\n\n{}\n\nzygo: 1 new messages; read them with mail_read", item(Some(ONE))));
        assert!(!got.contains("private"), "{got}");
    }
}
