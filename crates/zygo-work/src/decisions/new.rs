//! `zygo decision new` (D§10.2): the argument checks, the facts it fills, and the record template.
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

use super::record::{valid_hex, valid_role, Class, Kind, DIR, FILL_MARKER};
use crate::repo;

/// The arguments of `zygo decision new` (D§10.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewArgs {
    pub kind: Kind,
    pub title: String,
    pub unit: String,
    pub round: u32,
    pub role: String,
    pub class: Class,
    pub depends_on: Vec<String>,
    pub supersedes: Option<String>,
}

/// D§10.2 argument checks that need no repository: title (D§6.3: 1..=120 bytes, no `\n`/`\r`), unit (`valid_unit`),
/// round >= 1, role (`valid_role`, and in a session: in `legend` or `lead`), at most 64 `depends_on`. Err is the line.
pub fn check_args(a: &NewArgs, legend: Option<&[String]>) -> Result<(), String> {
    if !(1..=120).contains(&a.title.len()) || a.title.contains(['\n', '\r']) {
        return Err("zygo decision new: --title must be one line of 1 to 120 bytes".into());
    }
    if !crate::hooks::valid_unit(&a.unit) {
        return Err(format!("zygo decision new: --unit {} is not a unit id", a.unit));
    }
    if a.round < 1 {
        return Err("zygo decision new: --round must be 1 or more".into());
    }
    if !valid_role(&a.role) {
        return Err(format!("zygo decision new: --role {} is not a role name", a.role));
    }
    if let Some(legend) = legend
        && a.role != "lead"
        && !legend.contains(&a.role)
    {
        return Err(format!("zygo decision new: --role {} is not a role of this session", a.role));
    }
    if a.depends_on.len() > 64 {
        return Err("zygo decision new: more than 64 --depends-on".into());
    }
    Ok(())
}

/// The `model` of the newest `requests` row of `session` with `role_served = role`; `unknown` when none, when the
/// store cannot be opened read-only, or when the model is empty.
pub fn session_model(usage_db: &Path, session: &str, role: &str) -> String {
    let rows = zygo_usage::Store::open_read_only(usage_db).and_then(|s| s.requests_for_session(session)).unwrap_or_default();
    rows.into_iter()
        .rev()
        .find(|r| r.role_served.as_deref() == Some(role))
        .map(|r| r.model)
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| "unknown".into())
}

/// `git rev-parse --verify -q HEAD` of `top` under `repo::BOUND`; `Some` only for 40 or 64 lower-case hex.
pub fn head(top: &Path) -> Option<String> {
    let out = repo::git(top, &["rev-parse", "--verify", "-q", "HEAD"], Instant::now() + repo::BOUND)?;
    Some(out.trim_end().to_string()).filter(|h| valid_hex(h))
}

/// Today, UTC, `YYYY-MM-DD`.
pub fn today_utc() -> String {
    let d = time::OffsetDateTime::now_utc().date();
    format!("{:04}-{:02}-{:02}", d.year(), u8::from(d.month()), d.day())
}

/// A TOML string of `s`.
fn q(s: &str) -> String {
    toml::Value::String(s.into()).to_string()
}

/// The record text that `create` writes (D§6.2, D§10.2): every checked hint line holds [`FILL_MARKER`]; the
/// `## Finding` hint of a `decision` of a stop class (D§6.3: written, not checked) does not.
fn template(id: &str, a: &NewArgs, model: &str, commit: &str, date: &str) -> String {
    let deps: Vec<String> = a.depends_on.iter().map(|d| q(d)).collect();
    let mut t = format!(
        "+++\nid = \"{id}\"\nkind = \"{}\"\nstatus = \"open\"\nclass = \"{}\"\ntitle = {}\nunit = {}\nround = {}\nrole = {}\n\
         model = {}\ncommit = {}\ndate = {}\ndepends_on = [{}]\n# {FILL_MARKER} reverse_cost = \"low\", \"medium\" or \"high\" \
         (low: one node or less; high: a public contract, a stored format, a migration or data)\n",
        a.kind.name(), a.class.name(), q(&a.title), q(&a.unit), a.round, q(&a.role), q(model), q(commit), q(date),
        deps.join(", ")
    );
    if let Some(s) = &a.supersedes {
        t.push_str(&format!("supersedes = {}\n", q(s)));
    }
    t.push_str("+++\n");
    let finding = matches!(a.kind, Kind::AcceptedLimit | Kind::Compromise | Kind::OpenFinding);
    let hint = "The reviewer's finding, with its location, failure and evidence.";
    if finding {
        t.push_str(&format!("\n## Finding\n\n{FILL_MARKER} {hint}\n"));
    } else if a.kind == Kind::Decision && a.class != Class::None {
        t.push_str(&format!("\n## Finding\n\n{hint}\n"));
    }
    let (choice, alternatives) = if a.kind == Kind::OpenFinding {
        ("What stays open.", "The fixes considered, and why none was made.")
    } else {
        ("One paragraph: what the agent chose.", "Each alternative, and why it was rejected.")
    };
    for (name, hint) in [
        ("Choice", choice),
        ("Alternatives rejected", alternatives),
        ("Evidence", "The file:line, test output or document text that the choice rests on."),
        ("Cost to reverse", "What a reversal changes, and which later work it touches."),
    ] {
        t.push_str(&format!("\n## {name}\n\n{FILL_MARKER} {hint}\n"));
    }
    t
}

/// Writes `docs/decisions/<id>.tmp` (create_new, write, sync_all), renames it to `<id>.md`; creates `docs/decisions/`
/// when absent; does not commit. Err: `zygo decision: cannot write <path>: <io error>`; a failure after the open
/// leaves the `.tmp` file.
pub fn create(top: &Path, a: &NewArgs, model: &str, commit: &str, date: &str) -> Result<PathBuf, String> {
    let dir = top.join(DIR);
    let fail = |p: &Path, e: std::io::Error| format!("zygo decision: cannot write {}: {e}", p.display());
    fs::create_dir_all(&dir).map_err(|e| fail(&dir, e))?;
    let id = format!("DDR-{}", zygo_core::ids::ulid());
    let (tmp, path) = (dir.join(format!("{id}.tmp")), dir.join(format!("{id}.md")));
    let mut f = fs::OpenOptions::new().write(true).create_new(true).open(&tmp).map_err(|e| fail(&tmp, e))?;
    f.write_all(template(&id, a, model, commit, date).as_bytes()).and_then(|()| f.sync_all()).map_err(|e| fail(&tmp, e))?;
    drop(f);
    fs::rename(&tmp, &path).map_err(|e| fail(&path, e))?;
    Ok(path)
}
