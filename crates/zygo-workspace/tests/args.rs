use std::path::Path;
use zygo_core::Agent;
use zygo_workspace::args::{bypass_flags, filter, has_user_info, removable, Filtered};

const UUID: &str = "1b4e28ba-2fa1-41d2-883f-0016d3cca427";
const INLINE: &str = r#"{"mcpServers":{}}"#;

fn s(a: &[&str]) -> Vec<String> {
    a.iter().map(|x| x.to_string()).collect()
}
fn cc(a: &[&str]) -> Filtered {
    filter(Agent::ClaudeCode, &s(a))
}
fn oc(a: &[&str]) -> Filtered {
    filter(Agent::OpenCode, &s(a))
}
/// The absolute form of a path relative to the test's current directory (the package directory).
fn abs(rel: &str) -> String {
    std::env::current_dir().unwrap().join(rel).to_str().unwrap().to_string()
}

#[test] // AU30 removal rules; the W§8.3 session list; W§18 #6 values
fn claude_removes_session_print_and_prompt_arguments() {
    let gone: &[&[&str]] = &[
        &["-c"], &["--continue"], &["-r"], &["--resume"], &["-r", "abc"], &["--resume", "abc"], &["--resume=abc"], &["-rabc"],
        &["--session-id", UUID], &["--session-id=1b4e28ba-2fa1-41d2-883f-0016d3cca427"], &["--fork-session"],
        &["--from-pr"], &["--from-pr", "123"], &["--from-pr=https://example.com/pr/1"],
        &["--cloud"], &["--cloud", "fix the bug"], &["--teleport"], &["--teleport", "abc"],
        &["-p"], &["--print"], &["-pc"],
        &["--system-prompt", "be terse"], &["--system-prompt=be terse"], &["--append-system-prompt", "be terse"],
        &["fix it"], &["-"], &["--", "fix it", "--model", "x"],
        // a required value that looks like a flag is the value
        &["--session-id", "--model", "x"], &["--system-prompt", "-be terse"],
    ];
    for a in gone {
        assert_eq!(cc(a), Filtered::default(), "{a:?}");
        assert_eq!(removable(Agent::ClaudeCode, &s(a)), s(a), "{a:?}");
    }
    assert_eq!(cc(&["--resume", "abc", "fix it"]).kept, s(&[]));
    // an optional value that starts with `-` is not the value
    assert_eq!(cc(&["-r", "--model", "lead"]).kept, s(&["--model", "lead"]));
    assert_eq!(cc(&["--cloud", "--verbose"]).kept, s(&["--verbose"]));
    assert_eq!(removable(Agent::ClaudeCode, &s(&["-r", "--model", "lead", "fix it"])), s(&["-r", "fix it"]));
    assert_eq!(cc(&["--model", "lead", "--", "fix it", "--verbose"]).kept, s(&["--model", "lead"]));
    assert_eq!(removable(Agent::ClaudeCode, &s(&["--model", "lead", "--", "fix it", "--verbose"])), s(&["--", "fix it", "--verbose"]));
}

#[test] // AU30 "the rest is stored as typed"
fn claude_keeps_the_rest_as_typed() {
    let kept: &[&[&str]] = &[
        &["--model", "lead"], &["--model=lead"], &["--model", "-weird"], &["-n", "my name"], &["--add-dir", "a", "b", "--verbose"],
        &["-d", "hooks"], &["--debug=hooks"], &["--debug"], &["--tmux=classic"], &["--permission-mode", "plan"],
        &["--dangerously-skip-permissions"], &["--allow-dangerously-skip-permissions"], &["--settings", "s.json"],
        &["--future-flag"], &["--future=1"], &["-xyz"], &["--model"],
    ];
    for a in kept {
        assert_eq!(cc(a), Filtered { kept: s(a), dropped_flags: vec![] }, "{a:?}");
        assert_eq!(removable(Agent::ClaudeCode, &s(a)), s(&[]), "{a:?}");
    }
    assert_eq!(cc(&["--add-dir", "a", "b", "--verbose", "fix it"]).kept, s(&["--add-dir", "a", "b", "--verbose"]));
    assert_eq!(cc(&["--future-flag", "x"]).kept, s(&["--future-flag"]));
    // short clusters: a removed letter leaves the kept ones
    assert_eq!(cc(&["-pd"]).kept, s(&["-d"]));
    assert_eq!(removable(Agent::ClaudeCode, &s(&["-pd"])), s(&["-pd"]));
    assert_eq!(cc(&["-pdhooks"]).kept, s(&["-dhooks"]));
    assert_eq!(cc(&["-pn", "my name"]).kept, s(&["-n", "my name"]));
}

