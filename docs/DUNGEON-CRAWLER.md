# Dungeon Crawler Mode

**Turn your entire Azure subscription into an explorable, hand-drawn dungeon
map you can click through in a browser.**

Where the classic AzZork REPL (see the [Usage guide](USAGE.md)) plays out one
resource group at a time as a text adventure, Dungeon Crawler Mode steps back
and draws the **whole subscription at once**: every resource group is a room,
every resource inside it is an icon on the floor, and corridors connect rooms
that share a region or a network relationship. It is read-only, fully offline
by default, and safe to point at a real subscription — it never creates,
modifies, or deletes anything.

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
- [Icons](#icons)
- [Rendering](#rendering)
- [The optional Playwright renderer](#the-optional-playwright-renderer)
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
            [--budget <n>] [--playwright]
```

| Flag | Default | Meaning |
| --- | --- | --- |
| `--backend <id>` | `mock` | Which `az` backend to enumerate (`mock` or `az`, same ids as the REPL — see [Configuration reference](CONFIGURATION.md)). |
| `--serve` | off | Start the embedded HTTP server and serve the map + JSON API instead of (or in addition to) writing a file. |
| `--port <n>` | `0` (OS-assigned free port) | Port for `--serve`. `0` lets the OS pick a free ephemeral port, which is then printed to stdout. |
| `--out <path>` | none | Write the rendered map (self-contained HTML) to a file. Can be combined with `--serve`. |
| `--budget <n>` | `500` | Soft cap on in-memory resources buffered per enumeration window before flushing to the map graph; tune only if you are constrained on memory. Does **not** limit how much of the subscription is mapped — enumeration always continues to completion or cancellation, just in bounded-size batches. |
| `--playwright` | off | Best-effort: additionally attempt a richer hand-drawn render via a headless-browser pass. Silently falls back to the native renderer if unavailable — see [below](#the-optional-playwright-renderer). |

Press `Ctrl-C` to stop the server; enumeration itself can also be cancelled
mid-flight (`Ctrl-C` during the "Mapping subscription..." phase) and will still
serve whatever partial map has been assembled so far, clearly marked as
partial.

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
  layout engine required for the default render).

The graph is the single source of truth handed to both the native renderer and
the HTTP server's JSON API — the picture and the API are two views onto the
same model, never two separate sources of truth.

Enumeration is **strictly read-only**: it only ever issues `list`/`show`-class
`az` invocations (an explicit allow-list of read verbs), never anything that
creates, updates, or deletes a resource, group, or subscription-level setting.

## Icons

Every resource node is annotated with an icon looked up from its Azure resource
type (e.g. `Microsoft.Storage/storageAccounts`, `Microsoft.Compute/virtualMachines`,
`Microsoft.Network/virtualNetworks`, `Microsoft.Web/sites`,
`Microsoft.KeyVault/vaults`, `Microsoft.ContainerService/managedClusters`,
`Microsoft.Sql/servers`, `Microsoft.DocumentDB/databaseAccounts`, and more) via
a type → icon registry.

- Where licensing allows, the registry prefers the official Azure architecture
  icon set (SVG). Where it doesn't (offline run, no network, or the vendored
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

## Rendering

The primary renderer is **native, offline, and deterministic**: given the map
graph, it lays rooms out on a grid, draws corridor walls between adjacent/
connected rooms, and places each resource's icon inside its room, producing a
single self-contained HTML document (inline SVG + a small amount of vanilla
JS — no build step, no CDN fetch, nothing that requires network access to
view). The same subscription always produces the same map, because layout is
a pure function of the map graph, not of viewport size, timing, or randomness.

This is the mode used by both `--out` (write to a file you can open directly
in any browser, even with no network) and `--serve` (serve the same document
over HTTP).

## The optional Playwright renderer

Passing `--playwright` additionally attempts a **best-effort** second pass that
drives a headless browser against [dungeonscrawl.com](https://www.dungeonscrawl.com/)
(or an equivalent local renderer) to produce a more richly hand-drawn version
of the same map graph.

This path is intentionally isolated and optional:

- It lives in its own module and is never compiled into, required by, or
  exercised by the default build, `cargo test`, or CI.
- It requires a separate one-time setup step (installing Node.js/Playwright
  browsers) documented inline in that module — it is not a Cargo dependency of
  the `azork` crate.
- If the flag is passed but Playwright isn't installed, the site is
  unreachable, or anything about the browser-driven pass fails, Dungeon
  Crawler Mode **prints a warning and falls back to the native renderer**
  automatically. `--playwright` never turns a working native render into a
  hard failure.
- No Azure data is retained by the external site beyond what's needed to
  render the page in the local headless session; no credentials or resource
  IDs are ever sent anywhere by the native renderer, and the Playwright path
  only sends the same non-secret shape/label data that's already in the map
  graph (room names/regions and resource type/name labels — never resource
  IDs, secrets, or connection strings).

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

Resource IDs shown above are the resources' own Azure Resource Manager IDs —
identifiers, not secrets — and are the only sensitive-looking field the API
ever returns; no keys, connection strings, or tokens are ever included in any
response.

## Interactivity: room pop-ups

Clicking a room on the rendered map opens a client-side pop-up (no page
reload) that fetches `/api/v1/rooms/<id>` and lists every resource in that
room, each shown with:

1. Its **icon** (from the [type → icon registry](#icons)).
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
isn't installed, the network is unavailable, or dungeonscrawl.com couldn't be
reached; Dungeon Crawler Mode logs a one-line notice and continues with the
native render rather than failing the whole command.

**The map looks incomplete / says "partial"** — either enumeration was
cancelled mid-flight (`Ctrl-C`) or the subscription is still being paginated
through when `--out` was requested without `--serve`; re-run without
interrupting, or use `--serve` and refresh once the "Dungeon assembled" line
appears.
