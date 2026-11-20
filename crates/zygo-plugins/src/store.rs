//! The plugin store (spec §15.3, §15.6): `<root>/<marketplace>/<plugin>/<version>/` plus `manifest.json`,
//! staged copies checked on the copy, swapped in under the store lock, verified on every read.

use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::check::{check_dir_modes, valid_name, CheckReport, Rule, Violation};

/// §15.6 limits on raw bytes.
const FILE_LIMIT: u64 = 1_048_576;
const PLUGIN_LIMIT: u64 = 4_194_304;
const MANIFEST: &str = "manifest.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest { pub version: u32, pub marketplaces: BTreeMap<String, PinEntry>, pub plugins: Vec<StoredEntry> }
impl Default for Manifest {
    fn default() -> Self { Manifest { version: 1, marketplaces: BTreeMap::new(), plugins: vec![] } }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinEntry {
    pub source: String,
    #[serde(rename = "ref")]
    pub git_ref: String,
    pub commit: String,
    /// The config layer of the entry at its last full sync (`builtin`, `team`, `user`); `""` = unknown.
    #[serde(default)]
    pub layer: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry { pub sha256: String, pub exec: bool }

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredEntry {
    pub marketplace: String,
    pub plugin: String,
    pub version: String,
    /// The marketplace commit this copy was taken from (empty in a manifest written before round 2).
    #[serde(default)]
    pub commit: String,
    pub digest: Option<String>,
    pub files: BTreeMap<String, FileEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestState { Missing, Valid, Invalid(String) }

#[derive(Debug, Clone)]
pub struct StoredPlugin { pub entry: StoredEntry, pub source: String, pub dir: PathBuf, pub intact: bool }

#[derive(Debug, Clone)]
pub struct StoreState { pub manifest: ManifestState, pub plugins: Vec<StoredPlugin> }

pub fn entry_dir(root: &Path, e: &StoredEntry) -> PathBuf { root.join(&e.marketplace).join(&e.plugin).join(&e.version) }

/// A manifest path: non-empty, `/`-separated, no `\`, not absolute, no drive, every segment a real name and
/// not `.git` (any case), no Windows device name, no `:`.
fn safe_rel(rel: &str) -> bool {
    let b = rel.as_bytes();
    !rel.is_empty() && !rel.contains('\\') && !rel.starts_with('/') && !(b.len() >= 2 && b[1] == b':')
        && rel.split('/').all(|s| !s.is_empty() && s != "." && s != ".." && !crate::check::is_dot_git(s) && !crate::check::windows_unsafe(s))
}

/// Names that make `entry_dir` a path inside the store.
fn entry_ok(e: &StoredEntry) -> bool {
    valid_name(&e.marketplace) && valid_name(&e.plugin) && semver::Version::parse(&e.version).is_ok()
        && e.version.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'+'))
}

/// `base/rel` opened for reading when every component under `base` is a real directory and the last a regular
/// file (no symlink anywhere; unix opens with `O_NOFOLLOW`). `Err(true)` = a symlink or non-file, `Err(false)` = I/O.
fn open_nofollow(base: &Path, rel: &str) -> Result<File, bool> {
    let segs: Vec<&str> = rel.split('/').collect();
    let mut p = base.to_path_buf();
    for (i, s) in segs.iter().enumerate() {
        p.push(s);
        let m = std::fs::symlink_metadata(&p).map_err(|_| false)?;
        let last = i + 1 == segs.len();
        if m.file_type().is_symlink() || (last && !m.is_file()) || (!last && !m.is_dir()) { return Err(true); }
    }
    let mut o = std::fs::OpenOptions::new();
    o.read(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::custom_flags(&mut o, libc::O_NOFOLLOW);
    let f = o.open(&p).map_err(|_| false)?;
    if !f.metadata().map_err(|_| false)?.is_file() { return Err(true); }
    Ok(f)
}

/// Reads at most `FILE_LIMIT + 1` bytes.
fn read_limited(f: File) -> std::io::Result<Vec<u8>> {
    let mut b = vec![];
    f.take(FILE_LIMIT + 1).read_to_end(&mut b)?;
    Ok(b)
}

#[cfg(unix)]
fn exec_bit(f: &File) -> bool {
    use std::os::unix::fs::PermissionsExt;
    f.metadata().is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
}
#[cfg(not(unix))]
fn exec_bit(_f: &File) -> bool { false }

fn sha(b: &[u8]) -> String { hex::encode(Sha256::digest(b)) }

/// The stored dirs `<m>`, `<m>/<p>`, `<m>/<p>/<v>` are real directories (no symlink).
fn dirs_real(root: &Path, e: &StoredEntry) -> bool {
    let mut p = root.to_path_buf();
    [&e.marketplace, &e.plugin, &e.version].iter().all(|s| {
        p.push(s);
        std::fs::symlink_metadata(&p).is_ok_and(|m| m.is_dir())
    })
}

/// D8: safe names and paths, regular files only, SHA-256 of each file, exec bit (unix) as recorded.
pub fn verify(root: &Path, e: &StoredEntry) -> bool {
    if !entry_ok(e) || !dirs_real(root, e) { return false; }
    let dir = entry_dir(root, e);
    e.files.iter().all(|(rel, fe)| {
        if !safe_rel(rel) { return false; }
        let Ok(f) = open_nofollow(&dir, rel) else { return false };
        let exec = exec_bit(&f);
        let ok_exec = !cfg!(unix) || exec == fe.exec;
        read_limited(f).is_ok_and(|b| b.len() as u64 <= FILE_LIMIT && sha(&b) == fe.sha256) && ok_exec
    })
}

/// One stored file: safe path, listed in the entry, no symlink, ≤ 1 MiB, hash equal.
pub fn read_verified(root: &Path, e: &StoredEntry, rel: &str) -> anyhow::Result<Vec<u8>> {
    let fe = e.files.get(rel).filter(|_| safe_rel(rel) && entry_ok(e) && dirs_real(root, e))
        .ok_or_else(|| anyhow!("{rel}: not a stored file"))?;
    let f = open_nofollow(&entry_dir(root, e), rel).map_err(|_| anyhow!("{rel}: not a regular file in the store"))?;
    let b = read_limited(f).with_context(|| rel.to_string())?;
    if b.len() as u64 > FILE_LIMIT || sha(&b) != fe.sha256 { bail!("{rel}: store copy differs from its manifest"); }
    Ok(b)
}

/// The manifest's exec flags as git index modes.
fn modes_of(e: &StoredEntry) -> HashMap<String, String> {
    e.files.iter().map(|(k, f)| (k.clone(), if f.exec { "100755" } else { "100644" }.to_string())).collect()
}

/// `verify`, then a fresh check of the stored copy (modes from the manifest) whose digest equals the manifest's.
pub fn inspect(root: &Path, e: &StoredEntry) -> anyhow::Result<CheckReport> {
    let differs = || anyhow!("store copy differs from its manifest");
    if !verify(root, e) { return Err(differs()); }
    let r = check_dir_modes(&entry_dir(root, e), Instant::now() + Duration::from_secs(2), Some(modes_of(e)));
    let one = r.plugins.len() == 1 && r.plugins[0].name == e.plugin && r.plugins[0].version == e.version;
    if !r.ok() || !one || r.plugins[0].digest != e.digest { return Err(differs()); }
    Ok(r)
}

pub fn read_manifest(root: &Path) -> anyhow::Result<Option<Manifest>> {
    let p = root.join(MANIFEST);
    let bytes = match std::fs::read(&p) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| p.display().to_string()),
    };
    let m: Manifest = serde_json::from_slice(&bytes).with_context(|| p.display().to_string())?;
    if m.version != 1 { bail!("{}: unknown version {}", p.display(), m.version); }
    Ok(Some(m))
}

