//! DR-1: one decision record file (D§6.1–D§6.5, D§6.8).
use std::fs;
use std::path::Path;
use zygo_work::decisions::record::*;

const ID: &str = "DDR-01JDQ2Y7Z8K4M5N6P7Q8R9S0TV";
const ID2: &str = "DDR-01JDQ2Y7Z8K4M5N6P7Q8R9S0TW";
const DEP: &str = "DDR-01JDQ2W0000000000000000000";
const C40: &str = "6e7fe2f0c1d2e3f4a5b6c7d8e9f0011223344556";
const BODY: &str = "\n## Choice\n\nC.\n\n## Alternatives rejected\n\nA.\n\n## Evidence\n\nE.\n\n## Cost to reverse\n\nR.\n";
const SECTIONS: [(&str, &str); 4] = [("Choice", "C."), ("Alternatives rejected", "A."), ("Evidence", "E."), ("Cost to reverse", "R.")];

fn q(s: &str) -> String { format!("\"{s}\"") }

/// The required keys of a valid record, as TOML literals, in D§6.3 order.
fn keys() -> Vec<(&'static str, String)> {
    vec![("id", q(ID)), ("kind", q("decision")), ("status", q("open")), ("class", q("none")),
         ("title", q("Store the pane name")), ("unit", q("phase-3:N12")), ("round", "2".into()),
         ("role", q("code")), ("model", q("claude-sonnet-4-5")), ("commit", q(C40)),
         ("date", q("2026-11-24")), ("depends_on", format!("[{}]", q(DEP))), ("reverse_cost", q("low"))]
}

/// `keys()` with `k` removed, then set to `v` when `v` is given.
fn with(k: &'static str, v: Option<&str>) -> Vec<(&'static str, String)> {
    let mut ks: Vec<_> = keys().into_iter().filter(|(n, _)| *n != k).collect();
    if let Some(v) = v { ks.push((k, v.to_string())); }
    ks
}

fn text(ks: &[(&str, String)], body: &str) -> String {
    let mut s = String::from("+++\n");
    for (k, v) in ks { s.push_str(&format!("{k} = {v}\n")); }
    s + "+++\n" + body
}

fn ok(ks: &[(&str, String)], body: &str) -> Record { parse_record(ID, text(ks, body).as_bytes()).unwrap() }
fn faults(ks: &[(&str, String)], body: &str) -> Vec<String> { parse_record(ID, text(ks, body).as_bytes()).unwrap_err() }

/// A valid record whose closing `+++\n` ends at byte `total`.
fn padded(total: usize) -> String {
    let head = text(&keys(), "").trim_end_matches("+++\n").to_string();
    format!("{head}#{}\n+++\n{BODY}", "x".repeat(total - head.len() - 6))
}

/// A valid CRLF record whose closing `+++` (without its `\r\n`) ends at byte `plus_end`.
fn padded_crlf(plus_end: usize) -> String {
    let head = text(&keys(), "").trim_end_matches("+++\n").replace('\n', "\r\n");
    format!("{head}#{}\r\n+++\r\n{}", "x".repeat(plus_end - head.len() - 6), BODY.replace('\n', "\r\n"))
}

#[cfg(unix)]
fn mkfifo(path: &Path) { // as crates/zygo-hooks/tests/workspace.rs:392
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    // SAFETY: `c` is a valid NUL-terminated path.
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0, "mkfifo {}", path.display());
}

#[test] fn fixed_values() { assert_eq!(DIR, "docs/decisions"); assert_eq!(FILL_MARKER, "<!-- zygo: fill -->"); }

#[test] fn kind_status_class_names() {
    for (s, k) in [("assumption", Kind::Assumption), ("decision", Kind::Decision), ("accepted-limit", Kind::AcceptedLimit),
                   ("compromise", Kind::Compromise), ("open-finding", Kind::OpenFinding)] { assert_eq!(Kind::parse(s), Some(k)); assert_eq!(k.name(), s); }
    for (s, k) in [("open", Status::Open), ("ratified", Status::Ratified), ("overturned", Status::Overturned),
                   ("fixed", Status::Fixed), ("superseded", Status::Superseded)] { assert_eq!(Status::parse(s), Some(k)); assert_eq!(k.name(), s); }
    for (s, k) in [("none", Class::None), ("acceptance", Class::Acceptance), ("security", Class::Security), ("irreversible", Class::Irreversible),
                   ("contract", Class::Contract), ("spec", Class::Spec), ("rule", Class::Rule)] { assert_eq!(Class::parse(s), Some(k)); assert_eq!(k.name(), s); }
    assert_eq!(Kind::parse("Decision"), None); assert_eq!(Status::parse(""), None); assert_eq!(Class::parse("minor"), None);
}

