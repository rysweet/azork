//! Persistent graph memory — AzZork's cognitive spine.
//!
//! This is the ladybug-style memory that lets the game *evolve as it is used*.
//! It mirrors the Simard cognitive-memory pattern ([`MemoryKind`],
//! [`RecallWeights`], ranked recall with reinforcement) but stays a small,
//! dependency-free, fully-offline brick by default:
//!
//! * The default [`GraphMemory`] is an **in-memory graph** with a hand-rolled,
//!   line-based on-disk format — no native code, no network, deterministic
//!   (a monotonic tick, not wall-clock time, drives recency). This is what the
//!   default `cargo build`/`cargo test` and CI exercise.
//! * A future, opt-in `persistent` Cargo feature will swap in the native
//!   `lbug`-backed `amplihack-memory` store behind the same shape, per the
//!   Simard architecture. That backend never links into the default build.
//!
//! The memory stores the four things the game learns as it is played:
//! discovered `az` **capabilities**, the resource graph (resource groups are
//! **rooms**, resources are **objects**, relationships are **edges**), the
//! **intents** players express, and **friction** notes worth fixing. Ranked
//! recall over this graph informs help, navigation, and intent resolution.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::secrets::scrub;

/// The kind of thing a memory node represents.
///
/// Kept backend-neutral (à la Simard's [`MemoryKind`]) so a richer store can
/// slot in behind the same enum without touching callers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryKind {
    /// A learned `az` capability (a verb the game understands).
    Capability,
    /// A resource group — a *room* in the adventure.
    Room,
    /// An Azure resource — an *object* in a room.
    Resource,
    /// A user intent the game has seen expressed.
    Intent,
    /// A friction note: something confusing, missing, or worth improving.
    Friction,
}

impl MemoryKind {
    /// Stable token used in the on-disk format (and by external persistence
    /// backends that mirror this graph).
    pub fn as_token(self) -> &'static str {
        match self {
            MemoryKind::Capability => "capability",
            MemoryKind::Room => "room",
            MemoryKind::Resource => "resource",
            MemoryKind::Intent => "intent",
            MemoryKind::Friction => "friction",
        }
    }

    /// Parse a token produced by [`MemoryKind::as_token`].
    pub fn from_token(s: &str) -> Option<MemoryKind> {
        match s {
            "capability" => Some(MemoryKind::Capability),
            "room" => Some(MemoryKind::Room),
            "resource" => Some(MemoryKind::Resource),
            "intent" => Some(MemoryKind::Intent),
            "friction" => Some(MemoryKind::Friction),
            _ => None,
        }
    }
}

/// Per-term weights for ranked recall (mirrors Simard's `RecallWeightSet`).
///
/// Weights are un-normalized — only relative magnitudes matter. Each scales one
/// scoring term blended in [`GraphMemory::recall_ranked`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RecallWeights {
    /// Weight on keyword overlap between the query and the node's text.
    pub text: f64,
    /// Weight on the node's importance/salience.
    pub importance: f64,
    /// Weight on recency (a node touched more recently ranks higher).
    pub recency: f64,
    /// Weight on the sub-linear usage boost.
    pub usage: f64,
}

impl Default for RecallWeights {
    /// Text-led baseline: `1.0, 0.5, 0.4, 0.3`.
    fn default() -> RecallWeights {
        RecallWeights {
            text: 1.0,
            importance: 0.5,
            recency: 0.4,
            usage: 0.3,
        }
    }
}

/// A single node in the memory graph.
#[derive(Debug, Clone, PartialEq)]
pub struct MemoryNode {
    /// Stable, session-unique identifier (survives save/load).
    pub id: String,
    /// What kind of thing this node is.
    pub kind: MemoryKind,
    /// Short name/key, e.g. `"group create"` or `"alpha-rg"`.
    pub label: String,
    /// Human-readable summary / note text.
    pub content: String,
    /// Free-form tags for filtering and recall.
    pub tags: Vec<String>,
    /// Salience in `[0, 1]`.
    pub importance: f64,
    /// How many times this node has been reinforced (recalled/used).
    pub usage_count: u64,
    /// Monotonic tick of the last touch — drives deterministic recency.
    pub last_touch: u64,
}

/// A directed, labelled edge between two nodes.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Edge {
    from: String,
    relation: String,
    to: String,
}

/// The in-memory graph store (the default, offline backend).
#[derive(Debug, Clone, Default)]
pub struct GraphMemory {
    nodes: BTreeMap<String, MemoryNode>,
    edges: Vec<Edge>,
    /// Monotonic logical clock; every mutation advances it.
    clock: u64,
    /// Monotonic id sequence.
    seq: u64,
}

