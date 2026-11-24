use std::path::Path;
use zygo_core::Agent;
use zygo_workspace::args::{bypass_flags, events_off, filter, filter_from, flags, has_session_arg, has_user_info, is_flag, occurrences, opencode_subcommand, removable, removed_labels, session_flags, Filtered};

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
        &["-d", "hooks"], &["--debug=hooks"], &["--debug"], &["--permission-mode", "plan"],
        &["--dangerously-skip-permissions"], &["--allow-dangerously-skip-permissions"], &["--settings", "s.json"],
        &["--future-flag"], &["--future=1"], &["-xyz"], &["--model"],
    ];
    for a in kept {
        assert_eq!(cc(a), Filtered { kept: s(a), dropped_flags: vec![] }, "{a:?}");
        assert_eq!(removable(Agent::ClaudeCode, &s(a)), s(&[]), "{a:?}");
    }
    assert_eq!(cc(&["--add-dir", "a", "b", "--verbose", "fix it"]).kept, s(&["--add-dir", "a", "b", "--verbose"]));
    assert_eq!(cc(&["--future-flag", "x"]).kept, s(&["--future-flag"]));
    // short clusters (FIX-31 B1): a removed letter removes the whole cluster and its value
    assert_eq!(cc(&["-pd"]).kept, s(&[]));
    assert_eq!(removable(Agent::ClaudeCode, &s(&["-pd"])), s(&["-pd"]));
    assert_eq!(cc(&["-pdhooks"]).kept, s(&[]));
    assert_eq!(cc(&["-pn", "my name"]).kept, s(&[]));
    assert_eq!(cc(&["--tmux=classic"]).kept, s(&[]));
}

