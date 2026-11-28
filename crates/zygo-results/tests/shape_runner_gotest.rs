use std::path::PathBuf;
use std::sync::LazyLock;

use regex::Regex;
use zygo_results::Kind;
use zygo_results::shape::command::Runner::{Gotest, Libtest, Pytest};
use zygo_results::shape::command::parse;
use zygo_results::shape::runner::{gotest, gotest_passed_lines, hidden_by, view};
use zygo_results::shape::{self, ShapeInput, Shaped, Shaper, Step};
use Step::PassedTestsHidden as Passed;

static PASSED_COUNT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\[zygo: (\d+) lines of passed tests hidden\]$").unwrap());
static NO_TESTS_COUNT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\[zygo: (\d+) packages with no test files hidden\]$").unwrap());
/// The O§7.6 go test rule 4 package lines.
static PACKAGE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^((ok|FAIL|PASS)([ \t].*)?|panic:.*|exit status .*)$").unwrap());

const CASES: [&str; 3] = ["failing-parallel", "panic", "passing-verbose"];

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
fn case(name: &str) -> (String, Vec<String>) { case_in("gotest", name) }
fn bare(l: &str) -> &str { l.strip_suffix('\n').unwrap_or(l) }
fn is_count(l: &str) -> bool { PASSED_COUNT.is_match(l) || NO_TESTS_COUNT.is_match(l) }
/// The raw lines the view removed, with their 1-based line numbers (O§8.3 item 5): one cursor over the view lines
/// that are not count lines; a raw line equal to the line at the cursor is kept, any other raw line is removed.
fn removed(raw: &str, view: &str) -> Vec<(usize, String)> {
    let kept: Vec<&str> = view.split_inclusive('\n').map(bare).filter(|l| !is_count(l)).collect();
    let (mut i, mut out) = (0, Vec::new());
    for (n, l) in raw.split_inclusive('\n').map(bare).enumerate() {
        if kept.get(i) == Some(&l) {
            i += 1;
        } else {
            out.push((n + 1, l.to_string()));
        }
    }
    assert_eq!(i, kept.len(), "a view line is not a raw line in order: {:?}", kept.get(i));
    out
}
fn count(view: &str, re: &Regex) -> Vec<usize> {
    view.lines().filter_map(|l| re.captures(l)).map(|c| c[1].parse().unwrap()).collect()
}
fn saving(raw: &str, view: &str) -> f64 { 1.0 - view.len() as f64 / raw.len() as f64 }

/// `failing-parallel` through the view: only `TestGroup`, `TestGroup/fail_fetch` and package lines stay.
const FP_VIEW: &str = concat!(
    "[zygo: 1 packages with no test files hidden]\n",
    "=== RUN   TestGroup\n",
    "[zygo: 290 lines of passed tests hidden]\n",
    "=== RUN   TestGroup/fail_fetch\n",
    "=== PAUSE TestGroup/fail_fetch\n",
    "=== CONT  TestGroup/fail_fetch\n",
    "    group_test.go:22: fetch start\n",
    "=== NAME  TestGroup/fail_fetch\n",
    "    group_test.go:25: fetch attempt 1: status 503\n",
    "=== NAME  TestGroup/fail_fetch\n",
    "    group_test.go:25: fetch attempt 2: status 503\n",
    "=== NAME  TestGroup/fail_fetch\n",
    "    group_test.go:25: fetch attempt 3: status 503\n",
    "    group_test.go:28: want status 200, got 503\n",
    "--- FAIL: TestGroup (0.00s)\n",
    "    --- FAIL: TestGroup/fail_fetch (0.03s)\n",
    "FAIL\n",
    "FAIL\texample.com/gotestfix/group\t0.037s\n",
    "PASS\n",
    "ok  \texample.com/gotestfix/parse\t0.005s\n",
    "FAIL\n",
);

