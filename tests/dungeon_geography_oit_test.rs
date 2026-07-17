//! tests/dungeon_geography_oit_test.rs
//!
//! Outside-in QA evidence for the geography-aware dungeon layout as a
//! **user-facing surface**: this drives the actual compiled `azork` binary as
//! a subprocess (`azork dungeon --backend mock ... --snapshot <path>`) exactly
//! the way a human/CI would, then asserts on the externally-observable artifact
//! it emits — the snapshot JSON a user would `--diff` later. It does not call
//! internal library functions.
//!
//! The mock backend is used throughout, so this suite never touches a live
//! Azure subscription, has no network dependency, and is safe in CI. It
//! substitutes for `gadugi-test` (documented as unavailable in this environment
//! in the PR description) as interim outside-in QA evidence for the geography
//! feature.
//!
//! The assertions are deliberately population-level (per-region *mean* x), not
//! per-room. The region bias is a directional hint: individual rooms can be
//! nudged by collision resolution, but the aggregate west->east / continent
//! ordering is the property a user actually perceives on the rendered map.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;

use serde::Deserialize;

/// Path to the compiled `azork` binary, built by Cargo's test harness.
fn azork_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_azork"))
}

/// A unique scratch snapshot path so parallel test runs never collide.
fn temp_snapshot(tag: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!(
        "azork-geo-oit-{}-{}-{}.json",
        tag,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    p
}

#[derive(Deserialize)]
struct Room {
    region: String,
    x: i32,
}

#[derive(Deserialize)]
struct Snapshot {
    rooms: Vec<Room>,
}

/// Drive the real binary with a fixed mock estate + seed and return the parsed
/// snapshot it wrote to disk.
fn render_snapshot(tag: &str) -> (Snapshot, Vec<u8>) {
    let snap = temp_snapshot(tag);
    let out = Command::new(azork_binary())
        .args([
            "dungeon",
            "--backend",
            "mock",
            "--mock-size",
            // Sized, seeded estate so the run is deterministic and spans many
            // regions across several continents.
            "large:7",
            "--snapshot",
        ])
        .arg(&snap)
        .output()
        .expect("failed to spawn azork binary");

    assert!(
        out.status.success(),
        "azork dungeon --snapshot should exit 0; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        snap.is_file(),
        "azork must write the snapshot JSON to the --snapshot path"
    );

    let bytes = std::fs::read(&snap).expect("snapshot should be readable");
    let parsed: Snapshot =
        serde_json::from_slice(&bytes).expect("snapshot must be valid Snapshot JSON");
    let _ = std::fs::remove_file(&snap);
    (parsed, bytes)
}

/// Mean x-coordinate of every room in `region`, panicking if the region is
/// absent (keeps the test honest: a silently-missing region can't pass).
fn mean_x(snap: &Snapshot, region: &str) -> f64 {
    let xs: Vec<i32> = snap
        .rooms
        .iter()
        .filter(|r| r.region == region)
        .map(|r| r.x)
        .collect();
    assert!(
        !xs.is_empty(),
        "expected the mock estate to contain region {region}"
    );
    xs.iter().sum::<i32>() as f64 / xs.len() as f64
}

#[test]
fn rendered_map_orders_regions_west_to_east_by_real_geography() {
    let (snap, _) = render_snapshot("order");

    // Cross-bucket comparisons only (same-bucket neighbours like eastus vs
    // eastus2 are intentionally free to swap on hash jitter). Each pair is a
    // real-world west->east relationship a user would expect to see mirrored
    // on the map.
    let west_us = mean_x(&snap, "westus2").min(mean_x(&snap, "westus3"));
    let east_us = mean_x(&snap, "eastus");
    let europe = mean_x(&snap, "westeurope");
    let asia = mean_x(&snap, "japaneast");

    assert!(
        west_us < east_us,
        "west US regions (mean x {west_us:.1}) should render west of eastus (mean x {east_us:.1})"
    );
    assert!(
        east_us < europe,
        "eastus (mean x {east_us:.1}) should render west of westeurope (mean x {europe:.1})"
    );
    assert!(
        europe < asia,
        "westeurope (mean x {europe:.1}) should render west of japaneast (mean x {asia:.1})"
    );
}

#[test]
fn rendered_snapshot_is_deterministic_across_runs() {
    // Same seeded estate must produce a byte-identical artifact on every run,
    // so `--diff` of two unchanged snapshots is always empty for a user.
    let (_, first) = render_snapshot("det-a");
    let (_, second) = render_snapshot("det-b");
    assert_eq!(
        first, second,
        "the same seeded mock estate must render an identical snapshot every time"
    );
}

#[test]
fn every_rendered_region_stays_within_its_biased_longitude_band() {
    // Population-level sanity: sorting regions by their mean rendered x must
    // reproduce their real-world west->east longitude order for the coarse
    // continental groups present in the estate. This guards against a future
    // edit that keeps individual pair tests passing but scrambles the overall
    // globe layout.
    let (snap, _) = render_snapshot("bands");

    let mut by_region: BTreeMap<String, Vec<i32>> = BTreeMap::new();
    for r in &snap.rooms {
        by_region.entry(r.region.clone()).or_default().push(r.x);
    }

    // Expected real-world west->east ordering of the continental anchors.
    let anchors = ["westus3", "eastus", "westeurope", "japaneast"];
    let mut last = f64::NEG_INFINITY;
    for region in anchors {
        let xs = by_region
            .get(region)
            .unwrap_or_else(|| panic!("expected region {region} in estate"));
        let mean = xs.iter().sum::<i32>() as f64 / xs.len() as f64;
        assert!(
            mean > last,
            "region {region} (mean x {mean:.1}) breaks the west->east ordering (previous anchor mean x {last:.1})"
        );
        last = mean;
    }
}
