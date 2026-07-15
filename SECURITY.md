# Security Policy

## Reporting a Vulnerability

Please report suspected security issues privately via GitHub's
["Report a vulnerability"](https://github.com/rysweet/azork/security/advisories/new)
flow rather than a public issue. We aim to acknowledge reports within a few
business days.

## Threat Model (Summary)

AzZork is a text-adventure front end over the Azure CLI (`az`). Its trust
boundaries are:

1. **Player input (stdin)** — untrusted. Never used to build a shell command
   string; only ever parsed into a fixed, closed set of in-game verbs
   (`src/parser.rs`).
2. **`az` CLI output** — semi-trusted. It reflects the operator's own Azure
   subscription, but the *content* (resource names, error text) should still
   be treated as attacker-influenceable in principle (e.g. a resource named
   by someone else in a shared subscription) and must never be shell-executed
   or trusted to be well-formed.
3. **The `az` binary itself / network** — the live `az` backend
   (`--backend az`) is opt-in and read-only at world-build time; the default
   `mock` backend used by the test suite never touches Azure or the network.
4. **Downloaded release artifacts** (self-update, on the `feat/issue-5-*`
   branch) — untrusted until their SHA-256 digest is verified against the
   published checksum asset.
5. **Persisted state** (graph memory, on the `feat/issue-4-*` branch) — local
   SQLite storage; must never contain credentials/tokens.

Full findings, severities, and status are tracked in
[`docs/SECURITY-AUDIT.md`](docs/SECURITY-AUDIT.md).

## Guarantees Enforced in Code

- **No shell interpolation.** Every subprocess invocation uses
  `Command::new(program).args(&[...])` with each argument as a discrete
  vector element. Nothing is ever passed through `sh -c` with interpolated
  data. See `src/backend/az.rs::AzBackend::run_once` and the equivalent
  `AzRunner`/`azext` seams described in `docs/SECURITY-AUDIT.md`.
- **Secret scrubbing.** `src/secrets::scrub` redacts key-value pairs, Azure
  connection strings, SAS signatures, bearer tokens, and JWT-shaped strings
  before any `az` error text is surfaced. Wired into
  `AzBackend::run_once`'s error path.
- **No panics on untrusted CLI/JSON output.** Parsing of `az ... -o tsv`
  output and command input is written to tolerate missing columns, empty
  fields, and malformed lines without panicking (see tests in
  `src/backend/az.rs` and `src/parser.rs`).
- **Dependency hygiene.** `cargo audit` is run against the workspace; results
  are recorded in `docs/SECURITY-AUDIT.md` and re-checked in CI.

## Supported Versions

Security fixes are made against the `main` branch and released via tagged
GitHub releases.
