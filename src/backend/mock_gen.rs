//! Deterministic, parameterizable synthetic Azure estate generator.
//!
//! The offline mock backend ([`super::mock::MockBackend`]) ships a small,
//! fixed, hand-authored world by default so existing tests/UX never change.
//! This module adds an opt-in, *sized* generator so testers can synthesize
//! much larger fake tenants (many resource groups, each with many
//! resources) to iterate on and load-test the Dungeon Crawler map layout
//! offline and fast, without a slow `az` crawl.
//!
//! Generation is always deterministic: the same [`MockSizeParams`] (size +
//! seed) produce byte-for-byte identical output every run, so snapshot and
//! layout tests stay stable. There is no wall-clock or unseeded randomness
//! anywhere in this module.
//!
//! See `docs/DUNGEON-CRAWLER.md#generating-a-sized-mock-tenant` for the
//! user-facing knobs (`AZORK_MOCK_SIZE` / `--mock-size` and friends).

use crate::az_runner::FakeAzRunner;
use crate::parser::Direction;
use crate::world::{Resource, Room, World};

/// Seed used when a size is requested but no explicit seed is given.
/// Arbitrary but fixed, so "no seed specified" is still fully reproducible.
pub const DEFAULT_SEED: u64 = 42;

/// Named size presets: `(resource_groups, resources_per_group)`.
///
/// There is no hard cap enforced anywhere in this module (per policy: no
/// arbitrary fixed resource caps) — presets are just convenient defaults;
/// [`MockSizeParams`] can also be built directly with any explicit counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MockSizePreset {
    Small,
    Medium,
    Large,
    Huge,
}

impl MockSizePreset {
    fn counts(self) -> (usize, usize) {
        match self {
            MockSizePreset::Small => (5, 3),
            MockSizePreset::Medium => (25, 5),
            MockSizePreset::Large => (100, 8),
            MockSizePreset::Huge => (500, 10),
        }
    }

    /// Parse a preset name, case-insensitively. `"med"` is accepted as a
    /// shorthand for `"medium"`.
    pub fn parse(s: &str) -> Option<MockSizePreset> {
        match s.to_lowercase().as_str() {
            "small" => Some(MockSizePreset::Small),
            "medium" | "med" => Some(MockSizePreset::Medium),
            "large" => Some(MockSizePreset::Large),
            "huge" => Some(MockSizePreset::Huge),
            _ => None,
        }
    }
}

/// Explicit parameters for a generated synthetic estate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MockSizeParams {
    /// Number of resource groups (dungeon rooms) to generate.
    pub resource_groups: usize,
    /// Exact number of resources generated per resource group.
    pub resources_per_group: usize,
    /// Deterministic PRNG seed. Same params + same seed => identical world.
    pub seed: u64,
}

impl MockSizeParams {
    pub fn from_preset(preset: MockSizePreset) -> MockSizeParams {
        let (resource_groups, resources_per_group) = preset.counts();
        MockSizeParams {
            resource_groups,
            resources_per_group,
            seed: DEFAULT_SEED,
        }
    }

    /// Parse a `--mock-size`/`AZORK_MOCK_SIZE` value.
    ///
    /// Accepts, in order:
    /// * a named preset (`small`/`medium`/`large`/`huge`, case-insensitive);
    /// * an explicit `RGSxPER_GROUP` pair, e.g. `"200x15"`;
    /// * a bare resource-group count, e.g. `"200"` (resources-per-group
    ///   falls back to the medium preset's value).
    ///
    /// Any of the above may carry an optional `:<seed>` suffix to override
    /// the default seed, e.g. `"large:7"` or `"200x15:7"`.
    pub fn parse(spec: &str) -> Result<MockSizeParams, String> {
        let (body, seed) = match spec.rsplit_once(':') {
            Some((b, s)) => (
                b,
                s.parse::<u64>()
                    .map_err(|_| format!("invalid mock-size seed '{s}'"))?,
            ),
            None => (spec, DEFAULT_SEED),
        };

        if let Some(preset) = MockSizePreset::parse(body) {
            let mut params = MockSizeParams::from_preset(preset);
            params.seed = seed;
            return Ok(params);
        }

        if let Some((rgs_str, per_str)) = body.split_once(['x', 'X']) {
            let resource_groups = rgs_str
                .parse::<usize>()
                .map_err(|_| format!("invalid resource-group count '{rgs_str}'"))?;
            let resources_per_group = per_str
                .parse::<usize>()
                .map_err(|_| format!("invalid resources-per-group count '{per_str}'"))?;
            return Ok(MockSizeParams {
                resource_groups,
                resources_per_group,
                seed,
            });
        }

        let resource_groups = body.parse::<usize>().map_err(|_| {
            format!(
                "unrecognized mock size '{body}' (expected small/medium/large/huge, \
                 a resource-group count, or COUNTxPER_GROUP)"
            )
        })?;
        Ok(MockSizeParams {
            resource_groups,
            resources_per_group: MockSizePreset::Medium.counts().1,
            seed,
        })
    }

