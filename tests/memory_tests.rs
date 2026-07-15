//! tests/memory_tests.rs
//!
//! TDD contract for AzZork's **graph memory** — the persistent, ladybug-style
//! cognitive memory that lets the game *evolve as it is used* (mirroring the
//! `CognitiveMemoryOps` / `MemoryKind` / `RecallWeightSet` pattern of a typed,
//! ranked-recall memory model).
//!
//! These tests pin the *offline, in-memory* behaviour that the default build and
//! CI always exercise: the store is deterministic (a monotonic tick drives
//! recency, never wall-clock time) and never touches `az`, the network, or the
//! native `lbug` backend (which is gated behind the opt-in `persistent` feature).
//!
//! The memory records four things the mission calls out:
//!   * discovered `az` capabilities,
//!   * the resource graph — resource groups are *rooms*, resources are *objects*,
//!     and relationships are *edges*,
//!   * user intents seen,
//!   * friction notes.
//!
//! …and recalls them, ranked, to inform help, navigation, and intent resolution.

use azork::memory::{GraphMemory, MemoryKind, RecallWeights};

// ---- Storing & basic retrieval -----------------------------------------

#[test]
fn fresh_memory_is_empty() {
    let mem = GraphMemory::new();
    assert!(mem.is_empty());
    assert_eq!(mem.len(), 0);
}

#[test]
fn remember_returns_stable_id_and_is_retrievable() {
    let mut mem = GraphMemory::new();
    let id = mem.remember(
        MemoryKind::Capability,
        "group create",
        "az group create — Create a new resource group.",
        &["capability", "group"],
        0.8,
    );
    assert!(!id.is_empty());
    assert_eq!(mem.len(), 1);
    let node = mem.get(&id).expect("stored node must be retrievable by id");
    assert_eq!(node.kind, MemoryKind::Capability);
    assert_eq!(node.label, "group create");
    assert!(node.content.contains("resource group"));
    assert!(node.tags.iter().any(|t| t == "group"));
}

#[test]
fn nodes_can_be_filtered_by_kind() {
    let mut mem = GraphMemory::new();
    mem.remember(
        MemoryKind::Room,
        "alpha-rg",
        "Resource group in eastus.",
        &[],
        0.5,
    );
    mem.remember(
        MemoryKind::Room,
        "beta-rg",
        "Resource group in westus2.",
        &[],
        0.5,
    );
    mem.remember(
        MemoryKind::Resource,
        "store1",
        "A storage account.",
        &[],
        0.5,
    );

    assert_eq!(mem.nodes_of_kind(MemoryKind::Room).len(), 2);
    assert_eq!(mem.nodes_of_kind(MemoryKind::Resource).len(), 1);
    assert_eq!(mem.nodes_of_kind(MemoryKind::Friction).len(), 0);
}

// ---- The resource graph: rooms, objects, edges -------------------------

#[test]
fn edges_connect_rooms_to_their_resources() {
    let mut mem = GraphMemory::new();
    let rg = mem.remember(MemoryKind::Room, "alpha-rg", "A room.", &[], 0.5);
    let store = mem.remember(
        MemoryKind::Resource,
        "store1",
        "A storage account.",
        &[],
        0.5,
    );
    let vm = mem.remember(MemoryKind::Resource, "vm1", "A virtual machine.", &[], 0.5);

    mem.add_edge(&rg, "contains", &store).unwrap();
    mem.add_edge(&rg, "contains", &vm).unwrap();

    // Traversing the room's `contains` edges yields both objects.
    let contained = mem.neighbors(&rg, Some("contains"));
    let labels: Vec<&str> = contained.iter().map(|n| n.label.as_str()).collect();
    assert_eq!(contained.len(), 2);
    assert!(labels.contains(&"store1"));
    assert!(labels.contains(&"vm1"));

    // Unfiltered neighbours return every relation.
    assert_eq!(mem.neighbors(&rg, None).len(), 2);
}

#[test]
fn edge_to_unknown_node_is_rejected() {
    let mut mem = GraphMemory::new();
    let rg = mem.remember(MemoryKind::Room, "alpha-rg", "A room.", &[], 0.5);
    assert!(mem.add_edge(&rg, "contains", "does-not-exist").is_err());
    assert!(mem.add_edge("nope", "contains", &rg).is_err());
}

// ---- Ranked recall -----------------------------------------------------

