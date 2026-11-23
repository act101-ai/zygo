use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use zygo_config::{LoadContext, Merged};
use zygo_core::{hook::{HookInput, HookOutput}, overrides::Overrides, paths::Paths, session_dir::SessionDir, session_view::SessionView, Agent};
use zygo_usage::{Filter, Store};
use zygo_work::{guards, hooks::{self, HookCtx}, log::{self, Begin, CheckResult, Kind, LogLine}, WorkItem};

const SUB: &str = "a1a80dd100f197242";   // *-sub.json fixtures (N2)
const ZSUB: &str = "acfd66ba917fc3494";  // SubagentStop-zygo-code-sub.json
const ZC: Option<(&str, &str)> = Some((SUB, "zygo-code"));
const U1: &str = "beads:bd-a1b2";
const CHECK: &str = "cargo test -p demo";
const REDACTION: &str = "`[REDACTED:<id>]` marks a secret zygo removed; never write it into a file.";
const OUTPUT: &str = "zygo: do not shorten output with `| head`/`| tail`; zygo stores large results and gives you `result_search`.";
const CLAIMS_NOTIFY: &str = "zygo: this subagent did not run its acceptance check after its last edit (`beads:bd-a1b2`).";
const CLAIMS_ENFORCE: &str = "zygo: run the acceptance check `cargo test -p demo` alone, in the repository you changed, after your last edit, and quote its output in your report; say so if it fails.";
fn trailer(unit: &str) -> String {
    format!("Add the commit trailers `Zygo-Session: S1` and, when your brief names a unit, `Zygo-Work: {unit}`, each on its own line at the end of the message.")
}
fn scope_text(rel: &str, enforce: bool) -> String {
    let t = format!("zygo: `{rel}` is not in this unit's files; if the change is needed, report it.");
    if enforce { format!("{t} Ask the lead to widen the brief.") } else { t }
}
fn nav_text(p: &Path) -> String { format!("zygo: call skeleton or symbols on `{}` before reading all of it.", p.display()) }
fn deny(r: &str) -> Value { json!({"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "deny", "permissionDecisionReason": r}}) }
fn context(t: &str) -> Value { json!({"hookSpecificOutput": {"hookEventName": "PreToolUse", "additionalContext": t}}) }
fn block() -> Value { json!({"decision": "block", "reason": CLAIMS_ENFORCE}) }
fn g(guard: &str, level: &str) -> (String, String) { (guard.into(), level.into()) }

fn fixture(name: &str) -> Value {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/claude-code/hooks").join(name);
    serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap()
}
fn view(results: bool, navigation: bool) -> SessionView {
    SessionView { session: "S1".into(), agent: Agent::ClaudeCode, main_role: "lead".into(), effective_role: None, role_prompt: None,
        user_prompt: None, profile: None, profile_prompt: None, work_item: None, legend: vec![], disabled_roles: vec![],
        work_units: true, results, compact_tool: true, navigation, trim_threshold_bytes: 16384, head_lines: 60, tail_lines: 20, ..Default::default() }
}
struct Env { t: tempfile::TempDir, sd: SessionDir, repo: PathBuf, scratch: PathBuf }
/// Under CARGO_TARGET_TMPDIR: a session (units and results on), a repository whose path has a space, and `scratch`, the one
/// temp root the test context knows (G9), so no test depends on where the checkout or target dir lies.
fn env() -> Env {
    let base = Path::new(env!("CARGO_TARGET_TMPDIR"));
    std::fs::create_dir_all(base).unwrap();
    let t = tempfile::tempdir_in(base).unwrap();
    let sd = SessionDir::new(t.path().join("sessions").join("S1"));
    sd.create().unwrap();
    view(true, false).save(&sd).unwrap();
    let repo = t.path().join("repo dir");
    for d in [".git", "src"] { std::fs::create_dir_all(repo.join(d)).unwrap(); }
    let scratch = t.path().join("scratch");
    std::fs::create_dir_all(&scratch).unwrap();
    let scratch = scratch.canonicalize().unwrap();
    Env { t, sd, repo, scratch }
}
fn nav_env() -> Env { let e = env(); view(true, true).save(&e.sd).unwrap(); e }
fn merged(user: &str) -> Merged {
    let d = tempfile::tempdir().unwrap();
    let p = d.path();
    std::fs::write(p.join("config.toml"), user).unwrap();
    let paths = Paths { config_file: p.join("config.toml"), cache_dir: p.join("cache"), data_dir: p.join("data"),
        sessions_dir: p.join("cache/sessions"), marketplaces_dir: p.join("cache/marketplaces"), plugin_store: p.join("data/plugins"),
        approvals_file: p.join("data/approvals.toml"), usage_db: p.join("data/usage.db") };
    zygo_config::load(&LoadContext { paths, repo: None, team: None }).unwrap()
}
fn cfg(toml: String) -> impl Fn(&Path) -> Option<Merged> { move |_: &Path| Some(merged(&toml)) }
fn at(guard: &str, level: &str) -> impl Fn(&Path) -> Option<Merged> { cfg(format!("[guards]\n{guard} = \"{level}\"\n")) }
fn no_config(_: &Path) -> Option<Merged> { None }
fn no_item(_: &Path, _: &Merged) -> Option<WorkItem> { None }
fn quiet(_: &str) {}
fn ctx<'a>(e: &'a Env, config: &'a dyn Fn(&Path) -> Option<Merged>) -> HookCtx<'a> {
    HookCtx { session_dir: &e.sd, session: "S1".into(), usage_db: e.t.path().join("usage.db"), now: 1000, config, item: &no_item, warn: &quiet,
              temp_roots: vec![e.scratch.clone()] }
}
fn hook(v: &Value) -> HookInput { HookInput::parse(Agent::ClaudeCode, v.to_string().as_bytes()).unwrap() }
fn as_agent(mut v: Value, agent: Option<(&str, &str)>) -> Value {
    let o = v.as_object_mut().unwrap();
    o.remove("agent_id");
    o.remove("agent_type");
    if let Some((id, ty)) = agent { o.insert("agent_id".into(), json!(id)); o.insert("agent_type".into(), json!(ty)); }
    v
}
fn input(fx: &str, agent: Option<(&str, &str)>, tool: &str, tin: Value, cwd: &Path) -> Value {
    let mut v = as_agent(fixture(fx), agent);
    v["tool_name"] = json!(tool);
    v["tool_input"] = tin;
    v["cwd"] = json!(cwd);
    v
}
fn pre(agent: Option<(&str, &str)>, tool: &str, tin: Value, cwd: &Path) -> Value { input("PreToolUse-Bash-sub.json", agent, tool, tin, cwd) }
fn post(agent: Option<(&str, &str)>, tool: &str, tin: Value, cwd: &Path) -> Value { input("PostToolUse-Bash-sub.json", agent, tool, tin, cwd) }
fn edit(agent: Option<(&str, &str)>, p: Value, cwd: &Path) -> Value { pre(agent, "Edit", json!({"file_path": p, "old_string": "a", "new_string": "b"}), cwd) }
fn bash(agent: Option<(&str, &str)>, cmd: &str, cwd: &Path) -> Value { pre(agent, "Bash", json!({"command": cmd}), cwd) }
fn read(p: Value, e: &Env) -> Value { pre(ZC, "Read", json!({"file_path": p}), &e.repo) }
fn stop(e: &Env) -> Value { let mut v = fixture("SubagentStop-zygo-code-sub.json"); v["cwd"] = json!(e.repo); v }
fn json_out(o: HookOutput) -> Option<Value> {
    match o { HookOutput::None => None, HookOutput::Json(j) => Some(j), HookOutput::Text(t) => panic!("text output: {t}") }
}
/// The `guard` hook.
fn out(c: &HookCtx, v: &Value) -> Option<Value> { json_out(guards::run_with(&hook(v), c).unwrap()) }
/// The `work` hook (N24), which records repeat and runs claims.
fn work(c: &HookCtx, v: &Value) -> Option<Value> { json_out(hooks::run_with(&hook(v), c).unwrap()) }
fn text(e: &Env, agent: &str) -> String { std::fs::read_to_string(log::log_path(&e.sd.work(), agent)).unwrap_or_default() }
fn kinds(e: &Env, agent: &str) -> Vec<String> {
    text(e, agent).lines().map(|l| serde_json::from_str::<Value>(l).unwrap()["t"].as_str().unwrap().to_string()).collect()
}
fn guard_lines(e: &Env, agent: &str) -> Vec<(String, String)> {
    text(e, agent).lines().filter_map(|l| match serde_json::from_str::<LogLine>(l).unwrap().kind {
        Kind::Guard { guard, level } => Some((guard, level)), _ => None }).collect()
}
fn log_len(e: &Env, agent: &str) -> u64 { std::fs::metadata(log::log_path(&e.sd.work(), agent)).map(|m| m.len()).unwrap_or(0) }
fn put(e: &Env, agent: &str, kind: Kind) { log::append(&e.sd.work(), agent, &LogLine { n: 0, ts: 1000, kind }).unwrap(); }
fn start(e: &Env, agent: &str, check: &str, files: &[&str]) -> Begin {
    let offset = log_len(e, agent);
    put(e, agent, Kind::Begin { unit: U1.into(), round: 1 });
    let b = Begin { unit: U1.into(), round: 1, check: check.into(), files: files.iter().map(|f| f.to_string()).collect(), began_at: 1000,
                    offset, session: "S1".into(), agent_type: Some("zygo-code".into()), stopped_at: None, edit_root: None };
    std::fs::write(log::begin_path(&e.sd.work(), agent), serde_json::to_vec(&b).unwrap()).unwrap();
    b
}
/// ~900-byte `repeat` lines (no call, edit, check or navigated line) until the log has grown past the 256 KiB tail.
fn fill_past_tail(e: &Env, agent: &str) {
    let from = log_len(e, agent);
    while log_len(e, agent) - from <= log::TAIL_BYTES + 2048 { put(e, agent, Kind::Repeat { tool: "r".repeat(900) }); }
}
fn overrides(gs: &[(&str, &str)]) -> Overrides {
    Overrides { guards: gs.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect::<BTreeMap<_, _>>(), ..Default::default() }
}

#[test] // kills: wrong field per tool; a tool left out; the line not appended
fn redaction_denies_each_edit_tool_with_the_placeholder() {
    let e = env();
    let plain = cfg(String::new());
    let c = ctx(&e, &plain);
    let p = e.repo.join("src/lib.rs");
    for (agent, id) in [(ZC, SUB), (None, "main")] {
        for (tool, tin) in [("Write", json!({"file_path": p, "content": "k = \"[REDACTED:openai_key]\"\n"})),
                            ("edit", json!({"file_path": p, "old_string": "a", "new_string": "b [REDACTED:aws_key]"})),   // G15 case
                            ("NotebookEdit", json!({"notebook_path": e.repo.join("nb.ipynb"), "new_source": "x = '[REDACTED:jwt]'"}))] {
            assert_eq!(out(&c, &pre(agent, tool, tin, &e.repo)), Some(deny(REDACTION)), "{tool} {id}");
        }
        assert_eq!(guard_lines(&e, id), vec![g("redaction", "enforce"); 3], "{id}");
    }
}

#[test] // kills: every string field scanned; every Write denied; Bash scanned
fn redaction_allows_content_without_the_placeholder() {
    let e = env();
    let plain = cfg(String::new());
    let c = ctx(&e, &plain);
    let mut w = fixture("PreToolUse-Write-sub.json");                    // §20 #29b
    w["cwd"] = json!(e.repo);
    assert_eq!(out(&c, &w), None);
    w["tool_input"]["content"] = json!("[REDACTED:openai_key]");
    assert_eq!(out(&c, &w), Some(deny(REDACTION)));
    assert_eq!(out(&c, &pre(ZC, "Edit", json!({"file_path": "a.rs", "old_string": "[REDACTED:x]", "new_string": "y"}), &e.repo)), None);
    assert_eq!(out(&c, &bash(ZC, "echo '[REDACTED:x]' > f", &e.repo)), None);
    assert_eq!(guard_lines(&e, SUB), [g("redaction", "enforce")]);
}

#[test] // kills: redaction behind the units or config gate
fn redaction_independent_of_work_units() {
    let w = |e: &Env| pre(ZC, "Write", json!({"file_path": e.repo.join("a.txt"), "content": "[REDACTED:jwt]"}), &e.repo);
    let plain = cfg(String::new());
    let e = env();
    assert_eq!(out(&ctx(&e, &no_config), &w(&e)), Some(deny(REDACTION)));
    Overrides { work_units: Some(false), ..Default::default() }.save(&e.sd).unwrap();
    assert_eq!(out(&ctx(&e, &plain), &w(&e)), Some(deny(REDACTION)));
}

#[test] // G4, C6; kills: an append error swallowed and the deny printed; a non-string agent_id treated as the lead (mutant 9)
fn c6_log_write_failure() {
    let plain = cfg(String::new());
    let e = env();
    let mut v = pre(ZC, "Write", json!({"file_path": e.repo.join("a.txt"), "content": "[REDACTED:jwt]"}), &e.repo);
    v["agent_id"] = json!(7);
    assert!(guards::run_with(&hook(&v), &ctx(&e, &plain)).is_err());     // the work directory is valid here
    assert!(guard_lines(&e, "main").is_empty());
    v["agent_id"] = json!(SUB);
    std::fs::write(e.sd.work(), b"not a directory").unwrap();
    assert!(guards::run_with(&hook(&v), &ctx(&e, &plain)).is_err());     // guarded: no deny, one stderr line, exit 0
}

#[test] // kills: check_agent skipped (lock then errors: Err instead of Ok(None)), or reads outside work/
fn c6_invalid_agent_gives_no_event() {
    let e = env();
    let plain = cfg(String::new());
    let mut v = pre(ZC, "Write", json!({"file_path": e.repo.join("a.txt"), "content": "[REDACTED:jwt]"}), &e.repo);
    v["agent_id"] = json!("../escape");
    assert_eq!(out(&ctx(&e, &plain), &v), None);
    assert!(!e.sd.root.join("escape.jsonl").exists());
}

fn ls_head(e: &Env) -> Value { bash(None, "ls | head", &e.repo) }

#[test] // kills: quote-blind split, single (S11) or double; `||` taken as a pipe; `-n +N`, `-f` or extra args accepted
fn output_detects_the_spec_forms() {
    let e = env();
    let lv = at("output", "enforce");
    let c = ctx(&e, &lv);
    let hits = ["cargo test | tail", "cargo test 2>&1 | tail -n 20", "ls | head -5", "cat f | head -c 100", "a | b | tail -n 3",
                "x |& head", "make; ls | head -n 1 && echo ok", "echo 'a|b' | head"];
    for cmd in hits { assert_eq!(out(&c, &bash(None, cmd, &e.repo)), Some(deny(OUTPUT)), "{cmd}"); }
    for cmd in ["tail -f app.log", "x | tail -f app.log", "x | tail -n +5", "head -n 5 file", "x || tail -n 5", "x | head -n 5 file",
                "x | headless", "x | head -n", "grep '| tail'", "grep \"| head -n 3\"", "echo 'a | tail'", "echo a \\| head"] {
        assert_eq!(out(&c, &bash(None, cmd, &e.repo)), None, "{cmd}");
    }
    assert_eq!(guard_lines(&e, "main").len(), hits.len());
}

#[test] // kills: quote-blind split (the pipe sits inside the -m message)
fn output_quoted_pipe_in_commit_message_is_not_a_pipeline() {
    let e = env();
    let lv = cfg("[guards]\noutput = \"enforce\"\ntrailers = \"monitor\"\n".into());
    assert_eq!(out(&ctx(&e, &lv), &bash(None, "git commit -m \"x | head\"", &e.repo)), None);
    assert_eq!(guard_lines(&e, "main"), [g("trailers", "monitor")]);
}

#[test] // kills: notify printing permissionDecision; level not taken from the policy
fn output_levels_print_and_record() {
    for (level, want) in [("monitor", None), ("notify", Some(context(OUTPUT))), ("enforce", Some(deny(OUTPUT)))] {
        let e = env();
        let lv = at("output", level);
        assert_eq!(out(&ctx(&e, &lv), &bash(ZC, "cargo test 2>&1 | tail -n 20", &e.repo)), want, "{level}");
        assert_eq!(guard_lines(&e, SUB), [g("output", level)], "{level}");
    }
}

#[test] // kills: Overrides.results ignored; view.results ignored; store on without a view
fn output_needs_the_result_store() {
    let lv = at("output", "enforce");
    let e = env();
    view(false, false).save(&e.sd).unwrap();
    assert_eq!(out(&ctx(&e, &lv), &ls_head(&e)), None);
    Overrides { results: Some(true), ..Default::default() }.save(&e.sd).unwrap();
    assert_eq!(out(&ctx(&e, &lv), &ls_head(&e)), Some(deny(OUTPUT)));
    let e = env();
    Overrides { results: Some(false), ..Default::default() }.save(&e.sd).unwrap();
    assert_eq!(out(&ctx(&e, &lv), &ls_head(&e)), None);
    std::fs::remove_file(e.sd.view_json()).unwrap();
    std::fs::remove_file(e.sd.overrides_json()).unwrap();
    assert_eq!(out(&ctx(&e, &lv), &ls_head(&e)), None);
}

#[test] // kills: overrides.json not read (§17.1)
fn floor_session_override_applies() {
    let e = env();
    let lv = at("output", "monitor");
    overrides(&[("output", "enforce")]).save(&e.sd).unwrap();
    assert_eq!(out(&ctx(&e, &lv), &ls_head(&e)), Some(deny(OUTPUT)));
}

#[test] // kills: an override below min_level applied; config not raised to the floor
fn floor_never_below_min_level() {
    let e = env();
    let lv = cfg("[guards]\nmin_level = \"notify\"\n".into());
    overrides(&[("output", "monitor")]).save(&e.sd).unwrap();
    assert_eq!(out(&ctx(&e, &lv), &ls_head(&e)), Some(context(OUTPUT)));
    let e = env();
    let lv = cfg("[guards]\nmin_level = \"enforce\"\n".into());
    assert_eq!(out(&ctx(&e, &lv), &ls_head(&e)), Some(deny(OUTPUT)));
}

#[test] // kills: whole-set fallback (N24 R17), which drops the valid output override
fn floor_clamps_per_guard() {
    let e = env();
    let lv = cfg("[guards]\nmin_level = \"notify\"\n".into());
    overrides(&[("output", "enforce"), ("trailers", "monitor")]).save(&e.sd).unwrap();
    let c = ctx(&e, &lv);
    assert_eq!(out(&c, &ls_head(&e)), Some(deny(OUTPUT)));
    assert_eq!(out(&c, &bash(None, "git commit -m x", &e.repo)), Some(context(&trailer("<unit>"))));   // clamped to notify
}

#[test] // P-N25-1 shape; kills: first event only; notify text dropped under a deny
fn combined_deny_wins_and_texts_join() {
    let e = env();
    let lv = at("scope", "notify");
    start(&e, SUB, CHECK, &["src/lib.rs"]);
    let v = pre(ZC, "Write", json!({"file_path": e.repo.join("src/main.rs"), "content": "[REDACTED:jwt]"}), &e.repo);
    assert_eq!(out(&ctx(&e, &lv), &v), Some(deny(&format!("{REDACTION}\n{}", scope_text("src/main.rs", false)))));
    assert_eq!(guard_lines(&e, SUB), [g("redaction", "enforce"), g("scope", "notify")]);
}

#[test] // kills: commit word not required; message flag forms missed; inline text not searched
fn trailers_detects_git_commit_with_a_message() {
    let e = env();
    let lv = at("trailers", "enforce");
    let c = ctx(&e, &lv);
    for cmd in ["git commit -m \"fix: x\"", "git -C sub commit -am wip", "git commit --message=wip", "git commit --message wip",
                "git -c user.name=x commit -m y", "git commit -m \"$(cat <<'MSG'\nfix\nMSG\n)\""] {
        assert_eq!(out(&c, &bash(None, cmd, &e.repo)), Some(deny(&trailer("<unit>"))), "{cmd}");
    }
    for cmd in ["git commit", "git commit --amend --no-edit", "echo git commit -m x", "git log -m", "git commit -C HEAD",
                "git commit -m fix -m \"Zygo-Session: S1\"", "echo 'git commit -m x'"] {
        assert_eq!(out(&c, &bash(None, cmd, &e.repo)), None, "{cmd}");
    }
}

#[test] // kills: Zygo-Work not required with an open unit; required without one; <unit> not filled
fn trailers_open_unit_needs_zygo_work() {
    let e = env();
    let lv = at("trailers", "enforce");
    let c = ctx(&e, &lv);
    start(&e, SUB, CHECK, &[]);
    let commit = |a, msg: &str| bash(a, &format!("git commit -m \"{msg}\""), &e.repo);
    assert_eq!(out(&c, &commit(ZC, "fix\n\nZygo-Session: S1")), Some(deny(&trailer(U1))));
    assert_eq!(out(&c, &commit(ZC, "fix\n\nZygo-Session: S1\nZygo-Work: beads:bd-a1b2")), None);
    assert_eq!(out(&c, &commit(None, "fix\n\nZygo-Session: S1")), None);
}

#[test] // kills: no is_file check (/dev/null reads empty → event); -C ignored; a second -C not resolved against the first (S42);
          // earlier cd ignored (36); `--file=` missed (S48); bound ignored (d)
fn trailers_file_message_is_read() {
    let e = env();
    let lv = at("trailers", "enforce");
    let c = ctx(&e, &lv);
    std::fs::create_dir_all(e.repo.join("sub")).unwrap();
    std::fs::write(e.repo.join("msg.txt"), "fix\n\nZygo-Session: S1\n").unwrap();
    std::fs::write(e.repo.join("sub/msg.txt"), "fix\n").unwrap();
    std::fs::write(e.repo.join("bare.txt"), "fix\n").unwrap();
    let mut big = String::from("Zygo-Session: S1\n");
    big.push_str(&"x".repeat(log::TAIL_BYTES as usize + 10));            // the trailer lies before the last 256 KiB (G14)
    std::fs::write(e.repo.join("big.txt"), big).unwrap();
    let t = |cmd: &str| out(&c, &bash(None, cmd, &e.repo));
    for cmd in ["git commit -F msg.txt", "git commit --file=msg.txt", "git commit -F -", "git commit -F missing.txt",
                "cd sub && git commit -F msg.txt", "cd sub && git commit -F bare.txt", "git commit -F /dev/stdin", "git commit -F /dev/null"] {
        if cfg!(not(unix)) && cmd.contains("/dev/") { continue; }
        assert_eq!(t(cmd), None, "{cmd}");
    }
    for cmd in ["git commit -F bare.txt", "git commit -aF bare.txt", "git commit --file \"bare.txt\"", "git -C sub commit -F msg.txt",
                "git commit -F big.txt", "git commit --file=bare.txt", "git -C sub -C .. commit -F bare.txt"] {
        assert_eq!(t(cmd), Some(deny(&trailer("<unit>"))), "{cmd}");
    }
}

#[test] // kills: default level not notify
fn trailers_levels_print_and_record() {
    for (toml, level, want) in [("", "notify", Some(context(&trailer("<unit>")))), ("[guards]\ntrailers = \"monitor\"\n", "monitor", None),
                                ("[guards]\ntrailers = \"enforce\"\n", "enforce", Some(deny(&trailer("<unit>"))))] {
        let e = env();
        let lv = cfg(toml.into());
        assert_eq!(out(&ctx(&e, &lv), &bash(ZC, "git commit -m fix", &e.repo)), want, "{level}");
        assert_eq!(guard_lines(&e, SUB), [g("trailers", level)], "{level}");
    }
}

#[test] // kills: notify carrying the enforce suffix; level ignored
fn scope_levels_print_and_record() {
    for (level, want) in [("monitor", None), ("notify", Some(context(&scope_text("src/main.rs", false)))),
                          ("enforce", Some(deny(&scope_text("src/main.rs", true))))] {
        let e = env();
        let lv = at("scope", level);
        start(&e, SUB, CHECK, &["src/lib.rs", "docs/"]);
        assert_eq!(out(&ctx(&e, &lv), &edit(ZC, json!(e.repo.join("src/main.rs")), &e.repo)), want, "{level}");
        assert_eq!(guard_lines(&e, SUB), [g("scope", level)], "{level}");
    }
}

#[test] // kills: string-prefix match; `dir/` not a prefix; relative path not joined to cwd; notebook_path ignored (44)
fn scope_listed_paths_and_prefixes_pass() {
    let e = env();
    let lv = at("scope", "enforce");
    let c = ctx(&e, &lv);
    start(&e, SUB, CHECK, &["src/lib.rs", "docs/"]);
    for p in [json!(e.repo.join("src/lib.rs")), json!("src/lib.rs"), json!(e.repo.join("src/../src/lib.rs")), json!("docs/a/b.md")] {
        assert_eq!(out(&c, &edit(ZC, p.clone(), &e.repo)), None, "{p}");
    }
    assert_eq!(out(&c, &pre(ZC, "NotebookEdit", json!({"notebook_path": "docs/nb.ipynb", "new_source": "x"}), &e.repo)), None);
    assert_eq!(out(&c, &pre(ZC, "NotebookEdit", json!({"notebook_path": "nb.ipynb", "new_source": "x"}), &e.repo)),
               Some(deny(&scope_text("nb.ipynb", true))));
    assert_eq!(out(&c, &edit(ZC, json!("docsx/a.md"), &e.repo)), Some(deny(&scope_text("docsx/a.md", true))));
    assert_eq!(out(&c, &edit(ZC, json!("src/lib.rs.bak"), &e.repo)), Some(deny(&scope_text("src/lib.rs.bak", true))));
}

#[test] // kills: path relative to cwd's repository instead of its own
fn scope_path_is_relative_to_its_own_repository() {
    let e = env();
    let lv = at("scope", "enforce");
    start(&e, SUB, CHECK, &["src/lib.rs"]);
    let other = e.t.path().join("other");
    for d in [".git", "src"] { std::fs::create_dir_all(other.join(d)).unwrap(); }
    assert_eq!(out(&ctx(&e, &lv), &edit(ZC, json!(other.join("src/lib.rs")), &e.repo)), None);
    assert_eq!(out(&ctx(&e, &lv), &edit(ZC, json!(other.join("README.md")), &e.repo)), Some(deny(&scope_text("README.md", true))));
}

#[cfg(unix)]
#[test] // kills: lexical normalisation before canonicalize (link/../x); symlink judged by its own name
fn scope_symlink_is_judged_by_its_target() {
    let e = env();
    let lv = at("scope", "enforce");
    let c = ctx(&e, &lv);
    start(&e, SUB, CHECK, &["x.rs", "lib.rs"]);
    let other = e.t.path().join("other");
    for d in [".git", "deep/src"] { std::fs::create_dir_all(other.join(d)).unwrap(); }
    std::os::unix::fs::symlink(other.join("deep/src"), e.repo.join("link")).unwrap();
    assert_eq!(out(&c, &edit(ZC, json!("link/../x.rs"), &e.repo)), Some(deny(&scope_text("deep/x.rs", true))));   // lexical: x.rs, listed
    assert_eq!(out(&c, &edit(ZC, json!("link/lib.rs"), &e.repo)), Some(deny(&scope_text("deep/src/lib.rs", true))));
    std::os::unix::fs::symlink(&e.scratch, e.repo.join("tlink")).unwrap();
    assert_eq!(out(&c, &edit(ZC, json!("tlink/y.rs"), &e.repo)), None);   // its target is a temp root (G9)
}

#[test] // kills: the context's temp roots not read; a path under a root judged (G9)
fn scope_temp_paths_never_count() {
    let e = env();
    let lv = at("scope", "enforce");
    start(&e, SUB, CHECK, &["src/lib.rs"]);
    let in_repo = e.scratch.join("w");                                    // a repository inside the temp root
    std::fs::create_dir_all(in_repo.join(".git")).unwrap();
    assert_eq!(out(&ctx(&e, &lv), &edit(ZC, json!(in_repo.join("x.rs")), &e.repo)), None);
    assert!(guard_lines(&e, SUB).is_empty());
}

#[test] // G9 production list; kills: temp_dir left out; /tmp left out; /var/tmp left out (S40); roots not canonicalised
fn scope_default_temp_roots() {
    let e = env();
    let real = e.t.path().join("real-tmp");
    std::fs::create_dir_all(&real).unwrap();
    #[cfg(unix)]
    let given = {
        let l = e.t.path().join("tmp-link");                              // a non-canonical spelling of the temp dir
        std::os::unix::fs::symlink(&real, &l).unwrap();
        l
    };
    #[cfg(not(unix))]
    let given = real.clone();
    let mut want = vec![real.canonicalize().unwrap()];
    if cfg!(unix) {
        want.extend(["/tmp", "/var/tmp"].map(|r| Path::new(r).canonicalize().unwrap()));
    }
    assert_eq!(guards::temp_roots(&given), want);
}

#[cfg(unix)]
#[test] // G9 through the production list; kills: /var/tmp left out (S40)
fn scope_var_tmp_is_exempt() {
    let e = env();
    let lv = at("scope", "enforce");
    start(&e, SUB, CHECK, &["src/lib.rs"]);
    let var_tmp = tempfile::Builder::new().tempdir_in("/var/tmp").unwrap();
    std::fs::create_dir_all(var_tmp.path().join(".git")).unwrap();
    let c = HookCtx { temp_roots: guards::temp_roots(&e.scratch), ..ctx(&e, &lv) };
    assert_eq!(out(&c, &edit(ZC, json!(var_tmp.path().join("x.rs")), &e.repo)), None);
    assert!(guard_lines(&e, SUB).is_empty());
}

#[cfg(unix)]
#[test] // G9; kills: no repository root treated as exempt
fn scope_path_without_repository_root() {
    let e = env();
    let lv = at("scope", "enforce");
    start(&e, SUB, CHECK, &["src/lib.rs"]);
    let p = "/zygo-n25-no-such-dir/x.rs";                                 // nearest existing ancestor `/`: no `.git`
    assert_eq!(out(&ctx(&e, &lv), &edit(ZC, json!(p), &e.repo)), Some(deny(&scope_text(p, true))));
}

#[test] // kills: the lead judged; no-unit or empty-files units judged
fn scope_needs_a_subagent_with_an_open_unit_listing_files() {
    let e = env();
    let lv = at("scope", "enforce");
    let c = ctx(&e, &lv);
    let main_rs = json!(e.repo.join("src/main.rs"));
    assert_eq!(out(&c, &edit(ZC, main_rs.clone(), &e.repo)), None);
    start(&e, "main", CHECK, &["src/lib.rs"]);
    assert_eq!(out(&c, &edit(None, main_rs.clone(), &e.repo)), None);
    start(&e, SUB, CHECK, &[]);
    assert_eq!(out(&c, &edit(ZC, main_rs, &e.repo)), None);
    assert!(guard_lines(&e, SUB).is_empty() && guard_lines(&e, "main").is_empty());
}

fn source(e: &Env, rel: &str, lines: usize) -> PathBuf {
    let p = rel.split('/').fold(e.repo.clone(), |p, c| p.join(c)); // native separators, as the guard prints them
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, "fn f() {}\n".repeat(lines)).unwrap();
    p
}
fn navigated(e: &Env, agent: &str, p: &Path) { put(e, agent, Kind::Navigated { paths: vec![p.to_str().unwrap().into()] }); }

#[test] // kills: level ignored; relative path in the text
fn nav_levels_print_and_record() {
    for level in ["monitor", "notify", "enforce"] {
        let e = nav_env();
        let big = source(&e, "src/big.rs", 401);
        let want = match level { "monitor" => None, "notify" => Some(context(&nav_text(&big))), _ => Some(deny(&nav_text(&big))) };
        let lv = at("navigation", level);
        assert_eq!(out(&ctx(&e, &lv), &read(json!("src/big.rs"), &e)), want, "{level}");
        assert_eq!(guard_lines(&e, SUB), [g("navigation", level)], "{level}");
    }
}

#[test] // kills: `>=` for `>`; unterminated last line not counted; limit ignored; offset taken as limit; file_names or extensions ignored; config lines ignored
fn nav_source_file_and_line_threshold() {
    let e = nav_env();
    let lv = at("navigation", "enforce");
    let c = ctx(&e, &lv);
    assert_eq!(out(&c, &read(json!(source(&e, "src/a.rs", 400)), &e)), None);
    let p = source(&e, "src/b.rs", 0);
    std::fs::write(&p, format!("{}fn tail() {{}}", "fn f() {}\n".repeat(400))).unwrap();
    assert_eq!(out(&c, &read(json!(p), &e)), Some(deny(&nav_text(&p))));
    assert_eq!(out(&c, &pre(ZC, "Read", json!({"file_path": p, "limit": 2000}), &e.repo)), None);
    assert_eq!(out(&c, &pre(ZC, "Read", json!({"file_path": p, "offset": 10}), &e.repo)), Some(deny(&nav_text(&p))));
    for (rel, hit) in [("notes.txt", false), ("Makefile", true), ("CMakeLists.txt", true), ("tsconfig.base.json", true), ("x.R", true)] {
        assert_eq!(out(&c, &read(json!(source(&e, rel, 500)), &e)).is_some(), hit, "{rel}");
    }
    let ten = cfg("[guards]\nnavigation = \"enforce\"\n[navigation]\nlines = 10\n".into());
    assert_eq!(out(&ctx(&e, &ten), &read(json!(source(&e, "src/ten.rs", 10)), &e)), None);
    assert!(out(&ctx(&e, &ten), &read(json!(source(&e, "src/eleven.rs", 11)), &e)).is_some());
}

#[cfg(unix)]
#[test] // G10; kills: no is_file check (/dev/urandom yields many lines); a file over 256 KiB counted line by line (one line → not over)
fn nav_reads_regular_files_within_the_bound() {
    let e = nav_env();
    let lv = at("navigation", "enforce");
    std::os::unix::fs::symlink("/dev/urandom", e.repo.join("src/rand.rs")).unwrap();
    assert_eq!(out(&ctx(&e, &lv), &read(json!("src/rand.rs"), &e)), None);
    let one_line = e.repo.join("src/min.rs");
    std::fs::write(&one_line, "x".repeat(log::TAIL_BYTES as usize + 1)).unwrap();
    assert_eq!(out(&ctx(&e, &lv), &read(json!(one_line), &e)), Some(deny(&nav_text(&one_line))));
}

#[test] // kills: SessionView.navigation ignored; present without a view
fn nav_needs_navigation_present() {
    let lv = at("navigation", "enforce");
    let e = env();
    let big = source(&e, "src/big.rs", 401);
    assert_eq!(out(&ctx(&e, &lv), &read(json!(big), &e)), None);
    std::fs::remove_file(e.sd.view_json()).unwrap();
    assert_eq!(out(&ctx(&e, &lv), &read(json!(big), &e)), None);
}

#[test] // kills: string-prefix match; other agents' history counted; N24's recorded form not matched
fn nav_navigated_path_or_parent_dir_passes() {
    let lv = at("navigation", "enforce");
    for (nav, pass) in [("src/big.rs", true), ("src", true), ("", true), ("src/bi", false), ("src/big.rs.d", false)] {
        let e = nav_env();
        let big = source(&e, "src/big.rs", 401);
        navigated(&e, SUB, &e.repo.join(nav));
        assert_eq!(out(&ctx(&e, &lv), &read(json!(big), &e)).is_none(), pass, "{nav}");
    }
    let e = nav_env();
    let big = source(&e, "src/big.rs", 401);
    navigated(&e, "main", &big);
    assert_eq!(out(&ctx(&e, &lv), &read(json!(big), &e)), Some(deny(&nav_text(&big))));
    work(&ctx(&e, &lv), &post(ZC, "mcp__act-lite__skeleton", json!({"path": "src/big.rs"}), &e.repo));
    assert_eq!(out(&ctx(&e, &lv), &read(json!("src/big.rs"), &e)), None);
}

#[test] // kills: lines before the unit's begin counted
fn nav_history_is_the_open_unit_or_the_session() {
    let e = nav_env();
    let lv = at("navigation", "enforce");
    let c = ctx(&e, &lv);
    let big = source(&e, "src/big.rs", 401);
    navigated(&e, SUB, &big);
    assert_eq!(out(&c, &read(json!(big), &e)), None);
    start(&e, SUB, CHECK, &[]);
    assert_eq!(out(&c, &read(json!(big), &e)), Some(deny(&nav_text(&big))));
    navigated(&e, SUB, &big);
    assert_eq!(out(&c, &read(json!(big), &e)), None);
}

#[test] // kills: window ≠ 256 paths
fn nav_looks_back_over_256_paths() {
    let lv = at("navigation", "enforce");
    for (others, pass) in [(255, true), (256, false)] {
        let e = nav_env();
        let big = source(&e, "src/big.rs", 401);
        navigated(&e, SUB, &big);
        for i in 0..others { navigated(&e, SUB, &e.repo.join(format!("o{i}.rs"))); }
        assert_eq!(out(&ctx(&e, &lv), &read(json!(big), &e)).is_none(), pass, "{others}");
    }
}

#[test] // kills: incomplete history treated as complete; overflow ignored
fn nav_fails_open_beyond_256_kib_and_after_overflow() {
    let lv = at("navigation", "enforce");
    let e = nav_env();
    let big = source(&e, "src/big.rs", 401);
    navigated(&e, SUB, &big);
    fill_past_tail(&e, SUB);
    assert_eq!(out(&ctx(&e, &lv), &read(json!(big), &e)), None);
    start(&e, SUB, CHECK, &[]);
    assert_eq!(out(&ctx(&e, &lv), &read(json!(big), &e)), Some(deny(&nav_text(&big))));   // the unit's history is whole
    let e = nav_env();
    let big = source(&e, "src/big.rs", 401);
    for _ in 0..log::MAX_LINES { put(&e, SUB, Kind::Pushed); }            // line 4,000 is `overflow`
    assert_eq!(out(&ctx(&e, &lv), &read(json!(big), &e)), None);
}

fn rows(e: &Env) -> Vec<zygo_usage::WorkUnitRow> { Store::open(&e.t.path().join("usage.db")).unwrap().work_units(&Filter::default()).unwrap() }

#[test] // kills: SubagentStop notify printing a block; level ignored
fn claims_levels_print_and_record() {
    for (level, want) in [("monitor", None), ("enforce", Some(block())),
                          ("notify", Some(json!({"hookSpecificOutput": {"hookEventName": "SubagentStop", "additionalContext": CLAIMS_NOTIFY}})))] {
        let e = env();
        let lv = at("claims", level);
        start(&e, ZSUB, CHECK, &[]);
        assert_eq!(work(&ctx(&e, &lv), &stop(&e)), want, "{level}");
        assert_eq!(guard_lines(&e, ZSUB), [g("claims", level)], "{level}");
    }
}

#[test] // kills: any check (failed included) clearing an edit; a check before the last edit counted
fn claims_event_when_no_pass_after_the_last_edit() {
    let (edit_, pass, fail) = (|| Kind::Edit { root: None }, || Kind::Check { result: CheckResult::Passed }, || Kind::Check { result: CheckResult::Failed });
    let lv = at("claims", "enforce");
    for (seq, event) in [(vec![], true), (vec![edit_()], true), (vec![edit_(), pass()], false), (vec![pass(), edit_()], true),
                         (vec![edit_(), fail()], true), (vec![pass(), edit_(), fail()], true), (vec![fail()], false), (vec![pass()], false)] {
        let e = env();
        start(&e, ZSUB, CHECK, &[]);
        let label = format!("{seq:?}");
        for k in seq { put(&e, ZSUB, k); }
        assert_eq!(work(&ctx(&e, &lv), &stop(&e)), event.then(block), "{label}");
    }
}

#[test] // kills: blocking twice; "once" read from stop_hook_active
fn claims_continues_once() {
    let lv = at("claims", "enforce");
    let e = env();
    start(&e, ZSUB, CHECK, &[]);
    let mut active = stop(&e);
    active["stop_hook_active"] = json!(true);
    assert_eq!(work(&ctx(&e, &lv), &active), Some(block()));             // no claims line yet: the log decides (G6)
    assert_eq!(work(&ctx(&e, &lv), &stop(&e)), None);
    assert_eq!(guard_lines(&e, ZSUB), [g("claims", "enforce"), g("claims", "enforce")]);
}

#[test] // kills: blocking when the enforce line was not written
fn claims_log_full_enforce_gives_no_block() {
    let e = env();
    let lv = at("claims", "enforce");
    start(&e, ZSUB, CHECK, &[]);
    for _ in 0..log::MAX_LINES - 2 { put(&e, ZSUB, Kind::Pushed); }      // 3,999 lines: the guard line would be line 4,000
    assert_eq!(work(&ctx(&e, &lv), &stop(&e)), None);
    assert_eq!(kinds(&e, ZSUB).last().map(String::as_str), Some("overflow"));
}

#[test] // kills: claims outside the zygo-/open unit/non-empty check scope
fn claims_needs_a_zygo_subagent_with_an_open_unit_and_a_check() {
    let lv = at("claims", "enforce");
    let e = env();
    start(&e, ZSUB, "", &[]);
    assert_eq!(work(&ctx(&e, &lv), &stop(&e)), None);
    let e = env();
    assert_eq!(work(&ctx(&e, &lv), &stop(&e)), None);
    start(&e, ZSUB, CHECK, &[]);
    let mut other = stop(&e);
    other["agent_type"] = json!("claude");
    assert_eq!(work(&ctx(&e, &lv), &other), None);
    assert!(guard_lines(&e, ZSUB).is_empty());
}

#[test] // kills: lines before the begin counted; a partial unit judged; overflow ignored (§13.4 note 2026-11-19)
fn claims_fires_only_on_the_whole_unit() {
    let lv = at("claims", "enforce");
    let e = env();
    put(&e, ZSUB, Kind::Check { result: CheckResult::Passed });
    start(&e, ZSUB, CHECK, &[]);
    assert_eq!(work(&ctx(&e, &lv), &stop(&e)), Some(block()));
    let e = env();
    start(&e, ZSUB, CHECK, &[]);
    fill_past_tail(&e, ZSUB);
    put(&e, ZSUB, Kind::Edit { root: None });
    assert_eq!(work(&ctx(&e, &lv), &stop(&e)), None);                   // begin outside the window
    let e = env();
    start(&e, ZSUB, CHECK, &[]);
    for _ in 0..log::MAX_LINES { put(&e, ZSUB, Kind::Pushed); }
    assert_eq!(work(&ctx(&e, &lv), &stop(&e)), None);                   // overflow
}

#[test] // kills: claims line appended after the fold, or by another hook (H3)
fn claims_event_is_in_the_folded_row() {
    let e = env();
    let lv = at("claims", "enforce");
    start(&e, ZSUB, CHECK, &[]);
    assert_eq!(work(&ctx(&e, &lv), &stop(&e)), Some(block()));
    let r = rows(&e);
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].guard_events, BTreeMap::from([("claims".to_string(), BTreeMap::from([("enforce".to_string(), 1)]))]));
}

