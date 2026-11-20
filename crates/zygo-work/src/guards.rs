//! The guards (§13.4): `zygo hook guard` on `PreToolUse`; claims and repeat from the `work` hook.

use std::collections::{BTreeMap, HashSet};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;
use std::time::Instant;

use serde_json::{json, Value};
use zygo_config::{Level, Merged};
use zygo_core::hook::{HookInput, HookOutput};
use zygo_core::overrides::Overrides;
use zygo_core::session_dir::SessionDir;
use zygo_core::session_view::SessionView;

use crate::hooks::{self, HookCtx};
use crate::log::{self, AgentLock, Begin, CheckResult, Kind, LogLine};
use crate::policy::{Guard, Policy};
use crate::repo;

/// §16.1: the redaction sentence.
const REDACTION: &str = "`[REDACTED:<id>]` marks a secret zygo removed; never write it into a file.";
/// §13.4: the output guard's text.
const OUTPUT: &str = "zygo: do not shorten output with `| head`/`| tail`; zygo stores large results and gives you `result_search`.";

/// One guard event: the guard, its level in force and its text.
struct Event {
    guard: Guard,
    level: Level,
    text: String,
}

/// `zygo hook guard`; outside a session it does nothing.
pub fn run(input: &HookInput) -> anyhow::Result<HookOutput> {
    let Some(sd) = SessionDir::from_env() else { return Ok(HookOutput::None) };
    run_with(input, &hooks::env_ctx(&sd))
}

/// `PreToolUse` → [`pre_tool`]; other events → nothing. Units off or no config: redaction only (G3).
pub fn run_with(input: &HookInput, ctx: &HookCtx) -> anyhow::Result<HookOutput> {
    if input.event.as_deref() != Some("PreToolUse") {
        return Ok(HookOutput::None);
    }
    let cwd = hooks::cwd_of(input)?;
    let merged = if hooks::units_on(ctx, &cwd) { (ctx.config)(&cwd) } else { None };
    match merged {
        Some(m) => pre_tool(input, &policy(&m, ctx.session_dir, &top(&cwd)), m.config.navigation.lines, ctx),
        None => guard_tool(input, None, ctx),
    }
}

pub fn pre_tool(input: &HookInput, p: &Policy, nav_lines: u32, ctx: &HookCtx) -> anyhow::Result<HookOutput> {
    guard_tool(input, Some((p, nav_lines)), ctx)
}

