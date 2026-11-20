#![allow(dead_code)]
//! Shared fixtures for the plugin sync and store tests: temp dirs, local git repositories, configs.
//! No environment mutation, no network, no sleeps.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use sha2::{Digest, Sha256};
use zygo_config::{load_with_builtin, LoadContext, Merged};
use zygo_core::paths::Paths;

pub const NULL: &str = if cfg!(windows) { "NUL" } else { "/dev/null" };

pub struct Fx(pub PathBuf);
impl Fx {
    pub fn new() -> Fx {
        let d = std::env::temp_dir().join(format!("zygo-plugins-{}", zygo_core::ids::ulid()));
        std::fs::create_dir_all(&d).unwrap();
        Fx(d)
    }
    pub fn path(&self, rel: &str) -> PathBuf { self.0.join(rel) }
    pub fn write(&self, rel: &str, bytes: impl AsRef<[u8]>) -> PathBuf {
        let p = self.path(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, bytes).unwrap();
        p
    }
}
impl Drop for Fx {
    fn drop(&mut self) {
        #[cfg(unix)]
        for e in walkdir::WalkDir::new(&self.0).into_iter().flatten() {
            use std::os::unix::fs::PermissionsExt;
            if e.file_type().is_dir() { let _ = std::fs::set_permissions(e.path(), std::fs::Permissions::from_mode(0o755)); }
        }
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn paths(fx: &Fx) -> Paths {
    let h = fx.path("home");
    Paths {
        config_file: h.join("config.toml"),
        cache_dir: h.join("cache"),
        data_dir: h.join("data"),
        sessions_dir: h.join("cache/sessions"),
        marketplaces_dir: h.join("cache/marketplaces"),
        plugin_store: h.join("data/plugins"),
        approvals_file: h.join("data/approvals.toml"),
        usage_db: h.join("data/usage.db"),
    }
}

/// A test-side git call: no system or global config.
pub fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git").env("GIT_CONFIG_GLOBAL", NULL).env("GIT_CONFIG_NOSYSTEM", "1")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid", "-c", "commit.gpgsign=false", "-c", "tag.gpgsign=false"])
        .arg("-C").arg(dir).args(args).output().expect("git on PATH");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

pub struct Repo { pub dir: PathBuf }
impl Repo {
    pub fn new(fx: &Fx, name: &str) -> Repo {
        let dir = fx.path(name);
        std::fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q", "-b", "main"]);
        Repo { dir }
    }
    pub fn src(&self) -> String { self.dir.display().to_string() }
    pub fn write(&self, rel: &str, bytes: impl AsRef<[u8]>) {
        let p = self.dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }
    pub fn chmod_x(&self, rel: &str) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(self.dir.join(rel), std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        git(&self.dir, &["add", "--", rel]);
        git(&self.dir, &["update-index", "--chmod=+x", "--", rel]);
    }
    pub fn commit(&self, msg: &str) -> String {
        git(&self.dir, &["add", "-A"]);
        git(&self.dir, &["commit", "-q", "--allow-empty", "-m", msg]);
        git(&self.dir, &["rev-parse", "HEAD"])
    }
    pub fn tag(&self, name: &str) { git(&self.dir, &["tag", name]); }
    pub fn tag_force(&self, name: &str) { git(&self.dir, &["tag", "-f", name]); }
    pub fn orphan(&self, branch: &str) { git(&self.dir, &["checkout", "-q", "--orphan", branch]); }
    pub fn delete_branch(&self, name: &str) { git(&self.dir, &["branch", "-q", "-D", name]); }
    pub fn config(&self, key: &str, value: &str) { git(&self.dir, &["config", key, value]); }
}

fn at(dir: &str, rel: &str) -> String { if dir.is_empty() { rel.to_string() } else { format!("{dir}/{rel}") } }

