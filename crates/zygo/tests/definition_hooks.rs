//! WD-19: hooks, the MCP server and the status line read nothing from `.zygo/` (R-C8; R§8 "Not a layer"; R§10 row
//! "Hooks, the MCP server, the status line"; R§18 item 20; review focus 38). p-tests, every OS: source scans of the
//! crates that the hooks and the MCP server link, of the slash verbs that `zygo hook prompt` runs, of `load`, of the
//! one `zygo-workspace` caller of the read, and for an absolute `git` path. h-tests, unix (workspace state, a `sh`
//! fake `git`, process groups; OP5): each hook registration, each slash verb, each OpenCode plugin call and
//! `zygo mcp`, in a repository with an approved `.zygo/` and in the same repository without it, with three checks per
//! run: the fake `git` log (the output of each call, judged by a canary in each `.zygo/` file, and its arguments),
//! the `repo_def` tripwire (`ZYGO_TEST_TRAP_REPO_DEF`, feature `test-hooks`) and, on Linux,
//! an inotify watch of the working-tree `.zygo/`. No agent runs (R-C9), the built-in marketplace is off in the user
//! config (`Harness::config` adds `NO_MARKETPLACE`), no test asserts a time.
mod common;

use std::collections::BTreeSet;
use std::ops::RangeInclusive;
use std::path::{Path, PathBuf};

/// The workspace root.
fn ws_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// Every `.rs` file under `dir`, sorted.
fn rs_files(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = walkdir::WalkDir::new(dir)
        .into_iter()
        .map(|e| e.unwrap())
        .filter(|e| e.file_type().is_file() && e.path().extension().is_some_and(|x| x == "rs"))
        .map(|e| e.into_path())
        .collect();
    v.sort();
    v
}

/// The line indexes of the one item whose line, trimmed at the start, starts with `head`, through the first later
/// line that is its indentation and `}`.
fn body(lines: &[&str], head: &str) -> RangeInclusive<usize> {
    let starts: Vec<usize> =
        lines.iter().enumerate().filter(|(_, l)| l.trim_start().starts_with(head)).map(|(i, _)| i).collect();
    assert_eq!(starts.len(), 1, "`{head}` at {starts:?}");
    let s = starts[0];
    let close = format!("{}}}", &lines[s][..lines[s].len() - lines[s].trim_start().len()]);
    let e = (s + 1..lines.len()).find(|&j| lines[j] == close).unwrap_or_else(|| panic!("`{head}`: no closing brace"));
    s..=e
}

/// The `zygo-*` names in `[dependencies]` and `[target.*.dependencies]` of `crates/<name>/Cargo.toml`.
fn zygo_deps(name: &str) -> BTreeSet<String> {
    let t: toml::Table = read(&ws_root().join("crates").join(name).join("Cargo.toml")).parse().unwrap();
    let mut tables: Vec<&toml::Table> = t.get("dependencies").and_then(|v| v.as_table()).into_iter().collect();
    if let Some(targets) = t.get("target").and_then(|v| v.as_table()) {
        tables.extend(targets.values().filter_map(|v| v.get("dependencies").and_then(|d| d.as_table())));
    }
    tables.iter().flat_map(|d| d.keys()).filter(|k| k.starts_with("zygo-")).cloned().collect()
}

/// The crates of the hooks and the MCP server, and each crate they link except `zygo-config` (p4) and
/// `zygo-workspace` (p3).
const SCANNED: [&str; 10] = [
    "zygo-core", "zygo-hooks", "zygo-mail-sync", "zygo-mcp", "zygo-plugins", "zygo-proxy", "zygo-redact",
    "zygo-results", "zygo-usage", "zygo-work",
];
/// `zygo hook` (the HUD is `statusline_hud`), `zygo mcp` and the `zygo statusline` verb.
const CMD_FILES: [&str; 3] = ["crates/zygo/src/cmd/hook.rs", "crates/zygo/src/cmd/mcp.rs", "crates/zygo/src/cmd/statusline.rs"];
/// Names of the definition read (R§6.1, R§7.3, R§7.4), of its apply step (R§8) and of the code that calls them.
const FORBIDDEN: [&str; 6] = ["repo_def", "with_definition", "check_default", "check_pinned", "zygo_workspace::up", "zygo_launch"];
/// The public read entries of `repo_def` as whole words (`read_default_branch` is the `.zygo.toml` read, not one).
const READ_WORDS: &str = r"\b(check_default|check_pinned|read_default|read_at)\b";
/// The paths that the slash verb files name and that read no definition, removed from a line before the scan of p2:
/// the client of `zygo serve` (scanned itself, `verb_files`) and two items of `zygo_workspace::up` that are not
/// `up`/`up_all` (p3: only `up`/`up_all` reach the read).
const VERB_ALLOWED: [&str; 3] =
    ["zygo_launch::serve_client::", "zygo_workspace::up::{find_checkout, GIT_BOUND}", "zygo_workspace::up::GIT_BOUND"];

/// The files of the slash verbs that `zygo hook prompt` runs in the hook process (`cmd/mod.rs` `slash` → `run_verb`
/// → `VERBS`): `cmd/verbs.rs`, the module of each `VERBS` handler, and `zygo-launch/src/serve_client.rs`, the one
/// `zygo-launch` module they name.
fn verb_files() -> Vec<PathBuf> {
    let cmd = ws_root().join("crates/zygo/src/cmd");
    let handler = regex::Regex::new(r"handler: super::([a-z_]+)::verb\b").unwrap();
    let text = read(&cmd.join("verbs.rs"));
    let mods: Vec<String> = handler.captures_iter(&text).map(|c| c[1].to_string()).collect();
    assert_eq!(mods.len(), zygo_core::verbs::VERB_SPECS.len(), "{mods:?}");
    assert_eq!(mods.len(), 19, "{mods:?}");
    let mut v = vec![cmd.join("verbs.rs")];
    v.extend(mods.iter().map(|m| cmd.join(format!("{m}.rs"))));
    v.push(ws_root().join("crates/zygo-launch/src/serve_client.rs"));
    v
}

/// Every source file the hooks, the slash verbs and the MCP server run: the `SCANNED` crates, `CMD_FILES` and
/// `verb_files`, sorted, each once.
fn hook_files() -> Vec<PathBuf> {
    let mut files: BTreeSet<PathBuf> =
        SCANNED.iter().flat_map(|c| rs_files(&ws_root().join("crates").join(c).join("src"))).collect();
    files.extend(CMD_FILES.iter().map(|f| ws_root().join(f)));
    files.extend(verb_files());
    files.into_iter().collect()
}

#[test]
fn p1_hook_and_mcp_crates_link_only_the_scanned_crates() { // R-C8; drift 9
    let mut seen = BTreeSet::new();
    let mut todo = vec!["zygo-hooks".to_string(), "zygo-mcp".to_string()];
    while let Some(c) = todo.pop() {
        for d in zygo_deps(&c) {
            if seen.insert(d.clone()) {
                todo.push(d);
            }
        }
    }
    let expect: BTreeSet<String> = [
        "zygo-config", "zygo-core", "zygo-mail-sync", "zygo-plugins", "zygo-proxy", "zygo-redact", "zygo-results",
        "zygo-usage", "zygo-work", "zygo-workspace",
    ]
    .map(String::from)
    .into();
    assert_eq!(seen, expect);
}

#[test]
fn p2_no_hook_verb_or_mcp_source_names_the_definition_read() { // R-C8; R§10 row; R§18 item 20; review F3
    let brace_up = regex::Regex::new(r"zygo_workspace::\{[^}]*\bup\b").unwrap();
    let reads = regex::Regex::new(READ_WORDS).unwrap();
    let files = hook_files();
    assert!(files.len() >= 160, "{} files", files.len());
    for f in ["crates/zygo-hooks/src/statusline/data.rs", "crates/zygo/src/cmd/verbs.rs", "crates/zygo/src/cmd/status.rs"] {
        assert!(files.iter().any(|p| p.ends_with(f)), "{f}");
    }
    for f in &files {
        let text = read(f);
        for (i, line) in text.lines().enumerate() {
            let l = VERB_ALLOWED.iter().fold(line.to_string(), |l, a| l.replace(a, ""));
            for n in FORBIDDEN {
                assert!(!l.contains(n), "{}:{}: names {n}", f.display(), i + 1);
            }
            assert!(!reads.is_match(&l), "{}:{}: names a repo_def read: {line}", f.display(), i + 1);
        }
        assert!(!brace_up.is_match(&text), "{}: imports zygo_workspace::up", f.display());
    }
}

