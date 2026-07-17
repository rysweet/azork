The user-journeys layer traces six end-to-end scenarios through real modules and functions, from CLI entry points through parsing, backends, maps, memory, update, OIT, and reporting outputs.

![user-journeys-1-crawl-mock-large (mermaid)](user-journeys-1-crawl-mock-large-mermaid.svg)

![user-journeys-1-crawl-mock-large (dot)](user-journeys-1-crawl-mock-large-dot.svg)

![user-journeys-2-repl-look-move-quests-score (mermaid)](user-journeys-2-repl-look-move-quests-score-mermaid.svg)

![user-journeys-2-repl-look-move-quests-score (dot)](user-journeys-2-repl-look-move-quests-score-dot.svg)

![user-journeys-3-oit-dry-run-report (mermaid)](user-journeys-3-oit-dry-run-report-mermaid.svg)

![user-journeys-3-oit-dry-run-report (dot)](user-journeys-3-oit-dry-run-report-dot.svg)

![user-journeys-4-self-update-check (mermaid)](user-journeys-4-self-update-check-mermaid.svg)

![user-journeys-4-self-update-check (dot)](user-journeys-4-self-update-check-dot.svg)

![user-journeys-5-capabilities-autodiscover-registry (mermaid)](user-journeys-5-capabilities-autodiscover-registry-mermaid.svg)

![user-journeys-5-capabilities-autodiscover-registry (dot)](user-journeys-5-capabilities-autodiscover-registry-dot.svg)

![user-journeys-6-dungeon-snapshot-diff-time-rift (mermaid)](user-journeys-6-dungeon-snapshot-diff-time-rift-mermaid.svg)

![user-journeys-6-dungeon-snapshot-diff-time-rift (dot)](user-journeys-6-dungeon-snapshot-diff-time-rift-dot.svg)

| Journey | Entry | Outcome |
|---|---|---|
| 1 | `azork crawl --backend mock --mock-size large` | `DungeonMap` rendered to HTML or output guidance |
| 2 | REPL `look`, `move`, `quests`, `score` | world navigation and governance output |
| 3 | `azork-oit --dry-run --report path` | Markdown friction report |
| 4 | `azork update --check` or `azork update` | update availability or checked install path |
| 5 | startup autodiscovery | registry and memory updated with learned capabilities |
| 6 | `azork dungeon --snapshot` then `--diff` | snapshot JSON and Time Rift report |