/// The `PreToolUse` guards in G2 order; `None` runs redaction only.
fn guard_tool(input: &HookInput, p: Option<(&Policy, u32)>, ctx: &HookCtx) -> anyhow::Result<HookOutput> {
    let (agent, _agent_type) = hooks::agent_of(input)?;
    if log::check_agent(&agent).is_err() {
        return Ok(HookOutput::None);
    }
    let tool = input.tool_name.as_deref().unwrap_or_default();
    let tin = input.tool_input.as_ref().unwrap_or(&Value::Null);
    let mut events = Vec::new();
    if redaction(tool, tin) {
        events.push(Event { guard: Guard::Redaction, level: Level::Enforce, text: REDACTION.into() });
    }
    if let Some((p, nav_lines)) = p {
        let cwd = hooks::cwd_of(input)?;
        let unit = log::open_unit(&ctx.session_dir.work(), &agent);
        let edit_path = ["Edit", "Write", "NotebookEdit"]
            .iter()
            .any(|t| t.eq_ignore_ascii_case(tool))
            .then(|| ["file_path", "notebook_path"].iter().find_map(|k| tin.get(k)?.as_str()))
            .flatten();
        if let (true, Some(b), Some(raw)) = (input.has_agent_id, &unit, edit_path)
            && !b.files.is_empty()
            && let Some(path) = out_of_scope(&cwd.join(raw), &b.files, &ctx.temp_roots)
        {
            let level = p.level(Guard::Scope);
            let mut text = format!("zygo: `{path}` is not in this unit's files; if the change is needed, report it.");
            if level == Level::Enforce {
                text.push_str(" Ask the lead to widen the brief.");
            }
            events.push(Event { guard: Guard::Scope, level, text });
        }
        if tool.eq_ignore_ascii_case("Read")
            && tin.get("limit").is_none()
            && let Some(raw) = tin.get("file_path").and_then(Value::as_str)
            && SessionView::load(ctx.session_dir).is_ok_and(|v| v.navigation)
        {
            let path = hooks::normalize(&cwd.join(raw));
            if is_source(&path) && over_lines(&path, nav_lines) && !navigated(ctx, &agent, unit.as_ref(), &path) {
                let text = format!("zygo: call skeleton or symbols on `{}` before reading all of it.", path.display());
                events.push(Event { guard: Guard::Navigation, level: p.level(Guard::Navigation), text });
            }
        }
        let command = if tool.eq_ignore_ascii_case("Bash") { tin.get("command").and_then(Value::as_str) } else { None };
        if let Some(cmd) = command {
            let segs = segments(cmd);
            if store_on(ctx.session_dir) && shortens_output(&segs) {
                events.push(Event { guard: Guard::Output, level: p.level(Guard::Output), text: OUTPUT.into() });
            }
            if commit_text(cmd, &segs, &cwd).is_some_and(|t| lacks_trailers(&t, unit.is_some())) {
                let u = unit.as_ref().map_or("<unit>", |b| b.unit.as_str());
                let text = format!("Add the commit trailers `Zygo-Session: {}` and, when your brief names a unit, `Zygo-Work: {u}`, each on its own line at the end of the message.", ctx.session);
                events.push(Event { guard: Guard::Trailers, level: p.level(Guard::Trailers), text });
            }
        }
    }
    let written = record(ctx, &agent, events)?;
    Ok(print(&written, "PreToolUse"))
}

/// One `guard` line per event; the events to print. A line that cannot be written is an error for every guard (G4, C6).
/// A full log (`append` → `Ok(None)`) is not an error (Round 3 fix 1): navigation then prints nothing, as its history is
/// incomplete (§13.3); every other guard prints its decision.
fn record(ctx: &HookCtx, agent: &str, events: Vec<Event>) -> anyhow::Result<Vec<Event>> {
    if events.is_empty() {
        return Ok(events);
    }
    let g = log::lock(&ctx.session_dir.work(), agent)?;
    let mut shown = Vec::new();
    for e in events {
        if g.append(&LogLine { n: 0, ts: ctx.now, kind: guard_line(e.guard, e.level) })?.is_some() || e.guard != Guard::Navigation {
            shown.push(e);
        }
    }
    Ok(shown)
}

fn guard_line(g: Guard, l: Level) -> Kind {
    Kind::Guard { guard: g.name().into(), level: l.name().into() }
}

/// §13.4 output: enforce wins; the enforce texts, then the notify texts, joined by `\n` (G2).
fn print(events: &[Event], hook_event: &str) -> HookOutput {
    let texts = |l: Level| events.iter().filter(move |e| e.level == l).map(|e| e.text.as_str());
    let enforce: Vec<&str> = texts(Level::Enforce).collect();
    let all: Vec<&str> = texts(Level::Enforce).chain(texts(Level::Notify)).collect();
    if all.is_empty() {
        return HookOutput::None;
    }
    let t = all.join("\n");
    HookOutput::Json(match (enforce.is_empty(), hook_event) {
        (false, "SubagentStop") => json!({"decision": "block", "reason": t}),
        (false, _) => json!({"hookSpecificOutput": {"hookEventName": hook_event, "permissionDecision": "deny", "permissionDecisionReason": t}}),
        (true, _) => json!({"hookSpecificOutput": {"hookEventName": hook_event, "additionalContext": t}}),
    })
}

