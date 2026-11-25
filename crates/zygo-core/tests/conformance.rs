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
    items(&check(&repo(), &r, &g, &c, &resolve(&repo(), &g)))
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
        assert!(evidence(&repo(), bad, "Alpha").is_err(), "{bad}");
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
fn input_criterion_outside_its_capability_refused() {
    let e = Registry::parse(&text("registry.toml").replace("\"A-3.1\"", "\"A-7.1\"")).unwrap_err();
    assert!(e.contains("A-7.1"), "{e}");
    let e = Registry::parse(&text("registry.toml").replace("\"A-3.1\"", "\"WA-3.1\"")).unwrap_err();
    assert!(e.contains("WA-3.1"), "{e}");
    let e = Registry::parse(&text("registry.toml").replace("\"A-3.1\"", "\"A-3\"").replace("id = \"A-3\"\n", "id = \"A-30\"\n")).unwrap_err();
    assert!(e.contains("A-3"), "{e}");
    let e = Registry::parse(&text("registry.toml").replace("id = \"A-3\"\n", "id = \"A-3.5\"\n")).unwrap_err();
    assert!(e.contains("A-3.5"), "{e}");
    let e = GradeFile::parse(&text("alpha.toml").replace("capability = \"A-3\"", "capability = \"A-3.1\"")).unwrap_err();
    assert!(e.contains("A-3.1"), "{e}");
}

#[test]
fn input_empty_registry_or_capability_refused() {
    assert!(Registry::parse("capability = []\n").is_err());
    assert!(Registry::parse("").is_err());
    let t = "[[capability]]\nid = \"A-1\"\nname = \"n\"\ntier = \"core\"\nstandard = \"s\"\ncriterion = []\n";
    assert!(Registry::parse(t).unwrap_err().contains("A-1"));
}

#[test]
fn input_control_characters_refused() {
    for bad in ["\\u0000", "\\u0007", "\\t", "\\u2028", "\\u2029", "\\u0085", "\\u001b"] {
        let reg = text("registry.toml");
        let grade = text("alpha.toml");
        for t in [
            reg.replace("Turn start.", &format!("Turn{bad}start.")),
            reg.replace("zygo sees each turn.", &format!("zygo{bad}sees")),
            reg.replace("\"Turn signals\"", &format!("\"Turn{bad}signals\"")),
            reg.clone() + &format!("\n[[bar_exception]]\nclient = \"alpha\"\ncriterion = \"A-1.2\"\nreason = \"r\"\napproved = \"op{bad}x\"\n"),
            reg.clone() + &format!("\n[[bar_exception]]\nclient = \"alpha\"\ncriterion = \"A-1.2\"\nreason = \"r{bad}x\"\napproved = \"op\"\n"),
        ] {
            assert!(Registry::parse(&t).is_err(), "{bad}: {t}");
        }
        for t in [
            grade.replace("Alpha has no turn events", &format!("Alpha{bad}has")),
            grade.replace("alpha/models", &format!("alpha/{bad}models")),
            grade.replace("name = \"Alpha\"", &format!("name = \"Al{bad}pha\"")),
            grade.replace("#Q1", &format!("#Q{bad}1")),
        ] {
            assert!(GradeFile::parse(&t).is_err(), "{bad}: {t}");
        }
    }
}

#[test]
fn input_file_over_1_mib_refused() {
    let d = tempfile::tempdir().unwrap();
    let pad = format!("# {}\n", "x".repeat(1 << 20));
    for name in ["registry.toml", "alpha.toml"] {
        std::fs::write(d.path().join(name), text(name) + &pad).unwrap();
    }
    assert!(Registry::load(&d.path().join("registry.toml")).unwrap_err().contains("1 MiB"));
    assert!(GradeFile::load(&d.path().join("alpha.toml")).unwrap_err().contains("1 MiB"));
}

const EVERY: [&str; 3] = ["linux", "macos", "windows"];

#[test]
fn check_cfg_unix_test_counts_with_platform() {
    let p = |f: &str| evidence(&repo(), &format!("test:crates/demo/tests/ev.rs::{f}"), "Alpha").unwrap().platforms;
    assert_eq!(p("unix_only"), ["linux", "macos"]);
    assert_eq!(p("in_windows_mod"), ["windows"]);
    assert_eq!(p("in_cfg_test_mod"), EVERY);
    assert_eq!(p("plain"), EVERY);
}

