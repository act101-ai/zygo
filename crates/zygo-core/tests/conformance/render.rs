//! The generator (K§6.2.6) and check item 10 (K§6.2.5): feature matrix, rollup and score in Markdown and JSON.

use std::path::Path;

use serde::{Serialize, Serializer};

use crate::derive::{self, Cell, Counts, Grade, Rollup, Scores, Unmet};
use crate::model::*;

/// The generator's name: the first Markdown line and the JSON `generated_by`.
pub const GENERATED_BY: &str = "crates/zygo-core/tests/conformance.rs (ZYGO_WRITE_CONFORMANCE=1); do not edit: a hand edit fails the check";

const TIERS: [(Tier, &str); 3] = [(Tier::Core, "core"), (Tier::Standard, "standard"), (Tier::Extra, "extra")];

/// Each client with its grade file (the first with its id), in `clients` order; a client with no grade file is left out.
fn graded<'a>(grades: &'a [GradeFile], clients: &'a [Client]) -> Vec<(&'a Client, &'a GradeFile)> {
    clients.iter().filter_map(|c| grades.iter().find(|g| g.client == c.id).map(|g| (c, g))).collect()
}

/// The capabilities in tier order (core, standard, extra), registry order inside a tier.
fn ordered(reg: &Registry) -> Vec<(&Capability, &'static str)> {
    TIERS.iter().flat_map(|&(t, word)| reg.capability.iter().filter(move |c| c.tier == t).map(move |c| (c, word))).collect()
}

fn url_of<'a>(g: &'a GradeFile, capability: &str) -> &'a str {
    g.cell.iter().find(|c| c.capability == capability).map_or("", |c| c.url.as_str())
}

fn grade_word(g: Grade) -> &'static str {
    match g {
        Grade::Full => "full",
        Grade::Partial => "partial",
        Grade::None => "none",
        Grade::Na => "n/a",
    }
}

fn reason(u: &Unmet) -> &str {
    u.reason.as_deref().unwrap_or("no grade")
}

