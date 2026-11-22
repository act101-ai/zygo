//! Token estimates from byte counts (H§6.1 OP1).

/// Bytes per estimated token: the proxy's `count_tokens` estimate (§7.3) and the HUD's trimmed tokens.
pub const BYTES_PER_TOKEN: u64 = 4;
