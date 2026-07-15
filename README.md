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
learn <group>           manually refresh/relearn 'az <group> --help' (auto-discovered at startup too)
capabilities / caps     list the az capabilities AzZork has learned so far
recall <query>          ranked recall over AzZork's persistent graph memory
friction <note>         record something confusing/missing to improve later
memory / mem            summarise what AzZork remembers (rooms, objects, verbs)
help / ?                show this help
version / ver           show the AzZork version
quit / q                leave the dungeon
```

AzZork does **not** ship a frozen, hand-maintained table of `az` commands.
Instead it *derives* its vocabulary from the real CLI and grows automatically:

- **Automatic startup discovery.** On launch, AzZork enumerates the top-level
  `az` command groups (`az --help`) and learns any that aren't already cached,
  folding every discovered command into AzZork's [`CapabilityRegistry`] — no
  code edit, and no `learn` command, needed for AzZork to understand a new `az`
  group. Discovery runs on a background thread so it never blocks the first
  prompt: it recalls the cache first, skips groups already known (warm start
  stays fast), and streams newly-learned capabilities in between turns as they
  arrive. There's no arbitrary timeout or group cap — discovery is bounded by
  however many groups the real `az` CLI reports, and it stands down as soon as
  you start typing.
- **`learn <group>`** remains available as an explicit manual refresh: it runs
  `az <group> --help`, parses the command list, and folds every discovered
  command into the registry immediately (bypassing the incremental
  auto-discovery cadence) — handy for forcing a re-learn of a specific group.
- **Persistence.** Learned capabilities are cached (default
  `~/.local/share/azork/capabilities.tsv`, override with `AZORK_CACHE_DIR`) and
  **recalled on the next launch**, so AzZork accumulates knowledge across
  sessions regardless of whether it was learned automatically or manually.
- **Adaptive help.** `help` and `capabilities` surface everything learned so far
  (automatically or via `learn`), grouped by `az` command group.
- **Escape hatch.** Set `AZORK_AUTODISCOVER=0` (or `false`/`no`) to disable
  automatic discovery entirely — useful in offline/CI contexts. Even with
  discovery enabled, if the real `az` CLI is missing or unauthenticated,
  startup still succeeds using the cache and built-in verbs; it never crashes
  or blocks on a broken `az`.
- **Intent resolution, never a dead end.** Input that matches no built-in verb is
  routed through an agentic [`IntentResolver`]. Its default, fully-offline
  `MockAdapter` ranks your words against learned capabilities and answers with a
  confident match or a "did you mean…" list — AzZork *tries to figure out what
  you meant* rather than failing. The `Adapter` trait is the seam where a richer,
  live agentic resolver (recipe-runner style) can be slotted in.

All CLI access flows through a single `AzRunner` seam, so the entire
self-evolution machinery — automatic startup discovery included — is exercised
offline in tests with canned `az` output: `cargo test` never calls the real
`az` binary or the network.

[`CapabilityRegistry`]: src/capabilities/registry.rs
[`IntentResolver`]: src/agent/mod.rs

## Graph memory 🧠

AzZork carries a persistent, ladybug-style **graph memory** — a lightweight
cognitive-memory model of typed memory nodes with ranked recall — that
accumulates across sessions:

- **Rooms** (resource groups), **objects** (resources), **verbs** (learned az
  capabilities), **intents** (free-text you typed), and **friction** notes are
  all remembered as typed nodes.
- Memory is saved to `~/.local/share/azork/memory.graph` (override the directory
  with `AZORK_CACHE_DIR`) and **recalled on the next launch** — the banner shows
  `[memory: recalled N remembered nodes ...]`.
- **`recall <query>`** does a ranked recall across everything remembered.
- **`memory`** summarises counts by kind plus recent notes.
- **`friction <note>`** records anything confusing or missing so it can be fixed
  later; unresolved intents are auto-recorded as friction too.

The default memory is a fully in-memory/offline `GraphMemory` store (deterministic,
line-based persistence, zero deps) so `cargo build`/`cargo test` stay light and
green. Durable, SQLite-backed persistence over the native `amplihack-memory`
library is available as an **opt-in companion crate**,
[`memory-store/`](memory-store/): it mirrors the whole graph (nodes **and** edges)
into an `amplihack-memory` store, reloads it faithfully across sessions, and offers
full-text ranked recall through the library's own search engine. Unlike the
`agent_engine` module (below), it is kept out of the azork package so
the default build never links a native dependency — see
[`memory-store/README.md`](memory-store/README.md).

## Agentic intent resolution

The [`src/agent_engine/`](src/agent_engine/) module depends on and drives the
MIT-licensed [`recipe-runner-rs`] engine — it does not embed AzZork into the
runner, it implements the runner's `Adapter` trait (`AzorkAdapter`) so AzZork
can act as the agent the runner calls: *agent* steps resolve intent against
the learned registry (deterministic, offline at runtime), *bash* steps
delegate to the runner's CLI subprocess adapter so a recipe can shell out to
`az`. `run_intent_recipe` hands an inline amplihack recipe to the runner with
AzZork as the agent.

It is part of the **main azork crate**: `recipe-runner-rs` is a normal git
dependency, pinned to a specific upstream commit for reproducibility, so
`cargo build`/`cargo test` at the repo root compile and exercise this
capability **by default** — no separate crate to build and no reference repos
to check out side-by-side.

[`recipe-runner-rs`]: https://github.com/rysweet/amplihack-recipe-runner

## Install

The fastest way to get `azork` is the one-line installer, which downloads a
prebuilt binary from the latest [GitHub Release](https://github.com/rysweet/azork/releases),
verifies its SHA-256 checksum, and installs it to your `PATH` — no Rust
toolchain required:

```bash
curl -fsSL https://raw.githubusercontent.com/rysweet/azork/main/install.sh | sh
```

By default it installs to `~/.local/bin` (falling back to `/usr/local/bin`).
Override with `AZORK_INSTALL_DIR`, pin a version with `AZORK_VERSION` (or
`--version`), or preview the resolved download URL without installing:

```bash
# Pin to a specific release
curl -fsSL https://raw.githubusercontent.com/rysweet/azork/main/install.sh | sh -s -- --version v0.5.0