impl GraphMemory {
    /// An empty memory.
    pub fn new() -> GraphMemory {
        GraphMemory::default()
    }

    /// Number of nodes remembered.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether nothing has been remembered yet.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Remember a node, returning its stable id.
    ///
    /// `label` and `content` are free text that may originate from user input
    /// or `az` CLI output; both are passed through
    /// [`scrub`](crate::secrets::scrub) before being stored, so a stray
    /// secret never lands in the in-memory graph or its on-disk save format
    /// (see [`Self::save`]). This is the single choke point every recorder
    /// (`record_friction`, `record_intent`, `remember_capability`,
    /// `remember_room`, `remember_resource`) funnels through.
    ///
    /// **Maintenance invariant:** any future node-creation path must route
    /// through `remember()` rather than constructing a `MemoryNode`
    /// directly — bypassing it would skip scrubbing and reopen the gap
    /// tracked as issue #17.
    pub fn remember(
        &mut self,
        kind: MemoryKind,
        label: &str,
        content: &str,
        tags: &[&str],
        importance: f64,
    ) -> String {
        self.clock += 1;
        self.seq += 1;
        let id = format!("n{}", self.seq);
        let node = MemoryNode {
            id: id.clone(),
            kind,
            label: scrub(label),
            content: scrub(content),
            tags: tags.iter().map(|t| t.to_string()).collect(),
            importance: importance.clamp(0.0, 1.0),
            usage_count: 0,
            last_touch: self.clock,
        };
        self.nodes.insert(id.clone(), node);
        id
    }

    /// Look up a node by id.
    pub fn get(&self, id: &str) -> Option<&MemoryNode> {
        self.nodes.get(id)
    }

    /// All nodes of a given kind, in stable id order.
    pub fn nodes_of_kind(&self, kind: MemoryKind) -> Vec<&MemoryNode> {
        self.nodes.values().filter(|n| n.kind == kind).collect()
    }

    /// Connect two existing nodes with a labelled edge.
    ///
    /// Both endpoints must already exist — a dangling edge is an error rather
    /// than a silently-ignored no-op, so graph integrity is enforced.
    pub fn add_edge(&mut self, from: &str, relation: &str, to: &str) -> Result<(), String> {
        if !self.nodes.contains_key(from) {
            return Err(format!("unknown edge source node '{from}'"));
        }
        if !self.nodes.contains_key(to) {
            return Err(format!("unknown edge target node '{to}'"));
        }
        self.clock += 1;
        self.edges.push(Edge {
            from: from.to_string(),
            relation: relation.to_string(),
            to: to.to_string(),
        });
        Ok(())
    }

    /// The nodes reachable from `id` along `relation` (or any relation when
    /// `relation` is `None`), in edge-insertion order.
    pub fn neighbors(&self, id: &str, relation: Option<&str>) -> Vec<&MemoryNode> {
        self.edges
            .iter()
            .filter(|e| e.from == id && relation.map(|r| r == e.relation).unwrap_or(true))
            .filter_map(|e| self.nodes.get(&e.to))
            .collect()
    }

    /// All nodes in stable id order (for external mirroring / persistence).
    pub fn iter_nodes(&self) -> impl Iterator<Item = &MemoryNode> {
        self.nodes.values()
    }

    /// Every edge as a `(from, relation, to)` triple, in insertion order
    /// (for external mirroring / persistence).
    pub fn all_edges(&self) -> Vec<(&str, &str, &str)> {
        self.edges
            .iter()
            .map(|e| (e.from.as_str(), e.relation.as_str(), e.to.as_str()))
            .collect()
    }

    /// Insert a fully-formed node verbatim, preserving its id, usage, and touch
    /// tick. Used when rehydrating the graph from an external store; the logical
    /// clock and id sequence are advanced so later [`remember`](Self::remember)
    /// calls never collide with a restored id.
    pub fn insert_node(&mut self, node: MemoryNode) {
        self.clock = self.clock.max(node.last_touch);
        if let Some(n) = node
            .id
            .strip_prefix('n')
            .and_then(|s| s.parse::<u64>().ok())
        {
            self.seq = self.seq.max(n);
        }
        self.nodes.insert(node.id.clone(), node);
    }

    /// Reinforce a node: bump its usage and mark it freshly touched. This is the
    /// feedback loop that lets frequently-used knowledge float to the top of
    /// recall over time.
    pub fn reinforce(&mut self, id: &str) {
        self.clock += 1;
        let clock = self.clock;
        if let Some(node) = self.nodes.get_mut(id) {
            node.usage_count += 1;
            node.last_touch = clock;
        }
    }

