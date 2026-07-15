# Carl — the outside-in-testing (OIT) campaign

"Carl" is the persona name for the recurring **agentic, black-box product
test** that is run against the built `azork` binary (and its companions,
`azork crawl` and the `az azork` extension) the same way a real end user would
use them. Carl is not a unit-test suite and does not read source code to
decide what to test: it drives the *product surfaces* — the interactive REPL,
the CLI flags, Dungeon Crawler Mode, the served map, and the documented
quick-start — and treats any gap between what the docs promise and what the
binary actually does as a bug.

This document describes the campaign as a repeatable process. It complements,
but does not replace, the [`azork-oit`](USAGE.md#outside-in-testing-oit-agent)
binary: `azork-oit` is Carl's *automated* subset (a companion binary that
exercises use cases against a live or mocked Azure backend and writes
`docs/oit-friction-report.md`); a full Carl campaign additionally covers
everything `azork-oit` does not — CLI argument parsing, help/usage text,
error messages and exit codes, the served Dungeon Crawler map, README
fidelity, and manual/adversarial input (garbage, empty, very long, or
malformed commands).

## Scope

A Carl campaign exercises, at minimum:

- **The interactive game.** Launch `azork`, read the intro, try every
  documented verb plus unexpected/garbage/empty/very-long input, `help`,
  `capabilities`, `learn <group>`, intent resolution ("did you mean…"),
  error messages, and quitting.
- **`azork crawl`.** Both the offline mock backend and (when available)
  `--backend az`; `--serve` the map, fetch it over HTTP, and click through
  the interactive room pop-ups (portal links, suggested `az` commands).
- **The CLI surface.** `--help`, `-h`, bare `help`, every subcommand and flag,
  exit codes, unknown-subcommand/unknown-flag handling, environment variables
  (e.g. `AZORK_CACHE_DIR`), and cache persistence across runs.
- **The README quick-start**, followed verbatim, command for command.

Native/unit tests (`cargo test`) do **not** count toward this coverage — they
verify implementation, not the shipped product experience.

## Methodology

Carl treats `azork` as a black box:

1. **Build the real product.** `cargo build --release` and drive
   `./target/release/azork` (and `./target/release/azork-oit` where
   applicable) directly — never the library crate.
2. **Script real interactions.** A pty/expect-style harness drives the
   interactive REPL (stdin/stdout, not the library API); `curl` (or an
   equivalent HTTP client) fetches whatever `azork crawl --serve` binds to.
   Dungeon Crawler Mode binds to an OS-assigned port by default
   (`--port 0`); the harness must parse the bound URL from the process's
   printed startup line rather than assume a fixed port.
3. **Capture ground truth.** Exact stdout, stderr, and exit code for every
   command, compared against what the README and `docs/` promise.
4. **Record every discrepancy** — crash, panic, wrong output, confusing UX,
   broken link, doc-vs-reality mismatch, missing feature the README claims,
   poor error handling, or non-deterministic behavior — as a distinct
   finding with reproduction steps, observed vs. expected behavior, and a
   severity (`critical` / `high` / `medium` / `low`).

## Filing findings

Each confirmed, distinct problem is filed as its own GitHub issue on the
project repository, labeled `carl-oit` (the label is created on first use if
it does not already exist: `gh label create carl-oit`). An issue includes:

- A clear, specific title.
- Exact repro steps/commands.
- Observed vs. expected behavior.
- Severity.

Findings that duplicate an already-open or already-fixed `carl-oit` issue are
not re-filed; a campaign begins by reviewing existing `carl-oit` issues and
`docs/oit-friction-report.md` so it only reports genuinely new or
still-unfixed gaps.

## Fixing findings

Each confirmed bug is fixed through its own independent development
workstream — one focused pull request per distinct fix; unrelated fixes are
never bundled together. Workstreams for fixes that touch different areas of
the code run in parallel, each in its own git worktree off a fresh branch, so
they cannot collide on files or branch state. Every fix is expected to:

- Reference the issue it closes.
- Add a regression test that reproduces the original bug, where the bug is
  reproducible in an automated test (some findings — e.g. wording/UX
  friction — are addressed without a new test).
- Pass `cargo build`, `cargo test`, `cargo clippy --all-targets`, and
  `cargo fmt --check` before a PR is opened.
- Go through the project's normal PR review; a campaign run opens PRs but
  does not merge them itself.

## Re-testing and exit criteria

After a fix lands (merged, or its PR is open and green), the affected product
surface is re-tested from a fresh clone to confirm the fix and check for
regressions. The test → file → fix → re-test loop continues until either:

- A full play/usage session surfaces no new confirmed defects, or
- Every open finding already has an associated pull request.

## Campaign report

Each campaign produces a single consolidated report issue (titled
"Carl OIT campaign report") that links every `carl-oit` issue filed during the
run and every fix PR opened in response, plus a final verification note once
those fixes have been merged and re-tested against `main`.

## Relationship to `azork-oit`

| | Carl campaign | `azork-oit` |
| --- | --- | --- |
| What it drives | The full built product: REPL, CLI, `crawl`, README | The Azure-facing use-case catalog (navigation, examination, scoring, mock deployment, learned capabilities, memory recall) |
| Backend | Mock and/or live `az`, as documented per surface | Mock (`--dry-run`) or live, guardrailed subscription |
| Output | GitHub issues (`carl-oit` label) + a campaign report issue | `docs/oit-friction-report.md` |
| Cadence | Run on demand against a fresh clone of `main` | Run on demand or in CI as a smoke test |

A Carl campaign typically includes running `azork-oit --dry-run` as one of
its checks, since that binary is itself part of the product's tooling surface
and its own output (the friction report) is one of the artifacts Carl
compares against reality.
