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

#[test] // M§8a item 1
fn mail_item_named_member_lists_every_member_name() {
    let ws = ws_of("app", &[(OWN, Some("elena")), (PEER, Some("kai")), (LATE, None)]);
    assert_eq!(mail::session_item(&ws, OWN), item8("you are `elena` in workspace `app`", "elena, kai"));
}

#[test] // M§8a item 1, no-name clause
fn mail_item_member_with_no_name_gets_the_pane_clause() {
    let ws = ws_of("app", &[(OWN, None), (PEER, Some("kai"))]);
    assert_eq!(mail::session_item(&ws, OWN), item8("you have no mail name; others reach you by pane kaaaaaaaa", "kai"));
    assert_eq!(mail::session_item(&ws, LATE), item8("you have no mail name; others reach you by pane kcccccccc", "kai"));
}

#[test] // AU84, esc
fn mail_item_escapes_every_name_and_skips_invalid_names() {
    let ws = ws_of("a\u{1b}[2J\nb", &[(OWN, Some("elena")), (PEER, Some("Kai\u{202e}x"))]);
    assert_eq!(mail::session_item(&ws, OWN), item8("you are `elena` in workspace `a\\u{1b}[2J\\nb`", "elena"));
    let ws = ws_of("app", &[(OWN, Some("bad\nname")), (PEER, Some("kai"))]);
    assert_eq!(mail::session_item(&ws, OWN), item8("you have no mail name; others reach you by pane kaaaaaaaa", "kai"));
    assert_eq!(mail::session_item(&ws, "k\u{7}"), item8("you have no mail name; others reach you by pane k\\u{07}", "kai"));
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
