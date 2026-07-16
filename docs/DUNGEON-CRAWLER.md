# Dungeon Crawler Mode

**Turn your entire Azure subscription into an explorable, hand-drawn dungeon
map you can click through in a browser.**

Where the classic AzZork REPL (see the [Usage guide](USAGE.md)) plays out one
resource group at a time as a text adventure, Dungeon Crawler Mode steps back
and draws the **whole subscription at once** as a real dungeon map: every
resource group is a **walled, rectilinear room** (not a node in a graph),
every resource inside it is rendered with **Microsoft's official Azure
architecture icon** for its resource type, and rooms are joined by
**corridors with doors** where
they share a region or a network relationship. The whole thing is drawn on a
parchment-and-grid background in the style of classic tabletop dungeon maps.
It is read-only, fully offline by default, and safe to point at a real
subscription — it never creates, modifies, or deletes anything.

```
azork crawl --serve
```

```
🗺  Mapping subscription "Contoso-Prod" ...
    Discovered 14 resource groups, 87 resources.
🕯  Dungeon assembled. Serving map at http://127.0.0.1:53214
    (Ctrl-C to stop)
```

Open the printed URL in a browser to get an interactive, dungeon-scrawl-style
map of your subscription. Click any room to see what lives inside it.

---

## Table of contents

