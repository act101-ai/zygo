//! §10.6 compaction instructions: what to keep and what to drop.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::Value;
use zygo_core::hook::{error_line, HookInput, HookOutput};
use zygo_core::session_dir::SessionDir;
use zygo_work::log::{open_unit, Begin};

/// The hook entry: `run_to` with the session folder of the environment and the process stdout.
pub fn run(input: &HookInput) -> anyhow::Result<HookOutput> {
    run_to(input, SessionDir::from_env().as_ref(), &mut std::io::stdout())
}

/// `run_with` for the hook entry, with the stderr line after the text (T§14 item 2: the line is "reported as one
/// stderr line after the §10.6 text is printed"). When the text is given and the flag cannot be created, the text is
/// written to `out` and flushed, then the one stderr line is written, and the result is `HookOutput::None` (the
/// caller prints nothing more). Every other case is the result of `run_with`. A write error on `out` is ignored (C6).
pub fn run_to(input: &HookInput, dir: Option<&SessionDir>, out: &mut dyn Write) -> anyhow::Result<HookOutput> {
    match parts(input, dir) {
        (Ok(text), Some(line)) => {
            let _ = out.write_all(text.to_stdout().as_bytes());
            let _ = out.flush();
            error_line(&line);
            Ok(HookOutput::None)
        }
        (result, line) => joined(result, line),
    }
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
/// writes one line: `<run_in error>; pre-compact: cannot create <flag>: <reason>`. The stderr line of a flag that
/// cannot be created is written before this returns; the hook entry `run_to` writes it after the text.
pub fn run_with(input: &HookInput, dir: Option<&SessionDir>) -> anyhow::Result<HookOutput> {
    let (result, line) = parts(input, dir);
    joined(result, line)
}

/// The result of `run_in(dir)` and the error line of the flag (`None` when no flag is due or it was created).
fn parts(input: &HookInput, dir: Option<&SessionDir>) -> (anyhow::Result<HookOutput>, Option<String>) {
    let out = run_in(dir);
    let zygo = input.raw.get("agent_type").and_then(Value::as_str).is_some_and(|t| t.starts_with("zygo-"));
    let mut line = None;
    if !zygo
        && let (Some(d), Some(id)) = (dir, input.agent_id.as_deref())
        && let Some(p) = flag_path(d, id)
        && let Err(e) = create_flag(d, id)
    {
        line = Some(format!("pre-compact: cannot create {}: {e:#}", p.display()));
    }
    (out, line)
}

/// `run_with`'s join of the parts: with text, the line is written now and the text returned; with an error, the line
/// joins the error.
fn joined(result: anyhow::Result<HookOutput>, line: Option<String>) -> anyhow::Result<HookOutput> {
    match (result, line) {
        (Ok(out), Some(line)) => {
            error_line(&line);
            Ok(out)
        }
        (Ok(out), None) => Ok(out),
        (Err(e), Some(line)) => Err(anyhow::anyhow!("{e:#}; {line}")),
        (Err(e), None) => Err(e),
    }
}

/// Creates `reinject/` when it is missing, mode 0700 at creation. Only `reinject/` itself is created: a missing session
/// root is an error, never created again. Returns its path.
fn make_reinject(dir: &SessionDir) -> anyhow::Result<PathBuf> {
    let r = dir.reinject();
    let mut b = std::fs::DirBuilder::new();
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut b, 0o700);
    match b.create(&r) {
        Err(e) if e.kind() != std::io::ErrorKind::AlreadyExists => Err(e.into()),
        _ => Ok(r),
    }
}

