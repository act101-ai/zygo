//! WD-19: hooks, the MCP server and the status line read nothing from `.zygo/` (R-C8; R§8 "Not a layer"; R§10 row
//! "Hooks, the MCP server, the status line"; R§18 item 20; review focus 38). p-tests, every OS: source scans of the
//! crates that the hooks and the MCP server link, of `load`, and of the one `zygo-workspace` caller of the read.
//! h-tests, unix (workspace state, a `sh` fake `git`, process groups; OP5): each hook registration, each OpenCode
//! plugin call and `zygo mcp`, in a repository with an approved `.zygo/` and in the same repository without it. No
//! agent runs (R-C9), the built-in marketplace is off in the user config (`NO_MARKETPLACE`), no test asserts a time.
mod common;

use std::collections::BTreeSet;
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};

/// The workspace root.
fn ws_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// Every `.rs` file under `dir`, sorted.
fn rs_files(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = walkdir::WalkDir::new(dir)
        .into_iter()
        .map(|e| e.unwrap())
        .filter(|e| e.file_type().is_file() && e.path().extension().is_some_and(|x| x == "rs"))
        .map(|e| e.into_path())
        .collect();
    v.sort();
    v
}

/// The line indexes of the one item whose line, trimmed at the start, starts with `head`, through the first later
/// line that is its indentation and `}`.
fn body(lines: &[&str], head: &str) -> RangeInclusive<usize> {
    let starts: Vec<usize> =
        lines.iter().enumerate().filter(|(_, l)| l.trim_start().starts_with(head)).map(|(i, _)| i).collect();
    assert_eq!(starts.len(), 1, "`{head}` at {starts:?}");
    let s = starts[0];
    let close = format!("{}}}", &lines[s][..lines[s].len() - lines[s].trim_start().len()]);
    let e = (s + 1..lines.len()).find(|&j| lines[j] == close).unwrap_or_else(|| panic!("`{head}`: no closing brace"));
    s..=e
}

/// The `zygo-*` names in `[dependencies]` and `[target.*.dependencies]` of `crates/<name>/Cargo.toml`.
fn zygo_deps(name: &str) -> BTreeSet<String> {
    let t: toml::Table = read(&ws_root().join("crates").join(name).join("Cargo.toml")).parse().unwrap();
    let mut tables: Vec<&toml::Table> = t.get("dependencies").and_then(|v| v.as_table()).into_iter().collect();
    if let Some(targets) = t.get("target").and_then(|v| v.as_table()) {
        tables.extend(targets.values().filter_map(|v| v.get("dependencies").and_then(|d| d.as_table())));
    }
    tables.iter().flat_map(|d| d.keys()).filter(|k| k.starts_with("zygo-")).cloned().collect()
}

/// The crates of the hooks and the MCP server, and each crate they link except `zygo-config` (p4) and
/// `zygo-workspace` (p3).
const SCANNED: [&str; 10] = [
    "zygo-core", "zygo-hooks", "zygo-mail-sync", "zygo-mcp", "zygo-plugins", "zygo-proxy", "zygo-redact",
    "zygo-results", "zygo-usage", "zygo-work",
];
/// `zygo hook` (the HUD is `statusline_hud`), `zygo mcp` and the `zygo statusline` verb.
const CMD_FILES: [&str; 3] = ["crates/zygo/src/cmd/hook.rs", "crates/zygo/src/cmd/mcp.rs", "crates/zygo/src/cmd/statusline.rs"];
/// Names of the definition read (R§6.1, R§7.3, R§7.4), of its apply step (R§8) and of the code that calls them.
const FORBIDDEN: [&str; 6] = ["repo_def", "with_definition", "check_default", "check_pinned", "zygo_workspace::up", "zygo_launch"];

#[test]
fn p1_hook_and_mcp_crates_link_only_the_scanned_crates() { // R-C8; drift 9
    let mut seen = BTreeSet::new();
    let mut todo = vec!["zygo-hooks".to_string(), "zygo-mcp".to_string()];
    while let Some(c) = todo.pop() {
        for d in zygo_deps(&c) {
            if seen.insert(d.clone()) {
                todo.push(d);
            }
        }
    }
    let expect: BTreeSet<String> = [
        "zygo-config", "zygo-core", "zygo-mail-sync", "zygo-plugins", "zygo-proxy", "zygo-redact", "zygo-results",
        "zygo-usage", "zygo-work", "zygo-workspace",
    ]
    .map(String::from)
    .into();
    assert_eq!(seen, expect);
}

