//! FIX-27: `zygo loadout show`, `zygo config show` and `zygo config check` print no secret and name what they remove
//! (W§11, W§12, AU30, AU59, AU98; W§18 #63). `zygo loadout` is a workspace verb (OP5: exit 2 on Windows), so its tests
//! are `cfg(unix)`; the `config` tests are not gated.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use toml::{Table, Value};

const G: &str = "<user info removed>";
const H: &str = "<inline JSON not shown>";
const V: &str = "<value not shown>";

/// (argument, shown) for one claude-code pane, in this order. Each hidden argument holds `SECRET`; no shown text does.
const FORMS: &[(&str, &str)] = &[
    ("https://u:SECRET1@h.example.invalid", G),
    ("DATABASE_URL=postgres://u:SECRET2@h.example.invalid/db", G),
    ("--env=URL=https://u:SECRET3@h.example.invalid", G),
    ("-d=https://u:SECRET4@h.example.invalid", G),
    ("-Xhttps://u:SECRET5@h.example.invalid", G),
    ("https://u:SECRET6#x@h.example.invalid", G),
    ("https://h.example.invalid/p?u:SECRET7@x", G),
    ("https:/u:SECRET8@h.example.invalid", G),
    (r"https:\\u:SECRET9@h.example.invalid", G),
    ("u:SECRET10@h.example.invalid", G),
    ("a@b:SECRET11@h.example.invalid", G),
    ("u:SECRET12?x@h.example.invalid", G),
    ("u:SECRET13#x@h.example.invalid", G),
    ("u:pa SECRET14@h.example.invalid", G),
    ("https://u:pa'SECRET15@h.example.invalid", G),
    ("--x=u:SECRET16@h.example.invalid", G),
    ("-du:SECRET17@h.example.invalid", G),
    ("node:20@sha256:0123abcd", G),
    ("{\"k\":\"SECRET19\"", H),
    (" [ \"SECRET20\" ]", H),
    ("--model={\"k\":\"SECRET21\"}", "--model=<inline JSON not shown>"),
    ("-d{\"k\":\"SECRET22\"}", "-d<inline JSON not shown>"),
    ("--api-key=SECRET23", "--api-key=<value not shown>"),
    ("--auth-token", "--auth-token"),
    ("SECRET24", V),
    ("--Password=SECRET25", "--Password=<value not shown>"),
    ("--model=x", "--model=x"),
    ("me@example.com", "me@example.com"),
    ("claude-sonnet-4@20250514", "claude-sonnet-4@20250514"),
    ("--model=claude-sonnet-4@20250514", "--model=claude-sonnet-4@20250514"),
    (r"C:\Users\first.last@corp\x.json", r"C:\Users\first.last@corp\x.json"),
    ("me@h.example.invalid:8080", "me@h.example.invalid:8080"),
    ("Authorization: Bearer tok-visible", "Authorization: <value not shown>"),
];

/// Attempt 1 round 3 F2: AU30 content flags and `--settings` with no file behind them, and JSON after any `=`.
const CONTENT_16: [&str; 16] = [
    r#"--agents={"r":{"prompt":"SECRET1"}}"#,
    r#"--json-schema={"type":"object","description":"SECRET2"}"#,
    "--agents", "agents-SECRET3.json",
    "--json-schema", "schema-SECRET4.json",
    "--plugin-url", "plugin-SECRET5",
    "--plugin-url=plugin-SECRET6",
    "--mcp-config", "mcp-SECRET7.json",
    "--settings=settings-SECRET8.json",
    r#"--model={"k":"SECRET9"}"#,
    r#"--x=["SECRET10"]"#,
    "--model=x",
    "--name=a=b",
];
#[allow(dead_code)] // carried with the other test values; the CLI tests use CONTENT_16[..13]
const CONTENT_16_SHOWN: [&str; 16] = [
    "--agents=<inline JSON not shown>", "--json-schema=<inline JSON not shown>",
    "--agents", H, "--json-schema", H, "--plugin-url", H,
    "--plugin-url=<inline JSON not shown>", "--mcp-config", H,
    "--settings=<inline JSON not shown>", "--model=<inline JSON not shown>", "--x=<inline JSON not shown>",
    "--model=x", "--name=a=b",
];

