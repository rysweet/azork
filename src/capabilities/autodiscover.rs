//! Automatic, incremental startup discovery of `az` command groups.
//!
//! Historically AzZork only learned new `az` verbs when a player typed
//! `learn <group>`. This module lets AzZork learn proactively: at launch it
//! figures out which top-level groups the real `az` CLI reports that are
//! *not yet* in the (cache-recalled) [`CapabilityRegistry`], and learns each
//! of those — so the vocabulary is already rich before the first prompt, and
//! keeps growing across sessions without the player asking for it.
//!
//! Nothing here duplicates the help-text parser: every function is a thin
//! wrapper around [`crate::capabilities::derive::derive_groups`] and
//! [`crate::capabilities::derive::derive_group_capabilities`] — the exact
//! same seam the manual `learn <group>` command already uses via
//! [`CapabilityRegistry::learn_group`]. That keeps "what a group's commands
//! are" defined in exactly one place.
//!
//! The functions here are deliberately pure and synchronous so they can be
//! driven directly, deterministically, and offline in tests (via
//! [`crate::az_runner::FakeAzRunner`]) with no thread, channel, or timing
//! involved. The one exception is [`stream_startup_autodiscovery`], a thin
//! streaming variant meant to be called from a background thread; it reuses
//! the same [`discover_new_groups`] / [`learn_groups`] building blocks and
//! adds only cancellation and incremental delivery over a channel.

use super::derive;
use super::{Capability, CapabilityRegistry};
use crate::az_runner::AzRunner;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;

/// Environment variable that disables startup auto-discovery when set to
/// `0`, `false`, or `no` (case-insensitive). Any other value (or unset)
/// leaves auto-discovery enabled. This is the escape hatch for offline/CI
/// contexts where shelling out to `az` is undesirable even though it would
/// otherwise fail gracefully.
pub const AUTODISCOVER_ENV: &str = "AZORK_AUTODISCOVER";

/// Whether startup auto-discovery is enabled, per [`AUTODISCOVER_ENV`].
pub fn autodiscover_enabled() -> bool {
    match std::env::var(AUTODISCOVER_ENV) {
        Ok(v) => !matches!(v.trim().to_lowercase().as_str(), "0" | "false" | "no"),
        Err(_) => true,
    }
}

/// The outcome of attempting to learn one group's capabilities.
#[derive(Debug)]
pub struct GroupResult {
    /// The `az` group attempted (e.g. `"storage"`). Empty when the failure
    /// happened before any specific group was identified (e.g. `az --help`
    /// itself failed).
    pub group: String,
    /// The capabilities discovered, or a friendly error string.
    pub outcome: Result<Vec<Capability>, String>,
}

/// The outcome of applying one [`GroupResult`] into a [`CapabilityRegistry`].
#[derive(Debug)]
pub struct AppliedGroup {
    /// The `az` group this applies to.
    pub group: String,
    /// Number of newly-added capabilities, or the propagated error.
    pub result: Result<usize, String>,
}

/// Enumerate the top-level `az` groups and return the ones *not* already
/// present in `known_groups` (e.g. `registry.groups()` from a warm cache).
///
/// This is how warm starts stay cheap: a group already learned in a prior
/// session is skipped entirely rather than being re-derived from scratch.
pub fn discover_new_groups(
    runner: &dyn AzRunner,
    known_groups: &[String],
) -> Result<Vec<String>, String> {
    let all = derive::derive_groups(runner)?;
    Ok(all
        .into_iter()
        .filter(|g| !known_groups.iter().any(|k| k == g))
        .collect())
}

/// Learn every command of each of `groups` via `az <group> --help`,
/// returning one [`GroupResult`] per group in the order given.
///
/// This reuses [`derive::derive_group_capabilities`] directly — the same
/// parsing [`CapabilityRegistry::learn_group`] wraps for the manual `learn`
/// command — so there is exactly one place that understands `az` help text.
pub fn learn_groups(runner: &dyn AzRunner, groups: &[String]) -> Vec<GroupResult> {
    groups
        .iter()
        .map(|group| GroupResult {
            group: group.clone(),
            outcome: derive::derive_group_capabilities(runner, group),
        })
        .collect()
}

