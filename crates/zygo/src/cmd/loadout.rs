//! `zygo loadout list` (W§12; D7) and the secret hiding of `zygo loadout show` and `zygo config show` (W§12; FIX-27).

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use zygo_config::display::scrub_userinfo;
use zygo_config::{Loadout, LoadoutPane, BUILTIN_LOADOUT};
use zygo_core::paths::Paths;
use zygo_core::{esc, Agent};
use zygo_workspace::args::{has_user_info, occurrences};
use zygo_workspace::identity::Worktree;
use zygo_workspace::loadout::{self, pane_cwd, LoadoutError, Source};
use zygo_workspace::up::{find_checkout, GIT_BOUND};

use super::{op5_exit, print_err, print_out, user_config_or_exit};

const USER_INFO_REMOVED: &str = "<user info removed>";
const INLINE_JSON_HIDDEN: &str = "<inline JSON not shown>";
const VALUE_HIDDEN: &str = "<value not shown>";
/// The AU30 content flags and `--settings`: a value is shown only when it names a regular file (D4).
const CONTENT_FLAGS: [&str; 5] = ["--agents", "--mcp-config", "--json-schema", "--plugin-url", "--settings"];
/// A flag, a `NAME=value` part or a URL query parameter whose name (lower-cased) holds one of these has its value
/// hidden (D3; review round 2, findings 4, 5(c), 5(d)).
const SECRET_NAMES: [&str; 6] = ["key", "token", "secret", "password", "auth", "bearer"];

/// A scheme of 2 or more characters followed by `:/` or `:\` (D1 (a)); `C:\` is a drive letter, not a scheme.
static SCHEME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[A-Za-z][A-Za-z0-9+.-]+:[/\\]").expect("static pattern"));
/// A URL: a scheme and `://` (review round 2, 5(c)).
static URL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[A-Za-z][A-Za-z0-9+.-]*://").expect("static pattern"));
/// `user:pass@host` at the start of a text (review round 2, 5(a)): no `/` or blank before the first `:`, then an `@`,
/// with no blank between the `:` and that `@`.
static START_USER_PASS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[^/\s:]*:[^\s@]*@").expect("static pattern"));
/// A drive letter: one letter, `:`, then `\` or `/` (`C:\Users\…` is a path, not `user:pass@host`).
static DRIVE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z]:[/\\]").expect("static pattern"));

/// Review round 2, 5(a): the text starts with `user:pass@host` and is not a drive-letter path.
fn starts_with_user_pass(text: &str) -> bool {
    START_USER_PASS.is_match(text) && !DRIVE.is_match(text)
}

/// The text of a loadout field other than `args` as `loadout show`, `config show` and `config check` print it (D7;
/// review round 2, 3 and 5(f)): URL user info removed, then `<user info removed>` for a text that starts with
/// `user:pass@host`.
pub(crate) fn shown_field(text: &str) -> String {
    let t = scrub_userinfo(text);
    if starts_with_user_pass(&t) { USER_INFO_REMOVED.to_string() } else { t }
}

/// `lower` holds a [`SECRET_NAMES`] entry.
fn has_secret_word(lower: &str) -> bool {
    SECRET_NAMES.iter().any(|s| lower.contains(s))
}

/// A name of a `NAME=value` part: one or more of ASCII letters, digits, `_`, `-` and `.`.
fn is_name(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
}

/// Review round 2, finding 4 (and the D3 `=` form): the text is read as `p0=p1=…`; at the first part that is a name
/// with a secret word and has an `=` after it, the rest is `<value not shown>` (`--env=API_KEY=<value not shown>`).
fn hide_named_value(text: &str) -> Option<String> {
    let mut start = 0;
    for (at, _) in text.match_indices('=') {
        let name = &text[start..at];
        if is_name(name) && has_secret_word(&name.to_lowercase()) {
            return Some(format!("{}{VALUE_HIDDEN}", &text[..=at]));
        }
        start = at + 1;
    }
    None
}

/// Review round 2, 5(c): in each URL of the text, each query parameter whose name has a secret word shows as
/// `name=<value not shown>`. The query runs from the first `?` after the scheme to `#`, a blank or the end.
fn hide_query_secrets(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut done = 0;
    let mut from = 0;
    while let Some(m) = URL.find_at(text, from) {
        let url_end = text[m.end()..].find(char::is_whitespace).map_or(text.len(), |i| m.end() + i);
        from = url_end.max(m.end());
        let Some(q) = text[m.end()..url_end].find('?').map(|i| m.end() + i + 1) else { continue };
        let q_end = text[q..url_end].find('#').map_or(url_end, |i| q + i);
        let params: Vec<String> = text[q..q_end]
            .split('&')
            .map(|p| match p.split_once('=') {
                Some((name, _)) if has_secret_word(&name.to_lowercase()) => format!("{name}={VALUE_HIDDEN}"),
                _ => p.to_string(),
            })
            .collect();
        out.push_str(&text[done..q]);
        out.push_str(&params.join("&"));
        done = q_end;
    }
    out.push_str(&text[done..]);
    out
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

/// One token as `loadout show` and `config show` print it (D1-D5): D1 for any role, then the rule of the role. A shown
/// token then loses secret query values (5(c)) and secret `NAME=value` values (finding 4); a plain token then loses
/// inline JSON (D2).
fn shown_token(token: &str, role: Role) -> String {
    if user_info(token) {
        return USER_INFO_REMOVED.to_string();
    }
    let text = match role {
        Role::Content { attached: true, shown: false } => {
            return format!("{}={INLINE_JSON_HIDDEN}", token.split('=').next().unwrap_or(token));
        }
        Role::Content { attached: false, shown: false } => return INLINE_JSON_HIDDEN.to_string(),
        Role::SecretValue => return VALUE_HIDDEN.to_string(),
        Role::Content { shown: true, .. } | Role::Plain => hide_query_secrets(token),
    };
    if let Some(hidden) = hide_named_value(&text) {
        return hidden;
    }
    if role != Role::Plain {
        return text;
    }
    if json_like(&text) {
        return INLINE_JSON_HIDDEN.to_string();
    }
    if let Some((name, value)) = text.split_once('=')
        && json_like(value)
    {
        return format!("{name}={INLINE_JSON_HIDDEN}");
    }
    if let Some(value) = attached_short(&text).filter(|v| json_like(v)) {
        return format!("{}{INLINE_JSON_HIDDEN}", &text[..text.len() - value.len()]);
    }
    text
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
    // D3, review round 2 5(b): the next token after a secret-named flag without `=` is its value, also when it starts
    // with `-`; `--` is never a value here
    for i in 0..args.len().saturating_sub(1) {
        let token = &args[i];
        if roles[i] == Role::Plain
            && token.starts_with('-')
            && !token.contains('=')
            && secret_named(token)
            && roles[i + 1] == Role::Plain
            && args[i + 1] != "--"
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

/// D7: each pane's `args` through [`shown_args`] (with the pane's own folder, found before any scrub), a `server` with
/// user info (AU98) replaced, and every other text field through [`shown_field`] (review round 2, 5(f)).
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
            }
            p.kind = shown_field(&p.kind);
            scrub(&mut p.profile);
            scrub(&mut p.server);
            scrub(&mut p.compact);
            scrub(&mut p.config_dir);
            scrub(&mut p.cwd);
        }
        scrub(&mut w.name);
        scrub(&mut w.layout);
    }
}

