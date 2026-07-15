//! tests/evolution_tests.rs
//!
//! End-to-end proof of AzZork's *self-evolution*: capabilities are DERIVED from
//! the `az` CLI at runtime and surfaced adaptively, with **no per-command code
//! edits**. Every test injects a `FakeAzRunner` (canned `az` help output) and a
//! temp cache dir, so the suite never calls the real `az` binary or the network.

use azork::agent::{IntentResolver, MockAdapter, Resolution};
use azork::az_runner::{AzRunner, FakeAzRunner};
use azork::backend::{az::AzBackend, Backend};
use azork::capabilities::CapabilityRegistry;
use std::path::PathBuf;

/// A unique temp path for a test's capability cache.
fn temp_cache(tag: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!(
        "azork-evo-{}-{}-{}",
        tag,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    p.push("capabilities.tsv");
    p
}

/// The `az group --help` help block, verbatim in shape from the real CLI.
const GROUP_HELP: &str = "\nGroup\n    az group : Manage resource groups and template deployments.\n\nSubgroups:\n    lock   : Manage Azure resource group locks.\n\nCommands:\n    create : Create a new resource group.\n    delete : Delete a resource group.\n    list   : List resource groups.\n";

#[test]
fn derives_new_capability_with_no_code_edit() {
    // A brand-new az command the game has never been told about:
    let help = format!(
        "{}    teleport : Instantly relocate a resource group across regions.\n",
        GROUP_HELP
    );
    let runner = FakeAzRunner::new().with(&["group", "--help"], &help);

    let mut reg = CapabilityRegistry::new();
    let added = reg.learn_group(&runner, "group").unwrap();
    assert!(added >= 4);

    // The invented verb is now known, purely from parsing `az` output.
    let cap = reg
        .get("group teleport")
        .expect("newly-derived verb must be in the registry");
    assert_eq!(cap.verb, "teleport");
    assert!(cap.summary.contains("Instantly relocate"));

    // ...and it appears in the runtime help, with zero hand-mapping.
    assert!(reg.help_text().contains("teleport"));
    assert!(reg.help_text().contains("az group teleport"));
}

#[test]
fn learned_capabilities_persist_across_sessions() {
    let cache = temp_cache("persist");
    let runner = FakeAzRunner::new().with(&["group", "--help"], GROUP_HELP);

    // Session 1: learn and persist.
    {
        let mut reg = CapabilityRegistry::load(&cache);
        assert!(reg.is_empty(), "fresh cache starts empty");
        reg.learn_group(&runner, "group").unwrap();
        reg.save(&cache).unwrap();
    }

    // Session 2: a NEW registry recalls what the previous one learned.
    {
        let reg = CapabilityRegistry::load(&cache);
        assert!(!reg.is_empty(), "second session must recall learned powers");
        assert!(reg.get("group create").is_some());
        assert!(reg.get("group list").is_some());
    }

    let _ = std::fs::remove_dir_all(cache.parent().unwrap());
}

#[test]
fn unknown_intent_is_resolved_not_rejected() {
    let runner = FakeAzRunner::new().with(&["group", "--help"], GROUP_HELP);
    let mut reg = CapabilityRegistry::new();
    reg.learn_group(&runner, "group").unwrap();

    let resolver = IntentResolver::new(MockAdapter::new(), &reg);

    // Free-text intent that matches a learned verb resolves to it.
    match resolver.resolve("please list everything") {
        Resolution::Verb(c) => assert_eq!(c.verb, "list"),
        other => panic!("expected a resolved verb, got {:?}", other),
    }

    // Something with no relation still never hard-fails; it narrates guidance.
    let narration = resolver.resolve("xyzzy plugh").narrate();
    assert!(!narration.is_empty());
}

#[test]
fn az_backend_builds_world_from_injected_runner_offline() {
    // Drive the real AzBackend with canned `az` output — no network, no creds.
    let runner = FakeAzRunner::new()
        .with(
            &["account", "show", "--query", "name", "-o", "tsv"],
            "mock-sub\n",
        )
        .with(
            &[
                "group",
                "list",
                "--query",
                "[].{name:name,location:location}",
                "-o",
                "tsv",
            ],
            "alpha-rg\teastus\nbeta-rg\twestus2\n",
        )
        .with(
            &[
                "resource",
                "list",
                "-g",
                "alpha-rg",
                "--query",
                "[].{name:name,type:type}",
                "-o",
                "tsv",
            ],
            "store1\tMicrosoft.Storage/storageAccounts\n",
        )
        .with(
            &[
                "resource",
                "list",
                "-g",
                "beta-rg",
                "--query",
                "[].{name:name,type:type}",
                "-o",
                "tsv",
            ],
            "",
        );

    let backend = AzBackend::with_runner(Box::new(runner));
    let world = backend.build_world().expect("world builds from canned az");
    assert_eq!(world.subscription, "mock-sub");
    // Two resource groups became two rooms.
    assert_eq!(world.current_room().name, "alpha-rg");
}

#[test]
fn learn_gracefully_reports_when_az_unavailable() {
    // No canned response => the fake behaves like a failing `az` invocation.
    let runner = FakeAzRunner::new();
    let mut reg = CapabilityRegistry::new();
    let result = reg.learn_group(&runner, "storage");
    assert!(
        result.is_err(),
        "missing az must surface an error, not panic"
    );
    assert!(reg.is_empty());
}

#[test]
fn discovering_groups_lists_top_level_command_groups() {
    let root_help = "\nGroup\n    az\n\nSubgroups:\n    group   : Manage resource groups.\n    storage : Manage Azure Cloud Storage resources.\n    vm      : Manage Linux or Windows virtual machines.\n";
    let runner: Box<dyn AzRunner> = Box::new(FakeAzRunner::new().with(&["--help"], root_help));
    let reg = CapabilityRegistry::new();
    let groups = reg.discover_groups(runner.as_ref()).unwrap();
    assert!(groups.contains(&"storage".to_string()));
    assert!(groups.contains(&"vm".to_string()));
}