    /// Read sizing parameters from the environment, if any sizing variable
    /// is set. Returns `None` when none are set, so callers fall back to the
    /// default fixed hand-authored world.
    ///
    /// Recognised variables:
    /// * `AZORK_MOCK_SIZE` — preset name, bare count, or `COUNTxPER_GROUP`
    ///   (same grammar as [`MockSizeParams::parse`]).
    /// * `AZORK_MOCK_RGS` — explicit resource-group count (overrides the
    ///   count derived from `AZORK_MOCK_SIZE`, if both are set).
    /// * `AZORK_MOCK_RESOURCES_PER_RG` — explicit resources-per-group count
    ///   (overrides the value derived from `AZORK_MOCK_SIZE`, if both set).
    /// * `AZORK_MOCK_SEED` — deterministic seed override.
    pub fn from_env() -> Option<Result<MockSizeParams, String>> {
        Self::from_env_values(
            std::env::var("AZORK_MOCK_SIZE").ok(),
            std::env::var("AZORK_MOCK_RGS").ok(),
            std::env::var("AZORK_MOCK_RESOURCES_PER_RG").ok(),
            std::env::var("AZORK_MOCK_SEED").ok(),
        )
    }

    /// Testable core of [`Self::from_env`]: takes explicit optional values
    /// instead of reading the process environment directly.
    fn from_env_values(
        size: Option<String>,
        rgs: Option<String>,
        per: Option<String>,
        seed: Option<String>,
    ) -> Option<Result<MockSizeParams, String>> {
        if size.is_none() && rgs.is_none() && per.is_none() && seed.is_none() {
            return None;
        }
        let mut params = match &size {
            Some(s) => match MockSizeParams::parse(s) {
                Ok(p) => p,
                Err(e) => return Some(Err(e)),
            },
            None => MockSizeParams::from_preset(MockSizePreset::Medium),
        };
        if let Some(rgs) = rgs {
            match rgs.parse::<usize>() {
                Ok(n) => params.resource_groups = n,
                Err(_) => return Some(Err(format!("invalid AZORK_MOCK_RGS '{rgs}'"))),
            }
        }
        if let Some(per) = per {
            match per.parse::<usize>() {
                Ok(n) => params.resources_per_group = n,
                Err(_) => return Some(Err(format!("invalid AZORK_MOCK_RESOURCES_PER_RG '{per}'"))),
            }
        }
        if let Some(seed) = seed {
            match seed.parse::<u64>() {
                Ok(n) => params.seed = n,
                Err(_) => return Some(Err(format!("invalid AZORK_MOCK_SEED '{seed}'"))),
            }
        }
        Some(Ok(params))
    }
}

/// SplitMix64: a small, fast, fully deterministic PRNG. No external crate is
/// needed for synthetic-data generation, which keeps this offline generator
/// dependency-free.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(seed)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A value in `[0, bound)`. `bound == 0` always yields `0`.
    fn below(&mut self, bound: usize) -> usize {
        if bound == 0 {
            return 0;
        }
        (self.next_u64() % bound as u64) as usize
    }

    /// A percentage roll: `true` with probability `pct` out of 100.
    fn chance(&mut self, pct: u8) -> bool {
        self.below(100) < pct as usize
    }
}

struct ResourceKindSpec {
    /// Full Azure resource type, e.g. `Microsoft.Storage/storageAccounts`.
    /// Drawn from the icon/type table (`src/dungeon/type_table.rs`) so
    /// generated resources always render with a known icon.
    type_name: &'static str,
    /// Short, human-typed identifier prefix (e.g. `storage`, `vm`).
    short: &'static str,
    public_pct: u8,
    encrypted_pct: u8,
    locked_pct: u8,
    cost_min: u32,
    cost_max: u32,
}