- [Quick start](#quick-start)
- [Command reference](#command-reference)
- [The map model](#the-map-model)
- [Dungeon-map rendering](#dungeon-map-rendering)
- [Adaptive room sizing and corridor spacing](#adaptive-room-sizing-and-corridor-spacing)
- [Dungeon decorations](#dungeon-decorations)
- [Why a self-designed renderer (tool evaluation)](#why-a-self-designed-renderer-tool-evaluation)
- [Azure architecture icons](#azure-architecture-icons)
- [The optional Playwright pass](#the-optional-playwright-pass)
- [The local HTTP server](#the-local-http-server)
- [The JSON API](#the-json-api)
- [Interactivity: room pop-ups](#interactivity-room-pop-ups)
- [Portal deep links](#portal-deep-links)
- [Suggested `az` commands](#suggested-az-commands)
- [Safety and guardrails](#safety-and-guardrails)
- [Scaling to large subscriptions](#scaling-to-large-subscriptions)
- [Configuration](#configuration)
- [Troubleshooting](#troubleshooting)

---

## Quick start

```bash
# Draw the map and write it to a file (no server, no browser)
azork crawl --backend az --out dungeon.html

# Draw the map and serve it locally (opens no browser automatically —
# copy/paste the printed URL)
azork crawl --backend az --serve

# Pick a specific port instead of an OS-assigned free one
azork crawl --backend az --serve --port 8420

# Try it fully offline against the built-in mock estate first
azork crawl --serve
```

`crawl` (alias: `dungeon`) is a top-level subcommand of the `azork` binary,
alongside the classic REPL mode — it does not require a separate install:

```
azork crawl [--backend <id>] [--serve] [--port <n>] [--out <path>]
            [--budget <n>] [--playwright] [--mock-size <spec>]
            [--snapshot <path>] [--diff <old> <new>]
```

| Flag | Default | Meaning |
| --- | --- | --- |
| `--backend <id>` | `mock` | Which `az` backend to enumerate (`mock` or `az`, same ids as the REPL — see [Configuration reference](CONFIGURATION.md)). |
| `--serve` | off | Start the embedded HTTP server and serve the map + JSON API instead of (or in addition to) writing a file. |
| `--port <n>` | `0` (OS-assigned free port) | Port for `--serve`. `0` lets the OS pick a free ephemeral port, which is then printed to stdout. |
| `--out <path>` | none | Write the rendered map (self-contained HTML) to a file. Can be combined with `--serve`. |
| `--budget <n>` | `500` | Soft cap on in-memory resources buffered per enumeration window before flushing to the map graph; tune only if you are constrained on memory. Does **not** limit how much of the subscription is mapped — enumeration always continues to completion or cancellation, just in bounded-size batches. |
| `--playwright` | off | Best-effort, local-only headless-browser post-processing of the native render (e.g. a rasterized snapshot). Never drives an external website; silently no-ops back to the plain native renderer if browsers aren't installed locally — see [below](#the-optional-playwright-pass). |
| `--mock-size <spec>` | none (small fixed demo estate) | `mock` backend only: synthesize a larger, deterministic estate instead of the small fixed demo. See [Generating a sized mock tenant](#generating-a-sized-mock-tenant). |
| `--snapshot <path>` | none | Write the assembled map as JSON to `<path>`, for later `--diff`ing. Composes with `--out`/`--serve`. Refused (no file written, exit 1) if the map is partial (cancelled mid-enumeration). |
| `--diff <old> <new>` | none | Compare two previously-written `--snapshot` JSON files and print a "Time Rift" report of rooms/resources added, removed, and changed, then exit. Takes priority over every other flag — no map is built and no backend is contacted. |

Press `Ctrl-C` to stop the server; enumeration itself can also be cancelled
mid-flight (`Ctrl-C` during the "Mapping subscription..." phase) and will still
serve whatever partial map has been assembled so far, clearly marked as
partial.

### Comparing snapshots over time ("Time Rift")

Take a snapshot now, make some infrastructure changes, take another snapshot
later, then diff them:

```
azork crawl --snapshot before.json
# ... time passes, infrastructure changes ...
azork crawl --snapshot after.json
azork crawl --diff before.json after.json
```

`--diff` never contacts a backend — it only reads the two JSON files, so it
works fully offline regardless of which backend produced the snapshots.

**Snapshot format.** `--snapshot <path>` writes the exact in-memory
`DungeonMap` graph (the same model described in
[The map model](#the-map-model)) as pretty-printed JSON via `serde_json`:
subscription id, `rooms[]` (each with its resources), and `edges[]`. It is a
plain serialization of existing types — no separate snapshot schema to keep
in sync. A snapshot is refused, with a clear error and a non-zero exit code,
if the assembled map is `partial` (i.e. enumeration was cancelled mid-flight
with Ctrl-C), so a partial view of the estate can never be mistaken for a
complete one later when diffed:

```
$ azork crawl --snapshot partial.json
🗺  Mapping subscription "Contoso-Prod" ...
^C
🕯  Dungeon assembled (partial — enumeration was interrupted).
error: refusing to write snapshot: map is partial (enumeration was cancelled).
       Re-run without interrupting to capture a complete snapshot.
```

**Matching rules.** Rooms (resource groups) are matched between the two
snapshots by their id/name. Resources are matched by their **full ARM
resource id**, never by display name alone, so a rename is correctly reported
as unchanged (same id) and a same-named resource re-created in a different
resource group is correctly reported as removed + added (different ids). A
resource whose id is unchanged but whose `kind` or `region` differs between
the two snapshots is reported as **changed**. Resource matching is performed
across the whole map (not scoped per-room), so a resource moved from one
resource group to another between snapshots is reported as changed rather
than a false add/remove pair; the room-level move itself shows up as the
resource group changing (rooms added/removed) if the group's very existence
changed too.

**Report format.** `--diff` prints a themed, fully deterministic text report
— no timestamps, no non-deterministic ordering — grouped into sections that
are only printed when non-empty, followed by a one-line summary count of net
additions (`+`), removals (`-`), and in-place changes (`~`):

```
$ azork crawl --diff before.json after.json
⚡ Time Rift Report

Rooms added:
  + rg-newteam (rg-newteam)

Rooms removed:
  - rg-decommissioned (rg-decommissioned)

Resources added:
  + /subscriptions/.../rg-newteam/providers/Microsoft.Compute/virtualMachines/vm3 (Microsoft.Compute/vm)

Resources removed:
  - /subscriptions/.../rg-a/providers/Microsoft.Storage/storageAccounts/oldsa (Microsoft.Storage/storageAccount)

Resources changed:
  ~ /subscriptions/.../rg-a/providers/Microsoft.Compute/virtualMachines/vm1 (Microsoft.Compute/vm/eastus -> Microsoft.Compute/vm/westus)

Summary: +2 -2 ~1
```

When the two snapshots are structurally identical, the report is simply:

```
⚡ Time Rift Report
No changes detected — the dungeon is unchanged across time.
Summary: +0 -0 ~0
```

Every list (rooms added/removed, resources added/removed/changed) is sorted
by id before printing, so the report is byte-for-byte identical across runs
regardless of the original enumeration order in either snapshot.

**Exit codes.** `azork crawl --diff` exits `0` after printing the report,
whether or not differences were found — a diff with changes is not an error.
It exits non-zero with a clear error message (never a panic or stack trace)
if either file is missing, unreadable, or is not valid `DungeonMap` JSON
(e.g. hand-edited or truncated). `azork crawl --snapshot` exits `0` after
writing the file, and non-zero if the map was partial or the path could not
be written (e.g. a bad directory).

**Composability.** `--snapshot` runs the normal map-assembly path and can be
combined with `--out` and/or `--serve` in the same invocation (write HTML,
write a JSON snapshot, and/or serve — all from one enumeration pass).
`--diff`, by contrast, needs no enumeration at all: if `--diff` is present it
takes priority over every other flag (`--backend`, `--serve`, `--out`,
`--playwright`, `--mock-size`, `--snapshot`) — no backend is contacted and no
map is built, it only reads the two files named.

## Command reference

Dungeon Crawler Mode reuses the exact same backend selection rules as the REPL
(`--backend`, `-b`, `AZORK_BACKEND`), so anything you already know from
[Configuration](CONFIGURATION.md#backend-selection) applies unchanged. The
`mock` backend gives you a small, deterministic five-room dungeon to try the
renderer and server against without any Azure credentials.

```bash
azork crawl                       # mock estate, write nothing, just summarize
azork crawl --serve               # mock estate, serve on an OS-assigned port
azork crawl -b az --serve         # your real subscription, read-only, served
azork crawl -b az --out map.html  # your real subscription, saved to a file
```

## Generating a sized mock tenant

The default `mock` backend (both the interactive game and `azork crawl`)
serves a small, hand-authored, fixed estate so existing behavior never
changes. For iterating on the map layout itself (room sizing, corridor
spacing, decorations) it's often useful to try a much bigger synthetic
tenant instead — offline, fast, and fully reproducible.

Request a sized synthetic estate with `--mock-size` (on `azork crawl`) or the
`AZORK_MOCK_SIZE` environment variable (works for `azork crawl` **and** the
interactive REPL's `mock` backend):

```bash
# Named presets: small (5 RGs), medium (25), large (100), huge (500);
# resources-per-group scales with the preset too.
azork crawl --mock-size large --serve

# Or via the environment (also affects `azork` in interactive mode):
AZORK_MOCK_SIZE=huge azork crawl --serve
AZORK_MOCK_SIZE=large azork   # interactive REPL against a synthetic 100-RG estate

# Explicit counts: COUNTxPER_GROUP, e.g. 300 resource groups x 12 resources each
azork crawl --mock-size 300x12 --out big-map.html

# Bare resource-group count: resources-per-group falls back to the
# medium preset's value (5)
azork crawl --mock-size 200 --out big-map.html

# Override the seed for a different (but still reproducible) variant:
azork crawl --mock-size large:7 --serve
```

Env var equivalents, all optional and combinable:

| Variable | Meaning |
| --- | --- |
| `AZORK_MOCK_SIZE` | Preset name (`small`/`medium`/`med`/`large`/`huge`), bare resource-group count (resources-per-group falls back to the medium preset's value, 5), or `COUNTxPER_GROUP`, same grammar as `--mock-size`. |
| `AZORK_MOCK_RGS` | Explicit resource-group count; overrides the count implied by `AZORK_MOCK_SIZE`. |
| `AZORK_MOCK_RESOURCES_PER_RG` | Explicit resources-per-group count; overrides the value implied by `AZORK_MOCK_SIZE`. |
| `AZORK_MOCK_SEED` | Deterministic PRNG seed override. |

Generation is fully offline (no network, no `az`) and deterministic: the same
size + seed always produces byte-for-byte identical rooms, resources, and
corridors, so layout/snapshot tests and screenshots stay stable across runs.
There is no hard cap on size — generation is a straightforward, streaming
build bounded only by the counts you ask for. Generated resources are drawn
from the same Azure types the map's icon set already knows (storage
accounts, VMs, vnets, web apps, key vaults, AKS, SQL, Cosmos DB, NICs, NSGs,
public IPs, load balancers), with realistic-looking names and regions, and
resource groups are laid out on a grid so every room is reachable from the
start room (no disconnected islands).

## The map model

Enumeration walks the subscription through the existing `AzRunner` seam (the
same one the REPL's `az` backend uses — see
[`src/az_runner.rs`](../src/az_runner.rs)) and assembles a serializable graph:

- **Rooms** — one per resource group, tagged with its region/location.
- **Resource nodes** — one per resource, attached to its owning room, carrying
  its Azure resource type, name, resource ID, and region.
- **Edges (corridors)** — connect rooms that share a region, and separately
  connect rooms that have an observed network relationship (e.g. a VNet
  peering, a private endpoint, or a resource group referenced by another
  resource's dependencies), when that information is available from the `az`
  output being parsed.
- **Positions** — each room is assigned a deterministic (x, y) grid position
  computed from a stable hash of its name and region, so the same subscription
  always lays out the same way between runs (no random jitter, no external
  layout engine required for the default render). Positions are also biased
  by a coarse, built-in table of common Azure regions' real-world
  longitude/latitude, so the overall dungeon roughly mirrors real geography
  — westerly regions draw west of easterly ones, northerly regions draw
  north of southerly ones — while resources within a single region keep
  their existing hash-based scatter. Regions not in the table simply get no
  bias, falling back to the prior hash-only placement. (Implementation
  detail: longitude/latitude are bucketed into coarse 20°-by-20° cells
  before being applied as bias, so the effect is a rough geographic
  grouping rather than precise placement.)

The graph is the single source of truth handed to both the native renderer and
the HTTP server's JSON API — the picture and the API are two views onto the
same model, never two separate sources of truth.

Enumeration is **strictly read-only**: it only ever issues `list`/`show`-class
`az` invocations (an explicit allow-list of read verbs), never anything that
creates, updates, or deletes a resource, group, or subscription-level setting.

## Dungeon-map rendering

The renderer is **native, offline, and deterministic**, and it draws an
actual dungeon rather than a node-link diagram:

- Where licensing allows, the registry prefers the official Azure architecture
  icon set (SVG). Where it doesn't (offline run, no network, or the bundled
  set doesn't cover a type), it falls back to a curated, redistributable
  SVG/emoji icon so the map **always renders fully offline** — no icon is ever
  silently skipped.
- Unknown/unrecognized resource types get a sensible default "mystery chest" 📦
  icon rather than failing or omitting the resource from the map, so an
  unexpected or newly-released resource type never breaks the crawl.
- The registry is a simple, inspectable table (type prefix → icon → suggested
  `az` command), so adding or overriding an icon (or its suggested command)
  for a type is a one-line change — see [Suggested `az`
  commands](#suggested-az-commands) for how the two stay in sync.
- **Rooms are walled rectilinear chambers.** Each resource group is drawn as
  a rectangle with a visible double-line wall, **adaptively sized to fit the
  number of resources it contains** (more resources → a taller/wider room —
  see [Adaptive room sizing and corridor spacing](#adaptive-room-sizing-and-corridor-spacing)
  for the exact rule), not a fixed-size circle or box.
- **Corridors are orthogonal (L-shaped) hallways, not straight edges.**
  Where two rooms share a region or an observed network relationship, the
  renderer draws a right-angled corridor between the nearest wall segments of
  the two rooms, with a **door glyph** (a short perpendicular tick across the
  wall) at each end, matching the "wall + door + corridor" vocabulary used by
  tools like Dungeon Scrawl rather than a plain connecting line. Rooms sit on
  a generously spaced grid (see [below](#adaptive-room-sizing-and-corridor-spacing))
  so corridors always have clear room to route without clipping a wall.
- **Parchment/grid background.** The SVG canvas is filled with a subtle
  square-grid pattern over an off-white/parchment tone, evoking a graph-paper
  dungeon map rather than a plain white or dark UI background.
- **Resources are drawn inside their room** as small icon tiles (see [Azure
  architecture icons](#azure-architecture-icons) below), arranged in a simple
  near-square in-room grid so a room with many resources doesn't overlap its
  own walls — the room is sized to the grid, not the other way around.
- **Layout stays a pure function of the map graph.** Room position, size,
  corridor routing, and door placement are all derived deterministically from
  room/resource counts and the stable per-room grid position described in
  [The map model](#the-map-model) — the same subscription always produces
  pixel-identical output, with no random jitter and no external layout
  engine.
- **Purely decorative border/torch/chest/dragon dressing.** A fixed outer
  margin surrounds the room/corridor grid; a decorative border frame, evenly
  spaced torch glyphs, a treasure chest, and a dragon glyph are drawn inside
  that margin band only — see [Dungeon decorations](#dungeon-decorations).
  Because rooms and corridors never enter the margin, this dressing can never
  overlap or collide with the map's interactive content, and (like everything
  else) it's placed deterministically from the map's overall dimensions —
  never randomly.

The output is a single self-contained HTML document: inline SVG for the
dungeon geometry, icons, and decorations, plus a small amount of vanilla JS
for the click-to-popup interaction — no build step and no CDN fetch, so it
opens and looks correct with no network access at all. This is the document
produced by both `--out` (write to a file) and `--serve` (serve over HTTP);
the two share one renderer, so the file you save and the page you're served
are always the same map.

## Adaptive room sizing and corridor spacing

A room's footprint on the map is a **pure function of how many resources it
contains** — there is no fixed cap on room size and no scenario where a
resource group's icons can overflow, clip, or overlap its own walls, no
matter how large the resource group is.

- **At least one grid cell per resource.** Each room lays its resources out
  in a near-square icon grid: `cols = ceil(sqrt(n))`, `rows = ceil(n / cols)`,
  where `n` is the resource count (a room with zero resources is treated as
  `n = 1` so it still reads as a proper chamber). This guarantees the grid
  always has `cols * rows >= n` cells — room enough for every resource with
  no overlap.
- **Icons never shrink.** Every resource icon renders at the same fixed,
  legible size regardless of how many resources share a room; the *room*
  grows to fit the grid, the grid never shrinks to fit the room. This keeps
  icons readable and their click targets a consistent, predictable size on
  every map.
- **A room's pixel size is wall + padding + the icon grid.** Width and height
  are computed from the grid's content size plus fixed wall thickness, inner
  padding, and a label header reserved at the top of the room — plus a small
  minimum footprint so a room with very few resources still reads as a real
  chamber rather than a cramped sliver.
- **Example.** A resource group with 4 resources lays out as a 2×2 grid
  (`cols = ceil(sqrt(4)) = 2`, `rows = ceil(4/2) = 2`); one with 50 resources
  lays out as an 8×7 grid (`cols = ceil(sqrt(50)) = 8`, `rows = ceil(50/8) =
  7`, i.e. 56 cells for 50 resources); one with 200 resources lays out as a
  15×14 grid. In every case, the room's width and height grow to exactly
  contain that grid — there is no arbitrary resource-count ceiling above
  which the map stops scaling or resources start overlapping.
- **Wider inter-room spacing scales with the biggest room on the map.** All
  rooms sit on one shared, uniform grid whose cell size is derived from the
  **single largest room's** footprint (across the whole map) plus a fixed,
  generous gap. That means every room — even small ones — gets enough
  breathing room around it for an L-shaped corridor to route cleanly, and a
  map containing one very large resource group automatically widens spacing
  for *every* room on the map, so large rooms and their corridors never
  collide with a smaller neighbor's walls.
- **Corridors always target each room's real wall.** Corridor endpoints are
  computed from each room's own, individually-adaptive width and height
  (never a shared constant), so a corridor between a small room and a huge
  one still meets each room's actual wall exactly, with no gap and no
  overlap.
- **Fully deterministic.** Room sizing and grid spacing depend only on
  resource counts and the stable per-room grid position from [the map
  model](#the-map-model) — never on wall-clock time, randomness, or run
  order — so re-rendering the same subscription always produces the same
  room sizes, the same spacing, and the same corridor paths.

## Dungeon decorations

The map's overall canvas reserves a **fixed outer margin band** around the
room/corridor grid purely for decorative dressing. Because rooms and
corridors are always drawn at or inside that margin's inner edge, decorations
placed within the margin can never overlap a room, a corridor, a resource
icon, or the click target for any of them — there's no collision detection
needed because the two regions (interior grid vs. outer margin) never
intersect by construction.

Decorations are:

- **A decorative border frame.** A stone-wall-style double-line rectangle
  drawn just inside the canvas edge, visually distinct from each room's own
  wall stroke, framing the whole map.
- **Torches along the border.** Small torch glyphs (a post plus a stylized
  flame) spaced evenly along the top and bottom margin bands.
- **A treasure chest and a dragon**, placed once each in corners of the
  margin band that are guaranteed empty — the chest reuses the same bundled
  "mystery chest" icon artwork used elsewhere on the map, and the dragon is a
  small, original, geometric line-art glyph (not an Azure icon) bundled as an
  inline SVG asset and embedded the same way as every other icon:
  compiled into the binary via `include_str!`, no network fetch, no CDN.
- **Non-interactive by construction.** Every decoration element carries
  `pointer-events: none` and none of them use the `.resource` class, so a
  decoration can never intercept a click meant for a resource icon or
  spuriously trigger the detail popup.
- **Deterministic, not random.** Decoration placement is a pure function of
  the map's overall pixel dimensions (which are themselves a deterministic
  function of the map graph) — never a random seed or per-run jitter — so
  re-rendering the same subscription places every decoration in exactly the
  same spot every time, keeping test output and diffs stable.

## Why a self-designed renderer (tool evaluation)

Before writing a from-scratch dungeon renderer, the three most obvious
"draw me a dungeon map" tools were evaluated for **programmatic, offline,
CI-safe** use — i.e. could `crawl` drive one of them headlessly to lay out
the map, instead of drawing it itself:

| Tool | Headless/Playwright reachable? | Documented import/export format for automation? | License/ToS for automated bulk use | Deterministic & usable with no network (incl. in CI)? | Verdict |
| --- | --- | --- | --- | --- | --- |
| [Dungeon Scrawl](https://app.dungeonscrawl.com/) | Client-side canvas/WebGL app with no documented headless or scripting API | No stable, versioned public import/export schema for generating maps programmatically (only interactive save/export of hand-drawn maps) | Personal map-making tool; terms don't address automated/bulk generation | No — requires reaching the live site over the network at render time | **No-go** |
| [Mystic Waffle Maps](https://www.mysticwaffle.com/maps) | Same class of interactive web canvas editor, no headless/API mode documented | No public machine-readable import/export contract | Same gap — no automated-use terms published | No — network + live site required | **No-go** |
| [Dungeon Map Builder](https://dungeonmapbuilder.com/DungeonMapBuilder/) | Interactive browser tool, no headless/automation entry point documented | No stable export format documented for round-tripping generated data | Same gap | No — network + live site required | **No-go** |

All three are excellent tools for a *person* hand-drawing a dungeon in a
browser, but none publishes a stable, versioned, offline-usable API or export
contract for **automated, unattended, per-crawl map generation** — and none
publishes terms permitting automated/bulk use. Driving any of them would mean
every `azork crawl` (and every test) reaching a third-party website over the
network, which directly conflicts with the "offline, deterministic, no
network in CI" requirement and would make the map's appearance dependent on
a service AzZork doesn't control.

The decision: **build a small, native, self-designed renderer styled after
the visual language those tools popularized** — walled rectilinear rooms,
hatched/orthogonal corridors, door ticks, parchment-and-grid background —
without depending on any of the three at runtime. This keeps `crawl` fully
offline and deterministic while still delivering a map that reads as a
dungeon rather than a graph.

## Azure architecture icons

Every resource node is drawn using one of Microsoft's **official Azure
Architecture Icons** ("Azure Public Service Icons" set), looked up from its
Azure resource type (e.g. `Microsoft.Storage/storageAccounts`,
`Microsoft.Compute/virtualMachines`, `Microsoft.Network/virtualNetworks`,
`Microsoft.Web/sites`, `Microsoft.KeyVault/vaults`,
`Microsoft.ContainerService/managedClusters`, `Microsoft.Sql/servers`,
`Microsoft.DocumentDB/databaseAccounts`, and more) via the same type → icon
registry that also drives [suggested `az` commands](#suggested-az-commands).

- **Icons are Microsoft's official Azure Architecture Icons**, downloaded
  from [learn.microsoft.com/azure/architecture/icons](https://learn.microsoft.com/en-us/azure/architecture/icons/)
  and used unmodified — not cropped, flipped, rotated, distorted, or
  recolored — to label each resource's type on the dungeon map, consistent
  with Microsoft's published icon guidelines for architecture diagrams.
- **Icons are bundled in the repo, not hotlinked.** The SVGs are embedded
  directly into the crate at compile time via `include_str!` (see
  [`src/dungeon/icon_assets.rs`](../src/dungeon/icon_assets.rs)). The
  rendered map never fetches an icon from a CDN or any third-party site at
  run time, so a saved `--out` file or a `--serve` session works identically
  with no network at all.
- **Attribution and terms** are recorded in
  [`assets/azure-icons/LICENSE-NOTICE.md`](../assets/azure-icons/LICENSE-NOTICE.md):
  the icon files remain Microsoft's property, AzZork is not affiliated with
  or endorsed by Microsoft, and the icons must not be used to represent
  non-Microsoft products.
- **Unknown/unrecognized resource types** get Microsoft's generic "All
  Resources" icon (bundled as `mystery-chest.svg`) rather than failing or
  omitting the resource from the map, so an unexpected or newly-released
  resource type never breaks the crawl.
- The registry is a simple, inspectable table (type prefix → icon key →
  suggested `az` command) in [`src/dungeon/type_table.rs`](../src/dungeon/type_table.rs),
  so adding or overriding an icon (or its suggested command) for a type is a
  one-line change plus dropping in the corresponding official SVG file — see
  [Suggested `az` commands](#suggested-az-commands) for how the two stay in
  sync.
- Icons appear in two places, both driven by the same lookup: as a small tile
  inside each room on the map, and, larger, next to each resource's entry in
  the room pop-up.

## The optional Playwright pass

`--playwright` remains available as a **best-effort, fully optional**
secondary pass, but — matching the tool evaluation above — it never depends
on or drives an external dungeon-drawing website. Instead it's reserved for
future local, headless-browser-only post-processing of the *native* render
(e.g. producing a rasterized screenshot alongside the HTML) without ever
requiring network access to a third party.

- It lives in its own module ([`src/dungeon/playwright.rs`](../src/dungeon/playwright.rs))
  and is never compiled into, required by, or exercised by the default
  build, `cargo test`, or CI.
- It requires a separate one-time local setup step (installing Node.js/
  Playwright browsers) documented inline in that module — it is not a Cargo
  dependency of the `azork` crate.
- If the flag is passed but Playwright/browsers aren't installed locally, or
  anything about the pass fails, Dungeon Crawler Mode **prints a one-line
  warning and continues serving/writing the native dungeon-style render**
  unchanged. `--playwright` can never turn a working native render into a
  hard failure, and it never causes `crawl` to reach the network.
- No Azure data ever leaves the local machine because of this flag: it only
  ever operates on the already-rendered local HTML, never on live map data
  sent to a remote site.

## The local HTTP server

`--serve` starts a small embedded HTTP server, implemented directly on
`std::net::TcpListener` (no `axum`/`hyper`/web-framework dependency — this
keeps Dungeon Crawler Mode's dependency footprint minimal and consistent with
the rest of the project), bound to `127.0.0.1` only (never `0.0.0.0` — it is
never exposed on your network), that serves:

- `GET /` — the rendered dungeon map (HTML + inline SVG + JS).
- `GET /api/v1/rooms` — JSON list of rooms (id, name, region, position).
- `GET /api/v1/rooms/<id>` — JSON detail for one room: its full resource list.
- `GET /api/v1/resources/<id>` — JSON detail for a single resource: icon key,
  type, name, region, portal link, and suggested `az` commands.

The server picks a free port automatically when `--port` is `0` (the default),
so multiple `crawl --serve` sessions (or other local services) never collide.
The chosen port is always printed to stdout so it's easy to script against
(`azork crawl --serve --port 0 | grep -oE ':[0-9]+'`).

There is no authentication and no TLS — this is a local developer convenience
server, not something to expose beyond `localhost`. It has no write endpoints:
every route is read-only, mirroring the read-only guarantee of enumeration
itself.

## The JSON API

The JSON API is versioned under `/api/v1` so the client-side map JS and any
external tooling can evolve independently of the map HTML. Example:

```bash
curl http://127.0.0.1:53214/api/v1/rooms
```

```json
[
  { "id": "rg-web", "name": "web-rg", "region": "eastus", "x": 2, "y": 0 },
  { "id": "rg-data", "name": "data-rg", "region": "westus2", "x": 4, "y": 0 }
]
```

```bash
curl http://127.0.0.1:53214/api/v1/rooms/rg-web
```

```json
{
  "id": "rg-web",
  "name": "web-rg",
  "region": "eastus",
  "resources": [
    {
      "id": "/subscriptions/.../resourceGroups/web-rg/providers/Microsoft.Web/sites/app1",
      "name": "app1",
      "type": "Microsoft.Web/sites",
      "icon": "app-service",
      "portal_url": "https://portal.azure.com/#@/resource/<resourceId>",
      "suggested_commands": ["az webapp show --ids <resourceId>"]
    }
  ]
}
```

The `icon` field is the stable icon *key* (e.g. `"app-service"`), the same
key used to look up the bundled SVG under `assets/azure-icons/`; the map's
own HTML embeds the actual `<svg>` markup inline, while API consumers get
just the key so they can resolve it however they like (e.g. against their
own copy of the icon set, or simply displayed as a label).

Resource IDs shown above are the resources' own Azure Resource Manager IDs —
identifiers, not secrets — and are the only sensitive-looking field the API
ever returns; no keys, connection strings, or tokens are ever included in any
response.

## Interactivity: room pop-ups

Clicking a room on the rendered map opens a client-side pop-up (no page
reload) that fetches `/api/v1/rooms/<id>` and lists every resource in that
room, each shown with:

1. Its **Azure architecture icon** (from the [type → icon
   registry](#azure-architecture-icons)), rendered at a larger size than the
   in-room tile so the resource type is easy to identify at a glance.
2. A **deep link to the Azure portal** for that exact resource — see
   [Portal deep links](#portal-deep-links).
3. One or more **suggested read-only `az` commands** for inspecting it — see
   [Suggested `az` commands](#suggested-az-commands).

The pop-up is display-only: nothing you click executes a command or mutates
anything. It's a map legend, not a control panel.

## Portal deep links

Each resource's Azure Resource Manager ID is used to construct a direct link
into the Azure portal, in the standard resource-blade deep-link form:

```
https://portal.azure.com/#@/resource/<resourceId>
```

For example, a storage account with resource ID
`/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/data-rg/providers/Microsoft.Storage/storageAccounts/mystorageacct`
gets the link:

```
https://portal.azure.com/#@/resource/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/data-rg/providers/Microsoft.Storage/storageAccounts/mystorageacct
```

Clicking it opens the real resource's Overview blade in the portal, in a new
tab, for whichever tenant/account you're already signed into there — the map
itself never authenticates to Azure on your behalf.

## Suggested `az` commands

For each resource, the type → icon registry doubles as a type → suggested
command table — both are derived from the same single type-mapping table in
code (one row per resource type: icon + suggested command), so the icon and
its suggested command can never drift out of sync. This lets the pop-up show
one or more relevant read-only `az` invocations, e.g.:

| Resource type | Suggested command |
| --- | --- |
| `Microsoft.Storage/storageAccounts` | `az storage account show --ids <resourceId>` |
| `Microsoft.Compute/virtualMachines` | `az vm show --ids <resourceId>` |
| `Microsoft.Web/sites` | `az webapp show --ids <resourceId>` |
| `Microsoft.KeyVault/vaults` | `az keyvault show --ids <resourceId>` |
| `Microsoft.Sql/servers` | `az sql server show --ids <resourceId>` |
| `Microsoft.ContainerService/managedClusters` | `az aks show --ids <resourceId>` |
| `Microsoft.DocumentDB/databaseAccounts` | `az cosmosdb show --ids <resourceId>` |
| Unknown/unmapped type | `az resource show --ids <resourceId>` |

These are **text only** — the map never shells out to run them for you. Copy
one into your own terminal if you want to actually run it.

## Safety and guardrails

Dungeon Crawler Mode inherits AzZork's core safety property: every `az`
invocation goes through the [`AzRunner`](../src/az_runner.rs) argument-vector
seam (never shell string interpolation), and enumeration is restricted to an
explicit allow-list of read-only verbs (`list`, `show`, `account show`, and
similar), never anything that mutates:

- **No writes, ever.** Enumeration cannot create, update, delete, lock,
  unlock, or deploy anything. There is no code path from `crawl` back into a
  mutating `az` call.
- **No secrets on screen or on disk.** Any field in `az` JSON output that looks
  like a key, connection string, SAS token, or credential is scrubbed before it
  reaches the map graph, the rendered HTML, the JSON API, or memory — Dungeon
  Crawler Mode does not persist anything to AzZork's graph memory at all.
- **Defensive JSON parsing.** Untrusted `az … -o json` output is parsed
  structurally (via `serde`/`serde_json`), not via fragile string/line
  parsing, and malformed or unexpected output is handled as a recoverable
  per-resource skip with a logged warning — never a panic that aborts the
  whole crawl.
- **Escaped rendering.** Resource and room names are untrusted strings (an
  attacker-controlled Azure tenant could name a resource `<script>...`) and are
  always HTML/SVG-escaped before being written into the rendered map or JSON,
  so a hostile resource name can't inject markup or script into the page.
- **Loopback-only server.** The embedded HTTP server binds to `127.0.0.1`
  only, never a wildcard address, and sends no permissive CORS headers.

## Scaling to large subscriptions

There is no fixed cap on the number of resource groups or resources Dungeon
Crawler Mode will map. Instead, enumeration is adaptive:

- **Streaming/paginated `az` calls** rather than one unbounded call per
  resource type, so memory use tracks what's currently being processed, not
  the size of the whole subscription.
- **Bounded in-memory windows** (tunable via `--budget`, default `500`
  resources per window) — resources are flushed into the map graph in batches
  rather than held all at once, so very large subscriptions don't require
  proportionally large memory.
- **Cancellable** — `Ctrl-C` during enumeration stops cleanly and serves/writes
  whatever has been assembled so far, clearly labeled as a partial map (with a
  note on how many rooms/resources were seen before cancellation), rather than
  losing all progress or hanging.
- **Parallel, self-tuning resource-group enumeration** — resource groups are
  walked concurrently by a small worker pool (bounded by the host's available
  parallelism) instead of one at a time. Concurrency is governed by an AIMD
  (additive-increase/multiplicative-decrease) limiter: it ramps up gradually
  on sustained success and immediately halves the moment Azure signals
  throttling (HTTP 429 / `Retry-After`), then retries the throttled call with
  jittered backoff honoring any `Retry-After` value. Output ordering is
  unaffected by which worker finishes first: the map graph is always
  assembled in the same, deterministic order regardless of run-to-run timing.

## Configuration

Dungeon Crawler Mode reuses AzZork's existing backend configuration exactly
(see [Configuration reference](CONFIGURATION.md#backend-selection)): the same
`--backend`/`-b`/`AZORK_BACKEND` precedence, the same `mock` and `az` backend
ids. There is no separate config file and no additional environment variables
required for basic use; `--port`, `--out`, `--budget`, and `--playwright` are
the only mode-specific knobs, and all are plain CLI flags with the defaults
described above.

## Troubleshooting

**"Failed to build dungeon map via az backend"** — same causes and fix as the
REPL's `az` backend: make sure `az login` has been run and you have at least
one resource group. See [Configuration → The az backend](CONFIGURATION.md).

**The server prints a port but my browser can't connect** — the server binds
to `127.0.0.1`, so use `http://127.0.0.1:<port>` or `http://localhost:<port>`,
not a LAN/hostname address; it isn't reachable from another machine by design.

**`--playwright` did nothing different** — this is expected if Playwright
isn't installed locally; the pass is a local-only, optional post-process
step (never a driver of an external website), so it silently no-ops and
`crawl` continues with the native dungeon-style render.

**The map looks incomplete / says "partial"** — either enumeration was
cancelled mid-flight (`Ctrl-C`) or the subscription is still being paginated
through when `--out` was requested without `--serve`; re-run without
interrupting, or use `--serve` and refresh once the "Dungeon assembled" line
appears.
