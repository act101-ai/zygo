//! Marketplace sync (spec §15.1, §15.3): one hardened git builder, bounded runs, source forms, pins, and the
//! fetch → check → stage → swap → manifest sequence into the store.

use std::collections::{BTreeSet, HashMap};
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context};
use zygo_config::{Layer, Marketplace, Merged};
use zygo_core::paths::Paths;

use crate::check::{check_dir_modes, git_modes_by, valid_name, Violation};
use crate::select::{selection, PluginId};
use crate::store::{self, Manifest, PinEntry, Staged, StoredEntry, Swapped};

const NULL: &str = if cfg!(windows) { "NUL" } else { "/dev/null" };

/// D1: the config every sync git call gets on its command line (after the user's global config).
const OVERRIDES: &[&str] = &[
    "core.fsmonitor=false", "core.autocrlf=false", "protocol.ext.allow=never", "filter.lfs.process=",
    "filter.lfs.smudge=", "filter.lfs.clean=", "filter.lfs.required=false", "submodule.recurse=false",
];

/// D1: `git` in `cwd` with nothing from a repository's own config able to run: no fsmonitor, no hooks, no
/// LFS filter, no submodules, no `ext::` transport, no inherited repository or config variables. Global and
/// system config are read (credentials, CA, proxy are the user's).
pub fn git_command(cwd: &Path) -> Command { git_command_by(OsStr::new("git"), cwd) }

pub(crate) fn git_command_by(program: &OsStr, cwd: &Path) -> Command {
    let mut c = Command::new(program);
    c.current_dir(cwd).arg("--no-optional-locks");
    c.args(["-c", OVERRIDES[0], "-c"]).arg(format!("core.hooksPath={NULL}"));
    for kv in &OVERRIDES[1..] { c.args(["-c", kv]); }
    for k in ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_COMMON_DIR", "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES", "GIT_CONFIG", "GIT_CONFIG_GLOBAL", "GIT_CONFIG_SYSTEM", "GIT_CONFIG_NOSYSTEM",
        "GIT_CONFIG_COUNT", "GIT_CONFIG_PARAMETERS"] {
        c.env_remove(k);
    }
    for (k, _) in std::env::vars_os() {
        if k.to_str().is_some_and(|k| k.to_ascii_uppercase().starts_with("GIT_CONFIG")) { c.env_remove(k); }
    }
    c.env("GIT_OPTIONAL_LOCKS", "0").env("GIT_TERMINAL_PROMPT", "0").env("GIT_LFS_SKIP_SMUDGE", "1")
        .env("GIT_ALLOW_PROTOCOL", "file:https:ssh").env("LC_ALL", "C");
    if let Some(p) = cwd.parent() { c.env("GIT_CEILING_DIRECTORIES", p); }
    if std::env::var_os("GIT_SSH_COMMAND").is_none() { c.env("GIT_SSH_COMMAND", "ssh -o BatchMode=yes"); }
    c
}

/// D2: `git <args>` in `cwd` through `git_command`, killed with its process group at `deadline`.
/// `Err("git timed out")` (without spawning when the deadline has passed) or `Err("git failed: <first stderr line>")`.
pub fn run_git(cwd: &Path, args: &[&str], deadline: Instant) -> anyhow::Result<Vec<u8>> {
    let mut c = git_command(cwd);
    c.args(args);
    run_bounded(c, deadline)
}

pub(crate) fn run_bounded(mut c: Command, deadline: Instant) -> anyhow::Result<Vec<u8>> {
    if Instant::now() >= deadline { bail!("git timed out"); }
    c.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut c, 0);
    let mut child = c.spawn().map_err(|e| anyhow!("git failed: {e}"))?;
    let read = |r: Option<Box<dyn Read + Send>>| {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut b = vec![];
            if let Some(mut r) = r { let _ = r.read_to_end(&mut b); }
            let _ = tx.send(b);
        });
        rx
    };
    let out = read(child.stdout.take().map(|r| Box::new(r) as Box<dyn Read + Send>));
    let err = read(child.stderr.take().map(|r| Box::new(r) as Box<dyn Read + Send>));
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(5)),
            _ => {
                kill_tree(&mut child);
                bail!("git timed out");
            }
        }
    };
    // A reader still blocked after git exited (a pipe held open elsewhere) is abandoned, not joined.
    let left = || deadline.saturating_duration_since(Instant::now());
    if !status.success() {
        let stderr = err.recv_timeout(left()).unwrap_or_default();
        let text = String::from_utf8_lossy(&stderr);
        let first = text.lines().map(str::trim).find(|l| !l.is_empty()).map(str::to_string).unwrap_or_else(|| status.to_string());
        bail!("git failed: {}", zygo_config::display::scrub_userinfo(&first));
    }
    out.recv_timeout(left()).map_err(|_| anyhow!("git timed out"))
}

