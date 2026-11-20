use regex::Regex;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

const FORBIDDEN: &[&str] = &[
    "f50c51ed2315dcf3fa88181cf033f8029cac64f7dea4048327ca032ec102ea74",
    "e512a05583448f44790783f986b1f36925c8cfc42338ca0e1caa637755bd15ae",
    "d75a838dc758ba17f28bd8dbac605cb70c35465263d5733164521de2f7ef7926",
    "80d71140f751394c4712bf46d6b65abfa0184a35553dc2cb1b41546fbe2afa1a",
    "30a989afc82c0a21139573591de4e5ff37994f7d1506a9acf2b5997005c2649f",
    "796e43a5a8cdb73b92b5f59eb50610cea3efa8ce229cd7f0557983091b2b4552",
    "566f532d486c947709d3d0e6b7575af8380248db66dada211d58eb00ad585297",
    "06a1d8bf7fc210de23d9540ab35744ae9db310afa0afbabe9529333e175a850f",
    "3314b0999a53aafeaaf0c0881eb2032a08c6dedcf9924c7a3da3cb1465ffac41",
    "6ac3c336e4094835293a3fed8a4b5fedde1b5e2626d9838fed50693bba00af0e",
    "b690cdbe59f14b3be083db03237e04051abd85c4ffe35180723e9cb45cd3a519",
    "bb61ef40814ce34c1edf0edb609854be9793198a8f60b67d9fb26643c32281d3",
    "31506a8448a761a448a08aa69d9116ea8a6cb1c6b3f4244b3043051f69c9cc3c",
    "2f5f6ce5ae30b54aa5d7ced1ba566982bab34ba2814a51ce1865d2c2d8815cd4",
    "b04a8a40063584477f690200cb4262563f36068984ab4f53a51f26bf8e120215",
    "0f28c4960d96647e77e7ab6d13b85bd16c7ca56f45df802cdc763a5e5c0c7863",
    "85fc17f7069acd39a5c636cd0a6530651096128da447959f5e250824857dc559",
    "986c99004163a6c5155bbd2bfadc37bee9e279b73c669211f432910425b8e6dd",
    "c2c3b68b48832afd9a4dbdd474c1b6c81c8baecdb71446f9947dac72dd0fe93d",
    "dd92623b0a4b255f87cc4aaee7990ee182d91db49189df6229ce65b5e9d960da",
    "bdb1cc258d6976aafd3a0c3399dfff42a486e63a6948d8bc2e10e7c36fab7611",
    "9ae315a94e428a7ee3b5e48adae6541965d93b86acf10ffa1c45b93b6fe577b4",
];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn files() -> Vec<PathBuf> {
    let root = root();
    let git = Command::new("git")
        .args(["ls-files", "-z", "-co", "--exclude-standard"])
        .current_dir(&root)
        .output();
    if let Ok(o) = git
        && o.status.success() {
            return o
                .stdout
                .split(|b| *b == 0)
                .filter(|n| !n.is_empty())
                .map(|n| root.join(rel_path(n)))
                .filter(|p| p.is_file())
                .collect();
        }
    walkdir::WalkDir::new(&root)
        .into_iter()
        .filter_entry(|e| {
            let n = e.file_name().to_string_lossy();
            !(e.file_type().is_dir() && (n == "target" || n == ".git" || n == ".worktrees"))
        })
        .map(|e| e.unwrap_or_else(|err| panic!("unreadable entry: {err}")))
        .filter(|e| e.file_type().is_file())
        .map(|e| e.into_path())
        .collect()
}

#[cfg(unix)]
fn rel_path(name: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    PathBuf::from(std::ffi::OsStr::from_bytes(name))
}

/// Git prints paths as UTF-8 on Windows.
#[cfg(not(unix))]
fn rel_path(name: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(name).into_owned())
}

fn digest(word: &str) -> String {
    hex::encode(Sha256::digest(word.as_bytes()))
}

/// ASCII letter runs of 3..=40 characters whose digest is in `set`.
fn hits(line: &str, set: &HashSet<&str>) -> Vec<String> {
    let mut found = Vec::new();
    let mut run = String::new();
    let mut flush = |run: &mut String| {
        if (3..=40).contains(&run.len()) && set.contains(digest(&run.to_lowercase()).as_str()) {
            found.push(run.clone());
        }
        run.clear();
    };
    for c in line.chars() {
        if c.is_ascii_alphabetic() {
            run.push(c);
        } else {
            flush(&mut run);
        }
    }
    flush(&mut run);
    found
}

const OLD_CREDS: &[&str] = &[
    r"sk-ant-[a-z]{3}[0-9]{2}-[A-Za-z0-9_-]{20,}",
    r"sk-(proj-)?[A-Za-z0-9]{32,}",
    r"AKIA[0-9A-Z]{16}",
    r"gh[pousr]_[A-Za-z0-9]{36}",
    r"xox[abprs]-[A-Za-z0-9-]{10,}",
    r"-----BEGIN [A-Z ]*PRIVATE KEY-----",
];

/// The built-in pattern that matches config examples such as `token = "keychain:…"` by design.
const SKIPPED_BUILTIN: &str = "generic_secret_assignment";

