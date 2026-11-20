//! §10.6 compaction instructions: what to keep and what to drop.

use std::path::Path;

use zygo_core::hook::{HookInput, HookOutput};
use zygo_core::session_dir::SessionDir;
use zygo_work::log::{open_unit, Begin};

pub fn run(_input: &HookInput) -> anyhow::Result<HookOutput> {
    run_in(SessionDir::from_env().as_ref())
}

pub fn run_in(dir: Option<&SessionDir>) -> anyhow::Result<HookOutput> {
    let units = match dir {
        Some(d) => open_units(&d.work())?,
        None => Vec::new(),
    };
    Ok(HookOutput::Text(text(&units)))
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