#[test] // kills: the units gate skipped for the configurable guards
fn units_off_runs_only_redaction() {
    let e = env();
    let lv = cfg("[guards]\noutput = \"enforce\"\ntrailers = \"enforce\"\nclaims = \"enforce\"\nscope = \"enforce\"\n".into());
    let c = ctx(&e, &lv);
    Overrides { work_units: Some(false), ..Default::default() }.save(&e.sd).unwrap();
    assert_eq!(out(&c, &bash(None, "git commit -m x | head", &e.repo)), None);
    start(&e, ZSUB, CHECK, &[]);
    assert_eq!(work(&c, &stop(&e)), None);
    start(&e, SUB, CHECK, &["src/lib.rs"]);
    assert_eq!(out(&c, &edit(ZC, json!(e.repo.join("src/main.rs")), &e.repo)), None);
    assert_eq!(out(&c, &pre(None, "Write", json!({"file_path": "a.txt", "content": "[REDACTED:jwt]"}), &e.repo)), Some(deny(REDACTION)));
    assert_eq!(guard_lines(&e, "main"), [g("redaction", "enforce")]);
    assert!(guard_lines(&e, ZSUB).is_empty() && guard_lines(&e, SUB).is_empty());
}

#[test] // kills: no guard line with the repeat fact; level read from overrides
fn repeat_records_a_monitor_event_with_the_repeat_fact() {
    let e = env();
    let lv = cfg(String::new());
    overrides(&[("repeat", "enforce")]).save(&e.sd).unwrap();
    let r = post(None, "Read", json!({"file_path": "/r/x.rs"}), &e.repo);
    assert_eq!(work(&ctx(&e, &lv), &r), None);
    assert_eq!(work(&ctx(&e, &lv), &r), None);
    assert_eq!(kinds(&e, "main"), ["call", "call", "repeat", "guard"]);
    assert_eq!(guard_lines(&e, "main"), [g("repeat", "monitor")]);
}

