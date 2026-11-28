//! `zygo-test-edits`: the per-node test-edit check of K-C7 (amended), run by
//! `scripts/check-test-edits.sh <parent> <node file>`.
//!
//! Each test file under `<top>/<c>/tests/` (`<top>` is `crates` or `tools`), each file whose inner
//! `cfg` requires `test`, each `#[cfg(test)]` item and each bare `#[test]` function of
//! `<top>/<c>/src/` that exists at `<parent>` is a unit. Against its match at `HEAD`, the check
//! reports each removed or renamed test function, each added `#[ignore]` or changed `cfg`,
//! `cfg_attr` or `should_panic` attribute, each change of the multiset of literals, each changed
//! doc-test code block, each changed non-`.rs` file under a `tests` folder and each change to the
//! check itself, unless the node file at `HEAD` lists that exact change under
//! `## Test logic edits`. Exit 0: the check passes; 1: offences, one per line on stdout; 2: usage
//! error (see `main`).

use proc_macro2::{Delimiter, TokenStream, TokenTree};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::{Command, ExitCode};
use syn::parse::{ParseStream, Parser};
use syn::punctuated::Punctuated;
use syn::{Attribute, Item, Meta, Token};

/// The folders whose `<name>/src` and `<name>/tests` hold units.
const TOPS: [&str; 2] = ["crates", "tools"];

/// The files of the check itself (F5): a change to one is an offence unless listed.
const CHECKER: [&str; 3] = [
    "tools/test-edits",
    "scripts/check-test-edits.sh",
    "scripts/test-check-test-edits.sh",
];

/// Literal source text → count.
type Multiset = BTreeMap<String, usize>;

/// One test function of a unit.
#[derive(Debug, Clone, PartialEq, Default)]
struct TestFn {
    ignored: bool,
    /// The `cfg`, `cfg_attr` and `should_panic` attributes of the function and of each module
    /// between it and its unit, as text.
    gate: String,
}

/// One unit: a whole file (`path` is empty) or an item of a `src` file.
#[derive(Debug, Clone, PartialEq, Default)]
struct Unit {
    file: String,
    path: String,
    /// Test-function id (module path inside the unit) → its function.
    tests: BTreeMap<String, TestFn>,
    literals: Multiset,
    /// The `cfg` and `cfg_attr` attributes of the unit (inner ones for a whole file), as text.
    gate: String,
    /// An out-of-line `#[cfg(test)] mod x;` whose body file was not found.
    outline: bool,
    /// The candidate body files of an out-of-line test module, first match wins.
    targets: Vec<String>,
}

/// An entry under `## Test logic edits`.
#[derive(Debug, Clone, PartialEq)]
enum Entry {
    Removed { id: String },
    Renamed { old: String, new: String },
    Ignore { id: String },
    Literals { file: String, rest: String },
    Checker { path: String },
    Fixture { path: String },
    Doctest { id: String },
}

