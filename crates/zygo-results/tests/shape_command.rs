use zygo_results::shape::command::{parse, Runner::{self, *}};

fn fp(c: &str) -> bool { parse(c).file_printer }
fn all_fp(cs: &[&str]) { for c in cs { assert!(fp(c), "not a file printer: {c:?}"); } }

#[test]
fn o16_item7_commands_are_file_printers() {                  // O§16 item 7, O§7.1
    all_fp(&["cat f.txt", "cat f.txt && true", "true; head f.txt", "sh -c 'sed -n 1,9p f'",
        "bash -c \"cat f\"", "(cat f)", "echo $(cat f)", "ls | xargs cat",
        r"find . -name x -exec cat {} \;", "cargo test 2>&1 | tail -n 80", "echo 'unclosed"]);
}

#[test]
fn review_focus_19_forms_are_file_printers() {               // graph Review focus 19
    all_fp(&[r#"sh -c "bash -c 'cat f'""#, r#"zsh -c 'sh -c "head f"'"#, r#"echo "$(cat f)""#,
        "echo \"`cat f`\"", "ls | xargs -I{} cat {}", "ls | xargs -I {} cat {}", "ls | xargs -n 1 -0 cat",
        "find . -execdir cat {} +", "command cat f", "env FOO=1 cat f", "/bin/cat f",
        "FOO=1 BAR=2 nice -n 5 sudo -u root time cat f", "bash -lc 'cat f'", "env -S 'cat f'"]);
}

#[test]
fn every_listed_printer_is_found() {                         // O§7.1 printer list
    all_fp(&["cat f", "head f", "tail f", "sed p f", "awk 1 f", "nl f", "bat f", "less f", "more f",
        "grep x f", "rg x", "git show HEAD:f", "git cat-file -p HEAD:f", "git -C repo --no-pager show x",
        "git -c core.pager=cat show x"]);
}

#[test]
fn shell_mechanics_do_not_hide_a_printer() {                 // Decision 4
    all_fp(&["if true; then cat f; fi", "for x in a; do cat $x; done", "{ cat f; }", "! cat f",
        ">out cat f", "2>/dev/null cat f", "diff <(cat a) b", "true & cat f", "true |& cat",
        "true\ncat f", "true \\\n&& cat f", "\\cat f", "'cat' f", "$(which cat) f", "$PAGER f"]);
}

#[test]
fn unsure_parse_is_a_file_printer() {                        // O§7.1 unsure parse
    all_fp(&["echo \"open", "echo $(true", "echo `true", "(true", "true)", "echo )"]);
    assert!(!fp("( ( ( ( true ) ) ) )"));                    // depth 4
    assert!(fp("( ( ( ( ( true ) ) ) ) )"));                 // depth 5 > 4
    assert!(fp("echo $(echo $(echo $(echo $(echo $(true)))))"));
    let sized = |n: usize| format!("true {}", "x".repeat(n - 5));
    assert!(!fp(&sized(4096)));
    assert!(fp(&sized(4097)));                               // over 4 KiB
}

#[test]
fn commands_without_a_printer_are_not_file_printers() {
    for c in ["cargo test", "git diff", "ls -la", "echo cat", "echo 'cat f'", "git log --grep cat",
        "make test", "", "cargo test 2>&1", "cd x && go test ./... > out.txt", "python3 -m pytest -k head"] {
        assert!(!fp(c), "file printer: {c:?}");
    }
}

#[test]
fn stored_name_and_runners() {                               // O§7.2, O§7.6
    const ALL: &[Runner] = &[Libtest, Pytest, Gotest];
    let cases: &[(&str, &str, &[Runner])] = &[
        ("cargo test || true", "cargo test", &[Libtest]),
        ("cd x && git diff", "git diff", &[]),
        ("cargo +nightly test", "cargo test", &[Libtest]),
        ("make test", "make", ALL),
        ("FOO=1 env time python3 -m pytest", "python3", &[Pytest]),
        ("/usr/local/bin/go test ./...", "go test", &[Gotest]),
        ("frobnicate --all", "other", &[]),
        ("", "other", &[]),
        ("just test", "just", ALL), ("npm test", "npm test", ALL),
        ("pnpm test", "pnpm test", ALL), ("yarn test", "yarn test", ALL),
        ("make -C sub test", "make", ALL),
        ("pytest -x", "pytest", &[Pytest]), ("py.test", "other", &[Pytest]),
        ("python -m pytest", "python", &[Pytest]), ("uv run pytest", "uv", &[Pytest]),
        ("poetry run pytest", "other", &[Pytest]),
        ("git -C repo diff", "git diff", &[]), ("git frobnicate", "git", &[]),
        ("ls && cargo test", "cargo test", &[Libtest]),       // runner segment wins
        ("cargo test && go test", "cargo test", &[Libtest, Gotest]),
        ("make test && cargo test", "make", ALL),             // union, no repeats
        ("sh -c 'cargo test'", "other", &[]),                 // name and runners: top level only
        ("cargo nextest run", "cargo nextest", &[]),
    ];
    for (c, name, runners) in cases {
        let p = parse(c);
        assert_eq!((p.name.as_str(), p.runners.as_slice()), (*name, *runners), "{c:?}");
    }
}

#[test]
fn stored_name_holds_no_command_text() {                     // O-C2
    for (c, name) in [("cat /home/someone/secret.txt", "cat"), ("git secret-sauce", "git"),
        ("curl -H 'Authorization: x' https://x.test/", "curl"), ("TOKEN=abc ./deploy.sh", "other"),
        ("kubectl get secret prod", "kubectl get"), ("./target/debug/cargo test", "cargo test")] {
        assert_eq!(parse(c).name, name, "{c:?}");
    }
}

#[test]
fn reuse_forms() {                                           // O§7.3 command forms
    for c in ["git diff", "cd x && git diff", "git show HEAD", "git log -p", "git log --patch -3",
        "git format-patch --stdout HEAD~1", "diff a b", "/usr/bin/diff -u a b", "sh -c 'git diff'",
        "git -C r diff --stat"] { assert!(parse(c).reuse, "{c:?}"); }
    for c in ["git log", "git format-patch HEAD~1", "cargo test", "echo diff", "git status"] {
        assert!(!parse(c).reuse, "{c:?}");
    }
}

#[test]
fn no_input_panics() {
    assert!(fp(&"a".repeat(400_000)));
    assert!(fp(&"(".repeat(400_000)));
    let _ = parse(&"é".repeat(3000));                        // 6000 bytes: cut on a char boundary
    let _ = parse(&format!("{}é", "a".repeat(4095)));        // é spans bytes 4095..4097
    const A: [&str; 18] = [" ", "a", "'", "\"", "\\", "$", "(", ")", "`", "|", "&", ";", "\n", "<", ">", "#", "{", "é"];
    for a in A { for b in A { for c in A { let _ = parse(&format!("{a}{b}{c}")); } } }
}
