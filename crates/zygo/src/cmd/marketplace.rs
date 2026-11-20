//! `zygo marketplace add|list|update|remove` (spec §15.8, §15.7, §17, §17.1).

use std::io::{BufRead, Write};

use anyhow::anyhow;
use zygo_config::display::display_source;
use zygo_config::{load, load_with_builtin, LoadContext, Merged};
use zygo_plugins::marketplace::{add_in, list_in, remove_in, render_plugin, update_in, AddOpts, Enable, Env};
use zygo_plugins::check::esc;
use zygo_plugins::select::PluginId;
use zygo_plugins::sync::SyncOpts;

use super::plugins::approval;
use super::verbs::VerbCtx;
use super::UsageError;

const USAGE: &str = "zygo marketplace: usage: marketplace add <source> [--name <n>] [--ref <tag|commit>] [--enable all|<plugin>,…] | list | update [<name> | --all] [--accept] | remove <name>";

pub fn verb(ctx: &VerbCtx, args: &[String]) -> anyhow::Result<String> {
    run(ctx, args, super::verbs::team(ctx, "marketplace")?, &mut std::io::stdin().lock())
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

fn usage() -> anyhow::Error { UsageError(USAGE.into()).into() }

/// `--flag value` or `--flag=value` at `words[*i]`; `*i` moves past the value.
fn flag_value<'a>(words: &[&'a str], i: &mut usize, flag: &str) -> anyhow::Result<Option<&'a str>> {
    let w = words[*i];
    if w == flag {
        *i += 1;
        return words.get(*i).copied().map(Some).ok_or_else(usage);
    }
    Ok(w.strip_prefix(flag).and_then(|r| r.strip_prefix('=')))
}

/// `add <source> [--name <n>] [--ref <r>] [--enable all|a,b]`; `sync` is on (D10).
/// The last value is the index in `words` of the source.
fn parse_add(words: &[&str]) -> anyhow::Result<(String, AddOpts, usize)> {
    let mut opts = AddOpts { sync: true, ..Default::default() };
    let mut source = None;
    let mut source_at = 0;
    let mut i = 1;
    while i < words.len() {
        if let Some(v) = flag_value(words, &mut i, "--name")? {
            opts.name = Some(v.to_string());
        } else if let Some(v) = flag_value(words, &mut i, "--ref")? {
            opts.git_ref = Some(v.to_string());
        } else if let Some(v) = flag_value(words, &mut i, "--enable")? {
            let list: Vec<&str> = v.split(',').collect();
            if list.iter().any(|p| p.is_empty()) || (list.contains(&"all") && list.len() > 1) { return Err(usage()); }
            opts.enable = if v == "all" { Enable::All } else { Enable::Plugins(list.iter().map(|p| p.to_string()).collect()) };
        } else if words[i].starts_with('-') || source.replace(words[i]).is_some() {
            return Err(usage());
        } else {
            source_at = i;
        }
        i += 1;
    }
    Ok((source.ok_or_else(usage)?.to_string(), opts, source_at))
}

/// The command a session tells the user to run. C3: each argument is checked on its own (a `--flag=value` word by its value). The source
/// word (`source_at`, chosen by position) with userinfo prints as `<source>`, any other word with userinfo as `<hidden>`; a word with none prints as typed.
fn shell_command(words: &[&str], source_at: Option<usize>) -> String {
    fn flag_eq(w: &str) -> Option<(&str, &str)> { w.split_once('=').filter(|_| w.starts_with("--")) }
    let carries = |w: &str| {
        let v = flag_eq(w).map_or(w, |(_, v)| v);
        display_source(v) != v
    };
    let (mut src, mut other) = (false, false);
    let shown: Vec<String> = words.iter().enumerate().map(|(i, w)| {
        if !carries(w) { return w.to_string(); }
        if Some(i) == source_at { src = true; return "<source>".into(); }
        other = true;
        match flag_eq(w) { Some((f, _)) => format!("{f}=<hidden>"), None => "<hidden>".into() }
    }).collect();
    let notes: Vec<&str> = [(src, "<source> is the source you typed, credentials not shown"), (other, "<hidden> is another argument with credentials, not shown")]
        .into_iter().filter_map(|(on, n)| on.then_some(n)).collect();
    let tail = if notes.is_empty() { String::new() } else { format!(" ({})", notes.join("; ")) };
    format!("run in your shell: zygo marketplace {}{tail}", shown.join(" "))
}

