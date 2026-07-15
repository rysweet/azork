#!/bin/sh
# tests/install_sh_test.sh — POSIX-sh test harness for install.sh's OS/arch
# detection and asset-URL mapping.
#
# Exercises install.sh's --print-url mode under fake `uname` shims for every
# platform in the release matrix, without requiring those platforms or
# performing any network access. Run directly or via CI:
#   sh tests/install_sh_test.sh

set -eu

# shellcheck disable=SC1007  # intentional: `CDPATH=` clears CDPATH before `cd`
SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
# shellcheck disable=SC1007
REPO_ROOT="$(CDPATH= cd -- "${SCRIPT_DIR}/.." && pwd)"
INSTALL_SH="${REPO_ROOT}/install.sh"

FAKE_BIN_DIR="$(mktemp -d)"
trap 'rm -rf "$FAKE_BIN_DIR"' EXIT INT TERM

failures=0
pass_count=0

assert_url() {
  # assert_url <label> <fake-uname-s> <fake-uname-m> <expected-target> [extra-args...]
  label="$1"
  fake_s="$2"
  fake_m="$3"
  expected_target="$4"
  shift 4

  cat > "${FAKE_BIN_DIR}/uname" <<EOF
#!/bin/sh
case "\$1" in
  -s) echo "${fake_s}" ;;
  -m) echo "${fake_m}" ;;
  *) echo "unsupported uname flag: \$1" >&2; exit 1 ;;
esac
EOF
  chmod +x "${FAKE_BIN_DIR}/uname"

  expected="https://github.com/rysweet/azork/releases/latest/download/azork-${expected_target}.tar.gz"
  actual="$(PATH="${FAKE_BIN_DIR}:${PATH}" sh "$INSTALL_SH" --print-url "$@" 2>&1)" || {
    echo "FAIL: ${label} — install.sh exited non-zero: ${actual}"
    failures=$((failures + 1))
    return
  }

  if [ "$actual" = "$expected" ]; then
    echo "PASS: ${label} -> ${actual}"
    pass_count=$((pass_count + 1))
  else
    echo "FAIL: ${label} — expected '${expected}', got '${actual}'"
    failures=$((failures + 1))
  fi
}

assert_unsupported() {
  # assert_unsupported <label> <fake-uname-s> <fake-uname-m>
  label="$1"
  fake_s="$2"
  fake_m="$3"

  cat > "${FAKE_BIN_DIR}/uname" <<EOF
#!/bin/sh
case "\$1" in
  -s) echo "${fake_s}" ;;
  -m) echo "${fake_m}" ;;
  *) echo "unsupported uname flag: \$1" >&2; exit 1 ;;
esac
EOF
  chmod +x "${FAKE_BIN_DIR}/uname"

  if PATH="${FAKE_BIN_DIR}:${PATH}" sh "$INSTALL_SH" --print-url >/dev/null 2>&1; then
    echo "FAIL: ${label} — expected install.sh to reject unsupported platform, but it succeeded"
    failures=$((failures + 1))
  else
    echo "PASS: ${label} — correctly rejected as unsupported"
    pass_count=$((pass_count + 1))
  fi
}

# --- Release matrix coverage (must match .github/workflows/release.yml) ----
assert_url "linux/x86_64"       Linux  x86_64  x86_64-unknown-linux-gnu
assert_url "linux/amd64 alias"  Linux  amd64   x86_64-unknown-linux-gnu
assert_url "linux/aarch64"      Linux  aarch64 aarch64-unknown-linux-gnu
assert_url "linux/arm64 alias"  Linux  arm64   aarch64-unknown-linux-gnu
assert_url "macos/x86_64"       Darwin x86_64  x86_64-apple-darwin
assert_url "macos/arm64"        Darwin arm64   aarch64-apple-darwin
assert_url "macos/aarch64 alias" Darwin aarch64 aarch64-apple-darwin

# --- Unsupported platforms --------------------------------------------------
assert_unsupported "windows (uname unavailable pattern)" MINGW64_NT x86_64
assert_unsupported "unknown arch"                        Linux      riscv64

# --- Version pinning ---------------------------------------------------------
label="version pin"
cat > "${FAKE_BIN_DIR}/uname" <<'EOF'
#!/bin/sh
case "$1" in
  -s) echo "Linux" ;;
  -m) echo "x86_64" ;;
esac
EOF
chmod +x "${FAKE_BIN_DIR}/uname"
expected="https://github.com/rysweet/azork/releases/download/v1.2.3/azork-x86_64-unknown-linux-gnu.tar.gz"
actual="$(PATH="${FAKE_BIN_DIR}:${PATH}" sh "$INSTALL_SH" --print-url --version v1.2.3)"
if [ "$actual" = "$expected" ]; then
  echo "PASS: ${label} -> ${actual}"
  pass_count=$((pass_count + 1))
else
  echo "FAIL: ${label} — expected '${expected}', got '${actual}'"
  failures=$((failures + 1))
fi

label="AZORK_VERSION env var"
actual="$(PATH="${FAKE_BIN_DIR}:${PATH}" AZORK_VERSION=v9.9.9 sh "$INSTALL_SH" --print-url)"
expected="https://github.com/rysweet/azork/releases/download/v9.9.9/azork-x86_64-unknown-linux-gnu.tar.gz"
if [ "$actual" = "$expected" ]; then
  echo "PASS: ${label} -> ${actual}"
  pass_count=$((pass_count + 1))
else
  echo "FAIL: ${label} — expected '${expected}', got '${actual}'"
  failures=$((failures + 1))
fi

echo ""
echo "${pass_count} passed, ${failures} failed"

if [ "$failures" -ne 0 ]; then
  exit 1
fi
