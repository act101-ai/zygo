use std::path::PathBuf;
use std::sync::LazyLock;

use regex::Regex;
use zygo_results::shape::command::Runner::{Gotest, Libtest, Pytest};
use zygo_results::shape::runner::{hidden_by, libtest, view};
use zygo_results::shape::{Shaper, Step};
use Step::{BuildProgressHidden as Build, PassedTestsHidden as Passed};

static TESTS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\[zygo: (\d+) passed and (\d+) ignored test lines hidden\]$").unwrap());
static BUILD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\[zygo: (\d+) build progress lines hidden\]$").unwrap());

/// The O§7.2 raw text and the `must_keep` lines of an OS-19 libtest case.
fn case(name: &str) -> (String, Vec<String>) {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/shaping/libtest").join(name);
    let read = |f: &str| String::from_utf8(std::fs::read(p.join(f)).unwrap()).unwrap();
    let (so, se) = (read("stdout"), read("stderr"));
    let mut raw = so.clone();
    if !se.is_empty() {
        if !so.is_empty() && !so.ends_with('\n') {
            raw.push('\n');
        }
        raw.push_str("[stderr]\n");
        raw.push_str(&se);
    }
    let keep: Vec<String> = read("must_keep").lines().map(String::from).collect();
    assert!(keep.iter().all(|l| !l.is_empty()), "{name}: an empty must_keep line is invalid");
    (raw, keep)
}

fn bare(l: &str) -> &str {
    l.strip_suffix('\n').unwrap_or(l)
}

fn is_count(l: &str) -> bool {
    TESTS.is_match(l) || BUILD.is_match(l)
}

/// The raw lines that the view removed (O§8.3 item 5). One cursor over the view lines that are not
/// count lines: a raw line equal to the line at the cursor is kept and moves the cursor; any other
/// raw line is removed. Every view line must be used, in order.
fn removed<'a>(raw: &'a str, view: &str) -> Vec<&'a str> {
    let kept: Vec<&str> = view.split_inclusive('\n').map(bare).filter(|l| !is_count(l)).collect();
    let (mut i, mut out) = (0, Vec::new());
    for l in raw.split_inclusive('\n').map(bare) {
        if kept.get(i) == Some(&l) {
            i += 1;
        } else {
            out.push(l);
        }
    }
    assert_eq!(i, kept.len(), "a view line is not a raw line in order: {:?}", kept.get(i));
    out
}

fn saving(raw: &str, view: &str) -> f64 {
    1.0 - view.len() as f64 / raw.len() as f64
}

/// Hidden forms and the lines next to them that only look like them.
const A: &str = concat!(
    "   Compiling x v0.1.0 (/repo)\n",
    "    Checking y v1.0.0\n",
    "Compiling z v1\n",
    "    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.10s\n",
    "\n",
    "running 16 tests\n",
    "test t::ok_00 ... ok\n", "test t::ok_01 ... ok\n", "test t::ok_02 ... ok\n", "test t::ok_03 ... ok\n", "test t::ok_04 ... ok\n",
    "test t::ok_05 ... ok\n", "test t::ok_06 ... ok\n", "test t::ok_07 ... ok\n", "test t::ok_08 ... ok\n", "test t::ok_09 ... ok\n",
    "test t::b ... ignored\n",
    "test t::c ... ignored, needs db\n",
    "test t::d ... FAILED\n",
    "test t::e ... ok trailing\n",
    " test t::f ... ok\n",
    "test t::g ... okay\n",
    "note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace\n",
    "\n",
    "test result: FAILED. 10 passed; 1 failed; 2 ignored; 0 measured; 0 filtered out; finished in 0.00s\n",
);
const A_VIEW: &str = concat!(
    "[zygo: 2 build progress lines hidden]\n",
    "Compiling z v1\n",
    "    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.10s\n",
    "\n",
    "running 16 tests\n",
    "[zygo: 10 passed and 2 ignored test lines hidden]\n",
    "test t::d ... FAILED\n",
    "test t::e ... ok trailing\n",
    " test t::f ... ok\n",
    "test t::g ... okay\n",
    "note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace\n",
    "\n",
    "test result: FAILED. 10 passed; 1 failed; 2 ignored; 0 measured; 0 filtered out; finished in 0.00s\n",
);

/// Two test binaries.
const B: &str = concat!(
    "running 3 tests\n",
    "test alpha::adds_one ... ok\n",
    "test alpha::adds_two ... ok\n",
    "test alpha::adds_big ... ignored\n",
    "\n",
    "test result: ok. 2 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s\n",
    "\n",
    "running 3 tests\n",
    "test beta::subs_one ... ok\n",
    "test beta::subs_two ... ok\n",
    "test beta::subs_big ... ok\n",
    "\n",
    "test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n",
);
const B_VIEW: &str = concat!(
    "running 3 tests\n",
    "[zygo: 2 passed and 1 ignored test lines hidden]\n",
    "\n",
    "test result: ok. 2 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.00s\n",
    "\n",
    "running 3 tests\n",
    "[zygo: 3 passed and 0 ignored test lines hidden]\n",
    "\n",
    "test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n",
);

