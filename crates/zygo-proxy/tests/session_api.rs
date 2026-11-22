//! N10: proxy server, session API (§7.2, §7.3, §10.2.4, §17.1) and session client.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tempfile::TempDir;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use zygo_config::profile::Profile;
use zygo_config::{load, Action, CustomRole, Dialect, LoadContext, Merged, RedactionPattern, TokenUsage};
use zygo_core::ids::ulid;
use zygo_core::paths::Paths;
use zygo_core::session_dir::SessionDir;
use zygo_core::Agent;
use zygo_proxy::client::{ClientError, SessionClient, SessionStatus};
use zygo_proxy::errors::ZygoError;
use zygo_proxy::server::StartOpts;
use zygo_proxy::session::{
    Clock, ManualClock, PromptUsage, SessionGrant, SessionRequest, SessionSettings, TeamAuth, WindowArg, IDLE_TTL,
    REDACTION_NOTE, SUSPEND_AFTER, WINDOW_MAX, WINDOW_MIN,
};
use zygo_proxy::tee::{CacheSplit, Tee, Usage};
use zygo_proxy::{Proxy, ProxyHandle, Url};
use zygo_redact::act_lite_store::{StoredValue, MIN_VALUE_BYTES};
use zygo_usage::{RequestRow, Store};

// ---- helpers ----

fn ctx(d: &TempDir, team: Option<&str>) -> LoadContext {
    let d = d.path();
    let paths = Paths {
        config_file: d.join("config.toml"),
        cache_dir: d.join("cache"),
        data_dir: d.join("data"),
        sessions_dir: d.join("cache/sessions"),
        marketplaces_dir: d.join("cache/marketplaces"),
        plugin_store: d.join("data/plugins"),
        approvals_file: d.join("data/approvals.toml"),
        usage_db: d.join("data/usage.db"),
    };
    LoadContext { paths, repo: None, team: team.map(|t| toml::from_str(t).unwrap()) }
}

fn cfg(d: &TempDir) -> Merged {
    let mut m = load(&ctx(d, None)).unwrap();
    m.config.catalog.0.iter_mut().find(|r| r.id == "claude-opus-5-5").unwrap().soft_context_window = Some(800_000);
    m
}

const TEAM: &str = r#"
[features.redaction]
content = true

[[model]]
id = "claude-opus-5-5"
dialects = ["anthropic"]
soft_context_window = 800000
[model.facts]
context_window = 1000000
max_output_tokens = 128000
"#;

fn team_cfg(d: &TempDir) -> Merged {
    load(&ctx(d, Some(TEAM))).unwrap()
}

struct TestTeam(Vec<(String, String)>);

impl TeamAuth for TestTeam {
    fn label_for(&self, token: &str) -> Option<String> {
        self.0.iter().find(|(t, _)| t == token).map(|(_, l)| l.clone())
    }
}

/// Proxy with a test team-auth (team A "alpha", team B "beta") and a manual clock.
fn start_arc(cfg: Arc<Merged>) -> (ProxyHandle, Arc<ManualClock>, String, String) {
    let c = ManualClock::new();
    let a = format!("team-{}", ulid());
    let b = format!("team-{}", ulid());
    let team = TestTeam(vec![(a.clone(), "alpha".into()), (b.clone(), "beta".into())]);
    let opts = StartOpts { team: Some(Arc::new(team)), clock: c.clone(), ..Default::default() };
    let h = Proxy::start_local_with(cfg, opts).unwrap();
    (h, c, a, b)
}

fn start(cfg: Merged) -> (ProxyHandle, Arc<ManualClock>, String, String) {
    start_arc(Arc::new(cfg))
}

fn req() -> SessionRequest {
    SessionRequest {
        agent: Agent::ClaudeCode,
        agent_version: "2.1.328".into(),
        profile: None,
        passthrough: false,
        repo: None,
        work: Some("ENG-42".into()),
        stored_values: Vec::new(),
        known_plugins: Default::default(),
    }
}

fn p1() -> Profile {
    Profile { name: "p1".into(), ..Default::default() }
}

fn client(h: &ProxyHandle, g: &SessionGrant, d: &TempDir) -> SessionClient {
    let dir = d.path().join(&g.session);
    let sd = SessionDir::new(dir.clone());
    sd.create().unwrap();
    sd.write_token(&g.token).unwrap();
    std::fs::write(sd.url(), h.url().as_str()).unwrap();
    SessionClient::from_session_dir(&dir).unwrap()
}

fn http_client() -> reqwest::Client {
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::builder().no_proxy().build().unwrap()
}

struct Raw {
    rt: tokio::runtime::Runtime,
    http: reqwest::Client,
}

impl Raw {
    fn new() -> Raw {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        Raw { rt, http: http_client() }
    }

    fn call(&self, url: &Url, method: &str, path: &str, headers: &[(&str, &str)], body: Option<Vec<u8>>) -> (u16, Value, Vec<u8>) {
        let m = reqwest::Method::from_bytes(method.as_bytes()).unwrap();
        let mut rb = self.http.request(m, url.join(path).unwrap());
        for (k, v) in headers {
            rb = rb.header(*k, *v);
        }
        if let Some(b) = body {
            rb = rb.header("content-type", "application/json").body(b);
        }
        self.rt.block_on(async {
            let r = rb.send().await.unwrap();
            let status = r.status().as_u16();
            let bytes = r.bytes().await.unwrap().to_vec();
            let v = serde_json::from_slice(&bytes).unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into()));
            (status, v, bytes)
        })
    }
}

fn j(v: Value) -> Option<Vec<u8>> {
    Some(v.to_string().into_bytes())
}

fn err_type(v: &Value) -> &str {
    assert_eq!(v["type"], "error", "{v}");
    assert!(v["error"]["message"].is_string(), "{v}");
    v["error"]["type"].as_str().unwrap()
}

fn st(r: (u16, Value, Vec<u8>)) -> (u16, Value) {
    err_type(&r.1);
    (r.0, r.1["error"]["type"].clone())
}