/// An input text as Markdown source that shows it literally: each ASCII punctuation character is backslash-escaped
/// (CommonMark allows it for all of them) and nothing else changes. In a table GFM first turns `\|` into `|`, so a
/// cell keeps one column. The one rule for each text the Markdown takes from input, in and out of tables.
fn md_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii_punctuation() {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// A link destination: each UTF-8 byte not in `A-Za-z0-9-._~:/?#@!$&'*+,;=%` as `%XX`. Left out are, among others,
/// `(`, `)`, `[`, `]`, `<`, `>`, `\`, `|`, space and backtick, so the destination cannot end early or split a cell.
fn link_dest(url: &str) -> String {
    let keep = |b: u8| b.is_ascii_alphanumeric() || b"-._~:/?#@!$&'*+,;=%".contains(&b);
    url.bytes().map(|b| if keep(b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// The cell text of K§6.2.6 item 1, ready for its table cell; the URL is a link `[md_text(url)](link_dest(url))`.
fn cell_text(cell: &Cell, url: &str) -> String {
    let join = |parts: Vec<String>| parts.join("; ");
    let head = match cell.grade {
        Grade::Full => "full".to_string(),
        Grade::Partial => format!("partial (missing: {})", join(cell.missing.iter().map(|u| format!("{} {}", md_text(&u.criterion), md_text(&u.text))).collect())),
        Grade::None => format!("none ({})", join(cell.missing.iter().map(|u| format!("{}: {}", md_text(&u.criterion), md_text(reason(u)))).collect())),
        Grade::Na => return format!("n/a ({})", join(cell.na.iter().map(|u| md_text(reason(u))).collect())),
    };
    if url.is_empty() { head } else { format!("{head} [{}]({})", md_text(url), link_dest(url)) }
}

/// `unix` for linux and macos; `None` (no limit) for all three or none; else the names in the given order.
pub fn platform_text(platforms: &[String]) -> Option<String> {
    match platforms {
        [] => None,
        [a, b] if a == "linux" && b == "macos" => Some("unix".into()),
        [a, b, c] if a == "linux" && b == "macos" && c == "windows" => None,
        _ => Some(platforms.join(", ")),
    }
}

/// `s` as one CommonMark code span: a fence one backtick longer than the longest backtick run in `s`, with a
/// space inside the fence when `s` begins or ends with a backtick.
fn code_span(s: &str) -> String {
    let longest = s.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat(longest + 1);
    let pad = if s.starts_with('`') || s.ends_with('`') { " " } else { "" };
    format!("{fence}{pad}{s}{pad}{fence}")
}

fn percent(x: Option<u32>) -> String {
    x.map_or("n/a".into(), |n| format!("{n}%"))
}

fn table(header: &[String], rows: Vec<Vec<String>>) -> String {
    let line = |cells: &[String]| format!("| {} |", cells.join(" | "));
    let mut out = vec![line(header), format!("|{}", "---|".repeat(header.len()))];
    out.extend(rows.iter().map(|r| line(r)));
    out.join("\n")
}

fn counts_row(word: &str, n: &Counts) -> Vec<String> {
    vec![word.into(), n.full.to_string(), n.partial.to_string(), n.none.to_string(), n.na.to_string(), format!("{} / {}", n.met, n.applicable)]
}

fn rollup_block(reg: &Registry, c: &Client, g: &GradeFile) -> String {
    let r = derive::rollup(reg, g, c.supported);
    let header: Vec<String> = ["Tier", "full", "partial", "none", "n/a", "met / applicable"].map(String::from).to_vec();
    let rows = [("core", &r.core), ("standard", &r.standard), ("extra", &r.extra), ("total", &r.total)].map(|(w, n)| counts_row(w, n)).to_vec();
    let bar = if r.bar.meets { "meets bar".to_string() } else { format!("below bar: {}", r.bar.failing.iter().map(|id| md_text(id)).collect::<Vec<_>>().join(", ")) };
    let exceptions = if r.bar.exceptions.is_empty() {
        "none".to_string()
    } else {
        let text = |id: &String| {
            let e = reg.bar_exception.iter().find(|b| b.client == g.client && &b.criterion == id && !blank(&b.approved));
            let approved = |e: &BarException| md_text(e.approved.as_deref().unwrap_or_default());
            e.map_or(md_text(id), |e| format!("{}: {} (approved: {})", md_text(id), md_text(&e.reason), approved(e)))
        };
        r.bar.exceptions.iter().map(text).collect::<Vec<_>>().join("; ")
    };
    format!(
        "### {} {}\n\n{}\n\n- Bar: {bar}\n- Bar exceptions: {exceptions}\n- Release: {}",
        md_text(&c.name),
        md_text(&g.version),
        table(&header, rows),
        r.release
    )
}

fn criteria_block(reg: &Registry, c: &Client, g: &GradeFile, res: &Resolved) -> String {
    let mut lines = vec![format!("### {}", md_text(&c.name)), String::new()];
    for (cap, _) in ordered(reg) {
        let cell = derive::cell(reg, g, &cap.id);
        for cr in &cap.criterion {
            let line = if let Some(u) = cell.missing.iter().find(|u| u.criterion == cr.id) {
                format!("- {} not met: {}", md_text(&cr.id), md_text(reason(u)))
            } else if let Some(u) = cell.na.iter().find(|u| u.criterion == cr.id) {
                format!("- {} n/a: {}", md_text(&cr.id), md_text(reason(u)))
            } else {
                let items: Vec<String> = res
                    .get(&g.client, &cr.id)
                    .map(|rg| rg.evidence.iter().filter_map(|e| e.result.as_ref().ok().map(|ev| (e, ev))).collect::<Vec<_>>())
                    .unwrap_or_default()
                    .into_iter()
                    .map(|(e, ev)| match platform_text(&ev.platforms) {
                        Some(p) => format!("{} ({p})", code_span(&e.text)),
                        None => code_span(&e.text),
                    })
                    .collect();
                let text = if items.is_empty() { "no evidence that counts".to_string() } else { items.join(", ") };
                format!("- {} met: {text}", md_text(&cr.id))
            };
            lines.push(line);
        }
    }
    lines.join("\n")
}

/// The Markdown document of K§6.2.6: feature matrix, rollup, score, then the criteria list.
pub fn markdown(reg: &Registry, grades: &[GradeFile], clients: &[Client], res: &Resolved) -> String {
    let gc = graded(grades, clients);
    let caps = ordered(reg);
    let mut blocks = vec![format!("Generated by {GENERATED_BY}."), "# Conformance".into(), "## Feature matrix".into()];
    let mut header: Vec<String> = vec!["Capability".into(), "Tier".into()];
    header.extend(gc.iter().map(|(c, _)| md_text(&c.name)));
    let rows = caps
        .iter()
        .map(|(cap, word)| {
            let mut r = vec![md_text(&format!("{} {}", cap.id, cap.name)), word.to_string()];
            r.extend(gc.iter().map(|(_, g)| cell_text(&derive::cell(reg, g, &cap.id), url_of(g, &cap.id))));
            r
        })
        .collect();
    blocks.push(table(&header, rows));
    blocks.push("## Rollup".into());
    blocks.extend(gc.iter().map(|(c, g)| rollup_block(reg, c, g)));
    blocks.push("## Score".into());
    let header = ["Client", "Total", "Core", "Standard", "Extra"].map(String::from).to_vec();
    let rows = gc
        .iter()
        .map(|(c, g)| {
            let s = derive::scores(reg, g);
            vec![md_text(&c.name), percent(s.total), percent(s.core), percent(s.standard), percent(s.extra)]
        })
        .collect();
    blocks.push(table(&header, rows));
    blocks.push("## Criteria".into());
    blocks.extend(gc.iter().map(|(c, g)| criteria_block(reg, c, g, res)));
    blocks.join("\n\n") + "\n"
}

#[derive(Serialize)]
struct JDoc<'a> {
    generated_by: &'a str,
    weights: JWeights,
    clients: Vec<JClient<'a>>,
}

#[derive(Serialize)]
struct JWeights {
    core: u32,
    standard: u32,
    extra: u32,
}

#[derive(Serialize)]
struct JClient<'a> {
    id: &'a str,
    name: &'a str,
    version: &'a str,
    supported: bool,
    matrix: JMatrix<'a>,
    rollup: JRollup,
    score: JScore,
}

/// The matrix map, written in capability order without `serde_json::Map`.
struct JMatrix<'a>(Vec<(&'a str, JCell<'a>)>);

impl Serialize for JMatrix<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_map(self.0.iter().map(|(k, v)| (k, v)))
    }
}

