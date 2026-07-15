# Security Audit — AzZork

**Date:** 2026-07-15 (updated after integrating `origin/main`)
**Scope:** `security/audit-fixes`, rebased onto the current `main`, which now
includes the self-update mechanism, GitHub Actions CI, and the `az_runner.rs`
process-spawning refactor (formerly tracked on the since-merged
`feat/issue-4-*` and `feat/issue-5-*` branches, referenced below by their
original names where the finding predates the merge).

## Threat Model

See [`SECURITY.md`](../SECURITY.md) for the summary trust-boundary model.
In short: player stdin is fully untrusted but constrained to a closed parser
grammar; `az` CLI stdout/stderr is semi-trusted content that must never be
shell-interpreted; downloaded release artifacts are untrusted until a
SHA-256 check passes; persisted state (graph memory) must never carry
credentials.

## Findings

| # | Area | Finding | Severity | Location | Status |
|---|------|---------|----------|----------|--------|
| 1 | Secrets in error text | `az` stderr/stdout was surfaced verbatim in error messages/println with no redaction; a misconfigured extension, `--debug` output, or future code path could echo a token, SAS signature, or connection string into logs. | Medium | `src/backend/az.rs::AzBackend::run_once`; `src/capabilities/derive.rs::run_help` | **Fixed** — added `src/secrets::scrub`, wired symmetrically into both the error and success paths of `run_once` (via `format_az_error` and the `Ok` arm) and of `run_help`, the two call sites that turn `az_runner::AzRunner::run`'s raw `Output` bytes into text. Unit-tested (`az_error_messages_are_scrubbed_of_secrets`, `run_once_success_path_scrubs_stdout` in `src/backend/az.rs`; 8+ tests in `src/secrets.rs`). |
| 2 | Subprocess invocation | Confirmed all `az` invocations use `Command::new("az").args(&[...])` — argument vectors, never `sh -c`/string interpolation. | Info (verification) | `src/az_runner.rs::ProcessAzRunner::run` — the single hardened seam (wall-clock timeout, zombie-process cleanup, pipe-deadlock protection) used by both `src/backend/az.rs` and `src/capabilities/derive.rs` | **Verified safe**, regression test added (`run_once_never_shell_joins_arguments`) proving hostile metacharacters survive as a single unmodified argument. |
| 3 | Malformed/attacker-influenced `az` TSV output | `build_world`'s TSV parser (`line.split('\t')`) must not panic on missing columns, empty names, or stray tabs. | Low | `src/backend/az.rs::parse_group_tsv` / `parse_resource_tsv` | **Verified safe + tested** (`build_world_handles_malformed_tsv_without_panicking`). No panics found; parser already uses `Option`/`unwrap_or` defensively. |
| 4 | Player-input parsing | Parser must not panic on empty, whitespace-only, or adversarial multi-token input. | Low | `src/parser.rs` | **Verified safe** — `tokens.is_empty()` guard, no indexing without bounds checks, existing `parser_tests.rs` includes a dedicated `parser_never_panics_on_hostile_input` test. |
| 5 | Dependency hygiene | `cargo audit` had never been run against the workspace. | Info | `Cargo.toml` | **Run** — see raw output below; re-run after any dependency addition (the merged self-update module adds `ureq`, `sha2`, `tar`, `flate2`, `semver`, `serde`/`serde_json`). |
| 6 | Self-update supply chain (now merged into `main`) | Reviewed `src/update/{network,checksum,archive,install,check}.rs`. Host allowlist restricts downloads to `https://{api.,,objects.}github.com/...` (rejects `http://`, other hosts); SHA-256 verification (`verify_or_error`) runs **before** any bytes are extracted or executed and fails closed on malformed/absent digests; `extract_binary` rejects `..`, absolute paths, and non-file/dir entry types (symlinks/hardlinks refused) before writing, and caps decompressed size at 512 MiB; `install_binary_atomic` copies to a sibling temp file and `rename`s over the target (atomic on the same filesystem); `is_newer`/semver comparison in `check.rs` prevents installing an older or equal version (no downgrade). | — | `src/update/*.rs` | **Already hardened, now merged.** No code fix required as part of this audit. |
| 7 | OIT resource ownership guardrail (now merged into `main`) | Reviewed `src/oit/guardrails.rs`. `guard_mutation`/`is_own_resource` require **both** the `owner=azork-oit` tag and the `azork-oit=1` marker tag before any delete/mutate is permitted (checked in code, not just by resource-group naming convention); cost is gated via `assess_cost` (hard $500 cap, NaN/negative estimates rejected rather than assumed free). | — | `src/oit/guardrails.rs` | **Already hardened, now merged.** Unit-tested (`ownership_requires_both_tags`, `cost_gate_rejects_over_cap_and_untrusted`). No code fix required. |
| 8 | Python azext subprocess safety (now merged into `main`) | Reviewed `azext/azext_azork/custom.py`. All `az azork` shims call `subprocess.run([binary, ...])` with argument lists — no `shell=True`, no `eval`/`exec`, no string-built commands. `_backend_args` validates the `--backend` value against an explicit allowlist (`mock`/`az`) before it ever reaches `subprocess.run`. | — | `azext/azext_azork/custom.py` | **Already hardened, now merged.** No code fix required. |
| 9 | Memory-store persistence (now merged into `main`) | Reviewed `src/memory/mod.rs` and the optional `memory-store/src/lib.rs`. Storage goes through a structured API (typed inserts/searches) — no hand-built SQL strings, so no SQL-injection surface. No explicit secret-scrubbing layer existed before persistence, however. | Low | `src/memory/mod.rs`, `memory-store/src/lib.rs` | **Fixed** — `GraphMemory::remember` (the single choke point every recorder — `record_friction`, `record_intent`, `remember_capability`, `remember_room`, `remember_resource` — funnels through) now scrubs `label`/`content` with `crate::secrets::scrub` before a node is created, so both the in-memory graph and its on-disk save format (`GraphMemory::save`) are covered. `memory-store/src/lib.rs::node_to_experience` independently calls `azork::secrets::scrub` (reusing the same helper via memory-store's existing `azork` path dependency — no new crate/dependency) as defense-in-depth at the durable-store boundary, mirroring the pattern applied to Finding #1. Any future node-creation path **must** route through `remember()` rather than constructing nodes directly, or it will bypass scrubbing — see maintenance note in `src/memory/mod.rs`. Unit-tested: `memory::tests::remember_scrubs_secret_shaped_content_and_label`, `memory::tests::record_friction_scrubs_secret_shaped_note`, `memory::tests::save_and_load_round_trip_never_reintroduces_secret` (`src/memory/mod.rs`); `tests::save_scrubs_secret_shaped_content_before_persisting` (`memory-store/src/lib.rs`). Closes [issue #17](https://github.com/rysweet/azork/issues/17). |

## `cargo audit` Output

```
Fetching advisory database from `https://github.com/RustSec/advisory-db.git`
      Loaded 1160 security advisories (from ~/.cargo/advisory-db)
    Updating crates.io index
    Scanning Cargo.lock for vulnerabilities (93 crate dependencies)
```

No vulnerabilities found (exit code 0). The dependency graph is now 93
crates deep — the self-update module (`ureq`, `sha2`, `tar`, `flate2`,
`semver`, `serde`/`serde_json`) is merged into `main` and pulls in its own
transitive dependencies. Re-run `cargo audit` after any further dependency
addition.

## CI / Local Verification (this branch)

| Check | Result |
|---|---|
| `cargo build --locked` | ✅ Pass |
| `cargo test --locked` | ✅ Pass — 116 lib tests + integration/parser/world/update/memory/oit test binaries, all green |
| `cargo clippy --all-targets -- -D warnings` | ✅ Pass, zero warnings |
| `cargo fmt --check` | ✅ Pass |
| `cargo audit` | ✅ 0 vulnerabilities across 93 scanned dependencies |
| GitHub Actions `build · test · lint · coverage` (`.github/workflows/ci.yml`) | Runs the same four checks plus `cargo llvm-cov` (reported, not gated) on every push/PR |
| GitGuardian | Secret-shaped literals are confined to `src/secrets.rs`, the sole path listed in `.gitguardian.yaml`'s `ignored_paths` |

## Residual Risks / Accepted

- **`az` binary trust.** AzZork trusts whatever `az` binary is first on
  `PATH` (or bundled next to the `azext`/`AZORK_BIN` override). This mirrors
  the trust model of every other CLI tool that shells out to `az`;
  pinning/verifying the `az` binary itself is out of scope for AzZork and is
  the responsibility of the Azure CLI's own installer/update mechanism.
- **`az` output is not adversarially fuzzed against the real CLI.** Our
  parsers are defensive against malformed TSV/JSON by construction and unit
  tests, but we have not run a formal fuzzer (e.g. `cargo-fuzz`) against
  them. Given the small, well-bounded parsing surface (tab-separated text,
  no recursive/nested structures), this is accepted as low risk; revisit if
  the capability-derivation help-text parser's surface grows.
- **Findings #6–#8 are reviewed, not re-audited line-by-line in this pass.**
  That code is now merged into `main` and was already well-hardened when
  reviewed; this update focused on re-wiring secret scrubbing onto the
  `az_runner.rs`-based structure `main` introduced, not on re-auditing
  functionality unrelated to that refactor. Finding #9's recommendation
  (scrub free-text fields before persistence) is now **implemented** — see
  the Findings table above and [issue #17](https://github.com/rysweet/azork/issues/17).
- **Pre-existing on-disk memory files predating this fix.** Scrubbing is
  applied at write time going forward; any `GraphMemory` save files created
  *before* this fix landed may still contain unredacted secret-shaped text
  from earlier sessions. This fix does not retroactively rescan or rewrite
  historical saves. A lightweight follow-up (rescan/rotate existing save
  files through `src/secrets::scrub` on load, or prompt for a one-time
  migration) is recommended but out of scope for this PR; track separately
  if historical exposure is a concern in a given deployment.
