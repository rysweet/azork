# AzZork API / Module Reference

This reference documents the internal architecture of AzZork for contributors
and anyone embedding the engine. AzZork is a single binary crate (`azork`) with
**zero external dependencies** — the standard library only.

For player-facing docs see the [Usage guide](USAGE.md) and
[Configuration reference](CONFIGURATION.md).

## Module map

```
src/
├── main.rs            REPL: banner, input loop, dispatch, y/N confirmation
├── parser.rs          Total input parser: text -> Command
├── world.rs           World model: rooms, resources, hazards, Grue, scoring
└── backend/
    ├── mod.rs         Backend trait + select()
    ├── mock.rs        Default offline synthetic world
    └── az.rs          Optional read-only live-Azure world (shells out to `az`)
```

Data flows one way at startup: a `Backend` **builds** a `World`; thereafter the
REPL parses input into `Command`s and applies them to the `World`.

```
input ──parser::parse──▶ Command ──main::handle──▶ World mutation ──▶ text out
                                                     │
Backend::build_world ────────────────────────────────┘ (once, at startup)
```

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
| `Monitor` | Enable monitoring in the current room. |
| `Inventory` | List carried resources. |
| `Score` | Report governance posture. |
| `Cast(String)` | Cast a spell (currently `deploy [template]`). |
| `Help` | Show help. |
| `Quit` | Leave the game. |
| `Empty` | Player entered nothing. |
| `Unknown(String)` | Unrecognized input; carries the original text. |

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

Recognized verb aliases:

| Command | Verbs |
| --- | --- |
| `Look` | `look`, `l` |
| `Examine` | `examine`, `x`, `inspect`, `show` |
| `Go` | `go`, `move`, `walk` (or a bare direction) |
| `Take` | `take`, `get`, `grab`, `acquire` |
| `Drop` | `drop`, `delete`, `release`, `rm` |
| `Lock` | `lock`, `secure` |
| `Monitor` | `monitor`, `light` |
| `Inventory` | `inventory`, `i`, `inv` |
| `Score` | `score` |
| `Cast` | `cast <spell>`, or `deploy [template]` as a convenience alias for `cast deploy` |
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
| `monitor` | `fn monitor(&mut self) -> String` | Enable monitoring in the current room; resets the darkness streak. |
| `inventory` | `fn inventory(&self) -> String` | List carried resources. |
| `total_hazards` | `fn total_hazards(&self) -> u32` | Sum of resource hazards across all rooms and inventory, plus one per dark room. |
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

Builds a world from the live subscription via read-only `az` calls.
`name()` → `"az (live Azure)"`. See
[the `az` backend](CONFIGURATION.md#the-az-backend-live-azure) for the exact
commands and safety guarantees.

## `main` (REPL)

Entry point and orchestration. Responsibilities:

- Resolve the backend id from `--backend` / `-b` / `AZORK_BACKEND` and call
  `backend::select` + `build_world` (exiting with guidance on failure).
- Print the banner, backend/subscription status line, and initial room.
- Run the input loop: `parse` → `handle` → `run_grue_check`.
- Prompt for **y/N** confirmation on `take` and `drop` (default No).
- Implement the `cast deploy [template]` spell as a mock, credential-free
  deployment narration.

## Testing

32 unit tests are colocated with their modules under `#[cfg(test)]`:

- **`parser.rs`** — verb/alias parsing, bare directions, filler stripping,
  multi-word targets, unknown input.
- **`world.rs`** — `look`, `go`, `take`/`drop`, `lock` hazard reduction,
  scoring, and the Grue escalation model (with a seeded RNG).
- **`backend/mock.rs`** — the world builds, starts lit, exposes a reachable dark
  room, and seeds fixable hazards.
- **`main.rs`** — `cast deploy` is mock-safe, unknown spells are rejected, and
  the confirmation helper reads yes / defaults to no on EOF.

No test invokes the `az` backend, so the suite runs with zero credentials.

```bash
cargo build      # compiles cleanly, no warnings
cargo test       # 32 tests, all passing
```