/// G15: the new content of `Write`, `Edit` or `NotebookEdit` holds `[REDACTED:` (§16.1).
fn redaction(tool: &str, tin: &Value) -> bool {
    let field = [("Write", "content"), ("Edit", "new_string"), ("NotebookEdit", "new_source")]
        .iter()
        .find(|(t, _)| t.eq_ignore_ascii_case(tool))
        .map(|(_, f)| *f);
    field.and_then(|f| tin.get(f)?.as_str()).is_some_and(|s| s.contains("[REDACTED:"))
}

/// The result store is on (§10.1): the session view exists and `overrides.json` or the view says so (`trim.rs`).
fn store_on(sd: &SessionDir) -> bool {
    SessionView::load(sd).is_ok_and(|v| Overrides::load(sd).results.unwrap_or(v.results))
}

/// How a shell segment was joined to the one before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Join {
    /// The first segment.
    Start,
    /// `|` or `|&`.
    Pipe,
    /// `||`, `&&`, `&`, `;`, newline, `(`, `)`.
    Other,
}

/// One pipeline segment: how it joins the one before it and its words, quotes removed.
#[derive(Debug)]
struct Segment {
    join: Join,
    words: Vec<String>,
}

impl Segment {
    /// The words from the segment's command on: a `{` that opens a group is skipped (Round 3 fix 3).
    fn command(&self) -> &[String] {
        let skip = self.words.iter().take_while(|w| *w == "{").count();
        &self.words[skip..]
    }
}

/// G12: shell words and segments. Single quotes, double quotes (`\"`, `\\`), backslash escapes; the operators
/// split segments only outside quotes. `&` after `>` or `<`, or before `>`, is part of a redirection word.
/// Round 2 fix 7: a `#` that starts a word comments to the newline; heredoc bodies (`<<`, `<<-`, quoted delimiters)
/// are skipped to their terminator line; a newline right after `|` continues the pipeline. Round 3 fix 3: `$(( … ))`
/// and `(( … ))` are one word; backslash-newline is removed; a `(` that opens a segment is skipped.
fn segments(cmd: &str) -> Vec<Segment> {
    let mut out = vec![Segment { join: Join::Start, words: Vec::new() }];
    let mut word: Option<String> = None;
    let mut chars = cmd.chars().peekable();
    let mut prev: Option<char> = None;
    // Heredoc delimiters that wait for the next newline, and whether `<<-` strips leading tabs.
    let mut heredocs: Vec<(String, bool)> = Vec::new();
    fn end_word(out: &mut [Segment], word: &mut Option<String>) {
        if let Some(w) = word.take() {
            out.last_mut().expect("one segment").words.push(w);
        }
    }
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                let w = word.get_or_insert_with(String::new);
                for q in chars.by_ref() {
                    if q == '\'' {
                        break;
                    }
                    w.push(q);
                }
            }
            '"' => {
                let w = word.get_or_insert_with(String::new);
                while let Some(q) = chars.next() {
                    match q {
                        '"' => break,
                        '\\' => match chars.peek() {
                            Some(&n @ ('"' | '\\')) => {
                                w.push(n);
                                chars.next();
                            }
                            _ => w.push('\\'),
                        },
                        q => w.push(q),
                    }
                }
            }
            // Backslash-newline continues the line (Round 3 fix 3).
            '\\' if chars.next_if_eq(&'\n').is_some() => continue,
            '\\' => {
                let w = word.get_or_insert_with(String::new);
                if let Some(n) = chars.next() {
                    w.push(n);
                }
            }
            // `$((` or a `((` command: arithmetic, one word to its closing `))`, so its `<<` is a shift (Round 3 fix 3).
            '(' if chars.peek() == Some(&'(') && word.as_deref().is_none_or(|w| w.ends_with('$')) => {
                let w = word.get_or_insert_with(String::new);
                w.push(c);
                let mut depth = 1;
                while depth > 0
                    && let Some(n) = chars.next()
                {
                    depth += match n {
                        '(' => 1,
                        ')' => -1,
                        _ => 0,
                    };
                    w.push(n);
                }
            }
            // A `(` that opens a segment opens no new one, so the segment's command is the word after it.
            '(' if out.last().is_some_and(|s| s.words.is_empty()) && word.is_none() => {}
            '&' if matches!(prev, Some('>' | '<')) || chars.peek() == Some(&'>') => word.get_or_insert_with(String::new).push(c),
            '#' if word.is_none() => while chars.next_if(|n| *n != '\n').is_some() {},
            '<' if chars.peek() == Some(&'<') => {
                chars.next();
                if chars.next_if_eq(&'<').is_some() {
                    word.get_or_insert_with(String::new).push_str("<<<"); // a here-string
                } else {
                    end_word(&mut out, &mut word);
                    let strip = chars.next_if_eq(&'-').is_some();
                    while chars.next_if(|n| *n == ' ' || *n == '\t').is_some() {}
                    heredocs.push((heredoc_delimiter(&mut chars), strip));
                }
            }
            '\n' => {
                end_word(&mut out, &mut word);
                for (d, strip) in heredocs.drain(..) {
                    loop {
                        let mut line = String::new();
                        while let Some(n) = chars.next_if(|n| *n != '\n') {
                            line.push(n);
                        }
                        let ended = chars.next().is_none();
                        if (if strip { line.trim_start_matches('\t') } else { line.as_str() }) == d || ended {
                            break;
                        }
                    }
                }
                let last = out.last().expect("one segment");
                if !(last.join == Join::Pipe && last.words.is_empty()) {
                    out.push(Segment { join: Join::Other, words: Vec::new() });
                }
            }
            '|' | '&' | ';' | '(' | ')' => {
                end_word(&mut out, &mut word);
                let join = match (c, chars.peek()) {
                    ('|', Some('|')) | ('&', Some('&')) => {
                        chars.next();
                        Join::Other
                    }
                    ('|', Some('&')) => {
                        chars.next();
                        Join::Pipe
                    }
                    ('|', _) => Join::Pipe,
                    _ => Join::Other,
                };
                out.push(Segment { join, words: Vec::new() });
            }
            c if c.is_whitespace() => end_word(&mut out, &mut word),
            c => word.get_or_insert_with(String::new).push(c),
        }
        prev = Some(c);
    }
    end_word(&mut out, &mut word);
    out
}