#[test]
fn check_tokio_multi_thread_test_counts() {
    for f in ["tokio_multi", "tokio_plain"] {
        let e = evidence(&repo(), &format!("test:crates/demo/tests/ev.rs::{f}"), "Alpha");
        assert_eq!(e, Ok(Evidence { platforms: EVERY.map(String::from).to_vec() }));
    }
}

#[test]
fn check_cfg_predicates_evaluated_for_the_ci_targets() {
    let d = test_target(&[
        "#[cfg(any())] mod never { #[test] fn dead() {} }",
        "#[cfg(all(test, unix))] #[test] fn both() {}",
        "#[cfg(target_os = \"windows\")] #[test] fn tos() {}",
        "#[cfg_attr(unix, test)] fn ca() {}",
        "#[cfg_attr(feature = \"x\", test)] fn ca_feature() {}",
        "#[cfg(feature = \"x\")] #[test] fn feature() {}",
        "#[cfg(miri)] #[test] fn miri() {}",
        "#[test] #[cfg_attr(miri, ignore)] fn miri_ignore() {}",
        "#[cfg(test)] #[cfg(unix)] #[test] fn two() {}",
        "#[cfg(not(windows))] #[test] fn not_windows() {}",
        "#[cfg(any(target_os = \"linux\", target_family = \"windows\"))] #[test] fn mix() {}",
        "#[cfg(target_os = \"freebsd\")] #[test] fn bsd() {}",
        "#[cfg(all())] mod always { #[cfg(target_os = \"macos\")] mod mac { #[tokio::test] async fn deep() {} } }",
    ]);
    let ev = |f: &str| evidence(d.path(), &format!("test:crates/x/tests/t.rs::{f}"), "Alpha").map(|e| e.platforms);
    assert_eq!(ev("both").unwrap(), ["linux", "macos"]);
    assert_eq!(ev("tos").unwrap(), ["windows"]);
    assert_eq!(ev("ca").unwrap(), ["linux", "macos"]);
    assert_eq!(ev("two").unwrap(), ["linux", "macos"]);
    assert_eq!(ev("not_windows").unwrap(), ["linux", "macos"]);
    assert_eq!(ev("mix").unwrap(), ["linux", "windows"]);
    assert_eq!(ev("deep").unwrap(), ["macos"]);
    for f in ["dead", "bsd"] {
        assert!(ev(f).unwrap_err().contains("never runs in CI"), "{f}");
    }
    for f in ["feature", "ca_feature", "miri", "miri_ignore"] {
        assert!(ev(f).unwrap_err().contains("not provable"), "{f}");
    }
}

/// Write `src` as the test target `crates/x/tests/t.rs` of a new temporary repository, with a minimal `crates/x/Cargo.toml`.
fn test_target(src: &[&str]) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join("crates/x/tests")).unwrap();
    std::fs::write(d.path().join("crates/x/Cargo.toml"), "[package]\nname = \"x\"\nversion = \"0.1.0\"\n").unwrap();
    std::fs::write(d.path().join("crates/x/tests/t.rs"), src.join("\n")).unwrap();
    d
}

#[test]
fn input_deeply_nested_test_source_refused() {
    // Deep nesting overflows the parser's stack; the check refuses it with an error that names the file.
    for (open, close) in [("(", ")"), ("{", "}"), ("[", "]")] {
        let body = format!("{}0{}", open.repeat(100_000), close.repeat(100_000));
        let d = test_target(&["#[test] fn plain() {}", &format!("fn deep() {{ let _ = {body}; }}")]);
        let e = evidence(d.path(), "test:crates/x/tests/t.rs::plain", "Alpha").unwrap_err();
        assert!(e.contains("crates/x/tests/t.rs") && e.contains("nested"), "{e}");
    }
    // Nesting at the cap of 256 parses: 255 brackets inside the fn body's `{`.
    for (open, close) in [("(", ")"), ("{", "}"), ("[", "]")] {
        let body = format!("{}0{}", open.repeat(255), close.repeat(255));
        let d = test_target(&["#[test] fn plain() {}", &format!("fn deep() {{ let _ = {body}; }}")]);
        assert!(evidence(d.path(), "test:crates/x/tests/t.rs::plain", "Alpha").is_ok(), "{open}");
    }
}

