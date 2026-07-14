//! Command parser for the AzZork text adventure.
//!
//! Turns a raw line of player input into a structured [`Command`]. The parser is
//! deliberately forgiving: it lowercases input, strips filler words ("the", "a",
//! "an", "at", "to"), and understands common Zork-style aliases (e.g. bare
//! directions like `north`, or `l` for `look`).

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
    /// Enable monitoring on the current room, banishing the Grue.
    Monitor,
    /// Show carried resources.
    Inventory,
    /// Report governance-posture score.
    Score,
    /// Cast a "spell": currently `deploy` (bicep/ARM deployment).
    Cast(String),
    /// Show help.
    Help,
    /// Leave the game.
    Quit,
    /// Player entered nothing.
    Empty,
    /// Input could not be understood; carries the original text.
    Unknown(String),
}

/// Filler words that are stripped before interpreting a command.
const FILLER: &[&str] = &["the", "a", "an", "at", "to", "into", "on", "my"];

/// Parse a raw input line into a [`Command`].
pub fn parse(input: &str) -> Command {
    let lowered = input.to_lowercase();
    let tokens: Vec<String> = lowered
        .split_whitespace()
        .filter(|t| !FILLER.contains(t))
        .map(|t| t.to_string())
        .collect();

    if tokens.is_empty() {
        return Command::Empty;
    }

    let verb = tokens[0].as_str();
    let rest: Vec<String> = tokens[1..].to_vec();
    let arg = rest.join(" ");

    // A bare direction ("north", "n") is shorthand for "go north".
    if let Some(dir) = Direction::from_token(verb) {
        return Command::Go(dir);
    }

    match verb {
        "look" | "l" => Command::Look,
        "examine" | "x" | "inspect" | "show" => {
            if arg.is_empty() {
                Command::Unknown(input.to_string())
            } else {
                Command::Examine(arg)
            }
        }
        "go" | "move" | "walk" => match rest.first().and_then(|t| Direction::from_token(t)) {
            Some(dir) => Command::Go(dir),
            None => Command::Unknown(input.to_string()),
        },
        "take" | "get" | "grab" | "acquire" => {
            if arg.is_empty() {
                Command::Unknown(input.to_string())
            } else {
                Command::Take(arg)
            }
        }
        "drop" | "delete" | "release" | "rm" => {
            if arg.is_empty() {
                Command::Unknown(input.to_string())
            } else {
                Command::Drop(arg)
            }
        }
        "lock" | "secure" => {
            if arg.is_empty() {
                Command::Unknown(input.to_string())
            } else {
                Command::Lock(arg)
            }
        }
        "monitor" | "light" => Command::Monitor,
        "inventory" | "i" | "inv" => Command::Inventory,
        "score" => Command::Score,
        "cast" => {
            if arg.is_empty() {
                Command::Unknown(input.to_string())
            } else {
                Command::Cast(arg)
            }
        }
        // Allow "deploy ..." as a convenience alias for "cast deploy".
        "deploy" => Command::Cast(format!("deploy {}", arg).trim().to_string()),
        "help" | "?" | "h" => Command::Help,
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
        assert_eq!(parse("take keyvault"), Command::Take("keyvault".to_string()));
        assert_eq!(parse("get the keyvault"), Command::Take("keyvault".to_string()));
        assert_eq!(parse("drop database"), Command::Drop("database".to_string()));
        assert_eq!(parse("delete the database"), Command::Drop("database".to_string()));
    }

    #[test]
    fn lock_and_monitor() {
        assert_eq!(parse("lock storage"), Command::Lock("storage".to_string()));
        assert_eq!(parse("secure the storage"), Command::Lock("storage".to_string()));
        assert_eq!(parse("monitor"), Command::Monitor);
        assert_eq!(parse("light"), Command::Monitor);
    }

    #[test]
    fn inventory_score_help_quit() {
        assert_eq!(parse("inventory"), Command::Inventory);
        assert_eq!(parse("i"), Command::Inventory);
        assert_eq!(parse("score"), Command::Score);
        assert_eq!(parse("help"), Command::Help);
        assert_eq!(parse("?"), Command::Help);
        assert_eq!(parse("quit"), Command::Quit);
        assert_eq!(parse("exit"), Command::Quit);
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
    fn unknown_verb() {
        assert!(matches!(parse("frobnicate the vm"), Command::Unknown(_)));
    }

    #[test]
    fn direction_from_token() {
        assert_eq!(Direction::from_token("n"), Some(Direction::North));
        assert_eq!(Direction::from_token("east"), Some(Direction::East));
        assert_eq!(Direction::from_token("nowhere"), None);
    }
}
