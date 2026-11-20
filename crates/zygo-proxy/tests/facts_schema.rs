//! N14: model facts and strict schemas (§7.6, §20 #9, #10).

use serde_json::{json, Value};
use zygo_config::Dialect::{self, Openai, OpenaiChat};
use zygo_config::{Effort, Facts, Reasoning, StructuredOutput};
use zygo_proxy::pipeline::Adjustment;
use zygo_proxy::translate::facts::{apply, write_effort, IMAGE_OMITTED, SCHEMA_PROMPT};
use zygo_proxy::translate::{translate_request, Target};

fn tgt() -> Target {
    Target { provider: "p-oai".into(), upstream_model: "up-oai".into() }
}

fn codes(a: &[Adjustment]) -> Vec<&str> {
    a.iter().map(|x| x.0.as_str()).collect()
}

fn facts(levels: &[Effort]) -> Facts {
    Facts {
        reasoning: Reasoning::Always,
        effort_levels: levels.to_vec(),
        max_output_tokens: 128000,
        context_window: 200000,
        sampling: true,
        vision: true,
        structured_output: StructuredOutput::Native,
        forced_tool_choice: true,
        ..Default::default()
    }
}

/// The effort `write_effort` puts into an empty body.
fn eff(v: &Value, d: Dialect) -> Value {
    let mut out = json!({});
    write_effort(v, &mut out, d);
    out
}

/// `{"model":"m","max_tokens":8,"messages":[]}` plus the fields of `extra`.
fn body(extra: Value) -> Value {
    let mut b = json!({"model":"m","max_tokens":8,"messages":[]});
    for (k, v) in extra.as_object().unwrap() {
        b[k] = v.clone();
    }
    b
}

// ---- A. Facts ----

use Effort::{High, Low, Max, Medium, Minimal, Xhigh};

#[test]
fn fx_effort_clamp_ladder() {
    let l1 = [Low, Medium, High, Xhigh, Max];
    let l2 = [Low, High];
    let l3 = [Minimal, High];
    let e = |x: &str| json!({"output_config":{"effort":x}});
    let off = json!({"thinking":{"type":"disabled"}});
    let cases: Vec<(Value, &[Effort], &str, Vec<&str>)> = vec![
        (e("medium"), &l1, "medium", vec![]),
        (e("minimal"), &l1, "low", vec!["effort_clamped:minimal>low"]),
        (off.clone(), &l1, "low", vec!["effort_clamped:none>low"]),
        (e("medium"), &l2, "low", vec!["effort_clamped:medium>low"]),
        (e("max"), &l2, "high", vec!["effort_clamped:max>high"]),
        (e("medium"), &l3, "high", vec!["effort_clamped:medium>high"]),
        (e("low"), &l3, "minimal", vec!["effort_clamped:low>minimal"]),
        (json!({"thinking":{"type":"disabled"},"output_config":{"effort":"high"}}), &l1, "low", vec!["effort_clamped:none>low"]),
    ];
    for (client, levels, sent, code) in cases {
        let mut a = body(client.clone());
        let c = apply(&mut a, &facts(levels));
        assert_eq!(eff(&a, Openai), json!({"reasoning":{"effort":sent}}), "{client} {levels:?}");
        assert_eq!(eff(&a, OpenaiChat), json!({"reasoning_effort":sent}), "{client} {levels:?}");
        assert!(a.get("thinking").is_none(), "{client}");
        assert_eq!(codes(&c), code, "{client} {levels:?}");
    }
}

#[test]
fn fx_effort_absent_or_dropped() {
    let l1 = [Low, Medium, High, Xhigh, Max];
    let mut a = body(json!({}));
    assert_eq!(codes(&apply(&mut a, &facts(&l1))), Vec::<&str>::new());
    assert_eq!(eff(&a, Openai), json!({}));
    let mut a = body(json!({"thinking":{"type":"adaptive"}}));
    assert_eq!(codes(&apply(&mut a, &facts(&l1))), Vec::<&str>::new());
    assert_eq!(eff(&a, Openai), json!({}));
    let none = Facts { reasoning: Reasoning::None, effort_levels: vec![], ..facts(&[]) };
    let mut a = body(json!({"output_config":{"effort":"high"}}));
    assert_eq!(codes(&apply(&mut a, &none)), vec!["effort_dropped"]);
    assert_eq!(eff(&a, Openai), json!({}));
    assert_eq!(eff(&a, OpenaiChat), json!({}));
    let mut a = body(json!({"output_config":{"effort":"turbo"}}));
    assert_eq!(codes(&apply(&mut a, &facts(&l1))), vec!["effort_dropped"]);
    assert_eq!(eff(&a, Openai), json!({}));
}

#[test]
fn fx_max_output_and_sampling() {
    let f = facts(&[Low]);
    let mut a = body(json!({"max_tokens":200000}));
    assert_eq!(codes(&apply(&mut a, &f)), vec!["max_output_clamped:200000>128000"]);
    assert_eq!(a["max_tokens"], json!(128000));
    let mut a = body(json!({"max_tokens":1000}));
    assert_eq!(codes(&apply(&mut a, &f)), Vec::<&str>::new());
    assert_eq!(a["max_tokens"], json!(1000));
    let zero = Facts { max_output_tokens: 0, ..f.clone() };
    let mut a = body(json!({"max_tokens":5}));
    assert_eq!(codes(&apply(&mut a, &zero)), Vec::<&str>::new());
    assert_eq!(a["max_tokens"], json!(5));
    let nos = Facts { sampling: false, ..f.clone() };
    let mut a = body(json!({"temperature":0.5,"top_p":0.9}));
    assert_eq!(codes(&apply(&mut a, &nos)), vec!["sampling_dropped"]);
    assert!(a.get("temperature").is_none() && a.get("top_p").is_none());
    let mut a = body(json!({}));
    assert_eq!(codes(&apply(&mut a, &nos)), Vec::<&str>::new());
    let mut a = body(json!({"temperature":0.5,"top_p":0.9}));
    assert_eq!(codes(&apply(&mut a, &f)), Vec::<&str>::new());
    assert_eq!((&a["temperature"], &a["top_p"]), (&json!(0.5), &json!(0.9)));
}

#[test]
fn fx_images_dropped() {
    let img = json!({"type":"image","source":{"type":"base64","media_type":"image/png","data":"AAAA"}});
    let url = json!({"type":"image","source":{"type":"url","url":"https://example.com/a.png"}});
    let msgs = json!({"messages":[
        {"role":"user","content":[img, {"type":"text","text":"a"}]},
        {"role":"user","content":[{"type":"tool_result","tool_use_id":"t","content":[url, {"type":"text","text":"b"}]}]}
    ]});
    let omitted = json!({"type":"text","text":IMAGE_OMITTED});
    let novis = Facts { vision: false, ..facts(&[Low]) };
    let mut a = body(msgs.clone());
    assert_eq!(codes(&apply(&mut a, &novis)), vec!["images_dropped:2"]);
    assert_eq!(a["messages"][0]["content"][0], omitted);
    assert_eq!(a["messages"][0]["content"][1], json!({"type":"text","text":"a"}));
    assert_eq!(a["messages"][1]["content"][0]["content"][0], omitted);
    assert_eq!(a["messages"][1]["content"][0]["content"][1], json!({"type":"text","text":"b"}));
    let mut a = body(msgs.clone());
    assert_eq!(codes(&apply(&mut a, &facts(&[Low]))), Vec::<&str>::new());
    assert_eq!(a, body(msgs));
}

#[test]
fn fx_tool_choice_widened() {
    let f = Facts { forced_tool_choice: false, ..facts(&[Low]) };
    let mut a = body(json!({"tool_choice":{"type":"tool","name":"get","disable_parallel_tool_use":true}}));
    assert_eq!(codes(&apply(&mut a, &f)), vec!["tool_choice_widened"]);
    assert_eq!(a["tool_choice"], json!({"type":"any","disable_parallel_tool_use":true}));
    let (r, _) = translate_request(&a, Openai, &tgt()).unwrap();
    assert_eq!((&r["tool_choice"], &r["parallel_tool_calls"]), (&json!("required"), &json!(false)));
    let (r, _) = translate_request(&a, OpenaiChat, &tgt()).unwrap();
    assert_eq!(r["tool_choice"], json!("required"));
    let mut a = body(json!({"tool_choice":{"type":"auto"}}));
    assert_eq!(codes(&apply(&mut a, &f)), Vec::<&str>::new());
    assert_eq!(a["tool_choice"], json!({"type":"auto"}));
}

#[test]
fn fx_structured_output_prompted() {
    let s = json!({"type":"object","properties":{"x":{"type":"integer"}}});
    let p = SCHEMA_PROMPT.to_string() + &s.to_string();
    let oc = json!({"format":{"type":"json_schema","schema":s}});
    let f = Facts { structured_output: StructuredOutput::None, ..facts(&[Low]) };
    let mut a = body(json!({"system":"S1","output_config":oc}));
    assert_eq!(codes(&apply(&mut a, &f)), vec!["structured_output_prompted"]);
    assert_eq!(a["system"], json!(format!("S1\n\n{p}")));
    assert!(a.get("output_config").is_none());
    let (r, _) = translate_request(&a, Openai, &tgt()).unwrap();
    assert!(r.get("text").is_none());
    assert_eq!(r["instructions"], json!(format!("S1\n\n{p}")));
    let mut a = body(json!({"output_config":oc}));
    apply(&mut a, &f);
    assert_eq!(a["system"], json!(p));
    let mut a = body(json!({"system":[{"type":"text","text":"S1"}],"output_config":oc}));
    apply(&mut a, &f);
    assert_eq!(a["system"], json!([{"type":"text","text":"S1"},{"type":"text","text":p}]));
    let mut a = body(json!({"system":"S1","output_config":oc}));
    assert_eq!(codes(&apply(&mut a, &facts(&[Low]))), Vec::<&str>::new());
    assert_eq!(a, body(json!({"system":"S1","output_config":oc})));
}

