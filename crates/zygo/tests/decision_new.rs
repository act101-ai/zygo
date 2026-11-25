//! DR-4: `zygo decision new` (D§10.2, D§10.1 row `new`, D§6.1–D§6.3, D§6.8, D§17 #1 and #2).
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use zygo::cmd::decision::{run_in, DecisionIo, Repo};
use zygo_core::session_dir::SessionDir;
use zygo_core::session_view::{LegendEntry, SessionView};
use zygo_usage::{RequestRow, Store};
use zygo_work::decisions::record::{name_kind, parse_record, Class, Kind, NameKind, Status, FILL_MARKER};
use zygo_work::decisions::set::read_dir;

mod common;

const C40: &str = "6e7fe2f0c1d2e3f4a5b6c7d8e9f0011223344556";
const BODY: &str = "\n## Choice\n\nC.\n\n## Alternatives rejected\n\nA.\n\n## Evidence\n\nE.\n\n## Cost to reverse\n\nR.\n";
const USAGE: &str = "zygo decision: usage: zygo decision new --kind <k> --title <t> --unit <u> --round <n> --role <r> [--class <c>] [--depends-on <id>]... [--supersedes <id>] | ls [--status <s>] [--kind <k>] [--unit <prefix>] [--pending] | show <id> | deps <id> | check | ratify <id> [--note <text>] [--promote adr|spec|rule --as <text>] [--yes] | overturn <id> --reason <text> [--yes] | review [--kind <k>] [--unit <prefix>]\n";
fn id(n: u32) -> String { format!("DDR-01JDQ2Y7Z8K4M5N6P7Q8R{n:05}") }
fn md(n: u32) -> String { format!("{}.md", id(n)) }
/// A valid open decision titled `T` with id `rid` and `depends_on` of `deps`.
fn text(rid: &str, deps: &[u32]) -> String {
    let deps: Vec<String> = deps.iter().map(|d| id(*d)).collect();
    format!("+++\nid = \"{rid}\"\nkind = \"decision\"\nstatus = \"open\"\nclass = \"none\"\ntitle = \"T\"\nunit = \"phase-3:N12\"\nround = 1\nrole = \"code\"\nmodel = \"unknown\"\ncommit = \"{C40}\"\ndate = \"2026-11-24\"\ndepends_on = {deps:?}\nreverse_cost = \"low\"\n+++\n{BODY}")
}
fn dir(top: &Path) -> PathBuf { let d = top.join("docs/decisions"); fs::create_dir_all(&d).unwrap(); d }
fn put(top: &Path, n: u32, deps: &[u32]) { fs::write(dir(top).join(md(n)), text(&id(n), deps)).unwrap(); }
/// Git in `dir` with no user or system config: `GIT_CONFIG_GLOBAL` an empty file next to `dir`, `HOME` that directory,
/// and the identity by env.
fn git(dir: &Path, args: &[&str]) -> String {
    let home = dir.parent().unwrap().join("git-home");
    fs::create_dir_all(&home).unwrap();
    let global = home.join("gitconfig");
    if !global.exists() { fs::write(&global, "").unwrap(); }
    let o = Command::new("git").args(["-c", "commit.gpgsign=false"]).args(args).current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", &global).env("GIT_CONFIG_NOSYSTEM", "1").env("HOME", &home).env("XDG_CONFIG_HOME", &home)
        .env("GIT_AUTHOR_NAME", "t").env("GIT_AUTHOR_EMAIL", "t@t").env("GIT_COMMITTER_NAME", "t").env("GIT_COMMITTER_EMAIL", "t@t")
        .output().unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8(o.stdout).unwrap().trim_end().to_string()
}

