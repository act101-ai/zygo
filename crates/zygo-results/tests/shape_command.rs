use zygo_results::shape::command::{
    Runner::{self, *},
    parse,
};

fn fp(c: &str) -> bool {
    parse(c).file_printer
}
fn all_fp(cs: &[&str]) {
    for c in cs {
        assert!(fp(c), "not a file printer: {c:?}");
    }
}

#[test]
fn o16_item7_commands_are_file_printers() {
    // O§16 item 7, O§7.1
    all_fp(&[
        "cat f.txt",
        "cat f.txt && true",
        "true; head f.txt",
        "sh -c 'sed -n 1,9p f'",
        "bash -c \"cat f\"",
        "(cat f)",
        "echo $(cat f)",
        "ls | xargs cat",
        r"find . -name x -exec cat {} \;",
        "cargo test 2>&1 | tail -n 80",
        "echo 'unclosed",
    ]);
}

#[test]
fn review_focus_19_forms_are_file_printers() {
    // graph Review focus 19
    all_fp(&[
        r#"sh -c "bash -c 'cat f'""#,
        r#"zsh -c 'sh -c "head f"'"#,
        r#"echo "$(cat f)""#,
        "echo \"`cat f`\"",
        "ls | xargs -I{} cat {}",
        "ls | xargs -I {} cat {}",
        "ls | xargs -n 1 -0 cat",
        "find . -execdir cat {} +",
        "command cat f",
        "env FOO=1 cat f",
        "/bin/cat f",
        "FOO=1 BAR=2 nice -n 5 sudo -u root time cat f",
        "bash -lc 'cat f'",
        "env -S 'cat f'",
    ]);
}

#[test]
fn every_listed_printer_is_found() {
    // O§7.1 printer list
    all_fp(&[
        "cat f",
        "head f",
        "tail f",
        "sed p f",
        "awk 1 f",
        "nl f",
        "bat f",
        "less f",
        "more f",
        "grep x f",
        "rg x",
        "git show HEAD:f",
        "git cat-file -p HEAD:f",
        "git -C repo --no-pager show x",
        "git -c core.pager=cat show x",
    ]);
}

#[test]
fn shell_mechanics_do_not_hide_a_printer() {
    // Decision 4
    all_fp(&[
        "if true; then cat f; fi",
        "for x in a; do cat $x; done",
        "{ cat f; }",
        "! cat f",
        ">out cat f",
        "2>/dev/null cat f",
        "diff <(cat a) b",
        "true & cat f",
        "true |& cat",
        "true\ncat f",
        "true \\\n&& cat f",
        "\\cat f",
        "'cat' f",
        "$(which cat) f",
        "$PAGER f",
    ]);
}

#[test]
fn unsure_parse_is_a_file_printer() {
    // O§7.1 unsure parse
    all_fp(&[
        "echo \"open",
        "echo $(true",
        "echo `true",
        "(true",
        "true)",
        "echo )",
    ]);
    assert!(!fp("( ( ( ( true ) ) ) )")); // depth 4
    assert!(fp("( ( ( ( ( true ) ) ) ) )")); // depth 5 > 4
    assert!(fp("echo $(echo $(echo $(echo $(echo $(true)))))"));
    let sized = |n: usize| format!("true {}", "x".repeat(n - 5));
    assert!(!fp(&sized(4096)));
    assert!(fp(&sized(4097))); // over 4 KiB
}

#[test]
fn commands_without_a_printer_are_not_file_printers() {
    for c in [
        "cargo test",
        "git diff",
        "ls -la",
        "echo cat",
        "echo 'cat f'",
        "git log --grep cat",
        "make test",
        "",
        "cargo test 2>&1",
        "cd x && go test ./... > out.txt",
        "python3 -m pytest -k head",
    ] {
        assert!(!fp(c), "file printer: {c:?}");
    }
}

