# Azure architecture icons — attribution and license notice

The SVG files in this directory are **original artwork created for AzZork**,
used to represent Azure resource types inside Dungeon Crawler Mode's map and
room pop-ups (see
[`docs/DUNGEON-CRAWLER.md#azure-architecture-icons`](../../docs/DUNGEON-CRAWLER.md#azure-architecture-icons)).

## Why these are original icons, not Microsoft's

Microsoft publishes an official **Azure Architecture Icons** set for building
your own architecture diagrams
([learn.microsoft.com/azure/architecture/icons](https://learn.microsoft.com/en-us/azure/architecture/icons/)).
Those terms cover *using* the icons to illustrate a diagram you create — they
do not grant a license to **redistribute the icon files themselves** bundled
inside a third-party repository or compiled into a third-party binary, which
is what shipping `include_str!`-embedded copies of them inside `azork` would
be.

To avoid any ambiguity, every icon in this directory is a simple,
hand-authored monochrome line glyph created specifically for this project.
Each one is intended to evoke its resource category (e.g. a stacked-cylinder
motif for a storage account, a rack silhouette for a virtual machine) without
copying Microsoft's icon shapes, layout, or color palette. **No file in this
directory is derived from, traced from, or a modified copy of any Microsoft
icon asset.**

## Ownership and license

- These icons are original works owned by the AzZork project and are
  licensed under the same license as the rest of this repository (see the
  top-level [`LICENSE`](../../LICENSE)).
- AzZork is **not affiliated with or endorsed by Microsoft**. "Azure" and the
  names of Azure services referenced in resource type strings (e.g.
  `Microsoft.Storage/storageAccounts`) remain trademarks/assets of Microsoft
  Corporation; these icons merely label that a mapped resource is of that
  service type and must not be read as, or presented as, Microsoft's own
  Azure Architecture Icons.
- These icons must not be used to imply Microsoft's endorsement of AzZork,
  and must not be used as a substitute for or copy of Microsoft's official
  icon set in other projects.

## Fallback icon

`mystery-chest.svg` is used whenever a resource's Azure type doesn't match
any row in the [type → icon table](../../src/dungeon/type_table.rs), so an
unrecognized or newly-released resource type still renders with *some* icon
instead of being silently dropped from the map.

## Adding a new icon

1. Author a new original monochrome line SVG for the resource category and
   place it in this directory, named after its icon key (e.g.
   `event-hub.svg` for icon key `"event-hub"`). Do not copy or trace
   Microsoft's icon artwork when creating it.
2. Add a row to [`src/dungeon/type_table.rs`](../../src/dungeon/type_table.rs)
   mapping the resource's `type` prefix to that icon key and its suggested
   `az … show` command.
3. No other wiring is needed — the icon loader resolves keys from the type
   table against the files in this directory automatically, falling back to
   `mystery-chest.svg` for anything unmapped.
