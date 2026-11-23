use std::path::Path;
use zygo_core::Agent;
use zygo_workspace::args::{bypass_flags, filter, filter_from, flags, has_session_arg, has_user_info, occurrences, removable, removed_labels, session_flags, Filtered};

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

#[test] // W7 D32, AU30: a relative path in a loadout pane's `args` is taken relative to that pane's folder
fn filter_from_resolves_against_the_base() {
    let t = tempfile::TempDir::new().unwrap();
    let b = t.path();
    std::fs::write(b.join("x.json"), "{}").unwrap();
    let ab = |rel: &str| std::path::absolute(b.join(rel)).unwrap().to_str().unwrap().to_string();
    let r = filter_from(Agent::ClaudeCode, &s(&["--agents", "./x.json", "--append-system-prompt-file", "p.md"]), b);
    assert_eq!(r, Filtered { kept: s(&["--agents", &ab("./x.json"), "--append-system-prompt-file", &ab("p.md")]), dropped_flags: vec![] });
    assert_eq!(filter_from(Agent::ClaudeCode, &s(&["--agents", "./none.json"]), b), Filtered { kept: vec![], dropped_flags: s(&["--agents"]) });
    let here = std::env::current_dir().unwrap();
    let cases: &[&[&str]] = &[
        &["--mcp-config", "Cargo.toml", INLINE, "--model", "lead"], &["--mcp-config", INLINE, "--mcp-config=also-inline"],
        &["--agents", "Cargo.toml"], &["--agents=Cargo.toml"], &["--agents", "src"], &["--agents"],
        &["--json-schema", r#"{"type":"object"}"#, "--plugin-url", "https://example.com/p.zip", "--verbose"],
        &["--system-prompt-file", "Cargo.toml"], &["--append-system-prompt-file=Cargo.toml"], &["--system-prompt-file", "no-such.md"],
        &["--resume", "abc", "fix it"], &["-r", "--model", "lead"], &["--model", "lead", "--", "fix it", "--verbose"], &["-pn", "my name"],
    ];
    for a in cases {
        assert_eq!(filter_from(Agent::ClaudeCode, &s(a), &here), cc(a), "{a:?}");
    }
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

/// The short option letters of a parsed help, sorted.
fn short_letters(opts: &[(Vec<String>, Arity)]) -> Vec<char> {
    let names = opts.iter().flat_map(|o| o.0.iter());
    let mut letters: Vec<char> = names.filter(|n| n.chars().count() == 2 && !n.starts_with("--")).map(|n| n.chars().nth(1).unwrap()).collect();
    letters.sort_unstable();
    letters
}

/// The ASCII letters that the table of `agent` knows as a short flag, sorted. An unknown `-x` is kept and takes no
/// value (`zz` is then a removed positional). A flag of the table is removed, or is kept with the value `zz`.
fn table_letters(agent: Agent) -> Vec<char> {
    let known = |c: &char| filter(agent, &s(&[&format!("-{c}"), "zz"])).kept != s(&[&format!("-{c}")]);
    ('0'..='9').chain('A'..='Z').chain('a'..='z').filter(known).collect()
}

#[test] // AU30, W-C7, review focus 4: a short flag that a later pin adds must be classified before the test passes
fn every_short_flag_in_the_pinned_help_is_classified() {
    // An unknown letter at the start of a cluster keeps the rest of the cluster as typed, so each short flag of the
    // help must be in the table of its agent. The exempt letters are `h` and `v` only.
    let claude = short_letters(&claude_options());
    assert_eq!(claude, ['c', 'd', 'h', 'n', 'p', 'r', 'v', 'w']);
    assert_eq!(table_letters(Agent::ClaudeCode), ['c', 'd', 'n', 'p', 'r', 'w']);
    let opencode = short_letters(&opencode_options());
    assert_eq!(opencode, ['c', 'h', 'm', 's', 'v']);
    assert_eq!(table_letters(Agent::OpenCode), ['c', 'm', 's']);
    // the help set is the table set plus the exempt letters, and nothing else
    for (help, agent) in [(claude, Agent::ClaudeCode), (opencode, Agent::OpenCode)] {
        let mut want = table_letters(agent);
        want.extend(['h', 'v']);
        want.sort_unstable();
        assert_eq!(help, want, "{agent:?}");
    }
    // The exempt letters: a cluster that starts with `-h` or `-v` is kept whole, with a session or print letter
    // in it. This is safe: `-h` (help) and `-v` (version) make the agent exit before a session exists, so the saved
    // letter after them has no effect at restore.
    assert_eq!(cc(&["-vp"]), Filtered { kept: s(&["-vp"]), dropped_flags: vec![] });
    assert_eq!(cc(&["-hc"]), Filtered { kept: s(&["-hc"]), dropped_flags: vec![] });
    assert_eq!(removable(Agent::ClaudeCode, &s(&["-vp", "-hc"])), s(&[]));
    assert_eq!(oc(&["-vc"]), Filtered { kept: s(&["-vc"]), dropped_flags: vec![] });
    assert_eq!(oc(&["-hc"]), Filtered { kept: s(&["-hc"]), dropped_flags: vec![] });
    assert_eq!(removable(Agent::OpenCode, &s(&["-vc", "-hc"])), s(&[]));
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

#[test] // W§8.3, AU30: one source for the session flags; `--` and flag values are not flags
fn session_flags_come_from_the_table() {
    assert_eq!(session_flags(Agent::ClaudeCode), ["-c", "--continue", "-r", "--resume", "--session-id", "--fork-session", "--from-pr", "--cloud", "--teleport"]);
    assert_eq!(session_flags(Agent::OpenCode), ["-c", "--continue", "-s", "--session", "--fork"]);
    for a in [&["--teleport"][..], &["--resume=abc"][..], &["-pc"][..], &["--model", "opus", "-r", "x"][..], &["--session-id", "U1"][..]] {
        assert!(has_session_arg(Agent::ClaudeCode, &s(a)), "{a:?}");
    }
    for a in [&[][..], &["--", "--resume"][..], &["--append-system-prompt", "--resume"][..], &["-p"][..], &["--continued"][..], &["--model", "opus"][..]] {
        assert!(!has_session_arg(Agent::ClaudeCode, &s(a)), "{a:?}");
    }
    assert!(has_session_arg(Agent::OpenCode, &s(&["-sses_1"])));
    assert!(has_session_arg(Agent::OpenCode, &s(&["--fork"])));
    assert!(!has_session_arg(Agent::OpenCode, &s(&["--prompt", "-s"])));
    assert_eq!(flags(Agent::ClaudeCode, &s(&["--model", "opus", "--settings=f", "--", "--settings", "g"])), ["--model", "--settings"]);
    assert_eq!(flags(Agent::ClaudeCode, &s(&["--append-system-prompt", "--settings"])), ["--append-system-prompt"]);
    assert_eq!(filter(Agent::ClaudeCode, &s(&["--teleport", "--model", "opus"])).kept, s(&["--model", "opus"]));
}

#[test] // L2 round 2 item 9: flag values and token spans by the same walk; `--` ends the flags
fn occurrences_give_flag_values_before_double_dash() {
    let args = s(&["--model", "opus", "--agents={}", "--bare", "-pc", "--append-system-prompt", "--model", "--", "--agents", "x"]);
    let got: Vec<(&str, Vec<String>, std::ops::Range<usize>)> =
        occurrences(Agent::ClaudeCode, &args).into_iter().map(|o| (o.name, o.values, o.tokens)).collect();
    assert_eq!(
        got,
        vec![
            ("--model", s(&["opus"]), 0..2),
            ("--agents", s(&["{}"]), 2..3),
            ("--bare", vec![], 3..4),
            ("--print", vec![], 4..5),
            ("--continue", vec![], 4..5),
            ("--append-system-prompt", s(&["--model"]), 5..7),
        ]
    );
    // `--bare` is in the table now; the filter keeps it as typed, as before
    assert_eq!(cc(&["--bare", "--bare=x", "--model", "m"]).kept, s(&["--bare", "--bare=x", "--model", "m"]));
    let got: Vec<(&str, Vec<String>)> = occurrences(Agent::OpenCode, &s(&["-m", "zygo/lead", "-sses_1", "--", "-s"])).into_iter().map(|o| (o.name, o.values)).collect();
    assert_eq!(got, vec![("--model", s(&["zygo/lead"])), ("--session", s(&["ses_1"]))]);
}

// ---- removed_labels (FIX-27 D8) ----

fn l(agent: Agent, a: &[&str]) -> Vec<String> {
    removed_labels(agent, &s(a), Path::new(""))
}

/// Every `(agent, args)` of the `l(..)` calls of the tests below.
const LABEL_CASES: &[(Agent, &[&str])] = &[
    (Agent::ClaudeCode, &["--system-prompt", "S1", "--append-system-prompt=S2", "-p", "S3", "pos", "--", "S4"]),
    (Agent::ClaudeCode, &["-pX", "-p", "-pX"]),
    (Agent::ClaudeCode, &["--system-prompt", "--api-token-is-SECRETVALUE=x", "--append-system-prompt", "-SECRET2", "-p", "pos"]),
    (Agent::OpenCode, &["--prompt", "--SECRET3=y", "-s", "-SECRET4", "--prompt", "-"]),
    (Agent::ClaudeCode, &["--model", "-p", "-p"]),
    (Agent::ClaudeCode, &["--system-prompt", "--", "-p", "SECRET"]),
    (Agent::ClaudeCode, &["--model", "--", "pos"]),
    (Agent::ClaudeCode, &["--model", "-p", "--", "x"]),
    (Agent::OpenCode, &["-m", "--prompt", "--prompt", "-m"]),
    (Agent::ClaudeCode, &["-cr"]),
    (Agent::ClaudeCode, &["-cr", "abc"]),
    (Agent::ClaudeCode, &["-p", "hi"]),
];

#[test] // attempt 1 round 1; F17: every positional is labelled
fn removed_labels_name_flags_never_values() {
    assert_eq!(l(Agent::ClaudeCode, &["--system-prompt", "S1", "--append-system-prompt=S2", "-p", "S3", "pos", "--", "S4"]),
        s(&["--system-prompt", "--append-system-prompt", "-p", "<positional>", "<positional>", "--", "<after -->"]));
    assert_eq!(l(Agent::ClaudeCode, &["-pX", "-p", "-pX"]), s(&["-p", "-p", "-p"]));
}

#[test] // round 1 B1; kills: labelling by token text
fn removed_labels_never_label_a_value_that_starts_with_a_dash() {
    assert_eq!(l(Agent::ClaudeCode, &["--system-prompt", "--api-token-is-SECRETVALUE=x", "--append-system-prompt", "-SECRET2", "-p", "pos"]),
        s(&["--system-prompt", "--append-system-prompt", "-p", "<positional>"]));
    assert_eq!(l(Agent::OpenCode, &["--prompt", "--SECRET3=y", "-s", "-SECRET4", "--prompt", "-"]), s(&["--prompt", "-s", "--prompt"]));
}

#[test] // round 3 F5; kills: finding a removed token by its text; `--` as a value
fn removed_labels_go_by_index_never_by_text() {
    assert_eq!(l(Agent::ClaudeCode, &["--model", "-p", "-p"]), s(&["-p"]));
    assert_eq!(l(Agent::ClaudeCode, &["--system-prompt", "--", "-p", "SECRET"]), s(&["--system-prompt", "-p", "<positional>"]));
    assert_eq!(l(Agent::ClaudeCode, &["--model", "--", "pos"]), s(&["<positional>"]));
    assert_eq!(l(Agent::ClaudeCode, &["--model", "-p", "--", "x"]), s(&["--", "<after -->"]));
    assert_eq!(l(Agent::OpenCode, &["-m", "--prompt", "--prompt", "-m"]), s(&["--prompt"]));
}

#[test] // kills: one label per token
fn removed_labels_name_each_flag_of_a_short_group() {
    assert_eq!(l(Agent::ClaudeCode, &["-cr"]), s(&["-c", "-r"]));
    assert_eq!(l(Agent::ClaudeCode, &["-cr", "abc"]), s(&["-c", "-r"]));
}

#[test] // F17; kills: the old "no label right after a labelled flag" exception
fn removed_labels_label_every_positional() {
    assert_eq!(l(Agent::ClaudeCode, &["-p", "hi"]), s(&["-p", "<positional>"]));
}

#[test] // kills: labelling only removed flag tokens, which gives an empty line
fn removed_labels_name_a_flag_that_loses_only_a_value() {
    let t = tempfile::TempDir::new().unwrap();
    std::fs::write(t.path().join("a.json"), "{}").unwrap();
    assert_eq!(removed_labels(Agent::ClaudeCode, &s(&["--mcp-config", "a.json", "{\"k\":\"SECRET\"}"]), t.path()), s(&["--mcp-config"]));
}

#[test] // kills: the process folder in place of `base`
fn removed_labels_take_content_files_from_the_base() {
    let t = tempfile::TempDir::new().unwrap();
    std::fs::write(t.path().join("m.json"), "{}").unwrap();
    assert_eq!(removed_labels(Agent::ClaudeCode, &s(&["--mcp-config", "m.json"]), t.path()), Vec::<String>::new());
    let empty = tempfile::TempDir::new().unwrap();
    assert_eq!(removed_labels(Agent::ClaudeCode, &s(&["--mcp-config", "m.json"]), empty.path()), s(&["--mcp-config"]));
}

/// True when `label` is the name before `=` of a long-flag token of `args`, `-<letter>` of a short-flag token of
/// `args`, or one of the three fixed texts (D8: a table name or a fixed text, never a value).
fn label_shape(label: &str, args: &[&str]) -> bool {
    if matches!(label, "--" | "<positional>" | "<after -->") {
        return true;
    }
    args.iter().any(|t| {
        if t.starts_with("--") {
            t.split('=').next() == Some(label)
        } else if let Some(body) = t.strip_prefix('-') {
            let mut c = label.chars();
            c.next() == Some('-') && c.clone().count() == 1 && c.next().is_some_and(|x| body.contains(x))
        } else {
            false
        }
    })
}

#[test] // kills: a removal with no label
fn removed_labels_are_empty_only_when_nothing_is_removed() {
    let extra: &[(Agent, &[&str])] = &[
        (Agent::ClaudeCode, &["--model", "m"]),
        (Agent::ClaudeCode, &["--settings", "x"]),
        (Agent::ClaudeCode, &["--add-dir", "a", "b"]),
    ];
    for &(agent, a) in LABEL_CASES.iter().chain(extra) {
        let labels = l(agent, a);
        assert_eq!(labels.is_empty(), removable(agent, &s(a)).is_empty(), "{a:?}");
        for label in &labels {
            assert!(label_shape(label, a), "{label} for {a:?}");
        }
    }
}
