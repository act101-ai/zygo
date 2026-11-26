use std::path::PathBuf;
use std::sync::LazyLock;

use regex::Regex;
use zygo_results::Kind;
use zygo_results::shape::command::Runner::{Gotest, Libtest, Pytest};
use zygo_results::shape::command::parse;
use zygo_results::shape::runner::{hidden_by, libtest, pytest, view};
use zygo_results::shape::{self, ShapeInput, Shaped, Shaper, Step};
use Step::PassedTestsHidden as Passed;

static COUNT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\[zygo: (\d+) passed or skipped progress lines hidden\]$").unwrap());

const CASES: [&str; 3] = ["passing", "failing-verbose", "collection-error"];
const SUMMARY_PASSING: &str = "============================= 128 passed in 0.04s ==============================";
const FAILURES: &str = "=================================== FAILURES ===================================\n";
const ERRORS: &str = "==================================== ERRORS ====================================\n";
const SHORT: &str = "=========================== short test summary info ============================\n";

/// The O§7.2 raw text and the `must_keep` lines of a case under `fixtures/shaping/<dir>/<name>/`.
fn case_in(dir: &str, name: &str) -> (String, Vec<String>) {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/shaping").join(dir).join(name);
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
    assert!(keep.iter().all(|l| !l.is_empty()), "{dir}/{name}: an empty must_keep line is invalid");
    (raw, keep)
}
fn case(name: &str) -> (String, Vec<String>) { case_in("pytest", name) }
fn bare(l: &str) -> &str { l.strip_suffix('\n').unwrap_or(l) }
/// The raw lines the view removed (O§8.3 item 5): one cursor over the view lines that are not count lines;
/// a raw line equal to the line at the cursor is kept, any other raw line is removed; every view line is used.
fn removed<'a>(raw: &'a str, view: &str) -> Vec<&'a str> {
    let kept: Vec<&str> = view.split_inclusive('\n').map(bare).filter(|l| !COUNT.is_match(l)).collect();
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
fn saving(raw: &str, view: &str) -> f64 { 1.0 - view.len() as f64 / raw.len() as f64 }

/// Hidden forms and the lines next to them that only look like them.
const P: &str = concat!(
    "============================= test session starts ==============================\n",
    "collected 40 items\n",
    "\n",
    "tests/test_a.py ..........                                               [ 25%]\n",
    "tests/test_b.py ....F.....                                               [ 50%]\n",
    "tests/test_c.py sxX.                                                     [ 60%]\n",
    "tests/test_d.py E                                                        [ 62%]\n",
    "tests/test_e.py ......\n",
    " tests/test_f.py ..\n",
    "tests/test_g.pyc ..\n",
    "tests/test_h.py ..  [ 64%] extra\n",
    "tests/test_i.py::test_one PASSED                                         [ 65%]\n",
    "tests/test_i.py::test_two SKIPPED (no db)                                [ 67%]\n",
    "tests/test_i.py::test_three XFAIL (bug 7)                                [ 70%]\n",
    "tests/test_i.py::test_four XPASS (bug 8)                                 [ 72%]\n",
    "tests/test_i.py::test_five XPASS(strict) (bug 9)                         [ 75%]\n",
    "tests/test_i.py::test_six FAILED                                         [ 77%]\n",
    "tests/test_i.py::test_seven ERROR                                        [ 80%]\n",
    "\n",
    "=================================== FAILURES ===================================\n",
    "___________________________________ test_b5 ____________________________________\n",
    "E       assert 0\n",
    "tests/test_b.py:5: AssertionError\n",
    "=========================== short test summary info ============================\n",
    "FAILED tests/test_b.py::test_b5 - assert 0\n",
    "FAILED tests/test_i.py::test_five - [XPASS(strict)] bug 9\n",
    "FAILED tests/test_i.py::test_six - assert 0\n",
    "ERROR tests/test_d.py::test_d1 - RuntimeError\n",
    "ERROR tests/test_i.py::test_seven - RuntimeError\n",
    "=========== 3 failed, 31 passed, 2 skipped, 1 xfailed, 1 xpassed, 2 errors in 0.10s ===========\n",
);
/// `P` with lines 4, 6, 8 and 12-15 (1-based) hidden: 7 lines.
const P_VIEW: &str = concat!(
    "============================= test session starts ==============================\n",
    "collected 40 items\n",
    "\n",
    "[zygo: 7 passed or skipped progress lines hidden]\n",
    "tests/test_b.py ....F.....                                               [ 50%]\n",
    "tests/test_d.py E                                                        [ 62%]\n",
    " tests/test_f.py ..\n",
    "tests/test_g.pyc ..\n",
    "tests/test_h.py ..  [ 64%] extra\n",
    "tests/test_i.py::test_five XPASS(strict) (bug 9)                         [ 75%]\n",
    "tests/test_i.py::test_six FAILED                                         [ 77%]\n",
    "tests/test_i.py::test_seven ERROR                                        [ 80%]\n",
    "\n",
    "=================================== FAILURES ===================================\n",
    "___________________________________ test_b5 ____________________________________\n",
    "E       assert 0\n",
    "tests/test_b.py:5: AssertionError\n",
    "=========================== short test summary info ============================\n",
    "FAILED tests/test_b.py::test_b5 - assert 0\n",
    "FAILED tests/test_i.py::test_five - [XPASS(strict)] bug 9\n",
    "FAILED tests/test_i.py::test_six - assert 0\n",
    "ERROR tests/test_d.py::test_d1 - RuntimeError\n",
    "ERROR tests/test_i.py::test_seven - RuntimeError\n",
    "=========== 3 failed, 31 passed, 2 skipped, 1 xfailed, 1 xpassed, 2 errors in 0.10s ===========\n",
);

