//! tests/world_tests.rs
//!
//! Contract tests for the world model (`azork::world`).
//!
//! Builds small, purpose-made worlds through the public API and asserts the
//! behaviour of every player intent: look/examine, navigation, take/drop,
//! lock/unlock, resize, monitor, scoring, and the Grue danger mechanic —
//! including edge cases (prefix matching, inventory targets, missing targets,
//! score-rank boundaries, and darkness recovery).

use azork::parser::Direction;
use azork::world::{GrueOutcome, Resource, Room, World};

/// A public resource with an unencrypted, unlocked, pricey profile — i.e. all
/// four resource hazards active.
fn hazardous(name: &str) -> Resource {
    let mut r = Resource::new(
        name,
        "Microsoft.Storage/storageAccounts",
        "A hazardous store.",
    );
    r.public = true;
    r.encrypted = false;
    r.locked = false;
    r.monthly_cost = 800;
    r
}

/// Two-room world: a lit `prod-rg` (start) with one hazardous `storage`, and a
/// dark `dark-rg` to the north.
fn two_room_world() -> World {
    let lit = Room::new("prod-rg", "A well-lit aisle.", "eastus", true)
        .with_exit(Direction::North, "dark-rg")
        .with_resource(hazardous("storage"));
    let dark = Room::new("dark-rg", "?", "westus", false).with_exit(Direction::South, "prod-rg");
    World::new(vec![lit, dark], "prod-rg", "sub-test-001").expect("valid test room graph")
}

// --- look / examine -------------------------------------------------------

#[test]
fn look_shows_room_name_region_resources_and_sorted_exits() {
    let out = two_room_world().look();
    assert!(out.contains("prod-rg"));
    assert!(out.contains("eastus"));
    assert!(out.contains("storage"));
    assert!(out.contains("Exits: north"));
}

#[test]
fn look_in_dark_room_warns_of_grue_and_hides_contents() {
    let mut w = two_room_world();
    w.go(Direction::North).unwrap();
    let out = w.look();
    assert!(out.contains("pitch black"));
    assert!(out.contains("Grue"));
    // Content is not enumerated in the dark.
    assert!(!out.contains("You see:"));
}

#[test]
fn examine_reports_full_hazard_status() {
    let out = two_room_world().examine("storage");
    assert!(out.contains("PUBLIC"));
    assert!(out.contains("UNENCRYPTED"));
    assert!(out.contains("unlocked"));
    assert!(out.contains("Grue"));
}

#[test]
fn examine_supports_case_insensitive_prefix_matching() {
    let w = two_room_world();
    // "STOR" is a case-insensitive prefix of "storage".
    let out = w.examine("STOR");
    assert!(out.contains("storage"));
}

#[test]
fn examine_missing_target_is_reported() {
    assert!(two_room_world().examine("dragon").contains("don't see"));
}

#[test]
fn examine_in_the_dark_refuses() {
    let mut w = two_room_world();
    w.go(Direction::North).unwrap();
    assert!(w.examine("anything").to_lowercase().contains("dark"));
}

#[test]
fn examine_carried_item_is_marked_carried() {
    let mut w = two_room_world();
    w.take("storage");
    let out = w.examine("storage");
    assert!(out.contains("carried"));
}

// --- navigation -----------------------------------------------------------

#[test]
fn go_through_a_valid_exit_moves_and_counts_a_move() {
    let mut w = two_room_world();
    assert_eq!(w.moves(), 0);
    w.go(Direction::North).unwrap();
    assert_eq!(w.current_room().name, "dark-rg");
    assert_eq!(w.moves(), 1);
}

#[test]
fn go_through_a_missing_exit_errors_and_does_not_move() {
    let mut w = two_room_world();
    let before = w.current_room().name.clone();
    let err = w.go(Direction::East).unwrap_err();
    assert!(err.contains("can't go east"));
    assert_eq!(w.current_room().name, before);
}

// --- take / drop ----------------------------------------------------------

#[test]
fn take_moves_a_resource_from_room_to_inventory() {
    let mut w = two_room_world();
    let msg = w.take("storage");
    assert!(msg.contains("acquire"));
    assert!(w.inventory().contains("storage"));
    // The room no longer offers it.
    assert!(w.take("storage").contains("no 'storage'"));
}

#[test]
fn take_in_the_dark_grasps_nothing() {
    let mut w = two_room_world();
    w.go(Direction::North).unwrap();
    assert!(w.take("storage").to_lowercase().contains("dark"));
}

