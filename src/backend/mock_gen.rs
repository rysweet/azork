//! A parameterizable, deterministic generator for synthetic "mock" Azure
//! estates of arbitrary size.
//!
//! [`crate::backend::mock::MockBackend`] builds one fixed, hand-authored
//! world (a handful of rooms) so existing gameplay, tests and UX never
//! change. This module adds a second, opt-in path: given [`MockSizeParams`]
//! (a resource-group count, a resources-per-group count, and a seed), it
//! synthesizes a much larger estate offline and instantly, for iterating on
//! Dungeon Crawler map layout (room sizing, corridor spacing, decorations)
//! without a slow real `az` crawl.
//!
//! Generation is a pure function of `(resource_groups, resources_per_group,
//! seed)`: the same parameters always produce byte-identical output, so
//! snapshot/layout tests and screenshots stay stable. There is no wall-clock
//! or OS-entropy randomness anywhere in this module — only a small seeded
//! xorshift64 PRNG.
//!
//! No arbitrary hard cap is imposed on size; generation is a single pass of
//! simple, streaming loops bounded only by the requested counts, so it
//! scales to whatever size is requested (governed by the caller's own
//! judgment of available memory, not a magic constant baked in here).

use super::Backend;
use crate::az_runner::FakeAzRunner;
use crate::parser::Direction;
use crate::world::{Resource, Room, World};

/// Named, convenient size presets. Each maps to a `(resource_groups,
/// resources_per_group)` pair; see [`MockSizePreset::counts`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MockSizePreset {
    Small,
    Medium,
    Large,
    Huge,
}

impl MockSizePreset {
    /// Parse a preset name, case-insensitively. Accepts `med` as a short
    /// alias for `medium`.
    pub fn parse(s: &str) -> Option<MockSizePreset> {
        match s.to_lowercase().as_str() {
            "small" => Some(MockSizePreset::Small),
            "medium" | "med" => Some(MockSizePreset::Medium),
            "large" => Some(MockSizePreset::Large),
            "huge" => Some(MockSizePreset::Huge),
            _ => None,
        }
    }

    /// `(resource_groups, resources_per_group)` for this preset.
    pub fn counts(self) -> (usize, usize) {
        match self {
            MockSizePreset::Small => (5, 3),
            MockSizePreset::Medium => (25, 5),
            MockSizePreset::Large => (100, 8),
            MockSizePreset::Huge => (500, 10),
        }
    }
}

/// The default seed used when none is explicitly requested, so
/// `AZORK_MOCK_SIZE=large` alone (no `:seed` suffix, no `AZORK_MOCK_SEED`)
/// still generates a stable, reproducible world.
pub const DEFAULT_SEED: u64 = 0xA5A5_5A5A_1234_5678;

/// Environment variable naming the overall size: a preset (`small`,
/// `medium`/`med`, `large`, `huge`), an explicit `RGSxRESOURCES` pair (e.g.
/// `50x6`), a bare resource-group count (e.g. `200`, which falls back to the
/// medium preset's resources-per-group), or any of those suffixed with
/// `:<seed>` (e.g. `large:42`).
pub const SIZE_ENV: &str = "AZORK_MOCK_SIZE";
/// Environment variable overriding the resource-group count explicitly.
pub const RGS_ENV: &str = "AZORK_MOCK_RGS";
/// Environment variable overriding the resources-per-group count explicitly.
pub const RESOURCES_PER_RG_ENV: &str = "AZORK_MOCK_RESOURCES_PER_RG";
/// Environment variable overriding the PRNG seed explicitly.
pub const SEED_ENV: &str = "AZORK_MOCK_SEED";

/// Fully-resolved parameters for a sized synthetic mock world.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MockSizeParams {
    pub resource_groups: usize,
    pub resources_per_group: usize,
    pub seed: u64,
}

