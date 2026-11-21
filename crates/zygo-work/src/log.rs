//! The per-agent work log (§13.3): `<dir>/<agent>.jsonl`, one JSON line per fact, and `<agent>.begin.json` for the
//! open unit. Writers hold an exclusive lock on `<agent>.lock`.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};

/// §13.3 "Lines ≤ 1 KiB" (newline included).
pub const MAX_LINE_BYTES: usize = 1024;
/// §13.3: then `overflow` and nothing more.
pub const MAX_LINES: u32 = 4000;
/// §13.3: a hook reads at most the log's last 256 KiB.
pub const TAIL_BYTES: u64 = 256 * 1024;
/// §13.3: `repeat` looks back over the last 256 calls.
pub const REPEAT_WINDOW: usize = 256;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogLine {
    /// 1-based line number, set by `append`.
    #[serde(default)]
    pub n: u32,
    /// Unix seconds.
    pub ts: i64,
    #[serde(flatten)]
    pub kind: Kind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Kind {
    Begin { unit: String, round: u32 },
    End { reason: String },
    Call { tool: String, h: String },
    Failed { tool: String, h: String },
    Check { result: CheckResult },
    Edit { root: Option<String> },
    Repeat { tool: String },
    Navigated { paths: Vec<String> },
    Pushed,
    PrOpened,
    Trimmed,
    Guard { guard: String, level: String },
    Levels { levels: BTreeMap<String, String> },
    Overflow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckResult {
    Passed,
    Failed,
}

/// The open unit of one agent (`<agent>.begin.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Begin {
    pub unit: String,
    pub round: u32,
    pub check: String,
    pub files: Vec<String>,
    /// Unix seconds.
    pub began_at: i64,
    /// Byte offset of the unit's `begin` line in the log.
    pub offset: u64,
    pub session: String,
    pub agent_type: Option<String>,
    /// Unix seconds of the last `SubagentStop`; a stopped unit can resume.
    #[serde(default)]
    pub stopped_at: Option<i64>,
    /// The repository root of the unit's last edit; `None` before the first edit or when that edit was outside a repository.
    #[serde(default)]
    pub edit_root: Option<String>,
}

pub fn log_path(dir: &Path, agent: &str) -> PathBuf {
    dir.join(format!("{agent}.jsonl"))
}

pub fn begin_path(dir: &Path, agent: &str) -> PathBuf {
    dir.join(format!("{agent}.begin.json"))
}

/// The bytes of the log's last `TAIL_BYTES` from the first whole line on, with the file offset of that line.
fn tail_window(path: &Path) -> Option<(u64, Vec<u8>)> {
    let mut f = File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let start = len.saturating_sub(TAIL_BYTES);
    // From the byte before the window: a window that starts at a line start keeps that line.
    let from = start.saturating_sub(1);
    let mut buf = Vec::new();
    f.seek(SeekFrom::Start(from)).ok()?;
    f.read_to_end(&mut buf).ok()?;
    let pos = if start > 0 { buf.iter().position(|&b| b == b'\n')? + 1 } else { 0 };
    Some((from + pos as u64, buf.split_off(pos)))
}

/// `(byte offset, line)` of the whole, parseable lines inside the log's last `TAIL_BYTES`.
pub fn tail(dir: &Path, agent: &str) -> Vec<(u64, LogLine)> {
    let Some((at, buf)) = tail_window(&log_path(dir, agent)) else { return Vec::new() };
    let mut out = Vec::new();
    let mut pos = 0;
    while let Some(i) = buf[pos..].iter().position(|&b| b == b'\n') {
        if let Ok(l) = serde_json::from_slice::<LogLine>(&buf[pos..pos + i]) {
            out.push((at + pos as u64, l));
        }
        pos += i + 1;
    }
    out
}

/// The `n` of a line; any JSON object with an `n` counts, a line of a kind this version does not know included.
#[derive(Deserialize)]
struct LineNumber {
    n: u32,
}

/// The `n` of the last line in `buf` that has one.
fn last_n(buf: &[u8]) -> Option<u32> {
    buf.rsplit(|&b| b == b'\n').find_map(|l| serde_json::from_slice::<LineNumber>(l).ok()).map(|l| l.n)
}

/// `None` when there is no `begin.json` or it does not parse.
pub fn open_unit(dir: &Path, agent: &str) -> Option<Begin> {
    read_begin(dir, agent).ok().flatten()
}

/// `Ok(None)` when there is no `begin.json`; `Err` when it cannot be read or does not parse.
fn read_begin(dir: &Path, agent: &str) -> anyhow::Result<Option<Begin>> {
    match fs::read(begin_path(dir, agent)) {
        Ok(b) => Ok(Some(serde_json::from_slice(&b)?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Path safety: 1–64 of `[A-Za-z0-9_-]`.
pub(crate) fn check_agent(agent: &str) -> anyhow::Result<()> {
    if agent.is_empty() || agent.len() > 64 || !agent.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') {
        bail!("invalid agent name for the work log");
    }
    Ok(())
}

/// Read-write-create options, mode 0600 on unix.
fn private() -> OpenOptions {
    #[cfg_attr(not(unix), allow(unused_mut))]
    let mut o = OpenOptions::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    o
}

/// An exclusive lock on `<dir>/<agent>.lock`, released on drop.
pub struct AgentLock {
    _file: File,
    dir: PathBuf,
    agent: String,
}

pub fn lock(dir: &Path, agent: &str) -> anyhow::Result<AgentLock> {
    check_agent(agent)?;
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let path = dir.join(format!("{agent}.lock"));
    let f = private().read(true).write(true).create(true).truncate(false).open(&path)
        .with_context(|| format!("opening {}", path.display()))?;
    fs4::FileExt::lock(&f).with_context(|| format!("locking {}", path.display()))?;
    Ok(AgentLock { _file: f, dir: dir.to_path_buf(), agent: agent.to_string() })
}

impl AgentLock {
    pub fn tail(&self) -> Vec<(u64, LogLine)> {
        tail(&self.dir, &self.agent)
    }

    pub fn open_unit(&self) -> Option<Begin> {
        open_unit(&self.dir, &self.agent)
    }

    /// `Ok(None)` when there is no `begin.json`; `Err` when it cannot be read or does not parse.
    pub fn read_begin(&self) -> anyhow::Result<Option<Begin>> {
        read_begin(&self.dir, &self.agent)
    }

    /// `None` removes `begin.json`; `Some` writes a temp file and renames it over.
    pub fn write_begin(&self, b: Option<&Begin>) -> anyhow::Result<()> {
        let target = begin_path(&self.dir, &self.agent);
        let Some(b) = b else {
            return match fs::remove_file(&target) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
                _ => Ok(()),
            };
        };
        let tmp = self.dir.join(format!("{}.begin.json.tmp.{}", self.agent, std::process::id()));
        private().write(true).create(true).truncate(true).open(&tmp)?.write_all(&serde_json::to_vec(b)?)?;
        fs::rename(&tmp, &target)?;
        Ok(())
    }

    /// `Some(offset)` of the written line; `None` when the log is full; `Err` for a line over `MAX_LINE_BYTES`.
    pub fn append(&self, line: &LogLine) -> anyhow::Result<Option<u64>> {
        let path = log_path(&self.dir, &self.agent);
        // The final 2 KiB hold the last line's number and show whether the file ends with a newline.
        let (mut len, end, last_byte) = match File::open(&path) {
            Ok(mut f) => {
                let len = f.metadata()?.len();
                let from = len.saturating_sub(2 * MAX_LINE_BYTES as u64);
                let mut buf = Vec::new();
                f.seek(SeekFrom::Start(from))?;
                f.read_to_end(&mut buf)?;
                let last_byte = buf.last().copied();
                if from > 0 {
                    let cut = buf.iter().position(|&b| b == b'\n').map_or(buf.len(), |i| i + 1);
                    buf.drain(..cut);
                }
                (len, buf, last_byte)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (0, Vec::new(), None),
            Err(e) => return Err(e.into()),
        };
        let last = last_n(&end)
            .or_else(|| tail_window(&path).and_then(|(_, b)| last_n(&b)))
            .unwrap_or(0);
        if last >= MAX_LINES {
            return Ok(None);
        }
        let n = last + 1;
        let overflow = n == MAX_LINES;
        let written = if overflow { LogLine { n, ts: line.ts, kind: Kind::Overflow } } else { LogLine { n, ..line.clone() } };
        let mut text = serde_json::to_string(&written)?;
        text.push('\n');
        if text.len() > MAX_LINE_BYTES {
            bail!("work log line over 1 KiB");
        }
        let mut f = private().append(true).create(true).open(&path)
            .with_context(|| format!("opening {}", path.display()))?;
        if last_byte.is_some_and(|b| b != b'\n') {
            f.write_all(b"\n")?;
            len += 1;
        }
        f.write_all(text.as_bytes())?;
        Ok((!overflow).then_some(len))
    }
}

pub fn append(dir: &Path, agent: &str, line: &LogLine) -> anyhow::Result<()> {
    lock(dir, agent)?.append(line).map(|_| ())
}
