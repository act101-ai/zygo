//! The whole config schema (§6, §7, §10, §11.1, §13, §14, §15, §16).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::str::FromStr;
use std::time::Duration;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::features::Features;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub main_role: String,
    #[serde(rename = "model")]
    pub catalog: Catalog,
    pub role: BTreeMap<String, Role>,
    pub profile: BTreeMap<String, ProfileConfig>,
    pub user: UserConfig,
    pub server: ServerConfig,
    pub serve: ServeConfig,
    pub proxy: ProxyConfig,
    pub cache: CacheConfig,
    pub results: ResultsConfig,
    pub features: Features,
    pub guards: GuardsConfig,
    pub navigation: NavigationConfig,
    pub limits: LimitsConfig,
    pub work: WorkConfig,
    pub usage: UsageConfig,
    pub statusline: StatuslineConfig,
    pub update: UpdateConfig,
    pub mail: MailConfig,
    pub workspace: WorkspaceConfig,
    pub loadout: BTreeMap<String, Loadout>,
    #[serde(rename = "marketplace")]
    pub marketplaces: Vec<Marketplace>,
    pub plugins: PluginsConfig,
    pub redaction: RedactionConfig,
    pub schema_rule: Vec<SchemaRule>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            main_role: "lead".into(),
            catalog: Catalog::default(),
            role: BTreeMap::new(),
            profile: BTreeMap::new(),
            user: UserConfig::default(),
            server: ServerConfig::default(),
            serve: ServeConfig::default(),
            proxy: ProxyConfig::default(),
            cache: CacheConfig::default(),
            results: ResultsConfig::default(),
            features: Features::default(),
            guards: GuardsConfig::default(),
            navigation: NavigationConfig::default(),
            limits: LimitsConfig::default(),
            work: WorkConfig::default(),
            usage: UsageConfig::default(),
            statusline: StatuslineConfig::default(),
            update: UpdateConfig::default(),
            mail: MailConfig::default(),
            workspace: WorkspaceConfig::default(),
            loadout: BTreeMap::new(),
            marketplaces: Vec::new(),
            plugins: PluginsConfig::default(),
            redaction: RedactionConfig::default(),
            schema_rule: Vec::new(),
        }
    }
}

// ---- catalog (§6.3) ----

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Catalog(pub Vec<ModelRow>);

impl Catalog {
    pub fn get(&self, id: &str) -> Option<&ModelRow> {
        self.0.iter().find(|m| m.id == id)
    }

