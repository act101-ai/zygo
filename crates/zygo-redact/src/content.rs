//! The content (profanity) pass (§16.2): user-typed text only; fails open at its time budget.

use crate::cache::{self, CacheKey, Entry, SessionCache};
use crate::harness;
use crate::patterns::Compiled;
use rustrict::{Censor, Trie, Type};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::cell::Cell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Range;
use std::time::{Duration, Instant};
use zygo_core::Agent;

/// The classifier the cache keys on; a test checks it against `Cargo.lock`.
pub const CLASSIFIER: &str = "rustrict 0.7.39";
/// Finding ids; cache entries index into this table. A `deny` hit counts as `content_profane`.
pub const IDS: [&str; 3] = ["content_profane", "content_offensive", "content_sexual"];
pub const TIMEOUT_ID: &str = "content_timeout";
/// Deadline checks: every 256 classifications and every 64 KiB scanned or written.
const CHECK_EVERY: u64 = 256;
const CHECK_BYTES: u64 = 64 << 10;
/// A whitespace-free run longer than this is not tokenized.
const MAX_RUN: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// Messages and chat/completions: `messages[*]` with `"role": "user"`.
    Messages,
    /// Responses: `input`.
    Input,
}

/// Body layout, and the agent (OpenCode `<file>`/`<shell>` spans are skipped for OpenCode only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shape {
    pub layout: Layout,
    pub agent: Agent,
}

/// Finding ids and counts; never a word.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ContentFindings {
    pub counts: BTreeMap<String, u64>,
    /// True only when some string changed; when false, forward the original bytes (N12).
    pub changed: bool,
}

thread_local! {
    static BUILTIN_CALLS: Cell<u64> = const { Cell::new(0) };
    #[cfg(feature = "test-hooks")]
    static LOOKUP_DELAY: Cell<Duration> = const { Cell::new(Duration::ZERO) };
}

/// Test hook (feature `test-hooks`): lengthens the cache-lookup phase of later calls on this thread (stands in for a
/// large cached conversation).
#[cfg(feature = "test-hooks")]
#[doc(hidden)]
pub fn set_lookup_delay(d: Duration) {
    LOOKUP_DELAY.with(|c| c.set(d));
}

/// Test hook: built-in classifier calls made on this thread.
#[doc(hidden)]
pub fn builtin_classifier_calls() -> u64 {
    BUILTIN_CALLS.with(Cell::get)
}

/// The gate: `x-claude-code-request-class` is `main`, `compaction`, `auxiliary` or absent.
pub fn admits(request_class: Option<&str>) -> bool {
    matches!(request_class, None | Some("main" | "compaction" | "auxiliary"))
}

/// Replaces flagged words in user-typed text of `body` in place. `budget` covers strings this
/// session has not filtered; its clock starts after the cache lookup. On timeout the rest go out unchanged and uncached (`content_timeout`).
/// A panic propagates; the caller refuses with 503 `zygo_redaction_unavailable`.
pub fn filter(body: &mut Value, shape: Shape, c: &Compiled, cache: &mut SessionCache, budget: Duration) -> ContentFindings {
    pass(body, shape, c, cache, budget, None)
}

/// Test hook: `filter` with `trie` in place of the classifier's built-in word list.
#[doc(hidden)]
pub fn filter_with_trie(body: &mut Value, shape: Shape, c: &Compiled, cache: &mut SessionCache, budget: Duration, trie: &'static Trie) -> ContentFindings {
    pass(body, shape, c, cache, budget, Some(trie))
}

/// How a user-text string is filtered.
enum Ctx {
    Plain,
    /// The block directly after a `<command-message>` block, when it is a text block that is not
    /// a harness block and not itself a `<command-message>` block: a token is tested only when it
    /// equals, case-insensitive, a token of these typed arguments.
    Expanded(String),
}

