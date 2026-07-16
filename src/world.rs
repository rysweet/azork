//! World model for AzZork.
//!
//! The Azure control plane is modelled as a dungeon:
//! * **Rooms** are resource groups (each pinned to a region).
//! * **Objects/creatures** are resources (VMs, storage accounts, key vaults...).
//! * **Exits** connect rooms and represent navigation across resource groups,
//!   regions and subscriptions.
//! * **Grues** lurk in *dark* rooms — resource groups with no monitoring /
//!   diagnostics enabled. Act in the dark long enough and a Grue devours you.
//!
//! Governance hazards on resources (public exposure, missing encryption, cost
//! overruns, being unlocked) feed the [`World::score`] governance posture.

use crate::parser::Direction;
use std::collections::HashMap;

/// Case-insensitive resource-name match: exact or prefix.
///
/// `query` is expected to already be lowercased so the per-candidate name is
/// only lowercased once per comparison.
fn name_matches(name: &str, query: &str) -> bool {
    let lname = name.to_lowercase();
    lname == query || lname.starts_with(query)
}

/// Resolve a resource index by name, preferring an exact (case-insensitive)
/// match before falling back to first prefix match.
///
/// Exact-match precedence avoids silently targeting the wrong resource when one
/// name is a prefix of another (e.g. `storage` vs `storage-logs`) — a real risk
/// on live `az` estates even though the mock estate has no such collisions.
/// `query` is expected to already be lowercased.
fn find_by_name(resources: &[Resource], query: &str) -> Option<usize> {
    resources
        .iter()
        .position(|r| r.name.to_lowercase() == query)
        .or_else(|| resources.iter().position(|r| name_matches(&r.name, query)))
}

/// A single Azure resource, rendered as a dungeon object or creature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resource {
    /// Short identifier the player types (e.g. `storage`, `vm`).
    pub name: String,
    /// Azure resource type flavor (e.g. `Microsoft.Storage/storageAccounts`).
    pub kind: String,
    /// Prose shown when the resource is examined.
    pub description: String,
    /// Whether a management/RBAC lock protects the resource.
    pub locked: bool,
    /// Whether the resource is exposed to the public internet.
    pub public: bool,
    /// Whether data at rest is encrypted.
    pub encrypted: bool,
    /// Estimated monthly cost in USD (drives cost-overrun Grues).
    pub monthly_cost: u32,
}

impl Resource {
    /// Convenience constructor with sensible-ish (often insecure) defaults so
    /// the player has hazards to fix.
    pub fn new(name: &str, kind: &str, description: &str) -> Resource {
        Resource {
            name: name.to_string(),
            kind: kind.to_string(),
            description: description.to_string(),
            locked: false,
            public: false,
            encrypted: true,
            monthly_cost: 0,
        }
    }

    /// Count of governance hazards this resource currently exhibits.
    pub fn hazards(&self) -> u32 {
        let mut h = 0;
        if self.public {
            h += 1;
        }
        if !self.encrypted {
            h += 1;
        }
        if !self.locked {
            h += 1;
        }
        if self.monthly_cost >= 500 {
            h += 1;
        }
        h
    }

    /// One-line hazard summary for examine output.
    pub fn hazard_report(&self) -> String {
        let mut parts = Vec::new();
        if self.public {
            parts.push("exposed to the public internet".to_string());
        }
        if !self.encrypted {
            parts.push("storing its data unencrypted".to_string());
        }
        if !self.locked {
            parts.push("unlocked and vulnerable to deletion".to_string());
        }
        if self.monthly_cost >= 500 {
            parts.push(format!("bleeding ${}/mo in cost", self.monthly_cost));
        }
        if parts.is_empty() {
            "It looks well-governed and calm.".to_string()
        } else {
            format!("A Grue senses it is {}.", parts.join(", "))
        }
    }
}

/// A room in the dungeon — an Azure resource group.
#[derive(Debug, Clone)]
pub struct Room {
    /// Resource group name / room id.
    pub name: String,
    /// Prose describing the room.
    pub description: String,
    /// Azure region this group lives in.
    pub region: String,
    /// Whether monitoring/diagnostics are enabled. If `false`, the room is dark
    /// and a Grue lurks.
    pub monitored: bool,
    /// Direction -> destination room name.
    pub exits: HashMap<Direction, String>,
    /// Resources present in the room.
    pub resources: Vec<Resource>,
}

