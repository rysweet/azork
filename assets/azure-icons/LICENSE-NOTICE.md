# Azure architecture icons — attribution and license notice

The SVG files in this directory are Microsoft's **official Azure Architecture
Icons** ("Azure Public Service Icons" set), downloaded from Microsoft's
official distribution at
[learn.microsoft.com/azure/architecture/icons](https://learn.microsoft.com/en-us/azure/architecture/icons/),
and used unmodified to represent Azure resource types inside Dungeon Crawler
Mode's map and room pop-ups (see
[`docs/DUNGEON-CRAWLER.md#azure-architecture-icons`](../../docs/DUNGEON-CRAWLER.md#azure-architecture-icons)).

## Source and usage

Per Microsoft's published guidelines for this icon set, the icons are
provided to help build architecture diagrams that illustrate how Azure
products work together. Consistent with those guidelines, azork:

- Uses each icon unmodified (not cropped, flipped, rotated, distorted, or
  recolored) to label its corresponding Azure resource type on the rendered
  dungeon map — an architecture-diagram-style visualization of a real Azure
  subscription.
- Does not use any Microsoft product icon to represent a non-Microsoft
  product or service.
- Bundles the icon files (embedded at compile time via `include_str!`, see
  [`src/dungeon/icon_assets.rs`](../../src/dungeon/icon_assets.rs)) purely so
  the rendered map — including a saved `--out` HTML file — works fully
  offline, with no runtime hotlinking to Microsoft's servers.

## Ownership and trademarks

- These icon files remain the property of **Microsoft Corporation**. AzZork
  claims no ownership over them and includes them solely for the
  architecture-diagram illustration use described above.
- AzZork is **not affiliated with, sponsored by, or endorsed by Microsoft**.
  Use of these icons must not be read as, or presented as, an endorsement of
  AzZork by Microsoft.
- "Azure" and the names of Azure services referenced in resource type strings
  (e.g. `Microsoft.Storage/storageAccounts`) are trademarks of Microsoft
  Corporation.

## Fallback icon

`mystery-chest.svg` (Microsoft's "All Resources" icon) is used whenever a
resource's Azure type doesn't match any row in the
[type → icon table](../../src/dungeon/type_table.rs), so an unrecognized or
newly-released resource type still renders with *some* icon instead of being
silently dropped from the map.

## Adding a new icon

1. Download the correct official icon for the resource category from
   Microsoft's [Azure Architecture Icons](https://learn.microsoft.com/en-us/azure/architecture/icons/)
   set and place it in this directory, unmodified, named after its icon key
   (e.g. `event-hub.svg` for icon key `"event-hub"`).
2. Add a row to [`src/dungeon/type_table.rs`](../../src/dungeon/type_table.rs)
   mapping the resource's `type` prefix to that icon key and its suggested
   `az … show` command.
3. No other wiring is needed — the icon loader resolves keys from the type
   table against the files in this directory automatically, falling back to
   `mystery-chest.svg` for anything unmapped.
