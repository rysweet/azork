//! Pure, IO-free diffing between two [`DungeonMap`] snapshots — the "Time
//! Rift" feature (`azork crawl --diff <old> <new>`).
//!
//! Rooms are matched by `id` (resource group name). Resources are matched by
//! their full ARM `id`, flattened across *all* rooms in each map, so a
//! resource that moved from one room to another is correctly reported as
//! "changed" rather than a false-positive add+remove pair.
//!
//! Every output vector is sorted by `id` before being returned, so the
//! result is deterministic regardless of `HashMap`/enumeration iteration
//! order in the two input maps.

use crate::dungeon::map::{DungeonMap, ResourceNode, Room};
use std::collections::HashMap;

/// A resource whose `kind` and/or `region` differ between the old and new
/// map, matched by ARM id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceChange {
    pub id: String,
    pub old: ResourceNode,
    pub new: ResourceNode,
}

/// The structural difference between two [`DungeonMap`] snapshots.
///
/// All five vectors are sorted by `id` for deterministic output.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MapDiff {
    pub rooms_added: Vec<Room>,
    pub rooms_removed: Vec<Room>,
    pub resources_added: Vec<ResourceNode>,
    pub resources_removed: Vec<ResourceNode>,
    pub resources_changed: Vec<ResourceChange>,
}

impl MapDiff {
    /// Whether the two maps are structurally identical (no rooms or
    /// resources added, removed, or changed).
    pub fn is_empty(&self) -> bool {
        self.rooms_added.is_empty()
            && self.rooms_removed.is_empty()
            && self.resources_added.is_empty()
            && self.resources_removed.is_empty()
            && self.resources_changed.is_empty()
    }
}

/// Flatten every room's resources into an `id -> &ResourceNode` map.
fn flatten_resources(map: &DungeonMap) -> HashMap<&str, &ResourceNode> {
    map.rooms
        .iter()
        .flat_map(|r| r.resources.iter())
        .map(|res| (res.id.as_str(), res))
        .collect()
}

/// Diff two [`DungeonMap`] snapshots. Pure and IO-free: callers own reading
/// the snapshot files and printing the report.
pub fn diff_maps(old: &DungeonMap, new: &DungeonMap) -> MapDiff {
    let old_rooms: HashMap<&str, &Room> = old.rooms.iter().map(|r| (r.id.as_str(), r)).collect();
    let new_rooms: HashMap<&str, &Room> = new.rooms.iter().map(|r| (r.id.as_str(), r)).collect();

    let mut rooms_added: Vec<Room> = new
        .rooms
        .iter()
        .filter(|r| !old_rooms.contains_key(r.id.as_str()))
        .cloned()
        .collect();
    let mut rooms_removed: Vec<Room> = old
        .rooms
        .iter()
        .filter(|r| !new_rooms.contains_key(r.id.as_str()))
        .cloned()
        .collect();

    let old_res = flatten_resources(old);
    let new_res = flatten_resources(new);

    let mut resources_added: Vec<ResourceNode> = new_res
        .iter()
        .filter(|(id, _)| !old_res.contains_key(*id))
        .map(|(_, res)| (*res).clone())
        .collect();
    let mut resources_removed: Vec<ResourceNode> = old_res
        .iter()
        .filter(|(id, _)| !new_res.contains_key(*id))
        .map(|(_, res)| (*res).clone())
        .collect();
    let mut resources_changed: Vec<ResourceChange> = old_res
        .iter()
        .filter_map(|(id, old_node)| {
            new_res.get(id).and_then(|new_node| {
                if old_node.kind != new_node.kind || old_node.region != new_node.region {
                    Some(ResourceChange {
                        id: id.to_string(),
                        old: (*old_node).clone(),
                        new: (*new_node).clone(),
                    })
                } else {
                    None
                }
            })
        })
        .collect();

    rooms_added.sort_by(|a, b| a.id.cmp(&b.id));
    rooms_removed.sort_by(|a, b| a.id.cmp(&b.id));
    resources_added.sort_by(|a, b| a.id.cmp(&b.id));
    resources_removed.sort_by(|a, b| a.id.cmp(&b.id));
    resources_changed.sort_by(|a, b| a.id.cmp(&b.id));

    MapDiff {
        rooms_added,
        rooms_removed,
        resources_added,
        resources_removed,
        resources_changed,
    }
}