/// Interleaved `t.Parallel()` output of three subtests (pass, fail, skip), a `[no test files]` line printed by the
/// failing test, a `--- SKIP` subtest inside a failing parent's result block, the same test name passing in another
/// package, and build-error lines after a package line.
const G: &str = concat!(
    "?   \texample.com/m/docs\t[no test files]\n",
    "=== RUN   TestFetch\n",
    "=== RUN   TestFetch/ok\n",
    "=== PAUSE TestFetch/ok\n",
    "=== RUN   TestFetch/bad\n",
    "=== PAUSE TestFetch/bad\n",
    "=== RUN   TestFetch/later\n",
    "=== PAUSE TestFetch/later\n",
    "=== CONT  TestFetch/ok\n",
    "    fetch_test.go:12: ok: start\n",
    "=== CONT  TestFetch/bad\n",
    "    fetch_test.go:12: bad: start\n",
    "=== CONT  TestFetch/later\n",
    "    fetch_test.go:30: not on this host\n",
    "=== NAME  TestFetch/ok\n",
    "    fetch_test.go:14: ok: done\n",
    "print from ok\n",
    "=== NAME  TestFetch/bad\n",
    "    fetch_test.go:16: bad: status 503\n",
    "print from bad\n",
    "?   \texample.com/m/inner\t[no test files]\n",
    "--- FAIL: TestFetch (0.00s)\n",
    "    --- PASS: TestFetch/ok (0.01s)\n",
    "    --- FAIL: TestFetch/bad (0.01s)\n",
    "    --- SKIP: TestFetch/later (0.00s)\n",
    "FAIL\n",
    "FAIL\texample.com/m/fetch\t0.012s\n",
    "?   \texample.com/m/tools\t[no test files]\n",
    "=== RUN   TestSetup\n",
    "=== RUN   TestSetup/db\n",
    "--- FAIL: TestSetup (0.00s)\n",
    "    setup_test.go:9: setup failed\n",
    "    --- SKIP: TestSetup/db (0.00s)\n",
    "        setup_test.go:12: no db\n",
    "    setup_test.go:14: fatal after skip\n",
    "=== RUN   TestFetch\n",
    "    fetch_test.go:5: same name, other package\n",
    "--- PASS: TestFetch (0.00s)\n",
    "FAIL\n",
    "FAIL\texample.com/m/setup\t0.004s\n",
    "# example.com/m/broken\n",
    "broken/x.go:3:1: syntax error: unexpected }\n",
    "FAIL\texample.com/m/broken [build failed]\n",
    "FAIL\n",
);
/// The lines of `G` that belong to `TestFetch/ok`, `TestFetch/later`, `TestSetup/db` and the second `TestFetch`.
const G_PASSED: [usize; 19] = [3, 4, 7, 8, 9, 10, 13, 14, 15, 16, 17, 23, 25, 30, 33, 34, 36, 37, 38];
const G_VIEW: &str = concat!(
    "[zygo: 2 packages with no test files hidden]\n",
    "=== RUN   TestFetch\n",
    "[zygo: 19 lines of passed tests hidden]\n",
    "=== RUN   TestFetch/bad\n",
    "=== PAUSE TestFetch/bad\n",
    "=== CONT  TestFetch/bad\n",
    "    fetch_test.go:12: bad: start\n",
    "=== NAME  TestFetch/bad\n",
    "    fetch_test.go:16: bad: status 503\n",
    "print from bad\n",
    "?   \texample.com/m/inner\t[no test files]\n",
    "--- FAIL: TestFetch (0.00s)\n",
    "    --- FAIL: TestFetch/bad (0.01s)\n",
    "FAIL\n",
    "FAIL\texample.com/m/fetch\t0.012s\n",
    "=== RUN   TestSetup\n",
    "--- FAIL: TestSetup (0.00s)\n",
    "    setup_test.go:9: setup failed\n",
    "    setup_test.go:14: fatal after skip\n",
    "FAIL\n",
    "FAIL\texample.com/m/setup\t0.004s\n",
    "# example.com/m/broken\n",
    "broken/x.go:3:1: syntax error: unexpected }\n",
    "FAIL\texample.com/m/broken [build failed]\n",
    "FAIL\n",
);

