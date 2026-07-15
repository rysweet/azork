//! Hard guardrails for the Outside-In-Testing (OIT) agent.
//!
//! These rules are enforced **in code**, not just in agent behaviour, so the OIT
//! agent physically cannot violate the mission's safety contract:
//!
//! 1. **Cost** — never take an action estimated over `$500`; strongly prefer
//!    free/cheap SKUs. [`assess_cost`] is the gate every create must pass.
//! 2. **Cleanup** — everything the agent creates is tagged for identification and
//!    torn down. [`oit_tags`] stamps the canonical tags.
//! 3. **Non-destructive** — the agent only ever touches resources bearing *its
//!    own* tags. [`is_own_resource`] / [`guard_mutation`] are the gate every
//!    delete/mutate must pass.
//! 4. **Isolation** — all test resources live in dedicated resource groups
//!    prefixed [`OIT_RG_PREFIX`] in a cheap region ([`OIT_REGION`]).
//!
//! Everything here is pure and deterministic — no `az`, no network — so the whole
//! guardrail contract is unit-tested offline.

use std::collections::BTreeMap;

/// Prefix for every resource group the OIT agent creates.
pub const OIT_RG_PREFIX: &str = "azork-oit-";

/// Cheap region used for all OIT resources.
pub const OIT_REGION: &str = "eastus";

/// Hard cost ceiling: no single action may be *estimated* above this.
pub const COST_CAP_USD: f64 = 500.0;

/// Value of the ownership tag key that marks a resource as OIT-owned.
pub const OWNER_TAG_KEY: &str = "owner";
/// Canonical owner value.
pub const OWNER_TAG_VALUE: &str = "azork-oit";
/// Marker tag key/value pair asserting OIT ownership.
pub const OIT_TAG_KEY: &str = "azork-oit";
pub const OIT_TAG_VALUE: &str = "1";
/// TTL tag key (value is an epoch-seconds deadline, informational).
pub const TTL_TAG_KEY: &str = "ttl";

/// Build the canonical OIT tag set. `ttl_epoch` is an epoch-seconds deadline
/// stamped for humans/janitors; it does not itself trigger deletion.
pub fn oit_tags(ttl_epoch: u64) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    m.insert(OIT_TAG_KEY.to_string(), OIT_TAG_VALUE.to_string());
    m.insert(OWNER_TAG_KEY.to_string(), OWNER_TAG_VALUE.to_string());
    m.insert(TTL_TAG_KEY.to_string(), ttl_epoch.to_string());
    m
}

/// Render the canonical tags as `az ... --tags k=v k=v` arguments.
pub fn tag_args(ttl_epoch: u64) -> Vec<String> {
    let mut args = vec!["--tags".to_string()];
    for (k, v) in oit_tags(ttl_epoch) {
        args.push(format!("{k}={v}"));
    }
    args
}

/// Compose an OIT resource-group name from a short suffix, guaranteeing the
/// required prefix. A suffix that already carries the prefix is not doubled.
pub fn oit_rg_name(suffix: &str) -> String {
    let s = suffix.trim().trim_start_matches(OIT_RG_PREFIX);
    format!("{OIT_RG_PREFIX}{s}")
}

/// Whether a resource-group name is one the OIT agent is allowed to manage by
/// *name* (necessary, not sufficient — tags are still checked before mutation).
pub fn is_oit_rg(name: &str) -> bool {
    name.starts_with(OIT_RG_PREFIX)
}

/// The outcome of a cost assessment.
#[derive(Debug, Clone, PartialEq)]
pub enum CostDecision {
    /// Estimated cost is within budget; the action may proceed.
    Approved {
        /// True when the estimate is at/below the "cheap" threshold.
        cheap: bool,
    },
    /// Estimated cost exceeds the cap; the action must not proceed.
    Rejected(String),
}

impl CostDecision {
    /// Convenience: did the assessment approve the action?
    pub fn is_approved(&self) -> bool {
        matches!(self, CostDecision::Approved { .. })
    }
}

/// Anything estimated at or below this monthly cost is considered "cheap" and
/// strongly preferred.
pub const CHEAP_THRESHOLD_USD: f64 = 5.0;

/// Gate a create action on its estimated monthly cost. Rejects anything above
/// [`COST_CAP_USD`]; a negative or NaN estimate is treated as untrusted and
/// rejected rather than assumed free.
pub fn assess_cost(est_monthly_usd: f64) -> CostDecision {
    if est_monthly_usd.is_nan() || est_monthly_usd < 0.0 {
        return CostDecision::Rejected(format!(
            "untrusted cost estimate ({est_monthly_usd}); refusing to proceed"
        ));
    }
    if est_monthly_usd > COST_CAP_USD {
        return CostDecision::Rejected(format!(
            "estimated ${est_monthly_usd:.2}/mo exceeds the ${COST_CAP_USD:.0} cap"
        ));
    }
    CostDecision::Approved {
        cheap: est_monthly_usd <= CHEAP_THRESHOLD_USD,
    }
}

