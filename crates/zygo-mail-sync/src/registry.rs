//! The registry and acknowledgment files of the side branch and the member cache (mail sync design S§6.1,
//! S§7.3, S§7.4, S-C5, S-C8). Each file names its owner host and identity, so one writer owns each path (S§6.1).
//! Every file is checked at each read (S-C5): a refused file is named by its first failed [`Check`].
//! A refused ack counts as empty: the caller uses `parse_ack(..).map(|a| a.imported).unwrap_or_default()` and
//! the prune removes nothing for that host.
use std::collections::{BTreeMap, BTreeSet};

use serde::de::IgnoredAny;
use serde::{Deserialize, Serialize};
use zygo_config::valid_mail_host;
use zygo_core::ids::valid_member_name;

use crate::address::Address;
use crate::state::{SyncPaths, write_atomic};

/// The first check a registry or ack file failed: `mode`, `size`, `json`, `version`, `host`, `identity`,
/// `members`, `member` (registry); `entries`, `entry` (ack). Each is a valid `state.json` `last_error`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Check(pub &'static str);

/// `registry/<host>.json` (S§7.3): the members of one host, written only by that host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registry {
    pub host: String,
    pub identity: String,
    pub members: Vec<String>,
}

/// `ack/<host>.json` (S§7.4): the ULIDs that host imported, written only by that host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ack {
    pub host: String,
    pub identity: String,
    pub imported: BTreeSet<ulid::Ulid>,
}

/// The largest registry file, in bytes (S-C5).
pub const REGISTRY_MAX: u64 = 65536;
/// The largest ack file, in bytes (S-C5).
pub const ACK_MAX: u64 = 8388608;
/// The most entries in one ack file (S§7.4); a writer checks it before `to_bytes`.
pub const ACK_ENTRIES_MAX: usize = 200000;
/// The most members in one registry (S§7.3, S§19 #12); a writer checks it before `to_bytes`.
pub const MEMBERS_MAX: usize = 256;

/// The registry file form; field order is the key order of [`Registry::to_bytes`].
#[derive(Serialize, Deserialize)]
struct RegistryFile {
    version: u32,
    host: String,
    identity: String,
    members: Vec<String>,
}

/// The ack file form; field order is the key order of [`Ack::to_bytes`].
#[derive(Serialize, Deserialize)]
struct AckFile {
    version: u32,
    host: String,
    identity: String,
    imported: Vec<String>,
}

/// The top level of a file, read for `version` only. Serde skips every other key with `IgnoredAny`, so no value
/// tree is built (S-C5).
#[derive(Deserialize)]
struct Head {
    version: Option<u64>,
}

/// The checks both files share: git mode `100644`, `size` and the bytes at most `max`, JSON, then an object whose
/// `version` is the integer 1. The syntax pass and the `version` read skip values without building them (S-C5).
fn head(mode: &str, size: u64, bytes: &[u8], max: u64) -> Result<(), Check> {
    if mode != "100644" {
        return Err(Check("mode"));
    }
    if size > max || bytes.len() as u64 > max {
        return Err(Check("size"));
    }
    serde_json::from_slice::<IgnoredAny>(bytes).map_err(|_| Check("json"))?;
    // The JSON is valid, so its first byte that is not whitespace tells an object; serde would also read the
    // array form of a struct.
    let object = bytes.iter().find(|c| !c.is_ascii_whitespace()) == Some(&b'{');
    if !object
        || !matches!(
            serde_json::from_slice::<Head>(bytes),
            Ok(Head { version: Some(1) })
        )
    {
        return Err(Check("version"));
    }
    Ok(())
}

/// The owner checks (S§6.1): `host` is the file name `fh` and in the host form; `identity` is `^[0-9a-f]{32}$`.
fn owner(fh: &str, host: &str, identity: &str) -> Result<(), Check> {
    if host != fh || !valid_mail_host(host) {
        return Err(Check("host"));
    }
    if !crate::state::valid_identity(identity.as_bytes()) {
        return Err(Check("identity"));
    }
    Ok(())
}

