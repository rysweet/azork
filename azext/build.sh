#!/usr/bin/env bash
# Build the azork Azure CLI extension wheel.
#
# Usage: ./azext/build.sh [--bundle-binary]
#
#   --bundle-binary   Copy target/release/azork into the wheel so the extension
#                     is self-contained (no PATH / AZORK_BIN needed).
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$here/.." && pwd)"

bundle=0
if [[ "${1:-}" == "--bundle-binary" ]]; then
    bundle=1
fi

cd "$here"
rm -rf build dist ./*.egg-info azext_azork/bin

if [[ "$bundle" == "1" ]]; then
    bin="$repo_root/target/release/azork"
    if [[ ! -x "$bin" ]]; then
        echo "release binary not found at $bin — run 'cargo build --release' first" >&2
        exit 1
    fi
    mkdir -p azext_azork/bin
    cp "$bin" azext_azork/bin/azork
    echo "bundled $bin into the wheel"
fi

python3 setup.py bdist_wheel
echo
echo "wheel built:"
ls -1 "$here"/dist/*.whl
echo
echo "install with: az extension add --source $(ls "$here"/dist/*.whl) --yes"
