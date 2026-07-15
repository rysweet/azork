//! The single seam through which AzZork ever invokes the `az` CLI.
//!
//! Everything that needs to talk to Azure — the live [`crate::backend::az`]
//! backend and the dynamic [`crate::capabilities`] derivation — goes through an
//! [`AzRunner`]. Production code uses [`ProcessAzRunner`], which shells out to
//! the installed `az` binary. Tests inject a [`FakeAzRunner`] with canned
//! responses, so the suite never touches the real CLI or the network.

use std::collections::HashMap;
use std::io;
use std::process::{Command, Output};

/// Something that can run an `az` invocation and return its raw process output.
///
/// This is intentionally the *only* method: it is the narrow waist that makes
/// "no real az, no network in tests" a property we can enforce by construction.
pub trait AzRunner {
    /// Run `az <args...>` and return the completed process output.
    ///
    /// Returning [`io::Result`] (rather than pre-interpreting success) keeps the
    /// seam dumb: retry/transient classification and stdout parsing live in the
    /// callers, so a fake only has to hand back bytes.
    fn run(&self, args: &[&str]) -> io::Result<Output>;
}

/// Production runner: shells out to the real `az` binary on `PATH`.
#[derive(Debug, Default, Clone, Copy)]
pub struct ProcessAzRunner;

impl ProcessAzRunner {
    pub fn new() -> ProcessAzRunner {
        ProcessAzRunner
    }
}

impl AzRunner for ProcessAzRunner {
    fn run(&self, args: &[&str]) -> io::Result<Output> {
        Command::new("az").args(args).output()
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
