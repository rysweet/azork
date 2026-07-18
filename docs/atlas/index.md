# azork Code Atlas

A living, regeneratable architecture atlas for **rysweet/azork**, built directly from code
truth. Each layer below has Mermaid (`.mmd`) and Graphviz (`.dot`) sources, rendered SVGs, and
a README that embeds the diagrams inline. Diagrams are built in both formats deliberately —
they surface different classes of structural bug.

> Regenerate with `scripts/atlas-render.sh` (SVGs) and `scripts/atlas-kuzu-ingest.py` (graph).

## Layers

| # | Layer | Description | Diagrams |
|---|-------|-------------|----------|
| 1 | [Repository Surface](repo-surface/README.md) | Top-level dirs, build systems, the two binaries | Mermaid + DOT |
| 2 | [AST + LSP Symbol Bindings](ast-lsp-bindings/README.md) | Public symbols & cross-file refs (lsp-assisted) | Mermaid + DOT |
| 3 | [Compile-time Dependencies](compile-deps/README.md) | Cargo deps + inter-module use-graph | Mermaid + DOT |
| 4 | [Runtime Topology](runtime-topology/README.md) | Processes, ports, the embedded HTTP server | Mermaid + DOT |
| 5 | [API Contracts](api-contracts/README.md) | HTTP routes + CLI/REPL command surface | Mermaid + DOT |
| 6 | [Data Flow](data-flow/README.md) | backend → World → map/render/server/diff | Mermaid + DOT |
| 7 | [Service Components](service-components/README.md) | Per-subsystem internal module structure | Mermaid + DOT |
| 8 | [User Journeys](user-journeys/README.md) | 6 end-to-end traced scenarios | Mermaid + DOT |

## Graph model

The atlas is ingested into a [Kuzu](https://kuzudb.com/) graph and also emitted as portable
OpenCypher for any graph database. See [`cypher/`](cypher/):

- `schema.cypher` — node/relationship table definitions
- `atlas-layers.cypher`, `atlas-services.cypher`, `atlas-relationships.cypher`, `atlas-bugs.cypher`
- `queries.cypher` — ready-to-run example queries

`atlas-manifest.json` is the machine-readable source the ingestion reads.

## Bug hunting

Structural bugs and contradictions discovered while reasoning over the atlas are **filed as
GitHub issues** with the `code-atlas-bughunt` label — never stored in these docs. The atlas is
an architecture map, not a bug tracker.

## Staleness

`staleness-map.yaml` maps each layer to the source globs that, when changed, mark that layer
stale and due for a rebuild.
