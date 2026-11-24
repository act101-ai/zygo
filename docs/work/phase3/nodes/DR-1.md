# DR-1 — Record format: parse and validate one DDR file, and `docs/decisions/` in place

**Spec:** `docs/specs/2026-11-24-decision-records-design.md` D§6.1, D§6.2, D§6.3, D§6.4, D§6.5, D§6.8 (the per-file rules and the names), D§5
**Files:** `crates/zygo-work/Cargo.toml`, `crates/zygo-work/src/lib.rs`, `crates/zygo-work/src/decisions/mod.rs`, `crates/zygo-work/src/decisions/record.rs`, `crates/zygo-work/tests/decision_record.rs`, `docs/decisions/README.md`
**Consumes:** —
**Produces:** `decisions::record::DIR: &str` (`"docs/decisions"`); `decisions::record::FILL_MARKER: &str` (`"<!-- zygo: fill -->"`); `decisions::record::Kind` (`Assumption | Decision | AcceptedLimit | Compromise | OpenFinding`; `Kind::parse(s: &str) -> Option<Kind>`, `Kind::name(self) -> &'static str`); `decisions::record::Status` (`Open | Ratified | Overturned | Fixed | Superseded`; same two fns); `decisions::record::Class` (`None | Acceptance | Security | Irreversible | Contract | Spec | Rule`; same two fns); `decisions::record::Record` (one pub field per D§6.3 key, the optional and operator keys as `Option<_>`, `depends_on: Vec<String>`, `body: String`); `decisions::record::valid_id(s: &str) -> bool`; `decisions::record::NameKind` (`Record(String) | Malformed | Other`); `decisions::record::name_kind(file_name: &str) -> NameKind`; `decisions::record::parse_record(stem: &str, bytes: &[u8]) -> Result<Record, Vec<String>>` (each `Err` item is one fault line); `decisions::record::read_record(path: &Path) -> Result<Record, Vec<String>>` (`lstat` first: a link, a FIFO, a directory or a file over 64 KiB is never opened)
**Acceptance:** `cargo test -p zygo-work --locked --test decision_record` → all pass. The cases include each D§6.8 per-file fault (no `+++` on line 1; no second `+++` within 8 KiB; bad TOML; each missing, mistyped or out-of-limit key; `id` ≠ file stem; each missing or empty required section; `## Finding` missing for `accepted-limit`, `compromise`, `open-finding`; a section that holds `<!-- zygo: fill -->`; `fixed` without `fixed_by`; `overturned` with an empty `review_note`; `promoted_to` without `promotion`; a 65 KiB file; bytes that are not UTF-8); unknown keys and sections accepted; `name_kind` on `DDR-<26 Crockford chars>.md`, `DDR-bad.md`, `README.md`, `x.tmp`; a link and a FIFO are malformed and not opened (`#[cfg(unix)]`); the filled example record in `docs/decisions/README.md` parses as valid.

## Plan

Implementer notes. All paths are relative to the repository root `/Users/kagemartin/Work/zygo`. "The spec" is `docs/specs/2026-11-24-decision-records-design.md`; D§n is a section of it. A record is one Markdown file `docs/decisions/<id>.md` with TOML front matter between two `+++` lines (D§6.2). This node parses and checks one file. It does not read the directory (DR-2), print anything (DR-3) or edit a file (DR-5). Its fault lines hold no record text, so `zygo_core::esc` is not needed here; the printers in DR-3 use it.

Reused code (do not rebuild): `zygo_work::hooks::valid_unit` (`crates/zygo-work/src/hooks.rs:585`, the §13.1 unit rule: 2–3 segments, each 1–48 of `[A-Za-z0-9._-]` and starting alphanumeric, at most 128 bytes); `zygo_work::report::parse_since` (`crates/zygo-work/src/report.rs:99`, parses `YYYY-MM-DD`); the open flags of `read_session_file` (`crates/zygo-hooks/src/workspace.rs:94-116`, copy the pattern, not the function: `zygo-work` cannot depend on `zygo-hooks`); `mkfifo` (`crates/zygo-hooks/tests/workspace.rs:392`, copy into the test file).

Consumes: —

Produces (in `crates/zygo-work/src/decisions/record.rs`, module path `zygo_work::decisions::record`):

