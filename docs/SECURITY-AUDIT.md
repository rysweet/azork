# Security Audit — AzZork

**Date:** 2026-07-15
**Scope:** `origin/main` (audited and fixed directly), plus the two open,
unmerged feature branches `feat/issue-4-mission-evolve-the-azork-rust-project-repo-at-home`
and `feat/issue-5-mission-in-the-azork-rust-project-working-copy-at` (read
and reviewed for the incoming attack surface described in the mission brief;
**not modified**, per the audit's guardrails).

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
| 1 | Secrets in error text | `az` stdout/stderr was surfaced verbatim in error messages/println with no redaction; a misconfigured extension, `--debug` output, or future code path could echo a token, SAS signature, or connection string into logs. | Medium | `src/backend/az.rs::run_once` (main) | **Fixed** — added `src/secrets::scrub`, wired into both the error path and the success path (stdout), so the redaction is symmetric. Unit-tested (`az_error_messages_are_scrubbed_of_secrets`, `az_success_output_is_scrubbed_of_secrets`, 8 tests in `src/secrets.rs`). |
| 2 | Subprocess invocation | Confirmed all `az` invocations use `Command::new("az").args(&[...])` — argument vectors, never `sh -c`/string interpolation. | Info (verification) | `src/backend/az.rs` (main); `src/az_runner.rs`, `src/capabilities/derive.rs` (branch `feat/issue-4-*`) | **Verified safe**, regression test added (`run_once_never_shell_joins_arguments`) proving hostile metacharacters survive as a single unmodified argument. |
| 3 | Malformed/attacker-influenced `az` TSV output | `build_world`'s TSV parser (`line.split('\t')`) must not panic on missing columns, empty names, or stray tabs. | Low | `src/backend/az.rs` (main) | **Verified safe + tested** (`build_world_handles_malformed_tsv_without_panicking`). No panics found; parser already uses `Option`/`unwrap_or` defensively. |
| 4 | Player-input parsing | Parser must not panic on empty, whitespace-only, or adversarial multi-token input. | Low | `src/parser.rs` (main) | **Verified safe** — `tokens.is_empty()` guard, no indexing without bounds checks, existing `parser_tests.rs` includes a dedicated `parser_never_panics_on_hostile_input` test. |
| 5 | Dependency hygiene | `cargo audit` had never been run against the workspace. | Info | `Cargo.toml` (main) | **Run** — `main` currently declares **zero external dependencies**, so `cargo audit` reports 0 vulnerabilities (1160 advisories checked, 1 crate scanned — the workspace crate itself). See raw output below. |
| 6 | Self-update supply chain (not yet merged) | Reviewed `src/update/{network,checksum,archive,install,check}.rs` on `feat/issue-5-*`. Host allowlist restricts downloads to `https://{api.,,objects.}github.com/...` (rejects `http://`, other hosts); SHA-256 verification (`verify_or_error`) runs **before** any bytes are extracted or executed and fails closed on malformed/absent digests; `extract_binary` rejects `..`, absolute paths, and non-file/dir entry types (symlinks/hardlinks refused) before writing, and caps decompressed size at 512 MiB; `install_binary_atomic` copies to a sibling temp file and `rename`s over the target (atomic on the same filesystem); `is_newer`/semver comparison in `check.rs` prevents installing an older or equal version (no downgrade). | — | `src/update/*.rs` on `feat/issue-5-*` | **Already hardened on the branch.** No code fix required; documented here so it is verified before merge. Recommend the branch's CI also runs `cargo audit` (it already vendors `Cargo.lock`; not run in this audit since we do not modify that branch). |
| 7 | OIT resource ownership guardrail (not yet merged) | Reviewed `src/oit/guardrails.rs` on `feat/issue-4-*`. `guard_mutation`/`is_own_resource` require **both** the `owner=azork-oit` tag and the `azork-oit=1` marker tag before any delete/mutate is permitted (checked in code, not just by resource-group naming convention); cost is gated via `assess_cost` (hard $500 cap, NaN/negative estimates rejected rather than assumed free). | — | `src/oit/guardrails.rs` on `feat/issue-4-*` | **Already hardened on the branch.** Unit-tested (`ownership_requires_both_tags`, `cost_gate_rejects_over_cap_and_untrusted`). No code fix required. |
| 8 | Python azext subprocess safety (not yet merged) | Reviewed `azext/azext_azork/custom.py` on `feat/issue-4-*`. All `az azork` shims call `subprocess.run([binary, ...])` with argument lists — no `shell=True`, no `eval`/`exec`, no string-built commands. `_backend_args` validates the `--backend` value against an explicit allowlist (`mock`/`az`) before it ever reaches `subprocess.run`. | — | `azext/azext_azork/custom.py` on `feat/issue-4-*` | **Already hardened on the branch.** No code fix required. |
| 9 | Memory-store persistence (not yet merged) | Reviewed `memory-store/src/lib.rs` on `feat/issue-4-*`. Storage goes through the `amplihack-memory` crate's structured API (typed inserts/searches) — no hand-built SQL strings, so no SQL-injection surface. No explicit secret-scrubbing layer exists before persistence, however. | Low | `memory-store/src/lib.rs` on `feat/issue-4-*` | **Documented recommendation** (branch not modified): apply `src/secrets::scrub` (or an equivalent) to any free-text `content`/`label` fields before they are written to the store, mirroring the pattern added to `main` in Finding #1. This is a straightforward, low-risk follow-up once the branch merges. |

## `cargo audit` Output

```
Fetching advisory database from `https://github.com/RustSec/advisory-db.git`
      Loaded 1160 security advisories (from ~/.cargo/advisory-db)
    Updating crates.io index
    Scanning Cargo.lock for vulnerabilities (1 crate dependencies)
```

No vulnerabilities found. `main`'s `Cargo.toml` currently declares no
`[dependencies]`, so the scanned dependency graph is limited to the `azork`
crate itself. Re-run `cargo audit` after any dependency is added (including
when the `feat/issue-5-*` update module, which adds `ureq`, `sha2`, `tar`,
`flate2`, `semver`, `serde`/`serde_json`, is merged).

## CI / Local Verification (this branch)

| Check | Result |
|---|---|
| `cargo build` | ✅ Pass |
| `cargo test` | ✅ 49 lib tests + 63 integration/parser/world tests pass |
| `cargo clippy --all-targets -- -D warnings` | ✅ Pass, zero warnings |
| `cargo fmt --check` | ✅ Pass |
| `cargo audit` | ✅ 0 advisories (0 dependencies beyond the crate itself) |

## Residual Risks / Accepted

- **`az` binary trust.** AzZork trusts whatever `az` binary is first on
  `PATH` (or bundled next to the `azext`/`AZORK_BIN` override on the
  unmerged branch). This mirrors the trust model of every other CLI tool
  that shells out to `az`; pinning/verifying the `az` binary itself is out
  of scope for AzZork and is the responsibility of the Azure CLI's own
  installer/update mechanism.
- **`az` output is not adversarially fuzzed against the real CLI.** Our
  parsers are defensive against malformed TSV/JSON by construction and unit
  tests, but we have not run a formal fuzzer (e.g. `cargo-fuzz`) against
  them. Given the small, well-bounded parsing surface (tab-separated text,
  no recursive/nested structures), this is accepted as low risk; revisit if
  the parsing surface grows (e.g. when the `feat/issue-4-*` JSON-based
  capability/memory parsing merges).
- **Unmerged branches are reviewed, not gated.** Findings #6–#9 describe code
  that is not yet on `main`. They are already well-hardened, but this audit
  cannot enforce that they stay that way through further iteration on those
  branches; re-review is recommended at merge time.
