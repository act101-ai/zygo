use std::{ffi::OsString, fs, path::{Path, PathBuf}, process::Command, sync::{Mutex, MutexGuard}, time::{Duration, Instant}};
use zygo_config::WorkConfig;
use zygo_work::{current, repo, sources, WorkItem};

static LOCK: Mutex<()> = Mutex::new(()); // env is process-wide: every test holds it

struct Env { _g: MutexGuard<'static, ()>, tmp: tempfile::TempDir, git: PathBuf, old_path: Option<OsString> }
impl Env {
  fn new() -> Env {
      let g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
      let old_path = std::env::var_os("PATH");
      let git = std::env::split_paths(&old_path.clone().unwrap_or_default())
          .flat_map(|d| [d.join("git"), d.join("git.exe")]).find(|p| p.is_file()).expect("git on PATH");
      let tmp = tempfile::tempdir().unwrap();
      fs::write(tmp.path().join("gitconfig"), "").unwrap();
      fs::create_dir(tmp.path().join("bin")).unwrap();
      let ceiling = tmp.path().canonicalize().unwrap().parent().unwrap().to_path_buf();
      unsafe {
          std::env::set_var("GIT_CONFIG_GLOBAL", tmp.path().join("gitconfig"));
          std::env::set_var("GIT_CONFIG_NOSYSTEM", "1");
          std::env::set_var("GIT_CEILING_DIRECTORIES", ceiling); // a dir outside any test repo is never inside the developer's repo
      }
      let e = Env { _g: g, tmp, git, old_path };
      e.set_path();
      e
  }
  fn bin(&self) -> PathBuf { self.tmp.path().join("bin") }
  fn set_path(&self) { // unix: PATH is `bin/` only (wrapper `git` inside), so a real bd/gh is never found
      #[cfg(unix)] { self.script("git", &format!("exec '{}' \"$@\"", self.git.display())); unsafe { std::env::set_var("PATH", self.bin()) } }
      #[cfg(windows)] { let p = std::env::join_paths([self.bin(), self.git.parent().unwrap().to_path_buf()]).unwrap(); unsafe { std::env::set_var("PATH", p) } }
  }
  #[cfg(unix)]
  fn script(&self, name: &str, body: &str) {
      use std::os::unix::fs::PermissionsExt;
      let p = self.bin().join(name);
      fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
      fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
  }
}
impl Drop for Env {
  fn drop(&mut self) { unsafe {
      match &self.old_path { Some(p) => std::env::set_var("PATH", p), None => std::env::remove_var("PATH") }
      for k in ["GIT_CONFIG_GLOBAL", "GIT_CONFIG_NOSYSTEM", "GIT_CEILING_DIRECTORIES"] { std::env::remove_var(k); }
  } }
}
fn git(env: &Env, dir: &Path, args: &[&str]) {
  let st = Command::new(&env.git).current_dir(dir)
      .args(["-c", "commit.gpgsign=false", "-c", "user.name=t", "-c", "user.email=t@example.com"]).args(args).status().unwrap();
  assert!(st.success(), "git {args:?}");
}
fn repo_with(env: &Env, name: &str, branch: &str, remotes: &[&str]) -> PathBuf {
  let d = env.tmp.path().join(name);
  fs::create_dir(&d).unwrap();
  let d = d.canonicalize().unwrap();
  git(env, &d, &["init", "-q", "-b", branch]);
  for r in remotes { git(env, &d, &["config", "--add", "remote.origin.url", r]); }
  git(env, &d, &["commit", "-q", "--allow-empty", "-m", "i"]);
  d
}
// Tests not about timing use a 30 s budget so machine load never changes their outcome;
// only the timeout tests use the production 2 s path (`current::resolve`, `repo::repository_id`).
const LONG: Duration = Duration::from_secs(30);
fn resolve_noted(d: &Path, c: &WorkConfig) -> (Option<WorkItem>, Vec<String>) { current::resolve_noted_with_budget(d, c, LONG) }
fn resolve(d: &Path, c: &WorkConfig) -> Option<WorkItem> { resolve_noted(d, c).0 }
fn set(d: &Path, c: &WorkConfig, a: &str) -> anyhow::Result<WorkItem> { current::set_with_budget(d, c, a, LONG) }
fn clear(d: &Path) -> anyhow::Result<()> { current::clear_with_budget(d, LONG) }
fn repository_id(d: &Path) -> Option<String> { repo::repository_id_with_budget(d, LONG) }
fn cfg(s: &[&str]) -> WorkConfig { WorkConfig { sources: s.iter().map(|x| x.to_string()).collect() } }
fn all() -> WorkConfig { WorkConfig::default() }
fn plain(env: &Env) -> PathBuf { let p = env.tmp.path().join("plain"); fs::create_dir_all(&p).unwrap(); p }
#[cfg(unix)]
fn pid_gone(pid: &str) -> bool { // `kill -0` succeeds on a zombie, so this proves the child was reaped
  !Command::new("/bin/sh").args(["-c", &format!("kill -0 {pid}")]).status().unwrap().success()
}
#[test]
fn normalize_remote_table() {
  let long = |n: usize| format!("h.io/{}", "a".repeat(n - 5));
  let cases: Vec<(String, Option<String>)> = vec![
      ("https://github.com/Acme/Widgets.git".into(), Some("github.com/acme/widgets".into())),
      ("https://user:tok@github.com:8443/Acme/Widgets/".into(), Some("github.com/acme/widgets".into())),
      ("https://user:p@ss@host.io/a/b".into(), Some("host.io/a/b".into())),     // last `@` ends userinfo
      ("ssh://git@Example.COM:2222/team/sub/Repo.git".into(), Some("example.com/team/sub/repo".into())),
      ("ssh://git@host.io/a/b.git/".into(), Some("host.io/a/b".into())),
      ("https://host.io/a/B.GIT".into(), Some("host.io/a/b".into())),           // `.GIT` stripped after lower-casing
      ("git@github.com:Acme/Widgets.git".into(), Some("github.com/acme/widgets".into())),
      ("host.example:path/to/repo".into(), Some("host.example/path/to/repo".into())),
      ("git@host.example:/abs/repo.git/".into(), Some("host.example/abs/repo".into())),
      ("https://[::1]:8080/a/b".into(), None),    // bracketed IPv6 host fails the host class: none, by rule
      ("file:///srv/git/repo.git".into(), None),  // file scheme: none by rule
      ("FILE://host.io/a/b".into(), None),
      ("/srv/git/repo.git".into(), None),
      ("C:/Users/me/repo".into(), None),          // one-letter host + `:` is a Windows drive path
      ("C:\\repo".into(), None),
      ("https://host.io/".into(), None),
      ("https://host.io:notaport/a/b".into(), None),
      ("https://my_host/a/b".into(), None),
      ("https://host.io/a b/c".into(), None),
      ("https://host.io/a/b?x=1".into(), None),
      ("".into(), None),
      ("./a:b".into(), None),                     // `/` before the first `:`: a local path, not scp form
      ("../foo:bar".into(), None),
      ("/srv/git:repo".into(), None),
      (".foo:bar".into(), None),                  // `.`-led host part
      ("https://h.io/a/../b".into(), None),       // `.`/`..` path segments
      ("h.io:a/../b".into(), None),
      ("https://h.io/./a/b".into(), None),
      ("https://github.com/\u{212A}elvin/x".into(), None),   // Kelvin sign: non-ASCII, never folded to `k`
      ("git@github.com:\u{212A}/x".into(), None),
      (format!("https://{}", long(128)), Some(long(128))),
      (format!("https://{}", long(129)), None),
  ];
  for (url, want) in cases { assert_eq!(repo::normalize_remote(&url), want, "{url}"); }
}
#[test]
fn repository_id_reads_first_origin_url() {
  let env = Env::new();
  let d = repo_with(&env, "r", "main", &["git@github.com:Acme/Widgets.git", "https://other.example/x/y"]);
  assert_eq!(repository_id(&d).as_deref(), Some("github.com/acme/widgets"));
  fs::create_dir(d.join("sub")).unwrap();
  assert_eq!(repository_id(&d.join("sub")).as_deref(), Some("github.com/acme/widgets"));
}
#[test]
fn repository_id_none_without_remote_or_repo() {
  let env = Env::new();
  assert_eq!(repository_id(&repo_with(&env, "r", "main", &[])), None);
  assert_eq!(repository_id(&plain(&env)), None);   // GIT_CEILING_DIRECTORIES keeps discovery inside the temp dir
}
#[cfg(unix)]
#[test]
fn git_calls_are_pinned_never_run_repo_config_and_are_bounded() {
  let env = Env::new();
  let d = repo_with(&env, "r", "main", &["https://github.com/a/b"]);
  let marker = env.tmp.path().join("fsmonitor-ran");
  env.script("fsm", &format!("/usr/bin/touch '{}'", marker.display()));
  git(&env, &d, &["config", "core.fsmonitor", env.bin().join("fsm").to_str().unwrap()]);   // repository-controlled program
  let log = env.tmp.path().join("git.log");
  env.script("git", &format!("echo \"$@\" >> '{}'\nexec '{}' \"$@\"", log.display(), env.git.display()));
  repository_id(&d);
  resolve(&d, &all());
  assert!(!marker.exists(), "repo-controlled core.fsmonitor must never run");
  let text = fs::read_to_string(&log).unwrap();
  assert!(!text.is_empty());
  assert!(text.lines().all(|l| l.starts_with("-c core.fsmonitor=false ")), "{text}");
  // the fixture is live: an index-refreshing command without the prefix runs the hook; with zygo's prefix it does not
  fs::write(d.join("f"), "a").unwrap();
  git(&env, &d, &["-c", "core.fsmonitor=false", "add", "f"]);
  git(&env, &d, &["-c", "core.fsmonitor=false", "commit", "-q", "-m", "f"]);
  let status = |pre: &[&str]| Command::new(&env.git).args(pre).arg("-C").arg(&d).args(["status", "--porcelain"]).output().unwrap();
  status(&["-c", "core.fsmonitor=false"]);
  assert!(!marker.exists(), "zygo's prefix must disable core.fsmonitor");
  status(&[]);
  assert!(marker.exists(), "control: without the prefix the hook runs");
  // a git that hangs is cut at the 2 s bound, and reaped
  let pidfile = env.tmp.path().join("git.pid");
  // The child would run 12 s; returning well before that proves the 2 s bound cut it (a tight wall-clock margin flaked
  // under a parallel workspace run).
  env.script("git", &format!("echo $$ > '{}'\nexec /bin/sleep 12", pidfile.display()));
  let t = Instant::now();
  assert_eq!(repo::repository_id(&d), None);
  assert!(t.elapsed() < Duration::from_secs(8), "{:?}", t.elapsed());
  assert!(pid_gone(fs::read_to_string(&pidfile).unwrap().trim()));
}
#[cfg(unix)]
#[test]
fn a_grandchild_holding_stdout_does_not_hold_the_caller_past_the_bound() {
  // non-`exec` script: the background sleep inherits the stdout pipe and outlives the killed shell;
  // the drain thread is abandoned after the bound (documented, bounded leak of one thread per timeout)
  let env = Env::new();
  let d = repo_with(&env, "r", "42-fix", &["https://github.com/Acme/Widgets"]);
  env.script("gh", "/bin/sleep 10 &\n/bin/sleep 12");
  let t = Instant::now();
  let it = current::resolve(&d, &all()).unwrap();
  assert!(t.elapsed() < Duration::from_secs(8), "{:?}", t.elapsed());
  assert!(it.lookup_failed.unwrap().contains("timed out"));
}
#[test]
fn github_branch_gives_owner_repo_n_and_missing_gh_keeps_id() {          // §20 #35
  let env = Env::new();
  let d = repo_with(&env, "r", "42-fix-x", &["https://github.com/Acme/Widgets.git"]);
  let it = resolve(&d, &all()).unwrap();
  assert_eq!((it.source.as_str(), it.id.as_str(), it.title.clone()), ("github", "acme/widgets#42", None));
  assert_eq!(it.lookup_failed.as_deref(), Some("tool not found"));
}
#[cfg(unix)]
#[test]
fn github_lookup_reads_title_url_state() {
  let env = Env::new();
  let d = repo_with(&env, "r", "gh-7-thing", &["git@github.com:Acme/Widgets.git"]);
  let args = env.tmp.path().join("gh.args");
  env.script("gh", &format!("echo \"$@\" > '{}'\nprintf '%s' '{{\"title\":\"Fix x\",\"url\":\"https://github.com/acme/widgets/issues/7\",\"state\":\"OPEN\"}}'", args.display()));
  let it = resolve(&d, &all()).unwrap();
  assert_eq!((it.id.as_str(), it.title.as_deref(), it.status.as_deref()), ("acme/widgets#7", Some("Fix x"), Some("OPEN")));
  assert_eq!(it.url.as_deref(), Some("https://github.com/acme/widgets/issues/7"));
  assert_eq!(it.lookup_failed, None);
  assert_eq!(fs::read_to_string(args).unwrap().trim(), "issue view 7 --json title,url,state -R acme/widgets");
}
#[test]
fn github_needs_the_github_host_and_plain_ascii_numbers() {
  let env = Env::new();
  let src = |name: &str, branch: &str, remote: &str| {
      let d = repo_with(&env, name, branch, &[remote]);
      let it = resolve(&d, &all()).unwrap();
      (it.source, it.id)
  };
  assert_eq!(src("gl", "42-fix", "https://gitlab.com/acme/widgets"), ("branch".into(), "42-fix".into()));
  let u = repo_with(&env, "u", "٤٢-fix", &["https://github.com/acme/widgets"]);                                  // Arabic-Indic digits
  assert_eq!(resolve(&u, &all()), None);
  assert_eq!(src("date", "20241118-backup", "https://github.com/acme/widgets").0, "branch");
  assert_eq!(src("seven", "1234567-x", "https://github.com/acme/widgets").0, "branch");
  assert_eq!(src("six", "123456-x", "https://github.com/acme/widgets"), ("github".into(), "acme/widgets#123456".into()));
  assert_eq!(src("p1", "issue-7", "https://github.com/acme/widgets").1, "acme/widgets#7");
  assert_eq!(src("p2", "gh-9-x", "https://github.com/acme/widgets").1, "acme/widgets#9");
  assert_eq!(src("deep", "42-x", "https://github.com/a/b/c").0, "branch");                                        // not exactly owner/repo
}
#[test]
fn beads_branch_gives_bd_id_and_missing_bd_keeps_id() {                  // §20 #35
  let env = Env::new();
  let d = repo_with(&env, "r", "bd-a1b2-fix-it", &[]);
  fs::create_dir(d.join(".beads")).unwrap();
  let it = resolve(&d, &all()).unwrap();
  assert_eq!((it.source.as_str(), it.id.as_str(), it.title.clone()), ("beads", "bd-a1b2", None));
  assert!(it.lookup_failed.is_some());
}
#[test]
fn beads_rules_directory_prefix_digit_and_child_ids() {
  let env = Env::new();
  let id = |name: &str, branch: &str, cfgyaml: Option<&str>| {
      let d = repo_with(&env, name, branch, &[]);
      fs::create_dir(d.join(".beads")).unwrap();
      if let Some(y) = cfgyaml { fs::write(d.join(".beads/config.yaml"), y).unwrap(); }
      let it = resolve(&d, &all()).unwrap();
      (it.source, it.id)
  };
  let no_dir = repo_with(&env, "nodir", "bd-a1b2-fix", &[]);
  assert_eq!(resolve(&no_dir, &all()).unwrap().source, "branch");
  assert_eq!(id("p", "proj-k9x2-y", Some("issue-prefix: \"proj\"\n")), ("beads".into(), "proj-k9x2".into()));
  assert_eq!(id("q", "bd-a1b2-fix", Some("issue-prefix: proj\n")).0, "branch");                  // other prefix
  assert_eq!(id("fp1", "bd-fix-it", None).0, "branch");                                           // no digit
  assert_eq!(id("fp2", "bd-update-docs", None).0, "branch");
  assert_eq!(id("long", "bd-a1b2c3d4e5", None).0, "branch");                                      // hash > 8
  assert_eq!(id("child", "bd-a1b2.3.14-x", None), ("beads".into(), "bd-a1b2.3.14".into()));
  assert_eq!(id("meta", "x.y-a1b2", Some("issue-prefix: x.y\n")).0, "branch");                    // invalid prefix falls back to bd; no match
}
#[cfg(unix)]
#[test]
fn beads_lookup_is_read_only_and_reads_title_status_acceptance() {
  let env = Env::new();
  let d = repo_with(&env, "r", "bd-a1b2", &[]);
  fs::create_dir(d.join(".beads")).unwrap();
  let args = env.tmp.path().join("bd.args");
  env.script("bd", &format!("echo \"$@\" > '{}'\nprintf '%s' '[{{\"id\":\"bd-a1b2\",\"title\":\"T\",\"status\":\"open\",\"acceptance_criteria\":\"cargo test\"}}]'", args.display()));
  let it = resolve(&d, &all()).unwrap();
  assert_eq!((it.title.as_deref(), it.status.as_deref(), it.check_hint.as_deref()), (Some("T"), Some("open"), Some("cargo test")));
  assert_eq!(fs::read_to_string(args).unwrap().trim(), "--readonly show bd-a1b2 --json");
}
#[cfg(unix)]
#[test]
fn lookup_is_bounded_and_bad_output_keeps_the_id() {
  let env = Env::new();
  let d = repo_with(&env, "r", "42-fix-x", &["https://github.com/Acme/Widgets.git"]);
  env.script("gh", "exec /bin/sleep 5");
  let t = Instant::now();
  let it = current::resolve(&d, &all()).unwrap();
  assert!(t.elapsed() < Duration::from_millis(2500));
  assert_eq!((it.id.as_str(), it.title.clone()), ("acme/widgets#42", None));
  assert_eq!(it.lookup_failed.as_deref(), Some("timed out"));
  env.script("gh", "printf 'not json'");
  let it = resolve(&d, &all()).unwrap();
  assert_eq!((it.title.clone(), it.lookup_failed.as_deref()), (None, Some("unparseable output")));
  env.script("gh", "exit 1");
  assert_eq!(resolve(&d, &all()).unwrap().lookup_failed.as_deref(), Some("failed"));
}
#[test]
fn branch_is_the_fallback_and_order_comes_from_config() {
  let env = Env::new();
  let d = repo_with(&env, "r", "feature/x", &[]);
  let it = resolve(&d, &all()).unwrap();
  assert_eq!((it.source.as_str(), it.id.as_str(), it.title.clone(), it.lookup_failed), ("branch", "feature/x", None, None));
  let g = repo_with(&env, "g", "42-fix-x", &["https://github.com/Acme/Widgets.git"]);
  assert_eq!(resolve(&g, &cfg(&["branch", "github"])).unwrap().source, "branch");
  assert_eq!(resolve(&d, &cfg(&["github"])), None);
  assert_eq!(resolve(&d, &cfg(&["nonsense", "branch"])).unwrap().source, "branch");
  assert_eq!(resolve(&plain(&env), &all()), None);
}
fn graph(refs2: &str) -> String {
  format!("# prog Work Graph\n\n<!-- work-graph -->\n\n## Nodes\n\n| ID | Node | Depends | State | Model | Refs |\n|---|---|---|---|---|---|\n\
| N1 | First node | — | IN-PROGRESS | opus | wt .worktrees/prog/N1 |\n\
| N2 | Second node | N1 | IN-PROGRESS | sonnet | {refs2} |\n\
| N3 | Third node | N2 | TODO | sonnet | — |\n")
}
const NODE2: &str = "# N2 — Second node\n\n**Spec:** x\n**Files:** `crates/a/src/one.rs`, `crates/a/tests/t.rs` (note)\n**Acceptance:** `cargo test -p a --test t` → all pass (`x`)\n";

/// Main checkout `name` plus a linked worktree `.worktrees/prog/N2`; the graph exists ONLY in the main checkout.
fn graph_repo(env: &Env, name: &str, g: &str) -> (PathBuf, PathBuf) {
  let d = repo_with(env, name, "main", &[]);
  git(env, &d, &["worktree", "add", "-q", ".worktrees/prog/N2", "-b", "n2"]);
  fs::create_dir_all(d.join("docs/work/prog/nodes")).unwrap();
  fs::write(d.join("docs/work/prog/graph.md"), g).unwrap();
  fs::write(d.join("docs/work/prog/nodes/N2.md"), NODE2).unwrap();
  let wt = d.join(".worktrees/prog/N2").canonicalize().unwrap();
  (d, wt)
}
#[test]
fn work_graph_in_progress_node_naming_the_checkout() {                   // acceptance fixture
  let env = Env::new();
  let (main, wt) = graph_repo(&env, "r", &graph("wt .worktrees/prog/N2"));
  let it = resolve(&wt, &all()).unwrap();
  assert_eq!((it.source.as_str(), it.id.as_str(), it.title.as_deref(), it.status.as_deref()),
             ("work-graph", "prog/N2", Some("Second node"), Some("IN-PROGRESS")));
  assert_eq!(it.check_hint.as_deref(), Some("cargo test -p a --test t"));                 // the command only
  assert_eq!(it.files_hint, vec!["crates/a/src/one.rs".to_string(), "crates/a/tests/t.rs".to_string()]);
  assert_eq!(it.lookup_failed, None);
  fs::create_dir_all(wt.join("crates/a")).unwrap();
  assert_eq!(resolve(&wt.join("crates/a"), &all()).unwrap().id, "prog/N2");      // from a subdirectory
  assert_eq!(resolve(&main, &all()).unwrap().source, "branch");                  // main checkout named by no node
}
#[test]
fn work_graph_refs_forms_are_all_accepted() {
  let env = Env::new();
  let (main, wt) = graph_repo(&env, "r", &graph("x"));
  let abs = format!("wt {}", wt.display());
  for refs in ["wt .worktrees/prog/N2", "wt ./.worktrees/prog/N2", "wt .worktrees/prog/N2/", abs.as_str(),
               "wt .worktrees\\prog\\N2", "wt .worktrees/prog/N2; merged later", "abc123..def, wt .worktrees/prog/N2"] {
      fs::write(main.join("docs/work/prog/graph.md"), graph(refs)).unwrap();
      assert_eq!(resolve(&wt, &all()).unwrap().id, "prog/N2", "{refs}");
  }
  for refs in ["—", "wt .worktrees/prog/N9", "wt .worktrees/prog/N2x"] {
      fs::write(main.join("docs/work/prog/graph.md"), graph(refs)).unwrap();
      assert_eq!(resolve(&wt, &all()).unwrap().source, "branch", "{refs}");
  }
}
#[test]
fn work_graph_two_matches_give_no_item_and_one_note() {
  let env = Env::new();
  let (main, wt) = graph_repo(&env, "r", &graph("wt .worktrees/prog/N2"));
  let g = graph("wt .worktrees/prog/N2").replace("| N1 | First node | — | IN-PROGRESS | opus | wt .worktrees/prog/N1 |",
                                                 "| N1 | First node | — | IN-PROGRESS | opus | wt ./.worktrees/prog/N2 |");
  fs::write(main.join("docs/work/prog/graph.md"), g).unwrap();
  let (it, notes) = resolve_noted(&wt, &all());
  assert_eq!(it.unwrap().source, "branch");
  assert_eq!(notes.len(), 1);
  assert!(notes[0].contains("prog/N1") && notes[0].contains("prog/N2"));
}
#[test]
fn work_graph_needs_its_marker_and_files_hint_is_capped() {
  let env = Env::new();
  let (main, wt) = graph_repo(&env, "r", &graph("wt .worktrees/prog/N2").replace("<!-- work-graph -->", ""));
  assert_eq!(resolve(&wt, &all()).unwrap().source, "branch");
  fs::write(main.join("docs/work/prog/graph.md"), graph("wt .worktrees/prog/N2")).unwrap();
  let many: String = (0..70).map(|i| format!("`f{i}.rs`")).collect::<Vec<_>>().join(", ");
  fs::write(main.join("docs/work/prog/nodes/N3.md"), format!("**Files:** {many}\n**Acceptance:** none\n")).unwrap();
  let it = set(&wt, &all(), "prog/N3").unwrap();
  assert_eq!(it.files_hint.len(), 64);
  assert_eq!(it.check_hint, None);                                                        // no backticked span
}
#[test]
fn explicit_setting_beats_detection_is_shared_atomic_and_clears() {
  let env = Env::new();
  let (main, wt) = graph_repo(&env, "r", &graph("wt .worktrees/prog/N2"));
  let it = set(&wt, &all(), "prog/N3").unwrap();
  assert_eq!((it.source.as_str(), it.id.as_str(), it.title.as_deref(), it.status.as_deref()),
             ("work-graph", "prog/N3", Some("Third node"), Some("TODO")));
  set(&wt, &all(), "prog/N3").unwrap();                                           // replacing an existing file
  let file = main.join(".git/zygo-work");
  assert_eq!(fs::read_to_string(&file).unwrap(), "work-graph prog/N3\n");
  let stray = fs::read_dir(main.join(".git")).unwrap().filter_map(|e| e.ok())
      .filter(|e| e.file_name().to_string_lossy().starts_with("zygo-work.")).count();
  assert_eq!(stray, 0, "temp file must be renamed away");
  assert_eq!(resolve(&wt, &all()).unwrap().id, "prog/N3");                        // beats detection (prog/N2)
  assert_eq!(resolve(&main, &all()).unwrap().id, "prog/N3");                      // linked worktrees share it
  clear(&wt).unwrap();
  assert!(!file.exists());
  assert_eq!(resolve(&wt, &all()).unwrap().id, "prog/N2");
  clear(&wt).unwrap();                                                            // twice is fine
}
#[test]
fn a_corrupt_or_unknown_setting_is_unset_with_one_note() {
  let env = Env::new();
  let d = repo_with(&env, "r", "feature/x", &[]);
  for bad in ["", "garbage", "nosuch id\n", "branch\n", "\u{0}\u{1}"] {
      fs::write(d.join(".git/zygo-work"), bad).unwrap();
      let (it, notes) = resolve_noted(&d, &all());
      assert_eq!(it.unwrap().id, "feature/x", "{bad:?}");
      assert_eq!(notes.len(), 1, "{bad:?}");
  }
  fs::remove_file(d.join(".git/zygo-work")).unwrap();
  assert!(resolve_noted(&d, &all()).1.is_empty());
}
#[test]
fn set_and_clear_outside_a_repository_fail() {
  let env = Env::new();
  let p = plain(&env);
  assert!(set(&p, &all(), "x").unwrap_err().to_string().contains("not in a git repository"));
  assert!(clear(&p).unwrap_err().to_string().contains("not in a git repository"));
}
#[test]
fn set_normalizes_arguments_per_source() {
  let env = Env::new();
  let d = repo_with(&env, "r", "main", &["https://github.com/Acme/Widgets"]);
  fs::create_dir(d.join(".beads")).unwrap();
  let id = |a: &str| { let i = set(&d, &all(), a).unwrap(); (i.source, i.id) };
  assert_eq!(id("#42"), ("github".into(), "acme/widgets#42".into()));
  assert_eq!(id("gh-7"), ("github".into(), "acme/widgets#7".into()));
  assert_eq!(id("Other/Repo#9"), ("github".into(), "other/repo#9".into()));
  assert_eq!(id("bd-a1b2"), ("beads".into(), "bd-a1b2".into()));
  assert_eq!(id("my-branch"), ("branch".into(), "my-branch".into()));
  assert!(set(&d, &all(), "a b").is_err());
  assert!(set(&d, &cfg(&["github"]), "my-branch").is_err());
  let gl = repo_with(&env, "gl", "main", &["https://gitlab.com/a/b"]);
  assert!(set(&gl, &all(), "#42").is_err());                                       // not github.com; `#42` is no branch name
}
#[test]
fn unit_ids_follow_the_13_1_grammar() {
  let re = regex::Regex::new(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,47}(:[A-Za-z0-9][A-Za-z0-9._-]{0,47}){1,2}$").unwrap();
  let w = |s: &str, id: &str| WorkItem { source: s.into(), id: id.into(), title: None, url: None, status: None,
                                         check_hint: None, files_hint: vec![], lookup_failed: None };
  assert_eq!(w("github", "acme/widgets#42").unit(), "github:acme-widgets-42");
  assert_eq!(w("work-graph", "zygo-v1/N12").unit(), "work-graph:zygo-v1-N12");
  assert_eq!(w("branch", "feature/x").unit(), "branch:feature-x");
  assert_eq!(w("branch", "///").unit(), "branch:x---");
  assert_eq!(w("branch", "é").unit(), "branch:x-");
  assert_eq!(w("branch", "").unit(), "branch:x");
  assert_eq!(w("branch", &"é".repeat(60)).unit().chars().count(), "branch:".len() + 48);
  for it in [w("branch", &"a".repeat(100)), w("branch", "/-weird é"), w("beads", "bd-a1b2")] {
      let u = it.unit();
      assert!(re.is_match(&u) && u.len() <= 128, "{u}");
  }
}
#[test]
fn stderr_lines_are_one_per_note_plus_one_for_a_failed_lookup() {
  let w = |f: Option<&str>| WorkItem { source: "github".into(), id: "acme/widgets#42".into(), title: None, url: None,
      status: None, check_hint: None, files_hint: vec![], lookup_failed: f.map(String::from) };
  assert!(current::stderr_lines(Some(&w(None)), &[]).is_empty());
  assert!(current::stderr_lines(None, &[]).is_empty());
  assert_eq!(current::stderr_lines(Some(&w(Some("timed out"))), &[]),
             vec!["zygo: work item lookup failed for acme/widgets#42: timed out".to_string()]);
  assert_eq!(current::stderr_lines(Some(&w(Some("failed"))), &["zygo: note".to_string()]).len(), 2);
}
#[test]
fn source_trait_names() {
  for n in ["work-graph", "beads", "github", "branch"] { assert_eq!(sources::by_name(n).unwrap().name(), n); }
  assert!(sources::by_name("jira").is_none());
}

#[test]
fn work_graph_reads_only_the_nodes_table() {
  let env = Env::new();
  let table = |id: &str| format!("| ID | Node | Depends | State | Model | Refs |\n|---|---|---|---|---|---|\n\
| {id} | Example | — | IN-PROGRESS | — | wt .worktrees/prog/N2 |\n");
  let g = graph("wt .worktrees/prog/N2").replace("## Nodes", &format!("## Intro\n\n{}\n## Nodes", table("B")))
      + &format!("\n## Legend\n\n{}", table("A"));
  let (_main, wt) = graph_repo(&env, "r", &g);
  let (it, notes) = resolve_noted(&wt, &all());
  assert_eq!(it.unwrap().id, "prog/N2");
  assert!(notes.is_empty(), "{notes:?}");
  for id in ["prog/A", "prog/B"] { assert!(set(&wt, &cfg(&["work-graph"]), id).is_err(), "{id}"); }
  assert_eq!(set(&wt, &cfg(&["work-graph"]), "prog/N3").unwrap().id, "prog/N3");
}
#[test]
fn work_graph_refs_path_ends_at_a_comma() {
  let env = Env::new();
  let (main, _wt) = graph_repo(&env, "r", &graph("wt ../w2, PR #3"));
  git(&env, &main, &["worktree", "add", "-q", "../w2", "-b", "w2"]);
  let w2 = env.tmp.path().join("w2").canonicalize().unwrap();
  assert_eq!(resolve(&w2, &all()).unwrap().id, "prog/N2");
}
#[test]
fn beads_hyphenated_prefix_comment_and_digitless_explicit_ids() {
  let env = Env::new();
  let id = |name: &str, branch: &str, cfgyaml: Option<&str>| {
      let d = repo_with(&env, name, branch, &[]);
      fs::create_dir(d.join(".beads")).unwrap();
      if let Some(y) = cfgyaml { fs::write(d.join(".beads/config.yaml"), y).unwrap(); }
      let it = resolve(&d, &all()).unwrap();
      (it.source, it.id)
  };
  assert_eq!(id("hp", "my-proj-k9x2-y", Some("issue-prefix: my-proj  # team prefix\n")), ("beads".into(), "my-proj-k9x2".into()));
  assert_eq!(id("q", "proj-k9x2", Some("issue-prefix: \"proj\" # quoted\n")), ("beads".into(), "proj-k9x2".into()));
  assert_eq!(id("d0", "2fa-abc1", Some("issue-prefix: 2fa\n")), ("beads".into(), "2fa-abc1".into()));
  assert_eq!(id("nd", "bd-abcd", None).0, "branch");                                          // a branch still needs a digit
  let d = repo_with(&env, "set", "main", &[]);
  fs::create_dir(d.join(".beads")).unwrap();
  let set = |a: &str| { let i = set(&d, &all(), a).unwrap(); (i.source, i.id) };
  assert_eq!(set("bd-abcd"), ("beads".into(), "bd-abcd".into()));                            // explicit: no digit needed
  assert_eq!(set("bd-ab"), ("branch".into(), "bd-ab".into()));                                // hash < 3
}
#[test]
fn set_refuses_non_ascii_repo_slugs() {
  let env = Env::new();
  let d = repo_with(&env, "r", "main", &["https://github.com/Acme/Widgets"]);
  assert!(set(&d, &all(), "Other/\u{212A}#9").is_err());
  assert!(set(&d, &all(), "\u{212A}/x#9").is_err());
}
#[test]
fn git_env_from_the_caller_is_scrubbed() {
  let env = Env::new();
  let d = repo_with(&env, "r", "feature/mine", &["https://github.com/acme/mine"]);
  let other = repo_with(&env, "o", "feature/other", &["https://github.com/acme/other"]);
  unsafe {
      std::env::set_var("GIT_DIR", other.join(".git"));
      std::env::set_var("GIT_WORK_TREE", &other);
      std::env::set_var("GIT_INDEX_FILE", other.join(".git/index"));
      std::env::set_var("GIT_COMMON_DIR", other.join(".git"));
  }
  let id = repository_id(&d);
  let it = resolve(&d, &cfg(&["branch"]));
  unsafe { for k in ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_COMMON_DIR"] { std::env::remove_var(k); } }
  assert_eq!(id.as_deref(), Some("github.com/acme/mine"));
  assert_eq!(it.unwrap().id, "feature/mine");
}
#[test]
fn branch_source_refuses_unusable_branch_names() {
  let env = Env::new();
  for (name, b) in [("long", "a".repeat(250)), ("u", "é-x".to_string())] {
      let d = repo_with(&env, name, &b, &[]);
      let (it, notes) = resolve_noted(&d, &cfg(&["branch"]));
      assert_eq!(it, None, "{name}");
      assert_eq!(notes.len(), 1, "{name}: {notes:?}");
  }
}
#[test]
fn long_unit_ids_keep_a_hash_so_they_do_not_collide() {
  let w = |id: &str| WorkItem { source: "branch".into(), id: id.into(), title: None, url: None, status: None,
                                check_hint: None, files_hint: vec![], lookup_failed: None };
  assert_eq!(w(&"a".repeat(48)).unit(), format!("branch:{}", "a".repeat(48)));               // no truncation, no hash
  let (a, b) = (w(&"a".repeat(60)).unit(), w(&format!("{}b", "a".repeat(59))).unit());
  assert_ne!(a, b);
  use sha2::{Digest, Sha256};
  let h = hex::encode(Sha256::digest("a".repeat(60).as_bytes()));
  assert_eq!(a, format!("branch:{}-{}", "a".repeat(39), &h[..8]));
  assert_eq!(b.len(), "branch:".len() + 48);
}
#[cfg(unix)]
#[test]
fn one_budget_per_operation() {
  let env = Env::new();
  let d = repo_with(&env, "r", "42-fix", &["https://github.com/Acme/Widgets"]);
  env.script("git", &format!("/bin/sleep 0.1\nexec '{}' \"$@\"", env.git.display()));
  assert_eq!(current::resolve(&d, &all()).unwrap().id, "acme/widgets#42");                // short calls fit the 2 s budget
  env.script("git", &format!("/bin/sleep 1.5\nexec '{}' \"$@\"", env.git.display()));
  let t = Instant::now();
  current::resolve(&d, &all());                                                              // 1.5 s per call, 4 calls if each had its own 2 s bound (6 s); one shared budget ends it at 2 s
  let e = t.elapsed();
  assert!(e >= Duration::from_secs(2) && e < Duration::from_secs(4), "{e:?}");               // 2 s of budget used, 2 s of slack below the 6 s of per-call bounds
}