/// A parallel test killed by the test timeout while a passed test's lines sit around it: `TestB` has no result line.
const T: &str = concat!(
    "=== RUN   TestA\n",
    "=== PAUSE TestA\n",
    "=== RUN   TestB\n",
    "=== PAUSE TestB\n",
    "=== CONT  TestA\n",
    "=== CONT  TestB\n",
    "    b_test.go:5: b: working\n",
    "=== NAME  TestA\n",
    "    a_test.go:5: a: done\n",
    "--- PASS: TestA (0.00s)\n",
    "=== NAME  TestB\n",
    "    b_test.go:7: b: still waiting\n",
    "panic: test timed out after 1s\n",
    "\trunning tests:\n",
    "\t\tTestB (1s)\n",
    "FAIL\texample.com/m\t1.005s\n",
    "FAIL\n",
);
const T_VIEW: &str = concat!(
    "[zygo: 6 lines of passed tests hidden]\n",
    "=== RUN   TestB\n",
    "=== PAUSE TestB\n",
    "=== CONT  TestB\n",
    "    b_test.go:5: b: working\n",
    "=== NAME  TestB\n",
    "    b_test.go:7: b: still waiting\n",
    "panic: test timed out after 1s\n",
    "\trunning tests:\n",
    "\t\tTestB (1s)\n",
    "FAIL\texample.com/m\t1.005s\n",
    "FAIL\n",
);

/// The texts that have a go test view.
fn viewed() -> Vec<String> {
    let mut v: Vec<String> = CASES.iter().map(|n| case(n).0).collect();
    v.extend([G.to_string(), T.to_string()]);
    v
}

#[test]
fn failing_parallel_keeps_the_failing_test_and_hides_the_passing_ones() {
    // O§16 item 4, O§7.6 go test rules 1-3, O§8.2 failing floor
    let (raw, keep) = case("failing-parallel");
    assert_eq!(keep.len(), 7);
    let (v, steps) = gotest(&raw).unwrap();
    assert_eq!(v, FP_VIEW);
    assert_eq!(steps, vec![Passed]);
    assert_eq!((raw.len(), v.len()), (9900, 689));
    assert!(saving(&raw, &v) >= 0.30, "{}", saving(&raw, &v));
    for k in &keep { assert!(v.lines().any(|x| x == k), "{k}"); }
    let r: Vec<&str> = raw.lines().collect();
    let mut after = 0;
    for i in 0..r.len() {
        if r[i] == "=== CONT  TestGroup/fail_fetch" || r[i] == "=== NAME  TestGroup/fail_fetch" {
            assert!(v.lines().any(|x| x == r[i + 1]), "{}", r[i + 1]);
            after += 1;
        }
    }
    assert_eq!(after, 4);
    for l in v.lines() { assert!(!l.contains("pass_") && !l.contains("TestField"), "{l}"); }
}

#[test]
fn package_lines_stay() {
    // O§7.6 go test rule 4
    for raw in viewed() {
        let v = gotest(&raw).unwrap().0;
        let want: Vec<&str> = raw.lines().filter(|l| PACKAGE.is_match(l)).collect();
        assert!(!want.is_empty());
        let got: Vec<&str> = v.lines().filter(|l| PACKAGE.is_match(l)).collect();
        assert_eq!(got, want);
    }
}

#[test]
fn a_test_with_no_result_line_keeps_all_its_lines() {
    // O§7.6 go test rule 3: a panic or a timeout keeps the lines of the test with no result
    let (raw, keep) = case("panic");
    let (v, steps) = gotest(&raw).unwrap();
    assert_eq!((raw.len(), v.len()), (6038, 437));
    assert_eq!(steps, vec![Passed]);
    let tail = &raw[raw.find("=== RUN   TestWorker\n").unwrap()..];
    assert!(v.ends_with(tail));
    for k in &keep { assert!(v.lines().any(|x| x == k), "{k}"); }
    assert_eq!(gotest(T), Some((T_VIEW.to_string(), vec![Passed])));
}

#[test]
fn passing_verbose_keeps_only_package_lines() {
    // O§7.6 "Pass and fail": a run that passed keeps its summary lines
    let (raw, keep) = case("passing-verbose");
    assert!(keep.is_empty());
    let want = "[zygo: 1 packages with no test files hidden]\n[zygo: 292 lines of passed tests hidden]\n\
                PASS\nok  \texample.com/gotestfix/group\t0.038s\nPASS\nok  \texample.com/gotestfix/parse\t0.005s\n";
    assert_eq!(gotest(&raw), Some((want.to_string(), vec![Passed])));
    assert_eq!(raw.len(), 9435);
}

#[test]
fn interleaved_parallel_output_goes_to_the_right_test() {
    // O§7.6 go test rules 1-3; review focus 18: a `--- SKIP` around a failing parent, a line after `=== NAME`
    assert_eq!(gotest_passed_lines(G), G_PASSED.to_vec());
    assert_eq!(gotest(G), Some((G_VIEW.to_string(), vec![Passed])));
}

