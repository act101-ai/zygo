//! Shaping fixture harness (O§8.1): reads the cases under `fixtures/shaping/<shaper>/<case>/`.
#![allow(dead_code)]

use std::path::PathBuf;

/// The repository root.
pub fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