#[test]
fn p2_no_hook_or_mcp_source_names_the_definition_read() { // R-C8; R§10 row; R§18 item 20
    let brace_up = regex::Regex::new(r"zygo_workspace::\{[^}]*\bup\b").unwrap();
    let mut files: Vec<PathBuf> =
        SCANNED.iter().flat_map(|c| rs_files(&ws_root().join("crates").join(c).join("src"))).collect();
    files.extend(CMD_FILES.iter().map(|f| ws_root().join(f)));
    assert!(files.len() >= 140, "{} files", files.len());
    assert!(files.iter().any(|f| f.ends_with("crates/zygo-hooks/src/statusline/data.rs")));
    for f in &files {
        let text = read(f);
        for (i, line) in text.lines().enumerate() {
            for n in FORBIDDEN {
                assert!(!line.contains(n), "{}:{}: names {n}", f.display(), i + 1);
            }
        }
        assert!(!brace_up.is_match(&text), "{}: imports zygo_workspace::up", f.display());
    }
}

#[test]
fn p3_zygo_workspace_reads_the_definition_only_in_up_choose() { // R-C8; R§10 `zygo up` rows; drift 8
    let src = ws_root().join("crates/zygo-workspace/src");
    let rel = |f: &Path| f.strip_prefix(&src).unwrap().to_string_lossy().replace('\\', "/");
    let readers: Vec<String> = rs_files(&src)
        .iter()
        .filter(|f| {
            let t = read(f);
            t.contains("check_default") || t.contains("check_pinned")
        })
        .map(|f| rel(f))
        .collect();
    assert_eq!(readers, ["up.rs"]);
    let up = read(&src.join("up.rs"));
    let lines: Vec<&str> = up.lines().collect();
    let choose = body(&lines, "fn choose(");
    for (i, l) in lines.iter().enumerate() {
        if l.contains("check_default") || l.contains("check_pinned") {
            assert!(l.starts_with("use zygo_config::repo_def::") || choose.contains(&i), "up.rs:{}: {l}", i + 1);
        }
    }
    assert!(lines[choose.clone()].iter().any(|l| l.contains("check_default(")));
    for head in ["fn choose(", "fn loadout_plan(", "fn build(", "fn run(", "fn run_all("] {
        assert_eq!(lines.iter().filter(|l| l.starts_with(head)).count(), 1, "up.rs: `{head}` at column 0 (private)");
    }
    let entry = regex::Regex::new(r"\bup::(up|up_all)\b|crate::up::\{[^}]*\b(up|up_all)\b").unwrap();
    for f in rs_files(&src) {
        if rel(&f) != "up.rs" {
            assert!(!entry.is_match(&read(&f)), "{}: calls up::up or up::up_all", rel(&f));
        }
    }
}

#[test]
fn p4_load_reads_no_definition_and_repo_def_is_only_in_with_definition() { // R§8 "Not a layer"; R-C8; review focus 38
    let text = read(&ws_root().join("crates/zygo-config/src/layers.rs"));
    let lines: Vec<&str> = text.lines().collect();
    for head in ["pub fn load(", "pub fn load_with_builtin(", "pub fn load_with_repo_reader("] {
        for i in body(&lines, head) {
            for n in ["repo_def", "Definition", "with_definition", "check_default", "check_pinned", ".zygo/"] {
                assert!(!lines[i].contains(n), "layers.rs:{}: `{head}` names {n}", i + 1);
            }
        }
    }
    assert!(lines[body(&lines, "pub fn load(")].iter().any(|l| l.contains("load_with_builtin(ctx, builtin_layer())")));
    assert!(lines[body(&lines, "pub fn load_with_builtin(")]
        .iter()
        .any(|l| l.contains("load_with_repo_reader(ctx, builtin, read_default_branch)")));
    let with = body(&lines, "pub fn with_definition(");
    let at: Vec<usize> = lines.iter().enumerate().filter(|(_, l)| l.contains("repo_def")).map(|(i, _)| i).collect();
    for &i in &at {
        assert!(
            lines[i] == "use crate::repo_def::{self, Definition};" || with.contains(&i),
            "layers.rs:{}: {}",
            i + 1,
            lines[i]
        );
    }
    assert!(at.iter().any(|i| with.contains(i)));
    // No other file of zygo-config outside `repo_def/` reaches the read.
    let src = ws_root().join("crates/zygo-config/src");
    let mut named = Vec::new();
    for f in rs_files(&src) {
        let r = f.strip_prefix(&src).unwrap().to_string_lossy().replace('\\', "/");
        if r.starts_with("repo_def/") {
            continue;
        }
        let t = read(&f);
        for n in ["check_default", "check_pinned", "read_default(", "read_at("] {
            assert!(!t.contains(n), "{r}: names {n}");
        }
        if t.contains("repo_def") {
            named.push(r);
        }
    }
    assert_eq!(named, ["layers.rs", "lib.rs"]);
}