```rust
pub const DIR: &str = "docs/decisions";
pub const FILL_MARKER: &str = "<!-- zygo: fill -->";
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum Kind { Assumption, Decision, AcceptedLimit, Compromise, OpenFinding }
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum Status { Open, Ratified, Overturned, Fixed, Superseded }
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum Class { None, Acceptance, Security, Irreversible, Contract, Spec, Rule }
// each of Kind, Status, Class:
pub fn parse(s: &str) -> Option<Self>;
pub fn name(self) -> &'static str;
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub id: String, pub kind: Kind, pub status: Status, pub class: Class, pub title: String,
    pub unit: String, pub round: u32, pub role: String, pub model: String, pub commit: String,
    pub date: String, pub depends_on: Vec<String>, pub reverse_cost: String,
    pub supersedes: Option<String>, pub fixed_by: Option<String>, pub promoted_by: Option<String>,
    pub reviewed: Option<String>, pub review_note: Option<String>, pub promoted_to: Option<String>,
    pub promotion: Option<String>, pub body: String,
}
pub fn valid_id(s: &str) -> bool;
#[derive(Debug, Clone, PartialEq, Eq)] pub enum NameKind { Record(String), Malformed, Other }
pub fn name_kind(file_name: &str) -> NameKind;
pub fn parse_record(stem: &str, bytes: &[u8]) -> Result<Record, Vec<String>>; // each Err item is one fault line
pub fn read_record(path: &Path) -> Result<Record, Vec<String>>;             // lstat first
```

Names consumed later (checked against `docs/work/phase3/nodes/DR-{2,3,4,5,6,7,10,12,13}.md`): `DIR`, `FILL_MARKER`, `Kind`, `Status`, `Class`, `Record`, `valid_id`, `NameKind`, `name_kind`, `parse_record`, `read_record`.

Name strings (D§6.3, D§6.4, D§6.5, D§8.2): Kind `assumption`, `decision`, `accepted-limit`, `compromise`, `open-finding`; Status `open`, `ratified`, `overturned`, `fixed`, `superseded`; Class `none`, `acceptance`, `security`, `irreversible`, `contract`, `spec`, `rule`. `parse` is exact and case-sensitive.

Readings of the spec that this plan fixes (each is the plain text of D§6.3/D§6.8; none adds a rule):
- Required keys are the D§6.3 keys not marked optional, operator-written or conditional: `id kind status class title unit round role model commit date depends_on reverse_cost`. `depends_on` is required and may be `[]`.
- `model` is a non-empty string; it is not checked against the catalog (a catalog change must not make old records malformed).
- `role` uses the D§10.2 grammar `^[a-z][a-z0-9-]{0,31}$` (the session legend is not known to a file reader).
- Hex keys (`commit`, `fixed_by`, `promoted_by`) are 40 or 64 characters of lower-case `[0-9a-f]`, as git prints them.
- When `## Finding` is required it follows the same rule as the four sections: present and not empty, and the fill marker counts as empty.

### Steps

- [x] **1. Write the failing tests.** Create `crates/zygo-work/tests/decision_record.rs` with this content (26 tests):

```rust
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
    assert!(parse_record(ID, padded(8192).as_bytes()).is_ok());
    assert_eq!(parse_record(ID, padded(8193).as_bytes()).unwrap_err(), vec!["no closing +++ line within 8 KiB"]);
    let open_only = text(&keys(), BODY).replacen("+++\n\n## Choice", "\n## Choice", 1);
    assert_eq!(parse_record(ID, open_only.as_bytes()).unwrap_err(), vec!["no closing +++ line within 8 KiB"]);
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
    assert!(fs::symlink_metadata(&p).unwrap().file_type().is_fifo());
}

#[test] fn readme_example_is_valid() {
    let readme = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/decisions/README.md")).unwrap();
    let start = readme.find("```markdown\n").unwrap() + "```markdown\n".len();
    let end = start + readme[start..].find("\n```\n").unwrap() + 1;
    let r = parse_record(ID, readme[start..end].as_bytes()).unwrap();
    assert_eq!((r.id.as_str(), r.kind, r.status, r.class), (ID, Kind::Decision, Status::Open, Class::None));
}
```

- [x] **2. Run and see them fail.** `cargo test -p zygo-work --locked --test decision_record` fails to compile with `unresolved import zygo_work::decisions` (E0432).

- [x] **3. Move `toml` to the normal dependencies.** In `crates/zygo-work/Cargo.toml` move `toml.workspace = true` from `[dev-dependencies]` to `[dependencies]` (after `ulid.workspace = true`). No new dependency: `toml` 1.2.1 is a workspace dependency (`Cargo.toml:66`) and the `zygo-work` entry of `Cargo.lock` already lists `toml` (it holds dev-dependencies too). `Cargo.lock` does not change; `--locked` proves it in step 10. `toml_edit` is not needed here (DR-5 edits).

- [x] **4. Add the modules.** `crates/zygo-work/src/lib.rs`: add `pub mod decisions;` after `pub mod current;`. Create `crates/zygo-work/src/decisions/mod.rs` with `//! Delegated decision records (spec docs/specs/2026-11-24-decision-records-design.md).` and `pub mod record;`. Create `crates/zygo-work/src/decisions/record.rs` with a `//!` line naming D§6.

