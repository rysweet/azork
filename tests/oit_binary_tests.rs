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

/// Regression test for bug #91 ("OIT --dry-run is not offline: it can invoke
/// the real az binary"): put a fake, sentinel-writing `az` executable first
/// on `PATH` and run `azork-oit --dry-run`. The dry-run catalog includes
/// `learn group` / `learn storage` (see `src/oit/usecases.rs`), which used to
/// reach the real `az` CLI via `derive_group_capabilities` ->
/// `ProcessAzRunner` (root cause a), and startup autodiscovery used to run by
/// default and do the same (root cause b). If either path fired, the
/// sentinel file would exist after the run. It must not.
#[test]
fn dry_run_never_invokes_the_real_az_binary() {
    let fake_bin_dir = std::env::temp_dir().join(format!(
        "azork-oit-fakebin-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&fake_bin_dir).expect("create fake bin dir");
    let sentinel = fake_bin_dir.join("az-was-invoked.marker");

    // A fake `az` that, if ever executed, records the fact and prints
    // something a real `az --help`/`az <group> --help` would never say, then
    // exits non-zero (as it should never even be reached).
    let fake_az_path = fake_bin_dir.join("az");
    std::fs::write(
        &fake_az_path,
        format!(
            "#!/bin/sh\ntouch '{}'\necho 'FAKE AZ WAS INVOKED' 1>&2\nexit 1\n",
            sentinel.display()
        ),
    )
    .expect("write fake az script");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&fake_az_path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&fake_az_path, perms).unwrap();
    }

    // Put the fake bin dir first on PATH so it shadows any real `az` the host
    // may have installed; the spawned `azork` (and its own children, if any)
    // inherit this PATH.
    let real_path = std::env::var("PATH").unwrap_or_default();
    let test_path = format!("{}:{}", fake_bin_dir.display(), real_path);

    let report = temp_report("no-real-az");
    let out = Command::new(oit_binary())
        .arg("--dry-run")
        .arg("--report")
        .arg(&report)
        .env("PATH", &test_path)
        .output()
        .expect("failed to spawn azork-oit binary");

    assert!(
        out.status.success(),
        "azork-oit --dry-run should still exit 0 with a shadowed az; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !sentinel.exists(),
        "the fake `az` binary was invoked during --dry-run: this means the \
         dry-run campaign shelled out to `az` instead of staying offline"
    );

    let _ = std::fs::remove_file(&report);
    let _ = std::fs::remove_dir_all(&fake_bin_dir);
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