#[test]
fn input_long_toml_error_clipped() {
    let bad = format!("{}\n", "x".repeat(10_000));
    for e in [Registry::parse(&bad).unwrap_err(), GradeFile::parse(&bad).unwrap_err()] {
        assert!(e.chars().count() <= 301, "{} chars", e.chars().count());
    }
}

#[test]
fn check_cfg_attr_cfg_and_nested_cfg_attr_evaluated() {
    let d = test_target(&[
        "#[cfg_attr(unix, cfg(windows))] #[test] fn ca_cfg() {}",
        "#[cfg_attr(any(), cfg(windows))] mod m { #[test] fn in_m() {} }",
        "#[cfg_attr(windows, cfg(unix))] mod m2 { #[test] fn in_m2() {} }",
        "#[test] #[cfg_attr(unix, cfg_attr(all(), ignore))] fn nested_ignore() {}",
        "#[cfg_attr(all(), cfg_attr(unix, test))] fn nested_test() {}",
        "#[test] #[cfg_attr(all(), cfg_attr(unix, cfg(windows)))] fn nested_cfg() {}",
        "#[test] #[cfg_attr(feature = \"x\", allow(unused))] fn harmless() {}",
    ]);
    let ev = |f: &str| evidence(d.path(), &format!("test:crates/x/tests/t.rs::{f}"), "Alpha").map(|e| e.platforms);
    // `cfg_attr(p, cfg(q))` keeps the item where `p` does not hold or `q` holds.
    assert_eq!(ev("ca_cfg").unwrap(), ["windows"]);
    assert_eq!(ev("in_m").unwrap(), EVERY);
    assert_eq!(ev("in_m2").unwrap(), ["linux", "macos"]);
    assert_eq!(ev("nested_test").unwrap(), ["linux", "macos"]);
    assert_eq!(ev("nested_cfg").unwrap(), ["windows"]);
    assert_eq!(ev("harmless").unwrap(), EVERY);
    assert!(ev("nested_ignore").unwrap_err().contains("is ignored"));
}

#[test]
fn check_3_premise_of_another_client_not_evidence() {
    assert!(evidence(&repo(), "premise:docs/premises.md#Q1", "Alpha").is_ok());
    assert!(evidence(&repo(), "premise:docs/premises.md#Q1", "Beta").unwrap_err().contains("Alpha"));
    let (r, mut g, c) = base();
    entry(&mut g[1], "A-1.2").evidence = vec!["premise:docs/premises.md#Q1".into()];
    assert_eq!(items(&check(&repo(), &r, &g, &c, &resolve(&repo(), &g))), set(&[3]));
}

#[test]
fn check_3_test_file_no_test_target_compiles_not_evidence() {
    let ev = |e: &str| evidence(&repo(), e, "Alpha");
    // Reached from the test target tests/ev.rs by `#[path]`, then by `mod` from that file; from src/lib.rs by `mod`.
    assert_eq!(ev("test:crates/demo/tests/sub/reached.rs::in_reached").unwrap().platforms, EVERY);
    assert_eq!(ev("test:crates/demo/tests/sub/deeper.rs::in_deeper").unwrap().platforms, EVERY);
    assert_eq!(ev("test:crates/demo/src/inner/deep.rs::in_src_deep").unwrap().platforms, EVERY);
    // The cfg of the `mod` that reaches the file limits its targets.
    assert_eq!(ev("test:crates/demo/tests/sub/win.rs::in_win_file").unwrap().platforms, ["windows"]);
    // No test target and no crate root reaches these files.
    for e in ["test:crates/demo/tests/sub/orphan.rs::in_orphan", "test:crates/demo/src/orphan.rs::in_src_orphan"] {
        assert!(ev(e).unwrap_err().contains("no test target compiles"), "{e}");
    }
    // A file under tests/**/fixtures/ is never evidence, even when a `mod` reaches it.
    assert!(ev("test:crates/demo/tests/fixtures/f.rs::in_fixture").unwrap_err().contains("fixtures"));
    assert_eq!(with_evidence(&["test:crates/demo/tests/sub/orphan.rs::in_orphan"]), set(&[3]));
    assert_eq!(with_evidence(&["test:crates/demo/tests/sub/reached.rs::in_reached"]), set(&[]));
}

