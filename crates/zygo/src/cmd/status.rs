//! `zygo status` and `/zygo:status` (§17, §17.1).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use zygo_config::Merged;
use zygo_core::tokens::BYTES_PER_TOKEN;
use zygo_hooks::statusline::render::spend_text;
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
            // AU26, OP1: the trimmed bytes as estimated tokens, `ceil(bytes ÷ BYTES_PER_TOKEN)` (H§6.6).
            let t = tokens(trimmed(&ctx.paths.usage_db, &s.session).map(|bytes| bytes.div_ceil(BYTES_PER_TOKEN)));
            session.push(format!("savings: cache {}, trimmed {t} tok", pct(s.cache_read_share)));
            // AU26: the session spend in the AU15 forms; a reply with no `spend_usd` (an old server) gives no line.
            if let Some(spend) = spend_text(s.spend_usd, s.spend_notional, s.requests_total, s.requests_priced) {
                session.push(format!("spend: {spend}"));
            }
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
        assert!(!out.contains("spend:"), "{out}"); // AU26: no spend line outside a session
    }

    #[test]
    fn inside_a_session() {
        let d = TempDir::new().unwrap();
        let (_h, id, dir) = proxy(d.path());
        SessionClient::from_session_dir(&dir).unwrap().set_compact(WindowArg::Tokens(600_000)).unwrap();
        let c = ctx(d.path(), Some(&dir));
        let out = run(&c, None, false).unwrap();
        let session = format!("session: {id}");
        // No usage store yet: the trimmed count is unknown. No request yet: the spend is `$0.00` (AU15).
        for l in ["profile: none", "work: ENG-42", &session, "context: -/600k", "compact: 600k", "savings: cache -, trimmed - tok", "spend: $0.00"] {
            assert!(out.lines().any(|x| x == l), "{l}: {out}");
        }
        assert!(out.ends_with("\nsavings: cache -, trimmed - tok\nspend: $0.00"), "{out}");
        let out = run(&c, None, true).unwrap();
        for l in ["context: -", "compact: override set"] {
            assert!(out.lines().any(|x| x == l), "{l}: {out}");
        }
        assert!(!out.contains("600k"), "{out}");
        // C7 hides the window number only: the savings and the spend stay.
        for l in ["savings: cache -, trimmed - tok", "spend: $0.00"] {
            assert!(out.lines().any(|x| x == l), "{l}: {out}");
        }
    }

    /// A status reply with the OP13 numbers (H§14 #3) and `extra` merged over them.
    fn reply(id: &str, extra: serde_json::Value) -> String {
        let mut st = serde_json::json!({
            "session": id, "profile": null, "work": null, "used_context": 196_000, "effective_window": null, "compact": null,
            "cache_read_share": 0.92, "expires_at": "", "spend_usd": 3.41, "spend_notional": false, "requests_total": 12,
            "requests_priced": 12
        });
        for (k, v) in extra.as_object().unwrap() {
            st[k] = v.clone();
        }
        st.to_string()
    }

    fn trim(d: &Path, session: &str, stored_bytes: u64, view_bytes: u64) {
        let store = zygo_usage::Store::open(&crate::cmd::compact::tests::paths(d).usage_db).unwrap();
        store.insert_trim(&zygo_usage::TrimRow { session: session.into(), stored_bytes, view_bytes, ..Default::default() }).unwrap();
    }

    /// H§14 #18, AU26: 92,160 trimmed bytes print as `trimmed 23k tok`, and a session prints `spend: $3.41`.
    #[test]
    fn h14_18_trimmed_in_tokens_and_the_spend_line() {
        let d = TempDir::new().unwrap();
        let dir = canned_session(d.path(), "S1", 200, reply("S1", serde_json::json!({})));
        trim(d.path(), "S1", 100_000, 7_840);
        trim(d.path(), "other", 4_000, 0);
        for hide in [false, true] {
            let out = run(&ctx(d.path(), Some(&dir)), None, hide).unwrap();
            assert!(out.ends_with("\nsavings: cache 92%, trimmed 23k tok\nspend: $3.41"), "{out}");
            assert!(!out.contains("bytes"), "{out}");
        }
        // Estimated tokens are `ceil(bytes ÷ 4)`: 5 bytes are 2 tokens.
        let dir = canned_session(d.path(), "S2", 200, reply("S2", serde_json::json!({})));
        trim(d.path(), "S2", 5, 0);
        let out = run(&ctx(d.path(), Some(&dir)), None, false).unwrap();
        assert!(out.lines().any(|l| l == "savings: cache 92%, trimmed 2 tok"), "{out}");
    }

    /// AU26 with the AU15 forms: `≈` in front of a notional spend, `+` and `$?` for requests with no price, and no
    /// `spend:` line when the reply has no `spend_usd` (an old server).
    #[test]
    fn spend_line_forms() {
        let d = TempDir::new().unwrap();
        let cases = [
            ("N1", serde_json::json!({ "spend_usd": 1.84, "spend_notional": true }), Some("spend: ≈$1.84")),
            ("N2", serde_json::json!({ "requests_priced": 11 }), Some("spend: $3.41+")),
            ("N3", serde_json::json!({ "spend_usd": 0.0, "requests_priced": 0 }), Some("spend: $?")),
            ("N4", serde_json::json!({ "spend_usd": 0.0, "requests_total": 0, "requests_priced": 0 }), Some("spend: $0.00")),
            ("N5", serde_json::json!({ "spend_usd": 123.4 }), Some("spend: $123")),
            ("N6", serde_json::json!({ "spend_usd": null }), None),
        ];
        for (id, extra, want) in cases {
            let dir = canned_session(d.path(), id, 200, reply(id, extra));
            let out = run(&ctx(d.path(), Some(&dir)), None, false).unwrap();
            let spend: Vec<&str> = out.lines().filter(|l| l.starts_with("spend:")).collect();
            assert_eq!(spend, want.into_iter().collect::<Vec<_>>(), "{id}: {out}");
        }
    }

    #[test]
    fn unreachable_session() {
        let d = TempDir::new().unwrap();
        let dir = dead_session(d.path(), "S1");
        let out = run(&ctx(d.path(), Some(&dir)), None, false).unwrap();
        assert!(out.lines().any(|l| l.starts_with("session: unreachable (zygo proxy unreachable")), "{out}");
        assert!(!out.contains("spend:") && !out.contains("savings:"), "{out}");
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
             context: -/-\ncompact: none\nsavings: cache -, trimmed - tok"
        );
        let body = serde_json::json!({ "error": { "type": "bad", "message": "one\ntwo\u{1b}[31m" } }).to_string();
        let dir = canned_session(d.path(), "S2", 400, body);
        assert_eq!(
            run(&ctx(d.path(), Some(&dir)), None, false).unwrap(),
            "mode: local\nprofile: none\nwork: none\nrepo policy: defaults\nsession: unreachable (bad: one\\ntwo\\u{1b}[31m)"
        );
    } // mutations: `esc` removed at :50, :71 or :78
}