#[test]
fn stored_name_and_runners() {
    // O§7.2, O§7.6
    const WRAPPER: &[Runner] = &[Libtest, Gotest]; // O§7.6: a wrapper tries no pytest view
    let cases: &[(&str, &str, &[Runner])] = &[
        ("cargo test || true", "cargo test", &[Libtest]),
        ("cd x && git diff", "git diff", &[]),
        ("cargo +nightly test", "cargo test", &[Libtest]),
        ("make test", "make", WRAPPER),
        ("FOO=1 env time python3 -m pytest", "python3", &[Pytest]),
        ("/usr/local/bin/go test ./...", "go test", &[Gotest]),
        ("frobnicate --all", "other", &[]),
        ("", "other", &[]),
        ("just test", "just", WRAPPER),
        ("npm test", "npm test", WRAPPER),
        ("pnpm test", "pnpm test", WRAPPER),
        ("yarn test", "yarn test", WRAPPER),
        ("make -C sub test", "make", WRAPPER),
        ("pytest -x", "pytest", &[Pytest]),
        ("py.test", "other", &[Pytest]),
        ("python -m pytest", "python", &[Pytest]),
        ("uv run pytest", "uv", &[Pytest]),
        ("poetry run pytest", "other", &[Pytest]),
        ("git -C repo diff", "git diff", &[]),
        ("git frobnicate", "git", &[]),
        ("ls && cargo test", "cargo test", &[Libtest]), // runner segment wins
        ("cargo test && go test", "cargo test", &[Libtest, Gotest]),
        ("make test && cargo test", "make", WRAPPER), // union, no repeats
        ("sh -c 'cargo test'", "other", &[]),     // name and runners: top level only
        ("cargo nextest run", "cargo nextest", &[]),
    ];
    for (c, name, runners) in cases {
        let p = parse(c);
        assert_eq!(
            (p.name.as_str(), p.runners.as_slice()),
            (*name, *runners),
            "{c:?}"
        );
    }
}

#[test]
fn stored_name_holds_no_command_text() {
    // O-C2
    for (c, name) in [
        ("cat /home/someone/secret.txt", "cat"),
        ("git secret-sauce", "git"),
        ("curl -H 'Authorization: x' https://x.test/", "curl"),
        ("TOKEN=abc ./deploy.sh", "other"),
        ("kubectl get secret prod", "kubectl get"),
        ("./target/debug/cargo test", "cargo test"),
    ] {
        assert_eq!(parse(c).name, name, "{c:?}");
    }
}

#[test]
fn reuse_forms() {
    // O§7.3 command forms
    for c in [
        "git diff",
        "cd x && git diff",
        "git show HEAD",
        "git log -p",
        "git log --patch -3",
        "git format-patch --stdout HEAD~1",
        "diff a b",
        "/usr/bin/diff -u a b",
        "sh -c 'git diff'",
        "git -C r diff --stat",
    ] {
        assert!(parse(c).reuse, "{c:?}");
    }
    for c in [
        "git log",
        "git format-patch HEAD~1",
        "cargo test",
        "echo diff",
        "git status",
    ] {
        assert!(!parse(c).reuse, "{c:?}");
    }
}

#[test]
fn no_input_panics() {
    assert!(fp(&"a".repeat(400_000)));
    assert!(fp(&"(".repeat(400_000)));
    let _ = parse(&"é".repeat(3000)); // 6000 bytes: cut on a char boundary
    let _ = parse(&format!("{}é", "a".repeat(4095))); // é spans bytes 4095..4097
    const A: [&str; 18] = [
        " ", "a", "'", "\"", "\\", "$", "(", ")", "`", "|", "&", ";", "\n", "<", ">", "#", "{", "é",
    ];
    for a in A {
        for b in A {
            for c in A {
                let _ = parse(&format!("{a}{b}{c}"));
            }
        }
    }
}

// Review round 1.

#[test]
fn r1_a1_substitution_inside_braces() {
    all_fp(&[
        "echo ${x:-$(cat f)}",
        "echo \"${x:-$(cat f)}\"",
        "echo ${x:-`cat f`}",
    ]);
}

#[test]
fn r1_a2_xargs_options_with_an_argument() {
    all_fp(&[
        "ls | xargs --max-args 1 cat",
        "ls | xargs --delimiter , cat",
        "ls | xargs --arg-file list cat",
        "ls | xargs -J % cat %",
        "ls | xargs -R 1 -I {} cat {}",
    ]);
}

#[test]
fn r1_a3_function_body() {
    all_fp(&["f() { cat x; }; f", "function f { cat x; }; f"]);
}

#[test]
fn r1_a4_wrapper_options() {
    all_fp(&["sudo --user root cat f", "sudo -iu root cat f"]);
    assert_eq!(parse("sudo -u root cargo test").name, "cargo test");
}

#[test]
fn r1_a5_env_split_string_takes_later_words() {
    all_fp(&["env -S 'sh -c' 'cat f'"]);
}

