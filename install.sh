#!/bin/sh
# install.sh — download and install a prebuilt `azork` release binary.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/rysweet/azork/main/install.sh | sh
#   curl -fsSL https://raw.githubusercontent.com/rysweet/azork/main/install.sh | sh -s -- --version v0.5.0
#
# Environment variables:
#   AZORK_VERSION      Pin to a specific release tag (e.g. v0.5.0). Defaults to "latest".
#   AZORK_INSTALL_DIR  Directory to install the binary into. Defaults to "$HOME/.local/bin",
#                      falling back to "/usr/local/bin" if that is not writable.
#
# Flags:
#   --version <tag>    Same as AZORK_VERSION.
#   --print-url        Print the resolved download URL and exit (no download/install).
#   --dry-run          Resolve OS/arch/target/URL and print a summary; do not download/install.
#   -h, --help         Show this help.
#
# This script mirrors the asset naming scheme published by
# .github/workflows/release.yml: azork-<target-triple>.tar.gz plus a
# sibling azork-<target-triple>.tar.gz.sha256 checksum file, one-to-one
# with src/update/mod.rs::asset_name_for_target().

set -eu

REPO="rysweet/azork"
BINARY_NAME="azork"
VERSION="${AZORK_VERSION:-latest}"
INSTALL_DIR="${AZORK_INSTALL_DIR:-}"
PRINT_URL_ONLY=0
DRY_RUN=0

log() {
  printf '%s\n' "$*" >&2
}

die() {
  log "error: $*"
  exit 1
}

show_help() {
  sed -n '2,20p' "$0" | sed 's/^# \{0,1\}//'
}

# ---------------------------------------------------------------------------
# Argument parsing
# ---------------------------------------------------------------------------
while [ $# -gt 0 ]; do
  case "$1" in
    --version)
      [ $# -ge 2 ] || die "--version requires an argument"
      VERSION="$2"
      shift 2
      ;;
    --version=*)
      VERSION="${1#--version=}"
      shift
      ;;
    --print-url)
      PRINT_URL_ONLY=1
      shift
      ;;
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    -h | --help)
      show_help
      exit 0
      ;;
    *)
      die "unknown argument: $1 (see --help)"
      ;;
  esac
done

# ---------------------------------------------------------------------------
# OS/arch detection -> Rust target triple
# ---------------------------------------------------------------------------
detect_target() {
  os="$(uname -s)"
  arch="$(uname -m)"

  case "$os" in
    Linux)
      case "$arch" in
        x86_64 | amd64) echo "x86_64-unknown-linux-gnu" ;;
        aarch64 | arm64) echo "aarch64-unknown-linux-gnu" ;;
        *) return 1 ;;
      esac
      ;;
    Darwin)
      case "$arch" in
        x86_64 | amd64) echo "x86_64-apple-darwin" ;;
        arm64 | aarch64) echo "aarch64-apple-darwin" ;;
        *) return 1 ;;
      esac
      ;;
    *)
      return 1
      ;;
  esac
}

TARGET="$(detect_target)" || die "unsupported platform: $(uname -s)/$(uname -m). See https://github.com/${REPO}/releases for manual downloads (e.g. Windows)."

ARCHIVE_NAME="${BINARY_NAME}-${TARGET}.tar.gz"
CHECKSUM_NAME="${ARCHIVE_NAME}.sha256"

if [ "$VERSION" = "latest" ]; then
  BASE_URL="https://github.com/${REPO}/releases/latest/download"
else
  BASE_URL="https://github.com/${REPO}/releases/download/${VERSION}"
fi

ARCHIVE_URL="${BASE_URL}/${ARCHIVE_NAME}"
CHECKSUM_URL="${BASE_URL}/${CHECKSUM_NAME}"

if [ "$PRINT_URL_ONLY" -eq 1 ]; then
  echo "$ARCHIVE_URL"
  exit 0
fi

if [ "$DRY_RUN" -eq 1 ]; then
  log "os/arch:        $(uname -s)/$(uname -m)"
  log "target triple:  $TARGET"
  log "version:        $VERSION"
  log "archive url:    $ARCHIVE_URL"
  log "checksum url:   $CHECKSUM_URL"
  exit 0
fi

# ---------------------------------------------------------------------------
# Download helpers
# ---------------------------------------------------------------------------
fetch() {
  # fetch <url> <output-path>
  if command -v curl >/dev/null 2>&1; then
    # Require HTTPS for both the initial request and any redirect hop, so a
    # compromised or misconfigured redirect can never downgrade the transfer
    # to plaintext HTTP.
    curl -fsSL --proto '=https' --proto-redir '=https' "$1" -o "$2"
  elif command -v wget >/dev/null 2>&1; then
    wget -q --https-only "$1" -O "$2"
  else
    die "neither curl nor wget is available; please install one and retry"
  fi
}

