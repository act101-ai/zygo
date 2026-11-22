# HUD-5 — Proxy status: model, context_window, threshold (AU28, AU29); SessionStatus Default; BYTES_PER_TOKEN

**Spec:** `docs/specs/2026-11-21-statusline-hud-design.md` H§7.2 (rows `effective_window`, `model`, `context_window`, `threshold`), AU28, AU29, OP1, H§17 notes 1 and 5, H§9 rows §7.2, §10.2.2, §10.2.4 (this node builds the behavior; DOC-1 writes their base-spec text). Runs on today's code, before the workspaces graph: `SessionStatus` gains `#[derive(Default)]` and every test literal it breaks ends in `..Default::default`, so later fields (HUD-6, workspaces X1) change only `status` in `session.rs`
**Files:** `crates/zygo-core/src/tokens.rs`, `crates/zygo-core/src/lib.rs`, `crates/zygo-proxy/src/client.rs`, `crates/zygo-proxy/src/session.rs`, `crates/zygo-proxy/src/forward.rs`, `crates/zygo-proxy/src/window.rs`, `crates/zygo-proxy/tests/session_api.rs`, `crates/zygo-proxy/tests/window.rs`, `crates/zygo-proxy/tests/serve.rs`, `crates/zygo-proxy/tests/passthrough.rs`, `crates/zygo/tests/compact_cmd.rs`, `crates/zygo-hooks/src/statusline.rs`, `docs/specs/2026-11-21-statusline-hud-design.md`
**Consumes:** —
**Produces:** `tokens::BYTES_PER_TOKEN: u64`, `SessionStatus::default() -> SessionStatus`, `SessionStatus.model: Option<String>`, `SessionStatus.context_window: Option<u64>`, `SessionStatus.threshold: Option<u64>`, `window::soft_threshold(compact: Option<WindowArg>, row: &ModelRow, safety: Option<u64>) -> Option<u64>`
**Acceptance:** `cargo test -p zygo-proxy --test session_api --test window && cargo test -p zygo-hooks --lib && cargo test --workspace --locked --no-run && ! grep -rqE 'div_ceil\(4\)' crates/zygo-proxy/src` → all pass and the workspace builds, including H§14 #19, the existing AU29 assertions unchanged plus `model`/`context_window` assertions, a new test that builds `SessionStatus { model: .., ..Default::default() }`, and new tests for `[1m]`, another role's model, a non-streamed request and an OpenCode request

## Plan

Premises checked 2026-11-21 against the live code (HUD-1 merged, nothing else of this graph); the line numbers below are those before this node's edits. `SessionStatus` has the 8 fields that H§7.2 lists as "as today" plus `effective_window`, and no `Default` (`crates/zygo-proxy/src/client.rs:39-52`). Its literals: the producer `status()` (`crates/zygo-proxy/src/session.rs:267-277`) and one test literal that names every field, `fn s()` (`crates/zygo-hooks/src/statusline.rs:175-186`); the other four (`statusline.rs:221`, `:223`, `:229`, `:243`) already end in `..s()`. `status()` computes `effective_window` from the main role's model in the session's config snapshot (`self.cfg`) with `agent_window(row)` (`session.rs:259-264`), in passthrough too. `soft_threshold` is private (`crates/zygo-proxy/src/window.rs:76-83`) and reads only `row.soft_context_window`; `effective_for` (`window.rs:87-90`) reads only `context_window(row)` (`facts.context_window` when > 0) and `soft_threshold`; `ModelRow` and `Facts` derive `Default` (`crates/zygo-config/src/schema.rs:100-116`). The stage computes the agent window in `WindowStage::on_response_line` (`window.rs:133`) with `one_m` (`:42-50`). `Pending` (`crates/zygo-proxy/src/forward.rs:198-206`) is built in `handle`; `run` sets `Pending.row.model` to `Resolved.model` (`forward.rs:334`); `Pending::drop` sets `used_context` under route `/anthropic/v1/messages`, 2xx, class `main` or absent, prompt > 0 (`forward.rs:220-227`), and it runs for a body that ends early too (the `Flow` owns the `Pending`, `forward.rs:523`). Resolution is complete at `let resolved = ctx.resolved.clone().expect("stage 2 resolved the request");` in `run` (`forward.rs:340`; `Resolved.one_m` at `crates/zygo-proxy/src/resolve.rs:16`; passthrough fills it, `crates/zygo-proxy/src/passthrough.rs:229-236`). An unmatched passthrough name becomes `Resolved.model` as the requested name after the secret pass with `[1m]` removed, and `""` when that pass blocks or panics (`requested_name`, `row_name`, `passthrough_names`, `forward.rs:684-716`); with `[features.redaction] secrets = false` such a request is still served (`requested_name` falls to `row_name`). `ctx.cfg` is the serving config at request time (`forward.rs:296`, `app.served().cfg`); a serve-mode reload swaps it (`crates/zygo-proxy/src/server.rs:279-281`) while a session keeps its creation snapshot (`session.rs:444`). Every reply, count_tokens and non-2xx ones included, goes through the same tee, and a non-streamed body's `usage` is read whatever the status (`forward.rs:496`, `crates/zygo-proxy/src/tee.rs:106-138`); a 4xx is served, not retried (`forward.rs:402-406`). `div_ceil(4)` sites in `crates/zygo-proxy/src`: exactly two, both in `run` (`forward.rs:350`, the local count_tokens estimate, and `forward.rs:446`, `let estimate`), both on `string_bytes(..) -> u64`. `zygo-core` has no `tokens` module (`crates/zygo-core/src/lib.rs:6-15`); `zygo-proxy` already depends on `zygo-core`. AU29 test sites: `crates/zygo-proxy/tests/session_api.rs:566`, `:570`, `:572`; `crates/zygo-proxy/tests/window.rs:1043-1078` (the last test of the file); `crates/zygo/tests/compact_cmd.rs:173`, `:176`. A test calls `set_used_context` (`session_api.rs:592`), so it stays. The status route serializes `s.status()` (`crates/zygo-proxy/src/routes.rs:231-235`), so `routes.rs` needs no edit. Fixture values: session_api and compact_cmd main role `lead` = `claude-opus-5-5`, window 1,000,000, soft 800,000 (`session_api.rs:44-47`, `compact_cmd.rs:44-46`; `crates/zygo-config/data/catalog.toml:30-36`); window.rs rows `claude-t-1` (soft 150k, window 200k), `claude-ns` (200k), `claude-big` (soft 900k, 2M), `gpt-t-1` (400k, Anthropic dialect at `/g`) (`window.rs:194-206`); serve.rs row `test-code` (soft 150,000, window 200,000; role `code`) and an upstream that answers every request with usage input 1 (`crates/zygo-proxy/tests/serve.rs:38-52`, `:198-235`); a top-level `main_role` is a user-config key (`crates/zygo-config/tests/config_layers.rs:172`); passthrough.rs: `claude-test-1` matches row `test-pt` (no facts), the block pattern `TRACERBLOCK_[0-9]{6}`, an upstream that answers usage input 11 (`crates/zygo-proxy/tests/passthrough.rs:106-120`, `:60-70`). The window.rs fake upstream sends its scripted body in chunks and then ends (`window.rs:111-130`). The count estimate test `count_tokens_estimated_locally` asserts 6 for 22 bytes (`crates/zygo-proxy/tests/request_path.rs:1340-1351`). Toolchain 1.98 (`rust-toolchain.toml`), so `str::floor_char_boundary` is stable. The draft OpenCode HUD design changes no proxy or status field (`docs/specs/2026-11-21-opencode-hud-design.md:113`).

