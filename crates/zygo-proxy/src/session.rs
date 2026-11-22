//! Session state (§7.2): creation, token, idle TTL, liveness, overrides (§10.2.4, §17.1).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use http::StatusCode;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use zygo_config::profile::{validate, Profile};
use zygo_config::{CustomRole, Dialect, Features, GuardsConfig, Merged, Role, TokenUsage};
use zygo_core::ids::ulid;
use zygo_core::Agent;
use zygo_redact::act_lite_store::{StoredValue, MIN_VALUE_BYTES};
use zygo_redact::cache::SESSION_CAP_BYTES;
use zygo_redact::{compile, compile_with_store, CachePool, Compiled, SessionCache};

use crate::client::SessionStatus;
use crate::errors::ZygoError;
use crate::window::{self, RequestWindows};

pub const IDLE_TTL: Duration = Duration::from_secs(24 * 60 * 60); // §7.2
pub const SUSPEND_AFTER: Duration = Duration::from_secs(5 * 60); // §7.2
pub const WINDOW_MIN: u64 = 100_000; // §10.2.4
pub const WINDOW_MAX: u64 = 10_000_000; // §10.2.4
pub const REDACTION_NOTE: &str = "act-lite store values not used for redaction: their pattern does not build";

/// The longest stored model name in bytes (author's call, HUD-5 review): a passthrough name is client text.
pub(crate) const MODEL_NAME_MAX: usize = 128;

/// The last main-loop request (AU29): its model (`None` for an empty name) and, when the serving config had the
/// served row, that row's windows.
struct MainLoop {
    model: Option<String>,
    windows: Option<RequestWindows>,
}

/// Team-token check for serve mode (§7.2); N16 supplies the real one.
pub trait TeamAuth: Send + Sync {
    fn label_for(&self, token: &str) -> Option<String>;
}

pub trait Clock: Send + Sync {
    fn now(&self) -> Instant;
    fn utc(&self) -> OffsetDateTime;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
    fn utc(&self) -> OffsetDateTime {
        OffsetDateTime::now_utc()
    }
}

/// Test clock: both readings are the start plus the advanced offset.
#[doc(hidden)]
pub struct ManualClock {
    start: Instant,
    start_utc: OffsetDateTime,
    offset: Mutex<Duration>,
}

impl ManualClock {
    pub fn new() -> Arc<ManualClock> {
        Arc::new(ManualClock { start: Instant::now(), start_utc: OffsetDateTime::now_utc(), offset: Mutex::new(Duration::ZERO) })
    }
    pub fn advance(&self, d: Duration) {
        *lock(&self.offset) += d;
    }
    fn offset(&self) -> Duration {
        *lock(&self.offset)
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Instant {
        self.start + self.offset()
    }
    fn utc(&self) -> OffsetDateTime {
        self.start_utc + self.offset()
    }
}

/// A poisoned lock still holds consistent state here (every critical section is a plain field update).
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// JSON: a number, or the string "off".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowArg {
    Tokens(u64),
    Off,
}

impl Serialize for WindowArg {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            WindowArg::Tokens(n) => s.serialize_u64(*n),
            WindowArg::Off => s.serialize_str("off"),
        }
    }
}

impl<'de> Deserialize<'de> for WindowArg {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Wire {
            Tokens(u64),
            Word(String),
        }
        match Wire::deserialize(d)? {
            Wire::Tokens(n) => Ok(WindowArg::Tokens(n)),
            Wire::Word(w) if w == "off" => Ok(WindowArg::Off),
            Wire::Word(_) => Err(serde::de::Error::custom("window must be a number or \"off\"")),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redaction_content: Option<bool>,
}

/// Over HTTP an unknown field is refused (§7.3 note); the skipped fields count as unknown there.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionRequest {
    pub agent: Agent,
    pub agent_version: String,
    pub profile: Option<Profile>,
    pub passthrough: bool,
    pub repo: Option<String>,
    pub work: Option<String>,
    /// §16.1 A1: local mode only, in-process, never logged.
    #[serde(skip)]
    pub stored_values: Vec<StoredValue>,
    /// `<marketplace>:<plugin>` names the serving config knows (profile validation, §11.3).
    /// HTTP creation replaces it with the server's own set (`StartOpts::known_plugins`).
    #[serde(skip)]
    pub known_plugins: BTreeSet<String>,
}

