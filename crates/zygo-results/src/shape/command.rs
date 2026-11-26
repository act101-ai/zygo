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
    "cargo",
    "go",
    "pytest",
    "python",
    "python3",
    "uv",
    "npm",
    "pnpm",
    "yarn",
    "npx",
    "node",
    "make",
    "just",
    "git",
    "gh",
    "docker",
    "kubectl",
    "terraform",
    "ls",
    "find",
    "tree",
    "du",
    "ps",
    "cat",
    "head",
    "tail",
    "sed",
    "awk",
    "grep",
    "rg",
    "jq",
    "curl",
];

/// Programs that may print a file (O§7.1); `git show`, `git cat-file`, `git grep` and
/// `git blame` are found by subcommand.
const PRINTERS: &[&str] = &[
    "cat", "head", "tail", "sed", "awk", "nl", "bat", "less", "more", "grep", "rg", "egrep",
    "fgrep", "zcat", "gawk", "batcat", "tac",
];

/// Wrappers that are skipped before the program, with their options that take an argument
/// and the number of operand words skipped after the options.
/// O§7.2 lists the first eight. `exec` and `nohup` are skipped too, for file printers only.
const WRAPPERS: &[(&str, &[&str], usize)] = &[
    ("env", &["-u", "-C", "--unset", "--chdir"], 0),
    ("time", &["-f", "-o", "--format", "--output"], 0),
    (
        "sudo",
        &[
            "-u",
            "-g",
            "-h",
            "-p",
            "-C",
            "-D",
            "-r",
            "-t",
            "-T",
            "-U",
            "--user",
            "--group",
            "--host",
            "--prompt",
            "--chdir",
            "--role",
            "--type",
            "--other-user",
            "--close-from",
        ],
        0,
    ),
    ("nice", &["-n", "--adjustment"], 0),
    ("command", &[], 0),
    ("timeout", &["-k", "-s", "--kill-after", "--signal"], 1),
    (
        "stdbuf",
        &["-i", "-o", "-e", "--input", "--output", "--error"],
        0,
    ),
    ("builtin", &[], 0),
    ("exec", &["-a"], 0),
    ("nohup", &[], 0),
];

/// The wrappers that O§7.2 lists; a stage behind any other skipped word gives no name or runner.
const LISTED_WRAPPERS: &[&str] = &[
    "env", "time", "sudo", "nice", "command", "timeout", "stdbuf", "builtin",
];

/// Reserved words that are skipped at the start of a stage, for file printers only.
const RESERVED: &[&str] = &[
    "!", "{", "}", "if", "then", "else", "elif", "fi", "do", "done", "while", "until",
];

/// `xargs` options that take the next word as their argument.
const XARGS_TAKES: &[&str] = &[
    "-I",
    "-n",
    "-P",
    "-L",
    "-d",
    "-a",
    "-E",
    "-s",
    "-J",
    "-R",
    "-S",
    "--max-args",
    "--max-lines",
    "--max-procs",
    "--delimiter",
    "--arg-file",
    "--eof",
    "--process-slot-var",
];

