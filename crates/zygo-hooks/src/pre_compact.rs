//! §10.6 compaction instructions: what to keep and what to drop.

use std::path::{Path, PathBuf};

use serde_json::Value;
use zygo_core::hook::{error_line, HookInput, HookOutput};
use zygo_core::session_dir::SessionDir;
use zygo_work::log::{open_unit, Begin};

use crate::session_start::{folder_state, Folder};

pub fn run(input: &HookInput) -> anyhow::Result<HookOutput> {
    run_with(input, SessionDir::from_env().as_ref())
}

pub fn run_in(dir: Option<&SessionDir>) -> anyhow::Result<HookOutput> {
    let units = match dir {
        Some(d) => open_units(&d.work())?,
        None => Vec::new(),
    };
    Ok(HookOutput::Text(text(&units)))
}

/// `reinject/<agent_id>` when `agent_id` passes the work-log agent-name rule (1-64 of `[A-Za-z0-9_-]`, the rule of
/// `zygo_work::log::check_agent`); else `None`.
pub fn flag_path(dir: &SessionDir, agent_id: &str) -> Option<PathBuf> {
    let ok = (1..=64).contains(&agent_id.len())
        && agent_id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    ok.then(|| dir.reinject().join(agent_id))
}

/// T§9.2 item 1: the output of `run_in(dir)`. For an input whose `agent_id` is a string and whose `agent_type` does
/// not start with `zygo-`, it also creates the empty flag `flag_path(dir, agent_id)`, whatever `run_in` gives (FX-37).
/// If the flag cannot be created, it writes one stderr line and still returns the text (T§14 item 2). If `run_in`
/// fails, its error is the result (no text); when the flag also failed, the two are one error, so the C6 wrapper
/// writes one line: `<run_in error>; pre-compact: cannot create <flag>: <reason>`.
pub fn run_with(input: &HookInput, dir: Option<&SessionDir>) -> anyhow::Result<HookOutput> {
    let out = run_in(dir);
    let zygo = input.raw.get("agent_type").and_then(Value::as_str).is_some_and(|t| t.starts_with("zygo-"));
    let mut flag_error = None;
    if !zygo
        && let (Some(d), Some(id)) = (dir, input.agent_id.as_deref())
        && let Some(p) = flag_path(d, id)
        && let Err(e) = create_flag(d, &p)
    {
        flag_error = Some(format!("pre-compact: cannot create {}: {e:#}", p.display()));
    }
    match (out, flag_error) {
        (Ok(out), Some(line)) => {
            error_line(&line);
            Ok(out)
        }
        (Ok(out), None) => Ok(out),
        (Err(e), Some(line)) => Err(anyhow::anyhow!("{e:#}; {line}")),
        (Err(e), None) => Err(e),
    }
}

/// The empty flag `p` in `reinject/` (directory 0700, file 0600 on unix, set also when they exist). Only `reinject/`
/// itself is created: a missing session root is an error, never created again. `reinject/` must be a real folder, not
/// a link (`session_start::folder_state`), before its mode is set. On unix the flag is opened with
/// `O_NOFOLLOW | O_NONBLOCK`: a link at the flag name is an error and is not followed, and a FIFO there does not make
/// the hook wait (with no reader the open fails); an opened entry that is not a regular file is an error, and its mode
/// is not set (FX-37).
fn create_flag(dir: &SessionDir, p: &Path) -> anyhow::Result<()> {
    let r = dir.reinject();
    let mut b = std::fs::DirBuilder::new();
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut b, 0o700);
    match b.create(&r) {
        Err(e) if e.kind() != std::io::ErrorKind::AlreadyExists => return Err(e.into()),
        _ => {}
    }
    if !matches!(folder_state(&r), Folder::Usable) {
        anyhow::bail!("{}: is a link or not a folder", r.display());
    }
    #[cfg(unix)]
    std::fs::set_permissions(&r, std::os::unix::fs::PermissionsExt::from_mode(0o700))?;
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC);
    }
    let f = o.open(p)?;
    if !f.metadata()?.is_file() {
        anyhow::bail!("{}: is not a regular file", p.display());
    }
    #[cfg(unix)]
    f.set_permissions(std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
    Ok(())
}

/// Every unit with no `stopped_at`, sorted by `(unit, round, agent stem)`. A missing work directory is empty; any other read error is an error.
fn open_units(work: &Path) -> anyhow::Result<Vec<Begin>> {
    let entries = match std::fs::read_dir(work) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(anyhow::anyhow!("cannot read the work directory")),
    };
    let mut units = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|_| anyhow::anyhow!("cannot read the work directory"))?;
        let name = entry.file_name();
        let Some(stem) = name.to_str().and_then(|n| n.strip_suffix(".begin.json")) else { continue };
        if let Some(b) = open_unit(work, stem).filter(|b| b.stopped_at.is_none()) {
            units.push((stem.to_string(), b));
        }
    }
    units.sort_by(|(sa, a), (sb, b)| (&a.unit, a.round, sa).cmp(&(&b.unit, b.round, sb)));
    Ok(units.into_iter().map(|(_, b)| b).collect())
}

/// One line: `\r` and `\n` become a space.
fn one_line(s: &str) -> String {
    s.replace(['\r', '\n'], " ")
}

/// A CommonMark code span: the fence is one backtick longer than the longest backtick run in the value; a space pads
/// the value inside the fence when it starts or ends with a backtick.
fn code(s: &str) -> String {
    let s = one_line(s);
    let longest = s.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat(longest + 1);
    let pad = if s.starts_with('`') || s.ends_with('`') { " " } else { "" };
    format!("{fence}{pad}{s}{pad}{fence}")
}

fn quoted(items: &[String]) -> String {
    if items.is_empty() {
        "none".into()
    } else {
        items.iter().map(|i| code(i)).collect::<Vec<_>>().join(", ")
    }
}

fn text(units: &[Begin]) -> String {
    let mut s = String::from("Keep in the summary: the plan or tracker path and the current step; ");
    if !units.is_empty() {
        let list: Vec<String> = units
            .iter()
            .map(|b| {
                let check = if b.check.is_empty() { "none".to_string() } else { code(&b.check) };
                format!("{} (round {}, check: {check}, files: {})", one_line(&b.unit), b.round, quoted(&b.files))
            })
            .collect();
        s.push_str(&format!("every open unit of work with its id, round, check and files: {}; ", list.join("; ")));
    }
    s.push_str("the pending briefs and what each subagent reported; the result ids zygo stored (result_view reads them again). Drop: tool output that is in the result store; file contents already edited and committed.");
    s
}