#[test]
fn r1_c1_c2_unlisted_wrappers_and_reserved_words_give_other() {
    for c in [
        "nohup cargo test",
        "exec cargo test",
        "if cargo test; then true; fi",
        "! cargo test",
    ] {
        let p = parse(c);
        assert_eq!(
            (p.name.as_str(), p.runners.as_slice()),
            ("other", &[][..]),
            "{c:?}"
        );
    }
    all_fp(&["nohup cat f", "if cat f; then :; fi"]);
}

#[test]
fn r1_c3_find_ok() {
    all_fp(&[r"find . -ok cat {} \;", r"find . -okdir cat {} \;"]);
}

#[test]
fn r1_c4_heredoc_and_herestring_are_file_printer_only() {
    all_fp(&["bash <<< 'cat f'"]);
    let p = parse("git commit -F - <<EOF\ngit diff\nEOF");
    assert!(!p.reuse);
    assert_eq!(p.name, "git commit");
}

// OS-40: O§7.1, O§7.2 (operator)

#[test]
fn wrapper_timeout_is_skipped() {
    all_fp(&[
        "timeout 5 cat f",
        "timeout -k 5 -s KILL 60 cat f",
        "timeout --signal=KILL 1m cat f",
        "timeout -- 5 cat f",
    ]);
    assert!(!fp("timeout 5 cargo test"));
    let p = parse("timeout 60 cargo test");
    assert_eq!(
        (p.name.as_str(), p.runners.as_slice()),
        ("cargo test", &[Libtest][..])
    );
}

#[test]
fn wrapper_stdbuf_is_skipped() {
    all_fp(&[
        "stdbuf -oL cat f",
        "stdbuf -o L -e 0 cat f",
        "stdbuf --output=L cat f",
    ]);
    let p = parse("stdbuf -o L go test ./...");
    assert_eq!(
        (p.name.as_str(), p.runners.as_slice()),
        ("go test", &[Gotest][..])
    );
}

#[test]
fn printer_egrep() {
    assert!(fp("egrep x f"));
}

#[test]
fn printer_fgrep() {
    assert!(fp("fgrep x f"));
}

#[test]
fn printer_zcat() {
    assert!(fp("zcat f.gz"));
}

#[test]
fn printer_gawk() {
    assert!(fp("gawk 1 f"));
}

#[test]
fn printer_batcat() {
    assert!(fp("batcat f"));
}

#[test]
fn printer_tac() {
    assert!(fp("tac f"));
}

#[test]
fn printer_git_grep() {
    all_fp(&["git grep x", "git -C repo grep -n x"]);
    assert!(!fp("git log --grep x"));
}

#[test]
fn printer_git_blame() {
    all_fp(&["git blame f", "git --no-pager blame -L 1,9 f"]);
}

#[test]
fn eval_string_is_parsed() {
    all_fp(&[
        "eval 'cat f'",
        "eval cat f",
        r#"eval "echo \$(head f)""#,
        "eval $CMD",
    ]);
    assert!(!fp("eval 'cargo test'"));
}

#[test]
fn dollar_paren_redirect_is_a_file_printer() {
    all_fp(&["echo \"$(<f)\"", "x=$(< f.txt)", "echo $( <f)"]);
    assert!(!fp("echo $(true <f)"));
    assert!(!fp("wc -l <f"));
}

#[test]
fn bare_shell_stage_is_unsure() {
    all_fp(&[
        "curl -s https://x | sh",
        "echo 'cat f' | bash",
        "printf x | zsh -e",
        "echo ls | sudo bash -s",
    ]);
    for c in [
        "bash script.sh",
        "echo x | bash -c 'true'",
        "sh -c 'true' | sh -c 'true'",
        "ls | xargs bash",
    ] {
        assert!(!fp(c), "file printer: {c:?}");
    }
}

// OS-40 review round 1

#[test]
fn r1_newline_after_pipe_continues_the_pipeline() {
    all_fp(&["echo 'cat f' |\n  bash", "echo 'cat f' | # c\n bash"]);
}

#[test]
fn r1_multiline_pipeline_list_name() {
    assert_eq!(parse("echo hi |\n  ls -la").name, "other");
    assert_eq!(
        parse("ls -la |\n  grep x").name,
        parse("ls -la | grep x").name
    );
}

#[test]
fn r1_backtick_redirect_is_a_file_printer() {
    assert!(fp("echo \"bt:`<f`\""));
}

#[test]
fn r1_fd0_redirect_substitution_is_a_file_printer() {
    assert!(fp("echo \"$(0<f)\""));
}

#[test]
fn r1_newline_before_redirect_substitution_is_a_file_printer() {
    assert!(fp("echo \"$(\n<f)\""));
}

