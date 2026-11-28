use serde_json::Value;
use zygo_core::{
    hook::{guarded, HookInput, HookOutput},
    overrides::Overrides,
    session_dir::SessionDir,
    session_view::*,
    Agent,
};
use zygo_hooks::{
    mail, pre_compact, session_start,
    session_text::{build, mechanics},
};
use zygo_work::log::{lock, Begin};
use zygo_workspace::state::{MemberRecord, Workspace};
#[cfg(unix)]
use std::time::{Duration, SystemTime};
#[cfg(unix)]
use zygo_core::paths::Paths;
#[cfg(unix)]
use zygo_workspace::state::{Phase, Store};

const REDACT: &str = "`[REDACTED:<id>]` marks a secret zygo removed; never write it into a file.";
const ATTRIBUTION: &str = "Never add Claude attribution to commits, pull request titles or bodies, tags, release notes or comments: no `Co-Authored-By: Claude` trailer, no `noreply@anthropic.com` address, no `Generated with Claude Code` line and no Claude session link.";
const STORE: &str = "Large tool outputs are stored by zygo; read them with result_view, result_search or result_tree using the id in the marker; do not shorten output with head or tail.";
const NAV: &str = "Use act-lite's skeleton, symbols or repo_outline before reading a whole source file.";
const TRAILER: &str = "Add the commit trailers `Zygo-Session: <session>` and, when your brief names a unit, `Zygo-Work: <unit>`, each on its own line at the end of the message.";
const HEAD: &str = "A skill names a role or a capability: pick the subagent by the legend.";
const TAIL: &str = "Do not set a model on the subagent call.";
const OPUS: &str = "`opus` is the premium subagent that fits the task";
fn alias(clauses: &str) -> String {
    format!("{HEAD} When a skill still names a model, use the subagent instead: {clauses}. {TAIL}")
}
const READ: &str = "`haiku` is `zygo-read`";
const CODE: &str = "`sonnet` is `zygo-code`";
const DESIGN: &str = "`fable` is `zygo-design`";
const PTR: &str = "(`zygo-plan`, `zygo-triage` or `zygo-review`)";
const CAPS: &str = "`lookup` is `zygo-read`; `mechanical` and `implementation` are `zygo-code`; `judgment` is `zygo-design` for a design question and `zygo-plan`, `zygo-triage` or `zygo-review` for text.";
const KEEP_NO_UNIT: &str = "Keep in the summary: the plan or tracker path and the current step; the pending briefs and what each subagent reported; the result ids zygo stored (result_view reads them again). Drop: tool output that is in the result store; file contents already edited and committed.";
fn keep_units(u: &str) -> String {
    format!("Keep in the summary: the plan or tracker path and the current step; every open unit of work with its id, round, check and files: {u}; the pending briefs and what each subagent reported; the result ids zygo stored (result_view reads them again). Drop: tool output that is in the result store; file contents already edited and committed.")
}

fn view() -> SessionView {
    SessionView {
        session: "S1".into(),
        agent: Agent::ClaudeCode,
        main_role: "lead".into(),
        effective_role: None,
        role_prompt: Some("ROLE".into()),
        user_prompt: None,
        profile: None,
        profile_prompt: None,
        work_item: None,
        legend: vec![],
        disabled_roles: vec![],
        work_units: true,
        results: true,
        compact_tool: true,
        navigation: false,
        trim_threshold_bytes: 16384,
        head_lines: 60,
        tail_lines: 20,
        ..Default::default()
    }
}
fn off(r: &[&str]) -> SessionView {
    SessionView { disabled_roles: r.iter().map(|s| s.to_string()).collect(), legend: abc(), ..view() }
}
fn abc() -> Vec<LegendEntry> {
    vec![entry("design", "D.", false), entry("read", "R.", false), entry("code", "C.", false)]
}
fn entry(role: &str, d: &str, custom: bool) -> LegendEntry {
    LegendEntry { role: role.into(), description: d.into(), custom }
}
fn sd() -> (tempfile::TempDir, SessionDir) {
    let t = tempfile::tempdir().unwrap();
    let d = SessionDir::new(t.path().join("s"));
    d.create().unwrap();
    (t, d)
}
fn begin(unit: &str, round: u32, check: &str, files: &[&str], stopped_at: Option<i64>) -> Begin {
    Begin {
        unit: unit.into(),
        round,
        check: check.into(),
        files: files.iter().map(|s| s.to_string()).collect(),
        began_at: 0,
        offset: 0,
        session: "s".into(),
        agent_type: None,
        stopped_at,
        edit_root: None,
    }
}
fn open(d: &SessionDir, agent: &str, b: Begin) {
    lock(&d.work(), agent).unwrap().write_begin(Some(&b)).unwrap();
}
fn hook(name: &str) -> HookInput {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/claude-code/hooks").join(format!("{name}.json"));
    HookInput::parse(Agent::ClaudeCode, &std::fs::read(p).unwrap()).unwrap()
}
fn text(o: anyhow::Result<HookOutput>) -> String {
    o.unwrap().to_stdout()
}
fn m() -> String {
    [REDACT, ATTRIBUTION, STORE, TRAILER].join(" ")
}
fn full_alias() -> String {
    alias(&format!("{READ}, {CODE}, {OPUS} {PTR}, {DESIGN}"))
}

#[test]
fn mechanics_exact_all_on() {
    let v = SessionView { navigation: true, ..view() };
    assert_eq!(mechanics(&v), [REDACT, ATTRIBUTION, STORE, NAV, TRAILER].join(" "));
}

#[test]
fn mechanics_navigation_only_when_present() {
    let t = mechanics(&view());
    assert_eq!(t, m());
    assert!(!t.contains("act-lite"));
    assert!(mechanics(&SessionView { navigation: true, ..view() }).contains(NAV));
}

#[test]
fn mechanics_store_sentence_only_when_results_on() {
    assert_eq!(mechanics(&SessionView { results: false, ..view() }), [REDACT, ATTRIBUTION, TRAILER].join(" "));
}

#[test]
fn mechanics_trailer_only_when_work_units_on() {
    assert_eq!(mechanics(&SessionView { work_units: false, ..view() }), [REDACT, ATTRIBUTION, STORE].join(" "));
    assert_eq!(mechanics(&SessionView { work_units: false, results: false, ..view() }), [REDACT, ATTRIBUTION].join(" "));
}

#[test] // FIX-21, §8.6 item 7: kills: the attribution sentence behind a view flag, on one agent only, or out of the session text
fn mechanics_always_forbid_claude_attribution() {
    for agent in [Agent::ClaudeCode, Agent::OpenCode] {
        for bits in 0..8u8 {
            let (results, navigation, work_units) = (bits & 1 != 0, bits & 2 != 0, bits & 4 != 0);
            let v = SessionView { agent, results, navigation, work_units, ..view() };
            assert!(mechanics(&v).starts_with(&format!("{REDACT} {ATTRIBUTION}")), "{agent:?} {bits}");
            assert!(build(&v).contains(ATTRIBUTION), "{agent:?} {bits}");
        }
    }
}

