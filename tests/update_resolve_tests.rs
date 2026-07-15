//! Offline integration tests for the update orchestration layer.
//!
//! These exercise `resolve_latest_update`, `run_update_with(check_only)`, and
//! the cooldown-cache read/write helpers WITHOUT any network access, using the
//! `AZORK_TEST_FAKE_RELEASE_JSON` injection hook and an `XDG_CONFIG_HOME`
//! override. All env mutation is confined to a single serial test so parallel
//! test threads never observe a torn environment.

use azork::update::{
    self, now_unix, read_last_check, resolve_latest_update, run_update_with, write_last_check,
};

fn fake_release_json(tag: &str) -> String {
    // Include an asset for whatever target this test host actually runs, so the
    // resolution path is exercised on both x86_64 and aarch64 CI/dev machines.
    let target = update::supported_release_target().unwrap_or("x86_64-unknown-linux-gnu");
    let archive = update::asset_name_for_target(target);
    let checksum = update::checksum_asset_name(&archive);
    format!(
        r#"{{
            "tag_name": "{tag}",
            "draft": false,
            "prerelease": false,
            "assets": [
                {{
                    "name": "{archive}",
                    "browser_download_url": "https://github.com/rysweet/azork/releases/download/{tag}/{archive}"
                }},
                {{
                    "name": "{checksum}",
                    "browser_download_url": "https://github.com/rysweet/azork/releases/download/{tag}/{checksum}"
                }}
            ]
        }}"#
    )
}

#[test]
fn update_orchestration_is_offline_and_correct() {
    // --- resolve: a much newer release is offered with its checksum URL -----
    std::env::set_var("AZORK_TEST_FAKE_RELEASE_JSON", fake_release_json("v99.0.0"));
    let resolved = resolve_latest_update()
        .expect("resolve should succeed offline")
        .expect("a newer release should be offered");
    assert_eq!(resolved.version.to_string(), "99.0.0");
    assert!(
        resolved.checksum_url.is_some(),
        "checksum URL must be resolved"
    );
    assert!(resolved.asset_url.contains("-unknown-linux-gnu.tar.gz"));

    // check-only mode reports availability and exits 0 without installing.
    assert_eq!(run_update_with(true), 0);

    // --- resolve: an older release is NOT offered (anti-rollback) ----------
    std::env::set_var("AZORK_TEST_FAKE_RELEASE_JSON", fake_release_json("v0.0.1"));
    assert!(
        resolve_latest_update().unwrap().is_none(),
        "an older release must never be offered"
    );
    assert_eq!(run_update_with(true), 0);

    // --- resolve: a draft release is ignored --------------------------------
    std::env::set_var(
        "AZORK_TEST_FAKE_RELEASE_JSON",
        r#"{"tag_name":"v99.0.0","draft":true,"prerelease":false,"assets":[]}"#,
    );
    assert!(resolve_latest_update().unwrap().is_none(), "draft ignored");

    std::env::remove_var("AZORK_TEST_FAKE_RELEASE_JSON");

    // --- cooldown cache round-trips through XDG_CONFIG_HOME -----------------
    let tmp = std::env::temp_dir().join(format!("azork-cache-it-{}", now_unix()));
    std::fs::create_dir_all(&tmp).unwrap();
    let prev_xdg = std::env::var_os("XDG_CONFIG_HOME");
    std::env::set_var("XDG_CONFIG_HOME", &tmp);

    let stamp = now_unix();
    write_last_check(stamp).expect("write cache");
    assert_eq!(read_last_check(), Some(stamp));
    // The cache path is XDG-anchored.
    assert!(update::cache_path().unwrap().starts_with(&tmp));

    match prev_xdg {
        Some(v) => std::env::set_var("XDG_CONFIG_HOME", v),
        None => std::env::remove_var("XDG_CONFIG_HOME"),
    }
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn should_check_cooldown_boundaries() {
    // No cache → check. Within cooldown → skip. After cooldown → check.
    assert!(update::should_check(1_000_000, None));
    assert!(!update::should_check(
        1_000_000,
        Some(1_000_000 - (update::UPDATE_CHECK_COOLDOWN_SECS - 1))
    ));
    assert!(update::should_check(
        1_000_000,
        Some(1_000_000 - update::UPDATE_CHECK_COOLDOWN_SECS)
    ));
}

#[test]
fn startup_check_opts_out_without_network() {
    use azork::update::check::{maybe_startup_check, StartupUpdateOutcome};
    // Explicit opt-out short-circuits before any network access and must yield
    // Continue. (Env is process-local; no other test in this binary reads it.)
    std::env::set_var(update::NO_UPDATE_CHECK_ENV, "1");
    let args = vec!["azork".to_string()];
    assert_eq!(maybe_startup_check(&args), StartupUpdateOutcome::Continue);
    std::env::remove_var(update::NO_UPDATE_CHECK_ENV);
}
