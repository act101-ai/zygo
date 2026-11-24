//! Argument filter, bypass detection and user-info check. Owner: node W4 (AU30, AU70, AU98).
//! A relative path is resolved against the current directory of the process, or against the base of `filter_from` and
//! `removed_labels`.
//! The functions write no log and no output (W-C7).
use std::path::{Path, PathBuf};

use zygo_core::Agent;

/// How many value tokens a flag takes, from the pinned `--help` of the agent.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Arity {
    /// No value.
    None,
    /// The next token, whatever it is.
    Required,
    /// The next token when it does not start with `-`.
    Optional,
    /// The next tokens up to the first one that starts with `-`.
    Variadic,
}

/// What AU30 does with a flag and its values.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    /// Kept as typed.
    Keep,
    /// Removed with its values.
    Remove,
    /// Removed with its values; a session argument (W§8.3, AU30).
    Session,
    /// Kept; each value is a path and is stored absolute.
    Path,
    /// A value is kept (absolute) only when it names an existing file.
    Content,
    /// The class depends on the value (FIX-32 B2, W-C7): a value whose first non-space character is `{` is inline JSON
    /// and is removed with the flag, as an inline content value is (the flag name goes to `dropped_flags`); any other
    /// value is a file path and is kept as typed (no absolute form, no existence check). The flag with no value is kept.
    PathOrInline,
}

/// Whether `value` is inline JSON for [`Class::PathOrInline`]: its first non-space character is `{`.
fn inline_json(value: &str) -> bool {
    value.trim_start().starts_with('{')
}

struct Flag {
    /// All names of the flag; the long name is last.
    names: &'static [&'static str],
    arity: Arity,
    class: Class,
}

const fn flag(names: &'static [&'static str], arity: Arity, class: Class) -> Flag {
    Flag { names, arity, class }
}