/// A failing test whose captured output is a pytest run (pytester): an `=` header and hidden forms inside FAILURES.
const F: &str = concat!(
    "============================= test session starts ==============================\n",
    "collected 6 items\n",
    "\n",
    "tests/test_a.py ....                                                     [ 66%]\n",
    "tests/test_b.py .F                                                       [100%]\n",
    "\n",
    "=================================== FAILURES ===================================\n",
    "_________________________________ test_inner ___________________________________\n",
    "----------------------------- Captured stdout call -----------------------------\n",
    "============================= test session starts ==============================\n",
    "inner/test_x.py ..                                                       [100%]\n",
    "inner/test_x.py::test_y PASSED                                           [100%]\n",
    "============================== 2 passed in 0.01s ===============================\n",
    "tests/test_b.py:9: AssertionError\n",
    "=========================== short test summary info ============================\n",
    "FAILED tests/test_b.py::test_inner - assert False\n",
    "========================= 1 failed, 5 passed in 0.03s ==========================\n",
);
/// `F` with only line 4 hidden.
const F_VIEW: &str = concat!(
    "============================= test session starts ==============================\n",
    "collected 6 items\n",
    "\n",
    "[zygo: 1 passed or skipped progress lines hidden]\n",
    "tests/test_b.py .F                                                       [100%]\n",
    "\n",
    "=================================== FAILURES ===================================\n",
    "_________________________________ test_inner ___________________________________\n",
    "----------------------------- Captured stdout call -----------------------------\n",
    "============================= test session starts ==============================\n",
    "inner/test_x.py ..                                                       [100%]\n",
    "inner/test_x.py::test_y PASSED                                           [100%]\n",
    "============================== 2 passed in 0.01s ===============================\n",
    "tests/test_b.py:9: AssertionError\n",
    "=========================== short test summary info ============================\n",
    "FAILED tests/test_b.py::test_inner - assert False\n",
    "========================= 1 failed, 5 passed in 0.03s ==========================\n",
);

