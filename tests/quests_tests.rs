//! tests/quests_tests.rs
//!
//! Contract tests for the `azork::quests` module: gamified, read-only
//! governance objectives evaluated purely against the current in-memory
//! `World` hazard state (no new hazard sources, no persistence, no I/O).

use azork::parser::Direction;
use azork::quests::builtin_quests;
use azork::world::{Resource, Room, World};

/// A resource with every hazard active: public, unencrypted, unlocked.
fn hazardous(name: &str) -> Resource {
    let mut r = Resource::new(name, "Microsoft.Storage/storageAccounts", "Hazardous.");
    r.public = true;
    r.encrypted = false;
    r.locked = false;
    r
}

/// A resource with no hazards: private, encrypted, locked.
fn clean(name: &str) -> Resource {
    let mut r = Resource::new(name, "Microsoft.Compute/virtualMachines", "Well-governed.");
    r.public = false;
    r.encrypted = true;
    r.locked = true;
    r
}

/// Two resources in one room: one fully hazardous, one fully clean.
fn mixed_world() -> World {
    let room = Room::new("prod-rg", "A room.", "eastus", true)
        .with_resource(hazardous("public-storage"))
        .with_resource(clean("locked-vm"));
    World::new(vec![room], "prod-rg", "sub-test").expect("valid test room graph")
}

/// A world with only clean resources across two rooms.
fn all_clean_world() -> World {
    let a = Room::new("rg-a", "A room.", "eastus", true)
        .with_exit(Direction::North, "rg-b")
        .with_resource(clean("vm-a"));
    let b = Room::new("rg-b", "A room.", "westus", true)
        .with_exit(Direction::South, "rg-a")
        .with_resource(clean("vm-b"));
    World::new(vec![a, b], "rg-a", "sub-test").expect("valid test room graph")
}

/// A world with no resources at all (vacuous case).
fn empty_world() -> World {
    let room = Room::new("rg-empty", "An empty room.", "eastus", true);
    World::new(vec![room], "rg-empty", "sub-test").expect("valid test room graph")
}

#[test]
fn builtin_quests_returns_exactly_three_quests() {
    let quests = builtin_quests();
    assert_eq!(quests.len(), 3);
}

#[test]
fn builtin_quests_have_names_and_descriptions() {
    for quest in builtin_quests() {
        assert!(!quest.name.is_empty());
        assert!(!quest.description.is_empty());
    }
}

#[test]
fn secure_the_realm_counts_non_public_resources() {
    let w = mixed_world();
    let quests = builtin_quests();
    let secure = quests
        .iter()
        .find(|q| q.name.contains("Secure the Realm"))
        .expect("Secure the Realm quest must exist");
    let progress = secure.evaluate(&w);
    assert_eq!(progress.total, 2);
    assert_eq!(progress.done, 1); // only locked-vm is non-public
    assert!(!progress.complete);
}

#[test]
fn seal_the_vaults_counts_encrypted_resources() {
    let w = mixed_world();
    let quests = builtin_quests();
    let vaults = quests
        .iter()
        .find(|q| q.name.contains("Seal the Vaults"))
        .expect("Seal the Vaults quest must exist");
    let progress = vaults.evaluate(&w);
    assert_eq!(progress.total, 2);
    assert_eq!(progress.done, 1); // only locked-vm is encrypted
    assert!(!progress.complete);
}

#[test]
fn lift_the_curse_counts_locked_resources() {
    let w = mixed_world();
    let quests = builtin_quests();
    let curse = quests
        .iter()
        .find(|q| q.name.contains("Lift the Curse"))
        .expect("Lift the Curse quest must exist");
    let progress = curse.evaluate(&w);
    assert_eq!(progress.total, 2);
    assert_eq!(progress.done, 1); // only locked-vm is locked
    assert!(!progress.complete);
}

#[test]
fn clean_world_marks_all_quests_complete() {
    let w = all_clean_world();
    for quest in builtin_quests() {
        let progress = quest.evaluate(&w);
        assert_eq!(progress.done, progress.total);
        assert!(progress.complete, "{} should be complete", quest.name);
    }
}

#[test]
fn empty_world_is_vacuously_complete() {
    let w = empty_world();
    for quest in builtin_quests() {
        let progress = quest.evaluate(&w);
        assert_eq!(progress.total, 0);
        assert_eq!(progress.done, 0);
        assert!(
            progress.complete,
            "{} with zero resources should be vacuously complete",
            quest.name
        );
    }
}
