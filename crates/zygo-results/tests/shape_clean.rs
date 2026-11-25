use std::path::PathBuf;
use zygo_results::shape::{
    Step::{self, *},
    clean::clean,
};

/// Runs clean and checks the line count on every input (acceptance: lines kept).
fn run(input: &str, reuse: bool) -> Option<(String, Vec<Step>)> {
    let r = clean(input, reuse);
    if let Some((out, _)) = &r {
        assert_eq!(out.matches('\n').count(), input.matches('\n').count(), "lines: {input:?}");
    }
    r
}

fn got(input: &str, reuse: bool) -> (String, Vec<Step>) {
    run(input, reuse).unwrap_or_else(|| panic!("no change: {input:?}"))
}

fn color(input: &str, want: &str) {
    for reuse in [false, true] {
        assert_eq!(got(input, reuse), (want.to_string(), vec![ColorCodesRemoved]), "{input:?} reuse={reuse}");
    }
}

fn fixture(case: &str, file: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/shaping/clean").join(case);
    String::from_utf8(std::fs::read(p.join(file)).unwrap()).unwrap()
}

#[test]
fn step_values_are_the_spec_names() {
    assert_eq!(ColorCodesRemoved.as_str(), "color codes removed");
    assert_eq!(ProgressLinesRemoved.as_str(), "progress lines removed");
}

#[test]
fn csi_is_removed() {
    color("a\x1b[1;31mred\x1b[0m b", "ared b");
    color("x\x1b[?25l\x1b[2 qy", "xy"); // parameter 0x3F, intermediate 0x20
}

#[test]
fn eight_bit_csi_is_removed() {
    color("a\u{9b}31mb\u{9b}0m", "ab");
}

#[test]
fn osc_ended_by_bel_is_removed() {
    color("\x1b]0;title\x07ok", "ok");
}

#[test]
fn osc_ended_by_esc_backslash_is_removed() {
    color("\x1b]2;title\x1b\\ok", "ok");
}

#[test]
fn osc8_keeps_visible_text() {
    color("see \x1b]8;;https://example.com\x1b\\link\x1b]8;;\x1b\\ here", "see link here");
    color("see \x1b]8;;https://example.com\x07link\x1b]8;;\x07 here", "see link here");
}

#[test]
fn osc_over_4k_with_no_end_loses_only_esc_bracket() {
    let body = "x".repeat(5000);
    color(&format!("a\x1b]0;{body}\nb"), &format!("a0;{body}\nb"));
    color(&format!("a\x1b]0;{body}"), &format!("a0;{body}"));
}

#[test]
fn lone_trailing_esc_is_removed() {
    color("done\x1b", "done");
}

#[test]
fn other_esc_and_next_char_are_removed() {
    color("a\x1b7b\x1b8c\x1bMd", "abcd");
}

#[test]
fn crlf_becomes_lf() {
    assert_eq!(got("a\r\nb", false), ("a\nb".into(), vec![ProgressLinesRemoved]));
}

#[test]
fn carriage_return_keeps_text_after_last() {
    assert_eq!(got("10%\r50%\r100%", false), ("100%".into(), vec![ProgressLinesRemoved]));
    assert_eq!(
        got("start\n10%\r50%\r100%\nend\n", false),
        ("start\n100%\nend\n".into(), vec![ProgressLinesRemoved])
    );
}

#[test]
fn reuse_keeps_each_carriage_return() {
    assert_eq!(
        got("\x1b[32m+a\x1b[m\r\n10%\r100%\r\n", true),
        ("+a\r\n10%\r100%\r\n".into(), vec![ColorCodesRemoved])
    );
    assert_eq!(got("a\x1b\r\nb", true), ("a\r\nb".into(), vec![ColorCodesRemoved])); // Decision 2
    assert_eq!(run("a\r\nb", true), None);
    assert_eq!(run("10%\r50%\r100%", true), None);
}

#[test]
fn both_steps_in_order() {
    assert_eq!(
        got("\x1b[32mok\x1b[0m\r\n", false),
        ("ok\n".into(), vec![ColorCodesRemoved, ProgressLinesRemoved])
    );
}

#[test]
fn escapes_go_before_carriage_returns() {
    // Decision 1, diff fixture form
    assert_eq!(
        got("+x\x1b[41m\r\x1b[m\n", false),
        ("+x\n".into(), vec![ColorCodesRemoved, ProgressLinesRemoved])
    );
}

#[test]
fn an_escape_never_takes_a_newline() {
    // Decision 2
    color("a\x1b\nb", "a\nb");
    color("\x1b]0;t\nx\x07", "0;t\nx\x07");
}

#[test]
fn nothing_to_clean_gives_none() {
    for reuse in [false, true] {
        assert_eq!(run("plain\ntext\n", reuse), None);
        assert_eq!(run("", reuse), None);
        assert_eq!(run("a\u{9b}\n", reuse), None); // Decision 4
    }
}

#[test]
fn fixture_clone_progress() {
    let raw = fixture("git-clone-progress", "stderr");
    let (out, steps) = got(&raw, false);
    assert_eq!(steps, vec![ProgressLinesRemoved]);
    assert!(!out.contains('\r'));
    for l in fixture("git-clone-progress", "must_keep").lines() {
        assert!(out.lines().any(|o| o == l), "{l}");
    }
}

#[test]
fn fixture_diff_reuse_changes_only_escapes() {
    let raw = fixture("git-diff-color-crlf", "stdout");
    let (out, steps) = got(&raw, true);
    assert_eq!(steps, vec![ColorCodesRemoved]);
    let sgr = regex::Regex::new("\x1b\\[[0-9;]*m").unwrap();
    assert_eq!(out, sgr.replace_all(&raw, ""));
    assert_eq!(out.matches('\r').count(), raw.matches('\r').count());
}

#[test]
fn fixture_cargo_color_has_no_esc() {
    let raw = fixture("cargo-test-color", "stdout");
    let (out, steps) = got(&raw, false);
    assert_eq!(steps, vec![ColorCodesRemoved]);
    assert!(!out.contains('\x1b'));
    for l in fixture("cargo-test-color", "must_keep").lines() {
        assert!(out.lines().any(|o| o == l), "{l}");
    }
}
