use std::path::Path;
use std::time::{Duration, SystemTime};
use zygo_config::ResultsConfig;
use zygo_results::kind::{sniff, tree, Kind};
use zygo_results::{is_id, sweep, view, Store, Stored};
use zygo_results::shape::Step;

fn put(tool: &str, text: &str) -> (tempfile::TempDir, Stored) {
    let t = tempfile::tempdir().unwrap();
    let s = Store::put(&t.path().join("results"), tool, None, text.as_bytes()).unwrap();
    (t, s)
}
fn rows(n: usize) -> String { (1..=n).map(|i| format!("row {i} payload\n")).collect() }

#[test]
fn kind_is_sniffed_from_the_first_4_kib() {
    assert_eq!(sniff(br#"{"a":1,"b":[1,2]}"#), Kind::Json);
    assert_eq!(sniff(b"{\"a\":1}\n{\"a\":2}\n"), Kind::Jsonl);
    assert_eq!(sniff(b"diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+b\n"), Kind::Diff);
    assert_eq!(sniff(b"2026-11-18T10:00:00Z INFO start\n2026-11-18T10:00:01Z WARN slow\n"), Kind::Log);
    assert_eq!(sniff(b"ERROR boom\nWARN careful\nplain line\n"), Kind::Log);
    assert_eq!(sniff("project\n├── src\n│   └── lib.rs\n└── Cargo.toml\n".as_bytes()), Kind::Tree);
    assert_eq!(sniff(b"/repo/a/b.rs\n/repo/a/c.rs\n/repo/d.rs\n"), Kind::Tree);
    assert_eq!(sniff(b"/repo/a.rs\n/repo/b.rs\nnote\n"), Kind::Tree);          // 2 of 3 lines: more than half
    assert_eq!(sniff(b"/repo/a.rs\nnote one\nnote two\n"), Kind::Text);        // 1 of 3
    assert_eq!(sniff(b"hello world\nsecond line\n"), Kind::Text);
    assert_eq!(sniff(b""), Kind::Text);
    assert_eq!(sniff(b"a b/c\nd e/f\n"), Kind::Text);                           // spaces: not a path list
}

#[test]
fn kind_json_larger_than_the_sample_is_still_json() {
    // one line, over 4 KiB: the sample ends inside the line; the json rule uses the whole sample
    let mut big = String::from("{\"k\":[");
    for i in 0..5000 { big.push_str(&format!("{i},")); }
    big.push_str("0]}");
    assert!(big.len() > 4096 && !big.contains('\n'));
    assert_eq!(sniff(big.as_bytes()), Kind::Json);
    // many short lines: the cut last line of the sample is ignored by the line-based jsonl rule
    let mut lines = String::new();
    for i in 0..400 { lines.push_str(&format!("{{\"n\":{i},\"pad\":\"xxxxxxxxxxxx\"}}\n")); }
    assert_eq!(sniff(lines.as_bytes()), Kind::Jsonl);
}

#[test]
fn kind_only_the_first_4_kib_counts() {
    let mut late = "x\n".repeat(2100);                                   // 4200 bytes
    late.push_str("diff --git a/x b/x\n");
    assert_eq!(sniff(late.as_bytes()), Kind::Text);
    let mut cut = "é".repeat(2047);                                      // 4094 bytes; the next char spans 4094..4096
    cut.push_str(&"é".repeat(5));
    assert_eq!(sniff(cut.as_bytes()), Kind::Text);                       // a cut inside a char does not panic
}

#[test]
fn store_put_writes_raw_bytes_and_the_meta_file() {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("results");                                  // does not exist yet
    let data = b"one\ntwo\nthree";
    let s = Store::put(&dir, "Bash", None, data).unwrap();
    assert!(is_id(&s.id) && s.id.len() == 26);
    assert_eq!(std::fs::read(dir.join(&s.id)).unwrap(), data);
    let meta: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.join(format!("{}.json", s.id))).unwrap()).unwrap();
    assert_eq!(meta["tool"], "Bash");
    assert_eq!(meta["bytes"], 13);
    assert_eq!(meta["lines"], 3);
    assert_eq!(meta["kind"], "text");
    assert!(meta["created_at"].as_u64().unwrap() > 0);
    assert_eq!(meta.as_object().unwrap().len(), 5);
    assert_eq!((s.bytes, s.lines, s.kind, s.tool.as_str()), (13, 3, Kind::Text, "Bash"));
    assert_eq!(Store::get(&dir, &s.id).unwrap(), s);
    let names: Vec<String> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(names.len(), 2, "no temp file is left behind: {names:?}");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for p in [dir.join(&s.id), dir.join(format!("{}.json", s.id))] {
            assert_eq!(std::fs::metadata(p).unwrap().permissions().mode() & 0o777, 0o600);
        }
    }
}