#[test]
fn fx_codes_in_table_order() {
    let f = Facts { sampling: false, vision: false, forced_tool_choice: false, structured_output: StructuredOutput::None, ..facts(&[Low, High]) };
    let mut a = body(json!({
        "max_tokens":200000,
        "temperature":0.5,
        "output_config":{"effort":"medium","format":{"type":"json_schema","schema":{"type":"object"}}},
        "tool_choice":{"type":"tool","name":"get"},
        "messages":[{"role":"user","content":[{"type":"image","source":{"type":"url","url":"https://example.com/a.png"}}]}]
    }));
    assert_eq!(
        codes(&apply(&mut a, &f)),
        vec!["effort_clamped:medium>low", "max_output_clamped:200000>128000", "sampling_dropped", "images_dropped:1", "tool_choice_widened", "structured_output_prompted"]
    );
}

// ---- B. Schema rewrite ----

use std::collections::BTreeMap;
use zygo_config::builtin::schema_rules;
use zygo_config::{Family, RuleSeverity, SchemaRule};
use zygo_proxy::translate::schema::{rewrite, NullMap, Seg};

fn rule(f: Family, code: &str, sev: RuleSeverity, limit: Option<u64>) -> SchemaRule {
    SchemaRule { family: f, code: code.into(), severity: sev, limit }
}

fn one_tool(schema: Value) -> Value {
    json!({"model":"m","messages":[],"tools":[{"name":"get","input_schema":schema}]})
}

fn k(s: &str) -> Seg {
    Seg::Key(s.into())
}

fn s_props(names: &[&str]) -> Value {
    let mut p = serde_json::Map::new();
    for n in names {
        p.insert(n.to_string(), json!({"type":"string"}));
    }
    json!({"type":"object","properties":p})
}

#[test]
fn sch_each_builtin_refuse_rule_rewrites() {
    let wide: Vec<String> = (0..25).map(|i| format!("p{i}")).collect();
    let wide_in = s_props(&wide.iter().map(String::as_str).collect::<Vec<_>>());
    let mut wide_out = wide_in.clone();
    wide_out["properties"]["p24"] = json!({"type":["string","null"]});
    wide_out["required"] = json!(["p24"]);
    let map_in = json!({"type":"object","additionalProperties":{"type":"string"}});
    let map_out = json!({"type":"object","additionalProperties":false,"description":"{\"additionalProperties\":{\"type\":\"string\"}}"});
    let all_in = json!({"allOf":[{"type":"object","properties":{"a":{"type":"string"}},"required":["a"]},{"properties":{"b":{"type":"integer"}},"required":["b"]}]});
    let all_out = json!({"type":"object","properties":{"a":{"type":"string"},"b":{"type":"integer"}},"required":["a","b"]});
    let closed_in = json!({"type":"object","properties":{"a":{"type":"string"}}});
    let closed_out = json!({"type":"object","properties":{"a":{"type":"string"}},"additionalProperties":false});
    type Case = (&'static str, &'static str, Value, Value, Option<Vec<Vec<Seg>>>);
    let cases: Vec<Case> = vec![
        ("anthropic", "map_object", map_in.clone(), map_out.clone(), None),
        ("openai", "map_object", map_in, map_out, None),
        ("anthropic", "one_of", json!({"oneOf":[{"type":"string"},{"type":"integer"}]}), json!({"anyOf":[{"type":"string"},{"type":"integer"}]}), None),
        ("anthropic", "all_of", all_in.clone(), all_out.clone(), None),
        ("openai", "all_of", all_in, all_out, None),
        ("openai", "conditional", json!({"type":"object","if":{"required":["a"]},"then":{"required":["b"]}}),
            json!({"type":"object","description":"{\"if\":{\"required\":[\"a\"]},\"then\":{\"required\":[\"b\"]}}"}), None),
        ("anthropic", "closed_object", closed_in.clone(), closed_out.clone(), None),
        ("openai", "closed_object", closed_in, closed_out, None),
        ("anthropic", "numeric_bounds", json!({"type":"integer","minimum":1,"maximum":5}), json!({"type":"integer","description":"{\"minimum\":1,\"maximum\":5}"}), None),
        ("anthropic", "string_length", json!({"type":"string","minLength":1,"maxLength":9}), json!({"type":"string","description":"{\"minLength\":1,\"maxLength\":9}"}), None),
        ("anthropic", "array_bounds", json!({"type":"array","items":{"type":"string"},"minItems":2,"maxItems":4}),
            json!({"type":"array","items":{"type":"string"},"description":"{\"minItems\":2,\"maxItems\":4}"}), None),
        ("openai", "optional_property", json!({"type":"object","properties":{"a":{"type":"string"}}}),
            json!({"type":"object","properties":{"a":{"type":["string","null"]}},"required":["a"]}), Some(vec![vec![k("a")]])),
        ("anthropic", "optional_properties_max", wide_in, wide_out, Some(vec![vec![k("p24")]])),
    ];
    let rules = schema_rules();
    let refuse: Vec<&SchemaRule> = rules.iter().filter(|r| r.severity == RuleSeverity::Refuse).collect();
    assert_eq!(refuse.len(), 13);
    assert_eq!(cases.len(), 13);
    for r in refuse {
        let (_, _, inp, out, paths) = cases.iter().find(|c| c.0 == r.family.name() && c.1 == r.code).unwrap_or_else(|| panic!("no case {}:{}", r.family.name(), r.code));
        let mut b = one_tool(inp.clone());
        let (c, nulls) = rewrite(&mut b, std::slice::from_ref(r));
        assert_eq!(codes(&c), vec![format!("schema:{}:1", r.code)], "{}:{}", r.family.name(), r.code);
        assert_eq!(&b["tools"][0]["input_schema"], out, "{}:{}", r.family.name(), r.code);
        match paths {
            Some(p) => assert_eq!(nulls.tools.get("get"), Some(p), "{}", r.code),
            None => assert!(nulls.is_empty(), "{}", r.code),
        }
    }
    let warn = rules.iter().find(|r| r.code == "pattern").unwrap();
    assert_eq!(warn.severity, RuleSeverity::Warn);
    let mut b = one_tool(json!({"type":"string","pattern":"^a"}));
    let (c, _) = rewrite(&mut b, std::slice::from_ref(warn));
    assert!(c.is_empty());
    assert_eq!(b, one_tool(json!({"type":"string","pattern":"^a"})));
}

#[test]
fn sch_moves_and_edges() {
    let r = |code: &str, limit| rule(Family::Anthropic, code, RuleSeverity::Refuse, limit);
    let (a, bb) = (json!({"type":"string"}), json!({"type":"integer"}));
    let mut b = one_tool(json!({"oneOf":[a],"anyOf":[bb]}));
    assert_eq!(codes(&rewrite(&mut b, &[r("one_of", None)]).0), vec!["schema:one_of:1"]);
    assert_eq!(b["tools"][0]["input_schema"], json!({"anyOf":[bb],"description":json!({"oneOf":[a]}).to_string()}));

    let arr = json!({"type":"array","items":{"type":"string"},"minItems":1});
    let mut b = one_tool(arr.clone());
    assert!(rewrite(&mut b, &[r("array_bounds", Some(1))]).0.is_empty());
    assert_eq!(b["tools"][0]["input_schema"], arr);

    let mut b = one_tool(json!({"type":"string","pattern":"^a"}));
    assert_eq!(codes(&rewrite(&mut b, &[r("pattern", None)]).0), vec!["schema:pattern:1"]);
    assert_eq!(b["tools"][0]["input_schema"], json!({"type":"string","description":"{\"pattern\":\"^a\"}"}));

    let mut b = one_tool(json!({"type":"integer","description":"d","minimum":1}));
    rewrite(&mut b, &[r("numeric_bounds", None)]);
    assert_eq!(b["tools"][0]["input_schema"]["description"], json!("d\n\n{\"minimum\":1}"));

    let mut b = one_tool(json!({"type":["integer","string"],"minimum":1,"maxLength":3}));
    let (c, _) = rewrite(&mut b, &[r("string_length", None), r("numeric_bounds", None)]);
    assert_eq!(codes(&c), vec!["schema:numeric_bounds:1", "schema:string_length:1"]);
    assert_eq!(b["tools"][0]["input_schema"]["description"], json!("{\"minimum\":1}\n\n{\"maxLength\":3}"));
}

