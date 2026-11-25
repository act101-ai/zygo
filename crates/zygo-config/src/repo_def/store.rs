//! Workspace approvals (R§7.2): [[workspace]] id digest in the plugin approvals file, written with the protocol of
//! zygo_plugins::approve::approve_in.

use std::io::Write;
use std::path::Path;

use anyhow::{Context, bail};
use serde::Deserialize;

/// What `approve_workspace_in` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stored {
    Added,
    AlreadyPresent,
}

#[derive(Deserialize)]
struct File {
    #[serde(default)]
    workspace: Vec<Entry>,
}

#[derive(Deserialize)]
struct Entry {
    id: String,
    digest: String,
}

fn lower_hex(s: &str, len: usize) -> bool {
    s.len() == len && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn id16(s: &str) -> bool {
    lower_hex(s, 16)
}

fn hex64(s: &str) -> bool {
    lower_hex(s, 64)
}

impl File {
    fn has(&self, ws_id: &str, digest: &str) -> bool {
        self.workspace
            .iter()
            .any(|e| e.id == ws_id && e.digest == digest)
    }
}

/// The pair `(ws_id, digest)` is in `file`; an unreadable or invalid file approves nothing.
pub fn is_workspace_approved_in(file: &Path, ws_id: &str, digest: &str) -> bool {
    if !id16(ws_id) || !hex64(digest) {
        return false;
    }
    let Ok(text) = std::fs::read_to_string(file) else {
        return false;
    };
    let Ok(f) = toml::from_str::<File>(&text) else {
        return false;
    };
    f.has(ws_id, digest)
}

/// Adds the pair to `file` (once) under an exclusive lock on `<file>.lock`; temp file, `fsync`, rename.
/// An invalid file is an error and is left as it is.
pub fn approve_workspace_in(file: &Path, ws_id: &str, digest: &str) -> anyhow::Result<Stored> {
    if !id16(ws_id) {
        bail!("workspace id is not 16 lower-case hex characters");
    }
    if !hex64(digest) {
        bail!("digest is not 64 lower-case hex characters");
    }
    let dir = file
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir).with_context(|| dir.display().to_string())?;
    let name = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let lock_path = dir.join(format!("{name}.lock"));
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .with_context(|| lock_path.display().to_string())?;
    fs4::FileExt::lock(&lock).with_context(|| lock_path.display().to_string())?;

    let text = match std::fs::read_to_string(file) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e).with_context(|| file.display().to_string()),
    };
    let parsed = toml::from_str::<File>(&text)
        .with_context(|| format!("{}: invalid approvals file", file.display()))?;
    if parsed.has(ws_id, digest) {
        return Ok(Stored::AlreadyPresent);
    }
    // Appended as text: every byte already in the file stays.
    let q = |v: &str| toml::Value::String(v.to_string()).to_string();
    let mut out = text;
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(&format!(
        "[[workspace]]\nid = {}\ndigest = {}\n",
        q(ws_id),
        q(digest)
    ));
    toml::from_str::<File>(&out)
        .with_context(|| format!("{}: workspace is not an array of tables", file.display()))?;

    let tmp = dir.join(format!(".{name}.{}.tmp", zygo_core::ids::ulid()));
    write_replace(&tmp, &out, file, |from, to| std::fs::rename(from, to))?;
    drop(lock);
    Ok(Stored::Added)
}

/// Writes `out` to the new file `tmp`, `fsync`, then `rename(tmp, file)`; on any error `tmp` is removed.
fn write_replace(
    tmp: &Path,
    out: &str,
    file: &Path,
    rename: impl FnOnce(&Path, &Path) -> std::io::Result<()>,
) -> anyhow::Result<()> {
    let res = (|| -> std::io::Result<()> {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(tmp)?;
        f.write_all(out.as_bytes())?;
        f.sync_all()?;
        rename(tmp, file)
    })();
    if let Err(e) = res {
        let _ = std::fs::remove_file(tmp);
        return Err(e).with_context(|| file.display().to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// R§11 "Approval write fails": a rename that fails after the temp file exists leaves the file as it was and
    /// no temp file.
    #[test]
    fn failed_rename_removes_the_temp_file() {
        let t = tempfile::tempdir().unwrap();
        let file = t.path().join("approvals.toml");
        std::fs::write(&file, "# before\n").unwrap();
        let tmp = t.path().join(".approvals.toml.x.tmp");
        let r = write_replace(&tmp, "# after\n", &file, |from, _| {
            assert!(from.exists(), "the temp file exists when the rename runs");
            Err(std::io::Error::other("injected rename failure"))
        });
        assert!(r.is_err());
        assert_eq!(std::fs::read(&file).unwrap(), b"# before\n");
        let names: Vec<_> = std::fs::read_dir(t.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["approvals.toml"]);
    }
}
