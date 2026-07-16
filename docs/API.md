# AzZork API / Module Reference

This reference documents the internal architecture of AzZork for contributors
and anyone embedding the engine. AzZork is a single binary crate (`azork`) with
**a small set of external dependencies** for JSON parsing, rendering, and update verification; the core game logic still stays dependency-light.

For player-facing docs see the [Usage guide](USAGE.md) and
[Configuration reference](CONFIGURATION.md).

## Module map

```
src/
├── main.rs            REPL: banner, input loop, dispatch, y/N confirmation
├── parser.rs          Total input parser: text -> Command
├── world.rs           World model: rooms, resources, hazards, Grue, scoring
├── quests.rs          Quest/QuestProgress: read-only governance objectives over World
├── az_runner.rs       AzRunner seam: the one place `az` is invoked
├── capabilities/      Dynamic capability derivation + persistent registry
│   ├── mod.rs         Capability type
│   ├── derive.rs      Parse `az [<group>] --help` into capabilities
│   ├── registry.rs    CapabilityRegistry: lookup, suggest, help_text, cache I/O
│   └── autodiscover.rs  Startup auto-discovery of new `az` groups (see below)
├── agent/
│   └── mod.rs         IntentResolver + Adapter trait + offline MockAdapter
├── dungeon/
│   ├── mod.rs         Dungeon Crawler Mode wiring
│   ├── map.rs         Read-only subscription -> dungeon graph builder
│   ├── render.rs      Native HTML/SVG renderer (scrubbed, deterministic)
│   ├── server.rs      Loopback-only local HTTP server + JSON API
│   ├── commands.rs    Read-only `az` suggestion builder + validation
│   ├── links.rs       Azure portal deep links with ARM-id validation
│   ├── cli.rs         `azork crawl` / `azork dungeon` argument parsing
│   └── playwright.rs  Optional best-effort browser renderer
├── memory/
│   └── mod.rs         GraphMemory: dependency-free, ladybug-style persistent graph memory
├── oit/                Outside-In-Testing agent library core (pure, offline-testable)
│   ├── mod.rs         Module wiring
│   ├── guardrails.rs  Cost gate, ownership/cleanup tagging, isolation rules
│   ├── usecases.rs    Use-case catalog + friction detection
│   └── report.rs      Markdown friction-report rendering
├── bin/
│   └── azork-oit.rs   Live OIT driver binary: preflight, create/exercise/teardown
└── backend/
    ├── mod.rs         Backend trait + select()
    ├── mock.rs        Default offline synthetic world (fixed, hand-authored)
    ├── mock_gen.rs    Deterministic, seeded generator for sized synthetic worlds
    │                  (--mock-size / AZORK_MOCK_SIZE, see below)
    └── az.rs          Optional read-only live-Azure world (driven via AzRunner, with
                        timeout/zombie-process/pipe-deadlock hardening)
```

The [`src/agent_engine/`](../src/agent_engine/mod.rs) module is part of the
main crate: it connects AzZork's `CapabilityRegistry` into the
`recipe-runner-rs` agentic engine via an `AzorkAdapter`, compiled and tested by
default `cargo build`/`cargo test` (no `[workspace]` table needed — it depends
on a normal git dependency pinned to a specific commit, see
[`recipe-runner-rs`]).

One **optional companion crate** lives alongside the root package but is never
compiled by `cargo build`/`cargo test` at the repo root (no `[workspace]` table,
so it is fully opt-in):

- [`memory-store/`](../memory-store/README.md) — mirrors `GraphMemory` into a
  durable, SQLite-backed `amplihack-memory` store (`PersistentStore`).

The Azure CLI extension under [`azext/`](../azext/README.md) (`azext_azork`) is
pure Python and lives outside the Rust crate graph entirely; it shells out to
the compiled `azork` binary.

Data flows one way at startup: a `Backend` **builds** a `World`; thereafter the
REPL parses input into `Command`s and applies them to the `World`. Unknown input
is routed to the `agent::IntentResolver`, which consults the
`capabilities::CapabilityRegistry` (grown at runtime via `learn`).

```
input ──parser::parse──▶ Command ──main::handle──▶ World mutation ──▶ text out
                                     │                    ▲
                                     ├─ Unknown ─▶ IntentResolver ─┘
                                     └─ Learn ──▶ CapabilityRegistry ◀─ AzRunner ◀─ `az --help`
Backend::build_world ────────────────────────────────────────────────┘ (once, at startup)
```