#[cfg(unix)]
mod unix {
    use std::ffi::OsStr;
    use std::path::{Path, PathBuf};
    use std::process::{Output, Stdio};
    use std::time::{Duration, Instant};

    use serde_json::{json, Value};
    use zygo_config::repo_def::{approve_workspace_in, check_default, Definition, GitRunner, Use};
    use zygo_core::session_dir::SessionDir;
    use zygo_core::session_view::{Member, SessionView, ViewWorkspace};
    use zygo_core::Agent;
    use zygo_work::repo::checkout;
    use zygo_workspace::identity::workspace_id;
    use zygo_workspace::state::{MemberRecord, MemberState, Pane, PaneKind};

    use crate::common::feed_stdin;
    use crate::common::workspace::{ready_exe, Guarded, Harness, WS_ID};

    const RUST: &str = "overrides = { code = \"claude-haiku-5-5\" }\ndisabled = [\"design\"]\n";
    /// The built-in marketplace off (as `workspace_definition.rs:142`): no run reaches the network.
    const NO_MARKETPLACE: &str = "[[marketplace]]\nname = \"act101\"\nref = \"\"\n";
    const KEY: &str = "kaaaaaaaa";
    const SID: &str = "01JTESTSESSIONAAAAAAAAAAAA";
    const OC_SID: &str = "ses_placeholder0001";
    /// The `load` read of `.zygo.toml` through `bin/git` (`repo_file.rs:252`, `:257`; `origin/HEAD` is `origin/main`).
    const LOAD_READ: &str = "show origin/main:.zygo.toml";
    /// The R§6.1 step 2 lookup of the definition read (`repo_def/read.rs:80`).
    const DEFAULT_LOOKUP: &str = "symbolic-ref --quiet refs/remotes/origin/HEAD";

    /// A `bin/git` line of a read of `.zygo/` (drift 5); the `load` read of `.zygo.toml` is not one.
    fn reads_definition(line: &str) -> bool {
        line.contains(DEFAULT_LOOKUP)
            || line.split(' ').any(|a| a == ".zygo" || a.starts_with(".zygo/") || a.contains(":.zygo/") || a.ends_with(":.zygo"))
    }

    /// A test-side git call: no user or system config, no templates, no signing, a fixed identity; asserts success;
    /// trimmed stdout.
    fn git(dir: &Path, args: &[&str]) -> String {
        let o = std::process::Command::new("git")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_TEMPLATE_DIR", "")
            .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid", "-c", "commit.gpgsign=false", "-C"])
            .arg(dir)
            .args(args)
            .stdin(Stdio::null())
            .output()
            .expect("git on PATH");
        assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    }

