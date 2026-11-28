//! N42 acceptance (§17.1, §20 #17a–#17c): every verb in `VERBS` as `/zygo:<verb>` (through `zygo hook prompt`) and as `zygo <verb>`.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;

use serde_json::{json, Value};
use tempfile::TempDir;
use zygo::cmd::verbs::VERBS;
use zygo_config::{load, LoadContext};
use zygo_core::paths::Paths;
use zygo_core::session_dir::SessionDir;
use zygo_core::Agent;
use zygo_proxy::client::{SessionClient, WindowArg};
use zygo_proxy::server::StartOpts;
use zygo_proxy::session::SessionRequest;
use zygo_proxy::{Proxy, ProxyHandle};
use zygo_usage::Store;

mod common;

const NULL: &str = if cfg!(windows) { "NUL" } else { "/dev/null" };
const SECRET: &str = "refused: secret redaction cannot be turned off in a session";
const OP5: &str = "workspaces need tmux; run zygo in WSL";
/// W§9.3 step 1, OP11: /zygo:down names the marker's workspace and zygo down the cwd's; down has no SAME row.
const NOT_IN_SAME: &[&str] = &["down", "decision"];

/// The `Paths` that `Paths::from_env` gives for `HOME=LOCALAPPDATA=APPDATA=home`, `ZYGO_CONFIG=home/config.toml`.
fn paths(home: &Path) -> Paths {
    #[cfg(unix)]
    let (cache_dir, data_dir) = (home.join(".cache/zygo"), home.join(".local/share/zygo"));
    #[cfg(windows)]
    let (cache_dir, data_dir) = (home.join("zygo").join("cache"), home.join("zygo"));
    Paths {
        config_file: home.join("config.toml"),
        sessions_dir: cache_dir.join("sessions"),
        marketplaces_dir: cache_dir.join("marketplaces"),
        plugin_store: data_dir.join("plugins"),
        approvals_file: data_dir.join("approvals.toml"),
        usage_db: data_dir.join("usage.db"),
        cache_dir,
        data_dir,
    }
}

/// A live loopback proxy with one session and a usage database; returns the session id and its dir.
fn proxy(home: &Path) -> (ProxyHandle, String, PathBuf) {
    let mut m = load(&LoadContext { paths: paths(home), repo: None, team: None }).unwrap();
    m.config.catalog.0.iter_mut().find(|r| r.id == "claude-opus-5-5").unwrap().soft_context_window = Some(800_000);
    let h = Proxy::start_local_with(Arc::new(m), StartOpts { usage_db: Some(paths(home).usage_db), ..Default::default() }).unwrap();
    let g = h
        .create_session(SessionRequest {
            agent: Agent::ClaudeCode,
            agent_version: "2.1.328".into(),
            profile: None,
            passthrough: false,
            repo: None,
            work: Some("ENG-42".into()),
            stored_values: vec![],
            known_plugins: Default::default(),
        })
        .unwrap();
    let dir = paths(home).sessions_dir.join(&g.session);
    let sd = SessionDir::new(dir.clone());
    sd.create().unwrap();
    sd.write_token(&g.token).unwrap();
    std::fs::write(sd.url(), h.url().as_str()).unwrap();
    (h, g.session, dir)
}

/// A child command without the inherited `GIT_*` and `ZYGO_*` variables; the caller sets the ones it needs after this.
fn child(program: &str) -> Command {
    let mut cmd = Command::new(program);
    for (k, _) in std::env::vars_os() {
        if k.to_str().is_some_and(|k| k.starts_with("GIT_") || k.starts_with("ZYGO_")) {
            cmd.env_remove(k);
        }
    }
    cmd
}

/// Session environment of one child; `None` removes the name.
#[derive(Default, Clone)]
struct Env<'a> {
    session: Option<&'a str>,
    dir: Option<&'a Path>,
    agent: Option<&'a str>,
}

fn in_s<'a>(id: &'a str, dir: &'a Path) -> Env<'a> {
    Env { session: Some(id), dir: Some(dir), agent: None }
}

