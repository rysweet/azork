# AzZork Self-Update Guide

AzZork can keep itself current. It ships with a built-in updater that queries
the project's GitHub Releases, compares the published version against the binary
you are running, and — when a newer release exists — downloads the correct
per-platform asset, verifies its SHA-256 checksum, and atomically replaces the
running executable in place.

The updater is designed to stay **out of the way**:

- It is **cached** (a check runs at most once every 24 hours).
- It is **skippable** with a single environment variable.
- It is **subprocess- and CI-safe**: it never hangs, blocks, or prompts when
  running non-interactively, under CI, or when invoked by another program.
- It **never runs during the test suite** and **never hits the network in
  tests**.

- New here? Start with the [Usage guide](USAGE.md).
- Installing for the first time? See the [Install guide](INSTALL.md).
- Configuring backends and environment? See the [Configuration reference](CONFIGURATION.md).
- Building releases or contributing? See the [Development guide](DEVELOPMENT.md).

## Quick start

```bash
# Check for a newer release and install it if one exists.
azork update

# Only report whether an update is available; do not install.
azork update --check

# Install even if the cooldown says a check ran recently.
azork update --force
```

If you are already on the latest version:

```
$ azork update
azork 0.2.0 is already the latest release. Nothing to do.
```

If a newer version is available:

```
$ azork update
A new release of azork is available: 0.2.0 -> 0.3.0
  downloading  azork-x86_64-unknown-linux-gnu.tar.gz
  verifying    sha256 … ok
  extracting   azork
  installing   /usr/local/bin/azork
Updated azork 0.2.0 -> 0.3.0. Restart to use the new version.
```

## How it works

```
azork update
   │
   ├─ 1. resolve GITHUB_REPO = rysweet/azork
   ├─ 2. GET /repos/rysweet/azork/releases/latest      (GitHub Releases API)
   ├─ 3. normalize_tag("v0.3.0") -> semver 0.3.0
   ├─ 4. is_newer(current=0.2.0, latest=0.3.0)?        (strict '>' only)
   │        └─ no  -> "already the latest", exit 0
   ├─ 5. supported_release_target() -> azork-<triple>.tar.gz
   │        └─ pick the asset matching this OS/arch
   ├─ 6. download asset  +  download asset.sha256
   ├─ 7. verify SHA-256 (fail-closed: mismatch aborts, nothing installed)
   ├─ 8. extract the `azork` binary from the .tar.gz (traversal-safe)
   ├─ 9. atomically replace the current executable
   └─ 10. write .installed-version stamp for drift detection
```

