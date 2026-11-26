//! Plugin snapshot, embedded plugin and list (spec §15.3, §15.5). No environment mutation, no network, no sleeps.

mod common;

use std::ffi::OsStr;
use std::path::PathBuf;
use std::time::Instant;

use common::*;
use zygo_core::verbs::VERB_SPECS;
use zygo_core::Agent;
use zygo_plugins::approve::{approve_in, PreApproval};
use zygo_plugins::check::check_dir;
use zygo_plugins::embedded;
use zygo_plugins::select::selection;
use zygo_plugins::snapshot::{command_found, delivered_in};
use zygo_plugins::store::{lock_until, read_manifest, read_verified, write_manifest};

fn file(name: &str) -> String {
    let f = embedded::files();
    let (_, b) = f.iter().find(|(n, _)| n == name).unwrap_or_else(|| panic!("{name} missing"));
    String::from_utf8(b.clone()).unwrap()
}

#[test]
fn embedded_has_one_command_per_verb() {
    assert_eq!(VERB_SPECS.len(), 19);
    for v in VERB_SPECS {
        let t = file(&format!("commands/{}.md", v.name));
        assert!(t.contains("disable-model-invocation: true"), "{}", v.name);
        assert!(t.contains("description: \""), "{}", v.name);
        if v.name != "down" && v.name != "mail" {
            assert!(t.contains(&format!("zygo {} $ARGUMENTS", v.name)), "{}", v.name);
        }
    }
    assert_eq!(embedded::files().iter().filter(|(n, _)| n.starts_with("commands/")).count(), 19);
    assert!(file("commands/compact.md").contains("argument-hint: \"<tokens|on|off>\""));
    assert!(file("commands/statusline.md").contains("argument-hint: \"[compact|verbose]\""));
    assert!(!file("commands/status.md").contains("argument-hint"));
    let down = file("commands/down.md");
    // W§12, OP11: `/zygo:down` takes only `--now`; the CLI flags of `VerbSpec.args` are not slash arguments.
    assert!(down.contains("argument-hint: \"[--now]\"\n"), "{down}");
    assert!(!down.contains("--timeout"), "{down}");
    assert!(down.contains("disable-model-invocation: true"), "{down}");
    assert!(down.contains("If this text reaches you, /zygo:down did not run: tell the user that it failed. Do not run zygo down in a shell: it stops the workspace of the current folder, not the workspace of this session."), "{down}");
    assert!(!down.contains("$ARGUMENTS"), "{down}");
    let mail = file("commands/mail.md");
    // M§8: `/zygo:mail` lists only; the fallback runs only `zygo mail ls`, and send and read stay MCP tools.
    assert!(mail.contains("argument-hint: \"[ls [<name>]]\"\n"), "{mail}");
    assert!(mail.contains("zygo mail ls $ARGUMENTS"), "{mail}");
    assert!(!mail.contains("zygo mail $ARGUMENTS"), "{mail}");
    assert!(mail.contains("send and read are MCP tools"), "{mail}");
    assert!(!file("commands/ls.md").contains("argument-hint"));
    assert!(!file("commands/workspace.md").contains("argument-hint"));
}

#[test]
fn embedded_plugin_passes_the_check_and_has_no_code() {
    let fx = Fx::new();
    for (n, b) in embedded::files() { fx.write(&n, b); }
    let r = check_dir(&fx.0);
    assert!(r.ok(), "{}", r.render());
    assert_eq!(r.plugins[0].name, "zygo");
    assert_eq!(r.plugins[0].digest, None);
    assert!(r.plugins[0].code.is_empty());
    assert!(r.plugins[0].copied.iter().any(|c| c == "skills/zygo-cli/SKILL.md"));
}

#[test]
fn embedded_skill_covers_the_cli() {
    let t = file("skills/zygo-cli/SKILL.md").to_lowercase();
    assert!(t.contains("never state or guess the context window"));
    for c in ["zygo compact", "zygo status", "zygo work", "zygo usage", "zygo marketplace", "zygo plugins", "zygo statusline"] {
        assert!(t.contains(c), "{c}");
    }
    assert!(t.contains("name: zygo-cli"));
}

// ---- Task 2: delivery gate and Claude Code snapshot ----

const CMD: &str = "---\ndescription: Go\n---\nGo.\n";
const AGENT: &str = "---\nname: rev\ndescription: Reviews\n---\nReview.\n";

