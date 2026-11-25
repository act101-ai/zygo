/// A 26-character Crockford ULID.
pub fn ulid() -> String {
    ulid::Ulid::generate().to_string()
}

/// The creation time of ULID `s` in whole seconds (`timestamp_ms ÷ 1000`, floor; session run time design T§2.1);
/// `None` for a string that is not a ULID. The first character is `0`-`7`: a larger one is 130 bits.
pub fn ulid_secs(s: &str) -> Option<u64> {
    if !matches!(s.as_bytes().first(), Some(b'0'..=b'7')) {
        return None;
    }
    ulid::Ulid::from_string(s).ok().map(|u| u.timestamp_ms() / 1000)
}

/// Member names no pane may take (workspace mail design M§6).
pub const RESERVED_MEMBER_NAMES: [&str; 2] = ["all", "cli"];

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test] // T§2.1: the start is `timestamp_ms ÷ 1000`, floor
    fn ulid_secs_is_the_ulid_time_in_whole_seconds() {
        assert_eq!(ulid_secs(&ulid::Ulid::from_parts(1_800_000_000_000, 0).to_string()), Some(1_800_000_000));
        assert_eq!(ulid_secs(&ulid::Ulid::from_parts(1_800_000_000_999, 1).to_string()), Some(1_800_000_000));
        // The literal that the zygo-hooks and zygo tests use as `view.session`.
        assert_eq!(ulid_secs("01MCC5RM000000000000000000"), Some(1_800_000_000));
        assert_eq!(ulid_secs("00000000000000000000000000"), Some(0));
        assert_eq!(ulid_secs("7ZZZZZZZZZZZZZZZZZZZZZZZZZ"), Some(281_474_976_710));
    }

    #[test] // T§2.2 rule 1: a `session` that is not a ULID gives no start
    fn ulid_secs_of_a_string_that_is_not_a_ulid_is_none() {
        for bad in [
            "",
            "S1",
            "01MCC5RM00000000000000000",
            "01MCC5RM0000000000000000000",
            "01MCC5RM00000000000000000U",
            "80000000000000000000000000",
            "ZZZZZZZZZZZZZZZZZZZZZZZZZZ",
        ] {
            assert_eq!(ulid_secs(bad), None, "{bad:?}");
        }
    }
}