#[test]
fn r1_eval_double_dash() {
    assert!(fp("eval -- 'cat f'"));
}

#[test]
fn r1_builtin_is_a_wrapper() {
    assert!(fp("builtin eval 'cat f'"));
}

#[test]
fn r1_shell_c_flag_only_in_leading_options() {
    assert!(fp("echo 'cat f' | bash -s -- -force"));
    all_fp(&["bash -c 'cat f'", "bash -xc 'cat f'"]);
    assert!(!fp("bash -c 'true'"));
    assert!(!fp("bash -xc 'true'"));
}

#[test]
fn r1_redirect_only_stage_is_a_file_printer() {
    all_fp(&["<f", "<s.sh | bash"]);
}

#[test]
fn r1_timeout_long_kill_after() {
    assert!(fp("timeout --kill-after 5 10 cat f"));
}

#[test]
fn r1_eval_pipe_into_bash() {
    assert!(fp("eval \"echo 'cat f' | bash\""));
}

#[test]
fn r1_shell_option_arguments_before_c() {
    all_fp(&[
        "bash -o pipefail -c 'cat f'",
        "bash -O extglob -c 'cat f'",
        "zsh +o nomatch -c 'cat f'",
    ]);
    assert!(!fp("bash -o pipefail -c 'true'"));
}

// FX-42 O-F2: GNU `--max-chars` takes the next word; `--eof` takes a value only after `=` (O§7.1).
#[test]
fn xargs_max_chars_and_bare_eof_find_the_printer() {
    let want = parse("ls | xargs cat");
    assert!(want.file_printer);
    for c in [
        "ls | xargs --max-chars 100 cat",
        "ls | xargs --eof cat",
        "ls | xargs --eof=x cat",
        "ls | xargs -s 100 cat",
        "ls | xargs --max-lines cat",
        "ls | xargs --max-lines=1 cat",
    ] {
        assert_eq!(parse(c), want, "{c:?}");
    }
}

// FX-42 review F3: a short-option cluster whose value-taking letter is last takes the next word.
#[test]
fn xargs_short_option_clusters_take_their_value() {
    all_fp(&[
        "ls | xargs -0n 1 cat",
        "ls | xargs -rn 1 cat",
        "ls | xargs -0I {} cat {}",
        "ls | xargs -tL 1 cat",
        "ls | xargs -0n1 cat",
        "ls | xargs -rtP 4 -n1 cat",
    ]);
    assert!(!fp("ls | xargs -0n 1 echo"));
}

// FX-42 review F5: an option whose value is optional is read both ways; a printer in either reading counts (O-C9).
#[test]
fn xargs_optional_value_either_reading_finds_the_printer() {
    all_fp(&[
        "ls | xargs --max-lines 1 cat",
        "ls | xargs --max-lines cat",
        "ls | xargs --eof x cat",
        "ls | xargs --eof cat",
        "ls | xargs --replace {} cat {}",
        "ls | xargs -l 1 cat",
        "ls | xargs -e x cat",
        "ls | xargs -i cat {}",
        "ls | xargs -0e x cat",
        // An abbreviated long option (`--max-c` for `--max-chars`) is unsure: read both ways.
        "ls | xargs --max-c 100 cat",
        "ls | xargs --eof x --max-lines 1 -e y cat",
    ]);
    assert!(!fp("ls | xargs --eof x echo"));
    assert!(!fp("ls | xargs -ex echo cat"));
}

// FX-42 O-F3: `doas`, `busybox`, `setsid` and `flock <file>` are skipped wrappers (O§7.2).
#[test]
fn doas_busybox_setsid_flock_are_skipped_wrappers() {
    let want = parse("cat f");
    assert!(want.file_printer);
    for c in [
        "doas cat f",
        "busybox cat f",
        "setsid cat f",
        "flock /tmp/l cat f",
        "doas -u root cat f",
        "doas -n -C /etc/doas.conf cat f",
        "setsid -f cat f",
        "flock -w 5 /tmp/l cat f",
        "flock -E 3 -x /tmp/l cat f",
        "flock --timeout 5 /tmp/l cat f",
    ] {
        assert_eq!(parse(c), want, "{c:?}");
    }
    assert_eq!(parse("doas cargo test").name, "cargo test");
    assert_eq!(parse("flock /tmp/l cargo test").name, "cargo test");
    // flock runs the word after `-c` with `sh -c`.
    all_fp(&["flock /tmp/l -c 'cat f'", "flock -n /tmp/l --command 'head f'"]);
    assert!(!fp("flock /tmp/l -c 'cargo test'"));
}