struct Ran { code: i32, out: String, err: String }
/// `run_in` with `top`, HEAD `C40`, today `2026-11-24`, the usage store `db`, and `env` for ZYGO_SESSION_DIR.
fn go(top: &Path, env: &dyn Fn(&str) -> Option<OsString>, db: &Path, args: &[&str]) -> Ran { go_head(top, Some(C40), env, db, args) }
/// `go` with HEAD `head`.
fn go_head(top: &Path, head: Option<&str>, env: &dyn Fn(&str) -> Option<OsString>, db: &Path, args: &[&str]) -> Ran {
    let list = |_: &Path, _: &str, _: usize| Ok(vec![]);
    let head = |p: &Path| { assert_eq!(p, top); head.map(str::to_string) };
    let today = || "2026-11-24".to_string();
    let t = || Some((top.to_path_buf(), top.join(".git")));
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let mut input = std::io::empty();
    let mut io = DecisionIo { input: &mut input, out: &mut out, err: &mut err, tty: false };
    let code = run_in(&Repo { checkout: &t, commits: &list, head: &head, today: &today, usage_db: db }, env, &args, &mut io);
    Ran { code, out: String::from_utf8(out).unwrap(), err: String::from_utf8(err).unwrap() }
}
fn run(top: &Path, args: &[&str]) -> Ran { go(top, &|_| None, &top.join("none.db"), args) }
const BASE: [&str; 11] = ["new", "--kind", "decision", "--title", "T", "--unit", "phase-3:N12", "--round", "1", "--role", "code"];
fn with(extra: &[&str]) -> Vec<String> { BASE.iter().chain(extra).map(|s| s.to_string()).collect() }
fn new(top: &Path, extra: &[&str]) -> Ran {
    let a = with(extra);
    run(top, &a.iter().map(String::as_str).collect::<Vec<_>>())
}
/// Each line that holds FILL_MARKER replaced: the reverse_cost line by `reverse_cost = "low"`, others by `Filled.`
fn fill(path: &Path) {
    let t = fs::read_to_string(path).unwrap();
    let lines: Vec<&str> = t.split('\n').map(|l| match l {
        l if l.contains(FILL_MARKER) && l.contains("reverse_cost") => "reverse_cost = \"low\"",
        l if l.contains(FILL_MARKER) => "Filled.",
        l => l,
    }).collect();
    fs::write(path, lines.join("\n")).unwrap();
}
fn session(sd: &Path, roles: &[&str]) {
    let legend = roles.iter().map(|r| LegendEntry { role: r.to_string(), description: String::new(), custom: false }).collect();
    SessionView { session: "s1".into(), legend, ..Default::default() }.save(&SessionDir::new(sd.to_path_buf())).unwrap();
}
fn rows(db: &Path, rows: &[(i64, &str, &str, &str)]) {
    let s = Store::open(db).unwrap();
    for (ts, session, role, model) in rows {
        let r = RequestRow { ts: *ts, session: session.to_string(), role_served: Some(role.to_string()), model: model.to_string(),
                             ..Default::default() };
        s.insert_request(&r).unwrap();
    }
}
fn path_of(r: &Ran) -> PathBuf { assert_eq!((r.code, r.err.as_str()), (0, ""), "{}", r.out); PathBuf::from(r.out.trim_end()) }
fn name_of(p: &Path) -> String { p.file_name().unwrap().to_str().unwrap().to_string() }

