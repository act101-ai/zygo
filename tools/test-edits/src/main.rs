//! `zygo-test-edits`: the per-node test-edit check of K-C7 (amended), run by
//! `scripts/check-test-edits.sh <parent> <node file>`.
//!
//! Each test file under `crates/<c>/tests/` and each `#[cfg(test)]` item of `crates/<c>/src/`
//! that exists at `<parent>` is a unit. Against its match at `HEAD`, the check reports each
//! removed or renamed test function, each added `#[ignore]` and each change of the multiset of
//! literals, unless the node file lists that exact change under `## Test logic edits`.
//! Exit 0: the check passes; 1: offences, one per line on stdout; 2: usage error.

use proc_macro2::{Delimiter, TokenStream, TokenTree};
use std::collections::{BTreeMap, BTreeSet};
use std::process::{Command, ExitCode};
use syn::parse::{ParseStream, Parser};
use syn::punctuated::Punctuated;
use syn::{Attribute, Item, Meta, Token};

/// One unit: a test file (`path` is empty) or a `#[cfg(test)]` item of a `src` file.
#[derive(Debug, Clone, PartialEq)]
struct Unit {
    file: String,
    path: String,
    /// Test-function id (module path inside the unit) → ignored.
    tests: BTreeMap<String, bool>,
    /// Literal source text → count.
    literals: BTreeMap<String, usize>,
    /// An out-of-line `#[cfg(test)] mod x;` whose body is not read.
    outline: bool,
}

/// An entry under `## Test logic edits`.
#[derive(Debug, Clone, PartialEq)]
enum Entry {
    Removed { id: String },
    Renamed { old: String, new: String },
    Ignore { id: String },
    Literals { file: String, rest: String },
}

/// True when the `cfg` predicate `meta` requires `test`.
fn cfg_requires_test(meta: &Meta) -> bool {
    match meta {
        Meta::Path(p) => p.is_ident("test"),
        Meta::List(l) => {
            let args = match l.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated) {
                Ok(a) => a,
                Err(_) => return false,
            };
            if l.path.is_ident("all") {
                args.iter().any(cfg_requires_test)
            } else if l.path.is_ident("any") {
                !args.is_empty() && args.iter().all(cfg_requires_test)
            } else {
                false
            }
        }
        Meta::NameValue(_) => false,
    }
}

fn last_segment_is(path: &syn::Path, name: &str) -> bool {
    path.segments.last().is_some_and(|s| s.ident == name)
}

/// True for an attribute whose path's last segment is `test` (`#[test]`, `#[tokio::test(…)]`).
fn is_test_attr(a: &Attribute) -> bool {
    last_segment_is(a.path(), "test")
}

fn meta_ignores(meta: &Meta) -> bool {
    if last_segment_is(meta.path(), "ignore") {
        return true;
    }
    match meta {
        Meta::List(l) if l.path.is_ident("cfg_attr") => {
            match l.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated) {
                Ok(args) => args.iter().skip(1).any(meta_ignores),
                Err(_) => false,
            }
        }
        _ => false,
    }
}

/// True for `#[ignore]`, `#[ignore = "…"]`, or a `cfg_attr` that holds `ignore` at any depth.
fn is_ignore_attr(a: &Attribute) -> bool {
    meta_ignores(&a.meta)
}

fn item_requires_test(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|a| {
        a.path().is_ident("cfg") && a.parse_args::<Meta>().is_ok_and(|m| cfg_requires_test(&m))
    })
}

fn item_attrs(item: &Item) -> &[Attribute] {
    match item {
        Item::Const(i) => &i.attrs,
        Item::Enum(i) => &i.attrs,
        Item::ExternCrate(i) => &i.attrs,
        Item::Fn(i) => &i.attrs,
        Item::ForeignMod(i) => &i.attrs,
        Item::Impl(i) => &i.attrs,
        Item::Macro(i) => &i.attrs,
        Item::Mod(i) => &i.attrs,
        Item::Static(i) => &i.attrs,
        Item::Struct(i) => &i.attrs,
        Item::Trait(i) => &i.attrs,
        Item::TraitAlias(i) => &i.attrs,
        Item::Type(i) => &i.attrs,
        Item::Union(i) => &i.attrs,
        Item::Use(i) => &i.attrs,
        _ => &[],
    }
}