/// A heredoc delimiter word, quotes removed: up to a blank or an operator character.
fn heredoc_delimiter(chars: &mut std::iter::Peekable<std::str::Chars>) -> String {
    let mut d = String::new();
    while let Some(c) = chars.next_if(|n| !n.is_whitespace() && !"|&;()<>".contains(*n)) {
        match c {
            '\'' | '"' => {
                while let Some(q) = chars.next_if(|n| *n != c) {
                    d.push(q);
                }
                chars.next();
            }
            '\\' => d.extend(chars.next()),
            c => d.push(c),
        }
    }
    d
}

/// G13: a segment after `|`/`|&` that is `head` or `tail` with nothing, `-n N`, `-c N` or `-N` (N digits).
fn shortens_output(segs: &[Segment]) -> bool {
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    segs.iter().filter(|s| s.join == Join::Pipe).any(|s| {
        let w: Vec<&str> = s.command().iter().map(String::as_str).collect();
        match w.as_slice() {
            [c, rest @ ..] if *c == "head" || *c == "tail" => match rest {
                [] => true,
                [f, n] if *f == "-n" || *f == "-c" => digits(n),
                [f] => f.strip_prefix('-').is_some_and(digits),
                _ => false,
            },
            _ => false,
        }
    })
}

/// act-lite's language list (§10.5 note 2026-11-19): `(file names, extensions)`.
fn languages() -> &'static (Vec<String>, HashSet<String>) {
    static LANGS: OnceLock<(Vec<String>, HashSet<String>)> = OnceLock::new();
    LANGS.get_or_init(|| {
        let v: Value = serde_json::from_str(include_str!("../../../fixtures/act-lite/languages.json")).expect("languages.json parses");
        let (mut names, mut exts) = (Vec::new(), HashSet::new());
        for l in v["languages"].as_array().into_iter().flatten() {
            let strs = |k: &str| l[k].as_array().into_iter().flatten().filter_map(Value::as_str).map(String::from).collect::<Vec<_>>();
            names.extend(strs("file_names"));
            exts.extend(strs("extensions"));
        }
        (names, exts)
    })
}

