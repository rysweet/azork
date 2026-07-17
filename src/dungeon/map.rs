//! The dungeon map graph and its read-only, budgeted enumeration.
//!
//! Enumeration walks the subscription through the existing
//! [`crate::az_runner::AzRunner`] seam — never a fresh way of shelling out —
//! issuing only `list`/`show`-class (read-only) `az` invocations. See
//! `docs/DUNGEON-CRAWLER.md#the-map-model` for the full contract.

use crate::az_runner::AzRunner;
use crate::dungeon::concurrency::{backoff_with_jitter, AimdLimiter, ThrottleDetector};
use crate::dungeon::icons;
use crate::dungeon::validate;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::hash_map::DefaultHasher;
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::process::Output;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// Base delay for the throttle-triggered backoff on a single `az` call
/// (before the [`AimdLimiter`]'s own concurrency shrink kicks in). Doubled
/// per attempt and jittered by [`backoff_with_jitter`].
const BASE_BACKOFF: Duration = Duration::from_millis(200);
/// Max attempts (1 initial + retries) for a single `az` call that keeps
/// reporting a throttling signal.
const MAX_THROTTLE_ATTEMPTS: u32 = 4;

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

    let limiter = AimdLimiter::new();
    let group_out = call_with_throttle_retry(runner, &["group", "list", "-o", "json"], &limiter)
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

    // Rooms are written by their original index from worker threads, never
    // pushed — so the final, filtered order is byte-identical to the old
    // sequential loop's output regardless of which worker finishes which
    // group first.
    let slots: Mutex<Vec<Option<Room>>> = Mutex::new(vec![None; groups.len()]);
    let subscription: Mutex<String> = Mutex::new("unknown".to_string());
    let partial = AtomicBool::new(false);
    let cursor = AtomicUsize::new(0);

    // Never spawn more worker threads than there are groups to process, and
    // fall back to a small fixed pool if the host can't report its own
    // available parallelism.
    let worker_count = thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .min(groups.len().max(1))
        .max(1);

    thread::scope(|scope| {
        for _ in 0..worker_count {
            let groups = &groups;
            let slots = &slots;
            let subscription = &subscription;
            let partial = &partial;
            let cursor = &cursor;
            let limiter = &limiter;
            scope.spawn(move || loop {
                if cancel.is_cancelled() {
                    partial.store(true, Ordering::SeqCst);
                    break;
                }
                let idx = cursor.fetch_add(1, Ordering::SeqCst);
                if idx >= groups.len() {
                    break;
                }
                // Re-check right before doing any work for this group, so a
                // cancellation requested while other workers were busy is
                // still honored promptly rather than only at the next loop
                // iteration boundary.
                if cancel.is_cancelled() {
                    partial.store(true, Ordering::SeqCst);
                    break;
                }

                let processed = process_one_group(runner, &groups[idx], budget, limiter);

                if let Some(sub) = processed.subscription {
                    let mut guard = subscription.lock().unwrap_or_else(|e| e.into_inner());
                    if *guard == "unknown" {
                        *guard = sub;
                    }
                }
                if let Some(room) = processed.room {
                    slots.lock().unwrap_or_else(|e| e.into_inner())[idx] = Some(room);
                }
            });
        }
    });

    let mut rooms: Vec<Room> = slots
        .into_inner()
        .unwrap_or_else(|e| e.into_inner())
        .into_iter()
        .flatten()
        .collect();
    let subscription = subscription.into_inner().unwrap_or_else(|e| e.into_inner());
    let partial = partial.load(Ordering::SeqCst);

    resolve_room_collisions(&mut rooms);
    let edges = same_region_edges(&rooms);

    Ok(DungeonMap {
        subscription,
        rooms,
        edges,
        partial,
    })
}

/// The outcome of enumerating a single resource group: the [`Room`] it
/// produced (`None` if the group's own JSON row was malformed and had to be
/// skipped, matching the pre-parallelization behavior), and a subscription
/// id/name discovered from one of its resources, if any.
struct ProcessedGroup {
    room: Option<Room>,
    subscription: Option<String>,
}