#[test]
fn check_3_premise_of_a_joint_agent_cell() {
    // The Agent cell of Q4 is `Alpha + Beta`: the row counts for each client it names.
    for name in ["Alpha", "Beta"] {
        assert!(evidence(&repo(), "premise:docs/premises.md#Q4", name).is_ok(), "{name}");
    }
    for name in ["Gamma", "Alpha + Beta", "Alph"] {
        let e = evidence(&repo(), "premise:docs/premises.md#Q4", name).unwrap_err();
        assert!(e.contains("is for `Alpha + Beta`"), "{name}: {e}");
    }
}

#[test]
fn check_3_premise_or_test_source_over_1_mib_not_evidence() {
    let d = copy_repo();
    let pad = |p: &Path, line: &str| {
        let t = std::fs::read_to_string(p).unwrap() + &line.repeat((1 << 20) / line.len() + 1);
        std::fs::write(p, t).unwrap();
    };
    pad(&d.path().join("docs/premises.md"), "<!-- padding -->\n");
    pad(&d.path().join("crates/demo/tests/ev.rs"), "// padding\n");
    assert!(evidence(d.path(), "premise:docs/premises.md#Q1", "Alpha").unwrap_err().contains("1 MiB"));
    assert!(evidence(d.path(), "test:crates/demo/tests/ev.rs::plain", "Alpha").unwrap_err().contains("1 MiB"));
    let (r, g, c) = base();
    assert_eq!(items(&check(d.path(), &r, &g, &c, &resolve(d.path(), &g))), set(&[3, 7]));
}

#[test]
fn check_resolve_holds_met_evidence_with_platforms() {
    let (_, g, _) = base();
    let res = resolve(&repo(), &g);
    let met: Vec<(&str, &str)> = res.grade.iter().map(|x| (x.client.as_str(), x.criterion.as_str())).collect();
    assert_eq!(met, [("alpha", "A-1.1"), ("alpha", "A-1.2"), ("alpha", "A-3.1"), ("beta", "A-1.1"), ("beta", "A-1.2"), ("beta", "WA-2.1")]);
    let wa = res.get("beta", "WA-2.1").unwrap();
    assert_eq!(wa.file, 1);
    assert_eq!(wa.evidence[0].text, "test:crates/demo/tests/ev.rs::unix_only");
    assert_eq!(wa.evidence[0].result, Ok(Evidence { platforms: vec!["linux".into(), "macos".into()] }));
    assert!(res.get("alpha", "WA-2.1").is_none());
    let mut bad = g.clone();
    entry(&mut bad[0], "A-1.1").evidence.push("test:crates/demo/tests/ev.rs::nope".into());
    let res = resolve(&repo(), &bad);
    assert!(res.get("alpha", "A-1.1").unwrap().evidence[1].result.as_ref().unwrap_err().contains("no test fn"));
}

// Symbolic-link cases run only on unix: making a link on Windows needs a privilege the CI runner does not give.
#[cfg(unix)]
#[test]
fn input_evidence_link_leaving_repo_refused() {
    let (d, out) = (copy_repo(), tempfile::tempdir().unwrap());
    std::fs::write(out.path().join("x.json"), "{\"outside\": true}\n").unwrap();
    std::fs::copy(repo().join("crates/demo/tests/ev.rs"), out.path().join("ev.rs")).unwrap();
    std::os::unix::fs::symlink(out.path().join("x.json"), d.path().join("fixtures/beta/out.json")).unwrap();
    std::os::unix::fs::symlink(out.path().join("ev.rs"), d.path().join("crates/demo/tests/link.rs")).unwrap();
    assert!(evidence(d.path(), "fixture:fixtures/beta/out.json", "Beta").unwrap_err().contains("leaves the repository"));
    assert!(evidence(d.path(), "test:crates/demo/tests/link.rs::plain", "Beta").unwrap_err().contains("leaves the repository"));
}

// Unix only, for the reason given above `input_evidence_link_leaving_repo_refused`.
#[cfg(unix)]
#[test]
fn check_3_symlink_fixture_not_evidence() {
    let d = copy_repo();
    std::os::unix::fs::symlink("capture.json", d.path().join("fixtures/beta/link.json")).unwrap();
    assert!(evidence(d.path(), "fixture:fixtures/beta/link.json", "Beta").is_err());
    assert!(evidence(d.path(), "fixture:fixtures/beta/capture.json", "Beta").is_ok());
}

