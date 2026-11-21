//! The built-in act101 marketplace pin and its compiled-in digests (spec §15.7, §15.9). No environment mutation, no network, no sleeps.

mod common;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use common::*;
use serde_json::{json, Value};
use zygo_config::LoadContext;
#[cfg(unix)]
use zygo_config::{load_with_builtin, Merged};
#[cfg(unix)]
use zygo_core::paths::Paths;
#[cfg(unix)]
use zygo_core::Agent;
use zygo_plugins::approve::PreApproval;
use zygo_plugins::builtin::{approved, approved_from, render_approved, write_fixture};
use zygo_plugins::check::check_dir;
use zygo_plugins::select::{selection, PluginId};
#[cfg(unix)]
use zygo_plugins::sync::sync_with;

fn names(v: &[&str]) -> Vec<String> { v.iter().map(|s| s.to_string()).collect() }

fn code(name: &'static str) -> PluginSpec { PluginSpec::new(name).file("scripts/run.sh", "echo one\n") }

fn head(r: &str, c: &str) -> String {
    format!("# Written by scripts/pin-builtin-marketplace.sh. Do not edit.\nref = \"{r}\"\ncommit = \"{c}\"\n")
}

fn block(p: &str, d: &str) -> String { format!("\n[[approval]]\nmarketplace = \"act101\"\nplugin = \"{p}\"\ndigest = \"{d}\"\n") }

fn digest_of(root: &Path, name: &str) -> String {
    check_dir(root).plugins.into_iter().find(|p| p.name == name).unwrap().digest.clone().unwrap()
}

fn json_of(p: &Path) -> Value { serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap() }

/// `git add -A`, then a commit by `name <email>` (committer `committer` when `Some`), with no developer git config.
fn commit_as(dir: &Path, name: &str, email: &str, committer: Option<(&str, &str)>) {
    git(dir, &["add", "-A"]);
    let mut c = std::process::Command::new("git");
    c.current_dir(dir).env("GIT_CONFIG_GLOBAL", NULL).env("GIT_CONFIG_NOSYSTEM", "1")
        .arg("-c").arg(format!("user.name={name}")).arg("-c").arg(format!("user.email={email}"))
        .args(["-c", "commit.gpgsign=false", "commit", "-q", "-m", "id"]);
    if let Some((n, e)) = committer { c.env("GIT_COMMITTER_NAME", n).env("GIT_COMMITTER_EMAIL", e); }
    let out = c.output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
}

// ---- Task 1: approved_from, render_approved, write_fixture ----

#[test]
fn render_approved_writes_one_block_per_plugin_with_code() {
    let fx = Fx::new();
    let r = build_market(&fx, "m", &[code("act-lite"), code("other")]);
    let c = "a".repeat(40);
    assert_eq!(
        render_approved(&r.dir, "v1.0.0", &c, &names(&["other", "act-lite"])).unwrap(),
        head("v1.0.0", &c) + &block("other", &digest_of(&r.dir, "other")) + &block("act-lite", &digest_of(&r.dir, "act-lite"))
    );
}

#[test]
fn render_approved_skips_a_plugin_without_code() {
    let fx = Fx::new();
    let r = build_market(&fx, "m", &[PluginSpec::new("act-lite")]);
    let c = "b".repeat(40);
    assert_eq!(render_approved(&r.dir, "v1.0.0", &c, &names(&["act-lite"])).unwrap(), head("v1.0.0", &c));
}

#[test]
fn render_approved_refuses_a_missing_plugin() {
    let fx = Fx::new();
    let r = build_market(&fx, "m", &[code("other")]);
    let e = render_approved(&r.dir, "v1.0.0", &"a".repeat(40), &names(&["act-lite"])).unwrap_err();
    assert_eq!(e.to_string(), "act-lite is not in the marketplace");
}

#[test]
fn render_approved_refuses_a_plugin_that_fails_the_check() {
    let fx = Fx::new();
    let r = Repo::new(&fx, "m");
    market_json(&r, &[("act-lite", "1.0.0")]);
    plugin_files(&r, "plugins/act-lite", "act-lite", "1.0.0");
    r.write("plugins/act-lite/scripts/run.sh", "echo one\n");
    r.write("plugins/act-lite/skills/hello/big.txt", vec![b'a'; 1_048_577]);
    r.commit("first");
    let e = render_approved(&r.dir, "v1.0.0", &"a".repeat(40), &names(&["act-lite"])).unwrap_err();
    assert_eq!(e.to_string(), "act-lite: check failed: file-too-large skills/hello/big.txt (1048577 bytes, limit 1048576)");
}

#[cfg(unix)]
#[test]
fn fixture_disk_modes_give_the_digest_sync_stores() {
    use std::os::unix::fs::PermissionsExt;
    let fx = Fx::new();
    // `skills/hello/tool` (no extension, not under `scripts/` or `bin/`) is code only through its exec bit (check.rs D4).
    let r = build_market(&fx, "m", &[code("act-lite").file("skills/hello/tool", "echo two\n").exec("skills/hello/tool")]);
    let sha = git(&r.dir, &["rev-parse", "v1.0.0^{commit}"]);
    let out = fx.path("out");
    write_fixture(&r.dir, &out, &names(&["act-lite"])).unwrap();
    let run = out.join("plugins/act-lite/skills/hello/tool");
    assert!(std::fs::metadata(&run).unwrap().permissions().mode() & 0o111 != 0);
    let p = paths(&fx);
    std::fs::create_dir_all(p.config_file.parent().unwrap()).unwrap();
    std::fs::write(&p.config_file, "").unwrap();
    let cfg = load_with_builtin(&LoadContext { paths: p.clone(), repo: None, team: None },
        mp("act101", &r.src(), "v1.0.0", None, &["act-lite"]).parse().unwrap()).unwrap();
    let rep = sync_with(&cfg, &p, WIDE_TOTAL, &opts(&fx));
    assert!(rep.failures.is_empty(), "{:?}", rep.failures);
    let d = |root: &Path| approved_from(&render_approved(root, "v1.0.0", &sha, &names(&["act-lite"])).unwrap()).unwrap()[0].digest.clone();
    assert_eq!(d(&out), digest(&p, "act101", "act-lite"));
    std::fs::set_permissions(&run, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_ne!(d(&out), digest(&p, "act101", "act-lite"));
}

#[test]
fn approved_from_reads_the_rendered_text() {
    let text = head("v1", &"c".repeat(40)) + &block("x", &"1".repeat(64)) + &block("y", &"2".repeat(64));
    assert_eq!(approved_from(&text).unwrap(), vec![
        PreApproval { marketplace: "act101".into(), plugin: "x".into(), digest: "1".repeat(64) },
        PreApproval { marketplace: "act101".into(), plugin: "y".into(), digest: "2".repeat(64) },
    ]);
}

#[test]
fn approved_from_a_file_without_approvals_is_empty() {
    assert!(approved_from(&head("", "")).unwrap().is_empty());
    assert!(approved_from("").unwrap().is_empty());
}

#[test]
fn write_fixture_keeps_only_the_named_plugins() {
    let fx = Fx::new();
    let r = build_market(&fx, "m", &[code("act-lite"), code("other")]);
    r.write("README.md", "r\n");
    r.write("install.sh", "echo i\n");
    r.commit("more");
    let out = fx.path("out");
    write_fixture(&r.dir, &out, &names(&["act-lite"])).unwrap();
    assert_eq!(tree(&out), BTreeSet::from([".claude-plugin/marketplace.json", "plugins/act-lite/.claude-plugin/plugin.json",
        "plugins/act-lite/scripts/run.sh", "plugins/act-lite/skills/hello/SKILL.md"].map(String::from)));
    let m = json_of(&out.join(".claude-plugin/marketplace.json"));
    let listed: Vec<&str> = m["plugins"].as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap()).collect();
    assert_eq!(listed, ["act-lite"]);
    assert_eq!(digest_of(&out, "act-lite"), digest_of(&r.dir, "act-lite"));
}

#[test]
fn write_fixture_neutralizes_every_person_field() {
    let fx = Fx::new();
    let r = build_market(&fx, "m", &[code("act-lite").file(".claude-plugin/plugin.json",
        r#"{"name":"act-lite","version":"1.0.0","description":"Demo plugin","license":"MIT","author":{"name":"Author Two","email":"two@example.invalid"},"authors":[{"name":"Author Two"},"Maint Three"],"maintainers":["Maint Three"],"contributors":[{"name":"Helper Four","url":"https://example.invalid/h"}]}"#)]);
    r.write(".claude-plugin/marketplace.json",
        r#"{"name":"team","owner":{"name":"Owner","url":"https://example.invalid/o"},"plugins":[{"name":"act-lite","source":"./plugins/act-lite","description":"Demo","version":"1.0.0","author":{"name":"Author Two","url":"https://example.invalid/a"}}]}"#);
    r.commit("names");
    let out = fx.path("out");
    write_fixture(&r.dir, &out, &names(&["act-lite"])).unwrap();
    let m = json_of(&out.join(".claude-plugin/marketplace.json"));
    assert_eq!(m["owner"], json!({"name": "act101", "url": "act101"}));
    assert_eq!(m["plugins"][0]["author"], json!({"name": "act101", "url": "act101"}));
    assert_eq!(m["plugins"][0]["description"], "Demo");
    let p = json_of(&out.join("plugins/act-lite/.claude-plugin/plugin.json"));
    assert_eq!(p["author"], json!({"name": "act101", "email": "act101"}));
    assert_eq!(p["authors"], json!([{"name": "act101"}, "act101"]));
    assert_eq!(p["maintainers"], json!(["act101"]));
    assert_eq!(p["contributors"], json!([{"name": "act101", "url": "act101"}]));
    assert_eq!(p["license"], "MIT");
    assert_eq!(digest_of(&out, "act-lite"), digest_of(&r.dir, "act-lite"));
}

#[test]
fn write_fixture_stops_when_a_rewrite_changes_a_digest() {
    let fx = Fx::new();
    let r = build_market(&fx, "m", &[code("act-lite").file("scripts/meta.json", "{\"author\": {\"name\": \"x\"}}\n")]);
    let e = write_fixture(&r.dir, &fx.path("out"), &names(&["act-lite"])).unwrap_err();
    assert_eq!(e.to_string(), "act-lite: digest changed by the fixture rewrite");
}

#[test]
fn write_fixture_refuses_a_history_name_in_a_kept_file() {
    let fx = Fx::new();
    let r = build_market(&fx, "m", &[code("act-lite")]);
    r.write("plugins/act-lite/skills/hello/SKILL.md", "---\nname: hello\ndescription: Says hello\n---\nWritten by probe author.\n");
    commit_as(&r.dir, "Probe Author", "probe@example.invalid", None);
    let e = write_fixture(&r.dir, &fx.path("out"), &names(&["act-lite"])).unwrap_err().to_string();
    assert_eq!(e, "plugins/act-lite/skills/hello/SKILL.md: holds a name or e-mail of the source checkout");
    assert!(!e.to_lowercase().contains("probe"));
}

#[test]
fn write_fixture_skips_the_github_web_committer() {
    let fx = Fx::new();
    let r = build_market(&fx, "m", &[code("act-lite")]);
    r.write("plugins/act-lite/skills/hello/SKILL.md", "---\nname: hello\ndescription: Says hello\n---\nSee https://github.com/example.\n");
    commit_as(&r.dir, "Probe Author", "probe@example.invalid", Some(("GitHub", "noreply@github.com")));
    write_fixture(&r.dir, &fx.path("out"), &names(&["act-lite"])).unwrap();
}

const SKILL: &str = "plugins/act-lite/skills/hello/SKILL.md";

/// `write_fixture` of a market with `act-lite` and `files`, committed by `name <email>`.
fn fixture_by(fx: &Fx, name: &str, email: &str, files: &[(&str, &str)]) -> anyhow::Result<()> {
    let r = build_market(fx, "m", &[code("act-lite")]);
    for (p, t) in files { r.write(p, t); }
    commit_as(&r.dir, name, email, None);
    write_fixture(&r.dir, &fx.path("out"), &names(&["act-lite"]))
}

/// `res` is the name-scan refusal for `path`, and its text holds no `part` of the name.
fn refused(res: anyhow::Result<()>, path: &str, part: &str) {
    let e = res.unwrap_err().to_string();
    assert_eq!(e, format!("{path}: holds a name or e-mail of the source checkout"));
    assert!(!e.to_lowercase().contains(part), "{e}");
}

#[test]
fn write_fixture_refuses_a_first_name_alone() {
    let fx = Fx::new();
    refused(fixture_by(&fx, "Probe Author", "probe@example.invalid",
        &[(SKILL, "---\nname: hello\ndescription: Says hello\n---\nThanks, Probe!\n")]), SKILL, "probe");
}

#[test]
fn write_fixture_refuses_a_name_with_other_whitespace() {
    let fx = Fx::new();
    refused(fixture_by(&fx, "Jo Li", "jo@example.invalid",
        &[(SKILL, "---\nname: hello\ndescription: Says hello\n---\nThanks to JO  \n LI.\n")]), SKILL, "jo");
    let fx = Fx::new();
    refused(fixture_by(&fx, "Jo  Li", "jo@example.invalid",
        &[(SKILL, "---\nname: hello\ndescription: Says hello\n---\nThanks to jo li.\n")]), SKILL, "jo");
}

#[test]
fn write_fixture_refuses_a_name_as_a_json_key() {
    let fx = Fx::new();
    refused(fixture_by(&fx, "Probe Author", "probe@example.invalid",
        &[("plugins/act-lite/notes.json", "{\"thanks\": {\"Probe Author\": true}}\n")]), "plugins/act-lite/notes.json", "probe");
}

#[test]
fn write_fixture_refuses_a_name_in_markdown_front_matter() {
    let fx = Fx::new();
    refused(fixture_by(&fx, "Probe Author", "probe@example.invalid",
        &[(SKILL, "---\nname: hello\ndescription: Says hello\ncredit: PROBE\n---\nSay hello.\n")]), SKILL, "probe");
}

#[test]
fn write_fixture_matches_name_words_at_word_boundaries_only() {
    let fx = Fx::new();
    fixture_by(&fx, "Probe Author", "probe@example.invalid",
        &[(SKILL, "---\nname: hello\ndescription: Says hello\n---\nReprobe the authored probes.\n")]).unwrap();
}

/// `res` is the name-scan refusal for a file or directory name under `at`, and its text holds no `part`.
fn refused_at(res: anyhow::Result<()>, at: &str, part: &str) {
    let e = res.unwrap_err().to_string();
    assert_eq!(e, format!("{at}: a file or directory name holds a name or e-mail of the source checkout"));
    assert!(!e.to_lowercase().contains(part), "{e}");
}

#[test]
fn write_fixture_refuses_a_name_word_in_a_path() {
    let fx = Fx::new();
    refused_at(fixture_by(&fx, "Probe Author", "probe@example.invalid",
        &[("plugins/act-lite/docs/probe.md", "Notes.\n")]), "plugins/act-lite/docs", "probe");
    let fx = Fx::new();
    refused_at(fixture_by(&fx, "Probe Author", "probe@example.invalid",
        &[("plugins/act-lite/probe/notes.md", "Notes.\n")]), "plugins/act-lite", "probe");
}

#[test]
fn write_fixture_refuses_a_camel_case_name() {
    for text in ["probeAuthor", "ProbeAuthor", "byProbeTeam"] {
        let fx = Fx::new();
        refused(fixture_by(&fx, "Probe Author", "probe@example.invalid",
            &[(SKILL, &format!("---\nname: hello\ndescription: Says hello\n---\nSee {text}.\n"))]), SKILL, "probe");
    }
}

#[test]
fn write_fixture_refuses_a_github_handle() {
    let fx = Fx::new();
    refused(fixture_by(&fx, "Probe Author", "12345+phandle@users.noreply.github.com",
        &[(SKILL, "---\nname: hello\ndescription: Says hello\n---\nThanks @phandle.\n")]), SKILL, "phandle");
}

#[test]
fn write_fixture_refuses_a_part_of_a_hyphenated_name() {
    let fx = Fx::new();
    refused(fixture_by(&fx, "Anne-Probe", "anne@example.invalid",
        &[(SKILL, "---\nname: hello\ndescription: Says hello\n---\nThanks, Anne.\n")]), SKILL, "anne");
}

#[test]
fn write_fixture_skips_service_accounts() {
    let fx = Fx::new();
    let r = build_market(&fx, "m", &[code("act-lite")]);
    r.write(SKILL, "---\nname: hello\ndescription: Says hello\n---\nCut a release: act-release-bot and helper[bot] help.\n");
    commit_as(&r.dir, "act-release-bot", "release@example.invalid", Some(("helper[bot]", "helper@example.invalid")));
    write_fixture(&r.dir, &fx.path("out"), &names(&["act-lite"])).unwrap();
}

#[test]
fn write_fixture_matches_unicode_case_and_non_utf8_text() {
    let fx = Fx::new();
    refused(fixture_by(&fx, "Zoë Probe", "zoe@example.invalid",
        &[(SKILL, "---\nname: hello\ndescription: Says hello\n---\nZOË wrote this.\n")]), SKILL, "zoë");
    let fx = Fx::new();
    let r = build_market(&fx, "m", &[code("act-lite")]);
    r.write("plugins/act-lite/notes.txt", b"\xff\xfe thanks probe \xff\n");
    commit_as(&r.dir, "Probe Author", "probe@example.invalid", None);
    refused(write_fixture(&r.dir, &fx.path("out"), &names(&["act-lite"])), "plugins/act-lite/notes.txt", "probe");
}

// ---- Task 2: the pin script ----

fn repo_root() -> PathBuf { Path::new(env!("CARGO_MANIFEST_DIR")).join("../..") }

#[cfg(unix)]
fn script(args: &[&str], path: Option<&std::ffi::OsStr>) -> std::process::Output {
    let mut c = std::process::Command::new("sh");
    c.arg(repo_root().join("scripts/pin-builtin-marketplace.sh")).args(args).current_dir(repo_root())
        .env("GIT_CONFIG_GLOBAL", NULL).env("GIT_CONFIG_NOSYSTEM", "1");
    if let Some(p) = path { c.env("PATH", p); }
    c.output().unwrap()
}

#[cfg(unix)]
fn code_of(o: &std::process::Output) -> i32 { o.status.code().unwrap() }

#[cfg(unix)]
fn err(o: &std::process::Output) -> String { String::from_utf8_lossy(&o.stderr).into_owned() }

#[cfg(unix)]
const MT: &str = "# The built-in marketplace.\n[[marketplace]]\nname = \"act101\"\nsource = \"github:act101-ai/act101\"\ndefault_on = [\"act-lite\"]\n";
#[cfg(unix)]
const MTP: &str = "crates/zygo-config/data/marketplace.toml";
#[cfg(unix)]
const APP: &str = "crates/zygo-plugins/data/act101-approved.toml";
#[cfg(unix)]
const FXP: &str = "fixtures/act101-marketplace";

#[cfg(unix)]
fn pin_root(fx: &Fx) -> PathBuf {
    fx.write("root/crates/zygo-config/data/marketplace.toml", MT);
    fx.write("root/crates/zygo-plugins/data/act101-approved.toml", head("", ""));
    fx.path("root")
}

#[cfg(unix)]
fn s(p: &Path) -> &str { p.to_str().unwrap() }

#[cfg(unix)]
fn pinned_root(fx: &Fx) -> (PathBuf, String) {
    let r = build_market(fx, "src", &[code("act-lite"), code("other")]);
    let sha = git(&r.dir, &["rev-parse", "v1.0.0^{commit}"]);
    let root = pin_root(fx);
    let o = script(&["--root", s(&root), "--source", &r.src(), "v1.0.0"], None);
    assert_eq!(code_of(&o), 0, "{}", err(&o));
    (root, sha)
}

#[cfg(unix)]
fn unchanged(root: &Path) {
    assert_eq!(std::fs::read_to_string(root.join(MTP)).unwrap(), MT);
    assert_eq!(std::fs::read_to_string(root.join(APP)).unwrap(), head("", ""));
    assert!(!root.join(FXP).exists());
}

#[cfg(unix)]
fn write_run(fx: &Fx, root: &Path, tag: &str) -> std::process::Output {
    script(&["--root", s(root), "--source", s(&fx.path("src")), tag], None)
}

#[cfg(unix)]
#[test]
fn pin_write_pins_a_local_marketplace_tag() {
    let fx = Fx::new();
    let (root, sha) = pinned_root(&fx);
    assert_eq!(std::fs::read_to_string(root.join(MTP)).unwrap(), format!(
        "# The built-in marketplace.\n[[marketplace]]\nname = \"act101\"\nsource = \"github:act101-ai/act101\"\nref = \"v1.0.0\"\ncommit = \"{sha}\"\ndefault_on = [\"act-lite\"]\n"));
    assert_eq!(std::fs::read_to_string(root.join(FXP).join("PIN.toml")).unwrap(), head("v1.0.0", &sha));
    assert_eq!(tree(&root.join(FXP).join("tree")), BTreeSet::from([".claude-plugin/marketplace.json",
        "plugins/act-lite/.claude-plugin/plugin.json", "plugins/act-lite/scripts/run.sh", "plugins/act-lite/skills/hello/SKILL.md"].map(String::from)));
    assert_eq!(std::fs::read_to_string(root.join(APP)).unwrap(),
        render_approved(&root.join(FXP).join("tree"), "v1.0.0", &sha, &names(&["act-lite"])).unwrap());
}

#[cfg(unix)]
#[test]
fn pin_write_refuses_a_marketplace_without_a_default_on_plugin() {
    let fx = Fx::new();
    build_market(&fx, "src", &[code("other")]);
    let root = pin_root(&fx);
    let o = write_run(&fx, &root, "v1.0.0");
    assert_eq!(code_of(&o), 1);
    assert!(err(&o).contains("pin-builtin-marketplace: act-lite is not in the marketplace"), "{}", err(&o));
    unchanged(&root);
}

#[cfg(unix)]
#[test]
fn pin_write_refuses_a_branch_name() {
    let fx = Fx::new();
    build_market(&fx, "src", &[code("act-lite")]);
    let root = pin_root(&fx);
    let o = write_run(&fx, &root, "main");
    assert_eq!(code_of(&o), 1);
    assert!(err(&o).contains("pin-builtin-marketplace: main is not a tag"), "{}", err(&o));
    unchanged(&root);
}

#[cfg(unix)]
#[test]
fn pin_write_stops_when_the_rewrite_changes_a_digest() {
    let fx = Fx::new();
    build_market(&fx, "src", &[code("act-lite").file("scripts/meta.json", "{\"author\": {\"name\": \"x\"}}\n")]);
    let root = pin_root(&fx);
    let o = write_run(&fx, &root, "v1.0.0");
    assert_eq!(code_of(&o), 1);
    assert!(err(&o).contains("pin-builtin-marketplace: act-lite: digest changed by the fixture rewrite"), "{}", err(&o));
    unchanged(&root);
}

#[cfg(unix)]
#[test]
fn pin_check_passes_on_a_consistent_root() {
    let fx = Fx::new();
    let (root, _) = pinned_root(&fx);
    let o = script(&["--root", s(&root), "--check"], None);
    assert_eq!(code_of(&o), 0, "{}", err(&o));
    assert!(o.stdout.is_empty());
}

#[cfg(unix)]
#[test]
fn pin_check_fails_when_a_digest_differs() {
    let fx = Fx::new();
    let (root, _) = pinned_root(&fx);
    let text = std::fs::read_to_string(root.join(APP)).unwrap();
    let at = text.find("digest = \"").unwrap() + "digest = \"".len();
    let new = if &text[at..at + 1] == "0" { "1" } else { "0" };
    std::fs::write(root.join(APP), format!("{}{new}{}", &text[..at], &text[at + 1..])).unwrap();
    let o = script(&["--root", s(&root), "--check"], None);
    assert_eq!(code_of(&o), 1);
    assert!(err(&o).contains("approved data differs from the fixture"), "{}", err(&o));
}

#[cfg(unix)]
#[test]
fn pin_check_fails_when_the_pin_commit_differs() {
    let fx = Fx::new();
    let (root, _) = pinned_root(&fx);
    std::fs::write(root.join(FXP).join("PIN.toml"), head("v1.0.0", &"0".repeat(40))).unwrap();
    let o = script(&["--root", s(&root), "--check"], None);
    assert_eq!(code_of(&o), 1);
    assert!(err(&o).contains("PIN.toml commit differs from marketplace.toml"), "{}", err(&o));
}

#[cfg(unix)]
#[test]
fn pin_check_fails_when_the_fixture_code_changed() {
    let fx = Fx::new();
    let (root, _) = pinned_root(&fx);
    std::fs::write(root.join(FXP).join("tree/plugins/act-lite/scripts/extra.sh"), "echo hi\n").unwrap();
    let o = script(&["--root", s(&root), "--check"], None);
    assert_eq!(code_of(&o), 1);
    assert!(err(&o).contains("approved data differs from the fixture"), "{}", err(&o));
}

#[cfg(unix)]
#[test]
fn pin_check_fails_without_a_ref() {
    let fx = Fx::new();
    let (root, _) = pinned_root(&fx);
    let text = std::fs::read_to_string(root.join(MTP)).unwrap();
    let kept: String = text.lines().filter(|l| !l.starts_with("ref = ")).map(|l| format!("{l}\n")).collect();
    std::fs::write(root.join(MTP), kept).unwrap();
    let o = script(&["--root", s(&root), "--check"], None);
    assert_eq!(code_of(&o), 1);
    assert!(err(&o).contains("marketplace.toml has no ref"), "{}", err(&o));
}

#[cfg(unix)]
#[test]
fn pin_rejects_unknown_arguments() {
    for args in [&["--bogus"][..], &[], &["--check", "v1"], &["--source", "/x", "--check"]] {
        let o = script(args, None);
        assert_eq!(code_of(&o), 2, "{args:?}");
        assert!(err(&o).starts_with("usage: "), "{args:?}: {}", err(&o));
    }
}

#[cfg(unix)]
#[test]
fn pin_rejects_an_empty_root() {
    for args in [&["--root", "", "--check"][..], &["--root", "", "v1.0.0"]] {
        let o = script(args, None);
        assert_eq!(code_of(&o), 2, "{args:?}");
        assert!(err(&o).starts_with("usage: "), "{args:?}: {}", err(&o));
    }
}

#[cfg(unix)]
#[test]
fn pin_rejects_an_empty_source() {
    let o = script(&["--source", "", "v1.0.0"], None);
    assert_eq!(code_of(&o), 2);
    assert!(err(&o).starts_with("usage: "), "{}", err(&o));
}

#[cfg(unix)]
#[test]
fn pin_write_refuses_an_unsafe_tag() {
    let fx = Fx::new();
    let root = pin_root(&fx);
    for tag in ["v1;x", "v1 0", "a/b", "v1\"x", "v1\nx", "$x"] {
        let o = script(&["--root", s(&root), "--source", s(&fx.path("src")), tag], None);
        assert_eq!(code_of(&o), 2, "{tag:?}");
        assert!(err(&o).contains("pin-builtin-marketplace: tag must match ^[A-Za-z0-9._+-]+$"), "{tag:?}: {}", err(&o));
    }
    unchanged(&root);
}

#[cfg(unix)]
#[test]
fn pin_check_rejects_an_unsafe_ref() {
    let fx = Fx::new();
    let (root, sha) = pinned_root(&fx);
    let text = std::fs::read_to_string(root.join(MTP)).unwrap();
    for r in ["v1.0.0\\\" x", "v1 0", "a/b"] {
        let new: String = text.lines().map(|l| if l.starts_with("ref = ") { format!("ref = \"{r}\"\n") } else { format!("{l}\n") }).collect();
        std::fs::write(root.join(MTP), new).unwrap();
        std::fs::write(root.join(FXP).join("PIN.toml"), head(r, &sha)).unwrap();
        let o = script(&["--root", s(&root), "--check"], None);
        assert_eq!(code_of(&o), 1, "{r:?}");
        assert!(err(&o).contains("pin-builtin-marketplace: marketplace.toml ref must match ^[A-Za-z0-9._+-]+$"), "{r:?}: {}", err(&o));
    }
}

#[cfg(unix)]
#[test]
fn pin_check_runs_no_git() {
    use std::os::unix::fs::PermissionsExt;
    let fx = Fx::new();
    let (root, _) = pinned_root(&fx);
    let fake = fx.write("fakebin/git", format!("#!/bin/sh\ntouch '{}'\nexit 1\n", fx.path("git-ran").display()));
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = std::env::join_paths(std::iter::once(fx.path("fakebin"))
        .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap()))).unwrap();
    let o = script(&["--root", s(&root), "--check"], Some(&path));
    assert_eq!(code_of(&o), 0, "{}", err(&o));
    assert!(!fx.path("git-ran").exists());
}

#[cfg(unix)]
#[test]
fn pin_check_fails_on_a_bad_commit() {
    let fx = Fx::new();
    let (root, _) = pinned_root(&fx);
    let text = std::fs::read_to_string(root.join(MTP)).unwrap();
    let new: String = text.lines().map(|l| if l.starts_with("commit = ") { "commit = \"abc\"\n".to_string() } else { format!("{l}\n") }).collect();
    std::fs::write(root.join(MTP), new).unwrap();
    let o = script(&["--root", s(&root), "--check"], None);
    assert_eq!(code_of(&o), 1);
    assert!(err(&o).contains("marketplace.toml has no 40-hex commit"), "{}", err(&o));
}

#[cfg(unix)]
#[test]
fn pin_check_fails_when_the_pin_ref_differs() {
    let fx = Fx::new();
    let (root, sha) = pinned_root(&fx);
    std::fs::write(root.join(FXP).join("PIN.toml"), head("v9.9.9", &sha)).unwrap();
    let o = script(&["--root", s(&root), "--check"], None);
    assert_eq!(code_of(&o), 1);
    assert!(err(&o).contains("PIN.toml ref differs from marketplace.toml"), "{}", err(&o));
}

#[cfg(unix)]
#[test]
fn pin_write_refuses_a_history_name_and_prints_none() {
    let fx = Fx::new();
    let r = build_market(&fx, "src", &[code("act-lite")]);
    r.write("plugins/act-lite/skills/hello/SKILL.md", "---\nname: hello\ndescription: Says hello\n---\nWritten by probe author.\n");
    commit_as(&r.dir, "Probe Author", "probe@example.invalid", None);
    git(&r.dir, &["tag", "-f", "v1.0.0"]);
    let root = pin_root(&fx);
    let o = write_run(&fx, &root, "v1.0.0");
    assert_eq!(code_of(&o), 1);
    assert!(err(&o).contains("pin-builtin-marketplace: plugins/act-lite/skills/hello/SKILL.md: holds a name or e-mail of the source checkout"), "{}", err(&o));
    assert!(!String::from_utf8_lossy(&o.stdout).to_lowercase().contains("probe"));
    assert!(!err(&o).to_lowercase().contains("probe"));
    unchanged(&root);
}

// ---- Task 3: the pin of v2.9.2 and its fixture ----

fn fixture() -> PathBuf { repo_root().join("fixtures/act101-marketplace") }

fn pinned() -> zygo_config::Marketplace { zygo_config::builtin::defaults().marketplaces.remove(0) }

fn toml_of(p: &Path) -> toml::Table { std::fs::read_to_string(p).unwrap().parse().unwrap() }

const COMMIT: &str = "409b9dad5da85a9857e8d584d1c6a9fe4136c3e3";
#[cfg(unix)]
const ACT_LITE_DIGEST: &str = "8f7ac593245afb9c4cf3ca294e83d2648d76310026ac1f7865acb8476eda496d";

#[test]
fn data_file_pins_a_tag_and_a_commit() {
    assert_eq!(zygo_config::builtin::defaults().marketplaces.len(), 1);
    let m = pinned();
    assert_eq!((m.name.as_str(), m.source.as_str()), ("act101", "github:act101-ai/act101"));
    assert_eq!(m.default_on, ["act-lite"]);
    assert_eq!(m.git_ref, "v2.9.2");
    assert_eq!(m.commit.as_deref(), Some(COMMIT));
}

#[test]
fn ref_and_commit_agree_across_data_and_fixture() {
    for p in [fixture().join("PIN.toml"), repo_root().join("crates/zygo-plugins/data/act101-approved.toml")] {
        let t = toml_of(&p);
        assert_eq!(t["ref"], toml::Value::String("v2.9.2".into()), "{}", p.display());
        assert_eq!(t["commit"], toml::Value::String(COMMIT.into()), "{}", p.display());
    }
}

#[test]
fn fixture_commit_lists_act_lite_with_code() {
    let m = json_of(&fixture().join("tree/.claude-plugin/marketplace.json"));
    let list = m["plugins"].as_array().unwrap();
    let listed: Vec<&str> = list.iter().map(|p| p["name"].as_str().unwrap()).collect();
    assert_eq!(listed, ["act-lite"]);
    assert_eq!(list[0]["source"], "./plugin-act-lite");
    assert_eq!(list[0]["version"], "1.2.3");
    let files = tree(&fixture().join("tree"));
    assert_eq!(files.len(), 19);
    assert!(files.iter().all(|f| f.starts_with(".claude-plugin/") || f.starts_with("plugin-act-lite/")), "{files:?}");
    let r = check_dir(&fixture().join("tree"));
    assert!(r.violations.is_empty(), "{:?}", r.violations);
    assert!(r.skipped.is_empty(), "{:?}", r.skipped);
    assert_eq!(r.plugins.len(), 1);
    assert_eq!(r.plugins[0].name, "act-lite");
    assert!(r.plugins[0].ok(), "{:?}", r.plugins[0].violations);
    assert!(r.plugins[0].digest.is_some());
}

#[cfg(unix)]
#[test]
fn approved_data_is_what_render_writes_for_the_fixture() {
    assert_eq!(render_approved(&fixture().join("tree"), "v2.9.2", COMMIT, &names(&["act-lite"])).unwrap(),
        std::fs::read_to_string(repo_root().join("crates/zygo-plugins/data/act101-approved.toml")).unwrap());
    assert_eq!(approved(), vec![PreApproval { marketplace: "act101".into(), plugin: "act-lite".into(), digest: ACT_LITE_DIGEST.into() }]);
}

#[cfg(unix)]
#[test]
fn pin_check_passes_on_the_checked_in_data() {
    let o = script(&["--check"], None);
    assert_eq!(code_of(&o), 0, "{}", err(&o));
}

#[test]
fn act_lite_language_list_is_for_the_pinned_version() {
    let langs = json_of(&repo_root().join("fixtures/act-lite/languages.json"));
    let compiled = langs["act_lite"].as_str().unwrap().trim_start_matches('v').to_string();
    let m = json_of(&fixture().join("tree/.claude-plugin/marketplace.json"));
    let entry = m["plugins"].as_array().unwrap().iter().find(|p| p["name"] == "act-lite").unwrap();
    assert_eq!(compiled, entry["version"].as_str().unwrap());
    assert_eq!(compiled, "1.2.3");
}

/// Each person value under `key` at `at` is `act101`; failures name the path and key, never the value.
fn people_neutral(v: &Value, at: &str, key: &str, bad: &mut Vec<String>) {
    match v {
        Value::Array(a) => a.iter().for_each(|x| people_neutral(x, at, key, bad)),
        Value::String(s) => if s != "act101" { bad.push(format!("{at}: {key}")) },
        Value::Object(o) => for k in ["name", "url", "email"] {
            if o.get(k).is_some_and(|x| x != "act101") { bad.push(format!("{at}: {key}.{k}")); }
        },
        _ => {}
    }
}

fn person_fields(v: &Value, at: &str, bad: &mut Vec<String>) {
    match v {
        Value::Object(o) => for (k, x) in o {
            if ["owner", "author", "authors", "maintainers", "contributors"].contains(&k.as_str()) { people_neutral(x, at, k, bad); }
            else { person_fields(x, at, bad); }
        },
        Value::Array(a) => a.iter().for_each(|x| person_fields(x, at, bad)),
        _ => {}
    }
}

#[test]
fn fixture_holds_no_personal_names() {
    assert!(fixture().join("tree").is_dir());
    let email = regex::Regex::new(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}").unwrap();
    let mut bad = Vec::new();
    for e in walkdir::WalkDir::new(fixture()).into_iter().flatten().filter(|e| e.file_type().is_file()) {
        let at = e.path().strip_prefix(fixture()).unwrap().display().to_string();
        let text = String::from_utf8_lossy(&std::fs::read(e.path()).unwrap()).into_owned();
        if email.is_match(&text) { bad.push(format!("{at}: e-mail address")); }
        if at.ends_with(".json") { person_fields(&serde_json::from_str(&text).unwrap(), &at, &mut bad); }
    }
    assert!(bad.is_empty(), "{bad:?}");
}

// ---- Task 4: selection carries the built-in digests ----

fn id(s: &str) -> PluginId { s.parse().unwrap() }

#[cfg(unix)]
fn pinned_tree() -> PathBuf { fixture().join("tree") }

/// A `PATH` directory that holds `act-lite`, the `.mcp.json` command of act-lite.
#[cfg(unix)]
fn bin(fx: &Fx) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let f = fx.write("bin/act-lite", "#!/bin/sh\nexit 0\n");
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
    fx.path("bin")
}

/// Writes `user` to the user config and loads it with the built-in `act101` at `git_ref` of `repo` (R6).
#[cfg(unix)]
fn load_pinned(p: &Paths, repo: &Repo, git_ref: &str, user: &str) -> Merged {
    std::fs::create_dir_all(p.config_file.parent().unwrap()).unwrap();
    std::fs::write(&p.config_file, user).unwrap();
    load_with_builtin(&LoadContext { paths: p.clone(), repo: None, team: None },
        mp("act101", &repo.src(), git_ref, None, &["act-lite"]).parse().unwrap()).unwrap()
}

/// The pinned fixture plus a synthetic `act101` plugin (D2 (a)) in a local repository tagged `v2.9.2`.
#[cfg(unix)]
fn world(fx: &Fx, user: &str) -> (Repo, Paths, Merged) {
    let repo = Repo::new(fx, "pinned");
    copy_tree(&pinned_tree(), &repo.dir);
    plugin_files(&repo, "plugin", "act101", "2.9.2");
    repo.write("plugin/scripts/run.sh", "echo one\n");
    let mj = repo.dir.join(".claude-plugin/marketplace.json");
    let mut m = json_of(&mj);
    m["plugins"].as_array_mut().unwrap()
        .push(json!({"name": "act101", "source": "./plugin", "description": "Demo", "version": "2.9.2"}));
    std::fs::write(&mj, serde_json::to_string_pretty(&m).unwrap()).unwrap();
    repo.commit("pin");
    repo.tag("v2.9.2");
    let p = paths(fx);
    let cfg = load_pinned(&p, &repo, "v2.9.2", user);
    (repo, p, cfg)
}

#[test]
fn real_load_selection_carries_the_built_in_digests() {
    let fx = Fx::new();
    let cfg = zygo_config::load(&LoadContext { paths: paths(&fx), repo: None, team: None }).unwrap();
    let sel = selection(&cfg, None);
    assert_eq!(sel.pre_approved, approved());
    assert_eq!(approved().len(), 1);
    assert_eq!(sel.selected, BTreeSet::from([id("act101:act-lite")]));
}

#[test]
fn no_built_in_entry_means_no_built_in_digests() {
    assert!(selection(&merged(&Fx::new(), "", None), None).pre_approved.is_empty());
    let b = mp("act101", "/elsewhere", "v1", None, &["act-lite"]);
    assert!(selection(&merged(&Fx::new(), &b, None), None).pre_approved.is_empty());
    assert!(selection(&merged(&Fx::new(), "", Some(&b)), None).pre_approved.is_empty());
}

#[test]
fn built_in_digests_stay_with_team_approvals() {
    let fx = Fx::new();
    let team = mp("t", "/t", "v1", None, &[]) + &format!("approvals = [{{ plugin = \"a\", digest = \"{}\" }}]\n", "ab".repeat(32));
    let cfg = zygo_config::load(&LoadContext { paths: paths(&fx), repo: None, team: Some(team.parse().unwrap()) }).unwrap();
    assert_eq!(selection(&cfg, None).pre_approved,
        [approved(), vec![PreApproval { marketplace: "t".into(), plugin: "a".into(), digest: "ab".repeat(32) }]].concat());
}

#[cfg(unix)]
#[test]
fn pinned_act_lite_is_delivered_with_no_prompt() {
    let fx = Fx::new();
    let (_repo, p, cfg) = world(&fx, "");
    let sel = selection(&cfg, None);
    assert_eq!(sel.selected, BTreeSet::from([id("act101:act-lite")]));
    let rep = sync_with(&cfg, &p, WIDE_TOTAL, &opts(&fx));
    assert!(rep.failures.is_empty(), "{:?}", rep.failures);
    assert!(rep.lines.is_empty(), "{:?}", rep.lines);
    assert_eq!(digest(&p, "act101", "act-lite"), ACT_LITE_DIGEST);
    let session = fx.path("s1");
    let s = snap(&p, &session, Agent::ClaudeCode, &sel, Some(bin(&fx).as_os_str())).unwrap();
    assert!(s.lines.is_empty(), "{:?}", s.lines);
    assert!(session.join("plugins/act-lite/.claude-plugin/plugin.json").is_file());
    assert!(session.join("plugins/act-lite/.mcp.json").is_file());
    assert!(!session.join("plugins/act101").exists());
    assert!(!p.approvals_file.exists());
}

#[cfg(unix)]
#[test]
fn enabling_act101_needs_its_own_approval() {
    let fx = Fx::new();
    let (_repo, p, cfg) = world(&fx, "[plugins]\nenable = [\"act101:act101\"]\n");
    let rep = sync_with(&cfg, &p, WIDE_TOTAL, &opts(&fx));
    assert!(rep.failures.is_empty(), "{:?}", rep.failures);
    let sel = selection(&cfg, None);
    let s1 = fx.path("s1");
    let out = snap(&p, &s1, Agent::ClaudeCode, &sel, Some(bin(&fx).as_os_str())).unwrap();
    assert!(s1.join("plugins/act-lite").exists());
    assert!(!s1.join("plugins/act101").exists());
    assert_eq!(out.lines, ["plugin act101:act101: code needs approval; run zygo plugins approve act101:act101"]);
    approve(&p, "act101", "act101");
    let s2 = fx.path("s2");
    let out = snap(&p, &s2, Agent::ClaudeCode, &sel, Some(bin(&fx).as_os_str())).unwrap();
    assert!(s2.join("plugins/act-lite").exists());
    assert!(s2.join("plugins/act101").exists());
    assert!(out.lines.is_empty(), "{:?}", out.lines);
}

#[cfg(unix)]
#[test]
fn changed_code_at_another_ref_needs_approval() {
    let fx = Fx::new();
    let (repo, p, _) = world(&fx, "");
    repo.write("plugin-act-lite/scripts/extra.sh", "echo hi\n");
    repo.commit("next");
    repo.tag("zz-next");
    let cfg = load_pinned(&p, &repo, "v2.9.2", &mp("act101", &repo.src(), "zz-next", None, &["act-lite"]));
    assert!(cfg.warnings.is_empty(), "{:?}", cfg.warnings);
    let rep = sync_with(&cfg, &p, WIDE_TOTAL, &opts(&fx));
    assert!(rep.failures.is_empty(), "{:?}", rep.failures);
    assert_ne!(digest(&p, "act101", "act-lite"), ACT_LITE_DIGEST);
    let s1 = fx.path("s1");
    let out = snap(&p, &s1, Agent::ClaudeCode, &selection(&cfg, None), Some(bin(&fx).as_os_str())).unwrap();
    assert!(!s1.join("plugins/act-lite").exists());
    assert_eq!(out.lines, ["plugin act101:act-lite: code needs approval; run zygo plugins approve act101:act-lite"]);
    assert!(!p.approvals_file.exists());
}

#[cfg(unix)]
#[test]
fn user_ref_alone_moves_a_built_in_entry_with_a_commit() {
    let fx = Fx::new();
    let (repo, p, _) = world(&fx, "");
    let pin = git(&repo.dir, &["rev-parse", "v2.9.2^{commit}"]);
    repo.write("plugin-act-lite/scripts/extra.sh", "echo hi\n");
    repo.commit("next");
    repo.tag("zz-next");
    std::fs::write(&p.config_file, "[[marketplace]]\nname = \"act101\"\nref = \"zz-next\"\n").unwrap();
    let cfg = load_with_builtin(&LoadContext { paths: p.clone(), repo: None, team: None },
        mp("act101", &repo.src(), "v2.9.2", Some(&pin), &["act-lite"]).parse().unwrap()).unwrap();
    assert!(cfg.warnings.is_empty(), "{:?}", cfg.warnings);
    let rep = sync_with(&cfg, &p, WIDE_TOTAL, &opts(&fx));
    assert!(rep.failures.is_empty(), "{:?}", rep.failures);
    assert!(!rep.lines.iter().any(|l| l.contains("moved")), "{:?}", rep.lines);
    assert_ne!(digest(&p, "act101", "act-lite"), ACT_LITE_DIGEST);
    assert_eq!(digest(&p, "act101", "act-lite"), digest_of(&repo.dir, "act-lite"));
}