#[test]
fn session_text_default_roles() {
    let d = zygo_config::builtin::defaults();
    let lead = d.role["lead"].prompt.clone();
    assert!(lead.starts_with("You are the lead of this session: the elephant of the elephant-goldfish model."));
    assert!(lead.ends_with("Running a check to confirm a subagent's report is yours."));
    let order = ["design", "plan", "triage", "review", "code", "read"];
    let v = SessionView {
        role_prompt: Some(lead.clone()),
        legend: order.iter().map(|r| entry(r, &d.role[*r].description, false)).collect(),
        ..view()
    };
    let t = build(&v);
    assert!(t.starts_with(&lead));
    for r in order {
        assert!(t.contains(&format!("zygo-{r}: {}", d.role[r].description)), "{r}");
    }
    assert!(t.contains(&full_alias()));
    assert!(t.contains(CAPS));
    assert!(!t.contains("zygo-lead"));
}

#[test]
fn session_text_disabled_design() {
    let v = off(&["design"]);
    let t = build(&v);
    let want = [
        "ROLE",
        "zygo-read: R.\nzygo-code: C.",
        &alias(&format!("{READ}, {CODE}, {OPUS} {PTR}")),
        "`lookup` is `zygo-read`; `mechanical` and `implementation` are `zygo-code`; `judgment` is `zygo-plan`, `zygo-triage` or `zygo-review` for text.",
        &m(),
    ]
    .join("\n\n");
    assert_eq!(t, want);
    assert!(!t.contains("zygo-design"));
    assert!(!t.contains("fable"));
}

#[test]
fn session_text_disabled_read() {
    let want = [
        "ROLE",
        "zygo-design: D.\nzygo-code: C.",
        &alias(&format!("{CODE}, {OPUS} {PTR}, {DESIGN}")),
        "`mechanical` and `implementation` are `zygo-code`; `judgment` is `zygo-design` for a design question and `zygo-plan`, `zygo-triage` or `zygo-review` for text.",
        &m(),
    ]
    .join("\n\n");
    assert_eq!(build(&off(&["read"])), want);
}

#[test]
fn session_text_disabled_triage_review() {
    let want = [
        "ROLE",
        "zygo-design: D.\nzygo-read: R.\nzygo-code: C.",
        &alias(&format!("{READ}, {CODE}, {OPUS} (`zygo-plan`), {DESIGN}")),
        "`lookup` is `zygo-read`; `mechanical` and `implementation` are `zygo-code`; `judgment` is `zygo-design` for a design question and `zygo-plan` for text.",
        &m(),
    ]
    .join("\n\n");
    assert_eq!(build(&off(&["triage", "review"])), want);
}

#[test]
fn session_text_all_roles_disabled() {
    let v = off(&["read", "code", "design", "plan", "triage", "review"]);
    let want = ["ROLE", &format!("{HEAD} {TAIL}"), &m()].join("\n\n");
    assert_eq!(build(&v), want);
}

#[test]
fn session_text_main_role_excluded() {
    let v = SessionView { main_role: "code".into(), legend: abc(), ..view() };
    let t = build(&v);
    assert!(!t.contains("zygo-code"));
    assert!(!t.contains("sonnet"));
    assert!(!t.contains("mechanical"));
    assert!(t.contains("haiku"));
    let v = SessionView { main_role: "plan".into(), legend: abc(), ..view() };
    assert!(build(&v).contains("(`zygo-triage` or `zygo-review`)"));
}

#[test]
fn session_text_section_order() {
    let v = SessionView {
        user_prompt: Some("USER".into()),
        profile_prompt: Some("PROFILE".into()),
        work_item: Some(WorkItemView { unit: "gh:42".into(), title: Some("Fix it".into()) }),
        navigation: true,
        legend: vec![entry("bench", "Runs benchmarks.", true), entry("read", "Reads.", false), entry("code", "Codes.", false)],
        ..view()
    };
    let want = [
        "ROLE",
        "USER",
        "PROFILE",
        "Work item: gh:42 — Fix it",
        "zygo-read: Reads.\nzygo-code: Codes.\nzygo-bench: Runs benchmarks.",
        &full_alias(),
        CAPS,
        &mechanics(&v),
    ]
    .join("\n\n");
    assert_eq!(build(&v), want);
}

#[test]
fn session_text_work_item_without_title() {
    for title in [None, Some(String::new())] {
        let v = SessionView { work_item: Some(WorkItemView { unit: "gh:42".into(), title }), ..view() };
        let t = build(&v);
        assert!(t.lines().any(|l| l == "Work item: gh:42"));
        assert!(!t.contains('—'));
    }
}

#[test]
fn session_text_empty_items_leave_no_blank_sections() {
    let mut v = SessionView {
        role_prompt: Some(String::new()),
        user_prompt: Some(String::new()),
        profile_prompt: Some(String::new()),
        work_item: None,
        legend: vec![],
        ..view()
    };
    let t = build(&v);
    assert_eq!(t, [&full_alias(), CAPS, &m()].join("\n\n"));
    assert!(!t.contains("\n\n\n"));
    assert!(!t.starts_with('\n'));
    v.role_prompt = None;
    v.user_prompt = Some("USER".into());
    assert!(build(&v).starts_with("USER\n\n"));
}

#[test]
fn session_text_legend_empty_description_prints_name_only() {
    let v = SessionView { legend: vec![entry("read", "", false), entry("code", " \t", false), entry("design", "D.", false)], ..view() };
    assert!(build(&v).contains("\n\nzygo-read\nzygo-code\nzygo-design: D.\n\n"));
}

#[test]
fn session_text_opencode_no_alias() {
    let v = SessionView { agent: Agent::OpenCode, ..view() };
    let t = build(&v);
    assert!(!t.contains("A skill names a role"));
    assert!(!t.contains("fable"));
    assert!(t.contains(CAPS));
}

#[test]
fn session_text_no_window_size_price_or_token() {
    let d = zygo_config::builtin::defaults();
    let order = ["design", "plan", "triage", "review", "code", "read"];
    let v = SessionView {
        role_prompt: Some(d.role["lead"].prompt.clone()),
        legend: order.iter().map(|r| entry(r, &d.role[*r].description, false)).collect(),
        user_prompt: Some("Prefer small commits.".into()),
        profile_prompt: Some("Prefer small commits.".into()),
        navigation: true,
        ..view()
    };
    let (_t, dir) = sd();
    open(&dir, "main", begin("gh:1", 2, "cargo test", &["src/"], None));
    let re = regex::Regex::new(r"\d+[km]\b").unwrap();
    for out in [build(&v), text(pre_compact::run_in(Some(&dir)))] {
        let l = out.to_lowercase();
        assert!(!l.is_empty());
        for w in ["window", "size", "price", "cost", "token"] {
            assert!(!l.contains(w), "{w}");
        }
        assert!(!re.is_match(&l));
    }
}

#[test]
fn pre_compact_without_open_unit() {
    assert_eq!(text(pre_compact::run_in(None)), KEEP_NO_UNIT);
    let (_t, d) = sd();
    assert_eq!(text(pre_compact::run_in(Some(&d))), KEEP_NO_UNIT);
}