/// fixtures/claude-code/help.txt, and every other root option of Claude Code 2.1.331 that takes a value or must be removed (fixtures/claude-code/options-2.1.331.txt, FIX-31). The table is complete for 2.1.331; a flag that is not here is kept as typed and takes no value (unlisted_takes_value).
const CLAUDE_CODE: &[Flag] = &[
    // removed: session, print and prompt-text arguments
    flag(&["-c", "--continue"], Arity::None, Class::Session),
    flag(&["-r", "--resume"], Arity::Optional, Class::Session),
    flag(&["--session-id"], Arity::Required, Class::Session),
    flag(&["--fork-session"], Arity::None, Class::Session),
    flag(&["--from-pr"], Arity::Optional, Class::Session),
    flag(&["--cloud"], Arity::Optional, Class::Session),
    flag(&["--teleport"], Arity::Optional, Class::Session),
    flag(&["--remote"], Arity::Optional, Class::Session), // hidden alias of --cloud (FIX-31 minor 2)
    flag(&["-p", "--print"], Arity::None, Class::Remove),
    // removed booleans (FIX-31 D7): one action and exit, an action that a restore would repeat, data of one launch, or the companion of a removed value flag
    flag(&["--init-only"], Arity::None, Class::Remove), // (b) runs the Setup and SessionStart hooks, then exits
    flag(&["--desktop"], Arity::None, Class::Remove), // (b) opens the Claude Desktop app in place of the terminal
    flag(&["--bg", "--background"], Arity::None, Class::Remove), // (b) starts the session in the background and returns
    flag(&["--init"], Arity::None, Class::Remove), // (b) runs the Setup hooks with the init trigger again at each restore
    flag(&["--maintenance"], Arity::None, Class::Remove), // (b) runs the Setup hooks with the maintenance trigger again at each restore
    flag(&["--session-mirror"], Arity::None, Class::Remove), // (a) SDK-internal, set by ProcessTransport
    flag(&["--await-claim"], Arity::None, Class::Remove), // (a) pre-warmed spare of an SDK host
    flag(&["--await-initialize"], Arity::None, Class::Remove), // (a) only from the process that writes the initialize request
    flag(&["--enable-auth-status"], Arity::None, Class::Remove), // (a) auth status messages in SDK mode
    flag(&["--deep-link-origin"], Arity::None, Class::Remove), // (a), (c) a deep-link launch; its value flags are removed
    flag(&["--reply-on-resume"], Arity::None, Class::Remove), // (a), (b) set by /background mid-turn; queries at once on resume
    flag(&["--allow-other-models"], Arity::None, Class::Remove), // (c) goes with --client-data-url, which is removed
    flag(&["--managed-settings-in-initialize"], Arity::None, Class::Remove), // (a), (c) SDK only; requires --await-initialize
    flag(&["--host-auth-is-org-sign-in"], Arity::None, Class::Remove), // (a) desktop hosts only
    flag(&["--ignore-persisted-trust"], Arity::None, Class::Remove), // (a) set by an IDE host for one process
    flag(&["--tmux"], Arity::None, Class::Remove), // (b), (d) creates a tmux session for the worktree; a restore would create another
    flag(&["--agents-view-if-allowed"], Arity::None, Class::Remove), // (a), (d) opens the agents view in place of a session
    flag(&["--plan-mode-required"], Arity::None, Class::Remove), // (a) passed by a team lead with the teammate identity
    flag(&["--hold-repository-while-parked"], Arity::None, Class::Remove), // (a) sent by a host on a spare
    flag(&["--system-prompt"], Arity::Required, Class::Remove),
    flag(&["--append-system-prompt"], Arity::Required, Class::Remove),
    // path value
    flag(&["--system-prompt-file"], Arity::Required, Class::Path),
    flag(&["--append-system-prompt-file"], Arity::Required, Class::Path),
    // content
    flag(&["--agents"], Arity::Required, Class::Content),
    flag(&["--mcp-config"], Arity::Variadic, Class::Content),
    flag(&["--json-schema"], Arity::Required, Class::Content),
    flag(&["--plugin-url"], Arity::Required, Class::Content),
    // kept, required value
    flag(&["--agent"], Arity::Required, Class::Keep),
    flag(&["--autocompact"], Arity::Required, Class::Keep),
    flag(&["--debug-file"], Arity::Required, Class::Keep),
    flag(&["--effort"], Arity::Required, Class::Keep),
    flag(&["--environment"], Arity::Required, Class::Keep),
    flag(&["--fallback-model"], Arity::Required, Class::Keep),
    flag(&["--input-format"], Arity::Required, Class::Keep),
    flag(&["--max-budget-usd"], Arity::Required, Class::Keep),
    flag(&["--model"], Arity::Required, Class::Keep),
    flag(&["-n", "--name"], Arity::Required, Class::Keep),
    flag(&["--output-format"], Arity::Required, Class::Keep),
    flag(&["--permission-mode"], Arity::Required, Class::Keep),
    flag(&["--permission-prompts"], Arity::Required, Class::Keep),
    flag(&["--plugin-dir"], Arity::Required, Class::Keep),
    flag(&["--remote-control-session-name-prefix"], Arity::Required, Class::Keep),
    flag(&["--setting-sources"], Arity::Required, Class::Keep),
    flag(&["--settings"], Arity::Required, Class::PathOrInline), // inline JSON can hold a key (W-C7, FIX-32 B2)
    flag(&["--system-prompt-snapshot"], Arity::Required, Class::Keep),
    // kept, variadic value
    flag(&["--add-dir"], Arity::Variadic, Class::Keep),
    flag(&["--allowedTools", "--allowed-tools"], Arity::Variadic, Class::Keep),
    flag(&["--betas"], Arity::Variadic, Class::Keep),
    flag(&["--disallowedTools", "--disallowed-tools"], Arity::Variadic, Class::Keep),
    flag(&["--file"], Arity::Variadic, Class::Keep),
    flag(&["--tools"], Arity::Variadic, Class::Keep),
    // kept, optional value
    flag(&["-d", "--debug"], Arity::Optional, Class::Keep),
    flag(&["--prompt-suggestions"], Arity::Optional, Class::Keep),
    flag(&["--remote-control"], Arity::Optional, Class::Keep),
    flag(&["-w", "--worktree"], Arity::Optional, Class::Keep),
    // hidden in 2.1.331 (fixtures/claude-code/options-2.1.331.txt), kept with the value: a plain setting (FIX-31 D4)
    flag(&["--thinking"], Arity::Required, Class::Keep), // plain setting: enabled, adaptive or disabled
    flag(&["--thinking-display"], Arity::Required, Class::Keep), // plain setting: a choice
    flag(&["--max-thinking-tokens"], Arity::Required, Class::Keep), // plain setting: a number
    flag(&["--max-turns"], Arity::Required, Class::Keep), // plain setting: a number
    flag(&["--task-budget"], Arity::Required, Class::Keep), // plain setting: a positive integer
    flag(&["--permission-prompt-tool"], Arity::Required, Class::Keep), // plain setting: an MCP tool name
    flag(&["--fast-mode"], Arity::Required, Class::Keep), // plain setting
    flag(&["--advisor"], Arity::Required, Class::Keep), // plain setting: a model name
    flag(&["--proactivity"], Arity::Required, Class::Keep), // plain setting: a choice
    flag(&["--channels"], Arity::Variadic, Class::Keep), // plain setting: MCP server names
    flag(&["--teammate-mode"], Arity::Required, Class::Keep), // plain setting: a choice
    flag(&["--pool"], Arity::Required, Class::Keep), // alias of --environment, which the table keeps
    flag(&["--rc"], Arity::Optional, Class::Keep), // alias of --remote-control, which the table keeps
    // hidden in 2.1.331, removed with the value: a secret, content, a prompt, a path to user content, a URL, or data of one launch (FIX-31 D4)
    flag(&["--append-subagent-system-prompt"], Arity::Required, Class::Remove), // prompt text
    flag(&["--append-subagent-system-prompt-file"], Arity::Required, Class::Remove), // path to user content (a prompt file)
    flag(&["--plan-mode-instructions"], Arity::Required, Class::Remove), // prompt text
    flag(&["--inherit-permission-mode"], Arity::Required, Class::Remove), // set by a parent session for one launch; can carry bypassPermissions past the AU70 printout
    flag(&["--watch-artifact"], Arity::Required, Class::Remove), // id or URL; a URL can hold a token
    flag(&["--watch-artifact-no-autoreact"], Arity::Required, Class::Remove), // same as --watch-artifact
    flag(&["--prefill"], Arity::Required, Class::Remove), // prompt text
    flag(&["--deep-link-repo"], Arity::Required, Class::Remove), // data of one deep-link launch
    flag(&["--deep-link-last-fetch"], Arity::Required, Class::Remove), // data of one deep-link launch
    flag(&["--prefill-b64"], Arity::Required, Class::Remove), // encoded prompt text
    flag(&["--deep-link-cwd-b64"], Arity::Required, Class::Remove), // encoded path of one deep-link launch
    flag(&["--resume-session-at"], Arity::Required, Class::Remove), // belongs to one truncating resume
    flag(&["--resume-drops-turn"], Arity::Required, Class::Remove), // belongs to one truncating resume
    flag(&["--rewind-files"], Arity::Required, Class::Remove), // one action: restores files and exits
    flag(&["--workload"], Arity::Required, Class::Remove), // process-scoped billing tag set by SDK daemon callers for cron work; only with --print
    flag(&["--client-data-url"], Arity::Required, Class::Remove), // URL of a signed document; the help says to keep it out of the process list
    flag(&["--managed-settings"], Arity::Required, Class::Remove), // content: policy settings JSON
    flag(&["--project-config-root"], Arity::Required, Class::Remove), // path to user content, set by a host
    flag(&["--plugin-dir-no-mcp"], Arity::Required, Class::Remove), // path to user content; its caller owns the MCP connections
    flag(&["--messaging-socket-path"], Arity::Required, Class::Remove), // socket of one running server
    flag(&["--dangerously-load-development-channels"], Arity::Variadic, Class::Remove), // development only; shows a confirmation dialog at startup
    flag(&["--agent-id"], Arity::Required, Class::Remove), // teammate identity of one launch
    flag(&["--agent-name"], Arity::Required, Class::Remove), // teammate identity of one launch
    flag(&["--team-name"], Arity::Required, Class::Remove), // teammate identity of one launch
    flag(&["--agent-color"], Arity::Required, Class::Remove), // teammate identity of one launch
    flag(&["--parent-session-id"], Arity::Required, Class::Remove), // id of another session
    flag(&["--lead-verify-key"], Arity::Required, Class::Remove), // key material
    flag(&["--agent-type"], Arity::Required, Class::Remove), // teammate identity of one launch
    flag(&["--sdk-url"], Arity::Required, Class::Remove), // URL of an SDK endpoint; can hold a token or user info
    flag(&["--runner-declares"], Arity::Required, Class::Remove), // set by a runner host for one launch
    flag(&["--session-began-as-copy"], Arity::Required, Class::Remove), // sent by the server for one session
    flag(&["--initialize-fields"], Arity::Required, Class::Remove), // SDK only; needs --await-initialize
    flag(&["--forward-home-settings"], Arity::Required, Class::Remove), // data of one launch ("not saved"); requires --cloud or --environment
    flag(&["--attach-serve"], Arity::Required, Class::Remove), // session id; started by the desktop app, not for interactive use
    flag(&["--correlation-id"], Arity::Required, Class::Remove), // opaque id of one work order
    flag(&["--metadata"], Arity::Required, Class::Remove), // free labels; can hold any text
    flag(&["--ref"], Arity::Required, Class::Remove), // needs --cloud or --environment; in doubt
    flag(&["--on-branch"], Arity::Required, Class::Remove), // needs --cloud or --environment, as --ref
    // kept, no value: listed so that `flags` and `occurrences` report it (the adapter logs it, AU30 keeps it)
    flag(&["--bare"], Arity::None, Class::Keep),
];

