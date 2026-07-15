//! Automatic, incremental discovery of `az` capabilities at startup.
//!
//! Historically AzZork only grew its vocabulary when a player typed
//! `learn <group>`. This module lets AzZork learn proactively: at launch it
//! recalls whatever is already cached (see [`super::registry`]), figures out
//! which top-level `az` groups it doesn't know yet, and learns those too —
//! so the vocabulary is richer before the first prompt, and keeps growing
//! across sessions without the player having to ask for it.
//!
//! Everything here reuses the existing learning primitives
//! ([`CapabilityRegistry::learn_group`], [`super::derive::derive_groups`]) —
//! no parsing logic is duplicated between the manual `learn` command and
//! auto-discovery. All `az` access still flows through the single
//! [`AzRunner`] seam, so this is fully testable offline with
//! [`crate::az_runner::FakeAzRunner`].
//!
//! Discovery is deliberately *not* bounded by any fixed timeout or hardcoded
//! group-count cap: it is bounded by the real (finite) list of groups `az`
//! reports, and each individual `az` call already carries
//! [`crate::az_runner::ProcessAzRunner`]'s own hard per-call timeout. To keep
//! startup responsive, the game's real entry point (`main.rs`) runs the
//! discovery pipeline on a background thread that only *computes* results
//! (via [`learn_groups`]) and streams them back over a channel; the single
//! thread that owns the registry/memory applies them
//! ([`apply_learned`]) between prompts, so the first prompt is never
//! blocked waiting on `az`.

use super::derive;
use super::registry::CapabilityRegistry;
use super::Capability;
use crate::az_runner::AzRunner;
use crate::memory::GraphMemory;
use std::path::Path;
use std::sync::mpsc::Receiver;

/// Env var that gates auto-discovery. Any of `0`, `false`, or `no`
/// (case-insensitive) disables it; unset or any other value keeps it on.
pub const AUTODISCOVER_ENV: &str = "AZORK_AUTODISCOVER";

/// Whether startup auto-discovery is enabled, per [`AUTODISCOVER_ENV`].
pub fn autodiscover_enabled() -> bool {
    match std::env::var(AUTODISCOVER_ENV) {
        Ok(v) => !matches!(v.trim().to_lowercase().as_str(), "0" | "false" | "no"),
        Err(_) => true,
    }
}

/// One group's discovery outcome: either freshly-learned capabilities, or an
/// error explaining why that group couldn't be learned (never a panic).
#[derive(Debug, Clone)]
pub struct DiscoveredGroup {
    pub group: String,
    pub result: Result<Vec<Capability>, String>,
}

/// Summary of a completed auto-discovery pass, suitable for a friendly
/// startup message.
#[derive(Debug, Clone, Default)]
pub struct AutoDiscoverReport {
    /// Auto-discovery was skipped entirely (disabled via env var).
    pub disabled: bool,
    /// Groups already known from the cache — left untouched.
    pub already_known: Vec<String>,
    /// Groups that were newly learned this pass.
    pub newly_learned: Vec<String>,
    /// Number of brand-new capabilities added across all groups.
    pub added: usize,
    /// A friendly note when `az` itself was unavailable (top-level listing
    /// failed). Discovery still succeeds overall — it just falls back to
    /// whatever was already cached/built-in.
    pub az_unavailable: Option<String>,
    /// Per-group errors (e.g. one group's `--help` failed) that didn't stop
    /// the rest of the pass.
    pub group_errors: Vec<(String, String)>,
}

impl AutoDiscoverReport {
    /// A short, human-readable status line for startup banners/logs.
    pub fn message(&self) -> String {
        if self.disabled {
            return format!(
                "[capabilities: auto-discovery disabled ({}=0)]",
                AUTODISCOVER_ENV
            );
        }
        if let Some(reason) = &self.az_unavailable {
            return format!(
                "[capabilities: auto-discovery unavailable ({}); using cached/built-in verbs]",
                reason
            );
        }
        if self.newly_learned.is_empty() {
            "[capabilities: nothing new to auto-discover; already up to date]".to_string()
        } else {
            format!(
                "[capabilities: auto-discovered {} new az power(s) across {} group(s): {}]",
                self.added,
                self.newly_learned.len(),
                self.newly_learned.join(", ")
            )
        }
    }
}

