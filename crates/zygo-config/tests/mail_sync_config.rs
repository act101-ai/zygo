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

#[test]
fn defaults_and_the_s11_example() {
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
}

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

#[test]
fn check_refuses_forms_at_the_edges() {
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
}

#[test]
fn check_names_mail_host_when_sync_has_no_host() {
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
}

#[test]
fn workspace_host_overrides_mail_host() {
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
}

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