#[test] fn valid_id_cases() {
    assert!(valid_id(ID)); assert!(valid_id(DEP));
    for bad in ["DDR-01JDQ2Y7Z8K4M5N6P7Q8R9S0TI", "DDR-01JDQ2Y7Z8K4M5N6P7Q8R9S0TL", "DDR-01JDQ2Y7Z8K4M5N6P7Q8R9S0TO",
                "DDR-01JDQ2Y7Z8K4M5N6P7Q8R9S0TU", "ddr-01JDQ2Y7Z8K4M5N6P7Q8R9S0TV", "DDR-01jdq2y7z8k4m5n6p7q8r9s0tv",
                "DDR-01JDQ2Y7Z8K4M5N6P7Q8R9S0T", "DDR-01JDQ2Y7Z8K4M5N6P7Q8R9S0TVV", "DDR-", "DDR-bad"] { assert!(!valid_id(bad), "{bad}"); }
}

#[test] fn name_kind_cases() {
    assert_eq!(name_kind(&format!("{ID}.md")), NameKind::Record(ID.into()));
    assert_eq!(name_kind("DDR-bad.md"), NameKind::Malformed);
    assert_eq!(name_kind("DDR-01JDQ2Y7Z8K4M5N6P7Q8R9S0TI.md"), NameKind::Malformed);
    assert_eq!(name_kind("README.md"), NameKind::Other);
    assert_eq!(name_kind("x.tmp"), NameKind::Other);
    assert_eq!(name_kind(&format!("{ID}.tmp")), NameKind::Other);
    assert_eq!(name_kind(&format!("{}.md", ID.to_lowercase())), NameKind::Other);
}

#[test] fn valid_record_parses_every_field() {
    assert_eq!(ok(&keys(), BODY), Record {
        id: ID.into(), kind: Kind::Decision, status: Status::Open, class: Class::None, title: "Store the pane name".into(),
        unit: "phase-3:N12".into(), round: 2, role: "code".into(), model: "claude-sonnet-4-5".into(), commit: C40.into(),
        date: "2026-11-24".into(), depends_on: vec![DEP.into()], reverse_cost: "low".into(), supersedes: None, fixed_by: None,
        promoted_by: None, reviewed: None, review_note: None, promoted_to: None, promotion: None, body: BODY.into() });
    let mut ks = keys();
    ks.extend([("supersedes", q(DEP)), ("fixed_by", q(C40)), ("promoted_by", q(C40)), ("reviewed", q("2026-11-25")),
               ("review_note", q("ok")), ("promoted_to", q("adr")), ("promotion", q("ADR 7"))]);
    let r = ok(&ks, BODY);
    assert_eq!((r.supersedes.as_deref(), r.fixed_by.as_deref(), r.promoted_by.as_deref(), r.reviewed.as_deref(),
                r.review_note.as_deref(), r.promoted_to.as_deref(), r.promotion.as_deref()),
               (Some(DEP), Some(C40), Some(C40), Some("2026-11-25"), Some("ok"), Some("adr"), Some("ADR 7")));
}

#[test] fn over_64_kib() {
    let mut s = text(&keys(), BODY); s.push_str(&"x".repeat(65536 - s.len()));
    assert!(parse_record(ID, s.as_bytes()).is_ok());
    s.push('x'); assert_eq!(parse_record(ID, s.as_bytes()).unwrap_err(), vec!["over 64 KiB"]);
    let mut s = text(&keys(), BODY); s.push_str(&"x".repeat(66560 - s.len()));
    assert_eq!(parse_record(ID, s.as_bytes()).unwrap_err(), vec!["over 64 KiB"]);
}

