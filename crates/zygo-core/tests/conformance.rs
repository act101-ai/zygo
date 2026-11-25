//! The conformance check and generator (K§6.2.5). CF-1: the input model and check items 1-9 on fixture inputs.
#[allow(dead_code)] // later nodes read the rest of the model
#[path = "conformance/model.rs"]
mod model;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use model::*;

fn fx() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/conformance/fixtures")
}

fn repo() -> PathBuf {
    fx().join("repo")
}

fn text(name: &str) -> String {
    std::fs::read_to_string(fx().join(name)).unwrap()
}

fn client(id: &str, name: &str, version: &str) -> Client {
    Client { id: id.into(), name: name.into(), version: version.into(), supported: true }
}

fn base() -> (Registry, Vec<GradeFile>, Vec<Client>) {
    let r = Registry::load(&fx().join("registry.toml")).unwrap();
    let g = vec![GradeFile::load(&fx().join("alpha.toml")).unwrap(), GradeFile::load(&fx().join("beta.toml")).unwrap()];
    (r, g, vec![client("alpha", "Alpha", "1.0.0"), client("beta", "Beta", "2.0.0")])
}

fn items(f: &[Fault]) -> BTreeSet<u8> {
    f.iter().map(|x| x.item).collect()
}

fn set(v: &[u8]) -> BTreeSet<u8> {
    v.iter().copied().collect()
}

fn entry<'a>(g: &'a mut GradeFile, criterion: &str) -> &'a mut GradeEntry {
    g.grade.iter_mut().find(|e| e.criterion == criterion).unwrap()
}

fn with_evidence(ev: &[&str]) -> BTreeSet<u8> {
    let (r, mut g, c) = base();
    entry(&mut g[0], "A-3.1").evidence = ev.iter().map(|s| s.to_string()).collect();
    items(&check(&repo(), &r, &g, &c))
}

fn copy_repo() -> tempfile::TempDir {
    fn copy(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for e in std::fs::read_dir(from).unwrap() {
            let e = e.unwrap();
            if e.file_type().unwrap().is_dir() {
                copy(&e.path(), &to.join(e.file_name()));
            } else {
                std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
            }
        }
    }
    let d = tempfile::tempdir().unwrap();
    copy(&repo(), d.path());
    d
}

#[test]
fn input_unknown_key_refused() {
    let e = Registry::parse(&(text("registry.toml") + "\n[extra]\nx = 1\n")).unwrap_err();
    assert!(e.contains("extra"));
    let e = Registry::parse(&text("registry.toml").replace("tier = \"core\"", "tier = \"core\"\ncolor = \"red\"")).unwrap_err();
    assert!(e.contains("color"));
    let e = GradeFile::parse(&text("alpha.toml").replacen("status = \"met\"", "status = \"met\"\nweight = 2", 1)).unwrap_err();
    assert!(e.contains("weight"));
}

#[test]
fn input_bad_id_refused() {
    for bad in ["A-7.1x", "B-1", "A-1.2.3", "a-1", "A-", "WA-1."] {
        let q = format!("\"{bad}\"");
        assert!(Registry::parse(&text("registry.toml").replace("\"A-3.1\"", &q)).unwrap_err().contains(bad));
        assert!(GradeFile::parse(&text("alpha.toml").replace("\"A-3.1\"", &q)).unwrap_err().contains(bad));
        assert!(GradeFile::parse(&text("alpha.toml").replace("capability = \"A-3\"", &format!("capability = {q}")))
            .unwrap_err()
            .contains(bad));
    }
    let ok = text("registry.toml").replace("\"A-3\"", "\"WA-30\"").replace("\"A-3.1\"", "\"WA-30.12\"");
    assert!(Registry::parse(&ok).is_ok());
}

#[test]
fn input_evidence_path_absolute_or_dotdot_refused() {
    for bad in [
        "test:/crates/x.rs::f",
        "fixture:/etc/passwd",
        "fixture:C:/x.json",
        r"fixture:fixtures\beta\capture.json",
        "fixture:fixtures/../crates/demo/tests/ev.rs",
        "test:crates/../../x.rs::f",
        "test:docs/x.rs::f",
        "premise:docs/other.md#Q1",
        "doc:readme",
    ] {
        let t = text("alpha.toml").replace("\"test:crates/demo/tests/ev.rs::plain\"", &format!("'{bad}'"));
        assert!(GradeFile::parse(&t).is_err(), "{bad}");
        assert!(evidence(&repo(), bad).is_err(), "{bad}");
    }
}

