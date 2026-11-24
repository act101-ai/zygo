//! One decision record file: its names, its front matter and its sections (D§6, D§6.1–D§6.5, D§6.8).
use std::fs;
use std::io::Read;
use std::path::Path;

/// The records directory, relative to the repository root (D§6).
pub const DIR: &str = "docs/decisions";
/// A section line that holds this marker is not filled in yet; the section counts as empty (D§6.8).
pub const FILL_MARKER: &str = "<!-- zygo: fill -->";

/// The largest record file, in bytes (D§6.8).
const MAX_FILE: usize = 65536;
/// The closing `+++`, without its line ending, ends at or before this byte (D§6.8).
const MAX_FRONT: usize = 8192;

/// Gives an enum `parse` (exact, case-sensitive) and `name` from its name strings.
macro_rules! names {
    ($t:ident { $($v:ident = $s:literal),* $(,)? }) => {
        impl $t {
            pub fn parse(s: &str) -> Option<Self> {
                match s { $($s => Some(Self::$v),)* _ => None }
            }
            pub fn name(self) -> &'static str {
                match self { $(Self::$v => $s,)* }
            }
        }
    };
}

/// The `kind` key (D§6.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Assumption,
    Decision,
    AcceptedLimit,
    Compromise,
    OpenFinding,
}
names!(Kind { Assumption = "assumption", Decision = "decision", AcceptedLimit = "accepted-limit", Compromise = "compromise", OpenFinding = "open-finding" });

/// The `status` key (D§6.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Open,
    Ratified,
    Overturned,
    Fixed,
    Superseded,
}
names!(Status { Open = "open", Ratified = "ratified", Overturned = "overturned", Fixed = "fixed", Superseded = "superseded" });

/// The `class` key: `none` or a stop class of D§8.2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    None,
    Acceptance,
    Security,
    Irreversible,
    Contract,
    Spec,
    Rule,
}
names!(Class { None = "none", Acceptance = "acceptance", Security = "security", Irreversible = "irreversible", Contract = "contract", Spec = "spec", Rule = "rule" });

/// One valid record (D§6.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// `id`
    pub id: String,
    /// `kind`
    pub kind: Kind,
    /// `status`
    pub status: Status,
    /// `class`
    pub class: Class,
    /// `title`
    pub title: String,
    /// `unit`
    pub unit: String,
    /// `round`
    pub round: u32,
    /// `role`
    pub role: String,
    /// `model`
    pub model: String,
    /// `commit`
    pub commit: String,
    /// `date`
    pub date: String,
    /// `depends_on`
    pub depends_on: Vec<String>,
    /// `reverse_cost`
    pub reverse_cost: String,
    /// `supersedes` (optional)
    pub supersedes: Option<String>,
    /// `fixed_by` (optional)
    pub fixed_by: Option<String>,
    /// `promoted_by` (optional)
    pub promoted_by: Option<String>,
    /// `reviewed` (operator)
    pub reviewed: Option<String>,
    /// `review_note` (operator)
    pub review_note: Option<String>,
    /// `promoted_to` (operator)
    pub promoted_to: Option<String>,
    /// `promotion` (operator)
    pub promotion: Option<String>,
    /// The text after the closing `+++` line and its `\n`.
    pub body: String,
}

/// D§6.1: `DDR-` then 26 Crockford base32 characters, upper case.
pub fn valid_id(s: &str) -> bool {
    s.strip_prefix("DDR-").is_some_and(|u| {
        u.len() == 26 && u.bytes().all(|b| b.is_ascii_digit() || (b.is_ascii_uppercase() && !b"ILOU".contains(&b)))
    })
}

/// What a file name of the records directory is (D§6.8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameKind {
    /// `<id>.md` with a valid id; holds the id.
    Record(String),
    /// `DDR-*.md` with a bad id.
    Malformed,
    /// Any other name; zygo ignores it.
    Other,
}

pub fn name_kind(file_name: &str) -> NameKind {
    match file_name.strip_suffix(".md") {
        Some(stem) if valid_id(stem) => NameKind::Record(stem.to_string()),
        Some(stem) if stem.starts_with("DDR-") => NameKind::Malformed,
        _ => NameKind::Other,
    }
}

/// One line without its `\n` and one trailing `\r`.
fn bare(line: &str) -> &str {
    let line = line.strip_suffix('\n').unwrap_or(line);
    line.strip_suffix('\r').unwrap_or(line)
}

/// The front matter text and the body: line 1 is `+++` and the `+++` of a later `+++` line ends within 8 KiB
/// (D§6.2, D§6.8). The line ending is not counted, so LF and CRLF records get the same limit.
fn split(text: &str) -> Result<(&str, &str), String> {
    let mut lines = text.split_inclusive('\n');
    let first = lines.next().unwrap_or("");
    if bare(first) != "+++" {
        return Err("line 1 is not +++".into());
    }
    let mut at = first.len();
    for line in lines {
        if at + "+++".len() > MAX_FRONT {
            break;
        }
        let end = at + line.len();
        if bare(line) == "+++" {
            return Ok((&text[first.len()..at], &text[end..]));
        }
        at = end;
    }
    Err("no closing +++ line within 8 KiB".into())
}