Design decisions (author's calls, HUD-5 review; step 17 writes them into the spec): at request time the proxy takes from the serving config (`ctx.cfg`) only what `status()` needs from the served row, `soft_context_window` and `facts.context_window`, plus the request's agent window (`window::RequestWindows`), so the values stay those of the served row after a serve-mode reload. `soft_threshold` and `effective_for` keep their `&ModelRow` signatures; `RequestWindows::view` builds a `ModelRow` that holds only those two values (`..Default::default()` for the rest), which is all either function reads. The model name comes from `Pending.row.model`. It is cut to at most 128 bytes on a character boundary (`MODEL_NAME_MAX`), so that a passthrough client cannot put a long string into every status reply; an empty name (the secret pass blocked or failed) stores no model. Before the first main-loop request, also after a serve-mode reload, the status keeps today's rule: the main role's model and row from the session snapshot (AU29).

- [x] **1. Write the failing session API tests.** In `crates/zygo-proxy/tests/session_api.rs`, change the import at line 17 to `use zygo_proxy::client::{ClientError, SessionClient, SessionStatus};`. In `client_compact_and_status`, keep every line and add one assertion after each of lines 566, 570 and 572:

  ```rust
      assert_eq!(s.effective_window, Some(600_000));
      assert_eq!((s.model.as_deref(), s.context_window, s.threshold), (Some("claude-opus-5-5"), Some(1_000_000), Some(600_000)));
      // …
      assert_eq!(s.effective_window, Some(800_000));
      assert_eq!((s.model.as_deref(), s.context_window, s.threshold), (Some("claude-opus-5-5"), Some(1_000_000), Some(800_000)));
      // …
      assert_eq!(k.status().unwrap().effective_window, None);
      assert_eq!(k.status().unwrap().threshold, None); // AU28: override `off`
  ```

  Append:

  ```rust
  #[test]
  fn status_default_keeps_literals_short() { // Review focus 11, H§17 note 1; H§7.2 `#[serde(default)]`
      let s = SessionStatus { model: Some("claude-opus-5-5".into()), ..Default::default() };
      assert_eq!(s.model.as_deref(), Some("claude-opus-5-5"));
      assert_eq!((s.session.as_str(), s.context_window, s.threshold, s.effective_window), ("", None, None, None));
      // A reply without the new keys parses; each new field is None.
      let old: SessionStatus = serde_json::from_value(json!({"session":"S1","profile":null,"work":null,"used_context":null,
          "effective_window":null,"compact":null,"cache_read_share":null,"expires_at":""})).unwrap();
      assert_eq!(old, SessionStatus { session: "S1".into(), ..Default::default() });
  }

  #[test]
  fn status_reply_omits_absent_new_fields() { // H§7.2 `model`: in passthrough, absent until a request is served
      let d = TempDir::new().unwrap();
      let (h, _c, _, _) = start(cfg(&d));
      let raw = Raw::new();
      let mut r = req();
      r.passthrough = true;
      let a = h.create_session(r).unwrap();
      let (s, v, _) = raw.call(&h.url(), "GET", &sp(&a), &sess(&a), None);
      assert_eq!(s, 200);
      for k in ["model", "context_window", "threshold"] {
          assert!(v.get(k).is_none(), "{k} in {v}");
      }
      assert!(v.get("effective_window").is_some(), "an old field keeps its key: {v}");
      assert_eq!(client(&h, &a, &d).status().unwrap().model, None); // parsed with `#[serde(default)]`
      let b = h.create_session(req()).unwrap();
      let (_, v, _) = raw.call(&h.url(), "GET", &sp(&b), &sess(&b), None);
      assert_eq!((v["model"].clone(), v["context_window"].clone()), (json!("claude-opus-5-5"), json!(1000000)));
  }
  ```

- [x] **2. Extend the window.rs harness.** In `crates/zygo-proxy/tests/window.rs`: add `soft_threshold` to the `zygo_proxy::window::{…}` import (line 28) and `WindowArg` to the `zygo_proxy::session::{…}` import (line 25). `Reply` (line 57) gains `/// After the body, a `ping` event every 50 ms, never ending.` `pings: bool`; `sse_reply` and `json_reply` set `pings: false`. In `upstream_handler`, replace the `else` branch's `Body::from_stream(futures_util::stream::iter(parts))` with:

  ```rust
        let parts = futures_util::stream::iter(parts);
        if r.pings {
            Body::from_stream(futures_util::StreamExt::chain(parts, futures_util::stream::unfold((), |()| async {
                tokio::time::sleep(Duration::from_millis(50)).await;
                Some((Ok::<_, std::io::Error>(Bytes::from_static(b"event: ping\ndata: {\"type\":\"ping\"}\n\n")), ()))
            })))
        } else {
            Body::from_stream(parts)
        }
  ```

  Add these helpers above `status_used_and_effective`:

  ```rust
  /// Once every row is written: (model, context_window, effective_window, threshold) of the status (AU29).
  fn windows(t: &T, g: &SessionGrant) -> (Option<String>, Option<u64>, Option<u64>, Option<u64>) {
      rows(t, g, t.n.get());
      let s = t.h.session(&g.session).unwrap().status();
      (s.model, s.context_window, s.effective_window, s.threshold)
  }

  /// Posts `role` with `extra` headers and closes the connection once the body holds `needle`.
  fn read_then_drop(t: &T, g: &SessionGrant, role: &str, extra: &[(&str, &str)], needle: &str) {
      let mut hd = auth(g);
      hd.extend_from_slice(extra);
      t.n.set(t.n.get() + 1);
      let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
      rt.block_on(async {
          let mut rb = http_client().post(t.h.url().join(M).unwrap()).header("content-type", "application/json");
          for (k, v) in &hd {
              rb = rb.header(*k, *v);
          }
          let mut r = rb.body(req_body(role).to_string()).send().await.unwrap();
          assert_eq!(r.status().as_u16(), 200);
          let mut seen = Vec::new();
          while !String::from_utf8_lossy(&seen).contains(needle) {
              seen.extend_from_slice(&r.chunk().await.unwrap().expect("the body ended before the needle"));
          }
      }); // the response, its client and the runtime drop here: the connection closes
  }
  ```

  Check: `cargo test -p zygo-proxy --test window --no-run` → exactly these diagnostics: `error[E0603]: function `soft_threshold` is private` at the import (window.rs:28); three `error[E0609]: no field `model` / `context_window` / `threshold` on type `SessionStatus`` at window.rs:1056 (in `windows`); `warning: unused import: `WindowArg`` (window.rs:25). None points into `Reply`, `upstream_handler` or `read_then_drop`.

