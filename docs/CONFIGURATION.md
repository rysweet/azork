# AzZork Configuration Reference

AzZork is intentionally configuration-light. There are no config files, no save
files, and nothing is persisted to disk — all state lives in memory and is
discarded when you quit. Behaviour is controlled entirely by a single choice:
**which backend** builds the world.

## Backend selection

The backend decides where the dungeon map comes from. It is chosen (in
precedence order) by:

1. A `--backend <id>` / `-b <id>` command-line flag.
2. The `--backend=<id>` form.
3. The `AZORK_BACKEND` environment variable.
4. Default: `mock`.

```bash
azork                       # mock (default)
azork --backend az          # live Azure
azork -b az                 # same, short flag
azork --backend=mock        # explicit mock
AZORK_BACKEND=az azork       # via environment
```

### Recognized backend ids

| Id(s) | Backend | Requires credentials? | Network? |
| --- | --- | --- | --- |
| `mock` (default) | Offline synthetic estate | No | No |
| `az`, `real`, `azure` | Live Azure via the `az` CLI | Yes (`az login`) | Yes |

If no backend is requested, AzZork uses `mock`. If you *explicitly* request an
unrecognized id (e.g. a typo like `--backend azue`, or `AZORK_BACKEND=aws`),
AzZork still falls back to `mock` so you are never credential-gated — but it
first prints a warning to stderr making clear you are on the offline estate, not
your live subscription:

```
Warning: unknown backend 'azue'; falling back to the offline mock estate.
Recognised backends: mock, az. (This is NOT your live Azure subscription.)
```

## The `mock` backend (default)

The mock backend hand-authors a small, deliberately hazardous Azure estate so
the game is fully playable offline with **zero credentials and zero network
calls**. It always includes at least one dark (unmonitored) room so the Grue
mechanic is reachable.

The starting layout:

```
                unmon-rg  (DARK — Grue lurks)
                   |  north/south
web-rg  ── north ──┘
   | south
landing-rg (start) ── east ── data-rg
   | down
identity-rg
```

| Room | Region | Monitored | Notable resources |
| --- | --- | --- | --- |
| `landing-rg` (start) | eastus | yes | `portal` |
| `web-rg` | eastus | yes | `appservice` (public), `webstore` (public, unencrypted) |
| `data-rg` | westus2 | yes | `sqlserver` ($800/mo), `keyvault` (unlocked) |
| `identity-rg` | eastus | yes | `managed-identity` |
| `unmon-rg` | centralus | **no** | `orphan-vm` (public, unencrypted, $300/mo) |

This world is used by the entire test suite; no test path ever invokes the `az`
backend.

## The `az` backend (live Azure)

The `az` backend maps your **real** subscription into the dungeon by shelling
out to the installed Azure CLI. It is never used by default and never exercised
by the tests.

### Prerequisites

