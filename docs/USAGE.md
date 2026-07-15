# AzZork Usage Guide

AzZork is a text-adventure REPL that reimagines the Azure control plane as a
Zork-style dungeon. This guide covers everything you can do at the `az>` prompt.

- New here? Start with the [Tutorial](TUTORIAL.md).
- Configuring backends and environment? See the [Configuration reference](CONFIGURATION.md).
- Embedding or extending the engine? See the [API / module reference](API.md).

## Starting the game

```bash
# Offline mock dungeon (default — no Azure credentials required)
azork

# Explore your real subscription via the installed `az` CLI
azork --backend az
# or
AZORK_BACKEND=az azork
```

On launch AzZork prints an ASCII banner, a status line showing the active
backend and subscription, and a description of your starting room:

```
[backend: mock (offline) | subscription: Contoso-Dev (mock)]

== landing-rg (eastus) ==
The West Landing Zone. Cables snake overhead and a subscription portal hums
softly. This resource group is monitored and safe.
You see:
  - portal (Microsoft.Portal/dashboards)
Exits: down, east, north
```

## The prompt

Every turn begins with the prompt:

```
az>
```

Type a command and press Enter. Input is **case-insensitive** and forgiving:
filler words (`the`, `a`, `an`, `at`, `to`, `into`, `on`, `my`) are ignored, so
`examine the storage account` and `x storage account` mean the same thing.

Pressing Ctrl-D (EOF) at any prompt ends the session gracefully.

## Command reference

| Command | Aliases | What it does | Azure analogue |
| --- | --- | --- | --- |
| `look` | `l` | Describe the current room: resources present and available exits. | `az resource list` |
| `examine <name>` | `x`, `inspect`, `show` | Inspect one resource: type, security status, cost, hazards. | `az resource show` |
| `go <dir>` | `move`, `walk`, or a bare direction | Move between rooms. | Navigate resource groups / regions / subscriptions |
| `take <name>` | `get`, `grab`, `acquire` | Adopt a resource into your inventory (asks **y/N**). | `az resource create` / adopt |
| `drop <name>` | `delete`, `release`, `rm` | Delete a resource (destructive, asks **y/N**). | `az resource delete` |
| `lock <name>` | `secure` | Harden a resource: management lock + private + encrypted. | Enable protections / RBAC lock |
| `unlock <name>` | `unward`, `unsecure` | Remove a management lock so the resource can change or be deleted again. | `az lock delete` |
| `resize <name>` | `right-size`, `rightsize`, `scale`, `downsize` | Right-size a resource, roughly halving its monthly cost. | Change SKU / scale down a tier |
| `monitor` | `light` | Enable monitoring in the current room, banishing the Grue. | Enable diagnostics / Azure Monitor |
| `cast deploy [template]` | `deploy [template]` | Cast a deployment spell (mock bicep/ARM). | `az deployment group create` |
| `inventory` | `i`, `inv` | List the resources you are carrying. | — |
| `score` | — | Report your governance posture (0–100) and rank. | Governance posture |
| `learn <group>` | `discover`, `study` | Introspect `az <group> --help` and grow AzZork's vocabulary at runtime; learned verbs persist across sessions. | `az <group> --help` |
| `capabilities` | `caps`, `powers`, `spells` | List the `az` capabilities AzZork has learned so far. | — |
| `help` | `?`, `h` | Show the in-game command list (including learned capabilities). | — |
| `quit` | `q`, `exit` | Leave the dungeon (prints your final score). | — |

Unrecognized input returns a friendly hint rather than crashing:

```
az> frobnicate the vm
I don't understand "frobnicate the vm". Type 'help' for commands.
```

### Directions

`go` accepts six directions, each with a single-letter abbreviation. A bare
direction is shorthand for `go <direction>`:

| Direction | Abbreviation |
| --- | --- |
| `north` | `n` |
| `south` | `s` |
| `east` | `e` |
| `west` | `w` |
| `up` | `u` |
| `down` | `d` |

```
az> north      # same as "go north"
az> go s
az> u
```

Trying to move where there is no exit is harmless:

```
az> go west
You can't go west from here.
```

## Working with resources

Resource names are matched **case-insensitively** and by **prefix**, so you can
type the shortest unambiguous fragment:

```
az> examine web        # matches "webstore"
az> lock keyv          # matches "keyvault"
```

`examine` shows a full status block:

```
az> examine webstore
webstore [Microsoft.Storage/storageAccounts]
A storage account with its container door flung wide open.
Status: PUBLIC | UNENCRYPTED | unlocked | ~$60/mo
A Grue senses it is exposed to the public internet, storing its data
unencrypted, unlocked and vulnerable to deletion.
```

### Hardening: `lock`

`lock` is the primary way to remove hazards. It applies three protections at
once — a management lock, a private endpoint, and encryption at rest:

```
az> lock webstore
You ward the webstore with a management lock, private endpoints, and
encryption. A Grue recoils.
```

