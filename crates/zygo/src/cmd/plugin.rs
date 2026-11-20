/// `zygo plugin check <dir>` (§15.6): print the report; 0 when every plugin and the marketplace are clean.
pub fn run(dir: &str) -> i32 {
    let r = zygo_plugins::check::check_dir(std::path::Path::new(dir));
    super::print_out(&r.render());
    if r.ok() { 0 } else { 1 }
}