/// The texts that have a libtest view: three cases and the built inputs.
fn viewed() -> Vec<String> {
    let mut v: Vec<String> = ["passing", "failing-nocapture", "signal-crash"].iter().map(|n| case(n).0).collect();
    v.extend([A.to_string(), B.to_string()]);
    v
}

#[test]
fn passing_saves_80_percent_and_keeps_each_summary() {
    // O§8.2 row "libtest, passing"
    let (raw, _) = case("passing");
    let (v, steps) = libtest(&raw).expect("passing has a summary");
    assert!(saving(&raw, &v) >= 0.80, "{}", saving(&raw, &v));
    assert_eq!((raw.len(), v.len()), (3808, 501));
    let sums: Vec<&str> = raw.lines().filter(|l| l.starts_with("test result:")).collect();
    assert_eq!(sums.len(), 2);
    for s in sums {
        assert!(v.lines().any(|x| x == s), "{s}");
    }
    assert_eq!(steps, vec![Passed, Build]);
}

#[test]
fn passing_count_lines_sit_at_the_first_hidden_line() {
    // O§7.6 count line column
    let (raw, _) = case("passing");
    let v = libtest(&raw).unwrap().0;
    let l: Vec<&str> = v.lines().collect();
    let at = |s: &str| l.iter().position(|x| *x == s).unwrap();
    assert_eq!(l[at("running 120 tests") + 1], "[zygo: 120 passed and 0 ignored test lines hidden]");
    assert_eq!(l[at("[stderr]") + 1], "[zygo: 1 build progress lines hidden]");
    assert_eq!(l[at("running 0 tests") + 1], "");
    assert_eq!(l.iter().filter(|x| is_count(x)).count(), 2);
}

#[test]
fn failing_cases_keep_each_must_keep_line() {
    // O§8.2 row "failing", O-C9; compile-error has no view, so the raw text is what the model gets
    for name in ["failing-nocapture", "signal-crash", "compile-error"] {
        let (raw, keep) = case(name);
        assert!(!keep.is_empty(), "{name}");
        let v = libtest(&raw).map_or(raw.clone(), |(v, _)| v);
        for k in &keep {
            assert!(v.lines().any(|x| x == k), "{name}: {k}");
        }
    }
}

#[test]
fn failing_nocapture_saves_30_percent() {
    // O§8.2 row "libtest, pytest, go test, failing"
    let (raw, _) = case("failing-nocapture");
    let (v, steps) = libtest(&raw).unwrap();
    assert!(saving(&raw, &v) >= 0.30, "{}", saving(&raw, &v));
    assert_eq!((raw.len(), v.len()), (4256, 949));
    assert_eq!(steps, vec![Passed, Build]);
}

#[test]
fn compile_error_has_no_summary_and_gives_none() {
    // O§8.2 last paragraph: checked against the none row
    let (raw, _) = case("compile-error");
    assert!(!raw.lines().any(|l| l.starts_with("test result:")));
    assert_eq!(libtest(&raw), None);
    assert_eq!(view(&raw, &[Libtest, Pytest, Gotest]), None);
}

#[test]
fn removed_lines_match_hidden_by() {
    // O§8.3 item 5
    for raw in viewed() {
        let v = libtest(&raw).unwrap().0;
        let gone = removed(&raw, &v);
        assert!(!gone.is_empty());
        for l in gone {
            assert!(hidden_by(Libtest, l), "{l:?}");
        }
    }
}

#[test]
fn count_lines_name_only_what_was_hidden() {
    // O§7.6 "Pass and fail": the count lines name only what was hidden
    for raw in viewed() {
        let v = libtest(&raw).unwrap().0;
        let gone = removed(&raw, &v);
        let ok = gone.iter().filter(|l| l.starts_with("test ") && l.ends_with(" ... ok")).count();
        let ign = gone.iter().filter(|l| l.starts_with("test ") && !l.ends_with(" ... ok")).count();
        let prog = gone.len() - ok - ign;
        let (mut p, mut i, mut n) = (0, 0, 0);
        for l in v.lines() {
            if let Some(c) = TESTS.captures(l) {
                let (a, b): (usize, usize) = (c[1].parse().unwrap(), c[2].parse().unwrap());
                assert!(a + b > 0, "{l}");
                (p, i) = (p + a, i + b);
            }
            if let Some(c) = BUILD.captures(l) {
                let a: usize = c[1].parse().unwrap();
                assert!(a > 0, "{l}");
                n += a;
            }
        }
        assert_eq!((p, i, n), (ok, ign, prog));
        assert!(v.lines().filter(|l| BUILD.is_match(l)).count() <= 1);
    }
}

