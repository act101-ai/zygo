//! The conformance check and generator (K§6.2.5). CF-1: the input model and check items 1-9 on fixture inputs.
#[allow(dead_code)] // later nodes read the rest of the model
#[path = "conformance/model.rs"]
mod model;
#[path = "conformance/derive.rs"]
mod derive;
#[allow(dead_code)] // CF-4 calls render::compare_or_write
#[path = "conformance/render.rs"]
mod render;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use model::*;
use derive::{Bar, Cell, Counts, Grade, Scores, Unmet};

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

/// A registry and grade file from `(tier, statuses)` per capability: capability `i` is `A-{i}`, its
/// criterion `j` is `A-{i}.{j}` with text `t{i}.{j}`; a `not-met` or `n/a` grade has reason `r{i}.{j}`.
fn synth(caps: &[(Tier, &[Status])]) -> (Registry, GradeFile) {
    let mut capability = vec![];
    let mut cell = vec![];
    let mut grade = vec![];
    for (i, (tier, statuses)) in caps.iter().enumerate() {
        let i = i + 1;
        let mut criterion = vec![];
        for (j, status) in statuses.iter().enumerate() {
            let j = j + 1;
            criterion.push(Criterion { id: format!("A-{i}.{j}"), text: format!("t{i}.{j}") });
            let reason = (*status != Status::Met).then(|| format!("r{i}.{j}"));
            grade.push(GradeEntry { criterion: format!("A-{i}.{j}"), status: *status, evidence: vec![], reason });
        }
        capability.push(Capability { id: format!("A-{i}"), name: "c".into(), tier: *tier, standard: "s".into(), criterion });
        cell.push(CellUrl { capability: format!("A-{i}"), url: format!("https://example.com/{i}") });
    }
    let r = Registry { capability, bar_exception: vec![] };
    (r, GradeFile { client: "synth".into(), name: "Synth".into(), version: "1.0.0".into(), cell, grade })
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

/// `tests/sub/f.rs` loaded by two `mod`s: by `#[path]` (module folder `tests/sub`, `first` cfg) and as `mod f;`
/// in an inline `#[path = "sub"] mod m` (module folder `tests/sub/f`, `second` cfg). Its `mod g;` loads
/// `tests/sub/g.rs` in the first folder and `tests/sub/f/g.rs` in the second.
fn two_module_folders(first: &str, second: &str) -> tempfile::TempDir {
    let root = format!("#[cfg({first})]\n#[path = \"sub/f.rs\"]\nmod fa;\n#[cfg({second})]\n#[path = \"sub\"]\nmod m {{\n    mod f;\n}}\n");
    repo_with(&[
        ("crates/demo/tests/two.rs", &root),
        ("crates/demo/tests/sub/f.rs", "mod g;\n"),
        ("crates/demo/tests/sub/g.rs", "#[test]\nfn in_a() {}\n"),
        ("crates/demo/tests/sub/f/g.rs", "#[test]\nfn in_b() {}\n"),
    ])
}

#[test]
fn check_3_file_in_two_module_folders_windows_first() {
    let d = two_module_folders("windows", "target_os = \"linux\"");
    let ev = |e: &str| evidence(d.path(), e, "Alpha").map(|e| e.platforms);
    assert_eq!(ev("test:crates/demo/tests/sub/g.rs::in_a").unwrap(), ["windows"]);
    assert_eq!(ev("test:crates/demo/tests/sub/f/g.rs::in_b").unwrap(), ["linux"]);
}

#[test]
fn check_3_file_in_two_module_folders_linux_first() {
    let d = two_module_folders("target_os = \"linux\"", "windows");
    let ev = |e: &str| evidence(d.path(), e, "Alpha").map(|e| e.platforms);
    assert_eq!(ev("test:crates/demo/tests/sub/g.rs::in_a").unwrap(), ["linux"]);
    assert_eq!(ev("test:crates/demo/tests/sub/f/g.rs::in_b").unwrap(), ["windows"]);
}

#[test]
fn check_3_mod_cycle_through_parent_folder_ends() {
    // f.rs loads itself through `..`: the module folder `tests/cyc/../cyc` is `tests/cyc`, a known visit.
    let d = repo_with(&[
        ("crates/demo/tests/cyc.rs", "#[cfg(unix)]\n#[path = \"cyc/f.rs\"]\nmod f;\n"),
        ("crates/demo/tests/cyc/f.rs", "#[path = \"../cyc/f.rs\"]\nmod again;\n#[test]\nfn t() {}\n"),
    ]);
    assert_eq!(evidence(d.path(), "test:crates/demo/tests/cyc/f.rs::t", "Alpha").unwrap().platforms, ["linux", "macos"]);
}

#[test]
fn check_3_test_root_also_mod_target_reaches_its_modules() {
    // tests/m2.rs is a test target (module folder tests/) and `mod m2;` of tests/root2.rs (module folder tests/m2).
    let d = repo_with(&[
        ("crates/demo/tests/root2.rs", "mod m2;\n"),
        ("crates/demo/tests/m2.rs", "#![cfg(windows)]\n#[test]\nfn t() {}\nmod m3;\n"),
        ("crates/demo/tests/m2/m3.rs", "#[test]\nfn t3() {}\n"),
    ]);
    let ev = |e: &str| evidence(d.path(), e, "Alpha").map(|e| e.platforms);
    assert_eq!(ev("test:crates/demo/tests/m2.rs::t").unwrap(), ["windows"]);
    assert_eq!(ev("test:crates/demo/tests/m2/m3.rs::t3").unwrap(), ["windows"]);
}

#[test]
fn check_3_test_file_without_crate_manifest_not_evidence() {
    let d = repo_with(&[]);
    assert!(evidence(d.path(), "test:crates/demo/tests/ev.rs::plain", "Alpha").is_ok());
    std::fs::remove_file(d.path().join("crates/demo/Cargo.toml")).unwrap();
    let e = evidence(d.path(), "test:crates/demo/tests/ev.rs::plain", "Alpha").unwrap_err();
    assert!(e.contains("crates/demo/Cargo.toml") && e.contains("no test target compiles"), "{e}");
}

#[test]
fn check_3_cargo_target_settings_refused() {
    let pkg = "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n";
    for (extra, key) in [
        ("[[test]]\nname = \"h\"\npath = \"tests/h.rs\"\nharness = false\n", "[[test]]"),
        ("[[test]]\nname = \"ev\"\n", "[[test]]"),
        ("autotests = false\n", "autotests"),
        ("[lib]\nharness = false\n", "harness"),
        ("[lib]\ntest = false\n", "test = false"),
        ("[[bin]]\nname = \"b\"\nharness = false\n", "harness"),
        ("[[bin]]\nname = \"b\"\nrequired-features = [\"x\"]\n", "required-features"),
    ] {
        let d = repo_with(&[("crates/demo/Cargo.toml", &format!("{pkg}{extra}")), ("crates/demo/tests/h.rs", "#[test]\nfn t() {}\n")]);
        for ev in ["test:crates/demo/tests/h.rs::t", "test:crates/demo/tests/ev.rs::plain"] {
            let e = evidence(d.path(), ev, "Alpha").unwrap_err();
            assert!(
                e.contains("crates/demo/Cargo.toml") && e.contains("unsupported Cargo target setting") && e.contains(key),
                "{extra}: {e}"
            );
        }
    }
    // Each manifest key that changes which files Cargo compiles as test code is refused.
    for (extra, key) in [
        ("[[bin]]\nname = \"b\"\ntest = false\n", "test = false"),
        ("[lib]\npath = \"src/other.rs\"\n", "path"),
        ("[[bin]]\nname = \"b\"\npath = \"src/b.rs\"\n", "path"),
        ("autobins = false\n", "autobins"),
    ] {
        let d = repo_with(&[("crates/demo/Cargo.toml", &format!("{pkg}{extra}"))]);
        let e = evidence(d.path(), "test:crates/demo/tests/ev.rs::plain", "Alpha").unwrap_err();
        assert!(
            e.contains("crates/demo/Cargo.toml") && e.contains("unsupported Cargo target setting") && e.contains(key),
            "{extra}: {e}"
        );
    }
    // A plain `[lib]` or `[[bin]]` table is no target setting that changes the test targets.
    let d = repo_with(&[("crates/demo/Cargo.toml", &format!("{pkg}[lib]\ndoctest = false\n[[bin]]\nname = \"b\"\n"))]);
    assert!(evidence(d.path(), "test:crates/demo/tests/ev.rs::plain", "Alpha").is_ok());
    let d = repo_with(&[("crates/demo/Cargo.toml", "[package\n")]);
    assert!(evidence(d.path(), "test:crates/demo/tests/ev.rs::plain", "Alpha").unwrap_err().contains("crates/demo/Cargo.toml"));
}

#[test]
fn derive_cell_fixtures_full_none_na() {
    let (r, g, _) = base();
    assert_eq!(derive::cell(&r, &g[0], "A-1"), Cell { grade: Grade::Full, value: Some(1.0), missing: vec![], na: vec![] });
    assert_eq!(
        derive::cell(&r, &g[0], "WA-2"),
        Cell {
            grade: Grade::Na,
            value: None,
            missing: vec![],
            na: vec![Unmet { criterion: "WA-2.1".into(), text: "Turn start.".into(), reason: Some("Alpha has no turn events".into()) }]
        }
    );
    assert_eq!(
        derive::cell(&r, &g[1], "A-3"),
        Cell {
            grade: Grade::None,
            value: Some(0.0),
            missing: vec![Unmet {
                criterion: "A-3.1".into(),
                text: "The picker shows the role list.".into(),
                reason: Some("Beta has no model picker".into())
            }],
            na: vec![]
        }
    );
}

#[test]
fn derive_cell_partial_lists_missing() {
    let (r, mut g, _) = base();
    {
        let e = entry(&mut g[0], "A-1.2");
        e.status = Status::NotMet;
        e.reason = Some("no key helper".into());
    }
    assert_eq!(
        derive::cell(&r, &g[0], "A-1"),
        Cell {
            grade: Grade::Partial,
            value: Some(0.5),
            missing: vec![Unmet {
                criterion: "A-1.2".into(),
                text: "The credential is the session token.".into(),
                reason: Some("no key helper".into())
            }],
            na: vec![]
        }
    );
}

#[test]
fn derive_cell_na_criteria_leave_value_and_ungraded_is_unmet() {
    let (r, g) = synth(&[(Tier::Extra, &[Status::Met, Status::Na, Status::NotMet][..]), (Tier::Extra, &[Status::Na, Status::Na][..])]);
    let c = derive::cell(&r, &g, "A-1");
    assert_eq!((c.grade, c.value, c.missing.len(), c.na.len()), (Grade::Partial, Some(0.5), 1, 1));
    let c = derive::cell(&r, &g, "A-2"); // an all-n/a cell is n/a
    assert_eq!((c.grade, c.value, c.na.len()), (Grade::Na, None, 2));
    let (r, mut g) = synth(&[(Tier::Standard, &[Status::Met, Status::Met][..])]);
    g.grade.retain(|e| e.criterion != "A-1.2");
    assert_eq!(derive::cell(&r, &g, "A-1").missing, vec![Unmet { criterion: "A-1.2".into(), text: "t1.2".into(), reason: None }]);
}

fn pct(t: Option<u32>, c: Option<u32>, s: Option<u32>, e: Option<u32>) -> Scores {
    Scores { total: t, core: c, standard: s, extra: e }
}

#[test]
fn derive_scores_fixtures() {
    let (r, g, _) = base();
    assert_eq!(derive::scores(&r, &g[0]), pct(Some(100), Some(100), Some(100), None));
    assert_eq!(derive::scores(&r, &g[1]), pct(Some(66), Some(100), Some(0), Some(100))); // 2/3 rounds down
}

#[test]
fn derive_scores_na_tier_leaves_weights() {
    let (r, mut g, _) = base();
    {
        let e = entry(&mut g[0], "A-1.2");
        e.status = Status::NotMet;
        e.reason = Some("no key helper".into());
    }
    // (3 × 0.5 + 2 × 1) ÷ 5 = 0.7; an n/a extra tier counted as 0 with weight 1 would give 58
    assert_eq!(derive::scores(&r, &g[0]), pct(Some(70), Some(50), Some(100), None));
    let (r, g) = synth(&[(Tier::Extra, &[Status::Na][..])]);
    assert_eq!(derive::scores(&r, &g), pct(None, None, None, None));
}

#[test]
fn derive_scores_mean_of_cells_exact_floor() {
    use Status::{Met as M, NotMet as N};
    // cell values 1/1, 1/4, 2/5: exact mean 0.55 → 55 (f64 arithmetic gives 54)
    let (r, g) = synth(&[(Tier::Standard, &[M][..]), (Tier::Standard, &[M, N, N, N][..]), (Tier::Standard, &[M, M, N, N, N][..])]);
    assert_eq!(derive::scores(&r, &g), pct(Some(55), None, Some(55), None));
}

/// K§6.2.6: core 7 full + 1 at 3/4 (A-1.4 not-met, approved exception); standard 12 cells summing to 9;
/// extra 5/6, 1, 1 and one all-n/a cell.
fn spec_example() -> (Registry, GradeFile) {
    use Status::{Met as M, Na as X, NotMet as N};
    let mut caps: Vec<(Tier, &[Status])> = vec![(Tier::Core, &[M, M, M, N][..])];
    caps.extend([(Tier::Core, &[M, M][..]); 5]);
    caps.extend([(Tier::Core, &[M, M, M][..]); 2]);
    caps.extend([(Tier::Standard, &[M][..]); 9]);
    caps.extend([(Tier::Standard, &[N][..]); 3]);
    caps.extend([(Tier::Extra, &[M, M, M, M, M, N][..]), (Tier::Extra, &[M][..]), (Tier::Extra, &[M][..]), (Tier::Extra, &[X, X][..])]);
    let (mut r, g) = synth(&caps);
    r.bar_exception.push(BarException { client: "synth".into(), criterion: "A-1.4".into(), reason: "r".into(), approved: Some("operator".into()) });
    (r, g)
}

#[test]
fn derive_scores_spec_example_rounds_last() {
    let (r, g) = spec_example();
    assert_eq!(derive::cell(&r, &g, "A-1").value, Some(0.75));
    let s = derive::scores(&r, &g);
    assert_eq!(s, pct(Some(89), Some(96), Some(75), Some(94)));
    // review focus 3: the tier percents rounded before the total give 88, not 89
    let (c, st, e) = (s.core.unwrap(), s.standard.unwrap(), s.extra.unwrap());
    assert_eq!((3 * c + 2 * st + e) / 6, 88);
}

fn exception(client: &str, criterion: &str) -> BarException {
    BarException { client: client.into(), criterion: criterion.into(), reason: "r".into(), approved: Some("operator".into()) }
}

#[test]
fn derive_bar_below_then_exception_meets() {
    let (mut r, mut g, _) = base();
    assert_eq!(derive::bar(&r, &g[0]), Bar { meets: true, failing: vec![], exceptions: vec![] });
    {
        let e = entry(&mut g[0], "A-1.2");
        e.status = Status::NotMet;
        e.reason = Some("no key helper".into());
    }
    assert_eq!(derive::bar(&r, &g[0]), Bar { meets: false, failing: vec!["A-1".into()], exceptions: vec![] });
    r.bar_exception.push(exception("alpha", "A-1.2"));
    assert_eq!(derive::bar(&r, &g[0]), Bar { meets: true, failing: vec![], exceptions: vec!["A-1.2".into()] });
    assert!(derive::bar(&r, &g[1]).exceptions.is_empty()); // another client's exception
}

#[test]
fn derive_bar_unapproved_exception_does_not_count() {
    let (mut r, mut g, _) = base();
    {
        let e = entry(&mut g[0], "A-1.2");
        e.status = Status::NotMet;
        e.reason = Some("no key helper".into());
    }
    for approved in [None, Some(" ".to_string())] {
        r.bar_exception = vec![BarException { approved, ..exception("alpha", "A-1.2") }];
        assert_eq!(derive::bar(&r, &g[0]), Bar { meets: false, failing: vec!["A-1".into()], exceptions: vec![] });
    }
}

#[test]
fn derive_bar_core_na_or_ungraded_fails_even_with_exception() {
    let (mut r, g0, _) = base();
    r.bar_exception.push(exception("alpha", "A-1.2"));
    let mut g = g0.clone();
    {
        let e = entry(&mut g[0], "A-1.2");
        e.status = Status::Na;
        e.reason = Some("x".into());
    }
    assert_eq!(derive::bar(&r, &g[0]).failing, ["A-1"]); // an exception counts only for not-met (K§6.2.4)
    let mut g = g0.clone();
    g[0].grade.retain(|e| e.criterion != "A-1.1");
    assert_eq!(derive::bar(&r, &g[0]).failing, ["A-1"]);
    let mut g = g0.clone();
    entry(&mut g[0], "WA-2.1").status = Status::NotMet; // a non-core gap never fails the bar
    assert!(derive::bar(&r, &g[0]).meets);
}

fn n(full: u32, partial: u32, none: u32, na: u32, met: u32, applicable: u32) -> Counts {
    Counts { full, partial, none, na, met, applicable }
}

#[test]
fn derive_rollup_fixtures() {
    let (r, g, _) = base();
    let a = derive::rollup(&r, &g[0], true);
    assert_eq!((a.core, a.standard, a.extra, a.total), (n(1, 0, 0, 0, 2, 2), n(1, 0, 0, 0, 1, 1), n(0, 0, 0, 1, 0, 0), n(2, 0, 0, 1, 3, 3)));
    let b = derive::rollup(&r, &g[1], true);
    assert_eq!((b.core, b.standard, b.extra, b.total), (n(1, 0, 0, 0, 2, 2), n(0, 0, 1, 0, 0, 1), n(1, 0, 0, 0, 1, 1), n(2, 0, 1, 0, 3, 4)));
    assert_eq!(b.bar, derive::bar(&r, &g[1]));
}

#[test]
fn derive_rollup_spec_example() {
    let (r, g) = spec_example();
    let x = derive::rollup(&r, &g, true);
    assert_eq!((x.core, x.standard, x.extra, x.total), (n(7, 1, 0, 0, 19, 20), n(9, 0, 3, 0, 9, 12), n(2, 1, 0, 1, 7, 8), n(18, 2, 3, 1, 35, 40)));
    assert_eq!(x.bar, Bar { meets: true, failing: vec![], exceptions: vec!["A-1.4".into()] });
    assert_eq!(x.release, "supported");
}

#[test]
fn derive_release_state() {
    let (r, mut g, _) = base();
    assert_eq!(derive::rollup(&r, &g[0], true).release, "supported");
    assert_eq!(derive::rollup(&r, &g[0], false).release, "meets bar, not released");
    {
        let e = entry(&mut g[0], "A-1.2");
        e.status = Status::NotMet;
        e.reason = Some("no key helper".into());
    }
    assert_eq!(derive::rollup(&r, &g[0], false).release, "below bar");
    assert_eq!(derive::rollup(&r, &g[0], true).release, "below bar");
}

#[test]
#[should_panic(expected = "score fraction overflow")]
fn derive_scores_overflow_fails_closed() {
    // 27 standard capabilities with the prime criteria counts 2..103, each 1 met and the rest not-met:
    // the exact mean has a denominator over u128 (CF-2 review F1)
    let primes: Vec<usize> = (2..=103).filter(|n: &usize| (2..*n).all(|d| n % d != 0)).collect();
    assert_eq!(primes.len(), 27);
    let statuses: Vec<Vec<Status>> =
        primes.iter().map(|&p| std::iter::once(Status::Met).chain(std::iter::repeat_n(Status::NotMet, p - 1)).collect()).collect();
    let caps: Vec<(Tier, &[Status])> = statuses.iter().map(|s| (Tier::Standard, &s[..])).collect();
    let (r, g) = synth(&caps);
    derive::scores(&r, &g);
}

#[test]
fn check_8_duplicate_grade_any_met_no_item_8() {
    let (r, mut g, c) = base();
    let mut e = entry(&mut g[0], "A-1.2").clone();
    e.status = Status::NotMet;
    e.evidence = vec![];
    e.reason = Some("no key helper".into());
    let at = g[0].grade.iter().position(|x| x.criterion == "A-1.2").unwrap();
    g[0].grade.insert(at, e); // not-met first, met second
    let f = check(&repo(), &r, &g, &c, &resolve(&repo(), &g));
    assert_eq!(items(&f), set(&[2]), "{f:?}");
}

#[test]
fn check_8_core_na_with_exception_is_below_bar() {
    let (mut r, mut g, c) = base();
    {
        let e = entry(&mut g[0], "A-1.2");
        e.status = Status::Na;
        e.evidence = vec![];
        e.reason = Some("x".into());
    }
    r.bar_exception.push(exception("alpha", "A-1.2"));
    let f = check(&repo(), &r, &g, &c, &resolve(&repo(), &g));
    assert_eq!(items(&f), set(&[4, 8]), "{f:?}");
    assert!(f.iter().any(|x| x.item == 4 && x.text.contains("core criterion `A-1.2` is n/a")), "{f:?}");
    assert!(f.iter().any(|x| x.item == 8 && x.text.ends_with("below the bar: A-1")), "{f:?}");
}

#[test]
fn check_8_core_ungraded_with_exception_is_below_bar() {
    let (mut r, mut g, c) = base();
    g[0].grade.retain(|e| e.criterion != "A-1.2");
    r.bar_exception.push(exception("alpha", "A-1.2"));
    let f = check(&repo(), &r, &g, &c, &resolve(&repo(), &g));
    assert_eq!(items(&f), set(&[2, 8]), "{f:?}");
    assert!(f.iter().any(|x| x.item == 2 && x.text.contains("A-1.2")), "{f:?}");
    assert!(f.iter().any(|x| x.item == 8 && x.text.ends_with("below the bar: A-1")), "{f:?}");
}

/// Alpha with A-1.2 graded twice, not-met and n/a in the order given, and an approved exception for alpha A-1.2.
fn duplicate_not_met_na(not_met_first: bool) -> (Registry, Vec<GradeFile>, Vec<Client>) {
    let (mut r, mut g, c) = base();
    r.bar_exception.push(exception("alpha", "A-1.2"));
    let at = g[0].grade.iter().position(|x| x.criterion == "A-1.2").unwrap();
    let mut not_met = g[0].grade.remove(at);
    not_met.status = Status::NotMet;
    not_met.evidence = vec![];
    not_met.reason = Some("no key helper".into());
    let na = GradeEntry { status: Status::Na, reason: Some("not here".into()), ..not_met.clone() };
    let pair = if not_met_first { [not_met, na] } else { [na, not_met] };
    for (i, e) in pair.into_iter().enumerate() {
        g[0].grade.insert(at + i, e);
    }
    (r, g, c)
}

#[test]
fn derive_duplicate_grades_order_independent() {
    let (r, a, _) = duplicate_not_met_na(true);
    let (_, b, _) = duplicate_not_met_na(false);
    assert_eq!(derive::cell(&r, &a[0], "A-1"), derive::cell(&r, &b[0], "A-1"));
    assert_eq!(derive::bar(&r, &a[0]), derive::bar(&r, &b[0]));
    // not-met wins over n/a, and the first not-met entry keeps its reason
    assert_eq!(
        derive::cell(&r, &b[0], "A-1").missing,
        vec![Unmet { criterion: "A-1.2".into(), text: "The credential is the session token.".into(), reason: Some("no key helper".into()) }]
    );
    assert!(derive::bar(&r, &b[0]).meets);
}

#[test]
fn check_8_duplicate_grades_order_independent() {
    let (r, a, c) = duplicate_not_met_na(true);
    let (_, b, _) = duplicate_not_met_na(false);
    let fa = items(&check(&repo(), &r, &a, &c, &resolve(&repo(), &a)));
    let fb = items(&check(&repo(), &r, &b, &c, &resolve(&repo(), &b)));
    assert_eq!(fa, fb);
    assert!(!fb.contains(&8), "{fb:?}");
}

type RenderIn = (Registry, Vec<GradeFile>, Vec<Client>, Resolved);

/// `base()` with alpha A-1.2 not-met under an approved registry exception: the four cell forms.
fn render_input() -> RenderIn {
    let (mut r, mut g, c) = base();
    {
        let e = entry(&mut g[0], "A-1.2");
        e.status = Status::NotMet;
        e.reason = Some("no key helper".into());
    }
    r.bar_exception.push(exception("alpha", "A-1.2"));
    let res = resolve(&repo(), &g);
    (r, g, c, res)
}

fn synth_input(caps: &[(Tier, &[Status])]) -> RenderIn {
    let (r, g) = synth(caps);
    let g = vec![g];
    let res = resolve(&repo(), &g);
    (r, g, vec![client("synth", "Synth", "1.0.0")], res)
}

fn md_of(x: &RenderIn) -> String {
    render::markdown(&x.0, &x.1, &x.2, &x.3)
}

fn json_of(x: &RenderIn) -> String {
    render::json(&x.0, &x.1, &x.2, &x.3)
}

fn row<'a>(md: &'a str, start: &str) -> &'a str {
    md.lines().find(|l| l.starts_with(start)).unwrap_or_else(|| panic!("no line `{start}`"))
}