    /// `<root>/app`: a clone of the bare `<root>/o.git` whose `main` holds `files`, so `refs/remotes/origin/HEAD` is
    /// `refs/remotes/origin/main` (R§18 preamble; as `workspace_definition.rs:56-73`).
    fn repo_at(root: &Path, files: &[(&str, &str)]) -> PathBuf {
        let w = root.join("w");
        std::fs::create_dir_all(&w).unwrap();
        for (path, text) in files {
            let p = w.join(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        git(root, &["init", "-q", "--bare", "-b", "main", "o.git"]);
        git(&w, &["init", "-q", "-b", "main"]);
        git(&w, &["add", "-A"]);
        git(&w, &["commit", "-q", "-m", "c"]);
        git(&w, &["remote", "add", "origin", "../o.git"]);
        git(&w, &["push", "-q", "origin", "main"]);
        git(root, &["clone", "-q", "o.git", "app"]);
        root.join("app")
    }

    fn ws_id(app: &Path) -> String {
        workspace_id(&checkout(app, Instant::now() + Duration::from_secs(3600)).unwrap().common_dir)
    }

    /// Approves the default branch's files of `app`, read through `git` (the `bin/git` of [`fake_git`], so the read has
    /// no user or system config); returns the approved definition.
    fn approve(app: &Path, approvals: &Path, git: &dyn GitRunner) -> Definition {
        let id = ws_id(app);
        if let Use::NotApproved { digest, .. } = check_default(app, &id, approvals, git) {
            approve_workspace_in(approvals, &id, &digest).unwrap();
        }
        match check_default(app, &id, approvals, git) {
            Use::Approved { def, .. } => def,
            other => panic!("not approved: {other:?}"),
        }
    }

    /// `bin/git`: appends each call (`$*`) to `<root>/git.log`, then runs the real git with no user or system config and
    /// no templates (as `workspace_definition.rs:578`).
    fn fake_git(h: &Harness) {
        use std::os::unix::fs::PermissionsExt;
        let real = which::which("git").unwrap();
        let body = format!(
            "#!/bin/sh\necho \"$*\" >> '{}'\nGIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 GIT_TEMPLATE_DIR= \
             exec '{}' \"$@\"\n",
            h.root().join("git.log").display(),
            real.display()
        );
        let p = h.bin.join("git");
        std::fs::write(&p, body).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        ready_exe(&p);
        let _ = std::fs::remove_file(h.root().join("git.log"));
    }

    struct Fx {
        h: Harness,
        app: PathBuf,
        head: String,
    }

    /// A harness with `NO_MARKETPLACE` as the user config, `bin/git` from [`fake_git`], and `app` with `.zygo.toml` =
    /// `version = 1` and `README`. With `definition`, also `.zygo/workspace.toml` = `version = 1` and
    /// `.zygo/profiles/rust.toml` = `RUST`, approved for `app` (a definition that changes `up` and a `rust` launch).
    fn fx(definition: bool) -> Fx {
        let h = Harness::no_tmux();
        std::fs::write(h.root().join("config.toml"), NO_MARKETPLACE).unwrap();
        let mut files = vec![(".zygo.toml", "version = 1\n"), ("README", "app\n")];
        if definition {
            files.push((".zygo/workspace.toml", "version = 1\n"));
            files.push((".zygo/profiles/rust.toml", RUST));
        }
        let app = repo_at(h.root(), &files);
        let head = git(&app, &["rev-parse", "HEAD"]);
        fake_git(&h);
        if definition {
            let def = approve(&app, &h.paths.approvals_file, &LogGit(h.bin.join("git")));
            assert!(def.workspace.is_some() && def.profiles.contains_key("rust"), "{def:?}");
            let _ = std::fs::remove_file(h.root().join("git.log"));
        }
        Fx { h, app, head }
    }

    /// The `bin/git` lines since the last call, with the harness root as `<root>` and HEAD of `app` as `<head>`,
    /// sorted; the log is removed.
    fn take_log(fx: &Fx) -> Vec<String> {
        let p = fx.h.root().join("git.log");
        let text = std::fs::read_to_string(&p).unwrap_or_default();
        let _ = std::fs::remove_file(&p);
        let root = fx.h.root().display().to_string();
        let mut v: Vec<String> = text.lines().map(|l| l.replace(&root, "<root>").replace(&fx.head, "<head>")).collect();
        v.sort();
        v
    }

    /// `<root>/<name>`: `view.json` of session `SID` for `agent`, main and effective role `lead`, profile `rust`,
    /// work units, results, compact tool and mail on, `repo_root` = `app`, style `verbose`. With `member`: the marker
    /// `{WS_ID, KEY, worker-1}` (AU58) and the state of `WS_ID` with `KEY` running in `app`.
    fn session(fx: &Fx, name: &str, agent: Agent, member: bool) -> SessionDir {
        let sd = SessionDir::new(fx.h.root().join(name));
        sd.create().unwrap();
        let workspace = member.then(|| ViewWorkspace {
            member: Member { ws_id: WS_ID.into(), pane: KEY.into(), name: Some("worker-1".into()), ..Default::default() },
            resume_note: None,
            ..Default::default()
        });
        SessionView {
            session: SID.into(),
            agent,
            main_role: "lead".into(),
            effective_role: Some("lead".into()),
            profile: Some("rust".into()),
            work_units: true,
            results: true,
            compact_tool: true,
            mail: true,
            repo_root: Some(fx.app.display().to_string()),
            statusline_style: "verbose".into(),
            trim_threshold_bytes: 16384,
            head_lines: 60,
            tail_lines: 20,
            workspace,
            ..Default::default()
        }
        .save(&sd)
        .unwrap();
        if member {
            let app = fx.app.clone();
            let kind = if agent == Agent::OpenCode { PaneKind::OpenCode } else { PaneKind::ClaudeCode };
            fx.h.seed(WS_ID, &mut |w| {
                w.common_dir = app.join(".git");
                w.main_root = app.clone();
                w.panes = vec![Pane { key: KEY.into(), cwd: app.clone(), kind, ..Default::default() }];
                w.members.clear();
                w.members.insert(
                    KEY.into(),
                    MemberRecord { zygo_session: Some(SID.into()), state: MemberState::Running, ..MemberRecord::new(agent) },
                );
            });
        }
        sd
    }

    /// `zygo <args>` in `app` with `ZYGO_SESSION_DIR` = `sd`, `COLUMNS` = 200 and `NO_COLOR` = 1, `stdin` fed and
    /// closed; asserts exit 0 (C6; `zygo mcp` at the end of its input). Returns stdout and the run's `bin/git` lines.
    fn run(fx: &Fx, sd: &SessionDir, args: &[&str], stdin: &[u8]) -> (String, Vec<String>) {
        let extra: [(&str, &OsStr); 3] =
            [("ZYGO_SESSION_DIR", sd.root.as_os_str()), ("COLUMNS", OsStr::new("200")), ("NO_COLOR", OsStr::new("1"))];
        let mut c = fx.h.zygo(args, &extra);
        c.current_dir(&fx.app).stdin(Stdio::piped());
        let mut g = Guarded::spawn(c);
        feed_stdin(g.stdin().unwrap(), stdin);
        let o: Output = g.output(&args.join(" "));
        assert_eq!(o.status.code(), Some(0), "zygo {}: {}", args.join(" "), String::from_utf8_lossy(&o.stderr));
        (String::from_utf8_lossy(&o.stdout).into_owned(), take_log(fx))
    }

    /// The recorded Claude Code input `file` with `"/repo` (its `cwd` and tool paths) moved to `app`.
    fn cc(fx: &Fx, file: &str) -> Vec<u8> {
        let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/claude-code/hooks").join(file);
        std::fs::read_to_string(p).unwrap().replace("\"/repo", &format!("\"{}", fx.app.display())).into_bytes()
    }

    /// The recorded OpenCode plugin payload `file` (`{input, output}`, as the plugin sends it).
    fn oc(file: &str) -> Vec<u8> {
        std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/opencode/plugin-hooks").join(file)).unwrap()
    }

    /// `PreToolUse-Bash-sub.json` as a call of MCP tool `tool` with `input` (as `work_hook.rs:52-54`).
    fn work_tool(fx: &Fx, tool: &str, input: Value) -> Vec<u8> {
        let mut v: Value = serde_json::from_slice(&cc(fx, "PreToolUse-Bash-sub.json")).unwrap();
        v["tool_name"] = json!(tool);
        v["tool_input"] = input;
        v.to_string().into_bytes()
    }

    /// One row: (label, `zygo` arguments, stdin).
    type Row = (String, String, Vec<u8>);
    /// One run: (label, stdout, git lines).
    type Ran = (String, String, Vec<String>);

    /// The Claude Code registrations of every launch (`claude_code.rs:462-541`).
    fn cc_main(fx: &Fx) -> Vec<Row> {
        let app = fx.app.display().to_string();
        let hud = json!({"session_id": "00000000-0000-4000-8000-000000000001", "cwd": app,
            "workspace": {"current_dir": app, "project_dir": app}})
        .to_string()
        .into_bytes();
        let begin = json!({"unit": "beads:bd-a1b2", "round": 1, "check": "cargo test -p demo", "files": ["src/"]});
        let verdict = json!({"unit": "beads:bd-a1b2", "round": 1, "verdict": "discard"});
        let mut v = vec![
            ("hook token".to_string(), Vec::new()),
            ("hook statusline".to_string(), hud),
            ("hook work --event begin".to_string(), work_tool(fx, "mcp__zygo__work_begin", begin)),
            ("hook work --event verdict".to_string(), work_tool(fx, "mcp__zygo__work_verdict", verdict)),
        ];
        for (args, file) in [
            ("hook role-guard", "PreToolUse-Edit-main.json"),
            ("hook guard", "PreToolUse-Edit-main.json"),
            ("hook guard", "PreToolUse-Bash-main.json"),
            ("hook guard", "PreToolUse-Read-main.json"),
            ("hook trim", "PostToolUse-Bash-main.json"),
            ("hook trim", "PostToolUse-Read-main.json"),
            ("hook work", "PostToolUse-Bash-sub.json"),
            ("hook work", "PostToolUseFailure-Read-main.json"),
            ("hook work", "SubagentStop-zygo-code-sub.json"),
            ("hook session-start", "SessionStart-main.json"),
            ("hook session-start", "SubagentStart-sub.json"),
            ("hook prompt", "UserPromptSubmit-main.json"),
            ("hook pre-compact", "PreCompact-main.json"),
        ] {
            v.push((format!("{args} < {file}"), cc(fx, file)));
        }
        v.into_iter().map(|(label, stdin)| (label.clone(), label.split(" < ").next().unwrap().to_string(), stdin)).collect()
    }

    /// The registrations that a member adds (`claude_code.rs:489-525`) and the other `workspace` events
    /// (`crates/zygo-hooks/src/workspace.rs:41-42`).
    fn cc_member(fx: &Fx) -> Vec<Row> {
        [
            ("hook session-start --event member", "SessionStart-main.json"),
            ("hook workspace --event session", "SessionStart-main.json"),
            ("hook workspace --event prompt", "UserPromptSubmit-main.json"),
            ("hook workspace --event pre-tool", "PreToolUse-Bash-main.json"),
            ("hook workspace --event tool-end", "PostToolUse-Bash-main.json"),
            ("hook workspace --event subagent-start", "SubagentStart-sub.json"),
            ("hook workspace --event subagent-stop", "SubagentStop-sub.json"),
            ("hook workspace --event stop", "Stop-main.json"),
            ("hook workspace --event session-end", "Stop-main.json"),
        ]
        .into_iter()
        .map(|(args, file)| (format!("{args} < {file}"), args.to_string(), cc(fx, file)))
        .collect()
    }

    /// The `zygo hook` calls of the OpenCode plugin of a member (`opencode-plugin.js:23`, `:45-194`).
    fn oc_member() -> Vec<Row> {
        let s = |v: Value| v.to_string().into_bytes();
        vec![
            ("hook guard --agent opencode", oc("tool.execute.before-bash.json")),
            ("hook trim --agent opencode", oc("tool.execute.after-bash.json")),
            ("hook work --agent opencode", oc("tool.execute.after-bash.json")),
            ("hook session-start --agent opencode", oc("experimental.chat.system.transform.json")),
            ("hook session-start --agent opencode --event subagent", oc("experimental.chat.system.transform.json")),
            ("hook pre-compact --agent opencode", oc("experimental.session.compacting.json")),
            ("hook workspace --agent opencode --event session", s(json!({}))),
            ("hook workspace --agent opencode --event prompt", s(json!({"sessionID": OC_SID}))),
            ("hook workspace --agent opencode --event pre-tool", s(json!({"sessionID": OC_SID, "tool_name": "bash"}))),
            ("hook workspace --agent opencode --event tool-end", s(json!({"sessionID": OC_SID}))),
            ("hook workspace --agent opencode --event stop", s(json!({"sessionID": OC_SID}))),
        ]
        .into_iter()
        .map(|(args, stdin)| (args.to_string(), args.to_string(), stdin))
        .collect()
    }

    /// Each row of `rows` in session `sd`, in order.
    fn run_rows(fx: &Fx, sd: &SessionDir, rows: Vec<Row>) -> Vec<Ran> {
        rows.into_iter()
            .map(|(label, args, stdin)| {
                let a: Vec<&str> = args.split(' ').collect();
                let (out, log) = run(fx, sd, &a, &stdin);
                (label, out, log)
            })
            .collect()
    }

    /// `calls` on a fixture with an approved `.zygo/` and on one without: the same labels; in the first, no line
    /// reads `.zygo/`; per label the same git lines in both, except for the status line (drift 7). Returns the first.
    fn compare(calls: &dyn Fn(&Fx) -> Vec<Ran>) -> Vec<Ran> {
        let with = calls(&fx(true));
        let without = calls(&fx(false));
        let labels = |r: &[Ran]| r.iter().map(|c| c.0.clone()).collect::<Vec<_>>();
        assert_eq!(labels(&with), labels(&without));
        for ((label, _, w), (_, _, wo)) in with.iter().zip(&without) {
            for l in w {
                assert!(!reads_definition(l), "{label}: {l}");
            }
            if !label.starts_with("hook statusline") {
                assert_eq!(w, wo, "{label}: other git calls with .zygo/");
            }
        }
        with
    }

    /// The git lines of the run `label`.
    fn log_of<'a>(ran: &'a [Ran], label: &str) -> &'a [String] {
        &ran.iter().find(|c| c.0 == label).unwrap_or_else(|| panic!("no run {label}")).2
    }