#[test]
fn no_test_files_lines_are_hidden_with_their_count_line() {
    // O§7.6 go test row: `^\?\s+\S+\s+\[no test files\]$`, count line at the place of the first one
    for raw in viewed() {
        let v = gotest(&raw).unwrap().0;
        let n = raw.lines().filter(|l| hidden_by(Gotest, l)).count();
        let kept = v.lines().filter(|l| hidden_by(Gotest, l)).count();
        assert_eq!(count(&v, &NO_TESTS_COUNT).iter().sum::<usize>() + kept, n);
    }
    for name in CASES {
        let v = gotest(&case(name).0).unwrap().0;
        assert_eq!(v.lines().next(), Some("[zygo: 1 packages with no test files hidden]"), "{name}");
    }
    // O-C9: the line that the failing test printed stays
    assert!(G_VIEW.lines().any(|l| l == "?   \texample.com/m/inner\t[no test files]"));
}

#[test]
fn removed_lines_are_passed_lines_or_hidden_forms() {
    // O§8.3 item 5; contract: each removed line is in `gotest_passed_lines` or matches `hidden_by(Gotest, _)`
    for raw in viewed() {
        let v = gotest(&raw).unwrap().0;
        let passed = gotest_passed_lines(&raw);
        let gone = removed(&raw, &v);
        let by_test: Vec<usize> = gone.iter().filter(|(n, _)| passed.contains(n)).map(|(n, _)| *n).collect();
        assert_eq!(by_test, passed);
        for (n, l) in &gone { assert!(passed.contains(n) || hidden_by(Gotest, l), "{n}: {l:?}"); }
        assert_eq!(count(&v, &PASSED_COUNT), vec![passed.len()]);
        assert_eq!(count(&v, &NO_TESTS_COUNT).iter().sum::<usize>(), gone.len() - passed.len());
    }
}

#[test]
fn package_line_and_stderr_end_the_current_test() {
    // O-C9: a line after the package's summary line or after `[stderr]` belongs to no test
    let s = "=== RUN   TestA\n--- PASS: TestA (0.00s)\n[stderr]\nwarning: x\n";
    assert_eq!(gotest_passed_lines(s), vec![1, 2]);
    let p = "=== RUN   TestA\n--- PASS: TestA (0.00s)\nPASS\nok  \texample.com/m\t0.001s\n# example.com/m/b\nb/x.go:3:1: syntax error\n";
    assert_eq!(gotest_passed_lines(p), vec![1, 2]);
}

#[test]
fn cr_and_counted_lines_are_never_hidden() {
    // O§7.6 "a line that ends in CR matches no form"; O§7.5 counted lines stay (OS-30 review F3)
    let c = concat!(
        "=== RUN   TestX\n",
        "    x_test.go:3: x\n",
        "--- PASS: TestX (0.00s)\r\n",
        "=== RUN   TestY\n",
        "    y_test.go:3: retry [repeated 4 times]\n",
        "    y_test.go:4: y\n",
        "--- PASS: TestY (0.00s)\n",
        "PASS\n",
        "ok  \texample.com/m\t0.001s\n",
    );
    assert_eq!(gotest_passed_lines(c), vec![4, 6, 7]);
}

#[test]
fn hidden_by_takes_only_the_go_form() {
    for l in ["?   \texample.com/gotestfix/docs\t[no test files]", "? x [no test files]", "?\tx\t[no test files]"] {
        assert!(hidden_by(Gotest, l), "{l:?}");
        assert!(!hidden_by(Libtest, l) && !hidden_by(Pytest, l), "{l:?}");
    }
    for l in [
        " ?   \tx\t[no test files]", "?   \tx\t[no test files]\r", "?   \tx\t[no test files] [repeated 3 times]",
        "?\u{a0}x [no test files]", "?   \tx\t[no test files] extra", "?   \tx\t0.005s", "?   \tx",
        "ok  \texample.com/gotestfix/parse\t0.005s", "FAIL\texample.com/gotestfix/group\t0.037s", "FAIL", "PASS",
        "=== RUN   TestGroup", "--- PASS: TestGroup (0.00s)", "    --- PASS: TestGroup/pass_01 (0.03s)",
        "test t::a ... ok", "tests/test_a.py ....", "",
    ] {
        assert!(!hidden_by(Gotest, l), "{l:?}");
    }
}

