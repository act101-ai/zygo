//! HUD-11: the `statusline` verb on the `zygo` binary (H§8, AU1, H§14 #33): `zygo statusline [compact|verbose]
//! [--save]` and `/zygo:statusline` (through `zygo hook prompt`), and the next render of `zygo hook statusline`.
//!
//! Operator decision (2026-11-21): an OpenCode session has the same verb with the same behavior as a Claude Code
//! session, so no form is refused for `ZYGO_AGENT=opencode`.

mod common;

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde_json::{json, Value};
use tempfile::TempDir;
use zygo_core::session_dir::SessionDir;
use zygo_core::session_view::SessionView;

const USAGE: &str = "zygo statusline: usage: statusline [compact|verbose]\n";
const NO_SESSION: &str = "zygo statusline: not in a zygo session; add --save to change user config\n";

/// A home directory with a user config, and a session `S1` that has no proxy (the HUD shows `offline`).
struct Fx {
    home: TempDir,
    dir: PathBuf,
}

/// `view`: the `statusline_style` that the launcher wrote into `view.json`; `None` writes no `view.json`.
fn fx(config: &str, view: Option<&str>) -> Fx {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(home.path().join("config.toml"), config).unwrap();
    std::fs::write(home.path().join(".gitconfig"), "").unwrap();
    let dir = home.path().join("sessions").join("S1");
    let sd = SessionDir::new(dir.clone());
    sd.create().unwrap();
    if let Some(style) = view {
        SessionView { session: "S1".into(), statusline_style: style.into(), ..Default::default() }.save(&sd).unwrap();
    }
    Fx { home, dir }
}

/// Where a run is: in the session `S1` or outside a session; and the agent name of `ZYGO_AGENT`.
#[derive(Clone, Copy)]
struct At {
    session: bool,
    agent: Option<&'static str>,
}

const IN: At = At { session: true, agent: None };
const OUT: At = At { session: false, agent: None };
const IN_OPENCODE: At = At { session: true, agent: Some("opencode") };
const OUT_OPENCODE: At = At { session: false, agent: Some("opencode") };

impl Fx {
    fn run(&self, args: &[&str], at: At, stdin: &[u8]) -> (i32, String, String) {
        let home = self.home.path();
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_zygo"));
        for (k, _) in std::env::vars_os() {
            if k.to_str().is_some_and(|k| k.starts_with("GIT_") || k.starts_with("ZYGO_")) {
                cmd.env_remove(k);
            }
        }
        common::scratch_home(&mut cmd, home);
        cmd.args(args)
            .current_dir(home)
            .env("GIT_CONFIG_GLOBAL", home.join(".gitconfig"))
            .env("NO_COLOR", "1")
            .env("ZYGO_NERD_FONT", "0")
            .env("COLUMNS", "200");
        if at.session {
            cmd.env("ZYGO_SESSION", "S1").env("ZYGO_SESSION_DIR", &self.dir);
        }
        if let Some(agent) = at.agent {
            cmd.env("ZYGO_AGENT", agent);
        }
        let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        child.stdin.take().unwrap().write_all(stdin).unwrap();
        let out = child.wait_with_output().unwrap();
        (out.status.code().unwrap(), String::from_utf8(out.stdout).unwrap(), String::from_utf8(out.stderr).unwrap())
    }

    /// `zygo <line>`: (exit code, stdout, stderr).
    fn cli(&self, line: &str, at: At) -> (i32, String, String) {
        self.run(&line.split_whitespace().collect::<Vec<_>>(), at, b"")
    }

    /// `/zygo:<line>` through `zygo hook prompt`: the reply. The hook blocks the prompt, so the model never sees it.
    fn slash(&self, line: &str, at: At) -> String {
        let stdin = json!({ "hook_event_name": "UserPromptSubmit", "prompt": format!("/zygo:{line}") }).to_string();
        let (code, out, err) = self.run(&["hook", "prompt"], at, stdin.as_bytes());
        assert_eq!(code, 0, "{line}: {err}");
        let v: Value = serde_json::from_str(&out).unwrap_or_else(|e| panic!("{line}: {e}: {out:?}"));
        assert_eq!(v["decision"], "block", "{line}: {out}");
        let reason = v["reason"].as_str().unwrap_or_else(|| panic!("{line}: {out}"));
        reason.strip_prefix("zygo: ").unwrap_or_else(|| panic!("{line}: {reason}")).to_string()
    }