#[test]
fn pre_compact_one_open_unit() {
    let (_t, d) = sd();
    open(&d, "main", begin("gh:1", 2, "cargo test", &["src/a.rs", "docs/"], None));
    assert_eq!(
        text(pre_compact::run_in(Some(&d))),
        keep_units("gh:1 (round 2, check: `cargo test`, files: `src/a.rs`, `docs/`)")
    );
}

#[test]
fn pre_compact_two_units_and_stopped_unit() {
    let (_t, d) = sd();
    // File-name order (a1, main) is the reverse of the unit order (a:1, b:2): only the sort gives the expected order.
    open(&d, "a1", begin("b:2", 1, "", &[], None));
    open(&d, "main", begin("a:1", 1, "make", &["x"], None));
    open(&d, "a9", begin("z:9", 1, "t", &[], Some(5)));
    assert_eq!(
        text(pre_compact::run_in(Some(&d))),
        keep_units("a:1 (round 1, check: `make`, files: `x`); b:2 (round 1, check: none, files: none)")
    );
}

#[test]
fn pre_compact_sorts_by_unit_round_then_agent() {
    let (_t, d) = sd();
    // File-name order (a, b, c, x, y) differs from the unit order. u:1 round 2 sorts before u:2 round 1, so a sort by
    // round only fails; x and y tie on (unit, round) and sort by the agent stem.
    open(&d, "a", begin("u:2", 1, "a", &[], None));
    open(&d, "b", begin("u:1", 2, "b", &[], None));
    open(&d, "c", begin("u:1", 1, "c", &[], None));
    open(&d, "y", begin("u:3", 1, "y", &[], None));
    open(&d, "x", begin("u:3", 1, "x", &[], None));
    assert_eq!(
        text(pre_compact::run_in(Some(&d))),
        keep_units(
            "u:1 (round 1, check: `c`, files: none); u:1 (round 2, check: `b`, files: none); \
             u:2 (round 1, check: `a`, files: none); u:3 (round 1, check: `x`, files: none); \
             u:3 (round 1, check: `y`, files: none)"
        )
    );
}

#[test]
fn pre_compact_fields_with_backticks_use_a_longer_fence() {
    let (_t, d) = sd();
    open(&d, "main", begin("gh:5", 1, "echo `date`; x (y)", &["a`b.rs", "`x`", "p``q"], None));
    assert_eq!(
        text(pre_compact::run_in(Some(&d))),
        keep_units("gh:5 (round 1, check: ``echo `date`; x (y)``, files: ``a`b.rs``, `` `x` ``, ```p``q```)")
    );
}

#[test]
fn pre_compact_fields_with_line_breaks_become_one_line() {
    let (_t, d) = sd();
    open(&d, "main", begin("u\n2", 1, "line1\nline2", &["r\r\ns.rs"], None));
    let out = text(pre_compact::run_in(Some(&d)));
    assert_eq!(out, keep_units("u 2 (round 1, check: `line1 line2`, files: `r  s.rs`)"));
    assert!(!out.contains('\n') && !out.contains('\r'));
}

#[test]
fn pre_compact_check_with_comma_and_semicolon() {
    let (_t, d) = sd();
    open(&d, "main", begin("gh:3", 1, "a, b; c", &[], None));
    assert_eq!(text(pre_compact::run_in(Some(&d))), keep_units("gh:3 (round 1, check: `a, b; c`, files: none)"));
}

#[test]
fn pre_compact_prints_for_subagent_input() {
    for n in ["PreCompact-sub", "PreCompact-main"] {
        let want_agent = n.ends_with("sub");
        assert_eq!(hook(n).has_agent_id, want_agent);
        assert_eq!(guarded(&|_| pre_compact::run_in(None), b""), (KEEP_NO_UNIT.to_string(), 0));
    }
}

#[test]
fn session_start_json_envelope_claude_code() {
    let (_t, d) = sd();
    let v = view();
    v.save(&d).unwrap();
    let out = text(session_start::run_in(&hook("SessionStart-main"), &d));
    let got: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(
        got,
        serde_json::json!({"hookSpecificOutput": {"hookEventName": "SessionStart", "additionalContext": build(&v)}})
    );
    assert!(!got["hookSpecificOutput"]["additionalContext"].as_str().unwrap().is_empty());
}

#[test]
fn session_start_text_for_opencode() {
    let (_t, d) = sd();
    let v = SessionView { agent: Agent::OpenCode, ..view() };
    v.save(&d).unwrap();
    let input = HookInput::parse(Agent::OpenCode, b"{}").unwrap();
    assert_eq!(text(session_start::run_in(&input, &d)), build(&v));
}

#[test]
fn session_start_skips_subagent_input() {
    let (_t, d) = sd();
    view().save(&d).unwrap();
    assert!(hook("SessionStart-sub").has_agent_id);
    assert_eq!(text(session_start::run_in(&hook("SessionStart-sub"), &d)), "");
    assert!(!text(session_start::run_in(&hook("SessionStart-main"), &d)).is_empty());
}

fn context(d: &SessionDir) -> String {
    let out = text(session_start::run_in(&hook("SessionStart-main"), d));
    let v: Value = serde_json::from_str(&out).unwrap();
    v["hookSpecificOutput"]["additionalContext"].as_str().unwrap().to_string()
}

#[test]
fn session_start_results_off_in_view() {
    let (_t, d) = sd();
    SessionView { results: false, ..view() }.save(&d).unwrap();
    let c = context(&d);
    assert!(c.contains(REDACT));
    assert!(!c.contains("Large tool outputs"));
}

#[test]
fn session_start_results_off_in_overrides() {
    let (_t, d) = sd();
    SessionView { results: true, ..view() }.save(&d).unwrap();
    Overrides { results: Some(false), ..Default::default() }.save(&d).unwrap();
    assert!(!context(&d).contains("Large tool outputs"));
    SessionView { results: false, ..view() }.save(&d).unwrap();
    Overrides { results: Some(true), ..Default::default() }.save(&d).unwrap();
    assert!(context(&d).contains(STORE));
}

#[test]
fn session_start_work_units_override() {
    let (_t, d) = sd();
    SessionView { work_units: true, ..view() }.save(&d).unwrap();
    Overrides { work_units: Some(false), ..Default::default() }.save(&d).unwrap();
    assert!(!context(&d).contains("Zygo-Session"));
}

const MAIL_TAIL: &str = "Send with mail_send (to a name, or `all`); read with mail_read; mail_list shows unread mail and the current members. zygo tells you when mail arrives: before your turn ends, at your next prompt, or with a wake line. Do not poll or wait for mail: end your turn, and you will be woken.";
const WS_ID: &str = "0123456789abcdef";
const OWN: &str = "kaaaaaaaa";
const PEER: &str = "kbbbbbbbb";
const LATE: &str = "kcccccccc";
#[cfg(unix)]
const U1: &str = "Units u-1 were closed when zygo stopped this session; call work_begin to open them again if work continues.";