    /// Dialects each model can be served on (§7.6 v1 pairs).
    pub fn view(&self) -> CatalogView {
        let models = self
            .0
            .iter()
            .map(|m| {
                let dialects = if m.provider.is_empty() {
                    m.dialects.iter().copied().collect()
                } else {
                    let mut d: BTreeSet<Dialect> =
                        m.provider.iter().filter(|p| p.enabled).flat_map(|p| p.dialects.iter().copied()).collect();
                    if d.contains(&Dialect::Openai) || d.contains(&Dialect::OpenaiChat) {
                        d.insert(Dialect::Anthropic);
                    }
                    d
                };
                let view = ModelView { facts: m.facts.clone(), soft_context_window: m.soft_context_window, dialects };
                (m.id.clone(), view)
            })
            .collect();
        CatalogView { models }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelRow {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub soft_context_window: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub facts: Option<Facts>,
    pub provider: Vec<Provider>,
    pub dialects: Vec<Dialect>,
    #[serde(skip)]
    pub from_team: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Facts {
    pub reasoning: Reasoning,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_default_on: Option<bool>,
    pub effort_levels: Vec<Effort>,
    pub max_output_tokens: u64,
    pub context_window: u64,
    pub sampling: bool,
    pub vision: bool,
    pub structured_output: StructuredOutput,
    pub forced_tool_choice: bool,
    #[serde(deserialize_with = "date_or_string", skip_serializing_if = "Option::is_none")]
    pub verified_on: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// A TOML date (`2026-11-18`) or a string.
fn date_or_string<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    match Option::<toml::Value>::deserialize(d)? {
        None => Ok(None),
        Some(toml::Value::String(s)) => Ok(Some(s)),
        Some(toml::Value::Datetime(dt)) => Ok(Some(dt.to_string())),
        Some(v) => Err(serde::de::Error::invalid_type(serde::de::Unexpected::Other(v.type_str()), &"a date or a string")),
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Reasoning {
    Always,
    Switchable,
    #[default]
    None,
}

/// The §6.3 effort ladder, lowest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effort {
    None,
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StructuredOutput {
    Native,
    #[default]
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Dialect {
    Anthropic,
    Openai,
    OpenaiChat,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Provider {
    pub name: String,
    pub base_url: String,
    pub dialects: Vec<Dialect>,
    pub upstream_model: String,
    /// Absent = no key (a local endpoint or the passthrough host, §12.4).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<KeyRef>,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price_usd_per_mtok: Option<Prices>,
    pub flags: ProviderFlags,
    /// Replaces the default outbound header list (§7.3); the never-list still applies.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forward_headers: Option<Vec<String>>,
}

impl Default for Provider {
    fn default() -> Self {
        Provider {
            name: String::new(),
            base_url: String::new(),
            dialects: Vec::new(),
            upstream_model: String::new(),
            key: None,
            enabled: true,
            price_usd_per_mtok: None,
            flags: ProviderFlags::default(),
            forward_headers: None,
        }
    }
}

/// USD per million tokens (§6.3). `config check` refuses a rate that is negative or not finite (AU32).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prices {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    /// The rate of a cache write with no 5-minute rate of its own; for Anthropic, the 1-hour rate (AU33).
    pub cache_write: f64,
    /// The rate of a 5-minute cache write; absent = `cache_write` (AU33).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_write_5m: Option<f64>,
    /// The rates of a request whose prompt is longer than `long_context.above` (H§7.3).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub long_context: Option<LongContextPrices>,
}

/// The price tier of a long prompt: every rate of the request comes from here when
/// input + cache read + cache write tokens > `above` (H§7.3).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LongContextPrices {
    /// Prompt tokens; `config check` refuses 0 (AU32).
    pub above: u64,
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
    /// Absent = this tier's `cache_write`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_write_5m: Option<f64>,
}

/// The token counts of one request, as `Prices::cost` reads them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TokenUsage {
    /// Input tokens that are not a cache read or a cache write.
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    /// All cache-write tokens, of every TTL.
    pub cache_write: u64,
    /// The 5-minute part of `cache_write`, when the provider reports the split (AU33).
    pub cache_write_5m: Option<u64>,
    /// The 1-hour part of `cache_write`, when the provider reports the split (AU33).
    pub cache_write_1h: Option<u64>,
}

impl Prices {
    /// The list price of one request in USD (AU5, H§7.3 step 1); `None` when the result is not finite (AU32).
    ///
    /// All rates come from `long_context` when the prompt (input + cache read + cache write) is longer than
    /// its `above`. 5-minute writes use `cache_write_5m`, else `cache_write`; 1-hour writes and writes with
    /// no TTL use `cache_write` (AU33). A split larger than `cache_write` is priced, and counted in the
    /// prompt, as reported.
    pub fn cost(&self, u: &TokenUsage) -> Option<f64> {
        let write_5m = u.cache_write_5m.unwrap_or(0);
        let write_1h = u.cache_write_1h.unwrap_or(0);
        let split = write_5m.saturating_add(write_1h);
        let written = u.cache_write.max(split);
        let prompt = u.input.saturating_add(u.cache_read).saturating_add(written);
        let (input, output, cache_read, cache_write, cache_write_5m) = match &self.long_context {
            Some(l) if prompt > l.above => (l.input, l.output, l.cache_read, l.cache_write, l.cache_write_5m),
            _ => (self.input, self.output, self.cache_read, self.cache_write, self.cache_write_5m),
        };
        let per_mtok = u.input as f64 * input
            + u.output as f64 * output
            + u.cache_read as f64 * cache_read
            + write_5m as f64 * cache_write_5m.unwrap_or(cache_write)
            + write_1h as f64 * cache_write
            + (written - split) as f64 * cache_write;
        let usd = per_mtok / 1e6;
        usd.is_finite().then_some(usd)
    }
}

/// §7.5; open for later keys (unknown keys are a `config check` warning).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProviderFlags {
    pub zdr: bool,
    #[serde(flatten)]
    pub other: BTreeMap<String, toml::Value>,
}

/// `env:<NAME>` or `keychain:<name>` (§6.2); an empty name parses, and `check` reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyRef {
    Env(String),
    Keychain(String),
}

impl KeyRef {
    pub fn name_is_empty(&self) -> bool {
        match self {
            KeyRef::Env(n) | KeyRef::Keychain(n) => n.is_empty(),
        }
    }
}

impl fmt::Display for KeyRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KeyRef::Env(n) => write!(f, "env:{n}"),
            KeyRef::Keychain(n) => write!(f, "keychain:{n}"),
        }
    }
}