impl Room {
    pub fn new(name: &str, description: &str, region: &str, monitored: bool) -> Room {
        Room {
            name: name.to_string(),
            description: description.to_string(),
            region: region.to_string(),
            monitored,
            exits: HashMap::new(),
            resources: Vec::new(),
        }
    }

    /// Builder-style: add an exit.
    pub fn with_exit(mut self, dir: Direction, dest: &str) -> Room {
        self.exits.insert(dir, dest.to_string());
        self
    }

    /// Builder-style: add a resource.
    pub fn with_resource(mut self, res: Resource) -> Room {
        self.resources.push(res);
        self
    }

    /// A room is dark (Grue territory) when it is not monitored.
    pub fn is_dark(&self) -> bool {
        !self.monitored
    }
}

/// Outcome of a single turn's Grue check.
#[derive(Debug, PartialEq, Eq)]
pub enum GrueOutcome {
    /// The room is lit; no danger.
    Safe,
    /// The room is dark; a warning is issued but the player survives.
    Lurking,
    /// The Grue struck. The game is over.
    Devoured,
}

/// The complete, mutable game state.
pub struct World {
    rooms: HashMap<String, Room>,
    current: String,
    /// Subscription name, shown in the banner/prompt.
    pub subscription: String,
    inventory: Vec<Resource>,
    moves: u32,
    /// Turns spent in darkness on the current stretch; the Grue grows bolder.
    darkness_streak: u32,
    /// Simple deterministic RNG state (xorshift) for reproducible Grue attacks.
    rng: u64,
    /// Set when the player is eaten.
    pub game_over: bool,
}

impl World {
    /// Build a world from a list of rooms and the starting room name.
    ///
    /// Validates the room graph's integrity (start room exists, every exit
    /// points at a known room) at runtime in both debug and release builds —
    /// a corrupt graph produces a clear `Err` here instead of a panic later
    /// from `current_room()`.
    pub fn new(rooms: Vec<Room>, start: &str, subscription: &str) -> Result<World, String> {
        let mut map = HashMap::new();
        for r in rooms {
            map.insert(r.name.clone(), r);
        }
        if !map.contains_key(start) {
            return Err(format!("start room '{}' does not exist", start));
        }
        if let Some(dest) = map
            .values()
            .flat_map(|room| room.exits.values())
            .find(|dest| !map.contains_key(dest.as_str()))
        {
            return Err(format!(
                "a room exit points at a non-existent destination room '{}'",
                dest
            ));
        }
        Ok(World {
            rooms: map,
            current: start.to_string(),
            subscription: subscription.to_string(),
            inventory: Vec::new(),
            moves: 0,
            darkness_streak: 0,
            rng: 0x9E3779B97F4A7C15,
            game_over: false,
        })
    }

    /// Seed the RNG deterministically (used by tests).
    pub fn seed_rng(&mut self, seed: u64) {
        self.rng = seed | 1;
    }

    fn next_rand(&mut self) -> u64 {
        // xorshift64
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        x
    }

    /// Immutable reference to the current room.
    pub fn current_room(&self) -> &Room {
        self.rooms
            .get(&self.current)
            .expect("current room always exists")
    }

    /// Number of rooms (resource groups) in the world.
    pub fn rooms_len(&self) -> usize {
        self.rooms.len()
    }

    fn current_room_mut(&mut self) -> &mut Room {
        self.rooms
            .get_mut(&self.current)
            .expect("current room always exists")
    }

    pub fn moves(&self) -> u32 {
        self.moves
    }

    /// Describe the current room (equivalent to listing a resource group).
    pub fn look(&self) -> String {
        let room = self.current_room();
        let mut out = String::new();
        out.push_str(&format!("== {} ({}) ==\n", room.name, room.region));
        if room.is_dark() {
            out.push_str(
                "It is pitch black here — no monitoring, no diagnostics. \
                 You are likely to be eaten by a Grue.\n",
            );
        } else {
            out.push_str(&room.description);
            out.push('\n');
            if room.resources.is_empty() {
                out.push_str("The resource group is empty.\n");
            } else {
                out.push_str("You see:\n");
                for r in &room.resources {
                    out.push_str(&format!("  - {} ({})\n", r.name, r.kind));
                }
            }
        }
        let mut exits: Vec<&str> = room.exits.keys().map(|d| d.name()).collect();
        exits.sort();
        if exits.is_empty() {
            out.push_str("There are no obvious exits.");
        } else {
            out.push_str(&format!("Exits: {}", exits.join(", ")));
        }
        out
    }