/// A key's state: absent, valid, or present and bad.
#[derive(Clone, Copy)]
enum Key<T> {
    Absent,
    Valid(T),
    Bad,
}

impl<T> Key<T> {
    fn valid(self) -> Option<T> {
        match self {
            Key::Valid(v) => Some(v),
            _ => None,
        }
    }

    /// A required key's value once the record has no fault.
    fn req(self) -> T {
        self.valid().expect("a required key without a fault")
    }
}

/// Reads the keys of one front matter table and keeps the key faults in order.
struct Keys<'a> {
    t: &'a toml::Table,
    faults: Vec<String>,
}

impl Keys<'_> {
    fn get<T>(&mut self, k: &str, required: bool, f: impl FnOnce(&toml::Value) -> Option<T>) -> Key<T> {
        match self.t.get(k) {
            None => {
                if required {
                    self.faults.push(format!("missing key {k}"));
                }
                Key::Absent
            }
            Some(v) => match f(v) {
                Some(x) => Key::Valid(x),
                None => {
                    self.faults.push(format!("bad key {k}"));
                    Key::Bad
                }
            },
        }
    }

    fn string(&mut self, k: &str, required: bool, ok: impl FnOnce(&str) -> bool) -> Key<String> {
        self.get(k, required, |v| v.as_str().filter(|s| ok(s)).map(str::to_string))
    }
}

fn valid_hex(s: &str) -> bool {
    matches!(s.len(), 40 | 64) && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn valid_date(s: &str) -> bool {
    s.len() == 10 && crate::report::parse_since(s).is_ok()
}

/// D§10.2: `^[a-z][a-z0-9-]{0,31}$`.
fn valid_role(s: &str) -> bool {
    (1..=32).contains(&s.len())
        && s.starts_with(|c: char| c.is_ascii_lowercase())
        && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// The content lines of section `## <name>`: from its first heading line to the next `## ` line; `None` when absent.
fn section<'a>(body: &'a str, name: &str) -> Option<Vec<&'a str>> {
    let heading = format!("## {name}");
    let mut lines = body.split('\n').map(bare);
    lines.find(|l| l.trim_end() == heading)?;
    Some(lines.take_while(|l| !l.starts_with("## ")).collect())
}

/// Parses and checks one record file; `stem` is the file name without `.md`. Each `Err` item is one fault line.
pub fn parse_record(stem: &str, bytes: &[u8]) -> Result<Record, Vec<String>> {
    if bytes.len() > MAX_FILE {
        return Err(vec!["over 64 KiB".into()]);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| vec!["not UTF-8".to_string()])?;
    let (front, body) = split(text).map_err(|e| vec![e])?;
    let t: toml::Table = toml::from_str(front)
        .map_err(|e| vec![format!("front matter is not TOML: {}", e.message().replace(['\n', '\r'], " "))])?;

    let mut k = Keys { t: &t, faults: Vec::new() };
    let id = k.string("id", true, valid_id);
    let kind = k.get("kind", true, |v| v.as_str().and_then(Kind::parse));
    let status = k.get("status", true, |v| v.as_str().and_then(Status::parse));
    let class = k.get("class", true, |v| v.as_str().and_then(Class::parse));
    let title = k.string("title", true, |s| (1..=120).contains(&s.len()) && !s.contains(['\n', '\r']));
    let unit = k.string("unit", true, crate::hooks::valid_unit);
    let round = k.get("round", true, |v| v.as_integer().filter(|r| *r >= 1).and_then(|r| u32::try_from(r).ok()));
    let role = k.string("role", true, valid_role);
    let model = k.string("model", true, |s| !s.is_empty());
    let commit = k.string("commit", true, valid_hex);
    let date = k.string("date", true, valid_date);
    let depends_on = k.get("depends_on", true, |v| {
        let a = v.as_array().filter(|a| a.len() <= 64)?;
        a.iter().map(|x| x.as_str().filter(|s| valid_id(s)).map(str::to_string)).collect::<Option<Vec<_>>>()
    });
    let reverse_cost = k.string("reverse_cost", true, |s| matches!(s, "low" | "medium" | "high"));
    let supersedes = k.string("supersedes", false, valid_id);
    let fixed_by = k.string("fixed_by", false, valid_hex);
    let promoted_by = k.string("promoted_by", false, valid_hex);
    let reviewed = k.string("reviewed", false, valid_date);
    let review_note = k.string("review_note", false, |s| s.len() <= 2000);
    let promoted_to = k.string("promoted_to", false, |s| matches!(s, "adr" | "spec" | "rule"));
    let promotion = k.string("promotion", false, |s| s.len() <= 500);
    let mut faults = k.faults;

    if let Key::Valid(id) = &id
        && id != stem
    {
        faults.push("id does not equal the file stem".into());
    }

    let finding = matches!(kind, Key::Valid(Kind::AcceptedLimit | Kind::Compromise | Kind::OpenFinding));
    let names = ["Finding", "Choice", "Alternatives rejected", "Evidence", "Cost to reverse"];
    for name in names.into_iter().skip(usize::from(!finding)) {
        match section(body, name) {
            None => faults.push(format!("missing section ## {name}")),
            Some(c) if c.iter().all(|l| l.trim().is_empty()) || c.iter().any(|l| l.contains(FILL_MARKER)) => {
                faults.push(format!("empty section ## {name}"))
            }
            Some(_) => {}
        }
    }

    if matches!(status, Key::Valid(Status::Fixed)) && matches!(fixed_by, Key::Absent) {
        faults.push("fixed without fixed_by".into());
    }
    let note_empty = match &review_note {
        Key::Absent => true,
        Key::Valid(n) => n.is_empty(),
        Key::Bad => false,
    };
    if matches!(status, Key::Valid(Status::Overturned)) && note_empty {
        faults.push("overturned without review_note".into());
    }
    if matches!(promoted_to, Key::Valid(_)) && matches!(promotion, Key::Absent) {
        faults.push("promoted_to without promotion".into());
    }

    if !faults.is_empty() {
        return Err(faults);
    }
    // No fault: every required key is valid.
    Ok(Record {
        id: id.req(),
        kind: kind.req(),
        status: status.req(),
        class: class.req(),
        title: title.req(),
        unit: unit.req(),
        round: round.req(),
        role: role.req(),
        model: model.req(),
        commit: commit.req(),
        date: date.req(),
        depends_on: depends_on.req(),
        reverse_cost: reverse_cost.req(),
        supersedes: supersedes.valid(),
        fixed_by: fixed_by.valid(),
        promoted_by: promoted_by.valid(),
        reviewed: reviewed.valid(),
        review_note: review_note.valid(),
        promoted_to: promoted_to.valid(),
        promotion: promotion.valid(),
        body: body.to_string(),
    })
}

