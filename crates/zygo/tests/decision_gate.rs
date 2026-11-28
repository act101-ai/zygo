//! DR-14: end-to-end gate (D§8.5 steps 1-5, D§17 #7, #8, #16, D-C1, D§3).
//!
//! One scenario code, two drivers. `InProc` (CI) runs `work_verdict` with real git under deadlines an hour away and
//! `zygo decision` through `run_in` with the repository facts as values: no deadline, no clock. `Bin` (ignored for
//! timing: the binary's git calls run under the 2 s production bound) runs the `zygo` binary: `work_verdict` over
//! `zygo mcp --agent claude-code` stdio, with the work hooks Claude Code runs around it. No agent process runs.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tempfile::TempDir;
use zygo::cmd::decision::{DecisionIo, Repo, run_in};
use zygo_config::Level;
use zygo_core::Agent;
use zygo_core::session_dir::SessionDir;
use zygo_core::session_view::{LegendEntry, SessionView};
use zygo_mcp::tools::work::WorkVerdict;
use zygo_mcp::{Tool, ToolError};
use zygo_usage::{Filter, Store, WorkUnitRow};
use zygo_work::decisions::record::{Class, FILL_MARKER, Record, Status, parse_record};
use zygo_work::gate::git::{BatchIo, GitRunner, ProcessGit, ProcessRunner};
use zygo_work::repo::{RunError, Whole};

mod common;

/// The work unit of every flow.
const UNIT: &str = "fixture:N1";
/// The session id of the session view and the folded rows.
const SESSION: &str = "S1";
/// The label of the `security` finding that stops for the operator.
const LABEL_STOP: &str = "LABEL-7c41";
/// The label of the rejected finding.
const LABEL_REJ: &str = "LABEL-2b9e";
/// The reject reason.
const REASON: &str = "REASON-5d03";
/// The `findings` string of every keep.
const FINDINGS: &str = "FINDINGS-8a6f";
/// The record title.
const TITLE: &str = "TITLE-3e17";
/// The record body: every fill line.
const BODY: &str = "BODY-9f52";
/// The overturn reason (`review_note`).
const NOTE: &str = "NOTE-41ac";
/// The text no zygo store, log or session file may hold (#16, D-C1, D§3).
const MARKERS: [&str; 7] = [LABEL_STOP, LABEL_REJ, REASON, FINDINGS, TITLE, BODY, NOTE];
/// Today for the in-process `zygo decision`.
const TODAY: &str = "2026-11-25";
/// The developer variables no child process of this test inherits: zygo session and repository selection, tmux,
/// and user git config or repository selection. A variable the test sets on purpose is set after the removal.
const STRIP_ENV: [&str; 13] = [
    "ZYGO_SESSION",
    "ZYGO_SESSION_DIR",
    "ZYGO_URL",
    "ZYGO_REPO",
    "ZYGO_WORK",
    "ZYGO_MAIN_ROLE",
    "ZYGO_WORKSPACE",
    "TMUX",
    "XDG_CONFIG_HOME",
    "GIT_CONFIG_GLOBAL",
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
];

/// `cmd` without the [`STRIP_ENV`] variables.
fn clean_env(cmd: &mut Command) -> &mut Command {
    for k in STRIP_ENV {
        cmd.env_remove(k);
    }
    cmd
}

