//! tests/parser_tests.rs
//!
//! Contract tests for the command parser (`azork::parser`).
//!
//! These specify the parser's behaviour as an external consumer sees it: every
//! supported verb, its aliases, filler-word stripping, argument capture, and the
//! total-function guarantee that *any* input yields a `Command` and never panics.

use azork::parser::{parse, Command, Direction};

// --- verb + alias coverage ------------------------------------------------

#[test]
fn look_and_its_aliases_parse() {
    assert_eq!(parse("look"), Command::Look);
    assert_eq!(parse("l"), Command::Look);
    // Case-insensitive.
    assert_eq!(parse("LOOK"), Command::Look);
    assert_eq!(parse("Look"), Command::Look);
}

#[test]
fn examine_accepts_all_aliases() {
    for verb in ["examine", "x", "inspect", "show"] {
        assert_eq!(
            parse(&format!("{verb} storage")),
            Command::Examine("storage".to_string()),
            "verb `{verb}` should map to Examine"
        );
    }
}

#[test]
fn take_accepts_all_aliases() {
    for verb in ["take", "get", "grab", "acquire"] {
        assert_eq!(
            parse(&format!("{verb} keyvault")),
            Command::Take("keyvault".to_string()),
            "verb `{verb}` should map to Take"
        );
    }
}

#[test]
fn drop_accepts_all_aliases() {
    for verb in ["drop", "delete", "release", "rm"] {
        assert_eq!(
            parse(&format!("{verb} database")),
            Command::Drop("database".to_string()),
            "verb `{verb}` should map to Drop"
        );
    }
}

#[test]
fn lock_accepts_all_aliases() {
    for verb in ["lock", "secure"] {
        assert_eq!(
            parse(&format!("{verb} storage")),
            Command::Lock("storage".to_string()),
        );
    }
}

#[test]
fn unlock_accepts_all_aliases() {
    for verb in ["unlock", "unward", "unsecure"] {
        assert_eq!(
            parse(&format!("{verb} keyvault")),
            Command::Unlock("keyvault".to_string()),
        );
    }
}

#[test]
fn resize_accepts_all_aliases() {
    for verb in ["resize", "rightsize", "right-size", "scale", "downsize"] {
        assert_eq!(
            parse(&format!("{verb} sqlserver")),
            Command::Resize("sqlserver".to_string()),
            "verb `{verb}` should map to Resize"
        );
    }
}

#[test]
fn monitor_accepts_all_aliases() {
    assert_eq!(parse("monitor"), Command::Monitor);
    assert_eq!(parse("light"), Command::Monitor);
}

#[test]
fn inventory_accepts_all_aliases() {
    for verb in ["inventory", "i", "inv"] {
        assert_eq!(parse(verb), Command::Inventory, "verb `{verb}`");
    }
}

#[test]
fn help_and_quit_aliases() {
    for verb in ["help", "?", "h"] {
        assert_eq!(parse(verb), Command::Help, "verb `{verb}`");
    }
    for verb in ["quit", "q", "exit"] {
        assert_eq!(parse(verb), Command::Quit, "verb `{verb}`");
    }
    for verb in ["version", "ver"] {
        assert_eq!(parse(verb), Command::Version, "verb `{verb}`");
    }
}

// --- directions -----------------------------------------------------------

#[test]
fn bare_directions_are_shorthand_for_go() {
    let cases = [
        ("north", Direction::North),
        ("n", Direction::North),
        ("south", Direction::South),
        ("s", Direction::South),
        ("east", Direction::East),
        ("e", Direction::East),
        ("west", Direction::West),
        ("w", Direction::West),
        ("up", Direction::Up),
        ("u", Direction::Up),
        ("down", Direction::Down),
        ("d", Direction::Down),
    ];
    for (word, dir) in cases {
        assert_eq!(parse(word), Command::Go(dir), "bare direction `{word}`");
    }
}