- [x] **3. Add the model and window assertions to the existing window test.** In `status_used_and_effective`, keep every line; after the first `assert_eq!(st(&t, &g), (None, Some(150000)));` add `assert_eq!(windows(&t, &g), (Some("claude-t-1".into()), Some(200000), Some(150000), Some(150000)));`, and after the first `assert_eq!(st(&t, &g).1, Some(360000));` (the `gpt-t-1` part) add `assert_eq!(windows(&t, &g), (Some("gpt-t-1".into()), Some(400000), Some(360000), None));`.

- [x] **4. Write the failing window tests.** Append to `crates/zygo-proxy/tests/window.rs`:

  ```rust
  #[test]
  fn unit_soft_threshold_public() { // H§7.2 `threshold`
      let (t1, ns) = (row("claude-t-1"), row("claude-ns"));
      assert_eq!(soft_threshold(None, &t1, None), Some(150_000));
      assert_eq!(soft_threshold(None, &ns, None), None);
      assert_eq!(soft_threshold(Some(WindowArg::Tokens(190_000)), &ns, Some(180_000)), Some(180_000));
      assert_eq!(soft_threshold(Some(WindowArg::Off), &t1, Some(180_000)), None);
  }

  #[test]
  fn status_follows_one_m_request() { // AU29 `[1m]` name; AU28 example: 190k clamped by safety 180k → 180k
      let t = setup("claude-t-1");
      let g = grant(&t, Agent::ClaudeCode);
      let small = reply(S0(), &TEXT(), "end_turn", u4(1, 0, 5000, 1));
      run(&t, &g, "/ns", &small, "plan[1m]", &[MAIN]);
      assert_eq!(windows(&t, &g), (Some("claude-ns".into()), Some(200000), Some(180000), None));
      compact(&t, &g, json!(190000));
      assert_eq!(windows(&t, &g), (Some("claude-ns".into()), Some(200000), Some(180000), Some(180000)));
      run(&t, &g, "/ns", &small, "plan", &[MAIN]);
      assert_eq!(windows(&t, &g), (Some("claude-ns".into()), Some(200000), Some(190000), Some(190000)));
  }

  #[test]
  fn status_follows_one_m_beta() { // AU29: the `context-1m-` beta form of `[1m]` (Q7 capture)
      let t = setup("claude-t-1");
      let g = grant(&t, Agent::ClaudeCode);
      let fable = q7_beta("fable");
      run(&t, &g, "/ns", &reply(S0(), &TEXT(), "end_turn", u4(1, 0, 5000, 1)), "plan", &[MAIN, ("anthropic-beta", fable.as_str())]);
      assert_eq!(windows(&t, &g), (Some("claude-ns".into()), Some(200000), Some(180000), None));
  }

  #[test]
  fn status_follows_other_role() { // AU29: `/model` picks another role; a subagent request stores nothing
      let t = setup("claude-t-1");
      let g = grant(&t, Agent::ClaudeCode);
      run(&t, &g, "/b", &reply(S0(), &TEXT(), "end_turn", U15()), "triage", &[MAIN]);
      assert_eq!(windows(&t, &g), (Some("claude-big".into()), Some(2000000), Some(900000), Some(900000)));
      run(&t, &g, "/ns", &reply(S0(), &TEXT(), "end_turn", u4(1, 0, 5000, 1)), "plan", &[SUB]);
      assert_eq!(windows(&t, &g), (Some("claude-big".into()), Some(2000000), Some(900000), Some(900000)));
  }

  #[test]
  fn status_ignores_compaction_request() { // AU29: class `compaction` stores nothing
      let t = setup("claude-t-1");
      let g = grant(&t, Agent::ClaudeCode);
      run(&t, &g, "/b", &reply(S0(), &TEXT(), "end_turn", U15()), "triage", &[MAIN]);
      let before = windows(&t, &g);
      let compaction = [("x-claude-code-request-class", "compaction"), ("x-claude-code-compaction", "auto")];
      run(&t, &g, "/ns", &reply(S0(), &TEXT(), "end_turn", u4(1, 0, 5000, 1)), "plan", &compaction);
      assert_eq!(windows(&t, &g), before);
      assert_eq!(t.h.session(&g.session).unwrap().status().used_context, Some(151000));
  }

  #[test]
  fn status_ignores_non_2xx_reply() { // AU29: only a 2xx main-loop reply stores
      let t = setup("claude-t-1");
      let g = grant(&t, Agent::ClaudeCode);
      run(&t, &g, "/b", &reply(S0(), &TEXT(), "end_turn", U15()), "triage", &[MAIN]);
      let before = windows(&t, &g);
      // The 400 body carries usage, so the prompt is > 0 and only the 2xx test keeps it out.
      let mut r = json_reply(json!({"type":"error","error":{"type":"invalid_request_error","message":"bad"},"usage":{"input_tokens":5000}}));
      r.status = 400;
      t.up.script("/ns", vec![r]);
      let (s, _, b) = post(&t, &g, "plan", &[MAIN]);
      assert_eq!(s, 400, "{}", String::from_utf8_lossy(&b));
      assert_eq!(windows(&t, &g), before);
      let row = last(&t, &g);
      assert_eq!((row.status, tokens(&row).0), (400, 5000));
      assert_eq!(t.h.session(&g.session).unwrap().status().used_context, Some(151000));
  }

  #[test]
  fn status_ignores_count_tokens() { // AU29: requests on other routes store nothing
      let t = setup("claude-t-1");
      let g = grant(&t, Agent::ClaudeCode);
      run(&t, &g, "/b", &reply(S0(), &TEXT(), "end_turn", U15()), "triage", &[MAIN]);
      let before = windows(&t, &g);
      // The reply carries usage, so the prompt is > 0 and only the route keeps it out.
      t.up.script("/ns", vec![json_reply(json!({"input_tokens":5000,"usage":{"input_tokens":5000}}))]);
      let mut hd = auth(&g);
      hd.push(MAIN);
      t.n.set(t.n.get() + 1);
      let body = json!({"model":"plan","messages":[{"role":"user","content":"hi"}]}).to_string().into_bytes();
      let (s, _, b) = send(&t.h, "POST", "anthropic/v1/messages/count_tokens", &hd, Some(body));
      assert_eq!(s, 200, "{}", String::from_utf8_lossy(&b));
      assert_eq!(windows(&t, &g), before);
      let r = last(&t, &g);
      assert_eq!((r.route.as_str(), tokens(&r).0), ("/anthropic/v1/messages/count_tokens", 5000));
      assert_eq!(t.h.session(&g.session).unwrap().status().used_context, Some(151000));
  }

  #[test]
  fn status_follows_non_streamed_request() { // AU29: the condition does not test streaming
      let t = setup("claude-t-1");
      let g = grant(&t, Agent::ClaudeCode);
      let m = json!({"id":"msg_1","type":"message","role":"assistant","model":"gpt-t-1",
          "content":[{"type":"text","text":"ok"}],"stop_reason":"end_turn","stop_sequence":null,
          "usage":{"input_tokens":1,"cache_creation_input_tokens":0,"cache_read_input_tokens":5000,"output_tokens":5}});
      t.up.script("/g", vec![json_reply(m)]);
      let body = json!({"model":"code","max_tokens":64,"stream":false,"messages":[{"role":"user","content":"hi"}]});
      let (s, _, b) = post_body(&t, &g, &body, &[MAIN]);
      assert_eq!(s, 200, "{}", String::from_utf8_lossy(&b));
      assert_eq!(windows(&t, &g), (Some("gpt-t-1".into()), Some(400000), Some(360000), None));
      assert_eq!(t.h.session(&g.session).unwrap().status().used_context, Some(5001));
  }

  #[test]
  fn status_follows_dropped_stream() { // AU29: stored under the used_context condition, with no message_delta
      let t = setup("claude-t-1");
      let g = grant(&t, Agent::ClaudeCode);
      let start = json!({"message":{"id":"msg_1","type":"message","role":"assistant","content":[],"usage":u4(1, 0, 5000, 1)}});
      let mut r = sse_reply(&sse(&[("message_start", start)]));
      r.pings = true;
      t.up.script("/ns", vec![r]);
      read_then_drop(&t, &g, "plan", &[MAIN], "message_start");
      assert_eq!(windows(&t, &g), (Some("claude-ns".into()), Some(200000), None, None));
      assert_eq!(t.h.session(&g.session).unwrap().status().used_context, Some(5001));
      assert_eq!(last(&t, &g).status, 200);
  }

  #[test]
  fn status_follows_opencode_request() { // AU29: OpenCode stores its model with agent_window(row)
      let t = setup("claude-t-1");
      let o = grant(&t, Agent::OpenCode);
      assert_eq!(windows(&t, &o), (Some("claude-t-1".into()), Some(200000), Some(150000), Some(150000)));
      run(&t, &o, "/g", &reply(S0(), &TEXT(), "end_turn", U15()), "code", &[]);
      assert_eq!(windows(&t, &o), (Some("gpt-t-1".into()), Some(400000), Some(360000), None));
  }

  /// A passthrough Messages request for `name`, answered by the Anthropic fake upstream.
  fn passthrough_request(t: &T, g: &SessionGrant, name: &str) {
      t.anth.script("", vec![sse_reply(&reply(S0(), &TEXT(), "end_turn", u4(1, 0, 5000, 1)))]);
      let mut hd = pt(g);
      hd.extend([("authorization", OAT_BEARER), MAIN]);
      t.n.set(t.n.get() + 1);
      let (s, _, b) = send(&t.h, "POST", M, &hd, Some(req_body(name).to_string().into_bytes()));
      assert_eq!(s, 200, "{}", String::from_utf8_lossy(&b));
  }

  #[test]
  fn status_passthrough_no_model_until_served() { // AU29, H§7.2 `model`
      let t = setup("claude-t-1");
      let g = grant_with(&t, Agent::ClaudeCode, true);
      assert_eq!(windows(&t, &g), (None, None, None, None));
      passthrough_request(&t, &g, "claude-t-1");
      assert_eq!(windows(&t, &g), (Some("claude-t-1".into()), Some(200000), Some(150000), Some(150000)));
  }

  #[test]
  fn status_model_name_capped() { // author's call (HUD-5 review): at most 128 bytes, cut on a char boundary
      let t = setup("claude-t-1");
      let g = grant_with(&t, Agent::ClaudeCode, true);
      passthrough_request(&t, &g, &format!("x{}", "é".repeat(100))); // 201 bytes, no catalog row
      let w = windows(&t, &g);
      assert_eq!(w, (Some(format!("x{}", "é".repeat(63))), None, None, None)); // byte 128 is inside an `é`: 127 bytes
      assert_eq!(w.0.unwrap().len(), 127);
  }
  ```

