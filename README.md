# AzZork 🧙‍♂️☁️

**The Azure control plane, reimagined as a Zork-style text adventure.**

AzZork turns cloud governance into a dungeon crawl. Your Azure subscription is
the dungeon; **resource groups are rooms**, **resources are objects and
creatures**, **RBAC gates the deeper doors**, and the classic Zork **Grue**
lurks wherever you forget to turn on the lights — that is, in any *unmonitored*
resource group. Governance hazards (public endpoints, unencrypted data, runaway
cost, unlocked resources, dark rooms) are what breed Grues. Harden the estate,
banish the Grues, and raise your governance score.

> It is pitch black. You are likely to be eaten by a Grue.

## The metaphor

| Adventure concept        | Azure concept                                   |
| ------------------------ | ----------------------------------------------- |
| Room                     | Resource group (pinned to a region)             |
| Object / creature        | Resource (VM, storage account, key vault, ...)  |
| Exits (n/s/e/w/u/d)      | Navigation across resource groups / regions     |
| Dark room                | Resource group with **no monitoring**           |
| **Grue**                 | Danger: cost overrun, public/unencrypted, unmonitored |
| `look` / `examine`       | `az resource list` / `az resource show`         |
| `take` / `drop`          | Acquire / delete a resource (with confirmation) |
| `lock` / `unlock`        | Add / remove a management lock (+ private + encrypted) |
| `resize`                 | Right-size a resource to cut runaway cost       |
| `monitor`                | Enable diagnostics / Azure Monitor (banish Grue)|
| `cast deploy`            | `az deployment group create` (bicep/ARM, mock)  |
| `score`                  | Governance posture (0–100)                       |

## Verbs (mapped to `az` operations)

```
look / l                describe the current resource group (list resources)
examine <name> / x      inspect a resource (az resource show)
go <dir> | <dir>        navigate: north south east west up down (n/s/e/w/u/d)
take <name>             acquire a resource into inventory (with confirmation)
drop <name>             delete a resource (destructive, with confirmation)
lock <name>             secure a resource: lock + private + encrypted
unlock <name>           remove a management lock (so it can change/delete)
resize <name>           right-size a resource to cut runaway monthly cost
monitor / light         enable monitoring here (banish the Grue)
cast deploy [template]  cast a deployment spell (bicep/ARM, mock)
inventory / i           list resources you are carrying
score                   report your governance posture (0-100)
learn <group>           introspect 'az <group> --help' and grow AzZork at runtime
capabilities / caps     list the az capabilities AzZork has learned so far
help / ?                show this help
quit / q                leave the dungeon
```

## Self-evolution 🌱

AzZork does **not** ship a frozen, hand-maintained table of `az` commands.
Instead it *derives* its vocabulary from the real CLI and grows as you play:

- **`learn <group>`** runs `az <group> --help`, parses the command list, and folds
  every discovered command into AzZork's [`CapabilityRegistry`] as a new verb.
  No code edit is needed for AzZork to understand a new `az` command — it is
  learned, not compiled in.
- **Persistence.** Learned capabilities are cached (default
  `~/.local/share/azork/capabilities.tsv`, override with `AZORK_CACHE_DIR`) and
  **recalled on the next launch**, so AzZork accumulates knowledge across
  sessions.
- **Adaptive help.** `help` and `capabilities` surface everything learned so far,
  grouped by `az` command group.
- **Intent resolution, never a dead end.** Input that matches no built-in verb is
  routed through an agentic [`IntentResolver`]. Its default, fully-offline
  `MockAdapter` ranks your words against learned capabilities and answers with a
  confident match or a "did you mean…" list — AzZork *tries to figure out what
  you meant* rather than failing. The `Adapter` trait is the seam where a richer,
  live agentic resolver (recipe-runner style) can be slotted in.

All CLI access flows through a single `AzRunner` seam, so the entire
self-evolution machinery is exercised offline in tests with canned `az` output —
`cargo test` never calls the real `az` binary or the network.

[`CapabilityRegistry`]: src/capabilities/registry.rs
[`IntentResolver`]: src/agent/mod.rs

## Install

Requires a Rust toolchain (`cargo`). Then:

```bash
git clone https://github.com/rysweet/azork.git
cd azork
cargo build --release
# binary at target/release/azork
```

Run it directly during development:

```bash
cargo run
```

## Usage

### Offline mock backend (default — no Azure credentials needed)

```bash
azork
# or
cargo run
```

This loads a small synthetic Azure estate (subscriptions, resource groups and
resources) so the game runs anywhere with **zero credentials and no network**.

### Real backend (optional — shells out to the `az` CLI)