#[test] fn new_writes_a_template_that_check_refuses_until_filled() { // D§17 #1
    let t = tempfile::tempdir().unwrap();
    let top = t.path();
    let r = new(top, &[]);
    let p = path_of(&r);
    let name = name_of(&p);
    assert_eq!(p, top.join("docs/decisions").join(&name));
    assert!(matches!(name_kind(&name), NameKind::Record(_)), "{name}");
    let want: String = ["missing key reverse_cost", "unfilled hint in front matter", "empty section ## Choice", "empty section ## Alternatives rejected",
                        "empty section ## Evidence", "empty section ## Cost to reverse"]
        .iter().map(|f| format!("{name}: {f}\n")).collect();
    let c = run(top, &["check"]);
    assert_eq!((c.code, c.out.as_str(), c.err.as_str()), (1, want.as_str(), ""));
    fill(&p);
    let c = run(top, &["check"]);
    assert_eq!((c.code, c.out.as_str(), c.err.as_str()), (0, "", ""));
    let set = read_dir(top).unwrap();
    let rec = &set.valid[0];
    assert_eq!((rec.kind, rec.status, rec.class), (Kind::Decision, Status::Open, Class::None));
    assert_eq!((rec.title.as_str(), rec.unit.as_str(), rec.round, rec.role.as_str()), ("T", "phase-3:N12", 1, "code"));
    assert_eq!((rec.model.as_str(), rec.commit.as_str(), rec.date.as_str()), ("unknown", C40, "2026-11-24"));
    assert!(rec.depends_on.is_empty());
    assert_eq!((rec.reverse_cost.as_str(), &rec.supersedes), ("low", &None));
    assert_eq!((&rec.fixed_by, &rec.promoted_by, &rec.reviewed), (&None, &None, &None));
    assert_eq!((&rec.review_note, &rec.promoted_to, &rec.promotion), (&None, &None, &None));
}

#[test] fn new_fills_finding_class_links_and_title() { // D§6.2, D§6.3
    let t = tempfile::tempdir().unwrap();
    let top = t.path();
    put(top, 1, &[]);
    put(top, 2, &[]);
    let (i1, i2) = (id(1), id(2));
    let r = run(top, &["new", "--kind", "open-finding", "--title", "a \"b\" \\ c", "--unit", "phase-3:N12", "--round", "1",
                       "--role", "code", "--class", "security", "--depends-on", &i1, "--depends-on", &i2, "--supersedes", &i1]);
    let p = path_of(&r);
    let name = name_of(&p);
    let c = run(top, &["check"]);
    assert_eq!(c.code, 1);
    let lines: Vec<&str> = c.out.lines().collect();
    assert_eq!(&lines[..3], &[format!("{name}: missing key reverse_cost"), format!("{name}: unfilled hint in front matter"),
                              format!("{name}: empty section ## Finding")]);
    assert_eq!(fs::read_to_string(&p).unwrap().matches("\n## Finding\n").count(), 1);
    assert!(fs::read_to_string(&p).unwrap().contains("<!-- zygo: fill --> What stays open."));
    fill(&p);
    let stem = name.strip_suffix(".md").unwrap();
    let rec = parse_record(stem, &fs::read(&p).unwrap()).unwrap();
    assert_eq!((rec.kind, rec.class), (Kind::OpenFinding, Class::Security));
    assert_eq!(rec.depends_on, vec![i1.clone(), i2]);
    assert_eq!(rec.supersedes, Some(i1));
    assert_eq!(rec.title, "a \"b\" \\ c");
}

#[test] fn a_left_reverse_cost_hint_is_malformed() { // D§10.2, D§17 #1
    let t = tempfile::tempdir().unwrap();
    let top = t.path();
    let p = path_of(&new(top, &[]));
    let name = name_of(&p);
    let hint = |l: &str| l.contains(FILL_MARKER) && l.contains("reverse_cost");
    let text = fs::read_to_string(&p).unwrap();
    let lines: Vec<String> = text.split('\n').flat_map(|l| match l {
        l if hint(l) => vec![l.to_string(), "reverse_cost = \"low\"".to_string()],
        l if l.contains(FILL_MARKER) => vec!["Filled.".to_string()],
        l => vec![l.to_string()],
    }).collect();
    fs::write(&p, lines.join("\n")).unwrap();
    let c = run(top, &["check"]);
    assert_eq!((c.code, c.out.as_str(), c.err.as_str()), (1, format!("{name}: unfilled hint in front matter\n").as_str(), ""));
    let kept: Vec<&str> = lines.iter().map(String::as_str).filter(|l| !hint(l)).collect();
    fs::write(&p, kept.join("\n")).unwrap();
    let c = run(top, &["check"]);
    assert_eq!((c.code, c.out.as_str(), c.err.as_str()), (0, "", ""));
}