/// Run startup auto-discovery end-to-end, synchronously: discover which
/// groups are missing from `known_groups`, then learn each of them.
///
/// This is intentionally synchronous and side-effect-free with respect to
/// any registry — callers decide how (and whether concurrently) to apply the
/// results via [`apply_learned`]. That makes it directly, deterministically
/// testable with a [`crate::az_runner::FakeAzRunner`] and no thread involved.
/// Its work is bounded by the real, finite set of groups `az --help`
/// reports (no artificial cap), and each underlying call already carries
/// [`ProcessAzRunner`](crate::az_runner::ProcessAzRunner)'s own wall-clock
/// timeout.
pub fn run_startup_autodiscovery(
    runner: &dyn AzRunner,
    known_groups: &[String],
) -> Vec<GroupResult> {
    match discover_new_groups(runner, known_groups) {
        Ok(missing) => learn_groups(runner, &missing),
        Err(e) => vec![GroupResult {
            group: String::new(),
            outcome: Err(e),
        }],
    }
}

/// Fold a batch of [`GroupResult`]s into `registry`, returning one
/// [`AppliedGroup`] per input result (added-count on success, the original
/// error otherwise) so callers can report per-group outcomes.
pub fn apply_learned(
    registry: &mut CapabilityRegistry,
    results: impl IntoIterator<Item = GroupResult>,
) -> Vec<AppliedGroup> {
    results
        .into_iter()
        .map(|r| {
            let group = r.group;
            match r.outcome {
                Ok(caps) => AppliedGroup {
                    result: Ok(registry.extend(caps)),
                    group,
                },
                Err(e) => AppliedGroup {
                    result: Err(e),
                    group,
                },
            }
        })
        .collect()
}

