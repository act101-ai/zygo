//! N35: `zygo usage` and `zygo doctor` (spec §10.7, §17, §8.1 A-13, §10.5, §15.3).
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
#[cfg(unix)]
use std::time::Duration;
use tempfile::TempDir;
use zygo::cmd::doctor::{absolute_entries, compiled_act_lite_version, report as doctor_report, Env, Probe};
#[cfg(unix)]
use zygo::cmd::doctor::probe_version;
use zygo::cmd::usage::{self, Args};
use zygo::cmd::verbs::VerbCtx;
use zygo::cmd::UsageError;
use zygo_config::keys::{Auto, Backend, FileStore, MemStore, Secret, SecretStore};
use zygo_config::{load, LoadContext};
use zygo_core::paths::Paths;
use zygo_core::Agent;
use zygo_plugins::sync::SyncOpts;
use zygo_work::policy::Policy;
use zygo_usage::{RedactionCount, RequestRow, Store, TrimRow, WorkUnitRow};
use zygo_work::report::parse_since;
use zygo_workspace::mux::MuxError;

fn paths(root: &Path) -> Paths { Paths { config_file: root.join("config.toml"), cache_dir: root.join("cache"),
    data_dir: root.join("data"), sessions_dir: root.join("cache/sessions"), marketplaces_dir: root.join("cache/marketplaces"),
    plugin_store: root.join("data/plugins"), approvals_file: root.join("data/approvals.toml"), usage_db: root.join("data/usage.db") } }
fn ctx(p: &Paths, slash: bool) -> VerbCtx { VerbCtx { slash, save: false, session_dir: None, paths: p.clone() } }
fn v(a: &[&str]) -> Vec<String> { a.iter().map(|s| s.to_string()).collect() }
#[allow(clippy::too_many_arguments)] // test row builder: one positional field per column
fn req(s: &str, ts: i64, role: &str, model: &str, t: [u64; 4], usd: f64, adj: &[&str], compaction: bool) -> RequestRow {
    RequestRow { ts, session: s.into(), role_served: Some(role.into()), model: model.into(), input_tokens: t[0], output_tokens: t[1],
        cache_read_tokens: t[2], cache_write_tokens: t[3], estimated_usd: usd, adjustments: adj.iter().map(|a| a.to_string()).collect(),
        compaction, ..Default::default() } }
fn trim(s: &str, ts: i64, stored: u64, view: u64) -> TrimRow { TrimRow { ts, session: s.into(), tool: "Bash".into(), kind: "head".into(), stored_bytes: stored, view_bytes: view, agent_id: None } }
fn unit(s: &str, u: &str, at: i64, g: &str, l: &str, n: u32) -> WorkUnitRow { WorkUnitRow { unit: u.into(), round: 1, session: s.into(), agent_id: "main".into(),
    began_at: at, guard_events: BTreeMap::from([(g.to_string(), BTreeMap::from([(l.to_string(), n)]))]), ..Default::default() } }
/// S1: two `code`/`m-a` requests, S2: one `lead`/`m-b`; returns the UTC midnight of 2026-11-18.
fn seed(p: &Paths) -> i64 {
    let b = parse_since("2026-11-18").unwrap();
    let s = Store::open(&p.usage_db).unwrap();
    s.insert_request(&req("S1", b + 10, "code", "m-a", [100, 20, 300, 0], 0.01, &[], false)).unwrap();
    s.insert_request(&req("S1", b + 20, "code", "m-a", [50, 10, 150, 50], 0.005, &["context_signal"], true)).unwrap();
    s.insert_request(&req("S2", b + 30, "lead", "m-b", [200, 40, 0, 0], 0.02, &[], false)).unwrap();
    s.insert_trim(&trim("S1", b + 11, 5000, 1000)).unwrap();
    s.insert_trim(&trim("S2", b + 31, 2000, 500)).unwrap();
    s.insert_work_unit(&unit("S1", "ENG-1", b + 5, "claims", "notify", 2)).unwrap();
    s.insert_work_unit(&unit("S2", "ENG-2", b + 25, "scope", "enforce", 1)).unwrap();
    b
}
fn run(p: &Paths, a: &[&str], env: Option<&str>) -> anyhow::Result<String> { usage::run(&ctx(p, false), &v(a), env) }

/// Held by every test that forks a process, from its first stub write or fork until its last program has ended, so no
/// other test forks while a stub is open for writing (ETXTBSY).
static STUBS: Mutex<()> = Mutex::new(());
fn forks() -> std::sync::MutexGuard<'static, ()> { STUBS.lock().unwrap_or_else(|e| e.into_inner()) }