#[test] // AU30 OpenCode rules
fn opencode_rules() {
    let gone: &[&[&str]] = &[
        // `-sses_1` is removed whole, but not because `-s` takes `ses_1`: by the yargs-parser rule (OpenCode 1.19.6
        // @125729461) each `s` is set with no value, so the cluster holds the session flag three times (FIX-32 B1; see
        // opencode_clusters_follow_the_yargs_rule)
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
    // the Claude Code table does not apply: -r is an unknown flag here, and OpenCode gives it the next token (FIX-31)
    assert_eq!(oc(&["-r", "abc"]).kept, s(&["-r", "abc"]));
    assert_eq!(oc(&["--mcp-config", INLINE]), Filtered { kept: s(&["--mcp-config", INLINE]), dropped_flags: vec![] }); // not an OpenCode flag: its value is kept as typed, as --mcp-config=<json> is
}

/// The long name and the values of each table flag that the walk of `agent` meets in `a`.
fn occ(agent: Agent, a: &[&str]) -> Vec<(&'static str, Vec<String>)> {
    occurrences(agent, &s(a)).into_iter().map(|o| (o.name, o.values)).collect()
}

#[test] // FIX-32 B1: OpenCode reads a short cluster by the yargs-parser rule (opencode 1.19.6 @125729461, quoted in
        // docs/work/phase2/nodes/FIX-32.md); Claude Code keeps Commander's rule
fn opencode_clusters_follow_the_yargs_rule() {
    let none: Vec<String> = vec![];
    // `-mc`: `m` before a word character is set with no value; the last letter `c` is --continue
    assert_eq!(occ(Agent::OpenCode, &["-mc"]), [("--model", none.clone()), ("--continue", none.clone())]);
    assert_eq!(oc(&["-mc"]), Filtered::default());
    assert_eq!(removable(Agent::OpenCode, &s(&["-mc"])), s(&["-mc"]));
    assert!(has_session_arg(Agent::OpenCode, &s(&["-mc"])), "ws run restores no --session for these args (ws.rs launch_member)");
    // `-ms ses_1`: `m` has no value; the last letter `s` takes the next token
    assert_eq!(occ(Agent::OpenCode, &["-ms", "ses_1"]), [("--model", none.clone()), ("--session", s(&["ses_1"]))]);
    assert_eq!(oc(&["-ms", "ses_1"]), Filtered::default());
    assert!(has_session_arg(Agent::OpenCode, &s(&["-ms", "ses_1"])));
    // `-sses_1`: s, s, e, s and `_` are each followed by a word character, so each is set with no value; the last
    // letter `1` takes the next token by its arity (there is none)
    assert_eq!(occ(Agent::OpenCode, &["-sses_1"]), [("--session", none.clone()), ("--session", none.clone()), ("--session", none.clone())]);
    assert_eq!(oc(&["-sses_1"]), Filtered::default());
    assert!(has_session_arg(Agent::OpenCode, &s(&["-sses_1"])));
    // `=`: the letter takes the rest after it
    assert_eq!(occ(Agent::OpenCode, &["-m=zygo/lead"]), [("--model", s(&["zygo/lead"]))]);
    assert_eq!(oc(&["-m=zygo/lead"]).kept, s(&["-m=zygo/lead"]));
    // an alphabetic letter before a number takes the number
    assert_eq!(occ(Agent::OpenCode, &["-m5"]), [("--model", s(&["5"]))]);
    assert_eq!(oc(&["-m5"]).kept, s(&["-m5"]));
    // a non-word character: the letter takes the rest of the token, that character included
    assert_eq!(occ(Agent::OpenCode, &["-m/x"]), [("--model", s(&["/x"]))]);
    assert_eq!(occ(Agent::OpenCode, &["-m-"]), [("--model", s(&["-"]))]);
    // the loop does not see the last character of the token as the next character: `/` is the last letter here
    assert_eq!(occ(Agent::OpenCode, &["-m/"]), [("--model", none.clone())]);
    assert_eq!(oc(&["-m/", "v"]).kept, s(&["-m/", "v"]));
    // a letter that took the rest ends the cluster: the next token is not a value
    assert_eq!(oc(&["-m=a", "fix it"]).kept, s(&["-m=a"]));
    assert_eq!(oc(&["-cm=a"]), Filtered::default());
    // an unlisted letter follows the same rule: `x` takes `=1`, so no letter is left to take the next token
    assert_eq!(oc(&["-x=1", "fix it"]).kept, s(&["-x=1"]));
    assert_eq!(oc(&["-xy", "v"]).kept, s(&["-xy", "v"]));
    // Claude Code keeps Commander's rule: a value letter takes the rest of the token and ends the cluster
    assert_eq!(occ(Agent::ClaudeCode, &["-ncx"]), [("--name", s(&["cx"]))]);
    assert_eq!(cc(&["-ncx"]).kept, s(&["-ncx"]));
    assert!(!has_session_arg(Agent::ClaudeCode, &s(&["-ncx"])));
    assert_eq!(occ(Agent::ClaudeCode, &["-rabc"]), [("--resume", s(&["abc"]))]);
    assert_eq!(occ(Agent::ClaudeCode, &["-d=x"]), [("--debug", s(&["=x"]))]);
}

#[test] // FIX-32 B2, W-C7, AU30: an inline JSON value of --settings is removed with the flag; a file path is kept as typed
fn settings_keeps_a_path_and_removes_inline_json() {
    let json = r#"{"env":{"K":"v"}}"#;
    assert_eq!(cc(&["--settings", json]), Filtered { kept: vec![], dropped_flags: s(&["--settings"]) });
    assert_eq!(cc(&["--settings", json, "--model", "m"]).kept, s(&["--model", "m"]));
    assert_eq!(removable(Agent::ClaudeCode, &s(&["--settings", json])), s(&["--settings", json]));
    let here = std::env::current_dir().unwrap();
    assert_eq!(removed_labels(Agent::ClaudeCode, &s(&["--settings", json]), &here), ["--settings"]);
    assert_eq!(removed_labels(Agent::ClaudeCode, &s(&[&format!("--settings= {json}")]), &here), ["--settings"]);
    assert_eq!(cc(&[&format!("--settings={json}")]), Filtered { kept: vec![], dropped_flags: s(&["--settings"]) });
    assert_eq!(cc(&["--settings", &format!(" \n{json}")]).kept, s(&[]));
    // a file path is kept as typed: not made absolute, no existence check
    for a in [&["--settings=/abs/s.json"][..], &["--settings", "./s.json"][..], &["--settings", "s.json"][..], &["--settings"][..]] {
        assert_eq!(cc(a), Filtered { kept: s(a), dropped_flags: vec![] }, "{a:?}");
        assert_eq!(removed_labels(Agent::ClaudeCode, &s(a), &here), Vec::<String>::new(), "{a:?}");
    }
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
    rows.map(|l| claude_spec(l[2..].split("  ").next().unwrap())).collect()
}

/// One Claude Code option text (`-n, --name <name>`): the words that start with `-` are the names (a `,` at the end is
/// cut); the first other word is the value mark: none = boolean, `[x]` optional, `<x...>` variadic, `<x>` required.
fn claude_spec(spec: &str) -> (Vec<String>, Arity) {
    let names = spec.split_whitespace().filter(|w| w.starts_with('-')).map(|w| w.trim_end_matches(',').to_string()).collect();
    let arity = match spec.split_whitespace().find(|w| !w.starts_with('-')) {
        None => Arity::None,
        Some(m) if m.starts_with('[') => Arity::Optional,
        Some(m) if m.ends_with("...>") => Arity::Variadic,
        Some(m) => {
            assert!(m.starts_with('<'), "{spec}");
            Arity::Required
        }
    };
    (names, arity)
}

/// Every root option of the Claude Code 2.1.331 binary (`fixtures/claude-code/options-2.1.331.txt`, FIX-31): the third
/// tab-separated field of each row that is not empty and not a `#` comment.
fn binary_options() -> Vec<(Vec<String>, Arity)> {
    fixture("claude-code/options-2.1.331.txt")
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| claude_spec(l.split('\t').nth(2).unwrap()))
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
        // FIX-31 D4: --dangerously-load-development-channels is a removed variadic flag; every value up to the next
        // flag goes with it
        (true, Arity::Variadic) => assert_eq!(f(&[name, "aa", "bb", "--model", "zz"]), s(&["--model", "zz"]), "{name}"),
    }
}

