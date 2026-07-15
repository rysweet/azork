//! Optional companion crate: **durable graph memory** for AzZork, backed by the
//! external [`amplihack-memory`] store (the same SQLite/lbug-capable memory
//! library Simard uses).
//!
//! AzZork's own [`GraphMemory`](azork::memory::GraphMemory) is a small,
//! dependency-free, offline brick with a hand-rolled on-disk format. This crate
//! is the *live* durable counterpart: it mirrors that graph into an
//! `amplihack-memory` [`MemoryConnector`], giving AzZork a real, queryable,
//! SQLite-backed persistent memory whose full-text search powers ranked recall
//! across sessions — exactly the "ladybug-style graph memory" the mission calls
//! for.
//!
//! Each memory node becomes one [`Experience`]:
//!
//! | AzZork node          | Experience field                       |
//! |----------------------|----------------------------------------|
//! | `label`              | `context`                              |
//! | `content`            | `outcome`                              |
//! | `importance`         | `confidence`                           |
//! | `kind`               | [`ExperienceType`] + a tag             |
//! | `tags`               | `tags`                                 |
//! | `id`, `usage`, edges | `metadata` (for faithful reload)       |
//!
//! It lives in a **separate crate** on purpose: cargo resolves path/optional
//! dependencies even when a feature is off, so pulling `amplihack-memory` (which
//! compiles bundled SQLite) into the azork manifest would break the
//! zero-dependency, offline fresh-clone build. Building this crate is opt-in and
//! requires the reference repos checked out side-by-side.
//!
//! ```no_run
//! use azork::memory::{GraphMemory, MemoryKind};
//! use azork_memory_store::PersistentStore;
//!
//! let mut mem = GraphMemory::new();
//! mem.record_friction("create verb dead-ends", &["verb"]);
//!
//! // Persist the whole graph into the durable, SQLite-backed store...
//! let mut store = PersistentStore::open("azork", "/tmp/azork-mem").unwrap();
//! store.save(&mem).unwrap();
//!
//! // ...and rehydrate it (in a later session / process).
//! let restored = store.load().unwrap();
//! assert_eq!(restored.nodes_of_kind(MemoryKind::Friction).len(), 1);
//! ```
//!
//! [`amplihack-memory`]: https://github.com/rysweet/amplihack-memory-lib

use std::collections::HashMap;
use std::path::Path;

use amplihack_memory::{Experience, ExperienceType, MemoryConnector};
use azork::memory::{GraphMemory, MemoryKind, MemoryNode};
use serde_json::{json, Value};

/// Durable, SQLite-backed persistent store for an AzZork [`GraphMemory`].
///
/// Wraps an `amplihack-memory` [`MemoryConnector`], translating AzZork memory
/// nodes to/from [`Experience`] records so the graph survives across sessions
/// and can be recalled with full-text ranked search.
pub struct PersistentStore {
    connector: MemoryConnector,
}

/// A reconstruction error, surfaced as a plain string to stay dependency-light.
type StoreResult<T> = Result<T, String>;