/// Enumerate one resource group's resources into a [`Room`]. Pure aside from
/// the `runner.run` call, so it's independently testable and safely callable
/// from any worker thread — it borrows only `runner` (`Send + Sync`) and a
/// `&AimdLimiter` (internally synchronized).
fn process_one_group(
    runner: &dyn AzRunner,
    group: &Value,
    budget: usize,
    limiter: &AimdLimiter,
) -> ProcessedGroup {
    let name = match group.get("name").and_then(Value::as_str) {
        Some(n) if validate::is_valid_resource_group_name(n) => n.to_string(),
        // Malformed row: no usable room id, skip it (matches the original
        // sequential `continue`).
        _ => {
            return ProcessedGroup {
                room: None,
                subscription: None,
            }
        }
    };
    let region = group
        .get("location")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();

    let (x, y) = deterministic_position(&name, &region);
    let mut resources: Vec<ResourceNode> = Vec::with_capacity(budget.min(64));
    let mut found_subscription = None;

    let res_out = call_with_throttle_retry(
        runner,
        &["resource", "list", "--resource-group", &name, "-o", "json"],
        limiter,
    );
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
                    if found_subscription.is_none() {
                        found_subscription = subscription_from_id(parsed_id.raw());
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

    ProcessedGroup {
        room: Some(Room {
            name: name.clone(),
            id: name,
            region,
            x,
            y,
            resources,
        }),
        subscription: found_subscription,
    }
}

/// Run `runner.run(args)` under the concurrency gate of `limiter`, retrying
/// with jittered backoff (up to [`MAX_THROTTLE_ATTEMPTS`]) if the response
/// looks like Azure throttling ([`ThrottleDetector`]), and reporting the
/// outcome to `limiter` so its AIMD ceiling adapts. Any other failure
/// (non-throttling non-zero exit, or an `io::Error` from the runner itself,
/// e.g. a timeout) is returned immediately without retrying here — that
/// classification is unchanged from the pre-parallelization behavior and is
/// left to the caller.
fn call_with_throttle_retry(
    runner: &dyn AzRunner,
    args: &[&str],
    limiter: &AimdLimiter,
) -> std::io::Result<Output> {
    let mut attempt = 0u32;
    loop {
        attempt += 1;
        let permit = limiter.acquire();
        let result = runner.run(args);
        drop(permit); // Release the slot before any retry sleep.

        match &result {
            Ok(out) if out.status.success() => {
                limiter.report_success();
                return result;
            }
            Ok(out) => {
                let text = format!(
                    "{}{}",
                    String::from_utf8_lossy(&out.stdout),
                    String::from_utf8_lossy(&out.stderr)
                );
                match ThrottleDetector::detect(&text) {
                    Some(floor) if attempt < MAX_THROTTLE_ATTEMPTS => {
                        limiter.report_throttle();
                        thread::sleep(backoff_with_jitter(BASE_BACKOFF, attempt, floor));
                    }
                    _ => return result,
                }
            }
            Err(_) => return result,
        }
    }
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

/// Approximate real-world (longitude, latitude) for common Azure regions.
/// Used only to bias dungeon layout so regions cluster in roughly the
/// correct compass direction relative to one another — never for any
/// live/geo lookup, and never touching the network.
const REGION_COORDS: &[(&str, f64, f64)] = &[
    ("eastus", -79.0, 37.5),
    ("eastus2", -78.0, 36.6),
    ("westus", -122.4, 47.2),
    ("westus2", -119.7, 45.8),
    ("westus3", -112.0, 33.4),
    ("centralus", -93.6, 41.6),
    ("northcentralus", -87.6, 41.9),
    ("southcentralus", -97.6, 29.4),
    ("westeurope", 4.9, 52.4),
    ("northeurope", -6.2, 53.3),
    ("uksouth", -0.8, 51.5),
    ("ukwest", -3.2, 53.4),
    ("southeastasia", 103.8, 1.3),
    ("eastasia", 114.2, 22.3),
    ("japaneast", 139.8, 35.7),
    ("japanwest", 135.5, 34.7),
    ("australiaeast", 151.2, -33.9),
    ("australiasoutheast", 145.0, -37.8),
    ("brazilsouth", -46.6, -23.5),
    ("canadacentral", -79.4, 43.7),
    ("canadaeast", -71.2, 46.8),
    ("southafricanorth", 28.2, -25.7),
    ("centralindia", 73.9, 18.5),
    ("koreacentral", 127.0, 37.5),
];

/// Bucket `region`'s real-world (lon, lat) into a coarse grid offset used to
/// bias dungeon layout, so westerly regions draw west (smaller x) of
/// easterly regions, and northerly regions draw north (smaller y) of
/// southerly ones. Unknown regions return `(0, 0)` (no bias — falls back
/// fully to hash-only [`deterministic_position`] behavior). Never panics,
/// never touches the network; a pure lookup + arithmetic function.
///
/// Coordinates are normalized (`lon + 180`, `90 - lat`, both always >= 0)
/// then bucketed in 20-degree cells so the bias stays compact and doesn't
/// dominate/sprawl the existing tight hash-grid.
fn region_bias(region: &str) -> (i32, i32) {
    match REGION_COORDS.iter().find(|(name, _, _)| *name == region) {
        Some(&(_, lon, lat)) => {
            let bx = ((lon + 180.0) / 20.0).floor() as i32;
            let by = ((90.0 - lat) / 20.0).floor() as i32;
            (bx, by)
        }
        None => (0, 0),
    }
}

/// A stable, non-random grid position derived purely from `(name, region)`,
/// biased by [`region_bias`] so the overall dungeon layout *leans* toward real
/// Azure region geography (west/east, north/south) while resources within a
/// region keep their existing hash-based scatter. Never influenced by
/// enumeration order or process/run state, so the same subscription always
/// lays out identically.
///
/// Each region is offset into a [`WING_SIZE`]x[`WING_SIZE`] band chosen by its
/// coarse geographic bucket. Distinct buckets tile without gaps or overlap, so
/// the west->east / north->south lean holds between any two regions that fall
/// in different buckets. It stays a directional *hint* rather than a hard
/// ordering guarantee for two reasons. First, the bucket is coarse (20-degree
/// cells), so geographically close regions (e.g. `eastus` / `eastus2` /
/// `canadacentral` / `canadaeast`) can share a band and cluster together
/// instead of each getting a private wing — which is intended, since they *are*
/// close. Second, for a dense subscription, [`resolve_room_collisions`] may
/// walk a colliding room out of its band into a neighbouring one. So a handful
/// of resource groups still draw as a tight, walkable dungeon that roughly
/// mirrors real geography, rather than a few rooms scattered across a
/// mostly-empty parchment.
const WING_SIZE: i32 = 6;

fn deterministic_position(name: &str, region: &str) -> (i32, i32) {
    let mut hasher = DefaultHasher::new();
    (name, region).hash(&mut hasher);
    let h = hasher.finish();
    let fine_x = ((h & 0xFFFF) % WING_SIZE as u64) as i32;
    let fine_y = (((h >> 16) & 0xFFFF) % WING_SIZE as u64) as i32;

    let (bias_x, bias_y) = region_bias(region);
    (bias_x * WING_SIZE + fine_x, bias_y * WING_SIZE + fine_y)
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

#[cfg(test)]
mod region_geography_tests {
    use super::*;

    #[test]
    fn west_region_is_west_of_east_region() {
        // Same continent (US), so naming and real-world geography agree.
        let west = deterministic_position("r", "westus");
        let east = deterministic_position("r", "eastus");
        assert!(
            west.0 < east.0,
            "westus.x={} should be < eastus.x={}",
            west.0,
            east.0
        );
    }

    #[test]
    fn north_region_is_north_of_south_region() {
        // Same continent (US), so naming and real-world geography agree.
        let north = region_bias("northcentralus");
        let south = region_bias("southcentralus");
        assert!(
            north.1 < south.1,
            "northcentralus.y={} should be < southcentralus.y={}",
            north.1,
            south.1
        );
    }

    #[test]
    fn european_region_is_east_of_us_region() {
        // Cross-continent sanity: real westeurope (Amsterdam) lies east of real
        // eastus (Virginia). Naming ("west"europe) is intentionally misleading
        // here — the bias follows real longitude, not the region name — so this
        // guards against anyone "fixing" the table by name instead of geography.
        let eu = region_bias("westeurope");
        let us = region_bias("eastus");
        assert!(
            eu.0 > us.0,
            "westeurope.bias_x={} should be east of eastus.bias_x={}",
            eu.0,
            us.0
        );
    }

    #[test]
    fn geographically_close_regions_share_a_coarse_bucket() {
        // Documents (and locks in) the intended coarse-bucketing behavior: the
        // 20-degree cells are deliberately coarse, so nearby regions cluster
        // into the same band rather than each getting a private wing. This is a
        // directional hint, not per-region isolation.
        assert_eq!(region_bias("eastus"), region_bias("eastus2"));
        assert_eq!(region_bias("eastus"), region_bias("canadacentral"));
    }

    #[test]
    fn determinism_across_calls() {
        let a = deterministic_position("myapp-rg", "westeurope");
        let b = deterministic_position("myapp-rg", "westeurope");
        assert_eq!(a, b);
    }

    #[test]
    fn unknown_region_falls_back_to_hash_only() {
        assert_eq!(region_bias("mars-central"), (0, 0));

        // With a (0,0) bias, deterministic_position must equal the raw hash
        // fine offset (pre-bias behavior) exactly.
        let name = "some-rg";
        let region = "mars-central";
        let mut hasher = DefaultHasher::new();
        (name, region).hash(&mut hasher);
        let h = hasher.finish();
        let expected = (
            ((h & 0xFFFF) % WING_SIZE as u64) as i32,
            (((h >> 16) & 0xFFFF) % WING_SIZE as u64) as i32,
        );
        assert_eq!(deterministic_position(name, region), expected);
    }

    #[test]
    fn region_coords_table_has_no_duplicate_keys() {
        let mut names: Vec<&str> = REGION_COORDS.iter().map(|(n, _, _)| *n).collect();
        let original_len = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(
            names.len(),
            original_len,
            "REGION_COORDS has duplicate region keys"
        );
    }

    #[test]
    fn bias_is_never_negative_given_valid_lon_lat() {
        for &(name, _, _) in REGION_COORDS {
            let (bx, by) = region_bias(name);
            assert!(bx >= 0, "{name} produced negative bias_x={bx}");
            assert!(by >= 0, "{name} produced negative bias_y={by}");
        }
    }
}
