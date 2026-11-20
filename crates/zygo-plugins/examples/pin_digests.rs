//! The pin script's tool (`scripts/pin-builtin-marketplace.sh`): `render` prints the approved data file,
//! `fixture` writes the trimmed, neutralized fixture tree.

use std::path::Path;
use std::process::ExitCode;

use zygo_plugins::builtin::{render_approved, write_fixture};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let run = match args.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["render", root, r, c, plugins @ ..] if !plugins.is_empty() => {
            let names: Vec<String> = plugins.iter().map(|s| s.to_string()).collect();
            render_approved(Path::new(root), r, c, &names).map(|t| print!("{t}"))
        }
        ["fixture", clone, tree, plugins @ ..] if !plugins.is_empty() => {
            let names: Vec<String> = plugins.iter().map(|s| s.to_string()).collect();
            write_fixture(Path::new(clone), Path::new(tree), &names)
        }
        _ => {
            eprintln!("usage: pin_digests (render <root> <ref> <commit> | fixture <clone> <tree>) <plugin>...");
            return ExitCode::from(2);
        }
    };
    match run {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}
