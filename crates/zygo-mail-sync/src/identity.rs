//! The registry and ack files of a side-branch tip, the own hosts of this identity, and the collision and twin
//! checks (mail sync design S§7.3-S§7.5, S-C2, S-C5, S-C7, S-C8). Every file is checked at each read (S-C5); a
//! refused file is kept with its first failed [`Check`] and logged by check name and blob id only (S-C2). A file
//! name is remote data: it is only a map key, never a filesystem path. No check reads a time or a config of another
//! host (S-C8).
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Instant;

use zygo_config::valid_mail_host;
use zygo_core::paths::Paths;
use zygo_work::repo::Flow;

use crate::branch::{BranchError, Entry, cat, ls};
use crate::registry::{ACK_MAX, Ack, Check, REGISTRY_MAX, Registry, parse_ack, parse_registry};
use crate::state::{State, SyncPaths};

/// S§7.5 Collision: the line the CLI prints.
const COLLISION: &str = "zygo: the mail host name is taken on the side branch by another identity. If that is another machine, set another [mail] host. If it is this workspace from before (a moved repository, a ws forget, a restored backup), run zygo mail sync --take-host.";

/// S§7.5 Twin: the line the CLI prints.
const TWIN: &str = "zygo: another workspace writes with this mail identity (copied state?); run zygo mail sync --new-identity here, and set another [mail] host";

/// One registry or ack file of a tip: its blob id and its parsed value, or the first check it failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Read<T> {
    pub blob: String,
    pub value: Result<T, Check>,
}

/// The registry and ack files of a tip, by the host of their file name (`registry/<host>.json`, `ack/<host>.json`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TipMeta {
    pub registries: BTreeMap<String, Read<Registry>>,
    pub acks: BTreeMap<String, Read<Ack>>,
}

/// Why a sync stops before it writes (S§7.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stop {
    /// The current host's file holds another identity, or the current host's name is held by more than one file;
    /// `moved_from` is another workspace state on this machine (its workspace id) that holds that identity (S§19 #10).
    Collision { moved_from: Option<String> },
    /// Another workspace writes with this identity (S§19 #11).
    Twin,
}

impl Stop {
    /// The S§7.5 line. The CLI prints it; a child writes only [`Stop::check_name`] to `errors.log` (S-C2).
    pub fn line(&self) -> String {
        match self {
            Stop::Twin => TWIN.to_string(),
            Stop::Collision { moved_from: None } => COLLISION.to_string(),
            Stop::Collision { moved_from: Some(id) } => format!(
                "{COLLISION} The workspace state {id} on this machine also holds that identity."
            ),
        }
    }

    /// The check name: a valid `state.json` `last_error` value.
    pub fn check_name(&self) -> &'static str {
        match self {
            Stop::Collision { .. } => "collision",
            Stop::Twin => "twin",
        }
    }
}

/// S§7.3, S§7.4 Validation, S-C5: the `registry/<h>.json` and `ack/<h>.json` files of `tip`. A name without the
/// `.json` suffix is not read. A `.json` name whose `h` is not a mail host is `Check("name")` and is not kept. A
/// file that is not a `100644` blob is `Check("mode")` and one above the size limit is `Check("size")`, both
/// without a read. A second entry with the name of a kept one is not read, and the kept value becomes
/// `Check("duplicate")`. Each refused entry logs one line, `mail sync: registry file refused: check <check>, object
/// <id>` (or `ack file`), which never names the path (it holds a host, S-C2).
pub fn read_meta(wt: &Path, tip: &str, deadline: Instant, log: &mut dyn FnMut(&str))
    -> Result<TipMeta, BranchError> {
    let mut meta = TipMeta::default();
    for (folder, kind) in [("registry/", "registry"), ("ack/", "ack")] {
        let mut entries = Vec::new();
        ls(wt, tip, Some(folder), deadline, &mut |e| {
            entries.push(e);
            Flow::Go
        })?;
        for e in entries {
            let Some(h) = e.path.strip_prefix(folder).and_then(|n| n.strip_suffix(".json")) else { continue };
            if !valid_mail_host(h) {
                refused(kind, Check("name"), &e.id, log);
                continue;
            }
            let h = h.to_string();
            if kind == "registry" {
                if duplicate(&mut meta.registries, &h, kind, &e.id, log) {
                    continue;
                }
                let value = read_one(wt, &e, REGISTRY_MAX, deadline, |s, b| parse_registry(&h, &e.mode, s, b))?;
                meta.registries.insert(h, logged(kind, e.id, value, log));
            } else {
                if duplicate(&mut meta.acks, &h, kind, &e.id, log) {
                    continue;
                }
                let value = read_one(wt, &e, ACK_MAX, deadline, |s, b| parse_ack(&h, &e.mode, s, b))?;
                meta.acks.insert(h, logged(kind, e.id, value, log));
            }
        }
    }
    Ok(meta)
}

