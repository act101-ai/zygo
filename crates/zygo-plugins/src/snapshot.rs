//! The per-session plugin snapshot (spec §15.3, §15.5, §15.7): the delivery gate over the store, the Claude Code
//! snapshot under the shared store lock, and the missing-command rule. OpenCode delivery is the adapter's.

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{anyhow, bail, Context};
use serde_json::{Map, Value};
use zygo_core::paths::Paths;
use zygo_core::Agent;

use crate::approve::is_approved_in;
use crate::embedded;
use crate::select::{PluginId, Selection};
use crate::store::{self, ManifestState, StoreLock, StoreState, StoredEntry};

pub(crate) const BUSY: &str = "store lock busy";
const MANIFEST_INVALID: &str = "store manifest is invalid; run zygo plugins sync";
const WRITE_FAILED: &str = "cannot write the session snapshot";
const MCP: &str = ".mcp.json";
const PARTIAL: &str = "plugins: partial";
const ROOT_VAR: &str = "${CLAUDE_PLUGIN_ROOT}";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotPlugin {
    pub marketplace: String,
    pub name: String,
    pub dir: Option<PathBuf>,
    pub skills: Vec<String>,
    pub agents: Vec<String>,
    pub commands: Vec<String>,
    pub mcp: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub plugins: Vec<SnapshotPlugin>,
    /// The launch lines (D2).
    pub lines: Vec<String>,
    /// Full text of the conditions counted as partial.
    pub details: Vec<String>,
}

impl Snapshot {
    /// The `--plugin-dir` list, embedded plugin first.
    pub fn plugin_dirs(&self) -> Vec<PathBuf> { self.plugins.iter().filter_map(|p| p.dir.clone()).collect() }
}

#[derive(Debug, Clone)]
pub struct DeliveredPlugin { pub id: PluginId, pub entry: StoredEntry, pub dir: PathBuf }

/// Approved, intact, selected plugins. `lock` is the shared store lock: read the files only while it is held.
#[derive(Debug)]
pub struct Delivered {
    pub plugins: Vec<DeliveredPlugin>,
    pub lines: Vec<String>,
    pub details: Vec<String>,
    pub lock: StoreLock,
}

/// No code, or the digest is in the approvals file or pre-approved with the full `(marketplace, plugin, digest)` triple.
pub(crate) fn code_approved(paths: &Paths, sel: &Selection, id: &PluginId, digest: Option<&str>) -> bool {
    digest.is_none_or(|digest| {
        is_approved_in(&paths.approvals_file, &id.marketplace, &id.plugin, digest)
            || sel.pre_approved.iter().any(|a| a.marketplace == id.marketplace && a.plugin == id.plugin && a.digest == digest)
    })
}

/// The detail for a store-lock error: the I/O error kind, without paths.
pub(crate) fn lock_failed(e: &anyhow::Error) -> String {
    let kind = e.chain().find_map(|c| c.downcast_ref::<std::io::Error>()).map_or(std::io::ErrorKind::Other, std::io::Error::kind);
    format!("store lock failed: {kind}")
}

/// D1. `None` = the shared store lock was not obtained before `deadline`.
fn deliver(paths: &Paths, sel: &Selection, deadline: Instant) -> anyhow::Result<Option<Delivered>> {
    let Some(lock) = store::lock_until(&paths.plugin_store, false, deadline)? else { return Ok(None) };
    let state = store::state(&paths.plugin_store);
    Ok(Some(deliver_from(paths, sel, &state, lock)))
}