#[test]
fn p3_zygo_workspace_reads_the_definition_only_in_up_choose() { // R-C8; R§10 `zygo up` rows; drift 8
    let src = ws_root().join("crates/zygo-workspace/src");
    let rel = |f: &Path| f.strip_prefix(&src).unwrap().to_string_lossy().replace('\\', "/");
    // Each public read entry of `repo_def` (review F5: `read_default` and `read_at` too), by whole word.
    let reads = regex::Regex::new(READ_WORDS).unwrap();
    let readers: Vec<String> = rs_files(&src).iter().filter(|f| reads.is_match(&read(f))).map(|f| rel(f)).collect();
    assert_eq!(readers, ["up.rs"]);
    let up = read(&src.join("up.rs"));
    let lines: Vec<&str> = up.lines().collect();
    let choose = body(&lines, "fn choose(");
    for (i, l) in lines.iter().enumerate() {
        if reads.is_match(l) {
            assert!(l.starts_with("use zygo_config::repo_def::") || choose.contains(&i), "up.rs:{}: {l}", i + 1);
        }
    }
    assert!(lines[choose.clone()].iter().any(|l| l.contains("check_default(")));
    for head in ["fn choose(", "fn loadout_plan(", "fn build(", "fn run(", "fn run_all("] {
        assert_eq!(lines.iter().filter(|l| l.starts_with(head)).count(), 1, "up.rs: `{head}` at column 0 (private)");
    }
    let entry = regex::Regex::new(r"\bup::(up|up_all)\b|crate::up::\{[^}]*\b(up|up_all)\b").unwrap();
    for f in rs_files(&src) {
        if rel(&f) != "up.rs" {
            assert!(!entry.is_match(&read(&f)), "{}: calls up::up or up::up_all", rel(&f));
        }
    }
}

#[test]
fn p4_load_reads_no_definition_and_repo_def_is_only_in_with_definition() { // R§8 "Not a layer"; R-C8; review focus 38
    let text = read(&ws_root().join("crates/zygo-config/src/layers.rs"));
    let lines: Vec<&str> = text.lines().collect();
    for head in ["pub fn load(", "pub fn load_with_builtin(", "pub fn load_with_repo_reader("] {
        for i in body(&lines, head) {
            for n in ["repo_def", "Definition", "with_definition", "check_default", "check_pinned", ".zygo/"] {
                assert!(!lines[i].contains(n), "layers.rs:{}: `{head}` names {n}", i + 1);
            }
        }
    }
    assert!(lines[body(&lines, "pub fn load(")].iter().any(|l| l.contains("load_with_builtin(ctx, builtin_layer())")));
    assert!(lines[body(&lines, "pub fn load_with_builtin(")]
        .iter()
        .any(|l| l.contains("load_with_repo_reader(ctx, builtin, read_default_branch)")));
    let with = body(&lines, "pub fn with_definition(");
    let at: Vec<usize> = lines.iter().enumerate().filter(|(_, l)| l.contains("repo_def")).map(|(i, _)| i).collect();
    for &i in &at {
        assert!(
            lines[i] == "use crate::repo_def::{self, Definition};" || with.contains(&i),
            "layers.rs:{}: {}",
            i + 1,
            lines[i]
        );
    }
    assert!(at.iter().any(|i| with.contains(i)));
    // No other file of zygo-config outside `repo_def/` reaches the read, by name or by a renamed re-export
    // (review F4: `pub use repo_def::{read_default as …}` hid the read from p2).
    let src = ws_root().join("crates/zygo-config/src");
    let reads = regex::Regex::new(READ_WORDS).unwrap();
    let mut named = Vec::new();
    for f in rs_files(&src) {
        let r = f.strip_prefix(&src).unwrap().to_string_lossy().replace('\\', "/");
        if r.starts_with("repo_def/") {
            continue;
        }
        let t = read(&f);
        for (i, l) in t.lines().enumerate() {
            assert!(!reads.is_match(l), "{r}:{}: names a repo_def read: {l}", i + 1);
        }
        if t.contains("repo_def") {
            named.push(r);
        }
    }
    assert_eq!(named, ["layers.rs", "lib.rs"]);
    // The crate root declares the module and re-exports nothing of it: a hook reaches the read only by the path
    // `repo_def`, which p2 scans for.
    let lib = read(&src.join("lib.rs"));
    let at_root: Vec<&str> = lib.lines().filter(|l| l.contains("repo_def")).collect();
    assert_eq!(at_root, ["pub mod repo_def;"]);
}