#[derive(Serialize)]
struct JCell<'a> {
    tier: &'static str,
    grade: &'static str,
    value: Option<f64>,
    url: &'a str,
    missing: Vec<JUnmet>,
    na: Vec<JUnmet>,
}

#[derive(Serialize)]
struct JUnmet {
    criterion: String,
    text: String,
    reason: Option<String>,
}

#[derive(Serialize)]
struct JCounts {
    full: u32,
    partial: u32,
    none: u32,
    na: u32,
    met: u32,
    applicable: u32,
}

#[derive(Serialize)]
struct JBar {
    verdict: &'static str,
    failing: Vec<String>,
    exceptions: Vec<String>,
}

#[derive(Serialize)]
struct JRollup {
    core: JCounts,
    standard: JCounts,
    extra: JCounts,
    total: JCounts,
    bar: JBar,
    release: &'static str,
}

#[derive(Serialize)]
struct JScore {
    total: Option<u32>,
    core: Option<u32>,
    standard: Option<u32>,
    extra: Option<u32>,
}

fn j_unmet(v: Vec<Unmet>) -> Vec<JUnmet> {
    v.into_iter().map(|u| JUnmet { criterion: u.criterion, text: u.text, reason: u.reason }).collect()
}

fn j_counts(n: Counts) -> JCounts {
    JCounts { full: n.full, partial: n.partial, none: n.none, na: n.na, met: n.met, applicable: n.applicable }
}

