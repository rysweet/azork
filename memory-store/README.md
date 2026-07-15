# azork-memory-store

An **optional companion crate** that gives AzZork **durable, SQLite-backed graph
memory** on top of the MIT-licensed
[`amplihack-memory`](https://github.com/rysweet/amplihack-memory-lib) library —
the same cognitive-memory crate Simard uses.

AzZork's own [`GraphMemory`](../src/memory/mod.rs) is a small, dependency-free,
fully-offline brick with a hand-rolled on-disk format. This crate is the *live*
durable counterpart: it mirrors that graph into an `amplihack-memory`
`MemoryConnector`, so the game's learned capabilities, resource graph (rooms &
objects), intents, and friction notes survive across sessions and can be recalled
with full-text ranked search.

It is deliberately **separate** from the `azork` package so that azork's own
`cargo build` / `cargo test` remain **zero-dependency, offline, and green on a
fresh clone**. Cargo resolves path/optional dependencies even when a feature is
off, so pulling `amplihack-memory` (which compiles bundled SQLite) into the azork
manifest would break that invariant. azork has no `[workspace]` table, so building
at the azork root never compiles this crate.

## What it provides

- `PersistentStore::open(agent, dir)` — open/create a durable store (SQLite db
  under `dir`).
- `PersistentStore::save(&mem)` — mirror every node **and edge** of a
  `GraphMemory` into the store. Each node becomes one `amplihack-memory`
  `Experience`; ids, usage, touch ticks, and edges ride along in metadata for a
  faithful reload. Re-saving is safe (load de-duplicates by node id, freshest
  wins). Free-text `label`/`content` are passed through `azork::secrets::scrub`
  before being written — defense-in-depth alongside `GraphMemory::remember`'s
  own scrubbing — so a stray secret never lands in the durable store. Note
  this only applies going forward: store files written before this scrubbing
  was added are not retroactively rescanned.
- `PersistentStore::load()` — rehydrate a `GraphMemory` from the store, edges and
  all. Records AzZork didn't write are ignored, so the store can be shared.
- `PersistentStore::recall(query, limit)` — ranked recall through the external
  store's **SQLite full-text search** (the library's own recall engine).

### Node → Experience mapping

| AzZork node          | `Experience` field                     |
|----------------------|----------------------------------------|
| `label`              | `context`                              |
| `content`            | `outcome`                              |
| `importance`         | `confidence`                           |
| `kind`               | `ExperienceType` (+ a tag)             |
| `tags`               | `tags`                                 |
| `id`, `usage`, edges | `metadata` (for faithful reload)       |

## Requirements

This crate uses **path dependencies** to sibling repos and therefore needs them
checked out **side-by-side** in the canonical layout:

```
~/src/
├── azork/                     (this repo)
│   └── memory-store/          (this crate)
└── amplihack-memory-lib/      (the amplihack-memory crate)
```

If your layout differs, adjust the `amplihack-memory` path in `Cargo.toml`.
Building compiles a bundled SQLite (via `rusqlite`), so the first build takes a
few minutes.

## Build & test

```bash
cd memory-store
cargo test
```

Licensed MIT, same as azork.