fn sess(g: &SessionGrant) -> [(&'static str, &str); 2] {
    [("x-api-key", g.token.as_str()), ("x-zygo-session", g.session.as_str())]
}

fn sp(g: &SessionGrant) -> String {
    format!("v1/zygo/sessions/{}/status", g.session)
}
fn hb(g: &SessionGrant) -> String {
    format!("v1/zygo/sessions/{}/heartbeat", g.session)
}
fn cp(g: &SessionGrant) -> String {
    format!("v1/zygo/sessions/{}/compact", g.session)
}
fn sp_set(g: &SessionGrant) -> String {
    format!("v1/zygo/sessions/{}/settings", g.session)
}
fn del(g: &SessionGrant) -> String {
    format!("v1/zygo/sessions/{}", g.session)
}

fn team_body() -> Value {
    json!({"agent":"claude-code","agent_version":"2.1.328","profile":null,"passthrough":false,"repo":null,"work":null})
}

/// Team session T, created over HTTP with `bearer`.
fn team_session(raw: &Raw, u: &Url, bearer: &str) -> SessionGrant {
    let r = raw.call(u, "POST", "v1/zygo/sessions", &[("authorization", bearer)], j(team_body()));
    assert_eq!(r.0, 200, "{}", r.1);
    serde_json::from_value(r.1).unwrap()
}

// ---- tests ----

#[test]
fn healthz_on_loopback_and_shutdown() {
    let d = TempDir::new().unwrap();
    let (h, _c, _, _) = start(cfg(&d));
    let raw = Raw::new();
    let u = h.url();
    assert_eq!(h.url().host_str(), Some("127.0.0.1"));
    assert_ne!(h.url().port(), Some(0));
    assert_eq!(h.url().path(), "/");
    assert_eq!(raw.call(&u, "GET", "healthz", &[], None).0, 200);
    assert_eq!(raw.call(&u, "GET", "healthz", &[], None).1, json!("ok"));
    h.shutdown();
}

#[test]
fn create_session_grant_shape() {
    let d = TempDir::new().unwrap();
    let (h, c, _, _) = start(cfg(&d));
    let g = h.create_session(req()).unwrap();
    let g2 = h.create_session(req()).unwrap();
    assert_eq!(g.session.len(), 26);
    assert!(g.session.bytes().all(|b| b.is_ascii_digit() || b.is_ascii_uppercase()));
    assert!(g.token.starts_with("zyg_s_"));
    assert_eq!(g.token.len(), 49);
    assert!(g.token[6..].bytes().all(|b| b.is_ascii_alphanumeric()));
    assert_eq!(OffsetDateTime::parse(&g.expires_at, &Rfc3339).unwrap(), c.utc() + time::Duration::hours(24));
    assert!(g.config.roles.contains_key("lead"));
    assert_eq!(g.config.main_role, "lead");
    assert_eq!((g.profile_warning.as_deref(), g.redaction_note.as_deref()), (None, None));
    assert_ne!(g.session, g2.session);
    assert_ne!(g.token, g2.token);
    let dbg = format!("{g:?}");
    assert!(!dbg.contains(&g.token));
    assert!(dbg.contains("[redacted]"));
}

#[test]
fn profile_applied_to_session_config() {
    let d = TempDir::new().unwrap();
    let cfg_arc = Arc::new(cfg(&d));
    let (h, _c, _, _) = start_arc(cfg_arc.clone());
    let mut p = p1();
    p.disabled = vec!["design".into()];
    p.overrides = BTreeMap::from([("code".to_string(), "claude-haiku-5-5".to_string())]);
    p.roles = BTreeMap::from([(
        "bench".to_string(),
        CustomRole {
            model: "claude-haiku-5-5".into(),
            description: "Runs benchmarks.".into(),
            prompt: "You run the benchmark.".into(),
            writes: false,
        },
    )]);
    p.features = Some(toml::from_str("[redaction]\ncontent = false").unwrap());
    let mut r = req();
    r.profile = Some(p);
    let g = h.create_session(r).unwrap();
    assert_eq!(g.profile_warning, None);
    assert!(!g.config.roles.contains_key("design"));
    assert_eq!(g.config.roles["code"].model, "claude-haiku-5-5");
    assert_eq!(g.config.custom_roles["bench"].model, "claude-haiku-5-5");
    assert!(g.config.models.contains(&"claude-opus-5-5".to_string()));
    let s = h.session(&g.session).unwrap();
    assert_eq!(s.status().profile.as_deref(), Some("p1"));
    assert!(!s.features().redaction.content);
    assert!(s.features().results);
    assert!(Arc::ptr_eq(&s.cfg, &cfg_arc));
}

#[test]
fn invalid_profile_dropped_with_warning() {
    let d = TempDir::new().unwrap();
    let (h, _c, _, _) = start(cfg(&d));
    let mut p = p1();
    p.disabled = vec!["lead".into()];
    let mut r = req();
    r.profile = Some(p);
    let g = h.create_session(r).unwrap();
    assert_eq!(g.profile_warning.as_deref(), Some("profile.p1.disabled[0]: is the main role and cannot be disabled"));
    assert!(g.config.roles.contains_key("design"));
    assert_eq!(h.session(&g.session).unwrap().status().profile, None);
}

#[test]
fn stored_values_stay_in_process() {
    let d = TempDir::new().unwrap();
    let (h, _c, _, _) = start(cfg(&d));
    let v = format!("v{}", ulid());
    let mut r = req();
    r.stored_values = vec![StoredValue::new(v.clone())];
    assert!(!format!("{r:?}").contains(&v));
    assert!(!serde_json::to_string(&r).unwrap().contains(&v));
    let g = h.create_session(r).unwrap();
    assert_eq!(g.redaction_note, None);
    assert_eq!(h.session(&g.session).unwrap().redaction.compiled.ids()[0], "act_lite_secret");
}

#[test]
fn short_stored_values_dropped() {
    let d = TempDir::new().unwrap();
    let (h, _c, _, _) = start(cfg(&d));
    let mut r = req();
    r.stored_values = vec![StoredValue::new(ulid()[..5].to_string())];
    let g = h.create_session(r).unwrap();
    assert!(!h.session(&g.session).unwrap().redaction.compiled.ids().contains(&"act_lite_secret"));
    assert_eq!(g.redaction_note, None);
    assert_eq!(MIN_VALUE_BYTES, 6);
}

#[test]
fn store_pattern_that_fails_is_dropped_with_note() {
    let d = TempDir::new().unwrap();
    let (h, _c, _, _) = start(cfg(&d));
    let mut r = req();
    // Its literal regex exceeds the regex crate's default size limit (10 MiB compiled; a 512 KiB literal already does).
    r.stored_values = vec![StoredValue::new("a".repeat(1 << 20))];
    let g = h.create_session(r).unwrap();
    assert_eq!(g.redaction_note.as_deref(), Some(REDACTION_NOTE));
    let s = h.session(&g.session).unwrap();
    let ids = s.redaction.compiled.ids();
    assert!(!ids.contains(&"act_lite_secret"));
    assert!(!ids.is_empty());
}

#[test]
fn base_patterns_that_fail_refuse_session() {
    let d = TempDir::new().unwrap();
    let mut m = cfg(&d);
    m.config.redaction.pattern.extend((0..65_536).map(|i| RedactionPattern {
        id: format!("p{i}"),
        pattern: "x".into(),
        action: Action::Redact,
        description: String::new(),
    }));
    let (h, _c, _, _) = start(m);
    let e = h.create_session(req()).unwrap_err();
    assert_eq!((e.status.as_u16(), e.code), (503, "zygo_redaction_unavailable"));
}

#[test]
fn error_body_shapes() {
    let a = ZygoError::token_invalid().body(Dialect::Anthropic);
    assert_eq!(a, json!({"type":"error","error":{"type":"zygo_token_invalid","message":"invalid session token"}}));
    let o = ZygoError::session_suspended().body(Dialect::Openai);
    assert_eq!(o["error"]["type"], "zygo_session_suspended");
    assert!(o["error"]["message"].is_string());
    assert!(o.get("type").is_none());

    let d = TempDir::new().unwrap();
    let (h, _c, _, _) = start(cfg(&d));
    let raw = Raw::new();
    let u = h.url();
    let path = format!("v1/zygo/sessions/{}/status", ulid());
    let ct = raw.rt.block_on(async {
        let r = raw.http.get(u.join(&path).unwrap()).send().await.unwrap();
        r.headers().get("content-type").map(|v| v.to_str().unwrap().to_string())
    });
    assert_eq!(ct.as_deref(), Some("application/json"));
    assert_eq!(st(raw.call(&u, "GET", &path, &[], None)), (401, json!("zygo_token_invalid")));
}

#[test]
fn session_token_checks() {
    let d = TempDir::new().unwrap();
    let (h, _c, _, _) = start(cfg(&d));
    let raw = Raw::new();
    let u = h.url();
    let a = h.create_session(req()).unwrap();
    let b = h.create_session(req()).unwrap();
    let p = sp(&a);
    assert_eq!(st(raw.call(&u, "GET", &p, &[], None)), (401, json!("zygo_token_invalid")));
    assert_eq!(raw.call(&u, "GET", &p, &sess(&a), None).0, 200);
    let bearer = format!("Bearer {}", a.token);
    assert_eq!(raw.call(&u, "GET", &p, &[("authorization", &bearer), ("x-zygo-session", &a.session)], None).0, 200);
    assert_eq!(st(raw.call(&u, "GET", &p, &[("x-api-key", &a.token)], None)), (401, json!("zygo_token_invalid")));
    assert_eq!(
        st(raw.call(&u, "GET", &p, &[("x-api-key", &a.token), ("x-zygo-session", &b.session)], None)),
        (401, json!("zygo_token_invalid"))
    );
    assert_eq!(st(raw.call(&u, "GET", &sp(&b), &sess(&a), None)), (401, json!("zygo_token_invalid")));
    let r = raw.call(&u, "GET", &p, &[("x-api-key", &b.token), ("x-zygo-session", &a.session)], None);
    assert!(!String::from_utf8_lossy(&r.2).contains(&b.token));
    assert_eq!(st(r), (401, json!("zygo_token_invalid")));
    let n = ulid();
    assert_eq!(
        st(raw.call(&u, "GET", &format!("v1/zygo/sessions/{n}/status"), &[("x-api-key", &a.token), ("x-zygo-session", &n)], None)),
        (401, json!("zygo_session_unknown"))
    );
}

#[test]
fn restart_loses_sessions() {
    let d = TempDir::new().unwrap();
    let (h1, _c1, _, _) = start(cfg(&d));
    let a = h1.create_session(req()).unwrap();
    h1.shutdown();
    let (h2, _c2, _, _) = start(cfg(&d));
    let raw = Raw::new();
    let u2 = h2.url();
    assert_eq!(st(raw.call(&u2, "GET", &sp(&a), &sess(&a), None)), (401, json!("zygo_session_unknown")));
}

#[test]
fn idle_ttl_24h() {
    let d = TempDir::new().unwrap();
    let (h, c, _, _) = start(cfg(&d));
    let raw = Raw::new();
    let u = h.url();
    let a = h.create_session(req()).unwrap();
    let p = sp(&a);
    assert_eq!(IDLE_TTL, Duration::from_secs(86_400));
    c.advance(IDLE_TTL - Duration::from_secs(1));
    assert_eq!(raw.call(&u, "GET", &p, &sess(&a), None).0, 200);
    c.advance(IDLE_TTL - Duration::from_secs(1));
    assert_eq!(raw.call(&u, "GET", &p, &sess(&a), None).0, 200); // refreshed
    c.advance(IDLE_TTL + Duration::from_secs(1));
    assert_eq!(st(raw.call(&u, "GET", &p, &sess(&a), None)), (401, json!("zygo_session_unknown")));
}

#[test]
fn local_session_never_suspends() {
    let d = TempDir::new().unwrap();
    let (h, c, _, _) = start(cfg(&d));
    let raw = Raw::new();
    let u = h.url();
    let a = h.create_session(req()).unwrap();
    c.advance(Duration::from_secs(6 * 60));
    assert_eq!(raw.call(&u, "GET", &sp(&a), &sess(&a), None).0, 200);
}

#[test]
fn suspend_after_5_min_and_resume_on_heartbeat() {
    let d = TempDir::new().unwrap();
    let (h, c, ta, _) = start(cfg(&d));
    let raw = Raw::new();
    let u = h.url();
    let bearer_a = format!("Bearer {ta}");
    let t = team_session(&raw, &u, &bearer_a);
    assert_eq!(raw.call(&u, "POST", &cp(&t), &sess(&t), j(json!({"window": 600000}))).0, 200);
    assert_eq!(SUSPEND_AFTER, Duration::from_secs(300));
    c.advance(Duration::from_secs(299));
    assert_eq!(raw.call(&u, "GET", &sp(&t), &sess(&t), None).0, 200);
    c.advance(Duration::from_secs(2));
    assert_eq!(st(raw.call(&u, "GET", &sp(&t), &sess(&t), None)), (401, json!("zygo_session_suspended")));
    assert_eq!(raw.call(&u, "POST", &hb(&t), &[("authorization", &bearer_a)], None).0, 204);
    let r = raw.call(&u, "GET", &sp(&t), &sess(&t), None);
    assert_eq!(r.0, 200);
    assert_eq!(r.1["compact"], 600000);
}

#[test]
fn heartbeat_resets_suspend_clock() {
    let d = TempDir::new().unwrap();
    let (h, c, ta, _) = start(cfg(&d));
    let raw = Raw::new();
    let u = h.url();
    let bearer_a = format!("Bearer {ta}");
    let t = team_session(&raw, &u, &bearer_a);
    c.advance(Duration::from_secs(240));
    assert_eq!(raw.call(&u, "POST", &hb(&t), &[("authorization", &bearer_a)], None).0, 204);
    c.advance(Duration::from_secs(240));
    assert_eq!(raw.call(&u, "GET", &sp(&t), &sess(&t), None).0, 200);
}

#[test]
fn heartbeats_keep_session_past_idle_ttl() {
    let d = TempDir::new().unwrap();
    let (h, c, ta, _) = start(cfg(&d));
    let raw = Raw::new();
    let u = h.url();
    let bearer_a = format!("Bearer {ta}");
    let t = team_session(&raw, &u, &bearer_a);
    for _ in 0..1441 {
        c.advance(Duration::from_secs(60));
        assert_eq!(raw.call(&u, "POST", &hb(&t), &[("authorization", &bearer_a)], None).0, 204);
    }
    assert!(h.session(&t.session).is_some());
    assert_eq!(raw.call(&u, "GET", &sp(&t), &sess(&t), None).0, 200); // 24 h 1 min after creation
}

#[test]
fn suspended_session_deleted_after_idle_ttl() {
    let d = TempDir::new().unwrap();
    let (h, c, ta, _) = start(cfg(&d));
    let raw = Raw::new();
    let u = h.url();
    let bearer_a = format!("Bearer {ta}");
    let t = team_session(&raw, &u, &bearer_a);
    c.advance(IDLE_TTL + Duration::from_secs(1));
    assert_eq!(st(raw.call(&u, "POST", &hb(&t), &[("authorization", &bearer_a)], None)), (401, json!("zygo_session_unknown")));
    assert!(h.session(&t.session).is_none());
}

#[test]
fn credentials_per_route() {
    let d = TempDir::new().unwrap();
    let (h, _c, ta, tb) = start(cfg(&d));
    let raw = Raw::new();
    let u = h.url();
    let (bearer_a, bearer_b) = (format!("Bearer {ta}"), format!("Bearer {tb}"));
    let body = team_body();
    assert_eq!(st(raw.call(&u, "POST", "v1/zygo/sessions", &[], j(body.clone()))), (401, json!("zygo_token_invalid")));
    assert_eq!(
        st(raw.call(&u, "POST", "v1/zygo/sessions", &[("authorization", "Bearer team-unknown")], j(body.clone()))),
        (401, json!("zygo_token_invalid"))
    );
    let t = team_session(&raw, &u, &bearer_a);
    let session_bearer = format!("Bearer {}", t.token);
    assert_eq!(st(raw.call(&u, "POST", &hb(&t), &[("authorization", &session_bearer)], None)), (401, json!("zygo_token_invalid")));
    assert_eq!(st(raw.call(&u, "POST", &hb(&t), &[("authorization", &bearer_b)], None)), (401, json!("zygo_session_unknown")));
    assert_eq!(raw.call(&u, "POST", &hb(&t), &[("authorization", &bearer_a)], None).0, 204);
    let team_on_session = [("authorization", bearer_a.as_str()), ("x-zygo-session", t.session.as_str())];
    assert_eq!(st(raw.call(&u, "GET", &sp(&t), &team_on_session, None)), (401, json!("zygo_token_invalid")));
    assert_eq!(
        st(raw.call(&u, "POST", &cp(&t), &team_on_session, j(json!({"window": 600000})))),
        (401, json!("zygo_token_invalid"))
    );
    assert_eq!(
        st(raw.call(&u, "PATCH", &sp_set(&t), &team_on_session, j(json!({"redaction_content": false})))),
        (401, json!("zygo_token_invalid"))
    );
    assert_eq!(st(raw.call(&u, "DELETE", &del(&t), &[("authorization", &bearer_b)], None)), (401, json!("zygo_session_unknown")));
    assert_eq!(raw.call(&u, "DELETE", &del(&t), &[("authorization", &bearer_a)], None).0, 204);
    assert_eq!(st(raw.call(&u, "GET", &sp(&t), &sess(&t), None)), (401, json!("zygo_session_unknown")));
}

#[test]
fn team_routes_refused_without_team_auth() {
    let d = TempDir::new().unwrap();
    let h = Proxy::start_local(Arc::new(cfg(&d))).unwrap();
    let raw = Raw::new();
    let u = h.url();
    assert_eq!(
        st(raw.call(&u, "POST", "v1/zygo/sessions", &[("authorization", "Bearer team-any")], j(team_body()))),
        (401, json!("zygo_token_invalid"))
    );
}

#[test]
fn compact_override_range() {
    let d = TempDir::new().unwrap();
    let (h, _c, _, _) = start(cfg(&d));
    let raw = Raw::new();
    let u = h.url();
    let a = h.create_session(req()).unwrap();
    for bad in [json!(99_999), json!(10_000_001), json!("600k"), json!("on"), json!(600000.5)] {
        assert_eq!(st(raw.call(&u, "POST", &cp(&a), &sess(&a), j(json!({"window": bad})))), (400, json!("zygo_bad_request")));
    }
    assert_eq!((WINDOW_MIN, WINDOW_MAX), (100_000, 10_000_000));
    let k = client(&h, &a, &d);
    assert_eq!(k.set_compact(WindowArg::Tokens(100_000)).unwrap(), Some(WindowArg::Tokens(100_000)));
    assert_eq!(k.set_compact(WindowArg::Tokens(10_000_000)).unwrap(), Some(WindowArg::Tokens(10_000_000)));
    assert_eq!(k.set_compact(WindowArg::Off).unwrap(), Some(WindowArg::Off));
    assert_eq!(k.clear_compact().unwrap(), None);
    assert_eq!(raw.call(&u, "POST", &cp(&a), &sess(&a), j(json!({"window": "off"}))).1, json!({"window": "off"}));
    assert_eq!(raw.call(&u, "DELETE", &cp(&a), &sess(&a), None).1, json!({"window": null}));
}

#[test]
fn client_compact_and_status() {
    let d = TempDir::new().unwrap();
    let (h, _c, _, _) = start(cfg(&d));
    let a = h.create_session(req()).unwrap();
    let k = client(&h, &a, &d);
    assert_eq!(k.set_compact(WindowArg::Tokens(600_000)).unwrap(), Some(WindowArg::Tokens(600_000)));
    let s = k.status().unwrap();
    assert_eq!(s.compact, Some(WindowArg::Tokens(600_000)));
    assert_eq!(s.effective_window, Some(600_000));
    assert_eq!((s.model.as_deref(), s.context_window, s.threshold), (Some("claude-opus-5-5"), Some(1_000_000), Some(600_000)));
    assert_eq!(k.clear_compact().unwrap(), None);
    let s = k.status().unwrap();
    assert_eq!(s.compact, None);
    assert_eq!(s.effective_window, Some(800_000));
    assert_eq!((s.model.as_deref(), s.context_window, s.threshold), (Some("claude-opus-5-5"), Some(1_000_000), Some(800_000)));
    assert_eq!(k.set_compact(WindowArg::Off).unwrap(), Some(WindowArg::Off));
    assert_eq!(k.status().unwrap().effective_window, None);
    assert_eq!(k.status().unwrap().threshold, None); // AU28: override `off`
}

#[test]
fn client_status_fields() {
    let d = TempDir::new().unwrap();
    let (h, c, _, _) = start(cfg(&d));
    let mut p = p1();
    p.prompt = Some("Prefer small commits.".into());
    let mut r = req();
    r.profile = Some(p);
    let g = h.create_session(r).unwrap();
    let k = client(&h, &g, &d);
    let s = k.status().unwrap();
    assert_eq!(s.session, g.session);
    assert_eq!(s.profile.as_deref(), Some("p1"));
    assert_eq!(s.work.as_deref(), Some("ENG-42"));
    assert_eq!((s.used_context, s.cache_read_share), (None, None));
    let state = h.session(&g.session).unwrap();
    state.add_usage(PromptUsage { input: 100, cache_read: 300, cache_write: 100 });
    state.set_used_context(500);
    c.advance(Duration::from_secs(3600));
    let s = k.status().unwrap();
    assert_eq!(s.used_context, Some(500));
    assert_eq!(s.cache_read_share, Some(0.6));
    assert_eq!(
        OffsetDateTime::parse(&s.expires_at, &Rfc3339).unwrap(),
        OffsetDateTime::parse(&g.expires_at, &Rfc3339).unwrap() + time::Duration::hours(1)
    );
}

#[test]
fn settings_patch_merges() {
    let d = TempDir::new().unwrap();
    let (h, _c, _, _) = start(cfg(&d));
    let raw = Raw::new();
    let u = h.url();
    let g = h.create_session(req()).unwrap();
    let k = client(&h, &g, &d);
    let off = SessionSettings { redaction_content: Some(false) };
    assert_eq!(k.set_settings(&off).unwrap(), off);
    let s = h.session(&g.session).unwrap();
    assert_eq!(s.settings().redaction_content, Some(false));
    assert!(!s.features().redaction.content);
    assert_eq!(k.set_settings(&SessionSettings::default()).unwrap(), off); // {} changes nothing
    assert_eq!(
        st(raw.call(&u, "PATCH", &sp_set(&g), &sess(&g), j(json!({"redaction_content": "no"})))),
        (400, json!("zygo_bad_request"))
    );
    let r = raw.call(&u, "PATCH", &sp_set(&g), &sess(&g), j(json!({"frob": true})));
    assert_eq!(r.1["error"]["message"], "unknown field `frob`");
    assert_eq!(st(r), (400, json!("zygo_bad_request")));
}

#[test]
fn settings_patch_refuses_team_set_key() {
    let d = TempDir::new().unwrap();
    let (h, _c, _, _) = start(team_cfg(&d));
    let raw = Raw::new();
    let u = h.url();
    let g = h.create_session(req()).unwrap();
    let r = raw.call(&u, "PATCH", &sp_set(&g), &sess(&g), j(json!({"redaction_content": false})));
    assert!(r.1["error"]["message"].as_str().unwrap().contains("features.redaction.content"), "{}", r.1);
    assert_eq!(st(r), (403, json!("zygo_setting_refused")));
    assert_eq!(h.session(&g.session).unwrap().settings().redaction_content, None);
}

#[test]
fn client_errors() {
    let d = TempDir::new().unwrap();
    let empty = TempDir::new().unwrap();
    let (h, _c, _, _) = start(cfg(&d));
    let g = h.create_session(req()).unwrap();
    assert!(matches!(SessionClient::from_session_dir(empty.path()), Err(ClientError::Session(_))));
    let k = client(&h, &g, &d);
    let dbg = format!("{k:?}");
    assert!(!dbg.contains(&g.token));
    assert!(dbg.contains("[redacted]"));

    let write_dir = |root: &TempDir, url: &str, token: &str| {
        let dir = root.path().join(&g.session);
        let sd = SessionDir::new(dir.clone());
        sd.create().unwrap();
        sd.write_token(token).unwrap();
        std::fs::write(sd.url(), url).unwrap();
        dir
    };
    let bad_root = TempDir::new().unwrap();
    let wrong = format!("zyg_s_{}", "0".repeat(43));
    let k_bad = SessionClient::from_session_dir(&write_dir(&bad_root, h.url().as_str(), &wrong)).unwrap();
    assert!(matches!(k_bad.status(), Err(ClientError::Reply { status: 401, ref code, .. }) if code == "zygo_token_invalid"));

    let silent = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap(); // never accepted from
    let silent_root = TempDir::new().unwrap();
    let silent_url = format!("http://{}/", silent.local_addr().unwrap());
    let k_silent = SessionClient::from_session_dir(&write_dir(&silent_root, &silent_url, &g.token))
        .unwrap()
        .timeout(Duration::from_millis(200));
    assert!(matches!(k_silent.status(), Err(ClientError::Unreachable(_))));
}

#[test]
fn not_found_and_body_too_large() {
    let d = TempDir::new().unwrap();
    let (h, _c, _, _) = start(cfg(&d));
    let raw = Raw::new();
    let u = h.url();
    let g = h.create_session(req()).unwrap();
    assert_eq!(st(raw.call(&u, "GET", "v1/zygo/nope", &[], None)), (404, json!("zygo_not_found")));
    let big = vec![b' '; (32 << 20) + 1];
    assert_eq!(st(raw.call(&u, "PATCH", &sp_set(&g), &sess(&g), Some(big))), (413, json!("zygo_body_too_large")));
}

// ---- round 2 (N10 review) ----

use std::io::{Read, Write};

/// Sends `head` on a raw TCP connection and returns the first reply line, or "" when none comes within `wait`.
fn raw_first_line(h: &ProxyHandle, head: &[u8], wait: Duration) -> String {
    let mut s = std::net::TcpStream::connect(("127.0.0.1", h.url().port().unwrap())).unwrap();
    s.write_all(head).unwrap();
    s.set_read_timeout(Some(wait)).unwrap();
    let mut b = [0u8; 256];
    match s.read(&mut b) {
        Ok(n) => String::from_utf8_lossy(&b[..n]).lines().next().unwrap_or("").to_string(),
        Err(_) => String::new(),
    }
}

/// §7.2 note: a request refused after authentication does not refresh the idle TTL or `expires_at`.
#[test]
fn refused_requests_do_not_refresh_ttl() {
    for case in ["bad_body", "bad_window", "too_large", "refused_setting"] {
        let d = TempDir::new().unwrap();
        let m = if case == "refused_setting" { team_cfg(&d) } else { cfg(&d) };
        let (h, c, _, _) = start(m);
        let raw = Raw::new();
        let u = h.url();
        let g = h.create_session(req()).unwrap();
        c.advance(Duration::from_secs(23 * 3600));
        let r = match case {
            "bad_body" => raw.call(&u, "PATCH", &sp_set(&g), &sess(&g), j(json!({"redaction_content": "no"}))),
            "bad_window" => raw.call(&u, "POST", &cp(&g), &sess(&g), j(json!({"window": 5}))),
            "too_large" => raw.call(&u, "PATCH", &sp_set(&g), &sess(&g), Some(vec![b' '; (32 << 20) + 1])),
            _ => raw.call(&u, "PATCH", &sp_set(&g), &sess(&g), j(json!({"redaction_content": false}))),
        };
        let want = match case {
            "too_large" => 413,
            "refused_setting" => 403,
            _ => 400,
        };
        assert_eq!(r.0, want, "{case}: {}", r.1);
        assert_eq!(h.session(&g.session).unwrap().status().expires_at, g.expires_at, "{case}");
        c.advance(Duration::from_secs(2 * 3600)); // 25 h after creation
        assert_eq!(st(raw.call(&u, "GET", &sp(&g), &sess(&g), None)), (401, json!("zygo_session_unknown")), "{case}");
    }
}

/// Controller decision (N10 review): shutdown closes open connections at once.
#[test]
fn shutdown_with_part_sent_request_returns() {
    let d = TempDir::new().unwrap();
    let (h, _c, _, _) = start(cfg(&d));
    let addr = ("127.0.0.1", h.url().port().unwrap());
    let mut head = std::net::TcpStream::connect(addr).unwrap();
    head.write_all(b"GET /healthz HTTP/1.1\r\nHost: x\r\n").unwrap();
    let mut body = std::net::TcpStream::connect(addr).unwrap();
    body.write_all(b"PATCH /v1/zygo/sessions/X/settings HTTP/1.1\r\nHost: x\r\ncontent-length: 1000\r\n\r\n{").unwrap();
    // The server has the connections once it answers on a third one.
    assert_eq!(raw_first_line(&h, b"GET /healthz HTTP/1.1\r\nHost: x\r\n\r\n", Duration::from_secs(10)), "HTTP/1.1 200 OK");
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        h.shutdown();
        let _ = tx.send(());
    });
    // Hang detector only: shutdown does not wait for the stalled clients.
    assert!(rx.recv_timeout(Duration::from_secs(10)).is_ok(), "shutdown hangs on a part-sent request");
    for s in [&mut head, &mut body] {
        s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let mut b = Vec::new();
        assert!(!matches!(s.read_to_end(&mut b), Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut), "connection left open");
    }
}