#[test] fn not_utf8() {
    let mut b = text(&keys(), BODY).into_bytes(); b.extend_from_slice(b"\xff\n");
    assert_eq!(parse_record(ID, &b).unwrap_err(), vec!["not UTF-8"]);
}

#[test] fn line_one_not_open() {
    let t = text(&keys(), BODY);
    for s in [format!("\n{t}"), t.replacen("+++", "---", 1), t.replacen("+++", "+++ ", 1), String::new()] {
        assert_eq!(parse_record(ID, s.as_bytes()).unwrap_err(), vec!["line 1 is not +++"], "{s:?}");
    }
}

#[test] fn front_matter_not_closed_within_8_kib() {
    // The limit is measured to the end of the closing `+++`, without its line ending.
    assert!(parse_record(ID, padded(8193).as_bytes()).is_ok());
    assert_eq!(parse_record(ID, padded(8194).as_bytes()).unwrap_err(), vec!["no closing +++ line within 8 KiB"]);
    let open_only = text(&keys(), BODY).replacen("+++\n\n## Choice", "\n## Choice", 1);
    assert_eq!(parse_record(ID, open_only.as_bytes()).unwrap_err(), vec!["no closing +++ line within 8 KiB"]);
}

#[test] fn crlf_front_matter_limit_equals_lf() {
    assert_eq!(padded_crlf(8192).find("\r\n+++\r\n").unwrap() + 5, 8192);
    assert!(parse_record(ID, padded_crlf(8192).as_bytes()).is_ok());
    assert_eq!(parse_record(ID, padded_crlf(8193).as_bytes()).unwrap_err(), vec!["no closing +++ line within 8 KiB"]);
}

#[test] fn leading_bom_is_line_one_fault() {
    let s = format!("\u{FEFF}{}", text(&keys(), BODY));
    assert_eq!(parse_record(ID, s.as_bytes()).unwrap_err(), vec!["line 1 is not +++"]);
}

#[test] fn duplicate_key_is_not_toml() {
    let mut ks = keys(); ks.push(("id", q(ID)));
    assert_eq!(faults(&ks, BODY), vec!["front matter is not TOML: duplicate key"]);
}

#[test] fn plus_line_in_multiline_string_closes_early() {
    let ks = with("title", Some("\"\"\"a\n+++\nb\"\"\""));
    assert_eq!(faults(&ks, BODY), vec!["front matter is not TOML: invalid multi-line basic string, expected `\"`"]);
}

#[test] fn bad_toml() {
    let f = faults(&with("round", Some("")), BODY);
    assert_eq!(f.len(), 1, "{f:?}");
    assert!(f[0].starts_with("front matter is not TOML: ") && !f[0].contains('\n'), "{f:?}");
}

#[test] fn each_missing_required_key() {
    for (k, _) in keys() { assert_eq!(faults(&with(k, None), BODY), vec![format!("missing key {k}")], "{k}"); }
}

#[test] fn each_mistyped_or_out_of_limit_key() {
    let cases: Vec<(&'static str, String)> = vec![
        ("id", "1".into()), ("id", q("DDR-bad")), ("kind", q("choice")), ("kind", "1".into()), ("status", q("closed")),
        ("class", q("minor")), ("title", q("")), ("title", q(&"x".repeat(121))), ("title", q("a\\nb")),
        ("unit", q("phase-3")), ("unit", q("a:b c")), ("round", "0".into()), ("round", q("2")), ("round", "4294967296".into()),
        ("role", q("Code")), ("role", q("")), ("model", q("")), ("model", "1".into()),
        ("commit", q("abc")), ("commit", q(&format!("{C40}5"))), ("commit", q(&C40.to_uppercase())),
        ("date", q("2026-13-01")), ("date", "2026-11-24".into()),
        ("depends_on", q(DEP)), ("depends_on", "[\"x\"]".into()), ("depends_on", format!("[{}]", vec![q(DEP); 65].join(","))),
        ("reverse_cost", q("huge")), ("supersedes", q("x")), ("supersedes", format!("[{}]", q(DEP))),
        ("fixed_by", q("xyz")), ("promoted_by", q("xyz")), ("reviewed", q("yesterday")),
        ("review_note", q(&"x".repeat(2001))), ("promoted_to", q("wiki")), ("promotion", q(&"x".repeat(501))),
    ];
    for (k, v) in cases { assert_eq!(faults(&with(k, Some(&v)), BODY), vec![format!("bad key {k}")], "{k} = {v}"); }
}

