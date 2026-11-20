//! Provider keys and the OS secret store (§6.2). No error or output here holds a secret value.

use crate::schema::KeyRef;
use anyhow::{Context, Result};
use std::collections::{BTreeMap, HashMap};
use std::ffi::OsString;
use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use zygo_core::paths::Paths;

pub const SERVICE: &str = "zygo";
pub const FILE_NAME: &str = "keys.toml";
pub const PROBE_NAME: &str = "zygo-probe";

/// A secret value. No `Display`; `Debug` prints `Secret(<redacted>)`.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(v: impl Into<String>) -> Secret {
        Secret(v.into())
    }
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    OsKeychain,
    File,
}

impl fmt::Display for Backend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Backend::OsKeychain => f.write_str("os-keychain"),
            #[cfg(unix)]
            Backend::File => f.write_str("file (0600)"),
            #[cfg(not(unix))]
            Backend::File => f.write_str("file"),
        }
    }
}

pub trait SecretStore: Send + Sync {
    fn backend(&self) -> Backend;
    /// Absent is `Ok(None)`.
    fn get(&self, name: &str) -> Result<Option<Secret>>;
    /// Returns the backend that took the value.
    fn set(&self, name: &str, value: &Secret) -> Result<Backend>;
    /// True if something was removed.
    fn delete(&self, name: &str) -> Result<bool>;
    /// `set`, also reporting the fallback when the preferred backend refused the value.
    fn set_reporting(&self, name: &str, value: &Secret) -> Result<(Backend, Option<Fallback>)> {
        self.set(name, value).map(|b| (b, None))
    }
}

/// Why a value went to the file instead of the OS store. Holds no secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fallback {
    /// The error kind, for example `PlatformFailure`.
    pub reason: String,
    pub file: PathBuf,
}

/// In-memory store; reports `OsKeychain`.
#[derive(Default)]
pub struct MemStore(Mutex<HashMap<String, String>>);

impl MemStore {
    pub fn new() -> MemStore {
        MemStore::default()
    }
}

impl SecretStore for MemStore {
    fn backend(&self) -> Backend {
        Backend::OsKeychain
    }
    fn get(&self, name: &str) -> Result<Option<Secret>> {
        Ok(self.0.lock().unwrap().get(name).map(|v| Secret::new(v.clone())))
    }
    fn set(&self, name: &str, value: &Secret) -> Result<Backend> {
        self.0.lock().unwrap().insert(name.into(), value.expose().into());
        Ok(Backend::OsKeychain)
    }
    fn delete(&self, name: &str) -> Result<bool> {
        Ok(self.0.lock().unwrap().remove(name).is_some())
    }
}

/// Plaintext fallback: a TOML table of name to value, mode 0600 on unix.
pub struct FileStore {
    path: PathBuf,
}

impl FileStore {
    pub fn new(path: PathBuf) -> FileStore {
        FileStore { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn dir(&self) -> PathBuf {
        match self.path.parent() {
            Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
            _ => PathBuf::from("."),
        }
    }

    fn read(&self) -> Result<BTreeMap<String, String>> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
            Err(e) => return Err(e).with_context(|| format!("reading keys file {}", self.path.display())),
        };
        // The parser's message is dropped so no file text leaks.
        toml::from_str(&text).map_err(|_| anyhow::anyhow!("keys file {} is not valid TOML", self.path.display()))
    }

