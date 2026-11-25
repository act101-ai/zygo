#[test]
fn plain() {}

#[tokio::test]
async fn tokio_plain() {}

#[tokio::test(flavor = "multi_thread")]
async fn tokio_multi() {}

#[test]
#[ignore]
fn ignored_plain() {}

#[test]
#[ignore = "needs a live key"]
fn ignored_reason() {}

#[test]
#[cfg_attr(windows, ignore)]
fn ignored_cfg_attr() {}

fn helper_not_test() {}

#[cfg(unix)]
#[test]
fn unix_only() {}

#[cfg(windows)]
mod win {
    #[test]
    fn in_windows_mod() {}
}

#[cfg(test)]
mod inner {
    #[test]
    fn in_cfg_test_mod() {}
}

mod a {
    #[test]
    fn dup() {}
}

mod b {
    #[test]
    fn dup() {}
}

#[path = "sub/reached.rs"]
mod reached;

#[cfg(windows)]
#[path = "sub/win.rs"]
mod win_file;

#[path = "fixtures/f.rs"]
mod f;