/// `p` canonical (macOS `/var` is `/private/var`), without the Windows verbatim prefix `\\?\`: the paths go to git
/// (`git -C`, the cwd, `GIT_CEILING_DIRECTORIES`), and git for Windows and its own path output use plain paths.
fn canon(p: &Path) -> PathBuf {
    let c = p.canonicalize().unwrap();
    match c.to_str().and_then(|s| s.strip_prefix(r"\\?\")) {
        Some(s) if cfg!(windows) && !s.starts_with(r"UNC\") => PathBuf::from(s),
        _ => c,
    }
}

/// `Node`: a lead checkout and a linked node checkout, keeps without `base`. `One`: one checkout, keeps with `base`.
#[derive(Clone, Copy)]
enum Layout {
    Node,
    One,
}

/// One fixture: the repositories, the scratch home, the session dir.
struct Fx {
    _t: TempDir,
    /// The temp root: the repositories, `home`, `session` and `git-home` are in it.
    root: PathBuf,
    /// The scratch home of every zygo process (usage store, state, cache, logs, `config.toml`).
    home: PathBuf,
    sd: SessionDir,
    /// The session `repo_root`.
    lead: PathBuf,
    /// The checkout the agent works in.
    work: PathBuf,
    /// `HEAD` after the `base` commit in `One`; `None` in `Node`.
    base: Option<String>,
}

impl Fx {
    /// The layout (call 1) and the session (call 2).
    fn new(layout: Layout) -> Fx {
        let t = tempfile::tempdir().unwrap();
        let root = canon(t.path());
        let home = root.join("home");
        fs::create_dir_all(&home).unwrap();
        fs::write(home.join("config.toml"), "").unwrap();
        let (lead, work, base) = match layout {
            Layout::Node => {
                let lead = root.join("main");
                init(&lead);
                git(
                    &lead,
                    &["worktree", "add", "-q", "../node", "-b", "node/N1"],
                );
                (lead, root.join("node"), None)
            }
            Layout::One => {
                let one = root.join("one");
                let h = init(&one);
                (one.clone(), one, Some(h))
            }
        };
        let sd = SessionDir::new(root.join("session"));
        sd.create().unwrap();
        let legend = vec![LegendEntry {
            role: "code".into(),
            description: String::new(),
            custom: false,
        }];
        SessionView {
            session: SESSION.into(),
            agent: Agent::ClaudeCode,
            main_role: "lead".into(),
            legend,
            work_units: true,
            repo_root: Some(lead.to_str().unwrap().to_string()),
            ..Default::default()
        }
        .save(&sd)
        .unwrap();
        Fx {
            _t: t,
            root,
            home,
            sd,
            lead,
            work,
            base,
        }
    }

    /// The usage store under the scratch home (call 3).
    fn db(&self) -> PathBuf {
        if cfg!(windows) {
            self.home.join("zygo").join("usage.db")
        } else {
            self.home.join(".local/share/zygo/usage.db")
        }
    }

    /// The folded row of UNIT `round` (call 3): a precondition of a keep, not under test.
    fn row(&self, round: u32) {
        let db = self.db();
        fs::create_dir_all(db.parent().unwrap()).unwrap();
        let row = WorkUnitRow {
            unit: UNIT.into(),
            round,
            session: SESSION.into(),
            agent_id: "a1".into(),
            began_at: 100,
            ..Default::default()
        };
        Store::open(&db).unwrap().insert_work_unit(&row).unwrap();
    }

    /// The verdict of the UNIT row of `round`.
    fn verdict(&self, round: u32) -> Option<String> {
        let rows = Store::open(&self.db())
            .unwrap()
            .work_units(&Filter::default())
            .unwrap();
        let row = rows
            .into_iter()
            .find(|r| r.unit == UNIT && r.round == round);
        row.unwrap_or_else(|| panic!("no row for round {round}"))
            .verdict
    }
}

/// `git init -b main` in `dir` and one commit `base` that adds `a`; returns its id.
fn init(dir: &Path) -> String {
    fs::create_dir_all(dir).unwrap();
    git(dir, &["init", "-q", "-b", "main"]);
    fs::write(dir.join("a"), "a\n").unwrap();
    commit(dir, "base")
}

/// Git in `dir` with no user or system config: `GIT_CONFIG_GLOBAL` an empty file in `<t>/git-home`, `HOME` that
/// directory, and the identity by env (as `decision_new.rs`). Asserts success; the trimmed stdout.
fn git(dir: &Path, args: &[&str]) -> String {
    let home = dir.parent().unwrap().join("git-home");
    fs::create_dir_all(&home).unwrap();
    let global = home.join("gitconfig");
    if !global.exists() {
        fs::write(&global, "").unwrap();
    }
    let o = clean_env(&mut Command::new("git"))
        .args(["-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", &global)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", &home)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8(o.stdout).unwrap().trim().to_string()
}

/// `git add -A` and `git commit -q -m msg` in `dir`; the new `HEAD`.
fn commit(dir: &Path, msg: &str) -> String {
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", msg]);
    git(dir, &["rev-parse", "HEAD"])
}

/// An agent commit message: the subject, the session and the record trailers.
fn agent_msg(subject: &str, id: &str) -> String {
    format!("{subject}\n\nZygo-Session: {SESSION}\nZygo-Decision: {id}")
}

/// Each FILL_MARKER line replaced: the reverse_cost line by `reverse_cost = "low"`, every other by BODY.
fn fill(path: &Path) {
    let t = fs::read_to_string(path).unwrap();
    let lines: Vec<&str> = t
        .split('\n')
        .map(|l| match l {
            l if l.contains(FILL_MARKER) && l.contains("reverse_cost") => "reverse_cost = \"low\"",
            l if l.contains(FILL_MARKER) => BODY,
            l => l,
        })
        .collect();
    fs::write(path, lines.join("\n")).unwrap();
}

/// `<work>/docs/decisions/<id>.md`.
fn rec_path(fx: &Fx, id: &str) -> PathBuf {
    fx.work.join("docs/decisions").join(format!("{id}.md"))
}

/// The record `id` in the work checkout, parsed.
fn record(fx: &Fx, id: &str) -> Record {
    parse_record(id, &fs::read(rec_path(fx, id)).unwrap()).unwrap()
}

/// The recorded `security` entry for LABEL_STOP.
fn stop_entry(id: &str) -> Value {
    json!({"finding": LABEL_STOP, "class": "security", "disposition": "recorded", "record": id})
}

/// The rejected entry for LABEL_REJ.
fn rejected_entry() -> Value {
    json!({"finding": LABEL_REJ, "class": "none", "disposition": "rejected", "reason": REASON})
}

/// A keep of UNIT `round` judged at `head`, with FINDINGS, `entries`, and `base` when the fixture has one.
fn keep_args(fx: &Fx, round: u32, head: &str, entries: Vec<Value>) -> Value {
    let mut a = json!({"unit": UNIT, "round": round, "verdict": "keep", "findings": FINDINGS, "head": head, "dispositions": entries});
    if let Some(b) = &fx.base {
        a["base"] = json!(b);
    }
    a
}

/// No marker in any file under the scratch home or the session dir (call 6). The repositories are outside both:
/// record text there is repository content (D-C1).
fn assert_no_text(fx: &Fx) {
    for top in [&fx.home, &fx.sd.root] {
        for e in walkdir::WalkDir::new(top) {
            let e = e.unwrap();
            if !e.file_type().is_file() {
                continue;
            }
            let bytes = fs::read(e.path()).unwrap();
            for m in MARKERS {
                let found = bytes.windows(m.len()).any(|w| w == m.as_bytes());
                assert!(!found, "{m} is in {}", e.path().display());
            }
        }
    }
}

// ---- the far-deadline runner, copied from crates/zygo-work/tests/decision_verdict.rs (private to that test crate) ----

/// The real runner with its clock one hour ahead, so every deadline is an hour away and machine load cannot fail a test.
struct Far(ProcessRunner);

impl GitRunner for Far {
    fn run(&mut self, cmd: Command, deadline: Instant) -> Result<Whole, RunError> {
        self.0.run(cmd, deadline)
    }
    fn batch(&mut self, cmd: Command, deadline: Instant) -> Result<Box<dyn BatchIo>, RunError> {
        self.0.batch(cmd, deadline)
    }
    fn now(&self) -> Instant {
        Instant::now() + Duration::from_secs(3600)
    }
}

/// [`Far`] with `HOME`, `XDG_CONFIG_HOME` (and `USERPROFILE`) set to an empty temp dir on each command, so no
/// developer `~/.gitconfig` reaches a real-git test.
struct Iso {
    far: Far,
    home: TempDir,
}

impl Iso {
    fn isolate(&self, mut cmd: Command) -> Command {
        clean_env(&mut cmd)
            .env("HOME", self.home.path())
            .env("XDG_CONFIG_HOME", self.home.path());
        if cfg!(windows) {
            cmd.env("USERPROFILE", self.home.path());
        }
        cmd
    }
}

impl GitRunner for Iso {
    fn run(&mut self, cmd: Command, deadline: Instant) -> Result<Whole, RunError> {
        let cmd = self.isolate(cmd);
        self.far.run(cmd, deadline)
    }
    fn batch(&mut self, cmd: Command, deadline: Instant) -> Result<Box<dyn BatchIo>, RunError> {
        let cmd = self.isolate(cmd);
        self.far.batch(cmd, deadline)
    }
    fn now(&self) -> Instant {
        self.far.now()
    }
}

fn far_git(root: &Path) -> ProcessGit {
    ProcessGit::with_runner(
        root,
        Box::new(Iso {
            far: Far(ProcessRunner),
            home: TempDir::new().unwrap(),
        }),
    )
}

// ---- the drivers ----

trait Driver {
    /// One `work_verdict` call: Ok(reply text), or Err(text) of a tool error (`isError`). Panics on an argument refusal.
    fn keep(&self, fx: &Fx, args: Value) -> Result<String, String>;
    /// `zygo decision <args>` in `cwd`; `agent`: with ZYGO_SESSION_DIR = fx.sd, else without it. (exit code, stdout, stderr)
    fn decision(&self, fx: &Fx, cwd: &Path, agent: bool, args: &[&str]) -> (i32, String, String);
}

/// `WorkVerdict` and `run_in` in this process.
struct InProc;

impl Driver for InProc {
    fn keep(&self, fx: &Fx, args: Value) -> Result<String, String> {
        let tool = WorkVerdict::new(
            fx.db(),
            SESSION.into(),
            fx.sd.clone(),
            Box::new(|_| (Level::Enforce, None)),
            Box::new(|root| Box::new(far_git(root))),
        );
        match tool.call(args) {
            Ok(Value::String(s)) => Ok(s),
            Err(ToolError::Failed(s)) => Err(s),
            other => panic!("work_verdict: {other:?}"),
        }
    }

    fn decision(&self, fx: &Fx, cwd: &Path, agent: bool, args: &[&str]) -> (i32, String, String) {
        let checkout = || {
            let top = git(cwd, &["rev-parse", "--show-toplevel"]);
            let common = git(
                cwd,
                &["rev-parse", "--path-format=absolute", "--git-common-dir"],
            );
            Some((PathBuf::from(top), PathBuf::from(common)))
        };
        let commits = |_: &Path, _: &str, _: usize| Ok(vec![]);
        let head = |p: &Path| Some(git(p, &["rev-parse", "HEAD"]));
        let today = || TODAY.to_string();
        let db = fx.db();
        let sd: OsString = fx.sd.root.clone().into();
        let env = |k: &str| (agent && k == "ZYGO_SESSION_DIR").then(|| sd.clone());
        let repo = Repo {
            checkout: &checkout,
            commits: &commits,
            head: &head,
            today: &today,
            usage_db: &db,
        };
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let mut input = std::io::empty();
        let mut io = DecisionIo {
            input: &mut input,
            out: &mut out,
            err: &mut err,
            tty: false,
        };
        let code = run_in(&repo, &env, &args, &mut io);
        (
            code,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }
}

/// The `zygo` binary: `work_verdict` over `zygo mcp --agent claude-code` stdio between the work hooks, and `zygo
/// decision` as a process.
struct Bin;

/// The Claude Code hook fixture `name` (as `work_hook.rs`).
fn fixture(name: &str) -> Value {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/claude-code/hooks")
        .join(name);
    serde_json::from_slice(&fs::read(p).unwrap()).unwrap()
}

/// A `zygo` command with the scratch home, the session dir, no `ZYGO_AGENT`, git stopped at the temp root, cwd lead.
fn agent_cmd(fx: &Fx) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_zygo"));
    common::scratch_home(clean_env(&mut cmd), &fx.home);
    cmd.env("ZYGO_SESSION_DIR", &fx.sd.root)
        .env_remove("ZYGO_AGENT")
        .env("GIT_CEILING_DIRECTORIES", &fx.root)
        .current_dir(&fx.lead);
    cmd
}

/// `zygo hook <args>` with `payload` on stdin. (exit code, stdout, stderr)
fn hook(fx: &Fx, args: &[&str], payload: &Value) -> (Option<i32>, String, String) {
    use std::process::Stdio;
    let mut cmd = agent_cmd(fx);
    cmd.arg("hook").args(args);
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    common::write_stdin(child.stdin.take().unwrap(), payload.to_string().as_bytes());
    let out = child.wait_with_output().unwrap();
    (
        out.status.code(),
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
    )
}

/// The hook fixture `name` as a `work_verdict` call from the lead.
fn verdict_payload(fx: &Fx, name: &str, args: &Value) -> Value {
    let mut p = fixture(name);
    p["tool_name"] = json!("mcp__zygo__work_verdict");
    p["tool_input"] = args.clone();
    p["cwd"] = json!(fx.lead.to_str().unwrap());
    p
}

impl Driver for Bin {
    fn keep(&self, fx: &Fx, args: Value) -> Result<String, String> {
        use std::process::Stdio;
        let quiet = (Some(0), String::new(), String::new());
        let pre = verdict_payload(fx, "PreToolUse-Bash-main.json", &args);
        assert_eq!(
            hook(fx, &["work", "--event", "verdict"], &pre),
            quiet,
            "PreToolUse"
        );

        let call = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "work_verdict", "arguments": args}});
        let input = format!(
            "{}\n{}\n{call}\n",
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#,
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        );
        let mut cmd = agent_cmd(fx);
        cmd.args(["mcp", "--agent", "claude-code"]);
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        common::write_stdin(child.stdin.take().unwrap(), input.as_bytes());
        let out = child.wait_with_output().unwrap();
        let (stdout, stderr) = (
            String::from_utf8(out.stdout).unwrap(),
            String::from_utf8(out.stderr).unwrap(),
        );
        assert_eq!(out.status.code(), Some(0), "mcp: {stdout}\n{stderr}");
        for m in MARKERS {
            assert!(!stderr.contains(m), "{m} is in the mcp stderr: {stderr}");
        }
        let reply = stdout
            .lines()
            .map(|l| serde_json::from_str::<Value>(l).unwrap())
            .find(|v| v["id"] == 2)
            .unwrap_or_else(|| panic!("no reply 2: {stdout}\n{stderr}"));
        let result = reply
            .get("result")
            .unwrap_or_else(|| panic!("no result: {stdout}\n{stderr}"));
        let text = result["content"][0]["text"].as_str().unwrap().to_string();
        let is_error = result["isError"].as_bool().unwrap_or(false);

        let post = if is_error {
            let mut p = verdict_payload(fx, "PostToolUseFailure-Read-main.json", &args);
            p["error"] = json!(text);
            p
        } else {
            let mut p = verdict_payload(fx, "PostToolUse-Bash-main.json", &args);
            p["tool_response"] = json!([{"type": "text", "text": text}]);
            p
        };
        assert_eq!(hook(fx, &["work"], &post), quiet, "PostToolUse");
        if is_error { Err(text) } else { Ok(text) }
    }

    fn decision(&self, fx: &Fx, cwd: &Path, agent: bool, args: &[&str]) -> (i32, String, String) {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_zygo"));
        common::scratch_home(clean_env(&mut cmd), &fx.home);
        cmd.arg("decision")
            .args(args)
            .current_dir(cwd)
            .env("GIT_CEILING_DIRECTORIES", &fx.root);
        cmd.stdin(std::process::Stdio::null());
        if agent {
            cmd.env("ZYGO_SESSION_DIR", &fx.sd.root);
        } else {
            cmd.env_remove("ZYGO_SESSION_DIR");
        }
        let out = cmd.output().unwrap();
        (
            out.status.code().unwrap(),
            String::from_utf8(out.stdout).unwrap(),
            String::from_utf8(out.stderr).unwrap(),
        )
    }
}

