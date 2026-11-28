//! The git read of `.zygo/` from the default branch (R§6.1 steps 2-5) and at a pinned commit (R§7.4).

use std::collections::BTreeMap;
use std::path::Path;
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use zygo_core::esc;

/// One budget for all the git calls of a read (R§6.1, as `repo_file::GIT_BOUND`).
const BUDGET: Duration = Duration::from_secs(2);
const REFUSAL: &str = "origin/HEAD points outside refs/remotes/origin/";
/// git's pathspec-magic variables: inherited, they change what an `ls-tree` pathspec matches, or make it fail.
const PATHSPEC_ENV: [&str; 4] = ["GIT_ICASE_PATHSPECS", "GIT_GLOB_PATHSPECS", "GIT_NOGLOB_PATHSPECS", "GIT_LITERAL_PATHSPECS"];

pub trait GitRunner {
    /// `git -C <top> <args>`, finished by `deadline`. `Err` is a one-line reason (`timed out`, `tool not found`, `failed`).
    fn git(&self, top: &Path, args: &[&str], deadline: Instant) -> Result<Output, String>;
}

/// The [`GitRunner`] that runs `repo_file::git_command()`.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemGit;

impl SystemGit {
    /// The command each call runs: `repo_file::git_command()` with no lazy fetch of a partial clone's missing objects,
    /// no terminal prompt, no replace refs and none of the caller's [`PATHSPEC_ENV`] variables.
    pub fn command() -> Command {
        let mut c = crate::repo_file::git_command();
        for k in PATHSPEC_ENV {
            c.env_remove(k);
        }
        c.env("GIT_NO_LAZY_FETCH", "1").env("GIT_TERMINAL_PROMPT", "0").env("GIT_NO_REPLACE_OBJECTS", "1");
        c
    }
}

