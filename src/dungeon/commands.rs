//! Azure resource type -> suggested read-only `az` command lookup.
//!
//! Derived from the same single type-mapping table as
//! [`crate::dungeon::icons::icon_for`] (see `docs/DUNGEON-CRAWLER.md#suggested-az-commands`),
//! so the icon and its suggested command for a given type can never drift out
//! of sync. These strings are **display-only**: the map never shells out to
//! run them.

use crate::dungeon::type_table;
use crate::dungeon::validate;
use crate::secrets::scrub;

/// Suggested command used when a resource type has no specific mapping.
pub const DEFAULT_COMMAND_TEMPLATE: &str = type_table::DEFAULT_COMMAND_TEMPLATE;
const READ_ONLY_VERBS: &[&str] = &["list", "show"];
const MUTATING_VERBS: &[&str] = &[
    "add", "assign", "create", "delete", "deploy", "invoke", "lock", "move", "patch", "purge",
    "remove", "replace", "reset", "restart", "revoke", "rotate", "run", "set", "start", "stop",
    "sync", "unlock", "update", "write",
];

/// Return `true` when a displayed command stays within the crawler's read-only
/// policy.
pub fn is_read_only_command(command: &str) -> bool {
    if command
        .chars()
        .any(|ch| matches!(ch, ';' | '&' | '|' | '`' | '$' | '>' | '<'))
    {
        return false;
    }
    let tokens: Vec<String> = command
        .split_whitespace()
        .map(|tok| tok.to_ascii_lowercase())
        .collect();
    if tokens.first().map(String::as_str) != Some("az") {
        return false;
    }
    if tokens
        .iter()
        .any(|tok| MUTATING_VERBS.contains(&tok.as_str()))
    {
        return false;
    }
    tokens
        .iter()
        .any(|tok| READ_ONLY_VERBS.contains(&tok.as_str()))
}

/// Return one or more suggested, read-only `az` command lines for inspecting
/// a resource of `resource_type` with ARM id `resource_id`. The returned
/// strings have `resource_id` already substituted in and are ready to
/// display verbatim (never executed by AzZork itself).
pub fn suggested_commands(resource_type: &str, resource_id: &str) -> Vec<String> {
    let Some(parsed) = validate::parse_resource_id(resource_id) else {
        return Vec::new();
    };
    let template = match type_table::lookup(resource_type) {
        Some(entry) => entry.command_template,
        None => DEFAULT_COMMAND_TEMPLATE,
    };
    if !is_read_only_command(template) {
        return Vec::new();
    }
    let command = template
        .replace("{id}", parsed.raw())
        .replace("{resource_group}", parsed.resource_group())
        .replace("{subscription}", parsed.subscription_id());
    if !is_read_only_command(&command) {
        return Vec::new();
    }
    vec![scrub(&command)]
}