/// The empty flag `reinject/<name>` (folder 0700, file 0600, set also when they exist; FX-37). `reinject/` is opened
/// once with `O_DIRECTORY | O_NOFOLLOW` (a link or another entry there is an error), and every later step goes through
/// that handle: `fchmod` of the folder, then `openat` of the flag. The flag is created with
/// `O_CREAT | O_EXCL | O_NOFOLLOW | O_NONBLOCK`, mode 0600; if setting its mode then fails, the new flag is removed.
/// A name that exists is opened `O_RDONLY | O_NOFOLLOW | O_NONBLOCK` (a link is an error, a FIFO does not make the
/// hook wait) and is the flag only when it is a regular file with one link; else it is an error and no mode is set
/// (a hard link shares its inode with another name).
#[cfg(unix)]
fn create_flag(dir: &SessionDir, name: &str) -> anyhow::Result<()> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let r = make_reinject(dir)?;
    let folder = at::open_folder(&r).map_err(|e| anyhow::anyhow!("{}: {e}", r.display()))?;
    folder.set_permissions(std::fs::Permissions::from_mode(0o700))?;
    let flags = libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_NOCTTY;
    match at::open(&folder, name, libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | flags, 0o600) {
        Ok(f) => {
            if let Err(e) = f.set_permissions(std::fs::Permissions::from_mode(0o600)) {
                let _ = at::unlink(&folder, name);
                return Err(e.into());
            }
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            let f = at::open(&folder, name, libc::O_RDONLY | flags, 0)?;
            let m = f.metadata()?;
            if !m.is_file() || m.nlink() != 1 {
                anyhow::bail!("{}: is not a regular file with one link", r.join(name).display());
            }
            f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}

/// The empty flag `reinject/<name>`: `reinject/` must be a real folder, not a link, and the flag a regular file.
#[cfg(not(unix))]
fn create_flag(dir: &SessionDir, name: &str) -> anyhow::Result<()> {
    use crate::session_start::{folder_state, Folder};
    let r = make_reinject(dir)?;
    if !matches!(folder_state(&r), Folder::Usable) {
        anyhow::bail!("{}: is a link or not a folder", r.display());
    }
    let p = r.join(name);
    let f = std::fs::OpenOptions::new().write(true).create(true).truncate(false).open(&p)?;
    if !f.metadata()?.is_file() {
        anyhow::bail!("{}: is not a regular file", p.display());
    }
    Ok(())
}

/// Steps on an entry of a folder through the folder's handle (`openat`, `fstatat`, `linkat`, `unlinkat`), so a link put
/// in place of the folder after it was opened is never followed (FX-37). Shared by `create_flag`, `trim`'s flag take
/// and `session_start::fixed_item` (FX-44).
#[cfg(unix)]
pub(crate) mod at {
    use std::ffi::CString;
    use std::fs::File;
    use std::io;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::path::Path;

    /// `path` opened as a folder with `O_DIRECTORY | O_NOFOLLOW`: a link is `ELOOP`, another entry `ENOTDIR`.
    pub(crate) fn open_folder(path: &Path) -> io::Result<File> {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new().read(true).custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC).open(path)
    }

    /// `openat(folder, name, flags | O_CLOEXEC, mode)`.
    pub(crate) fn open(folder: &File, name: &str, flags: libc::c_int, mode: libc::mode_t) -> io::Result<File> {
        let c = CString::new(name)?;
        // SAFETY: the folder descriptor is open for the call and `c` is a NUL-terminated name.
        let fd = unsafe { libc::openat(folder.as_raw_fd(), c.as_ptr(), flags | libc::O_CLOEXEC, libc::c_uint::from(mode)) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `fd` is a new descriptor that nothing else owns.
        Ok(unsafe { File::from_raw_fd(fd) })
    }

    /// Whether `name` in `folder` is a regular file, by `fstatat(…, AT_SYMLINK_NOFOLLOW)`: a link is not followed.
    pub(crate) fn is_regular(folder: &File, name: &str) -> io::Result<bool> {
        let c = CString::new(name)?;
        // SAFETY: `stat` is plain data; an all-zero value is valid and is overwritten by the call.
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: the folder descriptor is open, `c` is NUL-terminated and `st` is a valid out pointer.
        if unsafe { libc::fstatat(folder.as_raw_fd(), c.as_ptr(), &mut st, libc::AT_SYMLINK_NOFOLLOW) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(st.st_mode & libc::S_IFMT == libc::S_IFREG)
    }

    /// `linkat(folder, old, folder, new, 0)`: a new name `new` for the entry `old`, both in `folder`. Flag 0: a link
    /// at `old` is not followed. A taken `new` is `AlreadyExists`.
    pub(crate) fn link(folder: &File, old: &str, new: &str) -> io::Result<()> {
        let (o, n) = (CString::new(old)?, CString::new(new)?);
        let fd = folder.as_raw_fd();
        // SAFETY: the folder descriptor is open for the call and `o` and `n` are NUL-terminated names.
        if unsafe { libc::linkat(fd, o.as_ptr(), fd, n.as_ptr(), 0) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// Whether `folder` has an entry whose name starts with `prefix` and ends with `suffix` and that is the file
    /// `(dev, ino)` (`fstatat(…, AT_SYMLINK_NOFOLLOW)`, a link is not followed). The entries are read through a copy of
    /// the folder handle (`fdopendir`), from the start. A read error ends the search (`false`).
    pub(crate) fn has_link(folder: &File, prefix: &str, suffix: &str, dev: u64, ino: u64) -> io::Result<bool> {
        // SAFETY: the folder descriptor is open for the call; the copy is new and owned by `d` below.
        let fd = unsafe { libc::fcntl(folder.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 0) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `fd` is an open folder descriptor that nothing else owns; on success `d` owns it.
        let d = unsafe { libc::fdopendir(fd) };
        if d.is_null() {
            let e = io::Error::last_os_error();
            // SAFETY: `fdopendir` failed, so `fd` is still this call's own.
            unsafe { libc::close(fd) };
            return Err(e);
        }
        // SAFETY: `d` is a valid stream; the copy shares its offset with `folder`, so the read starts from the start.
        unsafe { libc::rewinddir(d) };
        let mut found = false;
        loop {
            // SAFETY: `d` is a valid stream; the entry is valid until the next `readdir` or `closedir`.
            let ent = unsafe { libc::readdir(d) };
            if ent.is_null() {
                break;
            }
            // SAFETY: `d_name` of a valid entry is a NUL-terminated name.
            let name = unsafe { std::ffi::CStr::from_ptr((*ent).d_name.as_ptr()) };
            let Ok(n) = name.to_str() else { continue };
            if !(n.starts_with(prefix) && n.ends_with(suffix)) {
                continue;
            }
            // SAFETY: `stat` is plain data; an all-zero value is valid and is overwritten by the call.
            let mut st: libc::stat = unsafe { std::mem::zeroed() };
            // SAFETY: the folder descriptor is open, `name` is NUL-terminated and `st` is a valid out pointer.
            let ok = unsafe { libc::fstatat(folder.as_raw_fd(), name.as_ptr(), &mut st, libc::AT_SYMLINK_NOFOLLOW) } == 0;
            #[allow(clippy::unnecessary_cast)]
            if ok && st.st_dev as u64 == dev && st.st_ino as u64 == ino {
                found = true;
                break;
            }
        }
        // SAFETY: `d` is a valid stream, closed once; this also closes `fd`.
        unsafe { libc::closedir(d) };
        Ok(found)
    }

    /// `unlinkat(folder, name, 0)`.
    pub(crate) fn unlink(folder: &File, name: &str) -> io::Result<()> {
        let c = CString::new(name)?;
        // SAFETY: the folder descriptor is open for the call and `c` is a NUL-terminated name.
        if unsafe { libc::unlinkat(folder.as_raw_fd(), c.as_ptr(), 0) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
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