/// The whole manifest, temp file plus rename.
pub fn write_manifest(root: &Path, m: &Manifest) -> anyhow::Result<()> {
    std::fs::create_dir_all(root).with_context(|| root.display().to_string())?;
    let tmp = root.join(format!(".{MANIFEST}.{}.tmp", zygo_core::ids::ulid()));
    let res = (|| -> std::io::Result<()> {
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        f.write_all(&serde_json::to_vec_pretty(m).map_err(std::io::Error::other)?)?;
        f.sync_all()?;
        std::fs::rename(&tmp, root.join(MANIFEST))
    })();
    if let Err(e) = res {
        let _ = std::fs::remove_file(&tmp);
        return Err(e).with_context(|| root.join(MANIFEST).display().to_string());
    }
    Ok(())
}

pub fn state(root: &Path) -> StoreState {
    match read_manifest(root) {
        Ok(None) => StoreState { manifest: ManifestState::Missing, plugins: vec![] },
        Err(e) => StoreState { manifest: ManifestState::Invalid(format!("{e:#}")), plugins: vec![] },
        Ok(Some(m)) => StoreState {
            manifest: ManifestState::Valid,
            plugins: m.plugins.iter().map(|e| StoredPlugin {
                entry: e.clone(),
                source: m.marketplaces.get(&e.marketplace).map(|p| p.source.clone()).unwrap_or_default(),
                dir: entry_dir(root, e),
                intact: verify(root, e),
            }).collect(),
        },
    }
}