const CC_REMOVED: &[&str] = &[
    "-c", "--continue", "-r", "--resume", "--session-id", "--fork-session", "--from-pr", "--cloud", "--teleport", "-p", "--print",
    "--system-prompt", "--append-system-prompt",
];
const CC_CONTENT: &[&str] = &["--agents", "--mcp-config", "--json-schema", "--plugin-url"];
const OC_REMOVED: &[&str] = &["-c", "--continue", "-s", "--session", "--fork", "--prompt"];
/// The hidden Claude Code 2.1.331 options that are removed with their value (FIX-31 D4 `Remove` rows), and `--remote`
/// (a session row, as `--cloud`).
const CC_HIDDEN_REMOVED: &[&str] = &[
    "--append-subagent-system-prompt", "--append-subagent-system-prompt-file", "--plan-mode-instructions", "--inherit-permission-mode",
    "--watch-artifact", "--watch-artifact-no-autoreact", "--prefill", "--deep-link-repo", "--deep-link-last-fetch", "--prefill-b64",
    "--deep-link-cwd-b64", "--resume-session-at", "--resume-drops-turn", "--rewind-files", "--workload", "--client-data-url",
    "--managed-settings", "--project-config-root", "--plugin-dir-no-mcp", "--messaging-socket-path",
    "--dangerously-load-development-channels", "--agent-id", "--agent-name", "--team-name", "--agent-color", "--parent-session-id",
    "--lead-verify-key", "--agent-type", "--sdk-url", "--runner-declares", "--session-began-as-copy", "--initialize-fields",
    "--forward-home-settings", "--attach-serve", "--correlation-id", "--metadata", "--ref", "--on-branch", "--remote",
];
/// The Claude Code path flags: kept, with the value made absolute.
const CC_PATH: &[&str] = &["--system-prompt-file", "--append-system-prompt-file"];
/// The Claude Code 2.1.331 booleans that are removed (FIX-31 D7 table; `--bg` and `--background` are one row).
const CC_BOOL_REMOVED: &[&str] = &[
    "--init-only", "--desktop", "--bg", "--background", "--init", "--maintenance", "--session-mirror", "--await-claim",
    "--await-initialize", "--enable-auth-status", "--deep-link-origin", "--reply-on-resume", "--allow-other-models",
    "--managed-settings-in-initialize", "--host-auth-is-org-sign-in", "--ignore-persisted-trust", "--tmux",
    "--agents-view-if-allowed", "--plan-mode-required", "--hold-repository-while-parked",
];

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
            // --bg, --background, --desktop and --tmux are removed booleans of the pinned help (FIX-31 D7)
            probe(Agent::ClaudeCode, n, *arity, CC_REMOVED.contains(&n.as_str()) || CC_BOOL_REMOVED.contains(&n.as_str()));
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

/// The ASCII letters that the table of agent knows as a short flag, sorted: the walk reports -<letter> as a table flag.
fn table_letters(agent: Agent) -> Vec<char> {
    let known = |c: &char| !occurrences(agent, &s(&[&format!("-{c}")])).is_empty();
    ('0'..='9').chain('A'..='Z').chain('a'..='z').filter(known).collect()
}

