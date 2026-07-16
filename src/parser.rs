//! Command parser for the AzZork text adventure.
//!
//! Turns a raw line of player input into a structured [`Command`]. The parser is
//! deliberately forgiving: for the purposes of *dispatch* (deciding which verb
//! was used, and resolving simple single-target arguments like `examine the
//! webstore` -> `examine webstore`) it lowercases the input and strips filler
//! words ("the", "a", "an", "at", "to", "into", "on", "my"), and understands
//! common Zork-style aliases (e.g. bare directions like `north`, or `l` for
//! `look`). Free-text arguments (currently [`Command::Friction`] and
//! [`Command::Recall`]) are the exception: their captured text is the
//! verbatim substring of the player's original input (original case, filler
//! words intact) with only the leading verb token removed, so notes and
//! queries are never silently corrupted.

/// A compass / topology direction used to navigate the Azure "dungeon".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    North,
    South,
    East,
    West,
    Up,
    Down,
}

impl Direction {
    /// Parse a token into a direction, accepting both full words and the
    /// single-letter abbreviations Zork players expect.
    pub fn from_token(tok: &str) -> Option<Direction> {
        match tok {
            "north" | "n" => Some(Direction::North),
            "south" | "s" => Some(Direction::South),
            "east" | "e" => Some(Direction::East),
            "west" | "w" => Some(Direction::West),
            "up" | "u" => Some(Direction::Up),
            "down" | "d" => Some(Direction::Down),
            _ => None,
        }
    }

    /// Canonical lowercase name of the direction.
    pub fn name(&self) -> &'static str {
        match self {
            Direction::North => "north",
            Direction::South => "south",
            Direction::East => "east",
            Direction::West => "west",
            Direction::Up => "up",
            Direction::Down => "down",
        }
    }
}

/// A fully parsed player command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Describe the current room (maps to `az resource list` within a group).
    Look,
    /// Inspect a specific object/creature (maps to `az resource show`).
    Examine(String),
    /// Move in a direction (navigate resource groups / regions / subscriptions).
    Go(Direction),
    /// Acquire a resource into inventory (maps to `az resource create`/adopt).
    Take(String),
    /// Delete/release a resource, with confirmation (maps to `az resource delete`).
    Drop(String),
    /// Secure/lock a resource (maps to enabling protections / RBAC lock).
    Lock(String),
    /// Remove a management lock so the resource can be changed/deleted again
    /// (maps to `az lock delete`).
    Unlock(String),
    /// Right-size a resource to cut its monthly cost (maps to changing a SKU /
    /// scaling down a tier).
    Resize(String),
    /// Enable monitoring on the current room, banishing the Grue.
    Monitor,
    /// Show carried resources.
    Inventory,
    /// Report governance-posture score.
    Score,
    /// Report the governance scorecard: score/rank plus achievement badges.
    Achievements,
    /// Report progress on the built-in governance quests.
    Quest,
    /// Cast a "spell": currently `deploy` (bicep/ARM deployment).
    Cast(String),
    /// Teach AzZork a new az command group by introspecting `az <group> --help`.
    Learn(String),
    /// List the az capabilities AzZork has learned so far.
    Capabilities,
    /// Record a friction note into persistent memory (something to improve).
    Friction(String),
    /// Ranked recall over persistent memory for a free-text query.
    Recall(String),
    /// Summarise what AzZork remembers (counts by kind + recent notes).
    Memory,
    /// Show help.
    Help,
    /// Print the AzZork version banner.
    Version,
    /// Leave the game.
    Quit,
    /// Player entered nothing.
    Empty,
    /// Input could not be understood; carries the original text.
    Unknown(String),
}

/// Filler words that are stripped before interpreting a command.
const FILLER: &[&str] = &["the", "a", "an", "at", "to", "into", "on", "my"];

/// Returns the verbatim remainder of `input` after its first whitespace-
/// separated token (the verb), preserving original case and filler words.
/// Used for free-text arguments (e.g. [`Command::Friction`],
/// [`Command::Recall`]) that must not be mangled by dispatch-only
/// lowercasing/filler-stripping.
fn verbatim_remainder(input: &str) -> String {
    let mut words = input.split_whitespace();
    words.next(); // skip the verb token
    words.collect::<Vec<_>>().join(" ")
}

/// Build a single-argument [`Command`] from `arg`, falling back to
/// `Command::Unknown(input)` if `arg` is empty. Shared by every verb that
/// requires a non-empty target/text argument, avoiding a repeated
/// if-empty/else block per verb.
fn require_arg(arg: String, input: &str, ctor: fn(String) -> Command) -> Command {
    if arg.is_empty() {
        Command::Unknown(input.to_string())
    } else {
        ctor(arg)
    }
}