impl MockSizeParams {
    /// Parse a `--mock-size`/`AZORK_MOCK_SIZE`-style spec string.
    ///
    /// Grammar (case-insensitive, all forms may have an optional `:<seed>`
    /// suffix):
    /// * a preset name: `small`, `medium`/`med`, `large`, `huge`
    /// * an explicit pair: `<resource_groups>x<resources_per_group>` (e.g. `50x6`)
    /// * a bare resource-group count: `<resource_groups>` (e.g. `200`) — the
    ///   resources-per-group falls back to the medium preset's value (5)
    pub fn parse(spec: &str) -> Result<MockSizeParams, String> {
        let (body, seed) = match spec.split_once(':') {
            Some((b, s)) => {
                let seed = s
                    .parse::<u64>()
                    .map_err(|_| format!("invalid mock-size seed '{s}': not a valid u64"))?;
                (b, Some(seed))
            }
            None => (spec, None),
        };

        let (resource_groups, resources_per_group) =
            if let Some(preset) = MockSizePreset::parse(body) {
                preset.counts()
            } else if let Some((a, b)) = body.split_once(['x', 'X']) {
                let rgs = a
                    .parse::<usize>()
                    .map_err(|_| format!("invalid mock-size resource-group count '{a}'"))?;
                let per = b
                    .parse::<usize>()
                    .map_err(|_| format!("invalid mock-size resources-per-group count '{b}'"))?;
                (rgs, per)
            } else if let Ok(rgs) = body.parse::<usize>() {
                (rgs, MockSizePreset::Medium.counts().1)
            } else {
                return Err(format!(
                "invalid mock-size value '{spec}': expected a preset (small/medium/large/huge), \
                 an explicit '<rgs>x<resources-per-rg>' pair, or a bare resource-group count"
            ));
            };

        if resource_groups == 0 {
            return Err("mock-size resource-group count must be at least 1".to_string());
        }
        if resources_per_group == 0 {
            return Err("mock-size resources-per-group count must be at least 1".to_string());
        }

        Ok(MockSizeParams {
            resource_groups,
            resources_per_group,
            seed: seed.unwrap_or(DEFAULT_SEED),
        })
    }

    /// Resolve size params from the environment (`AZORK_MOCK_SIZE`,
    /// `AZORK_MOCK_RGS`, `AZORK_MOCK_RESOURCES_PER_RG`, `AZORK_MOCK_SEED`).
    ///
    /// Returns `None` when none of the env vars are set, so the default
    /// (unsized, hand-authored) mock estate is otherwise unaffected. `RGS`,
    /// `RESOURCES_PER_RG` and `SEED` each individually override whatever
    /// `AZORK_MOCK_SIZE` specified (or the medium preset's defaults, if
    /// `AZORK_MOCK_SIZE` itself is unset).
    pub fn from_env() -> Option<Result<MockSizeParams, String>> {
        let size_env = std::env::var(SIZE_ENV).ok();
        let rgs_env = std::env::var(RGS_ENV).ok();
        let per_env = std::env::var(RESOURCES_PER_RG_ENV).ok();
        let seed_env = std::env::var(SEED_ENV).ok();

        if size_env.is_none() && rgs_env.is_none() && per_env.is_none() && seed_env.is_none() {
            return None;
        }

        let mut params = match &size_env {
            Some(s) => match MockSizeParams::parse(s) {
                Ok(p) => p,
                Err(e) => return Some(Err(format!("{SIZE_ENV}: {e}"))),
            },
            None => {
                let (rgs, per) = MockSizePreset::Medium.counts();
                MockSizeParams {
                    resource_groups: rgs,
                    resources_per_group: per,
                    seed: DEFAULT_SEED,
                }
            }
        };

        if let Some(s) = rgs_env {
            match s.parse::<usize>() {
                Ok(v) if v > 0 => params.resource_groups = v,
                _ => return Some(Err(format!("{RGS_ENV}: '{s}' is not a positive integer"))),
            }
        }
        if let Some(s) = per_env {
            match s.parse::<usize>() {
                Ok(v) if v > 0 => params.resources_per_group = v,
                _ => {
                    return Some(Err(format!(
                        "{RESOURCES_PER_RG_ENV}: '{s}' is not a positive integer"
                    )))
                }
            }
        }
        if let Some(s) = seed_env {
            match s.parse::<u64>() {
                Ok(v) => params.seed = v,
                Err(_) => return Some(Err(format!("{SEED_ENV}: '{s}' is not a valid u64"))),
            }
        }

        Some(Ok(params))
    }
}