#[test]
fn input_reason_or_url_line_break_refused() {
    for brk in ["\\n", "\\r"] {
        assert!(GradeFile::parse(&text("alpha.toml").replace("Alpha has no turn events", &format!("Alpha has{brk}no turn events"))).is_err());
        assert!(GradeFile::parse(&text("alpha.toml").replace("alpha/models", &format!("alpha/{brk}models"))).is_err());
    }
    let reg = text("registry.toml")
        + "\n[[bar_exception]]\nclient = \"alpha\"\ncriterion = \"A-1.2\"\nreason = \"one\\ntwo\"\napproved = \"operator\"\n";
    assert!(Registry::parse(&reg).is_err());
}

#[test]
fn input_reason_or_url_over_300_refused() {
    let r = |n: usize| text("alpha.toml").replace("Alpha has no turn events", &"x".repeat(n));
    assert!(GradeFile::parse(&r(300)).is_ok());
    assert!(GradeFile::parse(&r(301)).is_err());
    let u = text("alpha.toml").replace("https://example.com/alpha/models", &format!("https://example.com/{}", "x".repeat(300)));
    assert!(GradeFile::parse(&u).is_err());
}

#[test]
fn input_unknown_status_or_tier_refused() {
    assert!(GradeFile::parse(&text("alpha.toml").replacen("status = \"met\"", "status = \"partial\"", 1)).is_err());
    assert!(Registry::parse(&text("registry.toml").replace("tier = \"core\"", "tier = \"optional\"")).is_err());
}

#[test]
fn input_duplicate_registry_id_refused() {
    let dup = "\n[[capability]]\nid = \"A-3\"\nname = \"Again\"\ntier = \"standard\"\nstandard = \"s\"\n\n[[capability.criterion]]\nid = \"A-3.9\"\ntext = \"t\"\n";
    assert!(Registry::parse(&(text("registry.toml") + dup)).unwrap_err().contains("A-3"));
    let dup = dup.replace("\"A-3\"", "\"A-4\"").replace("A-3.9", "A-3.1");
    assert!(Registry::parse(&(text("registry.toml") + &dup)).unwrap_err().contains("A-3.1"));
}

#[test]
fn check_cfg_unix_test_counts_with_platform() {
    let p = |f: &str| evidence(&repo(), &format!("test:crates/demo/tests/ev.rs::{f}")).unwrap().platforms;
    assert_eq!(p("unix_only"), ["unix"]);
    assert_eq!(p("in_windows_mod"), ["windows"]);
    assert!(p("in_cfg_test_mod").is_empty());
    assert!(p("plain").is_empty());
}

#[test]
fn check_tokio_multi_thread_test_counts() {
    for f in ["tokio_multi", "tokio_plain"] {
        assert_eq!(evidence(&repo(), &format!("test:crates/demo/tests/ev.rs::{f}")), Ok(Evidence { platforms: vec![] }));
    }
}

#[cfg(unix)]
#[test]
fn input_evidence_link_leaving_repo_refused() {
    let (d, out) = (copy_repo(), tempfile::tempdir().unwrap());
    std::fs::write(out.path().join("x.json"), "{\"outside\": true}\n").unwrap();
    std::fs::copy(repo().join("crates/demo/tests/ev.rs"), out.path().join("ev.rs")).unwrap();
    std::os::unix::fs::symlink(out.path().join("x.json"), d.path().join("fixtures/beta/out.json")).unwrap();
    std::os::unix::fs::symlink(out.path().join("ev.rs"), d.path().join("crates/demo/tests/link.rs")).unwrap();
    assert!(evidence(d.path(), "fixture:fixtures/beta/out.json").unwrap_err().contains("leaves the repository"));
    assert!(evidence(d.path(), "test:crates/demo/tests/link.rs::plain").unwrap_err().contains("leaves the repository"));
}

#[cfg(unix)]
#[test]
fn check_3_symlink_fixture_not_evidence() {
    let d = copy_repo();
    std::os::unix::fs::symlink("capture.json", d.path().join("fixtures/beta/link.json")).unwrap();
    assert!(evidence(d.path(), "fixture:fixtures/beta/link.json").is_err());
    assert!(evidence(d.path(), "fixture:fixtures/beta/capture.json").is_ok());
}

#[test]
fn check_valid_fixtures_pass() {
    let (r, g, c) = base();
    assert_eq!(check(&repo(), &r, &g, &c), vec![]);
}

#[test]
fn check_1_client_without_grade_file() {
    let (r, g, mut c) = base();
    c.push(client("gamma", "Gamma", "3.0.0"));
    let f = check(&repo(), &r, &g, &c);
    assert_eq!(items(&f), set(&[1, 9]));
    assert!(f[0].text.contains("gamma"));
}

