//! AzZork — the Azure control plane reimagined as a Zork-style text adventure.
//!
//! Run with no arguments for the offline mock dungeon (no Azure credentials
//! required). Pass `--backend az` (or set `AZORK_BACKEND=az`) to explore your
//! real subscription via the `az` CLI.

use azork::agent::{IntentResolver, MockAdapter};
use azork::az_runner::{AzRunner, ProcessAzRunner};
use azork::backend;
use azork::capabilities::{registry::default_cache_path, CapabilityRegistry};
use azork::memory::{default_memory_path, GraphMemory, MemoryKind};
use azork::parser::{self, Command};
use azork::world::{GrueOutcome, World};
use std::io::{self, BufRead, Write};
use std::path::Path;

const BANNER: &str = r#"
    ___    ______           __
   /   |  ____/ / __ \_____/ /__
  / /| | /_  / / / / / ___/ //_/
 / ___ |/ /_/ / /_/ / /  / ,<
/_/  |_|\____/\____/_/  /_/|_|

AzZork — an Azure Control-Plane Adventure
=========================================
You are standing at the edge of a vast Azure subscription. Resource groups
sprawl before you like torch-lit chambers; resources lurk within as objects and
creatures. Governance hazards — public endpoints, unencrypted data, runaway
cost, and above all the DARK of unmonitored rooms — breed GRUES.

Harden the estate. Raise your governance score. And whatever you do... keep the
lights on.

Type 'help' for commands. Type 'quit' to leave.
"#;

const HELP: &str = r#"Commands (Zork verbs -> Azure operations):
  look / l                describe the current resource group (list resources)
  examine <name> / x      inspect a resource (az resource show)
  go <dir> | <dir>        navigate: north south east west up down (n/s/e/w/u/d)
  take <name>             acquire a resource into inventory (with confirmation)
  drop <name>             delete a resource (destructive, with confirmation)
  lock <name>             secure a resource: lock + private + encrypted
  unlock <name>           remove a management lock (so it can change/delete)
  resize <name>           right-size a resource to cut runaway monthly cost
  monitor / light         enable monitoring here (banish the Grue)
  cast deploy [template]  cast a deployment spell (bicep/ARM, mock)
  inventory / i           list resources you are carrying
  score                   report your governance posture (0-100)
  learn <group>           introspect 'az <group> --help' and grow AzZork's powers
  capabilities / caps     list the az capabilities AzZork has learned
  recall <query>          ranked recall over AzZork's persistent memory
  friction <note>         record something confusing/missing to improve later
  memory / mem            summarise what AzZork remembers
  help / ?                show this help
  version / ver           show the AzZork version
  quit / q                leave the dungeon

Beware: acting in a dark (unmonitored) room invites a Grue to eat you.
AzZork evolves: unknown input is resolved against what it has learned, and
'learn <group>' teaches it new az verbs that persist across sessions."#;