/// fixtures/opencode/help.txt, and the subcommand options whose value can hold a secret, a URL, user text or a path. A flag that is not here is kept as typed, with the value that unlisted_takes_value gives it.
const OPENCODE: &[Flag] = &[
    // removed: session and prompt-text arguments
    flag(&["-c", "--continue"], Arity::None, Class::Session),
    flag(&["-s", "--session"], Arity::Required, Class::Session),
    flag(&["--fork"], Arity::None, Class::Session),
    flag(&["--prompt"], Arity::Required, Class::Remove),
    // removed: the options of the subcommands whose value can hold a secret, a URL, user text or a path (W-C7, FIX-31 D3)
    flag(&["-p", "--password"], Arity::Required, Class::Remove), // secret: basic-auth password
    flag(&["-u", "--username"], Arity::Required, Class::Remove), // credential: basic-auth user name
    flag(&["--attach"], Arity::Required, Class::Remove), // URL of a server; can hold user info
    flag(&["--token"], Arity::Required, Class::Remove), // secret: GitHub token
    flag(&["--url"], Arity::Required, Class::Remove), // URL; can hold user info
    flag(&["--env"], Arity::Variadic, Class::Remove), // secret: KEY=VALUE environment of an MCP server
    flag(&["--header"], Arity::Variadic, Class::Remove), // secret: KEY=VALUE HTTP header, often a token
    flag(&["-f", "--file"], Arity::Variadic, Class::Remove), // path to user content attached to the message
    flag(&["--title"], Arity::Required, Class::Remove), // user text: session title, else the truncated prompt
    flag(&["--dir"], Arity::Required, Class::Remove), // path; on attach, a remote path of one launch (in doubt)
    flag(&["--description"], Arity::Required, Class::Remove), // user text: what the agent should do
    flag(&["--path"], Arity::Required, Class::Remove), // path for the agent file (in doubt)
    flag(&["--query"], Arity::Required, Class::Remove), // user text: a file query
    flag(&["--params"], Arity::Required, Class::Remove), // content: tool params as JSON
    flag(&["--event"], Arity::Required, Class::Remove), // GitHub mock event of one run (in doubt)
    flag(&["--cwd"], Arity::Required, Class::Remove), // path: working directory (in doubt)
    // kept, required value
    flag(&["--log-level"], Arity::Required, Class::Keep),
    flag(&["--port"], Arity::Required, Class::Keep),
    flag(&["--hostname"], Arity::Required, Class::Keep),
    flag(&["--mdns-domain"], Arity::Required, Class::Keep),
    flag(&["-m", "--model"], Arity::Required, Class::Keep),
    flag(&["--agent"], Arity::Required, Class::Keep),
    flag(&["--replay-limit"], Arity::Required, Class::Keep),
    // kept, variadic value
    flag(&["--cors"], Arity::Variadic, Class::Keep),
];