    /// Runs `bin/git` of a harness as `git -C <top> <args>`, so [`fake_git`] logs the call.
    struct LogGit(PathBuf);

    impl GitRunner for LogGit {
        fn git(&self, top: &Path, args: &[&str], _deadline: Instant) -> Result<Output, String> {
            std::process::Command::new(&self.0)
                .arg("-C")
                .arg(top)
                .args(args)
                .env("LC_ALL", "C")
                .stdin(Stdio::null())
                .output()
                .map_err(|e| e.to_string())
        }
    }

    #[test]
    fn h0_the_log_predicate_sees_a_definition_read() { // R§18 item 20: the control of h1-h4
        let fx = fx(true);
        match check_default(&fx.app, &ws_id(&fx.app), &fx.h.paths.approvals_file, &LogGit(fx.h.bin.join("git"))) {
            Use::Approved { def, .. } => assert!(def.profiles.contains_key("rust")),
            other => panic!("{other:?}"),
        }
        let lines = take_log(&fx);
        assert!(lines.iter().any(|l| l.contains(DEFAULT_LOOKUP)), "{lines:?}");
        assert!(lines.iter().any(|l| l.contains("ls-tree -z --long --full-tree") && l.ends_with(" -- .zygo")), "{lines:?}");
        assert!(lines.iter().filter(|l| reads_definition(l)).count() >= 2, "{lines:?}");
        assert!(!reads_definition("-C <root>/app show origin/main:.zygo.toml"));
        assert!(!reads_definition("-C <root>/app ls-tree --name-only origin/main -- .zygo.toml"));
        assert!(!reads_definition("-c core.fsmonitor=false -C <root>/app symbolic-ref --short -q HEAD"));
    }