#[test] // AU30, W-C7, review focus 4: a short flag that a later pin adds must be classified before the test passes
fn every_short_flag_in_the_pinned_help_is_classified() {
    // An unknown letter is kept as typed and takes a value only by the rule of its agent, so each short flag of the
    // help must be in the table of its agent. The exempt letters are `h` and `v` only (FIX-31 B1).
    let claude = short_letters(&claude_options());
    assert_eq!(claude, ['c', 'd', 'h', 'n', 'p', 'r', 'v', 'w']);
    assert_eq!(table_letters(Agent::ClaudeCode), ['c', 'd', 'n', 'p', 'r', 'w']);
    let opencode = short_letters(&opencode_options());
    assert_eq!(opencode, ['c', 'h', 'm', 's', 'v']);
    assert_eq!(table_letters(Agent::OpenCode), ['c', 'f', 'm', 'p', 's', 'u']);
    // the help set is the table set plus the exempt letters; -f, -p and -u are options of run and attach (FIX-31 D3), not of the default help
    for (help, agent, sub) in [(claude, Agent::ClaudeCode, &[][..]), (opencode, Agent::OpenCode, &['f', 'p', 'u'][..])] {
        let mut want: Vec<char> = table_letters(agent).into_iter().filter(|c| !sub.contains(c)).collect();
        want.extend(['h', 'v']);
        want.sort_unstable();
        assert_eq!(help, want, "{agent:?}");
    }
    // FIX-31 B1: every letter of a cluster is looked up, so a cluster that holds a session or print letter is removed whole, also after -h or -v.
    assert_eq!(cc(&["-vp"]), Filtered::default());
    assert_eq!(cc(&["-hc"]), Filtered::default());
    assert_eq!(removable(Agent::ClaudeCode, &s(&["-vp", "-hc"])), s(&["-vp", "-hc"]));
    assert_eq!(oc(&["-vc"]), Filtered::default());
    assert_eq!(oc(&["-hc"]), Filtered::default());
    assert_eq!(removable(Agent::OpenCode, &s(&["-vc", "-hc"])), s(&["-vc", "-hc"]));
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
    assert_eq!(session_flags(Agent::ClaudeCode), ["-c", "--continue", "-r", "--resume", "--session-id", "--fork-session", "--from-pr", "--cloud", "--teleport", "--remote"]);
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
    // FIX-32 B1: by the yargs-parser rule (OpenCode 1.19.6 @125729461) `-sses_1` is three `-s` with no value; `-s=ses_1`
    // gives `-s` the value
    let none: Vec<String> = vec![];
    assert_eq!(got, vec![("--model", s(&["zygo/lead"])), ("--session", none.clone()), ("--session", none.clone()), ("--session", none)]);
    let got: Vec<(&str, Vec<String>)> = occurrences(Agent::OpenCode, &s(&["-s=ses_1", "--", "-s"])).into_iter().map(|o| (o.name, o.values)).collect();
    assert_eq!(got, vec![("--session", s(&["ses_1"]))]);
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

#[test] // FIX-24 D2, W§7.2 member.launch.events_off: Claude Code by the walk of `filter`, OpenCode by `opencode_subcommand`; a flag value and a token after `--` count as nothing
fn events_off_follows_the_walk_of_filter() {
    let on: &[(Agent, &[&str])] = &[
        (Agent::OpenCode, &["--pure"]),
        (Agent::OpenCode, &["--pure=true"]),
        (Agent::OpenCode, &["--model", "m", "--pure"]),
        (Agent::OpenCode, &["attach", "http://h"]),
        (Agent::OpenCode, &["--log-level", "DEBUG", "attach", "http://h"]),
        (Agent::OpenCode, &["attach", "--pure", "http://h"]),
        (Agent::OpenCode, &["run", "--attach", "http://h"]),
        (Agent::OpenCode, &["run", "--attach=http://h"]),
        (Agent::OpenCode, &["run", "x", "--attach", "http://h"]),
        (Agent::ClaudeCode, &["--bare"]),
        (Agent::ClaudeCode, &["--model", "m", "--bare"]),
    ];
    for &(agent, a) in on {
        assert!(events_off(agent, &s(a)), "events off: {agent:?} {a:?}");
    }
    let off: &[(Agent, &[&str])] = &[
        (Agent::OpenCode, &[]),
        (Agent::OpenCode, &["--attach"]), // a saved `run --attach`: the TUI ignores it (P12)
        (Agent::OpenCode, &["--attach=http://h"]),
        (Agent::OpenCode, &["--pure-x"]),
        (Agent::OpenCode, &["--attachx"]),
        (Agent::OpenCode, &["--prompt", "attach"]),
        (Agent::OpenCode, &["--", "attach"]),
        (Agent::OpenCode, &["--", "--pure"]),
        (Agent::OpenCode, &["run", "x"]),
        (Agent::OpenCode, &["-s", "x", "run"]),
        (Agent::ClaudeCode, &["attach"]),
        (Agent::ClaudeCode, &["--pure"]),
        (Agent::ClaudeCode, &["--", "--bare"]),
        (Agent::ClaudeCode, &["--model", "--bare"]), // the value of --model (a required value takes the next token)
    ];
    for &(agent, a) in off {
        assert!(!events_off(agent, &s(a)), "not events off: {agent:?} {a:?}");
    }
}

#[test] // FIX-24 diff review round 2, finding 1: OpenCode 1.19.6 gives an unlisted long flag without `=` the next token
fn events_off_takes_the_value_of_an_unlisted_opencode_flag() {
    let on: &[&[&str]] = &[
        &["--attach", "http://h", "run", "msg"],
        &["--zzunknown", "X", "attach", "http://h"],
        &["--zzunknown", "--pure"], // a token that starts with `-` is not the value
        &["--zzunknown=X", "attach", "http://h"], // `=` carries the value, so `attach` is the subcommand
    ];
    for a in on {
        assert!(events_off(Agent::OpenCode, &s(a)), "events off: {a:?}");
    }
    let off: &[&[&str]] = &[
        &["--zzunknown", "attach"], // `attach` is the value of --zzunknown, not the subcommand
        &["--zzunknown", "run", "--attach", "http://h"], // `run` is the value; no subcommand
    ];
    for a in off {
        assert!(!events_off(Agent::OpenCode, &s(a)), "not events off: {a:?}");
    }
}

#[test] // FIX-24 diff review round 3, finding 2: one case per row of the OpenCode 1.19.6 matrix (`<flags> attach --help`
        // and `<flags> run --help`, compared with `cmp` to `attach --help` and `run --help`; nodes/FIX-24.md)
fn opencode_subcommand_follows_the_opencode_1_19_6_parser() {
    // Rows "F S": F takes no value, so S stays the subcommand. Rows "F X S": X is the subcommand, not S.
    for f in ["--print-logs", "--no-replay", "--no-zz", "--pure"] {
        for sub in ["attach", "run"] {
            assert_eq!(opencode_subcommand(&s(&[f, sub])), Some((1, sub)), "{f} {sub}");
            assert_eq!(opencode_subcommand(&s(&[f, "X", sub])), Some((1, "X")), "{f} X {sub}");
        }
    }
    // Rows "F X S": F takes X, so S is the subcommand. Rows "F S": F takes S, so there is no subcommand.
    for f in ["--mini", "--mdns", "--auto", "--zzunknown", "-x"] {
        for sub in ["attach", "run"] {
            assert_eq!(opencode_subcommand(&s(&[f, "X", sub])), Some((2, sub)), "{f} X {sub}");
            assert_eq!(opencode_subcommand(&s(&[f, sub])), None, "{f} {sub}");
        }
    }
    for f in ["-c", "--continue", "--fork", "-xy"] {
        assert_eq!(opencode_subcommand(&s(&[f, "X", "attach"])), Some((2, "attach")), "{f} X attach");
        assert_eq!(opencode_subcommand(&s(&[f, "attach"])), None, "{f} attach");
    }
    type Row<'a> = (&'a [&'a str], Option<(usize, &'a str)>);
    let rows: &[Row] = &[
        // the global booleans of help.txt that the matrix could see (`-v` and `--version` print and stop)
        (&["-v", "attach"], Some((1, "attach"))),
        (&["-v", "X", "attach"], Some((1, "X"))),
        (&["--version", "attach"], Some((1, "attach"))),
        (&["-h", "attach"], Some((1, "attach"))),
        (&["-h", "X", "attach"], Some((1, "X"))),
        (&["--help", "X", "attach"], Some((1, "X"))),
        // a table flag with a value
        (&["--model", "X", "attach"], Some((2, "attach"))),
        // a flag takes `true` or `false`: an unknown flag and a default-command flag as a value, a global boolean too
        (&["--zzunknown", "true", "attach"], Some((2, "attach"))),
        (&["--zzunknown", "false", "attach"], Some((2, "attach"))),
        (&["--mini", "true", "attach"], Some((2, "attach"))),
        (&["--mini", "false", "attach"], Some((2, "attach"))),
        (&["--pure", "true", "attach"], Some((2, "attach"))),
        (&["--pure", "false", "attach"], Some((2, "attach"))),
        (&["--print-logs", "true", "attach"], Some((2, "attach"))),
        (&["--print-logs", "false", "attach"], Some((2, "attach"))),
        // a `--no-` flag does not take `true`
        (&["--no-zz", "true", "attach"], Some((1, "true"))),
        // a token that starts with `-` is not a value
        (&["--zzunknown", "--print-logs", "attach"], Some((2, "attach"))),
        (&["--zzunknown", "-x", "attach"], None),
        // `=` carries the value
        (&["-x=X", "attach"], Some((1, "attach"))),
        (&["--zzunknown=X", "attach"], Some((1, "attach"))),
        // a flag takes one token only
        (&["--cors", "a", "b", "attach"], Some((2, "b"))),
        // `--` ends the flags: no subcommand after it
        (&["--", "attach"], None),
    ];
    for (a, want) in rows {
        assert_eq!(opencode_subcommand(&s(a)), *want, "{a:?}");
    }
}