fn item8(first: &str, names: &str) -> String {
    format!("Workspace mail: {first}. Members: `{names}`. {MAIL_TAIL}")
}
fn rec(name: Option<&str>) -> MemberRecord {
    MemberRecord { name: name.map(String::from), ..MemberRecord::new(Agent::ClaudeCode) }
}
fn ws_of(name: &str, members: &[(&str, Option<&str>)]) -> Workspace {
    let mut ws = Workspace::new(WS_ID);
    ws.name = name.into();
    ws.members = members.iter().map(|(k, n)| (k.to_string(), rec(*n))).collect();
    ws
}

#[cfg(unix)]
struct Mx {
    t: tempfile::TempDir,
    paths: Paths,
    dir: SessionDir,
}
#[cfg(unix)]
fn mx(agent: Agent, mail: bool, member: bool, note: Option<ResumeNote>, members: &[(&str, Option<&str>)]) -> Mx {
    let t = tempfile::tempdir().unwrap();
    let p = t.path();
    let paths = Paths { config_file: p.join("config.toml"), cache_dir: p.join("cache"), data_dir: p.join("data"),
        sessions_dir: p.join("cache/sessions"), marketplaces_dir: p.join("cache/marketplaces"),
        plugin_store: p.join("data/plugins"), approvals_file: p.join("data/approvals.toml"), usage_db: p.join("data/usage.db") };
    let dir = SessionDir::new(paths.sessions_dir.join("S1"));
    dir.create().unwrap();
    let workspace = member.then(|| ViewWorkspace {
        member: Member { ws_id: WS_ID.into(), pane: OWN.into(), ..Default::default() },
        resume_note: note,
        ..Default::default()
    });
    SessionView { agent, mail, workspace, ..view() }.save(&dir).unwrap();
    let x = Mx { t, paths, dir };
    set_members(&x, members);
    x
}
#[cfg(unix)]
fn set_members(x: &Mx, members: &[(&str, Option<&str>)]) {
    let root = x.t.path().join("app");
    Store::open(&x.paths, WS_ID)
        .update(Duration::from_secs(5), &mut |ws| {
            ws.name = "app".into();
            ws.tmux_session = "zygo-app".into();
            ws.common_dir = root.join(".git");
            ws.main_root = root.clone();
            ws.loadout = "solo".into();
            ws.phase = Phase::Up;
            ws.members = members.iter().map(|(k, n)| (k.to_string(), rec(*n))).collect();
            Ok(())
        })
        .unwrap();
}
#[cfg(unix)]
fn member_text(x: &Mx, input: &HookInput) -> String {
    let mut out = Vec::new();
    let o = session_start::run_member(input, &x.dir, &x.paths, &x.t.path().join("missing"), &mut out).unwrap();
    let ctx = |v: &Value| v["hookSpecificOutput"]["additionalContext"].as_str().unwrap().to_string();
    match o {
        HookOutput::None => ctx(&serde_json::from_slice(&out).unwrap()),
        HookOutput::Json(v) => ctx(&v),
        HookOutput::Text(t) => t,
    }
}
#[cfg(unix)]
fn start(agent: Agent) -> HookInput {
    match agent {
        Agent::ClaudeCode => hook("SessionStart-main"),
        _ => HookInput::parse(Agent::OpenCode, b"{}").unwrap(),
    }
}
const ORCHESTRATOR_JOB: &str = "Your job: orchestrator. Give work to the workers with mail_send, collect their results with mail_read, and check each result against its brief.";
const JOB_LIST: &str = "orchestrator (orchestrator), worker-1 (worker), elena";
fn worker_job(o: &str) -> String {
    format!("Your job: worker. Take your work from {o} by mail, do it through your zygo-<role> subagents, and send the result to {o} with mail_send.")
}
/// `ws_of("app", ..)` with `launch.job` of each member: (key, name, job).
fn ws_jobs(members: &[(&str, Option<&str>, Option<&str>)]) -> Workspace {
    let mut ws = ws_of("app", &members.iter().map(|(k, n, _)| (*k, *n)).collect::<Vec<_>>());
    for (k, _, j) in members {
        ws.members.get_mut(*k).unwrap().launch.job = j.map(String::from);
    }
    ws
}
/// R§9.3's example: an orchestrator, a worker and a member with no job, in key order.
fn team() -> Workspace {
    ws_jobs(&[(OWN, Some("orchestrator"), Some("orchestrator")), (PEER, Some("worker-1"), Some("worker")), (LATE, Some("elena"), None)])
}
#[cfg(unix)]
fn set_jobs(x: &Mx, jobs: &[(&str, &str)]) {
    Store::open(&x.paths, WS_ID)
        .update(Duration::from_secs(5), &mut |ws| {
            for (k, j) in jobs {
                ws.members.get_mut(*k).unwrap().launch.job = Some(j.to_string());
            }
            Ok(())
        })
        .unwrap();
}

#[test] // R§9.3 effect 3, R§18 item 19
fn job_worker_text_lists_each_job_and_names_the_orchestrator() {
    let want = format!("{} {}", item8("you are `worker-1` in workspace `app`", JOB_LIST), worker_job("orchestrator"));
    assert_eq!(mail::session_item(&team(), PEER), Some(want));
    // The S§10.4 clause keeps its place; the job sentence stays last.
    let got = mail::session_item_with(&team(), PEER, &["bob@b".to_string()]).unwrap();
    let tail = format!("Members: `{JOB_LIST}`. Other machines: bob@b; mail them by the full address. {MAIL_TAIL} {}", worker_job("orchestrator"));
    assert!(got.ends_with(&tail), "{got}");
}

#[test] // R§9.3 effect 3
fn job_orchestrator_text_gets_the_orchestrator_sentence() {
    let want = format!("{} {ORCHESTRATOR_JOB}", item8("you are `orchestrator` in workspace `app`", JOB_LIST));
    assert_eq!(mail::session_item(&team(), OWN), Some(want));
}

#[test] // R§9.3: no orchestrator in the workspace
fn job_worker_with_no_orchestrator_names_the_member_that_gave_you_the_work() {
    let ws = ws_jobs(&[(OWN, Some("worker-1"), Some("worker")), (PEER, Some("elena"), None)]);
    let want = format!("{} {}", item8("you are `worker-1` in workspace `app`", "worker-1 (worker), elena"), worker_job("the member that gave you the work"));
    assert_eq!(mail::session_item(&ws, OWN), Some(want));
}

#[test] // R§9.3 "no job: no sentence", R§18 item 19
fn job_absent_member_is_listed_by_name_alone_and_gets_no_sentence() {
    assert_eq!(mail::session_item(&team(), LATE), Some(item8("you are `elena` in workspace `app`", JOB_LIST)));
    let ws = ws_jobs(&[(OWN, Some("elena"), None), (PEER, Some("kai"), None)]);
    assert_eq!(mail::session_item(&ws, OWN), Some(item8("you are `elena` in workspace `app`", "elena, kai")));
}

#[test] // Reading 4: the orchestrator's mail address is names::sender
fn job_orchestrator_with_no_name_is_named_by_its_pane_key() {
    let ws = ws_jobs(&[(OWN, None, Some("orchestrator")), (PEER, Some("worker-1"), Some("worker"))]);
    let want = format!("{} {}", item8("you are `worker-1` in workspace `app`", "worker-1 (worker)"), worker_job("kaaaaaaaa"));
    assert_eq!(mail::session_item(&ws, PEER), Some(want));
}