const RESOURCE_KINDS: &[ResourceKindSpec] = &[
    ResourceKindSpec {
        type_name: "Microsoft.Storage/storageAccounts",
        short: "storage",
        public_pct: 30,
        encrypted_pct: 80,
        locked_pct: 50,
        cost_min: 20,
        cost_max: 150,
    },
    ResourceKindSpec {
        type_name: "Microsoft.Compute/virtualMachines",
        short: "vm",
        public_pct: 20,
        encrypted_pct: 90,
        locked_pct: 50,
        cost_min: 50,
        cost_max: 600,
    },
    ResourceKindSpec {
        type_name: "Microsoft.Network/virtualNetworks",
        short: "vnet",
        public_pct: 0,
        encrypted_pct: 100,
        locked_pct: 70,
        cost_min: 0,
        cost_max: 10,
    },
    ResourceKindSpec {
        type_name: "Microsoft.Web/sites",
        short: "webapp",
        public_pct: 70,
        encrypted_pct: 90,
        locked_pct: 40,
        cost_min: 40,
        cost_max: 300,
    },
    ResourceKindSpec {
        type_name: "Microsoft.KeyVault/vaults",
        short: "keyvault",
        public_pct: 5,
        encrypted_pct: 100,
        locked_pct: 60,
        cost_min: 5,
        cost_max: 30,
    },
    ResourceKindSpec {
        type_name: "Microsoft.ContainerService/managedClusters",
        short: "aks",
        public_pct: 10,
        encrypted_pct: 95,
        locked_pct: 60,
        cost_min: 200,
        cost_max: 1200,
    },
    ResourceKindSpec {
        type_name: "Microsoft.Sql/servers",
        short: "sql",
        public_pct: 15,
        encrypted_pct: 85,
        locked_pct: 55,
        cost_min: 100,
        cost_max: 900,
    },
    ResourceKindSpec {
        type_name: "Microsoft.DocumentDB/databaseAccounts",
        short: "cosmos",
        public_pct: 15,
        encrypted_pct: 95,
        locked_pct: 55,
        cost_min: 80,
        cost_max: 700,
    },
    ResourceKindSpec {
        type_name: "Microsoft.Network/networkInterfaces",
        short: "nic",
        public_pct: 0,
        encrypted_pct: 100,
        locked_pct: 70,
        cost_min: 0,
        cost_max: 5,
    },
    ResourceKindSpec {
        type_name: "Microsoft.Network/networkSecurityGroups",
        short: "nsg",
        public_pct: 0,
        encrypted_pct: 100,
        locked_pct: 70,
        cost_min: 0,
        cost_max: 5,
    },
    ResourceKindSpec {
        type_name: "Microsoft.Network/publicIPAddresses",
        short: "pip",
        public_pct: 100,
        encrypted_pct: 100,
        locked_pct: 40,
        cost_min: 3,
        cost_max: 15,
    },
    ResourceKindSpec {
        type_name: "Microsoft.Network/loadBalancers",
        short: "lb",
        public_pct: 40,
        encrypted_pct: 100,
        locked_pct: 50,
        cost_min: 20,
        cost_max: 100,
    },
];

const NAME_PREFIXES: &[&str] = &[
    "contoso",
    "fabrikam",
    "adatum",
    "northwind",
    "tailspin",
    "wingtip",
    "relecloud",
    "litware",
];

const NAME_SUFFIXES: &[&str] = &[
    "prod", "dev", "test", "stage", "core", "edge", "hub", "spoke", "ops", "data",
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
    "uksouth",
    "japaneast",
];

/// A generated resource, in a backend-agnostic shape shared by both the
/// interactive [`World`] generator and the Dungeon Crawler `az`-shaped JSON
/// generator, so the two surfaces never drift out of sync.
struct GenResource {
    name: String,
    kind: &'static str,
    region: String,
    public: bool,
    encrypted: bool,
    locked: bool,
    monthly_cost: u32,
}

/// A generated room (resource group), in the same backend-agnostic shape.
struct GenRoom {
    name: String,
    region: String,
    monitored: bool,
    resources: Vec<GenResource>,
}

