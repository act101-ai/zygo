//! Step 3 of the pipeline, the runner views (O§7.6).

use std::collections::HashMap;
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

/// The libtest line of a failed test: part of its failure section (O§7.6).
static TEST_FAILED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^test \S+ \.\.\. FAILED$").unwrap());

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

/// The go test "Summary that confirms" `^(ok|FAIL|\?)\s+\S` (O§7.6), with `\s` as ASCII space or tab.
/// `(?m)` lets it test a whole text and one line.
static GO_SUMMARY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^(ok|FAIL|\?)[ \t]+\S").unwrap());

/// The go test hidden form `^\?\s+\S+\s+\[no test files\]$` (O§7.6).
static GO_NO_TESTS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\?[ \t]+\S+[ \t]+\[no test files\]$").unwrap());

/// A go test `=== RUN|PAUSE|CONT|NAME <name>` line (O§7.6 go test rule 1); group 2 is the test name.
static GO_MARK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^=== (RUN|PAUSE|CONT|NAME)[ \t]+(\S+)$").unwrap());

/// A go test result line `--- PASS|FAIL|SKIP: <name> (<time>)` at any indent (O§7.6 go test rule 2); group 1 is
/// the result, group 2 the test name.
static GO_RESULT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[ \t]*--- (PASS|FAIL|SKIP): (\S+) \(.*\)$").unwrap());

/// A go test package line (O§7.6 go test rule 4): `ok`, `FAIL`, `PASS`, `panic:` and `exit status` lines.
static GO_PACKAGE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^((ok|FAIL|PASS)([ \t].*)?|panic:.*|exit status .*)$").unwrap());

/// `TEST_FAILED` on a whole text: whether one of its lines is a libtest failed test line.
static TEST_FAILED_ANY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^test \S+ \.\.\. FAILED$").unwrap());

/// `PY_FAILURE` on a whole text: whether one of its lines opens a pytest failure section.
static PY_FAILURE_ANY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^=+ (FAILURES|ERRORS|PASSES|short test summary info) =+$").unwrap());

/// `GO_MARK` or `GO_RESULT` on a whole text: whether one of its lines can give a go test line an owner.
static GO_OWNER_ANY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^=== (RUN|PAUSE|CONT|NAME)[ \t]+\S+$|^[ \t]*--- (PASS|FAIL|SKIP): \S+ \(.*\)$").unwrap()
});

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
        Runner::Gotest => GO_NO_TESTS.is_match(line),
    }
}

/// Whether `text` has the summary that confirms the runner `r` (O§7.6).
fn has_summary(r: Runner, text: &str) -> bool {
    match r {
        Runner::Libtest => SUMMARY.is_match(text),
        Runner::Pytest => PY_SUMMARY.is_match(last_line(text)),
        Runner::Gotest => GO_SUMMARY.is_match(text),
    }
}

/// One flag per `split_inclusive('\n')` line of `text`: whether the line is inside a failure section of the
/// runner `r` (O§7.6). Libtest: a `test <name> ... FAILED` line, the block from a `---- ` line to the next
/// `failures:` line, and the `failures:` name list (a `failures:` line and the `    <name>` lines after it).
/// Pytest: from the first failure header to the end. Go test: a line of a failing test or of a test with no
/// result line.
fn section(r: Runner, text: &str) -> Vec<bool> {
    match r {
        Runner::Libtest => {
            let (mut block, mut list) = (false, false);
            text.split_inclusive('\n')
                .map(|line| {
                    let bare = line.strip_suffix('\n').unwrap_or(line);
                    let name = bare
                        .strip_prefix("    ")
                        .is_some_and(|n| !n.is_empty() && !n.contains(char::is_whitespace));
                    list = bare == "failures:" || (list && name);
                    if bare.starts_with("---- ") {
                        block = true;
                    } else if bare == "failures:" {
                        block = false;
                    }
                    block || list || TEST_FAILED.is_match(bare)
                })
                .collect()
        }
        Runner::Pytest => {
            let mut on = false;
            text.split_inclusive('\n')
                .map(|line| {
                    on |= PY_FAILURE.is_match(line.strip_suffix('\n').unwrap_or(line));
                    on
                })
                .collect()
        }
        Runner::Gotest => go_lines(text).into_iter().map(|g| g == GoLine::Kept).collect(),
    }
}