fn main() {
    let requested_backend = resolve_backend_id();
    if let Some(id) = &requested_backend {
        if !backend::is_recognized(id) {
            eprintln!(
                "Warning: unknown backend '{}'; falling back to the offline mock estate. \
                 Recognised backends: mock, az. (This is NOT your live Azure subscription.)",
                id
            );
        }
    }
    let backend_id = requested_backend.unwrap_or_else(|| "mock".to_string());
    let backend = backend::select(&backend_id);

    let mut world = match backend.build_world() {
        Ok(w) => w,
        Err(e) => {
            eprintln!(
                "Failed to build world via {} backend: {}",
                backend.name(),
                e
            );
            eprintln!("Tip: run without arguments to use the offline mock backend.");
            std::process::exit(1);
        }
    };

    println!("{}", BANNER);
    println!(
        "[backend: {} | subscription: {}]\n",
        backend.name(),
        world.subscription
    );
    println!("{}\n", world.look());

    // AzZork's learned vocabulary, persisted across sessions.
    let cache_path = default_cache_path();
    let mut registry = CapabilityRegistry::load(&cache_path);
    if !registry.is_empty() {
        println!(
            "[memory: recalled {} learned az capabilities from {}]\n",
            registry.len(),
            cache_path.display()
        );
    }
    // AzZork's persistent graph memory — rooms, resources, intents, friction.
    let memory_path = default_memory_path();
    let mut memory = GraphMemory::load(&memory_path);
    if !memory.is_empty() {
        println!(
            "[memory: recalled {} remembered nodes from {}]\n",
            memory.len(),
            memory_path.display()
        );
    }
    // Remember where we start so navigation memory has an anchor.
    record_room(&mut memory, &world);

    // Live derivation runs through the real `az` CLI at runtime.
    let runner = ProcessAzRunner::new();

    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();

    loop {
        if world.game_over {
            break;
        }
        print!("\naz> ");
        io::stdout().flush().ok();

        let line = match lines.next() {
            Some(Ok(l)) => l,
            _ => {
                println!("\nThe portal closes behind you. Farewell.");
                break;
            }
        };

        // Every non-empty line is an intent AzZork has now seen.
        if !line.trim().is_empty() {
            memory.record_intent(line.trim());
        }

        let cmd = parser::parse(&line);
        let quit = handle(
            &mut world,
            cmd,
            &mut lines,
            &mut registry,
            &mut memory,
            &runner,
            &cache_path,
        );
        // Keep navigation memory in step with wherever we now stand.
        record_room(&mut memory, &world);
        if quit {
            break;
        }

        // After each meaningful turn, the Grue may act in the dark.
        run_grue_check(&mut world);
    }

    // Persist everything AzZork learned this session.
    if let Err(e) = memory.save(&memory_path) {
        eprintln!("[memory: warning — could not persist memory: {}]", e);
    }

    if world.game_over {
        println!("\n{}", world.score());
    }
}

/// Fold the current room and its resources into the graph memory: a resource
/// group is a *room* node, each resource an *object* linked by `contains`.
fn record_room(memory: &mut GraphMemory, world: &World) {
    let room = world.current_room();
    let room_id = memory.remember_room(&room.name, &room.region);
    for res in &room.resources {
        memory.remember_resource(&room_id, &res.name, &res.kind);
    }
}

/// Determine which backend the user explicitly requested via `--backend <id>`
/// (or `-b`, `--backend=<id>`) or the `AZORK_BACKEND` env var.
///
/// Returns `None` when no backend was requested, so the caller can default to
/// the mock estate. An explicit-but-empty request (e.g. a trailing `--backend`
/// with no value) yields `Some("")`, which the caller treats as unrecognized
/// and warns about rather than silently defaulting.
fn resolve_backend_id() -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--backend" | "-b" => {
                return Some(args.get(i + 1).cloned().unwrap_or_default());
            }
            other if other.starts_with("--backend=") => {
                return Some(other.trim_start_matches("--backend=").to_string());
            }
            _ => {}
        }
        i += 1;
    }
    std::env::var("AZORK_BACKEND").ok()
}