/// Kills the child's process group (unix) or process tree (Windows; a descendant that left the tree survives).
fn kill_tree(child: &mut std::process::Child) {
    #[cfg(unix)]
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill").args(["/T", "/F", "/PID", &child.id().to_string()])
            .stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// D3: the git URL of a §15.1 source form; anything else is refused.
pub fn clone_url(source: &str) -> anyhow::Result<String> {
    let part = |p: &str| !p.is_empty() && !p.starts_with('-')
        && p.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    let url = if source.chars().any(char::is_control) {
        None
    } else if let Some(rest) = source.strip_prefix("github:") {
        rest.split_once('/').filter(|(o, r)| part(o) && part(r)).map(|(o, r)| format!("https://github.com/{o}/{r}.git"))
    } else if let Some(rest) = source.strip_prefix("git+https://") {
        Some(format!("https://{rest}"))
    } else if let Some(rest) = source.strip_prefix("git+ssh://") {
        Some(format!("ssh://{rest}"))
    } else if Path::new(source).is_absolute() {
        // git reads `\\?\C:\…` as a host name: give it the plain form.
        zygo_core::paths::plain(Path::new(source)).to_str().map(str::to_string)
    } else {
        None
    };
    url.ok_or_else(|| anyhow!("source is not github:<owner>/<repo>, git+https://, git+ssh:// or an absolute path"))
}

/// `version_output` (`git version 2.50.1 (…)`) is at least `major.minor`.
pub fn git_at_least(version_output: &str, major: u32, minor: u32) -> bool {
    let Some(v) = version_output.trim().strip_prefix("git version ") else { return false };
    let mut parts = v.split(['.', ' ']).map(|p| p.parse::<u32>());
    match (parts.next(), parts.next()) {
        (Some(Ok(a)), Some(Ok(b))) => (a, b) >= (major, minor),
        _ => false,
    }
}

// ---- sync (D4–D11) ----

pub const PER_MARKETPLACE: Duration = Duration::from_secs(5);
pub const TOTAL: Duration = Duration::from_secs(20);

#[derive(Debug, Default)]
pub struct SyncReport {
    /// Only lines the spec defines: moved tag, skipped source, `plugins: offline` / `plugins: partial`.
    pub lines: Vec<String>,
    /// `(<m> or <m>:<p>, reason)`.
    pub failures: Vec<(String, String)>,
    /// Plugins whose store entry is current at the pin after this sync.
    pub synced: Vec<PluginId>,
}

/// Test seam: per-marketplace deadline, git program, and the global git config file.
#[derive(Debug, Clone)]
pub struct SyncOpts { pub per_marketplace: Duration, pub git_program: Option<PathBuf>, pub git_config_global: Option<PathBuf> }

impl Default for SyncOpts {
    fn default() -> Self { SyncOpts { per_marketplace: PER_MARKETPLACE, git_program: None, git_config_global: None } }
}

/// `sync_all_in` with the paths of the environment.
pub fn sync_all(cfg: &Merged, budget: Duration) -> SyncReport { sync_all_in(cfg, &Paths::from_env(), budget) }

pub fn sync_all_in(cfg: &Merged, paths: &Paths, budget: Duration) -> SyncReport { sync_with(cfg, paths, budget, &SyncOpts::default()) }

/// Every configured marketplace (a built-in entry without `ref` is skipped), each selected plugin.
pub fn sync_with(cfg: &Merged, paths: &Paths, budget: Duration, opts: &SyncOpts) -> SyncReport {
    engine(cfg, paths, budget, opts, None)
}

/// One plugin: its entry is replaced or added; every other entry and the recorded commit stay.
pub fn sync_one_in(cfg: &Merged, paths: &Paths, budget: Duration, id: &PluginId) -> SyncReport {
    sync_one_with(cfg, paths, budget, &SyncOpts::default(), id)
}

/// `sync_one_in` with explicit options (test seam).
pub fn sync_one_with(cfg: &Merged, paths: &Paths, budget: Duration, opts: &SyncOpts, id: &PluginId) -> SyncReport {
    engine(cfg, paths, budget, opts, Some(id))
}

/// Holds `<marketplaces_dir>/<name>.lock`; released on drop.
#[derive(Debug)]
pub struct MarketplaceLock { _file: File }

/// D7: the marketplace lock, tried until `deadline`; `None` when not obtained. N40 takes it for add/update.
pub fn lock_marketplace(paths: &Paths, name: &str, deadline: Instant) -> anyhow::Result<Option<MarketplaceLock>> {
    if !valid_name(name) { bail!("unsafe marketplace name"); }
    let dir = &paths.marketplaces_dir;
    std::fs::create_dir_all(dir).with_context(|| dir.display().to_string())?;
    let p = dir.join(format!("{name}.lock"));
    let f = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&p)
        .with_context(|| p.display().to_string())?;
    Ok(store::try_lock_until(&f, true, deadline).then_some(MarketplaceLock { _file: f }))
}

/// The git program and global config every sync git call uses.
pub(crate) struct Git { program: OsString, global: Option<PathBuf>, version: std::cell::OnceCell<Result<(), String>> }

impl Git {
    pub(crate) fn new(opts: &SyncOpts) -> Git {
        Git {
            program: opts.git_program.clone().map(PathBuf::into_os_string).unwrap_or_else(|| "git".into()),
            global: opts.git_config_global.clone(),
            version: Default::default(),
        }
    }

    pub(crate) fn command(&self, cwd: &Path) -> Command {
        let mut c = git_command_by(&self.program, cwd);
        if let Some(g) = &self.global { c.env("GIT_CONFIG_GLOBAL", g).env("GIT_CONFIG_NOSYSTEM", "1"); }
        c
    }

