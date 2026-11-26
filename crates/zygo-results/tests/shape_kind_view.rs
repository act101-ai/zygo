use std::path::PathBuf;
use zygo_results::kind::{sniff, tree};
use zygo_results::shape::kind_view::kind_view;
use zygo_results::shape::{self, ShapeInput, Shaper, Step, SHAPE_MAX_BYTES};
use zygo_results::view::{shaped_view, SHAPED_MARKER_ROOM, VIEW_CAP_BYTES};
use zygo_results::{Kind, Stored};

/// `stdout` and the `must_keep` lines of an OS-18 case.
fn case(dir: &str, name: &str) -> (String, Vec<String>) {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/shaping").join(dir).join(name);
    let read = |f: &str| String::from_utf8(std::fs::read(p.join(f)).unwrap()).unwrap();
    let keep: Vec<String> = read("must_keep").lines().map(String::from).collect();
    assert!(keep.iter().all(|l| !l.is_empty()), "{name}: an empty must_keep line is invalid");
    (read("stdout"), keep)
}

/// O§8.2: a `must_keep` line holds when a view line equals it, or, in the log view, equals `<n>: ` and it.
fn holds(view: &str, line: &str, log: bool) -> bool {
    let numbered = |x: &str| {
        x.split_once(": ").is_some_and(|(n, rest)| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) && rest == line)
    };
    !line.is_empty() && view.lines().any(|x| x == line || (log && numbered(x)))
}

/// A git file section larger than the 8 KiB cap, so it is never shown.
fn big_git(path: &str) -> String {
    format!("diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n@@ -1 +1,2000 @@\n{}", "+line\n".repeat(2000))
}

/// The result_tree outline of a JSON text (`kind.rs:89`).
fn outline(json: &str) -> String {
    let s = Stored { id: String::new(), tool: "Bash".into(), bytes: 0, lines: 0, kind: Kind::Json, created_at: 0, session: None, data: json.as_bytes().to_vec() };
    tree(&s).unwrap()
}

fn nl<S: AsRef<str>>(v: &[S]) -> String {
    v.iter().map(|l| format!("{}\n", l.as_ref())).collect()
}

const S1: &str = "diff --git a/a.txt b/a.txt\nindex 1111111..2222222 100644\n--- a/a.txt\n+++ b/a.txt\n@@ -1,2 +1 @@\n-x\n--- y\n+z\n";
const S2: &str = "diff --git a/b.txt b/b.txt\nnew file mode 100644\n--- /dev/null\n+++ b/b.txt\n@@ -0,0 +1,2 @@\n+p\n+q\n";
const S3: &str = "diff --git a/c.txt b/c.txt\ndeleted file mode 100644\n--- a/c.txt\n+++ /dev/null\n@@ -1,3 +0,0 @@\n-1\n-2\n-3\n";
const S4: &str = "diff --git a/img.png b/img.png\nBinary files a/img.png and b/img.png differ\n";
const LIST: &str = "a.txt +1 -2\nb.txt +2 -0\nc.txt +0 -3\nimg.png +0 -0\n";

#[test]
fn cap_is_8_kib() {
    assert_eq!(VIEW_CAP_BYTES, 8192);
}

#[test]
fn fixture_cases_fit_the_cap_and_keep_must_keep_lines() {
    for (dir, name, kind, step, shaper) in [
        ("diff", "git-diff-repo", Kind::Diff, Step::DiffFilesListed, Shaper::Diff),
        ("json", "cargo-metadata", Kind::Json, Step::JsonOutlined, Shaper::Json),
        ("log", "errors-in-middle", Kind::Log, Step::LogErrorsKept, Shaper::Log),
    ] {
        let (raw, keep) = case(dir, name);
        assert_eq!(sniff(raw.as_bytes()), kind, "{name}");
        let (v, s, k) = kind_view(&raw, kind, VIEW_CAP_BYTES).expect(name);
        assert!(v.len() <= 8192, "{name}: {}", v.len());
        assert_eq!((s, k), (step, shaper), "{name}");
        for l in &keep {
            assert!(holds(&v, l, kind == Kind::Log), "{name}: {l}");
        }
    }
}