#[test]
fn recall_finds_by_text_relevance() {
    let mut mem = GraphMemory::new();
    mem.remember(
        MemoryKind::Capability,
        "group create",
        "Create a new resource group.",
        &[],
        0.5,
    );
    mem.remember(
        MemoryKind::Capability,
        "storage account create",
        "Create a storage account.",
        &[],
        0.5,
    );
    mem.remember(
        MemoryKind::Capability,
        "vm list",
        "List virtual machines.",
        &[],
        0.5,
    );

    let hits = mem.recall("storage", None, 5);
    assert!(!hits.is_empty());
    assert_eq!(hits[0].label, "storage account create");
}

#[test]
fn recall_can_be_scoped_to_a_kind() {
    let mut mem = GraphMemory::new();
    mem.remember(
        MemoryKind::Capability,
        "vm create",
        "Create a virtual machine.",
        &[],
        0.5,
    );
    mem.remember(
        MemoryKind::Resource,
        "myvm",
        "A virtual machine object.",
        &[],
        0.5,
    );

    let caps = mem.recall_kind("virtual machine", MemoryKind::Capability, 5);
    assert!(caps.iter().all(|n| n.kind == MemoryKind::Capability));
    assert!(caps.iter().any(|n| n.label == "vm create"));
}

#[test]
fn recall_respects_limit() {
    let mut mem = GraphMemory::new();
    for i in 0..10 {
        mem.remember(
            MemoryKind::Capability,
            &format!("group verb{i}"),
            "Manage a resource group.",
            &[],
            0.5,
        );
    }
    assert_eq!(mem.recall("resource group", None, 3).len(), 3);
}

#[test]
fn recall_ranks_more_important_facts_higher_when_text_ties() {
    let mut mem = GraphMemory::new();
    // Identical text; only importance differs.
    mem.remember(MemoryKind::Capability, "a", "deploy the template", &[], 0.1);
    let important = mem.remember(MemoryKind::Capability, "b", "deploy the template", &[], 0.9);

    let hits = mem.recall_ranked("deploy template", None, 5, RecallWeights::default());
    assert_eq!(
        hits.first().map(|n| n.id.as_str()),
        Some(important.as_str())
    );
}

#[test]
fn recall_ranks_more_recent_facts_higher_when_other_signals_tie() {
    let mut mem = GraphMemory::new();
    // Same text + importance; the SECOND insert is more recent (higher tick).
    mem.remember(MemoryKind::Intent, "old", "lock the vault", &[], 0.5);
    let newer = mem.remember(MemoryKind::Intent, "new", "lock the vault", &[], 0.5);

    let weights = RecallWeights {
        recency: 5.0,
        ..RecallWeights::default()
    };
    let hits = mem.recall_ranked("lock vault", None, 5, weights);
    assert_eq!(hits.first().map(|n| n.id.as_str()), Some(newer.as_str()));
}

#[test]
fn recall_is_deterministic_across_runs() {
    let build = || {
        let mut mem = GraphMemory::new();
        mem.remember(
            MemoryKind::Capability,
            "group create",
            "Create a resource group.",
            &[],
            0.5,
        );
        mem.remember(
            MemoryKind::Capability,
            "group delete",
            "Delete a resource group.",
            &[],
            0.5,
        );
        mem.remember(
            MemoryKind::Capability,
            "group list",
            "List resource groups.",
            &[],
            0.5,
        );
        mem
    };
    let a: Vec<String> = build()
        .recall("resource group", None, 5)
        .iter()
        .map(|n| n.id.clone())
        .collect();
    let b: Vec<String> = build()
        .recall("resource group", None, 5)
        .iter()
        .map(|n| n.id.clone())
        .collect();
    assert_eq!(a, b, "recall ordering must be deterministic");
}

// ---- Reinforcement (usage/recency feedback loop) -----------------------

#[test]
fn reinforced_recall_bumps_usage_and_lifts_rank() {
    let mut mem = GraphMemory::new();
    mem.remember(
        MemoryKind::Capability,
        "vm start",
        "Start a virtual machine.",
        &[],
        0.5,
    );
    let target = mem.remember(
        MemoryKind::Capability,
        "vm stop",
        "Stop a virtual machine.",
        &[],
        0.5,
    );

    // Reinforce the target several times: usage should climb.
    for _ in 0..5 {
        mem.reinforce(&target);
    }
    assert!(mem.get(&target).unwrap().usage_count >= 5);

    // With usage weighted heavily, the reinforced node wins a tie-ish query.
    let weights = RecallWeights {
        usage: 10.0,
        ..RecallWeights::default()
    };
    let hits = mem.recall_ranked("virtual machine", None, 5, weights);
    assert_eq!(hits.first().map(|n| n.id.as_str()), Some(target.as_str()));
}