#[test] // kills: guard line decided outside N24's 256-call/256-KiB look-back
fn repeat_window_is_256_calls_within_256_kib() {
    let lv = cfg(String::new());
    let r = |e: &Env| post(None, "Read", json!({"file_path": "/r/a.rs"}), &e.repo);
    for (between, want) in [(log::REPEAT_WINDOW - 1, 1), (log::REPEAT_WINDOW, 0)] {
        let e = env();
        work(&ctx(&e, &lv), &r(&e));
        for i in 0..between { put(&e, "main", Kind::Call { tool: "Read".into(), h: format!("{i:016x}") }); }
        work(&ctx(&e, &lv), &r(&e));
        assert_eq!(guard_lines(&e, "main").len(), want, "{between}");
    }
    let e = env();
    work(&ctx(&e, &lv), &r(&e));
    fill_past_tail(&e, "main");
    work(&ctx(&e, &lv), &r(&e));
    assert!(guard_lines(&e, "main").is_empty());
}

// ---- Round 2 ----

/// The deny reason of an output, for texts whose absolute part depends on the checkout's place.
fn reason(o: Option<Value>) -> String { o.and_then(|v| v["hookSpecificOutput"]["permissionDecisionReason"].as_str().map(String::from)).unwrap_or_default() }

#[test] // Round 2 fix 1; kills: the missing tail joined without lexical normalisation (`newdir/` prefix matched; temp root kept)
fn scope_missing_tail_is_normalised() {
    let e = env();
    let lv = at("scope", "enforce");
    let c = ctx(&e, &lv);
    start(&e, SUB, CHECK, &["newdir/", "src/lib.rs"]);
    for p in [json!(e.repo.join("newdir/../src/main.rs")), json!("newdir/../src/main.rs")] {
        assert_eq!(out(&c, &edit(ZC, p.clone(), &e.repo)), Some(deny(&scope_text("src/main.rs", true))), "{p}");
    }
    for p in [json!(e.repo.join("newdir/../../outside.rs")), json!("newdir/../../outside.rs")] {
        let r = reason(out(&c, &edit(ZC, p.clone(), &e.repo)));
        assert!(r.contains("outside.rs` is not in this unit's files") && !r.contains("newdir"), "{p}: {r}");
    }
    let via_root = e.scratch.join("missing/../../repo dir/src/main.rs");   // leaves the temp root through a missing dir
    assert_eq!(out(&c, &edit(ZC, json!(via_root), &e.repo)), Some(deny(&scope_text("src/main.rs", true))));
    assert_eq!(guard_lines(&e, SUB).len(), 5);
    #[cfg(unix)] {
        let via_tmp = format!("/tmp/zygo-n25-missing/../..{}", e.repo.join("src/main.rs").display());
        let c = HookCtx { temp_roots: guards::temp_roots(&e.scratch), ..ctx(&e, &lv) };
        assert!(reason(out(&c, &edit(ZC, json!(via_tmp), &e.repo))).contains("main.rs` is not in this unit's files"));
    }
}

