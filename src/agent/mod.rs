//! Agentic intent resolution.
//!
//! When a player types something AzZork's core verb grammar does not recognise,
//! the game should *try to figure out what they meant* rather than fail. This
//! module turns unknown input into a [`Resolution`] by consulting the learned
//! [`CapabilityRegistry`] through an [`Adapter`].
//!
//! The default [`MockAdapter`] is deterministic and fully offline: it ranks the
//! registry's learned capabilities against the input and returns the best
//! matches as suggestions ("did you mean…"). This is the `Adapter` seam a
//! richer, live agent back-end can slot into for real agentic steps, while
//! guaranteeing the offline build never performs network or subprocess work.

use crate::capabilities::{Capability, CapabilityRegistry};

/// Maximum number of characters of free-text player input that are ever
/// echoed back to the terminal or persisted to the memory graph as part of
/// an "unresolved intent" friction note. Without this cap a very long line
/// of unrecognised input would be stored verbatim in `memory.graph` and
/// echoed to the terminal in full, growing the on-disk memory file
/// unbounded (see GitHub issue #32).
pub const MAX_INTENT_ECHO_LEN: usize = 200;

/// Truncate `raw` to at most [`MAX_INTENT_ECHO_LEN`] characters (on a char
/// boundary, so multi-byte UTF-8 sequences are never split), appending an
/// indicator when truncation occurred. Used both before narrating unresolved
/// input back to the player and before persisting it as friction memory.
pub fn truncate_intent(raw: &str) -> String {
    // Single pass: find the byte offset of the char just past the cap (if
    // any) instead of first counting all chars and then re-iterating to
    // collect them, which would walk the string twice for long input.
    match raw.char_indices().nth(MAX_INTENT_ECHO_LEN) {
        None => raw.to_string(),
        Some((cut, _)) => format!("{}...(truncated)", &raw[..cut]),
    }
}

/// The outcome of trying to resolve an ambiguous / unknown player intent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// A single confident match — treat as if the player named this capability.
    Verb(Capability),
    /// Several plausible matches to offer the player ("did you mean…").
    Suggestions(Vec<Capability>),
    /// Nothing is learned yet, but the input's nouns point at a specific az
    /// domain — steer the player to `learn <group>` instead of a dead end.
    LearnHint {
        /// The original input.
        raw: String,
        /// The az group most likely to satisfy the intent (e.g. `"storage"`).
        group: String,
    },
    /// Nothing matched; carry the original input so the caller can explain.
    Unresolved(String),
}

impl Resolution {
    /// Render the resolution as player-facing narration.
    pub fn narrate(&self) -> String {
        match self {
            Resolution::Verb(c) => format!(
                "You reach for the '{}' rite (az {}). {}",
                c.verb,
                c.command_path.join(" "),
                c.summary
            ),
            Resolution::Suggestions(cands) => {
                let mut s =
                    String::from("The runes are hazy, but you sense related powers. Did you mean:");
                for c in cands {
                    s.push_str(&format!(
                        "\n  - {} (az {})",
                        c.verb,
                        c.command_path.join(" ")
                    ));
                }
                s.push_str("\nType 'learn <group>' to teach AzZork more, or 'help'.");
                s
            }
            Resolution::LearnHint { group, .. } => format!(
                "You sense the '{group}' domain governs this, but AzZork has not \
                 studied it yet. Type 'learn {group}' to gain those powers, then try again.",
            ),
            Resolution::Unresolved(raw) => format!(
                "The incantation \"{}\" stirs nothing yet. Try 'learn <group>' to \
                 discover new powers, or 'help'.",
                truncate_intent(raw.trim())
            ),
        }
    }
}

/// A strategy for resolving intent. The trait is the seam that lets a live,
/// agentic resolver replace the deterministic offline one without touching
/// callers — mirroring the recipe-runner `Adapter` pattern.
pub trait Adapter {
    /// Attempt to resolve `input` given what AzZork currently knows.
    fn resolve(&self, input: &str, registry: &CapabilityRegistry) -> Resolution;
}

/// Deterministic, offline adapter: ranks learned capabilities against the input.
#[derive(Debug, Default, Clone, Copy)]
pub struct MockAdapter {
    /// Score gap above which a single top hit is treated as confident.
    confident_margin: i32,
}

impl MockAdapter {
    pub fn new() -> MockAdapter {
        MockAdapter {
            confident_margin: 40,
        }
    }
}

impl Adapter for MockAdapter {
    fn resolve(&self, input: &str, registry: &CapabilityRegistry) -> Resolution {
        let hits = registry.suggest(input, 5);
        match hits.as_slice() {
            [] => match infer_group(input) {
                // Nothing learned yet, but the nouns point at a domain: steer to
                // `learn <group>` rather than dead-ending the player.
                Some(group) => Resolution::LearnHint {
                    raw: input.to_string(),
                    group,
                },
                None => Resolution::Unresolved(input.to_string()),
            },
            [only] => Resolution::Verb((*only).clone()),
            [first, second, ..] => {
                // If the input names an exact verb and the top hit dominates the
                // runner-up, treat it as a confident single match.
                let verb = input.split_whitespace().next().unwrap_or("").to_lowercase();
                let dominates = registry.suggest(input, 5).len() >= 2
                    && first.verb == verb
                    && second.verb != verb
                    && self.confident_margin > 0;
                if dominates {
                    Resolution::Verb((*first).clone())
                } else {
                    Resolution::Suggestions(hits.iter().map(|c| (*c).clone()).collect())
                }
            }
        }
    }
}