#[test] // Reading 3: R§9.3 defines two jobs; the text holds names and tool names only
fn job_value_outside_jobs_reads_as_no_job() {
    let ws = ws_jobs(&[(OWN, Some("elena"), Some("lead\n")), (PEER, Some("kai"), Some("worker"))]);
    assert_eq!(mail::session_item(&ws, OWN), Some(item8("you are `elena` in workspace `app`", "elena, kai (worker)")));
}

#[cfg(unix)]
#[test] // R§12 row member.launch.job: read by the session-start hook
fn job_sentence_at_session_start_from_the_member_record() {
    for agent in [Agent::ClaudeCode, Agent::OpenCode] {
        let x = mx(agent, true, true, None, &[(OWN, Some("worker-1")), (PEER, Some("orchestrator"))]);
        set_jobs(&x, &[(OWN, "worker"), (PEER, "orchestrator")]);
        let base = build(&SessionView::load(&x.dir).unwrap());
        let item = item8("you are `worker-1` in workspace `app`", "worker-1 (worker), orchestrator (orchestrator)");
        assert_eq!(member_text(&x, &start(agent)), format!("{base}\n\n{item} {}", worker_job("orchestrator")), "{agent:?}");
    }
}

#[cfg(unix)]
#[test] // R§9.3 "with mail off there is no job sentence"
fn job_sentence_absent_with_mail_off() {
    let x = mx(Agent::ClaudeCode, false, true, None, &[(OWN, Some("worker-1")), (PEER, Some("orchestrator"))]);
    set_jobs(&x, &[(OWN, "worker"), (PEER, "orchestrator")]);
    let got = member_text(&x, &start(Agent::ClaudeCode));
    assert_eq!(got, build(&SessionView::load(&x.dir).unwrap()));
    assert!(!got.contains("Your job:"), "{got}");
}

#[test] // M§8a item 1
fn mail_item_named_member_lists_every_member_name() {
    let ws = ws_of("app", &[(OWN, Some("elena")), (PEER, Some("kai")), (LATE, None)]);
    assert_eq!(mail::session_item(&ws, OWN), Some(item8("you are `elena` in workspace `app`", "elena, kai")));
}

#[test] // M§8a item 1, review round 2 F4: a name shared by two members is listed once, in key order of its first member
fn mail_item_lists_each_name_once() {
    let ws = ws_of("app", &[(OWN, Some("kai")), (PEER, Some("elena")), (LATE, Some("kai"))]);
    assert_eq!(mail::session_item(&ws, OWN), Some(item8("you are `kai` in workspace `app`", "kai, elena")));
}

#[test] // M§8a item 1, no-name clause
fn mail_item_member_with_no_name_gets_the_pane_clause() {
    let ws = ws_of("app", &[(OWN, None), (PEER, Some("kai"))]);
    assert_eq!(mail::session_item(&ws, OWN), Some(item8("you have no mail name; others reach you by pane kaaaaaaaa", "kai")));
}

#[test] // M§8a item 1, review round 2 F1: a pane with no member record is not a mail member (names::resolve)
fn mail_item_absent_for_a_pane_with_no_member_record() {
    let ws = ws_of("app", &[(OWN, None), (PEER, Some("kai"))]);
    assert_eq!(mail::session_item(&ws, LATE), None);
}

#[cfg(unix)]
#[test] // review round 2 F1, at session start
fn mail_item_absent_at_session_start_for_a_pane_with_no_member_record() {
    let x = mx(Agent::ClaudeCode, true, true, None, &[(PEER, Some("kai"))]);
    assert_eq!(member_text(&x, &start(Agent::ClaudeCode)), build(&SessionView::load(&x.dir).unwrap()));
}

#[test] // AU84, esc
fn mail_item_escapes_every_name_and_skips_invalid_names() {
    let ws = ws_of("a\u{1b}[2J\nb", &[(OWN, Some("elena")), (PEER, Some("Kai\u{202e}x"))]);
    assert_eq!(mail::session_item(&ws, OWN), Some(item8("you are `elena` in workspace `a\\u{1b}[2J\\nb`", "elena")));
    let ws = ws_of("app", &[(OWN, Some("bad\nname")), (PEER, Some("kai")), ("k\u{7}", None)]);
    assert_eq!(mail::session_item(&ws, OWN), Some(item8("you have no mail name; others reach you by pane kaaaaaaaa", "kai")));
    assert_eq!(mail::session_item(&ws, "k\u{7}"), Some(item8("you have no mail name; others reach you by pane k\\u{07}", "kai")));
}

#[cfg(unix)]
#[test] // M§8a item 1 order, §8.6 items 7 and 8; M§13 #12 second half
fn mail_item_at_session_start_after_mechanics_before_resume_note() {
    for agent in [Agent::ClaudeCode, Agent::OpenCode] {
        let note = ResumeNote { clean_stop: true, closed_units: vec!["u-1".into()], ..Default::default() };
        let x = mx(agent, true, true, Some(note), &[(OWN, Some("elena")), (PEER, Some("kai"))]);
        let base = build(&SessionView::load(&x.dir).unwrap());
        let first = "you are `elena` in workspace `app`";
        assert_eq!(member_text(&x, &start(agent)), format!("{base}\n\n{}\n\n{U1}", item8(first, "elena, kai")), "{agent:?}");
        set_members(&x, &[(OWN, Some("elena")), (PEER, Some("kai")), (LATE, Some("lena"))]);
        let tail = if agent == Agent::ClaudeCode { String::new() } else { format!("\n\n{U1}") };
        assert_eq!(member_text(&x, &start(agent)), format!("{base}\n\n{}{tail}", item8(first, "elena, kai, lena")), "{agent:?}");
    }
}

#[cfg(unix)]
#[test] // graph review focus 58: item 8 only with `features.mail` on, only in a workspace
fn mail_item_absent_when_mail_off_or_outside_a_workspace() {
    let x = mx(Agent::ClaudeCode, false, true, None, &[(OWN, Some("elena"))]);
    let got = member_text(&x, &start(Agent::ClaudeCode));
    assert_eq!(got, build(&SessionView::load(&x.dir).unwrap()));
    assert!(!got.contains("Workspace mail:"));
    let x = mx(Agent::ClaudeCode, true, false, None, &[(OWN, Some("elena"))]);
    assert_eq!(member_text(&x, &start(Agent::ClaudeCode)), build(&SessionView::load(&x.dir).unwrap()));
}

#[cfg(unix)]
#[test] // M§8a item 1: not appended to subagent prompts
fn mail_item_not_in_mechanics_or_subagent_text() {
    assert_eq!(mechanics(&SessionView { mail: true, ..view() }), mechanics(&view()));
    let x = mx(Agent::ClaudeCode, true, true, None, &[(OWN, Some("elena"))]);
    let m = mechanics(&SessionView::load(&x.dir).unwrap());
    assert_eq!(member_text(&x, &hook("SubagentStart-sub")), m);
    let sub = HookInput { cli_event: Some("subagent".into()), ..HookInput::parse(Agent::OpenCode, b"{}").unwrap() };
    assert_eq!(member_text(&x, &sub), m);
}

