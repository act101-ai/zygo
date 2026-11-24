//! The guards (§13.4): `zygo hook guard` on `PreToolUse`; claims and repeat from the `work` hook.

use std::collections::{BTreeMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
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

/// `PreToolUse` → [`pre_tool`]; other events → nothing. Units off or no config: redaction and attribution only (G3, FIX-21).
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

/// The `PreToolUse` guards in G2 order; `None` runs redaction and attribution only.
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
    let cwd = hooks::cwd_of(input)?;
    let command = if tool.eq_ignore_ascii_case("Bash") { tin.get("command").and_then(Value::as_str) } else { None };
    let segs = command.map(segments).unwrap_or_default();
    // FIX-21: always `enforce`, with or without work units and a config.
    if let Some(cmd) = command
        && let Some((label, place)) = find_attribution(cmd, Some(&cwd), 0)
    {
        events.push(Event { guard: Guard::Attribution, level: Level::Enforce, text: attribution_text(label, &place) });
    }
    if let Some((p, nav_lines)) = p {
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
        if let Some(cmd) = command {
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
    /// The subshells open where the segment starts: each `(` (a subshell or `$(`) before it, less each `)` (FIX-21 round 2
    /// F10).
    depth: usize,
    /// The body of each heredoc whose `<<` is in this segment, in order (FIX-21 round 2 F8).
    heredocs: Vec<String>,
}

impl Segment {
    fn new(join: Join, depth: usize) -> Segment {
        Segment { join, words: Vec::new(), depth, heredocs: Vec::new() }
    }

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
/// and `(( … ))` are one word; backslash-newline is removed; a `(` that opens a segment is skipped. FIX-21 round 2: each
/// segment keeps its subshell depth and the bodies of its heredocs.
fn segments(cmd: &str) -> Vec<Segment> {
    let mut depth = 0;
    let mut out = vec![Segment::new(Join::Start, depth)];
    let mut word: Option<String> = None;
    let mut chars = cmd.chars().peekable();
    let mut prev: Option<char> = None;
    // Heredoc delimiters that wait for the next newline, whether `<<-` strips leading tabs, and the segment of the `<<`.
    let mut heredocs: Vec<(String, bool, usize)> = Vec::new();
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
            '(' if out.last().is_some_and(|s| s.words.is_empty()) && word.is_none() => {
                depth += 1;
                out.last_mut().expect("one segment").depth = depth;
            }
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
                    heredocs.push((heredoc_delimiter(&mut chars), strip, out.len() - 1));
                }
            }
            '\n' => {
                end_word(&mut out, &mut word);
                for (d, strip, at) in heredocs.drain(..) {
                    let mut body = String::new();
                    loop {
                        let mut line = String::new();
                        while let Some(n) = chars.next_if(|n| *n != '\n') {
                            line.push(n);
                        }
                        let ended = chars.next().is_none();
                        let line = if strip { line.trim_start_matches('\t') } else { line.as_str() };
                        if line == d {
                            break;
                        }
                        body.push_str(line);
                        body.push('\n');
                        if ended {
                            break;
                        }
                    }
                    out[at].heredocs.push(body);
                }
                let last = out.last().expect("one segment");
                if !(last.join == Join::Pipe && last.words.is_empty()) {
                    out.push(Segment::new(Join::Other, depth));
                }
            }
            '|' | '&' | ';' | '(' | ')' => {
                end_word(&mut out, &mut word);
                match c {
                    '(' => depth += 1,
                    ')' => depth = depth.saturating_sub(1),
                    _ => {}
                }
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
                out.push(Segment::new(join, depth));
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

/// Round 2 fix 5: a `VAR=value` word.
fn assignment(x: &str) -> bool {
    x.split_once('=').is_some_and(|(n, _)| {
        n.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') && n.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
    })
}

/// The words of `s` from its program on: after a `{` that opens a group, leading `VAR=value` words and these wrappers
/// with their options (Round 3 fix 5; FIX-21 round 2 F6): `command` (`-p`; with `-v` or `-V` it runs nothing, so the
/// words stop there), `time`, `env` (then its `VAR=value` words), `exec`, `nohup`, `nice`, `stdbuf`, and `timeout` with
/// its duration.
fn program_words(s: &Segment) -> &[String] {
    let mut w: &[String] = s.command();
    loop {
        let Some(first) = w.first().map(String::as_str) else { return w };
        let rest = &w[1..];
        w = match first {
            x if assignment(x) => rest,
            "command" => match rest.first().map(String::as_str) {
                Some("-v" | "-V") => return w,
                _ => skip_options(rest, "", &[], 0),
            },
            "time" => skip_options(rest, "fo", &["format", "output"], 0),
            "env" => skip_options(rest, "uCS", &["unset", "chdir", "split-string"], 0),
            "exec" => skip_options(rest, "a", &[], 0),
            "nohup" => skip_options(rest, "", &[], 0),
            "nice" => skip_options(rest, "n", &["adjustment"], 0),
            "stdbuf" => skip_options(rest, "ioe", &["input", "output", "error"], 0),
            "timeout" => skip_options(rest, "sk", &["signal", "kill-after"], 1),
            _ => return w,
        };
    }
}

/// `w` after a wrapper's options, then `operands` more words. An option is a word that starts with `-` (a lone `-`
/// included); `--` ends them. A short option in `valued` takes the rest of its word or the next word; a long option
/// in `valued_long` takes the next word unless it has `=value` (FIX-21 round 2 F6).
fn skip_options<'a>(w: &'a [String], valued: &str, valued_long: &[&str], operands: usize) -> &'a [String] {
    let mut k = 0;
    while let Some(x) = w.get(k) {
        if x == "--" {
            k += 1;
            break;
        }
        if let Some(long) = x.strip_prefix("--") {
            k += if !long.contains('=') && valued_long.contains(&long) { 2 } else { 1 };
        } else if let Some(flags) = x.strip_prefix('-') {
            let at = flags.char_indices().find(|(_, c)| valued.contains(*c));
            k += match at {
                Some((i, c)) if i + c.len_utf8() == flags.len() => 2,
                _ => 1,
            };
        } else {
            break;
        }
    }
    w.get(k + operands..).unwrap_or_default()
}

/// Git global options that take the next word as their value (Round 2 fix 5).
const GIT_VALUED: [&str; 8] = ["-C", "-c", "--git-dir", "--work-tree", "--namespace", "--super-prefix", "--config-env", "--attr-source"];

/// For program words `w` that run `git` (which may be a path): the index of its subcommand and the directory its `-C`
/// options name, resolved from `cwd`; `None` for another program or no subcommand.
fn git_sub(w: &[String], cwd: &Path) -> Option<(usize, PathBuf)> {
    if !w.first().is_some_and(|x| x == "git" || x.ends_with("/git")) {
        return None;
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
        j += if GIT_VALUED.contains(&x.as_str()) { 2 } else { 1 };
    }
    w.get(j).map(|_| (j, dir))
}

/// G14: the text to search of a `git commit` with `-m`/`--message` (the whole command) or `-F`/`--file` (the file's last
/// `TAIL_BYTES`); `None` for no such commit, or a file message that cannot be read here.
fn commit_text(cmd: &str, segs: &[Segment], cwd: &Path) -> Option<String> {
    for (i, s) in segs.iter().enumerate() {
        let w = program_words(s);
        let Some((j, dir)) = git_sub(w, cwd) else { continue };
        if w[j] != "commit" {
            continue;
        }
        let (inline, file) = commit_args(w, j + 1);
        if inline {
            return Some(cmd.to_string());
        }
        let Some(f) = file else { continue };
        if f == "-" || f == "/dev/stdin" || after_cd(segs, i) {
            return None;
        }
        return read_last(&dir.join(f));
    }
    None
}

/// A segment before index `i` starts with `cd`: a relative file is then not resolved here.
fn after_cd(segs: &[Segment], i: usize) -> bool {
    segs[..i].iter().any(|s| s.words.first().map(String::as_str) == Some("cd"))
}

/// The option grammar of a git subcommand (git 2.50 `builtin/`): the short options that take a value (attached or the
/// next word), the short options that take only an attached one, the long options with whether each takes the next word,
/// and the long name the guards use for each short option they read.
struct Grammar {
    valued: &'static str,
    optional: &'static str,
    long: &'static [(&'static str, bool)],
    names: &'static [(char, &'static str)],
}