fn lookup(table: &'static [Flag], name: &str) -> Option<&'static Flag> {
    table.iter().find(|f| f.names.contains(&name))
}

/// The launch arguments that AU30 lets the workspace save.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filtered {
    /// The arguments to save, in input order. File paths are absolute.
    pub kept: Vec<String>,
    /// The names of the content flags that lost an inline value, each one time (`member.dropped_flags`).
    pub dropped_flags: Vec<String>,
}

/// The result of one walk over the arguments. No `Debug`: `removed` can hold prompt text.
#[derive(Default)]
struct Walk {
    kept: Vec<String>,
    dropped_flags: Vec<String>,
    removed: Vec<String>,
    /// A session flag was met before `--`.
    session: bool,
    /// Each table flag met before `--`, in order.
    occurrences: Vec<Occurrence>,
    /// One label per removal, in token order (FIX-27 D8): a table flag name, `<positional>`, `--` or `<after -->`.
    labels: Vec<String>,
}

/// One table flag that the walk met before `--`. No `Debug`: `values` can hold prompt text.
#[derive(Clone, PartialEq, Eq)]
pub struct Occurrence {
    /// The long name of the flag.
    pub name: &'static str,
    /// Its values as typed: the attached value, or the value tokens after the flag token.
    pub values: Vec<String>,
    /// The tokens of `args` that the flag and its values take; a short-flag cluster shares its token.
    pub tokens: std::ops::Range<usize>,
}

/// What became of one flag of the table and its values.
struct Outcome {
    /// The flag stays in the saved arguments.
    flag_kept: bool,
    /// The attached value to save with a kept flag.
    attached: Option<String>,
    /// The value tokens after the flag token that are saved.
    kept: Vec<String>,
    /// The value tokens after the flag token that are removed, as typed.
    removed: Vec<String>,
    /// A content value was dropped.
    dropped: bool,
}

/// The path that `value` names: relative to `base` when one is given (W7 D32), else as typed (the process cwd).
fn resolved(value: &str, base: Option<&Path>) -> PathBuf {
    base.map_or_else(|| PathBuf::from(value), |b| b.join(value))
}

/// The absolute form of `value` (taken relative to `base` when one is given) as text, or `None` when there is none.
fn absolute(value: &str, base: Option<&Path>) -> Option<String> {
    std::path::absolute(resolved(value, base)).ok()?.into_os_string().into_string().ok()
}

/// The text to save for one value of a flag of `class`, or `None` when the value is removed.
fn saved_value(class: Class, value: &str, base: Option<&Path>) -> Option<String> {
    match class {
        Class::Remove | Class::Session => None,
        Class::Keep => Some(value.to_string()),
        Class::Path => Some(absolute(value, base).unwrap_or_else(|| value.to_string())),
        Class::Content => match std::path::absolute(resolved(value, base)) {
            Ok(path) if path.is_file() => Some(path.into_os_string().into_string().unwrap_or_else(|_| value.to_string())),
            _ => None,
        },
        Class::PathOrInline => (!inline_json(value)).then(|| value.to_string()),
    }
}

/// Whether a removed value of a flag of `class` is a dropped inline value (`dropped_flags`).
fn drops(class: Class) -> bool {
    matches!(class, Class::Content | Class::PathOrInline)
}

/// Takes the values of `flag`: the attached value when there is one, else the next tokens by arity (`next` moves
/// past them). Then applies the class of the flag.
fn settle(flag: &Flag, attached: Option<&str>, args: &[String], next: &mut usize, base: Option<&Path>) -> Outcome {
    if let Some(value) = attached {
        let saved = saved_value(flag.class, value, base);
        return Outcome {
            flag_kept: saved.is_some(),
            dropped: drops(flag.class) && saved.is_none(),
            attached: saved,
            kept: vec![],
            removed: vec![],
        };
    }
    let start = *next;
    match flag.arity {
        Arity::None => {}
        Arity::Required => *next = args.len().min(start + 1),
        Arity::Optional => {
            if args.get(start).is_some_and(|t| !t.starts_with('-')) {
                *next = start + 1;
            }
        }
        Arity::Variadic => {
            while args.get(*next).is_some_and(|t| !t.starts_with('-')) {
                *next += 1;
            }
        }
    }
    let (mut kept, mut removed) = (vec![], vec![]);
    for value in &args[start..*next] {
        match saved_value(flag.class, value, base) {
            Some(saved) => kept.push(saved),
            None => removed.push(value.clone()),
        }
    }
    let flag_kept = match flag.class {
        Class::Remove | Class::Session => false,
        Class::Keep | Class::Path => true,
        Class::Content => !kept.is_empty(),
        Class::PathOrInline => removed.is_empty(),
    };
    Outcome { flag_kept, attached: None, dropped: drops(flag.class) && !removed.is_empty(), kept, removed }
}

