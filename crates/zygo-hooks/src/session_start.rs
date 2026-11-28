//! §8.6: the session text as additional context at session start; for a workspace member also the resume note and
//! the member record (workspaces design W§9.1, W§9.4; H1); for `--event subagent` the mechanics text only (session text
//! design T§7.2 item 2).

use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use zygo_core::hook::{error_line, HookInput, HookOutput};
use zygo_core::overrides::Overrides;
use zygo_core::paths::Paths;
use zygo_core::session_dir::SessionDir;
use zygo_core::session_view::SessionView;
use zygo_core::Agent;

use crate::session_text::{build, mechanics};
use crate::workspace;

pub fn run(input: &HookInput) -> anyhow::Result<HookOutput> {
    let Some(d) = SessionDir::from_env() else { return Ok(HookOutput::None) };
    run_member(input, &d, &Paths::from_env(), &std::env::current_exe().unwrap_or_default(), &mut std::io::stdout())
}

pub fn run_in(input: &HookInput, dir: &SessionDir) -> anyhow::Result<HookOutput> {
    // T§6.3 item 2: one branch for every input with `agent_id`; the member part never runs for it.
    if input.has_agent_id {
        return subagent(input, dir);
    }
    if input.cli_event.as_deref() == Some("subagent") {
        return subagent_text(dir);
    }
    let v = SessionView::load(dir)?;
    Ok(output(v.agent, text_of(dir, &v)))
}

/// T§6.3 item 2: the output for an input with `agent_id`. A `zygo-` agent type (the rule of
/// `zygo_work::hooks::is_zygo`) gets nothing: its `--agents` prompt holds the mechanics text (§8.5). Any other
/// subagent gets the mechanics text on `SubagentStart`; a missing or non-string `agent_type` is not `zygo-`. Any
/// other event (a subagent `SessionStart` with `source: "compact"`) gets nothing; T§9.2 owns that copy.
fn subagent(input: &HookInput, dir: &SessionDir) -> anyhow::Result<HookOutput> {
    let zygo = input.raw.get("agent_type").and_then(Value::as_str).is_some_and(|t| t.starts_with("zygo-"));
    if zygo || input.event.as_deref() != Some("SubagentStart") {
        return Ok(HookOutput::None);
    }
    let v = SessionView::load(dir)?;
    Ok(HookOutput::Json(json!({"hookSpecificOutput": {"hookEventName": "SubagentStart",
        "additionalContext": mechanics_with_overrides(dir, &v)}})))
}

/// T§7.2 item 2: `session-start --event subagent` (the OpenCode transform for a request on a `zygo/` model that is
/// not the main role): the mechanics text with the overrides applied (T-C5), as plain text for every view agent;
/// no member part, no resume note, no write. A load error is the caller's error (T§14 item 1).
fn subagent_text(dir: &SessionDir) -> anyhow::Result<HookOutput> {
    Ok(HookOutput::Text(mechanics_with_overrides(dir, &SessionView::load(dir)?)))
}

/// The session text of `v` with the overrides of the session folder applied.
fn text_of(dir: &SessionDir, v: &SessionView) -> String {
    build(&with_overrides(dir, v))
}

/// `mechanics(v)` with the `results` and `work_units` overrides of the session folder applied (T-C5).
pub fn mechanics_with_overrides(dir: &SessionDir, v: &SessionView) -> String {
    mechanics(&with_overrides(dir, v))
}

/// A copy of `v` with the `results` and `work_units` overrides of the session folder applied.
fn with_overrides(dir: &SessionDir, v: &SessionView) -> SessionView {
    let mut v = v.clone();
    let ov = Overrides::load(dir);
    v.results = ov.results.unwrap_or(v.results);
    v.work_units = ov.work_units.unwrap_or(v.work_units);
    v
}

/// The hook output of `text` for `agent`.
fn output(agent: Agent, text: String) -> HookOutput {
    match agent {
        Agent::ClaudeCode => {
            HookOutput::Json(json!({"hookSpecificOutput": {"hookEventName": "SessionStart", "additionalContext": text}}))
        }
        _ => HookOutput::Text(text),
    }
}