/// `git commit`: `-m`, `-F`, `-c`, `-C`, `-t` take a value; `-S`, `-u` an attached one.
const COMMIT: Grammar = Grammar { valued: "mFcCt", optional: "Su", long: &COMMIT_LONG, names: &[('m', "message"), ('F', "file")] };
/// `git tag` (FIX-21): `-m`, `-F`, `-u` take a value; `-n` an attached one.
const TAG: Grammar = Grammar { valued: "mFu", optional: "n", long: &TAG_LONG, names: &[('m', "message"), ('F', "file")] };
/// `git merge` (FIX-21 round 2 F5): `-m`, `-F`, `-s`, `-X` take a value; `-S` an attached one.
const MERGE: Grammar = Grammar { valued: "mFsX", optional: "S", long: &MERGE_LONG, names: &[('m', "message"), ('F', "file")] };
/// `git commit-tree` (FIX-21 round 2 F5): `-p`, `-m`, `-F` take a value; `-S` an attached one.
const COMMIT_TREE: Grammar =
    Grammar { valued: "pmF", optional: "S", long: &[("gpg-sign", false)], names: &[('m', "message"), ('F', "file")] };
/// `git notes add` and `git notes append` (FIX-21 round 2 F5): `-m`, `-F`, `-c`, `-C` take a value.
const NOTES: Grammar = Grammar { valued: "mFcC", optional: "", long: &NOTES_LONG, names: &[('m', "message"), ('F', "file")] };

