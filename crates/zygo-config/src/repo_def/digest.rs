//! The digest of `.zygo/` (R§6.5).

use sha2::{Digest, Sha256};

use super::Files;

/// R§6.5: SHA-256 over, per file in R§6.4 order, `path 0x00 len-decimal 0x00 bytes`; 64 lower-case hex.
pub fn digest(files: &Files) -> String {
    let mut h = Sha256::new();
    let ws = files.workspace.iter().map(|b| (".zygo/workspace.toml".to_string(), b));
    let ps = files.profiles.iter().map(|(n, b)| (format!(".zygo/profiles/{n}.toml"), b));
    for (path, bytes) in ws.chain(ps) {
        h.update(path.as_bytes());
        h.update([0u8]);
        h.update(bytes.len().to_string().as_bytes());
        h.update([0u8]);
        h.update(bytes);
    }
    hex::encode(h.finalize())
}

/// The first 12 characters, for one-line messages (R§6.5).
pub fn short(digest: &str) -> &str {
    digest.get(..12).unwrap_or(digest)
}
