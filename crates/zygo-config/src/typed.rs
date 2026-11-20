//! A TOML value into a config type. The error is built from the key path and the types only; a serde error's own
//! text quotes the value, which can be a secret (C3).

use std::fmt;

use serde::de::{self, DeserializeOwned, Expected, IntoDeserializer, Unexpected};
use toml::{Table, Value};

use crate::layers::seg;
use crate::repo_file::one_line;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Seg {
    Key(String),
    Index(usize),
}

/// Where a config value failed and what was expected; holds no value from the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TypeError {
    path: Vec<Seg>,
    what: String,
    /// For a type or value error: the kind of the TOML node that failed (set by the access that holds the node, never
    /// from serde's `Unexpected`) and the text after it.
    found: Option<(Option<&'static str>, &'static str)>,
}

impl fmt::Display for TypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.found {
            None => f.write_str(&self.what),
            Some((kind, rest)) => write!(f, "{}, found {}{rest}", self.what, kind.unwrap_or("a value")),
        }
    }
}

impl std::error::Error for TypeError {}

impl TypeError {
    fn new(what: String) -> Self {
        TypeError { path: Vec::new(), what, found: None }
    }

    fn expected(exp: &dyn Expected, rest: &'static str) -> Self {
        TypeError { path: Vec::new(), what: format!("expected {exp}"), found: Some((None, rest)) }
    }

    /// The node that failed is of kind `k`, unless a deeper node already said so.
    fn kind(mut self, k: &'static str) -> Self {
        if let Some((kind, _)) = &mut self.found {
            kind.get_or_insert(k);
        }
        self
    }

    fn at(mut self, s: Seg) -> Self {
        self.path.insert(0, s);
        self
    }

    /// `<key path>[ at line N]: <what>`. An array element is named by its index, never by an id, name or other value
    /// (C3); `text`, when given, is the TOML source and gives the line.
    pub(crate) fn describe(&self, text: Option<&str>) -> String {
        let mut out = String::new();
        for s in &self.path {
            match s {
                Seg::Key(k) => {
                    if !out.is_empty() {
                        out.push('.');
                    }
                    out.push_str(&seg(k));
                }
                Seg::Index(i) => out.push_str(&format!("[{i}]")),
            }
        }
        if out.is_empty() {
            out.push_str("(top level)");
        }
        let at = text.and_then(|t| line_of(t, &self.path)).map(|n| format!(" at line {n}")).unwrap_or_default();
        format!("{}{at}: {}", zygo_core::esc(&out), zygo_core::esc(&one_line(self)))
    }
}

/// The 1-based line of the value at `path` in `text`.
fn line_of(text: &str, path: &[Seg]) -> Option<usize> {
    let doc = toml::de::DeTable::parse(text).ok()?;
    let mut span = doc.span();
    let mut cur = toml::de::DeValue::Table(doc.into_inner());
    for s in path {
        let next = match (s, cur) {
            (Seg::Key(k), toml::de::DeValue::Table(t)) => t.get(k.as_str())?.clone(),
            (Seg::Index(i), toml::de::DeValue::Array(a)) => a.get(*i)?.clone(),
            _ => return None,
        };
        span = next.span();
        cur = next.into_inner();
    }
    Some(text.get(..span.start)?.matches('\n').count() + 1)
}

impl de::Error for TypeError {
    fn custom<T: fmt::Display>(_msg: T) -> Self {
        TypeError::new("invalid value".into())
    }
    fn invalid_type(_unexp: Unexpected, exp: &dyn Expected) -> Self {
        TypeError::expected(exp, "")
    }
    fn invalid_value(_unexp: Unexpected, exp: &dyn Expected) -> Self {
        TypeError::expected(exp, " that is not valid")
    }
    fn invalid_length(len: usize, exp: &dyn Expected) -> Self {
        TypeError::new(format!("expected {exp}, found {len} elements"))
    }
    fn unknown_variant(_variant: &str, expected: &'static [&'static str]) -> Self {
        let names: Vec<String> = expected.iter().map(|v| format!("`{v}`")).collect();
        TypeError::new(format!("expected one of {}, found another string", names.join(", ")))
    }
    fn unknown_field(field: &str, _expected: &'static [&'static str]) -> Self {
        TypeError::new("unknown key".into()).at(Seg::Key(field.into()))
    }
    fn missing_field(field: &'static str) -> Self {
        TypeError::new("missing".into()).at(Seg::Key(field.into()))
    }
    fn duplicate_field(field: &'static str) -> Self {
        TypeError::new("duplicate key".into()).at(Seg::Key(field.into()))
    }
}

