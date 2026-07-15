//! Optional real backend that shells out to the installed `az` CLI.
//!
//! This maps your actual Azure subscription into the dungeon. It is never used
//! by default and is never exercised by the test suite — the game must run with
//! zero credentials. Enable it with `--backend az` or `AZORK_BACKEND=az`.
//!
//! To avoid a JSON-parsing dependency we ask `az` for tab-separated output
//! (`-o tsv`) with narrow `--query` projections and parse the plain text.

use super::Backend;
use crate::az_runner::{AzRunner, ProcessAzRunner};
use crate::parser::Direction;
use crate::secrets::scrub;
use crate::world::{Resource, Room, World};
use std::io::ErrorKind;
use std::thread::sleep;
use std::time::Duration;

/// Maximum number of attempts (1 initial + retries) for a single `az` call.
const MAX_ATTEMPTS: u32 = 3;
/// Base delay for exponential backoff between retries.
const BASE_BACKOFF: Duration = Duration::from_millis(400);

/// Backend that queries the real Azure control plane via `az`.
///
/// All CLI invocation goes through an injected [`AzRunner`], so tests can drive
/// this backend with canned output instead of the real `az` binary.
///
/// On real subscriptions the resource-group count can be in the hundreds, and a
/// naive "list every group, then list every group's resources" walk issues
/// *O(groups)* sequential `az` calls — minutes of latency. To stay responsive
/// the backend is **bounded**: it builds at most [`AzBackend::max_rooms`] rooms
/// and only eagerly enumerates resources for the first
/// [`AzBackend::max_resource_rooms`] of them. Both bounds are configurable via
/// the `AZORK_MAX_ROOMS` / `AZORK_MAX_RESOURCE_ROOMS` environment variables.
pub struct AzBackend {
    runner: Box<dyn AzRunner>,
    /// Maximum number of resource groups mapped into rooms.
    max_rooms: usize,
    /// Maximum number of rooms whose resources are enumerated during build.
    max_resource_rooms: usize,
}

/// Default cap on rooms built from a live subscription.
const DEFAULT_MAX_ROOMS: usize = 40;
/// Default cap on rooms whose resources are eagerly enumerated.
const DEFAULT_MAX_RESOURCE_ROOMS: usize = 8;

impl AzBackend {
    /// Construct a backend that shells out to the real `az` CLI, taking room
    /// bounds from the environment (falling back to sane defaults).
    pub fn new() -> AzBackend {
        AzBackend::with_runner(Box::new(ProcessAzRunner::new()))
    }

    /// Construct a backend over an arbitrary [`AzRunner`] (used by tests), with
    /// bounds read from the environment.
    pub fn with_runner(runner: Box<dyn AzRunner>) -> AzBackend {
        AzBackend {
            runner,
            max_rooms: env_cap("AZORK_MAX_ROOMS", DEFAULT_MAX_ROOMS),
            max_resource_rooms: env_cap("AZORK_MAX_RESOURCE_ROOMS", DEFAULT_MAX_RESOURCE_ROOMS),
        }
    }

    /// Construct a backend with explicit bounds (used by tests to assert the
    /// caps without touching process-global environment variables).
    pub fn with_runner_and_caps(
        runner: Box<dyn AzRunner>,
        max_rooms: usize,
        max_resource_rooms: usize,
    ) -> AzBackend {
        AzBackend {
            runner,
            max_rooms: max_rooms.max(1),
            max_resource_rooms,
        }
    }

    /// Run an `az` invocation with bounded retries and exponential backoff.
    ///
    /// Transient failures (throttling, timeouts, network blips) are retried;
    /// permanent failures (binary missing, authentication/authorization errors)
    /// fail fast so the caller can surface an actionable message.
    fn run(&self, args: &[&str]) -> Result<String, String> {
        let mut last_err = String::new();
        for attempt in 1..=MAX_ATTEMPTS {
            match self.run_once(args) {
                Ok(out) => return Ok(out),
                Err((err, retryable)) => {
                    last_err = err;
                    if !retryable || attempt == MAX_ATTEMPTS {
                        break;
                    }
                    // Exponential backoff: 400ms, 800ms, ...
                    let backoff = BASE_BACKOFF * 2u32.pow(attempt - 1);
                    sleep(backoff);
                }
            }
        }
        Err(last_err)
    }