#[test]
fn p5_no_hook_source_names_an_absolute_git() { // R§18 item 20; review F7
    // `bin/git` logs only a `git` found on `PATH`; an absolute path to git would run git unlogged.
    let abs = regex::Regex::new(r#""/[^"\s]*/git(\.exe)?"|Command::new\(\s*"/[^"]*git"#).unwrap();
    let mut files = hook_files();
    for c in ["zygo-config", "zygo-workspace"] {
        files.extend(rs_files(&ws_root().join("crates").join(c).join("src")));
    }
    assert!(files.len() >= 190, "{} files", files.len());
    for f in &files {
        for (i, line) in read(f).lines().enumerate() {
            assert!(!abs.is_match(line), "{}:{}: names an absolute git: {line}", f.display(), i + 1);
        }
    }
    // Control: the pattern sees both forms.
    assert!(abs.is_match(r#"let c = Command::new("/usr/bin/git");"#));
    assert!(abs.is_match(r#"const GIT: &str = "/opt/homebrew/bin/git";"#));
    assert!(!abs.is_match(r#"Command::new("git")"#) && !abs.is_match(r#"p.join("/.git")"#));
}

#[cfg(unix)]
mod unix {
    use std::collections::BTreeSet;
    use std::ffi::OsStr;
    use std::path::{Path, PathBuf};
    use std::process::{Output, Stdio};
    use std::time::{Duration, Instant};

    use serde_json::{json, Value};
    use zygo_config::repo_def::{approve_workspace_in, check_default, Definition, GitRunner, Use, TRAP_ENV};
    use zygo_core::session_dir::SessionDir;
    use zygo_core::session_view::{Member, SessionView, ViewWorkspace};
    use zygo_core::Agent;
    use zygo_work::repo::checkout;
    use zygo_workspace::identity::workspace_id;
    use zygo_workspace::state::{MemberRecord, MemberState, Pane, PaneKind};

    use crate::common::write_stdin;
    use crate::common::workspace::{ready_exe, wait_child, Guarded, Harness, WS_ID};

    const RUST: &str = "overrides = { code = \"claude-haiku-5-5\" }\ndisabled = [\"design\"]\n";
    /// The user config body: the `workspace_down` MCP tool on (review F6; `crates/zygo-mcp/src/tools/workspace_down.rs:39`).
    /// [`Harness::config`] adds `NO_MARKETPLACE` (the built-in marketplace off), so no run reaches the network (FX-23).
    const USER_CONFIG: &str = "[features]\nworkspace_tool = true\n";
    const KEY: &str = "kaaaaaaaa";
    const SID: &str = "01JTESTSESSIONAAAAAAAAAAAA";
    const OC_SID: &str = "ses_placeholder0001";
    /// The `load` read of `.zygo.toml` through `bin/git` (`repo_file.rs:252`, `:257`; `origin/HEAD` is `origin/main`).
    const LOAD_READ: &str = "show origin/main:.zygo.toml";
    /// The R§6.1 step 2 lookup of the definition read (`repo_def/read.rs:80`).
    const DEFAULT_LOOKUP: &str = "symbolic-ref --quiet refs/remotes/origin/HEAD";
    /// The prefix of a `bin/git` log line that holds one stdin line of a `cat-file --batch*` call.
    const STDIN: &str = "stdin: ";
    /// The prefix of a `bin/git` log line that holds the arguments of a call whose stdout or stderr names the
    /// definition ([`output_reads_definition`]; review round 2 F1, F2).
    const OUTPUT: &str = "output: ";
    /// The canary of the fixture with a definition (review round 2 F1, F2): in each `.zygo/` file and in the name of
    /// `.zygo/<CANARY>.toml`, so a read of the content or of the `.zygo` tree carries it out of git.
    const CANARY: &str = "canary-5d2e9b4f0a71c386";
    /// The first line of each `.zygo/` file of the fixture: a TOML comment that holds [`CANARY`] (and an `x`, so a
    /// `git grep -e x` prints it).
    const CANARY_LINE: &str = "# zygo-wd19-canary-5d2e9b4f0a71c386: text of the WD-19 test fixture\n";
    /// The stored result that the result tools read (review F6): JSON, so `result_tree` has an outline.
    const RESULT: &str = "{\"alpha\": 1,\n\"beta\": [1, 2]}\n";

    /// Holds `.zygo` once each `.zygo.toml` is taken out (`./.zygo/…`, `-C …/.zygo`, `HEAD:.zygo/…`).
    fn names_zygo(s: &str) -> bool {
        s.replace(".zygo.toml", "").contains(".zygo")
    }

    /// A `bin/git` line of a read of `.zygo/` (drift 5; review F2). `defs` are the object ids of the root tree and of
    /// each `.zygo` object of the fixture ([`def_oids`]). A call line: the default-branch lookup; an argument that
    /// [`names_zygo`]; an `ls-tree` with `-r` and no pathspec (the whole tree); `cat-file --batch-all-objects`. A
    /// `cat-file --batch*` call takes its objects on stdin, so each of its stdin lines (`stdin: <object>`; with
    /// `--batch-command`, the last word) is judged: one that [`names_zygo`], one that names the root tree (`^{tree}`,
    /// an empty path or a path with a `.` or `..` part), or an id in `defs`. The `load` read of `.zygo.toml` is not one.
    /// This argument check is the second check: the first is the output of each call (`output: <args>` lines of
    /// [`take_log`], [`output_reads_definition`]), which needs no list of argument forms (review round 2 F1, F2).
    fn reads_definition(line: &str, defs: &BTreeSet<String>) -> bool {
        if line.starts_with(OUTPUT) {
            return true;
        }
        if let Some(x) = line.strip_prefix(STDIN) {
            let obj = x.split_whitespace().last().unwrap_or_default();
            let root_path = obj
                .split_once(':')
                .is_some_and(|(_, p)| p.is_empty() || p.starts_with('/') || p.split('/').any(|c| c == "." || c == ".."));
            return names_zygo(x) || obj.contains("^{tree}") || root_path || defs.contains(obj);
        }
        let args: Vec<&str> = line.split(' ').collect();
        let all_objects = args.contains(&"cat-file") && args.contains(&"--batch-all-objects");
        let whole_tree = match args.iter().position(|a| *a == "ls-tree") {
            Some(i) => {
                let rest = &args[i + 1..];
                let opts_end = rest.iter().position(|a| *a == "--").unwrap_or(rest.len());
                let recursive = rest[..opts_end].iter().any(|a| *a == "-r" || (a.starts_with('-') && !a.starts_with("--") && a.contains('r')));
                let positional = rest[..opts_end].iter().filter(|a| !a.starts_with('-')).count() + rest.len().saturating_sub(opts_end + 1);
                recursive && positional <= 1
            }
            None => false,
        };
        line.contains(DEFAULT_LOOKUP) || all_objects || whole_tree || args.iter().any(|a| names_zygo(a))
    }

    /// Whether the stdout or stderr `b` of one git call names the definition of the fixture (review round 2 F1, F2):
    /// it holds [`CANARY`] (the content of a `.zygo/` file: `show`, `cat-file`, patches, `archive`, `grep`; or the
    /// name `.zygo/<CANARY>.toml` in a listing of the `.zygo` tree), the raw entry `.zygo\0` (a binary tree object of
    /// `cat-file --batch`, a `-z` listing), or a line (split at `\n` and `\0`) whose part after the last tab is
    /// `.zygo` or starts with `.zygo/` (a text listing of the root tree: `ls-tree`, `cat-file -p`, `show <tree>`).
    /// `.zygo.toml` is none of these.
    fn output_reads_definition(b: &[u8]) -> bool {
        let has = |n: &[u8]| b.windows(n.len()).any(|w| w == n);
        has(CANARY.as_bytes())
            || has(b".zygo\0")
            || b.split(|&c| c == b'\n' || c == b'\0').any(|l| {
                let name = l.rsplit(|&c| c == b'\t').next().unwrap_or(l);
                name == b".zygo" || name.starts_with(b".zygo/")
            })
    }

    /// The object ids of the root tree of `app`'s HEAD and of `.zygo` and each tree and blob under it: the ids by
    /// which a `cat-file --batch` stdin line can reach `.zygo/` with no path in it.
    fn def_oids(app: &Path) -> BTreeSet<String> {
        let mut s: BTreeSet<String> =
            git(app, &["ls-tree", "-r", "-t", "--format=%(objectname)", "HEAD", "--", ".zygo"]).lines().map(String::from).collect();
        s.insert(git(app, &["rev-parse", "HEAD^{tree}"]));
        s
    }

    /// A test-side git call: no user or system config, no templates, no signing, a fixed identity; asserts success;
    /// trimmed stdout.
    fn git(dir: &Path, args: &[&str]) -> String {
        let o = std::process::Command::new("git")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_TEMPLATE_DIR", "")
            .args(["-c", "user.name=t", "-c", "user.email=t@example.invalid", "-c", "commit.gpgsign=false", "-C"])
            .arg(dir)
            .args(args)
            .stdin(Stdio::null())
            .output()
            .expect("git on PATH");
        assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    }

    /// `<root>/app`: a clone of the bare `<root>/o.git` whose `main` holds `files`, so `refs/remotes/origin/HEAD` is
    /// `refs/remotes/origin/main` (R§18 preamble; as `workspace_definition.rs:56-73`).
    fn repo_at(root: &Path, files: &[(&str, &str)]) -> PathBuf {
        let w = root.join("w");
        std::fs::create_dir_all(&w).unwrap();
        for (path, text) in files {
            let p = w.join(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        git(root, &["init", "-q", "--bare", "-b", "main", "o.git"]);
        git(&w, &["init", "-q", "-b", "main"]);
        git(&w, &["add", "-A"]);
        git(&w, &["commit", "-q", "-m", "c"]);
        git(&w, &["remote", "add", "origin", "../o.git"]);
        git(&w, &["push", "-q", "origin", "main"]);
        git(root, &["clone", "-q", "o.git", "app"]);
        root.join("app")
    }

    fn ws_id(app: &Path) -> String {
        workspace_id(&checkout(app, Instant::now() + Duration::from_secs(3600)).unwrap().common_dir)
    }

    /// Approves the default branch's files of `app`, read through `git` (the `bin/git` of [`fake_git`], so the read has
    /// no user or system config); returns the approved definition.
    fn approve(app: &Path, approvals: &Path, git: &dyn GitRunner) -> Definition {
        let id = ws_id(app);
        if let Use::NotApproved { digest, .. } = check_default(app, &id, approvals, git) {
            approve_workspace_in(approvals, &id, &digest).unwrap();
        }
        match check_default(app, &id, approvals, git) {
            Use::Approved { def, .. } => def,
            other => panic!("not approved: {other:?}"),
        }
    }

    /// `bin/git`: appends each call (`$*`) to `<root>/git.log`, and the stdin of a `cat-file --batch*` call to
    /// `<root>/git.stdin` (review F2; `tee` and `mktemp` by their full paths, since a run's `PATH` is `bin`), then
    /// runs the real git with no user or system config and no templates (as `workspace_definition.rs:578`). Each call
    /// has its capture `<root>/git.out/<name>` (review round 2 F1, F2): the file holds the arguments, `.out` and
    /// `.err` the stdout and stderr of the real git (each passed on as it comes, so a `cat-file --batch` exchange
    /// still works), `.st` its exit code, which the script returns.
    fn fake_git(h: &Harness) {
        use std::os::unix::fs::PermissionsExt;
        let real = which::which("git").unwrap();
        let tee = which::which("tee").unwrap();
        let mktemp = which::which("mktemp").unwrap();
        let env = "GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 GIT_TEMPLATE_DIR=";
        let out = h.root().join("git.out");
        std::fs::create_dir_all(&out).unwrap();
        let body = format!(
            "#!/bin/sh\n\
             echo \"$*\" >> '{log}'\n\
             cap=$('{mktemp}' '{out}/XXXXXXXXXX') || exit 125\n\
             echo \"$*\" > \"$cap\"\n\
             run() {{ {{ {{ {env} '{real}' \"$@\" 3>&-; echo $? > \"$cap.st\"; }} 2>&1 1>&3 3>&- \
             | '{tee}' \"$cap.err\" 1>&2; }} 3>&1 | '{tee}' \"$cap.out\"; }}\n\
             case \" $* \" in\n  *\" cat-file \"*--batch*) '{tee}' -a '{stdin}' | run \"$@\" ;;\n  *) run \"$@\" ;;\nesac\n\
             read st < \"$cap.st\"\n\
             exit \"${{st:-1}}\"\n",
            log = h.root().join("git.log").display(),
            stdin = h.root().join("git.stdin").display(),
            out = out.display(),
            mktemp = mktemp.display(),
            real = real.display(),
            tee = tee.display(),
        );
        let p = h.bin.join("git");
        std::fs::write(&p, body).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        ready_exe(&p);
        let _ = std::fs::remove_file(h.root().join("git.log"));
        let _ = std::fs::remove_file(h.root().join("git.stdin"));
    }

    /// The working-tree trap (review F1, Linux): an inotify watch with `IN_OPEN | IN_ACCESS` on a folder and on each
    /// folder and file under it, read non-blocking. The kernel queues an event when a process opens or reads a
    /// watched path, so the events of a child that has exited are all in the queue: [`Watch::drain`] needs no wait.
    #[cfg(target_os = "linux")]
    pub(super) struct Watch {
        fd: libc::c_int,
        paths: Vec<(libc::c_int, PathBuf)>,
    }

    #[cfg(target_os = "linux")]
    impl Watch {
        pub(super) fn new(dir: &Path) -> Watch {
            use std::os::unix::ffi::OsStrExt;
            // SAFETY: plain integer flags; the result is checked.
            let fd = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
            assert!(fd >= 0, "inotify_init1: {}", std::io::Error::last_os_error());
            let mut w = Watch { fd, paths: Vec::new() };
            // The walk ends before the first watch: its own folder reads are not events.
            let all: Vec<PathBuf> = walkdir::WalkDir::new(dir).into_iter().map(|e| e.unwrap().into_path()).collect();
            for p in all {
                let c = std::ffi::CString::new(p.as_os_str().as_bytes()).unwrap();
                // SAFETY: `fd` is the open inotify descriptor; `c` is a NUL-terminated path that lives for the call.
                let wd = unsafe { libc::inotify_add_watch(fd, c.as_ptr(), libc::IN_OPEN | libc::IN_ACCESS) };
                assert!(wd >= 0, "inotify_add_watch {}: {}", p.display(), std::io::Error::last_os_error());
                w.paths.push((wd, p));
            }
            assert!(w.paths.len() >= 2, "{dir:?}: {:?}", w.paths);
            w
        }

        /// Each queued event as `<watched path>[/<name>] <mask>`; the queue is empty after the call.
        pub(super) fn drain(&self) -> Vec<String> {
            let mut out = Vec::new();
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                // SAFETY: `buf` is a writable buffer of `buf.len()` bytes; `fd` is the open inotify descriptor.
                let n = unsafe { libc::read(self.fd, buf.as_mut_ptr().cast(), buf.len()) };
                if n < 0 {
                    let e = std::io::Error::last_os_error();
                    assert_eq!(e.kind(), std::io::ErrorKind::WouldBlock, "inotify read: {e}");
                    return out;
                }
                let n = n as usize;
                assert!(n > 0, "inotify read: end of file");
                let mut i = 0;
                // `struct inotify_event`: wd i32, mask u32, cookie u32, len u32, then `len` bytes of NUL-padded name.
                while i + 16 <= n {
                    let word = |k: usize| <[u8; 4]>::try_from(&buf[i + k..i + k + 4]).unwrap();
                    let wd = i32::from_ne_bytes(word(0));
                    let mask = u32::from_ne_bytes(word(4));
                    let len = u32::from_ne_bytes(word(12)) as usize;
                    let name = String::from_utf8_lossy(&buf[i + 16..i + 16 + len]).trim_end_matches('\0').to_string();
                    let path = self.paths.iter().find(|(w, _)| *w == wd).map_or("?".into(), |(_, p)| p.display().to_string());
                    out.push(if name.is_empty() { format!("{path} {mask:#x}") } else { format!("{path}/{name} {mask:#x}") });
                    i += 16 + len;
                }
            }
        }
    }

    #[cfg(target_os = "linux")]
    impl Drop for Watch {
        fn drop(&mut self) {
            // SAFETY: `fd` is the open inotify descriptor this value owns; it is closed once.
            unsafe { libc::close(self.fd) };
        }
    }

    struct Fx {
        h: Harness,
        app: PathBuf,
        head: String,
        /// The file of the `repo_def` tripwire (`TRAP_ENV`) of each run.
        trap: PathBuf,
        /// [`def_oids`] of `app`.
        defs: BTreeSet<String>,
        /// The watch of the working-tree `.zygo/` of a fixture with a definition.
        #[cfg(target_os = "linux")]
        watch: Option<Watch>,
    }

    /// A harness with `USER_CONFIG` as the user config, `bin/git` from [`fake_git`], and `app` with `.zygo.toml` =
    /// `version = 1` and `README`. With `definition`, also `.zygo/workspace.toml` = `version = 1` and
    /// `.zygo/profiles/rust.toml` = `RUST`, each after [`CANARY_LINE`], and `.zygo/<CANARY>.toml` = `CANARY_LINE`,
    /// approved for `app` (a definition that changes `up` and a `rust` launch), and on Linux the watch of `app/.zygo`,
    /// set after the approval.
    fn fx(definition: bool) -> Fx {
        let h = Harness::no_tmux();
        h.config(USER_CONFIG);
        let canary_file = format!(".zygo/{CANARY}.toml");
        let (workspace, rust) = (format!("{CANARY_LINE}version = 1\n"), format!("{CANARY_LINE}{RUST}"));
        let mut files = vec![(".zygo.toml", "version = 1\n"), ("README", "app\n")];
        if definition {
            files.push((".zygo/workspace.toml", &workspace));
            files.push((".zygo/profiles/rust.toml", &rust));
            files.push((&canary_file, CANARY_LINE));
        }
        let app = repo_at(h.root(), &files);
        let head = git(&app, &["rev-parse", "HEAD"]);
        fake_git(&h);
        if definition {
            let def = approve(&app, &h.paths.approvals_file, &LogGit(h.bin.join("git")));
            assert!(def.workspace.is_some() && def.profiles.contains_key("rust"), "{def:?}");
            let _ = std::fs::remove_file(h.root().join("git.log"));
            let _ = std::fs::remove_file(h.root().join("git.stdin"));
            std::fs::remove_dir_all(h.root().join("git.out")).unwrap();
            std::fs::create_dir(h.root().join("git.out")).unwrap();
        }
        let trap = h.root().join("trap.log");
        let defs = def_oids(&app);
        #[cfg(target_os = "linux")]
        let watch = definition.then(|| Watch::new(&app.join(".zygo")));
        Fx {
            h,
            app,
            head,
            trap,
            defs,
            #[cfg(target_os = "linux")]
            watch,
        }
    }

    /// The `bin/git` lines since the last call, each `cat-file --batch*` stdin line as `stdin: <line>`, and the
    /// arguments of each call whose stdout or stderr [`output_reads_definition`] as `output: <args>`, with the harness
    /// root as `<root>` and HEAD of `app` as `<head>`, sorted; the logs and the captures of the calls that ended are
    /// removed (a capture with no `.st` yet is judged again by the next call).
    fn take_log(fx: &Fx) -> Vec<String> {
        let take = |name: &str| {
            let p = fx.h.root().join(name);
            let text = std::fs::read_to_string(&p).unwrap_or_default();
            let _ = std::fs::remove_file(&p);
            text
        };
        let root = fx.h.root().display().to_string();
        let calls = take("git.log");
        let stdin = take("git.stdin");
        let mut outputs = Vec::new();
        for e in std::fs::read_dir(fx.h.root().join("git.out")).unwrap() {
            let cap = e.unwrap().path();
            if cap.extension().is_some() {
                continue;
            }
            let with = |ext: &str| cap.with_extension(ext);
            let named = ["out", "err"].iter().any(|x| output_reads_definition(&std::fs::read(with(x)).unwrap_or_default()));
            if named {
                outputs.push(format!("{OUTPUT}{}", std::fs::read_to_string(&cap).unwrap_or_default().trim_end()));
            }
            if with("st").is_file() {
                for p in [cap.clone(), with("out"), with("err"), with("st")] {
                    let _ = std::fs::remove_file(p);
                }
            }
        }
        let mut v: Vec<String> = calls
            .lines()
            .map(str::to_string)
            .chain(stdin.lines().map(|l| format!("{STDIN}{l}")))
            .chain(outputs)
            .map(|l| l.replace(&root, "<root>").replace(&fx.head, "<head>"))
            .collect();
        v.sort();
        v
    }

    /// The tripwire lines since the last call; the file is removed.
    fn take_trap(fx: &Fx) -> Vec<String> {
        let text = std::fs::read_to_string(&fx.trap).unwrap_or_default();
        let _ = std::fs::remove_file(&fx.trap);
        text.lines().map(str::to_string).collect()
    }

    /// The working-tree events since the last call (Linux, a fixture with a definition); else none.
    fn take_events(fx: &Fx) -> Vec<String> {
        #[cfg(target_os = "linux")]
        if let Some(w) = &fx.watch {
            return w.drain();
        }
        let _ = fx;
        Vec::new()
    }

    /// `<root>/<name>`: `view.json` of session `SID` for `agent`, main and effective role `lead`, profile `rust`,
    /// work units, results, compact tool and mail on, `repo_root` = `app`, style `verbose`. With `member`: the marker
    /// `{WS_ID, KEY, worker-1}` (AU58) and the state of `WS_ID` with `KEY` running in `app`.
    fn session(fx: &Fx, name: &str, agent: Agent, member: bool) -> SessionDir {
        let sd = SessionDir::new(fx.h.root().join(name));
        sd.create().unwrap();
        let workspace = member.then(|| ViewWorkspace {
            member: Member { ws_id: WS_ID.into(), pane: KEY.into(), name: Some("worker-1".into()), ..Default::default() },
            resume_note: None,
            ..Default::default()
        });
        SessionView {
            session: SID.into(),
            agent,
            main_role: "lead".into(),
            effective_role: Some("lead".into()),
            profile: Some("rust".into()),
            work_units: true,
            results: true,
            compact_tool: true,
            mail: true,
            repo_root: Some(fx.app.display().to_string()),
            statusline_style: "verbose".into(),
            trim_threshold_bytes: 16384,
            head_lines: 60,
            tail_lines: 20,
            workspace,
            ..Default::default()
        }
        .save(&sd)
        .unwrap();
        if member {
            let app = fx.app.clone();
            let kind = if agent == Agent::OpenCode { PaneKind::OpenCode } else { PaneKind::ClaudeCode };
            fx.h.seed(WS_ID, &mut |w| {
                w.common_dir = app.join(".git");
                w.main_root = app.clone();
                w.panes = vec![Pane { key: KEY.into(), cwd: app.clone(), kind, ..Default::default() }];
                w.members.clear();
                w.members.insert(
                    KEY.into(),
                    MemberRecord { zygo_session: Some(SID.into()), state: MemberState::Running, ..MemberRecord::new(agent) },
                );
            });
        }
        sd
    }

    /// The end of one `zygo` run: exit code, stdout, stderr, `bin/git` lines, tripwire lines, working-tree events.
    struct Out {
        code: Option<i32>,
        stdout: String,
        stderr: String,
        git: Vec<String>,
        trap: Vec<String>,
        events: Vec<String>,
    }

    /// `zygo <args>` in `app` with `ZYGO_SESSION_DIR` = `sd`, `COLUMNS` = 200, `NO_COLOR` = 1 and the tripwire file,
    /// `stdin` fed and closed.
    fn run_raw(fx: &Fx, sd: &SessionDir, args: &[&str], stdin: &[u8]) -> Out {
        let extra: [(&str, &OsStr); 4] = [
            ("ZYGO_SESSION_DIR", sd.root.as_os_str()),
            ("COLUMNS", OsStr::new("200")),
            ("NO_COLOR", OsStr::new("1")),
            (TRAP_ENV, fx.trap.as_os_str()),
        ];
        let mut c = fx.h.zygo(args, &extra);
        c.current_dir(&fx.app).stdin(Stdio::piped());
        let mut g = Guarded::spawn(c);
        write_stdin(g.stdin().unwrap(), stdin);
        let o: Output = g.output(&args.join(" "));
        Out {
            code: o.status.code(),
            stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
            git: take_log(fx),
            trap: take_trap(fx),
            events: take_events(fx),
        }
    }

    /// [`run_raw`] that asserts exit 0 (C6; `zygo mcp` at the end of its input), no tripwire line and no working-tree
    /// event; a message names the run as `label`, else as its arguments. Returns stdout and the run's `bin/git` lines.
    fn run_as(fx: &Fx, sd: &SessionDir, label: Option<&str>, args: &[&str], stdin: &[u8]) -> (String, Vec<String>) {
        let o = run_raw(fx, sd, args, stdin);
        let what = label.map_or_else(|| args.join(" "), str::to_string);
        assert_eq!(o.code, Some(0), "zygo {what}: {}", o.stderr);
        assert!(o.trap.is_empty(), "zygo {what}: the definition read ran: {:?}", o.trap);
        assert!(o.events.is_empty(), "zygo {what}: the working-tree .zygo/ was opened or read: {:?}", o.events);
        (o.stdout, o.git)
    }

    /// [`run_as`] with the arguments as the name of the run.
    fn run(fx: &Fx, sd: &SessionDir, args: &[&str], stdin: &[u8]) -> (String, Vec<String>) {
        run_as(fx, sd, None, args, stdin)
    }

    /// The recorded Claude Code input `file` with `"/repo` (its `cwd` and tool paths) moved to `app`.
    fn cc(fx: &Fx, file: &str) -> Vec<u8> {
        let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/claude-code/hooks").join(file);
        std::fs::read_to_string(p).unwrap().replace("\"/repo", &format!("\"{}", fx.app.display())).into_bytes()
    }

    /// The recorded OpenCode plugin payload `file` (`{input, output}`, as the plugin sends it).
    fn oc(file: &str) -> Vec<u8> {
        std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/opencode/plugin-hooks").join(file)).unwrap()
    }

    /// `PreToolUse-Bash-sub.json` as a call of MCP tool `tool` with `input` (as `work_hook.rs:52-54`).
    fn work_tool(fx: &Fx, tool: &str, input: Value) -> Vec<u8> {
        let mut v: Value = serde_json::from_slice(&cc(fx, "PreToolUse-Bash-sub.json")).unwrap();
        v["tool_name"] = json!(tool);
        v["tool_input"] = input;
        v.to_string().into_bytes()
    }

    /// One row: (label, `zygo` arguments, stdin).
    type Row = (String, String, Vec<u8>);
    /// One run: (label, stdout, git lines).
    type Ran = (String, String, Vec<String>);

    /// The Claude Code registrations of every launch (`claude_code.rs:462-541`), with each tool group of `trim` and
    /// `role-guard` (review F6).
    fn cc_main(fx: &Fx) -> Vec<Row> {
        let app = fx.app.display().to_string();
        let hud = json!({"session_id": "00000000-0000-4000-8000-000000000001", "cwd": app,
            "workspace": {"current_dir": app, "project_dir": app}})
        .to_string()
        .into_bytes();
        let begin = json!({"unit": "beads:bd-a1b2", "round": 1, "check": "cargo test -p demo", "files": ["src/"]});
        let verdict = json!({"unit": "beads:bd-a1b2", "round": 1, "verdict": "discard"});
        let mut v = vec![
            ("hook token".to_string(), Vec::new()),
            ("hook statusline".to_string(), hud),
            ("hook work --event begin".to_string(), work_tool(fx, "mcp__zygo__work_begin", begin)),
            ("hook work --event verdict".to_string(), work_tool(fx, "mcp__zygo__work_verdict", verdict)),
        ];
        for (args, file) in [
            ("hook role-guard", "PreToolUse-Edit-main.json"),
            ("hook role-guard", "PreToolUse-Write-main.json"),
            ("hook role-guard", "PreToolUse-NotebookEdit-main.json"),
            ("hook guard", "PreToolUse-Edit-main.json"),
            ("hook guard", "PreToolUse-Bash-main.json"),
            ("hook guard", "PreToolUse-Read-main.json"),
            ("hook trim", "PostToolUse-Bash-main.json"),
            ("hook trim", "PostToolUse-Read-main.json"),
            ("hook trim", "PostToolUse-Grep-main.json"),
            ("hook trim", "PostToolUse-Glob-main.json"),
            ("hook trim", "PostToolUse-WebFetch-main.json"),
            ("hook work", "PostToolUse-Bash-sub.json"),
            ("hook work", "PostToolUseFailure-Read-main.json"),
            ("hook work", "SubagentStop-zygo-code-sub.json"),
            ("hook session-start", "SessionStart-main.json"),
            ("hook session-start", "SubagentStart-sub.json"),
            ("hook prompt", "UserPromptSubmit-main.json"),
            ("hook pre-compact", "PreCompact-main.json"),
        ] {
            v.push((format!("{args} < {file}"), cc(fx, file)));
        }
        v.into_iter().map(|(label, stdin)| (label.clone(), label.split(" < ").next().unwrap().to_string(), stdin)).collect()
    }

    /// `zygo hook prompt` with the prompt `/zygo:<verb>` (`crates/zygo-hooks/src/prompt.rs:22`) for each verb of
    /// `VERB_SPECS`, in order: each slash verb runs in the hook process (`cmd/mod.rs` `slash`; review F3).
    fn verb_rows(fx: &Fx) -> Vec<Row> {
        let base: Value = serde_json::from_slice(&cc(fx, "UserPromptSubmit-main.json")).unwrap();
        zygo_core::verbs::VERB_SPECS
            .iter()
            .map(|s| {
                let mut v = base.clone();
                v["prompt"] = json!(format!("/zygo:{}", s.name));
                (format!("hook prompt < /zygo:{}", s.name), "hook prompt".to_string(), v.to_string().into_bytes())
            })
            .collect()
    }

    /// `zygo hook prompt` with `/zygo:workspace approve` and `/zygo:workspace approve --yes` (WD-15 review F4): the
    /// subverb that reads the definition in its CLI form replies with the shell command in the hook process.
    fn approve_rows(fx: &Fx) -> Vec<Row> {
        let base: Value = serde_json::from_slice(&cc(fx, "UserPromptSubmit-main.json")).unwrap();
        ["/zygo:workspace approve", "/zygo:workspace approve --yes"]
            .into_iter()
            .map(|p| {
                let mut v = base.clone();
                v["prompt"] = json!(p);
                (format!("hook prompt < {p}"), "hook prompt".to_string(), v.to_string().into_bytes())
            })
            .collect()
    }

    /// `zygo hook prompt` with each `/zygo:profile` form (WD-16 plan call 2; review F2): the CLI form of `list` and
    /// `show` shows the approved workspace files; no slash form reads any, whether it lists, shows or fails.
    const PROFILE_FORMS: [&str; 6] = [
        "/zygo:profile",
        "/zygo:profile list",
        "/zygo:profile show",
        "/zygo:profile show rust",
        "/zygo:profile show x y",
        "/zygo:profile --bad",
    ];

    fn profile_rows(fx: &Fx) -> Vec<Row> {
        let base: Value = serde_json::from_slice(&cc(fx, "UserPromptSubmit-main.json")).unwrap();
        PROFILE_FORMS
            .into_iter()
            .map(|p| {
                let mut v = base.clone();
                v["prompt"] = json!(p);
                (format!("hook prompt < {p}"), "hook prompt".to_string(), v.to_string().into_bytes())
            })
            .collect()
    }

    /// The registrations that a member adds (`claude_code.rs:489-525`) and the other `workspace` events
    /// (`crates/zygo-hooks/src/workspace.rs:41-42`).
    fn cc_member(fx: &Fx) -> Vec<Row> {
        [
            ("hook session-start --event member", "SessionStart-main.json"),
            ("hook workspace --event session", "SessionStart-main.json"),
            ("hook workspace --event prompt", "UserPromptSubmit-main.json"),
            ("hook workspace --event pre-tool", "PreToolUse-Bash-main.json"),
            ("hook workspace --event tool-end", "PostToolUse-Bash-main.json"),
            ("hook workspace --event subagent-start", "SubagentStart-sub.json"),
            ("hook workspace --event subagent-stop", "SubagentStop-sub.json"),
            ("hook workspace --event stop", "Stop-main.json"),
            ("hook workspace --event session-end", "Stop-main.json"),
        ]
        .into_iter()
        .map(|(args, file)| (format!("{args} < {file}"), args.to_string(), cc(fx, file)))
        .collect()
    }

    /// The `zygo hook` calls of the OpenCode plugin of a member (`opencode-plugin.js:23`, `:45-194`), with the
    /// `session {sessionID}` call (`:86`) and the `task` branch's `pre-tool {tool_name: "Agent"}` (`:159`; review F6).
    fn oc_member() -> Vec<Row> {
        let s = |v: Value| v.to_string().into_bytes();
        vec![
            ("", "hook guard --agent opencode", oc("tool.execute.before-bash.json")),
            ("", "hook trim --agent opencode", oc("tool.execute.after-bash.json")),
            ("", "hook work --agent opencode", oc("tool.execute.after-bash.json")),
            ("", "hook session-start --agent opencode", oc("experimental.chat.system.transform.json")),
            ("", "hook session-start --agent opencode --event subagent", oc("experimental.chat.system.transform.json")),
            ("", "hook pre-compact --agent opencode", oc("experimental.session.compacting.json")),
            ("", "hook workspace --agent opencode --event session", s(json!({}))),
            (" < {sessionID}", "hook workspace --agent opencode --event session", s(json!({"sessionID": OC_SID}))),
            ("", "hook workspace --agent opencode --event prompt", s(json!({"sessionID": OC_SID}))),
            ("", "hook workspace --agent opencode --event pre-tool", s(json!({"sessionID": OC_SID, "tool_name": "bash"}))),
            (" < Agent", "hook workspace --agent opencode --event pre-tool", s(json!({"sessionID": OC_SID, "tool_name": "Agent"}))),
            ("", "hook workspace --agent opencode --event tool-end", s(json!({"sessionID": OC_SID}))),
            ("", "hook workspace --agent opencode --event stop", s(json!({"sessionID": OC_SID}))),
        ]
        .into_iter()
        .map(|(tag, args, stdin)| (format!("{args}{tag}"), args.to_string(), stdin))
        .collect()
    }

    /// Each row of `rows` in session `sd`, in order.
    fn run_rows(fx: &Fx, sd: &SessionDir, rows: Vec<Row>) -> Vec<Ran> {
        rows.into_iter()
            .map(|(label, args, stdin)| {
                let a: Vec<&str> = args.split(' ').collect();
                let (out, log) = run_as(fx, sd, Some(&label), &a, &stdin);
                (label, out, log)
            })
            .collect()
    }

    /// `calls` on a fixture with an approved `.zygo/` and on one without: the same labels; in both, no line reads
    /// `.zygo/`; per label the same git lines in both, except for the status line (drift 7). Each run of both already
    /// had no tripwire line and no working-tree event ([`run`]); after all runs of a fixture, a detached child left
    /// none either. Returns the first.
    fn compare(calls: &dyn Fn(&Fx) -> Vec<Ran>) -> Vec<Ran> {
        let run_all = |definition: bool| {
            let fx = fx(definition);
            let ran = calls(&fx);
            assert_eq!(take_trap(&fx), Vec::<String>::new(), "after the runs, definition {definition}");
            assert_eq!(take_events(&fx), Vec::<String>::new(), "after the runs, definition {definition}");
            for (label, _, log) in &ran {
                for l in log {
                    assert!(!reads_definition(l, &fx.defs), "{label}: {l}");
                }
            }
            ran
        };
        let with = run_all(true);
        let without = run_all(false);
        let labels = |r: &[Ran]| r.iter().map(|c| c.0.clone()).collect::<Vec<_>>();
        assert_eq!(labels(&with), labels(&without));
        for ((label, _, w), (_, _, wo)) in with.iter().zip(&without) {
            if !label.starts_with("hook statusline") {
                assert_eq!(w, wo, "{label}: other git calls with .zygo/");
            }
        }
        with
    }

    /// The run `label`.
    fn ran_of<'a>(ran: &'a [Ran], label: &str) -> &'a Ran {
        ran.iter().find(|c| c.0 == label).unwrap_or_else(|| panic!("no run {label}"))
    }

    /// The git lines of the run `label`.
    fn log_of<'a>(ran: &'a [Ran], label: &str) -> &'a [String] {
        &ran_of(ran, label).2
    }

    /// Runs `bin/git` of a harness as `git -C <top> <args>`, so [`fake_git`] logs the call.
    struct LogGit(PathBuf);

    impl GitRunner for LogGit {
        fn git(&self, top: &Path, args: &[&str], _deadline: Instant) -> Result<Output, String> {
            std::process::Command::new(&self.0)
                .arg("-C")
                .arg(top)
                .args(args)
                .env("LC_ALL", "C")
                .stdin(Stdio::null())
                .output()
                .map_err(|e| e.to_string())
        }
    }

    /// The reads of `.zygo/` that review round 2 F1 (whole-tree and object reads on the command line) and F2
    /// (`cat-file --batch` stdin: abbreviated and uppercase ids, `%(rest)` formats) found the argument predicate
    /// missed: (label, arguments after `-C <app>`, stdin of the call).
    fn round2_forms(fx: &Fx) -> Vec<(String, Vec<String>, Option<String>)> {
        let head = fx.head.clone();
        let root_tree = git(&fx.app, &["rev-parse", "HEAD^{tree}"]);
        let rust_blob = git(&fx.app, &["rev-parse", "HEAD:.zygo/profiles/rust.toml"]);
        let a = |s: &str| s.split(' ').map(String::from).collect::<Vec<_>>();
        let batch = |label: &str, args: &str, line: String| (label.to_string(), a(args), Some(format!("{line}\n")));
        vec![
            ("F1 show <head>:".into(), vec!["show".into(), format!("{head}:")], None),
            ("F1 cat-file -p <head>^{tree}".into(), vec!["cat-file".into(), "-p".into(), format!("{head}^{{tree}}")], None),
            ("F1 cat-file -p <root tree id>".into(), vec!["cat-file".into(), "-p".into(), root_tree.clone()], None),
            ("F1 cat-file -p <.zygo blob id>".into(), vec!["cat-file".into(), "-p".into(), rust_blob.clone()], None),
            ("F1 archive HEAD".into(), a("archive HEAD"), None),
            ("F1 grep -e x HEAD".into(), a("grep -e x HEAD"), None),
            ("F1 show HEAD".into(), a("show HEAD"), None),
            ("F1 log -p".into(), a("log -p"), None),
            ("F1 diff-tree -r -p --root HEAD".into(), a("diff-tree -r -p --root HEAD"), None),
            ("F1 ls-tree -r HEAD -- .".into(), a("ls-tree -r HEAD -- ."), None),
            batch("F2 --batch abbreviated root tree", "cat-file --batch", root_tree[..10].to_string()),
            batch("F2 --batch abbreviated .zygo blob", "cat-file --batch", rust_blob[..10].to_string()),
            batch("F2 --batch uppercase root tree", "cat-file --batch", root_tree.to_uppercase()),
            batch("F2 --batch uppercase .zygo blob", "cat-file --batch", rust_blob.to_uppercase()),
            batch("F2 --batch=%(rest) root tree", "cat-file --batch=%(objectname)%(rest)", format!("{root_tree} r")),
            batch("F2 --batch=%(rest) .zygo blob", "cat-file --batch=%(objectname)%(rest)", format!("{rust_blob} r")),
        ]
    }

    /// Runs `bin/git -C <app> <args>` with `stdin` (else none) to its end; returns whether it succeeded.
    fn fake_call(fx: &Fx, args: &[String], stdin: Option<&str>) -> bool {
        let mut c = std::process::Command::new(fx.h.bin.join("git"))
            .arg("-C")
            .arg(&fx.app)
            .args(args)
            .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        if let Some(s) = stdin {
            write_stdin(c.stdin.take().unwrap(), s.as_bytes());
        }
        wait_child(&mut c, &format!("bin/git {args:?}")).success()
    }

    #[test]
    fn h0_the_log_predicate_and_the_tripwire_see_a_definition_read() { // R§18 item 20: the control of h1-h4
        let fx = fx(true);
        match check_default(&fx.app, &ws_id(&fx.app), &fx.h.paths.approvals_file, &LogGit(fx.h.bin.join("git"))) {
            Use::Approved { def, .. } => assert!(def.profiles.contains_key("rust")),
            other => panic!("{other:?}"),
        }
        let lines = take_log(&fx);
        assert!(lines.iter().any(|l| l.contains(DEFAULT_LOOKUP)), "{lines:?}");
        assert!(lines.iter().any(|l| l.contains("ls-tree -z --long --full-tree") && l.ends_with(" -- .zygo")), "{lines:?}");
        let defs = &fx.defs;
        assert!(lines.iter().filter(|l| reads_definition(l, defs)).count() >= 2, "{lines:?}");
        // Review F2: the forms that the first predicate missed, and the forms that are not a read.
        let root_tree = git(&fx.app, &["rev-parse", "HEAD^{tree}"]);
        let rust_blob = git(&fx.app, &["rev-parse", "HEAD:.zygo/profiles/rust.toml"]);
        assert!(defs.contains(&root_tree) && defs.contains(&rust_blob) && defs.len() == 6, "{defs:?}");
        for l in [
            "-C <root>/app show origin/main:./.zygo/profiles/rust.toml",
            "-C <root>/app/.zygo log -1",
            "-C <root>/app ls-tree -r HEAD",
            "-C <root>/app ls-tree -rz --name-only HEAD",
            "-C <root>/app cat-file --batch --batch-all-objects",
            "stdin: <head>:.zygo/profiles/rust.toml",
            "stdin: <head>^{tree}",
            "stdin: <head>:",
            "stdin: <head>:docs/..",
            "stdin: contents <head>:./.zygo",
            &format!("stdin: {root_tree}"),
            &format!("stdin: {rust_blob}"),
        ] {
            assert!(reads_definition(l, defs), "{l}");
        }
        for l in [
            "-C <root>/app show origin/main:.zygo.toml",
            "-C <root>/app ls-tree --name-only origin/main -- .zygo.toml",
            "-c core.fsmonitor=false -C <root>/app symbolic-ref --short -q HEAD",
            "-C <root>/app ls-tree -r HEAD -- src",
            "-C <root>/app ls-tree HEAD",
            "-c core.fsmonitor=false -C <root>/app cat-file --batch",
            "stdin: <head>^{commit}",
            "stdin: <head>:docs/decisions",
        ] {
            assert!(!reads_definition(l, defs), "{l}");
        }
        // `bin/git` logs the stdin of a `cat-file --batch` call: an object id with no path in it is seen.
        let mut c = std::process::Command::new(fx.h.bin.join("git"))
            .arg("-C")
            .arg(&fx.app)
            .args(["cat-file", "--batch"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        write_stdin(c.stdin.take().unwrap(), format!("{rust_blob}\n").as_bytes());
        assert!(wait_child(&mut c, "bin/git cat-file --batch").success());
        let lines = take_log(&fx);
        assert!(lines.contains(&format!("{STDIN}{rust_blob}")), "{lines:?}");
        // The stdin line by its id, and the call by its output (the blob holds the canary).
        assert!(lines.contains(&format!("{OUTPUT}-C <root>/app cat-file --batch")), "{lines:?}");
        assert_eq!(lines.iter().filter(|l| reads_definition(l, defs)).count(), 2, "{lines:?}");
        // Review round 2 F1, F2: each form that the argument predicate missed is caught by its output.
        let forms = round2_forms(&fx);
        assert_eq!(forms.len(), 16);
        for (label, args, stdin) in forms {
            assert!(fake_call(&fx, &args, stdin.as_deref()), "{label}");
            let lines = take_log(&fx);
            assert!(lines.iter().any(|l| l.starts_with(OUTPUT)), "{label}: {lines:?}");
        }
        // The output check passes the `load` read of `.zygo.toml` and reads of other paths.
        for args in ["show origin/main:.zygo.toml", "ls-tree --name-only origin/main -- .zygo.toml", "ls-tree HEAD -- README",
            "cat-file -p HEAD", "log -1"]
        {
            let a: Vec<String> = args.split(' ').map(String::from).collect();
            assert!(fake_call(&fx, &a, None), "{args}");
            let lines = take_log(&fx);
            assert!(!lines.iter().any(|l| l.starts_with(OUTPUT)), "{args}: {lines:?}");
        }
        // The tripwire: `zygo doctor` reads the definition (`cmd/doctor.rs:472`) in a child, through `bin/git`.
        let sd = session(&fx, "d", Agent::ClaudeCode, false);
        let o = run_raw(&fx, &sd, &["doctor"], b"");
        for entry in ["repo_def:check_default", "repo_def:read_default", "repo_def:SystemGit::git"] {
            assert!(o.trap.iter().any(|l| l == entry), "{entry}: {:?}\n{}", o.trap, o.stderr);
        }
        assert!(o.git.iter().any(|l| reads_definition(l, &fx.defs)), "{:?}", o.git);
        // The capture works in a `zygo` run too: the read's output is seen, not only its arguments.
        assert!(o.git.iter().any(|l| l.starts_with(OUTPUT)), "{:?}", o.git);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn h5_the_working_tree_trap_sees_an_open_of_zygo() { // review F1: the control of the watch in h1-h4
        let fx = fx(true);
        assert_eq!(take_events(&fx), Vec::<String>::new());
        assert_eq!(std::fs::read_to_string(fx.app.join(".zygo/profiles/rust.toml")).unwrap(), format!("{CANARY_LINE}{RUST}"));
        let ev = take_events(&fx);
        assert!(ev.iter().any(|e| e.contains(".zygo/profiles/rust.toml")), "{ev:?}");
        assert_eq!(take_events(&fx), Vec::<String>::new());
        // A listing of the folder is an open of it too.
        let _ = std::fs::read_dir(fx.app.join(".zygo")).unwrap().count();
        assert!(!take_events(&fx).is_empty());
    }

    #[test]
    fn h1_claude_code_hooks_and_slash_verbs_of_a_main_session_read_no_definition() { // R-C8; R§10 row; R§18 item 20; review focus 38, F3
        let ran = compare(&|fx| {
            let sd = session(fx, "s", Agent::ClaudeCode, false);
            let mut rows = cc_main(fx);
            rows.extend(verb_rows(fx));
            rows.extend(approve_rows(fx));
            rows.extend(profile_rows(fx));
            run_rows(fx, &sd, rows)
        });
        assert_eq!(ran.len(), 22 + 19 + 2 + PROFILE_FORMS.len());
        // WD-15 review F4: `/zygo:workspace approve` ran no definition read (`run_as`: no tripwire line) and replied
        // with the shell command.
        for p in ["/zygo:workspace approve", "/zygo:workspace approve --yes"] {
            let out = &ran_of(&ran, &format!("hook prompt < {p}")).1;
            let v: Value = serde_json::from_str(out).unwrap_or_else(|e| panic!("{p}: {e}: {out}"));
            assert_eq!(v["decision"], "block", "{p}: {out}");
            assert_eq!(v["reason"], "zygo: run in your shell: zygo workspace approve", "{p}: {out}");
        }
        // WD-16 plan call 2, review F2: each `/zygo:profile` form ran no definition read (`run_as`: no tripwire line),
        // replied, and showed no repository profile.
        for p in PROFILE_FORMS {
            let out = &ran_of(&ran, &format!("hook prompt < {p}")).1;
            let v: Value = serde_json::from_str(out).unwrap_or_else(|e| panic!("{p}: {e}: {out}"));
            assert_eq!(v["decision"], "block", "{p}: {out}");
            assert!(!out.contains("workspace-files"), "{p}: {out}");
        }
        let out = &ran_of(&ran, "hook prompt < /zygo:profile show rust").1;
        assert!(out.contains("profile rust: not found"), "{out}");
        // Control: the hook's `load` ran git through `bin/git`, so a definition read would be in the log.
        assert!(log_of(&ran, "hook guard < PreToolUse-Bash-main.json").iter().any(|l| l.contains(LOAD_READ)));
        // Control: each verb ran and replied (the prompt hook blocks the line with the verb's reply).
        for s in zygo_core::verbs::VERB_SPECS {
            let out = &ran_of(&ran, &format!("hook prompt < /zygo:{}", s.name)).1;
            let v: Value = serde_json::from_str(out).unwrap_or_else(|e| panic!("/zygo:{}: {e}: {out}", s.name));
            assert_eq!(v["decision"], "block", "/zygo:{}: {out}", s.name);
            assert!(!out.contains("unknown verb"), "/zygo:{}: {out}", s.name);
        }
    }

    #[test]
    fn h2_claude_code_hooks_of_a_member_read_no_definition() { // R-C8; R§10 row; R§18 item 20; review focus 38
        let ran = compare(&|fx| {
            let sd = session(fx, "m", Agent::ClaudeCode, true);
            let mut rows = cc_main(fx);
            rows.extend(cc_member(fx));
            let ran = run_rows(fx, &sd, rows);
            // Control: the member events ran (`workspace --event prompt` writes `activity.json`, W§9.1).
            assert!(sd.activity_json().is_file());
            ran
        });
        assert_eq!(ran.len(), 22 + 9);
        assert!(log_of(&ran, "hook guard < PreToolUse-Bash-main.json").iter().any(|l| l.contains(LOAD_READ)));
    }

    #[test]
    fn h3_opencode_plugin_calls_read_no_definition() { // R-C8; R§10 row; R§18 item 20; review focus 38, F6
        let ran = compare(&|fx| {
            let sd = session(fx, "o", Agent::OpenCode, true);
            let ran = run_rows(fx, &sd, oc_member());
            assert!(sd.activity_json().is_file());
            ran
        });
        assert_eq!(ran.len(), 13);
        assert!(log_of(&ran, "hook guard --agent opencode").iter().any(|l| l.contains(LOAD_READ)));
    }

    const INIT: &str = concat!(
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-06-18\"}}\n",
        "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n",
    );

    /// The arguments of a call of `tool` that pass its argument checks (review F6): a valid begin and a valid keep
    /// (so `work_verdict` reaches `session_config`, `crates/zygo-mcp/src/tools/work.rs:43`, `:226-227`), the stored
    /// result `id` for the result tools, the member's own pane key as the `to` of `mail_send`
    /// (`zygo-workspace/src/mail/names.rs:41`), else `{}`.
    fn tool_args(fx: &Fx, tool: &str, id: &str) -> Value {
        match tool {
            "work_begin" => json!({"unit": "beads:bd-a1b2", "round": 1, "check": "cargo test -p demo", "files": ["src/"]}),
            "work_verdict" => json!({"unit": "beads:bd-a1b2", "round": 1, "verdict": "keep", "head": fx.head,
                "base": fx.head, "dispositions": []}),
            "result_view" | "result_tree" => json!({"id": id}),
            "result_search" => json!({"id": id, "pattern": "beta"}),
            "mail_send" => json!({"to": KEY, "subject": "s", "body": "b"}),
            _ => json!({}),
        }
    }

    /// The text of the reply to the call with JSON-RPC id `id`; asserts it passed the argument checks (no JSON-RPC
    /// error) and returns (text, isError).
    fn reply_of(replies: &[Value], id: usize, tool: &str) -> (String, bool) {
        let r = replies.iter().find(|r| r["id"] == id).unwrap_or_else(|| panic!("{tool}: no reply {id}: {replies:?}"));
        assert!(r.get("error").is_none(), "{tool}: {r}");
        (r["result"]["content"][0]["text"].as_str().unwrap().to_string(), r["result"]["isError"] == true)
    }

    #[test]
    fn h4_mcp_server_and_tools_read_no_definition() { // R-C8; R§10 row; R§18 item 20; review focus 38, F6
        let ran = compare(&|fx| {
            let mut out = Vec::new();
            for (name, agent, member) in [("s", "claude-code", false), ("m", "claude-code", true), ("o", "opencode", true)] {
                let kind = if agent == "opencode" { Agent::OpenCode } else { Agent::ClaudeCode };
                let sd = session(fx, name, kind, member);
                let stored = zygo_results::Store::put(&sd.results(), "Bash", None, RESULT.as_bytes()).unwrap();
                let args = ["mcp", "--agent", agent];
                let list = format!("{INIT}{{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}}\n");
                let (stdout, log) = run(fx, &sd, &args, list.as_bytes());
                let replies: Vec<Value> = stdout.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
                let tools: Vec<String> = replies.iter().find(|r| r["id"] == 2).unwrap()["result"]["tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|t| t["name"].as_str().unwrap().to_string())
                    .collect();
                out.push((format!("mcp {agent} {name} list"), tools.join(","), log));
                let mut calls = INIT.to_string();
                for (i, t) in tools.iter().enumerate() {
                    let c = json!({"jsonrpc": "2.0", "id": 3 + i, "method": "tools/call",
                        "params": {"name": t, "arguments": tool_args(fx, t, &stored.id)}});
                    calls.push_str(&format!("{c}\n"));
                }
                let (stdout, log) = run(fx, &sd, &args, calls.as_bytes());
                assert_eq!(stdout.lines().count(), tools.len() + 1, "{stdout}");
                // Control (review F6): each call passed its argument checks and the named tools ran their main path.
                let replies: Vec<Value> = stdout.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
                for (i, t) in tools.iter().enumerate() {
                    let (text, is_error) = reply_of(&replies, 3 + i, t);
                    match t.as_str() {
                        "result_view" => assert_eq!((text.as_str(), is_error), (RESULT.trim_end(), false), "{t}"),
                        "result_search" | "result_tree" => assert!(!is_error && !text.is_empty(), "{t}: {text}"),
                        "mail_send" => assert!(!is_error && text.contains("\"sent\":[{"), "{t}: {text}"),
                        // `zygo down -- <name>` ran in a child and reached its tmux check (`zygo-workspace/src/down.rs:377`):
                        // `bin/tmux` fails every call, so nothing stops.
                        "workspace_down" => assert!(is_error && text.starts_with("zygo: cannot read the tmux version: "), "{t}: {text}"),
                        _ => {}
                    }
                }
                out.push((format!("mcp {agent} {name} calls"), stdout, log));
            }
            out
        });
        assert_eq!(ran.len(), 6);
        assert_eq!(ran[0].1, "compact,result_view,result_search,result_tree,work_begin,work_verdict");
        for i in [2, 4] {
            for t in ["mail_send", "mail_read", "mail_list", "workspace_down"] {
                assert!(ran[i].1.split(',').any(|n| n == t), "{}: {}", ran[i].0, ran[i].1);
            }
        }
        // Control: each start's `load` (`tool.rs:60`, repo = the cwd) ran git through `bin/git`.
        for i in [0, 2, 4] {
            assert!(ran[i].2.iter().any(|l| l.contains(LOAD_READ)), "{}: {:?}", ran[i].0, ran[i].2);
        }
        // Control (review F2): the `work_verdict` keep reads objects through `cat-file --batch`
        // (`crates/zygo-work/src/gate/git.rs:189`), and `bin/git` logged its stdin, which the predicate judged.
        for i in [1, 3, 5] {
            assert!(ran[i].2.iter().any(|l| l.ends_with(" cat-file --batch")), "{}: {:?}", ran[i].0, ran[i].2);
            assert!(ran[i].2.iter().any(|l| l == &format!("{STDIN}<head>^{{commit}}")), "{}: {:?}", ran[i].0, ran[i].2);
        }
    }
}