#[test]
fn check_1_grade_file_unknown_or_twice() {
    let (r, g, c) = base();
    let mut d = g.clone();
    let mut x = g[1].clone();
    x.client = "delta".into();
    d.push(x);
    assert_eq!(items(&check(&repo(), &r, &d, &c)), set(&[1]));
    let mut d = g.clone();
    d.push(g[0].clone());
    assert_eq!(items(&check(&repo(), &r, &d, &c)), set(&[1]));
}

#[test]
fn check_2_missing_grade() {
    let (r, mut g, c) = base();
    g[0].grade.retain(|e| e.criterion != "A-3.1");
    assert_eq!(items(&check(&repo(), &r, &g, &c)), set(&[2]));
}

#[test]
fn check_2_unknown_criterion() {
    let (r, mut g, c) = base();
    let mut e = entry(&mut g[0], "A-1.1").clone();
    e.criterion = "A-9.1".into();
    g[0].grade.push(e);
    assert_eq!(items(&check(&repo(), &r, &g, &c)), set(&[2]));
}

#[test]
fn check_2_duplicate_grade() {
    let (r, mut g, c) = base();
    let e = entry(&mut g[0], "A-3.1").clone();
    g[0].grade.push(e);
    assert_eq!(items(&check(&repo(), &r, &g, &c)), set(&[2]));
}

#[test]
fn check_3_ignored_test_not_evidence() {
    for f in ["ignored_plain", "ignored_reason", "ignored_cfg_attr"] {
        assert_eq!(with_evidence(&[&format!("test:crates/demo/tests/ev.rs::{f}")]), set(&[3]), "{f}");
    }
}

#[test]
fn check_3_missing_or_plain_function_not_evidence() {
    for e in [
        "test:crates/demo/tests/ev.rs::no_such_fn",
        "test:crates/demo/tests/ev.rs::helper_not_test",
        "test:crates/demo/tests/ev.rs::dup",
        "test:crates/demo/tests/none.rs::plain",
    ] {
        assert_eq!(with_evidence(&[e]), set(&[3]), "{e}");
    }
    assert_eq!(with_evidence(&[]), set(&[3]));
    assert_eq!(with_evidence(&["test:crates/demo/tests/ev.rs::no_such_fn", "test:crates/demo/tests/ev.rs::plain"]), set(&[]));
}

#[test]
fn check_3_cfg_test_fn_in_other_file_not_evidence() {
    assert_eq!(with_evidence(&["test:crates/demo/tests/ev.rs::only_in_lib"]), set(&[3]));
    assert_eq!(with_evidence(&["test:crates/demo/src/lib.rs::only_in_lib"]), set(&[]));
}

#[test]
fn check_3_premise_failed_or_observed_not_evidence() {
    assert_eq!(with_evidence(&["premise:docs/premises.md#Q2"]), set(&[3]));
    assert_eq!(with_evidence(&["premise:docs/premises.md#Q3"]), set(&[3]));
    assert_eq!(with_evidence(&["premise:docs/premises.md#Q1"]), set(&[]));
}

#[test]
fn check_3_premise_id_in_other_file_only() {
    assert_eq!(with_evidence(&["premise:docs/premises.md#L1"]), set(&[3]));
    assert_eq!(with_evidence(&["premise:docs/live-checks.md#Q1"]), set(&[3]));
}

#[test]
fn check_3_premise_not_run_absent_or_twice_not_evidence() {
    for e in ["premise:docs/live-checks.md#L2", "premise:docs/premises.md#Q8", "premise:docs/premises.md#Q9"] {
        assert_eq!(with_evidence(&[e]), set(&[3]), "{e}");
    }
}

#[test]
fn check_3_trivial_fixture_not_evidence() {
    for e in [
        "fixture:fixtures/beta/empty.json",
        "fixture:fixtures/beta/list.json",
        "fixture:fixtures/beta/tiny.txt",
        "fixture:fixtures/beta/missing.json",
        "fixture:fixtures/beta",
    ] {
        assert_eq!(with_evidence(&[e]), set(&[3]), "{e}");
    }
    assert_eq!(with_evidence(&["fixture:fixtures/beta/capture.json"]), set(&[]));
}

#[test]
fn check_4_reason_and_core_na() {
    let (r, g0, c) = base();
    let mut g = g0.clone();
    entry(&mut g[0], "WA-2.1").reason = None;
    assert_eq!(items(&check(&repo(), &r, &g, &c)), set(&[4]));
    let mut g = g0.clone();
    entry(&mut g[1], "A-3.1").reason = Some(" ".into());
    assert_eq!(items(&check(&repo(), &r, &g, &c)), set(&[4]));
    let mut g = g0.clone();
    {
        let e = entry(&mut g[0], "A-1.1");
        e.status = Status::Na;
        e.reason = Some("x".into());
    }
    assert_eq!(items(&check(&repo(), &r, &g, &c)), set(&[4, 8]));
}

