//! Shell command parse for output shaping (O§7.1, O§7.2, O§7.3, O§7.6).
//!
//! The parse never fails and never panics. Every place where it is unsure
//! counts as a file printer, so the call is not shaped (O§7.1).

use std::mem::take;

/// Only this many bytes of a command are read; a longer one is unsure (O§7.1, O§7.2).
pub const MAX_COMMAND_BYTES: usize = 4096;

/// A nesting deeper than this is unsure (O§7.1).
pub const MAX_DEPTH: usize = 4;

/// The closed list of stored command names (O§7.2).
pub const NAMES: &[&str] = &[
    "cargo", "go", "pytest", "python", "python3", "uv", "npm", "pnpm", "yarn", "npx", "node", "make", "just", "git",
    "gh", "docker", "kubectl", "terraform", "ls", "find", "tree", "du", "ps", "cat", "head", "tail", "sed", "awk",
    "grep", "rg", "jq", "curl",
];

/// Programs that may print a file (O§7.1); `git show` and `git cat-file` are found by subcommand.
const PRINTERS: &[&str] = &["cat", "head", "tail", "sed", "awk", "nl", "bat", "less", "more", "grep", "rg"];

/// Wrappers that are skipped before the program (O§7.2), with their options that take an
/// argument. `exec` and `nohup` are added toward safety (they run the next word).
const WRAPPERS: &[(&str, &[&str])] = &[
    ("env", &["-u", "-C", "--unset", "--chdir"]),
    ("time", &["-f", "-o", "--format", "--output"]),
    ("sudo", &["-u", "-g", "-h", "-p", "-C", "-D", "-r", "-t", "-T", "-U"]),
    ("nice", &["-n", "--adjustment"]),
    ("command", &[]),
    ("exec", &["-a"]),
    ("nohup", &[]),
];

/// Reserved words that are skipped at the start of a stage.
const RESERVED: &[&str] = &["!", "{", "}", "if", "then", "else", "elif", "fi", "do", "done", "while", "until"];

/// `xargs` options that take the next word as their argument.
const XARGS_TAKES: &[&str] = &["-I", "-n", "-P", "-L", "-d", "-a", "-E", "-s"];

// Subcommand lists (O§7.2, O§15 item 1).
const GIT: &[&str] = &[
    "add", "am", "apply", "archive", "bisect", "blame", "branch", "bundle", "cat-file", "checkout", "cherry-pick",
    "clean", "clone", "commit", "config", "describe", "diff", "fetch", "for-each-ref", "format-patch", "gc", "grep",
    "init", "log", "ls-files", "ls-remote", "ls-tree", "merge", "merge-base", "mv", "notes", "pull", "push",
    "range-diff", "rebase", "reflog", "remote", "reset", "restore", "rev-list", "rev-parse", "revert", "rm",
    "shortlog", "show", "show-ref", "stash", "status", "submodule", "switch", "tag", "worktree",
];
const CARGO: &[&str] = &[
    "add", "bench", "build", "check", "clean", "clippy", "doc", "fetch", "fix", "fmt", "generate-lockfile", "init",
    "install", "locate-project", "login", "metadata", "new", "nextest", "owner", "package", "pkgid", "publish",
    "remove", "report", "run", "rustc", "rustdoc", "search", "test", "tree", "uninstall", "update", "vendor",
    "verify-project", "version", "yank",
];
const GO: &[&str] = &[
    "bug", "build", "clean", "doc", "env", "fix", "fmt", "generate", "get", "install", "list", "mod", "run", "test",
    "tool", "version", "vet", "work",
];
const NPM: &[&str] = &[
    "access", "audit", "ci", "config", "dedupe", "exec", "explain", "fund", "init", "install", "link", "ls",
    "outdated", "pack", "publish", "rebuild", "run", "run-script", "start", "stop", "test", "uninstall", "update",
    "version", "view", "why",
];
const PNPM: &[&str] = &[
    "add", "audit", "dlx", "exec", "install", "list", "outdated", "publish", "remove", "run", "start", "store", "test",
    "update", "why",
];
const YARN: &[&str] = &[
    "add", "audit", "dlx", "exec", "info", "init", "install", "link", "outdated", "pack", "publish", "remove", "run",
    "start", "test", "up", "upgrade", "why", "workspace", "workspaces",
];
const DOCKER: &[&str] = &[
    "attach", "build", "buildx", "compose", "cp", "create", "exec", "images", "info", "inspect", "kill", "load",
    "login", "logout", "logs", "network", "ps", "pull", "push", "restart", "rm", "rmi", "run", "save", "start",
    "stats", "stop", "system", "tag", "top", "version", "volume",
];
const KUBECTL: &[&str] = &[
    "annotate", "apply", "auth", "autoscale", "config", "cp", "create", "delete", "describe", "diff", "edit", "events",
    "exec", "explain", "expose", "get", "label", "logs", "patch", "port-forward", "proxy", "rollout", "run", "scale",
    "set", "top", "version", "wait",
];
const GH: &[&str] = &[
    "api", "auth", "browse", "cache", "codespace", "extension", "gist", "issue", "label", "pr", "release", "repo",
    "run", "search", "secret", "ssh-key", "status", "variable", "workflow",
];

