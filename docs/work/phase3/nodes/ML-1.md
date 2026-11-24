# ML-1 — Mail config keys, the loadout pane `name` and the member-name grammar

**Spec:** `docs/specs/2026-11-22-workspace-mail-design.md` M§4 M-C6, M§6, M§8 (`[features] mail`), M§11
**Files:** `crates/zygo-config/src/schema.rs`, `crates/zygo-config/src/features.rs`, `crates/zygo-config/src/layers.rs`, `crates/zygo-config/src/check.rs`, `crates/zygo-core/src/ids.rs`, `crates/zygo-config/tests/config_layers.rs`, `crates/zygo-core/tests/core_basics.rs`, `crates/zygo-workspace/src/down.rs`, `crates/zygo/src/cmd/down.rs`, `crates/zygo-workspace/tests/down.rs`, `crates/zygo/tests/workspace_cli.rs`
**Consumes:** —
**Produces:** `zygo_config::schema::MailConfig { keep: String, max_unread: u32, wake_gap: String }` (Default `"30d"`, `500`, `"30s"`), `zygo_config::schema::Config.mail: MailConfig`, `zygo_config::schema::LoadoutPane.name: Option<String>`, `zygo_config::Features.mail: bool` (default `true`), `zygo_config::schema::parse_duration(s: &str) -> Option<Duration>` (adds the unit `d`), `zygo_config::layers::USER_ONLY: [&str; 3]` (`statusline`, `update`, `mail`), `zygo_core::ids::valid_member_name(s: &str) -> bool` (`^[a-z][a-z0-9-]{0,31}$` and not reserved), `zygo_core::ids::RESERVED_MEMBER_NAMES: [&str; 2]` (`all`, `cli`)
**Acceptance:** `test "$(cargo test -q -p zygo-config --test config_layers -- mail_ --list 2>/dev/null | grep -c ': test$')" -ge 5 && test "$(cargo test -q -p zygo-core --test core_basics -- valid_member_name --list 2>/dev/null | grep -c ': test$')" -ge 2 && cargo test -p zygo-config --test config_layers && cargo test -p zygo-core --test core_basics` → every count holds (at least 5 named `mail_…` in zygo-config `config_layers`; at least 2 named `valid_member_name…` in zygo-core `core_basics`; a count of 0 fails before any run), then all pass, with new cases: `[mail]` defaults 30d, 500, 30s; `parse_duration("30d")` is 2592000 s; a team-layer `[mail]` is dropped with the USER_ONLY warning; a `.zygo.toml` `[mail]` is dropped; `config check` refuses a pane `name` that fails the grammar, `all`, `cli`, a name used twice in one loadout, and a `[mail]` duration that `parse_duration` refuses; `valid_member_name` accepts `elena`, `a` and a 32-byte name, and refuses `Elena`, `1a`, `-a`, a 33-byte name, `all` and `cli`

## Plan

Consumes (live): — . Produces (exact, after this node):
- `crates/zygo-config/src/schema.rs`: `#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)] #[serde(default)] pub struct MailConfig { pub keep: String, pub max_unread: u32, pub wake_gap: String }`, `Default` = `{ keep: "30d", max_unread: 500, wake_gap: "30s" }`; `Config.mail: MailConfig` (field after `update`, `mail: MailConfig::default()` in `Config::default`); `LoadoutPane.name: Option<String>` (`#[serde(skip_serializing_if = "Option::is_none")]`, first field after `kind`); `pub fn parse_duration(s: &str) -> Option<Duration>` (live at `schema.rs:665`; gains the arm `b'd' => 86400`).
- `crates/zygo-config/src/features.rs`: `pub mail: bool` on `Features` (re-exported as `zygo_config::Features`), `mail: true` in `Default`.
- `crates/zygo-config/src/layers.rs`: `pub const USER_ONLY: [&str; 3] = ["statusline", "update", "mail"];` (live: `[&str; 2]` at `layers.rs:68`). `drop_user_only` (`layers.rs:80`) iterates `USER_ONLY`, so it needs no change: the warning becomes `config: [mail] in team config ignored; only user config may set them`.
- `crates/zygo-core/src/ids.rs`: `pub const RESERVED_MEMBER_NAMES: [&str; 2] = ["all", "cli"];` and `pub fn valid_member_name(s: &str) -> bool`.

Steps:

