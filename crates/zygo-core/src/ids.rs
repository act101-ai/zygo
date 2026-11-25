/// A 26-character Crockford ULID.
pub fn ulid() -> String {
    ulid::Ulid::generate().to_string()
}

/// Member names no pane may take (workspace mail design M§6; mail sync design S§7.2): `zygo` is the
/// sender of the mail sync notice for a refused message and has no inbox.
pub const RESERVED_MEMBER_NAMES: [&str; 3] = ["all", "cli", "zygo"];

/// A name no member may take (M§6): one of [`RESERVED_MEMBER_NAMES`], or of pane-key form `^k[a-z2-7]{8}$`
/// (workspaces design W§5), so `mail_send`'s `to` (a name or a pane key) is never ambiguous.
pub fn reserved_member_name(s: &str) -> bool {
    let b = s.as_bytes();
    let pane_key = b.len() == 9 && b[0] == b'k' && b[1..].iter().all(|c| matches!(c, b'a'..=b'z' | b'2'..=b'7'));
    pane_key || RESERVED_MEMBER_NAMES.contains(&s)
}

/// A member name (workspace mail design M§6): ^[a-z][a-z0-9-]{0,31}$ and not [`reserved_member_name`].
/// Checked at each read (AU84).
pub fn valid_member_name(s: &str) -> bool {
    let b = s.as_bytes();
    matches!(b.first(), Some(b'a'..=b'z'))
        && b.len() <= 32
        && b[1..]
            .iter()
            .all(|c| matches!(c, b'a'..=b'z' | b'0'..=b'9' | b'-'))
        && !reserved_member_name(s)
}