// ---- FIX-31: the value of a flag that the table does not hold; clusters; the complete Claude Code table ----

#[test] // FIX-31, AU30: an OpenCode flag that the table does not hold takes the next token unless it starts with -
fn an_unlisted_opencode_flag_keeps_its_value() {
    assert_eq!(oc(&["--zzunknown", "X"]), Filtered { kept: s(&["--zzunknown", "X"]), dropped_flags: vec![] }, "--zzunknown X");
    assert_eq!(removable(Agent::OpenCode, &s(&["--zzunknown", "X"])), s(&[]), "--zzunknown X: nothing removed");
    assert_eq!(oc(&["--zzunknown", "X", "proj"]).kept, s(&["--zzunknown", "X"]), "--zzunknown X proj");
    assert_eq!(removable(Agent::OpenCode, &s(&["--zzunknown", "X", "proj"])), s(&["proj"]), "--zzunknown X proj: proj removed");
    assert_eq!(oc(&["--zzunknown", "--model", "m"]).kept, s(&["--zzunknown", "--model", "m"]), "a flag is not the value");
    assert_eq!(oc(&["--zzunknown"]).kept, s(&["--zzunknown"]), "no next token");
    assert_eq!(oc(&["--zzunknown=X", "proj"]).kept, s(&["--zzunknown=X"]), "= carries the value");
    assert_eq!(oc(&["-x", "X"]).kept, s(&["-x", "X"]), "-x X");
    assert_eq!(oc(&["-xy", "X"]).kept, s(&["-xy", "X"]), "-xy X: the last letter takes the value");
    assert_eq!(oc(&["-r", "abc"]).kept, s(&["-r", "abc"]), "-r abc");
    assert_eq!(oc(&["-x=X", "proj"]).kept, s(&["-x=X"]), "-x=X proj");
    assert_eq!(oc(&["--zzunknown", "X", "--", "Y"]).kept, s(&["--zzunknown", "X"]), "--zzunknown X -- Y");
    assert_eq!(removable(Agent::OpenCode, &s(&["--zzunknown", "X", "--", "Y"])), s(&["--", "Y"]), "--zzunknown X -- Y: removed");
    assert_eq!(oc(&["--zzunknown", "--", "Y"]).kept, s(&["--zzunknown"]), "-- is not the value");
    assert_eq!(removed_labels(Agent::OpenCode, &s(&["--zzunknown", "X", "proj"]), Path::new("")), s(&["<positional>"]), "labels");
    assert_eq!(filter_from(Agent::OpenCode, &s(&["--zzunknown", "X"]), &std::env::current_dir().unwrap()), oc(&["--zzunknown", "X"]), "filter_from");
}