#[test]
fn render_markdown_golden_fixtures() {
    let x = render_input();
    assert_eq!(check(&repo(), &x.0, &x.1, &x.2, &x.3), vec![]); // valid input
    assert_eq!(md_of(&x), text("expected-conformance.md").replace("\r\n", "\n"));
}

#[test]
fn render_markdown_four_cell_forms() {
    let md = md_of(&render_input());
    assert_eq!(row(&md, "| A-1 "), "| A-1 Transport | core | partial (missing: A-1.2 The credential is the session token.) https://example.com/alpha/transport | full https://example.com/beta/transport |");
    assert_eq!(row(&md, "| A-3 "), "| A-3 Model list | standard | full https://example.com/alpha/models | none (A-3.1: Beta has no model picker) https://example.com/beta/models |");
    assert_eq!(row(&md, "| WA-2 "), "| WA-2 Turn signals | extra | n/a (Alpha has no turn events) | full https://example.com/beta/turns |");
    use Status::{Met as M, Na as X, NotMet as N};
    let md = md_of(&synth_input(&[(Tier::Standard, &[M, N, N][..]), (Tier::Standard, &[N, N][..]), (Tier::Extra, &[X, X][..])]));
    assert_eq!(row(&md, "| A-1 "), "| A-1 c | standard | partial (missing: A-1.2 t1.2; A-1.3 t1.3) https://example.com/1 |");
    assert_eq!(row(&md, "| A-2 "), "| A-2 c | standard | none (A-2.1: r2.1; A-2.2: r2.2) https://example.com/2 |");
    assert_eq!(row(&md, "| A-3 "), "| A-3 c | extra | n/a (r3.1; r3.2) |");
    let mut x = synth_input(&[(Tier::Standard, &[N][..])]);
    x.1[0].grade.clear(); // ungraded
    let md = md_of(&x);
    assert_eq!(row(&md, "| A-1 "), "| A-1 c | standard | none (A-1.1: no grade) https://example.com/1 |");
    assert!(md.contains("- A-1.1 not met: no grade\n"));
}