All `az` access — the live `AzBackend`, capability derivation, and Dungeon Crawler Mode's map enumeration — passes through the `AzRunner` trait, so tests inject a `FakeAzRunner`
and never touch the real CLI or network.

### Third-party dependencies

The core game, self-evolution, and graph memory add no license obligations
beyond the small set of dependencies in the main `Cargo.toml`. The default
build also drives one agentic integration and keeps one durable-storage
integration opt-in:

- **`src/agent_engine/`** (module, main crate) → drives the MIT-licensed
  [`recipe-runner-rs`] agentic `Adapter` engine (and its transitive deps),
  depended on via a normal git dependency pinned to a specific commit.
  Compiled and tested by default `cargo build`/`cargo test` — no opt-in step
  required.
- **`memory-store/`** (separate companion crate) → durable graph memory over the
  MIT-licensed `amplihack-memory` library (SQLite-backed, `lbug`-capable). Kept
  out of the azork package so the default build stays zero-dep for that
  integration.

Both are MIT-compatible with this project's MIT license. `agent_engine` compiles
into the default `cargo build`/`cargo test`; `memory-store` does not.

The Azure CLI extension under [`azext/`](../azext/) is pure Python with **zero**
third-party `install_requires` (it uses only the Azure CLI's own SDK).

[`recipe-runner-rs`]: https://github.com/rysweet/amplihack-recipe-runner

## `parser` module

Turns a raw line of player input into a structured, total `Command`. The parser
never panics and never returns an error type — unrecognized input becomes
`Command::Unknown`.

### `enum Direction`

`North`, `South`, `East`, `West`, `Up`, `Down`.
Derives `Debug, Clone, Copy, PartialEq, Eq, Hash` (used as a `HashMap` key for
room exits).

| Method | Signature | Description |
| --- | --- | --- |
| `from_token` | `fn from_token(tok: &str) -> Option<Direction>` | Parses full words or single-letter abbreviations (`n`, `s`, `e`, `w`, `u`, `d`). Returns `None` for anything else. |
| `name` | `fn name(&self) -> &'static str` | Canonical lowercase name of the direction. |

### `enum Command`

The full set of parsed commands. Derives `Debug, Clone, PartialEq, Eq`.

| Variant | Meaning |
| --- | --- |
| `Look` | Describe the current room. |
| `Examine(String)` | Inspect a named resource. |
| `Go(Direction)` | Move in a direction. |
| `Take(String)` | Acquire a resource (confirmed by REPL). |
| `Drop(String)` | Delete a resource (confirmed by REPL). |
| `Lock(String)` | Harden a resource. |
| `Unlock(String)` | Remove a management lock from a resource. |
| `Resize(String)` | Right-size a resource to cut its monthly cost. |
| `Monitor` | Enable monitoring in the current room. |
| `Inventory` | List carried resources. |
| `Score` | Report governance posture. |
| `Quest` | Show themed quest progress (read-only, derived from `World`). |
| `Cast(String)` | Cast a spell (currently `deploy [template]`). |
| `Learn(String)` | Introspect `az <group> --help` and grow the capability registry. |
| `Capabilities` | List the `az` capabilities learned so far. |
| `Recall(String)` | Ranked recall over persistent memory for a free-text query (verbatim capture, see below). |
| `Friction(String)` | Record a friction note into persistent memory (verbatim capture, see below). |
| `Memory` | Summarise what AzZork remembers (counts by kind + recent notes). |
| `Help` | Show help (built-in verbs plus learned capabilities). |
| `Quit` | Leave the game. |
| `Empty` | Player entered nothing. |
| `Unknown(String)` | Unrecognized input; routed to the `IntentResolver` rather than rejected. |

### `fn parse`

```rust
pub fn parse(input: &str) -> Command
```

Parsing rules:

1. Lowercase the input and split on whitespace.
2. Drop filler words: `the`, `a`, `an`, `at`, `to`, `into`, `on`, `my`.
3. Empty input → `Command::Empty`.
4. A leading bare direction (`north`, `n`, …) → `Command::Go`.
5. Otherwise match the verb (with aliases) and treat the remaining tokens as the
   target/argument. A verb requiring an argument with none given →
   `Command::Unknown`.
6. Exception — `Command::Recall` and `Command::Friction` take their argument
   **verbatim** from the original (non-lowercased) input instead of the
   filler-stripped, lowercased tokens: only the leading verb token is removed;
   case and filler words are preserved. Runs of internal whitespace are still
   collapsed to single spaces (via `split_whitespace().join(" ")`), and
   leading/trailing whitespace is trimmed, so original spacing is not
   preserved — only word content, order, and case are.

Recognized verb aliases:

| Command | Verbs |
| --- | --- |
| `Look` | `look`, `l` |
| `Examine` | `examine`, `x`, `inspect`, `show` |
| `Go` | `go`, `move`, `walk` (or a bare direction) |
| `Take` | `take`, `get`, `grab`, `acquire` |
| `Drop` | `drop`, `delete`, `release`, `rm` |
| `Lock` | `lock`, `secure` |
| `Unlock` | `unlock`, `unward`, `unsecure` |
| `Resize` | `resize`, `right-size`, `rightsize`, `scale`, `downsize` |
| `Monitor` | `monitor`, `light` |
| `Inventory` | `inventory`, `i`, `inv` |
| `Score` | `score` |
| `Quest` | `quest`, `quests` |
| `Cast` | `cast <spell>`, or `deploy [template]` as a convenience alias for `cast deploy` |
| `Learn` | `learn`, `discover`, `study` |
| `Capabilities` | `capabilities`, `caps`, `powers`, `spells` |
| `Recall` | `recall`, `remember` (verbatim free-text query) |
| `Friction` | `friction`, `note`, `gripe` (verbatim free-text note) |
| `Memory` | `memory`, `mem`, `recollect` |
| `Help` | `help`, `?`, `h` |
| `Quit` | `quit`, `q`, `exit` |

## `world` module

The complete, mutable game state and all game logic.

### `struct Resource`

An Azure resource rendered as a dungeon object/creature.
Fields: `name`, `kind`, `description`, `locked: bool`, `public: bool`,
`encrypted: bool`, `monthly_cost: u32`.

| Method | Signature | Description |
| --- | --- | --- |
| `new` | `fn new(name, kind, description) -> Resource` | Constructor. Defaults: `locked=false`, `public=false`, `encrypted=true`, `monthly_cost=0`. |
| `hazards` | `fn hazards(&self) -> u32` | Count of governance hazards: public, unencrypted, unlocked, and cost `≥ 500`. |
| `hazard_report` | `fn hazard_report(&self) -> String` | One-line prose hazard summary used by `examine`. |

### `struct Room`

A resource group. Fields: `name`, `description`, `region`, `monitored: bool`,
`exits: HashMap<Direction, String>`, `resources: Vec<Resource>`.

| Method | Signature | Description |
| --- | --- | --- |
| `new` | `fn new(name, description, region, monitored) -> Room` | Constructor. |
| `with_exit` | `fn with_exit(self, dir, dest) -> Room` | Builder: add an exit to a destination room. |
| `with_resource` | `fn with_resource(self, res) -> Room` | Builder: add a resource. |
| `is_dark` | `fn is_dark(&self) -> bool` | `true` when the room is unmonitored (Grue territory). |

### `enum GrueOutcome`

`Safe`, `Lurking`, `Devoured` — the result of a single turn's Grue check.

### `struct World`

The top-level game state. Public fields: `subscription: String`,
`game_over: bool`.

| Method | Signature | Description |
| --- | --- | --- |
| `new` | `fn new(rooms: Vec<Room>, start: &str, subscription: &str) -> World` | Build the world from rooms and a starting room name. |
| `seed_rng` | `fn seed_rng(&mut self, seed: u64)` | Seed the deterministic RNG (used by tests for reproducible Grue attacks). |
| `current_room` | `fn current_room(&self) -> &Room` | Reference to the current room. |
| `moves` | `fn moves(&self) -> u32` | Number of moves taken. |
| `look` | `fn look(&self) -> String` | Describe the current room (dark rooms warn about the Grue). |
| `examine` | `fn examine(&self, target: &str) -> String` | Inspect a resource in the room or inventory (prefix/case-insensitive match). Dark rooms refuse. |
| `go` | `fn go(&mut self, dir) -> Result<String, String>` | Move; `Ok(description)` or `Err(message)` when there is no exit. |
| `take` | `fn take(&mut self, target) -> String` | Move a resource from the room into inventory. Fails in the dark. |
| `drop_item` | `fn drop_item(&mut self, target) -> String` | Delete a resource from inventory or room. Refuses locked resources. Caller handles confirmation. |
| `lock` | `fn lock(&mut self, target) -> String` | Harden a resource: sets `locked`, clears `public`, sets `encrypted`. |
| `unlock` | `fn unlock(&mut self, target) -> String` | Remove a management lock (clears `locked`) so the resource can change or be deleted. |
| `resize` | `fn resize(&mut self, target) -> String` | Right-size a resource, roughly halving `monthly_cost`; clears the cost-overrun hazard once it drops below `500`. |
| `monitor` | `fn monitor(&mut self) -> String` | Enable monitoring in the current room; resets the darkness streak. |
| `inventory` | `fn inventory(&self) -> String` | List carried resources. |
| `total_hazards` | `fn total_hazards(&self) -> u32` | Sum of resource hazards across all rooms and inventory, plus one per dark room. |
| `all_resources` | `fn all_resources(&self) -> Vec<&Resource>` | Every resource across all rooms plus inventory, aggregated with the same traversal `total_hazards` uses. Read-only; used by the `quests` module. |
| `score` | `fn score(&self) -> String` | Governance posture string: `100 - hazards*5` (floored at 0), a rank, and move count. |
| `grue_check` | `fn grue_check(&mut self) -> GrueOutcome` | Run one turn's Grue check; escalates death probability with consecutive dark turns and sets `game_over` on `Devoured`. |

#### Grue probability model

`grue_check` tracks a `darkness_streak`. Leaving the dark (or `monitor`) resets
it to 0.

| Consecutive dark turns | Outcome |
| --- | --- |
| 1 | Always `Lurking` (warning). |
| 2 | ~25% `Devoured`, else `Lurking`. |
| 3 | ~50% `Devoured`. |
| 4+ | ~75% `Devoured`. |

Randomness uses a deterministic xorshift64 generator so tests are reproducible
via `seed_rng`.

## `quests` module

Read-only governance objectives layered over `World`. No traits, no
registry/plugin system, no config or save format — a fixed `Vec<Quest>` and
two small types.

### `struct QuestProgress`

`Copy`-able summary of how a quest is doing: `done: usize`, `total: usize`,
`complete: bool` (`true` iff `done == total`, including the vacuous case where
`total == 0`).

### `struct Quest`

| Field | Type | Description |
| --- | --- | --- |
| `name` | `&'static str` | Themed quest title, e.g. `"Secure the Realm"`. |
| `description` | `&'static str` | One-line statement of the goal. |
| `completion_line` | `&'static str` | Themed flourish printed once `complete`. |
| `satisfies` | `fn(&Resource) -> bool` (private) | Per-resource predicate defining "done" for this quest. |

| Method | Signature | Description |
| --- | --- | --- |
| `evaluate` | `fn evaluate(&self, world: &World) -> QuestProgress` | Runs `satisfies` over `world.all_resources()` and counts matches. Pure; never mutates `world`. |

### `fn builtin_quests`

```rust
pub fn builtin_quests() -> Vec<Quest>
```

Returns the three built-in quests, each mapped 1:1 onto an existing `Resource`
hazard field — no new hazard sources are introduced:

| Quest | `satisfies` |
| --- | --- |
| Secure the Realm | `!resource.public` |
| Seal the Vaults | `resource.encrypted` |
| Lift the Curse | `resource.locked` |

`main::quests_report(world: &World) -> String` (private to `main.rs`) formats
these into the REPL's `quest` output, printing `<done>/<total> resources
secured` per quest plus the `completion_line` when `complete`.

## `backend` module

### `trait Backend`

```rust
pub trait Backend {
    fn name(&self) -> &str;                       // shown in the banner
    fn build_world(&self) -> Result<World, String>;
}
```

Implementors construct the initial `World`. This is the single extension point:
add a new data source by implementing `Backend` and registering it in `select`.

### `fn select`

```rust
pub fn select(id: &str) -> Box<dyn Backend>
```

Maps a backend id to an implementation. `az` / `real` / `azure` →
`AzBackend`; everything else → `MockBackend` (safe default).

### `struct MockBackend`

Builds the fixed offline world (see
[Configuration reference](CONFIGURATION.md#the-mock-backend-default)).
`name()` → `"mock (offline)"`. Guarantees at least one dark room.

### `struct AzBackend`

Builds a world from the live subscription via read-only `az` calls, all routed
through an injected `AzRunner` (`AzBackend::new()` uses `ProcessAzRunner`;
`AzBackend::with_runner(..)` accepts any runner, e.g. `FakeAzRunner` in tests).
`name()` → `"az (live Azure)"`. See
[the `az` backend](CONFIGURATION.md#the-az-backend-live-azure) for the exact
commands and safety guarantees.

## `backend::mock_gen` module

Deterministic, seeded generator for **sized** synthetic mock worlds, used to
scale-test the Dungeon Crawler map (room sizing, corridor spacing,
decorations) offline. This module is additive: it never runs unless a size is
explicitly requested via `--mock-size` / `AZORK_MOCK_SIZE*`; the default
`MockBackend` (no size requested) is untouched and keeps building the fixed,
hand-authored world exactly as before. See
[Generating a sized mock tenant](DUNGEON-CRAWLER.md#generating-a-sized-mock-tenant)
for the user-facing CLI/env grammar.

```rust
pub const DEFAULT_SEED: u64 = 42;

pub enum MockSizePreset { Small, Medium, Large, Huge }
impl MockSizePreset {
    pub fn parse(s: &str) -> Option<MockSizePreset>;
}

pub struct MockSizeParams {
    pub resource_groups: usize,
    pub resources_per_group: usize,
    pub seed: u64,
}
impl MockSizeParams {
    pub fn from_preset(preset: MockSizePreset) -> MockSizeParams;
    pub fn parse(spec: &str) -> Result<MockSizeParams, String>;
    pub fn from_env() -> Option<Result<MockSizeParams, String>>;
}

pub fn generate_world(params: &MockSizeParams) -> Result<World, String>;
pub fn fake_runner(params: &MockSizeParams) -> FakeAzRunner;
```

- **`MockSizePreset::parse`** — case-insensitive lookup of `small` / `medium`
  (`med` shorthand accepted) / `large` / `huge`; returns `None` for anything
  else (callers fall back to `MockSizeParams::parse` for the
  explicit-count/`COUNTxPER_GROUP` grammar).
- **`MockSizeParams::from_preset`** — resolves a named preset to its
  `(resource_groups, resources_per_group)` counts (5×3, 25×5, 100×8, 500×10
  respectively) with `seed = DEFAULT_SEED`.
- **`MockSizeParams::parse`** — parses the full `--mock-size` / `AZORK_MOCK_SIZE`
  grammar: a bare preset name (`large`), a bare resource-group count (`200`),
  an explicit `COUNTxPER_GROUP` pair (`300x12`), or any of the above with a
  `:SEED` suffix (`large:7`) to override `DEFAULT_SEED`. Returns `Err(String)`
  with a human-readable message on malformed input (never panics).
- **`MockSizeParams::from_env`** — resolves `AZORK_MOCK_SIZE` (full grammar)
  then applies `AZORK_MOCK_RGS` / `AZORK_MOCK_RESOURCES_PER_RG` /
  `AZORK_MOCK_SEED` as individual overrides on top; returns `None` when none of
  the four env vars are set (i.e. "no size requested, use the default fixed
  world"), or `Some(Err(..))` if what *is* set fails to parse.
- **`generate_world`** — pure function: `MockSizeParams` in, a fully populated
  `World` out (resource groups as rooms, resources attached to rooms, exits/
  corridors between rooms). No I/O, no wall-clock, no OS randomness — the same
  `MockSizeParams` (including `seed`) always produces an identical `World`.
  Resource groups are laid out on a grid and connected so every room is
  reachable from the start room; resource types are drawn from the same set
  the map's icon renderer recognizes (storage accounts, VMs, vnets, web apps,
  key vaults, AKS, SQL, Cosmos DB, NICs, NSGs, public IPs, load balancers).
- **`fake_runner`** — convenience wrapper that builds a `FakeAzRunner` (see
  `az_runner` below) pre-seeded with the sized world's equivalent `az` CLI
  output, for exercising the `az`-backend code paths against a large synthetic
  estate in tests without a live subscription.

## `az_runner` module

The single seam through which AzZork ever invokes the `az` CLI.

```rust
pub trait AzRunner {
    fn run(&self, args: &[&str]) -> std::io::Result<std::process::Output>;
}
```

- **`ProcessAzRunner`** — production impl; shells out to `az` on `PATH`.
- **`FakeAzRunner`** — test impl; returns canned `(stdout, success)` keyed by the
  exact argument vector (`.with(...)` / `.with_failure(...)`).

## `capabilities` module

Dynamic derivation and persistence of AzZork's runtime vocabulary.

- **`struct Capability`** — `{ group, verb, summary, command_path, status }`;
  helpers `key()`, `az_args()`, `help_line()`.
- **`derive::derive_groups` / `derive::derive_group_capabilities`** — parse
  `az --help` / `az <group> --help` (folds wrapped summaries, extracts
  `[Preview]`-style status tags).
- **`registry::CapabilityRegistry`** — `learn_group`, `get`, `find_by_verb`,
  `suggest`, `groups`, `help_text`, and dependency-free `load`/`save` to a
  tab-separated cache (`default_cache_path()` honours `AZORK_CACHE_DIR` /
  `XDG_DATA_HOME`).
- **`autodiscover` module** — pure, synchronous, offline-testable functions
  driving startup auto-discovery (a background thread in `main.rs` wraps the
  streaming variant; see [Auto-Discovery guide](AUTODISCOVERY.md) for the
  player-facing behavior):
  - **`AUTODISCOVER_ENV`** / **`autodiscover_enabled() -> bool`** — the
    `AZORK_AUTODISCOVER` opt-out (`0`/`false`/`no`, case-insensitive).
  - **`struct GroupResult { group: String, outcome: Result<Vec<Capability>, String> }`**
    — the outcome of attempting to learn one group.
  - **`struct AppliedGroup { group: String, result: Result<usize, String> }`**
    — the outcome of folding one `GroupResult` into a registry (added-count or
    error).
  - **`discover_new_groups(runner: &dyn AzRunner, known_groups: &[String]) -> Result<Vec<String>, String>`**
    — runs `az --help`, returns the top-level groups not already in
    `known_groups`.
  - **`learn_groups(runner: &dyn AzRunner, groups: &[String]) -> Vec<GroupResult>`**
    — runs `az <group> --help` for each group via `derive::derive_group_capabilities`,
    one `GroupResult` per group.
  - **`run_startup_autodiscovery(runner: &dyn AzRunner, known_groups: &[String]) -> Vec<GroupResult>`**
    — synchronous convenience wrapper: `discover_new_groups` then
    `learn_groups`; used directly in tests (no thread needed).
  - **`apply_learned(registry: &mut CapabilityRegistry, results: impl IntoIterator<Item = GroupResult>) -> Vec<AppliedGroup>`**
    — folds a batch of `GroupResult`s into `registry`, returning one
    `AppliedGroup` per input.
  - **`stream_startup_autodiscovery(runner: &dyn AzRunner, known_groups: &[String], cancel: &AtomicBool, tx: &Sender<GroupResult>)`**
    — the production entry point: streams each `GroupResult` over `tx` as
    soon as it's learned (so `main.rs` can apply capabilities incrementally
    between turns) and checks `cancel` before/between groups so discovery
    yields to player input.

## `agent` module

Agentic resolution of unknown/ambiguous intent — AzZork never dead-ends.

- **`trait Adapter`** — `resolve(&self, input, &CapabilityRegistry) -> Resolution`.
- **`struct MockAdapter`** — deterministic, offline adapter that ranks learned
  capabilities against the input.
- **`enum Resolution`** — `Verb` | `Suggestions` | `Unresolved`, each with
  `narrate()`.
- **`IntentResolver<A: Adapter>`** — ties an adapter to a registry; never fails.

The embedded [`agent_engine`](../src/agent_engine/mod.rs) module's
`AzorkAdapter` implements a *different* trait — `recipe-runner-rs`'s own
`Adapter` seam — and uses `MockAdapter` (via this module's `Adapter` trait)
internally to resolve its agent steps, letting an amplihack recipe compose
AzZork's offline intent resolution with other steps (e.g. bash).

## `memory` module

Dependency-free persistent graph memory, accumulated across sessions.

- **`struct GraphMemory`** — typed nodes (`room`, `object`, `verb`, `intent`,
  `friction`) with importance/usage weighting; `record`, `touch`, `recall`,
  `summary`.
- **`load()` / `save()`** — line-based, dependency-free on-disk format under
  `~/.local/share/azork/memory.graph` (honours `AZORK_CACHE_DIR` /
  `XDG_DATA_HOME`, mirroring the capability cache).
- Recalled at startup (banner shows `[memory: recalled N remembered nodes]`) and
  updated as the game and OIT agent play.

The optional [`memory-store`](../memory-store/README.md) companion crate's
`PersistentStore` mirrors every `GraphMemory` node **and edge** into a durable,
SQLite-backed `amplihack-memory` store, adding full-text ranked recall — see
its README for the node → `Experience` field mapping.

## `oit` module (Outside-In-Testing agent core)

A pure, fully offline-testable library that backs the live `azork-oit` binary
(`src/bin/azork-oit.rs`). Kept separate from the binary so every safety rule and
heuristic is exercised in unit tests with no `az` calls and no network.

- **`guardrails` submodule** — the mission's hard safety contract, enforced in
  code, not just convention:
  - `assess_cost(est_monthly_usd) -> CostDecision` — rejects untrusted
    (negative/NaN) and over-cap (`COST_CAP_USD = $500`) estimates; every create
    in the live binary is gated on this before any `az` call.
  - `oit_tags(ttl_epoch)` / `tag_args(ttl_epoch)` — canonical
    `azork-oit=1`, `owner=azork-oit`, `ttl=<epoch>` tag set applied to every
    created resource.
  - `is_own_resource(tags)` / `guard_mutation(tags)` — refuses to mutate or
    delete anything not carrying the agent's own tags.
  - `oit_rg_name(suffix)` / `is_oit_rg(name)` — enforces the `azork-oit-*`
    resource-group naming/isolation convention.
  - `CheapResource` — the curated catalog (`ResourceGroup`, `StorageStandardLrs`)
    of resource kinds the agent is allowed to create, each with a conservative
    cost estimate; extending this catalog with pricier kinds requires feeding a
    real price estimate into `assess_cost` to keep the cap meaningful.
- **`usecases` submodule** — `catalog()` (a broad, categorised set of scenarios:
  navigation, examination, creation, security, governance, deployment,
  discovery, memory) and `detect_friction(command, output)`, a pure function
  classifying azork's response as `Unresolved`, `Empty`, `MissingCapability`, or
  `ConfusingMessage` friction (or none).
- **`report` submodule** — `ReportData` / `UseCaseRun`, rendered via
  `to_markdown()` into the friction report written by `azork-oit` (see
  [`docs/oit-friction-report.md`](oit-friction-report.md) for a sample).

The `azork-oit` binary itself (`src/bin/azork-oit.rs`) is a thin live driver:
subscription/tenant **preflight** (overridable with `AZORK_OIT_SUBSCRIPTION` /
`AZORK_OIT_TENANT`, see the [Configuration reference](CONFIGURATION.md)),
guardrailed create → drive the `catalog()` use cases against the real `azork`
binary over stdin/stdout → verified teardown → friction-report write. It has no
runtime dependencies of its own beyond the standard library and the `az` CLI on
`PATH`.

## `main` (REPL)

Entry point and orchestration. Responsibilities:

- Resolve the backend id from `--backend` / `-b` / `AZORK_BACKEND` and call
  `backend::select` + `build_world` (exiting with guidance on failure).
- Print the banner, backend/subscription status line, and initial room.
- Run the input loop: `parse` → `handle` → `run_grue_check`.
- Prompt for **y/N** confirmation on `take` and `drop` (default No).
- Implement the `cast deploy [template]` spell as a mock, credential-free
  deployment narration.
- Handle `learn`/`capabilities`, append learned capabilities to `help`, and route
  `Unknown` input through the `IntentResolver` (never a hard failure).

## Testing

The root crate's suite has **274 tests** (unit tests colocated with each module
under `#[cfg(test)]`, plus external contract/integration tests in `tests/` that
drive the public API of the `azork` library crate and the `azork-oit` binary's
library core). Counts drift as the suite grows — re-run `cargo test --all` for
the exact current total.

