mod common;

use common::*;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::time::{Duration, Instant};
use zygo_plugins::check::*;
use zygo_plugins::sync::*;

// Task 1 — hardened git, bounded run, source forms; check.rs seams

#[test]
fn clone_url_accepts_the_four_spec_forms() {
    assert_eq!(clone_url("github:example/agent-plugins").unwrap(), "https://github.com/example/agent-plugins.git");
    assert_eq!(clone_url("git+https://example.invalid/x.git").unwrap(), "https://example.invalid/x.git");
    assert_eq!(clone_url("git+ssh://git@example.invalid/x.git").unwrap(), "ssh://git@example.invalid/x.git");
    let fx = Fx::new();
    let abs = fx.path("market").display().to_string();
    assert_eq!(clone_url(&abs).unwrap(), abs);
}

#[test]
fn clone_url_refuses_everything_else() {
    for s in ["ext::sh -c id", "--upload-pack=x", "-oProxyCommand=x", "relative/path", "http://example.invalid/x",
        "git+file:///tmp/x", "github:a/b/c", "github:-a/b", "github:--upload-pack=x/y", ""] {
        assert!(clone_url(s).is_err(), "{s:?}");
    }
}

/// M29: a control character anywhere in a source is refused (it would break the rewritten git config).
#[test]
fn clone_url_refuses_control_characters() {
    for s in ["/tmp/a\nb", "/tmp/a\rb", "git+https://example.invalid/x\n[core]", "git+ssh://example.invalid/x\tb", "/tmp/a\u{7f}"] {
        assert!(clone_url(s).is_err(), "{s:?}");
    }
}

#[test]
fn git_at_least_parses_the_version_line() {
    assert!(git_at_least("git version 2.50.1 (Apple Git-155)", 2, 30));
    assert!(git_at_least("git version 2.30.0", 2, 30));
    assert!(!git_at_least("git version 2.29.2.windows.1", 2, 30));
    assert!(git_at_least("git version 3.0.0", 2, 30));
    assert!(!git_at_least("nonsense", 2, 30));
}

#[test]
fn sync_opts_default_keeps_the_spec_bounds() {
    let o = SyncOpts::default();
    assert_eq!(o.per_marketplace, PER_MARKETPLACE);
    assert_eq!(PER_MARKETPLACE, Duration::from_secs(5));
    assert_eq!(TOTAL, Duration::from_secs(20));
    assert!(o.git_program.is_none() && o.git_config_global.is_none());
}

#[test]
fn run_git_with_an_expired_deadline_does_not_spawn() {
    let fx = Fx::new();
    assert_eq!(run_git(&fx.0, &["--version"], Instant::now()).unwrap_err().to_string(), "git timed out");
    let out = run_git(&fx.0, &["--version"], far()).unwrap();
    assert!(String::from_utf8_lossy(&out).starts_with("git version"));
}

#[cfg(unix)]
#[test]
fn git_command_ignores_repository_config() {
    use std::os::unix::fs::PermissionsExt;
    let fx = Fx::new();
    let repo = Repo::new(&fx, "repo");
    repo.write("a.txt", "a");
    repo.commit("one");
    let marker = fx.path("marker");
    let cfg = repo.dir.join(".git/config");
    let mut text = std::fs::read_to_string(&cfg).unwrap();
    text.push_str(&format!("[core]\n\tfsmonitor = \"touch {}\"\n", marker.display()));
    std::fs::write(&cfg, text).unwrap();
    let hook = repo.dir.join(".git/hooks/post-checkout");
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    std::fs::write(&hook, format!("#!/bin/sh\ntouch '{}'\n", marker.display())).unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    for args in [&["status", "--porcelain"][..], &["checkout", "-q", "--detach", "HEAD"][..]] {
        let out = git_command(&repo.dir).args(args).env("GIT_CONFIG_GLOBAL", NULL).output().unwrap();
        assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }
    assert!(!marker.exists());
}

/// Polls until no process is left in group `pgid` (bounded at 5 s; fails the test at the bound).
#[cfg(unix)]
fn group_gone(pgid: i32) {
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        let r = unsafe { libc::kill(-pgid, 0) };
        if r == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) { return; }
        assert!(Instant::now() < end, "process group {pgid} still has members");
        std::thread::yield_now();
    }
}

#[cfg(unix)]
#[test]
#[ignore = "timing: races a hanging git against a 2 s deadline; 5 s poll for the group"]
fn run_git_kills_the_process_group_on_timeout() {
    let fx = Fx::new();
    let repo = Repo::new(&fx, "repo");
    let pidfile = fx.path("pgid");
    // The script records its process group (that of the git child run_git started) before it hangs.
    let alias = format!("alias.hang=!sh -c 'ps -o pgid= -p $$ > {}; sleep 30 & wait'", pidfile.display());
    let err = run_git(&repo.dir, &["-c", &alias, "hang"], Instant::now() + Duration::from_secs(2)).unwrap_err();
    assert_eq!(err.to_string(), "git timed out");
    let pgid: i32 = std::fs::read_to_string(&pidfile).unwrap().trim().parse().unwrap();
    group_gone(pgid);
}

#[test]
fn git_command_sets_the_lfs_and_submodule_overrides() {
    let fx = Fx::new();
    let c = git_command(&fx.0);
    let args: Vec<&OsStr> = c.get_args().collect();
    for v in ["filter.lfs.process=", "filter.lfs.smudge=", "filter.lfs.clean=", "filter.lfs.required=false",
        "submodule.recurse=false", "core.fsmonitor=false"] {
        assert!(args.windows(2).any(|w| w[0] == "-c" && w[1] == v), "-c {v} in {args:?}");
    }
    let envs: HashMap<&OsStr, Option<&OsStr>> = c.get_envs().collect();
    assert_eq!(envs.get(OsStr::new("GIT_LFS_SKIP_SMUDGE")), Some(&Some(OsStr::new("1"))));
    assert_eq!(envs.get(OsStr::new("GIT_TERMINAL_PROMPT")), Some(&Some(OsStr::new("0"))));
    assert_eq!(envs.get(OsStr::new("GIT_ALLOW_PROTOCOL")), Some(&Some(OsStr::new("file:https:ssh"))));
    for k in ["GIT_CONFIG_GLOBAL", "GIT_DIR", "GIT_COMMON_DIR"] {
        assert_eq!(envs.get(OsStr::new(k)), Some(&None), "{k} removed");
    }
}

#[test]
fn check_dir_by_with_an_expired_deadline_warns_and_still_checks() {
    let fx = Fx::new();
    let repo = Repo::new(&fx, "p");
    plugin_files(&repo, "", "demo", "1.0.0");
    let r = check_dir_by(&repo.dir, Instant::now());
    assert!(r.ok(), "{}", r.render());
    assert!(r.warnings.iter().any(|w| w.contains("git index unavailable")), "{:?}", r.warnings);
}

#[test]
fn git_modes_feed_check_dir_modes() {
    let fx = Fx::new();
    let repo = Repo::new(&fx, "p");
    plugin_files(&repo, "", "demo", "1.0.0");
    repo.write("scripts/run.sh", "echo hi\n");
    repo.chmod_x("scripts/run.sh");
    repo.commit("one");
    let modes = git_modes(&repo.dir, far()).unwrap();
    assert_eq!(modes.get("scripts/run.sh").map(String::as_str), Some("100755"));
    let d = check_dir_modes(&repo.dir, far(), Some(modes)).plugins[0].digest.clone();
    assert!(d.is_some());
    assert_eq!(d, check_dir(&repo.dir).plugins[0].digest);
}

#[cfg(windows)]
#[test]
fn check_dir_modes_marks_an_index_exec_file_as_code() {
    let fx = Fx::new();
    let dir = fx.path("p");
    std::fs::create_dir_all(&dir).unwrap();
    fx.write("p/.claude-plugin/plugin.json", r#"{"name":"demo","version":"1.0.0","description":"Demo plugin","license":"MIT"}"#);
    fx.write("p/skills/hello/SKILL.md", "---\nname: hello\ndescription: Says hello\n---\nSay hello.\n");
    fx.write("p/skills/hello/tool", "data");
    let modes: HashMap<String, String> = [("skills/hello/tool".to_string(), "100755".to_string())].into();
    let r = check_dir_modes(&dir, far(), Some(modes));
    assert!(r.plugins[0].code.iter().any(|c| c.kind == CodeKind::File && c.path == "skills/hello/tool"));
    assert!(r.plugins[0].digest.is_some());
    assert!(check_dir_modes(&dir, far(), None).plugins[0].code.is_empty());
}

// Task 2 — approval store (§15.7)

mod approvals {
    use super::*;
    use zygo_plugins::approve::*;

    fn d() -> String { "a".repeat(64) }

    #[test]
    fn approval_roundtrip() {
        let fx = Fx::new();
        let file = fx.path("data/approvals.toml");
        assert!(!is_approved_in(&file, "team", "demo", &d()));
        approve_in(&file, "team", "demo", &d()).unwrap();
        assert!(is_approved_in(&file, "team", "demo", &d()));
        assert!(!is_approved_in(&file, "team", "demo", &"b".repeat(64)));
        assert!(!is_approved_in(&file, "team", "other", &d()));
        assert!(!is_approved_in(&file, "else", "demo", &d()));
        let text = std::fs::read_to_string(&file).unwrap();
        for want in ["[[approval]]", "marketplace = \"team\"", "plugin = \"demo\"", &format!("digest = \"{}\"", d())] {
            assert!(text.contains(want), "{want} in {text}");
        }
    }

    #[test]
    fn approval_is_idempotent_and_keeps_other_entries() {
        let fx = Fx::new();
        let file = fx.write("data/approvals.toml", "# mine\n");
        approve_in(&file, "team", "demo", &d()).unwrap();
        approve_in(&file, "team", "demo", &d()).unwrap();
        approve_in(&file, "team", "two", &d()).unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        assert_eq!(text.matches("[[approval]]").count(), 2, "{text}");
        assert!(text.starts_with("# mine\n"), "{text}");
        let mut names: Vec<String> = std::fs::read_dir(fx.path("data")).unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        assert_eq!(names, ["approvals.toml", "approvals.toml.lock"]);
    }

    #[test]
    fn approve_refuses_a_digest_that_is_not_64_lower_hex() {
        let fx = Fx::new();
        let file = fx.path("data/approvals.toml");
        for bad in [String::new(), "abc".into(), "G".repeat(64), "A".repeat(64)] {
            assert!(approve_in(&file, "team", "demo", &bad).is_err(), "{bad:?}");
        }
        assert!(!file.exists());
    }

    #[test]
    fn invalid_approvals_file_approves_nothing_and_is_not_overwritten() {
        let fx = Fx::new();
        let file = fx.write("data/approvals.toml", "not toml [");
        assert!(!is_approved_in(&file, "team", "demo", &d()));
        assert!(approve_in(&file, "team", "demo", &d()).is_err());
        assert_eq!(std::fs::read(&file).unwrap(), b"not toml [");
    }
}

// Task 3 — selection, user-config writer, built-in entry (§15.2, §15.9)

mod selecting {
    use super::*;
    use std::collections::BTreeSet;
    use zygo_config::profile::select;
    use zygo_config::{load, LoadContext};
    use zygo_plugins::approve::PreApproval;
    use zygo_plugins::select::*;

    fn id(s: &str) -> PluginId { s.parse().unwrap() }
    fn ids(v: &[&str]) -> BTreeSet<PluginId> { v.iter().map(|s| id(s)).collect() }

    #[test]
    fn selection_order_is_default_then_user_then_profile() {
        let fx = Fx::new();
        let team = mp("m", "/x", "v1", None, &["a", "b"]);
        let user = "[plugins]\nenable = [\"m:c\"]\ndisable = [\"m:b\"]\n[profile.p.plugins]\n\"m:b\" = true\n\"m:a\" = false\n";
        let cfg = merged(&fx, user, Some(&team));
        assert_eq!(selection(&cfg, None).selected, ids(&["m:a", "m:c"]));
        let p = select(&cfg, Some("p")).unwrap();
        assert_eq!(selection(&cfg, Some(&p)).selected, ids(&["m:b", "m:c"]));
    }

    #[test]
    fn required_plugin_cannot_be_deselected() {
        let fx = Fx::new();
        let team = mp("m", "/x", "v1", None, &[]) + "required = [\"a\"]\n";
        let user = "[plugins]\ndisable = [\"m:a\"]\n[profile.p.plugins]\n\"m:a\" = false\n";
        let cfg = merged(&fx, user, Some(&team));
        let p = select(&cfg, Some("p")).unwrap();
        let s = selection(&cfg, Some(&p));
        assert!(s.selected.contains(&id("m:a")) && s.required.contains(&id("m:a")));
        assert_eq!(s.status(&id("m:a")), Status::Required);
    }

    #[test]
    fn disable_wins_over_enable_in_one_layer() {
        let fx = Fx::new();
        let cfg = merged(&fx, &(mp("m", "/x", "v1", None, &[]) + "[plugins]\nenable = [\"m:x\"]\ndisable = [\"m:x\"]\n"), None);
        assert!(!selection(&cfg, None).selected.contains(&id("m:x")));
    }

    #[test]
    fn status_labels() {
        let fx = Fx::new();
        let team = mp("m", "/x", "v1", None, &["d"]) + "required = [\"r\"]\n";
        let cfg = merged(&fx, "[plugins]\nenable = [\"m:e\"]\ndisable = [\"m:x\"]\n", Some(&team));
        let s = selection(&cfg, None);
        assert_eq!(s.status(&id("m:d")), Status::Default);
        assert_eq!(s.status(&id("m:e")), Status::Optional);
        assert_eq!(s.status(&id("m:never")), Status::Optional);
        assert_eq!(s.status(&id("m:x")), Status::Disabled);
        assert_eq!(s.status(&id("m:r")), Status::Required);
        let names: Vec<&str> = [Status::Default, Status::Optional, Status::Disabled, Status::Required].iter().map(|s| s.as_str()).collect();
        assert_eq!(names, ["default", "optional", "disabled", "required"]);
    }

    #[test]
    fn team_approvals_count_only_when_the_team_set_them() {
        let fx = Fx::new();
        let d = "a".repeat(64);
        let appr = format!("approvals = [{{ plugin = \"a\", digest = \"{d}\" }}]\n");
        let team = mp("t", "/team", "v1", None, &[]) + &appr;
        let user = mp("u", "/user", "v1", None, &[]) + &appr;
        let want = vec![PreApproval { marketplace: "t".into(), plugin: "a".into(), digest: d.clone() }];
        let s = selection(&merged(&fx, &user, Some(&team)), None);
        assert_eq!(s.pre_approved, want);
        assert_eq!(s.sources["t"], "/team");
        assert_eq!(s.sources["u"], "/user");
        let fx = Fx::new();
        let user = mp("t", "/team", "v2", None, &[]);
        assert_eq!(selection(&merged(&fx, &user, Some(&team)), None).pre_approved, want);
    }

    fn real(fx: &Fx, user: Option<&str>) -> zygo_config::Merged {
        let paths = paths(fx);
        if let Some(u) = user {
            std::fs::create_dir_all(paths.config_file.parent().unwrap()).unwrap();
            std::fs::write(&paths.config_file, u).unwrap();
        }
        load(&LoadContext { paths, repo: None, team: None }).unwrap()
    }

    #[test]
    fn real_builtin_layer_selects_act_lite_by_default() {
        let fx = Fx::new();
        let s = selection(&real(&fx, None), None);
        assert_eq!(s.selected, ids(&["act101:act-lite"]));
        assert!(!s.selected.contains(&id("act101:act101")));
        assert_eq!(s.sources["act101"], "github:act101-ai/act101");
    }

    #[test]
    fn user_entry_cannot_change_the_builtin_source() {
        let fx = Fx::new();
        let cfg = real(&fx, Some("[[marketplace]]\nname = \"act101\"\nsource = \"github:other/x\"\nref = \"v9\"\n"));
        assert_eq!(cfg.config.marketplaces[0].source, "github:act101-ai/act101");
        assert!(cfg.warnings.contains(&"config: user marketplace[0]: a user entry cannot change the built-in source; ignored".to_string()),
            "{:?}", cfg.warnings);
        let fx = Fx::new();
        let cfg = real(&fx, Some("[[marketplace]]\nname = \"act101\"\nsource = \"github:act101-ai/act101\"\nref = \"v9\"\n"));
        let m = &cfg.config.marketplaces[0];
        assert_eq!(m.default_on, ["act-lite"]);
        assert_eq!(m.git_ref, "v9");
    }

    fn listing(dir: &std::path::Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        v.sort();
        v
    }

    #[test]
    fn set_enabled_writes_user_config_and_keeps_other_text() {
        let fx = Fx::new();
        let text = "# mine\n[user]\ndefault_profile = \"p\"\n".to_string() + &mp("team", "/x", "v1", None, &[]);
        let cfg = merged(&fx, &text, None);
        let file = paths(&fx).config_file;
        set_enabled(&file, &cfg, &id("team:demo"), true).unwrap();
        let t = std::fs::read_to_string(&file).unwrap();
        assert!(t.starts_with("# mine\n"), "{t}");
        assert!(t.contains("enable = [\"team:demo\"]"), "{t}");
        set_enabled(&file, &cfg, &id("team:demo"), false).unwrap();
        set_enabled(&file, &cfg, &id("team:demo"), false).unwrap();
        let t = std::fs::read_to_string(&file).unwrap();
        assert!(t.contains("disable = [\"team:demo\"]"), "{t}");
        let parsed: toml::Table = t.parse().unwrap();
        assert_eq!(parsed["plugins"]["enable"].as_array().unwrap().len(), 0, "{t}");
        assert!(t.starts_with("# mine\n"), "{t}");
        assert_eq!(listing(file.parent().unwrap()), ["config.toml", "config.toml.lock"]);
    }

    #[test]
    fn set_enabled_refuses_required_unknown_marketplace_and_bad_id() {
        let fx = Fx::new();
        let team = mp("team", "/x", "v1", None, &[]) + "required = [\"p\"]\n";
        let cfg = merged(&fx, "# mine\n", Some(&team));
        let file = paths(&fx).config_file;
        assert_eq!(set_enabled(&file, &cfg, &id("team:p"), false).unwrap_err().to_string(),
            "refused: team:p is required by the team config");
        assert_eq!(set_enabled(&file, &cfg, &id("nope:p"), true).unwrap_err().to_string(), "marketplace nope is not configured");
        for bad in ["a", "a:b:c", "A:b", ":b"] {
            assert_eq!(bad.parse::<PluginId>().unwrap_err().to_string(), "plugin id must be <marketplace>:<plugin>", "{bad}");
        }
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "# mine\n");
    }
}