- [x] **5. Constants, enums, ids and names** in `record.rs`: `DIR`, `FILL_MARKER`, `Kind`, `Status`, `Class` with `parse`/`name` from the name strings above. `valid_id`: `DDR-` then exactly 26 characters of `0-9A-HJKMNP-TV-Z` (D§6.1). `name_kind`: `Record(<name without .md>)` when the name is `valid_id(stem)` plus `.md`; else `Malformed` when it starts with `DDR-` and ends with `.md`; else `Other` (D§6.8).

- [x] **6. `Record`** as in Produces, with a `///` line per field naming its D§6.3 key. `body` is the text after the closing `+++` line and that line's `\n`.

- [x] **7. `parse_record(stem, bytes)`.** Structural checks in this order; the first that fails returns `Err` with that one line:
  1. `bytes.len() > 65536` → `over 64 KiB`.
  2. not UTF-8 → `not UTF-8`.
  3. Line 1 (up to the first `\n`, one trailing `\r` removed) is not exactly `+++` → `line 1 is not +++`. An empty file fails here.
  4. The closing line is the first later line equal to `+++` (one trailing `\r` removed) whose end (the byte after its `\n`, or the text end when it has none) is at most 8192. A line that ends past byte 8192 before a closing line is found, or no closing line → `no closing +++ line within 8 KiB`.
  5. `toml::from_str::<toml::Table>(<text between the two lines>)` fails → `front matter is not TOML: <e.message() with each \n and \r made a space>`.

  Then collect all of these faults, in this order, and return `Err(all)` when any:
  - Key faults in D§6.3 row order. Absent required key → `missing key <k>`. Present key of the wrong TOML type or outside its rule → `bad key <k>`. Unknown keys and tables are ignored.

    | key | TOML type | rule |
    |---|---|---|
    | `id` | string | `valid_id` |
    | `kind` / `status` / `class` | string | `Kind::parse` / `Status::parse` / `Class::parse` is `Some` |
    | `title` | string | 1–120 bytes, no `\n` or `\r` |
    | `unit` | string | `crate::hooks::valid_unit` |
    | `round` | integer | 1 ..= `u32::MAX` |
    | `role` | string | `^[a-z][a-z0-9-]{0,31}$` |
    | `model` | string | not empty |
    | `commit`; optional `fixed_by`, `promoted_by` | string | 40 or 64 of `[0-9a-f]` |
    | `date`; optional `reviewed` | string | 10 bytes and `crate::report::parse_since` is `Ok` |
    | `depends_on` | array of strings | at most 64 items, each `valid_id` |
    | `reverse_cost` | string | `low`, `medium` or `high` |
    | optional `supersedes` | string | `valid_id` |
    | optional `review_note` | string | at most 2000 bytes |
    | optional `promoted_to` | string | `adr`, `spec` or `rule` |
    | optional `promotion` | string | at most 500 bytes |
  - `id` valid and not equal to `stem` → `id does not equal the file stem`.
  - Section faults, order `Finding`, `Choice`, `Alternatives rejected`, `Evidence`, `Cost to reverse`; `Finding` only when `kind` is `accepted-limit`, `compromise` or `open-finding`. In the body (lines split on `\n`, one trailing `\r` removed), a section starts at the first line whose `trim_end()` is `## <name>` and runs to the next line that starts with `## ` or the end. No such line → `missing section ## <name>`. Every content line blank, or any content line holds `FILL_MARKER` → `empty section ## <name>`.
  - Agreement faults, reported only when each key they read is absent or valid: `status = fixed` and no `fixed_by` → `fixed without fixed_by`; `status = overturned` and `review_note` absent or `""` → `overturned without review_note`; `promoted_to` present and `promotion` absent → `promoted_to without promotion`.

- [x] **8. `read_record(path)`**, never opening a link, FIFO, directory or a file over 64 KiB (D§6.8):
  1. `fs::symlink_metadata(path)`; error → `cannot read: <e>`; a link → `is a link; not read`; not `is_file()` → `is not a regular file; not read`; `len() > 65536` → `over 64 KiB`.
  2. Open read-only; on unix add `custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)` (pattern of `crates/zygo-hooks/src/workspace.rs:97-101`). Open error: `ELOOP` on unix → `is a link; not read`, else `cannot read: <e>`. The opened handle's `metadata()` not `is_file()` → `is not a regular file; not read`.
  3. Read at most 65537 bytes (`Read::take(65537)`), so a file that grew after step 1 still hits the size fault.
  4. `parse_record(<path.file_stem() as UTF-8, else "">, &bytes)`.