impl FromStr for KeyRef {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        let r = match s.split_once(':') {
            Some(("env", n)) => KeyRef::Env(n.into()),
            Some(("keychain", n)) => KeyRef::Keychain(n.into()),
            // The text is not echoed: a bad reference is often the key itself (C3).
            _ => return Err("expected env:<NAME> or keychain:<name>".to_string()),
        };
        Ok(r)
    }
}

impl Serialize for KeyRef {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for KeyRef {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d)?.parse().map_err(|_: String| serde::de::Error::invalid_value(serde::de::Unexpected::Other("another string"), &"env:<NAME> or keychain:<name>"))
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CatalogView {
    pub models: BTreeMap<String, ModelView>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModelView {
    pub facts: Option<Facts>,
    pub soft_context_window: Option<u64>,
    pub dialects: BTreeSet<Dialect>,
}

// ---- roles (§6.4) and profiles (§11.1) ----

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Role {
    pub model: String,
    pub description: String,
    pub prompt: String,
    pub writes: bool,
    pub dialect_models: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProfileConfig {
    pub overrides: BTreeMap<String, String>,
    pub disabled: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    pub plugins: BTreeMap<String, bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub features: Option<toml::Table>,
    pub roles: BTreeMap<String, CustomRole>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CustomRole {
    pub model: String,
    pub description: String,
    pub prompt: String,
    pub writes: bool,
}

impl Default for CustomRole {
    fn default() -> Self {
        CustomRole { model: String::new(), description: String::new(), prompt: String::new(), writes: true }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UserConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_profile: Option<String>,
}

// ---- server and proxy (§7) ----

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<KeyRef>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ServeConfig {
    pub tokens: Vec<ServeToken>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ServeToken {
    pub label: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProxyConfig {
    pub require_zdr: bool,
    pub strict_schemas: bool,
    pub max_inflight_mib: u64,
    pub redaction_cache_mib: u64,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        ProxyConfig { require_zdr: false, strict_schemas: false, max_inflight_mib: 512, redaction_cache_mib: 256 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CacheConfig {
    pub insert_markers: BTreeMap<String, bool>,
}

impl Default for CacheConfig {
    fn default() -> Self {
        CacheConfig {
            insert_markers: BTreeMap::from([("claude-code".to_string(), false), ("opencode".to_string(), true)]),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ResultsConfig {
    pub trim_threshold_bytes: u64,
    pub head_lines: u32,
    pub tail_lines: u32,
    /// O§9: the shaping pipeline for Bash (O§7); `false` gives the behavior at `97d8767`.
    pub shape: bool,
}

impl Default for ResultsConfig {
    fn default() -> Self {
        ResultsConfig { trim_threshold_bytes: 16384, head_lines: 60, tail_lines: 20, shape: true }
    }
}

// ---- guards and the repository file tables (§13.4, §13.5) ----

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    #[default]
    Monitor,
    Notify,
    Enforce,
}

impl Level {
    pub fn name(self) -> &'static str {
        match self {
            Level::Monitor => "monitor",
            Level::Notify => "notify",
            Level::Enforce => "enforce",
        }
    }
}

impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for Level {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "monitor" => Ok(Level::Monitor),
            "notify" => Ok(Level::Notify),
            "enforce" => Ok(Level::Enforce),
            _ => Err(format!("unknown level `{s}`")),
        }
    }
}

pub const GUARDS: [&str; 7] = ["claims", "scope", "navigation", "output", "trailers", "repeat", "dispositions"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GuardsConfig {
    pub min_level: Level,
    pub claims: Level,
    pub scope: Level,
    pub navigation: Level,
    pub output: Level,
    pub repeat: Level,
    pub trailers: Level,
    pub dispositions: Level,
}

impl Default for GuardsConfig {
    fn default() -> Self {
        GuardsConfig {
            min_level: Level::Monitor,
            claims: Level::Monitor,
            scope: Level::Monitor,
            navigation: Level::Monitor,
            output: Level::Monitor,
            repeat: Level::Monitor,
            trailers: Level::Notify,
            dispositions: Level::Enforce,
        }
    }
}

impl GuardsConfig {
    fn slot(&mut self, guard: &str) -> Option<&mut Level> {
        Some(match guard {
            "claims" => &mut self.claims,
            "scope" => &mut self.scope,
            "navigation" => &mut self.navigation,
            "output" => &mut self.output,
            "trailers" => &mut self.trailers,
            "repeat" => &mut self.repeat,
            "dispositions" => &mut self.dispositions,
            _ => return None,
        })
    }

    pub fn get(&self, guard: &str) -> Option<Level> {
        Some(match guard {
            "claims" => self.claims,
            "scope" => self.scope,
            "navigation" => self.navigation,
            "output" => self.output,
            "trailers" => self.trailers,
            "repeat" => self.repeat,
            "dispositions" => self.dispositions,
            _ => return None,
        })
    }

    /// False when `guard` is not one of `GUARDS`.
    pub fn set(&mut self, guard: &str, l: Level) -> bool {
        self.slot(guard).map(|s| *s = l).is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NavigationConfig {
    pub lines: u32,
}

impl Default for NavigationConfig {
    fn default() -> Self {
        NavigationConfig { lines: 400 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LimitsConfig {
    /// 0 = no cap.
    pub subagent_max_turns: u32,
    /// The most agent panes a repository loadout may start, 1-32 (R§9.4); built-in, team and user layers only.
    pub max_members: u32,
}

impl Default for LimitsConfig {
    fn default() -> Self {
        LimitsConfig { subagent_max_turns: 0, max_members: MAX_MEMBERS_DEFAULT }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkConfig {
    pub sources: Vec<String>,
}

impl Default for WorkConfig {
    fn default() -> Self {
        WorkConfig { sources: ["work-graph", "beads", "github", "branch"].map(String::from).to_vec() }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UsageConfig {
    pub retain_days: u32,
}

impl Default for UsageConfig {
    fn default() -> Self {
        UsageConfig { retain_days: 90 }
    }
}

/// `[statusline]` (statusline HUD design §8, OP7, AU1). User config only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct StatuslineConfig {
    /// One of [`STATUSLINE_STYLES`]; `config check` refuses any other value.
    pub style: String,
}

/// The values of `statusline.style`; the first is the default.
pub const STATUSLINE_STYLES: [&str; 2] = ["compact", "verbose"];

impl Default for StatuslineConfig {
    fn default() -> Self {
        StatuslineConfig { style: STATUSLINE_STYLES[0].into() }
    }
}

/// `[update]` (auto-update design, "Opt-out"). User config only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdateConfig {
    /// `false` turns the update check off.
    pub check: bool,
}

impl Default for UpdateConfig {
    fn default() -> Self {
        UpdateConfig { check: true }
    }
}

/// `[mail.workspace.<name>]` (mail sync design S§11): the per-workspace mail sync keys. `sync` turns sync on for that
/// workspace; `remote` defaults to `origin`; `host` overrides `[mail] host`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MailSyncWorkspace {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sync: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
}

/// `[mail]` (workspace mail design M§11). User config only; `keep` and `wake_gap` in the form [`parse_duration`] reads.
/// `host`, `sync_gap` and `workspace` are mail sync design S§11; `sync_gap` takes `<n>s`, `<n>m` or `<n>h`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MailConfig {
    pub keep: String,
    pub max_unread: u32,
    pub wake_gap: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    pub sync_gap: String,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub workspace: BTreeMap<String, MailSyncWorkspace>,
}

impl Default for MailConfig {
    fn default() -> Self {
        MailConfig {
            keep: "30d".into(),
            max_unread: 500,
            wake_gap: "30s".into(),
            host: None,
            sync_gap: "60s".into(),
            workspace: BTreeMap::new(),
        }
    }
}

/// A duration written as `<n>s`, `<n>m`, `<n>h` or `<n>d` (W§11 drain_timeout, M§11 keep and wake_gap; W§19): ASCII digits and
/// one unit letter. `None` for any other text and for a value that does not fit in `u64` seconds.
pub fn parse_duration(s: &str) -> Option<Duration> {
    let (unit, digits) = s.as_bytes().split_last()?;
    let scale: u64 = match unit {
        b's' => 1,
        b'm' => 60,
        b'h' => 3600,
        b'd' => 86400,
        _ => return None,
    };
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let n: u64 = std::str::from_utf8(digits).ok()?.parse().ok()?;
    n.checked_mul(scale).map(Duration::from_secs)
}

/// The mail host form (mail sync design S§5, S§11): `^[a-z0-9][a-z0-9-]{0,31}$`.
pub fn valid_mail_host(s: &str) -> bool {
    let b = s.as_bytes();
    let lower_digit = |c: &u8| c.is_ascii_lowercase() || c.is_ascii_digit();
    matches!(b.first(), Some(c) if lower_digit(c))
        && b.len() <= 32
        && b[1..].iter().all(|c| lower_digit(c) || *c == b'-')
}

/// The mail sync branch form (mail sync design S§11): `^zygo/[a-z0-9][a-z0-9-]{0,31}$`.
pub fn valid_sync_branch(s: &str) -> bool {
    s.strip_prefix("zygo/").is_some_and(valid_mail_host)
}

/// `[mail] sync_gap` (mail sync design S§11, S§8.4): [`parse_duration`] without the `d` unit.
pub(crate) fn parse_sync_gap(s: &str) -> Option<Duration> {
    if s.ends_with('d') {
        return None;
    }
    parse_duration(s)
}

/// The mail sync settings of one workspace, resolved by [`sync_for`] (mail sync design S§11, S§7.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncSettings {
    pub branch: String,
    pub remote: String,
    pub host: String,
    pub sync_gap: Duration,
}

/// Why [`sync_for`] gives no mail sync for a workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncOff {
    /// No `[mail.workspace.<name>] sync` key (S-G6).
    NotOn,
    /// Neither the workspace `host` nor `[mail] host` is set (S§7.1).
    NoHost,
    /// The host in force fails [`valid_mail_host`].
    BadHost,
    /// `sync` fails [`valid_sync_branch`].
    BadBranch,
    /// `[mail] sync_gap` is not `<n>s`, `<n>m` or `<n>h`.
    BadGap,
}

/// The mail sync settings of workspace `ws_name` (mail sync design S§11, S§7.1). Checks in order: not on, branch form,
/// host present (workspace `host`, else `[mail] host`), host form, `sync_gap` form. `remote` defaults to `origin`.
pub fn sync_for(cfg: &Config, ws_name: &str) -> Result<SyncSettings, SyncOff> {
    let ws = cfg.mail.workspace.get(ws_name).ok_or(SyncOff::NotOn)?;
    let branch = ws.sync.as_ref().ok_or(SyncOff::NotOn)?;
    if !valid_sync_branch(branch) {
        return Err(SyncOff::BadBranch);
    }
    let host = ws.host.as_ref().or(cfg.mail.host.as_ref()).ok_or(SyncOff::NoHost)?;
    if !valid_mail_host(host) {
        return Err(SyncOff::BadHost);
    }
    let sync_gap = parse_sync_gap(&cfg.mail.sync_gap).ok_or(SyncOff::BadGap)?;
    Ok(SyncSettings {
        branch: branch.clone(),
        remote: ws.remote.clone().unwrap_or_else(|| "origin".into()),
        host: host.clone(),
        sync_gap,
    })
}

/// The loadout that needs no config (workspaces design W§11, AU45): one `claude-code` pane in the main worktree.
pub const BUILTIN_LOADOUT: &str = "solo";

/// The name of the loadout that the workspace definition gives (R§9.1). `config check` decides
/// `default_loadout = "repository"` by this name (R§10, FX-29) and does not depend on the definition read.
pub const REPOSITORY_LOADOUT: &str = "repository";

/// The values of `loadout.<name>.window.pane.kind` (W§11); `config check` refuses any other value.
pub const LOADOUT_KINDS: [&str; 3] = ["claude-code", "opencode", "shell"];

/// The values of `loadout.<name>.window.pane.job` (workspace definition design R§9.3); `config check` refuses any other
/// value.
pub const JOBS: [&str; 2] = ["orchestrator", "worker"];

/// `[limits] max_members` when no layer sets it (R§9.4).
pub const MAX_MEMBERS_DEFAULT: u32 = 8;

/// `[workspace]` (workspaces design W§11, AU10). User config only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkspaceConfig {
    /// The loadout of `zygo up` with no name: [`BUILTIN_LOADOUT`] or a key of `[loadout]`.
    pub default_loadout: String,
    /// A draining workspace refuses new dispatches (OP6).
    pub drain_block_dispatch: bool,
    /// How long a drain waits (AU4), in the form [`parse_duration`] reads.
    pub drain_timeout: String,
    /// The config-directory pool (R§9.5); `~` expanded as `--config-dir` does; absolute after that.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pool: Option<String>,
}

impl Default for WorkspaceConfig {
    fn default() -> Self {
        WorkspaceConfig {
            default_loadout: BUILTIN_LOADOUT.into(),
            drain_block_dispatch: true,
            drain_timeout: "10m".into(),
            pool: None,
        }
    }
}

/// `[loadout.<name>]` (workspaces design W§11, AU10): the windows that `zygo up` starts. User config only.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Loadout {
    pub window: Vec<LoadoutWindow>,
}

/// `[[loadout.<name>.window]]` (W§11). User config only.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LoadoutWindow {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout: Option<String>,
    pub pane: Vec<LoadoutPane>,
}

/// `[[loadout.<name>.window.pane]]` (W§11). User config only. An unset field takes the default of `zygo claude` or
/// `zygo opencode` (AU45).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LoadoutPane {
    /// One of [`LOADOUT_KINDS`]; `config check` refuses any other value.
    pub kind: String,
    /// The member name (M§6); config check applies zygo_core::ids::valid_member_name and uniqueness in the loadout.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// One of [`JOBS`] (R§9.3); absent: no job.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    pub args: Vec<String>,
    pub yolo: bool,
    /// Only with `kind = "claude-code"`.
    pub passthrough: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compact: Option<String>,
    /// Only with `kind = "claude-code"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_dir: Option<String>,
    /// The config directory from `[workspace] pool` (R§9.5). Only with `kind = "claude-code"`; not with `config_dir`.
    pub pool: bool,
    /// Absolute, relative to the main worktree, or `branch:<name>` for the worktree that has that branch checked out.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
}

// ---- plugins (§15) ----

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Marketplace {
    pub name: String,
    pub source: String,
    #[serde(rename = "ref")]
    pub git_ref: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    pub default_on: Vec<String>,
    pub required: Vec<String>,
    pub approvals: Vec<Approval>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Approval {
    pub plugin: String,
    pub digest: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PluginsConfig {
    pub enable: Vec<String>,
    pub disable: Vec<String>,
}

// ---- redaction (§16) ----

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RedactionConfig {
    pub pattern: Vec<RedactionPattern>,
    pub content: ContentConfig,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RedactionPattern {
    pub id: String,
    pub pattern: String,
    pub action: Action,
    pub description: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    #[default]
    Redact,
    Block,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ContentConfig {
    pub replacement: String,
    pub allow: Vec<String>,
    pub deny: Vec<String>,
}

impl Default for ContentConfig {
    fn default() -> Self {
        ContentConfig { replacement: "🤬".into(), allow: Vec::new(), deny: Vec::new() }
    }
}

// ---- strict-schema rules (§7.6) ----

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SchemaRule {
    pub family: Family,
    pub code: String,
    pub severity: RuleSeverity,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Family {
    #[default]
    Anthropic,
    Openai,
}

impl Family {
    pub fn name(self) -> &'static str {
        match self {
            Family::Anthropic => "anthropic",
            Family::Openai => "openai",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleSeverity {
    #[default]
    Refuse,
    Warn,
}