/// A test runner whose view may apply (O§7.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runner {
    Libtest,
    Pytest,
    Gotest,
}

const ALL_RUNNERS: &[Runner] = &[Runner::Libtest, Runner::Pytest, Runner::Gotest];

/// What output shaping needs to know about a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    /// The command may print a file, or the parse is unsure (O§7.1).
    pub file_printer: bool,
    /// The stored name (O§7.2): a closed-list name, maybe with a subcommand, or `other`.
    pub name: String,
    /// The runners to try, in order (O§7.6).
    pub runners: Vec<Runner>,
    /// A segment prints text that the model may reuse (O§7.3).
    pub reuse: bool,
}

/// Parses a shell command. Never panics.
pub fn parse(cmd: &str) -> Parsed {
    let mut cut = cmd.len().min(MAX_COMMAND_BYTES);
    while !cmd.is_char_boundary(cut) {
        cut -= 1;
    }
    let mut acc = Acc { unsure: cmd.len() > MAX_COMMAND_BYTES, ..Acc::default() };
    let script = lex(&cmd[..cut], 0, &mut acc.unsure);
    visit(&script, 0, true, &mut acc);
    Parsed {
        file_printer: acc.file_printer || acc.unsure,
        name: acc.runner_name.or(acc.list_name).unwrap_or_else(|| "other".into()),
        runners: acc.runners,
        reuse: acc.reuse,
    }
}

/// One shell word after quote removal.
#[derive(Default)]
struct Word {
    text: String,
    /// The word held `$…`, `${…}`, `$( )` or a backtick; their raw text is kept in `text`.
    dynamic: bool,
}

/// One pipeline stage.
#[derive(Default)]
struct Stage {
    words: Vec<Word>,
    /// Scripts met in this stage: `( )`, `$( )`, backticks, `<( )`, `>( )`.
    nested: Vec<Script>,
}

#[derive(Default)]
struct Script {
    /// Each segment is a list of pipeline stages.
    segments: Vec<Vec<Stage>>,
    /// Here-doc bodies, lexed as commands.
    bodies: Vec<Script>,
}

/// What the next finished word is for.
#[derive(Default, Clone, Copy)]
enum Redir {
    #[default]
    No,
    /// A redirection target: not argv.
    Target,
    /// A here-doc delimiter; `true` for `<<-`.
    Heredoc(bool),
}

/// The state of one script level while it is lexed.
#[derive(Default)]
struct Builder {
    script: Script,
    seg: Vec<Stage>,
    stage: Stage,
    word: Option<Word>,
    redir: Redir,
    heredocs: Vec<(String, bool)>,
}

impl Builder {
    fn word(&mut self) -> &mut Word {
        self.word.get_or_insert_with(Word::default)
    }

    fn end_word(&mut self) {
        let Some(w) = self.word.take() else { return };
        match take(&mut self.redir) {
            Redir::No => self.stage.words.push(w),
            Redir::Target => {}
            Redir::Heredoc(strip) => self.heredocs.push((w.text, strip)),
        }
    }

    fn end_stage(&mut self) {
        self.end_word();
        let st = take(&mut self.stage);
        if !st.words.is_empty() || !st.nested.is_empty() {
            self.seg.push(st);
        }
    }

    fn end_segment(&mut self) {
        self.end_stage();
        self.redir = Redir::No;
        let seg = take(&mut self.seg);
        if !seg.is_empty() {
            self.script.segments.push(seg);
        }
    }