    #[test]
    fn h1_claude_code_hooks_of_a_main_session_read_no_definition() { // R-C8; R§10 row; R§18 item 20; review focus 38
        let ran = compare(&|fx| {
            let sd = session(fx, "s", Agent::ClaudeCode, false);
            run_rows(fx, &sd, cc_main(fx))
        });
        assert_eq!(ran.len(), 17);
        // Control: the hook's `load` ran git through `bin/git`, so a definition read would be in the log.
        assert!(log_of(&ran, "hook guard < PreToolUse-Bash-main.json").iter().any(|l| l.contains(LOAD_READ)));
    }

    #[test]
    fn h2_claude_code_hooks_of_a_member_read_no_definition() { // R-C8; R§10 row; R§18 item 20; review focus 38
        let ran = compare(&|fx| {
            let sd = session(fx, "m", Agent::ClaudeCode, true);
            let mut rows = cc_main(fx);
            rows.extend(cc_member(fx));
            let ran = run_rows(fx, &sd, rows);
            // Control: the member events ran (`workspace --event prompt` writes `activity.json`, W§9.1).
            assert!(sd.activity_json().is_file());
            ran
        });
        assert_eq!(ran.len(), 26);
        assert!(log_of(&ran, "hook guard < PreToolUse-Bash-main.json").iter().any(|l| l.contains(LOAD_READ)));
    }