/// `name` matches `pat`, where `*` is any run of characters.
fn glob(pat: &str, name: &str) -> bool {
    match pat.split_once('*') {
        None => pat == name,
        Some((head, rest)) => {
            let Some(tail) = name.strip_prefix(head) else { return false };
            (0..=tail.len()).filter(|i| tail.is_char_boundary(*i)).any(|i| glob(rest, &tail[i..]))
        }
    }
}

/// G10: a regular file whose name or extension is in act-lite's list, exact case.
fn is_source(p: &Path) -> bool {
    if !std::fs::metadata(p).is_ok_and(|m| m.is_file()) {
        return false;
    }
    let (names, exts) = languages();
    let name = p.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
    names.iter().any(|n| glob(n, &name)) || p.extension().is_some_and(|e| exts.contains(e.to_string_lossy().as_ref()))
}

/// G10: more than `lines` lines; a file over `TAIL_BYTES` counts as over without reading; unreadable → false.
fn over_lines(p: &Path, lines: u32) -> bool {
    let Ok(f) = File::open(p) else { return false };
    let Ok(m) = f.metadata() else { return false };
    if m.len() > log::TAIL_BYTES {
        return true;
    }
    let mut buf = Vec::new();
    if f.take(log::TAIL_BYTES).read_to_end(&mut buf).is_err() {
        return false;
    }
    let mut n: u64 = 0;
    for b in &buf {
        if *b == b'\n' {
            n += 1;
            if n > u64::from(lines) {
                return true;
            }
        }
    }
    if buf.last().is_some_and(|b| *b != b'\n') {
        n += 1;
    }
    n > u64::from(lines)
}

/// G11: `p` or a parent dir is among this agent's last `REPEAT_WINDOW` `navigated` paths since the open unit's begin
/// (else in the whole tail). Incomplete history (fewer paths and the scope start outside the window) or `overflow` → true.
fn navigated(ctx: &HookCtx, agent: &str, unit: Option<&Begin>, p: &Path) -> bool {
    let tail = log::tail(&ctx.session_dir.work(), agent);
    if tail.iter().any(|(_, l)| l.kind == Kind::Overflow) {
        return true;
    }
    let from = unit.map_or(0, |b| b.offset);
    let whole = match unit {
        Some(b) => tail.iter().any(|(o, _)| *o == b.offset),
        None => tail.first().is_none_or(|(o, _)| *o == 0),
    };
    let paths: Vec<&String> = tail
        .iter()
        .filter(|(o, _)| *o >= from)
        .filter_map(|(_, l)| match &l.kind {
            Kind::Navigated { paths } => Some(paths),
            _ => None,
        })
        .flatten()
        .collect();
    if paths.len() < log::REPEAT_WINDOW && !whole {
        return true;
    }
    paths.iter().rev().take(log::REPEAT_WINDOW).any(|q| p.starts_with(Path::new(q.as_str())))
}

/// G8 (Round 3 fix 2): physical resolution, as the OS does on a write. From the root, one component at a time: a
/// component that exists is canonicalised (a symlink is followed before any later `..`); `..` is applied lexically only
/// inside the part that does not exist. A symlink that does not resolve (dangling), the leaf included, is judged by its
/// target: `read_link`, resolved against the link's directory.
fn resolve(p: &Path) -> PathBuf {
    resolve_in(PathBuf::new(), p, 0)
}

/// Symlink hops followed before a component is kept as it is (the OS stops with `ELOOP` there, so nothing is written).
const MAX_LINK_HOPS: u32 = 40;

