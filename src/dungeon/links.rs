//! Azure portal deep-link construction.
//!
//! See `docs/DUNGEON-CRAWLER.md#portal-deep-links`: a resource's Azure
//! Resource Manager ID is turned directly into a portal resource-blade deep
//! link of the form `https://portal.azure.com/#@/resource/<resourceId>`. No
//! network call, no authentication — this is pure string construction.

use crate::dungeon::validate;

/// Base portal URL prefix that a (leading-slash-stripped) resource ID is
/// appended to.
pub const PORTAL_BASE: &str = "https://portal.azure.com/#@/resource/";

/// Return `true` when `resource_id` is a well-formed ARM resource id.
pub fn is_valid_resource_id(resource_id: &str) -> bool {
    validate::parse_resource_id(resource_id).is_some()
}

/// Build the Azure portal deep link for a resource's full ARM id, if valid.
pub fn try_portal_url(resource_id: &str) -> Option<String> {
    let parsed = validate::parse_resource_id(resource_id)?;
    let trimmed = parsed.raw().strip_prefix('/').unwrap_or(parsed.raw());
    Some(format!("{}{}", PORTAL_BASE, trimmed))
}

/// Build the Azure portal deep link for a resource's full ARM id.
///
/// A leading `/` on `resource_id` (as returned by `az ... -o json`) is
/// stripped so the link matches the documented example exactly; an id with
/// no leading slash is accepted unchanged.
pub fn portal_url(resource_id: &str) -> String {
    try_portal_url(resource_id).unwrap_or_else(|| "about:blank".to_string())
}
