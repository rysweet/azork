//! tests/cli_args_tests.rs
//!
//! Outside-in QA for `azork`'s top-level CLI argument handling (issue #33):
//! an unrecognized subcommand or flag must be a **usage error** — a one-line
//! diagnostic on stderr and exit code `2` — rather than silently falling
//! through to the interactive REPL. This drives the actual compiled `azork`
//! binary as a subprocess, the same way a human/CI would invoke it, and
//! asserts on its externally-observable contract.

use std::path::PathBuf;
use std::process::{Command, Stdio};

fn azork_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_azork"))
}

/// Run `azork` with the given args, feeding it empty stdin (so it never
/// blocks on a prompt if it *does* fall through to the REPL) and disabling
/// the startup update-check network call.
fn run(args: &[&str]) -> std::process::Output {
    let mut cmd = Command::new(azork_binary());
    cmd.args(args)
        .env("AZORK_NO_UPDATE_CHECK", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().expect("failed to spawn azork binary");
    // Close stdin immediately (empty input) so a REPL fallback exits fast
    // instead of hanging the test.
    drop(child.stdin.take());
    child.wait_with_output().expect("failed to wait on azork")
}

#[test]
fn unknown_subcommand_is_a_usage_error() {
    let out = run(&["totally-bogus-subcommand"]);
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("unknown subcommand 'totally-bogus-subcommand'"),
        "stderr was: {stderr}"
    );
    assert!(stderr.contains("Try 'azork --help' for usage."));
    assert!(String::from_utf8_lossy(&out.stdout).is_empty());
}

#[test]
fn unknown_flag_is_a_usage_error() {
    let out = run(&["--this-flag-does-not-exist"]);
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("unknown flag '--this-flag-does-not-exist'"),
        "stderr was: {stderr}"
    );
    assert!(stderr.contains("Try 'azork --help' for usage."));
}

#[test]
fn bare_help_is_not_a_recognized_subcommand() {
    // `help` (no dashes) is not a subcommand -- only `--help`/`-h` are.
    let out = run(&["help"]);
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("unknown subcommand 'help'"),
        "stderr was: {stderr}"
    );
}

#[test]
fn dashdash_help_is_recognized() {
    let out = run(&["--help"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stdout).contains("AzZork"));
    assert!(out.stderr.is_empty());
}

#[test]
fn dash_h_is_recognized() {
    let out = run(&["-h"]);
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn dashdash_version_is_recognized() {
    let out = run(&["--version"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stdout).contains("azork"));
}

#[test]
fn dash_capital_v_is_recognized() {
    let out = run(&["-V"]);
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn bare_version_subcommand_is_recognized() {
    let out = run(&["version"]);
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn backend_flag_with_space_is_recognized() {
    let out = run(&["--backend", "mock"]);
    // Falls through to launching the REPL against empty stdin, so it should
    // exit cleanly rather than being rejected as a usage error.
    assert_ne!(out.status.code(), Some(2));
}

#[test]
fn backend_short_flag_is_recognized() {
    let out = run(&["-b", "mock"]);
    assert_ne!(out.status.code(), Some(2));
}

#[test]
fn backend_flag_with_equals_is_recognized() {
    let out = run(&["--backend=mock"]);
    assert_ne!(out.status.code(), Some(2));
}

#[test]
fn missing_backend_value_is_a_usage_error() {
    let out = run(&["--backend"]);
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("missing value for '--backend'"),
        "stderr was: {stderr}"
    );
}

#[test]
fn crawl_subcommand_is_not_rejected_as_unknown() {
    // `crawl` has its own argument parser and error format; it must not be
    // caught by the top-level "unknown subcommand" rejection.
    let out = run(&["crawl", "--this-flag-does-not-exist-either"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("azork: unknown subcommand 'crawl'"),
        "stderr was: {stderr}"
    );
}

#[test]
fn update_subcommand_is_not_rejected_as_unknown() {
    let out = run(&["update", "--check"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("azork: unknown subcommand 'update'"),
        "stderr was: {stderr}"
    );
}

#[test]
fn no_args_launches_without_usage_error() {
    let out = run(&[]);
    assert_ne!(out.status.code(), Some(2));
}
