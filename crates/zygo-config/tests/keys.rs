use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
use std::sync::Arc;
use zygo_config::keys::*;
use zygo_config::KeyRef;

fn val(tag: &str) -> Secret { Secret::new(format!("{tag}-{}", ulid::Ulid::generate())) }
fn no_env(_: &str) -> Option<OsString> { None }

/// A unique directory removed on drop.
struct Dir(PathBuf);
impl Dir {
    fn new() -> Dir {
        let p = std::env::temp_dir().join(format!("zygo-n5-{}", ulid::Ulid::generate()));
        std::fs::create_dir_all(&p).unwrap();
        Dir(p)
    }
    fn file(&self) -> PathBuf { self.0.join("keys.toml") }
}
impl Drop for Dir { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }

/// An OS store backed by memory whose failures are switched on after construction.
#[derive(Default)]
struct Flags { get: AtomicBool, set: AtomicBool, del: AtomicBool, mem: MemStore }
struct Fake(Arc<Flags>);
fn fake() -> (Box<dyn SecretStore>, Arc<Flags>) {
    let f = Arc::new(Flags::default());
    (Box::new(Fake(f.clone())), f)
}
impl SecretStore for Fake {
    fn backend(&self) -> Backend { Backend::OsKeychain }
    fn get(&self, n: &str) -> anyhow::Result<Option<Secret>> {
        if self.0.get.load(SeqCst) { anyhow::bail!("os get failed") }
        self.0.mem.get(n)
    }
    fn set(&self, n: &str, v: &Secret) -> anyhow::Result<Backend> {
        if self.0.set.load(SeqCst) { anyhow::bail!("os set failed") }
        self.0.mem.set(n, v)
    }
    fn delete(&self, n: &str) -> anyhow::Result<bool> {
        if self.0.del.load(SeqCst) { anyhow::bail!("os delete failed") }
        self.0.mem.delete(n)
    }
}

#[test]
fn env_ref_resolves_only_non_empty() {
    let v = val("e");
    let look = |n: &str| match n {
        "A" => Some(OsString::from(v.expose())),
        "B" => Some(OsString::new()),
        "D" => Some(OsString::from(" \t\r\n")),
        _ => None,
    };
    let s = MemStore::new();
    assert_eq!(resolve_with(&KeyRef::Env("A".into()), &s, &look).unwrap(), Some(v.clone()));
    assert_eq!(resolve_with(&KeyRef::Env("B".into()), &s, &look).unwrap(), None);
    assert_eq!(resolve_with(&KeyRef::Env("C".into()), &s, &look).unwrap(), None);
    assert_eq!(resolve_with(&KeyRef::Env("D".into()), &s, &look).unwrap(), None, "whitespace only is unset");
}

#[test]
fn keychain_ref_resolves_via_store_and_missing_is_none() {
    let s = MemStore::new();
    let v = val("k");
    s.set("anthropic", &v).unwrap();
    assert_eq!(resolve_with(&KeyRef::Keychain("anthropic".into()), &s, &no_env).unwrap(), Some(v));
    assert_eq!(resolve_with(&KeyRef::Keychain("other".into()), &s, &no_env).unwrap(), None);
}

#[test]
fn os_get_error_falls_through_to_the_file_key() {
    let d = Dir::new();
    let file = FileStore::new(d.file());
    let v = val("f");
    file.set("n", &v).unwrap();
    let (os, flags) = fake();
    let a = Auto::new(Some(os), file);
    flags.get.store(true, SeqCst); // fails after the probe passed
    assert_eq!(resolve_with(&KeyRef::Keychain("n".into()), &a, &no_env).unwrap(), Some(v));
}

#[test]
fn secret_debug_is_redacted() {
    let v = val("d");
    assert!(!format!("{v:?}").contains(v.expose()));
}

#[test]
fn file_store_round_trip_overwrite_delete() {
    let d = Dir::new();
    let f = FileStore::new(d.0.join("sub").join("keys.toml"));
    assert_eq!(f.get("a").unwrap(), None);
    let (v1, v2) = (val("one"), val("two"));
    assert_eq!(f.set("a", &v1).unwrap(), Backend::File);
    f.set("b", &v2).unwrap();
    f.set("a", &v2).unwrap();
    assert_eq!(f.get("a").unwrap(), Some(v2.clone()));
    assert_eq!(f.get("b").unwrap(), Some(v2));
    assert!(f.delete("a").unwrap());
    assert!(!f.delete("a").unwrap());
    assert_eq!(f.get("a").unwrap(), None);
}

#[test]
fn file_backend_display_names_the_mode_on_unix_only() {
    #[cfg(unix)]
    assert_eq!(Backend::File.to_string(), "file (0600)");
    #[cfg(windows)]
    assert_eq!(Backend::File.to_string(), "file");
}

#[cfg(unix)]
#[test]
fn file_store_makes_an_existing_0644_file_0600() {
    use std::os::unix::fs::PermissionsExt;
    let d = Dir::new();
    std::fs::write(d.file(), "").unwrap();
    std::fs::set_permissions(d.file(), std::fs::Permissions::from_mode(0o644)).unwrap();
    FileStore::new(d.file()).set("a", &val("x")).unwrap();
    assert_eq!(std::fs::metadata(d.file()).unwrap().permissions().mode() & 0o777, 0o600);
}

#[cfg(unix)]
#[test]
fn file_store_creates_its_directory_0700() {
    use std::os::unix::fs::PermissionsExt;
    let d = Dir::new();
    let dir = d.0.join("data").join("zygo");
    FileStore::new(dir.join("keys.toml")).set("a", &val("x")).unwrap();
    for p in [&dir, &d.0.join("data")] {
        assert_eq!(std::fs::metadata(p).unwrap().permissions().mode() & 0o777, 0o700, "{}", p.display());
    }
}

