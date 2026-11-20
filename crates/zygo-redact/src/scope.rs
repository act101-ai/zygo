//! Secret-pass exclusions by JSON path at the body root, never by a `type` value elsewhere (§16.1).

use serde_json::Value;
use std::collections::HashSet;

fn is_base64_media(v: &Value) -> bool {
    matches!(v["type"].as_str(), Some("image" | "document")) && v["source"]["type"] == "base64"
}

fn is_data_url(v: &Value) -> bool {
    v.as_str().is_some_and(|s| s.starts_with("data:"))
}

/// RFC 6901 pointers of the subtrees the secret pass skips.
pub fn excluded(body: &Value) -> HashSet<String> {
    let mut ex = HashSet::new();
    for (i, m) in body["messages"].as_array().into_iter().flatten().enumerate() {
        for (j, b) in m["content"].as_array().into_iter().flatten().enumerate() {
            let p = format!("/messages/{i}/content/{j}");
            match b["type"].as_str() {
                Some("thinking") => {
                    ex.insert(format!("{p}/thinking"));
                    ex.insert(format!("{p}/signature"));
                }
                Some("redacted_thinking") => {
                    ex.insert(format!("{p}/data"));
                }
                Some("image" | "document") if is_base64_media(b) => {
                    ex.insert(format!("{p}/source/data"));
                }
                Some("tool_result") => {
                    for (k, c) in b["content"].as_array().into_iter().flatten().enumerate() {
                        if is_base64_media(c) {
                            ex.insert(format!("{p}/content/{k}/source/data"));
                        }
                    }
                }
                Some("image_url") if is_data_url(&b["image_url"]["url"]) => {
                    ex.insert(format!("{p}/image_url/url"));
                }
                _ => {}
            }
        }
    }
    for (i, item) in body["input"].as_array().into_iter().flatten().enumerate() {
        let p = format!("/input/{i}");
        if item["type"] == "reasoning" {
            ex.insert(p);
            continue;
        }
        for (j, c) in item["content"].as_array().into_iter().flatten().enumerate() {
            match c["type"].as_str() {
                Some("input_image") if is_data_url(&c["image_url"]) => {
                    ex.insert(format!("{p}/content/{j}/image_url"));
                }
                Some("input_file") => {
                    ex.insert(format!("{p}/content/{j}/file_data"));
                }
                _ => {}
            }
        }
    }
    ex
}

fn push_key(ptr: &mut String, k: &str) {
    ptr.push('/');
    ptr.push_str(&k.replace('~', "~0").replace('/', "~1"));
}

fn push_index(ptr: &mut String, i: usize) {
    ptr.push('/');
    ptr.push_str(&i.to_string());
}

/// Calls `f` on every string value outside `ex`, in serde_json `Map` order; keys are never passed.
pub fn for_each_str(v: &Value, ex: &HashSet<String>, f: &mut dyn FnMut(&str)) {
    fn walk(v: &Value, ptr: &mut String, ex: &HashSet<String>, f: &mut dyn FnMut(&str)) {
        if ex.contains(ptr.as_str()) {
            return;
        }
        let len = ptr.len();
        match v {
            Value::String(s) => f(s),
            Value::Array(a) => {
                for (i, x) in a.iter().enumerate() {
                    push_index(ptr, i);
                    walk(x, ptr, ex, f);
                    ptr.truncate(len);
                }
            }
            Value::Object(o) => {
                for (k, x) in o {
                    push_key(ptr, k);
                    walk(x, ptr, ex, f);
                    ptr.truncate(len);
                }
            }
            _ => {}
        }
    }
    walk(v, &mut String::new(), ex, f)
}

/// `for_each_str` with mutable strings, in the same order.
pub fn for_each_str_mut(v: &mut Value, ex: &HashSet<String>, f: &mut dyn FnMut(&mut String)) {
    fn walk(v: &mut Value, ptr: &mut String, ex: &HashSet<String>, f: &mut dyn FnMut(&mut String)) {
        if ex.contains(ptr.as_str()) {
            return;
        }
        let len = ptr.len();
        match v {
            Value::String(s) => f(s),
            Value::Array(a) => {
                for (i, x) in a.iter_mut().enumerate() {
                    push_index(ptr, i);
                    walk(x, ptr, ex, f);
                    ptr.truncate(len);
                }
            }
            Value::Object(o) => {
                for (k, x) in o.iter_mut() {
                    push_key(ptr, k);
                    walk(x, ptr, ex, f);
                    ptr.truncate(len);
                }
            }
            _ => {}
        }
    }
    walk(v, &mut String::new(), ex, f)
}