// Unix only, for the reason given above `input_evidence_link_leaving_repo_refused`.
#[cfg(unix)]
#[test]
fn check_3_fixture_through_link_out_of_fixtures_not_evidence() {
    let d = copy_repo();
    std::os::unix::fs::symlink(d.path().join("crates/demo"), d.path().join("fixtures/beta/dl")).unwrap();
    let e = evidence(d.path(), "fixture:fixtures/beta/dl/tests/ev.rs", "Beta").unwrap_err();
    assert!(e.contains("leaves fixtures/"), "{e}");
}

// Unix only, for the reason given above `input_evidence_link_leaving_repo_refused`.
#[cfg(unix)]
#[test]
fn check_3_fixture_through_dir_link_inside_fixtures_not_evidence() {
    let d = copy_repo();
    // The link and its target are both in fixtures/: a link in any component is refused, not only the last.
    std::os::unix::fs::symlink("../alpha", d.path().join("fixtures/beta/dl")).unwrap();
    std::os::unix::fs::symlink("capture.json", d.path().join("fixtures/beta/link.json")).unwrap();
    assert!(evidence(d.path(), "fixture:fixtures/alpha/a.json", "Beta").is_ok());
    for (e, link) in [("fixture:fixtures/beta/dl/a.json", "fixtures/beta/dl"), ("fixture:fixtures/beta/link.json", "fixtures/beta/link.json")] {
        let err = evidence(d.path(), e, "Beta").unwrap_err();
        assert!(err.contains(&format!("{link} is a symbolic link")), "{err}");
    }
}

#[test]
fn check_3_fixture_over_1_mib_not_evidence() {
    let d = copy_repo();
    std::fs::write(d.path().join("fixtures/beta/big.json"), format!("[{}]", "1,".repeat(1 << 19))).unwrap();
    let e = evidence(d.path(), "fixture:fixtures/beta/big.json", "Beta").unwrap_err();
    assert!(e.contains("fixtures/beta/big.json") && e.contains("1 MiB"), "{e}");
}

#[test]
fn check_7_version_line_after_the_first_heading_does_not_count() {
    let (r, g, c) = base();
    let d = copy_repo();
    let p = d.path().join("docs/premises.md");
    let text = std::fs::read_to_string(&p).unwrap().replace("Alpha: 1.0.0\n", "") + "Alpha: 1.0.0\n";
    std::fs::write(&p, text).unwrap();
    let f = check(d.path(), &r, &g, &c, &resolve(d.path(), &g));
    assert_eq!(items(&f), set(&[7]));
    assert!(f[0].text.contains("Alpha: 1.0.0"), "{f:?}");
}

#[test]
fn input_evidence_path_with_colon_refused() {
    for e in ["fixture:fixtures/beta/a:b.json", "test:crates/demo/tests/e:v.rs::plain"] {
        assert!(evidence(&repo(), e, "Beta").unwrap_err().contains("not a plain repository-relative path"), "{e}");
        let t = text("beta.toml").replace("fixture:fixtures/beta/capture.json", e);
        assert!(GradeFile::parse(&t).unwrap_err().contains("not a plain repository-relative path"), "{e}");
    }
}

#[test]
fn check_valid_fixtures_pass() {
    let (r, g, c) = base();
    assert_eq!(check(&repo(), &r, &g, &c, &resolve(&repo(), &g)), vec![]);
}

#[test]
fn check_1_client_without_grade_file() {
    let (r, g, mut c) = base();
    c.push(client("gamma", "Gamma", "3.0.0"));
    let f = check(&repo(), &r, &g, &c, &resolve(&repo(), &g));
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
    assert_eq!(items(&check(&repo(), &r, &d, &c, &resolve(&repo(), &d))), set(&[1]));
    let mut d = g.clone();
    d.push(g[0].clone());
    assert_eq!(items(&check(&repo(), &r, &d, &c, &resolve(&repo(), &d))), set(&[1]));
}

#[test]
fn check_2_missing_grade() {
    let (r, mut g, c) = base();
    g[0].grade.retain(|e| e.criterion != "A-3.1");
    assert_eq!(items(&check(&repo(), &r, &g, &c, &resolve(&repo(), &g))), set(&[2]));
}

