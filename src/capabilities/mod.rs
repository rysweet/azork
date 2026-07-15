//! Dynamic derivation and persistence of AzZork capabilities.
//!
//! Instead of hand-mapping each `az` command to a game verb, AzZork *derives*
//! its vocabulary at runtime by introspecting the real CLI: `az --help` lists
//! command groups, and `az <group> --help` lists that group's commands. Each
//! discovered command becomes a [`Capability`] — a verb the game understands,
//! surfaced adaptively through the runtime help system.
//!
//! Capabilities are persisted to a small on-disk cache so that what one session
//! learns is available to the next: AzZork *evolves* as it is used. The cache is
//! a hand-rolled, dependency-free line format (see [`registry`]).
//!
//! Nothing here calls `az` directly — everything goes through an injected
//! [`crate::az_runner::AzRunner`], so derivation is fully testable offline.

pub mod derive;
pub mod registry;

pub use registry::CapabilityRegistry;

/// A single capability AzZork has learned from the `az` CLI.
///
/// A capability maps an Azure command onto a game verb. For a top-level group
/// command like `az group create`, the `group` is `"group"`, the `verb` is
/// `"create"`, and `command_path` is `["group", "create"]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capability {
    /// The az command group this belongs to (e.g. `"group"`, `"storage"`).
    pub group: String,
    /// The leaf verb the player types (e.g. `"create"`, `"list"`).
    pub verb: String,
    /// One-line summary lifted from `az` help text.
    pub summary: String,
    /// Full command path, e.g. `["storage", "account", "create"]`.
    pub command_path: Vec<String>,
    /// Lifecycle tag if `az` flagged one (Preview / Experimental / Deprecated).
    pub status: Option<String>,
}

impl Capability {
    /// Build a capability from a group and a parsed command entry.
    pub fn new(group: &str, verb: &str, summary: &str, status: Option<String>) -> Capability {
        Capability {
            group: group.to_string(),
            verb: verb.to_string(),
            summary: summary.to_string(),
            command_path: vec![group.to_string(), verb.to_string()],
            status,
        }
    }

    /// A stable identifier used as the cache/registry key: the command path
    /// joined by spaces, e.g. `"group create"`.
    pub fn key(&self) -> String {
        self.command_path.join(" ")
    }

    /// The `az` invocation this capability represents, e.g.
    /// `["group", "create"]`, ready to hand to an [`crate::az_runner::AzRunner`].
    pub fn az_args(&self) -> Vec<String> {
        self.command_path.clone()
    }

    /// A single help line describing this capability for the runtime help
    /// system, e.g. `"  create        az group create — Create a new ..."`.
    pub fn help_line(&self) -> String {
        let tag = match &self.status {
            Some(s) => format!(" [{}]", s),
            None => String::new(),
        };
        format!(
            "  {:<14} az {}{} — {}",
            self.verb,
            self.command_path.join(" "),
            tag,
            self.summary
        )
    }
}
