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

/// `fixtures/claude-code/help.txt`. A flag that is not here takes no value and is kept as typed.
const CLAUDE_CODE: &[Flag] = &[
    // removed: session, print and prompt-text arguments
    flag(&["-c", "--continue"], Arity::None, Class::Session),
    flag(&["-r", "--resume"], Arity::Optional, Class::Session),
    flag(&["--session-id"], Arity::Required, Class::Session),
    flag(&["--fork-session"], Arity::None, Class::Session),
    flag(&["--from-pr"], Arity::Optional, Class::Session),
    flag(&["--cloud"], Arity::Optional, Class::Session),
    flag(&["--teleport"], Arity::Optional, Class::Session),
    flag(&["-p", "--print"], Arity::None, Class::Remove),
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
    flag(&["--settings"], Arity::Required, Class::Keep),
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
    // kept, no value: listed so that `flags` and `occurrences` report it (the adapter logs it, AU30 keeps it)
    flag(&["--bare"], Arity::None, Class::Keep),
];

/// `fixtures/opencode/help.txt`. A flag that is not here takes no value and is kept as typed.
const OPENCODE: &[Flag] = &[
    // removed: session and prompt-text arguments
    flag(&["-c", "--continue"], Arity::None, Class::Session),
    flag(&["-s", "--session"], Arity::Required, Class::Session),
    flag(&["--fork"], Arity::None, Class::Session),
    flag(&["--prompt"], Arity::Required, Class::Remove),
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
    }
}

/// Takes the values of `flag`: the attached value when there is one, else the next tokens by arity (`next` moves
/// past them). Then applies the class of the flag.
fn settle(flag: &Flag, attached: Option<&str>, args: &[String], next: &mut usize, base: Option<&Path>) -> Outcome {
    if let Some(value) = attached {
        let saved = saved_value(flag.class, value, base);
        return Outcome {
            flag_kept: saved.is_some(),
            dropped: flag.class == Class::Content && saved.is_none(),
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
    };
    Outcome { flag_kept, attached: None, dropped: flag.class == Class::Content && !removed.is_empty(), kept, removed }
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
            // `-x` or a cluster `-xyz`: each letter is a flag until one takes a value or is not in the table
            let body = &token[1..];
            let mut kept_letters = String::new();
            let mut letter_removed = false;
            for (at, letter) in body.char_indices() {
                let Some(flag) = lookup(table, &format!("-{letter}")) else {
                    kept_letters.push_str(&body[at..]);
                    break;
                };
                let rest = &body[at + letter.len_utf8()..];
                let attached = (flag.arity != Arity::None && !rest.is_empty()).then_some(rest);
                let start = next;
                let outcome = settle(flag, attached, args, &mut next, base);
                out.occurrences.push(occurrence(flag, attached, &args[start..next], token_at..next));
                if outcome.flag_kept {
                    kept_letters.push(letter);
                    kept_letters.push_str(outcome.attached.as_deref().unwrap_or(""));
                } else {
                    letter_removed = true;
                }
                if !outcome.flag_kept || !outcome.removed.is_empty() {
                    out.labels.push(format!("-{letter}"));
                }
                outcomes.push((flag, outcome));
                if flag.arity != Arity::None {
                    break;
                }
            }
            if !kept_letters.is_empty() {
                out.kept.push(format!("-{kept_letters}"));
            }
            if letter_removed {
                out.removed.push(token.clone());
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
/// when it names an existing file; its name goes to `dropped_flags` when a value was dropped. File paths become
/// absolute. All other tokens are kept as typed.
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

/// The OpenCode subcommand in `args` and its index, as OpenCode 1.19.6 reads the tokens before its subcommand (W§7.2;
/// FIX-24 diff review round 3, findings 2 and 5): the first token that does not start with `-` (or is `-`) and is not
/// the value of a flag. A flag that holds `=` takes no next token. A `--no-<x>` flag takes no next token. A global
/// boolean of [`OPENCODE_GLOBAL_BOOLEANS`] takes the next token only when it is `true` or `false`. Every other flag,
/// long or short, listed in the table or not, takes the next token unless it starts with `-`. `--` ends the flags and
/// there is no subcommand after it. `events_off` and the launcher (session arguments after `run`) use this one walk.
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
        if token.contains('=') || token.starts_with("--no-") {
            continue;
        }
        let after = args.get(next).map(String::as_str);
        let takes = if OPENCODE_GLOBAL_BOOLEANS.contains(&token) {
            matches!(after, Some("true" | "false"))
        } else {
            after.is_some_and(|t| !t.starts_with('-'))
        };
        if takes {
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