    #[test]
    fn h3_opencode_plugin_calls_read_no_definition() { // R-C8; R§10 row; R§18 item 20; review focus 38
        let ran = compare(&|fx| {
            let sd = session(fx, "o", Agent::OpenCode, true);
            let ran = run_rows(fx, &sd, oc_member());
            assert!(sd.activity_json().is_file());
            ran
        });
        assert_eq!(ran.len(), 11);
        assert!(log_of(&ran, "hook guard --agent opencode").iter().any(|l| l.contains(LOAD_READ)));
    }

    const INIT: &str = concat!(
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-06-18\"}}\n",
        "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n",
    );

    /// The arguments of a call of `tool`: a valid begin and a valid keep (so `work_verdict` reaches `session_config`,
    /// `crates/zygo-mcp/src/tools/work.rs:43`, `:226-227`), else `{}`.
    fn tool_args(fx: &Fx, tool: &str) -> Value {
        match tool {
            "work_begin" => json!({"unit": "beads:bd-a1b2", "round": 1, "check": "cargo test -p demo", "files": ["src/"]}),
            "work_verdict" => json!({"unit": "beads:bd-a1b2", "round": 1, "verdict": "keep", "head": fx.head,
                "base": fx.head, "dispositions": []}),
            _ => json!({}),
        }
    }

