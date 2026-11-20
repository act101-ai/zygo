//! Serve-mode team tokens (§7.2) and the server's known plugin set.

use std::collections::{BTreeSet, HashMap};
use std::io::Write;
use std::path::Path;

use anyhow::{anyhow, bail, Context};
use serde::{Deserialize, Serialize};
use zygo_config::{Config, Dialect, Facts, Marketplace, Merged, ServeToken};

use crate::session::{random_token, sha256, TeamAuth};

/// A new team token (`zyg_t_` + 43 base62 characters, §7.2) and its lower-case hex SHA-256.
pub fn new_token() -> (String, String) {
    let t = random_token("zyg_t_");
    let h = hex::encode(sha256(&t));
    (t, h)
}

/// Appends `{ label, sha256 }` to `serve.tokens` in `config_file` (every other byte kept); refuses a label the file
/// already has (a duplicate label is a load error). Exclusive lock on `<file>.lock`, temp file plus rename, the file
/// mode kept (a new file: 0600 on unix); nothing is written on an error.
pub fn append_token(config_file: &Path, label: &str, sha256_hex: &str) -> anyhow::Result<()> {
    let target = std::fs::canonicalize(config_file).unwrap_or_else(|_| config_file.to_path_buf());
    let dir = target.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir).with_context(|| dir.display().to_string())?;
    let name = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let lock_path = dir.join(format!("{name}.lock"));
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .with_context(|| lock_path.display().to_string())?;
    fs4::FileExt::lock(&lock).with_context(|| lock_path.display().to_string())?;

    let text = match std::fs::read_to_string(&target) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e).with_context(|| target.display().to_string()),
    };
    // No parser text: it can quote a value from the file (C3).
    let mut doc = text.parse::<toml_edit::DocumentMut>().map_err(|_| anyhow!("config {}: not valid TOML", target.display()))?;
    push_token(&mut doc, label, sha256_hex)?;

    let tmp = dir.join(format!(".{name}.{}.tmp", zygo_core::ids::ulid()));
    let res = (|| -> std::io::Result<()> {
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create_new(true);
        // The token hashes are never readable by others, even mid-write (N16 round 2).
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
        let mut f = o.open(&tmp)?;
        f.write_all(doc.to_string().as_bytes())?;
        f.sync_all()?;
        if let Ok(m) = std::fs::metadata(&target) {
            std::fs::set_permissions(&tmp, m.permissions())?;
        }
        std::fs::rename(&tmp, &target)
    })();
    if let Err(e) = res {
        let _ = std::fs::remove_file(&tmp);
        return Err(e).with_context(|| target.display().to_string());
    }
    drop(lock);
    Ok(())
}

fn push_token(doc: &mut toml_edit::DocumentMut, label: &str, sha256_hex: &str) -> anyhow::Result<()> {
    let not_array = || anyhow!("config: serve.tokens is not an array");
    let serve = doc.entry("serve").or_insert(toml_edit::table()).as_table_like_mut().ok_or_else(not_array)?;
    let has = |l: Option<&str>| l == Some(label);
    match serve.get_mut("tokens") {
        None => {
            let mut a = toml_edit::Array::new();
            a.push(inline(label, sha256_hex));
            serve.insert("tokens", toml_edit::value(a));
        }
        Some(toml_edit::Item::Value(toml_edit::Value::Array(a))) => {
            if a.iter().any(|v| has(v.as_inline_table().and_then(|t| t.get("label")).and_then(|v| v.as_str()))) {
                bail!("label {label} exists; remove it from the config first");
            }
            a.push(inline(label, sha256_hex));
        }
        Some(toml_edit::Item::ArrayOfTables(a)) => {
            if a.iter().any(|t| has(t.get("label").and_then(|v| v.as_str()))) {
                bail!("label {label} exists; remove it from the config first");
            }
            let mut t = toml_edit::Table::new();
            t.insert("label", toml_edit::value(label));
            t.insert("sha256", toml_edit::value(sha256_hex));
            a.push(t);
        }
        Some(_) => return Err(not_array()),
    }
    Ok(())
}

fn inline(label: &str, sha256_hex: &str) -> toml_edit::InlineTable {
    let mut t = toml_edit::InlineTable::new();
    t.insert("label", label.into());
    t.insert("sha256", sha256_hex.into());
    t
}

/// The `<marketplace>:<plugin>` names the server config knows (it does not sync): each marketplace's `default_on`
/// and `required`, `[plugins] enable` and `disable`, and every key of every `profile.<n>.plugins` map.
pub fn known_plugins(c: &Config) -> BTreeSet<String> {
    let mut k: BTreeSet<String> = c
        .marketplaces
        .iter()
        .flat_map(|m| m.default_on.iter().chain(&m.required).map(move |p| format!("{}:{p}", m.name)))
        .collect();
    k.extend(c.plugins.enable.iter().chain(&c.plugins.disable).cloned());
    k.extend(c.profile.values().flat_map(|p| p.plugins.keys().cloned()));
    k
}

/// `[serve] tokens`: lower-case hex SHA-256 → label. Only the hash is kept.
pub struct Tokens(HashMap<String, String>);

impl Tokens {
    pub fn new(t: &[ServeToken]) -> Tokens {
        Tokens(t.iter().map(|t| (t.sha256.to_ascii_lowercase(), t.label.clone())).collect())
    }
}

impl TeamAuth for Tokens {
    fn label_for(&self, token: &str) -> Option<String> {
        self.0.get(&hex::encode(sha256(token))).cloned()
    }
}