    /// Find a resource by name in the current room. Prefers an exact
    /// (case-insensitive) match, then falls back to first prefix match.
    fn find_in_room(&self, target: &str) -> Option<usize> {
        let t = target.to_lowercase();
        find_by_name(&self.current_room().resources, &t)
    }

    fn find_in_inventory(&self, target: &str) -> Option<usize> {
        let t = target.to_lowercase();
        find_by_name(&self.inventory, &t)
    }

    /// Examine an object in the room or inventory (equivalent to `az ... show`).
    pub fn examine(&self, target: &str) -> String {
        if self.current_room().is_dark() {
            return "It's too dark to make anything out. Enable monitoring first.".to_string();
        }
        if let Some(i) = self.find_in_room(target) {
            let r = &self.current_room().resources[i];
            return Self::describe_resource(r);
        }
        if let Some(i) = self.find_in_inventory(target) {
            let r = &self.inventory[i];
            return format!("(carried) {}", Self::describe_resource(r));
        }
        format!("You don't see any '{}' here.", target)
    }

    fn describe_resource(r: &Resource) -> String {
        format!(
            "{} [{}]\n{}\nStatus: {} | {} | {} | ~${}/mo\n{}",
            r.name,
            r.kind,
            r.description,
            if r.public { "PUBLIC" } else { "private" },
            if r.encrypted {
                "encrypted"
            } else {
                "UNENCRYPTED"
            },
            if r.locked { "locked" } else { "unlocked" },
            r.monthly_cost,
            r.hazard_report(),
        )
    }

    /// Move in a direction. Returns the description of the new room, or an error
    /// message if there is no exit that way.
    pub fn go(&mut self, dir: Direction) -> Result<String, String> {
        let dest = self.current_room().exits.get(&dir).cloned();
        match dest {
            Some(name) => {
                self.current = name;
                self.moves += 1;
                Ok(self.look())
            }
            None => Err(format!("You can't go {} from here.", dir.name())),
        }
    }

    /// Take/adopt a resource into inventory.
    pub fn take(&mut self, target: &str) -> String {
        if self.current_room().is_dark() {
            return "You grope blindly in the dark and grasp nothing.".to_string();
        }
        match self.find_in_room(target) {
            Some(i) => {
                let r = self.current_room_mut().resources.remove(i);
                let name = r.name.clone();
                self.inventory.push(r);
                self.moves += 1;
                format!("You acquire the {} and add it to your inventory.", name)
            }
            None => format!("There is no '{}' here to take.", target),
        }
    }

    /// Drop/delete a resource. The caller is responsible for confirmation; this
    /// method performs the deletion once confirmed.
    pub fn drop_item(&mut self, target: &str) -> String {
        // Deleting from inventory (releasing an owned resource).
        if let Some(i) = self.find_in_inventory(target) {
            let r = &self.inventory[i];
            if r.locked {
                return format!(
                    "The {} is locked. Unlock it before you can delete it.",
                    r.name
                );
            }
            let r = self.inventory.remove(i);
            self.moves += 1;
            return format!("You delete the {}. It dissolves into the void.", r.name);
        }
        // Deleting something in the room requires light: destroying what you
        // cannot see is how estates lose data. Mirrors the guard on `take`.
        if self.current_room().is_dark() {
            return "It's too dark to safely delete anything here. Enable monitoring first."
                .to_string();
        }
        // Deleting something in the room.
        if let Some(i) = self.find_in_room(target) {
            let r = &self.current_room().resources[i];
            if r.locked {
                return format!(
                    "The {} is locked. Unlock it before you can delete it.",
                    r.name
                );
            }
            let r = self.current_room_mut().resources.remove(i);
            self.moves += 1;
            return format!("You delete the {}. It dissolves into the void.", r.name);
        }
        format!("There is no '{}' here to delete.", target)
    }

    /// Lock/secure a resource (in room or inventory). Also encrypts it and pulls
    /// it off the public internet — a proper warding.
    pub fn lock(&mut self, target: &str) -> String {
        if let Some(i) = self.find_in_room(target) {
            let r = &mut self.current_room_mut().resources[i];
            r.locked = true;
            r.public = false;
            r.encrypted = true;
            let name = r.name.clone();
            self.moves += 1;
            return format!(
                "You ward the {} with a management lock, private endpoints, and encryption. A Grue recoils.",
                name
            );
        }
        if let Some(i) = self.find_in_inventory(target) {
            let r = &mut self.inventory[i];
            r.locked = true;
            r.public = false;
            r.encrypted = true;
            let name = r.name.clone();
            self.moves += 1;
            return format!("You secure the carried {}.", name);
        }
        format!("There is no '{}' here to lock.", target)
    }