/// Minimal seeded xorshift64 PRNG. Not cryptographic — just deterministic,
/// fast, and dependency-free, which is all reproducible synthetic-data
/// generation needs.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Rng {
        // xorshift64 requires a nonzero state.
        Rng(if seed == 0 { DEFAULT_SEED } else { seed })
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// Uniform value in `0..n` (`n` must be nonzero).
    fn next_index(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }

    /// `true` with probability `pct` percent (`0..=100`).
    fn next_chance(&mut self, pct: u64) -> bool {
        (self.next_u64() % 100) < pct
    }
}

/// Azure resource types drawn from the same set the map/icon table
/// (`crate::dungeon::icons`) already recognises, plus a rough monthly-cost
/// baseline used to seed realistic-looking costs.
const RESOURCE_TYPES: &[(&str, &str, u32)] = &[
    ("Microsoft.Storage/storageAccounts", "storage", 40),
    ("Microsoft.Compute/virtualMachines", "vm", 150),
    ("Microsoft.Web/sites", "webapp", 90),
    ("Microsoft.KeyVault/vaults", "kv", 5),
    ("Microsoft.ContainerService/managedClusters", "aks", 250),
    ("Microsoft.Sql/servers", "sql", 400),
    ("Microsoft.DocumentDB/databaseAccounts", "cosmos", 300),
    ("Microsoft.Network/virtualNetworks", "vnet", 0),
    ("Microsoft.Network/publicIPAddresses", "pip", 4),
    ("Microsoft.Network/networkSecurityGroups", "nsg", 0),
    ("Microsoft.Network/loadBalancers", "lb", 20),
    ("Microsoft.Network/networkInterfaces", "nic", 0),
];

const REGIONS: &[&str] = &[
    "eastus",
    "eastus2",
    "westus2",
    "westus3",
    "centralus",
    "northeurope",
    "westeurope",
    "southeastasia",
    "japaneast",
    "australiaeast",
];

const ADJECTIVES: &[&str] = &[
    "shadow", "crimson", "silent", "bright", "hidden", "frozen", "ember", "azure", "rusty",
    "gilded", "murky", "arcane", "iron", "misty", "sunken",
];

const NOUNS: &[&str] = &[
    "hall", "vault", "chamber", "outpost", "tower", "keep", "cellar", "gate", "sanctum", "landing",
    "forge", "archive", "bastion", "grotto", "spire",
];

/// A generated Azure resource, prior to conversion into a [`Resource`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedResource {
    pub name: String,
    pub kind: String,
    pub public: bool,
    pub encrypted: bool,
    pub monthly_cost: u32,
}

/// A generated resource group, prior to conversion into a [`Room`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedRoom {
    pub name: String,
    pub region: String,
    pub monitored: bool,
    /// Deterministically ordered `(direction, destination room name)` pairs.
    pub exits: Vec<(Direction, String)>,
    pub resources: Vec<GeneratedResource>,
}

/// Room name for grid position `i` (`0`-indexed).
fn room_name(i: usize, rng_seed_word: (&str, &str)) -> String {
    let (adj, noun) = rng_seed_word;
    format!("rg-{i:04}-{adj}-{noun}")
}

