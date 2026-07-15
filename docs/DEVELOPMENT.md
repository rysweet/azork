# AzZork Development Guide

This guide covers the contributor workflow for AzZork: local quality gates
(pre-commit), continuous integration, and test coverage.

- Player docs: [Usage guide](USAGE.md), [Tutorial](TUTORIAL.md).
- Internals: [API / module reference](API.md).
- Self-update mechanism & releases: [Self-Update guide](UPDATING.md).

## Everyday commands

```bash
cargo build          # compile
cargo test           # run the full test suite (unit + integration)
cargo run            # play with the offline mock backend
cargo fmt            # format
cargo fmt --check    # verify formatting (used by hooks/CI)
cargo clippy -- -D warnings   # lint, warnings are errors
```

All four gates — `fmt --check`, `clippy -D warnings`, `build`, and `test` — must
be green before code is merged.

## Pre-commit hooks

AzZork ships a lightweight [pre-commit](https://pre-commit.com/) configuration
(`.pre-commit-config.yaml`) that runs the same gates locally that CI enforces, so
problems are caught before you push.

### What runs

On every commit the hooks run:

| Hook | Command | Purpose |
| ---- | ------- | ------- |
| Format check | `cargo fmt --check` | Fail if code is not rustfmt-formatted. |
| Lint | `cargo clippy -- -D warnings` | Fail on any clippy warning. |
| Tests | `cargo test` | Fail if any test fails. |

The configuration is intentionally minimal — three Rust gates, no unrelated
plugins.

### Install

```bash
# One-time: install the pre-commit tool
pipx install pre-commit        # or: pip install pre-commit / brew install pre-commit

# From the repo root, install the git hook
pre-commit install
```

After installation the hooks run automatically on `git commit`. To run them
against the whole tree on demand:

```bash
pre-commit run --all-files
```

To bypass hooks for a work-in-progress commit (use sparingly):

```bash
git commit --no-verify
```

### Formatting & lint configuration

- `rustfmt.toml` — repository formatting rules (kept close to rustfmt defaults).
- Clippy runs with `-D warnings`, so warnings are treated as errors both locally
  and in CI.

`Cargo.lock` is committed so that hooks and CI build the exact same dependency
versions (`cargo build --locked`).

## Continuous integration

CI is a single lean GitHub Actions workflow at `.github/workflows/ci.yml` that
runs on every push and pull request.

### What CI does

One job (kept deliberately small — no sprawling matrix):

1. Check out the repo and install a stable Rust toolchain
   (with `rustfmt`, `clippy`, and `llvm-tools-preview`).
2. `cargo fmt --check`
3. `cargo clippy -- -D warnings`
4. `cargo build --locked`
5. `cargo test --locked`
6. Measure and **print** line coverage (see below).

Tests never touch the network — the update/release layer is mocked — so CI is
deterministic and offline.

### Coverage as a reported metric (not a gate)

CI measures line coverage with
[`cargo-llvm-cov`](https://github.com/taiki-e/cargo-llvm-cov) and **prints the
number** in the job log and summary, for example:

```
Coverage: 73.4% lines
```

Coverage is reported, **not enforced as a hard gate**. This is intentional: a
strict 70% gate could block unrelated, concurrently developed pull requests. The
project targets **≥ 70% line coverage** as a standard, verified by the reported
number, without a merge-blocking threshold.

## Test coverage

### Measuring locally

```bash
# Install once
cargo install cargo-llvm-cov
rustup component add llvm-tools-preview

# Summary (prints total line coverage)
cargo llvm-cov --summary-only

# HTML report you can open in a browser
cargo llvm-cov --html         # target/llvm-cov/html/index.html

# LCOV output (for editors / external tooling)
cargo llvm-cov --lcov --output-path lcov.info
```

> Alternative: `cargo tarpaulin` also works if `cargo-llvm-cov` is unavailable,
> though `cargo-llvm-cov` is preferred for accuracy.

### Current coverage

The suite holds line coverage at **≥ 70%** (measured with `cargo-llvm-cov`). The
exact percentage is printed by CI on every run and by
`cargo llvm-cov --summary-only` locally.

### What is tested

Tests live both alongside the code (unit tests) and in the external `tests/`
directory (integration/contract tests). Coverage focuses on the highest-value,
highest-LOC surfaces:

| Area | Representative tests |
| ---- | -------------------- |
| Parser | verbs, directions, aliases, whitespace, and edge cases (`tests/parser_tests.rs`). |
| World model | rooms, resources, hazards, scoring, and the Grue mechanic (`tests/world_tests.rs`). |
| Backend | backend selection + mock-estate invariants (`tests/backend_tests.rs`). |
| Update — version logic | `normalize_tag`, `is_newer`, `select_asset`, `should_check` (`tests/update_pure_tests.rs`). |
| Update — checksum | fail-closed `verify_sha256` on match/mismatch (`tests/update_checksum_tests.rs`). |
| Update — archive | traversal-safe extraction; rejects `..`/absolute/symlink entries (`tests/update_archive_tests.rs`). |
| Update — startup safety | `classify_skip_reason` for CI/NONINTERACTIVE/AGENT/TTY/opt-out; never prompts (`tests/update_startup_tests.rs`). |
| End-to-end | typed-session workflows through the public API (`tests/integration_tests.rs`). |

### Tests never hit the network

The updater's network access is isolated to a single module (`update::network`).
All other update logic is pure and tested directly. The test suite exercises
version comparison, target selection, checksum verification, archive extraction,
and skip-classification **entirely offline** — no test makes an outbound
request, so the suite is fast and reliable in CI.

## Project layout

```
azork/
├── Cargo.toml / Cargo.lock
├── rustfmt.toml
├── .pre-commit-config.yaml
├── .github/workflows/
│   ├── ci.yml            build + test + fmt + clippy + coverage (reported)
│   └── release.yml       on v* tag: build, package, checksum, upload assets
├── src/
│   ├── main.rs           REPL + `azork update` dispatch + startup gate
│   ├── lib.rs            crate root; pub const VERSION
│   ├── parser.rs
│   ├── world.rs
│   ├── backend/
│   └── update/           self-update mechanism (see docs/UPDATING.md)
├── tests/                integration & contract tests
└── docs/
```

## Cutting a release

See the [Self-Update guide → Release flow](UPDATING.md#release-flow-where-updates-come-from)
for full detail. In short:

```bash
git tag v0.3.0
git push origin v0.3.0
```

`release.yml` builds the binary, produces `azork-<triple>.tar.gz` and its
`.sha256`, and uploads them to a new GitHub Release — which is exactly what the
self-updater consumes.