    /// Remove a management lock from a resource (in room or inventory) so it can
    /// be changed or deleted again. Mirrors `az lock delete`.
    pub fn unlock(&mut self, target: &str) -> String {
        if let Some(i) = self.find_in_room(target) {
            let r = &mut self.current_room_mut().resources[i];
            let name = r.name.clone();
            if !r.locked {
                return format!("The {} is not locked.", name);
            }
            r.locked = false;
            self.moves += 1;
            return format!(
                "You lift the management lock from the {}. It can now be changed or deleted \
                 — but it is once more vulnerable.",
                name
            );
        }
        if let Some(i) = self.find_in_inventory(target) {
            let r = &mut self.inventory[i];
            let name = r.name.clone();
            if !r.locked {
                return format!("The carried {} is not locked.", name);
            }
            r.locked = false;
            self.moves += 1;
            return format!("You remove the lock from the carried {}.", name);
        }
        format!("There is no '{}' here to unlock.", target)
    }

    /// Right-size a resource to cut runaway monthly cost (mirrors changing a SKU
    /// or scaling down a tier). Roughly halves the cost, clearing the
    /// cost-overrun hazard once it drops below the $500/mo threshold.
    pub fn resize(&mut self, target: &str) -> String {
        let apply = |r: &mut Resource| -> String {
            let name = r.name.clone();
            if r.monthly_cost == 0 {
                return format!(
                    "The {} costs nothing to run; there is nothing to right-size.",
                    name
                );
            }
            let before = r.monthly_cost;
            let after = before / 2;
            r.monthly_cost = after;
            if before >= 500 && after < 500 {
                format!(
                    "You right-size the {} to a reserved tier: ~${}/mo down to ~${}/mo. \
                     The cost-overrun Grue loses its scent.",
                    name, before, after
                )
            } else {
                format!(
                    "You right-size the {}: ~${}/mo down to ~${}/mo.",
                    name, before, after
                )
            }
        };
        if let Some(i) = self.find_in_room(target) {
            let msg = apply(&mut self.current_room_mut().resources[i]);
            self.moves += 1;
            return msg;
        }
        if let Some(i) = self.find_in_inventory(target) {
            let msg = apply(&mut self.inventory[i]);
            self.moves += 1;
            return msg;
        }
        format!("There is no '{}' here to right-size.", target)
    }

    /// Enable monitoring on the current room — lights it and banishes the Grue.
    pub fn monitor(&mut self) -> String {
        if self.current_room().monitored {
            return "Monitoring is already enabled here. The room is well lit.".to_string();
        }
        self.current_room_mut().monitored = true;
        self.darkness_streak = 0;
        self.moves += 1;
        "You enable diagnostic settings and Azure Monitor. Light floods the room; \
         the lurking Grue shrieks and flees."
            .to_string()
    }

    /// List carried resources.
    pub fn inventory(&self) -> String {
        if self.inventory.is_empty() {
            "You are carrying nothing.".to_string()
        } else {
            let mut out = String::from("You are carrying:\n");
            for r in &self.inventory {
                out.push_str(&format!("  - {} ({})\n", r.name, r.kind));
            }
            out.trim_end().to_string()
        }
    }

    /// Total governance hazards across every room and the inventory.
    pub fn total_hazards(&self) -> u32 {
        let room_hazards: u32 = self
            .rooms
            .values()
            .map(|room| {
                let dark = if room.is_dark() { 1 } else { 0 };
                let res: u32 = room.resources.iter().map(|r| r.hazards()).sum();
                dark + res
            })
            .sum();
        let inv_hazards: u32 = self.inventory.iter().map(|r| r.hazards()).sum();
        room_hazards + inv_hazards
    }

    /// Governance posture score (0..=100). Fewer hazards => higher score.
    pub fn score(&self) -> String {
        let hazards = self.total_hazards();
        // Each hazard costs 5 points off a perfect 100, floored at 0.
        let score = 100i32 - (hazards as i32) * 5;
        let score = score.max(0);
        let rank = match score {
            90..=100 => "Cloud Guardian",
            70..=89 => "Diligent Steward",
            50..=69 => "Apprentice Admin",
            30..=49 => "Reckless Tinkerer",
            _ => "Grue Chow",
        };
        format!(
            "Governance posture: {}/100  —  rank: {}\n\
             Outstanding hazards: {} (public/unencrypted/unlocked resources, \
             cost overruns, unmonitored rooms)\n\
             Moves taken: {}",
            score, rank, hazards, self.moves
        )
    }

