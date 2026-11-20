//! `zygo hook statusline` (§17, §10.2.4, §10.7): one line for the user, never sent to the model.

use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use zygo_core::hook::{HookInput, HookOutput};
use zygo_core::paths::Paths;
use zygo_core::session_dir::SessionDir;
use zygo_proxy::client::{SessionClient, SessionStatus, WindowArg};
use zygo_usage::{Filter, Store};

/// The whole run (§17).
const BOUND: Duration = Duration::from_secs(3);
/// A cached line younger than this is printed without a fetch (§17).
const CACHE_SECS: u64 = 30;

#[derive(Serialize, Deserialize)]
struct Cached {
    at: u64,
    line: String,
}

/// Reads `ZYGO_SESSION_DIR` and the usage database path from the environment; loads no config. The status call and the
/// usage read run on a worker; the hook waits for it only until the deadline, and a worker still running then ends
/// with the process (`main` ends with `process::exit`), so a slow drop of the client cannot hold the hook.
pub fn run(_input: &HookInput) -> anyhow::Result<HookOutput> {
    let Some(dir) = SessionDir::from_env() else { return Ok(HookOutput::None) };
    let deadline = Instant::now() + BOUND;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let usage_db = Paths::from_env().usage_db;
    let fetch = |left: Duration| {
        let (root, db) = (dir.root.clone(), usage_db.clone());
        let (tx, rx) = mpsc::channel();
        // Not joined.
        std::thread::spawn(move || {
            #[cfg(feature = "test-hooks")]
            test_block();
            let client = SessionClient::from_session_dir(&root).map(|c| c.timeout(left));
            let line = client.as_ref().map_err(|e| anyhow!("{e}")).and_then(|c| fetch_line(c, &db));
            let _ = tx.send(line);
            // `client` and its runtime drop after the send.
        });
        rx.recv_timeout(left).unwrap_or_else(|_| Err(anyhow!("no reply within the deadline")))
    };
    Ok(HookOutput::Text(line_in(&dir, now, deadline, &fetch)?))
}

/// The session status and the session's trimmed bytes, rendered.
fn fetch_line(client: &SessionClient, usage_db: &Path) -> anyhow::Result<String> {
    let s = client.status()?;
    Ok(render(&s, trimmed(usage_db, &s.session)))
}

/// Test seam: with `ZYGO_TEST_STATUSLINE_BLOCK` set, the fetch waits 10 s on a condvar that nothing notifies. The code
/// exists only under the `test-hooks` feature.
#[cfg(feature = "test-hooks")]
fn test_block() {
    use std::sync::{Condvar, Mutex};
    static GATE: (Mutex<()>, Condvar) = (Mutex::new(()), Condvar::new());
    if std::env::var_os("ZYGO_TEST_STATUSLINE_BLOCK").is_none() {
        return;
    }
    let g = GATE.0.lock().unwrap_or_else(|e| e.into_inner());
    let _ = GATE.1.wait_timeout_while(g, Duration::from_secs(10), |_| true);
}

/// A fresh cache (`at <= now < at + 30`) answers at once. Else `fetch` gets the time left before `deadline` and gives
/// the line; a fetch that fails or ends after `deadline` falls back to the cached line of any age, else is an error.
pub fn line_in(
    dir: &SessionDir,
    now: u64,
    deadline: Instant,
    fetch: &dyn Fn(Duration) -> anyhow::Result<String>,
) -> anyhow::Result<String> {
    let path = dir.root.join("statusline.json");
    let cached: Option<Cached> = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok());
    if let Some(c) = &cached
        && c.at <= now
        && now - c.at < CACHE_SECS
    {
        return Ok(c.line.clone());
    }
    let left = deadline.saturating_duration_since(Instant::now());
    let got = if left.is_zero() {
        Err(anyhow!("deadline passed"))
    } else {
        match fetch(left) {
            Ok(_) if Instant::now() > deadline => Err(anyhow!("deadline passed")),
            r => r,
        }
    };
    match got {
        Ok(line) => {
            write_cache(&dir.root, &path, now, &line);
            Ok(line)
        }
        Err(why) => cached.map(|c| c.line).ok_or_else(|| anyhow!("statusline: {}", zygo_core::esc(&why.to_string()))),
    }
}