/// S§7.3 Validation: the registry `registry/<file_host>.json` of git mode `mode` and blob size `size`.
/// Unknown keys are ignored.
pub fn parse_registry(
    file_host: &str,
    mode: &str,
    size: u64,
    bytes: &[u8],
) -> Result<Registry, Check> {
    head(mode, size, bytes, REGISTRY_MAX)?;
    let f: RegistryFile = serde_json::from_slice(bytes).map_err(|_| Check("json"))?;
    owner(file_host, &f.host, &f.identity)?;
    if f.members.len() > MEMBERS_MAX {
        return Err(Check("members"));
    }
    if !f.members.iter().all(|n| valid_member_name(n)) {
        return Err(Check("member"));
    }
    Ok(Registry {
        host: f.host,
        identity: f.identity,
        members: f.members,
    })
}

impl Registry {
    /// Compact JSON, keys `version, host, identity, members`, no trailing newline: the same value gives the
    /// same bytes (S§7.5).
    pub fn to_bytes(&self) -> Vec<u8> {
        let f = RegistryFile {
            version: 1,
            host: self.host.clone(),
            identity: self.identity.clone(),
            members: self.members.clone(),
        };
        serde_json::to_vec(&f).expect("a registry serializes")
    }
}

/// S§7.4 Validation: the ack `ack/<file_host>.json` of git mode `mode` and blob size `size`. The entry limit
/// counts the array as given; a duplicate entry collapses in the set. Unknown keys are ignored.
pub fn parse_ack(file_host: &str, mode: &str, size: u64, bytes: &[u8]) -> Result<Ack, Check> {
    head(mode, size, bytes, ACK_MAX)?;
    let f: AckFile = serde_json::from_slice(bytes).map_err(|_| Check("json"))?;
    owner(file_host, &f.host, &f.identity)?;
    if f.imported.len() > ACK_ENTRIES_MAX {
        return Err(Check("entries"));
    }
    let imported = f
        .imported
        .iter()
        .map(|e| crate::address::ulid_file(&format!("{e}.json")).ok_or(Check("entry")))
        .collect::<Result<BTreeSet<_>, _>>()?;
    Ok(Ack {
        host: f.host,
        identity: f.identity,
        imported,
    })
}

impl Ack {
    /// Compact JSON, keys `version, host, identity, imported`, `imported` in ULID order, no trailing newline:
    /// the same value gives the same bytes (S§7.5).
    pub fn to_bytes(&self) -> Vec<u8> {
        let f = AckFile {
            version: 1,
            host: self.host.clone(),
            identity: self.identity.clone(),
            imported: self.imported.iter().map(|u| u.to_string()).collect(),
        };
        serde_json::to_vec(&f).expect("an ack serializes")
    }
}

/// `mail/sync/members.json` (S§7.3 Member cache, S-C8): the member names of each host, from the valid
/// registries. It holds names only: no identity, no time, and no age filter.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemberCache {
    pub hosts: BTreeMap<String, Vec<String>>,
}

impl MemberCache {
    /// Each `<name>@<host>`, in host order, then names as stored.
    pub fn addresses(&self) -> Vec<String> {
        self.hosts
            .iter()
            .flat_map(|(h, ns)| ns.iter().map(move |n| format!("{n}@{h}")))
            .collect()
    }

    /// Whether `a` is a member of its host in the cache.
    pub fn knows(&self, a: &Address) -> bool {
        self.hosts
            .get(&a.host)
            .is_some_and(|ns| ns.contains(&a.name))
    }
}

/// Writes the member cache `0600`, atomically; `mail/` is checked first (`0700`, no link, M-C4). The caller
/// holds `mail/sync/lock` (S§15).
pub fn write_cache(sp: &SyncPaths, c: &MemberCache) -> std::io::Result<()> {
    zygo_workspace::mail::ensure_mail_dir(&sp.mail)?;
    write_atomic(
        &sp.members,
        &serde_json::to_vec(c).map_err(std::io::Error::other)?,
    )
}

/// The member cache. A missing, linked, unreadable or unparsable file is the empty cache. Only hosts in the host
/// form and names in the M§6 form are kept, checked at each read (AU84); a host left with no names is dropped.
pub fn read_cache(sp: &SyncPaths) -> MemberCache {
    let mut c = match crate::state::read_regular(&sp.members) {
        Ok(Some(b)) => serde_json::from_slice::<MemberCache>(&b).unwrap_or_default(),
        _ => return MemberCache::default(),
    };
    c.hosts.retain(|h, ns| {
        ns.retain(|n| valid_member_name(n));
        valid_mail_host(h) && !ns.is_empty()
    });
    c
}
