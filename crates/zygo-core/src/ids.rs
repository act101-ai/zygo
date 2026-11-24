/// A 26-character Crockford ULID.
pub fn ulid() -> String {
    ulid::Ulid::generate().to_string()
}

/// Member names no pane may take (workspace mail design M§6).
pub const RESERVED_MEMBER_NAMES: [&str; 2] = ["all", "cli"];

/// A member name (workspace mail design M§6): ^[a-z][a-z0-9-]{0,31}$ and not in
/// [`RESERVED_MEMBER_NAMES`]. Checked at each read (AU84).
pub fn valid_member_name(s: &str) -> bool {
    let b = s.as_bytes();
    matches!(b.first(), Some(b'a'..=b'z'))
        && b.len() <= 32
        && b[1..]
            .iter()
            .all(|c| matches!(c, b'a'..=b'z' | b'0'..=b'9' | b'-'))
        && !RESERVED_MEMBER_NAMES.contains(&s)
}
