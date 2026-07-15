//! Build script for azork.
//!
//! `src/lib.rs` bakes the release version from the optional
//! `AZORK_RELEASE_VERSION` environment variable via `option_env!`. Cargo does
//! not, by itself, treat an `option_env!` read as a rebuild trigger, so a cached
//! object compiled without the variable could be reused for a tagged release —
//! shipping a binary that self-reports `CARGO_PKG_VERSION` instead of the tag,
//! which would undermine the strict-greater-than anti-rollback update check.
//!
//! Emitting `rerun-if-env-changed` makes the crate recompile whenever the
//! release-version input changes, so the baked `VERSION` is always current.
fn main() {
    println!("cargo:rerun-if-env-changed=AZORK_RELEASE_VERSION");
}
