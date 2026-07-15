//! AzZork library crate.
//!
//! Exposes the reusable pieces of the game — the command [`parser`], the
//! [`world`] model, and the [`backend`] abstraction — so they can be driven by
//! the `azork` binary *and* exercised directly from the integration tests in
//! `tests/`.

/// The single source of truth for the running AzZork version.
///
/// Defaults to the crate version baked in at compile time. A release build may
/// override it by setting `AZORK_RELEASE_VERSION` in the build environment
/// (used by the release workflow so the binary self-reports the tagged
/// version). A `match` — not `Option::unwrap_or` — is used so the value stays a
/// compile-time constant.
pub const VERSION: &str = match option_env!("AZORK_RELEASE_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};

pub mod backend;
pub mod parser;
pub mod update;
pub mod world;