/// Whether a tag map marks a resource as OIT-owned: it must carry *both* the
/// ownership tag and the marker tag with the expected values. Requiring both
/// makes accidental collisions with unrelated resources vanishingly unlikely.
pub fn is_own_resource(tags: &BTreeMap<String, String>) -> bool {
    let owned = tags
        .get(OWNER_TAG_KEY)
        .map(|v| v == OWNER_TAG_VALUE)
        .unwrap_or(false);
    let marked = tags
        .get(OIT_TAG_KEY)
        .map(|v| v == OIT_TAG_VALUE)
        .unwrap_or(false);
    owned && marked
}

/// Gate a destructive/mutating action on ownership. Returns `Err` (blocking the
/// action) for anything the OIT agent did not create.
pub fn guard_mutation(tags: &BTreeMap<String, String>) -> Result<(), String> {
    if is_own_resource(tags) {
        Ok(())
    } else {
        Err(
            "refusing to mutate/delete a resource not owned by azork-oit \
             (missing owner=azork-oit / azork-oit=1 tags)"
                .to_string(),
        )
    }
}

/// A small, curated catalog of cheap/free resource kinds the OIT agent is
/// allowed to create, with conservative monthly-cost estimates (USD).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheapResource {
    /// A resource group — always free.
    ResourceGroup,
    /// A Standard_LRS storage account — effectively free when idle/empty.
    StorageStandardLrs,
}

impl CheapResource {
    /// A conservative upper-bound monthly cost estimate in USD.
    pub fn est_monthly_usd(self) -> f64 {
        match self {
            // Resource groups are containers and never billed.
            CheapResource::ResourceGroup => 0.0,
            // An empty Standard_LRS account bills only for what it stores; an
            // idle, empty account rounds to a few cents. Budget $1 to be safe.
            CheapResource::StorageStandardLrs => 1.0,
        }
    }

    /// Human label for reports.
    pub fn label(self) -> &'static str {
        match self {
            CheapResource::ResourceGroup => "resource group",
            CheapResource::StorageStandardLrs => "storage account (Standard_LRS)",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rg_name_carries_prefix_and_is_not_doubled() {
        assert_eq!(oit_rg_name("nav"), "azork-oit-nav");
        assert_eq!(oit_rg_name("azork-oit-nav"), "azork-oit-nav");
        assert!(is_oit_rg("azork-oit-nav"));
        assert!(!is_oit_rg("prod-rg"));
    }

    #[test]
    fn tags_include_owner_marker_and_ttl() {
        let tags = oit_tags(1234);
        assert_eq!(tags.get("owner").unwrap(), "azork-oit");
        assert_eq!(tags.get("azork-oit").unwrap(), "1");
        assert_eq!(tags.get("ttl").unwrap(), "1234");
        let args = tag_args(1234);
        assert_eq!(args[0], "--tags");
        assert!(args.iter().any(|a| a == "owner=azork-oit"));
        assert!(args.iter().any(|a| a == "azork-oit=1"));
    }

    #[test]
    fn cost_gate_rejects_over_cap_and_untrusted() {
        assert!(assess_cost(0.0).is_approved());
        assert!(matches!(
            assess_cost(0.0),
            CostDecision::Approved { cheap: true }
        ));
        assert!(assess_cost(499.99).is_approved());
        assert!(!assess_cost(500.01).is_approved());
        assert!(!assess_cost(-1.0).is_approved());
        assert!(!assess_cost(f64::NAN).is_approved());
    }

    #[test]
    fn cheap_flag_tracks_threshold() {
        assert!(matches!(
            assess_cost(5.0),
            CostDecision::Approved { cheap: true }
        ));
        assert!(matches!(
            assess_cost(50.0),
            CostDecision::Approved { cheap: false }
        ));
    }

    #[test]
    fn ownership_requires_both_tags() {
        let owned = oit_tags(1);
        assert!(is_own_resource(&owned));
        assert!(guard_mutation(&owned).is_ok());

        let mut partial = BTreeMap::new();
        partial.insert("owner".to_string(), "azork-oit".to_string());
        assert!(!is_own_resource(&partial), "owner alone is insufficient");
        assert!(guard_mutation(&partial).is_err());

        let foreign = BTreeMap::new();
        assert!(!is_own_resource(&foreign));
        assert!(guard_mutation(&foreign).is_err());
    }

    #[test]
    fn catalog_resources_are_within_budget() {
        for r in [
            CheapResource::ResourceGroup,
            CheapResource::StorageStandardLrs,
        ] {
            assert!(assess_cost(r.est_monthly_usd()).is_approved());
        }
    }
}