#[test]
fn go_with_explicit_direction_and_movement_verbs() {
    for verb in ["go", "move", "walk"] {
        assert_eq!(
            parse(&format!("{verb} south")),
            Command::Go(Direction::South),
            "movement verb `{verb}`"
        );
    }
}

#[test]
fn go_without_a_valid_direction_is_unknown() {
    assert!(matches!(parse("go"), Command::Unknown(_)));
    assert!(matches!(parse("go sideways"), Command::Unknown(_)));
    assert!(matches!(parse("walk nowhere"), Command::Unknown(_)));
}

#[test]
fn direction_from_token_and_name_round_trip() {
    for dir in [
        Direction::North,
        Direction::South,
        Direction::East,
        Direction::West,
        Direction::Up,
        Direction::Down,
    ] {
        assert_eq!(Direction::from_token(dir.name()), Some(dir));
    }
    assert_eq!(Direction::from_token("nowhere"), None);
}

// --- filler stripping & multi-word args -----------------------------------

#[test]
fn filler_words_are_stripped() {
    // "the", "a", "an", "at", "to", "into", "on", "my" are all filler.
    assert_eq!(
        parse("examine the storage account"),
        Command::Examine("storage account".to_string())
    );
    assert_eq!(parse("take the vm"), Command::Take("vm".to_string()));
    assert_eq!(
        parse("drop into my database"),
        Command::Drop("database".to_string())
    );
    assert_eq!(parse("go to the north"), Command::Go(Direction::North));
}

#[test]
fn multiword_targets_are_preserved_in_order() {
    assert_eq!(
        parse("examine managed identity principal"),
        Command::Examine("managed identity principal".to_string())
    );
}

// --- cast / deploy --------------------------------------------------------

#[test]
fn cast_deploy_with_and_without_template() {
    assert_eq!(parse("cast deploy"), Command::Cast("deploy".to_string()));
    assert_eq!(
        parse("cast deploy webapp.bicep"),
        Command::Cast("deploy webapp.bicep".to_string())
    );
}

#[test]
fn deploy_is_a_convenience_alias_for_cast_deploy() {
    assert_eq!(parse("deploy"), Command::Cast("deploy".to_string()));
    assert_eq!(
        parse("deploy main.bicep"),
        Command::Cast("deploy main.bicep".to_string())
    );
}

#[test]
fn cast_without_a_spell_is_unknown() {
    assert!(matches!(parse("cast"), Command::Unknown(_)));
}

// --- empty & unknown / total-function guarantee ---------------------------

#[test]
fn empty_and_whitespace_only_input_is_empty() {
    assert_eq!(parse(""), Command::Empty);
    assert_eq!(parse("   "), Command::Empty);
    assert_eq!(parse("\t\n"), Command::Empty);
    // A line of only filler words collapses to nothing meaningful.
    assert_eq!(parse("the a an"), Command::Empty);
}

#[test]
fn verbs_that_require_an_argument_are_unknown_without_one() {
    for verb in [
        "examine", "take", "drop", "lock", "unlock", "resize", "cast",
    ] {
        assert!(
            matches!(parse(verb), Command::Unknown(_)),
            "bare `{verb}` should be Unknown"
        );
    }
}

#[test]
fn unknown_verbs_preserve_original_text() {
    match parse("frobnicate the vm") {
        Command::Unknown(raw) => assert_eq!(raw, "frobnicate the vm"),
        other => panic!("expected Unknown, got {other:?}"),
    }
}

#[test]
fn parser_never_panics_on_hostile_input() {
    // Contract: parse is a total function over arbitrary strings.
    let big = "a".repeat(10_000);
    let hostile = [
        "!!!",
        "🔥🔥🔥",
        "; rm -rf / --no-preserve-root",
        "$(whoami)",
        "\u{0}\u{0}\u{0}",
        big.as_str(),
    ];
    for input in hostile {
        let _ = parse(input); // must return, not panic
    }
}