fn code(name: &'static str) -> PluginSpec { PluginSpec::new(name).file("scripts/run.sh", "echo one\n") }
fn approve_line(p: &str) -> String { format!("plugin team:{p}: code needs approval; run zygo plugins approve team:{p}") }
fn empty_dir(fx: &Fx, name: &str) -> PathBuf { let d = fx.path(name); std::fs::create_dir_all(&d).unwrap(); d }
fn user(fx: &Fx, plugins: &[&str]) -> String { mp("team", &origin_src(fx), "v1.0.0", None, plugins) }
fn mcp(servers: serde_json::Value) -> String { serde_json::json!({ "mcpServers": servers }).to_string() }
fn keys(path: &std::path::Path) -> Vec<String> {
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let m = v.get("mcpServers").unwrap_or(&v);
    m.as_object().unwrap().keys().cloned().collect()
}

#[test]
fn claude_snapshot_has_plugin_dirs_and_the_embedded_plugin() {
    let fx = Fx::new();
    let demo = PluginSpec::new("demo").file("commands/go.md", CMD).file("agents/rev.md", AGENT);
    let (paths, cfg) = stored(&fx, &[demo], "");
    let session = fx.path("session");
    let s = snap(&paths, &session, Agent::ClaudeCode, &selection(&cfg, None), None).unwrap();
    let mut want: std::collections::BTreeSet<String> = embedded::files().into_iter().map(|(n, _)| format!("plugins/zygo/{n}")).collect();
    assert_eq!(want.len(), 21);
    want.extend(["plugins/demo/.claude-plugin/plugin.json", "plugins/demo/skills/hello/SKILL.md", "plugins/demo/commands/go.md", "plugins/demo/agents/rev.md"].map(String::from));
    assert_eq!(tree(&session), want);
    assert_eq!(s.plugins.len(), 2);
    assert_eq!((s.plugins[0].name.as_str(), s.plugins[0].marketplace.as_str()), ("zygo", "embedded"));
    let d = &s.plugins[1];
    assert_eq!((d.marketplace.as_str(), d.name.as_str()), ("team", "demo"));
    assert_eq!((d.skills.clone(), d.commands.clone(), d.agents.clone(), d.mcp.clone()), (vec!["hello".to_string()], vec!["go".to_string()], vec!["rev".to_string()], vec![]));
    assert_eq!(s.plugin_dirs(), vec![session.join("plugins/zygo"), session.join("plugins/demo")]);
    assert!(s.lines.is_empty() && s.details.is_empty());
}

#[test]
fn code_plugin_waits_for_approval_and_a_changed_script_needs_a_new_one() {
    let fx = Fx::new();
    let (paths, cfg) = stored(&fx, &[code("demo")], "");
    let sel = selection(&cfg, None);
    let s = snap(&paths, &fx.path("s1"), Agent::ClaudeCode, &sel, None).unwrap();
    assert!(!fx.path("s1/plugins/demo").exists());
    assert_eq!(s.lines, vec![approve_line("demo")]);
    let d1 = digest(&paths, "team", "demo");
    approve_in(&paths.approvals_file, "team", "demo", &d1).unwrap();
    let s = snap(&paths, &fx.path("s2"), Agent::ClaudeCode, &sel, None).unwrap();
    assert!(fx.path("s2/plugins/demo/scripts/run.sh").is_file());
    assert!(s.lines.is_empty());

    let origin = Repo { dir: fx.path("origin") };
    origin.write("plugins/demo/scripts/run.sh", "echo onf\n");
    origin.commit("second");
    origin.tag("v1.0.1");
    let cfg = merged(&fx, &mp("team", &origin.src(), "v1.0.1", None, &["demo"]), None);
    let rep = zygo_plugins::sync::sync_with(&cfg, &paths, WIDE_TOTAL, &opts(&fx));
    assert!(rep.failures.is_empty(), "{:?}", rep.failures);
    let d2 = digest(&paths, "team", "demo");
    assert_ne!(d1, d2);
    let sel = selection(&cfg, None);
    let s = snap(&paths, &fx.path("s3"), Agent::ClaudeCode, &sel, None).unwrap();
    assert!(!fx.path("s3/plugins/demo").exists());
    assert_eq!(s.lines, vec![approve_line("demo")]);
    approve_in(&paths.approvals_file, "team", "demo", &d2).unwrap();
    snap(&paths, &fx.path("s4"), Agent::ClaudeCode, &sel, None).unwrap();
    assert!(fx.path("s4/plugins/demo/scripts/run.sh").is_file());
}

#[test]
fn team_pre_approval_delivers_without_the_approvals_file() {
    let fx = Fx::new();
    let (paths, _) = stored(&fx, &[code("demo")], "");
    let d1 = digest(&paths, "team", "demo");
    let team = mp("team", &origin_src(&fx), "v1.0.0", None, &["demo"]) + &format!("approvals = [{{ plugin = \"demo\", digest = \"{d1}\" }}]\n");
    let cfg = merged(&fx, "", Some(&team));
    let sel = selection(&cfg, None);
    let s = snap(&paths, &fx.path("session"), Agent::ClaudeCode, &sel, None).unwrap();
    assert!(fx.path("session/plugins/demo/scripts/run.sh").is_file());
    assert!(s.lines.is_empty());
    assert!(!paths.approvals_file.exists());
}

#[test]
fn builtin_style_pre_approval_delivers_only_on_an_exact_triple() {
    let fx = Fx::new();
    let r = build_market(&fx, "act101-repo", &[code("act-lite")]);
    let builtin: toml::Table = mp("act101", &r.src(), "v1.0.0", None, &["act-lite"]).parse().unwrap();
    let paths = paths(&fx);
    std::fs::create_dir_all(paths.config_file.parent().unwrap()).unwrap();
    std::fs::write(&paths.config_file, "").unwrap();
    let cfg = zygo_config::load_with_builtin(&zygo_config::LoadContext { paths: paths.clone(), repo: None, team: None }, builtin).unwrap();
    let rep = zygo_plugins::sync::sync_with(&cfg, &paths, WIDE_TOTAL, &opts(&fx));
    assert!(rep.failures.is_empty(), "{:?}", rep.failures);
    let d = digest(&paths, "act101", "act-lite");
    let pre = |m: &str, p: &str, d: &str| PreApproval { marketplace: m.into(), plugin: p.into(), digest: d.into() };
    let line = "plugin act101:act-lite: code needs approval; run zygo plugins approve act101:act-lite".to_string();
    let base = selection(&cfg, None);
    let mut n = 0;
    for (item, delivered) in [
        (pre("act101", "act-lite", &d), true),
        (pre("act101", "act-lite", &"0".repeat(64)), false),
        (pre("act101", "other", &d), false),
        (pre("team2", "act-lite", &d), false),
    ] {
        let mut sel = base.clone();
        sel.pre_approved.push(item);
        n += 1;
        let session = fx.path(&format!("s{n}"));
        let s = snap(&paths, &session, Agent::ClaudeCode, &sel, None).unwrap();
        assert_eq!(session.join("plugins/act-lite").exists(), delivered, "case {n}");
        assert_eq!(s.lines, if delivered { vec![] } else { vec![line.clone()] }, "case {n}");
    }
    assert!(!paths.approvals_file.exists());
}

#[test]
fn tampered_store_copy_is_not_delivered() {
    let fx = Fx::new();
    let (paths, cfg) = stored(&fx, &[code("demo")], "");
    approve(&paths, "team", "demo");
    let sel = selection(&cfg, None);
    let f = paths.plugin_store.join("team/demo/1.0.0/scripts/run.sh");
    let mut b = std::fs::read(&f).unwrap();
    b.push(b'x');
    std::fs::write(&f, b).unwrap();
    let s = snap(&paths, &fx.path("session"), Agent::ClaudeCode, &sel, None).unwrap();
    assert!(!fx.path("session/plugins/demo").exists());
    assert_eq!(s.lines, vec!["plugins: partial"]);
    assert_eq!(s.details, vec!["plugin team:demo: store copy differs from its manifest; run zygo plugins sync"]);

    // a changed execute bit (unix) is the same condition
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let fx = Fx::new();
        let (paths, cfg) = stored(&fx, &[PluginSpec::new("demo")], "");
        let f = paths.plugin_store.join("team/demo/1.0.0/skills/hello/SKILL.md");
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
        let s = snap(&paths, &fx.path("session"), Agent::ClaudeCode, &selection(&cfg, None), None).unwrap();
        assert!(!fx.path("session/plugins/demo").exists());
        assert_eq!(s.lines, vec!["plugins: partial"]);
        assert_eq!(s.details, vec!["plugin team:demo: store copy differs from its manifest; run zygo plugins sync"]);
    }
}