fn zygo(home: &Path, args: &[&str], env: &Env, stdin: &[u8]) -> (i32, String, String) {
    let mut cmd = child(env!("CARGO_BIN_EXE_zygo"));
    cmd.args(args)
        .current_dir(home)
        .env("HOME", home)
        .env("LOCALAPPDATA", home)
        .env("APPDATA", home)
        .env("ZYGO_CONFIG", home.join("config.toml"));
    for (k, v) in [("ZYGO_SESSION", env.session.map(Path::new)), ("ZYGO_SESSION_DIR", env.dir), ("ZYGO_AGENT", env.agent.map(Path::new))] {
        match v {
            Some(v) => cmd.env(k, v),
            None => cmd.env_remove(k),
        };
    }
    let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(stdin).unwrap();
    let out = child.wait_with_output().unwrap();
    (out.status.code().unwrap(), String::from_utf8(out.stdout).unwrap(), String::from_utf8(out.stderr).unwrap())
}

/// `prompt` through `zygo hook prompt`. §20 #17a "blocked and absent from the upstream request": stdout is exactly one JSON object,
/// `decision` block, `suppressOriginalPrompt` true, `reason` `zygo: <reply>`, no other field (so no copy of the line). Returns the reply.
fn slash_reply(home: &Path, env: &Env, prompt: &str) -> String {
    let stdin = json!({ "hook_event_name": "UserPromptSubmit", "prompt": prompt }).to_string();
    let (code, out, err) = zygo(home, &["hook", "prompt"], env, stdin.as_bytes());
    assert_eq!(code, 0, "{prompt}: {err}");
    let v: Value = serde_json::from_str(&out).unwrap_or_else(|e| panic!("{prompt}: {e}: {out:?}"));
    let reason = v["reason"].as_str().unwrap_or_else(|| panic!("{prompt}: {out}")).to_string();
    assert_eq!(
        v,
        json!({
            "decision": "block",
            "reason": reason,
            "hookSpecificOutput": { "hookEventName": "UserPromptSubmit", "suppressOriginalPrompt": true }
        }),
        "{prompt}"
    );
    reason.strip_prefix("zygo: ").unwrap_or_else(|| panic!("{prompt}: {reason}")).to_string()
}

/// The CLI reply of a run that must exit 0.
fn cli_reply(home: &Path, env: &Env, line: &str) -> String {
    let (code, out, err) = zygo(home, &line.split_whitespace().collect::<Vec<_>>(), env, b"");
    assert_eq!(code, 0, "{line}: {err}");
    out.strip_suffix('\n').unwrap_or(&out).to_string()
}

/// One form: `/zygo:<line>` or `zygo <line>`.
fn said(slash: bool, home: &Path, env: &Env, line: &str) -> String {
    if slash { slash_reply(home, env, &format!("/zygo:{line}")) } else { cli_reply(home, env, line) }
}

fn home() -> TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("config.toml"), "").unwrap();
    d
}

/// What a verb can change: user config, the session's `overrides.json`, the repository's work item, and the two proxy-side settings.
#[derive(Debug, PartialEq)]
struct Effect {
    config: String,
    overrides: Option<Vec<u8>>,
    work: Option<Vec<u8>>,
    window: Option<WindowArg>,
    content: Option<bool>,
}

/// One isolated place to run one form: a home that is a git repository, a user config, and, for `session`, a live proxy session.
struct Arm {
    /// Declared before `home`: the proxy drops before its temp dir.
    live: Option<(ProxyHandle, String, PathBuf)>,
    home: TempDir,
}