#[test] // AU30 OpenCode rules
fn opencode_rules() {
    let gone: &[&[&str]] = &[
        &["-c"], &["--continue"], &["-s", "ses_1"], &["--session", "ses_1"], &["--session=ses_1"], &["-sses_1"], &["-cs", "ses_1"],
        &["--fork"], &["--prompt", "do it"], &["--prompt=do it"], &["--prompt", "--fork"],
        &["proj"], &["run", "fix it"], &["attach", "https://u:p@h"], &["--", "proj"],
    ];
    for a in gone {
        assert_eq!(oc(a), Filtered::default(), "{a:?}");
        assert_eq!(removable(Agent::OpenCode, &s(a)), s(a), "{a:?}");
    }
    let kept: &[&[&str]] = &[
        &["-m", "zygo/lead"], &["--model=zygo/lead"], &["--agent", "build"], &["--cors", "a", "b"], &["--port", "0"], &["--auto"],
        &["--print-logs"], &["--log-level", "DEBUG"],
    ];
    for a in kept {
        assert_eq!(oc(a), Filtered { kept: s(a), dropped_flags: vec![] }, "{a:?}");
    }
    // the Claude Code table does not apply: `-r` is an unknown flag here, `abc` a positional
    assert_eq!(oc(&["-r", "abc"]).kept, s(&["-r"]));
    assert_eq!(oc(&["--mcp-config", INLINE]), Filtered { kept: s(&["--mcp-config"]), dropped_flags: vec![] });
}

