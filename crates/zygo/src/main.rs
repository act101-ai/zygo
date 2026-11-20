fn main() {
    zygo::install_panic_hook();
    #[cfg(feature = "test-hooks")]
    zygo::test_panic();
    std::process::exit(zygo::cmd::run());
}