#[test]
fn entries_not_selected_or_from_another_source_are_not_delivered() {
    let fx = Fx::new();
    let (paths, _) = stored(&fx, &[PluginSpec::new("demo"), PluginSpec::new("other")], "");
    let cfg = merged(&fx, &(user(&fx, &["demo", "other"]) + "[plugins]\ndisable = [\"team:other\"]\n"), None);
    let sel = selection(&cfg, None);
    let s = snap(&paths, &fx.path("s1"), Agent::ClaudeCode, &sel, None).unwrap();
    assert_eq!(s.plugins.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["zygo", "demo"]);
    assert_eq!(delivered_in(&paths, &sel, far()).unwrap().plugins.iter().map(|p| p.id.to_string()).collect::<Vec<_>>(), ["team:demo"]);

    let elsewhere = fx.path("elsewhere").display().to_string();
    let cfg = merged(&fx, &mp("team", &elsewhere, "v1.0.0", None, &["demo"]), None);
    let sel = selection(&cfg, None);
    let s = snap(&paths, &fx.path("s2"), Agent::ClaudeCode, &sel, None).unwrap();
    assert!(!fx.path("s2/plugins/demo").exists());
    assert!(s.lines.is_empty() && s.details.is_empty());
}

#[test]
fn missing_mcp_command_is_left_out_with_one_line() {
    let fx = Fx::new();
    let pj = |name: &str, home: bool| {
        let h = if home { ",\"homepage\":\"https://example.test/nav\"" } else { "" };
        format!(r#"{{"name":"{name}","version":"1.0.0","description":"Demo plugin","license":"MIT"{h}}}"#)
    };
    let servers = serde_json::json!({
        "nav": {"command": "zygo-no-such-cmd", "args": ["serve"]},
        "nav2": {"command": "zygo-no-such-cmd"},
        "ok": {"command": exe()},
        "web": {"type": "http", "url": "https://example.test/mcp"},
    });
    let flat = serde_json::json!({"nav": {"command": "zygo-no-such-cmd"}, "ok": {"command": exe()}}).to_string();
    let demo = PluginSpec::new("demo").file(".claude-plugin/plugin.json", pj("demo", true)).file(".mcp.json", mcp(servers));
    let plain = PluginSpec::new("plain").file(".mcp.json", mcp(serde_json::json!({"nav": {"command": "zygo-no-such-cmd"}})));
    let flat_p = PluginSpec::new("flat").file(".mcp.json", &flat);
    let (paths, cfg) = stored(&fx, &[demo, flat_p, plain], "");
    for p in ["demo", "flat", "plain"] { approve(&paths, "team", p); }
    let sel = selection(&cfg, None);
    let empty = empty_dir(&fx, "empty");
    let s = snap(&paths, &fx.path("session"), Agent::ClaudeCode, &sel, Some(empty.as_os_str())).unwrap();
    // the remaining servers keep the order of the file
    assert_eq!(keys(&fx.path("session/plugins/demo/.mcp.json")), ["ok", "web"]);
    let d = s.plugins.iter().find(|p| p.name == "demo").unwrap();
    assert_eq!(d.mcp, ["ok", "web"]);
    assert_eq!(s.lines, vec![
        "plugin team:demo: zygo-no-such-cmd not found (https://example.test/nav)".to_string(),
        "plugin team:flat: zygo-no-such-cmd not found".to_string(),
        "plugin team:plain: zygo-no-such-cmd not found".to_string(),
    ]);
    assert!(s.details.is_empty());
    let source = std::fs::read(fx.path("origin/plugins/demo/.mcp.json")).unwrap();
    assert_eq!(std::fs::read(paths.plugin_store.join("team/demo/1.0.0/.mcp.json")).unwrap(), source);
    // the top-level map is written back as a top-level map
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(fx.path("session/plugins/flat/.mcp.json")).unwrap()).unwrap();
    assert!(v.get("mcpServers").is_none());
    assert_eq!(v.as_object().unwrap().keys().collect::<Vec<_>>(), ["ok"]);
}

