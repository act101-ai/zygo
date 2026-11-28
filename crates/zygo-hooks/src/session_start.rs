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

/// How many temp names one store tries before it gives up (FX-9 review F7).
const TEMP_TRIES: u32 = 8;

/// `<dir>/session-item/<hex>`, the stored item 8 of an OpenCode session (FX-9 Decision 3), when `agent_session`
/// passes the rule of `pre_compact::flag_path` (1-64 of `[A-Za-z0-9_-]`, T§9.2 item 1); else `None`. `<hex>` is the
/// lower-case hex of the id's bytes (at most 128 characters), so ids that differ only in case get different names on
/// a case-insensitive file system (review F8). The path is used only while `session-item` is a real folder, not a
/// link (`fixed_item`, review F6).
pub fn item_path(dir: &SessionDir, agent_session: &str) -> Option<PathBuf> {
    crate::pre_compact::flag_path(dir, agent_session)?;
    let hex: String = agent_session.bytes().map(|b| format!("{b:02x}")).collect();
    Some(dir.root.join(ITEM_DIR).join(hex))
}

/// Item 8 of the OpenCode session `agent_session`, fixed at its first main-loop request (M§8a item 1, FX-9
/// Decisions 1-5). A stored, non-empty text is returned as it is and `build` is not called. Else `build()`; a `Some`
/// text is stored first-writer-wins (a temp file, then a hard link that fails when the name is taken), and a call that
/// loses the race returns the stored text. `None` stores nothing. An id that fails `item_path` gives `build()` with no
/// store. A store that cannot be used gives `build()` and stores nothing: a `session-item` that is a link or not a
/// folder, a stored entry that is a link, not a regular file, empty, larger than `MAX_ITEM` or unreadable, or a store
/// that fails. It writes no line: the OpenCode call runs per model request (D6 of `workspace::session_start_with_sync`;
/// review F1). On unix `session-item` is opened once (`open_item_folder`) and every read and store step goes through
/// that handle (FX-44); a stored entry with more than one link is not used (FX-44 review 2).
pub fn fixed_item(dir: &SessionDir, agent_session: &str, build: &mut dyn FnMut() -> Option<String>) -> Option<String> {
    let Some(path) = item_path(dir, agent_session) else { return build() };
    fixed_in(&dir.root.join(ITEM_DIR), &path, build)
}

/// `fixed_item` for the folder `folder` and the item `path` in it, through one folder handle (FX-44).
#[cfg(unix)]
fn fixed_in(folder: &Path, path: &Path, build: &mut dyn FnMut() -> Option<String>) -> Option<String> {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else { return build() };
    let handle = match open_item_folder(folder) {
        Ok(h) => h,
        Err(()) => return build(),
    };
    if let Some(h) = &handle {
        match read_item_in(h, name) {
            Ok(Some(t)) => return Some(t),
            Ok(None) => {}
            Err(()) => return build(),
        }
    }
    let text = build()?;
    let h = match handle {
        Some(h) => h,
        None => match make_folder(folder).and_then(|()| open_item_folder(folder)) {
            Ok(Some(h)) => h,
            _ => return Some(text),
        },
    };
    match store_item_in(&h, name, &text) {
        // Another request stored its text after this one read: that text is the session's item 8.
        Ok(false) => match read_item_in(&h, name) {
            Ok(Some(t)) => Some(t),
            _ => Some(text),
        },
        Ok(true) | Err(()) => Some(text),
    }
}

/// `fixed_item` for the folder `folder` and the item `path` in it, by path: `folder` must be a real folder, not a link.
#[cfg(not(unix))]
fn fixed_in(folder: &Path, path: &Path, build: &mut dyn FnMut() -> Option<String>) -> Option<String> {
    match folder_state(folder) {
        Folder::Unusable => return build(),
        Folder::Missing => {}
        Folder::Usable => match read_item(path) {
            Ok(Some(t)) => return Some(t),
            Ok(None) => {}
            Err(()) => return build(),
        },
    }
    let text = build()?;
    match store_item(folder, path, &text) {
        // Another request stored its text after this one read: that text is the session's item 8.
        Ok(false) => match read_item(path) {
            Ok(Some(t)) => Some(t),
            _ => Some(text),
        },
        Ok(true) | Err(()) => Some(text),
    }
}