#[cfg(unix)]
#[test] // review round 2 F2: a `workspace.json` that does not read gives no item 8; the rest is as with `mail` off
fn mail_item_absent_when_workspace_state_is_unreadable() {
    let note = || Some(ResumeNote { clean_stop: true, closed_units: vec!["u-1".into()], ..Default::default() });
    for agent in [Agent::ClaudeCode, Agent::OpenCode] {
        let text = |mail: bool| {
            let x = mx(agent, mail, true, note(), &[(OWN, Some("elena"))]);
            let state = x.paths.workspaces_dir().join(WS_ID);
            std::fs::write(state.join("workspace.json"), b"{not json").unwrap();
            let got = member_text(&x, &start(agent));
            let errors = std::fs::read_to_string(state.join("errors.log")).unwrap_or_default();
            (got, errors.lines().count(), build(&SessionView::load(&x.dir).unwrap()))
        };
        let (on, on_lines, base) = text(true);
        let (off, off_lines, _) = text(false);
        assert!(!on.contains("Workspace mail:"), "{agent:?}: {on}");
        assert_eq!(on, off, "{agent:?}");
        assert_eq!(on, format!("{base}\n\n{U1}"), "{agent:?}");
        // Claude Code: record_session's read writes the one line (C6); OpenCode writes nothing (D10).
        let lines = if agent == Agent::ClaudeCode { 1 } else { 0 };
        assert_eq!((on_lines, off_lines), (lines, lines), "{agent:?}");
    }
}

#[cfg(unix)]
#[test] // M§8a "names and tool names only", M-C2
fn mail_item_holds_no_subject_or_body_text() {
    let x = mx(Agent::ClaudeCode, true, true, None, &[(OWN, Some("elena")), (PEER, Some("kai"))]);
    let ws = Store::open(&x.paths, WS_ID).read().unwrap().unwrap();
    let r = zygo_workspace::mail::send(&x.paths, &ws, Some(PEER), "elena", "subj-7f3a", "body-9c1d", &zygo_config::MailConfig::default(), SystemTime::now()).unwrap();
    assert_eq!(r.sent.len(), 1);
    let got = member_text(&x, &start(Agent::ClaudeCode));
    assert!(got.contains(&item8("you are `elena` in workspace `app`", "elena, kai")), "{got}");
    assert!(!got.contains("subj-7f3a") && !got.contains("body-9c1d"), "{got}");
}

#[test] // Produces: `#[serde(default)]`
fn mail_item_view_field_defaults_to_off() {
    let (_t, d) = sd();
    let mut json = serde_json::to_value(view()).unwrap();
    json.as_object_mut().unwrap().remove("mail");
    std::fs::write(d.view_json(), serde_json::to_vec(&json).unwrap()).unwrap();
    assert!(!SessionView::load(&d).unwrap().mail);
    assert!(!SessionView::default().mail);
    SessionView { mail: true, ..view() }.save(&d).unwrap();
    let raw: Value = serde_json::from_slice(&std::fs::read(d.view_json()).unwrap()).unwrap();
    assert_eq!(raw["mail"], true);
}

/// The OpenCode transform payload of a main-role request (`crates/zygo-launch/assets/opencode-plugin.js`, the
/// `experimental.chat.system.transform` handler): `{input, output}` with `input.sessionID`
/// (`fixtures/opencode/live/st-transform.jsonl`: keys `model,sessionID`).
#[cfg(unix)]
fn oc(session: &str) -> HookInput {
    let p = serde_json::json!({"input": {"sessionID": session, "model": {"providerID": "zygo", "id": "lead"}},
        "output": {"system": []}, "cwd": "/"});
    HookInput::parse(Agent::OpenCode, p.to_string().as_bytes()).unwrap()
}
#[cfg(unix)]
const ELENA: &str = "you are `elena` in workspace `app`";
#[cfg(unix)]
fn joined(x: &Mx) {
    set_members(x, &[(OWN, Some("elena")), (PEER, Some("kai")), (LATE, Some("lena"))]);
}

#[cfg(unix)]
#[test] // FX-9 Acceptance, M§8a item 1: item 8 is built at the main-loop SessionStart, not at each OpenCode request
fn opencode_item_8_is_fixed_for_the_session_after_a_member_joins() {
    let x = mx(Agent::OpenCode, true, true, None, &[(OWN, Some("elena")), (PEER, Some("kai"))]);
    let base = build(&SessionView::load(&x.dir).unwrap());
    let first = member_text(&x, &oc("ses_A"));
    assert_eq!(first, format!("{base}\n\n{}", item8(ELENA, "elena, kai")));
    joined(&x);
    assert_eq!(member_text(&x, &oc("ses_A")), first);
    assert_eq!(member_text(&x, &oc("ses_A")), first);
}

#[cfg(unix)]
#[test] // FX-9: a new OpenCode session id is a new main-loop SessionStart and gets the current names
fn opencode_new_session_after_a_join_gets_the_new_member() {
    let x = mx(Agent::OpenCode, true, true, None, &[(OWN, Some("elena")), (PEER, Some("kai"))]);
    let base = build(&SessionView::load(&x.dir).unwrap());
    let first = member_text(&x, &oc("ses_A"));
    joined(&x);
    assert_eq!(member_text(&x, &oc("ses_B")), format!("{base}\n\n{}", item8(ELENA, "elena, kai, lena")));
    assert_eq!(member_text(&x, &oc("ses_A")), first);
}

#[cfg(unix)]
#[test] // FX-9, S§10.4: the fixed item 8 holds the "Other machines" clause of its SessionStart
fn opencode_fixed_item_8_holds_the_other_machines_clause() {
    use zygo_mail_sync::registry::{write_cache, MemberCache};
    use zygo_mail_sync::state::SyncPaths;
    let x = mx(Agent::OpenCode, true, true, None, &[(OWN, Some("elena")), (PEER, Some("kai"))]);
    std::fs::write(&x.paths.config_file, "[mail]\nhost = \"studio\"\n\n[mail.workspace.app]\nsync = \"zygo/mail\"\n").unwrap();
    let cache = |names: &[&str]| {
        let c = MemberCache { hosts: [("b".to_string(), names.iter().map(|n| n.to_string()).collect())].into_iter().collect() };
        write_cache(&SyncPaths::new(&x.paths, WS_ID).unwrap(), &c).unwrap();
    };
    cache(&["bob"]);
    let base = build(&SessionView::load(&x.dir).unwrap());
    let item = format!("Workspace mail: {ELENA}. Members: `elena, kai`. Other machines: bob@b; mail them by the full address. {MAIL_TAIL}");
    assert_eq!(member_text(&x, &oc("ses_A")), format!("{base}\n\n{item}"));
    cache(&["bob", "eve"]);
    joined(&x);
    assert_eq!(member_text(&x, &oc("ses_A")), format!("{base}\n\n{item}"));
    assert_eq!(std::fs::read_to_string(session_start::item_path(&x.dir, "ses_A").unwrap()).unwrap(), item);
}