/// The old six plus every built-in secret pattern (§16.1) except `SKIPPED_BUILTIN`.
fn creds() -> Vec<Regex> {
    OLD_CREDS
        .iter()
        .map(|r| r.to_string())
        .chain(zygo_config::builtin_patterns().into_iter().filter(|p| p.id != SKIPPED_BUILTIN).map(|p| p.pattern))
        .map(|r| Regex::new(&r).unwrap())
        .collect()
}

fn findings(line: &str, set: &HashSet<&str>, creds: &[Regex]) -> Vec<&'static str> {
    let mut out = Vec::new();
    if !hits(line, set).is_empty() {
        out.push("forbidden word");
    }
    if creds.iter().any(|r| r.is_match(line)) {
        out.push("credential-shaped string");
    }
    out
}

fn scan(paths: &[PathBuf], set: &HashSet<&str>, creds: &[Regex]) -> Vec<String> {
    let mut bad = Vec::new();
    for p in paths {
        // A file removed after the listing (the `file_list` probe) has nothing to scan.
        let bytes = match std::fs::read(p) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => panic!("unreadable {}: {e}", p.display()),
        };
        let text = String::from_utf8_lossy(&bytes);
        for (i, line) in text.lines().enumerate() {
            for f in findings(line, set, creds) {
                bad.push(format!("{}:{}: {f}", p.display(), i + 1));
            }
        }
    }
    bad
}

#[test]
fn no_forbidden_content() {
    let set: HashSet<&str> = FORBIDDEN.iter().copied().collect();
    let bad = scan(&files(), &set, &creds());
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// One sample per built-in id, built at run time so this file holds no credential-shaped literal.
fn samples() -> Vec<(&'static str, String)> {
    let d = "-".repeat(5);
    let a = |n: usize| "A".repeat(n);
    let h = |n: usize| "a1".repeat(n / 2);
    vec![
        ("anthropic_api_key", format!("sk-{}-api03-{}", "ant", a(40))),
        ("openrouter_api_key", format!("sk-{}-v1-{}", "or", "0f".repeat(32))),
        ("openai_api_key", ["sk-", "proj-", &a(20), "T3Blbk", "FJ", &a(20)].concat()),
        ("google_api_key", ["AI", "za", &a(35)].concat()),
        ("aws_access_key_id", ["AK", "IA", &"B".repeat(16), " "].concat()),
        ("azure_storage_key", format!("Account{}={}==", "Key", a(86))),
        ("gcp_service_account_key", format!("\"private_key\": \"{d}BEGIN PRIVATE KEY{d}x\"")),
        ("github_token", format!("gh{}_{}", "p", h(36))),
        ("gitlab_token", format!("gl{}-{}", "pat", a(20))),
        ("slack_token", format!("xo{}-{}", "xb", h(12))),
        ("private_key_pem", format!("{d}BEGIN RSA PRIVATE KEY{d}")),
        ("pgp_private_key_block", format!("{d}BEGIN PGP PRIVATE KEY BLOCK{d}")),
        ("jwt", ["ey", "J", &a(8), ".ey", "J", &a(8), ".", &a(8)].concat()),
        ("zygo_token", format!("zy{}_s_{}", "g", a(43))),
    ]
}

#[test]
fn every_builtin_kind_is_caught() {
    let builtin: Vec<_> = zygo_config::builtin_patterns().into_iter().filter(|p| p.id != SKIPPED_BUILTIN).collect();
    let samples = samples();
    let ids: Vec<&str> = samples.iter().map(|(i, _)| *i).collect();
    let want: Vec<&str> = builtin.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids, want, "one sample per built-in pattern, in order");
    let srcs: Vec<String> = creds().iter().map(|r| r.as_str().to_string()).collect();
    for ((id, s), p) in samples.iter().zip(&builtin) {
        assert!(Regex::new(&p.pattern).unwrap().is_match(s), "{id}: sample does not match its own pattern");
        assert!(srcs.contains(&p.pattern), "{id}: pattern missing from creds()");
    }
}

#[test]
fn non_utf8_file_is_scanned() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("bin.dat");
    let mut bytes = vec![0xff, 0xfe, b'\n'];
    bytes.extend(format!("gl{}-{}\n", "pat", "A".repeat(20)).into_bytes());
    std::fs::write(&p, bytes).unwrap();
    let set: HashSet<&str> = HashSet::new();
    let bad = scan(std::slice::from_ref(&p), &set, &creds());
    assert_eq!(bad, vec![format!("{}:2: credential-shaped string", p.display())]);
}

#[test]
fn file_list() {
    struct Probe(PathBuf);
    impl Drop for Probe {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    // An untracked, non-ASCII file name must be scanned.
    let probe = Probe(root().join(format!("probe-\u{e9}-{}.txt", std::process::id())));
    std::fs::write(&probe.0, "benign").unwrap();
    let list = files();
    assert!(!list.is_empty());
    for want in [root().join("Cargo.toml"), root().join("tests/forbidden_content.rs"), probe.0.clone()] {
        assert!(list.contains(&want), "file list lacks {}", want.display());
    }
}

#[test]
fn list_has_no_clear_words() {
    for d in FORBIDDEN {
        assert_eq!(d.len(), 64);
        assert!(d.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')));
    }
}

#[test]
fn self_check() {
    let local = digest("zygo");
    let set: HashSet<&str> = [local.as_str()].into_iter().collect();
    assert_eq!(hits("a Zygo b", &set), vec!["Zygo".to_string()]);
    assert!(hits("other words", &set).is_empty());
}