/// `session-item/` opened with `O_DIRECTORY | O_NOFOLLOW`: `Ok(None)` when it is missing, `Err(())` when it is a link,
/// not a folder or cannot be opened. A real folder (not a link) owned by this user that cannot be opened for lack of
/// read permission (`EACCES`, for example mode 0300) is set to 0700 by path and opened once more (FX-44 review 3).
#[cfg(unix)]
fn open_item_folder(folder: &Path) -> Result<Option<std::fs::File>, ()> {
    use crate::pre_compact::at;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let h = match at::open_folder(folder) {
        Ok(h) => h,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) if e.raw_os_error() == Some(libc::EACCES) => {
            let m = std::fs::symlink_metadata(folder).map_err(|_| ())?;
            // SAFETY: `geteuid` has no preconditions and cannot fail.
            if !m.file_type().is_dir() || m.uid() != unsafe { libc::geteuid() } {
                return Err(());
            }
            std::fs::set_permissions(folder, std::fs::Permissions::from_mode(0o700)).map_err(|_| ())?;
            at::open_folder(folder).map_err(|_| ())?
        }
        Err(_) => return Err(()),
    };
    #[cfg(feature = "test-hooks")]
    testing::fire(folder);
    Ok(Some(h))
}

/// The stored text `name` in the opened folder `folder`, read as `read_item` reads, through the handle (`openat` with
/// `O_NOFOLLOW | O_NONBLOCK`). Only a regular file with no name outside the folder is used (FX-44 review 2, the rule of
/// the FX-37 flag): one link, or two while a store is in progress (`store_in_progress`).
#[cfg(unix)]
fn read_item_in(folder: &std::fs::File, name: &str) -> Result<Option<String>, ()> {
    use std::os::unix::fs::MetadataExt;
    let flags = libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_NOCTTY;
    let f = match crate::pre_compact::at::open(folder, name, flags, 0) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(()),
    };
    let m = f.metadata().map_err(|_| ())?;
    if m.nlink() != 1 && !store_in_progress(folder, name, &f, &m) {
        return Err(());
    }
    read_opened(f)
}

/// Whether the opened item `f` (`name` in `folder`, metadata `m`) has two links because a store is in progress
/// (`store_item_in` links its temp file to `name`, then removes the temp name): the other link is a temp name of `name`
/// (`.<name>.….tmp`, `temp_name`) in the same folder, so the file has no name outside it. When no such temp name is
/// found, the store may have removed it since `m` was read: the file is read again and must then have one link.
#[cfg(unix)]
fn store_in_progress(folder: &std::fs::File, name: &str, f: &std::fs::File, m: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    if m.nlink() != 2 {
        return false;
    }
    let prefix = format!(".{name}.");
    if crate::pre_compact::at::has_link(folder, &prefix, ".tmp", m.dev(), m.ino()).unwrap_or(false) {
        return true;
    }
    f.metadata().is_ok_and(|m| m.nlink() == 1)
}

/// The text of the opened entry `f`: only a regular, non-empty file of at most `MAX_ITEM` bytes of UTF-8 is used
/// (review F2, F5); else `Err(())`.
fn read_opened(f: std::fs::File) -> Result<Option<String>, ()> {
    let m = f.metadata().map_err(|_| ())?;
    if !m.is_file() || m.len() == 0 || m.len() > MAX_ITEM {
        return Err(());
    }
    let mut text = String::new();
    std::io::Read::read_to_string(&mut std::io::Read::take(f, MAX_ITEM), &mut text).map_err(|_| ())?;
    if text.is_empty() {
        return Err(());
    }
    Ok(Some(text))
}

/// What `session-item` (and `reinject/` in `pre_compact`, FX-37) is.
#[cfg(not(unix))]
pub(crate) enum Folder {
    Missing,
    /// A real folder (not a link).
    Usable,
    /// A link, or not a folder, or a stat error other than NotFound.
    Unusable,
}

#[cfg(not(unix))]
pub(crate) fn folder_state(folder: &Path) -> Folder {
    match std::fs::symlink_metadata(folder) {
        Ok(m) if m.file_type().is_dir() => Folder::Usable,
        Ok(_) => Folder::Unusable,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Folder::Missing,
        Err(_) => Folder::Unusable,
    }
}

/// The stored text at `path`: `Ok(None)` when nothing is there, `Err(())` when the entry cannot be used
/// (`read_opened`).
#[cfg(not(unix))]
fn read_item(path: &Path) -> Result<Option<String>, ()> {
    match std::fs::File::open(path) {
        Ok(f) => read_opened(f),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(()),
    }
}