#[test]
fn sch_optional_forms_and_paths() {
    let props = json!({
        "a":{"type":"string"},
        "b":{"type":["integer","null"]},
        "c":{"type":"string","enum":["x","y"]},
        "c2":{"enum":["x","y"]},
        "d":{"$ref":"#/$defs/D"},
        "e":{"anyOf":[{"type":"string"},{"type":"integer"}]},
        "f":{"type":"null"},
        "g":{"anyOf":[{"type":"string"},{"type":["integer","null"]}]},
        "h":{"oneOf":[{"type":"null"},{"type":"string"}]},
        "o":{"type":"object","properties":{"i":{"type":"string"}}},
        "l":{"type":"array","items":{"type":"object","properties":{"k":{"type":"string"}}}}
    });
    let defs = json!({"D":{"type":"object","properties":{"z":{"type":"string"}}}});
    let mut b = one_tool(json!({"type":"object","properties":props,"$defs":defs}));
    b["output_config"] = json!({"format":{"type":"json_schema","schema":{"type":"object","properties":{"x":{"type":"string"}}}}});
    let (c, nulls) = rewrite(&mut b, &[rule(Family::Openai, "optional_property", RuleSeverity::Refuse, None)]);
    let s = &b["tools"][0]["input_schema"];
    let p = &s["properties"];
    let order = ["a", "b", "c", "c2", "d", "e", "f", "g", "h", "o", "l"];
    assert_eq!(p.as_object().unwrap().keys().collect::<Vec<_>>(), order);
    assert_eq!(s["required"], json!(order));
    assert_eq!(p["a"], json!({"type":["string","null"]}));
    assert_eq!(p["b"], json!({"type":["integer","null"]}));
    assert_eq!(p["c"], json!({"type":["string","null"],"enum":["x","y",null]}));
    assert_eq!(p["c2"], json!({"enum":["x","y",null]}));
    assert_eq!(p["d"], json!({"anyOf":[{"$ref":"#/$defs/D"},{"type":"null"}]}));
    assert_eq!(p["e"], json!({"anyOf":[{"type":"string"},{"type":"integer"},{"type":"null"}]}));
    for x in ["f", "g", "h"] {
        assert_eq!(p[x], props[x], "{x}");
    }
    assert_eq!(p["o"], json!({"type":["object","null"],"properties":{"i":{"type":["string","null"]}},"required":["i"]}));
    assert_eq!(p["l"], json!({"type":["array","null"],"items":{"type":"object","properties":{"k":{"type":["string","null"]}},"required":["k"]}}));
    assert_eq!(s["$defs"], defs);
    assert_eq!(codes(&c), vec!["schema:optional_property:14"]);
    let want: Vec<Vec<Seg>> = vec![vec![k("a")], vec![k("c")], vec![k("c2")], vec![k("d")], vec![k("e")], vec![k("o")], vec![k("l")],
        vec![k("o"), k("i")], vec![k("l"), Seg::Items, k("k")]];
    assert_eq!(nulls.tools.get("get"), Some(&want));
    assert_eq!(nulls.output, vec![vec![k("x")]]);
}

#[test]
fn sch_all_of_nested_merged() {
    // Round 2 (F5): all_of merges until no `allOf` remains at the node; a branch's own allOf follows that branch.
    let mut b = one_tool(json!({"type":"object","allOf":[
        {"allOf":[{"properties":{"a":{"type":"string"},"b":{"maxLength":2}},"required":["a"]}],"properties":{"b":{"type":"string"}}},
        {"allOf":[{"properties":{"c":{"type":"string"}},"required":["c"]}],"description":"d"}
    ]}));
    let (c, _) = rewrite(&mut b, &[rule(Family::Openai, "all_of", RuleSeverity::Refuse, None)]);
    let s = &b["tools"][0]["input_schema"];
    assert!(!s.to_string().contains("allOf"), "{s}");
    assert_eq!(s["properties"], json!({"b":{"type":"string","maxLength":2},"a":{"type":"string"},"c":{"type":"string"}}));
    assert_eq!(s["required"], json!(["a", "c"]));
    assert_eq!(s["description"], json!("d"));
    assert_eq!(codes(&c), vec!["schema:all_of:1"]);
}

#[test]
fn sch_optional_accepts_null_forms() {
    // Round 2 (F2): an enum with null, `const: null`, `true` and `{}` accept null: required, no injected null, no path.
    let props = json!({"a":{"enum":["x",null]},"b":{"const":null},"c":true,"d":{},"e":{"type":"string","enum":["x",null]}});
    let mut b = one_tool(json!({"type":"object","properties":props}));
    let (c, nulls) = rewrite(&mut b, &[rule(Family::Openai, "optional_property", RuleSeverity::Refuse, None)]);
    let s = &b["tools"][0]["input_schema"];
    for x in ["a", "b", "c", "d"] {
        assert_eq!(s["properties"][x], props[x], "{x}");
    }
    // A `type` that excludes null is still widened: the null is injected.
    assert_eq!(s["properties"]["e"], json!({"type":["string","null"],"enum":["x",null]}));
    assert_eq!(s["required"], json!(["a", "b", "c", "d", "e"]));
    assert_eq!(codes(&c), vec!["schema:optional_property:5"]);
    assert_eq!(nulls.tools.get("get"), Some(&vec![vec![k("e")]]));
    let mut m = json!({"content":[{"type":"tool_use","id":"t","name":"get","input":{"a":null,"b":null,"c":null,"d":null,"e":null}}]});
    assert!(nulls.restore_message(&mut m));
    assert_eq!(m["content"][0]["input"], json!({"a":null,"b":null,"c":null,"d":null}));
}

#[test]
fn sch_optional_false_property_untouched() {
    // Round 2 (F3): a `false` property ("must be absent") is not made required and gets no path.
    let mut b = one_tool(json!({"type":"object","properties":{"a":false,"b":{"type":"string"},"c":{"type":"string"}},"required":["b"]}));
    let (c, nulls) = rewrite(&mut b, &[rule(Family::Openai, "optional_property", RuleSeverity::Refuse, None)]);
    let s = &b["tools"][0]["input_schema"];
    assert_eq!(s["properties"]["a"], json!(false));
    assert_eq!(s["required"], json!(["b", "c"]));
    assert_eq!(codes(&c), vec!["schema:optional_property:1"]);
    assert_eq!(nulls.tools.get("get"), Some(&vec![vec![k("c")]]));
}

#[test]
fn sch_optional_const_wrapped() {
    // Round 2 (F4): a non-null `const` (with or without `type`) is wrapped in anyOf with null; path recorded.
    let props = json!({"a":{"type":"string","const":"x"},"b":{"const":5}});
    let mut b = one_tool(json!({"type":"object","properties":props}));
    let (c, nulls) = rewrite(&mut b, &[rule(Family::Openai, "optional_property", RuleSeverity::Refuse, None)]);
    let p = &b["tools"][0]["input_schema"]["properties"];
    for x in ["a", "b"] {
        assert_eq!(p[x], json!({"anyOf":[props[x], {"type":"null"}]}), "{x}");
    }
    assert_eq!(codes(&c), vec!["schema:optional_property:2"]);
    assert_eq!(nulls.tools.get("get"), Some(&vec![vec![k("a")], vec![k("b")]]));
}

#[test]
fn sch_optional_max_excess_in_order() {
    let mut b = json!({"model":"m","messages":[],"tools":[{"name":"t1","input_schema":s_props(&["a","b"])},{"name":"t2","input_schema":s_props(&["c","d"])}],
        "output_config":{"format":{"type":"json_schema","schema":s_props(&["x"])}}});
    let (c, nulls) = rewrite(&mut b, &[rule(Family::Anthropic, "optional_properties_max", RuleSeverity::Refuse, Some(2))]);
    assert_eq!(b["tools"][0]["input_schema"], s_props(&["a", "b"]));
    let n = json!({"type":["string","null"]});
    assert_eq!(b["tools"][1]["input_schema"], json!({"type":"object","properties":{"c":n,"d":n},"required":["c","d"]}));
    assert_eq!(b["output_config"]["format"]["schema"], json!({"type":"object","properties":{"x":n},"required":["x"]}));
    assert_eq!(codes(&c), vec!["schema:optional_properties_max:3"]);
    let want = NullMap { tools: BTreeMap::from([("t2".to_string(), vec![vec![k("c")], vec![k("d")]])]), output: vec![vec![k("x")]] };
    assert_eq!(nulls, want);
}

#[test]
fn sch_order_locations_no_change() {
    let openai: Vec<SchemaRule> = schema_rules().into_iter().filter(|r| r.family == Family::Openai).collect();
    let get = json!({"type":"object","properties":{"m":{"type":"object","additionalProperties":true},
        "q":{"allOf":[{"type":"object","properties":{"r":{"type":"string"}},"required":["r"]}]}}});
    let server = json!({"type":"web_search_20250305","name":"web_search"});
    let mut b = json!({"model":"m","messages":[],"tools":[{"name":"get","input_schema":get},server],
        "output_config":{"format":{"type":"json_schema","schema":{"type":"object","properties":{"x":{"type":"string"}},"required":["x"]}}}});
    let (c, _) = rewrite(&mut b, &openai);
    assert_eq!(codes(&c), vec!["schema:map_object:1", "schema:all_of:1", "schema:closed_object:3", "schema:optional_property:2"]);
    assert_eq!(b["tools"][1], server);
    assert_eq!(b["output_config"]["format"]["schema"]["additionalProperties"], json!(false));

    let ok = json!({"model":"m","messages":[],"tools":[{"name":"get","input_schema":{"type":"object","properties":{
        "a":{"type":"string"},"o":{"type":"object","properties":{"i":{"type":"integer"}},"required":["i"],"additionalProperties":false}},
        "required":["a","o"],"additionalProperties":false}}],
        "output_config":{"format":{"type":"json_schema","schema":{"type":"object","properties":{"x":{"type":"string"}},"required":["x"],"additionalProperties":false}}}});
    let mut b = ok.clone();
    let (c, nulls) = rewrite(&mut b, &openai);
    assert!(c.is_empty());
    assert!(nulls.is_empty());
    assert_eq!(b, ok);
}

// ---- C. Restore ----

use zygo_proxy::tee::HOLD_MAX;
use zygo_proxy::translate::schema::RestoreFilter;
use zygo_proxy::translate::SseEvent;

fn ev(name: &str, data: Value) -> SseEvent {
    SseEvent { event: name.into(), data }
}

fn msg_start(id: &str, model: &str) -> SseEvent {
    ev("message_start", json!({"type":"message_start","message":{"id":id,"type":"message","role":"assistant","model":model,"content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":0,"output_tokens":0}}}))
}

fn block_start(i: u32, b: Value) -> SseEvent {
    ev("content_block_start", json!({"type":"content_block_start","index":i,"content_block":b}))
}

fn text_start(i: u32) -> SseEvent {
    block_start(i, json!({"type":"text","text":""}))
}

fn delta(i: u32, d: Value) -> SseEvent {
    ev("content_block_delta", json!({"type":"content_block_delta","index":i,"delta":d}))
}

fn text_d(i: u32, x: &str) -> SseEvent {
    delta(i, json!({"type":"text_delta","text":x}))
}