#[test]
fn store_put_ids_differ_and_lines_are_counted_like_str_lines() {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("results");
    let a = Store::put(&dir, "Bash", None, b"x").unwrap();
    let b = Store::put(&dir, "Bash", None, b"x").unwrap();
    assert_ne!(a.id, b.id);
    for (data, n) in [(&b""[..], 0), (b"a\n", 1), (b"a\nb", 2), (b"a\r\nb\r\n", 2), (b"\n\n", 2)] {
        assert_eq!(Store::put(&dir, "Bash", None, data).unwrap().lines, n, "{data:?}");
    }
    assert_eq!(Store::put(&dir, "Bash", None, br#"{"a":1}"#).unwrap().kind, Kind::Json);
}

#[test]
fn store_get_refuses_a_malformed_id_and_names_an_unknown_one() {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("results");
    Store::put(&dir, "Bash", None, b"x").unwrap();
    let other = ulid::Ulid::generate().to_string();
    assert!(is_id(&other));
    for bad in ["", "abc", "../x", "01ARZ3NDEKTSV4RRFFQ69G5FAV/../x", "01ARZ3NDEKTSV4RRFFQ69G5FAV.json", &other.to_lowercase(), &format!(" {other}")] {
        assert!(!is_id(bad), "{bad}");                                    // canonical uppercase Crockford only
        assert!(Store::get(&dir, bad).is_err(), "{bad}");
    }
    let e = Store::get(&dir, &other).unwrap_err().to_string();
    assert!(e.contains(&other), "{e}");
}

#[test]
fn store_get_without_a_meta_file_is_no_result() {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("results");
    let s = Store::put(&dir, "Bash", None, b"x").unwrap();
    std::fs::remove_file(dir.join(format!("{}.json", s.id))).unwrap();   // the bytes file alone is an unfinished entry
    let e = Store::get(&dir, &s.id).unwrap_err().to_string();
    assert!(e.contains("no result") && e.contains(&s.id), "{e}");
    let orphan = ulid::Ulid::generate().to_string();
    std::fs::write(dir.join(&orphan), b"y").unwrap();
    assert!(Store::get(&dir, &orphan).unwrap_err().to_string().contains("no result"));
}

fn marker(s: &Stored) -> String {
    format!(
        "[zygo trimmed this output: {} lines, {} bytes; only part of it is shown. The full output is saved as result {id}. To read the rest, call result_search, result_view or result_tree with id \"{id}\".]",
        s.lines, s.bytes, id = s.id
    )
}

#[test]
fn view_is_head_marker_tail() {
    let text = rows(200);
    let (_t, s) = put("Bash", &text);
    let m = format!("[zygo trimmed this output: 200 lines, {} bytes; only part of it is shown. The full output is saved as result {id}. To read the rest, call result_search, result_view or result_tree with id \"{id}\".]", text.len(), id = s.id);
    let want: Vec<String> = (1..=60).map(|i| format!("row {i} payload")).chain([m]).chain((181..=200).map(|i| format!("row {i} payload"))).collect();
    assert_eq!(view(&s, &ResultsConfig::default()), want.join("\n"));
}

#[test]
fn view_of_read_has_no_tail() {
    for tool in ["Read", "read"] {                                       // Claude Code `Read`, OpenCode `read`: matched without case
        let (_t, s) = put(tool, &rows(200));
        let v = view(&s, &ResultsConfig::default());
        assert!(v.ends_with(&marker(&s)), "{tool}: {v}");
        assert!(v.starts_with("row 1 payload\n") && v.contains("row 60 payload\n") && !v.contains("row 61 payload"));
        assert!(!v.contains("row 200 payload"));
    }
}

#[test]
fn view_follows_the_config_counts_and_never_repeats_a_line() {
    let cfg = ResultsConfig { head_lines: 2, tail_lines: 1, ..ResultsConfig::default() };
    let (_t, s) = put("Bash", &rows(10));
    assert_eq!(view(&s, &cfg), format!("row 1 payload\nrow 2 payload\n{}\nrow 10 payload", marker(&s)));
    let cfg = ResultsConfig { head_lines: 3, tail_lines: 5, ..ResultsConfig::default() };
    let (_t, s) = put("Bash", &rows(6));                                 // head and tail meet
    assert_eq!(view(&s, &cfg), format!("row 1 payload\nrow 2 payload\nrow 3 payload\n{}\nrow 4 payload\nrow 5 payload\nrow 6 payload", marker(&s)));
}

#[test]
fn view_cuts_lines_over_400_characters() {
    let text = format!("{}\n{}\n{}\n{}", "a".repeat(1000), "b".repeat(400), "é".repeat(401), rows(100));
    let (_t, s) = put("Bash", &text);
    let v = view(&s, &ResultsConfig::default());
    let l: Vec<&str> = v.lines().collect();
    assert_eq!(l[0], format!("{}…", "a".repeat(400)));
    assert_eq!(l[1], "b".repeat(400));
    assert_eq!(l[2], format!("{}…", "é".repeat(400)));
}

#[test]
fn view_never_exceeds_8_kib_and_keeps_the_marker() {
    let line = "x".repeat(399);                                         // 400 bytes with the newline
    let text: String = (0..260).map(|_| format!("{line}\n")).collect(); // 104000 bytes
    let (_t, s) = put("Bash", &text);
    let v = view(&s, &ResultsConfig::default());
    assert!(v.len() <= 8192, "{}", v.len());
    assert!(v.len() > 8192 - 400, "over-trimmed: {}", v.len());
    assert!(v.starts_with(&line) && v.ends_with(&marker(&s)));          // tail dropped first, then head from its end
    let wide: String = (0..260).map(|_| format!("{}\n", "é".repeat(400))).collect();   // 801 bytes a line
    let (_t, s) = put("Bash", &wide);
    let v = view(&s, &ResultsConfig::default());
    assert!(v.len() <= 8192 && v.contains(&marker(&s)));
}

#[test]
fn view_of_bytes_that_are_not_utf8_does_not_panic() {
    let t = tempfile::tempdir().unwrap();
    let s = Store::put(&t.path().join("results"), "Bash", None, b"ok\xff\xfe\nrest\n").unwrap();
    assert!(view(&s, &ResultsConfig::default()).contains('\u{fffd}'));
}

#[test]
fn shaped_view_is_the_marker_line_then_the_text() {
    let (_t, s) = put("Bash", &rows(200));                               // 200 lines, 3092 bytes
    let v = view::shaped_view(&[Step::ColorCodesRemoved, Step::RepeatedLinesCounted], "x", &s);
    assert_eq!(v, format!("[zygo shaped this output: color codes removed, repeated lines counted. The full output (200 lines, 3092 bytes) is saved as result {}; read it with result_view, result_search or result_tree.]\nx", s.id));
}

#[test]
fn shaped_view_lists_steps_in_order_and_keeps_the_raw_numbers() {
    let (_t, s) = put("Bash", &rows(200));
    let all = [Step::ColorCodesRemoved, Step::ProgressLinesRemoved, Step::RepeatedLinesCounted, Step::PassedTestsHidden,
        Step::BuildProgressHidden, Step::DiffFilesListed, Step::JsonOutlined, Step::LogErrorsKept];
    let shaped = "row 1 payload\nrow 200 payload\n";                    // shorter than the raw text
    let v = view::shaped_view(&all, shaped, &s);
    let (first, rest) = v.split_once('\n').unwrap();
    assert_eq!(first, format!("[zygo shaped this output: color codes removed, progress lines removed, repeated lines counted, passed tests hidden, build progress hidden, diff files listed, json outlined, log errors kept. The full output (200 lines, 3092 bytes) is saved as result {}; read it with result_view, result_search or result_tree.]", s.id));
    assert_eq!(rest, shaped);                                           // the text follows unchanged
}

#[test]
fn size_view_of_a_shorter_text_keeps_the_raw_numbers_within_the_cap() {
    let raw: String = (0..300).map(|_| format!("{}\n", "x".repeat(399))).collect();     // 300 lines, 120000 bytes
    let (_t, s) = put("Bash", &raw);
    let shaped: String = (0..260).map(|_| format!("{}\n", "y".repeat(399))).collect();  // 104000 bytes
    let v = view::size_view(&shaped, &s, &ResultsConfig::default());
    assert!(v.len() <= view::VIEW_CAP_BYTES, "{}", v.len());
    assert!(v.starts_with(&"y".repeat(399)) && !v.contains(&"x".repeat(399)));
    assert!(v.ends_with(&marker(&s)) && marker(&s).contains("300 lines, 120000 bytes"));
    let cfg = ResultsConfig { head_lines: 2, tail_lines: 1, ..ResultsConfig::default() };
    let (_t, s) = put("Bash", &rows(10));
    assert_eq!(view::size_view("a\nb\nc\nd\n", &s, &cfg), format!("a\nb\n{}\nd", marker(&s)));
}

#[test]
fn view_equals_size_view_of_the_stored_text() {
    for tool in ["Bash", "Read"] {
        let (_t, s) = put(tool, &rows(200));
        let cfg = ResultsConfig::default();
        assert_eq!(view(&s, &cfg), view::size_view(&s.text(), &s, &cfg), "{tool}");
    }
}

#[test]
fn find_range_returns_lines_first_to_last_inclusive() {
    let (_t, s) = put("Bash", &rows(1000));
    let want: Vec<String> = (61..=120).map(|i| format!("row {i} payload")).collect();
    assert_eq!(s.range(61, 120).unwrap(), want.join("\n"));
    assert_eq!(s.range(1, 400).unwrap().lines().count(), 400);
    assert_eq!(s.range(990, 1100).unwrap().lines().count(), 11);         // last is clamped to the line count
    assert_eq!(s.range(7, 7).unwrap(), "row 7 payload");
    let e = s.range(1, 401).unwrap_err();                                 // the spec's own limit
    assert!(e.contains("400"), "{e}");
    assert!(s.range(0, 5).is_err() && s.range(5, 4).is_err());           // malformed range
    assert_eq!(s.range(1001, 1002).unwrap(), "no lines in that range; the result has 1000 lines");
}

#[test]
fn find_search_gives_line_numbers_and_one_line_of_context() {
    let (_t, s) = put("Bash", "alpha\nbeta\nneedle one\ngamma\ndelta\nepsilon\nneedle two\nzeta\n");
    assert_eq!(s.search("needle").unwrap(), "2- beta\n3: needle one\n4- gamma\n--\n6- epsilon\n7: needle two\n8- zeta");
    let (_t, s) = put("Bash", "needle one\nmid\nneedle two\n");
    assert_eq!(s.search("needle").unwrap(), "1: needle one\n2- mid\n3: needle two");     // contexts meet: no separator
    assert_eq!(s.search("^mid$|needle t").unwrap(), "1- needle one\n2: mid\n3: needle two");
    assert_eq!(s.search("absent").unwrap(), "no match");
    assert!(s.search("(").is_err());
}

#[test]
fn find_search_stops_at_50_matches() {
    let text: String = (1..=60).map(|i| format!("needle {i}\n")).collect();
    let (_t, s) = put("Bash", &text);
    let out = s.search("needle").unwrap();
    assert_eq!(out.lines().filter(|l| l.contains(": needle")).count(), 50);
    assert!(out.contains("\n51- needle 51\n"), "{out}");
    assert_eq!(out.lines().last().unwrap(), "[more matches not shown; the limit is 50]");
}

#[test]
fn find_search_windows_a_long_line_on_its_first_match() {
    let y = "y".repeat(394);
    let (_t, s) = put("Bash", &format!("{}needle{}\n", "x".repeat(1000), "y".repeat(1000)));
    assert_eq!(s.search("needle").unwrap(), format!("1: …needle{y}…"));                  // 400 chars from the match, both sides cut
    let (_t, s) = put("Bash", &format!("needle{}\n", "x".repeat(1000)));
    assert_eq!(s.search("needle").unwrap(), format!("1: needle{}…", "x".repeat(394)));    // match at the start: cut on the right only
    let (_t, s) = put("Bash", &format!("{}needle\n", "x".repeat(1000)));
    assert_eq!(s.search("needle").unwrap(), format!("1: …{}needle", "x".repeat(394)));    // match at the end: window ends at the line end
    let (_t, s) = put("Bash", &format!("{}\nneedle\n", "z".repeat(1000)));
    assert_eq!(s.search("needle").unwrap(), format!("1- {}…\n2: needle", "z".repeat(400))); // a context line: first 400 characters
}

#[test]
fn tree_of_json_lists_keys_to_depth_3_in_source_order_with_array_lengths() {
    let (_t, s) = put("Bash", r#"{"n":1,"a":{"list":[1,2,3],"b":{"c":{"d":1}}},"items":[{"name":"x","id":1},{"id":2}],"e":{}}"#);
    assert_eq!(tree(&s).unwrap(), "n: number\na: object\n  list: array[3]\n  b: object\n    c: object\nitems: array[2]\n  []: object\n    name: string\n    id: number\ne: object");
    let (_t, s) = put("Bash", r#"[{"a":true},{"a":null}]"#);
    assert_eq!(tree(&s).unwrap(), "array[2]\n[]: object\n  a: bool");
}

#[test]
fn tree_of_a_diff_lists_its_files() {
    let d = "diff --git a/src/a.rs b/src/a.rs\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1 +1 @@\n-x\n+y\ndiff --git a/new.txt b/new.txt\nnew file mode 100644\n--- /dev/null\n+++ b/new.txt\n@@ -0,0 +1 @@\n+hi\ndiff --git a/gone.txt b/gone.txt\ndeleted file mode 100644\n--- a/gone.txt\n+++ /dev/null\n@@ -1 +0,0 @@\n-bye\n";
    let (_t, s) = put("Bash", d);
    assert_eq!(s.kind, Kind::Diff);
    assert_eq!(tree(&s).unwrap(), "src/a.rs\nnew.txt\ngone.txt");
}

#[test]
fn tree_of_a_tree_lists_its_directories() {
    let (_t, s) = put("Bash", "project\n├── src\n│   ├── lib.rs\n│   └── util\n│       └── mod.rs\n└── Cargo.toml\n");
    assert_eq!(s.kind, Kind::Tree);
    assert_eq!(tree(&s).unwrap(), "project/\n  src/\n    util/");
    let (_t, s) = put("Glob", "/repo/src/a.rs\n/repo/src/b.rs\n/repo/docs/c.md\n/repo/d.rs\n");
    assert_eq!(s.kind, Kind::Tree);
    assert_eq!(tree(&s).unwrap(), "repo/\n  docs/\n  src/");
}

#[test]
fn tree_of_text_lists_headings_of_a_log_nothing_and_of_broken_json_an_error() {
    let (_t, s) = put("WebFetch", "# Title\ntext\n## Sub\nmore\n#nospace\n    # indented\n");
    assert_eq!(s.kind, Kind::Text);
    assert_eq!(tree(&s).unwrap(), "1: # Title\n3: ## Sub");
    let (_t, s) = put("Bash", "2026-11-18T10:00:00Z INFO start\n2026-11-18T10:00:01Z WARN slow\n");
    assert_eq!(s.kind, Kind::Log);
    assert_eq!(tree(&s).unwrap(), "");
    let (_t, s) = put("Bash", "{\"a\":1}\n{\"a\":2}\n");
    assert_eq!(s.kind, Kind::Jsonl);
    assert_eq!(tree(&s).unwrap(), "");                                    // jsonl: nothing (chosen)
    // sniffed as json (the first 4 KiB is a valid prefix) but the whole is malformed: the tail is `}}` after a comma
    let mut bad = String::from("{\"k\":[");
    for i in 0..2000 { bad.push_str(&format!("{i},")); }                  // about 8.9 KiB
    bad.push_str("}}");
    let (_t, s) = put("Bash", &bad);
    assert!(s.bytes > 4096);
    assert_eq!(s.kind, Kind::Json);
    assert!(tree(&s).is_err());
}

#[test]
fn tree_of_a_tree_with_one_very_deep_path_is_cut_at_the_stored_size() {
    // C5: the first 4 KiB sniffs as a path list; then one path line of 20,000 components
    let mut text = String::from("a/b/c\nd/e/f\n");
    while text.len() < 4200 { text.push_str("x/y/z\n"); }
    text.push_str(&"a/".repeat(20_000));
    text.push('\n');
    let (_t, s) = put("Bash", &text);
    assert_eq!(s.kind, Kind::Tree);
    let out = tree(&s).unwrap();
    let marker = "[tree cut: longer than the stored result]";
    assert!(out.len() as u64 <= s.bytes + marker.len() as u64, "{} > {} + {}", out.len(), s.bytes, marker.len());
    assert!(out.ends_with(&format!("\n{marker}")));
    assert!(out.starts_with("a/\n  a/\n    a/\n"));
}

const DAY: Duration = Duration::from_secs(24 * 60 * 60);
const HOUR: Duration = Duration::from_secs(60 * 60);

fn whole_now() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs())
}
fn set_mtime(p: &Path, t: SystemTime) {
    let mut o = std::fs::OpenOptions::new();
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        o.write(true).custom_flags(0x0200_0000);                          // FILE_FLAG_BACKUP_SEMANTICS: lets a directory be opened
    }
    #[cfg(not(windows))]
    o.read(true);
    o.open(p).unwrap().set_modified(t).unwrap();
}

/// <root>/sessions/<name>/results/f0 (mtime = now, as a late write would leave it); `token` only for a live session;
/// the session directory's own mtime = `changed` (what removing the other entries leaves at exit, §8.2 step 6).
fn session(root: &Path, name: &str, live: bool, changed: SystemTime) -> std::path::PathBuf {
    let d = root.join("sessions").join(name);
    let r = d.join("results");
    std::fs::create_dir_all(&r).unwrap();
    std::fs::write(r.join("f0"), "x").unwrap();
    set_mtime(&r.join("f0"), whole_now());
    if live { std::fs::write(d.join("token"), "t").unwrap(); }
    age(&d, changed);
    d
}

/// Sets the mtime of each direct entry of `dir`, then of `dir`, to `t` (the sweep ages a session by the newest of these).
fn age(dir: &Path, t: SystemTime) {
    for e in std::fs::read_dir(dir).unwrap() {
        set_mtime(&e.unwrap().path(), t);
    }
    set_mtime(dir, t);
}

/// Locks the existing `<dir>/pid` while the returned file lives, as the launcher holds it (N37 U3). Opening it for
/// write and locking it change no mtime; nothing is written through the lock (Windows locks are mandatory, N37 V2).
fn lock_pid(dir: &Path) -> std::fs::File {
    let f = std::fs::OpenOptions::new().write(true).open(dir.join("pid")).unwrap();
    fs4::FileExt::try_lock(&f).unwrap();
    f
}

#[test]
fn sweep_removes_results_24_hours_after_the_session_directory_changed() {
    let t = tempfile::tempdir().unwrap();
    let now = whole_now();
    let old = session(t.path(), "old", false, now - 25 * HOUR);
    let exact = session(t.path(), "exact", false, now - DAY);
    let young = session(t.path(), "young", false, now - 23 * HOUR);
    let future = session(t.path(), "future", false, now + 10 * HOUR);
    sweep(t.path(), &t.path().join("workspaces"), now);
    assert!(!old.exists(), "25 h: removed although results/f0 was written just now");
    assert!(!exact.exists(), "exactly 24 h counts as expired");
    assert!(young.join("results").join("f0").exists(), "23 h: kept");
    assert!(future.join("results").join("f0").exists(), "an mtime in the future is not expired");
}

#[test]
fn sweep_keeps_a_live_session_and_ignores_stray_entries() {
    let t = tempfile::tempdir().unwrap();
    let now = whole_now();
    let live = session(t.path(), "live", true, now - 72 * HOUR);
    std::fs::write(live.join("pid"), "1").unwrap();
    age(&live, now - 72 * HOUR);                                          // every entry 72 h old: only the lock keeps it
    let held = lock_pid(&live);
    std::fs::write(t.path().join("sessions").join("stray-file"), "x").unwrap();
    std::fs::create_dir_all(t.path().join("sessions").join("no-results")).unwrap();
    sweep(t.path(), &t.path().join("workspaces"), now);
    assert!(live.join("results").join("f0").exists() && live.join("token").exists(), "locked pid: live, kept");
    assert!(t.path().join("sessions").join("stray-file").exists());
    assert!(t.path().join("sessions").join("no-results").exists());     // changed just now
    let missing = t.path().join("missing");
    sweep(&missing, &missing.join("workspaces"), now);                    // no sessions directory: no panic
    drop(held);
}

#[test]
fn sweep_removes_a_session_whose_launcher_is_gone() {
    let t = tempfile::tempdir().unwrap();
    let now = whole_now();
    let crashed = session(t.path(), "crashed", true, now - 25 * HOUR);
    std::fs::write(crashed.join("pid"), "1").unwrap();                  // the launcher died: `pid` and `token` left, no lock
    std::fs::write(crashed.join("session.log"), "line\n").unwrap();
    age(&crashed, now - 25 * HOUR);
    let bare = t.path().join("sessions").join("bare");                  // died before any trim: no `results/`
    std::fs::create_dir_all(&bare).unwrap();
    std::fs::write(bare.join("token"), "t").unwrap();
    std::fs::write(bare.join("pid"), "1").unwrap();
    age(&bare, now - 25 * HOUR);
    let young = session(t.path(), "young", true, now - 23 * HOUR);
    std::fs::write(young.join("pid"), "1").unwrap();
    age(&young, now - 23 * HOUR);
    sweep(t.path(), &t.path().join("workspaces"), now);
    assert!(!crashed.exists(), "unlocked pid + token, 25 h: swept with its token");
    assert!(!bare.exists(), "a dead launcher's directory without results/ is swept too");
    assert!(young.join("token").exists(), "23 h: the age rule still applies");
}

#[test]
fn sweep_ages_a_session_by_its_newest_direct_entry() {
    let t = tempfile::tempdir().unwrap();
    let now = whole_now();
    let dead = |name: &str| {
        let d = session(t.path(), name, true, now - 25 * HOUR);
        std::fs::write(d.join("pid"), "1").unwrap();                     // a dead launcher: no lock
        std::fs::write(d.join("session.log"), "line\n").unwrap();
        age(&d, now - 25 * HOUR);
        d
    };
    let by_results = dead("by-results");
    let by_log = dead("by-log");
    let control = dead("control");
    let ended = session(t.path(), "ended", false, now - HOUR);          // ended normally: the dir was bumped at exit
    set_mtime(&ended.join("results"), now - 30 * HOUR);
    set_mtime(&by_results.join("results"), now - HOUR);                  // a result stored 1 h ago
    set_mtime(&by_results, now - 25 * HOUR);
    set_mtime(&by_log.join("session.log"), now - HOUR);                  // a log line 1 h ago
    set_mtime(&by_log, now - 25 * HOUR);
    sweep(t.path(), &t.path().join("workspaces"), now);
    assert!(by_results.join("token").exists(), "dir 25 h, results/ 1 h: kept");
    assert!(by_log.join("token").exists(), "dir 25 h, session.log 1 h: kept");
    assert!(ended.join("results").exists(), "dir 1 h, results/ 30 h: kept");
    assert!(!control.exists(), "every entry 25 h: swept");
}

#[cfg(unix)]
#[test]
fn sweep_does_not_follow_a_symlinked_session_or_results_directory() {
    let t = tempfile::tempdir().unwrap();
    let now = whole_now();
    let outside = t.path().join("outside");
    std::fs::create_dir_all(outside.join("results")).unwrap();
    std::fs::write(outside.join("results").join("f0"), "x").unwrap();
    std::fs::create_dir_all(t.path().join("sessions")).unwrap();
    std::os::unix::fs::symlink(&outside, t.path().join("sessions").join("link")).unwrap();
    let other = t.path().join("other");
    std::fs::create_dir_all(other.join("results")).unwrap();
    std::fs::write(other.join("results").join("f0"), "x").unwrap();
    let real = t.path().join("sessions").join("real");
    std::fs::create_dir_all(&real).unwrap();
    std::os::unix::fs::symlink(other.join("results"), real.join("results")).unwrap();
    sweep(t.path(), &t.path().join("workspaces"), now + 48 * HOUR);
    assert!(outside.join("results").join("f0").exists(), "the target of a symlinked session is kept");
    assert!(other.join("results").join("f0").exists(), "the target of a symlinked results is kept");
    assert!(real.join("results").symlink_metadata().is_ok(), "a session whose results is a symlink is skipped");
}

#[cfg(unix)]
fn pane_results(root: &Path, key: &str) -> std::path::PathBuf {
    let r = root.join("workspaces").join("0123456789abcdef").join("results").join(key);
    std::fs::create_dir_all(&r).unwrap();
    std::fs::write(r.join("f0"), "x").unwrap();
    r
}

/// A session folder with `token`, `pid` (no lock: the launcher has ended; with `lock_pid` it is live, because `live`
/// needs `token` and a held lock, `zygo-core/src/session_dir.rs:13-17`), `session.log`, and `results` as a symlink to
/// `target`. Every entry is new: its age comes from the `now` given to `sweep`.
#[cfg(unix)]
fn linked_session(root: &Path, name: &str, target: &Path) -> std::path::PathBuf {
    let d = root.join("sessions").join(name);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("token"), "t").unwrap();
    std::fs::write(d.join("pid"), "1").unwrap();
    std::fs::write(d.join("session.log"), "line\n").unwrap();
    std::os::unix::fs::symlink(target, d.join("results")).unwrap();
    d
}