#[test]
fn check_2_unknown_criterion() {
    let (r, mut g, c) = base();
    let mut e = entry(&mut g[0], "A-1.1").clone();
    e.criterion = "A-9.1".into();
    g[0].grade.push(e);
    assert_eq!(items(&check(&repo(), &r, &g, &c, &resolve(&repo(), &g))), set(&[2]));
}

#[test]
fn check_2_duplicate_grade() {
    let (r, mut g, c) = base();
    let e = entry(&mut g[0], "A-3.1").clone();
    g[0].grade.push(e);
    assert_eq!(items(&check(&repo(), &r, &g, &c, &resolve(&repo(), &g))), set(&[2]));
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
    assert_eq!(items(&check(&repo(), &r, &g, &c, &resolve(&repo(), &g))), set(&[4]));
    let mut g = g0.clone();
    entry(&mut g[1], "A-3.1").reason = Some(" ".into());
    assert_eq!(items(&check(&repo(), &r, &g, &c, &resolve(&repo(), &g))), set(&[4]));
    let mut g = g0.clone();
    {
        let e = entry(&mut g[0], "A-1.1");
        e.status = Status::Na;
        e.reason = Some("x".into());
    }
    assert_eq!(items(&check(&repo(), &r, &g, &c, &resolve(&repo(), &g))), set(&[4, 8]));
}

#[test]
fn check_5_cell_url() {
    let (r, g0, c) = base();
    let mut g = g0.clone();
    g[0].cell.retain(|x| x.capability != "A-3");
    assert_eq!(items(&check(&repo(), &r, &g, &c, &resolve(&repo(), &g))), set(&[5]));
    let mut g = g0.clone();
    g[0].cell.iter_mut().find(|x| x.capability == "A-3").unwrap().url = "http://example.com".into();
    assert_eq!(items(&check(&repo(), &r, &g, &c, &resolve(&repo(), &g))), set(&[5]));
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
        assert_eq!(items(&check(&repo(), &r, &g0, &c, &resolve(&repo(), &g0))), set(&[6]));
    }
    let mut g = g0.clone();
    {
        let e = entry(&mut g[0], "A-1.2");
        e.status = Status::NotMet;
        e.reason = Some("no helper".into());
    }
    // An exception with no approval is a fault and does not count for the bar.
    for a in [None, Some(" ")] {
        let mut r = r0.clone();
        r.bar_exception.push(x("alpha", "A-1.2", a));
        assert_eq!(items(&check(&repo(), &r, &g, &c, &resolve(&repo(), &g))), set(&[6, 8]));
    }
}

#[test]
fn check_6_duplicate_or_reasonless_bar_exception() {
    let x = |reason: &str| BarException {
        client: "alpha".into(),
        criterion: "A-1.2".into(),
        reason: reason.into(),
        approved: Some("operator".into()),
    };
    let (r0, mut g, c) = base();
    {
        let e = entry(&mut g[0], "A-1.2");
        e.status = Status::NotMet;
        e.reason = Some("no helper".into());
    }
    let mut r = r0.clone();
    r.bar_exception.push(x("r"));
    assert_eq!(items(&check(&repo(), &r, &g, &c, &resolve(&repo(), &g))), set(&[]));
    r.bar_exception.push(x("again"));
    let f = check(&repo(), &r, &g, &c, &resolve(&repo(), &g));
    assert_eq!(items(&f), set(&[6]));
    assert!(f[0].text.contains("A-1.2"));
    for reason in ["", " "] {
        let mut r = r0.clone();
        r.bar_exception.push(x(reason));
        assert_eq!(items(&check(&repo(), &r, &g, &c, &resolve(&repo(), &g))), set(&[6]), "{reason:?}");
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
    let f = check(&repo(), &r, &g, &c, &resolve(&repo(), &g));
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
        assert_eq!(items(&check(d.path(), &r, &g, &c, &resolve(d.path(), &g))), set(&[7]), "{file} {from}");
    }
}

