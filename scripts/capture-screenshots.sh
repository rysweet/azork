#!/usr/bin/env bash
# Regenerate docs/images/crawl-map-*.png against the local mock-backend
# Dungeon Crawler map.
#
# This is a LOCAL-ONLY, MANUAL documentation tool. It is never invoked by
# `cargo test`, `cargo build`, or any CI workflow — it requires a headless
# Chromium/Chrome binary on the developer's machine and network-free access
# to a server this script starts on 127.0.0.1.
#
# Usage:
#   ./scripts/capture-screenshots.sh
#
# Requires one of: `chromium`, `chromium-browser`, `google-chrome` on PATH.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="$REPO_ROOT/docs/images"
PORT="${CAPTURE_PORT:-8931}"
BASE_URL="http://127.0.0.1:${PORT}"

find_chromium() {
    for bin in chromium chromium-browser google-chrome google-chrome-stable; do
        if command -v "$bin" >/dev/null 2>&1; then
            echo "$bin"
            return 0
        fi
    done
    return 1
}

CHROMIUM_BIN="$(find_chromium || true)"
if [ -z "$CHROMIUM_BIN" ]; then
    echo "No headless Chromium/Chrome binary found on PATH (looked for" \
         "chromium, chromium-browser, google-chrome, google-chrome-stable)." >&2
    echo "Skipping screenshot capture — this script is optional, local-only" \
         "documentation tooling." >&2
    exit 0
fi

echo "Building azork (release) ..."
(cd "$REPO_ROOT" && cargo build --release --quiet)

BIN="$REPO_ROOT/target/release/azork"
if [ ! -x "$BIN" ]; then
    echo "Expected release binary at $BIN — build failed?" >&2
    exit 1
fi

echo "Starting mock Dungeon Crawler server on $BASE_URL ..."
"$BIN" crawl --backend mock --serve --port "$PORT" >/tmp/capture-screenshots-server.log 2>&1 &
SERVER_PID=$!
trap 'kill "$SERVER_PID" >/dev/null 2>&1 || true' EXIT

# Give the embedded server a moment to bind before we start driving it.
for _ in $(seq 1 20); do
    if curl -s -o /dev/null "$BASE_URL/"; then
        break
    fi
    sleep 0.25
done

mkdir -p "$OUT_DIR"

capture() {
    local name="$1"
    local url="$2"
    local width="$3"
    local height="$4"
    echo "Capturing $name ..."
    "$CHROMIUM_BIN" \
        --headless=new \
        --disable-gpu \
        --hide-scrollbars \
        --window-size="${width},${height}" \
        --screenshot="$OUT_DIR/$name" \
        "$url" \
        >/dev/null 2>&1 || echo "  warning: capture of $name failed" >&2
}

capture "crawl-map-overview.png" "$BASE_URL/" 1600 1000
capture "crawl-map-zoom.png" "$BASE_URL/" 900 700
capture "crawl-resource-popup.png" "$BASE_URL/" 900 700

echo "Done. Review $OUT_DIR before committing regenerated screenshots."