fn pass(body: &mut Value, shape: Shape, c: &Compiled, cache: &mut SessionCache, budget: Duration, trie: Option<&'static Trie>) -> ContentFindings {
    let base = version(c, shape.agent, trie);
    let texts = user_texts(body, shape.layout);
    let keys: Vec<CacheKey> = texts.iter().map(|(s, ctx)| key(&base, ctx, s)).collect();
    // One lock; hits are copies. The budget starts here, so cached strings do not spend it.
    let found = cache.get_many(&keys);
    #[cfg(feature = "test-hooks")]
    {
        let delay = LOOKUP_DELAY.with(Cell::get);
        if !delay.is_zero() {
            std::thread::sleep(delay);
        }
    }
    let mut cx = Cx {
        trie,
        deadline: Instant::now() + budget,
        deny: c.content.deny.iter().map(String::as_str).collect(),
        allow: c.content.allow.iter().map(String::as_str).collect(),
        repl: &c.content.replacement,
        verdicts: HashMap::new(),
        only: None,
        classified: 0,
        bytes: 0,
        timed_out: false,
    };
    let mut f = ContentFindings::default();
    // Entries filtered earlier in this pass, so a string repeated in one body is filtered once.
    let mut computed: HashMap<CacheKey, Entry> = HashMap::new();
    for ((s, ctx), k) in texts.into_iter().zip(keys) {
        let e = match found.get(&k).or_else(|| computed.get(&k)) {
            Some(e) => e.clone(),
            None if cx.timed_out => continue,
            None => {
                cache.record_scan();
                let Some(e) = cx.string(s, &ctx, shape.agent) else { continue };
                cache.insert(k, e.clone());
                computed.insert(k, e.clone());
                e
            }
        };
        for &(i, n) in &e.findings {
            *f.counts.entry(IDS[usize::from(i)].to_string()).or_default() += u64::from(n);
        }
        if let Some(r) = e.replacement {
            f.changed |= *s != r;
            *s = r;
        }
    }
    if cx.timed_out {
        f.counts.insert(TIMEOUT_ID.to_string(), 1);
    }
    f
}

/// Classifier, thresholds, agent, replacement and lists; the trie hook by address (in-memory cache only).
fn version(c: &Compiled, agent: Agent, trie: Option<&'static Trie>) -> [u8; 16] {
    let mut h = Sha256::new();
    h.update(b"zygo-content-v1\n");
    let addr = trie.map_or(0usize, |t| t as *const Trie as usize).to_le_bytes();
    let mut parts: Vec<&[u8]> = vec![CLASSIFIER.as_bytes(), agent.as_str().as_bytes(), c.content.replacement.as_bytes(), &addr, b"allow"];
    parts.extend(c.content.allow.iter().map(String::as_bytes));
    parts.push(b"deny");
    parts.extend(c.content.deny.iter().map(String::as_bytes));
    for x in parts {
        h.update((x.len() as u64).to_le_bytes());
        h.update(x);
    }
    let mut v = [0u8; 16];
    v.copy_from_slice(&h.finalize()[..16]);
    v
}

fn key(base: &[u8; 16], ctx: &Ctx, s: &str) -> CacheKey {
    match ctx {
        Ctx::Plain => cache::key(base, s),
        Ctx::Expanded(args) => {
            let mut h = Sha256::new();
            h.update(base);
            h.update(b"expanded");
            h.update((args.len() as u64).to_le_bytes());
            h.update(args.as_bytes());
            let mut v = [0u8; 16];
            v.copy_from_slice(&h.finalize()[..16]);
            cache::key(&v, s)
        }
    }
}

/// The user-text strings of §16.2, in body order; harness blocks are left out.
fn user_texts(body: &mut Value, layout: Layout) -> Vec<(&mut String, Ctx)> {
    let mut out = Vec::new();
    match layout {
        Layout::Messages => {
            for m in body.get_mut("messages").and_then(Value::as_array_mut).into_iter().flatten() {
                if m["role"] == "user" {
                    push_content(m.get_mut("content"), "text", &mut out);
                }
            }
        }
        Layout::Input => match body.get_mut("input") {
            Some(Value::String(s)) if !harness::is_harness(s) => out.push((s, Ctx::Plain)),
            Some(Value::Array(items)) => {
                for it in items {
                    let message = it.get("type").is_none() || it["type"] == "message";
                    if message && it["role"] == "user" {
                        push_content(it.get_mut("content"), "input_text", &mut out);
                    }
                }
            }
            _ => {}
        },
    }
    out
}

