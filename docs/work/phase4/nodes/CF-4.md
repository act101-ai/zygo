# CF-4 — Registry, Claude Code and OpenCode grade files, generated outputs, the tree test, CI paths, old scripts deleted

**Spec:** docs/specs/2026-11-25-client-model-design.md K§6.2.1, K§6.2.2 (the migration table and tiers), K§6.2.3 (grade form; bar exception A-1.4), K§6.2.5 (the check on the tree; script deletion), K§6.6 (owners), K§9 (P4-2), K§11 items 1 (on the tree), 3 and 7; K-D9, K-D10, K-D18, K-D26
**Files:** `docs/conformance/capabilities.toml` (new), `docs/conformance/claude-code.toml` (new), `docs/conformance/opencode.toml` (new), `docs/conformance.md`, `docs/conformance.json` (new), `crates/zygo-core/tests/conformance.rs`, `crates/zygo-core/tests/conformance/model.rs`, `.github/workflows/ci.yml`, `scripts/check-conformance.sh` (deleted), `scripts/test-check-conformance.sh` (deleted), `docs/roadmap/phase-5.md`
**Consumes:** `model::check(root: &Path, reg: &Registry, grades: &[GradeFile], clients: &[Client], resolved: &Resolved) -> Vec<Fault>` (CF-1): a met grade with no entry in the resolved set is an item 3 fault, evidence not resolved; `model::resolve(root: &Path, grades: &[GradeFile]) -> Resolved` (CF-1): resolve once, give the result to check and to the renderers; `model::clients(root: &Path) -> Result<Vec<Client>, String>` (CF-1): one Client per Agent::ALL; `Registry::load` (CF-1), `GradeFile::load` (CF-1), `model::Client` (CF-1), `zygo_core::Agent::ALL` (CF-1), `render::markdown` (CF-3), `render::json` (CF-3), `render::compare_or_write` (CF-3)
**Produces:** `conformance_tree()` (the `#[test]` that runs the check and the generator on the repository), `docs/conformance/capabilities.toml`, `docs/conformance/claude-code.toml`, `docs/conformance/opencode.toml`
**Acceptance:** `cargo test -p zygo-core --test conformance --locked -- --exact conformance_tree && ZYGO_WRITE_CONFORMANCE=1 cargo test -p zygo-core --test conformance --locked -- --exact conformance_tree && git diff --exit-code docs/conformance.md docs/conformance.json && cargo test -p zygo-config --test builtin --locked -- --exact minimum_agent_versions_match_premises && git diff --exit-code origin/phase-4 -- crates/zygo-config/tests/builtin.rs && test ! -e scripts/check-conformance.sh && test ! -e scripts/test-check-conformance.sh && test "$(awk '/^  pull_request:/{exit} 1' .github/workflows/ci.yml | grep -cE "^ +- 'docs/(conformance/[*][*]|conformance[.]md|conformance[.]json|live-checks[.]md)'$")" -eq 4 && test "$(awk 'f; /^  pull_request:/{f=1}' .github/workflows/ci.yml | grep -cE "^ +- 'docs/(conformance/[*][*]|conformance[.]md|conformance[.]json|live-checks[.]md)'$")" -eq 4` → every command exits 0 (the last two: the four new paths are in the `push` list, `ci.yml:6-14` at `ec17582b`, and in the `pull_request` list, `:16-24`), and `cargo test --workspace --locked --no-fail-fast` is green on ubuntu, macOS and Windows CI

## Planning notes

- The pull request stops for the operator before merge (K-D26): its body lists each derived cell that differs from the hand grade in today's `docs/conformance.md`, with its reason.
- A core criterion that grades `not-met` for Claude Code or OpenCode with no registry exception fails check item 8 (both count as supported until P4-6). The only approved exception is Claude Code A-1.4 (K-D18). Any other one is a new bar exception, which stops for the operator (K§6.2.1): the node goes `BLOCKED(operator: bar exception <client> <criterion>)` with the evidence. No grade is raised to avoid it (K§3).
- Evidence counts only by K§6.2.3. `docs/premises.md` rows P6a, Q7, PW6, PW7, PW15, PW15-bash and PW16 are `failed` at `ec17582b`, so they do not count. The K§6.2.3 example cites Q7 for A-1.2; that example shows the form, not the evidence.
- The hand notes of today's `docs/conformance.md` ("Windows shell for the generated commands", "Documentation caveat (OpenCode)", per-cell detail) do not survive generation. Their content goes into the `reason` of the grade or bar exception it explains, inside the 300-character rule; the A-1.4 exception may keep the K§6.2.3 example reason.
- CI paths (`.github/workflows/ci.yml:5-24`) skip `docs/**` except three files. Add `docs/conformance/**`, `docs/conformance.md`, `docs/conformance.json` and `docs/live-checks.md` to both lists, so a change of a grade, an output or a premise result runs the check in CI (K§11 item 3).
- `minimum_agent_versions_match_premises` (`crates/zygo-config/tests/builtin.rs:223`) stays unchanged (K§11 item 7).

## Plan

Base: `origin/phase-4` at `16481ba8` (CF-1, CF-2 and CF-3 merged). Branch `phase-4--CF-4`. "Trial run" below means: the planned test and files were run on this base in a scratch checkout with the real resolver and check, then removed; nothing of it is committed.

### Premise check (§3.1)

Holds at `16481ba8`:

- The CF-1 to CF-3 signatures of "Interfaces" are in the code at the lines named there.
- `Agent::ALL` gives `claude-code` and `opencode` (`crates/zygo-core/src/lib.rs:56-57`). `crates/zygo-config/data/agents.toml` has `min_version` `2.1.328` and `1.18.16`. `docs/premises.md` has `Claude Code: 2.1.328` and `OpenCode: 1.18.16`. `docs/live-checks.md` has `Claude Code: 2.1.328 or later` and `OpenCode: 1.18.16 or later`. So check item 7 holds.
- `fixtures/claude-code/` and `fixtures/opencode/` are not empty (item 9).
- `.github/workflows/ci.yml`: the `push` paths are `:6-14` and the `pull_request` paths are `:16-24`, as at `ec17582b`.
- `minimum_agent_versions_match_premises` is at `crates/zygo-config/tests/builtin.rs:223`.
- `docs/premises.md`: P6a, Q7, PW6, PW7, PW15, PW15-bash and PW16 are `failed`. All other rows are `verified`. In `docs/live-checks.md`, only `pass` counts. So the `verified` rows `P-HUD-1/*`, `PRB-1/*` and the `fail`, `observed` and `not run` rows do not count.

Drift:

- **D1. Defect that blocks the node (§5.4: fixed in this node).** `crate_manifest` (`crates/zygo-core/tests/conformance/model.rs:545-580`) refuses each `test:` item in `crates/zygo-launch`. The reason is `[[bin]] stub-agent` with `path = "tests/stub_agent/stub.rs"` and `test = false` (`crates/zygo-launch/Cargo.toml`). It also refuses each `test:` item in `crates/zygo`, because of `[[test]] forbidden_content` with `path = "../../tests/forbidden_content.rs"` (`crates/zygo/Cargo.toml:46-48`). The trial run gave 29 item 3 faults of the form `unsupported Cargo target setting`. Without a fix, these core criteria have no test evidence for both clients: A-5.1, A-5.2, A-10.1, A-13.2, A-21.1 and A-21.2. Neither target changes a target that the walk of `reach` counts. Steps 3-5 let these two forms through and keep every case of `check_3_cargo_target_settings_refused`. **Files gains `crates/zygo-core/tests/conformance/model.rs`; the lead records this in the contract line.**
- **D2.** Q7 is `failed`. Today's hand grades cite Q7 for Claude Code A-1, A-2, A-4, A-5, A-6, A-10, A-11, A-12, A-14, A-20 and A-21, and for OpenCode A-21. The K§6.2.3 example also cites Q7, for A-1.2. None of these counts. The table below cites tests in their place, and never cites a fixture that only a failed row recorded (`fixtures/claude-code/q7/*`, `workspace/pw7.txt`, `workspace/pw16.txt`).
- **D3.** The two stub-launch tests that assert the environment values are `#[ignore = "timing: …"]` (`crates/zygo-launch/tests/stub_launch.rs:38-39`, `:577-578`), so they are not evidence. This includes the `stub_launch.rs` fn of the K§6.2.3 example. Tests that are not ignored carry A-1, A-2, A-10 and A-21.
- **D4.** Graded as written, A-1 and A-10 fail for both clients beyond the approved A-1.4 exception (see "Stops"). No spec section is rewritten: K§6.2.3 does not fix the manifest rule (it is the CF-1 author's rule), and the criteria texts are not changed without the operator (K§6.2.1).

### Interfaces

Consumes, exactly as in the code now:

- `pub fn resolve(root: &Path, grades: &[GradeFile]) -> Resolved` (`model.rs:391`)
- `pub fn check(root: &Path, reg: &Registry, grades: &[GradeFile], clients: &[Client], resolved: &Resolved) -> Vec<Fault>` (`model.rs:928`)
- `pub fn clients(root: &Path) -> Result<Vec<Client>, String>` (`model.rs:1081`): one `Client` per `zygo_core::Agent::ALL`, `supported: true`
- `Registry::load(path: &Path) -> Result<Registry, String>` (`model.rs:261`), `GradeFile::load(path: &Path) -> Result<GradeFile, String>` (`model.rs:306`)
- `pub struct Fault { pub item: u8, pub text: String }` (`model.rs:137`)
- `pub fn markdown(reg: &Registry, grades: &[GradeFile], clients: &[Client], res: &Resolved) -> String` (`render.rs:170`)
- `pub fn json(reg: &Registry, grades: &[GradeFile], clients: &[Client], _res: &Resolved) -> String` (`render.rs:313`)
- `pub fn compare_or_write(root: &Path, md: &str, json: &str) -> Result<(), Fault>` (`render.rs:342`): writes both files when `ZYGO_WRITE_CONFORMANCE=1`, else compares them after CR LF → LF

Produces: `#[test] fn conformance_tree()` in `crates/zygo-core/tests/conformance.rs`; `docs/conformance/capabilities.toml`, `docs/conformance/claude-code.toml`, `docs/conformance/opencode.toml`. BS-1, CC-3 and CC-12 read these files. No node consumes a Rust symbol of CF-4.

Model change (D1), private to `model.rs`: `fn walk_root(p: &str) -> bool` and `fn leaves_crate(crate_dir: &str, p: &str) -> bool`.

### Stops for the operator

The tree test fails check item 8 for both clients until the operator decides each line below. No grade is raised and no exception is added without that decision (K§3, K-D18). Each criterion is core (K-D9).

- **BLOCKED(operator: bar exception claude-code A-1.2).** In passthrough (base spec §12.3), the session token is in the `ANTHROPIC_CUSTOM_HEADERS` environment row as `X-Zygo-Token`. `stub_launch.rs::claude_code_session_token_stays_in_the_token_file_and_in_passthrough_in_settings_and_custom_headers` (`:542-576`) asserts this. A-1.2 allows only the `token` file and "the `0600` passthrough files of §12.3", and an environment row is not a file. If the operator reads A-1.2 as allowing every §12.3 carrier, that is a change of standard text, and it also stops for the operator (K§6.2.1).
- **BLOCKED(operator: bar exception claude-code A-10.2).** The same passthrough row puts the session token in the client's environment. Outside passthrough the token is not in the environment (same test).
- **BLOCKED(operator: bar exception opencode A-1.2).** OpenCode has no key helper. zygo writes the session token as `provider.zygo.options.apiKey` inside the `OPENCODE_CONFIG_CONTENT` environment row (`crates/zygo-launch/src/opencode.rs:162`; base spec §8.7). `opencode.rs::launch_sets_the_opencode_environment` (`crates/zygo-launch/tests/opencode.rs:1183-1188`) asserts this.
- **BLOCKED(operator: bar exception opencode A-1.3).** The inline config sets `model` and `small_model` to `zygo/<role>` (`src/opencode.rs:156-157`), and the preflight stops conflicting user keys. But zygo writes no `enabled_providers` key, although the pinned schema has one (`fixtures/opencode/config-schema.json`). OpenCode's own provider logins (`opencode providers`, alias `auth`, `fixtures/opencode/help.txt:14`) and its other providers stay selectable, so a model request can go to a vendor host.
- **BLOCKED(operator: bar exception opencode A-1.4).** There is no key-helper command to run in any shell (see A-1.2). A core criterion cannot be `n/a` (check item 4), so it is `not-met`.
- **BLOCKED(operator: bar exception opencode A-10.2).** The token is in the client's environment through `OPENCODE_CONFIG_CONTENT` (same evidence as OpenCode A-1.2; base spec §8.4 names it as an "Accepted limit").

For each line, the operator's decision is one of three:

1. **Approve an exception.** Step 8 adds a `[[bar_exception]]` with the reason from the table and the operator's approval text.
2. **Fix the code.** The fix is a new node by §5.4, and the grade stays `not-met` until that node lands.
3. **Change the criterion text.** This is a registry change that the operator decides (K§6.2.1).

The pull request also stops before merge for the operator by K-D26.

### Registry: `docs/conformance/capabilities.toml`

Build it by these rules, with no other key:

- One `[[capability]]` per row of the K§6.2.2 migration table, in table order: A-1 … A-21, WA-1, WA-2. The table is at `docs/specs/2026-11-25-client-model-design.md:229-251`. A-22 is not added (P4-10 adds it).
- `id`: the id in column 1. `name`: the rest of column 1 (`Transport`, …, `Resume`, `Turn signals`). `tier`: column 2.
- `standard`: the Standard cell of the base spec §8.1 table, verbatim (`docs/specs/2026-11-18-zygo-design.md:328-350`). For A-21, cut the cell before ` *(Note`. Backticks stay.
- One `[[capability.criterion]]` per `.<n> ` marker in column 3. `id` = `<cap>.<n>`. `text` = the words from after the marker up to the next marker, trimmed, backticks kept.
- A-9 has six criteria, with this exact text:
  - A-9.1: "The profile shows on the client's status surface, or in `zygo status` when the client has no surface."
  - A-9.2: the same sentence with "The work item shows".
  - A-9.3: the same sentence with "The context use shows".
  - A-9.4: the same sentence with "The compaction override shows".
  - A-9.5: "The session savings show on the client's status surface, or in `zygo status` when the client has no surface."
  - A-9.6: "Each status command that zygo writes runs in each shell the client uses."
- Strings are TOML basic strings, with `"` and `\` escaped.
- One bar exception, from K-D18 and K§6.2.3. The reason holds the "Windows shell for the generated commands" note, in 300 characters or fewer:

```toml
[[bar_exception]]
client = "claude-code"
criterion = "A-1.4"
reason = "Windows without Git Bash runs the key helper in PowerShell, which needs & before a quoted path; zygo writes the double-quoted path, and CI runs the command only in Git Bash"
approved = "operator 2026-11-25, docs/specs/2026-11-25-client-model-design.md K-D18"
```

Expected result: 23 capabilities and 63 criteria. By tier:

| Tier | Capabilities | Criteria |
|---|---|---|
| core | 8: A-1, A-2, A-5, A-6, A-8, A-10, A-13, A-21 | 20 |
| standard | 12 | 33 |
| extra | 3: A-9, A-17, A-18 | 10 |

### Grade files: `docs/conformance/claude-code.toml`, `docs/conformance/opencode.toml`

The form is K§6.2.3 and has no other key.

- Header: `client` (`claude-code`, `opencode`), `name` (`Claude Code`, `OpenCode`), `version` (`2.1.328`, `1.18.16`).
- One `[[cell]]` per capability, in registry order. The `url` is today's URL in `docs/conformance.md`. The URLs per client:

| Capability | Claude Code URL | OpenCode URL |
|---|---|---|
| A-1 | https://code.claude.com/docs/en/llm-gateway | https://opencode.ai/docs/providers/ |
| A-2 | https://code.claude.com/docs/en/env-vars | https://opencode.ai/docs/config/ |
| A-3 | https://code.claude.com/docs/en/model-config | https://opencode.ai/docs/models/ |
| A-4 | https://code.claude.com/docs/en/sub-agents | https://opencode.ai/docs/agents/ |
| A-5 | https://code.claude.com/docs/en/cli-reference | https://opencode.ai/docs/config/ |
| A-6 | https://code.claude.com/docs/en/mcp | https://opencode.ai/docs/mcp-servers/ |
| A-7 | https://code.claude.com/docs/en/hooks | https://opencode.ai/docs/plugins/ |
| A-8 | https://code.claude.com/docs/en/hooks | https://opencode.ai/docs/plugins/ |
| A-9 | https://code.claude.com/docs/en/statusline | https://opencode.ai/docs/cli/ |
| A-10 | https://code.claude.com/docs/en/env-vars | https://opencode.ai/docs/cli/ |
| A-11 | https://code.claude.com/docs/en/plugins | https://opencode.ai/docs/skills/ |
| A-12 | https://code.claude.com/docs/en/settings | https://opencode.ai/docs/models/ |
| A-13 | https://code.claude.com/docs/en/cli-reference | https://opencode.ai/docs/cli/ |
| A-14 | https://code.claude.com/docs/en/sub-agents | https://opencode.ai/docs/agents/ |
| A-15 | https://code.claude.com/docs/en/hooks | https://opencode.ai/docs/permissions/ |
| A-16 | https://code.claude.com/docs/en/context-window | https://opencode.ai/docs/config/ |
| A-17 | https://code.claude.com/docs/en/mcp | https://opencode.ai/docs/mcp-servers/ |
| A-18 | https://code.claude.com/docs/en/hooks | https://opencode.ai/docs/plugins/ |
| A-19 | https://code.claude.com/docs/en/hooks | https://opencode.ai/docs/plugins/ |
| A-20 | https://code.claude.com/docs/en/sub-agents | https://opencode.ai/docs/permissions/ |
| A-21 | https://code.claude.com/docs/en/cli-reference | https://opencode.ai/docs/cli/ |
| WA-1 | https://code.claude.com/docs/en/cli-reference | https://opencode.ai/docs/cli/ |
| WA-2 | https://code.claude.com/docs/en/hooks | https://opencode.ai/docs/plugins/ |

- One `[[grade]]` per criterion, in registry order, with the `status` from the table below.
  - A `met` grade has `evidence`: the listed strings, in the listed order.
  - A `met` grade has a `reason` only where the table gives one. That reason keeps the "Documentation caveat (OpenCode)" note of today's file. The generator does not show it; it stays in the grade file.
  - A `not-met` or `n/a` grade has the listed `reason` and no `evidence`.

### Grading table

CC = Claude Code, OC = OpenCode.

Each `met` item was checked by the trial run with the D1 model change: `resolve` gave each item `Ok`, and `check` gave no fault other than item 8 for A-1 and A-10 of both clients (see "Stops"). Each cited test function was opened: none is `#[ignore]`, and each is in a `crates/<crate>/tests/*.rs` target or in the `src` module tree. Each `premise:` row is `verified` (`docs/premises.md`) or `pass` (`docs/live-checks.md`), and its Agent cell names the client.

Tests under a `cfg` show their platforms in the generated document:

- `claude_code.rs::alias_of_a_disabled_role_picks_the_redirect_role`, `ws.rs::saved_config_dir_replaces_or_removes_the_variable` and the `crates/zygo-hooks/tests/workspace.rs` tests (`#![cfg(unix)]`) show `linux, macos`.
- `crates/zygo/src/cmd/compact.rs::sets_and_clears_on_a_live_session` and the `crates/zygo/src/cmd/status.rs` tests are unit tests in the `src` tree.

| Criterion | CC | CC evidence (met) or reason | OC | OC evidence (met) or reason |
|---|---|---|---|---|
| A-1.1 | met | `test:crates/zygo-launch/tests/claude_code.rs::settings_env_agents_and_mcp_match_fixtures`, `test:crates/zygo-launch/tests/stub_launch.rs::claude_code_flags_settings_agents_and_mcp_match_the_spec` | met | `test:crates/zygo-launch/tests/opencode.rs::inline_config_values`, `premise:docs/premises.md#P4` |
| A-1.2 | not-met | In passthrough (base spec 12.3) the session token is also in the ANTHROPIC_CUSTOM_HEADERS environment row as X-Zygo-Token (stub_launch.rs claude_code_session_token_stays_in_the_token_file...); the criterion allows only the token file and the 0600 passthrough files | not-met | OpenCode has no key helper: zygo writes the session token as provider.zygo.options.apiKey in the OPENCODE_CONFIG_CONTENT environment row (base spec 8.7; opencode.rs launch_sets_the_opencode_environment), so the token is in an environment row |
| A-1.3 | met | `test:crates/zygo-launch/tests/claude_code.rs::settings_env_agents_and_mcp_match_fixtures`, `test:crates/zygo-launch/tests/claude_code.rs::preflight_names_the_file_and_key`, `test:crates/zygo-launch/tests/launcher.rs::denylist_removes_provider_and_routing_names` | not-met | zygo sets model and small_model to zygo roles and stops on conflicting user keys, but writes no enabled_providers: OpenCode's own provider logins (opencode providers) and other providers stay selectable, so a model request can go to a vendor host |
| A-1.4 | not-met | Windows without Git Bash runs the apiKeyHelper command in PowerShell, which needs & before a quoted path; zygo writes the double-quoted path, and CI runs the command only in Git Bash (stub_launch.rs every_command_zygo_writes_for_claude_code_runs_in_the_agents_shell) | not-met | OpenCode has no key-helper command: zygo writes the token inline as provider.zygo.options.apiKey, so no key-helper command runs in any shell; a core criterion cannot be n/a |
| A-2.1 | met | `test:crates/zygo-launch/tests/claude_code.rs::settings_env_agents_and_mcp_match_fixtures` | met | `test:crates/zygo-launch/tests/opencode.rs::inline_config_values`, `premise:docs/premises.md#P4` |
| A-2.2 | met | `test:crates/zygo-launch/tests/claude_code.rs::settings_env_agents_and_mcp_match_fixtures`, `test:crates/zygo-launch/tests/claude_code.rs::agent_version_line_is_left_out_when_empty` | met | `test:crates/zygo-launch/tests/opencode.rs::inline_config_values`, `premise:docs/premises.md#P4` |
| A-2.3 | met | `test:crates/zygo-launch/tests/claude_code.rs::settings_env_agents_and_mcp_match_fixtures` | met | `test:crates/zygo-launch/tests/opencode.rs::inline_config_values` |
| A-2.4 | met | `test:crates/zygo-launch/tests/claude_code.rs::settings_env_agents_and_mcp_match_fixtures` | met | `test:crates/zygo-launch/tests/opencode.rs::inline_config_values` |
| A-3.1 | met | `test:crates/zygo-launch/tests/claude_code.rs::settings_env_agents_and_mcp_match_fixtures`, `premise:docs/premises.md#Q3`, `premise:docs/premises.md#P2` | met | `test:crates/zygo-launch/tests/opencode.rs::inline_config_values`, `premise:docs/premises.md#P4` |
| A-3.2 | met | `test:crates/zygo-launch/tests/claude_code.rs::settings_env_agents_and_mcp_match_fixtures`, `test:crates/zygo-launch/tests/claude_code.rs::disabled_alias_targets_are_redirected` | met | `test:crates/zygo-launch/tests/opencode.rs::inline_config_values` |
| A-3.3 | met | `test:crates/zygo-launch/tests/claude_code.rs::disabled_alias_targets_are_redirected`, `test:crates/zygo-launch/tests/claude_code.rs::settings_env_agents_and_mcp_match_fixtures` | n/a | OpenCode has no model aliases: its built-in agents are the map (base spec 8.7, no alias sentence); small_model is set to a role |
| A-4.1 | met | `test:crates/zygo-launch/tests/claude_code.rs::disabled_alias_targets_are_redirected`, `test:crates/zygo-launch/tests/claude_code.rs::settings_env_agents_and_mcp_match_fixtures` | met | `test:crates/zygo-launch/tests/opencode.rs::agents_map_to_roles`, `test:crates/zygo-launch/tests/opencode.rs::small_model_follows_read`, `premise:docs/premises.md#P5` |
| A-4.2 | met | `test:crates/zygo-launch/tests/claude_code.rs::settings_env_agents_and_mcp_match_fixtures` | met | `test:crates/zygo-launch/tests/opencode.rs::agents_map_to_roles`, `premise:docs/premises.md#P5` |
| A-5.1 | met | `test:crates/zygo-launch/tests/stub_launch.rs::claude_code_flags_settings_agents_and_mcp_match_the_spec`, `test:crates/zygo-launch/tests/claude_code.rs::settings_env_agents_and_mcp_match_fixtures` | met | `test:crates/zygo-launch/tests/opencode.rs::env_rows_and_args`, `test:crates/zygo-launch/tests/opencode.rs::launch_sets_the_opencode_environment`, `premise:docs/premises.md#P5`, `premise:docs/premises.md#Q2`; reason: The live OpenCode docs describe a newer plugin API than 1.18.16 and do not name OPENCODE_CONFIG_CONTENT or OPENCODE_CONFIG_DIR; the grade rests on the cited premise, the fixtures and fixtures/opencode/binary-names.txt |
| A-5.2 | met | `test:crates/zygo-launch/tests/stub_launch.rs::launch_leaves_the_users_own_files_untouched` | met | `test:crates/zygo-launch/tests/stub_launch.rs::launch_leaves_the_users_own_files_untouched` |
| A-5.3 | met | `test:crates/zygo-launch/tests/claude_code.rs::preflight_names_the_file_and_key`, `test:crates/zygo-launch/tests/claude_code.rs::unreadable_user_settings_keep_only_zygos_file`, `test:crates/zygo-launch/tests/stub_launch.rs::claude_code_flags_settings_agents_and_mcp_match_the_spec` | met | `premise:docs/premises.md#P4`, `test:crates/zygo-launch/tests/opencode.rs::preflight_names_the_file_and_key`, `test:crates/zygo-launch/tests/opencode.rs::a_project_config_that_defines_provider_zygo_stops_the_launch` |
| A-6.1 | met | `test:crates/zygo-launch/tests/stub_launch.rs::claude_code_flags_settings_agents_and_mcp_match_the_spec`, `test:crates/zygo-launch/tests/stub_launch.rs::every_command_zygo_writes_for_claude_code_runs_in_the_agents_shell`, `test:crates/zygo/tests/mcp_stdio.rs::claude_code_binary_serves_compact_and_exits_zero_on_eof` | met | `test:crates/zygo-launch/tests/opencode.rs::inline_config_values`, `premise:docs/premises.md#P5` |
| A-7.1 | met | `test:crates/zygo-hooks/tests/hooks_basic.rs::trim_bash_100kib_returns_view_and_stores_bytes`, `premise:docs/premises.md#Q4` | met | `test:crates/zygo-launch/tests/opencode.rs::hooks_on_fixtures_give_the_specified_output`, `test:crates/zygo-launch/tests/opencode.rs::normalize_after_bash_fixture`, `premise:docs/premises.md#Q2`; reason: The live OpenCode docs describe a newer plugin API than 1.18.16 and do not name the tool.execute.after output assignment; the grade rests on the cited premise, the fixtures and fixtures/opencode/binary-names.txt |
| A-7.2 | met | `test:crates/zygo-hooks/tests/hooks_basic.rs::trim_read_text_view_has_no_tail`, `premise:docs/premises.md#Q4` | not-met | The plugin trims bash output only: hook_opencode::normalize gives a tool_response only for bash, so read output reaches the model whole |
| A-7.3 | met | `test:crates/zygo-hooks/tests/hooks_basic.rs::trim_grep_content_mode`, `test:crates/zygo-hooks/tests/hooks_basic.rs::trim_glob_filenames`, `premise:docs/premises.md#Q4` | not-met | The plugin trims bash output only: hook_opencode::normalize gives a tool_response only for bash, so grep, glob and list output reaches the model whole |
| A-7.4 | met | `test:crates/zygo-hooks/tests/hooks_basic.rs::trim_webfetch_result`, `premise:docs/premises.md#Q4` | not-met | OpenCode has a webfetch tool (live check OC-SHAPES), but hook_opencode::normalize gives a tool_response only for bash, so webfetch output is never trimmed |
| A-7.5 | met | `test:crates/zygo-hooks/tests/hooks_basic.rs::trim_other_events_pass`, `premise:docs/premises.md#Q4` | met | `test:crates/zygo-launch/tests/opencode.rs::plugin_text_matches_the_fixtures`, `fixture:fixtures/opencode/plugin-hooks/tool.execute.after-bash.json`, `premise:docs/premises.md#Q2` |
| A-8.1 | met | `test:crates/zygo-hooks/tests/session_text.rs::session_start_json_envelope_claude_code`, `test:crates/zygo-launch/tests/stub_launch.rs::claude_code_flags_settings_agents_and_mcp_match_the_spec`, `premise:docs/premises.md#PW10`, `premise:docs/premises.md#PW2`, `fixture:fixtures/claude-code/hooks/SessionStart-main.json` | met | `test:crates/zygo-hooks/tests/session_text.rs::session_start_text_for_opencode`, `test:crates/zygo-launch/tests/opencode.rs::plugin_text_matches_the_fixtures`, `premise:docs/premises.md#Q2`; reason: The live OpenCode docs describe a newer plugin API than 1.18.16 and do not name experimental.chat.system.transform; the grade rests on the cited premise, the fixtures and fixtures/opencode/binary-names.txt |
| A-8.2 | met | `test:crates/zygo-launch/tests/claude_code.rs::settings_env_agents_and_mcp_match_fixtures` | met | `test:crates/zygo-launch/tests/opencode.rs::agents_map_to_roles`, `premise:docs/premises.md#P5` |
| A-9.1 | met | `test:crates/zygo-hooks/tests/statusline_render.rs::worktree_git_marks_model_profile`, `test:crates/zygo-launch/tests/stub_launch.rs::claude_code_flags_settings_agents_and_mcp_match_the_spec` | met | `test:crates/zygo/src/cmd/status.rs::inside_a_session`, `test:crates/zygo/tests/serve_cli.rs::status_and_help_escape_the_profile_name` |
| A-9.2 | met | `test:crates/zygo-hooks/tests/statusline_render.rs::work_item_markers`, `test:crates/zygo-launch/tests/stub_launch.rs::claude_code_flags_settings_agents_and_mcp_match_the_spec` | met | `test:crates/zygo/src/cmd/status.rs::inside_a_session` |
| A-9.3 | met | `test:crates/zygo-hooks/tests/statusline_render.rs::h14_08_ctx_numbers_bar_and_threshold_mark`, `test:crates/zygo-launch/tests/stub_launch.rs::claude_code_flags_settings_agents_and_mcp_match_the_spec` | met | `test:crates/zygo/src/cmd/status.rs::inside_a_session` |
| A-9.4 | met | `test:crates/zygo-hooks/tests/statusline_render.rs::h14_08_ctx_numbers_bar_and_threshold_mark`, `test:crates/zygo-launch/tests/stub_launch.rs::claude_code_flags_settings_agents_and_mcp_match_the_spec` | met | `test:crates/zygo/src/cmd/status.rs::inside_a_session` |
| A-9.5 | met | `test:crates/zygo-hooks/tests/statusline_render.rs::tokens_spend_cache_trimmed`, `test:crates/zygo-hooks/tests/statusline_render.rs::h14_18_trimmed_bytes_show_as_estimated_tokens`, `test:crates/zygo-launch/tests/stub_launch.rs::claude_code_flags_settings_agents_and_mcp_match_the_spec` | met | `test:crates/zygo/src/cmd/status.rs::inside_a_session`, `test:crates/zygo/src/cmd/status.rs::h14_18_trimmed_in_tokens_and_the_spend_line` |
| A-9.6 | not-met | Windows without Git Bash runs the statusLine command in PowerShell, which needs & before a quoted path; zygo writes the double-quoted path, and CI runs the command only in Git Bash | n/a | zygo writes no status command for OpenCode: the OpenCode launch has no status-line setting, and the A-9 values show in zygo status |
| A-10.1 | met | `test:crates/zygo-launch/tests/launcher.rs::denylist_removes_provider_and_routing_names`, `test:crates/zygo-launch/tests/launcher.rs::denylist_removes_the_claude_code_markers_and_the_member_names`, `test:crates/zygo-launch/tests/launcher.rs::denylist_folds_case_with_windows_rules`, `test:crates/zygo-launch/tests/stub_launch.rs::non_member_child_has_neither_workspace_name_nor_a_marker` | met | `test:crates/zygo-launch/tests/launcher.rs::denylist_removes_provider_and_routing_names`, `test:crates/zygo-launch/tests/launcher.rs::denylist_folds_case_with_windows_rules`, `test:crates/zygo-launch/tests/opencode.rs::launch_sets_the_opencode_environment` |
| A-10.2 | not-met | In passthrough (base spec 12.3) the session token is in the client's environment: ANTHROPIC_CUSTOM_HEADERS holds X-Zygo-Token (stub_launch.rs claude_code_session_token_stays_in_the_token_file...); outside passthrough it is not | not-met | The session token is in the client's environment: OPENCODE_CONFIG_CONTENT holds it as provider.zygo.options.apiKey (base spec 8.4 accepted limit, 8.7; opencode.rs launch_sets_the_opencode_environment) |
| A-11.1 | met | `test:crates/zygo-launch/tests/premise.rs::claude_code_plugin_snapshot_loads_plugin_colon_skill`, `premise:docs/live-checks.md#40` | met | `test:crates/zygo-launch/tests/opencode.rs::delivery_writes_the_opencode_forms`, `test:crates/zygo-launch/tests/opencode.rs::embedded_skill_is_delivered_always`, `premise:docs/premises.md#P5` |
| A-11.2 | met | `test:crates/zygo-plugins/tests/snapshot.rs::claude_snapshot_has_plugin_dirs_and_the_embedded_plugin`, `test:crates/zygo-launch/tests/claude_code.rs::settings_env_agents_and_mcp_match_fixtures` | met | `test:crates/zygo-launch/tests/opencode.rs::delivery_writes_the_opencode_forms`, `premise:docs/premises.md#P5` |
| A-12.1 | met | `test:crates/zygo-launch/tests/claude_code.rs::alias_of_a_disabled_role_picks_the_redirect_role`, `test:crates/zygo-proxy/tests/request_path.rs::route_profile_override_serves`, `test:crates/zygo-proxy/tests/session_api.rs::profile_applied_to_session_config` | met | `test:crates/zygo-proxy/tests/session_api.rs::profile_applied_to_session_config`, `test:crates/zygo-launch/tests/opencode.rs::agents_map_to_roles`, `premise:docs/premises.md#P4` |
| A-13.1 | met | `test:crates/zygo-config/tests/builtin.rs::minimum_agent_versions_match_premises` | met | `test:crates/zygo-config/tests/builtin.rs::minimum_agent_versions_match_premises` |
| A-13.2 | met | `test:crates/zygo/tests/usage_doctor.rs::doctor_agents_versions`, `test:crates/zygo/tests/usage_doctor.rs::doctor_agents_below_minimum_missing_unreadable` | met | `test:crates/zygo/tests/usage_doctor.rs::doctor_agents_versions`, `test:crates/zygo/tests/usage_doctor.rs::doctor_agents_below_minimum_missing_unreadable` |
| A-14.1 | met | `test:crates/zygo-hooks/tests/session_text.rs::session_text_section_order`, `test:crates/zygo-hooks/tests/session_text.rs::session_start_json_envelope_claude_code` | met | `test:crates/zygo-hooks/tests/session_text.rs::session_start_text_for_opencode`, `test:crates/zygo-launch/tests/opencode.rs::plugin_text_matches_the_fixtures`, `premise:docs/premises.md#Q2` |
| A-14.2 | met | `test:crates/zygo-launch/tests/claude_code.rs::settings_env_agents_and_mcp_match_fixtures` | met | `test:crates/zygo-launch/tests/opencode.rs::agents_map_to_roles`, `premise:docs/premises.md#P5` |
| A-15.1 | met | `test:crates/zygo-hooks/tests/hooks_basic.rs::lead_main_edit_write_notebook_denied`, `test:crates/zygo-launch/tests/claude_code.rs::role_guard_follows_the_effective_role`, `fixture:fixtures/claude-code/hooks/PreToolUse-Edit-main.json` | met | `test:crates/zygo-launch/tests/opencode.rs::lead_guard_and_default_agent_follow_main_role`, `premise:docs/premises.md#Q2` |
| A-15.2 | met | `test:crates/zygo-hooks/tests/hooks_basic.rs::lead_subagent_gets_no_output`, `fixture:fixtures/claude-code/hooks/PreToolUse-Edit-sub.json` | met | `test:crates/zygo-launch/tests/opencode.rs::agents_map_to_roles`, `test:crates/zygo-launch/tests/opencode.rs::lead_guard_and_default_agent_follow_main_role` |
| A-16.1 | met | `test:crates/zygo-proxy/tests/window.rs::soft_end_turn_main_rewritten`, `premise:docs/premises.md#P1` | met | `test:crates/zygo-launch/tests/opencode.rs::model_limits`, `test:crates/zygo-launch/tests/opencode.rs::compact_flag_sets_the_window`, `premise:docs/premises.md#Q2` |
| A-16.2 | met | `test:crates/zygo/src/cmd/compact.rs::sets_and_clears_on_a_live_session`, `test:crates/zygo-proxy/tests/window.rs::soft_override_and_off` | not-met | OpenCode reads limit.context once at start and gets no proxy compaction signal, so a window change needs a relaunch with zygo opencode --compact <n> |
| A-16.3 | met | `test:crates/zygo/src/cmd/compact.rs::sets_and_clears_on_a_live_session` | not-met | zygo compact refuses in an OpenCode session (exit 2): OpenCode reads its window at start; relaunch with zygo opencode --compact <n> |
| A-17.1 | met | `test:crates/zygo-mcp/tests/mcp_server.rs::compact_listed_for_claude_code_and_returns_compaction_requested`, `test:crates/zygo-launch/tests/stub_launch.rs::claude_code_flags_settings_agents_and_mcp_match_the_spec` | not-met | zygo mcp lists the compact tool for Claude Code only (base spec 10.3); the OpenCode tool list has no compact tool |
| A-17.2 | met | `test:crates/zygo-proxy/tests/window.rs::compact_call_rewritten_any_size`, `premise:docs/premises.md#P1` | not-met | The proxy SSE rewrite that turns a compact tool call into a compaction is not armed for OpenCode in v1 (base spec 10.2.5), and the tool is not listed |
| A-18.1 | met | `test:crates/zygo-launch/tests/stub_launch.rs::claude_code_flags_settings_agents_and_mcp_match_the_spec`, `test:crates/zygo-hooks/tests/session_text.rs::pre_compact_without_open_unit`, `fixture:fixtures/claude-code/hooks/PreCompact-main.json` | met | `test:crates/zygo-launch/tests/opencode.rs::plugin_text_matches_the_fixtures`, `premise:docs/premises.md#Q2`, `fixture:fixtures/opencode/plugin-hooks/experimental.session.compacting.json`; reason: The live OpenCode docs describe a newer plugin API than 1.18.16 and do not name experimental.session.compacting; the grade rests on the cited premise, the fixtures and fixtures/opencode/binary-names.txt |
| A-18.2 | met | `test:crates/zygo-hooks/tests/session_text.rs::pre_compact_one_open_unit`, `test:crates/zygo-hooks/tests/session_text.rs::pre_compact_two_units_and_stopped_unit` | met | `test:crates/zygo-hooks/tests/session_text.rs::pre_compact_one_open_unit`, `test:crates/zygo-launch/tests/opencode.rs::plugin_text_matches_the_fixtures` |
| A-19.1 | met | `test:crates/zygo/tests/work_hook.rs::binary_records_begin_call_and_stop`, `test:crates/zygo-work/tests/collection.rs::begin_writes_line_and_begin_json`, `test:crates/zygo-work/tests/collection.rs::begin_lead_uses_main`, `premise:docs/premises.md#P3` | not-met | work_begin is recorded only by the work hook with --event begin, which zygo registers for Claude Code only; OpenCode hook input carries no agent id, so no unit is recorded per agent |
| A-19.2 | met | `test:crates/zygo-work/tests/collection.rs::post_every_call_appends_call_or_failed`, `test:crates/zygo/tests/work_hook.rs::binary_records_begin_call_and_stop` | met | `test:crates/zygo-launch/tests/opencode.rs::plugin_text_matches_the_fixtures`, `test:crates/zygo-launch/tests/opencode.rs::normalize_after_bash_fixture`, `test:crates/zygo-work/tests/collection.rs::post_every_call_appends_call_or_failed` |
| A-19.3 | met | `test:crates/zygo-work/tests/guards.rs::claims_continues_once`, `premise:docs/premises.md#Q1` | not-met | The pinned OpenCode plugin API has no subagent-stop event (only tool.execute hooks in fixtures/opencode/plugin-hooks/), so the claims guard never runs |
| A-19.4 | met | `test:crates/zygo/tests/guard_hook.rs::binary_guard_denies_claude_attribution_in_a_pr_body` | met | `test:crates/zygo-launch/tests/opencode.rs::hooks_on_fixtures_give_the_specified_output`, `premise:docs/live-checks.md#OC-THROW` |
| A-20.1 | met | `test:crates/zygo-launch/tests/stub_launch.rs::claude_code_flags_settings_agents_and_mcp_match_the_spec`, `premise:docs/live-checks.md#A-20`, `fixture:fixtures/claude-code/live/a20-tools.json` | met | `test:crates/zygo-launch/tests/opencode.rs::agents_map_to_roles`, `premise:docs/premises.md#Q2` |
| A-20.2 | met | `test:crates/zygo-launch/tests/claude_code.rs::turn_cap_sets_max_turns` | not-met | limits.subagent_max_turns reaches only Claude Code (maxTurns in claude_code.rs); zygo writes no OpenCode agent steps key, though OpenCode has one (Q2) |
| A-21.1 | met | `test:crates/zygo-launch/tests/stub_launch.rs::claude_code_flags_settings_agents_and_mcp_match_the_spec`, `test:crates/zygo-launch/tests/stub_launch.rs::claude_code_session_token_stays_in_the_token_file_and_in_passthrough_in_settings_and_custom_headers`, `test:crates/zygo-launch/tests/stub_launch.rs::non_member_child_has_neither_workspace_name_nor_a_marker`, `test:crates/zygo-launch/tests/stub_launch.rs::every_command_zygo_writes_for_claude_code_runs_in_the_agents_shell` | met | `test:crates/zygo-launch/tests/opencode.rs::launch_sets_the_opencode_environment`, `test:crates/zygo-launch/tests/premise.rs::opencode_inline_config_keys_flags_and_env_are_in_the_pinned_schema`, `test:crates/zygo-launch/tests/stub_launch.rs::launch_leaves_the_users_own_files_untouched` |
| A-21.2 | met | `test:crates/zygo-launch/tests/premise.rs::claude_code_flags_are_in_the_pinned_help`, `test:crates/zygo-launch/tests/premise.rs::claude_code_setting_keys_are_in_the_pinned_schema`, `test:crates/zygo-launch/tests/premise.rs::claude_code_env_rows_and_agent_keys_have_evidence` | met | `test:crates/zygo-launch/tests/premise.rs::opencode_inline_config_keys_flags_and_env_are_in_the_pinned_schema` |
| WA-1.1 | met | `test:crates/zygo-launch/tests/stub_launch.rs::stub_reads_the_one_token_session_args_of_a_claude_code_launch`, `test:crates/zygo-launch/tests/claude_code.rs::session_args_follow_w4_and_the_pinned_help`, `premise:docs/premises.md#PW1`, `premise:docs/premises.md#PW2` | met | `test:crates/zygo-launch/tests/opencode.rs::plugin_workspace_events_match_pw4_fixtures`, `premise:docs/premises.md#PW4`, `fixture:fixtures/opencode/workspace/pw4.txt` |
| WA-1.2 | met | `premise:docs/premises.md#PW1`, `premise:docs/premises.md#PW3`, `premise:docs/premises.md#PW10`, `test:crates/zygo-launch/tests/stub_launch.rs::stub_reads_the_one_token_session_args_of_a_claude_code_launch`, `test:crates/zygo/src/cmd/ws.rs::saved_config_dir_replaces_or_removes_the_variable` | met | `test:crates/zygo-launch/tests/opencode.rs::session_args_follow_w4_and_the_pinned_help`, `premise:docs/premises.md#PW4` |
| WA-2.1 | met | `test:crates/zygo-hooks/tests/workspace.rs::claude_code_prompt_entry_sets_busy_and_blocks_while_stopping`, `premise:docs/premises.md#PW5`, `fixture:fixtures/claude-code/workspace/UserPromptSubmit-blocked.json` | met | `test:crates/zygo-hooks/tests/workspace.rs::opencode_turn_signals`, `test:crates/zygo-launch/tests/opencode.rs::plugin_workspace_events_match_pw4_fixtures`, `premise:docs/premises.md#PW4` |
| WA-2.2 | not-met | A turn ended by Esc sends no Stop or StopFailure (PW7 sub-checks 7.2 and 7.3, a failed premise); zygo falls back to the AU26 idle rule | not-met | No evidence that a turn the user interrupts gives session.idle or a status idle; PW4 sub-check 4.3 covers only a turn that ends by itself |
| WA-2.3 | not-met | Internal subagents send SubagentStop with no SubagentStart (PW7 sub-check 7.5, a failed premise), so a subagent start is not seen for each subagent | not-met | The pinned OpenCode plugin API has no subagent start event; the plugin sees only the task tool call (workspaces design AU27) |
| WA-2.4 | met | `test:crates/zygo-hooks/tests/workspace.rs::claude_code_events_change_busy_and_the_tool_count`, `premise:docs/premises.md#Q1`, `fixture:fixtures/claude-code/workspace/SubagentStop-last-dispatched.json` | not-met | The pinned OpenCode plugin API has no subagent end event; a child session idle is not a subagent stop signal (workspaces design AU27) |
| WA-2.5 | met | `premise:docs/premises.md#PW5`, `fixture:fixtures/claude-code/workspace/PreToolUse-Agent-denied.json` | met | `premise:docs/premises.md#PW4`, `premise:docs/live-checks.md#OC-THROW`, `fixture:fixtures/opencode/workspace/tool.execute.before-task.json`, `test:crates/zygo-launch/tests/opencode.rs::plugin_workspace_events_match_pw4_fixtures` |

### Derived result (K§6.2.4)

Cells from the table above. "Missing" lists the criteria that are not met, and "n/a" lists the criteria graded `n/a`.

**Claude Code** (all other cells: `full`):

| Capability | Tier | Cell | Met / applicable | Missing |
|---|---|---|---|---|
| A-1 | core | partial | 2 / 4 | A-1.2, A-1.4 |
| A-9 | extra | partial | 5 / 6 | A-9.6 |
| A-10 | core | partial | 1 / 2 | A-10.2 |
| WA-2 | standard | partial | 3 / 5 | WA-2.2, WA-2.3 |

**OpenCode** (all other cells: `full`):

| Capability | Tier | Cell | Met / applicable | Missing or n/a |
|---|---|---|---|---|
| A-1 | core | partial | 1 / 4 | missing A-1.2, A-1.3, A-1.4 |
| A-3 | standard | full | 2 / 2 | n/a A-3.3 |
| A-7 | standard | partial | 2 / 5 | missing A-7.2, A-7.3, A-7.4 |
| A-9 | extra | full | 5 / 5 | n/a A-9.6 |
| A-10 | core | partial | 1 / 2 | missing A-10.2 |
| A-16 | standard | partial | 1 / 3 | missing A-16.2, A-16.3 |
| A-17 | extra | none | 0 / 2 | missing A-17.1, A-17.2 |
| A-19 | standard | partial | 2 / 4 | missing A-19.1, A-19.3 |
| A-20 | standard | partial | 1 / 2 | missing A-20.2 |
| WA-2 | standard | partial | 2 / 5 | missing WA-2.2, WA-2.3, WA-2.4 |

Scores (K§6.2.4, rounded down last; a bar exception does not change a score):

| Client | Total | Core | Standard | Extra |
|---|---|---|---|---|
| Claude Code | 91 | 87 | 96 | 94 |
| OpenCode | 78 | 84 | 76 | 66 |

Bar, as graded:

- Claude Code is below the bar on A-1 (A-1.2) and A-10 (A-10.2). A-1.4 has the approved exception.
- OpenCode is below the bar on A-1 (A-1.2, A-1.3, A-1.4) and A-10 (A-10.2).
- Each other core criterion of both clients is met: A-1.1, A-1.3 (Claude Code), A-2.1 to A-2.4, A-5.1 to A-5.3, A-6.1, A-8.1, A-8.2, A-10.1, A-13.1, A-13.2, A-21.1 and A-21.2.

### Differences from today's hand grades (pull request body, K-D26)

Grade changed:

1. **Claude Code A-10: full → partial.** A-10.2 is not met: in passthrough, `ANTHROPIC_CUSTOM_HEADERS` holds `X-Zygo-Token`, so the session token is in the client's environment. Stop for the operator.
2. **Claude Code WA-1: partial → full.** PW1, PW3 and PW10 and the one-token session arguments meet WA-1.1 and WA-1.2 as written. The PW16 16.2 gap (another `CLAUDE_CONFIG_DIR`) is closed by the AU61 saved config folder, which `ws.rs::saved_config_dir_replaces_or_removes_the_variable` tests. That gap is not part of the criterion text.
3. **OpenCode A-1: full → partial.** A-1.2, A-1.3 and A-1.4 are not met:
   - the token is inline in `OPENCODE_CONFIG_CONTENT`, with no key helper;
   - zygo writes no `enabled_providers`, so other providers stay selectable;
   - there is no key-helper command.

   Stop for the operator.
4. **OpenCode A-7: full → partial.** A-7.2, A-7.3 and A-7.4 are not met. `hook_opencode::normalize` gives a `tool_response` only for `bash`, so read, grep, glob, list and webfetch output reaches the model whole.
5. **OpenCode A-10: full → partial.** A-10.2 is not met: the token is in `OPENCODE_CONFIG_CONTENT`. Stop for the operator.
6. **OpenCode A-20: full → partial.** A-20.2 is not met. zygo writes a turn cap (`maxTurns`) for Claude Code only. It writes no OpenCode `steps` key, although Q2 shows that OpenCode has one.
7. **OpenCode WA-1: partial → full.** PW4 4.1, 4.2 and 4.8 meet WA-1.1 and WA-1.2 as written. The TUI-switch limit (AU27 known limit 1) is after the first turn, which the criterion text does not cover.

Same grade, changed content:

8. **Claude Code A-1 (partial).** The missing list was "PowerShell" (A-1.4) only. It now also has A-1.2 (the passthrough environment row). Stop for the operator.
9. **OpenCode WA-2 (partial).** WA-2.2 is newly in the missing list: no evidence shows that an interrupted OpenCode turn sends idle. PW4 4.3 covers only a turn that ends by itself.
10. **OpenCode A-3 and A-9 (full).** They stay full with one criterion each graded `n/a`:
    - A-3.3: OpenCode has no model aliases.
    - A-9.6: zygo writes no status command for OpenCode.

Unchanged cells:

- Claude Code: A-9 partial (A-9.6, the same PowerShell gap); WA-2 partial (WA-2.2, WA-2.3).
- OpenCode: A-16 partial; A-17 none; A-19 partial.
- Every other cell of both clients stays full.

### Judgment calls (pull request body)

Each line names the reading used. Line 1 drives two stops for the operator. The other lines do not change the bar.

1. **Passthrough, Claude Code A-1.2 and A-10.2.** Graded strictly: an environment row is not a "file". The other reading is a change of the criterion text, which is the operator's decision.
2. **A-4.2, both clients: met.** "One `zygo-<role>` subagent per enabled role" is read with base spec §8.5 and §8.7, which write no `zygo-<main_role>` agent because the main role is the main loop. Read strictly, A-4 is partial for both clients.
3. **Claude Code A-5.3: met.** "User or project file" is read as Claude Code's user and project settings scopes. A `--settings` value that the user passes with `disableAllHooks: true` wins by design (FIX-21, `claude_code.rs::user_disable_all_hooks_wins_and_attribution_keys_stay`), and zygo prints a line for it.
4. **Claude Code A-18.1: met.** Three things are shown: zygo registers `PreCompact`, the hook prints the keep/drop text, and Claude Code fires `PreCompact` (fixture). No `pass` live check shows the text inside the compaction request: live check 21 is "not run". Read strictly, A-18.1 is not met and A-18 is partial (extra tier).
5. **Claude Code A-21.1: met.** The tests that are not ignored assert the flags, the generated settings, the environment mirror of the settings file, the removed names and the token's absence. The test that asserts each environment value is ignored for timing (D3).
6. **OpenCode A-15.1: met.** With the default `main_role` `lead`, `build` gets `permission.edit: deny`. When `main_role` maps to no primary OpenCode agent, the guard is off and the launch logs it. `opencode.rs::lead_guard_and_default_agent_follow_main_role` asserts that case.
7. **OpenCode A-12.1: met, on indirect evidence.** The profile is applied in the session config, which the inline config is built from. No OpenCode launch test passes `--profile`.

### References to the old scripts and to `docs/conformance.md`

Found with `git grep -n -e check-conformance -e test-check-conformance` and `git grep -n 'conformance\.md'` at `16481ba8`:

| Reference | Handling |
|---|---|
| `scripts/check-conformance.sh` and `scripts/test-check-conformance.sh` (each runs or names the other) | Both deleted in step 11. |
| `docs/conformance.md:3` ("Checked by `scripts/check-conformance.sh`") | Replaced by the generated file in step 8. |
| `.github/workflows/*` | No reference: the scripts run in no CI job (K-D10). |
| `docs/specs/2026-11-25-client-model-design.md:54, :321, :685, :770` | Unchanged. This is the spec that orders the deletion. |
| `docs/work/phase2/history.md:44, :59`, `docs/work/phase2/nodes/L2.md`, `D1.md`, `PRB-2.md` | Unchanged. These are records of closed nodes. |
| `docs/work/phase4/nodes/PC-1.md:11` | Unchanged. It already says that CF-4 deletes the script. |
| `scripts/check-premises.sh:146` greps `docs/conformance.md` for lines that name `N38` | No generated text names `N38`. Step 13 runs `sh scripts/check-premises.sh`. |
| `scripts/premises/ver_test.sh:179-188` and `scripts/premises/live-selftest.sh:242` copy `docs/conformance.md` | Unchanged. The file still exists. |
| `crates/zygo-launch/tests/stub_agent/harness.rs:21` (comment: "versions pinned in `docs/conformance.md`") | Unchanged. The rollup headings of the generated file hold `Claude Code 2.1.328` and `OpenCode 1.18.16`. |
| `docs/roadmap/phase-5.md:77` cites `docs/conformance.md:31`, `:34` (the old evidence lines) | The cited lines move. The file is outside Files, so this is reported to the lead and not changed here. |

### Steps

Each commit message ends with the two trailers `Zygo-Session: <the implementer's session id>` and `Zygo-Work: phase4:CF-4`, each on its own line, and no other attribution.

- [x] 1. **Test first: `conformance_tree`.** Append to `crates/zygo-core/tests/conformance.rs`, with the doc line `/// K§6.2.5 on the repository: check items 1-9 on \`docs/conformance/\`, then the generator and item 10.` The test does this:
  - `root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")`;
  - `reg = Registry::load(&root.join("docs/conformance/capabilities.toml"))`, which panics with `capabilities.toml: {e}` on an error;
  - `grades` = `GradeFile::load` of each `*.toml` in `docs/conformance/` except `capabilities.toml`, sorted by path, so that a grade file of an unknown client meets item 1; it panics with `{path}: {e}` on an error;
  - `clients = clients(&root).unwrap()`;
  - `resolved = resolve(&root, &grades)`;
  - `faults = check(&root, &reg, &grades, &clients, &resolved)`;
  - `assert!(faults.is_empty(), "{}", …)`, where the message has one line `item {item}: {text}` per fault;
  - `md = render::markdown(&reg, &grades, &clients, &resolved)` and `json = render::json(&reg, &grades, &clients, &resolved)`;
  - `render::compare_or_write(&root, &md, &json)`: on `Err(f)` it panics with `item {f.item}: {f.text}`.

  `Path`, `PathBuf` and `model::*` are already in scope (`conformance.rs:11-13`). Check: `cargo test -p zygo-core --test conformance --locked -- --exact conformance_tree` fails with a panic that starts `capabilities.toml:`.
- [x] 2. **Test first: `check_3_cargo_targets_off_the_walk_accepted`.** Put it in `conformance.rs` after `check_3_cargo_target_settings_refused`. Use `pkg = "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n"` and `repo_with(&[("crates/demo/Cargo.toml", &format!("{pkg}{extra}"))])`, and assert on `evidence(d.path(), "test:crates/demo/tests/ev.rs::plain", "Alpha")`:
  - `is_ok()` for `extra` = `"[[bin]]\nname = \"stub\"\npath = \"tests/helper/stub.rs\"\ntest = false\n"` (the `crates/zygo-launch` form) and for `"[[test]]\nname = \"outside\"\npath = \"../../tests/outside.rs\"\n"` (the `crates/zygo` form);
  - `unwrap_err()` that holds `crates/demo/Cargo.toml`, `unsupported Cargo target setting` and the key, for each of these (extra → key):

    | extra | key |
    |---|---|
    | `[[bin]]` `path = "src/main.rs"`, `test = false` | `path` |
    | `[[bin]]` `path = "tests/ev.rs"`, `test = false` | `path` |
    | `[[bin]]` `path = "../demo/src/main.rs"`, `test = false` | `path` |
    | `[[test]]` `name = "ev"`, `path = "../../tests/ev.rs"` | `[[test]]` |
    | `[[test]]` `name = "x"`, `path = "../demo/tests/x.rs"` | `[[test]]` |
    | `[[test]]` `name = "x"`, `path = "/abs/x.rs"` | `[[test]]` |
    | `[[test]]` `name = "x"` with no path | `[[test]]` |

    Each `[[bin]]` in the table also has `name = "b"`.

  Check: it fails on the `[[bin]]` accepted case with `unsupported Cargo target setting \`path\``.
- [x] 3. **Code, `crates/zygo-core/tests/conformance/model.rs`, `crate_manifest` (D1).**
  - A `[[test]]` array is let through when each entry has a `path` for which `leaves_crate(crate_dir, path)` holds, and a `name` that is no file stem of an entry of `<crate>/tests/`. Else the result is `unsupported("[[test]]")`, as now.
  - A `[[bin]]` with `test = false` and a `path` that is relative, holds no `\`, `:` or `..` component, and is not `walk_root(path)` is skipped by the `[lib]`/`[[bin]]` key loop. Every other `[lib]` or `[[bin]]` keeps today's checks.
  - Add `fn walk_root(p: &str) -> bool`. It is true for a path whose components, with empty and `.` components dropped, are `src/lib.rs`, `src/main.rs`, `src/bin/…`, `tests/<x>` or `tests/<x>/main.rs`. These are the roots of `reach`, `model.rs:686-697`.
  - Add `fn leaves_crate(crate_dir: &str, p: &str) -> bool`. It is false for an absolute path or a path with `\` or `:`. Otherwise it resolves `p` lexically from `crate_dir` (`..` pops a component; popping past the start is false), and it is true when the result is neither `crate_dir` nor under `crate_dir/`.
  - Extend the doc comment of `crate_manifest` by the two let-through cases.
- [x] 4. **Check.**
  - `cargo test -p zygo-core --test conformance --locked -- --exact check_3_cargo_targets_off_the_walk_accepted check_3_cargo_target_settings_refused` gives 2 passed.
  - `cargo test -p zygo-core --test conformance --locked -- --skip conformance_tree` gives 110 passed, 0 failed.

  Commit: `fix(conformance): accept Cargo targets that change no walked test target`.
- [ ] 5. **Registry and grade files.** Write `docs/conformance/capabilities.toml` by "Registry" (with only the A-1.4 bar exception). Write `docs/conformance/claude-code.toml` and `docs/conformance/opencode.toml` by "Grade files" and the "Grading table". Check: `cargo test -p zygo-core --test conformance --locked -- --exact conformance_tree` fails, and its message is exactly these two lines:

  ```
  item 8: client `claude-code` is supported and below the bar: A-1, A-10
  item 8: client `opencode` is supported and below the bar: A-1, A-10
  ```

  Any other fault is a slip in step 5: fix the file to match the table, and never change a grade to clear a fault. Commit: `feat(conformance): capability registry and grade files for Claude Code and OpenCode`.
- [ ] 6. **Stop for the operator (K§6.2.1, K-D18, K-D26).**
  1. Push the branch.
  2. Open the pull request as a draft. Its body holds "Stops for the operator", "Differences from today's hand grades" and "Judgment calls", verbatim. It also says that CI is red on `conformance_tree` item 8 alone until the decisions land.
  3. The controller sets CF-4 to `BLOCKED(operator: bar exception …)`, with one line per stop.
- [ ] 7. **Apply each decision in `docs/conformance/capabilities.toml`.**
  - An approved exception: add one `[[bar_exception]]` after the A-1.4 entry, in the order of "Stops". `client` and `criterion` are the stop's; `reason` is that grade's reason in the table, verbatim; `approved` is the operator's approval text, verbatim (who, date, where it is recorded).
  - A code fix: no registry change here. The grade stays `not-met` until the fix node lands. Then its test becomes the `met` evidence, and the cell changes are added to the pull request body.
  - A changed criterion text: replace that `text`, then grade the criterion again by the new text with evidence by K§6.2.3.

  Check: `cargo test -p zygo-core --test conformance --locked -- --exact conformance_tree` fails with item 10 only (`docs/conformance.md: differs from the generated text …` and `docs/conformance.json: …`). Commit: `feat(conformance): bar exceptions approved by the operator`.
- [ ] 8. **Generate the outputs.**
  1. `ZYGO_WRITE_CONFORMANCE=1 cargo test -p zygo-core --test conformance --locked -- --exact conformance_tree` passes.
  2. `cargo test -p zygo-core --test conformance --locked -- --exact conformance_tree` passes.
  3. A second write run leaves `git diff --exit-code docs/conformance.md docs/conformance.json` at exit 0.

  Check the content:
  - The first line of `docs/conformance.md` is `Generated by crates/zygo-core/tests/conformance.rs (ZYGO_WRITE_CONFORMANCE=1); do not edit: a hand edit fails the check.`
  - `docs/conformance.json` `generated_by` holds the same text.
  - Each rollup shows `- Bar: meets bar` and `- Release: supported`.
  - With the grades of the table unchanged by step 7, the score table rows are `| Claude Code | 91 | 87 | 96 | 94 |` and `| OpenCode | 78 | 84 | 76 | 66 |`.
- [ ] 9. Commit `docs/conformance.md` and `docs/conformance.json`: `docs(conformance): generated feature matrix, rollup and score`.
- [ ] 10. **CI paths, `.github/workflows/ci.yml`.** After line 14 (`- 'docs/specs/2026-11-18-zygo-design.md'` of `push`) and after line 24 (the same line of `pull_request`), insert these four lines, each with 6 spaces of indent:

  ```yaml
        - 'docs/conformance/**'
        - 'docs/conformance.md'
        - 'docs/conformance.json'
        - 'docs/live-checks.md'
  ```

  Check: the last two commands of Acceptance each give 4, and the test with `-eq 4` exits 0.
- [ ] 11. **Delete the old scripts.**
  1. `git grep -n -e check-conformance -e test-check-conformance -- . ':!docs/work' ':!docs/specs'` lists only lines of the two scripts. (`docs/conformance.md:3` is gone after step 8.)
  2. `git rm scripts/check-conformance.sh scripts/test-check-conformance.sh`.

  Check: the same `git grep` prints nothing, and `test ! -e scripts/check-conformance.sh && test ! -e scripts/test-check-conformance.sh` exits 0. Commit the CI paths and the deletion together: `ci: run the conformance check on registry, grade, output and live-check changes; remove the conformance scripts`.
- [ ] 12. **Acceptance.** Run the Acceptance command of the contract; each command exits 0. Then:
  - `git diff --exit-code origin/phase-4 -- crates/zygo-config/tests/builtin.rs` exits 0, so `builtin.rs` is unchanged (K§11 item 7);
  - `cargo test --workspace --locked --no-fail-fast` is green, `forbidden_content` included.
- [ ] 13. **Other readers.** `sh scripts/check-premises.sh` prints `check-premises: ok`.
- [ ] 14. **Deliver.** Push. Move the pull request out of draft. The body keeps the three lists and adds the final scores and bar lines. CI is green on ubuntu, macOS and Windows. The pull request does not merge without the operator (K-D26).

### Author's checks (§3.3)

1. **Spec coverage.**

   | Requirement | Steps |
   |---|---|
   | K§6.2.1 files and owners | 5, 8 |
   | K§6.2.2 migration and tiers | 5 and "Registry" |
   | K§6.2.3 grade form, evidence and A-1.4 exception | 5, the table, "Registry" |
   | K§6.2.5 check on the tree, write mode, script deletion | 1, 8, 11 |
   | K§6.6 owners (the generator is the only writer) | 8 |
   | K§9 P4-2 | all steps |
   | K§11 item 1 on the tree | 1, 8 |
   | K§11 item 3 | 8, 10 |
   | K§11 item 7 | 12 |
   | K-D9, K-D18 (bar and exception) | "Stops", 6, 7 |
   | K-D10 | 11 |
   | K-D26 | "Differences", 6, 14 |
   | Planning notes (hand notes kept in reasons; CI paths; `builtin.rs` unchanged) | "Registry", the table, 10, 12 |

2. **Step scan.** Each step has one action and a check. The code steps give signatures and rules, not bodies.
3. **Interface match.** Each Consumes signature was copied from `model.rs` and `render.rs` at `16481ba8`, and the trial run compiled them.
4. **Review focus.** The graph's review focus 1 (evidence that only looks real) is covered by CF-1's `check_3_*` tests. Step 2 adds the two manifest forms of D1 and keeps each refused form refused. Focus 2-4 belong to CF-1 to CF-3.
5. **Proportion.** The plan has no code bodies. The table is the grading that K-D26 asks the operator to review.
6. **Live code.** Each cited path, line, test function, fixture and premise row was opened at `16481ba8`. The trial run resolved each `met` item. No existing function walks the repository's grade files: `conformance_tree` is new.