/// Streaming variant of [`run_startup_autodiscovery`] intended for a
/// background thread: discovers missing groups, then learns them one at a
/// time, sending each [`GroupResult`] over `tx` as soon as it is ready so a
/// caller (the main thread) can apply capabilities incrementally rather than
/// waiting for every group to finish.
///
/// Checks `cancel` before starting and between each group so a caller can
/// stop further (not-yet-started) discovery once the player begins
/// interacting, without needing to interrupt an in-flight `az` call. Also
/// stops early if the receiving end has gone away (e.g. the game already
/// exited).
pub fn stream_startup_autodiscovery(
    runner: &dyn AzRunner,
    known_groups: &[String],
    cancel: &AtomicBool,
    tx: &Sender<GroupResult>,
) {
    if cancel.load(Ordering::Relaxed) {
        return;
    }
    let missing = match discover_new_groups(runner, known_groups) {
        Ok(groups) => groups,
        Err(e) => {
            let _ = tx.send(GroupResult {
                group: String::new(),
                outcome: Err(e),
            });
            return;
        }
    };
    for group in missing {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let outcome = derive::derive_group_capabilities(runner, &group);
        if tx.send(GroupResult { group, outcome }).is_err() {
            // Receiver dropped: nobody is listening any more, stop working.
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::az_runner::FakeAzRunner;

    const ROOT_HELP: &str = "\nGroup\n    az\n\nSubgroups:\n    group   : Manage resource groups.\n    storage : Manage Azure Cloud Storage resources.\n\nCommands:\n    login : Log in to Azure.\n";
    const GROUP_HELP: &str =
        "\nCommands:\n    create : Create a new resource group.\n    list   : List resource groups.\n";
    const STORAGE_HELP: &str =
        "\nCommands:\n    create : Create a storage account.\n    list   : List storage accounts.\n";

    fn runner_with_root_and_groups() -> FakeAzRunner {
        FakeAzRunner::new()
            .with(&["--help"], ROOT_HELP)
            .with(&["group", "--help"], GROUP_HELP)
            .with(&["storage", "--help"], STORAGE_HELP)
    }

    #[test]
    fn discover_new_groups_skips_known() {
        let runner = runner_with_root_and_groups();
        let known = vec!["group".to_string()];
        let missing = discover_new_groups(&runner, &known).unwrap();
        assert_eq!(missing, vec!["storage".to_string()]);
    }

    #[test]
    fn run_startup_autodiscovery_learns_missing_groups_without_manual_learn() {
        let runner = runner_with_root_and_groups();
        let mut registry = CapabilityRegistry::new();
        assert!(registry.is_empty());

        // No `learn <group>` call anywhere here — this is the automatic path.
        let results = run_startup_autodiscovery(&runner, &registry.groups());
        let applied = apply_learned(&mut registry, results);

        assert!(!registry.is_empty());
        assert!(registry.get("group create").is_some());
        assert!(registry.get("storage list").is_some());
        assert_eq!(applied.len(), 2);
        assert!(applied.iter().all(|a| matches!(a.result, Ok(n) if n > 0)));
    }

    #[test]
    fn warm_start_skips_already_known_groups() {
        let runner = runner_with_root_and_groups();
        let mut registry = CapabilityRegistry::new();
        // Pre-seed the cache/registry as if 'group' was learned last session.
        registry.insert(Capability::new(
            "group",
            "create",
            "Create a new resource group.",
            None,
        ));

        let known_before = registry.groups();
        assert_eq!(known_before, vec!["group".to_string()]);

        let results = run_startup_autodiscovery(&runner, &known_before);
        // Only 'storage' should have been (re)discovered; 'group' is skipped.
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].group, "storage");

        let applied = apply_learned(&mut registry, results);
        assert_eq!(applied.len(), 1);
        assert!(registry.get("storage list").is_some());
        // The pre-seeded capability is untouched and still present.
        assert!(registry.get("group create").is_some());
    }

    #[test]
    fn az_unavailable_does_not_panic_and_yields_error_result() {
        // Simulate `az` missing/unreachable: every call fails.
        let runner = FakeAzRunner::new().with_failure(&["--help"], "az: command not found");
        let mut registry = CapabilityRegistry::new();

        let results = run_startup_autodiscovery(&runner, &registry.groups());
        assert_eq!(results.len(), 1);
        assert!(results[0].outcome.is_err());

        let applied = apply_learned(&mut registry, results);
        assert_eq!(applied.len(), 1);
        assert!(applied[0].result.is_err());
        // Registry stays empty but usable — no panic, no partial corruption.
        assert!(registry.is_empty());
    }

    #[test]
    fn stream_variant_delivers_each_group_incrementally() {
        let runner = runner_with_root_and_groups();
        let (tx, rx) = std::sync::mpsc::channel();
        let cancel = AtomicBool::new(false);

        stream_startup_autodiscovery(&runner, &[], &cancel, &tx);
        drop(tx);

        let received: Vec<GroupResult> = rx.into_iter().collect();
        assert_eq!(received.len(), 2);
        let groups: Vec<&str> = received.iter().map(|r| r.group.as_str()).collect();
        assert!(groups.contains(&"group"));
        assert!(groups.contains(&"storage"));
    }

    #[test]
    fn stream_variant_respects_pre_set_cancellation() {
        let runner = runner_with_root_and_groups();
        let (tx, rx) = std::sync::mpsc::channel();
        let cancel = AtomicBool::new(true); // already cancelled before starting

        stream_startup_autodiscovery(&runner, &[], &cancel, &tx);
        drop(tx);

        let received: Vec<GroupResult> = rx.into_iter().collect();
        assert!(received.is_empty());
    }

    #[test]
    fn autodiscover_enabled_respects_env_var() {
        // This test mutates process env; run assertions immediately after
        // each set to avoid interference from parallel tests touching the
        // same var (the crate's other tests do not touch this var).
        std::env::set_var(AUTODISCOVER_ENV, "0");
        assert!(!autodiscover_enabled());
        std::env::set_var(AUTODISCOVER_ENV, "false");
        assert!(!autodiscover_enabled());
        std::env::set_var(AUTODISCOVER_ENV, "NO");
        assert!(!autodiscover_enabled());
        std::env::set_var(AUTODISCOVER_ENV, "1");
        assert!(autodiscover_enabled());
        std::env::remove_var(AUTODISCOVER_ENV);
        assert!(autodiscover_enabled());
    }
}
