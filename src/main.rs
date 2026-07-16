//! AzZork — the Azure control plane reimagined as a Zork-style text adventure.
//!
//! Run with no arguments for the offline mock dungeon (no Azure credentials
//! required). Pass `--backend az` (or set `AZORK_BACKEND=az`) to explore your
//! real subscription via the `az` CLI.

use azork::agent::{truncate_intent, IntentResolver, MockAdapter};
use azork::az_runner::{AzRunner, FakeAzRunner, ProcessAzRunner};
use azork::backend;
use azork::capabilities::{autodiscover, registry::default_cache_path, CapabilityRegistry};
use azork::dungeon::{cli as dungeon_cli, map as dungeon_map, playwright, render, server};
use azork::memory::{default_memory_path, GraphMemory, MemoryKind};
use azork::parser::{self, Command};
use azork::quests::builtin_quests;
use azork::update;
use azork::world::{GrueOutcome, World};
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::thread;

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
  achievements / badges   show your governance scorecard (score + badges)
  quest / quests          show progress on governance quests
  learn <group>           manually refresh 'az <group> --help' (also auto-learned at startup)
  capabilities / caps     list the az capabilities AzZork has learned
  recall <query>          ranked recall over AzZork's persistent memory
  friction <note>         record something confusing/missing to improve later
  memory / mem            summarise what AzZork remembers
  help / ?                show this help
  version / ver           show the AzZork version
  quit / q                leave the dungeon

Beware: acting in a dark (unmonitored) room invites a Grue to eat you.
AzZork evolves automatically: it discovers new az command groups at startup
without being asked, resolves unknown input against what it has learned, and
'learn <group>' remains available to manually refresh a group on demand —
all of it persists across sessions."#;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let Some(first) = args.get(1) {
        if dungeon_cli::is_crawl_subcommand(first) {
            let rest: Vec<String> = args[2..].to_vec();
            if dungeon_cli::wants_help(&rest) {
                print!("{}", dungeon_cli::CRAWL_HELP);
                return;
            }
            match dungeon_cli::parse(&rest) {
                Ok(crawl_args) => run_crawl(crawl_args),
                Err(e) => {
                    eprintln!("azork {}: {}", first, e);
                    std::process::exit(2);
                }
            }
            return;
        }
    }

    // Top-level subcommands / flags handled before the game starts.
    match args.get(1).map(String::as_str) {
        Some("update") => {
            if args.iter().any(|a| a == "--help" || a == "-h") {
                println!(
                    "azork update [--check]\n\n\
                     Self-update AzZork to the latest release.\n\n\
                     Flags:\n  \
                     --check       check for an available update without installing it\n  \
                     --help, -h    show this help and exit\n"
                );
                return;
            }
            let check_only = args.iter().any(|a| a == "--check");
            std::process::exit(update::run_update_with(check_only));
        }
        Some("--version") | Some("-V") | Some("version") => {
            println!("azork {}", azork::VERSION);
            return;
        }
        Some("--help") | Some("-h") => {
            println!("{}", BANNER);
            println!("{}", HELP);
            println!(
                "\nSubcommands:\n  azork                 play the adventure (offline mock by default)\n  \
                 azork crawl [flags]   render the Dungeon Crawler map (alias: dungeon)\n  \
                 azork update          self-update to the latest release\n  \
                 azork --version       print the version\n\nEnvironment:\n  \
                 AZORK_BACKEND=az      use the live `az` CLI backend\n  \
                 {}=0/false  disable automatic startup capability discovery\n  \
                 {}=1  disable the startup update check",
                autodiscover::AUTODISCOVER_ENV,
                update::NO_UPDATE_CHECK_ENV
            );
            return;
        }
        _ => {}
    }

    // Anything left in `args[1]`'s position that isn't a recognized launch
    // flag for the game itself (`--backend`/`-b`, `--backend=<id>`) is not a
    // usage error — it's an intent AzZork hasn't parsed yet. Route it through
    // the same offline IntentResolver the interactive REPL uses instead of
    // rejecting it (see issue #33; PR #37's hard-reject defeated the whole
    // point of an agentic CLI).
    if let Some(raw) = unrecognized_top_level_intent(&args) {
        print_top_level_intent_resolution(&raw);
        return;
    }

    // Optional, cached, subprocess-safe startup update check. Never hangs or
    // prompts under CI / non-TTY; see `update::check`.
    if matches!(
        update::check::maybe_startup_check(&args),
        update::check::StartupUpdateOutcome::ExitSuccess
    ) {
        return;
    }

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

    // Startup auto-discovery: proactively learn any `az` groups missing from
    // the (just-recalled) cache, without the player typing `learn`. Runs on
    // a background thread — computation only, no shared state — so it never
    // blocks the first prompt; discovered capabilities stream in and are
    // applied between turns. Disable via AZORK_AUTODISCOVER=0 for
    // offline/CI contexts, or it degrades gracefully on its own if `az`
    // isn't installed.
    let discovery_cancel = Arc::new(AtomicBool::new(false));
    let discovery_rx = if autodiscover::autodiscover_enabled() {
        Some(spawn_autodiscovery(
            registry.groups(),
            discovery_cancel.clone(),
        ))
    } else {
        None
    };

    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();

    loop {
        if world.game_over {
            break;
        }
        // Fold in anything auto-discovery has learned since the last turn
        // before showing the prompt, so the vocabulary keeps growing live.
        if let Some(rx) = &discovery_rx {
            drain_discovery(rx, &mut registry, &mut memory, &cache_path);
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

        // The player is now actively interacting: let any still-running
        // startup discovery know it can stop enumerating further (not yet
        // started) groups rather than compete for attention.
        discovery_cancel.store(true, Ordering::Relaxed);

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

/// Spawn the background startup-discovery thread and return the channel it
/// streams [`autodiscover::GroupResult`]s over as it learns them.
///
/// This thread only *computes*: it owns its own [`ProcessAzRunner`] (a cheap
/// `Copy` type) and a snapshot of the groups already known, and never
/// touches the registry, memory, or cache directly — the main thread remains
/// the sole owner of that state and applies results via [`drain_discovery`].
fn spawn_autodiscovery(
    known_groups: Vec<String>,
    cancel: Arc<AtomicBool>,
) -> Receiver<autodiscover::GroupResult> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let runner = ProcessAzRunner::new();
        autodiscover::stream_startup_autodiscovery(&runner, &known_groups, &cancel, &tx);
    });
    rx
}