/// The name of an item: its identifier, or `<kind>#<n>` (the n-th unnamed item of that kind at
/// its level) for an item with none.
fn item_name(item: &Item, unnamed: &mut BTreeMap<&'static str, usize>) -> String {
    let ident = match item {
        Item::Const(i) => Some(i.ident.to_string()),
        Item::Enum(i) => Some(i.ident.to_string()),
        Item::ExternCrate(i) => Some(i.ident.to_string()),
        Item::Fn(i) => Some(i.sig.ident.to_string()),
        Item::Macro(i) => i.ident.as_ref().map(|x| x.to_string()),
        Item::Mod(i) => Some(i.ident.to_string()),
        Item::Static(i) => Some(i.ident.to_string()),
        Item::Struct(i) => Some(i.ident.to_string()),
        Item::Trait(i) => Some(i.ident.to_string()),
        Item::TraitAlias(i) => Some(i.ident.to_string()),
        Item::Type(i) => Some(i.ident.to_string()),
        Item::Union(i) => Some(i.ident.to_string()),
        _ => None,
    };
    if let Some(name) = ident {
        return name;
    }
    let kind = match item {
        Item::Impl(_) => "impl",
        Item::Use(_) => "use",
        Item::Macro(_) => "macro",
        Item::ForeignMod(_) => "extern",
        _ => "item",
    };
    let n = unnamed.entry(kind).or_insert(0);
    *n += 1;
    format!("{kind}#{n}")
}

/// Items, each with its own token trees.
type Split = Vec<(Item, Vec<TokenTree>)>;

/// Literal source text → count.
type Multiset = BTreeMap<String, usize>;

/// Splits a token stream into its items.
fn split_items(ts: TokenStream) -> syn::Result<Split> {
    let parser = |input: ParseStream| -> syn::Result<Split> {
        input.call(Attribute::parse_inner)?;
        let mut out = Vec::new();
        while !input.is_empty() {
            let begin = input.cursor();
            let item: Item = input.parse()?;
            let end = input.cursor();
            let mut c = begin;
            let mut toks = Vec::new();
            while c != end {
                match c.token_tree() {
                    Some((tt, next)) => {
                        toks.push(tt);
                        c = next;
                    }
                    None => break,
                }
            }
            out.push((item, toks));
        }
        Ok(out)
    };
    parser.parse2(ts)
}

fn is_doc_group(tt: Option<&TokenTree>) -> bool {
    match tt {
        Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Bracket => {
            matches!(g.stream().into_iter().next(), Some(TokenTree::Ident(i)) if i == "doc")
        }
        _ => false,
    }
}

/// Counts each literal token, also inside groups (macro calls, attribute lists); skips each
/// `doc` attribute (`///`, `//!`, `#[doc = …]`).
fn count_literals(tts: &[TokenTree], out: &mut BTreeMap<String, usize>) {
    let mut i = 0;
    while i < tts.len() {
        if let TokenTree::Punct(p) = &tts[i]
            && p.as_char() == '#'
        {
            let mut j = i + 1;
            if matches!(tts.get(j), Some(TokenTree::Punct(q)) if q.as_char() == '!') {
                j += 1;
            }
            if is_doc_group(tts.get(j)) {
                i = j + 1;
                continue;
            }
        }
        match &tts[i] {
            TokenTree::Literal(l) => *out.entry(l.to_string()).or_insert(0) += 1,
            TokenTree::Group(g) => {
                let inner: Vec<TokenTree> = g.stream().into_iter().collect();
                count_literals(&inner, out);
            }
            _ => {}
        }
        i += 1;
    }
}

/// Collects the test functions of `items` with ids prefixed by `prefix`.
fn collect_tests(items: &[Item], prefix: &str, out: &mut BTreeMap<String, bool>) {
    for item in items {
        match item {
            Item::Fn(f) => {
                if f.attrs.iter().any(is_test_attr) {
                    let ignored = f.attrs.iter().any(is_ignore_attr);
                    out.insert(join(prefix, &f.sig.ident.to_string()), ignored);
                }
            }
            Item::Mod(m) => {
                if let Some((_, inner)) = &m.content {
                    collect_tests(inner, &join(prefix, &m.ident.to_string()), out);
                }
            }
            _ => {}
        }
    }
}

fn join(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{prefix}::{name}")
    }
}

/// True for a path `crates/<c>/tests/…`.
fn is_test_file(file: &str) -> bool {
    let parts: Vec<&str> = file.split('/').collect();
    parts.len() >= 4 && parts[0] == "crates" && parts[2] == "tests"
}

/// The crate folder `crates/<c>` of a path under `crates/`.
fn crate_dir(file: &str) -> Option<String> {
    let parts: Vec<&str> = file.split('/').collect();
    (parts.len() >= 3 && parts[0] == "crates").then(|| format!("crates/{}", parts[1]))
}

