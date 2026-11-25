# MS-3 — Mail sync config keys, forms and per-workspace resolution

**Spec:** `docs/specs/2026-11-24-mail-sync-design.md` S§11, S§7.1, S-C6, S-G6; S§16 (M§11); S§19 #7 (config parts), #25
**Files:** `crates/zygo-config/src/schema.rs`, `crates/zygo-config/src/check.rs`, `crates/zygo-config/src/layers.rs`, `crates/zygo-config/tests/mail_sync_config.rs`, `docs/specs/2026-11-22-workspace-mail-design.md` (S§16 change note: M§11 gains host, sync_gap, [mail.workspace.<name>])
**Consumes:** the [mail] config table: `MailConfig` (ML-1), `Config.mail` (ML-1), `USER_ONLY` (ML-1); schema::parse_duration (existing, crates/zygo-config/src/schema.rs)
**Produces:** `zygo_config::schema::MailSyncWorkspace { sync: Option<String>, remote: Option<String>, host: Option<String> }`; on the P3-1 `[mail]` struct the fields `host: Option<String>`, `sync_gap: String` (default `"60s"`), `workspace: BTreeMap<String, MailSyncWorkspace>`; `zygo_config::schema::valid_mail_host(s: &str) -> bool`, `zygo_config::schema::valid_sync_branch(s: &str) -> bool`, `zygo_config::schema::SyncSettings { branch: String, remote: String, host: String, sync_gap: Duration }`, `zygo_config::schema::SyncOff` (enum: `NotOn`, `NoHost`, `BadHost`, `BadBranch`, `BadGap`), `zygo_config::schema::sync_for(cfg: &Config, ws_name: &str) -> Result<SyncSettings, SyncOff>`; `"mail"` in `zygo_config::layers::USER_ONLY` when P3-1 did not add it
**Acceptance:** `cargo test -p zygo-config --test mail_sync_config && cargo test -p zygo-config` → all pass: `sync = "main"` is a config check error and `sync_for` gives `BadBranch`; a set `sync` with no host gives a check error that names `[mail] host` with the S§7.1 text; `host` and `sync` are accepted and refused at the edges of `^[a-z0-9][a-z0-9-]{0,31}$` and `^zygo/[a-z0-9][a-z0-9-]{0,31}$`; `[mail.workspace.<n>] host` overrides `[mail] host`; `remote` defaults to `origin`; no `sync` key gives `NotOn`; `[mail]` in `.zygo.toml` is ignored with the existing line and in team config is dropped with one warning (S§19 #25); the existing config tests still pass.

## Plan

Premise check (live code at origin/phase-3, b10525fc):
- ML-1 landed `[mail]`: `MailConfig { keep, max_unread, wake_gap }` (`crates/zygo-config/src/schema.rs:673-686`), `Config.mail` (`schema.rs:34`), and `"mail"` is already in `USER_ONLY` (`crates/zygo-config/src/layers.rs:69`). The team drop (`layers.rs:81-91`) and the `.zygo.toml` kept list (`crates/zygo-config/src/repo_file.rs:39`) already drop `[mail]` as a whole table, so they drop the new keys with no change. `layers.rs` needs no edit.
- WD-5 landed the git-free `.zygo.toml` seam `load_with_repo_reader` (`layers.rs:436-440`); the S§19 #25 test uses it, so it runs in CI with no time bound (the ML-1 git test `mail_repo_file_table_is_dropped` is `#[ignore]` for timing, `crates/zygo-config/tests/config_layers.rs:1778-1779`).
- Files-list gap: `crates/zygo-config/tests/config_layers.rs:1723` builds `MailConfig { keep, max_unread, wake_gap }` with all fields named; the new fields break that line at compile time. Step 9 adds the three new fields to that one literal. It is the only other literal of `MailConfig` in the workspace (searched `crates/`).
- Author's call, from S§11 (`sync_gap = "60s"  # S§8.4; <n>s | <n>m | <n>h`): `sync_gap` takes `s`, `m` and `h` only. `parse_duration` also takes `d` (ML-1), so `sync_gap` uses a narrower reader.
- No review-focus line in `docs/work/phase3/graph.md` names MS-3.

Consumes (live):
- `pub struct MailConfig { pub keep: String, pub max_unread: u32, pub wake_gap: String }`, `#[serde(default)]`, default `{ keep: "30d", max_unread: 500, wake_gap: "30s" }` (`schema.rs:673-686`, ML-1)
- `pub mail: MailConfig` on `Config` (`schema.rs:34`, ML-1)
- `pub const USER_ONLY: [&str; 3] = ["statusline", "update", "mail"];` (`layers.rs:69`, ML-1)
- `pub fn parse_duration(s: &str) -> Option<Duration>` (`schema.rs:690`)
- `pub(crate) fn join<S: AsRef<str>>(segs: &[S]) -> String` (`layers.rs:160`): the source-key path that `check` already uses for user-named tables
- test seams: `#[doc(hidden)] pub fn load_with_repo_reader(ctx: &LoadContext, builtin: Table, read: impl FnOnce(&Path) -> RepoFile) -> Result<Merged, ConfigError>` (`layers.rs:436`), `pub fn from_text(branch: String, text: &str) -> RepoFile` (`repo_file.rs:269`), `pub fn source_of(&self, key: &str) -> Layer` (`layers.rs:679`)

Produces (exact, after this node; all in `crates/zygo-config/src/schema.rs`, re-exported by `pub use schema::*`):
- `#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)] #[serde(default)] pub struct MailSyncWorkspace { pub sync: Option<String>, pub remote: Option<String>, pub host: Option<String> }`; each field `#[serde(skip_serializing_if = "Option::is_none")]`
- on `MailConfig`, after `wake_gap`: `#[serde(skip_serializing_if = "Option::is_none")] pub host: Option<String>`, `pub sync_gap: String`, `#[serde(skip_serializing_if = "BTreeMap::is_empty")] pub workspace: BTreeMap<String, MailSyncWorkspace>`; `MailConfig::default()` adds `host: None, sync_gap: "60s".into(), workspace: BTreeMap::new()`
- `pub fn valid_mail_host(s: &str) -> bool`: true exactly for `^[a-z0-9][a-z0-9-]{0,31}$` (ASCII bytes; no regex needed)
- `pub fn valid_sync_branch(s: &str) -> bool`: true exactly for `^zygo/[a-z0-9][a-z0-9-]{0,31}$` (`zygo/` then `valid_mail_host`)
- `pub(crate) fn parse_sync_gap(s: &str) -> Option<Duration>`: `parse_duration(s)`, `None` when the unit is `d`
- `#[derive(Debug, Clone, PartialEq, Eq)] pub struct SyncSettings { pub branch: String, pub remote: String, pub host: String, pub sync_gap: Duration }`
- `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum SyncOff { NotOn, NoHost, BadHost, BadBranch, BadGap }`
- `pub fn sync_for(cfg: &Config, ws_name: &str) -> Result<SyncSettings, SyncOff>`, checks in this order: no `cfg.mail.workspace[ws_name]` or its `sync` is `None` → `NotOn`; `sync` fails `valid_sync_branch` → `BadBranch`; host = the workspace `host`, else `[mail] host`; neither set → `NoHost`; host fails `valid_mail_host` → `BadHost`; `parse_sync_gap(&cfg.mail.sync_gap)` is `None` → `BadGap`; else `Ok` with `remote` = the workspace `remote`, else `"origin"`
- `zygo_config::layers::USER_ONLY`: unchanged, it already holds `"mail"`

`config check` rules (`crates/zygo-config/src/check.rs`, in `check`, right after the `keep`/`wake_gap` loop at `check.rs:114-118`), in this order; no message echoes a value (user text):
1. `[mail] host` set and not `valid_mail_host` → `mail.host: must match ^[a-z0-9][a-z0-9-]{0,31}$`
2. `parse_sync_gap(sync_gap)` is `None` → `mail.sync_gap: must be <n>s, <n>m or <n>h`
3. for each `[mail.workspace.<n>]` in key order, with path `join(&["mail", "workspace", n])`: `sync` set and not `valid_sync_branch` → `<path>.sync: must match ^zygo/[a-z0-9][a-z0-9-]{0,31}$`; then `host` set and not `valid_mail_host` → `<path>.host: must match ^[a-z0-9][a-z0-9-]{0,31}$`
4. `[mail] host` not set and some workspace has `sync` set and no `host` → one line `mail.host: set [mail] host to turn on mail sync; zygo does not publish the system host name` (the S§7.1 text; `zygo config check` adds `error: `)

All are `Severity::Error`. The four texts are private `const`s in `check.rs`.

Steps:

- [x] 1. Create `crates/zygo-config/tests/mail_sync_config.rs` with the helpers and constants:
  ```rust
  //! MS-3: mail sync config keys, forms and per-workspace resolution (mail sync design S§11, S§7.1, S-C6, S-G6;
  //! S§19 #7 config parts, #25).

  use std::collections::BTreeMap;
  use std::path::PathBuf;
  use std::time::Duration;

  use zygo_config::layers::load_with_repo_reader;
  use zygo_config::*;
  use zygo_core::paths::Paths;

  const S11: &str = "[mail]\nhost = \"studio\"\nsync_gap = \"60s\"\n\n[mail.workspace.zygo]\nsync = \"zygo/mail\"\nremote = \"origin\"\nhost = \"studio-2\"\n";
  const HOST_FORM: &str = "must match ^[a-z0-9][a-z0-9-]{0,31}$";
  const SYNC_FORM: &str = "must match ^zygo/[a-z0-9][a-z0-9-]{0,31}$";
  const GAP_FORM: &str = "must be <n>s, <n>m or <n>h";
  const NO_HOST: &str = "set [mail] host to turn on mail sync; zygo does not publish the system host name";
  const MAIL_TEAM: &str = "config: [mail] in team config ignored; only user config may set them";
  const MAIL_REPO: &str = ".zygo.toml: [mail] ignored; the repository file may set only [guards], [navigation], [limits], [work]";

  #[must_use = "the directory is removed when this value drops; bind it"]
  struct Cx {
      _d: tempfile::TempDir,
      c: LoadContext,
  }

  fn ctx(user: &str, team: Option<&str>, repo: Option<PathBuf>) -> Cx {
      let d = tempfile::Builder::new().prefix("zygo-ms3-").tempdir().unwrap();
      let p = d.path();
      std::fs::write(p.join("config.toml"), user).unwrap();
      let paths = Paths {
          config_file: p.join("config.toml"),
          cache_dir: p.join("cache"),
          data_dir: p.join("data"),
          sessions_dir: p.join("cache/sessions"),
          marketplaces_dir: p.join("cache/marketplaces"),
          plugin_store: p.join("data/plugins"),
          approvals_file: p.join("data/approvals.toml"),
          usage_db: p.join("data/usage.db"),
      };
      Cx { c: LoadContext { paths, repo, team: team.map(|t| toml::from_str(t).unwrap()) }, _d: d }
  }

  fn loaded(user: &str) -> Merged {
      load(&ctx(user, None, None).c).unwrap()
  }

  /// Every `config check` violation of `user`, as `<severity> <path>: <message>`, in check order.
  fn violations(user: &str) -> Vec<String> {
      check(&loaded(user).config).iter().map(|v| format!("{:?} {v}", v.severity)).collect()
  }

  fn sync(user: &str, ws: &str) -> Result<SyncSettings, SyncOff> {
      sync_for(&loaded(user).config, ws)
  }

  /// The merged config when the `.zygo.toml` read returns `repo` through the real filter (no git call, no time bound).
  fn with_repo_file(user: &str, repo: &str) -> Merged {
      let cx = ctx(user, None, Some(PathBuf::from("repo")));
      let builtin = toml::Table::try_from(builtin::defaults()).unwrap();
      load_with_repo_reader(&cx.c, builtin, |_| repo_file::from_text("main".into(), repo)).unwrap()
  }

  fn on(gap: &str) -> String {
      format!("[mail]\nhost = \"a\"\nsync_gap = \"{gap}\"\n[mail.workspace.zygo]\nsync = \"zygo/mail\"\n")
  }
  ```

- [x] 2. Add test `defaults_and_the_s11_example` (S§11; `config show` source, S§11 last bullets):
  ```rust
  let d = MailConfig::default();
  assert_eq!((d.host.clone(), d.sync_gap.as_str(), d.workspace.clone()), (None, "60s", BTreeMap::new()));
  assert_eq!((d.keep.as_str(), d.max_unread, d.wake_gap.as_str()), ("30d", 500, "30s"));
  assert_eq!(Config::default().mail, d);
  assert_eq!(loaded("").source_of("mail.sync_gap"), Layer::Builtin);
  assert_eq!(violations(""), Vec::<String>::new());
  let m = loaded(S11);
  assert_eq!(m.warnings, Vec::<String>::new());
  assert_eq!(m.config.mail.host.as_deref(), Some("studio"));
  assert_eq!(m.config.mail.workspace["zygo"],
      MailSyncWorkspace { sync: Some("zygo/mail".into()), remote: Some("origin".into()), host: Some("studio-2".into()) });
  for k in ["mail.host", "mail.sync_gap", "mail.workspace.zygo.sync", "mail.workspace.zygo.remote", "mail.workspace.zygo.host"] {
      assert_eq!(m.source_of(k), Layer::User, "{k}");
  }
  assert_eq!(violations(S11), Vec::<String>::new());
  assert_eq!(sync_for(&m.config, "zygo"), Ok(SyncSettings {
      branch: "zygo/mail".into(), remote: "origin".into(), host: "studio-2".into(), sync_gap: Duration::from_secs(60) }));
  ```

- [x] 3. Add tests `valid_mail_host_edges` and `valid_sync_branch_edges` (S§5 Host form, S§11 `sync` form):
  ```rust
  #[test]
  fn valid_mail_host_edges() {
      let max = "a".repeat(32);
      let over = "a".repeat(33);
      for ok in ["a", "0", "a-", "0-9", "studio", "studio-2", max.as_str()] {
          assert!(valid_mail_host(ok), "{ok:?}");
      }
      for bad in ["", "-", "-a", "A", "Studio", "a_b", "a.b", "a b", "a/b", "é", "studio\n", over.as_str()] {
          assert!(!valid_mail_host(bad), "{bad:?}");
      }
  }

  #[test]
  fn valid_sync_branch_edges() {
      let max = format!("zygo/{}", "a".repeat(32));
      let over = format!("zygo/{}", "a".repeat(33));
      for ok in ["zygo/mail", "zygo/a", "zygo/0", "zygo/a-", max.as_str()] {
          assert!(valid_sync_branch(ok), "{ok:?}");
      }
      for bad in ["main", "zygo", "zygo/", "zygo/-a", "zygo/Mail", "Zygo/mail", "zygo//mail", "zygo/mail/x",
          "refs/heads/zygo/mail", "zygo/mail ", " zygo/mail", over.as_str()] {
          assert!(!valid_sync_branch(bad), "{bad:?}");
      }
  }
  ```

- [x] 4. Add test `check_refuses_forms_at_the_edges` (S§19 #7: `sync = "main"`; S§11, S§7.1 forms through `config check` and `sync_for`):
  ```rust
  let user = "[mail]\nhost = \"a\"\n[mail.workspace.zygo]\nsync = \"main\"\n";
  assert_eq!(violations(user), [format!("Error mail.workspace.zygo.sync: {SYNC_FORM}")]);
  assert_eq!(sync(user, "zygo"), Err(SyncOff::BadBranch));
  let (max, over) = ("a".repeat(32), "a".repeat(33));
  for (v, ok) in [("a", true), (max.as_str(), true), ("-a", false), (over.as_str(), false), ("A", false)] {
      let want = |path: &str| if ok { Vec::<String>::new() } else { vec![format!("Error {path}: {HOST_FORM}")] };
      assert_eq!(violations(&format!("[mail]\nhost = \"{v}\"\n")), want("mail.host"), "{v:?}");
      let ws = format!("[mail]\nhost = \"a\"\n[mail.workspace.zygo]\nsync = \"zygo/mail\"\nhost = \"{v}\"\n");
      assert_eq!(violations(&ws), want("mail.workspace.zygo.host"), "{v:?}");
      assert_eq!(sync(&ws, "zygo").map(|s| s.host), if ok { Ok(v.to_string()) } else { Err(SyncOff::BadHost) }, "{v:?}");
      let br = format!("[mail]\nhost = \"a\"\n[mail.workspace.zygo]\nsync = \"zygo/{v}\"\n");
      let want_br: Vec<String> = if ok { vec![] } else { vec![format!("Error mail.workspace.zygo.sync: {SYNC_FORM}")] };
      assert_eq!(violations(&br), want_br, "{v:?}");
      assert_eq!(sync(&br, "zygo").map(|s| s.branch), if ok { Ok(format!("zygo/{v}")) } else { Err(SyncOff::BadBranch) }, "{v:?}");
  }
  // the path quotes a workspace name that is not bare; no message echoes a value
  let odd = "[mail]\nhost = \"a\"\n[mail.workspace.\"my ws\"]\nsync = \"zygo/Bad-Branch\"\nhost = \"Bad-Host\"\n";
  let v = violations(odd);
  assert_eq!(v, [format!("Error mail.workspace.\"my ws\".sync: {SYNC_FORM}"), format!("Error mail.workspace.\"my ws\".host: {HOST_FORM}")]);
  assert!(v.iter().all(|l| !l.contains("Bad-")), "{v:?}");
  assert_eq!(sync(odd, "my ws"), Err(SyncOff::BadBranch));
  ```

- [x] 5. Add test `check_names_mail_host_when_sync_has_no_host` (S§7.1, S§19 #7):
  ```rust
  let user = "[mail.workspace.zygo]\nsync = \"zygo/mail\"\n";
  assert_eq!(violations(user), [format!("Error mail.host: {NO_HOST}")]);
  assert_eq!(sync(user, "zygo"), Err(SyncOff::NoHost));
  // one line for any number of workspaces with no host
  let two = "[mail.workspace.a]\nsync = \"zygo/mail\"\n[mail.workspace.b]\nsync = \"zygo/mail\"\nhost = \"b\"\n[mail.workspace.c]\nsync = \"zygo/x\"\n";
  assert_eq!(violations(two), [format!("Error mail.host: {NO_HOST}")]);
  // the form error comes first, then the host line
  assert_eq!(violations("[mail.workspace.zygo]\nsync = \"main\"\n"),
      [format!("Error mail.workspace.zygo.sync: {SYNC_FORM}"), format!("Error mail.host: {NO_HOST}")]);
  // a workspace host alone is enough; no sync needs no host
  assert_eq!(violations("[mail.workspace.zygo]\nsync = \"zygo/mail\"\nhost = \"b\"\n"), Vec::<String>::new());
  assert_eq!(violations("[mail.workspace.zygo]\nremote = \"up\"\n"), Vec::<String>::new());
  assert_eq!(violations("[mail]\nhost = \"a\"\n"), Vec::<String>::new());
  ```

- [x] 6. Add test `workspace_host_overrides_mail_host` (S§7.1):
  ```rust
  let user = "[mail]\nhost = \"a\"\n[mail.workspace.zygo]\nsync = \"zygo/mail\"\nhost = \"b\"\n[mail.workspace.other]\nsync = \"zygo/x\"\n";
  let c = loaded(user).config;
  assert_eq!(sync_for(&c, "zygo").map(|s| s.host), Ok("b".into()));
  assert_eq!(sync_for(&c, "other").map(|s| s.host), Ok("a".into()));
  // a bad workspace host is not replaced by a good [mail] host
  assert_eq!(sync("[mail]\nhost = \"a\"\n[mail.workspace.zygo]\nsync = \"zygo/mail\"\nhost = \"B\"\n", "zygo"), Err(SyncOff::BadHost));
  // a bad [mail] host with no workspace host
  let bad = "[mail]\nhost = \"-a\"\n[mail.workspace.zygo]\nsync = \"zygo/mail\"\n";
  assert_eq!(sync(bad, "zygo"), Err(SyncOff::BadHost));
  assert_eq!(violations(bad), [format!("Error mail.host: {HOST_FORM}")]);
  // a good workspace host wins over a bad [mail] host (the check still reports mail.host)
  let ok = "[mail]\nhost = \"-a\"\n[mail.workspace.zygo]\nsync = \"zygo/mail\"\nhost = \"b\"\n";
  assert_eq!(sync(ok, "zygo").map(|s| s.host), Ok("b".into()));
  assert_eq!(violations(ok), [format!("Error mail.host: {HOST_FORM}")]);
  ```

- [x] 7. Add tests `remote_defaults_to_origin`, `no_sync_key_is_not_on` and `sync_gap_forms` (S§11, S-G6, S§8.4):
  ```rust
  #[test]
  fn remote_defaults_to_origin() {
      assert_eq!(sync(&on("60s"), "zygo"), Ok(SyncSettings {
          branch: "zygo/mail".into(), remote: "origin".into(), host: "a".into(), sync_gap: Duration::from_secs(60) }));
      let up = "[mail]\nhost = \"a\"\n[mail.workspace.zygo]\nsync = \"zygo/mail\"\nremote = \"upstream\"\n";
      assert_eq!(sync(up, "zygo").map(|s| s.remote), Ok("upstream".into()));
  }

  #[test]
  fn no_sync_key_is_not_on() {
      assert_eq!(sync("", "zygo"), Err(SyncOff::NotOn));
      assert_eq!(sync("[mail]\nhost = \"a\"\n", "zygo"), Err(SyncOff::NotOn));
      assert_eq!(sync("[mail]\nhost = \"a\"\n[mail.workspace.zygo]\nremote = \"origin\"\nhost = \"b\"\n", "zygo"), Err(SyncOff::NotOn));
      // another workspace's opt-in does not turn this one on (S-G6)
      assert_eq!(sync("[mail]\nhost = \"a\"\n[mail.workspace.other]\nsync = \"zygo/mail\"\n", "zygo"), Err(SyncOff::NotOn));
      // order: NotOn, BadBranch, NoHost/BadHost, BadGap
      assert_eq!(sync("[mail]\nhost = \"-a\"\nsync_gap = \"x\"\n", "zygo"), Err(SyncOff::NotOn));
      assert_eq!(sync("[mail]\nsync_gap = \"x\"\n[mail.workspace.zygo]\nsync = \"main\"\n", "zygo"), Err(SyncOff::BadBranch));
      assert_eq!(sync("[mail]\nsync_gap = \"x\"\n[mail.workspace.zygo]\nsync = \"zygo/mail\"\n", "zygo"), Err(SyncOff::NoHost));
      assert_eq!(sync("[mail]\nhost = \"-a\"\nsync_gap = \"x\"\n[mail.workspace.zygo]\nsync = \"zygo/mail\"\n", "zygo"), Err(SyncOff::BadHost));
  }

  #[test]
  fn sync_gap_forms() {
      for (gap, secs) in [("0s", 0), ("60s", 60), ("5m", 300), ("2h", 7200)] {
          assert_eq!(sync(&on(gap), "zygo").map(|s| s.sync_gap), Ok(Duration::from_secs(secs)), "{gap}");
          assert_eq!(violations(&on(gap)), Vec::<String>::new(), "{gap}");
      }
      for gap in ["1d", "60", "", "1w", "-1s", "1.5m", "60S", "s"] {
          assert_eq!(sync(&on(gap), "zygo"), Err(SyncOff::BadGap), "{gap}");
          assert_eq!(violations(&on(gap)), [format!("Error mail.sync_gap: {GAP_FORM}")], "{gap}");
      }
  }
  ```

- [x] 8. Add tests `team_layer_drops_mail_sync_keys` and `repo_file_drops_mail_sync_keys` (S§19 #25, S-C6, S§11 first paragraph):
  ```rust
  #[test]
  fn team_layer_drops_mail_sync_keys() {
      assert!(layers::USER_ONLY.contains(&"mail"));
      let team = "[mail]\nhost = \"t\"\nsync_gap = \"1s\"\n[mail.workspace.zygo]\nsync = \"zygo/mail\"\n";
      let m = load(&ctx("", Some(team), None).c).unwrap();
      assert_eq!(m.warnings, [MAIL_TEAM]);
      assert_eq!(m.config.mail, MailConfig::default());
      assert_eq!(m.source_of("mail.workspace.zygo.sync"), Layer::Builtin);
      assert_eq!(sync_for(&m.config, "zygo"), Err(SyncOff::NotOn));
      // the user's opt-in holds in a team session
      let user = "[mail]\nhost = \"a\"\n[mail.workspace.zygo]\nsync = \"zygo/mail\"\n";
      let m = load(&ctx(user, Some(team), None).c).unwrap();
      assert_eq!(m.warnings, [MAIL_TEAM]);
      assert_eq!(sync_for(&m.config, "zygo").map(|s| (s.host, s.sync_gap)), Ok(("a".into(), Duration::from_secs(60))));
  }

  #[test]
  fn repo_file_drops_mail_sync_keys() {
      let repo = "version = 1\n[mail]\nhost = \"r\"\nsync_gap = \"1s\"\n[mail.workspace.zygo]\nsync = \"zygo/mail\"\n";
      let m = with_repo_file("", repo);
      assert_eq!(m.warnings, [MAIL_REPO]);
      assert_eq!(m.config.mail, MailConfig::default());
      assert_eq!(m.source_of("mail.workspace.zygo.sync"), Layer::Builtin);
      assert_eq!(sync_for(&m.config, "zygo"), Err(SyncOff::NotOn));
  }
  ```

- [x] 9. In `crates/zygo-config/tests/config_layers.rs:1723` (test `mail_defaults_match_m11`), change the literal to `MailConfig { keep: "30d".into(), max_unread: 500, wake_gap: "30s".into(), host: None, sync_gap: "60s".into(), workspace: BTreeMap::new() }` (`BTreeMap` is already imported at `config_layers.rs:1`). No other line of that file changes.

- [x] 10. Run `cargo test -p zygo-config --test mail_sync_config`; see it fail to compile on the missing `MailSyncWorkspace`, `SyncSettings`, `SyncOff`, `sync_for`, `valid_mail_host`, `valid_sync_branch` and the missing `MailConfig` fields.

- [x] 11. In `crates/zygo-config/src/schema.rs`: add `MailSyncWorkspace` before `MailConfig`, the three `MailConfig` fields and their defaults, `valid_mail_host`, `valid_sync_branch`, `parse_sync_gap` (after `parse_duration`), `SyncSettings`, `SyncOff` and `sync_for`, with the signatures, derives, serde attributes and check order in Produces. Extend the `MailConfig` doc comment: `host`, `sync_gap` and `workspace` are mail sync design S§11; `sync_gap` takes `<n>s`, `<n>m` or `<n>h`.

- [x] 12. In `crates/zygo-config/src/check.rs`: extend the `use crate::schema::{…}` line (`check.rs:7`) with `parse_sync_gap, valid_mail_host, valid_sync_branch`; add the four private message `const`s (texts as in the `config check` rules above); add rules 1-4 after `check.rs:118`, in that order, through the existing `e` closure, with workspace paths from `join(&["mail", "workspace", name.as_str()])`.

- [x] 13. Run `cargo test -p zygo-config --test mail_sync_config`; see all 11 tests pass.

- [x] 14. In `docs/specs/2026-11-22-workspace-mail-design.md` M§11: after the TOML block (line 175), add the line `Mail sync between machines adds \`[mail] host\`, \`[mail] sync_gap\` and the per-workspace table \`[mail.workspace.<name>]\` (keys \`sync\`, \`remote\`, \`host\`); their forms and defaults are in \`docs/specs/2026-11-24-mail-sync-design.md\` S§11.` and append to the change log table the row `| 2026-11-24 | M§11 | mail sync design S§16 | Gains \`host\`, \`sync_gap\` and \`[mail.workspace.<name>]\` (S§11). |`.

- [x] 15. Run the acceptance: `cargo test -p zygo-config --test mail_sync_config && cargo test -p zygo-config`; see all pass, with the `typed.rs` `differential` test and the ML-1 tests in `config_layers.rs` among them. Then `cargo build --workspace` to confirm that no other crate builds a `MailConfig` literal.

- [x] 16. Commit `crates/zygo-config/src/schema.rs`, `crates/zygo-config/src/check.rs`, `crates/zygo-config/tests/mail_sync_config.rs`, `crates/zygo-config/tests/config_layers.rs` and `docs/specs/2026-11-22-workspace-mail-design.md` with the message `MS-3: mail sync config keys, forms and per-workspace resolution`, then a blank line, then the trailers `Zygo-Session: <session>` and `Zygo-Work: <unit>` from the implementing brief, each on its own line. No attribution lines.

Ran (implementation, round 1):
- Step 10: `env -u ZYGO_SESSION_DIR cargo test --locked -p zygo-config --test mail_sync_config` failed to compile on the missing `MailSyncWorkspace`, `SyncSettings`, `SyncOff`, `sync_for`, `valid_mail_host`, `valid_sync_branch` and the `MailConfig` fields `host`, `sync_gap`, `workspace`.
- Step 13: the same command: 11 passed, 0 failed.
- Step 15: `env -u ZYGO_SESSION_DIR cargo test --locked -p zygo-config`: all pass (config_layers 90 passed, 16 ignored; mail_sync_config 11 passed; no failures). `cargo build --locked --workspace`: ok.
- `cargo clippy --locked -p zygo-config --all-targets`: no finding on a changed line. `cargo fmt --check`: the default 100-column width rewraps the whole crate (no `rustfmt.toml`); the changed lines keep the crate's existing wide style.

Fix round 1 (review findings F2, F4, F5, F6; F1 and F3 out of scope):
- [x] F2. `config check` refuses a `[mail.workspace.<n>]` key that is not a W§5 name (`^[a-z0-9-]{1,32}$`): `mail.workspace.<n>: must match ^[a-z0-9-]{1,32}$`, path only, before that table's `sync`/`host` lines. The form comes from `zygo_config::profile::valid_name` (same pattern); `zygo-workspace` depends on `zygo-config`, so its `identity::valid_name` is out of reach. `check_refuses_forms_at_the_edges` now uses `my-ws` and refuses `""`, `My WS`, `a.b`, `a_b` and 33 characters. Seen to fail first (`left: []`), then pass.
- [x] F4. Removed the three `source_of(...) == Layer::Builtin` assertions; `defaults_and_the_s11_example` now asserts the builtin table holds `mail.sync_gap = "60s"`.
- [x] F5. The `typed.rs` `differential` sample has `[mail]` with every key and `[mail.workspace.x]` with `sync` and `remote`; `cargo test -p zygo-config --lib differential` passes.
- [x] F6. New tests `wrong_type_in_a_workspace_table_stops_the_load` (`sync = 4242` gives `ConfigError::Parse` with message `mail.workspace.zygo.sync at line 2: expected a string, found an integer`, one line, no value) and `repo_file_cannot_change_the_user_opt_in` (`.zygo.toml` `host`/`sync_gap` dropped; user `a`/`5m` kept, source `User`). Both passed on first run: the code already behaved; they close coverage gaps.
- Ran: `env -u ZYGO_SESSION_DIR cargo test --locked -p zygo-config --test mail_sync_config`: 13 passed, 0 failed. `env -u ZYGO_SESSION_DIR cargo test --locked -p zygo-config`: 258 passed, 0 failed, 18 ignored. `cargo clippy --locked -p zygo-config --all-targets`: no finding on a changed line.