impl Arm {
    fn new(session: bool, user: &str, team: Option<&str>) -> Arm {
        let home = tempfile::tempdir().unwrap();
        std::fs::write(home.path().join("config.toml"), user).unwrap();
        let init = child("git")
            .env("GIT_CONFIG_GLOBAL", NULL)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .args(["init", "-q", "-b", "main"])
            .arg(home.path())
            .output()
            .unwrap();
        assert!(init.status.success(), "git init: {}", String::from_utf8_lossy(&init.stderr));
        let live = session.then(|| proxy(home.path()));
        if let (Some((_, _, dir)), Some(t)) = (&live, team) {
            std::fs::write(dir.join("team.toml"), t).unwrap();
        }
        Arm { home, live }
    }

    fn env(&self) -> Env<'_> {
        match &self.live {
            Some((_, id, dir)) => in_s(id, dir),
            None => Env::default(),
        }
    }

    fn slash(&self, line: &str) -> String {
        slash_reply(self.home.path(), &self.env(), &format!("/zygo:{line}"))
    }

    fn cli(&self, line: &str) -> (i32, String, String) {
        zygo(self.home.path(), &line.split_whitespace().collect::<Vec<_>>(), &self.env(), b"")
    }

    fn effect(&self) -> Effect {
        let h = self.home.path();
        let live = self.live.as_ref();
        Effect {
            config: std::fs::read_to_string(h.join("config.toml")).unwrap(),
            overrides: live.and_then(|(_, _, d)| std::fs::read(d.join("overrides.json")).ok()),
            work: std::fs::read(h.join(".git/zygo-work")).ok(),
            window: live.and_then(|(p, id, _)| p.session(id).unwrap().status().compact),
            content: live.and_then(|(p, id, _)| p.session(id).unwrap().settings().redaction_content),
        }
    }

    /// §20 #17a "absent from the upstream request": stops the proxy of a session arm, which writes its queued usage rows, and
    /// asserts that the session has none. Every model request gets a row, refusals included (`forward.rs`, `handle`).
    fn assert_no_upstream(&mut self, line: &str) {
        if let Some((p, id, _)) = self.live.take() {
            p.shutdown();
            let rows = Store::open(&paths(self.home.path()).usage_db).unwrap().requests_for_session(&id).unwrap();
            assert_eq!(rows.len(), 0, "{line}: the proxy saw a request");
        }
    }

    /// `line` in one form: slash gives no exit code; the CLI gives its code and stdout (exit 0) or stderr. Normalized, no final newline.
    fn reply(&self, slash: bool, line: &str) -> (Option<i32>, String) {
        if slash {
            return (None, self.norm(&self.slash(line)));
        }
        let (c, out, err) = self.cli(line);
        let s = if c == 0 { out } else { err };
        (Some(c), self.norm(s.strip_suffix('\n').unwrap_or_else(|| panic!("{line}: no final newline: {s:?}"))))
    }

    /// `reply` for a row that reads the current work item through the 2 s git bound (`zygo-work/src/repo.rs:13`). Under CPU load the
    /// bound can pass and the form then finds no work item: product behaviour, not a parity difference. Each attempt's work value is
    /// `want` or none, and one of 3 attempts gives `want`; the first such attempt is returned.
    fn reply_with_work(&self, slash: bool, verb: &str, line: &str, want: &str) -> (Option<i32>, String) {
        for _ in 0..3 {
            let r = self.reply(slash, line);
            let w = work(verb, &r.1);
            if w == want {
                return r;
            }
            assert!(NO_WORK.contains(&w), "{line} (slash {slash}): work {w:?} is neither {want:?} nor none");
        }
        panic!("{line} (slash {slash}): no attempt of 3 gave work {want:?}");
    }

    /// The session id and the home path differ between two arms; the text must not.
    fn norm(&self, s: &str) -> String {
        let s = s.replace(&self.home.path().display().to_string(), "<home>");
        match &self.live {
            Some((_, id, _)) => s.replace(id.as_str(), "<id>"),
            None => s,
        }
    }
}