- [x] **5. Write the failing reload test.** In `crates/zygo-proxy/tests/serve.rs`, add `use zygo_proxy::client::SessionStatus;` and append:

  ```rust
  #[test]
  fn status_windows_after_reload() { // AU29: snapshot before the first main-loop request; then the served row
      let s = setup_with(|t| format!("main_role = \"code\"\n{t}"), |_| {});
      let g = grant(&s, &s.alice);
      let status = |s: &Srv, g: &SessionGrant, rows: u64| {
          assert!(s.h.usage_written(rows, Duration::from_secs(10)), "usage rows not written");
          let r = send(s, "GET", &format!("v1/zygo/sessions/{}/status", g.session), &session_hd(g), None);
          assert_eq!(r.0, 200, "{}", String::from_utf8_lossy(&r.2));
          let v: SessionStatus = json_of(&r);
          (v.model, v.context_window, v.effective_window, v.threshold)
      };
      // test-code: window 200k, soft 150k; agent window 1M (not a claude- id) → safety 180k.
      let snapshot = (Some("test-code".into()), Some(200000), Some(150000), Some(150000));
      assert_eq!(status(&s, &g, 0), snapshot);
      rewrite(&s, |t| {
          let old = "soft_context_window = 150000\n[model.facts]\ncontext_window = 200000\n";
          assert!(t.contains(old));
          t.replace(old, "soft_context_window = 120000\n[model.facts]\ncontext_window = 300000\n")
      });
      s.h.reload().unwrap();
      // No main-loop request yet: the main-role rule reads the session's snapshot.
      assert_eq!(status(&s, &g, 0), snapshot);
      assert_eq!(msg(&s, &g, "code").0, 200);
      // The served row of the new config: window 300k, soft 120k, safety 270k.
      assert_eq!(status(&s, &g, 1), (Some("test-code".into()), Some(300000), Some(120000), Some(120000)));
      s.h.shutdown();
  }
  ```