/// The options in `w` from index `from` by `g`, in order, each as its long name and its value; `--` ends them. A long
/// option is named as [`long_option`] resolves it (Round 3 fix 5: a unique abbreviation as git's parse-options takes it;
/// unknown or ambiguous: left out); a short option not in `g.names` is left out.
fn options(w: &[String], from: usize, g: &Grammar) -> Vec<(&'static str, Option<String>)> {
    let mut out = Vec::new();
    let mut k = from;
    while let Some(x) = w.get(k) {
        if x == "--" {
            break;
        }
        if let Some(long) = x.strip_prefix("--") {
            let (name, attached) = match long.split_once('=') {
                Some((n, v)) => (n, Some(v.to_string())),
                None => (long, None),
            };
            if let Some((opt, valued)) = long_option(g.long, name) {
                let value = match attached {
                    Some(v) => Some(v),
                    None if valued => {
                        k += 1;
                        w.get(k).cloned()
                    }
                    None => None,
                };
                out.push((opt, value));
            }
        } else if let Some(flags) = x.strip_prefix('-') {
            for (n, ch) in flags.char_indices() {
                let rest = &flags[n + ch.len_utf8()..];
                if g.optional.contains(ch) {
                    break;
                }
                if g.valued.contains(ch) {
                    let value = if rest.is_empty() {
                        k += 1;
                        w.get(k).cloned()
                    } else {
                        Some(rest.to_string())
                    };
                    if let Some((_, name)) = g.names.iter().find(|(c, _)| *c == ch) {
                        out.push((*name, value));
                    }
                    break;
                }
            }
        }
        k += 1;
    }
    out
}

/// The `git commit` options in `w` from index `from`: whether a `-m`/`--message` is given, and the last `-F`/`--file` value.
fn commit_args(w: &[String], from: usize) -> (bool, Option<String>) {
    let o = options(w, from, &COMMIT);
    let file = o.iter().rev().find(|(n, _)| *n == "file").and_then(|(_, v)| v.clone());
    (o.iter().any(|(n, _)| *n == "message"), file)
}