/// A unique temp file, then rename; a failed write leaves no cache and is ignored.
fn write_cache(root: &Path, path: &Path, now: u64, line: &str) {
    let tmp = root.join(format!("statusline.json.{}.tmp", std::process::id()));
    let Ok(bytes) = serde_json::to_vec(&Cached { at: now, line: line.to_string() }) else { return };
    if std::fs::write(&tmp, bytes).and_then(|()| std::fs::rename(&tmp, path)).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

/// `zygo ◇ <work> ◇ <profile> ◇ ctx <used>/<effective>[ ◇ compact <n|off>] ◇ cache <pct> ◇ trimmed <bytes>`; an
/// absent value is `-`.
pub fn render(s: &SessionStatus, trimmed: Option<u64>) -> String {
    let text = |v: &Option<String>| v.as_deref().map_or_else(|| "-".into(), zygo_core::esc);
    let tokens = |v: Option<u64>| v.map_or_else(|| "-".into(), short);
    let mut out = format!(
        "zygo ◇ {} ◇ {} ◇ ctx {}/{}",
        text(&s.work),
        text(&s.profile),
        tokens(s.used_context),
        tokens(s.effective_window)
    );
    match s.compact {
        Some(WindowArg::Tokens(n)) => out.push_str(&format!(" ◇ compact {}", short(n))),
        Some(WindowArg::Off) => out.push_str(" ◇ compact off"),
        None => {}
    }
    let trimmed = trimmed.map_or_else(|| "-".into(), |n| n.to_string());
    out.push_str(&format!(" ◇ cache {} ◇ trimmed {trimmed}", pct(s.cache_read_share)));
    out
}

/// `999`, `412k` (floor), `1m`, `1.2m` (tenths, floor).
pub fn short(n: u64) -> String {
    if n < 1000 {
        n.to_string()
    } else if n.is_multiple_of(1_000_000) {
        format!("{}m", n / 1_000_000)
    } else if n >= 1_000_000 {
        format!("{}.{}m", n / 1_000_000, n % 1_000_000 / 100_000)
    } else {
        format!("{}k", n / 1000)
    }
}

pub fn pct(share: Option<f64>) -> String {
    share.map_or_else(|| "-".into(), |s| format!("{}%", (s * 100.0).round()))
}

/// The session's trimmed bytes from the usage store opened read-only; `None` when it is missing, busy or unreadable.
pub fn trimmed(usage_db: &Path, session: &str) -> Option<u64> {
    let store = Store::open_read_only(usage_db).ok()?;
    store.summary(&Filter { session: Some(session.to_string()), since: None }).ok().map(|s| s.bytes_trimmed)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use zygo_proxy::client::WindowArg;
    use zygo_usage::{Store, TrimRow};

    use super::*;

    fn s() -> SessionStatus {
        SessionStatus {
            session: "S1".into(),
            profile: Some("p1".into()),
            work: Some("ENG-42".into()),
            used_context: Some(412_345),
            effective_window: Some(600_000),
            compact: Some(WindowArg::Tokens(600_000)),
            cache_read_share: Some(0.6),
            expires_at: String::new(),
            usage_note: None,
        }
    }

    fn far() -> Instant {
        Instant::now() + Duration::from_secs(60)
    }

    fn line() -> String {
        render(&s(), None)
    }

    fn failing(_: Duration) -> anyhow::Result<String> {
        Err(anyhow!("closed"))
    }

    fn dir() -> (tempfile::TempDir, SessionDir) {
        let t = tempfile::tempdir().unwrap();
        let sd = SessionDir::new(t.path().to_path_buf());
        (t, sd)
    }

    fn files(sd: &SessionDir) -> Vec<String> {
        let mut v: Vec<String> =
            std::fs::read_dir(&sd.root).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        v.sort();
        v
    }

    #[test]
    fn render_with_override() {
        assert_eq!(render(&s(), Some(92_160)), "zygo ◇ ENG-42 ◇ p1 ◇ ctx 412k/600k ◇ compact 600k ◇ cache 60% ◇ trimmed 92160");
    }

    #[test]
    fn render_without_override_and_off() {
        let none = SessionStatus { compact: None, effective_window: Some(800_000), ..s() };
        assert_eq!(render(&none, Some(92_160)), "zygo ◇ ENG-42 ◇ p1 ◇ ctx 412k/800k ◇ cache 60% ◇ trimmed 92160");
        let off = SessionStatus { compact: Some(WindowArg::Off), effective_window: None, ..s() };
        assert_eq!(render(&off, Some(92_160)), "zygo ◇ ENG-42 ◇ p1 ◇ ctx 412k/- ◇ compact off ◇ cache 60% ◇ trimmed 92160");
    }

    #[test]
    fn render_absent_values() {
        let empty = SessionStatus {
            profile: None,
            work: None,
            used_context: None,
            effective_window: None,
            compact: None,
            cache_read_share: None,
            ..s()
        };
        assert_eq!(render(&empty, None), "zygo ◇ - ◇ - ◇ ctx -/- ◇ cache - ◇ trimmed -");
    }

    #[test]
    fn render_escapes_server_text() { // N70 N2
        let forged = SessionStatus { work: Some("ENG-42\nzygo ◇ x".into()), profile: Some("p1\u{1b}[31m".into()), ..s() };
        assert_eq!(
            render(&forged, None),
            "zygo ◇ ENG-42\\nzygo ◇ x ◇ p1\\u{1b}[31m ◇ ctx 412k/600k ◇ compact 600k ◇ cache 60% ◇ trimmed -"
        );
    } // mutations: `work` or `profile` printed raw

    #[test]
    fn a_fetch_error_is_escaped() { // N70: statusline.rs:100
        let (_t, sd) = dir();
        let fetch = |_: Duration| -> anyhow::Result<String> { Err(anyhow!("bad: one\ntwo\u{1b}[31m")) };
        let e = line_in(&sd, 1_000, far(), &fetch).unwrap_err();
        assert_eq!(e.to_string(), "statusline: bad: one\\ntwo\\u{1b}[31m");
    } // mutation: `esc` removed at :100 (raw ESC; the hook wrapper folds only `\n` and `\r`)

    #[test]
    fn short_forms() {
        for (n, want) in [
            (0, "0"),
            (999, "999"),
            (1000, "1k"),
            (999_999, "999k"),
            (412_345, "412k"),
            (600_000, "600k"),
            (1_000_000, "1m"),
            (1_250_000, "1.2m"),
            (1_500_000, "1.5m"),
            (10_000_000, "10m"),
        ] {
            assert_eq!(short(n), want, "{n}");
        }
    }

    #[test]
    fn cache_holds_30_s() {
        let (_t, sd) = dir();
        let calls = Cell::new(0);
        let ok = |_: Duration| {
            calls.set(calls.get() + 1);
            Ok(line())
        };
        let first = line_in(&sd, 1000, far(), &ok).unwrap();
        assert_eq!(calls.get(), 1);
        assert_eq!(files(&sd), ["statusline.json"]);
        assert_eq!(line_in(&sd, 1029, far(), &failing).unwrap(), first);
        assert_eq!(line_in(&sd, 1030, far(), &ok).unwrap(), first);
        assert_eq!(calls.get(), 2);
        assert_eq!(files(&sd), ["statusline.json"]);
    }

    #[test]
    fn stale_or_bad_cache() {
        let (_t, sd) = dir();
        let cached = line_in(&sd, 1000, far(), &|_| Ok(line())).unwrap();
        assert_eq!(line_in(&sd, 1100, far(), &failing).unwrap(), cached);

        let calls = Cell::new(0);
        let counted = |_: Duration| {
            calls.set(calls.get() + 1);
            Ok(line())
        };
        std::fs::write(sd.root.join("statusline.json"), r#"{"at":2000,"line":"future"}"#).unwrap();
        line_in(&sd, 1000, far(), &counted).unwrap();
        assert_eq!(calls.get(), 1);

        std::fs::write(sd.root.join("statusline.json"), r#"{"at":"#).unwrap();
        line_in(&sd, 1000, far(), &counted).unwrap();
        assert_eq!(calls.get(), 2);
        std::fs::write(sd.root.join("statusline.json"), r#"{"at":"#).unwrap();
        assert!(line_in(&sd, 1000, far(), &failing).is_err());

        std::fs::remove_file(sd.root.join("statusline.json")).unwrap();
        assert!(line_in(&sd, 1000, far(), &failing).is_err());
    }

    #[test]
    fn deadline_passed() {
        let (_t, sd) = dir();
        let calls = Cell::new(0);
        let counted = |_: Duration| {
            calls.set(calls.get() + 1);
            Ok(line())
        };
        assert!(line_in(&sd, 1000, Instant::now(), &counted).is_err());
        let cached = line_in(&sd, 1000, far(), &|_| Ok(line())).unwrap();
        assert_eq!(line_in(&sd, 1100, Instant::now(), &counted).unwrap(), cached);
        assert_eq!(calls.get(), 0);
    }

    #[test]
    fn fetch_ending_after_the_deadline_fails() {
        let (_t, sd) = dir();
        let calls = Cell::new(0);
        let late = |deadline: Instant, text: &'static str| {
            move |_: Duration| {
                while Instant::now() <= deadline {
                    std::hint::spin_loop();
                }
                anyhow::Ok(text.to_string())
            }
        };
        let deadline = Instant::now() + Duration::from_millis(50);
        let f = late(deadline, "new");
        let counted = |left: Duration| {
            calls.set(calls.get() + 1);
            f(left)
        };
        assert!(line_in(&sd, 1000, deadline, &counted).is_err());
        assert_eq!(calls.get(), 1);
        assert!(!sd.root.join("statusline.json").exists());

        assert_eq!(line_in(&sd, 1000, far(), &|_| Ok("old".into())).unwrap(), "old");
        let deadline = Instant::now() + Duration::from_millis(50);
        let f = late(deadline, "new");
        let counted = |left: Duration| {
            calls.set(calls.get() + 1);
            f(left)
        };
        assert_eq!(line_in(&sd, 1100, deadline, &counted).unwrap(), "old");
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn trimmed_from_the_usage_store() {
        let t = tempfile::tempdir().unwrap();
        let db = t.path().join("usage.db");
        {
            let st = Store::open(&db).unwrap();
            st.insert_trim(&TrimRow { session: "S1".into(), stored_bytes: 100_000, view_bytes: 7_840, ..Default::default() })
                .unwrap();
        }
        assert_eq!(trimmed(&db, "S1"), Some(92_160));
        assert_eq!(trimmed(&db, "other"), Some(0));
        let missing = t.path().join("sub/usage.db");
        assert_eq!(trimmed(&missing, "S1"), None);
        assert!(!t.path().join("sub").exists());
        let bad = t.path().join("bad.db");
        std::fs::write(&bad, "not a database").unwrap();
        assert_eq!(trimmed(&bad, "S1"), None);
    }
}