/// Resolves `rel` against the folder of `file`, folding `.` and `..`.
fn resolve(file: &str, rel: &str) -> String {
    let mut parts: Vec<&str> = file.split('/').collect();
    parts.pop();
    for seg in rel.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

/// The file that the `#[path = "…"]` attribute of an out-of-line module names, if any.
fn path_attr(file: &str, attrs: &[Attribute]) -> Option<String> {
    attrs.iter().find_map(|a| match &a.meta {
        Meta::NameValue(nv) if nv.path.is_ident("path") => match &nv.value {
            syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(s),
                ..
            }) => Some(resolve(file, &s.value())),
            _ => None,
        },
        _ => None,
    })
}

fn src_units(file: &str, split: Split, prefix: &str, out: &mut Vec<Unit>) -> Result<(), String> {
    let mut unnamed = BTreeMap::new();
    for (item, toks) in split {
        let name = item_name(&item, &mut unnamed);
        let path = join(prefix, &name);
        if item_requires_test(item_attrs(&item)) {
            let mut tests = BTreeMap::new();
            let mut outline = false;
            match &item {
                Item::Fn(f) if f.attrs.iter().any(is_test_attr) => {
                    tests.insert(name.clone(), f.attrs.iter().any(is_ignore_attr));
                }
                Item::Mod(m) => match &m.content {
                    Some((_, inner)) => collect_tests(inner, "", &mut tests),
                    None => {
                        // A `#[path]` into `crates/<c>/tests/` names a test file, which is a
                        // unit of its own; any other out-of-line test module is not read.
                        let in_tests = path_attr(file, &m.attrs).is_some_and(|p| is_test_file(&p));
                        if in_tests {
                            continue;
                        }
                        outline = true;
                    }
                },
                _ => {}
            }
            let mut literals = BTreeMap::new();
            if !outline {
                count_literals(&toks, &mut literals);
            }
            out.push(Unit {
                file: file.to_string(),
                path,
                tests,
                literals,
                outline,
            });
        } else if let Item::Mod(m) = &item
            && m.content.is_some()
        {
            let body = match toks.last() {
                Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Brace => g.stream(),
                _ => return Err(format!("unreadable {file}")),
            };
            let inner = split_items(body).map_err(|e| format!("unreadable {file}: {e}"))?;
            src_units(file, inner, &path, out)?;
        }
    }
    Ok(())
}

/// The units of `file` (a whole-file unit for a test file, else its `#[cfg(test)]` items).
/// `Err` names the file when it does not parse.
fn units(file: &str, src: &str) -> Result<Vec<Unit>, String> {
    let unreadable = |e: &dyn std::fmt::Display| format!("unreadable {file}: {e}");
    let ts: TokenStream = src.parse().map_err(|e| unreadable(&e))?;
    if is_test_file(file) {
        let parsed = syn::parse_file(src).map_err(|e| unreadable(&e))?;
        let mut tests = BTreeMap::new();
        collect_tests(&parsed.items, "", &mut tests);
        let mut literals = BTreeMap::new();
        let toks: Vec<TokenTree> = ts.into_iter().collect();
        count_literals(&toks, &mut literals);
        return Ok(vec![Unit {
            file: file.to_string(),
            path: String::new(),
            tests,
            literals,
            outline: false,
        }]);
    }
    let split = split_items(ts).map_err(|e| unreadable(&e))?;
    let mut out = Vec::new();
    src_units(file, split, "", &mut out)?;
    Ok(out)
}

/// A literal key as printed in a delta: line breaks as `\n` and `\r`, so an entry is one line.
fn printed_key(key: &str) -> String {
    key.replace('\r', "\\r").replace('\n', "\\n")
}

/// The multiset change from `parent` to `head`: `+` items sorted by key, then `-` items,
/// each `<sign><key> x<n>`, joined by `, `; empty when equal.
fn delta(parent: &BTreeMap<String, usize>, head: &BTreeMap<String, usize>) -> String {
    let mut plus = Vec::new();
    let mut minus = Vec::new();
    let keys: BTreeSet<&String> = parent.keys().chain(head.keys()).collect();
    for k in keys {
        let p = parent.get(k).copied().unwrap_or(0);
        let h = head.get(k).copied().unwrap_or(0);
        if h > p {
            plus.push(format!("+{} x{}", printed_key(k), h - p));
        } else if p > h {
            minus.push(format!("-{} x{}", printed_key(k), p - h));
        }
    }
    plus.extend(minus);
    plus.join(", ")
}

