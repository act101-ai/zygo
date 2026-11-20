//! `zygo marketplace` library tests (spec §15.8, §20 #38b). Every source is a local repository; no network, no
//! sleeps, no environment change.

mod common;

use common::*;
use zygo_config::{load_with_builtin, LoadContext, Merged};
use zygo_core::paths::Paths;
use zygo_plugins::marketplace::*;
use zygo_plugins::sync::SyncOpts;

struct T { fx: Fx, paths: Paths, git: SyncOpts }

impl T {
    fn new() -> T {
        let fx = Fx::new();
        let paths = paths(&fx);
        let git = opts(&fx);
        T { fx, paths, git }
    }
    fn write_config(&self, text: &str) {
        std::fs::create_dir_all(self.paths.config_file.parent().unwrap()).unwrap();
        std::fs::write(&self.paths.config_file, text).unwrap();
    }
    fn config(&self) -> String { std::fs::read_to_string(&self.paths.config_file).unwrap_or_default() }
}

fn cfg(t: &T, builtin: toml::Table, team: Option<&str>) -> anyhow::Result<Merged> {
    let ctx = LoadContext { paths: t.paths.clone(), repo: None, team: team.map(|x| x.parse().unwrap()) };
    Ok(load_with_builtin(&ctx, builtin)?)
}

fn table(s: &str) -> toml::Table { s.parse().unwrap() }

/// `f` with an `Env` over `t` with the given built-in and team layers.
fn with_layers<R>(t: &T, builtin: &str, team: Option<&str>, f: impl FnOnce(&Env) -> R) -> R {
    let builtin = table(builtin);
    let load = || cfg(t, builtin.clone(), team);
    f(&Env { paths: &t.paths, load: &load, git: &t.git })
}

fn with_env<R>(t: &T, f: impl FnOnce(&Env) -> R) -> R { with_layers(t, "", None, f) }

/// A marketplace `team` with plugins `demo` and `plain` at 1.0.0, tagged `v1.0.0`; `demo` has code.
fn market(t: &T) -> Repo {
    let r = market_repo_at(&t.fx, "origin", &[("demo", "1.0.0"), ("plain", "1.0.0")]);
    r.write("plugins/demo/scripts/run.sh", "echo one\n");
    r.commit("code");
    r.tag_force("v1.0.0");
    r
}

/// The plugins of `r` at `ver`, `demo`'s script `demo_script`; committed and tagged `v<ver>`.
fn release(r: &Repo, ver: &str, demo_script: &str) {
    market_json(r, &[("demo", ver), ("plain", ver)]);
    plugin_files(r, "plugins/demo", "demo", ver);
    plugin_files(r, "plugins/plain", "plain", ver);
    r.write("plugins/demo/scripts/run.sh", demo_script);
    r.commit(&format!("release {ver}"));
    r.tag(&format!("v{ver}"));
}

fn user_entries(t: &T) -> Vec<zygo_config::Marketplace> { cfg(t, toml::Table::new(), None).unwrap().config.marketplaces }

fn rev(r: &Repo, spec: &str) -> String { git(&r.dir, &["rev-parse", &format!("{spec}^{{commit}}")]) }

const A: fn() -> AddOpts = || AddOpts { sync: false, ..Default::default() };

mod listing {
    use super::*;

    #[test]
    fn row_has_name_source_ref_commit_origin_and_plugins() {
        let t = T::new();
        let r = market(&t);
        let src = r.src();
        let opts = AddOpts { git_ref: Some("v1.0.0".into()), ..A() };
        with_env(&t, |env| add_in(env, &src, &opts)).unwrap();
        let sha = rev(&r, "v1.0.0");
        let team = "[[marketplace]]\nname = \"corp\"\nsource = \"/x/corp\"\nref = \"v2.0.0\"\n";
        let builtin = "[[marketplace]]\nname = \"act101\"\nsource = \"/x/act\"\n";
        let rows = with_layers(&t, builtin, Some(team), list_in).unwrap();
        let by = |n: &str| rows.iter().find(|r| r.name == n).unwrap().clone();
        let tm = by("team");
        assert_eq!((tm.source.as_str(), tm.git_ref.as_str(), tm.commit.as_str(), tm.origin), (src.as_str(), "v1.0.0", sha.as_str(), "user"));
        assert_eq!(tm.plugins, ["demo", "plain"]);
        let corp = by("corp");
        assert_eq!((corp.origin, corp.commit.as_str(), corp.plugins.len()), ("team", "", 0));
        let act = by("act101");
        assert_eq!((act.origin, act.git_ref.as_str()), ("built-in", ""));
        assert_eq!(tm.line(), format!("team {src} v1.0.0 {sha} user plugins: demo, plain"));
        assert_eq!(corp.line(), "corp /x/corp v2.0.0 - team plugins: -");
        assert_eq!(act.line(), "act101 /x/act - - built-in plugins: -");
    }