/// `v` as `T`, as `Value::try_into` gives it (the same visits as `toml::Value`'s deserializer), with a `TypeError`.
pub(crate) fn from_value<T: DeserializeOwned>(v: Value) -> Result<T, TypeError> {
    let k = kind(&v);
    T::deserialize(De(v)).map_err(|e| e.kind(k))
}

struct De(Value);

impl<'de> de::Deserializer<'de> for De {
    type Error = TypeError;

    fn deserialize_any<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, TypeError> {
        match self.0 {
            Value::Boolean(b) => visitor.visit_bool(b),
            Value::Integer(n) => visitor.visit_i64(n),
            Value::Float(n) => visitor.visit_f64(n),
            Value::String(s) => visitor.visit_string(s),
            Value::Datetime(d) => visitor.visit_string(d.to_string()),
            Value::Array(a) => {
                let len = a.len();
                let mut seq = Seq { iter: a.into_iter(), i: 0 };
                let out = visitor.visit_seq(&mut seq)?;
                if seq.iter.len() == 0 { Ok(out) } else { Err(de::Error::invalid_length(len, &"fewer elements in array")) }
            }
            Value::Table(t) => {
                let len = t.len();
                let mut map = Map { iter: t.into_iter(), value: None };
                let out = visitor.visit_map(&mut map)?;
                if map.iter.len() == 0 { Ok(out) } else { Err(de::Error::invalid_length(len, &"fewer elements in map")) }
            }
        }
    }

    fn deserialize_enum<V: de::Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, TypeError> {
        match self.0 {
            Value::String(s) => visitor.visit_enum(s.into_deserializer()),
            // toml also takes a table with one key, the variant (`value.rs` `MapEnumDeserializer`).
            Value::Table(t) if t.len() == 1 => visitor.visit_enum(Enum(t.into_iter().next().expect("one key"))),
            Value::Table(t) => Err(de::Error::invalid_length(t.len(), &"a table with one key")),
            _ => Err(de::Error::invalid_type(Unexpected::Other(""), &"a string")),
        }
    }

    fn deserialize_option<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, TypeError> {
        visitor.visit_some(self)
    }

    fn deserialize_newtype_struct<V: de::Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, TypeError> {
        visitor.visit_newtype_struct(self)
    }

    serde::forward_to_deserialize_any! {
        bool u8 u16 u32 u64 i8 i16 i32 i64 f32 f64 char str string unit seq
        bytes byte_buf map unit_struct tuple_struct struct
        tuple ignored_any identifier
    }
}

/// The noun for the kind of a TOML node; never its value.
fn kind(v: &Value) -> &'static str {
    match v {
        Value::Boolean(_) => "a boolean",
        Value::Integer(_) => "an integer",
        Value::Float(_) => "a float",
        Value::String(_) => "a string",
        Value::Datetime(_) => "a datetime",
        Value::Array(_) => "an array",
        Value::Table(_) => "a table",
    }
}

/// An enum given as a table with one key: the variant and its value.
struct Enum((String, Value));

impl<'de> de::EnumAccess<'de> for Enum {
    type Error = TypeError;
    type Variant = Enum;

    fn variant_seed<V: de::DeserializeSeed<'de>>(self, seed: V) -> Result<(V::Value, Enum), TypeError> {
        let v = seed.deserialize(self.0.0.clone().into_deserializer()).map_err(|e: TypeError| e.kind("a string"))?;
        Ok((v, self))
    }
}

impl<'de> de::VariantAccess<'de> for Enum {
    type Error = TypeError;

    /// As toml: the value is an empty table or an empty array.
    fn unit_variant(self) -> Result<(), TypeError> {
        let (k, v) = self.0;
        match &v {
            Value::Array(a) if a.is_empty() => Ok(()),
            Value::Table(t) if t.is_empty() => Ok(()),
            _ => Err(TypeError::expected(&"an empty table or array", "").kind(kind(&v)).at(Seg::Key(k))),
        }
    }

    fn newtype_variant_seed<T: de::DeserializeSeed<'de>>(self, seed: T) -> Result<T::Value, TypeError> {
        let (k, v) = self.0;
        let n = kind(&v);
        seed.deserialize(De(v)).map_err(|e| e.kind(n).at(Seg::Key(k)))
    }

    // The schema has no enum with data, so these are unreachable today. They are simpler than toml's
    // `MapEnumDeserializer` (no length check; no `{ "0" = .., "1" = .. }` tables, `toml-1.2.1/src/value.rs:793-830`).
    fn tuple_variant<V: de::Visitor<'de>>(self, _len: usize, visitor: V) -> Result<V::Value, TypeError> {
        let (k, v) = self.0;
        let n = kind(&v);
        de::Deserializer::deserialize_seq(De(v), visitor).map_err(|e| e.kind(n).at(Seg::Key(k)))
    }

    fn struct_variant<V: de::Visitor<'de>>(
        self,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, TypeError> {
        let (k, v) = self.0;
        let n = kind(&v);
        de::Deserializer::deserialize_struct(De(v), "", fields, visitor).map_err(|e| e.kind(n).at(Seg::Key(k)))
    }
}