/// Generate `params.resource_groups` rooms, each with
/// `params.resources_per_group` resources, deterministically from
/// `params.seed`.
///
/// Rooms are laid out and connected as a grid dungeon (East/West chain
/// across each row, plus North/South links between rows) so the result is
/// always a single connected graph, never disconnected islands, regardless
/// of size.
pub fn generate_rooms(params: MockSizeParams) -> Vec<GeneratedRoom> {
    let n = params.resource_groups;
    let mut rng = Rng::new(params.seed);

    // Precompute names, regions and monitoring first (exits need every
    // room's final name up front).
    let mut names = Vec::with_capacity(n);
    let mut regions = Vec::with_capacity(n);
    let mut monitored = Vec::with_capacity(n);
    for i in 0..n {
        let adj = ADJECTIVES[rng.next_index(ADJECTIVES.len())];
        let noun = NOUNS[rng.next_index(NOUNS.len())];
        names.push(room_name(i, (adj, noun)));
        regions.push(REGIONS[rng.next_index(REGIONS.len())].to_string());
        // The start room (index 0) is always monitored so the player never
        // starts in darkness; every other room is dark ~12% of the time,
        // giving larger estates realistic (deterministic) Grue hazards.
        monitored.push(i == 0 || !rng.next_chance(12));
    }

    // Grid layout: width = ceil(sqrt(n)). A simple East/West row-chain plus
    // North/South row-to-row links always yields one connected component,
    // since every room in row r>0 links back (via North) to a room in row
    // r-1, and every room in a row links (via East/West) to its neighbours.
    let width = (n as f64).sqrt().ceil().max(1.0) as usize;
    let mut exits: Vec<Vec<(Direction, String)>> = vec![Vec::new(); n];
    for i in 0..n {
        let col = i % width;
        if col + 1 < width && i + 1 < n {
            exits[i].push((Direction::East, names[i + 1].clone()));
            exits[i + 1].push((Direction::West, names[i].clone()));
        }
        if i + width < n {
            exits[i].push((Direction::South, names[i + width].clone()));
            exits[i + width].push((Direction::North, names[i].clone()));
        }
    }

    let mut rooms = Vec::with_capacity(n);
    for i in 0..n {
        let mut resources = Vec::with_capacity(params.resources_per_group);
        for j in 0..params.resources_per_group {
            let (kind, slug, base_cost) = RESOURCE_TYPES[rng.next_index(RESOURCE_TYPES.len())];
            let name = format!("{slug}-{i:04}-{j:02}");
            let public = rng.next_chance(20);
            let encrypted = !rng.next_chance(15);
            let jitter = rng.next_index(base_cost.max(1) as usize + 1) as u32;
            let monthly_cost = base_cost + jitter;
            resources.push(GeneratedResource {
                name,
                kind: kind.to_string(),
                public,
                encrypted,
                monthly_cost,
            });
        }

        rooms.push(GeneratedRoom {
            name: names[i].clone(),
            region: regions[i].clone(),
            monitored: monitored[i],
            exits: std::mem::take(&mut exits[i]),
            resources,
        });
    }

    rooms
}

impl GeneratedResource {
    fn into_resource(self) -> Resource {
        let mut r = Resource::new(
            &self.name,
            &self.kind,
            &format!("A synthetic {}.", self.kind),
        );
        r.public = self.public;
        r.encrypted = self.encrypted;
        r.monthly_cost = self.monthly_cost;
        r
    }
}

impl GeneratedRoom {
    fn into_room(self) -> Room {
        let description = format!(
            "A synthetic resource group in {} with {} resource(s).",
            self.region,
            self.resources.len()
        );
        let mut room = Room::new(&self.name, &description, &self.region, self.monitored);
        for (dir, dest) in self.exits {
            room = room.with_exit(dir, &dest);
        }
        for res in self.resources {
            room = room.with_resource(res.into_resource());
        }
        room
    }
}

/// Build a full [`World`] for `params`. The world's start room is always the
/// first generated room (grid position `0`), which is always monitored.
pub fn generate_world(params: MockSizeParams) -> Result<World, String> {
    let rooms = generate_rooms(params);
    let start = rooms[0].name.clone();
    let subscription = format!(
        "Contoso-Sized-{}rg-{}res (mock, seed {:#x})",
        params.resource_groups, params.resources_per_group, params.seed
    );
    let world_rooms: Vec<Room> = rooms.into_iter().map(GeneratedRoom::into_room).collect();
    World::new(world_rooms, &start, &subscription)
}

/// Build a [`FakeAzRunner`] with canned `az group list` / `az resource list`
/// JSON responses for `params`'s generated estate — the Dungeon Crawler
/// Mode (`azork crawl`) analogue of [`generate_world`], flowing through the
/// exact same [`crate::az_runner::AzRunner`]-driven map-building path a real
/// subscription would.
pub fn build_fake_runner(params: MockSizeParams) -> FakeAzRunner {
    let rooms = generate_rooms(params);
    const SUB: &str = "00000000-0000-0000-0000-000000000000";

    let groups_json: Vec<String> = rooms
        .iter()
        .map(|r| {
            format!(
                r#"{{"id": "/subscriptions/{SUB}/resourceGroups/{name}", "location": "{region}", "name": "{name}"}}"#,
                name = r.name,
                region = r.region,
            )
        })
        .collect();
    let group_list_json = format!("[{}]", groups_json.join(","));

    let mut runner = FakeAzRunner::new().with(&["group", "list", "-o", "json"], &group_list_json);

    for room in &rooms {
        let resources_json: Vec<String> = room
            .resources
            .iter()
            .map(|res| {
                format!(
                    r#"{{"id": "/subscriptions/{SUB}/resourceGroups/{rg}/providers/{kind}/{name}", "name": "{name}", "type": "{kind}", "location": "{region}"}}"#,
                    rg = room.name,
                    kind = res.kind,
                    name = res.name,
                    region = room.region,
                )
            })
            .collect();
        let resource_list_json = format!("[{}]", resources_json.join(","));
        runner = runner.with(
            &[
                "resource",
                "list",
                "--resource-group",
                &room.name,
                "-o",
                "json",
            ],
            &resource_list_json,
        );
    }

    runner
}