/// [`resolve`] of `p` from `base`, a canonical path or empty; `hops` symlinks followed so far.
fn resolve_in(base: PathBuf, p: &Path, hops: u32) -> PathBuf {
    let mut cur = base;
    for c in p.components() {
        match c {
            Component::Prefix(_) | Component::RootDir => cur.push(c),
            Component::CurDir => {}
            // `cur` is canonical while it exists, so its parent is the physical one; in the missing part, lexical.
            Component::ParentDir => {
                cur.pop();
            }
            Component::Normal(n) => {
                let next = cur.join(n);
                cur = match next.canonicalize() {
                    Ok(c) => c,
                    Err(_) => match std::fs::read_link(&next) {
                        Ok(target) if hops < MAX_LINK_HOPS => resolve_in(cur, &target, hops + 1),
                        _ => next,
                    },
                };
            }
        }
    }
    cur
}

/// G9: `temp_dir` (production: the system temp directory), `/tmp` and `/var/tmp` (the last two on Unix), canonicalised;
/// one that does not exist is left out.
pub fn temp_roots(temp_dir: &Path) -> Vec<PathBuf> {
    let mut roots = vec![temp_dir.to_path_buf()];
    if cfg!(unix) {
        roots.extend([PathBuf::from("/tmp"), PathBuf::from("/var/tmp")]);
    }
    roots.into_iter().filter_map(|r| r.canonicalize().ok()).collect()
}

/// G8, G9: `Some(<path>)` for the event text when `p` is not listed in `files` (relative to its own repository root, `/`
/// separated; absolute when it has no repository root); `None` when it is listed or under a temp root.
fn out_of_scope(p: &Path, files: &[String], temp_roots: &[PathBuf]) -> Option<String> {
    let p = resolve(p);
    if temp_roots.iter().any(|r| p.starts_with(r)) {
        return None;
    }
    let Some(root) = p.parent().and_then(hooks::repo_root) else { return Some(p.display().to_string()) };
    let rel = p.strip_prefix(&root).ok()?;
    let rel = rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/");
    let listed = files.iter().any(|f| *f == rel || (f.ends_with('/') && rel.starts_with(f.as_str())));
    (!listed).then_some(rel)
}