#[test]
fn diff_fixture_lists_files_then_sections_then_marker() {
    let (raw, keep) = case("diff", "git-diff-repo");
    let v = kind_view(&raw, Kind::Diff, VIEW_CAP_BYTES).unwrap().0;
    let cut = raw.find("diff --git a/tests/results.rs").unwrap();
    let list = nl(&["Cargo.toml +1 -0", "src/kind.rs +1 -0", "src/lib.rs +1 -0", "src/shape/mod.rs +1 -0", "src/store.rs +1 -0", "src/sweep.rs +1 -0", "src/view.rs +1 -0", "tests/results.rs +586 -586"]);
    assert_eq!(v, format!("{list}{}[zygo: 1 file sections not shown]\n", &raw[..cut]));
    for l in &keep {
        assert!(holds(&v, l, false), "{l}");
    }
}

#[test]
fn diff_view_keeps_whole_sections_in_order_while_they_fit() {
    let t = [S1, S2, S3, S4].concat();
    let w = format!("{LIST}{S1}{S2}[zygo: 2 file sections not shown]\n");
    assert_eq!(kind_view(&t, Kind::Diff, w.len()), Some((w.clone(), Step::DiffFilesListed, Shaper::Diff)));
    assert_eq!(kind_view(&t, Kind::Diff, w.len() - 1), Some((format!("{LIST}{S1}[zygo: 3 file sections not shown]\n"), Step::DiffFilesListed, Shaper::Diff)));
    // The first section that does not fit ends the walk; a later, smaller one is not tried.
    let big = format!("diff --git a/b.txt b/b.txt\n--- a/b.txt\n+++ b/b.txt\n@@ -1 +1,200 @@\n{}", "+line\n".repeat(200));
    let t2 = [S1, &big, S3].concat();
    let w2 = format!("a.txt +1 -2\nb.txt +200 -0\nc.txt +0 -3\n{S1}[zygo: 2 file sections not shown]\n");
    assert_eq!(kind_view(&t2, Kind::Diff, w2.len() + S3.len()).unwrap().0, w2);
}

#[test]
fn diff_whose_first_section_does_not_fit_gives_none() {
    let t = [S1, S2, S3, S4].concat();
    let w = format!("{LIST}{S1}[zygo: 3 file sections not shown]\n");
    assert!(kind_view(&t, Kind::Diff, w.len() - 1).is_none());
    assert!(kind_view(&t, Kind::Diff, w.len()).is_some());
}

#[test]
fn diff_without_git_headers_splits_at_minus_plus_pairs() {
    let t = "--- a/x\n+++ b/x\n@@ -1 +1 @@\n-1\n+2\n--- y.orig\t2026-01-01\n+++ y\t2026-01-01\n@@ -1 +1 @@\n-3\n+4\n";
    let big = format!("--- z\n+++ z\n@@ -1 +1,2000 @@\n{}", "+line\n".repeat(2000));
    assert_eq!(kind_view(&(t.to_string() + &big), Kind::Diff, 8192).unwrap().0, format!("x +1 -1\ny +1 -1\nz +2000 -0\n{t}[zygo: 1 file sections not shown]\n"));
}

#[test]
fn diff_crlf_section_stays_byte_equal() {
    let s = "diff --git a/w b/w\n--- a/w\n+++ b/w\n@@ -1 +1 @@\n-a\r\n+b\r\n";
    assert_eq!(kind_view(&(s.to_string() + &big_git("z")), Kind::Diff, 8192).unwrap().0, format!("w +1 -1\nz +2000 -0\n{s}[zygo: 1 file sections not shown]\n"));
}

#[test]
fn json_fixture_is_the_outline() {
    let (raw, _) = case("json", "cargo-metadata");
    let o = outline(&raw);
    assert!(o.len() + 1 <= 4096, "{}", o.len());
    assert_eq!(kind_view(&raw, Kind::Json, VIEW_CAP_BYTES).unwrap().0, format!("{o}\n"));
}

#[test]
fn json_outline_is_cut_to_half_the_cap_then_text_lines_fill() {
    let mut t = String::from("{\n");
    for i in 0..1000 {
        t += &format!("  \"k{i:03}\": 0{}\n", if i < 999 { "," } else { "" });
    }
    t += "}\n";
    let want = (0..315).map(|i| format!("k{i:03}: number\n")).collect::<String>() + "{\n" + &(0..315).map(|i| format!("  \"k{i:03}\": 0,\n")).collect::<String>();
    assert_eq!(want.len(), 8192);
    assert_eq!(kind_view(&t, Kind::Json, 8192), Some((want, Step::JsonOutlined, Shaper::Json)));
}

#[test]
fn json_parse_error_and_deep_array_give_none() {
    assert!(kind_view("{\"a\": 1,\n", Kind::Json, 8192).is_none());
    assert!(kind_view(&("[".repeat(10_000) + &"]".repeat(10_000)), Kind::Json, 8192).is_none());
}