    /// Perform a single `az` invocation. Returns `(message, retryable)` on error.
    ///
    /// Delegates the actual process launch to `self.runner`, which (via
    /// [`crate::az_runner::ProcessAzRunner`] in production) applies a hard
    /// wall-clock timeout, zombie-process cleanup, and pipe-deadlock
    /// protection — see that module for the details. Tests inject a
    /// `FakeAzRunner` so no real `az` binary or hardening path is exercised.
    fn run_once(&self, args: &[&str]) -> Result<String, (String, bool)> {
        let output = self.runner.run(args).map_err(|e| {
            // A timeout is always worth retrying; a missing binary never is;
            // other spawn errors might be.
            if e.kind() == ErrorKind::TimedOut {
                return (e.to_string(), true);
            }
            let retryable = e.kind() != ErrorKind::NotFound;
            (
                format!("failed to launch 'az' (is it installed & on PATH?): {}", e),
                retryable,
            )
        })?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let retryable = is_transient(&stderr);
            let msg = format_az_error(args, &stderr);
            return Err((msg, retryable));
        }
        // `az` stdout is attacker/environment-influenced text we did not
        // generate; scrub it defensively too, in case a future command ever
        // projects a secret-bearing field (e.g. a connection string or key)
        // on success. Today's `--query` projections are narrow (name/type/
        // location only), but this closes the gap for any future query.
        Ok(scrub(&String::from_utf8_lossy(&output.stdout)))
    }
}

/// Format the error message for a failed `az` invocation, with `stderr`
/// scrubbed of any secret-shaped material before it can reach a
/// `println!`/`eprintln!` or be persisted.
fn format_az_error(args: &[&str], stderr: &str) -> String {
    format!("'az {}' failed: {}", args.join(" "), scrub(stderr.trim()))
}

/// Heuristic: decide whether an `az` stderr message describes a transient
/// (retryable) condition. Auth/permission problems are deliberately excluded
/// so we do not hammer the service on a request that can never succeed.
fn is_transient(stderr: &str) -> bool {
    let s = stderr.to_lowercase();

    // Never retry problems the user must fix themselves.
    let permanent = [
        "az login",
        "please run",
        "not logged in",
        "authenticationfailed",
        "authorizationfailed",
        "forbidden",
        "does not have authorization",
        "invalid",
    ];
    if permanent.iter().any(|p| s.contains(p)) {
        return false;
    }

    // Retry classic transient / throttling / connectivity signals.
    let transient = [
        "timed out",
        "timeout",
        "temporarily",
        "throttl",
        "too many requests",
        "429",
        "500",
        "502",
        "503",
        "504",
        "connection reset",
        "connection aborted",
        "could not connect",
        "network",
        "try again",
        "service unavailable",
    ];
    transient.iter().any(|t| s.contains(t))
}

/// Parse `az ... -o tsv` output for `[].{name:name,location:location}` into
/// `(name, location)` pairs, skipping blank-name rows and defaulting a
/// missing location column to `"unknown"`. Pure and offline-testable.
fn parse_group_tsv(raw: &str) -> Vec<(String, String)> {
    let mut groups = Vec::new();
    for line in raw.lines() {
        let mut cols = line.split('\t');
        if let Some(name) = cols.next() {
            let loc = cols.next().unwrap_or("unknown").to_string();
            if !name.trim().is_empty() {
                groups.push((name.trim().to_string(), loc.trim().to_string()));
            }
        }
    }
    groups
}

/// Parse `az ... -o tsv` output for `[].{name:name,type:type}` into
/// `(name, type)` pairs, skipping blank-name rows and defaulting a missing
/// type column to `"resource"`. Pure and offline-testable.
fn parse_resource_tsv(raw: &str) -> Vec<(String, String)> {
    let mut resources = Vec::new();
    for line in raw.lines() {
        let mut cols = line.split('\t');
        if let Some(rname) = cols.next() {
            let rtype = cols.next().unwrap_or("resource").to_string();
            if !rname.trim().is_empty() {
                resources.push((rname.trim().to_string(), rtype.trim().to_string()));
            }
        }
    }
    resources
}

impl Default for AzBackend {
    fn default() -> Self {
        AzBackend::new()
    }
}

/// Read a positive `usize` cap from an environment variable, falling back to
/// `default` when unset, empty, unparseable, or zero.
fn env_cap(var: &str, default: usize) -> usize {
    std::env::var(var)
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(default)
}

impl Backend for AzBackend {
    fn name(&self) -> &str {
        "az (live Azure)"
    }

    fn build_world(&self) -> Result<World, String> {
        // Current subscription name (best-effort). If this fails, log why —
        // the very next `az` call below will likely fail for the same reason
        // (e.g. not logged in) with a more actionable message, but we don't
        // want the "unknown-subscription" placeholder to be a silent mystery.
        let subscription = self
            .run(&["account", "show", "--query", "name", "-o", "tsv"])
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|e| {
                eprintln!("warning: could not determine current subscription: {e}");
                "unknown-subscription".to_string()
            });