    pub(crate) fn run(&self, cwd: &Path, args: &[&str], deadline: Instant) -> Result<String, String> {
        let mut c = self.command(cwd);
        c.args(args);
        run_bounded(c, deadline).map(|b| String::from_utf8_lossy(&b).trim().to_string()).map_err(|e| e.to_string())
    }

    /// `checkout --detach --force <commit>` in `cwd`. Git 2.30–2.42 refuses `--end-of-options` in `checkout` ("does
    /// not take a path argument"), so `commit` must be a hex object id (40 or 64 hex): it never reads as an option.
    pub(crate) fn checkout_detached(&self, cwd: &Path, commit: &str, deadline: Instant) -> Result<String, String> {
        let hex = matches!(commit.len(), 40 | 64) && commit.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
        if !hex { return Err("git failed: commit is not a hex object id".into()); }
        self.run(cwd, &["checkout", "--quiet", "--detach", "--force", commit], deadline)
    }

    /// D2: once per sync, before the first git call of the first attempted marketplace.
    pub(crate) fn check_version(&self, cwd: &Path, deadline: Instant) -> Result<(), String> {
        self.version.get_or_init(|| {
            let out = self.run(cwd, &["--version"], deadline)?;
            if git_at_least(&out, 2, 30) { Ok(()) } else { Err("git 2.30 or newer is required".into()) }
        }).clone()
    }
}

/// How one marketplace ended.
#[derive(PartialEq)]
enum Status { Ok, Partial, Failed }

struct Ctx<'a> { paths: &'a Paths, git: Git, set: BTreeSet<PluginId>, one: Option<&'a PluginId> }

/// The marketplace's `commit` and `ref` checks that need no git (D4).
pub(crate) fn hex40(s: &str) -> bool { s.len() == 40 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) }

fn engine(cfg: &Merged, paths: &Paths, budget: Duration, opts: &SyncOpts, one: Option<&PluginId>) -> SyncReport {
    let start = Instant::now();
    let end = start + budget;
    let mut set = selection(cfg, None).selected;
    for p in cfg.config.profile.values() {
        set.extend(p.plugins.iter().filter(|(_, on)| **on).filter_map(|(k, _)| k.parse::<PluginId>().ok()));
    }
    if let Some(id) = one { set = [id.clone()].into(); }
    let ctx = Ctx { paths, git: Git::new(opts), set, one };
    let mut report = SyncReport::default();
    let mut statuses = vec![];
    for m in &cfg.config.marketplaces {
        if one.is_some_and(|id| id.marketplace != m.name) { continue; }
        let layer = cfg.source_of(&format!("marketplace.{}", key_seg(&m.name)));
        if layer == Layer::Builtin && m.git_ref.is_empty() { continue; }
        let deadline = (Instant::now() + opts.per_marketplace).min(end);
        let status = match sync_marketplace(&ctx, m, layer.name(), deadline, &mut report) {
            Ok(s) => s,
            Err(why) => {
                report.failures.push((m.name.clone(), why));
                Status::Failed
            }
        };
        statuses.push(status);
    }
    if one.is_none() { prune_unconfigured(cfg, paths, end); }
    if !statuses.is_empty() && statuses.iter().all(|s| *s == Status::Failed) {
        report.lines.push("plugins: offline".into());
    } else if statuses.iter().any(|s| *s != Status::Ok) {
        report.lines.push("plugins: partial".into());
    }
    report
}

/// Item 8 (round 2), items 2 and 6 (round 3): a full sync drops the record, the entries and the cache dir of
/// each marketplace no longer in config. A record is dropped only when the config layer it came from is loaded
/// now: `user` and `builtin` always are; `team` only when the team layer is loaded (`team_loaded`; a team layer that could
/// not be fetched keeps the store, §15.3); a record without a layer (written before round 3) is kept. Each name
/// is dropped under its marketplace lock, after the user config file is read again (an add that finished after
/// this config was loaded keeps it), then under the exclusive store lock. Best effort: a busy lock or a failed
/// write leaves it for the next sync.
fn prune_unconfigured(cfg: &Merged, paths: &Paths, deadline: Instant) {
    let root = &paths.plugin_store;
    let configured: BTreeSet<&str> = cfg.config.marketplaces.iter().map(|m| m.name.as_str()).collect();
    let team = cfg.team_loaded;
    let prunable = |k: &str, r: &PinEntry| !configured.contains(k)
        && match r.layer.as_str() { "user" | "builtin" => true, "team" => team, _ => false };
    let Some(m) = store::read_manifest(root).ok().flatten() else { return };
    let mut held = vec![];
    for k in m.marketplaces.iter().filter(|(k, r)| prunable(k, r)).map(|(k, _)| k) {
        if let Ok(Some(l)) = lock_marketplace(paths, k, deadline) { held.push((k.clone(), l)); }
    }
    let Some(added) = user_marketplaces(&paths.config_file) else { return };
    held.retain(|(k, _)| !added.contains(k));
    if held.is_empty() { return; }
    let Ok(Some(lock)) = store::lock_until(root, true, deadline) else { return };
    let Ok(Some(old)) = store::read_manifest(root) else { return };
    let gone: BTreeSet<&str> = held.iter().map(|(k, _)| k.as_str())
        .filter(|k| old.marketplaces.get(*k).is_some_and(|r| prunable(k, r))).collect();
    let mut new = old.clone();
    new.marketplaces.retain(|k, _| !gone.contains(k.as_str()));
    new.plugins.retain(|e| !gone.contains(e.marketplace.as_str()));
    if store::write_manifest(root, &new).is_err() { return; }
    store::finish(root, vec![], &old, &new);
    drop(lock);
    for k in gone {
        let _ = std::fs::remove_dir_all(paths.marketplaces_dir.join(k));
        remove_old_caches(&paths.marketplaces_dir, k); // `held` still holds this name's lock
    }
}