# Install somewhere else
curl -fsSL https://raw.githubusercontent.com/rysweet/azork/main/install.sh | AZORK_INSTALL_DIR=/usr/local/bin sh

# See what would be downloaded, without installing
curl -fsSL https://raw.githubusercontent.com/rysweet/azork/main/install.sh | sh -s -- --dry-run
```

Supported platforms: Linux (`x86_64`, `aarch64`) and macOS (`x86_64`,
`aarch64`/Apple Silicon). Windows users should download a release asset
manually from the [Releases page](https://github.com/rysweet/azork/releases).

To uninstall, simply remove the binary: `rm $(command -v azork)`.

See the [full Install guide](docs/INSTALL.md) for the checksum verification
model, `AZORK_INSTALL_DIR`/`AZORK_VERSION` details, `--help`/`--print-url`
flags, and a troubleshooting table.

### Alternative: build from source

Requires a Rust toolchain (`cargo`). Install directly from GitHub without
cloning (the crate is not published to crates.io):

```bash
cargo install --git https://github.com/rysweet/azork
```

Or clone and build locally:

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

### Keeping it up to date

AzZork can update itself from GitHub Releases:

```bash
azork update            # download & install the latest release, if newer
azork update --check    # only report whether an update is available
```

It also performs a cheap, cached update check at startup that is fully
opt-out and safe under CI / non-interactive use:

```bash
export AZORK_NO_UPDATE_CHECK=1   # disable the automatic startup check
```

Updates are verified by SHA-256 before install and the check is skipped
automatically under CI, non-TTY, or subprocess invocation, so it never hangs or
prompts in automation. See the [Self-Update guide](docs/UPDATING.md) for the
full trust model, exit codes, and release flow.

## Usage

### Offline mock backend (default — no Azure credentials needed)

```bash
azork
# or
cargo run
```

This loads a small synthetic Azure estate (subscriptions, resource groups and
resources) so the game runs anywhere with **zero credentials and no network**.

Want a bigger synthetic tenant to explore or test the map layout against?
Request a sized, deterministic, offline-generated estate instead:

```bash
AZORK_MOCK_SIZE=large azork          # ~100 synthetic resource groups
AZORK_MOCK_SIZE=200x10 azork         # explicit: 200 RGs, 10 resources each
```

See [Generating a sized mock tenant](docs/DUNGEON-CRAWLER.md#generating-a-sized-mock-tenant)
for the full grammar (presets, explicit counts, seeds) and the `azork crawl
--mock-size` equivalent.

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

On large tenants the live backend is **bounded** so it never fans out into
hundreds of sequential `az resource list` calls:

- `AZORK_MAX_ROOMS` — max resource groups mapped into rooms (default 40).
- `AZORK_MAX_RESOURCE_ROOMS` — max rooms whose resources are enumerated (default 8).

Rooms beyond the cap are still navigable; their contents are lazily summarised.

> ⚠️ The real backend performs **read-only** discovery. Destructive verbs in the
> game (`drop`) operate on the in-memory world model only — AzZork does not
> delete real Azure resources.


## Dungeon Crawler Mode 🗺️

Prefer a map to a REPL? `azork crawl` (alias `azork dungeon`) turns your whole
subscription into a single explorable dungeon map instead of one resource
group at a time: resource groups become **walled rooms** joined by
**corridors and doors**, resources appear inside their room as **Microsoft's
official Azure architecture icons** (bundled offline, never hotlinked), and
shared regions/relationships become the corridors between rooms — drawn on a
parchment-and-grid background so it reads as a dungeon map, not a graph.

```bash
azork crawl --backend az --serve
```

```
🗺  Mapping subscription "Contoso-Prod" ...
    Discovered 14 resource groups, 87 resources.