/// Parse a raw input line into a [`Command`].
pub fn parse(input: &str) -> Command {
    let lowered = input.to_lowercase();
    // Borrow tokens from `lowered` instead of allocating a new String per
    // token; `rest` below is a plain slice, avoiding a second Vec clone.
    let tokens: Vec<&str> = lowered
        .split_whitespace()
        .filter(|t| !FILLER.contains(t))
        .collect();

    if tokens.is_empty() {
        return Command::Empty;
    }

    let verb = tokens[0];
    let rest = &tokens[1..];
    let arg = rest.join(" ");

    // A bare direction ("north", "n") is shorthand for "go north".
    if let Some(dir) = Direction::from_token(verb) {
        return Command::Go(dir);
    }

    match verb {
        "look" | "l" => Command::Look,
        "examine" | "x" | "inspect" | "show" => require_arg(arg, input, Command::Examine),
        "go" | "move" | "walk" => match rest.first().copied().and_then(Direction::from_token) {
            Some(dir) => Command::Go(dir),
            None => Command::Unknown(input.to_string()),
        },
        "take" | "get" | "grab" | "acquire" => require_arg(arg, input, Command::Take),
        "drop" | "delete" | "release" | "rm" => require_arg(arg, input, Command::Drop),
        "lock" | "secure" => require_arg(arg, input, Command::Lock),
        "unlock" | "unward" | "unsecure" => require_arg(arg, input, Command::Unlock),
        "resize" | "rightsize" | "right-size" | "scale" | "downsize" => {
            require_arg(arg, input, Command::Resize)
        }
        "monitor" | "light" => Command::Monitor,
        "inventory" | "i" | "inv" => Command::Inventory,
        "score" => Command::Score,
        "achievements" | "badges" => Command::Achievements,
        "quest" | "quests" => Command::Quest,
        "cast" => require_arg(arg, input, Command::Cast),
        // Allow "deploy ..." as a convenience alias for "cast deploy".
        "deploy" => Command::Cast(format!("deploy {}", arg).trim().to_string()),
        "learn" | "discover" | "study" => require_arg(arg, input, Command::Learn),
        "capabilities" | "caps" | "powers" | "spells" => Command::Capabilities,
        // Verbatim (original-case, filler-preserving) text after the verb
        // token; computed lazily here since only Friction/Recall need it.
        "friction" | "note" | "gripe" => {
            require_arg(verbatim_remainder(input), input, Command::Friction)
        }
        "recall" | "remember" => require_arg(verbatim_remainder(input), input, Command::Recall),
        "memory" | "mem" | "recollect" => Command::Memory,
        "help" | "?" | "h" => Command::Help,
        "version" | "ver" => Command::Version,
        "quit" | "q" | "exit" => Command::Quit,
        _ => Command::Unknown(input.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_is_empty_command() {
        assert_eq!(parse(""), Command::Empty);
        assert_eq!(parse("   "), Command::Empty);
    }

    #[test]
    fn look_aliases() {
        assert_eq!(parse("look"), Command::Look);
        assert_eq!(parse("l"), Command::Look);
        assert_eq!(parse("LOOK"), Command::Look);
    }

    #[test]
    fn bare_directions_become_go() {
        assert_eq!(parse("north"), Command::Go(Direction::North));
        assert_eq!(parse("n"), Command::Go(Direction::North));
        assert_eq!(parse("down"), Command::Go(Direction::Down));
    }

    #[test]
    fn go_with_direction() {
        assert_eq!(parse("go south"), Command::Go(Direction::South));
        assert_eq!(parse("move west"), Command::Go(Direction::West));
        assert_eq!(parse("go the north"), Command::Go(Direction::North));
    }

    #[test]
    fn go_without_direction_is_unknown() {
        assert!(matches!(parse("go sideways"), Command::Unknown(_)));
        assert!(matches!(parse("go"), Command::Unknown(_)));
    }

    #[test]
    fn examine_captures_multiword_target() {
        assert_eq!(
            parse("examine the storage account"),
            Command::Examine("storage account".to_string())
        );
        assert_eq!(parse("x vm"), Command::Examine("vm".to_string()));
    }

    #[test]
    fn take_and_drop() {
        assert_eq!(
            parse("take keyvault"),
            Command::Take("keyvault".to_string())
        );
        assert_eq!(
            parse("get the keyvault"),
            Command::Take("keyvault".to_string())
        );
        assert_eq!(
            parse("drop database"),
            Command::Drop("database".to_string())
        );
        assert_eq!(
            parse("delete the database"),
            Command::Drop("database".to_string())
        );
    }

    #[test]
    fn lock_and_monitor() {
        assert_eq!(parse("lock storage"), Command::Lock("storage".to_string()));
        assert_eq!(
            parse("secure the storage"),
            Command::Lock("storage".to_string())
        );
        assert_eq!(parse("monitor"), Command::Monitor);
        assert_eq!(parse("light"), Command::Monitor);
    }

    #[test]
    fn unlock_and_resize() {
        assert_eq!(
            parse("unlock keyvault"),
            Command::Unlock("keyvault".to_string())
        );
        assert_eq!(
            parse("unward the keyvault"),
            Command::Unlock("keyvault".to_string())
        );
        assert_eq!(
            parse("resize sqlserver"),
            Command::Resize("sqlserver".to_string())
        );
        assert_eq!(
            parse("right-size sqlserver"),
            Command::Resize("sqlserver".to_string())
        );
        assert_eq!(
            parse("scale the sqlserver"),
            Command::Resize("sqlserver".to_string())
        );
        assert!(matches!(parse("unlock"), Command::Unknown(_)));
        assert!(matches!(parse("resize"), Command::Unknown(_)));
    }

    #[test]
    fn inventory_score_help_quit() {
        assert_eq!(parse("inventory"), Command::Inventory);
        assert_eq!(parse("i"), Command::Inventory);
        assert_eq!(parse("score"), Command::Score);
        assert_eq!(parse("achievements"), Command::Achievements);
        assert_eq!(parse("badges"), Command::Achievements);
        assert_eq!(parse("help"), Command::Help);
        assert_eq!(parse("?"), Command::Help);
        assert_eq!(parse("quit"), Command::Quit);
        assert_eq!(parse("exit"), Command::Quit);
    }

    #[test]
    fn quest_aliases() {
        assert_eq!(parse("quest"), Command::Quest);
        assert_eq!(parse("quests"), Command::Quest);
        assert_eq!(parse("QUEST"), Command::Quest);
    }

    #[test]
    fn cast_deploy() {
        assert_eq!(parse("cast deploy"), Command::Cast("deploy".to_string()));
        assert_eq!(
            parse("cast deploy webapp.bicep"),
            Command::Cast("deploy webapp.bicep".to_string())
        );
        assert_eq!(parse("deploy"), Command::Cast("deploy".to_string()));
    }

    #[test]
    fn learn_and_capabilities() {
        assert_eq!(
            parse("learn storage"),
            Command::Learn("storage".to_string())
        );
        assert_eq!(
            parse("discover the network"),
            Command::Learn("network".to_string())
        );
        assert!(matches!(parse("learn"), Command::Unknown(_)));
        assert_eq!(parse("capabilities"), Command::Capabilities);
        assert_eq!(parse("caps"), Command::Capabilities);
        assert_eq!(parse("powers"), Command::Capabilities);
    }

    #[test]
    fn unknown_verb() {
        assert!(matches!(parse("frobnicate the vm"), Command::Unknown(_)));
    }

    #[test]
    fn memory_commands() {
        assert_eq!(
            parse("friction help is confusing"),
            Command::Friction("help is confusing".to_string())
        );
        // The verb ("note") is stripped, but the remaining free text is kept
        // verbatim -- including filler words like "the" -- since it is not
        // used for dispatch.
        assert_eq!(
            parse("note the errors are cryptic"),
            Command::Friction("the errors are cryptic".to_string())
        );
        assert_eq!(
            parse("recall storage"),
            Command::Recall("storage".to_string())
        );
        assert_eq!(parse("memory"), Command::Memory);
        assert_eq!(parse("mem"), Command::Memory);
        assert!(matches!(parse("friction"), Command::Unknown(_)));
        assert!(matches!(parse("recall"), Command::Unknown(_)));
    }

    #[test]
    fn friction_and_recall_preserve_verbatim_free_text() {
        // Regression test for #79: filler-word stripping and forced
        // lowercasing must never corrupt the free-text argument captured by
        // Friction/Recall -- only the leading verb token is removed.
        assert_eq!(
            parse("friction this is a test note"),
            Command::Friction("this is a test note".to_string())
        );
        assert_eq!(
            parse("friction a cat sat on a mat"),
            Command::Friction("a cat sat on a mat".to_string())
        );
        assert_eq!(
            parse("friction I am a robot"),
            Command::Friction("I am a robot".to_string())
        );
        assert_eq!(
            parse("recall Some MixedCase Query"),
            Command::Recall("Some MixedCase Query".to_string())
        );
    }

    #[test]
    fn friction_and_recall_handle_non_ascii_text() {
        // Regression test: multi-byte UTF-8 characters must not panic or
        // corrupt the split when computing the verbatim remainder.
        assert_eq!(
            parse("friction café ☕ is confusing"),
            Command::Friction("café ☕ is confusing".to_string())
        );
        assert_eq!(
            parse("recall naïve résumé 日本語"),
            Command::Recall("naïve résumé 日本語".to_string())
        );
    }

    #[test]
    fn direction_from_token() {
        assert_eq!(Direction::from_token("n"), Some(Direction::North));
        assert_eq!(Direction::from_token("east"), Some(Direction::East));
        assert_eq!(Direction::from_token("nowhere"), None);
    }
}