/// Figure out which top-level `az` groups aren't in the registry yet.
///
/// Calls `az --help` exactly once via `runner`. Returns an error (never
/// panics) if that listing itself fails — callers should treat that as "az
/// unavailable" and fall back gracefully.
pub fn discover_new_groups(
    registry: &CapabilityRegistry,
    runner: &dyn AzRunner,
) -> Result<Vec<String>, String> {
    discover_new_groups_given_known(&registry.groups(), runner)
}

/// Same as [`discover_new_groups`], but takes an owned snapshot of known
/// group names instead of a live `&CapabilityRegistry` — this is the variant
/// a background discovery thread uses, since it never touches the registry
/// directly (only the thread that owns it does, via [`apply_learned`]).
pub fn discover_new_groups_given_known(
    known: &[String],
    runner: &dyn AzRunner,
) -> Result<Vec<String>, String> {
    let all = derive::derive_groups(runner)?;
    Ok(all.into_iter().filter(|g| !known.contains(g)).collect())
}

/// Drain every message currently sitting in the channel without blocking —
/// used to pull streamed discovery results into the owning thread between
/// prompts, so the background discovery never stalls user interaction.
pub fn drain_available(rx: &Receiver<DiscoveredGroup>) -> Vec<DiscoveredGroup> {
    let mut out = Vec::new();
    while let Ok(dg) = rx.try_recv() {
        out.push(dg);
    }
    out
}

/// Learn every capability of each listed group, one `az <group> --help` call
/// per group. This performs no registry mutation and no I/O beyond the
/// `AzRunner` calls, so it's safe to run on a background thread — the
/// result is streamed back for the owning thread to apply.
pub fn learn_groups(runner: &dyn AzRunner, groups: &[String]) -> Vec<DiscoveredGroup> {
    groups
        .iter()
        .map(|group| DiscoveredGroup {
            group: group.clone(),
            result: derive::derive_group_capabilities(runner, group),
        })
        .collect()
}

/// Fold discovered groups into the registry and mirror them into graph
/// memory, exactly as the manual `learn <group>` command does. Does not
/// persist to disk; callers decide when to save (e.g. once per batch).
pub fn apply_learned(
    registry: &mut CapabilityRegistry,
    memory: &mut GraphMemory,
    discovered: &[DiscoveredGroup],
) -> AutoDiscoverReport {
    let mut report = AutoDiscoverReport::default();
    for dg in discovered {
        match &dg.result {
            Ok(caps) => {
                report.added += registry.extend(caps.iter().cloned());
                report.newly_learned.push(dg.group.clone());
                for cap in registry.iter().filter(|c| c.group == dg.group) {
                    memory.remember_capability(cap);
                }
            }
            Err(e) => report.group_errors.push((dg.group.clone(), e.clone())),
        }
    }
    report
}