/// G14: the text to search of a `git commit` with `-m`/`--message` (the whole command) or `-F`/`--file` (the file's last
/// `TAIL_BYTES`); `None` for no such commit, or a file message that cannot be read here.
fn commit_text(cmd: &str, segs: &[Segment], cwd: &Path) -> Option<String> {
    // Round 2 fix 5: leading `VAR=value` words; git global options, the listed ones with a separate value.
    let assignment = |x: &str| x.split_once('=').is_some_and(|(n, _)| {
        n.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') && n.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
    });
    const VALUED: [&str; 8] = ["-C", "-c", "--git-dir", "--work-tree", "--namespace", "--super-prefix", "--config-env", "--attr-source"];
    for (i, s) in segs.iter().enumerate() {
        // Round 3 fix 5: `env` (with `-i`, `-u NAME`, `VAR=value`), `command` and `time` before `git`, which may be a path.
        let mut w: &[String] = s.command();
        loop {
            match w.first().map(String::as_str) {
                Some(x) if assignment(x) => w = &w[1..],
                Some("command" | "time") => w = &w[1..],
                Some("env") => {
                    w = &w[1..];
                    loop {
                        match w.first().map(String::as_str) {
                            Some("-i") => w = &w[1..],
                            Some("-u") => w = w.get(2..).unwrap_or_default(),
                            Some(x) if assignment(x) => w = &w[1..],
                            _ => break,
                        }
                    }
                }
                _ => break,
            }
        }
        if !w.first().is_some_and(|x| x == "git" || x.ends_with("/git")) {
            continue;
        }
        let mut dir = cwd.to_path_buf();
        let mut j = 1;
        while let Some(x) = w.get(j)
            && x.starts_with('-')
        {
            if *x == "-C"
                && let Some(d) = w.get(j + 1)
            {
                dir = dir.join(d);
            }
            j += if VALUED.contains(&x.as_str()) { 2 } else { 1 };
        }
        if w.get(j).map(|x| x.as_str()) != Some("commit") {
            continue;
        }
        let (mut inline, mut file) = (false, None);
        let mut k = j + 1;
        while let Some(x) = w.get(k) {
            if let Some(long) = x.strip_prefix("--") {
                // Round 3 fix 5: a long option or its unique abbreviation, as git's parse-options takes it.
                let (name, attached) = match long.split_once('=') {
                    Some((n, v)) => (n, Some(v.to_string())),
                    None => (long, None),
                };
                if let Some((opt, valued)) = commit_option(name) {
                    let value = match attached {
                        Some(v) => Some(v),
                        None if valued => {
                            k += 1;
                            w.get(k).map(|v| v.to_string())
                        }
                        None => None,
                    };
                    match opt {
                        "message" => inline = true,
                        "file" => file = value,
                        _ => {}
                    }
                }
            } else if let Some(flags) = x.strip_prefix('-').filter(|f| !f.starts_with('-')) {
                // A short-option cluster: `m`, `F`, `c`, `C`, `t` take a value, attached or the next word; `S`, `u` an
                // optional attached one.
                for (n, ch) in flags.char_indices() {
                    let rest = &flags[n + ch.len_utf8()..];
                    if matches!(ch, 'S' | 'u') {
                        break;
                    }
                    if matches!(ch, 'm' | 'F' | 'c' | 'C' | 't') {
                        let value = if rest.is_empty() {
                            k += 1;
                            w.get(k).map(|v| v.to_string())
                        } else {
                            Some(rest.to_string())
                        };
                        match ch {
                            'm' => inline = true,
                            'F' => file = value,
                            _ => {}
                        }
                        break;
                    }
                }
            }
            k += 1;
        }
        if inline {
            return Some(cmd.to_string());
        }
        let Some(f) = file else { continue };
        if f == "-" || f == "/dev/stdin" || segs[..i].iter().any(|s| s.words.first().map(String::as_str) == Some("cd")) {
            return None;
        }
        return read_last(&dir.join(f));
    }
    None
}

/// `git commit`'s long options (git 2.50 `builtin/commit.c`, hidden ones included) and whether each takes a value in
/// the next word. `--gpg-sign` and `--untracked-files` take one only after `=`.
const COMMIT_LONG: [(&str, bool); 40] = [
    ("quiet", false), ("verbose", false), ("file", true), ("author", true), ("date", true), ("message", true),
    ("reedit-message", true), ("reuse-message", true), ("fixup", true), ("squash", true), ("reset-author", false),
    ("trailer", true), ("signoff", false), ("template", true), ("edit", false), ("cleanup", true), ("status", false),
    ("gpg-sign", false), ("all", false), ("include", false), ("interactive", false), ("patch", false), ("only", false),
    ("no-verify", false), ("verify", false), ("dry-run", false), ("short", false), ("branch", false), ("ahead-behind", false),
    ("porcelain", false), ("long", false), ("null", false), ("amend", false), ("no-post-rewrite", false),
    ("post-rewrite", false), ("untracked-files", false), ("pathspec-from-file", true), ("pathspec-file-nul", false),
    ("allow-empty", false), ("allow-empty-message", false),
];

/// The `git commit` long option `name` names, exactly or as a unique abbreviation, with whether it takes the next word as
/// its value; `None` for a `no-` form or a name git would refuse (unknown or ambiguous).
fn commit_option(name: &str) -> Option<(&'static str, bool)> {
    if let Some(o) = COMMIT_LONG.iter().find(|(o, _)| *o == name) {
        return Some(*o);
    }
    if name.is_empty() || name.starts_with("no-") {
        return None;
    }
    let mut hits = COMMIT_LONG.iter().filter(|(o, _)| o.starts_with(name));
    match (hits.next(), hits.next()) {
        (Some(o), None) => Some(*o),
        _ => None,
    }
}

