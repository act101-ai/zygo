//! OS-42: runner dispatch under wrappers and unseen live output (O§7.6, O-C9).

use zygo_results::Kind;
use zygo_results::shape::command::Runner::{Gotest, Libtest, Pytest};
use zygo_results::shape::command::parse;
use zygo_results::shape::runner::{libtest, view};
use zygo_results::shape::{self, ShapeInput, Shaped, Shaper, Step};

const WRAPPERS: [&str; 5] = ["make test", "just test", "npm test", "pnpm test", "yarn test"];

/// A pytest run whose `FAILURES` section captured a passing `cargo test` run (defect a).
const A: &str = concat!(
    "collected 1 item\n",
    "\n",
    "tests/test_cargo.py F                                                    [100%]\n",
    "\n",
    "=================================== FAILURES ===================================\n",
    "__________________________________ test_cargo __________________________________\n",
    "----------------------------- Captured stdout call -----------------------------\n",
    "running 6 tests\n",
    "test alpha::one ... ok\n",
    "test alpha::two ... ok\n",
    "test alpha::three ... ok\n",
    "test alpha::four ... ok\n",
    "test alpha::five ... ok\n",
    "test alpha::six ... ok\n",
    "\n",
    "test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n",
    "\n",
    "=========================== short test summary info ============================\n",
    "FAILED tests/test_cargo.py::test_cargo - assert 1 == 0\n",
    "============================== 1 failed in 0.31s ===============================\n",
);

/// A `cargo test` run that a wrapper ran before `A`.
const L: &str = concat!(
    "running 6 tests\n",
    "test beta::one ... ok\n",
    "test beta::two ... ok\n",
    "test beta::three ... ok\n",
    "test beta::four ... ok\n",
    "test beta::five ... ok\n",
    "test beta::six ... ok\n",
    "\n",
    "test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n",
    "\n",
);
const L_VIEW: &str = concat!(
    "running 6 tests\n",
    "[zygo: 6 passed and 0 ignored test lines hidden]\n",
    "\n",
    "test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n",
    "\n",
);

/// A failing go test that runs `cargo test`: the libtest lines are its own lines (O§7.6 go test rule 3).
const G: &str = concat!(
    "=== RUN   TestCargo\n",
    "running 6 tests\n",
    "test gamma::one ... ok\n",
    "test gamma::two ... ok\n",
    "test gamma::three ... ok\n",
    "test gamma::four ... ok\n",
    "test gamma::five ... ok\n",
    "test gamma::six ... ok\n",
    "\n",
    "test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n",
    "    cargo_test.go:21: cargo test passed, want a failure\n",
    "--- FAIL: TestCargo (0.31s)\n",
    "FAIL\n",
    "FAIL\texample.com/m\t0.31s\n",
);

/// A pytest `-s` run: the failing test `test_c` printed `step one` and then a line of the progress form (defect b).
const PRINT: &str = "inner/test_x.py ss\n";
const B: &str = concat!(
    "collected 23 items\n",
    "\n",
    "tests/test_b.py ..........                                               [ 43%]\n",
    "tests/test_d.py ..........                                               [ 86%]\n",
    "tests/test_a.py ..step one\n",
    "inner/test_x.py ss\n",
    "F                                                                        [100%]\n",
    "\n",
    "=================================== FAILURES ===================================\n",
    "___________________________________ test_c ____________________________________\n",
    "E   assert 0\n",
    "=========================== short test summary info ============================\n",
    "FAILED tests/test_a.py::test_c - assert 0\n",
    "========================= 1 failed, 22 passed in 0.02s =========================\n",
);

fn run(t: &str, cmd: &str) -> Shaped {
    shape::run(t, &ShapeInput { parsed: Some(parse(cmd)), kind: Kind::Text, threshold: 16384 })
}

fn unchanged(t: &str) -> Shaped {
    Shaped { text: t.to_string(), steps: vec![], shaper: None, size_view: false }
}

/// Acceptance (a): the captured `test … ... ok` lines in a pytest failure section stay under every command form.
#[test]
fn captured_cargo_lines_in_pytest_failures_stay() {
    assert!(libtest(A).is_some(), "the unmasked libtest view hides the captured lines");
    for cmd in WRAPPERS.into_iter().chain(["cargo test", "cargo test && pytest", "pytest; cargo test", "make test && cargo test"]) {
        assert_eq!(run(A, cmd), unchanged(A), "{cmd}");
    }
    for rs in [&[Libtest][..], &[Libtest, Gotest], &[Libtest, Pytest, Gotest], &[Pytest, Libtest]] {
        assert_eq!(view(A, rs), None, "{rs:?}");
    }
}