fn json_d(i: u32, x: &str) -> SseEvent {
    delta(i, json!({"type":"input_json_delta","partial_json":x}))
}

fn block_stop(i: u32) -> SseEvent {
    ev("content_block_stop", json!({"type":"content_block_stop","index":i}))
}

fn msg_stop() -> SseEvent {
    ev("message_stop", json!({"type":"message_stop"}))
}

fn tu(i: u32, name: &str) -> SseEvent {
    block_start(i, json!({"type":"tool_use","id":format!("t{i}"),"name":name,"input":{}}))
}

fn ping() -> SseEvent {
    ev("ping", json!({"type":"ping"}))
}

fn run(mut f: RestoreFilter, evs: &[SseEvent]) -> Vec<SseEvent> {
    let mut out: Vec<SseEvent> = evs.iter().flat_map(|e| f.push(e.clone())).collect();
    out.extend(f.finish());
    out
}

fn raw(mut f: RestoreFilter, bytes: &[u8], n: usize) -> Vec<u8> {
    let mut out: Vec<u8> = bytes.chunks(n).flat_map(|c| f.push_raw(c)).collect();
    out.extend(f.finish_raw());
    out
}

fn bytes_of(evs: &[SseEvent]) -> Vec<u8> {
    evs.iter().flat_map(SseEvent::to_bytes).collect()
}

#[allow(non_snake_case)]
fn NA() -> NullMap {
    NullMap { tools: BTreeMap::from([("get".to_string(), vec![vec![k("a")]])]), output: vec![] }
}

#[test]
fn rst_message_removes_injected_nulls() {
    let n = NullMap {
        tools: BTreeMap::from([("get".to_string(), vec![vec![k("a")], vec![k("o"), k("i")], vec![k("l"), Seg::Items, k("k")]])]),
        output: vec![vec![k("x")]],
    };
    let mut m = json!({"content":[
        {"type":"tool_use","id":"t1","name":"get","input":{"a":null,"b":null,"o":{"i":null},"l":[{"k":null},{"k":1}]}},
        {"type":"tool_use","id":"t2","name":"other","input":{"a":null}},
        {"type":"text","text":"{\"x\":null,\"y\":1}"},
        {"type":"text","text":"plain"}
    ]});
    assert!(n.restore_message(&mut m));
    assert_eq!(m["content"][0]["input"], json!({"b":null,"o":{},"l":[{},{"k":1}]}));
    assert_eq!(m["content"][1]["input"], json!({"a":null}));
    assert_eq!(m["content"][2]["text"], json!("{\"y\":1}"));
    assert_eq!(m["content"][3]["text"], json!("plain"));
    let mut m2 = json!({"content":[{"type":"tool_use","id":"t1","name":"get","input":{"a":1}},{"type":"text","text":"{\"y\":1}"}]});
    let before = m2.clone();
    assert!(!n.restore_message(&mut m2));
    assert_eq!(m2, before);
}

#[test]
fn rst_events_tool_block() {
    let e = vec![
        msg_start("m1", "x"),
        tu(0, "get"),
        json_d(0, "{\"a\":nu"),
        json_d(0, "ll,\"b\":2}"),
        block_stop(0),
        tu(1, "get"),
        json_d(1, "{\"a\""),
        json_d(1, ":1}"),
        block_stop(1),
        tu(2, "other"),
        json_d(2, "{\"a\":null}"),
        block_stop(2),
        text_start(3),
        text_d(3, "{\"a\":null}"),
        block_stop(3),
        msg_stop(),
    ];
    let mut f = RestoreFilter::new(NA());
    assert_eq!(f.push(e[0].clone()), vec![e[0].clone()]);
    assert_eq!(f.push(e[1].clone()), vec![e[1].clone()]);
    assert_eq!(f.push(e[2].clone()), vec![]);
    let mut want = e.clone();
    want.splice(2..4, [json_d(0, "{\"b\":2}")]);
    assert_eq!(run(RestoreFilter::new(NA()), &e), want);

    let out = NullMap { tools: BTreeMap::new(), output: vec![vec![k("x")]] };
    let t = [text_start(0), text_d(0, "{\"x\":nu"), text_d(0, "ll,\"y\":1}"), block_stop(0)];
    assert_eq!(run(RestoreFilter::new(out), &t), vec![text_start(0), text_d(0, "{\"y\":1}"), block_stop(0)]);
}

#[test]
fn rst_events_bound_and_finish() {
    // (a) over the bound: flushed unchanged; the rest of the block passes.
    let mut f = RestoreFilter::new(NA());
    assert_eq!(f.push(tu(0, "get")), vec![tu(0, "get")]);
    assert_eq!(f.push(json_d(0, "{\"a\":")), vec![]);
    let big = json_d(0, &"x".repeat(HOLD_MAX));
    assert_eq!(f.push(big.clone()), vec![json_d(0, "{\"a\":"), big]);
    assert_eq!(f.push(json_d(0, "}")), vec![json_d(0, "}")]);
    assert_eq!(f.push(block_stop(0)), vec![block_stop(0)]);
    // (b) the end of the stream: held deltas unchanged.
    let mut f = RestoreFilter::new(NA());
    f.push(tu(0, "get"));
    assert_eq!(f.push(json_d(0, "{\"a\":null")), vec![]);
    assert_eq!(f.finish(), vec![json_d(0, "{\"a\":null")]);
    // (c) another event: the held delta, then the event.
    let mut f = RestoreFilter::new(NA());
    f.push(tu(0, "get"));
    f.push(json_d(0, "{\"a\":null"));
    let err = ev("error", json!({"type":"error","error":{"type":"api_error","message":"m"}}));
    assert_eq!(f.push(err.clone()), vec![json_d(0, "{\"a\":null"), err]);
}

#[test]
fn rst_ping_inside_held_block() {
    // Round 2 (F1): a ping inside a held block is held in place; the rebuilt delta goes where the first delta was.
    let e = [tu(0, "get"), json_d(0, "{\"a\":nu"), ping(), json_d(0, "ll,\"b\":2}"), block_stop(0)];
    let mut f = RestoreFilter::new(NA());
    f.push(e[0].clone());
    f.push(e[1].clone());
    assert_eq!(f.push(ping()), vec![]);
    let want = [tu(0, "get"), json_d(0, "{\"b\":2}"), ping(), block_stop(0)];
    assert_eq!(run(RestoreFilter::new(NA()), &e), want);
    assert_eq!(raw(RestoreFilter::new(NA()), &bytes_of(&e), 5), bytes_of(&want));
    // No null removed: the events and the bytes go out as received.
    let e2 = [tu(0, "get"), json_d(0, "{\"a\":"), ping(), json_d(0, "1}"), block_stop(0)];
    assert_eq!(run(RestoreFilter::new(NA()), &e2), e2);
    let b2 = bytes_of(&e2);
    for n in 1..=b2.len() {
        assert!(raw(RestoreFilter::new(NA()), &b2, n) == b2, "chunk {n}");
    }
}

#[test]
fn rst_raw_comment_in_held_block_kept_in_place() {
    let (cm, id) = (b": keep-alive\n\n".to_vec(), b"id: 7\n\n".to_vec());
    // No null removed: byte identical.
    let input = [bytes_of(&[tu(0, "get"), json_d(0, "{\"a\":")]), cm.clone(), bytes_of(&[json_d(0, "1}"), block_stop(0)])].concat();
    for n in 1..=input.len() {
        assert!(raw(RestoreFilter::new(NA()), &input, n) == input, "chunk {n}");
    }
    // A null removed: the non-delta events keep their order; the rebuilt delta goes where the first delta was.
    let input = [
        bytes_of(&[tu(0, "get"), ping(), json_d(0, "{\"a\":nu")]),
        cm.clone(),
        id.clone(),
        bytes_of(&[json_d(0, "ll,\"b\":2}"), block_stop(0)]),
    ]
    .concat();
    let want = [bytes_of(&[tu(0, "get"), ping(), json_d(0, "{\"b\":2}")]), cm, id, bytes_of(&[block_stop(0)])].concat();
    for n in 1..=input.len() {
        assert_eq!(String::from_utf8(raw(RestoreFilter::new(NA()), &input, n)).unwrap(), String::from_utf8(want.clone()).unwrap(), "chunk {n}");
    }
}