/// The text of `zygo loadout show <name>`: `[loadout.<name>]` as TOML after [`hide_secrets`] (D7), each line through
/// `zygo_core::esc` as `zygo config show` prints each line (review round 2, finding 2).
fn show(name: &str, mut l: Loadout, base: &PaneBase) -> Result<String, toml::ser::Error> {
    hide_secrets(&mut l, base);
    let mut named = toml::Table::new();
    named.insert(name.to_string(), toml::Value::Table(toml::Table::try_from(&l)?));
    let mut top = toml::Table::new();
    top.insert("loadout".to_string(), toml::Value::Table(named));
    Ok(scrub_userinfo(&toml::to_string(&top)?).lines().map(|line| format!("{}\n", esc(line))).collect())
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

/// The OP5 check, the user config (D3), then D7: `<name escaped>  built-in` or `<name escaped>  user` per line, in the
/// order of `loadout::list`.
pub fn run(a: &LoadoutArgs) -> i32 {
    if let Some(code) = op5_exit() {
        return code;
    }
    let cfg = match user_config_or_exit(&Paths::from_env(), "loadout") {
        Ok(c) => c,
        Err(code) => return code,
    };
    match &a.sub {
        LoadoutCmd::List => {
            let text: String = loadout::list(&cfg)
                .into_iter()
                .map(|(name, source)| {
                    let source = match source {
                        Source::Builtin => "built-in",
                        Source::User => "user",
                    };
                    format!("{}  {source}\n", esc(&name))
                })
                .collect();
            print_out(&text);
            0
        }
        LoadoutCmd::Show { name } => {
            // D7: the user's `[loadout.<name>]` first, then the built-in `solo`
            let l = match cfg.loadout.get(name) {
                Some(l) => l.clone(),
                None if name == BUILTIN_LOADOUT => loadout::builtin_solo(),
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
        ("Authorization: Bearer tok-visible", "Authorization: Bearer tok-visible"),
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

    #[test] // F15; kills: a field left out of the scrub
    fn hide_secrets_scrubs_every_other_text_field() {
        let t = tempfile::tempdir().unwrap();
        let u = |n: u32| Some(format!("https://u:SECRET{n}@h.example.invalid/x"));
        let p = LoadoutPane { profile: u(3), compact: u(4), config_dir: u(5), cwd: u(6), ..pane(&[]) };
        let mut l = Loadout { window: vec![LoadoutWindow { name: u(1), layout: u(2), pane: vec![p] }] };
        hide_secrets(&mut l, &base(t.path()));
        let clean = Some("https://h.example.invalid/x".to_string());
        let w = &l.window[0];
        let p = &w.pane[0];
        assert_eq!([&w.name, &w.layout, &p.profile, &p.compact, &p.config_dir, &p.cwd], [&clean; 6]);
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

    #[test] // round 2 finding 5(b); kills: `!starts_with('-')` kept on the next token; `--` hidden as a value
    fn shown_args_hide_the_value_after_a_secret_flag_even_with_a_dash() {
        assert_eq!(
            shown(&["--api-token", "-SECRET1", "--auth", "--", "plain"]),
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
            ..pane(&[])
        };
        let mut l = Loadout { window: vec![LoadoutWindow { name: u(1), layout: u(2), pane: vec![p] }] };
        hide_secrets(&mut l, &base(t.path()));
        let g = Some(G.to_string());
        let w = &l.window[0];
        let p = &w.pane[0];
        assert_eq!(p.kind, G);
        assert_eq!([&w.name, &w.layout, &p.profile, &p.server, &p.compact, &p.config_dir, &p.cwd], [&g; 7]);
        // a profile name with an `@` and no `:` before it stays
        assert_eq!(shown_field("me@work"), "me@work");
    }

    #[test] // round 2 finding 8; kills: a removed `!token.contains('=')` guard (the `=` form hides its own value only)
    fn shown_args_keep_the_token_after_an_equals_secret_flag() {
        assert_eq!(shown(&["--x-key=SECRET1", "shown"]), strings(&["--x-key=<value not shown>", "shown"]));
    }
}