#[test] fn a_stop_class_decision_gets_an_unchecked_finding_heading() { // D§6.3
    let t = tempfile::tempdir().unwrap();
    let top = t.path();
    let p = path_of(&new(top, &["--class", "security"]));
    let text = fs::read_to_string(&p).unwrap();
    assert_eq!(text.matches("\n## Finding\n").count(), 1);
    let finding = text.split("\n## Finding\n").nth(1).unwrap().split("\n## ").next().unwrap().to_string();
    assert!(!finding.contains(FILL_MARKER), "{finding}");
    assert!(!finding.trim().is_empty());
    fill(&p);
    let c = run(top, &["check"]);
    assert_eq!((c.code, c.out.as_str(), c.err.as_str()), (0, "", ""));
    assert!(fs::read_to_string(&p).unwrap().contains(&finding));
    let p = path_of(&new(top, &[])); // class none: no Finding heading
    assert!(!fs::read_to_string(&p).unwrap().contains("## Finding"));
}

#[test] fn help_words_as_option_values_are_values() { // D§10.1
    let t = tempfile::tempdir().unwrap();
    let top = t.path();
    for v in ["-h", "--help"] {
        let p = path_of(&new(top, &["--title", v]));
        fill(&p);
        let rec = parse_record(name_of(&p).strip_suffix(".md").unwrap(), &fs::read(&p).unwrap()).unwrap();
        assert_eq!(rec.title, v);
    }
    for a in [&["new", "-h"][..], &["new", "--kind", "decision", "--help"], &["ls", "--pending", "-h"]] {
        let r = run(top, a);
        assert_eq!((r.code, r.out.as_str(), r.err.as_str()), (0, USAGE, ""), "{a:?}");
    }
}

#[test] fn an_unreadable_head_exits_1_and_writes_nothing() { // D§10.2
    let t = tempfile::tempdir().unwrap();
    let top = t.path();
    let a = with(&[]);
    let r = go_head(top, None, &|_| None, &top.join("none.db"), &a.iter().map(String::as_str).collect::<Vec<_>>());
    assert_eq!((r.code, r.out.as_str(), r.err.as_str()), (1, "", "zygo decision: cannot read HEAD\n"));
    assert!(!top.join("docs").exists());
}

#[test] fn two_linked_checkouts_get_different_ids_and_merge_cleanly() { // D§17 #2
    let t = tempfile::tempdir().unwrap();
    let (a, b) = (t.path().join("a"), t.path().join("b"));
    fs::create_dir(&a).unwrap();
    git(&a, &["init", "-q"]);
    git(&a, &["commit", "-q", "--allow-empty", "-m", "base"]);
    git(&a, &["worktree", "add", "-q", "../b", "-b", "side"]);
    let na = name_of(&path_of(&new(&a, &[])));
    let nb = name_of(&path_of(&new(&b, &[])));
    assert_ne!(na, nb);
    for d in [&a, &b] {
        git(d, &["add", "."]);
        git(d, &["commit", "-q", "-m", "r"]);
    }
    git(&a, &["merge", "-q", "--no-edit", "side"]);
    for n in [&na, &nb] {
        assert!(a.join("docs/decisions").join(n).is_file(), "{n}");
    }
}