#[test]
fn rst_raw_sweep_identity_and_restore() {
    let nm = NullMap { tools: NA().tools, output: vec![vec![k("x")]] };
    let base = [
        bytes_of(&[tu(0, "get"), json_d(0, "{\"a\":"), json_d(0, "1}"), block_stop(0)]),
        b"data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n".to_vec(),
        b"data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"{\\\"x\\\":1}\"}}\n\n".to_vec(),
        b"data: {\"type\":\"content_block_stop\",\"index\":1}\n\n".to_vec(),
    ]
    .concat();
    let crlf = String::from_utf8(base.clone()).unwrap().replace('\n', "\r\n").into_bytes();
    for input in [&base, &crlf] {
        for n in 1..=input.len() {
            assert!(raw(RestoreFilter::new(nm.clone()), input, n) == *input, "chunk {n}");
        }
    }
    let with = bytes_of(&[tu(0, "get"), json_d(0, "{\"a\":nu"), json_d(0, "ll,\"b\":1}"), block_stop(0)]);
    let want = bytes_of(&[tu(0, "get"), json_d(0, "{\"b\":1}"), block_stop(0)]);
    for n in 1..=with.len() {
        assert!(raw(RestoreFilter::new(nm.clone()), &with, n) == want, "chunk {n}");
    }
}

fn r_parts() -> Vec<Vec<u8>> {
    vec![
        b": keep-alive\n\n".to_vec(),
        bytes_of(&[msg_start("m1", "x"), tu(0, "get"), json_d(0, "{\"a\":nu")]),
        b"id: 7\nretry: 100\n\n".to_vec(),
        bytes_of(&[json_d(0, "ll,\"b\":2}"), block_stop(0), tu(1, "get"), json_d(1, "{\"a\":1}"), block_stop(1)]),
        b"data: not-json\n\n".to_vec(),
        bytes_of(&[msg_stop()]),
    ]
}

#[test]
fn rst_raw_rebuilds_only_changed_block() {
    let r = r_parts().concat();
    let want = [
        b": keep-alive\n\n".to_vec(),
        bytes_of(&[msg_start("m1", "x"), tu(0, "get"), json_d(0, "{\"b\":2}")]),
        b"id: 7\nretry: 100\n\n".to_vec(),
        bytes_of(&[block_stop(0), tu(1, "get"), json_d(1, "{\"a\":1}"), block_stop(1)]),
        b"data: not-json\n\n".to_vec(),
        bytes_of(&[msg_stop()]),
    ]
    .concat();
    assert_eq!(String::from_utf8(raw(RestoreFilter::new(NA()), &r, 5)).unwrap(), String::from_utf8(want).unwrap());
}

#[test]
fn rst_raw_no_null_byte_identical() {
    let r2 = [
        b": keep-alive\n\n".to_vec(),
        bytes_of(&[msg_start("m1", "x")]),
        b"id: 7\nretry: 100\n\n".to_vec(),
        bytes_of(&[tu(1, "get"), json_d(1, "{\"a\":1}"), block_stop(1)]),
        b"data: not-json\n\n".to_vec(),
        bytes_of(&[msg_stop()]),
    ]
    .concat();
    let r2 = String::from_utf8(r2).unwrap().replace('\n', "\r\n").into_bytes();
    assert_eq!(raw(RestoreFilter::new(NA()), &r2, 3), r2);
    assert_eq!(raw(RestoreFilter::new(NA()), &r2, r2.len()), r2);
}

#[test]
fn rst_raw_over_bound_flushed_unchanged() {
    // (a) a held block over the bound.
    let head = bytes_of(&[tu(0, "get")]);
    let big = format!("{{\"a\":null,\"p\":\"{}\"}}", "x".repeat(HOLD_MAX));
    let input = [head.clone(), bytes_of(&[json_d(0, &big), block_stop(0)])].concat();
    let mut f = RestoreFilter::new(NA());
    let mut out = Vec::new();
    let mut fed = 0;
    let mut crossed = false;
    for c in input.chunks(64 << 10) {
        let o = f.push_raw(c);
        let before = fed;
        fed += c.len();
        if before <= head.len() + HOLD_MAX && fed > head.len() + HOLD_MAX {
            assert!(!o.is_empty(), "the push that crosses the bound returns output");
            crossed = true;
        }
        out.extend(o);
    }
    out.extend(f.finish_raw());
    assert!(crossed);
    assert!(out == input, "over the bound: the bytes pass unchanged");

    // (b) not holding: a partial line over the bound.
    let line = [b"data: ".to_vec(), vec![b'x'; HOLD_MAX + 4]].concat();
    assert_eq!(line.len(), HOLD_MAX + 10);
    let mut f = RestoreFilter::new(NA());
    assert!(f.push_raw(&line[..HOLD_MAX]).is_empty());
    assert!(f.push_raw(&line[HOLD_MAX..]) == line, "the crossing push returns everything so far");
    let rest = [b"\n".to_vec(), bytes_of(&[msg_stop()])].concat();
    let mut o = f.push_raw(&rest);
    o.extend(f.finish_raw());
    assert_eq!(o, rest);
}

// ---- D. The request path (HTTP) ----

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Body;
use axum::extract::Request;
use axum::response::Response;
use bytes::Bytes;
use http::HeaderMap;
use tempfile::TempDir;
use zygo_config::keys::Secret;
use zygo_config::{load, KeyRef, LoadContext, Merged};
use zygo_core::ids::ulid;
use zygo_core::paths::Paths;
use zygo_core::Agent;
use zygo_proxy::provider::KeyResolver;
use zygo_proxy::redact_stage::RedactLimits;
use zygo_proxy::server::StartOpts;
use zygo_proxy::session::{ManualClock, SessionGrant, SessionRequest};
use zygo_proxy::{Proxy, ProxyHandle};
use zygo_redact::DeadlinePolicy;
use zygo_usage::{RequestRow, Store};

/// `"event: <t>\ndata: <json>\n\n"` per event, with `"type": <t>` in the data.
fn sse(events: &[(&str, Value)]) -> String {
    events
        .iter()
        .map(|(t, d)| {
            let mut d = d.clone();
            d["type"] = json!(t);
            format!("event: {t}\ndata: {d}\n\n")
        })
        .collect()
}

/// A chat chunk with id `chatcmpl-1` and model `up-fc`.
fn chunk(rest: Value) -> String {
    let mut c = json!({"id":"chatcmpl-1","object":"chat.completion.chunk","model":"up-fc"});
    for (k, v) in rest.as_object().unwrap() {
        c[k] = v.clone();
    }
    format!("data: {c}\n\n")
}

fn chat_delta(d: Value, finish: Value) -> String {
    chunk(json!({"choices":[{"index":0,"delta":d,"finish_reason":finish}]}))
}

const DONE: &str = "data: [DONE]\n\n";
const M: &str = "anthropic/v1/messages";
const ANTH_SSE: &str = concat!(
    "event: message_start\n",
    r#"data: {"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","content":[],"usage":{"input_tokens":120,"cache_read_input_tokens":30,"cache_creation_input_tokens":7,"output_tokens":1}}}"#,
    "\n\n",
    "event: message_delta\n",
    r#"data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":42}}"#,
    "\n\n",
    "event: message_stop\n",
    r#"data: {"type":"message_stop"}"#,
    "\n\n",
);
/// §20 #25 value; not credential-shaped (two digits must follow `oat`).
const OAT_BEARER: &str = "Bearer sk-ant-oat-x";

#[derive(Clone)]
struct Reply {
    status: u16,
    ctype: &'static str,
    body: String,
}

fn sse_reply(s: &str) -> Reply {
    Reply { status: 200, ctype: "text/event-stream", body: s.into() }
}

fn json_reply(status: u16, v: Value) -> Reply {
    Reply { status, ctype: "application/json", body: v.to_string() }
}

/// The content type that makes the fake upstream fail after the body's first third.
const CUT: &str = "application/json; test=cut";

/// A 200 JSON reply whose body read fails mid-body.
fn cut_reply(v: Value) -> Reply {
    Reply { status: 200, ctype: CUT, body: v.to_string() }
}

#[derive(Clone)]
struct Got {
    path: String,
    body: Vec<u8>,
}

/// Path only.
impl fmt::Debug for Got {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Got").field("path", &self.path).finish()
    }
}

#[derive(Default)]
struct UpState {
    got: Mutex<Vec<Got>>,
    script: Mutex<HashMap<String, VecDeque<Reply>>>,
}

/// A fake upstream on 127.0.0.1:0 with its own runtime thread; records each request.
struct Upstream {
    addr: SocketAddr,
    st: Arc<UpState>,
    _stop: tokio::sync::oneshot::Sender<()>,
}

/// "/a/v1/messages" → "/a", "/v1/messages" → "".
fn prefix(path: &str) -> String {
    if path.starts_with("/v1/") {
        return String::new();
    }
    let rest = &path[1..];
    format!("/{}", &rest[..rest.find('/').unwrap_or(rest.len())])
}

/// The scripted reply for the path's prefix (default `ANTH_SSE`), in 3 chunks at byte thirds.
async fn upstream_handler(st: Arc<UpState>, req: Request) -> Response {
    let path = req.uri().path_and_query().map(|p| p.as_str().to_string()).unwrap_or_default();
    let body = axum::body::to_bytes(req.into_body(), usize::MAX).await.unwrap().to_vec();
    let p = prefix(&path);
    st.got.lock().unwrap().push(Got { path, body });
    let r = st.script.lock().unwrap().get_mut(&p).and_then(|q| q.pop_front()).unwrap_or_else(|| sse_reply(ANTH_SSE));
    let b = r.body.into_bytes();
    let (a, c) = (b.len() / 3, 2 * b.len() / 3);
    let parts: Vec<Result<Bytes, std::io::Error>> =
        [&b[..a], &b[a..c], &b[c..]].iter().map(|s| Ok(Bytes::copy_from_slice(s))).collect();
    let body = if r.ctype == CUT {
        // The first third, then (after one yield, so the headers are flushed first) a read error.
        let cut = futures_util::stream::once(async {
            tokio::task::yield_now().await;
            Err::<Bytes, _>(std::io::Error::other("cut"))
        });
        Body::from_stream(futures_util::StreamExt::chain(futures_util::stream::iter(parts.into_iter().take(1)), cut))
    } else {
        Body::from_stream(futures_util::stream::iter(parts))
    };
    Response::builder().status(r.status).header("content-type", r.ctype).body(body).unwrap()
}

impl Upstream {
    fn start() -> Upstream {
        let st = Arc::new(UpState::default());
        let l = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        l.set_nonblocking(true).unwrap();
        let addr = l.local_addr().unwrap();
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let s2 = st.clone();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
            rt.block_on(async move {
                let l = tokio::net::TcpListener::from_std(l).unwrap();
                let app = axum::Router::new().fallback(move |req: Request| upstream_handler(s2.clone(), req));
                tokio::select! {
                    _ = async { axum::serve(l, app).await } => {}
                    _ = rx => {}
                }
            });
        });
        Upstream { addr, st, _stop: tx }
    }
    fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.addr.port())
    }
    fn script(&self, prefix: &str, r: Vec<Reply>) {
        self.st.script.lock().unwrap().insert(prefix.to_string(), r.into());
    }
    fn got(&self) -> Vec<Got> {
        self.st.got.lock().unwrap().clone()
    }
    fn count(&self, p: &str) -> usize {
        self.got().iter().filter(|g| prefix(&g.path) == p).count()
    }
    /// The last request under `p`.
    fn at(&self, p: &str) -> Got {
        self.got().into_iter().rfind(|g| prefix(&g.path) == p).unwrap_or_else(|| panic!("no request at {p:?}"))
    }
}

