The service-components layer splits AzZork into subsystem diagrams so each graph stays readable while still showing internal module coupling and the real Rust files that define each boundary.

![service-components-dungeon (mermaid)](service-components-dungeon-mermaid.svg)

![service-components-dungeon (dot)](service-components-dungeon-dot.svg)

![service-components-backend (mermaid)](service-components-backend-mermaid.svg)

![service-components-backend (dot)](service-components-backend-dot.svg)

![service-components-capabilities (mermaid)](service-components-capabilities-mermaid.svg)

![service-components-capabilities (dot)](service-components-capabilities-dot.svg)

![service-components-memory (mermaid)](service-components-memory-mermaid.svg)

![service-components-memory (dot)](service-components-memory-dot.svg)

![service-components-update (mermaid)](service-components-update-mermaid.svg)

![service-components-update (dot)](service-components-update-dot.svg)

![service-components-oit (mermaid)](service-components-oit-mermaid.svg)

![service-components-oit (dot)](service-components-oit-dot.svg)

![service-components-agent-agent_engine (mermaid)](service-components-agent-agent_engine-mermaid.svg)

![service-components-agent-agent_engine (dot)](service-components-agent-agent_engine-dot.svg)

| Subsystem | Primary files | Key coupling |
|---|---|---|
| dungeon | `src/dungeon/*.rs` | map build, render, server, diff, icon/type tables |
| backend | `src/backend/*.rs`, `src/az_runner.rs` | `Backend` trait, live Azure runner, mock estate generation |
| capabilities | `src/capabilities/*.rs` | help parsing, registry persistence, startup autodiscovery |
| memory | `src/memory/mod.rs` | scrubbed graph memory and line-format persistence |
| update | `src/update/*.rs` | release check, download, checksum, archive, install |
| oit | `src/oit/*.rs`, `src/bin/azork-oit.rs` | guardrails, use-case catalog, friction report |
| agent and agent_engine | `src/agent/mod.rs`, `src/agent_engine/mod.rs` | offline intent resolution and recipe-runner adapter |