    #[test]
    fn h4_mcp_server_and_tools_read_no_definition() { // R-C8; R§10 row; R§18 item 20; review focus 38
        let ran = compare(&|fx| {
            let mut out = Vec::new();
            for (name, agent, member) in [("s", "claude-code", false), ("m", "claude-code", true), ("o", "opencode", true)] {
                let kind = if agent == "opencode" { Agent::OpenCode } else { Agent::ClaudeCode };
                let sd = session(fx, name, kind, member);
                let args = ["mcp", "--agent", agent];
                let list = format!("{INIT}{{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}}\n");
                let (stdout, log) = run(fx, &sd, &args, list.as_bytes());
                let replies: Vec<Value> = stdout.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
                let tools: Vec<String> = replies.iter().find(|r| r["id"] == 2).unwrap()["result"]["tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|t| t["name"].as_str().unwrap().to_string())
                    .collect();
                out.push((format!("mcp {agent} {name} list"), tools.join(","), log));
                let mut calls = INIT.to_string();
                for (i, t) in tools.iter().enumerate() {
                    let c = json!({"jsonrpc": "2.0", "id": 3 + i, "method": "tools/call",
                        "params": {"name": t, "arguments": tool_args(fx, t)}});
                    calls.push_str(&format!("{c}\n"));
                }
                let (stdout, log) = run(fx, &sd, &args, calls.as_bytes());
                assert_eq!(stdout.lines().count(), tools.len() + 1, "{stdout}");
                out.push((format!("mcp {agent} {name} calls"), stdout, log));
            }
            out
        });
        assert_eq!(ran.len(), 6);
        assert_eq!(ran[0].1, "compact,result_view,result_search,result_tree,work_begin,work_verdict");
        for t in ["mail_send", "mail_read", "mail_list"] {
            assert!(ran[2].1.split(',').any(|n| n == t), "{}", ran[2].1);
        }
        // Control: each start's `load` (`tool.rs:60`, repo = the cwd) ran git through `bin/git`.
        for i in [0, 2, 4] {
            assert!(ran[i].2.iter().any(|l| l.contains(LOAD_READ)), "{}: {:?}", ran[i].0, ran[i].2);
        }
    }
}