#[test]
fn check_5_cell_url() {
    let (r, g0, c) = base();
    let mut g = g0.clone();
    g[0].cell.retain(|x| x.capability != "A-3");
    assert_eq!(items(&check(&repo(), &r, &g, &c)), set(&[5]));
    let mut g = g0.clone();
    g[0].cell.iter_mut().find(|x| x.capability == "A-3").unwrap().url = "http://example.com".into();
    assert_eq!(items(&check(&repo(), &r, &g, &c)), set(&[5]));
}

#[test]
fn check_6_bar_exception_faults() {
    let x = |client: &str, criterion: &str, approved: Option<&str>| BarException {
        client: client.into(),
        criterion: criterion.into(),
        reason: "r".into(),
        approved: approved.map(Into::into),
    };
    let (r0, g0, c) = base();
    for b in [x("gamma", "A-1.2", Some("operator")), x("alpha", "A-1.1", Some("operator")), x("beta", "A-3.1", Some("operator"))] {
        let mut r = r0.clone();
        r.bar_exception.push(b);
        assert_eq!(items(&check(&repo(), &r, &g0, &c)), set(&[6]));
    }
    let mut g = g0.clone();
    {
        let e = entry(&mut g[0], "A-1.2");
        e.status = Status::NotMet;
        e.reason = Some("no helper".into());
    }
    for a in [None, Some(" ")] {
        let mut r = r0.clone();
        r.bar_exception.push(x("alpha", "A-1.2", a));
        assert_eq!(items(&check(&repo(), &r, &g, &c)), set(&[6]));
    }
}

#[test]
fn check_6_bar_exception_key_in_grade_file() {
    assert!(GradeFile::load(&fx().join("grade-with-bar-exception.toml")).unwrap_err().contains("bar_exception"));
}

#[test]
fn check_7_version_faults() {
    let (r, g, mut c) = base();
    c[0].version = "1.0.1".into();
    let f = check(&repo(), &r, &g, &c);
    assert_eq!(items(&f), set(&[7]));
    assert!(f[0].text.contains("alpha"));
    let (_, _, c) = base();
    for (file, from, to) in [
        ("docs/premises.md", "Alpha: 1.0.0\n", "Alpha: 1.0.2\n"),
        ("docs/live-checks.md", "Alpha: 1.0.0 or later", "Alpha: 1.0.0"),
        ("docs/premises.md", "Beta: 2.0.0\n", ""),
    ] {
        let d = copy_repo();
        let p = d.path().join(file);
        std::fs::write(&p, std::fs::read_to_string(&p).unwrap().replace(from, to)).unwrap();
        assert_eq!(items(&check(d.path(), &r, &g, &c)), set(&[7]), "{file} {from}");
    }
}

#[test]
fn check_8_supported_client_below_bar() {
    let (r, mut g, mut c) = base();
    {
        let e = entry(&mut g[0], "A-1.2");
        e.status = Status::NotMet;
        e.reason = Some("no key helper".into());
    }
    let f = check(&repo(), &r, &g, &c);
    assert_eq!(items(&f), set(&[8]));
    assert!(f[0].text.contains("A-1"));
    c[0].supported = false;
    assert_eq!(check(&repo(), &r, &g, &c), vec![]);
}

#[test]
fn check_8_registry_exception_meets_bar() {
    let (mut r, mut g, c) = base();
    {
        let e = entry(&mut g[0], "A-1.2");
        e.status = Status::NotMet;
        e.reason = Some("no key helper".into());
    }
    r.bar_exception.push(BarException {
        client: "alpha".into(),
        criterion: "A-1.2".into(),
        reason: "r".into(),
        approved: Some("operator 2026-11-25".into()),
    });
    assert_eq!(check(&repo(), &r, &g, &c), vec![]);
}

#[test]
fn check_9_fixtures_folder_missing_or_empty() {
    let (r, g, c) = base();
    let d = copy_repo();
    std::fs::remove_dir_all(d.path().join("fixtures/alpha")).unwrap();
    assert_eq!(items(&check(d.path(), &r, &g, &c)), set(&[9]));
    std::fs::create_dir(d.path().join("fixtures/alpha")).unwrap();
    assert_eq!(items(&check(d.path(), &r, &g, &c)), set(&[9]));
}

#[test]
fn check_clients_from_agent_all() {
    let c = clients(&repo()).unwrap();
    assert_eq!(
        c,
        vec![
            Client { id: "claude-code".into(), name: "Claude Code".into(), version: "9.1.0".into(), supported: true },
            Client { id: "opencode".into(), name: "OpenCode".into(), version: "9.2.0".into(), supported: true },
        ]
    );
    assert!(clients(tempfile::tempdir().unwrap().path()).is_err());
}