/// `GET /v1/zygo/config` (§7.2, §7.3): the team layer in config key names, with no provider data (provider names,
/// URLs, keys, prices, flags never leave the server). `team` holds only the keys the server's config file names
/// (§6.1 layer 7, owner decision at N37 planning), valued from the typed config.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TeamConfigReply {
    #[serde(flatten)]
    pub team: toml::Table,
    pub model: Vec<TeamModel>,
}

/// A catalog model as the client sees it: id, facts, soft window and the dialects it is served on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TeamModel {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub facts: Option<Facts>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub soft_context_window: Option<u64>,
    pub dialects: Vec<Dialect>,
}

/// The config sections the team layer can set (§7.2); `marketplace` comes from `file_marketplaces`.
pub const TEAM_SECTIONS: [&str; 9] =
    ["main_role", "role", "profile", "redaction", "guards", "limits", "features", "results", "navigation"];

/// The role fields that go out (§7.2).
const ROLE_FIELDS: [&str; 4] = ["model", "description", "prompt", "writes"];

impl TeamConfigReply {
    pub fn from_merged(m: &Merged) -> Self {
        // A config that came from TOML serializes to TOML; on a failure nothing goes out (fail closed).
        let typed = toml::Table::try_from(&m.config).unwrap_or_default();
        let mut team = toml::Table::new();
        for k in TEAM_SECTIONS {
            if let (Some(raw), Some(t)) = (m.user_table.get(k), typed.get(k))
                && let Some(v) = pick(&[k], raw, t)
            {
                team.insert(k.into(), v);
            }
        }
        let markets = file_marketplaces(m);
        if !markets.is_empty()
            && let Ok(v) = toml::Value::try_from(&markets)
        {
            team.insert("marketplace".into(), v);
        }
        let model = m
            .config
            .catalog
            .view()
            .models
            .into_iter()
            .map(|(id, v)| TeamModel { id, facts: v.facts, soft_context_window: v.soft_context_window, dialects: v.dialects.into_iter().collect() })
            .collect();
        TeamConfigReply { team, model }
    }
}

/// The part of the typed value `typed` at `path` that the file's value `raw` names (B3: a key the schema ignores is
/// in `raw` only, so it never goes out).
fn pick(path: &[&str], raw: &toml::Value, typed: &toml::Value) -> Option<toml::Value> {
    use toml::Value;
    match (path, raw, typed) {
        (["role", _], Value::Table(r), Value::Table(t)) => {
            let out: toml::Table = ROLE_FIELDS
                .iter()
                .filter(|k| r.contains_key(**k))
                .filter_map(|k| t.get(*k).map(|v| (k.to_string(), v.clone())))
                .collect();
            (!out.is_empty()).then_some(Value::Table(out))
        }
        // An empty preset goes out as `{}`.
        (["profile", _], Value::Table(r), Value::Table(t)) => {
            let mut out = toml::Table::new();
            for (k, rv) in r {
                let Some(tv) = t.get(k) else { continue };
                let v = if k == "features" { profile_features(tv) } else { pick(&[path, &[k.as_str()]].concat(), rv, tv) };
                if let Some(v) = v {
                    out.insert(k.clone(), v);
                }
            }
            Some(Value::Table(out))
        }
        (_, Value::Table(r), Value::Table(t)) => {
            let mut out = toml::Table::new();
            for (k, rv) in r {
                if let Some(tv) = t.get(k)
                    && let Some(v) = pick(&[path, &[k.as_str()]].concat(), rv, tv)
                {
                    out.insert(k.clone(), v);
                }
            }
            (!out.is_empty()).then_some(Value::Table(out))
        }
        // Redaction patterns: the typed items whose `id` a file item names.
        (_, Value::Array(r), Value::Array(t)) if r.iter().any(|i| i.get("id").is_some()) => {
            let ids: Vec<&toml::Value> = r.iter().filter_map(|i| i.get("id")).collect();
            let out: Vec<toml::Value> = t.iter().filter(|i| i.get("id").is_some_and(|id| ids.contains(&id))).cloned().collect();
            (!out.is_empty()).then_some(Value::Array(out))
        }
        _ => Some(typed.clone()),
    }
}

/// `profile.<n>.features` is free-form (checked at session creation): only the known toggles, each only as a boolean.
fn profile_features(typed: &toml::Value) -> Option<toml::Value> {
    let mut out = toml::Table::new();
    for k in ["results", "work_units", "compact_tool"] {
        if let Some(b) = typed.get(k).filter(|v| v.is_bool()) {
            out.insert(k.into(), b.clone());
        }
    }
    if let Some(b) = typed.get("redaction").and_then(|r| r.get("content")).filter(|v| v.is_bool()) {
        out.insert("redaction".into(), toml::Value::Table(toml::Table::from_iter([("content".to_string(), b.clone())])));
    }
    (!out.is_empty()).then_some(toml::Value::Table(out))
}

/// The marketplaces the server's config file names.
fn file_marketplaces(m: &Merged) -> Vec<Marketplace> {
    let named: Vec<&str> = m
        .user_table
        .get("marketplace")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|e| e.get("name").and_then(toml::Value::as_str))
        .collect();
    m.config.marketplaces.iter().filter(|x| named.contains(&x.name.as_str())).cloned().collect()
}

/// `GET /v1/zygo/plugins` (§15.3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginsReply {
    pub marketplace: Vec<Marketplace>,
}

impl PluginsReply {
    pub fn from_merged(m: &Merged) -> Self {
        PluginsReply { marketplace: file_marketplaces(m) }
    }
}