fn j_rollup(r: Rollup) -> JRollup {
    let verdict = if r.bar.meets { "meets bar" } else { "below bar" };
    JRollup {
        core: j_counts(r.core),
        standard: j_counts(r.standard),
        extra: j_counts(r.extra),
        total: j_counts(r.total),
        bar: JBar { verdict, failing: r.bar.failing, exceptions: r.bar.exceptions },
        release: r.release,
    }
}

fn j_score(s: Scores) -> JScore {
    JScore { total: s.total, core: s.core, standard: s.standard, extra: s.extra }
}

/// The JSON document of K§6.2.6, with a fixed key order and a final `\n`.
pub fn json(reg: &Registry, grades: &[GradeFile], clients: &[Client], _res: &Resolved) -> String {
    let caps = ordered(reg);
    let clients = graded(grades, clients)
        .into_iter()
        .map(|(c, g)| {
            let matrix = caps
                .iter()
                .map(|(cap, tier)| {
                    let cell = derive::cell(reg, g, &cap.id);
                    let jc = JCell { tier, grade: grade_word(cell.grade), value: cell.value, url: url_of(g, &cap.id), missing: j_unmet(cell.missing), na: j_unmet(cell.na) };
                    (cap.id.as_str(), jc)
                })
                .collect();
            JClient {
                id: &c.id,
                name: &c.name,
                version: &g.version,
                supported: c.supported,
                matrix: JMatrix(matrix),
                rollup: j_rollup(derive::rollup(reg, g, c.supported)),
                score: j_score(derive::scores(reg, g)),
            }
        })
        .collect();
    let doc = JDoc { generated_by: GENERATED_BY, weights: JWeights { core: derive::WEIGHTS[0], standard: derive::WEIGHTS[1], extra: derive::WEIGHTS[2] }, clients };
    serde_json::to_string_pretty(&doc).expect("the conformance JSON serializes") + "\n"
}

/// Check item 10: write both files when `ZYGO_WRITE_CONFORMANCE=1`, else compare them with the generated text.
pub fn compare_or_write(root: &Path, md: &str, json: &str) -> Result<(), Fault> {
    let write = std::env::var_os("ZYGO_WRITE_CONFORMANCE").is_some_and(|v| v == "1");
    compare_or_write_mode(root, md, json, write)
}

/// Check item 10 with the mode given: `root/docs/conformance.md` and `.json` are written (`write`) or compared
/// with `md` and `json` after CR LF → LF; any problem is one fault of item 10.
pub fn compare_or_write_mode(root: &Path, md: &str, json: &str, write: bool) -> Result<(), Fault> {
    let files = [(root.join("docs/conformance.md"), md), (root.join("docs/conformance.json"), json)];
    let fault = |problems: Vec<String>| if problems.is_empty() { Ok(()) } else { Err(Fault { item: 10, text: problems.join("; ") }) };
    if write {
        let docs = root.join("docs");
        std::fs::create_dir_all(&docs).map_err(|e| Fault { item: 10, text: format!("{}: {e}", docs.display()) })?;
        let problems = files.iter().filter_map(|(p, text)| std::fs::write(p, text).err().map(|e| format!("{}: {e}", p.display()))).collect();
        return fault(problems);
    }
    let lf = |s: &str| s.replace("\r\n", "\n");
    let problems = files
        .iter()
        .filter_map(|(p, text)| {
            let have = std::fs::read(p).map_err(|e| e.to_string()).and_then(|b| String::from_utf8(b).map_err(|e| e.to_string()));
            match have {
                Err(e) => Some(format!("{}: {e}", p.display())),
                Ok(h) if lf(&h) != lf(text) => Some(format!("{}: differs from the generated text (run with ZYGO_WRITE_CONFORMANCE=1)", p.display())),
                Ok(_) => None,
            }
        })
        .collect();
    fault(problems)
}