    /// Ranked recall over the whole graph with default weights.
    pub fn recall(&self, query: &str, kind: Option<MemoryKind>, limit: usize) -> Vec<&MemoryNode> {
        self.recall_ranked(query, kind, limit, RecallWeights::default())
    }

    /// Ranked recall scoped to a single [`MemoryKind`], with default weights.
    pub fn recall_kind(&self, query: &str, kind: MemoryKind, limit: usize) -> Vec<&MemoryNode> {
        self.recall_ranked(query, Some(kind), limit, RecallWeights::default())
    }

    /// Ranked recall: score every candidate across text relevance, importance,
    /// recency, and usage, weighted by `weights`, and return the best `limit`
    /// nodes in descending score order (ties broken by id for determinism).
    ///
    /// Only nodes with some textual overlap with the query are considered, so an
    /// unrelated query returns nothing rather than noise.
    pub fn recall_ranked(
        &self,
        query: &str,
        kind: Option<MemoryKind>,
        limit: usize,
        weights: RecallWeights,
    ) -> Vec<&MemoryNode> {
        let tokens: Vec<String> = query
            .to_lowercase()
            .split_whitespace()
            .map(|t| t.to_string())
            .collect();
        if tokens.is_empty() {
            return Vec::new();
        }
        // Normalise recency against the current clock so it lands in [0, 1].
        let clock = self.clock.max(1) as f64;

        let mut scored: Vec<(f64, &MemoryNode)> = self
            .nodes
            .values()
            .filter(|n| kind.map(|k| k == n.kind).unwrap_or(true))
            .filter_map(|n| {
                let text = text_score(n, &tokens);
                if text <= 0.0 {
                    return None;
                }
                let recency = n.last_touch as f64 / clock;
                let usage = 1.0 - 1.0 / (1.0 + n.usage_count as f64);
                let score = weights.text * text
                    + weights.importance * n.importance
                    + weights.recency * recency
                    + weights.usage * usage;
                Some((score, n))
            })
            .collect();

        // Higher score first; stable id order breaks ties deterministically.
        scored.sort_by(|a, b| {
            b.0.partial_cmp(&a.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.1.id.cmp(&b.1.id))
        });
        scored.into_iter().take(limit).map(|(_, n)| n).collect()
    }

    // ---- Convenience recorders for the mission's data kinds ------------

    /// Record a friction note (something confusing / missing / worth fixing).
    pub fn record_friction(&mut self, note: &str, tags: &[&str]) -> String {
        // Friction is high-salience by default: it is what we want to surface
        // and fix.
        self.remember(MemoryKind::Friction, "friction", note, tags, 0.9)
    }

    /// Record a raw user intent the game has seen.
    pub fn record_intent(&mut self, raw: &str) -> String {
        self.remember(MemoryKind::Intent, "intent", raw, &[], 0.4)
    }

    /// Remember a learned [`crate::capabilities::Capability`] as a node, so the
    /// capability registry and the graph memory stay in step.
    pub fn remember_capability(&mut self, cap: &crate::capabilities::Capability) -> String {
        self.remember(
            MemoryKind::Capability,
            &cap.key(),
            &cap.summary,
            &[cap.group.as_str(), "capability"],
            0.6,
        )
    }

    /// Remember (or reinforce) a resource group as a *room* node. Idempotent on
    /// label: a room seen again is reinforced rather than duplicated. Returns the
    /// node id.
    pub fn remember_room(&mut self, name: &str, region: &str) -> String {
        if let Some(id) = self.find_by_label(MemoryKind::Room, name) {
            self.reinforce(&id);
            return id;
        }
        self.remember(
            MemoryKind::Room,
            name,
            &format!("resource group in {region}"),
            &[region, "room"],
            0.5,
        )
    }

    /// Remember (or reinforce) a resource as an *object* node inside a room, and
    /// link `room -> contains -> resource`. Returns the resource node id.
    pub fn remember_resource(&mut self, room_id: &str, name: &str, kind: &str) -> String {
        let id = if let Some(existing) = self.find_by_label(MemoryKind::Resource, name) {
            self.reinforce(&existing);
            existing
        } else {
            self.remember(
                MemoryKind::Resource,
                name,
                &format!("{kind} resource"),
                &[kind, "resource"],
                0.4,
            )
        };
        // Link the room to the resource if the edge does not already exist.
        if self.nodes.contains_key(room_id)
            && !self
                .edges
                .iter()
                .any(|e| e.from == room_id && e.to == id && e.relation == "contains")
        {
            let _ = self.add_edge(room_id, "contains", &id);
        }
        id
    }