/// A [`Backend`] that builds a parameterized, sized synthetic world instead
/// of the fixed hand-authored one — same offline/no-credentials contract as
/// [`super::mock::MockBackend`], just larger and generated on demand.
pub struct SizedMockBackend {
    params: MockSizeParams,
}

impl SizedMockBackend {
    pub fn new(params: MockSizeParams) -> SizedMockBackend {
        SizedMockBackend { params }
    }
}

impl Backend for SizedMockBackend {
    fn name(&self) -> &str {
        "mock (offline, sized)"
    }

    fn build_world(&self) -> Result<World, String> {
        generate_world(self.params)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashSet, VecDeque};

    #[test]
    fn preset_parse_is_case_insensitive_and_supports_med_alias() {
        assert_eq!(MockSizePreset::parse("SMALL"), Some(MockSizePreset::Small));
        assert_eq!(
            MockSizePreset::parse("Medium"),
            Some(MockSizePreset::Medium)
        );
        assert_eq!(MockSizePreset::parse("med"), Some(MockSizePreset::Medium));
        assert_eq!(MockSizePreset::parse("MED"), Some(MockSizePreset::Medium));
        assert_eq!(MockSizePreset::parse("large"), Some(MockSizePreset::Large));
        assert_eq!(MockSizePreset::parse("HUGE"), Some(MockSizePreset::Huge));
        assert_eq!(MockSizePreset::parse("nonsense"), None);
    }

    #[test]
    fn params_parse_accepts_preset_pair_and_bare_count() {
        let p = MockSizeParams::parse("small").unwrap();
        assert_eq!((p.resource_groups, p.resources_per_group), (5, 3));
        assert_eq!(p.seed, DEFAULT_SEED);

        let p = MockSizeParams::parse("50x6").unwrap();
        assert_eq!((p.resource_groups, p.resources_per_group), (50, 6));

        let p = MockSizeParams::parse("200").unwrap();
        assert_eq!(p.resource_groups, 200);
        assert_eq!(p.resources_per_group, MockSizePreset::Medium.counts().1);

        let p = MockSizeParams::parse("large:42").unwrap();
        assert_eq!((p.resource_groups, p.resources_per_group), (100, 8));
        assert_eq!(p.seed, 42);
    }

    #[test]
    fn params_parse_rejects_garbage() {
        assert!(MockSizeParams::parse("not-a-size").is_err());
        assert!(MockSizeParams::parse("0").is_err());
        assert!(MockSizeParams::parse("5x0").is_err());
        assert!(MockSizeParams::parse("large:notanumber").is_err());
    }

    #[test]
    fn sized_generation_yields_expected_counts() {
        let params = MockSizeParams::parse("large:7").unwrap();
        let rooms = generate_rooms(params);
        assert_eq!(rooms.len(), 100);
        let total_resources: usize = rooms.iter().map(|r| r.resources.len()).sum();
        assert_eq!(total_resources, 100 * 8);
    }

    #[test]
    fn sized_generation_is_reproducible() {
        let params = MockSizeParams::parse("medium:123").unwrap();
        let a = generate_rooms(params);
        let b = generate_rooms(params);
        assert_eq!(a, b, "identical params/seed must yield identical worlds");
    }

    #[test]
    fn different_seeds_yield_different_worlds() {
        let a = generate_rooms(MockSizeParams::parse("medium:1").unwrap());
        let b = generate_rooms(MockSizeParams::parse("medium:2").unwrap());
        assert_ne!(a, b);
    }