#[test] // FIX-31, AU30, W-C7: a boolean of an OpenCode command keeps no value; a run message is never saved
fn an_opencode_boolean_takes_only_true_or_false() {
    for n in [
        "-h", "--help", "-v", "--version", "--print-logs", "--pure", "--mdns", "--auto", "--mini", "--replay", "--share", "--thinking",
        "-i", "--interactive", "--yolo", "--dangerously-skip-permissions", "--demo", "--major", "--keep-config", "--keep-data",
        "--dry-run", "--force", "--verbose", "--refresh", "--sanitize", "-g", "--global",
    ] {
        assert_eq!(oc(&[n, "msg"]).kept, s(&[n]), "{n} msg");
        assert_eq!(removable(Agent::OpenCode, &s(&[n, "msg"])), s(&["msg"]), "{n} msg: msg removed");
        assert_eq!(oc(&[n, "true"]).kept, s(&[n, "true"]), "{n} true");
        assert_eq!(oc(&[n, "false"]).kept, s(&[n, "false"]), "{n} false");
    }
    assert_eq!(oc(&["--print-logs", "msg"]).kept, s(&["--print-logs"]), "--print-logs msg");
    assert_eq!(oc(&["--no-x", "msg"]).kept, s(&["--no-x"]), "--no-x msg");
    assert_eq!(removable(Agent::OpenCode, &s(&["--no-x", "msg"])), s(&["msg"]), "--no-x msg: msg removed");
    assert_eq!(oc(&["--no-x", "true"]).kept, s(&["--no-x"]), "--no-x true");
    assert_eq!(oc(&["--print-logs", "true", "msg"]).kept, s(&["--print-logs", "true"]), "--print-logs true msg");
    assert_eq!(oc(&["run", "--thinking", "fix it"]).kept, s(&["--thinking"]), "run --thinking msg");
    assert_eq!(removable(Agent::OpenCode, &s(&["run", "--thinking", "fix it"])), s(&["run", "fix it"]), "run --thinking msg: removed");
    assert_eq!(oc(&["run", "-i", "fix it"]).kept, s(&["-i"]), "run -i msg");
    // clusters (B1)
    assert_eq!(oc(&["run", "-ci", "fix it"]), Filtered::default(), "run -ci msg");
    assert_eq!(removable(Agent::OpenCode, &s(&["run", "-ci", "fix it"])), s(&["run", "-ci", "fix it"]), "run -ci msg: removed");
    assert_eq!(oc(&["run", "-fi", "msg"]), Filtered::default(), "run -fi msg");
    assert_eq!(removable(Agent::OpenCode, &s(&["run", "-fi", "msg"])), s(&["run", "-fi", "msg"]), "run -fi msg: removed");
    assert_eq!(oc(&["-ih", "msg"]).kept, s(&["-ih"]), "-ih msg: a cluster of booleans only");
    assert_eq!(removable(Agent::OpenCode, &s(&["-ih", "msg"])), s(&["msg"]), "-ih msg: msg removed");
    assert_eq!(oc(&["-xi", "X"]).kept, s(&["-xi"]), "-xi X: the last letter is a boolean");
    assert_eq!(removable(Agent::OpenCode, &s(&["-xi", "X"])), s(&["X"]), "-xi X: X removed");
    assert_eq!(oc(&["-ix", "X"]).kept, s(&["-ix", "X"]), "-ix X: the last letter is not a boolean");
}

#[test] // FIX-31 D3, AU30, W-C7: no secret, URL user info or user text of an OpenCode subcommand is saved
fn opencode_secret_options_are_removed_with_their_value() {
    let gone: &[&[&str]] = &[
        &["-p", "pw"], &["--password", "pw"], &["--password=pw"], &["-ppw"], &["--password", "-pw"],
        &["-u", "me"], &["--username", "me"],
        &["--attach", "https://u:p@h"], &["--attach=https://u:p@h"], &["--token", "ghp_x"], &["--url", "https://u:p@h"],
        &["--env", "A=1", "B=2"], &["--env", "A=1", "--env", "B=2"], &["--env=A=1"],
        &["--header", "Authorization: Bearer x"], &["--header", "A: 1", "--header", "B: 2"], &["--header=A: 1"],
        &["-f", "a.txt", "b.txt"], &["--title", "fix it"], &["--dir", "/x"], &["--description", "do it"], &["--path", "p"],
        &["--query", "q"], &["--params", "{}"], &["--event", "e"], &["--cwd", "/x"],
    ];
    for a in gone {
        assert_eq!(oc(a), Filtered::default(), "{a:?}");
        assert_eq!(removable(Agent::OpenCode, &s(a)), s(a), "{a:?}: removed");
    }
    assert_eq!(oc(&["attach", "http://h", "-p", "SECRET", "--mini"]).kept, s(&["--mini"]), "attach -p");
    assert_eq!(oc(&["run", "--attach", "https://u:p@h", "msg"]), Filtered::default(), "run --attach");
    // clusters (B1)
    assert_eq!(oc(&["run", "-ip", "SECRET"]), Filtered::default(), "run -ip SECRET");
    assert_eq!(removable(Agent::OpenCode, &s(&["run", "-ip", "SECRET"])), s(&["run", "-ip", "SECRET"]), "run -ip SECRET: removed");
    assert_eq!(oc(&["-pi", "SECRET"]), Filtered::default(), "-pi SECRET");
    assert_eq!(removable(Agent::OpenCode, &s(&["-pi", "SECRET"])), s(&["-pi", "SECRET"]), "-pi SECRET: removed");
    assert_eq!(removed_labels(Agent::OpenCode, &s(&["-p", "SECRET"]), Path::new("")), s(&["-p"]), "label -p");
    assert_eq!(removed_labels(Agent::OpenCode, &s(&["--attach", "https://u:p@h"]), Path::new("")), s(&["--attach"]), "label --attach");
    assert_eq!(removed_labels(Agent::OpenCode, &s(&["-ip", "SECRET"]), Path::new("")), s(&["-p"]), "label -ip");
}