    fn finish(mut self) -> Script {
        self.end_segment();
        self.script
    }
}

struct Lexer {
    c: Vec<char>,
    i: usize,
    unsure: bool,
}

/// Lexes `s` as a script at `depth`; sets `unsure` when the lex is unsure.
fn lex(s: &str, depth: usize, unsure: &mut bool) -> Script {
    let mut lx = Lexer { c: s.chars().collect(), i: 0, unsure: false };
    let script = lx.script(depth, None);
    *unsure |= lx.unsure;
    script
}

impl Lexer {
    fn peek(&self, k: usize) -> Option<char> {
        self.c.get(self.i + k).copied()
    }

    /// Lexes to the end of the text, or to the `close` bracket. Unsure stops the lex.
    fn script(&mut self, depth: usize, close: Option<char>) -> Script {
        if depth > MAX_DEPTH {
            self.unsure = true;
            return Script::default();
        }
        let mut b = Builder::default();
        while !self.unsure {
            let Some(c) = self.peek(0) else {
                if close.is_some() {
                    self.unsure = true;
                }
                break;
            };
            match c {
                ' ' | '\t' | '\r' => {
                    b.end_word();
                    self.i += 1;
                }
                '\n' => {
                    self.i += 1;
                    b.end_segment();
                    self.bodies(&mut b, depth);
                }
                ';' => {
                    self.i += 1;
                    b.end_segment();
                }
                '&' if self.peek(1) == Some('>') => {
                    b.end_word();
                    self.i += 2;
                    if self.peek(0) == Some('>') {
                        self.i += 1;
                    }
                    b.redir = Redir::Target;
                }
                '&' => {
                    self.i += if self.peek(1) == Some('&') { 2 } else { 1 };
                    b.end_segment();
                }
                '|' => {
                    if self.peek(1) == Some('|') {
                        self.i += 2;
                        b.end_segment();
                    } else {
                        self.i += if self.peek(1) == Some('&') { 2 } else { 1 };
                        b.end_stage();
                    }
                }
                ')' => {
                    self.i += 1;
                    if close == Some(')') {
                        return b.finish();
                    }
                    self.unsure = true;
                }
                '(' => {
                    b.end_word();
                    self.i += 1;
                    let s = self.script(depth + 1, Some(')'));
                    b.stage.nested.push(s);
                }
                '<' | '>' => self.redirect(&mut b, depth),
                '#' if b.word.is_none() => {
                    while self.peek(0).is_some_and(|ch| ch != '\n') {
                        self.i += 1;
                    }
                }
                _ => self.word_part(&mut b, depth),
            }
        }
        b.finish()
    }

    fn word_part(&mut self, b: &mut Builder, depth: usize) {
        match self.c[self.i] {
            '\\' => {
                self.i += 1;
                if let Some(ch) = self.peek(0) {
                    self.i += 1;
                    if ch != '\n' {
                        b.word().text.push(ch);
                    }
                }
            }
            '\'' => {
                self.i += 1;
                match self.c[self.i..].iter().position(|&ch| ch == '\'') {
                    Some(n) => {
                        let s: String = self.c[self.i..self.i + n].iter().collect();
                        b.word().text.push_str(&s);
                        self.i += n + 1;
                    }
                    None => self.unsure = true,
                }
            }
            '"' => self.dquote(b, depth),
            '$' => self.dollar(b, depth, false),
            '`' => self.backtick(b, depth),
            ch => {
                b.word().text.push(ch);
                self.i += 1;
            }
        }
    }

    fn dquote(&mut self, b: &mut Builder, depth: usize) {
        self.i += 1;
        b.word();
        while !self.unsure {
            let Some(c) = self.peek(0) else {
                self.unsure = true;
                return;
            };
            match c {
                '"' => {
                    self.i += 1;
                    return;
                }
                '\\' => match self.peek(1) {
                    Some('\n') => self.i += 2,
                    Some(n @ ('$' | '`' | '"' | '\\')) => {
                        b.word().text.push(n);
                        self.i += 2;
                    }
                    _ => {
                        b.word().text.push('\\');
                        self.i += 1;
                    }
                },
                '$' => self.dollar(b, depth, true),
                '`' => self.backtick(b, depth),
                _ => {
                    b.word().text.push(c);
                    self.i += 1;
                }
            }
        }
    }