/// Run the full startup auto-discovery pass synchronously: check the escape
/// hatch, recall what's already cached, discover only what's missing, learn
/// it, mirror it into memory, and persist once. Never panics — `az` being
/// unavailable degrades to a friendly report rather than an error.
///
/// This is the function the real startup path (background thread in
/// `main.rs`) and tests both drive: tests call it directly (synchronously)
/// against a `FakeAzRunner`, exercising exactly the same logic that runs at
/// real launch.
pub fn run_startup_autodiscovery(
    registry: &mut CapabilityRegistry,
    memory: &mut GraphMemory,
    runner: &dyn AzRunner,
    cache_path: &Path,
) -> AutoDiscoverReport {
    if !autodiscover_enabled() {
        return AutoDiscoverReport {
            disabled: true,
            already_known: registry.groups(),
            ..Default::default()
        };
    }

    let already_known = registry.groups();
    let missing = match discover_new_groups(registry, runner) {
        Ok(groups) => groups,
        Err(e) => {
            return AutoDiscoverReport {
                already_known,
                az_unavailable: Some(e),
                ..Default::default()
            };
        }
    };

    if missing.is_empty() {
        return AutoDiscoverReport {
            already_known,
            ..Default::default()
        };
    }

    let discovered = learn_groups(runner, &missing);
    let mut report = apply_learned(registry, memory, &discovered);
    report.already_known = already_known;
    if report.added > 0 {
        if let Err(e) = registry.save(cache_path) {
            report.group_errors.push(("(cache save)".to_string(), e));
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::az_runner::FakeAzRunner;
    use std::sync::Mutex;

    // `std::env` is process-global; Rust test binaries run tests in parallel
    // threads, so any test that touches AZORK_AUTODISCOVER must serialize
    // against the others to avoid cross-test races.
    static ENV_GUARD: Mutex<()> = Mutex::new(());

    const ROOT_HELP: &str = r#"
Group
    az : Manage Azure resources.

Subgroups:
    group   : Manage resource groups.
    vm      : Manage virtual machines.
    storage : Manage storage accounts.
"#;

    const GROUP_HELP: &str = r#"
Group
    az group : Manage resource groups.

Commands:
    create : Create a resource group.
    list   : List resource groups.
"#;

    const VM_HELP: &str = r#"
Group
    az vm : Manage virtual machines.

Commands:
    create : Create a VM.
    list   : List VMs.
"#;

    const STORAGE_HELP: &str = r#"
Group
    az storage : Manage storage accounts.

Commands:
    create : Create a storage account.
"#;

    fn full_runner() -> FakeAzRunner {
        FakeAzRunner::new()
            .with(&["--help"], ROOT_HELP)
            .with(&["group", "--help"], GROUP_HELP)
            .with(&["vm", "--help"], VM_HELP)
            .with(&["storage", "--help"], STORAGE_HELP)
    }

    /// Guard so tests that toggle AZORK_AUTODISCOVER don't leak state across
    /// the (parallel) test binary — tests run in the same process.
    fn with_env<T>(key: &str, val: Option<&str>, f: impl FnOnce() -> T) -> T {
        let _guard = ENV_GUARD.lock().unwrap_or_else(|p| p.into_inner());
        let prev = std::env::var(key).ok();
        match val {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
        let result = f();
        match prev {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
        result
    }

    #[test]
    fn cold_start_discovers_everything_without_manual_learn() {
        with_env(AUTODISCOVER_ENV, None, || {
            let runner = full_runner();
            let mut registry = CapabilityRegistry::new();
            let mut memory = GraphMemory::new();
            let cache_dir =
                std::env::temp_dir().join(format!("azork-test-cold-{}", std::process::id()));
            let cache_path = cache_dir.join("capabilities.tsv");

            let report =
                run_startup_autodiscovery(&mut registry, &mut memory, &runner, &cache_path);

            assert!(!report.disabled);
            assert!(report.az_unavailable.is_none());
            assert_eq!(report.newly_learned.len(), 3);
            assert!(registry.get("group create").is_some());
            assert!(registry.get("vm list").is_some());
            assert!(registry.get("storage create").is_some());
            assert!(registry.len() >= 5);
            // The manual `learn` handler was never invoked — only the shared
            // discovery pipeline — yet the registry is fully populated.
            let _ = std::fs::remove_dir_all(&cache_dir);
        });
    }

    #[test]
    fn warm_start_skips_already_known_groups() {
        with_env(AUTODISCOVER_ENV, None, || {
            let runner = full_runner();
            let mut registry = CapabilityRegistry::new();
            let mut memory = GraphMemory::new();
            // Pre-seed the cache: `group` is already known.
            registry.insert(Capability::new(
                "group",
                "create",
                "Create a resource group.",
                None,
            ));
            registry.insert(Capability::new(
                "group",
                "list",
                "List resource groups.",
                None,
            ));

            let missing = discover_new_groups(&registry, &runner).expect("discover ok");
            assert_eq!(missing, vec!["vm".to_string(), "storage".to_string()]);

            let cache_dir =
                std::env::temp_dir().join(format!("azork-test-warm-{}", std::process::id()));
            let cache_path = cache_dir.join("capabilities.tsv");
            let report =
                run_startup_autodiscovery(&mut registry, &mut memory, &runner, &cache_path);

            // Only the missing groups were (re)learned; `group` was left alone.
            assert_eq!(
                report.newly_learned,
                vec!["vm".to_string(), "storage".to_string()]
            );
            assert!(!report.newly_learned.contains(&"group".to_string()));
            assert!(registry.get("vm create").is_some());
            assert!(registry.get("storage create").is_some());
            let _ = std::fs::remove_dir_all(&cache_dir);
        });
    }

    #[test]
    fn az_unavailable_falls_back_gracefully_without_panicking() {
        with_env(AUTODISCOVER_ENV, None, || {
            let runner = FakeAzRunner::new().with_failure(&["--help"], "az: command not found");
            let mut registry = CapabilityRegistry::new();
            registry.insert(Capability::new(
                "group",
                "list",
                "List resource groups.",
                None,
            ));
            let mut memory = GraphMemory::new();
            let cache_dir =
                std::env::temp_dir().join(format!("azork-test-unavailable-{}", std::process::id()));
            let cache_path = cache_dir.join("capabilities.tsv");

            let report =
                run_startup_autodiscovery(&mut registry, &mut memory, &runner, &cache_path);

            assert!(!report.disabled);
            assert!(report.az_unavailable.is_some());
            assert!(report.message().contains("unavailable"));
            // Fell back to the cache: the pre-seeded capability survives.
            assert!(registry.get("group list").is_some());
            assert_eq!(registry.len(), 1);
            let _ = std::fs::remove_dir_all(&cache_dir);
        });
    }

    #[test]
    fn disabled_via_env_var_issues_no_discovery_calls() {
        with_env(AUTODISCOVER_ENV, Some("0"), || {
            // A runner with no canned responses at all: if discovery made any
            // call, `derive_groups` would get the "no canned response"
            // failure text rather than a clean skip.
            let runner = FakeAzRunner::new();
            let mut registry = CapabilityRegistry::new();
            let mut memory = GraphMemory::new();
            let cache_path = std::env::temp_dir().join(format!(
                "azork-test-disabled-{}/capabilities.tsv",
                std::process::id()
            ));

            let report =
                run_startup_autodiscovery(&mut registry, &mut memory, &runner, &cache_path);

            assert!(report.disabled);
            assert!(registry.is_empty());
        });
    }

    #[test]
    fn autodiscover_enabled_respects_common_falsey_values() {
        with_env(AUTODISCOVER_ENV, Some("false"), || {
            assert!(!autodiscover_enabled());
        });
        with_env(AUTODISCOVER_ENV, Some("No"), || {
            assert!(!autodiscover_enabled());
        });
        with_env(AUTODISCOVER_ENV, Some("1"), || {
            assert!(autodiscover_enabled());
        });
        with_env(AUTODISCOVER_ENV, None, || {
            assert!(autodiscover_enabled());
        });
    }

    #[test]
    fn learn_groups_streams_per_group_results_without_touching_registry() {
        let runner = full_runner();
        let discovered = learn_groups(&runner, &["group".to_string(), "vm".to_string()]);
        assert_eq!(discovered.len(), 2);
        assert!(discovered[0].result.is_ok());
        assert!(discovered[1].result.is_ok());
    }

    #[test]
    fn apply_learned_mirrors_into_memory() {
        let mut registry = CapabilityRegistry::new();
        let mut memory = GraphMemory::new();
        let discovered = vec![DiscoveredGroup {
            group: "group".to_string(),
            result: Ok(vec![Capability::new(
                "group",
                "create",
                "Create a resource group.",
                None,
            )]),
        }];
        let report = apply_learned(&mut registry, &mut memory, &discovered);
        assert_eq!(report.added, 1);
        assert!(registry.get("group create").is_some());
        assert!(!memory.is_empty());
    }
}