fn occurrence(flag: &Flag, attached: Option<&str>, after: &[String], tokens: std::ops::Range<usize>) -> Occurrence {
    let values = match attached {
        Some(v) => vec![v.to_string()],
        None => after.to_vec(),
    };
    Occurrence { name: flag.names[flag.names.len() - 1], values, tokens }
}

fn table(agent: Agent) -> &'static [Flag] {
    match agent {
        Agent::ClaudeCode => CLAUDE_CODE,
        Agent::OpenCode => OPENCODE,
    }
}

/// Whether `s` is a number by the yargs-parser pattern `^-?\d+(\.\d*)?(e-?\d+)?$` (ASCII digits only, as in a
/// JavaScript pattern without the `u` flag).
fn yargs_number(s: &str) -> bool {
    let digits = |t: &str| t.find(|c: char| !c.is_ascii_digit()).unwrap_or(t.len());
    let s = s.strip_prefix('-').unwrap_or(s);
    let int = digits(s);
    if int == 0 {
        return false;
    }
    let mut r = &s[int..];
    if let Some(f) = r.strip_prefix('.') {
        r = &f[digits(f)..];
    }
    match r.strip_prefix('e') {
        Some(e) => {
            let e = e.strip_prefix('-').unwrap_or(e);
            !e.is_empty() && digits(e) == e.len()
        }
        None => r.is_empty(),
    }
}

/// The value that the letter `letter` of a short cluster takes from `rest`, the text after it in the same token, with
/// the separator that a kept token writes before the saved value; `None` when the letter takes no value from the
/// token and the walk goes on to the next letter (FIX-32 B1). The last letter (`rest` empty) never has one here; it
/// takes the next tokens.
/// - Claude Code (Commander): a table letter that takes a value takes all of `rest`; an unlisted letter followed by
///   `=` keeps `rest` as typed. Any other letter takes nothing.
/// - OpenCode, by the yargs-parser short-flag loop of OpenCode 1.19.6 (binary offset 125729461, quoted in
///   `docs/work/phase2/nodes/FIX-32.md`), for a listed or an unlisted letter alike. The loop sees the next character
///   only when it is not the last one of the token. In this order: the next character is `=`: the text after it;
///   `rest` is `-`: `-`; an ASCII letter and a `rest` that is a number ([`yargs_number`]): that number; the next
///   character is not a word character (`[A-Za-z0-9_]`): all of `rest`, that character included; else nothing. (yargs
///   also refuses the number when it is the name of a boolean option; no OpenCode option name is a number.)
fn attached_value<'a>(agent: Agent, flag: Option<&Flag>, letter: char, rest: &'a str) -> Option<(&'static str, &'a str)> {
    if rest.is_empty() {
        return None;
    }
    match agent {
        Agent::ClaudeCode => match flag {
            Some(f) => (f.arity != Arity::None).then_some(("", rest)),
            None => rest.starts_with('=').then_some(("", rest)),
        },
        Agent::OpenCode => {
            let mut chars = rest.chars();
            let next = chars.next()?;
            let seen = chars.next().is_some();
            if seen && next == '=' {
                return Some(("=", &rest[1..]));
            }
            let word = next.is_ascii_alphanumeric() || next == '_';
            (rest == "-" || (letter.is_ascii_alphabetic() && yargs_number(rest)) || (seen && !word)).then_some(("", rest))
        }
    }
}