/// Drain any startup-discovery results that have arrived so far (without
/// blocking), fold them into the registry and graph memory, persist the
/// cache if anything new was learned, and let the player know.
fn drain_discovery(
    rx: &Receiver<autodiscover::GroupResult>,
    registry: &mut CapabilityRegistry,
    memory: &mut GraphMemory,
    cache_path: &Path,
) {
    let mut learned_anything = false;
    let results: Vec<autodiscover::GroupResult> = rx.try_iter().collect();
    for applied in autodiscover::apply_learned(registry, results) {
        match applied.result {
            Ok(added) if added > 0 => {
                learned_anything = true;
                for cap in registry.iter().filter(|c| c.group == applied.group) {
                    memory.remember_capability(cap);
                }
                println!(
                    "[autodiscover: learned {} new az power(s) from '{}'; {} known in total]",
                    added,
                    applied.group,
                    registry.len()
                );
            }
            Ok(_) => {}
            Err(_e) => {
                // Discovery failures (e.g. `az` unavailable/unauthenticated)
                // are expected offline/in CI and shouldn't spam the player;
                // startup already succeeded via cache + built-ins.
            }
        }
    }
    if learned_anything {
        if let Err(e) = registry.save(cache_path) {
            eprintln!(
                "[autodiscover: warning — could not persist capabilities cache: {}]",
                e
            );
        }
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

/// If `args[1]` is not a recognized launch flag for the game itself
/// (`--backend`/`-b`, `--backend=<id>`), return the remaining words joined as
/// natural-language intent text.
///
/// By the time this runs, `crawl`/`dungeon`, `update`, `--version`/`-V`/`version`,
/// and `--help`/`-h` have already been handled (and returned/exited), so
/// anything still in `args[1]`'s position — a bare word or any other flag —
/// is not a usage error; it's input for the [`IntentResolver`] to make sense
/// of, exactly like an unrecognized line typed at the interactive `az>`
/// prompt.
fn unrecognized_top_level_intent(args: &[String]) -> Option<String> {
    match args.get(1).map(String::as_str) {
        None => None,
        Some("--backend") | Some("-b") => None,
        Some(other) if other.starts_with("--backend=") => None,
        Some(_) => Some(args[1..].join(" ")),
    }
}

/// Resolve unrecognized top-level CLI input the same way the interactive
/// REPL resolves an unrecognized line: via the offline [`IntentResolver`]
/// against the learned [`CapabilityRegistry`], never a hard failure.
fn print_top_level_intent_resolution(raw: &str) {
    let cache_path = default_cache_path();
    let registry = CapabilityRegistry::load(&cache_path);
    let resolver = IntentResolver::new(MockAdapter::new(), &registry);
    let resolution = resolver.resolve(raw);
    println!("{}", resolution.narrate());
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
        Command::Achievements => println!("{}", achievements_report(world)),
        Command::Quest => println!("{}", quests_report(world)),
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
                    &format!("unresolved intent: {}", truncate_intent(raw.trim())),
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

/// Render the governance scorecard: the score/rank line followed by each
/// achievement badge, earned or locked (with its specific blocker).
fn achievements_report(world: &World) -> String {
    let mut out = world.score();
    out.push_str("\n\nAchievements:");
    for badge in world.achievements() {
        if badge.earned {
            out.push_str(&format!(
                "\n  [x] {} {} — {}",
                badge.emoji, badge.name, badge.description
            ));
        } else {
            out.push_str(&format!(
                "\n  [ ] {} {} — locked: {}",
                badge.emoji,
                badge.name,
                badge.blocker.unwrap_or_default()
            ));
        }
    }
    out
}

/// Render progress for every built-in quest against the current world state.
fn quests_report(world: &World) -> String {
    let mut out = String::from("Quests — governance objectives for this estate:\n");
    for quest in builtin_quests() {
        let progress = quest.evaluate(world);
        out.push_str(&format!(
            "\n* {} — {}\n  {}/{} resources secured",
            quest.name, quest.description, progress.done, progress.total
        ));
        if progress.complete {
            out.push_str(&format!(" — COMPLETE!\n  {}", quest.completion_line));
        }
        out.push('\n');
    }
    out.trim_end().to_string()
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

/// Run Dungeon Crawler Mode: enumerate (read-only) the selected backend's
/// subscription via the `AzRunner` seam, assemble a `DungeonMap`, and then
/// write it to a file, serve it, and/or print a summary, per `args`.
fn run_crawl(args: dungeon_cli::CrawlArgs) {
    let runner: Box<dyn AzRunner> = match args.backend.to_lowercase().as_str() {
        "az" | "real" | "azure" => Box::new(ProcessAzRunner::new()),
        other => {
            if !backend::is_recognized(other) {
                eprintln!(
                    "Warning: unknown backend '{other}'; falling back to the offline mock estate."
                );
            }
            match resolve_crawl_mock_size(&args.mock_size) {
                Some(Ok(params)) => Box::new(backend::mock_gen::fake_runner(&params)),
                Some(Err(e)) => {
                    eprintln!(
                        "Warning: ignoring invalid mock size '{}' ({e}); using the default demo estate.",
                        args.mock_size.as_deref().unwrap_or("")
                    );
                    Box::new(demo_runner())
                }
                None => Box::new(demo_runner()),
            }
        }
    };

    // No signal handler is installed here (that would need an extra
    // dependency such as `ctrlc`, which this feature intentionally avoids).
    // A plain Ctrl-C during enumeration therefore terminates the process the
    // default OS way, same as any other AzZork command; `CancelToken` and
    // `build_cancellable`'s partial-map handling exist so a future signal
    // handler is a drop-in addition, and are already exercised directly by
    // `tests/dungeon_tests.rs`.
    let cancel = dungeon_map::CancelToken::new();

    let dmap = match dungeon_map::build_cancellable(runner.as_ref(), args.budget, &cancel) {
        Ok(m) => m,
        Err(e) => {
            eprintln!(
                "Failed to build dungeon map via {} backend: {}",
                args.backend, e
            );
            eprintln!("Tip: for the `az` backend, make sure `az login` has been run.");
            std::process::exit(1);
        }
    };

    println!(
        "Dungeon assembled: {} room(s), {} resource(s){}.",
        dmap.rooms.len(),
        dmap.resource_count(),
        if dmap.partial {
            " (PARTIAL — cancelled)"
        } else {
            ""
        }
    );

    let html = if args.playwright {
        match playwright::try_render(&dmap) {
            Some(rendered) => rendered,
            None => {
                println!("(--playwright requested but unavailable; using the native renderer.)");
                render::render_html(&dmap)
            }
        }
    } else {
        render::render_html(&dmap)
    };

    if let Some(path) = &args.out {
        if let Err(e) = std::fs::write(path, &html) {
            eprintln!("Failed to write dungeon map to '{path}': {e}");
            std::process::exit(1);
        }
        println!("Wrote dungeon map to {path}");
    }

    if args.serve {
        let bind_addr = format!("127.0.0.1:{}", args.port);
        match server::serve(dmap, &bind_addr) {
            Ok(handle) => {
                println!(
                    "Serving the dungeon map at http://{} (Ctrl-C to stop).",
                    handle.addr()
                );
                // Foreground server: block indefinitely. The OS reclaims the
                // listener and its thread when the process is killed
                // (Ctrl-C), matching the "press Ctrl-C to stop" contract.
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(3600));
                }
            }
            Err(e) => {
                eprintln!("Failed to start the dungeon crawler server: {e}");
                std::process::exit(1);
            }
        }
    } else if args.out.is_none() {
        println!(
            "(Nothing written to disk and no --serve requested; pass --out <path> or --serve to view the map.)"
        );
    }
}

/// Resolve the mock estate size requested for `azork crawl`: an explicit
/// `--mock-size <spec>` flag takes precedence, falling back to the
/// `AZORK_MOCK_SIZE`/`AZORK_MOCK_RGS`/`AZORK_MOCK_RESOURCES_PER_RG`/
/// `AZORK_MOCK_SEED` environment variables (see
/// [`azork::backend::mock_gen::MockSizeParams::from_env`]). Returns `None`
/// when no sizing was requested at all, so the caller uses the small,
/// fixed, hand-authored demo estate unchanged.
fn resolve_crawl_mock_size(
    explicit: &Option<String>,
) -> Option<Result<backend::mock_gen::MockSizeParams, String>> {
    if let Some(spec) = explicit {
        return Some(backend::mock_gen::MockSizeParams::parse(spec));
    }
    backend::mock_gen::MockSizeParams::from_env()
}

/// A small, hardcoded, offline "mock estate" for Dungeon Crawler Mode's
/// default `mock` backend — the crawl-mode analogue of
/// [`azork::backend::mock::MockBackend`], but shaped as canned `az ... -o
/// json` responses so it flows through the exact same [`AzRunner`]-driven
/// [`dungeon_map::build`] path a real subscription would.
fn demo_runner() -> FakeAzRunner {
    const GROUP_LIST_JSON: &str = r#"[
      {"id": "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/entry-hall",
       "location": "eastus", "name": "entry-hall"},
      {"id": "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/vault-chamber",
       "location": "eastus", "name": "vault-chamber"},
      {"id": "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/watchtower",
       "location": "westus2", "name": "watchtower"}
    ]"#;
    const ENTRY_HALL_JSON: &str = r#"[
      {"id": "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/entry-hall/providers/Microsoft.Web/sites/torchbearer",
       "name": "torchbearer", "type": "Microsoft.Web/sites", "location": "eastus"},
      {"id": "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/entry-hall/providers/Microsoft.Network/virtualNetworks/great-corridor",
       "name": "great-corridor", "type": "Microsoft.Network/virtualNetworks", "location": "eastus"}
    ]"#;
    const VAULT_CHAMBER_JSON: &str = r#"[
      {"id": "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/vault-chamber/providers/Microsoft.KeyVault/vaults/hoard",
       "name": "hoard", "type": "Microsoft.KeyVault/vaults", "location": "eastus"},
      {"id": "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/vault-chamber/providers/Microsoft.Storage/storageAccounts/treasury",
       "name": "treasury", "type": "Microsoft.Storage/storageAccounts", "location": "eastus"}
    ]"#;
    const WATCHTOWER_JSON: &str = r#"[
      {"id": "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/watchtower/providers/Microsoft.Compute/virtualMachines/sentinel",
       "name": "sentinel", "type": "Microsoft.Compute/virtualMachines", "location": "westus2"}
    ]"#;

    FakeAzRunner::new()
        .with(&["group", "list", "-o", "json"], GROUP_LIST_JSON)
        .with(
            &[
                "resource",
                "list",
                "--resource-group",
                "entry-hall",
                "-o",
                "json",
            ],
            ENTRY_HALL_JSON,
        )
        .with(
            &[
                "resource",
                "list",
                "--resource-group",
                "vault-chamber",
                "-o",
                "json",
            ],
            VAULT_CHAMBER_JSON,
        )
        .with(
            &[
                "resource",
                "list",
                "--resource-group",
                "watchtower",
                "-o",
                "json",
            ],
            WATCHTOWER_JSON,
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use azork::agent::MAX_INTENT_ECHO_LEN;
    use azork::backend::{mock::MockBackend, Backend};

    #[test]
    fn achievements_report_renders_locked_badges_with_blocker_text() {
        let world = MockBackend::new().build_world().unwrap();
        let out = achievements_report(&world);
        assert!(out.starts_with("Governance posture:"));
        assert!(out.contains("Achievements:"));
        // A fresh mock world has outstanding hazards, so at least one badge
        // must render as locked with its "locked: <reason>" blocker text.
        assert!(out.contains("[ ]"));
        assert!(out.contains("locked:"));
    }

    #[test]
    fn achievements_report_renders_earned_badges_with_checkmarks() {
        use azork::world::{Resource, Room, World};

        let mut resource = Resource::new(
            "storage",
            "Microsoft.Storage/storageAccounts",
            "A well-governed store.",
        );
        resource.locked = true;
        resource.encrypted = true;
        resource.public = false;
        resource.monthly_cost = 10;
        let room = Room::new("rg", "A tidy room.", "eastus", true).with_resource(resource);
        let world = World::new(vec![room], "rg", "sub-test").unwrap();

        let out = achievements_report(&world);
        assert!(out.contains("[x] 🔐 Fort Knox"));
        assert!(out.contains("[x] 🚪 No Open Doors"));
        assert!(out.contains("[x] 🛡️ Warded"));
        assert!(out.contains("[x] 💰 Under Budget"));
        assert!(!out.contains("[ ]"));
    }

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

    /// A very long, unrecognised line of input must not be persisted verbatim
    /// as an "unresolved intent" friction node — regression test for #32
    /// (unbounded growth of memory.graph from a single long line).
    #[test]
    fn long_unresolved_intent_is_truncated_before_persisting() {
        let mut world = MockBackend::new().build_world().unwrap();
        let mut registry = CapabilityRegistry::new();
        let mut memory = GraphMemory::new();
        let runner = FakeAzRunner::new();
        let cache_path = Path::new("/tmp/azork-test-cache-unused");
        let mut lines: std::vec::IntoIter<io::Result<String>> = vec![].into_iter();

        // Nonsense input, well past the truncation cap, with no recognisable
        // az domain keyword so it resolves as `Unresolved` rather than a
        // `LearnHint`.
        let long_input = "qzxjk ".repeat(400); // ~2400 chars, no known keywords
        assert!(long_input.len() > MAX_INTENT_ECHO_LEN * 2);

        let quit = handle(
            &mut world,
            Command::Unknown(long_input.clone()),
            &mut lines,
            &mut registry,
            &mut memory,
            &runner,
            cache_path,
        );
        assert!(!quit);

        let friction_nodes = memory.nodes_of_kind(MemoryKind::Friction);
        assert_eq!(friction_nodes.len(), 1);
        let content = &friction_nodes[0].content;
        // Persisted content must be capped, not the full ~2400-char input.
        assert!(
            content.len() < long_input.len(),
            "expected persisted friction note to be truncated, got {} chars",
            content.len()
        );
        assert!(content.contains("...(truncated)"));
    }
}