/// §7.3 note: an extractor rejection on `/v1/zygo/*` uses the error shape.
#[test]
fn bad_path_id_is_bad_request() {
    let d = TempDir::new().unwrap();
    let (h, _c, a, _) = start(cfg(&d));
    let raw = Raw::new();
    let u = h.url();
    let bearer = format!("Bearer {a}");
    let session_headers = [("x-api-key", "k"), ("x-zygo-session", "k")];
    for (m, p) in [
        ("GET", "v1/zygo/sessions/%FF/status"),
        ("POST", "v1/zygo/sessions/%FF/compact"),
        ("DELETE", "v1/zygo/sessions/%FF/compact"),
        ("PATCH", "v1/zygo/sessions/%FF/settings"),
    ] {
        assert_eq!(st(raw.call(&u, m, p, &session_headers, j(json!({})))), (400, json!("zygo_bad_request")), "{m} {p}");
    }
    for (m, p) in [("POST", "v1/zygo/sessions/%FF/heartbeat"), ("DELETE", "v1/zygo/sessions/%FF")] {
        assert_eq!(st(raw.call(&u, m, p, &[("authorization", &bearer)], None)), (400, json!("zygo_bad_request")), "{m} {p}");
    }
}

fn write_session_dir(root: &TempDir, name: &str, url: &str, token: &str) -> std::path::PathBuf {
    let dir = root.path().join(name);
    let sd = SessionDir::new(dir.clone());
    sd.create().unwrap();
    sd.write_token(token).unwrap();
    std::fs::write(sd.url(), url).unwrap();
    dir
}

