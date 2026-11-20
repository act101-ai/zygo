use std::path::Path;
use std::time::{Duration, SystemTime};
use zygo_config::ResultsConfig;
use zygo_results::kind::{sniff, tree, Kind};
use zygo_results::{is_id, sweep, view, Store, Stored};

fn put(tool: &str, text: &str) -> (tempfile::TempDir, Stored) {
    let t = tempfile::tempdir().unwrap();
    let s = Store::put(&t.path().join("results"), tool, text.as_bytes()).unwrap();
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
    let s = Store::put(&dir, "Bash", data).unwrap();
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
    let a = Store::put(&dir, "Bash", b"x").unwrap();
    let b = Store::put(&dir, "Bash", b"x").unwrap();
    assert_ne!(a.id, b.id);
    for (data, n) in [(&b""[..], 0), (b"a\n", 1), (b"a\nb", 2), (b"a\r\nb\r\n", 2), (b"\n\n", 2)] {
        assert_eq!(Store::put(&dir, "Bash", data).unwrap().lines, n, "{data:?}");
    }
    assert_eq!(Store::put(&dir, "Bash", br#"{"a":1}"#).unwrap().kind, Kind::Json);
}

#[test]
fn store_get_refuses_a_malformed_id_and_names_an_unknown_one() {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("results");
    Store::put(&dir, "Bash", b"x").unwrap();
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
    let s = Store::put(&dir, "Bash", b"x").unwrap();
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
    let s = Store::put(&t.path().join("results"), "Bash", b"ok\xff\xfe\nrest\n").unwrap();
    assert!(view(&s, &ResultsConfig::default()).contains('\u{fffd}'));
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
    sweep(t.path(), now);
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
    sweep(t.path(), now);
    assert!(live.join("results").join("f0").exists() && live.join("token").exists(), "locked pid: live, kept");
    assert!(t.path().join("sessions").join("stray-file").exists());
    assert!(t.path().join("sessions").join("no-results").exists());     // changed just now
    sweep(&t.path().join("missing"), now);                                // no sessions directory: no panic
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
    sweep(t.path(), now);
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
    sweep(t.path(), now);
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
    sweep(t.path(), now + 48 * HOUR);
    assert!(outside.join("results").join("f0").exists(), "the target of a symlinked session is kept");
    assert!(other.join("results").join("f0").exists(), "the target of a symlinked results is kept");
    assert!(real.join("results").symlink_metadata().is_ok(), "a session whose results is a symlink is skipped");
}

#[test]
fn store_get_with_a_raw_file_of_another_length_is_no_result() {
    let t = tempfile::tempdir().unwrap();
    let dir = t.path().join("results");
    let s = Store::put(&dir, "Bash", b"abc").unwrap();
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
    Store::put(&dir, "Bash", b"x").unwrap();
    assert_eq!(std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);
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
    let s = Store::put(&sd.results(), "Bash", text.as_bytes()).unwrap();
    let v = view(&s, &ResultsConfig::default());
    assert!(v.len() <= 8192);
    assert!(v.contains(&format!("[zygo trimmed this output: {} lines, {} bytes; only part of it is shown. The full output is saved as result {id}. To read the rest, call result_search, result_view or result_tree with id \"{id}\".]", s.lines, text.len(), id = s.id)));
    let tools = zygo_mcp::tools::results::tools(&ctx(Some(t.path())));
    let first_400: Vec<String> = (1..=400).map(|i| format!("row {i} payload")).collect();
    assert_eq!(call(by_name(&tools, "result_view"), serde_json::json!({"id": s.id})).unwrap(), first_400.join("\n"));
    let want: Vec<String> = (61..=120).map(|i| format!("row {i} payload")).collect();
    assert_eq!(call(by_name(&tools, "result_view"), serde_json::json!({"id": s.id, "lines": "61-120"})).unwrap(), want.join("\n"));
    let found = call(by_name(&tools, "result_search"), serde_json::json!({"id": s.id, "pattern": "row 500 "})).unwrap();
    assert_eq!(found, "499- row 499 payload\n500: row 500 payload\n501- row 501 payload");
    let j = Store::put(&sd.results(), "Bash", br#"{"b":{"a":[1,2]}}"#).unwrap();
    assert_eq!(call(by_name(&tools, "result_tree"), serde_json::json!({"id": j.id})).unwrap(), "b: object\n  a: array[2]");
}

#[test]
fn tools_bad_arguments_are_invalid_params_and_an_unknown_id_is_a_failure() {
    let t = tempfile::tempdir().unwrap();
    let sd = SessionDir::new(t.path().to_path_buf());
    let s = Store::put(&sd.results(), "Bash", b"a\nb\n").unwrap();
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