/// §20 #17a, the table over every verb: (verb, arguments, inside a session, CLI exit code, the reply starts with, the form changes state).
/// Slash gives no exit code: it always blocks with the reply. Rows with a refusal give the CLI code of that refusal.
const SAME: &[(&str, &str, bool, i32, &str, bool)] = &[
    ("help", "", false, 0, "zygo verbs: /zygo:<verb> in Claude Code, zygo <verb> in a shell\nhelp [<verb>] — ", false),
    ("help", "compact", false, 0, "compact <tokens|on|off>\nSet the compaction window for the session\nscope: session", false),
    ("help", "frob", false, 2, "zygo help: unknown verb `frob`", false),
    ("status", "", false, 0, "mode: local\nprofile: none\nwork: branch:main\nrepo policy: defaults", false),
    ("usage", "", false, 0, "scope: all sessions\nrequests: 0", false),
    ("usage", "", true, 0, "scope: session <id>\nrequests: 0", false),
    ("compact", "600k", true, 0, "compact 600k for this session", true),
    ("compact", "off", true, 0, "compact off for this session", true),
    ("compact", "600x", true, 2, "compact failed: bad value `600x`", false),
    ("compact", "600k", false, 2, "compact failed: no session; pass --session <id> or run it inside a zygo session", false),
    ("results", "off", true, 0, "results off for this session", true),
    ("results", "threshold 32768", true, 0, "results threshold 32768 for this session", true),
    ("results", "off --save", true, 0, "results off for this session; saved to user config", true),
    ("results", "off", false, 2, "zygo results: not in a zygo session; add --save to change user config", false),
    ("results", "threshold 32768 --save", false, 0, "results threshold 32768 saved to user config", true),
    ("redaction", "", true, 0, "secrets on (status only), content on", false),
    ("redaction", "content off", true, 0, "redaction content off for this session", true),
    ("guards", "", true, 0, "claims monitor (builtin)\nscope monitor (builtin)", false),
    ("guards", "claims enforce", true, 0, "guards claims enforce for this session", true),
    ("units", "off", true, 0, "units off for this session", true),
    ("units", "off --save", false, 0, "units off saved to user config", true),
    ("units", "maybe", true, 2, "zygo units: usage: units on|off", false),
    ("work", "", false, 0, "branch:main", false),
    ("work", "42-fix", false, 0, "branch:42-fix", true),
    ("work", "clear", false, 0, "cleared", true),
    ("plugins", "", false, 0, "act101:act-lite (not synced)", false),
    ("plugins", "list", false, 0, "act101:act-lite (not synced)", false),
    ("plugins", "disable nope:x", false, 1, "marketplace nope is not configured", false),
    ("marketplace", "list", false, 0, "act101 github:act101-ai/act101 ", false),
    ("marketplace", "remove nope", false, 1, "marketplace nope is not configured", false),
    ("profile", "", false, 0, "default profile: none", false),
    ("profile", "list", false, 0, "no profiles", false),
    ("profile", "help", false, 0, "profile [<name>|none]\nDefault profile\nscope: next launch\nnow: none", false),
    ("profile", "show work", false, 1, "profile work: not found", false),
    ("profile", "use work", false, 0, "default profile: work (applies at next launch)\nzygo profile: work is not defined in your config (it may be a team preset)", true),
    ("profile", "none", false, 0, "default profile: none (applies at next launch)", true),
    ("profile", "work", false, 0, "default profile: work (applies at next launch)\nzygo profile: work is not defined in your config (it may be a team preset)", true),
    ("compact-tool", "off", false, 0, "compact-tool off (applies at next launch)", true),
    ("compact-tool", "on", false, 0, "compact-tool on (applies at next launch)", true),
    ("statusline", "", false, 0, "statusline compact (default)", false),
    ("statusline", "", true, 0, "statusline compact (default)", false),
    ("statusline", "verbose", true, 0, "statusline verbose for this session", true),
    ("statusline", "verbose --save", true, 0, "statusline verbose for this session; saved to user config", true),
    ("statusline", "verbose", false, 2, "zygo statusline: not in a zygo session; add --save to change user config", false),
    ("statusline", "verbose --save", false, 0, "statusline verbose saved to user config", true),
    ("statusline", "wide", true, 2, "zygo statusline: usage: statusline [compact|verbose]", false),
    ("ls", "", false, if cfg!(windows) { 2 } else { 0 }, if cfg!(windows) { OP5 } else { "no workspaces" }, false),
    ("workspace", "", false, if cfg!(windows) { 2 } else { 1 }, if cfg!(windows) { OP5 } else { "no workspace for this session or folder" }, false),
    ("mail", "", false, if cfg!(windows) { 2 } else { 1 }, if cfg!(windows) { OP5 } else { "zygo: no workspace for this session or folder" }, false),
];