/// A checked copy waiting to be swapped in.
#[derive(Debug)]
pub struct Staged { pub entry: StoredEntry, pub tmp: PathBuf }

/// A swapped-in version dir and where the version it replaced was put aside.
#[derive(Debug, Clone)]
pub struct Swapped { pub target: PathBuf, pub aside: Option<PathBuf> }

fn viol(rule: Rule, path: &str, detail: impl Into<String>) -> Violation {
    Violation { rule, path: path.to_string(), detail: detail.into() }
}

/// Removes `dir` and then its parents up to `root` while they are empty.
fn remove_up(root: &Path, dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
    let mut p = dir.parent();
    while let Some(d) = p {
        if d == root || !d.starts_with(root) || std::fs::remove_dir(d).is_err() { break; }
        p = d.parent();
    }
}

/// D8: copies `copied` (raw bytes, no-follow, §15.6 limits, deterministic modes) from `plugin_dir` to
/// `<root>/<marketplace>/<plugin>/<version>.tmp-<ulid>/`, checks the copy with `modes`, and records what is
/// stored: digest and files from the copy's report, SHA-256 and exec flag per file. Nothing is left on error.
pub fn stage(root: &Path, marketplace: &str, plugin_dir: &Path, copied: &[String], modes: &HashMap<String, String>,
    deadline: Instant) -> Result<Staged, Violation> {
    const PJ: &str = ".claude-plugin/plugin.json";
    if !valid_name(marketplace) { return Err(viol(Rule::BadName, ".", format!("marketplace name {marketplace:?}"))); }
    let pj = open_nofollow(plugin_dir, PJ).map_err(|l| viol(if l { Rule::Symlink } else { Rule::Malformed }, PJ, "unreadable"))?;
    let pj: serde_json::Value = read_limited(pj).ok().and_then(|b| serde_json::from_slice(&b).ok())
        .ok_or_else(|| viol(Rule::Malformed, PJ, "not JSON"))?;
    let field = |k: &str| pj.get(k).and_then(serde_json::Value::as_str).unwrap_or("").to_string();
    let (name, version) = (field("name"), field("version"));
    let mut entry = StoredEntry { marketplace: marketplace.into(), plugin: name.clone(), version: version.clone(), ..Default::default() };
    if !entry_ok(&entry) { return Err(viol(Rule::BadName, PJ, format!("plugin {name:?} version {version:?}"))); }
    let pdir = root.join(marketplace).join(&name);
    let tmp = pdir.join(format!("{version}.tmp-{}", zygo_core::ids::ulid()));
    let fail = |v: Violation| {
        remove_up(root, &tmp);
        v
    };
    let mut b = std::fs::DirBuilder::new();
    b.recursive(true);
    std::fs::create_dir_all(&pdir).map_err(|e| fail(viol(Rule::Malformed, ".", e.to_string())))?;
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut b, 0o700);
    b.create(&tmp).map_err(|e| fail(viol(Rule::Malformed, ".", e.to_string())))?;

    let mut total = 0u64;
    let mut files: BTreeMap<String, FileEntry> = BTreeMap::new();
    for rel in copied {
        if Instant::now() >= deadline { return Err(fail(viol(Rule::Malformed, ".", "sync deadline reached"))); }
        if !safe_rel(rel) { return Err(fail(viol(Rule::PathOutside, rel, "path leaves the plugin"))); }
        let f = open_nofollow(plugin_dir, rel)
            .map_err(|l| fail(if l { viol(Rule::Symlink, rel, "symlinks are refused") } else { viol(Rule::Malformed, rel, "unreadable") }))?;
        let exec = exec_bit(&f) || modes.get(rel).is_some_and(|m| m == "100755");
        let bytes = read_limited(f).map_err(|e| fail(viol(Rule::Malformed, rel, e.to_string())))?;
        let n = bytes.len() as u64;
        if n > FILE_LIMIT { return Err(fail(viol(Rule::FileTooLarge, rel, format!("over {FILE_LIMIT} bytes")))); }
        total += n;
        if total > PLUGIN_LIMIT { return Err(fail(viol(Rule::PluginTooLarge, ".", format!("over {PLUGIN_LIMIT} bytes")))); }
        let dst = rel.split('/').fold(tmp.clone(), |p, s| p.join(s));
        let res = (|| -> std::io::Result<()> {
            if let Some(d) = dst.parent() { std::fs::create_dir_all(d)?; }
            let mut o = std::fs::OpenOptions::new();
            o.write(true).create_new(true);
            #[cfg(unix)]
            std::os::unix::fs::OpenOptionsExt::mode(&mut o, if exec { 0o755 } else { 0o644 });
            let mut out = o.open(&dst)?;
            out.write_all(&bytes)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                out.set_permissions(std::fs::Permissions::from_mode(if exec { 0o755 } else { 0o644 }))?;
            }
            out.sync_all()
        })();
        res.map_err(|e| fail(viol(Rule::Malformed, rel, e.to_string())))?;
        files.insert(rel.clone(), FileEntry { sha256: sha(&bytes), exec });
    }

    let r = check_dir_modes(&tmp, deadline, Some(modes.clone()));
    if let Some(v) = r.violations.iter().chain(r.plugins.iter().flat_map(|p| p.violations.iter())).next() {
        return Err(fail(v.clone()));
    }
    let [p] = r.plugins.as_slice() else { return Err(fail(viol(Rule::Malformed, ".", "the copy is not one plugin"))) };
    if p.name != name || p.version != version { return Err(fail(viol(Rule::Malformed, PJ, "the copy names another plugin"))); }
    entry.digest = p.digest.clone();
    entry.files = p.copied.iter().filter_map(|c| files.get(c).map(|f| (c.clone(), f.clone()))).collect();
    if entry.files.len() != p.copied.len() { return Err(fail(viol(Rule::Malformed, ".", "the copy holds a file that was not copied"))); }
    Ok(Staged { entry, tmp })
}

