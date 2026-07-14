//! tests/backend_tests.rs
//!
//! Contract tests for the backend abstraction (`azork::backend`).
//!
//! Verifies backend selection, the mock estate's invariants (offline, starts
//! lit, reachable dark room, hazards to fix, fully winnable), and that the `az`
//! backend can be constructed without ever touching credentials or the network.

use azork::backend::{self, mock::MockBackend, Backend};
use azork::parser::Direction;

// --- selection ------------------------------------------------------------

#[test]
fn select_defaults_to_mock_for_unknown_and_empty_ids() {
    for id in ["", "mock", "offline", "nonsense", "MOCK"] {
        let b = backend::select(id);
        assert!(
            b.name().contains("mock"),
            "id `{id}` should select the mock backend, got `{}`",
            b.name()
        );
    }
}

#[test]
fn select_returns_az_backend_for_real_azure_ids() {
    for id in ["az", "real", "azure", "AZ", "Azure"] {
        let b = backend::select(id);
        assert!(
            b.name().contains("az") || b.name().to_lowercase().contains("azure"),
            "id `{id}` should select the az backend, got `{}`",
            b.name()
        );
    }
}

// --- mock estate invariants ----------------------------------------------

#[test]
fn mock_backend_builds_offline_and_starts_in_a_lit_room() {
    let w = MockBackend::new().build_world().unwrap();
    assert_eq!(w.current_room().name, "landing-rg");
    assert!(!w.current_room().is_dark());
    assert!(w.subscription.contains("mock"));
}

#[test]
fn mock_backend_default_matches_new() {
    #[allow(clippy::default_constructed_unit_structs)]
    let a = MockBackend::default().build_world().unwrap();
    let b = MockBackend::new().build_world().unwrap();
    assert_eq!(a.current_room().name, b.current_room().name);
}

#[test]
fn mock_estate_contains_at_least_one_reachable_dark_room() {
    let mut w = MockBackend::new().build_world().unwrap();
    w.go(Direction::North).unwrap(); // web-rg
    w.go(Direction::North).unwrap(); // unmon-rg
    assert_eq!(w.current_room().name, "unmon-rg");
    assert!(w.current_room().is_dark());
}

#[test]
fn mock_estate_starts_with_hazards_to_fix() {
    let w = MockBackend::new().build_world().unwrap();
    assert!(w.total_hazards() > 0);
    // And is therefore not a perfect score at the start.
    assert!(!w.score().contains("100/100"));
}

#[test]
fn mock_estate_is_fully_winnable_to_a_perfect_score() {
    let mut w = MockBackend::new().build_world().unwrap();

    // landing-rg
    w.lock("portal");

    // web-rg
    w.go(Direction::North).unwrap();
    w.lock("appservice");
    w.lock("webstore");

    // unmon-rg (dark): light, then harden.
    w.go(Direction::North).unwrap();
    w.monitor();
    w.lock("orphan-vm");

    // data-rg
    w.go(Direction::South).unwrap();
    w.go(Direction::South).unwrap();
    w.go(Direction::East).unwrap();
    w.lock("keyvault");
    w.lock("sqlserver");
    w.resize("sqlserver");

    // identity-rg
    w.go(Direction::West).unwrap();
    w.go(Direction::Down).unwrap();
    w.lock("managed-identity");

    assert_eq!(
        w.total_hazards(),
        0,
        "a hardened estate must have zero hazards"
    );
    assert!(w.score().contains("100/100"));
    assert!(w.score().contains("Cloud Guardian"));
}

// --- az backend safety ----------------------------------------------------

#[test]
fn az_backend_constructs_without_credentials_or_network() {
    // Constructing and naming the backend must be side-effect free. We do NOT
    // call build_world(), which would shell out to `az`.
    let b = backend::select("az");
    assert!(b.name().to_lowercase().contains("az") || b.name().to_lowercase().contains("azure"));
}