// Subcommand lists (O§7.2, O§15 item 1).
const GIT: &[&str] = &[
    "add",
    "am",
    "apply",
    "archive",
    "bisect",
    "blame",
    "branch",
    "bundle",
    "cat-file",
    "checkout",
    "cherry-pick",
    "clean",
    "clone",
    "commit",
    "config",
    "describe",
    "diff",
    "fetch",
    "for-each-ref",
    "format-patch",
    "gc",
    "grep",
    "init",
    "log",
    "ls-files",
    "ls-remote",
    "ls-tree",
    "merge",
    "merge-base",
    "mv",
    "notes",
    "pull",
    "push",
    "range-diff",
    "rebase",
    "reflog",
    "remote",
    "reset",
    "restore",
    "rev-list",
    "rev-parse",
    "revert",
    "rm",
    "shortlog",
    "show",
    "show-ref",
    "stash",
    "status",
    "submodule",
    "switch",
    "tag",
    "worktree",
];
const CARGO: &[&str] = &[
    "add",
    "bench",
    "build",
    "check",
    "clean",
    "clippy",
    "doc",
    "fetch",
    "fix",
    "fmt",
    "generate-lockfile",
    "init",
    "install",
    "locate-project",
    "login",
    "metadata",
    "new",
    "nextest",
    "owner",
    "package",
    "pkgid",
    "publish",
    "remove",
    "report",
    "run",
    "rustc",
    "rustdoc",
    "search",
    "test",
    "tree",
    "uninstall",
    "update",
    "vendor",
    "verify-project",
    "version",
    "yank",
];
const GO: &[&str] = &[
    "bug", "build", "clean", "doc", "env", "fix", "fmt", "generate", "get", "install", "list",
    "mod", "run", "test", "tool", "version", "vet", "work",
];
const NPM: &[&str] = &[
    "access",
    "audit",
    "ci",
    "config",
    "dedupe",
    "exec",
    "explain",
    "fund",
    "init",
    "install",
    "link",
    "ls",
    "outdated",
    "pack",
    "publish",
    "rebuild",
    "run",
    "run-script",
    "start",
    "stop",
    "test",
    "uninstall",
    "update",
    "version",
    "view",
    "why",
];
const PNPM: &[&str] = &[
    "add", "audit", "dlx", "exec", "install", "list", "outdated", "publish", "remove", "run",
    "start", "store", "test", "update", "why",
];
const YARN: &[&str] = &[
    "add",
    "audit",
    "dlx",
    "exec",
    "info",
    "init",
    "install",
    "link",
    "outdated",
    "pack",
    "publish",
    "remove",
    "run",
    "start",
    "test",
    "up",
    "upgrade",
    "why",
    "workspace",
    "workspaces",
];
const DOCKER: &[&str] = &[
    "attach", "build", "buildx", "compose", "cp", "create", "exec", "images", "info", "inspect",
    "kill", "load", "login", "logout", "logs", "network", "ps", "pull", "push", "restart", "rm",
    "rmi", "run", "save", "start", "stats", "stop", "system", "tag", "top", "version", "volume",
];
const KUBECTL: &[&str] = &[
    "annotate",
    "apply",
    "auth",
    "autoscale",
    "config",
    "cp",
    "create",
    "delete",
    "describe",
    "diff",
    "edit",
    "events",
    "exec",
    "explain",
    "expose",
    "get",
    "label",
    "logs",
    "patch",
    "port-forward",
    "proxy",
    "rollout",
    "run",
    "scale",
    "set",
    "top",
    "version",
    "wait",
];
const GH: &[&str] = &[
    "api",
    "auth",
    "browse",
    "cache",
    "codespace",
    "extension",
    "gist",
    "issue",
    "label",
    "pr",
    "release",
    "repo",
    "run",
    "search",
    "secret",
    "ssh-key",
    "status",
    "variable",
    "workflow",
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
    let mut acc = Acc {
        unsure: cmd.len() > MAX_COMMAND_BYTES,
        ..Acc::default()
    };
    let script = lex(&cmd[..cut], 0, &mut acc.unsure);
    visit(&script, 0, Mode::Top, &mut acc);
    Parsed {
        file_printer: acc.file_printer || acc.unsure,
        name: acc
            .runner_name
            .or(acc.list_name)
            .unwrap_or_else(|| "other".into()),
        runners: acc
            .runners
            .into_iter()
            .filter(|&r| !(acc.live_pytest && r == Runner::Pytest))
            .collect(),
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
    /// Here-string (`<<<`) texts.
    here: Vec<String>,
    /// The stage has an input redirect (`<f`, `<>f`); with no words, zsh prints the file
    /// (NULLCMD, O§7.1).
    input: bool,
}

#[derive(Default)]
struct Script {
    /// Each segment is a list of pipeline stages.
    segments: Vec<Vec<Stage>>,
    /// Here-doc bodies, lexed as commands.
    bodies: Vec<Script>,
    /// The script is a `$( … )` or backtick that starts with one `<` (`$(<f)`, O§7.1).
    reads: bool,
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
    /// A here-string text.
    HereString,
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
            Redir::HereString => self.stage.here.push(w.text),
        }
    }

    fn end_stage(&mut self) {
        self.end_word();
        let st = take(&mut self.stage);
        if !st.words.is_empty() || !st.nested.is_empty() || !st.here.is_empty() || st.input {
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
    let mut lx = Lexer {
        c: s.chars().collect(),
        i: 0,
        unsure: false,
    };
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
                        // The pipeline goes on over blanks, comments and newlines.
                        loop {
                            match self.peek(0) {
                                Some(' ' | '\t' | '\r') => self.i += 1,
                                Some('\n') => {
                                    self.i += 1;
                                    self.bodies(&mut b, depth);
                                }
                                Some('#') => {
                                    while self.peek(0).is_some_and(|ch| ch != '\n') {
                                        self.i += 1;
                                    }
                                }
                                _ => break,
                            }
                        }
                    }
                }
                ')' => {
                    self.i += 1;
                    if close == Some(')') {
                        return b.finish();
                    }
                    self.unsure = true;
                }
                '(' if self.peek(1) == Some(')') => {
                    // A function header `name()`: the name is not argv, and the body that
                    // follows is lexed as segments.
                    if b.word.take().is_none() {
                        b.stage.words.pop();
                    }
                    self.i += 2;
                    b.end_segment();
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
                let reads = reads(&self.c[self.i + 2..]);
                self.i += 2;
                let mut s = self.script(depth + 1, Some(')'));
                s.reads = reads;
                b.stage.nested.push(s);
                self.raw(b, start);
            }
            Some('{') => self.braces(b, depth, in_dq),
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
                while self
                    .peek(0)
                    .is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_')
                {
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

    /// `${…}`, read to its `}`. The `$( )`, backticks and quotes inside are looked into, one
    /// level deeper, so a nest of `${` stays bounded.
    fn braces(&mut self, b: &mut Builder, depth: usize, in_dq: bool) {
        if depth + 1 > MAX_DEPTH {
            self.unsure = true;
            return;
        }
        let start = self.i;
        let keep = b.word().text.len();
        self.i += 2;
        while !self.unsure {
            match self.peek(0) {
                None => {
                    self.unsure = true;
                    return;
                }
                Some('}') => {
                    self.i += 1;
                    break;
                }
                Some('\\') => self.i = (self.i + 2).min(self.c.len()),
                Some('$') => self.dollar(b, depth + 1, in_dq),
                Some('`') => self.backtick(b, depth + 1),
                Some('"') => self.dquote(b, depth + 1),
                Some('\'') if !in_dq => {
                    match self.c[self.i + 1..].iter().position(|&ch| ch == '\'') {
                        Some(n) => self.i += n + 2,
                        None => self.unsure = true,
                    }
                }
                Some(_) => self.i += 1,
            }
        }
        b.word().text.truncate(keep);
        self.raw(b, start);
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
        let mut s = lex(&inner, depth + 1, &mut self.unsure);
        s.reads = reads(&inner.chars().collect::<Vec<_>>());
        b.stage.nested.push(s);
        self.raw(b, start);
    }

    /// Lexes a redirection operator; its target word is not argv. Digits just before it
    /// are an IO number. `<( )` and `>( )` are looked into.
    fn redirect(&mut self, b: &mut Builder, depth: usize) {
        let c = self.c[self.i];
        if b.word.as_ref().is_some_and(|w| {
            !w.dynamic && !w.text.is_empty() && w.text.bytes().all(|x| x.is_ascii_digit())
        }) {
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
        b.redir = Redir::Target;
        if c == '<' {
            match self.peek(0) {
                Some('<') => {
                    self.i += 1;
                    b.redir = match self.peek(0) {
                        Some('<') => {
                            self.i += 1;
                            Redir::HereString
                        }
                        Some('-') => {
                            self.i += 1;
                            Redir::Heredoc(true)
                        }
                        _ => Redir::Heredoc(false),
                    };
                }
                Some('&') => self.i += 1,
                Some('>') => {
                    self.i += 1;
                    b.stage.input = true;
                }
                _ => b.stage.input = true,
            }
        } else if matches!(self.peek(0), Some('>' | '&' | '|')) {
            self.i += 1;
        }
    }

    /// After a newline, reads the bodies of the pending here-docs and lexes them as commands.
    fn bodies(&mut self, b: &mut Builder, depth: usize) {
        for (delim, strip) in take(&mut b.heredocs) {
            let mut body = String::new();
            while self.i < self.c.len() {
                let end = self.c[self.i..]
                    .iter()
                    .position(|&ch| ch == '\n')
                    .map_or(self.c.len(), |n| self.i + n);
                let line: String = self.c[self.i..end].iter().collect();
                self.i = (end + 1).min(self.c.len());
                let check = if strip {
                    line.trim_start_matches('\t')
                } else {
                    line.as_str()
                };
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

/// The text of a `$( … )` or backtick starts, after blanks and newlines, with one `<`,
/// optionally after `0` (not `<<` or `<(`): it prints the file (O§7.1).
fn reads(c: &[char]) -> bool {
    let mut k = c
        .iter()
        .position(|ch| !matches!(ch, ' ' | '\t' | '\n'))
        .unwrap_or(c.len());
    if c.get(k) == Some(&'0') {
        k += 1;
    }
    c.get(k) == Some(&'<') && !matches!(c.get(k + 1), Some('<' | '('))
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
    cs.next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && cs.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// A stage's words after the leading `NAME=value` words, reserved words and wrappers.
struct Argv<'a> {
    /// The program and its args.
    args: &'a [Word],
    /// The command of `env -S`, with the later words appended; it is parsed in turn.
    split: Option<String>,
    /// Only `NAME=value` words and O§7.2 wrappers were skipped. Else the stage gives no name
    /// and no runner.
    listed: bool,
}

/// One wrapper option.
enum Opt {
    Flag,
    /// The next word is the option's argument.
    Arg,
    /// `env -S`: the string, or `None` when it is the next word.
    Split(Option<String>),
}

fn opt(env: bool, takes: &[&str], o: &str) -> Opt {
    if o.starts_with("--") {
        if env && o == "--split-string" {
            return Opt::Split(None);
        }
        if let Some(s) = o.strip_prefix("--split-string=").filter(|_| env) {
            return Opt::Split(Some(s.to_string()));
        }
        return if takes.contains(&o) {
            Opt::Arg
        } else {
            Opt::Flag
        };
    }
    // A short cluster: the first letter that takes an argument takes the rest of the word,
    // or the next word when it is the last letter.
    for (k, ch) in o.char_indices().skip(1) {
        let rest = &o[k + ch.len_utf8()..];
        if env && ch == 'S' {
            return Opt::Split((!rest.is_empty()).then(|| rest.to_string()));
        }
        if takes.iter().any(|t| t.len() == 2 && t.ends_with(ch)) {
            return if rest.is_empty() { Opt::Arg } else { Opt::Flag };
        }
    }
    Opt::Flag
}

/// The `env -S` command: the string, then each later word, as env appends them.
fn split_command(mut text: String, later: &[Word]) -> String {
    for w in later {
        text.push(' ');
        if w.dynamic {
            text.push_str(&w.text);
        } else {
            text.push('\'');
            text.push_str(&w.text.replace('\'', "'\\''"));
            text.push('\'');
        }
    }
    text
}

fn argv(words: &[Word]) -> Argv<'_> {
    let mut i = 0;
    let mut listed = true;
    while let Some(w) = words.get(i) {
        let t = w.text.as_str();
        if is_assign(t) {
            i += 1;
            continue;
        }
        if RESERVED.contains(&t) || t == "function" {
            listed = false;
            i += if t == "function" { 2 } else { 1 };
            continue;
        }
        let Some(&(wrapper, takes, operands)) = WRAPPERS.iter().find(|(n, _, _)| *n == base(t))
        else {
            break;
        };
        listed &= LISTED_WRAPPERS.contains(&wrapper);
        i += 1;
        while let Some(o) = words.get(i).map(|w| w.text.as_str()) {
            if !o.starts_with('-') {
                break;
            }
            i += 1;
            if o == "--" {
                break;
            }
            let (text, later) = match opt(wrapper == "env", takes, o) {
                Opt::Flag => continue,
                Opt::Arg => {
                    i += 1;
                    continue;
                }
                Opt::Split(Some(s)) => (s, &words[i..]),
                Opt::Split(None) => match words.get(i) {
                    Some(w) => (w.text.clone(), &words[i + 1..]),
                    None => {
                        return Argv {
                            args: &[],
                            split: None,
                            listed,
                        };
                    }
                },
            };
            return Argv {
                args: &[],
                split: Some(split_command(text, later)),
                listed,
            };
        }
        i += operands;
    }
    Argv {
        args: &words[i.min(words.len())..],
        split: None,
        listed,
    }
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
        "npm" | "pnpm" | "yarn" if subs_of(name).and_then(|l| sub(rest, l)) == Some("test") => {
            ALL_RUNNERS
        }
        _ => &[],
    }
}

/// Whether the words of a pytest stage ask for live test output: `-s` (also in a short-option cluster such as
/// `-sv`, before an option that takes a value), `--capture=no`, `--capture=tee-sys`, `--capture no`,
/// `--capture tee-sys`, `-p no:capture` (also `-pno:capture`, `-vp no:capture`), or a dynamic word that may hold
/// one of them (O§7.6, O-C9).
fn pytest_live(rest: &[Word]) -> bool {
    rest.iter().enumerate().any(|(i, w)| {
        let t = w.text.as_str();
        w.dynamic
            || match t.strip_prefix("--capture") {
                Some(v) if v.is_empty() => rest
                    .get(i + 1)
                    .is_some_and(|n| matches!(n.text.as_str(), "no" | "tee-sys")),
                Some(v) => matches!(v, "=no" | "=tee-sys"),
                None => t.strip_prefix('-').filter(|c| !c.starts_with('-')).is_some_and(|c| {
                    let lead = c.find(|ch| "kmrpcoWn".contains(ch)).unwrap_or(c.len());
                    c[..lead].contains('s')
                        || c[lead..].strip_prefix('p').is_some_and(|v| {
                            let v = if v.is_empty() { rest.get(i + 1).map_or("", |n| n.text.as_str()) } else { v };
                            v == "no:capture"
                        })
                }),
            }
    })
}

/// A word that sets `PYTEST_ADDOPTS` (`PYTEST_ADDOPTS=…` or `PYTEST_ADDOPTS+=…`): pytest reads live-output
/// flags from it, and its value is not parsed (O§7.6, O-C9).
fn sets_pytest_addopts(words: &[Word]) -> bool {
    words.iter().any(|w| {
        w.text.strip_prefix("PYTEST_ADDOPTS").is_some_and(|r| r.starts_with('=') || r.starts_with("+="))
    })
}

#[derive(Default)]
struct Acc {
    unsure: bool,
    file_printer: bool,
    reuse: bool,
    runners: Vec<Runner>,
    runner_name: Option<String>,
    list_name: Option<String>,
    /// The pytest live-output setting is on or cannot be seen (a live-output flag, `-p no:capture`, a
    /// `PYTEST_ADDOPTS` word, a wrapper), so the pytest view does not apply (O§7.6, O-C9).
    live_pytest: bool,
}

/// Where a script or stage is met.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// The top-level script: name, runners, reuse and file printers.
    Top,
    /// Looked into (O§7.1): reuse and file printers.
    Nested,
    /// Here-doc bodies and here-strings fed to a shell: file printers only.
    PrinterOnly,
}

impl Mode {
    fn child(self) -> Mode {
        if self == Mode::Top {
            Mode::Nested
        } else {
            self
        }
    }
}

fn visit(s: &Script, depth: usize, mode: Mode, acc: &mut Acc) {
    acc.file_printer |= s.reads;
    for seg in &s.segments {
        for (k, st) in seg.iter().enumerate() {
            acc.file_printer |= st.input && st.words.is_empty();
            stage(&st.words, &st.here, depth, mode, k, acc);
            for n in &st.nested {
                visit(n, depth + 1, mode.child(), acc);
            }
        }
    }
    for body in &s.bodies {
        visit(body, depth + 1, Mode::PrinterOnly, acc);
    }
}

/// Lexes and visits a string that a shell runs (`-c`, `env -S`, a here-string) one level deeper.
fn nest(text: &str, depth: usize, mode: Mode, acc: &mut Acc) {
    let s = lex(text, depth + 1, &mut acc.unsure);
    visit(&s, depth + 1, mode.child(), acc);
}

/// Visits command words that another program runs (`xargs`, `find -exec`) one level deeper.
fn inner(words: &[Word], depth: usize, mode: Mode, acc: &mut Acc) {
    if depth + 1 > MAX_DEPTH {
        acc.unsure = true;
        return;
    }
    stage(words, &[], depth + 1, mode.child(), 0, acc);
}

fn stage(words: &[Word], here: &[String], depth: usize, mode: Mode, pos: usize, acc: &mut Acc) {
    acc.live_pytest |= sets_pytest_addopts(words);
    let a = argv(words);
    if let Some(s) = &a.split {
        nest(s, depth, mode, acc);
    }
    let Some((prog, rest)) = a.args.split_first() else {
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
    let mut reuse = false;
    match name {
        "git" => match sub(rest, GIT) {
            Some("show") => {
                acc.file_printer = true;
                reuse = true;
            }
            Some("cat-file" | "grep" | "blame") => acc.file_printer = true,
            Some("diff") => reuse = true,
            Some("log") if has(&["-p", "--patch"]) => reuse = true,
            Some("format-patch") if has(&["--stdout"]) => reuse = true,
            _ => {}
        },
        "diff" => reuse = true,
        "bash" | "sh" | "zsh" => {
            // `-c` is looked for among the leading options only: to `--` or the first word
            // that is not an option. `-o`, `+o`, `-O`, `+O` take the next word.
            let mut lead = 0;
            while let Some(t) = rest.get(lead).map(|w| w.text.as_str()) {
                if t == "--" || !(t.starts_with('-') || t.starts_with('+')) {
                    break;
                }
                lead += if matches!(t, "-o" | "+o" | "-O" | "+O") {
                    2
                } else {
                    1
                };
            }
            let lead = lead.min(rest.len());
            let flag = rest[..lead].iter().position(|w| {
                let t = &w.text;
                t.starts_with('-') && !t.starts_with("--") && t.contains('c')
            });
            if let Some(w) =
                flag.and_then(|k| rest[k + 1..].iter().find(|w| !w.text.starts_with('-')))
            {
                nest(&w.text, depth, mode, acc);
            }
            for h in here {
                nest(h, depth, Mode::PrinterOnly, acc);
            }
            if flag.is_none() && pos > 0 {
                acc.unsure = true;
            }
        }
        "eval" => {
            let words = match rest.first() {
                Some(w) if w.text == "--" => &rest[1..],
                _ => rest,
            };
            let text = words
                .iter()
                .map(|w| w.text.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            nest(&text, depth, mode, acc);
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
            inner(&rest[i.min(rest.len())..], depth, mode, acc);
        }
        "find" => {
            let mut i = 0;
            while i < rest.len() {
                if matches!(
                    rest[i].text.as_str(),
                    "-exec" | "-execdir" | "-ok" | "-okdir"
                ) {
                    let from = i + 1;
                    let end = rest[from..]
                        .iter()
                        .position(|w| w.text == ";" || w.text == "+")
                        .map_or(rest.len(), |n| from + n);
                    inner(&rest[from..end], depth, mode, acc);
                    i = end;
                }
                i += 1;
            }
        }
        _ => {}
    }
    if mode != Mode::PrinterOnly {
        acc.reuse |= reuse;
    }
    if mode != Mode::Top || !a.listed {
        return;
    }
    let runners = runners_of(name, rest);
    if runners == ALL_RUNNERS || (runners == [Runner::Pytest] && pytest_live(rest)) {
        acc.live_pytest = true;
    }
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
    if pos == 0 && acc.list_name.is_none() && NAMES.contains(&name) {
        acc.list_name = Some(stored(name, rest));
    }
}
