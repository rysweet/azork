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
/// The mock backend additionally honors `AZORK_MOCK_SIZE` / `AZORK_MOCK_RGS`
/// / `AZORK_MOCK_RESOURCES_PER_RG` / `AZORK_MOCK_SEED` (see
/// [`mock_gen::MockSizeParams::from_env`]) to synthesize a larger, sized
/// tenant instead of the default fixed hand-authored world. With none of
/// those set, behavior is unchanged from before this existed.
pub fn select(id: &str) -> Box<dyn Backend> {
    match id.to_lowercase().as_str() {
        "az" | "real" | "azure" => Box::new(az::AzBackend::new()),
        _ => Box::new(mock_backend_from_env()),
    }
}

/// Build the mock backend, honoring any sizing environment variables. Falls
/// back to the default fixed world (with a warning) on invalid input.
fn mock_backend_from_env() -> mock::MockBackend {
    match mock_gen::MockSizeParams::from_env() {
        Some(Ok(params)) => mock::MockBackend::sized(params),
        Some(Err(e)) => {
            eprintln!("Warning: ignoring invalid mock size configuration ({e}); using the default mock estate.");
            mock::MockBackend::new()
        }
        None => mock::MockBackend::new(),
    }
}

/// Whether `id` names a backend AzZork recognises.
///
/// Used to detect an explicitly-requested but misspelled backend so the caller
/// can warn instead of silently serving the mock estate as though it were live.
pub fn is_recognized(id: &str) -> bool {
    matches!(id.to_lowercase().as_str(), "mock" | "az" | "real" | "azure")
}