#[test]
fn command_lookup_rules() {
    let fx = Fx::new();
    let root = fx.path("plugin");
    fx.write("plugin/bin/srv", "x");
    fx.write("plugin/bin/plain", "x");
    std::fs::create_dir_all(root.join("bin/adir")).unwrap();
    fx.write("outside", "x");
    let search = fx.path("search");
    std::fs::create_dir_all(search.join("dirbin")).unwrap();
    let fake = if cfg!(windows) { "fake-srv.exe" } else { "fake-srv" };
    fx.write(&format!("search/{fake}"), "x");
    std::fs::create_dir_all(fx.path("search2/fake-srv")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let m = |p: &std::path::Path, mode| std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode)).unwrap();
        m(&root.join("bin/srv"), 0o755);
        m(&root.join("bin/plain"), 0o644);
        m(&fx.path("outside"), 0o755);
        m(&search.join(fake), 0o755);
    }
    let empty = empty_dir(&fx, "empty");
    let found = |c: &str, p: Option<&OsStr>| command_found(c, &root, p);
    assert!(found("${CLAUDE_PLUGIN_ROOT}/bin/srv", None));
    #[cfg(unix)]
    assert!(!found("${CLAUDE_PLUGIN_ROOT}/bin/plain", None));
    assert!(!found("${CLAUDE_PLUGIN_ROOT}/bin/adir", None));
    assert!(!found("${CLAUDE_PLUGIN_ROOT}/../outside", None));
    assert!(found(&exe(), None));
    assert!(!found(&fx.path("search").display().to_string(), None));
    assert!(found("fake-srv", Some(search.as_os_str())));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(search.join(fake), std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(!found("fake-srv", Some(search.as_os_str())));
    }
    assert!(!found("fake-srv", Some(empty.as_os_str())));
    assert!(!found("fake-srv", Some(fx.path("search2").as_os_str())));
}

#[test]
fn same_plugin_name_from_two_marketplaces_is_delivered_once() {
    let fx = Fx::new();
    let (paths, cfg) = stored_multi(&fx, &[("m1", &[PluginSpec::new("demo")]), ("m2", &[PluginSpec::new("demo")])], "");
    let s = snap(&paths, &fx.path("session"), Agent::ClaudeCode, &selection(&cfg, None), None).unwrap();
    assert_eq!(s.details, vec!["plugin m2:demo: name already delivered from m1"]);
    assert_eq!(s.lines, vec!["plugins: partial"]);
    let demos: Vec<_> = s.plugins.iter().filter(|p| p.name == "demo").collect();
    assert_eq!(demos.len(), 1);
    assert_eq!(demos[0].marketplace, "m1");
}

/// A manifest entry `team:zygo` (a copy of `team:demo`), as N30 would refuse to store it.
fn plant_zygo(paths: &zygo_core::paths::Paths) {
    let mut m = read_manifest(&paths.plugin_store).unwrap().unwrap();
    let mut e = m.plugins.iter().find(|e| e.plugin == "demo" && e.marketplace == "team").unwrap().clone();
    e.plugin = "zygo".into();
    copy_tree(&paths.plugin_store.join("team/demo/1.0.0"), &paths.plugin_store.join("team/zygo/1.0.0"));
    m.plugins.push(e);
    write_manifest(&paths.plugin_store, &m).unwrap();
}

#[test]
fn reserved_zygo_name_in_the_manifest_is_never_delivered() {
    let fx = Fx::new();
    let (paths, _) = stored(&fx, &[PluginSpec::new("demo")], "");
    plant_zygo(&paths);
    let cfg = merged(&fx, &user(&fx, &["demo", "zygo"]), None);
    let s = snap(&paths, &fx.path("session"), Agent::ClaudeCode, &selection(&cfg, None), None).unwrap();
    assert_eq!(s.details, vec!["plugin team:zygo: name zygo is reserved"]);
    let pj = std::fs::read(fx.path("session/plugins/zygo/.claude-plugin/plugin.json")).unwrap();
    let want = embedded::files().into_iter().find(|(n, _)| n == ".claude-plugin/plugin.json").unwrap().1;
    assert_eq!(pj, want);
    assert!(s.plugins.iter().all(|p| p.marketplace != "team" || p.name != "zygo"));
}

#[test]
fn opencode_is_not_built_here() {
    let fx = Fx::new();
    let (paths, cfg) = stored(&fx, &[PluginSpec::new("demo")], "");
    let session = fx.path("session");
    let e = snap(&paths, &session, Agent::OpenCode, &selection(&cfg, None), None).unwrap_err();
    assert!(e.to_string().contains("OpenCode adapter"), "{e}");
    assert!(!session.exists());
}