/// The parsed value of `e`: `mode` and `size` are checked before `cat`.
fn read_one<T>(wt: &Path, e: &Entry, max: u64, deadline: Instant, parse: impl FnOnce(u64, &[u8]) -> Result<T, Check>)
    -> Result<Result<T, Check>, BranchError> {
    let size = match e.size {
        Some(s) if e.mode == "100644" && e.kind == "blob" => s,
        _ => return Ok(Err(Check("mode"))),
    };
    if size > max {
        return Ok(Err(Check("size")));
    }
    let bytes = cat(wt, &e.id, max, deadline)?;
    Ok(parse(size, &bytes))
}

/// Whether `h` is already a key of `map`: then its value becomes `Check("duplicate")` and `id` is logged, so no
/// entry of a name that is listed twice can be taken as the file of that host.
fn duplicate<T>(map: &mut BTreeMap<String, Read<T>>, h: &str, kind: &str, id: &str, log: &mut dyn FnMut(&str)) -> bool {
    let Some(r) = map.get_mut(h) else { return false };
    r.value = Err(Check("duplicate"));
    refused(kind, Check("duplicate"), id, log);
    true
}

/// The log line of one refused entry: its kind, check and object id only (S-C2).
fn refused(kind: &str, c: Check, id: &str, log: &mut dyn FnMut(&str)) {
    log(&format!("mail sync: {kind} file refused: check {}, object {id}", c.0));
}

/// `value` as a [`Read`]; a refused value is logged.
fn logged<T>(kind: &str, blob: String, value: Result<T, Check>, log: &mut dyn FnMut(&str)) -> Read<T> {
    if let Err(c) = &value {
        refused(kind, *c, &blob, log);
    }
    Read { blob, value }
}

/// S§7.5 Own hosts: `host`, then each other host, in key order, whose valid ack holds `identity`.
pub fn own_hosts(meta: &TipMeta, identity: &str, host: &str) -> Vec<String> {
    let mut v = vec![host.to_string()];
    v.extend(meta.acks.iter()
        .filter(|(k, r)| k.as_str() != host && matches!(&r.value, Ok(a) if a.identity == identity))
        .map(|(k, _)| k.clone()));
    v
}

/// S§7.5: collision first (the current host's registry, then its ack, holds another identity, or is listed twice
/// at the tip), then twin (a valid file of this identity, at any host, whose blob is neither the pushed nor the
/// pending one in `st`). Any other refused file is skipped. `paths` and `self_ws` find another workspace state that holds the colliding identity (S§19 #10).
pub fn check(meta: &TipMeta, host: &str, identity: &str, st: &State, paths: &Paths, self_ws: &str)
    -> Result<(), Stop> {
    let dup = Check("duplicate");
    if meta.registries.get(host).is_some_and(|r| r.value.as_ref().err() == Some(&dup))
        || meta.acks.get(host).is_some_and(|r| r.value.as_ref().err() == Some(&dup)) {
        return Err(Stop::Collision { moved_from: None });
    }
    let at_host = [
        meta.registries.get(host).and_then(|r| r.value.as_ref().ok()).map(|r| &r.identity),
        meta.acks.get(host).and_then(|r| r.value.as_ref().ok()).map(|a| &a.identity),
    ];
    if let Some(other) = at_host.into_iter().flatten().find(|i| i.as_str() != identity) {
        return Err(Stop::Collision { moved_from: moved(paths, self_ws, other) });
    }
    let regs = meta.registries.iter()
        .filter(|(_, r)| matches!(&r.value, Ok(v) if v.identity == identity))
        .map(|(k, r)| (format!("registry/{k}.json"), &r.blob));
    let acks = meta.acks.iter()
        .filter(|(_, r)| matches!(&r.value, Ok(v) if v.identity == identity))
        .map(|(k, r)| (format!("ack/{k}.json"), &r.blob));
    for (path, blob) in regs.chain(acks) {
        if st.pushed.get(&path) != Some(blob) && st.pending.get(&path) != Some(blob) {
            return Err(Stop::Twin);
        }
    }
    Ok(())
}

/// The first other workspace state, by sorted id, whose identity is `identity`. A name that is not a workspace id
/// is never read; a state whose `<id>`, `mail` or `sync` folder is a link, or whose identity file is a link,
/// unreadable or invalid, is skipped.
fn moved(paths: &Paths, self_ws: &str, identity: &str) -> Option<String> {
    let mut names: Vec<String> = std::fs::read_dir(paths.workspaces_dir()).ok()?
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n != self_ws)
        .collect();
    let link = |p: &Path| std::fs::symlink_metadata(p).map_or(true, |m| m.file_type().is_symlink());
    names.sort();
    names.into_iter().find(|n| {
        let ws = paths.workspaces_dir().join(n);
        if [ws.clone(), ws.join("mail"), ws.join("mail/sync")].iter().any(|p| link(p)) {
            return false;
        }
        SyncPaths::new(paths, n).ok()
            .and_then(|sp| crate::state::identity(&sp).ok().flatten())
            .is_some_and(|i| i == identity)
    })
}