/// The folder of the stored OpenCode item 8 in the session folder, one file per OpenCode session id (FX-9).
const ITEM_DIR: &str = "session-item";

/// The most bytes of a stored item 8 that are read; a larger file is not used.
const MAX_ITEM: u64 = 1 << 20;

/// `<dir>/session-item/<agent_session>`, the stored item 8 of an OpenCode session (FX-9 Decision 3), when
/// `agent_session` passes the rule of `pre_compact::flag_path` (1-64 of `[A-Za-z0-9_-]`, T§9.2 item 1), so the path
/// stays in the session folder; else `None`.
pub fn item_path(dir: &SessionDir, agent_session: &str) -> Option<PathBuf> {
    crate::pre_compact::flag_path(dir, agent_session).map(|_| dir.root.join(ITEM_DIR).join(agent_session))
}

/// Item 8 of the OpenCode session `agent_session`, fixed at its first main-loop request (M§8a item 1, FX-9
/// Decisions 1-5). A stored text is returned as it is and `build` is not called. Else `build()`; a `Some` text is
/// stored first-writer-wins (a temp file, then a hard link that fails when the name is taken), and a call that loses
/// the race returns the stored text. `None` stores nothing. An id that fails `item_path` gives `build()` with no
/// store. A stored entry that is a link, not a regular file, too large or unreadable gives `build()` with no store and
/// one `log` line; a store that fails gives the built text and one `log` line.
pub fn fixed_item(
    dir: &SessionDir, agent_session: &str, build: &mut dyn FnMut() -> Option<String>, log: &dyn Fn(&str),
) -> Option<String> {
    let Some(path) = item_path(dir, agent_session) else { return build() };
    match read_item(&path) {
        Ok(Some(t)) => return Some(t),
        Ok(None) => {}
        Err(e) => {
            log(&format!("{}: {e:#}; item 8 not stored", path.display()));
            return build();
        }
    }
    let text = build()?;
    match store_item(dir, &path, &text) {
        Ok(true) => Some(text),
        // Another request stored its text after this one read: that text is the session's item 8.
        Ok(false) => match read_item(&path) {
            Ok(Some(t)) => Some(t),
            Ok(None) => Some(text),
            Err(e) => {
                log(&format!("{}: {e:#}", path.display()));
                Some(text)
            }
        },
        Err(e) => {
            log(&format!("{}: cannot store item 8: {e:#}", path.display()));
            Some(text)
        }
    }
}

/// The stored text at `path`: `Ok(None)` when nothing is there. Opened with `O_NOFOLLOW | O_NONBLOCK`, so a link is
/// not followed and a FIFO does not make the reader wait; only a regular file of at most `MAX_ITEM` bytes of UTF-8 is
/// read.
fn read_item(path: &Path) -> anyhow::Result<Option<String>> {
    let mut o = std::fs::OpenOptions::new();
    o.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC);
    }
    let f = match o.open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(anyhow::Error::new(e).context("cannot open")),
    };
    let m = f.metadata()?;
    if !m.is_file() {
        anyhow::bail!("is not a regular file");
    }
    if m.len() > MAX_ITEM {
        anyhow::bail!("is larger than {MAX_ITEM} bytes");
    }
    let mut text = String::new();
    std::io::Read::read_to_string(&mut std::io::Read::take(f, MAX_ITEM), &mut text)?;
    Ok(Some(text))
}

/// Stores `text` at `path` unless the name is taken: `Ok(true)` when stored, `Ok(false)` when the name is taken, so a
/// stored text is never replaced. `session-item/` is made when missing (0700, set also when it exists, as `reinject/`
/// in `pre_compact`); `text` goes to a new temp file next to `path` (0600, synced, a name unique to this process and
/// call), which is hard-linked to `path` and then removed. A missing session folder is an error and is not made again.
fn store_item(dir: &SessionDir, path: &Path, text: &str) -> anyhow::Result<bool> {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let folder = dir.root.join(ITEM_DIR);
    let mut b = std::fs::DirBuilder::new();
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut b, 0o700);
    match b.create(&folder) {
        Err(e) if e.kind() != std::io::ErrorKind::AlreadyExists => return Err(e.into()),
        _ => {}
    }
    #[cfg(unix)]
    std::fs::set_permissions(&folder, std::os::unix::fs::PermissionsExt::from_mode(0o700))?;
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = folder.join(format!(".{name}.{}.{seq}.tmp", std::process::id()));
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
    let mut f = o.open(&tmp)?;
    let linked = (|| -> std::io::Result<bool> {
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
        match std::fs::hard_link(&tmp, path) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
            Err(e) => Err(e),
        }
    })();
    // The temp file is this call's own; the stored text stays under `path`.
    let _ = std::fs::remove_file(&tmp);
    Ok(linked?)
}

