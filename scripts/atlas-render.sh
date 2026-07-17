#!/usr/bin/env bash
# Render every Mermaid (.mmd) and Graphviz (.dot) source under docs/atlas/ to SVG.
# Naming convention: <base>.mmd -> <base>-mermaid.svg ; <base>.dot -> <base>-dot.svg
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ATLAS="$ROOT/docs/atlas"
PUPPETEER_CFG="${PUPPETEER_CFG:-/tmp/puppeteer.json}"
if [ ! -f "$PUPPETEER_CFG" ]; then
  printf '{ "args": ["--no-sandbox", "--disable-setuid-sandbox"] }\n' > "$PUPPETEER_CFG"
fi
fail=0
while IFS= read -r -d '' f; do
  out="${f%.mmd}-mermaid.svg"
  if ! mmdc -i "$f" -o "$out" -p "$PUPPETEER_CFG" >/dev/null 2>/tmp/mmdc.err; then
    echo "MERMAID FAIL: $f"; sed 's/^/    /' /tmp/mmdc.err | tail -5; fail=1
  else echo "ok mermaid: ${out#"$ROOT"/}"; fi
done < <(find "$ATLAS" -name '*.mmd' -print0)
while IFS= read -r -d '' f; do
  out="${f%.dot}-dot.svg"
  if ! dot -Tsvg "$f" -o "$out" 2>/tmp/dot.err; then
    echo "DOT FAIL: $f"; sed 's/^/    /' /tmp/dot.err | tail -5; fail=1
  else echo "ok dot: ${out#"$ROOT"/}"; fi
done < <(find "$ATLAS" -name '*.dot' -print0)
exit $fail