#[test]
fn json_with_no_line_that_fits_gives_none() {
    assert!(kind_view("{\"a\": 1}\n", Kind::Json, 4).is_none());
}

#[test]
fn log_fixture_keeps_head_level_lines_and_tail() {
    let (raw, _) = case("log", "errors-in-middle");
    let r: Vec<&str> = raw.lines().collect();
    assert_eq!(r.len(), 420);
    let want = nl(&r[..20]) + &[175, 180, 190, 205, 220, 230, 235, 250].iter().map(|i| format!("{i}: {}\n", r[i - 1])).collect::<String>() + &nl(&r[400..]);
    assert_eq!(kind_view(&raw, Kind::Log, VIEW_CAP_BYTES).unwrap().0, want);
}

/// 60 log lines; the odd lines test each level and start form.
fn log60() -> Vec<String> {
    (1..=60)
        .map(|i| match i {
            5 => "2026-11-25 10:05:00 ERROR in head".to_string(),
            30 => "2026-11-25 10:30:00 ERROR boom".to_string(),
            31 => "2026-11-25 10:31:00 FATAL db gone".to_string(),
            32 => "[CRITICAL] disk".to_string(),
            33 => "2026-11-25 10:33:00 error lower case".to_string(),
            34 => "2026-11-25 10:34:00 INFO 3 ERRORS seen".to_string(),
            35 => "panic: index out of range".to_string(),
            36 => "W WARN slow".to_string(),
            37 => "2026-11-25 10:37:00 WARNING retry".to_string(),
            38 => "  panic inside".to_string(),
            40 => "Traceback (most recent call last):".to_string(),
            55 => "2026-11-25 10:55:00 ERROR in tail".to_string(),
            _ => format!("2026-11-25 10:{i:02}:00 INFO step {i}"),
        })
        .collect()
}

#[test]
fn log_view_keeps_each_level_and_start_form() {
    let l = log60();
    let text = nl(&l);
    let want = nl(&l[..20]) + &[30, 31, 32, 35, 36, 37, 40].iter().map(|i| format!("{i}: {}\n", l[i - 1])).collect::<String>() + &nl(&l[40..]);
    assert_eq!(kind_view(&text, Kind::Log, 8192), Some((want, Step::LogErrorsKept, Shaper::Log)));
}

#[test]
fn log_level_lines_stop_at_the_first_that_does_not_fit() {
    let l = log60();
    let text = nl(&l);
    let ht = nl(&l[..20]) + &nl(&l[40..]);
    let line30 = "30: 2026-11-25 10:30:00 ERROR boom\n";
    let want = nl(&l[..20]) + line30 + &nl(&l[40..]);
    assert_eq!(kind_view(&text, Kind::Log, ht.len() + line30.len()).unwrap().0, want);
    assert!(kind_view(&text, Kind::Log, ht.len() - 1).is_none());
}

#[test]
fn log_of_40_lines_gives_none() {
    assert!(kind_view(&nl(&log60()[..40]), Kind::Log, 8192).is_none());
}

#[test]
fn other_kinds_give_none() {
    let (raw, _) = case("log", "errors-in-middle");
    for k in [Kind::Jsonl, Kind::Tree, Kind::Text] {
        assert!(kind_view(&raw, k, VIEW_CAP_BYTES).is_none(), "{k:?}");
    }
}

#[test]
fn diff_view_not_smaller_than_text_gives_none() {
    // R1, O§7.3 "No step adds content"
    assert!(kind_view(S1, Kind::Diff, 8192).is_none());
}

#[test]
fn json_view_not_smaller_than_text_gives_none() {
    assert!(kind_view("{\n  \"a\": 1,\n  \"b\": [1, 2]\n}\n", Kind::Json, 8192).is_none());
}

#[test]
fn log_view_not_smaller_than_text_gives_none() {
    let t: String = (1..=60).map(|i| format!("2026-11-25 10:{i:02}:00 ERROR boom {i}\n")).collect();
    assert!(kind_view(&t, Kind::Log, 8192).is_none());
}