fn walk(agent: Agent, args: &[String], base: Option<&Path>) -> Walk {
    let table = table(agent);
    let mut out = Walk::default();
    let mut next = 0;
    while next < args.len() {
        let token = &args[next];
        next += 1;
        if token == "--" {
            // `--` ends the flags: it and every later token are positionals
            out.removed.extend(args[next - 1..].iter().cloned());
            out.labels.push("--".to_string());
            out.labels.extend(args[next..].iter().map(|_| "<after -->".to_string()));
            break;
        }
        if token == "-" || !token.starts_with('-') {
            out.removed.push(token.clone());
            out.labels.push("<positional>".to_string());
            continue;
        }
        let token_at = next - 1;
        let mut outcomes: Vec<(&Flag, Outcome)> = vec![];
        if token.starts_with("--") {
            let (name, attached) = match token.split_once('=') {
                Some((name, value)) => (name, Some(value)),
                None => (token.as_str(), None),
            };
            let Some(flag) = lookup(table, name) else {
                out.kept.push(token.clone());
                if unlisted_takes_value(agent, token, args.get(next).map(String::as_str)) {
                    out.kept.push(args[next].clone());
                    next += 1;
                }
                continue;
            };
            let start = next;
            let outcome = settle(flag, attached, args, &mut next, base);
            out.occurrences.push(occurrence(flag, attached, &args[start..next], token_at..next));
            match (outcome.flag_kept, &outcome.attached) {
                (true, Some(value)) => out.kept.push(format!("{name}={value}")),
                (true, None) => out.kept.push(token.clone()),
                (false, _) => out.removed.push(token.clone()),
            }
            if !outcome.flag_kept || !outcome.removed.is_empty() {
                out.labels.push(name.to_string());
            }
            outcomes.push((flag, outcome));
        } else {
            // `-x` or a cluster `-xyz` (FIX-31 B1): every letter is looked up. Whether a letter that is not the last
            // takes the rest of the token as its value, and so ends the cluster, is the rule of the agent's parser
            // ([`attached_value`], FIX-32 B1). The next token belongs to the last letter only: a table letter takes it by
            // its arity, an unlisted one by the rule of that letter; a cluster that a letter ended gives it to nobody.
            // When any table letter is not kept, the whole cluster and its value are removed.
            let body = &token[1..];
            let mut shape = String::new();
            let mut letters: Vec<char> = vec![];
            let mut last_unlisted: Option<char> = None;
            let mut ended = false;
            for (at, letter) in body.char_indices() {
                let rest = &body[at + letter.len_utf8()..];
                let flag = lookup(table, &format!("-{letter}"));
                let attached = attached_value(agent, flag, letter, rest);
                let Some(flag) = flag else {
                    if attached.is_some() {
                        // the letter and its value, as typed
                        shape.push_str(&body[at..]);
                        last_unlisted = None;
                        ended = true;
                        break;
                    }
                    shape.push(letter);
                    last_unlisted = Some(letter);
                    continue;
                };
                last_unlisted = None;
                let value = attached.map(|(_, v)| v);
                let start = next;
                // only the last letter takes the next tokens
                let outcome = if rest.is_empty() { settle(flag, value, args, &mut next, base) } else { settle(flag, value, &[], &mut 0, base) };
                out.occurrences.push(occurrence(flag, value, &args[start..next], token_at..next));
                if outcome.flag_kept {
                    shape.push(letter);
                    if let Some(saved) = &outcome.attached {
                        shape.push_str(attached.map_or("", |(sep, _)| sep));
                        shape.push_str(saved);
                    }
                }
                letters.push(letter);
                outcomes.push((flag, outcome));
                if attached.is_some() || (agent == Agent::ClaudeCode && flag.arity != Arity::None) {
                    ended = true;
                    break;
                }
            }
            let mut taken: Vec<String> = vec![];
            if let Some(letter) = last_unlisted.filter(|_| !ended)
                && unlisted_takes_value(agent, &format!("-{letter}"), args.get(next).map(String::as_str))
            {
                taken.push(args[next].clone());
                next += 1;
            }
            let all_kept = outcomes.iter().all(|(_, o)| o.flag_kept);
            for (letter, (_, outcome)) in letters.iter().zip(outcomes.iter_mut()) {
                if !all_kept {
                    let kept = std::mem::take(&mut outcome.kept);
                    outcome.removed.extend(kept);
                }
                if !all_kept || !outcome.removed.is_empty() {
                    out.labels.push(format!("-{letter}"));
                }
            }
            if all_kept {
                out.kept.push(format!("-{shape}"));
                out.kept.extend(taken);
            } else {
                out.removed.push(token.clone());
                out.removed.extend(taken);
            }
        }
        for (flag, outcome) in outcomes {
            out.kept.extend(outcome.kept);
            out.removed.extend(outcome.removed);
            let name = flag.names[flag.names.len() - 1];
            out.session |= flag.class == Class::Session;
            if outcome.dropped && !out.dropped_flags.iter().any(|d| d == name) {
                out.dropped_flags.push(name.to_string());
            }
        }
    }
    out
}

/// Filters the launch arguments of `agent` by AU30. Removed: session arguments, `-p`/`--print`, prompt-text flags
/// with their values, every positional token, and `--` with all tokens after it. A content flag keeps a value only
/// when it names an existing file; its name goes to `dropped_flags` when a value was dropped. Claude Code `--settings`
/// keeps a file path as typed and loses an inline JSON value the same way ([`Class::PathOrInline`], FIX-32 B2). File
/// paths become absolute. All other tokens are kept as typed; a flag that the table does not hold keeps the next token
/// as its value when [`unlisted_takes_value`] says so, and a short cluster gives the next token to its last letter
/// only (FIX-31), by the cluster rule of the agent's parser ([`attached_value`], FIX-32 B1).
pub fn filter(agent: Agent, args: &[String]) -> Filtered {
    let walk = walk(agent, args, None);
    Filtered { kept: walk.kept, dropped_flags: walk.dropped_flags }
}

/// [`filter`], with each relative path value (a file path or a content file) taken relative to `base` before it is
/// made absolute and before the file check (W7 D32, AU30: a loadout pane's `args` are relative to that pane's folder).
pub fn filter_from(agent: Agent, args: &[String], base: &Path) -> Filtered {
    let walk = walk(agent, args, Some(base));
    Filtered { kept: walk.kept, dropped_flags: walk.dropped_flags }
}