- [x] **6. Write the failing blocked-name test.** Append to `crates/zygo-proxy/tests/passthrough.rs`:

  ```rust
  #[test] // AU29, H§7.2 `model`: a name the secret pass blocks is "" in the row; the status then has no model
  fn status_no_model_for_blocked_name() {
      let t = setup("\n[features.redaction]\nsecrets = false\n");
      let g = grant(&t, Agent::ClaudeCode, true);
      let status = |n: u64| {
          assert!(t.h.usage_written(n, Duration::from_secs(10)));
          let s = t.h.session(&g.session).unwrap().status();
          (s.model, s.context_window, s.effective_window, s.threshold)
      };
      assert_eq!(post_pt(&t, &g, M, mb("claude-test-1"), &[]).0, 200);
      assert_eq!(status(1), (Some("test-pt".into()), None, None, None)); // the catalog match; test-pt has no facts
      let blocked = format!("TRACERBLOCK_{}", 123456);
      assert_eq!(post_pt(&t, &g, M, mb(&blocked), &[]).0, 200); // the secret pass is off: the request is served
      assert_eq!(status(2), (None, None, None, None));
      assert!(rows(&t, &g).iter().any(|r| r.model.is_empty() && r.status == 200));
  }
  ```

- [x] **7. Add the compact_cmd assertions (H§17 note 5).** In `crates/zygo/tests/compact_cmd.rs` `slash_off_disables_and_on_clears`, keep every line; after line 173 add `assert_eq!((s.model.as_deref(), s.context_window, s.threshold), (Some("claude-opus-5-5"), Some(1_000_000), None));` and after line 176 add `assert_eq!((s.model.as_deref(), s.context_window, s.threshold), (Some("claude-opus-5-5"), Some(1_000_000), Some(800_000)));`.

- [x] **8. Run them and see them fail.** `cargo test -p zygo-proxy --test session_api --test window --test serve --test passthrough` and `cargo test -p zygo --test compact_cmd` → compile errors only: no field `model`, `context_window` or `threshold` on `SessionStatus` (E0609, E0560), no `Default` for `SessionStatus` (E0277), `soft_threshold` private (E0603). No temporary body is written to get a runtime red (stubs are forbidden). Once it builds, the assertions discriminate: today's rule gives `effective_window` 150000, not 180000, after `plan[1m]`; `Some(150000)`, not `None`, in passthrough before a request; the snapshot's 200000/150000 after the reload and a request.

