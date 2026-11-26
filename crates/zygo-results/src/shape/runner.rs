//! Step 3 of the pipeline, the runner views (O§7.6).

use std::sync::LazyLock;

use regex::Regex;

use super::command::Runner;
use super::{Shaper, Step};

/// The libtest "Summary that confirms": a line `^test result: (ok|FAILED)\. \d+ passed; \d+ failed;` (O§7.6).
/// `(?m)` lets it test a whole text and one line.
static SUMMARY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^test result: (ok|FAILED)\. \d+ passed; \d+ failed;").unwrap());

/// The libtest hidden test forms `^test \S+ \.\.\. (ok|ignored)$` and `^test \S+ \.\.\. ignored, .*$` (O§7.6).
/// Group 1 is `ok` for a passed test, else the test was ignored.
static TEST_HIDDEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^test \S+ \.\.\. (ok|ignored|ignored, .*)$").unwrap());

/// The cargo progress form
/// `^\s+(Compiling|Checking|Fresh|Downloaded|Downloading|Locking|Updating|Blocking) \S.*$` (O§7.6),
/// with `\s` as ASCII space or tab.
static PROGRESS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[ \t]+(Compiling|Checking|Fresh|Downloaded|Downloading|Locking|Updating|Blocking) \S.*$").unwrap()
});

/// A cargo test binary line `^\s+(Running|Doc-tests) ` (O§7.6): it ends the test lines of the binary before it.
static BINARY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[ \t]+(Running|Doc-tests) ").unwrap());

/// The dedup counted form `<line> [repeated <n> times]` (O§7.5).
static COUNTED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r" \[repeated [0-9]+ times\]$").unwrap());

/// The pytest "Summary that confirms" `^=+ .*\b(passed|failed|error|errors|skipped)\b.* in [\d.]+s.* =+$` (O§7.6),
/// tested on the last line of the text.
static PY_SUMMARY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^=+ .*\b(passed|failed|error|errors|skipped)\b.* in [\d.]+s.* =+$").unwrap()
});

/// The pytest progress form (O§7.6): a file and only `.`, `s`, `x`, `X` (no `F`, no `E`), with an optional
/// percent tail; `\s` as ASCII space or tab, `\d` as ASCII digit.
static PY_PROGRESS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\S+\.py [.sxX]+[ \t]*(\[[ \t]*[0-9]+%\])?$").unwrap());

/// The pytest `-v` form (O§7.6): a passed, skipped, xfailed or xpassed test. The word is followed by an ASCII
/// space, a tab or the line end, so `XPASS(strict)` (a failed test) does not match.
static PY_VERBOSE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\S+::\S+ (PASSED|SKIPPED|XFAIL|XPASS)([ \t].*)?$").unwrap());

/// The header that opens a pytest failure section, or the `-rP` `PASSES` section (O§7.6).
static PY_FAILURE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^=+ (FAILURES|ERRORS|PASSES|short test summary info) =+$").unwrap());

/// The last line of `text`, without one final `\n` (O§7.6 "a last line"). A text that ends in a blank line
/// has an empty last line.
fn last_line(text: &str) -> &str {
    let t = text.strip_suffix('\n').unwrap_or(text);
    t.rsplit('\n').next().unwrap_or("")
}

/// Whether `line` matches a whole-line hidden form of the row of `r` (O§7.6). `line` is one line of the text
/// that step 3 sees (after clean, O§7.4, and dedup, O§7.5), without its `\n`. A line that ends in `\r` or
/// in the dedup counted form matches no form.
pub fn hidden_by(r: Runner, line: &str) -> bool {
    if line.ends_with('\r') || COUNTED.is_match(line) {
        return false;
    }
    match r {
        Runner::Libtest => TEST_HIDDEN.is_match(line) || PROGRESS.is_match(line),
        Runner::Pytest => PY_PROGRESS.is_match(line) || PY_VERBOSE.is_match(line),
        Runner::Gotest => false,
    }
}

/// Whether `text` has the summary that confirms the runner `r` (O§7.6).
fn has_summary(r: Runner, text: &str) -> bool {
    match r {
        Runner::Libtest => SUMMARY.is_match(text),
        Runner::Pytest => PY_SUMMARY.is_match(last_line(text)),
        Runner::Gotest => false,
    }
}

/// The runner view of `text` (O§7.6): the first runner in `runners` whose summary is in `text` decides;
/// its view (or its "no change") is the result. `None` when no runner has its summary in `text`.
pub fn view(text: &str, runners: &[Runner]) -> Option<(String, Vec<Step>, Shaper)> {
    let r = runners.iter().copied().find(|&r| has_summary(r, text))?;
    match r {
        Runner::Libtest => libtest(text).map(|(v, s)| (v, s, Shaper::Libtest)),
        Runner::Pytest => pytest(text).map(|(v, s)| (v, s, Shaper::Pytest)),
        Runner::Gotest => None,
    }
}

