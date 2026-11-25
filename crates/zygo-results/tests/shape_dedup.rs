use std::path::PathBuf;
use zygo_results::shape::dedup::{dedup, expand};

const W: &str = "warning: warnbuild@0.1.0: libfoo not found; using the bundled copy";

/// Inputs that dedup changes, without a trailing newline.
const BUILT: &[&str] = &[
    "x\nx\nx",
    "a\nx\nx\nx\nb",
    "x\nx\nx\nx\nx\nx\nx\nx\nx\nx\nx\nx",
    "x\nx\nx\ny\ny\ny\ny",
    "a\n\n\n\nx\nx\nx\nx\nx\ny\ny",
];

fn fixture(case: &str, file: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/shaping/dedup")
        .join(case);
    String::from_utf8(std::fs::read(p.join(file)).unwrap()).unwrap()
}

#[test]
fn three_equal_lines_become_one_counted() {
    assert_eq!(
        dedup("a\nx\nx\nx\nb\n").as_deref(),
        Some("a\nx [repeated 3 times]\nb\n")
    );
}

#[test]
fn count_is_the_run_length_in_decimal() {
    assert_eq!(
        dedup(&"x\n".repeat(12)).as_deref(),
        Some("x [repeated 12 times]\n")
    );
    assert_eq!(
        dedup("x\nx\nx\ny\ny\ny\ny\n").as_deref(),
        Some("x [repeated 3 times]\ny [repeated 4 times]\n")
    );
}

#[test]
fn two_equal_lines_stay() {
    assert_eq!(dedup("x\nx\n"), None);
    assert_eq!(dedup("x\nx\ny\ny\n"), None);
}

#[test]
fn three_equal_blank_lines_stay() {
    for b in ["", " ", "\t", " \t "] {
        assert_eq!(dedup(&format!("a\n{b}\n{b}\n{b}\nc\n")), None, "{b:?}");
    }
}

#[test]
fn text_already_counted_gives_none() {
    assert_eq!(dedup("poll: start\nq\nq\nq\nq [repeated 4 times]\n"), None);
    assert_eq!(dedup("z [repeated 4 times]\nx\nx\nx\n"), None);
}

#[test]
fn equal_lines_not_consecutive_stay() {
    assert_eq!(dedup("x\ny\nx\ny\nx\ny\n"), None);
    assert_eq!(dedup("x\nx\ny\nx\n"), None);
}

#[test]
fn last_run_without_trailing_newline() {
    assert_eq!(
        dedup("a\nx\nx\nx").as_deref(),
        Some("a\nx [repeated 3 times]")
    );
}

#[test]
fn expand_gives_each_built_input_back() {
    for x in BUILT {
        for x in [x.to_string(), format!("{x}\n")] {
            assert_eq!(expand(&dedup(&x).unwrap()), x, "{x:?}");
        }
    }
}

#[test]
fn fixture_build_repeated_warning() {
    let raw = fixture("build-repeated-warning", "stderr");
    let out = dedup(&raw).unwrap();
    assert_eq!(out.lines().count(), 3);
    assert!(out.lines().any(|l| l == format!("{W} [repeated 40 times]")));
    assert_eq!(expand(&out), raw);
}

#[test]
fn fixture_already_repeated_gives_none() {
    assert_eq!(dedup(&fixture("already-repeated", "stdout")), None);
}