/// The tokens of `args` that [`filter`] does not keep, as typed, in input order. A short-flag cluster that loses a
/// letter is given one time as typed.
pub fn removable(agent: Agent, args: &[String]) -> Vec<String> {
    walk(agent, args, None).removed
}

/// One label for each removal that [`filter_from`] makes with `base`, in token order, by the same walk (FIX-27 D8): a
/// table flag whose token or any value is removed gives its typed name (before `=`; `-<letter>` for each letter of a
/// short-flag token); a removed positional (also `-`) gives `<positional>`; the `--` that ends the flags gives `--`,
/// then `<after -->` for each later token. A flag value is never labelled, so no label holds a value. The list is
/// empty only when the walk removes nothing.
pub fn removed_labels(agent: Agent, args: &[String], base: &Path) -> Vec<String> {
    walk(agent, args, Some(base)).labels
}

/// Whether `args` hold a session argument of `agent` (W§8.3, AU30), by the same walk as [`filter`]: short clusters,
/// flag values and `--` count as they do there.
pub fn has_session_arg(agent: Agent, args: &[String]) -> bool {
    walk(agent, args, None).session
}

/// W§7.2 member.launch.events_off (FIX-24, AU26): the agent of a start with `args` sends no member events. Claude
/// Code: `--bare`, by the walk of [filter] (a flag value and a token after `--` count as nothing). OpenCode: a flag
/// `--pure` or `--attach` is a token before `--` with that name before any `=` (a value never starts with `-`, so such
/// a token is always a flag); the subcommand comes from [`opencode_subcommand`]. Events are off with `--pure`, the
/// subcommand `attach`, or the subcommand `run` with `--attach`.
pub fn events_off(agent: Agent, args: &[String]) -> bool {
    match agent {
        Agent::ClaudeCode => walk(agent, args, None).occurrences.iter().any(|o| o.name == "--bare"),
        Agent::OpenCode => {
            let end = args.iter().position(|a| a == "--").unwrap_or(args.len());
            let has = |n: &str| args[..end].iter().any(|t| t.starts_with('-') && t.split('=').next() == Some(n));
            has("--pure")
                || match opencode_subcommand(args).map(|(_, s)| s) {
                    Some("attach") => true,
                    Some("run") => has("--attach"),
                    _ => false,
                }
        }
    }
}

/// The global boolean options of OpenCode 1.19.6 (`fixtures/opencode/help.txt`, the first block of `Options:`, which
/// also opens `attach --help` and `run --help`). Each takes no value, but a next token `true` or `false` is its value.
const OPENCODE_GLOBAL_BOOLEANS: &[&str] = &["-h", "--help", "-v", "--version", "--print-logs", "--pure"];

/// The boolean options of every OpenCode 1.19.6 command that the OpenCode table does not hold (the `[boolean]` rows of
/// `fixtures/opencode/help.txt` and the boolean options of each subcommand builder in the binary). In the restored
/// default command these take no value; a boolean of another command, which the default command does not know, loses
/// only a value that it ignores (AU30, FIX-31). `--no-<x>` is not listed ([`opencode_takes_value`] gives it no value),
/// and `-f` is not (the `-f`/`--file` row decides it).
const OPENCODE_COMMAND_BOOLEANS: &[&str] = &[
    "-h", "--help", "-v", "--version", "--print-logs", "--pure", "--mdns", "--auto", "--mini", "--replay", "--share", "--thinking", "-i",
    "--interactive", "--yolo", "--dangerously-skip-permissions", "--demo", "--major", "--keep-config", "--keep-data", "--dry-run",
    "--force", "--verbose", "--refresh", "--sanitize", "-g", "--global",
];

/// Whether the OpenCode 1.19.6 flag `token` takes `after`, the next token, as its value (FIX-24 matrix; FIX-31): `=` or
/// a `--no-` prefix: never; the name is the token for a long flag and `-<last letter>` for a short token, because the
/// value goes to the last letter; a name in `booleans` takes only `true` or `false`; any other name takes the next token
/// unless it starts with `-`. The one value rule of [`opencode_subcommand`] and of the AU30 walk.
fn opencode_takes_value(token: &str, after: Option<&str>, booleans: &[&str]) -> bool {
    if token.contains('=') || token.starts_with("--no-") {
        return false;
    }
    let last;
    let name = if token.starts_with("--") {
        token
    } else {
        last = token.chars().last().map(|c| format!("-{c}")).unwrap_or_default();
        last.as_str()
    };
    if booleans.contains(&name) {
        matches!(after, Some("true" | "false"))
    } else {
        after.is_some_and(|t| !t.starts_with('-'))
    }
}

/// Whether a flag (or the last letter of a cluster, as `-<letter>`) that the table of `agent` does not hold takes
/// `after` as its value (AU30, FIX-31). Claude Code: never; the table holds every option with a value of 2.1.331, and
/// Claude Code stops at a flag that it does not register. OpenCode: [`opencode_takes_value`] with
/// [`OPENCODE_COMMAND_BOOLEANS`].
fn unlisted_takes_value(agent: Agent, token: &str, after: Option<&str>) -> bool {
    match agent {
        Agent::ClaudeCode => false,
        Agent::OpenCode => opencode_takes_value(token, after, OPENCODE_COMMAND_BOOLEANS),
    }
}