    #[test]
    fn a_config_value_with_a_newline_is_one_escaped_row() {
        let t = T::new();
        t.write_config(&mp("a\nb", "/x/repo", "v1", Some("c1\nc2"), &[]));
        let team = "[[marketplace]]\nname = \"c\\nd\"\nsource = \"/x/corp\"\nref = \"v1\"\n";
        let rows = with_layers(&t, "", Some(team), list_in).unwrap();
        let text = rows.iter().map(Row::line).collect::<Vec<_>>().join("\n");
        assert_eq!(text.lines().count(), 2, "{text}");
        assert!(text.lines().any(|l| l.starts_with("a\\nb /x/repo v1 c1\\nc2 user plugins: -")), "{text}");
        assert!(text.lines().any(|l| l.starts_with("c\\nd /x/corp v1 - team plugins: -")), "{text}");
    }

    #[test]
    fn an_unsafe_name_is_listed_without_reading_a_cache() {
        let t = T::new();
        let outside = t.paths.marketplaces_dir.parent().unwrap().join("x/.claude-plugin");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::create_dir_all(&t.paths.marketplaces_dir).unwrap();
        std::fs::write(outside.join("marketplace.json"), r#"{"name":"x","plugins":[{"name":"leak"}]}"#).unwrap();
        t.write_config(&mp("../x", "/x/repo", "v1", None, &[]));
        let rows = with_env(&t, list_in).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].line(), "../x /x/repo v1 - user plugins: -");
    }
}

mod removing {
    use super::*;

    fn entry(name: &str, r: &str) -> String { mp(name, "/x/repo", r, None, &[]) }

    #[test]
    fn deletes_only_the_named_user_entry() {
        let t = T::new();
        let head = "# keep\n[plugins]\nenable = [\"team:demo\"]\n\n";
        t.write_config(&format!("{head}{}{}", entry("team", "v1"), entry("other", "v1")));
        with_env(&t, |env| remove_in(env, "other")).unwrap();
        assert_eq!(t.config(), format!("{head}{}", entry("team", "v1")));
        let names: Vec<String> = user_entries(&t).into_iter().map(|m| m.name).collect();
        assert_eq!(names, ["team"]);
    }

    #[test]
    fn builtin_and_team_entries_are_refused() {
        let t = T::new();
        t.write_config("# cfg\n");
        let builtin = "[[marketplace]]\nname = \"act101\"\nsource = \"/x/act\"\nref = \"v1\"\n";
        let team = "[[marketplace]]\nname = \"corp\"\nsource = \"/x/corp\"\nref = \"v1\"\n";
        let err = |n: &str| with_layers(&t, builtin, Some(team), |env| remove_in(env, n)).unwrap_err().to_string();
        assert_eq!(err("act101"), "marketplace act101 is built in and cannot be removed; disable its plugins instead: zygo plugins disable act101:<plugin>");
        assert_eq!(err("corp"), "marketplace corp is from the team config and cannot be removed; disable its plugins instead: zygo plugins disable corp:<plugin>");
        assert_eq!(t.config(), "# cfg\n");
    }

    #[test]
    fn a_user_entry_with_an_invalid_name_is_removable() {
        let t = T::new();
        t.write_config(&format!("{}{}", entry("Bad_Name", "v1"), entry("team", "v1")));
        with_env(&t, |env| remove_in(env, "Bad_Name")).unwrap();
        assert_eq!(t.config(), entry("team", "v1"));
    }

    #[test]
    fn unknown_name_is_refused() {
        let t = T::new();
        let e = with_env(&t, |env| remove_in(env, "nope")).unwrap_err();
        assert_eq!(e.to_string(), "marketplace nope is not configured");
        assert!(!t.paths.config_file.exists());
    }

    #[test]
    fn a_user_entry_over_a_team_entry_is_removable() {
        let t = T::new();
        t.write_config(&entry("corp", "v2"));
        let team = "[[marketplace]]\nname = \"corp\"\nsource = \"/x/repo\"\nref = \"v1\"\n";
        with_layers(&t, "", Some(team), |env| remove_in(env, "corp")).unwrap();
        let m = cfg(&t, toml::Table::new(), Some(team)).unwrap();
        assert_eq!(m.config.marketplaces[0].git_ref, "v1");
        assert_eq!(m.source_of("marketplace.corp"), zygo_config::Layer::Team);
    }

    #[test]
    fn remove_user_override_of_builtin_restores_the_release_pin() {
        let t = T::new();
        let builtin = "[[marketplace]]\nname = \"act101\"\nsource = \"/x/act\"\nref = \"v1.0.0\"\n";
        t.write_config(&entry("act101", "v1.1.0").replace("/x/repo", "/x/act"));
        with_layers(&t, builtin, None, |env| remove_in(env, "act101")).unwrap();
        assert!(!t.config().contains("[[marketplace]]"));
        let m = cfg(&t, table(builtin), None).unwrap();
        assert_eq!(m.config.marketplaces[0].git_ref, "v1.0.0");
        assert_eq!(m.source_of("marketplace.act101"), zygo_config::Layer::Builtin);
    }
}

/// A repository holding `demo` and one commit tagged with each of `tags`; never `market_repo_at`.
fn bare_repo(t: &T, dir: &str, tags: &[&str]) -> Repo {
    let r = Repo::new(&t.fx, dir);
    market_json(&r, &[("demo", "1.0.0")]);
    plugin_files(&r, "plugins/demo", "demo", "1.0.0");
    r.commit("first");
    for tag in tags { r.tag(tag); }
    r
}