#[cfg(unix)]
#[test] // Round 2 fixes 1, 2 (G8); kills: an existing file judged by its own name, not its target; no second resolve after `..`
fn scope_symlinked_file_is_judged_by_its_target() {
    let e = env();
    let lv = at("scope", "enforce");
    start(&e, SUB, CHECK, &["src/lib.rs"]);
    let other = e.t.path().join("other");
    for d in [".git", "src"] { std::fs::create_dir_all(other.join(d)).unwrap(); }
    std::fs::write(other.join("src/secret.rs"), "s").unwrap();
    std::os::unix::fs::symlink(other.join("src/secret.rs"), e.repo.join("src/lib.rs")).unwrap();
    assert_eq!(out(&ctx(&e, &lv), &edit(ZC, json!("src/lib.rs"), &e.repo)), Some(deny(&scope_text("src/secret.rs", true))));
    // `..` in the missing tail leads back through a symlink: resolved again (kills: the normalised path not re-resolved).
    std::os::unix::fs::symlink(other.join("src"), e.repo.join("link")).unwrap();
    assert_eq!(out(&ctx(&e, &lv), &edit(ZC, json!("newdir/../link/x.rs"), &e.repo)), Some(deny(&scope_text("src/x.rs", true))));
}

#[cfg(target_os = "macos")]
#[test] // Round 2 fix 2, defect 8 (needs a case-insensitive file system); kills: the file name's case taken from the input
fn scope_existing_file_case_follows_the_file_system() {
    let e = env();
    let lv = at("scope", "enforce");
    start(&e, SUB, CHECK, &["src/lib.rs"]);
    std::fs::write(e.repo.join("src/lib.rs"), "x").unwrap();
    assert_eq!(out(&ctx(&e, &lv), &edit(ZC, json!(e.repo.join("SRC/LIB.RS")), &e.repo)), None);
}