A locked resource cannot be deleted, protecting you from accidental destruction.
If you later need to change or remove it, lift the lock first with `unlock`:

```
az> unlock webstore
You lift the management lock from the webstore. It can now be changed or
deleted — but it is once more vulnerable.
```

### Cutting cost: `resize`

A resource whose estimated spend reaches **$500/mo** counts as a cost-overrun
hazard, and `lock` cannot clear it. Use `resize` (aliases `right-size`, `scale`)
to move it to a smaller tier — this roughly halves the monthly cost:

```
az> resize sqlserver
You right-size the sqlserver to a reserved tier: ~$800/mo down to ~$400/mo.
The cost-overrun Grue loses its scent.
```

### Confirmation prompts

`take` and `drop` are the only mutating verbs that touch resources, and both ask
for confirmation. The default is **No** — pressing Enter (or anything other than
`y`/`yes`) cancels:

```
az> drop orphan-vm
DELETE 'orphan-vm'? This is destructive and cannot be undone. [y/N] n
You stay your hand. The resource survives.
```

> In the mock backend, `take` and `drop` only mutate the in-memory world — no
> real resources are ever affected. Even with `--backend az`, discovery is
> read-only; deletions are simulated in memory.

## The Grue mechanic

A room is **dark** when monitoring is disabled. In the dark you cannot `look`
at detail, cannot `examine`, and cannot `take` — and a **Grue** stalks you.

Each turn spent in a dark room escalates the danger:

1. **First turn** — always a warning:
   `>> It is dark. You hear the slavering fangs of a Grue nearby. Enable monitoring (type 'monitor') before it strikes!`
2. **Second turn** — ~25% chance of death.
3. **Third turn** — ~50% chance.
4. **Fourth turn and beyond** — ~75% chance.

Escape by leaving the room (`go <dir>` back toward the light) or by casting
light with `monitor`:

```
az> monitor
You enable diagnostic settings and Azure Monitor. Light floods the room; the
lurking Grue shrieks and flees.
```

If a Grue catches you, the game ends and your final score is printed:

```
>> Oh no! You have walked too long in the dark. A GRUE lunges from the shadows
and DEVOURS you.

*** You have died. ***
```

## Scoring

`score` reports your governance posture from 0 to 100. You start well below 100
because the world seeds hazards for you to fix. Each outstanding hazard costs 5
points. The default mock estate opens with **14 hazards**:

```
az> score
Governance posture: 30/100  —  rank: Reckless Tinkerer
Outstanding hazards: 14 (public/unencrypted/unlocked resources, cost overruns,
unmonitored rooms)
Moves taken: 0
```

Hazards counted:

- A resource that is **public**.
- A resource that is **unencrypted**.
- A resource that is **unlocked**.
- A resource whose estimated cost is **≥ $500/mo**.
- Each **dark (unmonitored)** room.

Ranks by score:

| Score | Rank |
| --- | --- |
| 90–100 | Cloud Guardian |
| 70–89 | Diligent Steward |
| 50–69 | Apprentice Admin |
| 30–49 | Reckless Tinkerer |
| 0–29 | Grue Chow |

Your goal: `lock` every hazardous resource, `monitor` every dark room, and
`resize` any resource bleeding **≥ $500/mo**. Clear all 14 hazards to reach a
perfect **100/100 — Cloud Guardian**.

> A **cost overrun** (≥ $500/mo) cannot be `lock`ed away — it reflects real
> spend, so only `resize` clears it. In the default mock estate the `sqlserver`
> ($800/mo) is the one resource that needs right-sizing as well as locking; do
> both and a perfect 100/100 run is achievable.

## Self-evolution: `learn` and `capabilities`

AzZork's verb set is not frozen. It **derives** capabilities from the real `az`
CLI and remembers them across sessions.

```text
az> learn group
You study the 'group' grimoire. AzZork learns 8 new az power(s); 8 known in
total. (remembered in ~/.local/share/azork/capabilities.tsv)

az> capabilities
AzZork has learned 8 az capabilities across 1 groups:
Discovered az capabilities (learned at runtime):
 [group]
  create         az group create — Create a new resource group.
  list           az group list — List resource groups.
  ...
```

- **`learn <group>`** shells out to `az <group> --help`, parses the command list,
  and folds each discovered command into AzZork's capability registry as a new
  verb — **no code change required**. This needs the real `az` CLI on `PATH`; if
  it is missing, `learn` reports the problem instead of failing hard.
- **Persistence.** Learned capabilities are written to a small cache file and
  **recalled automatically on the next launch**. The default location is
  `~/.local/share/azork/capabilities.tsv` (honouring `XDG_DATA_HOME`); set
  `AZORK_CACHE_DIR` to override it.
- **Adaptive help.** `help` appends everything learned so far, grouped by `az`
  command group. `capabilities` (aliases `caps`, `powers`, `spells`) lists them.
- **Intent resolution.** Any input that matches no built-in verb is not rejected
  outright: AzZork ranks your words against its learned capabilities and either
  acts on a confident match or offers a "did you mean…" list.