/// Attempt 1 round 3 F1, corrected for D1 (a whole token is replaced).
const USER_INFO_12: [&str; 12] = [
    "--x=https:/u:SECRET1@h.example.invalid",
    r"https:\\u:SECRET2@h.example.invalid",
    r"https:\u:SECRET3@h.example.invalid",
    "u:SECRET4@h.example.invalid",
    "https://u:pa'SECRET5@h.example.invalid",
    "https://u:pa\"SECRET6@h.example.invalid",
    "https://u:pa`SECRET7@h.example.invalid",
    "https://u:pa SECRET8@h.example.invalid",
    "https://u:SECRET9@h.example.invalid/x",
    "--model=x",
    "https://h.example.invalid/x",
    "plain",
];
#[allow(dead_code)] // carried with the other test values; the CLI tests use USER_INFO_12[..7]
const USER_INFO_12_SHOWN: [&str; 12] = [G, G, G, G, G, G, G, G, G, "--model=x", "https://h.example.invalid/x", "plain"];

/// Attempt 1 `workspace_cli.rs`.
#[cfg(unix)]
const PAIR_CFG: &str = r#"
[loadout.pair]
[[loadout.pair.window]]
name = "agents"
[[loadout.pair.window.pane]]
kind = "claude-code"
server = 'https://u:sekrit1@zygo.example.invalid'
[[loadout.pair.window.pane]]
kind = "claude-code"
server = 'https:/u:sekrit2@zygo.example.invalid'
[[loadout.pair.window.pane]]
kind = "claude-code"
server = 'https:\\u:sekrit3@zygo.example.invalid'
[[loadout.pair.window.pane]]
kind = "opencode"
server = 'u:sekrit4@zygo.example.invalid'
[[loadout.pair.window.pane]]
kind = "opencode"
server = 'https://zygo.example.invalid'
[loadout.solo]
[[loadout.solo.window]]
[[loadout.solo.window.pane]]
kind = "shell"
"#;

/// Attempt 1 `cli_surface.rs`.
const LOADOUT_CFG: &str = r#"
[loadout.pair]
[[loadout.pair.window]]
[[loadout.pair.window.pane]]
kind = "claude-code"
compact = "5k"
server = "https://u:sekrit@zygo.example.invalid"
args = ["--settings", "x", "-p", "hi"]
[[loadout.pair.window.pane]]
kind = "opencode"
compact = "on"
"#;

/// Attempt 1 round 1 M3.
const SHOW_CFG: &str = r#"
[loadout.pair]
[[loadout.pair.window]]
[[loadout.pair.window.pane]]
kind = "claude-code"
server = "https://u:SECRETA@h.example.invalid"
args = ["https://u:SECRETB@h.example.invalid/x", "--settings", '{"env":{"ANTHROPIC_API_KEY":"sk-SECRET1"}}', "--mcp-config={\"headers\":{\"Authorization\":\"Bearer SECRET2\"}}", '["SECRET3"]', "--model=x"]
"#;

/// Attempt 1 round 1 B1.
const DASH_CFG: &str = r#"
[loadout.pair]
[[loadout.pair.window]]
[[loadout.pair.window.pane]]
kind = "claude-code"
args = ["--system-prompt", "--api-token-is-SECRETVALUE=x", "--append-system-prompt", "-SECRET2"]
[[loadout.pair.window.pane]]
kind = "opencode"
args = ["--prompt", "--SECRET3=y", "-s", "-SECRET4"]
"#;

const W: &str = "warning: loadout.pair.window[0].pane[0].args: removed from the saved arguments (AU30): ";
const NO_FOLDER: &str =
    "zygo: the pane folder could not be found (zygo up needs a git repository); paths are checked against the current folder\n";