- [x] 1. In `crates/zygo-core/tests/core_basics.rs`: change the import to `use zygo_core::ids::{ulid, valid_member_name, RESERVED_MEMBER_NAMES};` and add:
  ```rust
  #[test]
  fn valid_member_name_accepts_the_m6_grammar() { // M§6: ^[a-z][a-z0-9-]{0,31}$
      let max = format!("a{}", "b".repeat(31));
      assert_eq!(max.len(), 32);
      for ok in ["elena", "a", max.as_str(), "a-", "a--b", "x9-y", "alle", "clip"] {
          assert!(valid_member_name(ok), "{ok:?}");
      }
  }

  #[test]
  fn valid_member_name_refuses_others_and_the_reserved() {
      assert_eq!(RESERVED_MEMBER_NAMES, ["all", "cli"]);
      let long = format!("a{}", "b".repeat(32));
      assert_eq!(long.len(), 33);
      for bad in ["Elena", "1a", "-a", long.as_str(), "all", "cli", "", "a_b", "a.b", "a b", "é", "a/b"] {
          assert!(!valid_member_name(bad), "{bad:?}");
      }
  }
  ```
- [x] 2. Run `cargo test -p zygo-core --test core_basics --locked valid_member_name`. Fail: compile error `unresolved imports ... valid_member_name`.
- [x] 3. In `crates/zygo-core/src/ids.rs` add `RESERVED_MEMBER_NAMES` as in Produces and `valid_member_name` with doc `/// A member name (workspace mail design M§6): ^[a-z][a-z0-9-]{0,31}$ and not in [`RESERVED_MEMBER_NAMES`]. Checked at each read (AU84).` Body by bytes (zygo-core has no `regex` dependency): first byte `b'a'..=b'z'`, length at most 32, every other byte `a-z`, `0-9` or `-`, and `!RESERVED_MEMBER_NAMES.contains(&s)`. The existing `zygo_config::profile::valid_name` (`profile.rs:49`) is not reused: it refuses `a-` and `a--b`, which M§6 allows, and zygo-core cannot depend on zygo-config.
- [x] 4. Run step 2's command. Pass: 2 tests pass.
- [x] 5. In `crates/zygo-config/tests/config_layers.rs`:
  - `parse_duration_forms` (`:1544`): remove `"10d"` from the `bad` list (it is now a valid form); add `"10D"` and `"213503982334602d"` (overflow) to it.
  - Add `name: None` to the two full `LoadoutPane { … }` literals (`:1596` in `workspace_and_loadout_defaults`, `:1612` in `loadout_parses_as_w11_writes_it`).
  - Add these tests at the end (`one`, `err`, `none`, `base`, `loaded`, `team_ctx`, `repo_with`, `ctx` are the live helpers):
  ```rust
  // ---- ML-1: [mail], pane name, member-name grammar (M§6, M§8, M§11, M-C6) ----

  const M11: &str = "[mail]\nkeep = \"30d\"\nmax_unread = 500\nwake_gap = \"30s\"\n\n[[loadout.team.window]]\n[[loadout.team.window.pane]]\nkind = \"claude-code\"\nname = \"elena\"\n";
  const MAIL_TEAM: &str = "config: [mail] in team config ignored; only user config may set them";

  #[test]
  fn mail_defaults_match_m11() {
      let d = MailConfig { keep: "30d".into(), max_unread: 500, wake_gap: "30s".into() };
      assert_eq!(MailConfig::default(), d);
      assert_eq!(Config::default().mail, d);
      assert_eq!(cfg("").mail, d);
      assert_eq!(cfg("[mail]\nmax_unread = 10\n").mail, MailConfig { max_unread: 10, ..d.clone() });
      assert!(Features::default().mail && Config::default().features.mail);
      assert!(!cfg("[features]\nmail = false\n").features.mail);
      assert!(cfg("[features]\nresults = false\n").features.mail);
      assert_eq!(LoadoutPane::default().name, None);
      let m = loaded("");
      assert_eq!((m.config.mail.clone(), m.source_of("mail.keep")), (d, Layer::Builtin));
  }

  #[test]
  fn mail_m11_example_parses() {
      let m = load(&ctx(Some(M11), None, None)).unwrap();
      assert_eq!(m.warnings, Vec::<String>::new());
      assert_eq!(m.config.mail, MailConfig::default());
      assert_eq!(m.source_of("mail.wake_gap"), Layer::User);
      assert_eq!(m.config.loadout["team"].window[0].pane[0].name.as_deref(), Some("elena"));
      none(&(base() + M11));
  }

  #[test]
  fn mail_parse_duration_takes_days() {
      use std::time::Duration;
      assert_eq!(parse_duration("30d"), Some(Duration::from_secs(2_592_000)));
      assert_eq!(parse_duration("1d"), Some(Duration::from_secs(86_400)));
      assert_eq!(parse_duration("0d"), Some(Duration::ZERO));
      assert_eq!(parse_duration("213503982334601d"), Some(Duration::from_secs(213_503_982_334_601 * 86_400)));
      for bad in ["d", "30D", "213503982334602d", "1.5d", "-1d", "30 d"] {
          assert_eq!(parse_duration(bad), None, "{bad:?}");
      }
      assert_eq!(parse_duration(&MailConfig::default().keep), Some(Duration::from_secs(2_592_000)));
      assert_eq!(parse_duration(&MailConfig::default().wake_gap), Some(Duration::from_secs(30)));
  }

  #[test]
  fn mail_team_layer_drops_mail_with_the_user_only_warning() { // M§11, M-C6
      assert_eq!(layers::USER_ONLY, ["statusline", "update", "mail"]);
      let m = load(&team_ctx("[mail]\nkeep = \"1d\"\nmax_unread = 1\n[guards]\nmin_level = \"notify\"\n", "")).unwrap();
      assert_eq!(m.config.mail, MailConfig::default());
      assert_eq!(m.source_of("mail.keep"), Layer::Builtin);
      assert_eq!(m.team_keys, BTreeSet::from(["guards.min_level".to_string()]));
      assert_eq!(m.warnings, [MAIL_TEAM]);
      // a value of the wrong type is dropped before the layer is parsed
      assert_eq!(load(&team_ctx("mail = 3\n", "")).unwrap().warnings, [MAIL_TEAM]);
      let all = "[statusline]\nstyle = \"verbose\"\n[update]\ncheck = false\n[mail]\nwake_gap = \"1s\"\n";
      assert_eq!(load(&team_ctx(all, "")).unwrap().warnings,
          ["config: [statusline], [update], [mail] in team config ignored; only user config may set them"]);
      // the user's [mail] holds in a team session
      let m = load(&team_ctx("[mail]\nkeep = \"1d\"\n", "[mail]\nkeep = \"7d\"\n")).unwrap();
      assert_eq!((m.config.mail.keep.as_str(), m.source_of("mail.keep")), ("7d", Layer::User));
  }

  #[test]
  #[ignore = "timing: git calls run under the 2 s production bound (GIT_BOUND)"]
  fn mail_repo_file_table_is_dropped() { // M-C6: a cloned repository sets no [mail]
      let d = repo_with("ml1", "version = 1\n[mail]\nkeep = \"1d\"\n");
      match repo_file::read_default_branch(&d) {
          RepoFile::Found { table, ignored, .. } => {
              assert_eq!(ignored, ["mail"]);
              assert!(!table.contains_key("mail"));
          }
          other => panic!("{other:?}"),
      }
      let m = load(&ctx(None, Some(d.to_path_buf()), None)).unwrap();
      assert_eq!((m.config.mail.clone(), m.source_of("mail.keep")), (MailConfig::default(), Layer::Builtin));
      assert_eq!(m.warnings,
          [".zygo.toml: [mail] ignored; the repository file may set only [guards], [navigation], [limits], [work]"]);
  }

  const NAME_GRAMMAR: &str = "must be 1-32 of [a-z0-9-], starting with a letter";

  #[test]
  fn mail_check_refuses_bad_pane_names() { // M§6
      let p = "loadout.pair.window[0].pane[0].name";
      none(&(base() + LO + "kind = \"claude-code\"\nname = \"elena\"\n"));
      err(&(base() + LO + "kind = \"claude-code\"\nname = \"Elena\"\n"), &format!("{p}: {NAME_GRAMMAR}"));
      err(&(base() + LO + "kind = \"shell\"\nname = \"\"\n"), &format!("{p}: {NAME_GRAMMAR}"));
      err(&(base() + LO + "kind = \"claude-code\"\nname = \"all\"\n"), &format!("{p}: is reserved"));
      err(&(base() + LO + "kind = \"opencode\"\nname = \"cli\"\n"), &format!("{p}: is reserved"));
      // unique in the loadout, across windows; the later pane is reported
      let twice = "kind = \"claude-code\"\nname = \"elena\"\n[[loadout.pair.window]]\n[[loadout.pair.window.pane]]\nkind = \"opencode\"\nname = \"elena\"\n";
      err(&(base() + LO + twice), "loadout.pair.window[1].pane[0].name: is used by another pane in the loadout");
      // the same name in two loadouts is allowed
      let other = "[loadout.solo2]\n[[loadout.solo2.window]]\n[[loadout.solo2.window.pane]]\nkind = \"shell\"\nname = \"elena\"\n";
      none(&(base() + LO + "kind = \"claude-code\"\nname = \"elena\"\n" + other));
  }

  #[test]
  fn mail_check_refuses_bad_durations() { // M§11; the forms of parse_duration
      const FORMS: &str = "must be <n>s, <n>m, <n>h or <n>d";
      none(&(base() + "[mail]\nkeep = \"12h\"\nwake_gap = \"0s\"\n"));
      err(&(base() + "[mail]\nkeep = \"30\"\n"), &format!("mail.keep: {FORMS}"));
      err(&(base() + "[mail]\nwake_gap = \"1w\"\n"), &format!("mail.wake_gap: {FORMS}"));
      let v: Vec<String> = check(&loaded(&(base() + "[mail]\nkeep = \"x\"\nwake_gap = \"\"\n")).config).iter().map(ToString::to_string).collect();
      assert_eq!(v, [format!("mail.keep: {FORMS}"), format!("mail.wake_gap: {FORMS}")]);
  }
  ```