/// The last `TAIL_BYTES` of a regular file (§13.3 hook read bound); `None` for anything else or a read error.
fn read_last(p: &Path) -> Option<String> {
    if !std::fs::metadata(p).ok()?.is_file() {
        return None;
    }
    let mut f = File::open(p).ok()?;
    let len = f.metadata().ok()?.len();
    f.seek(SeekFrom::Start(len.saturating_sub(log::TAIL_BYTES))).ok()?;
    let mut buf = Vec::new();
    f.take(log::TAIL_BYTES).read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// §13.4: no `Zygo-Session:`, or a unit open and no `Zygo-Work:`.
fn lacks_trailers(text: &str, unit_open: bool) -> bool {
    !text.contains("Zygo-Session:") || (unit_open && !text.contains("Zygo-Work:"))
}

/// The levels in force: config, then each session override alone, clamped to `min_level` (M7). Also feeds the begin hook's `levels` line.
pub(crate) fn policy(m: &Merged, sd: &SessionDir, top: &Path) -> Policy {
    let mut cur = m.clone();
    for (g, l) in Overrides::load(sd).guards {
        let Ok(l) = l.parse::<Level>() else { continue };
        let l = l.max(cur.config.guards.min_level);
        let one = Overrides { guards: BTreeMap::from([(g, l.name().to_string())]), ..Default::default() };
        if let Ok(next) = cur.with_overrides(&one) {
            cur = next;
        }
    }
    Policy::resolve(&cur, top)
}

/// The checkout top for `cwd` (N23 note), else `cwd`.
pub(crate) fn top(cwd: &Path) -> PathBuf {
    repo::checkout(cwd, Instant::now() + repo::BOUND).map(|c| c.top).unwrap_or_else(|| cwd.to_path_buf())
}

/// Claims (§13.4) from the `work` hook's SubagentStop path, under `g`, before `stopped_at` and the fold.
pub(crate) fn claims(g: &AgentLock, b: &Begin, p: &Policy, ctx: &HookCtx) -> anyhow::Result<HookOutput> {
    let tail = g.tail();
    // G5: only when the log shows the whole unit.
    if !tail.iter().any(|(o, _)| *o == b.offset) || tail.iter().any(|(_, l)| l.kind == Kind::Overflow) {
        return Ok(HookOutput::None);
    }
    let unit: Vec<&Kind> = tail.iter().filter(|(o, _)| *o > b.offset).map(|(_, l)| &l.kind).collect();
    let checked = unit.iter().any(|k| matches!(k, Kind::Check { .. }));
    let last_edit = unit.iter().rposition(|k| matches!(k, Kind::Edit { .. }));
    let passed_after = |i: usize| unit[i..].iter().any(|k| matches!(k, Kind::Check { result: CheckResult::Passed }));
    if checked && last_edit.is_none_or(passed_after) {
        return Ok(HookOutput::None);
    }
    let level = p.level(Guard::Claims);
    let line = guard_line(Guard::Claims, level);
    // G6: continue once; the unit's log decides, not `stop_hook_active`.
    let blocked_before = unit.contains(&&line);
    // A line not written (log full) prints nothing, at every level (§13.3; Round 3 fix 1).
    if g.append(&LogLine { n: 0, ts: ctx.now, kind: line })?.is_none() {
        return Ok(HookOutput::None);
    }
    let text = match level {
        Level::Monitor => return Ok(HookOutput::None),
        Level::Notify => format!("zygo: this subagent did not run its acceptance check after its last edit (`{}`).", b.unit),
        Level::Enforce if !blocked_before => format!(
            "zygo: run the acceptance check `{}` alone, in the repository you changed, after your last edit, and quote its output in your report; say so if it fails.",
            b.check
        ),
        Level::Enforce => return Ok(HookOutput::None),
    };
    Ok(print(&[Event { guard: Guard::Claims, level, text }], "SubagentStop"))
}

/// `{"t":"guard","guard":"repeat","level":"monitor"}` (§13.4: monitor only).
pub(crate) fn repeat_event() -> Kind {
    guard_line(Guard::Repeat, Level::Monitor)
}