- The [`az` CLI](https://learn.microsoft.com/cli/azure/install-azure-cli) must
  be installed and on your `PATH`.
- You must be authenticated: `az login`.
- An active subscription with at least one resource group.

### What it reads (read-only)

The backend performs only non-mutating discovery calls, requesting
tab-separated output (`-o tsv`) with narrow `--query` projections so no JSON
dependency is needed:

| Purpose | Command |
| --- | --- |
| Subscription name | `az account show --query name -o tsv` |
| Rooms (resource groups) | `az group list --query "[].{name:name,location:location}" -o tsv` |
| Objects (resources) | `az resource list -g <group> --query "[].{name:name,type:type}" -o tsv` |

Resource groups become rooms, chained north↔south into a navigable corridor.
Each group's resources become objects in that room. Live rooms are assumed
monitored (the game cannot cheaply prove otherwise), so the Grue mechanic is
primarily a mock-world feature.

### Safety guarantees

- **Read-only:** only `show`/`list` discovery verbs are ever invoked. No
  create, update, or delete calls are made against Azure.
- **No injection surface:** `az` is invoked via an argument array
  (`Command::args`), never through a shell; player input never flows into `az`
  arguments.
- **No secrets surfaced:** only non-secret metadata (names, types, locations)
  is read — never Key Vault contents, connection strings, or keys.
- **In-memory only:** `take`, `drop`, `lock`, `unlock`, and `resize` mutate the
  in-memory world exclusively. Nothing is written back to Azure.
- **Graceful failure:** if `az` is missing, you are not logged in, or no
  resource groups are found, AzZork prints a helpful message and exits with a
  tip to use the default mock backend:

  ```
  Failed to build world via az (live Azure) backend: no resource groups found
  (or not logged in). Try 'az login', or run with the default mock backend.
  Tip: run without arguments to use the offline mock backend.
  ```

## Environment variables

| Variable | Values | Default | Effect |
| --- | --- | --- | --- |
| `AZORK_BACKEND` | `mock`, `az`, `real`, `azure` | `mock` | Selects the backend when no `--backend` flag is given. |
| `AZORK_CACHE_DIR` | any writable directory | see below | Directory for the learned-capability cache (`capabilities.tsv`) and graph memory (`memory.graph`). |
| `AZORK_MAX_ROOMS` | positive integer | `40` | (`az` backend) max resource groups mapped into rooms, so large tenants stay responsive. |
| `AZORK_MAX_RESOURCE_ROOMS` | positive integer | `8` | (`az` backend) max rooms whose resources are enumerated via `az resource list`. |
| `AZORK_NO_UPDATE_CHECK` | any non-empty value | unset | Disables the cheap, cached self-update check performed at startup. See the [Self-Update guide](UPDATING.md). |
| `AZORK_BIN` | path to the `azork` executable | see below | Used by the `az azork` CLI extension (and other launchers) to locate the compiled binary. |
| `AZORK_OIT_SUBSCRIPTION` | Azure subscription id | maintainer's test subscription | (`azork-oit` only) overrides the subscription id the OIT agent's preflight check requires before running live. |
| `AZORK_OIT_TENANT` | tenant display name | maintainer's test tenant | (`azork-oit` only) overrides the tenant name recorded/expected by the OIT agent; informational, not enforced by preflight. |
| `AZORK_OIT_ISSUES` | comma-separated list | unset | (`azork-oit` only) issue references (e.g. `#42,#57`) to cite in the generated friction report. |

### `azork-oit` (Outside-In-Testing agent) environment variables

The `azork-oit` binary (see [Usage: OIT agent](USAGE.md#outside-in-testing-oit-agent))
drives AzZork against a **live** subscription. It refuses to run against any
subscription other than the one it expects, as a safety guardrail:

- `AZORK_OIT_SUBSCRIPTION` — the subscription id `az account show` must match
  during preflight. Defaults to the maintainer's non-secret test subscription.
  Set this to authorise the agent against your own tenant.
- `AZORK_OIT_TENANT` — the expected tenant display name, recorded in the
  friction report. Defaults to the maintainer's test tenant name.
- `AZORK_OIT_ISSUES` — a comma-separated list of issue references (e.g.
  `#12,#34`) folded into the `## Issues` section of the generated
  `docs/oit-friction-report.md`, so friction findings can be traced back to
  tracked work.

None of these variables affect the default `azork` game binary — they are read
only by `azork-oit`.

## Persistence

AzZork keeps **no** configuration file, no world save/restore, and no
serialization of the *game state* — every session starts fresh from the selected
backend, and world changes are lost on exit. This keeps the destructive verbs
safe.

The one thing that *does* persist is AzZork's **learned vocabulary**. Running
`learn <group>` introspects `az <group> --help` and writes the discovered
capabilities to a small tab-separated cache file, which is recalled on the next
launch so the game accumulates knowledge over time. The cache location is:

1. `$AZORK_CACHE_DIR/capabilities.tsv` if `AZORK_CACHE_DIR` is set;
2. else `$XDG_DATA_HOME/azork/capabilities.tsv`;
3. else `~/.local/share/azork/capabilities.tsv`.

The cache holds only public `az` command names and their one-line help summaries
— no credentials, subscription data, or resource contents. Delete the file to
reset AzZork's learned capabilities.