// ---- the flows ----

/// `zygo decision new` as the agent: an open `security` finding of UNIT round 1, filled. The id.
fn new_record(d: &dyn Driver, fx: &Fx) -> String {
    let args = [
        "new",
        "--kind",
        "open-finding",
        "--title",
        TITLE,
        "--unit",
        UNIT,
        "--round",
        "1",
        "--role",
        "code",
        "--class",
        "security",
    ];
    let (code, out, err) = d.decision(fx, &fx.work, true, &args);
    assert_eq!((code, err.as_str()), (0, ""), "new: {out}");
    let id = Path::new(out.trim_end())
        .file_stem()
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    fill(&rec_path(fx, &id));
    let r = record(fx, &id);
    assert_eq!((r.status, r.class), (Status::Open, Class::Security));
    id
}

/// An operator verb: exit 0, no stderr, one stdout line, the path of the record `id`.
fn assert_operator_ok(fx: &Fx, ran: (i32, String, String), id: &str) {
    let (code, out, err) = ran;
    assert_eq!((code, err.as_str()), (0, ""), "{out}");
    assert_eq!(out.lines().count(), 1, "{out:?}");
    assert_eq!(
        Path::new(out.trim_end())
            .file_name()
            .unwrap()
            .to_str()
            .unwrap(),
        format!("{id}.md")
    );
    assert!(rec_path(fx, id).is_file());
}