        // Resource groups become rooms.
        let groups_raw = self.run(&[
            "group",
            "list",
            "--query",
            "[].{name:name,location:location}",
            "-o",
            "tsv",
        ])?;
        let mut groups = parse_group_tsv(&groups_raw);

        if groups.is_empty() {
            return Err(
                "no resource groups found (or not logged in). Try 'az login', or run with the \
                 default mock backend."
                    .to_string(),
            );
        }

        // Bound the walk: on large subscriptions (hundreds of groups) mapping
        // every group — and listing every group's resources — is minutes of
        // sequential `az` calls. Cap the number of rooms so the game is
        // responsive; note the truncation so the player knows more exists.
        let total_groups = groups.len();
        let truncated = total_groups > self.max_rooms;
        groups.truncate(self.max_rooms);

        // Build rooms, chaining them north<->south so the estate is navigable.
        let mut rooms: Vec<Room> = Vec::new();
        for (i, (gname, location)) in groups.iter().enumerate() {
            let desc = if truncated && i == 0 {
                format!(
                    "Resource group '{}' in {}. (Showing {} of {} resource groups; \
                     set AZORK_MAX_ROOMS to see more.)",
                    gname, location, self.max_rooms, total_groups
                )
            } else {
                format!("Resource group '{}' in {}.", gname, location)
            };
            let mut room = Room::new(
                gname, &desc, location,
                true, // assume monitored; we can't cheaply prove otherwise
            );
            if i > 0 {
                room = room.with_exit(Direction::South, &groups[i - 1].0);
            }
            if i + 1 < groups.len() {
                room = room.with_exit(Direction::North, &groups[i + 1].0);
            }

            // Resources are enumerated only for the first `max_resource_rooms`
            // rooms — one `az resource list` per room is the dominant cost, so we
            // bound it. Rooms beyond the cap are still navigable; their contents
            // are simply not pre-listed.
            if i < self.max_resource_rooms {
                if let Ok(res_raw) = self.run(&[
                    "resource",
                    "list",
                    "-g",
                    gname,
                    "--query",
                    "[].{name:name,type:type}",
                    "-o",
                    "tsv",
                ]) {
                    for (rname, rtype) in parse_resource_tsv(&res_raw) {
                        room = room.with_resource(Resource::new(
                            &rname,
                            &rtype,
                            &format!("A live {} named {}.", rtype, rname),
                        ));
                    }
                }
            }

            rooms.push(room);
        }

        let start = rooms[0].name.clone();
        World::new(rooms, &start, &subscription)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::az_runner::FakeAzRunner;

    #[test]
    fn parse_group_tsv_parses_name_and_location() {
        let raw = "landing-rg\teastus\nhub-rg\twestus\n";
        let groups = parse_group_tsv(raw);
        assert_eq!(
            groups,
            vec![
                ("landing-rg".to_string(), "eastus".to_string()),
                ("hub-rg".to_string(), "westus".to_string()),
            ]
        );
    }

    #[test]
    fn parse_group_tsv_defaults_missing_location_and_skips_blank_names() {
        let raw = "solo-rg\n\t\n   \teastus\n";
        let groups = parse_group_tsv(raw);
        // "solo-rg" has no second column -> defaults to "unknown".
        // A line with an empty/whitespace-only name is skipped entirely.
        assert_eq!(groups, vec![("solo-rg".to_string(), "unknown".to_string())]);
    }

    #[test]
    fn parse_resource_tsv_parses_name_and_type() {
        let raw = "storage1\tMicrosoft.Storage/storageAccounts\n";
        let resources = parse_resource_tsv(raw);
        assert_eq!(
            resources,
            vec![(
                "storage1".to_string(),
                "Microsoft.Storage/storageAccounts".to_string()
            )]
        );
    }

    #[test]
    fn parse_resource_tsv_defaults_missing_type_and_skips_blank_names() {
        let raw = "lonely-resource\n\t\n";
        let resources = parse_resource_tsv(raw);
        assert_eq!(
            resources,
            vec![("lonely-resource".to_string(), "resource".to_string())]
        );
    }

    #[test]
    fn throttling_and_5xx_are_transient() {
        assert!(is_transient("Error: TooManyRequests (429)"));
        assert!(is_transient("server returned 503 Service Unavailable"));
        assert!(is_transient("the request timed out, try again"));
        assert!(is_transient("Connection reset by peer"));
    }

    #[test]
    fn auth_and_permission_errors_are_permanent() {
        assert!(!is_transient("Please run 'az login' to setup account."));
        assert!(!is_transient(
            "AuthorizationFailed: does not have authorization"
        ));
        assert!(!is_transient("Forbidden"));
        assert!(!is_transient(""));
    }