/// The config `[loadout.pair]` with one window of `claude-code` panes, each with its `args` and, when given, its `cwd`.
fn pair_of(panes: &[(&[&str], Option<&str>)]) -> String {
    let panes: Vec<Value> = panes
        .iter()
        .map(|(args, cwd)| {
            let mut p = Table::new();
            p.insert("kind".into(), Value::String("claude-code".into()));
            p.insert("args".into(), Value::Array(args.iter().map(|a| Value::String(a.to_string())).collect()));
            if let Some(c) = cwd {
                p.insert("cwd".into(), Value::String(c.to_string()));
            }
            Value::Table(p)
        })
        .collect();
    let mut window = Table::new();
    window.insert("pane".into(), Value::Array(panes));
    let mut pair = Table::new();
    pair.insert("window".into(), Value::Array(vec![Value::Table(window)]));
    let mut loadout = Table::new();
    loadout.insert("pair".into(), Value::Table(pair));
    let mut top = Table::new();
    top.insert("loadout".into(), Value::Table(loadout));
    toml::to_string(&top).unwrap()
}

fn one_pane_at(args: &[&str], cwd: Option<&str>) -> String {
    pair_of(&[(args, cwd)])
}

fn one_pane(args: &[&str]) -> String {
    one_pane_at(args, None)
}

