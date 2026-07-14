//! AzZork library crate.
//!
//! Exposes the reusable pieces of the game — the command [`parser`], the
//! [`world`] model, and the [`backend`] abstraction — so they can be driven by
//! the `azork` binary *and* exercised directly from the integration tests in
//! `tests/`.

pub mod backend;
pub mod parser;
pub mod world;