#[cfg(unix)]
#[test]
fn au69_a_deletes_an_ended_linked_session_after_24_hours() {
    let t = tempfile::tempdir().unwrap();
    let target = pane_results(t.path(), "kaaaaaaaa");
    let s = linked_session(t.path(), "ended", &target);
    sweep(t.path(), &t.path().join("workspaces"), whole_now() + 25 * HOUR);
    assert!(s.symlink_metadata().is_err(), "an ended session linked into the workspace cache, 25 h: removed");
    assert_eq!(std::fs::read_to_string(target.join("f0")).unwrap(), "x", "the link target is never touched");
}

#[cfg(unix)]
#[test]
fn au69_a_keeps_an_ended_linked_session_younger_than_24_hours() {
    let t = tempfile::tempdir().unwrap();
    let target = pane_results(t.path(), "kaaaaaaaa");
    let s = linked_session(t.path(), "ended", &target);
    sweep(t.path(), &t.path().join("workspaces"), whole_now() + 23 * HOUR);
    assert!(s.join("results").symlink_metadata().is_ok_and(|m| m.file_type().is_symlink()), "23 h: kept");
    assert!(s.join("session.log").exists());
    assert_eq!(std::fs::read_to_string(target.join("f0")).unwrap(), "x");
}

#[cfg(unix)]
#[test]
fn au69_a_keeps_a_live_linked_session() {
    let t = tempfile::tempdir().unwrap();
    let target = pane_results(t.path(), "kaaaaaaaa");
    let s = linked_session(t.path(), "live", &target);
    let held = lock_pid(&s);
    sweep(t.path(), &t.path().join("workspaces"), whole_now() + 48 * HOUR);
    assert!(s.join("results").symlink_metadata().is_ok_and(|m| m.file_type().is_symlink()), "locked pid: live, kept");
    assert!(s.join("token").exists());
    assert_eq!(std::fs::read_to_string(target.join("f0")).unwrap(), "x");
    drop(held);
}

