//! TDD contract tests for the PURE logic of `azork::update`.
//!
//! These tests define the offline, network-free contract that the update
//! module must satisfy. They exercise version normalisation, semantic-version
//! comparison, per-target asset selection, the 24h cooldown decision, the
//! `UpdateError` → exit-code mapping, and GitHub Releases JSON deserialisation.
//!
//! None of these tests touch the network. They MUST remain offline.

use azork::update::{
    self, is_newer, normalize_tag, select_asset, should_check, supported_release_target,
    GithubAsset, GithubRelease, UpdateError, UPDATE_CHECK_COOLDOWN_SECS,
};
use semver::Version;

// ---------------------------------------------------------------------------
// Constants / crate wiring
// ---------------------------------------------------------------------------

#[test]
fn github_repo_points_at_azork() {
    assert_eq!(update::GITHUB_REPO, "rysweet/azork");
}

#[test]
fn opt_out_env_var_name_is_stable() {
    assert_eq!(update::NO_UPDATE_CHECK_ENV, "AZORK_NO_UPDATE_CHECK");
}

#[test]
fn current_version_matches_crate_version() {
    // update::CURRENT_VERSION must re-export the single source of truth.
    assert_eq!(update::CURRENT_VERSION, azork::VERSION);
    // And it must be a parseable semantic version.
    assert!(Version::parse(update::CURRENT_VERSION).is_ok());
}

#[test]
fn cooldown_is_twenty_four_hours() {
    assert_eq!(UPDATE_CHECK_COOLDOWN_SECS, 24 * 60 * 60);
}

// ---------------------------------------------------------------------------
// normalize_tag
// ---------------------------------------------------------------------------

#[test]
fn normalize_tag_strips_leading_v() {
    let v = normalize_tag("v1.2.3").expect("valid tag");
    assert_eq!(v, Version::new(1, 2, 3));
}

#[test]
fn normalize_tag_accepts_bare_semver() {
    let v = normalize_tag("0.3.0").expect("valid tag");
    assert_eq!(v, Version::new(0, 3, 0));
}

#[test]
fn normalize_tag_trims_whitespace() {
    let v = normalize_tag("  v2.0.1\n").expect("valid tag");
    assert_eq!(v, Version::new(2, 0, 1));
}

#[test]
fn normalize_tag_rejects_garbage() {
    assert!(normalize_tag("not-a-version").is_err());
    assert!(normalize_tag("").is_err());
    assert!(normalize_tag("v").is_err());
}

// ---------------------------------------------------------------------------
// is_newer  (strict '>' only — anti-rollback)
// ---------------------------------------------------------------------------

fn v(s: &str) -> Version {
    Version::parse(s).unwrap()
}

#[test]
fn is_newer_true_for_greater() {
    assert!(is_newer(&v("0.2.0"), &v("0.3.0")));
    assert!(is_newer(&v("0.2.0"), &v("1.0.0")));
    assert!(is_newer(&v("0.2.0"), &v("0.2.1")));
}

#[test]
fn is_newer_false_for_equal() {
    assert!(!is_newer(&v("0.2.0"), &v("0.2.0")));
}

#[test]
fn is_newer_false_for_older() {
    assert!(!is_newer(&v("0.3.0"), &v("0.2.0")));
    assert!(!is_newer(&v("1.0.0"), &v("0.9.9")));
}

// ---------------------------------------------------------------------------
// supported_release_target / expected asset name
// ---------------------------------------------------------------------------

#[test]
fn supported_target_is_present_on_linux_x86_64() {
    // The CI/build host for this project is linux x86_64.
    if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        assert_eq!(
            supported_release_target(),
            Some("x86_64-unknown-linux-gnu")
        );
    }
}

#[test]
fn asset_name_encodes_target_triple_and_extension() {
    let name = update::asset_name_for_target("x86_64-unknown-linux-gnu");
    assert_eq!(name, "azork-x86_64-unknown-linux-gnu.tar.gz");
}

#[test]
fn checksum_asset_name_is_archive_plus_sha256() {
    let name = update::checksum_asset_name("azork-x86_64-unknown-linux-gnu.tar.gz");
    assert_eq!(name, "azork-x86_64-unknown-linux-gnu.tar.gz.sha256");
}

// ---------------------------------------------------------------------------
// select_asset
// ---------------------------------------------------------------------------

fn asset(name: &str) -> GithubAsset {
    GithubAsset {
        name: name.to_string(),
        browser_download_url: format!("https://example.invalid/{name}"),
    }
}

fn release_with(assets: Vec<GithubAsset>) -> GithubRelease {
    GithubRelease {
        tag_name: "v0.3.0".to_string(),
        draft: false,
        prerelease: false,
        assets,
    }
}