/// §7.2 "the session's view with the profile applied".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionConfig {
    pub main_role: String,
    /// Bundle roles minus `disabled`; `model` = the profile override when set.
    pub roles: BTreeMap<String, Role>,
    pub custom_roles: BTreeMap<String, CustomRole>,
    /// Catalog ids served on the agent's dialect.
    pub models: Vec<String>,
    pub guards: GuardsConfig,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SessionGrant {
    pub session: String,
    pub token: String,
    pub config: SessionConfig,
    pub expires_at: String,
    pub profile_warning: Option<String>,
    pub redaction_note: Option<String>,
    /// `USAGE_OFF_NOTE` when the usage database did not open.
    #[serde(default)]
    pub usage_note: Option<String>,
}

impl fmt::Debug for SessionGrant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SessionGrant")
            .field("session", &self.session)
            .field("token", &"[redacted]")
            .field("config", &self.config)
            .field("expires_at", &self.expires_at)
            .field("profile_warning", &self.profile_warning)
            .field("redaction_note", &self.redaction_note)
            .field("usage_note", &self.usage_note)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PromptUsage {
    pub input: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

/// The session's running sums for the status reply (AU5, AU15, AU16).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Spend {
    tokens_in: u64,
    tokens_out: u64,
    usd: f64,
    requests_total: u64,
    requests_priced: u64,
}

pub struct Redaction {
    pub compiled: Compiled,
    pub secret_cache: Mutex<SessionCache>,
    pub content_cache: Mutex<SessionCache>,
}

struct Live {
    last_seen: Instant,
    last_seen_utc: OffsetDateTime,
    last_heartbeat: Instant,
    compact: Option<WindowArg>,
    settings: SessionSettings,
    usage: PromptUsage,
    spend: Spend,
    used_context: Option<u64>,
    main_loop: Option<MainLoop>,
}

pub struct SessionState {
    pub id: String,
    pub agent: Agent,
    pub agent_version: String,
    pub profile: Option<Profile>,
    pub passthrough: bool,
    pub repo: Option<String>,
    pub work: Option<String>,
    pub team_label: Option<String>,
    /// The serving config snapshot at creation.
    pub cfg: Arc<Merged>,
    pub config: SessionConfig,
    pub redaction: Redaction,
    /// Model id → provider name (§10.4).
    pub pins: Mutex<BTreeMap<String, String>>,
    token_sha256: [u8; 32],
    profile_features: Features,
    usage_note: Option<String>,
    live: Mutex<Live>,
}

fn rfc3339(t: OffsetDateTime) -> String {
    t.format(&Rfc3339).expect("a UTC time formats as RFC 3339")
}