struct TestKeys(HashMap<String, Secret>);

impl KeyResolver for TestKeys {
    fn resolve(&self, r: &KeyRef) -> Option<Secret> {
        match r {
            KeyRef::Keychain(n) => self.0.get(n).cloned(),
            KeyRef::Env(_) => None,
        }
    }
}

/// Built at run time (no credential-shaped literal).
fn keys() -> TestKeys {
    TestKeys(["k-oai"].iter().map(|n| (n.to_string(), Secret::new(format!("test-key-{n}-{}", ulid())))).collect())
}

/// One `[[model.provider]]` block; an `openai*` provider has `key = "keychain:k-oai"`.
fn prov(name: &str, url: &str, dialects: &str, upstream: &str) -> String {
    let key = if dialects.contains("openai") { "key = \"keychain:k-oai\"\n" } else { "" };
    format!("[[model.provider]]\nname = \"{name}\"\nbase_url = \"{url}\"\ndialects = {dialects}\nupstream_model = \"{upstream}\"\n{key}\n")
}

const F: &str = "[model.facts]\nreasoning = \"always\"\neffort_levels = [\"low\",\"high\"]\nmax_output_tokens = 128000\ncontext_window = 200000\n\
sampling = true\nvision = true\nstructured_output = \"native\"\nforced_tool_choice = true\n\n";

fn model(id: &str, facts: bool, p: String) -> String {
    format!("[[model]]\nid = \"{id}\"\n{}{p}\n", if facts { F } else { "" })
}

fn config(u: &str, strict: bool) -> String {
    let (o, c, a) = ("[\"openai\"]", "[\"openai-chat\"]", "[\"anthropic\"]");
    let mut t = format!("[proxy]\nstrict_schemas = {strict}\n\n[role.code]\nmodel = \"f-oai\"\n\n");
    t += &model("f-oai", true, prov("p-f", &format!("{u}/f"), o, "up-f"));
    t += &model("f-chat", true, prov("p-fc", &format!("{u}/fc"), c, "up-fc"));
    t += &model("n-oai", false, prov("p-n", &format!("{u}/n"), o, "up-n"));
    t += &model("a-anth", false, prov("p-a", &format!("{u}/a"), a, "a-anth"));
    t += &model("pt-oai", false, prov("p-pt", &format!("{u}/pt"), o, "claude-test-1"));
    t
}

fn paths(d: &TempDir) -> Paths {
    let d = d.path();
    Paths {
        config_file: d.join("config.toml"),
        cache_dir: d.join("cache"),
        data_dir: d.join("data"),
        sessions_dir: d.join("cache/sessions"),
        marketplaces_dir: d.join("cache/marketplaces"),
        plugin_store: d.join("data/plugins"),
        approvals_file: d.join("data/approvals.toml"),
        usage_db: d.join("data/usage.db"),
    }
}

fn cfg(d: &TempDir, toml: &str) -> Merged {
    std::fs::write(d.path().join("config.toml"), toml).unwrap();
    load(&LoadContext { paths: paths(d), repo: None, team: None }).unwrap()
}

/// A pass that must finish never hits its deadline.
fn generous() -> RedactLimits {
    RedactLimits {
        secret: DeadlinePolicy { base: Duration::from_secs(60), per_mib: Duration::ZERO, expire_after_scans: None },
        content_budget: Duration::from_secs(60),
        ..Default::default()
    }
}

struct T {
    d: TempDir,
    up: Upstream,
    anth: Upstream,
    h: ProxyHandle,
}

fn setup(strict: bool) -> T {
    let d = TempDir::new().unwrap();
    let (up, anth) = (Upstream::start(), Upstream::start());
    let o = StartOpts {
        clock: ManualClock::new(),
        keys: Arc::new(keys()),
        usage_db: Some(d.path().join("data/usage.db")),
        upstream_test: Some(vec![("api.anthropic.com".into(), anth.addr)]),
        redact_limits: Some(generous()),
        ..Default::default()
    };
    let h = Proxy::start_local_with(Arc::new(cfg(&d, &config(&up.url(), strict))), o).unwrap();
    T { d, up, anth, h }
}

fn grant(t: &T, passthrough: bool) -> SessionGrant {
    t.h.create_session(SessionRequest {
        agent: Agent::ClaudeCode,
        agent_version: "2.1.328".into(),
        profile: None,
        passthrough,
        repo: None,
        work: None,
        stored_values: Vec::new(),
        known_plugins: Default::default(),
    })
    .unwrap()
}

fn http_client() -> reqwest::Client {
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::builder().no_proxy().build().unwrap()
}

/// A request to the proxy with exactly `headers` (plus content-type when a body is sent).
fn send(h: &ProxyHandle, method: &str, path: &str, headers: &[(&str, &str)], body: Option<Vec<u8>>) -> (u16, HeaderMap, Vec<u8>) {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let m = reqwest::Method::from_bytes(method.as_bytes()).unwrap();
        let mut rb = http_client().request(m, h.url().join(path).unwrap());
        for (k, v) in headers {
            rb = rb.header(*k, *v);
        }
        if let Some(b) = body {
            rb = rb.header("content-type", "application/json").body(b);
        }
        let r = rb.send().await.unwrap();
        let s = r.status().as_u16();
        let hd = r.headers().clone();
        (s, hd, r.bytes().await.unwrap().to_vec())
    })
}

fn auth(g: &SessionGrant) -> Vec<(&str, &str)> {
    vec![("x-api-key", g.token.as_str()), ("x-zygo-session", g.session.as_str())]
}

fn pt(g: &SessionGrant) -> Vec<(&str, &str)> {
    vec![("x-zygo-token", g.token.as_str()), ("x-zygo-session", g.session.as_str())]
}

fn post(t: &T, g: &SessionGrant, body: &Value) -> (u16, HeaderMap, Vec<u8>) {
    send(&t.h, "POST", M, &auth(g), Some(body.to_string().into_bytes()))
}

/// The usage rows of `g`, once `n` rows were written in all.
fn rows(t: &T, g: &SessionGrant, n: u64) -> Vec<RequestRow> {
    assert!(t.h.usage_written(n, Duration::from_secs(10)), "usage rows not written");
    Store::open(&t.d.path().join("data/usage.db")).unwrap().requests_for_session(&g.session).unwrap()
}

fn hv<'a>(h: &'a HeaderMap, n: &str) -> &'a str {
    h.get(n).map(|v| v.to_str().unwrap()).unwrap_or("")
}

fn adj(h: &HeaderMap) -> &str {
    hv(h, "x-zygo-adjustments")
}

fn body_json(g: &Got) -> Value {
    serde_json::from_slice(&g.body).unwrap()
}

/// The SSE events of a response body.
fn events(b: &[u8]) -> Vec<SseEvent> {
    let s = std::str::from_utf8(b).unwrap();
    s.split("\n\n")
        .filter(|e| !e.trim().is_empty())
        .map(|e| {
            let mut name = String::new();
            let mut data = Value::Null;
            for l in e.lines() {
                if let Some(x) = l.strip_prefix("event: ") {
                    name = x.into();
                } else if let Some(x) = l.strip_prefix("data: ") {
                    data = serde_json::from_str(x).unwrap();
                }
            }
            ev(&name, data)
        })
        .collect()
}

/// The `content` the events build.
fn assemble(evs: &[SseEvent]) -> Value {
    let mut blocks: Vec<Value> = Vec::new();
    let mut partial: Vec<String> = Vec::new();
    for e in evs {
        let d = &e.data;
        let i = d["index"].as_u64().unwrap_or(0) as usize;
        match e.event.as_str() {
            "content_block_start" => {
                blocks.push(d["content_block"].clone());
                partial.push(String::new());
            }
            "content_block_delta" => {
                let x = &d["delta"];
                let b = &mut blocks[i];
                match x["type"].as_str().unwrap() {
                    "text_delta" => b["text"] = json!(b["text"].as_str().unwrap().to_string() + x["text"].as_str().unwrap()),
                    "input_json_delta" => partial[i] += x["partial_json"].as_str().unwrap(),
                    _ => {}
                }
            }
            "content_block_stop" if blocks[i]["type"] == json!("tool_use") && !partial[i].is_empty() => {
                blocks[i]["input"] = serde_json::from_str(&partial[i]).unwrap();
            }
            _ => {}
        }
    }
    Value::Array(blocks)
}

fn tokens(r: &RequestRow) -> (u64, u64, u64, u64) {
    (r.input_tokens, r.output_tokens, r.cache_read_tokens, r.cache_write_tokens)
}

fn hi() -> Value {
    json!([{"role":"user","content":"hi"}])
}

#[allow(non_snake_case)]
fn TEXT_SSE() -> String {
    sse(&[
        ("response.created", json!({"response":{"id":"resp_3","model":"up-f","status":"in_progress","output":[],"usage":null}})),
        ("response.output_item.added", json!({"output_index":0,"item":{"type":"message","id":"msg_1","role":"assistant","content":[]}})),
        ("response.output_text.delta", json!({"output_index":0,"content_index":0,"delta":"ok"})),
        ("response.content_part.done", json!({"output_index":0,"content_index":0,"part":{"type":"output_text","text":"ok"}})),
        ("response.output_item.done", json!({"output_index":0,"item":{"type":"message"}})),
        ("response.completed", json!({"response":{"id":"resp_3","status":"completed","model":"up-f","usage":{"input_tokens":10,"output_tokens":2}}})),
    ])
}

#[allow(non_snake_case)]
fn CHAT_TEXT_SSE() -> String {
    [
        chat_delta(json!({"role":"assistant","content":"ok"}), Value::Null),
        chat_delta(json!({}), json!("stop")),
        chunk(json!({"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":2}})),
        DONE.to_string(),
    ]
    .concat()
}

