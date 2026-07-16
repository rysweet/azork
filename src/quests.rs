//! Quests — gamified, read-only governance objectives.
//!
//! A quest scores the *current* [`World`] state against a single governance
//! goal (no public exposure, universal encryption, etc). Quests never mutate
//! the world; they only read the same hazard fields `World::score()` already
//! derives from.

use crate::world::Resource;

/// Progress toward completing a quest: how many resources currently satisfy
/// the quest's goal out of the total tracked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuestProgress {
    pub done: usize,
    pub total: usize,
    pub complete: bool,
}

/// A single governance objective, themed as a dungeon quest.
pub struct Quest {
    pub name: &'static str,
    pub description: &'static str,
    /// Themed line shown once the quest is complete.
    pub completion_line: &'static str,
    /// Predicate: does this resource satisfy the quest's goal?
    satisfies: fn(&Resource) -> bool,
}

impl Quest {
    /// Evaluate this quest against an already-collected slice of resources.
    ///
    /// Callers evaluating multiple quests should collect `World::all_resources()`
    /// once and reuse the slice, rather than re-deriving it per quest.
    pub fn evaluate(&self, resources: &[&Resource]) -> QuestProgress {
        let total = resources.len();
        let done = resources.iter().filter(|r| (self.satisfies)(r)).count();
        QuestProgress {
            done,
            total,
            // A world with no resources at all counts as vacuously complete.
            complete: done == total,
        }
    }
}

/// The built-in set of quests. Deliberately a flat `Vec`, not a registry —
/// this is a thin prototype, not a plugin system.
pub fn builtin_quests() -> Vec<Quest> {
    vec![
        Quest {
            name: "Secure the Realm",
            description: "No resource may face the public internet.",
            completion_line: "The realm's walls stand unbreached. Not a single \
                              gate is left open to the wilds beyond.",
            satisfies: |r| !r.public,
        },
        Quest {
            name: "Seal the Vaults",
            description: "Every resource's data must be encrypted at rest.",
            completion_line: "The vaults are sealed. Every ledger and hoard \
                              lies safe behind unbroken wards.",
            satisfies: |r| r.encrypted,
        },
        Quest {
            name: "Lift the Curse",
            description: "No resource may be left unlocked and vulnerable.",
            completion_line: "The curse is lifted. Every chamber is warded \
                              and locked against plunder.",
            satisfies: |r| r.locked,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{Room, World};

    /// A tiny world with one hazard-laden and one clean resource in a room,
    /// plus a hazard-laden resource in inventory.
    fn hazard_world() -> World {
        let mut room = Room::new("vault", "A dim vault.", "eastus", true);
        room = room.with_resource(Resource {
            name: "leaky-blob".to_string(),
            kind: "Microsoft.Storage/storageAccounts".to_string(),
            description: "A cracked jar.".to_string(),
            locked: false,
            public: true,
            encrypted: false,
            monthly_cost: 10,
        });
        room = room.with_resource(Resource {
            name: "sound-vm".to_string(),
            kind: "Microsoft.Compute/virtualMachines".to_string(),
            description: "A sturdy golem.".to_string(),
            locked: true,
            public: false,
            encrypted: true,
            monthly_cost: 10,
        });
        let mut world = World::new(vec![room], "vault", "test-sub").unwrap();
        world.seed_rng(1);
        world
    }

    #[test]
    fn hazard_world_yields_partial_progress() {
        let world = hazard_world();
        let resources = world.all_resources();
        let quests = builtin_quests();

        let secure = quests[0].evaluate(&resources);
        assert_eq!(secure.done, 1); // only sound-vm is non-public
        assert_eq!(secure.total, 2);
        assert!(!secure.complete);

        let vaults = quests[1].evaluate(&resources);
        assert_eq!(vaults.done, 1); // only sound-vm is encrypted
        assert_eq!(vaults.total, 2);
        assert!(!vaults.complete);

        let curse = quests[2].evaluate(&resources);
        assert_eq!(curse.done, 1); // only sound-vm is locked
        assert_eq!(curse.total, 2);
        assert!(!curse.complete);
    }

    #[test]
    fn clean_world_completes_all_quests() {
        let mut room = Room::new("hall", "A bright hall.", "eastus", true);
        room = room.with_resource(Resource {
            name: "vm".to_string(),
            kind: "Microsoft.Compute/virtualMachines".to_string(),
            description: "A well-kept golem.".to_string(),
            locked: true,
            public: false,
            encrypted: true,
            monthly_cost: 5,
        });
        let world = World::new(vec![room], "hall", "test-sub").unwrap();
        let resources = world.all_resources();

        for quest in builtin_quests() {
            let progress = quest.evaluate(&resources);
            assert!(
                progress.complete,
                "quest '{}' should be complete",
                quest.name
            );
            assert_eq!(progress.done, progress.total);
        }
    }

    #[test]
    fn empty_world_is_vacuously_complete() {
        let room = Room::new("hall", "An empty hall.", "eastus", true);
        let world = World::new(vec![room], "hall", "test-sub").unwrap();
        let resources = world.all_resources();
        for quest in builtin_quests() {
            let progress = quest.evaluate(&resources);
            assert_eq!(progress.total, 0);
            assert!(progress.complete);
        }
    }
}