#[test] fn limits_at_the_edge_are_valid() {
    for (k, v) in [("title", q(&"x".repeat(120))), ("review_note", q(&"x".repeat(2000))), ("commit", q(&"a".repeat(64))),
                   ("depends_on", format!("[{}]", vec![q(DEP); 64].join(","))), ("depends_on", "[]".into()),
                   ("round", "4294967295".into()), ("model", q("unknown")), ("unit", q("phase-3:N12:plan"))] {
        assert!(parse_record(ID, text(&with(k, Some(&v)), BODY).as_bytes()).is_ok(), "{k} = {v}");
    }
    let mut ks = with("promoted_to", Some(&q("rule"))); ks.push(("promotion", q(&"x".repeat(500))));
    assert_eq!(ok(&ks, BODY).promotion.unwrap().len(), 500);
}

#[test] fn id_not_file_stem() {
    assert_eq!(parse_record(ID2, text(&keys(), BODY).as_bytes()).unwrap_err(), vec!["id does not equal the file stem"]);
}

#[test] fn each_missing_or_empty_section() {
    for (n, c) in SECTIONS {
        assert_eq!(faults(&keys(), &BODY.replace(&format!("## {n}\n"), "## Other\n")), vec![format!("missing section ## {n}")], "{n}");
        assert_eq!(faults(&keys(), &BODY.replace(c, " ")), vec![format!("empty section ## {n}")], "{n}");
        assert_eq!(faults(&keys(), &BODY.replace(c, &format!("{FILL_MARKER} One paragraph."))), vec![format!("empty section ## {n}")], "{n}");
    }
}

#[test] fn finding_required_for_three_kinds() {
    for k in ["accepted-limit", "compromise", "open-finding"] {
        let ks = with("kind", Some(&q(k)));
        assert_eq!(faults(&ks, BODY), vec!["missing section ## Finding"], "{k}");
        assert_eq!(ok(&ks, &format!("\n## Finding\n\nF.\n{BODY}")).kind.name(), k);
        assert_eq!(faults(&ks, &format!("\n## Finding\n\n{FILL_MARKER} The finding.\n{BODY}")), vec!["empty section ## Finding"], "{k}");
        assert_eq!(faults(&ks, &format!("\n## Finding\n\n  \t\n\n{BODY}")), vec!["empty section ## Finding"], "{k}");
    }
    for k in ["assumption", "decision"] { assert_eq!(ok(&with("kind", Some(&q(k))), BODY).kind.name(), k); }
}

#[test] fn status_keys_agree() {
    assert_eq!(faults(&with("status", Some(&q("fixed"))), BODY), vec!["fixed without fixed_by"]);
    let mut ks = with("status", Some(&q("fixed"))); ks.push(("fixed_by", q(C40))); assert_eq!(ok(&ks, BODY).status, Status::Fixed);
    assert_eq!(faults(&with("status", Some(&q("overturned"))), BODY), vec!["overturned without review_note"]);
    let mut ks = with("status", Some(&q("overturned"))); ks.push(("review_note", q("")));
    assert_eq!(faults(&ks, BODY), vec!["overturned without review_note"]);
    ks.pop(); ks.push(("review_note", q("wrong"))); assert_eq!(ok(&ks, BODY).status, Status::Overturned);
    assert_eq!(faults(&with("promoted_to", Some(&q("spec"))), BODY), vec!["promoted_to without promotion"]);
    for s in ["ratified", "superseded"] { assert_eq!(ok(&with("status", Some(&q(s))), BODY).status.name(), s); }
}

#[test] fn unknown_keys_and_sections_accepted() {
    let mut ks = keys(); ks.push(("extra", "1".into()));
    let body = format!("{BODY}\n## Notes\n\nN.\n");
    let s = format!("{}[tool]\nx = 1\n+++\n{body}", text(&ks, "").trim_end_matches("+++\n"));
    assert_eq!(parse_record(ID, s.as_bytes()).unwrap().body, body);
}

