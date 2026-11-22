//! Argument filter, bypass detection and user-info check. Owner: node W4 (AU30, AU70, AU98).
//! A relative path is resolved against the current directory of the process. The functions write no log and no output
//! (W-C7).
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
    flag(&["-c", "--continue"], Arity::None, Class::Remove),
    flag(&["-r", "--resume"], Arity::Optional, Class::Remove),
    flag(&["--session-id"], Arity::Required, Class::Remove),
    flag(&["--fork-session"], Arity::None, Class::Remove),
    flag(&["--from-pr"], Arity::Optional, Class::Remove),
    flag(&["--cloud"], Arity::Optional, Class::Remove),
    flag(&["--teleport"], Arity::Optional, Class::Remove),
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
];

/// `fixtures/opencode/help.txt`. A flag that is not here takes no value and is kept as typed.
const OPENCODE: &[Flag] = &[
    // removed: session and prompt-text arguments
    flag(&["-c", "--continue"], Arity::None, Class::Remove),
    flag(&["-s", "--session"], Arity::Required, Class::Remove),
    flag(&["--fork"], Arity::None, Class::Remove),
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

/// The absolute form of `value` as text, or `None` when there is none.
fn absolute(value: &str) -> Option<String> {
    std::path::absolute(value).ok()?.into_os_string().into_string().ok()
}

/// The text to save for one value of a flag of `class`, or `None` when the value is removed.
fn saved_value(class: Class, value: &str) -> Option<String> {
    match class {
        Class::Remove => None,
        Class::Keep => Some(value.to_string()),
        Class::Path => Some(absolute(value).unwrap_or_else(|| value.to_string())),
        Class::Content => match std::path::absolute(value) {
            Ok(path) if path.is_file() => Some(path.into_os_string().into_string().unwrap_or_else(|_| value.to_string())),
            _ => None,
        },
    }
}

/// Takes the values of `flag`: the attached value when there is one, else the next tokens by arity (`next` moves
/// past them). Then applies the class of the flag.
fn settle(flag: &Flag, attached: Option<&str>, args: &[String], next: &mut usize) -> Outcome {
    if let Some(value) = attached {
        let saved = saved_value(flag.class, value);
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
        match saved_value(flag.class, value) {
            Some(saved) => kept.push(saved),
            None => removed.push(value.clone()),
        }
    }
    let flag_kept = match flag.class {
        Class::Remove => false,
        Class::Keep | Class::Path => true,
        Class::Content => !kept.is_empty(),
    };
    Outcome { flag_kept, attached: None, dropped: flag.class == Class::Content && !removed.is_empty(), kept, removed }
}

fn walk(agent: Agent, args: &[String]) -> Walk {
    let table = match agent {
        Agent::ClaudeCode => CLAUDE_CODE,
        Agent::OpenCode => OPENCODE,
    };
    let mut out = Walk::default();
    let mut next = 0;
    while next < args.len() {
        let token = &args[next];
        next += 1;
        if token == "--" {
            // `--` ends the flags: it and every later token are positionals
            out.removed.extend(args[next - 1..].iter().cloned());
            break;
        }
        if token == "-" || !token.starts_with('-') {
            out.removed.push(token.clone());
            continue;
        }
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
            let outcome = settle(flag, attached, args, &mut next);
            match (outcome.flag_kept, &outcome.attached) {
                (true, Some(value)) => out.kept.push(format!("{name}={value}")),
                (true, None) => out.kept.push(token.clone()),
                (false, _) => out.removed.push(token.clone()),
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
                let outcome = settle(flag, attached, args, &mut next);
                if outcome.flag_kept {
                    kept_letters.push(letter);
                    kept_letters.push_str(outcome.attached.as_deref().unwrap_or(""));
                } else {
                    letter_removed = true;
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
    let walk = walk(agent, args);
    Filtered { kept: walk.kept, dropped_flags: walk.dropped_flags }
}

/// The tokens of `args` that [`filter`] does not keep, as typed, in input order. A short-flag cluster that loses a
/// letter is given one time as typed.
pub fn removable(agent: Agent, args: &[String]) -> Vec<String> {
    walk(agent, args).removed
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