    /// The number of rows of the next render of the HUD in the session: 1 is compact, 2 is verbose.
    fn rows(&self) -> usize {
        let (code, out, err) = self.run(&["hook", "statusline"], IN, b"{}");
        assert_eq!(code, 0, "{err}");
        assert!(out.contains("zygo"), "no HUD: {out:?} {err:?}");
        out.lines().count()
    }

    fn overrides(&self) -> Option<String> {
        std::fs::read_to_string(self.dir.join("overrides.json")).ok()
    }

    fn config(&self) -> String {
        std::fs::read_to_string(self.home.path().join("config.toml")).unwrap()
    }

    /// `statusline.style` of the user config file.
    fn saved_style(&self) -> Option<String> {
        let t: toml::Table = self.config().parse().unwrap();
        t.get("statusline")?.get("style")?.as_str().map(str::to_owned)
    }

    /// Every write is a temporary file and then a rename: no temporary file stays in the session or beside the config.
    fn assert_no_temp_files(&self) {
        for dir in [self.home.path(), self.dir.as_path()] {
            for e in std::fs::read_dir(dir).unwrap() {
                let name = e.unwrap().file_name().to_string_lossy().into_owned();
                assert!(!name.ends_with(".tmp"), "temporary file left in {}: {name}", dir.display());
            }
        }
    }
}

fn ok(r: (i32, String, String)) -> String {
    assert_eq!(r.0, 0, "stderr: {}", r.2);
    assert_eq!(r.2, "", "stderr of a run that exits 0");
    r.1
}