/// The work value of a reply: the `work:` line of `status`, else the whole reply (a `work` row).
fn work<'a>(verb: &str, reply: &'a str) -> &'a str {
    match verb {
        "status" => reply.lines().find_map(|l| l.strip_prefix("work: ")).unwrap_or_else(|| panic!("no work line: {reply}")),
        _ => reply,
    }
}

/// What a form gives when the git bound passes: `status` (`cmd/status.rs:80`), `work` (`cmd/work.rs:53`), `work <id>|clear`
/// (`current.rs` `checkout`, `UsageError::NotInRepository`).
const NO_WORK: &[&str] = &["none", "no current work item", "zygo work: not in a git repository"];

/// The reply without the `work:` line of `status`; other replies as they are (a `work` reply is checked by `reply_with_work`).
fn without_work(verb: &str, reply: &str) -> String {
    match verb {
        "status" => reply.lines().filter(|l| !l.starts_with("work: ")).collect::<Vec<_>>().join("\n"),
        _ => reply.to_string(),
    }
}

#[test]
fn every_verb_has_a_row_and_a_cli_command() {
    let rows: BTreeSet<&str> = SAME.iter().map(|r| r.0).chain(NOT_IN_SAME.iter().copied()).collect();
    let verbs: BTreeSet<&str> = VERBS.iter().map(|v| v.spec.name).collect();
    assert_eq!(rows, verbs, "SAME and VERBS name different verbs");
    assert!(SAME.iter().all(|r| !NOT_IN_SAME.contains(&r.0)), "a verb of NOT_IN_SAME has a SAME row");
    for name in &verbs {
        let h = tempfile::tempdir().unwrap();
        let mut c = child(env!("CARGO_BIN_EXE_zygo"));
        common::scratch_home(&mut c, h.path());
        let o = c.args([*name, "--help"]).stdin(Stdio::null()).output().unwrap();
        assert!(o.status.success(), "zygo {name} --help exited {:?}", o.status.code());
    }
}

#[test]
#[ignore = "timing: the 4.5 s prompt hook deadline in the zygo binary"]
fn slash_and_cli_same_reply_and_effect_for_every_verb() {
    for &(verb, args, session, code, starts, changes) in SAME {
        let line = format!("{verb} {args}");
        let line = line.trim_end();
        let (mut a, b) = (Arm::new(session, "", None), Arm::new(session, "", None));
        if line == "work clear" {
            // A `clear` that does nothing must show: give both arms a work item first.
            for arm in [&b, &a] {
                assert_eq!(arm.reply_with_work(false, "work", "work 42-fix", "branch:42-fix").0, Some(0));
            }
        }
        let before = a.effect();
        assert_eq!(before, b.effect(), "{line}: arms start alike");
        let want = work(verb, starts);
        let form = |arm: &Arm, slash: bool| match verb {
            "status" | "work" => arm.reply_with_work(slash, verb, line, want),
            _ => arm.reply(slash, line),
        };
        let (_, slash) = form(&a, true);
        let (c, cli) = form(&b, false);
        assert_eq!(c, Some(code), "{line}: exit code; stderr {cli:?}");
        let (slash, cli) = (without_work(verb, &slash), without_work(verb, &cli));
        assert_eq!(slash, cli, "{line}: reply");
        assert!(slash.starts_with(&without_work(verb, starts)), "{line}: {slash:?}");
        assert_eq!(a.effect(), b.effect(), "{line}: effect");
        assert_eq!(a.effect() != before, changes, "{line}: state change");
        if line == "profile help" {
            // Spec §11 note (N42 review): `help` is not a profile name; the reply is `help profile`'s and the config stays empty.
            assert_eq!(slash, a.reply(true, "help profile").1, "{line}");
            assert_eq!((a.effect().config, b.effect().config), (String::new(), String::new()), "{line}: config");
        }
        a.assert_no_upstream(line);
    }
}