🕯  Dungeon assembled. Serving map at http://127.0.0.1:53214
```

Open the printed URL and click any room to pop up its contents: each resource
shows its resource-type icon (one of Microsoft's official Azure architecture
icons), a deep link straight to that resource's page in the Azure portal,
and one or more suggested read-only `az` commands to inspect it (display-only
— nothing is ever executed for you).

Here's a real map generated by `azork crawl` against a live subscription (257
rooms, 2,857 resources):

![Dungeon map of an Azure subscription](docs/images/crawl-map-overview.png)
*The full dungeon: every resource group is a walled room, every resource is
drawn with its official Azure architecture icon, and corridors with doors
connect resource groups that share regions or relationships.*

![Zoomed-in view of a section of the dungeon map](docs/images/crawl-map-zoom.png)
*Zoomed in: room labels are the resource-group names, walls and doors mark
room boundaries and corridor entrances, and each resource inside a room shows
Microsoft's official icon for its type (storage account, virtual network,
virtual machine, key vault, Cosmos DB, and more) — see
[`assets/azure-icons/LICENSE-NOTICE.md`](assets/azure-icons/LICENSE-NOTICE.md).*

![Resource detail pop-up in the dungeon map](docs/images/crawl-resource-popup.png)
*Clicking a resource icon pops up its full-size icon, name, type, an "Open in
Azure Portal" link, and a suggested read-only `az` command to inspect it.*

(Maintainers: these screenshots are captured against a real, running
`azork crawl --backend az --serve` session with
[`scripts/capture-real-screenshots.mjs`](scripts/capture-real-screenshots.mjs),
a Playwright-driven, manual, offline-friendly tool that is never run in CI.
`scripts/capture-screenshots.sh` remains available for a quick local
mock-backend smoke test of the renderer.)

It is **strictly read-only** (only `list`/`show`-class `az` calls), uses the
same `AzRunner` seam as the rest of AzZork, validates resource IDs before
building deep links or command suggestions, scrubs secret-shaped text from the
rendered output, and binds its local server to loopback only. Full details:
[docs/DUNGEON-CRAWLER.md](docs/DUNGEON-CRAWLER.md).

## Azure CLI extension (`az azork`) — optional

AzZork also ships as an **Azure CLI extension** so you can play from `az`:

```bash
cd azext && python3 setup.py bdist_wheel
az extension add --source azext/dist/azork-0.2.0-py3-none-any.whl --yes
az azork run --commands "look; score"
az azork play --backend az
```

The extension (`azext_azork`) is a thin Python shim that shells out to the
compiled `azork` binary (found via `AZORK_BIN`, a bundled `bin/azork`, or `PATH`).
See [`azext/README.md`](azext/README.md) for details.

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

## Development

```bash
cargo build      # compile (default: includes the embedded agent_engine module)
cargo test       # run the unit test suite (parser + world model + backends + memory + agent_engine)
cargo run        # play with the offline mock backend
cargo clippy --all-targets   # lints (CI enforces -D warnings)

cargo build --bin azork-oit          # the live outside-in-testing agent
(cd memory-store && cargo test)      # opt-in amplihack-memory durable-memory crate
```

`azork-oit` (`src/bin/azork-oit.rs`) is an internal self-testing tool: it drives
AzZork like a real user against a live subscription to surface friction and
feed fixes back into the project. It is not a player-facing feature — see
[docs/oit-friction-report.md](docs/oit-friction-report.md) for its latest findings.

## Documentation

Full documentation lives in [`docs/`](docs/):

- [Usage guide](docs/USAGE.md) — every command, the Grue mechanic, and scoring.
- [Tutorial](docs/TUTORIAL.md) — a guided playthrough from first `look` to Cloud Guardian.
- [Configuration reference](docs/CONFIGURATION.md) — backend selection, the mock world, and the read-only `az` backend.
- [Auto-Discovery guide](docs/AUTODISCOVERY.md) — how AzZork learns new `az` groups automatically at startup, cache behavior, and the `autodiscover` module API.
- [Self-Update guide](docs/UPDATING.md) — the `azork update` command, the cached startup check, security/trust model, and release flow.
- [Development guide](docs/DEVELOPMENT.md) — pre-commit hooks, CI, and test coverage.
- [API / module reference](docs/API.md) — internal architecture for contributors.
- [Dungeon Crawler Mode](docs/DUNGEON-CRAWLER.md) — the map view: `azork crawl`, icons, the local server, and interactive room pop-ups.
- [Security policy](SECURITY.md) — threat model, guarantees, and how to report vulnerabilities.
- [Security audit](docs/SECURITY-AUDIT.md) — findings, fixes, and verification results.
- [Carl OIT campaign](docs/CARL-OIT-CAMPAIGN.md) — the outside-in, black-box product-testing process run against the built binary.

## License

[MIT](LICENSE) © 2026 rysweet