- [x] 6. Run `cargo test -p zygo-config --test config_layers --locked mail_`. Fail: compile errors `cannot find type MailConfig`, `no field mail`, `no field name`.
- [x] 7. In `crates/zygo-config/src/schema.rs`: add `MailConfig` and its `Default` after `UpdateConfig` (doc `/// [mail] (workspace mail design M§11). User config only; keep and wake_gap in the form [`parse_duration`] reads.`); add `Config.mail`; add `LoadoutPane.name` (doc `/// The member name (M§6); config check applies zygo_core::ids::valid_member_name and uniqueness in the loadout.`); in `parse_duration` add `b'd' => 86400` and change its doc to `<n>s, <n>m, <n>h or <n>d (W§11 drain_timeout, M§11 keep and wake_gap)`.
- [x] 7a. `parse_duration` now takes `d`, so the drain-timeout messages name it. Change `<n>s, <n>m or <n>h` to `<n>s, <n>m, <n>h or <n>d` in: `crates/zygo-workspace/src/down.rs:463` (the `drain_timeout "…" is not … ; using 10m` warning); `crates/zygo/src/cmd/down.rs:25` (the `--timeout` doc comment `/// The drain timeout: …`) and `:49` (the `bad --timeout value … : use …` error); and the tests that assert the old text: `crates/zygo-workspace/tests/down.rs:1348` (in `signals_cancel_drains_and_restore_once`, `#[ignore]` timing test, so the floor does not run it) and `crates/zygo/tests/workspace_cli.rs:121` (in `w12_down_refuses_a_bad_timeout_a_name_with_all_and_a_dash_name`). Then `grep -rn "or <n>h" crates` prints nothing. Pass: `cargo test -p zygo --test workspace_cli --locked w12_down_refuses_a_bad_timeout` passes.
  - Note (implement): step 7a supersedes the drift bullet below that says these messages "stay as they are"; they now name `<n>d`.