struct Seq {
    iter: std::vec::IntoIter<Value>,
    i: usize,
}

impl<'de> de::SeqAccess<'de> for Seq {
    type Error = TypeError;

    fn next_element_seed<T: de::DeserializeSeed<'de>>(&mut self, seed: T) -> Result<Option<T::Value>, TypeError> {
        let Some(v) = self.iter.next() else { return Ok(None) };
        let i = self.i;
        self.i += 1;
        let n = kind(&v);
        seed.deserialize(De(v)).map(Some).map_err(|e| e.kind(n).at(Seg::Index(i)))
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.iter.len())
    }
}

struct Map {
    iter: <Table as IntoIterator>::IntoIter,
    value: Option<(String, Value)>,
}

impl<'de> de::MapAccess<'de> for Map {
    type Error = TypeError;

    fn next_key_seed<T: de::DeserializeSeed<'de>>(&mut self, seed: T) -> Result<Option<T::Value>, TypeError> {
        let Some((k, v)) = self.iter.next() else { return Ok(None) };
        self.value = Some((k.clone(), v));
        seed.deserialize(De(Value::String(k))).map(Some).map_err(|e| e.kind("a string"))
    }

    fn next_value_seed<T: de::DeserializeSeed<'de>>(&mut self, seed: T) -> Result<T::Value, TypeError> {
        let (k, v) = self.value.take().ok_or_else(|| <TypeError as de::Error>::custom("value is missing"))?;
        let n = kind(&v);
        seed.deserialize(De(v)).map_err(|e| e.kind(n).at(Seg::Key(k)))
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.iter.len())
    }
}

#[cfg(test)]
mod diff {
    use super::*;
    use crate::schema::Config;

    fn sample() -> Table {
        let mut t = Table::try_from(crate::builtin::defaults()).unwrap();
        let extra: Table = r#"
[[model]]
id = "x"
soft_context_window = 10
[model.facts]
context_window = 100
max_output_tokens = 5
verified_on = 2026-11-18
[[model.provider]]
name = "p"
base_url = "u"
upstream_model = "m"
key = "env:K"
forward_headers = ["a"]
[model.provider.flags]
zdr = true
odd = 1
when = 2026-11-18T10:00:00Z
[model.provider.price_usd_per_mtok]
input = 1.5
[profile.p]
prompt = "x"
overrides = { review = "x" }
plugins = { "a:b" = true }
features = { results = true, when = 1979-05-27 }
[profile.p.roles.bench]
model = "x"
[server]
url = "http://x"
token = "keychain:t"
"#.parse().unwrap();
        for (k, v) in extra { t.insert(k, v); }
        t
    }

    fn cands() -> Vec<Value> {
        vec![
            Value::String("s".into()), Value::String("".into()), Value::String("monitor".into()),
            Value::String("env:A".into()), Value::String("anthropic".into()),
            Value::Integer(0), Value::Integer(-1), Value::Integer(1 << 40), Value::Integer(i64::MAX),
            Value::Float(1.5), Value::Float(2.0), Value::Boolean(true),
            Value::Datetime("2026-11-18".parse().unwrap()),
            Value::Datetime("2026-11-18T10:00:00Z".parse().unwrap()),
            Value::Array(vec![]), Value::Array(vec![Value::String("a".into())]), Value::Array(vec![Value::Integer(1)]),
            Value::Table(Table::new()),
            Value::Table([("a".to_string(), Value::Integer(1))].into_iter().collect()),
        ]
    }

    // paths to every node
    fn paths(v: &Value, cur: &mut Vec<Seg>, out: &mut Vec<Vec<Seg>>) {
        match v {
            Value::Table(t) => for (k, x) in t { cur.push(Seg::Key(k.clone())); out.push(cur.clone()); paths(x, cur, out); cur.pop(); },
            Value::Array(a) => for (i, x) in a.iter().enumerate() { cur.push(Seg::Index(i)); out.push(cur.clone()); paths(x, cur, out); cur.pop(); },
            _ => {}
        }
    }