- [x] **9. Add the fields and `Default`.** In `crates/zygo-proxy/src/client.rs`, `SessionStatus` derives `Debug, Clone, Default, PartialEq, Serialize, Deserialize` and gains, after `usage_note`, each with `#[serde(default, skip_serializing_if = "Option::is_none")]`: `pub model: Option<String>`, `pub context_window: Option<u64>`, `pub threshold: Option<u64>`. In `crates/zygo-hooks/src/statusline.rs` `fn s()`, add `..Default::default()` after `usage_note: None,`. Check: `cargo build -p zygo-proxy` → the only error is the `SessionStatus { … }` literal in `status()` (E0063, missing fields); step 11 fixes it.

- [x] **10. Window helpers.** In `crates/zygo-proxy/src/window.rs`, change the import to `use zygo_config::{Facts, ModelRow};` and make `soft_threshold` `pub` (signature unchanged: `pub fn soft_threshold(compact: Option<WindowArg>, row: &ModelRow, safety: Option<u64>) -> Option<u64>`). Insert right after the closing brace of `one_m`, before the `/// 90 % of `cw`` comment of `safety`:

  ```rust
  /// The agent's assumed window for one request (§10.2.2, AU29): 1M for a `[1m]` request, else `agent_window(row)`.
  pub(crate) fn agent_window_for(suffix: bool, headers: &HeaderMap, row: &ModelRow) -> u64 {
      if one_m(suffix, headers) { ONE_M_WINDOW } else { agent_window(row) }
  }

  /// What the status needs of a served row (AU29): its soft window, its context window and the agent window.
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub(crate) struct RequestWindows {
      pub(crate) soft: Option<u64>,
      pub(crate) context_window: Option<u64>,
      pub(crate) agent_window: u64,
  }

  impl RequestWindows {
      pub(crate) fn of(row: &ModelRow, agent_window: u64) -> RequestWindows {
          RequestWindows { soft: row.soft_context_window, context_window: context_window(row), agent_window }
      }
      /// A row holding only the two values `soft_threshold` and `effective_for` read.
      pub(crate) fn view(&self) -> ModelRow {
          let facts = self.context_window.map(|context_window| Facts { context_window, ..Default::default() });
          ModelRow { soft_context_window: self.soft, facts, ..Default::default() }
      }
  }
  ```

  In `WindowStage::on_response_line`, replace `let aw = if one_m(ctx.resolved.one_m, &ctx.headers) { ONE_M_WINDOW } else { agent_window(row) };` with `let aw = agent_window_for(ctx.resolved.one_m, &ctx.headers, row);`.