#[test]
fn check_7_grade_file_name_differs_from_client_name() {
    let (r, g0, c) = base();
    let mut g = g0.clone();
    g[0].name = "Beta".into();
    // The Alpha premise row Q1 no longer counts for a file named Beta (item 3).
    let f = check(&repo(), &r, &g, &c, &resolve(&repo(), &g));
    assert_eq!(items(&f), set(&[3, 7]));
    assert!(f.iter().any(|x| x.item == 7 && x.text.contains("alpha") && x.text.contains("Beta")));
    // The headers are built from the client's name: `Beta: 2.0.0` is in both files, `Alpha: 2.0.0` is not.
    let mut c2 = c.clone();
    c2[0].version = "2.0.0".into();
    g[0].version = "2.0.0".into();
    let f = check(&repo(), &r, &g, &c2, &resolve(&repo(), &g));
    assert_eq!(f.iter().filter(|x| x.item == 7).count(), 3, "{f:?}");
}

#[test]
fn check_5_duplicate_or_unknown_cell() {
    let (r, g0, c) = base();
    let mut g = g0.clone();
    g[0].cell.push(CellUrl { capability: "A-3".into(), url: "https://example.com/again".into() });
    let f = check(&repo(), &r, &g, &c, &resolve(&repo(), &g));
    assert_eq!(items(&f), set(&[5]));
    assert!(f[0].text.contains("A-3"));
    let mut g = g0.clone();
    g[0].cell.push(CellUrl { capability: "A-99".into(), url: "https://example.com/x".into() });
    let f = check(&repo(), &r, &g, &c, &resolve(&repo(), &g));
    assert_eq!(items(&f), set(&[5]));
    assert!(f[0].text.contains("A-99"));
}

#[test]
fn check_8_supported_client_below_bar() {
    let (r, mut g, mut c) = base();
    {
        let e = entry(&mut g[0], "A-1.2");
        e.status = Status::NotMet;
        e.reason = Some("no key helper".into());
    }
    let f = check(&repo(), &r, &g, &c, &resolve(&repo(), &g));
    assert_eq!(items(&f), set(&[8]));
    assert!(f[0].text.contains("A-1"));
    c[0].supported = false;
    assert_eq!(check(&repo(), &r, &g, &c, &resolve(&repo(), &g)), vec![]);
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
    assert_eq!(check(&repo(), &r, &g, &c, &resolve(&repo(), &g)), vec![]);
}

#[test]
fn check_9_fixtures_folder_missing_or_empty() {
    let (r, g, c) = base();
    let d = copy_repo();
    std::fs::remove_dir_all(d.path().join("fixtures/alpha")).unwrap();
    assert_eq!(items(&check(d.path(), &r, &g, &c, &resolve(d.path(), &g))), set(&[9]));
    std::fs::create_dir(d.path().join("fixtures/alpha")).unwrap();
    assert_eq!(items(&check(d.path(), &r, &g, &c, &resolve(d.path(), &g))), set(&[9]));
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

#[test]
fn check_3_reads_the_resolved_evidence() {
    let (r, g, c) = base();
    let mut res = resolve(&repo(), &g);
    assert_eq!(check(&repo(), &r, &g, &c, &res), vec![]);
    for x in &mut res.grade {
        if (x.client.as_str(), x.criterion.as_str()) == ("alpha", "A-3.1") {
            for e in &mut x.evidence {
                e.result = Err("stub: does not count".into());
            }
        }
    }
    let f = check(&repo(), &r, &g, &c, &res);
    assert_eq!(items(&f), set(&[3]));
    assert!(f[0].text.contains("stub: does not count"), "{f:?}");
}

/// A copy of the fixture repository with each `(path, text)` of `files` written into it.
fn repo_with(files: &[(&str, &str)]) -> tempfile::TempDir {
    let d = copy_repo();
    for (p, t) in files {
        let f = d.path().join(p);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, t).unwrap();
    }
    d
}

#[test]
fn check_3_bom_trivial_fixture_not_evidence() {
    let d = repo_with(&[]);
    std::fs::write(d.path().join("fixtures/alpha/bom.json"), b"\xEF\xBB\xBF{}").unwrap();
    std::fs::write(d.path().join("fixtures/alpha/bom-list.json"), b"\xEF\xBB\xBF [ ]\n").unwrap();
    for e in ["fixture:fixtures/alpha/bom.json", "fixture:fixtures/alpha/bom-list.json"] {
        assert!(evidence(d.path(), e, "Alpha").unwrap_err().contains("trivial"), "{e}");
    }
}

