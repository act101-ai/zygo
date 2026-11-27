//! `zygo loadout list` (W§12; D7) and the secret hiding of `zygo loadout show` and `zygo config show` (W§12; FIX-27).

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use zygo_config::repo_def::REPOSITORY_LOADOUT;
use zygo_config::{Loadout, LoadoutPane, BUILTIN_LOADOUT};
use zygo_core::paths::Paths;
use zygo_core::{esc, Agent};
use zygo_workspace::args::{has_user_info, is_flag, occurrences};
use zygo_workspace::identity::Worktree;
use zygo_workspace::loadout::{self, pane_cwd, LoadoutError, Source};
use zygo_workspace::up::{find_checkout, GIT_BOUND};

use super::workspace_files::{here, State};
use super::{op5_exit, print_err, print_out, user_merged_or_exit};

const USER_INFO_REMOVED: &str = "<user info removed>";
const INLINE_JSON_HIDDEN: &str = "<inline JSON not shown>";
const VALUE_HIDDEN: &str = "<value not shown>";
/// The AU30 content flags and `--settings`: a value is shown only when it names a regular file (D4).
const CONTENT_FLAGS: [&str; 5] = ["--agents", "--mcp-config", "--json-schema", "--plugin-url", "--settings"];
/// A flag name, a name inside a text or a query parameter name whose lower-cased text holds one of these hides its value
/// (D3; review round 2, findings 4, 5(c), 5(d); round 3, finding 6 adds `cookie`; `session` is not one, because
/// `--session-id` and `--resume` values are no secrets).
const SECRET_NAMES: [&str; 7] = ["key", "token", "secret", "password", "auth", "bearer", "cookie"];

/// D1 (a): a scheme of 2 or more characters followed by `:/` or `:\`, or a one-letter scheme followed by `://` (round 3,
/// finding 1); `C:\` and `C:/x` are drive letters, not schemes.
static SCHEME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[A-Za-z][A-Za-z0-9+.-]+:[/\\]|[A-Za-z]://").expect("static pattern"));
/// `user:pass@host` at the start of a text (review round 2, 5(a)): no `/` or blank before the first `:`, then an `@`,
/// with no blank between the `:` and that `@`.
static START_USER_PASS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[^/\s:]*:[^\s@]*@").expect("static pattern"));
/// A drive letter: one letter, `:`, then `\`, or `/` that is not followed by a second `/` (round 3, finding 1: `x://` is a
/// scheme, not a drive).
static DRIVE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z]:(\\|/([^/]|$))").expect("static pattern"));

/// Review round 2, 5(a): the text starts with `user:pass@host` and is not a drive-letter path.
fn starts_with_user_pass(text: &str) -> bool {
    START_USER_PASS.is_match(text) && !DRIVE.is_match(text)
}

/// The text of a loadout field other than `args`, as `loadout show`, `config show` and the `config check` `compact` line
/// print it: the rule of one plain argument (review round 3, finding 5).
pub(crate) fn shown_field(text: &str) -> String {
    shown_text(text, true)
}

/// `lower` holds a [`SECRET_NAMES`] entry.
fn has_secret_word(lower: &str) -> bool {
    SECRET_NAMES.iter().any(|s| lower.contains(s))
}

/// A byte of a name: an ASCII letter or digit, `_`, `-` or `.`.
fn is_name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.')
}

/// A name: one or more name bytes.
fn is_name(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(is_name_byte)
}