/// For each parent unit, the index of its match in `head`, or `None`.
fn match_units(
    parent: &[Unit],
    head: &[Unit],
    renamed: &BTreeMap<String, String>,
) -> Vec<Option<usize>> {
    let mut used = vec![false; head.len()];
    let mut out: Vec<Option<usize>> = vec![None; parent.len()];
    // In place: the same path in the same file or its renamed file.
    for (i, p) in parent.iter().enumerate() {
        let target = renamed.get(&p.file).unwrap_or(&p.file);
        if let Some(j) = head
            .iter()
            .enumerate()
            .position(|(j, h)| !used[j] && &h.file == target && h.path == p.path)
        {
            used[j] = true;
            out[i] = Some(j);
        }
    }
    // Moved: a `src` item to another `src` file of the same crate folder.
    let in_place = used.clone();
    for (i, p) in parent.iter().enumerate() {
        if out[i].is_some() || is_test_file(&p.file) || p.outline {
            continue;
        }
        let Some(dir) = crate_dir(&p.file) else {
            continue;
        };
        let src_prefix = format!("{dir}/src/");
        let candidates: Vec<usize> = (0..head.len())
            .filter(|&j| !in_place[j] && !used[j] && head[j].file.starts_with(&src_prefix))
            .collect();
        let chosen = if p.tests.is_empty() {
            let same: Vec<usize> = candidates
                .iter()
                .copied()
                .filter(|&j| head[j].path == p.path)
                .collect();
            (same.len() == 1).then(|| same[0])
        } else {
            let score = |j: usize| {
                p.tests
                    .keys()
                    .filter(|t| head[j].tests.contains_key(*t))
                    .count()
            };
            let best = candidates.iter().map(|&j| score(j)).max().unwrap_or(0);
            let top: Vec<usize> = candidates
                .iter()
                .copied()
                .filter(|&j| score(j) == best)
                .collect();
            (best >= 1 && top.len() == 1).then(|| top[0])
        };
        if let Some(j) = chosen {
            used[j] = true;
            out[i] = Some(j);
        }
    }
    out
}

/// Reads the entries under `## Test logic edits`: each entry with its line, and the
/// `unreadable entry` offences.
fn entries(node: &str) -> (Vec<(Entry, String)>, Vec<String>) {
    let mut out = Vec::new();
    let mut bad = Vec::new();
    let mut inside = false;
    for raw in node.lines() {
        let line = raw.trim_end_matches('\r');
        if line.starts_with("## ") {
            inside = line.trim_end() == "## Test logic edits";
            continue;
        }
        if !inside || line.trim().is_empty() || line.trim() == "None." {
            continue;
        }
        let Some(body) = line.strip_prefix("- ") else {
            continue;
        };
        match read_entry(body) {
            Some(e) => out.push((e, line.to_string())),
            None => bad.push(format!("unreadable entry: {line}")),
        }
    }
    (out, bad)
}

fn one_id(s: &str) -> Option<String> {
    (!s.is_empty() && !s.contains(char::is_whitespace)).then(|| s.to_string())
}

fn read_entry(body: &str) -> Option<Entry> {
    const SEP: &str = " — ";
    if let Some(rest) = body.strip_prefix("literals ") {
        let (file, rest) = rest.split_once(": ")?;
        let (_, reason) = rest.rsplit_once(SEP)?;
        if reason.trim().is_empty() {
            return None;
        }
        return Some(Entry::Literals {
            file: one_id(file)?,
            rest: rest.to_string(),
        });
    }
    let (head, reason) = body.split_once(SEP)?;
    if reason.trim().is_empty() {
        return None;
    }
    if let Some(id) = head.strip_prefix("removed ") {
        return Some(Entry::Removed { id: one_id(id)? });
    }
    if let Some(id) = head.strip_prefix("ignore ") {
        return Some(Entry::Ignore { id: one_id(id)? });
    }
    if let Some(r) = head.strip_prefix("renamed ") {
        let (old, new) = r.split_once(" -> ")?;
        return Some(Entry::Renamed {
            old: one_id(old)?,
            new: one_id(new)?,
        });
    }
    None
}

enum Kind {
    Removed { unit: usize },
    Ignore,
    Literals { file: String, delta: String },
    Outline,
}

struct Offence {
    text: String,
    id: String,
    kind: Kind,
    cleared: bool,
}

