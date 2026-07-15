//! The single seam through which AzZork ever invokes the `az` CLI.
//!
//! Everything that needs to talk to Azure — the live [`crate::backend::az`]
//! backend and the dynamic [`crate::capabilities`] derivation — goes through an
//! [`AzRunner`]. Production code uses [`ProcessAzRunner`], which shells out to
//! the installed `az` binary. Tests inject a [`FakeAzRunner`] with canned
//! responses, so the suite never touches the real CLI or the network.

use std::collections::HashMap;
use std::io::{self, ErrorKind, Read};
use std::process::{Command, Output, Stdio};
use std::sync::mpsc;
use std::thread::sleep;
use std::time::{Duration, Instant};

/// Hard wall-clock timeout for a single `az` invocation. `az` can otherwise
/// block indefinitely (e.g. an interactive device-code/browser login prompt,
/// or a hung network call), which would freeze the whole game with no way
/// out for the player.
const AZ_CALL_TIMEOUT: Duration = Duration::from_secs(30);
/// How often to poll a running `az` child process for completion.
const AZ_POLL_INTERVAL: Duration = Duration::from_millis(50);
/// Upper bound on how long to wait for the stdout/stderr reader threads to
/// finish once the child has exited (or been killed on timeout). This is a
/// safety net, not the expected path: if `az` spawned a grandchild that
/// inherited the piped file descriptors and is still holding them open,
/// `read_to_end` would otherwise block forever. Bounding it here guarantees
/// a call always returns within a predictable wall-clock budget, at the
/// cost of leaking the (now-orphaned) reader thread in that rare case.
const AZ_READER_JOIN_TIMEOUT: Duration = Duration::from_secs(5);

/// Something that can run an `az` invocation and return its raw process output.
///
/// This is intentionally the *only* method: it is the narrow waist that makes
/// "no real az, no network in tests" a property we can enforce by construction.
pub trait AzRunner {
    /// Run `az <args...>` and return the completed process output.
    ///
    /// Returning [`io::Result`] (rather than pre-interpreting success) keeps the
    /// seam dumb: retry/transient classification and stdout parsing live in the
    /// callers, so a fake only has to hand back bytes. A timeout is reported as
    /// an `io::Error` with [`io::ErrorKind::TimedOut`] so callers can treat it
    /// as retryable without depending on message text.
    fn run(&self, args: &[&str]) -> io::Result<Output>;
}

/// Production runner: shells out to the real `az` binary on `PATH`.
///
/// This is the single hardened seam for launching `az`: every caller (the
/// live [`crate::backend::az`] backend and [`crate::capabilities`]
/// derivation) gets the same wall-clock timeout, zombie-process cleanup, and
/// pipe-deadlock protection for free.
#[derive(Debug, Default, Clone, Copy)]
pub struct ProcessAzRunner;

impl ProcessAzRunner {
    pub fn new() -> ProcessAzRunner {
        ProcessAzRunner
    }
}

impl AzRunner for ProcessAzRunner {
    fn run(&self, args: &[&str]) -> io::Result<Output> {
        let mut child = Command::new("az")
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        // Drain stdout/stderr concurrently on background threads, handing
        // each buffer back over a channel rather than joining the thread
        // directly — that lets the caller bound how long it waits below
        // (AZ_READER_JOIN_TIMEOUT) instead of risking an indefinite join if
        // a grandchild process keeps the pipe open. OS pipe buffers are only
        // ~64KB; `az resource list` on a busy subscription can easily exceed
        // that, and without draining while we wait, the child would block
        // inside its own write() the moment the buffer fills — `try_wait()`
        // would then never observe an exit, and a slow-but-successful
        // command would be misreported as a timeout.
        let mut stdout_pipe = child.stdout.take().expect("stdout was piped");
        let mut stderr_pipe = child.stderr.take().expect("stderr was piped");
        let (stdout_tx, stdout_rx) = mpsc::channel();
        let (stderr_tx, stderr_rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = stdout_pipe.read_to_end(&mut buf);
            let _ = stdout_tx.send(buf);
        });
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = stderr_pipe.read_to_end(&mut buf);
            let _ = stderr_tx.send(buf);
        });

        // Single cleanup path for every exit from the poll loop (normal
        // completion, timeout, or a try_wait() error): always attempt to
        // kill+reap the child so no zombie process or leaked reader thread
        // survives a failed/timed-out attempt, especially since these
        // outcomes are retried by the backend's `run()`.
        let deadline = Instant::now() + AZ_CALL_TIMEOUT;
        let (status, timed_out) = loop {
            match child.try_wait() {
                Ok(Some(status)) => break (Some(status), false),
                Ok(None) => {
                    if Instant::now() >= deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        break (None, true);
                    }
                    sleep(AZ_POLL_INTERVAL);
                }
                Err(_e) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break (None, false);
                }
            }
        };

        let stdout = stdout_rx
            .recv_timeout(AZ_READER_JOIN_TIMEOUT)
            .unwrap_or_default();
        let stderr = stderr_rx
            .recv_timeout(AZ_READER_JOIN_TIMEOUT)
            .unwrap_or_default();

        if timed_out {
            return Err(io::Error::new(
                ErrorKind::TimedOut,
                format!(
                    "'az {}' timed out after {}s (killed)",
                    args.join(" "),
                    AZ_CALL_TIMEOUT.as_secs()
                ),
            ));
        }
        let status = match status {
            Some(status) => status,
            None => {
                return Err(io::Error::other(
                    "failed to wait on 'az' process: process could not be reaped",
                ));
            }
        };

        Ok(Output {
            status,
            stdout,
            stderr,
        })
    }
}