/// The record, its agent commit, and the first round-1 keep refused with the stop. The id.
fn record_and_stop(d: &dyn Driver, fx: &Fx) -> String {
    let id = new_record(d, fx);
    let h1 = commit(&fx.work, &agent_msg("add record", &id));
    fx.row(1);
    let refused = d.keep(
        fx,
        keep_args(fx, 1, &h1, vec![stop_entry(&id), rejected_entry()]),
    );
    let want = format!(
        "zygo: keep refused: {LABEL_STOP}: class security stops for the operator: ratify or overturn {id}"
    );
    assert_eq!(refused, Err(want));
    assert_eq!(fx.verdict(1), None);
    id
}

/// D§8.5 steps 1, 3-5; #7 ratify.
fn ratify_flow(d: &dyn Driver, fx: &Fx) {
    let id = record_and_stop(d, fx);
    assert_operator_ok(
        fx,
        d.decision(fx, &fx.work, false, &["ratify", &id, "--yes"]),
        &id,
    );
    assert_eq!(record(fx, &id).status, Status::Ratified);
    let h2 = commit(&fx.work, "ratify");
    let kept = d.keep(
        fx,
        keep_args(fx, 1, &h2, vec![stop_entry(&id), rejected_entry()]),
    );
    assert_eq!(kept, Ok(format!("recorded keep for {UNIT} round 1")));
    assert_eq!(fx.verdict(1).as_deref(), Some("keep"));
}