    /// Appends the raw text from `start` to the current position to the word, as dynamic.
    fn raw(&self, b: &mut Builder, start: usize) {
        let s: String = self.c[start..self.i.min(self.c.len())].iter().collect();
        let w = b.word();
        w.text.push_str(&s);
        w.dynamic = true;
    }

    fn dollar(&mut self, b: &mut Builder, depth: usize, in_dq: bool) {
        let start = self.i;
        match self.peek(1) {
            Some('(') => {
                self.i += 2;
                let s = self.script(depth + 1, Some(')'));
                b.stage.nested.push(s);
                self.raw(b, start);
            }
            Some('{') => match self.c[self.i..].iter().position(|&ch| ch == '}') {
                Some(n) => {
                    self.i += n + 1;
                    self.raw(b, start);
                }
                None => self.unsure = true,
            },
            Some('\'') if !in_dq => {
                self.i += 2;
                b.word();
                loop {
                    match self.peek(0) {
                        None => {
                            self.unsure = true;
                            return;
                        }
                        Some('\'') => {
                            self.i += 1;
                            return;
                        }
                        Some('\\') if self.peek(1).is_some() => {
                            let n = self.c[self.i + 1];
                            b.word().text.push(n);
                            self.i += 2;
                        }
                        Some(ch) => {
                            b.word().text.push(ch);
                            self.i += 1;
                        }
                    }
                }
            }
            Some(ch) if ch.is_ascii_alphabetic() || ch == '_' => {
                self.i += 1;
                while self.peek(0).is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_') {
                    self.i += 1;
                }
                self.raw(b, start);
            }
            Some(ch) if ch.is_ascii_digit() || "@*#?$!-".contains(ch) => {
                self.i += 2;
                self.raw(b, start);
            }
            _ => {
                b.word().text.push('$');
                self.i += 1;
            }
        }
    }

    fn backtick(&mut self, b: &mut Builder, depth: usize) {
        let start = self.i;
        self.i += 1;
        let mut inner = String::new();
        loop {
            match self.peek(0) {
                None => {
                    self.unsure = true;
                    return;
                }
                Some('\\') if matches!(self.peek(1), Some('`' | '\\' | '$')) => {
                    inner.push(self.c[self.i + 1]);
                    self.i += 2;
                }
                Some('`') => {
                    self.i += 1;
                    break;
                }
                Some(ch) => {
                    inner.push(ch);
                    self.i += 1;
                }
            }
        }
        let s = lex(&inner, depth + 1, &mut self.unsure);
        b.stage.nested.push(s);
        self.raw(b, start);
    }

    /// Lexes a redirection operator; its target word is not argv. Digits just before it
    /// are an IO number. `<( )` and `>( )` are looked into.
    fn redirect(&mut self, b: &mut Builder, depth: usize) {
        let c = self.c[self.i];
        if b.word
            .as_ref()
            .is_some_and(|w| !w.dynamic && !w.text.is_empty() && w.text.bytes().all(|x| x.is_ascii_digit()))
        {
            b.word = None;
        }
        b.end_word();
        if self.peek(1) == Some('(') {
            let start = self.i;
            self.i += 2;
            let s = self.script(depth + 1, Some(')'));
            b.stage.nested.push(s);
            self.raw(b, start);
            return;
        }
        self.i += 1;
        let mut heredoc = None;
        if c == '<' {
            match self.peek(0) {
                Some('<') => {
                    self.i += 1;
                    match self.peek(0) {
                        Some('<') => self.i += 1,
                        Some('-') => {
                            self.i += 1;
                            heredoc = Some(true);
                        }
                        _ => heredoc = Some(false),
                    }
                }
                Some('&' | '>') => self.i += 1,
                _ => {}
            }
        } else if matches!(self.peek(0), Some('>' | '&' | '|')) {
            self.i += 1;
        }
        b.redir = heredoc.map_or(Redir::Target, Redir::Heredoc);
    }

    /// After a newline, reads the bodies of the pending here-docs and lexes them as commands.
    fn bodies(&mut self, b: &mut Builder, depth: usize) {
        for (delim, strip) in take(&mut b.heredocs) {
            let mut body = String::new();
            while self.i < self.c.len() {
                let end = self.c[self.i..].iter().position(|&ch| ch == '\n').map_or(self.c.len(), |n| self.i + n);
                let line: String = self.c[self.i..end].iter().collect();
                self.i = (end + 1).min(self.c.len());
                let check = if strip { line.trim_start_matches('\t') } else { line.as_str() };
                if check == delim {
                    break;
                }
                body.push_str(&line);
                body.push('\n');
            }
            let s = lex(&body, depth + 1, &mut self.unsure);
            b.script.bodies.push(s);
        }
    }
}