/// N46 item 5: `<mdir>/<name>.old-<ulid>` dirs a crashed reclone left. The caller holds the lock of `name`; no
/// other name's leftover is touched (a valid name has no `.`, so `<name>.old-` cannot be another name's prefix).
pub(crate) fn remove_old_caches(mdir: &Path, name: &str) {
    let Ok(rd) = std::fs::read_dir(mdir) else { return };
    let prefix = format!("{name}.old-");
    for e in rd.flatten() {
        let n = e.file_name();
        let Some(id) = n.to_str().and_then(|n| n.strip_prefix(prefix.as_str())) else { continue };
        if id.len() == 26 && id.bytes().all(|b| b.is_ascii_digit() || b.is_ascii_uppercase()) {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

/// The `[[marketplace]]` names in the user config file now; `None` when it cannot be read or parsed.
fn user_marketplaces(file: &Path) -> Option<BTreeSet<String>> {
    let t: toml::Table = match std::fs::read_to_string(file) {
        Ok(s) => s.parse().ok()?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Some(BTreeSet::new()),
        Err(_) => return None,
    };
    let list = t.get("marketplace").and_then(toml::Value::as_array).map(Vec::as_slice).unwrap_or_default();
    Some(list.iter().filter_map(|m| m.get("name")?.as_str().map(str::to_string)).collect())
}

/// `<name>.<ulid>.tmp`: a temp file `replace_file` or `store::write_manifest` left.
fn tmp_leftover(n: &str) -> bool {
    n.strip_suffix(".tmp").and_then(|s| s.rsplit_once('.'))
        .is_some_and(|(_, id)| id.len() == 26 && id.bytes().all(|b| b.is_ascii_digit() || b.is_ascii_uppercase()))
}

/// Round 3 item 6: `.manifest.json.<ulid>.tmp` files beside the manifest.
fn manifest_tmps(root: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(root) else { return vec![] };
    rd.flatten().filter(|e| e.file_name().to_str().is_some_and(|n| n.starts_with(".manifest.json.") && tmp_leftover(n)))
        .map(|e| e.path()).collect()
}

/// Item 7 (round 2): `<root>/<m>/<p>/<version>.tmp-<ulid>` and `.old-<ulid>` dirs a crashed sync left, other
/// than a version `man` stores and other than `keep`.
fn leftovers(root: &Path, m: &str, man: &Manifest, keep: &[&Path]) -> Vec<PathBuf> {
    let ulid = |s: &str| s.len() == 26 && s.bytes().all(|b| b.is_ascii_digit() || b.is_ascii_uppercase());
    let crashed = |n: &str| [".tmp-", ".old-"].iter().any(|k| n.rsplit_once(k).is_some_and(|(v, id)| !v.is_empty() && ulid(id)));
    let mut out = vec![];
    let Ok(plugins) = std::fs::read_dir(root.join(m)) else { return out };
    for p in plugins.flatten().filter(|p| p.file_type().is_ok_and(|t| t.is_dir())) {
        let pname = p.file_name().to_string_lossy().into_owned();
        let Ok(versions) = std::fs::read_dir(p.path()) else { continue };
        for v in versions.flatten() {
            let n = v.file_name().to_string_lossy().into_owned();
            let stored = man.plugins.iter().any(|e| e.marketplace == m && e.plugin == pname && e.version == n);
            if crashed(&n) && !stored && !keep.contains(&v.path().as_path()) { out.push(v.path()); }
        }
    }
    out
}

/// A source-key segment as `zygo_config` writes it.
pub(crate) fn key_seg(s: &str) -> String {
    if !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') {
        s.to_string()
    } else {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

/// `check failed: <rule> <path> (<detail>)`; the detail says why (`malformed .` alone could be a deadline or an I/O error).
pub(crate) fn check_failed(v: &Violation) -> String {
    let detail = if v.detail.is_empty() { String::new() } else { format!(" ({})", v.detail) };
    format!("check failed: {} {}{detail}", v.rule.as_str(), v.path)
}

fn sync_marketplace(ctx: &Ctx, m: &Marketplace, layer: &str, deadline: Instant, report: &mut SyncReport) -> Result<Status, String> {
    if Instant::now() >= deadline { return Err("git timed out".into()); }
    // D4: config values, before any git call.
    if !valid_name(&m.name) { return Err("unsafe marketplace name".into()); }
    let url = clone_url(&m.source).map_err(|e| e.to_string())?;
    if m.commit.as_deref().is_some_and(|c| !hex40(c)) { return Err("commit is not a 40-hex id".into()); }
    if m.git_ref.is_empty() { return Err("ref is not a tag or a commit".into()); }
    if m.git_ref.starts_with('-') { return Err("ref is not a valid tag name".into()); }
    let mdir = &ctx.paths.marketplaces_dir;
    let _lock = lock_marketplace(ctx.paths, &m.name, deadline).map_err(|e| e.to_string())?.ok_or("locked")?;
    remove_old_caches(mdir, &m.name);
    ctx.git.check_version(mdir, deadline)?;
    let tag_ref = format!("refs/tags/{}", m.git_ref);
    ctx.git.run(mdir, &["check-ref-format", &tag_ref], deadline).map_err(|e| timed(e, "ref is not a valid tag name"))?;

    let root = &ctx.paths.plugin_store;
    let old = store::read_manifest(root).ok().flatten().unwrap_or_default();
    let recorded = old.marketplaces.get(&m.name).filter(|r| hex40(&r.commit));
    let cache = mdir.join(&m.name);
    let mut fresh = ensure_cache(ctx, &cache, &url, recorded.map(|r| r.source.as_str()), &m.source, deadline)?;
    rewrite_config(&cache, &url).map_err(|e| format!("cache config: {e}"))?;
    // Round 3 item 1: a fetch or checkout that fails (not a timeout) on a reused cache is cloned once more. The old
    // cache is put aside and comes back when the clone fails (an unreachable remote), so it keeps a pin only it holds.
    let reclone = || -> Result<(), String> {
        let aside = mdir.join(format!("{}.old-{}", m.name, zygo_core::ids::ulid()));
        std::fs::rename(&cache, &aside).map_err(|e| format!("cache: {e}"))?;
        let cloned = ensure_cache(ctx, &cache, &url, None, &m.source, deadline)
            .and_then(|_| rewrite_config(&cache, &url).map_err(|e| format!("cache config: {e}")));
        if cloned.is_err() {
            let _ = std::fs::remove_dir_all(&cache);
            std::fs::rename(&aside, &cache).map_err(|e| format!("cache: {e}"))?;
        } else {
            let _ = std::fs::remove_dir_all(&aside);
        }
        cloned
    };
    if !fresh {
        let fetched = ctx.git.run(&cache, &["fetch", "--quiet", "--force", "--prune", "--prune-tags", "--no-recurse-submodules",
            "--", "origin", "+refs/heads/*:refs/remotes/origin/*", "+refs/tags/*:refs/tags/*"], deadline);
        if let Err(e) = fetched {
            if e == "git timed out" { return Err(e); }
            reclone()?;
            fresh = true;
        }
    }

    // D4, D6: resolve `ref`, choose the pin.
    let (resolved, is_tag) = resolve(&ctx.git, &cache, &m.git_ref, deadline)?;
    let same_record = recorded.filter(|r| r.source == m.source && r.git_ref == m.git_ref);
    // D9: `sync_one_in` (enable) syncs at the recorded pin; a full sync takes a configured commit first.
    let pin = match (ctx.one, same_record) {
        (Some(_), Some(r)) => r.commit.clone(),
        _ => m.commit.clone().or_else(|| same_record.map(|r| r.commit.clone())).unwrap_or_else(|| resolved.clone()),
    };
    let present = |d| ctx.git.run(&cache, &["cat-file", "-e", "--end-of-options", &format!("{pin}^{{commit}}")], d).is_ok();
    let ensure_pin = || -> Result<(), String> {
        if !present(deadline) {
            let fetched = ctx.git.run(&cache, &["fetch", "--quiet", "--no-recurse-submodules", "--", "origin", &pin], deadline);
            if let Err(e) = fetched && e == "git timed out" { return Err(e); }
            if !present(deadline) { return Err("pinned commit not found".into()); }
        }
        Ok(())
    };
    ensure_pin()?;
    if ctx.one.is_none() && is_tag && pin != resolved {
        report.lines.push(format!("marketplace {0}: tag {1} moved; run zygo marketplace update {0} --accept", m.name, zygo_core::esc(&m.git_ref)));
    }

    // D9 item 25: unchanged content is skipped.
    let mine = |e: &&StoredEntry| e.marketplace == m.name;
    let selected: Vec<&PluginId> = ctx.set.iter().filter(|i| i.marketplace == m.name).collect();
    if ctx.one.is_none() && same_record.is_some_and(|r| r.commit == pin && r.layer == layer)
        && selected.iter().all(|i| old.plugins.iter().any(|e| mine(&e) && e.plugin == i.plugin && e.commit == pin && store::verify(root, e)))
        && old.plugins.iter().filter(mine).all(|e| selected.iter().any(|i| i.plugin == e.plugin))
        && leftovers(root, &m.name, &old, &[]).is_empty() && manifest_tmps(root).is_empty() {
        report.synced.extend(selected.into_iter().cloned());
        return Ok(Status::Ok);
    }

    let checkout = || -> Result<(), String> {
        ctx.git.checkout_detached(&cache, &pin, deadline)?;
        ctx.git.run(&cache, &["clean", "-ffdxq"], deadline).map(drop)
    };
    // N46 item 1: a pin git cannot check out is marked in the cache, so a later launch does not reclone for it.
    if let Err(e) = checkout() {
        if e == "git timed out" { return Err(e); }
        if fresh { mark_pin(&cache, &pin); return Err(e); }
        if pin_marked(&cache, &pin) { return Err(e); }
        reclone()?;
        ensure_pin()?;
        if let Err(e) = checkout() {
            if e != "git timed out" { mark_pin(&cache, &pin); }
            return Err(e);
        }
    }

    // D8, D10: check the checkout, stage each selected plugin.
    let modes = git_modes_by(ctx.git.command(&cache), deadline);
    let r = check_dir_modes(&cache, deadline, modes.clone());
    let mut status = Status::Ok;
    if r.marketplace.is_none() && r.violations.is_empty() { return Err("no .claude-plugin/marketplace.json".into()); }
    if let Some(v) = r.violations.first() {
        if r.marketplace.is_none() { return Err(check_failed(v)); }
        report.failures.push((m.name.clone(), check_failed(v)));
        status = Status::Partial;
    }
    for name in &r.skipped {
        if selected.iter().any(|i| &i.plugin == name) {
            report.lines.push(format!("plugin {}:{name}: skipped, source is not a relative path", m.name));
        }
    }
    let mut staged: Vec<Staged> = vec![];
    let mut failed: BTreeSet<String> = BTreeSet::new();
    let (mut tried, mut stage_errors) = (0, 0);
    let modes = modes.unwrap_or_default();
    for p in r.plugins.iter().filter(|p| selected.iter().any(|i| i.plugin == p.name)) {
        let scope = format!("{}:{}", m.name, p.name);
        let why = if p.name == "zygo" {
            Some("plugin name zygo is reserved".to_string())
        } else if let Some(v) = p.violations.first() {
            Some(check_failed(v))
        } else {
            tried += 1;
            let prefix = p.dir.strip_prefix(&cache).map(|x| x.components().map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>().join("/")).unwrap_or_default();
            let pm: HashMap<String, String> = modes.iter().filter_map(|(k, v)| {
                let rel = if prefix.is_empty() { Some(k.as_str()) } else { k.strip_prefix(&prefix).and_then(|x| x.strip_prefix('/')) };
                rel.map(|r| (r.to_string(), v.clone()))
            }).collect();
            match store::stage(root, &m.name, &p.dir, &p.copied, &pm, deadline) {
                Ok(mut s) => {
                    s.entry.commit = pin.clone();
                    staged.push(s);
                    None
                }
                Err(v) => {
                    stage_errors += 1;
                    Some(check_failed(&v))
                }
            }
        };
        if let Some(why) = why {
            report.failures.push((scope, why));
            failed.insert(p.name.clone());
            status = Status::Partial;
        }
    }
    if tried > 0 && stage_errors == tried {
        return Err("no plugin could be staged".into());
    }

    // Swap, manifest, finish under the exclusive store lock.
    let discard_all = |v: &[Staged]| v.iter().for_each(store::discard);
    let Some(lock) = store::lock_until(root, true, deadline).map_err(|e| e.to_string())? else {
        discard_all(&staged);
        return Err("locked".into());
    };
    let old = store::read_manifest(root).ok().flatten().unwrap_or_default();
    let mine_tmp: Vec<&Path> = staged.iter().map(|s| s.tmp.as_path()).collect();
    for d in leftovers(root, &m.name, &old, &mine_tmp) { let _ = std::fs::remove_dir_all(d); }
    for f in manifest_tmps(root) { let _ = std::fs::remove_file(f); }
    let mut new = Manifest { version: 1, marketplaces: old.marketplaces.clone(), plugins: vec![] };
    let record = PinEntry { source: m.source.clone(), git_ref: m.git_ref.clone(), commit: pin.clone(), layer: layer.into() };
    let staged_names: BTreeSet<&str> = staged.iter().map(|s| s.entry.plugin.as_str()).collect();
    match ctx.one {
        Some(_) => {
            // Other entries stay with their record; a new record (none, or another source/ref) drops them.
            let keep_others = same_record.is_some();
            new.plugins = old.plugins.iter().filter(|e| !mine(e) || (keep_others && !staged_names.contains(e.plugin.as_str())))
                .cloned().collect();
            if !keep_others { new.marketplaces.insert(m.name.clone(), record); }
        }
        None => {
            let source_kept = old.marketplaces.get(&m.name).is_some_and(|r| r.source == m.source);
            new.plugins = old.plugins.iter().filter(|e| !mine(e)
                || (source_kept && failed.contains(&e.plugin) && !staged_names.contains(e.plugin.as_str()))).cloned().collect();
            new.marketplaces.insert(m.name.clone(), record);
        }
    }
    new.plugins.extend(staged.iter().map(|s| s.entry.clone()));
    let mut swapped: Vec<Swapped> = vec![];
    let undo = |swapped: &[Swapped], rest: &[Staged]| {
        swapped.iter().rev().for_each(store::rollback);
        rest.iter().for_each(store::discard);
    };
    for (i, s) in staged.iter().enumerate() {
        match store::swap_in(root, s) {
            Ok(sw) => swapped.push(sw),
            Err(e) => {
                undo(&swapped, &staged[i..]);
                return Err(format!("swap failed: {e:#}"));
            }
        }
    }
    if let Err(e) = store::write_manifest(root, &new) {
        undo(&swapped, &[]);
        return Err(format!("manifest write failed: {e:#}"));
    }
    store::finish(root, swapped, &old, &new);
    drop(lock);
    report.synced.extend(staged.iter().map(|s| PluginId { marketplace: m.name.clone(), plugin: s.entry.plugin.clone() }));
    Ok(status)
}

/// A git error other than a timeout becomes `why`.
fn timed(e: String, why: &str) -> String { if e == "git timed out" { e } else { why.into() } }

/// D4: `refs/tags/<ref>`, else a 7–40 hex commit id; `(commit, is_tag)`. Round 2: a 40-hex `ref` is a commit
/// id only (a tag of that name never moves the pin). N46 item 4: a 7–39 hex `ref` is accepted only as a prefix
/// of a commit's id, never as an abbreviated tag-object id.
pub(crate) fn resolve(git: &Git, cache: &Path, git_ref: &str, deadline: Instant) -> Result<(String, bool), String> {
    let rev = |spec: String| git.run(cache, &["rev-parse", "--verify", "--quiet", "--end-of-options", &spec], deadline);
    if !hex40(git_ref) {
        match rev(format!("refs/tags/{git_ref}^{{commit}}")) {
            Ok(c) if hex40(&c) => return Ok((c, true)),
            Err(e) if e == "git timed out" => return Err(e),
            _ => {}
        }
    }
    if (7..=40).contains(&git_ref.len()) && git_ref.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        match rev(format!("{git_ref}^{{commit}}")) {
            // Round 3 item 5, N46 item 4: the commit's id must start with `ref` (equal, for 40 hex), so a tag
            // object id, whole or abbreviated, that peels to a commit is refused.
            Ok(c) if hex40(&c) && c.starts_with(git_ref) => return Ok((c, false)),
            Err(e) if e == "git timed out" => return Err(e),
            _ => {}
        }
    }
    Err("ref is not a tag or a commit".into())
}

/// D5: `<cache>` is a real directory with a real `.git` directory for `url`, else it is removed and cloned
/// (`--no-checkout --no-local`). A recorded source other than the configured one also removes it. `true` = cloned now.
fn ensure_cache(ctx: &Ctx, cache: &Path, url: &str, recorded: Option<&str>, source: &str, deadline: Instant) -> Result<bool, String> {
    let meta = std::fs::symlink_metadata(cache).ok();
    let dotgit = std::fs::symlink_metadata(cache.join(".git")).ok();
    let usable = meta.as_ref().is_some_and(|m| m.is_dir()) && dotgit.as_ref().is_some_and(|m| m.is_dir())
        && recorded.is_none_or(|r| r == source)
        && parse_config(cache).get("url").is_none_or(|u| u == url);
    if usable {
        clear_stale(&cache.join(".git"));
        // Round 2 item 5: an empty or corrupt `.git` (git sees no repository there) is cloned again.
        match ctx.git.run(cache, &["rev-parse", "--git-dir"], deadline) {
            Ok(_) => return Ok(false),
            Err(e) if e == "git timed out" => return Err(e),
            Err(_) => {}
        }
    }
    match meta {
        Some(m) if m.is_dir() => {
            if dotgit.is_some_and(|d| !d.is_dir()) { let _ = std::fs::remove_file(cache.join(".git")); }
            std::fs::remove_dir_all(cache).map_err(|e| format!("cache: {e}"))?;
        }
        Some(_) => std::fs::remove_file(cache).map_err(|e| format!("cache: {e}"))?,
        None => {}
    }
    let name = cache.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mdir = cache.parent().unwrap_or(cache);
    if let Err(e) = ctx.git.run(mdir, &["clone", "--no-checkout", "--no-local", "--quiet", "--", url, &name], deadline) {
        let _ = std::fs::remove_dir_all(cache);
        return Err(e);
    }
    Ok(true)
}

/// Round 3 items 1, 6; N46 item 3: `*.lock` and `*.<ulid>.tmp` files under the cache `.git` that a git killed at
/// its deadline or a crashed sync left. Only zygo writes to the cache, and only under the marketplace lock, which
/// the caller holds. A `*.lock` younger than `PER_MARKETPLACE` (5 s, the longest a git child of a live zygo runs)
/// is kept: a git orphaned by a killed zygo may still hold it. An mtime up to `PER_MARKETPLACE` in the future
/// counts as young; one further ahead is stale, so a clock set back cannot keep a lock for ever. An unreadable
/// mtime also keeps the lock. Symlinks are not followed.
fn clear_stale(dotgit: &Path) {
    let old_lock = |e: &walkdir::DirEntry| match std::fs::symlink_metadata(e.path()).and_then(|m| m.modified()) {
        Ok(t) => match t.elapsed() {
            Ok(age) => age >= PER_MARKETPLACE,
            Err(ahead) => ahead.duration() > PER_MARKETPLACE,
        },
        Err(_) => false,
    };
    for e in walkdir::WalkDir::new(dotgit).into_iter().flatten() {
        if e.file_type().is_dir() { continue; }
        let Some(n) = e.file_name().to_str() else { continue };
        if tmp_leftover(n) || (n.ends_with(".lock") && old_lock(&e)) { let _ = std::fs::remove_file(e.path()); }
    }
}

/// N46 item 1: `<cache>/.git/zygo-bad-pin` holds the pin whose checkout failed on a fresh clone. It is set only
/// for that pin; `clear_stale` leaves it (not `*.lock`, not `*.<ulid>.tmp`), and a recloned cache drops it.
const BAD_PIN: &str = "zygo-bad-pin";

fn pin_marked(cache: &Path, pin: &str) -> bool {
    std::fs::read_to_string(cache.join(".git").join(BAD_PIN)).is_ok_and(|t| t == pin)
}

/// An I/O error is ignored: the next launch just retries.
fn mark_pin(cache: &Path, pin: &str) { let _ = replace_file(&cache.join(".git"), BAD_PIN, pin); }

/// The values D5 keeps from `<cache>/.git/config` (parsed here, not by git), plus the origin `url`.
fn parse_config(cache: &Path) -> HashMap<&'static str, String> {
    let p = cache.join(".git/config");
    let mut out = HashMap::new();
    if !std::fs::symlink_metadata(&p).is_ok_and(|m| m.is_file()) { return out; }
    let Ok(text) = std::fs::read_to_string(&p) else { return out };
    let mut section = String::new();
    for line in text.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            section = l.trim_matches(|c| c == '[' || c == ']').trim().to_ascii_lowercase();
            continue;
        }
        let Some((k, v)) = l.split_once('=') else { continue };
        let (k, v) = (k.trim().to_ascii_lowercase(), config_value(v));
        let slot = match (section.as_str(), k.as_str()) {
            ("core", "repositoryformatversion") if matches!(v.as_str(), "0" | "1") => "repositoryformatversion",
            ("core", "ignorecase") if matches!(v.as_str(), "true" | "false") => "ignorecase",
            ("core", "precomposeunicode") if matches!(v.as_str(), "true" | "false") => "precomposeunicode",
            ("extensions", "objectformat") if matches!(v.as_str(), "sha1" | "sha256") => "objectformat",
            ("remote \"origin\"", "url") => "url",
            _ => continue,
        };
        out.insert(slot, v);
    }
    out
}

