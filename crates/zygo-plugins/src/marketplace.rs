//! `zygo marketplace add|list|update|remove` (spec §15.8, §15.1, §15.7): the library side.

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{anyhow, bail};
use zygo_config::{Layer, LoadContext, Merged};
use zygo_core::paths::Paths;

use crate::check::{check_dir_modes, esc, git_modes_by, valid_name, CheckReport, PluginReport};
use crate::select::{edit_user_config, selection, set_enabled, PluginId};
use crate::store;
use crate::sync::{
    check_failed, clone_url, hex40, key_seg, lock_marketplace, remove_old_caches, resolve, rewrite_config, sync_with, Git, SyncOpts,
    SyncReport, TOTAL,
};

/// The roots one command works in: paths, the config loader and the git seam.
pub struct Env<'a> { pub paths: &'a Paths, pub load: &'a dyn Fn() -> anyhow::Result<Merged>, pub git: &'a SyncOpts }

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Enable { #[default] None, All, Plugins(Vec<String>) }

#[derive(Debug, Clone, Default)]
pub struct AddOpts { pub name: Option<String>, pub git_ref: Option<String>, pub enable: Enable, pub sync: bool }

#[derive(Debug)]
pub struct AddReport {
    pub name: String, pub source: String, pub git_ref: String, pub commit: String, pub replaced: bool,
    pub plugins: Vec<PluginReport>, pub skipped: Vec<String>, pub enabled: Vec<PluginId>, pub sync: Option<SyncReport>,
}

/// `f` over the paths, config loader and git seam of the process environment.
fn process_env<R>(f: impl FnOnce(&Env) -> R) -> R {
    let paths = Paths::from_env();
    let load = || Ok(zygo_config::load(&LoadContext { paths: paths.clone(), repo: None, team: None })?);
    f(&Env { paths: &paths, load: &load, git: &SyncOpts::default() })
}

pub fn add(src: &str, opts: AddOpts) -> anyhow::Result<AddReport> { process_env(|env| add_in(env, src, &opts)) }

/// A git error as an `anyhow` error.
fn g<T>(r: Result<T, String>) -> anyhow::Result<T> { r.map_err(anyhow::Error::msg) }

/// A clone of a marketplace in `<marketplaces_dir>/.probe-<ulid>`; removed on drop unless it became the cache.
struct Probe<'a> { git: &'a Git, dir: PathBuf }

impl Drop for Probe<'_> {
    fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.dir); }
}