#[test] fn model_is_unknown_outside_a_session_and_the_newest_row_inside() { // D§10.2
    let t = tempfile::tempdir().unwrap();
    let (top, sd, db) = (t.path().join("top"), t.path().join("sd"), t.path().join("usage.db"));
    fs::create_dir_all(&top).unwrap();
    fs::create_dir_all(&sd).unwrap();
    session(&sd, &["code", "review", "plan"]);
    rows(&db, &[(1, "s1", "code", "m-old"), (2, "s1", "code", "m-new"), (3, "s1", "review", "m-rev"), (5, "s2", "code", "m-other"),
                (6, "s1", "plan", "m-plan"), (7, "s1", "plan", "")]);
    let env = |k: &str| (k == "ZYGO_SESSION_DIR").then(|| sd.clone().into_os_string());
    let model = |env: &dyn Fn(&str) -> Option<OsString>, db: &Path, role: &str| {
        let a = ["new", "--kind", "decision", "--title", "T", "--unit", "phase-3:N12", "--round", "1", "--role", role];
        let p = path_of(&go(&top, env, db, &a));
        fill(&p);
        let rec = parse_record(name_of(&p).strip_suffix(".md").unwrap(), &fs::read(&p).unwrap()).unwrap();
        rec.model
    };
    assert_eq!(model(&env, &db, "code"), "m-new");
    assert_eq!(model(&env, &db, "lead"), "unknown"); // no row: also the serve-mode case
    assert_eq!(model(&env, &db, "plan"), "unknown"); // the newest row has an empty model
    assert_eq!(model(&|_| None, &db, "code"), "unknown");
    assert_eq!(model(&env, &t.path().join("missing.db"), "code"), "unknown");
}

#[test] fn argument_refusals_exit_2_and_write_nothing() { // D§10.2
    let t = tempfile::tempdir().unwrap();
    let (top, sd) = (t.path().join("top"), t.path().join("sd"));
    fs::create_dir_all(&top).unwrap();
    fs::create_dir_all(&sd).unwrap();
    session(&sd, &["code"]);
    let usage = USAGE.to_string();
    let title = "zygo decision new: --title must be one line of 1 to 120 bytes\n".to_string();
    let long = "x".repeat(121);
    let i1 = id(1);
    let mut many = Vec::new();
    for _ in 0..65 { many.extend(["--depends-on", i1.as_str()]); }
    let base = |extra: &[&str]| with(extra);
    let set = |k: &str, v: &str| { // BASE with flag `k` given the value `v` (a later flag replaces an earlier one)
        let mut a = with(&[]);
        a.extend([k.to_string(), v.to_string()]);
        a
    };
    let without_kind: Vec<String> = with(&[]).into_iter().enumerate().filter(|(i, _)| *i != 1 && *i != 2).map(|(_, s)| s).collect();
    let cases: Vec<(Vec<String>, String)> = vec![
        (set("--unit", "nounit"), "zygo decision new: --unit nounit is not a unit id\n".into()),
        (set("--round", "0"), "zygo decision new: --round must be 1 or more\n".into()),
        (set("--round", "x"), usage.clone()),
        (without_kind, usage.clone()),
        (set("--kind", "bogus"), usage.clone()),
        (set("--class", "bogus"), usage.clone()),
        (base(&["--frob"]), usage.clone()),
        (set("--title", ""), title.clone()),
        (set("--title", "a\nb"), title.clone()),
        (set("--title", &long), title.clone()),
        (set("--role", "Bad"), "zygo decision new: --role Bad is not a role name\n".into()),
        (base(&many), "zygo decision new: more than 64 --depends-on\n".into()),
    ];
    let none = t.path().join("none.db");
    let check = |a: &[String], env: &dyn Fn(&str) -> Option<OsString>, err: &str| {
        let r = go(&top, env, &none, &a.iter().map(String::as_str).collect::<Vec<_>>());
        assert_eq!((r.code, r.out.as_str(), r.err.as_str()), (2, "", err), "{a:?}");
        assert!(!top.join("docs").exists(), "{a:?}");
    };
    for (a, err) in &cases {
        check(a, &|_| None, err);
    }
    let env = |k: &str| (k == "ZYGO_SESSION_DIR").then(|| sd.clone().into_os_string());
    check(&set("--role", "review"), &env, "zygo decision new: --role review is not a role of this session\n");
    let a = set("--role", "lead");
    let r = go(&top, &env, &none, &a.iter().map(String::as_str).collect::<Vec<_>>());
    assert_eq!((r.code, r.err.as_str()), (0, ""));
}