/// The names in the marketplaces dir other than lock files.
fn cache_entries(t: &T) -> Vec<String> {
    std::fs::read_dir(&t.paths.marketplaces_dir).map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| !n.ends_with(".lock")).collect()).unwrap_or_default()
}

fn add_plain(t: &T, src: &str) -> anyhow::Result<AddReport> { with_env(t, |env| add_in(env, src, &A())) }

fn manifest(t: &T) -> zygo_plugins::store::Manifest { zygo_plugins::store::read_manifest(&t.paths.plugin_store).unwrap().unwrap() }

mod adding {
    use super::*;
    use std::time::{Duration, Instant};
    use zygo_plugins::store::PinEntry;
    use zygo_plugins::sync::{lock_marketplace, sync_with};

    #[test]
    fn highest_semver_tag_is_recorded_with_its_peeled_commit() {
        let t = T::new();
        let r = market(&t);
        r.commit("c1");
        r.tag("v1.2.0");
        r.commit("c2");
        r.tag("v1.9.0");
        r.tag("release-9");
        r.commit("c3");
        git(&r.dir, &["tag", "-a", "-m", "m", "v1.10.0"]);
        let src = r.src();
        let rep = add_plain(&t, &src).unwrap();
        assert_eq!(rep.git_ref, "v1.10.0");
        assert_eq!(rep.commit, rev(&r, "v1.10.0"));
        assert_ne!(rep.commit, git(&r.dir, &["rev-parse", "v1.10.0"]));
        let want = zygo_config::Marketplace { name: "team".into(), source: src, git_ref: "v1.10.0".into(), commit: Some(rep.commit), ..Default::default() };
        assert_eq!(user_entries(&t), [want]);
    }

    #[test]
    fn bare_and_v_prefixed_tags_compare_by_version() {
        let t = T::new();
        let r = bare_repo(&t, "origin", &["v1.2.0", "1.3.0"]);
        assert_eq!(add_plain(&t, &r.src()).unwrap().git_ref, "1.3.0");
    }

    #[test]
    fn no_semver_tag_is_refused_naming_ref() {
        let t = T::new();
        for (dir, tags) in [("o1", &[][..]), ("o2", &["release-9"][..])] {
            let r = bare_repo(&t, dir, tags);
            let e = add_plain(&t, &r.src()).unwrap_err().to_string();
            assert_eq!(e, format!("no semver tag in {}; pass --ref <tag|commit>", r.src()));
            assert!(e.contains("--ref"));
            assert!(!t.paths.config_file.exists());
            assert_eq!(cache_entries(&t), Vec::<String>::new());
        }
    }

    #[test]
    fn ref_accepts_a_tag_a_full_commit_and_an_abbreviated_commit() {
        let t = T::new();
        let r = market(&t);
        release(&r, "1.1.0", "echo two\n");
        let src = r.src();
        let sha = rev(&r, "v1.0.0");
        for given in ["v1.0.0", sha.as_str(), &sha[..7]] {
            let opts = AddOpts { git_ref: Some(given.into()), ..A() };
            let rep = with_env(&t, |env| add_in(env, &src, &opts)).unwrap();
            assert_eq!((rep.git_ref.as_str(), rep.commit.as_str()), (given, sha.as_str()));
        }
    }

    #[test]
    fn branch_ref_is_refused() {
        let t = T::new();
        let r = market(&t);
        let src = r.src();
        let err = |given: &str| {
            let opts = AddOpts { git_ref: Some(given.into()), ..A() };
            with_env(&t, |env| add_in(env, &src, &opts)).unwrap_err().to_string()
        };
        assert_eq!(err("main"), "ref is not a tag or a commit");
        assert!(!t.paths.config_file.exists());
        assert_eq!(err("-x"), "ref is not a valid tag name");
    }

    #[test]
    fn name_defaults_to_the_marketplace_name_and_name_overrides() {
        let t = T::new();
        let r = market(&t);
        add_plain(&t, &r.src()).unwrap();
        assert_eq!(user_entries(&t)[0].name, "team");
        let t = T::new();
        let r = market(&t);
        let opts = AddOpts { name: Some("mine".into()), ..A() };
        with_env(&t, |env| add_in(env, &r.src(), &opts)).unwrap();
        assert_eq!(user_entries(&t)[0].name, "mine");
        assert!(t.paths.marketplaces_dir.join("mine/.git").is_dir());
        assert!(!t.paths.marketplaces_dir.join("team").exists());
    }

    #[test]
    fn unsafe_name_is_refused() {
        let t = T::new();
        let r = market(&t);
        for bad in ["Bad Name", "../x"] {
            let opts = AddOpts { name: Some(bad.into()), ..A() };
            let e = with_env(&t, |env| add_in(env, &r.src(), &opts)).unwrap_err();
            assert_eq!(e.to_string(), "unsafe marketplace name");
            assert!(!t.paths.config_file.exists());
        }
    }

