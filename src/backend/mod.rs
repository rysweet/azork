//! Backend abstraction: where the dungeon's map comes from.
//!
//! A [`Backend`] is responsible for building the initial [`World`]. The default
//! [`mock::MockBackend`] uses hardcoded, synthetic Azure-like data so the game
//! runs with zero credentials. The optional [`az::AzBackend`] shells out to the
//! real `az` CLI to map your actual subscription.

use crate::world::World;

pub mod az;
pub mod mock;
pub mod mock_gen;

/// Something that can construct the initial game world.
pub trait Backend {
    /// Human-readable backend name, shown in the banner.
    fn name(&self) -> &str;

    /// Build the initial world, or return an error string on failure.
    fn build_world(&self) -> Result<World, String>;
}

/// Select a backend by identifier. Falls back to mock for anything unknown.
///
/// Recognised ids: `mock` (default), `az` / `real` / `azure`.
///
/// For the mock backend, if `AZORK_MOCK_SIZE` / `AZORK_MOCK_RGS` /
/// `AZORK_MOCK_RESOURCES_PER_RG` / `AZORK_MOCK_SEED` request a sized
/// synthetic estate (see [`mock_gen::MockSizeParams::from_env`]), a
/// [`mock_gen::SizedMockBackend`] is returned instead of the fixed
/// hand-authored [`mock::MockBackend`]. With none of those env vars set,
/// behaviour is unchanged from before this parameterization existed.
pub fn select(id: &str) -> Box<dyn Backend> {
    match id.to_lowercase().as_str() {
        "az" | "real" | "azure" => Box::new(az::AzBackend::new()),
        _ => match mock_gen::MockSizeParams::from_env() {
            Some(Ok(params)) => Box::new(mock_gen::SizedMockBackend::new(params)),
            Some(Err(e)) => {
                eprintln!(
                    "Warning: invalid mock size configuration ({e}); using the default \
                     offline mock estate."
                );
                Box::new(mock::MockBackend::new())
            }
            None => Box::new(mock::MockBackend::new()),
        },
    }
}

/// Whether `id` names a backend AzZork recognises.
///
/// Used to detect an explicitly-requested but misspelled backend so the caller
/// can warn instead of silently serving the mock estate as though it were live.
pub fn is_recognized(id: &str) -> bool {
    matches!(id.to_lowercase().as_str(), "mock" | "az" | "real" | "azure")
}