impl SessionState {
    /// The profile's features, then the session's `redaction_content` setting.
    pub fn features(&self) -> Features {
        let mut f = self.profile_features.clone();
        if let Some(c) = lock(&self.live).settings.redaction_content {
            f.redaction.content = c;
        }
        f
    }
    pub fn compact(&self) -> Option<WindowArg> {
        lock(&self.live).compact
    }
    pub fn settings(&self) -> SessionSettings {
        lock(&self.live).settings.clone()
    }
    /// Sums for the cache share (§10.7).
    pub fn add_usage(&self, u: PromptUsage) {
        Self::add_prompt(&mut lock(&self.live), u);
    }
    fn add_prompt(l: &mut Live, u: PromptUsage) {
        l.usage.input = l.usage.input.saturating_add(u.input);
        l.usage.cache_read = l.usage.cache_read.saturating_add(u.cache_read);
        l.usage.cache_write = l.usage.cache_write.saturating_add(u.cache_write);
    }
    /// One request's tokens and its `Prices::cost` result (`None`: not priced), under one lock (H§7.3 step 3): the
    /// `add_usage` sums, tokens in (input + cache read + cache write) and out (AU16), and, for a request with at
    /// least one token, the counts and the spend (AU15). The split parts of `u` are not read: `cache_write` is the
    /// total.
    pub fn add_request(&self, u: &TokenUsage, cost: Option<f64>) {
        let prompt = u.input.saturating_add(u.cache_read).saturating_add(u.cache_write);
        let mut l = lock(&self.live);
        Self::add_prompt(&mut l, PromptUsage { input: u.input, cache_read: u.cache_read, cache_write: u.cache_write });
        let s = &mut l.spend;
        s.tokens_in = s.tokens_in.saturating_add(prompt);
        s.tokens_out = s.tokens_out.saturating_add(u.output);
        if prompt == 0 && u.output == 0 {
            return;
        }
        s.requests_total += 1;
        if let Some(c) = cost {
            s.requests_priced += 1;
            s.usd += c;
        }
    }
    pub fn set_used_context(&self, prompt: u64) {
        lock(&self.live).used_context = Some(prompt);
    }
    /// The last main-loop request (AU29), under one lock; the name is cut to `MODEL_NAME_MAX` bytes.
    pub(crate) fn set_main_loop(&self, prompt: u64, model: &str, windows: Option<RequestWindows>) {
        let model = (!model.is_empty()).then(|| model[..model.floor_char_boundary(MODEL_NAME_MAX)].to_string());
        let mut l = lock(&self.live);
        l.used_context = Some(prompt);
        l.main_loop = Some(MainLoop { model, windows });
    }
    pub fn status(&self) -> SessionStatus {
        let l = lock(&self.live);
        let (model, w) = match &l.main_loop {
            Some(m) => (m.model.clone(), m.windows.clone()),
            None if self.passthrough => (None, None),
            None => {
                let r = self.config.roles.get(&self.config.main_role);
                let row = r.and_then(|r| self.cfg.config.catalog.get(&r.model));
                (r.map(|r| r.model.clone()), row.map(|row| RequestWindows::of(row, window::agent_window(row))))
            }
        };
        let context_window = w.as_ref().and_then(|w| w.context_window);
        let (threshold, effective_window) = w.map_or((None, None), |w| {
            let row = w.view();
            let s = context_window.and_then(|cw| window::safety(cw, w.agent_window));
            (window::soft_threshold(l.compact, &row, s), window::effective_for(l.compact, &row, w.agent_window))
        });
        let u = l.usage;
        let total = u.input as f64 + u.cache_read as f64 + u.cache_write as f64;
        // A team session's spend is not sent here: what the session sums is priced with the server's configured
        // prices, and no configured price leaves a server (AU14, H§9 row §7.2).
        let team = self.team_label.is_some();
        SessionStatus {
            session: self.id.clone(),
            profile: self.profile.as_ref().map(|p| p.name.clone()),
            work: self.work.clone(),
            used_context: l.used_context,
            effective_window,
            compact: l.compact,
            cache_read_share: (total > 0.0).then(|| u.cache_read as f64 / total),
            expires_at: rfc3339(l.last_seen_utc + IDLE_TTL),
            usage_note: self.usage_note.clone(),
            model,
            context_window,
            threshold,
            tokens_in: Some(l.spend.tokens_in),
            tokens_out: Some(l.spend.tokens_out),
            spend_usd: (!team).then_some(l.spend.usd),
            spend_notional: Some(self.passthrough || team),
            requests_total: Some(l.spend.requests_total),
            requests_priced: (!team).then_some(l.spend.requests_priced),
            passthrough: Some(self.passthrough),
            team_session: Some(team),
        }
    }

    /// Sets or clears the compaction override (§10.2.4); returns the new value.
    pub(crate) fn update_compact(&self, w: Option<WindowArg>) -> Option<WindowArg> {
        lock(&self.live).compact = w;
        w
    }