#[test]
fn view_dispatches_go_output_to_gotest() {
    // O§7.6 dispatch: the first runner whose summary is in the text decides
    for name in CASES {
        let (raw, _) = case(name);
        let (v, s) = gotest(&raw).unwrap();
        let want = Some((v, s, Shaper::Gotest));
        for rs in [&[Gotest][..], &[Libtest, Pytest, Gotest], &[Pytest, Gotest], &[Gotest, Libtest]] {
            assert_eq!(view(&raw, rs), want, "{name} {rs:?}");
        }
        for rs in [&[][..], &[Libtest], &[Pytest], &[Libtest, Pytest]] {
            assert_eq!(view(&raw, rs), None, "{name} {rs:?}");
        }
    }
    for (dir, name) in [("libtest", "passing"), ("libtest", "failing-nocapture"), ("libtest", "signal-crash"),
                        ("libtest", "compile-error"), ("pytest", "passing"), ("pytest", "failing-verbose"),
                        ("pytest", "collection-error")] {
        let (raw, _) = case_in(dir, name);
        assert_eq!(gotest(&raw), None, "{dir}/{name}");
        assert_eq!(view(&raw, &[Gotest]), None, "{dir}/{name}");
    }
}

#[test]
fn text_without_a_summary_line_gives_none() {
    // O§7.6 "A parse that does not find the summary gives no change"
    let (raw, _) = case("failing-parallel");
    let body: String = raw.split_inclusive('\n')
        .filter(|l| !l.starts_with("ok") && !l.starts_with("FAIL\t") && !l.starts_with('?')).collect();
    for t in [
        String::new(),
        body,
        "--- FAIL: TestX (0.00s)\nFAIL\n".to_string(),
        G.replace("FAIL\texample.com/m/", " FAIL\texample.com/m/").replace("?   \t", " ?   \t"),
    ] {
        assert_eq!(gotest(&t), None, "{t:?}");
        assert_eq!(view(&t, &[Gotest]), None, "{t:?}");
    }
}

#[test]
fn summary_with_nothing_hidden_or_no_saving_gives_none() {
    // O§7.3 "No step adds content"
    let f = "=== RUN   TestA\n    a_test.go:3: boom\n--- FAIL: TestA (0.00s)\nFAIL\nFAIL\texample.com/m\t0.001s\nFAIL\n";
    assert_eq!(gotest(f), None);
    let n = "?   \tx\t[no test files]\n";
    assert!(hidden_by(Gotest, bare(n)));
    assert_eq!(gotest(n), None);
}

#[test]
fn pipeline_uses_the_gotest_view() {
    // O§7.3 step 3 with the `go test` command form and the wrapper `make test`
    for (name, cmd) in [("failing-parallel", "go test -v ./..."), ("panic", "go test -v ./..."),
                        ("passing-verbose", "go test -v ./..."), ("failing-parallel", "make test")] {
        let (raw, _) = case(name);
        let out = shape::run(&raw, &ShapeInput { parsed: Some(parse(cmd)), kind: Kind::Text, threshold: 16384 });
        let want = Shaped { text: gotest(&raw).unwrap().0, steps: vec![Passed], shaper: Some(Shaper::Gotest), size_view: false };
        assert_eq!(out, want, "{name}: {cmd}");
    }
}

#[test]
fn last_line_without_newline_stays_verbatim() {
    let (raw, _) = case("failing-parallel");
    let t = raw.strip_suffix('\n').unwrap();
    assert_eq!(gotest(t).unwrap().0, FP_VIEW.strip_suffix('\n').unwrap());
}

/// The view of `raw` keeps each of `keep`, and the lines it removes are exactly `gotest_passed_lines(raw)`.
fn keeps_and_hides_only_passed(raw: &str, keep: &[&str]) {
    let v = gotest(raw).expect("a view").0;
    for k in keep { assert!(v.lines().any(|x| x == *k), "{k:?} not in view:\n{v}"); }
    let gone: Vec<usize> = removed(raw, &v).into_iter().map(|(n, _)| n).collect();
    assert_eq!(gone, gotest_passed_lines(raw));
}

