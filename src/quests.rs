//! Gamified, read-only governance objectives ("quests").
//!
//! Each [`Quest`] wraps a predicate over a single [`Resource`] hazard field
//! (public exposure, encryption-at-rest, management locks) and evaluates it
//! against every resource currently in the [`World`] (rooms + inventory).
//! Quests are purely observational: they never mutate the world, add new
//! hazard sources, or perform any I/O.

use crate::world::{Resource, World};

/// Progress toward completing a single quest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    /// Number of resources that satisfy the quest's condition.
    pub done: u32,
    /// Total number of resources considered.
    pub total: u32,
    /// True when every considered resource satisfies the condition. A world
    /// with zero resources is vacuously complete.
    pub complete: bool,
}

/// A single governance objective, evaluated read-only against a [`World`].
pub struct Quest {
    /// Player-facing quest name (e.g. `"Secure the Realm"`).
    pub name: &'static str,
    /// Player-facing description of what the quest asks for.
    pub description: &'static str,
    condition: fn(&Resource) -> bool,
}

impl Quest {
    /// Evaluate this quest's progress against the current world state.
    pub fn evaluate(&self, world: &World) -> Progress {
        let resources = world.all_resources();
        let total = resources.len() as u32;
        let done = resources.iter().filter(|r| (self.condition)(r)).count() as u32;
        Progress {
            done,
            total,
            complete: done == total,
        }
    }
}

/// The built-in set of governance quests.
pub fn builtin_quests() -> Vec<Quest> {
    vec![
        Quest {
            name: "Secure the Realm",
            description: "Ensure no resource is exposed to the public internet.",
            condition: |r| !r.public,
        },
        Quest {
            name: "Seal the Vaults",
            description: "Ensure every resource has data at rest encrypted.",
            condition: |r| r.encrypted,
        },
        Quest {
            name: "Lift the Curse",
            description: "Ensure every resource is protected by a management lock.",
            condition: |r| r.locked,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Direction;
    use crate::world::Room;

    fn hazardous(name: &str) -> Resource {
        let mut r = Resource::new(name, "Microsoft.Storage/storageAccounts", "Hazardous.");
        r.public = true;
        r.encrypted = false;
        r.locked = false;
        r
    }

    fn clean(name: &str) -> Resource {
        let mut r = Resource::new(name, "Microsoft.Compute/virtualMachines", "Well-governed.");
        r.public = false;
        r.encrypted = true;
        r.locked = true;
        r
    }

    #[test]
    fn builtin_quests_has_three_quests_with_names_and_descriptions() {
        let quests = builtin_quests();
        assert_eq!(quests.len(), 3);
        for q in &quests {
            assert!(!q.name.is_empty());
            assert!(!q.description.is_empty());
        }
    }

    #[test]
    fn partial_progress_on_mixed_world() {
        let room = Room::new("prod-rg", "A room.", "eastus", true)
            .with_resource(hazardous("public-storage"))
            .with_resource(clean("locked-vm"));
        let w = World::new(vec![room], "prod-rg", "sub-test").expect("valid room graph");

        for quest in builtin_quests() {
            let progress = quest.evaluate(&w);
            assert_eq!(progress.total, 2);
            assert_eq!(progress.done, 1);
            assert!(!progress.complete);
        }
    }

    #[test]
    fn full_completion_on_all_clean_world() {
        let a = Room::new("rg-a", "A room.", "eastus", true)
            .with_exit(Direction::North, "rg-b")
            .with_resource(clean("vm-a"));
        let b = Room::new("rg-b", "A room.", "westus", true)
            .with_exit(Direction::South, "rg-a")
            .with_resource(clean("vm-b"));
        let w = World::new(vec![a, b], "rg-a", "sub-test").expect("valid room graph");

        for quest in builtin_quests() {
            let progress = quest.evaluate(&w);
            assert_eq!(progress.done, progress.total);
            assert!(progress.complete);
        }
    }

    #[test]
    fn empty_world_is_vacuously_complete() {
        let room = Room::new("rg-empty", "An empty room.", "eastus", true);
        let w = World::new(vec![room], "rg-empty", "sub-test").expect("valid room graph");

        for quest in builtin_quests() {
            let progress = quest.evaluate(&w);
            assert_eq!(progress.total, 0);
            assert_eq!(progress.done, 0);
            assert!(progress.complete);
        }
    }
}