/// Core, backend-agnostic generation: deterministic given `params`.
fn generate_rooms(params: &MockSizeParams) -> Vec<GenRoom> {
    let n = params.resource_groups.max(1);
    let mut rng = Rng::new(params.seed);

    let mut names: Vec<String> = Vec::with_capacity(n);
    for i in 0..n {
        let prefix = NAME_PREFIXES[rng.below(NAME_PREFIXES.len())];
        let suffix = NAME_SUFFIXES[rng.below(NAME_SUFFIXES.len())];
        names.push(format!("{prefix}-{suffix}-rg-{i:05}"));
    }

    let mut rooms = Vec::with_capacity(n);
    for name in names {
        let region = REGIONS[rng.below(REGIONS.len())].to_string();
        // Every 11th room (never the start room) is unmonitored, i.e. dark
        // Grue territory, echoing the hand-authored default world's flavor.
        let idx = rooms.len();
        let monitored = idx == 0 || idx % 11 != 0;

        let mut resources = Vec::with_capacity(params.resources_per_group);
        for j in 0..params.resources_per_group {
            let spec = &RESOURCE_KINDS[rng.below(RESOURCE_KINDS.len())];
            let cost_span = spec.cost_max.saturating_sub(spec.cost_min);
            let monthly_cost = spec.cost_min + rng.below(cost_span as usize + 1) as u32;
            resources.push(GenResource {
                name: format!("{}-{:03}", spec.short, j),
                kind: spec.type_name,
                region: region.clone(),
                public: rng.chance(spec.public_pct),
                encrypted: rng.chance(spec.encrypted_pct),
                locked: rng.chance(spec.locked_pct),
                monthly_cost,
            });
        }

        rooms.push(GenRoom {
            name,
            region,
            monitored,
            resources,
        });
    }
    rooms
}

/// Grid width used to lay generated rooms out (and connect them) so the
/// result is always a single connected component: rooms fill row-major,
/// each room links West/East to its row neighbours and North/South to the
/// room directly above/below it, so every room reaches the start room.
fn grid_width(n: usize) -> usize {
    (n as f64).sqrt().ceil().max(1.0) as usize
}

/// Build the deterministic, sized synthetic [`World`] described by `params`.
pub fn generate_world(params: &MockSizeParams) -> Result<World, String> {
    let gen_rooms = generate_rooms(params);
    let n = gen_rooms.len();
    let width = grid_width(n);
    let names: Vec<&str> = gen_rooms.iter().map(|r| r.name.as_str()).collect();

    let mut rooms = Vec::with_capacity(n);
    for (i, gr) in gen_rooms.iter().enumerate() {
        let description = if gr.monitored {
            format!(
                "A synthetic resource group in {}. Monitoring is enabled here.",
                gr.region
            )
        } else {
            "?".to_string()
        };
        let mut room = Room::new(&gr.name, &description, &gr.region, gr.monitored);

        let row = i / width;
        let col = i % width;
        if col + 1 < width && i + 1 < n {
            room = room.with_exit(Direction::East, names[i + 1]);
        }
        if col > 0 {
            room = room.with_exit(Direction::West, names[i - 1]);
        }
        if i + width < n {
            room = room.with_exit(Direction::South, names[i + width]);
        }
        if row > 0 {
            room = room.with_exit(Direction::North, names[i - width]);
        }

        for res in &gr.resources {
            let mut resource = Resource::new(
                &res.name,
                res.kind,
                &format!("A synthetic {} resource in {}.", res.kind, res.region),
            );
            resource.public = res.public;
            resource.encrypted = res.encrypted;
            resource.locked = res.locked;
            resource.monthly_cost = res.monthly_cost;
            room = room.with_resource(resource);
        }
        rooms.push(room);
    }

    World::new(
        rooms,
        &gen_rooms[0].name,
        &format!("Contoso-Synthetic-{n}rg (mock)"),
    )
}

