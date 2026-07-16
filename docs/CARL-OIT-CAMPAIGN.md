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

## Harness scripts

A Carl run is driven by a small set of black-box harness scripts. They are
**never committed to the repository** — each run materializes them into a
fresh external scratch directory (`$(mktemp -d /tmp/carl-XXXX)`) so that no
tooling artifact (logs, captured HTML, expect scripts, node/python cache
dirs) ever lands inside the git worktree and trips the artifact guard at a
workflow checkpoint. The four scripts are:

- **`repl_driver.sh`** — a pty/expect-style driver for the interactive game.
  It spawns `./target/release/azork`, feeds it a scripted sequence of lines
  (documented verbs, `help`, `capabilities`, `learn <group>`, deliberately
  garbled verbs to probe intent resolution, empty lines, a multi-kilobyte
  line to probe input-length handling, and `quit`), and records every byte of
  stdout/stderr plus the process exit code to a timestamped log file in the
  scratch directory. It uses idle-detection (no output for N seconds after a
  prompt) rather than a fixed wall-clock timeout to decide when the product
  has finished responding to a line, so slow-but-correct responses are never
  misclassified as hangs.
- **`crawl_probe.sh`** — launches `azork crawl --serve --port 0` against the
  offline mock backend (and, when `AZORK_TEST_LIVE_AZURE=1` is set in the
  environment, additionally against `--backend az`), parses the bound URL
  from the process's printed startup line, `curl`s the served map and its
  JSON/HTML room endpoints, and asserts on the adaptive room-sizing
  invariant (every room's grid allocation is `>= 1 cell per resource it
  contains`, with no visual overflow) and on the presence of the dungeon
  decorations (border, torches, treasure chest, dragon marker) in the served
  markup. It also drives `--mock-size` end to end: it runs the built-in
  presets (`small`, `medium`, `large`) and at least one explicit
  `COUNTxPER_GROUP:seed` spec, and separately re-runs the same scenario via
  the `AZORK_MOCK_SIZE` / `AZORK_MOCK_RGS` / `AZORK_MOCK_RESOURCES_PER_RG` /
  `AZORK_MOCK_SEED` environment variables to confirm the CLI flag and the
  env vars agree on the same synthetic tenant shape for a given seed
  (determinism check).
- **`readme_verbatim.sh`** — copies every shell command block out of
  `README.md` (including any install/quick-start steps) into a temp
  directory, executes each one exactly as printed (no substituted or
  additional flags), and diffs the observed output/exit code against what
  the surrounding README prose claims will happen.
- **`az_backend_min.sh`** — the minimal live-Azure validation path (see
  "Minimal live-Azure validation" below). Guarded by an explicit opt-in
  environment variable so it is never run accidentally; it creates exactly
  one cheap, tagged, azork-owned resource, exercises the relevant `azork
  crawl --backend az` / `azork-oit` path against it, and tears it down
  before exiting, even on failure (via a trap).

All four scripts exit non-zero on any captured discrepancy so a campaign run
can be scripted as `script.sh || record_finding.sh`, and all four write their
raw captures (logs, HTML, JSON) under the same scratch directory so a finding
can always cite the exact artifact that demonstrates it.

## Findings ledger

Within a single campaign run, confirmed and candidate findings are tracked in
a structured ledger (a local table, not committed to the repository) with
one row per observation:

| column | meaning |
| --- | --- |
| `id` | short kebab-case identifier for the finding |
| `surface` | which product surface it was found on (`repl`, `crawl`, `cli`, `readme`, `az-backend`) |
| `repro` | exact command(s)/input(s) that trigger it |
| `observed` | captured stdout/stderr/exit code (or a pointer to the harness log) |
| `expected` | what the README/docs/help text promises instead |
| `severity` | `critical` / `high` / `medium` / `low` |
| `occurrences` | how many times it has been reproduced |
| `dup_of` | an existing `carl-oit` issue number, if this duplicates one, else empty |
| `status` | `candidate`, `confirmed`, `filed`, `fix-in-progress`, `fixed`, `wont-fix` |
| `issue_number` / `pr_number` | links once filed/fixed |

A finding is only promoted from `candidate` to `confirmed` — and therefore
eligible to become a GitHub issue — once it has been reproduced **at least
twice** with a captured stdout/stderr/exit code each time, and only if
`dup_of` is empty after checking it against the known-issue list below.
Single-occurrence anomalies stay `candidate` and are noted in the campaign
report's "not yet confirmed" section rather than filed.

## Duplicate-check policy

Before filing any new issue, a campaign run checks the finding against
**all** existing issues in the project repository — regardless of label or
open/closed state — not merely those already carrying the `carl-oit` label.
Many previously-triaged issues predate the `carl-oit` label and instead
carry unrelated labels (`bug`, `enhancement`) or no label at all, so a
label-scoped search alone would miss them. In addition to this general
all-issue search, this fixed set of previously-triaged issues is always
checked explicitly so the campaign never re-files a known problem under a
new number:

- **#8** (`az` enumeration is slow / N+1) — labeled `bug`, **already closed**
  as fixed. A campaign run does not reopen or re-file it; it re-verifies the
  fix still holds against current `main` (see "Live-Azure validation"
  below) and adds a fresh timing comment. It is only reopened if a
  regression is observed with new evidence.
- **#9** (tenant-policy storage create) — open; referenced, not re-filed.
- **#10** / **#11** (creation-intent disambiguation) — open; referenced, not
  re-filed.
- **#16** (update TOCTOU) — open; referenced, not re-filed.
- **#45** (sibling secret-detector audit) — open; referenced, not re-filed.

This campaign's own tracking work is filed under **#66** (the umbrella
issue for the OIT harness/tooling build), which the campaign report should
cross-reference as its parent issue rather than treat as a `carl-oit`
finding.

## Severity taxonomy (applied consistently)

- **Critical** — crash, panic, or data loss.
- **High** — a documented feature is broken, or output is simply wrong.
- **Medium** — confusing UX or a misleading error message; the feature
  technically works.
- **Low** — cosmetic issue or a documentation nit (typo, stale screenshot,
  broken link).

## Minimal live-Azure validation

When a finding can only be confirmed against a real subscription (rather
than the mock backend), a campaign performs the smallest possible live check
rather than skipping the surface entirely:

1. Create exactly one cheap resource group + `Standard_LRS` storage account,
   named with the `azork-oit-` prefix and tagged `owner=azork-oit`,
   `azork-oit=1`, and a `ttl`, so ownership is unambiguous before any
   mutation is attempted — consistent with the ownership-gated mutation
   rules in `src/oit/guardrails.rs`.
2. Exercise the target code path (`azork crawl --backend az` and/or the
   relevant `azork-oit` use case) against that one resource.
3. Tear the resource down via azork's own OIT teardown path before the
   campaign continues, regardless of whether the check passed or failed.
4. Never mutate or delete any resource azork did not create itself, and stay
   under azork's `COST_CAP_USD` guardrail at all times — no bypassing or
   weakening of `src/oit/guardrails.rs` or `src/bin/azork-oit.rs`.

Large-tenant enumeration-performance checks (e.g. confirming a fix for
"#8-style" N+1 slowness) are instead validated against a `--mock-size
large`/`huge` synthetic tenant, which reproduces the same fan-out shape
without any live-Azure cost or risk.

## Fix-workstream dispatch

Each confirmed, filed finding is fixed by launching an independent
dev-orchestrator workstream as its own subprocess, in its own fresh clone
under `/home/azureuser/src/<bugname>-fix` (never under any other user's home
directory), so parallel fixes for unrelated findings can never collide on
files, branch state, or build artifacts:

```
cd /home/azureuser/src/<bugname>-fix && \
  env -u CLAUDECODE AMPLIHACK_HOME=/home/azureuser/.amplihack \
  amplihack recipe run \
  /home/azureuser/.amplihack/amplifier-bundle/recipes/smart-orchestrator.yaml \
  -c task_description="Fix <issue-number>: <repro + observed/expected + artifact-hygiene rules>" \
  -c repo_path="." \
  -c force_single_workstream="true" \
  --verbose
```

Workstreams are launched detached (`nohup ... &`) so Carl's own test → file →
fix → re-test loop is never blocked waiting on a fix to land; their state is
polled by checking PR/branch status, never by sleeping on a fixed timeout.
Each workstream's own task description carries forward the same artifact
hygiene constraints (no `node_modules/`/`.pytest_cache` left in its worktree)
so a fix workstream cannot itself trip the artifact guard at its own
checkpoint. Once a fix workstream's PR is open, its `target/` build directory
is removed to reclaim disk (never its source), and the campaign re-tests the
affected surface from a fresh clone once the fix lands or the PR is green.

## Campaign report format

The consolidated report issue's body is a markdown checklist, one line per
finding, so campaign status is readable at a glance:

```
- [ ] #71 REPL: empty-line input after `learn` panics — fix: #72 (open)
- [x] #68 crawl --serve: adaptive room sizing overflows at >40 resources/RG — fix: #70 (merged)
- [ ] #8 (verified fixed, not reopened): #61 parallel enumeration confirmed via --mock-size large timing
```

Each line links the finding issue and, once one exists, its fix PR, plus a
short status word (`open`, `merged-pending-review`, `merged`, `wont-fix`). No
additional labels beyond `carl-oit` are introduced for report bookkeeping.
The report issue itself references **#66** as its parent/umbrella issue
(the harness-build task that produced this campaign tooling), not as a
`carl-oit` finding.

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
