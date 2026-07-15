//! tests/cli_args_tests.rs
//!
//! Outside-in coverage for the top-level `azork` CLI argument parsing
//! (see `src/main.rs`). Drives the actual compiled `azork` binary as a
//! subprocess and asserts on its externally-observable contract: exit code
//! and stderr/stdout content.
//!
//! Regression coverage for https://github.com/rysweet/azork/issues/33:
//! unrecognized subcommands and flags used to be silently ignored, falling
//! through to launch the interactive mock-backend game with exit code 0.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// Path to the compiled `azork` binary, built by Cargo's test harness
/// (`CARGO_BIN_EXE_<name>` is set automatically for binaries defined via
/// `[[bin]]` in Cargo.toml).
fn azork_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_azork"))
}

#[test]
fn unknown_subcommand_errors_with_nonzero_exit() {
    let out = Command::new(azork_binary())
        .arg("totally-bogus-subcommand")
        .output()
        .expect("failed to spawn azork binary");

    assert!(
        !out.status.success(),
        "unknown subcommand must exit non-zero, got status {:?}",
        out.status
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("unknown subcommand") && stderr.contains("totally-bogus-subcommand"),
        "expected an unknown-subcommand error on stderr, got: {stderr:?}"
    );
    assert!(
        stderr.contains("azork --help"),
        "expected a usage hint pointing at --help, got: {stderr:?}"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains("AzZork"),
        "the game banner must NOT print for an unknown subcommand, got stdout: {stdout:?}"
    );
}

#[test]
fn unknown_flag_errors_with_nonzero_exit() {
    let out = Command::new(azork_binary())
        .arg("--this-flag-does-not-exist")
        .output()
        .expect("failed to spawn azork binary");

    assert!(
        !out.status.success(),
        "unknown flag must exit non-zero, got status {:?}",
        out.status
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("unknown flag") && stderr.contains("--this-flag-does-not-exist"),
        "expected an unknown-flag error on stderr, got: {stderr:?}"
    );
    assert!(
        stderr.contains("azork --help"),
        "expected a usage hint pointing at --help, got: {stderr:?}"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains("AzZork"),
        "the game banner must NOT print for an unknown flag, got stdout: {stdout:?}"
    );
}

#[test]
fn bare_help_word_is_not_a_documented_subcommand() {
    // Only `--help`/`-h` are documented; the bare word `help` is not a
    // top-level subcommand and must be rejected like any other unknown one.
    let out = Command::new(azork_binary())
        .arg("help")
        .output()
        .expect("failed to spawn azork binary");

    assert!(
        !out.status.success(),
        "bare 'help' must exit non-zero, got status {:?}",
        out.status
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("unknown subcommand") && stderr.contains("'help'"),
        "expected an unknown-subcommand error for bare 'help', got: {stderr:?}"
    );
}

#[test]
fn help_flag_still_exits_zero_and_prints_help() {
    for flag in ["--help", "-h"] {
        let out = Command::new(azork_binary())
            .arg(flag)
            .output()
            .expect("failed to spawn azork binary");
        assert!(
            out.status.success(),
            "{flag} must exit zero, got status {:?}",
            out.status
        );
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            stdout.contains("Subcommands:"),
            "{flag} should print usage/help text, got: {stdout:?}"
        );
    }
}

#[test]
fn version_flag_still_exits_zero() {
    let out = Command::new(azork_binary())
        .arg("--version")
        .output()
        .expect("failed to spawn azork binary");
    assert!(
        out.status.success(),
        "--version must exit zero, got status {:?}",
        out.status
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("azork"),
        "expected version string on stdout, got: {stdout:?}"
    );
}

#[test]
fn no_args_still_launches_mock_game() {
    // No arguments should still launch the interactive mock-backend game.
    // Feed 'quit' on stdin so the process exits promptly instead of blocking
    // on further input.
    let mut child = Command::new(azork_binary())
        .env("AZORK_NO_UPDATE_CHECK", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn azork binary");

    child
        .stdin
        .as_mut()
        .expect("stdin should be piped")
        .write_all(b"quit\n")
        .expect("failed to write to stdin");

    let out = child
        .wait_with_output()
        .expect("failed waiting on azork process");

    assert!(
        out.status.success(),
        "no-args launch must still exit zero after quitting, got status {:?}",
        out.status
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("AzZork"),
        "expected the game banner to print for no-args launch, got: {stdout:?}"
    );
}