/// A request that cannot be built is a session-directory error, not "unreachable" (exit 12).
#[test]
fn client_build_errors_are_session_errors() {
    let d = TempDir::new().unwrap();
    let (h, _c, _, _) = start(cfg(&d));
    let g = h.create_session(req()).unwrap();
    let root = TempDir::new().unwrap();
    let k = SessionClient::from_session_dir(&write_session_dir(&root, &g.session, h.url().as_str(), &format!("{}\nX", g.token)));
    let r = k.and_then(|k| k.status().map(|_| ()));
    assert!(matches!(r, Err(ClientError::Session(_))), "token with a newline: {r:?}");
    // Windows refuses a control character in a file name (os error 123): no such session directory can exist.
    if cfg!(windows) { return; }
    let k = SessionClient::from_session_dir(&write_session_dir(&root, &format!("{}\u{1}", g.session), h.url().as_str(), &g.token));
    let r = k.and_then(|k| k.status().map(|_| ()));
    assert!(matches!(r, Err(ClientError::Session(_))), "session id with a control character: {r:?}");
}

/// C8: serve mode is HTTPS on a non-loopback address, on both ends.
#[test]
fn client_refuses_plain_http_to_remote_host() {
    let root = TempDir::new().unwrap();
    let token = format!("zyg_s_{}", "0".repeat(43));
    for url in ["http://example.com/", "http://10.0.0.1:8080/", "http://[2001:db8::1]/", "ftp://127.0.0.1/"] {
        let r = SessionClient::from_session_dir(&write_session_dir(&root, &ulid(), url, &token));
        assert!(matches!(r, Err(ClientError::Session(_))), "{url}: {r:?}");
    }
    for url in ["https://example.com/", "http://127.0.0.1:1/", "http://127.1.2.3:1/", "http://[::1]:1/", "http://localhost:1/"] {
        assert!(SessionClient::from_session_dir(&write_session_dir(&root, &ulid(), url, &token)).is_ok(), "{url}");
    }
}