/// The values of every `file` option in `o`.
fn files_of(o: Vec<(&'static str, Option<String>)>) -> Vec<String> {
    o.into_iter().filter(|(n, _)| *n == "file").filter_map(|(_, v)| v).collect()
}

/// FIX-21 round 2 F5: for `git notes` words `w` from index `from` (after `notes`): the index of an `add` or `append`
/// subcommand, after `--ref <ref>` or `--ref=<ref>`; `None` for another subcommand.
fn notes_sub(w: &[String], from: usize) -> Option<usize> {
    let mut k = from;
    loop {
        match w.get(k)?.as_str() {
            "--ref" => k += 2,
            x if x.starts_with("--ref=") => k += 1,
            "add" | "append" => return Some(k),
            _ => return None,
        }
    }
}

/// FIX-21, §13.4: for program words `w` of a message command, the directory its relative files resolve against (`base`,
/// joined with git's `-C` options) and the message, body or notes files it names: `git commit`, `git tag`, `git merge`,
/// `git commit-tree` and `git notes add|append` `-F`/`--file`, and the `gh` files of [`gh_files`]. `None` for another
/// command.
fn message_files(w: &[String], base: &Path) -> Option<(PathBuf, Vec<String>)> {
    if let Some((j, dir)) = git_sub(w, base) {
        let files = match w[j].as_str() {
            "commit" => files_of(options(w, j + 1, &COMMIT)),
            "tag" => files_of(options(w, j + 1, &TAG)),
            "merge" => files_of(options(w, j + 1, &MERGE)),
            "commit-tree" => files_of(options(w, j + 1, &COMMIT_TREE)),
            "notes" => files_of(options(w, notes_sub(w, j + 1)? + 1, &NOTES)),
            _ => return None,
        };
        return Some((dir, files));
    }
    gh_files(w).map(|f| (base.to_path_buf(), f))
}

/// FIX-21: for program words `w` that run `gh` (which may be a path) with `pr create|new|edit|comment|review|merge`,
/// `issue create|new|edit|comment` or `release create|new|edit`: the files named by `-F <f>`, `-F<f>`, `-F=<f>`,
/// `--body-file <f>`, `--body-file=<f>`, `--notes-file <f>` and `--notes-file=<f>`. The value of `-t`, `--title`,
/// `--subject`, `-b`, `--body`, `-n`, `--notes`, `-R` or `--repo` is never a file; `--` ends the options. `None` for
/// another command.
fn gh_files(w: &[String]) -> Option<Vec<String>> {
    if !w.first().is_some_and(|x| x == "gh" || x.ends_with("/gh")) {
        return None;
    }
    let verbs: &[&str] = match w.get(1).map(String::as_str) {
        Some("pr") => &["create", "new", "edit", "comment", "review", "merge"],
        Some("issue") => &["create", "new", "edit", "comment"],
        Some("release") => &["create", "new", "edit"],
        _ => return None,
    };
    if !w.get(2).is_some_and(|v| verbs.contains(&v.as_str())) {
        return None;
    }
    const TEXT: [&str; 9] = ["-t", "--title", "--subject", "-b", "--body", "-n", "--notes", "-R", "--repo"];
    let mut files = Vec::new();
    let mut k = 3;
    while let Some(x) = w.get(k) {
        let x = x.as_str();
        if x == "--" {
            break;
        }
        if TEXT.contains(&x) {
            k += 1;
        } else if matches!(x, "-F" | "--body-file" | "--notes-file") {
            k += 1;
            files.extend(w.get(k).cloned());
        } else if let Some(f) = x.strip_prefix("--body-file=").or_else(|| x.strip_prefix("--notes-file=")) {
            files.push(f.to_string());
        } else if let Some(f) = x.strip_prefix("-F") {
            files.push(f.strip_prefix('=').unwrap_or(f).to_string());
        }
        k += 1;
    }
    Some(files)
}

/// FIX-21 round 2 F10: the directory each segment of `segs` runs in, from `start`. `cd` and `pushd` (after a `{` that
/// opens a group) change it for the later segments at their subshell depth and deeper; the change ends with the
/// subshell. `None`: not known here (`cd -`, `popd`, a word with `$` or a backquote, `pushd` with no directory, or an
/// unknown start).
fn segment_dirs(segs: &[Segment], start: Option<PathBuf>) -> Vec<Option<PathBuf>> {
    let mut stack = vec![start];
    let mut out = Vec::with_capacity(segs.len());
    for s in segs {
        stack.truncate(s.depth + 1);
        while stack.len() < s.depth + 1 {
            let top = stack.last().cloned().flatten();
            stack.push(top);
        }
        let d = s.depth;
        out.push(stack[d].clone());
        let c = s.command();
        match c.first().map(String::as_str) {
            Some(p @ ("cd" | "pushd")) => stack[d] = cd_dir(p, &c[1..], stack[d].take()),
            Some("popd") => stack[d] = None,
            _ => {}
        }
    }
    out
}

/// FIX-21 round 2 F10: the directory `cd` or `pushd` (`program`) with `args` changes to from `cur`: an absolute path, or
/// a relative one joined to `cur`; `cd` with no directory, `~` and `~/<path>` go to `HOME`. `None` when not known here.
fn cd_dir(program: &str, args: &[String], cur: Option<PathBuf>) -> Option<PathBuf> {
    let mut a = args.iter().map(String::as_str).skip_while(|x| matches!(*x, "-L" | "-P" | "-e" | "-@" | "-n"));
    let target = match a.next() {
        Some("--") => a.next(),
        t => t,
    };
    let home = || std::env::var_os("HOME").map(PathBuf::from);
    match target {
        None if program == "cd" => home(),
        None => None,
        Some(t) if t.starts_with(['-', '+']) || t.contains(['$', '`']) => None,
        Some("~") => home(),
        Some(t) if t.starts_with("~/") => home().map(|h| h.join(&t[2..])),
        Some(t) if t.starts_with('~') => None,
        Some(t) if Path::new(t).is_absolute() => Some(PathBuf::from(t)),
        Some(t) => cur.map(|c| c.join(t)),
    }
}

/// FIX-21 round 2 F6: for program words `w` that run `bash`, `sh`, `zsh`, `dash` or `ksh` (which may be a path) with a
/// `-c` among its options: the command string, the first word after the options. `None` otherwise.
fn shell_string(w: &[String]) -> Option<&str> {
    let program = w.first()?.rsplit('/').next()?;
    if !matches!(program, "bash" | "sh" | "zsh" | "dash" | "ksh") {
        return None;
    }
    let mut c = false;
    let mut k = 1;
    while let Some(x) = w.get(k) {
        match x.as_str() {
            "--" => {
                k += 1;
                break;
            }
            "-o" | "+o" | "-O" | "+O" | "--rcfile" | "--init-file" => k += 2,
            x if x.starts_with("--") => k += 1,
            x if x.len() > 1 && x.starts_with(['-', '+']) => {
                c |= x.starts_with('-') && x.contains('c');
                k += 1;
            }
            _ => break,
        }
    }
    if c { w.get(k).map(String::as_str) } else { None }
}

/// The shell `-c` strings the attribution guard follows inside one another.
const MAX_SHELL_NEST: u32 = 4;

/// FIX-21, §13.4: the first Claude attribution in a message command of `cmd` ([`message_files`]), as its label and the
/// place it is in; `dir` is the directory `cmd` starts in (`None`: not known). For each message command, in order: its
/// arguments, quotes removed ("a command argument"); the heredoc bodies of it and of every command before it in `cmd`
/// ("a heredoc"; FIX-32 B4); each file it names (not `-` or `/dev/stdin`): when a command before it writes that file
/// ([`written_files`]), the words of the last such command and of the commands piped into it ("the command that writes
/// `<path>`"), and the file is not read; else the file read whole ("the file `<path>`"; a relative file only when its
/// directory is known, [`segment_dirs`]). A shell `-c` string ([`shell_string`]) is searched as a command. The rest of
/// `cmd` is not searched (round 2 F8).
fn find_attribution(cmd: &str, dir: Option<&Path>, nest: u32) -> Option<(&'static str, String)> {
    let segs = segments(cmd);
    let dirs = segment_dirs(&segs, dir.map(Path::to_path_buf));
    for (i, s) in segs.iter().enumerate() {
        let w = program_words(s);
        if let Some(inner) = shell_string(w) {
            if nest < MAX_SHELL_NEST
                && let Some(hit) = find_attribution(inner, dirs[i].as_deref(), nest + 1)
            {
                return Some(hit);
            }
            continue;
        }
        let Some((fdir, files)) = message_files(w, dirs[i].as_deref().unwrap_or(Path::new(""))) else { continue };
        if let Some(label) = claude_attribution(&w.join("\n")) {
            return Some((label, "a command argument".into()));
        }
        // FIX-32 B4 (b): the heredoc bodies of this command and of every command before it in `cmd` (any join).
        let bodies = s.heredocs.iter().chain(segs[..i].iter().flat_map(|e| &e.heredocs));
        if let Some(label) = bodies.into_iter().find_map(|b| claude_attribution(b)) {
            return Some((label, "a heredoc".into()));
        }
        for f in files.iter().filter(|f| *f != "-" && *f != "/dev/stdin") {
            let p = fdir.join(f);
            // FIX-32 B4 (a): a file that a command before this one writes is what that command writes, not what is on
            // disk now. Its heredoc bodies were searched above; its words, and those of the commands piped into it, are
            // searched here, and the file is not read.
            let writer = (0..i).rev().find(|&k| {
                let base = dirs[k].as_deref().unwrap_or(Path::new(""));
                written_files(&segs[k]).iter().any(|t| same_file(&base.join(t), &p))
            });
            if let Some(mut k) = writer {
                let mut texts = vec![segs[k].words.join("\n")];
                while k > 0 && segs[k].join == Join::Pipe {
                    k -= 1;
                    texts.push(segs[k].words.join("\n"));
                }
                if let Some(label) = texts.iter().find_map(|t| claude_attribution(t)) {
                    return Some((label, format!("the command that writes `{}`", p.display())));
                }
                continue;
            }
            if (dirs[i].is_some() || p.is_absolute())
                && let Some(label) = file_attribution(&p)
            {
                return Some((label, format!("the file `{}`", p.display())));
            }
        }
    }
    None
}

/// FIX-32 B4: the file that the redirection word `x` writes: `>`, `>>`, `>|`, `&>` or `&>>`, with an optional file
/// descriptor number before it (`1>`, `2>>`). `Some("")` when the file is the next word; `None` for any other word, a
/// read (`<`) and a duplication (`>&1`, `2>&1`).
fn redirect_target(x: &str) -> Option<&str> {
    let r = x.strip_prefix('&').unwrap_or_else(|| x.trim_start_matches(|c: char| c.is_ascii_digit()));
    let r = r.strip_prefix('>')?;
    let r = r.strip_prefix('>').unwrap_or(r);
    let r = r.strip_prefix('|').unwrap_or(r);
    if r.starts_with('&') { None } else { Some(r) }
}

/// FIX-32 B4: the files that segment `s` writes, as typed: each redirection target ([`redirect_target`]), and, when its
/// program ([`program_words`]) is `tee` (which may be a path), each operand of `tee` (its options skipped; `--` ends
/// them; a redirection word and its file are not operands).
fn written_files(s: &Segment) -> Vec<String> {
    let w = &s.words;
    let mut out = Vec::new();
    // The indices of the redirection words and of the words they take (the file of an operator that stands alone).
    let mut redirection = HashSet::new();
    let op = |x: &str| x.trim_start_matches(|c: char| c.is_ascii_digit() || c == '&').to_string();
    let mut k = 0;
    while let Some(x) = w.get(k) {
        if let Some(t) = redirect_target(x) {
            redirection.insert(k);
            if t.is_empty() {
                k += 1;
                redirection.insert(k);
                out.extend(w.get(k).cloned());
            } else {
                out.push(t.to_string());
            }
        } else if op(x).starts_with(['<', '>']) {
            // a read or a duplication, which writes no file
            redirection.insert(k);
            if op(x).chars().all(|c| matches!(c, '<' | '>' | '&' | '|')) {
                k += 1;
                redirection.insert(k);
            }
        }
        k += 1;
    }
    let p = program_words(s);
    if p.first().is_some_and(|x| x == "tee" || x.ends_with("/tee")) {
        let from = w.len() - p.len() + 1;
        let mut options = true;
        for (at, x) in w.iter().enumerate().skip(from) {
            if redirection.contains(&at) {
                continue;
            }
            if options && x == "--" {
                options = false;
            } else if !(options && x.len() > 1 && x.starts_with('-')) {
                out.push(x.clone());
            }
        }
    }
    out
}

/// FIX-32 B4: `a` and `b` name the same path, `.` parts left out (`./msg.txt` is `msg.txt`). No link is followed.
fn same_file(a: &Path, b: &Path) -> bool {
    a.components().filter(|c| *c != Component::CurDir).eq(b.components().filter(|c| *c != Component::CurDir))
}

/// FIX-21 round 2 F9: the first Claude attribution in the regular file `p`, read whole, one line at a time; `None` for
/// anything else (a FIFO or a device is never opened, as in [`read_last`]) or on a read error.
fn file_attribution(p: &Path) -> Option<&'static str> {
    if !std::fs::metadata(p).ok()?.is_file() {
        return None;
    }
    let mut r = BufReader::new(File::open(p).ok()?);
    let mut line = Vec::new();
    loop {
        line.clear();
        if r.read_until(b'\n', &mut line).ok()? == 0 {
            return None;
        }
        if let Some(label) = claude_attribution(&String::from_utf8_lossy(&line)) {
            return Some(label);
        }
    }
}