impl<'a> Probe<'a> {
    fn clone(git: &'a Git, mdir: &Path, url: &str, deadline: Instant) -> anyhow::Result<Probe<'a>> {
        let name = format!(".probe-{}", zygo_core::ids::ulid());
        let probe = Probe { git, dir: mdir.join(&name) };
        g(git.run(mdir, &["clone", "--no-checkout", "--no-local", "--quiet", "--", url, &name], deadline))?;
        rewrite_config(&probe.dir, url)?;
        Ok(probe)
    }

    fn tags(&self, deadline: Instant) -> anyhow::Result<Vec<String>> {
        let out = g(self.git.run(&self.dir, &["for-each-ref", "--format=%(refname:strip=2)", "refs/tags"], deadline))?;
        Ok(out.lines().map(str::to_string).collect())
    }

    /// D2: `git_ref` as sync reads it; the commit it names.
    fn resolve(&self, git_ref: &str, deadline: Instant) -> anyhow::Result<(String, bool)> {
        let bad = || anyhow!("ref is not a valid tag name");
        if git_ref.starts_with('-') { return Err(bad()); }
        if let Err(e) = self.git.run(&self.dir, &["check-ref-format", &format!("refs/tags/{git_ref}")], deadline) {
            return Err(if e == "git timed out" { anyhow!(e) } else { bad() });
        }
        g(resolve(self.git, &self.dir, git_ref, deadline))
    }

    /// The tree at `commit`, checked as sync checks it.
    fn checkout_check(&self, commit: &str, deadline: Instant) -> anyhow::Result<CheckReport> {
        g(self.git.run(&self.dir, &["checkout", "--quiet", "--detach", "--force", "--end-of-options", commit], deadline))?;
        let modes = git_modes_by(self.git.command(&self.dir), deadline);
        Ok(check_dir_modes(&self.dir, deadline, modes))
    }

    /// The probe becomes `<marketplaces_dir>/<name>`, replacing any cache there.
    fn into_cache(self, name: &str) -> anyhow::Result<()> {
        let target = self.dir.with_file_name(name);
        match std::fs::symlink_metadata(&target) {
            Ok(m) if m.is_dir() => std::fs::remove_dir_all(&target)?,
            Ok(_) => std::fs::remove_file(&target)?,
            Err(_) => {}
        }
        Ok(std::fs::rename(&self.dir, &target)?)
    }
}

/// D1: the highest semver tag (a leading `v` allowed) above `above`, ties broken by the tag name.
fn highest(tags: &[String], above: Option<&semver::Version>) -> Option<String> {
    tags.iter().filter_map(|t| Some((semver::Version::parse(t.strip_prefix('v').unwrap_or(t)).ok()?, t)))
        .filter(|(v, _)| above.is_none_or(|a| v > a)).max().map(|(_, t)| t.clone())
}

/// `<mdir>/.probe-<ulid>` dirs a crashed add left, at least `TOTAL` old (a live add ends within `TOTAL`).
fn remove_stale_probes(mdir: &Path) {
    let Ok(rd) = std::fs::read_dir(mdir) else { return };
    for e in rd.flatten() {
        let n = e.file_name();
        let Some(id) = n.to_str().and_then(|n| n.strip_prefix(".probe-")) else { continue };
        let old = e.metadata().and_then(|m| m.modified()).is_ok_and(|t| t.elapsed().is_ok_and(|age| age >= TOTAL));
        if id.len() == 26 && id.bytes().all(|b| b.is_ascii_digit() || b.is_ascii_uppercase()) && old {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

/// §15.8 add: clone, pick the pin (D1, D2), check, then write the user entry under the marketplace lock (D5).
pub fn add_in(env: &Env, src: &str, opts: &AddOpts) -> anyhow::Result<AddReport> {
    let end = Instant::now() + TOTAL;
    let step = || (Instant::now() + env.git.per_marketplace).min(end);
    if opts.name.as_deref().is_some_and(|n| !valid_name(n)) { bail!("unsafe marketplace name"); }
    let url = clone_url(src)?;
    let mdir = &env.paths.marketplaces_dir;
    std::fs::create_dir_all(mdir)?;
    let git = Git::new(env.git);
    g(git.check_version(mdir, step()))?;
    let probe = Probe::clone(&git, mdir, &url, step())?;
    let (git_ref, commit) = match &opts.git_ref {
        Some(r) => (r.clone(), probe.resolve(r, step())?.0),
        None => {
            let tag = highest(&probe.tags(step())?, None).ok_or_else(|| anyhow!("no semver tag in {}; pass --ref <tag|commit>", zygo_config::display::display_source(src)))?;
            let commit = probe.resolve(&tag, step())?.0;
            (tag, commit)
        }
    };
    let mut check = probe.checkout_check(&commit, step())?;
    if check.marketplace.is_none() {
        bail!("{}", check.violations.first().map_or("no .claude-plugin/marketplace.json".to_string(), check_failed));
    }
    let name = match (&opts.name, check.marketplace.as_deref()) {
        (Some(n), _) => n.clone(),
        (None, Some("")) => bail!("marketplace has no name; pass --name <n>"),
        (None, Some(m)) if valid_name(m) => m.to_string(),
        _ => bail!("unsafe marketplace name"),
    };
    check.plugins.sort_by(|a, b| a.name.cmp(&b.name));
    let plugin_id = |p: &str| PluginId { marketplace: name.clone(), plugin: p.to_string() };
    // Only a plugin sync can install: no check violation and not the reserved name (sync.rs `sync_marketplace`).
    let ok = |p: &&PluginReport| p.violations.is_empty() && p.name != crate::embedded::NAME;
    let enabled: Vec<PluginId> = match &opts.enable {
        Enable::None => vec![],
        Enable::All => check.plugins.iter().filter(ok).map(|p| plugin_id(&p.name)).collect(),
        Enable::Plugins(list) => {
            if let Some(p) = list.iter().find(|p| !check.plugins.iter().filter(ok).any(|x| &x.name == *p)) {
                bail!("{name} has no plugin {p}");
            }
            list.iter().map(|p| plugin_id(p)).collect()
        }
    };

    // D5: everything under the marketplace lock; the sync below takes the same lock itself.
    let replaced = {
        let _lock = lock_marketplace(env.paths, &name, end)?.ok_or_else(|| anyhow!("marketplace {name} is locked; try again"))?;
        remove_old_caches(mdir, &name);
        remove_stale_probes(mdir);
        let cfg = (env.load)()?;
        let built_in = cfg.config.marketplaces.iter()
            .find(|m| m.name == name && cfg.source_of(&format!("marketplace.{}", key_seg(&name))) == Layer::Builtin);
        if built_in.is_some_and(|m| m.source != src) { bail!("marketplace {name} is built in; its source cannot change"); }
        probe.into_cache(&name)?;
        let entry = Entry { name: name.clone(), source: src.to_string(), git_ref: git_ref.clone(), commit: commit.clone(), default_on: None };
        let replaced = edit_user_config(&env.paths.config_file, |doc| put_entry(doc, &entry))?;
        if !enabled.is_empty() {
            let cfg = (env.load)()?;
            for id in &enabled { set_enabled(&env.paths.config_file, &cfg, id, true)?; }
        }
        replaced
    };

    // D15: the sync of the whole config, cut to this marketplace.
    let sync = if opts.sync && !enabled.is_empty() {
        let mut r = sync_with(&(env.load)()?, env.paths, TOTAL, env.git);
        let (m, p) = (format!("marketplace {name}:"), format!("plugin {name}:"));
        let scope = format!("{name}:");
        r.lines.retain(|l| l.starts_with(&m) || l.starts_with(&p));
        r.failures.retain(|(s, _)| *s == name || s.starts_with(&scope));
        r.synced.retain(|i| i.marketplace == name);
        Some(r)
    } else {
        None
    };
    Ok(AddReport { name, source: src.to_string(), git_ref, commit, replaced, plugins: check.plugins, skipped: check.skipped, enabled, sync })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Move { pub name: String, pub from_ref: String, pub to_ref: String, pub from_commit: String, pub to_commit: String }

#[derive(Debug, Clone)]
pub struct Change {
    pub id: PluginId, pub old_version: Option<String>, pub new_version: String,
    pub old_digest: Option<String>, pub new_digest: Option<String>, pub block: String,
}

#[derive(Debug, Default)]
pub struct UpdateReport {
    pub moves: Vec<Move>, pub current: Vec<String>, pub changes: Vec<Change>,
    pub lines: Vec<String>, pub failures: Vec<(String, String)>, pub sync: Option<SyncReport>,
}

pub fn update(name: Option<&str>, accept: bool) -> anyhow::Result<UpdateReport> {
    process_env(|env| update_in(env, name, accept, true))
}

/// §15.8 update (D8, D9, D14): each target moves to the highest semver tag above its current one, or takes a moved
/// tag with `accept`; one full sync follows when `sync` and something moved.
pub fn update_in(env: &Env, name: Option<&str>, accept: bool, sync: bool) -> anyhow::Result<UpdateReport> {
    let cfg = (env.load)()?;
    let layer_of = |n: &str| cfg.source_of(&format!("marketplace.{}", key_seg(n)));
    let targets: Vec<&zygo_config::Marketplace> = match name {
        Some(n) => vec![cfg.config.marketplaces.iter().find(|m| m.name == n).ok_or_else(|| anyhow!("marketplace {} is not configured", esc(n)))?],
        None => cfg.config.marketplaces.iter().filter(|m| !(layer_of(&m.name) == Layer::Builtin && m.git_ref.is_empty())).collect(),
    };
    let manifest = store::read_manifest(&env.paths.plugin_store).ok().flatten().unwrap_or_default();
    let selected = selection(&cfg, None).selected;
    let mut report = UpdateReport::default();
    for m in targets {
        let pin = m.commit.clone().or_else(|| manifest.marketplaces.get(&m.name)
            .filter(|r| r.source == m.source && r.git_ref == m.git_ref).map(|r| r.commit.clone()));
        let now = Instant::now();
        let one = Update { env, m, pin, accept, team: layer_of(&m.name) == Layer::Team, end: now + TOTAL, selected: &selected, manifest: &manifest };
        if let Err(e) = one.run(&mut report) { report.failures.push((m.name.clone(), e.to_string())); }
    }
    if sync && !report.moves.is_empty() {
        let r = sync_with(&(env.load)()?, env.paths, TOTAL, env.git);
        for l in &r.lines { if !report.lines.contains(l) { report.lines.push(l.clone()); } }
        report.sync = Some(r);
    }
    Ok(report)
}

/// One marketplace of an update.
struct Update<'a> {
    env: &'a Env<'a>, m: &'a zygo_config::Marketplace, pin: Option<String>, accept: bool, team: bool, end: Instant,
    selected: &'a std::collections::BTreeSet<PluginId>, manifest: &'a store::Manifest,
}

impl Update<'_> {
    fn run(&self, report: &mut UpdateReport) -> anyhow::Result<()> {
        let (env, m) = (self.env, self.m);
        let step = || (Instant::now() + env.git.per_marketplace).min(self.end);
        let url = clone_url(&m.source)?;
        let mdir = &env.paths.marketplaces_dir;
        std::fs::create_dir_all(mdir)?;
        let git = Git::new(env.git);
        g(git.check_version(mdir, step()))?;
        let probe = Probe::clone(&git, mdir, &url, step())?;
        let tags = probe.tags(step())?;
        let current = semver::Version::parse(m.git_ref.strip_prefix('v').unwrap_or(&m.git_ref)).ok();
        let mut now = None;
        if !m.git_ref.is_empty() {
            let (commit, is_tag) = probe.resolve(&m.git_ref, step())?;
            if !is_tag {
                report.lines.push(format!("{}: pinned to commit {}; nothing to update", esc(&m.name), &commit[..7]));
                return Ok(());
            }
            now = Some(commit);
        }
        let above = if m.git_ref.is_empty() { highest(&tags, None) } else { current.as_ref().and_then(|c| highest(&tags, Some(c))) };
        let (to_ref, to_commit) = match (above, now) {
            (Some(tag), _) => {
                let commit = probe.resolve(&tag, step())?.0;
                (tag, commit)
            }
            (None, Some(commit)) if self.pin.as_ref().is_some_and(|p| *p != commit) => {
                if !self.accept {
                    report.lines.push(format!("marketplace {0}: tag {1} moved; run zygo marketplace update {0} --accept", esc(&m.name), esc(&m.git_ref)));
                    return Ok(());
                }
                (m.git_ref.clone(), commit)
            }
            _ => {
                report.current.push(m.name.clone());
                return Ok(());
            }
        };
        let check = probe.checkout_check(&to_commit, step())?;
        let entry = Entry { name: m.name.clone(), source: m.source.clone(), git_ref: to_ref.clone(), commit: to_commit.clone(),
            default_on: self.team.then(|| m.default_on.clone()) };
        {
            let _lock = lock_marketplace(env.paths, &m.name, self.end)?.ok_or_else(|| anyhow!("marketplace {} is locked; try again", m.name))?;
            edit_user_config(&env.paths.config_file, |doc| put_entry(doc, &entry))?;
        }
        report.moves.push(Move { name: m.name.clone(), from_ref: m.git_ref.clone(), to_ref, from_commit: self.pin.clone().unwrap_or_default(), to_commit });
        for p in check.plugins.iter().filter(|p| p.violations.is_empty()) {
            let id = PluginId { marketplace: m.name.clone(), plugin: p.name.clone() };
            if !self.selected.contains(&id) { continue; }
            let old = self.manifest.plugins.iter().find(|e| e.marketplace == id.marketplace && e.plugin == id.plugin);
            if old.is_some_and(|o| o.version == p.version && o.digest == p.digest) { continue; }
            report.changes.push(Change { id, old_version: old.map(|o| o.version.clone()), new_version: p.version.clone(),
                old_digest: old.and_then(|o| o.digest.clone()), new_digest: p.digest.clone(), block: render_plugin(p) });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub name: String, pub source: String, pub git_ref: String, pub commit: String, pub origin: &'static str,
    pub plugins: Vec<String>,
}

impl Row {
    /// `<name> <source> <ref> <commit> <origin> plugins: <a, b>`; `-` for an empty ref, commit or plugin list.
    pub fn line(&self) -> String {
        let dash = |s: String| if s.is_empty() { "-".to_string() } else { s };
        let plugins = self.plugins.iter().map(|p| esc(p)).collect::<Vec<_>>().join(", ");
        format!("{} {} {} {} {} plugins: {}", esc(&self.name), esc(&zygo_config::display::display_source(&self.source)), dash(esc(&self.git_ref)), dash(esc(&self.commit)),
            self.origin, dash(plugins))
    }
}

/// D12: one row per merged entry; the commit is the configured one, else the manifest's for the same source and ref.
pub fn list_in(env: &Env) -> anyhow::Result<Vec<Row>> {
    let cfg = (env.load)()?;
    let manifest = store::read_manifest(&env.paths.plugin_store).ok().flatten().unwrap_or_default();
    Ok(cfg.config.marketplaces.iter().map(|m| {
        let origin = match cfg.source_of(&format!("marketplace.{}", key_seg(&m.name))) {
            Layer::Builtin => "built-in",
            Layer::Team => "team",
            _ => "user",
        };
        let commit = m.commit.clone().or_else(|| manifest.marketplaces.get(&m.name)
            .filter(|r| r.source == m.source && r.git_ref == m.git_ref).map(|r| r.commit.clone())).unwrap_or_default();
        let listed = valid_name(&m.name).then(|| std::fs::read(env.paths.marketplaces_dir.join(&m.name).join(".claude-plugin/marketplace.json")).ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())).flatten();
        let plugins = listed.as_ref().and_then(|v| v.get("plugins")?.as_array().map(|a| {
            a.iter().filter_map(|p| p.get("name")?.as_str().map(str::to_string)).collect()
        })).unwrap_or_default();
        Row { name: m.name.clone(), source: m.source.clone(), git_ref: m.git_ref.clone(), commit, origin, plugins }
    }).collect())
}

/// The user config file has a `[[marketplace]]` named `name`; a missing file has none.
fn user_has_entry(config_file: &Path, name: &str) -> anyhow::Result<bool> {
    let text = match std::fs::read_to_string(config_file) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(anyhow!("{}: {e}", config_file.display())),
    };
    let t = text.parse::<toml::Table>().map_err(|e| anyhow!("user config {}: {}", config_file.display(), zygo_config::repo_file::syntax_error(&text, e.message(), e.span())))?;
    let list = t.get("marketplace").and_then(toml::Value::as_array).map(Vec::as_slice).unwrap_or_default();
    Ok(list.iter().any(|m| m.get("name").and_then(toml::Value::as_str) == Some(name)))
}

/// The `[[marketplace]]` named `name` removed from `doc`; the key goes when it was the last.
fn drop_entry(doc: &mut toml_edit::DocumentMut, name: &str) -> anyhow::Result<()> {
    let Some(item) = doc.get_mut("marketplace") else { return Ok(()) };
    let list = item.as_array_of_tables_mut().ok_or_else(|| anyhow!("user config: marketplace is not an array of tables"))?;
    let at = list.iter().position(|t| t.get("name").and_then(toml_edit::Item::as_str) == Some(name));
    if let Some(i) = at { list.remove(i); }
    if list.is_empty() { doc.remove("marketplace"); }
    Ok(())
}

/// One `[[marketplace]]` the commands write.
#[derive(Debug, Clone, Default)]
pub(crate) struct Entry { pub name: String, pub source: String, pub git_ref: String, pub commit: String, pub default_on: Option<Vec<String>> }

/// `e` written into `doc`'s `[[marketplace]]`; `true` = it replaced an entry of that name.
pub(crate) fn put_entry(doc: &mut toml_edit::DocumentMut, e: &Entry) -> anyhow::Result<bool> {
    use toml_edit::{value, ArrayOfTables, Item, Table};
    if !hex40(&e.commit) { bail!("commit is not a 40-hex id"); }
    if doc.get("marketplace").is_none() { doc["marketplace"] = Item::ArrayOfTables(ArrayOfTables::new()); }
    let list = doc["marketplace"].as_array_of_tables_mut().ok_or_else(|| anyhow!("user config: marketplace is not an array of tables"))?;
    let at = list.iter().position(|t| t.get("name").and_then(Item::as_str) == Some(e.name.as_str()));
    let mut fresh = Table::new();
    let t = match at { Some(i) => list.get_mut(i).expect("position is in range"), None => &mut fresh };
    for (k, v) in [("name", &e.name), ("source", &e.source), ("ref", &e.git_ref), ("commit", &e.commit)] { t[k] = value(v.as_str()); }
    if let (None, Some(on)) = (at, &e.default_on) { t["default_on"] = value(on.iter().map(String::as_str).collect::<toml_edit::Array>()); }
    if at.is_none() { list.push(fresh); }
    Ok(at.is_some())
}

/// D11: removes the user entry of `name`; a built-in or team entry with no user entry is refused.
pub fn remove_in(env: &Env, name: &str) -> anyhow::Result<()> {
    let cfg = (env.load)()?;
    if !user_has_entry(&env.paths.config_file, name)? {
        if !cfg.config.marketplaces.iter().any(|m| m.name == name) { bail!("marketplace {} is not configured", esc(name)); }
        let from = match cfg.source_of(&format!("marketplace.{}", key_seg(name))) {
            Layer::Builtin => "is built in",
            _ => "is from the team config",
        };
        bail!("marketplace {0} {from} and cannot be removed; disable its plugins instead: zygo plugins disable {0}:<plugin>", esc(name));
    }
    // An invalid name has no cache or pin to guard, and `lock_marketplace` refuses it.
    if !valid_name(name) { return edit_user_config(&env.paths.config_file, |doc| drop_entry(doc, name)); }
    let _lock = lock_marketplace(env.paths, name, Instant::now() + TOTAL)?
        .ok_or_else(|| anyhow!("marketplace {name} is locked; try again"))?;
    edit_user_config(&env.paths.config_file, |doc| drop_entry(doc, name))
}

/// `CheckReport::render` of the one plugin, with `  license <license|->` after its first line.
pub fn render_plugin(p: &PluginReport) -> String {
    let one = CheckReport { marketplace: None, violations: vec![], plugins: vec![p.clone()], skipped: vec![], warnings: vec![] };
    let text = one.render();
    let (first, rest) = text.split_once('\n').unwrap_or((&text, ""));
    format!("{first}\n  license {}\n{rest}", p.license.as_deref().map_or("-".to_string(), esc))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_entry_refuses_a_commit_that_is_not_40_hex() {
        let mut doc = toml_edit::DocumentMut::new();
        let e = Entry { commit: "abc".into(), ..Default::default() };
        assert_eq!(put_entry(&mut doc, &e).unwrap_err().to_string(), "commit is not a 40-hex id");
        assert_eq!(doc.to_string(), "");
    }

    #[test]
    fn user_has_entry_syntax_error_does_not_echo_the_source() {
        let value = format!("sk-ant-{}", zygo_core::ids::ulid());
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("config.toml");
        std::fs::write(&f, format!("[user]\nkey = {value}\n")).unwrap();
        let r = user_has_entry(&f, "m");
        let e = r.unwrap_err().to_string();
        assert!(e.starts_with("user config ") && e.contains(": TOML parse error at line 2, column "), "{e}");
        assert!(!e.contains(&value) && !e.contains("sk-ant"), "{e}");
    }
}