#[test]
fn render_markdown_rows_by_tier_columns_by_client() {
    use Status::Met as M;
    let md = md_of(&synth_input(&[(Tier::Extra, &[M][..]), (Tier::Core, &[M][..]), (Tier::Standard, &[M][..]), (Tier::Core, &[M][..])]));
    let pos = |id: &str| md.find(&format!("| {id} c |")).unwrap();
    assert!(pos("A-2") < pos("A-4") && pos("A-4") < pos("A-3") && pos("A-3") < pos("A-1"));
    assert!(md.contains("- A-1.1 met: no evidence that counts\n"));
    let mut x = render_input();
    x.2.reverse();
    let md = md_of(&x);
    assert!(md.lines().any(|l| l == "| Capability | Tier | Beta | Alpha |"));
    assert!(md.find("### Beta 2.0.0").unwrap() < md.find("### Alpha 1.0.0").unwrap());
    assert!(md.find("| Beta | 66%").unwrap() < md.find("| Alpha | 70%").unwrap());
}

#[test]
fn render_markdown_platform_text_and_unix_shown() {
    let p = |v: &[&str]| render::platform_text(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    assert_eq!(p(&["linux", "macos"]), Some("unix".to_string()));
    assert_eq!(p(&["linux", "macos", "windows"]), None);
    assert_eq!(p(&["windows"]), Some("windows".to_string()));
    assert_eq!(p(&["linux"]), Some("linux".to_string()));
    assert_eq!(p(&["macos", "windows"]), Some("macos, windows".to_string()));
    let md = md_of(&render_input());
    assert!(md.contains("- WA-2.1 met: `test:crates/demo/tests/ev.rs::unix_only` (unix)\n"));
    assert!(md.contains("- A-1.1 met: `test:crates/demo/tests/ev.rs::plain`\n"));
    let (r, mut g, c) = base();
    entry(&mut g[1], "WA-2.1").evidence = vec!["test:crates/demo/tests/ev.rs::in_windows_mod".into(), "fixture:fixtures/beta/capture.json".into()];
    let res = resolve(&repo(), &g);
    assert!(render::markdown(&r, &g, &c, &res).contains(
        "- WA-2.1 met: `test:crates/demo/tests/ev.rs::in_windows_mod` (windows), `fixture:fixtures/beta/capture.json`\n"
    ));
}

/// Review focus 4, first half: a `|` in a reason, URL, name or client name keeps the row one row.
#[test]
fn render_markdown_pipe_escaped_one_row() {
    let mut x = render_input();
    entry(&mut x.1[1], "A-3.1").reason = Some("a|b".into());
    x.1[1].cell.iter_mut().find(|c| c.capability == "A-3").unwrap().url = "https://example.com/x|y".into();
    x.0.capability[1].name = "Model|list".into();
    x.2[1].name = "Be|ta".into();
    let md = md_of(&x);
    let line = row(&md, "| A-3 ");
    assert_eq!(line, r"| A-3 Model\|list | standard | full https://example.com/alpha/models | none (A-3.1: a\|b) https://example.com/x\|y |");
    assert_eq!(unescaped(line), 5);
    assert!(md.lines().any(|l| l == r"| Capability | Tier | Alpha | Be\|ta |"));
    assert!(md.lines().any(|l| l == r"| Be\|ta | 66% | 100% | 0% | 100% |"));
    assert!(md.contains("- A-3.1 not met: a|b\n")); // outside a table: as written
}

/// The `|` of `l` that are cell borders: a `|` is escaped only when an odd number of `\` come before it.
fn unescaped(l: &str) -> usize {
    l.match_indices('|').filter(|(i, _)| l[..*i].chars().rev().take_while(|&c| c == '\\').count() % 2 == 0).count()
}

#[test]
fn render_markdown_backslash_kept() {
    let mut x = render_input();
    entry(&mut x.1[1], "A-3.1").reason = Some(r"a\|b".into());
    let md = md_of(&x);
    let line = row(&md, "| A-3 ");
    assert_eq!(line, r"| A-3 Model list | standard | full https://example.com/alpha/models | none (A-3.1: a\\\|b) https://example.com/beta/models |");
    assert_eq!(unescaped(line), 5);
}

/// Raw HTML from input text is shown as text, in table cells and list lines; a URL is not changed.
#[test]
fn render_markdown_html_escaped() {
    let mut x = render_input();
    entry(&mut x.1[1], "A-3.1").reason = Some("<details>".into());
    entry(&mut x.1[0], "WA-2.1").reason = Some("<!--".into());
    x.1[1].cell.iter_mut().find(|c| c.capability == "A-3").unwrap().url = "https://example.com/m?a=1&b=2".into();
    let md = md_of(&x);
    assert_eq!(row(&md, "| A-3 "), "| A-3 Model list | standard | full https://example.com/alpha/models | none (A-3.1: &lt;details&gt;) https://example.com/m?a=1&b=2 |");
    assert_eq!(row(&md, "| WA-2 "), "| WA-2 Turn signals | extra | n/a (&lt;!--) | full https://example.com/beta/turns |");
    assert!(md.contains("- A-3.1 not met: &lt;details&gt;\n"));
    assert!(md.contains("- WA-2.1 n/a: &lt;!--\n"));
    assert!(!md.contains("<details>") && !md.contains("<!--"));
}

/// An evidence text with backticks stays one code span (CommonMark): the fence is one backtick longer than the
/// longest run inside, with a space inside the fence when the text begins or ends with a backtick.
#[test]
fn render_markdown_evidence_backtick_span() {
    let (r, g, c) = base();
    let ok = || Ok(Evidence { platforms: vec!["linux".into(), "macos".into(), "windows".into()] });
    let evidence = vec![
        ResolvedEvidence { text: "fixture:a`b".into(), result: ok() },
        ResolvedEvidence { text: "`x``".into(), result: ok() },
    ];
    let res = Resolved { grade: vec![ResolvedGrade { file: 0, client: "alpha".into(), criterion: "A-1.1".into(), evidence }] };
    let md = render::markdown(&r, &g, &c, &res);
    assert!(md.contains("- A-1.1 met: ``fixture:a`b``, ``` `x`` ```\n"), "{md}");
}

#[test]
fn render_markdown_below_bar_rollup() {
    let (r, mut g, c) = base();
    {
        let e = entry(&mut g[0], "A-1.2");
        e.status = Status::NotMet;
        e.reason = Some("no key helper".into());
    }
    let res = resolve(&repo(), &g);
    let md = render::markdown(&r, &g, &c, &res);
    assert!(md.contains("### Alpha 1.0.0\n"));
    assert!(md.contains("- Bar: below bar: A-1\n- Bar exceptions: none\n- Release: below bar\n"));
}

fn jv(s: &str) -> serde_json::Value {
    serde_json::from_str(s).unwrap()
}

/// The keys of pretty JSON in text order (one key per line).
fn keys(json: &str) -> Vec<String> {
    json.lines().filter_map(|l| l.trim_start().strip_prefix('"')?.split_once("\": ").map(|(k, _)| k.to_string())).collect()
}

#[test]
fn render_json_generated_by_matches_markdown_line() {
    let x = render_input();
    assert_eq!(render::GENERATED_BY, "crates/zygo-core/tests/conformance.rs (ZYGO_WRITE_CONFORMANCE=1); do not edit: a hand edit fails the check");
    assert_eq!(md_of(&x).lines().next(), Some(format!("Generated by {}.", render::GENERATED_BY).as_str()));
    assert_eq!(jv(&json_of(&x))["generated_by"].as_str(), Some(render::GENERATED_BY));
}

#[test]
fn render_json_matrix_matches_markdown() {
    let x = render_input();
    let (md, v) = (md_of(&x), jv(&json_of(&x)));
    for (i, c) in x.2.iter().enumerate() {
        let j = &v["clients"][i];
        assert_eq!(j["id"].as_str(), Some(c.id.as_str()));
        for cap in &x.0.capability {
            let cell = derive::cell(&x.0, &x.1[i], &cap.id);
            let word = match cell.grade {
                Grade::Full => "full",
                Grade::Partial => "partial",
                Grade::None => "none",
                Grade::Na => "n/a",
            };
            let m = &j["matrix"][cap.id.as_str()];
            assert_eq!(m["grade"].as_str(), Some(word));
            assert_eq!(m["value"].as_f64(), cell.value);
            assert_eq!(m["missing"].as_array().unwrap().len(), cell.missing.len());
            assert_eq!(m["na"].as_array().unwrap().len(), cell.na.len());
            let md_cell = row(&md, &format!("| {} ", cap.id)).split(" | ").nth(2 + i).unwrap();
            assert!(md_cell.starts_with(&format!("{word} ")), "{md_cell}");
        }
    }
    let a = &v["clients"][0];
    assert_eq!(a["matrix"]["A-1"]["missing"][0]["reason"].as_str(), Some("no key helper"));
    assert_eq!(a["matrix"]["WA-2"]["na"][0]["criterion"].as_str(), Some("WA-2.1"));
    assert!(a["matrix"]["WA-2"]["value"].is_null() && a["score"]["extra"].is_null());
    assert_eq!(["total", "core", "standard"].map(|k| a["score"][k].as_u64()), [Some(70), Some(50), Some(100)]);
    assert_eq!(a["rollup"]["bar"]["exceptions"][0].as_str(), Some("A-1.2"));
}

/// K§6.2.6 machine-readable example, value by value.
#[test]
fn render_json_spec_example_values() {
    let (r, g) = spec_example();
    let x: RenderIn = {
        let g = vec![g];
        let res = resolve(&repo(), &g);
        (r, g, vec![client("synth", "Synth", "1.0.0")], res)
    };
    let v = jv(&json_of(&x));
    assert!(v["generated_by"].as_str().unwrap().starts_with("crates/zygo-core/tests/conformance.rs (ZYGO_WRITE_CONFORMANCE=1); do not edit"));
    assert_eq!(["core", "standard", "extra"].map(|k| v["weights"][k].as_u64()), [Some(3), Some(2), Some(1)]);
    let c = &v["clients"][0];
    assert_eq!((c["id"].as_str(), c["name"].as_str(), c["version"].as_str(), c["supported"].as_bool()), (Some("synth"), Some("Synth"), Some("1.0.0"), Some(true)));
    let a1 = &c["matrix"]["A-1"];
    assert_eq!((a1["tier"].as_str(), a1["grade"].as_str(), a1["value"].as_f64(), a1["url"].as_str()), (Some("core"), Some("partial"), Some(0.75), Some("https://example.com/1")));
    let m = &a1["missing"];
    assert_eq!((m.as_array().unwrap().len(), a1["na"].as_array().unwrap().len()), (1, 0));
    assert_eq!((m[0]["criterion"].as_str(), m[0]["text"].as_str(), m[0]["reason"].as_str()), (Some("A-1.4"), Some("t1.4"), Some("r1.4")));
    let counts = |t: &str| ["full", "partial", "none", "na", "met", "applicable"].map(|k| c["rollup"][t][k].as_u64().unwrap());
    assert_eq!(counts("core"), [7, 1, 0, 0, 19, 20]);
    assert_eq!(counts("standard"), [9, 0, 3, 0, 9, 12]);
    assert_eq!(counts("extra"), [2, 1, 0, 1, 7, 8]);
    assert_eq!(counts("total"), [18, 2, 3, 1, 35, 40]);
    let bar = &c["rollup"]["bar"];
    assert_eq!(bar["verdict"].as_str(), Some("meets bar"));
    assert_eq!(bar["failing"].as_array().unwrap().len(), 0);
    assert_eq!(bar["exceptions"].as_array().unwrap().iter().map(|e| e.as_str().unwrap()).collect::<Vec<_>>(), ["A-1.4"]);
    assert_eq!(c["rollup"]["release"].as_str(), Some("supported"));
    assert_eq!(["total", "core", "standard", "extra"].map(|k| c["score"][k].as_u64()), [Some(89), Some(96), Some(75), Some(94)]);
    assert!(md_of(&x).lines().any(|l| l == "| Synth | 89% | 96% | 75% | 94% |"));
}

#[test]
fn render_json_key_order() {
    let json = json_of(&synth_input(&[(Tier::Core, &[Status::Met][..])]));
    let counts = ["full", "partial", "none", "na", "met", "applicable"];
    let mut want = vec!["generated_by", "weights", "core", "standard", "extra", "clients", "id", "name", "version", "supported", "matrix", "A-1", "tier", "grade", "value", "url", "missing", "na", "rollup"];
    for t in ["core", "standard", "extra", "total"] {
        want.push(t);
        want.extend(counts);
    }
    want.extend(["bar", "verdict", "failing", "exceptions", "release", "score", "total", "core", "standard", "extra"]);
    assert_eq!(keys(&json), want);
    assert!(json.ends_with("}\n") && !json.ends_with("\n\n"));
}

#[test]
fn render_json_matrix_in_tier_order() {
    use Status::Met as M;
    let json = json_of(&synth_input(&[(Tier::Extra, &[M][..]), (Tier::Core, &[M][..]), (Tier::Standard, &[M][..]), (Tier::Core, &[M][..])]));
    let order: Vec<String> = keys(&json).into_iter().filter(|k| k.starts_with("A-")).collect();
    assert_eq!(order, ["A-2", "A-4", "A-3", "A-1"]);
}

#[test]
fn render_json_two_renders_byte_equal() {
    let (x, y) = (render_input(), render_input());
    assert_eq!(md_of(&x), md_of(&y));
    assert_eq!(json_of(&x), json_of(&y));
    assert_eq!(json_of(&x), json_of(&x));
}

#[test]
fn render_json_client_without_grade_file_left_out() {
    let mut x = render_input();
    x.2.push(client("gamma", "Gamma", "3.0.0"));
    assert!(!md_of(&x).contains("Gamma"));
    assert_eq!(jv(&json_of(&x))["clients"].as_array().unwrap().len(), 2);
}

fn docs_root(md: &str, json: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("docs")).unwrap();
    std::fs::write(dir.path().join("docs/conformance.md"), md).unwrap();
    std::fs::write(dir.path().join("docs/conformance.json"), json).unwrap();
    dir
}

