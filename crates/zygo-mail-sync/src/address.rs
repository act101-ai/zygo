//! Addresses (mail sync design S§5, S§7.2) and the ULID file name form (S§8.5 "Checks").

use zygo_config::valid_mail_host;
use zygo_core::ids::{reserved_member_name, valid_member_name, RESERVED_MEMBER_NAMES};

/// Names no address may take (S§7.2; S§16 M§6): the core list of reserved member names.
pub use zygo_core::ids::RESERVED_MEMBER_NAMES as RESERVED;

/// A member on a host of the side branch, `<name>@<host>` (S§5 Address).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Address {
    pub name: String,
    pub host: String,
}

/// Where a `to` value sends a message (S§7.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum To {
    /// A member name or a pane key; callers tell them apart with `zygo_core::ids::reserved_member_name`
    /// (the pane-key form `^k[a-z2-7]{8}$`).
    Local(String),
    Remote(Address),
    All,
}

/// Why a `to` value is refused (S§7.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddrError {
    Reserved,
    BadForm,
}

impl AddrError {
    /// The user line of the refusal (S§7.2).
    pub fn line(self) -> &'static str {
        match self {
            AddrError::Reserved => "zygo: all, cli and zygo have no address; send to <name> or <name>@<host>",
            AddrError::BadForm => "zygo: not an address; send to a member name, a pane key, all, or <name>@<host>",
        }
    }
}

/// `<name>@<host>` with a member name not in [`RESERVED`] and a mail host form (S§5 Address).
pub fn parse(s: &str) -> Option<Address> {
    let (name, host) = s.split_once('@')?;
    (valid_member_name(name) && !RESERVED.contains(&name) && valid_mail_host(host)).then(|| Address {
        name: name.into(),
        host: host.into(),
    })
}

/// Resolves a `to` value against the hosts of this workspace (S§5 Local address, S§7.2; M§8).
pub fn resolve_to(s: &str, own_hosts: &[String]) -> Result<To, AddrError> {
    if s == "all" {
        return Ok(To::All);
    }
    let Some((name, host)) = s.split_once('@') else {
        if RESERVED.contains(&s) {
            return Err(AddrError::Reserved);
        }
        let pane_key = reserved_member_name(s) && !RESERVED_MEMBER_NAMES.contains(&s);
        return if valid_member_name(s) || pane_key {
            Ok(To::Local(s.into()))
        } else {
            Err(AddrError::BadForm)
        };
    };
    if !valid_mail_host(host) {
        return Err(AddrError::BadForm);
    }
    if RESERVED.contains(&name) {
        return Err(AddrError::Reserved);
    }
    if !valid_member_name(name) {
        return Err(AddrError::BadForm);
    }
    if own_hosts.iter().any(|h| h == host) {
        Ok(To::Local(name.into()))
    } else {
        Ok(To::Remote(Address {
            name: name.into(),
            host: host.into(),
        }))
    }
}

/// The ULID of a message file named `^[0-9A-HJKMNP-TV-Z]{26}\.json$` that decodes without loss (S§8.5 "Checks").
pub fn ulid_file(name: &str) -> Option<ulid::Ulid> {
    let stem = name.strip_suffix(".json")?;
    let crockford = stem.len() == 26
        && stem.bytes().all(|c| {
            matches!(c, b'0'..=b'9' | b'A'..=b'H' | b'J' | b'K' | b'M' | b'N' | b'P'..=b'T' | b'V'..=b'Z')
        });
    if !crockford {
        return None;
    }
    let u = ulid::Ulid::from_string(stem).ok()?;
    (u.to_string() == stem).then_some(u)
}