#[test] // FIX-31 D1: one value function; only the boolean set differs, by the parse context
fn filter_and_opencode_subcommand_share_one_value_rule() {
    for f in ["--zzunknown", "-x", "-xy", "-xh", "--zzunknown=V", "-x=V", "--no-zz", "--no-replay", "--print-logs", "--pure", "-h", "--help", "-v", "--version"] {
        for x in ["X", "true", "false"] {
            assert_eq!(
                removable(Agent::OpenCode, &s(&[f, x])).is_empty(),
                opencode_subcommand(&s(&[f, x, "attach"])) == Some((2, "attach")),
                "{f} {x}"
            );
        }
    }
    for f in ["--mini", "--mdns", "--auto", "--thinking"] {
        assert_eq!(removable(Agent::OpenCode, &s(&[f, "X"])), s(&["X"]), "{f} X: a command boolean takes no value in the filter");
        assert_eq!(opencode_subcommand(&s(&[f, "X", "attach"])), Some((2, "attach")), "{f} X attach: not a global boolean");
    }
}

#[test] // FIX-31 D2, AU30: the table is complete for 2.1.331; Claude Code stops at a flag it does not register
fn an_unlisted_claude_code_flag_takes_no_value() {
    assert_eq!(cc(&["--zzunknown", "X"]), Filtered { kept: s(&["--zzunknown"]), dropped_flags: vec![] }, "--zzunknown X");
    assert_eq!(removable(Agent::ClaudeCode, &s(&["--zzunknown", "X"])), s(&["X"]), "--zzunknown X: X removed");
    assert_eq!(cc(&["--zzunknown", "true"]).kept, s(&["--zzunknown"]), "--zzunknown true");
    assert_eq!(cc(&["--verbose", "fix it"]).kept, s(&["--verbose"]), "--verbose msg");
    assert_eq!(removable(Agent::ClaudeCode, &s(&["--verbose", "fix it"])), s(&["fix it"]), "--verbose msg: msg removed");
    assert_eq!(cc(&["-x", "X"]).kept, s(&["-x"]), "-x X");
    // clusters (B1)
    assert_eq!(cc(&["-pd", "hooks"]), Filtered::default(), "-pd hooks");
    assert_eq!(removable(Agent::ClaudeCode, &s(&["-pd", "hooks"])), s(&["-pd", "hooks"]), "-pd hooks: removed");
    assert_eq!(removed_labels(Agent::ClaudeCode, &s(&["-pd", "hooks"]), Path::new("")), s(&["-p", "-d"]), "-pd hooks: labels");
    assert_eq!(cc(&["-cx", "X"]), Filtered::default(), "-cx X");
    assert_eq!(removable(Agent::ClaudeCode, &s(&["-cx", "X"])), s(&["-cx", "X"]), "-cx X: removed");
    assert_eq!(cc(&["-xd", "hooks"]).kept, s(&["-xd", "hooks"]), "-xd hooks: the last letter -d takes its optional value");
    assert_eq!(cc(&["-xyz", "X"]).kept, s(&["-xyz"]), "-xyz X");
    assert_eq!(removable(Agent::ClaudeCode, &s(&["-xyz", "X"])), s(&["X"]), "-xyz X: X removed");
}

#[test] // FIX-31 D4, AU30
fn a_hidden_claude_code_setting_keeps_its_value() {
    let a = ["--max-turns", "5", "--model", "opus"];
    assert_eq!(cc(&a), Filtered { kept: s(&a), dropped_flags: vec![] }, "--max-turns 5 --model opus");
    assert_eq!(removable(Agent::ClaudeCode, &s(&a)), s(&[]), "--max-turns 5 --model opus: nothing removed");
    assert_eq!(cc(&["--max-turns", "5", "fix it"]).kept, s(&["--max-turns", "5"]), "--max-turns 5 msg");
    assert_eq!(removable(Agent::ClaudeCode, &s(&["--max-turns", "5", "fix it"])), s(&["fix it"]), "--max-turns 5 msg: msg removed");
    assert_eq!(cc(&["--max-turns=5"]).kept, s(&["--max-turns=5"]), "--max-turns=5");
    assert_eq!(cc(&["--channels", "a", "b", "--model", "m"]).kept, s(&["--channels", "a", "b", "--model", "m"]), "--channels a b");
    assert_eq!(cc(&["--rc", "name", "fix it"]).kept, s(&["--rc", "name"]), "--rc name msg");
    assert_eq!(cc(&["--rc", "--model", "m"]).kept, s(&["--rc", "--model", "m"]), "--rc --model m");
}