/// D§8.5 step 5; #7 overturn. The final head.
fn overturn_flow(d: &dyn Driver, fx: &Fx) -> String {
    let id = record_and_stop(d, fx);
    assert_operator_ok(
        fx,
        d.decision(
            fx,
            &fx.work,
            false,
            &["overturn", &id, "--reason", NOTE, "--yes"],
        ),
        &id,
    );
    let r = record(fx, &id);
    assert_eq!(
        (r.status, r.review_note.as_deref()),
        (Status::Overturned, Some(NOTE))
    );
    let h2 = commit(&fx.work, "overturn");
    let refused = d.keep(
        fx,
        keep_args(fx, 1, &h2, vec![stop_entry(&id), rejected_entry()]),
    );
    assert_eq!(
        refused,
        Err(format!(
            "zygo: keep refused: {LABEL_STOP}: {id} overturned by the operator"
        ))
    );
    assert_eq!(fx.verdict(1), None);

    fx.row(2);
    fs::write(fx.work.join("fix.txt"), "fixed\n").unwrap();
    let a = commit(&fx.work, &agent_msg("fix", &id));
    let p = rec_path(fx, &id);
    let text = fs::read_to_string(&p).unwrap();
    assert_eq!(text.matches("status = \"overturned\"").count(), 1, "{text}");
    fs::write(
        &p,
        text.replace(
            "status = \"overturned\"",
            &format!("status = \"fixed\"\nfixed_by = \"{a}\""),
        ),
    )
    .unwrap();
    let h3 = commit(&fx.work, &agent_msg("set fixed", &id));
    let fixed =
        json!({"finding": LABEL_STOP, "class": "security", "disposition": "fixed", "commit": a});
    let kept = d.keep(fx, keep_args(fx, 2, &h3, vec![fixed, rejected_entry()]));
    assert_eq!(kept, Ok(format!("recorded keep for {UNIT} round 2")));
    assert_eq!(fx.verdict(2).as_deref(), Some("keep"));
    assert_eq!(fx.verdict(1), None);
    let r = record(fx, &id);
    assert_eq!(
        (r.status, r.fixed_by.as_deref()),
        (Status::Fixed, Some(a.as_str()))
    );
    h3
}

