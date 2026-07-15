//! tests/cli_args_tests.rs
//!
//! Outside-in coverage for the top-level `azork` CLI argument parsing
//! (see `src/main.rs`). Drives the actual compiled `azork` binary as a
//! subprocess and asserts on its externally-observable contract: exit code
//! and stderr/stdout content.
//!
//! Regression coverage for https://github.com/rysweet/azork/issues/33:
//! unrecognized top-level input is AzZork's core design surface, not a usage
//! error. AzZork is agentic: anything that isn't a recognized subcommand
//! (`crawl`/`dungeon`/`update`) or a recognized launch flag (`--backend`/
//! `-b`, `--backend=<id>`, `--help`/`-h`, `--version`/`-V`/`version`) is
//! natural-language intent, routed through the same offline
//! `IntentResolver`/`MockAdapter`/`CapabilityRegistry` machinery the
//! interactive REPL uses to resolve an unrecognized typed line. PR #37 made
//! this a hard `exit(2)` rejection, which defeated that design; these tests
//! assert the corrected behavior.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// Path to the compiled `azork` binary, built by Cargo's test harness
/// (`CARGO_BIN_EXE_<name>` is set automatically for binaries defined via
/// `[[bin]]` in Cargo.toml).
fn azork_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_azork"))
}

/// Run the `azork` binary with `args`, isolated from any real learned
/// capability cache (`AZORK_CACHE_DIR` pointed at a fresh empty temp dir) so
/// intent resolution is fully offline and deterministic via the
/// `MockAdapter` — never real `az` or network calls.
fn run_isolated(args: &[&str]) -> std::process::Output {
    let cache_dir = tempdir();
    Command::new(azork_binary())
        .args(args)
        .env("AZORK_CACHE_DIR", cache_dir.path())
        .env("AZORK_NO_UPDATE_CHECK", "1")
        .output()
        .expect("failed to spawn azork binary")
}

// --- tiny temp-dir helper (avoids an extra dev-dependency) ---------------

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn tempdir() -> TempDir {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "azork-cli-args-test-{nanos}-{:p}",
        &nanos as *const _
    ));
    std::fs::create_dir_all(&path).expect("create temp dir");
    TempDir { path }
}

#[test]
fn unknown_subcommand_is_routed_through_intent_detection_not_rejected() {
    let out = run_isolated(&["totally-bogus-subcommand"]);

    assert!(
        out.status.success(),
        "unrecognized top-level input is intent, not a usage error — must exit zero, got status {:?}",
        out.status
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("unknown subcommand") && !stderr.contains("unknown flag"),
        "must never print a hard-reject 'unknown subcommand/flag' error, got stderr: {stderr:?}"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains("Type 'quit' to leave"),
        "the game banner must NOT print for a one-shot intent-resolution invocation, got stdout: {stdout:?}"
    );
    // With an empty (freshly isolated) capability registry, the MockAdapter
    // deterministically resolves unmatched input to `Resolution::Unresolved`,
    // whose narration steers the player toward `learn <group>` / `help`.
    assert!(
        stdout.contains("learn"),
        "expected an intent-resolution response mentioning 'learn', got stdout: {stdout:?}"
    );
}

#[test]
fn unrecognized_flag_is_routed_through_intent_detection_not_rejected() {
    let out = run_isolated(&["--this-flag-does-not-exist"]);

    assert!(
        out.status.success(),
        "unrecognized flag is intent, not a usage error — must exit zero, got status {:?}",
        out.status
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("unknown flag") && !stderr.contains("unknown subcommand"),
        "must never print a hard-reject 'unknown flag/subcommand' error, got stderr: {stderr:?}"
    );
}

#[test]
fn multi_word_intent_is_resolved_via_capability_registry_domain_hint() {
    // "list my vms" is never a recognized subcommand, but its nouns point at
    // the `vm` az domain — the same `infer_group` steering the REPL uses.
    let out = run_isolated(&["list", "my", "vms"]);

    assert!(
        out.status.success(),
        "multi-word natural-language intent must exit zero, got status {:?}",
        out.status
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.to_lowercase().contains("vm"),
        "expected the resolver to recognize the 'vm' domain in \"list my vms\", got stdout: {stdout:?}"
    );
    assert!(
        !stdout.contains("Type 'quit' to leave"),
        "the game banner must NOT print for a one-shot intent-resolution invocation, got stdout: {stdout:?}"
    );
}

#[test]
fn bare_help_word_is_resolved_as_intent_not_rejected() {
    // Only `--help`/`-h` are documented flags; the bare word `help` is not a
    // recognized top-level subcommand, so it now flows through intent
    // detection like any other unrecognized word — never a hard rejection.
    let out = run_isolated(&["help"]);

    assert!(
        out.status.success(),
        "bare 'help' must exit zero via intent detection, got status {:?}",
        out.status
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("unknown subcommand"),
        "must never print a hard-reject 'unknown subcommand' error, got stderr: {stderr:?}"
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