/// The expanded command file is the block directly after `<command-message>`, when that block is
/// a text block, not a harness block and not a `<command-message>` block (§16.2); otherwise no
/// block is. A `<command-message>` block is always plain text.
fn push_content<'a>(content: Option<&'a mut Value>, kind: &str, out: &mut Vec<(&'a mut String, Ctx)>) {
    match content {
        Some(Value::String(s)) if !harness::is_harness(s) => out.push((s, Ctx::Plain)),
        Some(Value::Array(blocks)) => {
            let mut pending: Option<String> = None;
            for b in blocks {
                let after_command = pending.take();
                if b["type"] != kind {
                    continue;
                }
                let Some(Value::String(s)) = b.get_mut("text") else { continue };
                if harness::is_harness(s) {
                    continue;
                }
                pending = harness::command_args(s).map(str::to_owned);
                let ctx = if pending.is_some() { Ctx::Plain } else { after_command.map_or(Ctx::Plain, Ctx::Expanded) };
                out.push((s, ctx));
            }
        }
        _ => {}
    }
}

struct Cx<'a> {
    trie: Option<&'static Trie>,
    deadline: Instant,
    deny: HashSet<&'a str>,
    allow: HashSet<&'a str>,
    repl: &'a str,
    /// Per-request verdict cache: lower-case token → finding index.
    verdicts: HashMap<String, Option<usize>>,
    /// In an expanded command file: the lower-case tokens of the typed arguments; only these are tested.
    only: Option<HashSet<String>>,
    classified: u64,
    bytes: u64,
    timed_out: bool,
}

impl Cx<'_> {
    /// One string's entry; `None` when the deadline passed while filtering it.
    fn string(&mut self, s: &str, ctx: &Ctx, agent: Agent) -> Option<Entry> {
        self.only = match ctx {
            Ctx::Expanded(args) => {
                self.count(args.len())?;
                Some(pieces(args).filter(|&(w, _)| w).map(|(_, t)| t.to_ascii_lowercase()).collect())
            }
            Ctx::Plain => None,
        };
        let ranges = outside(s.len(), &harness::skipped_spans(s, agent == Agent::OpenCode));
        let (mut out, mut last, mut counts, mut changed) = (String::with_capacity(s.len()), 0, [0u32; 3], false);
        for r in ranges {
            out.push_str(&s[last..r.start]);
            changed |= self.text(s, r.clone(), &mut out, &mut counts)?;
            last = r.end;
        }
        out.push_str(&s[last..]);
        let findings = (0..3u16).filter(|&i| counts[usize::from(i)] > 0).map(|i| (i, counts[usize::from(i)])).collect();
        Some(Entry { replacement: changed.then_some(out), findings })
    }

    /// Appends `s[r]` to `out` with flagged tokens replaced; true when one was. Whitespace-free
    /// runs are measured over the whole of `s`, also across the edges of `r`.
    fn text(&mut self, s: &str, r: Range<usize>, out: &mut String, counts: &mut [u32; 3]) -> Option<bool> {
        let mut changed = false;
        let mut i = r.start;
        while i < r.end {
            let start = i + s[i..r.end].find(|c: char| !c.is_whitespace()).unwrap_or(r.end - i);
            let end = start + s[start..r.end].find(char::is_whitespace).unwrap_or(r.end - start);
            self.count(end - i)?;
            out.push_str(&s[i..start]);
            let run = &s[start..end];
            let left = if start == r.start { edge(s[..start].chars().rev()) } else { 0 };
            let right = if end == r.end { edge(s[end..].chars()) } else { 0 };
            if run.len() + left + right > MAX_RUN {
                out.push_str(run);
            } else {
                changed |= self.run(run, out, counts)?;
            }
            i = end;
        }
        Some(changed)
    }

    /// Tokens of `[A-Za-z0-9_]` in one whitespace-free run of at most 64 bytes.
    fn run(&mut self, run: &str, out: &mut String, counts: &mut [u32; 3]) -> Option<bool> {
        let mut changed = false;
        for (is_word, tok) in pieces(run) {
            match if is_word { self.verdict(tok)? } else { None } {
                Some(cat) => {
                    counts[cat] += 1;
                    let r = self.repl;
                    out.push_str(r);
                    self.count(r.len())?;
                    changed = true;
                }
                None => out.push_str(tok),
            }
        }
        Some(changed)
    }

    /// `deny` (flag) → `allow` (pass) → per-request verdict → classifier; tokens of 3–40 letters only.
    fn verdict(&mut self, tok: &str) -> Option<Option<usize>> {
        if !(3..=40).contains(&tok.len()) || !tok.bytes().all(|c| c.is_ascii_alphabetic()) {
            return Some(None);
        }
        let w = tok.to_ascii_lowercase();
        if self.only.as_ref().is_some_and(|o| !o.contains(&w)) {
            return Some(None);
        }
        if self.deny.contains(w.as_str()) {
            return Some(Some(0));
        }
        if self.allow.contains(w.as_str()) {
            return Some(None);
        }
        if let Some(v) = self.verdicts.get(&w) {
            return Some(*v);
        }
        if self.trie.is_none() {
            BUILTIN_CALLS.with(|n| n.set(n.get() + 1));
        }
        let v = classify(&w, self.trie);
        self.verdicts.insert(w, v);
        self.classified += 1;
        if self.classified.is_multiple_of(CHECK_EVERY) {
            self.check()?;
        }
        Some(v)
    }

    fn count(&mut self, n: usize) -> Option<()> {
        let before = self.bytes / CHECK_BYTES;
        self.bytes += n as u64;
        if self.bytes / CHECK_BYTES != before {
            self.check()?;
        }
        Some(())
    }

    fn check(&mut self) -> Option<()> {
        if Instant::now() >= self.deadline {
            self.timed_out = true;
            return None;
        }
        Some(())
    }
}