/// D1 over `state`, read while `lock` (kept in the result) is held.
pub(crate) fn deliver_from(paths: &Paths, sel: &Selection, state: &StoreState, lock: StoreLock) -> Delivered {
    let mut d = Delivered { plugins: vec![], lines: vec![], details: vec![], lock };
    if matches!(state.manifest, ManifestState::Invalid(_)) { d.details.push(MANIFEST_INVALID.into()); }
    let mut names: Vec<(String, String)> = vec![];
    for id in &sel.selected {
        let Some(p) = state.plugins.iter().find(|p| p.entry.marketplace == id.marketplace && p.entry.plugin == id.plugin) else { continue };
        if sel.sources.get(&id.marketplace) != Some(&p.source) { continue; }
        if !p.intact {
            d.details.push(format!("plugin {id}: store copy differs from its manifest; run zygo plugins sync"));
            continue;
        }
        if id.plugin == embedded::NAME {
            d.details.push(format!("plugin {id}: name {} is reserved", embedded::NAME));
            continue;
        }
        if let Some((m, _)) = names.iter().find(|(_, n)| *n == id.plugin) {
            d.details.push(format!("plugin {id}: name already delivered from {m}"));
            continue;
        }
        if !code_approved(paths, sel, id, p.entry.digest.as_deref()) {
            d.lines.push(format!("plugin {id}: code needs approval; run zygo plugins approve {id}"));
            continue;
        }
        names.push((id.marketplace.clone(), id.plugin.clone()));
        d.plugins.push(DeliveredPlugin { id: id.clone(), entry: p.entry.clone(), dir: p.dir.clone() });
    }
    d
}

/// D1: the selected plugins that may be delivered, holding the shared store lock until dropped.
/// `Err("store lock busy")` when the lock is not obtained.
pub fn delivered_in(paths: &Paths, sel: &Selection, deadline: Instant) -> anyhow::Result<Delivered> {
    deliver(paths, sel, deadline)?.ok_or_else(|| anyhow!(BUSY))
}

/// Skill, agent and command names of a plugin's files (default locations).
pub(crate) fn parts<'a>(files: impl Iterator<Item = &'a str>) -> (Vec<String>, Vec<String>, Vec<String>) {
    let (mut skills, mut agents, mut commands) = (BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
    for f in files {
        let segs: Vec<&str> = f.split('/').collect();
        match segs.as_slice() {
            ["skills", name, "SKILL.md"] => { skills.insert(name.to_string()); }
            ["agents", file] => { if let Some(n) = file.strip_suffix(".md") { agents.insert(n.to_string()); } }
            ["commands", file] => { if let Some(n) = file.strip_suffix(".md") { commands.insert(n.to_string()); } }
            _ => {}
        }
    }
    (skills.into_iter().collect(), agents.into_iter().collect(), commands.into_iter().collect())
}

fn write_file(dest: &Path, rel: &str, bytes: &[u8], exec: bool) -> anyhow::Result<()> {
    let p = rel.split('/').fold(dest.to_path_buf(), |p, s| p.join(s));
    if let Some(d) = p.parent() { std::fs::create_dir_all(d).with_context(|| d.display().to_string())?; }
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, if exec { 0o755 } else { 0o644 });
    let mut f = o.open(&p).with_context(|| p.display().to_string())?;
    f.write_all(bytes).with_context(|| p.display().to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        f.set_permissions(std::fs::Permissions::from_mode(if exec { 0o755 } else { 0o644 })).with_context(|| p.display().to_string())?;
    }
    #[cfg(not(unix))]
    let _ = exec;
    Ok(())
}

fn fresh_dir(dir: &Path) -> anyhow::Result<()> {
    match std::fs::remove_dir_all(dir) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e).with_context(|| dir.display().to_string()),
    }
    std::fs::create_dir_all(dir).with_context(|| dir.display().to_string())
}

/// §15.3 missing commands: a path with `..` is never found; an absolute path is an existing regular file (with an
/// execute bit on unix); a bare name is looked up in the absolute entries of `search_path` (a relative entry such
/// as `.` or an empty one would resolve inside the plugin); a relative path resolves against the plugin root.
pub fn command_found(command: &str, plugin_root: &Path, search_path: Option<&OsStr>) -> bool {
    // A verbatim root (`\\?\C:\…`) does not read `/` as a separator: substitute the plain form.
    let expanded = command.replace(ROOT_VAR, &zygo_core::paths::plain(plugin_root).to_string_lossy());
    if expanded.is_empty() || Path::new(&expanded).components().any(|c| matches!(c, std::path::Component::ParentDir)) {
        return false;
    }
    let path = Path::new(&expanded);
    if path.is_absolute() {
        return std::fs::metadata(path).is_ok_and(|m| m.is_file() && executable(&m));
    }
    let entries: Vec<PathBuf> = search_path.map(|s| std::env::split_paths(s).filter(|p| p.is_absolute()).collect()).unwrap_or_default();
    if path.components().count() == 1 && entries.is_empty() { return false; }
    which::which_in(&expanded, std::env::join_paths(entries).ok(), plugin_root).is_ok()
}

