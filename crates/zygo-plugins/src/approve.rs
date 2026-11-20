//! Code approvals (spec §15.7): `(marketplace, plugin, code digest)` in `approvals.toml`.

use std::io::Write;
use std::path::Path;

use anyhow::{bail, Context};
use serde::Deserialize;
use zygo_core::paths::Paths;

use crate::check::valid_name;

/// An approval that config or the release carries (team approvals, built-in digests).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreApproval { pub marketplace: String, pub plugin: String, pub digest: String }

#[derive(Deserialize)]
struct File { #[serde(default)] approval: Vec<Entry> }
#[derive(Deserialize)]
struct Entry { marketplace: String, plugin: String, digest: String }

fn hex64(d: &str) -> bool { d.len() == 64 && d.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) }

/// `is_approved_in` the approvals file of the environment.
pub fn is_approved(m: &str, p: &str, digest: &str) -> bool { is_approved_in(&Paths::from_env().approvals_file, m, p, digest) }

/// `approve_in` the approvals file of the environment.
pub fn approve(m: &str, p: &str, digest: &str) -> anyhow::Result<()> { approve_in(&Paths::from_env().approvals_file, m, p, digest) }

/// The triple is in `file`; an unreadable or invalid file approves nothing.
pub fn is_approved_in(file: &Path, m: &str, p: &str, digest: &str) -> bool {
    let Ok(text) = std::fs::read_to_string(file) else { return false };
    let Ok(f) = toml::from_str::<File>(&text) else { return false };
    hex64(digest) && f.approval.iter().any(|e| e.marketplace == m && e.plugin == p && e.digest == digest)
}

/// Adds the triple to `file` (once) under an exclusive lock on `<file>.lock`; temp file plus rename.
/// An invalid file is an error and is left as it is.
pub fn approve_in(file: &Path, m: &str, p: &str, digest: &str) -> anyhow::Result<()> {
    if !hex64(digest) { bail!("digest is not 64 lower-case hex characters"); }
    if !valid_name(m) || !valid_name(p) { bail!("unsafe marketplace or plugin name"); }
    let dir = file.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir).with_context(|| dir.display().to_string())?;
    let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let lock_path = dir.join(format!("{name}.lock"));
    let lock = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&lock_path)
        .with_context(|| lock_path.display().to_string())?;
    fs4::FileExt::lock(&lock).with_context(|| lock_path.display().to_string())?;

    let text = match std::fs::read_to_string(file) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e).with_context(|| file.display().to_string()),
    };
    toml::from_str::<File>(&text).with_context(|| format!("{}: invalid approvals file", file.display()))?;
    if is_approved_in(file, m, p, digest) { return Ok(()); }
    // Appended as text: every byte already in the file stays.
    let q = |v: &str| toml::Value::String(v.to_string()).to_string();
    let mut out = text;
    if !out.is_empty() && !out.ends_with('\n') { out.push('\n'); }
    if !out.is_empty() { out.push('\n'); }
    out.push_str(&format!("[[approval]]\nmarketplace = {}\nplugin = {}\ndigest = {}\n", q(m), q(p), q(digest)));
    toml::from_str::<File>(&out).with_context(|| format!("{}: approval is not an array of tables", file.display()))?;

    let tmp = dir.join(format!(".{name}.{}.tmp", zygo_core::ids::ulid()));
    let res = (|| -> std::io::Result<()> {
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        f.write_all(out.as_bytes())?;
        f.sync_all()?;
        std::fs::rename(&tmp, file)
    })();
    if let Err(e) = res {
        let _ = std::fs::remove_file(&tmp);
        return Err(e).with_context(|| file.display().to_string());
    }
    drop(lock);
    Ok(())
}
