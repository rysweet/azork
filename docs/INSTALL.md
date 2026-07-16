# Installing AzZork

This is the full reference for installing `azork`. For the short version, see
the [README → Install](../README.md#install) section.

- New here? Start with the [Usage guide](USAGE.md).
- Already installed and want to stay current? See the [Self-Update guide](UPDATING.md).
- Building or cutting releases yourself? See the [Development guide](DEVELOPMENT.md).

## Quick install (recommended once releases exist)

> **⚠️ No GitHub Release has been published yet.** Until one is cut, the
> command below (and `azork update`) will fail with a 404 / "no published
> release found for rysweet/azork yet". Use
> [Build from source](#build-from-source-with-cargo-no-cratesio-publish-required)
> instead until a release is available.

```bash
curl -fsSL https://raw.githubusercontent.com/rysweet/azork/main/install.sh | sh
```

This downloads `install.sh` from the `main` branch and runs it with `sh`. The
script is POSIX `/bin/sh` compatible — it does not require `bash`, and it does
not require a Rust toolchain. It performs, in order:

1. **Detect OS/arch** via `uname -s` / `uname -m` and map it to a Rust target
   triple (see [Supported platforms](#supported-platforms)).
2. **Resolve the download URL** for the latest GitHub Release (or a pinned
   version — see [Pinning a version](#pinning-a-version)):
   `https://github.com/rysweet/azork/releases/<version>/download/azork-<target>.tar.gz`
3. **Download** the archive and its sibling `azork-<target>.tar.gz.sha256`
   checksum file over HTTPS (HTTPS is enforced even across redirects).
4. **Verify** the archive's SHA-256 digest against the checksum file. If they
   don't match, the script exits with a non-zero status and installs nothing.
5. **Extract** the `azork` binary and install it into the target directory
   (see [Install location](#install-location)), setting the executable bit.
6. **Report** the install path, whether that path is already on `PATH`, and
   how to run and uninstall `azork`.

Any failure (unsupported platform, network error, missing/malformed checksum,
checksum mismatch, unwritable install directory) causes the script to print an
`error: ...` message to stderr and exit non-zero — nothing is left partially
installed.

## Supported platforms

The installer supports the same target matrix published by the release
workflow:

| OS      | Arch             | Rust target triple           |
|---------|------------------|-------------------------------|
| Linux   | x86_64 / amd64   | `x86_64-unknown-linux-gnu`   |
| Linux   | aarch64 / arm64  | `aarch64-unknown-linux-gnu`  |
| macOS   | x86_64 / amd64   | `x86_64-apple-darwin`        |
| macOS   | arm64 / aarch64  | `aarch64-apple-darwin`       |

Windows is **not** supported by `install.sh` (there is no `install.ps1`).
Windows users should download the `x86_64-pc-windows-msvc` asset manually from
the [Releases page](https://github.com/rysweet/azork/releases), or build from
source with `cargo install --git https://github.com/rysweet/azork`.

On any other OS/arch combination, the script fails with:

```
error: unsupported platform: <os>/<arch>. See https://github.com/rysweet/azork/releases for manual downloads (e.g. Windows).
```

## Install location

By default the binary is installed to `$HOME/.local/bin`. If that directory
cannot be created or is not writable, the script falls back to
`/usr/local/bin` (when writable).

Override the destination with `AZORK_INSTALL_DIR`:

```bash
curl -fsSL https://raw.githubusercontent.com/rysweet/azork/main/install.sh \
  | AZORK_INSTALL_DIR=/usr/local/bin sh
```

If the resolved install directory is not on your `PATH`, the script prints a
reminder with the exact `export PATH=...` line to add to your shell profile.

## Pinning a version

By default the installer fetches the **latest** GitHub Release. To install a
specific tagged version instead, set `AZORK_VERSION` or pass `--version`:

```bash
# via environment variable
curl -fsSL https://raw.githubusercontent.com/rysweet/azork/main/install.sh \
  | AZORK_VERSION=v0.5.0 sh

# via flag (note the `sh -s --` separator so the flag reaches the script)
curl -fsSL https://raw.githubusercontent.com/rysweet/azork/main/install.sh \
  | sh -s -- --version v0.5.0
```

`--version` and `AZORK_VERSION` are equivalent; the flag takes precedence if
both are somehow supplied. Version strings must match an existing release tag
(e.g. `v0.5.0`), including the `v` prefix.

## Previewing without installing

Two flags let you inspect what the installer *would* do without touching your
filesystem or downloading anything beyond `install.sh` itself:

```bash
# Print the resolved OS, arch, target triple, version, and both URLs.
curl -fsSL https://raw.githubusercontent.com/rysweet/azork/main/install.sh \
  | sh -s -- --dry-run

# Print only the resolved archive download URL, one line, nothing else.
curl -fsSL https://raw.githubusercontent.com/rysweet/azork/main/install.sh \
  | sh -s -- --print-url
```

`--print-url` is useful for scripting (e.g. `curl -fsSL "$(sh install.sh --print-url)"`)
and is exercised directly by the project's own test suite as a way to unit
test OS/arch → asset-name mapping without a real network call.

## Getting help

```bash
curl -fsSL https://raw.githubusercontent.com/rysweet/azork/main/install.sh | sh -s -- --help
```

or, if you've already saved the script locally:

```bash
sh install.sh --help
```

This prints the full flag and environment-variable reference embedded as a
comment header at the top of `install.sh`.

## Checksum verification, in detail

Every release asset `azork-<target>.tar.gz` is published alongside a
`azork-<target>.tar.gz.sha256` file containing a standard `sha256sum`-format
line (`<hex-digest>  <filename>`). The installer:

1. Downloads both files into a private temporary directory (`mktemp -d`,
   cleaned up on exit via `trap ... EXIT INT TERM`, even on failure).
2. Parses the checksum file with `awk`, matching the exact archive filename
   (or a bare `*`-prefixed entry) to extract the expected digest.
3. Validates the expected digest is 64 hex characters — rejecting a
   truncated, corrupted, or malformed checksum file outright.
4. Computes the actual digest of the downloaded archive using `sha256sum`
   (or `shasum -a 256` if `sha256sum` is unavailable, e.g. some macOS setups).
5. Compares the two digests byte-for-byte. Any mismatch aborts installation
   with both digests printed, and no file is copied into the install
   directory.

This is the same checksum scheme used by azork's own `azork update`
self-updater (see [Self-Update guide](UPDATING.md)) — both consume assets
produced by the same `release.yml` workflow, so the guarantees are identical
whether you install for the first time or update in place.

## Alternative install methods

### Download a release binary manually

Visit the [Releases page](https://github.com/rysweet/azork/releases), download
the archive matching your platform (e.g. `azork-x86_64-unknown-linux-gnu.tar.gz`)
and its `.sha256` file, verify manually, then extract and move the binary onto
your `PATH`:

```bash
tar xzf azork-x86_64-unknown-linux-gnu.tar.gz
sha256sum -c azork-x86_64-unknown-linux-gnu.tar.gz.sha256
chmod +x azork
mv azork ~/.local/bin/   # or any directory on your PATH
```

### Build from source with Cargo (no crates.io publish required)

`azork` is not published to crates.io. Install directly from GitHub with a
Rust toolchain already installed:

```bash
cargo install --git https://github.com/rysweet/azork
```

### Clone and build

```bash
git clone https://github.com/rysweet/azork.git
cd azork
cargo build --release
./target/release/azork --help
```

## Uninstalling

Since the installer only ever places a single self-contained binary, removing
it is a one-liner:

```bash
rm "$(command -v azork)"
```

or, if you know the install directory:

```bash
rm ~/.local/bin/azork   # or wherever AZORK_INSTALL_DIR pointed
```

There are no config files, daemons, or shell integrations left behind by the
installer itself.

## Troubleshooting

| Symptom | Cause | Fix |
|---|---|---|
| `error: unsupported platform: ...` | Running on Windows, or an unrecognized `uname` output | Use the manual download or `cargo install` methods for your platform |
| `error: failed to download ...` | No release exists for the requested version, or no network | Check `--version`/`AZORK_VERSION`, confirm connectivity, browse the [Releases page](https://github.com/rysweet/azork/releases) |
| `error: checksum verification failed ...` | Corrupted download, or a tampered/incomplete mirror | Re-run the installer; if it persists, file an issue — do not run the binary |
| `error: neither curl nor wget is available ...` | Minimal container/base image | Install `curl` (preferred) or `wget`, then re-run |
| `error: neither sha256sum nor shasum is available ...` | Minimal image missing coreutils | Install `coreutils` (Linux) — macOS ships `shasum` by default |
| `<install dir> is not on your PATH` | Default `~/.local/bin` isn't in `PATH` on this system | Add the printed `export PATH=...` line to your shell profile |
| `error: install directory ... is not writable` | `AZORK_INSTALL_DIR` (or the `/usr/local/bin` fallback) requires elevated permissions | Set `AZORK_INSTALL_DIR` to a writable path, or re-run with `sudo sh -c 'curl ... \| sh'` |

## How this relates to release engineering

The installer's asset expectations are produced by
`.github/workflows/release.yml` on every `v*` tag push: it builds `azork` for
each target in [Supported platforms](#supported-platforms) (plus
`x86_64-pc-windows-msvc` for manual download), packages each binary as
`azork-<target>.tar.gz`, computes a `sha256` checksum file per archive, and
publishes both as assets on a GitHub Release. See the
[Development guide → Cutting a release](DEVELOPMENT.md#cutting-a-release) for
the maintainer-facing side of this pipeline.