#[cfg(unix)]
fn executable(m: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    m.permissions().mode() & 0o111 != 0
}
#[cfg(not(unix))]
fn executable(_m: &std::fs::Metadata) -> bool { true }

/// The servers object of a `.mcp.json` value: `mcpServers`, or the top-level map.
fn servers_mut(v: &mut Value) -> Option<&mut Map<String, Value>> {
    if v.get("mcpServers").is_some_and(Value::is_object) { v.get_mut("mcpServers")?.as_object_mut() } else { v.as_object_mut() }
}

/// The server names of a `.mcp.json` (empty when it is not JSON).
pub(crate) fn server_names(bytes: &[u8]) -> Vec<String> {
    let Ok(mut v) = serde_json::from_slice::<Value>(bytes) else { return vec![] };
    servers_mut(&mut v).map(|m| m.keys().cloned().collect()).unwrap_or_default()
}

pub(crate) struct Filtered { pub missing: Vec<(String, String)>, pub kept: Vec<String>, pub rewritten: Option<Vec<u8>> }

/// The `.mcp.json` `bytes` of the plugin at `plugin_root`: the servers whose command is not found are listed and
/// removed from `rewritten` (`None` when none is removed or the rewrite fails).
pub(crate) fn filter_mcp(bytes: &[u8], plugin_root: &Path, search_path: Option<&OsStr>) -> Filtered {
    let mut out = Filtered { missing: vec![], kept: vec![], rewritten: None };
    let Ok(mut v) = serde_json::from_slice::<Value>(bytes) else { return out };
    let Some(map) = servers_mut(&mut v) else { return out };
    let mut drop: Vec<String> = vec![];
    for (name, s) in map.iter() {
        match s.get("command").and_then(Value::as_str) {
            Some(c) if !command_found(c, plugin_root, search_path) => {
                out.missing.push((name.clone(), c.to_string()));
                drop.push(name.clone());
            }
            _ => out.kept.push(name.clone()),
        }
    }
    if !drop.is_empty() {
        map.retain(|k, _| !drop.contains(k));
        out.rewritten = serde_json::to_vec_pretty(&v).ok();
    }
    out
}

/// The `.mcp.json` servers of a plugin dir whose command is not found: `(server, command)`.
pub fn missing_commands(plugin_dir: &Path, search_path: Option<&OsStr>) -> Vec<(String, String)> {
    std::fs::read(plugin_dir.join(MCP)).map(|b| filter_mcp(&b, plugin_dir, search_path).missing).unwrap_or_default()
}

fn homepage(plugin_dir: &Path) -> Option<String> {
    let b = std::fs::read(plugin_dir.join(".claude-plugin/plugin.json")).ok()?;
    serde_json::from_slice::<Value>(&b).ok()?.get("homepage")?.as_str().map(str::to_string)
}

/// `snapshot_in` with the paths and `PATH` of the environment and a lock wait of `sync::TOTAL`.
pub fn snapshot(session_dir: &Path, agent: Agent, sel: &Selection) -> anyhow::Result<Snapshot> {
    let path = std::env::var_os("PATH");
    snapshot_in(&Paths::from_env(), session_dir, agent, sel, path.as_deref(), Instant::now() + crate::sync::TOTAL)
}

