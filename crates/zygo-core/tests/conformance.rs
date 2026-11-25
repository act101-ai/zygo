//! The conformance check and generator (K§6.2.5). CF-1: the input model and check items 1-9 on fixture inputs.
#[allow(dead_code)] // later nodes read the rest of the model
#[path = "conformance/model.rs"]
mod model;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use model::*;
use zygo_core::Agent;

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
fn check_agent_all() {
    assert_eq!(Agent::ALL, [Agent::ClaudeCode, Agent::OpenCode]);
    for a in Agent::ALL {
        assert_eq!(a.as_str().parse::<Agent>().unwrap(), a);
    }
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
