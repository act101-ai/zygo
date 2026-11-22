//! The whole config schema (§6, §7, §10, §11.1, §13, §14, §15, §16).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::str::FromStr;

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

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prices {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
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
}

impl Default for ResultsConfig {
    fn default() -> Self {
        ResultsConfig { trim_threshold_bytes: 16384, head_lines: 60, tail_lines: 20 }
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

pub const GUARDS: [&str; 6] = ["claims", "scope", "navigation", "output", "trailers", "repeat"];

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

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LimitsConfig {
    /// 0 = no cap.
    pub subagent_max_turns: u32,
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