/// The offences of `parent` against its matches in `head`, after the node's entries.
fn offences(
    parent: &[Unit],
    head: &[Unit],
    matched: &[Option<usize>],
    entries: &[(Entry, String)],
) -> Vec<String> {
    let empty = BTreeMap::new();
    let mut list: Vec<Offence> = Vec::new();
    let mut files: BTreeMap<&str, (Multiset, Multiset)> = BTreeMap::new();
    for (i, p) in parent.iter().enumerate() {
        let base = if p.path.is_empty() {
            p.file.clone()
        } else {
            format!("{}::{}", p.file, p.path)
        };
        if p.outline {
            let text = format!("unreadable {base} — out-of-line test module");
            list.push(Offence {
                text,
                id: base,
                kind: Kind::Outline,
                cleared: false,
            });
            continue;
        }
        let h = matched.get(i).copied().flatten().map(|j| &head[j]);
        let h_tests = h.map_or(&empty, |u| &u.tests);
        for t in p.tests.keys().filter(|t| !h_tests.contains_key(*t)) {
            let id = format!("{base}::{t}");
            let kind = Kind::Removed { unit: i };
            list.push(Offence {
                text: format!("removed {id}"),
                id,
                kind,
                cleared: false,
            });
        }
        for (t, ignored) in &p.tests {
            if !ignored && h_tests.get(t) == Some(&true) {
                let id = format!("{base}::{t}");
                list.push(Offence {
                    text: format!("ignore {id}"),
                    id,
                    kind: Kind::Ignore,
                    cleared: false,
                });
            }
        }
        let acc = files.entry(&p.file).or_default();
        for (k, n) in &p.literals {
            *acc.0.entry(k.clone()).or_insert(0) += n;
        }
        if let Some(h) = h {
            for (k, n) in &h.literals {
                *acc.1.entry(k.clone()).or_insert(0) += n;
            }
        }
    }
    for (file, (p, h)) in &files {
        let d = delta(p, h);
        if !d.is_empty() {
            list.push(Offence {
                text: format!("literals {file}: {d}"),
                id: file.to_string(),
                kind: Kind::Literals {
                    file: file.to_string(),
                    delta: d,
                },
                cleared: false,
            });
        }
    }
    let mut not_found = Vec::new();
    for (entry, line) in entries {
        let hit = list.iter_mut().find(|o| {
            !o.cleared
                && match (entry, &o.kind) {
                    (Entry::Removed { id }, Kind::Removed { .. }) => *id == o.id,
                    (Entry::Ignore { id }, Kind::Ignore) => *id == o.id,
                    (Entry::Renamed { old, new }, Kind::Removed { unit }) => {
                        *old == o.id
                            && !parent[*unit].tests.contains_key(new)
                            && matched
                                .get(*unit)
                                .copied()
                                .flatten()
                                .is_some_and(|j| head[j].tests.contains_key(new))
                    }
                    (Entry::Literals { file, rest }, Kind::Literals { file: f, delta }) => {
                        file == f
                            && rest
                                .strip_prefix(delta.as_str())
                                .and_then(|r| r.strip_prefix(" — "))
                                .is_some_and(|r| !r.trim().is_empty())
                    }
                    _ => false,
                }
        });
        match hit {
            Some(o) => o.cleared = true,
            None => not_found.push(format!("not found: {line}")),
        }
    }
    list.into_iter()
        .filter(|o| !o.cleared)
        .map(|o| o.text)
        .chain(not_found)
        .collect()
}

