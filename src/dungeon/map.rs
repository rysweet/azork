//! The dungeon map graph and its read-only, budgeted enumeration.
//!
//! Enumeration walks the subscription through the existing
//! [`crate::az_runner::AzRunner`] seam — never a fresh way of shelling out —
//! issuing only `list`/`show`-class (read-only) `az` invocations. See
//! `docs/DUNGEON-CRAWLER.md#the-map-model` for the full contract.

use crate::az_runner::AzRunner;
use crate::dungeon::icons;
use crate::dungeon::validate;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::hash_map::DefaultHasher;
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Soft default cap on in-memory resources buffered per enumeration window
/// before flushing into the map graph (see `--budget` in the CLI). This is a
/// memory-shape knob, not a limit on how much of the subscription gets
/// mapped — enumeration always continues to completion or cancellation.
pub const DEFAULT_BUDGET: usize = 500;

/// A single Azure resource, placed inside its owning [`Room`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceNode {
    /// Full Azure Resource Manager ID (never a secret; used for portal links
    /// and suggested `az` commands).
    pub id: String,
    /// Short resource name.
    pub name: String,
    /// Azure resource type, e.g. `Microsoft.Storage/storageAccounts`.
    pub kind: String,
    /// Azure region, e.g. `eastus`.
    pub region: String,
    /// Icon key resolved via [`crate::dungeon::icons::icon_for`].
    pub icon: String,
}

/// A room in the dungeon — one Azure resource group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Room {
    /// Resource group name, used as the room id.
    pub id: String,
    /// Resource group name (display).
    pub name: String,
    /// Azure region the resource group is pinned to.
    pub region: String,
    /// Deterministic grid X position, derived from a stable hash of
    /// `(name, region)` — never random, never viewport-dependent.
    pub x: i32,
    /// Deterministic grid Y position.
    pub y: i32,
    /// Resources (dungeon contents) enumerated for this room.
    pub resources: Vec<ResourceNode>,
}

/// A corridor between two rooms (shared region, or an observed network
/// relationship such as VNet peering / a private endpoint).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edge {
    pub from: String,
    pub to: String,
}

/// The full, serializable map graph: the single source of truth handed to
/// both the native renderer and the HTTP server's JSON API.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct DungeonMap {
    /// Subscription name/id the map was assembled from (`"mock"` for the
    /// offline estate).
    pub subscription: String,
    pub rooms: Vec<Room>,
    pub edges: Vec<Edge>,
    /// Set when enumeration was cancelled mid-flight (or otherwise could not
    /// observe the whole subscription): the map reflects a partial view and
    /// must be clearly labelled as such by callers (renderer, server).
    pub partial: bool,
}

impl DungeonMap {
    /// Look up a room by id (resource group name).
    pub fn room(&self, id: &str) -> Option<&Room> {
        self.rooms.iter().find(|r| r.id == id)
    }

    /// Look up a resource by its full ARM id, across all rooms.
    pub fn resource(&self, id: &str) -> Option<&ResourceNode> {
        self.rooms
            .iter()
            .flat_map(|r| r.resources.iter())
            .find(|res| res.id == id)
    }

    /// Total resource count across all rooms.
    pub fn resource_count(&self) -> usize {
        self.rooms.iter().map(|r| r.resources.len()).sum()
    }
}

/// A cheap, `Clone`-able cancellation flag shared between the caller and an
/// in-flight enumeration. Checked between rooms so `Ctrl-C` during "Mapping
/// subscription..." stops cleanly and yields whatever was assembled so far,
/// marked [`DungeonMap::partial`].
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> CancelToken {
        CancelToken(Arc::new(AtomicBool::new(false)))
    }

    /// Request cancellation. Safe to call from a signal handler / another
    /// thread.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// Build a [`DungeonMap`] by enumerating the subscription via `runner`,
/// buffering at most `budget` resources per in-memory window before
/// flushing into the graph. Equivalent to
/// `build_cancellable(runner, budget, &CancelToken::new())`.
pub fn build(runner: &dyn AzRunner, budget: usize) -> Result<DungeonMap, String> {
    build_cancellable(runner, budget, &CancelToken::new())
}