/// A new temp name for the item `name`: the process id, a per-process counter and the clock's nanoseconds (review F7).
fn temp_name(name: &str) -> String {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    format!(".{name}.{}.{seq}.{nanos}.tmp", std::process::id())
}

/// Creates `folder` when it is missing, mode 0700 at creation. Only `folder` itself is made: a missing session folder
/// is not made again.
fn make_folder(folder: &Path) -> Result<(), ()> {
    let mut b = std::fs::DirBuilder::new();
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut b, 0o700);
    match b.create(folder) {
        Err(e) if e.kind() != std::io::ErrorKind::AlreadyExists => Err(()),
        _ => Ok(()),
    }
}

/// Stores `text` as `name` in the opened folder `dir` unless the name is taken: `Ok(true)` when stored, `Ok(false)`
/// when the name is taken, so a stored text is never replaced; `Err(())` when the store cannot be used. Every step goes
/// through the handle (FX-44), so a link put in place of the folder after it was opened (`open_item_folder`) is never
/// followed: `fchmod` 0700 of the folder (as `reinject/` in `pre_compact`); `openat` of a new temp file
/// (`O_CREAT | O_EXCL | O_NOFOLLOW`, 0600) that gets `text` and is synced; `linkat` of the temp name to `name`;
/// `unlinkat` of the temp name. A temp name that exists is tried again with a new name, at most `TEMP_TRIES` times
/// (review F7).
#[cfg(unix)]
fn store_item_in(dir: &std::fs::File, name: &str, text: &str) -> Result<bool, ()> {
    use crate::pre_compact::at;
    use std::os::unix::fs::PermissionsExt;
    dir.set_permissions(std::fs::Permissions::from_mode(0o700)).map_err(|_| ())?;
    let flags = libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_NOCTTY;
    let mut opened = None;
    for _ in 0..TEMP_TRIES {
        let tmp = temp_name(name);
        match at::open(dir, &tmp, flags, 0o600) {
            Ok(f) => {
                opened = Some((tmp, f));
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(()),
        }
    }
    let Some((tmp, mut f)) = opened else { return Err(()) };
    let linked = (|| -> std::io::Result<bool> {
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
        match at::link(dir, &tmp, name) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
            Err(e) => Err(e),
        }
    })();
    // The temp file is this call's own; the stored text stays under the item name.
    let _ = at::unlink(dir, &tmp);
    linked.map_err(|_| ())
}

/// Stores `text` at `path` in `folder` unless the name is taken, as the unix `store_item_in` but by path: `folder` is made
/// when missing and must then be a real folder, not a link (review F6); `text` goes to a new temp file in `folder`,
/// which is hard-linked to `path` and then removed.
#[cfg(not(unix))]
fn store_item(folder: &Path, path: &Path, text: &str) -> Result<bool, ()> {
    make_folder(folder)?;
    if !matches!(folder_state(folder), Folder::Usable) {
        return Err(());
    }
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    let mut opened = None;
    for _ in 0..TEMP_TRIES {
        let tmp = folder.join(temp_name(&name));
        match o.open(&tmp) {
            Ok(f) => {
                opened = Some((tmp, f));
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err(()),
        }
    }
    let Some((tmp, mut f)) = opened else { return Err(()) };
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
    linked.map_err(|_| ())
}

/// Test hooks of `fixed_item` (FX-44). Only the `test-hooks` feature builds them.
#[cfg(all(unix, feature = "test-hooks"))]
pub mod testing {
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};

    type Hook = Arc<dyn Fn() + Send + Sync>;

    /// The hooks by `session-item` folder path, so parallel tests do not meet.
    static HOOKS: Mutex<BTreeMap<PathBuf, Hook>> = Mutex::new(BTreeMap::new());

    /// Sets (`Some`) or clears (`None`) the hook of the `session-item` folder `folder`. The hook runs one time, when
    /// `open_item_folder` has opened the folder, before the first read or store step through the handle (a test
    /// replaces the folder there); it is then cleared.
    pub fn set_after_open(folder: &Path, hook: Option<Hook>) {
        let mut hooks = HOOKS.lock().unwrap_or_else(|e| e.into_inner());
        match hook {
            Some(h) => hooks.insert(folder.to_path_buf(), h),
            None => hooks.remove(folder),
        };
    }

    /// Runs the hook of `folder`, if one is set, and clears it.
    pub(crate) fn fire(folder: &Path) {
        let hook = HOOKS.lock().unwrap_or_else(|e| e.into_inner()).remove(folder);
        if let Some(h) = hook {
            h();
        }
    }
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
