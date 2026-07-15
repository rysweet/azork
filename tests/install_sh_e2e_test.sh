#!/bin/sh
# tests/install_sh_e2e_test.sh — exercises install.sh's full download,
# checksum-verify, extract, and install pipeline against a stubbed `curl`
# (no real network access), for both the success and checksum-mismatch
# paths.

set -eu

# shellcheck disable=SC1007
SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
# shellcheck disable=SC1007
REPO_ROOT="$(CDPATH= cd -- "${SCRIPT_DIR}/.." && pwd)"
INSTALL_SH="${REPO_ROOT}/install.sh"

WORKDIR="$(mktemp -d)"
FAKE_BIN_DIR="${WORKDIR}/bin"
FIXTURE_DIR="${WORKDIR}/fixtures"
INSTALL_DIR="${WORKDIR}/install"
mkdir -p "$FAKE_BIN_DIR" "$FIXTURE_DIR" "$INSTALL_DIR"
trap 'rm -rf "$WORKDIR"' EXIT INT TERM

failures=0
pass_count=0

pass() {
  echo "PASS: $1"
  pass_count=$((pass_count + 1))
}

fail() {
  echo "FAIL: $1"
  failures=$((failures + 1))
}

# Build a fake release archive containing a trivial "azork" binary.
STAGE="${WORKDIR}/stage"
mkdir -p "$STAGE"
printf '#!/bin/sh\necho "fake azork"\n' > "${STAGE}/azork"
chmod +x "${STAGE}/azork"
ARCHIVE="${FIXTURE_DIR}/azork-x86_64-unknown-linux-gnu.tar.gz"
tar -czf "$ARCHIVE" -C "$STAGE" azork
sha256sum "$ARCHIVE" | awk '{print $1"  azork-x86_64-unknown-linux-gnu.tar.gz"}' \
  > "${FIXTURE_DIR}/azork-x86_64-unknown-linux-gnu.tar.gz.sha256"

# Fake uname always reports linux/x86_64.
cat > "${FAKE_BIN_DIR}/uname" <<'EOF'
#!/bin/sh
case "$1" in
  -s) echo "Linux" ;;
  -m) echo "x86_64" ;;
esac
EOF
chmod +x "${FAKE_BIN_DIR}/uname"

# Fake curl serves local fixture files keyed by the URL's basename, so
# install.sh's real GitHub URL construction is exercised but no network
# call is made.
cat > "${FAKE_BIN_DIR}/curl" <<EOF
#!/bin/sh
# Parses: curl -fsSL <url> -o <output>
url=""
out=""
while [ \$# -gt 0 ]; do
  case "\$1" in
    -o) out="\$2"; shift 2 ;;
    -fsSL) shift ;;
    *) url="\$1"; shift ;;
  esac
done
name="\$(basename "\$url")"
src="${FIXTURE_DIR}/\${name}"
if [ ! -f "\$src" ]; then
  echo "fake curl: no fixture for \$name" >&2
  exit 22
fi
cp "\$src" "\$out"
EOF
chmod +x "${FAKE_BIN_DIR}/curl"

# --- Happy path --------------------------------------------------------------
if PATH="${FAKE_BIN_DIR}:${PATH}" AZORK_INSTALL_DIR="$INSTALL_DIR" sh "$INSTALL_SH" >"${WORKDIR}/happy.log" 2>&1; then
  if [ -x "${INSTALL_DIR}/azork" ] && [ "$("${INSTALL_DIR}/azork")" = "fake azork" ]; then
    pass "happy path installs a working, executable binary"
  else
    fail "happy path: binary missing, not executable, or wrong output"
    cat "${WORKDIR}/happy.log"
  fi
else
  fail "happy path: install.sh exited non-zero"
  cat "${WORKDIR}/happy.log"
fi