#[test] fn faults_are_listed_together_in_order() {
    let mut ks: Vec<_> = keys().into_iter().filter(|(k, _)| !matches!(*k, "title" | "round" | "status")).collect();
    ks.push(("round", "0".into())); ks.push(("status", q("fixed")));
    assert_eq!(parse_record(ID2, text(&ks, &BODY.replace("E.", " ")).as_bytes()).unwrap_err(),
               vec!["missing key title", "bad key round", "id does not equal the file stem", "empty section ## Evidence", "fixed without fixed_by"]);
}

#[test] fn crlf_record_parses() {
    let s = text(&keys(), BODY).replace('\n', "\r\n");
    assert_eq!(parse_record(ID, s.as_bytes()).unwrap().body, BODY.replace('\n', "\r\n"));
}

#[test] fn read_record_reads_a_regular_file() {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join(format!("{ID}.md")); fs::write(&p, text(&keys(), BODY)).unwrap();
    assert_eq!(read_record(&p).unwrap(), ok(&keys(), BODY));
    let p2 = t.path().join(format!("{ID2}.md")); fs::write(&p2, text(&keys(), BODY)).unwrap();
    assert_eq!(read_record(&p2).unwrap_err(), vec!["id does not equal the file stem"]);
}

#[test] fn read_record_over_64_kib_is_not_opened() {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join(format!("{ID}.md"));
    let mut s = text(&keys(), BODY); s.push_str(&"x".repeat(66560 - s.len())); fs::write(&p, s).unwrap();
    assert_eq!(read_record(&p).unwrap_err(), vec!["over 64 KiB"]);
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&p, fs::Permissions::from_mode(0o000)).unwrap();
        assert_eq!(read_record(&p).unwrap_err(), vec!["over 64 KiB"]); // an open would fail with "cannot read: …"
    }
}

#[test] fn directory_and_missing_file_are_malformed() {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join(format!("{ID}.md")); fs::create_dir(&p).unwrap();
    assert_eq!(read_record(&p).unwrap_err(), vec!["is not a regular file; not read"]);
    let f = read_record(&t.path().join(format!("{ID2}.md"))).unwrap_err();
    assert!(f.len() == 1 && f[0].starts_with("cannot read: "), "{f:?}");
}

#[cfg(unix)]
#[test] // review focus 42 (D§6.8)
fn link_is_malformed_and_not_followed() {
    let t = tempfile::tempdir().unwrap();
    let target = t.path().join("target.md"); fs::write(&target, text(&keys(), BODY)).unwrap();
    let p = t.path().join(format!("{ID}.md")); std::os::unix::fs::symlink(&target, &p).unwrap();
    assert_eq!(read_record(&p).unwrap_err(), vec!["is a link; not read"]);
    assert!(fs::symlink_metadata(&p).unwrap().file_type().is_symlink());
}

#[cfg(unix)]
#[test] // review focus 42 (D§6.8)
fn fifo_is_malformed_and_not_opened() {
    use std::os::unix::fs::FileTypeExt;
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join(format!("{ID}.md")); mkfifo(&p);
    assert_eq!(read_record(&p).unwrap_err(), vec!["is not a regular file; not read"]);
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&p, fs::Permissions::from_mode(0o000)).unwrap();
    // An open would fail with "cannot read: …" (EACCES); only the lstat check gives this text.
    assert_eq!(read_record(&p).unwrap_err(), vec!["is not a regular file; not read"]);
    assert!(fs::symlink_metadata(&p).unwrap().file_type().is_fifo());
}

#[test] fn readme_example_is_valid() {
    let readme = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/decisions/README.md")).unwrap();
    let start = readme.find("```markdown\n").unwrap() + "```markdown\n".len();
    let end = start + readme[start..].find("\n```\n").unwrap() + 1;
    let r = parse_record(ID, readme[start..end].as_bytes()).unwrap();
    assert_eq!((r.id.as_str(), r.kind, r.status, r.class), (ID, Kind::Decision, Status::Open, Class::None));
}
