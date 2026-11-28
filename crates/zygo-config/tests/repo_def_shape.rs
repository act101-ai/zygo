//! WD-2: shapes, allow-list validation and digest of `.zygo/` (R§6.2-R§6.5, R-C3, R-C7, R§19 PR3, R§18 item 4).
use std::collections::BTreeMap;
use zygo_config::repo_def::{self, Definition, Files, MemberDef, RepoProfile, WorkspaceFile};

const W: &str = ".zygo/workspace.toml";
const P: &str = ".zygo/profiles/rust.toml";

fn files(ws: Option<&str>, profiles: &[(&str, &str)]) -> Files {
    Files {
        full_ref: Some("refs/remotes/origin/main".into()),
        commit: "a".repeat(40),
        workspace: ws.map(|t| t.as_bytes().to_vec()),
        profiles: profiles.iter().map(|(n, t)| (n.to_string(), t.as_bytes().to_vec())).collect(),
        ignored: vec![],
    }
}
fn ws_err(text: &str) -> String {
    repo_def::parse(&files(Some(text), &[])).unwrap_err()
}
fn prof_err(text: &str) -> String {
    repo_def::parse(&files(None, &[("rust", text)])).unwrap_err()
}
fn member(body: &str) -> String {
    format!("version = 1\n[members.a]\n{body}\n")
}

#[test]
fn parse_rules_of_r18_item_4() {
    assert_eq!(
        ws_err(&member("args = [\"x\"]")),
        ".zygo/workspace.toml: members.a.args: not allowed in a repository file"
    );
    let many: String = (0..33).map(|i| format!("[members.m{i}]\n")).collect();
    let ws: Vec<(String, &str)> = vec![
        (member("config_dir = \"/x\""), "members.a.config_dir: not allowed in a repository file"),
        (member("passthrough = true"), "members.a.passthrough: not allowed in a repository file"),
        (member("yolo = true"), "members.a.yolo: not allowed in a repository file"),
        (member("server = \"x\""), "members.a.server: not allowed in a repository file"),
        (member("compact = true"), "members.a.compact: not allowed in a repository file"),
        (member("kind = \"shell\""), "members.a.kind: must be claude-code or opencode"),
        ("version = 1\n[[model]]\nid = \"x\"\n".into(), "model: not allowed in a repository file"),
        ("version = 1\n[redaction]\ncontent = false\n".into(), "redaction: not allowed in a repository file"),
        ("version = 1\n[update]\nchannel = \"x\"\n".into(), "update: not allowed in a repository file"),
        ("version = 1\n[workspace]\npool = \"/x\"\n".into(), "workspace: not allowed in a repository file"),
        ("version = 1\nfoo = 1\n".into(), "foo: not allowed in a repository file"),
        ("version = 2\n".into(), "version: must be the integer 1"),
        (
            "version = 1\n[members.a]\njob = \"orchestrator\"\n[members.b]\njob = \"orchestrator\"\n".into(),
            "members.b.job: at most one member can be the orchestrator",
        ),
        (format!("version = 1\n{many}"), "members: at most 32 members"),
        (
            member("kind = \"opencode\"\npool = true"),
            "members.a.pool: only a claude-code member can take a pool directory",
        ),
        (member("cwd = \"/tmp\""), "members.a.cwd: must not be an absolute path"),
        (member("cwd = \"../x\""), "members.a.cwd: must not hold a . or .. part"),
        (member("cwd = \"~/x\""), "members.a.cwd: must not start with ~"),
        (member("cwd = \"d\u{43e}cs\""), "members.a.cwd: must be printable ASCII (bytes 0x20-0x7e)"),
    ];
    for (text, want) in &ws {
        assert_eq!(ws_err(text), format!("{W}: {want}"), "input: {text}");
    }
    let prof = [
        ("overrides = { review = \"gpt-6-s\u{f3}l\" }\n", "overrides.review: must be 1-128 bytes of printable ASCII"),
        ("prompt = \"x\"\n", "prompt: not allowed in a repository file"),
        ("[roles.x]\nprompt = \"y\"\n", "roles: not allowed in a repository file"),
        ("plugins = [\"x\"]\n", "plugins: not allowed in a repository file"),
        ("[features]\nresults = false\n", "features: not allowed in a repository file"),
        ("", "a profile must set overrides or disabled"),
        ("overrides = {}\ndisabled = []\n", "a profile must set overrides or disabled"),
    ];
    for (text, want) in prof {
        assert_eq!(prof_err(text), format!("{P}: {want}"), "input: {text}");
    }
}