#[cfg(unix)]
#[test] // FX-9 Decision 4: a text stored for the id (another request stored it first) is the item 8 of the output
fn opencode_stored_item_8_is_used_as_it_is() {
    let x = mx(Agent::OpenCode, true, true, None, &[(OWN, Some("elena"))]);
    let base = build(&SessionView::load(&x.dir).unwrap());
    std::fs::create_dir(x.dir.root.join("session-item")).unwrap();
    std::fs::write(session_start::item_path(&x.dir, "ses_A").unwrap(), "STORED ITEM").unwrap();
    assert_eq!(member_text(&x, &oc("ses_A")), format!("{base}\n\nSTORED ITEM"));
}

#[cfg(unix)]
#[test] // FX-9: Claude Code is not changed; its item 8 is built at each SessionStart and nothing is stored
fn claude_code_item_8_is_built_at_each_session_start() {
    let x = mx(Agent::ClaudeCode, true, true, None, &[(OWN, Some("elena")), (PEER, Some("kai"))]);
    let base = build(&SessionView::load(&x.dir).unwrap());
    assert_eq!(member_text(&x, &start(Agent::ClaudeCode)), format!("{base}\n\n{}", item8(ELENA, "elena, kai")));
    joined(&x);
    assert_eq!(member_text(&x, &start(Agent::ClaudeCode)), format!("{base}\n\n{}", item8(ELENA, "elena, kai, lena")));
    assert!(!x.dir.root.join("session-item").exists());
}

#[cfg(unix)]
#[test] // FX-9 Decision 3: an id that fails the `flag_path` rule, or no id, keeps the per-request build; nothing stored
fn opencode_session_id_that_fails_the_rule_is_not_stored() {
    let no_id = || {
        HookInput::parse(Agent::OpenCode, br#"{"input": {"model": {"providerID": "zygo", "id": "lead"}}, "output": {"system": []}}"#)
            .unwrap()
    };
    let long = "a".repeat(65);
    for input in [oc("../x"), oc(""), oc(&long), no_id()] {
        let x = mx(Agent::OpenCode, true, true, None, &[(OWN, Some("elena")), (PEER, Some("kai"))]);
        let base = build(&SessionView::load(&x.dir).unwrap());
        assert_eq!(member_text(&x, &input), format!("{base}\n\n{}", item8(ELENA, "elena, kai")));
        joined(&x);
        assert_eq!(member_text(&x, &input), format!("{base}\n\n{}", item8(ELENA, "elena, kai, lena")));
        assert!(!x.dir.root.join("session-item").exists());
        assert!(!x.dir.root.join("x").exists() && !x.t.path().join("cache/sessions/x").exists());
    }
}

#[cfg(unix)]
#[test] // FX-9 Decision 4: the request that loses the race (the name is taken after its read) returns the winner's text
fn fixed_item_loser_of_a_race_returns_the_winner() {
    use std::os::unix::fs::PermissionsExt;
    let (_t, d) = sd();
    let no_log = |l: &str| panic!("no log line expected: {l}");
    let mut loser = || {
        // Another request stores its text while this one builds.
        let mut winner = || Some("WINNER".to_string());
        assert_eq!(session_start::fixed_item(&d, "ses_A", &mut winner, &no_log).as_deref(), Some("WINNER"));
        Some("LOSER".to_string())
    };
    assert_eq!(session_start::fixed_item(&d, "ses_A", &mut loser, &no_log).as_deref(), Some("WINNER"));
    let mut later = || panic!("a stored item is not built again");
    assert_eq!(session_start::fixed_item(&d, "ses_A", &mut later, &no_log).as_deref(), Some("WINNER"));
    let folder = d.root.join("session-item");
    let file = session_start::item_path(&d, "ses_A").unwrap();
    assert_eq!(file.parent(), Some(folder.as_path()));
    assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 1, "the temp files are removed");
    assert_eq!(std::fs::metadata(&folder).unwrap().permissions().mode() & 0o777, 0o700);
    assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
    assert_eq!(session_start::item_path(&d, "../x"), None);
}

#[cfg(unix)]
#[test] // FX-9 Decision 4: concurrent first requests of one session give one text
fn fixed_item_from_threads_gives_one_text() {
    let (_t, d) = sd();
    let barrier = std::sync::Barrier::new(8);
    let got: Vec<Option<String>> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..8)
            .map(|i| {
                let (d, barrier) = (&d, &barrier);
                s.spawn(move || {
                    barrier.wait();
                    let mut build = || Some(format!("TEXT-{i}"));
                    session_start::fixed_item(d, "ses_A", &mut build, &|l| panic!("no log line expected: {l}"))
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let stored = std::fs::read_to_string(session_start::item_path(&d, "ses_A").unwrap()).unwrap();
    assert!(got.iter().all(|g| g.as_deref() == Some(stored.as_str())), "{got:?} vs {stored}");
    assert_eq!(std::fs::read_dir(d.root.join("session-item")).unwrap().count(), 1);
}

#[cfg(unix)]
#[test] // FX-9 Decision 5, review F1 (D6): `None` stores nothing; a stored entry that is a link is not read, no line
fn fixed_item_stores_no_none_and_reads_no_link() {
    let (t, d) = sd();
    let lines = std::cell::RefCell::new(Vec::new());
    let log = |l: &str| lines.borrow_mut().push(l.to_string());
    assert_eq!(session_start::fixed_item(&d, "ses_A", &mut || None, &log), None);
    assert_eq!(session_start::fixed_item(&d, "ses_A", &mut || Some("NOW".into()), &log).as_deref(), Some("NOW"));
    std::fs::write(t.path().join("outside"), "OUTSIDE").unwrap();
    std::os::unix::fs::symlink(t.path().join("outside"), session_start::item_path(&d, "ses_B").unwrap()).unwrap();
    assert_eq!(session_start::fixed_item(&d, "ses_B", &mut || Some("BUILT".into()), &log).as_deref(), Some("BUILT"));
    assert!(lines.borrow().is_empty(), "{:?}", lines.borrow());
    assert_eq!(std::fs::read_to_string(t.path().join("outside")).unwrap(), "OUTSIDE");
}

/// The number of lines in the session log of `x`.
#[cfg(unix)]
fn log_lines(x: &Mx) -> usize {
    std::fs::read_to_string(x.dir.log()).unwrap_or_default().lines().count()
}
/// The OpenCode member of `mx` with `elena` and `kai`, its session text and its item 8 before and after `lena` joins.
#[cfg(unix)]
fn two() -> (Mx, String, String, String) {
    let x = mx(Agent::OpenCode, true, true, None, &[(OWN, Some("elena")), (PEER, Some("kai"))]);
    let base = build(&SessionView::load(&x.dir).unwrap());
    (x, base, item8(ELENA, "elena, kai"), item8(ELENA, "elena, kai, lena"))
}

#[cfg(unix)]
#[test] // review F1 (D6: the OpenCode call runs per model request): a bad stored entry writes no line and builds item 8
fn opencode_bad_stored_entry_writes_no_line() {
    let (x, base, before, after) = two();
    std::fs::create_dir_all(session_start::item_path(&x.dir, "ses_A").unwrap()).unwrap();
    let lines = log_lines(&x);
    assert_eq!(member_text(&x, &oc("ses_A")), format!("{base}\n\n{before}"));
    joined(&x);
    for _ in 0..4 {
        assert_eq!(member_text(&x, &oc("ses_A")), format!("{base}\n\n{after}"));
    }
    assert_eq!(log_lines(&x), lines, "{}", std::fs::read_to_string(x.dir.log()).unwrap_or_default());
}

#[cfg(unix)]
#[test] // review F2: a FIFO at the stored name does not make the request wait (O_NONBLOCK); 60 s fail-safe bound
fn opencode_fifo_at_the_stored_name_does_not_block() {
    let (x, base, before, _) = two();
    let path = session_start::item_path(&x.dir, "ses_A").unwrap();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let c = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
    // SAFETY: `c` is a NUL-terminated path that lives for the call.
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
    let (tx, rx) = std::sync::mpsc::channel();
    let x = std::sync::Arc::new(x);
    let xc = x.clone();
    std::thread::spawn(move || {
        let _ = tx.send(member_text(&xc, &oc("ses_A")));
    });
    let got = rx.recv_timeout(Duration::from_secs(60)).expect("the request waited on a FIFO");
    assert_eq!(got, format!("{base}\n\n{before}"));
}

#[cfg(unix)]
#[test] // review F2: a stored file over MAX_ITEM (1 MiB) is not used
fn opencode_stored_file_over_the_limit_is_not_used() {
    let (x, base, before, _) = two();
    let path = session_start::item_path(&x.dir, "ses_A").unwrap();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "x".repeat((1 << 20) + 1)).unwrap();
    assert_eq!(member_text(&x, &oc("ses_A")), format!("{base}\n\n{before}"));
}

#[cfg(unix)]
#[test] // review F2: a `session-item/` made before at 0755 ends at 0700
fn opencode_store_folder_ends_at_0700() {
    use std::os::unix::fs::PermissionsExt;
    let (x, ..) = two();
    let folder = x.dir.root.join("session-item");
    std::fs::create_dir(&folder).unwrap();
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o755)).unwrap();
    member_text(&x, &oc("ses_A"));
    assert_eq!(std::fs::metadata(&folder).unwrap().permissions().mode() & 0o777, 0o700);
    assert!(session_start::item_path(&x.dir, "ses_A").unwrap().is_file());
}