impl PersistentStore {
    /// Open (or create) a durable store for `agent_name` at `storage_dir`.
    ///
    /// The directory is created if missing; a SQLite database is opened inside
    /// it. A generous 512 MB quota and no auto-compression are used so the graph
    /// is stored verbatim.
    pub fn open(agent_name: &str, storage_dir: impl AsRef<Path>) -> StoreResult<Self> {
        let dir = storage_dir.as_ref();
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("could not create store dir {}: {e}", dir.display()))?;
        let connector = MemoryConnector::new(agent_name, Some(dir), 512, false)
            .map_err(|e| format!("could not open amplihack-memory store: {e}"))?;
        Ok(PersistentStore { connector })
    }

    /// Mirror every node and edge of `mem` into the durable store.
    ///
    /// Returns the number of nodes written. Re-saving the same graph is safe:
    /// [`load`](Self::load) de-duplicates by the AzZork node id, keeping the most
    /// recently touched copy, so the latest snapshot always wins.
    pub fn save(&mut self, mem: &GraphMemory) -> StoreResult<usize> {
        // Group outgoing edges by source node so each node carries its own edges.
        let mut out_edges: HashMap<&str, Vec<Value>> = HashMap::new();
        for (from, relation, to) in mem.all_edges() {
            out_edges
                .entry(from)
                .or_default()
                .push(json!([relation, to]));
        }

        let mut written = 0usize;
        for node in mem.iter_nodes() {
            let edges = out_edges.remove(node.id.as_str()).unwrap_or_default();
            let exp = node_to_experience(node, edges)?;
            self.connector
                .store_experience(&exp)
                .map_err(|e| format!("could not store node {}: {e}", node.id))?;
            written += 1;
        }
        Ok(written)
    }

    /// Rehydrate a [`GraphMemory`] from everything in the durable store.
    ///
    /// Nodes are de-duplicated by AzZork id (highest `last_touch` wins), then
    /// edges are rebuilt. Records not written by AzZork (missing the `azork_id`
    /// metadata marker) are ignored, so the store can be shared safely.
    pub fn load(&self) -> StoreResult<GraphMemory> {
        let experiences = self
            .connector
            .retrieve_experiences(None, None, 0.0)
            .map_err(|e| format!("could not read store: {e}"))?;

        // De-duplicate by azork id, keeping the freshest (highest last_touch).
        let mut best: HashMap<String, (MemoryNode, Vec<(String, String)>)> = HashMap::new();
        for exp in &experiences {
            let Some((node, edges)) = experience_to_node(exp) else {
                continue;
            };
            match best.get(&node.id) {
                Some((existing, _)) if existing.last_touch >= node.last_touch => {}
                _ => {
                    best.insert(node.id.clone(), (node, edges));
                }
            }
        }

        let mut mem = GraphMemory::new();
        // Insert all nodes first so edge endpoints exist.
        let mut all_edges: Vec<(String, String, String)> = Vec::new();
        for (id, (node, edges)) in best {
            for (relation, to) in edges {
                all_edges.push((id.clone(), relation, to));
            }
            mem.insert_node(node);
        }
        for (from, relation, to) in all_edges {
            // Skip dangling edges rather than fail the whole load.
            let _ = mem.add_edge(&from, &relation, &to);
        }
        Ok(mem)
    }

    /// Full-text ranked recall through the durable store, returning up to
    /// `limit` `(label, content)` pairs most relevant to `query`.
    ///
    /// This delegates to `amplihack-memory`'s SQLite FTS search — the external
    /// service's recall engine — rather than AzZork's in-memory scorer.
    pub fn recall(&self, query: &str, limit: usize) -> StoreResult<Vec<(String, String)>> {
        let hits = self
            .connector
            .search(query, None, 0.0, limit)
            .map_err(|e| format!("recall failed: {e}"))?;
        Ok(hits.into_iter().map(|e| (e.context, e.outcome)).collect())
    }
}

/// Map an AzZork [`MemoryKind`] onto the closest [`ExperienceType`].
fn kind_to_type(kind: MemoryKind) -> ExperienceType {
    match kind {
        // Learned verbs and observed intents are insights the game accrued.
        MemoryKind::Capability | MemoryKind::Intent => ExperienceType::Insight,
        // Rooms and resources are concrete, successfully-observed facts.
        MemoryKind::Room | MemoryKind::Resource => ExperienceType::Success,
        // Friction is something that went wrong / needs fixing.
        MemoryKind::Friction => ExperienceType::Failure,
    }
}

/// Truncate to at most `max` characters, falling back to `fallback` when empty.
fn clip(s: &str, max: usize, fallback: &str) -> String {
    let trimmed = s.trim();
    let src = if trimmed.is_empty() {
        fallback
    } else {
        trimmed
    };
    src.chars().take(max).collect()
}

/// Convert an AzZork node (plus its outgoing edges) into an [`Experience`].
fn node_to_experience(node: &MemoryNode, edges: Vec<Value>) -> StoreResult<Experience> {
    // amplihack-memory caps context at 500 and outcome at 1000 chars; clip to fit
    // and never let either be empty (both are required).
    let context = clip(&node.label, 500, &node.id);
    let outcome = clip(&node.content, 1000, &node.label);
    let mut exp = Experience::new(
        kind_to_type(node.kind),
        context,
        outcome,
        node.importance.clamp(0.0, 1.0),
    )
    .map_err(|e| format!("invalid experience for node {}: {e}", node.id))?;

    // Tags: preserve the node's own tags and stamp the kind for filtering.
    exp.tags = node.tags.clone();
    let token = node.kind.as_token().to_string();
    if !exp.tags.iter().any(|t| t == &token) {
        exp.tags.push(token);
    }

    // Metadata carries everything needed for a faithful reload.
    exp.metadata.insert("azork_id".into(), json!(node.id));
    exp.metadata
        .insert("azork_kind".into(), json!(node.kind.as_token()));
    exp.metadata
        .insert("azork_usage".into(), json!(node.usage_count));
    exp.metadata
        .insert("azork_touch".into(), json!(node.last_touch));
    exp.metadata.insert("azork_tags".into(), json!(node.tags));
    exp.metadata.insert("azork_edges".into(), json!(edges));
    Ok(exp)
}