- [x] **11. Session store and status.** In `crates/zygo-proxy/src/session.rs`, change `use crate::window;` to `use crate::window::{self, RequestWindows};`, and right after `pub const REDACTION_NOTE: …;` add:

  ```rust
  /// The longest stored model name in bytes (author's call, HUD-5 review): a passthrough name is client text.
  pub(crate) const MODEL_NAME_MAX: usize = 128;

  /// The last main-loop request (AU29): its model (`None` for an empty name) and, when the serving config had the
  /// served row, that row's windows.
  struct MainLoop {
      model: Option<String>,
      windows: Option<RequestWindows>,
  }
  ```

  `Live` gains `main_loop: Option<MainLoop>`; the `Live { … }` literal in `Sessions::create` gains `main_loop: None`. Add to `impl SessionState`, after `set_used_context`:

  ```rust
  /// The last main-loop request (AU29), under one lock; the name is cut to `MODEL_NAME_MAX` bytes.
  pub(crate) fn set_main_loop(&self, prompt: u64, model: &str, windows: Option<RequestWindows>) {
      let model = (!model.is_empty()).then(|| model[..model.floor_char_boundary(MODEL_NAME_MAX)].to_string());
      let mut l = lock(&self.live);
      l.used_context = Some(prompt);
      l.main_loop = Some(MainLoop { model, windows });
  }
  ```

  In `status()`, replace the `let effective_window = …;` statement (and its comment) with this rule (the stored request; else nothing in passthrough; else today's main-role rule on the snapshot), and set `model`, `context_window` and `threshold` in the `SessionStatus` literal:

  ```rust
  let (model, w) = match &l.main_loop {
      Some(m) => (m.model.clone(), m.windows),
      None if self.passthrough => (None, None),
      None => {
          let r = self.config.roles.get(&self.config.main_role);
          let row = r.and_then(|r| self.cfg.config.catalog.get(&r.model));
          (r.map(|r| r.model.clone()), row.map(|row| RequestWindows::of(row, window::agent_window(row))))
      }
  };
  let context_window = w.and_then(|w| w.context_window);
  let (threshold, effective_window) = w.map_or((None, None), |w| {
      let row = w.view();
      let s = context_window.and_then(|cw| window::safety(cw, w.agent_window));
      (window::soft_threshold(l.compact, &row, s), window::effective_for(l.compact, &row, w.agent_window))
  });
  ```

- [x] **12. Store at request time.** In `crates/zygo-proxy/src/forward.rs`, `Pending` gains `/// Messages only: the served row's windows from the serving config (AU29).` `windows: Option<window::RequestWindows>`; the `Pending { … }` literal in `handle` gains `windows: None`. In `run`, right after `let resolved = ctx.resolved.clone().expect("stage 2 resolved the request");`, add:

  ```rust
  if route == ModelRoute::Messages {
      pending(slot).windows = ctx.cfg.config.catalog.get(&resolved.model)
          .map(|row| window::RequestWindows::of(row, window::agent_window_for(resolved.one_m, &ctx.headers, row)));
  }
  ```

  In `Pending::drop`, keep the `used_context` condition and replace `self.session.set_used_context(prompt);` with `self.session.set_main_loop(prompt, &r.model, self.windows.take());`. The model name is `Pending.row.model`, which `run` sets from `Resolved.model`; an unmatched passthrough name has no row and stores the name with no windows.

- [x] **13. Run them and see them pass.** `cargo test -p zygo-proxy --test session_api --test window` → `0 failed` in both targets, including `status_default_keeps_literals_short`, `status_reply_omits_absent_new_fields`, `client_compact_and_status`, `status_used_and_effective`, `unit_soft_threshold_public`, `status_follows_one_m_request`, `status_follows_one_m_beta`, `status_follows_other_role`, `status_ignores_compaction_request`, `status_ignores_non_2xx_reply`, `status_ignores_count_tokens`, `status_follows_non_streamed_request`, `status_follows_dropped_stream`, `status_follows_opencode_request`, `status_passthrough_no_model_until_served` and `status_model_name_capped`. `cargo test -p zygo-proxy --test serve status_windows_after_reload` → `1 passed`. `cargo test -p zygo-proxy --test passthrough status_no_model_for_blocked_name` → `1 passed`. `cargo test -p zygo --test compact_cmd slash_off_disables_and_on_clears` → `1 passed`. `cargo test -p zygo-hooks --lib` → `0 failed` (the old `render` output does not change).

- [x] **14. See H§14 #19 fail.** `rg 'div_ceil\(4\)' crates/zygo-proxy/src` → two lines, both in `forward.rs` `run`: the local count_tokens estimate (`"input_tokens": string_bytes(&ctx.body).div_ceil(4)`) and `let estimate = string_bytes(&ctx.body).div_ceil(4);`.

- [x] **15. Add `BYTES_PER_TOKEN` (OP1).** New file `crates/zygo-core/src/tokens.rs`:

  ```rust
  //! Token estimates from byte counts (H§6.1 OP1).

  /// Bytes per estimated token: the proxy's `count_tokens` estimate (§7.3) and the HUD's trimmed tokens.
  pub const BYTES_PER_TOKEN: u64 = 4;
  ```

  Add `pub mod tokens;` after `pub mod text;` in `crates/zygo-core/src/lib.rs`. In `forward.rs`, add `use zygo_core::tokens::BYTES_PER_TOKEN;` and change both sites to `string_bytes(&ctx.body).div_ceil(BYTES_PER_TOKEN)`.

- [x] **16. See H§14 #19 pass.** `rg 'div_ceil\(4\)' crates/zygo-proxy/src` → no output, exit 1; `rg -c BYTES_PER_TOKEN crates/zygo-proxy/src/forward.rs` → `3`. `cargo test -p zygo-proxy --test request_path count_tokens_estimated_locally` → `1 passed` (the estimate stays 6 for 22 bytes).

- [x] **17. Align the spec with the stored values (AU29, H§7.2).** In `docs/specs/2026-11-21-statusline-hud-design.md` (DOC-1 copies this text into the base spec), replace these table cells with the text given:

  - AU29 (the `| AU29 |` row of H§6.1, its decision cell): `The proxy's status `effective_window`, `threshold` and `context_window` follow the last main-loop request. At request time, after resolution, the proxy looks up the catalog row of `resolved.model` in that request's serving config (`ctx.cfg`) and keeps in `Pending` what the status needs of it: `soft_context_window`, `facts.context_window` and the request's agent window, computed with the rule of the window stage, `if one_m(resolved.one_m, &headers) { ONE_M_WINDOW } else { agent_window(row) }` (`crates/zygo-proxy/src/window.rs`, `agent_window_for`). `Pending::drop` stores these values with the served model in the session under the existing `used_context` condition (route `/anthropic/v1/messages`, 2xx, class `main` or absent, prompt > 0), also when the stream ends before `message_delta`. `status()` computes `context_window`, `threshold` and `effective_window` from the stored values and the current override each time; it does not read the session's config snapshot for them, so after a serve-mode reload they stay those of the row that served the request. So a non-streamed main-loop request stores them too (the condition does not test streaming), and an OpenCode request on the Anthropic route stores its model with `agent_window(row)` (OpenCode sends no `[1m]` name and no `context-1m-` beta); requests on other routes, non-2xx replies, subagent and compaction requests store nothing, and the status keeps the previous values. A served model with no row in the serving config (an unmatched passthrough name) gives `model` and no windows. Before the first main-loop request, also after a serve-mode reload, it uses the main role's model and row from the session's config snapshot and `agent_window(row)` (today's rule, which ignores `[1m]`). In passthrough it sends no `model` (and no windows) until a request is served, because the main role does not apply. After `/model` picks another role, the HUD shows that role's window.`
  - H§7.2 `model` row, last cell: `Catalog id of the model of the last main-loop 2xx Messages request with a prompt above 0, stored with the row values of AU29 when the request's usage row is written (`Pending::drop`), under the same condition as `used_context`, also when the stream ends before `message_delta`. Before the first one, the main role's model from the session's config snapshot; in passthrough, absent from the reply until a request is served (AU29). An unmatched passthrough name is the requested name after the secret pass, with `[1m]` removed (`requested_name`, `passthrough_names` in `crates/zygo-proxy/src/forward.rs`), cut to at most 128 bytes on a character boundary; when that name is empty (the secret pass blocked or failed), `model` is absent.`
  - H§7.2 `context_window` row, last cell: `facts.context_window` of `model`'s catalog row, taken from the serving config when the request is served and stored (AU29); before the first main-loop request, from the main role's row in the session's config snapshot.`
  - H§7.2 `threshold` row, last cell: `Present only when a numeric override or the row's `soft_context_window` exists; value = that number, clamped by the safety window (`min`), for `model` (`soft_threshold`, `crates/zygo-proxy/src/window.rs`, made public), from the row values stored at request time from the serving config (AU29; before the first main-loop request, the main role's row in the session's config snapshot). Absent with a safety window alone, with no soft window and no override, and with the override `off` (AU28).`

  Append to the `## Change log` table, at its end:

  ```
  | 2026-11-21 | H§7.2 model row | author's call (HUD-5 review) | matches AU29: stored under the used_context condition |
  | 2026-11-21 | AU29, H§7.2 rows context_window and threshold | author's call (HUD-5 review) | status() uses the row values stored at request time from the serving config, so a serve-mode reload does not change them; before the first main-loop request, also after a reload, the main role's row from the session snapshot |
  | 2026-11-21 | H§7.2 model row | author's call (HUD-5 review) | the stored model name is at most 128 bytes, cut on a character boundary, so a passthrough client cannot put a long string into every status reply; an empty name stores no model |
  ```

  Check: `grep -c 'HUD-5 review' docs/specs/2026-11-21-statusline-hud-design.md` → `3`; `grep -c 'stored with its agent window at that request' docs/specs/2026-11-21-statusline-hud-design.md` → `0`; `grep -c 'session.s config snapshot' docs/specs/2026-11-21-statusline-hud-design.md` → at least `4`.

- [x] **18. Run the wider proxy and CLI tests.** `cargo test -p zygo-proxy` (every target: lib unit tests including `forward.rs`, `request_path`, `passthrough`, `serve`, `session_api`, `window` and the rest) → `0 failed` in each target. `cargo test -p zygo --test compact_cmd` → `0 failed`.

- [x] **19. Run the node's Acceptance exactly as written.** `cargo test -p zygo-proxy --test session_api --test window && cargo test -p zygo-hooks --lib && cargo test --workspace --locked --no-run && ! grep -rqE 'div_ceil\(4\)' crates/zygo-proxy/src` → exit 0; every listed test passes; every workspace test target builds (this covers `crates/zygo/src/cmd/status.rs` and `crates/zygo-launch/tests/serve_client.rs`, which use `SessionStatus` with no literal); `Cargo.lock` does not change.

- [x] **20. Commit** `crates/zygo-core/src/tokens.rs`, `crates/zygo-core/src/lib.rs`, `crates/zygo-proxy/src/client.rs`, `crates/zygo-proxy/src/session.rs`, `crates/zygo-proxy/src/forward.rs`, `crates/zygo-proxy/src/window.rs`, `crates/zygo-proxy/tests/session_api.rs`, `crates/zygo-proxy/tests/window.rs`, `crates/zygo-proxy/tests/serve.rs`, `crates/zygo-proxy/tests/passthrough.rs`, `crates/zygo/tests/compact_cmd.rs`, `crates/zygo-hooks/src/statusline.rs` and `docs/specs/2026-11-21-statusline-hud-design.md` with this message:

  ```
  feat(proxy): status model, context_window and threshold of the last main-loop request

  AU29: at request time the proxy keeps the served row's soft and context
  windows from the serving config and the request's agent window; under the
  used_context condition they are stored with the served model (at most 128
  bytes; an empty name stores none), and status() derives the windows from
  them and the current override. AU28: threshold is the soft or override
  point clamped by the safety window. SessionStatus derives Default (H§17
  note 1); absent new fields are not serialized. OP1: BYTES_PER_TOKEN
  replaces the literal 4. The spec's AU29 and H§7.2 rows match.

  Zygo-Session: <the executor's own zygo session id>
  Zygo-Work: HUD-5
  ```

### Interfaces

**Consumes** (existing, unchanged):
- `zygo_proxy::window::context_window(row: &ModelRow) -> Option<u64>` — `window.rs:24`
- `zygo_proxy::window::agent_window(row: &ModelRow) -> u64` — `window.rs:29`
- `zygo_proxy::window::one_m(suffix: bool, headers: &HeaderMap) -> bool` — `window.rs:42`
- `zygo_proxy::window::safety(cw: u64, agent_window: u64) -> Option<u64>` — `window.rs:53`
- `zygo_proxy::window::effective_for(compact: Option<WindowArg>, served: &ModelRow, agent_window: u64) -> Option<u64>` — `window.rs:87`
- `zygo_proxy::resolve::Resolved { requested: String, one_m: bool, role: String, model: String, .. }` — `resolve.rs:13-23`
- `zygo_config::ModelRow`, `zygo_config::Facts` (both `Default`) — `crates/zygo-config/src/schema.rs:100-116`
- `str::floor_char_boundary(&self, index: usize) -> usize` — std, stable on toolchain 1.98

**Produces:**
- `zygo_core::tokens::BYTES_PER_TOKEN: u64 = 4` — named by HUD-8 and HUD-12 (Consumes).
- `SessionStatus` with `#[derive(Default)]`, so `SessionStatus::default() -> SessionStatus` — named by HUD-6 (Consumes). Other nodes build `SessionStatus { .., ..Default::default() }` in their tests or add fields after the HUD's: HUD-8, HUD-9, HUD-10, HUD-11, HUD-12, W9 and X1 (each in its Spec line).
- `SessionStatus.model: Option<String>`, `SessionStatus.threshold: Option<u64>` — named by HUD-8 (Consumes). `SessionStatus.context_window: Option<u64>` — no node names it yet. All three are `#[serde(default, skip_serializing_if = "Option::is_none")]`.
- `pub fn zygo_proxy::window::soft_threshold(compact: Option<WindowArg>, row: &ModelRow, safety: Option<u64>) -> Option<u64>` — no node names it yet.
- Internal to `zygo-proxy` (no other node uses them): `pub(crate) const session::MODEL_NAME_MAX: usize = 128`; `pub(crate) fn window::agent_window_for(suffix: bool, headers: &HeaderMap, row: &ModelRow) -> u64`; `pub(crate) struct window::RequestWindows { soft: Option<u64>, context_window: Option<u64>, agent_window: u64 }` with `of(row: &ModelRow, agent_window: u64) -> RequestWindows` and `view(&self) -> ModelRow`; `pub(crate) fn SessionState::set_main_loop(&self, prompt: u64, model: &str, windows: Option<RequestWindows>)`; private `session::MainLoop { model: Option<String>, windows: Option<RequestWindows> }`, `Live.main_loop: Option<MainLoop>`; `Pending.windows: Option<RequestWindows>`.