#[test]
fn hidden_forms_are_whole_lines() {
    // O§7.6 "A hidden form must match the whole line"
    assert_eq!(libtest(A), Some((A_VIEW.to_string(), vec![Passed, Build])));
}

#[test]
fn each_test_binary_gets_its_own_count_line() {
    // O§7.6 "at the place of the first hidden test line of each test binary"
    assert_eq!(libtest(B), Some((B_VIEW.to_string(), vec![Passed])));
}

#[test]
fn hidden_by_takes_only_the_row_forms() {
    for l in [
        "test t::a ... ok",
        "test t::b ... ignored",
        "test t::c ... ignored, needs db",
        "   Compiling x v0.1.0 (/repo)",
        "    Checking y v1.0.0",
        "\tFresh z v1",
        "  Downloaded a v1",
        "  Downloading crates ...",
        "    Locking 3 packages",
        "    Updating crates.io index",
        "    Blocking waiting for file lock on build directory",
    ] {
        assert!(hidden_by(Libtest, l), "{l:?}");
    }
    for l in [
        "test t::d ... FAILED",
        "test t::e ... ok trailing",
        " test t::f ... ok",
        "test t::g ... okay",
        "test t::h ... ok\r",
        "test a b ... ok",
        "Compiling z v1",
        "    Compiling",
        "    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.10s",
        "     Running unittests src/lib.rs (target/debug/deps/scratch-b66679a8c20ae5f6)",
        "   Doc-tests scratch",
        "running 3 tests",
        "test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s",
        "failures:",
        "---- t::d stdout ----",
        "error: test failed, to rerun pass `--lib`",
        "  process didn't exit successfully: `/repo/target/debug/deps/crash-06b6539749caaeec` (signal: 6, SIGABRT: process abort signal)",
        "thread 't::d' panicked at src/lib.rs:1:1:",
        "",
    ] {
        assert!(!hidden_by(Libtest, l), "{l:?}");
    }
    for r in [Pytest, Gotest] {
        assert!(!hidden_by(r, "test t::a ... ok"), "{r:?}");
        assert!(!hidden_by(r, "   Compiling x v0.1.0 (/repo)"), "{r:?}");
    }
}

#[test]
fn text_without_a_summary_gives_none() {
    // O§7.6 "A parse that does not find the summary gives no change"
    let oks = "test t::x ... ok\n".repeat(50);
    for head in [
        "",
        "   Compiling x v0.1.0 (/repo)\n",
        "test result: ok. 3 passed\n",
        "test result: ok. 3 passed; 0 failed\n",
        " test result: ok. 3 passed; 0 failed; 0 ignored\n",
        "test result: FAILED 3 passed; 1 failed;\n",
    ] {
        assert_eq!(libtest(&format!("{head}{oks}")), None, "{head:?}");
    }
    assert_eq!(libtest(""), None);
}

#[test]
fn summary_with_nothing_hidden_gives_none() {
    // O§7.3 "no change"; O§8.3 built input "a `test result:` line with no tests"; O§7.6 the first runner with a summary decides
    let t = "\nrunning 0 tests\n\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n\n";
    assert_eq!(libtest(t), None);
    assert_eq!(view(t, &[Libtest, Pytest, Gotest]), None);
}

#[test]
fn view_not_shorter_than_text_gives_none() {
    // O§7.3 "No step adds content": 1 hidden line of 14 bytes, a count line of 49
    let t = "test a ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n";
    assert_eq!(libtest(t), None);
}

#[test]
fn last_line_without_newline_stays_verbatim() {
    let t = B.strip_suffix('\n').unwrap();
    assert_eq!(libtest(t).unwrap().0, B_VIEW.strip_suffix('\n').unwrap());
}

#[test]
fn view_tries_runners_in_order() {
    // O§7.6 dispatch and wrapper order: a [Libtest, Pytest, Gotest] list on libtest output gives the libtest view
    let (raw, _) = case("passing");
    let (v, s) = libtest(&raw).unwrap();
    let want = Some((v, s, Shaper::Libtest));
    for rs in [&[Libtest, Pytest, Gotest][..], &[Libtest], &[Pytest, Libtest], &[Gotest, Pytest, Libtest]] {
        assert_eq!(view(&raw, rs), want, "{rs:?}");
    }
    for rs in [&[][..], &[Pytest], &[Gotest], &[Pytest, Gotest]] {
        assert_eq!(view(&raw, rs), None, "{rs:?}");
    }
}
