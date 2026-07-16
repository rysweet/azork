//! tests/integration_tests.rs
//!
//! End-to-end workflow tests: parse raw player input and dispatch the resulting
//! commands against a live world, exactly as the REPL does. These prove the
//! parser and world model compose correctly across multi-turn sessions.

use azork::backend::{mock::MockBackend, Backend};
use azork::parser::{parse, Command};
use azork::world::{GrueOutcome, World};

/// Minimal REPL-style dispatcher mirroring `main::handle` for the non-interactive
/// verbs (take/drop confirmation is elided; those are covered separately).
fn dispatch(world: &mut World, line: &str) -> String {
    match parse(line) {
        Command::Empty => String::new(),
        Command::Look => world.look(),
        Command::Examine(t) => world.examine(&t),
        Command::Go(dir) => match world.go(dir) {
            Ok(desc) => desc,
            Err(e) => e,
        },
        Command::Take(t) => world.take(&t),
        Command::Drop(t) => world.drop_item(&t),
        Command::Lock(t) => world.lock(&t),
        Command::Unlock(t) => world.unlock(&t),
        Command::Resize(t) => world.resize(&t),
        Command::Monitor => world.monitor(),
        Command::Inventory => world.inventory(),
        Command::Score => world.score(),
        Command::Quest => "quest".to_string(),
        Command::Cast(_) => "cast (mock)".to_string(),
        Command::Learn(g) => format!("learn: {g}"),
        Command::Capabilities => "capabilities".to_string(),
        Command::Friction(n) => format!("friction: {n}"),
        Command::Recall(q) => format!("recall: {q}"),
        Command::Memory => "memory".to_string(),
        Command::Help => "help".to_string(),
        Command::Version => "version".to_string(),
        Command::Quit => "quit".to_string(),
        Command::Unknown(raw) => format!("unknown: {raw}"),
    }
}

fn mock_world() -> World {
    MockBackend::new().build_world().unwrap()
}

#[test]
fn a_typed_session_navigates_and_hardens_a_resource() {
    let mut w = mock_world();

    // Look around the entrance.
    assert!(dispatch(&mut w, "look").contains("landing-rg"));

    // Walk north into the public web tier and inspect the open blob store.
    assert!(dispatch(&mut w, "go north").contains("web-rg"));
    let examined = dispatch(&mut w, "examine webstore");
    assert!(examined.contains("PUBLIC"));
    assert!(examined.contains("UNENCRYPTED"));

    // Harden it with a lock; re-examination shows it is now private/encrypted.
    assert!(dispatch(&mut w, "lock webstore")
        .to_lowercase()
        .contains("ward"));
    let after = dispatch(&mut w, "x webstore");
    assert!(after.contains("private"));
    assert!(after.contains("encrypted"));
    assert!(after.contains("locked"));
}

#[test]
fn bare_directions_and_verb_aliases_drive_the_world() {
    let mut w = mock_world();
    // Bare "n" == "go north"; "i" == inventory; "l" == look.
    assert!(dispatch(&mut w, "n").contains("web-rg"));
    assert!(dispatch(&mut w, "i").contains("nothing"));
    assert!(dispatch(&mut w, "l").contains("web-rg"));
}

#[test]
fn take_then_drop_round_trip_through_typed_commands() {
    let mut w = mock_world();
    dispatch(&mut w, "east"); // data-rg
    assert!(dispatch(&mut w, "take keyvault").contains("acquire"));
    assert!(dispatch(&mut w, "inventory").contains("keyvault"));
    // keyvault is unlocked, so it can be deleted.
    assert!(dispatch(&mut w, "drop keyvault")
        .to_lowercase()
        .contains("dissolve"));
    assert!(dispatch(&mut w, "inventory").contains("nothing"));
}

#[test]
fn walking_into_the_dark_and_lighting_it_defeats_the_grue() {
    let mut w = mock_world();
    w.seed_rng(7);

    // Navigate to the unmonitored (dark) room.
    dispatch(&mut w, "north"); // web-rg
    dispatch(&mut w, "north"); // unmon-rg (dark)
    assert!(w.current_room().is_dark());

    // First darkness tick is a warning...
    assert_eq!(w.grue_check(), GrueOutcome::Lurking);

    // ...light the room via a typed command; the Grue is banished.
    assert!(dispatch(&mut w, "monitor").to_lowercase().contains("light"));
    assert_eq!(w.grue_check(), GrueOutcome::Safe);
    assert!(!w.game_over);
}

#[test]
fn a_full_hardening_playthrough_reaches_a_perfect_score() {
    let mut w = mock_world();

    let script = [
        "lock portal",
        "north", // web-rg
        "lock appservice",
        "lock webstore",
        "north", // unmon-rg (dark)
        "monitor",
        "lock orphan-vm",
        "south", // web-rg
        "south", // landing-rg
        "east",  // data-rg
        "lock keyvault",
        "lock sqlserver",
        "resize sqlserver",
        "west", // landing-rg
        "down", // identity-rg
        "lock managed-identity",
    ];
    for line in script {
        dispatch(&mut w, line);
    }

    let final_score = dispatch(&mut w, "score");
    assert!(final_score.contains("100/100"), "score was: {final_score}");
    assert!(final_score.contains("Cloud Guardian"));
    assert_eq!(w.total_hazards(), 0);
}

#[test]
fn unknown_and_empty_input_do_not_disturb_the_world() {
    let mut w = mock_world();
    let start = w.current_room().name.clone();
    assert!(dispatch(&mut w, "frobnicate").starts_with("unknown"));
    assert_eq!(dispatch(&mut w, ""), "");
    assert_eq!(w.current_room().name, start);
    assert_eq!(w.moves(), 0);
}