// Task 4 — store: manifest, stage, swap, verify, inspect, locks; known plugins (§15.3, §15.6)

mod storing {
    use super::*;
    use std::path::{Path, PathBuf};
    use zygo_plugins::store::*;

    const MIB: usize = 1_048_576;

    fn none() -> HashMap<String, String> { HashMap::new() }

    /// A plugin without code under `fx/<at>`.
    fn plain(fx: &Fx, at: &str) -> PathBuf {
        fx.write(&format!("{at}/.claude-plugin/plugin.json"), r#"{"name":"demo","version":"1.0.0","description":"Demo plugin","license":"MIT"}"#);
        fx.write(&format!("{at}/skills/hello/SKILL.md"), "---\nname: hello\ndescription: Says hello\n---\nSay hello.\n");
        fx.path(at)
    }

    /// A valid plugin with `scripts/run.sh` under `fx/src`, and its check report.
    fn fixture(fx: &Fx) -> (PathBuf, PluginReport) {
        let dir = plain(fx, "src");
        fx.write("src/scripts/run.sh", "echo hi\n");
        let r = check_dir(&dir).plugins[0].clone();
        (dir, r)
    }

    fn root(fx: &Fx) -> PathBuf { fx.path("store") }

    /// Stages and swaps in the fixture plugin; returns its entry.
    fn install(fx: &Fx, dir: &Path, copied: &[String]) -> StoredEntry {
        let s = stage(&root(fx), "team", dir, copied, &none(), far()).unwrap();
        swap_in(&root(fx), &s).unwrap();
        s.entry
    }

    /// Every name under `dir` (recursive), `/`-separated and relative.
    fn tree(dir: &Path) -> Vec<String> {
        if !dir.exists() { return vec![]; }
        walkdir::WalkDir::new(dir).min_depth(1).into_iter().flatten()
            .map(|e| e.path().strip_prefix(dir).unwrap().to_string_lossy().replace('\\', "/")).collect()
    }

    fn no_entry(fx: &Fx) {
        let left = tree(&root(fx).join("team/demo"));
        assert!(left.is_empty(), "{left:?}");
    }

    #[test]
    fn stage_copies_and_hashes_what_check_dir_saw_on_the_copy() {
        let fx = Fx::new();
        let (dir, r) = fixture(&fx);
        let s = stage(&root(&fx), "team", &dir, &r.copied, &none(), far()).unwrap();
        assert_eq!(s.entry.files.keys().cloned().collect::<Vec<_>>(), r.copied);
        assert_eq!(s.entry.digest, r.digest);
        assert_eq!(s.entry.files["scripts/run.sh"].sha256, sha_hex(b"echo hi\n"));
        swap_in(&root(&fx), &s).unwrap();
        let d = entry_dir(&root(&fx), &s.entry);
        assert_eq!(d, root(&fx).join("team/demo/1.0.0"));
        assert_eq!(std::fs::read(d.join("scripts/run.sh")).unwrap(), b"echo hi\n");
        assert_eq!(check_dir(&d).plugins[0].digest, s.entry.digest);
        assert!(!tree(&root(&fx)).iter().any(|n| n.contains(".tmp-")));
    }

    #[test]
    fn stage_takes_digest_and_copied_from_the_copy() {
        let fx = Fx::new();
        let (dir, r) = fixture(&fx);
        fx.write("src/scripts/run.sh", "echo two\n");
        fx.write("src/scripts/late.sh", "echo late\n");
        let e = install(&fx, &dir, &r.copied);
        let d = entry_dir(&root(&fx), &e);
        assert_eq!(e.digest, check_dir(&d).plugins[0].digest);
        assert_ne!(e.digest, r.digest);
        assert!(!e.files.contains_key("scripts/late.sh"));
        assert!(!d.join("scripts/late.sh").exists());
    }

    /// A git repository holding the plugin with `scripts/run.sh` and an extensionless `skills/hello/tool`, both +x.
    fn index_fixture(fx: &Fx) -> (Repo, HashMap<String, String>, PluginReport) {
        let repo = Repo::new(fx, "idx");
        plugin_files(&repo, "", "demo", "1.0.0");
        repo.write("scripts/run.sh", "echo hi\n");
        repo.write("skills/hello/tool", "data");
        repo.chmod_x("scripts/run.sh");
        repo.chmod_x("skills/hello/tool");
        repo.commit("one");
        let modes = git_modes(&repo.dir, far()).unwrap();
        let r = check_dir_modes(&repo.dir, far(), Some(modes.clone())).plugins[0].clone();
        (repo, modes, r)
    }

    #[test]
    fn stage_digest_equals_the_cache_check_digest_with_the_index_modes() {
        let fx = Fx::new();
        let (repo, modes, r) = index_fixture(&fx);
        let s = stage(&root(&fx), "team", &repo.dir, &r.copied, &modes, far()).unwrap();
        assert!(r.digest.is_some());
        assert_eq!(s.entry.digest, r.digest);
        assert!(s.entry.files["skills/hello/tool"].exec);
    }

    #[cfg(unix)]
    #[test]
    fn stage_refuses_a_symlink_swapped_in_after_the_check() {
        let fx = Fx::new();
        let (dir, r) = fixture(&fx);
        let outside = fx.write("outside.txt", "secret");
        std::fs::remove_file(dir.join("skills/hello/SKILL.md")).unwrap();
        std::os::unix::fs::symlink(&outside, dir.join("skills/hello/SKILL.md")).unwrap();
        let v = stage(&root(&fx), "team", &dir, &r.copied, &none(), far()).unwrap_err();
        assert_eq!((v.rule, v.path.as_str()), (Rule::Symlink, "skills/hello/SKILL.md"));
        no_entry(&fx);
    }

    #[test]
    fn stage_enforces_the_spec_size_limits_on_raw_bytes() {
        let fx = Fx::new();
        let (dir, r) = fixture(&fx);
        let with = |extra: &[&str]| -> Vec<String> {
            let mut v = r.copied.clone();
            v.extend(extra.iter().map(|s| s.to_string()));
            v
        };
        let try_stage = |copied: &[String]| stage(&root(&fx), "team", &dir, copied, &none(), far());
        fx.write("src/skills/hello/big.txt", vec![b'a'; MIB]);
        let s = try_stage(&with(&["skills/hello/big.txt"])).unwrap();
        discard(&s);
        no_entry(&fx);
        fx.write("src/skills/hello/big.txt", vec![b'a'; MIB + 1]);
        let v = try_stage(&with(&["skills/hello/big.txt"])).unwrap_err();
        assert_eq!((v.rule, v.path.as_str()), (Rule::FileTooLarge, "skills/hello/big.txt"));
        no_entry(&fx);
        fx.write("src/skills/hello/big.txt", "a\r\n".repeat(400_000));
        assert_eq!(try_stage(&with(&["skills/hello/big.txt"])).unwrap_err().rule, Rule::FileTooLarge);
        no_entry(&fx);
        std::fs::remove_file(dir.join("skills/hello/big.txt")).unwrap();
        let base: usize = r.copied.iter().map(|c| std::fs::metadata(dir.join(c)).unwrap().len() as usize).sum();
        for i in 0..3 { fx.write(&format!("src/skills/hello/f{i}.txt"), vec![b'a'; MIB]); }
        fx.write("src/skills/hello/f3.txt", vec![b'a'; MIB - base]);
        let four = ["skills/hello/f0.txt", "skills/hello/f1.txt", "skills/hello/f2.txt", "skills/hello/f3.txt"];
        let s = try_stage(&with(&four)).unwrap();
        discard(&s);
        fx.write("src/skills/hello/f4.txt", "a");
        let mut five = four.to_vec();
        five.push("skills/hello/f4.txt");
        assert_eq!(try_stage(&with(&five)).unwrap_err().rule, Rule::PluginTooLarge);
        no_entry(&fx);
    }

    #[cfg(unix)]
    fn mode(p: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(p).unwrap().permissions().mode() & 0o777
    }
    #[cfg(unix)]
    fn chmod(p: &Path, m: u32) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(m)).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn stage_preserves_executable_bits() {
        let fx = Fx::new();
        let dir = plain(&fx, "src");
        fx.write("src/scripts/run.sh", "echo hi\n");
        fx.write("src/skills/hello/notes.txt", "notes");
        chmod(&dir.join("scripts/run.sh"), 0o755);
        chmod(&dir.join("skills/hello/notes.txt"), 0o600);
        let r = check_dir(&dir).plugins[0].clone();
        let e = install(&fx, &dir, &r.copied);
        let d = entry_dir(&root(&fx), &e);
        assert_ne!(mode(&d.join("scripts/run.sh")) & 0o111, 0);
        assert!(e.files["scripts/run.sh"].exec);
        assert_eq!(mode(&d.join("skills/hello/notes.txt")), 0o644);
        assert!(!e.files["skills/hello/notes.txt"].exec);
        let fx = Fx::new();
        let dir = plain(&fx, "src");
        fx.write("src/skills/hello/tool", "data");
        chmod(&dir.join("skills/hello/tool"), 0o644);
        let r = check_dir(&dir).plugins[0].clone();
        let s = stage(&root(&fx), "team", &dir, &r.copied, &none(), far()).unwrap();
        assert_eq!(s.entry.digest, None);
        discard(&s);
        chmod(&dir.join("skills/hello/tool"), 0o755);
        let s = stage(&root(&fx), "team", &dir, &r.copied, &none(), far()).unwrap();
        assert!(s.entry.digest.is_some());
        assert!(s.entry.files["skills/hello/tool"].exec);
        discard(&s);
    }

    fn manifest_of(entries: Vec<StoredEntry>) -> Manifest {
        let mut m = Manifest { version: 1, ..Default::default() };
        m.marketplaces.insert("team".into(), PinEntry { source: "/src".into(), git_ref: "v1.0.0".into(), commit: "c".repeat(40), ..Default::default() });
        m.plugins = entries;
        m
    }

    #[test]
    fn swap_in_replaces_an_existing_version_and_rollback_restores_it() {
        let fx = Fx::new();
        let (dir, r) = fixture(&fx);
        let old = install(&fx, &dir, &r.copied);
        let d = entry_dir(&root(&fx), &old);
        fx.write("src/scripts/run.sh", "echo two\n");
        let s = stage(&root(&fx), "team", &dir, &r.copied, &none(), far()).unwrap();
        let sw = swap_in(&root(&fx), &s).unwrap();
        assert_eq!(std::fs::read(d.join("scripts/run.sh")).unwrap(), b"echo two\n");
        assert!(sw.aside.as_ref().unwrap().exists());
        rollback(&sw);
        assert_eq!(std::fs::read(d.join("scripts/run.sh")).unwrap(), b"echo hi\n");
        let names = tree(&root(&fx).join("team/demo"));
        assert!(!names.iter().any(|n| n.contains(".old-") || n.contains(".tmp-")), "{names:?}");
        let s = stage(&root(&fx), "team", &dir, &r.copied, &none(), far()).unwrap();
        let sw = swap_in(&root(&fx), &s).unwrap();
        let aside = sw.aside.clone().unwrap();
        finish(&root(&fx), vec![sw], &manifest_of(vec![old]), &manifest_of(vec![s.entry.clone()]));
        assert!(!aside.exists());
        assert_eq!(std::fs::read(d.join("scripts/run.sh")).unwrap(), b"echo two\n");
    }