#[cfg(unix)]
#[test]
fn au69_a_keeps_a_session_linked_outside_the_workspace_cache() {
    let t = tempfile::tempdir().unwrap();
    let target = t.path().join("elsewhere").join("results");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(target.join("f0"), "x").unwrap();
    let s = linked_session(t.path(), "ended", &target);
    sweep(t.path(), &t.path().join("workspaces"), whole_now() + 48 * HOUR);
    assert!(s.join("results").symlink_metadata().is_ok(), "a link outside the workspace cache: kept");
    assert!(target.join("f0").exists());
}

/// M1 review round 1 (M1, D1): a link whose target is gone (after `zygo ws forget` or `forget-pane`) is decided by its
/// text, made absolute against the session folder: under the workspace cache, the AU69 (a) rule applies. Round 3 (F4):
/// only a link text with no `..` component is decided this way (the `..` cases moved to the next test).
#[cfg(unix)]
#[test]
fn au69_a_sweeps_an_ended_session_linked_to_a_missing_folder_in_the_workspace_cache() {
    let t = tempfile::tempdir().unwrap();
    let cache = t.path().join("workspaces");
    std::fs::create_dir_all(&cache).unwrap();
    let gone = cache.join("0123456789abcdef").join("results").join("kaaaaaaaa");
    let absolute = linked_session(t.path(), "absolute", &gone);
    let dotted = linked_session(t.path(), "dotted", &cache.join(".").join("0123456789abcdef").join(".").join("results"));
    sweep(t.path(), &cache, whole_now() + 3 * DAY);
    for s in [&absolute, &dotted] {
        assert!(s.symlink_metadata().is_err(), "{}: an ended session, 3 days, linked to a missing folder in the cache: removed", s.display());
    }
    assert!(cache.is_dir(), "the workspace cache is never touched");
}

