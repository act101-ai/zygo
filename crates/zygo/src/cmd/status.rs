//! `zygo status` and `/zygo:status` (§17, §17.1).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use zygo_config::Merged;
use zygo_hooks::statusline::{pct, short, trimmed};
use zygo_plugins::check::esc;
use zygo_proxy::client::{ClientError, SessionClient, SessionStatus, WindowArg};
use zygo_work::policy::Policy;
use zygo_work::{current, repo};

use super::compact::client_timeout;
use super::verbs::VerbCtx;

/// The top of the checkout holding the working directory, found within 2 s.
pub fn repo_top() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    repo::checkout(&cwd, Instant::now() + Duration::from_secs(2)).map(|c| c.top)
}

/// C7: the CLI form inside a session (a session dir, where the window comes from; §8.4) prints no window number.
pub fn hide_window(ctx: &VerbCtx) -> bool {
    !ctx.slash && ctx.session_dir.is_some()
}

/// The config with the team layer (§14 note 2026-11-19: every config-reading verb) and the session overrides (layer 7),
/// each applied on its own ([`super::verbs::with_session`]). Errors start `zygo status: `.
pub(crate) fn merged(ctx: &VerbCtx, repo: Option<&Path>) -> anyhow::Result<Merged> {
    let m = super::verbs::base(ctx, "status", super::verbs::team(ctx, "status")?, repo)?;
    Ok(super::verbs::with_session(m, ctx.session_dir.as_ref()).0)
}

/// `None` outside a session.
pub(crate) fn session_status(ctx: &VerbCtx) -> Option<Result<SessionStatus, ClientError>> {
    let dir = ctx.session_dir.as_ref()?;
    Some(SessionClient::from_session_dir(&dir.root).and_then(|c| c.timeout(client_timeout(ctx.slash)).status()))
}

pub fn run(ctx: &VerbCtx, repo: Option<&Path>, hide: bool) -> anyhow::Result<String> {
    let m = merged(ctx, repo)?;
    let mut lines = vec![match &m.config.server.url {
        Some(u) => format!("mode: serve {u}"),
        None => "mode: local".to_string(),
    }];
    let tokens = |v: Option<u64>| v.map_or_else(|| "-".to_string(), short);
    let mut session = Vec::new();
    let (profile, work) = match session_status(ctx) {
        Some(Ok(s)) => {
            session.push(format!("session: {}", esc(&s.session)));
            if hide {
                session.push(format!("context: {}", tokens(s.used_context)));
                session.push(format!("compact: {}", if s.compact.is_some() { "override set" } else { "none" }));
            } else {
                session.push(format!("context: {}/{}", tokens(s.used_context), tokens(s.effective_window)));
                session.push(format!(
                    "compact: {}",
                    match s.compact {
                        Some(WindowArg::Tokens(n)) => short(n),
                        Some(WindowArg::Off) => "off".to_string(),
                        None => "none".to_string(),
                    }
                ));
            }
            let t = trimmed(&ctx.paths.usage_db, &s.session).map_or_else(|| "-".to_string(), |n| n.to_string());
            session.push(format!("savings: cache {}, trimmed {t} bytes", pct(s.cache_read_share)));
            (s.profile, s.work)
        }
        outside => {
            if let Some(Err(e)) = outside {
                session.push(format!("session: unreachable ({})", esc(&e.to_string())));
            }
            let work = repo.and_then(|r| current::resolve(r, &m.config.work)).map(|w| w.unit());
            (m.config.user.default_profile.clone(), work)
        }
    };
    lines.push(format!("profile: {}", esc(profile.as_deref().unwrap_or("none"))));
    lines.push(format!("work: {}", esc(work.as_deref().unwrap_or("none"))));
    lines.push(Policy::resolve(&m, repo.unwrap_or(Path::new(""))).describe());
    lines.extend(session);
    Ok(lines.join("\n"))
}

pub fn verb(ctx: &VerbCtx, _args: &[String]) -> anyhow::Result<String> {
    run(ctx, repo_top().as_deref(), hide_window(ctx))
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;
    use zygo_proxy::client::{SessionClient, WindowArg};

    use super::*;
    use crate::cmd::compact::tests::{canned_session, ctx, dead_session, proxy};

    #[test]
    fn outside_a_session() {
        let d = TempDir::new().unwrap();
        assert_eq!(run(&ctx(d.path(), None), None, false).unwrap(), "mode: local\nprofile: none\nwork: none\nrepo policy: defaults");
        std::fs::write(d.path().join("config.toml"), "[server]\nurl = \"https://zygo.example\"\n").unwrap();
        let e = run(&ctx(d.path(), None), None, false).unwrap_err().to_string();
        assert_eq!(e, "zygo status: serve mode needs a team token: [server] token is not set or names an empty value");
        let dir = d.path().join("s");
        std::fs::create_dir_all(&dir).unwrap();
        let out = run(&ctx(d.path(), Some(&dir)), None, false).unwrap();
        assert_eq!(out.lines().next(), Some("mode: serve https://zygo.example"));
    }

    #[test]
    fn inside_a_session() {
        let d = TempDir::new().unwrap();
        let (_h, id, dir) = proxy(d.path());
        SessionClient::from_session_dir(&dir).unwrap().set_compact(WindowArg::Tokens(600_000)).unwrap();
        let c = ctx(d.path(), Some(&dir));
        let out = run(&c, None, false).unwrap();
        let session = format!("session: {id}");
        for l in ["profile: none", "work: ENG-42", &session, "context: -/600k", "compact: 600k", "savings: cache -, trimmed - bytes"] {
            assert!(out.lines().any(|x| x == l), "{l}: {out}");
        }
        let out = run(&c, None, true).unwrap();
        for l in ["context: -", "compact: override set"] {
            assert!(out.lines().any(|x| x == l), "{l}: {out}");
        }
        assert!(!out.contains("600k"), "{out}");
    }

    #[test]
    fn unreachable_session() {
        let d = TempDir::new().unwrap();
        let dir = dead_session(d.path(), "S1");
        let out = run(&ctx(d.path(), Some(&dir)), None, false).unwrap();
        assert!(out.lines().any(|l| l.starts_with("session: unreachable (zygo proxy unreachable")), "{out}");
    }

    #[test]
    fn server_text_cannot_forge_a_status_line() { // N70: status.rs:50, :71, :78
        let d = TempDir::new().unwrap();
        let st = serde_json::json!({
            "session": "S1\u{1b}[31m", "profile": null, "work": "one\ntwo\u{1b}[31m", "used_context": null,
            "effective_window": null, "compact": null, "cache_read_share": null, "expires_at": ""
        })
        .to_string();
        let dir = canned_session(d.path(), "S1", 200, st);
        assert_eq!(
            run(&ctx(d.path(), Some(&dir)), None, false).unwrap(),
            "mode: local\nprofile: none\nwork: one\\ntwo\\u{1b}[31m\nrepo policy: defaults\nsession: S1\\u{1b}[31m\n\
             context: -/-\ncompact: none\nsavings: cache -, trimmed - bytes"
        );
        let body = serde_json::json!({ "error": { "type": "bad", "message": "one\ntwo\u{1b}[31m" } }).to_string();
        let dir = canned_session(d.path(), "S2", 400, body);
        assert_eq!(
            run(&ctx(d.path(), Some(&dir)), None, false).unwrap(),
            "mode: local\nprofile: none\nwork: none\nrepo policy: defaults\nsession: unreachable (bad: one\\ntwo\\u{1b}[31m)"
        );
    } // mutations: `esc` removed at :50, :71 or :78
}
