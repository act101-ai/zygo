//! The member wrapper of `zygo ws run` (W§8.2 steps 4, 5 and 7): the member record at the start and at the exit of
//! the agent, the pre-assigned session id, the exit classification and the exit lines.

use std::path::{Path, PathBuf};
use std::time::Duration;

use zygo_core::Agent;
use zygo_launch::env::Env;
use zygo_launch::launcher::LaunchOutcome;
use zygo_workspace::args;
use zygo_workspace::state::{MemberRecord, MemberState, Pending};

/// A resume that fails with a non-zero code in less than this time from the agent's spawn failed fast (W§8.2 step 7).
const FAST_FAIL: Duration = Duration::from_secs(10);

/// The 16 bytes `b` as a UUID version 4 (RFC 9562): the version 4 in byte 6, the variant `0b10` in byte 8; lowercase
/// hex in groups 8-4-4-4-12.
pub(crate) fn uuid_v4_from(mut b: [u8; 16]) -> String {
    b[6] = b[6] & 0x0f | 0x40;
    b[8] = b[8] & 0x3f | 0x80;
    let h = hex::encode(b);
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

/// A fresh random session id for the pre-assign of a Claude Code member (W§8.3).
pub(crate) fn new_session_uuid() -> String {
    uuid_v4_from(rand::random())
}

/// How a member launch ended (AU17, AU60, AU73, W§8.2 step 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Exit {
    /// A signal ended the agent or reached the launcher, or a resume failed fast.
    Resumable { failed_fast: bool },
    Ended,
    /// The agent was not spawned.
    NotStarted,
}

/// AU17, AU73, W§8.2 step 7: no spawn is `NotStarted`; a signal received by the launcher or one that ended the agent
/// is `Resumable`; a resume that exits non-zero within [`FAST_FAIL`] of its spawn failed fast; else `Ended`.
pub(crate) fn classify(o: &LaunchOutcome, resumed: bool) -> Exit {
    if !o.spawned {
        return Exit::NotStarted;
    }
    if o.received || o.signal.is_some() {
        return Exit::Resumable { failed_fast: false };
    }
    if resumed && o.code != 0 && o.ran.is_some_and(|r| r < FAST_FAIL) {
        return Exit::Resumable { failed_fast: true };
    }
    Exit::Ended
}

/// What the session callback records for one start (AU92).
pub(crate) struct Start {
    pub zygo_session: String,
    pub agent_session: Option<String>,
    pub started_at: u64,
    pub config_dir: Option<PathBuf>,
    pub program: PathBuf,
}

/// The start write (AU92): the AU30-filtered `launch`, `dropped_flags` (D4: the stored list stays when the filtered
/// launch equals the stored one, else the filter's list replaces it), `pending`, `config_dir` and `program`. Nothing
/// else changes.
pub(crate) fn record_start(rec: &mut MemberRecord, launch: &zygo_workspace::state::Launch, s: &Start) {
    let f = args::filter(launch.kind, &launch.args);
    let mut kept = launch.clone();
    kept.args = f.kept;
    if kept != rec.launch {
        rec.dropped_flags = f.dropped_flags;
    }
    rec.launch = kept;
    rec.pending = Some(Pending {
        zygo_session: s.zygo_session.clone(),
        agent_session: s.agent_session.clone(),
        started_at: s.started_at,
        ..Default::default()
    });
    rec.config_dir = s.config_dir.clone();
    rec.program = Some(s.program.clone());
}

/// The exit write (D5, AU60, AU92, H1 seam b-d). It changes a record only when its `pending` or its `zygo_session`
/// names `zygo_session`; it gives `false` with the record unchanged otherwise. `NotStarted` removes that `pending`.
/// Other exits commit that `pending` when no hook did (its `agent_session` only when set, never a stored id replaced
/// with none) and set the state; `closed_units`, `interrupted`, `clean_stop`, `saved_for` and `note_pending` stay.
pub(crate) fn record_exit(rec: &mut MemberRecord, zygo_session: &str, exit: Exit) -> bool {
    let names = |r: &MemberRecord| r.pending.as_ref().is_some_and(|p| p.zygo_session == zygo_session);
    if exit == Exit::NotStarted {
        if names(rec) {
            rec.pending = None;
            return true;
        }
        return false;
    }
    if names(rec)
        && let Some(p) = rec.pending.take()
    {
        rec.zygo_session = Some(p.zygo_session);
        if p.agent_session.is_some() {
            rec.agent_session = p.agent_session;
        }
    }
    if rec.zygo_session.as_deref() != Some(zygo_session) {
        return false;
    }
    rec.state = match exit {
        Exit::Resumable { .. } => MemberState::Resumable,
        _ => MemberState::Ended,
    };
    true
}