    /// Run the Grue check for the current turn. Call this after each action.
    ///
    /// The longer you linger in the dark, the higher the chance of being eaten.
    pub fn grue_check(&mut self) -> GrueOutcome {
        if !self.current_room().is_dark() {
            self.darkness_streak = 0;
            return GrueOutcome::Safe;
        }
        self.darkness_streak += 1;
        // First turn in the dark is always a warning. After that, an escalating
        // chance of death: streak 2 -> ~1/4, streak 3 -> ~1/2, streak 4+ -> ~3/4.
        if self.darkness_streak <= 1 {
            return GrueOutcome::Lurking;
        }
        let threshold = match self.darkness_streak {
            2 => 64,  // 25% of 256
            3 => 128, // 50%
            _ => 192, // 75%
        };
        let roll = self.next_rand() % 256;
        if roll < threshold {
            self.game_over = true;
            GrueOutcome::Devoured
        } else {
            GrueOutcome::Lurking
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny_world() -> World {
        let lit = Room::new(
            "prod-rg",
            "A humming, well-lit datacenter aisle.",
            "eastus",
            true,
        )
        .with_exit(Direction::North, "dark-rg")
        .with_resource({
            let mut r = Resource::new(
                "storage",
                "Microsoft.Storage/storageAccounts",
                "A squat storage account.",
            );
            r.public = true;
            r.encrypted = false;
            r
        });
        let dark =
            Room::new("dark-rg", "?", "westus", false).with_exit(Direction::South, "prod-rg");
        World::new(vec![lit, dark], "prod-rg", "sub-mock-001").expect("valid test room graph")
    }

    #[test]
    fn look_lists_resources_and_exits() {
        let w = tiny_world();
        let out = w.look();
        assert!(out.contains("prod-rg"));
        assert!(out.contains("storage"));
        assert!(out.contains("Exits: north"));
    }

    #[test]
    fn dark_room_look_warns_about_grue() {
        let mut w = tiny_world();
        w.go(Direction::North).unwrap();
        let out = w.look();
        assert!(out.contains("Grue"));
        assert!(out.contains("pitch black"));
    }

    #[test]
    fn go_valid_and_invalid() {
        let mut w = tiny_world();
        assert!(w.go(Direction::North).is_ok());
        assert_eq!(w.current_room().name, "dark-rg");
        // No east exit from dark-rg.
        assert!(w.go(Direction::East).is_err());
    }

    #[test]
    fn take_moves_resource_to_inventory() {
        let mut w = tiny_world();
        let msg = w.take("storage");
        assert!(msg.contains("acquire"));
        assert!(w.inventory().contains("storage"));
        assert!(w.find_in_room("storage").is_none());
    }

    #[test]
    fn cannot_drop_room_resource_in_dark() {
        // Destroying what you cannot see is blocked, just like `take`.
        let mut w = tiny_world();
        w.go(Direction::North).unwrap(); // dark-rg
        assert!(w.current_room().is_dark());
        let msg = w.drop_item("anything");
        assert!(msg.contains("dark"), "got: {msg}");
    }

    #[test]
    fn can_drop_carried_resource_even_in_dark() {
        // Inventory items are held, so releasing them does not need light.
        let mut w = tiny_world();
        w.take("storage"); // acquired in the lit room
        w.go(Direction::North).unwrap(); // into the dark room
        assert!(w.current_room().is_dark());
        let msg = w.drop_item("storage");
        assert!(msg.contains("delete"), "got: {msg}");
        assert!(w.inventory().contains("nothing"));
    }

    #[test]
    fn find_prefers_exact_match_over_prefix() {
        // `storage` is a prefix of `storage-logs`; an exact query for the
        // shorter name must resolve to it, not the first prefix hit.
        let room = Room::new("rg", "well lit", "eastus", true)
            .with_resource(Resource::new(
                "storage-logs",
                "Microsoft.Storage/storageAccounts",
                "Log archive.",
            ))
            .with_resource(Resource::new(
                "storage",
                "Microsoft.Storage/storageAccounts",
                "Primary account.",
            ));
        let w = World::new(vec![room], "rg", "sub-mock-001").expect("valid test room graph");
        let idx = w.find_in_room("storage").expect("should resolve");
        assert_eq!(w.current_room().resources[idx].name, "storage");
        // A non-exact prefix query still resolves to the first prefix match.
        let pfx = w.find_in_room("storage-l").expect("prefix should resolve");
        assert_eq!(w.current_room().resources[pfx].name, "storage-logs");
    }

    #[test]
    fn cannot_take_in_dark() {
        let mut w = tiny_world();
        w.go(Direction::North).unwrap();
        let msg = w.take("anything");
        assert!(msg.contains("dark"));
    }

    #[test]
    fn lock_removes_hazards() {
        let mut w = tiny_world();
        let before = w.total_hazards();
        w.lock("storage");
        let after = w.total_hazards();
        assert!(after < before, "locking should reduce hazards");
    }

    #[test]
    fn drop_deletes_resource() {
        let mut w = tiny_world();
        w.take("storage");
        let msg = w.drop_item("storage");
        assert!(msg.contains("delete"));
        assert!(w.inventory().contains("nothing"));
    }

    #[test]
    fn locked_resource_cannot_be_dropped() {
        let mut w = tiny_world();
        w.lock("storage");
        let msg = w.drop_item("storage");
        assert!(msg.contains("locked"));
        assert!(w.find_in_room("storage").is_some());
    }

    #[test]
    fn unlock_reverses_a_lock() {
        let mut w = tiny_world();
        w.lock("storage");
        assert!(w.drop_item("storage").contains("locked"));
        let msg = w.unlock("storage");
        assert!(msg.contains("lift"));
        // Now the resource is deletable again.
        assert!(w.drop_item("storage").contains("delete"));
    }

    #[test]
    fn unlock_on_unlocked_is_noop_message() {
        let mut w = tiny_world();
        assert!(w.unlock("storage").contains("not locked"));
    }

    #[test]
    fn resize_reduces_cost_and_clears_overrun_hazard() {
        let mut w = tiny_world();
        // Give the storage account a cost-overrun hazard.
        w.current_room_mut().resources[0].monthly_cost = 800;
        let before = w.total_hazards();
        let msg = w.resize("storage");
        assert!(msg.contains("right-size"));
        assert!(
            w.total_hazards() < before,
            "right-sizing should clear the cost hazard"
        );
    }

    #[test]
    fn monitor_lights_room_and_banishes_grue() {
        let mut w = tiny_world();
        w.go(Direction::North).unwrap();
        assert!(w.current_room().is_dark());
        w.monitor();
        assert!(!w.current_room().is_dark());
        assert_eq!(w.grue_check(), GrueOutcome::Safe);
    }

    #[test]
    fn grue_first_turn_is_warning_then_can_kill() {
        let mut w = tiny_world();
        w.seed_rng(1);
        w.go(Direction::North).unwrap();
        // First check in the dark is only a warning.
        assert_eq!(w.grue_check(), GrueOutcome::Lurking);
        // Keep lingering; eventually the Grue strikes within a few turns.
        let mut devoured = false;
        for _ in 0..20 {
            if w.grue_check() == GrueOutcome::Devoured {
                devoured = true;
                break;
            }
        }
        assert!(devoured, "lingering in the dark should get you eaten");
        assert!(w.game_over);
    }

    #[test]
    fn score_reflects_hazards() {
        let mut w = tiny_world();
        let dirty = w.score();
        // Fix the storage account and light the dark room.
        w.lock("storage");
        w.go(Direction::North).unwrap();
        w.monitor();
        let clean = w.score();
        assert_ne!(dirty, clean);
        assert!(w.total_hazards() < 4);
    }

    #[test]
    fn all_resources_aggregates_rooms_and_inventory() {
        let mut w = tiny_world();
        // tiny_world has exactly one resource (storage) in prod-rg, none in dark-rg.
        assert_eq!(w.all_resources().len(), 1);
        assert_eq!(w.all_resources()[0].name, "storage");

        // Taking it moves it into inventory but the total count is unchanged.
        w.take("storage");
        assert_eq!(w.all_resources().len(), 1);
        assert_eq!(w.all_resources()[0].name, "storage");
    }

    #[test]
    fn examine_reports_status() {
        let w = tiny_world();
        let out = w.examine("storage");
        assert!(out.contains("PUBLIC"));
        assert!(out.contains("UNENCRYPTED"));
    }

    #[test]
    fn examine_missing_object() {
        let w = tiny_world();
        assert!(w.examine("dragon").contains("don't see"));
    }
}