/// The OpenCode subcommand in `args` and its index, as OpenCode 1.19.6 reads the tokens before its subcommand (W§7.2;
/// FIX-24 diff review round 3, findings 2 and 5): the first token that does not start with `-` (or is `-`) and is not
/// the value of a flag. Whether a flag (long or short, listed in the table or not) takes the next token is
/// [`opencode_takes_value`] with the global booleans of [`OPENCODE_GLOBAL_BOOLEANS`], the value rule that AU30's
/// filter also uses (FIX-31). `--` ends the flags and there is no subcommand after it. `events_off` and the launcher
/// (session arguments after `run`) use this one walk.
pub fn opencode_subcommand(args: &[String]) -> Option<(usize, &str)> {
    let mut next = 0;
    while next < args.len() {
        let token = args[next].as_str();
        next += 1;
        if token == "--" {
            return None;
        }
        if token == "-" || !token.starts_with('-') {
            return Some((next - 1, token));
        }
        if opencode_takes_value(token, args.get(next).map(String::as_str), OPENCODE_GLOBAL_BOOLEANS) {
            next += 1;
        }
    }
    None
}

/// Whether `token` (its text before any `=`) is a flag of the table of `agent` (FIX-27 review round 3: the flag after a
/// secret-named flag is shown as a flag when it is a known one).
pub fn is_flag(agent: Agent, token: &str) -> bool {
    lookup(table(agent), token.split('=').next().unwrap_or(token)).is_some()
}

/// Every name of the session flags of `agent`, in table order.
pub fn session_flags(agent: Agent) -> Vec<&'static str> {
    table(agent).iter().filter(|f| f.class == Class::Session).flat_map(|f| f.names.iter().copied()).collect()
}

/// The long name of each table flag in `args` before `--`, in order; values and positionals are not flags.
pub fn flags(agent: Agent, args: &[String]) -> Vec<&'static str> {
    walk(agent, args, None).occurrences.into_iter().map(|o| o.name).collect()
}

/// Each table flag in `args` before `--`, in order, with its values as typed and its tokens, by the same walk as
/// [`filter`]: a flag value or a token after `--` is never a flag.
pub fn occurrences(agent: Agent, args: &[String]) -> Vec<Occurrence> {
    walk(agent, args, None).occurrences
}

const SKIP_PERMISSIONS: &str = "--dangerously-skip-permissions";
const ALLOW_SKIP_PERMISSIONS: &str = "--allow-dangerously-skip-permissions";
const PERMISSION_MODE: &str = "--permission-mode";
const BYPASS_MODE: &str = "bypassPermissions";
const PERMISSION_MODE_BYPASS: &str = "--permission-mode bypassPermissions";
const PERMISSION_MODE_BYPASS_ATTACHED: &str = "--permission-mode=bypassPermissions";

/// The AU70 bypass forms in `args` (the tokens before the first `--`), each one time, in the order of first use:
/// `--dangerously-skip-permissions`, `--allow-dangerously-skip-permissions` and `--permission-mode bypassPermissions`
/// (also typed with `=`). `yolo` adds `yolo = true` at the end. The comparison is exact. The result holds only
/// these fixed texts, so `up` can print it.
pub fn bypass_flags(args: &[String], yolo: bool) -> Vec<String> {
    let flags = args.iter().position(|t| t == "--").map_or(args, |end| &args[..end]);
    let mut found: Vec<&'static str> = vec![];
    for (at, token) in flags.iter().enumerate() {
        let form = match token.as_str() {
            SKIP_PERMISSIONS => SKIP_PERMISSIONS,
            ALLOW_SKIP_PERMISSIONS => ALLOW_SKIP_PERMISSIONS,
            PERMISSION_MODE_BYPASS_ATTACHED => PERMISSION_MODE_BYPASS,
            PERMISSION_MODE if flags.get(at + 1).is_some_and(|v| v == BYPASS_MODE) => PERMISSION_MODE_BYPASS,
            _ => continue,
        };
        if !found.contains(&form) {
            found.push(form);
        }
    }
    if yolo {
        found.push("yolo = true");
    }
    found.into_iter().map(str::to_string).collect()
}

fn is_scheme(text: &str) -> bool {
    let mut chars = text.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic()) && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// True when the authority of `url` holds an `@` (AU98), also with an empty user. No URL crate: the rule is wider
/// than a URL parser, and a false `true` only keeps the launch a v1 launch. The function writes no log.
pub fn has_user_info(url: &str) -> bool {
    // a URL parser removes each tab, LF and CR, and the C0 controls and spaces at the two ends
    let text: String = url.chars().filter(|c| !matches!(c, '\t' | '\n' | '\r')).collect();
    let text = text.trim_matches(|c: char| c <= ' ');
    // a scheme counts only with a `/` or `\` after its `:`, so `host:port` and `user:pass@host` are not read as one
    let rest = match text.split_once(':') {
        Some((scheme, rest)) if is_scheme(scheme) && rest.starts_with(['/', '\\']) => rest,
        _ => text,
    };
    // a URL parser skips each `/` and `\` before the authority of a special URL (`https:/u@h`, `https:\\u@h`)
    let rest = rest.trim_start_matches(['/', '\\']);
    // a `\` does not end the authority here: `https://h\@evil` is reported
    let authority = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    authority.contains('@')
}