/// Review round 3, finding 6 (it holds the round 2 `NAME=value` rule and the D3 `=` form): the first run of name bytes
/// that holds a secret word and is followed by `=` or `:` (blanks may come before and after it), or by one or more
/// blanks and more text, hides the rest of the text: `API_KEY = <value not shown>`, `X-Api-Key: <value not shown>`,
/// `GITHUB_TOKEN <value not shown>`, `--env=PATH=/usr/bin:AUTH_TOKEN=<value not shown>`.
fn hide_after_name(text: &str) -> Option<String> {
    let b = text.as_bytes();
    let blank = |i: usize| i < b.len() && matches!(b[i], b' ' | b'\t');
    let mut i = 0;
    while i < b.len() {
        if !is_name_byte(b[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < b.len() && is_name_byte(b[i]) {
            i += 1;
        }
        if !has_secret_word(&text[start..i].to_ascii_lowercase()) {
            continue;
        }
        let mut j = i;
        while blank(j) {
            j += 1;
        }
        let cut = if j < b.len() && matches!(b[j], b'=' | b':') {
            let mut k = j + 1;
            while blank(k) {
                k += 1;
            }
            k
        } else if j > i && j < b.len() {
            j
        } else {
            continue;
        };
        return Some(format!("{}{VALUE_HIDDEN}", &text[..cut]));
    }
    None
}

/// `%XX` sequences decoded (round 3, finding 4); other bytes as they are.
fn percent_decoded(text: &str) -> String {
    let b = text.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |k: usize| b.get(k).and_then(|c| (*c as char).to_digit(16));
        if b[i] == b'%'
            && let (Some(hi), Some(lo)) = (hex(i + 1), hex(i + 2))
        {
            out.push((hi * 16 + lo) as u8);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Review round 2, 5(c), round 3, findings 3 and 4: the text from the first `?` or `#` of a token is a list of
/// parameters split on `&`, `;`, `?` and `#`. A parameter `name=value` whose percent-decoded name holds a secret word
/// is `name=<value not shown>` (the name as typed); any other value goes through the whole argument rule again.
fn hide_parameters(tail: &str) -> String {
    tail.split_inclusive(['&', ';', '?', '#'])
        .map(|piece| {
            let (param, sep) = match piece.char_indices().last() {
                Some((at, '&' | ';' | '?' | '#')) => piece.split_at(at),
                _ => (piece, ""),
            };
            let param = match param.split_once('=') {
                Some((name, _)) if has_secret_word(&percent_decoded(name).to_lowercase()) => format!("{name}={VALUE_HIDDEN}"),
                Some((name, value)) => format!("{name}={}", shown_text(value, true)),
                None => param.to_string(),
            };
            format!("{param}{sep}")
        })
        .collect()
}

/// The one rule for a plain argument and for a field (round 3, findings 1 and 5): user info (D1) gives
/// `<user info removed>`; with `json`, inline JSON (D2) gives `<inline JSON not shown>`; then the text before the first
/// `?` or `#` hides what follows a secret name ([`hide_after_name`]) and the rest hides secret parameters
/// ([`hide_parameters`]).
fn shown_text(text: &str, json: bool) -> String {
    if user_info(text) {
        return USER_INFO_REMOVED.to_string();
    }
    if json {
        if json_like(text) {
            return INLINE_JSON_HIDDEN.to_string();
        }
        if let Some((name, value)) = text.split_once('=')
            && json_like(value)
        {
            return format!("{name}={INLINE_JSON_HIDDEN}");
        }
        if let Some(value) = attached_short(text).filter(|v| json_like(v)) {
            return format!("{}{INLINE_JSON_HIDDEN}", &text[..text.len() - value.len()]);
        }
    }
    let (head, tail) = text.find(['?', '#']).map_or((text, ""), |at| text.split_at(at));
    if let Some(cut) = hide_after_name(head) {
        return cut;
    }
    format!("{head}{}", hide_parameters(tail))
}

/// The `user:pass@host` form (D1 (b)): an `@`, and the text before the last `@` holds a `:` and no `/` and no `\`.
fn user_pass(text: &str) -> bool {
    text.rfind('@').is_some_and(|at| {
        let before = &text[..at];
        before.contains(':') && !before.contains(['/', '\\'])
    })
}

/// The text after the first 2 characters of a token that starts with exactly one `-` and has 3 or more characters.
fn attached_short(token: &str) -> Option<&str> {
    if !token.starts_with('-') || token.starts_with("--") {
        return None;
    }
    let (at, _) = token.char_indices().nth(2)?;
    Some(&token[at..])
}

/// D1: the token holds user info. (a) A scheme anywhere, with an `@` anywhere after its `:` (`?`, `#` and `/` do not
/// end the search); (b) the token, its `=` value or its attached short value has the `user:pass@host` form; (c, review
/// round 2, 5(a)) the token starts with `user:pass@host`, also with a `/` in the password.
fn user_info(token: &str) -> bool {
    if SCHEME.find(token).is_some_and(|m| token[m.end()..].contains('@')) {
        return true;
    }
    starts_with_user_pass(token)
        || user_pass(token)
        || token.split_once('=').is_some_and(|(_, value)| user_pass(value))
        || attached_short(token).is_some_and(user_pass)
}

/// The first non-blank character of `text` is `{` or `[` (D2).
fn json_like(text: &str) -> bool {
    text.trim_start().starts_with(['{', '['])
}

/// The text of `token` before its first `=` is a name (see [`is_name`]; `-d{"k":"secret"}` is not one) and, lower-cased,
/// holds a [`SECRET_NAMES`] entry (D3).
fn secret_named(token: &str) -> bool {
    let name = token.split('=').next().unwrap_or(token);
    is_name(name) && has_secret_word(&name.to_lowercase())
}

/// What a token is to the hiding rules (D5). Tokens after the `--` that ends the flags have the same rules (review round
/// 2, 5(e)); only a content value needs the AU30 walk, which ends at that `--`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    /// A value of a [`CONTENT_FLAGS`] flag: the attached form `--x=v`, or a value token after the flag token.
    Content { attached: bool, shown: bool },
    /// The value token after a separate secret-named flag (D3).
    SecretValue,
    /// Any other token.
    Plain,
}

/// One token as `loadout show` and `config show` print it (D1-D5): D1 for any role, then the rule of the role. A plain
/// token goes through [`shown_text`]; a content value that names a file goes through it without the JSON rule.
fn shown_token(token: &str, role: Role) -> String {
    if user_info(token) {
        return USER_INFO_REMOVED.to_string();
    }
    match role {
        Role::Content { attached: true, shown: false } => {
            format!("{}={INLINE_JSON_HIDDEN}", token.split('=').next().unwrap_or(token))
        }
        Role::Content { attached: false, shown: false } => INLINE_JSON_HIDDEN.to_string(),
        Role::SecretValue => VALUE_HIDDEN.to_string(),
        Role::Content { shown: true, .. } => shown_text(token, false),
        Role::Plain => shown_text(token, true),
    }
}

/// A content value is shown only when it names a regular file relative to `base` (the AU30 rule; an absolute value
/// replaces `base`; a link is followed).
fn names_a_file(base: &Path, value: &str) -> bool {
    std::fs::metadata(base.join(value)).is_ok_and(|m| m.is_file())
}

/// Each argument of a pane of `agent` whose folder is `base`, as `loadout show` and `config show` print it (D5). The
/// content values come from the AU30 walk (`occurrences`, which ends at the `--` that ends the flags); every other rule
/// applies to every token, also after that `--` (review round 2, 5(e)).
pub(crate) fn shown_args(agent: Agent, args: &[String], base: &Path) -> Vec<String> {
    let occs = occurrences(agent, args);
    let mut roles: Vec<Role> = vec![Role::Plain; args.len()];
    for o in occs.iter().filter(|o| CONTENT_FLAGS.contains(&o.name)) {
        if o.tokens.len() == 1 && o.values.len() == 1 {
            roles[o.tokens.start] = Role::Content { attached: true, shown: names_a_file(base, &o.values[0]) };
        } else {
            for i in o.tokens.start + 1..o.tokens.end {
                roles[i] = Role::Content { attached: false, shown: names_a_file(base, &args[i]) };
            }
        }
    }
    // D3, round 2 5(b), round 3 finding 2: the next token after a secret-named flag with no `=` value (no `=`, or an
    // empty value after it) is its value, also when it starts with `-`, except: `--`; (a) a secret-named flag, which is
    // shown and hides its own next token (a chain); (b) a `--` flag of the agent's table, which is shown and walked as
    // that flag
    let secret_flag = |t: &str| t.starts_with('-') && secret_named(t);
    for i in 0..args.len().saturating_sub(1) {
        let (token, next) = (&args[i], &args[i + 1]);
        if roles[i] == Role::Plain
            && secret_flag(token)
            && token.split_once('=').is_none_or(|(_, v)| v.is_empty())
            && roles[i + 1] == Role::Plain
            && next != "--"
            && !secret_flag(next)
            && !(next.starts_with("--") && is_flag(agent, next))
        {
            roles[i + 1] = Role::SecretValue;
        }
    }
    args.iter().zip(roles).map(|(t, r)| shown_token(t, r)).collect()
}

/// The folders that a pane's `cwd` is taken against (D6): the main worktree and the worktree list.
pub(crate) struct PaneBase {
    pub(crate) main_root: PathBuf,
    pub(crate) worktrees: Vec<Worktree>,
}

impl PaneBase {
    /// The base of the repository of `here`, by `zygo up`'s own lookup (`find_checkout`). When it fails: `here` with no
    /// worktree, and the reason.
    pub(crate) fn from_folder(here: &Path) -> (PaneBase, Option<String>) {
        match find_checkout(here, GIT_BOUND) {
            Ok((_, list, main)) => (PaneBase { main_root: main, worktrees: list }, None),
            Err(e) => (PaneBase { main_root: here.to_path_buf(), worktrees: vec![] }, Some(e.message)),
        }
    }

    /// The folder of `pane`, by the rule of `zygo up` (`pane_cwd`).
    pub(crate) fn of(&self, pane: &LoadoutPane) -> PathBuf {
        pane_cwd(pane.cwd.as_deref(), &self.main_root, &self.worktrees).0
    }
}

/// The one stderr line for a [`PaneBase::from_folder`] failure (D6).
pub(crate) fn print_folder_warning(reason: &str) {
    print_err(&format!(
        "zygo: the pane folder could not be found ({}); paths are checked against the current folder\n",
        esc(reason)
    ));
}

/// D7: each pane's `args` through [`shown_args`] (with the pane's own folder, found before any change), a `server` with
/// user info (AU98) replaced, and every other text field through [`shown_field`] (review round 2, 5(f); round 3,
/// finding 5). A `cwd` of the form `branch:<name>` is `branch:` and `<name>` through [`shown_field`], so the `branch:`
/// prefix is never read as user info (round 3, finding 7).
pub(crate) fn hide_secrets(l: &mut Loadout, base: &PaneBase) {
    let scrub = |f: &mut Option<String>| {
        if let Some(t) = f.as_mut() {
            *t = shown_field(t);
        }
    };
    for w in &mut l.window {
        for p in &mut w.pane {
            let dir = base.of(p);
            let agent = p.kind.parse::<Agent>().unwrap_or(Agent::ClaudeCode);
            p.args = shown_args(agent, &p.args, &dir);
            if p.server.as_deref().is_some_and(has_user_info) {
                p.server = Some(USER_INFO_REMOVED.to_string());
            } else {
                scrub(&mut p.server);
            }
            p.kind = shown_field(&p.kind);
            scrub(&mut p.profile);
            scrub(&mut p.compact);
            scrub(&mut p.config_dir);
            scrub(&mut p.name);
            scrub(&mut p.job);
            if let Some(t) = p.cwd.as_mut() {
                *t = match t.strip_prefix("branch:") {
                    Some(branch) => format!("branch:{}", shown_field(branch)),
                    None => shown_field(t),
                };
            }
        }
        scrub(&mut w.name);
        scrub(&mut w.layout);
    }
}

/// The text of `zygo loadout show <name>`: `[loadout.<name>]` as TOML after [`hide_secrets`] (D7), each line through
/// `zygo_core::esc` as `zygo config show` prints each line (review round 2, finding 2). No other step: `config show`
/// runs the same [`hide_secrets`], so both print the same text for each argument and field (round 3, finding 1).
fn show(name: &str, mut l: Loadout, base: &PaneBase) -> Result<String, toml::ser::Error> {
    hide_secrets(&mut l, base);
    let mut named = toml::Table::new();
    named.insert(name.to_string(), toml::Value::Table(toml::Table::try_from(&l)?));
    let mut top = toml::Table::new();
    top.insert("loadout".to_string(), toml::Value::Table(named));
    Ok(toml::to_string(&top)?.lines().map(|line| format!("{}\n", esc(line))).collect())
}

#[derive(clap::Args)]
pub struct LoadoutArgs {
    #[command(subcommand)]
    pub sub: LoadoutCmd,
}

#[derive(clap::Subcommand)]
pub enum LoadoutCmd {
    /// Every loadout and its source
    List,
    /// One loadout as TOML, with no secret
    Show { name: String },
}

/// The OP5 check, the user config (D3), then D7: `<name escaped>  built-in`, `user` or `workspace-files` per line, in
/// the order of `loadout::list`, with the R§10 row of the workspace files of the current folder's repository.
pub fn run(a: &LoadoutArgs) -> i32 {
    if let Some(code) = op5_exit() {
        return code;
    }
    let paths = Paths::from_env();
    let m = match user_merged_or_exit(&paths, "loadout") {
        Ok(m) => m,
        Err(code) => return code,
    };
    match &a.sub {
        LoadoutCmd::List => {
            // R§10 `zygo loadout list`: the approved files add `repository` (source `workspace-files`); a not-approved,
            // invalid or shadowed repository loadout is one more `repository` row after the others of that name. Only
            // an unread read prints its line; the other states are the text of that row.
            let v = here(&m, &paths.approvals_file);
            if let (State::Unread, Some(line)) = (&v.state, &v.line) {
                print_err(&format!("{line}\n"));
            }
            let from_files = v.merged.definition_file_of(&format!("loadout.{REPOSITORY_LOADOUT}")).is_some();
            let mut rows: Vec<(String, &str)> = loadout::list(&v.merged.config, from_files)
                .into_iter()
                .map(|(name, source)| {
                    let source = match source {
                        Source::Builtin => "built-in",
                        Source::User => "user",
                        Source::Repository => "workspace-files",
                    };
                    (name, source)
                })
                .collect();
            let extra = match &v.state {
                State::NotApproved { workspace: true } => Some("workspace-files, not approved"),
                State::Invalid { .. } => Some("workspace-files, invalid"),
                State::Approved { workspace: true } if !from_files => Some("workspace-files, shadowed"),
                _ => None,
            };
            if let Some(text) = extra {
                rows.push((REPOSITORY_LOADOUT.to_string(), text));
                // Stable: the extra row stays after the rows of its name.
                rows.sort_by(|a, b| a.0.cmp(&b.0));
            }
            let text: String = rows.iter().map(|(name, source)| format!("{}  {source}\n", esc(name))).collect();
            print_out(&text);
            0
        }
        LoadoutCmd::Show { name } => {
            // D7: the user's `[loadout.<name>]` first, then the built-in `solo`; R§10: `repository` with no user
            // `[loadout.repository]` is the loadout of the approved workspace files, else their line.
            let l = match m.config.loadout.get(name) {
                Some(l) => l.clone(),
                None if name == BUILTIN_LOADOUT => loadout::builtin_solo(),
                None if name == REPOSITORY_LOADOUT => {
                    let v = here(&m, &paths.approvals_file);
                    match (v.merged.config.loadout.get(name), &v.line) {
                        (Some(l), _) => l.clone(),
                        (None, Some(line)) => {
                            print_err(&format!("{line}\n"));
                            return 1;
                        }
                        (None, None) => {
                            print_err(&format!("zygo: {}\n", LoadoutError::Unknown { name: name.clone() }));
                            return 1;
                        }
                    }
                }
                None => {
                    print_err(&format!("zygo: {}\n", LoadoutError::Unknown { name: name.clone() }));
                    return 1;
                }
            };
            let here = match std::env::current_dir() {
                Ok(d) => d,
                Err(e) => {
                    print_err(&format!("zygo: cannot read the current folder: {}\n", esc(&e.to_string())));
                    return 1;
                }
            };
            // D6: the same lookup as `config show` and `config check`
            let (base, warning) = PaneBase::from_folder(&here);
            if let Some(m) = warning {
                print_folder_warning(&m);
            }
            match show(name, l, &base) {
                Ok(text) => {
                    print_out(&text);
                    0
                }
                Err(e) => {
                    print_err(&format!("zygo: {}\n", esc(&e.to_string())));
                    1
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use zygo_config::{Loadout, LoadoutPane, LoadoutWindow};
    use zygo_core::Agent;

    use super::*;

    const G: &str = "<user info removed>";
    const H: &str = "<inline JSON not shown>";
    const V: &str = "<value not shown>";

    /// (argument, shown) for one claude-code pane, in this order. Each hidden argument holds `SECRET`; no shown text does.
    const FORMS: &[(&str, &str)] = &[
        ("https://u:SECRET1@h.example.invalid", G),
        ("DATABASE_URL=postgres://u:SECRET2@h.example.invalid/db", G),
        ("--env=URL=https://u:SECRET3@h.example.invalid", G),
        ("-d=https://u:SECRET4@h.example.invalid", G),
        ("-Xhttps://u:SECRET5@h.example.invalid", G),
        ("https://u:SECRET6#x@h.example.invalid", G),
        ("https://h.example.invalid/p?u:SECRET7@x", G),
        ("https:/u:SECRET8@h.example.invalid", G),
        (r"https:\\u:SECRET9@h.example.invalid", G),
        ("u:SECRET10@h.example.invalid", G),
        ("a@b:SECRET11@h.example.invalid", G),
        ("u:SECRET12?x@h.example.invalid", G),
        ("u:SECRET13#x@h.example.invalid", G),
        ("u:pa SECRET14@h.example.invalid", G),
        ("https://u:pa'SECRET15@h.example.invalid", G),
        ("--x=u:SECRET16@h.example.invalid", G),
        ("-du:SECRET17@h.example.invalid", G),
        ("node:20@sha256:0123abcd", G),
        ("{\"k\":\"SECRET19\"", H),
        (" [ \"SECRET20\" ]", H),
        ("--model={\"k\":\"SECRET21\"}", "--model=<inline JSON not shown>"),
        ("-d{\"k\":\"SECRET22\"}", "-d<inline JSON not shown>"),
        ("--api-key=SECRET23", "--api-key=<value not shown>"),
        ("--auth-token", "--auth-token"),
        ("SECRET24", V),
        ("--Password=SECRET25", "--Password=<value not shown>"),
        ("--model=x", "--model=x"),
        ("me@example.com", "me@example.com"),
        ("claude-sonnet-4@20250514", "claude-sonnet-4@20250514"),
        ("--model=claude-sonnet-4@20250514", "--model=claude-sonnet-4@20250514"),
        (r"C:\Users\first.last@corp\x.json", r"C:\Users\first.last@corp\x.json"),
        ("me@h.example.invalid:8080", "me@h.example.invalid:8080"),
        ("Authorization: Bearer tok-visible", "Authorization: <value not shown>"),
    ];

    /// Attempt 1 round 3 F2: AU30 content flags and `--settings` with no file behind them, and JSON after any `=`.
    const CONTENT_16: [&str; 16] = [
        r#"--agents={"r":{"prompt":"SECRET1"}}"#,
        r#"--json-schema={"type":"object","description":"SECRET2"}"#,
        "--agents", "agents-SECRET3.json",
        "--json-schema", "schema-SECRET4.json",
        "--plugin-url", "plugin-SECRET5",
        "--plugin-url=plugin-SECRET6",
        "--mcp-config", "mcp-SECRET7.json",
        "--settings=settings-SECRET8.json",
        r#"--model={"k":"SECRET9"}"#,
        r#"--x=["SECRET10"]"#,
        "--model=x",
        "--name=a=b",
    ];
    const CONTENT_16_SHOWN: [&str; 16] = [
        "--agents=<inline JSON not shown>", "--json-schema=<inline JSON not shown>",
        "--agents", H, "--json-schema", H, "--plugin-url", H,
        "--plugin-url=<inline JSON not shown>", "--mcp-config", H,
        "--settings=<inline JSON not shown>", "--model=<inline JSON not shown>", "--x=<inline JSON not shown>",
        "--model=x", "--name=a=b",
    ];

    /// Attempt 1 round 3 F1, corrected for D1 (a whole token is replaced).
    const USER_INFO_12: [&str; 12] = [
        "--x=https:/u:SECRET1@h.example.invalid",
        r"https:\\u:SECRET2@h.example.invalid",
        r"https:\u:SECRET3@h.example.invalid",
        "u:SECRET4@h.example.invalid",
        "https://u:pa'SECRET5@h.example.invalid",
        "https://u:pa\"SECRET6@h.example.invalid",
        "https://u:pa`SECRET7@h.example.invalid",
        "https://u:pa SECRET8@h.example.invalid",
        "https://u:SECRET9@h.example.invalid/x",
        "--model=x",
        "https://h.example.invalid/x",
        "plain",
    ];
    const USER_INFO_12_SHOWN: [&str; 12] = [G, G, G, G, G, G, G, G, G, "--model=x", "https://h.example.invalid/x", "plain"];

    fn strings(a: &[&str]) -> Vec<String> {
        a.iter().map(|x| x.to_string()).collect()
    }

    /// The `args` of pane `p` of window 0 of the loadout `pair` in a `show` text.
    fn args_in_text(text: &str, p: usize) -> Vec<String> {
        let t: toml::Table = text.parse().unwrap();
        t["loadout"]["pair"]["window"][0]["pane"][p]["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect()
    }

    fn base(dir: &Path) -> PaneBase {
        PaneBase { main_root: dir.to_path_buf(), worktrees: vec![] }
    }

    fn pane(args: &[&str]) -> LoadoutPane {
        LoadoutPane { kind: "claude-code".into(), args: strings(args), ..LoadoutPane::default() }
    }

    /// One window, one `claude-code` pane.
    fn one(args: &[&str]) -> Loadout {
        Loadout { window: vec![LoadoutWindow { name: None, layout: None, pane: vec![pane(args)] }] }
    }

    #[test] // D1; kills: `has_user_info` as the rule; a search that stops at `?`, `#` or `/`; a 1-character scheme; the
    // first `@` in place of the last; the literal "before that `:`" reading
    fn user_info_reads_a_scheme_anywhere_and_the_user_pass_form() {
        for (input, shown) in FORMS {
            if *shown == G {
                assert!(user_info(input), "{input}");
            }
        }
        for t in ["https://u:p@h", "ab://u@h", "-du:p@h"] {
            assert!(user_info(t), "{t}");
        }
        for t in [
            "me@example.com", "claude-sonnet-4@20250514", "--model=claude-sonnet-4@20250514", r"C:\Users\first.last@corp\x.json",
            "me@h.example.invalid:8080", "host:8080", "https://h.example.invalid/x", "Authorization: Bearer tok", "--model=x",
            "plain", "x:/u:p@h",
        ] {
            assert!(!user_info(t), "{t}");
        }
    }

    #[test] // D1-D3, D5; kills: a flag name dropped from a `=` JSON or secret value; JSON that must parse; a secret value
    // in the separate form shown
    fn shown_args_hide_every_form() {
        let t = tempfile::tempdir().unwrap();
        let input: Vec<&str> = FORMS.iter().map(|f| f.0).collect();
        let shown: Vec<&str> = FORMS.iter().map(|f| f.1).collect();
        assert_eq!(shown_args(Agent::ClaudeCode, &strings(&input), t.path()), strings(&shown));
    }

    #[cfg(unix)]
    #[test] // D4; kills: only the first `--mcp-config` value hidden; the process folder in place of `base`;
    // `symlink_metadata` or `exists` in place of `is_file`; the next token after a content flag with no value hidden;
    // a token after `--` read as a content value (review round 2, 5(e): JSON after `--` is hidden too)
    fn shown_args_show_a_content_value_only_when_it_names_a_file() {
        let t = tempfile::tempdir().unwrap();
        let b = t.path();
        std::fs::write(b.join("a.json"), "{}").unwrap();
        std::fs::create_dir(b.join("dir")).unwrap();
        std::os::unix::fs::symlink(b.join("missing"), b.join("dangling")).unwrap();
        let abs = b.join("a.json").to_str().unwrap().to_string();
        let args = [
            "--mcp-config", "a.json", "{\"k\":\"SECRET1\"}", "--agents", "dir", "--json-schema", "dangling", "--agents", &abs,
            "--settings=a.json", "--settings=SECRET3.json", "--plugin-url=SECRET5", "--mcp-config", "--model", "m", "--name=a=b",
            "--", "--settings", "after", "{\"k\":\"after-json\"}",
        ];
        let shown = [
            "--mcp-config", "a.json", H, "--agents", H, "--json-schema", H, "--agents", &abs, "--settings=a.json",
            "--settings=<inline JSON not shown>", "--plugin-url=<inline JSON not shown>", "--mcp-config", "--model", "m",
            "--name=a=b", "--", "--settings", "after", H,
        ];
        assert_eq!(shown_args(Agent::ClaudeCode, &strings(&args), b), strings(&shown));
    }

    #[test] // attempt 1 round 1 and round 3 F1
    fn show_replaces_a_server_with_user_info_and_hides_args() {
        let t = tempfile::tempdir().unwrap();
        let p0 = LoadoutPane { server: Some("https://u:k1@h.example.invalid".into()), ..pane(&["https://u:k2@h.example.invalid/x"]) };
        let p1 = LoadoutPane { server: Some("https://h.example.invalid".into()), ..pane(&[]) };
        let l = Loadout { window: vec![LoadoutWindow { name: None, layout: None, pane: vec![p0, p1] }] };
        let text = show("pair", l, &base(t.path())).unwrap();
        assert!(!text.contains("k1") && !text.contains("k2"), "{text}");
        assert!(text.contains("server = \"<user info removed>\""), "{text}");
        assert!(text.contains("server = \"https://h.example.invalid\""), "{text}");
        assert_eq!(args_in_text(&text, 0), [G]);
    }

    #[test] // round 3 F1, corrected for D1
    fn show_replaces_an_argument_with_user_info() {
        let t = tempfile::tempdir().unwrap();
        let text = show("pair", one(&USER_INFO_12), &base(t.path())).unwrap();
        assert_eq!(args_in_text(&text, 0), strings(&USER_INFO_12_SHOWN));
        assert!(!text.contains("SECRET"), "{text}");
    }

    #[test] // round 3 F2
    fn show_hides_every_content_flag_value_and_inline_json_after_any_flag() {
        let t = tempfile::tempdir().unwrap();
        let text = show("pair", one(&CONTENT_16), &base(t.path())).unwrap();
        assert_eq!(args_in_text(&text, 0), strings(&CONTENT_16_SHOWN));
        assert!(!text.contains("SECRET"), "{text}");
    }

    #[test] // round 1 B2
    fn show_hides_inline_json() {
        let t = tempfile::tempdir().unwrap();
        let p0 = pane(&["--settings", r#"{"env":{"ANTHROPIC_API_KEY":"sk-SECRET1"}}"#, "--mcp-config", r#"{"headers":{"Authorization":"Bearer SECRET2"}}"#, "--model=x"]);
        let p1 = pane(&[r#"--settings={"k":"SECRET3"}"#, r#"--mcp-config={"k":"SECRET4"}"#, r#"["SECRET5"]"#, "--settings", "settings.json", "plain"]);
        let l = Loadout { window: vec![LoadoutWindow { name: None, layout: None, pane: vec![p0, p1] }] };
        let text = show("pair", l, &base(t.path())).unwrap();
        assert_eq!(args_in_text(&text, 0), strings(&["--settings", H, "--mcp-config", H, "--model=x"]));
        assert_eq!(
            args_in_text(&text, 1),
            strings(&["--settings=<inline JSON not shown>", "--mcp-config=<inline JSON not shown>", H, "--settings", H, "plain"])
        );
        assert!(!text.contains("SECRET"), "{text}");
    }

    #[test] // F15; kills: a field left out of the scrub, `job` included (WD-24) (round 3, finding 5: a field is one token
    // of the argument rule, so a URL with user info is `<user info removed>` as an argument is)
    fn hide_secrets_scrubs_every_other_text_field() {
        let t = tempfile::tempdir().unwrap();
        let u = |n: u32| Some(format!("https://u:SECRET{n}@h.example.invalid/x"));
        let p = LoadoutPane {
            profile: u(3),
            compact: u(4),
            config_dir: u(5),
            cwd: u(6),
            name: u(7),
            job: u(8),
            ..pane(&[])
        };
        let mut l = Loadout { window: vec![LoadoutWindow { name: u(1), layout: u(2), pane: vec![p] }] };
        hide_secrets(&mut l, &base(t.path()));
        let clean = Some(G.to_string());
        let w = &l.window[0];
        let p = &w.pane[0];
        assert_eq!(
            [
                &w.name,
                &w.layout,
                &p.profile,
                &p.compact,
                &p.config_dir,
                &p.cwd,
                &p.name,
                &p.job
            ],
            [&clean; 8]
        );
    }

    // ---- diff review round 2 ----

    fn shown(args: &[&str]) -> Vec<String> {
        let t = tempfile::tempdir().unwrap();
        shown_args(Agent::ClaudeCode, &strings(args), t.path())
    }

    #[test] // round 2 finding 1; kills: `starts_with` in place of the `json_like` blank rule for an attached short value
    fn shown_args_hide_attached_json_after_blanks() {
        assert_eq!(shown(&["-d [ \"SECRET1\" ]", "-d\t{\"k\":\"SECRET2\"}", "-d  {}"]), strings(&["-d<inline JSON not shown>"; 3]));
    }

    #[test] // round 2 finding 4; kills: the NAME=value rule only on flags, or only on the part before the first `=`
    fn shown_args_hide_env_style_secrets() {
        assert_eq!(
            shown(&[
                "--env=ANTHROPIC_API_KEY=sk-SECRET1", "ANTHROPIC_API_KEY=sk-SECRET2", "--env=X=1=MY_PASSWORD=SECRET3",
                "Github_Token=SECRET4", "--env=URL=plain", "--name=a=b", "PATH=/usr/bin",
            ]),
            strings(&[
                "--env=ANTHROPIC_API_KEY=<value not shown>", "ANTHROPIC_API_KEY=<value not shown>",
                "--env=X=1=MY_PASSWORD=<value not shown>", "Github_Token=<value not shown>", "--env=URL=plain", "--name=a=b",
                "PATH=/usr/bin",
            ])
        );
    }

    #[test] // round 2 finding 5(a); kills: no `user:pass@host` rule at the start of a token (a `/` in the password)
    fn user_info_reads_user_pass_at_the_start_of_a_token() {
        for t in ["u:pa/SECRET1@h.example.invalid", "u:SECRET2/x@h.example.invalid/y", "a.b-c:SECRET3/@h"] {
            assert!(user_info(t), "{t}");
        }
        for t in ["./a@b", "/x/y@z", "git@github.com:org/repo", r"C:\Users\first.last@corp\x.json", "C:/Users/a@b", "u:a b/c@h"] {
            assert!(!user_info(t), "{t}");
        }
    }

    #[test] // round 2 finding 5(b); kills: `!starts_with('-')` kept on the next token; `--` hidden as a value (round 3:
    // a secret-named next token is a flag, so the value is `-hunter6`, which is no secret-named flag)
    fn shown_args_hide_the_value_after_a_secret_flag_even_with_a_dash() {
        assert_eq!(
            shown(&["--api-token", "-hunter6", "--auth", "--", "plain"]),
            strings(&["--api-token", "<value not shown>", "--auth", "--", "plain"])
        );
    }

    #[test] // round 2 finding 5(c); kills: no query rule; a query rule only on whole tokens, not on `=` values
    fn shown_args_hide_secret_query_parameters() {
        assert_eq!(
            shown(&[
                "https://h.example.invalid/x?a=1&access_token=SECRET1&b=2#f", "--url=https://h.example.invalid/?Api_Key=SECRET2",
                "https://h.example.invalid/?q=1&page=2",
            ]),
            strings(&[
                "https://h.example.invalid/x?a=1&access_token=<value not shown>&b=2#f",
                "--url=https://h.example.invalid/?Api_Key=<value not shown>", "https://h.example.invalid/?q=1&page=2",
            ])
        );
    }

    #[test] // round 2 finding 5(d); kills: `bearer` missing from the secret words
    fn shown_args_hide_bearer_named_flags() {
        assert_eq!(
            shown(&["--bearer=SECRET1", "--x-bearer", "SECRET2", "--Auth", "SECRET3"]),
            strings(&["--bearer=<value not shown>", "--x-bearer", "<value not shown>", "--Auth", "<value not shown>"])
        );
    }

    #[test] // round 2 finding 5(e); kills: tokens after `--` shown as typed
    fn shown_args_apply_every_rule_after_double_dash() {
        assert_eq!(
            shown(&["--", "{\"k\":\"SECRET1\"}", "TOKEN=SECRET2", "--api-key", "SECRET3", "https://u:SECRET4@h.example.invalid", "-d[1]"]),
            strings(&["--", H, "TOKEN=<value not shown>", "--api-key", V, G, "-d<inline JSON not shown>"])
        );
    }

    #[test] // round 2 finding 5(f); kills: a field without the scheme-less user-info rule
    fn hide_secrets_hides_user_info_without_a_scheme_in_every_field() {
        let t = tempfile::tempdir().unwrap();
        let u = |n: u32| Some(format!("u:SECRET{n}@h.example.invalid"));
        let p = LoadoutPane {
            kind: "u:SECRET0@h.example.invalid".into(), profile: u(3), server: u(7), compact: u(4), config_dir: u(5), cwd: u(6),
            name: u(8), job: u(9),
            ..pane(&[])
        };
        let mut l = Loadout { window: vec![LoadoutWindow { name: u(1), layout: u(2), pane: vec![p] }] };
        hide_secrets(&mut l, &base(t.path()));
        let g = Some(G.to_string());
        let w = &l.window[0];
        let p = &w.pane[0];
        assert_eq!(p.kind, G);
        assert_eq!(
            [&w.name, &w.layout, &p.profile, &p.server, &p.compact, &p.config_dir, &p.cwd, &p.name, &p.job],
            [&g; 9]
        );
        // a profile name with an `@` and no `:` before it stays
        assert_eq!(shown_field("me@work"), "me@work");
    }

    #[test] // round 2 finding 8; kills: a removed `!token.contains('=')` guard (the `=` form hides its own value only)
    fn shown_args_keep_the_token_after_an_equals_secret_flag() {
        assert_eq!(shown(&["--x-key=SECRET1", "shown"]), strings(&["--x-key=<value not shown>", "shown"]));
    }

    // ---- diff review round 3 ----

    #[test] // round 3 finding 1; kills: a drive letter that matches `x://`; no one-letter scheme with `://` in the middle
    fn user_info_reads_a_one_letter_scheme_with_two_slashes() {
        assert!(starts_with_user_pass("x://u:p@h"));
        for t in ["x://u:SECRET1@h", "s://u:SECRET2@h/p", "--x=s://u:SECRET3@h", "see x://u:SECRET4@h"] {
            assert!(user_info(t), "{t}");
        }
        for t in ["c:/x@y", r"C:\Users\a:b@c", "x:/u:p@h", "C:/Users/a@b"] {
            assert!(!user_info(t), "{t}");
        }
    }

    #[test] // round 3 finding 2; kills: a secret-named next token hidden (no chain); a known flag after a secret flag
    // hidden; a flag value `-hunter6` shown; a secret flag with an empty `=` value whose next token is shown
    fn shown_args_chain_secret_flags_and_keep_known_flags() {
        let t = tempfile::tempdir().unwrap();
        std::fs::write(t.path().join("m.json"), "{}").unwrap();
        let s = |a: &[&str]| shown_args(Agent::ClaudeCode, &strings(a), t.path());
        assert_eq!(s(&["--auth", "--api-key", "SECRET1"]), strings(&["--auth", "--api-key", V]));
        assert_eq!(s(&["--api-key", "--mcp-config", "m.json"]), strings(&["--api-key", "--mcp-config", "m.json"]));
        assert_eq!(s(&["--api-key", "--mcp-config", "SECRET2.json"]), strings(&["--api-key", "--mcp-config", H]));
        assert_eq!(s(&["--api-key", "-hunter6", "--model", "m"]), strings(&["--api-key", V, "--model", "m"]));
        assert_eq!(s(&["--token=", "SECRET3"]), strings(&["--token=<value not shown>", V]));
        assert_eq!(s(&["--auth", "--token", "--secret", "SECRET4"]), strings(&["--auth", "--token", "--secret", V]));
    }

    #[test] // round 3 finding 3; kills: a query rule that stops at `#`
    fn shown_args_hide_secret_fragment_parameters() {
        assert_eq!(
            shown(&["https://h.example.invalid/cb#access_token=SECRET1&x=1", "https://h/x?a=1#y=2;Token=SECRET2"]),
            strings(&["https://h.example.invalid/cb#access_token=<value not shown>&x=1", "https://h/x?a=1#y=2;Token=<value not shown>"])
        );
    }

    #[test] // round 3 finding 4; kills: no `;` separator; no later `?` separator; no percent-decoding of names; no rule on a
    // parameter value; a query rule only with a scheme
    fn shown_args_read_query_parameters_precisely() {
        assert_eq!(
            shown(&[
                "https://h.example.invalid/x?a=1;token=SECRET1;b=2", "https://a.example/p?a=1?token=SECRET2",
                "https://h.example.invalid/x?to%6Ben=SECRET3", "https://a.example/?r=s://h/API_KEY:SECRET4",
                "h.example.invalid/x?token=SECRET5&b=2", "https://h/x?&&token", "https://h/x?token?x=1", "https://é.example/?tökén=1&token=SECRET6",
            ]),
            strings(&[
                "https://h.example.invalid/x?a=1;token=<value not shown>;b=2", "https://a.example/p?a=1?token=<value not shown>",
                "https://h.example.invalid/x?to%6Ben=<value not shown>", "https://a.example/?r=s://h/API_KEY:<value not shown>",
                "h.example.invalid/x?token=<value not shown>&b=2", "https://h/x?&&token", "https://h/x?token?x=1", "https://é.example/?tökén=1&token=<value not shown>",
            ])
        );
    }

    #[test] // round 3 finding 5; kills: a field through the old URL scrub (which stops at a blank), not the argument rule
    fn hide_secrets_reads_each_field_as_one_argument() {
        let t = tempfile::tempdir().unwrap();
        let p = LoadoutPane {
            cwd: Some("https://u:pa SECRET1@h.example.invalid".into()), profile: Some("x https://u:SECRET2@h".into()),
            compact: Some("https://u:pa SECRET3@h".into()), config_dir: Some("API_KEY=SECRET4".into()),
            name: Some("u:pa SECRET10@h".into()), job: Some("x https://u:SECRET11@h".into()), ..pane(&[])
        };
        let mut l = Loadout { window: vec![LoadoutWindow { name: Some("u:pa SECRET5@h".into()), layout: Some("{\"k\":\"SECRET6\"}".into()), pane: vec![p] }] };
        hide_secrets(&mut l, &base(t.path()));
        let w = &l.window[0];
        let p = &w.pane[0];
        let g = Some(G.to_string());
        assert_eq!([&w.name, &p.cwd, &p.profile, &p.compact, &p.name, &p.job], [&g; 6]);
        assert_eq!(p.config_dir.as_deref(), Some("API_KEY=<value not shown>"));
        assert_eq!(w.layout.as_deref(), Some(H));
        // the same function for an argument and a field
        for a in ["https://u:pa SECRET7@h", "API_KEY = SECRET8", "x?token=SECRET9", "plain"] {
            assert_eq!(shown_field(a), shown(&[a])[0], "{a}");
        }
    }

    #[test] // round 3 finding 6; kills: `:` not a separator; blanks not a separator; no blanks around `=`; no `cookie`
    fn shown_args_hide_secret_names_inside_a_token() {
        assert_eq!(
            shown(&[
                "--env=PATH=/usr/bin:AUTH_TOKEN=SECRET1", "export GITHUB_TOKEN=SECRET2", "API_KEY = SECRET3", "X-Api-Key: SECRET4",
                "--password:SECRET5", "GITHUB_TOKEN SECRET6", "Cookie: session=SECRET7", "x-api-key:SECRET8",
                "--header=X-Api-Key: SECRET9", "PATH=/usr/bin:/bin", "a: b", "--name=a=b", "keyboard.json", "--session-id",
            ]),
            strings(&[
                "--env=PATH=/usr/bin:AUTH_TOKEN=<value not shown>", "export GITHUB_TOKEN=<value not shown>", "API_KEY = <value not shown>",
                "X-Api-Key: <value not shown>", "--password:<value not shown>", "GITHUB_TOKEN <value not shown>",
                "Cookie: <value not shown>", "x-api-key:<value not shown>", "--header=X-Api-Key: <value not shown>",
                "PATH=/usr/bin:/bin", "a: b", "--name=a=b", "keyboard.json", "--session-id",
            ])
        );
        assert_eq!(shown(&["--session-id", "abc", "--resume", "abc"]), strings(&["--session-id", "abc", "--resume", "abc"]));
    }

    #[test] // round 3 finding 7; kills: a `branch:` cwd read by the user-info rule
    fn hide_secrets_keeps_a_branch_cwd() {
        let t = tempfile::tempdir().unwrap();
        let mut l = Loadout {
            window: vec![LoadoutWindow {
                name: None,
                layout: None,
                pane: vec![
                    LoadoutPane { cwd: Some("branch:alice@fix-1".into()), ..pane(&[]) },
                    LoadoutPane { cwd: Some("u:SECRET1@h".into()), ..pane(&[]) },
                    LoadoutPane { cwd: Some("branch:x?token=SECRET2".into()), ..pane(&[]) },
                ],
            }],
        };
        hide_secrets(&mut l, &base(t.path()));
        let cwd: Vec<Option<&str>> = l.window[0].pane.iter().map(|p| p.cwd.as_deref()).collect();
        assert_eq!(cwd, [Some("branch:alice@fix-1"), Some(G), Some("branch:x?token=<value not shown>")]);
    }

    #[test] // round 3: the known limits of the W§12 row, as stated there
    fn known_limits_are_as_the_spec_states() {
        assert_eq!(shown(&["-t", "x"]), strings(&["-t", "x"]));
        assert_eq!(shown(&["--secret", "--x", "S"]), strings(&["--secret", V, "S"]));
        assert_eq!(
            shown(&["node:20@sha256:0", "openai:gpt@v1", "ssh://git@host/x", "monkey=x", "https://keyvault.example:443"]),
            strings(&[G, G, G, "monkey=<value not shown>", "https://keyvault.example:<value not shown>"])
        );
        assert_eq!(
            shown(&["--author", "me", "--max-tokens", "9", "--keybindings", "vim", "--session-id", "abc"]),
            strings(&["--author", V, "--max-tokens", V, "--keybindings", V, "--session-id", "abc"])
        );
    }

    /// Each whitespace word of a pinned `--help` fixture that the argument rule changes, as `(word, shown)`; and each
    /// `--flag` of it whose next value the rule hides, as `(flag, "<next value hidden>")`.
    fn help_words_changed(rel: &str) -> Vec<(String, String)> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures").join(rel);
        let text = std::fs::read_to_string(&path).unwrap();
        let mut out = vec![];
        for word in text.split_whitespace() {
            let got = shown(&[word]).remove(0);
            if got != word {
                out.push((word.to_string(), got));
            }
            let flag = word.trim_end_matches([',', ']', ')']);
            if flag.starts_with("--") && shown(&[flag, "value"])[1] != "value" {
                out.push((flag.to_string(), "<next value hidden>".to_string()));
            }
        }
        out.sort();
        out.dedup();
        out
    }

    #[test] // round 3 finding 6: the rules against every word and flag of the pinned help (the list is the report)
    fn help_fixture_words_that_the_rules_change() {
        let pairs = |a: &[(&str, &str)]| a.iter().map(|(w, s)| (w.to_string(), s.to_string())).collect::<Vec<_>>();
        // D4: a content flag's value that names no file; D2: usage words that start with `[` or `{`. No word is changed by a
        // rule of round 3 (names inside a token, parameters, `cookie`, the flag chain).
        let next = "<next value hidden>";
        assert_eq!(
            help_words_changed("claude-code/help.txt"),
            pairs(&[
                ("--agents", next), ("--json-schema", next), ("--mcp-config", next), ("--plugin-url", next), ("--settings", next),
                ("[])", H), ("[command]", H), ("[description|session_id|url]", H), ("[filter]", H), ("[id]", H), ("[name]", H),
                ("[options]", H), ("[path]", H), ("[prompt]", H), ("[session]", H), ("[source]", H), ("[target]", H), ("[value]", H),
                ("{\"description\":", H), ("{\"type\":\"object\",\"properties\":{\"name\":{\"type\":\"string\"}},\"required\":[\"name\"]}", H),
            ])
        );
        assert_eq!(
            help_words_changed("opencode/help.txt"),
            pairs(&[
                ("[]]", H), ("[aliases:", H), ("[array]", H), ("[boolean]", H), ("[choices:", H), ("[default:", H), ("[default]", H),
                ("[message..]", H), ("[number]", H), ("[project]", H), ("[provider]", H), ("[sessionID]", H), ("[string]", H),
                ("[target]", H),
            ])
        );
    }
}