#[test]
fn delivered_in_holds_the_lock_while_the_caller_reads() {
    let fx = Fx::new();
    let (paths, cfg) = stored(&fx, &[code("ok"), code("held")], "");
    approve(&paths, "team", "ok");
    let sel = selection(&cfg, None);
    let delivered = delivered_in(&paths, &sel, far()).unwrap();
    assert_eq!(delivered.plugins.iter().map(|p| p.id.to_string()).collect::<Vec<_>>(), ["team:ok"]);
    assert_eq!(delivered.lines, vec![approve_line("held")]);
    let e = &delivered.plugins[0].entry;
    assert_eq!(read_verified(&paths.plugin_store, e, "scripts/run.sh").unwrap(), b"echo one\n");
    assert!(lock_until(&paths.plugin_store, true, Instant::now()).unwrap().is_none());
    drop(delivered);
    // A git child of a parallel test can hold a copy of the lock fd until it execs, so wait for the lock.
    assert!(lock_until(&paths.plugin_store, true, far()).unwrap().is_some());
}

#[cfg(unix)]
#[test]
fn snapshot_preserves_executable_bits() {
    use std::os::unix::fs::PermissionsExt;
    let fx = Fx::new();
    let demo = PluginSpec::new("demo").file("scripts/run.sh", "echo one\n").exec("scripts/run.sh").file("skills/hello/notes.txt", "n");
    let (paths, cfg) = stored(&fx, &[demo], "");
    approve(&paths, "team", "demo");
    snap(&paths, &fx.path("session"), Agent::ClaudeCode, &selection(&cfg, None), None).unwrap();
    let mode = |r: &str| std::fs::metadata(fx.path(&format!("session/plugins/demo/{r}"))).unwrap().permissions().mode() & 0o777;
    assert_ne!(mode("scripts/run.sh") & 0o111, 0);
    assert_eq!(mode("skills/hello/notes.txt"), 0o644);
}

#[test]
fn busy_store_lock_still_delivers_the_embedded_plugin() {
    let fx = Fx::new();
    let (paths, cfg) = stored(&fx, &[PluginSpec::new("demo")], "");
    let sel = selection(&cfg, None);
    let held = lock_until(&paths.plugin_store, true, far()).unwrap().unwrap();
    let session = fx.path("session");
    let s = zygo_plugins::snapshot::snapshot_in(&paths, &session, Agent::ClaudeCode, &sel, None, Instant::now()).unwrap();
    assert!(session.join("plugins/zygo/skills/zygo-cli/SKILL.md").is_file());
    assert_eq!(s.plugins.len(), 1);
    assert_eq!(s.plugins[0].name, "zygo");
    assert_eq!(s.lines, vec!["plugins: partial"]);
    assert_eq!(s.details, vec!["store lock busy"]);
    let e = delivered_in(&paths, &sel, Instant::now()).expect_err("busy").to_string();
    assert!(e.contains("store lock busy"), "{e}");
    drop(held);
    let s = snap(&paths, &fx.path("s2"), Agent::ClaudeCode, &sel, None).unwrap();
    assert_eq!(s.plugins.len(), 2);
    assert_eq!(delivered_in(&paths, &sel, far()).unwrap().plugins.len(), 1);
}

#[test]
fn launch_lines_are_only_the_spec_lines() {
    let fx = Fx::new();
    let mcpm = PluginSpec::new("mcpm").file(".mcp.json", mcp(serde_json::json!({"nav": {"command": "zygo-no-such-cmd"}})));
    let (paths, _) = stored_multi(&fx, &[
        ("team", &[PluginSpec::new("demo"), mcpm, code("needs"), code("tamper")]),
        ("zz", &[PluginSpec::new("demo")]),
    ], "");
    plant_zygo(&paths);
    approve(&paths, "team", "tamper");
    approve(&paths, "team", "mcpm");
    let f = paths.plugin_store.join("team/tamper/1.0.0/scripts/run.sh");
    let mut b = std::fs::read(&f).unwrap();
    b.push(b'x');
    std::fs::write(&f, b).unwrap();
    let cfg = merged(&fx, &(mp("team", &origin_src(&fx), "v1.0.0", None, &["demo", "mcpm", "needs", "tamper", "zygo"])
        + &mp("zz", &fx.path("origin-zz").display().to_string(), "v1.0.0", None, &["demo"])), None);
    let empty = empty_dir(&fx, "empty");
    let s = snap(&paths, &fx.path("session"), Agent::ClaudeCode, &selection(&cfg, None), Some(empty.as_os_str())).unwrap();
    assert_eq!(s.lines, vec![
        approve_line("needs"),
        "plugin team:mcpm: zygo-no-such-cmd not found".to_string(),
        "plugins: partial".to_string(),
    ]);
    assert_eq!(s.details, vec![
        "plugin team:tamper: store copy differs from its manifest; run zygo plugins sync".to_string(),
        "plugin team:zygo: name zygo is reserved".to_string(),
        "plugin zz:demo: name already delivered from team".to_string(),
    ]);
}

// ---- Task 3: list ----

use zygo_plugins::select::list;