- [x] **9. Write `docs/decisions/README.md`** with exactly this content (the D§6.2 example; see the premise note):

~~~~markdown
# Decision records

Each file `DDR-<ULID>.md` in this directory is one delegated decision record: a choice an agent made under delegated authority that the operator has not yet ratified. The rules are in `docs/specs/2026-11-24-decision-records-design.md` D§6. zygo reads only names that match `DDR-<26 Crockford base32 characters>.md`; it ignores this file and every other name. A record is never deleted or renamed; it leaves the open set by a status change.

## Example

```markdown
+++
id = "DDR-01JDQ2Y7Z8K4M5N6P7Q8R9S0TV"
kind = "decision"
status = "open"
class = "none"
title = "Store the pane name in the member record, not in the session view"
unit = "phase-3:N12"
round = 2
role = "code"
model = "claude-sonnet-4-5"
commit = "6e7fe2f0c1d2e3f4a5b6c7d8e9f0011223344556"
date = "2026-11-24"
depends_on = ["DDR-01JDQ2W0000000000000000000"]
reverse_cost = "low"
+++

## Choice

One paragraph: what the agent chose.

## Alternatives rejected

Each alternative, and why it was rejected.

## Evidence

The file:line, test output or document text that the choice rests on.

## Cost to reverse

What a reversal changes, and which later work it touches.
```
~~~~

- [x] **10. Run and see them pass.** `cargo test -p zygo-work --locked --test decision_record` → 26 passed on unix (24 on Windows: the two `#[cfg(unix)]` tests compile out).

- [x] **11. Verify the workspace.** `cargo test --workspace --locked --no-fail-fast` → all pass (the `test` job of `.github/workflows/ci.yml:45`). CI runs no `cargo clippy` (`.github/workflows/ci.yml`, `release.yml`), so clippy is not a gate. `git diff --stat main` names only the six files of **Files** and this node file (no `Cargo.lock`).

- [x] **12. Commit** on the node branch: message `feat(work): parse and check one decision record (DR-1)`, then the trailers `Zygo-Session: <session>` and `Zygo-Work: <unit of your brief>`, each on its own line. No Claude attribution.

Premise note: the D§6.2 example in the spec now has a 40-character `commit` (commit 3511ad5) and matches the README.

### Review round 1

- **F1** FIFO test did not prove the FIFO is never opened: fixed (9612477). The test chmods the FIFO to 000 and reads again; an open would give `cannot read: …` (EACCES), so only the lstat path gives `is not a regular file; not read`. The test fails when the lstat `is_file()` check is removed. The link test stays as it is: `O_NOFOLLOW` gives `ELOOP` before the permission check.
- **F2** Windows open followed a link swapped in after lstat: fixed (9612477). On Windows the open adds `FILE_FLAG_OPEN_REPARSE_POINT` (`0x0020_0000`), and the opened handle is refused with `is a link; not read` when `file_type().is_symlink()`, and with `is not a regular file; not read` when not `is_file()`.
- **F3** Only `ELOOP` gave the link text: fixed (9612477). A failed open also gives `is a link; not read` when a second lstat sees a link (other BSDs give `EMLINK` or `EFTYPE`), as `open_met_link` in `crates/zygo-hooks/src/workspace.rs:72-80`. The `ELOOP` fast path stays.
- **F4a** hex case: rejected. git writes lower-case hex, and one canonical form lets readers compare commits by string equality.
- **F4b** TOML native dates: rejected. D§6.2 writes dates as strings, and one wire type keeps every reader in agreement.
- **F5** code fences: rejected. D§6.8 defines a section as a `## ` heading line and does not define Markdown parsing. The first match is the conservative reading.
- **F6** CRLF records got a smaller front matter limit: fixed (9612477). The 8 KiB limit is measured to the end of the closing `+++`, without `\r` or `\n`. New test `crlf_front_matter_limit_equals_lf`. Changed test `front_matter_not_closed_within_8_kib`: it encoded the old rule (`+++\n` ending at 8192 valid, 8193 refused); now `+++\n` ending at 8193 (`+++` at 8192) is valid and 8194 is refused.
- **F7** missing tests: fixed (9612477). New tests `leading_bom_is_line_one_fault`, `duplicate_key_is_not_toml`, `plus_line_in_multiline_string_closes_early`, and a whitespace-only `## Finding` case in `finding_required_for_three_kinds`. Each passed when written.

`cargo test -p zygo-work --locked --test decision_record` → 30 passed on unix.
