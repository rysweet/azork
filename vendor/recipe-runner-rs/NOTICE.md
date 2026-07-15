# Vendored dependency: recipe-runner-rs

This directory contains a vendored copy of the `recipe-runner-rs` crate
(version 0.3.6), sourced from the `amplihack-recipe-runner` repository at
commit `90b91f941d0b7487c7bb1d92eca0a4140a630706`.

- Upstream project: `amplihack-recipe-runner` (crate `recipe-runner-rs`)
- License: MIT (as declared in the upstream `Cargo.toml`)
- Reason for vendoring: `recipe-runner-rs` is not published on crates.io, so it
  cannot be pulled in as a registry dependency, and a git dependency would
  require network access on a fresh clone / CI cold cache. Vendoring the
  source and referencing it via a `path` dependency keeps azork's default
  `cargo build`/`cargo test` fully offline and reproducible while embedding
  the agentic recipe-running capability by default.

Only `src/`, `Cargo.toml`, and `README.md` are vendored; docs, examples, and
test fixtures from the upstream repo are omitted since they are not needed to
build the library.