#[test]
fn list_line_format() {
    let fx = Fx::new();
    let demo = PluginSpec::new("demo").file(".mcp.json", mcp(serde_json::json!({"a": {"command": exe()}, "b": {"command": exe()}})));
    let (paths, _) = stored(&fx, &[demo, PluginSpec::new("plain")], "");
    let empty = empty_dir(&fx, "empty");
    let p = Some(empty.as_os_str());
    let cfg = merged(&fx, &user(&fx, &["demo", "plain"]), None);
    // `.mcp.json` servers are code (§15.7), so demo needs approval; plain has none.
    let want = [
        "team:demo 1.0.0 (default) skills: hello · mcp: a, b · code: needs approval · missing: -",
        "team:plain 1.0.0 (default) skills: hello · mcp: - · code: none · missing: -",
    ];
    assert_eq!(list(&cfg, None, &paths, p), want);
    let cfg = merged(&fx, &(user(&fx, &["demo", "plain"]) + "[plugins]\nenable = [\"team:ghost\"]\n"), None);
    assert_eq!(list(&cfg, None, &paths, p), [want[0], "team:ghost (not synced)", want[1]]);
}

#[test]
fn list_shows_needs_approval_changed_missing_and_details() {
    let fx = Fx::new();
    let demo = PluginSpec::new("demo").file(".mcp.json", mcp(serde_json::json!({"nav": {"command": "zygo-no-such-cmd"}})));
    let (paths, cfg) = stored(&fx, &[demo, PluginSpec::new("other")], "");
    let empty = empty_dir(&fx, "empty");
    let p = Some(empty.as_os_str());
    let row = |code: &str| format!("team:demo 1.0.0 (default) skills: hello · mcp: nav · code: {code} · missing: zygo-no-such-cmd");
    let other = "team:other 1.0.0 (optional) skills: hello · mcp: - · code: none · missing: -";
    let cfg_demo = merged(&fx, &user(&fx, &["demo"]), None);
    assert_eq!(list(&cfg, None, &paths, p).first().unwrap(), &row("needs approval"));

    // team pre-approval counts only on the full triple
    let d = digest(&paths, "team", "demo");
    let team = |plugin: &str, digest: &str| mp("team", &origin_src(&fx), "v1.0.0", None, &["demo"])
        + &format!("approvals = [{{ plugin = \"{plugin}\", digest = \"{digest}\" }}]\n");
    for (plugin, dg, want) in [("demo", d.as_str(), "approved"), ("demo", &"0".repeat(64), "needs approval"), ("other", d.as_str(), "needs approval")] {
        let c = merged(&fx, "", Some(&team(plugin, dg)));
        assert_eq!(list(&c, None, &paths, p)[0], row(want), "{plugin} {dg}");
    }
    assert!(!paths.approvals_file.exists());

    approve(&paths, "team", "demo");
    assert_eq!(list(&cfg_demo, None, &paths, p), [row("approved").as_str(), other]);

    // user-disabled default plugin
    let off = merged(&fx, &(user(&fx, &["demo"]) + "[plugins]\ndisable = [\"team:demo\"]\n"), None);
    assert_eq!(list(&off, None, &paths, p)[0], row("approved").replace("(default)", "(disabled)"));

    // an entry of another source is not listed
    let elsewhere = merged(&fx, &mp("team", &fx.path("elsewhere").display().to_string(), "v1.0.0", None, &["demo"]), None);
    assert_eq!(list(&elsewhere, None, &paths, p), ["team:demo (not synced)"]);

    // a reserved name adds its detail line
    plant_zygo(&paths);
    let with_zygo = merged(&fx, &user(&fx, &["demo", "zygo"]), None);
    let l = list(&with_zygo, None, &paths, p);
    assert_eq!(l.last().unwrap(), "  plugin team:zygo: name zygo is reserved");
    assert!(l.iter().any(|x| x.starts_with("team:zygo 1.0.0 (default)")), "{l:?}");

    // a changed store copy
    let f = paths.plugin_store.join("team/demo/1.0.0/.mcp.json");
    let mut b = std::fs::read(&f).unwrap();
    b.push(b' ');
    std::fs::write(&f, b).unwrap();
    let l = list(&cfg_demo, None, &paths, p);
    assert!(l[0].starts_with("team:demo 1.0.0 (default) skills: hello") && l[0].contains("code: changed"), "{l:?}");
    assert_eq!(l.last().unwrap(), "  plugin team:demo: store copy differs from its manifest; run zygo plugins sync");
}

const FORGED: &str = "\nplugin team:demo: forged\u{1b}[2K";
const FORGED_ESC: &str = "\\nplugin team:demo: forged\\u{1b}[2K";

fn forged_demo() -> PluginSpec {
    let pj = serde_json::json!({"name": "demo", "version": "1.0.0", "description": "Demo plugin", "license": "MIT",
        "homepage": format!("https://example.test/nav{FORGED}")}).to_string();
    let servers = serde_json::json!({ format!("nav{FORGED}"): {"command": format!("zygo-no-such-cmd{FORGED}")} });
    PluginSpec::new("demo").file(".claude-plugin/plugin.json", pj).file(".mcp.json", mcp(servers))
}

#[test]
fn list_escapes_server_names_and_missing_commands() {
    let fx = Fx::new();
    let (paths, cfg) = stored(&fx, &[forged_demo()], "");
    let empty = empty_dir(&fx, "empty");
    let l = list(&cfg, None, &paths, Some(empty.as_os_str()));
    assert!(l[0].ends_with(&format!("skills: hello · mcp: nav{FORGED_ESC} · code: needs approval · missing: zygo-no-such-cmd{FORGED_ESC}")), "{l:?}");
    assert!(!l.iter().any(|x| x.contains('\u{1b}') || x.contains('\n')), "{l:?}");
}