    /// Creates the directory (0700 on unix) and holds an exclusive lock on `keys.toml.lock` until dropped.
    fn lock(&self) -> Result<std::fs::File> {
        let dir = self.dir();
        let mut b = std::fs::DirBuilder::new();
        b.recursive(true);
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut b, 0o700);
        b.create(&dir).with_context(|| format!("creating {}", dir.display()))?;
        let lock_path = dir.join(format!("{FILE_NAME}.lock"));
        let mut opts = std::fs::OpenOptions::new();
        opts.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
        let f = opts.open(&lock_path).with_context(|| format!("opening {}", lock_path.display()))?;
        fs4::FileExt::lock(&f).with_context(|| format!("locking {}", lock_path.display()))?;
        Ok(f)
    }

    /// Writes a 0600 temp file, syncs it, renames it over the file, then syncs the directory (unix).
    fn write(&self, table: &BTreeMap<String, String>) -> Result<()> {
        let text = toml::to_string(table).map_err(|_| anyhow::anyhow!("cannot encode keys file"))?;
        let dir = self.dir();
        let tmp = dir.join(format!("{FILE_NAME}.{}.tmp", ulid::Ulid::generate()));
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
        let res = (|| -> std::io::Result<()> {
            let mut f = opts.open(&tmp)?;
            f.write_all(text.as_bytes())?;
            f.sync_all()?;
            std::fs::rename(&tmp, &self.path)
        })();
        if res.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        res.with_context(|| format!("writing keys file {}", self.path.display()))?;
        #[cfg(unix)]
        std::fs::File::open(&dir)
            .and_then(|d| d.sync_all())
            .with_context(|| format!("syncing {}", dir.display()))?;
        Ok(())
    }
}

impl SecretStore for FileStore {
    fn backend(&self) -> Backend {
        Backend::File
    }
    fn get(&self, name: &str) -> Result<Option<Secret>> {
        Ok(self.read()?.remove(name).map(Secret::new))
    }
    fn set(&self, name: &str, value: &Secret) -> Result<Backend> {
        let _lock = self.lock()?;
        let mut t = self.read()?;
        t.insert(name.into(), value.expose().into());
        self.write(&t)?;
        Ok(Backend::File)
    }
    fn delete(&self, name: &str) -> Result<bool> {
        // No lock (and no directory) when there is nothing to remove.
        if !self.read()?.contains_key(name) {
            return Ok(false);
        }
        let _lock = self.lock()?;
        let mut t = self.read()?;
        if t.remove(name).is_none() {
            return Ok(false);
        }
        self.write(&t)?;
        Ok(true)
    }
}

/// The variant name of a `keyring` error, else `other`; never the error text, which can hold bytes.
pub fn error_kind(e: &anyhow::Error) -> &'static str {
    use keyring::Error as K;
    match e.downcast_ref::<K>() {
        Some(K::PlatformFailure(_)) => "PlatformFailure",
        Some(K::NoStorageAccess(_)) => "NoStorageAccess",
        Some(K::NoEntry) => "NoEntry",
        Some(K::BadEncoding(_)) => "BadEncoding",
        Some(K::BadDataFormat(..)) => "BadDataFormat",
        Some(K::BadStoreFormat(_)) => "BadStoreFormat",
        Some(K::TooLong(..)) => "TooLong",
        Some(K::Invalid(..)) => "Invalid",
        Some(K::Ambiguous(_)) => "Ambiguous",
        Some(K::NoDefaultStore) => "NoDefaultStore",
        Some(K::NotSupportedByStore(_)) => "NotSupportedByStore",
        _ => "other",
    }
}

/// The operating-system keychain through `keyring`.
pub struct OsStore;

impl OsStore {
    /// `Some` when the platform store initialised.
    pub fn probe() -> Option<OsStore> {
        keyring::Entry::store_status().as_ref().ok().map(|_| OsStore)
    }
}