/// #8 (`Node` only): after the merge, a keep without `base` is refused.
fn merged_keep_is_refused(d: &dyn Driver, fx: &Fx, head: &str) {
    git(
        &fx.lead,
        &["merge", "-q", "--no-ff", "--no-edit", "node/N1"],
    );
    fx.row(3);
    let refused = d.keep(fx, keep_args(fx, 3, head, vec![rejected_entry()]));
    assert_eq!(
        refused,
        Err(
            "zygo: keep refused: head is already in HEAD: call before the merge, or give base"
                .to_string()
        )
    );
    assert_eq!(fx.verdict(3), None);
}

// ---- CI tests: in process ----

#[test]
fn inproc_node_checkout_ratify_flow() {
    let fx = Fx::new(Layout::Node);
    ratify_flow(&InProc, &fx);
    assert_no_text(&fx);
}

#[test]
fn inproc_node_checkout_overturn_flow_then_merged_keep_is_refused() {
    let fx = Fx::new(Layout::Node);
    let head = overturn_flow(&InProc, &fx);
    merged_keep_is_refused(&InProc, &fx, &head);
    assert_no_text(&fx);
}

#[test]
fn inproc_one_checkout_ratify_flow() {
    let fx = Fx::new(Layout::One);
    ratify_flow(&InProc, &fx);
    assert_no_text(&fx);
}