/// True when the `cfg` predicate `meta` requires or allows `test` (F9: `any(test, …)` counts).
fn cfg_requires_test(meta: &Meta) -> bool {
    match meta {
        Meta::Path(p) => p.is_ident("test"),
        Meta::List(l) => {
            let args = match l.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated) {
                Ok(a) => a,
                Err(_) => return false,
            };
            if l.path.is_ident("all") || l.path.is_ident("any") {
                args.iter().any(cfg_requires_test)
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

fn requires_test(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|a| {
        a.path().is_ident("cfg") && a.parse_args::<Meta>().is_ok_and(|m| cfg_requires_test(&m))
    })
}

fn path_text(p: &syn::Path) -> String {
    let segs: Vec<String> = p.segments.iter().map(|s| s.ident.to_string()).collect();
    segs.join("::")
}

fn lit_text(l: &syn::Lit) -> String {
    match l {
        syn::Lit::Str(x) => x.token().to_string(),
        syn::Lit::ByteStr(x) => x.token().to_string(),
        syn::Lit::CStr(x) => x.token().to_string(),
        syn::Lit::Byte(x) => x.token().to_string(),
        syn::Lit::Char(x) => x.token().to_string(),
        syn::Lit::Int(x) => x.token().to_string(),
        syn::Lit::Float(x) => x.token().to_string(),
        syn::Lit::Bool(x) => x.value.to_string(),
        syn::Lit::Verbatim(x) => x.to_string(),
        _ => "<lit>".to_string(),
    }
}

fn expr_text(e: &syn::Expr) -> String {
    match e {
        syn::Expr::Lit(l) => lit_text(&l.lit),
        syn::Expr::Macro(m) => format!("{}!({})", path_text(&m.mac.path), m.mac.tokens),
        _ => "<expr>".to_string(),
    }
}

/// An attribute as text: `cfg(all(test, unix))`, `should_panic(expected = "x")`.
fn attr_text(a: &Attribute) -> String {
    let path = path_text(a.path());
    match &a.meta {
        Meta::Path(_) => path,
        Meta::List(l) => format!("{path}({})", l.tokens),
        Meta::NameValue(nv) => format!("{path} = {}", expr_text(&nv.value)),
    }
}

/// The `cfg`, `cfg_attr` and `should_panic` attributes as text, joined by `#`.
fn gate_of(attrs: &[Attribute]) -> String {
    let parts: Vec<String> = attrs
        .iter()
        .filter(|a| {
            a.path().is_ident("cfg")
                || a.path().is_ident("cfg_attr")
                || last_segment_is(a.path(), "should_panic")
        })
        .map(|a| format!("#[{}]", attr_text(a)))
        .collect();
    parts.join(" ")
}

fn join_gate(a: &str, b: &str) -> String {
    match (a.is_empty(), b.is_empty()) {
        (true, _) => b.to_string(),
        (_, true) => a.to_string(),
        _ => format!("{a} {b}"),
    }
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

fn is_number(l: &proc_macro2::Literal) -> bool {
    l.to_string().starts_with(|c: char| c.is_ascii_digit())
}

/// Counts each literal token, also inside groups (macro calls, attribute lists); skips each
/// `doc` attribute (`///`, `//!`, `#[doc = …]`). A `-` directly before a number literal is part
/// of it (F2: `-2` is not `2`).
fn count_literals(tts: &[TokenTree], out: &mut Multiset) {
    let mut i = 0;
    while i < tts.len() {
        if let TokenTree::Punct(p) = &tts[i] {
            if p.as_char() == '#' {
                let mut j = i + 1;
                if matches!(tts.get(j), Some(TokenTree::Punct(q)) if q.as_char() == '!') {
                    j += 1;
                }
                if is_doc_group(tts.get(j)) {
                    i = j + 1;
                    continue;
                }
            }
            if p.as_char() == '-'
                && let Some(TokenTree::Literal(l)) = tts.get(i + 1)
                && is_number(l)
            {
                *out.entry(format!("-{l}")).or_insert(0) += 1;
                i += 2;
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

/// Collects the test functions of `items` with ids prefixed by `prefix`; `gate` is the text of
/// the modules between them and the unit.
fn collect_tests(items: &[Item], prefix: &str, gate: &str, out: &mut BTreeMap<String, TestFn>) {
    for item in items {
        match item {
            Item::Fn(f) => {
                if f.attrs.iter().any(is_test_attr) {
                    let t = TestFn {
                        ignored: f.attrs.iter().any(is_ignore_attr),
                        gate: join_gate(gate, &gate_of(&f.attrs)),
                    };
                    out.insert(join(prefix, &f.sig.ident.to_string()), t);
                }
            }
            Item::Mod(m) => {
                if let Some((_, inner)) = &m.content {
                    let g = join_gate(gate, &gate_of(&m.attrs));
                    collect_tests(inner, &join(prefix, &m.ident.to_string()), &g, out);
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

/// True for a path `<top>/<c>/tests/…`.
fn is_test_file(file: &str) -> bool {
    let parts: Vec<&str> = file.split('/').collect();
    parts.len() >= 4 && TOPS.contains(&parts[0]) && parts[2] == "tests"
}

/// The crate folder `<top>/<c>` of a path.
fn crate_dir(file: &str) -> Option<String> {
    let parts: Vec<&str> = file.split('/').collect();
    (parts.len() >= 3 && TOPS.contains(&parts[0])).then(|| format!("{}/{}", parts[0], parts[1]))
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

/// The folder that holds the files of the child modules of `inline` (inline modules) in `file`.
fn module_dir(file: &str, inline: &[String]) -> String {
    let (dir, base) = file.rsplit_once('/').unwrap_or(("", file));
    let mut d = if matches!(base, "mod.rs" | "lib.rs" | "main.rs") {
        dir.to_string()
    } else {
        format!("{dir}/{}", base.trim_end_matches(".rs"))
    };
    for m in inline {
        d = format!("{d}/{m}");
    }
    d
}

/// The `#[path = "…"]` value of an out-of-line module, if any.
fn path_value(attrs: &[Attribute]) -> Option<String> {
    attrs.iter().find_map(|a| match &a.meta {
        Meta::NameValue(nv) if nv.path.is_ident("path") => match &nv.value {
            syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(s),
                ..
            }) => Some(s.value()),
            _ => None,
        },
        _ => None,
    })
}

/// The candidate body files of `mod name;` declared in `file` inside the inline modules `inline`,
/// by Rust's module path rules and `#[path]` (F6).
fn module_targets(file: &str, inline: &[String], name: &str, attrs: &[Attribute]) -> Vec<String> {
    match path_value(attrs) {
        Some(p) if inline.is_empty() => vec![resolve(file, &p)],
        Some(p) => vec![resolve(&format!("{}/_", module_dir(file, inline)), &p)],
        None => {
            let d = module_dir(file, inline);
            vec![format!("{d}/{name}.rs"), format!("{d}/{name}/mod.rs")]
        }
    }
}

fn src_units(
    file: &str,
    split: Split,
    prefix: &str,
    inline: &[String],
    out: &mut Vec<Unit>,
) -> Result<(), String> {
    let mut unnamed = BTreeMap::new();
    for (item, toks) in split {
        let name = item_name(&item, &mut unnamed);
        let path = join(prefix, &name);
        let attrs = item_attrs(&item);
        let bare_test = matches!(&item, Item::Fn(f) if f.attrs.iter().any(is_test_attr));
        if requires_test(attrs) || bare_test {
            let mut unit = Unit {
                file: file.to_string(),
                path,
                gate: gate_of(attrs),
                ..Unit::default()
            };
            match &item {
                Item::Fn(f) if bare_test => {
                    let t = TestFn {
                        ignored: f.attrs.iter().any(is_ignore_attr),
                        gate: String::new(),
                    };
                    unit.tests.insert(name.clone(), t);
                }
                Item::Mod(m) => match &m.content {
                    Some((_, inner)) => collect_tests(inner, "", "", &mut unit.tests),
                    None => {
                        let targets = module_targets(file, inline, &name, &m.attrs);
                        // A body file under `<top>/<c>/tests/` is a unit of its own.
                        if targets.iter().any(|t| is_test_file(t)) {
                            continue;
                        }
                        unit.outline = true;
                        unit.targets = targets;
                    }
                },
                _ => {}
            }
            if !unit.outline {
                count_literals(&toks, &mut unit.literals);
            }
            out.push(unit);
        } else if let Item::Mod(m) = &item
            && m.content.is_some()
        {
            let body = match toks.last() {
                Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Brace => g.stream(),
                _ => return Err(format!("unreadable {file}")),
            };
            let inner = split_items(body).map_err(|e| format!("unreadable {file}: {e}"))?;
            let mut inl = inline.to_vec();
            inl.push(name);
            src_units(file, inner, &path, &inl, out)?;
        }
    }
    Ok(())
}

/// A whole file as one unit: its tests, literals and inner `cfg` attributes.
fn whole_file(file: &str, parsed: &syn::File, ts: TokenStream, gate: &str) -> Unit {
    let mut unit = Unit {
        file: file.to_string(),
        gate: join_gate(gate, &gate_of(&parsed.attrs)),
        ..Unit::default()
    };
    collect_tests(&parsed.items, "", "", &mut unit.tests);
    let toks: Vec<TokenTree> = ts.into_iter().collect();
    count_literals(&toks, &mut unit.literals);
    unit
}

fn parse_both(file: &str, src: &str) -> Result<(TokenStream, syn::File), String> {
    let ts: TokenStream = src.parse().map_err(|e| format!("unreadable {file}: {e}"))?;
    let parsed = syn::parse_file(src).map_err(|e| format!("unreadable {file}: {e}"))?;
    Ok((ts, parsed))
}

/// The units of `file`: a whole-file unit for a test file or a file whose inner `cfg` requires
/// `test` (F9), else its test items. `Err` names the file when it does not parse.
fn units(file: &str, src: &str) -> Result<Vec<Unit>, String> {
    let (ts, parsed) = parse_both(file, src)?;
    if is_test_file(file) || requires_test(&parsed.attrs) {
        return Ok(vec![whole_file(file, &parsed, ts, "")]);
    }
    let split = split_items(ts).map_err(|e| format!("unreadable {file}: {e}"))?;
    let mut out = Vec::new();
    src_units(file, split, "", &[], &mut out)?;
    Ok(out)
}

/// The fenced code blocks (```` ``` ```` or `~~~`) of a doc text, each with its info string.
fn code_blocks(doc: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur: Option<(String, String)> = None;
    for line in doc.lines() {
        let t = line.trim_start();
        let fence = if t.starts_with("```") {
            Some("```")
        } else if t.starts_with("~~~") {
            Some("~~~")
        } else {
            None
        };
        match (&mut cur, fence) {
            (None, Some(f)) => cur = Some((f.to_string(), format!("{t}\n"))),
            (Some((f, body)), Some(g)) if f == g => {
                body.push_str(t);
                out.push(std::mem::take(body));
                cur = None;
            }
            (Some((_, body)), _) => {
                body.push_str(line);
                body.push('\n');
            }
            (None, None) => {}
        }
    }
    if let Some((_, body)) = cur {
        out.push(body);
    }
    out
}

/// Item path → the doc-test code blocks of its doc text.
type Docs = BTreeMap<String, Vec<String>>;

fn add_doc(out: &mut Docs, path: &str, attrs: &[Attribute]) {
    let mut text = String::new();
    for a in attrs.iter().filter(|a| a.path().is_ident("doc")) {
        if let Meta::NameValue(nv) = &a.meta {
            match &nv.value {
                syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(s),
                    ..
                }) => text.push_str(&s.value()),
                e => text.push_str(&expr_text(e)),
            }
            text.push('\n');
        }
    }
    let blocks = code_blocks(&text);
    if !blocks.is_empty() {
        out.entry(path.to_string()).or_default().extend(blocks);
    }
}

fn add_field_docs(out: &mut Docs, path: &str, fields: &syn::Fields) {
    for (n, f) in fields.iter().enumerate() {
        let name = f
            .ident
            .as_ref()
            .map_or_else(|| n.to_string(), |i| i.to_string());
        add_doc(out, &join(path, &name), &f.attrs);
    }
}

fn collect_docs(items: &[Item], prefix: &str, out: &mut Docs) {
    let mut unnamed = BTreeMap::new();
    for item in items {
        let path = join(prefix, &item_name(item, &mut unnamed));
        add_doc(out, &path, item_attrs(item));
        match item {
            Item::Mod(m) => {
                if let Some((_, inner)) = &m.content {
                    collect_docs(inner, &path, out);
                }
            }
            Item::Impl(i) => {
                for ii in &i.items {
                    let (name, attrs) = match ii {
                        syn::ImplItem::Fn(f) => (f.sig.ident.to_string(), &f.attrs),
                        syn::ImplItem::Const(c) => (c.ident.to_string(), &c.attrs),
                        syn::ImplItem::Type(t) => (t.ident.to_string(), &t.attrs),
                        syn::ImplItem::Macro(m) => ("macro".to_string(), &m.attrs),
                        _ => continue,
                    };
                    add_doc(out, &join(&path, &name), attrs);
                }
            }
            Item::Trait(t) => {
                for ti in &t.items {
                    let (name, attrs) = match ti {
                        syn::TraitItem::Fn(f) => (f.sig.ident.to_string(), &f.attrs),
                        syn::TraitItem::Const(c) => (c.ident.to_string(), &c.attrs),
                        syn::TraitItem::Type(t) => (t.ident.to_string(), &t.attrs),
                        syn::TraitItem::Macro(m) => ("macro".to_string(), &m.attrs),
                        _ => continue,
                    };
                    add_doc(out, &join(&path, &name), attrs);
                }
            }
            Item::Struct(s) => add_field_docs(out, &path, &s.fields),
            Item::Enum(e) => {
                for v in &e.variants {
                    let vp = join(&path, &v.ident.to_string());
                    add_doc(out, &vp, &v.attrs);
                    add_field_docs(out, &vp, &v.fields);
                }
            }
            Item::Union(u) => {
                for f in &u.fields.named {
                    let name = f.ident.as_ref().map_or_else(String::new, |i| i.to_string());
                    add_doc(out, &join(&path, &name), &f.attrs);
                }
            }
            _ => {}
        }
    }
}

/// The doc-test code blocks of a file by item path; `self` for the file's inner docs (F7).
fn doc_blocks(file: &str, src: &str) -> Result<Docs, String> {
    let parsed = syn::parse_file(src).map_err(|e| format!("unreadable {file}: {e}"))?;
    let mut out = Docs::new();
    add_doc(&mut out, "self", &parsed.attrs);
    collect_docs(&parsed.items, "", &mut out);
    Ok(out)
}

/// A literal key as printed in a delta: line breaks as `\n` and `\r`, so an entry is one line.
fn printed_key(key: &str) -> String {
    key.replace('\r', "\\r").replace('\n', "\\n")
}

/// The multiset change from `parent` to `head`: `+` items sorted by key, then `-` items,
/// each `<sign><key> x<n>`, joined by `, `; empty when equal.
fn delta(parent: &Multiset, head: &Multiset) -> String {
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
        if out[i].is_some() || p.path.is_empty() || p.outline {
            continue;
        }
        let Some(dir) = crate_dir(&p.file) else {
            continue;
        };
        let src_prefix = format!("{dir}/src/");
        let candidates: Vec<usize> = (0..head.len())
            .filter(|&j| {
                !in_place[j]
                    && !used[j]
                    && !head[j].path.is_empty()
                    && head[j].file.starts_with(&src_prefix)
            })
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
/// `unreadable entry` offences. A list item that does not start at column 0 with `- ` is
/// unreadable (F11: fail closed).
fn entries(node: &str) -> (Vec<(Entry, String)>, Vec<String>) {
    let mut out = Vec::new();
    let mut bad = Vec::new();
    let mut inside = false;
    let mut fence: Option<&str> = None;
    for raw in node.lines() {
        let line = raw.trim_end_matches('\r');
        // A heading inside a fenced block (```` ``` ```` or `~~~`) is text, not a heading.
        let lead = line.trim_start();
        let mark = ["```", "~~~"].into_iter().find(|m| lead.starts_with(m));
        match (fence, mark) {
            (None, Some(m)) => fence = Some(m),
            (Some(f), Some(m)) if f == m => fence = None,
            _ => {}
        }
        if fence.is_some() || mark.is_some() {
            if inside {
                bad.push(format!("unreadable entry: {line}"));
            }
            continue;
        }
        if line.starts_with("## ") {
            inside = line.trim_end() == "## Test logic edits";
            continue;
        }
        let t = line.trim();
        if !inside || t.is_empty() || t == "None." {
            continue;
        }
        match line.strip_prefix("- ") {
            Some(body) => match read_entry(body) {
                Some(e) => out.push((e, line.to_string())),
                None => bad.push(format!("unreadable entry: {line}")),
            },
            None => {
                let bullet = ["-", "*", "+"].iter().any(|b| {
                    t.strip_prefix(b)
                        .is_some_and(|r| r.is_empty() || r.starts_with(char::is_whitespace))
                });
                if bullet {
                    bad.push(format!("unreadable entry: {line}"));
                }
            }
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
    let (kind, arg) = head.split_once(' ')?;
    match kind {
        "removed" => Some(Entry::Removed { id: one_id(arg)? }),
        "ignore" => Some(Entry::Ignore { id: one_id(arg)? }),
        "checker" => Some(Entry::Checker { path: one_id(arg)? }),
        "fixture" => Some(Entry::Fixture { path: one_id(arg)? }),
        "doctest" => Some(Entry::Doctest { id: one_id(arg)? }),
        "renamed" => {
            let (old, new) = arg.split_once(" -> ")?;
            Some(Entry::Renamed {
                old: one_id(old)?,
                new: one_id(new)?,
            })
        }
        _ => None,
    }
}

enum Kind {
    Removed {
        unit: usize,
    },
    Ignore,
    Literals {
        file: String,
        delta: String,
    },
    /// `checker`, `fixture` and `doctest` offences: cleared by the entry of the same text.
    Plain,
    Outline,
}

struct Offence {
    text: String,
    id: String,
    kind: Kind,
    cleared: bool,
}

/// The ignore-kind offence text for a gate change.
fn ignore_text(id: &str, newly_ignored: bool, from: &str, to: &str) -> String {
    match (newly_ignored, from == to) {
        (true, true) => format!("ignore {id}"),
        (true, false) => format!("ignore {id} (#[ignore] added; attributes `{from}` -> `{to}`)"),
        _ => format!("ignore {id} (attributes `{from}` -> `{to}`)"),
    }
}

/// The offences of `parent` against its matches in `head`, and the `extra` offences
/// (`checker`, `fixture`, `doctest`), after the node's entries.
fn offences(
    parent: &[Unit],
    head: &[Unit],
    matched: &[Option<usize>],
    extra: &[String],
    entries: &[(Entry, String)],
) -> Vec<String> {
    let empty = BTreeMap::new();
    let mut list: Vec<Offence> = Vec::new();
    let push = |list: &mut Vec<Offence>, text: String, id: String, kind: Kind| {
        list.push(Offence {
            text,
            id,
            kind,
            cleared: false,
        })
    };
    let mut files: BTreeMap<&str, (Multiset, Multiset)> = BTreeMap::new();
    for (i, p) in parent.iter().enumerate() {
        let base = if p.path.is_empty() {
            p.file.clone()
        } else {
            format!("{}::{}", p.file, p.path)
        };
        if p.outline {
            let text = format!("unreadable {base} — out-of-line test module");
            push(&mut list, text, base, Kind::Outline);
            continue;
        }
        let h = matched.get(i).copied().flatten().map(|j| &head[j]);
        if let Some(h) = h
            && h.gate != p.gate
        {
            let text = ignore_text(&base, false, &p.gate, &h.gate);
            push(&mut list, text, base.clone(), Kind::Ignore);
        }
        let h_tests = h.map_or(&empty, |u| &u.tests);
        for t in p.tests.keys().filter(|t| !h_tests.contains_key(*t)) {
            let id = format!("{base}::{t}");
            push(
                &mut list,
                format!("removed {id}"),
                id,
                Kind::Removed { unit: i },
            );
        }
        for (t, pt) in &p.tests {
            let Some(ht) = h_tests.get(t) else { continue };
            let newly = ht.ignored && !pt.ignored;
            if newly || ht.gate != pt.gate {
                let id = format!("{base}::{t}");
                let text = ignore_text(&id, newly, &pt.gate, &ht.gate);
                push(&mut list, text, id, Kind::Ignore);
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
            let kind = Kind::Literals {
                file: file.to_string(),
                delta: d.clone(),
            };
            push(
                &mut list,
                format!("literals {file}: {d}"),
                file.to_string(),
                kind,
            );
        }
    }
    for e in extra {
        push(&mut list, e.clone(), e.clone(), Kind::Plain);
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
                    (Entry::Checker { path }, Kind::Plain) => o.id == format!("checker {path}"),
                    (Entry::Fixture { path }, Kind::Plain) => o.id == format!("fixture {path}"),
                    (Entry::Doctest { id }, Kind::Plain) => o.id == format!("doctest {id}"),
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

/// The doctest offences: each item whose code blocks at `<parent>` differ at `HEAD` (F7).
fn doctest_offences(
    parent: &BTreeMap<String, Docs>,
    head: &BTreeMap<String, Docs>,
    renamed: &BTreeMap<String, String>,
) -> Vec<String> {
    let empty = Docs::new();
    let mut out = Vec::new();
    for (file, docs) in parent {
        let hf = renamed.get(file).unwrap_or(file);
        let hd = head.get(hf).unwrap_or(&empty);
        for (path, blocks) in docs {
            if hd.get(path) != Some(blocks) {
                out.push(format!("doctest {file}::{path}"));
            }
        }
    }
    out
}

/// Runs git in the repository's top folder (F1: the check works from any subfolder).
struct Git {
    top: String,
}

impl Git {
    fn run(&self, args: &[&str]) -> Result<Vec<u8>, String> {
        let out = Command::new("git")
            .arg("-C")
            .arg(&self.top)
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

    fn ok(&self, args: &[&str]) -> bool {
        self.run(args).is_ok()
    }

    fn paths(&self, args: &[&str]) -> Result<Vec<String>, String> {
        Ok(self
            .run(args)?
            .split(|b| *b == 0)
            .filter(|p| !p.is_empty())
            .map(|p| String::from_utf8_lossy(p).into_owned())
            .collect())
    }
}

/// The `.rs` files under `<top>/<c>/src/` and `<top>/<c>/tests/` at `rev`.
fn rust_files(git: &Git, rev: &str) -> Result<Vec<String>, String> {
    let mut args = vec!["ls-tree", "-r", "--name-only", "-z", rev, "--"];
    args.extend(TOPS);
    Ok(git
        .paths(&args)?
        .into_iter()
        .filter(|p| {
            let parts: Vec<&str> = p.split('/').collect();
            p.ends_with(".rs") && parts.len() >= 4 && (parts[2] == "src" || parts[2] == "tests")
        })
        .collect())
}

/// What the check reads at one revision.
struct Rev {
    units: Vec<Unit>,
    docs: BTreeMap<String, Docs>,
    bad: Vec<String>,
}

/// Fills an out-of-line test module from its body file.
fn fill_outline(unit: &mut Unit, target: &str, src: &str) -> Result<(), String> {
    let (ts, parsed) = parse_both(target, src)?;
    let body = whole_file(target, &parsed, ts, &unit.gate);
    unit.tests = body.tests;
    unit.literals = body.literals;
    unit.gate = body.gate;
    unit.outline = false;
    Ok(())
}

fn read_rev(git: &Git, rev: &str) -> Result<Rev, String> {
    let mut srcs = BTreeMap::new();
    let mut bad = Vec::new();
    for file in rust_files(git, rev)? {
        let raw = git.run(&["show", &format!("{rev}:{file}")])?;
        match String::from_utf8(raw) {
            Ok(s) => {
                srcs.insert(file, s);
            }
            Err(_) => bad.push(format!("unreadable {file}: not UTF-8")),
        }
    }
    let mut per_file: BTreeMap<&str, Vec<Unit>> = BTreeMap::new();
    let mut docs = BTreeMap::new();
    for (file, src) in &srcs {
        match units(file, src).and_then(|u| Ok((u, doc_blocks(file, src)?))) {
            Ok((u, d)) => {
                per_file.insert(file, u);
                docs.insert(file.clone(), d);
            }
            Err(e) => bad.push(e),
        }
    }
    // Out-of-line test modules: the body file is the module's unit, not a unit of its own.
    let mut consumed = BTreeSet::new();
    for us in per_file.values_mut() {
        for u in us.iter_mut().filter(|u| u.outline) {
            let Some(t) = u.targets.iter().find(|t| srcs.contains_key(*t)).cloned() else {
                continue;
            };
            match fill_outline(u, &t, &srcs[&t]) {
                Ok(()) => {
                    consumed.insert(t);
                }
                Err(e) => bad.push(e),
            }
        }
    }
    let units = per_file
        .into_iter()
        .filter(|(f, _)| !consumed.contains(*f))
        .flat_map(|(_, u)| u)
        .collect();
    Ok(Rev { units, docs, bad })
}

/// Old path → new path for each rename from `parent` to `HEAD`.
fn renames(git: &Git, parent: &str) -> Result<BTreeMap<String, String>, String> {
    let mut args = vec!["diff", "-M", "-z", "--name-status", parent, "HEAD", "--"];
    args.extend(TOPS);
    let fields = git.paths(&args)?;
    let mut out = BTreeMap::new();
    let mut i = 0;
    while i < fields.len() {
        let status = fields[i].as_str();
        if status.starts_with('R') || status.starts_with('C') {
            if status.starts_with('R') && i + 2 < fields.len() {
                out.insert(fields[i + 1].clone(), fields[i + 2].clone());
            }
            i += 3;
        } else {
            i += 2;
        }
    }
    Ok(out)
}

/// `fixture <path>` for each changed non-`.rs` file under a `tests` folder (F8), and
/// `checker <path>` for each changed file of the check (F5).
fn changed_files(git: &Git, parent: &str) -> Result<Vec<String>, String> {
    let mut args = vec![
        "diff",
        "--no-renames",
        "--name-only",
        "-z",
        parent,
        "HEAD",
        "--",
    ];
    args.extend(TOPS);
    let mut out: Vec<String> = git
        .paths(&args)?
        .into_iter()
        .filter(|p| is_test_file(p) && !p.ends_with(".rs"))
        .map(|p| format!("fixture {p}"))
        .collect();
    let mut cargs = vec![
        "diff",
        "--no-renames",
        "--name-only",
        "-z",
        parent,
        "HEAD",
        "--",
    ];
    cargs.extend(CHECKER);
    out.extend(
        git.paths(&cargs)?
            .into_iter()
            .map(|p| format!("checker {p}")),
    );
    Ok(out)
}

fn usage(msg: &str) -> ExitCode {
    eprintln!("check-test-edits: {msg}");
    eprintln!("usage: sh scripts/check-test-edits.sh <parent> <node file>");
    ExitCode::from(2)
}

/// The checks before the run; `Err` is a usage message (exit 2). Returns the git runner, the
/// parent commit and the node file text at `HEAD`.
fn prepare(parent: &str, node_file: &str) -> Result<(Git, String, String), String> {
    if parent.trim().is_empty() {
        return Err("<parent> is empty".to_string());
    }
    let top = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .ok_or("not inside a git repository")?;
    let git = Git { top };
    let commit = format!("{parent}^{{commit}}");
    let parent_sha = git
        .run(&["rev-parse", "--verify", "--quiet", &commit])
        .map(|o| String::from_utf8_lossy(&o).trim().to_string())
        .map_err(|_| format!("<parent> is not a commit: {parent}"))?;
    let head = git
        .run(&["rev-parse", "--verify", "--quiet", "HEAD^{commit}"])
        .map(|o| String::from_utf8_lossy(&o).trim().to_string())
        .map_err(|_| "HEAD is not a commit".to_string())?;
    if parent_sha == head {
        return Err(format!("<parent> is HEAD: {parent}"));
    }
    if !git.ok(&["merge-base", "--is-ancestor", &parent_sha, &head]) {
        return Err(format!("<parent> is not an ancestor of HEAD: {parent}"));
    }
    let node = Path::new(node_file);
    if !node.is_file() {
        return Err(format!("node file not found: {node_file}"));
    }
    let abs = node
        .canonicalize()
        .map_err(|e| format!("node file {node_file}: {e}"))?;
    let top_abs = Path::new(&git.top)
        .canonicalize()
        .map_err(|e| format!("{}: {e}", git.top))?;
    let rel = abs
        .strip_prefix(&top_abs)
        .map_err(|_| format!("node file is not in the repository: {node_file}"))?
        .to_string_lossy()
        .replace('\\', "/");
    let text = git
        .run(&["show", &format!("HEAD:{rel}")])
        .map_err(|_| format!("node file is not committed at HEAD: {rel}"))?;
    let text = String::from_utf8(text).map_err(|_| format!("node file is not UTF-8: {rel}"))?;
    let mut sargs = vec!["status", "--porcelain", "--untracked-files=all", "--"];
    sargs.extend(TOPS);
    sargs.extend(&CHECKER[1..]);
    sargs.push(&rel);
    let dirty = git.run(&sargs).map_err(|e| e.to_string())?;
    if !dirty.is_empty() {
        return Err(format!(
            "uncommitted changes; commit them first:\n{}",
            String::from_utf8_lossy(&dirty).trim_end()
        ));
    }
    Ok((git, parent_sha, text))
}

fn run(git: &Git, parent: &str, node: &str) -> Result<Vec<String>, String> {
    let p = read_rev(git, parent)?;
    let h = read_rev(git, "HEAD")?;
    let renamed = renames(git, parent)?;
    let matched = match_units(&p.units, &h.units, &renamed);
    let mut extra = doctest_offences(&p.docs, &h.docs, &renamed);
    extra.extend(changed_files(git, parent)?);
    let (es, bad_entries) = entries(node);
    let mut out = p.bad;
    out.extend(h.bad);
    out.extend(offences(&p.units, &h.units, &matched, &extra, &es));
    out.extend(bad_entries);
    if out.is_empty() {
        println!("check-test-edits: ok ({} units)", p.units.len());
    }
    Ok(out)
}

/// Exit 2 when an argument is missing, `<parent>` is empty, not a commit, `HEAD` itself or not
/// an ancestor of `HEAD`, the node file is missing or not committed at `HEAD`, or a checked path
/// or the node file has uncommitted changes.
fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return usage("two arguments are required");
    }
    let (git, parent, node) = match prepare(&args[0], &args[1]) {
        Ok(x) => x,
        Err(e) => return usage(&e),
    };
    match run(&git, &parent, &node) {
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

    fn lits(pairs: &[(&str, usize)]) -> Multiset {
        pairs.iter().map(|(k, n)| (k.to_string(), *n)).collect()
    }

    fn tests_of(names: &[&str]) -> BTreeMap<String, TestFn> {
        names
            .iter()
            .map(|n| (n.to_string(), TestFn::default()))
            .collect()
    }

    fn unit(file: &str, path: &str, tests: &[&str], literals: &[(&str, usize)]) -> Unit {
        Unit {
            file: file.to_string(),
            path: path.to_string(),
            tests: tests_of(tests),
            literals: lits(literals),
            ..Unit::default()
        }
    }

    fn node(lines: &str) -> (Vec<(Entry, String)>, Vec<String>) {
        entries(&format!("## Test logic edits\n{lines}\n"))
    }

    #[test]
    fn cfg_requires_test_cases() {
        for s in [
            "test",
            "all(test, unix)",
            "all(unix, all(test))",
            "any(test, all(test, unix))",
            "any(test, unix)",
            "any(test, feature = \"x\")",
        ] {
            assert!(cfg_requires_test(&meta(s)), "{s}");
        }
        for s in [
            "unix",
            "not(test)",
            "feature = \"test-hooks\"",
            "all(unix, feature = \"test-hooks\")",
            "any(unix, not(test))",
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
    fn negative_number_is_one_literal() {
        let src = "#[test] fn f() { assert_eq!(d(-1), -2); let x = y - 3; let s = -\"q\"; }";
        let u = units("crates/a/tests/t.rs", src).expect("units");
        assert_eq!(
            u[0].literals,
            lits(&[("-1", 1), ("-2", 1), ("-3", 1), ("\"q\"", 1)])
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
        assert_eq!(u[0].gate, "#[cfg(test)]");
        assert!(u[0].literals.is_empty());
        assert_eq!(u[1].path, "helper");
        assert!(u[1].tests.is_empty());
        assert_eq!(u[1].literals, lits(&[("1", 1)]));
    }

    #[test]
    fn bare_test_and_any_test_items_are_units() {
        let src = "#[test] fn bare() { let _ = 1; } \
                   #[cfg(any(test, feature = \"x\"))] mod helpers { fn h() -> u8 { 2 } }";
        let u = units("crates/a/src/bare.rs", src).expect("units");
        assert_eq!(u.len(), 2);
        assert_eq!(u[0].path, "bare");
        assert_eq!(u[0].tests, tests_of(&["bare"]));
        assert_eq!(u[0].literals, lits(&[("1", 1)]));
        assert_eq!(u[1].path, "helpers");
        // The unit's own attributes are part of it.
        assert_eq!(u[1].literals, lits(&[("\"x\"", 1), ("2", 1)]));
    }

    #[test]
    fn inner_cfg_test_src_file_is_whole_file_unit() {
        let src = "#![cfg(test)]\n#[test]\nfn inner() { assert_eq!(1, 1); }\n";
        let u = units("crates/a/src/inner.rs", src).expect("units");
        assert_eq!(u.len(), 1);
        assert_eq!(u[0].path, "");
        assert_eq!(u[0].gate, "#[cfg(test)]");
        assert_eq!(u[0].tests, tests_of(&["inner"]));
        assert_eq!(u[0].literals, lits(&[("1", 2)]));
    }

    #[test]
    fn gates_of_tests_and_modules() {
        let src = "#![cfg(unix)]\n#[cfg(windows)] #[test] fn a() {}\n\
                   #[cfg_attr(miri, ignore)] mod m { #[test] #[should_panic] fn b() {} }\n";
        let u = units("crates/a/tests/t.rs", src).expect("units");
        assert_eq!(u[0].gate, "#[cfg(unix)]");
        assert_eq!(u[0].tests["a"].gate, "#[cfg(windows)]");
        assert!(!u[0].tests["a"].ignored);
        assert_eq!(
            u[0].tests["m::b"].gate,
            "#[cfg_attr(miri , ignore)] #[should_panic]"
        );
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
        assert_eq!(
            u[1].targets,
            vec![
                "crates/a/src/elsewhere.rs".to_string(),
                "crates/a/src/elsewhere/mod.rs".to_string()
            ]
        );
    }

    #[test]
    fn module_targets_follow_rust_rules() {
        let none: Vec<syn::Attribute> = Vec::new();
        assert_eq!(
            module_targets("crates/a/src/cmd/up.rs", &[], "t", &none),
            vec!["crates/a/src/cmd/up/t.rs", "crates/a/src/cmd/up/t/mod.rs"]
        );
        assert_eq!(
            module_targets("crates/a/src/cmd/mod.rs", &["x".to_string()], "t", &none),
            vec!["crates/a/src/cmd/x/t.rs", "crates/a/src/cmd/x/t/mod.rs"]
        );
        let p = vec![attr("#[path = \"../../tests/dead/mod.rs\"]")];
        assert_eq!(
            module_targets("crates/a/src/cmd/up.rs", &[], "dead", &p),
            vec!["crates/a/tests/dead/mod.rs"]
        );
        assert!(is_test_file("crates/a/tests/dead/mod.rs"));
        assert!(is_test_file("tools/x/tests/n.rs"));
        assert!(!is_test_file("crates/a/src/tests/x.rs"));
    }

    #[test]
    fn test_file_unit() {
        let src = "#[test] fn a() {}\nmod m { #[test] #[ignore] fn b() {} }\n";
        let u = units("crates/a/tests/t.rs", src).expect("units");
        assert_eq!(u.len(), 1);
        assert_eq!(u[0].path, "");
        let mut want = tests_of(&["a"]);
        want.insert(
            "m::b".to_string(),
            TestFn {
                ignored: true,
                gate: String::new(),
            },
        );
        assert_eq!(u[0].tests, want);
    }

    #[test]
    fn unparsable_file_is_err() {
        let e = units("crates/a/src/lib.rs", "fn (").expect_err("err");
        assert!(e.contains("crates/a/src/lib.rs"), "{e}");
    }

    #[test]
    fn doc_blocks_by_item() {
        let src = "//! ```\n//! a();\n//! ```\n/// Doubles.\n///\n/// ```\n/// assert_eq!(d(2), 4);\n/// ```\n\
                   pub fn d() {}\nimpl S {\n    /// ```text\n    /// x\n    /// ```\n    fn m() {}\n}\n\
                   /// No block.\nfn e() {}\n";
        let d = doc_blocks("crates/a/src/lib.rs", src).expect("docs");
        assert_eq!(d.len(), 3, "{d:?}");
        assert_eq!(d["self"], vec!["```\n a();\n```".to_string()]);
        assert_eq!(d["d"], vec!["```\n assert_eq!(d(2), 4);\n```".to_string()]);
        assert_eq!(d["impl#1::m"], vec!["```text\n x\n```".to_string()]);
    }

    #[test]
    fn doctest_offence_on_change() {
        let mut p = BTreeMap::new();
        p.insert(
            "a.rs".to_string(),
            Docs::from([("d".to_string(), vec!["```\nx\n```".to_string()])]),
        );
        let mut h = BTreeMap::new();
        h.insert(
            "a.rs".to_string(),
            Docs::from([("d".to_string(), vec!["```\ny\n```".to_string()])]),
        );
        let none = BTreeMap::new();
        assert_eq!(doctest_offences(&p, &h, &none), vec!["doctest a.rs::d"]);
        assert!(doctest_offences(&p, &p, &none).is_empty());
    }

    #[test]
    fn moved_item_matches() {
        let none = BTreeMap::new();
        let parent = vec![unit("crates/a/src/a.rs", "tests", &["p", "q"], &[])];
        let head = vec![unit("crates/a/src/b.rs", "tests", &["p", "q"], &[])];
        let m = match_units(&parent, &head, &none);
        assert_eq!(m, vec![Some(0)]);
        assert!(offences(&parent, &head, &m, &[], &[]).is_empty());

        let head2 = vec![
            unit("crates/a/src/b.rs", "tests", &["p", "q"], &[]),
            unit("crates/a/src/c.rs", "tests", &["p", "q"], &[]),
        ];
        let m2 = match_units(&parent, &head2, &none);
        assert_eq!(m2, vec![None]);
        assert_eq!(
            offences(&parent, &head2, &m2, &[], &[]),
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
                    - checker scripts/check-test-edits.sh — K-C7\n\
                    - fixture crates/a/tests/fixtures/g.json — K§9\n\
                    - doctest crates/a/src/lib.rs::d — K§9\n\
                    - frob x — y\n\
                    - removed a.rs::f — \n\
                      \x20\x20- removed a.rs::g — indented\n\
                    * removed a.rs::h — star\n\
                    ### Sub\n\
                    ## Plan\n\n- removed crates/x.rs::f — not in the section\n";
        let (es, bad) = entries(node);
        let kinds: Vec<Entry> = es.into_iter().map(|(e, _)| e).collect();
        assert_eq!(
            kinds,
            vec![
                Entry::Removed {
                    id: "crates/a/tests/t.rs::f".into()
                },
                Entry::Renamed {
                    old: "crates/a/src/lib.rs::tests::old".into(),
                    new: "new".into()
                },
                Entry::Ignore {
                    id: "crates/a/tests/t.rs::m::g".into()
                },
                Entry::Literals {
                    file: "crates/a/tests/t.rs".into(),
                    rest: "+\"b\" x1, -\"a\" x1 — K§9".into()
                },
                Entry::Checker {
                    path: "scripts/check-test-edits.sh".into()
                },
                Entry::Fixture {
                    path: "crates/a/tests/fixtures/g.json".into()
                },
                Entry::Doctest {
                    id: "crates/a/src/lib.rs::d".into()
                },
            ]
        );
        assert_eq!(
            bad,
            vec![
                "unreadable entry: - frob x — y".to_string(),
                "unreadable entry: - removed a.rs::f — ".to_string(),
                "unreadable entry:   - removed a.rs::g — indented".to_string(),
                "unreadable entry: * removed a.rs::h — star".to_string(),
            ]
        );
        let (none, bad2) = entries("## Test logic edits\n\nNone.\n");
        assert!(none.is_empty() && bad2.is_empty());
    }

    #[test]
    fn entries_line_forms() {
        let want = Entry::Removed {
            id: "crates/a/tests/t.rs::other".into(),
        };
        let line = "- removed crates/a/tests/t.rs::other — r";
        for node in [
            format!("# N\r\n\r\n## Test logic edits\r\n\r\n{line}\r\n\r\n## Plan\r\n"),
            format!("# N\n\n## Test logic edits  \n\n{line}\n"),
            format!("# N\n\n## Test logic edits\n\n### CC-x\n\n{line}\n"),
            format!("# N\n\n## Test logic edits\n\n{line} — more\n"),
        ] {
            let (es, bad) = entries(&node);
            assert!(bad.is_empty(), "{node:?}: {bad:?}");
            assert_eq!(es.len(), 1, "{node:?}");
            assert_eq!(es[0].0, want, "{node:?}");
        }
        let fenced = format!("# N\n\n## Plan\n\n```\n## Test logic edits\n{line}\n```\n");
        let (es, bad) = entries(&fenced);
        assert!(es.is_empty() && bad.is_empty(), "{es:?} {bad:?}");
        let (dup, _) = node(&format!("{line}\n{line}"));
        let parent = vec![unit("crates/a/tests/t.rs", "", &["other"], &[])];
        let head = vec![unit("crates/a/tests/t.rs", "", &[], &[])];
        let m = match_units(&parent, &head, &BTreeMap::new());
        assert_eq!(
            offences(&parent, &head, &m, &[], &dup),
            vec![format!("not found: {line}")]
        );
    }

    #[test]
    fn entry_clears_only_its_offence() {
        let none = BTreeMap::new();
        let parent = vec![unit("a.rs", "", &["f"], &[])];
        let line = "- literals a.rs: +\"b\" x1 — reason";
        let (es, bad) = node(line);
        assert!(bad.is_empty());

        let head1 = vec![unit("a.rs", "", &["f"], &[("\"b\"", 1)])];
        let m1 = match_units(&parent, &head1, &none);
        assert!(offences(&parent, &head1, &m1, &[], &es).is_empty());

        let head2 = vec![unit("a.rs", "", &["f"], &[("\"b\"", 2)])];
        let m2 = match_units(&parent, &head2, &none);
        assert_eq!(
            offences(&parent, &head2, &m2, &[], &es),
            vec![
                "literals a.rs: +\"b\" x2".to_string(),
                format!("not found: {line}")
            ]
        );

        let rline = "- removed a.rs::f — reason";
        let (res, _) = node(rline);
        assert_eq!(
            offences(&parent, &head1, &m1, &[], &res),
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
        head.tests.get_mut("g").expect("g").ignored = true;
        let head = vec![head];
        let m = match_units(&parent, &head, &none);
        assert_eq!(
            offences(&parent, &head, &m, &[], &[]),
            vec![
                "removed a.rs::old".to_string(),
                "ignore a.rs::g".to_string()
            ]
        );
        let (es, _) = node("- renamed a.rs::old -> new — r\n- ignore a.rs::g — r");
        assert!(offences(&parent, &head, &m, &[], &es).is_empty());
        let (wrong, _) = node("- renamed a.rs::old -> g — r");
        assert_eq!(
            offences(&parent, &head, &m, &[], &wrong),
            vec![
                "removed a.rs::old".to_string(),
                "ignore a.rs::g".to_string(),
                "not found: - renamed a.rs::old -> g — r".to_string(),
            ]
        );
    }

    #[test]
    fn gate_change_is_ignore_kind() {
        let none = BTreeMap::new();
        let mut p = unit("a.rs", "tests", &["f"], &[]);
        p.gate = "#[cfg(test)]".into();
        let mut h = p.clone();
        h.gate = "#[cfg(all(test, windows))]".into();
        h.tests.get_mut("f").expect("f").gate = "#[should_panic]".into();
        let (parent, head) = (vec![p], vec![h]);
        let m = match_units(&parent, &head, &none);
        assert_eq!(
            offences(&parent, &head, &m, &[], &[]),
            vec![
                "ignore a.rs::tests (attributes `#[cfg(test)]` -> `#[cfg(all(test, windows))]`)"
                    .to_string(),
                "ignore a.rs::tests::f (attributes `` -> `#[should_panic]`)".to_string(),
            ]
        );
        let (es, _) = node("- ignore a.rs::tests — r\n- ignore a.rs::tests::f — r");
        assert!(offences(&parent, &head, &m, &[], &es).is_empty());
    }

    #[test]
    fn extra_offences_clear_by_entry() {
        let extra = vec![
            "checker scripts/check-test-edits.sh".to_string(),
            "fixture crates/a/tests/g.json".to_string(),
            "doctest crates/a/src/lib.rs::d".to_string(),
        ];
        assert_eq!(offences(&[], &[], &[], &extra, &[]), extra);
        let (es, _) = node(
            "- checker scripts/check-test-edits.sh — r\n- fixture crates/a/tests/g.json — r\n\
             - doctest crates/a/src/lib.rs::d — r",
        );
        assert!(offences(&[], &[], &[], &extra, &es).is_empty());
        let (wrong, _) = node("- fixture crates/a/tests/h.json — r");
        assert_eq!(
            offences(&[], &[], &[], &extra[1..2], &wrong),
            vec![
                "fixture crates/a/tests/g.json".to_string(),
                "not found: - fixture crates/a/tests/h.json — r".to_string()
            ]
        );
    }
}