/// Reconstruct an AzZork node and its outgoing edges from an [`Experience`].
///
/// Returns `None` for records that AzZork did not write (no `azork_id`), so an
/// externally-shared store never corrupts the rehydrated graph.
fn experience_to_node(exp: &Experience) -> Option<(MemoryNode, Vec<(String, String)>)> {
    let id = exp.metadata.get("azork_id")?.as_str()?.to_string();
    let kind = exp
        .metadata
        .get("azork_kind")
        .and_then(|v| v.as_str())
        .and_then(MemoryKind::from_token)?;
    let usage_count = exp
        .metadata
        .get("azork_usage")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let last_touch = exp
        .metadata
        .get("azork_touch")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let tags: Vec<String> = exp
        .metadata
        .get("azork_tags")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|t| t.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let edges: Vec<(String, String)> = exp
        .metadata
        .get("azork_edges")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|pair| {
                    let arr = pair.as_array()?;
                    let relation = arr.first()?.as_str()?.to_string();
                    let to = arr.get(1)?.as_str()?.to_string();
                    Some((relation, to))
                })
                .collect()
        })
        .unwrap_or_default();

    let node = MemoryNode {
        id,
        kind,
        label: exp.context.clone(),
        content: exp.outcome.clone(),
        tags,
        importance: exp.confidence,
        usage_count,
        last_touch,
    };
    Some((node, edges))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn seed() -> GraphMemory {
        let mut mem = GraphMemory::new();
        let room = mem.remember_room("alpha-rg", "eastus");
        mem.remember_resource(&room, "alphastore", "storage");
        mem.record_intent("secure the storage account");
        mem.record_friction("lock verb missing a confirmation prompt", &["lock", "ux"]);
        mem
    }

    #[test]
    fn round_trips_nodes_edges_and_kinds() {
        let mem = seed();
        let dir = TempDir::new().unwrap();
        let mut store = PersistentStore::open("azork-test", dir.path()).unwrap();
        let written = store.save(&mem).unwrap();
        assert_eq!(written, mem.iter_nodes().count());

        let restored = store.load().unwrap();

        // Same node population, kind by kind.
        for kind in [
            MemoryKind::Room,
            MemoryKind::Resource,
            MemoryKind::Intent,
            MemoryKind::Friction,
        ] {
            assert_eq!(
                restored.nodes_of_kind(kind).len(),
                mem.nodes_of_kind(kind).len(),
                "kind {kind:?} count mismatch"
            );
        }

        // The room -> contains -> resource edge survived.
        let room = restored.nodes_of_kind(MemoryKind::Room)[0];
        let neighbors = restored.neighbors(&room.id, Some("contains"));
        assert_eq!(neighbors.len(), 1);
        assert_eq!(neighbors[0].label, "alphastore");

        // Node fidelity: importance and content preserved for the friction note.
        let friction = restored.nodes_of_kind(MemoryKind::Friction)[0];
        assert!(friction.content.contains("confirmation prompt"));
        assert!((friction.importance - 0.9).abs() < 1e-9);
        assert!(friction.tags.contains(&"lock".to_string()));
    }

    #[test]
    fn recall_finds_relevant_nodes_via_external_fts() {
        let mem = seed();
        let dir = TempDir::new().unwrap();
        let mut store = PersistentStore::open("azork-recall", dir.path()).unwrap();
        store.save(&mem).unwrap();

        let hits = store.recall("storage", 10).unwrap();
        assert!(
            hits.iter().any(|(label, _)| label == "alphastore"),
            "expected the storage resource in recall hits, got {hits:?}"
        );
    }

    #[test]
    fn re_saving_keeps_freshest_snapshot() {
        let mut mem = seed();
        let dir = TempDir::new().unwrap();
        let mut store = PersistentStore::open("azork-resave", dir.path()).unwrap();
        store.save(&mem).unwrap();

        // Reinforce a room (bumps last_touch) and save again.
        let room = mem.remember_room("alpha-rg", "eastus");
        mem.reinforce(&room);
        store.save(&mem).unwrap();

        let restored = store.load().unwrap();
        // De-dup by id: still exactly one room, not two.
        assert_eq!(restored.nodes_of_kind(MemoryKind::Room).len(), 1);
    }
}