/// C8 (U1, V3): a user name or password in the proxy url is refused, and the text says so without printing it.
#[test]
fn client_refusal_names_userinfo_as_the_cause() {
    let root = TempDir::new().unwrap();
    let token = format!("zyg_s_{}", "0".repeat(43));
    let userinfo = ["n37user", "n37pass"].join(":");
    let url = format!("http://{userinfo}@127.0.0.1:1/");
    let e = SessionClient::from_session_dir(&write_session_dir(&root, &ulid(), &url, &token)).unwrap_err();
    assert_eq!(e.to_string(), "session directory: the proxy url holds a user name or password (C8)");
} // mutation: the loopback text for every refusal

/// The client never follows a redirect (the token stays with the proxy it was written for).
#[test]
fn client_does_not_follow_redirects() {
    let d = TempDir::new().unwrap();
    let (h, _c, _, _) = start(cfg(&d));
    let g = h.create_session(req()).unwrap();
    let target = h.url().join(&sp(&g)).unwrap();
    let l = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let url = format!("http://{}/", l.local_addr().unwrap());
    std::thread::spawn(move || {
        if let Ok((mut s, _)) = l.accept() {
            let mut buf = Vec::new();
            let mut b = [0u8; 1024];
            while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                match s.read(&mut b) {
                    Ok(0) | Err(_) => return,
                    Ok(n) => buf.extend_from_slice(&b[..n]),
                }
            }
            let reply = format!("HTTP/1.1 307 Temporary Redirect\r\nLocation: {target}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            let _ = s.write_all(reply.as_bytes());
        }
    });
    let root = TempDir::new().unwrap();
    let k = SessionClient::from_session_dir(&write_session_dir(&root, &g.session, &url, &g.token)).unwrap();
    let r = k.status();
    assert!(matches!(r, Err(ClientError::Reply { status: 307, .. })), "{r:?}");
}

/// §7.2 note (N10 review): expired sessions are removed on any session-API request.
#[test]
fn expired_sessions_swept_on_any_request() {
    let d = TempDir::new().unwrap();
    let (h, c, ta, _) = start(cfg(&d));
    let raw = Raw::new();
    let u = h.url();
    let old = h.create_session(req()).unwrap();
    let keep = h.create_session(req()).unwrap();
    c.advance(Duration::from_secs(23 * 3600));
    assert_eq!(raw.call(&u, "GET", &sp(&keep), &sess(&keep), None).0, 200);
    assert!(h.session(&old.session).is_some());
    c.advance(Duration::from_secs(2 * 3600));
    assert_eq!(raw.call(&u, "GET", &sp(&keep), &sess(&keep), None).0, 200);
    assert!(h.session(&old.session).is_none(), "status on another session");

    let bearer = format!("Bearer {ta}");
    let t_old = team_session(&raw, &u, &bearer);
    let t_keep = team_session(&raw, &u, &bearer);
    c.advance(Duration::from_secs(30 * 24 * 3600));
    let _ = raw.call(&u, "POST", &hb(&t_keep), &[("authorization", &bearer)], None);
    assert!(h.session(&t_old.session).is_none(), "heartbeat on another session");
    assert!(h.session(&keep.session).is_none());
}

/// Stage 1 runs before the body is read: an unauthenticated part-sent body gets its 401 at once.
#[test]
fn unauthenticated_request_refused_before_body() {
    let d = TempDir::new().unwrap();
    let (h, _c, _, _) = start(cfg(&d));
    let id = ulid();
    let head = format!("PATCH /v1/zygo/sessions/{id}/settings HTTP/1.1\r\nHost: x\r\ncontent-length: 1000\r\n\r\n{{");
    assert_eq!(raw_first_line(&h, head.as_bytes(), Duration::from_secs(10)), "HTTP/1.1 401 Unauthorized");
    let head = "POST /v1/zygo/sessions HTTP/1.1\r\nHost: x\r\ncontent-length: 1000\r\n\r\n{";
    assert_eq!(raw_first_line(&h, head.as_bytes(), Duration::from_secs(10)), "HTTP/1.1 401 Unauthorized");
}

/// §7.3 note: unknown fields are refused and named; a null setting is refused.
#[test]
fn unknown_fields_refused() {
    let d = TempDir::new().unwrap();
    let (h, _c, ta, _) = start(cfg(&d));
    let raw = Raw::new();
    let u = h.url();
    let g = h.create_session(req()).unwrap();
    let r = raw.call(&u, "POST", &cp(&g), &sess(&g), j(json!({"window": "off", "frob": 1})));
    assert_eq!(r.1["error"]["message"], "unknown field `frob`");
    assert_eq!(st(r), (400, json!("zygo_bad_request")));
    assert_eq!(h.session(&g.session).unwrap().compact(), None);
    let mut b = team_body();
    b["frob"] = json!(1);
    let r = raw.call(&u, "POST", "v1/zygo/sessions", &[("authorization", &format!("Bearer {ta}"))], j(b));
    assert_eq!(r.1["error"]["message"], "unknown field `frob`");
    assert_eq!(st(r), (400, json!("zygo_bad_request")));
    let r = raw.call(&u, "PATCH", &sp_set(&g), &sess(&g), j(json!({"redaction_content": null})));
    assert_eq!(st(r), (400, json!("zygo_bad_request")));
}

/// RFC 9110 §11.1: the auth scheme is case-insensitive.
#[test]
fn bearer_scheme_case_insensitive() {
    let d = TempDir::new().unwrap();
    let (h, _c, ta, _) = start(cfg(&d));
    let raw = Raw::new();
    let u = h.url();
    let g = h.create_session(req()).unwrap();
    for scheme in ["bearer", "BEARER", "Bearer"] {
        let a = format!("{scheme} {}", g.token);
        assert_eq!(raw.call(&u, "GET", &sp(&g), &[("authorization", &a), ("x-zygo-session", &g.session)], None).0, 200, "{scheme}");
        let r = raw.call(&u, "POST", "v1/zygo/sessions", &[("authorization", &format!("{scheme} {ta}"))], j(team_body()));
        assert_eq!(r.0, 200, "{scheme}: {}", r.1);
    }
}