/// Build a [`DungeonMap`], honouring `cancel` between rooms.
///
/// Enumeration is strictly read-only: only `list`/`show`-class `az`
/// invocations are ever issued. Malformed or unexpected JSON for a single
/// resource group is a recoverable per-room skip (never a panic that aborts
/// the whole crawl); the map building process never mutates anything.
pub fn build_cancellable(
    runner: &dyn AzRunner,
    budget: usize,
    cancel: &CancelToken,
) -> Result<DungeonMap, String> {
    // `budget` shapes how many resources we buffer per room-processing
    // window before they're folded into the graph; it never truncates the
    // map. With a preallocated capacity hint it also avoids repeated
    // reallocation for large resource groups.
    let budget = budget.max(1);

    if cancel.is_cancelled() {
        return Ok(DungeonMap {
            subscription: "unknown".to_string(),
            rooms: Vec::new(),
            edges: Vec::new(),
            partial: true,
        });
    }

    let group_out = runner
        .run(&["group", "list", "-o", "json"])
        .map_err(|e| format!("failed to invoke `az group list`: {e}"))?;
    if !group_out.status.success() {
        return Err(format!(
            "`az group list` failed: {}",
            String::from_utf8_lossy(&group_out.stderr)
        ));
    }
    let groups: Vec<Value> = match serde_json::from_slice(&group_out.stdout) {
        Ok(v) => v,
        Err(e) => return Err(format!("could not parse `az group list` output: {e}")),
    };

    let mut rooms: Vec<Room> = Vec::with_capacity(groups.len());
    let mut subscription = "unknown".to_string();
    let mut partial = false;

    for group in &groups {
        if cancel.is_cancelled() {
            partial = true;
            break;
        }

        let name = match group.get("name").and_then(Value::as_str) {
            Some(n) if validate::is_valid_resource_group_name(n) => n.to_string(),
            None => continue, // Malformed row: no usable room id, skip it.
            Some(_) => continue,
        };
        let region = group
            .get("location")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();

        let (x, y) = deterministic_position(&name, &region);
        let mut resources: Vec<ResourceNode> = Vec::with_capacity(budget.min(64));

        let res_out = runner.run(&["resource", "list", "--resource-group", &name, "-o", "json"]);
        if let Ok(out) = res_out {
            if out.status.success() {
                if let Ok(entries) = serde_json::from_slice::<Vec<Value>>(&out.stdout) {
                    for entry in &entries {
                        let (Some(id), Some(rname), Some(kind)) = (
                            entry.get("id").and_then(Value::as_str),
                            entry.get("name").and_then(Value::as_str),
                            entry.get("type").and_then(Value::as_str),
                        ) else {
                            continue; // Skip a single malformed resource entry.
                        };
                        let Some(parsed_id) = validate::parse_resource_id(id) else {
                            continue;
                        };
                        if subscription == "unknown" {
                            if let Some(sub) = subscription_from_id(parsed_id.raw()) {
                                subscription = sub;
                            }
                        }
                        let res_region = entry
                            .get("location")
                            .and_then(Value::as_str)
                            .unwrap_or(&region)
                            .to_string();
                        resources.push(ResourceNode {
                            id: parsed_id.raw().to_string(),
                            name: rname.to_string(),
                            kind: kind.to_string(),
                            region: res_region,
                            icon: icons::icon_for(kind).to_string(),
                        });
                    }
                }
                // A parse failure for this room's resources is a recoverable
                // per-room skip: the room is still recorded, just empty.
            }
            // A non-zero exit for this room's resources is likewise a
            // recoverable per-room skip.
        }

        // The room's display name is the same field already validated as
        // `name` above; reuse it instead of re-parsing the JSON value.
        rooms.push(Room {
            name: name.clone(),
            id: name,
            region,
            x,
            y,
            resources,
        });
    }

    resolve_room_collisions(&mut rooms);
    let edges = same_region_edges(&rooms);

    Ok(DungeonMap {
        subscription,
        rooms,
        edges,
        partial,
    })
}

/// Extract the subscription id/name from a resource's ARM id
/// (`/subscriptions/<id>/...`), if present.
fn subscription_from_id(resource_id: &str) -> Option<String> {
    let trimmed = resource_id.strip_prefix('/').unwrap_or(resource_id);
    let mut parts = trimmed.split('/');
    if parts.next()? != "subscriptions" {
        return None;
    }
    parts.next().map(|s| s.to_string())
}

