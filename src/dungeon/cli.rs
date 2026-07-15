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
        }
    }
}

/// Whether `arg` (the first positional argument after the `azork` binary
/// name) selects Dungeon Crawler Mode. Accepts both documented aliases,
/// case-sensitively (matching the rest of AzZork's subcommand handling).
pub fn is_crawl_subcommand(arg: &str) -> bool {
    arg == "crawl" || arg == "dungeon"
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
            other => return Err(format!("unknown flag '{other}'")),
        }
        i += 1;
    }
    Ok(parsed)
}