/// A git config value as git reads it: quotes removed, `\\`, `\"`, `\n`, `\t`, `\b` unescaped, a `#` or `;` outside
/// quotes starts a comment, unquoted trailing blanks dropped. `rewrite_config` writes a Windows path as `C:\\x`.
fn config_value(raw: &str) -> String {
    let (mut out, mut keep, mut quoted) = (String::new(), 0, false);
    let mut it = raw.trim_start().chars();
    while let Some(c) = it.next() {
        match c {
            '"' => { quoted = !quoted; keep = out.len(); continue; }
            '\\' => match it.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('b') => out.push('\u{8}'),
                Some(e) => out.push(e),
                None => {}
            },
            '#' | ';' if !quoted => break,
            c => { out.push(c); if !quoted && c.is_whitespace() { continue; } }
        }
        keep = out.len();
    }
    out.truncate(keep);
    out
}

/// D5: `<cache>/.git/config` rewritten to fixed text; nothing an earlier process wrote survives.
pub(crate) fn rewrite_config(cache: &Path, url: &str) -> anyhow::Result<()> {
    let kept = parse_config(cache);
    let mut t = format!("[core]\n\trepositoryformatversion = {}\n\tfilemode = true\n\tbare = false\n\tlogallrefupdates = false\n",
        kept.get("repositoryformatversion").map_or("0", String::as_str));
    for k in ["ignorecase", "precomposeunicode"] {
        if let Some(v) = kept.get(k) { t.push_str(&format!("\t{k} = {v}\n")); }
    }
    if let Some(v) = kept.get("objectformat") { t.push_str(&format!("[extensions]\n\tobjectFormat = {v}\n")); }
    t.push_str(&format!("[remote \"origin\"]\n\turl = \"{}\"\n\tfetch = +refs/heads/*:refs/remotes/origin/*\n",
        url.replace('\\', "\\\\").replace('"', "\\\"")));
    let dir = cache.join(".git");
    replace_file(&dir, "config", &t)?;
    // Round 2 item 6: `info/attributes` outranks every `.gitattributes` and `core.attributesFile`, so no path
    // selects a filter (smudge/process), diff (textconv) or merge driver, whoever defines one (global config too).
    // Round 3 item 3: `-text` (so no `eol` or `crlf` conversion), `-ident`, `-working-tree-encoding`: the
    // checkout bytes are the blob bytes.
    let info = dir.join("info");
    if std::fs::symlink_metadata(&info).is_ok_and(|m| !m.is_dir()) { std::fs::remove_file(&info)?; }
    std::fs::create_dir_all(&info)?;
    replace_file(&info, "attributes", "* -filter -diff -merge -text -ident -working-tree-encoding\n")
}

/// `dir/name` replaced by `text` (temp file plus rename: a symlink at `name` is replaced, not followed).
fn replace_file(dir: &Path, name: &str, text: &str) -> anyhow::Result<()> {
    let tmp = dir.join(format!("{name}.{}.tmp", zygo_core::ids::ulid()));
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, dir.join(name)).inspect_err(|_| { let _ = std::fs::remove_file(&tmp); })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::config_value;

    #[test]
    fn config_values_read_as_git_reads_them() {
        assert_eq!(config_value(r#""C:\\Users\\a b\\mk""#), r"C:\Users\a b\mk"); // what rewrite_config writes
        assert_eq!(config_value(r"C:\\Users\\mk"), r"C:\Users\mk");
        assert_eq!(config_value("https://x/y.git"), "https://x/y.git");
        assert_eq!(config_value(r#""a \"q\" \t\n\b""#), "a \"q\" \t\n\u{8}");
        assert_eq!(config_value(r#" "keep  "  ; comment"#), "keep  ");
        assert_eq!(config_value("x # comment"), "x");
        assert_eq!(config_value(r#""a # b""#), "a # b");
        assert_eq!(config_value("a   b  "), "a   b");
        assert_eq!(config_value(""), "");
    }
}