/// Writes an executable `#!/bin/sh` program; the caller holds `forks()`.
#[cfg(unix)]
fn stub(dir: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let f = dir.join(name);
    std::fs::write(&f, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
}

const ALL: &str = "scope: all sessions\nrequests: 3\ntokens: 350 in, 70 out, 450 cache read, 50 cache write\ncache read share: 53%\nestimated cost: $0.0350\nbytes trimmed: 5500\ntokens trimmed (estimate): 1375\ncompaction signals: 1\ncompactions observed: 1\nguard events: claims notify 2, scope enforce 1\nby role and model:\n  code m-a: 2 requests, 150 in, 30 out, 450 cache read, 50 cache write, $0.0150\n  lead m-b: 1 requests, 200 in, 40 out, 0 cache read, 0 cache write, $0.0200";
const S1: &str = "scope: session S1\nrequests: 2\ntokens: 150 in, 30 out, 450 cache read, 50 cache write\ncache read share: 69%\nestimated cost: $0.0150\nbytes trimmed: 4000\ntokens trimmed (estimate): 1000\ncompaction signals: 1\ncompactions observed: 1\nguard events: claims notify 2\nby role and model:\n  code m-a: 2 requests, 150 in, 30 out, 450 cache read, 50 cache write, $0.0150";

#[test]
fn usage_report_all_sessions() {
    let d = TempDir::new().unwrap();
    let p = paths(d.path());
    seed(&p);
    assert_eq!(run(&p, &[], None).unwrap(), ALL);
}

#[test]
fn usage_report_one_session() {
    let d = TempDir::new().unwrap();
    let p = paths(d.path());
    seed(&p);
    assert_eq!(run(&p, &["--session", "S1"], None).unwrap(), S1);
    assert_eq!(run(&p, &["--session=S1"], None).unwrap(), S1);
}

#[test]
fn usage_report_since_boundary() {
    let d = TempDir::new().unwrap();
    let p = paths(d.path());
    let b = parse_since("2026-11-18").unwrap();
    let s = Store::open(&p.usage_db).unwrap();
    s.insert_request(&req("S3", b - 1, "code", "m-a", [7, 0, 0, 0], 0.0, &[], false)).unwrap();
    s.insert_request(&req("S3", b, "code", "m-a", [11, 0, 0, 0], 0.0, &[], false)).unwrap();
    let want = "scope: since 2026-11-18\nrequests: 1\ntokens: 11 in, 0 out, 0 cache read, 0 cache write\ncache read share: 0%\nestimated cost: $0.0000\nbytes trimmed: 0\ntokens trimmed (estimate): 0\ncompaction signals: 0\ncompactions observed: 0\nguard events: none\nby role and model:\n  code m-a: 1 requests, 11 in, 0 out, 0 cache read, 0 cache write, $0.0000";
    assert_eq!(run(&p, &["--since", "2026-11-18"], None).unwrap(), want);
    assert_eq!(run(&p, &["--since=2026-11-18"], None).unwrap(), want);
}

#[test]
fn usage_report_env_session_default() {
    let d = TempDir::new().unwrap();
    let p = paths(d.path());
    seed(&p);
    assert_eq!(run(&p, &[], Some("S1")).unwrap(), S1);
    assert!(run(&p, &["--since", "2026-11-18"], Some("S1")).unwrap().starts_with("scope: since 2026-11-18\nrequests: 3\n"));
    assert!(run(&p, &["--session", "S2"], Some("S1")).unwrap().starts_with("scope: session S2\nrequests: 1\n"));
    assert!(run(&p, &["--session", "S1", "--since", "2026-11-18"], None).unwrap().starts_with("scope: session S1 since 2026-11-18\nrequests: 2\n"));
}

#[test]
fn usage_report_empty_store_creates_nothing() {
    let d = TempDir::new().unwrap();
    let p = paths(d.path());
    let want = "scope: all sessions\nrequests: 0\ntokens: 0 in, 0 out, 0 cache read, 0 cache write\ncache read share: -\nestimated cost: $0.0000\nbytes trimmed: 0\ntokens trimmed (estimate): 0\ncompaction signals: 0\ncompactions observed: 0\nguard events: none\nby role and model: none";
    assert_eq!(run(&p, &[], None).unwrap(), want);
    assert!(!p.usage_db.exists() && !p.data_dir.exists());
}

/// H§14 #18, AU26: the exact `bytes trimmed` line stays, and the estimate `ceil(bytes ÷ 4)` follows it as a whole number.
#[test]
fn h14_18_usage_keeps_bytes_and_adds_estimated_tokens() {
    let d = TempDir::new().unwrap();
    let p = paths(d.path());
    let s = Store::open(&p.usage_db).unwrap();
    s.insert_trim(&trim("S1", 1, 100_000, 7_840)).unwrap();
    s.insert_trim(&trim("S2", 2, 5, 0)).unwrap();
    let lines = |a: &[&str]| -> Vec<String> { run(&p, a, None).unwrap().lines().map(str::to_string).collect() };
    let one = lines(&["--session", "S1"]);
    let at = one.iter().position(|l| l == "bytes trimmed: 92160").unwrap_or_else(|| panic!("{one:?}"));
    assert_eq!(one[at + 1], "tokens trimmed (estimate): 23040");
    assert_eq!(one[at + 2], "compaction signals: 0");
    // 5 bytes are 2 estimated tokens: the estimate rounds up.
    let two = lines(&["--session", "S2"]);
    assert!(has(&two, "bytes trimmed: 5") && has(&two, "tokens trimmed (estimate): 2"), "{two:?}");
    let all = lines(&[]);
    assert!(has(&all, "bytes trimmed: 92165") && has(&all, "tokens trimmed (estimate): 23042"), "{all:?}");
}

#[test]
fn usage_report_slash_equals_cli() {
    let d = TempDir::new().unwrap();
    let p = paths(d.path());
    seed(&p);
    let slash = usage::run(&ctx(&p, true), &v(&[]), None).unwrap();
    assert_eq!(slash, usage::run(&ctx(&p, false), &v(&[]), None).unwrap());
    assert_eq!(slash, ALL);
}

#[test]
fn usage_report_names_no_row_text() {
    let d = TempDir::new().unwrap();
    let p = paths(d.path());
    let s = Store::open(&p.usage_db).unwrap();
    let t = || Some("x-sentinel-n35".to_string());
    s.insert_request(&RequestRow { ts: 1, session: "S9".into(), role_served: Some("code".into()), model: "m".into(), repo: t(), work_item: t(),
        agent_id: t(), token_label: t(), profile: t(), provider: "prov-sentinel-n35".into(), agent_type: t(),
        adjustments: vec!["adj-sentinel-n35".into()], redactions: vec![RedactionCount { id: "red-sentinel-n35".into(), count: 3 }],
        ..Default::default() }).unwrap();
    s.insert_work_unit(&unit("S9", "unit-sentinel-n35", 1, "claims", "notify", 1)).unwrap();
    for a in [&[][..], &["--session", "S9"][..]] {
        let out = run(&p, a, None).unwrap();
        assert!(out.contains("requests: 1"), "{out}");
        assert!(!out.contains("sentinel"), "{out}");
    }
}

#[test]
fn usage_report_escapes_role_and_model() {
    let d = TempDir::new().unwrap();
    let p = paths(d.path());
    let s = Store::open(&p.usage_db).unwrap();
    s.insert_request(&req("S1", 1, "code\nforged: x\u{1b}", "m\nforged: y", [1, 0, 0, 0], 0.0, &[], false)).unwrap();
    let out = run(&p, &[], None).unwrap();
    assert!(!out.lines().any(|l| l.starts_with("forged:")), "{out}");
    assert!(out.contains("code\\nforged: x\\u{1b}") && out.contains("m\\nforged: y"), "{out}");
    assert_eq!(out.lines().count(), 12, "{out}");
}

#[test]
fn usage_report_corrupt_db_error() {
    let d = TempDir::new().unwrap();
    let p = paths(d.path());
    std::fs::create_dir_all(&p.data_dir).unwrap();
    std::fs::write(&p.usage_db, b"not a database").unwrap();
    let e = run(&p, &[], None).unwrap_err();
    assert!(e.to_string().starts_with("zygo usage: "), "{e}");
    assert!(e.downcast_ref::<UsageError>().is_none());
}

#[test]
fn usage_args_parse_table() {
    let s = |a: &[&str]| usage::parse(&v(a)).unwrap();
    assert_eq!(s(&[]), Args::default());
    for a in [&["--session", "S1"][..], &["--session=S1"][..]] {
        assert_eq!(s(a), Args { session: Some("S1".into()), ..Default::default() });
    }
    for a in [&["--since", "2026-11-18"][..], &["--since=2026-11-18"][..]] {
        assert_eq!(s(a), Args { since: Some("2026-11-18".into()), ..Default::default() });
    }
    assert_eq!(s(&["--sync"]), Args { sync: true, ..Default::default() });
    assert_eq!(s(&["--session", "S1", "--since", "2026-11-18"]),
        Args { session: Some("S1".into()), since: Some("2026-11-18".into()), sync: false });
}

#[test]
fn usage_args_errors_are_usage_errors() {
    let d = TempDir::new().unwrap();
    let p = paths(d.path());
    let usage_text = "zygo usage: usage: usage [--session <id> | --since <date>] [--sync]";
    for a in [&["--session"][..], &["--since"], &["--bogus"], &["S1"], &["--session", "a", "--session", "b"], &["--since", "a", "--since", "b"]] {
        let e = run(&p, a, None).unwrap_err();
        assert_eq!(e.downcast_ref::<UsageError>().unwrap_or_else(|| panic!("{a:?}: {e}")).0, usage_text, "{a:?}");
    }
    for bad in ["nope", "2026-13-45"] {
        let e = run(&p, &["--since", bad], None).unwrap_err();
        assert_eq!(e.downcast_ref::<UsageError>().unwrap().0, format!("zygo usage: --since takes a date YYYY-MM-DD, not `{bad}`"));
    }
    let e = run(&p, &["--sync"], None).unwrap_err();
    assert!(e.downcast_ref::<UsageError>().is_none() && e.to_string().contains("--sync"), "{e}");
}

#[test]
fn usage_args_binary_takes_session_value() {
    let _g = forks();
    let d = TempDir::new().unwrap();
    let zygo = |args: &[&str]| Command::new(env!("CARGO_BIN_EXE_zygo")).args(args).env("HOME", d.path()).env("LOCALAPPDATA", d.path())
        .env("APPDATA", d.path()).env("ZYGO_CONFIG", d.path().join("config.toml")).env_remove("ZYGO_SESSION")
        .env_remove("ZYGO_SESSION_DIR").current_dir(d.path()).output().unwrap();
    let o = zygo(&["usage", "--session", "S1"]);
    assert_eq!(o.status.code(), Some(0), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(String::from_utf8_lossy(&o.stdout).starts_with("scope: session S1\nrequests: 0\n"));
    let o = zygo(&["usage", "--since", "nope"]);
    assert_eq!(o.status.code(), Some(2));
    assert_eq!(String::from_utf8_lossy(&o.stderr), "zygo usage: --since takes a date YYYY-MM-DD, not `nope`\n");
}

fn no_env(_: &str) -> Option<OsString> { None }
fn found(s: &str) -> Probe { Probe::Output(s.into()) }
fn mn(a: Agent) -> String { zygo_config::builtin::min_agent_version(a).to_string() }
fn lines_with(p: &Paths, secrets: &dyn SecretStore, probe: &dyn Fn(&str) -> Probe) -> Vec<String> {
    let m = load(&LoadContext { paths: p.clone(), repo: None, team: None }).unwrap();
    doctor_report(&Env { paths: p, repo: None, search_path: None, claude_config_dir: None, secrets, env: &no_env, probe, server: None, tmux: Err(MuxError::TimedOut) }, &m)
}
fn lines(p: &Paths, probe: &dyn Fn(&str) -> Probe) -> Vec<String> { lines_with(p, &MemStore::new(), probe) }
fn has(l: &[String], t: &str) -> bool { l.iter().any(|x| x == t) }

#[test]
fn doctor_agents_versions() {
    let d = TempDir::new().unwrap();
    let p = paths(d.path());
    let (c, o) = (mn(Agent::ClaudeCode), mn(Agent::OpenCode));
    let act = compiled_act_lite_version();
    let l = lines(&p, &|prog| match prog {
        "claude" => found(&format!("{c} (Claude Code)")),
        "opencode" => found(&o),
        "act-lite" => found(&format!("act-lite {act}")),
        _ => Probe::NotFound,
    });
    assert!(has(&l, &format!("agent claude-code: {c} (minimum {c})")), "{l:?}");
    assert!(has(&l, &format!("agent opencode: {o} (minimum {o})")), "{l:?}");
    assert!(has(&l, &format!("act-lite: {act} (list compiled for {act})")), "{l:?}");
    let l = lines(&p, &|prog| if prog == "claude" { found("99.0.0 (Claude Code)") } else { Probe::NotFound });
    assert!(has(&l, &format!("agent claude-code: 99.0.0 (minimum {c})")), "{l:?}");
    let l = lines(&p, &|prog| if prog == "claude" { found("1.0.0\n99.0.0") } else { Probe::NotFound });
    assert!(has(&l, &format!("agent claude-code: 1.0.0 (minimum {c}; below minimum)")), "{l:?}");
}

#[test]
fn doctor_agents_below_minimum_missing_unreadable() {
    let d = TempDir::new().unwrap();
    let p = paths(d.path());
    let (c, o) = (mn(Agent::ClaudeCode), mn(Agent::OpenCode));
    let l = lines(&p, &|prog| if prog == "claude" { found("0.0.1 (Claude Code)") } else { Probe::NotFound });
    assert!(has(&l, &format!("agent claude-code: 0.0.1 (minimum {c}; below minimum)")), "{l:?}");
    assert!(has(&l, "agent opencode: not found"), "{l:?}");
    let l = lines(&p, &|prog| if prog == "claude" { found(&format!("{c}-beta (Claude Code)")) } else { Probe::NotFound });
    assert!(has(&l, &format!("agent claude-code: {c}-beta (minimum {c}; below minimum)")), "{l:?}");
    let l = lines(&p, &|prog| if prog == "claude" { found("garbage") } else { Probe::Failed });
    assert!(has(&l, &format!("agent claude-code: unknown (minimum {c})")), "{l:?}");
    assert!(has(&l, &format!("agent opencode: unknown (minimum {o})")), "{l:?}");
}

#[test]
fn doctor_agents_act_lite_versions() {
    let d = TempDir::new().unwrap();
    let p = paths(d.path());
    let act = compiled_act_lite_version();
    let at = |r: Probe| lines(&p, &move |prog| if prog == "act-lite" { r.clone() } else { Probe::NotFound });
    assert!(has(&at(found("act-lite 9.9.9")), &format!("act-lite: 9.9.9 (list compiled for {act}; version mismatch)")));
    assert!(has(&at(Probe::NotFound), "act-lite: not found"));
    assert!(has(&at(found("???")), "act-lite: unknown"));
    assert!(has(&at(Probe::Failed), "act-lite: unknown"));
}

#[test]
fn doctor_agents_hostile_version_output() {
    let d = TempDir::new().unwrap();
    let p = paths(d.path());
    let c = mn(Agent::ClaudeCode);
    let l = lines(&p, &|prog| if prog == "claude" { found("\u{1b}[2J\nagent claude-code: 99.0.0") } else { Probe::NotFound });
    assert!(has(&l, &format!("agent claude-code: unknown (minimum {c})")), "{l:?}");
    assert!(!l.iter().any(|x| x.starts_with("agent claude-code: 99.0.0")), "{l:?}");
    assert!(!l.iter().any(|x| x.contains('\u{1b}')), "{l:?}");
}

#[test]
fn doctor_agents_compiled_version_is_the_fixture() {
    let t = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/act-lite/languages.json")).unwrap();
    let j: serde_json::Value = serde_json::from_str(&t).unwrap();
    assert_eq!(j["act_lite"].as_str().unwrap().trim_start_matches('v'), compiled_act_lite_version());
}


#[cfg(unix)]
#[test]
fn doctor_probe_reads_version_output() {
    let d = TempDir::new().unwrap();
    let _g = forks();
    stub(d.path(), "act-lite", "[ \"$1\" = --version ] || exit 3; echo \"act-lite 1.2.5\"");
    assert_eq!(probe_version("act-lite", Some(d.path().as_os_str()), Duration::from_secs(5)), Probe::Output("act-lite 1.2.5".into()));
}

#[cfg(unix)]
#[test]
fn doctor_probe_skips_leading_blank_lines() {
    let d = TempDir::new().unwrap();
    let _g = forks();
    stub(d.path(), "act-lite", "printf '\\n  \\nfoo 1.2.3\\n'");
    assert_eq!(probe_version("act-lite", Some(d.path().as_os_str()), Duration::from_secs(5)), Probe::Output("foo 1.2.3".into()));
}

#[cfg(unix)]
#[test]
fn doctor_probe_not_found_and_failed() {
    let d = TempDir::new().unwrap();
    assert_eq!(probe_version("act-lite", Some(d.path().as_os_str()), Duration::from_secs(5)), Probe::NotFound);
    let _g = forks();
    stub(d.path(), "act-lite", "exit 1");
    assert_eq!(probe_version("act-lite", Some(d.path().as_os_str()), Duration::from_secs(5)), Probe::Failed);
}

#[cfg(unix)]
#[test]
fn doctor_probe_past_deadline_spawns_nothing() {
    let d = TempDir::new().unwrap();
    let marker = d.path().join("marker");
    let _g = forks();
    stub(d.path(), "act-lite", &format!("echo ran > \"{}\"; echo 1.0.0", marker.display()));
    assert_eq!(probe_version("act-lite", Some(d.path().as_os_str()), Duration::ZERO), Probe::Failed);
    assert!(!marker.exists());
}

#[cfg(unix)]
#[test]
fn doctor_probe_kills_a_hung_program() {
    let d = TempDir::new().unwrap();
    let pidfile = d.path().join("pid");
    let _g = forks();
    stub(d.path(), "act-lite", &format!("echo $$ > \"{}\"; exec sleep 30", pidfile.display()));
    let t = std::time::Instant::now();
    assert_eq!(probe_version("act-lite", Some(d.path().as_os_str()), Duration::from_secs(1)), Probe::Failed);
    assert!(t.elapsed() < Duration::from_secs(5));
    let pid = std::fs::read_to_string(&pidfile).unwrap();
    let alive = Command::new("kill").args(["-0", pid.trim()]).output().unwrap().status.success();
    assert!(!alive, "the hung program is still running");
}

#[cfg(unix)]
#[test]
fn doctor_probe_output_is_capped() {
    let d = TempDir::new().unwrap();
    let _g = forks();
    stub(d.path(), "act-lite", "head -c 10000 /dev/zero | tr '\\0' x; echo ' 1.0.0'");
    let Probe::Output(s) = probe_version("act-lite", Some(d.path().as_os_str()), Duration::from_secs(5)) else { panic!("no output") };
    assert!(s.len() <= 4096 && !s.contains("1.0.0"), "{}", s.len());
}

/// The pid in `pidfile` names a live process (or an unreaped zombie).
#[cfg(unix)]
fn alive(pidfile: &Path) -> bool {
    let pid = std::fs::read_to_string(pidfile).unwrap();
    Command::new("kill").args(["-0", pid.trim()]).output().unwrap().status.success()
}

#[cfg(unix)]
#[test]
fn doctor_probe_grandchild_holding_stdout_keeps_the_version() {
    let d = TempDir::new().unwrap();
    let _g = forks();
    stub(d.path(), "act-lite", "sleep 31 &\necho 1.0.0");
    let t = std::time::Instant::now();
    assert_eq!(probe_version("act-lite", Some(d.path().as_os_str()), Duration::from_secs(2)), Probe::Output("1.0.0".into()));
    assert!(t.elapsed() < Duration::from_secs(2), "{:?}", t.elapsed());
}

#[cfg(unix)]
#[test]
fn doctor_probe_no_grandchild_survives() {
    for last in ["exit 1", "echo 1.0.0"] {
        let d = TempDir::new().unwrap();
        let pf = d.path().join("pid");
        let _g = forks();
        stub(d.path(), "act-lite", &format!("sleep 31 &\necho $! > \"{}\"\n{last}", pf.display()));
        let r = probe_version("act-lite", Some(d.path().as_os_str()), Duration::from_secs(2));
        assert_eq!(r, if last == "exit 1" { Probe::Failed } else { Probe::Output("1.0.0".into()) });
        // The killed grandchild is reaped by init; give it a moment.
        let t = std::time::Instant::now();
        while alive(&pf) && t.elapsed() < Duration::from_secs(2) { std::thread::sleep(Duration::from_millis(20)); }
        let a = alive(&pf);
        if a { let _ = Command::new("kill").args(["-9", std::fs::read_to_string(&pf).unwrap().trim()]).output(); }
        assert!(!a, "the grandchild of `{last}` survives the probe");
    }
}

#[cfg(unix)]
#[test]
fn doctor_probe_kills_a_term_ignoring_endless_printer() {
    let d = TempDir::new().unwrap();
    let pf = d.path().join("pid");
    let _g = forks();
    stub(d.path(), "act-lite", &format!("trap '' TERM\necho $$ > \"{}\"\nwhile :; do echo xxxxxxxxxxxxxxxxxxxxxxxx; done", pf.display()));
    let t = std::time::Instant::now();
    assert_eq!(probe_version("act-lite", Some(d.path().as_os_str()), Duration::from_secs(1)), Probe::Failed);
    assert!(t.elapsed() < Duration::from_secs(3), "{:?}", t.elapsed());
    assert!(!alive(&pf), "the printer is still running");
}

#[test]
fn doctor_probe_skips_relative_path_entries() {
    let d = TempDir::new().unwrap();
    let joined = std::env::join_paths(["bin".as_ref(), Path::new("."), Path::new(""), d.path()]).unwrap();
    assert_eq!(absolute_entries(Some(&joined)).unwrap(), d.path().as_os_str());
    assert!(absolute_entries(None).is_none());
    assert!(absolute_entries(Some(&std::env::join_paths(["bin", "."]).unwrap())).is_none());
}

/// A config with one model and one `[[model.provider]]` per `(name, extra line)`.
fn cfg(providers: &[(&str, &str)]) -> String {
    let mut t = String::from("[[model]]\nid = \"m1\"\n[model.facts]\nreasoning = \"always\"\neffort_levels = [\"low\", \"high\"]\ncontext_window = 200000\nmax_output_tokens = 64000\n");
    for (n, extra) in providers {
        t += &format!("[[model.provider]]\nname = \"{n}\"\nbase_url = \"https://p.example.invalid\"\nupstream_model = \"u\"\ndialects = [\"anthropic\"]\n{extra}\n");
    }
    t
}
fn never(_: &str) -> Probe { Probe::NotFound }

#[test]
fn doctor_keys_secret_store_backend() {
    let d = TempDir::new().unwrap();
    let p = paths(d.path());
    assert!(has(&lines_with(&p, &MemStore::new(), &never), "secret store: os-keychain"));
    let file = Auto::new(None, FileStore::new(d.path().join("keys.toml")));
    assert!(has(&lines_with(&p, &file, &never), &format!("secret store: {}", Backend::File)));
}

struct Broken;
impl SecretStore for Broken {
    fn backend(&self) -> Backend { Backend::OsKeychain }
    fn get(&self, _: &str) -> anyhow::Result<Option<Secret>> { Err(anyhow::anyhow!("store unavailable")) }
    fn set(&self, _: &str, _: &Secret) -> anyhow::Result<Backend> { unreachable!() }
    fn delete(&self, _: &str) -> anyhow::Result<bool> { unreachable!() }
}

#[test]
fn doctor_keys_per_provider_and_no_value_printed() {
    let d = TempDir::new().unwrap();
    let p = paths(d.path());
    std::fs::write(&p.config_file, cfg(&[("acme", "key = \"keychain:acme\""), ("viaenv", "key = \"env:ZYGO_N35_KEY\""), ("local", ""),
        ("off", "key = \"env:ZYGO_N35_OFF\"\nenabled = false")])).unwrap();
    let store = MemStore::new();
    let stored = format!("k-{}", ulid::Ulid::generate());
    store.set("acme", &Secret::new(stored.clone())).unwrap();
    let from_env = format!("e-{}", ulid::Ulid::generate());
    let m = load(&LoadContext { paths: p.clone(), repo: None, team: None }).unwrap();
    let with_env = |value: Option<String>| {
        let env = move |n: &str| if n == "ZYGO_N35_KEY" { value.clone().map(OsString::from) } else { None };
        doctor_report(&Env { paths: &p, repo: None, search_path: None, claude_config_dir: None, secrets: &store, env: &env, probe: &never, server: None, tmux: Err(MuxError::TimedOut) }, &m)
    };
    let first = with_env(None);
    assert!(has(&first, "provider acme: keychain:acme set"), "{first:?}");
    assert!(has(&first, "provider viaenv: env:ZYGO_N35_KEY unset"), "{first:?}");
    assert!(has(&first, "provider local: no key"), "{first:?}");
    assert!(!first.iter().any(|l| l.starts_with("provider off:")), "{first:?}");
    let second = with_env(Some(from_env.clone()));
    assert!(has(&second, "provider viaenv: env:ZYGO_N35_KEY set"), "{second:?}");
    let all = [first, second].concat().join("\n");
    assert!(!all.contains(&stored) && !all.contains(&from_env));
    let l = lines_with(&p, &Broken, &never);
    assert!(has(&l, "provider acme: keychain:acme unreadable (other)"), "{l:?}");
    assert!(!l.join("\n").contains("store unavailable"));
}

const NULL: &str = if cfg!(windows) { "NUL" } else { "/dev/null" };

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git").env("GIT_CONFIG_GLOBAL", NULL).env("GIT_CONFIG_NOSYSTEM", "1")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid", "-c", "commit.gpgsign=false", "-c", "tag.gpgsign=false"])
        .arg("-C").arg(dir).args(args).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