    fn get_mut<'a>(v: &'a mut Value, p: &[Seg]) -> &'a mut Value {
        let mut c = v;
        for s in p { c = match s { Seg::Key(k) => c.as_table_mut().unwrap().get_mut(k).unwrap(), Seg::Index(i) => &mut c.as_array_mut().unwrap()[*i] }; }
        c
    }

    fn cmp(v: &Value, what: &str, diffs: &mut Vec<String>, n: &mut usize) {
        *n += 1;
        let a = v.clone().try_into::<Config>();
        let b = from_value::<Config>(v.clone());
        match (&a, &b) {
            (Ok(x), Ok(y)) if x == y => {}
            (Err(_), Err(_)) => {}
            _ => diffs.push(format!("{what}: toml={:?} typed={:?}", a.as_ref().map(|_| "ok").map_err(|e| e.to_string().replace('\n', " ")), b.as_ref().map(|_| "ok").map_err(|e| e.what.clone()))),
        }
    }

    /// When the node at `p` is a unit enum (a string there gives "expected one of ..."), tables in every form toml's
    /// `deserialize_enum` takes or refuses.
    fn enum_tables(root: &Value, p: &[Seg]) -> Vec<Table> {
        let mut v = root.clone();
        *get_mut(&mut v, p) = Value::String("zz_none".into());
        let Err(e) = from_value::<Config>(v) else { return Vec::new() };
        let Some(list) = e.what.strip_prefix("expected one of ") else { return Vec::new() };
        if e.path != p { return Vec::new() }
        let names: Vec<String> = list.split('`').skip(1).step_by(2).map(String::from).collect();
        let one = |k: &str, x: Value| -> Table { [(k.to_string(), x)].into_iter().collect() };
        let mut out = Vec::new();
        for name in &names {
            for x in [
                Value::Table(Table::new()), Value::Array(vec![]), Value::Integer(1), Value::String("s".into()),
                Value::Boolean(true), Value::Array(vec![Value::Integer(1)]), Value::Table(one("a", Value::Integer(1))),
            ] {
                out.push(one(name, x));
            }
            let mut two = one(name, Value::Table(Table::new()));
            two.insert("zz".into(), Value::Table(Table::new()));
            out.push(two);
        }
        if let [a, b, ..] = names.as_slice() {
            let mut two = one(a, Value::Array(vec![]));
            two.insert(b.clone(), Value::Array(vec![]));
            out.push(two);
        }
        out.push(one("zz", Value::Table(Table::new())));
        out
    }

    #[test]
    fn differential() {
        let root = Value::Table(sample());
        let mut ps = Vec::new();
        paths(&root, &mut Vec::new(), &mut ps);
        let (mut diffs, mut n) = (Vec::new(), 0);
        cmp(&root, "base", &mut diffs, &mut n);
        cmp(&Value::Table(Table::new()), "empty", &mut diffs, &mut n);
        for p in &ps {
            for c in cands() {
                let mut v = root.clone();
                *get_mut(&mut v, p) = c.clone();
                cmp(&v, &format!("{p:?}={c}"), &mut diffs, &mut n);
            }
            // delete
            let (last, parent) = p.split_last().unwrap();
            let mut v = root.clone();
            let par = get_mut(&mut v, parent);
            match last { Seg::Key(k) => { par.as_table_mut().unwrap().remove(k); } Seg::Index(i) => { par.as_array_mut().unwrap().remove(*i); } }
            cmp(&v, &format!("{p:?} deleted"), &mut diffs, &mut n);
            // unknown key into tables
            let mut v = root.clone();
            if let Some(t) = get_mut(&mut v, p).as_table_mut() { t.insert("zz_unknown".into(), Value::Integer(1)); cmp(&v, &format!("{p:?} +unknown"), &mut diffs, &mut n); }
            // a unit enum as a table: one key per variant with an empty, a non-empty and a scalar value, and two keys
            for t in enum_tables(&root, p) {
                let mut v = root.clone();
                *get_mut(&mut v, p) = Value::Table(t.clone());
                cmp(&v, &format!("{p:?}={t}"), &mut diffs, &mut n);
            }
        }
        eprintln!("cases {n}, diffs {}", diffs.len());
        for d in diffs.iter().take(40) { eprintln!("{d}"); }
        assert!(diffs.is_empty());
    }

    #[test]
    fn line_of_works() {
        let text = "[[model]]\nid = \"a\"\n[model.facts]\ncontext_window = \"x\"\n";
        assert_eq!(line_of(text, &[Seg::Key("model".into()), Seg::Index(0), Seg::Key("facts".into()), Seg::Key("context_window".into())]), Some(4));
    }
}
