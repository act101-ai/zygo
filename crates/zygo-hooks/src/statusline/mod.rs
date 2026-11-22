//! `zygo hook statusline` (§17, H§6, H§7): the status line HUD for the user, never sent to the model.
//!
//! `data` collects what one render shows (bounded stdin, cache v2, lease, workers, deadline); `render` is the pure
//! render (H§6); `width` holds the width sources. `phase` reads the workspace phase of a member session (H§7.6). The
//! hook entry is in the `zygo` crate (`cmd/hook.rs`): it reads the real environment, resolves the style with `style`,
//! and calls `data::collect`, then `render::render`.

pub mod data;
pub mod phase;
pub mod render;
pub mod width;

use std::path::Path;
use std::time::Duration;

use zygo_core::overrides::Overrides;
use zygo_core::session_view::SessionView;
use zygo_usage::{Filter, Store};

use self::render::Style;

/// The whole run of the hook (H-C2, §17).
pub const BOUND: Duration = Duration::from_secs(3);

/// The style of this call (AU1, H§8): `statusline` of `overrides.json`, else `statusline_style` of `view.json`, else
/// `compact`. A value other than `compact` or `verbose` is ignored at its layer.
pub fn style(overrides: &Overrides, view: Option<&SessionView>) -> Style {
    overrides
        .statusline
        .as_deref()
        .and_then(Style::parse)
        .or_else(|| view.and_then(|v| Style::parse(&v.statusline_style)))
        .unwrap_or_default()
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
    use zygo_usage::{Store, TrimRow};

    use super::*;

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