#[test] // AU30 content flags, W§7.2 `member.dropped_flags`, W-C7
fn content_flags_keep_files_only() {
    let file = abs("Cargo.toml");
    let r = cc(&["--mcp-config", "Cargo.toml", INLINE, "--model", "lead"]);
    assert_eq!(r.kept, s(&["--mcp-config", &file, "--model", "lead"]));
    assert_eq!(r.dropped_flags, s(&["--mcp-config"]));
    assert_eq!(removable(Agent::ClaudeCode, &s(&["--mcp-config", "Cargo.toml", INLINE])), s(&[INLINE]));
    // each value is checked; the name is listed one time
    assert_eq!(cc(&["--mcp-config", INLINE, "--mcp-config=also-inline"]), Filtered { kept: vec![], dropped_flags: s(&["--mcp-config"]) });
    assert_eq!(cc(&["--agents", r#"{"r":{"description":"d","prompt":"p"}}"#]), Filtered { kept: vec![], dropped_flags: s(&["--agents"]) });
    assert_eq!(cc(&["--agents", "Cargo.toml"]), Filtered { kept: s(&["--agents", &file]), dropped_flags: vec![] });
    assert_eq!(cc(&["--agents=Cargo.toml"]).kept, vec![format!("--agents={file}")]);
    assert_eq!(cc(&["--agents", &file]).kept, s(&["--agents", &file]));
    assert_eq!(cc(&["--agents", "src"]).dropped_flags, s(&["--agents"])); // a directory is not a file
    let r = cc(&["--json-schema", r#"{"type":"object"}"#, "--plugin-url", "https://example.com/p.zip", "--verbose"]);
    assert_eq!(r, Filtered { kept: s(&["--verbose"]), dropped_flags: s(&["--json-schema", "--plugin-url"]) });
    // a content flag with no value is removed and names no dropped value
    assert_eq!(cc(&["--agents"]), Filtered::default());
}

#[test] // AU30 "keeps --system-prompt-file and --append-system-prompt-file with the path only"; paths are absolute
fn prompt_file_flags_keep_the_absolute_path() {
    let file = abs("Cargo.toml");
    assert_eq!(cc(&["--system-prompt-file", "Cargo.toml"]).kept, s(&["--system-prompt-file", &file]));
    assert_eq!(cc(&["--append-system-prompt-file=Cargo.toml"]).kept, vec![format!("--append-system-prompt-file={file}")]);
    assert_eq!(cc(&["--append-system-prompt-file", &file]).kept, s(&["--append-system-prompt-file", &file]));
    // no existence check for a prompt file
    assert_eq!(cc(&["--system-prompt-file", "no-such.md"]).kept, s(&["--system-prompt-file", &abs("no-such.md")]));
    assert_eq!(removable(Agent::ClaudeCode, &s(&["--system-prompt-file", "Cargo.toml"])), s(&[]));
}

// ---- the pinned help (review focus 4) ----

#[derive(Debug, Clone, Copy, PartialEq)]
enum Arity {
    None,
    Required,
    Optional,
    Variadic,
}

fn fixture(rel: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures").join(rel)).unwrap()
}

/// Claude Code: the rows between `Options:` and `Commands:` that start with two spaces and `-`. The option text ends at
/// the first run of two spaces. Each word that starts with `-` is a name (a `,` at its end is cut); the other word is
/// the value mark: `[x]` optional, `<x...>` variadic, `<x>` required, none = boolean.
fn claude_options() -> Vec<(Vec<String>, Arity)> {
    let text = fixture("claude-code/help.txt");
    let rows = text.lines().skip_while(|l| *l != "Options:").take_while(|l| *l != "Commands:").filter(|l| l.starts_with("  -"));
    rows.map(|l| {
        let spec = l[2..].split("  ").next().unwrap();
        let names = spec.split_whitespace().filter(|w| w.starts_with('-')).map(|w| w.trim_end_matches(',').to_string()).collect();
        let arity = match spec.split_whitespace().find(|w| !w.starts_with('-')) {
            None => Arity::None,
            Some(m) if m.starts_with('[') => Arity::Optional,
            Some(m) if m.ends_with("...>") => Arity::Variadic,
            Some(m) => {
                assert!(m.starts_with('<'), "{l}");
                Arity::Required
            }
        };
        (names, arity)
    })
    .collect()
}

/// OpenCode: after `Options:`, a row is a line whose first non-space character is `-`; the names are the text up to
/// the first run of two spaces, split at `, `. The row and the lines below it up to the next row hold the type tag:
/// `[array]` variadic, `[string]` or `[number]` required, `[boolean]` boolean.
fn opencode_options() -> Vec<(Vec<String>, Arity)> {
    let text = fixture("opencode/help.txt");
    let mut rows: Vec<(Vec<String>, String)> = vec![];
    for l in text.lines().skip_while(|l| *l != "Options:").skip(1) {
        let t = l.trim_start();
        if t.starts_with('-') {
            rows.push((t.split("  ").next().unwrap().split(", ").map(str::to_string).collect(), t.to_string()));
        } else if let Some(last) = rows.last_mut() {
            last.1.push_str(t);
        }
    }
    rows.into_iter()
        .map(|(names, row)| {
            let arity = if row.contains("[array]") {
                Arity::Variadic
            } else if row.contains("[string]") || row.contains("[number]") {
                Arity::Required
            } else {
                assert!(row.contains("[boolean]"), "{row}");
                Arity::None
            };
            (names, arity)
        })
        .collect()
}

/// Proves by behaviour that the filter gives `name` the arity of the help and the class of the plan. `--model` is a
/// kept flag with a required value in both helps. A removed boolean and a removed optional flag give the same result
/// for every input (the next token is removed as a value or as a positional), so one probe covers both.
fn probe(agent: Agent, name: &str, arity: Arity, removed: bool) {
    let f = |a: &[&str]| filter(agent, &s(a)).kept;
    match (removed, arity) {
        (false, Arity::None) => assert_eq!(f(&[name, "zz"]), s(&[name]), "{name}"),
        (false, Arity::Required) => assert_eq!(f(&[name, "--model", "zz"]), s(&[name, "--model"]), "{name}"),
        (false, Arity::Optional) => {
            assert_eq!(f(&[name, "zz", "yy"]), s(&[name, "zz"]), "{name}");
            assert_eq!(f(&[name, "--model", "zz"]), s(&[name, "--model", "zz"]), "{name}");
        }
        (false, Arity::Variadic) => assert_eq!(f(&[name, "aa", "bb", "--model", "zz"]), s(&[name, "aa", "bb", "--model", "zz"]), "{name}"),
        (true, Arity::Required) => assert_eq!(f(&[name, "--model", "zz"]), s(&[]), "{name}"),
        (true, Arity::None | Arity::Optional) => assert_eq!(f(&[name, "zz", "--model", "m"]), s(&["--model", "m"]), "{name}"),
        (true, Arity::Variadic) => panic!("the plan removes no variadic flag: {name}"),
    }
}

const CC_REMOVED: &[&str] = &[
    "-c", "--continue", "-r", "--resume", "--session-id", "--fork-session", "--from-pr", "--cloud", "--teleport", "-p", "--print",
    "--system-prompt", "--append-system-prompt",
];
const CC_CONTENT: &[&str] = &["--agents", "--mcp-config", "--json-schema", "--plugin-url"];
const OC_REMOVED: &[&str] = &["-c", "--continue", "-s", "--session", "--fork", "--prompt"];

#[test] // AU30 "the builder classifies flags from the pinned --help"; review focus 4
fn claude_filter_matches_pinned_help() {
    let opts = claude_options();
    let all: Vec<&str> = opts.iter().flat_map(|o| o.0.iter().map(String::as_str)).collect();
    for n in CC_REMOVED.iter().chain(CC_CONTENT).chain(&["--settings", "--dangerously-skip-permissions", "--allow-dangerously-skip-permissions", "--permission-mode"]) {
        assert!(all.contains(n), "{n} is not in the pinned help");
    }
    // every option of the help, every name: the filter has the arity of the help
    for (names, arity) in &opts {
        for n in names.iter().filter(|n| !CC_CONTENT.contains(&n.as_str())) {
            probe(Agent::ClaudeCode, n, *arity, CC_REMOVED.contains(&n.as_str()));
        }
    }
    // the optional-value options are these and no other; a new one in a later pin must be classified
    let optional: Vec<&str> = opts.iter().filter(|o| o.1 == Arity::Optional).map(|o| o.0.last().unwrap().as_str()).collect();
    assert_eq!(optional, ["--cloud", "--debug", "--from-pr", "--prompt-suggestions", "--remote-control", "--resume", "--teleport", "--worktree"]);
    // content flags: the arity of the help, and the filter uses it
    let arity = |n: &str| opts.iter().find(|o| o.0.iter().any(|x| x == n)).unwrap().1;
    assert_eq!(arity("--mcp-config"), Arity::Variadic);
    let file = abs("Cargo.toml");
    assert_eq!(cc(&["--mcp-config", &file, &file, "--model", "m"]).kept, s(&["--mcp-config", &file, &file, "--model", "m"]));
    for n in ["--agents", "--json-schema", "--plugin-url"] {
        assert_eq!(arity(n), Arity::Required, "{n}");
        assert_eq!(cc(&[n, "--model", "zz"]), Filtered { kept: vec![], dropped_flags: s(&[n]) }, "{n}");
    }
    // the two prompt-file flags have no option row in this pin; the help names them in the `--bare` text only
    assert!(!all.contains(&"--system-prompt-file") && !all.contains(&"--append-system-prompt-file"));
    let text = fixture("claude-code/help.txt");
    assert!(text.contains("--system-prompt[-file]") && text.contains("--append-system-prompt[-file]"));
    assert!(text.contains("\"bypassPermissions\""));
}

#[test] // AU30, review focus 4
fn opencode_filter_matches_pinned_help() {
    let opts = opencode_options();
    let all: Vec<&str> = opts.iter().flat_map(|o| o.0.iter().map(String::as_str)).collect();
    for n in OC_REMOVED {
        assert!(all.contains(n), "{n} is not in the pinned help");
    }
    for (names, arity) in &opts {
        for n in names {
            probe(Agent::OpenCode, n, *arity, OC_REMOVED.contains(&n.as_str()));
        }
    }
}

#[test] // AU70: the four bypass forms; W§18 #33 values
fn bypass_forms() {
    assert_eq!(bypass_flags(&s(&["--model", "lead"]), false), s(&[]));
    assert_eq!(bypass_flags(&s(&["--dangerously-skip-permissions"]), false), s(&["--dangerously-skip-permissions"]));
    assert_eq!(bypass_flags(&s(&["--allow-dangerously-skip-permissions"]), false), s(&["--allow-dangerously-skip-permissions"]));
    assert_eq!(bypass_flags(&s(&["--permission-mode", "bypassPermissions"]), false), s(&["--permission-mode bypassPermissions"]));
    assert_eq!(bypass_flags(&s(&["--permission-mode=bypassPermissions"]), false), s(&["--permission-mode bypassPermissions"]));
    assert_eq!(bypass_flags(&s(&["--permission-mode", "plan"]), false), s(&[]));
    assert_eq!(bypass_flags(&s(&["--permission-mode"]), false), s(&[]));
    assert_eq!(bypass_flags(&s(&[]), true), s(&["yolo = true"]));
    // argument order, each form one time, `yolo = true` last
    let all = s(&["--permission-mode", "bypassPermissions", "--model", "lead", "--dangerously-skip-permissions",
        "--allow-dangerously-skip-permissions", "--dangerously-skip-permissions", "--permission-mode=bypassPermissions"]);
    assert_eq!(bypass_flags(&all, true), s(&["--permission-mode bypassPermissions", "--dangerously-skip-permissions",
        "--allow-dangerously-skip-permissions", "yolo = true"]));
    // a token after `--` is not a flag
    assert_eq!(bypass_flags(&s(&["--", "--dangerously-skip-permissions"]), false), s(&[]));
}

#[test] // AU98, W-C7; W§18 #19 value `https://u:p@h`
fn user_info() {
    for url in [
        "https://u:p@h", "https://user:pass@zygo.example:8443/x", "https://user@zygo.example", "https://:pass@zygo.example",
        "https://@zygo.example",          // empty user
        "https://u:p@[::1]:8443/x",       // IPv6 host
        "u:p@zygo.example:8443",          // no scheme
        "//u:p@zygo.example",
        "https:\\\\u:p@zygo.example",     // a URL parser reads `\` as `/` here
        "https:u:p@zygo.example",
        "ht\ttps://u:p@zygo.example",     // a URL parser removes tab, LF and CR
        " https://u:p@zygo.example\n",
        "https://zygo.example\\@evil.example",
        // a URL parser skips each `/` and `\` after the scheme of a special URL
        "https:/u:p@zygo.example", "https:///u:p@zygo.example", "https:/\\u:p@zygo.example", "https:\\/\\u:p@zygo.example",
        "https:/\r\n/\t/u:p@zygo.example",
        "///u:p@zygo.example", "\\\\u:p@zygo.example", "/\\u:p@zygo.example",
    ] {
        assert!(has_user_info(url), "{url:?}");
    }
    for url in [
        "", "https://zygo.example", "https://127.0.0.1:8443", "https://zygo.example:8443/a@b", "https://zygo.example/?next=a@b",
        "https://zygo.example?next=a@b", "https://zygo.example#a@b", "https://zygo.example/?u=https://a@b",
        "https://[::1]:8443/", "https://[2001:db8::1]/p@q",
        "zygo.example:8443", "zygo.example/a@b", "zygo.example/?u=https://a@b", "//zygo.example/a@b",
        "https://u%40p.example",
        "https:/zygo.example/a@b", "https:///zygo.example/a@b", "https:\\\\zygo.example/a@b", "///zygo.example?a@b", "/", ":", "://",
    ] {
        assert!(!has_user_info(url), "{url:?}");
    }
}
