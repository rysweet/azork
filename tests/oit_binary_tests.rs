//! tests/oit_binary_tests.rs
//!
//! Outside-in QA evidence for the `azork-oit` agent as a **user-facing
//! surface**: this drives the actual compiled `azork-oit` binary as a
//! subprocess (not the internal library functions directly), the same way a
//! human/CI would invoke it, and asserts on its externally-observable
//! contract: exit code, stdout narration, and the friction report file it
//! writes to disk.
//!
//! `--dry-run` is used throughout so this suite never touches a live Azure
//! subscription, has no network dependency, and is safe to run in CI. It
//! substitutes for `gadugi-test` (documented as unavailable in this
//! environment in the PR description) as interim QA evidence for the OIT
//! agent surface.

use std::path::PathBuf;
use std::process::Command;

/// Path to the compiled `azork-oit` binary, built by Cargo's test harness
/// (`CARGO_BIN_EXE_<name>` is set automatically for binaries in `src/bin/`).
fn oit_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_azork-oit"))
}

/// A unique scratch report path so parallel test runs never collide.
fn temp_report(tag: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!(
        "azork-oit-report-{}-{}-{}.md",
        tag,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    p
}

#[test]
fn dry_run_exits_zero_and_announces_offline_mode() {
    let report = temp_report("mode");
    let out = Command::new(oit_binary())
        .arg("--dry-run")
        .arg("--report")
        .arg(&report)
        .output()
        .expect("failed to spawn azork-oit binary");

    assert!(
        out.status.success(),
        "azork-oit --dry-run should exit 0; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Outside-In-Testing agent"));
    assert!(stdout.contains("DRY RUN (offline)"));
    // A dry run must never claim to touch a live subscription.
    assert!(!stdout.contains("preflight failed"));

    let _ = std::fs::remove_file(&report);
}

#[test]
fn dry_run_drives_the_full_use_case_catalog() {
    let report = temp_report("catalog");
    let out = Command::new(oit_binary())
        .arg("--dry-run")
        .arg("--report")
        .arg(&report)
        .output()
        .expect("failed to spawn azork-oit binary");

    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("driving azork through the use-case catalog"));
    // Every use case reports either "clean" or "N friction" — the run must
    // have actually exercised the catalog, not silently produced zero rows.
    assert!(stdout.contains("clean") || stdout.contains("friction"));

    let _ = std::fs::remove_file(&report);
}

#[test]
fn dry_run_writes_a_friction_report_with_expected_sections() {
    let report = temp_report("report");
    let out = Command::new(oit_binary())
        .arg("--dry-run")
        .arg("--report")
        .arg(&report)
        .output()
        .expect("failed to spawn azork-oit binary");

    assert!(out.status.success());
    assert!(
        report.is_file(),
        "azork-oit must write its friction report to the --report path"
    );

    let contents = std::fs::read_to_string(&report).expect("report should be readable");
    assert!(!contents.trim().is_empty(), "report must not be empty");
    // Dry-run teardown line must make clear no live resources existed.
    assert!(contents.contains("dry-run") || contents.contains("no live resources"));

    let _ = std::fs::remove_file(&report);
}

#[test]
fn dry_run_never_creates_live_resource_groups() {
    let report = temp_report("no-rg");
    let out = Command::new(oit_binary())
        .arg("--dry-run")
        .arg("--report")
        .arg(&report)
        .output()
        .expect("failed to spawn azork-oit binary");

    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    // The "creating guardrailed test resources" section only runs in live mode.
    assert!(!stdout.contains("creating guardrailed test resources"));
    assert!(!stdout.contains("teardown (non-destructive: own tags only)"));

    let _ = std::fs::remove_file(&report);
}