    /// Merges the present fields (§17.1 note) and returns the full settings.
    pub(crate) fn update_settings(&self, s: &SessionSettings) -> SessionSettings {
        let mut l = lock(&self.live);
        if let Some(c) = s.redaction_content {
            l.settings.redaction_content = Some(c);
        }
        l.settings.clone()
    }
}

enum Liveness {
    Active,
    Suspended,
    Gone,
}

fn liveness(last_seen: Instant, last_heartbeat: Instant, team: bool, now: Instant) -> Liveness {
    if now.saturating_duration_since(last_seen) > IDLE_TTL {
        return Liveness::Gone;
    }
    if team && now.saturating_duration_since(last_heartbeat) > SUSPEND_AFTER {
        return Liveness::Suspended;
    }
    Liveness::Active
}

const ALPHABET: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// `prefix` + 43 base62 characters (§7.2: `zyg_s_` session tokens, `zyg_t_` team tokens).
pub(crate) fn random_token(prefix: &str) -> String {
    let mut t = String::with_capacity(prefix.len() + 43);
    t.push_str(prefix);
    for _ in 0..43 {
        t.push(ALPHABET[rand::random_range(0..ALPHABET.len())] as char);
    }
    t
}

pub(crate) fn sha256(s: &str) -> [u8; 32] {
    Sha256::digest(s.as_bytes()).into()
}

/// `base` with each feature the profile sets (validated: booleans only) laid over it.
fn profile_features(base: &Features, p: Option<&Profile>) -> Features {
    let mut f = base.clone();
    let Some(t) = p.and_then(|p| p.features.as_ref()) else { return f };
    let b = |k: &str| t.get(k).and_then(|v| v.as_bool());
    if let Some(v) = b("results") {
        f.results = v;
    }
    if let Some(v) = b("work_units") {
        f.work_units = v;
    }
    if let Some(v) = b("compact_tool") {
        f.compact_tool = v;
    }
    if let Some(v) = t.get("redaction").and_then(|r| r.get("content")).and_then(|v| v.as_bool()) {
        f.redaction.content = v;
    }
    f
}

pub(crate) struct Sessions {
    clock: Arc<dyn Clock>,
    pool: Arc<CachePool>,
    map: Mutex<HashMap<String, Arc<SessionState>>>,
    usage_note: Option<String>,
}

impl Sessions {
    /// `redaction_cache_mib`: `[proxy] redaction_cache_mib` of the config at start (a restart changes it).
    pub(crate) fn new(redaction_cache_mib: u64, clock: Arc<dyn Clock>, usage_note: Option<String>) -> Sessions {
        let pool = CachePool::new(cache_total_bytes(redaction_cache_mib), SESSION_CAP_BYTES);
        Sessions { clock, pool, map: Mutex::new(HashMap::new()), usage_note }
    }

