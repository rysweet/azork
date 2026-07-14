# AzZork Tutorial: Harden the Estate

This walkthrough takes you from your first `look` to the coveted **Cloud
Guardian** rank, all in the offline mock dungeon — no Azure account required.

If you just want a command reference, see the [Usage guide](USAGE.md). For how
backends work, see the [Configuration reference](CONFIGURATION.md).

## 1. Enter the dungeon

```bash
azork
```

You are greeted by the banner and dropped into `landing-rg`:

```
[backend: mock (offline) | subscription: Contoso-Dev (mock)]

== landing-rg (eastus) ==
The West Landing Zone. Cables snake overhead and a subscription portal hums
softly. This resource group is monitored and safe.
You see:
  - portal (Microsoft.Portal/dashboards)
Exits: down, east, north
```

Check your starting posture:

```
az> score
Governance posture: 30/100  —  rank: Reckless Tinkerer
Outstanding hazards: 14 (public/unencrypted/unlocked resources, cost overruns,
unmonitored rooms)
Moves taken: 0
```

Fourteen hazards stand between you and Cloud Guardian: every resource starts
unlocked, several are public or unencrypted, one room is dark, and the SQL
server is bleeding cost. Let's hunt them down.

## 2. Secure the public web tier

Head north into the exposed web tier:

```
az> north
== web-rg (eastus) ==
The Public Web Tier. Wind howls through open ports. Something here is exposed to
the whole internet.
You see:
  - appservice (Microsoft.Web/sites)
  - webstore (Microsoft.Storage/storageAccounts)
Exits: north, south
```

Examine the storage account to see why the Grues are circling:

```
az> examine webstore
webstore [Microsoft.Storage/storageAccounts]
A storage account with its container door flung wide open.
Status: PUBLIC | UNENCRYPTED | unlocked | ~$60/mo
A Grue senses it is exposed to the public internet, storing its data
unencrypted, unlocked and vulnerable to deletion.
```

Three hazards on one resource. Ward it:

```
az> lock webstore
You ward the webstore with a management lock, private endpoints, and
encryption. A Grue recoils.

az> lock appservice
You ward the appservice with a management lock, private endpoints, and
encryption. A Grue recoils.
```

`lock` clears the public, unencrypted, and unlocked flags in one move. Two
resources hardened, four hazards gone.

## 3. Tame the cost-overrun creature

Return to the landing zone and go east to the data vaults:

```
az> south
az> east
== data-rg (westus2) ==
The Data Vaults. Cold air, rows of disks, and the low growl of an overpriced
database.
You see:
  - sqlserver (Microsoft.Sql/servers)
  - keyvault (Microsoft.KeyVault/vaults)
Exits: west

az> examine sqlserver
sqlserver [Microsoft.Sql/servers]
A hulking SQL server, scales slick with transaction logs.
Status: private | encrypted | unlocked | ~$800/mo
A Grue senses it is unlocked and vulnerable to deletion, bleeding $800/mo in
cost.
```

The `$800/mo` cost is itself a hazard (any resource ≥ $500/mo). Locking removes
the *unlocked* hazard, but the cost hazard needs a different tool — `resize`,
which right-sizes the resource to a smaller, cheaper tier:

```
az> lock keyvault
az> lock sqlserver
az> resize sqlserver
You right-size the sqlserver to a reserved tier: ~$800/mo down to ~$400/mo.
The cost-overrun Grue loses its scent.
```

Now the SQL server is locked *and* under the $500/mo threshold — all of its
hazards are cleared.

### Don't forget the Hall of Identity

One room is easy to miss. From the landing zone, go **down** into `identity-rg`
and lock the managed identity lurking there:

```
az> west
az> down
== identity-rg (eastus) ==
The Hall of Identity. Managed identities drift like wisps and RBAC wards bar the
deeper doors.
You see:
  - managed-identity (Microsoft.ManagedIdentity/userAssignedIdentities)
Exits: up

az> lock managed-identity
```

## 4. Face the Grue in the dark

The most dangerous hazard is a **dark room**. From `web-rg`, go north into the
unmonitored group:

```
az> west
az> north
az> north
== unmon-rg (centralus) ==
It is pitch black here — no monitoring, no diagnostics. You are likely to be
eaten by a Grue.
Exits: south

>> It is dark. You hear the slavering fangs of a Grue nearby. Enable monitoring
(type 'monitor') before it strikes!
```

The first dark turn is only a warning — but linger and the odds turn deadly
(~25% on turn 2, ~50% on turn 3, ~75% thereafter). **Do not dawdle.** Turn on
the lights immediately:

```
az> monitor
You enable diagnostic settings and Azure Monitor. Light floods the room; the
lurking Grue shrieks and flees.
```

The room is now lit. You can see its contents and harden them:

```
az> look
== unmon-rg (centralus) ==
...
You see:
  - orphan-vm (Microsoft.Compute/virtualMachines)
Exits: south

az> lock orphan-vm
You ward the orphan-vm with a management lock, private endpoints, and
encryption. A Grue recoils.
```

> **If a Grue catches you** the game ends:
> ```
> >> Oh no! You have walked too long in the dark. A GRUE lunges from the shadows
> and DEVOURS you.
>
> *** You have died. ***
> ```
> Restart with `azork` and try again — this time, `monitor` sooner.

## 5. Locking, unlocking, and deleting safely

`drop` deletes a resource and `take` moves it into your inventory. Both ask for
**y/N** confirmation and default to **No**, so a stray keystroke never destroys
anything.

A locked resource **refuses deletion** — a deliberate safeguard. Because you
locked `orphan-vm` in the previous step, an attempted delete is blocked:

```
az> drop orphan-vm
DELETE 'orphan-vm'? This is destructive and cannot be undone. [y/N] y
The orphan-vm is locked. Unlock it before you can delete it.
```

If you genuinely need to remove it, lift the lock first with `unlock`, then
delete:

```
az> unlock orphan-vm
You lift the management lock from the orphan-vm. It can now be changed or
deleted — but it is once more vulnerable.
```

> Re-locking restores all its protections:
> ```
> az> lock orphan-vm
> You ward the orphan-vm with a management lock, private endpoints, and
> encryption. A Grue recoils.
> ```
> For a clean Cloud-Guardian run, leave everything locked — only unlock what you
> truly mean to change.

With `orphan-vm` locked again, `take` moves it into your inventory (locked
resources move with their protections intact):

```
az> take orphan-vm
Acquire 'orphan-vm' into your inventory? [y/N] y
You acquire the orphan-vm and add it to your inventory.

az> inventory
You are carrying:
  - orphan-vm (Microsoft.Compute/virtualMachines)
```

Nothing here touches real Azure — the mock world is entirely in memory.

## 6. Cast a deployment spell

`cast deploy` simulates a bicep/ARM deployment into the current room:

```
az> cast deploy webapp.bicep
You invoke the deployment spell with 'webapp.bicep'...
The bicep incantation compiles and deploys into web-rg. (mock: no real
resources were provisioned.)
```

`deploy webapp.bicep` (without `cast`) is an accepted shorthand.

## 7. Claim your rank

Once every resource is locked, the pricey SQL server is right-sized, and every
room is monitored, check your posture:

```
az> score
Governance posture: 100/100  —  rank: Cloud Guardian
Outstanding hazards: 0 (public/unencrypted/unlocked resources, cost overruns,
unmonitored rooms)
Moves taken: 20
```

A flawless run: all seven resources locked (clearing public, unencrypted, and
unlocked flags), the one dark room monitored, and the `sqlserver` right-sized
below the $500/mo threshold. Zero hazards remain — a perfect **100/100 Cloud
Guardian**.

> Miss the `resize sqlserver` step and you'll top out at **95/100** — still
> Cloud Guardian, but with one cost-overrun hazard the lock could not clear.

Leave the dungeon in triumph:

```
az> quit

You step back through the portal.
Governance posture: 100/100  —  rank: Cloud Guardian
...
```

## Where to next?

- Point AzZork at your **real** subscription (read-only) with
  `azork --backend az` — see the
  [Configuration reference](CONFIGURATION.md#the-az-backend-live-azure).
- Explore the engine internals in the [API / module reference](API.md).
- Full command details live in the [Usage guide](USAGE.md).