/// Serve mode: HTTP creation validates the profile against the server's known plugins (given at start).
#[test]
fn http_create_uses_server_known_plugins() {
    let d = TempDir::new().unwrap();
    let ta = format!("team-{}", ulid());
    let team = TestTeam(vec![(ta.clone(), "alpha".into())]);
    let opts = StartOpts {
        team: Some(Arc::new(team)),
        known_plugins: ["m:p".to_string()].into_iter().collect(),
        ..Default::default()
    };
    let h = Proxy::start_local_with(Arc::new(cfg(&d)), opts).unwrap();
    let raw = Raw::new();
    let u = h.url();
    let bearer = format!("Bearer {ta}");
    let mut b = team_body();
    b["profile"] = json!({"name": "p1", "plugins": {"m:p": true}});
    let r = raw.call(&u, "POST", "v1/zygo/sessions", &[("authorization", &bearer)], j(b.clone()));
    assert_eq!(r.0, 200, "{}", r.1);
    assert_eq!(r.1["profile_warning"], Value::Null);
    let t: SessionGrant = serde_json::from_value(r.1).unwrap();
    assert_eq!(h.session(&t.session).unwrap().status().profile.as_deref(), Some("p1"));
    b["profile"]["plugins"] = json!({"m:q": true});
    let r = raw.call(&u, "POST", "v1/zygo/sessions", &[("authorization", &bearer)], j(b));
    assert!(r.1["profile_warning"].as_str().unwrap().contains("m:q"), "{}", r.1);
    // In-process creation keeps the request's own list.
    let mut q = req();
    q.profile = Some(Profile { plugins: BTreeMap::from([("m:q".to_string(), true)]), ..p1() });
    q.known_plugins = ["m:q".to_string()].into_iter().collect();
    assert_eq!(h.create_session(q).unwrap().profile_warning, None);
}

#[test]
fn status_default_keeps_literals_short() { // Review focus 11, H§17 note 1; H§7.2 `#[serde(default)]`
    let s = SessionStatus { model: Some("claude-opus-5-5".into()), ..Default::default() };
    assert_eq!(s.model.as_deref(), Some("claude-opus-5-5"));
    assert_eq!((s.session.as_str(), s.context_window, s.threshold, s.effective_window), ("", None, None, None));
    // A reply without the new keys parses; each new field is None.
    let old: SessionStatus = serde_json::from_value(json!({"session":"S1","profile":null,"work":null,"used_context":null,
        "effective_window":null,"compact":null,"cache_read_share":null,"expires_at":""})).unwrap();
    assert_eq!(old, SessionStatus { session: "S1".into(), ..Default::default() });
}

#[test]
fn status_reply_omits_absent_new_fields() { // H§7.2 `model`: in passthrough, absent until a request is served
    let d = TempDir::new().unwrap();
    let (h, _c, _, _) = start(cfg(&d));
    let raw = Raw::new();
    let mut r = req();
    r.passthrough = true;
    let a = h.create_session(r).unwrap();
    let (s, v, _) = raw.call(&h.url(), "GET", &sp(&a), &sess(&a), None);
    assert_eq!(s, 200);
    for k in ["model", "context_window", "threshold"] {
        assert!(v.get(k).is_none(), "{k} in {v}");
    }
    assert!(v.get("effective_window").is_some(), "an old field keeps its key: {v}");
    assert_eq!(client(&h, &a, &d).status().unwrap().model, None); // parsed with `#[serde(default)]`
    let b = h.create_session(req()).unwrap();
    let (_, v, _) = raw.call(&h.url(), "GET", &sp(&b), &sess(&b), None);
    assert_eq!((v["model"].clone(), v["context_window"].clone()), (json!("claude-opus-5-5"), json!(1000000)));
}

// ---- HUD-6: session spend and tokens ----

/// The reply keys HUD-6 adds (H§7.2).
const SPEND_KEYS: [&str; 8] =
    ["tokens_in", "tokens_out", "spend_usd", "spend_notional", "requests_total", "requests_priced", "passthrough", "team_session"];

fn tu(input: u64, output: u64, cache_read: u64, cache_write: u64) -> TokenUsage {
    TokenUsage { input, output, cache_read, cache_write, ..Default::default() }
}

#[test]
fn status_spend_tokens_and_counts() { // AU15, AU16, H§7.2, H§7.3 step 3
    let d = TempDir::new().unwrap();
    let (h, _c, _, _) = start(cfg(&d));
    let raw = Raw::new();
    let g = h.create_session(req()).unwrap();
    let k = client(&h, &g, &d);
    let s = k.status().unwrap();
    assert_eq!((s.tokens_in, s.tokens_out, s.spend_usd, s.requests_total, s.requests_priced), (Some(0), Some(0), Some(0.0), Some(0), Some(0)));
    assert_eq!((s.spend_notional, s.passthrough, s.team_session), (Some(false), Some(false), Some(false)));
    let state = h.session(&g.session).unwrap();
    state.add_request(&tu(100, 7, 300, 100), Some(0.25)); // priced
    state.add_request(&tu(1, 2, 0, 0), None); // tokens, no price
    state.add_request(&tu(0, 0, 0, 0), Some(0.0)); // no token: in neither count
    state.add_request(&tu(0, 0, 0, 0), None);
    state.add_request(&tu(0, 5, 0, 0), Some(0.5)); // output only is a token
    let s = k.status().unwrap();
    assert_eq!((s.tokens_in, s.tokens_out), (Some(501), Some(14))); // AU16: in = input + cache read + cache write
    assert_eq!((s.spend_usd, s.requests_total, s.requests_priced), (Some(0.75), Some(3), Some(2)));
    assert_eq!(s.cache_read_share, Some(300.0 / 501.0)); // the §10.7 sums come from the same call
    let (_, v, _) = raw.call(&h.url(), "GET", &sp(&g), &sess(&g), None);
    for key in SPEND_KEYS {
        assert!(v.get(key).is_some(), "{key} in {v}");
    }
    assert_eq!((v["tokens_in"].clone(), v["tokens_out"].clone(), v["spend_usd"].clone()), (json!(501), json!(14), json!(0.75)));
    assert_eq!((v["requests_total"].clone(), v["requests_priced"].clone()), (json!(3), json!(2)));
    assert_eq!((v["spend_notional"].clone(), v["passthrough"].clone(), v["team_session"].clone()), (json!(false), json!(false), json!(false)));
    // A passthrough session: the spend is notional.
    let mut r = req();
    r.passthrough = true;
    let p = h.create_session(r).unwrap();
    let s = client(&h, &p, &d).status().unwrap();
    assert_eq!((s.spend_usd, s.spend_notional, s.passthrough, s.team_session), (Some(0.0), Some(true), Some(true), Some(false)));
    // A reply of an old server: every new field is None.
    let old: SessionStatus = serde_json::from_value(json!({"session":"S1","profile":null,"work":null,"used_context":null,
        "effective_window":null,"compact":null,"cache_read_share":null,"expires_at":""})).unwrap();
    assert_eq!((old.tokens_in, old.tokens_out, old.spend_usd, old.spend_notional), (None, None, None, None));
    assert_eq!((old.requests_total, old.requests_priced, old.passthrough, old.team_session), (None, None, None, None));
    let none = serde_json::to_value(SessionStatus::default()).unwrap();
    for key in SPEND_KEYS {
        assert!(none.get(key).is_none(), "{key} in {none}");
    }
}

#[test]
fn status_team_session_sends_no_configured_price() { // AU13, AU14, H§9 row §7.2
    let d = TempDir::new().unwrap();
    let (h, _c, ta, _) = start(cfg(&d));
    let raw = Raw::new();
    let u = h.url();
    let t = team_session(&raw, &u, &format!("Bearer {ta}"));
    let state = h.session(&t.session).unwrap();
    assert_eq!(state.team_label.as_deref(), Some("alpha"));
    // A cost at the server's configured prices never reaches the reply, also when a caller hands one in.
    state.add_request(&tu(1000, 10, 0, 0), Some(1234.5));
    let (s, v, bytes) = raw.call(&u, "GET", &sp(&t), &sess(&t), None);
    assert_eq!(s, 200, "{v}");
    assert_eq!((v["team_session"].clone(), v["spend_notional"].clone(), v["passthrough"].clone()), (json!(true), json!(true), json!(false)));
    assert_eq!((v["tokens_in"].clone(), v["tokens_out"].clone(), v["requests_total"].clone()), (json!(1000), json!(10), json!(1)));
    for key in ["spend_usd", "requests_priced"] {
        assert!(v.get(key).is_none(), "{key} in {v}");
    }
    let text = String::from_utf8(bytes).unwrap();
    assert!(!text.contains("alpha"), "{text}"); // AU13: the flag, never the label
    assert!(v.as_object().unwrap().values().all(|x| x.as_f64() != Some(1234.5)), "{text}");
}

fn sse(kind: &str, usage: Value) -> String {
    let v = if kind == "message_start" { json!({"type": kind, "message": {"usage": usage}}) } else { json!({"type": kind, "usage": usage}) };
    format!("event: {kind}\ndata: {v}\n\n")
}

