use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashMap};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use zygo_config::{builtin_patterns, Action, RedactionConfig, RedactionPattern};
use zygo_redact::act_lite_store::{self, Lookup, StoredValue};
use zygo_redact::cache::{self, CachePool, Entry, ENTRY_OVERHEAD, SESSION_CAP_BYTES};
use zygo_redact::secret::{redact, redact_after_wait, scan_calls, set_lookup_delay, Findings, RedactError};
use zygo_redact::{compile, compile_with_store, Compiled, DeadlinePolicy, SessionCache};

fn lk(vars: &[(&str, &str)], home: Option<&str>, known: Option<&str>, windows: bool) -> Lookup {
    let m: HashMap<String, OsString> = vars.iter().map(|(k, v)| (k.to_string(), OsString::from(v))).collect();
    Lookup { env: Box::new(move |k| m.get(k).cloned()), home: home.map(PathBuf::from), known_config: known.map(PathBuf::from), windows }
}
fn act(cfg: &Path) -> Lookup { lk(&[("ACT_CONFIG_DIR", cfg.to_str().unwrap())], None, None, false) }

/// A repository with `.git` and an act-lite store under its own config directory.
struct Fx { _t: tempfile::TempDir, repo: PathBuf, cfg: PathBuf }
fn fixture(store: Option<&str>) -> Fx {
    let t = tempfile::tempdir().unwrap();
    let base = t.path().canonicalize().unwrap();
    let repo = base.join("repo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    std::fs::create_dir_all(repo.join("src")).unwrap();
    let cfg = base.join("act");
    std::fs::create_dir_all(cfg.join("secrets")).unwrap();
    if let Some(text) = store {
        let file = cfg.join("secrets").join(format!("{}.env", act_lite_store::project_id(&repo)));
        std::fs::write(file, text).unwrap();
    }
    Fx { _t: t, repo, cfg }
}

#[test]
fn act_lite_store_dir_rules() {
    let d = |l: Lookup| act_lite_store::config_dir(&l);
    assert_eq!(d(lk(&[("ACT_CONFIG_DIR", "/a"), ("XDG_CONFIG_HOME", "/x")], Some("/h"), None, false)), PathBuf::from("/a"));
    assert_eq!(d(lk(&[("XDG_CONFIG_HOME", "/x")], Some("/h"), None, false)), PathBuf::from("/x").join("act"));
    assert_eq!(d(lk(&[], Some("/h"), None, false)), PathBuf::from("/h").join(".config").join("act"));
    assert_eq!(d(lk(&[], None, None, false)), PathBuf::from(".").join(".config").join("act")); // act-lite fallback
    assert_eq!(d(lk(&[("XDG_CONFIG_HOME", "/x")], Some("/h"), Some("/roaming"), true)), PathBuf::from("/roaming").join("act"));
    assert_eq!(d(lk(&[], Some("/h"), None, true)), PathBuf::from(".config").join("act")); // act-lite fallback
    assert_eq!(d(lk(&[("ACT_CONFIG_DIR", "/a")], None, Some("/roaming"), true)), PathBuf::from("/a"));
}

#[test]
fn act_lite_project_id_matches_act_lite() {
    // act-lite paths.rs: first 8 bytes of sha256("/home/dev/project"), hex
    assert_eq!(act_lite_store::project_id(Path::new("/home/dev/project")), "1afbf223bb0b58ba");
}

#[test]
fn act_lite_worktree_shares_main_store_and_submodule_has_its_own() {
    let t = tempfile::tempdir().unwrap();
    let base = t.path().canonicalize().unwrap();
    let main = base.join("main");
    std::fs::create_dir_all(main.join(".git").join("worktrees").join("wt1")).unwrap();
    let wt = base.join("wt1");
    std::fs::create_dir_all(wt.join("src")).unwrap();
    std::fs::write(wt.join(".git"), format!("gitdir: {}\n", main.join(".git").join("worktrees").join("wt1").display())).unwrap();
    let sub = main.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(sub.join(".git"), "gitdir: ../.git/modules/sub\n").unwrap();
    let l = act(Path::new("/cfg"));
    let at = |root: &Path| PathBuf::from("/cfg").join("secrets").join(format!("{}.env", act_lite_store::project_id(root)));
    assert_eq!(act_lite_store::store_path(&wt.join("src"), &l), at(&main));
    assert_eq!(act_lite_store::store_path(&main, &l), at(&main));
    assert_eq!(act_lite_store::store_path(&sub, &l), at(&sub));
}

#[test]
fn act_lite_read_keeps_values_of_six_bytes_or_more() {
    let long = ["fxval", "-7q2"].concat();
    let fx = fixture(Some(&format!("# header\nLONG={long}\nexport QUOTED=\"fx spaced 42\" # note\nSHORT=abcde\nSIX=abcdef\nLONG={long}\n")));
    let v = act_lite_store::read_with(&fx.repo.join("src"), &act(&fx.cfg)).unwrap();
    let got: Vec<&str> = v.iter().map(StoredValue::expose).collect();
    assert_eq!(got, [long.as_str(), "fx spaced 42", "abcdef"]); // a repeat with the same token is accepted
    let xdg = lk(&[("XDG_CONFIG_HOME", fx.cfg.parent().unwrap().to_str().unwrap())], Some("/nowhere"), None, false);
    assert_eq!(act_lite_store::read_with(&fx.repo, &xdg).unwrap(), v);
}

#[test]
fn act_lite_read_errors_hold_no_value() {
    let fx = fixture(None);
    let e = act_lite_store::read_with(&fx.repo, &act(&fx.cfg)).unwrap_err().to_string();
    assert!(e.starts_with("no act-lite store at "), "{e}");
    let v = ["fxval", "-9z8"].concat();
    let fx = fixture(Some(&format!("GOOD={v}\nBAD \"{v}\n")));
    let e = format!("{:#}", act_lite_store::read_with(&fx.repo, &act(&fx.cfg)).unwrap_err());
    assert!(e.contains("line 2: cannot parse") && !e.contains(&v), "{e}");
    assert_eq!(format!("{:?}", StoredValue::new(v)), "StoredValue(<redacted>)");
}

#[test]
fn act_lite_duplicate_key_with_another_value_is_an_error() {
    let (a, b) = (["fxval", "-one1"].concat(), ["fxval", "-two2"].concat());
    let fx = fixture(Some(&format!("K={a}\nOTHER=x\nK={b}\n")));
    let e = format!("{:#}", act_lite_store::read_with(&fx.repo, &act(&fx.cfg)).unwrap_err());
    assert!(e.contains("line 3: duplicate key K") && !e.contains(&a) && !e.contains(&b), "{e}");
}

fn e() -> Entry { Entry { replacement: None, findings: vec![] } }
fn k(n: u8) -> cache::CacheKey { [n; 16] }

#[test]
fn cache_lru_within_session() {
    let p = CachePool::new(2 * ENTRY_OVERHEAD, SESSION_CAP_BYTES);
    let mut s = SessionCache::new(p.clone());
    s.insert(k(1), e());
    s.insert(k(2), e());
    assert!(s.get(&k(1)).is_some()); // 1 is now the most recent
    s.insert(k(3), e());
    assert!(!s.contains(&k(2)));
    assert!(s.contains(&k(1)) && s.contains(&k(3)));
    assert_eq!(p.used_bytes(), 2 * ENTRY_OVERHEAD);
}

#[test]
fn cache_cap_is_shared_across_sessions() {
    let p = CachePool::new(2 * ENTRY_OVERHEAD, SESSION_CAP_BYTES);
    let mut a = SessionCache::new(p.clone());
    let mut b = SessionCache::new(p.clone());
    a.insert(k(1), e());
    assert!(!b.contains(&k(1))); // caches do not see each other's entries
    b.insert(k(2), e());
    b.insert(k(3), e());
    assert!(!a.contains(&k(1))); // a's entry was the least recently used in the process
    assert!(b.contains(&k(2)) && b.contains(&k(3)));
}

#[test]
fn cache_session_cap_and_drop() {
    let p = CachePool::new(1 << 20, ENTRY_OVERHEAD);
    let mut s = SessionCache::new(p.clone());
    s.insert(k(1), e());
    s.insert(k(2), e());
    assert!(!s.contains(&k(1)) && s.contains(&k(2)));
    let p = CachePool::new(1 << 20, SESSION_CAP_BYTES);
    let mut s = SessionCache::new(p.clone());
    let big = Entry { replacement: Some("x".repeat(100)), findings: vec![(0, 1)] };
    assert_eq!(big.size(), ENTRY_OVERHEAD + 100 + 8);
    s.insert(k(1), big.clone());
    assert_eq!(s.get(&k(1)), Some(big));
    s.insert(k(1), e());
    assert_eq!(p.used_bytes(), ENTRY_OVERHEAD);
    drop(s);
    assert_eq!(p.used_bytes(), 0);
}

#[test]
fn cache_counts_at_least_256_bytes_per_entry() {
    let p = CachePool::new(1 << 20, SESSION_CAP_BYTES);
    let mut s = SessionCache::new(p.clone());
    for n in 0..10u8 { s.insert(k(n), e()); }
    assert!(p.used_bytes() >= 10 * 256, "{}", p.used_bytes());
}

/// (id, positive, negative, positive after redaction inside "x … y") for every built-in, in order.
fn samples() -> Vec<(&'static str, String, String, String)> {
    let d = "-".repeat(5);
    let rows: Vec<(&'static str, String, String)> = vec![
        ("anthropic_api_key", ["sk-", "ant-api03-", &"Ab1_".repeat(24)].concat(), ["sk-", "ant-api03-short"].concat()),
        ("openrouter_api_key", ["sk-", "or-v1-", &"0a".repeat(32)].concat(), ["sk-", "or-v1-", &"0a".repeat(10)].concat()),
        ("openai_api_key", ["sk-", "proj-", &"a1".repeat(10), "T3Blbk", "FJ", &"b2".repeat(10)].concat(), ["sk-", "proj-", &"a1".repeat(30)].concat()),
        ("google_api_key", ["AI", "za", &"B".repeat(35)].concat(), ["AI", "za", &"B".repeat(20)].concat()),
        ("aws_access_key_id", ["AK", "IA", "ABCDEFGHIJKLMNOP"].concat(), ["AK", "IA", "ABCDEFGH"].concat()),
        ("azure_storage_key", ["Account", "Key=", &"Ab+/".repeat(21), "Ab", "=="].concat(), ["Account", "Key=", &"Ab".repeat(10), "=="].concat()),
        ("gcp_service_account_key", format!(r#""private_key": "{d}BEGIN PRIVATE KEY{d}\nMIIEvQIBADAN\n{d}END PRIVATE KEY{d}\n""#), r#""private_key": "none""#.to_string()),
        ("github_token", ["gh", "p_", &"a1B2".repeat(9)].concat(), ["gh", "p_", "short1"].concat()),
        ("gitlab_token", ["gl", "pat-", &"x1y2".repeat(5)].concat(), ["gl", "pat-", "abc"].concat()),
        ("slack_token", ["xo", "xb-", "1234567890-abcdef"].concat(), ["xo", "xb-", "123"].concat()),
        ("private_key_pem", format!("{d}BEGIN RSA PRIVATE KEY{d}\nMIIBOgIBAAJB\n{d}END RSA PRIVATE KEY{d}"), format!("{d}BEGIN PUBLIC KEY{d}\nMIIB\n{d}END PUBLIC KEY{d}")),
        ("pgp_private_key_block", format!("{d}BEGIN PGP PRIVATE KEY BLOCK{d}\n\nlQOYBGX\n{d}END PGP PRIVATE KEY BLOCK{d}"), format!("{d}BEGIN PGP PUBLIC KEY BLOCK{d}\nmQIN\n{d}END PGP PUBLIC KEY BLOCK{d}")),
        ("jwt", ["ey", "JhbGciOiJIUzI1NiJ9", ".", "ey", "JzdWIiOiIxMjM0In0", ".", "c2lnbmF0dXJlLXNhbXBsZQ"].concat(), "eyJhbGci.notjwt".to_string()),
        ("zygo_token", ["zyg_", "s_", &"a1".repeat(21), "Z"].concat(), ["zyg_", "s_", "short"].concat()),
        ("generic_secret_assignment", ["password = \"", "fixture-pass-123\""].concat(), ["password = ", "request.form.get(\"password\")"].concat()),
    ];
    rows.into_iter().map(|(id, pos, neg)| {
        let want = if id == "generic_secret_assignment" {
            format!("x password = \"[REDACTED:{id}]\" y")
        } else {
            format!("x [REDACTED:{id}] y")
        };
        (id, pos, neg, want)
    }).collect()
}
fn key() -> String { samples()[0].1.clone() }
const R: &str = "[REDACTED:anthropic_api_key]";
fn tool(t: &str) -> Value { json!({"messages": [{"role": "user", "content": [{"type": "tool_result", "tool_use_id": "t1", "content": t}]}]}) }
fn text(b: &Value) -> String { b["messages"][0]["content"][0]["content"].as_str().unwrap().to_string() }
fn pool() -> Arc<CachePool> { CachePool::new(256 << 20, SESSION_CAP_BYTES) }
fn run(b: &mut Value, c: &Compiled) -> Result<Findings, RedactError> {
    redact(b, c, &mut SessionCache::new(pool()), &DeadlinePolicy::default())
}
fn red(c: &Compiled, t: &str) -> String { let mut b = tool(t); run(&mut b, c).unwrap(); text(&b) }
fn pat(id: &str, re: &str, action: Action) -> RedactionPattern {
    RedactionPattern { id: id.into(), pattern: re.into(), action, description: String::new() }
}
/// Built-ins (as layer 1 gives them) followed by `p`.
fn cfg(p: Vec<RedactionPattern>) -> RedactionConfig {
    RedactionConfig { pattern: builtin_patterns().into_iter().chain(p).collect(), ..Default::default() }
}
fn base() -> RedactionConfig { cfg(vec![]) }

#[test]
fn builtin_ids_and_samples() {
    let b = builtin_patterns();
    assert!(b.iter().all(|p| p.action == Action::Redact));
    let ids: Vec<String> = b.into_iter().map(|p| p.id).collect();
    assert_eq!(ids, samples().iter().map(|x| x.0).collect::<Vec<_>>());
    let c = compile(&base()).unwrap();
    for (id, pos, neg, want) in samples() {
        let mut b = tool(&format!("x {pos} y"));
        let f = run(&mut b, &c).unwrap();
        assert_eq!(text(&b), want, "{id}");
        assert_eq!(f.counts, BTreeMap::from([(id.to_string(), 1)]), "{id}");
        let mut b = tool(&format!("x {neg} y"));
        assert!(run(&mut b, &c).unwrap().counts.is_empty(), "{id} negative");
        assert_eq!(text(&b), format!("x {neg} y"));
    }
}

#[test]
fn builtin_edge_cases() {
    let c = compile(&base()).unwrap();
    let d = "-".repeat(5);
    let aws = ["AK", "IA", "ABCDEFGHIJKLMNOP"].concat();
    assert_eq!(red(&c, &("QUJD".repeat(3) + &aws)), "QUJDQUJDQUJD[REDACTED:aws_access_key_id]"); // glued to base64
    assert_eq!(red(&c, &["xo", "xe-", "1234567890-abc"].concat()), "[REDACTED:slack_token]");
    assert_eq!(red(&c, &["xa", "pp-", "1-A0123456789-abc"].concat()), "[REDACTED:slack_token]");
    assert_eq!(red(&c, &format!("x {d}BEGIN RSA PRIVATE KEY{d}\nMIIBOgIBAAJB")), "x [REDACTED:private_key_pem]"); // truncated
}

#[test]
fn generic_assignment_matches_values_not_code() {
    let c = compile(&base()).unwrap();
    let g = "[REDACTED:generic_secret_assignment]";
    for code in ["password = request.form.get(\"password\")", "const cfg = { secret: process.env.CLIENT_SECRET };",
                 "self.password = hashlib.sha256(x)", "if password == \"abcdefghijklmn\":"] {
        assert_eq!(red(&c, code), code);
    }
    let aws_secret = "abcdEFGH1234/+".repeat(3)[..40].to_string();
    assert_eq!(red(&c, "SECRET_KEY=abcdefghijklmnop"), format!("SECRET_KEY={g}"));
    assert_eq!(red(&c, &format!("AWS_SECRET_ACCESS_KEY={aws_secret}")), format!("AWS_SECRET_ACCESS_KEY={g}"));
    assert_eq!(red(&c, "password = \"fixture-pass-123\""), format!("password = \"{g}\""));
    assert_eq!(red(&c, "{\"api_key\": \"fixture-key-12345\"}"), format!("{{\"api_key\": \"{g}\"}}"));
    assert_eq!(red(&c, "a\nexport DB_PASSWORD=fixture-pass-123\r\nb"), format!("a\nexport DB_PASSWORD={g}\r\nb"));
}

#[test]
fn compile_uses_exactly_the_config_patterns_in_order() {
    let only = RedactionConfig { pattern: vec![pat("tracer_a", "tracera[0-9]{4}", Action::Redact), pat("jwt", "tracerjwt[0-9]{4}", Action::Redact)], ..Default::default() };
    let c = compile(&only).unwrap();
    assert_eq!(c.ids(), ["tracer_a", "jwt"]);
    assert_eq!(red(&c, &format!("tracerjwt1234 tracera5678 {}", key())), format!("[REDACTED:jwt] [REDACTED:tracer_a] {}", key())); // no built-in is added
    assert!(compile(&cfg(vec![pat("tracer_bad", "(", Action::Redact)])).is_err());
}

#[test]
fn overlap_merges_into_the_earlier_pattern() {
    let c = compile(&cfg(vec![pat("tracer_a", "abc[0-9]+", Action::Redact), pat("tracer_b", "[0-9]+xyz", Action::Redact)])).unwrap();
    let mut b = tool("abc123xyz 456xyz");
    let f = run(&mut b, &c).unwrap();
    assert_eq!(text(&b), "[REDACTED:tracer_a] [REDACTED:tracer_b]");
    assert_eq!(f.counts, BTreeMap::from([("tracer_a".to_string(), 1), ("tracer_b".to_string(), 1)]));
}

#[test]
fn later_pattern_starting_before_an_overlap_is_merged_too() {
    let c = compile(&cfg(vec![pat("tracer_a", "[0-9]+xyz", Action::Redact), pat("tracer_b", "abc[0-9]+", Action::Redact)])).unwrap();
    let mut b = tool("abc123xyz");
    let f = run(&mut b, &c).unwrap();
    assert_eq!(text(&b), "[REDACTED:tracer_a]");
    assert_eq!(f.counts, BTreeMap::from([("tracer_a".to_string(), 1)]));
}

#[test]
fn block_lists_ids_only() {
    let c = compile(&cfg(vec![pat("tracer_block", "blockme[0-9]{3}", Action::Block)])).unwrap();
    let mut b = tool("before blockme123 after");
    let e = run(&mut b, &c).unwrap_err();
    assert_eq!(e, RedactError::Blocked { ids: vec!["tracer_block".into()] });
    assert!(!e.to_string().contains("blockme123"));
}

#[test]
fn findings_count_every_occurrence_and_repeat_from_cache() {
    let c = compile(&base()).unwrap();
    let k = key();
    let body = json!(["a", format!("{k} and {k}"), format!("x {k}")]);
    let mut cache = SessionCache::new(pool());
    for _ in 0..2 {
        let mut b = body.clone();
        let f = redact(&mut b, &c, &mut cache, &DeadlinePolicy::default()).unwrap();
        assert_eq!(f.counts, BTreeMap::from([("anthropic_api_key".to_string(), 3)]));
        assert_eq!(b, json!(["a", format!("{R} and {R}"), format!("x {R}")]));
    }
}

#[test]
fn large_base64_string_with_key_at_end_is_redacted() {
    let c = compile(&base()).unwrap();
    assert_eq!(red(&c, &("QUJD".repeat(17920) + &key())), "QUJD".repeat(17920) + R);
}

#[test]
fn scope_is_by_json_path() {
    let c = compile(&base()).unwrap();
    let k = key();
    let pw = ["password=\"", "fixture-pass-123\""].concat();
    let data_url = format!("data:image/png;base64,{k}");
    let mut b = json!({
        "messages": [{"role": "assistant", "content": [
            {"type": "thinking", "thinking": pw, "signature": k},
            {"type": "redacted_thinking", "data": k},
            {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": k}},
            {"type": "document", "source": {"type": "base64", "media_type": "application/pdf", "data": k}},
            {"type": "image_url", "image_url": {"url": data_url}},
            {"type": "tool_result", "tool_use_id": "t1", "content": [
                {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": k}},
                {"type": "text", "text": k}
            ]},
            {"type": "tool_use", "id": "u1", "name": "n", "input": {
                "a": {"type": "reasoning", "summary": k},
                "b": {"type": "thinking", "thinking": k},
                "c": {"type": "image", "source": {"type": "base64", "data": k}}
            }}
        ]}],
        "input": [
            {"type": "reasoning", "summary": [{"type": "summary_text", "text": k}], "encrypted_content": k},
            {"type": "message", "role": "user", "content": [
                {"type": "input_image", "image_url": data_url},
                {"type": "input_file", "file_data": k},
                {"type": "input_text", "text": k}
            ]}
        ],
        "metadata": {"type": "thinking", "thinking": k}
    });
    b["extra"] = Value::Object(Map::from_iter([(k.clone(), json!("v"))])); // keys are never scanned
    let mut want = b.clone();
    for p in ["/messages/0/content/5/content/1/text", "/messages/0/content/6/input/a/summary",
              "/messages/0/content/6/input/b/thinking", "/messages/0/content/6/input/c/source/data",
              "/input/1/content/2/text", "/metadata/thinking"] {
        *want.pointer_mut(p).unwrap() = json!(R);
    }
    run(&mut b, &c).unwrap();
    assert_eq!(b, want);
    let url = format!("https://example.test/?k={k}");
    let mut b = json!({"messages": [{"role": "user", "content": [
        {"type": "image_url", "image_url": {"url": url}},
        {"type": "image", "source": {"type": "url", "url": url}}
    ]}]});
    run(&mut b, &c).unwrap();
    let r = format!("https://example.test/?k={R}");
    assert_eq!(b["messages"][0]["content"][0]["image_url"]["url"], json!(r));
    assert_eq!(b["messages"][0]["content"][1]["source"]["url"], json!(r));
}

#[test]
fn act_lite_values_are_redacted_before_regex_patterns() {
    let long = ["fxval", "-7q2"].concat();
    let fx = fixture(Some(&format!("LONG={long}\nQUOTED=\"fx spaced 42\"\nSHORT=abcde\nKEY={}\n", key())));
    let v = act_lite_store::read_with(&fx.repo, &act(&fx.cfg)).unwrap();
    let c = compile_with_store(&base(), &v).unwrap();
    let mut b = tool(&format!("a {long} b fx spaced 42 c abcde d {}", key()));
    let f = run(&mut b, &c).unwrap();
    let r = "[REDACTED:act_lite_secret]";
    assert_eq!(text(&b), format!("a {r} b {r} c abcde d {r}"));
    assert_eq!(f.counts, BTreeMap::from([("act_lite_secret".to_string(), 3)]));
    let c = compile_with_store(&base(), &[StoredValue::new("abcdef"), StoredValue::new("abcdefgh")]).unwrap();
    assert_eq!(red(&c, "abcdefgh"), r); // the longest stored value wins at a start
}

#[test]
fn version_depends_on_the_value_set_only() {
    let d = base();
    let a = compile_with_store(&d, &[StoredValue::new("value-one"), StoredValue::new("value-two")]).unwrap();
    let b = compile_with_store(&d, &[StoredValue::new("value-two"), StoredValue::new("value-one")]).unwrap();
    assert_eq!(a.version(), b.version());
    assert_ne!(a.version(), compile(&d).unwrap().version());
    assert_eq!(compile(&d).unwrap().version(), compile(&d).unwrap().version());
}

#[test]
fn resend_scans_only_new_strings_and_keeps_bytes() {
    let c = compile(&base()).unwrap();
    let raw1 = format!(r#"{{"model":"m","messages":[{{"role":"user","content":"first {}"}},{{"role":"assistant","content":"second"}}]}}"#, key());
    let turn1: Value = serde_json::from_str(&raw1).unwrap();
    let mut cache = SessionCache::new(pool());
    let mut b1 = turn1.clone();
    redact(&mut b1, &c, &mut cache, &DeadlinePolicy::default()).unwrap();
    assert_eq!(cache.scan_count(), 5); // "m", "user", "first …", "assistant", "second"
    let mut b2 = turn1.clone();
    b2["messages"].as_array_mut().unwrap().push(json!({"role": "user", "content": "third"}));
    redact(&mut b2, &c, &mut cache, &DeadlinePolicy::default()).unwrap();
    assert_eq!(cache.scan_count(), 6);
    let want1 = raw1.replace(&key(), R);
    assert_eq!(serde_json::to_string(&b1).unwrap(), want1);
    let want2 = want1.replace("}]}", r#"},{"role":"user","content":"third"}]}"#);
    assert_eq!(serde_json::to_string(&b2).unwrap(), want2);
    let mut b3 = turn1.clone();
    run(&mut b3, &c).unwrap(); // a fresh cache gives the same bytes
    assert_eq!(serde_json::to_vec(&b3).unwrap(), serde_json::to_vec(&b1).unwrap());
}

#[test]
fn forced_timeout_keeps_scanned_strings() {
    let c = compile(&base()).unwrap();
    let body = json!(["one", "two", "three"]);
    let mut cache = SessionCache::new(pool());
    let hook = |n| DeadlinePolicy { expire_after_scans: Some(n), ..DeadlinePolicy::default() };
    assert_eq!(redact(&mut body.clone(), &c, &mut cache, &hook(1)), Err(RedactError::Timeout));
    assert_eq!(cache.scan_count(), 1);
    redact(&mut body.clone(), &c, &mut cache, &DeadlinePolicy::default()).unwrap();
    assert_eq!(cache.scan_count(), 3); // the retry scanned only the two left
    assert!(redact(&mut body.clone(), &c, &mut cache, &hook(0)).is_ok()); // all cached: no scan
}

#[test]
fn wall_clock_deadline_counts_only_uncached_bytes() {
    let c = compile(&base()).unwrap();
    let zero = DeadlinePolicy { base: Duration::ZERO, per_mib: Duration::ZERO, expire_after_scans: None };
    let body = json!(["one", "two"]);
    let mut cache = SessionCache::new(pool());
    assert_eq!(redact(&mut body.clone(), &c, &mut cache, &zero), Err(RedactError::Timeout));
    assert_eq!(cache.scan_count(), 1); // every attempt scans one uncached string (§16.1 Bound note)
    redact(&mut body.clone(), &c, &mut cache, &DeadlinePolicy::default()).unwrap();
    assert!(redact(&mut body.clone(), &c, &mut cache, &zero).is_ok()); // fully cached: no deadline to miss
}

#[test]
fn deadline_grows_with_uncached_bytes() {
    let p = DeadlinePolicy::default();
    assert_eq!(p.budget(0), Duration::from_millis(200));
    assert_eq!(p.budget(512 << 10), Duration::from_millis(225));
    assert_eq!(p.budget(2 << 20), Duration::from_millis(300));
}

// ---- round 2 (2026-11-18) ----

const G: &str = "[REDACTED:generic_secret_assignment]";
fn pw() -> String { ["Hunter", "Pass", "-", "x9Qz7", "LmN2"].concat() }

#[test]
fn overlapping_matches_merge_so_no_part_of_a_secret_survives() {
    let c = compile_with_store(&base(), &[StoredValue::new("Hunter")]).unwrap();
    let mut b = tool(&format!("password = \"{}\"", pw()));
    let f = run(&mut b, &c).unwrap();
    assert_eq!(text(&b), "password = \"[REDACTED:act_lite_secret]\"");
    assert_eq!(f.counts, BTreeMap::from([("act_lite_secret".to_string(), 1)]));
    let aws = ["AK", "IA", "ABCDEFGHIJKLMNOP"].concat();
    let c = compile(&base()).unwrap();
    assert_eq!(red(&c, &format!("password = \"{aws}-rest-of-pw\"")), "password = \"[REDACTED:aws_access_key_id]\"");
}

#[test]
fn touching_spans_merge_into_the_earliest_id() {
    let c = compile(&cfg(vec![pat("tracer_a", "abc[0-9]+", Action::Redact), pat("tracer_b", "xyz[0-9]+", Action::Redact)])).unwrap();
    assert_eq!(red(&c, "xyz9abc1 abc1 xyz2"), "[REDACTED:tracer_a] [REDACTED:tracer_a] [REDACTED:tracer_b]");
}

/// `\uXXXX` for non-ASCII (UTF-16), `\"`, `\\`; the way Python's `json.dumps` writes a string.
fn ascii_escape(s: &str) -> String {
    let mut o = String::new();
    for ch in s.chars() {
        match ch {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            c if c.is_ascii() => o.push(c),
            c => { let mut u = [0u16; 2]; for x in c.encode_utf16(&mut u) { o.push_str(&format!("\\u{x:04x}")); } }
        }
    }
    o
}

#[test]
fn stored_values_match_in_json_escaped_text() {
    let quoted = ["p\"a", "ss\\w0rd"].concat();
    let accented = ["pässw", "ört-7€"].concat();
    let c = compile_with_store(&base(), &[StoredValue::new(quoted.clone()), StoredValue::new(accented.clone())]).unwrap();
    let r = "[REDACTED:act_lite_secret]";
    let args = serde_json::to_string(&json!({"cmd": format!("login {quoted} {accented}")})).unwrap();
    let mut b = json!({"input": [{"type": "function_call", "name": "sh", "call_id": "c", "arguments": args}]});
    let f = run(&mut b, &c).unwrap();
    assert_eq!(b["input"][0]["arguments"], json!(format!("{{\"cmd\":\"login {r} {r}\"}}")));
    assert_eq!(f.counts, BTreeMap::from([("act_lite_secret".to_string(), 2)]));
    let args = format!("{{\"cmd\": \"login {} {}\"}}", ascii_escape(&quoted), ascii_escape(&accented));
    let upper = ascii_escape(&accented).replace("\\u00e4", "\\u00E4").replace("\\u00f6", "\\u00F6").replace("\\u20ac", "\\u20AC");
    let mut b = json!({"input": [{"type": "function_call", "name": "sh", "call_id": "c", "arguments": args}, {"type": "message", "role": "user", "content": upper}]});
    run(&mut b, &c).unwrap();
    assert_eq!(b["input"][0]["arguments"], json!(format!("{{\"cmd\": \"login {r} {r}\"}}")));
    assert_eq!(b["input"][1]["content"], json!(r));
}

#[test]
fn generic_assignment_in_double_encoded_json() {
    let c = compile(&base()).unwrap();
    let inner = serde_json::to_string(&json!({"password": pw()})).unwrap();
    let dbl = serde_json::to_string(&json!({"log": inner})).unwrap();
    assert_eq!(red(&c, &dbl), format!("{{\"log\":\"{{\\\"password\\\":\\\"{G}\\\"}}\"}}"));
    assert!(!red(&c, &dbl).contains(&pw()));
}

#[test]
fn generic_assignment_needs_the_key_word_at_the_end_of_the_name() {
    let c = compile(&base()).unwrap();
    for s in ["TOKENIZER_PATH = \"/models/tokenizer.json\"", "tokenizer_name = \"bert-base-uncased\"",
              "secretName: \"my-app-db-credentials\"", "password_field_id = \"login-password-input\"",
              "const csrfTokenHeader = 'X-CSRF-Token-Header';", "MAX_TOKENS=4096000000000",
              "SECRET_KEY_FILE=/etc/app/secret.key"] {
        let mut b = tool(s);
        let f = run(&mut b, &c).unwrap();
        assert_eq!(text(&b), s);
        assert!(f.counts.is_empty() && !f.changed, "{s}");
    }
    for (s, want) in [
        ("password = \"correct horse battery staple\"".to_string(), format!("password = \"{G}\"")),
        (format!("api_key: {}", pw()), format!("api_key: {G}")),
        (format!("db:\n  Password: {}\n  user: x", pw()), format!("db:\n  Password: {G}\n  user: x")),
        (format!("const API_TOKEN = '{}';", pw()), format!("const API_TOKEN = '{G}';")),
        (format!("Db_Passwd={}", pw()), format!("Db_Passwd={G}")),
    ] {
        assert_eq!(red(&c, &s), want);
    }
    assert_eq!(red(&c, "password = \"two\nlines-of-text\""), "password = \"two\nlines-of-text\""); // no newline in a value
}

#[test]
fn second_pass_over_redacted_output_changes_nothing() {
    let c = compile_with_store(&base(), &[StoredValue::new("REDACTED:generic_secret")]).unwrap();
    let d = "-".repeat(5);
    let body = tool(&format!("password = \"{}\"\nAPI_KEY={}\n{} {d}BEGIN RSA PRIVATE KEY{d}\nMIIB", pw(), pw(), key()));
    let mut once = body.clone();
    assert!(run(&mut once, &c).unwrap().changed);
    let mut twice = once.clone();
    let f = run(&mut twice, &c).unwrap();
    assert_eq!(serde_json::to_vec(&twice).unwrap(), serde_json::to_vec(&once).unwrap());
    assert!(f.counts.is_empty() && !f.changed, "{:?}", f.counts);
    // a placeholder id that matches a user pattern
    let c = compile(&cfg(vec![pat("tracer_id", "api_key|secret_assignment", Action::Redact)])).unwrap();
    let mut b = tool(&format!("{R} and {G}"));
    let f = run(&mut b, &c).unwrap();
    assert_eq!(text(&b), format!("{R} and {G}"));
    assert!(f.counts.is_empty());
}

#[test]
fn changed_is_false_when_no_string_changed() {
    let c = compile(&base()).unwrap();
    let mut b = tool("nothing secret here");
    assert!(!run(&mut b, &c).unwrap().changed);
    let mut b = tool(&key());
    assert!(run(&mut b, &c).unwrap().changed);
}

#[test]
fn key_order_survives_a_pass_that_changed_another_string() {
    let c = compile(&base()).unwrap();
    let thinking = r#"{"type":"thinking","thinking":"plan","signature":"c2ln"}"#;
    let raw = format!(r#"{{"model":"m","messages":[{{"role":"assistant","content":[{thinking},{{"type":"text","text":"{}"}}]}}]}}"#, key());
    let mut b: Value = serde_json::from_str(&raw).unwrap();
    assert!(run(&mut b, &c).unwrap().changed);
    let out = serde_json::to_string(&b).unwrap();
    assert_eq!(out, raw.replace(&key(), R));
}

#[test]
fn pgp_private_key_block_is_redacted() {
    let c = compile(&base()).unwrap();
    let d = "-".repeat(5);
    let block = format!("{d}BEGIN PGP PRIVATE KEY BLOCK{d}\n\nlQOYBGXyz\n=abcd\n{d}END PGP PRIVATE KEY BLOCK{d}");
    assert_eq!(red(&c, &format!("x {block} y")), "x [REDACTED:pgp_private_key_block] y");
    assert_eq!(red(&c, &format!("x {d}BEGIN PGP PRIVATE KEY BLOCK{d}\n\nlQOYBGXyz")), "x [REDACTED:pgp_private_key_block]");
    let public = format!("{d}BEGIN PGP PUBLIC KEY BLOCK{d}\nmQINBGX\n{d}END PGP PUBLIC KEY BLOCK{d}");
    assert_eq!(red(&c, &public), public);
}

#[test]
fn eviction_between_passes_does_not_cause_an_unbudgeted_scan() {
    let c = compile(&base()).unwrap();
    let p = CachePool::new(ENTRY_OVERHEAD, SESSION_CAP_BYTES); // room for one entry with no finding
    let mut cache = SessionCache::new(p);
    redact(&mut json!(["one"]), &c, &mut cache, &DeadlinePolicy::default()).unwrap();
    assert_eq!(cache.scan_count(), 1);
    // "two" is scanned first and its insert evicts "one"; "one" was counted cached, so it must not be scanned
    let hook = DeadlinePolicy { expire_after_scans: Some(1), ..DeadlinePolicy::default() };
    assert!(redact(&mut json!(["two", "one"]), &c, &mut cache, &hook).is_ok());
    assert_eq!(cache.scan_count(), 2);
}

#[test]
fn cached_bytes_are_not_scanned_again_under_a_small_budget() {
    let c = compile(&base()).unwrap();
    let big = format!("{} {}", "QUJD eyJh AIza tokenizer ".repeat(1 << 16), key());
    let mut cache = SessionCache::new(pool());
    redact(&mut json!([big.clone()]), &c, &mut cache, &DeadlinePolicy::default()).unwrap();
    // one uncached scan allowed: it goes to "small" only if `big` is served from the cache
    let tight = DeadlinePolicy { base: Duration::from_secs(60), per_mib: Duration::ZERO, expire_after_scans: Some(1) };
    let mut b = json!([big.clone(), "small"]);
    let n0 = scan_calls();
    let f = redact(&mut b, &c, &mut cache, &tight).unwrap();
    // The scan function itself ran once ("small"): `big` came from the cache, not a silent rescan.
    assert_eq!(scan_calls() - n0, 1);
    assert_eq!(cache.scan_count(), 2);
    assert_eq!(f.counts, BTreeMap::from([("anthropic_api_key".to_string(), 1)]));
}

// ---- round 3 (N68, 2026-11-19) ----

#[test] // §16.1 Scope note: only a placeholder with a configured id is skipped
fn round_3_placeholder_with_an_unconfigured_id_is_scanned() {
    let c = compile(&base()).unwrap();
    let aws = ["AK", "IA", "ABCDEFGHIJKLMNOP"].concat();
    let gh = ["gh", "p_", &"a1B2".repeat(9)].concat();
    assert_eq!(red(&c, &format!("key [REDACTED:{aws}] end")), "key [REDACTED:[REDACTED:aws_access_key_id]] end");
    assert_eq!(red(&c, &format!("key [REDACTED:{gh}] end")), "key [REDACTED:[REDACTED:github_token]] end");
    assert_eq!(red(&c, "password=\"[REDACTED:x]abcdefghijklmnopq\""), format!("password=\"{G}\""));
}

#[test] // §16.1 Scope note: patterns run on the whole string, so a placeholder does not split a secret
fn round_3_placeholder_inside_a_secret_does_not_split_it() {
    let c = compile(&base()).unwrap();
    let d = "-".repeat(5);
    let mut b = tool(&format!("x {d}BEGIN RSA PRIVATE KEY{d}\nMIIB{R}OgIBAAJB\n{d}END RSA PRIVATE KEY{d} y"));
    let f = run(&mut b, &c).unwrap();
    assert_eq!(text(&b), "x [REDACTED:private_key_pem] y");
    assert_eq!(f.counts, BTreeMap::from([("private_key_pem".to_string(), 1)]));
    assert_eq!(red(&c, &format!("password=\"{R}abcdefghijklmnopq\"")), format!("password=\"{G}\""));
}

#[test] // D7: deterministic but not idempotent; a second pass may widen a redaction
fn round_3_second_pass_may_widen_a_redaction() {
    let v = ["hun", "ter22"].concat();
    let c = compile_with_store(&base(), &[StoredValue::new(v.clone())]).unwrap();
    let once = red(&c, &format!("password = \"ab {v}\""));
    assert_eq!(once, "password = \"ab [REDACTED:act_lite_secret]\"");
    let mut b = tool(&once);
    let f = run(&mut b, &c).unwrap();
    assert_eq!(text(&b), format!("password = \"{G}\""));
    assert_eq!(f.counts, BTreeMap::from([("generic_secret_assignment".to_string(), 1)]));
}

#[test] // D2: a block match across a placeholder or inside an unconfigured id is refused; wholly inside a configured one is dropped
fn round_3_block_patterns_and_placeholders() {
    let c = compile(&cfg(vec![pat("tracer_block", r"blockme\S*?[0-9]{3}", Action::Block),
                              pat("tracer_inside", "anthropic_api", Action::Block)])).unwrap();
    let go = |t: &str| run(&mut tool(t), &c).map(|f| f.counts);
    let refused = Err(RedactError::Blocked { ids: vec!["tracer_block".to_string()] });
    assert_eq!(go(&format!("a blockme{R}123 b")), refused); // across a placeholder: refused (tracer_inside, inside R, dropped)
    assert_eq!(go("a [REDACTED:blockme123] b"), refused); // inside an unconfigured id: refused
    assert_eq!(go(&format!("a {R} b")), Ok(BTreeMap::new())); // wholly inside a configured placeholder: dropped
}

#[test] // D6: the search resumes inside the placeholder, so trailing context cannot hide a later match
fn round_3_dropped_match_does_not_hide_a_later_match() {
    let c = compile(&cfg(vec![pat("zqtail", r"(?P<secret>zq[a-z]+)\S*", Action::Redact)])).unwrap();
    let hidden = ["zq", "hidden"].concat();
    // The first match starts at `zqtail` inside the placeholder (dropped); its `\S*` runs over `]zqhidden`.
    assert_eq!(red(&c, &format!("[REDACTED:zqtail]{hidden}")), "[REDACTED:zqtail][REDACTED:zqtail]");
}

#[test] // §16.1 Bound note: the deadline starts after the cache lookup (the seam stands in for hashing a large body)
fn round_3_cached_conversation_plus_new_strings_passes() {
    let c = compile(&base()).unwrap();
    let big = "QUJD".repeat(1 << 18); // 1 MiB, cached below
    let mut cache = SessionCache::new(pool());
    redact(&mut json!([big.clone()]), &c, &mut cache, &DeadlinePolicy::default()).unwrap();
    let p = DeadlinePolicy { base: Duration::from_millis(500), per_mib: Duration::ZERO, expire_after_scans: None };
    set_lookup_delay(Duration::from_secs(1));
    let n0 = scan_calls();
    let r = redact(&mut json!([big, "new one", "new two"]), &c, &mut cache, &p);
    set_lookup_delay(Duration::ZERO);
    assert!(r.is_ok(), "{r:?}");
    assert_eq!(scan_calls() - n0, 2);
    assert_eq!(cache.scan_count(), 3);
}

#[test] // §16.1 Bound note: every attempt scans at least one uncached string, so a retry makes progress
fn round_3_every_attempt_scans_one_uncached_string() {
    let c = compile(&base()).unwrap();
    let zero = DeadlinePolicy { base: Duration::ZERO, per_mib: Duration::ZERO, expire_after_scans: None };
    let mut cache = SessionCache::new(pool());
    for n in 1..=2 {
        assert_eq!(redact(&mut json!(["one", "two", "three"]), &c, &mut cache, &zero), Err(RedactError::Timeout));
        assert_eq!(cache.scan_count(), n);
    }
    assert!(redact(&mut json!(["one", "two", "three"]), &c, &mut cache, &zero).is_ok());
    assert_eq!(cache.scan_count(), 3);
    let hook0 = DeadlinePolicy { expire_after_scans: Some(0), ..DeadlinePolicy::default() };
    assert_eq!(redact(&mut json!(["four", "five"]), &c, &mut cache, &hook0), Err(RedactError::Timeout));
    assert_eq!(cache.scan_count(), 4); // D3
}

#[test] // §16.1 Bound note: the lock wait comes off the whole budget (base plus per-MiB), never past it
fn round_3_lock_wait_reduces_the_whole_budget() {
    let c = compile(&base()).unwrap();
    // 6 uncached bytes: 0 + 2^30 s * 6 / 2^20 = 6144 s exactly
    let p = DeadlinePolicy { base: Duration::ZERO, per_mib: Duration::from_secs(1 << 30), expire_after_scans: None };
    let go = |w: u64| {
        let mut cache = SessionCache::new(pool());
        let r = redact_after_wait(&mut json!(["one", "two"]), &c, &mut cache, &p, Duration::from_secs(w));
        (r.is_ok(), cache.scan_count())
    };
    assert_eq!(go(6000), (true, 2)); // 144 s left
    assert_eq!(go(6144), (false, 1)); // the whole budget used: one scan, then timeout
    assert_eq!(go(1 << 40), (false, 1)); // saturates at zero, never past it
}

// ---- round 4 (N68 code review R1–R3, 2026-11-19) ----

#[test] // R1: a configured placeholder inside a token-shaped secret does not split it; one at a span edge stays outside
fn round_4_placeholder_inside_a_token_does_not_split_it() {
    let c = compile(&base()).unwrap();
    let aws = ["AK", "IA", "ABCDEFGHIJKLMNOP"].concat();
    let gh = ["gh", "p_", &"a1B2".repeat(9)].concat();
    let ant = ["sk-", "ant-api03-", &"Ab3_".repeat(10)].concat();
    let ph = "[REDACTED:jwt]";
    for (k, id) in [(&aws, "aws_access_key_id"), (&gh, "github_token"), (&ant, "anthropic_api_key")] {
        let (x, y) = k.split_at(k.len() / 2);
        for t in [format!("k {x}{ph}{y} e"), format!("k {x}{ph}{ph}{y} e")] {
            let mut b = tool(&t);
            let f = run(&mut b, &c).unwrap();
            assert_eq!(text(&b), format!("k [REDACTED:{id}] e"), "{id}");
            assert_eq!(f.counts, BTreeMap::from([(id.to_string(), 1)]), "{id}");
        }
    }
    assert_eq!(red(&c, &format!("k {ph}{aws}{ph} e")), format!("k {ph}[REDACTED:aws_access_key_id]{ph} e"));
    let v = ["hun", "ter22"].concat();
    let cs = compile_with_store(&base(), &[StoredValue::new(v.clone())]).unwrap();
    let (x, y) = v.split_at(4);
    for p in [ph, "[REDACTED:act_lite_secret]"] {
        assert_eq!(red(&cs, &format!("v {x}{p}{y} e")), "v [REDACTED:act_lite_secret] e");
    }
}

#[test] // R2: a block pattern's drop is decided on the whole match, not its `secret*` group
fn round_4_block_pattern_drop_uses_the_whole_match() {
    let c = compile(&cfg(vec![pat("blk_g", r"(?P<secret>[a-z]+)\]\s+forbidden", Action::Block)])).unwrap();
    let refused = Err(RedactError::Blocked { ids: vec!["blk_g".to_string()] });
    assert_eq!(run(&mut tool("[REDACTED:jwt] forbidden"), &c).map(|f| f.counts), refused);
    assert_eq!(run(&mut tool("[REDACTED:blk_g] forbidden"), &c).map(|f| f.counts), refused);
}

#[test] // R3: every participating `secret*` group is redacted; empty groups and non-ASCII text are safe
fn round_4_every_secret_group_is_redacted() {
    let c = compile(&cfg(vec![
        pat("two_groups", r"(?P<secret>u[a-z]{5}):(?P<secret2>p[a-z]{5})@", Action::Redact),
        pat("emptyg", r"(?P<secret>)é", Action::Redact),
    ])).unwrap();
    let mut b = tool("x uabcde:pqrstu@host");
    let f = run(&mut b, &c).unwrap();
    assert_eq!(text(&b), "x [REDACTED:two_groups]:[REDACTED:two_groups]@host");
    assert_eq!(f.counts, BTreeMap::from([("two_groups".to_string(), 2)]));
    let uni = "é[REDACTED:jwt]é[REDACTED:emptyg]é";
    let mut b = tool(uni);
    assert_eq!(run(&mut b, &c).map(|f| f.counts), Ok(BTreeMap::new()));
    assert_eq!(text(&b), uni);
    let c2 = compile(&cfg(vec![pat("clé", r"é+", Action::Redact)])).unwrap();
    assert_eq!(red(&c2, "aé [REDACTED:clé] éé"), "a[REDACTED:clé] [REDACTED:clé] [REDACTED:clé]");
}