/// Windows `FILE_FLAG_OPEN_REPARSE_POINT`: open a link itself, not its target (the no-follow open on Windows).
#[cfg(windows)]
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;

/// `true` when the failed open of `path` met a link: `ELOOP` from `O_NOFOLLOW`, or a link seen after the failure
/// (other BSDs give `EMLINK` or `EFTYPE`). As `zygo_hooks::workspace::open_met_link`.
fn open_met_link(e: &std::io::Error, path: &Path) -> bool {
    #[cfg(unix)]
    if e.raw_os_error() == Some(libc::ELOOP) {
        return true;
    }
    #[cfg(not(unix))]
    let _ = e;
    fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
}

/// Reads the bytes of one record file (D§6.8); [`parse_record`] checks them. `lstat` comes first: a link, a FIFO, a directory or a file over 64 KiB
/// is never opened. On unix the open adds `O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC` (the pattern of
/// `zygo_hooks::workspace::read_session_file`); on Windows it adds `FILE_FLAG_OPEN_REPARSE_POINT`. The opened handle
/// must be a regular file and not a link. At most 64 KiB + 1 bytes are read.
pub fn read_bytes(path: &Path) -> Result<Vec<u8>, Vec<String>> {
    let one = |s: String| vec![s];
    let m = fs::symlink_metadata(path).map_err(|e| one(format!("cannot read: {e}")))?;
    if m.file_type().is_symlink() {
        return Err(one("is a link; not read".into()));
    }
    if !m.is_file() {
        return Err(one("is not a regular file; not read".into()));
    }
    if m.len() > MAX_FILE as u64 {
        return Err(one("over 64 KiB".into()));
    }
    let mut o = fs::OpenOptions::new();
    o.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        o.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let f = o.open(path).map_err(|e| {
        if open_met_link(&e, path) {
            return one("is a link; not read".into());
        }
        one(format!("cannot read: {e}"))
    })?;
    match f.metadata() {
        Ok(m) if m.file_type().is_symlink() => return Err(one("is a link; not read".into())),
        Ok(m) if m.is_file() => {}
        Ok(_) => return Err(one("is not a regular file; not read".into())),
        Err(e) => return Err(one(format!("cannot read: {e}"))),
    }
    let mut bytes = Vec::new();
    f.take(MAX_FILE as u64 + 1).read_to_end(&mut bytes).map_err(|e| one(format!("cannot read: {e}")))?;
    Ok(bytes)
}

/// Reads one record file with [`read_bytes`] and checks it with [`parse_record`] (D§6.8).
pub fn read_record(path: &Path) -> Result<Record, Vec<String>> {
    let bytes = read_bytes(path)?;
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    parse_record(stem, &bytes)
}