#[test]
fn take_missing_target_is_reported() {
    assert!(two_room_world().take("ghost").contains("no 'ghost'"));
}

#[test]
fn drop_deletes_a_carried_resource() {
    let mut w = two_room_world();
    w.take("storage");
    let msg = w.drop_item("storage");
    assert!(msg.contains("delete") || msg.contains("dissolve"));
    assert!(w.inventory().contains("nothing"));
}

#[test]
fn drop_deletes_a_resource_directly_from_the_room() {
    let mut w = two_room_world();
    let msg = w.drop_item("storage");
    assert!(msg.contains("delete") || msg.contains("dissolve"));
    assert!(two_room_world().examine("storage").contains("storage")); // sanity: fresh world still has it
    assert!(w.examine("storage").contains("don't see"));
}

#[test]
fn drop_missing_target_is_reported() {
    assert!(two_room_world().drop_item("ghost").contains("no 'ghost'"));
}

#[test]
fn locked_resource_refuses_deletion_until_unlocked() {
    let mut w = two_room_world();
    w.lock("storage");
    assert!(w.drop_item("storage").contains("locked"));
    // It survives.
    assert!(w.examine("storage").contains("storage"));
    // Unlock, then it can be deleted.
    assert!(w.unlock("storage").to_lowercase().contains("lift"));
    assert!(w.drop_item("storage").to_lowercase().contains("dissolve"));
}

// --- lock / unlock --------------------------------------------------------

#[test]
fn lock_wards_a_room_resource_and_clears_all_resource_hazards() {
    let mut w = two_room_world();
    // storage starts with public + unencrypted + unlocked = 3 resource hazards
    // (cost is 800 => a 4th). lock() clears public/encrypted/locked (3 of 4).
    let before = w.total_hazards();
    w.lock("storage");
    let after = w.total_hazards();
    assert!(
        after < before,
        "locking must reduce hazards ({before} -> {after})"
    );
    let status = w.examine("storage");
    assert!(status.contains("private"));
    assert!(status.contains("encrypted"));
    assert!(status.contains("locked"));
}

#[test]
fn lock_and_unlock_work_on_carried_resources() {
    let mut w = two_room_world();
    w.take("storage");
    assert!(w.lock("storage").to_lowercase().contains("secure"));
    assert!(w.examine("storage").contains("locked"));
    assert!(w.unlock("storage").to_lowercase().contains("lock"));
    assert!(w.examine("storage").contains("unlocked"));
}

#[test]
fn unlock_on_an_unlocked_resource_is_a_noop_message() {
    let mut w = two_room_world();
    assert!(w.unlock("storage").contains("not locked"));
}

#[test]
fn lock_and_unlock_on_missing_targets_are_reported() {
    let mut w = two_room_world();
    assert!(w.lock("ghost").contains("no 'ghost'"));
    assert!(w.unlock("ghost").contains("no 'ghost'"));
}

// --- resize ---------------------------------------------------------------

#[test]
fn resize_halves_cost_and_clears_the_overrun_hazard() {
    let mut w = two_room_world(); // storage costs 800/mo (overrun)
    let before = w.total_hazards();
    let msg = w.resize("storage");
    assert!(msg.contains("right-size"));
    assert!(msg.contains("400")); // 800 / 2
    assert!(
        w.total_hazards() < before,
        "right-sizing must clear the cost-overrun hazard"
    );
}

#[test]
fn resize_a_zero_cost_resource_has_nothing_to_do() {
    let mut w = two_room_world();
    // A free portal resource with no cost.
    let free = Resource::new("portal", "Microsoft.Portal/dashboards", "free dashboard");
    let room = Room::new("free-rg", "Free zone.", "eastus", true).with_resource(free);
    let mut w2 = World::new(vec![room], "free-rg", "sub").expect("valid test room graph");
    assert!(w2.resize("portal").contains("nothing to right-size"));
    // Unrelated: resize on missing target reported.
    assert!(w.resize("ghost").contains("no 'ghost'"));
}

#[test]
fn resize_works_on_carried_resources() {
    let mut w = two_room_world();
    w.take("storage");
    let msg = w.resize("storage");
    assert!(msg.contains("right-size"));
}

// --- monitor & scoring ----------------------------------------------------

#[test]
fn monitor_lights_a_dark_room_and_banishes_the_grue() {
    let mut w = two_room_world();
    w.go(Direction::North).unwrap();
    assert!(w.current_room().is_dark());
    let msg = w.monitor();
    assert!(msg.to_lowercase().contains("light") || msg.to_lowercase().contains("monitor"));
    assert!(!w.current_room().is_dark());
    assert_eq!(w.grue_check(), GrueOutcome::Safe);
}