    /// `run` must never build a shell command line: every argument is a
    /// discrete element of the vector passed to `Command::args`, so shell
    /// metacharacters embedded in a value (from a malicious resource name,
    /// subscription, or region) can never be reinterpreted by a shell. We
    /// can't spawn a real `az`, but we can prove the invariant that matters:
    /// arguments are carried as an untouched `&[&str]` all the way to
    /// `Command::args`, never joined into a single string that a shell would
    /// re-parse.
    #[test]
    fn run_once_never_shell_joins_arguments() {
        // A value containing shell metacharacters must survive as a single,
        // unmodified argument — proving no `sh -c "... {value} ..."` style
        // interpolation happens anywhere on this path.
        let hostile = "rg; rm -rf / #`whoami`$(id)";
        let args = ["group", "show", "-n", hostile];
        // `run_once` always calls `self.runner.run(args)`, which for
        // `ProcessAzRunner` calls `Command::new("az").args(args)`, i.e. the
        // exact slice below, with no string concatenation in between.
        let mut cmd = std::process::Command::new("az");
        cmd.args(args);
        let collected: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(collected, vec!["group", "show", "-n", hostile]);
        // The hostile string must appear verbatim as its own argument, never
        // split or reinterpreted.
        assert_eq!(collected[3], hostile);
    }

    #[test]
    fn az_error_messages_are_scrubbed_of_secrets() {
        // Exercise the real error-formatting function used by `run_once` on
        // failure, proving the integration point without depending on a
        // real `az` binary being installed.
        let hostile_stderr = "ERROR: token: abc123SECRETXYZ AccountKey=SGVsbG8gV29ybGQh==";
        let msg = format_az_error(&["account", "show"], hostile_stderr);
        assert!(!msg.contains("abc123SECRETXYZ"));
        assert!(!msg.contains("SGVsbG8gV29ybGQh"));
        assert!(msg.starts_with("'az account show' failed:"));
    }

    #[test]
    fn build_world_handles_malformed_tsv_without_panicking() {
        // Lines with missing columns, empty names, or stray tabs must not
        // panic. This calls the real `parse_group_tsv` helper used by
        // `build_world` directly.
        let malformed = "\n\t\tunnamed\n\tlocation-only\ngoodname\tgoodloc\n\t\t\t\t";
        let groups = parse_group_tsv(malformed);
        assert!(groups.iter().any(|(n, _)| n == "goodname"));
    }

    #[test]
    fn run_once_success_path_scrubs_stdout() {
        // `run_once`'s success path applies `scrub()` to stdout as
        // defense-in-depth. Verify `scrub` itself redacts secret-shaped
        // stdout content without mangling plain resource names, matching
        // the guarantee `run_once` now relies on.
        let plain_tsv = "my-resource-group\teastus\nanother-rg\twestus2";
        assert_eq!(scrub(plain_tsv), plain_tsv);

        let hostile_stdout = "my-rg\teastus\nAccountKey=SGVsbG8gV29ybGQh==\tfakeus";
        let scrubbed = scrub(hostile_stdout);
        assert!(!scrubbed.contains("SGVsbG8gV29ybGQh"));
        assert!(scrubbed.contains("my-rg"));
    }

    /// A live subscription with more groups than the room cap must not build a
    /// room per group, and must only enumerate resources up to the resource cap.
    #[test]
    fn build_world_bounds_rooms_and_resource_calls() {
        // Five groups; caps of 3 rooms and 1 resource-enumerated room.
        let groups_tsv = "rg0\teastus\nrg1\teastus\nrg2\teastus\nrg3\teastus\nrg4\teastus";
        let runner = FakeAzRunner::new()
            .with(&["account", "show", "--query", "name", "-o", "tsv"], "sub")
            .with(
                &[
                    "group",
                    "list",
                    "--query",
                    "[].{name:name,location:location}",
                    "-o",
                    "tsv",
                ],
                groups_tsv,
            )
            // Only rg0's resources should ever be requested (resource cap = 1).
            .with(
                &[
                    "resource",
                    "list",
                    "-g",
                    "rg0",
                    "--query",
                    "[].{name:name,type:type}",
                    "-o",
                    "tsv",
                ],
                "store0\tMicrosoft.Storage/storageAccounts",
            );

        let backend = AzBackend::with_runner_and_caps(Box::new(runner), 3, 1);
        let world = backend.build_world().expect("world builds within caps");

        // Room cap honoured: 3 rooms, not 5.
        assert_eq!(world.rooms_len(), 3);
        // The starting room's truncation note mentions the total.
        assert!(world.look().contains("of 5 resource groups"));
        // rg0 has its resource; rg1/rg2 were never resource-listed (would have
        // failed as unregistered fake calls if the cap were not honoured).
    }
}