/// §17.1 N28 note (C7), the one exception to "same reply": inside the agent shell the CLI `status` and `help` print no window number.
#[test]
#[ignore = "timing: the 4.5 s prompt hook deadline in the zygo binary"]
fn status_and_help_differ_only_in_the_window_inside_the_agent_shell() {
    let a = Arm::new(true, "", None);
    let (_, _, dir) = a.live.as_ref().unwrap();
    SessionClient::from_session_dir(dir).unwrap().set_compact(WindowArg::Tokens(600_000)).unwrap();
    let (slash, (c, cli, _)) = (a.slash("status"), a.cli("status"));
    assert_eq!(c, 0);
    assert!(slash.lines().any(|l| l == "compact: 600k") && slash.lines().any(|l| l == "context: -/600k"), "{slash}");
    assert!(cli.lines().any(|l| l == "compact: override set") && cli.lines().any(|l| l == "context: -"), "{cli}");
    let rest = |s: &str| s.lines().filter(|l| !l.starts_with("context: ") && !l.starts_with("compact: ")).collect::<Vec<_>>().join("\n");
    assert_eq!(rest(&slash), rest(cli.trim_end()));
    let (slash, (c, cli, _)) = (a.slash("help"), a.cli("help"));
    assert_eq!(c, 0);
    assert!(slash.contains("(now: 600k)"), "{slash}");
    assert_eq!(slash.replace("(now: 600k)", "(now: override set)"), cli.trim_end());
    assert!(!cli.contains("600k"), "{cli}");
}

/// §20 #17b, first two sentences: (change, the verb whose `help` shows it, the `now:` line after the change).
const KEEP: &[(&str, &str, &str)] = &[
    ("results off", "results", "now: off, threshold 16384"),
    ("results threshold 32768", "results", "now: on, threshold 32768"),
    ("units off", "units", "now: off"),
    ("guards claims enforce", "guards", "now: claims enforce, scope monitor, navigation monitor, output monitor, trailers notify, repeat monitor, dispositions enforce"),
    ("redaction content off", "redaction", "now: secrets on, content off"),
];

fn now(home: &Path, env: &Env, verb: &str) -> String {
    let out = cli_reply(home, env, &format!("help {verb}"));
    out.lines().find(|l| l.starts_with("now: ")).unwrap_or_else(|| panic!("{out}")).to_string()
}

#[test]
#[ignore = "timing: the 4.5 s prompt hook deadline in the zygo binary"]
fn session_change_is_gone_in_the_next_session_and_save_keeps_it() {
    for slash in [true, false] {
        for &(change, verb, after) in KEEP {
            let d = home();
            let home = d.path();
            let (_h1, id1, s1) = proxy(home);
            let e1 = in_s(&id1, &s1);
            let before = now(home, &e1, verb);
            assert_ne!(before, after, "{change}: the change must show");
            assert_eq!(said(slash, home, &e1, change), format!("{change} for this session"));
            assert_eq!(now(home, &e1, verb), after, "{change}: in the session (slash {slash})");
            let (_h2, id2, s2) = proxy(home);
            assert_eq!(now(home, &in_s(&id2, &s2), verb), before, "{change}: next session (slash {slash})");
            assert_eq!(std::fs::read_to_string(home.join("config.toml")).unwrap(), "", "{change}: no --save, no user config");
            assert_eq!(
                said(slash, home, &e1, &format!("{change} --save")),
                format!("{change} for this session; saved to user config")
            );
            let (_h3, id3, s3) = proxy(home);
            assert_eq!(now(home, &in_s(&id3, &s3), verb), after, "{change}: saved (slash {slash})");
        }
        // `compact` is session only.
        let d = home();
        let home = d.path();
        let (_h1, id1, s1) = proxy(home);
        let e1 = in_s(&id1, &s1);
        assert_eq!(said(slash, home, &e1, "compact 600k"), "compact 600k for this session");
        assert!(slash_reply(home, &e1, "/zygo:status").lines().any(|l| l == "compact: 600k"));
        let (_h2, id2, s2) = proxy(home);
        assert!(slash_reply(home, &in_s(&id2, &s2), "/zygo:status").lines().any(|l| l == "compact: none"));
    }
}