#[test]
fn select_asset_finds_matching_target() {
    let rel = release_with(vec![
        asset("azork-x86_64-apple-darwin.tar.gz"),
        asset("azork-x86_64-unknown-linux-gnu.tar.gz"),
        asset("azork-x86_64-unknown-linux-gnu.tar.gz.sha256"),
    ]);
    let picked = select_asset(&rel, "x86_64-unknown-linux-gnu").expect("asset present");
    assert_eq!(picked.name, "azork-x86_64-unknown-linux-gnu.tar.gz");
}

#[test]
fn select_asset_does_not_return_checksum_file() {
    // The .sha256 sibling must not be mistaken for the archive.
    let rel = release_with(vec![asset("azork-x86_64-unknown-linux-gnu.tar.gz.sha256")]);
    assert!(select_asset(&rel, "x86_64-unknown-linux-gnu").is_none());
}

#[test]
fn select_asset_returns_none_when_no_match() {
    let rel = release_with(vec![asset("azork-x86_64-apple-darwin.tar.gz")]);
    assert!(select_asset(&rel, "x86_64-unknown-linux-gnu").is_none());
}

// ---------------------------------------------------------------------------
// should_check  (24h cooldown)
// ---------------------------------------------------------------------------

#[test]
fn should_check_when_no_previous_check() {
    // No cache timestamp => check (fail-open toward checking).
    assert!(should_check(1_000_000, None));
}

#[test]
fn should_check_false_within_cooldown() {
    let now = 1_000_000u64;
    let last = now - (UPDATE_CHECK_COOLDOWN_SECS - 10);
    assert!(!should_check(now, Some(last)));
}

#[test]
fn should_check_true_after_cooldown() {
    let now = 1_000_000u64;
    let last = now - UPDATE_CHECK_COOLDOWN_SECS;
    assert!(should_check(now, Some(last)));
    let older = now - (UPDATE_CHECK_COOLDOWN_SECS + 5);
    assert!(should_check(now, Some(older)));
}

#[test]
fn should_check_handles_clock_skew_gracefully() {
    // A last-check timestamp in the future must not panic or under/overflow;
    // treat as "recently checked" (do not check).
    let now = 1_000u64;
    let future = now + 5_000;
    assert!(!should_check(now, Some(future)));
}

// ---------------------------------------------------------------------------
// UpdateError -> exit code mapping (authoritative contract)
// ---------------------------------------------------------------------------

#[test]
fn exit_code_network_is_2() {
    assert_eq!(UpdateError::Network("boom".into()).exit_code(), 2);
}

#[test]
fn exit_code_checksum_mismatch_is_3() {
    let e = UpdateError::ChecksumMismatch {
        expected: "aa".into(),
        actual: "bb".into(),
    };
    assert_eq!(e.exit_code(), 3);
}

#[test]
fn exit_code_target_not_writable_is_4() {
    assert_eq!(
        UpdateError::TargetNotWritable("/usr/local/bin/azork".into()).exit_code(),
        4
    );
}

#[test]
fn exit_code_no_supported_asset_is_5() {
    assert_eq!(UpdateError::NoSupportedAsset.exit_code(), 5);
}

// ---------------------------------------------------------------------------
// GitHub Releases JSON deserialisation
// ---------------------------------------------------------------------------

#[test]
fn github_release_deserialises_from_api_shape() {
    let json = r#"{
        "tag_name": "v0.3.0",
        "draft": false,
        "prerelease": false,
        "assets": [
            {
                "name": "azork-x86_64-unknown-linux-gnu.tar.gz",
                "browser_download_url": "https://example.invalid/a.tar.gz"
            },
            {
                "name": "azork-x86_64-unknown-linux-gnu.tar.gz.sha256",
                "browser_download_url": "https://example.invalid/a.tar.gz.sha256"
            }
        ]
    }"#;

    let rel: GithubRelease = serde_json::from_str(json).expect("valid release json");
    assert_eq!(rel.tag_name, "v0.3.0");
    assert!(!rel.draft);
    assert!(!rel.prerelease);
    assert_eq!(rel.assets.len(), 2);
    assert_eq!(rel.assets[0].name, "azork-x86_64-unknown-linux-gnu.tar.gz");
}

#[test]
fn github_release_ignores_unknown_fields() {
    // The GitHub API returns many extra fields; deserialisation must tolerate them.
    let json = r#"{
        "url": "https://api.github.com/…",
        "id": 12345,
        "tag_name": "v1.0.0",
        "name": "Release 1.0.0",
        "draft": false,
        "prerelease": false,
        "created_at": "2024-01-01T00:00:00Z",
        "assets": []
    }"#;
    let rel: GithubRelease = serde_json::from_str(json).expect("tolerant of extra fields");
    assert_eq!(rel.tag_name, "v1.0.0");
    assert!(rel.assets.is_empty());
}