/// One delivered plugin into the session at `dest`: every manifest file from the store (`read_verified`), the
/// `.mcp.json` last and without the servers whose command is missing. `Err(true)` = the store copy, `Err(false)` =
/// the session write.
fn write_plugin(paths: &Paths, p: &DeliveredPlugin, dest: &Path, search_path: Option<&OsStr>) -> Result<Filtered, bool> {
    std::fs::create_dir(dest).map_err(|_| false)?;
    let mut mcp = None;
    for (rel, fe) in &p.entry.files {
        let bytes = store::read_verified(&paths.plugin_store, &p.entry, rel).map_err(|_| true)?;
        if rel == MCP { mcp = Some((bytes, fe.exec)); continue; }
        write_file(dest, rel, &bytes, fe.exec).map_err(|_| false)?;
    }
    let Some((bytes, exec)) = mcp else { return Ok(Filtered { missing: vec![], kept: vec![], rewritten: None }) };
    let f = filter_mcp(&bytes, dest, search_path);
    let out = if f.missing.is_empty() { Some(&bytes) } else { f.rewritten.as_ref() };
    write_file(dest, MCP, out.ok_or(false)?, exec).map_err(|_| false)?;
    Ok(f)
}

/// D3: `<session>/plugins` cleared, the embedded plugin first, then (under the shared store lock) one dir per
/// delivered plugin. `Ok` whenever the embedded plugin was written.
pub fn snapshot_in(paths: &Paths, session_dir: &Path, agent: Agent, sel: &Selection, search_path: Option<&OsStr>,
    deadline: Instant) -> anyhow::Result<Snapshot> {
    if agent == Agent::OpenCode {
        bail!("OpenCode delivery is made by the OpenCode adapter; use delivered_in");
    }
    let mut snap = Snapshot::default();
    let root = session_dir.join("plugins");
    fresh_dir(&root)?;
    let own = root.join(embedded::NAME);
    let files = embedded::files();
    for (rel, bytes) in &files { write_file(&own, rel, bytes, false)?; }
    let (skills, agents, commands) = parts(files.iter().map(|(n, _)| n.as_str()));
    snap.plugins.push(SnapshotPlugin {
        marketplace: "embedded".into(), name: embedded::NAME.into(), dir: Some(own), skills, agents, commands, mcp: vec![],
    });

    let delivered = match deliver(paths, sel, deadline) {
        Ok(Some(d)) => d,
        // §15.5 "always in the snapshot": a busy lock or a lock I/O error leaves the embedded plugin only.
        Ok(None) => {
            snap.lines.push(PARTIAL.into());
            snap.details.push(BUSY.into());
            return Ok(snap);
        }
        Err(e) => {
            snap.lines.push(PARTIAL.into());
            snap.details.push(lock_failed(&e));
            return Ok(snap);
        }
    };
    snap.details.extend(delivered.details.iter().cloned());
    snap.lines.extend(delivered.lines.iter().cloned());
    let mut missing_lines = vec![];
    for p in &delivered.plugins {
        let dest = root.join(&p.id.plugin);
        let f = match write_plugin(paths, p, &dest, search_path) {
            Ok(f) => f,
            Err(store_side) => {
                let _ = std::fs::remove_dir_all(&dest);
                snap.details.push(if store_side {
                    format!("plugin {}: store copy differs from its manifest; run zygo plugins sync", p.id)
                } else {
                    format!("plugin {}: {WRITE_FAILED}", p.id)
                });
                continue;
            }
        };
        let home = homepage(&dest);
        let mut seen: Vec<&str> = vec![];
        for (_, c) in &f.missing {
            if seen.contains(&c.as_str()) { continue; }
            seen.push(c);
            missing_lines.push(match &home {
                Some(h) => format!("plugin {}: {} not found ({})", p.id, zygo_core::esc(c), zygo_core::esc(h)),
                None => format!("plugin {}: {} not found", p.id, zygo_core::esc(c)),
            });
        }
        let (skills, agents, commands) = parts(p.entry.files.keys().map(String::as_str));
        snap.plugins.push(SnapshotPlugin {
            marketplace: p.id.marketplace.clone(), name: p.id.plugin.clone(), dir: Some(dest), skills, agents, commands, mcp: f.kept,
        });
    }
    snap.lines.extend(missing_lines);
    if !snap.details.is_empty() { snap.lines.push(PARTIAL.into()); }
    Ok(snap)
}