    /// `cfg`: the serving config of this request's snapshot; the session keeps it. `still_valid` runs under the
    /// lock `retain_labels` takes, just before the insert: false → `token_invalid` and no session (N16 round 2).
    pub(crate) fn create(
        &self,
        req: SessionRequest,
        team_label: Option<String>,
        cfg: &Arc<Merged>,
        still_valid: &dyn Fn() -> bool,
    ) -> Result<SessionGrant, ZygoError> {
        let c = &cfg.config;
        let mut profile_warning = None;
        let profile = match req.profile {
            Some(p) => match validate(&p, &c.role, &c.main_role, &c.catalog.view(), Dialect::Anthropic, &req.known_plugins) {
                Ok(()) => Some(p),
                Err(v) => {
                    profile_warning = Some(v.to_string());
                    None
                }
            },
            None => None,
        };
        let model_of = |role: &str, r: &Role| {
            profile.as_ref().and_then(|p| p.overrides.get(role)).cloned().unwrap_or_else(|| r.model.clone())
        };
        let roles: BTreeMap<String, Role> = c
            .role
            .iter()
            .filter(|(name, _)| !profile.as_ref().is_some_and(|p| p.disabled.contains(name)))
            .map(|(name, r)| (name.clone(), Role { model: model_of(name, r), ..r.clone() }))
            .collect();
        let models = c
            .catalog
            .view()
            .models
            .into_iter()
            .filter(|(_, v)| v.dialects.contains(&Dialect::Anthropic))
            .map(|(id, _)| id)
            .collect();
        let config = SessionConfig {
            main_role: c.main_role.clone(),
            roles,
            custom_roles: profile.as_ref().map(|p| p.roles.clone()).unwrap_or_default(),
            models,
            guards: c.guards.clone(),
        };

        // §16.1 A1 and its N10 note; C4.
        let kept: Vec<StoredValue> =
            req.stored_values.into_iter().filter(|v| v.expose().len() >= MIN_VALUE_BYTES).collect();
        let mut redaction_note = None;
        let compiled = match compile_with_store(&c.redaction, &kept) {
            Ok(x) => Ok(x),
            Err(_) if !kept.is_empty() => {
                redaction_note = Some(REDACTION_NOTE.to_string());
                compile(&c.redaction)
            }
            Err(e) => Err(e),
        }
        .map_err(|_| {
            ZygoError::new(StatusCode::SERVICE_UNAVAILABLE, "zygo_redaction_unavailable", "redaction patterns do not build")
        })?;
        drop(kept);
        let redaction = Redaction {
            compiled,
            secret_cache: Mutex::new(SessionCache::new(self.pool.clone())),
            content_cache: Mutex::new(SessionCache::new(self.pool.clone())),
        };

        let id = ulid();
        let token = random_token("zyg_s_");
        let now = self.clock.now();
        let utc = self.clock.utc();
        let state = Arc::new(SessionState {
            id: id.clone(),
            agent: req.agent,
            agent_version: req.agent_version,
            profile_features: profile_features(&c.features, profile.as_ref()),
            profile,
            passthrough: req.passthrough,
            repo: req.repo,
            work: req.work,
            team_label,
            cfg: cfg.clone(),
            config: config.clone(),
            redaction,
            pins: Mutex::new(BTreeMap::new()),
            token_sha256: sha256(&token),
            usage_note: self.usage_note.clone(),
            live: Mutex::new(Live {
                last_seen: now,
                last_seen_utc: utc,
                last_heartbeat: now,
                compact: None,
                settings: SessionSettings::default(),
                usage: PromptUsage::default(),
                spend: Spend::default(),
                used_context: None,
                main_loop: None,
            }),
        });
        {
            let mut map = lock(&self.map);
            if !still_valid() {
                return Err(ZygoError::token_invalid());
            }
            retain_live(&mut map, now);
            map.insert(id.clone(), state);
        }
        Ok(SessionGrant {
            session: id,
            token,
            config,
            expires_at: rfc3339(utc + IDLE_TTL),
            profile_warning,
            redaction_note,
            usage_note: self.usage_note.clone(),
        })
    }

    /// Removes every expired session (§7.2 note, N10 review); runs on each session-API request.
    pub(crate) fn sweep(&self) {
        retain_live(&mut lock(&self.map), self.clock.now());
    }

    /// Stage 1 for session-token routes (§7.2). Does not refresh the idle TTL: call `accept` once the request is
    /// accepted.
    pub(crate) fn by_token(&self, id: &str, token: &str) -> Result<Arc<SessionState>, ZygoError> {
        let s = self.get(id).ok_or_else(ZygoError::session_unknown)?;
        if sha256(token) != s.token_sha256 {
            return Err(ZygoError::token_invalid());
        }
        let now = self.clock.now();
        self.locked(&s, |l| match liveness(l.last_seen, l.last_heartbeat, s.team_label.is_some(), now) {
            Liveness::Active => Ok(()),
            Liveness::Suspended => Err(ZygoError::session_suspended()),
            Liveness::Gone => Err(ZygoError::session_unknown()),
        })?;
        Ok(s)
    }