/// Test runner: returns canned output keyed by the exact joined arguments.
///
/// Register responses with [`FakeAzRunner::with`]. Any unregistered argument
/// vector yields a non-zero exit and an explanatory stderr, mimicking `az`'s
/// behaviour for an unknown command so callers exercise their error paths.
#[derive(Debug, Default, Clone)]
pub struct FakeAzRunner {
    /// Map of `args.join(" ")` -> (stdout, success).
    responses: HashMap<String, (String, bool)>,
}

impl FakeAzRunner {
    pub fn new() -> FakeAzRunner {
        FakeAzRunner {
            responses: HashMap::new(),
        }
    }

    /// Register a successful stdout response for an exact argument vector.
    pub fn with(mut self, args: &[&str], stdout: &str) -> FakeAzRunner {
        self.responses
            .insert(args.join(" "), (stdout.to_string(), true));
        self
    }

    /// Register a failing (non-zero exit) response for an exact argument vector.
    /// The supplied text is delivered on stderr.
    pub fn with_failure(mut self, args: &[&str], stderr: &str) -> FakeAzRunner {
        self.responses
            .insert(args.join(" "), (stderr.to_string(), false));
        self
    }
}

impl AzRunner for FakeAzRunner {
    fn run(&self, args: &[&str]) -> io::Result<Output> {
        let key = args.join(" ");
        match self.responses.get(&key) {
            Some((body, true)) => Ok(fake_output(body, "", true)),
            Some((body, false)) => Ok(fake_output("", body, false)),
            None => Ok(fake_output(
                "",
                &format!("FakeAzRunner: no canned response for 'az {}'", key),
                false,
            )),
        }
    }
}

/// Build a synthetic [`Output`] without spawning a process.
fn fake_output(stdout: &str, stderr: &str, success: bool) -> Output {
    Output {
        status: exit_status(success),
        stdout: stdout.as_bytes().to_vec(),
        stderr: stderr.as_bytes().to_vec(),
    }
}

/// Construct an [`std::process::ExitStatus`] with the given success-ness.
///
/// `ExitStatus` has no public constructor, so on Unix we build one from a raw
/// wait-status; elsewhere we fall back to actually running a trivial process.
#[cfg(unix)]
fn exit_status(success: bool) -> std::process::ExitStatus {
    use std::os::unix::process::ExitStatusExt;
    // Encode exit code in the high byte, matching wait(2) semantics.
    std::process::ExitStatus::from_raw(if success { 0 } else { 1 << 8 })
}

#[cfg(not(unix))]
fn exit_status(success: bool) -> std::process::ExitStatus {
    // Portable fallback: spawn `true`/`false` to obtain a real ExitStatus.
    let program = if success { "true" } else { "false" };
    Command::new(program)
        .status()
        .unwrap_or_else(|_| panic!("could not synthesise ExitStatus via '{}'", program))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_returns_registered_stdout() {
        let runner = FakeAzRunner::new().with(&["group", "list"], "rg-one\nrg-two");
        let out = runner.run(&["group", "list"]).unwrap();
        assert!(out.status.success());
        assert_eq!(String::from_utf8_lossy(&out.stdout), "rg-one\nrg-two");
    }

    #[test]
    fn fake_unknown_command_fails() {
        let runner = FakeAzRunner::new();
        let out = runner.run(&["does", "not", "exist"]).unwrap();
        assert!(!out.status.success());
        assert!(String::from_utf8_lossy(&out.stderr).contains("no canned response"));
    }

    #[test]
    fn fake_registered_failure_reports_stderr() {
        let runner =
            FakeAzRunner::new().with_failure(&["account", "show"], "Please run 'az login'");
        let out = runner.run(&["account", "show"]).unwrap();
        assert!(!out.status.success());
        assert!(String::from_utf8_lossy(&out.stderr).contains("az login"));
    }
}