    #[test]
    fn each_plugin_is_listed_with_version_license_and_code() {
        let t = T::new();
        let r = market_repo_at(&t.fx, "origin", &[("demo", "1.0.0"), ("plain", "1.0.0"), ("bad", "1.0.0")]);
        plugin_files(&r, "plugins/bad", "bad", "1.0.1");
        r.write("plugins/demo/scripts/run.sh", "echo one\n");
        r.commit("code");
        r.tag_force("v1.0.0");
        let rep = add_plain(&t, &r.src()).unwrap();
        let names: Vec<&str> = rep.plugins.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["bad", "demo", "plain"]);
        let by = |n: &str| rep.plugins.iter().find(|p| p.name == n).unwrap();
        let demo = by("demo");
        assert_eq!((demo.version.as_str(), demo.license.as_deref()), ("1.0.0", Some("MIT")));
        assert!(demo.digest.is_some() && by("plain").digest.is_none());
        assert_eq!(by("bad").violations[0].rule.as_str(), "version-mismatch");
        let text = render_plugin(demo);
        assert!(text.contains("plugin demo 1.0.0: ok\n  license MIT\n  code file scripts/run.sh"), "{text}");
        assert!(text.contains("digest "));
        assert_eq!(user_entries(&t).len(), 1);
    }

    #[test]
    fn enable_all_enables_every_plugin_and_syncs() {
        let t = T::new();
        let r = market(&t);
        let src = r.src();
        let opts = AddOpts { enable: Enable::All, sync: true, ..Default::default() };
        let rep = with_env(&t, |env| add_in(env, &src, &opts)).unwrap();
        let ids: Vec<String> = rep.enabled.iter().map(|i| i.to_string()).collect();
        assert_eq!(ids, ["team:demo", "team:plain"]);
        let m = cfg(&t, toml::Table::new(), None).unwrap();
        assert_eq!(m.config.plugins.enable, ["team:demo", "team:plain"]);
        let man = manifest(&t);
        assert_eq!(man.plugins.len(), 2);
        assert!(man.plugins.iter().all(|e| e.commit == rep.commit));
        let pin = PinEntry { source: src, git_ref: "v1.0.0".into(), commit: rep.commit, layer: "user".into() };
        assert_eq!(man.marketplaces["team"], pin);
    }

    #[test]
    fn enable_list_enables_only_the_named_plugins() {
        let t = T::new();
        let r = market(&t);
        let opts = AddOpts { enable: Enable::Plugins(vec!["demo".into()]), sync: true, ..Default::default() };
        with_env(&t, |env| add_in(env, &r.src(), &opts)).unwrap();
        let m = cfg(&t, toml::Table::new(), None).unwrap();
        assert_eq!(m.config.plugins.enable, ["team:demo"]);
        let plugins: Vec<String> = manifest(&t).plugins.iter().map(|e| e.plugin.clone()).collect();
        assert_eq!(plugins, ["demo"]);
    }

    #[test]
    fn config_entry_is_written_while_the_marketplace_lock_is_held() {
        let t = T::new();
        let r = market(&t);
        let calls = std::cell::Cell::new(0);
        let load = || {
            calls.set(calls.get() + 1);
            // A git child forked by another test thread may hold a released lock for a moment: the free case waits.
            let wait = if calls.get() < 3 { Duration::ZERO } else { Duration::from_secs(10) };
            let busy = lock_marketplace(&t.paths, "team", Instant::now() + wait).unwrap().is_none();
            let written = t.config().contains("name = \"team\"");
            match calls.get() {
                1 => assert!(busy && !written, "call 1"),
                2 => assert!(busy && written, "call 2"),
                _ => assert!(!busy && written, "call 3"),
            }
            cfg(&t, toml::Table::new(), None)
        };
        let opts = AddOpts { enable: Enable::All, sync: true, ..Default::default() };
        add_in(&Env { paths: &t.paths, load: &load, git: &t.git }, &r.src(), &opts).unwrap();
        assert_eq!(calls.get(), 3);
    }

    #[test]
    fn an_add_after_a_removal_survives_a_prune_of_the_old_config() {
        let t = T::new();
        let r = market(&t);
        let opts = AddOpts { enable: Enable::All, sync: true, ..Default::default() };
        with_env(&t, |env| add_in(env, &r.src(), &opts)).unwrap();
        with_env(&t, |env| remove_in(env, "team")).unwrap();
        let old = cfg(&t, toml::Table::new(), None).unwrap();
        assert!(old.config.marketplaces.is_empty());
        add_plain(&t, &r.src()).unwrap();
        sync_with(&old, &t.paths, WIDE_TOTAL, &t.git);
        assert!(manifest(&t).marketplaces.contains_key("team"));
        assert!(t.paths.marketplaces_dir.join("team").exists());
    }

    #[test]
    fn same_name_replaces_the_entry_in_place() {
        let t = T::new();
        let r = market(&t);
        release(&r, "1.1.0", "echo two\n");
        t.write_config(&format!("# keep\n{}[plugins]\ndisable = [\"team:plain\"]\n", mp("team", &r.src(), "v1.0.0", None, &[])));
        let opts = AddOpts { git_ref: Some("v1.1.0".into()), ..A() };
        let rep = with_env(&t, |env| add_in(env, &r.src(), &opts)).unwrap();
        assert!(rep.replaced);
        let text = t.config();
        assert!(text.starts_with("# keep\n") && text.contains("[plugins]\ndisable = [\"team:plain\"]"), "{text}");
        assert_eq!(text.matches("[[marketplace]]").count(), 1);
        assert_eq!(user_entries(&t)[0].git_ref, "v1.1.0");
    }

    #[test]
    fn builtin_source_cannot_change() {
        let t = T::new();
        let r = market(&t);
        let builtin = "[[marketplace]]\nname = \"act101\"\nsource = \"/x/act\"\nref = \"v1\"\n";
        let opts = AddOpts { name: Some("act101".into()), ..A() };
        let e = with_layers(&t, builtin, None, |env| add_in(env, &r.src(), &opts)).unwrap_err();
        assert_eq!(e.to_string(), "marketplace act101 is built in; its source cannot change");
        assert!(!t.paths.config_file.exists());
        assert_eq!(cache_entries(&t), Vec::<String>::new());
    }

    #[test]
    fn cache_is_the_hardened_checkout_and_no_probe_dir_is_left() {
        let t = T::new();
        let r = market(&t);
        let rep = add_plain(&t, &r.src()).unwrap();
        let cache = t.paths.marketplaces_dir.join("team");
        let attrs = std::fs::read_to_string(cache.join(".git/info/attributes")).unwrap();
        assert_eq!(attrs, "* -filter -diff -merge -text -ident -working-tree-encoding\n");
        assert_eq!(git(&cache, &["rev-parse", "HEAD"]), rep.commit);
        assert!(cache_entries(&t).iter().all(|n| !n.starts_with(".probe-")));
    }

    #[test]
    fn sync_false_leaves_the_store_alone() {
        let t = T::new();
        let r = market(&t);
        let opts = AddOpts { enable: Enable::All, sync: false, ..Default::default() };
        let rep = with_env(&t, |env| add_in(env, &r.src(), &opts)).unwrap();
        assert!(rep.sync.is_none() && !t.paths.plugin_store.exists());
        assert_eq!(cfg(&t, toml::Table::new(), None).unwrap().config.plugins.enable, ["team:demo", "team:plain"]);
    }

    #[test]
    fn pre_release_tag_counts_and_the_release_is_higher() {
        let t = T::new();
        let r = market(&t);
        r.commit("rc");
        r.tag("v1.1.0-rc1");
        assert_eq!(add_plain(&t, &r.src()).unwrap().git_ref, "v1.1.0-rc1");
        r.commit("release");
        r.tag("v1.1.0");
        assert_eq!(add_plain(&t, &r.src()).unwrap().git_ref, "v1.1.0");
    }

    #[test]
    fn marketplace_without_a_name_needs_name() {
        let t = T::new();
        let r = bare_repo(&t, "origin", &["v1.0.0"]);
        r.write(".claude-plugin/marketplace.json", r#"{"plugins":[{"name":"demo","source":"./plugins/demo","version":"1.0.0"}]}"#);
        r.commit("no name");
        r.tag_force("v1.0.0");
        let e = add_plain(&t, &r.src()).unwrap_err();
        assert_eq!(e.to_string(), "marketplace has no name; pass --name <n>");
        assert!(!t.paths.config_file.exists());
        let opts = AddOpts { name: Some("x".into()), ..A() };
        with_env(&t, |env| add_in(env, &r.src(), &opts)).unwrap();
        assert_eq!(user_entries(&t)[0].name, "x");
    }

    #[test]
    fn an_unlisted_enable_name_is_refused_and_nothing_is_written() {
        let t = T::new();
        let r = market(&t);
        let opts = AddOpts { enable: Enable::Plugins(vec!["nope".into()]), ..A() };
        let e = with_env(&t, |env| add_in(env, &r.src(), &opts)).unwrap_err();
        assert_eq!(e.to_string(), "team has no plugin nope");
        assert!(!t.paths.config_file.exists());
        assert_eq!(cache_entries(&t), Vec::<String>::new());
    }

    /// `team` at `v1.0.0` with `demo` (ok), `bad` (a check violation), `zygo` (reserved) and `far` (skipped).
    fn mixed(t: &T) -> Repo {
        let r = market_repo_at(&t.fx, "origin", &[("demo", "1.0.0"), ("bad", "1.0.0"), ("zygo", "1.0.0")]);
        plugin_files(&r, "plugins/bad", "bad", "1.0.1");
        let entry = |n: &str, s: &str| format!(r#"{{"name":"{n}","source":"{s}","description":"Demo","version":"1.0.0"}}"#);
        let list = [entry("demo", "./plugins/demo"), entry("bad", "./plugins/bad"), entry("zygo", "./plugins/zygo"), entry("far", "plugins/far")];
        r.write(".claude-plugin/marketplace.json", format!(r#"{{"name":"team","owner":{{"name":"Owner"}},"plugins":[{}]}}"#, list.join(",")));
        r.commit("mixed");
        r.tag_force("v1.0.0");
        r
    }

    #[test]
    fn enable_all_leaves_out_violations_and_the_reserved_name() {
        let t = T::new();
        let r = mixed(&t);
        let opts = AddOpts { enable: Enable::All, ..A() };
        let rep = with_env(&t, |env| add_in(env, &r.src(), &opts)).unwrap();
        assert_eq!(rep.skipped, ["far"]);
        let ids: Vec<String> = rep.enabled.iter().map(|i| i.to_string()).collect();
        assert_eq!(ids, ["team:demo"]);
        assert_eq!(cfg(&t, toml::Table::new(), None).unwrap().config.plugins.enable, ["team:demo"]);
    }

    #[test]
    fn an_enable_name_with_violations_reserved_or_skipped_is_refused_and_nothing_is_written() {
        let t = T::new();
        let r = mixed(&t);
        for p in ["bad", "zygo", "far"] {
            let opts = AddOpts { enable: Enable::Plugins(vec!["demo".into(), p.into()]), ..A() };
            let e = with_env(&t, |env| add_in(env, &r.src(), &opts)).unwrap_err();
            assert_eq!(e.to_string(), format!("team has no plugin {p}"));
            assert!(!t.paths.config_file.exists());
            assert_eq!(cache_entries(&t), Vec::<String>::new());
        }
    }

    #[test]
    fn stale_probe_dirs_are_removed_and_a_fresh_one_is_kept() {
        let t = T::new();
        let r = market(&t);
        let probe = || t.paths.marketplaces_dir.join(format!(".probe-{}", zygo_core::ids::ulid()));
        let (old, fresh) = (probe(), probe());
        for d in [&old, &fresh] { std::fs::create_dir_all(d).unwrap(); }
        let ago = std::time::SystemTime::now() - std::time::Duration::from_secs(120);
        std::fs::File::open(&old).unwrap().set_modified(ago).unwrap();
        add_plain(&t, &r.src()).unwrap();
        assert!(!old.exists() && fresh.exists());
    }

    #[test]
    fn sync_lines_are_limited_to_the_added_marketplace() {
        let t = T::new();
        let r = market(&t);
        t.write_config(&mp("gone", &t.fx.path("missing").display().to_string(), "v1.0.0", None, &["x"]));
        let opts = AddOpts { enable: Enable::All, sync: true, ..Default::default() };
        let rep = with_env(&t, |env| add_in(env, &r.src(), &opts)).unwrap();
        let s = rep.sync.unwrap();
        assert!(s.failures.iter().all(|(scope, _)| !scope.starts_with("gone")), "{:?}", s.failures);
        assert!(s.lines.iter().all(|l| !l.starts_with("plugins:")), "{:?}", s.lines);
        let synced: Vec<String> = s.synced.iter().map(|i| i.to_string()).collect();
        assert_eq!(synced, ["team:demo", "team:plain"]);
    }
}

mod updating {
    use super::*;
    use zygo_plugins::sync::sync_with;

    fn all() -> AddOpts { AddOpts { enable: Enable::All, sync: true, ..Default::default() } }

    fn added(t: &T, r: &Repo) -> AddReport { with_env(t, |env| add_in(env, &r.src(), &all())).unwrap() }

    fn upd(t: &T, name: Option<&str>, accept: bool) -> UpdateReport {
        with_env(t, |env| update_in(env, name, accept, true)).unwrap()
    }

    /// One more commit on `r`, tagged `tag`.
    fn tagged(r: &Repo, tag: &str) {
        r.commit(&format!("for {tag}"));
        r.tag(tag);
    }

    #[test]
    fn moves_to_the_highest_tag_above_the_current_one() {
        let t = T::new();
        let r = market(&t);
        let first = added(&t, &r);
        tagged(&r, "v1.0.5");
        tagged(&r, "v1.1.0");
        r.tag("v0.9.0");
        let rep = upd(&t, Some("team"), false);
        let want = Move { name: "team".into(), from_ref: "v1.0.0".into(), to_ref: "v1.1.0".into(), from_commit: first.commit, to_commit: rev(&r, "v1.1.0") };
        assert_eq!(rep.moves, std::slice::from_ref(&want));
        let e = &user_entries(&t)[0];
        assert_eq!((e.git_ref.as_str(), e.commit.as_deref()), ("v1.1.0", Some(want.to_commit.as_str())));
        let pin = &manifest(&t).marketplaces["team"];
        assert_eq!((pin.commit.as_str(), pin.layer.as_str()), (want.to_commit.as_str(), "user"));
        assert!(rep.sync.is_some());
    }

    #[test]
    fn no_higher_tag_changes_nothing() {
        let t = T::new();
        let r = market(&t);
        added(&t, &r);
        let before = t.config();
        let rep = upd(&t, Some("team"), false);
        assert!(rep.moves.is_empty() && rep.lines.is_empty() && rep.sync.is_none());
        assert_eq!(rep.current, ["team"]);
        assert_eq!(t.config(), before);
    }

    #[test]
    fn moved_tag_is_kept_without_accept_and_taken_with_it() {
        let t = T::new();
        let r = market(&t);
        let first = added(&t, &r);
        r.commit("moved");
        r.tag_force("v1.0.0");
        let before = t.config();
        let rep = upd(&t, Some("team"), false);
        assert_eq!(rep.lines, ["marketplace team: tag v1.0.0 moved; run zygo marketplace update team --accept"]);
        assert!(rep.moves.is_empty());
        assert_eq!(t.config(), before);
        let rep = upd(&t, Some("team"), true);
        let new = rev(&r, "v1.0.0");
        assert_ne!(new, first.commit);
        assert_eq!((rep.moves[0].to_ref.as_str(), rep.moves[0].to_commit.as_str()), ("v1.0.0", new.as_str()));
        assert_eq!(user_entries(&t)[0].commit.as_deref(), Some(new.as_str()));
        let man = manifest(&t);
        assert_eq!(man.marketplaces["team"].commit, new);
        assert!(man.plugins.iter().filter(|e| e.plugin == "demo").all(|e| e.commit == new));
        let again = sync_with(&cfg(&t, toml::Table::new(), None).unwrap(), &t.paths, WIDE_TOTAL, &t.git);
        assert!(again.lines.iter().all(|l| !l.contains("moved")), "{:?}", again.lines);
    }

    #[test]
    fn changes_list_version_and_digest_changes() {
        let t = T::new();
        let r = market(&t);
        added(&t, &r);
        release(&r, "1.1.0", "echo two\n");
        let rep = upd(&t, Some("team"), false);
        let by = |n: &str| rep.changes.iter().find(|c| c.id.to_string() == n).unwrap().clone();
        let demo = by("team:demo");
        assert_eq!((demo.old_version.as_deref(), demo.new_version.as_str()), (Some("1.0.0"), "1.1.0"));
        assert!(demo.old_digest.is_some() && demo.new_digest.is_some() && demo.old_digest != demo.new_digest);
        assert!(demo.block.contains("scripts/run.sh"));
        let plain = by("team:plain");
        assert_eq!((plain.old_version.as_deref(), plain.new_version.as_str()), (Some("1.0.0"), "1.1.0"));
        assert!(plain.old_digest.is_none() && plain.new_digest.is_none());
        release(&r, "1.2.0", "echo two\n");
        let rep = upd(&t, Some("team"), false);
        let demo = rep.changes.iter().find(|c| c.id.to_string() == "team:demo").unwrap();
        assert!(demo.old_digest.is_some() && demo.old_digest == demo.new_digest);
    }

    #[test]
    fn all_updates_every_marketplace_and_names_a_failure() {
        let t = T::new();
        let r1 = market(&t);
        let r2 = market_repo_at(&t.fx, "origin2", &[("demo", "1.0.0")]);
        added(&t, &r1);
        let opts = AddOpts { name: Some("other".into()), ..all() };
        with_env(&t, |env| add_in(env, &r2.src(), &opts)).unwrap();
        t.write_config(&format!("{}{}", t.config(), mp("gone", &t.fx.path("missing").display().to_string(), "v1.0.0", None, &[])));
        tagged(&r1, "v1.1.0");
        tagged(&r2, "v1.1.0");
        let rep = with_env(&t, |env| update_in(env, None, false, true)).unwrap();
        let moved: Vec<&str> = rep.moves.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(moved, ["team", "other"]);
        assert_eq!(rep.failures.len(), 1);
        assert_eq!(rep.failures[0].0, "gone");
        assert!(rep.sync.is_some());
    }

    #[test]
    fn team_entry_is_overridden_by_a_user_entry_keeping_default_on() {
        let t = T::new();
        let r = market(&t);
        let team = format!("{}required = [\"plain\"]\n", mp("corp", &r.src(), "v1.0.0", None, &["demo"]));
        release(&r, "1.1.0", "echo two\n");
        let rep = with_layers(&t, "", Some(&team), |env| update_in(env, Some("corp"), false, true)).unwrap();
        let to = rev(&r, "v1.1.0");
        assert_eq!(rep.moves.len(), 1);
        let text = t.config();
        assert_eq!(text.matches("[[marketplace]]").count(), 1, "{text}");
        let m = cfg(&t, toml::Table::new(), Some(&team)).unwrap();
        let e = &m.config.marketplaces[0];
        assert_eq!((e.git_ref.as_str(), e.commit.as_deref(), e.source.as_str()), ("v1.1.0", Some(to.as_str()), r.src().as_str()));
        assert_eq!((&e.default_on[..], &e.required[..]), (&["demo".to_string()][..], &["plain".to_string()][..]));
        assert_eq!(m.source_of("marketplace.corp"), zygo_config::Layer::User);
    }

    #[test]
    fn builtin_entry_moves_ref_and_commit_only() {
        let t = T::new();
        let r = market(&t);
        let builtin = format!("[[marketplace]]\nname = \"act101\"\nsource = \"{}\"\nref = \"v1.0.0\"\ndefault_on = [\"demo\"]\n", r.src());
        release(&r, "1.1.0", "echo two\n");
        with_layers(&t, &builtin, None, |env| update_in(env, Some("act101"), false, true)).unwrap();
        let to = rev(&r, "v1.1.0");
        let doc = t.config().parse::<toml::Table>().unwrap();
        let user = doc["marketplace"][0].as_table().unwrap();
        let mut keys: Vec<&str> = user.keys().map(String::as_str).collect();
        keys.sort();
        assert_eq!(keys, ["commit", "name", "ref", "source"]);
        let m = cfg(&t, table(&builtin), None).unwrap();
        let e = &m.config.marketplaces[0];
        assert_eq!((e.git_ref.as_str(), e.commit.as_deref(), e.source.as_str()), ("v1.1.0", Some(to.as_str()), r.src().as_str()));
        assert_eq!(m.source_of("marketplace.act101"), zygo_config::Layer::Builtin);
    }

    #[test]
    fn unknown_name_is_refused() {
        let t = T::new();
        let e = with_env(&t, |env| update_in(env, Some("nope"), false, true)).unwrap_err();
        assert_eq!(e.to_string(), "marketplace nope is not configured");
    }

    #[test]
    fn commit_ref_is_left_unchanged() {
        let t = T::new();
        let r = market(&t);
        let sha = rev(&r, "v1.0.0");
        let opts = AddOpts { git_ref: Some(sha.clone()), ..all() };
        with_env(&t, |env| add_in(env, &r.src(), &opts)).unwrap();
        release(&r, "1.1.0", "echo two\n");
        let before = t.config();
        let rep = upd(&t, Some("team"), false);
        assert_eq!(rep.lines, [format!("team: pinned to commit {}; nothing to update", &sha[..7])]);
        assert!(rep.moves.is_empty() && rep.current.is_empty() && rep.changes.is_empty() && rep.sync.is_none());
        assert_eq!(t.config(), before);
    }
}

mod output {
    use super::*;

    fn token() -> String { format!("tk{}", zygo_core::ids::ulid()) }

    #[test]
    fn row_line_shows_the_source_without_userinfo() {
        let t = token();
        let row = |source: String| Row { name: "m".into(), source, git_ref: "v1.0.0".into(), commit: "a".repeat(40), origin: "user", plugins: vec![] };
        assert_eq!(row(format!("git+https://bob:{t}@h/o/r.git")).line(), format!("m git+https://h/o/r.git v1.0.0 {} user plugins: -", "a".repeat(40)));
        let nl = row(format!("https://bob:{t}@h/o\nx")).line();
        assert!(nl.contains("https://h/o\\nx") && !nl.contains(&t) && nl.lines().count() == 1, "{nl}");
    }

    #[test]
    fn list_in_leaves_the_config_unchanged() {
        let t = T::new();
        let tk = token();
        let text = mp("a", &format!("git+https://bob:{tk}@h.example/o/r.git"), "v1.0.0", None, &[])
            + &mp("c", &format!("git+ssh://git:{tk}@h.example/o/r.git"), "v1.0.0", None, &[]);
        t.write_config(&text);
        let rows = with_env(&t, list_in).unwrap();
        assert_eq!(rows.len(), 2);
        for r in rows { assert!(!r.line().contains(&tk), "{}", r.line()); }
        assert_eq!(t.config(), text);
    }

    #[cfg(unix)]
    #[test]
    fn add_without_a_tag_names_the_source_without_userinfo() {
        let mut t = T::new();
        let tk = token();
        let repo = bare_repo(&t, "notag", &[]);
        t.git.git_program = Some(redirect_git(&t.fx, &repo.dir));
        let e = add_plain(&t, &format!("git+https://bob:{tk}@h.example/o/r.git")).unwrap_err().to_string();
        assert_eq!(e, "no semver tag in git+https://h.example/o/r.git; pass --ref <tag|commit>");
    }

    #[cfg(unix)]
    #[test]
    fn add_failure_scrubs_git_stderr() {
        let mut t = T::new();
        let tk = token();
        t.git.git_program = Some(url_git(&t.fx));
        let e = add_plain(&t, &format!("git+https://bob:{tk}@h.example/o/r.git")).unwrap_err().to_string();
        assert_eq!(e, "git failed: fatal: unable to access 'https://h.example/o/r.git/': boom");
        assert!(!e.contains(&tk));
    }

    #[test]
    fn add_refuses_an_https_source_without_echoing_it() {
        let t = T::new();
        let tk = token();
        for src in [format!("https://{tk}@h.example/o/r"), format!("http://bob:{tk}@h/o")] {
            let e = add_plain(&t, &src).unwrap_err().to_string();
            assert_eq!(e, "source is not github:<owner>/<repo>, git+https://, git+ssh:// or an absolute path");
        }
    }

    #[cfg(unix)]
    #[test]
    fn update_failure_scrubs_stderr_and_keeps_the_config() {
        let mut t = T::new();
        let tk = token();
        let text = mp("a", &format!("git+https://bob:{tk}@h.example/o/r.git"), "v1.0.0", Some(&"a".repeat(40)), &[]);
        t.write_config(&text);
        t.git.git_program = Some(url_git(&t.fx));
        let report = with_env(&t, |env| update_in(env, Some("a"), false, true)).unwrap();
        assert_eq!(report.failures, [("a".to_string(), "git failed: fatal: unable to access 'https://h.example/o/r.git/': boom".to_string())]);
        assert_eq!(t.config(), text);
        assert!(t.config().contains(&tk));
    }
}
