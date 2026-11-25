//! Mail sync between machines over a side branch of the repository's remote (S§1).
pub mod address;
pub mod branch;
pub mod env;
pub mod git;
pub mod identity;
pub mod import;
pub mod ops;
pub mod outbox;
pub mod prune;
pub mod public;
pub mod registry;
pub mod secret;
pub mod spawn;
pub mod state;
pub mod sync;
