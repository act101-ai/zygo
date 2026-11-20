//! Plugin selection (spec §15.2) and the `[plugins] enable|disable` writer.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fmt;
use std::io::Write;
use std::path::Path;
use std::str::FromStr;

use anyhow::{anyhow, bail, Context};
use zygo_config::profile::Profile;
use zygo_config::{Layer, Merged, Refusal};
use zygo_core::paths::Paths;

use crate::approve::PreApproval;
use crate::check::valid_name;
use crate::snapshot::{code_approved, deliver_from, filter_mcp, lock_failed, parts, server_names, BUSY};
use crate::store::{self, StoreState};

/// `<marketplace>:<plugin>`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PluginId { pub marketplace: String, pub plugin: String }

impl fmt::Display for PluginId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "{}:{}", self.marketplace, self.plugin) }
}

impl FromStr for PluginId {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> anyhow::Result<PluginId> {
        match s.split_once(':') {
            Some((m, p)) if valid_name(m) && valid_name(p) => Ok(PluginId { marketplace: m.into(), plugin: p.into() }),
            _ => bail!("plugin id must be <marketplace>:<plugin>"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status { Required, Default, Optional, Disabled }

impl Status {
    pub fn as_str(&self) -> &'static str {
        match self {
            Status::Required => "required",
            Status::Default => "default",
            Status::Optional => "optional",
            Status::Disabled => "disabled",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Selection {
    pub selected: BTreeSet<PluginId>,
    pub required: BTreeSet<PluginId>,
    pub default_on: BTreeSet<PluginId>,
    /// User `disable` and profile `false`.
    pub disabled: BTreeSet<PluginId>,
    pub pre_approved: Vec<PreApproval>,
    /// The configured `source` per marketplace.
    pub sources: BTreeMap<String, String>,
}

impl Selection {
    /// D13: `Required`; selected → `Default` or `Optional`; unselected → `Disabled` when default or switched off.
    pub fn status(&self, id: &PluginId) -> Status {
        if self.required.contains(id) {
            Status::Required
        } else if self.selected.contains(id) {
            if self.default_on.contains(id) { Status::Default } else { Status::Optional }
        } else if self.default_on.contains(id) || self.disabled.contains(id) {
            Status::Disabled
        } else {
            Status::Optional
        }
    }
}

fn ids<'a>(list: impl IntoIterator<Item = &'a String>) -> BTreeSet<PluginId> {
    list.into_iter().filter_map(|s| s.parse().ok()).collect()
}

/// A source-key segment as `zygo_config` writes it: bare, or a quoted string.
fn seg(s: &str) -> String {
    if !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') {
        s.to_string()
    } else {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

/// §15.2: `((default_on ∪ user enable) − user disable)`, then the profile map, then `∪ required`.
pub fn selection(cfg: &Merged, p: Option<&Profile>) -> Selection {
    let mut s = Selection::default();
    for m in &cfg.config.marketplaces {
        s.sources.insert(m.name.clone(), m.source.clone());
        let of = |list: &[String]| list.iter().filter_map(|n| format!("{}:{n}", m.name).parse::<PluginId>().ok()).collect::<Vec<_>>();
        s.default_on.extend(of(&m.default_on));
        s.required.extend(of(&m.required));
        if cfg.source_of(&format!("marketplace.{}.approvals", seg(&m.name))) == Layer::Team {
            s.pre_approved.extend(m.approvals.iter().map(|a| PreApproval {
                marketplace: m.name.clone(), plugin: a.plugin.clone(), digest: a.digest.clone(),
            }));
        }
        if cfg.source_of(&format!("marketplace.{}", seg(&m.name))) == Layer::Builtin {
            s.pre_approved.extend(crate::builtin::approved().into_iter().filter(|a| a.marketplace == m.name));
        }
    }
    let enable = ids(&cfg.config.plugins.enable);
    s.disabled = ids(&cfg.config.plugins.disable);
    s.selected = s.default_on.union(&enable).filter(|i| !s.disabled.contains(i)).cloned().collect();
    if let Some(p) = p {
        for (k, on) in &p.plugins {
            let Ok(id) = k.parse::<PluginId>() else { continue };
            if *on {
                s.selected.insert(id);
            } else {
                s.selected.remove(&id);
                s.disabled.insert(id);
            }
        }
    }
    s.selected.extend(s.required.iter().cloned());
    s
}

/// `<m>:<p>` ids of `default_on ∪ required ∪ selected ∪ disabled` plus the store's entries (profile validation).
pub fn known_plugins(sel: &Selection, state: &StoreState) -> BTreeSet<String> {
    let config = sel.default_on.iter().chain(&sel.required).chain(&sel.selected).chain(&sel.disabled).map(PluginId::to_string);
    let stored = state.plugins.iter().map(|p| format!("{}:{}", p.entry.marketplace, p.entry.plugin));
    config.chain(stored).collect()
}

/// Writes `id` into `[plugins] enable` (`on`) or `disable` of the user config at `config_file` and drops it from
/// the other list, keeping every other byte; exclusive lock on `<file>.lock`, temp file plus rename.
pub fn set_enabled(config_file: &Path, cfg: &Merged, id: &PluginId, on: bool) -> anyhow::Result<()> {
    if !cfg.config.marketplaces.iter().any(|m| m.name == id.marketplace) {
        bail!("marketplace {} is not configured", id.marketplace);
    }
    if !on && selection(cfg, None).required.contains(id) {
        return Err(Refusal::Required { plugin: id.to_string() }.into());
    }
    edit_user_config(config_file, |doc| {
        if doc.get("plugins").is_none() { doc["plugins"] = toml_edit::table(); }
        let plugins = doc["plugins"].as_table_like_mut().ok_or_else(|| anyhow!("user config: plugins is not a table"))?;
        let s = id.to_string();
        let (add, drop_from) = if on { ("enable", "disable") } else { ("disable", "enable") };
        if let Some(item) = plugins.get_mut(drop_from) {
            let a = item.as_array_mut().ok_or_else(|| anyhow!("user config: plugins.{drop_from} is not an array"))?;
            a.retain(|v| v.as_str() != Some(s.as_str()));
        }
        if plugins.get(add).is_none() { plugins.insert(add, toml_edit::value(toml_edit::Array::new())); }
        let a = plugins.get_mut(add).and_then(toml_edit::Item::as_array_mut)
            .ok_or_else(|| anyhow!("user config: plugins.{add} is not an array"))?;
        if !a.iter().any(|v| v.as_str() == Some(s.as_str())) { a.push(s.as_str()); }
        Ok(())
    })
}

/// D7 (N49): sets the key at `path` in the user config; absent parents are created as tables.
pub fn set_user_key(config_file: &Path, path: &[&str], v: impl Into<toml_edit::Value>) -> anyhow::Result<()> {
    let (last, parents) = path.split_last().ok_or_else(|| anyhow!("user config: empty key"))?;
    let v = v.into();
    edit_user_config(config_file, |doc| {
        if let Some(top) = parents.first() && doc.get(top).is_none() {
            // A new table goes at the end; the text after the last item (a closing comment) stays ahead of it.
            let mut t = toml_edit::Table::new();
            t.decor_mut().set_prefix(doc.trailing().clone());
            doc.set_trailing("");
            doc.insert(top, toml_edit::Item::Table(t));
        }
        let mut t: &mut dyn toml_edit::TableLike = doc.as_table_mut();
        for (i, seg) in parents.iter().enumerate() {
            if t.get(seg).is_none() { t.insert(seg, toml_edit::table()); }
            t = t.get_mut(seg).and_then(toml_edit::Item::as_table_like_mut)
                .ok_or_else(|| anyhow!("user config: {} is not a table", parents[..=i].join(".")))?;
        }
        // F2: a replaced value keeps its decor (the comment after it).
        match t.get_mut(last).and_then(toml_edit::Item::as_value_mut) {
            Some(old) => {
                let decor = old.decor().clone();
                *old = v;
                *old.decor_mut() = decor;
            }
            None => { t.insert(last, toml_edit::value(v)); }
        }
        Ok(())
    })
}

/// `f` applied to the parsed user config at `config_file` (every other byte kept); exclusive lock on
/// `<file>.lock`, temp file plus rename. Nothing is written when `f` fails.
pub(crate) fn edit_user_config<T>(config_file: &Path,
    f: impl FnOnce(&mut toml_edit::DocumentMut) -> anyhow::Result<T>) -> anyhow::Result<T> {
    let target = std::fs::canonicalize(config_file).unwrap_or_else(|_| config_file.to_path_buf());
    let dir = target.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir).with_context(|| dir.display().to_string())?;
    let name = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let lock_path = dir.join(format!("{name}.lock"));
    let lock = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&lock_path)
        .with_context(|| lock_path.display().to_string())?;
    fs4::FileExt::lock(&lock).with_context(|| lock_path.display().to_string())?;

    let text = match std::fs::read_to_string(&target) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e).with_context(|| target.display().to_string()),
    };
    let mut doc = text.parse::<toml_edit::DocumentMut>()
        .map_err(|e| anyhow!("user config {}: {}", target.display(), zygo_config::repo_file::syntax_error(&text, e.message(), e.span())))?;
    let out = f(&mut doc)?;
    // F3 (N49): a file with CRLF line ends keeps them.
    let mut body = doc.to_string();
    if text.contains("\r\n") {
        body = body.replace("\r\n", "\n").replace('\n', "\r\n");
    }

    let tmp = dir.join(format!(".{name}.{}.tmp", zygo_core::ids::ulid()));
    let res = (|| -> std::io::Result<()> {
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        f.write_all(body.as_bytes())?;
        f.sync_all()?;
        if let Ok(m) = std::fs::metadata(&target) { std::fs::set_permissions(&tmp, m.permissions())?; }
        std::fs::rename(&tmp, &target)
    })();
    if let Err(e) = res {
        let _ = std::fs::remove_file(&tmp);
        return Err(e).with_context(|| target.display().to_string());
    }
    drop(lock);
    Ok(out)
}

/// D6: one line per store entry of a configured source and per selected plugin absent from the store, sorted by
/// id, then the delivery `details` indented by two spaces.
pub fn list(cfg: &Merged, p: Option<&Profile>, paths: &Paths, search_path: Option<&OsStr>) -> Vec<String> {
    let sel = selection(cfg, p);
    let deadline = std::time::Instant::now() + crate::sync::TOTAL;
    // The shared lock keeps the store files steady while they are read; the delivery details use the same lock.
    let lock = match store::lock_until(&paths.plugin_store, false, deadline) {
        Ok(Some(l)) => l,
        Ok(None) => return vec![format!("  {BUSY}")],
        Err(e) => return vec![format!("  {}", lock_failed(&e))],
    };
    let state = store::state(&paths.plugin_store);
    let mut rows: BTreeMap<PluginId, String> = BTreeMap::new();
    for sp in &state.plugins {
        let e = &sp.entry;
        let id = PluginId { marketplace: e.marketplace.clone(), plugin: e.plugin.clone() };
        if sel.sources.get(&id.marketplace) != Some(&sp.source) { continue; }
        let (skills, _, _) = parts(e.files.keys().map(String::as_str));
        let mut mcp: Vec<String> = vec![];
        let mut missing: Vec<String> = vec![];
        // `.mcp.json` only as the manifest lists it, read and checked like the snapshot reads it.
        if sp.intact && let Ok(b) = store::read_verified(&paths.plugin_store, e, ".mcp.json") {
            mcp = server_names(&b);
            for (_, c) in filter_mcp(&b, &sp.dir, search_path).missing {
                if !missing.contains(&c) { missing.push(c); }
            }
        }
        let code = if !sp.intact {
            "changed"
        } else if e.digest.is_none() {
            "none"
        } else if code_approved(paths, &sel, &id, e.digest.as_deref()) {
            "approved"
        } else {
            "needs approval"
        };
        let dash = |v: Vec<String>| if v.is_empty() { "-".to_string() } else { v.iter().map(|s| zygo_core::esc(s)).collect::<Vec<_>>().join(", ") };
        rows.insert(id.clone(), format!("{id} {} ({}) skills: {} · mcp: {} · code: {code} · missing: {}",
            e.version, sel.status(&id).as_str(), dash(skills), dash(mcp), dash(missing)));
    }
    for id in &sel.selected {
        rows.entry(id.clone()).or_insert_with(|| format!("{id} (not synced)"));
    }
    let mut out: Vec<String> = rows.into_values().collect();
    out.extend(deliver_from(paths, &sel, &state, lock).details.iter().map(|x| format!("  {x}")));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_user_config_syntax_error_does_not_echo_the_source() {
        let value = format!("sk-ant-{}", zygo_core::ids::ulid());
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("config.toml");
        std::fs::write(&f, format!("[user]\nkey = {value}\n")).unwrap();
        let r = edit_user_config(&f, |_| Ok(()));
        let e = r.unwrap_err().to_string();
        assert!(e.starts_with("user config ") && e.contains(": TOML parse error at line 2, column "), "{e}");
        assert!(!e.contains(&value) && !e.contains("sk-ant"), "{e}");
    }
}