/// `s` holds `w` (lower case) as a whole word: no letter, digit or `_` right before or after it.
fn has_word(s: &str, w: &str) -> bool {
    let word = |c: char| c.is_alphanumeric() || c == '_';
    s.match_indices(w).any(|(i, _)| !s[..i].chars().next_back().is_some_and(word) && !s[i + w.len()..].chars().next().is_some_and(word))
}

/// FIX-21, §13.4: the first Claude attribution form in `text`, matched without case, as the label the guard text names:
/// `noreply@anthropic.com`; `Co-Authored-By`, optional blanks, `:` or `=`, then a name (up to `<` or the line end) that
/// holds `claude` as a whole word; `Generated with`, optional blanks, then `[Claude Code]`, or `Claude Code` as whole
/// words; `Generated by`, optional blanks, then `claude` as a whole word; `🤖`, optional blanks or U+FE0F, then
/// `Generated`; `Claude-Session:`; `https://claude.ai/code/` then a path. A product name such as "Claude Code plugin" is
/// not attribution.
pub fn claude_attribution(text: &str) -> Option<&'static str> {
    let t = text.to_lowercase();
    // What follows each occurrence of `m`, leading blanks and U+FE0F removed.
    let after = |m: &str| t.match_indices(m).map(|(i, _)| t[i + m.len()..].trim_start_matches([' ', '\t', '\u{fe0f}'])).collect::<Vec<_>>();
    let word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let word_end = |s: &str| !s.starts_with(word);
    if t.contains("noreply@anthropic.com") {
        return Some("noreply@anthropic.com");
    }
    let co_author = |r: &&str| r.strip_prefix([':', '=']).is_some_and(|n| has_word(n.split(['<', '\n']).next().unwrap_or_default(), "claude"));
    if after("co-authored-by").iter().any(co_author) {
        return Some("Co-Authored-By: Claude");
    }
    for r in after("generated with") {
        if r.starts_with("[claude code]") {
            return Some("Generated with [Claude Code]");
        }
        if r.strip_prefix("claude code").is_some_and(word_end) {
            return Some("Generated with Claude Code");
        }
    }
    if after("generated by").iter().any(|r| r.strip_prefix("claude").is_some_and(word_end)) {
        return Some("Generated by Claude");
    }
    if after("🤖").iter().any(|r| r.starts_with("generated")) {
        return Some("🤖 Generated");
    }
    if t.contains("claude-session:") {
        return Some("Claude-Session:");
    }
    if t.match_indices("https://claude.ai/code/").any(|(i, m)| t[i + m.len()..].starts_with(word)) {
        return Some("https://claude.ai/code/");
    }
    None
}

