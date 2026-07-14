//! Backend abstraction: where the dungeon's map comes from.
//!
//! A [`Backend`] is responsible for building the initial [`World`]. The default
//! [`mock::MockBackend`] uses hardcoded, synthetic Azure-like data so the game
//! runs with zero credentials. The optional [`az::AzBackend`] shells out to the
//! real `az` CLI to map your actual subscription.

use crate::world::World;

pub mod az;
pub mod mock;

/// Something that can construct the initial game world.
pub trait Backend {
    /// Human-readable backend name, shown in the banner.
    fn name(&self) -> &str;

    /// Build the initial world, or return an error string on failure.
    fn build_world(&self) -> Result<World, String>;
}

/// Select a backend by identifier. Falls back to mock for anything unknown.
///
/// Recognised ids: `mock` (default), `az` / `real`.
pub fn select(id: &str) -> Box<dyn Backend> {
    match id.to_lowercase().as_str() {
        "az" | "real" | "azure" => Box::new(az::AzBackend::new()),
        _ => Box::new(mock::MockBackend::new()),
    }
}