/// The runner view of `text` (O§7.6): the first runner in `runners` whose summary is in `text` decides;
/// its view (or its "no change") is the result. No line with `keep` set is hidden (O§7.6, another runner's
/// failure section): `keep` is the failure sections of the other runners of the table. `None` when no runner
/// has its summary in `text`.
pub fn view(text: &str, runners: &[Runner]) -> Option<(String, Vec<Step>, Shaper)> {
    let r = runners.iter().copied().find(|&r| has_summary(r, text))?;
    let keep = other_sections(r, text);
    match r {
        Runner::Libtest => libtest_kept(text, &keep).map(|(v, s)| (v, s, Shaper::Libtest)),
        Runner::Pytest => pytest_kept(text, &keep).map(|(v, s)| (v, s, Shaper::Pytest)),
        Runner::Gotest => gotest_kept(text, &keep).map(|(v, s)| (v, s, Shaper::Gotest)),
    }
}

/// Whether `text` can have a line in a failure section of `r`: false only when `section(r, text)` is all false.
fn may_have_section(r: Runner, text: &str) -> bool {
    match r {
        Runner::Libtest => text.contains("failures:") || text.contains("---- ") || TEST_FAILED_ANY.is_match(text),
        Runner::Pytest => PY_FAILURE_ANY.is_match(text),
        Runner::Gotest => GO_OWNER_ANY.is_match(text),
    }
}

/// One flag per `split_inclusive('\n')` line: the union of the failure sections of the runners other than `r`;
/// empty when `may_have_section` is false for each of them.
fn other_sections(r: Runner, text: &str) -> Vec<bool> {
    let mut keep = Vec::new();
    for o in [Runner::Libtest, Runner::Pytest, Runner::Gotest]
        .into_iter()
        .filter(|&o| o != r && may_have_section(o, text))
    {
        if keep.is_empty() {
            keep = vec![false; text.split_inclusive('\n').count()];
        }
        for (k, s) in keep.iter_mut().zip(section(o, text)) {
            *k |= s;
        }
    }
    keep
}

/// For a line that matches `TEST_HIDDEN`: whether group 1 is `ok`.
fn libtest_ok(bare: &str) -> bool {
    bare.strip_prefix("test ")
        .and_then(|r| r.strip_suffix(" ... ok"))
        .is_some_and(|n| !n.is_empty() && !n.contains(char::is_whitespace))
}

/// `GO_SUMMARY.is_match(bare)`, with the first-byte pre-check.
fn go_summary_line(bare: &str) -> bool {
    matches!(bare.as_bytes().first(), Some(b'o' | b'F' | b'?')) && GO_SUMMARY.is_match(bare)
}

/// `GO_PACKAGE.is_match(bare)`, with the first-byte pre-check.
fn go_package_line(bare: &str) -> bool {
    matches!(bare.as_bytes().first(), Some(b'o' | b'F' | b'P' | b'p' | b'e')) && GO_PACKAGE.is_match(bare)
}

/// `GO_MARK` group 2, or `None` when `GO_MARK` does not match.
fn go_mark(bare: &str) -> Option<&str> {
    if !bare.starts_with("=== ") || !GO_MARK.is_match(bare) {
        return None;
    }
    let r = &bare[4..];
    let kind = r.find([' ', '\t'])?;
    Some(r[kind..].trim_start_matches([' ', '\t']))
}

/// `GO_RESULT` groups 1 and 2, or `None` when `GO_RESULT` does not match.
fn go_result(bare: &str) -> Option<(&str, &str)> {
    let rest = bare.trim_start_matches([' ', '\t']);
    if !rest.starts_with("--- ") || !GO_RESULT.is_match(bare) {
        return None;
    }
    let name = rest[10..].split(' ').next().unwrap_or("");
    Some((&rest[4..8], name))
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
    libtest_kept(text, &[])
}