/// §20 #17b, last sentence: (user config, team.toml of the session, inside a session, line, reply).
fn refused() -> Vec<(&'static str, Option<String>, bool, &'static str, &'static str)> {
    let required = format!(
        "[[marketplace]]\nname = \"team\"\nsource = \"github:example/agent-plugins\"\nref = \"v1.4.0\"\ncommit = \"{}\"\nrequired = [\"lint\"]\n",
        "4f2c".repeat(10)
    );
    vec![
        ("[guards]\nmin_level = \"notify\"\n", None, true, "guards claims monitor", "refused: guard claims cannot go below min_level notify"),
        ("", Some("[features]\nwork_units = true\n".into()), true, "units off", "refused: `features.work_units` is set by the team config"),
        ("", Some("[features]\nresults = true\n".into()), true, "results off --save", "refused: `features.results` is set by the team config"),
        ("", Some(required), true, "plugins disable team:lint", "refused: team:lint is required by the team config"),
        ("", None, true, "redaction off", SECRET),
        ("", None, true, "redaction secrets off", SECRET),
        ("", None, false, "redaction off", SECRET),
        ("", None, false, "redaction secrets off", SECRET),
    ]
}

#[test]
#[ignore = "timing: the 4.5 s prompt hook deadline in the zygo binary"]
fn limits_are_refused_with_the_limit_named_by_both_forms() {
    for (user, team, session, line, reply) in refused() {
        let (a, b) = (Arm::new(session, user, team.as_deref()), Arm::new(session, user, team.as_deref()));
        let before = a.effect();
        assert_eq!(a.slash(line), reply, "{line}");
        let (code, out, err) = b.cli(line);
        assert_eq!((code, out.as_str(), err.as_str()), (1, "", format!("{reply}\n").as_str()), "{line}");
        assert_eq!(a.effect(), before, "{line}: slash changed state");
        assert_eq!(b.effect(), before, "{line}: CLI changed state");
    }
}

/// §20 #17c: `help` lists the verbs that have a `commands/<verb>.md`, each verb works, an unknown verb blocks with the `help` text.
#[test]
#[ignore = "timing: the 4.5 s prompt hook deadline in the zygo binary"]
fn help_lists_every_embedded_command_and_each_works() {
    let embedded: BTreeSet<String> = zygo_plugins::embedded::files()
        .into_iter()
        .filter_map(|(p, _)| p.strip_prefix("commands/").and_then(|p| p.strip_suffix(".md")).map(str::to_owned))
        .collect();
    let (a, b) = (Arm::new(true, "", None), Arm::new(true, "", None));
    let help = a.slash("help");
    let listed: BTreeSet<String> = help.lines().skip(1).filter_map(|l| l.split(' ').next()).map(str::to_owned).collect();
    assert_eq!(listed, embedded, "help lists the verbs that have a commands/<verb>.md");
    assert_eq!(help.lines().count(), 1 + embedded.len(), "one help line per verb");
    let verbs: BTreeSet<String> = VERBS.iter().map(|v| v.spec.name.to_owned()).collect();
    assert_eq!(embedded, verbs);
    for line in ["frob", "frob x y", "nonesuch"] {
        assert_eq!(a.slash(line), help, "{line}");
    }
    for verb in verbs.iter().filter(|v| !NOT_IN_SAME.contains(&v.as_str())) {
        let row = SAME.iter().find(|r| r.0 == verb && r.3 == 0).or_else(|| SAME.iter().find(|r| r.0 == verb)).unwrap();
        let args = row.1;
        let line = format!("{verb} {args}");
        let line = line.trim_end();
        let slash = a.slash(line);
        let (code, _, err) = b.cli(line);
        assert_eq!(code, row.3, "{line}: {err}");
        // `help` is the one verb whose own reply is the help text; any other verb that answers with it was not dispatched.
        assert_eq!(slash.starts_with("zygo verbs: "), verb == "help", "{line}: {slash:?}");
    }
}