/// §13.4: the attribution guard's text for the matched form `label` found in `place`.
fn attribution_text(label: &str, place: &str) -> String {
    format!("zygo: Claude attribution is never allowed in commits, tags, notes, pull requests, issues, comments, reviews or releases. `{label}` is in {place}; reword that text without it and run the command again.")
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

/// `git tag`'s long options (git 2.50 `builtin/tag.c`) and whether each takes a value in the next word (FIX-21).
/// `--column` and `--color` take one only after `=`; `--contains`, `--merged` and `--points-at` take the next word.
const TAG_LONG: [(&str, bool); 24] = [
    ("list", false), ("delete", false), ("verify", false), ("annotate", false), ("message", true), ("file", true),
    ("trailer", true), ("edit", false), ("sign", false), ("cleanup", true), ("local-user", true), ("force", false),
    ("create-reflog", false), ("column", false), ("contains", true), ("no-contains", true), ("merged", true),
    ("no-merged", true), ("points-at", true), ("sort", true), ("color", false), ("ignore-case", false),
    ("omit-empty", false), ("format", true),
];

/// `git merge`'s long options (git 2.50 `builtin/merge.c`) and whether each takes a value in the next word (FIX-21 round
/// 2 F5). `--log` and `--gpg-sign` take one only after `=`.
const MERGE_LONG: [(&str, bool); 29] = [
    ("stat", false), ("summary", false), ("log", false), ("compact-summary", false), ("squash", false), ("commit", false),
    ("edit", false), ("cleanup", true), ("ff", false), ("ff-only", false), ("rerere-autoupdate", false),
    ("verify-signatures", false), ("strategy", true), ("strategy-option", true), ("message", true), ("file", true),
    ("into-name", true), ("verbose", false), ("quiet", false), ("abort", false), ("quit", false), ("continue", false),
    ("allow-unrelated-histories", false), ("progress", false), ("gpg-sign", false), ("autostash", false),
    ("overwrite-ignore", false), ("signoff", false), ("verify", false),
];

/// `git notes add|append`'s long options (git 2.50 `builtin/notes.c`) and whether each takes a value in the next word
/// (FIX-21 round 2 F5). `--separator` takes one only after `=`.
const NOTES_LONG: [(&str, bool); 8] = [
    ("message", true), ("file", true), ("reuse-message", true), ("reedit-message", true), ("force", false),
    ("allow-empty", false), ("separator", false), ("stripspace", false),
];

/// The long option of `table` that `name` names, exactly or as a unique abbreviation, with whether it takes the next word
/// as its value; `None` for a `no-` form or a name git would refuse (unknown or ambiguous).
fn long_option(table: &[(&'static str, bool)], name: &str) -> Option<(&'static str, bool)> {
    if let Some(o) = table.iter().find(|(o, _)| *o == name) {
        return Some(*o);
    }
    if name.is_empty() || name.starts_with("no-") {
        return None;
    }
    let mut hits = table.iter().filter(|(o, _)| o.starts_with(name));
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