fn git(args: &[&str]) -> Result<Vec<u8>, String> {
    let out = Command::new("git")
        .args(args)
        .output()
        .map_err(|e| format!("git {}: {e}", args.join(" ")))?;
    if !out.status.success() {
        return Err(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(out.stdout)
}

/// The `.rs` files under `crates/<c>/src/` and `crates/<c>/tests/` at `rev`.
fn rust_files(rev: &str) -> Result<Vec<String>, String> {
    let raw = git(&["ls-tree", "-r", "--name-only", "-z", rev, "--", "crates"])?;
    Ok(raw
        .split(|b| *b == 0)
        .filter_map(|p| std::str::from_utf8(p).ok())
        .filter(|p| {
            let parts: Vec<&str> = p.split('/').collect();
            p.ends_with(".rs")
                && parts.len() >= 4
                && parts[0] == "crates"
                && (parts[2] == "src" || parts[2] == "tests")
        })
        .map(str::to_string)
        .collect())
}

/// The units of each file at `rev`, and the `unreadable` offences.
fn read_units(rev: &str) -> Result<(Vec<Unit>, Vec<String>), String> {
    let mut all = Vec::new();
    let mut bad = Vec::new();
    for file in rust_files(rev)? {
        let raw = git(&["show", &format!("{rev}:{file}")])?;
        let parsed = String::from_utf8(raw)
            .map_err(|_| format!("unreadable {file}: not UTF-8"))
            .and_then(|src| units(&file, &src));
        match parsed {
            Ok(u) => all.extend(u),
            Err(e) => bad.push(e),
        }
    }
    Ok((all, bad))
}

/// Old path → new path for each rename from `parent` to `HEAD` under `crates`.
fn renames(parent: &str) -> Result<BTreeMap<String, String>, String> {
    let raw = git(&[
        "diff",
        "-M",
        "-z",
        "--name-status",
        parent,
        "HEAD",
        "--",
        "crates",
    ])?;
    let fields: Vec<&str> = raw
        .split(|b| *b == 0)
        .map(|f| std::str::from_utf8(f).unwrap_or(""))
        .collect();
    let mut out = BTreeMap::new();
    let mut i = 0;
    while i < fields.len() {
        let status = fields[i];
        if status.starts_with('R') || status.starts_with('C') {
            if status.starts_with('R') && i + 2 < fields.len() {
                out.insert(fields[i + 1].to_string(), fields[i + 2].to_string());
            }
            i += 3;
        } else if status.is_empty() {
            i += 1;
        } else {
            i += 2;
        }
    }
    Ok(out)
}

fn usage(msg: &str) -> ExitCode {
    eprintln!("check-test-edits: {msg}");
    eprintln!("usage: sh scripts/check-test-edits.sh <parent> <node file>");
    ExitCode::from(2)
}

fn run(parent: &str, node: &str) -> Result<Vec<String>, String> {
    let parent = String::from_utf8_lossy(&git(&[
        "rev-parse",
        "--verify",
        "--quiet",
        &format!("{parent}^{{commit}}"),
    ])?)
    .trim()
    .to_string();
    let (p_units, p_bad) = read_units(&parent)?;
    let (h_units, h_bad) = read_units("HEAD")?;
    let renamed = renames(&parent)?;
    let matched = match_units(&p_units, &h_units, &renamed);
    let (es, bad_entries) = entries(node);
    let mut out = p_bad;
    out.extend(h_bad);
    out.extend(offences(&p_units, &h_units, &matched, &es));
    out.extend(bad_entries);
    if out.is_empty() {
        println!("check-test-edits: ok ({} units)", p_units.len());
    }
    Ok(out)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return usage("two arguments are required");
    }
    let (parent, node_file) = (&args[0], &args[1]);
    if parent.trim().is_empty() {
        return usage("<parent> is empty");
    }
    let is_commit = Command::new("git")
        .args([
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{parent}^{{commit}}"),
        ])
        .output()
        .is_ok_and(|o| o.status.success());
    if !is_commit {
        return usage(&format!("<parent> is not a commit: {parent}"));
    }
    let node = match std::fs::read_to_string(node_file) {
        Ok(s) => s,
        Err(e) => return usage(&format!("cannot read node file {node_file}: {e}")),
    };
    match run(parent, &node) {
        Ok(out) if out.is_empty() => ExitCode::SUCCESS,
        Ok(out) => {
            for line in out {
                println!("{line}");
            }
            ExitCode::from(1)
        }
        Err(e) => {
            eprintln!("check-test-edits: {e}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn meta(s: &str) -> syn::Meta {
        syn::parse_str::<syn::Meta>(s).expect("meta")
    }

    fn attr(s: &str) -> syn::Attribute {
        let f = syn::parse_file(&format!("{s} fn f() {{}}")).expect("file");
        match &f.items[0] {
            syn::Item::Fn(f) => f.attrs[0].clone(),
            _ => panic!("not a fn"),
        }
    }

    fn lits(pairs: &[(&str, usize)]) -> BTreeMap<String, usize> {
        pairs.iter().map(|(k, n)| (k.to_string(), *n)).collect()
    }

    fn tests_of(names: &[&str]) -> BTreeMap<String, bool> {
        names.iter().map(|n| (n.to_string(), false)).collect()
    }

    fn unit(file: &str, path: &str, tests: &[&str], literals: &[(&str, usize)]) -> Unit {
        Unit {
            file: file.to_string(),
            path: path.to_string(),
            tests: tests_of(tests),
            literals: lits(literals),
            outline: false,
        }
    }

    #[test]
    fn cfg_requires_test_cases() {
        for s in [
            "test",
            "all(test, unix)",
            "all(unix, all(test))",
            "any(test, all(test, unix))",
        ] {
            assert!(cfg_requires_test(&meta(s)), "{s}");
        }
        for s in [
            "unix",
            "any(test, unix)",
            "not(test)",
            "feature = \"test-hooks\"",
            "all(unix, feature = \"test-hooks\")",
        ] {
            assert!(!cfg_requires_test(&meta(s)), "{s}");
        }
    }

    #[test]
    fn test_and_ignore_attrs() {
        assert!(is_test_attr(&attr("#[test]")));
        assert!(is_test_attr(&attr(
            "#[tokio::test(flavor = \"multi_thread\")]"
        )));
        assert!(!is_test_attr(&attr("#[testing]")));
        for s in [
            "#[ignore]",
            "#[ignore = \"r\"]",
            "#[cfg_attr(windows, ignore)]",
            "#[cfg_attr(unix, cfg_attr(target_os = \"linux\", ignore = \"r\"))]",
        ] {
            assert!(is_ignore_attr(&attr(s)), "{s}");
        }
        assert!(!is_ignore_attr(&attr("#[should_panic]")));
    }

    #[test]
    fn literals_in_macros_and_attributes() {
        let src = "#[test] #[should_panic(expected = \"boom\")] fn f() { assert_eq!(x, \"a\"); \
                   let _ = format!(\"{}\", 'c'); let n = 0x1F; let b = b\"z\"; let t = true; }";
        let u = units("crates/a/tests/t.rs", src).expect("units");
        assert_eq!(u.len(), 1);
        assert_eq!(
            u[0].literals,
            lits(&[
                ("\"boom\"", 1),
                ("\"a\"", 1),
                ("\"{}\"", 1),
                ("'c'", 1),
                ("0x1F", 1),
                ("b\"z\"", 1),
            ])
        );
    }

    #[test]
    fn doc_comments_skipped() {
        let src = "/// \"x\" 1\n#[doc = \"y\"] #[test] fn f() {}\n";
        let u = units("crates/a/tests/t.rs", src).expect("units");
        assert_eq!(u.len(), 1);
        assert!(u[0].literals.is_empty(), "{:?}", u[0].literals);
        assert_eq!(u[0].tests, tests_of(&["f"]));
    }

    #[test]
    fn delta_text() {
        assert_eq!(
            delta(&lits(&[("\"x\"", 1)]), &lits(&[("\"claude-code\"", 3)])),
            "+\"claude-code\" x3, -\"x\" x1"
        );
        assert_eq!(delta(&lits(&[("1", 2)]), &lits(&[("1", 2)])), "");
    }

    #[test]
    fn src_units() {
        let src = "#[cfg(test)] mod tests { #[test] fn a() {} mod inner { #[test] fn b() {} } } \
                   #[cfg(all(test, unix))] fn helper() { let _ = 1; } \
                   #[cfg(feature = \"test-hooks\")] fn hook() {} fn prod() { let _ = \"p\"; }";
        let u = units("crates/a/src/lib.rs", src).expect("units");
        assert_eq!(u.len(), 2);
        assert_eq!(u[0].path, "tests");
        assert_eq!(u[0].tests, tests_of(&["a", "inner::b"]));
        assert!(u[0].literals.is_empty());
        assert_eq!(u[1].path, "helper");
        assert!(u[1].tests.is_empty());
        assert_eq!(u[1].literals, lits(&[("1", 1)]));
    }

    #[test]
    fn src_units_in_plain_modules_and_out_of_line() {
        let src = "mod outer { #[cfg(test)] mod tests { #[test] fn a() { let _ = 2; } } } \
                   #[cfg(test)] #[path = \"../tests/common/mod.rs\"] mod common; \
                   #[cfg(test)] mod elsewhere;";
        let u = units("crates/a/src/lib.rs", src).expect("units");
        assert_eq!(u.len(), 2);
        assert_eq!(u[0].path, "outer::tests");
        assert_eq!(u[0].tests, tests_of(&["a"]));
        assert_eq!(u[0].literals, lits(&[("2", 1)]));
        assert!(!u[0].outline);
        assert_eq!(u[1].path, "elsewhere");
        assert!(u[1].outline);
    }

    #[test]
    fn test_file_unit() {
        let src = "#[test] fn a() {}\nmod m { #[test] #[ignore] fn b() {} }\n";
        let u = units("crates/a/tests/t.rs", src).expect("units");
        assert_eq!(u.len(), 1);
        assert_eq!(u[0].path, "");
        let mut want = tests_of(&["a"]);
        want.insert("m::b".to_string(), true);
        assert_eq!(u[0].tests, want);
    }

    #[test]
    fn unparsable_file_is_err() {
        let e = units("crates/a/src/lib.rs", "fn (").expect_err("err");
        assert!(e.contains("crates/a/src/lib.rs"), "{e}");
    }

    #[test]
    fn moved_item_matches() {
        let none = BTreeMap::new();
        let parent = vec![unit("crates/a/src/a.rs", "tests", &["p", "q"], &[])];
        let head = vec![unit("crates/a/src/b.rs", "tests", &["p", "q"], &[])];
        let m = match_units(&parent, &head, &none);
        assert_eq!(m, vec![Some(0)]);
        assert!(offences(&parent, &head, &m, &[]).is_empty());

        let head2 = vec![
            unit("crates/a/src/b.rs", "tests", &["p", "q"], &[]),
            unit("crates/a/src/c.rs", "tests", &["p", "q"], &[]),
        ];
        let m2 = match_units(&parent, &head2, &none);
        assert_eq!(m2, vec![None]);
        assert_eq!(
            offences(&parent, &head2, &m2, &[]),
            vec![
                "removed crates/a/src/a.rs::tests::p".to_string(),
                "removed crates/a/src/a.rs::tests::q".to_string(),
            ]
        );
    }

    #[test]
    fn renamed_file_matches_in_place() {
        let mut renamed = BTreeMap::new();
        renamed.insert(
            "crates/a/tests/t.rs".to_string(),
            "crates/a/tests/u.rs".to_string(),
        );
        let parent = vec![unit("crates/a/tests/t.rs", "", &["a"], &[("1", 1)])];
        let head = vec![
            unit("crates/a/tests/t.rs", "", &[], &[]),
            unit("crates/a/tests/u.rs", "", &["a"], &[("1", 1)]),
        ];
        assert_eq!(match_units(&parent, &head, &renamed), vec![Some(1)]);
    }

    #[test]
    fn entries_read() {
        let node = "# N\n\n## Test logic edits\n\nNone.\n\
                    - removed crates/a/tests/t.rs::f — spec K-C7\n\
                    - renamed crates/a/src/lib.rs::tests::old -> new — K§9\n\
                    - ignore crates/a/tests/t.rs::m::g — K§9\n\
                    - literals crates/a/tests/t.rs: +\"b\" x1, -\"a\" x1 — K§9\n\
                    - frob x — y\n\
                    - removed a.rs::f — \n\
                    ## Plan\n\n- removed crates/x.rs::f — not in the section\n";
        let (es, bad) = entries(node);
        assert_eq!(es.len(), 4);
        match &es[0].0 {
            Entry::Removed { id } => assert_eq!(id, "crates/a/tests/t.rs::f"),
            e => panic!("{e:?}"),
        }
        match &es[1].0 {
            Entry::Renamed { old, new } => {
                assert_eq!(old, "crates/a/src/lib.rs::tests::old");
                assert_eq!(new, "new");
            }
            e => panic!("{e:?}"),
        }
        match &es[2].0 {
            Entry::Ignore { id } => assert_eq!(id, "crates/a/tests/t.rs::m::g"),
            e => panic!("{e:?}"),
        }
        match &es[3].0 {
            Entry::Literals { file, rest } => {
                assert_eq!(file, "crates/a/tests/t.rs");
                assert_eq!(rest, "+\"b\" x1, -\"a\" x1 — K§9");
            }
            e => panic!("{e:?}"),
        }
        assert_eq!(
            bad,
            vec![
                "unreadable entry: - frob x — y".to_string(),
                "unreadable entry: - removed a.rs::f — ".to_string(),
            ]
        );
        let (none, bad2) = entries("## Test logic edits\n\nNone.\n");
        assert!(none.is_empty() && bad2.is_empty());
    }

    #[test]
    fn entry_clears_only_its_offence() {
        let none = BTreeMap::new();
        let parent = vec![unit("a.rs", "", &["f"], &[])];
        let line = "- literals a.rs: +\"b\" x1 — reason";
        let (es, bad) = entries(&format!("## Test logic edits\n{line}\n"));
        assert!(bad.is_empty());

        let head1 = vec![unit("a.rs", "", &["f"], &[("\"b\"", 1)])];
        let m1 = match_units(&parent, &head1, &none);
        assert!(offences(&parent, &head1, &m1, &es).is_empty());

        let head2 = vec![unit("a.rs", "", &["f"], &[("\"b\"", 2)])];
        let m2 = match_units(&parent, &head2, &none);
        assert_eq!(
            offences(&parent, &head2, &m2, &es),
            vec![
                "literals a.rs: +\"b\" x2".to_string(),
                format!("not found: {line}")
            ]
        );

        let rline = "- removed a.rs::f — reason";
        let (res, _) = entries(&format!("## Test logic edits\n{rline}\n"));
        assert_eq!(
            offences(&parent, &head1, &m1, &res),
            vec![
                "literals a.rs: +\"b\" x1".to_string(),
                format!("not found: {rline}")
            ]
        );
    }

    #[test]
    fn rename_and_ignore_entries_clear() {
        let none = BTreeMap::new();
        let parent = vec![unit("a.rs", "", &["old", "g"], &[])];
        let mut head = unit("a.rs", "", &["new", "g"], &[]);
        head.tests.insert("g".to_string(), true);
        let head = vec![head];
        let m = match_units(&parent, &head, &none);
        assert_eq!(
            offences(&parent, &head, &m, &[]),
            vec![
                "removed a.rs::old".to_string(),
                "ignore a.rs::g".to_string()
            ]
        );
        let (es, _) =
            entries("## Test logic edits\n- renamed a.rs::old -> new — r\n- ignore a.rs::g — r\n");
        assert!(offences(&parent, &head, &m, &es).is_empty());
        let (wrong, _) = entries("## Test logic edits\n- renamed a.rs::old -> g — r\n");
        assert_eq!(
            offences(&parent, &head, &m, &wrong),
            vec![
                "removed a.rs::old".to_string(),
                "ignore a.rs::g".to_string(),
                "not found: - renamed a.rs::old -> g — r".to_string(),
            ]
        );
    }
}