#[test]
fn panic_after_a_passed_sibling_keeps_its_stack() {
    // O-C9, review F1: the result line of the current test ends it, so the stack belongs to no test
    let p = concat!(
        "=== RUN   TestP\n",
        "=== RUN   TestP/a\n",
        "=== PAUSE TestP/a\n",
        "=== RUN   TestP/b\n",
        "=== PAUSE TestP/b\n",
        "=== CONT  TestP/b\n",
        "    x_test.go:18: b: start\n",
        "=== CONT  TestP/a\n",
        "    x_test.go:12: a: done\n",
        "    --- PASS: TestP/a (0.00s)\n",
        "panic: boom\n",
        "\n",
        "goroutine 21 [running]:\n",
        "example.com/m.TestP.func2(0xc000007a00)\n",
        "\t/repo/x_test.go:20 +0x5c\n",
        "testing.tRunner(0xc000007a00, 0x1100f8)\n",
        "\t/usr/local/go/src/testing/testing.go:1689 +0xec\n",
        "FAIL\texample.com/m\t0.012s\n",
        "FAIL\n",
    );
    let keep: Vec<&str> = p.lines().skip(10).collect();
    keeps_and_hides_only_passed(p, &keep);
    assert_eq!(gotest_passed_lines(p), vec![2, 3, 8, 9, 10]);
}

#[test]
fn panic_after_a_passed_test_keeps_its_stack() {
    // O-C9, review F1
    let p = concat!(
        "=== RUN   TestA\n",
        "    a_test.go:3: a ok\n",
        "--- PASS: TestA (0.00s)\n",
        "panic: boom\n",
        "\n",
        "goroutine 7 [running]:\n",
        "example.com/m.init.func1()\n",
        "\t/repo/a.go:9 +0x2c\n",
        "FAIL\texample.com/m\t0.01s\n",
    );
    let keep: Vec<&str> = p.lines().skip(3).collect();
    keeps_and_hides_only_passed(p, &keep);
}

#[test]
fn lines_after_pass_belong_to_no_test() {
    // O-C9, review F1: a data race report, TestMain teardown text and the coverage line after `PASS` stay
    let race = concat!(
        "=== RUN   TestA\n",
        "    a_test.go:3: a ok\n",
        "--- PASS: TestA (0.00s)\n",
        "PASS\n",
        "==================\n",
        "WARNING: DATA RACE\n",
        "Write at 0x00c000012345 by goroutine 8:\n",
        "  example.com/m.TestA.func1()\n",
        "      /repo/a_test.go:9 +0x44\n",
        "==================\n",
        "Found 1 data race(s)\n",
        "FAIL\texample.com/m\t0.02s\n",
    );
    let teardown = concat!(
        "=== RUN   TestA\n",
        "    a_test.go:3: a ok\n",
        "--- PASS: TestA (0.00s)\n",
        "PASS\n",
        "teardown: leaked 3 db connections\n",
        "FAIL\texample.com/m\t0.02s\n",
    );
    let cover = concat!(
        "=== RUN   TestA\n",
        "    a_test.go:3: a ok\n",
        "--- PASS: TestA (0.00s)\n",
        "PASS\n",
        "coverage: 81.2% of statements\n",
        "ok  \texample.com/m\t0.02s\n",
    );
    for t in [race, teardown, cover] {
        let keep: Vec<&str> = t.lines().skip(3).collect();
        keeps_and_hides_only_passed(t, &keep);
        assert_eq!(gotest_passed_lines(t), vec![1, 2, 3]);
    }
}

#[test]
fn an_unparsed_marker_line_ends_the_current_test() {
    // O-C9, review F2: a `=== ` line with no name (or a mark that ends in CR) gives later lines to no test
    let u = concat!(
        "=== RUN   TestA\n",
        "    a.go:1: x\n",
        "=== NAME \n",
        "unowned line\n",
        "--- PASS: TestA (0.00s)\n",
        "ok  \texample.com/m\t0.001s\n",
    );
    keeps_and_hides_only_passed(u, &["=== NAME ", "unowned line", "ok  \texample.com/m\t0.001s"]);
    assert_eq!(gotest_passed_lines(u), vec![1, 2, 5]);
    let c = u.replace("=== NAME \n", "=== NAME  TestA\r\n");
    assert_eq!(gotest_passed_lines(&c), vec![1, 2, 5]);
}