#[test]
fn tee_reads_the_cache_write_split() { // AU33, H§9 row §14 usage recording
    let whole = |body: Value| {
        let mut t = Tee::new(false);
        t.push(body.to_string().into(), &mut |_| {});
        t.finish(&mut |_| {});
        (t.usage(), t.cache_split())
    };
    let u = Usage { input: 5, output: 6, cache_read: 7, cache_write: 300 };
    // Not streamed: the split is read; `Usage` keeps its four fields and the total.
    let with = json!({"usage":{"input_tokens":5,"output_tokens":6,"cache_read_input_tokens":7,"cache_creation_input_tokens":300,
        "cache_creation":{"ephemeral_5m_input_tokens":200,"ephemeral_1h_input_tokens":100}}});
    assert_eq!(whole(with), (u, Some(CacheSplit { m5: 200, h1: 100 })));
    // No split: None, not zeros.
    let without = json!({"usage":{"input_tokens":5,"output_tokens":6,"cache_read_input_tokens":7,"cache_creation_input_tokens":300}});
    assert_eq!(whole(without), (u, None));
    let empty = json!({"usage":{"input_tokens":5,"output_tokens":6,"cache_read_input_tokens":7,"cache_creation_input_tokens":300,"cache_creation":{}}});
    assert_eq!(whole(empty), (u, None));
    let half = json!({"usage":{"cache_creation_input_tokens":9,"cache_creation":{"ephemeral_1h_input_tokens":9}}});
    assert_eq!(whole(half).1, Some(CacheSplit { m5: 0, h1: 9 }));
    assert_eq!(Tee::new(true).cache_split(), None);
    // Streamed: `message_start` sets it; `message_delta` raises each part (cumulative usage), and never lowers it.
    let streamed = |events: &[String], armed: bool| {
        let mut t = Tee::new(true);
        if armed {
            t.arm_line_mode();
        }
        for e in events {
            // Two chunks per event: a line is read whole.
            let (a, b) = e.split_at(e.len() / 2);
            t.push(a.to_string().into(), &mut |_| {});
            t.push(b.to_string().into(), &mut |_| {});
        }
        t.finish(&mut |_| {});
        (t.usage(), t.cache_split())
    };
    let start = sse("message_start", json!({"input_tokens":5,"output_tokens":1,"cache_read_input_tokens":7,"cache_creation_input_tokens":300,
        "cache_creation":{"ephemeral_5m_input_tokens":200,"ephemeral_1h_input_tokens":100}}));
    let delta = sse("message_delta", json!({"output_tokens":6,"cache_creation_input_tokens":300,
        "cache_creation":{"ephemeral_5m_input_tokens":150,"ephemeral_1h_input_tokens":120}}));
    let plain = sse("message_delta", json!({"output_tokens":6}));
    for armed in [false, true] {
        assert_eq!(streamed(&[start.clone(), plain.clone()], armed), (u, Some(CacheSplit { m5: 200, h1: 100 })), "armed {armed}");
        assert_eq!(streamed(&[start.clone(), delta.clone()], armed), (u, Some(CacheSplit { m5: 200, h1: 120 })), "armed {armed}");
        assert_eq!(streamed(std::slice::from_ref(&plain), armed).1, None, "armed {armed}");
        assert_eq!(streamed(std::slice::from_ref(&delta), armed).1, Some(CacheSplit { m5: 150, h1: 120 }), "armed {armed}");
    }
}

// ---- HUD-8: H§14 #8 and #12, proxy parts ----

#[test]
fn h14_08_status_threshold_follows_the_override_without_a_request() { // H§14 #8 (proxy part): AU28, S3
    let d = TempDir::new().unwrap();
    let (h, _c, _, _) = start(cfg(&d));
    let g = h.create_session(req()).unwrap();
    let k = client(&h, &g, &d);
    let windows = |s: SessionStatus| (s.model, s.context_window, s.effective_window, s.threshold);
    let opus = || Some("claude-opus-5-5".to_string());
    // The row's soft window (800,000) below the model window (1,000,000).
    assert_eq!(windows(k.status().unwrap()), (opus(), Some(1_000_000), Some(800_000), Some(800_000)));
    // S3: `zygo compact 150k` with no new request: the next status has `threshold` 150,000.
    k.set_compact(WindowArg::Tokens(150_000)).unwrap();
    assert_eq!(windows(k.status().unwrap()), (opus(), Some(1_000_000), Some(150_000), Some(150_000)));
    // The override `off`: no `threshold`, and `compact` says `off` (the HUD shows `off` from it).
    k.set_compact(WindowArg::Off).unwrap();
    let s = k.status().unwrap();
    assert_eq!((s.compact, s.threshold, s.context_window), (Some(WindowArg::Off), None, Some(1_000_000)));
    // Passthrough before the first request: no `model` and no window in the reply.
    let mut r = req();
    r.passthrough = true;
    let p = h.create_session(r).unwrap();
    assert_eq!(windows(client(&h, &p, &d).status().unwrap()), (None, None, None, None));
}

#[test]
fn h14_12_tee_split_reaches_the_price_at_two_rates() { // H§14 #12 (proxy part): AU33
    let d = TempDir::new().unwrap();
    let prices = cfg(&d).config.catalog.get("claude-opus-5-5").unwrap().provider[0].price_usd_per_mtok.clone().unwrap();
    let usage_of = |body: Value| {
        let mut t = Tee::new(false);
        t.push(body.to_string().into(), &mut |_| {});
        t.finish(&mut |_| {});
        let (u, split) = (t.usage(), t.cache_split());
        // As `Pending::drop` builds the usage that it prices (`forward.rs`).
        TokenUsage {
            input: u.input,
            output: u.output,
            cache_read: u.cache_read,
            cache_write: u.cache_write,
            cache_write_5m: split.map(|s| s.m5),
            cache_write_1h: split.map(|s| s.h1),
        }
    };
    // 1,000 5-minute and 1,000 1-hour cache-write tokens: (1,000 × cache_write_5m + 1,000 × cache_write) ÷ 10⁶.
    let split = usage_of(json!({"usage":{"cache_creation_input_tokens":2000,
        "cache_creation":{"ephemeral_5m_input_tokens":1000,"ephemeral_1h_input_tokens":1000}}}));
    assert_eq!((split.cache_write, split.cache_write_5m, split.cache_write_1h), (2000, Some(1000), Some(1000)));
    let m5 = prices.cache_write_5m.expect("the built-in Anthropic rows have the 5-minute rate");
    let want = (1000.0 * m5 + 1000.0 * prices.cache_write) / 1e6;
    assert!(m5 < prices.cache_write, "{m5} {}", prices.cache_write);
    assert!((prices.cost(&split).unwrap() - want).abs() < 1e-9);
    // Without the split all 2,000 use `cache_write`.
    let whole = usage_of(json!({"usage":{"cache_creation_input_tokens":2000}}));
    assert_eq!((whole.cache_write_5m, whole.cache_write_1h), (None, None));
    assert!((prices.cost(&whole).unwrap() - 2000.0 * prices.cache_write / 1e6).abs() < 1e-9);
    // A cost that is not finite is not priced, so the session does not count the request as priced.
    let g_d = TempDir::new().unwrap();
    let (h, _c, _, _) = start(cfg(&g_d));
    let g = h.create_session(req()).unwrap();
    let state = h.session(&g.session).unwrap();
    let broken = zygo_config::Prices { input: f64::MAX, output: f64::MAX, ..Default::default() };
    let usage = tu(10, 10, 0, 0);
    assert_eq!(broken.cost(&usage), None);
    state.add_request(&usage, broken.cost(&usage));
    let s = client(&h, &g, &g_d).status().unwrap();
    assert_eq!((s.spend_usd, s.requests_total, s.requests_priced), (Some(0.0), Some(1), Some(0)));
}

// ---- HUD-9: H§14 #13 and #16, proxy parts ----

/// The usage of the reply of each fake provider, by the first path segment of its base URL:
/// (input, output, cache read, cache write).
const ROUTE_USAGE: [(&str, [u64; 4]); 4] =
    [("/pa/", [100, 10, 1000, 300]), ("/pb/", [7, 3, 40, 0]), ("/free/", [50, 5, 20, 0]), ("/zero/", [0, 0, 0, 0])];

/// The price rows of `p-pa` and `p-pb` (USD per million tokens): (input, output, cache read, cache write).
const ROUTE_PRICES: [(&str, [f64; 4]); 2] = [("pa", [4.0, 20.0, 0.2, 8.0]), ("pb", [1.0, 5.0, 0.1, 2.0])];

/// `usage` at `prices`, as H§7.3 step 1 with no tier and no cache-write split.
fn route_cost(usage: [u64; 4], prices: [f64; 4]) -> f64 {
    usage.iter().zip(prices).map(|(t, p)| *t as f64 * p).sum::<f64>() / 1e6
}

/// A fake upstream on 127.0.0.1 with its own runtime thread: each request gets 200 and a JSON message with the
/// `ROUTE_USAGE` of its path. It stops when this value drops.
struct RoutedUpstream {
    addr: std::net::SocketAddr,
    _stop: tokio::sync::oneshot::Sender<()>,
}

fn routed_upstream() -> RoutedUpstream {
    let l = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    l.set_nonblocking(true).unwrap();
    let addr = l.local_addr().unwrap();
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        rt.block_on(async move {
            let l = tokio::net::TcpListener::from_std(l).unwrap();
            let app = axum::Router::new().fallback(|req: axum::extract::Request| async move {
                let path = req.uri().path().to_string();
                let _ = axum::body::to_bytes(req.into_body(), usize::MAX).await;
                let [input, output, cache_read, cache_write] =
                    ROUTE_USAGE.iter().find(|(p, _)| path.starts_with(p)).map(|(_, u)| *u).expect("a path of ROUTE_USAGE");
                let body = json!({"id":"m","type":"message","role":"assistant","content":[],"usage":{"input_tokens":input,
                    "output_tokens":output,"cache_read_input_tokens":cache_read,"cache_creation_input_tokens":cache_write}});
                axum::response::Response::builder()
                    .status(200)
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(body.to_string()))
                    .unwrap()
            });
            tokio::select! {
                _ = async { axum::serve(l, app).await } => {}
                _ = rx => {}
            }
        });
    });
    RoutedUpstream { addr, _stop: tx }
}