enum Piece<'a> {
    Line(&'a str),
    /// The count line of one test binary: index into the groups.
    Tests(usize),
    Build,
}

/// The libtest view of `text` (O§7.6): hides the passed and ignored test lines and the cargo progress lines,
/// keeps every other line verbatim and in order, and writes `[zygo: <p> passed and <i> ignored test lines
/// hidden]` at the place of the first hidden test line of each test binary and `[zygo: <n> build progress
/// lines hidden]` at the place of the first hidden progress line. A test binary's hidden test lines end at its
/// summary line or at the next binary line. No line of a failure section (from a `---- ` header to the next
/// `failures:` line) is hidden (O-C9). `None` when the text has no summary,
/// nothing is hidden, or the view is not shorter than `text` (O§7.3 "No step adds content").
pub fn libtest(text: &str) -> Option<(String, Vec<Step>)> {
    if !SUMMARY.is_match(text) {
        return None;
    }
    let mut pieces = Vec::new();
    let mut groups: Vec<(usize, usize)> = Vec::new();
    let mut open = false;
    let mut build = 0usize;
    let mut failure = false;
    for line in text.split_inclusive('\n') {
        let bare = line.strip_suffix('\n').unwrap_or(line);
        if bare.starts_with("---- ") {
            failure = true;
        } else if bare == "failures:" {
            failure = false;
        }
        if failure || !hidden_by(Runner::Libtest, bare) {
            pieces.push(Piece::Line(line));
            if SUMMARY.is_match(bare) || BINARY.is_match(bare) {
                open = false;
            }
        } else if let Some(c) = TEST_HIDDEN.captures(bare) {
            if !open {
                groups.push((0, 0));
                pieces.push(Piece::Tests(groups.len() - 1));
                open = true;
            }
            let g = groups.last_mut().expect("a group is open");
            if &c[1] == "ok" {
                g.0 += 1;
            } else {
                g.1 += 1;
            }
        } else if PROGRESS.is_match(bare) {
            if build == 0 {
                pieces.push(Piece::Build);
            }
            build += 1;
        }
    }
    if groups.is_empty() && build == 0 {
        return None;
    }
    let mut out = String::with_capacity(text.len());
    for p in pieces {
        match p {
            Piece::Line(l) => out.push_str(l),
            Piece::Tests(i) => {
                let (p, ig) = groups[i];
                out.push_str(&format!("[zygo: {p} passed and {ig} ignored test lines hidden]\n"));
            }
            Piece::Build => out.push_str(&format!("[zygo: {build} build progress lines hidden]\n")),
        }
    }
    if out.len() >= text.len() {
        return None;
    }
    let mut steps = Vec::new();
    if !groups.is_empty() {
        steps.push(Step::PassedTestsHidden);
    }
    if build > 0 {
        steps.push(Step::BuildProgressHidden);
    }
    Some((out, steps))
}

/// The pytest view of `text` (O§7.6): hides the progress lines with no `F` and no `E` and the `-v` lines of
/// passed, skipped, xfailed and xpassed tests, keeps every other line verbatim and in order, and writes
/// `[zygo: <n> passed or skipped progress lines hidden]` at the place of the first hidden line. The failure
/// section is sticky: from the first `FAILURES`, `ERRORS`, `PASSES` or `short test summary info` header to the end of
/// the text no line is hidden (O-C9). `None` when the last line is not the summary, nothing is hidden, or the
/// view is not shorter than `text` (O§7.3 "No step adds content").
pub fn pytest(text: &str) -> Option<(String, Vec<Step>)> {
    if !has_summary(Runner::Pytest, text) {
        return None;
    }
    let mut pieces: Vec<Option<&str>> = Vec::new();
    let mut hidden = 0usize;
    let mut failure = false;
    for line in text.split_inclusive('\n') {
        let bare = line.strip_suffix('\n').unwrap_or(line);
        if PY_FAILURE.is_match(bare) {
            failure = true;
        }
        if failure || !hidden_by(Runner::Pytest, bare) {
            pieces.push(Some(line));
        } else {
            if hidden == 0 {
                pieces.push(None);
            }
            hidden += 1;
        }
    }
    if hidden == 0 {
        return None;
    }
    let mut out = String::with_capacity(text.len());
    for p in pieces {
        match p {
            Some(l) => out.push_str(l),
            None => out.push_str(&format!("[zygo: {hidden} passed or skipped progress lines hidden]\n")),
        }
    }
    if out.len() >= text.len() {
        return None;
    }
    Some((out, vec![Step::PassedTestsHidden]))
}
