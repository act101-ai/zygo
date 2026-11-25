# SX-6 — OpenCode plugin transform gives the mechanics text on non-main `zygo/` models

**Spec:** docs/specs/2026-11-24-session-text-design.md §7.2 item 1, §14 item 3, §17 check 10
**Files:** `crates/zygo-launch/assets/opencode-plugin.js`, `crates/zygo-launch/tests/opencode.rs`
**Consumes:** `zygo hook session-start --agent opencode --event subagent` (SX-2)
**Produces:** plugin hook `experimental.chat.system.transform` (routes by `input.model`)
**Acceptance:** `test "$(cargo test -q -p zygo-launch --test opencode -- subagent_transform --list 2>/dev/null | grep -c ': test$')" -ge 4 && cargo test -p zygo-launch --test opencode` → every count holds (at least 4 named `subagent_transform…` in zygo-launch `opencode`; a count of 0 fails before any run), then all pass on ubuntu and macOS (the `node_run` tests are `cfg(unix)`, `crates/zygo-launch/tests/opencode.rs:669`), with T§17 check 10 asserted from the stub's `calls.txt`: with `ZYGO_MAIN_ROLE=lead`, `read/zygo` and `code/zygo` call `hook session-start --agent opencode --event subagent` and push the stub output into `output.system`; `lead/zygo` calls `hook session-start --agent opencode` with no `--event`; `lead/other` calls nothing; with `ZYGO_MAIN_ROLE` unset a `zygo` model calls `--event subagent`

## Plan

### Premises (checked against `origin/phase-3` at `b0fe268c`)