#[test]
fn allow_list_holds_at_every_depth() {
    assert_eq!(
        ws_err("version = 1\nmembers = { a = { kind = \"claude-code\", args = [\"x\"] } }\n"),
        ".zygo/workspace.toml: members.a.args: not allowed in a repository file"
    );
    assert_eq!(
        ws_err(&member("[members.a.extra]\nx = 1")),
        ".zygo/workspace.toml: members.a.extra: not allowed in a repository file"
    );
    assert_eq!(
        prof_err("overrides = { review = { model = \"x\" } }\n"),
        ".zygo/profiles/rust.toml: overrides.review: must be 1-128 bytes of printable ASCII"
    );
    assert_eq!(
        prof_err("disabled = [[\"x\"]]\n"),
        ".zygo/profiles/rust.toml: disabled[0]: a role name must be 1-32 of [a-z0-9-]"
    );
}

#[test]
fn value_rules() {
    let cases: Vec<(String, &str)> = vec![
        (
            "version = 1\nlayout = \"tiled,abc\"\n".into(),
            "layout: must be one of even-horizontal, even-vertical, main-horizontal, main-vertical, tiled",
        ),
        ("[members.a]\n".into(), "version: must be the integer 1"),
        ("version = 1.0\n".into(), "version: must be the integer 1"),
        ("version = 1\nmembers = 1\n".into(), "members: must be a table"),
        ("version = 1\nmembers = { a = 1 }\n".into(), "members.a: must be a table"),
        ("version = 1\n[members.Bad]\n".into(), "members.Bad: not a valid member name"),
        ("version = 1\n[members.all]\n".into(), "members.all: not a valid member name"),
        (member("job = \"lead\""), "members.a.job: must be orchestrator or worker"),
        (member("profile = \"none\""), "members.a.profile: must be 1-32 of [a-z0-9-] and not none"),
        (member("profile = \"Rust\""), "members.a.profile: must be 1-32 of [a-z0-9-] and not none"),
        (member("pool = \"yes\""), "members.a.pool: must be true or false"),
        (member("cwd = 1"), "members.a.cwd: must be a string"),
        (member("cwd = \"branch:\""), "members.a.cwd: a branch name must be 1-255 bytes"),
        (member(&format!("cwd = \"branch:{}\"", "b".repeat(256))), "members.a.cwd: a branch name must be 1-255 bytes"),
        (member(&format!("cwd = \"{}\"", vec!["a"; 513].join("/"))), "members.a.cwd: must be at most 1024 bytes"),
        (member(&format!("cwd = \"x/{}\"", "a".repeat(256))), "members.a.cwd: each part must be 1-255 bytes"),
        (member("cwd = \"a//b\""), "members.a.cwd: each part must be 1-255 bytes"),
        (member("cwd = 'a\\b'"), "members.a.cwd: must not hold \\"),
        (member("cwd = \"a/./b\""), "members.a.cwd: must not hold a . or .. part"),
    ];
    for (text, want) in &cases {
        assert_eq!(ws_err(text), format!("{W}: {want}"), "input: {text}");
    }
    let o17: String = (0..17).map(|i| format!("r{i} = \"m\"\n")).collect();
    let d17 = (0..17).map(|i| format!("\"r{i}\"")).collect::<Vec<_>>().join(", ");
    let prof: Vec<(String, &str)> = vec![
        ("overrides = 1\n".into(), "overrides: must be a table"),
        (format!("[overrides]\n{o17}"), "overrides: at most 16 entries"),
        ("overrides = { Review = \"m\" }\n".into(), "overrides.Review: a role name must be 1-32 of [a-z0-9-]"),
        ("overrides = { review = \"\" }\n".into(), "overrides.review: must be 1-128 bytes of printable ASCII"),
        (
            format!("overrides = {{ review = \"{}\" }}\n", "m".repeat(129)),
            "overrides.review: must be 1-128 bytes of printable ASCII",
        ),
        ("disabled = \"design\"\n".into(), "disabled: must be an array"),
        (format!("disabled = [{d17}]\n"), "disabled: at most 16 entries"),
        ("disabled = [\"design\", \"x y\"]\n".into(), "disabled[1]: a role name must be 1-32 of [a-z0-9-]"),
    ];
    for (text, want) in &prof {
        assert_eq!(prof_err(text), format!("{P}: {want}"), "input: {text}");
    }
    let mut f = files(None, &[]);
    f.workspace = Some(vec![0xff]);
    assert_eq!(repo_def::parse(&f).unwrap_err(), ".zygo/workspace.toml: not UTF-8");
    let ok32: String = (0..32).map(|i| format!("[members.m{i}]\n")).collect();
    let def = repo_def::parse(&files(Some(&format!("version = 1\n{ok32}")), &[])).unwrap();
    assert_eq!(def.workspace.unwrap().members.len(), 32);
}