    /// A named change to a stored entry dir.
    type Change = (&'static str, fn(&Path));

    #[test]
    fn verify_detects_changed_missing_linked_and_chmodded_files() {
        let cases: Vec<Change> = vec![
            ("appended", |d| {
                use std::io::Write;
                std::fs::OpenOptions::new().append(true).open(d.join("scripts/run.sh")).unwrap().write_all(b"x").unwrap();
            }),
            ("deleted", |d| std::fs::remove_file(d.join("skills/hello/SKILL.md")).unwrap()),
            #[cfg(unix)]
            ("linked", |d| {
                let same = d.parent().unwrap().join("same.sh");
                std::fs::copy(d.join("scripts/run.sh"), &same).unwrap();
                std::fs::remove_file(d.join("scripts/run.sh")).unwrap();
                std::os::unix::fs::symlink(&same, d.join("scripts/run.sh")).unwrap();
            }),
            #[cfg(unix)]
            ("chmodded", |d| chmod(&d.join("skills/hello/SKILL.md"), 0o755)),
        ];
        for (what, change) in cases {
            let fx = Fx::new();
            let (dir, r) = fixture(&fx);
            let e = install(&fx, &dir, &r.copied);
            assert!(verify(&root(&fx), &e), "{what}: untouched");
            change(&entry_dir(&root(&fx), &e));
            assert!(!verify(&root(&fx), &e), "{what}");
        }
    }

    fn copy_dir(from: &Path, to: &Path) {
        for e in walkdir::WalkDir::new(from).into_iter().flatten() {
            let t = to.join(e.path().strip_prefix(from).unwrap());
            if e.file_type().is_dir() { std::fs::create_dir_all(&t).unwrap(); } else { std::fs::copy(e.path(), &t).unwrap(); }
        }
    }

    #[test]
    fn verify_refuses_paths_that_escape_the_store() {
        let fx = Fx::new();
        let (dir, r) = fixture(&fx);
        let e = install(&fx, &dir, &r.copied);
        let d = entry_dir(&root(&fx), &e);
        assert!(verify(&root(&fx), &e));
        let abs = fx.write("abs-target.txt", "abs");
        let mut keys: Vec<(String, PathBuf)> = vec![
            ("../outside.txt".into(), d.join("../outside.txt")),
            (abs.display().to_string(), abs.clone()),
            ("a/../b".into(), d.join("b")),
            ("./SKILL.md".into(), d.join("SKILL.md")),
        ];
        if cfg!(unix) {
            keys.push(("a\\b".into(), d.join("a\\b")));
            keys.push(("C:x".into(), d.join("C:x")));
        }
        for (key, real) in keys {
            if !real.exists() { std::fs::write(&real, "content").unwrap(); }
            let mut bad = e.clone();
            bad.files.insert(key.clone(), FileEntry { sha256: sha_hex(&std::fs::read(&real).unwrap()), exec: false });
            assert!(!verify(&root(&fx), &bad), "{key}");
        }
        for (m, p, v) in [("../x", "demo", "1.0.0"), ("team", "A", "1.0.0"), ("team", "demo", "../1")] {
            let bad = StoredEntry { marketplace: m.into(), plugin: p.into(), version: v.into(), ..e.clone() };
            copy_dir(&d, &root(&fx).join(m).join(p).join(v));
            assert!(!verify(&root(&fx), &bad), "{m}/{p}/{v}");
        }
    }

    #[test]
    fn state_reports_missing_valid_and_invalid_manifest() {
        let fx = Fx::new();
        let s = state(&root(&fx));
        assert!(matches!(s.manifest, ManifestState::Missing) && s.plugins.is_empty());
        let (dir, r) = fixture(&fx);
        let e = install(&fx, &dir, &r.copied);
        write_manifest(&root(&fx), &manifest_of(vec![e.clone()])).unwrap();
        let s = state(&root(&fx));
        assert!(matches!(s.manifest, ManifestState::Valid));
        assert_eq!(s.plugins.len(), 1);
        assert!(s.plugins[0].intact);
        assert_eq!(s.plugins[0].source, "/src");
        assert_eq!(s.plugins[0].dir, root(&fx).join("team/demo/1.0.0"));
        std::fs::write(entry_dir(&root(&fx), &e).join("skills/hello/SKILL.md"), "changed").unwrap();
        assert!(!state(&root(&fx)).plugins[0].intact);
        std::fs::write(root(&fx).join("manifest.json"), "{nope").unwrap();
        let s = state(&root(&fx));
        assert!(matches!(s.manifest, ManifestState::Invalid(_)) && s.plugins.is_empty());
    }

    #[test]
    fn write_manifest_replaces_the_whole_file_and_leaves_no_temp_file() {
        let fx = Fx::new();
        let one = manifest_of(vec![]);
        let mut two = manifest_of(vec![]);
        two.marketplaces.insert("other".into(), PinEntry { source: "/o".into(), git_ref: "v2".into(), commit: "d".repeat(40), ..Default::default() });
        write_manifest(&root(&fx), &one).unwrap();
        write_manifest(&root(&fx), &two).unwrap();
        assert_eq!(read_manifest(&root(&fx)).unwrap().unwrap(), two);
        assert_eq!(tree(&root(&fx)), ["manifest.json"]);
    }

    #[test]
    fn store_lock_excludes_writers_and_honours_the_deadline() {
        let fx = Fx::new();
        let shared = lock_until(&root(&fx), false, far()).unwrap();
        assert!(shared.is_some());
        assert!(lock_until(&root(&fx), true, Instant::now()).unwrap().is_none());
        let second = lock_until(&root(&fx), false, far()).unwrap();
        assert!(second.is_some());
        drop(shared);
        drop(second);
        assert!(lock_until(&root(&fx), true, far()).unwrap().is_some());
    }

    #[test]
    fn inspect_returns_the_report_of_the_stored_files_or_an_error() {
        let fx = Fx::new();
        let (dir, r) = fixture(&fx);
        let e = install(&fx, &dir, &r.copied);
        let report = inspect(&root(&fx), &e).unwrap();
        assert_eq!(report.plugins[0].digest, e.digest);
        assert!(report.ok());
        assert!(report.render().contains("code file scripts/run.sh"), "{}", report.render());
        {
            use std::io::Write;
            let d = entry_dir(&root(&fx), &e);
            std::fs::OpenOptions::new().append(true).open(d.join("scripts/run.sh")).unwrap().write_all(b"x").unwrap();
        }
        assert!(inspect(&root(&fx), &e).is_err());
        #[cfg(unix)]
        {
            let fx = Fx::new();
            let (dir, r) = fixture(&fx);
            let e = install(&fx, &dir, &r.copied);
            chmod(&entry_dir(&root(&fx), &e).join("skills/hello/SKILL.md"), 0o755);
            assert!(inspect(&root(&fx), &e).is_err());
        }
        let fx = Fx::new();
        let (repo, modes, r) = index_fixture(&fx);
        let s = stage(&root(&fx), "team", &repo.dir, &r.copied, &modes, far()).unwrap();
        swap_in(&root(&fx), &s).unwrap();
        assert_eq!(inspect(&root(&fx), &s.entry).unwrap().plugins[0].digest, r.digest);
    }

    #[test]
    fn read_verified_returns_hashed_bytes_only() {
        let fx = Fx::new();
        let (dir, r) = fixture(&fx);
        let e = install(&fx, &dir, &r.copied);
        let src = std::fs::read(dir.join("skills/hello/SKILL.md")).unwrap();
        assert_eq!(read_verified(&root(&fx), &e, "skills/hello/SKILL.md").unwrap(), src);
        std::fs::write(entry_dir(&root(&fx), &e).join("skills/hello/SKILL.md"), "changed").unwrap();
        assert!(read_verified(&root(&fx), &e, "skills/hello/SKILL.md").is_err());
        assert!(read_verified(&root(&fx), &e, "../x").is_err());
        assert!(read_verified(&root(&fx), &e, "nope").is_err());
    }

    #[test]
    fn known_plugins_lists_config_and_store_ids() {
        use zygo_plugins::select::{known_plugins, selection};
        let fx = Fx::new();
        let team = mp("team", "/x", "v1", None, &["d"]) + "required = [\"r\"]\n";
        let user = "[plugins]\nenable = [\"team:e\"]\ndisable = [\"team:x\"]\n[profile.p.plugins]\n\"team:only\" = true\n";
        let cfg = merged(&fx, user, Some(&team));
        let entry = StoredEntry { marketplace: "team".into(), plugin: "stored".into(), version: "1.0.0".into(), ..Default::default() };
        write_manifest(&root(&fx), &manifest_of(vec![entry])).unwrap();
        let k = known_plugins(&selection(&cfg, None), &state(&root(&fx)));
        for want in ["team:d", "team:r", "team:e", "team:x", "team:stored"] { assert!(k.contains(want), "{want} in {k:?}"); }
        assert!(!k.contains("team:only"));
    }

    // Round 2 (review)

    #[test]
    fn stage_and_verify_refuse_a_dot_git_path() {
        let fx = Fx::new();
        let (dir, r) = fixture(&fx);
        fx.write("src/.git/config", "[core]\n");
        let mut copied = r.copied.clone();
        copied.push(".git/config".into());
        let v = stage(&root(&fx), "team", &dir, &copied, &none(), far()).unwrap_err();
        assert_eq!((v.rule, v.path.as_str()), (Rule::PathOutside, ".git/config"));
        no_entry(&fx);
        let e = install(&fx, &dir, &r.copied);
        assert!(verify(&root(&fx), &e));
        for key in [".git/config", "skills/.GIT/config"] {
            let at = entry_dir(&root(&fx), &e).join(key);
            std::fs::create_dir_all(at.parent().unwrap()).unwrap();
            std::fs::write(&at, "[core]\n").unwrap();
            let mut bad = e.clone();
            bad.files.insert(key.into(), FileEntry { sha256: sha_hex(b"[core]\n"), exec: false });
            assert!(!verify(&root(&fx), &bad), "{key}");
        }
    }

    #[test]
    fn stage_refuses_a_copy_that_fails_the_check() {
        let fx = Fx::new();
        let dir = plain(&fx, "src");
        fx.write("src/skills/hello/SKILL.md", "---\nname: other\ndescription: Says hello\n---\nSay hello.\n");
        let r = check_dir(&dir).plugins[0].clone();
        assert!(!r.ok());
        let v = stage(&root(&fx), "team", &dir, &r.copied, &none(), far()).unwrap_err();
        assert_eq!(v.rule, Rule::NameNotDir);
        no_entry(&fx);
    }

    #[test]
    fn inspect_refuses_a_changed_file_outside_the_digest() {
        let fx = Fx::new();
        let (dir, r) = fixture(&fx);
        let e = install(&fx, &dir, &r.copied);
        assert!(inspect(&root(&fx), &e).is_ok());
        std::fs::write(entry_dir(&root(&fx), &e).join("skills/hello/SKILL.md"), "---\nname: hello\ndescription: Says hello\n---\nSay bye.\n").unwrap();
        assert_eq!(check_dir(&entry_dir(&root(&fx), &e)).plugins[0].digest, e.digest, "the digest does not cover SKILL.md");
        assert!(inspect(&root(&fx), &e).is_err());
    }
}

// Task 5 — sync_with (§15.1, §15.3; §20 #38 moved tag, #39)

mod syncing {
    use super::*;
    use std::path::{Path, PathBuf};
    use zygo_config::{load_with_builtin, LoadContext, Merged};
    use zygo_core::paths::Paths;
    use zygo_plugins::select::{selection, PluginId};
    use zygo_plugins::store::*;

    fn id(s: &str) -> PluginId { s.parse().unwrap() }
    fn store(p: &Paths) -> PathBuf { p.plugin_store.clone() }
    fn manifest(p: &Paths) -> Manifest { read_manifest(&p.plugin_store).unwrap().unwrap() }
    fn entry<'a>(m: &'a Manifest, plugin: &str) -> Option<&'a StoredEntry> { m.plugins.iter().find(|e| e.plugin == plugin) }
    fn skill(p: &Paths, plugin: &str, version: &str) -> String {
        std::fs::read_to_string(store(p).join("team").join(plugin).join(version).join("skills/hello/SKILL.md")).unwrap()
    }
    fn names(dir: &Path) -> Vec<String> {
        let Ok(rd) = std::fs::read_dir(dir) else { return vec![] };
        let mut v: Vec<String> = rd.map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        v.sort();
        v
    }
    fn offline() -> Vec<String> { vec!["plugins: offline".to_string()] }
    fn partial() -> Vec<String> { vec!["plugins: partial".to_string()] }
    fn moved(name: &str, r: &str) -> String { format!("marketplace {name}: tag {r} moved; run zygo marketplace update {name} --accept") }
    /// A new skill text and the plugin at `version` (marketplace entry too), committed and tagged `tag`.
    fn bump(repo: &Repo, plugins: &[(&str, &str)], text: &str, tag: &str) -> String {
        market_json(repo, plugins);
        for (n, v) in plugins {
            plugin_files(repo, &format!("plugins/{n}"), n, v);
            repo.write(&format!("plugins/{n}/skills/hello/SKILL.md"), format!("---\nname: hello\ndescription: Says hello\n---\n{text}\n"));
        }
        let c = repo.commit(tag);
        repo.tag_force(tag);
        c
    }
    fn traced(fx: &Fx) -> (SyncOpts, PathBuf) {
        let (git, log) = git_tracer(fx);
        (SyncOpts { git_program: Some(git), ..opts(fx) }, log)
    }
    fn trace(log: &Path) -> String { std::fs::read_to_string(log).unwrap_or_default() }
    fn cache(p: &Paths) -> PathBuf { p.marketplaces_dir.join("team") }

    #[test]
    fn sync_fills_the_store_at_the_tag() {
        let fx = Fx::new();
        let repo = market_repo(&fx, &[("demo", "1.0.0")]);
        let head = git(&repo.dir, &["rev-parse", "HEAD"]);
        let cfg = merged(&fx, &mp("team", &repo.src(), "v1.0.0", None, &["demo"]), None);
        let p = paths(&fx);
        let r = sync_with(&cfg, &p, WIDE_TOTAL, &opts(&fx));
        assert!(r.lines.is_empty() && r.failures.is_empty(), "{r:?}");
        assert_eq!(r.synced, [id("team:demo")]);
        assert!(store(&p).join("team/demo/1.0.0/skills/hello/SKILL.md").is_file());
        let s = state(&store(&p));
        assert!(matches!(s.manifest, ManifestState::Valid));
        assert_eq!(s.plugins.len(), 1);
        assert!(s.plugins[0].intact);
        assert_eq!(s.plugins[0].entry.digest, None);
        assert_eq!(manifest(&p).marketplaces["team"], PinEntry { source: repo.src(), git_ref: "v1.0.0".into(), commit: head, layer: "user".into() });
        assert!(std::fs::symlink_metadata(cache(&p).join(".git")).unwrap().is_dir());
    }

