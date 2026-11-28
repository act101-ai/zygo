//! `zygo-test-edits`: the per-node test-edit check of K-C7 (amended), run by
//! `scripts/check-test-edits.sh <parent> <node file>`, which builds this helper from `<parent>`.
//!
//! A unit is each test file (under a crate's `tests/`, the root `tests/`, or a `[[test]]` path),
//! each `.rs` file whose inner `cfg` requires `test`, each `#[cfg(test)]` item and each bare
//! `#[test]` function of a crate's `src/`, that exists at `<parent>`; an out-of-line module of a
//! unit is part of it. Against its match at `HEAD`, the check reports each removed or renamed
//! test function, each added `#[ignore]` or changed `cfg`, `cfg_attr` or `should_panic`
//! attribute on the test's path to its crate root, and each change of the multiset of literals;
//! also each changed doc-test block, test-relevant manifest key, fixture and file of the check.
//! The node file at `HEAD` lists each allowed change under `## Test logic edits`.
//! Exit 0: the check passes; 1: offences, one per line on stdout; 2: usage error (see `main`).

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

/// A set of attributes as normalised text (D4: order and layout do not matter).
type Gate = BTreeSet<String>;

fn gate_text(g: &Gate) -> String {
    g.iter().cloned().collect::<Vec<_>>().join(" ")
}

/// One test function of a unit.
#[derive(Debug, Clone, PartialEq, Default)]
struct TestFn {
    ignored: bool,
    /// The `cfg`, `cfg_attr` and `should_panic` attributes of the function and of each module
    /// between it and its unit.
    gate: Gate,
}

/// An out-of-line `mod x;` declaration.
#[derive(Debug, Clone, PartialEq, Default)]
struct Decl {
    /// The declaring file.
    file: String,
    /// The inline modules around the declaration in its file.
    inline: Vec<String>,
    name: String,
    /// The `#[path = "…"]` value, if any.
    path: Option<String>,
    /// The module path from the start of the walk to the declared module (`a::x`).
    modpath: String,
    /// The `cfg`, `cfg_attr` attributes of the declaration and of the inline modules around it.
    gate: Gate,
}

/// One unit: a whole file (`path` is empty) or an item of a `src` file.
#[derive(Debug, Clone, PartialEq, Default)]
struct Unit {
    file: String,
    path: String,
    /// Test-function id (module path inside the unit) → its functions (D10: one name may hold
    /// several functions under different `cfg`s).
    tests: BTreeMap<String, Vec<TestFn>>,
    literals: Multiset,
    /// The `cfg` and `cfg_attr` attributes from the unit up to its crate root (D3).
    gate: Gate,
    /// An out-of-line `#[cfg(test)] mod x;`: its declaration, until its body file is read.
    outline: Option<Decl>,
    /// The out-of-line modules declared inside the unit (D8).
    decls: Vec<Decl>,
    /// The files named by `include_str!` and `include_bytes!` in the unit (D2).
    includes: BTreeSet<String>,
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
    Manifest { path: String },
}