#[test] // Round 3 fix 4 (C6 line 56); kills: a fold error after the claims line printing the block (round-2 fix 3), or two stderr lines
fn claims_fold_error_prints_nothing_and_one_stderr_line() {
    let e = env();
    let lv = at("claims", "enforce");
    start(&e, ZSUB, CHECK, &[]);
    std::fs::create_dir_all(e.t.path().join("usage.db")).unwrap();      // Store::open fails
    let warned = std::cell::RefCell::new(Vec::<String>::new());
    let sink = |s: &str| warned.borrow_mut().push(s.to_string());
    let c = HookCtx { warn: &sink, ..ctx(&e, &lv) };
    assert!(hooks::run_with(&hook(&stop(&e)), &c).is_err());             // guarded: no block, its one stderr line, exit 0
    assert!(warned.borrow().is_empty(), "{:?}", warned.borrow());
    assert_eq!(guard_lines(&e, ZSUB), [g("claims", "enforce")]);         // known limit: the "once" is spent
}

#[cfg(unix)]
#[test] // Round 3 fix 4; kills: a claims error alone propagated (the fold then not run) or printing a decision
fn claims_error_alone_is_one_warn_line_and_the_fold_runs() {
    use std::os::unix::fs::PermissionsExt;
    let e = env();
    let lv = at("claims", "enforce");
    start(&e, ZSUB, CHECK, &[]);
    let log_file = log::log_path(&e.sd.work(), ZSUB);
    std::fs::set_permissions(&log_file, std::fs::Permissions::from_mode(0o444)).unwrap();   // the claims line cannot be appended
    let warned = std::cell::RefCell::new(Vec::<String>::new());
    let sink = |s: &str| warned.borrow_mut().push(s.to_string());
    let c = HookCtx { warn: &sink, ..ctx(&e, &lv) };
    let r = hooks::run_with(&hook(&stop(&e)), &c).map(json_out);
    std::fs::set_permissions(&log_file, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(r.unwrap(), None);
    assert_eq!(warned.borrow().len(), 1, "{:?}", warned.borrow());
    assert_eq!(rows(&e).len(), 1);
}

#[cfg(unix)]
#[test] // Round 3 fix 4; kills: a claims error and a fold error each writing a stderr line
fn claims_error_and_fold_error_give_one_stderr_line() {
    use std::os::unix::fs::PermissionsExt;
    let e = env();
    let lv = at("claims", "enforce");
    start(&e, ZSUB, CHECK, &[]);
    let log_file = log::log_path(&e.sd.work(), ZSUB);
    std::fs::set_permissions(&log_file, std::fs::Permissions::from_mode(0o444)).unwrap();   // the claims line cannot be appended
    std::fs::create_dir_all(e.t.path().join("usage.db")).unwrap();      // and the fold fails
    let warned = std::cell::RefCell::new(Vec::<String>::new());
    let sink = |s: &str| warned.borrow_mut().push(s.to_string());
    let c = HookCtx { warn: &sink, ..ctx(&e, &lv) };
    let r = hooks::run_with(&hook(&stop(&e)), &c);
    std::fs::set_permissions(&log_file, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(r.is_err());
    assert!(warned.borrow().is_empty(), "{:?}", warned.borrow());
    assert!(guard_lines(&e, ZSUB).is_empty());
}

#[test] // Round 2 fix 4; kills: hooks::levels whole-set fallback (S43); the recorded and the applied levels differing
fn floor_begin_levels_line_matches_the_applied_levels() {
    let e = env();
    let lv = cfg("[guards]\nmin_level = \"notify\"\n".into());
    overrides(&[("output", "enforce"), ("trailers", "monitor")]).save(&e.sd).unwrap();   // one valid, one below min_level
    let c = ctx(&e, &lv);
    let mut b = pre(ZC, "mcp__zygo__work_begin", json!({"unit": U1, "round": 1, "check": CHECK, "files": ["src/"]}), &e.repo);
    b["hook_event_name"] = json!("PreToolUse");
    let mut i = hook(&b);
    i.cli_event = Some("begin".into());
    assert!(json_out(hooks::run_with(&i, &c).unwrap()).is_none());
    let levels = text(&e, SUB).lines().find_map(|l| match serde_json::from_str::<LogLine>(l).unwrap().kind {
        Kind::Levels { levels } => Some(levels), _ => None }).expect("levels line");
    assert_eq!((levels["output"].as_str(), levels["trailers"].as_str()), ("enforce", "notify"));
    assert_eq!(out(&c, &bash(ZC, "ls | head", &e.repo)), Some(deny(OUTPUT)));                        // applied: enforce
    assert_eq!(out(&c, &bash(None, "git commit -m x", &e.repo)), Some(context(&trailer("<unit>"))));  // applied: notify
}

#[test] // Round 2 test gap S41; kills: an override below min_level refused instead of clamped (config level kept)
fn floor_override_below_min_level_is_clamped() {
    let e = env();
    let lv = cfg("[guards]\nmin_level = \"notify\"\noutput = \"enforce\"\n".into());
    overrides(&[("output", "monitor")]).save(&e.sd).unwrap();
    assert_eq!(out(&ctx(&e, &lv), &ls_head(&e)), Some(context(OUTPUT)));
}

#[test] // Round 2 fix 5; kills: attached values, leading assignments or git global options missed; a second -C from cwd (S42)
fn trailers_attached_values_assignments_and_global_options() {
    let e = env();
    let lv = at("trailers", "enforce");
    let c = ctx(&e, &lv);
    std::fs::create_dir_all(e.repo.join("sub")).unwrap();
    std::fs::write(e.repo.join("m.txt"), "fix\n").unwrap();
    std::fs::write(e.repo.join("ok.txt"), "fix\n\nZygo-Session: S1\n").unwrap();
    for cmd in ["git commit -mfix", "git commit -am\"fix\"", "git commit -Fm.txt", "git commit -aFm.txt", "git commit --message=x",
                "git commit --file=m.txt", "GIT_AUTHOR_NAME=x git commit -m fix", "A=1 B_2=y git commit -m fix",
                "git -C sub -C .. commit -F m.txt", "git -c k=v commit -m fix", "git --git-dir .git commit -m fix",
                "git --git-dir=.git commit -m fix", "git --work-tree . commit -m fix", "git --work-tree=. commit -m fix",
                "git --no-pager commit -m fix", "git -p --bare commit -m fix", "git --namespace n commit -m fix"] {
        assert_eq!(out(&c, &bash(None, cmd, &e.repo)), Some(deny(&trailer("<unit>"))), "{cmd}");
    }
    for cmd in ["git commit -Fok.txt", "git commit -C HEAD", "git commit -mfix -m\"Zygo-Session: S1\"", "X=1 echo git commit -m x",
                "git --git-dir .git log -m"] {
        assert_eq!(out(&c, &bash(None, cmd, &e.repo)), None, "{cmd}");
    }
}

#[test] // Round 2 fix 6 (defect 6); kills: claims notify printed when its line was not written (log full, or overflow: 87)
fn claims_notify_needs_its_line_written() {
    let lv = at("claims", "notify");
    let e = env();
    start(&e, ZSUB, CHECK, &[]);
    for _ in 0..log::MAX_LINES - 2 { put(&e, ZSUB, Kind::Pushed); }      // the guard line would be line 4,000
    assert_eq!(work(&ctx(&e, &lv), &stop(&e)), None);
    assert_eq!(kinds(&e, ZSUB).last().map(String::as_str), Some("overflow"));
    let e = env();
    start(&e, ZSUB, CHECK, &[]);
    for _ in 0..log::MAX_LINES { put(&e, ZSUB, Kind::Pushed); }          // overflow already written
    assert_eq!(work(&ctx(&e, &lv), &stop(&e)), None);
}

#[test] // Round 3 fix 1 (corrects round-2 fix 6); kills: a full log (`append` → `Ok(None)`) dropping a scope or redaction decision
fn c6_full_log_keeps_the_decision() {
    let lv = at("scope", "enforce");
    let e = env();
    start(&e, SUB, CHECK, &["src/lib.rs"]);
    for _ in 0..log::MAX_LINES + 1 { put(&e, SUB, Kind::Pushed); }
    assert_eq!(out(&ctx(&e, &lv), &edit(ZC, json!(e.repo.join("src/main.rs")), &e.repo)), Some(deny(&scope_text("src/main.rs", true))));
    assert!(guard_lines(&e, SUB).is_empty());
    let e = env();
    start(&e, SUB, CHECK, &["src/lib.rs"]);
    for _ in 0..log::MAX_LINES - 3 { put(&e, SUB, Kind::Pushed); }       // room for one guard line before `overflow`
    let v = pre(ZC, "Write", json!({"file_path": e.repo.join("src/main.rs"), "content": "[REDACTED:jwt]"}), &e.repo);
    assert_eq!(out(&ctx(&e, &lv), &v), Some(deny(&format!("{REDACTION}\n{}", scope_text("src/main.rs", true)))));
    assert_eq!(guard_lines(&e, SUB), [g("redaction", "enforce")]);
    assert_eq!(kinds(&e, SUB).last().map(String::as_str), Some("overflow"));
}

#[test] // Round 3 fix 1; kills: redaction dropped on a full log, for the lead or for a subagent with work units off
fn c6_full_log_still_denies_redaction() {
    let e = env();
    let plain = cfg(String::new());
    for _ in 0..log::MAX_LINES { put(&e, "main", Kind::Pushed); }
    let v = pre(None, "Write", json!({"file_path": e.repo.join("a.txt"), "content": "[REDACTED:jwt]"}), &e.repo);
    assert_eq!(out(&ctx(&e, &plain), &v), Some(deny(REDACTION)));
    let e = env();
    for _ in 0..log::MAX_LINES { put(&e, SUB, Kind::Pushed); }
    let v = pre(ZC, "Write", json!({"file_path": e.repo.join("a.txt"), "content": "[REDACTED:jwt]"}), &e.repo);
    assert_eq!(out(&ctx(&e, &no_config), &v), Some(deny(REDACTION)));
    assert_eq!(kinds(&e, SUB).len(), log::MAX_LINES as usize);
}

#[test] // Round 3 fix 1; kills: navigation printing on a full log
fn c6_full_log_navigation_prints_nothing() {
    let lv = at("navigation", "enforce");
    let e = nav_env();
    let big = source(&e, "src/big.rs", 401);
    for _ in 0..log::MAX_LINES + 1 { put(&e, SUB, Kind::Pushed); }
    assert_eq!(out(&ctx(&e, &lv), &read(json!(big), &e)), None);
    // Full without an `overflow` line (written by hand: line 4,000 is `pushed`), so only `append` → `Ok(None)` decides (N1).
    let e = nav_env();
    let big = source(&e, "src/big.rs", 401);
    let lines: String = (1..=log::MAX_LINES).map(|n| format!("{{\"n\":{n},\"ts\":1000,\"t\":\"pushed\"}}\n")).collect();
    std::fs::create_dir_all(e.sd.work()).unwrap();
    std::fs::write(log::log_path(&e.sd.work(), SUB), lines).unwrap();
    assert_eq!(out(&ctx(&e, &lv), &read(json!(big), &e)), None);
}

#[test] // Round 2 fix 7; kills: comments, heredoc bodies or a newline after `|` not handled by the tokenizer
fn output_comments_heredocs_and_continued_pipes() {
    let e = env();
    let lv = cfg("[guards]\noutput = \"enforce\"\ntrailers = \"enforce\"\n".into());
    let c = ctx(&e, &lv);
    for cmd in ["ls # | head", "ls #x\n# | tail", "cat <<EOF\na | head\nEOF", "cat <<-EOF\n\ta | head\n\tEOF", "cat <<'EOF'\na | head\nEOF",
                "cat << \"EOF\"\na | head\nEOF", "cat <<A <<B\n| head\nA\n| tail\nB", "git commit -F - <<EOF\nfix\nx | head\nEOF",
                "echo a#b | head -n x"] {
        assert_eq!(out(&c, &bash(None, cmd, &e.repo)), None, "{cmd}");
    }
    for cmd in ["ls |\nhead", "ls |\n\n  tail -n 3", "cat <<EOF\nbody\nEOF\nls | head", "cat <<-EOF\n\tbody\n\tEOF\nls | head", "cat <<< x | head", "echo '<<EOF'\nls | head"] {
        assert_eq!(out(&c, &bash(None, cmd, &e.repo)), Some(deny(OUTPUT)), "{cmd}");
    }
}

// ---- Round 3 ----

#[test] // Round 3 fix 1, mutant 87; kills: the claims overflow check removed (an `overflow` line with room left to append)
fn claims_overflow_line_fires_nothing() {
    let lv = at("claims", "enforce");
    let e = env();
    start(&e, ZSUB, CHECK, &[]);
    put(&e, ZSUB, Kind::Edit { root: None });
    // A log that holds an `overflow` line though its last `n` leaves room: written by hand, as no writer makes it.
    let mut f = std::fs::OpenOptions::new().append(true).open(log::log_path(&e.sd.work(), ZSUB)).unwrap();
    std::io::Write::write_all(&mut f, b"{\"n\":3,\"ts\":1000,\"t\":\"overflow\"}\n").unwrap();
    assert_eq!(work(&ctx(&e, &lv), &stop(&e)), None);
    assert!(guard_lines(&e, ZSUB).is_empty());
}

#[cfg(unix)]
#[test] // Round 3 fix 2; kills: `..` applied lexically after a symlink that exists (missing or existing first part)
fn scope_dotdot_after_a_symlink_is_physical() {
    let e = env();
    let lv = at("scope", "enforce");
    start(&e, SUB, CHECK, &["src/lib.rs"]);
    std::fs::write(e.repo.join("src/lib.rs"), "x").unwrap();
    let other = e.t.path().join("other");
    std::fs::create_dir_all(other.join(".git")).unwrap();
    std::fs::create_dir_all(other.join("a/b")).unwrap();
    std::os::unix::fs::symlink(other.join("a/b"), e.repo.join("l")).unwrap();
    // `l/..` is other/a: the write lands in other/a/src/lib.rs, whatever exists.
    let want = Some(deny(&scope_text("a/src/lib.rs", true)));
    assert_eq!(out(&ctx(&e, &lv), &edit(ZC, json!("newdir/../l/../src/lib.rs"), &e.repo)), want, "newdir missing");
    std::fs::create_dir_all(e.repo.join("newdir")).unwrap();
    assert_eq!(out(&ctx(&e, &lv), &edit(ZC, json!("newdir/../l/../src/lib.rs"), &e.repo)), want, "newdir exists");
    std::fs::create_dir_all(other.join("a/src")).unwrap();
    assert_eq!(out(&ctx(&e, &lv), &edit(ZC, json!("newdir/../l/../src/lib.rs"), &e.repo)), want, "target dir exists");
}

#[cfg(unix)]
#[test] // Round 3 fix 2; kills: `l/..` taken lexically (repo/lib.rs) where the OS writes src/lib.rs
fn scope_symlink_inside_the_repository_is_physical() {
    let e = env();
    let lv = at("scope", "enforce");
    std::fs::write(e.repo.join("src/lib.rs"), "x").unwrap();
    std::fs::create_dir_all(e.repo.join("src/sub")).unwrap();
    std::os::unix::fs::symlink(e.repo.join("src/sub"), e.repo.join("l")).unwrap();
    start(&e, SUB, CHECK, &["lib.rs"]);
    assert_eq!(out(&ctx(&e, &lv), &edit(ZC, json!("l/../lib.rs"), &e.repo)), Some(deny(&scope_text("src/lib.rs", true))));
    assert_eq!(out(&ctx(&e, &lv), &edit(ZC, json!("l/../new.rs"), &e.repo)), Some(deny(&scope_text("src/new.rs", true))));
    start(&e, SUB, CHECK, &["src/lib.rs"]);
    assert_eq!(out(&ctx(&e, &lv), &edit(ZC, json!("l/../lib.rs"), &e.repo)), None);
}

#[cfg(unix)]
#[test] // Round 3 fix 2; kills: a dangling symlink judged by its own name; its relative target not resolved against its directory
fn scope_dangling_symlink_is_judged_by_its_target() {
    let e = env();
    let lv = at("scope", "enforce");
    start(&e, SUB, CHECK, &["src/lib.rs", "src/abs.rs"]);
    std::fs::create_dir_all(e.repo.join("other/x")).unwrap();
    std::os::unix::fs::symlink("../other/x/new.rs", e.repo.join("src/lib.rs")).unwrap();
    assert_eq!(out(&ctx(&e, &lv), &edit(ZC, json!("src/lib.rs"), &e.repo)), Some(deny(&scope_text("other/x/new.rs", true))));
    let other = e.t.path().join("other");
    for d in [".git", "x"] { std::fs::create_dir_all(other.join(d)).unwrap(); }
    std::os::unix::fs::symlink(other.join("x/new.rs"), e.repo.join("src/abs.rs")).unwrap();
    assert_eq!(out(&ctx(&e, &lv), &edit(ZC, json!("src/abs.rs"), &e.repo)), Some(deny(&scope_text("x/new.rs", true))));
    std::fs::write(e.repo.join("src/abs.rs"), "w").unwrap();            // a write follows the link
    assert!(other.join("x/new.rs").is_file());
}

/// The output guard at enforce on each command: `(command, deny wanted)`.
fn output_cases(cases: &[(&str, bool)]) {
    let e = env();
    let lv = cfg("[guards]\noutput = \"enforce\"\ntrailers = \"monitor\"\n".into());
    let c = ctx(&e, &lv);
    let bad: Vec<&str> = cases.iter().filter(|(cmd, hit)| (out(&c, &bash(None, cmd, &e.repo)) == Some(deny(OUTPUT))) != *hit).map(|(c, _)| *c).collect();
    assert!(bad.is_empty(), "{bad:?}");
}

#[test] // Round 3 fix 3; kills: `<<` in arithmetic taken as a heredoc; backslash-newline kept; `(` or `{` before the command
fn output_arithmetic_continuations_and_groups() {
    output_cases(&[
        ("x=$((1<<2))\nls | head", true),
        ("(( y = 1 << 3 ))\nls | head", true),
        ("echo $(( (1+2) << 1 )) | tail", true),
        ("ls | \\\nhead", true),
        ("ls | he\\\nad", true),
        ("ls | (head)", true),
        ("ls | { head; }", true),
        ("ls | { head -n 3; }", true),
        ("ls | (grep x)", false),
        ("ls | { grep x; }", false),
        ("echo '{' | grep head", false),
    ]);
}

#[test] // Round 2 and 3 tokenizer cases (reviewer probe p_tok)
fn output_tokenizer_probe_cases() {
    output_cases(&[
        ("cat <<EOF | head\nbody\nEOF", true),
        ("cat <<EOF|tail -n 2\nbody\nEOF", true),
        ("cat <<A <<'B' | tail\nx\nA\ny\nB", true),
        ("cat <<A; cat <<B\nx\nA\ny\nB\nls | head", true),
        ("cat <<EOF # c\nbody\nEOF\nls | head", true),
        ("cat <<EOF\nls | head", false),
        ("cat <<\"E O\"\nx\nE O\nls | head", true),
        ("cat <<E\"O\"F\nx\nEOF\nls | head", true),
        ("cat <<\\EOF\nx\nEOF\nls | head", true),
        ("echo a#b | head", true),
        ("echo x;#c\nls | head", true),
        ("cat <<EOF |\nbody\nEOF\nhead", true),
        ("cat <<-EOF\n\tx\n  EOF\nls | head", false),
        ("echo $# | head", true),
        ("ls|#c\nhead", true),
    ]);
}

#[test] // Round 3 fix 5; kills: --attr-source taking no value; long-option abbreviations missed; a value read as the message;
        // env, command, time or a path to git not accepted before `git`
fn trailers_prefixes_and_abbreviations() {
    let e = env();
    let lv = at("trailers", "enforce");
    let c = ctx(&e, &lv);
    std::fs::write(e.repo.join("m.txt"), "fix\n").unwrap();
    std::fs::write(e.repo.join("ok.txt"), "fix\n\nZygo-Session: S1\n").unwrap();
    for cmd in ["git --attr-source HEAD commit -m x", "git commit --mess x", "git commit --messag=x", "git commit --m x",
                "git commit --fil m.txt", "git commit --fil=m.txt", "git commit --file=m.txt", "/usr/bin/git commit -m x", "bin/git commit -m x",
                "env git commit -m x", "env -i git commit -m x", "env -u HOME X=1 git commit -m x", "A=1 env B=2 git commit -m x",
                "command git commit -m x", "time git commit -m x", "{ git commit -m x; }", "X=1 git -C . commit -am\"x\"",
                "git --no-pager -c a=b -C . commit -Fm.txt"] {
        assert_eq!(out(&c, &bash(None, cmd, &e.repo)), Some(deny(&trailer("<unit>"))), "{cmd}");
    }
    // A value of another option is never the message; an ambiguous abbreviation is no option git accepts.
    for cmd in ["git commit --author -mx -F ok.txt", "git commit --auth=-m -F ok.txt", "git commit --tem -mx -F ok.txt",
                "git commit --trailer -mx --fil ok.txt", "git commit --date -mx --file ok.txt", "git commit --fi m.txt",
                "git --attr-source commit log -m x"] {
        assert_eq!(out(&c, &bash(None, cmd, &e.repo)), None, "{cmd}");
    }
}

#[test] // Round 3 fix 5 known limits: sudo, xargs and git aliases are not detected
fn trailers_known_limits() {
    let e = env();
    let lv = at("trailers", "enforce");
    let c = ctx(&e, &lv);
    for cmd in ["sudo git commit -m x", "xargs git commit -m x", "git -c alias.x=commit x -m y"] {
        assert_eq!(out(&c, &bash(None, cmd, &e.repo)), None, "{cmd}");
    }
}

/// §13.4: the attribution guard's text for the matched form `label`.
fn attr(label: &str) -> String {
    format!("zygo: Claude attribution is never allowed in commits, pull requests or releases. Remove the line with `{label}` from the message and run the command again.")
}

#[test] // FIX-21, §13.4; kills: a form left out; case kept; `claude` not a whole word; the plain generated line passed
        // (controller change A); a product name refused
fn attribution_patterns_and_ordinary_text() {
    for (text, want) in [
        ("x\n\nCo-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>", "noreply@anthropic.com"),
        ("Co-Authored-By: Jane <NOREPLY@ANTHROPIC.COM>", "noreply@anthropic.com"),
        ("co-authored-by:claude", "Co-Authored-By: Claude"),
        ("Co-Authored-By:\tClaude <c@example.com>", "Co-Authored-By: Claude"),
        ("CO-AUTHORED-BY: CLAUDE", "Co-Authored-By: Claude"),
        ("x\nCo-Authored-By: Jane <j@example.com>\nCo-Authored-By: Claude", "Co-Authored-By: Claude"),
        ("Generated with [Claude Code](https://claude.com/claude-code)", "Generated with [Claude Code]"),
        ("🤖 Generated with [Claude Code](https://claude.com/claude-code)", "Generated with [Claude Code]"),
        ("Generated with Claude Code", "Generated with Claude Code"),
        ("generated with claude code", "Generated with Claude Code"),
        ("Summary\n\nGENERATED WITH  CLAUDE CODE (https://claude.com/claude-code)", "Generated with Claude Code"),
        ("Generated with Claude Code.", "Generated with Claude Code"),
        ("🤖  generated by an assistant", "🤖 Generated"),
        ("fix\n\nclaude-session: https://claude.ai/code/session_01", "Claude-Session:"),
    ] {
        assert_eq!(guards::claude_attribution(text), Some(want), "{text}");
    }
    for text in ["feat: Claude Code plugin support", "docs: supported hosts are Claude Code and OpenCode", "supports Claude Code",
                 "Co-Authored-By: Jane <jane@example.com>", "Co-Authored-By: Claudette <c@example.com>", "Claudette",
                 "fix(launch): claude_code.rs settings", "Generated with Claude Codex", "generated with claude", "Claude Code generated this test",
                 "anthropic.com docs link", "🤖 robot", ""] {
        assert_eq!(guards::claude_attribution(text), None, "{text}");
    }
}

#[test] // FIX-21, §13.4, controller change B; kills: a command, a message flag form or a file form left out; a command that
        // only reads text refused; the line not recorded
fn attribution_denies_commits_prs_and_releases() {
    let e = env();
    let c = ctx(&e, &no_config);
    std::fs::write(e.repo.join("co.md"), "Body\n\nCo-Authored-By: Claude <c@example.com>\n").unwrap();
    std::fs::write(e.repo.join("clean.md"), "Adds the Claude Code host.\n").unwrap();
    let co = attr("Co-Authored-By: Claude");
    let denied = [
        // git commit, --amend included
        "git commit -m \"fix\n\nCo-Authored-By: Claude <c@example.com>\"",
        "git commit -m \"$(cat <<'EOF'\nfix\n\nCo-Authored-By: Claude <c@example.com>\nEOF\n)\"",
        "git commit -F - <<'EOF'\nfix\n\nCo-Authored-By: Claude <c@example.com>\nEOF",
        "git commit -F co.md",
        "git commit --file=co.md",
        "git commit --message \"Co-Authored-By: Claude\"",
        "git commit --amend -m \"fix\n\nCo-Authored-By: Claude\"",
        "git commit --amend -F co.md",
        "git commit --amend --no-edit --trailer \"Co-Authored-By: Claude <c@example.com>\"",
        // git tag
        "git tag -a v1 -m \"Co-Authored-By: Claude\"",
        "git tag -a v1 --message=\"Co-Authored-By: Claude\"",
        "git tag -a v1 -m \"$(cat <<'EOF'\nv1\n\nCo-Authored-By: Claude\nEOF\n)\"",
        "git tag -a v1 -F co.md",
        "git tag -aF co.md v1",
        "git tag -a v1 --file co.md",
        "git tag -a v1 --file=co.md",
        "git -C . tag -s v1 -Fco.md",
        // gh pr create, edit, comment, review
        "gh pr create --title t --body-file co.md",
        "gh pr edit 7 -F co.md",
        "gh pr create --body-file=co.md",
        "gh pr create --title t --body-file - <<'EOF'\nBody\n\nCo-Authored-By: Claude\nEOF",
        "GH_HOST=x /usr/bin/gh pr create --title t --body \"Co-Authored-By: Claude\"",
        "gh pr comment 7 --body \"Co-Authored-By: Claude\"",
        "gh pr comment 7 -F co.md",
        "gh pr review 7 --approve --body-file co.md",
        "gh pr review 7 --comment -b \"co-authored-by: claude\"",
        // gh issue create, edit, comment
        "gh issue create --title t --body-file co.md",
        "gh issue edit 3 --body \"Co-Authored-By: Claude\"",
        "gh issue comment 3 -F=co.md",
        // gh release create, edit
        "gh release create v1 --notes-file co.md",
        "gh release edit v1 -Fco.md",
        "env gh release create v1 --notes \"co-authored-by: claude\"",
        "gh release edit v1 --notes-file=co.md",
    ];
    for cmd in denied {
        assert_eq!(out(&c, &bash(None, cmd, &e.repo)), Some(deny(&co)), "{cmd}");
    }
    let other = [
        ("gh pr create --title t --body \"x <noreply@anthropic.com>\"", attr("noreply@anthropic.com")),
        ("gh pr create --title t --body \"Summary\n\n🤖 Generated by a bot\"", attr("🤖 Generated")),
        ("gh pr create --title t --body \"Summary\n\nGenerated with Claude Code\"", attr("Generated with Claude Code")),
        ("git commit -m \"fix\n\nGenerated with [Claude Code](https://claude.com/claude-code)\"", attr("Generated with [Claude Code]")),
        ("gh pr comment 7 --body \"Claude-Session: https://claude.ai/code/session_01\"", attr("Claude-Session:")),
    ];
    for (cmd, want) in &other {
        assert_eq!(out(&c, &bash(None, cmd, &e.repo)), Some(deny(want)), "{cmd}");
    }
    for cmd in ["git commit -m \"feat: Claude Code plugin\"", "gh pr create --title \"Claude Code plugin\" --body \"Adds the Claude Code host\"",
                "gh pr create -F clean.md", "gh release create v1 --notes-file clean.md", "git tag -a v1 -F clean.md",
                "git tag -a v1 -m \"supports Claude Code\"", "gh pr comment 7 --body \"The Claude Code plugin works\"",
                "gh issue create --title t -F clean.md", "gh pr review 7 --approve", "gh pr view 7", "gh issue view 3", "git tag -l",
                "rg -n \"noreply@anthropic.com\" .", "echo \"Co-Authored-By: Claude\"", "git log --grep \"Co-Authored-By: Claude\""] {
        assert_eq!(out(&c, &bash(None, cmd, &e.repo)), None, "{cmd}");
    }
    assert_eq!(guard_lines(&e, "main"), vec![g("attribution", "enforce"); denied.len() + other.len()]);
}

#[test] // FIX-21 D1; kills: attribution behind the units or config gate; its text after the configurable guards'
fn attribution_runs_without_work_units() {
    let co = attr("Co-Authored-By: Claude");
    let e = env();
    Overrides { work_units: Some(false), ..Default::default() }.save(&e.sd).unwrap();
    let plain = cfg(String::new());
    assert_eq!(out(&ctx(&e, &plain), &bash(None, "git commit -m \"Co-Authored-By: Claude\"", &e.repo)), Some(deny(&co)));
    assert_eq!(out(&ctx(&e, &no_config), &bash(None, "git commit -m \"Co-Authored-By: Claude\"", &e.repo)), Some(deny(&co)));
    assert_eq!(guard_lines(&e, "main"), [g("attribution", "enforce"), g("attribution", "enforce")]);
    let e = env();
    let lv = at("trailers", "enforce");
    assert_eq!(out(&ctx(&e, &lv), &bash(None, "git commit -m \"fix Co-Authored-By: Claude\"", &e.repo)),
               Some(deny(&format!("{co}\n{}", trailer("<unit>")))));
    assert_eq!(guard_lines(&e, "main"), [g("attribution", "enforce"), g("trailers", "enforce")]);
}

#[test] // FIX-21, §13.4 known limits, pinned so a later widening is a visible change
fn attribution_known_limits() {
    let e = env();
    let c = ctx(&e, &no_config);
    std::fs::create_dir_all(e.repo.join("sub")).unwrap();
    std::fs::write(e.repo.join("sub/co.md"), "Body\n\nCo-Authored-By: Claude <c@example.com>\n").unwrap();
    for cmd in ["git commit", "git commit -F -", "git commit -F /dev/stdin", "cd sub && git commit -F co.md", "cd sub && gh pr create -F co.md",
                "sudo git commit -m \"Co-Authored-By: Claude\"", "xargs git commit -m \"Co-Authored-By: Claude\"",
                "git -c alias.x=commit x -m \"Co-Authored-By: Claude\"", "gh api repos/o/r/pulls -f body=\"Co-Authored-By: Claude\"",
                "gh pr create --body-file -"] {
        if cfg!(not(unix)) && cmd.contains("/dev/") { continue; }
        assert_eq!(out(&c, &bash(None, cmd, &e.repo)), None, "{cmd}");
    }
}