/// Handle a single command. Returns `true` if the player asked to quit.
fn handle<I>(
    world: &mut World,
    cmd: Command,
    lines: &mut I,
    registry: &mut CapabilityRegistry,
    memory: &mut GraphMemory,
    runner: &dyn AzRunner,
    cache_path: &Path,
) -> bool
where
    I: Iterator<Item = io::Result<String>>,
{
    match cmd {
        Command::Empty => {}
        Command::Look => println!("{}", world.look()),
        Command::Examine(t) => println!("{}", world.examine(&t)),
        Command::Go(dir) => match world.go(dir) {
            Ok(desc) => println!("{}", desc),
            Err(e) => println!("{}", e),
        },
        Command::Take(t) => {
            if confirm(&format!("Acquire '{}' into your inventory?", t), lines) {
                println!("{}", world.take(&t));
            } else {
                println!("You leave it be.");
            }
        }
        Command::Drop(t) => {
            if confirm(
                &format!("DELETE '{}'? This is destructive and cannot be undone.", t),
                lines,
            ) {
                println!("{}", world.drop_item(&t));
            } else {
                println!("You stay your hand. The resource survives.");
            }
        }
        Command::Lock(t) => println!("{}", world.lock(&t)),
        Command::Unlock(t) => println!("{}", world.unlock(&t)),
        Command::Resize(t) => println!("{}", world.resize(&t)),
        Command::Monitor => println!("{}", world.monitor()),
        Command::Inventory => println!("{}", world.inventory()),
        Command::Score => println!("{}", world.score()),
        Command::Cast(spell) => println!("{}", cast(world, &spell)),
        Command::Learn(group) => {
            println!("{}", learn(registry, memory, runner, cache_path, &group))
        }
        Command::Capabilities => println!("{}", capabilities_report(registry)),
        Command::Friction(note) => {
            memory.record_friction(&note, &["player"]);
            println!(
                "Noted. AzZork will remember this friction: \"{}\".",
                note.trim()
            );
        }
        Command::Recall(query) => println!("{}", recall_report(memory, &query)),
        Command::Memory => println!("{}", memory.summary()),
        Command::Help => println!("{}", help_text(registry)),
        Command::Version => println!(
            "AzZork v{} — the Azure control plane as a text adventure.",
            env!("CARGO_PKG_VERSION")
        ),
        Command::Quit => {
            println!("\nYou step back through the portal.\n{}", world.score());
            return true;
        }
        Command::Unknown(raw) => {
            // Never hard-fail: try to resolve intent against learned capabilities.
            let resolver = IntentResolver::new(MockAdapter::new(), registry);
            let resolution = resolver.resolve(&raw);
            println!("{}", resolution.narrate());
            // An input AzZork could not resolve is friction worth remembering.
            if matches!(resolution, azork::agent::Resolution::Unresolved(_)) {
                memory.record_friction(
                    &format!("unresolved intent: {}", raw.trim()),
                    &["intent", "unresolved"],
                );
            }
        }
    }
    false
}

/// Ranked recall over the graph memory, rendered for the player.
fn recall_report(memory: &GraphMemory, query: &str) -> String {
    let hits = memory.recall(query, None, 6);
    if hits.is_empty() {
        return format!("AzZork recalls nothing about \"{}\".", query.trim());
    }
    let mut out = format!("AzZork recalls, for \"{}\":", query.trim());
    for n in hits {
        out.push_str(&format!(
            "\n  [{}] {} — {}",
            kind_token(n.kind),
            n.label,
            n.content
        ));
    }
    out
}

/// Short display token for a memory kind.
fn kind_token(kind: MemoryKind) -> &'static str {
    match kind {
        MemoryKind::Capability => "cap",
        MemoryKind::Room => "room",
        MemoryKind::Resource => "res",
        MemoryKind::Intent => "intent",
        MemoryKind::Friction => "friction",
    }
}

/// The full help text: static core verbs plus any learned capabilities.
fn help_text(registry: &CapabilityRegistry) -> String {
    let learned = registry.help_text();
    if learned.is_empty() {
        HELP.to_string()
    } else {
        format!("{}\n\n{}", HELP, learned)
    }
}

/// Report what AzZork has learned so far.
fn capabilities_report(registry: &CapabilityRegistry) -> String {
    if registry.is_empty() {
        "AzZork has learned no az capabilities yet. Try 'learn group' or \
         'learn storage' to teach it (requires the real 'az' CLI)."
            .to_string()
    } else {
        format!(
            "AzZork has learned {} az capabilities across {} groups:\n{}",
            registry.len(),
            registry.groups().len(),
            registry.help_text()
        )
    }
}