impl SecretStore for OsStore {
    fn backend(&self) -> Backend {
        Backend::OsKeychain
    }
    fn get(&self, name: &str) -> Result<Option<Secret>> {
        match keyring::Entry::new(SERVICE, name)?.get_password() {
            Ok(v) => Ok(Some(Secret::new(v))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
    fn set(&self, name: &str, value: &Secret) -> Result<Backend> {
        keyring::Entry::new(SERVICE, name)?.set_password(value.expose())?;
        Ok(Backend::OsKeychain)
    }
    fn delete(&self, name: &str) -> Result<bool> {
        match keyring::Entry::new(SERVICE, name)?.delete_credential() {
            Ok(()) => Ok(true),
            Err(keyring::Error::NoEntry) => Ok(false),
            Err(e) => Err(e.into()),
        }
    }
}

/// OS store when it works, the file otherwise.
pub struct Auto {
    os: Option<Box<dyn SecretStore>>,
    file: FileStore,
}

impl Auto {
    pub fn new(os: Option<Box<dyn SecretStore>>, file: FileStore) -> Auto {
        let os = os.filter(|s| s.get(PROBE_NAME).is_ok());
        Auto { os, file }
    }
}

impl SecretStore for Auto {
    fn backend(&self) -> Backend {
        if self.os.is_some() { Backend::OsKeychain } else { Backend::File }
    }
    /// The file first: it holds an entry only when the last write fell back.
    fn get(&self, name: &str) -> Result<Option<Secret>> {
        if let Some(v) = self.file.get(name)? {
            return Ok(Some(v));
        }
        match &self.os {
            Some(o) => o.get(name),
            None => Ok(None),
        }
    }
    fn set(&self, name: &str, value: &Secret) -> Result<Backend> {
        self.set_reporting(name, value).map(|(b, _)| b)
    }
    /// OS store taken: a stale file entry is removed. OS store refused: the file takes the value.
    fn set_reporting(&self, name: &str, value: &Secret) -> Result<(Backend, Option<Fallback>)> {
        let Some(o) = &self.os else {
            return Ok((self.file.set(name, value)?, None));
        };
        match o.set(name, value) {
            Ok(b) => {
                self.file.delete(name)?;
                Ok((b, None))
            }
            Err(e) => {
                let b = self.file.set(name, value)?;
                Ok((b, Some(Fallback { reason: error_kind(&e).into(), file: self.file.path.clone() })))
            }
        }
    }
    /// Both stores always run; the first error is returned after both.
    fn delete(&self, name: &str) -> Result<bool> {
        let from_file = self.file.delete(name);
        let from_os = match &self.os {
            Some(o) => o.delete(name),
            None => Ok(false),
        };
        Ok(from_file? | from_os?)
    }
}

pub fn default_store(paths: &Paths) -> Auto {
    Auto::new(
        OsStore::probe().map(|o| Box::new(o) as Box<dyn SecretStore>),
        FileStore::new(paths.data_dir.join(FILE_NAME)),
    )
}

/// `Ok(None)` when the key is absent (an env value that is empty or only whitespace counts as absent);
/// `Err` when the store failed. The error holds no secret.
pub fn resolve_with(
    r: &KeyRef,
    s: &dyn SecretStore,
    env: &dyn Fn(&str) -> Option<OsString>,
) -> Result<Option<Secret>> {
    Ok(match r {
        KeyRef::Env(n) => env(n).and_then(|v| v.into_string().ok()).filter(|v| !v.trim().is_empty()).map(Secret::new),
        KeyRef::Keychain(n) => s.get(n)?,
    })
}

pub fn resolve(r: &KeyRef) -> Option<Secret> {
    resolve_with(r, &default_store(&Paths::from_env()), &|n| std::env::var_os(n)).ok().flatten()
}

pub fn store(name: &str, value: &Secret) -> Result<Backend> {
    default_store(&Paths::from_env()).set(name, value)
}

pub fn backend() -> Backend {
    default_store(&Paths::from_env()).backend()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_write_leaves_no_temp_file() {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path();
        let target = dir.join(FILE_NAME);
        std::fs::create_dir_all(target.join("occupied")).unwrap(); // rename onto a non-empty directory fails
        let mut t = BTreeMap::new();
        t.insert("a".to_string(), format!("v-{}", ulid::Ulid::generate()));
        assert!(FileStore::new(target.clone()).write(&t).is_err());
        let tmp: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(tmp.is_empty(), "{tmp:?}");
    }
}