#[test]
fn missing_command_line_escapes_command_and_homepage() {
    let fx = Fx::new();
    let (paths, cfg) = stored(&fx, &[forged_demo()], "");
    approve(&paths, "team", "demo");
    let empty = empty_dir(&fx, "empty");
    let s = snap(&paths, &fx.path("session"), Agent::ClaudeCode, &selection(&cfg, None), Some(empty.as_os_str())).unwrap();
    assert_eq!(s.lines, vec![format!("plugin team:demo: zygo-no-such-cmd{FORGED_ESC} not found (https://example.test/nav{FORGED_ESC})")]);
}

// ---- Task 4: contract signatures ----

#[test]
fn contract_signatures_exist() {
    use zygo_plugins::snapshot::{missing_commands, snapshot, Delivered, Snapshot};
    type Sel = zygo_plugins::select::Selection;
    let _: fn(&std::path::Path, Agent, &Sel) -> anyhow::Result<Snapshot> = snapshot;
    let _: fn(&zygo_core::paths::Paths, &Sel, Instant) -> anyhow::Result<Delivered> = delivered_in;
    type Pairs = Vec<(String, String)>;
    let _: fn(&std::path::Path, Option<&OsStr>) -> Pairs = missing_commands;
    type Files = Vec<(String, Vec<u8>)>;
    let _: fn() -> Files = embedded::files;
}

// ---- Round 2 ----

const MANIFEST_INVALID: &str = "store manifest is invalid; run zygo plugins sync";

/// A lock I/O error: the embedded plugin only, `plugins: partial`, one detail with the error kind and no path.
fn assert_lock_failed(fx: &Fx, paths: &zygo_core::paths::Paths, cfg: &zygo_config::Merged) {
    let sel = selection(cfg, None);
    let session = fx.path("session");
    let s = snap(paths, &session, Agent::ClaudeCode, &sel, None).unwrap();
    assert!(session.join("plugins/zygo/skills/zygo-cli/SKILL.md").is_file());
    assert_eq!(s.plugins.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["zygo"]);
    assert_eq!(s.lines, vec!["plugins: partial"]);
    assert_eq!(s.details.len(), 1, "{:?}", s.details);
    let d = &s.details[0];
    assert!(d.starts_with("store lock failed: ") && d.len() > "store lock failed: ".len(), "{d}");
    assert!(!d.contains("busy") && !d.contains(&fx.0.display().to_string()) && !d.contains('/'), "{d}");
    assert!(delivered_in(paths, &sel, far()).is_err());
    // list reports the same condition, apart from "store lock busy"
    assert_eq!(list(cfg, None, paths, None), [format!("  {d}")]);
}

#[test]
fn store_lock_error_still_delivers_the_embedded_plugin() {
    let fx = Fx::new();
    let (paths, cfg) = stored(&fx, &[PluginSpec::new("demo")], "");
    std::fs::remove_dir_all(&paths.plugin_store).unwrap();
    std::fs::write(&paths.plugin_store, "x").unwrap();
    assert_lock_failed(&fx, &paths, &cfg);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let fx = Fx::new();
        let (paths, cfg) = stored(&fx, &[PluginSpec::new("demo")], "");
        std::fs::set_permissions(paths.plugin_store.join(".lock"), std::fs::Permissions::from_mode(0o444)).unwrap();
        assert_lock_failed(&fx, &paths, &cfg);
    }
}

#[test]
fn invalid_manifest_is_partial_in_the_snapshot_and_in_list() {
    let fx = Fx::new();
    let (paths, cfg) = stored(&fx, &[PluginSpec::new("demo")], "");
    std::fs::write(paths.plugin_store.join("manifest.json"), "{garbage").unwrap();
    let s = snap(&paths, &fx.path("session"), Agent::ClaudeCode, &selection(&cfg, None), None).unwrap();
    assert_eq!(s.plugins.len(), 1);
    assert_eq!(s.lines, vec!["plugins: partial"]);
    assert_eq!(s.details, vec![MANIFEST_INVALID]);
    assert_eq!(list(&cfg, None, &paths, None), ["team:demo (not synced)".to_string(), format!("  {MANIFEST_INVALID}")]);
}

#[test]
fn list_and_snapshot_read_mcp_only_from_the_manifest() {
    let fx = Fx::new();
    let (paths, cfg) = stored(&fx, &[PluginSpec::new("demo")], "");
    std::fs::write(paths.plugin_store.join("team/demo/1.0.0/.mcp.json"), mcp(serde_json::json!({"x": {"command": "zygo-no-such-cmd"}}))).unwrap();
    let empty = empty_dir(&fx, "empty");
    let p = Some(empty.as_os_str());
    assert_eq!(list(&cfg, None, &paths, p), ["team:demo 1.0.0 (default) skills: hello · mcp: - · code: none · missing: -"]);
    let s = snap(&paths, &fx.path("session"), Agent::ClaudeCode, &selection(&cfg, None), p).unwrap();
    assert!(s.lines.is_empty() && s.details.is_empty(), "{:?} {:?}", s.lines, s.details);
    assert!(!fx.path("session/plugins/demo/.mcp.json").exists());
}