/// A `make test` that ran `cargo test` and then `pytest`: both summaries, no captured output.
const M: &str = concat!(
    "running 3 tests\n",
    "test alpha::adds_one ... ok\n",
    "test alpha::adds_two ... ok\n",
    "test alpha::adds_three ... ok\n",
    "\n",
    "test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n",
    "\n",
    "============================= test session starts ==============================\n",
    "collected 12 items\n",
    "\n",
    "tests/test_a.py ............                                             [100%]\n",
    "\n",
    "============================== 12 passed in 0.02s ==============================\n",
);

/// The texts that have a pytest view.
fn viewed() -> Vec<String> {
    let mut v: Vec<String> = CASES.iter().map(|n| case(n).0).collect();
    v.extend([P.to_string(), F.to_string(), M.to_string()]);
    v
}

#[test]
fn passing_keeps_the_last_summary_line() {
    let (raw, keep) = case("passing");
    assert!(keep.is_empty());
    let (v, steps) = pytest(&raw).expect("passing has a summary");
    assert_eq!((raw.len(), v.len()), (1220, 311));
    assert_eq!(v.lines().last(), Some(SUMMARY_PASSING));
    assert_eq!(steps, vec![Passed]);
}

#[test]
fn failing_cases_keep_each_must_keep_line() {
    for name in ["failing-verbose", "collection-error"] {
        let (raw, keep) = case(name);
        assert!(!keep.is_empty(), "{name}");
        let v = pytest(&raw).unwrap().0;
        for k in &keep { assert!(v.lines().any(|x| x == k), "{name}: {k}"); }
    }
}

#[test]
fn failure_sections_are_whole() {
    for (name, head) in [("failing-verbose", FAILURES), ("collection-error", ERRORS)] {
        let (raw, _) = case(name);
        let v = pytest(&raw).unwrap().0;
        let tail = &raw[raw.find(head).unwrap()..];
        assert!(tail.contains(SHORT), "{name}");
        assert!(v.ends_with(tail), "{name}");
    }
}

#[test]
fn failing_verbose_saves_30_percent() {
    let (raw, _) = case("failing-verbose");
    let (v, steps) = pytest(&raw).unwrap();
    assert!(saving(&raw, &v) >= 0.30, "{}", saving(&raw, &v));
    assert_eq!((raw.len(), v.len()), (5168, 1379));
    assert_eq!(steps, vec![Passed]);
    let (raw, _) = case("collection-error");
    assert_eq!((raw.len(), pytest(&raw).unwrap().0.len()), (4995, 1206));
}

#[test]
fn count_line_sits_at_the_first_hidden_line() {
    for (name, head, n, next) in [
        ("passing", "collected 128 items", 12, ""),
        ("failing-verbose", "collecting ... collected 50 items", 48,
         "tests/test_calc.py::test_total FAILED                                    [ 82%]"),
        ("collection-error", "collecting ... collected 48 items / 1 error", 48, ""),
    ] {
        let (raw, _) = case(name);
        let v = pytest(&raw).unwrap().0;
        let l: Vec<&str> = v.lines().collect();
        let at = l.iter().position(|x| *x == head).unwrap();
        assert_eq!(l[at + 1], "", "{name}");
        assert_eq!(l[at + 2], format!("[zygo: {n} passed or skipped progress lines hidden]"), "{name}");
        assert_eq!(l[at + 3], next, "{name}");
        assert_eq!(l.iter().filter(|x| COUNT.is_match(x)).count(), 1, "{name}");
    }
}

#[test]
fn progress_lines_with_f_or_e_stay() {
    assert_eq!(pytest(P), Some((P_VIEW.to_string(), vec![Passed])));
    for l in [
        "tests/test_b.py ....F.....                                               [ 50%]",
        "tests/test_d.py E                                                        [ 62%]",
        "tests/test_i.py::test_five XPASS(strict) (bug 9)                         [ 75%]",
    ] {
        assert!(!hidden_by(Pytest, l), "{l:?}");
    }
    let (raw, _) = case("failing-verbose");
    let v = pytest(&raw).unwrap().0;
    assert!(v.lines().any(|x| x == "tests/test_text.py::test_plural FAILED                                   [100%]"));
}