/// Build a [`FakeAzRunner`] that serves the same generated estate as
/// `az`-shaped canned JSON responses, so Dungeon Crawler Mode's mock backend
/// can render a sized map through the exact same [`crate::dungeon::map`]
/// path a real subscription would use.
pub fn fake_runner(params: &MockSizeParams) -> FakeAzRunner {
    let gen_rooms = generate_rooms(params);

    let mut group_entries = Vec::with_capacity(gen_rooms.len());
    let mut runner = FakeAzRunner::new();
    let mut resource_json_by_group: Vec<(String, String)> = Vec::with_capacity(gen_rooms.len());

    for gr in &gen_rooms {
        group_entries.push(format!(
            r#"{{"id":"/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/{name}","location":"{region}","name":"{name}"}}"#,
            name = gr.name,
            region = gr.region
        ));

        let mut entries = Vec::with_capacity(gr.resources.len());
        for res in &gr.resources {
            entries.push(format!(
                r#"{{"id":"/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/{rg}/providers/{kind}/{rname}","name":"{rname}","type":"{kind}","location":"{region}"}}"#,
                rg = gr.name,
                kind = res.kind,
                rname = res.name,
                region = res.region
            ));
        }
        resource_json_by_group.push((gr.name.clone(), format!("[{}]", entries.join(","))));
    }

    let group_json = format!("[{}]", group_entries.join(","));
    runner = runner.with(&["group", "list", "-o", "json"], &group_json);
    for (name, json) in &resource_json_by_group {
        runner = runner.with(
            &[
                "resource",
                "list",
                "--resource-group",
                name.as_str(),
                "-o",
                "json",
            ],
            json,
        );
    }
    runner
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{mock::MockBackend, Backend};
    use std::collections::HashSet;

    #[test]
    fn preset_parses_case_insensitively() {
        assert_eq!(MockSizePreset::parse("Large"), Some(MockSizePreset::Large));
        assert_eq!(MockSizePreset::parse("MED"), Some(MockSizePreset::Medium));
        assert_eq!(MockSizePreset::parse("nonsense"), None);
    }

    #[test]
    fn parse_explicit_counts_and_seed() {
        let params = MockSizeParams::parse("200x15:7").unwrap();
        assert_eq!(params.resource_groups, 200);
        assert_eq!(params.resources_per_group, 15);
        assert_eq!(params.seed, 7);
    }

    #[test]
    fn parse_bare_count() {
        let params = MockSizeParams::parse("50").unwrap();
        assert_eq!(params.resource_groups, 50);
        assert_eq!(params.seed, DEFAULT_SEED);
    }

    #[test]
    fn parse_rejects_garbage() {
        assert!(MockSizeParams::parse("not-a-size").is_err());
    }

    #[test]
    fn env_values_none_when_unset() {
        assert!(MockSizeParams::from_env_values(None, None, None, None).is_none());
    }

    #[test]
    fn env_values_rgs_and_per_override_size() {
        let result = MockSizeParams::from_env_values(
            Some("small".to_string()),
            Some("30".to_string()),
            Some("2".to_string()),
            Some("99".to_string()),
        )
        .unwrap()
        .unwrap();
        assert_eq!(result.resource_groups, 30);
        assert_eq!(result.resources_per_group, 2);
        assert_eq!(result.seed, 99);
    }

    #[test]
    fn default_mock_world_is_unchanged_by_this_module() {
        // Sanity check: the plain default backend must not be affected by
        // anything in this module (it never calls into it).
        let w1 = MockBackend::new().build_world().unwrap();
        let w2 = MockBackend::new().build_world().unwrap();
        assert_eq!(w1.rooms_len(), w2.rooms_len());
        assert_eq!(w1.rooms_len(), 5);
    }

    #[test]
    fn sized_generation_yields_expected_counts() {
        let params = MockSizeParams {
            resource_groups: 37,
            resources_per_group: 4,
            seed: 123,
        };
        let world = generate_world(&params).unwrap();
        assert_eq!(world.rooms_len(), 37);
    }

    #[test]
    fn sized_generation_is_reproducible() {
        let params = MockSizeParams {
            resource_groups: 42,
            resources_per_group: 5,
            seed: 12345,
        };
        let gen1 = generate_rooms(&params);
        let gen2 = generate_rooms(&params);
        assert_eq!(gen1.len(), gen2.len());
        for (a, b) in gen1.iter().zip(gen2.iter()) {
            assert_eq!(a.name, b.name);
            assert_eq!(a.region, b.region);
            assert_eq!(a.monitored, b.monitored);
            assert_eq!(a.resources.len(), b.resources.len());
            for (ra, rb) in a.resources.iter().zip(b.resources.iter()) {
                assert_eq!(ra.name, rb.name);
                assert_eq!(ra.kind, rb.kind);
                assert_eq!(ra.public, rb.public);
                assert_eq!(ra.encrypted, rb.encrypted);
                assert_eq!(ra.locked, rb.locked);
                assert_eq!(ra.monthly_cost, rb.monthly_cost);
            }
        }

        let world1 = generate_world(&params).unwrap();
        let world2 = generate_world(&params).unwrap();
        assert_eq!(world1.rooms_len(), world2.rooms_len());
        assert_eq!(world1.subscription, world2.subscription);
    }

    #[test]
    fn different_seeds_yield_different_worlds() {
        let params_a = MockSizeParams {
            resource_groups: 20,
            resources_per_group: 3,
            seed: 1,
        };
        let params_b = MockSizeParams {
            resource_groups: 20,
            resources_per_group: 3,
            seed: 2,
        };
        let a = generate_rooms(&params_a);
        let b = generate_rooms(&params_b);
        let a_names: Vec<&str> = a.iter().map(|r| r.name.as_str()).collect();
        let b_names: Vec<&str> = b.iter().map(|r| r.name.as_str()).collect();
        assert_ne!(a_names, b_names);
    }

    #[test]
    fn generated_world_resource_count_matches_target() {
        let params = MockSizeParams {
            resource_groups: 10,
            resources_per_group: 6,
            seed: 7,
        };
        let rooms = generate_rooms(&params);
        let total: usize = rooms.iter().map(|r| r.resources.len()).sum();
        assert_eq!(total, 10 * 6);
    }

    fn opposite(d: Direction) -> Direction {
        match d {
            Direction::North => Direction::South,
            Direction::South => Direction::North,
            Direction::East => Direction::West,
            Direction::West => Direction::East,
            Direction::Up => Direction::Down,
            Direction::Down => Direction::Up,
        }
    }

    /// Depth-first walk of the actual navigable world graph (via `World::go`,
    /// backtracking through the opposite direction after each descent),
    /// recording every room name visited.
    fn dfs_visit(world: &mut World, seen: &mut HashSet<String>) {
        let here = world.current_room().name.clone();
        if !seen.insert(here) {
            return;
        }
        let exits: Vec<Direction> = world.current_room().exits.keys().copied().collect();
        for dir in exits {
            let dest = world.current_room().exits.get(&dir).cloned().unwrap();
            if seen.contains(&dest) {
                continue;
            }
            if world.go(dir).is_ok() {
                dfs_visit(world, seen);
                let _ = world.go(opposite(dir));
            }
        }
    }

    #[test]
    fn generated_world_is_fully_connected() {
        for n in [1usize, 2, 5, 17, 50, 121] {
            let params = MockSizeParams {
                resource_groups: n,
                resources_per_group: 1,
                seed: 5,
            };
            let mut world = generate_world(&params).unwrap();
            assert_eq!(world.rooms_len(), n, "room count mismatch for n={n}");

            let mut seen: HashSet<String> = HashSet::new();
            dfs_visit(&mut world, &mut seen);
            assert_eq!(seen.len(), n, "not all rooms reachable for n={n}");
        }
    }

    #[test]
    fn generated_resource_types_are_icon_known() {
        use crate::dungeon::icons::{icon_for, DEFAULT_ICON};
        let params = MockSizeParams {
            resource_groups: 15,
            resources_per_group: 6,
            seed: 3,
        };
        let rooms = generate_rooms(&params);
        for room in &rooms {
            for res in &room.resources {
                assert_ne!(
                    icon_for(res.kind),
                    DEFAULT_ICON,
                    "resource type '{}' is not in the icon-known set",
                    res.kind
                );
            }
        }
    }

    #[test]
    fn fake_runner_serves_group_and_resource_lists() {
        use crate::az_runner::AzRunner;
        let params = MockSizeParams {
            resource_groups: 4,
            resources_per_group: 2,
            seed: 9,
        };
        let runner = fake_runner(&params);
        let out = runner.run(&["group", "list", "-o", "json"]).unwrap();
        assert!(out.status.success());
        let groups: Vec<serde_json::Value> = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(groups.len(), 4);
    }
}