/// A valid plugin `name` at `version` under `dir` of `repo`'s work tree.
pub fn plugin_files(repo: &Repo, dir: &str, name: &str, version: &str) {
    repo.write(&at(dir, ".claude-plugin/plugin.json"),
        format!(r#"{{"name":"{name}","version":"{version}","description":"Demo plugin","license":"MIT"}}"#));
    repo.write(&at(dir, "skills/hello/SKILL.md"), "---\nname: hello\ndescription: Says hello\n---\nSay hello.\n");
}

/// `marketplace.json` listing `plugins` (each `./plugins/<name>`).
pub fn market_json(repo: &Repo, plugins: &[(&str, &str)]) {
    let entries: Vec<String> = plugins.iter()
        .map(|(n, v)| format!(r#"{{"name":"{n}","source":"./plugins/{n}","description":"Demo","version":"{v}"}}"#))
        .collect();
    repo.write(".claude-plugin/marketplace.json",
        format!(r#"{{"name":"team","owner":{{"name":"Owner"}},"plugins":[{}]}}"#, entries.join(",")));
}

/// A marketplace repository `origin` with `plugins`, committed and tagged `v<first version>`.
pub fn market_repo(fx: &Fx, plugins: &[(&str, &str)]) -> Repo { market_repo_at(fx, "origin", plugins) }
pub fn market_repo_at(fx: &Fx, name: &str, plugins: &[(&str, &str)]) -> Repo {
    let r = Repo::new(fx, name);
    market_json(&r, plugins);
    for (n, v) in plugins { plugin_files(&r, &format!("plugins/{n}"), n, v); }
    r.commit("first");
    r.tag(&format!("v{}", plugins[0].1));
    r
}

fn q(s: &str) -> String { toml::Value::String(s.to_string()).to_string() }

/// One `[[marketplace]]` block.
pub fn mp(name: &str, source: &str, git_ref: &str, commit: Option<&str>, default_on: &[&str]) -> String {
    let mut s = format!("[[marketplace]]\nname = {}\nsource = {}\nref = {}\n", q(name), q(source), q(git_ref));
    if let Some(c) = commit { s.push_str(&format!("commit = {}\n", q(c))); }
    let on: Vec<String> = default_on.iter().map(|d| q(d)).collect();
    s.push_str(&format!("default_on = [{}]\n", on.join(", ")));
    s
}

/// Writes `user` to the user config and loads it with an empty built-in layer (no `act101`).
pub fn merged(fx: &Fx, user: &str, team: Option<&str>) -> Merged {
    let paths = paths(fx);
    std::fs::create_dir_all(paths.config_file.parent().unwrap()).unwrap();
    std::fs::write(&paths.config_file, user).unwrap();
    load_with_builtin(&LoadContext { paths, repo: None, team: team.map(|t| t.parse().unwrap()) }, toml::Table::new()).unwrap()
}

pub fn sha_hex(bytes: &[u8]) -> String { hex::encode(Sha256::digest(bytes)) }

/// An executable that logs its arguments (one line per call) and runs the real git.
#[cfg(unix)]
pub fn git_tracer(fx: &Fx) -> (PathBuf, PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let log = fx.path("trace.log");
    let real = which::which("git").expect("git on PATH");
    let script = fx.write("bin/trace-git", format!("#!/bin/sh\necho \"$*\" >> '{}'\nexec '{}' \"$@\"\n", log.display(), real.display()));
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    (script, log)
}
#[cfg(windows)]
pub fn git_tracer(fx: &Fx) -> (PathBuf, PathBuf) {
    let log = fx.path("trace.log");
    let script = fx.write("bin/trace-git.bat", format!("@echo %* >> \"{}\"\r\n@git %*\r\n", log.display()));
    (script, log)
}

/// A git that fails every `clone` with a message naming the URL it was given (`fatal: unable to access '<url>/': boom`).
#[cfg(unix)]
pub fn url_git(fx: &Fx) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let real = which::which("git").expect("git on PATH");
    let script = fx.write("bin/url-git", format!(
        "#!/bin/sh\nfor a; do case \"$a\" in *://*) u=$a;; esac; done\ncase \" $* \" in *\" clone \"*) echo \"fatal: unable to access '$u/': boom\" >&2; exit 128;; esac\nexec '{}' \"$@\"\n",
        real.display()));
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    script
}

/// A git that replaces every URL argument that has userinfo with the path of `repo`, then runs the real git.
#[cfg(unix)]
pub fn redirect_git(fx: &Fx, repo: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let real = which::which("git").expect("git on PATH");
    let script = fx.write("bin/redirect-git", format!(
        "#!/bin/sh\nn=$#; while [ $n -gt 0 ]; do a=$1; shift; case \"$a\" in *://*@*) a='{}';; esac; set -- \"$@\" \"$a\"; n=$((n-1)); done\nexec '{}' \"$@\"\n",
        repo.display(), real.display()));
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    script
}

pub fn far() -> std::time::Instant { std::time::Instant::now() + std::time::Duration::from_secs(60) }

/// A budget no loaded machine reaches; for tests that do not test a deadline.
pub const WIDE_TOTAL: Duration = Duration::from_secs(120);

/// `SyncOpts` whose git calls read an empty global config file (never the developer's).
pub fn opts(fx: &Fx) -> zygo_plugins::sync::SyncOpts {
    let empty = fx.write("empty-gitconfig", "");
    zygo_plugins::sync::SyncOpts { per_marketplace: WIDE_TOTAL, git_config_global: Some(empty), ..Default::default() }
}

// ---- snapshot fixtures (N43) ----

/// One plugin of a test marketplace: `files` are plugin-relative and override the defaults of `plugin_files`;
/// `exec` lists the plugin-relative files committed with the execute bit.
pub struct PluginSpec { pub name: &'static str, pub files: Vec<(&'static str, Vec<u8>)>, pub exec: Vec<&'static str> }
impl PluginSpec {
    pub fn new(name: &'static str) -> PluginSpec { PluginSpec { name, files: vec![], exec: vec![] } }
    pub fn file(mut self, rel: &'static str, bytes: impl AsRef<[u8]>) -> PluginSpec { self.files.push((rel, bytes.as_ref().to_vec())); self }
    pub fn exec(mut self, rel: &'static str) -> PluginSpec { self.exec.push(rel); self }
}

/// A marketplace repository at `fx/<dir>` holding `specs` at version 1.0.0, tagged `v1.0.0`.
pub fn build_market(fx: &Fx, dir: &str, specs: &[PluginSpec]) -> Repo {
    let r = Repo::new(fx, dir);
    let pairs: Vec<(&str, &str)> = specs.iter().map(|s| (s.name, "1.0.0")).collect();
    market_json(&r, &pairs);
    for s in specs {
        let base = format!("plugins/{}", s.name);
        plugin_files(&r, &base, s.name, "1.0.0");
        for (rel, b) in &s.files { r.write(&format!("{base}/{rel}"), b); }
        for rel in &s.exec { r.chmod_x(&format!("{base}/{rel}")); }
    }
    r.commit("first");
    r.tag("v1.0.0");
    r
}

/// Marketplaces `(name, specs)`, each in `fx/origin` (name `team`) or `fx/origin-<name>`, all plugins `default_on`;
/// user config = the marketplace blocks plus `extra_user`; synced once; asserts no failure.
pub fn stored_multi(fx: &Fx, markets: &[(&str, &[PluginSpec])], extra_user: &str) -> (Paths, Merged) {
    let mut user = String::new();
    for (name, specs) in markets {
        let dir = if *name == "team" { "origin".to_string() } else { format!("origin-{name}") };
        let r = build_market(fx, &dir, specs);
        let on: Vec<&str> = specs.iter().map(|s| s.name).collect();
        user.push_str(&mp(name, &r.src(), "v1.0.0", None, &on));
    }
    user.push_str(extra_user);
    let cfg = merged(fx, &user, None);
    let paths = paths(fx);
    let rep = zygo_plugins::sync::sync_with(&cfg, &paths, WIDE_TOTAL, &opts(fx));
    assert!(rep.failures.is_empty(), "{:?}", rep.failures);
    (paths, cfg)
}

/// Marketplace `team` (all specs `default_on`, tag `v1.0.0`) synced into the store.
pub fn stored(fx: &Fx, specs: &[PluginSpec], extra_user: &str) -> (Paths, Merged) { stored_multi(fx, &[("team", specs)], extra_user) }

/// `snapshot_in` with a 60 s lock deadline.
pub fn snap(paths: &Paths, session: &Path, agent: zygo_core::Agent, sel: &zygo_plugins::select::Selection,
    path: Option<&std::ffi::OsStr>) -> anyhow::Result<zygo_plugins::snapshot::Snapshot> {
    zygo_plugins::snapshot::snapshot_in(paths, session, agent, sel, path, far())
}

/// The path of the running test executable (an absolute path to an executable regular file).
pub fn exe() -> String { std::env::current_exe().unwrap().display().to_string() }

/// The code digest recorded for `team:<plugin>`.
pub fn digest(paths: &Paths, marketplace: &str, plugin: &str) -> String {
    let st = zygo_plugins::store::state(&paths.plugin_store);
    st.plugins.iter().find(|p| p.entry.marketplace == marketplace && p.entry.plugin == plugin)
        .and_then(|p| p.entry.digest.clone()).unwrap_or_else(|| panic!("{marketplace}:{plugin} has no digest"))
}

pub fn approve(paths: &Paths, marketplace: &str, plugin: &str) {
    zygo_plugins::approve::approve_in(&paths.approvals_file, marketplace, plugin, &digest(paths, marketplace, plugin)).unwrap();
}

/// The stored entry of `<marketplace>:<plugin>`.
pub fn entry_of(paths: &Paths, marketplace: &str, plugin: &str) -> zygo_plugins::store::StoredEntry {
    zygo_plugins::store::state(&paths.plugin_store).plugins.into_iter()
        .find(|p| p.entry.marketplace == marketplace && p.entry.plugin == plugin).unwrap().entry
}

/// The `team` source of `fx/origin`.
pub fn origin_src(fx: &Fx) -> String { fx.path("origin").display().to_string() }

/// Recursive copy of a directory.
pub fn copy_tree(from: &Path, to: &Path) {
    for e in walkdir::WalkDir::new(from).into_iter().flatten() {
        let rel = e.path().strip_prefix(from).unwrap();
        let dst = to.join(rel);
        if e.file_type().is_dir() { std::fs::create_dir_all(&dst).unwrap(); } else { std::fs::copy(e.path(), &dst).unwrap(); }
    }
}

/// The files under `dir`, `/`-separated and relative.
pub fn tree(dir: &Path) -> std::collections::BTreeSet<String> {
    walkdir::WalkDir::new(dir).into_iter().flatten().filter(|e| e.file_type().is_file())
        .map(|e| e.path().strip_prefix(dir).unwrap().components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/"))
        .collect()
}