/// What the manifests of a revision say about crate roots and test targets.
#[derive(Debug, Clone, Default)]
struct Ctx {
    /// Files that the manifests name as crate roots (`[lib]`, `[[bin]]`, `[[test]]`, …).
    roots: BTreeSet<String>,
    /// Files that the manifests name as `[[test]]` targets.
    test_targets: BTreeSet<String>,
    /// Files loaded by `#[path]`, whose child modules sit beside them as for `mod.rs`.
    modrs_like: BTreeSet<String>,
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

/// An attribute as text: `cfg(all (test , unix))`, `should_panic(expected = "x")`. The token
/// printer spaces tokens by kind, so source layout does not matter (D4).
fn attr_text(a: &Attribute) -> String {
    let path = path_text(a.path());
    match &a.meta {
        Meta::Path(_) => path,
        Meta::List(l) => format!("{path}({})", l.tokens),
        Meta::NameValue(nv) => format!("{path} = {}", expr_text(&nv.value)),
    }
}

/// The `cfg`, `cfg_attr` and `should_panic` attributes as a set of `#[…]` texts.
fn gate_of(attrs: &[Attribute]) -> Gate {
    attrs
        .iter()
        .filter(|a| {
            a.path().is_ident("cfg")
                || a.path().is_ident("cfg_attr")
                || last_segment_is(a.path(), "should_panic")
        })
        .map(|a| format!("#[{}]", attr_text(a)))
        .collect()
}

fn union(a: &Gate, b: &Gate) -> Gate {
    a.union(b).cloned().collect()
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

/// A type as text for an impl name, generic arguments left out.
fn type_text(ty: &syn::Type) -> String {
    match ty {
        syn::Type::Path(p) => path_text(&p.path),
        syn::Type::Reference(r) => format!("&{}", type_text(&r.elem)),
        syn::Type::Slice(s) => format!("[{}]", type_text(&s.elem)),
        syn::Type::Array(a) => format!("[{};_]", type_text(&a.elem)),
        syn::Type::Tuple(t) => {
            let parts: Vec<String> = t.elems.iter().map(type_text).collect();
            format!("({})", parts.join(","))
        }
        _ => "_".to_string(),
    }
}

/// The name of an item: its identifier; for an impl `impl<Type>` or `impl<Trait>for<Type>`
/// (D9); else `<kind>#<n>`. A name that repeats at its level gets `#<n>` from the second on.
fn item_name(item: &Item, seen: &mut BTreeMap<String, usize>) -> String {
    let base = match item {
        Item::Const(i) => i.ident.to_string(),
        Item::Enum(i) => i.ident.to_string(),
        Item::ExternCrate(i) => i.ident.to_string(),
        Item::Fn(i) => i.sig.ident.to_string(),
        Item::Macro(i) => i
            .ident
            .as_ref()
            .map_or_else(|| "macro".to_string(), |x| x.to_string()),
        Item::Mod(i) => i.ident.to_string(),
        Item::Static(i) => i.ident.to_string(),
        Item::Struct(i) => i.ident.to_string(),
        Item::Trait(i) => i.ident.to_string(),
        Item::TraitAlias(i) => i.ident.to_string(),
        Item::Type(i) => i.ident.to_string(),
        Item::Union(i) => i.ident.to_string(),
        Item::Impl(i) => match &i.trait_ {
            Some((path, _)) => {
                format!("impl<{}>for<{}>", path_text(path), type_text(&i.self_ty))
            }
            None => format!("impl<{}>", type_text(&i.self_ty)),
        },
        Item::Use(_) => "use".to_string(),
        Item::ForeignMod(_) => "extern".to_string(),
        _ => "item".to_string(),
    };
    let n = seen.entry(base.clone()).or_insert(0);
    *n += 1;
    let unnamed = matches!(item, Item::Use(_) | Item::ForeignMod(_) | Item::Verbatim(_))
        || matches!(item, Item::Macro(m) if m.ident.is_none());
    if unnamed || *n > 1 {
        format!("{base}#{n}")
    } else {
        base
    }
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

/// The literals and included files of a token list.
struct Walk<'a> {
    file: &'a str,
    literals: Multiset,
    includes: BTreeSet<String>,
}

impl Walk<'_> {
    /// Counts each literal token, also inside groups (macro calls, attribute lists); skips each
    /// `doc` attribute. A `-` directly before a number literal is part of it (F2). Records the
    /// file of each `include_str!("…")` and `include_bytes!("…")` (D2).
    fn walk(&mut self, tts: &[TokenTree]) {
        let mut i = 0;
        while i < tts.len() {
            match &tts[i] {
                TokenTree::Punct(p) if p.as_char() == '#' => {
                    let mut j = i + 1;
                    if matches!(tts.get(j), Some(TokenTree::Punct(q)) if q.as_char() == '!') {
                        j += 1;
                    }
                    if is_doc_group(tts.get(j)) {
                        i = j + 1;
                        continue;
                    }
                }
                TokenTree::Punct(p) if p.as_char() == '-' => {
                    if let Some(TokenTree::Literal(l)) = tts.get(i + 1)
                        && is_number(l)
                    {
                        *self.literals.entry(format!("-{l}")).or_insert(0) += 1;
                        i += 2;
                        continue;
                    }
                }
                TokenTree::Ident(id) if id == "include_str" || id == "include_bytes" => {
                    if matches!(tts.get(i + 1), Some(TokenTree::Punct(q)) if q.as_char() == '!')
                        && let Some(TokenTree::Group(g)) = tts.get(i + 2)
                        && let Some(TokenTree::Literal(l)) = g.stream().into_iter().next()
                        && let Ok(s) = syn::parse_str::<syn::LitStr>(&l.to_string())
                    {
                        self.includes.insert(resolve(self.file, &s.value()));
                    }
                }
                _ => {}
            }
            match &tts[i] {
                TokenTree::Literal(l) => *self.literals.entry(l.to_string()).or_insert(0) += 1,
                TokenTree::Group(g) => {
                    let inner: Vec<TokenTree> = g.stream().into_iter().collect();
                    self.walk(&inner);
                }
                _ => {}
            }
            i += 1;
        }
    }
}

fn walk_tokens(file: &str, tts: &[TokenTree]) -> (Multiset, BTreeSet<String>) {
    let mut w = Walk {
        file,
        literals: Multiset::new(),
        includes: BTreeSet::new(),
    };
    w.walk(tts);
    (w.literals, w.includes)
}

/// Collects the test functions of `items` with ids prefixed by `prefix`; `gate` holds the
/// attributes of the modules between them and the unit.
fn collect_tests(
    items: &[Item],
    prefix: &str,
    gate: &Gate,
    out: &mut BTreeMap<String, Vec<TestFn>>,
) {
    for item in items {
        match item {
            Item::Fn(f) => {
                if f.attrs.iter().any(is_test_attr) {
                    let t = TestFn {
                        ignored: f.attrs.iter().any(is_ignore_attr),
                        gate: union(gate, &gate_of(&f.attrs)),
                    };
                    out.entry(join(prefix, &f.sig.ident.to_string()))
                        .or_default()
                        .push(t);
                }
            }
            Item::Mod(m) => {
                if let Some((_, inner)) = &m.content {
                    let g = union(gate, &gate_of(&m.attrs));
                    collect_tests(inner, &join(prefix, &m.ident.to_string()), &g, out);
                }
            }
            _ => {}
        }
    }
}

/// Every out-of-line module declaration in `items`, inline modules followed.
fn collect_decls(
    file: &str,
    items: &[Item],
    inline: &[String],
    prefix: &str,
    gate: &Gate,
    out: &mut Vec<Decl>,
) {
    for item in items {
        if let Item::Mod(m) = item {
            let name = m.ident.to_string();
            let g = union(gate, &gate_of(&m.attrs));
            match &m.content {
                None => out.push(Decl {
                    file: file.to_string(),
                    inline: inline.to_vec(),
                    name: name.clone(),
                    path: path_value(&m.attrs),
                    modpath: join(prefix, &name),
                    gate: g,
                }),
                Some((_, inner)) => {
                    let mut inl = inline.to_vec();
                    inl.push(name.clone());
                    collect_decls(file, inner, &inl, &join(prefix, &name), &g, out);
                }
            }
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

fn parts(file: &str) -> Vec<&str> {
    file.split('/').collect()
}

/// True for a test file: `<top>/<c>/tests/…`, the root `tests/…`, or a `[[test]]` path (D2).
fn is_test_path(file: &str, ctx: &Ctx) -> bool {
    let p = parts(file);
    (p.len() >= 4 && TOPS.contains(&p[0]) && p[2] == "tests")
        || (p.len() >= 2 && p[0] == "tests")
        || ctx.test_targets.contains(file)
}

/// True for a crate root by Cargo's layout or a manifest's `path` (D7).
fn is_root(file: &str, ctx: &Ctx) -> bool {
    if ctx.roots.contains(file) {
        return true;
    }
    let p = parts(file);
    let n = p.len();
    let base = p[n - 1];
    if matches!(base, "lib.rs" | "main.rs" | "build.rs") {
        return true;
    }
    // A direct child of a crate's (or the root's) tests/, benches/, examples/ or src/bin/.
    let in_crate_dir = |k: usize| k == 0 || (k == 2 && TOPS.contains(&p[0]));
    n >= 2
        && ((matches!(p[n - 2], "tests" | "benches" | "examples") && in_crate_dir(n - 2))
            || (n >= 3 && p[n - 2] == "bin" && p[n - 3] == "src"))
}

/// The crate folder `<top>/<c>` of a path.
fn crate_dir(file: &str) -> Option<String> {
    let p = parts(file);
    (p.len() >= 3 && TOPS.contains(&p[0])).then(|| format!("{}/{}", p[0], p[1]))
}

/// Resolves `rel` against the folder of `file`, folding `.` and `..`.
fn resolve(file: &str, rel: &str) -> String {
    let mut out: Vec<&str> = file.split('/').collect();
    out.pop();
    for seg in rel.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            s => out.push(s),
        }
    }
    out.join("/")
}

/// The folder that holds the files of the child modules of `inline` in `file`: beside a crate
/// root, a `mod.rs` or a file loaded by `#[path]`; else in a folder named after the file.
fn module_dir(file: &str, inline: &[String], ctx: &Ctx) -> String {
    let (dir, base) = file.rsplit_once('/').unwrap_or(("", file));
    let mut d = if base == "mod.rs" || is_root(file, ctx) || ctx.modrs_like.contains(file) {
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

/// The candidate body files of a declaration, by Rust's module path rules and `#[path]` (F6).
fn decl_targets(d: &Decl, ctx: &Ctx) -> Vec<String> {
    match &d.path {
        Some(p) if d.inline.is_empty() => vec![resolve(&d.file, p)],
        Some(p) => vec![resolve(
            &format!("{}/_", module_dir(&d.file, &d.inline, ctx)),
            p,
        )],
        None => {
            let m = module_dir(&d.file, &d.inline, ctx);
            vec![
                format!("{m}/{}.rs", d.name),
                format!("{m}/{}/mod.rs", d.name),
            ]
        }
    }
}

/// The units of a `src` file's items; `inline` and `gate` describe the plain modules around them.
fn src_units(
    file: &str,
    split: Split,
    prefix: &str,
    inline: &[String],
    gate: &Gate,
    out: &mut Vec<Unit>,
) -> Result<(), String> {
    let mut seen = BTreeMap::new();
    for (item, toks) in split {
        let name = item_name(&item, &mut seen);
        let path = join(prefix, &name);
        let attrs = item_attrs(&item);
        let bare_test = matches!(&item, Item::Fn(f) if f.attrs.iter().any(is_test_attr));
        if requires_test(attrs) || bare_test {
            let mut unit = Unit {
                file: file.to_string(),
                path,
                gate: union(gate, &gate_of(attrs)),
                ..Unit::default()
            };
            match &item {
                Item::Fn(f) if bare_test => {
                    let t = TestFn {
                        ignored: f.attrs.iter().any(is_ignore_attr),
                        gate: Gate::new(),
                    };
                    unit.tests.entry(name.clone()).or_default().push(t);
                }
                Item::Mod(m) => match &m.content {
                    Some((_, inner)) => {
                        collect_tests(inner, "", &Gate::new(), &mut unit.tests);
                        let mut inl = inline.to_vec();
                        inl.push(name.clone());
                        collect_decls(file, inner, &inl, "", &Gate::new(), &mut unit.decls);
                    }
                    None => {
                        unit.outline = Some(Decl {
                            file: file.to_string(),
                            inline: inline.to_vec(),
                            name: name.clone(),
                            path: path_value(&m.attrs),
                            modpath: name.clone(),
                            gate: Gate::new(),
                        });
                    }
                },
                _ => {}
            }
            if unit.outline.is_none() {
                let (l, inc) = walk_tokens(file, &toks);
                unit.literals = l;
                unit.includes = inc;
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
            let g = union(gate, &gate_of(&m.attrs));
            src_units(file, inner, &path, &inl, &g, out)?;
        }
    }
    Ok(())
}

/// A parsed `.rs` file.
struct Parsed {
    ts: TokenStream,
    file: syn::File,
}

fn parse(file: &str, src: &str) -> Result<Parsed, String> {
    let ts: TokenStream = src.parse().map_err(|e| format!("unreadable {file}: {e}"))?;
    let parsed = syn::parse_file(src).map_err(|e| format!("unreadable {file}: {e}"))?;
    Ok(Parsed { ts, file: parsed })
}

/// A whole file as one unit: its tests, literals, includes and out-of-line modules. Its gate is
/// left for the chain.
fn whole_file(file: &str, p: &Parsed) -> Unit {
    let mut unit = Unit {
        file: file.to_string(),
        ..Unit::default()
    };
    collect_tests(&p.file.items, "", &Gate::new(), &mut unit.tests);
    collect_decls(file, &p.file.items, &[], "", &Gate::new(), &mut unit.decls);
    let toks: Vec<TokenTree> = p.ts.clone().into_iter().collect();
    let (l, inc) = walk_tokens(file, &toks);
    unit.literals = l;
    unit.includes = inc;
    unit
}

/// True when `file` is a whole-file unit.
fn is_whole(file: &str, p: &Parsed, ctx: &Ctx) -> bool {
    is_test_path(file, ctx) || requires_test(&p.file.attrs)
}

/// The units of a parsed file, before the chain: a whole-file unit for a test file or a file
/// whose inner `cfg` requires `test` (F9), else its test items.
fn file_units(file: &str, p: &Parsed, ctx: &Ctx) -> Result<Vec<Unit>, String> {
    if is_whole(file, p, ctx) {
        return Ok(vec![whole_file(file, p)]);
    }
    let split = split_items(p.ts.clone()).map_err(|e| format!("unreadable {file}: {e}"))?;
    let mut out = Vec::new();
    src_units(file, split, "", &[], &Gate::new(), &mut out)?;
    Ok(out)
}

/// The units of one file with no manifest and no chain (unit tests).
#[cfg(test)]
fn units(file: &str, src: &str) -> Result<Vec<Unit>, String> {
    let p = parse(file, src)?;
    let mut us = file_units(file, &p, &Ctx::default())?;
    for u in &mut us {
        if u.path.is_empty() {
            u.gate = gate_of(&p.file.attrs);
        }
    }
    Ok(us)
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
    let mut seen = BTreeMap::new();
    for item in items {
        let path = join(prefix, &item_name(item, &mut seen));
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

/// The doc-test code blocks of a parsed file by item path; `self` for its inner docs (F7).
fn doc_blocks_of(p: &syn::File) -> Docs {
    let mut out = Docs::new();
    add_doc(&mut out, "self", &p.attrs);
    collect_docs(&p.items, "", &mut out);
    out
}

#[cfg(test)]
fn doc_blocks(file: &str, src: &str) -> Result<Docs, String> {
    Ok(doc_blocks_of(&parse(file, src)?.file))
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
        if out[i].is_some() || p.path.is_empty() || p.outline.is_some() {
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

/// A Markdown heading: its level and its text, closing hashes removed (D11).
fn heading(line: &str) -> Option<(usize, &str)> {
    let level = line.chars().take_while(|c| *c == '#').count();
    if level == 0 || level > 6 {
        return None;
    }
    let rest = &line[level..];
    if !(rest.is_empty() || rest.starts_with(' ')) {
        return None;
    }
    Some((level, rest.trim().trim_end_matches('#').trim_end()))
}

/// Reads the entries under `## Test logic edits`: each entry with its line, and the
/// `unreadable entry` and section offences. The section ends at the next heading of level 1 or 2
/// (D11); inside it, each non-blank line other than `None.` and an entry is unreadable (F11,
/// D11: fail closed); a second section is an offence.
fn entries(node: &str) -> (Vec<(Entry, String)>, Vec<String>) {
    let mut out = Vec::new();
    let mut bad = Vec::new();
    let mut inside = false;
    let mut sections = 0;
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
        if let Some((level, title)) = heading(line)
            && level <= 2
        {
            inside = level == 2 && title == "Test logic edits";
            if inside {
                sections += 1;
                if sections == 2 {
                    bad.push("duplicate section: ## Test logic edits".to_string());
                }
            }
            continue;
        }
        let t = line.trim();
        if !inside || t.is_empty() || t == "None." {
            continue;
        }
        match line.strip_prefix("- ").and_then(read_entry) {
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
    let (kind, arg) = head.split_once(' ')?;
    match kind {
        "removed" => Some(Entry::Removed { id: one_id(arg)? }),
        "ignore" => Some(Entry::Ignore { id: one_id(arg)? }),
        "checker" => Some(Entry::Checker { path: one_id(arg)? }),
        "fixture" => Some(Entry::Fixture { path: one_id(arg)? }),
        "doctest" => Some(Entry::Doctest { id: one_id(arg)? }),
        "manifest" => Some(Entry::Manifest { path: one_id(arg)? }),
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
    /// `checker`, `fixture`, `doctest` and `manifest` offences: cleared by the entry of the same
    /// text.
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
fn ignore_text(id: &str, newly_ignored: bool, from: &Gate, to: &Gate) -> String {
    let (f, t) = (gate_text(from), gate_text(to));
    match (newly_ignored, from == to) {
        (true, true) => format!("ignore {id}"),
        (true, false) => format!("ignore {id} (#[ignore] added; attributes `{f}` -> `{t}`)"),
        _ => format!("ignore {id} (attributes `{f}` -> `{t}`)"),
    }
}

/// Pairs the functions of one test id at `<parent>` with those at `HEAD`: equal gates first,
/// then in order (D10). Returns the pairs and the `<parent>` functions left over.
fn pair_tests<'a>(p: &'a [TestFn], h: &'a [TestFn]) -> (Vec<(&'a TestFn, &'a TestFn)>, usize) {
    let mut h_left: Vec<&TestFn> = h.iter().collect();
    let mut p_left: Vec<&TestFn> = Vec::new();
    let mut pairs = Vec::new();
    for pt in p {
        match h_left.iter().position(|ht| ht.gate == pt.gate) {
            Some(k) => pairs.push((pt, h_left.remove(k))),
            None => p_left.push(pt),
        }
    }
    let mut left = 0;
    for pt in p_left {
        if h_left.is_empty() {
            left += 1;
        } else {
            pairs.push((pt, h_left.remove(0)));
        }
    }
    (pairs, left)
}

/// The offences of `parent` against its matches in `head`, and the `extra` offences
/// (`checker`, `fixture`, `doctest`, `manifest`), after the node's entries.
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
        if p.outline.is_some() {
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
        for (t, pts) in &p.tests {
            let hts = h_tests.get(t).map_or(&[][..], Vec::as_slice);
            let (pairs, left) = pair_tests(pts, hts);
            let id = format!("{base}::{t}");
            for _ in 0..left {
                push(
                    &mut list,
                    format!("removed {id}"),
                    id.clone(),
                    Kind::Removed { unit: i },
                );
            }
            for (pt, ht) in pairs {
                let newly = ht.ignored && !pt.ignored;
                if newly || ht.gate != pt.gate {
                    let text = ignore_text(&id, newly, &pt.gate, &ht.gate);
                    push(&mut list, text, id.clone(), Kind::Ignore);
                }
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
                    (Entry::Manifest { path }, Kind::Plain) => o.id == format!("manifest {path}"),
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

/// The tables of a Cargo manifest: each `[x]` or `[[x]]` header with its `key = value` lines.
/// A subset of TOML: one key per line, full-line comments; a value that spans lines keeps its
/// first line.
fn manifest_tables(text: &str) -> Vec<(String, Vec<(String, String)>)> {
    let mut out: Vec<(String, Vec<(String, String)>)> = vec![(String::new(), Vec::new())];
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            let h = line.trim_start_matches('[');
            let h = h.split(']').next().unwrap_or("").trim().to_string();
            let h = if line.starts_with("[[") {
                format!("[[{h}]]")
            } else {
                h
            };
            out.push((h, Vec::new()));
            continue;
        }
        if let Some((k, v)) = line.split_once('=')
            && let Some(last) = out.last_mut()
        {
            last.1.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    out
}

fn unquote(v: &str) -> &str {
    v.trim().trim_matches('"').trim_matches('\'')
}

const TARGETS: [&str; 5] = ["lib", "[[bin]]", "[[test]]", "[[bench]]", "[[example]]"];

/// The test-relevant settings of a manifest (D3): `package.autotests`, every `[[test]]` key, and
/// the `test`, `doctest` and `harness` keys of each target, as sorted text.
fn manifest_settings(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (h, kvs) in manifest_tables(text) {
        let name = kvs
            .iter()
            .find(|(k, _)| k == "name" || k == "path")
            .map_or("", |(_, v)| unquote(v));
        for (k, v) in &kvs {
            let keep = (h == "package" && k == "autotests")
                || h == "[[test]]"
                || (TARGETS.contains(&h.as_str())
                    && matches!(k.as_str(), "test" | "doctest" | "harness"));
            if keep {
                out.push(format!(
                    "{h}{{{name}}}.{k} = {}",
                    v.split_whitespace().collect::<Vec<_>>().join(" ")
                ));
            }
        }
    }
    out.sort();
    out
}

/// The crate roots and test targets that a manifest at `path` names (D2, D7).
fn manifest_paths(path: &str, text: &str, ctx: &mut Ctx) {
    for (h, kvs) in manifest_tables(text) {
        if !TARGETS.contains(&h.as_str()) {
            continue;
        }
        if let Some((_, v)) = kvs.iter().find(|(k, _)| k == "path") {
            let file = resolve(path, unquote(v));
            if h == "[[test]]" {
                ctx.test_targets.insert(file.clone());
            }
            ctx.roots.insert(file);
        }
    }
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

    fn show(&self, rev: &str, path: &str) -> Result<Vec<u8>, String> {
        self.run(&["show", &format!("{rev}:{path}")])
    }

    /// The blobs `<rev>:<path>` of many paths through one `git cat-file --batch`; `None` for a
    /// missing one.
    fn show_many(&self, rev: &str, paths: &[String]) -> Result<Vec<Option<Vec<u8>>>, String> {
        use std::io::{BufRead, BufReader, Read, Write};
        use std::process::Stdio;
        let mut child = Command::new("git")
            .arg("-C")
            .arg(&self.top)
            .args(["cat-file", "--batch"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| format!("git cat-file --batch: {e}"))?;
        let mut stdin = child.stdin.take().ok_or("git cat-file: no stdin")?;
        let input: String = paths.iter().map(|p| format!("{rev}:{p}\n")).collect();
        // Write from a thread, so a full output pipe cannot block the input.
        let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()));
        let mut out = BufReader::new(child.stdout.take().ok_or("git cat-file: no stdout")?);
        let mut blobs = Vec::with_capacity(paths.len());
        for p in paths {
            let mut header = String::new();
            out.read_line(&mut header)
                .map_err(|e| format!("git cat-file {rev}:{p}: {e}"))?;
            let fields: Vec<&str> = header.split_whitespace().collect();
            if fields.len() != 3 {
                blobs.push(None);
                continue;
            }
            let size: usize = fields[2]
                .parse()
                .map_err(|_| format!("git cat-file {rev}:{p}: bad header {header:?}"))?;
            let mut buf = vec![0; size + 1];
            out.read_exact(&mut buf)
                .map_err(|e| format!("git cat-file {rev}:{p}: {e}"))?;
            buf.pop();
            blobs.push(Some(buf));
        }
        writer
            .join()
            .map_err(|_| "git cat-file: writer failed".to_string())?
            .map_err(|e| format!("git cat-file: {e}"))?;
        let status = child.wait().map_err(|e| format!("git cat-file: {e}"))?;
        if !status.success() {
            return Err("git cat-file --batch failed".to_string());
        }
        Ok(blobs)
    }
}

/// What the check reads at one revision.
#[derive(Default)]
struct Rev {
    units: Vec<Unit>,
    docs: BTreeMap<String, Docs>,
    /// `.rs` files in scope that do not parse: fixtures (D6).
    unparsed: BTreeSet<String>,
    /// Files named by `include_str!`/`include_bytes!` in a unit (D2).
    includes: BTreeSet<String>,
    bad: Vec<String>,
}

/// True for a `.rs` file the check reads: under a crate's `src/` or `tests/`, the root `tests/`,
/// or named by a manifest. The check's own files are not read: a `checker` entry lists any change
/// to them, their unit tests included.
fn in_scope(file: &str, ctx: &Ctx) -> bool {
    let p = parts(file);
    !file.starts_with(&format!("{}/", CHECKER[0]))
        && file.ends_with(".rs")
        && ((p.len() >= 4 && TOPS.contains(&p[0]) && (p[2] == "src" || p[2] == "tests"))
            || (p.len() >= 2 && p[0] == "tests")
            || ctx.roots.contains(file))
}

/// The chain of a file (D3): its inner attributes and, for each declaration of it, the
/// declaration's attributes and the chain of the declaring file, up to the crate root.
fn chain(
    file: &str,
    parsed: &BTreeMap<String, Parsed>,
    declarers: &BTreeMap<String, Vec<Decl>>,
    memo: &mut BTreeMap<String, Gate>,
    visiting: &mut BTreeSet<String>,
) -> Gate {
    if let Some(g) = memo.get(file) {
        return g.clone();
    }
    if !visiting.insert(file.to_string()) {
        return Gate::new();
    }
    let mut g = parsed
        .get(file)
        .map_or_else(Gate::new, |p| gate_of(&p.file.attrs));
    for d in declarers.get(file).map_or(&[][..], Vec::as_slice) {
        g = union(&g, &d.gate);
        let up = chain(&d.file, parsed, declarers, memo, visiting);
        g = union(&g, &up);
    }
    visiting.remove(file);
    memo.insert(file.to_string(), g.clone());
    g
}

/// The first existing target of a declaration.
fn target_of(d: &Decl, ctx: &Ctx, exists: &BTreeSet<String>) -> Option<String> {
    decl_targets(d, ctx)
        .into_iter()
        .find(|t| exists.contains(t))
}

/// Merges the out-of-line modules declared in a unit into it, recursively, as test code (D8).
/// A body file that is a test file of its own is left to its own unit.
#[allow(clippy::too_many_arguments)]
fn merge_decls(
    unit: &mut Unit,
    decls: &[Decl],
    prefix: &str,
    gate: &Gate,
    parsed: &BTreeMap<String, Parsed>,
    ctx: &Ctx,
    exists: &BTreeSet<String>,
    consumed: &mut BTreeSet<String>,
    visited: &mut BTreeSet<String>,
    depth: usize,
) {
    if depth > 32 {
        return;
    }
    for d in decls {
        let Some(t) = target_of(d, ctx, exists) else {
            continue;
        };
        if is_test_path(&t, ctx) || !visited.insert(t.clone()) {
            continue;
        }
        let Some(p) = parsed.get(&t) else {
            continue;
        };
        consumed.insert(t.clone());
        let body = whole_file(&t, p);
        let g = union(&union(gate, &d.gate), &gate_of(&p.file.attrs));
        let mp = join(prefix, &d.modpath);
        for (id, fns) in body.tests {
            for mut f in fns {
                f.gate = union(&f.gate, &g);
                unit.tests.entry(join(&mp, &id)).or_default().push(f);
            }
        }
        for (k, n) in body.literals {
            *unit.literals.entry(k).or_insert(0) += n;
        }
        unit.includes.extend(body.includes);
        merge_decls(
            unit,
            &body.decls,
            &mp,
            &g,
            parsed,
            ctx,
            exists,
            consumed,
            visited,
            depth + 1,
        );
    }
}

fn read_rev(git: &Git, rev: &str) -> Result<Rev, String> {
    let all = git.paths(&["ls-tree", "-r", "--name-only", "-z", rev])?;
    let mut ctx = Ctx::default();
    let manifests: Vec<String> = all
        .iter()
        .filter(|p| *p == "Cargo.toml" || p.ends_with("/Cargo.toml"))
        .cloned()
        .collect();
    for (m, raw) in manifests.iter().zip(git.show_many(rev, &manifests)?) {
        let text = String::from_utf8_lossy(&raw.unwrap_or_default()).into_owned();
        manifest_paths(m, &text, &mut ctx);
    }
    let mut out = Rev::default();
    let mut parsed: BTreeMap<String, Parsed> = BTreeMap::new();
    let exists: BTreeSet<String> = all.iter().filter(|p| p.ends_with(".rs")).cloned().collect();
    let files: Vec<String> = all.iter().filter(|f| in_scope(f, &ctx)).cloned().collect();
    for (file, raw) in files.iter().zip(git.show_many(rev, &files)?) {
        let raw = raw.ok_or_else(|| format!("git cat-file {rev}:{file}: missing"))?;
        match String::from_utf8(raw)
            .map_err(|_| format!("unreadable {file}: not UTF-8"))
            .and_then(|src| parse(file, &src))
        {
            Ok(p) => {
                parsed.insert(file.clone(), p);
            }
            Err(_) => {
                out.unparsed.insert(file.clone());
            }
        }
    }
    // Every declaration, and the files loaded by #[path] (they place children as mod.rs does).
    let mut decls: Vec<Decl> = Vec::new();
    for (file, p) in &parsed {
        collect_decls(file, &p.file.items, &[], "", &Gate::new(), &mut decls);
    }
    for _ in 0..8 {
        let before = ctx.modrs_like.len();
        for d in decls.iter().filter(|d| d.path.is_some()) {
            if let Some(t) = target_of(d, &ctx, &exists) {
                ctx.modrs_like.insert(t);
            }
        }
        if ctx.modrs_like.len() == before {
            break;
        }
    }
    let mut declarers: BTreeMap<String, Vec<Decl>> = BTreeMap::new();
    for d in &decls {
        if let Some(t) = target_of(d, &ctx, &exists) {
            declarers.entry(t).or_default().push(d.clone());
        }
    }
    let mut memo = BTreeMap::new();
    let mut per_file: Vec<Unit> = Vec::new();
    let mut consumed = BTreeSet::new();
    for (file, p) in &parsed {
        out.docs.insert(file.clone(), doc_blocks_of(&p.file));
        let us = match file_units(file, p, &ctx) {
            Ok(u) => u,
            Err(e) => {
                out.bad.push(e);
                continue;
            }
        };
        for mut u in us {
            let decls = std::mem::take(&mut u.decls);
            if let Some(d) = u.outline.clone() {
                match target_of(&d, &ctx, &exists) {
                    // A body file under a tests/ folder is a unit of its own.
                    Some(t) if is_test_path(&t, &ctx) => continue,
                    Some(t) => {
                        if let Some(bp) = parsed.get(&t) {
                            consumed.insert(t.clone());
                            let body = whole_file(&t, bp);
                            u.tests = body.tests;
                            u.literals = body.literals;
                            u.includes = body.includes;
                            u.outline = None;
                            u.gate =
                                chain(&t, &parsed, &declarers, &mut memo, &mut BTreeSet::new());
                            let g = Gate::new();
                            let mut visited = BTreeSet::from([t.clone()]);
                            merge_decls(
                                &mut u,
                                &body.decls,
                                "",
                                &g,
                                &parsed,
                                &ctx,
                                &exists,
                                &mut consumed,
                                &mut visited,
                                0,
                            );
                        }
                    }
                    None => {}
                }
                if u.outline.is_some() {
                    per_file.push(u);
                    continue;
                }
            } else {
                let c = chain(file, &parsed, &declarers, &mut memo, &mut BTreeSet::new());
                u.gate = union(&c, &u.gate);
                let g = Gate::new();
                let mut visited = BTreeSet::from([file.clone()]);
                merge_decls(
                    &mut u,
                    &decls,
                    "",
                    &g,
                    &parsed,
                    &ctx,
                    &exists,
                    &mut consumed,
                    &mut visited,
                    0,
                );
            }
            per_file.push(u);
        }
    }
    // A body file merged into a unit is not a unit of its own.
    out.units = per_file
        .into_iter()
        .filter(|u| !consumed.contains(&u.file))
        .collect();
    for u in &out.units {
        out.includes.extend(u.includes.iter().cloned());
    }
    Ok(out)
}

/// Old path → new path for each rename from `parent` to `HEAD`.
fn renames(git: &Git, parent: &str) -> Result<BTreeMap<String, String>, String> {
    let fields = git.paths(&["diff", "-M", "-z", "--name-status", parent, "HEAD"])?;
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

/// True for a fixture path by place (D2): a non-`.rs` file under any `tests/` folder, or under
/// the root `fixtures/`.
fn is_fixture_path(p: &str) -> bool {
    !p.ends_with(".rs")
        && (p.starts_with("fixtures/") || p.split('/').rev().skip(1).any(|s| s == "tests"))
}

/// The offences found from the changed files: `fixture` (D2, D6, F8), `manifest` (D3) and
/// `checker` (F5).
fn changed_files(git: &Git, parent: &str, p: &Rev, h: &Rev) -> Result<Vec<String>, String> {
    let changed = git.paths(&["diff", "--no-renames", "--name-only", "-z", parent, "HEAD"])?;
    let mut out = Vec::new();
    for c in &changed {
        let fixture = is_fixture_path(c)
            || p.includes.contains(c)
            || h.includes.contains(c)
            || p.unparsed.contains(c)
            || h.unparsed.contains(c);
        if fixture {
            out.push(format!("fixture {c}"));
        }
        if c == "Cargo.toml" || c.ends_with("/Cargo.toml") {
            let read = |rev: &str| {
                git.show(rev, c)
                    .map(|b| manifest_settings(&String::from_utf8_lossy(&b)))
                    .unwrap_or_default()
            };
            if read(parent) != read("HEAD") {
                out.push(format!("manifest {c}"));
            }
        }
    }
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
    sargs.extend(["tests", "fixtures"]);
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
    extra.extend(changed_files(git, parent, &p, &h)?);
    let (es, bad_entries) = entries(node);
    let mut out = p.bad.clone();
    out.extend(h.bad.iter().cloned());
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

    fn gate(items: &[&str]) -> Gate {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn tests_of(names: &[&str]) -> BTreeMap<String, Vec<TestFn>> {
        names
            .iter()
            .map(|n| (n.to_string(), vec![TestFn::default()]))
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

    fn first(u: &Unit, id: &str) -> TestFn {
        u.tests[id][0].clone()
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
    fn includes_are_recorded() {
        let src = "#[cfg(test)] mod tests { #[test] fn g() { \
                   assert_eq!(include_str!(\"testdata/g.txt\"), \"v1\"); \
                   let _ = include_bytes!(\"../data.bin\"); } }";
        let u = units("crates/a/src/lib.rs", src).expect("units");
        assert_eq!(
            u[0].includes,
            BTreeSet::from([
                "crates/a/src/testdata/g.txt".to_string(),
                "crates/a/data.bin".to_string()
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
        assert_eq!(u[0].gate, gate(&["#[cfg(test)]"]));
        assert!(u[0].literals.is_empty());
        assert_eq!(u[1].path, "helper");
        assert!(u[1].tests.is_empty());
        assert_eq!(u[1].literals, lits(&[("1", 1)]));
    }

    #[test]
    fn enclosing_plain_module_gate_is_in_the_unit() {
        let src = "#[cfg(unix)] pub mod a { #[cfg(test)] mod tests { #[test] fn t() {} } }";
        let u = units("crates/a/src/lib.rs", src).expect("units");
        assert_eq!(u[0].path, "a::tests");
        assert_eq!(u[0].gate, gate(&["#[cfg(test)]", "#[cfg(unix)]"]));
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
        assert_eq!(u[0].gate, gate(&["#[cfg(test)]"]));
        assert_eq!(u[0].tests, tests_of(&["inner"]));
        assert_eq!(u[0].literals, lits(&[("1", 2)]));
    }

    #[test]
    fn gates_of_tests_and_modules() {
        let src = "#![cfg(unix)]\n#[cfg(windows)] #[test] fn a() {}\n\
                   #[cfg_attr(miri, ignore)] mod m { #[test] #[should_panic] fn b() {} }\n";
        let u = units("crates/a/tests/t.rs", src).expect("units");
        assert_eq!(u[0].gate, gate(&["#[cfg(unix)]"]));
        assert_eq!(first(&u[0], "a").gate, gate(&["#[cfg(windows)]"]));
        assert!(!first(&u[0], "a").ignored);
        assert_eq!(
            first(&u[0], "m::b").gate,
            gate(&["#[cfg_attr(miri , ignore)]", "#[should_panic]"])
        );
    }

    #[test]
    fn gate_is_order_and_layout_free() {
        let a = units(
            "crates/a/tests/t.rs",
            "#[test] #[cfg(unix)] #[should_panic] #[cfg_attr(windows,ignore = \"x\")] fn a() {}",
        )
        .expect("a");
        let b = units(
            "crates/a/tests/t.rs",
            "#[should_panic]\n#[cfg_attr(\n    windows,\n    ignore = \"x\"\n)]\n#[test]\n#[cfg(unix)]\nfn a() {}",
        )
        .expect("b");
        assert_eq!(first(&a[0], "a").gate, first(&b[0], "a").gate);
    }

    #[test]
    fn same_name_tests_under_two_cfgs() {
        let src = "#[cfg(unix)] #[test] fn a() {} #[cfg(windows)] #[test] fn a() {}";
        let u = units("crates/a/tests/t.rs", src).expect("units");
        assert_eq!(u[0].tests["a"].len(), 2);
        let mut head = u[0].clone();
        head.tests.get_mut("a").expect("a").remove(0);
        let parent = vec![u[0].clone()];
        let head = vec![head];
        let m = match_units(&parent, &head, &BTreeMap::new());
        assert_eq!(
            offences(&parent, &head, &m, &[], &[]),
            vec!["removed crates/a/tests/t.rs::a".to_string()]
        );
    }

    #[test]
    fn out_of_line_test_module_is_recorded() {
        let src = "mod outer { #[cfg(test)] mod tests { #[test] fn a() { let _ = 2; } } } \
                   #[cfg(test)] mod elsewhere;";
        let u = units("crates/a/src/lib.rs", src).expect("units");
        assert_eq!(u.len(), 2);
        assert_eq!(u[0].path, "outer::tests");
        assert_eq!(u[0].tests, tests_of(&["a"]));
        assert_eq!(u[0].literals, lits(&[("2", 1)]));
        assert!(u[0].outline.is_none());
        assert_eq!(u[1].path, "elsewhere");
        let d = u[1].outline.clone().expect("outline");
        assert_eq!(
            decl_targets(&d, &Ctx::default()),
            vec!["crates/a/src/elsewhere.rs", "crates/a/src/elsewhere/mod.rs"]
        );
    }

    fn decl(file: &str, inline: &[&str], name: &str, path: Option<&str>) -> Decl {
        Decl {
            file: file.into(),
            inline: inline.iter().map(|s| s.to_string()).collect(),
            name: name.into(),
            path: path.map(str::to_string),
            ..Decl::default()
        }
    }

    #[test]
    fn module_targets_follow_rust_rules() {
        let c = Ctx::default();
        assert_eq!(
            decl_targets(&decl("crates/a/src/cmd/up.rs", &[], "t", None), &c),
            vec!["crates/a/src/cmd/up/t.rs", "crates/a/src/cmd/up/t/mod.rs"]
        );
        assert_eq!(
            decl_targets(&decl("crates/a/src/cmd/mod.rs", &["x"], "t", None), &c),
            vec!["crates/a/src/cmd/x/t.rs", "crates/a/src/cmd/x/t/mod.rs"]
        );
        assert_eq!(
            decl_targets(
                &decl(
                    "crates/a/src/cmd/up.rs",
                    &[],
                    "d",
                    Some("../../tests/dead/mod.rs")
                ),
                &c
            ),
            vec!["crates/a/tests/dead/mod.rs"]
        );
        assert_eq!(
            decl_targets(
                &decl("crates/a/src/a.rs", &["inner"], "t", Some("t.rs")),
                &c
            ),
            vec!["crates/a/src/a/inner/t.rs"]
        );
        // Crate roots by layout (D7).
        assert_eq!(
            decl_targets(&decl("crates/a/src/bin/tool.rs", &[], "tests", None), &c),
            vec!["crates/a/src/bin/tests.rs", "crates/a/src/bin/tests/mod.rs"]
        );
        assert_eq!(
            decl_targets(&decl("crates/a/tests/suite.rs", &[], "m", None), &c),
            vec!["crates/a/tests/m.rs", "crates/a/tests/m/mod.rs"]
        );
        // A crate root by manifest, and a file loaded by #[path].
        let mut c2 = Ctx::default();
        c2.roots.insert("crates/a/tests/stub/stub.rs".into());
        c2.modrs_like.insert("crates/a/src/tst/body.rs".into());
        assert_eq!(
            decl_targets(&decl("crates/a/tests/stub/stub.rs", &[], "m", None), &c2)[0],
            "crates/a/tests/stub/m.rs"
        );
        assert_eq!(
            decl_targets(&decl("crates/a/src/tst/body.rs", &[], "deep", None), &c2)[0],
            "crates/a/src/tst/deep.rs"
        );
    }

    #[test]
    fn test_paths_and_roots() {
        let c = Ctx::default();
        assert!(is_test_path("crates/a/tests/dead/mod.rs", &c));
        assert!(is_test_path("tools/x/tests/n.rs", &c));
        assert!(is_test_path("tests/forbidden_content.rs", &c));
        assert!(!is_test_path("crates/a/src/tests/x.rs", &c));
        assert!(is_root("tests/fc.rs", &c));
        assert!(is_root("crates/a/tests/t.rs", &c));
        assert!(!is_root("crates/a/tests/common/mod.rs", &c));
        assert!(!is_root("crates/a/src/tests/util.rs", &c));
        assert!(is_root("crates/a/src/bin/tool.rs", &c));
        assert!(is_fixture_path("fixtures/codex/x.json"));
        assert!(is_fixture_path("crates/a/tests/fixtures/g.json"));
        assert!(is_fixture_path("tests/data.txt"));
        assert!(!is_fixture_path("crates/a/tests/t.rs"));
        assert!(!is_fixture_path("scripts/fixtures/x.json"));
    }

    #[test]
    fn manifest_reading() {
        let m = "[package]\nname = \"a\"\nversion = \"0.1.0\"\nautotests = false\n\
                 [lib]\ndoctest = false\n\
                 [[test]]\nname = \"fc\"\npath = \"../../tests/fc.rs\"\n\
                 [[bin]]\nname = \"stub\"\npath = \"tests/stub/stub.rs\"\ntest = false\n\
                 [dependencies]\ntest = \"1\"\n";
        assert_eq!(
            manifest_settings(m),
            vec![
                "[[bin]]{stub}.test = false",
                "[[test]]{fc}.name = \"fc\"",
                "[[test]]{fc}.path = \"../../tests/fc.rs\"",
                "lib{}.doctest = false",
                "package{a}.autotests = false",
            ]
        );
        let mut c = Ctx::default();
        manifest_paths("crates/a/Cargo.toml", m, &mut c);
        assert!(c.test_targets.contains("tests/fc.rs"));
        assert!(c.roots.contains("crates/a/tests/stub/stub.rs"));
        assert_eq!(
            manifest_settings("[package]\nname = \"a\"\nversion = \"0.2.0\"\n"),
            Vec::<String>::new()
        );
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
            vec![TestFn {
                ignored: true,
                gate: Gate::new(),
            }],
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
                   impl std::fmt::Display for S {\n    /// ```\n    /// y\n    /// ```\n    fn fmt() {}\n}\n\
                   /// No block.\nfn e() {}\n";
        let d = doc_blocks("crates/a/src/lib.rs", src).expect("docs");
        assert_eq!(d.len(), 4, "{d:?}");
        assert_eq!(d["self"], vec!["```\n a();\n```".to_string()]);
        assert_eq!(d["d"], vec!["```\n assert_eq!(d(2), 4);\n```".to_string()]);
        assert_eq!(d["impl<S>::m"], vec!["```text\n x\n```".to_string()]);
        assert_eq!(
            d["impl<std::fmt::Display>for<S>::fmt"],
            vec!["```\n y\n```".to_string()]
        );
    }

    #[test]
    fn impl_names_tie_break_by_index() {
        let f = syn::parse_file("impl A {} impl B {} impl A {} use x; use y;").expect("file");
        let mut seen = BTreeMap::new();
        let names: Vec<String> = f.items.iter().map(|i| item_name(i, &mut seen)).collect();
        assert_eq!(
            names,
            vec!["impl<A>", "impl<B>", "impl<A>#2", "use#1", "use#2"]
        );
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
                    - manifest crates/a/Cargo.toml — K§9\n\
                    - frob x — y\n\
                    - removed a.rs::f — \n\
                    \x20\x20- removed a.rs::g — indented\n\
                    * removed a.rs::h — star\n\
                    1. removed a.rs::i — numbered\n\
                    removed a.rs::j — plain\n\
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
                Entry::Manifest {
                    path: "crates/a/Cargo.toml".into()
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
                "unreadable entry: 1. removed a.rs::i — numbered".to_string(),
                "unreadable entry: removed a.rs::j — plain".to_string(),
                "unreadable entry: ### Sub".to_string(),
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
            format!("# N\n\n## Test logic edits ##\n\n{line}\n"),
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
        // A heading of level 1 closes the section.
        let h1 = format!("## Test logic edits\n\nNone.\n\n# Appendix\n\n{line}\n");
        let (es, bad) = entries(&h1);
        assert!(es.is_empty() && bad.is_empty(), "{es:?} {bad:?}");
        // A second section is an offence.
        let twice =
            format!("## Test logic edits\n\nNone.\n\n## Plan\n\n## Test logic edits\n\n{line}\n");
        let (_, bad) = entries(&twice);
        assert_eq!(
            bad,
            vec!["duplicate section: ## Test logic edits".to_string()]
        );
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
        head.tests.get_mut("g").expect("g")[0].ignored = true;
        let head = vec![head];
        let m = match_units(&parent, &head, &none);
        assert_eq!(
            offences(&parent, &head, &m, &[], &[]),
            vec![
                "ignore a.rs::g".to_string(),
                "removed a.rs::old".to_string()
            ]
        );
        let (es, _) = node("- renamed a.rs::old -> new — r\n- ignore a.rs::g — r");
        assert!(offences(&parent, &head, &m, &[], &es).is_empty());
        let (wrong, _) = node("- renamed a.rs::old -> g — r");
        assert_eq!(
            offences(&parent, &head, &m, &[], &wrong),
            vec![
                "ignore a.rs::g".to_string(),
                "removed a.rs::old".to_string(),
                "not found: - renamed a.rs::old -> g — r".to_string(),
            ]
        );
    }

    #[test]
    fn gate_change_is_ignore_kind() {
        let none = BTreeMap::new();
        let mut p = unit("a.rs", "tests", &["f"], &[]);
        p.gate = gate(&["#[cfg(test)]"]);
        let mut h = p.clone();
        h.gate = gate(&["#[cfg(all (test , windows))]"]);
        h.tests.get_mut("f").expect("f")[0].gate = gate(&["#[should_panic]"]);
        let (parent, head) = (vec![p], vec![h]);
        let m = match_units(&parent, &head, &none);
        assert_eq!(
            offences(&parent, &head, &m, &[], &[]),
            vec![
                "ignore a.rs::tests (attributes `#[cfg(test)]` -> `#[cfg(all (test , windows))]`)"
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
            "manifest crates/a/Cargo.toml".to_string(),
        ];
        assert_eq!(offences(&[], &[], &[], &extra, &[]), extra);
        let (es, _) = node(
            "- checker scripts/check-test-edits.sh — r\n- fixture crates/a/tests/g.json — r\n\
             - doctest crates/a/src/lib.rs::d — r\n- manifest crates/a/Cargo.toml — r",
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