sha256_of() {
  # sha256_of <path> -> prints the hex digest
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    die "neither sha256sum nor shasum is available; cannot verify checksum"
  fi
}

# ---------------------------------------------------------------------------
# Download + verify + install
# ---------------------------------------------------------------------------
WORKDIR="$(mktemp -d)"
trap 'rm -rf "$WORKDIR"' EXIT INT TERM

log "Downloading ${ARCHIVE_NAME} (${VERSION})..."
fetch "$ARCHIVE_URL" "${WORKDIR}/${ARCHIVE_NAME}" \
  || die "failed to download ${ARCHIVE_URL}. Does a release for '${VERSION}' exist with an asset for ${TARGET}?"

log "Downloading checksum..."
fetch "$CHECKSUM_URL" "${WORKDIR}/${CHECKSUM_NAME}" \
  || die "failed to download checksum ${CHECKSUM_URL}"

EXPECTED_DIGEST="$(awk -v want="$ARCHIVE_NAME" '$2 == want || $2 == "*" want { print $1; exit }' "${WORKDIR}/${CHECKSUM_NAME}")"
[ -n "$EXPECTED_DIGEST" ] || die "checksum file ${CHECKSUM_NAME} did not contain an entry for ${ARCHIVE_NAME}"

echo "$EXPECTED_DIGEST" | grep -Eq '^[0-9a-fA-F]{64}$' \
  || die "checksum file ${CHECKSUM_NAME} contained a malformed digest for ${ARCHIVE_NAME}: ${EXPECTED_DIGEST}"

ACTUAL_DIGEST="$(sha256_of "${WORKDIR}/${ARCHIVE_NAME}")"

if [ "$EXPECTED_DIGEST" != "$ACTUAL_DIGEST" ]; then
  die "checksum verification failed for ${ARCHIVE_NAME}
  expected: ${EXPECTED_DIGEST}
  actual:   ${ACTUAL_DIGEST}"
fi
log "Checksum verified."

log "Extracting..."
tar -xzf "${WORKDIR}/${ARCHIVE_NAME}" -C "$WORKDIR" \
  || die "failed to extract ${ARCHIVE_NAME}"

[ -f "${WORKDIR}/${BINARY_NAME}" ] || die "archive did not contain expected binary '${BINARY_NAME}'"

# ---------------------------------------------------------------------------
# Choose install directory
# ---------------------------------------------------------------------------
if [ -z "$INSTALL_DIR" ]; then
  DEFAULT_DIR="${HOME}/.local/bin"
  mkdir -p "$DEFAULT_DIR" 2>/dev/null || true
  if [ -d "$DEFAULT_DIR" ] && [ -w "$DEFAULT_DIR" ]; then
    INSTALL_DIR="$DEFAULT_DIR"
  elif [ -w "/usr/local/bin" ]; then
    INSTALL_DIR="/usr/local/bin"
  else
    INSTALL_DIR="$DEFAULT_DIR"
    mkdir -p "$INSTALL_DIR" || die "could not create install directory ${INSTALL_DIR}"
  fi
fi

mkdir -p "$INSTALL_DIR" || die "could not create install directory ${INSTALL_DIR}"
[ -w "$INSTALL_DIR" ] || die "install directory ${INSTALL_DIR} is not writable. Set AZORK_INSTALL_DIR to a writable path, or re-run with sudo."

DEST="${INSTALL_DIR}/${BINARY_NAME}"
TMP_DEST="$(mktemp "${INSTALL_DIR}/.${BINARY_NAME}.XXXXXX" 2>/dev/null)" || TMP_DEST="${DEST}.tmp.$$"
cp "${WORKDIR}/${BINARY_NAME}" "$TMP_DEST" || die "failed to stage binary in ${INSTALL_DIR}"
chmod +x "$TMP_DEST" || die "failed to chmod +x ${TMP_DEST}"
mv "$TMP_DEST" "$DEST" || die "failed to install binary to ${DEST}"

log ""
log "azork installed to ${DEST}"

case ":$PATH:" in
  *":${INSTALL_DIR}:"*) ;;
  *)
    log ""
    log "Note: ${INSTALL_DIR} is not on your PATH."
    log "Add it, e.g.:"
    log "  export PATH=\"${INSTALL_DIR}:\$PATH\""
    ;;
esac

log ""
log "Run 'azork --help' to get started."
log "To uninstall: rm ${DEST}"