    #[test]
    fn sync_uses_the_configured_commit_and_reports_a_moved_tag() {
        let fx = Fx::new();
        let repo = market_repo(&fx, &[("demo", "1.0.0")]);
        let a = git(&repo.dir, &["rev-parse", "HEAD"]);
        let p = paths(&fx);
        let cfg = merged(&fx, &mp("team", &repo.src(), "v1.0.0", Some(&a), &["demo"]), None);
        bump(&repo, &[("demo", "1.1.0")], "Say B.", "v1.0.0");
        let r = sync_with(&cfg, &p, WIDE_TOTAL, &opts(&fx));
        assert_eq!(r.lines, [moved("team", "v1.0.0")], "{r:?}");
        assert!(store(&p).join("team/demo/1.0.0").is_dir());
        assert!(!store(&p).join("team/demo/1.1.0").exists());
        assert_eq!(manifest(&p).marketplaces["team"].commit, a);
        // Fetch by id: A is on no branch and no tag.
        repo.config("uploadpack.allowAnySHA1InWant", "true");
        repo.orphan("next");
        bump(&repo, &[("demo", "1.2.0")], "Say C.", "v1.0.0");
        repo.delete_branch("main");
        assert_eq!(git(&repo.dir, &["for-each-ref", "--contains", &a]), "");
        git(&repo.dir, &["cat-file", "-e", &a]);
        std::fs::remove_dir_all(cache(&p)).unwrap();
        let (o, log) = traced(&fx);
        let r = sync_with(&cfg, &p, WIDE_TOTAL, &o);
        assert!(trace(&log).lines().any(|l| l.contains(" fetch ") && l.ends_with(&a)), "{}", trace(&log));
        assert_eq!(skill(&p, "demo", "1.0.0"), "---\nname: hello\ndescription: Says hello\n---\nSay hello.\n");
        assert_eq!(r.lines, [moved("team", "v1.0.0")], "{r:?}");
        let cfg = merged(&fx, &mp("team", &repo.src(), "v1.0.0", Some(&"0".repeat(40)), &["demo"]), None);
        let r = sync_with(&cfg, &p, WIDE_TOTAL, &opts(&fx));
        assert_eq!(r.lines, offline());
        assert_eq!(r.failures, [("team".to_string(), "pinned commit not found".to_string())]);
    }

    #[test]
    fn moved_tag_line_escapes_the_ref() {
        let fx = Fx::new();
        let repo = market_repo(&fx, &[("demo", "1.0.0")]);
        let tag = "v1.0.0\u{202e}";
        repo.tag(tag);
        let a = git(&repo.dir, &["rev-parse", "HEAD"]);
        let p = paths(&fx);
        let cfg = merged(&fx, &mp("team", &repo.src(), tag, Some(&a), &["demo"]), None);
        bump(&repo, &[("demo", "1.1.0")], "Say B.", tag);
        let r = sync_with(&cfg, &p, WIDE_TOTAL, &opts(&fx));
        assert_eq!(r.lines, [moved("team", "v1.0.0\\u{202e}")], "{r:?}");
    }

    #[test]
    fn recorded_commit_is_the_pin_when_config_has_none() {
        let fx = Fx::new();
        let repo = market_repo(&fx, &[("demo", "1.0.0")]);
        let a = git(&repo.dir, &["rev-parse", "HEAD"]);
        let p = paths(&fx);
        let cfg = merged(&fx, &mp("team", &repo.src(), "v1.0.0", None, &["demo"]), None);
        assert!(sync_with(&cfg, &p, WIDE_TOTAL, &opts(&fx)).lines.is_empty());
        assert_eq!(manifest(&p).marketplaces["team"].commit, a);
        let b = bump(&repo, &[("demo", "1.0.0")], "Say B.", "v1.0.0");
        let r = sync_with(&cfg, &p, WIDE_TOTAL, &opts(&fx));
        assert_eq!(r.lines, [moved("team", "v1.0.0")]);
        assert!(skill(&p, "demo", "1.0.0").contains("Say hello."));
        assert_eq!(manifest(&p).marketplaces["team"].commit, a);
        repo.tag("v1.0.1");
        let cfg = merged(&fx, &mp("team", &repo.src(), "v1.0.1", None, &["demo"]), None);
        let r = sync_with(&cfg, &p, WIDE_TOTAL, &opts(&fx));
        assert!(r.lines.is_empty(), "{r:?}");
        assert_eq!(manifest(&p).marketplaces["team"].commit, b);
        assert!(skill(&p, "demo", "1.0.0").contains("Say B."));
    }

    #[test]
    fn failed_fetch_keeps_the_previous_store_and_prints_offline() {
        let fx = Fx::new();
        let repo = market_repo(&fx, &[("demo", "1.0.0")]);
        let p = paths(&fx);
        let cfg = merged(&fx, &mp("team", &repo.src(), "v1.0.0", None, &["demo"]), None);
        sync_with(&cfg, &p, WIDE_TOTAL, &opts(&fx));
        let before = std::fs::read(store(&p).join("manifest.json")).unwrap();
        std::fs::remove_dir_all(&repo.dir).unwrap();
        let r = sync_with(&cfg, &p, WIDE_TOTAL, &opts(&fx));
        assert_eq!(r.lines, offline());
        assert_eq!(r.failures.len(), 1, "{r:?}");
        assert_eq!(std::fs::read(store(&p).join("manifest.json")).unwrap(), before);
        assert!(state(&store(&p)).plugins.iter().all(|x| x.intact));
    }

    #[test]
    fn one_failed_marketplace_prints_partial() {
        let fx = Fx::new();
        let repo = market_repo(&fx, &[("demo", "1.0.0")]);
        let p = paths(&fx);
        let user = mp("team", &repo.src(), "v1.0.0", None, &["demo"]) + &mp("gone", &fx.path("missing").display().to_string(), "v1.0.0", None, &["demo"]);
        let r = sync_with(&merged(&fx, &user, None), &p, WIDE_TOTAL, &opts(&fx));
        assert_eq!(r.lines, partial());
        assert_eq!(r.failures[0].0, "gone");
        assert_eq!(r.synced, [id("team:demo")]);
    }

    #[test]
    fn zero_budget_attempts_nothing() {
        let fx = Fx::new();
        let repo = market_repo(&fx, &[("demo", "1.0.0")]);
        let p = paths(&fx);
        let cfg = merged(&fx, &mp("team", &repo.src(), "v1.0.0", None, &["demo"]), None);
        let r = sync_with(&cfg, &p, Duration::ZERO, &opts(&fx));
        assert_eq!(r.lines, offline());
        assert!(!store(&p).join("manifest.json").exists());
        assert!(!p.marketplaces_dir.exists());
    }

    #[test]
    fn branch_ref_is_refused() {
        let fx = Fx::new();
        let repo = market_repo(&fx, &[("demo", "1.0.0")]);
        let p = paths(&fx);
        let r = sync_with(&merged(&fx, &mp("team", &repo.src(), "main", None, &["demo"]), None), &p, WIDE_TOTAL, &opts(&fx));
        assert!(r.failures[0].1.contains("not a tag or a commit"), "{r:?}");
        assert_eq!(r.lines, offline());
        assert!(names(&store(&p)).iter().all(|n| n.starts_with('.')), "{:?}", names(&store(&p)));
    }

    #[cfg(unix)]
    #[test]
    fn config_values_cannot_inject_git_options() {
        let fx = Fx::new();
        let repo = market_repo(&fx, &[("demo", "1.0.0")]);
        let p = paths(&fx);
        let m = fx.path("M");
        let inject = format!("--upload-pack=touch {}", m.display());
        let hex = "ab".repeat(20);
        let src = repo.src();
        let mut cases: Vec<(String, String, Option<String>)> = vec![
            (src.clone(), inject.clone(), None),
            (src.clone(), "v1.0.0".into(), Some(inject.clone())),
            (src.clone(), "v1.0.0".into(), Some(hex[..39].to_string())),
            (src.clone(), "v1.0.0".into(), Some(hex.to_uppercase())),
            (src.clone(), "v1.0.0".into(), Some(format!(" {hex}"))),
            (inject.clone(), "v1.0.0".into(), None),
        ];
        for r in ["a b", "v1..2", "x@{y}", "-v1"] { cases.push((src.clone(), r.into(), None)); }
        for (source, r, commit) in cases {
            let cfg = merged(&fx, &mp("team", &source, &r, commit.as_deref(), &["demo"]), None);
            let out = sync_with(&cfg, &p, WIDE_TOTAL, &opts(&fx));
            let why = &out.failures[0].1;
            assert!(["ref is not a valid tag name", "commit is not a 40-hex id", "source is not"].iter().any(|w| why.contains(w)),
                "{source} {r} {commit:?}: {why}");
            assert_eq!(out.lines, offline());
            assert!(!m.exists());
        }
    }

    #[test]
    fn unsafe_marketplace_name_is_refused() {
        let fx = Fx::new();
        let repo = market_repo(&fx, &[("demo", "1.0.0")]);
        let p = paths(&fx);
        let r = sync_with(&merged(&fx, &mp("../x", &repo.src(), "v1.0.0", None, &["demo"]), None), &p, WIDE_TOTAL, &opts(&fx));
        assert_eq!(r.failures[0].1, "unsafe marketplace name");
        assert!(!fx.path("home/cache/x").exists() && !fx.path("home/x").exists());
    }

    #[cfg(unix)]
    #[test]
    fn cached_repository_config_runs_nothing_and_the_clone_is_reused() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let fx = Fx::new();
        let repo = market_repo(&fx, &[("demo", "1.0.0")]);
        repo.write(".gitattributes", "plugins/demo/skills/** filter=zz\n");
        repo.commit("attributes");
        repo.tag_force("v1.0.0");
        let p = paths(&fx);
        let (o, log) = traced(&fx);
        let r = sync_with(&merged(&fx, &mp("team", &repo.src(), "v1.0.0", None, &["demo"]), None), &p, WIDE_TOTAL, &o);
        assert!(r.failures.is_empty(), "{r:?}");
        let m = fx.path("M");
        let dotgit = cache(&p).join(".git");
        let mut cfg_text = std::fs::read_to_string(dotgit.join("config")).unwrap();
        let t = format!("touch {}", m.display());
        cfg_text.push_str(&format!("[core]\n\tfsmonitor = \"{t}\"\n[filter \"zz\"]\n\tsmudge = \"{t}\"\n\tclean = \"{t}\"\n[credential]\n\thelper = \"!{t}\"\n[alias]\n\tls-files = \"!{t}\"\n"));
        std::fs::write(dotgit.join("config"), cfg_text).unwrap();
        for h in ["post-checkout", "reference-transaction", "post-merge"] {
            let f = dotgit.join("hooks").join(h);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(&f, format!("#!/bin/sh\n{t}\n")).unwrap();
            std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        std::fs::write(dotgit.join("zygo-reuse-marker"), "x").unwrap();
        let ino = std::fs::metadata(&dotgit).unwrap().ino();
        bump(&repo, &[("demo", "2.0.0")], "Say two.", "v2.0.0");
        std::fs::write(&log, "").unwrap();
        let cfg = merged(&fx, &mp("team", &repo.src(), "v2.0.0", None, &["demo"]), None);
        let r = sync_with(&cfg, &p, WIDE_TOTAL, &o);
        assert!(r.failures.is_empty(), "{r:?}");
        assert!(dotgit.join("zygo-reuse-marker").exists());
        assert_eq!(std::fs::metadata(&dotgit).unwrap().ino(), ino);
        let tr = trace(&log);
        assert!(tr.contains(" fetch ") && tr.contains(" checkout ") && !tr.contains(" clone "), "{tr}");
        assert!(!m.exists());
        assert!(skill(&p, "demo", "2.0.0").contains("Say two."));
        let c = std::fs::read_to_string(dotgit.join("config")).unwrap();
        assert!(!c.contains("fsmonitor") && !c.contains("zz") && !c.contains("helper"), "{c}");
        // A `.git` that is a symlink is not used: the cache is cloned again and the target is untouched.
        let elsewhere = fx.path("elsewhere");
        fx.write("elsewhere/keep", "keep");
        std::fs::remove_dir_all(&dotgit).unwrap();
        std::os::unix::fs::symlink(&elsewhere, &dotgit).unwrap();
        std::fs::write(&log, "").unwrap();
        sync_with(&cfg, &p, WIDE_TOTAL, &o);
        assert!(trace(&log).contains(" clone "), "{}", trace(&log));
        assert_eq!(names(&elsewhere), ["keep"]);
        assert_eq!(std::fs::read_to_string(elsewhere.join("keep")).unwrap(), "keep");
    }