#[test]
fn toml_syntax_error_is_message_and_position_only() {
    let e = ws_err("version = 1\n\nfoo = \n");
    assert_eq!(
        e,
        ".zygo/workspace.toml: TOML parse error at line 3, column 7: string values must be quoted, expected literal string"
    );
    let e = prof_err("disabled = [\"design\"]\ntoken = sk-live-123\n");
    assert!(e.starts_with(".zygo/profiles/rust.toml: TOML parse error at line 2, column "), "{e}");
    assert!(!e.contains('\n') && !e.contains('|') && !e.contains("sk-live-123"), "{e}");
}

#[test]
fn first_violation_wins_in_r6_4_order() {
    assert_eq!(
        repo_def::parse(&files(Some("version = 2\n"), &[("a", "prompt = \"x\"\n")])).unwrap_err(),
        ".zygo/workspace.toml: version: must be the integer 1"
    );
    assert_eq!(
        repo_def::parse(&files(Some("version = 1\n"), &[("b", "prompt = \"x\"\n"), ("a", "roles = 1\n")]))
            .unwrap_err(),
        ".zygo/profiles/a.toml: roles: not allowed in a repository file"
    );
    assert_eq!(ws_err("version = 1\nfoo = 1\nbar = 2\n"), ".zygo/workspace.toml: bar: not allowed in a repository file");
}

#[test]
fn key_path_is_escaped() {
    assert_eq!(
        ws_err("version = 1\n[members.\"a\\u001b\"]\n"),
        ".zygo/workspace.toml: members.a\\u{1b}: not a valid member name"
    );
    assert_eq!(prof_err("\"x\\u202e\" = 1\n"), ".zygo/profiles/rust.toml: x\\u{202e}: not allowed in a repository file");
}