#[test] // FIX-31 D4, AU30, W-C7
fn a_hidden_claude_code_secret_option_drops_its_value() {
    let gone: &[&[&str]] = &[
        &["--prefill", "fix it"], &["--prefill-b64", "Zm9v"], &["--lead-verify-key", "1.k"], &["--sdk-url", "wss://u:p@h"],
        &["--sdk-url=wss://u:p@h"], &["--managed-settings", "{\"x\":1}"], &["--rewind-files", "m1"], &["--workload", "cron"],
        &["--forward-home-settings", "true"], &["--dangerously-load-development-channels", "a", "b"], &["--remote", "fix it"],
    ];
    for a in gone {
        assert_eq!(cc(a), Filtered::default(), "{a:?}");
        assert_eq!(removable(Agent::ClaudeCode, &s(a)), s(a), "{a:?}: removed");
    }
    assert_eq!(cc(&["--prefill", "fix it", "--model", "m"]).kept, s(&["--model", "m"]), "--prefill msg --model m");
    assert_eq!(removed_labels(Agent::ClaudeCode, &s(&["--lead-verify-key", "K"]), Path::new("")), s(&["--lead-verify-key"]), "label");
    assert!(has_session_arg(Agent::ClaudeCode, &s(&["--remote", "X"])), "--remote is a session argument, as --cloud");
}

#[test] // FIX-31 D4, D6, D7, AU30: every root option of Claude Code 2.1.331 filters by its arity and class
fn every_option_of_the_claude_code_binary_is_classified() {
    let opts = binary_options();
    assert_eq!(opts.len(), 139, "the root options of the 2.1.331 binary");
    let mut booleans: Vec<&str> =
        opts.iter().filter(|o| o.1 == Arity::None).flat_map(|o| o.0.iter().map(String::as_str)).filter(|n| is_flag(Agent::ClaudeCode, n)).collect();
    booleans.sort_unstable();
    assert_eq!(
        booleans,
        [
            "--agents-view-if-allowed", "--allow-other-models", "--await-claim", "--await-initialize", "--background", "--bare", "--bg",
            "--continue", "--deep-link-origin", "--desktop", "--enable-auth-status", "--fork-session", "--hold-repository-while-parked",
            "--host-auth-is-org-sign-in", "--ignore-persisted-trust", "--init", "--init-only", "--maintenance",
            "--managed-settings-in-initialize", "--plan-mode-required", "--print", "--reply-on-resume", "--session-mirror", "--tmux",
            "-c", "-p",
        ],
        "the boolean table rows"
    );
    for (names, arity) in opts.iter().filter(|o| o.1 != Arity::None) {
        for n in names {
            assert!(is_flag(Agent::ClaudeCode, n), "{n} has no table row");
            if !CC_CONTENT.contains(&n.as_str()) && !CC_PATH.contains(&n.as_str()) {
                probe(Agent::ClaudeCode, n, *arity, CC_REMOVED.contains(&n.as_str()) || CC_HIDDEN_REMOVED.contains(&n.as_str()));
            }
        }
    }
    for (names, _) in opts.iter().filter(|o| o.1 == Arity::None) {
        for n in names {
            if is_flag(Agent::ClaudeCode, n) && n != "--bare" {
                assert_eq!(cc(&[n, "zz", "--model", "m"]).kept, s(&["--model", "m"]), "{n}: a boolean in the table is removed");
            } else {
                assert_eq!(cc(&[n, "zz"]).kept, s(&[n]), "{n}: a boolean not in the table is kept, with no value");
            }
        }
    }
    for n in CC_PATH {
        assert_eq!(cc(&[n, "x.md"]).kept, s(&[n, &abs("x.md")]), "{n}");
    }
}

#[test] // FIX-31 D7, AU30: a removed boolean takes no value; the next token is its own argument
fn a_removed_claude_code_boolean_keeps_the_next_token_apart() {
    for n in CC_BOOL_REMOVED {
        assert_eq!(cc(&[n, "--model", "opus"]), Filtered { kept: s(&["--model", "opus"]), dropped_flags: vec![] }, "{n} --model opus");
        assert_eq!(removable(Agent::ClaudeCode, &s(&[n, "--model", "opus"])), s(&[n]), "{n} --model opus: removed");
        assert_eq!(cc(&[n, "--verbose"]).kept, s(&["--verbose"]), "{n} --verbose");
        assert_eq!(removable(Agent::ClaudeCode, &s(&[n, "fix it"])), s(&[n, "fix it"]), "{n} msg: removed");
        assert_eq!(cc(&[&format!("{n}=x")]).kept, s(&[]), "{n}=x");
        assert_eq!(removed_labels(Agent::ClaudeCode, &s(&[n, "--model", "m"]), Path::new("")), s(&[n]), "{n}: labels");
    }
}