/// A local proxy with a usage database in `d` and four catalog models on the fake upstream: `test-pa` (provider `p-pa`)
/// and `test-pb` (`p-pb`) with the `ROUTE_PRICES`, `test-free` (`p-free`) with no price, and `test-zero` (`p-zero`, the
/// `pa` prices) whose reply has no token. No provider has a key, so none is sent.
fn start_routed(d: &TempDir) -> (RoutedUpstream, ProxyHandle) {
    let up = routed_upstream();
    let model = |name: &str, prices: Option<[f64; 4]>| {
        let prices = prices.map_or(String::new(), |[i, o, r, w]| {
            format!("[model.provider.price_usd_per_mtok]\ninput = {i:?}\noutput = {o:?}\ncache_read = {r:?}\ncache_write = {w:?}\n")
        });
        format!(
            "[[model]]\nid = \"test-{name}\"\n[[model.provider]]\nname = \"p-{name}\"\nbase_url = \"http://{}/{name}\"\ndialects = [\"anthropic\"]\nupstream_model = \"up-{name}\"\n{prices}\n",
            up.addr
        )
    };
    let [(_, pa), (_, pb)] = ROUTE_PRICES;
    let text = [model("pa", Some(pa)), model("pb", Some(pb)), model("free", None), model("zero", Some(pa))].concat();
    std::fs::write(d.path().join("config.toml"), text).unwrap();
    let cfg = load(&ctx(d, None)).unwrap();
    let opts = StartOpts { clock: ManualClock::new(), usage_db: Some(d.path().join("data/usage.db")), ..Default::default() };
    (up, Proxy::start_local_with(Arc::new(cfg), opts).unwrap())
}

/// One messages request of session `g` for catalog model `model`; the HTTP status.
fn post_model(raw: &Raw, h: &ProxyHandle, g: &SessionGrant, model: Value) -> u16 {
    let body = json!({"model": model, "max_tokens": 8, "messages": [{"role": "user", "content": "hi"}]});
    raw.call(&h.url(), "POST", "anthropic/v1/messages", &sess(g), j(body)).0
}

/// The status reply of `k` once the proxy handled `n` usage rows (all sessions).
fn status_after(h: &ProxyHandle, k: &SessionClient, n: u64) -> SessionStatus {
    assert!(h.usage_written(n, Duration::from_secs(10)), "usage rows not written");
    k.status().unwrap()
}

fn usage_rows(d: &TempDir, g: &SessionGrant) -> Vec<RequestRow> {
    Store::open(&d.path().join("data/usage.db")).unwrap().requests_for_session(&g.session).unwrap()
}

/// (spend_usd, requests_total, requests_priced) of a status reply.
fn spend_counts(s: &SessionStatus) -> (Option<f64>, Option<u64>, Option<u64>) {
    (s.spend_usd, s.requests_total, s.requests_priced)
}

#[test]
fn h14_13_spend_is_the_sum_of_two_priced_rows() { // H§14 #13 (proxy part): AU15, AU16, H§7.3 step 3
    let d = TempDir::new().unwrap();
    let (_up, h) = start_routed(&d);
    let raw = Raw::new();
    let g = h.create_session(req()).unwrap();
    let k = client(&h, &g, &d);
    assert_eq!(post_model(&raw, &h, &g, json!("test-pa")), 200);
    assert_eq!(post_model(&raw, &h, &g, json!("test-pb")), 200);
    let s = status_after(&h, &k, 2);
    // The two rows: one per provider, each with the cost of its own price row.
    let rows = usage_rows(&d, &g);
    assert_eq!(rows.iter().map(|r| (r.provider.as_str(), r.status)).collect::<Vec<_>>(), [("p-pa", 200), ("p-pb", 200)]);
    let [(_, ua), (_, ub), ..] = ROUTE_USAGE;
    let [(_, pa), (_, pb)] = ROUTE_PRICES;
    for (row, want) in rows.iter().zip([route_cost(ua, pa), route_cost(ub, pb)]) {
        assert!(want > 0.0 && (row.estimated_usd - want).abs() < 1e-12, "{}: {} != {want}", row.provider, row.estimated_usd);
    }
    let sum: f64 = rows.iter().map(|r| r.estimated_usd).sum();
    // The session holds exactly the sum of the two row costs; the reply carries it (a JSON number: 1e-12 is its bound).
    assert_eq!(h.session(&g.session).unwrap().status().spend_usd, Some(sum));
    assert!((s.spend_usd.unwrap() - sum).abs() < 1e-12, "{:?} != {sum}", s.spend_usd);
    assert_eq!((s.requests_total, s.requests_priced, s.spend_notional), (Some(2), Some(2), Some(false)));
    // tokens_in = Σ(input + cache read + cache write) and tokens_out = Σ output, from the rows and from the fixture.
    let tokens_in: u64 = rows.iter().map(|r| r.input_tokens + r.cache_read_tokens + r.cache_write_tokens).sum();
    let tokens_out: u64 = rows.iter().map(|r| r.output_tokens).sum();
    assert_eq!((tokens_in, tokens_out), ((100 + 1000 + 300) + (7 + 40), 10 + 3));
    assert_eq!((s.tokens_in, s.tokens_out), (Some(tokens_in), Some(tokens_out)));
}

#[test]
fn h14_16_unpriced_and_zero_token_requests_in_the_counts() { // H§14 #16 (proxy part): AU15, H§7.2
    let d = TempDir::new().unwrap();
    let (_up, h) = start_routed(&d);
    let raw = Raw::new();
    let [(_, ua), ..] = ROUTE_USAGE;
    let [(_, pa), _] = ROUTE_PRICES;
    // No request: `requests_total = 0` and `spend_usd = 0.0`.
    let none = h.create_session(req()).unwrap();
    let k_none = client(&h, &none, &d);
    assert_eq!(spend_counts(&k_none.status().unwrap()), (Some(0.0), Some(0), Some(0)));
    // Two requests, one on the provider with no price: 2 and 1; the spend is the priced row alone.
    let two = h.create_session(req()).unwrap();
    let k_two = client(&h, &two, &d);
    assert_eq!(post_model(&raw, &h, &two, json!("test-pa")), 200);
    assert_eq!(post_model(&raw, &h, &two, json!("test-free")), 200);
    let s = status_after(&h, &k_two, 2);
    let rows = usage_rows(&d, &two);
    assert_eq!(rows.iter().map(|r| r.provider.as_str()).collect::<Vec<_>>(), ["p-pa", "p-free"]);
    assert!((rows[0].estimated_usd - route_cost(ua, pa)).abs() < 1e-12, "{}", rows[0].estimated_usd);
    assert_eq!(rows[1].estimated_usd, 0.0); // no price: the row costs 0
    assert_eq!((s.requests_total, s.requests_priced), (Some(2), Some(1)));
    assert!((s.spend_usd.unwrap() - rows[0].estimated_usd).abs() < 1e-12, "{:?}", s.spend_usd);
    assert_eq!((s.tokens_in, s.tokens_out), (Some(1400 + 70), Some(10 + 5))); // the unpriced request's tokens count
    // A request with zero tokens counts in neither number: a refusal (400, no upstream call), then a reply with no
    // token on a priced provider. Each writes a usage row.
    assert_eq!(post_model(&raw, &h, &two, json!(5)), 400);
    assert_eq!(post_model(&raw, &h, &two, json!("test-zero")), 200);
    let after = status_after(&h, &k_two, 4);
    assert_eq!(usage_rows(&d, &two).iter().map(|r| (r.status, r.input_tokens + r.output_tokens)).collect::<Vec<_>>(),
               [(200, 110), (200, 55), (400, 0), (200, 0)]);
    assert_eq!((after.requests_total, after.requests_priced), (Some(2), Some(1)));
    assert_eq!((after.spend_usd, after.tokens_in, after.tokens_out), (s.spend_usd, s.tokens_in, s.tokens_out));
    // One request, on the provider with no price: 1 and 0, spend 0.0.
    let one = h.create_session(req()).unwrap();
    let k_one = client(&h, &one, &d);
    assert_eq!(post_model(&raw, &h, &one, json!("test-free")), 200);
    assert_eq!(spend_counts(&status_after(&h, &k_one, 5)), (Some(0.0), Some(1), Some(0)));
    // Only zero-token requests: as no request.
    assert_eq!(post_model(&raw, &h, &none, json!(5)), 400);
    assert_eq!(post_model(&raw, &h, &none, json!("test-zero")), 200);
    let s = status_after(&h, &k_none, 7);
    assert_eq!(spend_counts(&s), (Some(0.0), Some(0), Some(0)));
    assert_eq!((s.tokens_in, s.tokens_out), (Some(0), Some(0)));
}