#[test]
fn valid_files_parse_to_the_typed_shape() {
    let ws = "version = 1\nlayout = \"tiled\"\n[members.lead]\nkind = \"claude-code\"\njob = \"orchestrator\"\nprofile = \"rust\"\npool = true\ncwd = \"branch:feature-x\"\n[members.w]\nkind = \"opencode\"\ncwd = \"crates/zygo\"\n";
    let p = "overrides = { review = \"gpt-6-sol\", code = \"claude-sonnet-5-5\" }\ndisabled = [\"design\"]\n";
    assert_eq!(
        repo_def::parse(&files(Some(ws), &[("rust", p)])).unwrap(),
        Definition {
            workspace: Some(WorkspaceFile {
                version: 1,
                layout: Some("tiled".into()),
                members: BTreeMap::from([
                    (
                        "lead".into(),
                        MemberDef {
                            kind: Some("claude-code".into()),
                            job: Some("orchestrator".into()),
                            profile: Some("rust".into()),
                            pool: true,
                            cwd: Some("branch:feature-x".into()),
                        }
                    ),
                    (
                        "w".into(),
                        MemberDef {
                            kind: Some("opencode".into()),
                            job: None,
                            profile: None,
                            pool: false,
                            cwd: Some("crates/zygo".into()),
                        }
                    ),
                ]),
            }),
            profiles: BTreeMap::from([(
                "rust".into(),
                RepoProfile {
                    overrides: BTreeMap::from([
                        ("code".into(), "claude-sonnet-5-5".into()),
                        ("review".into(), "gpt-6-sol".into()),
                    ]),
                    disabled: vec!["design".into()],
                }
            )]),
        }
    );
    assert_eq!(repo_def::parse(&files(Some("version = 1\n"), &[])).unwrap().workspace.unwrap().members, BTreeMap::new());
    assert_eq!(repo_def::parse(&files(None, &[])).unwrap(), Definition { workspace: None, profiles: BTreeMap::new() });
}

#[test]
fn digest_is_sha256_of_the_r6_5_layout() {
    let f = files(Some("version = 1\n"), &[("rust", "disabled = [\"design\"]\n")]);
    let d = repo_def::digest(&f);
    assert_eq!(d, "2019b3cde22d08cf3f77e15dcd2e8983da90fc3f48fec869e020e20490454e37");
    assert_eq!(repo_def::short(&d), "2019b3cde22d");
    assert_eq!(repo_def::digest(&files(None, &[])), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    let mut g = f.clone();
    g.full_ref = None;
    g.commit = "b".repeat(40);
    g.ignored = vec![".zygo/README".into()];
    assert_eq!(repo_def::digest(&g), d);
}

#[test]
fn pr3_deep_nesting_does_not_overflow_the_stack() {
    let arrays = format!("a = {}{}", "[".repeat(32766), "]".repeat(32766));
    let tables = format!("a = {}{}", "{b = ".repeat(10922), "}".repeat(10922));
    for text in [arrays, tables] {
        assert_eq!(text.len() as u64, repo_def::WORKSPACE_MAX);
        let f = files(Some(&text), &[]);
        let r = std::thread::spawn(move || repo_def::parse(&f)).join().expect("no panic, no overflow");
        let e = r.unwrap_err();
        assert!(e.starts_with(".zygo/workspace.toml: TOML parse error at line 1, column "), "{e}");
    }
}

#[test]
fn cwd_absolute_check_is_textual_on_every_os() {
    for cwd in ["/tmp", "C:/x", "C:x", "c:"] {
        assert_eq!(
            ws_err(&member(&format!("cwd = \"{cwd}\""))),
            format!("{W}: members.a.cwd: must not be an absolute path"),
            "cwd: {cwd}"
        );
    }
    let def = repo_def::parse(&files(Some(&member("cwd = \"ab:c\"")), &[])).unwrap();
    assert_eq!(def.workspace.unwrap().members["a"].cwd.as_deref(), Some("ab:c"));
}

#[test]
fn pool_without_kind_is_claude_code() {
    let def = repo_def::parse(&files(Some(&member("pool = true")), &[])).unwrap();
    let a = &def.workspace.unwrap().members["a"];
    assert!(a.pool);
    assert_eq!(a.kind, None);
}

#[test]
fn each_r6_2_layout_name_parses() {
    for name in ["even-horizontal", "even-vertical", "main-horizontal", "main-vertical", "tiled"] {
        let def = repo_def::parse(&files(Some(&format!("version = 1\nlayout = \"{name}\"\n")), &[])).unwrap();
        assert_eq!(def.workspace.unwrap().layout.as_deref(), Some(name));
    }
    assert_eq!(
        ws_err("version = 1\nlayout = 1\n"),
        format!("{W}: layout: must be one of even-horizontal, even-vertical, main-horizontal, main-vertical, tiled")
    );
}