    #[test]
    fn generated_world_is_fully_connected() {
        for spec in ["small", "medium", "large", "5x1", "17x2"] {
            let params = MockSizeParams::parse(spec).unwrap();
            let rooms = generate_rooms(params);
            let by_name: std::collections::HashMap<&str, &GeneratedRoom> =
                rooms.iter().map(|r| (r.name.as_str(), r)).collect();

            let mut visited: HashSet<&str> = HashSet::new();
            let mut queue: VecDeque<&str> = VecDeque::new();
            let start = rooms[0].name.as_str();
            visited.insert(start);
            queue.push_back(start);
            while let Some(cur) = queue.pop_front() {
                let room = by_name[cur];
                for (_, dest) in &room.exits {
                    if visited.insert(dest.as_str()) {
                        queue.push_back(dest.as_str());
                    }
                }
            }

            assert_eq!(
                visited.len(),
                rooms.len(),
                "spec '{spec}': every room must be reachable from the start room \
                 (visited {} of {})",
                visited.len(),
                rooms.len()
            );
        }
    }

    #[test]
    fn generated_resource_types_are_icon_known() {
        use crate::dungeon::icons::{icon_for, DEFAULT_ICON};

        let params = MockSizeParams::parse("large:9").unwrap();
        let rooms = generate_rooms(params);
        for room in &rooms {
            for res in &room.resources {
                assert_ne!(
                    icon_for(&res.kind),
                    DEFAULT_ICON,
                    "generated resource kind '{}' has no known icon",
                    res.kind
                );
            }
        }
    }

    #[test]
    fn generate_world_builds_a_valid_world() {
        let params = MockSizeParams::parse("small:1").unwrap();
        let world = generate_world(params).unwrap();
        assert_eq!(world.rooms_len(), 5);
        assert!(!world.current_room().is_dark());
    }

    #[test]
    fn sized_backend_builds_requested_size() {
        let params = MockSizeParams::parse("50x4:99").unwrap();
        let backend = SizedMockBackend::new(params);
        let world = backend.build_world().unwrap();
        assert_eq!(world.rooms_len(), 50);
    }

    #[test]
    fn from_env_is_none_when_unset() {
        // Guard against leakage from a parallel test in this process, though
        // each of these vars is unique to this module.
        std::env::remove_var(SIZE_ENV);
        std::env::remove_var(RGS_ENV);
        std::env::remove_var(RESOURCES_PER_RG_ENV);
        std::env::remove_var(SEED_ENV);
        assert!(MockSizeParams::from_env().is_none());
    }

    #[test]
    fn from_env_reads_size_and_explicit_overrides() {
        std::env::set_var(SIZE_ENV, "large");
        std::env::set_var(RGS_ENV, "10");
        std::env::remove_var(RESOURCES_PER_RG_ENV);
        std::env::remove_var(SEED_ENV);
        let params = MockSizeParams::from_env().unwrap().unwrap();
        assert_eq!(params.resource_groups, 10); // overridden
        assert_eq!(params.resources_per_group, 8); // from the "large" preset
        std::env::remove_var(SIZE_ENV);
        std::env::remove_var(RGS_ENV);
    }

    #[test]
    fn build_fake_runner_produces_matching_group_and_resource_lists() {
        use crate::az_runner::AzRunner;

        let params = MockSizeParams::parse("10x3:5").unwrap();
        let runner = build_fake_runner(params);
        let out = runner.run(&["group", "list", "-o", "json"]).unwrap();
        assert!(out.status.success());
        let groups: serde_json::Value =
            serde_json::from_slice(&out.stdout).expect("valid JSON group list");
        assert_eq!(groups.as_array().unwrap().len(), 10);

        let first_rg = groups[0]["name"].as_str().unwrap();
        let out = runner
            .run(&[
                "resource",
                "list",
                "--resource-group",
                first_rg,
                "-o",
                "json",
            ])
            .unwrap();
        assert!(out.status.success());
        let resources: serde_json::Value =
            serde_json::from_slice(&out.stdout).expect("valid JSON resource list");
        assert_eq!(resources.as_array().unwrap().len(), 3);
    }
}