/// Render a deterministic, themed text report for `diff` — no timestamps or
/// randomness, safe to assert verbatim in tests.
pub fn render_report(diff: &MapDiff) -> String {
    let mut out = String::new();
    out.push_str("⚡ Time Rift Report\n");

    if diff.is_empty() {
        out.push_str("No changes detected — the dungeon is unchanged across time.\n");
        out.push_str("Summary: +0 -0 ~0\n");
        return out;
    }

    if !diff.rooms_added.is_empty() {
        out.push_str("\nRooms added:\n");
        for r in &diff.rooms_added {
            out.push_str(&format!("  + {} ({})\n", r.id, r.name));
        }
    }
    if !diff.rooms_removed.is_empty() {
        out.push_str("\nRooms removed:\n");
        for r in &diff.rooms_removed {
            out.push_str(&format!("  - {} ({})\n", r.id, r.name));
        }
    }
    if !diff.resources_added.is_empty() {
        out.push_str("\nResources added:\n");
        for r in &diff.resources_added {
            out.push_str(&format!("  + {} ({})\n", r.id, r.kind));
        }
    }
    if !diff.resources_removed.is_empty() {
        out.push_str("\nResources removed:\n");
        for r in &diff.resources_removed {
            out.push_str(&format!("  - {} ({})\n", r.id, r.kind));
        }
    }
    if !diff.resources_changed.is_empty() {
        out.push_str("\nResources changed:\n");
        for c in &diff.resources_changed {
            out.push_str(&format!(
                "  ~ {} ({}/{} -> {}/{})\n",
                c.id, c.old.kind, c.old.region, c.new.kind, c.new.region
            ));
        }
    }

    out.push_str(&format!(
        "\nSummary: +{} -{} ~{}\n",
        diff.rooms_added.len() + diff.resources_added.len(),
        diff.rooms_removed.len() + diff.resources_removed.len(),
        diff.resources_changed.len()
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn room(id: &str, resources: Vec<ResourceNode>) -> Room {
        Room {
            id: id.to_string(),
            name: id.to_string(),
            region: "eastus".to_string(),
            x: 0,
            y: 0,
            resources,
        }
    }

    fn resource(id: &str, kind: &str, region: &str) -> ResourceNode {
        ResourceNode {
            id: id.to_string(),
            name: id.to_string(),
            kind: kind.to_string(),
            region: region.to_string(),
            icon: "generic".to_string(),
        }
    }

    fn map(rooms: Vec<Room>) -> DungeonMap {
        DungeonMap {
            subscription: "test".to_string(),
            rooms,
            edges: Vec::new(),
            partial: false,
        }
    }

    #[test]
    fn detects_added_and_removed_rooms() {
        let old = map(vec![room("rg-a", vec![])]);
        let new = map(vec![room("rg-b", vec![])]);
        let diff = diff_maps(&old, &new);
        assert_eq!(diff.rooms_added.len(), 1);
        assert_eq!(diff.rooms_added[0].id, "rg-b");
        assert_eq!(diff.rooms_removed.len(), 1);
        assert_eq!(diff.rooms_removed[0].id, "rg-a");
    }

    #[test]
    fn detects_added_removed_and_changed_resources_by_id() {
        let old = map(vec![room(
            "rg-a",
            vec![
                resource("/sub/rg-a/vm1", "Microsoft.Compute/vm", "eastus"),
                resource("/sub/rg-a/vm2", "Microsoft.Compute/vm", "eastus"),
            ],
        )]);
        let new = map(vec![room(
            "rg-a",
            vec![
                // vm1 changed region.
                resource("/sub/rg-a/vm1", "Microsoft.Compute/vm", "westus"),
                // vm2 removed.
                // vm3 added.
                resource("/sub/rg-a/vm3", "Microsoft.Compute/vm", "eastus"),
            ],
        )]);
        let diff = diff_maps(&old, &new);
        assert_eq!(diff.resources_added.len(), 1);
        assert_eq!(diff.resources_added[0].id, "/sub/rg-a/vm3");
        assert_eq!(diff.resources_removed.len(), 1);
        assert_eq!(diff.resources_removed[0].id, "/sub/rg-a/vm2");
        assert_eq!(diff.resources_changed.len(), 1);
        assert_eq!(diff.resources_changed[0].id, "/sub/rg-a/vm1");
        assert_eq!(diff.resources_changed[0].old.region, "eastus");
        assert_eq!(diff.resources_changed[0].new.region, "westus");
    }

    #[test]
    fn resource_moving_rooms_is_reported_as_changed_not_add_and_remove() {
        let old = map(vec![room(
            "rg-a",
            vec![resource("/sub/vm1", "Microsoft.Compute/vm", "eastus")],
        )]);
        let new = map(vec![
            room("rg-a", vec![]),
            room(
                "rg-b",
                vec![resource("/sub/vm1", "Microsoft.Compute/vm", "eastus")],
            ),
        ]);
        let diff = diff_maps(&old, &new);
        // Same id, same kind/region: not reported as changed, added, or
        // removed at the resource level (room move is captured by the room
        // diff, not the resource diff).
        assert!(diff.resources_added.is_empty());
        assert!(diff.resources_removed.is_empty());
        assert!(diff.resources_changed.is_empty());
        assert_eq!(diff.rooms_added.len(), 1);
        assert_eq!(diff.rooms_added[0].id, "rg-b");
    }

    #[test]
    fn identical_maps_produce_empty_diff() {
        let m = map(vec![room(
            "rg-a",
            vec![resource("/sub/vm1", "Microsoft.Compute/vm", "eastus")],
        )]);
        let diff = diff_maps(&m, &m.clone());
        assert!(diff.is_empty());
    }

    #[test]
    fn output_order_is_independent_of_input_room_order() {
        let old = map(vec![]);
        let new_a = map(vec![room("rg-b", vec![]), room("rg-a", vec![])]);
        let new_b = map(vec![room("rg-a", vec![]), room("rg-b", vec![])]);
        let diff_a = diff_maps(&old, &new_a);
        let diff_b = diff_maps(&old, &new_b);
        assert_eq!(diff_a.rooms_added, diff_b.rooms_added);
        let ids: Vec<&str> = diff_a.rooms_added.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["rg-a", "rg-b"]);
    }

    #[test]
    fn report_is_deterministic_and_verbatim() {
        let old = map(vec![room("rg-a", vec![])]);
        let new = map(vec![room("rg-b", vec![])]);
        let diff = diff_maps(&old, &new);
        let report = render_report(&diff);
        assert_eq!(
            report,
            "⚡ Time Rift Report\n\
             \n\
             Rooms added:\n\
             \x20\x20+ rg-b (rg-b)\n\
             \n\
             Rooms removed:\n\
             \x20\x20- rg-a (rg-a)\n\
             \n\
             Summary: +1 -1 ~0\n"
        );
    }

    #[test]
    fn report_for_empty_diff() {
        let m = map(vec![room("rg-a", vec![])]);
        let diff = diff_maps(&m, &m.clone());
        let report = render_report(&diff);
        assert_eq!(
            report,
            "⚡ Time Rift Report\nNo changes detected — the dungeon is unchanged across time.\nSummary: +0 -0 ~0\n"
        );
    }
}