/// M1 review round 1 (M1, D1): a link to a missing folder outside the workspace cache keeps the session folder. Round 3
/// (F4): a link text with a `..` component keeps the session folder, also when its lexical form is in the cache.
#[cfg(unix)]
#[test]
fn au69_a_keeps_an_ended_session_linked_to_a_missing_folder_outside_the_workspace_cache() {
    let t = tempfile::tempdir().unwrap();
    let cache = t.path().join("workspaces");
    std::fs::create_dir_all(&cache).unwrap();
    let outside = linked_session(t.path(), "outside", &t.path().join("elsewhere").join("results"));
    let escape = linked_session(t.path(), "escape", &cache.join("..").join("elsewhere").join("results"));
    let relative = linked_session(t.path(), "relative", Path::new("../../elsewhere/results"));
    let prefix = linked_session(t.path(), "prefix", &t.path().join("workspaces-other").join("results"));
    let up_in = linked_session(t.path(), "up-in", Path::new("../../workspaces/0123456789abcdef/results/kaaaaaaaa"));
    let dotted_in = linked_session(t.path(), "dotted-in", &cache.join("x").join("..").join("0123456789abcdef").join("results"));
    sweep(t.path(), &cache, whole_now() + 3 * DAY);
    for s in [&outside, &escape, &relative, &prefix, &up_in, &dotted_in] {
        assert!(s.join("results").symlink_metadata().is_ok_and(|m| m.file_type().is_symlink()), "{}: kept", s.display());
        assert!(s.join("token").exists() && s.join("pid").exists() && s.join("session.log").exists(), "{}", s.display());
    }
}