#[test]
fn failure_section_lines_are_never_hidden() {
    assert_eq!(pytest(F), Some((F_VIEW.to_string(), vec![Passed])));
    assert!(hidden_by(Pytest, "inner/test_x.py ..                                                       [100%]"));
    assert!(hidden_by(Pytest, "inner/test_x.py::test_y PASSED                                           [100%]"));
}

#[test]
fn removed_lines_match_hidden_by() {
    for raw in viewed() {
        let v = pytest(&raw).unwrap().0;
        let gone = removed(&raw, &v);
        assert!(!gone.is_empty());
        for l in gone { assert!(hidden_by(Pytest, l), "{l:?}"); }
    }
}

#[test]
fn count_line_names_only_what_was_hidden() {
    for raw in viewed() {
        let v = pytest(&raw).unwrap().0;
        let n: Vec<usize> = v.lines().filter_map(|l| COUNT.captures(l)).map(|c| c[1].parse().unwrap()).collect();
        assert_eq!(n, vec![removed(&raw, &v).len()]);
    }
}

#[test]
fn hidden_by_takes_only_the_pytest_forms() {
    for l in [
        "tests/test_calc.py ........................................              [ 31%]",
        "tests/test_a.py .", "tests/test_a.py sxX.", "tests/test_a.py ....\t[100%]", "tests/test_a.py .... [  5%]", "a.py ..",
        "tests/test_calc.py::test_add[0] PASSED                                   [  2%]",
        "tests/t.py::test_a PASSED", "tests/t.py::C::test_a SKIPPED (no db)",
        "tests/t.py::test_a XFAIL (bug 7)", "tests/t.py::test_a XPASS (bug 8)", "tests/t.py::test_a XPASS",
    ] {
        assert!(hidden_by(Pytest, l), "{l:?}");
        assert!(!hidden_by(Libtest, l) && !hidden_by(Gotest, l), "{l:?}");
    }
    for l in [
        "tests/test_a.py ....F.....                                               [ 50%]",
        "tests/test_a.py E", "tests/test_a.py .F", "tests/test_a.py ", " tests/test_a.py ..",
        "tests/test_a.pyc ..", "tests/test_a.py ..  [ 64%] extra", "tests/test_a.py .. [64%",
        "tests/test_a.py .. [ \u{665}%]", "tests/test_a.py\u{a0}..", "tests/test_a.py ..\u{2003}[ 50%]", "tests/test_a.py ..\r",
        "tests/t.py::test_a FAILED", "tests/t.py::test_a ERROR", "tests/t.py::test_a XPASS(strict) (bug 9)",
        "tests/t.py::test_a PASSEDX", "tests/t.py::test a PASSED", "tests/t.py::test_a  PASSED",
        "tests/t.py::test_a PASSED\r", "tests/t.py::test_a PASSED\u{2003}[ 50%]",
        "tests/t.py::test_a PASSED [repeated 3 times]",
        "FAILED tests/t.py::test_a - assert 0", "ERROR tests/test_broken.py",
        "========================= 2 failed, 48 passed in 0.02s =========================",
        FAILURES.trim_end(), "collected 128 items", "",
    ] {
        assert!(!hidden_by(Pytest, l), "{l:?}");
    }
}

#[test]
fn view_dispatches_pytest_output_to_pytest() {
    for name in CASES {
        let (raw, _) = case(name);
        let (v, s) = pytest(&raw).unwrap();
        let want = Some((v, s, Shaper::Pytest));
        for rs in [&[Pytest][..], &[Libtest, Pytest, Gotest], &[Gotest, Pytest], &[Pytest, Libtest]] {
            assert_eq!(view(&raw, rs), want, "{name} {rs:?}");
        }
        for rs in [&[][..], &[Libtest], &[Gotest], &[Libtest, Gotest]] {
            assert_eq!(view(&raw, rs), None, "{name} {rs:?}");
        }
    }
    for (dir, name) in [("libtest", "passing"), ("libtest", "failing-nocapture"), ("libtest", "signal-crash"),
                        ("libtest", "compile-error"), ("gotest", "passing-verbose"), ("gotest", "failing-parallel"),
                        ("gotest", "panic")] {
        let (raw, _) = case_in(dir, name);
        assert_eq!(pytest(&raw), None, "{dir}/{name}");
        assert_eq!(view(&raw, &[Pytest]), None, "{dir}/{name}");
    }
}