/// `zygo <args>` with `<home>/config.toml` = `config`, run in `cwd` (the form of `with_config` in `cli_surface.rs`).
/// No git variable of the caller reaches `zygo`, and git looks for no repository above `home` (review round 2, 7).
fn zygo_at(home: &Path, cwd: &Path, config: &str, args: &[&str]) -> Output {
    std::fs::write(home.join("config.toml"), config).unwrap();
    Command::new(env!("CARGO_BIN_EXE_zygo"))
        .args(args)
        .env("HOME", home)
        .env("LOCALAPPDATA", home)
        .env("APPDATA", home)
        .env("ZYGO_CONFIG", home.join("config.toml"))
        .env_remove("ZYGO_SESSION_DIR")
        .env_remove("ZYGO_AGENT")
        .env_remove("XDG_DATA_HOME")
        .env_remove("XDG_CACHE_HOME")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env("GIT_CEILING_DIRECTORIES", home)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn git(dir: &Path, args: &[&str]) {
    let o = Command::new("git")
        .args(["-c", "user.name=zygo-test", "-c", "user.email=zygo-test@example.invalid", "-c", "init.defaultBranch=main"])
        .args(args)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .current_dir(dir)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
}

/// A repository at `<home>/r` with one empty commit.
fn repo(home: &Path) -> PathBuf {
    let r = home.join("r");
    git(home, &["init", "-q", r.to_str().unwrap()]);
    git(home, &["-C", r.to_str().unwrap(), "commit", "-q", "--allow-empty", "-m", "init"]);
    r
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// The `args` of pane 0 of `loadout show pair`.
#[cfg(unix)]
fn args_of_show(stdout: &str) -> Vec<String> {
    let t: Table = stdout.parse().unwrap();
    t["loadout"]["pair"]["window"][0]["pane"][0]["args"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect()
}

/// The one `config show` line that holds `args = `.
fn args_line(stdout: &str) -> String {
    let lines: Vec<&str> = stdout.lines().filter(|l| l.contains("args = ")).collect();
    assert_eq!(lines.len(), 1, "{stdout}");
    lines[0].to_string()
}

/// The TOML array text of `items`.
fn array_text(items: &[&str]) -> String {
    Value::Array(items.iter().map(|s| Value::String(s.to_string())).collect()).to_string()
}

fn inputs() -> Vec<&'static str> {
    FORMS.iter().map(|f| f.0).collect()
}

fn shown() -> Vec<&'static str> {
    FORMS.iter().map(|f| f.1).collect()
}

fn strings(a: &[&str]) -> Vec<String> {
    a.iter().map(|x| x.to_string()).collect()
}

const SHOW_FORMS: [&[&str]; 2] = [&["config", "show"], &["config", "show", "--layer", "user"]];

// ---- zygo loadout show ----

#[cfg(unix)]
#[test] // attempt 1 `loadout_list_and_show`, show part; kills: a server shown with user info
fn loadout_show_hides_server_user_info() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path();
    let o = zygo_at(home, home, PAIR_CFG, &["loadout", "show", "pair"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    let text = out(&o);
    for s in ["sekrit1", "sekrit2", "sekrit3", "sekrit4", "u:"] {
        assert!(!text.contains(s), "{s}: {text}");
    }
    let shown: Table = text.parse().unwrap();
    let panes = shown["loadout"]["pair"]["window"][0]["pane"].as_array().unwrap();
    for p in &panes[..4] {
        assert_eq!(p["server"].as_str(), Some(G), "{text}");
    }
    assert_eq!(panes[4]["server"].as_str(), Some("https://zygo.example.invalid"));
    let o = zygo_at(home, home, PAIR_CFG, &["loadout", "show", "solo"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    let solo: Table = out(&o).parse().unwrap();
    assert_eq!(solo["loadout"]["solo"]["window"][0]["pane"][0]["kind"].as_str(), Some("shell"));
    let o = zygo_at(home, home, "", &["loadout", "show", "solo"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    let shown: Table = out(&o).parse().unwrap();
    assert_eq!(shown["loadout"]["solo"].clone().try_into::<zygo_config::Loadout>().unwrap(), zygo_workspace::loadout::builtin_solo());
    let o = zygo_at(home, home, PAIR_CFG, &["loadout", "show", "nope"]);
    assert_eq!((o.status.code(), out(&o).as_str(), err(&o).as_str()), (Some(1), "", "zygo: unknown loadout `nope`\n"));
}

#[cfg(unix)]
#[test] // kills: the process folder as the base
fn loadout_show_takes_a_relative_content_file_from_the_pane_cwd() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path();
    let proj = home.join("proj");
    std::fs::create_dir(&proj).unwrap();
    std::fs::write(proj.join("m.json"), "{}").unwrap();
    let cfg = one_pane_at(&["--mcp-config", "m.json", "--agents", "gone.json"], Some(proj.to_str().unwrap()));
    let o = zygo_at(home, home, &cfg, &["loadout", "show", "pair"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(args_of_show(&out(&o)), strings(&["--mcp-config", "m.json", "--agents", H]));
    assert_eq!(err(&o), NO_FOLDER);
}

#[cfg(unix)]
#[test] // D5, F9, F10; kills: two code paths
fn loadout_show_and_config_show_print_the_same_args() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path();
    let cfg = one_pane(&inputs());
    let o = zygo_at(home, home, &cfg, &["loadout", "show", "pair"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(args_of_show(&out(&o)), strings(&shown()));
    assert!(!out(&o).contains("SECRET"), "{}", out(&o));
    let o = zygo_at(home, home, &cfg, &["config", "show"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert!(args_line(&out(&o)).contains(&array_text(&shown())), "{}", out(&o));
    assert!(!out(&o).contains("SECRET"), "{}", out(&o));
}

// ---- zygo config show ----

#[test] // F3 on `config show` alone
fn config_show_hides_every_form() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path();
    let cfg = one_pane(&inputs());
    for args in SHOW_FORMS {
        let o = zygo_at(home, home, &cfg, args);
        assert_eq!(o.status.code(), Some(0), "{args:?}: {}", err(&o));
        let text = out(&o);
        assert!(args_line(&text).contains(&array_text(&shown())), "{args:?}: {text}");
        assert!(!text.contains("SECRET") && !err(&o).contains("SECRET"), "{args:?}: {text}");
        for s in ["\"--model=x\"", "\"me@example.com\"", "\"claude-sonnet-4@20250514\"", r"'C:\Users\first.last@corp\x.json'"] {
            assert!(text.contains(s), "{args:?}: {s}: {text}");
        }
    }
}

#[test] // round 1 M3
fn config_show_hides_loadout_user_info_and_inline_json() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path();
    for args in SHOW_FORMS {
        let o = zygo_at(home, home, SHOW_CFG, args);
        assert_eq!(o.status.code(), Some(0), "{args:?}: {}", err(&o));
        let text = out(&o);
        assert!(!text.contains("SECRET") && !err(&o).contains("SECRET"), "{args:?}: {text}");
        for s in [
            "server = \"<user info removed>\"",
            "[\"<user info removed>\", \"--settings\", \"<inline JSON not shown>\"",
            "\"--mcp-config=<inline JSON not shown>\", \"<inline JSON not shown>\"",
            "\"--model=x\"",
        ] {
            assert!(text.contains(s), "{args:?}: {s}: {text}");
        }
    }
}

#[test] // round 3 F1 F2
fn config_show_hides_args_with_user_info_and_content_flag_values() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path();
    let cfg = one_pane(&[&USER_INFO_12[..7], &CONTENT_16[..13], &["--model=x"]].concat());
    let want = [
        G, G, G, G, G, G, G, "--agents=<inline JSON not shown>", "--json-schema=<inline JSON not shown>", "--agents", H,
        "--json-schema", H, "--plugin-url", H, "--plugin-url=<inline JSON not shown>", "--mcp-config", H,
        "--settings=<inline JSON not shown>", "--model=<inline JSON not shown>", "--model=x",
    ];
    assert_eq!(want.len(), 21);
    for args in SHOW_FORMS {
        let o = zygo_at(home, home, &cfg, args);
        assert_eq!(o.status.code(), Some(0), "{args:?}: {}", err(&o));
        let text = out(&o);
        assert!(!text.contains("SECRET") && !err(&o).contains("SECRET"), "{args:?}: {text}");
        assert!(args_line(&text).contains(&array_text(&want)), "{args:?}: {text}");
    }
}

// ---- zygo config check ----

#[test] // W§11, AU98, AU30; F13: `on` is an error
fn config_check_reports_loadout_compact_server_and_removable_args() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path();
    let o = zygo_at(home, home, LOADOUT_CFG, &["config", "check"]);
    assert_eq!(o.status.code(), Some(11), "{}", err(&o));
    assert_eq!(
        out(&o),
        "error: loadout.pair.window[0].pane[0].compact: bad --compact value `5k`: use <tokens>, <n>k or <n>m from 100k to 10m, or off\n\
         error: loadout.pair.window[0].pane[0].server: holds user info; zygo up starts this pane as a shell and does not save it (AU98)\n\
         warning: loadout.pair.window[0].pane[0].args: removed from the saved arguments (AU30): -p <positional>\n\
         error: loadout.pair.window[0].pane[1].compact: bad --compact value `on`: use <tokens>, <n>k or <n>m from 100k to 10m, or off\n"
    );
    assert!(!out(&o).contains("sekrit") && !err(&o).contains("sekrit"));
}

#[test] // round 1 B1
fn config_check_never_labels_a_value_that_starts_with_a_dash() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path();
    let o = zygo_at(home, home, DASH_CFG, &["config", "check"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(
        out(&o),
        "warning: loadout.pair.window[0].pane[0].args: removed from the saved arguments (AU30): --system-prompt --append-system-prompt\n\
         warning: loadout.pair.window[0].pane[1].args: removed from the saved arguments (AU30): --prompt -s\n"
    );
    for s in ["SECRET", "--api-token", "-S"] {
        assert!(!out(&o).contains(s) && !err(&o).contains(s), "{s}");
    }
}

#[test]
fn config_check_never_prints_a_removed_argument_value() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path();
    let cfg = one_pane(&["--system-prompt", "SECRET1", "--append-system-prompt=SECRET2", "-p", "SECRET3", "--", "SECRET4"]);
    let o = zygo_at(home, home, &cfg, &["config", "check"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(out(&o), format!("{W}--system-prompt --append-system-prompt -p <positional> -- <after -->\n"));
    for s in ["SECRET1", "SECRET2", "SECRET3", "SECRET4"] {
        assert!(!out(&o).contains(s) && !err(&o).contains(s), "{s}");
    }
}

#[test] // round 3 F5, decision 3; kills: a label by text, one label per short group, an empty line
fn config_check_labels_a_removed_token_by_its_index() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path();
    let cases: [(&[&str], &str); 4] = [
        (&["--model", "-p", "-p"], "-p"),
        (&["--system-prompt", "--", "-p", "SECRET"], "--system-prompt -p <positional>"),
        (&["--model", "--", "-p"], "-p"),
        (&["-cr"], "-c -r"),
    ];
    for (args, labels) in cases {
        let o = zygo_at(home, home, &one_pane(args), &["config", "check"]);
        assert_eq!((o.status.code(), out(&o)), (Some(0), format!("{W}{labels}\n")), "{args:?}");
    }
}

#[test] // kills: the process folder as the walk base; a flag that loses only a value left unnamed
fn config_check_takes_a_relative_content_file_from_the_pane_cwd() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path();
    let proj = home.join("proj");
    std::fs::create_dir(&proj).unwrap();
    std::fs::write(proj.join("m.json"), "{}").unwrap();
    let p = proj.to_str().unwrap();
    let cfg = pair_of(&[
        (&["--mcp-config", "m.json"], Some(p)),
        (&["--mcp-config", "m.json", r#"{"k":"SECRET"}"#], Some(p)),
        (&["--mcp-config", "m.json"], None),
    ]);
    let o = zygo_at(home, home, &cfg, &["config", "check"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(
        out(&o),
        "warning: loadout.pair.window[0].pane[1].args: removed from the saved arguments (AU30): --mcp-config\n\
         warning: loadout.pair.window[0].pane[2].args: removed from the saved arguments (AU30): --mcp-config\n"
    );
    assert_eq!(err(&o), NO_FOLDER);
    assert!(!out(&o).contains("SECRET"));
}

/// W§18 #63 in full: a `vim` pane and an OpenCode pane with `passthrough = true` are reported; the pane with
/// `args = ["--settings", "x"]` is not.
const W18_63_CFG: &str = r#"
[loadout.pair]
[[loadout.pair.window]]
[[loadout.pair.window.pane]]
kind = "vim"
[[loadout.pair.window.pane]]
kind = "opencode"
passthrough = true
[[loadout.pair.window.pane]]
kind = "claude-code"
args = ["--settings", "x"]
"#;

#[test] // W§18 #63, AU59; kills: `--settings` reported by the AU30 warning (round 2 finding 8: the whole #63)
fn w18_63_config_check_does_not_report_settings() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path();
    let o = zygo_at(home, home, W18_63_CFG, &["config", "check"]);
    assert_eq!(
        (o.status.code(), out(&o).as_str()),
        (
            Some(11),
            "error: loadout.pair.window[0].pane[0].kind: must be \"claude-code\", \"opencode\" or \"shell\"\n\
             error: loadout.pair.window[0].pane[1].passthrough: only with kind = \"claude-code\"\n"
        )
    );
    let o = zygo_at(home, home, &one_pane(&["--settings", "x"]), &["config", "check"]);
    assert_eq!((o.status.code(), out(&o).as_str()), (Some(0), "config ok\n"));
}

#[test] // round 2 finding 3; kills: the `compact` value printed as typed in the error line
fn config_check_hides_user_info_in_a_bad_compact_value() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path();
    let cfg = "[loadout.pair]\n[[loadout.pair.window]]\n[[loadout.pair.window.pane]]\nkind = \"claude-code\"\n\
               compact = \"https://u:SECRET1@h.example.invalid/x\"\n[[loadout.pair.window.pane]]\nkind = \"opencode\"\n\
               compact = \"u:SECRET2@h.example.invalid\"\n";
    let o = zygo_at(home, home, cfg, &["config", "check"]);
    assert_eq!(
        (o.status.code(), out(&o).as_str()),
        (
            Some(11),
            "error: loadout.pair.window[0].pane[0].compact: bad --compact value `<user info removed>`: use <tokens>, <n>k or <n>m from 100k to 10m, or off\n\
             error: loadout.pair.window[0].pane[1].compact: bad --compact value `<user info removed>`: use <tokens>, <n>k or <n>m from 100k to 10m, or off\n"
        )
    );
    assert!(!err(&o).contains("SECRET"));
}

#[test] // round 2 finding 6; kills: a folder lookup for a shown layer that cannot hold a loadout
fn config_show_of_a_layer_without_loadouts_runs_no_folder_lookup() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path();
    let cfg = one_pane(&["--mcp-config", "m.json"]);
    for layer in ["team", "builtin", "repository"] {
        let o = zygo_at(home, home, &cfg, &["config", "show", "--layer", layer]);
        assert_eq!((o.status.code(), err(&o).as_str()), (Some(0), ""), "{layer}");
    }
    let o = zygo_at(home, home, &cfg, &["config", "show", "--layer", "user"]);
    assert_eq!((o.status.code(), err(&o).as_str()), (Some(0), NO_FOLDER));
}

/// Round 3 finding 1: secret forms in arguments and fields, and a control character.
const PARITY_CFG: &str = r#"
[loadout.pair]
[[loadout.pair.window]]
name = "x://u:SECRET6@h"
layout = "u:pa SECRET7@h"
[[loadout.pair.window.pane]]
kind = "claude-code"
profile = "s://u:SECRET5@h/p"
compact = "API_KEY = SECRET8"
args = ["x://u:SECRET1@h", "s://u:SECRET2@h/p", '{"k":"SECRET3"}', "TOKEN=SECRET4", "a\u202Eb", "https://h/x#token=SECRET9"]
"#;

/// The one line of `text` that starts with `<key> = `.
fn key_line<'a>(text: &'a str, key: &str) -> &'a str {
    let lines: Vec<&str> = text.lines().filter(|l| l.starts_with(&format!("{key} = "))).collect();
    assert_eq!(lines.len(), 1, "{key}: {text}");
    lines[0]
}

#[cfg(unix)]
#[test] // round 3 finding 1; kills: `loadout show` with a step that `config show` has not (the round 2 whole-text scrub
// with the round 2 drive-letter rule), or without the line escape
fn loadout_show_and_config_show_print_the_same_secret_forms() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path();
    let shown = zygo_at(home, home, PARITY_CFG, &["loadout", "show", "pair"]);
    assert_eq!(shown.status.code(), Some(0), "{}", err(&shown));
    let config = zygo_at(home, home, PARITY_CFG, &["config", "show"]);
    assert_eq!(config.status.code(), Some(0), "{}", err(&config));
    let (shown, config) = (out(&shown), out(&config));
    assert!(!shown.contains("SECRET") && !config.contains("SECRET"), "{shown}\n{config}");
    let line = args_line(&config);
    for key in ["args", "name", "layout", "profile", "compact"] {
        let want = key_line(&shown, key);
        assert!(line.as_bytes().windows(want.len()).any(|w| w == want.as_bytes()), "{want}\n{line}");
    }
    assert!(key_line(&shown, "args").contains(r#""<user info removed>", "<user info removed>", "<inline JSON not shown>", "TOKEN=<value not shown>", "a\u{202e}b""#), "{shown}");
}

#[test] // round 3 finding 5; kills: the `compact` check line through the old field scrub, which stops at a blank
fn config_check_reads_a_compact_value_as_one_argument() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path();
    let cfg = "[loadout.pair]\n[[loadout.pair.window]]\n[[loadout.pair.window.pane]]\nkind = \"claude-code\"\n\
               compact = \"https://u:pa SECRET1@h\"\n";
    let o = zygo_at(home, home, cfg, &["config", "check"]);
    assert_eq!(
        (o.status.code(), out(&o).as_str()),
        (
            Some(11),
            "error: loadout.pair.window[0].pane[0].compact: bad --compact value `<user info removed>`: use <tokens>, <n>k or <n>m from 100k to 10m, or off\n"
        )
    );
}

#[cfg(unix)]
#[test] // round 2 finding 2; kills: `loadout show` that prints bidi or C1 characters raw while `config show` escapes them
fn loadout_show_and_config_show_escape_the_same_way() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path();
    let cfg = one_pane(&["a\u{202e}b", "c\u{9b}d", "e\u{200f}f"]);
    let shown = zygo_at(home, home, &cfg, &["loadout", "show", "pair"]);
    assert_eq!(shown.status.code(), Some(0), "{}", err(&shown));
    let config = zygo_at(home, home, &cfg, &["config", "show"]);
    assert_eq!(config.status.code(), Some(0), "{}", err(&config));
    let (shown, config) = (out(&shown), out(&config));
    for c in ['\u{202e}', '\u{9b}', '\u{200f}'] {
        assert!(!shown.contains(c) && !config.contains(c), "{c:?}");
    }
    let args: Vec<&str> = shown.lines().filter(|l| l.starts_with("args = ")).collect();
    assert_eq!(args.len(), 1, "{shown}");
    // byte for byte: the `args = [...]` text of `loadout show` is in the `config show` line
    assert!(args_line(&config).as_bytes().windows(args[0].len()).any(|w| w == args[0].as_bytes()), "{}\n{}", args[0], args_line(&config));
}

// ---- the pane folder (D6) ----

fn check_ok(home: &Path, from: &Path, cwd: Option<&str>) {
    let o = zygo_at(home, from, &one_pane_at(&["--mcp-config", "m.json"], cwd), &["config", "check"]);
    assert_eq!((o.status.code(), out(&o).as_str(), err(&o).as_str()), (Some(0), "config ok\n", ""));
}

#[test] // kills: the current folder as the base
fn config_check_takes_the_main_worktree_from_a_repository_subfolder() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path();
    let r = repo(home);
    std::fs::write(r.join("m.json"), "{}").unwrap();
    std::fs::create_dir(r.join("sub")).unwrap();
    check_ok(home, &r.join("sub"), None);
    let o = zygo_at(home, &r.join("sub"), &one_pane(&["--mcp-config", "m.json"]), &["config", "show"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert!(args_line(&out(&o)).contains(r#"["--mcp-config", "m.json"]"#), "{}", out(&o));
}

#[test] // kills: the checkout top in place of the main worktree
fn config_check_takes_the_main_worktree_from_a_linked_worktree() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path();
    let r = repo(home);
    let lw = home.join("lw");
    git(&r, &["worktree", "add", "-q", "-b", "lw", lw.to_str().unwrap()]);
    std::fs::write(r.join("m.json"), "{}").unwrap();
    check_ok(home, &lw, None);
}

#[test] // kills: a copy of `find_checkout` without its bare case
fn config_check_takes_the_main_worktree_from_a_bare_repository() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path();
    let r = repo(home);
    let b = home.join("b.git");
    let bw = home.join("bw");
    git(home, &["clone", "-q", "--bare", r.to_str().unwrap(), b.to_str().unwrap()]);
    git(&b, &["worktree", "add", "-q", bw.to_str().unwrap()]);
    std::fs::write(bw.join("m.json"), "{}").unwrap();
    check_ok(home, &b, None);
}

#[test] // kills: `branch:` not resolved
fn config_check_takes_a_branch_cwd_from_its_worktree() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path();
    let r = repo(home);
    let lx = home.join("lx");
    git(&r, &["worktree", "add", "-q", "-b", "x", lx.to_str().unwrap()]);
    std::fs::write(lx.join("m.json"), "{}").unwrap();
    check_ok(home, &r, Some("branch:x"));
}

#[test] // kills: a silent fallback; a lookup for a config with no loadout
fn config_check_and_show_warn_when_the_pane_folder_cannot_be_found() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path();
    std::fs::write(home.join("m.json"), "{}").unwrap();
    let cfg = one_pane(&["--mcp-config", "m.json"]);
    let o = zygo_at(home, home, &cfg, &["config", "check"]);
    assert_eq!((o.status.code(), out(&o).as_str(), err(&o).as_str()), (Some(0), "config ok\n", NO_FOLDER));
    let o = zygo_at(home, home, &cfg, &["config", "show"]);
    assert_eq!((o.status.code(), err(&o).as_str()), (Some(0), NO_FOLDER));
    let o = zygo_at(home, home, "", &["config", "check"]);
    assert_eq!((o.status.code(), out(&o).as_str(), err(&o).as_str()), (Some(0), "config ok\n", ""));
}