# --- Checksum mismatch is rejected -------------------------------------------
rm -rf "${INSTALL_DIR:?}"/*
echo "0000000000000000000000000000000000000000000000000000000000000000  azork-x86_64-unknown-linux-gnu.tar.gz" \
  > "${FIXTURE_DIR}/azork-x86_64-unknown-linux-gnu.tar.gz.sha256"

if PATH="${FAKE_BIN_DIR}:${PATH}" AZORK_INSTALL_DIR="$INSTALL_DIR" sh "$INSTALL_SH" >"${WORKDIR}/bad_checksum.log" 2>&1; then
  fail "checksum mismatch: install.sh should have exited non-zero but succeeded"
else
  if grep -qi "checksum verification failed" "${WORKDIR}/bad_checksum.log"; then
    pass "checksum mismatch is detected and rejected with a clear error"
  else
    fail "checksum mismatch: rejected, but without the expected error message"
    cat "${WORKDIR}/bad_checksum.log"
  fi
fi

if [ -e "${INSTALL_DIR}/azork" ]; then
  fail "checksum mismatch: binary must not be installed"
else
  pass "checksum mismatch: no binary was installed"
fi

# --- Malformed checksum entry (non-hex) is rejected --------------------------
rm -rf "${INSTALL_DIR:?}"/*
echo "not-a-valid-digest  azork-x86_64-unknown-linux-gnu.tar.gz" \
  > "${FIXTURE_DIR}/azork-x86_64-unknown-linux-gnu.tar.gz.sha256"

if PATH="${FAKE_BIN_DIR}:${PATH}" AZORK_INSTALL_DIR="$INSTALL_DIR" sh "$INSTALL_SH" >"${WORKDIR}/malformed.log" 2>&1; then
  fail "malformed checksum: install.sh should have exited non-zero but succeeded"
elif grep -qi "malformed digest" "${WORKDIR}/malformed.log"; then
  pass "malformed checksum entry is detected and rejected"
else
  fail "malformed checksum: rejected, but without the expected error message"
  cat "${WORKDIR}/malformed.log"
fi

# --- Checksum entry naming a different archive is rejected -------------------
rm -rf "${INSTALL_DIR:?}"/*
echo "$(sha256sum "$ARCHIVE" | awk '{print $1}')  some-other-file.tar.gz" \
  > "${FIXTURE_DIR}/azork-x86_64-unknown-linux-gnu.tar.gz.sha256"

if PATH="${FAKE_BIN_DIR}:${PATH}" AZORK_INSTALL_DIR="$INSTALL_DIR" sh "$INSTALL_SH" >"${WORKDIR}/wrong_name.log" 2>&1; then
  fail "mismatched checksum filename: install.sh should have exited non-zero but succeeded"
elif grep -qi "did not contain an entry" "${WORKDIR}/wrong_name.log"; then
  pass "checksum entry naming a different archive is rejected"
else
  fail "mismatched checksum filename: rejected, but without the expected error message"
  cat "${WORKDIR}/wrong_name.log"
fi

# Restore a valid checksum fixture for the remaining scenarios below.
sha256sum "$ARCHIVE" | awk '{print $1"  azork-x86_64-unknown-linux-gnu.tar.gz"}' \
  > "${FIXTURE_DIR}/azork-x86_64-unknown-linux-gnu.tar.gz.sha256"

# --- Download failure surfaces a clear error ---------------------------------
rm -rf "${INSTALL_DIR:?}"/*
mv "$ARCHIVE" "${ARCHIVE}.hidden"

if PATH="${FAKE_BIN_DIR}:${PATH}" AZORK_INSTALL_DIR="$INSTALL_DIR" sh "$INSTALL_SH" >"${WORKDIR}/download_fail.log" 2>&1; then
  fail "download failure: install.sh should have exited non-zero but succeeded"
elif grep -qi "failed to download" "${WORKDIR}/download_fail.log"; then
  pass "download failure is detected and rejected with a clear error"
else
  fail "download failure: rejected, but without the expected error message"
  cat "${WORKDIR}/download_fail.log"
fi
mv "${ARCHIVE}.hidden" "$ARCHIVE"

# --- macOS `shasum` fallback (no `sha256sum` on PATH) ------------------------
rm -rf "${INSTALL_DIR:?}"/*
NO_SHA256SUM_BIN_DIR="${WORKDIR}/bin-no-sha256sum"
mkdir -p "$NO_SHA256SUM_BIN_DIR"
# Build a curated PATH containing everything install.sh needs *except*
# sha256sum, so `command -v sha256sum` genuinely fails and the script must
# take its `shasum -a 256` fallback branch (the one real macOS runners use).
for tool in sh tar gzip awk grep mktemp chmod mv cp mkdir rm dirname basename cat sed printf true shasum; do
  tool_path="$(command -v "$tool" 2>/dev/null || true)"
  [ -n "$tool_path" ] && ln -sf "$tool_path" "${NO_SHA256SUM_BIN_DIR}/${tool}"
done
ln -sf "${FAKE_BIN_DIR}/curl" "${NO_SHA256SUM_BIN_DIR}/curl"
ln -sf "${FAKE_BIN_DIR}/uname" "${NO_SHA256SUM_BIN_DIR}/uname"

if command -v shasum >/dev/null 2>&1; then
  if PATH="$NO_SHA256SUM_BIN_DIR" AZORK_INSTALL_DIR="$INSTALL_DIR" sh "$INSTALL_SH" \
    >"${WORKDIR}/shasum.log" 2>&1; then
    if [ -x "${INSTALL_DIR}/azork" ]; then
      pass "shasum fallback verifies checksum and installs when sha256sum is absent"
    else
      fail "shasum fallback: binary missing after apparent success"
      cat "${WORKDIR}/shasum.log"
    fi
  else
    fail "shasum fallback: install.sh exited non-zero"
    cat "${WORKDIR}/shasum.log"
  fi
else
  echo "SKIP: shasum fallback — no 'shasum' binary available on this system"
fi

echo ""
echo "${pass_count} passed, ${failures} failed"

if [ "$failures" -ne 0 ]; then
  exit 1
fi