/// OP11, W§12: `down`, `ls`, `workspace` are slash verbs; `up`, `attach`, `loadout`, `ws` are not, so their lines
/// block with the `help` text (base §20 #17c). `/zygo:down` with no marker replies `this session is not in a
/// workspace` (W§9.3 step 1) and changes nothing. `/zygo:help down` gives the text of `zygo help down`.
#[test]
#[ignore = "timing: the 4.5 s prompt hook deadline in the zygo binary"]
fn workspace_slash_verbs_are_down_ls_and_workspace() {
    let names: BTreeSet<&str> = VERBS.iter().map(|v| v.spec.name).collect();
    for n in ["down", "ls", "workspace"] {
        assert!(names.contains(n), "{n} is not a slash verb");
    }
    for n in ["up", "attach", "loadout", "ws"] {
        assert!(!names.contains(n), "{n} is a slash verb");
    }
    let a = Arm::new(true, "", None);
    let before = a.effect();
    let none = if cfg!(windows) { OP5 } else { "this session is not in a workspace" };
    assert_eq!(a.slash("down"), none);
    assert_eq!(a.slash("down --now"), none);
    assert_eq!(a.slash("down --bogus"), if cfg!(windows) { OP5 } else { "usage: /zygo:down [--now]" });
    let (code, cli_help, _) = a.cli("help down");
    assert_eq!(code, 0);
    assert_eq!(a.slash("help down"), cli_help.trim_end());
    assert!(cli_help.starts_with("down [--now] [--timeout <dur>] [--wait] [--all] [--] [<name>]\n"), "{cli_help}");
    let help = a.slash("help");
    assert!(help.lines().any(|l| l.starts_with("down [--now] [--timeout <dur>] [--wait] [--all] [--] [<name>] — zygo down: ")), "{help}");
    for line in ["up", "up --no-attach", "attach", "loadout list", "ws forget x"] {
        assert_eq!(a.slash(line), help, "{line}");
    }
    assert_eq!(a.effect(), before);
}

/// §17.1 N30 and N50 notes: `plugins sync` is not a slash verb, and `marketplace add|update` fetch, so slash replies with the shell command
/// and changes nothing (no file, no directory).
#[test]
#[ignore = "timing: the 4.5 s prompt hook deadline in the zygo binary"]
fn shell_only_slash_forms_reply_with_the_command_and_change_nothing() {
    let tree = |dir: &Path| -> Vec<(PathBuf, Vec<u8>)> {
        let mut v: Vec<_> = walkdir::WalkDir::new(dir)
            .sort_by_file_name()
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_file())
            .map(|e| (e.path().strip_prefix(dir).unwrap().to_path_buf(), std::fs::read(e.path()).unwrap()))
            .collect();
        v.sort();
        v
    };
    let a = Arm::new(false, "", None);
    let before = tree(a.home.path());
    for line in ["plugins sync", "marketplace add /x", "marketplace add /x --name n --ref v1.0.0", "marketplace update", "marketplace update --all --accept", "decision ls"] {
        assert_eq!(a.slash(line), format!("run in your shell: zygo {line}"), "{line}");
        assert_eq!(tree(a.home.path()), before, "{line}: wrote a file");
    }
    assert!(!a.home.path().join(".cache").exists() && !a.home.path().join(".local").exists());
}