/// M1 review round 3 (F4, D1): the link text `<cache>/L/../x`, where `<cache>/L` is a symlink to a folder outside the
/// cache. Lexically it is `<cache>/x`; the kernel resolves it to `<outside parent>/x`. The `..` keeps the folder.
#[cfg(unix)]
#[test]
fn au69_a_keeps_a_session_whose_dangling_link_text_has_dot_dot_through_a_symlink() {
    let t = tempfile::tempdir().unwrap();
    let cache = t.path().join("workspaces");
    std::fs::create_dir_all(&cache).unwrap();
    let outside = t.path().join("elsewhere").join("deep");
    std::fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, cache.join("L")).unwrap();
    let s = linked_session(t.path(), "ended", &cache.join("L").join("..").join("x"));
    sweep(t.path(), &cache, whole_now() + 3 * DAY);
    assert!(s.join("results").symlink_metadata().is_ok_and(|m| m.file_type().is_symlink()), "a `..` in the link text: kept");
    assert!(s.join("token").exists() && s.join("pid").exists() && s.join("session.log").exists());
    assert!(outside.is_dir() && cache.join("L").symlink_metadata().is_ok());
}

/// M1 review round 3 (F4, D1): the workspace cache path is itself a symlink, and the dangling link text goes through
/// it (no `..`): its existing part made canonical is under the canonical cache, so the folder is swept.
#[cfg(unix)]
#[test]
fn au69_a_sweeps_a_dangling_link_under_a_symlinked_workspace_cache() {
    let t = tempfile::tempdir().unwrap();
    let real = t.path().join("real-cache");
    std::fs::create_dir_all(&real).unwrap();
    let cache = t.path().join("workspaces");
    std::os::unix::fs::symlink(&real, &cache).unwrap();
    let s = linked_session(t.path(), "ended", &cache.join("0123456789abcdef").join("results").join("kaaaaaaaa"));
    sweep(t.path(), &cache, whole_now() + 3 * DAY);
    assert!(s.symlink_metadata().is_err(), "a dangling link under a symlinked cache: removed");
    assert!(real.is_dir() && cache.symlink_metadata().is_ok_and(|m| m.file_type().is_symlink()), "the cache is never touched");
}

/// M1 review round 3 (F4, D1): a dangling link text with no `..` that goes through a symlinked parent is decided where
/// the kernel resolves it: through `<root>/alias` (a symlink to the cache) it is in the cache and swept; through
/// `<cache>/L` (a symlink to a folder outside) it is outside and kept.
#[cfg(unix)]
#[test]
fn au69_a_decides_a_dangling_link_through_a_symlinked_parent_where_it_resolves() {
    let t = tempfile::tempdir().unwrap();
    let cache = t.path().join("workspaces");
    std::fs::create_dir_all(&cache).unwrap();
    std::os::unix::fs::symlink(&cache, t.path().join("alias")).unwrap();
    let outside = t.path().join("elsewhere");
    std::fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, cache.join("L")).unwrap();
    let inside = linked_session(t.path(), "inside", &t.path().join("alias").join("0123456789abcdef").join("results"));
    let out = linked_session(t.path(), "out", &cache.join("L").join("0123456789abcdef").join("results"));
    sweep(t.path(), &cache, whole_now() + 3 * DAY);
    assert!(inside.symlink_metadata().is_err(), "through a symlink to the cache: removed");
    assert!(out.join("results").symlink_metadata().is_ok_and(|m| m.file_type().is_symlink()), "through a symlink to outside: kept");
    assert!(out.join("token").exists() && outside.is_dir());
}

/// Plan review F7 (D1): the dangling link text `<cache>/D/x`, where `<cache>/D` is a symlink whose target is missing.
/// `canonicalize` cannot resolve `<cache>/D`, so the walk cannot tell where the text points: the folder is kept. Kills a
/// walk that adds every missing part back as text (it would see `<cache>/D/x` under the cache and sweep).
#[cfg(unix)]
#[test]
fn au69_a_keeps_a_session_whose_dangling_link_goes_through_a_missing_inner_link() {
    let t = tempfile::tempdir().unwrap();
    let cache = t.path().join("workspaces");
    std::fs::create_dir_all(&cache).unwrap();
    std::os::unix::fs::symlink(t.path().join("elsewhere").join("gone"), cache.join("D")).unwrap();
    let s = linked_session(t.path(), "ended", &cache.join("D").join("x"));
    sweep(t.path(), &cache, whole_now() + 3 * DAY);
    assert!(s.join("results").symlink_metadata().is_ok_and(|m| m.file_type().is_symlink()), "a missing inner link: kept");
    assert!(s.join("token").exists());
    assert!(cache.join("D").symlink_metadata().is_ok_and(|m| m.file_type().is_symlink()), "the inner link stays");
}