impl GitRunner for SystemGit {
    fn git(&self, top: &Path, args: &[&str], deadline: Instant) -> Result<Output, String> {
        super::trap("SystemGit::git");
        crate::repo_file::git_out_with(Self::command(), top, args, deadline)
            .map(|o| Output { status: o.status, stdout: o.out, stderr: o.err })
            .map_err(str::to_owned)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Files {
    /// None for a pinned read.
    pub full_ref: Option<String>,
    /// The read commit, 40 or 64 lower-case hex.
    pub commit: String,
    /// `.zygo/workspace.toml`.
    pub workspace: Option<Vec<u8>>,
    /// Key: the stem, `rust` for `.zygo/profiles/rust.toml`.
    pub profiles: BTreeMap<String, Vec<u8>>,
    /// Full paths, e.g. `.zygo/README`, `.zygo/profiles/notes.md`.
    pub ignored: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Read {
    Absent { full_ref: Option<String> },
    Unread { why: String },
    /// `workspace`: the read lists a `.zygo/workspace.toml` entry (WD-16 review F4: `zygo loadout list` names an
    /// invalid repository loadout only then).
    Invalid { full_ref: Option<String>, commit: String, why: String, workspace: bool },
    Gone { commit: String },
    Found { files: Files },
}

/// R§6.1 step 4: 64 KiB.
pub const WORKSPACE_MAX: u64 = 64 * 1024;
/// R§6.1 step 4: 16 KiB.
pub const PROFILE_MAX: u64 = 16 * 1024;
/// R§6.1 step 4.
pub const PROFILES_MAX: usize = 32;

/// R§6.1 steps 2-5 on the default branch, as a full ref, under one budget.
pub fn read_default(top: &Path, git: &dyn GitRunner) -> Read {
    super::trap("read_default");
    let d = Instant::now() + BUDGET;
    let unread = |why: &str| Read::Unread { why: why.to_string() };
    let full_ref = match git.git(top, &["symbolic-ref", "--quiet", "refs/remotes/origin/HEAD"], d) {
        Err(w) => return unread(&format!("git symbolic-ref: {w}")),
        Ok(o) if o.status.success() => {
            let t = o.stdout.strip_suffix(b"\n").unwrap_or(&o.stdout);
            let Ok(t) = std::str::from_utf8(t) else { return unread("git symbolic-ref: failed") };
            if !t.starts_with("refs/remotes/origin/") {
                return unread(REFUSAL);
            }
            t.to_string()
        }
        // No such ref, or a direct one: `symbolic-ref` says the same, `show-ref` tells them apart.
        Ok(o) if quiet_exit(&o, 1) => match exists(top, git, "refs/remotes/origin/HEAD", d) {
            Err(w) => return unread(&w),
            Ok(false) => "refs/heads/main".to_string(),
            Ok(true) => return unread(REFUSAL),
        },
        Ok(_) => return unread("git symbolic-ref: failed"),
    };
    // The ref by exact name: `rev-parse <name>` would also take `refs/tags/<name>` when the ref is missing.
    match exists(top, git, &full_ref, d) {
        Err(w) => return unread(&w),
        Ok(false) => return Read::Absent { full_ref: Some(full_ref) },
        Ok(true) => {}
    }
    let oid = match git.git(top, &["show-ref", "--verify", "--hash", &full_ref], d) {
        Err(w) => return unread(&format!("git show-ref: {w}")),
        Ok(o) if o.status.success() => match std::str::from_utf8(&o.stdout).map(str::trim) {
            Ok(c) if is_oid(c) => c.to_string(),
            _ => return unread("git show-ref: failed"),
        },
        Ok(_) => return unread("git show-ref: failed"),
    };
    let commit = match git.git(top, &["rev-parse", "--verify", "--quiet", &format!("{oid}^{{commit}}")], d) {
        Err(w) => return unread(&format!("git rev-parse: {w}")),
        Ok(o) if quiet_exit(&o, 1) => return Read::Absent { full_ref: Some(full_ref) },
        Ok(o) if o.status.success() => match std::str::from_utf8(&o.stdout).map(str::trim) {
            Ok(c) if is_oid(c) => c.to_string(),
            _ => return unread("git rev-parse: failed"),
        },
        Ok(_) => return unread("git rev-parse: failed"),
    };
    match steps(top, git, Some(full_ref), commit, d) {
        Read::Gone { commit } => Read::Unread { why: format!("commit {commit} or one of its objects is missing") },
        r => r,
    }
}

/// R§7.4: R§6.1 steps 3-5 at the saved commit, with no default-branch lookup.
pub fn read_at(top: &Path, commit: &str, git: &dyn GitRunner) -> Read {
    super::trap("read_at");
    if !is_oid(commit) {
        return Read::Unread { why: format!("pinned commit {} is not an object id", esc(super::short(commit))) };
    }
    steps(top, git, None, commit.to_string(), Instant::now() + BUDGET)
}

/// Whether the ref `full_ref` exists, by exact name; `Err` is the unread reason.
fn exists(top: &Path, git: &dyn GitRunner, full_ref: &str, d: Instant) -> Result<bool, String> {
    match git.git(top, &["show-ref", "--verify", "--quiet", full_ref], d) {
        Err(w) => Err(format!("git show-ref: {w}")),
        Ok(o) if quiet_exit(&o, 1) => Ok(false),
        Ok(o) if o.status.success() => Ok(true),
        Ok(_) => Err("git show-ref: failed".into()),
    }
}

/// 40 or 64 lower-case hex digits.
fn is_oid(s: &str) -> bool {
    (s.len() == 40 || s.len() == 64) && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Exit `code` with nothing on stdout or stderr.
fn quiet_exit(o: &Output, code: i32) -> bool {
    o.status.code() == Some(code) && o.stdout.is_empty() && o.stderr.is_empty()
}

/// One `ls-tree --long` entry; `size` is 0 for a tree or a gitlink (`-`).
struct Ent {
    mode: String,
    kind: String,
    oid: String,
    size: u64,
    path: Vec<u8>,
}

impl Ent {
    fn regular(&self) -> bool {
        (self.mode == "100644" || self.mode == "100755") && self.kind == "blob"
    }
    fn tree(&self) -> bool {
        self.mode == "040000" && self.kind == "tree"
    }
    fn name(&self) -> String {
        String::from_utf8_lossy(&self.path).into_owned()
    }
}

/// A failed git call of steps 3-5: its reason and the tree or blob it named, when known from a listing.
struct Fail {
    why: String,
    oid: Option<String>,
}

/// Why steps 3-5 stopped early.
enum Stop {
    Fail(Fail),
    Read(Read),
}

impl From<Fail> for Stop {
    fn from(f: Fail) -> Self {
        Stop::Fail(f)
    }
}

/// Steps 3-5; a failed call is `Gone` when the commit, its root tree or the object it named is missing, else `Unread`.
fn steps(top: &Path, git: &dyn GitRunner, full_ref: Option<String>, commit: String, d: Instant) -> Read {
    match files(top, git, &full_ref, &commit, d) {
        Ok(files) => Read::Found { files },
        Err(Stop::Read(r)) => r,
        Err(Stop::Fail(f)) => {
            let gone = |oid: &str| matches!(git.git(top, &["cat-file", "-e", oid], d), Ok(o) if quiet_exit(&o, 1));
            // The tree id from the commit object itself: `<commit>^{tree}` fails when the tree is missing.
            let tree = || match git.git(top, &["cat-file", "commit", &commit], d) {
                Ok(o) if o.status.success() => {
                    let line = o.stdout.strip_prefix(b"tree ").and_then(|r| r.split(|&b| b == b'\n').next());
                    line.and_then(|t| std::str::from_utf8(t).ok()).map(str::to_owned)
                }
                _ => None,
            };
            if gone(&commit) || tree().is_some_and(|t| is_oid(&t) && gone(&t)) || f.oid.as_deref().is_some_and(gone) {
                Read::Gone { commit }
            } else {
                Read::Unread { why: f.why }
            }
        }
    }
}

fn files(top: &Path, git: &dyn GitRunner, full_ref: &Option<String>, commit: &str, d: Instant) -> Result<Files, Stop> {
    // `workspace`: the read lists a `.zygo/workspace.toml` entry, whatever its type (WD-16 review F4).
    let invalid = |why: String, workspace: bool| {
        Stop::Read(Read::Invalid { full_ref: full_ref.clone(), commit: commit.to_string(), why, workspace })
    };
    let list = |spec: &str, oid: Option<&str>, prefix: &[u8]| list(top, git, commit, spec, oid, prefix, d);

    // Step 3, then the step 4 checks in order.
    let l1 = list(".zygo", None, b"")?;
    let Some(zygo) = l1.first() else { return Err(Stop::Read(Read::Absent { full_ref: full_ref.clone() })) };
    if l1.len() != 1 || zygo.path != b".zygo" {
        return Err(Fail { why: "git ls-tree: failed".into(), oid: None }.into());
    }
    if !zygo.tree() {
        return Err(invalid(".zygo is not a directory".into(), false));
    }
    let (mut workspace, mut dir, mut ignored) = (None, None, Vec::new());
    for e in list(".zygo/", Some(&zygo.oid), b".zygo/")? {
        match &e.path[b".zygo/".len()..] {
            b"workspace.toml" => workspace = Some(e),
            b"profiles" => dir = Some(e),
            _ => ignored.push(e.name()),
        }
    }
    let ws = workspace.is_some();
    if workspace.as_ref().is_some_and(|w| !w.regular()) {
        return Err(invalid(".zygo/workspace.toml is not a regular file".into(), ws));
    }
    let mut profiles = Vec::new();
    if let Some(dir) = &dir {
        if !dir.tree() {
            return Err(invalid(".zygo/profiles is not a directory".into(), ws));
        }
        for e in list(".zygo/profiles/", Some(&dir.oid), b".zygo/profiles/")? {
            if e.path.ends_with(b".toml") {
                profiles.push(e);
            } else {
                ignored.push(e.name());
            }
        }
    }
    for e in &profiles {
        let shown = esc(&e.name());
        if !e.regular() {
            return Err(invalid(format!("{shown} is not a regular file"), ws));
        }
        let stem = &e.path[b".zygo/profiles/".len()..e.path.len() - b".toml".len()];
        if !(1..=32).contains(&stem.len()) || !stem.iter().all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'-')) {
            return Err(invalid(format!("{shown}: a profile file name must match ^[a-z0-9-]{{1,32}}\\.toml$"), ws));
        }
        if stem == b"none" {
            return Err(invalid(format!("{shown}: the profile name none is reserved"), ws));
        }
    }
    let sized = workspace.iter().map(|e| (e, WORKSPACE_MAX)).chain(profiles.iter().map(|e| (e, PROFILE_MAX)));
    for (e, max) in sized {
        if e.size > max {
            return Err(invalid(format!("{} is larger than {max} bytes", esc(&e.name())), ws));
        }
    }
    if profiles.len() > PROFILES_MAX {
        return Err(invalid(format!(".zygo/profiles holds more than {PROFILES_MAX} profile files"), ws));
    }
    if workspace.is_none() && profiles.is_empty() {
        return Err(Stop::Read(Read::Absent { full_ref: full_ref.clone() }));
    }

    // Step 5: each file by its listed object id.
    let content = |e: &Ent| -> Result<Vec<u8>, Stop> {
        let fail = |w: &str| Stop::Fail(Fail { why: format!("git cat-file: {w}"), oid: Some(e.oid.clone()) });
        let o = git.git(top, &["cat-file", "blob", &e.oid], d).map_err(|w| fail(&w))?;
        if !o.status.success() {
            return Err(fail("failed"));
        }
        let shown = esc(&e.name());
        if o.stdout.len() as u64 != e.size {
            return Err(invalid(format!("{shown}: read {} bytes, listed {}", o.stdout.len(), e.size), ws));
        }
        if std::str::from_utf8(&o.stdout).is_err() {
            return Err(invalid(format!("{shown} is not UTF-8"), ws));
        }
        Ok(o.stdout)
    };
    let workspace = workspace.as_ref().map(content).transpose()?;
    let mut read = BTreeMap::new();
    for e in &profiles {
        let stem = String::from_utf8_lossy(&e.path[b".zygo/profiles/".len()..e.path.len() - b".toml".len()]).into_owned();
        read.insert(stem, content(e)?);
    }
    ignored.sort();
    Ok(Files { full_ref: full_ref.clone(), commit: commit.to_string(), workspace, profiles: read, ignored })
}

/// `ls-tree -z --long --full-tree <commit> -- <spec>`, sorted by path bytes. Every path must be `prefix` and one more
/// name (with an empty `prefix`, any path without `/`). `oid`: the tree listed, for the gone check.
fn list(top: &Path, git: &dyn GitRunner, commit: &str, spec: &str, oid: Option<&str>, prefix: &[u8], d: Instant) -> Result<Vec<Ent>, Fail> {
    let fail = |w: &str| Fail { why: format!("git ls-tree: {w}"), oid: oid.map(str::to_owned) };
    let o = git.git(top, &["ls-tree", "-z", "--long", "--full-tree", commit, "--", spec], d).map_err(|w| fail(&w))?;
    if !o.status.success() {
        return Err(fail("failed"));
    }
    let Some(mut ents) = parse(&o.stdout) else {
        // `ls-tree --long` lists a blob it does not have with the size `BAD`, and exits 0: name that blob.
        return Err(Fail { oid: bad_blob(&o.stdout).or(oid.map(str::to_owned)), ..fail("failed") });
    };
    let child = |p: &[u8]| p.strip_prefix(prefix).is_some_and(|r| !r.is_empty() && !r.contains(&b'/'));
    if !ents.iter().all(|e| child(&e.path)) {
        return Err(fail("failed"));
    }
    ents.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(ents)
}

/// The object id of the first record `<mode> blob <oid> BAD\t<path>`.
fn bad_blob(out: &[u8]) -> Option<String> {
    out.split(|&b| b == 0).find_map(|rec| {
        let meta = std::str::from_utf8(&rec[..rec.iter().position(|&b| b == b'\t')?]).ok()?;
        match meta.split_ascii_whitespace().collect::<Vec<_>>()[..] {
            [_, "blob", oid, "BAD"] if is_oid(oid) => Some(oid.to_string()),
            _ => None,
        }
    })
}

/// Records `<mode> <type> <oid> <size>\t<path>\0`; `None` for any other shape.
fn parse(out: &[u8]) -> Option<Vec<Ent>> {
    let mut recs: Vec<&[u8]> = out.split(|&b| b == 0).collect();
    if recs.pop() != Some(&b""[..]) {
        return None;
    }
    recs.into_iter()
        .map(|rec| {
            let tab = rec.iter().position(|&b| b == b'\t')?;
            let meta = std::str::from_utf8(&rec[..tab]).ok()?;
            let f: Vec<&str> = meta.split_ascii_whitespace().collect();
            let [mode, kind, oid, size] = f[..] else { return None };
            let size = match (kind, size) {
                ("blob", s) => s.parse().ok()?,
                (_, "-") => 0,
                _ => return None,
            };
            let path = rec[tab + 1..].to_vec();
            (is_oid(oid) && !path.is_empty()).then(|| Ent { mode: mode.into(), kind: kind.into(), oid: oid.into(), size, path })
        })
        .collect()
}