#[allow(non_snake_case)]
fn TOOL_SSE() -> String {
    sse(&[
        ("response.created", json!({"response":{"id":"resp_2","model":"up-f","status":"in_progress","output":[],"usage":null}})),
        ("response.output_item.added", json!({"output_index":0,"item":{"type":"function_call","id":"fc_1","call_id":"call_9","name":"get","arguments":""}})),
        ("response.function_call_arguments.delta", json!({"output_index":0,"delta":"{\"a\":nu"})),
        ("response.function_call_arguments.delta", json!({"output_index":0,"delta":"ll,\"b\":2}"})),
        ("response.output_item.done", json!({"output_index":0,"item":{"type":"function_call","call_id":"call_9","name":"get","arguments":"{\"a\":null,\"b\":2}"}})),
        ("response.completed", json!({"response":{"id":"resp_2","status":"completed","model":"up-f","usage":{"input_tokens":10,"output_tokens":3}}})),
    ])
}

fn anth_tool_sse(d1: &str, d2: &str) -> String {
    sse(&[
        ("message_start", json!({"message":{"id":"msg_2","type":"message","role":"assistant","model":"a-anth","content":[],"stop_reason":null,"stop_sequence":null,
            "usage":{"input_tokens":10,"output_tokens":1}}})),
        ("content_block_start", json!({"index":0,"content_block":{"type":"tool_use","id":"toolu_1","name":"get","input":{}}})),
        ("content_block_delta", json!({"index":0,"delta":{"type":"input_json_delta","partial_json":d1}})),
        ("content_block_delta", json!({"index":0,"delta":{"type":"input_json_delta","partial_json":d2}})),
        ("content_block_stop", json!({"index":0})),
        ("message_delta", json!({"delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":5}})),
        ("message_stop", json!({})),
    ])
}

fn tool_ab() -> Value {
    json!({"name":"get","description":"d","input_schema":{"type":"object","properties":{"a":{"type":"string"},"b":{"type":"integer"}},"required":["b"]}})
}

fn wide_tool() -> Value {
    let mut p = serde_json::Map::new();
    p.insert("n".into(), json!({"type":"integer","minimum":1}));
    for i in 0..25 {
        p.insert(format!("p{i}"), json!({"type":"string"}));
    }
    json!({"name":"get","input_schema":{"type":"object","properties":p,"required":["n"]}})
}

/// The rewritten `wide_tool` schema (anthropic rules).
fn assert_wide_out(s: &Value) {
    assert_eq!(s["properties"]["n"], json!({"type":"integer","description":"{\"minimum\":1}"}));
    assert_eq!(s["properties"]["p24"], json!({"type":["string","null"]}));
    assert_eq!(s["properties"]["p0"], json!({"type":"string"}));
    assert_eq!(s["required"], json!(["n", "p24"]));
    assert_eq!(s["additionalProperties"], json!(false));
}

fn wide_req(model: &str) -> Value {
    json!({"model":model,"max_tokens":64,"stream":true,"tools":[wide_tool()],"messages":hi()})
}

#[test]
fn wire_effort_clamp_on_usage_row() {
    let t = setup(false);
    let g = grant(&t, false);
    t.up.script("/f", vec![sse_reply(&TEXT_SSE())]);
    let r = post(&t, &g, &json!({"model":"f-oai","max_tokens":64,"stream":true,"output_config":{"effort":"medium"},"messages":hi()}));
    assert_eq!(r.0, 200, "{}", String::from_utf8_lossy(&r.2));
    let b = body_json(&t.up.at("/f"));
    assert_eq!(b["reasoning"], json!({"effort":"low"}));
    assert!(b.get("output_config").is_none() && b.get("thinking").is_none());
    assert!(adj(&r.1).contains("effort_clamped:medium>low"), "{}", adj(&r.1));
    assert!(rows(&t, &g, 1)[0].adjustments.iter().any(|a| a == "effort_clamped:medium>low"));

    t.up.script("/fc", vec![sse_reply(&CHAT_TEXT_SSE())]);
    let r = post(&t, &g, &json!({"model":"f-chat","max_tokens":64,"stream":true,"output_config":{"effort":"max"},"messages":hi()}));
    assert_eq!(r.0, 200, "{}", String::from_utf8_lossy(&r.2));
    assert_eq!(body_json(&t.up.at("/fc"))["reasoning_effort"], json!("high"));
    let rw = rows(&t, &g, 2);
    assert!(rw.iter().any(|x| x.adjustments.iter().any(|a| a == "effort_clamped:max>high")));
}

#[test]
fn wire_facts_missing() {
    let t = setup(false);
    let g = grant(&t, false);
    t.up.script("/n", vec![sse_reply(&TEXT_SSE())]);
    let r = post(&t, &g, &json!({"model":"n-oai","max_tokens":64,"stream":true,"output_config":{"effort":"high"},"messages":hi()}));
    assert_eq!(r.0, 200, "{}", String::from_utf8_lossy(&r.2));
    assert!(body_json(&t.up.at("/n")).get("reasoning").is_none());
    assert!(adj(&r.1).contains("facts_missing"), "{}", adj(&r.1));
    assert!(!adj(&r.1).contains("effort_"));
}

#[test]
fn wire_strict_restore_streamed() {
    let t = setup(false);
    let g = grant(&t, false);
    t.up.script("/f", vec![sse_reply(&TOOL_SSE())]);
    let r = post(&t, &g, &json!({"model":"f-oai","max_tokens":64,"stream":true,"tools":[tool_ab()],"messages":hi()}));
    assert_eq!(r.0, 200, "{}", String::from_utf8_lossy(&r.2));
    assert_eq!(assemble(&events(&r.2))[0], json!({"type":"tool_use","id":"call_9","name":"get","input":{"b":2}}));
    let b = body_json(&t.up.at("/f"));
    assert_eq!(b["tools"][0]["parameters"],
        json!({"type":"object","properties":{"a":{"type":["string","null"]},"b":{"type":"integer"}},"required":["b","a"],"additionalProperties":false}));
    assert_eq!(b["tools"][0]["strict"], json!(false));
    let a = adj(&r.1);
    let (c, o) = (a.find("schema:closed_object:1"), a.find("schema:optional_property:1"));
    assert!(c.is_some() && o.is_some() && c < o, "{a}");
    let row = &rows(&t, &g, 1)[0];
    assert!(["schema:closed_object:1", "schema:optional_property:1"].iter().all(|x| row.adjustments.iter().any(|a| a == x)));
}

#[test]
fn wire_strict_untranslated() {
    let t = setup(true);
    let g = grant(&t, false);
    t.up.script("/a", vec![sse_reply(&anth_tool_sse("{\"p24\":nu", "ll,\"p0\":\"x\",\"n\":2}"))]);
    let r = post(&t, &g, &wide_req("a-anth"));
    assert_eq!(r.0, 200, "{}", String::from_utf8_lossy(&r.2));
    assert_wide_out(&body_json(&t.up.at("/a"))["tools"][0]["input_schema"]);
    assert_eq!(assemble(&events(&r.2))[0]["input"], json!({"p0":"x","n":2}));
    let a = adj(&r.1);
    for x in ["schema:closed_object:1", "schema:numeric_bounds:1", "schema:optional_properties_max:1"] {
        assert!(a.contains(x), "{a}");
    }
    assert!(!a.contains("facts_missing"));
    assert_eq!(tokens(&rows(&t, &g, 1)[0]), (10, 5, 0, 0));
}

#[test]
fn wire_untranslated_unchanged_without_strict() {
    let t = setup(false);
    let g = grant(&t, false);
    t.up.script("/a", vec![sse_reply(&anth_tool_sse("{\"p24\":nu", "ll,\"p0\":\"x\",\"n\":2}"))]);
    let sent = wide_req("a-anth").to_string().into_bytes();
    let r = send(&t.h, "POST", M, &auth(&g), Some(sent.clone()));
    assert_eq!(r.0, 200);
    assert!(t.up.at("/a").body == sent);
    assert!(!adj(&r.1).contains("schema:"));
}

#[test]
fn wire_strict_restore_non_stream() {
    let t = setup(true);
    let g = grant(&t, false);
    t.up.script("/f", vec![json_reply(200, json!({"id":"resp_4","status":"completed","model":"up-f",
        "output":[{"type":"function_call","call_id":"call_9","name":"get","arguments":"{\"a\":null,\"b\":2}"}],"usage":{"input_tokens":5,"output_tokens":1}}))]);
    let r = post(&t, &g, &json!({"model":"f-oai","max_tokens":64,"tools":[tool_ab()],"messages":hi()}));
    assert_eq!(r.0, 200, "{}", String::from_utf8_lossy(&r.2));
    let v: Value = serde_json::from_slice(&r.2).unwrap();
    assert_eq!(v["content"][0]["input"], json!({"b":2}));

    t.up.script("/a", vec![json_reply(200, json!({"id":"msg_2","type":"message","role":"assistant","model":"a-anth",
        "content":[{"type":"tool_use","id":"toolu_1","name":"get","input":{"p24":null,"p0":"x","n":2}}],
        "stop_reason":"tool_use","stop_sequence":null,"usage":{"input_tokens":5,"output_tokens":1}}))]);
    let r = post(&t, &g, &json!({"model":"a-anth","max_tokens":64,"tools":[wide_tool()],"messages":hi()}));
    assert_eq!(r.0, 200, "{}", String::from_utf8_lossy(&r.2));
    let v: Value = serde_json::from_slice(&r.2).unwrap();
    assert_eq!(v["content"][0]["input"], json!({"p0":"x","n":2}));
    let cl = hv(&r.1, "content-length");
    assert!(cl.is_empty() || cl == r.2.len().to_string(), "{cl}");
}