/// M1 review round 1 (M2): the link first, the folder only when that worked. With a read-only session folder the link
/// cannot be removed, so the folder and every entry stay. Stated limit (D1, plan review F8): a read-only folder also
/// stops `remove_dir_all` of the other entries, so this test fixes the outcome but cannot tell the guarded removal from
/// an unguarded one; no fault stops only the unlink of the link.
#[cfg(unix)]
#[test]
fn au69_a_keeps_the_folder_when_the_link_cannot_be_removed() {
    use std::os::unix::fs::PermissionsExt;
    let t = tempfile::tempdir().unwrap();
    let target = pane_results(t.path(), "kaaaaaaaa");
    let s = linked_session(t.path(), "ended", &target);
    std::fs::set_permissions(&s, std::fs::Permissions::from_mode(0o555)).unwrap();
    if std::fs::write(s.join("probe"), "").is_ok() {
        // the mode does not stop this user (root): the fault cannot be made here
        std::fs::remove_file(s.join("probe")).unwrap();
        std::fs::set_permissions(&s, std::fs::Permissions::from_mode(0o755)).unwrap();
        eprintln!("au69_a_keeps_the_folder_when_the_link_cannot_be_removed: skipped, a read-only folder does not stop this user");
        return;
    }
    sweep(t.path(), &t.path().join("workspaces"), whole_now() + 48 * HOUR);
    std::fs::set_permissions(&s, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(s.join("results").symlink_metadata().is_ok_and(|m| m.file_type().is_symlink()), "the link stays");
    for f in ["token", "pid", "session.log"] {
        assert!(s.join(f).exists(), "{f} stays");
    }
    assert_eq!(std::fs::read_to_string(target.join("f0")).unwrap(), "x");
}

/// W§18 #61: AU69 (b) is dropped (D1); results have no age rule (FIX-20).
#[cfg(unix)]
#[test]
fn w18_61_the_sweep_deletes_no_workspace_result() {
    let t = tempfile::tempdir().unwrap();
    let state = t.path().join("state").join("workspaces").join("0123456789abcdef");
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(state.join("workspace.json"), "{").unwrap();     // a state that does not parse; the sweep reads none
    let linked = pane_results(t.path(), "kaaaaaaaa");
    let orphan = pane_results(t.path(), "kbbbbbbbb");
    set_mtime(&orphan.join("f0"), whole_now() - 30 * DAY);
    set_mtime(&orphan, whole_now() - 30 * DAY);
    linked_session(t.path(), "ended", &linked);
    sweep(t.path(), &t.path().join("workspaces"), whole_now() + 48 * HOUR);
    assert!(linked.join("f0").exists() && orphan.join("f0").exists(), "the sweep deletes no workspace result");
}

#[test]
fn store_get_with_a_raw_file_of_another_length_is_no_result() {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("results");
    let s = Store::put(&dir, "Bash", None, b"abc").unwrap();
    std::fs::write(dir.join(&s.id), b"abcd").unwrap();
    let e = Store::get(&dir, &s.id).unwrap_err().to_string();
    assert!(e.contains("no result") && e.contains(&s.id), "{e}");
}

#[cfg(unix)]
#[test]
fn store_put_creates_the_results_directory_with_mode_0700() {
    use std::os::unix::fs::PermissionsExt;
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("s").join("results");
    Store::put(&dir, "Bash", None, b"x").unwrap();
    assert_eq!(std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);
}

#[test]
fn session_put_with_some_is_in_the_meta_and_read_back_by_get() {     // O§6.5 item 1, O§6.4 Key
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("results");
    let s = Store::put(&dir, "Bash", Some("S1"), b"x").unwrap();
    assert_eq!(s.session.as_deref(), Some("S1"));
    let meta: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.join(format!("{}.json", s.id))).unwrap()).unwrap();
    assert_eq!(meta["session"], "S1");
    assert_eq!(meta.as_object().unwrap().len(), 6);
    assert_eq!(Store::get(&dir, &s.id).unwrap().session.as_deref(), Some("S1"));
    assert_eq!(Store::get(&dir, &s.id).unwrap(), s);
}

#[test]
fn session_absent_from_an_old_meta_file_reads_as_none() {           // the 97d8767 meta form
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("results");
    std::fs::create_dir_all(&dir).unwrap();
    let id = ulid::Ulid::generate().to_string();
    std::fs::write(dir.join(&id), b"x").unwrap();
    std::fs::write(dir.join(format!("{id}.json")), br#"{"tool":"Bash","bytes":1,"lines":1,"kind":"text","created_at":1700000000}"#).unwrap();
    let s = Store::get(&dir, &id).unwrap();
    assert_eq!(s.session, None);
    assert_eq!((s.tool.as_str(), s.bytes, s.lines, s.kind, s.created_at, s.data.as_slice()), ("Bash", 1, 1, Kind::Text, 1700000000, &b"x"[..]));
}

#[test]
fn session_none_writes_no_session_key() {                           // a None put keeps the 97d8767 meta shape
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("results");
    let s = Store::put(&dir, "Bash", None, b"x").unwrap();
    assert_eq!(s.session, None);
    let meta: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.join(format!("{}.json", s.id))).unwrap()).unwrap();
    assert!(meta.get("session").is_none(), "{meta}");
    assert_eq!(meta.as_object().unwrap().len(), 5);
    assert_eq!(Store::get(&dir, &s.id).unwrap().session, None);
}

#[test]
fn session_empty_is_no_session() {                                  // O§6.4: an empty id must not be a key
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("results");
    let s = Store::put(&dir, "Bash", Some(""), b"x").unwrap();
    assert_eq!(s.session, None);
    let meta: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.join(format!("{}.json", s.id))).unwrap()).unwrap();
    assert!(meta.get("session").is_none(), "{meta}");
    assert_eq!(meta.as_object().unwrap().len(), 5);
    assert_eq!(Store::get(&dir, &s.id).unwrap().session, None);
}

/// Writes the raw result `x` and the meta text `meta` under a new id in `dir`, and reads them back through `get`.
fn get_with_meta(dir: &Path, meta: &[u8]) -> Stored {
    std::fs::create_dir_all(dir).unwrap();
    let id = ulid::Ulid::generate().to_string();
    std::fs::write(dir.join(&id), b"x").unwrap();
    std::fs::write(dir.join(format!("{id}.json")), meta).unwrap();
    Store::get(dir, &id).unwrap()
}

