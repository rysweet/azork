# azork-agentic-bridge

An **optional companion crate** that bridges AzZork into the MIT-licensed
[`recipe-runner-rs`](https://crates.io/crates/recipe-runner-rs) agentic engine —
mirroring how Simard and Powderfinger embed the runner in a Rust agent.

It is deliberately **separate** from the `azork` package so that azork's own
`cargo build` / `cargo test` remain **zero-dependency, offline, and green on a
fresh clone**. azork has no `[workspace]` table, so building at the azork root
never compiles this crate.

## What it provides

- `AzorkAdapter` — implements the recipe-runner `Adapter` trait. *Agent* steps
  resolve intent against AzZork's learned `CapabilityRegistry` using the offline
  resolver (deterministic, no LLM, no network); *bash* steps delegate to the
  runner's `CLISubprocessAdapter` so recipes can shell out to `az`.
- `run_intent_recipe(yaml, registry, dry_run)` — runs an inline amplihack recipe
  with AzZork as the adapter.
- `INTENT_RESOLUTION_RECIPE` — a minimal built-in single-step recipe.

## Requirements

This crate uses **path dependencies** to sibling repos and therefore needs them
checked out **side-by-side** in the canonical layout:

```
~/src/
├── azork/                     (this repo)
│   └── agentic-bridge/        (this crate)
└── amplihack-recipe-runner/   (the recipe-runner-rs crate)
```

If your layout differs, adjust the `recipe-runner-rs` path in `Cargo.toml`.

## Build & test

```bash
cd agentic-bridge
cargo test
```

Licensed MIT, same as azork.