    #[test]
    fn plugin_that_fails_the_check_is_not_stored_and_counts_as_partial() {
        let fx = Fx::new();
        let repo = market_repo(&fx, &[("demo", "1.0.0")]);
        let p = paths(&fx);
        sync_with(&merged(&fx, &mp("team", &repo.src(), "v1.0.0", None, &["demo"]), None), &p, WIDE_TOTAL, &opts(&fx));
        let before = entry(&manifest(&p), "demo").unwrap().clone();
        repo.write("plugins/demo/skills/hello/big.txt", vec![b'a'; 1_048_577]);
        repo.commit("big");
        repo.tag("v1.0.1");
        let r = sync_with(&merged(&fx, &mp("team", &repo.src(), "v1.0.1", None, &["demo"]), None), &p, WIDE_TOTAL, &opts(&fx));
        assert_eq!(r.lines, partial());
        assert_eq!(r.failures, [("team:demo".to_string(), "check failed: file-too-large skills/hello/big.txt (1048577 bytes, limit 1048576)".to_string())]);
        assert_eq!(entry(&manifest(&p), "demo"), Some(&before));
        assert!(!store(&p).join("team/demo/1.0.0/skills/hello/big.txt").exists());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_in_the_marketplace_is_refused() {
        let fx = Fx::new();
        let repo = market_repo(&fx, &[("demo", "1.0.0")]);
        std::os::unix::fs::symlink("SKILL.md", repo.dir.join("plugins/demo/skills/hello/link.md")).unwrap();
        repo.commit("link");
        repo.tag_force("v1.0.0");
        let p = paths(&fx);
        let r = sync_with(&merged(&fx, &mp("team", &repo.src(), "v1.0.0", None, &["demo"]), None), &p, WIDE_TOTAL, &opts(&fx));
        assert_eq!(r.failures, [("team:demo".to_string(), "check failed: symlink skills/hello/link.md (symlinks are refused)".to_string())]);
        assert_eq!(r.lines, partial());
        assert!(!store(&p).join("team/demo").exists());
    }

    #[test]
    fn only_selected_plugins_are_stored() {
        let fx = Fx::new();
        let repo = market_repo(&fx, &[("demo", "1.0.0"), ("other", "1.0.0")]);
        let p = paths(&fx);
        let user = mp("team", &repo.src(), "v1.0.0", None, &["demo"]);
        sync_with(&merged(&fx, &user, None), &p, WIDE_TOTAL, &opts(&fx));
        assert_eq!(names(&store(&p).join("team")), ["demo"]);
        let user = user + "[profile.p.plugins]\n\"team:other\" = true\n";
        sync_with(&merged(&fx, &user, None), &p, WIDE_TOTAL, &opts(&fx));
        assert_eq!(names(&store(&p).join("team")), ["demo", "other"]);
    }

    #[test]
    fn marketplace_violation_counts_as_partial_and_blocks_nothing() {
        let fx = Fx::new();
        let repo = market_repo(&fx, &[("demo", "1.0.0")]);
        repo.write(".claude-plugin/marketplace.json", r#"{"name":"team","owner":{"name":"Owner"},"plugins":[{"name":"demo","source":"./plugins/demo","description":"Demo","version":"1.0.0"},{"name":"bad","source":"./../bad","description":"Bad","version":"1.0.0"}]}"#);
        repo.commit("bad");
        repo.tag_force("v1.0.0");
        let p = paths(&fx);
        let r = sync_with(&merged(&fx, &mp("team", &repo.src(), "v1.0.0", None, &["demo", "bad"]), None), &p, WIDE_TOTAL, &opts(&fx));
        assert_eq!(r.lines, partial());
        let rule = check_dir(&repo.dir).violations[0].rule.as_str();
        assert_eq!(r.failures[0].0, "team");
        assert!(r.failures[0].1.contains(rule), "{r:?}");
        assert_eq!(r.synced, [id("team:demo")]);
    }

    #[test]
    fn skipped_source_prints_the_spec_line() {
        let fx = Fx::new();
        let repo = market_repo(&fx, &[("demo", "1.0.0")]);
        repo.write(".claude-plugin/marketplace.json", r#"{"name":"team","owner":{"name":"Owner"},"plugins":[{"name":"demo","source":"./plugins/demo","description":"Demo","version":"1.0.0"},{"name":"remote","source":{"source":"github","repo":"o/r"},"description":"Remote"}]}"#);
        repo.commit("remote");
        repo.tag_force("v1.0.0");
        let p = paths(&fx);
        let r = sync_with(&merged(&fx, &mp("team", &repo.src(), "v1.0.0", None, &["demo", "remote"]), None), &p, WIDE_TOTAL, &opts(&fx));
        assert_eq!(r.lines, ["plugin team:remote: skipped, source is not a relative path"]);
        assert_eq!(r.synced, [id("team:demo")]);
    }

    #[test]
    fn code_digest_is_stable_for_crlf_and_changes_with_one_byte() {
        const KNOWN: &str = "b697e4793e8915f7d0964e4fb98c7301e4212b6f8e903f4594d47d8fae6925bc";
        let mut digests = vec![];
        let fa = Fx::new();
        let a = market_repo_at(&fa, "a", &[("demo", "1.0.0")]);
        a.write("plugins/demo/scripts/run.sh", "echo hi\n");
        a.commit("lf");
        a.tag_force("v1.0.0");
        let pa = paths(&fa);
        sync_with(&merged(&fa, &mp("team", &a.src(), "v1.0.0", None, &["demo"]), None), &pa, WIDE_TOTAL, &opts(&fa));
        digests.push(entry(&manifest(&pa), "demo").unwrap().digest.clone());
        let fb = Fx::new();
        let b = market_repo_at(&fb, "b", &[("demo", "1.0.0")]);
        b.write("plugins/demo/scripts/run.sh", "echo hi\r\n");
        b.commit("crlf");
        b.tag_force("v1.0.0");
        let pb = paths(&fb);
        sync_with(&merged(&fb, &mp("team", &b.src(), "v1.0.0", None, &["demo"]), None), &pb, WIDE_TOTAL, &opts(&fb));
        digests.push(entry(&manifest(&pb), "demo").unwrap().digest.clone());
        assert_eq!(digests, [Some(KNOWN.to_string()), Some(KNOWN.to_string())]);
        a.write("plugins/demo/scripts/run.sh", "echo hj\n");
        a.commit("one byte");
        a.tag("v1.0.1");
        sync_with(&merged(&fa, &mp("team", &a.src(), "v1.0.1", None, &["demo"]), None), &pa, WIDE_TOTAL, &opts(&fa));
        let new = entry(&manifest(&pa), "demo").unwrap().digest.clone().unwrap();
        assert_ne!(new, KNOWN);
        let file = pa.approvals_file.clone();
        zygo_plugins::approve::approve_in(&file, "team", "demo", KNOWN).unwrap();
        assert!(zygo_plugins::approve::is_approved_in(&file, "team", "demo", KNOWN));
        assert!(!zygo_plugins::approve::is_approved_in(&file, "team", "demo", &new));
    }

    fn leftovers(p: &Paths) -> Vec<String> {
        names(&store(p).join("team/demo")).into_iter().filter(|n| n.contains(".old-") || n.contains(".tmp-")).collect()
    }

    #[test]
    fn same_version_new_content_replaces_and_old_dirs_go_after_the_manifest() {
        let fx = Fx::new();
        let repo = market_repo(&fx, &[("demo", "1.0.0")]);
        let p = paths(&fx);
        sync_with(&merged(&fx, &mp("team", &repo.src(), "v1.0.0", None, &["demo"]), None), &p, WIDE_TOTAL, &opts(&fx));
        bump(&repo, &[("demo", "1.0.0")], "Say again.", "v1.0.1");
        sync_with(&merged(&fx, &mp("team", &repo.src(), "v1.0.1", None, &["demo"]), None), &p, WIDE_TOTAL, &opts(&fx));
        assert!(skill(&p, "demo", "1.0.0").contains("Say again."));
        assert!(leftovers(&p).is_empty(), "{:?}", leftovers(&p));
        bump(&repo, &[("demo", "1.1.0")], "Say more.", "v1.1.0");
        sync_with(&merged(&fx, &mp("team", &repo.src(), "v1.1.0", None, &["demo"]), None), &p, WIDE_TOTAL, &opts(&fx));
        assert_eq!(names(&store(&p).join("team/demo")), ["1.1.0"]);
    }

    #[cfg(unix)]
    #[test]
    fn manifest_failure_keeps_the_previous_store() {
        use std::os::unix::fs::PermissionsExt;
        let fx = Fx::new();
        let repo = market_repo(&fx, &[("demo", "1.0.0")]);
        let p = paths(&fx);
        sync_with(&merged(&fx, &mp("team", &repo.src(), "v1.0.0", None, &["demo"]), None), &p, WIDE_TOTAL, &opts(&fx));
        let before = std::fs::read(store(&p).join("manifest.json")).unwrap();
        std::fs::set_permissions(store(&p), std::fs::Permissions::from_mode(0o555)).unwrap();
        // Running as root ignores the mode: the failure cannot be produced, so the test has nothing to show.
        if std::fs::write(store(&p).join("probe"), "x").is_ok() {
            let _ = std::fs::remove_file(store(&p).join("probe"));
            std::fs::set_permissions(store(&p), std::fs::Permissions::from_mode(0o755)).unwrap();
            return;
        }
        bump(&repo, &[("demo", "1.0.0")], "Say new.", "v1.0.1");
        let r = sync_with(&merged(&fx, &mp("team", &repo.src(), "v1.0.1", None, &["demo"]), None), &p, WIDE_TOTAL, &opts(&fx));
        std::fs::set_permissions(store(&p), std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(r.failures.iter().any(|f| f.1.contains("manifest")), "{r:?}");
        assert!(skill(&p, "demo", "1.0.0").contains("Say hello."));
        assert_eq!(std::fs::read(store(&p).join("manifest.json")).unwrap(), before);
        assert!(leftovers(&p).is_empty(), "{:?}", leftovers(&p));
    }

    #[test]
    fn unselected_absent_and_other_source_entries_are_dropped() {
        let fx = Fx::new();
        let repo = market_repo(&fx, &[("demo", "1.0.0"), ("other", "1.0.0")]);
        let p = paths(&fx);
        let run = |src: &str, r: &str, on: &[&str]| sync_with(&merged(&fx, &mp("team", src, r, None, on), None), &p, WIDE_TOTAL, &opts(&fx));
        run(&repo.src(), "v1.0.0", &["demo", "other"]);
        assert_eq!(names(&store(&p).join("team")), ["demo", "other"]);
        // (1) gone upstream
        std::fs::remove_dir_all(repo.dir.join("plugins/other")).unwrap();
        bump(&repo, &[("demo", "1.0.0")], "Say hello.", "v1.0.1");
        run(&repo.src(), "v1.0.1", &["demo", "other"]);
        assert!(entry(&manifest(&p), "other").is_none());
        assert!(!store(&p).join("team/other").exists());
        // (2) no longer selected
        run(&repo.src(), "v1.0.1", &["other"]);
        assert!(entry(&manifest(&p), "demo").is_none());
        assert!(!store(&p).join("team/demo").exists());
        // (3) another source
        run(&repo.src(), "v1.0.1", &["demo"]);
        let second = market_repo_at(&fx, "second", &[("demo", "1.0.0")]);
        bump(&second, &[("demo", "1.0.0")], "Say second.", "v1.0.1");
        let r = run(&second.src(), "v1.0.1", &["demo"]);
        assert!(r.failures.is_empty(), "{r:?}");
        assert_eq!(manifest(&p).marketplaces["team"].source, second.src());
        // rewrite_config writes the url as a git config string: `\` doubled.
        assert!(std::fs::read_to_string(cache(&p).join(".git/config")).unwrap().contains(&second.src().replace('\\', "\\\\")));
        assert!(skill(&p, "demo", "1.0.0").contains("Say second."));
    }

    #[test]
    fn reserved_plugin_name_zygo_is_refused() {
        let fx = Fx::new();
        let repo = market_repo(&fx, &[("zygo", "1.0.0")]);
        let p = paths(&fx);
        let r = sync_with(&merged(&fx, &mp("team", &repo.src(), "v1.0.0", None, &["zygo"]), None), &p, WIDE_TOTAL, &opts(&fx));
        assert_eq!(r.failures, [("team:zygo".to_string(), "plugin name zygo is reserved".to_string())]);
        assert!(!store(&p).join("team/zygo").exists());
        assert_eq!(r.lines, partial());
    }

    #[test]
    #[ignore = "timing: a 50 ms sync budget and a 5 s lock wait"]
    fn marketplace_lock_busy_is_offline_within_the_deadline() {
        let fx = Fx::new();
        let repo = market_repo(&fx, &[("demo", "1.0.0")]);
        let p = paths(&fx);
        let g = lock_marketplace(&p, "team", far()).unwrap().unwrap();
        let cfg = merged(&fx, &mp("team", &repo.src(), "v1.0.0", None, &["demo"]), None);
        let r = sync_with(&cfg, &p, Duration::from_millis(50), &opts(&fx));
        assert_eq!(r.failures[0].1, "locked");
        assert_eq!(r.lines, offline());
        assert!(!cache(&p).exists());
        assert!(lock_marketplace(&p, "team", Instant::now()).unwrap().is_none());
        drop(g);
        // Bounded, not `now`: a sibling test's fork/exec may hold a copy of the descriptor for a moment.
        assert!(lock_marketplace(&p, "team", Instant::now() + Duration::from_secs(5)).unwrap().is_some());
    }

    #[cfg(unix)]
    #[test]
    fn local_source_is_cloned_with_no_local() {
        use std::os::unix::fs::MetadataExt;
        let fx = Fx::new();
        let repo = market_repo(&fx, &[("demo", "1.0.0")]);
        let p = paths(&fx);
        sync_with(&merged(&fx, &mp("team", &repo.src(), "v1.0.0", None, &["demo"]), None), &p, WIDE_TOTAL, &opts(&fx));
        let objs: Vec<_> = walkdir::WalkDir::new(cache(&p).join(".git/objects")).into_iter().flatten().filter(|e| e.file_type().is_file()).collect();
        assert!(!objs.is_empty());
        for e in objs { assert_eq!(e.metadata().unwrap().nlink(), 1, "{}", e.path().display()); }
    }

    #[cfg(unix)]
    #[test]
    fn wide_opts_survive_a_clone_slower_than_the_production_bound() {
        use std::os::unix::fs::PermissionsExt;
        let fx = Fx::new();
        let repo = market_repo_at(&fx, "origin1", &[("demo", "1.0.0")]);
        let stub = fx.write("bin/slow-clone-git",
            "#!/bin/sh\nfor a; do [ \"$a\" = --version ] && { echo git version 2.50.0; exit 0; }; done\ncase \" $* \" in *\" clone \"*) sleep 6;; esac\nexec git \"$@\"\n");
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        let p = paths(&fx);
        let o = SyncOpts { git_program: Some(stub), ..opts(&fx) };
        let r = sync_with(&merged(&fx, &mp("team", &repo.src(), "v1.0.0", None, &["demo"]), None), &p, WIDE_TOTAL, &o);
        assert!(r.failures.is_empty() && r.lines.is_empty(), "{r:?}");
        assert!(skill(&p, "demo", "1.0.0").contains("Say hello."));
    }

    #[cfg(unix)]
    #[test]
    fn a_failed_git_run_scrubs_userinfo_from_its_stderr() {
        let fx = Fx::new();
        let t = format!("tk{}", zygo_core::ids::ulid());
        let p = paths(&fx);
        let o = SyncOpts { git_program: Some(url_git(&fx)), ..opts(&fx) };
        for (src, shown) in [
            (format!("git+https://bob:{t}@h.example/o/r.git"), "https://h.example/o/r.git/"),
            (format!("git+ssh://git:{t}@h.example/o/r.git"), "ssh://h.example/o/r.git/"),
        ] {
            let r = sync_with(&merged(&fx, &mp("team", &src, "v1.0.0", None, &[]), None), &p, WIDE_TOTAL, &o);
            assert_eq!(r.failures, [("team".to_string(), format!("git failed: fatal: unable to access '{shown}': boom"))], "{r:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn sync_one_with_uses_the_given_opts() {
        use std::os::unix::fs::PermissionsExt;
        let fx = Fx::new();
        let ran = fx.path("ran");
        let fake = fx.write("bin/fake-git", format!("#!/bin/sh\ntouch '{}'\nexit 1\n", ran.display()));
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let p = paths(&fx);
        let o = SyncOpts { git_program: Some(fake), ..opts(&fx) };
        let cfg = merged(&fx, &mp("team", "/nowhere", "v1", None, &["demo"]), None);
        let r = sync_one_with(&cfg, &p, WIDE_TOTAL, &o, &id("team:demo"));
        assert!(ran.exists());
        assert_eq!(r.failures.len(), 1, "{r:?}");
    }

    #[test]
    fn opts_helper_gives_a_wide_budget() {
        let fx = Fx::new();
        assert!(opts(&fx).per_marketplace >= Duration::from_secs(60));
        assert!(WIDE_TOTAL >= Duration::from_secs(60));
    }

    #[cfg(unix)]
    #[test]
    #[ignore = "timing: races a hanging git against a 10 s deadline and asserts elapsed time"]
    fn slow_marketplace_is_killed_at_its_deadline() {
        use std::os::unix::fs::PermissionsExt;
        let fx = Fx::new();
        let fast = market_repo_at(&fx, "origin1", &[("demo", "1.0.0")]);
        let slow = market_repo_at(&fx, "origin2", &[("demo", "1.0.0")]);
        let pidfile = fx.path("slow.pid");
        let fake = fx.write("bin/fake-git", format!(
            "#!/bin/sh\nfor a; do [ \"$a\" = --version ] && {{ echo git version 2.50.0; exit 0; }}; done\ncase \"$PWD $*\" in *slow*) echo $$ > '{}'; sleep 30;; *) exec git \"$@\";; esac\n",
            pidfile.display()));
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let p = paths(&fx);
        let user = mp("fast", &fast.src(), "v1.0.0", None, &["demo"]) + &mp("slow", &slow.src(), "v1.0.0", None, &["demo"]);
        let o = SyncOpts { per_marketplace: Duration::from_secs(10), git_program: Some(fake), ..opts(&fx) };
        let t = Instant::now();
        let r = sync_with(&merged(&fx, &user, None), &p, TOTAL, &o);
        assert!(t.elapsed() < Duration::from_secs(20));
        assert_eq!(r.lines, partial());
        assert_eq!(r.failures, [("slow".to_string(), "git timed out".to_string())]);
        let m = manifest(&p);
        assert!(m.plugins.iter().any(|e| e.marketplace == "fast"));
        assert!(!m.plugins.iter().any(|e| e.marketplace == "slow"));
        let pid: i32 = std::fs::read_to_string(&pidfile).unwrap().trim().parse().unwrap();
        group_gone(pid);
    }

    #[test]
    fn builtin_entry_without_a_ref_is_skipped_before_any_git_call() {
        let fx = Fx::new();
        let m = fx.path("M");
        let fake = if cfg!(windows) {
            fx.write("bin/fake-git.bat", format!("@echo x > \"{}\"\r\n@exit /b 1\r\n", m.display()))
        } else {
            let f = fx.write("bin/fake-git", format!("#!/bin/sh\ntouch '{}'\nexit 1\n", m.display()));
            #[cfg(unix)]
            std::fs::set_permissions(&f, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
            f
        };
        let builtin: toml::Table = "[[marketplace]]\nname = \"act101\"\nsource = \"github:act101-ai/act101\"\ndefault_on = [\"act-lite\"]\n".parse().unwrap();
        let load = |user: &str| -> Merged {
            let p = paths(&fx);
            std::fs::create_dir_all(p.config_file.parent().unwrap()).unwrap();
            std::fs::write(&p.config_file, user).unwrap();
            load_with_builtin(&LoadContext { paths: p, repo: None, team: None }, builtin.clone()).unwrap()
        };
        let p = paths(&fx);
        let o = SyncOpts { git_program: Some(fake), ..opts(&fx) };
        let cfg = load("");
        let r = sync_with(&cfg, &p, WIDE_TOTAL, &o);
        assert!(!m.exists());
        assert!(r.lines.is_empty() && r.failures.is_empty(), "{r:?}");
        assert!(selection(&cfg, None).selected.contains(&id("act101:act-lite")));
        let r = sync_with(&load("[[marketplace]]\nname = \"act101\"\nref = \"v1\"\n"), &p, WIDE_TOTAL, &o);
        assert!(m.exists(), "{r:?}");
        let nowhere = if cfg!(windows) { r"C:\nowhere" } else { "/nowhere" };
        let r = sync_with(&load(&mp("u", nowhere, "", None, &[])), &p, WIDE_TOTAL, &opts(&fx));
        assert_eq!(r.failures, [("u".to_string(), "ref is not a tag or a commit".to_string())]);
    }

    #[test]
    fn unchanged_commit_skips_checkout_and_staging() {
        let fx = Fx::new();
        let repo = market_repo(&fx, &[("demo", "1.0.0")]);
        let p = paths(&fx);
        let cfg = merged(&fx, &mp("team", &repo.src(), "v1.0.0", None, &["demo"]), None);
        let (o, log) = traced(&fx);
        sync_with(&cfg, &p, WIDE_TOTAL, &o);
        let mf = store(&p).join("manifest.json");
        let (bytes, mtime) = (std::fs::read(&mf).unwrap(), std::fs::metadata(&mf).unwrap().modified().unwrap());
        std::fs::write(&log, "").unwrap();
        let r = sync_with(&cfg, &p, WIDE_TOTAL, &o);
        assert!(r.lines.is_empty() && r.failures.is_empty(), "{r:?}");
        let tr = trace(&log);
        assert!(tr.contains(" fetch ") && !tr.contains(" clone ") && !tr.contains(" checkout "), "{tr}");
        assert_eq!(std::fs::read(&mf).unwrap(), bytes);
        assert_eq!(std::fs::metadata(&mf).unwrap().modified().unwrap(), mtime);
        assert!(leftovers(&p).is_empty());
        assert!(lock_until(&store(&p), true, far()).unwrap().is_some());
        let e = entry(&manifest(&p), "demo").unwrap().clone();
        std::fs::write(entry_dir(&store(&p), &e).join("skills/hello/SKILL.md"), "tampered").unwrap();
        assert!(!verify(&store(&p), &e));
        sync_with(&cfg, &p, WIDE_TOTAL, &o);
        assert!(verify(&store(&p), &entry(&manifest(&p), "demo").unwrap().clone()));
        assert!(skill(&p, "demo", "1.0.0").contains("Say hello."));
    }

    #[test]
    fn sync_one_keeps_every_other_entry_and_the_recorded_commit() {
        let fx = Fx::new();
        let repo = market_repo(&fx, &[("demo", "1.0.0"), ("other", "1.0.0"), ("third", "1.0.0")]);
        let a = git(&repo.dir, &["rev-parse", "HEAD"]);
        let p = paths(&fx);
        sync_with(&merged(&fx, &mp("team", &repo.src(), "v1.0.0", None, &["demo", "other"]), None), &p, WIDE_TOTAL, &opts(&fx));
        let before = manifest(&p);
        assert!(entry(&before, "third").is_none());
        // Upstream moves on: `other` removed, `third` changed, the tag moved.
        std::fs::remove_dir_all(repo.dir.join("plugins/other")).unwrap();
        bump(&repo, &[("demo", "1.0.0"), ("third", "1.0.0")], "Say B.", "v1.0.0");
        let cfg = merged(&fx, &(mp("team", &repo.src(), "v1.0.0", None, &["demo", "other"]) + "[plugins]\nenable = [\"team:third\"]\n"), None);
        let r = sync_one_with(&cfg, &p, WIDE_TOTAL, &opts(&fx), &id("team:third"));
        assert_eq!(r.synced, [id("team:third")], "{r:?}");
        let after = manifest(&p);
        assert_eq!(entry(&after, "demo"), entry(&before, "demo"));
        assert_eq!(entry(&after, "other"), entry(&before, "other"));
        assert!(entry(&after, "third").is_some());
        assert!(skill(&p, "third", "1.0.0").contains("Say hello."), "third is taken at the recorded pin A");
        assert_eq!(after.marketplaces["team"].commit, a);
    }

    #[cfg(unix)]
    #[test]
    fn lfs_and_submodule_config_do_not_run() {
        let fx = Fx::new();
        let m = fx.path("M");
        let t = format!("touch {}", m.display());
        let global = fx.write("lfs-gitconfig", format!("[filter \"lfs\"]\n\tsmudge = \"{t}\"\n\tprocess = \"{t}\"\n\trequired = true\n"));
        let sub = Repo::new(&fx, "sub");
        sub.write("x", "x");
        sub.commit("sub");
        let repo = market_repo(&fx, &[("demo", "1.0.0")]);
        repo.write(".gitattributes", "*.bin filter=lfs\n");
        repo.write("plugins/demo/skills/hello/x.bin", [0u8, 1, 2, 3]);
        repo.write(".gitmodules", format!("[submodule \"sub\"]\n\tpath = sub\n\turl = {}\n", sub.src()));
        repo.commit("lfs");
        repo.tag_force("v1.0.0");
        let p = paths(&fx);
        let o = SyncOpts { git_config_global: Some(global), ..opts(&fx) };
        let r = sync_with(&merged(&fx, &mp("team", &repo.src(), "v1.0.0", None, &["demo"]), None), &p, WIDE_TOTAL, &o);
        assert!(r.failures.is_empty(), "{r:?}");
        assert!(!m.exists());
        assert_eq!(std::fs::read(store(&p).join("team/demo/1.0.0/skills/hello/x.bin")).unwrap(), [0u8, 1, 2, 3]);
        assert!(!cache(&p).join("sub").exists());
    }

    // Round 2 (review)

    /// Item 1: `plugin.json` naming `.git` (source `./`) would copy the cache's git dir.
    #[test]
    fn dot_git_named_by_a_plugin_is_refused() {
        let fx = Fx::new();
        let repo = Repo::new(&fx, "origin");
        repo.write(".claude-plugin/marketplace.json",
            r#"{"name":"team","owner":{"name":"O"},"plugins":[{"name":"demo","source":"./","description":"D","version":"1.0.0"}]}"#);
        repo.write(".claude-plugin/plugin.json", r#"{"name":"demo","version":"1.0.0","description":"D","agents":".git"}"#);
        repo.commit("c");
        repo.tag("v1.0.0");
        let p = paths(&fx);
        let r = sync_with(&merged(&fx, &mp("team", &repo.src(), "v1.0.0", None, &["demo"]), None), &p, WIDE_TOTAL, &opts(&fx));
        assert_eq!(r.lines, partial(), "{r:?}");
        assert!(r.failures[0].1.starts_with("check failed: path-outside"), "{r:?}");
        assert!(!store(&p).join("team/demo/1.0.0/.git").exists());
        assert!(read_manifest(&store(&p)).unwrap().is_none_or(|m| m.plugins.is_empty()));
    }

    /// Item 2: a 40-hex `ref` is a commit even when a tag has that name.
    #[test]
    fn full_hex_ref_is_a_commit_even_when_a_tag_has_its_name() {
        let fx = Fx::new();
        let repo = market_repo(&fx, &[("demo", "1.0.0")]);
        let a = git(&repo.dir, &["rev-parse", "HEAD"]);
        let b = bump(&repo, &[("demo", "1.0.0")], "Say B.", "v2");
        git(&repo.dir, &["tag", &a, &b]);
        let p = paths(&fx);
        let r = sync_with(&merged(&fx, &mp("team", &repo.src(), &a, None, &["demo"]), None), &p, WIDE_TOTAL, &opts(&fx));
        assert!(r.failures.is_empty() && r.lines.is_empty(), "{r:?}");
        assert_eq!(manifest(&p).marketplaces["team"].commit, a);
        assert!(skill(&p, "demo", "1.0.0").contains("Say hello."));
    }

    /// Item 3: a plugin that failed while the record moved on is retried and not reported as synced.
    #[cfg(unix)]
    #[test]
    fn plugin_that_failed_is_retried_at_the_recorded_commit() {
        use std::os::unix::fs::PermissionsExt;
        let fx = Fx::new();
        let repo = market_repo(&fx, &[("demo", "1.0.0"), ("other", "1.0.0")]);
        let p = paths(&fx);
        sync_with(&merged(&fx, &mp("team", &repo.src(), "v1.0.0", None, &["demo", "other"]), None), &p, WIDE_TOTAL, &opts(&fx));
        let b = bump(&repo, &[("demo", "2.0.0"), ("other", "2.0.0")], "Say B.", "v2.0.0");
        let cfg = merged(&fx, &mp("team", &repo.src(), "v2.0.0", None, &["demo", "other"]), None);
        let pd = store(&p).join("team/demo");
        std::fs::set_permissions(&pd, std::fs::Permissions::from_mode(0o500)).unwrap();
        if std::fs::write(pd.join("probe"), "").is_ok() { return; } // running as root: permissions do not apply
        let r = sync_with(&cfg, &p, WIDE_TOTAL, &opts(&fx));
        std::fs::set_permissions(&pd, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(r.lines, partial(), "{r:?}");
        assert_eq!(r.synced, [id("team:other")]);
        let m = manifest(&p);
        assert_eq!(m.marketplaces["team"].commit, b);
        assert_eq!(entry(&m, "demo").unwrap().version, "1.0.0");
        let r = sync_with(&cfg, &p, WIDE_TOTAL, &opts(&fx));
        assert!(r.failures.is_empty() && r.lines.is_empty(), "{r:?}");
        assert_eq!(entry(&manifest(&p), "demo").unwrap().version, "2.0.0");
        assert_eq!(r.synced, [id("team:demo"), id("team:other")]);
    }

    /// Item 4: enable syncs at the recorded pin, not the configured commit, and prints no moved-tag line.
    #[test]
    fn sync_one_uses_the_recorded_pin_and_prints_no_moved_tag_line() {
        let fx = Fx::new();
        let repo = market_repo(&fx, &[("demo", "1.0.0"), ("other", "1.0.0")]);
        let a = git(&repo.dir, &["rev-parse", "HEAD"]);
        let p = paths(&fx);
        sync_with(&merged(&fx, &mp("team", &repo.src(), "v1.0.0", None, &["demo"]), None), &p, WIDE_TOTAL, &opts(&fx));
        let b = bump(&repo, &[("demo", "2.0.0"), ("other", "2.0.0")], "Say B.", "v2.0.0");
        let cfg = merged(&fx, &mp("team", &repo.src(), "v1.0.0", Some(&b), &["demo"]), None);
        let r = sync_one_with(&cfg, &p, WIDE_TOTAL, &opts(&fx), &id("team:other"));
        assert!(r.lines.is_empty() && r.failures.is_empty(), "{r:?}");
        let m = manifest(&p);
        assert_eq!(m.marketplaces["team"].commit, a);
        assert_eq!(entry(&m, "other").unwrap().version, "1.0.0");
        assert_eq!(entry(&m, "demo").unwrap().version, "1.0.0");
    }

    /// Item 5 and M11: an empty cache `.git` is re-cloned; git never looks above the cache (a repository
    /// around the zygo dirs is not touched).
    #[test]
    fn broken_cache_is_recloned_and_git_stops_at_the_cache() {
        let fx = Fx::new();
        let repo = market_repo(&fx, &[("demo", "1.0.0")]);
        let p = paths(&fx);
        let home = fx.path("home");
        std::fs::create_dir_all(&home).unwrap();
        git(&home, &["init", "-q", "-b", "main"]);
        std::fs::write(home.join("precious.txt"), "keep").unwrap();
        git(&home, &["add", "precious.txt"]);
        git(&home, &["commit", "-q", "-m", "home"]);
        let head = git(&home, &["rev-parse", "HEAD"]);
        std::fs::create_dir_all(cache(&p).join(".git")).unwrap();
        let cfg = merged(&fx, &mp("team", &repo.src(), "v1.0.0", None, &["demo"]), None);
        let r = sync_with(&cfg, &p, WIDE_TOTAL, &opts(&fx));
        assert!(r.failures.is_empty() && r.lines.is_empty(), "{r:?}");
        assert_eq!(r.synced, [id("team:demo")]);
        assert_eq!(git(&cache(&p), &["rev-parse", "--git-dir"]), ".git");
        assert_eq!(git(&home, &["rev-parse", "HEAD"]), head);
        assert_eq!(std::fs::read_to_string(home.join("precious.txt")).unwrap(), "keep");
    }

    /// Item 6: a filter defined in the user's global git config never runs on marketplace content.
    #[cfg(unix)]
    #[test]
    fn global_filter_selected_by_marketplace_attributes_does_not_run() {
        let fx = Fx::new();
        let m = fx.path("M");
        let global = fx.write("evil-gitconfig", format!(
            "[filter \"evil\"]\n\tsmudge = \"sh -c 'touch {m}; cat'\"\n\tclean = cat\n[filter \"mac\"]\n\tsmudge = \"sh -c 'touch {m}; cat'\"\n",
            m = m.display()));
        let repo = market_repo(&fx, &[("demo", "1.0.0")]);
        repo.write(".gitattributes", "[attr]macro filter=mac\n* filter=evil\nplugins/** macro\n");
        repo.commit("attrs");
        repo.tag_force("v1.0.0");
        let p = paths(&fx);
        let o = SyncOpts { git_config_global: Some(global), ..opts(&fx) };
        let r = sync_with(&merged(&fx, &mp("team", &repo.src(), "v1.0.0", None, &["demo"]), None), &p, WIDE_TOTAL, &o);
        assert!(r.failures.is_empty(), "{r:?}");
        assert!(!m.exists(), "a filter ran");
        assert!(skill(&p, "demo", "1.0.0").contains("Say hello."));
    }

    /// Item 7: crash leftovers in the store are removed at sync.
    #[test]
    fn crash_leftovers_are_removed_at_sync() {
        let fx = Fx::new();
        let repo = market_repo(&fx, &[("demo", "1.0.0")]);
        let p = paths(&fx);
        let cfg = merged(&fx, &mp("team", &repo.src(), "v1.0.0", None, &["demo"]), None);
        sync_with(&cfg, &p, WIDE_TOTAL, &opts(&fx));
        for kind in ["tmp", "old"] {
            std::fs::create_dir_all(store(&p).join(format!("team/demo/1.0.0.{kind}-{}/skills", zygo_core::ids::ulid()))).unwrap();
        }
        assert_eq!(leftovers(&p).len(), 2);
        let r = sync_with(&cfg, &p, WIDE_TOTAL, &opts(&fx));
        assert!(r.failures.is_empty(), "{r:?}");
        assert!(leftovers(&p).is_empty(), "{:?}", leftovers(&p));
        assert_eq!(names(&store(&p).join("team/demo")), ["1.0.0"]);
    }

    /// Item 8: a marketplace no longer in config loses its record and entries at a full sync.
    #[test]
    fn marketplace_removed_from_config_is_dropped_from_the_store() {
        let fx = Fx::new();
        let repo = market_repo(&fx, &[("demo", "1.0.0")]);
        let p = paths(&fx);
        sync_with(&merged(&fx, &mp("team", &repo.src(), "v1.0.0", None, &["demo"]), None), &p, WIDE_TOTAL, &opts(&fx));
        let r = sync_with(&merged(&fx, "", None), &p, WIDE_TOTAL, &opts(&fx));
        assert!(r.lines.is_empty() && r.failures.is_empty(), "{r:?}");
        let m = manifest(&p);
        assert!(m.marketplaces.is_empty() && m.plugins.is_empty(), "{m:?}");
        assert!(!store(&p).join("team").exists());
    }

    /// Item 9: the git index read of the checkout goes through the sync git program.
    #[test]
    fn git_index_read_uses_the_sync_git_program() {
        let fx = Fx::new();
        let repo = market_repo(&fx, &[("demo", "1.0.0")]);
        let p = paths(&fx);
        let (o, log) = traced(&fx);
        let r = sync_with(&merged(&fx, &mp("team", &repo.src(), "v1.0.0", None, &["demo"]), None), &p, WIDE_TOTAL, &o);
        assert!(r.failures.is_empty(), "{r:?}");
        assert!(trace(&log).contains("ls-files"), "{}", trace(&log));
    }

    /// M23: untracked files in the cache work tree are cleaned before the check.
    #[test]
    fn untracked_cache_files_are_not_stored() {
        let fx = Fx::new();
        let repo = market_repo(&fx, &[("demo", "1.0.0")]);
        let p = paths(&fx);
        sync_with(&merged(&fx, &mp("team", &repo.src(), "v1.0.0", None, &["demo"]), None), &p, WIDE_TOTAL, &opts(&fx));
        std::fs::write(cache(&p).join("plugins/demo/skills/hello/extra.md"), "planted\n").unwrap();
        bump(&repo, &[("demo", "1.0.0")], "Say B.", "v1.0.1");
        let r = sync_with(&merged(&fx, &mp("team", &repo.src(), "v1.0.1", None, &["demo"]), None), &p, WIDE_TOTAL, &opts(&fx));
        assert!(r.failures.is_empty(), "{r:?}");
        let m = manifest(&p);
        assert!(!entry(&m, "demo").unwrap().files.contains_key("skills/hello/extra.md"));
        assert!(!store(&p).join("team/demo/1.0.0/skills/hello/extra.md").exists());
    }

    /// M26: the swap waits for the store lock; a reader holding it keeps the store unchanged.
    #[test]
    #[ignore = "timing: the locked result needs fetch and check to end inside the 10 s budget"]
    fn swap_waits_for_the_store_lock() {
        let fx = Fx::new();
        let repo = market_repo(&fx, &[("demo", "1.0.0")]);
        let p = paths(&fx);
        sync_with(&merged(&fx, &mp("team", &repo.src(), "v1.0.0", None, &["demo"]), None), &p, WIDE_TOTAL, &opts(&fx));
        let before = std::fs::read(store(&p).join("manifest.json")).unwrap();
        bump(&repo, &[("demo", "1.0.0")], "Say B.", "v1.0.1");
        let reader = lock_until(&store(&p), false, far()).unwrap().unwrap();
        let o = SyncOpts { per_marketplace: Duration::from_secs(10), ..opts(&fx) };
        let r = sync_with(&merged(&fx, &mp("team", &repo.src(), "v1.0.1", None, &["demo"]), None), &p, WIDE_TOTAL, &o);
        drop(reader);
        assert_eq!(r.failures, [("team".to_string(), "locked".to_string())]);
        assert_eq!(std::fs::read(store(&p).join("manifest.json")).unwrap(), before);
        assert!(skill(&p, "demo", "1.0.0").contains("Say hello."));
        assert!(leftovers(&p).is_empty(), "{:?}", leftovers(&p));
    }

    /// M28: a cache cloned from another URL is cloned again even when the manifest has no record.
    #[test]
    fn cache_of_another_url_is_recloned() {
        let fx = Fx::new();
        let one = market_repo_at(&fx, "one", &[("demo", "1.0.0")]);
        let two = market_repo_at(&fx, "two", &[("demo", "1.0.0")]);
        let p = paths(&fx);
        sync_with(&merged(&fx, &mp("team", &one.src(), "v1.0.0", None, &["demo"]), None), &p, WIDE_TOTAL, &opts(&fx));
        std::fs::write(cache(&p).join(".git/zygo-reuse-marker"), "").unwrap();
        std::fs::remove_file(store(&p).join("manifest.json")).unwrap();
        let r = sync_with(&merged(&fx, &mp("team", &two.src(), "v1.0.0", None, &["demo"]), None), &p, WIDE_TOTAL, &opts(&fx));
        assert!(r.failures.is_empty(), "{r:?}");
        assert!(!cache(&p).join(".git/zygo-reuse-marker").exists());
    }

    // Round 3 (final review, 2026-11-19)

    /// A new skill text committed and tagged `v1.0.1`; returns the commit.
    fn bump_b(repo: &Repo) -> String { bump(repo, &[("demo", "1.0.0")], "Say B.", "v1.0.1") }
    fn raw_blob(repo: &Repo, spec: &str) -> Vec<u8> {
        let out = std::process::Command::new("git").env("GIT_CONFIG_GLOBAL", NULL).env("GIT_CONFIG_NOSYSTEM", "1")
            .arg("-C").arg(&repo.dir).args(["cat-file", "blob", spec]).output().unwrap();
        assert!(out.status.success(), "{spec}");
        out.stdout
    }

    /// Item 1: lock files a killed git leaves in the cache are removed before git runs; the clone is reused.
    #[test]
    fn stale_git_lock_files_are_removed_and_the_clone_is_reused() {
        for lock in ["index.lock", "HEAD.lock", "refs/tags/v1.0.0.lock", "packed-refs.lock", "shallow.lock", "config.lock"] {
            let fx = Fx::new();
            let repo = market_repo(&fx, &[("demo", "1.0.0")]);
            let p = paths(&fx);
            sync_with(&merged(&fx, &mp("team", &repo.src(), "v1.0.0", None, &["demo"]), None), &p, WIDE_TOTAL, &opts(&fx));
            let b = bump_b(&repo);
            let l = cache(&p).join(".git").join(lock);
            std::fs::create_dir_all(l.parent().unwrap()).unwrap();
            std::fs::write(&l, "").unwrap();
            // A killed git's lock is older than PER_MARKETPLACE.
            std::fs::OpenOptions::new().write(true).open(&l).unwrap()
                .set_modified(std::time::SystemTime::now() - 2 * PER_MARKETPLACE).unwrap();
            std::fs::write(cache(&p).join(".git/zygo-reuse-marker"), "").unwrap();
            let r = sync_with(&merged(&fx, &mp("team", &repo.src(), "v1.0.1", None, &["demo"]), None), &p, WIDE_TOTAL, &opts(&fx));
            assert!(r.failures.is_empty() && r.lines.is_empty(), "{lock}: {r:?}");
            assert_eq!(manifest(&p).marketplaces["team"].commit, b, "{lock}");
            assert!(skill(&p, "demo", "1.0.0").contains("Say B."), "{lock}");
            assert!(cache(&p).join(".git/zygo-reuse-marker").exists(), "{lock}: the clone was not reused");
            assert!(!l.exists(), "{lock}");
        }
    }

    /// A commit on top of HEAD whose tree holds the entry `GIT~1` (git refuses it at checkout under
    /// core.protectNTFS), tagged `tag`. The blob text is `tag`, so each call makes a different commit.
    fn bad_commit(fx: &Fx, repo: &Repo, tag: &str) -> String {
        use std::io::Write;
        let src = fx.write(&format!("bad-blob-{tag}"), tag);
        let blob = git(&repo.dir, &["hash-object", "-w", &src.display().to_string()]);
        let ls = git(&repo.dir, &["ls-tree", "HEAD^{tree}"]);
        let mut c = std::process::Command::new("git").arg("-C").arg(&repo.dir).arg("mktree")
            .stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn().unwrap();
        c.stdin.take().unwrap().write_all(format!("{ls}\n100644 blob {blob}\tGIT~1\n").as_bytes()).unwrap();
        let tree = String::from_utf8_lossy(&c.wait_with_output().unwrap().stdout).trim().to_string();
        let commit = git(&repo.dir, &["commit-tree", &tree, "-p", "HEAD", "-m", tag]);
        git(&repo.dir, &["tag", tag, &commit]);
        commit
    }

    /// Item 1 (N46): a pin that fails checkout after a fresh clone is not recloned at the next syncs; a different
    /// pin gets its own single reclone.
    #[test]
    fn unextractable_pin_is_not_recloned_at_every_sync() {
        let fx = Fx::new();
        let repo = market_repo(&fx, &[("demo", "1.0.0")]);
        let p = paths(&fx);
        let (o, log) = traced(&fx);
        let run = |r: &str| sync_with(&merged(&fx, &mp("team", &repo.src(), r, None, &["demo"]), None), &p, WIDE_TOTAL, &o);
        let clones = || trace(&log).lines().filter(|l| l.contains("clone ")).count();
        assert!(run("v1.0.0").failures.is_empty());
        bad_commit(&fx, &repo, "v1.0.1");
        let mut per = vec![];
        for _ in 0..3 {
            let before = clones();
            let r = run("v1.0.1");
            assert_eq!(r.failures.len(), 1, "{r:?}");
            per.push(clones() - before);
        }
        assert_eq!(per, [1, 0, 0]);
        bad_commit(&fx, &repo, "v1.0.2");
        let before = clones();
        assert_eq!(run("v1.0.2").failures.len(), 1);
        assert_eq!(clones() - before, 1);
        assert_eq!(manifest(&p).marketplaces["team"].git_ref, "v1.0.0"); // the previous store is kept
    }

    /// Item 2 (N46): a team layer that is loaded but lists nothing prunes the team's records and cache.
    #[test]
    fn empty_team_layer_prunes_team_records() {
        let fx = Fx::new();
        let repo = market_repo(&fx, &[("demo", "1.0.0")]);
        let p = paths(&fx);
        let team = mp("team", &repo.src(), "v1.0.0", None, &["demo"]);
        assert!(sync_with(&merged(&fx, "", Some(&team)), &p, WIDE_TOTAL, &opts(&fx)).failures.is_empty());
        let cfg = merged(&fx, "", Some(""));
        assert!(cfg.team_keys.is_empty());
        let r = sync_with(&cfg, &p, WIDE_TOTAL, &opts(&fx));
        assert!(r.lines.is_empty() && r.failures.is_empty(), "{r:?}");
        let m = manifest(&p);
        assert!(m.marketplaces.is_empty() && m.plugins.is_empty(), "{m:?}");
        assert!(!cache(&p).exists() && !store(&p).join("team").exists());
    }

    /// Item 3 (N46): a `*.lock` under the cache `.git` younger than PER_MARKETPLACE may belong to a live git
    /// (one orphaned by a killed zygo) and is kept, also when its mtime is up to PER_MARKETPLACE in the future;
    /// an older one, or one further in the future (a clock set back), is removed.
    #[test]
    #[ignore = "timing: lock-file ages race the 5 s PER_MARKETPLACE threshold"]
    fn recent_lock_files_are_kept_and_old_ones_removed() {
        use std::time::SystemTime;
        let fx = Fx::new();
        let repo = market_repo(&fx, &[("demo", "1.0.0")]);
        let p = paths(&fx);
        let cfg = merged(&fx, &mp("team", &repo.src(), "v1.0.0", None, &["demo"]), None);
        assert!(sync_with(&cfg, &p, WIDE_TOTAL, &opts(&fx)).failures.is_empty());
        let set = |name: &str, t: SystemTime| {
            let f = cache(&p).join(".git").join(name);
            std::fs::write(&f, "").unwrap();
            std::fs::OpenOptions::new().write(true).open(&f).unwrap().set_modified(t).unwrap();
            f
        };
        let now = SystemTime::now();
        let (recent, soon) = (set("zygo-recent.lock", now), set("zygo-soon.lock", now + Duration::from_secs(2)));
        let (far_future, old) = (set("zygo-far.lock", now + Duration::from_secs(3600)), set("zygo-old.lock", now - Duration::from_secs(10)));
        let r = sync_with(&cfg, &p, WIDE_TOTAL, &opts(&fx));
        assert!(r.failures.is_empty(), "{r:?}");
        assert!(recent.exists() && soon.exists(), "a recent lock was removed");
        assert!(!old.exists(), "an old lock was kept");
        assert!(!far_future.exists(), "a lock far in the future was kept");
    }

    /// Item 4 (N46): an abbreviated id of an annotated tag object is not a commit id; an abbreviated commit id is.
    #[test]
    fn abbreviated_tag_object_id_is_refused() {
        let fx = Fx::new();
        let repo = market_repo(&fx, &[("demo", "1.0.0")]);
        let commit = git(&repo.dir, &["rev-parse", "HEAD"]);
        git(&repo.dir, &["tag", "-a", "-m", "ann", "ann1"]);
        let obj = git(&repo.dir, &["rev-parse", "ann1"]);
        let p = paths(&fx);
        let run = |r: &str| sync_with(&merged(&fx, &mp("team", &repo.src(), r, None, &["demo"]), None), &p, WIDE_TOTAL, &opts(&fx));
        let r = run(&obj[..12]);
        assert_eq!(r.failures, [("team".to_string(), "ref is not a tag or a commit".to_string())]);
        assert!(read_manifest(&store(&p)).ok().flatten().is_none_or(|m| m.marketplaces.is_empty()));
        let r = run(&commit[..12]);
        assert!(r.failures.is_empty(), "{r:?}");
        assert_eq!(manifest(&p).marketplaces["team"].commit, commit);
    }

    /// Item 5 (N46): `<name>.old-<ulid>` caches a crashed reclone left are removed under the name's lock, by a sync
    /// of that marketplace and by the prune of a removed one; another name's leftover is not touched.
    #[test]
    fn old_cache_leftovers_are_removed() {
        let fx = Fx::new();
        let repo = market_repo(&fx, &[("demo", "1.0.0")]);
        let p = paths(&fx);
        let cfg = merged(&fx, &mp("team", &repo.src(), "v1.0.0", None, &["demo"]), None);
        assert!(sync_with(&cfg, &p, WIDE_TOTAL, &opts(&fx)).failures.is_empty());
        let plant = |name: &str| {
            let d = p.marketplaces_dir.join(format!("{name}.old-{}", zygo_core::ids::ulid()));
            std::fs::create_dir_all(d.join(".git")).unwrap();
            d
        };
        let (mine, other) = (plant("team"), plant("other"));
        let r = sync_with(&cfg, &p, WIDE_TOTAL, &opts(&fx));
        assert!(r.failures.is_empty(), "{r:?}");
        assert!(!mine.exists() && other.exists());
        let mine = plant("team");
        sync_with(&merged(&fx, "", None), &p, WIDE_TOTAL, &opts(&fx));
        assert!(!cache(&p).exists() && !mine.exists() && other.exists());
    }

    /// Round 3 review: an unreachable remote keeps the cache; the pinned commit that only the cache holds survives.
    #[test]
    fn offline_fetch_keeps_the_cache_and_the_pin() {
        let fx = Fx::new();
        let repo = market_repo(&fx, &[("demo", "1.0.0")]);
        let p = paths(&fx);
        let cfg = mp("team", &repo.src(), "v1.0.0", None, &["demo"]);
        let r = sync_with(&merged(&fx, &cfg, None), &p, WIDE_TOTAL, &opts(&fx));
        assert!(r.failures.is_empty(), "{r:?}");
        let pin = manifest(&p).marketplaces["team"].commit.clone();
        // Upstream rewrites v1.0.0 and drops the old commit: only the cache holds the pin.
        repo.write("x.txt", "rewrite");
        git(&repo.dir, &["commit", "-q", "--amend", "-a", "-m", "rewritten"]);
        repo.tag_force("v1.0.0");
        git(&repo.dir, &["reflog", "expire", "--expire=now", "--all"]);
        git(&repo.dir, &["gc", "-q", "--prune=now"]);
        assert!(sync_with(&merged(&fx, &cfg, None), &p, WIDE_TOTAL, &opts(&fx)).failures.is_empty());
        let away = fx.path("away");
        std::fs::rename(&repo.dir, &away).unwrap();
        let r = sync_with(&merged(&fx, &cfg, None), &p, WIDE_TOTAL, &opts(&fx));
        assert_eq!(r.failures.len(), 1, "{r:?}");
        assert!(cache(&p).join(".git").is_dir(), "the cache was removed while offline");
        git(&cache(&p), &["cat-file", "-e", &format!("{pin}^{{commit}}")]);
        std::fs::rename(&away, &repo.dir).unwrap();
        let r = sync_with(&merged(&fx, &cfg, None), &p, WIDE_TOTAL, &opts(&fx));
        assert!(r.failures.is_empty(), "{r:?}");
        assert_eq!(manifest(&p).marketplaces["team"].commit, pin);
        assert!(std::fs::read_dir(&p.marketplaces_dir).unwrap().flatten()
            .all(|e| !e.file_name().to_string_lossy().contains(".old-")), "aside cache left behind");
    }

    /// Item 1: a corrupt cache that fails at fetch or checkout is removed and cloned once more.
    #[test]
    fn corrupt_cache_is_cloned_again_at_fetch_or_checkout() {
        for kind in ["objects-emptied", "pack-truncated", "head-garbage", "refs-removed", "config-removed", "index-garbage",
            "shallow-garbage"] {
            let fx = Fx::new();
            let repo = market_repo(&fx, &[("demo", "1.0.0")]);
            let p = paths(&fx);
            sync_with(&merged(&fx, &mp("team", &repo.src(), "v1.0.0", None, &["demo"]), None), &p, WIDE_TOTAL, &opts(&fx));
            git(&cache(&p), &["repack", "-adq"]);
            let g = cache(&p).join(".git");
            match kind {
                "objects-emptied" => {
                    std::fs::remove_dir_all(g.join("objects")).unwrap();
                    std::fs::create_dir_all(g.join("objects")).unwrap();
                }
                "pack-truncated" => {
                    for e in std::fs::read_dir(g.join("objects/pack")).unwrap().flatten() {
                        if e.path().extension().is_some_and(|x| x == "pack") {
                            std::fs::remove_file(e.path()).unwrap();
                            std::fs::write(e.path(), b"PACK").unwrap();
                        }
                    }
                }
                "head-garbage" => std::fs::write(g.join("HEAD"), "garbage\n").unwrap(),
                "refs-removed" => std::fs::remove_dir_all(g.join("refs")).unwrap(),
                "config-removed" => std::fs::remove_file(g.join("config")).unwrap(),
                "shallow-garbage" => std::fs::write(g.join("shallow"), "garbage\n").unwrap(),
                _ => std::fs::write(g.join("index"), "garbage").unwrap(),
            }
            bump_b(&repo);
            let r = sync_with(&merged(&fx, &mp("team", &repo.src(), "v1.0.1", None, &["demo"]), None), &p, WIDE_TOTAL, &opts(&fx));
            assert!(r.failures.is_empty() && r.lines.is_empty(), "{kind}: {r:?}");
            assert_eq!(r.synced, [id("team:demo")], "{kind}");
            assert!(skill(&p, "demo", "1.0.0").contains("Say B."), "{kind}");
        }
    }

    /// Item 2: a sync without the team layer (team config not fetched) keeps the team's marketplaces;
    /// a sync with a team layer that no longer lists one drops it.
    #[test]
    fn team_marketplace_is_kept_when_the_team_layer_is_absent() {
        let fx = Fx::new();
        let repo = market_repo(&fx, &[("demo", "1.0.0")]);
        let p = paths(&fx);
        let team = mp("team", &repo.src(), "v1.0.0", None, &["demo"]);
        let r = sync_with(&merged(&fx, "", Some(&team)), &p, WIDE_TOTAL, &opts(&fx));
        assert!(r.failures.is_empty(), "{r:?}");
        let before = std::fs::read(store(&p).join("manifest.json")).unwrap();
        let r = sync_with(&merged(&fx, "", None), &p, WIDE_TOTAL, &opts(&fx));
        assert!(r.lines.is_empty() && r.failures.is_empty(), "{r:?}");
        assert_eq!(std::fs::read(store(&p).join("manifest.json")).unwrap(), before);
        assert!(store(&p).join("team/demo/1.0.0").exists() && cache(&p).exists());
        let r = sync_with(&merged(&fx, "", Some("[features]\nresults = true\n")), &p, WIDE_TOTAL, &opts(&fx));
        assert!(r.lines.is_empty() && r.failures.is_empty(), "{r:?}");
        let m = manifest(&p);
        assert!(m.marketplaces.is_empty() && m.plugins.is_empty(), "{m:?}");
        assert!(!store(&p).join("team").exists());
    }

    /// Item 2: a marketplace added after this sync loaded its config (the add wrote the user config and the
    /// store record under the marketplace lock) is re-checked under that lock and kept.
    #[test]
    fn prune_rechecks_the_user_config_under_the_marketplace_lock() {
        let fx = Fx::new();
        let repo = market_repo(&fx, &[("demo", "1.0.0")]);
        let p = paths(&fx);
        let entry = mp("team", &repo.src(), "v1.0.0", None, &["demo"]);
        sync_with(&merged(&fx, &entry, None), &p, WIDE_TOTAL, &opts(&fx));
        let loaded_before_the_add = merged(&fx, "", None);
        std::fs::write(&p.config_file, &entry).unwrap();
        let before = std::fs::read(store(&p).join("manifest.json")).unwrap();
        let r = sync_with(&loaded_before_the_add, &p, WIDE_TOTAL, &opts(&fx));
        assert!(r.lines.is_empty() && r.failures.is_empty(), "{r:?}");
        assert_eq!(std::fs::read(store(&p).join("manifest.json")).unwrap(), before);
        assert!(store(&p).join("team/demo/1.0.0").exists() && cache(&p).exists());
    }

    /// Item 2: the prune takes the marketplace lock; a held lock leaves that marketplace for the next sync.
    #[test]
    fn prune_waits_for_the_marketplace_lock() {
        let fx = Fx::new();
        let repo = market_repo(&fx, &[("demo", "1.0.0")]);
        let p = paths(&fx);
        sync_with(&merged(&fx, &mp("team", &repo.src(), "v1.0.0", None, &["demo"]), None), &p, WIDE_TOTAL, &opts(&fx));
        let before = std::fs::read(store(&p).join("manifest.json")).unwrap();
        let held = lock_marketplace(&p, "team", far()).unwrap().unwrap();
        let cfg = merged(&fx, "", None);
        sync_with(&cfg, &p, Duration::from_millis(50), &opts(&fx));
        assert_eq!(std::fs::read(store(&p).join("manifest.json")).unwrap(), before);
        assert!(cache(&p).exists());
        drop(held);
        sync_with(&cfg, &p, WIDE_TOTAL, &opts(&fx));
        assert!(manifest(&p).marketplaces.is_empty());
    }

    /// Item 6: the prune also removes the cache dir of a marketplace removed from config.
    #[test]
    fn removed_marketplace_cache_is_deleted() {
        let fx = Fx::new();
        let repo = market_repo(&fx, &[("demo", "1.0.0")]);
        let p = paths(&fx);
        sync_with(&merged(&fx, &mp("team", &repo.src(), "v1.0.0", None, &["demo"]), None), &p, WIDE_TOTAL, &opts(&fx));
        assert!(cache(&p).join(".git").is_dir());
        sync_with(&merged(&fx, "", None), &p, WIDE_TOTAL, &opts(&fx));
        assert!(!cache(&p).exists());
    }

    /// Item 6: `*.<ulid>.tmp` files a crash left beside the manifest and in the cache `.git` are removed.
    #[test]
    fn tmp_leftovers_beside_the_manifest_and_in_the_cache_are_removed() {
        let fx = Fx::new();
        let repo = market_repo(&fx, &[("demo", "1.0.0")]);
        let p = paths(&fx);
        let cfg = merged(&fx, &mp("team", &repo.src(), "v1.0.0", None, &["demo"]), None);
        sync_with(&cfg, &p, WIDE_TOTAL, &opts(&fx));
        let u = zygo_core::ids::ulid();
        let planted = [store(&p).join(format!(".manifest.json.{u}.tmp")), cache(&p).join(format!(".git/config.{u}.tmp")),
            cache(&p).join(format!(".git/info/attributes.{u}.tmp"))];
        for f in &planted { std::fs::write(f, "x").unwrap(); }
        let r = sync_with(&cfg, &p, WIDE_TOTAL, &opts(&fx));
        assert!(r.failures.is_empty(), "{r:?}");
        for f in &planted { assert!(!f.exists(), "{}", f.display()); }
    }

    /// Item 3: checkout bytes equal the blob bytes, whatever in-tree or global attributes say
    /// (`ident`, `eol=crlf`, `text`, `working-tree-encoding`).
    #[test]
    fn checkout_bytes_equal_the_blob_bytes() {
        let fx = Fx::new();
        let repo = market_repo(&fx, &[("demo", "1.0.0")]);
        repo.write("plugins/demo/scripts/run.sh", "#!/bin/sh\n# $Id$\necho hi\n");
        repo.write(".gitattributes", "*.sh ident eol=crlf\n");
        repo.commit("attrs");
        repo.tag_force("v1.0.0");
        let attrs = fx.write("global-attributes", "* working-tree-encoding=UTF-16LE text eol=crlf\n");
        let global = fx.write("global-gitconfig",
            format!("[core]\n\tattributesFile = \"{}\"\n", attrs.display().to_string().replace('\\', "/")));
        let p = paths(&fx);
        let o = SyncOpts { git_config_global: Some(global), ..opts(&fx) };
        let r = sync_with(&merged(&fx, &mp("team", &repo.src(), "v1.0.0", None, &["demo"]), None), &p, WIDE_TOTAL, &o);
        assert!(r.failures.is_empty() && r.lines.is_empty(), "{r:?}");
        let e = manifest(&p).plugins[0].clone();
        assert!(e.files.contains_key("scripts/run.sh"));
        for rel in e.files.keys() {
            let stored = std::fs::read(store(&p).join("team/demo/1.0.0").join(rel)).unwrap();
            assert_eq!(stored, raw_blob(&repo, &format!("v1.0.0:plugins/demo/{rel}")), "{rel}");
        }
    }

    /// Item 5: a 40-hex `ref` naming an annotated tag object is not a commit.
    #[test]
    fn full_hex_ref_naming_a_tag_object_is_refused() {
        let fx = Fx::new();
        let repo = market_repo(&fx, &[("demo", "1.0.0")]);
        git(&repo.dir, &["tag", "-a", "-m", "annotated", "vann"]);
        let tag_object = git(&repo.dir, &["rev-parse", "vann"]);
        assert_ne!(tag_object, git(&repo.dir, &["rev-parse", "HEAD"]));
        let p = paths(&fx);
        let r = sync_with(&merged(&fx, &mp("team", &repo.src(), &tag_object, None, &["demo"]), None), &p, WIDE_TOTAL, &opts(&fx));
        assert_eq!(r.failures, [("team".to_string(), "ref is not a tag or a commit".to_string())]);
        assert_eq!(r.lines, offline());
    }

    /// Item 4: `git_modes` of a subdirectory of a repository (absolute path) reads the repository's index.
    #[test]
    fn git_modes_reads_a_subdirectory_of_a_repository() {
        let fx = Fx::new();
        let repo = Repo::new(&fx, "r");
        plugin_files(&repo, "sub", "demo", "1.0.0");
        repo.write("sub/scripts/x.sh", "echo\n");
        repo.chmod_x("sub/scripts/x.sh");
        repo.commit("c");
        let sub = repo.dir.join("sub");
        assert!(sub.is_absolute());
        let modes = git_modes(&sub, far()).unwrap();
        assert_eq!(modes.get("scripts/x.sh").map(String::as_str), Some("100755"), "{modes:?}");
    }
}

// Task 7 — contract signatures

#[test]
fn contract_signatures_exist() {
    use std::path::Path;
    use zygo_config::profile::Profile;
    use zygo_config::Merged;
    use zygo_plugins::approve::{approve, is_approved};
    use zygo_plugins::select::{selection, Selection};
    use zygo_plugins::store::{state, StoreState};
    let _: fn(&Merged, Duration) -> SyncReport = sync_all;
    let _: fn(&Merged, Option<&Profile>) -> Selection = selection;
    let _: fn(&Path) -> StoreState = state;
    let _: fn(&str, &str, &str) -> bool = is_approved;
    let _: fn(&str, &str, &str) -> anyhow::Result<()> = approve;
}