Each step maps to a small, single-purpose module (see
[API reference](#api-reference) below). The only module that performs network
I/O is `update::network`; everything else is pure and offline-testable.

## The startup check

In addition to the explicit `azork update` command, AzZork performs an optional,
**cheap, cached** update check when you launch the game interactively. The
startup check is deliberately conservative:

1. If any **skip condition** applies (see below), the check is skipped
   immediately and silently-but-visibly (a one-line notice), and the game
   continues.
2. Otherwise, if the 24-hour cooldown has not elapsed since the last check, it
   is skipped.
3. Otherwise it makes a single lightweight request. If a newer version exists it
   prints a short notice. When interactive, it may offer to update with a
   **5-second prompt timeout** — if you do not answer within 5 seconds it
   assumes "no", continues, and never blocks.

The startup check outcome is one of:

| Outcome | Meaning |
| ------- | ------- |
| `Continue`   | Proceed into the normal REPL (the common case). |
| `ExitSuccess` | An update was installed; exit cleanly so the user can relaunch. |

### Skip conditions (never prompt, never hang)

The startup check is **skipped entirely** — with no network call and no prompt —
if any of the following is true:

| Condition | Detected via |
| --------- | ------------ |
| Opt-out flag set        | `AZORK_NO_UPDATE_CHECK=1` |
| Running under CI        | `CI` is set |
| Non-interactive shell   | `NONINTERACTIVE` is set |
| Invoked by an agent     | `AGENT_BINARY` is set |
| stdin is not a TTY      | terminal detection |
| Explicit subprocess flag | `--subprocess-safe` on the command line |
| Cooldown not elapsed    | `< 24h` since last check (cache file) |

When a skip occurs at startup, AzZork prints a single visible skip line so the
behaviour is never a silent mystery, for example:

```
[update-check: skipped (AZORK_NO_UPDATE_CHECK)]
```

This "visible skip-line" is a contract: any skip is observable, but it is always
a single line and never interactive.

## Configuration

### Opt out of update checks

Set `AZORK_NO_UPDATE_CHECK=1` to disable the automatic startup check entirely.
The explicit `azork update` command still works when you run it yourself.

```bash
export AZORK_NO_UPDATE_CHECK=1
azork                 # no startup check
azork update          # still works on demand
```

### Environment variables

| Variable | Effect |
| -------- | ------ |
| `AZORK_NO_UPDATE_CHECK=1` | Disable the automatic startup update check. |
| `CI`                      | Treated as CI; startup check skipped. |
| `NONINTERACTIVE`          | Treated as non-interactive; startup check skipped. |
| `AGENT_BINARY`            | Treated as agent-invoked; startup check skipped. |
| `AZORK_RELEASE_VERSION`   | Overrides the reported build version (set at release time). |

### Cache file

The updater records the timestamp of its last check in an XDG config file:

```
$XDG_CONFIG_HOME/azork/last_update_check   # or ~/.config/azork/last_update_check
```

The base directory honours `XDG_CONFIG_HOME` when it is set and non-empty,
falling back to `~/.config` otherwise (standard XDG Base Directory resolution).

- The file contains a single timestamp; it is created on the first successful
  check and rewritten on each subsequent check.
- If the file is missing, unreadable, or malformed, the updater treats the
  cooldown as **elapsed** (fail-open toward checking, never toward crashing).
- Delete the file to force the next launch to perform a check:

  ```bash
  rm ~/.config/azork/last_update_check
  ```

- The cooldown is a fixed **24 hours**. To bypass it once, use
  `azork update --force`.

### Installed-version stamp & drift detection

After a successful install, AzZork writes an `.installed-version` stamp next to
the executable using an **atomic write** (write-to-temp then rename). On the next
run, AzZork compares the stamp against the running binary's compiled version:

- **In sync** — nothing to do.
- **Drift detected** (stamp missing or does not match the running binary, e.g.
  the binary was swapped or re-staged out of band) — AzZork rewrites the stamp
  and continues. Drift detection never blocks startup.

This mirrors the version-stamp / self-heal design so that partial or external
updates are detected and reconciled without user intervention.

## Command reference

```
azork update [--check] [--force] [--subprocess-safe]
```

| Flag | Meaning |
| ---- | ------- |
| *(none)* | Check, and if a newer release exists, download, verify, and install it. |
| `--check` | Report whether an update is available; do **not** install anything. Exit code reflects availability. |
| `--force` | Ignore the 24-hour cooldown and check now. |
| `--subprocess-safe` | Never prompt; suitable for scripts and other programs. Also honoured on the normal launch path to guarantee non-interactive behaviour. |

### Exit codes

The updater uses distinct exit codes so scripts can react precisely:

| Code | Meaning |
| ---- | ------- |
| `0` | Success — up to date, or update installed. |
| `1` | Generic / usage error. |
| `2` | Network error (could not reach GitHub Releases or download an asset). |
| `3` | Checksum verification failed — **nothing was installed** (fail-closed). |
| `4` | Install target not writable (e.g. binary owned by root); no changes made. |
| `5` | No supported release asset for this OS/architecture. |

Every non-generic code corresponds to an `UpdateError` variant. The
`UpdateError → exit code` mapping is centralised in `update::mod` (via an
`UpdateError::exit_code()`), so codes `2`–`5` above are the authoritative
contract that the error enum must match:

| `UpdateError` variant | Exit code |
| --------------------- | --------- |
| `Network(..)`         | `2` |
| `ChecksumMismatch`    | `3` |
| `TargetNotWritable`   | `4` |
| `NoSupportedAsset`    | `5` |

## Supported targets

The updater installs the asset whose name matches the running platform's target
triple, following the naming convention below:

| Platform | Asset name |
| -------- | ---------- |
| Linux x86_64 (glibc) | `azork-x86_64-unknown-linux-gnu.tar.gz` |
| Linux aarch64 (glibc) | `azork-aarch64-unknown-linux-gnu.tar.gz` |
| macOS x86_64 | `azork-x86_64-apple-darwin.tar.gz` |
| macOS aarch64 (Apple Silicon) | `azork-aarch64-apple-darwin.tar.gz` |

Each asset is accompanied by a sibling checksum file with the same name plus a
`.sha256` suffix, for example:

```
azork-x86_64-unknown-linux-gnu.tar.gz
azork-x86_64-unknown-linux-gnu.tar.gz.sha256
```

Additional target triples (e.g. Windows) share the same convention and can be
added over time; the target-selection logic is forward-compatible with them.
The `install.sh` bootstrap script (see the top-level [Install](../README.md#install)
section) uses this same naming convention for its one-line install.

## Security & trust model

The updater is built to be safe by default:

- **Verify before trust.** The downloaded archive's SHA-256 is checked against
  the published `.sha256` **before** anything is installed. A mismatch aborts
  with exit code `3` and touches nothing.
- **Anti-rollback.** Only a strictly greater semantic version is treated as an
  update (`is_newer` uses `>` only). Draft and pre-release releases are ignored.
- **TLS only.** All network access uses HTTPS (rustls). The **initial** request
  URL is checked against a host allowlist (`api.github.com`, `github.com`,
  `objects.githubusercontent.com`). GitHub redirects release-asset downloads to
  rotating storage hosts, so redirect targets are **not** re-validated against
  the allowlist; their integrity is instead guaranteed end-to-end by TLS plus the
  fail-closed SHA-256 verification below.
- **No privilege escalation.** The updater writes only to the current
  executable's own path. If that path is not writable it exits `4` and makes no
  changes — it never invokes `sudo` or otherwise elevates.
- **Traversal-safe extraction.** Archive entries with `..`, absolute paths, or
  symlinks are rejected; only the expected `azork` binary is extracted.
- **Atomic self-replace.** The new binary is written to a temporary file in the
  target directory and then atomically renamed over the old one, so an
  interrupted update never leaves a half-written executable. The temporary
  filename mixes the PID, a nanosecond timestamp, and an ASLR-influenced
  stack address through an avalanche hash, so it is not predictable by
  another local process ahead of time (mitigating the CWE-377
  TOCTOU/symlink-plant class of risk on shared multi-user hosts) without
  adding a dependency.
- **Bounded reads (compressed *and* decompressed).** HTTP responses are parsed
  against a fixed schema and capped at 512 MiB of *compressed* download to resist
  malformed payloads. Archive extraction independently caps the *decompressed*
  binary at 512 MiB and aborts (removing any partial file) if the tar entry
  exceeds it, so a checksum-consistent but malicious release cannot exhaust disk
  via a decompression bomb.
- **No telemetry / no secrets.** The updater sends no analytics and logs no
  tokens or signed URLs.
- **User-gated & inert under automation.** The interactive prompt is opt-in and
  time-limited; under CI / non-TTY / subprocess conditions the check is skipped
  entirely.

> Residual trust anchor: releases are trusted via GitHub Releases plus their
> published SHA-256. Artifact signing (e.g. cosign) is a future enhancement and
> not required for the checksum guarantee above.

## Release flow (where updates come from)

Releases are produced automatically by a GitHub Actions workflow
(`.github/workflows/release.yml`) that triggers on any version tag matching
`v*`:

```bash
# Maintainer cuts a release:
git tag v0.3.0
git push origin v0.3.0
```

On that tag the workflow:

1. Builds the `azork` binary for `x86_64-unknown-linux-gnu` (and any additional
   configured targets).
2. Packages the binary into `azork-<triple>.tar.gz`.
3. Computes a matching `azork-<triple>.tar.gz.sha256`.
4. Creates a GitHub Release for the tag and uploads both the archive and its
   checksum as release assets.

Because the asset names encode the target triple and each archive has a sibling
`.sha256`, the client-side `supported_release_target()` and checksum
verification work without any server-side coordination beyond publishing the
assets.

## API reference

For contributors, the updater lives under `src/update/` as a set of small,
single-responsibility modules. Only `network` performs I/O against the network;
the rest are pure and offline-testable.

```
src/update/
├── mod.rs          Public API, constants, and PURE logic:
│                     - GithubRelease / GithubAsset response structs
│                     - GITHUB_REPO const, CURRENT_VERSION (= crate::VERSION)
│                     - UpdateError enum + exit_code() (network=2, checksum=3,
│                       not-writable=4, no-asset=5)
│                     - normalize_tag()  strip leading 'v', parse semver
│                     - is_newer()       strict version comparison
│                     - select_asset()   pick per-target asset
│                     - should_check()   24h cooldown against the cache file
├── check.rs        Startup gate:
│                     - StartupUpdateOutcome { Continue, ExitSuccess }
│                     - classify_skip_reason()  CI/NONINTERACTIVE/AGENT/TTY/flag
│                     - 5s prompt timeout; visible skip-line contract
├── network.rs      The ONLY network module (ureq + rustls):
│                     - fetch latest release JSON, asset bytes, and .sha256
├── checksum.rs     verify_sha256()  fail-closed verification before install
├── archive.rs      traversal-safe .tar.gz extraction of the expected binary
│                     (streamed to disk, capped at 512 MiB decompressed)
├── install.rs      atomic self-replace of the running executable
└── post_install.rs .installed-version stamp + self-heal drift reconciliation
```

Key pure functions (all unit-tested without network access):

| Function | Responsibility |
| -------- | -------------- |
| `normalize_tag(tag: &str) -> Version` | Strip a leading `v` and parse a semantic version. |
| `is_newer(current, latest) -> bool` | True only when `latest > current`. |
| `select_asset(release, target) -> Option<&Asset>` | Choose the asset for this OS/arch. |
| `should_check(now, last_check) -> bool` | Cooldown logic (24h) using the cache timestamp. |
| `verify_sha256(bytes, expected) -> bool` | Fail-closed checksum verification. |
| `classify_skip_reason(env) -> Option<SkipReason>` | Determine whether/why the startup check is skipped. |

### Version source of truth

The running version is exposed as a single constant:

```rust
// src/lib.rs
//
// Single source of truth for the running version. Honours an optional
// AZORK_RELEASE_VERSION override baked in at release time, otherwise falls
// back to the Cargo package version. The `match option_env!(..)` form is
// const-valid (unlike `.unwrap_or(..)`), so this remains a compile-time const.
pub const VERSION: &str = match option_env!("AZORK_RELEASE_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};
```

`update::mod` re-exports this as `CURRENT_VERSION` so version comparison always
uses one authoritative value.

## Troubleshooting

| Symptom | Cause / fix |
| ------- | ----------- |
| "already the latest" but a newer tag exists | The release may be a draft/prerelease (ignored), or the cooldown returned a cached "no". Try `azork update --force`. |
| Checksum verification failed (exit 3) | The download was corrupted or tampered with. Nothing was installed; re-run `azork update`. |
| Install target not writable (exit 4) | The binary lives in a root-owned path. Re-run with appropriate permissions, or reinstall to a user-writable location. |
| No supported release asset (exit 5) | No asset matches your OS/arch yet. Build from source (see [README](../README.md#install)). |
| Startup keeps checking every launch | The cache file (`$XDG_CONFIG_HOME/azork/last_update_check`, default `~/.config/azork/last_update_check`) may be unwritable; check permissions on that directory. |
| Want no checks at all | `export AZORK_NO_UPDATE_CHECK=1`. |