Colocated unit tests:

- **`parser.rs`** — verb/alias parsing (including `unlock`/`resize`), bare
  directions, filler stripping, multi-word targets, unknown input.
- **`world.rs`** — `look`, `go`, `take`/`drop`, `lock` hazard reduction,
  `unlock` reversal, `resize` cost reduction, scoring, and the Grue escalation
  model (with a seeded RNG).
- **`quests.rs`** — partial progress on a hazard-laden world, all-quests-complete
  on a clean world, and vacuous completion on a world with zero resources.
- **`backend/mock.rs`** — the world builds, starts lit, exposes a reachable dark
  room, seeds fixable hazards, and is fully winnable to a perfect **100/100**.
- **`main.rs`** — `cast deploy` is mock-safe, unknown spells are rejected, and
  the confirmation helper reads yes / defaults to no on EOF.
- **`memory/mod.rs`** — node recording, recall ranking, on-disk round-trip.
- **`oit/guardrails.rs`** — cost-gate boundaries (cap, cheap threshold,
  untrusted estimates), tag composition, resource-group naming/isolation,
  ownership-gated mutation.
- **`oit/usecases.rs`** — catalog breadth/uniqueness/category coverage, and
  every `detect_friction` classification (including the "deploy flavour text is
  not friction" false-positive guard) and prompt-splitting alignment.

External test files (in `tests/`, exercising the public contract):

- **`parser_tests.rs`** — every verb + alias, direction round-trips, filler
  stripping, the total-function guarantee (no panic on hostile input), and
  verbatim-capture regressions for `friction`/`recall` (case, filler words,
  and word order preserved; internal whitespace still collapsed).
- **`world_tests.rs`** — prefix matching, inventory-targeted lock/unlock/resize,
  missing-target handling, score-rank boundaries, zero-cost resize, and
  darkness-streak recovery when returning to the light.
- **`backend_tests.rs`** — backend selection, mock estate invariants, a full
  winnable playthrough, and credential-free `az` backend construction.
- **`integration_tests.rs`** — end-to-end sessions parsing raw input and
  dispatching commands against a live world.
- **`evolution_tests.rs`** — self-evolution: deriving a brand-new capability with
  no code edit, persistence/recall across sessions, non-failing intent
  resolution, and driving `AzBackend` from a `FakeAzRunner` — all offline.
- **`dungeon_tests.rs`** — Dungeon Crawler Mode: fake-`az` map building,
  read-only command validation, portal-link validation, scrubbed SVG/HTML
  rendering, loopback-only server responses, and popup/resource-detail JSON.
- **`memory_tests.rs`** — `GraphMemory` persistence, recall ranking, and
  cross-session accumulation via the public API.
- **update_*.rs** (`update_startup_tests.rs`, `update_stamp_tests.rs`,
  `update_archive_tests.rs`, `update_pure_tests.rs`,
  `update_resolve_tests.rs`, `update_checksum_tests.rs`) — the self-update
  mechanism: startup gating, version stamps, archive extraction, checksum
  verification, and release-asset resolution, all against fixtures/fakes (no
  network).

No test invokes the real `az` CLI (everything goes through `FakeAzRunner`) or the
`az` backend against a live subscription, so the suite runs with zero
credentials. Similarly, no test in `tests/` or `src/` invokes the live
`azork-oit` binary against a real subscription — its guardrails and friction
heuristics are exercised entirely through the pure `oit` module's unit tests.

### Agentic tests (run by default `cargo test` at the repo root)

- `src/agent_engine/mod.rs` unit tests — `AzorkAdapter` intent resolution
  against a `CapabilityRegistry`, and `run_intent_recipe` executing
  `INTENT_RESOLUTION_RECIPE` end-to-end via the `recipe-runner-rs` git
  dependency (see [`recipe-runner-rs`]) — no sibling checkout required. Intent
  resolution itself is deterministic and network-free at runtime.

### Companion-crate tests (opt-in, not run by `cargo test` at the repo root)

- `(cd memory-store && cargo test)` — `PersistentStore::save`/`load`/`recall`
  round-tripping a `GraphMemory` (nodes and edges) through a real, temporary
  SQLite-backed `amplihack-memory` store (requires `amplihack-memory-lib`
  checked out as a sibling; see [`memory-store/README.md`](../memory-store/README.md)).

### QA / outside-in product testing

Full outside-in product testing of the new user-facing surfaces (runtime
`az`-capability derivation, the `azork-oit` binary, and the `azext_azork` CLI
extension) with the project's `gadugi-test` harness is currently **blocked**:
`gadugi-test` is not installed in this environment. Until it is available, the
interim QA evidence for these surfaces is:

- `tests/evolution_tests.rs` — capability derivation/learning/persistence and
  intent resolution, driven end-to-end against a `FakeAzRunner`.
- `tests/memory_tests.rs` — graph-memory recall/persistence behaviour exercised
  by the same paths the game and `azork-oit` use.
- `tests/integration_tests.rs` and `tests/parser_tests.rs` — full session
  workflows and command parsing that the OIT agent's use-case catalog and the
  `az azork run` extension command both rely on.
- `azork-oit --dry-run` itself, run against the mock backend, as a manual
  outside-in smoke test of the OIT agent's own catalog before any live run.

**Action required before closing the QA phase:** this interim evidence is a
substitute, not a replacement, for a `gadugi-test` run. The parent
orchestration must either (a) install `gadugi-test` and execute it against
these three surfaces, or (b) explicitly and formally accept the interim
evidence above as sufficient. Do not treat this section as closing the QA
phase on its own.

```bash
cargo build      # compiles cleanly, no warnings
cargo test --all # 274 tests, all passing
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```