#[test] fn depends_on_and_supersedes_must_name_valid_records() { // D§10.2
    let t = tempfile::tempdir().unwrap();
    let top = t.path();
    put(top, 1, &[]);
    fs::write(dir(top).join(md(2)), "x").unwrap();
    let (i2, i9) = (id(2), id(9));
    for (flag, v) in [("--depends-on", i9.as_str()), ("--depends-on", "nope"), ("--depends-on", &i2), ("--supersedes", &i9)] {
        let r = new(top, &[flag, v]);
        assert_eq!((r.code, r.out.as_str(), r.err.as_str()), (1, "", format!("zygo decision: {v} is not a valid record\n").as_str()));
    }
    let mut names: Vec<String> = fs::read_dir(dir(top)).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
    names.sort();
    assert_eq!(names, vec![md(1), md(2)]);
}

#[test] fn docs_decisions_a_file_exits_1() { // D§6.8
    let t = tempfile::tempdir().unwrap();
    fs::create_dir(t.path().join("docs")).unwrap();
    fs::write(t.path().join("docs/decisions"), "x").unwrap();
    let r = new(t.path(), &[]);
    assert_eq!((r.code, r.out.as_str(), r.err.as_str()), (1, "", "zygo decision: docs/decisions is not a directory\n"));
}

#[cfg(unix)]
#[test] fn a_write_failure_exits_1_and_a_left_tmp_is_ignored() { // D§10.2, review focus 44
    use std::os::unix::fs::PermissionsExt;
    // SAFETY: geteuid has no preconditions.
    if unsafe { libc::geteuid() } == 0 {
        eprintln!("skipped: root ignores directory modes");
        return;
    }
    let t = tempfile::tempdir().unwrap();
    let top = t.path();
    let d = dir(top);
    fs::set_permissions(&d, fs::Permissions::from_mode(0o500)).unwrap();
    let r = new(top, &[]);
    fs::set_permissions(&d, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!((r.code, r.out.as_str()), (1, ""));
    assert!(r.err.starts_with("zygo decision: cannot write "), "{}", r.err);
    assert_eq!(fs::read_dir(&d).unwrap().count(), 0);
    fs::write(d.join(format!("{}.tmp", id(3))), "+++\npartial").unwrap();
    for a in ["check", "ls"] {
        let r = run(top, &[a]);
        assert_eq!((r.code, r.out.as_str(), r.err.as_str()), (0, "", ""), "{a}");
    }
}

#[test]
#[ignore = "timing: git calls run under the 2 s production bound (BOUND)"]
fn binary_new_writes_head() {
    let t = tempfile::tempdir().unwrap();
    let (r, home) = (t.path().join("r"), t.path().join("home"));
    fs::create_dir_all(&r).unwrap();
    fs::create_dir_all(&home).unwrap();
    git(&r, &["init", "-q"]);
    git(&r, &["commit", "-q", "--allow-empty", "-m", "base"]);
    let mut c = Command::new(env!("CARGO_BIN_EXE_zygo"));
    c.arg("decision").args(&BASE[..]).current_dir(&r).env_remove("ZYGO_SESSION_DIR").env("GIT_CEILING_DIRECTORIES", t.path());
    let o = common::scratch_home(&mut c, &home).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let p = PathBuf::from(String::from_utf8(o.stdout).unwrap().trim_end());
    assert!(p.is_file(), "{}", p.display());
    fill(&p);
    let rec = parse_record(name_of(&p).strip_suffix(".md").unwrap(), &fs::read(&p).unwrap()).unwrap();
    assert_eq!((rec.commit, rec.model.as_str()), (git(&r, &["rev-parse", "HEAD"]), "unknown"));
}