Explore your *actual* subscription. Requires the [Azure CLI](https://learn.microsoft.com/cli/azure/)
installed and logged in (`az login`):

```bash
azork --backend az
# or
AZORK_BACKEND=az azork
```

The real backend maps your live resource groups into rooms and their resources
into objects by shelling out to `az group list` / `az resource list`. It never
runs by default and is never exercised by the test suite.

> ⚠️ The real backend performs **read-only** discovery. Destructive verbs in the
> game (`drop`) operate on the in-memory world model only — AzZork does not
> delete real Azure resources.

## Example session

```
    ___    ______           __
   /   |  ____/ / __ \_____/ /__
  / /| | /_  / / / / / ___/ //_/
 / ___ |/ /_/ / /_/ / /  / ,<
/_/  |_|\____/\____/_/  /_/|_|

AzZork — an Azure Control-Plane Adventure
=========================================
[backend: mock (offline) | subscription: Contoso-Dev (mock)]

== landing-rg (eastus) ==
The West Landing Zone. Cables snake overhead and a subscription portal hums softly.
You see:
  - portal (Microsoft.Portal/dashboards)
Exits: down, east, north

az> north
== web-rg (eastus) ==
The Public Web Tier. Wind howls through open ports.
You see:
  - appservice (Microsoft.Web/sites)
  - webstore (Microsoft.Storage/storageAccounts)
Exits: north, south

az> examine webstore
webstore [Microsoft.Storage/storageAccounts]
A storage account with its container door flung wide open.
Status: PUBLIC | UNENCRYPTED | unlocked | ~$60/mo
A Grue senses it is exposed to the public internet, storing its data unencrypted, ...

az> lock webstore
You ward the webstore with a management lock, private endpoints, and encryption. A Grue recoils.

az> north
== unmon-rg (centralus) ==
It is pitch black here — no monitoring, no diagnostics. You are likely to be eaten by a Grue.
Exits: south

>> It is dark. You hear the slavering fangs of a Grue nearby. Enable monitoring (type 'monitor') before it strikes!

az> monitor
You enable diagnostic settings and Azure Monitor. Light floods the room; the lurking Grue shrieks and flees.

az> score
Governance posture: 50/100  —  rank: Apprentice Admin
Outstanding hazards: 10 (public/unencrypted/unlocked resources, cost overruns, unmonitored rooms)
Moves taken: 4
```

### Getting eaten by a Grue

Linger in a dark (unmonitored) room and act turn after turn without enabling
monitoring, and the Grue will eventually strike:

```
az> look

>> It is dark. You hear the slavering fangs of a Grue nearby. ...
az> look

>> Oh no! You have walked too long in the dark. A GRUE lunges from the shadows and DEVOURS you.

*** You have died. ***
```

## Architecture

Idiomatic Rust modules:

```
src/
├── main.rs            binary: REPL, intro banner, backend selection, confirmations, Grue turns
├── lib.rs             library crate root re-exporting parser, world, backend, az_runner, capabilities, agent
├── parser.rs          command parser: verbs, directions, aliases (+ unit tests)
├── world.rs           world model: rooms, resources, hazards, scoring, Grue mechanic (+ unit tests)
├── az_runner.rs       the single seam for invoking `az` (ProcessAzRunner / FakeAzRunner)
├── capabilities/
│   ├── mod.rs         Capability type
│   ├── derive.rs      parse `az --help` / `az <group> --help` into capabilities
│   └── registry.rs    CapabilityRegistry: lookup, suggestions, help text, on-disk cache
├── agent/
│   └── mod.rs         IntentResolver + Adapter trait + offline MockAdapter
└── backend/
    ├── mod.rs         Backend trait + selection
    ├── mock.rs        default offline synthetic estate (+ unit tests)
    └── az.rs          optional live backend, driven through an injected AzRunner

tests/                 external contract & integration tests (drive the public API)
├── parser_tests.rs    parser verb/alias/edge-case contract
├── world_tests.rs     world-model behaviour & edge cases
├── backend_tests.rs   backend selection + mock estate invariants
├── integration_tests.rs  end-to-end typed-session workflows
└── evolution_tests.rs    self-evolution: derive/persist/resolve with a fake `az`
```

The engine is split into a thin `azork` binary and an `azork` library crate.
The `Backend` trait cleanly separates *where the map comes from* (mock vs. live
Azure) from the game engine, so the world model and parser are fully testable
without any Azure dependency. All `az` invocation is funnelled through the
`AzRunner` seam, letting the capability-derivation and intent-resolution paths be
exercised offline with canned CLI output — from both colocated unit tests and the
external `tests/` suite.

### Third-party dependencies

The crate remains **dependency-free** (standard library only), so nothing new
needs a license note. The self-evolution design anticipates optional, feature-
gated integration with the MIT-licensed `amplihack-memory` (ladybug graph memory)
and `amplihack-recipe-runner` (agentic `Adapter`) crates; those would be added
behind a `persistent` feature so the default build stays light and offline. Their
MIT terms are compatible with this project's MIT license.


## Development

```bash
cargo build      # compile
cargo test       # run the unit test suite (parser + world model + backends)
cargo run        # play with the offline mock backend
```

## Documentation

Full documentation lives in [`docs/`](docs/):

- [Usage guide](docs/USAGE.md) — every command, the Grue mechanic, and scoring.
- [Tutorial](docs/TUTORIAL.md) — a guided playthrough from first `look` to Cloud Guardian.
- [Configuration reference](docs/CONFIGURATION.md) — backend selection, the mock world, and the read-only `az` backend.
- [API / module reference](docs/API.md) — internal architecture for contributors.

## License

[MIT](LICENSE) © 2026 rysweet