/// `run_in` with the member part (H1 D3, D11). The view is loaded as `run_in` loads it. A launch that is not a member
/// gets the output or the error of `run_in` and no other effect. The Claude Code `SessionStart` entry of a member is
/// `session-start --event member` (`input.cli_event == Some("member")`): the session folder is checked first, and a
/// folder that `workspace::check_folder` refuses gives one stderr line (the D2 text), the session text of `run_in`'s
/// read with no note (nothing when that read fails) and no write (attempt-2 review round 1, F3); after the check a
/// load error writes one session-log line and prints nothing (a `view.json` that is a FIFO, a folder or not JSON is a
/// load error); a `view.json` that is a link to a regular view loads, then writes one session-log line and gives the
/// session text with no note and no other change, as does an entry that is no longer a regular file after the load.
/// `--event subagent` (`input.cli_event == Some("subagent")`, the OpenCode transform for another role, T§7.2) gives
/// `subagent_text` before any member part.
/// Without the flag (a launch that is not a member, or an OpenCode request on the main role) a load error is `run_in`'s
/// error and that check does not run. A session folder that `workspace::check_folder` refuses skips the member part.
/// Else `workspace::session_start` runs; a Claude Code member's output is printed to `out` there, so the result is
/// `HookOutput::None`; an OpenCode member's item 8 is fixed per OpenCode session id there (`fixed_item`, FX-9). A `view.json` that is a FIFO is a load error of `SessionView::load` (`<path>: is a FIFO`), with
/// no wait, for every launch (FIX-22).
pub fn run_member(
    input: &HookInput, dir: &SessionDir, paths: &Paths, zygo: &Path, out: &mut dyn Write,
) -> anyhow::Result<HookOutput> {
    if input.has_agent_id {
        return subagent(input, dir);
    }
    if input.cli_event.as_deref() == Some("subagent") {
        return subagent_text(dir);
    }
    let member = input.cli_event.as_deref() == Some("member");
    if member && let Err(e) = workspace::check_folder(dir) {
        // D2 (attempt-2 review round 1, F3): nothing is written into a refused folder; the session text of today's
        // read is printed with no note, and the refusal is the one stderr line.
        error_line(&format!("{e:#}"));
        return Ok(match SessionView::load(dir) {
            Ok(v) => output(v.agent, text_of(dir, &v)),
            Err(_) => HookOutput::None,
        });
    }
    let v = match SessionView::load(dir) {
        Ok(v) => v,
        Err(e) if member => {
            dir.append_log(&format!("workspace hook session-start: {e:#}"));
            return Ok(HookOutput::None);
        }
        Err(e) => return Err(e),
    };
    let text = text_of(dir, &v);
    if v.workspace.is_none() || workspace::check_folder(dir).is_err() {
        return Ok(output(v.agent, text));
    }
    if member {
        let view = dir.view_json();
        let refused = match std::fs::symlink_metadata(&view) {
            Ok(m) if m.file_type().is_symlink() => Some("is a link"),
            Ok(m) if !m.is_file() => Some("is not a regular file"),
            Ok(_) => None,
            // Gone since the load: nothing to change in it.
            Err(_) => return Ok(output(v.agent, text)),
        };
        if let Some(why) = refused {
            dir.append_log(&format!("workspace hook session-start: {}: {why}; not read", view.display()));
            return Ok(output(v.agent, text));
        }
    }
    Ok(match workspace::session_start(&input.raw, dir, &v, paths, zygo, out, &text)? {
        None => HookOutput::None,
        Some(t) => output(v.agent, t),
    })
}