#[cfg(unix)]
#[test] // review F2: mail off reads and stores nothing, also with a session id
fn opencode_mail_off_makes_no_store() {
    let x = mx(Agent::OpenCode, false, true, None, &[(OWN, Some("elena"))]);
    assert_eq!(member_text(&x, &oc("ses_A")), build(&SessionView::load(&x.dir).unwrap()));
    assert!(!x.dir.root.join("session-item").exists());
}

#[cfg(unix)]
#[test] // review F2: the `Agent::OpenCode` guard: a Claude Code payload that holds `input.sessionID` stores nothing
fn claude_code_payload_with_a_session_id_makes_no_store() {
    let x = mx(Agent::ClaudeCode, true, true, None, &[(OWN, Some("elena")), (PEER, Some("kai"))]);
    let base = build(&SessionView::load(&x.dir).unwrap());
    let mut input = start(Agent::ClaudeCode);
    input.raw["input"] = serde_json::json!({"sessionID": "ses_A"});
    assert_eq!(member_text(&x, &input), format!("{base}\n\n{}", item8(ELENA, "elena, kai")));
    joined(&x);
    assert_eq!(member_text(&x, &input), format!("{base}\n\n{}", item8(ELENA, "elena, kai, lena")));
    assert!(!x.dir.root.join("session-item").exists());
}

#[cfg(unix)]
#[test] // review F5: an empty stored file is not usable: item 8 is built, with no line
fn opencode_empty_stored_file_is_not_used() {
    let (x, base, before, after) = two();
    let path = session_start::item_path(&x.dir, "ses_A").unwrap();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "").unwrap();
    let lines = log_lines(&x);
    assert_eq!(member_text(&x, &oc("ses_A")), format!("{base}\n\n{before}"));
    joined(&x);
    assert_eq!(member_text(&x, &oc("ses_A")), format!("{base}\n\n{after}"));
    assert_eq!(log_lines(&x), lines);
}

#[cfg(unix)]
#[test] // review F6: a `session-item` that is a link to a folder is not used: built, no line, nothing written outside
fn opencode_store_folder_that_is_a_link_is_not_used() {
    use std::os::unix::fs::PermissionsExt;
    let (x, base, before, after) = two();
    let outside = x.t.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(outside.join("keep"), "KEEP").unwrap();
    std::fs::set_permissions(&outside, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::os::unix::fs::symlink(&outside, x.dir.root.join("session-item")).unwrap();
    let lines = log_lines(&x);
    assert_eq!(member_text(&x, &oc("ses_A")), format!("{base}\n\n{before}"));
    joined(&x);
    assert_eq!(member_text(&x, &oc("ses_A")), format!("{base}\n\n{after}"));
    assert_eq!(log_lines(&x), lines);
    assert_eq!(std::fs::metadata(&outside).unwrap().permissions().mode() & 0o777, 0o755);
    let names: Vec<_> = std::fs::read_dir(&outside).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(names, [std::ffi::OsString::from("keep")]);
}

#[cfg(unix)]
#[test] // review F7: leftover temp files with the old names (pid and counter only) do not stop the store
fn opencode_leftover_temp_files_do_not_stop_the_store() {
    let (x, base, before, _) = two();
    let path = session_start::item_path(&x.dir, "ses_A").unwrap();
    let folder = path.parent().unwrap();
    std::fs::create_dir_all(folder).unwrap();
    let encoded = path.file_name().unwrap().to_str().unwrap().to_string();
    for name in ["ses_A".to_string(), encoded] {
        for k in 0..256 {
            std::fs::write(folder.join(format!(".{name}.{}.{k}.tmp", std::process::id())), "").unwrap();
        }
    }
    let first = member_text(&x, &oc("ses_A"));
    assert_eq!(first, format!("{base}\n\n{before}"));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
}

#[cfg(unix)]
#[test] // review F8: ids that differ only in case get names that differ on a case-insensitive file system
fn ids_that_differ_in_case_get_different_names() {
    let (_t, d) = sd();
    let a = session_start::item_path(&d, "ses_a").unwrap();
    let b = session_start::item_path(&d, "ses_A").unwrap();
    let low = |p: &std::path::Path| p.file_name().unwrap().to_str().unwrap().to_lowercase();
    assert_ne!(low(&a), low(&b));
    let long = session_start::item_path(&d, &"Z".repeat(64)).unwrap();
    assert!(long.file_name().unwrap().len() <= 255);
    let no_log = |l: &str| panic!("no log line expected: {l}");
    assert_eq!(session_start::fixed_item(&d, "ses_a", &mut || Some("LOWER".into()), &no_log).as_deref(), Some("LOWER"));
    assert_eq!(session_start::fixed_item(&d, "ses_A", &mut || Some("UPPER".into()), &no_log).as_deref(), Some("UPPER"));
    assert_eq!(std::fs::read_to_string(a).unwrap(), "LOWER");
    assert_eq!(std::fs::read_to_string(b).unwrap(), "UPPER");
}