/// `run_with` with the stream the approval prompts are written to and the git seam.
pub fn run_io(ctx: &VerbCtx, args: &[String], team: Option<toml::Table>, builtin: Option<toml::Table>, input: &mut dyn BufRead,
    prompt: &mut dyn Write, git: &SyncOpts) -> anyhow::Result<String> {
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    let load_cfg = || -> anyhow::Result<Merged> {
        let lc = LoadContext { paths: ctx.paths.clone(), repo: None, team: team.clone() };
        match &builtin { None => load(&lc), Some(b) => load_with_builtin(&lc, b.clone()) }.map_err(|e| anyhow!("zygo marketplace: {e}"))
    };
    let env = Env { paths: &ctx.paths, load: &load_cfg, git };
    let mut out: Vec<String> = vec![];
    // The approval step of D7, D9, D10: a shell outside a session asks; a session prints the command.
    let mut approve = |ids: Vec<PluginId>, out: &mut Vec<String>| -> anyhow::Result<()> {
        if ids.is_empty() { return Ok(()); }
        let cfg = load_cfg()?;
        for id in ids {
            if ctx.session_dir.is_some() {
                out.push(format!("run: zygo plugins approve {id}"));
            } else {
                out.push(approval(ctx, &cfg, &id, false, input, prompt).unwrap_or_else(|e| format!("  {id}: {}", esc(&e.to_string()))));
            }
        }
        Ok(())
    };
    match words.as_slice() {
        ["add", ..] => {
            let (src, opts, source_at) = parse_add(&words)?;
            if ctx.slash { return Ok(shell_command(&words, Some(source_at))); }
            let r = add_in(&env, &src, &opts)?;
            out.push(format!("marketplace {} {} {}", r.name, r.git_ref, r.commit));
            out.extend(r.plugins.iter().map(|p| render_plugin(p).trim_end().to_string()));
            out.extend(r.skipped.iter().map(|s| format!("plugin {}:{}: skipped, source is not a relative path", r.name, esc(s))));
            out.push(format!("{} marketplace {} {} {}", if r.replaced { "replaced" } else { "added" }, r.name, r.git_ref, r.commit));
            out.extend(r.enabled.iter().map(|id| format!("enabled {id}")));
            if let Some(s) = &r.sync {
                for l in &s.lines { if !out.contains(l) { out.push(l.clone()); } }
                out.extend(s.failures.iter().map(|(scope, why)| format!("  {}: {}", esc(scope), esc(why))));
            }
            let coded = r.enabled.iter().filter(|id| r.plugins.iter()
                .any(|p| p.name == id.plugin && p.digest.is_some() && p.violations.is_empty())).cloned().collect();
            approve(coded, &mut out)?;
        }
        ["list"] => {
            let rows = list_in(&env)?;
            return Ok(if rows.is_empty() { "no marketplaces".to_string() } else { rows.iter().map(|r| r.line()).collect::<Vec<_>>().join("\n") });
        }
        ["update", rest @ ..] => {
            let mut name = None;
            let (mut all, mut accept) = (false, false);
            for w in rest {
                match *w {
                    "--all" => all = true,
                    "--accept" => accept = true,
                    w if w.starts_with('-') => return Err(usage()),
                    w => if name.replace(w).is_some() { return Err(usage()) },
                }
            }
            if all && name.is_some() { return Err(usage()); }
            if ctx.slash { return Ok(shell_command(&words, None)); }
            let r = update_in(&env, name, accept, true)?;
            let cfg = load_cfg()?;
            let dash = |s: &str| if s.is_empty() { "-".to_string() } else { s.to_string() };
            for m in &r.moves {
                out.push(format!("marketplace {}: {} {} -> {} {}", esc(&m.name), dash(&esc(&m.from_ref)), dash(&esc(&m.from_commit)), esc(&m.to_ref), m.to_commit));
            }
            for n in &r.current {
                let git_ref = cfg.config.marketplaces.iter().find(|m| m.name == *n).map(|m| m.git_ref.as_str()).unwrap_or_default();
                out.push(format!("marketplace {}: up to date at {}", esc(n), dash(&esc(git_ref))));
            }
            out.extend(r.lines.iter().cloned());
            for c in &r.changes {
                out.push(format!("plugin {}: {} -> {}", c.id, c.old_version.as_deref().unwrap_or("new"), c.new_version));
                if c.old_digest != c.new_digest {
                    out.push("  code changed".into());
                    out.extend(c.block.trim_end().lines().map(str::to_string));
                }
            }
            if let Some(s) = &r.sync { out.extend(s.failures.iter().map(|(scope, why)| format!("  {}: {}", esc(scope), esc(why)))); }
            let changed = r.changes.iter().filter(|c| c.new_digest.is_some() && c.new_digest != c.old_digest).map(|c| c.id.clone()).collect();
            approve(changed, &mut out)?;
            if !r.failures.is_empty() {
                out.extend(r.failures.iter().map(|(n, why)| format!("marketplace {}: {}", esc(n), esc(why))));
                return Err(super::EscapedLines::new(&out).into());
            }
        }
        ["remove", name] => {
            remove_in(&env, name)?;
            return Ok(format!("removed marketplace {}", esc(name)));
        }
        _ => return Err(usage()),
    }
    Ok(out.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::path::Path;
    use std::process::Command;
    use std::time::Duration;
    use tempfile::TempDir;
    use zygo_core::paths::Paths;
    use zygo_core::session_dir::SessionDir;
    use zygo_plugins::approve::is_approved_in;
    use zygo_plugins::marketplace::Enable;
    use zygo_plugins::store;

    const NULL: &str = if cfg!(windows) { "NUL" } else { "/dev/null" };

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

    const WIDE: Duration = Duration::from_secs(120);

    /// `SyncOpts` whose git calls read an empty global config file (never the developer's).
    fn opts(dir: &Path) -> SyncOpts {
        let file = dir.join("empty-gitconfig");
        std::fs::write(&file, "").unwrap();
        SyncOpts { per_marketplace: WIDE, git_config_global: Some(file), ..Default::default() }
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

    /// Commits `plugins` (name, `scripts/run.sh` text or `""` for no code) at `ver` and tags `v<ver>`.
    fn publish(dir: &Path, ver: &str, plugins: &[(&str, &str)]) {
        let entries: Vec<String> = plugins.iter()
            .map(|(n, _)| format!(r#"{{"name":"{n}","source":"./plugins/{n}","description":"Demo","version":"{ver}"}}"#)).collect();
        write(dir, ".claude-plugin/marketplace.json", &format!(r#"{{"name":"team","owner":{{"name":"Owner"}},"plugins":[{}]}}"#, entries.join(",")));
        for (n, script) in plugins {
            write(dir, &format!("plugins/{n}/.claude-plugin/plugin.json"),
                &format!(r#"{{"name":"{n}","version":"{ver}","description":"Demo plugin","license":"MIT"}}"#));
            write(dir, &format!("plugins/{n}/skills/hello/SKILL.md"), "---\nname: hello\ndescription: Says hello\n---\nSay hello.\n");
            if !script.is_empty() { write(dir, &format!("plugins/{n}/scripts/run.sh"), script); }
        }
        git(dir, &["add", "-A"]);
        git(dir, &["commit", "-q", "-m", ver]);
        git(dir, &["tag", &format!("v{ver}")]);
    }

    /// A repository at `dir` with `plugins` at 1.0.0; its path.
    fn market(dir: &Path, plugins: &[(&str, &str)]) -> String {
        std::fs::create_dir_all(dir).unwrap();
        git(dir, &["init", "-q", "-b", "main"]);
        publish(dir, "1.0.0", plugins);
        dir.display().to_string()
    }

    fn args(a: &[&str]) -> Vec<String> { a.iter().map(|s| s.to_string()).collect() }

    /// The result and what was written to the prompt stream; the built-in layer is `builtin` (default empty).
    fn prompted_with(c: &VerbCtx, a: &[&str], builtin: &str, input: &str) -> (anyhow::Result<String>, String) {
        let mut prompt = vec![];
        let git = opts(c.paths.cache_dir.parent().unwrap());
        let r = run_io(c, &args(a), None, Some(builtin.parse().unwrap()), &mut Cursor::new(input.to_string()), &mut prompt, &git);
        (r, String::from_utf8(prompt).unwrap())
    }

    fn prompted(c: &VerbCtx, a: &[&str], input: &str) -> (anyhow::Result<String>, String) { prompted_with(c, a, "", input) }

    fn run(c: &VerbCtx, a: &[&str], input: &str) -> anyhow::Result<String> { prompted(c, a, input).0 }

    fn stored_entry(paths: &Paths, plugin: &str) -> store::StoredEntry {
        store::read_manifest(&paths.plugin_store).unwrap().unwrap().plugins.into_iter().find(|e| e.plugin == plugin).unwrap()
    }

    /// Runs the shell command named by a `run in your shell: zygo marketplace <args>` reply, answering `y`.
    fn run_shell(c: &VerbCtx, out: &str) -> String {
        let line = out.strip_prefix("run in your shell: zygo marketplace ").expect("a shell command reply");
        let words: Vec<&str> = line.split_whitespace().collect();
        run(&with(&c.paths, false, false), &words, "y\n").unwrap()
    }

    fn sha(dir: &Path) -> String { git(dir, &["rev-parse", "v1.0.0^{commit}"]) }

    #[test]
    fn add_lists_code_and_asks_for_each_enabled_code_plugin() {
        let (c, t) = ctx(false, false);
        let dir = t.path().join("origin");
        let src = market(&dir, &[("demo", "echo hi\n"), ("plain", "")]);
        let (out, shown) = prompted(&c, &["add", &src, "--enable", "all"], "y\n");
        let out = out.unwrap();
        let digest = stored_entry(&c.paths, "demo").digest.unwrap();
        assert!(shown.contains("code file scripts/run.sh") && shown.ends_with("approve team:demo? [y/N] "), "{shown}");
        assert_eq!(shown.matches("approve team:").count(), 1, "{shown}");
        for want in ["plugin demo 1.0.0: ok", "  license MIT", &format!("added marketplace team v1.0.0 {}", sha(&dir)), "enabled team:demo"] {
            assert!(out.contains(want), "{want} not in {out}");
        }
        assert!(out.ends_with(&format!("approved team:demo {digest}")), "{out}");
        assert!(is_approved_in(&c.paths.approvals_file, "team", "demo", &digest));
    }

    #[test]
    fn add_declined_approves_nothing() {
        let (c, t) = ctx(false, false);
        let src = market(&t.path().join("origin"), &[("demo", "echo hi\n")]);
        let out = run(&c, &["add", &src, "--enable", "all"], "n\n").unwrap();
        assert!(out.ends_with("not approved; run zygo plugins approve team:demo"), "{out}");
        assert!(!c.paths.approvals_file.exists());
    }

    #[test]
    fn add_without_enable_asks_nothing() {
        let (c, t) = ctx(false, false);
        let dir = t.path().join("origin");
        let src = market(&dir, &[("demo", "echo hi\n")]);
        let (out, shown) = prompted(&c, &["add", &src], "");
        let out = out.unwrap();
        assert_eq!(shown, "");
        assert!(out.contains("scripts/run.sh") && out.contains(&format!("added marketplace team v1.0.0 {}", sha(&dir))), "{out}");
        assert!(!c.paths.plugin_store.exists() && !c.paths.approvals_file.exists());
    }

    #[test]
    fn update_asks_only_for_changed_digests() {
        let (c, t) = ctx(false, false);
        let dir = t.path().join("origin");
        let src = market(&dir, &[("a", "echo a\n"), ("b", "echo b\n")]);
        run(&c, &["add", &src, "--enable", "all"], "y\ny\n").unwrap();
        publish(&dir, "1.1.0", &[("a", "echo changed\n"), ("b", "echo b\n")]);
        let (out, shown) = prompted(&c, &["update", "team"], "y\n");
        let out = out.unwrap();
        assert_eq!(shown.matches("approve team:a? [y/N] ").count(), 1, "{shown}");
        assert!(!shown.contains("team:b"), "{shown}");
        let new = stored_entry(&c.paths, "a").digest.unwrap();
        for want in ["plugin team:a: 1.0.0 -> 1.1.0", "  code changed"] { assert!(out.contains(want), "{want} not in {out}"); }
        assert!(out.ends_with(&format!("approved team:a {new}")), "{out}");
    }

    #[test]
    fn slash_add_replies_with_the_shell_command_and_it_approves() {
        let (c, t) = ctx(true, false);
        let src = market(&t.path().join("origin"), &[("demo", "echo hi\n")]);
        let (out, shown) = prompted(&c, &["add", &src, "--enable", "demo"], "");
        let out = out.unwrap();
        assert_eq!(out, format!("run in your shell: zygo marketplace add {src} --enable demo"));
        assert!(shown.is_empty() && !c.paths.config_file.exists() && !c.paths.plugin_store.exists() && !c.paths.approvals_file.exists());
        let done = run_shell(&c, &out);
        assert!(done.ends_with(&format!("approved team:demo {}", stored_entry(&c.paths, "demo").digest.unwrap())), "{done}");
    }

    #[test]
    fn slash_update_replies_with_the_shell_command_and_it_approves() {
        let (c, t) = ctx(false, false);
        let dir = t.path().join("origin");
        let src = market(&dir, &[("demo", "echo hi\n")]);
        run(&c, &["add", &src, "--enable", "all"], "y\n").unwrap();
        let old = stored_entry(&c.paths, "demo").digest.unwrap();
        publish(&dir, "1.1.0", &[("demo", "echo changed\n")]);
        let out = run(&with(&c.paths, true, false), &["update", "team"], "").unwrap();
        assert_eq!(out, "run in your shell: zygo marketplace update team");
        assert_eq!(stored_entry(&c.paths, "demo").digest.unwrap(), old);
        let done = run_shell(&c, &out);
        let new = stored_entry(&c.paths, "demo").digest.unwrap();
        assert_ne!(old, new);
        assert!(done.ends_with(&format!("approved team:demo {new}")), "{done}");
    }

    #[test]
    fn add_in_an_agent_session_prints_the_approve_command() {
        let (c, t) = ctx(false, true);
        let src = market(&t.path().join("origin"), &[("demo", "echo hi\n")]);
        let (out, shown) = prompted(&c, &["add", &src, "--enable", "all"], "");
        assert!(out.unwrap().contains("run: zygo plugins approve team:demo"));
        assert_eq!(stored_entry(&c.paths, "demo").plugin, "demo");
        assert!(shown.is_empty() && !c.paths.approvals_file.exists());
    }

    #[test]
    fn update_name_with_all_is_a_usage_error() {
        let (c, _t) = ctx(false, false);
        let e = run(&c, &["update", "team", "--all"], "").unwrap_err();
        assert!(e.downcast_ref::<UsageError>().is_some(), "{e}");
    }

    #[test]
    fn bare_update_updates_every_marketplace() {
        let (c, t) = ctx(false, false);
        let (d1, d2) = (t.path().join("origin"), t.path().join("origin2"));
        let (s1, s2) = (market(&d1, &[("demo", "")]), market(&d2, &[("demo", "")]));
        run(&c, &["add", &s1], "").unwrap();
        run(&c, &["add", &s2, "--name", "other"], "").unwrap();
        let (old1, old2) = (sha(&d1), sha(&d2));
        publish(&d1, "1.1.0", &[("demo", "")]);
        publish(&d2, "1.1.0", &[("demo", "")]);
        let out = run(&c, &["update"], "").unwrap();
        let new = |d: &Path| git(d, &["rev-parse", "v1.1.0^{commit}"]);
        assert!(out.contains(&format!("marketplace team: v1.0.0 {old1} -> v1.1.0 {}", new(&d1))), "{out}");
        assert!(out.contains(&format!("marketplace other: v1.0.0 {old2} -> v1.1.0 {}", new(&d2))), "{out}");
    }

    #[test]
    fn add_flags_parse_both_value_forms() {
        let (src, o, at) = parse_add(&["add", "/s", "--name", "n", "--ref=v1", "--enable", "a,b"]).unwrap();
        assert_eq!(at, 1);
        assert_eq!((src.as_str(), o.name.as_deref(), o.git_ref.as_deref(), o.sync), ("/s", Some("n"), Some("v1"), true));
        assert_eq!(o.enable, Enable::Plugins(vec!["a".into(), "b".into()]));
        assert_eq!(parse_add(&["add", "/s", "--enable=all"]).unwrap().1.enable, Enable::All);
        for bad in [&["add", "/s", "--enable", "all,x"][..], &["add", "/s", "--enable", "x,all"], &["add", "/s", "--bogus"],
            &["add", "/s", "--name"], &["add", "/s", "--enable", ""], &["add"], &["add", "/s", "/t"]] {
            let e = parse_add(bad).unwrap_err();
            assert!(e.downcast_ref::<UsageError>().is_some(), "{bad:?}");
        }
    }

    #[test]
    fn list_and_remove_print_their_lines() {
        let (c, t) = ctx(false, false);
        assert_eq!(run(&c, &["list"], "").unwrap(), "no marketplaces");
        let src = market(&t.path().join("origin"), &[("demo", "")]);
        run(&c, &["add", &src], "").unwrap();
        let builtin = "[[marketplace]]\nname = \"act101\"\nsource = \"/x/act\"\n";
        let out = prompted_with(&c, &["list"], builtin, "").0.unwrap();
        let sha = sha(&t.path().join("origin"));
        assert_eq!(out, format!("act101 /x/act - - built-in plugins: -\nteam {src} v1.0.0 {sha} user plugins: demo"));
        let e = prompted_with(&c, &["remove", "act101"], builtin, "").0.unwrap_err();
        assert_eq!(e.to_string(), "marketplace act101 is built in and cannot be removed; disable its plugins instead: zygo plugins disable act101:<plugin>");
        assert_eq!(run(&c, &["remove", "team"], "").unwrap(), "removed marketplace team");
        assert_eq!(run(&c, &["list"], "").unwrap(), "no marketplaces");
    }

    #[test]
    fn update_failure_is_an_error_with_every_line() {
        let (c, t) = ctx(false, false);
        let dir = t.path().join("origin");
        let src = market(&dir, &[("demo", "")]);
        run(&c, &["add", &src], "").unwrap();
        let gone = t.path().join("missing").display().to_string();
        let cfg = std::fs::read_to_string(&c.paths.config_file).unwrap();
        std::fs::write(&c.paths.config_file, format!("{cfg}\n[[marketplace]]\nname = \"gone\"\nsource = {}\nref = \"v1.0.0\"\n", toml::Value::String(gone))).unwrap();
        publish(&dir, "1.1.0", &[("demo", "")]);
        let e = run(&c, &["update"], "").unwrap_err().to_string();
        assert!(e.contains("marketplace gone: ") && e.contains("marketplace team: v1.0.0 "), "{e}");
    }

    // ---- N55: output drops source userinfo ----

    fn token() -> String { format!("tk{}", zygo_core::ids::ulid()) }

    fn put_config(c: &VerbCtx, text: &str) {
        std::fs::create_dir_all(c.paths.config_file.parent().unwrap()).unwrap();
        std::fs::write(&c.paths.config_file, text).unwrap();
    }

    /// A git that fails every `clone`, naming the URL it was given.
    #[cfg(unix)]
    fn failing_git(dir: &Path) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let real = which::which("git").expect("git on PATH");
        let stub = dir.join("failing-git");
        std::fs::write(&stub, format!(
            "#!/bin/sh\nfor a; do case \"$a\" in *://*) u=$a;; esac; done\ncase \" $* \" in *\" clone \"*) echo \"fatal: unable to access '$u/': boom\" >&2; exit 128;; esac\nexec '{}' \"$@\"\n",
            real.display())).unwrap();
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        stub
    }

    #[test]
    fn list_prints_the_source_without_userinfo() {
        let (c, _t) = ctx(false, false);
        let tk = token();
        let text = format!("[[marketplace]]\nname = \"a\"\nsource = \"git+https://bob:{tk}@h.example/o/r.git\"\nref = \"v1.0.0\"\n[[marketplace]]\nname = \"b\"\nsource = \"https://{tk}@h.example/o/r\"\nref = \"v1.0.0\"\n");
        put_config(&c, &text);
        let out = run(&c, &["list"], "").unwrap();
        assert!(out.contains("a git+https://h.example/o/r.git v1.0.0 - user plugins: -"), "{out}");
        assert!(out.contains("b https://h.example/o/r v1.0.0 - user plugins: -"), "{out}");
        assert!(!out.contains(&tk), "{out}");
        assert_eq!(std::fs::read_to_string(&c.paths.config_file).unwrap(), text);
    }

    #[test]
    fn slash_add_keeps_a_source_with_no_userinfo() {
        let (c, _t) = ctx(true, false);
        for a in [&["add", "/tmp/a@b", "--ref", "v1.0.0"][..], &["add", "github:o/r"], &["add", "git+https://h.example/o/r.git"]] {
            assert_eq!(run(&c, a, "").unwrap(), format!("run in your shell: zygo marketplace {}", a.join(" ")));
        }
    }

    #[test]
    fn slash_add_hides_a_source_with_userinfo() {
        let (c, _t) = ctx(true, false);
        let tk = token();
        let note = "(<source> is the source you typed, credentials not shown)";
        let out = run(&c, &["add", &format!("git+https://bob:{tk}@h/o/r.git"), "--ref", "v1.0.0"], "").unwrap();
        assert_eq!(out, format!("run in your shell: zygo marketplace add <source> --ref v1.0.0 {note}"));
        let out = run(&c, &["add", &format!("{tk}@h:o/r")], "").unwrap();
        assert_eq!(out, format!("run in your shell: zygo marketplace add <source> {note}"));
        assert!(!out.contains(&tk));
    }

    #[test]
    fn slash_add_picks_the_source_by_position() {
        let (c, _t) = ctx(true, false);
        let tk = token();
        let s = format!("git+https://bob:{tk}@h/o/r.git");
        let out = run(&c, &["add", "--name", &s, &s], "").unwrap();
        assert_eq!(out, "run in your shell: zygo marketplace add --name <hidden> <source> (<source> is the source you typed, credentials not shown; <hidden> is another argument with credentials, not shown)");
        assert!(!out.contains(&tk));
    }

    #[test]
    fn slash_add_hides_each_argument_with_userinfo() {
        let (c, _t) = ctx(true, false);
        let tk = token();
        let v = format!("https://u:{tk}@h/x");
        let note = "(<hidden> is another argument with credentials, not shown)";
        for (a, shown) in [
            (vec!["add", "/tmp/x", "--name", &v], "add /tmp/x --name <hidden>"),
            (vec!["add", "/tmp/x", "--ref", &v], "add /tmp/x --ref <hidden>"),
            (vec!["add", "/tmp/x", &format!("--name={v}")], "add /tmp/x --name=<hidden>"),
            (vec!["add", "/tmp/x", &format!("--ref={v}")], "add /tmp/x --ref=<hidden>"),
        ] {
            let out = run(&c, &a, "").unwrap();
            assert_eq!(out, format!("run in your shell: zygo marketplace {shown} {note}"));
            assert!(!out.contains(&tk), "{out}");
        }
        let out = run(&c, &["add", "/tmp/x", "--name=/tmp/a@b", "--ref", "v1.0.0"], "").unwrap();
        assert_eq!(out, "run in your shell: zygo marketplace add /tmp/x --name=/tmp/a@b --ref v1.0.0");
    }

    #[test]
    fn remove_prints_only_the_name() {
        let (c, _t) = ctx(false, false);
        let tk = token();
        put_config(&c, &format!("[[marketplace]]\nname = \"a\"\nsource = \"git+https://bob:{tk}@h.example/o/r.git\"\nref = \"v1.0.0\"\n"));
        let out = run(&c, &["remove", "a"], "").unwrap();
        assert_eq!(out, "removed marketplace a");
        assert!(!out.contains(&tk));
    }

    #[cfg(unix)]
    #[test]
    fn add_and_update_failures_are_scrubbed() {
        let (c, t) = ctx(false, false);
        let tk = token();
        let git = SyncOpts { git_program: Some(failing_git(t.path())), ..opts(t.path()) };
        let go = |a: &[&str]| run_io(&c, &args(a), None, Some(toml::Table::new()), &mut Cursor::new(String::new()), &mut vec![], &git);
        let src = format!("git+https://bob:{tk}@h.example/o/r.git");
        let e = go(&["add", &src]).unwrap_err().to_string();
        assert_eq!(e, "git failed: fatal: unable to access 'https://h.example/o/r.git/': boom");
        put_config(&c, &format!("[[marketplace]]\nname = \"a\"\nsource = \"{src}\"\nref = \"v1.0.0\"\ncommit = \"{}\"\n", "a".repeat(40)));
        let e = go(&["update", "a"]).unwrap_err().to_string();
        assert!(e.contains("marketplace a: git failed: fatal: unable to access 'https://h.example/o/r.git/': boom"), "{e}");
        assert!(!e.contains(&tk), "{e}");
    }


    // ---- N72: printed marketplace text is escaped ----

    const FORGED_MP: &str = "[[marketplace]]\nname = \"x\\u001b[2K\\n  forged: line\"\nsource = \"/nonexistent\"\nref = \"v1.0.0\"\n";

    #[test]
    fn add_escapes_a_skipped_plugin_name() {
        let (c, t) = ctx(false, false);
        let dir = t.path().join("origin");
        let src = market(&dir, &[("demo", "")]);
        let mj = serde_json::json!({"name": "team", "owner": {"name": "Owner"}, "plugins": [
            {"name": "demo", "source": "./plugins/demo", "description": "Demo", "version": "1.0.0"},
            {"name": "x\u{1b}[2K\nplugin team:forged: ok", "source": {"source": "github", "repo": "o/r"}}]}).to_string();
        write(&dir, ".claude-plugin/marketplace.json", &mj);
        git(&dir, &["commit", "-qam", "skip"]);
        git(&dir, &["tag", "-f", "v1.0.0"]);
        let out = run(&c, &["add", &src], "").unwrap();
        assert!(out.contains("plugin team:x\\u{1b}[2K\\nplugin team:forged: ok: skipped, source is not a relative path"), "{out}");
        assert!(!out.contains('\u{1b}') && !out.lines().any(|l| l.starts_with("plugin team:forged")), "{out}");
    }

    #[test]
    fn update_sync_failures_are_escaped() {
        let (c, t) = ctx(false, false);
        let dir = t.path().join("origin");
        let src = market(&dir, &[("demo", "")]);
        run(&c, &["add", &src, "--enable", "demo"], "").unwrap();
        let cfg = std::fs::read_to_string(&c.paths.config_file).unwrap();
        std::fs::write(&c.paths.config_file, format!("{cfg}\n{FORGED_MP}")).unwrap();
        publish(&dir, "1.1.0", &[("demo", "echo hi\n")]);
        let e = run(&c, &["update"], "").unwrap_err().to_string();
        assert!(e.contains("  x\\u{1b}[2K\\n  forged: line: unsafe marketplace name"), "{e}");
        assert!(!e.contains('\u{1b}') && !e.lines().any(|l| l.starts_with("  forged")), "{e}");
        // N1: the code-changed block keeps one line per item.
        assert!(e.lines().any(|l| l == "  code changed") && e.lines().any(|l| l.starts_with("  code file scripts/run.sh")), "{e}");
    }

    #[test]
    fn remove_escapes_the_name() {
        let (c, _t) = ctx(false, false);
        put_config(&c, "[[marketplace]]\nname = \"x\\u001b[2K\"\nsource = \"/x\"\nref = \"v1.0.0\"\n");
        assert_eq!(run(&c, &["remove", "x\u{1b}[2K"], "").unwrap(), "removed marketplace x\\u{1b}[2K");
    }

    #[test]
    fn name_errors_escape_the_name() {
        let (c, _t) = ctx(false, false);
        let e = run(&c, &["remove", "x\u{1b}[2K\nforged"], "").unwrap_err().to_string();
        assert_eq!(e, "marketplace x\\u{1b}[2K\\nforged is not configured");
        let e = run(&c, &["update", "x\u{1b}[2K\nforged"], "").unwrap_err().to_string();
        assert_eq!(e, "marketplace x\\u{1b}[2K\\nforged is not configured");
        let builtin = "[[marketplace]]\nname = \"x\\u001b[2K\"\nsource = \"/x/act\"\n";
        let e = prompted_with(&c, &["remove", "x\u{1b}[2K"], builtin, "").0.unwrap_err().to_string();
        assert_eq!(e, "marketplace x\\u{1b}[2K is built in and cannot be removed; disable its plugins instead: zygo plugins disable x\\u{1b}[2K:<plugin>");
    }
}