// ---- Convenience recorders for the mission's four data kinds -----------

#[test]
fn friction_notes_are_recorded_and_recallable() {
    let mut mem = GraphMemory::new();
    mem.record_friction(
        "`examine` gave a confusing error on empty groups",
        &["ux", "examine"],
    );
    let hits = mem.recall_kind("examine error", MemoryKind::Friction, 5);
    assert!(hits.iter().any(|n| n.content.contains("confusing error")));
}

#[test]
fn intents_are_recorded_for_later_recall() {
    let mut mem = GraphMemory::new();
    mem.record_intent("please lock the production storage account");
    assert_eq!(mem.nodes_of_kind(MemoryKind::Intent).len(), 1);
    let hits = mem.recall_kind("lock storage", MemoryKind::Intent, 5);
    assert!(!hits.is_empty());
}

// ---- Dependency-free persistence (evolves across sessions) -------------

#[test]
fn memory_round_trips_through_disk() {
    let dir = std::env::temp_dir().join(format!(
        "azork-mem-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let path = dir.join("memory.graph");

    // Session 1: build a small graph and persist it.
    let rg;
    let store;
    {
        let mut mem = GraphMemory::new();
        rg = mem.remember(
            MemoryKind::Room,
            "alpha-rg",
            "A room in\teastus.",
            &["room"],
            0.6,
        );
        store = mem.remember(
            MemoryKind::Resource,
            "store1",
            "A storage account.",
            &["object"],
            0.7,
        );
        mem.add_edge(&rg, "contains", &store).unwrap();
        mem.reinforce(&store);
        mem.save(&path).unwrap();
    }

    // Session 2: a NEW memory recalls what the previous one learned.
    {
        let mem = GraphMemory::load(&path);
        assert_eq!(mem.len(), 2, "both nodes survive the round-trip");
        let loaded_store = mem
            .get(&store)
            .expect("resource id is stable across save/load");
        assert_eq!(loaded_store.label, "store1");
        assert!(loaded_store.usage_count >= 1, "usage is preserved");
        // The room->resource edge survives, so navigation still works.
        let contained = mem.neighbors(&rg, Some("contains"));
        assert_eq!(contained.len(), 1);
        assert_eq!(contained[0].label, "store1");
        // Tabs in content were neutralised so the line format stays intact.
        let loaded_rg = mem.get(&rg).unwrap();
        assert!(!loaded_rg.content.contains('\t'));
    }

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn loading_missing_file_yields_empty_memory() {
    let path = std::path::Path::new("/nonexistent/azork/no-such.graph");
    assert!(GraphMemory::load(path).is_empty());
}

// ---- Gameplay recorders (rooms, resources, summary) --------------------

#[test]
fn remember_room_is_idempotent_and_reinforces() {
    let mut mem = GraphMemory::new();
    let a = mem.remember_room("azork-oit-rg", "eastus");
    let b = mem.remember_room("azork-oit-rg", "eastus");
    assert_eq!(a, b, "the same room is not duplicated");
    assert_eq!(mem.nodes_of_kind(MemoryKind::Room).len(), 1);
    assert!(
        mem.get(&a).unwrap().usage_count >= 1,
        "re-seeing reinforces"
    );
}

#[test]
fn remember_resource_links_room_contains_resource() {
    let mut mem = GraphMemory::new();
    let room = mem.remember_room("rg1", "eastus");
    let res = mem.remember_resource(&room, "store1", "storage");
    let contained = mem.neighbors(&room, Some("contains"));
    assert_eq!(contained.len(), 1);
    assert_eq!(contained[0].id, res);
    // Recording the same resource again does not duplicate the edge.
    mem.remember_resource(&room, "store1", "storage");
    assert_eq!(mem.neighbors(&room, Some("contains")).len(), 1);
}

#[test]
fn summary_counts_each_kind_and_surfaces_friction() {
    let mut mem = GraphMemory::new();
    let room = mem.remember_room("rg1", "eastus");
    mem.remember_resource(&room, "store1", "storage");
    mem.record_intent("look around");
    mem.record_friction("errors are cryptic", &["oit"]);
    let s = mem.summary();
    assert!(s.contains("1 rooms"));
    assert!(s.contains("1 resources"));
    assert!(s.contains("errors are cryptic"));
}
