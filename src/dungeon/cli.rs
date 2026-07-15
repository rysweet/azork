//! CLI argument parsing for the `azork crawl` / `azork dungeon` subcommand.
//!
//! See `docs/DUNGEON-CRAWLER.md#command-reference`:
//!
//! ```text
//! azork crawl [--backend <id>] [--serve] [--port <n>] [--out <path>]
//!             [--budget <n>] [--playwright]
//! ```

use crate::dungeon::map::DEFAULT_BUDGET;

/// Parsed flags for a `crawl`/`dungeon` invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrawlArgs {
    /// `--backend <id>` / `-b <id>`, default `"mock"` — same backend ids as
    /// the REPL.
    pub backend: String,
    /// `--serve` — start the embedded HTTP server.
    pub serve: bool,
    /// `--port <n>`, default `0` (OS-assigned free port).
    pub port: u16,
    /// `--out <path>` — write the rendered map to a file.
    pub out: Option<String>,
    /// `--budget <n>`, default [`DEFAULT_BUDGET`].
    pub budget: usize,
    /// `--playwright` — best-effort richer render, always degrades
    /// gracefully to the native renderer.
    pub playwright: bool,
    /// `--mock-size <spec>` — request a sized synthetic estate from the
    /// mock backend instead of its small default demo estate. Same grammar
    /// as `AZORK_MOCK_SIZE` (see
    /// [`crate::backend::mock_gen::MockSizeParams::parse`]): a preset name
    /// (`small`/`medium`/`large`/`huge`), a bare resource-group count, or
    /// `COUNTxPER_GROUP`, with an optional `:<seed>` suffix. Ignored for
    /// non-mock backends.
    pub mock_size: Option<String>,
}

impl Default for CrawlArgs {
    fn default() -> CrawlArgs {
        CrawlArgs {
            backend: "mock".to_string(),
            serve: false,
            port: 0,
            out: None,
            budget: DEFAULT_BUDGET,
            playwright: false,
            mock_size: None,
        }
    }
}

/// Whether `arg` (the first positional argument after the `azork` binary
/// name) selects Dungeon Crawler Mode. Accepts both documented aliases,
/// case-sensitively (matching the rest of AzZork's subcommand handling).
pub fn is_crawl_subcommand(arg: &str) -> bool {
    arg == "crawl" || arg == "dungeon"
}

/// Usage/help text for `azork crawl` / `azork dungeon`, printed when
/// `--help`/`-h` is passed and on request exits 0 rather than being treated
/// as an unknown flag.
pub const CRAWL_HELP: &str = r#"azork crawl [--backend <id>] [--serve] [--port <n>] [--out <path>]
            [--budget <n>] [--playwright] [--mock-size <spec>]

Render the Dungeon Crawler map (alias: azork dungeon).

Flags:
  --backend <id>, -b <id>  backend to explore, default "mock" (same ids as the REPL)
  --serve                  start the embedded HTTP server
  --port <n>               port for --serve, default 0 (OS-assigned free port)
  --out <path>             write the rendered map to a file
  --budget <n>             room/exploration budget, default 64
  --playwright             best-effort richer render; degrades gracefully to the
                           native renderer if unavailable
  --mock-size <spec>       generate a sized synthetic mock estate instead of the
                           small default demo estate (mock backend only); spec is
                           a preset (small/medium/large/huge), a resource-group
                           count, or COUNTxPER_GROUP, optionally suffixed with
                           `:<seed>` (e.g. "large", "200", "200x15:7"). Also settable
                           via AZORK_MOCK_SIZE.
  --help, -h               show this help and exit
"#;

/// Whether `args` (the flags following the `crawl`/`dungeon` subcommand)
/// requests help via `--help`/`-h`. Checked ahead of [`parse`] so that help
/// short-circuits regardless of its position among other flags.
pub fn wants_help(args: &[String]) -> bool {
    args.iter().any(|a| a == "--help" || a == "-h")
}

/// Parse the flags following the `crawl`/`dungeon` subcommand name.
///
/// Unknown flags, or a value that fails to parse for a typed flag (e.g. a
/// non-numeric `--port`), return a descriptive `Err` rather than panicking —
/// this is user-facing CLI input, not trusted internal data.
pub fn parse(args: &[String]) -> Result<CrawlArgs, String> {
    let mut parsed = CrawlArgs::default();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                // Handled by `wants_help` before `parse` is called in
                // ordinary CLI dispatch, but guard here too so any direct
                // caller of `parse` doesn't get "unknown flag '--help'".
                return Err("help requested".to_string());
            }
            "--backend" | "-b" => {
                i += 1;
                parsed.backend = args
                    .get(i)
                    .cloned()
                    .ok_or_else(|| "--backend requires a value".to_string())?;
            }
            "--serve" => parsed.serve = true,
            "--port" => {
                i += 1;
                let raw = args
                    .get(i)
                    .ok_or_else(|| "--port requires a value".to_string())?;
                parsed.port = raw
                    .parse::<u16>()
                    .map_err(|_| format!("--port value '{raw}' is not a valid port number"))?;
            }
            "--out" => {
                i += 1;
                parsed.out = Some(
                    args.get(i)
                        .cloned()
                        .ok_or_else(|| "--out requires a value".to_string())?,
                );
            }
            "--budget" => {
                i += 1;
                let raw = args
                    .get(i)
                    .ok_or_else(|| "--budget requires a value".to_string())?;
                parsed.budget = raw
                    .parse::<usize>()
                    .map_err(|_| format!("--budget value '{raw}' is not a valid number"))?;
            }
            "--playwright" => parsed.playwright = true,
            "--mock-size" => {
                i += 1;
                parsed.mock_size = Some(
                    args.get(i)
                        .cloned()
                        .ok_or_else(|| "--mock-size requires a value".to_string())?,
                );
            }
            other => return Err(format!("unknown flag '{other}'")),
        }
        i += 1;
    }
    Ok(parsed)
}