#[test]
fn monitor_on_an_already_lit_room_is_a_noop_message() {
    let mut w = two_room_world();
    assert!(w.monitor().to_lowercase().contains("already"));
}

#[test]
fn total_hazards_counts_rooms_darkness_and_inventory() {
    let mut w = two_room_world();
    // storage: public + unencrypted + unlocked + cost>=500 = 4.
    // dark-rg: darkness = 1. Total = 5.
    assert_eq!(w.total_hazards(), 5);
    // Carrying the hazardous storage doesn't change the count (still 4 from it).
    w.take("storage");
    assert_eq!(w.total_hazards(), 5);
}

#[test]
fn score_reflects_hazard_reduction() {
    let mut w = two_room_world();
    let dirty = w.score();
    w.lock("storage");
    w.resize("storage");
    w.go(Direction::North).unwrap();
    w.monitor();
    let clean = w.score();
    assert_ne!(dirty, clean);
    assert_eq!(w.total_hazards(), 0);
    assert!(clean.contains("100/100"));
    assert!(clean.contains("Cloud Guardian"));
}

#[test]
fn score_is_floored_at_zero_for_a_disaster_estate() {
    // Build a room stuffed with hazards so the raw score would go negative.
    let mut room = Room::new("chaos-rg", "Total chaos.", "eastus", false); // dark
    for i in 0..10 {
        room = room.with_resource(hazardous(&format!("bad{i}")));
    }
    let w = World::new(vec![room], "chaos-rg", "sub").expect("valid test room graph");
    let score = w.score();
    // 10 resources * 4 hazards + 1 darkness = 41 hazards => far below 0, floored.
    assert!(score.contains("0/100"));
    assert!(score.contains("Grue Chow"));
}

#[test]
fn score_ranks_span_the_expected_bands() {
    // Helper: build a world with exactly `n` hazards via n free-but-unlocked
    // resources in a lit room (each contributes exactly 1 hazard: unlocked).
    fn world_with_hazards(n: u32) -> World {
        let mut room = Room::new("rg", "A room.", "eastus", true);
        for i in 0..n {
            // Encrypted, private, free => only the "unlocked" hazard counts.
            let r = Resource::new(&format!("r{i}"), "kind", "desc");
            room = room.with_resource(r);
        }
        World::new(vec![room], "rg", "sub").expect("valid test room graph")
    }

    // 0 hazards => 100 => Cloud Guardian.
    assert!(world_with_hazards(0).score().contains("Cloud Guardian"));
    // 4 hazards => 80 => Diligent Steward.
    assert!(world_with_hazards(4).score().contains("Diligent Steward"));
    // 8 hazards => 60 => Apprentice Admin.
    assert!(world_with_hazards(8).score().contains("Apprentice Admin"));
    // 12 hazards => 40 => Reckless Tinkerer.
    assert!(world_with_hazards(12).score().contains("Reckless Tinkerer"));
    // 16 hazards => 20 => Grue Chow.
    assert!(world_with_hazards(16).score().contains("Grue Chow"));
}

// --- Grue danger mechanic -------------------------------------------------

#[test]
fn grue_stays_safe_in_a_lit_room() {
    let mut w = two_room_world();
    assert_eq!(w.grue_check(), GrueOutcome::Safe);
    assert!(!w.game_over);
}

#[test]
fn first_turn_in_the_dark_is_only_a_warning() {
    let mut w = two_room_world();
    w.seed_rng(1);
    w.go(Direction::North).unwrap();
    assert_eq!(w.grue_check(), GrueOutcome::Lurking);
    assert!(!w.game_over);
}

#[test]
fn lingering_in_the_dark_eventually_gets_you_devoured() {
    let mut w = two_room_world();
    w.seed_rng(1);
    w.go(Direction::North).unwrap();
    let mut devoured = false;
    for _ in 0..30 {
        if w.grue_check() == GrueOutcome::Devoured {
            devoured = true;
            break;
        }
    }
    assert!(devoured, "escalating darkness must eventually kill");
    assert!(w.game_over);
}

