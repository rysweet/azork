//! AzZork — the Azure control plane reimagined as a Zork-style text adventure.
//!
//! Run with no arguments for the offline mock dungeon (no Azure credentials
//! required). Pass `--backend az` (or set `AZORK_BACKEND=az`) to explore your
//! real subscription via the `az` CLI.

use azork::backend;
use azork::parser::{self, Command};
use azork::update;
use azork::world::{GrueOutcome, World};
use std::io::{self, BufRead, Write};

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
  help / ?                show this help
  quit / q                leave the dungeon

Beware: acting in a dark (unmonitored) room invites a Grue to eat you."#;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // Top-level subcommands / flags handled before the game starts.
    match args.get(1).map(String::as_str) {
        Some("update") => {
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
                 azork update          self-update to the latest release\n  \
                 azork --version       print the version\n\nEnvironment:\n  \
                 AZORK_BACKEND=az      use the live `az` CLI backend\n  \
                 {}=1  disable the startup update check",
                update::NO_UPDATE_CHECK_ENV
            );
            return;
        }
        _ => {}
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

        let cmd = parser::parse(&line);
        let quit = handle(&mut world, cmd, &mut lines);
        if quit {
            break;
        }

        // After each meaningful turn, the Grue may act in the dark.
        run_grue_check(&mut world);
    }

    if world.game_over {
        println!("\n{}", world.score());
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
fn handle<I>(world: &mut World, cmd: Command, lines: &mut I) -> bool
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
        Command::Help => println!("{}", HELP),
        Command::Quit => {
            println!("\nYou step back through the portal.\n{}", world.score());
            return true;
        }
        Command::Unknown(raw) => {
            println!(
                "I don't understand \"{}\". Type 'help' for commands.",
                raw.trim()
            );
        }
    }
    false
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
