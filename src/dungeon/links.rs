//! Azure portal deep-link construction.
//!
//! See `docs/DUNGEON-CRAWLER.md#portal-deep-links`: a resource's Azure
//! Resource Manager ID is turned directly into a portal resource-blade deep
//! link of the form `https://portal.azure.com/#@/resource/<resourceId>`. No
//! network call, no authentication — this is pure string construction.

/// Base portal URL prefix that a (leading-slash-stripped) resource ID is
/// appended to.
pub const PORTAL_BASE: &str = "https://portal.azure.com/#@/resource/";

/// Build the Azure portal deep link for a resource's full ARM id.
///
/// A leading `/` on `resource_id` (as returned by `az ... -o json`) is
/// stripped so the link matches the documented example exactly; an id with
/// no leading slash is accepted unchanged.
pub fn portal_url(resource_id: &str) -> String {
    let trimmed = resource_id.strip_prefix('/').unwrap_or(resource_id);
    format!("{}{}", PORTAL_BASE, trimmed)
}
