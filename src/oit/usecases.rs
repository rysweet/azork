//! The OIT agent's catalog of use cases and its friction-detection heuristics.
//!
//! A [`UseCase`] is a small, self-contained scenario the agent drives *through
//! azork* (by feeding the game a script of commands). The catalog aims for
//! breadth and creativity — navigation, examination, cheap creation, securing,
//! governance scoring, deployment, and dynamically-derived capabilities — not
//! just happy paths.
//!
//! Friction detection ([`detect_friction`]) is a pure function over a command
//! and azork's response, so the agent's judgement about "what to improve" is
//! deterministic and unit-tested offline.

/// Broad categories a use case can exercise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Navigation,
    Examination,
    Creation,
    Security,
    Governance,
    Deployment,
    Discovery,
    Memory,
}

impl Category {
    pub fn label(self) -> &'static str {
        match self {
            Category::Navigation => "navigation",
            Category::Examination => "examination",
            Category::Creation => "creation",
            Category::Security => "security",
            Category::Governance => "governance",
            Category::Deployment => "deployment",
            Category::Discovery => "discovery",
            Category::Memory => "memory",
        }
    }
}

/// One scenario the agent drives through azork.
#[derive(Debug, Clone)]
pub struct UseCase {
    /// Stable identifier, e.g. `"nav-look"`.
    pub id: &'static str,
    /// One-line human description.
    pub title: &'static str,
    /// What this use case exercises.
    pub category: Category,
    /// The lines fed to azork's stdin, in order.
    pub script: Vec<&'static str>,
}

/// The full, curated use-case catalog the agent runs against azork.
pub fn catalog() -> Vec<UseCase> {
    vec![
        UseCase {
            id: "nav-look",
            title: "Look around the starting resource group",
            category: Category::Navigation,
            script: vec!["look"],
        },
        UseCase {
            id: "nav-explore",
            title: "Wander the estate in every direction",
            category: Category::Navigation,
            script: vec!["look", "north", "look", "east", "look", "south", "west"],
        },
        UseCase {
            id: "exam-resource",
            title: "Examine a resource in the current room",
            category: Category::Examination,
            script: vec!["look", "examine storage"],
        },
        UseCase {
            id: "gov-score",
            title: "Report the governance posture score",
            category: Category::Governance,
            script: vec!["score"],
        },
        UseCase {
            id: "gov-inventory",
            title: "Inspect carried resources",
            category: Category::Governance,
            script: vec!["inventory"],
        },
        UseCase {
            id: "sec-monitor",
            title: "Light the current room (enable monitoring)",
            category: Category::Security,
            script: vec!["monitor", "score"],
        },
        UseCase {
            id: "sec-lock",
            title: "Secure a resource (lock + private + encrypted)",
            category: Category::Security,
            script: vec!["look", "lock storage", "score"],
        },
        UseCase {
            id: "deploy-cast",
            title: "Cast a (mock) deployment spell",
            category: Category::Deployment,
            script: vec!["cast deploy", "deploy webapp.bicep"],
        },
        UseCase {
            id: "disc-learn-group",
            title: "Dynamically derive the 'group' capabilities from az",
            category: Category::Discovery,
            script: vec!["learn group", "capabilities"],
        },
        UseCase {
            id: "disc-learn-storage",
            title: "Dynamically derive the 'storage' capabilities from az",
            category: Category::Discovery,
            script: vec!["learn storage", "capabilities"],
        },
        UseCase {
            id: "disc-intent",
            title: "Resolve an ambiguous free-text intent",
            category: Category::Discovery,
            script: vec!["learn group", "make a new resource group please"],
        },
        UseCase {
            id: "mem-recall",
            title: "Recall what azork has learned and remembered",
            category: Category::Memory,
            script: vec!["learn group", "recall create", "memory"],
        },
        UseCase {
            id: "create-guidance",
            title: "Ask azork how to create something new",
            category: Category::Creation,
            script: vec!["create a storage account"],
        },
        UseCase {
            id: "exam-missing",
            title: "Examine a resource that does not exist",
            category: Category::Examination,
            script: vec!["examine nonexistent-thing"],
        },
    ]
}

/// A single friction observation — something confusing, missing, or worth
/// improving that surfaced while driving a use case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Friction {
    /// The azork command that provoked it.
    pub command: String,
    /// The category of problem.
    pub kind: FrictionKind,
    /// Human-readable note.
    pub note: String,
}

/// The kind of friction observed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrictionKind {
    /// azork could not understand the input at all.
    Unresolved,
    /// The response was empty / unhelpful.
    Empty,
    /// azork reported it lacked a capability.
    MissingCapability,
    /// A confusing or low-quality error/message.
    ConfusingMessage,
}