#[test]
fn inproc_one_checkout_overturn_flow() {
    let fx = Fx::new(Layout::One);
    overturn_flow(&InProc, &fx);
    assert_no_text(&fx);
}

// ---- on demand: the binary (timing) ----

#[test]
#[ignore = "timing: git calls run under the 2 s production bound (GIT_STEP)"]
fn binary_node_checkout_ratify_flow() {
    let fx = Fx::new(Layout::Node);
    ratify_flow(&Bin, &fx);
    assert_no_text(&fx);
}

#[test]
#[ignore = "timing: git calls run under the 2 s production bound (GIT_STEP)"]
fn binary_node_checkout_overturn_flow_then_merged_keep_is_refused() {
    let fx = Fx::new(Layout::Node);
    let head = overturn_flow(&Bin, &fx);
    merged_keep_is_refused(&Bin, &fx, &head);
    assert_no_text(&fx);
}

#[test]
#[ignore = "timing: git calls run under the 2 s production bound (GIT_STEP)"]
fn binary_one_checkout_ratify_flow() {
    let fx = Fx::new(Layout::One);
    ratify_flow(&Bin, &fx);
    assert_no_text(&fx);
}

#[test]
#[ignore = "timing: git calls run under the 2 s production bound (GIT_STEP)"]
fn binary_one_checkout_overturn_flow() {
    let fx = Fx::new(Layout::One);
    overturn_flow(&Bin, &fx);
    assert_no_text(&fx);
}