/// H§14 #33, sentence 1: the slash form in a session sets the override, and the next render has two rows.
#[test]
fn slash_verbose_in_a_session_sets_the_override_and_the_next_render_has_two_rows() {
    let f = fx("", Some("compact"));
    assert_eq!(f.rows(), 1);
    assert_eq!(f.slash("statusline verbose", IN), "statusline verbose for this session");
    let o: Value = serde_json::from_str(&f.overrides().unwrap()).unwrap();
    assert_eq!(o["statusline"], "verbose");
    assert!(f.overrides().unwrap().contains(r#""statusline":"verbose""#), "{:?}", f.overrides());
    assert_eq!(f.rows(), 2);
    assert_eq!(f.config(), "", "no --save, no user config");
    assert_eq!(ok(f.cli("statusline", IN)), "statusline verbose (session)\n");
    assert_eq!(f.slash("statusline", IN), "statusline verbose (session)");
    // The CLI form gives the same reply and the same effect.
    assert_eq!(ok(f.cli("statusline compact", IN)), "statusline compact for this session\n");
    assert!(f.overrides().unwrap().contains(r#""statusline":"compact""#));
    assert_eq!(f.rows(), 1);
    assert_eq!(ok(f.cli("statusline verbose", IN)), "statusline verbose for this session\n");
    assert_eq!(f.rows(), 2);
    f.assert_no_temp_files();
}

/// A session change keeps the other session settings of `overrides.json`.
#[test]
fn session_change_keeps_the_other_overrides() {
    let f = fx("", Some("compact"));
    std::fs::write(f.dir.join("overrides.json"), r#"{"work_units":false,"guards":{"claims":"enforce"}}"#).unwrap();
    assert_eq!(ok(f.cli("statusline verbose", IN)), "statusline verbose for this session\n");
    let o: Value = serde_json::from_str(&f.overrides().unwrap()).unwrap();
    assert_eq!(o["statusline"], "verbose");
    assert_eq!(o["work_units"], false);
    assert_eq!(o["guards"]["claims"], "enforce");
}

/// H§14 #33, sentence 2: `--save` in a session also writes `[statusline] style = "verbose"` to user config.
#[test]
fn save_in_a_session_also_writes_user_config() {
    let before = "# my config\n[features]\nresults = true # keep\n";
    for slash in [true, false] {
        let f = fx(before, Some("compact"));
        let want = "statusline verbose for this session; saved to user config";
        if slash {
            assert_eq!(f.slash("statusline verbose --save", IN), want);
        } else {
            assert_eq!(ok(f.cli("statusline verbose --save", IN)), format!("{want}\n"));
        }
        assert!(f.overrides().unwrap().contains(r#""statusline":"verbose""#));
        assert_eq!(f.rows(), 2);
        let after = f.config();
        assert!(after.starts_with(before), "the comments and the other keys stay: {after:?}");
        assert!(after.contains("[statusline]\nstyle = \"verbose\"\n"), "{after:?}");
        assert_eq!(f.saved_style().as_deref(), Some("verbose"));
        // A second save replaces the value in place.
        assert_eq!(ok(f.cli("statusline compact --save", IN)), "statusline compact for this session; saved to user config\n");
        assert_eq!(f.config(), after.replace("\"verbose\"", "\"compact\""));
        f.assert_no_temp_files();
    }
}

/// H§14 #33: `zygo statusline wide` → exit 2; nothing is written.
#[test]
fn unknown_style_exits_2_and_changes_nothing() {
    let f = fx("", Some("compact"));
    for line in ["statusline wide", "statusline verbose extra", "statusline Verbose", "statusline wide --save"] {
        for at in [IN, OUT] {
            assert_eq!(f.cli(line, at), (2, String::new(), USAGE.to_string()), "{line}");
        }
        assert_eq!(f.slash(line, IN), USAGE.trim_end(), "{line}");
    }
    assert_eq!(f.overrides(), None);
    assert_eq!(f.config(), "");
}

/// H§14 #33: outside a session without `--save` → exit 2 with the §17.1 text.
#[test]
fn outside_a_session_without_save_exits_2() {
    let f = fx("", Some("compact"));
    assert_eq!(f.cli("statusline verbose", OUT), (2, String::new(), NO_SESSION.to_string()));
    assert_eq!(f.slash("statusline verbose", OUT), NO_SESSION.trim_end());
    assert_eq!(f.config(), "");
    assert_eq!(f.overrides(), None);
}

/// H§14 #33 and AU1: `--save` outside a session changes user config; a running compact session stays compact.
#[test]
fn save_outside_a_session_changes_user_config_and_a_running_session_keeps_its_style() {
    let f = fx("", Some("compact"));
    assert_eq!(ok(f.cli("statusline verbose --save", OUT)), "statusline verbose saved to user config\n");
    assert_eq!(f.config(), "[statusline]\nstyle = \"verbose\"\n");
    assert_eq!(f.overrides(), None, "the session is not changed");
    assert_eq!(f.rows(), 1, "a running compact session stays compact");
    assert_eq!(ok(f.cli("statusline", IN)), "statusline compact (launch)\n");
    assert_eq!(ok(f.cli("statusline", OUT)), "statusline verbose (user config)\n");
    f.assert_no_temp_files();
}

/// H§14 #33 and AU1: a `statusline` value in `overrides.json` other than the two styles is ignored; the style comes
/// from `view.json`.
#[test]
fn bad_override_is_ignored_and_view_json_gives_the_style() {
    for (view, rows) in [("verbose", 2), ("compact", 1)] {
        let f = fx("", Some(view));
        std::fs::write(f.dir.join("overrides.json"), r#"{"statusline":"wide"}"#).unwrap();
        assert_eq!(f.rows(), rows, "{view}");
        assert_eq!(ok(f.cli("statusline", IN)), format!("statusline {view} (launch)\n"));
    }
}

/// H§8: with no argument the verb prints the current style and its source, and exits 0, in a session and outside one.
#[test]
fn no_argument_prints_the_style_and_its_source() {
    let f = fx("", None);
    assert_eq!(ok(f.cli("statusline", OUT)), "statusline compact (default)\n");
    assert_eq!(f.slash("statusline", OUT), "statusline compact (default)");
    // A session whose `view.json` is missing renders the default, as the hook does.
    assert_eq!(ok(f.cli("statusline", IN)), "statusline compact (default)\n");
    assert_eq!(f.rows(), 1);
    let f = fx("[statusline]\nstyle = \"verbose\"\n", Some("verbose"));
    assert_eq!(ok(f.cli("statusline", OUT)), "statusline verbose (user config)\n");
    assert_eq!(ok(f.cli("statusline", IN)), "statusline verbose (launch)\n");
    assert_eq!(f.rows(), 2);
    let f = fx("[statusline]\nstyle = \"compact\"\n", None);
    assert_eq!(ok(f.cli("statusline", OUT)), "statusline compact (user config)\n");
    // The read form changes nothing, with `--save` too.
    assert_eq!(ok(f.cli("statusline --save", IN)), "statusline compact (default)\n");
    assert_eq!(f.overrides(), None);
    assert_eq!(f.config(), "[statusline]\nstyle = \"compact\"\n");
}

/// H§14 #33: `zygo help` lists `statusline`; `help statusline` shows its scope and the current value.
#[test]
fn help_lists_statusline() {
    let f = fx("", Some("compact"));
    let help = ok(f.cli("help", OUT));
    assert!(
        help.lines().any(|l| l == "statusline [compact|verbose] — Status line style (now: compact)"),
        "{help}"
    );
    assert_eq!(
        ok(f.cli("help statusline", OUT)),
        "statusline [compact|verbose]\nStatus line style\nscope: session, --save\nnow: compact\n"
    );
    assert_eq!(ok(f.cli("statusline verbose", IN)), "statusline verbose for this session\n");
    for at in [IN, OUT] {
        let want = if at.session { "now: verbose" } else { "now: compact" };
        assert_eq!(ok(f.cli("help statusline", at)).lines().last(), Some(want));
    }
    assert!(f.slash("help", IN).lines().any(|l| l == "statusline [compact|verbose] — Status line style (now: verbose)"));
    let top = ok(f.cli("--help", OUT));
    assert!(top.lines().any(|l| l.split_whitespace().next() == Some("statusline")), "{top}");
    let own = ok(f.cli("statusline --help", OUT));
    assert!(own.contains("--save"), "{own}");
}

/// H§14 #33: the embedded plugin has `commands/statusline.md`, and the skill names the verb.
#[test]
fn embedded_plugin_has_the_command_and_the_skill_names_it() {
    let files = zygo_plugins::embedded::files();
    let text = |name: &str| {
        let (_, b) = files.iter().find(|(n, _)| n == name).unwrap_or_else(|| panic!("{name} missing"));
        String::from_utf8(b.clone()).unwrap()
    };
    let t = text("commands/statusline.md");
    assert!(t.contains("description: \"Status line style\""), "{t}");
    assert!(t.contains("argument-hint: \"[compact|verbose]\""), "{t}");
    assert!(t.contains("zygo statusline $ARGUMENTS"), "{t}");
    let skill = text("skills/zygo-cli/SKILL.md");
    assert!(skill.contains("`zygo statusline [compact|verbose]`"), "{skill}");
    assert!(skill.contains("--save"), "{skill}");
}

/// Operator decision: an OpenCode session has the same verb and the same behavior as a Claude Code session.
#[test]
fn opencode_session_behaves_as_a_claude_code_session() {
    let f = fx("", Some("compact"));
    assert_eq!(ok(f.cli("statusline", IN_OPENCODE)), "statusline compact (launch)\n");
    assert_eq!(ok(f.cli("statusline verbose", IN_OPENCODE)), "statusline verbose for this session\n");
    assert!(f.overrides().unwrap().contains(r#""statusline":"verbose""#));
    assert_eq!(ok(f.cli("statusline", IN_OPENCODE)), "statusline verbose (session)\n");
    assert_eq!(f.config(), "");
    assert_eq!(
        ok(f.cli("statusline compact --save", IN_OPENCODE)),
        "statusline compact for this session; saved to user config\n"
    );
    assert_eq!(f.saved_style().as_deref(), Some("compact"));
    assert_eq!(f.cli("statusline verbose", OUT_OPENCODE), (2, String::new(), NO_SESSION.to_string()));
    assert_eq!(ok(f.cli("statusline verbose --save", OUT_OPENCODE)), "statusline verbose saved to user config\n");
    assert_eq!(f.saved_style().as_deref(), Some("verbose"));
    assert_eq!(f.cli("statusline wide", IN_OPENCODE), (2, String::new(), USAGE.to_string()));
}

/// A user config that does not load stops a change before any write (exit 1), as for the other session verbs.
#[test]
fn broken_user_config_stops_a_change_before_any_write() {
    let f = fx("[statusline\n", Some("compact"));
    let (code, out, err) = f.cli("statusline verbose", IN);
    assert_eq!((code, out.as_str()), (1, ""), "{err}");
    assert!(err.starts_with("zygo statusline: "), "{err}");
    assert_eq!(f.overrides(), None);
    assert_eq!(f.config(), "[statusline\n");
    // The read form in a session needs no config.
    assert_eq!(ok(f.cli("statusline", IN)), "statusline compact (launch)\n");
}