/// Review focus 4, second half: a Windows checkout with CR LF compares equal.
#[test]
fn render_compare_crlf_equal() {
    let x = render_input();
    let (md, json) = (md_of(&x), json_of(&x));
    let dir = docs_root(&md.replace('\n', "\r\n"), &json.replace('\n', "\r\n"));
    assert_eq!(render::compare_or_write_mode(dir.path(), &md, &json, false), Ok(()));
    let dir = docs_root(&md, &json);
    assert_eq!(render::compare_or_write_mode(dir.path(), &md, &json, false), Ok(()));
}

#[test]
fn render_compare_planted_difference_is_item_10() {
    let x = render_input();
    let (md, json) = (md_of(&x), json_of(&x));
    let planted = md.replacen("full https://example.com/beta/transport", "full https://example.com/beta/transporT", 1);
    for (m, j, path) in [(planted.as_str(), json.as_str(), "docs/conformance.md"), (md.as_str(), &json.replacen("\"meets bar\"", "\"below bar\"", 1)[..], "docs/conformance.json")] {
        let dir = docs_root(m, j);
        let f = render::compare_or_write_mode(dir.path(), &md, &json, false).unwrap_err();
        assert_eq!(f.item, 10);
        assert!(f.text.contains(path), "{}", f.text);
    }
    let dir = docs_root(&md, &json);
    std::fs::remove_file(dir.path().join("docs/conformance.json")).unwrap();
    let f = render::compare_or_write_mode(dir.path(), &md, &json, false).unwrap_err();
    assert!(f.item == 10 && f.text.contains("docs/conformance.json"));
}

#[test]
fn render_compare_write_mode_writes_both() {
    let x = render_input();
    let (md, json) = (md_of(&x), json_of(&x));
    let dir = tempfile::tempdir().unwrap(); // no docs/ yet
    assert_eq!(render::compare_or_write_mode(dir.path(), &md, &json, true), Ok(()));
    assert_eq!(std::fs::read(dir.path().join("docs/conformance.md")).unwrap(), md.as_bytes());
    assert_eq!(std::fs::read(dir.path().join("docs/conformance.json")).unwrap(), json.as_bytes());
    assert_eq!(render::compare_or_write_mode(dir.path(), &md, &json, false), Ok(()));
    let file = dir.path().join("docs/conformance.md"); // a root that is a file cannot be written
    assert_eq!(render::compare_or_write_mode(&file, &md, &json, true).unwrap_err().item, 10);
}
