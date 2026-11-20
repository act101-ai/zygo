use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::kind::{self, Kind};
use crate::view::{cut, LINE_CAP_CHARS};

#[derive(Serialize, Deserialize)]
struct Meta {
    tool: String,
    bytes: u64,
    lines: u64,
    kind: Kind,
    created_at: u64,
}

fn create_private(path: &Path) -> std::io::Result<std::fs::File> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    o.open(path)
}

/// Creates `dir` and its missing parents; a new `dir` gets mode 0700 on unix. An existing directory is left as it is.
fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    let mut b = std::fs::DirBuilder::new();
    b.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        b.mode(0o700);
    }
    b.create(dir)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stored {
    pub id: String,
    pub tool: String,
    pub bytes: u64,
    pub lines: u64,
    pub kind: Kind,
    pub created_at: u64,
    pub data: Vec<u8>,
}

pub struct Store;

impl Store {
    pub fn put(dir: &Path, tool: &str, bytes: &[u8]) -> anyhow::Result<Stored> {
        create_private_dir(dir).with_context(|| format!("cannot create {}", dir.display()))?;
        let id = ulid::Ulid::generate().to_string();
        let meta = Meta {
            tool: tool.to_string(),
            bytes: bytes.len() as u64,
            lines: String::from_utf8_lossy(bytes).lines().count() as u64,
            kind: kind::sniff(bytes),
            created_at: SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
        };
        let raw = dir.join(&id);
        let tmp = dir.join(format!("{id}.tmp"));
        let mut f = create_private(&raw).context("cannot write the result")?;
        let written = f
            .write_all(bytes)
            .context("cannot write the result")
            .and_then(|()| create_private(&tmp).and_then(|mut f| f.write_all(&serde_json::to_vec(&meta)?)).context("cannot write the result meta"))
            .and_then(|()| std::fs::rename(&tmp, dir.join(format!("{id}.json"))).context("cannot finish the result meta"));
        if let Err(e) = written {
            let _ = std::fs::remove_file(&tmp);
            let _ = std::fs::remove_file(&raw);
            return Err(e);
        }
        Ok(Stored { id, tool: meta.tool, bytes: meta.bytes, lines: meta.lines, kind: meta.kind, created_at: meta.created_at, data: bytes.to_vec() })
    }

    pub fn get(dir: &Path, id: &str) -> anyhow::Result<Stored> {
        anyhow::ensure!(is_id(id), "not a result id: {id}");
        let meta: Meta = std::fs::read(dir.join(format!("{id}.json")))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .ok_or_else(|| anyhow::anyhow!("no result with id {id}"))?;
        let data = std::fs::read(dir.join(id)).ok().filter(|d| d.len() as u64 == meta.bytes).ok_or_else(|| anyhow::anyhow!("no result with id {id}"))?;
        Ok(Stored { id: id.to_string(), tool: meta.tool, bytes: meta.bytes, lines: meta.lines, kind: meta.kind, created_at: meta.created_at, data })
    }
}

pub fn is_id(s: &str) -> bool {
    s.len() == 26 && s.chars().all(|c| c.is_ascii_digit() || (c.is_ascii_uppercase() && !"ILOU".contains(c))) && ulid::Ulid::from_string(s).is_ok()
}

pub const MAX_RANGE_LINES: u64 = 400;
pub const MAX_MATCHES: usize = 50;

impl Stored {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.data).into_owned()
    }
    pub fn range(&self, first: u64, last: u64) -> Result<String, String> {
        if first == 0 || last < first {
            return Err("line numbers start at 1 and last must not be below first".to_string());
        }
        if last - first + 1 > MAX_RANGE_LINES {
            return Err(format!("at most {MAX_RANGE_LINES} lines per call"));
        }
        let text = self.text();
        let lines: Vec<&str> = text.lines().collect();
        let n = lines.len() as u64;
        if first > n {
            return Ok(format!("no lines in that range; the result has {n} lines"));
        }
        Ok(lines[(first - 1) as usize..last.min(n) as usize].join("\n"))
    }

    pub fn search(&self, pattern: &str) -> Result<String, String> {
        let re = regex::Regex::new(pattern).map_err(|e| e.to_string())?;
        let text = self.text();
        let lines: Vec<&str> = text.lines().collect();
        let mut hits: Vec<usize> = Vec::new();
        let mut more = false;
        for (i, l) in lines.iter().enumerate() {
            if re.is_match(l) {
                if hits.len() == MAX_MATCHES {
                    more = true;
                    break;
                }
                hits.push(i);
            }
        }
        if hits.is_empty() {
            return Ok("no match".to_string());
        }
        let shown: std::collections::BTreeSet<usize> = hits.iter().flat_map(|&h| h.saturating_sub(1)..(h + 2).min(lines.len())).collect();
        let mut out: Vec<String> = Vec::new();
        let mut prev: Option<usize> = None;
        for &i in &shown {
            if prev.is_some_and(|p| i > p + 1) {
                out.push("--".to_string());
            }
            if hits.binary_search(&i).is_ok() {
                out.push(format!("{}: {}", i + 1, window(lines[i], &re)));
            } else {
                out.push(format!("{}- {}", i + 1, cut(lines[i])));
            }
            prev = Some(i);
        }
        if more {
            out.push(format!("[more matches not shown; the limit is {MAX_MATCHES}]"));
        }
        Ok(out.join("\n"))
    }
}

fn window(line: &str, re: &regex::Regex) -> String {
    let len = line.chars().count();
    if len <= LINE_CAP_CHARS {
        return line.to_string();
    }
    let at = re.find(line).map_or(0, |m| line[..m.start()].chars().count());
    let start = at.min(len - LINE_CAP_CHARS);
    let body: String = line.chars().skip(start).take(LINE_CAP_CHARS).collect();
    format!("{}{body}{}", if start > 0 { "…" } else { "" }, if start + LINE_CAP_CHARS < len { "…" } else { "" })
}