fn write(dir: &Path, rel: &str, text: &str) {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

/// A repository at `dir` with the plugin `demo` 1.0.0 whose `.mcp.json` is `mcp`, tagged `v1.0.0`; its path.
fn market_with(dir: &Path, mcp: &str) -> String {
    std::fs::create_dir_all(dir).unwrap();
    git(dir, &["init", "-q", "-b", "main"]);
    write(dir, ".claude-plugin/marketplace.json",
        r#"{"name":"team","owner":{"name":"Owner"},"plugins":[{"name":"demo","source":"./plugins/demo","description":"Demo","version":"1.0.0"}]}"#);
    write(dir, "plugins/demo/.claude-plugin/plugin.json", r#"{"name":"demo","version":"1.0.0","description":"Demo plugin","license":"MIT"}"#);
    write(dir, "plugins/demo/skills/hello/SKILL.md", "---\nname: hello\ndescription: Says hello\n---\nSay hello.\n");
    write(dir, "plugins/demo/.mcp.json", mcp);
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", "1.0.0"]);
    git(dir, &["tag", "v1.0.0"]);
    dir.display().to_string()
}
fn market(dir: &Path) -> String { market_with(dir, r#"{"mcpServers":{"nav":{"command":"zygo-n35-absent-tool"}}}"#) }

/// Adds and enables `demo` from `src` into the store of `p`.
fn fill(p: &Paths, src: &str, tmp: &Path) {
    let empty = tmp.join("empty-gitconfig");
    std::fs::write(&empty, "").unwrap();
    zygo::cmd::marketplace::run_io(&ctx(p, false), &v(&["add", src, "--enable", "demo"]), None, Some(toml::Table::new()),
        &mut Cursor::new(String::new()), &mut Vec::new(), &SyncOpts { git_config_global: Some(empty), ..Default::default() }).unwrap();
}

fn plug_lines(p: &Paths, search: Option<&Path>, claude: Option<&Path>) -> Vec<String> {
    let m = load(&LoadContext { paths: p.clone(), repo: None, team: None }).unwrap();
    doctor_report(&Env { paths: p, repo: None, search_path: search.map(|s| s.as_os_str().to_owned()), claude_config_dir: claude.map(Path::to_path_buf),
        secrets: &MemStore::new(), env: &no_env, probe: &never, server: None, tmux: Err(MuxError::TimedOut) }, &m)
}
fn claude_dir(d: &Path, json: &str) -> PathBuf {
    let c = d.join("claude");
    write(&c, "plugins/installed_plugins.json", json);
    c
}

#[test]
fn doctor_plugins_store_states() {
    let _g = forks();
    let d = TempDir::new().unwrap();
    let p = paths(&d.path().join("a"));
    assert!(has(&plug_lines(&p, None, None), "plugin store: manifest missing"));
    write(&p.plugin_store, "manifest.json", "{");
    assert!(plug_lines(&p, None, None).iter().any(|l| l.starts_with("plugin store: manifest invalid (")));

    let p = paths(&d.path().join("b"));
    fill(&p, &market(&d.path().join("src")), d.path());
    let l = plug_lines(&p, None, None);
    assert!(has(&l, "plugin store: manifest valid, plugins: 1"), "{l:?}");
    assert!(has(&l, "plugin act101:act-lite: not in store; run zygo plugins sync"), "{l:?}");
    let mcp = p.plugin_store.join("team/demo/1.0.0/.mcp.json");
    let mut bytes = std::fs::read(&mcp).unwrap();
    bytes.push(b' ');
    std::fs::write(&mcp, bytes).unwrap();
    let l = plug_lines(&p, None, None);
    assert!(has(&l, "plugin team:demo: store copy changed; run zygo plugins sync"), "{l:?}");
}

#[test]
fn doctor_plugins_missing_commands() {
    let _g = forks();
    let d = TempDir::new().unwrap();
    let p = paths(&d.path().join("a"));
    fill(&p, &market(&d.path().join("src")), d.path());
    let bin = d.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let l = plug_lines(&p, Some(&bin), None);
    assert!(has(&l, "plugin team:demo: zygo-n35-absent-tool not found (mcp server nav)"), "{l:?}");
    #[cfg(unix)]
    {
        stub(&bin, "zygo-n35-absent-tool", "exit 0");
        let l = plug_lines(&p, Some(&bin), None);
        assert!(!l.iter().any(|x| x.starts_with("plugin ") && x.contains("not found")), "{l:?}");
    }
}

#[test]
fn doctor_plugins_echoed_fields_are_escaped() {
    let _g = forks();
    let d = TempDir::new().unwrap();
    let bin = d.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let p = paths(&d.path().join("a"));
    fill(&p, &market_with(&d.path().join("src"),
        r#"{"mcpServers":{"na\nv\u001b":{"command":"zygo-n35-absent\nforged: line\u001b[2J"}}}"#), d.path());
    let known = ["agent ", "act-lite:", "tmux:", "secret store:", "provider ", "server:", "plugin store:", "plugin ", "claude config:", "icons:", "repo policy:"];
    let sane = |l: &[String]| {
        assert!(!l.iter().any(|x| x.starts_with("forged:") || x.contains('\u{1b}')), "{l:?}");
        assert!(l.iter().all(|x| known.iter().any(|k| x.starts_with(k))), "{l:?}");
    };
    let l = plug_lines(&p, Some(&bin), None);
    sane(&l);
    let want = r"plugin team:demo: zygo-n35-absent\nforged: line\u{1b}[2J not found (mcp server na\nv\u{1b})";
    assert_eq!(l.iter().filter(|x| **x == want).count(), 1, "{l:?}");

    let c = claude_dir(d.path(), r#"{"version":2,"plugins":{"demo@evil\nforged: line\u001b":[]}}"#);
    let l = plug_lines(&p, Some(&bin), Some(&c));
    sane(&l);
    let want = r"plugin team:demo: also installed in Claude Code as demo@evil\nforged: line\u{1b}; it would load twice";
    assert_eq!(l.iter().filter(|x| **x == want).count(), 1, "{l:?}");

    let p2 = paths(&d.path().join("b"));
    write(&p2.plugin_store, "manifest.json", "{\"plugins\":\n[1]}");
    let l = plug_lines(&p2, None, None);
    sane(&l);
    assert_eq!(l.iter().filter(|x| x.starts_with("plugin store: manifest invalid (")).count(), 1, "{l:?}");
}

#[test]
fn doctor_plugins_also_in_claude_config() {
    let d = TempDir::new().unwrap();
    let p = paths(d.path());
    std::fs::write(&p.config_file, "[[marketplace]]\nname = \"team\"\nsource = \"github:example/agent-plugins\"\nref = \"v1.4.0\"\n[plugins]\nenable = [\"team:demo\"]\n").unwrap();
    let c = claude_dir(d.path(), r#"{"version":2,"plugins":{"demo@other-market":[{"scope":"user"}],"unrelated@x":[]}}"#);
    let l = plug_lines(&p, None, Some(&c));
    assert!(has(&l, "plugin team:demo: also installed in Claude Code as demo@other-market; it would load twice"), "{l:?}");
    assert!(!l.iter().any(|x| x.contains("unrelated")), "{l:?}");
    let none = d.path().join("none");
    let l = plug_lines(&p, None, Some(&none));
    assert!(!l.iter().any(|x| x.contains("also installed") || x.starts_with("claude config:")), "{l:?}");
    let c = claude_dir(&d.path().join("bad"), "{");
    assert!(plug_lines(&p, None, Some(&c)).iter().any(|x| x.starts_with("claude config: cannot read ")));
}

fn with_server(p: &Paths, server: Option<&str>) -> Vec<String> {
    let m = load(&LoadContext { paths: p.clone(), repo: None, team: None }).unwrap();
    doctor_report(&Env { paths: p, repo: None, search_path: None, claude_config_dir: None, secrets: &MemStore::new(), env: &no_env, probe: &never,
        server: server.map(str::to_string), tmux: Err(MuxError::TimedOut) }, &m)
}

#[test]
fn doctor_report_server_line() {
    let d = TempDir::new().unwrap();
    let p = paths(d.path());
    assert!(has(&with_server(&p, None), "server: local mode"));
    assert!(has(&with_server(&p, Some("https://zygo.example.invalid")), "server: https://zygo.example.invalid"));
    let l = with_server(&p, Some("u\nforged: x"));
    assert!(has(&l, r"server: u\nforged: x"), "{l:?}");
    assert!(!l.iter().any(|x| x.starts_with("forged:")));
}

#[test]
fn doctor_report_repo_policy_line() {
    let _g = forks();
    let d = TempDir::new().unwrap();
    let p = paths(&d.path().join("home"));
    assert_eq!(with_server(&p, None).last().unwrap(), "repo policy: defaults");
    let root = d.path().join("repo");
    std::fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    write(&root, ".zygo.toml", "version = 1\n");
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "init"]);
    let m = load(&LoadContext { paths: p.clone(), repo: Some(root.clone()), team: None }).unwrap();
    let l = doctor_report(&Env { paths: &p, repo: Some(&root), search_path: None, claude_config_dir: None, secrets: &MemStore::new(), env: &no_env,
        probe: &never, server: None, tmux: Err(MuxError::TimedOut) }, &m);
    let last = l.last().unwrap();
    assert_eq!(*last, Policy::resolve(&m, &root).describe());
    assert!(last.starts_with(&format!("repo policy: {}@main (", root.display())), "{last}");
}

#[cfg(unix)]
#[test]
fn doctor_report_policy_line_is_escaped() {
    let d = TempDir::new().unwrap();
    let _g = forks();
    let p = paths(&d.path().join("home"));
    let root = d.path().join("re\npo\u{202e}x");
    std::fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    write(&root, ".zygo.toml", "version = 1\n");
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "init"]);
    let m = load(&LoadContext { paths: p.clone(), repo: Some(root.clone()), team: None }).unwrap();
    let l = doctor_report(&Env { paths: &p, repo: Some(&root), search_path: None, claude_config_dir: None, secrets: &MemStore::new(), env: &no_env,
        probe: &never, server: None, tmux: Err(MuxError::TimedOut) }, &m);
    assert!(!l.iter().any(|x| x.contains('\n') || x.contains('\u{202e}')), "{l:?}");
    let last = l.last().unwrap();
    assert!(last.starts_with("repo policy: ") && last.contains(r"re\npo\u{202e}x@main ("), "{last}");
}

fn icon_lines(p: &Paths, vars: &[(&str, &str)]) -> Vec<String> {
    let m = load(&LoadContext { paths: p.clone(), repo: None, team: None }).unwrap();
    let env = |n: &str| vars.iter().find(|(k, _)| *k == n).map(|(_, v)| OsString::from(v));
    let l = doctor_report(&Env { paths: p, repo: None, search_path: None, claude_config_dir: None, secrets: &MemStore::new(), env: &env,
        probe: &never, server: None, tmux: Err(MuxError::TimedOut) }, &m);
    l.into_iter().filter(|x| x.starts_with("icons: ")).collect()
}

/// The Nerd glyphs of H§6.5, in its order: stripe, logo, reflection, branch, GitHub issue, work graph, bead, worktree,
/// ahead, behind, model, profile, bar filled, bar empty, threshold, notional, cache, trimmed, truncation.
const NERD_GLYPHS: [&str; 19] = ["\u{258c}", "\u{1d607}\u{1d606}\u{1d5f4}\u{1d5fc}", "z\u{28e}\u{183}o", "\u{e0a0}", "\u{f408}", "\u{25c6}",
    "\u{f41b}", "\u{f0645}", "\u{2191}", "\u{2193}", "\u{f4bc}", "\u{f0626}", "\u{25b0}", "\u{25b1}", "\u{21e3}", "\u{2248}", "\u{f01bc}",
    "\u{2702}", "\u{2026}"];

/// H§14 #32, AU2: one line holds the Nerd glyphs of H§6.5 and names the active set.
#[test]
fn h14_32_doctor_icon_line_holds_the_nerd_glyphs_and_names_the_set() {
    let d = TempDir::new().unwrap();
    let p = paths(d.path());
    let test = NERD_GLYPHS.join(" ");
    let nerd = format!("icons: nerd set (ZYGO_NERD_FONT=0 selects the plain set); test: {test}");
    let plain = format!("icons: plain set (ZYGO_NERD_FONT=1 selects the nerd set); test: {test}");
    // The set is the one the status line uses (AU2). `xterm` selects no set by itself on any platform. On a Windows
    // build `WT_SESSION` and `TERM_PROGRAM` select no set either (the PH8 and PH10 fallback), so there the
    // automatic set is plain; elsewhere it is nerd.
    let auto = if cfg!(windows) { &plain } else { &nerd };
    let cases: [(&[(&str, &str)], &String); 8] = [
        (&[("ZYGO_NERD_FONT", "1")], &nerd),
        (&[("ZYGO_NERD_FONT", "1"), ("TERM", "dumb")], &nerd),
        (&[("ZYGO_NERD_FONT", "0")], &plain),
        (&[("ZYGO_NERD_FONT", "0"), ("TERM", "xterm"), ("WT_SESSION", "1")], &plain),
        (&[("TERM", "linux")], &plain),
        (&[("TERM", "dumb"), ("ZYGO_NERD_FONT", "yes")], &plain),
        (&[("TERM", "xterm"), ("TERM_PROGRAM", "x"), ("NO_COLOR", "1")], auto),
        (&[("TERM", "xterm"), ("WT_SESSION", "1")], auto),
    ];
    for (vars, want) in cases {
        assert_eq!(icon_lines(&p, vars), std::slice::from_ref(want), "{vars:?}");
    }
    for g in NERD_GLYPHS {
        assert!(nerd.contains(g) && plain.contains(g), "{g:?}");
    }
    // One line, with no escape byte.
    assert!(!nerd.contains('\n') && !nerd.contains('\x1b'));
}

#[test]
fn h14_32_doctor_cli_prints_the_icon_line() {
    let d = TempDir::new().unwrap();
    let _g = forks();
    for (value, set) in [("1", "icons: nerd set "), ("0", "icons: plain set ")] {
        let o = doctor_cmd(d.path(), &cfg(&[])).env("ZYGO_NERD_FONT", value).output().unwrap();
        let out = String::from_utf8(o.stdout).unwrap();
        assert_eq!(o.status.code(), Some(0), "{}", String::from_utf8_lossy(&o.stderr));
        let icons: Vec<&str> = out.lines().filter(|l| l.starts_with("icons: ")).collect();
        assert_eq!(icons.len(), 1, "{out}");
        assert!(icons[0].starts_with(set) && icons[0].ends_with(&format!("test: {}", NERD_GLYPHS.join(" "))), "{out}");
    }
}

/// W§12, AU7: the tmux line after the act-lite line.
#[test]
fn doctor_tmux_line() {
    let d = TempDir::new().unwrap();
    let p = paths(d.path());
    std::fs::write(&p.config_file, "").unwrap();
    let m = load(&LoadContext { paths: p.clone(), repo: None, team: None }).unwrap();
    let never = |_: &str| Probe::NotFound;
    let line = |tmux: Result<(u32, u32), MuxError>| -> Vec<String> {
        let l = doctor_report(&Env { paths: &p, repo: None, search_path: None, claude_config_dir: None, secrets: &MemStore::new(),
            env: &no_env, probe: &never, server: None, tmux }, &m);
        l.into_iter().filter(|x| x.starts_with("tmux: ")).collect()
    };
    assert_eq!(line(Ok((3, 7))), ["tmux: 3.7 (minimum 3.2)"]);
    assert_eq!(line(Ok((3, 2))), ["tmux: 3.2 (minimum 3.2)"]);
    assert_eq!(line(Ok((3, 1))), ["tmux: 3.1 (minimum 3.2; below minimum)"]);
    assert_eq!(line(Ok((2, 9))), ["tmux: 2.9 (minimum 3.2; below minimum)"]);
    // M1 review round 1 (N2): `not found` only for a spawn error of kind `NotFound`; any other spawn error is named
    let spawn = |e: std::io::Error| line(Err(MuxError::Spawn { message: e.to_string() }));
    assert_eq!(spawn(std::io::Error::from(std::io::ErrorKind::NotFound)), ["tmux: not found"]);
    assert_eq!(spawn(std::io::Error::from_raw_os_error(2)), ["tmux: not found"]); // ENOENT; ERROR_FILE_NOT_FOUND on Windows
    let denied = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
    assert_eq!(spawn(denied), [format!("tmux: cannot run: {}", std::io::Error::from(std::io::ErrorKind::PermissionDenied))]);
    #[cfg(unix)]
    assert_eq!(spawn(std::io::Error::from_raw_os_error(13)), ["tmux: cannot run: Permission denied (os error 13)"]);
    assert_eq!(line(Err(MuxError::Spawn { message: "No such file or directory".into() })), ["tmux: cannot run: No such file or directory"]);
    assert_eq!(line(Err(MuxError::Spawn { message: "no pipes\u{1b}[2K\nforged".into() })), ["tmux: cannot run: no pipes\\u{1b}[2K\\nforged"]);
    assert_eq!(line(Err(MuxError::TimedOut)), ["tmux: unknown (minimum 3.2)"]);
    assert_eq!(line(Err(MuxError::Failed { message: "unknown tmux version: tmux master".into() })), ["tmux: unknown (minimum 3.2)"]);
}

#[test]
fn doctor_report_section_order() {
    let d = TempDir::new().unwrap();
    let p = paths(d.path());
    std::fs::write(&p.config_file, cfg(&[("acme", "key = \"keychain:acme\"")])).unwrap();
    let l = with_server(&p, Some("https://zygo.example.invalid"));
    let at = |pre: &str| l.iter().position(|x| x.starts_with(pre)).unwrap_or_else(|| panic!("{pre}: {l:?}"));
    let pos: Vec<usize> = ["agent claude-code", "agent opencode", "act-lite", "tmux: ", "secret store", "provider ", "server", "plugin store", "icons: ", "repo policy"].iter().map(|x| at(x)).collect();
    assert!(pos.windows(2).all(|w| w[0] < w[1]), "{pos:?}");
    assert_eq!(*pos.last().unwrap(), l.len() - 1);
}

#[test]
fn doctor_cli_invalid_config_closed_stderr() {
    let _g = forks();
    let d = TempDir::new().unwrap();
    std::fs::write(d.path().join("config.toml"), "not = = toml").unwrap();
    let cmd = |stderr: Stdio| Command::new(env!("CARGO_BIN_EXE_zygo")).arg("doctor").env("HOME", d.path()).env("LOCALAPPDATA", d.path())
        .env("APPDATA", d.path()).env("ZYGO_CONFIG", d.path().join("config.toml")).env_remove("ZYGO_SESSION_DIR").env_remove("ZYGO_AGENT")
        .current_dir(d.path()).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(stderr).output().unwrap();
    let (r, w) = std::io::pipe().unwrap();
    drop(r);
    let o = cmd(w.into());
    assert_eq!(o.status.code(), Some(11));
    let o = cmd(Stdio::piped());
    assert_eq!(o.status.code(), Some(11));
    assert!(String::from_utf8_lossy(&o.stderr).starts_with("zygo doctor: "), "{}", String::from_utf8_lossy(&o.stderr));
}

/// `zygo doctor` in a temporary home whose `config.toml` holds `config`, with an empty `PATH` directory (no probe finds
/// a program).
fn doctor_cli(d: &Path, config: &str) -> std::process::Output {
    doctor_cmd(d, config).output().unwrap()
}

/// The command of `doctor_cli`, not yet run.
fn doctor_cmd(d: &Path, config: &str) -> Command {
    std::fs::write(d.join("config.toml"), config).unwrap();
    let bin = d.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_zygo"));
    cmd.arg("doctor").env("HOME", d).env("LOCALAPPDATA", d).env("APPDATA", d)
        .env("ZYGO_CONFIG", d.join("config.toml")).env("PATH", &bin).env("CLAUDE_CONFIG_DIR", d.join("claude"))
        .env_remove("ZYGO_SESSION_DIR").env_remove("ZYGO_AGENT").current_dir(d).stdin(Stdio::null());
    cmd
}

#[test]
fn doctor_cli_bad_key_ref_is_not_echoed() {
    let d = TempDir::new().unwrap();
    let _g = forks();
    let value = format!("sk-ant-{}", ulid::Ulid::generate());
    let o = doctor_cli(d.path(), &cfg(&[("acme", &format!("key = \"{value}\""))]));
    let (out, err) = (String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    assert_eq!(o.status.code(), Some(11), "{err}");
    assert!(err.starts_with("zygo doctor: ") && err.contains("expected env:<NAME> or keychain:<name>"), "{err}");
    assert!(!out.contains(&value) && !err.contains(&value), "{out}{err}");
}

#[test]
fn doctor_cli_valid_config_exits_zero() {
    let d = TempDir::new().unwrap();
    let _g = forks();
    let o = doctor_cli(d.path(), &cfg(&[("acme", "key = \"env:ZYGO_N35_KEY\"")]));
    let out = String::from_utf8_lossy(&o.stdout);
    assert_eq!(o.status.code(), Some(0), "{}", String::from_utf8_lossy(&o.stderr));
    let l: Vec<String> = out.lines().map(str::to_string).collect();
    assert!(has(&l, "agent claude-code: not found") && has(&l, "act-lite: not found"), "{out}");
    assert!(has(&l, "tmux: not found"), "{out}");
    assert!(l.iter().any(|x| x.starts_with("provider acme: env:ZYGO_N35_KEY ")), "{out}");
    assert_eq!(l.last().map(String::as_str), Some("repo policy: defaults"), "{out}");
}

/// D2 (plan review F7): `zygo doctor` looks for tmux only in the absolute entries of `PATH`. Kills: a `find_tmux` that
/// searches a relative `PATH` entry (the command's cwd is `d`, so `.` names the folder of the relative fake).
#[cfg(unix)]
#[test]
fn doctor_tmux_is_found_only_in_absolute_path_entries() {
    use std::os::unix::fs::PermissionsExt;
    let d = TempDir::new().unwrap();
    let _g = forks();
    let used = d.path().join("relative-used");
    let write = |f: &Path, text: &str| {
        std::fs::write(f, text).unwrap();
        std::fs::set_permissions(f, std::fs::Permissions::from_mode(0o755)).unwrap();
    };
    write(&d.path().join("tmux"), &format!("#!/bin/sh\ntouch \"{}\"\necho \"tmux 9.9\"\n", used.display()));
    let abs = d.path().join("abs");
    std::fs::create_dir_all(&abs).unwrap();
    write(&abs.join("tmux"), "#!/bin/sh\necho \"tmux 3.7\"\n");
    let run = |path: OsString| -> Vec<String> {
        let o = doctor_cmd(d.path(), "").env("PATH", path).output().unwrap();
        String::from_utf8_lossy(&o.stdout).lines().map(str::to_string).collect()
    };
    let mut both = OsString::from(".:");
    both.push(&abs);
    let l = run(both);
    assert!(has(&l, "tmux: 3.7 (minimum 3.2)"), "{l:?}");
    assert!(!used.exists(), "the relative fake ran");
    let l = run(OsString::from("."));
    assert!(has(&l, "tmux: not found"), "{l:?}");
    assert!(!used.exists(), "the relative fake ran");
}

/// The `tmux` lines of `zygo doctor` with `PATH` = the one absolute folder `dir` (D2, D9).
#[cfg(unix)]
fn doctor_tmux_lines(d: &Path, dir: &Path) -> Vec<String> {
    let o = doctor_cmd(d, "").env("PATH", dir).output().unwrap();
    String::from_utf8_lossy(&o.stdout).lines().filter(|x| x.starts_with("tmux: ")).map(str::to_string).collect()
}

/// D2: a file named `tmux` that is not executable, in an absolute `PATH` entry, is not tmux. Kills: a `find_tmux` that
/// takes the first entry named `tmux` that exists, with no executable check (`dir.join("tmux").exists()`): the doctor
/// would then spawn it and print `tmux: cannot run: Permission denied (os error 13)`.
#[cfg(unix)]
#[test]
fn doctor_tmux_skips_a_file_that_is_not_executable() {
    use std::os::unix::fs::PermissionsExt;
    let d = TempDir::new().unwrap();
    let _g = forks();
    let dir = d.path().join("abs");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("tmux"), "#!/bin/sh\necho \"tmux 3.7\"\n").unwrap();
    std::fs::set_permissions(dir.join("tmux"), std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(doctor_tmux_lines(d.path(), &dir), ["tmux: not found"]);
}

/// D2: a directory named `tmux`, in an absolute `PATH` entry, is not tmux. Kills: a `find_tmux` that takes the first
/// entry named `tmux` that exists (`dir.join("tmux").exists()`), or one that checks only the execute bits of the mode
/// (a directory has them): the doctor would then spawn the directory and print `tmux: cannot run: Permission denied
/// (os error 13)`.
#[cfg(unix)]
#[test]
fn doctor_tmux_skips_a_directory() {
    let d = TempDir::new().unwrap();
    let _g = forks();
    let dir = d.path().join("abs");
    std::fs::create_dir_all(dir.join("tmux")).unwrap();
    assert_eq!(doctor_tmux_lines(d.path(), &dir), ["tmux: not found"]);
}

#[test]
fn doctor_cli_toml_syntax_error_is_not_echoed() {
    let d = TempDir::new().unwrap();
    let _g = forks();
    let value = format!("sk-ant-{}", ulid::Ulid::generate());
    let o = doctor_cli(d.path(), &format!("[user]\nkey = {value}\n"));
    let (out, err) = (String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    assert_eq!(o.status.code(), Some(11), "{err}");
    assert!(err.starts_with("zygo doctor: ") && err.contains("TOML parse error at line 2, column "), "{err}");
    assert!(!out.contains(&value) && !err.contains(&value), "{out}{err}");
}

#[test]
fn doctor_cli_type_error_is_not_echoed() {
    let d = TempDir::new().unwrap();
    let _g = forks();
    let value = format!("sk-ant-{}", ulid::Ulid::generate());
    let o = doctor_cli(d.path(), &cfg(&[]).replace("context_window = 200000", &format!("context_window = \"{value}\"")));
    let (out, err) = (String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    assert_eq!(o.status.code(), Some(11), "{err}");
    assert!(err.starts_with("zygo doctor: ") && err.contains("model[0].facts.context_window at line 6: expected u64, found a string"), "{err}");
    assert!(!out.contains(&value) && !err.contains(&value), "{out}{err}");
}

#[test]
fn doctor_and_status_never_print_a_marketplace_source() {
    let d = TempDir::new().unwrap();
    let _g = forks();
    let tk = format!("tk{}", ulid::Ulid::generate());
    let config = cfg(&[]) + &format!("[[marketplace]]\nname = \"a\"\nsource = \"git+https://bob:{tk}@h.example/o/r.git\"\nref = \"v1.0.0\"\n");
    let o = doctor_cli(d.path(), &config);
    let (out, err) = (String::from_utf8_lossy(&o.stdout).into_owned(), String::from_utf8_lossy(&o.stderr).into_owned());
    assert!(!out.contains(&tk) && !err.contains(&tk), "doctor: {out}{err}");
    let o = Command::new(env!("CARGO_BIN_EXE_zygo")).arg("status").env("HOME", d.path()).env("LOCALAPPDATA", d.path()).env("APPDATA", d.path())
        .env("ZYGO_CONFIG", d.path().join("config.toml")).env("PATH", d.path().join("bin")).env("CLAUDE_CONFIG_DIR", d.path().join("claude"))
        .env_remove("ZYGO_SESSION_DIR").env_remove("ZYGO_AGENT").current_dir(d.path()).stdin(Stdio::null()).output().unwrap();
    let (out, err) = (String::from_utf8_lossy(&o.stdout).into_owned(), String::from_utf8_lossy(&o.stderr).into_owned());
    assert!(!out.contains(&tk) && !err.contains(&tk), "status: {out}{err}");
}
