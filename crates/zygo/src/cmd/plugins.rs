//! `zygo plugins sync|enable|disable|approve` (spec §15.2, §15.3, §15.7, §17, §17.1).

use std::io::{BufRead, Write};
use std::time::Instant;

use anyhow::{anyhow, bail};
use zygo_config::{load, load_with_builtin, LoadContext, Merged};
use zygo_plugins::approve::{approve_in, is_approved_in};
use zygo_plugins::select::{list, selection, set_enabled, PluginId};
use zygo_plugins::snapshot::delivered_in;
use zygo_plugins::store::{self, StoredEntry};
use zygo_plugins::sync::{sync_one_with, sync_with, SyncOpts, TOTAL};

use super::verbs::VerbCtx;
use super::UsageError;

const USAGE: &str = "zygo plugins: usage: plugins [list|enable|disable|approve <m>:<p>] | sync";

pub fn verb(ctx: &VerbCtx, args: &[String]) -> anyhow::Result<String> {
    run(ctx, args, super::verbs::team(ctx, "plugins")?, &mut std::io::stdin().lock())
}

/// `team`: the fetched team table (N37); `None` without a server.
pub fn run(ctx: &VerbCtx, args: &[String], team: Option<toml::Table>, input: &mut dyn BufRead) -> anyhow::Result<String> {
    run_with(ctx, args, team, None, input)
}

/// `builtin`: `None` loads the real built-in layer; tests pass an empty table.
pub fn run_with(ctx: &VerbCtx, args: &[String], team: Option<toml::Table>, builtin: Option<toml::Table>,
    input: &mut dyn BufRead) -> anyhow::Result<String> {
    run_io(ctx, args, team, builtin, input, &mut std::io::stderr(), &SyncOpts::default())
}

/// `run_with` with the stream the approval prompt (code listing and question) is written to, and the sync options.
fn run_io(ctx: &VerbCtx, args: &[String], team: Option<toml::Table>, builtin: Option<toml::Table>,
    input: &mut dyn BufRead, prompt: &mut dyn Write, opts: &SyncOpts) -> anyhow::Result<String> {
    let yes = args.iter().any(|a| a == "--yes");
    let words: Vec<&str> = args.iter().map(String::as_str).filter(|a| *a != "--yes").collect();
    let load_cfg = || -> anyhow::Result<Merged> {
        let lc = LoadContext { paths: ctx.paths.clone(), repo: None, team: team.clone() };
        match &builtin { None => load(&lc), Some(b) => load_with_builtin(&lc, b.clone()) }.map_err(|e| anyhow!("zygo plugins: {e}"))
    };
    let id = |s: &str| s.parse::<PluginId>().map_err(|_| anyhow::Error::new(UsageError(USAGE.into())));
    match words.as_slice() {
        [] | ["list"] => {
            let lines = list(&load_cfg()?, None, &ctx.paths, std::env::var_os("PATH").as_deref());
            Ok(if lines.is_empty() { "no plugins".to_string() } else { lines.join("\n") })
        }
        ["sync"] => {
            if ctx.slash { return Ok("run in your shell: zygo plugins sync".into()); }
            let cfg = load_cfg()?;
            let r = sync_with(&cfg, &ctx.paths, TOTAL, opts);
            let mut out = r.lines;
            out.extend(r.failures.iter().map(|(scope, why)| format!("  {}: {}", zygo_core::esc(scope), zygo_core::esc(why))));
            match delivered_in(&ctx.paths, &selection(&cfg, None), Instant::now() + TOTAL) {
                Ok(d) => out.extend(d.details.iter().map(|x| format!("  {x}"))),
                Err(e) => out.push(format!("  {e}")),
            }
            Ok(out.join("\n"))
        }
        [verb @ ("enable" | "disable" | "approve"), spec] => {
            let id = id(spec)?;
            let cfg = load_cfg()?;
            if !cfg.config.marketplaces.iter().any(|m| m.name == id.marketplace) {
                bail!("marketplace {} is not configured", id.marketplace);
            }
            let mut out: Vec<String> = vec![];
            match *verb {
                "disable" => {
                    set_enabled(&ctx.paths.config_file, &cfg, &id, false)?;
                    out.push(format!("disabled {id}"));
                }
                "enable" => {
                    if ctx.slash && inspected(ctx, &id)?.is_none() { return Ok(format!("run in your shell: zygo plugins enable {id}")); }
                    set_enabled(&ctx.paths.config_file, &cfg, &id, true)?;
                    out.push(format!("enabled {id}"));
                    let cfg = load_cfg()?;
                    let mut failures: Vec<String> = vec![];
                    if !ctx.slash {
                        let r = sync_one_with(&cfg, &ctx.paths, TOTAL, opts, &id);
                        out.extend(r.lines);
                        failures = r.failures.iter().map(|(scope, why)| format!("  {}: {}", zygo_core::esc(scope), zygo_core::esc(why))).collect();
                        out.extend(failures.iter().cloned());
                    }
                    match approval(ctx, &cfg, &id, yes, input, prompt) {
                        Ok(s) => out.push(s),
                        Err(e) if failures.is_empty() => return Err(e),
                        Err(e) => return Err(super::EscapedLines::new(out.iter().cloned().chain([e.to_string()])).into()),
                    }
                }
                _ => out.push(approval(ctx, &cfg, &id, yes, input, prompt)?),
            }
            if ctx.slash && *verb != "approve" { out.push("applies at next launch".into()); }
            Ok(out.join("\n"))
        }
        _ => Err(UsageError(USAGE.into()).into()),
    }
}