#[test]
fn check_3_met_grade_not_resolved_faults() {
    let (r, g, c) = base();
    let mut res = resolve(&repo(), &g);
    res.grade.retain(|x| (x.client.as_str(), x.criterion.as_str()) != ("alpha", "A-3.1"));
    let f = check(&repo(), &r, &g, &c, &res);
    assert_eq!(items(&f), set(&[3]));
    assert!(f[0].text.contains("alpha") && f[0].text.contains("A-3.1") && f[0].text.contains("evidence not resolved"), "{f:?}");
    let f = check(&repo(), &r, &g, &c, &Resolved { grade: vec![] });
    let met = g.iter().flat_map(|g| &g.grade).filter(|e| e.status == Status::Met).count();
    assert_eq!(f.iter().filter(|x| x.item == 3).count(), met, "{f:?}");
}

#[test]
fn check_3_file_inner_cfg_limits_targets() {
    let d = repo_with(&[
        ("crates/demo/tests/gone.rs", "#![cfg(any())]\n#[test]\nfn never() {}\n"),
        ("crates/demo/tests/unixonly.rs", "#![cfg(unix)]\n#[test]\nfn t() {}\n"),
        ("crates/demo/tests/root2.rs", "mod m2;\n"),
        ("crates/demo/tests/m2.rs", "#![cfg(windows)]\n#[test]\nfn t() {}\n"),
        ("crates/demo/tests/root3.rs", "#[path = \"in3/m3.rs\"]\nmod m3;\n"),
        ("crates/demo/tests/in3/m3.rs", "#![cfg(windows)]\nmod m4;\n"),
        ("crates/demo/tests/in3/m4.rs", "#[test]\nfn t4() {}\n"),
        ("crates/demo/tests/attr.rs", "#![cfg_attr(unix, cfg(any()))]\n#[test]\nfn t() {}\n"),
    ]);
    let ev = |e: &str| evidence(d.path(), e, "Alpha").map(|e| e.platforms);
    assert!(ev("test:crates/demo/tests/gone.rs::never").unwrap_err().contains("never runs in CI"));
    assert_eq!(ev("test:crates/demo/tests/unixonly.rs::t").unwrap(), ["linux", "macos"]);
    assert_eq!(ev("test:crates/demo/tests/m2.rs::t").unwrap(), ["windows"]);
    // The inner cfg of a file limits the files it reaches by `mod`.
    assert_eq!(ev("test:crates/demo/tests/in3/m4.rs::t4").unwrap(), ["windows"]);
    assert_eq!(ev("test:crates/demo/tests/attr.rs::t").unwrap(), ["windows"]);
}

#[test]
fn check_3_cfg_attr_path_resolved_per_platform() {
    // A `tests/*.rs` file is a test target of its own on every platform, so the module files sit in a subfolder.
    let d = repo_with(&[
        ("crates/demo/tests/root5.rs", "#[path = \"p5/m.rs\"]\nmod m;\n"),
        ("crates/demo/tests/p5/m.rs", "#[cfg_attr(windows, path = \"win2.rs\")]\nmod plat;\n"),
        ("crates/demo/tests/p5/plat.rs", "#[test]\nfn t() {}\n"),
        ("crates/demo/tests/p5/win2.rs", "#[test]\nfn w() {}\n"),
    ]);
    let ev = |e: &str| evidence(d.path(), e, "Alpha").map(|e| e.platforms);
    assert_eq!(ev("test:crates/demo/tests/p5/plat.rs::t").unwrap(), ["linux", "macos"]);
    assert_eq!(ev("test:crates/demo/tests/p5/win2.rs::w").unwrap(), ["windows"]);
}

#[test]
fn check_3_test_file_without_crate_manifest_not_evidence() {
    let d = repo_with(&[]);
    assert!(evidence(d.path(), "test:crates/demo/tests/ev.rs::plain", "Alpha").is_ok());
    std::fs::remove_file(d.path().join("crates/demo/Cargo.toml")).unwrap();
    let e = evidence(d.path(), "test:crates/demo/tests/ev.rs::plain", "Alpha").unwrap_err();
    assert!(e.contains("crates/demo/Cargo.toml") && e.contains("no test target compiles"), "{e}");
}
