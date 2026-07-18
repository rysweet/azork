# AzZork Startup Auto-Discovery

## Overview

AzZork no longer needs a player to type `learn <group>` before it understands
an `az` command group. On every launch, AzZork automatically enumerates the
top-level `az` command groups and folds newly-discovered commands into its
`CapabilityRegistry`, so the vocabulary is already rich by the time the first
prompt appears — and it keeps growing across sessions as new `az` groups
become available.

## Usage

**Cold start (no cache):** AzZork runs `az --help`, learns each top-level
group on a background thread, and streams results into the registry between
turns. `capabilities`/`help` grow richer as discovery completes; the game is
playable immediately and never blocks waiting for discovery.

**Warm start (cache present):** AzZork loads
`~/.local/share/azork/capabilities.tsv` (or `$AZORK_CACHE_DIR/capabilities.tsv`)
first. Only groups *not* already in the cache are re-enumerated —
already-known groups are skipped, so warm starts stay fast.

**Manual refresh — `learn <group>`:** Still available. Forces an immediate
re-learn of one group (e.g. `learn storage`), bypassing the incremental
background cadence — useful right after an `az` CLI upgrade adds new
subcommands.

```
> learn keyvault
Learned 14 new capabilities from 'az keyvault'.
```

**Discovering what's known — `capabilities` / `caps`:**

```
> capabilities
AzZork has learned 87 capabilities across 12 az command groups:
  storage (9), keyvault (14), network (11), vm (18), ...
```

**Cancellation:** If you start typing before background discovery finishes,
discovery yields to your input rather than delaying the response — no group
is left half-applied; already-streamed results remain in the registry.

**Graceful degradation:** If the `az` CLI is missing, unauthenticated, or
errors out, startup still succeeds using the cache plus AzZork's built-in
verbs, with a friendly one-line notice — never a crash or a hang.

## Configuration

| Variable             | Default                | Effect                                                                                                                                |
| --------------------- | ----------------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| `AZORK_AUTODISCOVER`  | enabled                | Set to `0`, `false`, or `no` (case-insensitive) to disable automatic startup discovery entirely. `learn <group>` still works manually. |
| `AZORK_CACHE_DIR`     | `~/.local/share/azork`  | Overrides where `capabilities.tsv` is read from and written to.                                                                        |

Example — disable auto-discovery for CI/offline runs:

```
AZORK_AUTODISCOVER=0 azork
```

**`azork-oit --dry-run` always sets this for you.** The OIT agent's dry-run
mode spawns its `azork` child with `AZORK_AUTODISCOVER=0` forced, regardless
of the value (or absence) of `AZORK_AUTODISCOVER` in the parent environment.
This is what makes `--dry-run` genuinely offline: without it, startup
auto-discovery in the child would otherwise reach out to a real `az` install
the moment the child process starts, before any use case even runs. See
[`azork-oit --dry-run` is genuinely offline](USAGE.md#--dry-run-is-genuinely-offline)
for the full picture (autodiscovery kill-switch plus a stubbed `az` runner for
`learn`-style use cases).

## API (module `src/capabilities/autodiscover.rs`)

Pure, synchronous, offline-testable functions — no threading inside them;
the caller decides how to schedule. All types/functions live in
`azork::capabilities::autodiscover`.

- `struct GroupResult { group: String, outcome: Result<Vec<Capability>, String> }`
  The outcome of attempting to learn one group's capabilities.

- `struct AppliedGroup { group: String, result: Result<usize, String> }`
  The outcome of applying one `GroupResult` into a `CapabilityRegistry`
  (added-count on success, the original error otherwise).

- `discover_new_groups(runner: &dyn AzRunner, known_groups: &[String]) -> Result<Vec<String>, String>`
  Runs `az --help` through the given `AzRunner`, parses top-level group
  names, and returns only those **not** already in `known_groups`.

- `learn_groups(runner: &dyn AzRunner, groups: &[String]) -> Vec<GroupResult>`
  For each group, runs `az <group> --help` and parses it into `Capability`
  records (reusing `derive::derive_group_capabilities` — the same parser
  `learn <group>` uses). Returns one `GroupResult` per group so a single
  group's failure doesn't abort the others.

- `run_startup_autodiscovery(runner: &dyn AzRunner, known_groups: &[String]) -> Vec<GroupResult>`
  End-to-end, fully synchronous convenience wrapper combining the two
  functions above (`discover_new_groups` then `learn_groups`). Used directly
  in tests (no thread needed).

- `apply_learned(registry: &mut CapabilityRegistry, results: impl IntoIterator<Item = GroupResult>) -> Vec<AppliedGroup>`
  Applies successful results into the registry (returning the number of
  newly-added capabilities per group) and skips/reports failed groups per
  `AppliedGroup`; persisting the updated cache is the caller's
  responsibility (`main.rs` does this after applying).

- `stream_startup_autodiscovery(runner: &dyn AzRunner, known_groups: &[String], cancel: &AtomicBool, tx: &Sender<GroupResult>)`
  The production entry point, wired into `main.rs`'s background
  `std::thread`: discovers missing groups, then learns them one at a time,
  sending each `GroupResult` over `tx` as soon as it's ready so the main
  thread can apply capabilities incrementally between turns rather than
  waiting for every group to finish. Checks `cancel` before starting and
  between each group so a caller can stop further (not-yet-started)
  discovery once the player begins interacting.

- `autodiscover_enabled() -> bool`
  Reads the `AZORK_AUTODISCOVER` environment variable (`AUTODISCOVER_ENV`)
  to decide whether startup discovery should run at all.

## Examples

**Fresh clone, first run:**

```
$ azork
AzZork is exploring the az CLI in the background...
> look
(you can play immediately; capabilities keep arriving)
> capabilities
AzZork has learned 42 capabilities across 6 az command groups so far...
```

**Second run (warm cache), one new `az` extension installed:**

```
$ azork
(cache loaded instantly; only the new group is discovered)
> capabilities
AzZork has learned 51 capabilities across 7 az command groups.
```

**`az` not installed:**

```
$ azork
az CLI not found — starting with cached and built-in capabilities only.
```

See the [API / module reference](API.md#capabilities-module) for how this
fits into the rest of the `capabilities` module.
