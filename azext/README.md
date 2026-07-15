# AzZork as an Azure CLI extension (`az azork`)

`azext_azork` is a **thin** Azure CLI extension: it exposes AzZork under the
`az azork` command group by shelling out to the compiled `azork` Rust binary.
All the real work — dynamic capability derivation from `az`, the ladybug graph
memory, and agentic intent resolution — lives in the binary. This wheel only
wires it into the `az` command tree.

## Commands

| Command | What it does |
| --- | --- |
| `az azork play [--backend mock\|az]` | Launch the interactive REPL adventure, attached to your terminal. |
| `az azork run --commands "look; score" [--backend mock\|az]` | Run one or more commands non-interactively and print the narration. Separate commands with `;` or newlines. |
| `az azork version` | Print the located binary path and its version banner. |

`--backend az` explores your **live** subscription (read-only navigation);
the default `mock` backend is fully offline.

## Locating the binary

The extension finds `azork` in this order:

1. `AZORK_BIN` environment variable (an explicit path to the executable).
2. A binary bundled inside the wheel at `azext_azork/bin/azork` (optional).
3. `azork` on your `PATH`.

If none is found, the command fails with guidance to build the binary.

## Build the binary

```bash
# in the azork repo root
cargo build --release
export AZORK_BIN="$PWD/target/release/azork"   # or add it to PATH
```

## Build the wheel

```bash
cd azext
python3 setup.py bdist_wheel
# -> azext/dist/azork-<version>-py3-none-any.whl
```

Or use the helper:

```bash
./azext/build.sh
```

## Install into `az`

```bash
az extension add --source azext/dist/azork-0.2.0-py3-none-any.whl --yes
az azork version
az azork run --commands "look; score"
```

Remove it again with:

```bash
az extension remove --name azork
```

## Notes

- Requires Azure CLI core >= 2.40.0 (declared in `azext_metadata.json`); tested
  against az 2.83.0.
- The extension is marked **preview**.
- Pure-Python, **zero third-party install requirements** — it only uses the
  Azure CLI's own SDK, which is already present wherever `az` runs.
- Licensed MIT, same as azork.