/// `libtest`, where no line with `keep` set is hidden (O§7.6, another runner's failure section).
fn libtest_kept(text: &str, keep: &[bool]) -> Option<(String, Vec<Step>)> {
    if !SUMMARY.is_match(text) {
        return None;
    }
    let mut pieces = Vec::new();
    let mut groups: Vec<(usize, usize)> = Vec::new();
    let mut open = false;
    let mut build = 0usize;
    let mut failure = false;
    for (i, line) in text.split_inclusive('\n').enumerate() {
        let kept = keep.get(i) == Some(&true);
        let bare = line.strip_suffix('\n').unwrap_or(line);
        if bare.starts_with("---- ") {
            failure = true;
        } else if bare == "failures:" {
            failure = false;
        }
        if failure || kept || !hidden_by(Runner::Libtest, bare) {
            pieces.push(Piece::Line(line));
            if SUMMARY.is_match(bare) || BINARY.is_match(bare) {
                open = false;
            }
        } else if bare.starts_with("test ") {
            if !open {
                groups.push((0, 0));
                pieces.push(Piece::Tests(groups.len() - 1));
                open = true;
            }
            let g = groups.last_mut().expect("a group is open");
            if libtest_ok(bare) {
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
    pytest_kept(text, &[])
}

/// `pytest`, where no line with `keep` set is hidden (O§7.6, another runner's failure section).
fn pytest_kept(text: &str, keep: &[bool]) -> Option<(String, Vec<Step>)> {
    if !has_summary(Runner::Pytest, text) {
        return None;
    }
    let mut pieces: Vec<Option<&str>> = Vec::new();
    let mut hidden = 0usize;
    let mut failure = false;
    for (i, line) in text.split_inclusive('\n').enumerate() {
        let kept = keep.get(i) == Some(&true);
        let bare = line.strip_suffix('\n').unwrap_or(line);
        if PY_FAILURE.is_match(bare) {
            failure = true;
        }
        if failure || kept || !hidden_by(Runner::Pytest, bare) {
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

/// The place of one line of a go test text in its runner view (O§7.6 go test rules 1-4).
#[derive(Clone, Copy, PartialEq)]
enum GoLine {
    /// A line of a test whose results are all `PASS` or `SKIP`: hidden.
    Passed,
    /// A line of a test with a `FAIL` result or with no result line (rule 3): kept.
    Kept,
    /// A line of no test: kept unless it matches the hidden form.
    Free,
}

/// The test key: the package block (counted by its `ok`/`FAIL` summary lines and `[stderr]`) and the test name.
type GoKey<'a> = (usize, &'a str);

/// One `GoLine` for each `split_inclusive('\n')` line of `text` (O§7.6 go test rules 1-4). A line belongs to
/// the test that names it (`=== ` or result line), else to the test whose open result block holds it
/// (rule 2), else to the current test (rule 1). A package summary line and `[stderr]` end the current test, and
/// tests are keyed by package block and name (O-C9, O§15 item 2). The current test also ends at its own result
/// line or that of a test that contains it, at a package line (rule 4) and at a `=== `/`--- ` line that is not
/// parsed, so a panic stack, a race report or text after `PASS` belongs to no test and stays. A line that ends
/// in `\r` or in the dedup counted form is never `Passed`.
fn go_lines(text: &str) -> Vec<GoLine> {
    let mut pkg = 0usize;
    let mut cur: Option<GoKey> = None;
    let mut open: Vec<(GoKey, usize)> = Vec::new();
    let mut owner: Vec<Option<GoKey>> = Vec::new();
    let mut passed: HashMap<GoKey, bool> = HashMap::new();
    for line in text.split_inclusive('\n') {
        let bare = line.strip_suffix('\n').unwrap_or(line);
        let rest = bare.trim_start_matches([' ', '\t']);
        let depth = bare.len() - rest.len();
        let cr = bare.ends_with('\r');
        while open.last().is_some_and(|&(_, d)| d >= depth) {
            open.pop();
        }
        let summary = go_summary_line(bare);
        let o = if bare == "[stderr]" || (summary && !bare.starts_with('?')) {
            pkg += 1;
            cur = None;
            open.clear();
            None
        } else if summary && cur.is_none() {
            None
        } else if go_package_line(bare) {
            cur = None;
            open.clear();
            None
        } else if let Some(name) = go_mark(bare).filter(|_| !cr) {
            cur = Some((pkg, name));
            cur
        } else if let Some((res, name)) = go_result(bare).filter(|_| !cr) {
            let key = (pkg, name);
            let ok = res != "FAIL";
            passed.entry(key).and_modify(|p| *p &= ok).or_insert(ok);
            open.push((key, depth));
            let within = |n: &str| n == key.1 || n.strip_prefix(key.1).is_some_and(|r| r.starts_with('/'));
            if cur.is_some_and(|(p, n)| p == key.0 && within(n)) {
                cur = None;
            }
            Some(key)
        } else if rest.starts_with("=== ") || rest.starts_with("--- ") {
            cur = None;
            None
        } else {
            open.last().map(|&(k, _)| k).or(cur)
        };
        owner.push(o);
    }
    text.split_inclusive('\n')
        .zip(owner)
        .map(|(line, o)| {
            let bare = line.strip_suffix('\n').unwrap_or(line);
            match o {
                None => GoLine::Free,
                Some(k) if passed.get(&k) == Some(&true) && !bare.ends_with('\r') && !COUNTED.is_match(bare) => {
                    GoLine::Passed
                }
                Some(_) => GoLine::Kept,
            }
        })
        .collect()
}

/// The 1-based line numbers of the lines of `text` (split at `\n`) that belong to a test whose results are all
/// `PASS` or `SKIP`, less those that end in `\r` or in the dedup counted form: exactly the lines `gotest` hides
/// as passed-test lines (O§7.6 go test rules 1-3). Used by the O§8.3 item 5 check.
pub fn gotest_passed_lines(text: &str) -> Vec<usize> {
    go_lines(text).iter().enumerate().filter(|(_, g)| **g == GoLine::Passed).map(|(i, _)| i + 1).collect()
}

/// The go test view of `text` (O§7.6): hides the lines of each test whose results are all `PASS` or `SKIP` and
/// the `[no test files]` package lines, keeps every other line verbatim and in order (the lines of a failing
/// test or of a test with no result line, and the package lines), and writes `[zygo: <n> lines of passed tests
/// hidden]` at the place of the first hidden test line and `[zygo: <n> packages with no test files hidden]` at
/// the place of the first hidden `[no test files]` line. `None` when the text has no summary, nothing is hidden,
/// or the view is not shorter than `text` (O§7.3 "No step adds content").
pub fn gotest(text: &str) -> Option<(String, Vec<Step>)> {
    gotest_kept(text, &[])
}

/// `gotest`, where no line with `keep` set is hidden (O§7.6, another runner's failure section).
fn gotest_kept(text: &str, keep: &[bool]) -> Option<(String, Vec<Step>)> {
    if !has_summary(Runner::Gotest, text) {
        return None;
    }
    enum P<'a> {
        Line(&'a str),
        Passed,
        NoTests,
    }
    let mut pieces = Vec::new();
    let (mut tests, mut no_tests) = (0usize, 0usize);
    for (i, (line, g)) in text.split_inclusive('\n').zip(go_lines(text)).enumerate() {
        let kept = keep.get(i) == Some(&true);
        let bare = line.strip_suffix('\n').unwrap_or(line);
        if g == GoLine::Passed && !kept {
            if tests == 0 {
                pieces.push(P::Passed);
            }
            tests += 1;
        } else if g == GoLine::Free && !kept && hidden_by(Runner::Gotest, bare) {
            if no_tests == 0 {
                pieces.push(P::NoTests);
            }
            no_tests += 1;
        } else {
            pieces.push(P::Line(line));
        }
    }
    if tests == 0 && no_tests == 0 {
        return None;
    }
    let mut out = String::with_capacity(text.len());
    for p in pieces {
        match p {
            P::Line(l) => out.push_str(l),
            P::Passed => out.push_str(&format!("[zygo: {tests} lines of passed tests hidden]\n")),
            P::NoTests => out.push_str(&format!("[zygo: {no_tests} packages with no test files hidden]\n")),
        }
    }
    if out.len() >= text.len() {
        return None;
    }
    Some((out, vec![Step::PassedTestsHidden]))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Edge lines of changes A, B and D (OS-47 triage): Unicode white space in names, `\r` endings, the dedup
    /// counted form, near-miss prefixes, indented results, and package lines.
    const LINES: &[&str] = &[
        "test a ... ok", "test a ... ignored", "test a ... ignored, x ... ok", "test a ... ignored, ",
        "test a ... ok ", "test  ... ok", "test ... ok", "test a b ... ok", "test b\u{2003}c ... ok",
        "test d\u{a0}e ... ok", "test e\u{85}f ... ok", "test g\u{180e}h ... ok", "test i\u{200b}j ... ok",
        "test f ... ok\r", "test g ... ok [repeated 3 times]", "test i ... okay", "test ok ... ok",
        "test a ... FAILED", "   Compiling zygo v0.1.0", "test result: ok. 1 passed; 0 failed;",
        "=== RUN\tTestX", "=== RUN   TestX", "=== RUNNER TestY", "=== RUN", "=== RUN ", "=== RUN   a b",
        "=== RUN   a\u{2003}b", "=== PAUSE TestX", "=== CONT  TestX", "=== NAME  TestX", "=== RUN   TestZ\r",
        " === RUN   TestX", "--- PASS: TestX (0.00s)", "--- PASSX: TestX (0.00s)",
        "    --- PASS: TestX/sub (0.00s)", "\t--- FAIL: TestX (1.2s)", "--- SKIP: TestX (0.00s)",
        "--- PASS: TestX\u{2003}(0.00s)", "--- PASS: TestX  (0.00s)", "--- PASS: TestX (0.00s) tail",
        "--- FAIL: TestX (0.00s)\r", "--- PASS:  TestX (0.00s)", "--- PASS: \u{e9}t\u{e9} (0.00s)", "[stderr]",
        "ok", "okay", "ok  \texample.com/x\t0.1s", "ok example.com/x", "FAIL", "FAIL\texample.com/x\t0.1s",
        "FAILED", "PASS", "PASSED", "panic: boom", "exit status 1", "exit status",
        "?   \texample.com/none\t[no test files]", "? ", "", "=== FAILURES ===",
        "==== short test summary info ====", "= PASSES =", "failures:", "    boom", "---- boom stdout ----",
    ];

    /// The go test fixture texts (O§7.2 raw text; their `stderr` files are empty, so the raw text is `stdout`).
    const GO_FIXTURES: &[&str] = &[
        include_str!("../../../../fixtures/shaping/gotest/failing-parallel/stdout"),
        include_str!("../../../../fixtures/shaping/gotest/panic/stdout"),
        include_str!("../../../../fixtures/shaping/gotest/passing-verbose/stdout"),
    ];

    /// The edge texts of the section guard test: each line of `LINES` alone, with `\n`, between two lines, and
    /// all of them joined by `\n` and by `\r\n`.
    fn edge_texts() -> Vec<String> {
        let mut texts: Vec<String> =
            LINES.iter().flat_map(|l| [l.to_string(), format!("{l}\n"), format!("x\n{l}\ny\n")]).collect();
        texts.push(LINES.join("\n"));
        texts.push(LINES.join("\r\n"));
        texts
    }

    /// `section` as it is at 442defa3: go test reads `go_lines_reference`, not the fast `go_lines`.
    fn section_reference(r: Runner, text: &str) -> Vec<bool> {
        match r {
            Runner::Gotest => go_lines_reference(text).into_iter().map(|g| g == GoLine::Kept).collect(),
            _ => section(r, text),
        }
    }

    /// `go_lines` verbatim as it is at commit 442defa3 (before the OS-47 fast path), with the regex `captures`
    /// calls: the reference that the fast `go_lines` must equal.
    fn go_lines_reference(text: &str) -> Vec<GoLine> {
        let mut pkg = 0usize;
        let mut cur: Option<GoKey> = None;
        let mut open: Vec<(GoKey, usize)> = Vec::new();
        let mut owner: Vec<Option<GoKey>> = Vec::new();
        let mut passed: HashMap<GoKey, bool> = HashMap::new();
        for line in text.split_inclusive('\n') {
            let bare = line.strip_suffix('\n').unwrap_or(line);
            let rest = bare.trim_start_matches([' ', '\t']);
            let depth = bare.len() - rest.len();
            let cr = bare.ends_with('\r');
            while open.last().is_some_and(|&(_, d)| d >= depth) {
                open.pop();
            }
            let summary = GO_SUMMARY.is_match(bare);
            let o = if bare == "[stderr]" || (summary && !bare.starts_with('?')) {
                pkg += 1;
                cur = None;
                open.clear();
                None
            } else if summary && cur.is_none() {
                None
            } else if GO_PACKAGE.is_match(bare) {
                cur = None;
                open.clear();
                None
            } else if let Some(c) = GO_MARK.captures(bare).filter(|_| !cr) {
                cur = Some((pkg, c.get(2).expect("group 2").as_str()));
                cur
            } else if let Some(c) = GO_RESULT.captures(bare).filter(|_| !cr) {
                let key = (pkg, c.get(2).expect("group 2").as_str());
                let ok = &c[1] != "FAIL";
                passed.entry(key).and_modify(|p| *p &= ok).or_insert(ok);
                open.push((key, depth));
                let within = |n: &str| n == key.1 || n.strip_prefix(key.1).is_some_and(|r| r.starts_with('/'));
                if cur.is_some_and(|(p, n)| p == key.0 && within(n)) {
                    cur = None;
                }
                Some(key)
            } else if rest.starts_with("=== ") || rest.starts_with("--- ") {
                cur = None;
                None
            } else {
                open.last().map(|&(k, _)| k).or(cur)
            };
            owner.push(o);
        }
        text.split_inclusive('\n')
            .zip(owner)
            .map(|(line, o)| {
                let bare = line.strip_suffix('\n').unwrap_or(line);
                match o {
                    None => GoLine::Free,
                    Some(k) if passed.get(&k) == Some(&true) && !bare.ends_with('\r') && !COUNTED.is_match(bare) => {
                        GoLine::Passed
                    }
                    Some(_) => GoLine::Kept,
                }
            })
            .collect()
    }

    #[test]
    fn runner_fast_path_libtest_test_line_and_ok_match_test_hidden() {
        for line in LINES {
            if hidden_by(Runner::Libtest, line) {
                assert_eq!(line.starts_with("test "), TEST_HIDDEN.is_match(line), "{line:?}");
            }
            if let Some(c) = TEST_HIDDEN.captures(line) {
                assert_eq!(libtest_ok(line), &c[1] == "ok", "{line:?}");
            }
        }
        assert!(libtest_ok("test a ... ok") && !libtest_ok("test a ... ignored, x ... ok"));
    }

    #[test]
    fn runner_fast_path_go_guards_give_the_regex_answers() {
        for line in LINES {
            assert_eq!(go_summary_line(line), GO_SUMMARY.is_match(line), "{line:?}");
            assert_eq!(go_package_line(line), GO_PACKAGE.is_match(line), "{line:?}");
            assert_eq!(go_mark(line), GO_MARK.captures(line).map(|c| c.get(2).expect("group 2").as_str()), "{line:?}");
            let want = GO_RESULT.captures(line).map(|c| (c.get(1).expect("1").as_str(), c.get(2).expect("2").as_str()));
            assert_eq!(go_result(line), want, "{line:?}");
        }
        assert_eq!(go_mark("=== RUN\tTestX"), Some("TestX"));
        assert_eq!(go_result("    --- PASS: TestX/sub (0.00s)"), Some(("PASS", "TestX/sub")));
    }

    #[test]
    fn runner_fast_path_section_guard_keeps_every_section() {
        let all = [Runner::Libtest, Runner::Pytest, Runner::Gotest];
        for t in &edge_texts() {
            for o in all {
                if section_reference(o, t).contains(&true) {
                    assert!(may_have_section(o, t), "{o:?} {t:?}");
                }
            }
            for r in all {
                // the mask of `view` at 442defa3: both other runners, unguarded
                let mut old = vec![false; t.split_inclusive('\n').count()];
                for o in all.into_iter().filter(|&o| o != r) {
                    for (k, s) in old.iter_mut().zip(section_reference(o, t)) {
                        *k |= s;
                    }
                }
                let new = other_sections(r, t);
                assert!(new == old || (new.is_empty() && !old.contains(&true)), "{r:?} {t:?}");
            }
        }
        for o in all {
            assert!(!may_have_section(o, "test a ... ok\n"), "{o:?}");
        }
    }

    #[test]
    fn runner_fast_path_go_lines_equal_the_reference() {
        let mut texts = edge_texts();
        texts.extend(GO_FIXTURES.iter().map(|f| f.to_string()));
        texts.push(GO_FIXTURES.concat());
        for t in &texts {
            assert!(go_lines(t) == go_lines_reference(t), "{t:?}");
        }
    }
}