/// The effective `CLAUDE_CONFIG_DIR` of a launch environment (AU61): the non-empty value, joined to `cwd` when it is
/// relative; else none.
pub(crate) fn effective_config_dir(env: &Env, cwd: &Path) -> Option<PathBuf> {
    let v = env.get("CLAUDE_CONFIG_DIR").filter(|v| !v.is_empty())?;
    let p = PathBuf::from(v);
    Some(if p.is_absolute() { p } else { cwd.join(p) })
}

/// The `zygo` subcommand of an agent: `claude` or `opencode` (D7).
pub(crate) fn verb(kind: Agent) -> &'static str {
    match kind {
        Agent::ClaudeCode => "claude",
        Agent::OpenCode => "opencode",
    }
}

/// The command that resumes the agent session `id` (D7, AU3): `zygo claude -- --resume <id>` or
/// `zygo opencode -- --session=<id>`. `id` is printed as given; a caller escapes it.
pub(crate) fn resume_command(kind: Agent, id: &str) -> String {
    match kind {
        Agent::ClaudeCode => format!("zygo claude -- --resume {id}"),
        Agent::OpenCode => format!("zygo opencode -- --session={id}"),
    }
}

/// The lines `ws run` prints after the agent (D7). `resumed` is the id that this launch resumed; `kept` is the
/// `agent_session` of the record that the exit write gave back (none when it gave none or an error).
pub(crate) fn exit_lines(kind: Agent, o: &LaunchOutcome, exit: Exit, resumed: Option<&str>, kept: Option<&str>) -> Vec<String> {
    let k = verb(kind);
    let n = o.code;
    match exit {
        Exit::NotStarted => vec![format!("zygo: the {k} launch failed before the agent started (exit {n})")],
        Exit::Resumable { failed_fast: true } => match resumed {
            Some(id) => {
                let id = zygo_core::esc(id);
                vec![format!("zygo: resume of {k} session {id} failed (exit {n}); try: {}", resume_command(kind, &id))]
            }
            None => vec![],
        },
        Exit::Ended if n != 0 => match kept {
            Some(id) => {
                let id = zygo_core::esc(id);
                vec![format!(
                    "zygo: {k} exited {n}; agent session {id} is kept by the agent; to resume it: {}",
                    resume_command(kind, &id)
                )]
            }
            None => vec![],
        },
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::time::Duration;
    use zygo_core::Agent;
    use zygo_launch::env::Env;
    use zygo_launch::launcher::LaunchOutcome;
    use zygo_workspace::state::{Launch, MemberRecord, MemberState, Pending, Subagent};

    fn out(code: i32, signal: Option<i32>, received: bool, ran: u64) -> LaunchOutcome {
        LaunchOutcome { code, signal, spawned: true, received, ran: Some(Duration::from_secs(ran)) }
    }
    fn not_spawned(code: i32) -> LaunchOutcome {
        LaunchOutcome { code, signal: None, spawned: false, received: false, ran: None }
    }

    #[test]
    fn uuid_v4_from_sets_only_the_version_and_variant_bits() { // RFC 9562: version 4 in byte 6, variant 0b10 in byte 8
        let b: [u8; 16] = std::array::from_fn(|i| i as u8); // 0x00..=0x0f
        assert_eq!(uuid_v4_from(b), "00010203-0405-4607-8809-0a0b0c0d0e0f");
        assert_eq!(uuid_v4_from([0x00; 16]), "00000000-0000-4000-8000-000000000000");
        assert_eq!(uuid_v4_from([0xff; 16]), "ffffffff-ffff-4fff-bfff-ffffffffffff");
    }

    #[test]
    fn new_session_uuid_is_a_fresh_v4() {
        let re = regex::Regex::new("^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$").unwrap();
        let (a, b) = (new_session_uuid(), new_session_uuid());
        assert!(re.is_match(&a) && re.is_match(&b), "{a} {b}");
        assert_ne!(a, b);
    }

    #[test]
    fn classify_follows_au17_and_the_fast_fail_rule() {
        assert_eq!(classify(&out(143, Some(15), true, 60), false), Exit::Resumable { failed_fast: false });
        assert_eq!(classify(&out(143, None, true, 60), false), Exit::Resumable { failed_fast: false }); // controller M3
        assert_eq!(classify(&out(0, None, true, 60), false), Exit::Resumable { failed_fast: false });
        assert_eq!(classify(&out(129, Some(1), false, 60), false), Exit::Resumable { failed_fast: false });
        assert_eq!(classify(&out(1, None, false, 9), true), Exit::Resumable { failed_fast: true });
        assert_eq!(classify(&out(1, None, false, 10), true), Exit::Ended);
        assert_eq!(classify(&out(0, None, false, 1), true), Exit::Ended);
        assert_eq!(classify(&out(1, None, false, 1), false), Exit::Ended);
        assert_eq!(classify(&out(130, None, false, 1), false), Exit::Ended); // AU73: a real 130 is not a signal
    }

    #[test]
    fn classify_without_a_spawn_is_not_started() { // controller M1
        assert_eq!(classify(&not_spawned(12), true), Exit::NotStarted);
        assert_eq!(classify(&not_spawned(13), true), Exit::NotStarted);
        assert_eq!(classify(&not_spawned(13), false), Exit::NotStarted);
    }

    #[test]
    fn record_start_writes_pending_with_the_filtered_launch() {
        let mut r = MemberRecord::new(Agent::ClaudeCode);
        r.dropped_flags = vec!["--old".into()];
        let mut l = Launch::new(Agent::ClaudeCode);
        l.args = ["--model", "lead", "--resume", "x"].map(String::from).to_vec();
        l.compact = Some("600000".into());
        let start = |zs: &str| Start {
            zygo_session: zs.into(),
            agent_session: Some("U1".into()),
            started_at: 7,
            config_dir: Some("/c".into()),
            program: "/b/claude".into(),
        };
        record_start(&mut r, &l, &start("S1"));
        assert_eq!(
            r.pending,
            Some(Pending { zygo_session: "S1".into(), agent_session: Some("U1".into()), started_at: 7, ..Default::default() })
        );
        assert_eq!(r.launch.args, ["--model", "lead"]);
        assert_eq!(r.launch.compact.as_deref(), Some("600000"));
        assert!(r.dropped_flags.is_empty()); // D4
        assert_eq!((r.config_dir.clone(), r.program.clone()), (Some("/c".into()), Some("/b/claude".into())));
        assert_eq!((r.zygo_session.clone(), r.state), (None, MemberState::Ended));
        r.dropped_flags = vec!["--agents".into()];
        let saved = r.launch.clone();
        record_start(&mut r, &saved, &start("S2"));
        assert_eq!(r.dropped_flags, ["--agents"]); // D4
        assert_eq!(r.pending.as_ref().unwrap().zygo_session, "S2");
    }

    fn base() -> MemberRecord {
        let mut r = MemberRecord::new(Agent::OpenCode);
        r.zygo_session = Some("Z0".into());
        r.agent_session = Some("OLD".into());
        r.state = MemberState::Resumable;
        r.closed_units = vec!["u1".into()];
        r.interrupted = vec![Subagent { agent_id: "a1".into(), agent_type: "t1".into(), ..Default::default() }];
        r.clean_stop = true;
        r.saved_for = Some("Z0".into());
        r.note_pending = true;
        r.pending = Some(Pending { zygo_session: "S1".into(), agent_session: None, started_at: 1, ..Default::default() });
        r
    }

    #[test]
    fn record_exit_commits_without_clears_and_keeps_a_hook_id() { // H1 seam b, c, d
        let mut r = base();
        assert!(record_exit(&mut r, "S1", Exit::Ended));
        assert_eq!(
            (r.zygo_session.as_deref(), r.agent_session.as_deref(), r.pending.is_none(), r.state),
            (Some("S1"), Some("OLD"), true, MemberState::Ended)
        );
        assert_eq!(
            (r.closed_units.clone(), r.interrupted.len(), r.clean_stop, r.saved_for.as_deref(), r.note_pending),
            (vec!["u1".to_string()], 1, true, Some("Z0"), true)
        );
        let mut r = base();
        r.pending.as_mut().unwrap().agent_session = Some("NEW".into());
        assert!(record_exit(&mut r, "S1", Exit::Resumable { failed_fast: false }));
        assert_eq!((r.agent_session.as_deref(), r.state), (Some("NEW"), MemberState::Resumable));
        let mut r = base();
        r.pending = None;
        r.zygo_session = Some("S1".into());
        r.agent_session = Some("ses_A".into());
        assert!(record_exit(&mut r, "S1", Exit::Ended));
        assert_eq!((r.agent_session.as_deref(), r.state), (Some("ses_A"), MemberState::Ended));
        let mut r = base();
        let before = r.clone();
        assert!(record_exit(&mut r, "S1", Exit::NotStarted));
        assert_eq!(r, MemberRecord { pending: None, ..before }); // AU60, AU92
    }

    #[test]
    fn record_exit_changes_nothing_for_another_session() { // minor 3
        for exit in [Exit::Ended, Exit::Resumable { failed_fast: false }, Exit::NotStarted] {
            let mut r = base(); // pending names S1, zygo_session is Z0
            let before = r.clone();
            assert!(!record_exit(&mut r, "S9", exit));
            assert_eq!(r, before, "{exit:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn effective_config_dir_is_absolute_or_none() {
        let e = |v: &str| Env(vec![("CLAUDE_CONFIG_DIR".into(), v.into())]);
        let w = Path::new("/w");
        assert_eq!(effective_config_dir(&e("/a/b"), w), Some(PathBuf::from("/a/b")));
        assert_eq!(effective_config_dir(&e("rel"), w), Some(PathBuf::from("/w/rel")));
        assert_eq!(effective_config_dir(&e(""), w), None);
        assert_eq!(effective_config_dir(&Env::default(), w), None);
    }

    #[test]
    fn exit_lines_are_the_spec_texts() { // D7
        assert_eq!(
            exit_lines(Agent::ClaudeCode, &out(1, None, false, 1), Exit::Resumable { failed_fast: true }, Some("abc"), None),
            ["zygo: resume of claude session abc failed (exit 1); try: zygo claude -- --resume abc"]
        );
        assert_eq!(
            exit_lines(Agent::OpenCode, &out(1, None, false, 1), Exit::Resumable { failed_fast: true }, Some("ses_1"), Some("ses_1")),
            ["zygo: resume of opencode session ses_1 failed (exit 1); try: zygo opencode -- --session=ses_1"]
        );
        assert_eq!(
            exit_lines(Agent::OpenCode, &out(3, None, false, 60), Exit::Ended, None, Some("ses_1")),
            ["zygo: opencode exited 3; agent session ses_1 is kept by the agent; to resume it: zygo opencode -- --session=ses_1"]
        );
        assert_eq!(
            exit_lines(Agent::ClaudeCode, &out(3, None, false, 60), Exit::Ended, None, Some("abc")),
            ["zygo: claude exited 3; agent session abc is kept by the agent; to resume it: zygo claude -- --resume abc"]
        );
        for code in [12, 13] {
            assert_eq!(
                exit_lines(Agent::ClaudeCode, &not_spawned(code), Exit::NotStarted, Some("abc"), None),
                [format!("zygo: the claude launch failed before the agent started (exit {code})")]
            );
        }
        assert!(exit_lines(Agent::ClaudeCode, &out(0, None, false, 60), Exit::Ended, None, Some("abc")).is_empty());
        assert!(
            exit_lines(Agent::ClaudeCode, &out(143, None, true, 60), Exit::Resumable { failed_fast: false }, None, Some("abc"))
                .is_empty()
        );
        assert!(exit_lines(Agent::ClaudeCode, &out(3, None, false, 60), Exit::Ended, None, None).is_empty()); // no saved record (D5)
    }
}