/// Infer the most relevant `az` command group from the nouns in a free-text
/// intent, so AzZork can point an unstudied player at the right `learn <group>`.
///
/// Deterministic and offline: a small keyword→group table covering the common
/// domains. Returns `None` when nothing recognisable is mentioned.
pub fn infer_group(input: &str) -> Option<String> {
    let s = input.to_lowercase();
    // Ordered most-specific first so multi-word phrases win over bare tokens.
    const TABLE: &[(&str, &str)] = &[
        ("resource group", "group"),
        ("storage account", "storage"),
        ("virtual machine", "vm"),
        ("key vault", "keyvault"),
        ("keyvault", "keyvault"),
        ("blob", "storage"),
        ("container", "storage"),
        ("storage", "storage"),
        ("network", "network"),
        ("vnet", "network"),
        ("subnet", "network"),
        ("database", "sql"),
        ("sql", "sql"),
        ("cosmos", "cosmosdb"),
        ("function", "functionapp"),
        ("webapp", "webapp"),
        ("web app", "webapp"),
        ("app service", "webapp"),
        ("aks", "aks"),
        ("kubernetes", "aks"),
        ("vm", "vm"),
        ("group", "group"),
    ];
    for (needle, group) in TABLE {
        if s.contains(needle) {
            return Some((*group).to_string());
        }
    }
    None
}

/// Convenience wrapper tying an [`Adapter`] to a registry.
pub struct IntentResolver<'a, A: Adapter> {
    adapter: A,
    registry: &'a CapabilityRegistry,
}

impl<'a, A: Adapter> IntentResolver<'a, A> {
    pub fn new(adapter: A, registry: &'a CapabilityRegistry) -> IntentResolver<'a, A> {
        IntentResolver { adapter, registry }
    }

    /// Resolve a raw input line into a [`Resolution`]. Never fails.
    pub fn resolve(&self, input: &str) -> Resolution {
        self.adapter.resolve(input, self.registry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> CapabilityRegistry {
        let mut reg = CapabilityRegistry::new();
        reg.insert(Capability::new(
            "group",
            "create",
            "Create a new resource group.",
            None,
        ));
        reg.insert(Capability::new(
            "storage",
            "create",
            "Create a storage account.",
            None,
        ));
        reg.insert(Capability::new(
            "vm",
            "list",
            "List virtual machines.",
            None,
        ));
        reg
    }

    #[test]
    fn unknown_input_is_unresolved_on_empty_registry() {
        let reg = CapabilityRegistry::new();
        let r = IntentResolver::new(MockAdapter::new(), &reg);
        // "frobnicate" names no known az domain, so it stays unresolved.
        assert!(matches!(r.resolve("frobnicate"), Resolution::Unresolved(_)));
    }

    #[test]
    fn creation_intent_on_empty_registry_hints_at_learn_group() {
        let reg = CapabilityRegistry::new();
        let r = IntentResolver::new(MockAdapter::new(), &reg);
        match r.resolve("create a storage account") {
            Resolution::LearnHint { group, .. } => assert_eq!(group, "storage"),
            other => panic!("expected LearnHint, got {:?}", other),
        }
        assert!(r
            .resolve("make a new resource group")
            .narrate()
            .contains("learn group"));
    }

    #[test]
    fn infer_group_maps_common_domains() {
        assert_eq!(
            infer_group("spin up a virtual machine").as_deref(),
            Some("vm")
        );
        assert_eq!(infer_group("a new key vault").as_deref(), Some("keyvault"));
        assert_eq!(infer_group("some sql database").as_deref(), Some("sql"));
        assert_eq!(infer_group("provision a vnet").as_deref(), Some("network"));
        assert_eq!(infer_group("total gibberish here"), None);
    }

    #[test]
    fn single_match_resolves_to_verb() {
        let reg = registry();
        let r = IntentResolver::new(MockAdapter::new(), &reg);
        match r.resolve("list the vms") {
            Resolution::Verb(c) => assert_eq!(c.verb, "list"),
            other => panic!("expected Verb, got {:?}", other),
        }
    }

    #[test]
    fn ambiguous_verb_offers_suggestions() {
        let reg = registry();
        let r = IntentResolver::new(MockAdapter::new(), &reg);
        // "create" matches both group and storage — offer both.
        match r.resolve("make a create thing") {
            Resolution::Suggestions(cands) => {
                assert!(cands.iter().any(|c| c.group == "group"));
                assert!(cands.iter().any(|c| c.group == "storage"));
            }
            other => panic!("expected Suggestions, got {:?}", other),
        }
    }

    #[test]
    fn resolution_narration_is_nonempty() {
        let reg = registry();
        let r = IntentResolver::new(MockAdapter::new(), &reg);
        assert!(!r.resolve("list").narrate().is_empty());
        assert!(!r.resolve("nonsense-token").narrate().is_empty());
    }

    #[test]
    fn truncate_intent_leaves_short_input_untouched() {
        let short = "frobnicate the widget";
        assert_eq!(truncate_intent(short), short);
    }

    #[test]
    fn truncate_intent_caps_long_input_with_indicator() {
        let long = "x".repeat(2400);
        let truncated = truncate_intent(&long);
        assert!(truncated.len() < long.len());
        assert!(truncated.ends_with("...(truncated)"));
        assert_eq!(
            truncated.chars().count(),
            MAX_INTENT_ECHO_LEN + "...(truncated)".chars().count()
        );
    }

    #[test]
    fn unresolved_narration_is_bounded_for_long_input() {
        let reg = registry();
        let r = IntentResolver::new(MockAdapter::new(), &reg);
        let long_input = "z".repeat(2400);
        let narration = r.resolve(&long_input).narrate();
        assert!(narration.contains("...(truncated)"));
        // The narration wraps the (possibly truncated) input in extra prose,
        // but it must never grow proportionally with an unbounded input.
        assert!(narration.len() < 400);
    }
}