#[test]
fn mcp_rewrite_keeps_key_order_and_number_text() {
    let fx = Fx::new();
    let big = "123456789012345678901234567890";
    let exe = serde_json::to_string(&exe()).unwrap();
    let text = format!(r#"{{"mcpServers":{{"zeta":{{"command":{exe},"timeout":{big}}},"gone":{{"command":"zygo-no-such-cmd"}},"alpha":{{"command":{exe},"args":["a"]}}}}}}"#);
    let (paths, cfg) = stored(&fx, &[PluginSpec::new("demo").file(".mcp.json", &text)], "");
    approve(&paths, "team", "demo");
    let empty = empty_dir(&fx, "empty");
    let s = snap(&paths, &fx.path("session"), Agent::ClaudeCode, &selection(&cfg, None), Some(empty.as_os_str())).unwrap();
    assert_eq!(s.plugins[1].mcp, ["zeta", "alpha"]);
    let out = std::fs::read_to_string(fx.path("session/plugins/demo/.mcp.json")).unwrap();
    let (z, a) = (out.find("\"zeta\"").unwrap(), out.find("\"alpha\"").unwrap());
    assert!(z < a && !out.contains("\"gone\""), "{out}");
    assert!(out.contains(&format!(": {big}")), "{out}");
}

/// A session path whose own files fit `PATH_MAX` while a long plugin file does not.
#[cfg(unix)]
#[test]
fn session_write_failure_is_not_blamed_on_the_store() {
    const PATH_MAX: usize = if cfg!(target_os = "macos") { 1024 } else { 4096 };
    let fx = Fx::new();
    let rel: &'static str = Box::leak(format!("skills/hello/{}.txt", "n".repeat(200)).into_boxed_str());
    let (paths, cfg) = stored(&fx, &[PluginSpec::new("demo").file(rel, "n")], "");
    let target = PATH_MAX - 100;
    let mut session = fx.path("s");
    while session.as_os_str().len() < target {
        let n = (target - session.as_os_str().len() - 1).clamp(1, 100);
        session.push("d".repeat(n));
    }
    assert!(session.as_os_str().len() + "/plugins/zygo/.claude-plugin/plugin.json".len() < PATH_MAX);
    assert!(session.as_os_str().len() + "/plugins/demo/".len() + rel.len() >= PATH_MAX);
    let s = snap(&paths, &session, Agent::ClaudeCode, &selection(&cfg, None), None).unwrap();
    assert_eq!(s.details, vec!["plugin team:demo: cannot write the session snapshot"]);
    assert_eq!(s.lines, vec!["plugins: partial"]);
    assert_eq!(s.plugins.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["zygo"]);
    assert!(!session.join("plugins/demo").exists());
    assert!(session.join("plugins/zygo/skills/zygo-cli/SKILL.md").is_file());
}

/// Two manifest keys that differ only in case: on a case-folding file system the second write meets the first
/// file and is refused (never truncated through); elsewhere the second key is not in the store.
#[test]
fn a_leaf_that_already_exists_is_not_written_through() {
    let fx = Fx::new();
    let (paths, cfg) = stored(&fx, &[PluginSpec::new("demo")], "");
    let mut m = read_manifest(&paths.plugin_store).unwrap().unwrap();
    let e = m.plugins.iter_mut().find(|e| e.plugin == "demo").unwrap();
    let fe = e.files["skills/hello/SKILL.md"].clone();
    e.files.insert("skills/hello/skill.md".into(), fe);
    write_manifest(&paths.plugin_store, &m).unwrap();
    let folds = paths.plugin_store.join("team/demo/1.0.0/skills/hello/skill.md").exists();
    let s = snap(&paths, &fx.path("session"), Agent::ClaudeCode, &selection(&cfg, None), None).unwrap();
    assert!(!fx.path("session/plugins/demo").exists());
    let want = if folds { "plugin team:demo: cannot write the session snapshot" } else { "plugin team:demo: store copy differs from its manifest; run zygo plugins sync" };
    assert_eq!(s.details, vec![want]);
}

#[test]
fn stale_session_plugins_are_removed() {
    let fx = Fx::new();
    let (paths, cfg) = stored(&fx, &[PluginSpec::new("demo")], "");
    for f in ["session/plugins/demo/stale.txt", "session/plugins/gone/skills/old/SKILL.md", "session/plugins/zygo/old.md"] { fx.write(f, "x"); }
    let session = fx.path("session");
    snap(&paths, &session, Agent::ClaudeCode, &selection(&cfg, None), None).unwrap();
    let mut want: std::collections::BTreeSet<String> = embedded::files().into_iter().map(|(n, _)| format!("plugins/zygo/{n}")).collect();
    want.extend(["plugins/demo/.claude-plugin/plugin.json", "plugins/demo/skills/hello/SKILL.md"].map(String::from));
    assert_eq!(tree(&session), want);
    assert!(!fx.path("session/plugins/gone").exists());
}

#[test]
fn relative_path_entries_are_not_searched() {
    let fx = Fx::new();
    let root = fx.path("plugin");
    let fake = if cfg!(windows) { "fake-srv.exe" } else { "fake-srv" };
    fx.write(&format!("plugin/{fake}"), "x");
    fx.write(&format!("plugin/bin/{fake}"), "x");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for p in [root.join(fake), root.join("bin").join(fake)] { std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap(); }
    }
    let joined = std::env::join_paths([".", "", "bin"]).unwrap();
    for p in [OsStr::new("."), OsStr::new(""), OsStr::new("bin"), joined.as_os_str()] {
        assert!(!command_found("fake-srv", &root, Some(p)), "{p:?}");
    }
    let abs = std::env::join_paths([OsStr::new("."), root.join("bin").as_os_str()]).unwrap();
    assert!(command_found("fake-srv", &root, Some(abs.as_os_str())));
}