#[test]
fn concurrent_file_writes_lose_nothing() {
    let d = Dir::new();
    let threads: Vec<_> = ["p", "q"]
        .into_iter()
        .map(|n| {
            let path = d.file();
            std::thread::spawn(move || {
                let f = FileStore::new(path);
                for _ in 0..50 {
                    f.set(n, &val(n)).unwrap();
                }
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    let f = FileStore::new(d.file());
    assert!(f.get("p").unwrap().is_some() && f.get("q").unwrap().is_some());
}

#[test]
fn corrupt_file_error_does_not_contain_file_text() {
    let d = Dir::new();
    let secret = val("leak");
    std::fs::write(d.file(), format!("a = {}\n", secret.expose())).unwrap(); // bare word: invalid TOML
    let f = FileStore::new(d.file());
    let e = format!("{:#}", f.get("a").unwrap_err());
    assert!(e.contains("is not valid TOML") && e.contains("keys.toml"), "{e}");
    assert!(!e.contains(secret.expose()), "{e}");
    let e = format!("{:#}", f.set("a", &val("n")).unwrap_err());
    assert!(!e.contains(secret.expose()), "{e}");
}

#[test]
fn auto_falls_back_to_file_when_os_set_fails() {
    let d = Dir::new();
    let (os, flags) = fake();
    let a = Auto::new(Some(os), FileStore::new(d.file()));
    flags.set.store(true, SeqCst);
    let v = val("s");
    assert_eq!(a.set("n", &v).unwrap(), Backend::File);
    assert!(d.file().exists());
    assert_eq!(a.get("n").unwrap(), Some(v));
}

#[test]
fn auto_backend_is_os_only_when_the_probe_passes() {
    let d = Dir::new();
    let (ok, _) = fake();
    assert_eq!(Auto::new(Some(ok), FileStore::new(d.file())).backend(), Backend::OsKeychain);
    let (bad, flags) = fake();
    flags.get.store(true, SeqCst); // inits, but get errors
    let a = Auto::new(Some(bad), FileStore::new(d.file()));
    assert_eq!(a.backend(), Backend::File);
    assert_eq!(a.set("n", &val("p")).unwrap(), Backend::File);
    assert_eq!(Auto::new(None, FileStore::new(d.file())).backend(), Backend::File);
}

#[test]
fn auto_delete_returns_an_os_error_and_still_clears_the_file() {
    let d = Dir::new();
    let file = FileStore::new(d.file());
    file.set("n", &val("g")).unwrap();
    let (os, flags) = fake();
    let a = Auto::new(Some(os), file);
    flags.del.store(true, SeqCst);
    let e = a.delete("n").unwrap_err().to_string();
    assert!(e.contains("os delete failed"), "{e}");
    assert_eq!(FileStore::new(d.file()).get("n").unwrap(), None);
}

#[test]
fn auto_delete_clears_the_os_entry_when_the_file_is_corrupt() {
    let d = Dir::new();
    std::fs::write(d.file(), "not [ toml").unwrap();
    let (os, flags) = fake();
    let a = Auto::new(Some(os), FileStore::new(d.file()));
    flags.mem.set("n", &val("o")).unwrap();
    let e = a.delete("n").unwrap_err().to_string();
    assert!(e.contains("is not valid TOML"), "{e}");
    assert_eq!(flags.mem.get("n").unwrap(), None);
}

#[test]
fn auto_reads_the_file_first_and_an_os_set_clears_the_file_entry() {
    let d = Dir::new();
    let (os, flags) = fake();
    let a = Auto::new(Some(os), FileStore::new(d.file()));
    let (va, vb, vc) = (val("a"), val("b"), val("c"));
    assert_eq!(a.set("n", &va).unwrap(), Backend::OsKeychain);
    flags.set.store(true, SeqCst);
    assert_eq!(a.set("n", &vb).unwrap(), Backend::File);
    assert_eq!(a.get("n").unwrap(), Some(vb));
    flags.set.store(false, SeqCst);
    assert_eq!(a.set("n", &vc).unwrap(), Backend::OsKeychain);
    assert_eq!(a.get("n").unwrap(), Some(vc));
    assert_eq!(FileStore::new(d.file()).get("n").unwrap(), None);
}

#[test]
fn auto_set_reports_the_fallback_kind_and_file_without_the_value() {
    let d = Dir::new();
    let (os, flags) = fake();
    let a = Auto::new(Some(os), FileStore::new(d.file()));
    let v = val("w");
    assert_eq!(a.set_reporting("n", &v).unwrap(), (Backend::OsKeychain, None));
    flags.set.store(true, SeqCst);
    let (b, fb) = a.set_reporting("n", &v).unwrap();
    let fb = fb.expect("fell back");
    assert_eq!((b, fb.file.as_path()), (Backend::File, d.file().as_path()));
    assert!(!fb.reason.is_empty() && !fb.reason.contains(v.expose()), "{}", fb.reason);
}

#[test]
fn resolve_with_tells_a_store_error_from_absence() {
    let d = Dir::new();
    let secret = val("leak");
    std::fs::write(d.file(), format!("a = {}\n", secret.expose())).unwrap();
    let a = Auto::new(None, FileStore::new(d.file()));
    let e = format!("{:#}", resolve_with(&KeyRef::Keychain("a".into()), &a, &no_env).unwrap_err());
    assert!(!e.contains(secret.expose()), "{e}");
    let ok = Auto::new(None, FileStore::new(d.0.join("none.toml")));
    assert_eq!(resolve_with(&KeyRef::Keychain("a".into()), &ok, &no_env).unwrap(), None);
}