- [x] 8. In `crates/zygo-config/src/features.rs`: add `pub mail: bool` with doc `/// The mail tools and the session mail item (M§8, M§8a). Default true; the consumer applies them only in a workspace member.` and `mail: true` in `Default` (the module doc "Every toggle defaults to `true`, but `workspace_tool`" stays true). In `crates/zygo-config/src/layers.rs`: `USER_ONLY` as in Produces; change its doc's citation to `(statusline HUD design §8; auto-update design "Opt-out"; workspace mail design M§11, M-C6)`.
- [x] 9. In `crates/zygo-config/src/check.rs`:
  - In `check`, after the `statusline.style` rule: for `(key, text)` in `[("keep", &cfg.mail.keep), ("wake_gap", &cfg.mail.wake_gap)]`, when `parse_duration(text).is_none()`, `e(format!("mail.{key}"), "must be <n>s, <n>m, <n>h or <n>d".into())`.
  - In `loadout_violations`, per loadout keep `let mut seen: BTreeSet<&str>`; for each pane with `Some(n) = &p.name` at path `{pp}.name`: if `RESERVED_MEMBER_NAMES.contains(&n.as_str())` → Error `is reserved`; else if `!valid_member_name(n)` → Error `must be 1-32 of [a-z0-9-], starting with a letter`; else if `!seen.insert(n)` → Error `is used by another pane in the loadout`. The message never echoes the name (it is user text).
  - Imports: `parse_duration` from `crate::schema`, `zygo_core::ids::{valid_member_name, RESERVED_MEMBER_NAMES}`, `std::collections::BTreeSet`.