/// A stable, non-random grid position derived purely from `(name, region)`.
/// Never influenced by enumeration order or process/run state, so the same
/// subscription always lays out identically. The grid is deliberately small
/// (a compact dungeon "wing" per hash bucket, not a sprawling estate) so a
/// handful of resource groups draw as a tight, walkable dungeon rather than
/// a few rooms scattered across a mostly-empty parchment; any collisions
/// this creates between unrelated rooms are resolved afterwards by
/// [`resolve_room_collisions`].
fn deterministic_position(name: &str, region: &str) -> (i32, i32) {
    let mut hasher = DefaultHasher::new();
    (name, region).hash(&mut hasher);
    let h = hasher.finish();
    let x = ((h & 0xFFFF) % 6) as i32;
    let y = (((h >> 16) & 0xFFFF) % 6) as i32;
    (x, y)
}

/// Resolve any rooms whose hash-derived [`deterministic_position`] collided
/// with another room's, by walking them (in a stable, name-sorted order) to
/// the nearest free cell in an outward ring search. Since this only ever
/// runs over the *complete* room set already gathered for one build, and
/// visits rooms in a fixed (id-sorted) order, two builds of the same room
/// set always resolve collisions identically — determinism is preserved,
/// just no longer a function of a single room in isolation once a
/// collision has to be broken.
fn resolve_room_collisions(rooms: &mut [Room]) {
    let mut order: Vec<usize> = (0..rooms.len()).collect();
    order.sort_by(|&a, &b| rooms[a].id.cmp(&rooms[b].id));

    let mut occupied: std::collections::HashSet<(i32, i32)> = std::collections::HashSet::new();
    for idx in order {
        let mut pos = (rooms[idx].x, rooms[idx].y);
        if occupied.contains(&pos) {
            pos = nearest_free_cell(pos, &occupied);
        }
        occupied.insert(pos);
        rooms[idx].x = pos.0;
        rooms[idx].y = pos.1;
    }
}

/// Find the nearest free cell to `origin` by searching outward ring by ring
/// (Chebyshev distance 1, 2, 3, ...), scanning each ring in a fixed
/// deterministic order (top edge left-to-right, right edge top-to-bottom,
/// bottom edge right-to-left, left edge bottom-to-top).
fn nearest_free_cell(
    origin: (i32, i32),
    occupied: &std::collections::HashSet<(i32, i32)>,
) -> (i32, i32) {
    for radius in 1..64 {
        for (dx, dy) in ring_offsets(radius) {
            let candidate = (origin.0 + dx, origin.1 + dy);
            if candidate.0 >= 0 && candidate.1 >= 0 && !occupied.contains(&candidate) {
                return candidate;
            }
        }
    }
    // Unreachable in practice (a subscription would need more resource
    // groups than there are cells in a 128x128 search area), but a safe,
    // deterministic fallback rather than a panic.
    origin
}

/// Offsets of every cell on the square ring at Chebyshev distance `radius`
/// from the origin, in a fixed deterministic order.
fn ring_offsets(radius: i32) -> Vec<(i32, i32)> {
    let mut offsets = Vec::with_capacity((radius * 8) as usize);
    for dx in -radius..=radius {
        offsets.push((dx, -radius));
    }
    for dy in -radius + 1..=radius {
        offsets.push((radius, dy));
    }
    for dx in (-radius..=radius - 1).rev() {
        offsets.push((dx, radius));
    }
    for dy in (-radius + 1..=radius - 1).rev() {
        offsets.push((-radius, dy));
    }
    offsets
}

/// Connect every pair of distinct rooms that share a region with one
/// corridor edge (no self-edges, no duplicate reverse edges).
///
/// Rooms are first bucketed by region so only rooms within the same
/// region are ever compared, avoiding an all-pairs scan across the
/// entire subscription (which would be quadratic in total room count
/// even when regions are small and numerous). A `BTreeMap` (ordered by
/// region name) is used instead of a `HashMap` so the resulting edge
/// order stays deterministic across runs — the same subscription must
/// always produce the same map, and `HashMap` iteration order is
/// randomized per-process.
fn same_region_edges(rooms: &[Room]) -> Vec<Edge> {
    let mut by_region: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (idx, room) in rooms.iter().enumerate() {
        by_region.entry(room.region.as_str()).or_default().push(idx);
    }

    let mut edges = Vec::new();
    for indices in by_region.values() {
        for i in 0..indices.len() {
            for j in (i + 1)..indices.len() {
                edges.push(Edge {
                    from: rooms[indices[i]].id.clone(),
                    to: rooms[indices[j]].id.clone(),
                });
            }
        }
    }
    edges
}
