//! Workspace id, name slug and collision suffix (AU2), main worktree (AU77), validation (AU84). Owner: node W1 (W§5).

use std::path::Path;

use sha2::{Digest, Sha256};

/// Longest workspace name (AU2).
const NAME_MAX: usize = 32;

/// The workspace id (W§5): the first 16 hex characters of the SHA-256 of the bytes of `common_dir`. The path is used
/// as given; the caller gives the canonical `Checkout.common_dir`.
pub fn workspace_id(common_dir: &Path) -> String {
    let mut id = hex::encode(Sha256::digest(common_dir.as_os_str().as_encoded_bytes()));
    id.truncate(16);
    id
}

/// The name slug (AU2): ASCII letters in lower case, each other character outside `[a-z0-9-]` as `-`, runs of `-` as
/// one, no `-` at either end, at most 32 characters with no `-` at the end; `ws` when nothing is left.
pub fn slug(name: &str) -> String {
    let mut s = String::with_capacity(name.len());
    for c in name.chars() {
        let c = c.to_ascii_lowercase();
        let c = if c.is_ascii_lowercase() || c.is_ascii_digit() { c } else { '-' };
        if c != '-' || !s.ends_with('-') {
            s.push(c);
        }
    }
    let s: String = s.trim_matches('-').chars().take(NAME_MAX).collect();
    let s = s.trim_end_matches('-');
    if s.is_empty() { "ws".to_string() } else { s.to_string() }
}

/// `slug` when it is free; else the first free `<part>-<n>` for `n` = 2, 3, …, where `part` is the start of `slug`
/// cut so that the name has at most 32 characters, with no `-` at its end (AU2).
pub fn unique_name(slug: &str, taken: &dyn Fn(&str) -> bool) -> String {
    if !taken(slug) {
        return slug.to_string();
    }
    let mut n: u64 = 2;
    loop {
        let suffix = format!("-{n}");
        let part: String = slug.chars().take(NAME_MAX.saturating_sub(suffix.len())).collect();
        let candidate = format!("{}{suffix}", part.trim_end_matches('-'));
        if !taken(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// `^k[a-z2-7]{8}$` on the whole string (AU84).
pub fn valid_pane_key(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 9 && b[0] == b'k' && b[1..].iter().all(|c| matches!(c, b'a'..=b'z' | b'2'..=b'7'))
}

/// `^[0-9a-f]{16}$` on the whole string (AU84).
pub fn valid_ws_id(s: &str) -> bool {
    s.len() == 16 && s.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'))
}

/// `^[a-z0-9-]{1,32}$` on the whole string (AU84).
pub fn valid_name(s: &str) -> bool {
    (1..=NAME_MAX).contains(&s.len()) && s.bytes().all(|c| matches!(c, b'a'..=b'z' | b'0'..=b'9' | b'-'))
}

/// `^zygo-[a-z0-9-]{1,32}$` on the whole string, and equal to `zygo-<name>` (AU84).
pub fn valid_tmux_session(s: &str, name: &str) -> bool {
    s.strip_prefix("zygo-").is_some_and(|rest| rest == name && valid_name(rest))
}

const BASE32: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";

/// A new pane key (W§5): `k` and 8 random characters of lower-case base32.
pub fn new_pane_key() -> String {
    let mut k = String::with_capacity(9);
    k.push('k');
    for _ in 0..8 {
        k.push(BASE32[rand::random_range(0..BASE32.len())] as char);
    }
    k
}
