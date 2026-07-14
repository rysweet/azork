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
            let msg = format!("'az {}' failed: {}", args.join(" "), stderr.trim());
            return Err((msg, is_transient(&stderr)));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
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

        let mut groups: Vec<(String, String)> = Vec::new();
        for line in groups_raw.lines() {
            let mut cols = line.split('\t');
            if let Some(name) = cols.next() {
                let loc = cols.next().unwrap_or("unknown").to_string();
                if !name.trim().is_empty() {
                    groups.push((name.trim().to_string(), loc.trim().to_string()));
                }
            }
        }

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
                for line in res_raw.lines() {
                    let mut cols = line.split('\t');
                    if let Some(rname) = cols.next() {
                        let rtype = cols.next().unwrap_or("resource").to_string();
                        if !rname.trim().is_empty() {
                            room = room.with_resource(Resource::new(
                                rname.trim(),
                                rtype.trim(),
                                &format!("A live {} named {}.", rtype.trim(), rname.trim()),
                            ));
                        }
                    }
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
    use super::is_transient;

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
}