    /// The request on `s` is accepted: checks again that `s` is mapped and active, then refreshes the idle TTL
    /// (§7.2 note: a refused request does not refresh it).
    pub(crate) fn accept(&self, s: &Arc<SessionState>) -> Result<(), ZygoError> {
        let now = self.clock.now();
        self.locked(s, |l| match liveness(l.last_seen, l.last_heartbeat, s.team_label.is_some(), now) {
            Liveness::Active => {
                l.last_seen = now;
                l.last_seen_utc = self.clock.utc();
                Ok(())
            }
            Liveness::Suspended => Err(ZygoError::session_suspended()),
            Liveness::Gone => Err(ZygoError::session_unknown()),
        })
    }

    /// Team route (§7.2 liveness): resumes a suspended session; its state is kept.
    pub(crate) fn heartbeat(&self, id: &str, label: &str) -> Result<(), ZygoError> {
        let s = self.owned(id, label)?;
        let now = self.clock.now();
        self.locked(&s, |l| {
            if matches!(liveness(l.last_seen, l.last_heartbeat, true, now), Liveness::Gone) {
                return Err(ZygoError::session_unknown());
            }
            l.last_seen = now;
            l.last_heartbeat = now;
            l.last_seen_utc = self.clock.utc();
            Ok(())
        })
    }

    pub(crate) fn delete(&self, id: &str, label: &str) -> Result<(), ZygoError> {
        let mut map = lock(&self.map);
        let s = map
            .get(id)
            .filter(|s| s.team_label.as_deref() == Some(label))
            .cloned()
            .ok_or_else(ZygoError::session_unknown)?;
        let gone = {
            let l = lock(&s.live);
            matches!(liveness(l.last_seen, l.last_heartbeat, true, self.clock.now()), Liveness::Gone)
        };
        map.remove(id);
        if gone { Err(ZygoError::session_unknown()) } else { Ok(()) }
    }

    /// Reload (§7.2 N16 note): removes each session whose team label is not in `keep`; returns how many.
    pub(crate) fn retain_labels(&self, keep: &BTreeSet<String>) -> usize {
        let mut map = lock(&self.map);
        let before = map.len();
        map.retain(|_, s| s.team_label.as_ref().is_none_or(|l| keep.contains(l)));
        before - map.len()
    }

    pub(crate) fn get(&self, id: &str) -> Option<Arc<SessionState>> {
        lock(&self.map).get(id).cloned()
    }

    /// The session when `label` created it; else `session_unknown` (§7.2 note).
    fn owned(&self, id: &str, label: &str) -> Result<Arc<SessionState>, ZygoError> {
        self.get(id).filter(|s| s.team_label.as_deref() == Some(label)).ok_or_else(ZygoError::session_unknown)
    }

    /// Runs `f` on the live state of `s` while `s` is still mapped at its id: a session deleted after `get` is
    /// unknown. Lock order: `map`, then `live`.
    fn locked<R>(&self, s: &Arc<SessionState>, f: impl FnOnce(&mut Live) -> Result<R, ZygoError>) -> Result<R, ZygoError> {
        let map = lock(&self.map);
        if !map.get(&s.id).is_some_and(|cur| Arc::ptr_eq(cur, s)) {
            return Err(ZygoError::session_unknown());
        }
        let mut l = lock(&s.live);
        f(&mut l)
    }
}

/// Keeps the sessions that are not past the idle TTL at `now`. Lock order: `map`, then `live`.
fn retain_live(map: &mut HashMap<String, Arc<SessionState>>, now: Instant) {
    map.retain(|_, s| {
        let l = lock(&s.live);
        !matches!(liveness(l.last_seen, l.last_heartbeat, s.team_label.is_some(), now), Liveness::Gone)
    });
}

/// `[proxy] redaction_cache_mib` in bytes.
fn cache_total_bytes(mib: u64) -> usize {
    usize::try_from(mib).unwrap_or(usize::MAX).saturating_mul(1 << 20)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_total_bytes_saturates() {
        assert_eq!(cache_total_bytes(256), 256 << 20);
        assert_eq!(cache_total_bytes(u64::MAX), usize::MAX);
        assert_eq!(cache_total_bytes(1 << 44), usize::MAX);
    }
}