    /// First node of `kind` whose label matches `label` (case-insensitive).
    fn find_by_label(&self, kind: MemoryKind, label: &str) -> Option<String> {
        let want = label.to_lowercase();
        self.nodes
            .values()
            .find(|n| n.kind == kind && n.label.to_lowercase() == want)
            .map(|n| n.id.clone())
    }

    /// A short, player-facing summary of what AzZork remembers: counts by kind
    /// plus the most salient recent notes.
    pub fn summary(&self) -> String {
        if self.nodes.is_empty() {
            return "AzZork's memory is empty — nothing learned yet.".to_string();
        }
        let count = |k: MemoryKind| self.nodes_of_kind(k).len();
        let mut out = format!(
            "AzZork remembers {} things: {} capabilities, {} rooms, {} resources, \
             {} intents, {} friction notes.",
            self.nodes.len(),
            count(MemoryKind::Capability),
            count(MemoryKind::Room),
            count(MemoryKind::Resource),
            count(MemoryKind::Intent),
            count(MemoryKind::Friction),
        );
        // Surface the freshest friction notes — those are what we want to fix.
        let mut frictions: Vec<&MemoryNode> = self.nodes_of_kind(MemoryKind::Friction);
        frictions.sort_by_key(|f| std::cmp::Reverse(f.last_touch));
        if !frictions.is_empty() {
            out.push_str("\nRecent friction:");
            for f in frictions.iter().take(5) {
                out.push_str(&format!("\n  - {}", f.content));
            }
        }
        out
    }

    // ---- Persistence ---------------------------------------------------

    /// Persist the graph to `path` in a dependency-free line format, creating
    /// parent directories as needed.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).map_err(|e| {
                    format!("could not create memory dir {}: {}", parent.display(), e)
                })?;
            }
        }
        let mut body = String::from("# azork graph memory v1\n");
        body.push_str(&format!("M\t{}\t{}\n", self.clock, self.seq));
        for node in self.nodes.values() {
            body.push_str(&format!(
                "N\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                sanitize(&node.id),
                node.kind.as_token(),
                node.importance,
                node.usage_count,
                node.last_touch,
                sanitize(&node.label),
                sanitize(&node.content),
                sanitize(&node.tags.join(",")),
            ));
        }
        for edge in &self.edges {
            body.push_str(&format!(
                "E\t{}\t{}\t{}\n",
                sanitize(&edge.from),
                sanitize(&edge.relation),
                sanitize(&edge.to),
            ));
        }
        fs::write(path, body)
            .map_err(|e| format!("could not write memory {}: {}", path.display(), e))
    }

    /// Load a graph from `path`. A missing file yields an empty memory (first
    /// run); malformed lines are skipped defensively.
    pub fn load(path: &Path) -> GraphMemory {
        let mut mem = GraphMemory::new();
        let Ok(text) = fs::read_to_string(path) else {
            return mem;
        };
        for line in text.lines() {
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.split('\t');
            match parts.next() {
                Some("M") => {
                    if let (Some(c), Some(s)) = (parts.next(), parts.next()) {
                        mem.clock = c.trim().parse().unwrap_or(0);
                        mem.seq = s.trim().parse().unwrap_or(0);
                    }
                }
                Some("N") => {
                    if let Some(node) = parse_node_line(&mut parts) {
                        mem.nodes.insert(node.id.clone(), node);
                    }
                }
                Some("E") => {
                    if let (Some(f), Some(r), Some(t)) = (parts.next(), parts.next(), parts.next())
                    {
                        mem.edges.push(Edge {
                            from: f.to_string(),
                            relation: r.to_string(),
                            to: t.to_string(),
                        });
                    }
                }
                _ => {}
            }
        }
        mem
    }
}

/// Default memory location: `$AZORK_CACHE_DIR/memory.graph`, else
/// `$XDG_DATA_HOME/azork/…`, else `~/.local/share/azork/…`, else `./`.
pub fn default_memory_path() -> PathBuf {
    if let Ok(dir) = std::env::var("AZORK_CACHE_DIR") {
        return PathBuf::from(dir).join("memory.graph");
    }
    let base = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .ok()
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|h| PathBuf::from(h).join(".local/share"))
        })
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("azork").join("memory.graph")
}

/// Fraction of query tokens that appear in a node's text (`[0, 1]`).
fn text_score(node: &MemoryNode, tokens: &[String]) -> f64 {
    if tokens.is_empty() {
        return 0.0;
    }
    let haystack = format!(
        "{} {} {}",
        node.label.to_lowercase(),
        node.content.to_lowercase(),
        node.tags.join(" ").to_lowercase(),
    );
    let matched = tokens.iter().filter(|t| haystack.contains(*t)).count();
    matched as f64 / tokens.len() as f64
}