#[test]
fn plain_diff_minus_plus_lines_inside_a_hunk_are_body_lines() {
    // R2: `--- a` / `+++ b` are hunk lines while the `@@ -1,3 +1,3 @@` counts have lines left
    let t = "--- a/x\n+++ b/x\n@@ -1,3 +1,3 @@\n ctx\n--- a\n+++ b\n ctx2\n";
    let big = format!("--- z\n+++ z\n@@ -1 +1,2000 @@\n{}", "+line\n".repeat(2000));
    let v = kind_view(&(t.to_string() + &big), Kind::Diff, 8192).unwrap().0;
    assert_eq!(v, format!("x +1 -1\nz +2000 -0\n{t}[zygo: 1 file sections not shown]\n"));
    assert!(!v.lines().any(|l| l.starts_with("b +")));
}

#[test]
fn must_keep_rule_needs_a_whole_line() {
    // R3, O§8.2
    assert!(!holds("diff --git a/x b/x.orig\n", "diff --git a/x b/x", false));
    assert!(holds("diff --git a/x b/x\n", "diff --git a/x b/x", false));
    assert!(holds("12: E boom\n", "E boom", true));
    assert!(!holds("12: E boom\n", "E boom", false));
    assert!(!holds("x12: E boom\n", "E boom", true));
    assert!(!holds("\n", "", false));
}

#[test]
fn empty_text_gives_none_for_each_kind() {
    for k in [Kind::Json, Kind::Jsonl, Kind::Diff, Kind::Log, Kind::Tree, Kind::Text] {
        assert!(kind_view("", k, VIEW_CAP_BYTES).is_none(), "{k:?}");
    }
}

#[test]
fn text_sniffed_as_json_that_does_not_parse_gives_none() {
    let t = format!("{{\"a\": \"{}\n", "x".repeat(5000));
    assert_eq!(sniff(t.as_bytes()), Kind::Json);
    assert!(kind_view(&t, Kind::Json, VIEW_CAP_BYTES).is_none());
}

#[test]
fn diff_of_300_files_gives_none() {
    // R4: the file list alone passes the cap, so no section fits (current behavior, O§7.7)
    let t: String = (0..300)
        .map(|i| format!("diff --git a/src/some/deep/module/path/file_{i:03}.rs b/src/some/deep/module/path/file_{i:03}.rs\n--- a/src/some/deep/module/path/file_{i:03}.rs\n+++ b/src/some/deep/module/path/file_{i:03}.rs\n@@ -1 +1 @@\n-a\n+b\n"))
        .collect();
    assert!(kind_view(&t, Kind::Diff, VIEW_CAP_BYTES).is_none());
}

/// A stored raw text with the largest numbers the shaped path allows (raw ≤ SHAPE_MAX_BYTES, 7 digits) and a 26-character id.
fn largest_stored() -> Stored {
    Stored { id: "0".repeat(26), tool: "Bash".into(), bytes: SHAPE_MAX_BYTES as u64, lines: SHAPE_MAX_BYTES as u64, kind: Kind::Text, created_at: 0, session: None, data: vec![] }
}

#[test]
fn shaped_marker_room_is_the_longest_marker() {
    // O§7.7, O§7.8: every step once (shape::run pushes each at most once), 7-digit numbers, a 26-character id
    assert_eq!(shaped_view(&Step::ALL, "", &largest_stored()).len(), SHAPED_MARKER_ROOM);
}

/// `n` distinct lines `2026-11-25 10:00:00 ERROR line <i:05>`, 37 bytes each with the `\n`; sniffed as log.
fn error_log(n: usize) -> String {
    (1..=n).map(|i| format!("2026-11-25 10:00:00 ERROR line {i:05}\n")).collect()
}

#[test]
fn run_leaves_room_for_the_shaped_marker() {
    // O§7.7: the kind view stays within VIEW_CAP_BYTES less the largest shaped marker
    let raw = error_log(1000);
    assert_eq!(raw.len(), 37_000);
    assert_eq!(sniff(raw.as_bytes()), Kind::Log);
    let full = kind_view(&raw, Kind::Log, VIEW_CAP_BYTES).unwrap().0;
    assert!(full.len() + SHAPED_MARKER_ROOM > VIEW_CAP_BYTES, "{}", full.len()); // within one marker length of 8 KiB
    let out = shape::run(&raw, &ShapeInput { parsed: None, kind: Kind::Log, threshold: 16384 });
    assert_eq!(out.text, kind_view(&raw, Kind::Log, VIEW_CAP_BYTES - SHAPED_MARKER_ROOM).unwrap().0);
    assert_eq!((out.steps.clone(), out.shaper, out.size_view), (vec![Step::LogErrorsKept], Some(Shaper::Log), false));
    assert!(shaped_view(&Step::ALL, &out.text, &largest_stored()).len() <= VIEW_CAP_BYTES);
}