impl FrictionKind {
    pub fn label(self) -> &'static str {
        match self {
            FrictionKind::Unresolved => "unresolved-intent",
            FrictionKind::Empty => "empty-response",
            FrictionKind::MissingCapability => "missing-capability",
            FrictionKind::ConfusingMessage => "confusing-message",
        }
    }
}

/// Inspect azork's response to a command and decide whether it constitutes
/// friction. Pure and deterministic — the heart of the "find the gap" loop.
pub fn detect_friction(command: &str, output: &str) -> Option<Friction> {
    let cmd = command.trim();
    // Commands that intentionally do nothing (blank) are not friction.
    if cmd.is_empty() {
        return None;
    }
    let lowered = output.to_lowercase();

    if output.trim().is_empty() {
        return Some(Friction {
            command: cmd.to_string(),
            kind: FrictionKind::Empty,
            note: format!("'{cmd}' produced no response at all"),
        });
    }
    // azork's own "I don't understand" narration. (Note: "incantation" also
    // appears in *successful* deploy flavour text, so it is deliberately NOT a
    // signal here — only phrases unique to non-resolution are.)
    if lowered.contains("stirs nothing") || lowered.contains("could not be understood") {
        return Some(Friction {
            command: cmd.to_string(),
            kind: FrictionKind::Unresolved,
            note: format!("'{cmd}' was not resolved to any azork capability"),
        });
    }
    if lowered.contains("don't know") || lowered.contains("nothing usable") {
        return Some(Friction {
            command: cmd.to_string(),
            kind: FrictionKind::MissingCapability,
            note: format!("'{cmd}' hit a missing/unknown capability"),
        });
    }
    // Suggestion mode ("did you mean…") is soft friction: azork coped, but the
    // player still had to disambiguate.
    if lowered.contains("did you mean") {
        return Some(Friction {
            command: cmd.to_string(),
            kind: FrictionKind::ConfusingMessage,
            note: format!("'{cmd}' required disambiguation (did-you-mean)"),
        });
    }
    None
}

/// Split a full azork session transcript into per-command output chunks.
///
/// azork prints an `az> ` prompt before reading each command, so splitting on
/// that marker yields: `[banner+intro, out_of_cmd_1, out_of_cmd_2, ...]`. This
/// returns the command outputs only (the leading intro chunk is dropped), so the
/// result aligns positionally with the script that was fed in.
pub fn split_by_prompt(transcript: &str) -> Vec<String> {
    let mut parts: Vec<String> = transcript.split("az> ").map(|s| s.to_string()).collect();
    if !parts.is_empty() {
        parts.remove(0); // drop the banner / initial-look preamble
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_broad_and_unique() {
        let cases = catalog();
        assert!(cases.len() >= 12, "expect a broad catalog");
        // ids are unique
        let mut ids: Vec<&str> = cases.iter().map(|c| c.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), cases.len(), "use-case ids must be unique");
        // covers many categories
        let mut cats: Vec<&str> = cases.iter().map(|c| c.category.label()).collect();
        cats.sort();
        cats.dedup();
        assert!(cats.len() >= 6, "expect broad category coverage");
    }

    #[test]
    fn empty_output_is_friction() {
        let f = detect_friction("score", "").unwrap();
        assert_eq!(f.kind, FrictionKind::Empty);
    }

    #[test]
    fn unresolved_narration_is_friction() {
        let f = detect_friction(
            "frobnicate",
            "The incantation \"frobnicate\" stirs nothing yet.",
        )
        .unwrap();
        assert_eq!(f.kind, FrictionKind::Unresolved);
    }

    #[test]
    fn missing_capability_is_friction() {
        let f = detect_friction("cast fireball", "You don't know the spell 'fireball'.").unwrap();
        assert_eq!(f.kind, FrictionKind::MissingCapability);
    }

    #[test]
    fn healthy_output_is_not_friction() {
        assert!(detect_friction("score", "Governance score: 82/100 — Fortified.").is_none());
        assert!(detect_friction("", "anything").is_none());
    }

    #[test]
    fn did_you_mean_is_soft_friction() {
        let f = detect_friction("make thing", "The runes are hazy. Did you mean: create").unwrap();
        assert_eq!(f.kind, FrictionKind::ConfusingMessage);
    }

    #[test]
    fn deploy_flavour_text_is_not_friction() {
        // "incantation" appears in successful deploy output — must not flag.
        assert!(detect_friction(
            "deploy webapp.bicep",
            "The bicep incantation compiles and deploys. (mock: no real resources.)"
        )
        .is_none());
    }

    #[test]
    fn split_by_prompt_aligns_with_script() {
        let transcript = "BANNER\nintro\naz> look output\naz> score output\naz> bye";
        let parts = split_by_prompt(transcript);
        assert_eq!(parts.len(), 3);
        assert!(parts[0].contains("look output"));
        assert!(parts[1].contains("score output"));
    }
}