/// `s` split into alternating runs of `[A-Za-z0-9_]` (true) and of other characters (false).
fn pieces(s: &str) -> impl Iterator<Item = (bool, &str)> {
    let word = |c: &u8| c.is_ascii_alphanumeric() || *c == b'_';
    let b = s.as_bytes();
    let mut i = 0;
    std::iter::from_fn(move || {
        let is_word = word(b.get(i)?);
        let j = i + b[i..].iter().position(|c| word(c) != is_word).unwrap_or(b.len() - i);
        let p = (is_word, &s[i..j]);
        i = j;
        Some(p)
    })
}

/// Profane or offensive at moderate or higher, sexual at severe; a 3-letter word only at severe.
/// Flagged only when the censored form covers the whole word. Returns the index into `IDS`.
fn classify(w: &str, trie: Option<&'static Trie>) -> Option<usize> {
    let thr = if w.len() == 3 {
        (Type::PROFANE | Type::OFFENSIVE | Type::SEXUAL) & Type::SEVERE
    } else {
        ((Type::PROFANE | Type::OFFENSIVE) & Type::MODERATE_OR_HIGHER) | (Type::SEXUAL & Type::SEVERE)
    };
    let mut c = Censor::from_str(w);
    if let Some(t) = trie {
        c.with_trie(t);
    }
    c.with_censor_threshold(thr).with_censor_first_character_threshold(thr);
    let (censored, a) = c.censor_and_analyze();
    if !a.is(thr) || censored.is_empty() || !censored.bytes().all(|b| b == b'*') {
        return None;
    }
    [Type::PROFANE, Type::OFFENSIVE, Type::SEXUAL].iter().position(|&t| a.is(t & thr))
}

/// Bytes of non-whitespace next to a range edge, counted up to `MAX_RUN + 1`.
fn edge(chars: impl Iterator<Item = char>) -> usize {
    chars.take(MAX_RUN + 1).take_while(|c| !c.is_whitespace()).map(char::len_utf8).sum()
}

/// The ranges of `0..len` outside `spans` (sorted, disjoint).
fn outside(len: usize, spans: &[Range<usize>]) -> Vec<Range<usize>> {
    let mut out = Vec::with_capacity(spans.len() + 1);
    let mut last = 0;
    for r in spans {
        out.push(last..r.start);
        last = r.end;
    }
    out.push(last..len);
    out
}