/// Removes a staged copy that will not be swapped in.
pub fn discard(s: &Staged) {
    if let Some(root) = s.tmp.parent().and_then(Path::parent).and_then(Path::parent) { remove_up(root, &s.tmp); }
}

/// Renames the staged copy to its version dir; an existing version dir is put aside to `<version>.old-<ulid>`.
pub fn swap_in(root: &Path, s: &Staged) -> anyhow::Result<Swapped> {
    if !entry_ok(&s.entry) { bail!("unsafe store entry"); }
    let target = entry_dir(root, &s.entry);
    let aside = if std::fs::symlink_metadata(&target).is_ok() {
        let a = target.with_file_name(format!("{}.old-{}", s.entry.version, zygo_core::ids::ulid()));
        std::fs::rename(&target, &a).with_context(|| target.display().to_string())?;
        Some(a)
    } else {
        None
    };
    if let Err(e) = std::fs::rename(&s.tmp, &target) {
        if let Some(a) = &aside { let _ = std::fs::rename(a, &target); }
        return Err(e).with_context(|| target.display().to_string());
    }
    Ok(Swapped { target, aside })
}

/// Undoes one `swap_in`: the new version dir goes, the old one comes back.
pub fn rollback(sw: &Swapped) {
    let _ = std::fs::remove_dir_all(&sw.target);
    if let Some(a) = &sw.aside { let _ = std::fs::rename(a, &sw.target); }
}

/// After the new manifest is written: removes put-aside dirs and the dirs of `old` entries that `new` does not
/// hold (superseded versions, dropped plugins). Best effort; a leftover is never in the manifest.
pub fn finish(root: &Path, swapped: Vec<Swapped>, old: &Manifest, new: &Manifest) {
    for sw in swapped {
        if let Some(a) = sw.aside { let _ = std::fs::remove_dir_all(a); }
    }
    let key = |e: &StoredEntry| (e.marketplace.clone(), e.plugin.clone(), e.version.clone());
    for e in &old.plugins {
        if entry_ok(e) && !new.plugins.iter().any(|n| key(n) == key(e)) { remove_up(root, &entry_dir(root, e)); }
    }
}

/// Holds `<store>/.lock`; released on drop.
#[derive(Debug)]
pub struct StoreLock { _file: File }

/// Takes `<root>/.lock` shared or exclusive, trying until `deadline`; `None` when not obtained.
pub fn lock_until(root: &Path, exclusive: bool, deadline: Instant) -> anyhow::Result<Option<StoreLock>> {
    std::fs::create_dir_all(root).with_context(|| root.display().to_string())?;
    let p = root.join(".lock");
    let f = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&p)
        .with_context(|| p.display().to_string())?;
    Ok(try_lock_until(&f, exclusive, deadline).then_some(StoreLock { _file: f }))
}

/// `fs4` try-lock polled every 5 ms until `deadline` (one try when it has passed).
pub(crate) fn try_lock_until(f: &File, exclusive: bool, deadline: Instant) -> bool {
    loop {
        let r = if exclusive { fs4::FileExt::try_lock(f) } else { fs4::FileExt::try_lock_shared(f) };
        if r.is_ok() { return true; }
        if Instant::now() >= deadline { return false; }
        std::thread::sleep(Duration::from_millis(5));
    }
}