/// Text after the last `/`.
fn base(w: &str) -> &str {
    w.rsplit('/').next().unwrap_or(w)
}

/// A leading `NAME=value` word.
fn is_assign(t: &str) -> bool {
    let Some((k, _)) = t.split_once('=') else {
        return false;
    };
    let mut cs = k.chars();
    cs.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') && cs.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Strips `NAME=value` words, reserved words and wrappers (with their options). The second
/// value is the string of `env -S`, which is parsed in turn.
fn argv(words: &[Word]) -> (&[Word], Option<String>) {
    let mut i = 0;
    while let Some(w) = words.get(i) {
        let t = w.text.as_str();
        if is_assign(t) || RESERVED.contains(&t) {
            i += 1;
            continue;
        }
        let Some(&(wrapper, takes)) = WRAPPERS.iter().find(|(n, _)| *n == base(t)) else {
            break;
        };
        i += 1;
        while let Some(o) = words.get(i).map(|w| w.text.as_str()) {
            if !o.starts_with('-') {
                break;
            }
            i += 1;
            if o == "--" {
                break;
            }
            if wrapper == "env" {
                if o == "-S" || o == "--split-string" {
                    return (&words[(i + 1).min(words.len())..], words.get(i).map(|w| w.text.clone()));
                }
                if let Some(s) = o.strip_prefix("--split-string=").or_else(|| o.strip_prefix("-S")) {
                    return (&words[i..], Some(s.to_string()));
                }
            }
            if takes.contains(&o) {
                i += 1;
            }
        }
    }
    (&words[i.min(words.len())..], None)
}

/// The subcommand: the first word in `list`, skipping options and the word after an option.
fn sub(args: &[Word], list: &[&'static str]) -> Option<&'static str> {
    let mut prev_opt = false;
    for w in args {
        let t = w.text.as_str();
        if t.starts_with('-') || t.starts_with('+') {
            prev_opt = true;
            continue;
        }
        if let Some(s) = list.iter().find(|s| **s == t) {
            return Some(s);
        }
        if !prev_opt {
            return None;
        }
        prev_opt = false;
    }
    None
}

fn subs_of(name: &str) -> Option<&'static [&'static str]> {
    Some(match name {
        "git" => GIT,
        "cargo" => CARGO,
        "go" => GO,
        "npm" => NPM,
        "pnpm" => PNPM,
        "yarn" => YARN,
        "docker" => DOCKER,
        "kubectl" => KUBECTL,
        "gh" => GH,
        _ => return None,
    })
}

/// The stored name of a program and its args (O§7.2, O-C2).
fn stored(name: &str, rest: &[Word]) -> String {
    if !NAMES.contains(&name) {
        return "other".into();
    }
    match subs_of(name).and_then(|l| sub(rest, l)) {
        Some(s) => format!("{name} {s}"),
        None => name.into(),
    }
}

/// The runners that a stage names (O§7.6).
fn runners_of(name: &str, rest: &[Word]) -> &'static [Runner] {
    let has = |a: &str, b: &str| {
        let mut it = rest.iter().filter(|w| !w.text.starts_with('-'));
        it.next().is_some_and(|w| w.text == a) && it.next().is_some_and(|w| w.text == b)
    };
    match name {
        "cargo" if sub(rest, CARGO) == Some("test") => &[Runner::Libtest],
        "go" if sub(rest, GO) == Some("test") => &[Runner::Gotest],
        "pytest" | "py.test" => &[Runner::Pytest],
        "python" | "python3"
            if rest
                .iter()
                .position(|w| w.text == "-m")
                .and_then(|k| rest.get(k + 1))
                .is_some_and(|w| w.text == "pytest") =>
        {
            &[Runner::Pytest]
        }
        "uv" | "poetry" if has("run", "pytest") => &[Runner::Pytest],
        "make" | "just" if sub(rest, &["test"]).is_some() => ALL_RUNNERS,
        "npm" | "pnpm" | "yarn" if subs_of(name).and_then(|l| sub(rest, l)) == Some("test") => ALL_RUNNERS,
        _ => &[],
    }
}

#[derive(Default)]
struct Acc {
    unsure: bool,
    file_printer: bool,
    reuse: bool,
    runners: Vec<Runner>,
    runner_name: Option<String>,
    list_name: Option<String>,
}

fn visit(s: &Script, depth: usize, top: bool, acc: &mut Acc) {
    for seg in &s.segments {
        for (k, st) in seg.iter().enumerate() {
            stage(&st.words, depth, top, k == 0, acc);
            for n in &st.nested {
                visit(n, depth + 1, false, acc);
            }
        }
    }
    for body in &s.bodies {
        visit(body, depth + 1, false, acc);
    }
}

/// Lexes and visits a string that a shell runs (`-c`, `env -S`) one level deeper.
fn nest(text: &str, depth: usize, acc: &mut Acc) {
    let s = lex(text, depth + 1, &mut acc.unsure);
    visit(&s, depth + 1, false, acc);
}

/// Visits command words that another program runs (`xargs`, `find -exec`) one level deeper.
fn inner(words: &[Word], depth: usize, acc: &mut Acc) {
    if depth + 1 > MAX_DEPTH {
        acc.unsure = true;
        return;
    }
    stage(words, depth + 1, false, false, acc);
}

fn stage(words: &[Word], depth: usize, top: bool, first: bool, acc: &mut Acc) {
    let (args, split) = argv(words);
    if let Some(s) = split {
        nest(&s, depth, acc);
    }
    let Some((prog, rest)) = args.split_first() else {
        return;
    };
    if prog.dynamic {
        acc.unsure = true;
        return;
    }
    let name = base(&prog.text);
    if PRINTERS.contains(&name) {
        acc.file_printer = true;
    }
    let has = |opts: &[&str]| rest.iter().any(|w| opts.contains(&w.text.as_str()));
    match name {
        "git" => match sub(rest, GIT) {
            Some("show") => {
                acc.file_printer = true;
                acc.reuse = true;
            }
            Some("cat-file") => acc.file_printer = true,
            Some("diff") => acc.reuse = true,
            Some("log") if has(&["-p", "--patch"]) => acc.reuse = true,
            Some("format-patch") if has(&["--stdout"]) => acc.reuse = true,
            _ => {}
        },
        "diff" => acc.reuse = true,
        "bash" | "sh" | "zsh" => {
            let flag = rest.iter().position(|w| {
                let t = &w.text;
                t.starts_with('-') && !t.starts_with("--") && t.contains('c')
            });
            if let Some(w) = flag.and_then(|k| rest[k + 1..].iter().find(|w| !w.text.starts_with('-'))) {
                nest(&w.text, depth, acc);
            }
        }
        "xargs" => {
            let mut i = 0;
            while let Some(w) = rest.get(i) {
                if !w.text.starts_with('-') {
                    break;
                }
                i += 1;
                if XARGS_TAKES.contains(&w.text.as_str()) {
                    i += 1;
                }
            }
            inner(&rest[i.min(rest.len())..], depth, acc);
        }
        "find" => {
            let mut i = 0;
            while i < rest.len() {
                if matches!(rest[i].text.as_str(), "-exec" | "-execdir" | "-ok" | "-okdir") {
                    let from = i + 1;
                    let end = rest[from..]
                        .iter()
                        .position(|w| w.text == ";" || w.text == "+")
                        .map_or(rest.len(), |n| from + n);
                    inner(&rest[from..end], depth, acc);
                    i = end;
                }
                i += 1;
            }
        }
        _ => {}
    }
    if !top {
        return;
    }
    let runners = runners_of(name, rest);
    if !runners.is_empty() {
        for r in runners {
            if !acc.runners.contains(r) {
                acc.runners.push(*r);
            }
        }
        if acc.runner_name.is_none() {
            acc.runner_name = Some(stored(name, rest));
        }
    }
    if first && acc.list_name.is_none() && NAMES.contains(&name) {
        acc.list_name = Some(stored(name, rest));
    }
}
