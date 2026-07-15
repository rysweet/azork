//! The default offline backend: a hand-authored synthetic Azure estate.
//!
//! No credentials, no network. Just a small, deliberately hazardous cloud for
//! the player to explore and harden.

use super::mock_gen::MockSizeParams;
use super::Backend;
use crate::parser::Direction;
use crate::world::{Resource, Room, World};

/// The synthetic world a [`MockBackend`] builds.
enum MockWorldSpec {
    /// The original, fixed, hand-authored world — unchanged default
    /// behavior for anyone not requesting a sized estate.
    Fixed,
    /// A deterministically-generated, parameterized synthetic estate. See
    /// [`crate::backend::mock_gen`].
    Sized(MockSizeParams),
}

/// Builds a synthetic world: a fixed, hand-authored one by default, or a
/// deterministically-generated, sized one when requested via
/// [`MockBackend::sized`].
pub struct MockBackend {
    spec: MockWorldSpec,
}

impl MockBackend {
    pub fn new() -> MockBackend {
        MockBackend {
            spec: MockWorldSpec::Fixed,
        }
    }

    /// Build a mock backend that generates a sized synthetic estate instead
    /// of the default fixed world. See `docs/DUNGEON-CRAWLER.md#generating-a-sized-mock-tenant`.
    pub fn sized(params: MockSizeParams) -> MockBackend {
        MockBackend {
            spec: MockWorldSpec::Sized(params),
        }
    }
}

impl Default for MockBackend {
    fn default() -> Self {
        MockBackend::new()
    }
}

impl Backend for MockBackend {
    fn name(&self) -> &str {
        "mock (offline)"
    }

    fn build_world(&self) -> Result<World, String> {
        match &self.spec {
            MockWorldSpec::Sized(params) => return super::mock_gen::generate_world(params),
            MockWorldSpec::Fixed => {}
        }

        // --- landing-rg (start): lit, the safe entrance ---
        let landing = Room::new(
            "landing-rg",
            "The West Landing Zone. Cables snake overhead and a subscription \
             portal hums softly. This resource group is monitored and safe.",
            "eastus",
            true,
        )
        .with_exit(Direction::North, "web-rg")
        .with_exit(Direction::East, "data-rg")
        .with_exit(Direction::Down, "identity-rg")
        .with_resource(Resource::new(
            "portal",
            "Microsoft.Portal/dashboards",
            "A glowing management dashboard etched with glyphs of telemetry.",
        ));

        // --- web-rg: a public web tier with an exposed, unencrypted store ---
        let mut app = Resource::new(
            "appservice",
            "Microsoft.Web/sites",
            "A restless App Service, straining against its plan.",
        );
        app.public = true;
        app.monthly_cost = 120;

        let mut blob = Resource::new(
            "webstore",
            "Microsoft.Storage/storageAccounts",
            "A storage account with its container door flung wide open.",
        );
        blob.public = true;
        blob.encrypted = false;
        blob.monthly_cost = 60;

        let web = Room::new(
            "web-rg",
            "The Public Web Tier. Wind howls through open ports. Something here \
             is exposed to the whole internet.",
            "eastus",
            true,
        )
        .with_exit(Direction::South, "landing-rg")
        .with_exit(Direction::North, "unmon-rg")
        .with_resource(app)
        .with_resource(blob);

        // --- data-rg: expensive database creature ---
        let mut db = Resource::new(
            "sqlserver",
            "Microsoft.Sql/servers",
            "A hulking SQL server, scales slick with transaction logs.",
        );
        db.monthly_cost = 800; // cost-overrun Grue bait
        db.encrypted = true;

        let mut kv = Resource::new(
            "keyvault",
            "Microsoft.KeyVault/vaults",
            "An iron key vault, humming with secrets.",
        );
        kv.locked = false;

        let data = Room::new(
            "data-rg",
            "The Data Vaults. Cold air, rows of disks, and the low growl of an \
             overpriced database.",
            "westus2",
            true,
        )
        .with_exit(Direction::West, "landing-rg")
        .with_resource(db)
        .with_resource(kv);

        // --- identity-rg: RBAC gate room ---
        let identity = Room::new(
            "identity-rg",
            "The Hall of Identity. Managed identities drift like wisps and RBAC \
             wards bar the deeper doors.",
            "eastus",
            true,
        )
        .with_exit(Direction::Up, "landing-rg")
        .with_resource(Resource::new(
            "managed-identity",
            "Microsoft.ManagedIdentity/userAssignedIdentities",
            "A spectral service principal, keeper of least privilege.",
        ));

        // --- unmon-rg: the dark room; a Grue lurks ---
        let mut orphan = Resource::new(
            "orphan-vm",
            "Microsoft.Compute/virtualMachines",
            "An abandoned VM, fans still spinning in the dark.",
        );
        orphan.public = true;
        orphan.encrypted = false;
        orphan.monthly_cost = 300;

        let unmon = Room::new(
            "unmon-rg",
            "?",
            "centralus",
            false, // unmonitored => dark => Grue
        )
        .with_exit(Direction::South, "web-rg")
        .with_resource(orphan);

        let rooms = vec![landing, web, data, identity, unmon];
        World::new(rooms, "landing-rg", "Contoso-Dev (mock)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_world_builds_and_starts_lit() {
        let w = MockBackend::new().build_world().unwrap();
        assert_eq!(w.current_room().name, "landing-rg");
        assert!(!w.current_room().is_dark());
    }

    #[test]
    fn mock_world_has_a_dark_room_reachable() {
        let mut w = MockBackend::new().build_world().unwrap();
        w.go(Direction::North).unwrap(); // web-rg
        w.go(Direction::North).unwrap(); // unmon-rg (dark)
        assert_eq!(w.current_room().name, "unmon-rg");
        assert!(w.current_room().is_dark());
    }

    #[test]
    fn mock_world_has_hazards_to_fix() {
        let w = MockBackend::new().build_world().unwrap();
        assert!(w.total_hazards() > 0);
    }

    #[test]
    fn mock_world_is_fully_winnable_to_perfect_score() {
        let mut w = MockBackend::new().build_world().unwrap();

        // landing-rg: lock the portal.
        w.lock("portal");

        // web-rg: lock the app service and the open storage account.
        w.go(Direction::North).unwrap();
        w.lock("appservice");
        w.lock("webstore");

        // unmon-rg (dark): light it, then lock the orphaned VM.
        w.go(Direction::North).unwrap();
        w.monitor();
        w.lock("orphan-vm");

        // data-rg: lock the key vault, then lock and right-size the pricey SQL server.
        w.go(Direction::South).unwrap(); // back to web-rg
        w.go(Direction::South).unwrap(); // back to landing-rg
        w.go(Direction::East).unwrap(); // data-rg
        w.lock("keyvault");
        w.lock("sqlserver");
        w.resize("sqlserver"); // 800 -> 400, clears the cost-overrun hazard

        // identity-rg: lock the managed identity.
        w.go(Direction::West).unwrap(); // back to landing-rg
        w.go(Direction::Down).unwrap(); // identity-rg
        w.lock("managed-identity");

        assert_eq!(
            w.total_hazards(),
            0,
            "a hardened estate should have zero hazards"
        );
        assert!(w.score().contains("100/100"), "score was: {}", w.score());
        assert!(w.score().contains("Cloud Guardian"));
    }
}