/// Introspect `az <group> --help`, fold new capabilities into the registry, and
/// persist them so the knowledge survives to future sessions.
fn learn(
    registry: &mut CapabilityRegistry,
    memory: &mut GraphMemory,
    runner: &dyn AzRunner,
    cache_path: &Path,
    group: &str,
) -> String {
    let group = group.split_whitespace().next().unwrap_or(group);
    match registry.learn_group(runner, group) {
        Ok(added) => {
            // Mirror the freshly-learned capabilities into graph memory so recall
            // can surface them alongside rooms, resources, and intents.
            for cap in registry.iter().filter(|c| c.group == group) {
                memory.remember_capability(cap);
            }
            let save_note = match registry.save(cache_path) {
                Ok(()) => format!("(remembered in {})", cache_path.display()),
                Err(e) => format!("(warning: could not persist: {})", e),
            };
            format!(
                "You study the '{}' grimoire. AzZork learns {} new az power(s); \
                 {} known in total. {}",
                group,
                added,
                registry.len(),
                save_note
            )
        }
        Err(e) => format!(
            "You pore over the '{}' tomes but find nothing usable: {}. \
             (Is the real 'az' CLI installed and on PATH?)",
            group, e
        ),
    }
}

/// Cast a spell. Currently only `deploy` (a mock bicep/ARM deployment).
fn cast(world: &World, spell: &str) -> String {
    let lowered = spell.to_lowercase();
    if lowered.starts_with("deploy") {
        let template = lowered.trim_start_matches("deploy").trim();
        let target = world.current_room().name.clone();
        if template.is_empty() {
            format!(
                "You raise your staff and chant 'az deployment group create'...\n\
                 A shimmering ARM template materialises and deploys into {}. \
                 (mock: no resources were harmed.)",
                target
            )
        } else {
            format!(
                "You invoke the deployment spell with '{}'...\n\
                 The bicep incantation compiles and deploys into {}. \
                 (mock: no real resources were provisioned.)",
                template, target
            )
        }
    } else {
        format!(
            "You don't know the spell '{}'. You only know 'cast deploy'.",
            spell
        )
    }
}

/// Prompt the player for yes/no confirmation. Defaults to "no".
fn confirm<I>(question: &str, lines: &mut I) -> bool
where
    I: Iterator<Item = io::Result<String>>,
{
    print!("{} [y/N] ", question);
    io::stdout().flush().ok();
    match lines.next() {
        Some(Ok(ans)) => {
            let a = ans.trim().to_lowercase();
            a == "y" || a == "yes"
        }
        _ => false,
    }
}

/// Run the per-turn Grue check and narrate the outcome.
fn run_grue_check(world: &mut World) {
    match world.grue_check() {
        GrueOutcome::Safe => {}
        GrueOutcome::Lurking => {
            println!(
                "\n>> It is dark. You hear the slavering fangs of a Grue nearby. \
                 Enable monitoring (type 'monitor') before it strikes!"
            );
        }
        GrueOutcome::Devoured => {
            println!(
                "\n>> Oh no! You have walked too long in the dark. A GRUE lunges \
                 from the shadows and DEVOURS you.\n\n*** You have died. ***"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use azork::backend::{mock::MockBackend, Backend};

    #[test]
    fn cast_deploy_is_mock_safe() {
        let w = MockBackend::new().build_world().unwrap();
        let out = cast(&w, "deploy");
        assert!(out.to_lowercase().contains("mock"));
    }

    #[test]
    fn cast_unknown_spell() {
        let w = MockBackend::new().build_world().unwrap();
        let out = cast(&w, "fireball");
        assert!(out.contains("don't know"));
    }

    #[test]
    fn confirm_reads_yes() {
        let mut it = vec![Ok("yes".to_string())].into_iter();
        assert!(confirm("go?", &mut it));
    }

    #[test]
    fn confirm_defaults_no_on_eof() {
        let mut it: std::vec::IntoIter<io::Result<String>> = vec![].into_iter();
        assert!(!confirm("go?", &mut it));
    }
}