#[test]
fn first_runner_with_a_summary_decides_on_mixed_output() {
    let (lv, ls) = libtest(M).unwrap();
    let (pv, ps) = pytest(M).unwrap();
    assert_ne!(lv, pv);
    assert_eq!(view(M, &[Libtest, Pytest, Gotest]), Some((lv, ls, Shaper::Libtest)));
    assert_eq!(view(M, &[Pytest, Libtest]), Some((pv, ps, Shaper::Pytest)));
}

#[test]
fn text_without_a_last_summary_line_gives_none() {
    let (raw, _) = case("passing");
    let body = &raw[..raw.trim_end_matches('\n').rfind('\n').unwrap() + 1];
    for t in [
        String::new(),
        body.to_string(),
        format!("{raw}extra\n"),
        format!("{raw}[stderr]\nwarning: x\n"),
        format!("{}\r\n", raw.trim_end_matches('\n')),
        raw.replace(SUMMARY_PASSING, "128 passed in 0.04s"),
        raw.replace(SUMMARY_PASSING, &format!(" {SUMMARY_PASSING}")),
        raw.replace(SUMMARY_PASSING, "============================= 128 passed in 0.04s"),
        raw.replace(SUMMARY_PASSING, "============================= no tests ran in 0.04s =============================="),
    ] {
        assert_eq!(pytest(&t), None, "{t:?}");
        assert_eq!(view(&t, &[Pytest]), None, "{t:?}");
    }
    assert_eq!(pytest(&format!("{raw}\n\n")).map(|x| x.1), Some(vec![Passed]));
}

#[test]
fn summary_with_nothing_hidden_or_no_saving_gives_none() {
    let i = concat!(
        "collected 0 items / 1 error\n",
        "\n",
        "==================================== ERRORS ====================================\n",
        "____________________ ERROR collecting tests/test_broken.py _____________________\n",
        "E   ModuleNotFoundError: No module named 'missingdep'\n",
        "=========================== short test summary info ============================\n",
        "ERROR tests/test_broken.py\n",
        "!!!!!!!!!!!!!!!!!!!! Interrupted: 1 error during collection !!!!!!!!!!!!!!!!!!!!\n",
        "=============================== 1 error in 0.05s ===============================\n",
    );
    assert_eq!(pytest(i), None);
    assert_eq!(view(i, &[Pytest]), None);
    let s = "a.py .\n============ 1 passed in 0.01s ============\n";
    assert!(hidden_by(Pytest, "a.py ."));
    assert_eq!(pytest(s), None);
}

#[test]
fn pipeline_uses_the_pytest_view() {
    for (name, cmd) in [("passing", "pytest"), ("failing-verbose", "pytest -v"),
                        ("collection-error", "pytest -v --continue-on-collection-errors"),
                        ("failing-verbose", "python3 -m pytest -v"), ("failing-verbose", "make test")] {
        let (raw, _) = case(name);
        let out = shape::run(&raw, &ShapeInput { parsed: Some(parse(cmd)), kind: Kind::Text, threshold: 16384 });
        let want = Shaped { text: pytest(&raw).unwrap().0, steps: vec![Passed], shaper: Some(Shaper::Pytest), size_view: false };
        assert_eq!(out, want, "{name}: {cmd}");
    }
}

#[test]
fn last_line_without_newline_stays_verbatim() {
    let (raw, _) = case("passing");
    let t = raw.strip_suffix('\n').unwrap();
    assert_eq!(pytest(t).unwrap().0, pytest(&raw).unwrap().0.strip_suffix('\n').unwrap());
}