#[test]
fn wire_strict_armed_no_null_byte_identical() {
    let t = setup(true);
    let g = grant(&t, false);
    let s = anth_tool_sse("{\"p0\":", "\"x\",\"n\":2}");
    t.up.script("/a", vec![sse_reply(&s)]);
    let r = post(&t, &g, &wide_req("a-anth"));
    assert_eq!(r.0, 200);
    assert!(r.2 == s.as_bytes(), "{}", String::from_utf8_lossy(&r.2));
    assert!(adj(&r.1).contains("schema:optional_properties_max:1"), "{}", adj(&r.1));
}

#[test]
fn wire_strict_passthrough() {
    let t = setup(true);
    let g = grant(&t, true);
    let mut hd = pt(&g);
    hd.push(("authorization", OAT_BEARER));
    let r = send(&t.h, "POST", M, &hd, Some(wide_req("claude-test-1").to_string().into_bytes()));
    assert_eq!(r.0, 200, "{}", String::from_utf8_lossy(&r.2));
    let got = t.anth.at("");
    assert_eq!(got.path, "/v1/messages");
    assert_wide_out(&body_json(&got)["tools"][0]["input_schema"]);
    assert!(adj(&r.1).contains("schema:optional_properties_max:1"), "{}", adj(&r.1));
    assert_eq!(t.up.count("/pt"), 0);
}

#[test]
fn wire_strict_count_tokens_untranslated() {
    let t = setup(true);
    let g = grant(&t, false);
    t.up.script("/a", vec![json_reply(200, json!({"input_tokens":7}))]);
    let body = |m: &str| json!({"model":m,"tools":[wide_tool()],"messages":hi()}).to_string().into_bytes();
    let r = send(&t.h, "POST", "anthropic/v1/messages/count_tokens", &auth(&g), Some(body("a-anth")));
    assert_eq!(r.0, 200, "{}", String::from_utf8_lossy(&r.2));
    assert_eq!(serde_json::from_slice::<Value>(&r.2).unwrap(), json!({"input_tokens":7}));
    let got = t.up.at("/a");
    assert_eq!(got.path, "/a/v1/messages/count_tokens");
    assert_wide_out(&body_json(&got)["tools"][0]["input_schema"]);
    assert!(adj(&r.1).contains("schema:optional_properties_max:1"), "{}", adj(&r.1));

    let r = send(&t.h, "POST", "anthropic/v1/messages/count_tokens", &auth(&g), Some(body("f-oai")));
    assert_eq!(r.0, 200, "{}", String::from_utf8_lossy(&r.2));
    assert!(serde_json::from_slice::<Value>(&r.2).unwrap()["input_tokens"].as_u64().is_some());
    assert_eq!(t.up.count("/f"), 0);
    assert!(adj(&r.1).contains("count_estimated"));
    assert!(!adj(&r.1).contains("schema:"));
}

#[test]
fn wire_ping_in_block_without_null_byte_identical() {
    let t = setup(true);
    let g = grant(&t, false);
    let s = anth_tool_sse("{\"p0\":", "\"x\",\"n\":2}");
    let cut = s.find("event: content_block_delta").unwrap();
    let cut2 = cut + s[cut..].find("\n\n").unwrap() + 2;
    let s = format!("{}event: ping\ndata: {{\"type\":\"ping\"}}\n\n{}", &s[..cut2], &s[cut2..]);
    t.up.script("/a", vec![sse_reply(&s)]);
    let r = post(&t, &g, &wide_req("a-anth"));
    assert_eq!(r.0, 200);
    assert_eq!(String::from_utf8_lossy(&r.2), s);
    assert!(adj(&r.1).contains("schema:optional_properties_max:1"), "{}", adj(&r.1));
}

/// Round 2 (F6): a read error gives 503 `zygo_provider_unavailable`; the row has the provider and the adjustments.
fn assert_read_error_row(t: &T, g: &SessionGrant, r: &(u16, HeaderMap, Vec<u8>), provider: &str) {
    assert_eq!(r.0, 503, "{}", String::from_utf8_lossy(&r.2));
    let v: Value = serde_json::from_slice(&r.2).unwrap();
    assert_eq!(v["error"]["type"], json!("zygo_provider_unavailable"));
    let rw = rows(t, g, 1);
    assert_eq!(rw[0].status, 503);
    assert_eq!(rw[0].provider, provider);
    assert!(rw[0].adjustments.iter().any(|a| a.starts_with("schema:")), "{:?}", rw[0].adjustments);
}

#[test]
fn wire_read_error_row_untranslated() {
    let t = setup(true);
    let g = grant(&t, false);
    t.up.script("/a", vec![cut_reply(json!({"id":"msg_2","type":"message","role":"assistant","model":"a-anth",
        "content":[{"type":"tool_use","id":"toolu_1","name":"get","input":{"p24":null,"p0":"x","n":2}}],
        "stop_reason":"tool_use","stop_sequence":null,"usage":{"input_tokens":5,"output_tokens":1}}))]);
    let r = post(&t, &g, &json!({"model":"a-anth","max_tokens":64,"tools":[wide_tool()],"messages":hi()}));
    assert_read_error_row(&t, &g, &r, "p-a");
}

#[test]
fn wire_read_error_row_translated() {
    let t = setup(false);
    let g = grant(&t, false);
    t.up.script("/f", vec![cut_reply(json!({"id":"resp_4","status":"completed","model":"up-f",
        "output":[{"type":"function_call","call_id":"call_9","name":"get","arguments":"{\"a\":null,\"b\":2}"}],"usage":{"input_tokens":5,"output_tokens":1}}))]);
    let r = post(&t, &g, &json!({"model":"f-oai","max_tokens":64,"tools":[tool_ab()],"messages":hi()}));
    assert_read_error_row(&t, &g, &r, "p-f");
}

// ---- Round 3 ----

#[test]
fn sch_all_of_merges_property_schemas() {
    // Round 3 (D1): the same property in two branches merges both schemas; no constraint is dropped.
    let mut b = one_tool(json!({"type":"object","allOf":[
        {"properties":{"a":{"type":"string"}}},
        {"properties":{"a":{"maxLength":3}}}
    ]}));
    let (c, _) = rewrite(&mut b, &[rule(Family::Openai, "all_of", RuleSeverity::Refuse, None)]);
    let s = &b["tools"][0]["input_schema"];
    assert_eq!(s, &json!({"type":"object","properties":{"a":{"type":"string","maxLength":3}}}));
    assert_eq!(codes(&c), vec!["schema:all_of:1"]);
}

#[test]
fn sch_all_of_conflict_unrewritten() {
    // Round 3 (D1): values that cannot merge leave the allOf node as the client sent it, not counted.
    for (x, y) in [(json!({"type":"string"}), json!({"type":"integer"})), (json!({"maxLength":3}), json!({"maxLength":4}))] {
        let sch = json!({"type":"object","allOf":[{"properties":{"a":x}},{"properties":{"a":y}}]});
        let mut b = one_tool(sch.clone());
        let (c, _) = rewrite(&mut b, &[rule(Family::Openai, "all_of", RuleSeverity::Refuse, None)]);
        assert_eq!(b["tools"][0]["input_schema"], sch);
        assert!(c.is_empty(), "{:?}", codes(&c));
    }
}

#[test]
fn sch_optional_type_null_enum_without_null() {
    // Round 3 (D2): a `type` array with "null" but an `enum` without null does not accept null: null injected.
    let mut b = one_tool(json!({"type":"object","properties":{"a":{"type":["string","null"],"enum":["x"]}}}));
    let (_, nulls) = rewrite(&mut b, &[rule(Family::Openai, "optional_property", RuleSeverity::Refuse, None)]);
    let s = &b["tools"][0]["input_schema"];
    assert_eq!(s["properties"]["a"], json!({"type":["string","null"],"enum":["x",null]}));
    assert_eq!(s["required"], json!(["a"]));
    assert_eq!(nulls.tools.get("get"), Some(&vec![vec![k("a")]]));
}

#[test]
fn sch_all_of_nested_one_of_rewritten() {
    // Round 3 (D3): one_of and map_object rewrite inside allOf branches before all_of merges them. The built-in
    // anthropic rules are used: openai has no one_of rule (N4 note).
    let mut b = one_tool(json!({"type":"object","allOf":[{"oneOf":[{"type":"object"},{"type":"null"}]},
        {"properties":{"m":{"type":"object","additionalProperties":{"type":"string"}}}}]}));
    let (c, _) = rewrite(&mut b, &schema_rules().into_iter().filter(|r| r.family == Family::Anthropic).collect::<Vec<_>>());
    let s = &b["tools"][0]["input_schema"];
    assert!(!s.to_string().contains("oneOf"), "oneOf survives the rewrite: {s}");
    assert!(!s.to_string().contains("allOf"), "{s}");
    assert_eq!(s["properties"]["m"]["additionalProperties"], json!(false), "{s}");
    let c = codes(&c);
    assert!(c.starts_with(&["schema:map_object:1", "schema:one_of:1", "schema:all_of:1"]), "{c:?}");
}

#[test]
fn wire_bad_response_row_provider() {
    // Round 3 (D4): a translated 2xx body that is not a response: 502, and the row has the provider and adjustments.
    let t = setup(false);
    let g = grant(&t, false);
    t.up.script("/f", vec![json_reply(200, json!({"not":"a response"}))]);
    let r = post(&t, &g, &json!({"model":"f-oai","max_tokens":64,"tools":[tool_ab()],"messages":hi()}));
    assert_eq!(r.0, 502, "{}", String::from_utf8_lossy(&r.2));
    let rw = rows(&t, &g, 1);
    assert_eq!(rw.len(), 1);
    assert_eq!(rw[0].status, 502);
    assert_eq!(rw[0].provider, "p-f", "row: {:?}", rw[0].adjustments);
    assert!(rw[0].adjustments.iter().any(|a| a.starts_with("schema:")), "{:?}", rw[0].adjustments);
}
