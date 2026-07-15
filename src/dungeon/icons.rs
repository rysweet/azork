//! Azure resource type -> icon lookup.
//!
//! This is one half of the single type-mapping table described in
//! `docs/DUNGEON-CRAWLER.md#suggested-az-commands` (the other half,
//! [`crate::dungeon::commands::suggested_commands`], is derived from the same
//! table so the icon and its suggested command can never drift apart).
//! Unknown/unrecognized resource types resolve to [`DEFAULT_ICON`] rather
//! than failing or omitting the resource from the map.

use crate::dungeon::type_table;

/// Icon key used when a resource type has no specific mapping — a "mystery
/// chest" rather than a hard failure, so an unexpected or newly-released
/// resource type never breaks the crawl.
pub const DEFAULT_ICON: &str = type_table::DEFAULT_ICON;

/// Resolve the icon key for an Azure resource type (e.g.
/// `Microsoft.Storage/storageAccounts`). Matching is case-insensitive and
/// falls back to [`DEFAULT_ICON`] for anything unrecognized.
pub fn icon_for(resource_type: &str) -> &'static str {
    match type_table::lookup(resource_type) {
        Some(entry) => entry.icon,
        None => DEFAULT_ICON,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_type_falls_back_to_default() {
        assert_eq!(icon_for("Microsoft.Nonexistent/thing"), DEFAULT_ICON);
    }
}