#[test]
fn returning_to_the_light_resets_the_darkness_streak() {
    let mut w = two_room_world();
    w.seed_rng(42);
    // Enter the dark and take one warning tick.
    w.go(Direction::North).unwrap();
    assert_eq!(w.grue_check(), GrueOutcome::Lurking);
    // Retreat to the lit room; the streak resets to Safe.
    w.go(Direction::South).unwrap();
    assert_eq!(w.grue_check(), GrueOutcome::Safe);
    // Re-entering the dark is once again only a warning (streak restarted).
    w.go(Direction::North).unwrap();
    assert_eq!(w.grue_check(), GrueOutcome::Lurking);
    assert!(!w.game_over);
}

// --- achievements -----------------------------------------------------------

/// A single clean, well-governed resource: encrypted, private, locked, free.
fn clean_resource(name: &str) -> Resource {
    let mut r = Resource::new(name, "Microsoft.Storage/storageAccounts", "A tidy store.");
    r.locked = true;
    r.public = false;
    r.encrypted = true;
    r.monthly_cost = 0;
    r
}

fn world_with(resource: Resource) -> World {
    let room = Room::new("rg", "A room.", "eastus", true).with_resource(resource);
    World::new(vec![room], "rg", "sub").expect("valid test room graph")
}

#[test]
fn clean_world_earns_all_four_badges() {
    let w = world_with(clean_resource("storage"));
    let badges = w.achievements();
    assert_eq!(badges.len(), 4);
    for b in &badges {
        assert!(b.earned, "{} should be earned on a clean world", b.name);
        assert!(b.blocker.is_none());
    }
    let names: Vec<&str> = badges.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["Fort Knox", "No Open Doors", "Warded", "Under Budget"]
    );
}

#[test]
fn unencrypted_resource_locks_only_fort_knox() {
    let mut r = clean_resource("storage");
    r.encrypted = false;
    let w = world_with(r);
    let badges = w.achievements();
    for b in &badges {
        if b.name == "Fort Knox" {
            assert!(!b.earned);
            assert!(b.blocker.as_deref().unwrap().contains("unencrypted"));
        } else {
            assert!(b.earned, "{} must stay earned", b.name);
        }
    }
}

#[test]
fn public_resource_locks_only_no_open_doors() {
    let mut r = clean_resource("storage");
    r.public = true;
    let w = world_with(r);
    let badges = w.achievements();
    for b in &badges {
        if b.name == "No Open Doors" {
            assert!(!b.earned);
            assert!(b.blocker.as_deref().unwrap().contains("public"));
        } else {
            assert!(b.earned, "{} must stay earned", b.name);
        }
    }
}

#[test]
fn unlocked_resource_locks_only_warded() {
    let mut r = clean_resource("storage");
    r.locked = false;
    let w = world_with(r);
    let badges = w.achievements();
    for b in &badges {
        if b.name == "Warded" {
            assert!(!b.earned);
            assert!(b.blocker.as_deref().unwrap().contains("unlocked"));
        } else {
            assert!(b.earned, "{} must stay earned", b.name);
        }
    }
}

#[test]
fn cost_overrun_resource_locks_only_under_budget() {
    let mut r = clean_resource("storage");
    r.monthly_cost = 800;
    let w = world_with(r);
    let badges = w.achievements();
    for b in &badges {
        if b.name == "Under Budget" {
            assert!(!b.earned);
            assert!(b.blocker.as_deref().unwrap().contains("over budget"));
        } else {
            assert!(b.earned, "{} must stay earned", b.name);
        }
    }
}

#[test]
fn hazardous_world_fails_all_four_badges_with_named_blockers() {
    let w = two_room_world(); // "storage" resource has all 4 hazards active
    let badges = w.achievements();
    assert_eq!(badges.len(), 4);
    for b in &badges {
        assert!(!b.earned, "{} should be locked", b.name);
        assert!(b.blocker.as_deref().unwrap().contains("storage"));
    }
}

#[test]
fn achievements_are_deterministic_across_calls() {
    let w = two_room_world();
    assert_eq!(w.achievements(), w.achievements());
}

#[test]
fn resource_hazard_helpers_agree_with_flags() {
    let clean = Resource::new("ok", "kind", "desc"); // encrypted, private, free, but unlocked
    assert_eq!(clean.hazards(), 1); // only "unlocked"
    assert!(clean.hazard_report().contains("Grue"));

    let mut perfect = Resource::new("ok", "kind", "desc");
    perfect.locked = true;
    assert_eq!(perfect.hazards(), 0);
    assert!(perfect.hazard_report().contains("well-governed"));

    let bad = hazardous("bad");
    assert_eq!(bad.hazards(), 4);
}