- [x] 10. Run `cargo test -p zygo-config --test config_layers --locked`. Pass: every non-ignored test passes, the 6 new CI `mail_` tests, `parse_duration_forms`, `workspace_and_loadout_defaults`, `loadout_parses_as_w11_writes_it` and the HUD-3 team tests (their warnings name only `[statusline]`/`[update]`, so they are unchanged) included.
- [x] 11. On demand, local only (not CI): `cargo test -p zygo-config --test config_layers --locked -- --ignored mail_repo_file_table_is_dropped`. Pass: 1 passed.
- [x] 12. Acceptance: `test "$(cargo test -q -p zygo-config --test config_layers -- mail_ --list 2>/dev/null | grep -c ': test$')" -ge 5 && test "$(cargo test -q -p zygo-core --test core_basics -- valid_member_name --list 2>/dev/null | grep -c ': test$')" -ge 2 && cargo test -p zygo-config --test config_layers && cargo test -p zygo-core --test core_basics`. Pass: counts 7 and 2, exit 0.
- [x] 13. Floor: `cargo test --workspace --locked --no-fail-fast`. Pass: exit 0, `0 failed` in every result line (the `..LoadoutPane::default()` literals in `crates/zygo-workspace/tests/up.rs` compile unchanged).
  - Deviation (implement): the plan missed one full `LoadoutPane { … }` literal at `crates/zygo-workspace/tests/loadout.rs:46` (`unset_pane_fields_take_the_launch_defaults_and_set_fields_are_kept`); the floor failed with `E0063 missing field name`. Fixed by adding `name: None` to it. The file is outside Files; no other fix compiles (`#[non_exhaustive]` would forbid the literals in other crates).
- [x] 14. Commit the 11 Files (and `crates/zygo-workspace/tests/loadout.rs`, see step 13): `feat(config): [mail] keys, pane name, member-name grammar (ML-1)`, trailers `Zygo-Session: <session>` and `Zygo-Work: phase3:ML-1`, no Claude attribution.
  - Note (implement): the trailer is `Zygo-Work: phase3:ML-1` as the executor's brief gives it.

Drift and author's calls:
- M§8 says `[features] mail` is "default on in a workspace member, off otherwise". A config default cannot see membership, so `Features.mail` defaults to `true` (as Produces states) and the member condition is applied by its consumers (ML-9 `SessionView.mail`, ML-11 tool listing for a member view).
- Adding `d` to `parse_duration` also makes `[workspace] drain_timeout` and `zygo down --timeout` accept `<n>d`. Their messages (`crates/zygo-workspace/src/down.rs:463`, `crates/zygo/src/cmd/down.rs:49`, `:25`) still say `<n>s, <n>m or <n>h`; they are outside Files and stay as they are (the forms they name still work). The existing test `parse_duration_forms` lists `"10d"` as refused (`config_layers.rs:1553`); step 5 moves it.
- `.zygo.toml` `[mail]` is already dropped by the repository filter (`KEPT`, `repo_file.rs:39`); ML-1 only proves it. That proof reads git under the 2 s `GIT_BOUND`, so `mail_repo_file_table_is_dropped` is `#[ignore = "timing: …"]` like every other repo-file test here and runs in step 11; the 6 other `mail_` tests run in CI.
- Not specified, chosen: the `config check` messages (above); the duplicate is reported on the later pane only; the check never echoes the name; a name is unique per loadout, not across loadouts (M§6 "unique in the loadout"); no `config check` rule for `max_unread` (spec gives none; `u32` refuses negatives at parse).
- A profile cannot set `features.mail`: `profile::features_ok` (`profile.rs:160`) refuses every key outside its list with `not a feature key`, as for `workspace_tool`. The spec does not ask for a profile override, and `profile.rs` is outside Files.
- `drain_timeout` has no `config check` rule today; ML-1 adds rules for the two `[mail]` durations only (Acceptance).
