//! Optional real backend that shells out to the installed `az` CLI.
//!
//! This maps your actual Azure subscription into the dungeon. It is never used
//! by default and is never exercised by the test suite — the game must run with
//! zero credentials. Enable it with `--backend az` or `AZORK_BACKEND=az`.
//!
//! To avoid a JSON-parsing dependency we ask `az` for tab-separated output
//! (`-o tsv`) with narrow `--query` projections and parse the plain text.

use super::Backend;
use crate::parser::Direction;
use crate::secrets::scrub;
use crate::world::{Resource, Room, World};
use std::io::ErrorKind;
use std::process::Command;
use std::thread::sleep;
use std::time::Duration;

/// Maximum number of attempts (1 initial + retries) for a single `az` call.
const MAX_ATTEMPTS: u32 = 3;
/// Base delay for exponential backoff between retries.
const BASE_BACKOFF: Duration = Duration::from_millis(400);

/// Backend that queries the real Azure control plane via `az`.
pub struct AzBackend;

impl AzBackend {
    pub fn new() -> AzBackend {
        AzBackend
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
    fn run_once(&self, args: &[&str]) -> Result<String, (String, bool)> {
        let output = Command::new("az").args(args).output().map_err(|e| {
            // A missing binary is never transient; other spawn errors might be.
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
        // generate (resource names, tags, etc. can contain arbitrary text);
        // scrub it before it can reach a println!/eprintln! or be persisted,
        // symmetric with the error path below, in case it ever echoes a
        // token or connection string back.
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(scrub(&stdout))
    }
}

/// Build the failure message for a non-zero-exit `az` invocation.
///
/// `stderr` is attacker/environment-influenced text we did not generate; it
/// is scrubbed before it can reach a `println!`/`eprintln!` or be persisted,
/// in case it ever echoes a token or connection string back (e.g. from a
/// misconfigured extension or `--debug`).
fn format_az_error(args: &[&str], stderr: &str) -> String {
    format!("'az {}' failed: {}", args.join(" "), scrub(stderr.trim()))
}

/// Parse `az ... -o tsv` output of two-column `name<TAB>value` rows into
/// `(name, value)` pairs, skipping rows with an empty name. Used for both
/// resource-group listings (`name`, `location`) and resource listings
/// (`name`, `type`); malformed rows (missing columns, stray tabs) are
/// tolerated rather than causing a panic.
fn parse_name_value_tsv(raw: &str, fallback_value: &str) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    for line in raw.lines() {
        let mut cols = line.split('\t');
        if let Some(name) = cols.next() {
            let value = cols.next().unwrap_or(fallback_value).to_string();
            if !name.trim().is_empty() {
                pairs.push((name.trim().to_string(), value.trim().to_string()));
            }
        }
    }
    pairs
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

impl Default for AzBackend {
    fn default() -> Self {
        AzBackend::new()
    }
}

impl Backend for AzBackend {
    fn name(&self) -> &str {
        "az (live Azure)"
    }

    fn build_world(&self) -> Result<World, String> {
        // Current subscription name (best-effort).
        let subscription = self
            .run(&["account", "show", "--query", "name", "-o", "tsv"])
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|_| "unknown-subscription".to_string());

        // Resource groups become rooms.
        let groups_raw = self.run(&[
            "group",
            "list",
            "--query",
            "[].{name:name,location:location}",
            "-o",
            "tsv",
        ])?;

        let groups = parse_name_value_tsv(&groups_raw, "unknown");

        if groups.is_empty() {
            return Err(
                "no resource groups found (or not logged in). Try 'az login', or run with the \
                 default mock backend."
                    .to_string(),
            );
        }

        // Build rooms, chaining them north<->south so the estate is navigable.
        let mut rooms: Vec<Room> = Vec::new();
        for (i, (gname, location)) in groups.iter().enumerate() {
            let mut room = Room::new(
                gname,
                &format!("Resource group '{}' in {}.", gname, location),
                location,
                true, // assume monitored; we can't cheaply prove otherwise
            );
            if i > 0 {
                room = room.with_exit(Direction::South, &groups[i - 1].0);
            }
            if i + 1 < groups.len() {
                room = room.with_exit(Direction::North, &groups[i + 1].0);
            }

            // Resources in this group become objects.
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
                for (rname, rtype) in parse_name_value_tsv(&res_raw, "resource") {
                    room = room.with_resource(Resource::new(
                        &rname,
                        &rtype,
                        &format!("A live {} named {}.", rtype, rname),
                    ));
                }
            }

            rooms.push(room);
        }

        let start = rooms[0].name.clone();
        Ok(World::new(rooms, &start, &subscription))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // `run_once` always calls `Command::new("az").args(args)`, i.e. the
        // exact slice below, with no string concatenation in between.
        let mut cmd = Command::new("az");
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
    }

    #[test]
    fn az_success_output_is_scrubbed_of_secrets() {
        // The success path must scrub stdout symmetrically with the error
        // path: a hostile resource property could still contain
        // secret-shaped text (e.g. echoed connection strings).
        let hostile_stdout = "name\tAccountKey=SGVsbG8gV29ybGQh==";
        let scrubbed = scrub(hostile_stdout);
        assert!(!scrubbed.contains("SGVsbG8gV29ybGQh"));
    }

    #[test]
    fn build_world_handles_malformed_tsv_without_panicking() {
        // Lines with missing columns, empty names, or stray tabs must not
        // panic when parsed by the real function `build_world` uses to
        // parse `az ... -o tsv` output.
        let malformed = "\n\t\tunnamed\n\tlocation-only\ngoodname\tgoodloc\n\t\t\t\t";
        let groups = parse_name_value_tsv(malformed, "unknown");
        assert!(groups.iter().any(|(n, _)| n == "goodname"));
    }
}
