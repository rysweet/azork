//! Azure resource type -> suggested read-only `az` command lookup.
//!
//! Derived from the same single type-mapping table as
//! [`crate::dungeon::icons::icon_for`] (see `docs/DUNGEON-CRAWLER.md#suggested-az-commands`),
//! so the icon and its suggested command for a given type can never drift out
//! of sync. These strings are **display-only**: the map never shells out to
//! run them.

use crate::dungeon::type_table;

/// Suggested command used when a resource type has no specific mapping.
pub const DEFAULT_COMMAND_TEMPLATE: &str = type_table::DEFAULT_COMMAND_TEMPLATE;

/// Return one or more suggested, read-only `az` command lines for inspecting
/// a resource of `resource_type` with ARM id `resource_id`. The returned
/// strings have `resource_id` already substituted in and are ready to
/// display verbatim (never executed by AzZork itself).
pub fn suggested_commands(resource_type: &str, resource_id: &str) -> Vec<String> {
    let template = match type_table::lookup(resource_type) {
        Some(entry) => entry.command_template,
        None => DEFAULT_COMMAND_TEMPLATE,
    };
    vec![template.replace("{id}", resource_id)]
}
