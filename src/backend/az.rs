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
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

/// Maximum number of attempts (1 initial + retries) for a single `az` call.
const MAX_ATTEMPTS: u32 = 3;
/// Base delay for exponential backoff between retries.
const BASE_BACKOFF: Duration = Duration::from_millis(400);
/// Hard wall-clock timeout for a single `az` invocation. `az` can otherwise
/// block indefinitely (e.g. an interactive device-code/browser login prompt,
/// or a hung network call), which would freeze the whole game with no way
/// out for the player.
const AZ_CALL_TIMEOUT: Duration = Duration::from_secs(30);
/// How often to poll a running `az` child process for completion.
const AZ_POLL_INTERVAL: Duration = Duration::from_millis(50);

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
        use std::io::Read;

        let mut child = Command::new("az")
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                // A missing binary is never transient; other spawn errors might be.
                let retryable = e.kind() != ErrorKind::NotFound;
                (
                    format!("failed to launch 'az' (is it installed & on PATH?): {}", e),
                    retryable,
                )
            })?;

        // Drain stdout/stderr concurrently on background threads. The pipes'
        // OS buffers are only ~64KB; `az resource list` on a busy
        // subscription can easily exceed that, and without draining while we
        // wait, the child would block inside its own write() the moment the
        // buffer fills — `try_wait()` would then never observe an exit, and
        // a slow-but-successful command would be misreported as a timeout.
        let mut stdout_pipe = child.stdout.take().expect("stdout was piped");
        let mut stderr_pipe = child.stderr.take().expect("stderr was piped");
        let stdout_thread = std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = stdout_pipe.read_to_end(&mut buf);
            buf
        });
        let stderr_thread = std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = stderr_pipe.read_to_end(&mut buf);
            buf
        });

        let deadline = Instant::now() + AZ_CALL_TIMEOUT;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) => {
                    if Instant::now() >= deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        break None;
                    }
                    sleep(AZ_POLL_INTERVAL);
                }
                Err(e) => {
                    return Err((format!("failed to wait on 'az' process: {}", e), true));
                }
            }
        };

        // Join the reader threads regardless of outcome: killing the child
        // closes its end of the pipes, so `read_to_end` returns promptly.
        let stdout = stdout_thread.join().unwrap_or_default();
        let stderr = stderr_thread.join().unwrap_or_default();

        let status = match status {
            Some(status) => status,
            None => {
                return Err((
                    format!(
                        "'az {}' timed out after {}s (killed)",
                        args.join(" "),
                        AZ_CALL_TIMEOUT.as_secs()
                    ),
                    true,
                ));
            }
        };

        if !status.success() {
            let stderr = String::from_utf8_lossy(&stderr);
            let msg = format!("'az {}' failed: {}", args.join(" "), stderr.trim());
            return Err((msg, is_transient(&stderr)));
        }
        Ok(String::from_utf8_lossy(&stdout).into_owned())
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
        let groups = parse_group_tsv(&groups_raw);

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
                for (rname, rtype) in parse_resource_tsv(&res_raw) {
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
        World::new(rooms, &start, &subscription)
    }
}

#[cfg(test)]
mod tests {
    use super::{is_transient, parse_group_tsv, parse_resource_tsv};

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
}