/// Parse the fields of an `N` (node) line after the leading tag.
fn parse_node_line<'a>(parts: &mut impl Iterator<Item = &'a str>) -> Option<MemoryNode> {
    let id = parts.next()?.to_string();
    let kind = MemoryKind::from_token(parts.next()?)?;
    let importance: f64 = parts.next()?.trim().parse().ok()?;
    let usage_count: u64 = parts.next()?.trim().parse().ok()?;
    let last_touch: u64 = parts.next()?.trim().parse().ok()?;
    let label = parts.next()?.to_string();
    let content = parts.next().unwrap_or("").to_string();
    let tags_csv = parts.next().unwrap_or("");
    let tags: Vec<String> = tags_csv
        .split(',')
        .filter(|t| !t.trim().is_empty())
        .map(|t| t.trim().to_string())
        .collect();
    Some(MemoryNode {
        id,
        kind,
        label,
        content,
        tags,
        importance,
        usage_count,
        last_touch,
    })
}

/// Replace tabs/newlines with spaces so a field stays on one line.
fn sanitize(s: &str) -> String {
    s.replace(['\t', '\n', '\r'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remember_and_get() {
        let mut mem = GraphMemory::new();
        let id = mem.remember(MemoryKind::Room, "rg", "a room", &["x"], 0.5);
        assert_eq!(mem.get(&id).unwrap().label, "rg");
    }

    #[test]
    fn importance_is_clamped() {
        let mut mem = GraphMemory::new();
        let id = mem.remember(MemoryKind::Room, "rg", "a room", &[], 5.0);
        assert_eq!(mem.get(&id).unwrap().importance, 1.0);
    }

    #[test]
    fn empty_query_recalls_nothing() {
        let mut mem = GraphMemory::new();
        mem.remember(MemoryKind::Room, "rg", "a room", &[], 0.5);
        assert!(mem.recall("   ", None, 5).is_empty());
    }

    #[test]
    fn remember_scrubs_secret_shaped_content_and_label() {
        use crate::secrets::test_fixtures::{HOSTILE_ACCOUNT_KEY_VALUE, HOSTILE_TOKEN};

        let mut mem = GraphMemory::new();
        let label = format!("token: {HOSTILE_TOKEN}");
        let content = format!(
            "DefaultEndpointsProtocol=https;AccountKey={HOSTILE_ACCOUNT_KEY_VALUE};EndpointSuffix=core.windows.net"
        );
        let id = mem.remember(MemoryKind::Friction, &label, &content, &[], 0.9);

        let node = mem.get(&id).unwrap();
        assert!(!node.label.contains(HOSTILE_TOKEN));
        assert!(!node.content.contains(HOSTILE_ACCOUNT_KEY_VALUE));
        assert!(node.content.contains("AccountKey=***REDACTED***"));
    }

    #[test]
    fn record_friction_scrubs_secret_shaped_note() {
        use crate::secrets::test_fixtures::HOSTILE_TOKEN;

        let mut mem = GraphMemory::new();
        let note = format!("saw a leaked client_secret={HOSTILE_TOKEN} in az output");
        let id = mem.record_friction(&note, &["az"]);

        let node = mem.get(&id).unwrap();
        assert!(!node.content.contains(HOSTILE_TOKEN));
        assert!(node.content.contains("***REDACTED***"));
    }

    #[test]
    fn save_and_load_round_trip_never_reintroduces_secret() {
        use crate::secrets::test_fixtures::HOSTILE_TOKEN;
        use std::env;

        let mut mem = GraphMemory::new();
        let note = format!("password={HOSTILE_TOKEN}");
        mem.record_friction(&note, &["x"]);

        let mut path = env::temp_dir();
        path.push(format!(
            "azork-memory-scrub-test-{}.mem",
            std::process::id()
        ));
        mem.save(&path).expect("save should succeed");

        let restored = GraphMemory::load(&path);
        let _ = fs::remove_file(&path);

        let friction = restored.nodes_of_kind(MemoryKind::Friction);
        assert_eq!(friction.len(), 1);
        assert!(!friction[0].content.contains(HOSTILE_TOKEN));
        assert!(friction[0].content.contains("***REDACTED***"));
    }

    #[test]
    fn kind_token_round_trips() {
        for k in [
            MemoryKind::Capability,
            MemoryKind::Room,
            MemoryKind::Resource,
            MemoryKind::Intent,
            MemoryKind::Friction,
        ] {
            assert_eq!(MemoryKind::from_token(k.as_token()), Some(k));
        }
    }
}