- The transform today: `crates/zygo-launch/assets/opencode-plugin.js:185-190` returns unless `ZYGO_MAIN_ROLE` is non-empty and `input.model` is `{providerID: "zygo", id: <role>}`, then `run("session-start", { input, output })` and pushes a non-empty result into `output.system`. `run(name, payload, extra = [])` (`:7`) appends `extra` after `--agent opencode` (`:23`) and resolves `""` on any failure (`:12-15`, `:27-28`). Matches T§7.1.
- SX-2 is merged (PR #47, `88549958`): `run_in` and `run_member` in `crates/zygo-hooks/src/session_start.rs:29-30` and `:107-108` return `subagent_text(dir: &SessionDir) -> anyhow::Result<HookOutput>` (`:53`) when `input.cli_event.as_deref() == Some("subagent")`; `crates/zygo/src/cmd/hook.rs:123,132` set `cli_event` from `--event`.
- Test harness: `node_run(fx: &Fx, name: &str, mode: &str, role: &str, big: bool) -> NodeRun` (`crates/zygo-launch/tests/opencode.rs:670`); its stub `zygo` logs `$*` to `calls.txt`, writes stdin to `stdin-$2.json`, and prints `TEXT` for `session-start:ok` (`:684`); its driver calls the transform for `lead/zygo`, `code/zygo`, `read/zygo`, `lead/other` in that order (`:703-707`). `no_node(test: &str) -> bool` (`:1412`). `plugin_behaves_under_node` (`:732`) asserts `code/zygo` and `read/zygo` give `[]` (`:748-750`) and the `norole` run (`ZYGO_MAIN_ROLE=""`) calls no `session-start` (`:760-765`); T§17 check 10 changes these.
- Drift: every `node_run` and `ws_node` test is `#[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]` (commit `96c1cbc9`, owner rule: no test bound by a fixed production time limit in CI). The new tests follow that rule. So `cargo test -p zygo-launch --test opencode` lists them (libtest `--list` prints ignored tests as `<name>: test`, checked: `plugin_behaves_under_node: test`) but does not run them; check 10 is run by the `--ignored` command in the verification step. The one CI-run assertion this node adds is the template token in step 1.
- Baseline: `cargo test -q -p zygo-launch --test opencode -- --ignored plugin_behaves_under_node` passes (1 passed).

### Decisions the spec left open

- `ZYGO_MAIN_ROLE=""` counts as not set (today's `!role` test); a `zygo/` model then gets `--event subagent`, as an unset variable does (T§7.2 item 1 "not set").
- `node_run` takes `role: Option<&str>`; `None` removes `ZYGO_MAIN_ROLE` from the child environment, so the unset case of the Acceptance is tested and the empty case stays tested.
- The stub prints `TEXT` followed by `$6` for `session-start:ok`, so the main call prints `TEXT` and the subagent call prints `TEXTsubagent`; each `output.system` entry then shows which call filled it.
- The `norole` block of `plugin_behaves_under_node` moves into `subagent_transform_unset_main_role_calls_event_subagent`; the rest of that test is kept with its expectations updated.

### Steps

- [x] 1. Write the failing tests in `crates/zygo-launch/tests/opencode.rs`.
  - In `plugin_text_matches_the_fixtures` (`:643-645`) add the token `"\"subagent\""` to the list.
  - `node_run`: change `role: &str` to `role: Option<&str>`; set the variable with `match role { Some(r) => cmd.env("ZYGO_MAIN_ROLE", r), None => cmd.env_remove("ZYGO_MAIN_ROLE") }` in place of `.env("ZYGO_MAIN_ROLE", role)`; change the stub line `session-start:ok) printf TEXT ;;` to `session-start:ok) printf 'TEXT%s' "$6" ;;`. Callers at `:739, :742, :768, :777, :785` pass `Some("lead")`.
  - `plugin_behaves_under_node`: replace the loop at `:748-750` with `assert_eq!(sys(&v, "code/zygo"), json!(["TEXTsubagent"])); assert_eq!(sys(&v, "read/zygo"), json!(["TEXTsubagent"])); assert_eq!(sys(&v, "lead/other"), json!([]));`; delete the `norole` block (`:760-765`).
  - Add after `plugin_behaves_under_node`:
    ```rust
    #[cfg(unix)]
    const MAIN_CALL: &str = "hook session-start --agent opencode";
    #[cfg(unix)]
    const SUB_CALL: &str = "hook session-start --agent opencode --event subagent";
    #[cfg(unix)]
    fn session_starts(r: &NodeRun) -> Vec<&str> {
        r.calls.lines().filter(|l| l.starts_with("hook session-start")).collect()
    }
    ```
    and four tests, each `#[cfg(unix)] #[test] #[ignore = "timing: each stub zygo call under node has the plugin's fixed 10 s timeout"]`, each starting `if no_node("<its name>") { return; } let fx = fx();`, each with a comment `// T§7.2 item 1, T§17 check 10`:
    ```rust
    fn subagent_transform_other_zygo_role_calls_event_subagent() {
        let r = node_run(&fx, "sx-sub", "ok", Some("lead"), false);
        let v = r.result.clone().unwrap();
        assert_eq!(v["system"]["code/zygo"], json!(["TEXTsubagent"]));
        assert_eq!(v["system"]["read/zygo"], json!(["TEXTsubagent"]));
        assert_eq!(session_starts(&r)[1..], [SUB_CALL, SUB_CALL]);
        let stdin: Value = serde_json::from_slice(&std::fs::read(r.dir.join("stdin-session-start.json")).unwrap()).unwrap();
        assert_eq!(stdin, json!({"input": {"model": {"id": "read", "providerID": "zygo"}}, "output": {"system": []}, "cwd": "/r"}));
    }
    fn subagent_transform_main_role_calls_without_event() {
        let r = node_run(&fx, "sx-main", "ok", Some("lead"), false);
        assert_eq!(r.result.clone().unwrap()["system"]["lead/zygo"], json!(["TEXT"]));
        assert_eq!(session_starts(&r)[0], MAIN_CALL);
        assert_eq!(session_starts(&r).iter().filter(|l| **l == MAIN_CALL).count(), 1);
    }
    fn subagent_transform_other_provider_calls_nothing() {
        let r = node_run(&fx, "sx-other", "ok", Some("lead"), false);
        assert_eq!(r.result.clone().unwrap()["system"]["lead/other"], json!([]));
        assert_eq!(session_starts(&r), [MAIN_CALL, SUB_CALL, SUB_CALL]);
    }
    fn subagent_transform_unset_main_role_calls_event_subagent() {
        for (name, role) in [("sx-unset", None), ("sx-empty", Some(""))] {
            let r = node_run(&fx, name, "ok", role, false);
            let v = r.result.clone().unwrap();
            for k in ["lead/zygo", "code/zygo", "read/zygo"] {
                assert_eq!(v["system"][k], json!(["TEXTsubagent"]), "{name} {k}");
            }
            assert_eq!(v["system"]["lead/other"], json!([]), "{name}");
            assert_eq!(session_starts(&r), [SUB_CALL, SUB_CALL, SUB_CALL], "{name}");
        }
    }
    ```
  - Check: `cargo test -q -p zygo-launch --test opencode -- subagent_transform --list 2>/dev/null | grep -c ': test$'` prints `4`.
- [x] 2. See them fail. `cargo test -p zygo-launch --test opencode -- --include-ignored subagent_transform plugin_behaves_under_node plugin_text_matches_the_fixtures` → `plugin_text_matches_the_fixtures` fails on `"subagent"`; `subagent_transform_other_zygo_role_calls_event_subagent`, `subagent_transform_other_provider_calls_nothing`, `subagent_transform_unset_main_role_calls_event_subagent` and `plugin_behaves_under_node` fail (`code/zygo` is `[]`); `subagent_transform_main_role_calls_without_event` passes (it pins today's main-role call).
- [x] 3. Implement in `crates/zygo-launch/assets/opencode-plugin.js:185-190`. The hook keeps its key and `async (input, output)` signature:
  ```js
  // T§7.2 item 1: every `zygo/` model; the main role as today, any other role (or no main role) `--event subagent`.
  "experimental.chat.system.transform": async (input, output) => {
    if (!input.model || input.model.providerID !== "zygo") return;
    const role = process.env.ZYGO_MAIN_ROLE;
    const extra = role && input.model.id === role ? [] : ["--event", "subagent"];
    const text = await run("session-start", { input, output }, extra);
    if (text) output.system.push(text);
  },
  ```
  Check: `git diff --stat` names only `opencode-plugin.js` and `tests/opencode.rs`.
- [x] 4. See them pass. The step-2 command → `test result: ok. 6 passed; 0 failed`.
- [x] 5. Commit `crates/zygo-launch/assets/opencode-plugin.js` and `crates/zygo-launch/tests/opencode.rs` with the message `SX-6: OpenCode transform gives the mechanics text on non-main zygo models`, then a blank line and the trailers `Zygo-Session: 01M87GS3GVFD3PYEFC74AYVBY7` and `Zygo-Work: phase3:SX-6`, each on its own line.

### Consumes

- `zygo hook session-start --agent opencode --event subagent` (SX-2): stdin `{input, output, cwd}`; `run_member(input: &HookInput, dir: &SessionDir, paths: &Paths, zygo: &Path, out: &mut dyn Write) -> anyhow::Result<HookOutput>` returns `subagent_text(dir)` (`crates/zygo-hooks/src/session_start.rs:53`), the mechanics text as plain text, exit 0; a failure gives exit non-zero and `run` resolves `""`.
- `run(name, payload, extra = [])` (`crates/zygo-launch/assets/opencode-plugin.js:7`), unchanged.

### Produces

- Plugin hook `"experimental.chat.system.transform": async (input, output)` in `crates/zygo-launch/assets/opencode-plugin.js`: `input.model.providerID === "zygo"` and `input.model.id === ZYGO_MAIN_ROLE` (non-empty) → `hook session-start --agent opencode`; another `zygo` id, or `ZYGO_MAIN_ROLE` empty or unset → `hook session-start --agent opencode --event subagent`; another provider → no call. A non-empty output is pushed into `output.system`.
- Test helper `node_run(fx: &Fx, name: &str, mode: &str, role: Option<&str>, big: bool) -> NodeRun`.

### Verification

- `test "$(cargo test -q -p zygo-launch --test opencode -- subagent_transform --list 2>/dev/null | grep -c ': test$')" -ge 4 && cargo test -p zygo-launch --test opencode` → the count is 4, then `test result: ok.` with `0 failed` (the node tests show as ignored).
- `cargo test -p zygo-launch --test opencode -- --ignored subagent_transform plugin_behaves_under_node` → `test result: ok. 5 passed; 0 failed` (T§17 check 10 from the stub's `calls.txt`; needs `node` on `PATH`).
- No new test reads a clock or runs a real `opencode` or `claude` binary (T§17 check 15): the driver has no `Date.now`, and the binary is the `node_run` stub.

### Review round 1

- [x] **R1. `anthropic` provider (T§17 check 10).** The `node_run` driver's other-provider case is `{providerID: "anthropic", id: "lead"}` (key `lead/anthropic`); `subagent_transform_other_provider_calls_nothing` is renamed `subagent_transform_anthropic_provider_calls_nothing`. Passes on the current plugin; fails with the provider check removed.
- [x] **R2. Every subagent call's stdin.** The stub appends each call's stdin as one line to `stdin-<hook>.jsonl`; `session_start_stdins(r: &NodeRun) -> Vec<Value>` reads it. `subagent_transform_other_zygo_role_calls_event_subagent` asserts 3 records, and exactly one each for the `code/zygo` and `read/zygo` payloads, whatever the driver order. Passes on the current plugin; fails when only the `code` payload is wrong (the old last-file assert did not).
- [x] **R3. Malformed `input.model`.** `node_run_edge(.., edge: Option<&str>)` runs one driver `EDGE` case only. `subagent_transform_missing_or_null_model_calls_nothing`: `{}` and `{model: null}` → no zygo call, no throw, `output.system` stays `[]`. `subagent_transform_zygo_model_without_id_calls_event_subagent`: `{providerID: "zygo"}` → one `--event subagent` call. Both pass on the current plugin; the null case fails with `!input.model` changed to `input.model === undefined`.
- [x] **R4. No `output.system` array (T-C1).** `subagent_transform_without_system_array_calls_nothing`: `output = {}` → no zygo call, no throw. Failed first (`Cannot read properties of undefined (reading 'push')`, after the zygo call); the transform now returns before `run` when `output.system` is not an array.