/// The mask keeps only the other runner's section: libtest lines before the pytest section are still hidden.
#[test]
fn libtest_lines_before_the_pytest_section_are_still_hidden() {
    let t = [L, A].concat();
    let want = Shaped {
        text: [L_VIEW, A].concat(),
        steps: vec![Step::PassedTestsHidden],
        shaper: Some(Shaper::Libtest),
        size_view: false,
    };
    for cmd in WRAPPERS.into_iter().chain(["cargo test"]) {
        assert_eq!(run(&t, cmd), want, "{cmd}");
    }
}

/// O§7.6 rewrite item 3: a go test's failure section is kept by the libtest view too.
#[test]
fn cargo_lines_of_a_failing_go_test_stay() {
    assert!(libtest(G).is_some(), "the unmasked libtest view hides the go test's lines");
    for cmd in WRAPPERS.into_iter().chain(["cargo test", "go test ./... && cargo test", "cargo test && go test ./..."]) {
        assert_eq!(run(G, cmd), unchanged(G), "{cmd}");
    }
    for rs in [&[Libtest][..], &[Libtest, Gotest], &[Gotest, Libtest]] {
        assert_eq!(view(G, rs), None, "{rs:?}");
    }
}

/// Acceptance (b): a wrapper tries no pytest view, so a failing test's progress-like print stays.
#[test]
fn wrapper_tries_no_pytest_view() {
    let direct = run(B, "pytest");
    assert_eq!(direct.shaper, Some(Shaper::Pytest), "the pytest view hides the print when it applies");
    assert!(!direct.text.contains(PRINT));
    for cmd in WRAPPERS {
        assert_eq!(parse(cmd).runners, vec![Libtest, Gotest], "{cmd}");
    }
    for cmd in WRAPPERS.into_iter().chain(["make -C sub test", "make test && pytest -v", "pytest -v; make test", "cd py && make test"]) {
        assert!(!parse(cmd).runners.contains(&Pytest), "{cmd}");
        let out = run(B, cmd);
        assert!(out.text.contains(PRINT), "{cmd}");
        assert_eq!(out, unchanged(B), "{cmd}");
    }
}

/// Acceptance (c): a `PYTEST_ADDOPTS` word or `-p no:capture` turns the pytest view off; the stored name stays.
#[test]
fn pytest_addopts_and_no_capture_turn_the_pytest_view_off() {
    for cmd in [
        "PYTEST_ADDOPTS=-s pytest",
        "PYTEST_ADDOPTS='-v -s' pytest -v",
        "PYTEST_ADDOPTS=-x pytest",
        "env PYTEST_ADDOPTS=-s pytest",
        "export PYTEST_ADDOPTS=-s; pytest",
        "export PYTEST_ADDOPTS=-s && python3 -m pytest",
        "PYTEST_ADDOPTS+=-s pytest",
        "pytest -p no:capture",
        "pytest -pno:capture",
        "pytest -vp no:capture",
        "pytest -vpno:capture",
        "python3 -m pytest -v -p no:capture",
        "uv run pytest -p no:capture",
    ] {
        assert!(!parse(cmd).runners.contains(&Pytest), "{cmd}");
        assert_eq!(run(B, cmd), unchanged(B), "{cmd}");
    }
    assert_eq!(parse("PYTEST_ADDOPTS=-s pytest").name, "pytest");
    assert_eq!(parse("pytest -p no:capture").name, "pytest");
}

/// Guard: settings that do not turn capture off keep the pytest view.
#[test]
fn pytest_view_stays_without_a_live_output_setting() {
    for cmd in [
        "pytest",
        "pytest -p xdist",
        "pytest -p no:cacheprovider",
        "pytest -v -p no:randomly -k capture",
        "pytest -kp",
        "pytest --capture=fd",
        "PYTEST_TIMEOUT=5 pytest",
    ] {
        assert_eq!(parse(cmd).runners, vec![Pytest], "{cmd}");
        assert_eq!(run(B, cmd).shaper, Some(Shaper::Pytest), "{cmd}");
    }
}
