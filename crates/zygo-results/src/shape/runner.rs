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
/// `^\s+(Compiling|Checking|Fresh|Downloaded|Downloading|Locking|Updating|Blocking) \S.*$` (O§7.6).
static PROGRESS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s+(Compiling|Checking|Fresh|Downloaded|Downloading|Locking|Updating|Blocking) \S.*$").unwrap()
});

/// Whether `line` (one line without its `\n`) matches a whole-line hidden form of the row of `r` (O§7.6).
pub fn hidden_by(r: Runner, line: &str) -> bool {
    match r {
        Runner::Libtest => TEST_HIDDEN.is_match(line) || PROGRESS.is_match(line),
        Runner::Pytest | Runner::Gotest => false,
    }
}

/// Whether `text` has the summary that confirms the runner `r` (O§7.6).
fn has_summary(r: Runner, text: &str) -> bool {
    match r {
        Runner::Libtest => SUMMARY.is_match(text),
        Runner::Pytest | Runner::Gotest => false,
    }
}

/// The runner view of `text` (O§7.6): the first runner in `runners` whose summary is in `text` decides;
/// its view (or its "no change") is the result. `None` when no runner has its summary in `text`.
pub fn view(text: &str, runners: &[Runner]) -> Option<(String, Vec<Step>, Shaper)> {
    let r = runners.iter().copied().find(|&r| has_summary(r, text))?;
    match r {
        Runner::Libtest => libtest(text).map(|(v, s)| (v, s, Shaper::Libtest)),
        Runner::Pytest | Runner::Gotest => None,
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
/// lines hidden]` at the place of the first hidden progress line. `None` when the text has no summary,
/// nothing is hidden, or the view is not shorter than `text` (O§7.3 "No step adds content").
pub fn libtest(text: &str) -> Option<(String, Vec<Step>)> {
    if !SUMMARY.is_match(text) {
        return None;
    }
    let mut pieces = Vec::new();
    let mut groups: Vec<(usize, usize)> = Vec::new();
    let mut open = false;
    let mut build = 0usize;
    for line in text.split_inclusive('\n') {
        let bare = line.strip_suffix('\n').unwrap_or(line);
        if let Some(c) = TEST_HIDDEN.captures(bare) {
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
        } else {
            pieces.push(Piece::Line(line));
            if SUMMARY.is_match(bare) {
                open = false;
            }
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