/// `y` or `yes` (any case, trimmed) on the next line.
pub(crate) fn confirm(input: &mut dyn BufRead) -> bool {
    let mut s = String::new();
    input.read_line(&mut s).is_ok() && matches!(s.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

/// The stored entry of `id` and a fresh check of it, under the shared store lock (released on return); a slash reply
/// writes nothing, so it reads without the lock.
fn inspected(ctx: &VerbCtx, id: &PluginId) -> anyhow::Result<Option<(StoredEntry, zygo_plugins::check::CheckReport)>> {
    let root = &ctx.paths.plugin_store;
    let _lock = if ctx.slash { None } else {
        Some(store::lock_until(root, false, Instant::now() + TOTAL)?.ok_or_else(|| anyhow!("the plugin store is locked; try again"))?)
    };
    let m = store::read_manifest(root).map_err(|_| anyhow!("the plugin store manifest is invalid; run zygo plugins sync"))?;
    let Some(e) = m.and_then(|m| m.plugins.into_iter().find(|e| e.marketplace == id.marketplace && e.plugin == id.plugin)) else {
        return Ok(None);
    };
    let r = store::inspect(root, &e).map_err(|_| anyhow!("store copy differs from its manifest; run zygo plugins sync"))?;
    Ok(Some((e, r)))
}

/// D14: show the code of the stored copy and ask; never while holding a lock, never inside an agent session.
pub(super) fn approval(ctx: &VerbCtx, cfg: &Merged, id: &PluginId, yes: bool, input: &mut dyn BufRead,
    prompt: &mut dyn Write) -> anyhow::Result<String> {
    let Some((entry, report)) = inspected(ctx, id)? else {
        bail!("{id} is not in the store; run zygo plugins sync");
    };
    let Some(digest) = entry.digest.clone() else { return Ok(format!("{id} has no code; nothing to approve")) };
    let file = &ctx.paths.approvals_file;
    let pre = selection(cfg, None).pre_approved.iter()
        .any(|a| a.marketplace == id.marketplace && a.plugin == id.plugin && a.digest == digest);
    if pre || is_approved_in(file, &id.marketplace, &id.plugin, &digest) { return Ok(format!("{id} code already approved")); }
    let shown = report.render().trim_end().to_string();
    if ctx.slash { return Ok(format!("{shown}\nrun: zygo plugins approve {id}")); }
    if ctx.session_dir.is_some() {
        bail!("approval is not available inside an agent session; run it in your own shell: zygo plugins approve {id}");
    }
    // With `--yes` the listing is part of the output; with a question it is written once, with the question, to
    // the prompt stream (stderr), so the person answering sees the code before the reply is read.
    let mut out = vec![];
    if yes {
        out.push(shown);
    } else {
        let _ = write!(prompt, "{shown}\napprove {id}? [y/N] ");
        let _ = prompt.flush();
        if !confirm(input) { return Ok(format!("not approved; run zygo plugins approve {id}")); }
    }
    let again = || anyhow!("store changed while waiting; run zygo plugins approve {id} again");
    let root = &ctx.paths.plugin_store;
    let _lock = store::lock_until(root, false, Instant::now() + TOTAL)?.ok_or_else(|| anyhow!("the plugin store is locked; try again"))?;
    let now = store::read_manifest(root).ok().flatten()
        .and_then(|m| m.plugins.into_iter().find(|e| e.marketplace == id.marketplace && e.plugin == id.plugin));
    if now.as_ref() != Some(&entry) { return Err(again()); }
    let fresh = store::inspect(root, &entry).map_err(|_| again())?;
    if fresh.plugins.first().and_then(|p| p.digest.as_deref()) != Some(digest.as_str()) { return Err(again()); }
    approve_in(file, &id.marketplace, &id.plugin, &digest)?;
    out.push(format!("approved {id} {digest}"));
    Ok(out.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::io::{BufRead, Cursor, Read};
    use std::path::Path;
    use std::process::Command;
    use std::rc::Rc;
    use std::time::{Duration, Instant};
    use tempfile::TempDir;
    use zygo_core::paths::Paths;
    use zygo_core::session_dir::SessionDir;
    use zygo_plugins::approve::is_approved_in;
    use zygo_plugins::store;

    const WIDE: Duration = Duration::from_secs(120);
    const NULL: &str = if cfg!(windows) { "NUL" } else { "/dev/null" };
    const USAGE: &str = "zygo plugins: usage: plugins [list|enable|disable|approve <m>:<p>] | sync";
    const REFUSED: &str = "approval is not available inside an agent session; run it in your own shell: zygo plugins approve team:demo";

    fn ctx(slash: bool, session: bool) -> (VerbCtx, TempDir) {
        let t = tempfile::tempdir().unwrap();
        let r = t.path();
        let paths = Paths {
            config_file: r.join("config.toml"),
            cache_dir: r.join("cache"),
            data_dir: r.join("data"),
            sessions_dir: r.join("cache/sessions"),
            marketplaces_dir: r.join("cache/marketplaces"),
            plugin_store: r.join("data/plugins"),
            approvals_file: r.join("data/approvals.toml"),
            usage_db: r.join("data/usage.db"),
        };
        (with(&paths, slash, session), t)
    }

    /// Another context over the same paths.
    fn with(paths: &Paths, slash: bool, session: bool) -> VerbCtx {
        let session_dir = session.then(|| SessionDir::new(paths.sessions_dir.join("s1")));
        VerbCtx { slash, save: false, session_dir, paths: paths.clone() }
    }

    fn git(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git").env("GIT_CONFIG_GLOBAL", NULL).env("GIT_CONFIG_NOSYSTEM", "1")
            .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid", "-c", "commit.gpgsign=false", "-c", "tag.gpgsign=false"])
            .arg("-C").arg(dir).args(args).output().unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn write(dir: &Path, rel: &str, text: &str) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    /// Commits `plugins` (each 1.0.0; `scripts/run.sh` in each when `with_code`) and tags `v1.0.0`.
    fn commit_market(dir: &Path, plugins: &[&str], with_code: bool, skill: &str) {
        let entries: Vec<String> = plugins.iter()
            .map(|n| format!(r#"{{"name":"{n}","source":"./plugins/{n}","description":"Demo","version":"1.0.0"}}"#)).collect();
        write(dir, ".claude-plugin/marketplace.json", &format!(r#"{{"name":"team","owner":{{"name":"Owner"}},"plugins":[{}]}}"#, entries.join(",")));
        for n in plugins {
            write(dir, &format!("plugins/{n}/.claude-plugin/plugin.json"),
                &format!(r#"{{"name":"{n}","version":"1.0.0","description":"Demo plugin","license":"MIT"}}"#));
            write(dir, &format!("plugins/{n}/skills/hello/SKILL.md"), &format!("---\nname: hello\ndescription: Says hello\n---\n{skill}\n"));
            if with_code { write(dir, &format!("plugins/{n}/scripts/run.sh"), "echo hi\n"); }
        }
        git(dir, &["add", "-A"]);
        git(dir, &["commit", "-q", "-m", "c"]);
        git(dir, &["tag", "-f", "v1.0.0"]);
    }

    fn git_market_with(dir: &Path, plugins: &[&str], with_code: bool) -> String {
        std::fs::create_dir_all(dir).unwrap();
        git(dir, &["init", "-q", "-b", "main"]);
        commit_market(dir, plugins, with_code, "Say hello.");
        dir.display().to_string()
    }

    fn git_market(dir: &Path, with_code: bool) -> String { git_market_with(dir, &["demo"], with_code) }

    fn mp(src: &str, default_on: &[&str]) -> String {
        let on: Vec<String> = default_on.iter().map(|d| format!("\"{d}\"")).collect();
        format!("[[marketplace]]\nname = \"team\"\nsource = {}\nref = \"v1.0.0\"\ndefault_on = [{}]\n",
            toml::Value::String(src.into()), on.join(", "))
    }

    fn write_cfg(paths: &Paths, src: &str, default_on: &[&str]) { std::fs::write(&paths.config_file, mp(src, default_on)).unwrap(); }

    fn run(c: &VerbCtx, args: &[&str], team: Option<&str>, input: &mut dyn BufRead) -> anyhow::Result<String> {
        prompted(c, args, team, input).0
    }

    /// The result and what was written to the prompt stream.
    fn prompted(c: &VerbCtx, args: &[&str], team: Option<&str>, input: &mut dyn BufRead) -> (anyhow::Result<String>, String) {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let mut prompt = vec![];
        let r = super::run_io(c, &args, team.map(|t| t.parse().unwrap()), Some(toml::Table::new()), input, &mut prompt,
            &SyncOpts { per_marketplace: WIDE, ..Default::default() });
        (r, String::from_utf8(prompt).unwrap())
    }

    fn say(c: &VerbCtx, args: &[&str], text: &str) -> anyhow::Result<String> { run(c, args, None, &mut Cursor::new(text.to_string())) }

    #[cfg(unix)]
    #[test]
    #[ignore = "timing: git sync inside the 20 s plugins TOTAL bound"]
    fn sync_uses_the_given_opts() {
        use std::os::unix::fs::PermissionsExt;
        let (c, t) = ctx(false, false);
        write_cfg(&c.paths, "/nonexistent", &["demo"]);
        let ran = t.path().join("ran");
        let stub = t.path().join("fake-git");
        std::fs::write(&stub, format!("#!/bin/sh\ntouch '{}'\nexit 1\n", ran.display())).unwrap();
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        let opts = SyncOpts { git_program: Some(stub), per_marketplace: WIDE, ..Default::default() };
        let args = vec!["sync".to_string()];
        let _ = super::run_io(&c, &args, None, Some(toml::Table::new()), &mut Cursor::new(String::new()), &mut vec![], &opts);
        assert!(ran.exists());
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "timing: git sync inside the 20 s plugins TOTAL bound"]
    fn sync_failure_is_scrubbed() {
        use std::os::unix::fs::PermissionsExt;
        let (c, t) = ctx(false, false);
        let tk = format!("tk{}", zygo_core::ids::ulid());
        write_cfg(&c.paths, &format!("git+https://bob:{tk}@h.example/o/r.git"), &["demo"]);
        let stub = t.path().join("failing-git");
        std::fs::write(&stub, format!(
            "#!/bin/sh\nfor a; do case \"$a\" in *://*) u=$a;; esac; done\ncase \" $* \" in *\" clone \"*) echo \"fatal: unable to access '$u/': boom\" >&2; exit 128;; esac\nexec '{}' \"$@\"\n",
            which::which("git").unwrap().display())).unwrap();
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        let opts = SyncOpts { git_program: Some(stub), per_marketplace: WIDE, ..Default::default() };
        let args = vec!["sync".to_string()];
        let out = super::run_io(&c, &args, None, Some(toml::Table::new()), &mut Cursor::new(String::new()), &mut vec![], &opts).unwrap();
        assert!(out.contains("  team: git failed: fatal: unable to access 'https://h.example/o/r.git/': boom"), "{out}");
        assert!(!out.contains(&tk), "{out}");
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "timing: git sync inside the 20 s plugins TOTAL bound"]
    fn enable_uses_the_given_opts() {
        use std::os::unix::fs::PermissionsExt;
        let (c, t) = ctx(false, false);
        write_cfg(&c.paths, "/nonexistent", &[]);
        let ran = t.path().join("ran");
        let stub = t.path().join("fake-git");
        std::fs::write(&stub, format!("#!/bin/sh\ntouch '{}'\nexit 1\n", ran.display())).unwrap();
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        let opts = SyncOpts { git_program: Some(stub), per_marketplace: WIDE, ..Default::default() };
        let args = vec!["enable".to_string(), "team:demo".to_string()];
        let _ = super::run_io(&c, &args, None, Some(toml::Table::new()), &mut Cursor::new(String::new()), &mut vec![], &opts);
        assert!(ran.exists());
    }

    fn stored_entry(paths: &Paths, plugin: &str) -> store::StoredEntry {
        store::read_manifest(&paths.plugin_store).unwrap().unwrap().plugins.into_iter().find(|e| e.plugin == plugin).unwrap()
    }

    /// A code plugin `demo` stored by a full sync.
    fn synced(c: &VerbCtx, with_code: bool) -> String {
        let src = git_market(&c.paths.cache_dir.parent().unwrap().join("origin"), with_code);
        write_cfg(&c.paths, &src, &["demo"]);
        assert_eq!(say(&with(&c.paths, false, false), &["sync"], "").unwrap(), "");
        src
    }

    fn approvals_absent(c: &VerbCtx) -> bool { !c.paths.approvals_file.exists() }

    /// Calls `on_read` once, before the first byte is read.
    struct HookedInput { on_read: Option<Box<dyn FnMut()>>, text: Cursor<Vec<u8>> }
    impl HookedInput {
        fn new(on_read: impl FnMut() + 'static, text: &str) -> Self { HookedInput { on_read: Some(Box::new(on_read)), text: Cursor::new(text.as_bytes().to_vec()) } }
        fn hook(&mut self) { if let Some(mut f) = self.on_read.take() { f(); } }
    }
    impl Read for HookedInput {
        fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> { self.hook(); self.text.read(b) }
    }
    impl BufRead for HookedInput {
        fn fill_buf(&mut self) -> std::io::Result<&[u8]> { self.hook(); self.text.fill_buf() }
        fn consume(&mut self, n: usize) { self.text.consume(n) }
    }

    #[test]
    #[ignore = "timing: git sync inside the 20 s plugins TOTAL bound"]
    fn sync_prints_the_launch_lines_and_failure_details() {
        let (c, t) = ctx(false, false);
        write_cfg(&c.paths, &t.path().join("missing").display().to_string(), &["demo"]);
        let out = say(&c, &["sync"], "").unwrap();
        let mut lines = out.lines();
        assert_eq!(lines.next(), Some("plugins: offline"));
        assert!(lines.next().unwrap().starts_with("  team: "), "{out}");
        let src = git_market(&t.path().join("origin"), false);
        write_cfg(&c.paths, &src, &["demo"]);
        assert_eq!(say(&c, &["sync"], "").unwrap(), "");
        std::fs::write(&c.paths.config_file, "").unwrap();
        assert_eq!(say(&c, &["sync"], "").unwrap(), "");
    }

    #[test]
    fn slash_sync_replies_with_the_shell_command() {
        let (c, t) = ctx(true, false);
        write_cfg(&c.paths, &git_market(&t.path().join("origin"), false), &["demo"]);
        assert_eq!(say(&c, &["sync"], "").unwrap(), "run in your shell: zygo plugins sync");
        assert!(!c.paths.marketplaces_dir.exists());
    }

    #[test]
    #[ignore = "timing: git sync inside the 20 s plugins TOTAL bound"]
    fn enable_syncs_shows_code_and_asks() {
        let (c, t) = ctx(false, false);
        write_cfg(&c.paths, &git_market(&t.path().join("origin"), true), &[]);
        let (out, shown) = prompted(&c, &["enable", "team:demo"], None, &mut Cursor::new("y\n"));
        let out = out.unwrap();
        assert!(std::fs::read_to_string(&c.paths.config_file).unwrap().contains("enable = [\"team:demo\"]"));
        let digest = stored_entry(&c.paths, "demo").digest.unwrap();
        assert!(shown.contains("code file scripts/run.sh") && shown.ends_with("approve team:demo? [y/N] "), "{shown}");
        assert!(out.ends_with(&format!("approved team:demo {digest}")), "{out}");
        assert!(is_approved_in(&c.paths.approvals_file, "team", "demo", &digest));
        let (c, t) = ctx(false, false);
        write_cfg(&c.paths, &git_market(&t.path().join("origin"), true), &[]);
        let out = say(&c, &["enable", "team:demo"], "n\n").unwrap();
        assert!(out.ends_with("not approved; run zygo plugins approve team:demo"), "{out}");
        assert!(approvals_absent(&c));
        let (c, t) = ctx(false, false);
        write_cfg(&c.paths, &git_market(&t.path().join("origin"), true), &[]);
        let (out, shown) = prompted(&c, &["enable", "team:demo", "--yes"], None, &mut Cursor::new(""));
        let out = out.unwrap();
        assert!(out.ends_with(&format!("approved team:demo {}", stored_entry(&c.paths, "demo").digest.unwrap())), "{out}");
        assert!(out.contains("code file scripts/run.sh") && !out.contains("[y/N]") && shown.is_empty(), "{out}");
    }

    #[test]
    #[ignore = "timing: git sync inside the 20 s plugins TOTAL bound"]
    fn enable_keeps_the_other_stored_plugins() {
        let (c, t) = ctx(false, false);
        let origin = t.path().join("origin");
        let src = git_market_with(&origin, &["demo", "other", "third"], false);
        let a = git(&origin, &["rev-parse", "HEAD"]);
        write_cfg(&c.paths, &src, &["demo", "other"]);
        assert_eq!(say(&c, &["sync"], "").unwrap(), "");
        let before = store::read_manifest(&c.paths.plugin_store).unwrap().unwrap();
        // Upstream moves on (other removed, tag moved); the pin stays A.
        std::fs::remove_dir_all(origin.join("plugins/other")).unwrap();
        commit_market(&origin, &["demo", "third"], false, "Say B.");
        let out = say(&c, &["enable", "team:third"], "").unwrap();
        assert!(out.contains("team:third has no code; nothing to approve"), "{out}");
        let after = store::read_manifest(&c.paths.plugin_store).unwrap().unwrap();
        for p in ["demo", "other"] {
            assert_eq!(after.plugins.iter().find(|e| e.plugin == p), before.plugins.iter().find(|e| e.plugin == p));
        }
        assert!(after.plugins.iter().any(|e| e.plugin == "third"));
        assert_eq!(after.marketplaces["team"].commit, a);
    }

    #[test]
    #[ignore = "timing: git sync inside the 20 s plugins TOTAL bound"]
    fn enable_of_a_plugin_without_code_needs_no_approval() {
        let (c, t) = ctx(false, false);
        write_cfg(&c.paths, &git_market(&t.path().join("origin"), false), &[]);
        assert_eq!(say(&c, &["enable", "team:demo"], "").unwrap(), "enabled team:demo\nteam:demo has no code; nothing to approve");
    }

    #[test]
    #[ignore = "timing: git sync inside the 20 s plugins TOTAL bound"]
    fn slash_enable_and_disable_reply_applies_at_next_launch() {
        let (c, t) = ctx(true, false);
        write_cfg(&c.paths, &git_market(&t.path().join("origin"), true), &[]);
        assert_eq!(say(&c, &["enable", "team:demo"], "").unwrap(), "run in your shell: zygo plugins enable team:demo");
        assert!(!std::fs::read_to_string(&c.paths.config_file).unwrap().contains("enable"));
        assert!(!c.paths.plugin_store.exists());
        say(&with(&c.paths, false, false), &["enable", "team:demo"], "n\n").unwrap();
        let out = say(&c, &["enable", "team:demo"], "").unwrap();
        assert!(out.contains("code file scripts/run.sh"), "{out}");
        assert!(out.ends_with("run: zygo plugins approve team:demo\napplies at next launch"), "{out}");
        assert!(approvals_absent(&c));
        assert_eq!(say(&c, &["disable", "team:demo"], "").unwrap(), "disabled team:demo\napplies at next launch");
        let cli = with(&c.paths, false, false);
        assert_eq!(say(&cli, &["disable", "team:demo"], "").unwrap(), "disabled team:demo");
        assert!(!say(&cli, &["enable", "team:demo"], "n\n").unwrap().contains("applies at next launch"));
    }

    #[test]
    fn disable_of_a_required_plugin_is_refused() {
        let (c, t) = ctx(false, false);
        let src = git_market(&t.path().join("origin"), false);
        let team = format!("[[marketplace]]\nname = \"team\"\nsource = {}\nref = \"v1.0.0\"\nrequired = [\"demo\"]\n", toml::Value::String(src));
        std::fs::write(&c.paths.config_file, "# mine\n").unwrap();
        let err = run(&c, &["disable", "team:demo"], Some(&team), &mut Cursor::new(String::new())).unwrap_err();
        assert_eq!(err.to_string(), "refused: team:demo is required by the team config");
        assert_eq!(std::fs::read_to_string(&c.paths.config_file).unwrap(), "# mine\n");
    }

    #[test]
    #[ignore = "timing: git sync inside the 20 s plugins TOTAL bound"]
    fn approve_shows_the_items_the_digest_covers_and_checks_the_fresh_digest() {
        let (c, _t) = ctx(false, false);
        synced(&c, true);
        let e = stored_entry(&c.paths, "demo");
        let dir = store::entry_dir(&c.paths.plugin_store, &e);
        let rendered = zygo_plugins::check::check_dir(&dir).render();
        let (out, shown) = prompted(&c, &["approve", "team:demo"], None, &mut Cursor::new("y\n"));
        let out = out.unwrap();
        // Round 2 item 10: the listing and the question are written once, to the prompt stream only.
        assert_eq!(shown, format!("{}\napprove team:demo? [y/N] ", rendered.trim_end()));
        assert_eq!(out, format!("approved team:demo {}", e.digest.clone().unwrap()));
        let (c, _t) = ctx(false, false);
        synced(&c, true);
        let e = stored_entry(&c.paths, "demo");
        let script = store::entry_dir(&c.paths.plugin_store, &e).join("scripts/run.sh");
        std::fs::write(&script, "echo hi\nx").unwrap();
        let err = say(&c, &["approve", "team:demo"], "y\n").unwrap_err().to_string();
        assert!(err.contains("store copy differs from its manifest; run zygo plugins sync"), "{err}");
        assert!(approvals_absent(&c));
    }

    #[test]
    #[ignore = "timing: git sync inside the 20 s plugins TOTAL bound"]
    fn approval_is_refused_inside_an_agent_session() {
        let (c, _t) = ctx(false, true);
        synced(&c, true);
        assert_eq!(say(&c, &["approve", "team:demo"], "y\n").unwrap_err().to_string(), REFUSED);
        assert_eq!(say(&c, &["approve", "team:demo", "--yes"], "").unwrap_err().to_string(), REFUSED);
        assert!(approvals_absent(&c));
        let (c, t) = ctx(false, true);
        write_cfg(&c.paths, &git_market(&t.path().join("origin"), true), &[]);
        assert_eq!(say(&c, &["enable", "team:demo", "--yes"], "").unwrap_err().to_string(), REFUSED);
        assert!(std::fs::read_to_string(&c.paths.config_file).unwrap().contains("enable = [\"team:demo\"]"));
        assert!(stored_entry(&c.paths, "demo").digest.is_some());
        assert!(approvals_absent(&c));
        let cli = with(&c.paths, false, false);
        assert!(say(&cli, &["approve", "team:demo", "--yes"], "").unwrap().contains("approved team:demo "));
        assert!(!approvals_absent(&c));
        let (c, _t) = ctx(false, false);
        synced(&c, true);
        assert!(say(&c, &["approve", "team:demo"], "y\n").unwrap().contains("approved team:demo "));
    }

    #[test]
    #[ignore = "timing: 5 s store lock wait contended by forked git children; git sync inside the 20 s TOTAL bound"]
    fn approval_prompt_holds_no_lock_and_rechecks_the_digest() {
        let (c, _t) = ctx(false, false);
        synced(&c, true);
        // A 5 s wait, as the product uses: a git child forked by a parallel test briefly holds a copy of the lock fd.
        let free = Rc::new(Cell::new(false));
        let (f, root) = (free.clone(), c.paths.plugin_store.clone());
        let mut input = HookedInput::new(move || f.set(store::lock_until(&root, true, Instant::now() + std::time::Duration::from_secs(5)).unwrap().is_some()), "y\n");
        let out = run(&c, &["approve", "team:demo"], None, &mut input).unwrap();
        assert!(free.get(), "a store lock was held while waiting");
        assert!(out.contains("approved team:demo "), "{out}");
        let (c, _t) = ctx(false, false);
        synced(&c, true);
        let script = store::entry_dir(&c.paths.plugin_store, &stored_entry(&c.paths, "demo")).join("scripts/run.sh");
        let mut input = HookedInput::new(move || std::fs::write(&script, "echo hi\nx").unwrap(), "y\n");
        let err = run(&c, &["approve", "team:demo"], None, &mut input).unwrap_err().to_string();
        assert!(err.contains("store changed while waiting; run zygo plugins approve team:demo again"), "{err}");
        assert!(approvals_absent(&c));
    }

    #[test]
    #[ignore = "timing: git sync inside the 20 s plugins TOTAL bound"]
    fn slash_approve_lists_code_and_approves_nothing() {
        let (c, _t) = ctx(true, false);
        synced(&c, true);
        let out = say(&c, &["approve", "team:demo"], "y\n").unwrap();
        assert!(out.contains("code file scripts/run.sh") && out.ends_with("run: zygo plugins approve team:demo"), "{out}");
        assert!(approvals_absent(&c));
    }

    #[test]
    #[ignore = "timing: git sync inside the 20 s plugins TOTAL bound"]
    fn approve_of_a_plugin_without_code_or_not_in_the_store() {
        let (c, _t) = ctx(false, false);
        synced(&c, false);
        assert_eq!(say(&c, &["approve", "team:demo"], "").unwrap(), "team:demo has no code; nothing to approve");
        assert_eq!(say(&c, &["approve", "team:ghost"], "").unwrap_err().to_string(), "team:ghost is not in the store; run zygo plugins sync");
        assert_eq!(say(&c, &["approve", "nope:demo"], "").unwrap_err().to_string(), "marketplace nope is not configured");
        let (c, _t) = ctx(false, false);
        synced(&c, true);
        say(&c, &["approve", "team:demo", "--yes"], "").unwrap();
        assert_eq!(say(&c, &["approve", "team:demo"], "").unwrap(), "team:demo code already approved");
    }

    /// M33: a team pre-approval counts only for the stored digest.
    #[test]
    #[ignore = "timing: git sync inside the 20 s plugins TOTAL bound"]
    fn team_pre_approval_counts_only_for_the_same_digest() {
        let (c, t) = ctx(false, false);
        let src = git_market(&t.path().join("origin"), true);
        let team = |digest: &str| format!("{}approvals = [{{ plugin = \"demo\", digest = \"{digest}\" }}]\n", mp(&src, &["demo"]));
        let other = team(&"a".repeat(64));
        assert_eq!(run(&c, &["sync"], Some(&other), &mut Cursor::new("")).unwrap(), "");
        let digest = stored_entry(&c.paths, "demo").digest.unwrap();
        assert_ne!(digest, "a".repeat(64));
        assert_eq!(run(&c, &["approve", "team:demo"], Some(&team(&digest)), &mut Cursor::new("")).unwrap(), "team:demo code already approved");
        let out = run(&c, &["approve", "team:demo"], Some(&other), &mut Cursor::new("n\n")).unwrap();
        assert_eq!(out, "not approved; run zygo plugins approve team:demo");
    }

    /// M34: a manifest entry changed while the prompt waits refuses the approval, even when the old copy still checks.
    #[test]
    #[ignore = "timing: git sync inside the 20 s plugins TOTAL bound"]
    fn approval_refuses_a_manifest_entry_changed_during_the_prompt() {
        let (c, _t) = ctx(false, false);
        synced(&c, true);
        let root = c.paths.plugin_store.clone();
        let mut input = HookedInput::new(move || {
            let mut m = store::read_manifest(&root).unwrap().unwrap();
            m.plugins[0].commit = "0".repeat(40);
            store::write_manifest(&root, &m).unwrap();
        }, "y\n");
        let err = run(&c, &["approve", "team:demo"], None, &mut input).unwrap_err().to_string();
        assert!(err.contains("store changed while waiting; run zygo plugins approve team:demo again"), "{err}");
        assert!(approvals_absent(&c));
    }

    #[test]
    fn confirm_accepts_only_y_and_yes() {
        for (text, want) in [("y\n", true), ("YES\n", true), (" yes \n", true), ("n\n", false), ("", false), ("\n", false), ("yep\n", false)] {
            assert_eq!(super::confirm(&mut Cursor::new(text.to_string())), want, "{text:?}");
        }
    }

    #[test]
    fn usage_errors() {
        let (c, _t) = ctx(false, false);
        for args in [&["frobnicate"][..], &["enable"], &["approve", "a", "b"], &["enable", "nocolon"]] {
            let e = say(&c, args, "").unwrap_err();
            assert!(e.downcast_ref::<super::super::UsageError>().is_some(), "{args:?}");
            assert_eq!(e.to_string(), USAGE);
        }
    }

    const DEMO_ROW: &str = "team:demo 1.0.0 (default) skills: hello · mcp: - · code: none · missing: -";

    #[test]
    #[ignore = "timing: git sync inside the 20 s plugins TOTAL bound"]
    fn list_prints_the_lines_or_no_plugins() {
        let (c, _t) = ctx(false, false);
        std::fs::write(&c.paths.config_file, "").unwrap();
        assert_eq!(say(&c, &["list"], "").unwrap(), "no plugins");
        assert_eq!(say(&c, &[], "").unwrap(), "no plugins");
        synced(&c, false);
        assert_eq!(say(&c, &["list"], "").unwrap(), DEMO_ROW);
        assert_eq!(say(&c, &[], "").unwrap(), DEMO_ROW);
    }

    #[test]
    #[ignore = "timing: git sync inside the 20 s plugins TOTAL bound"]
    fn slash_list_is_allowed() {
        let (c, _t) = ctx(true, false);
        synced(&c, false);
        assert_eq!(say(&c, &["list"], "").unwrap(), DEMO_ROW);
        assert_eq!(say(&c, &[], "").unwrap(), DEMO_ROW);
    }

    #[test]
    #[ignore = "timing: git sync inside the 20 s plugins TOTAL bound"]
    fn sync_prints_the_delivery_details_after_the_failures() {
        let (c, t) = ctx(false, false);
        let root = t.path();
        let a = git_market(&root.join("origin-a"), false);
        let b = git_market(&root.join("origin-b"), false);
        let block = |name: &str, src: &str| mp(src, &["demo"]).replace("name = \"team\"", &format!("name = \"{name}\""));
        let gone = root.join("gone").display().to_string();
        std::fs::write(&c.paths.config_file, block("m1", &a) + &block("m2", &b) + &block("m3", &gone)).unwrap();
        let out = say(&c, &["sync"], "").unwrap();
        let lines: Vec<&str> = out.lines().collect();
        let detail = "  plugin m2:demo: name already delivered from m1";
        assert_eq!(lines.last(), Some(&detail), "{out}");
        let failure = lines.iter().position(|l| l.starts_with("  m3: ")).expect(&out);
        assert!(failure < lines.len() - 1, "{out}");
    }

    // ---- N72: printed plugin and marketplace text is escaped ----

    const FORGED_MP: &str = "[[marketplace]]\nname = \"x\\u001b[2K\\n  forged: line\"\nsource = \"/nonexistent\"\nref = \"v1.0.0\"\n";

    #[test]
    fn sync_failure_scope_is_escaped() {
        let (c, _t) = ctx(false, false);
        std::fs::write(&c.paths.config_file, FORGED_MP).unwrap();
        let out = say(&c, &["sync"], "").unwrap();
        assert!(out.contains("  x\\u{1b}[2K\\n  forged: line: unsafe marketplace name"), "{out}");
        assert!(!out.contains('\u{1b}') && !out.lines().any(|l| l.starts_with("  forged")), "{out}");
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "timing: git sync inside the 20 s plugins TOTAL bound"]
    fn sync_failure_reason_is_escaped() {
        let (c, t) = ctx(false, false);
        let origin = t.path().join("origin");
        let src = git_market(&origin, false);
        write(&origin, "plugins/demo/skills/x\u{1b}[2K\n  forged/SKILL.md", "---\nname: hello\ndescription: Says hello\n---\n");
        git(&origin, &["add", "-A"]);
        git(&origin, &["commit", "-q", "-m", "forged"]);
        git(&origin, &["tag", "-f", "v1.0.0"]);
        write_cfg(&c.paths, &src, &["demo"]);
        let out = say(&c, &["sync"], "").unwrap();
        assert!(out.contains("  team:demo: check failed: bad-name skills/x\\u{1b}[2K\\n  forged/SKILL.md"), "{out}");
        assert!(!out.contains('\u{1b}') && !out.lines().any(|l| l.starts_with("  forged")), "{out}");
    }

    #[test]
    #[ignore = "timing: git sync inside the 20 s plugins TOTAL bound"]
    fn enable_shows_the_check_failure_reason() {
        let (c, t) = ctx(false, false);
        let origin = t.path().join("origin");
        let src = git_market(&origin, false);
        write(&origin, "plugins/demo/skills/Bad_Name/SKILL.md", "---\nname: Bad_Name\ndescription: Says hello\n---\n");
        git(&origin, &["add", "-A"]);
        git(&origin, &["commit", "-q", "-m", "bad"]);
        git(&origin, &["tag", "-f", "v1.0.0"]);
        write_cfg(&c.paths, &src, &[]);
        let e = say(&c, &["enable", "team:demo"], "").unwrap_err().to_string();
        assert!(e.starts_with("enabled team:demo\n"), "{e}");
        assert!(e.ends_with("\n  team:demo: check failed: bad-name skills/Bad_Name/SKILL.md (skill name \"Bad_Name\")\nteam:demo is not in the store; run zygo plugins sync"), "{e}");
    }

    /// M1: a git failure while the stored copy is kept; `approval` succeeds, so the failure lines go out through `out`.
    #[cfg(unix)]
    #[test]
    #[ignore = "timing: git sync inside the 20 s plugins TOTAL bound"]
    fn enable_escapes_a_git_failure_it_prints() {
        use std::os::unix::fs::PermissionsExt;
        let (c, t) = ctx(false, false);
        synced(&c, false);
        let stub = t.path().join("forging-git");
        std::fs::write(&stub, format!(
            "#!/bin/sh\ncase \" $* \" in *\" clone \"*|*\" fetch \"*) printf 'fatal: x\\033[2K forged\\n' >&2; exit 128;; esac\nexec '{}' \"$@\"\n",
            which::which("git").unwrap().display())).unwrap();
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        let opts = SyncOpts { git_program: Some(stub), per_marketplace: WIDE, ..Default::default() };
        let args = vec!["enable".to_string(), "team:demo".to_string()];
        let out = super::run_io(&c, &args, None, Some(toml::Table::new()), &mut Cursor::new(String::new()), &mut vec![], &opts).unwrap();
        assert!(out.contains("  team: git failed: fatal: x\\u{1b}[2K forged"), "{out}");
        assert!(out.ends_with("team:demo has no code; nothing to approve") && !out.contains('\u{1b}'), "{out}");
    }
}