#[test]
fn session_null_reads_as_none() {
    let t = tempfile::tempdir().unwrap();
    let s = get_with_meta(&t.path().join("results"),
        br#"{"tool":"Bash","bytes":1,"lines":1,"kind":"text","created_at":1700000000,"session":null}"#);
    assert_eq!(s.session, None);
    assert_eq!((s.tool.as_str(), s.bytes, s.lines, s.kind, s.created_at, s.data.as_slice()), ("Bash", 1, 1, Kind::Text, 1700000000, &b"x"[..]));
}

#[test]
fn session_meta_with_an_unknown_extra_key_still_loads() {           // O-C8: an older binary reads a newer meta
    let t = tempfile::tempdir().unwrap();
    let s = get_with_meta(&t.path().join("results"),
        br#"{"tool":"Bash","bytes":1,"lines":1,"kind":"text","created_at":1700000000,"session":"S1","future_field":1}"#);
    assert_eq!(s.session.as_deref(), Some("S1"));
    assert_eq!((s.tool.as_str(), s.bytes, s.lines, s.kind, s.created_at, s.data.as_slice()), ("Bash", 1, 1, Kind::Text, 1700000000, &b"x"[..]));
}

use zygo_config::Features;
use zygo_core::session_dir::SessionDir;
use zygo_core::Agent;
use zygo_mcp::{McpCtx, Tool, ToolError};

fn ctx(root: Option<&Path>) -> McpCtx {
    McpCtx { agent: Agent::ClaudeCode, features: Features::default(), session_dir: root.map(|r| SessionDir::new(r.to_path_buf())) }
}
fn by_name<'a>(t: &'a [Box<dyn Tool>], n: &str) -> &'a dyn Tool { t.iter().find(|x| x.def().name == n).unwrap().as_ref() }
fn call(t: &dyn Tool, args: serde_json::Value) -> Result<String, ToolError> {
    t.call(args).map(|v| v.as_str().expect("a string reply").to_string())
}

#[test]
fn tools_exist_inside_a_session_only() {
    let t = tempfile::tempdir().unwrap();
    let tools = zygo_mcp::tools::results::tools(&ctx(Some(t.path())));
    let names: Vec<String> = tools.iter().map(|x| x.def().name).collect();
    assert_eq!(names, ["result_view", "result_search", "result_tree"]);
    for (n, req) in [("result_view", vec!["id"]), ("result_search", vec!["id", "pattern"]), ("result_tree", vec!["id"])] {
        let d = by_name(&tools, n).def();
        assert_eq!(d.input_schema["type"], "object");
        assert_eq!(d.input_schema["required"], serde_json::json!(req));
        assert_eq!(d.input_schema["additionalProperties"], false);
        assert_eq!(d.input_schema["properties"]["id"]["type"], "string");
    }
    assert_eq!(by_name(&tools, "result_view").def().input_schema["properties"]["lines"]["type"], "string");
    assert_eq!(by_name(&tools, "result_search").def().input_schema["properties"]["pattern"]["type"], "string");
    assert!(zygo_mcp::tools::results::tools(&ctx(None)).is_empty());
}

#[test]
fn tools_acceptance_100_kib_bash_output_is_trimmed_and_the_three_tools_read_the_stored_bytes() {
    let t = tempfile::tempdir().unwrap();
    let sd = SessionDir::new(t.path().to_path_buf());
    let text: String = (1..).map(|i| format!("row {i} payload\n")).scan(0usize, |n, l| { if *n >= 100 * 1024 { None } else { *n += l.len(); Some(l) } }).collect();
    assert!(text.len() >= 100 * 1024);
    let s = Store::put(&sd.results(), "Bash", None, text.as_bytes()).unwrap();
    let v = view(&s, &ResultsConfig::default());
    assert!(v.len() <= 8192);
    assert!(v.contains(&format!("[zygo trimmed this output: {} lines, {} bytes; only part of it is shown. The full output is saved as result {id}. To read the rest, call result_search, result_view or result_tree with id \"{id}\".]", s.lines, text.len(), id = s.id)));
    let tools = zygo_mcp::tools::results::tools_with(sd.results(), None);
    let first_400: Vec<String> = (1..=400).map(|i| format!("row {i} payload")).collect();
    assert_eq!(call(by_name(&tools, "result_view"), serde_json::json!({"id": s.id})).unwrap(), first_400.join("\n"));
    let want: Vec<String> = (61..=120).map(|i| format!("row {i} payload")).collect();
    assert_eq!(call(by_name(&tools, "result_view"), serde_json::json!({"id": s.id, "lines": "61-120"})).unwrap(), want.join("\n"));
    let found = call(by_name(&tools, "result_search"), serde_json::json!({"id": s.id, "pattern": "row 500 "})).unwrap();
    assert_eq!(found, "499- row 499 payload\n500: row 500 payload\n501- row 501 payload");
    let j = Store::put(&sd.results(), "Bash", None, br#"{"b":{"a":[1,2]}}"#).unwrap();
    assert_eq!(call(by_name(&tools, "result_tree"), serde_json::json!({"id": j.id})).unwrap(), "b: object\n  a: array[2]");
}

#[test]
fn tools_bad_arguments_are_invalid_params_and_an_unknown_id_is_a_failure() {
    let t = tempfile::tempdir().unwrap();
    let sd = SessionDir::new(t.path().to_path_buf());
    let s = Store::put(&sd.results(), "Bash", None, b"a\nb\n").unwrap();
    let tools = zygo_mcp::tools::results::tools(&ctx(Some(t.path())));
    let (view_t, search_t, tree_t) = (by_name(&tools, "result_view"), by_name(&tools, "result_search"), by_name(&tools, "result_tree"));
    for args in [serde_json::json!({}), serde_json::json!({"id": 5}), serde_json::json!({"id": "../x"}), serde_json::json!({"id": s.id, "lines": "abc"}),
                 serde_json::json!({"id": s.id, "lines": "0-5"}), serde_json::json!({"id": s.id, "lines": "1-401"}), serde_json::json!({"id": s.id, "lines": 5}),
                 serde_json::json!({"id": s.id, "lines": "1-99999999999999999999"}), serde_json::json!({"id": s.id, "lines": "99999999999999999999-5"})] {
        assert!(matches!(call(view_t, args.clone()), Err(ToolError::InvalidParams(_))), "{args}");
    }
    assert!(matches!(call(search_t, serde_json::json!({"id": s.id})), Err(ToolError::InvalidParams(_))));
    assert!(matches!(call(search_t, serde_json::json!({"id": s.id, "pattern": "("})), Err(ToolError::InvalidParams(_))));
    assert!(matches!(call(tree_t, serde_json::json!({"id": "nope"})), Err(ToolError::InvalidParams(_))));
    let other = ulid::Ulid::generate().to_string();
    for t in [view_t, tree_t] {
        match call(t, serde_json::json!({"id": other})) { Err(ToolError::Failed(m)) => assert!(m.contains(&other), "{m}"), r => panic!("{r:?}") }
    }
    match call(search_t, serde_json::json!({"id": other, "pattern": "a"})) { Err(ToolError::Failed(m)) => assert!(m.contains(&other)), r => panic!("{r:?}") }
}
