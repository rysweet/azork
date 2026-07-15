//! Validation of Azure identifiers before Dungeon Crawler Mode interpolates
//! them into deep links or suggested command strings.

/// Parsed, validated ARM resource id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceId<'a> {
    raw: &'a str,
    subscription_id: &'a str,
    resource_group: &'a str,
}

impl<'a> ResourceId<'a> {
    pub fn raw(&self) -> &'a str {
        self.raw
    }

    pub fn subscription_id(&self) -> &'a str {
        self.subscription_id
    }

    pub fn resource_group(&self) -> &'a str {
        self.resource_group
    }
}

/// Validate an Azure subscription id in canonical GUID form.
pub fn is_valid_subscription_id(value: &str) -> bool {
    if value.len() != 36 {
        return false;
    }
    for (idx, ch) in value.chars().enumerate() {
        let is_hyphen = matches!(idx, 8 | 13 | 18 | 23);
        if is_hyphen {
            if ch != '-' {
                return false;
            }
        } else if !ch.is_ascii_hexdigit() {
            return false;
        }
    }
    true
}

/// Validate an Azure resource-group name.
pub fn is_valid_resource_group_name(value: &str) -> bool {
    if value.is_empty() || value.len() > 90 || value.ends_with('.') {
        return false;
    }
    value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '(' | ')' | '-'))
}

/// Parse and validate an ARM resource id.
pub fn parse_resource_id(resource_id: &str) -> Option<ResourceId<'_>> {
    let trimmed = resource_id.strip_prefix('/').unwrap_or(resource_id);
    let parts: Vec<&str> = trimmed.split('/').collect();
    if parts.len() < 8 || parts.first()? != &"subscriptions" {
        return None;
    }
    let subscription_id = *parts.get(1)?;
    if !is_valid_subscription_id(subscription_id) {
        return None;
    }
    if parts.get(2)? != &"resourceGroups" {
        return None;
    }
    let resource_group = *parts.get(3)?;
    if !is_valid_resource_group_name(resource_group) {
        return None;
    }
    if parts.get(4)? != &"providers" || !is_valid_provider_namespace(parts.get(5)?) {
        return None;
    }
    let tail = &parts[6..];
    if tail.len() < 2 || !tail.len().is_multiple_of(2) {
        return None;
    }
    for segment in tail {
        if !is_valid_resource_segment(segment) {
            return None;
        }
    }
    Some(ResourceId {
        raw: resource_id,
        subscription_id,
        resource_group,
    })
}

fn is_valid_provider_namespace(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-'))
}

fn is_valid_resource_segment(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '(' | ')' | '-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_subscription_ids() {
        assert!(is_valid_subscription_id(
            "00000000-0000-0000-0000-000000000000"
        ));
        assert!(!is_valid_subscription_id("not-a-guid"));
    }

    #[test]
    fn validates_resource_group_names() {
        assert!(is_valid_resource_group_name("rg-prod_eastus-1"));
        assert!(!is_valid_resource_group_name("rg bad"));
        assert!(!is_valid_resource_group_name("bad."));
    }

    #[test]
    fn parses_valid_resource_ids() {
        let parsed = parse_resource_id(
            "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/web-rg/providers/Microsoft.Web/sites/app1",
        )
        .expect("resource id should parse");
        assert_eq!(
            parsed.subscription_id(),
            "00000000-0000-0000-0000-000000000000"
        );
        assert_eq!(parsed.resource_group(), "web-rg");
    }

    #[test]
    fn rejects_invalid_resource_ids() {
        assert!(parse_resource_id(
            "/subscriptions/not-a-guid/resourceGroups/web-rg/providers/Microsoft.Web/sites/app1"
        )
        .is_none());
        assert!(parse_resource_id("/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/web rg/providers/Microsoft.Web/sites/app1").is_none());
        assert!(parse_resource_id("/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/web-rg/providers/Microsoft.Web/sites/app 1").is_none());
    }
}
