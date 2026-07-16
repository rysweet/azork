//! tests/crawl_screenshot_replacement_tests.rs
//!
//! TDD (red phase) contract tests for replacing the three illegible
//! real-Azure Dungeon Crawler screenshots with clearer, reproducible
//! renders from the built-in deterministic offline mock backend.
//!
//! These tests define the expected end state and are expected to FAIL
//! against the pre-change tree (8000px real-subscription capture, old
//! caption text) and PASS once the three PNGs under `docs/images/` are
//! replaced in place and `README.md` is updated as specified.
//!
//! No new crate dependencies are introduced: PNG width/height are read
//! directly from the file's IHDR chunk (first 8-byte signature + 4-byte
//! length + 4-byte "IHDR" + 4-byte width + 4-byte height, all big-endian),
//! which is part of every valid PNG per the PNG spec and needs no decoder.

use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read_readme() -> String {
    fs::read_to_string(repo_root().join("README.md")).expect("README.md must exist and be UTF-8")
}

/// Reads the (width, height) of a PNG file directly from its IHDR chunk.
fn png_dimensions(path: &Path) -> (u32, u32) {
    let bytes = fs::read(path).unwrap_or_else(|e| panic!("failed to read {path:?}: {e}"));
    assert!(bytes.len() >= 24, "{path:?} is too small to be a valid PNG");
    assert_eq!(
        &bytes[0..8],
        &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A],
        "{path:?} does not start with the PNG signature"
    );
    assert_eq!(
        &bytes[12..16],
        b"IHDR",
        "{path:?} must have IHDR as its first chunk"
    );
    let width = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let height = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    (width, height)
}

/// The three screenshot files must exist (in-place replacement, no renames).
#[test]
fn crawl_screenshot_files_exist() {
    let root = repo_root();
    for name in [
        "crawl-map-overview.png",
        "crawl-map-zoom.png",
        "crawl-resource-popup.png",
    ] {
        let path = root.join("docs/images").join(name);
        assert!(
            path.is_file(),
            "expected screenshot to exist at docs/images/{name}"
        );
    }
}

/// The overview screenshot must be the new, legible mock-backend render
/// (3532x3534), not the old illegible 8000px real-subscription capture.
#[test]
fn overview_screenshot_has_expected_mock_backend_dimensions() {
    let path = repo_root().join("docs/images/crawl-map-overview.png");
    let (w, h) = png_dimensions(&path);
    assert_eq!(
        (w, h),
        (3532, 3534),
        "docs/images/crawl-map-overview.png must be the new 3532x3534 mock-backend render, got {w}x{h}"
    );
}

/// The zoom screenshot must be the new mock-backend render (3000x2000).
#[test]
fn zoom_screenshot_has_expected_mock_backend_dimensions() {
    let path = repo_root().join("docs/images/crawl-map-zoom.png");
    let (w, h) = png_dimensions(&path);
    assert_eq!(
        (w, h),
        (3000, 2000),
        "docs/images/crawl-map-zoom.png must be the new 3000x2000 mock-backend render, got {w}x{h}"
    );
}

/// The resource pop-up screenshot must be the new mock-backend render
/// (1168x308).
#[test]
fn popup_screenshot_has_expected_mock_backend_dimensions() {
    let path = repo_root().join("docs/images/crawl-resource-popup.png");
    let (w, h) = png_dimensions(&path);
    assert_eq!(
        (w, h),
        (1168, 308),
        "docs/images/crawl-resource-popup.png must be the new 1168x308 mock-backend render, got {w}x{h}"
    );
}

/// None of the three screenshots may retain the old real-subscription
/// dimensions (8000x7630, 588x469, 968x139), regardless of what new
/// dimensions are chosen — this pins down that a real replacement (not a
/// no-op) happened.
#[test]
fn screenshots_no_longer_have_old_real_subscription_dimensions() {
    let root = repo_root();
    let old_dims = [
        ("crawl-map-overview.png", (8000, 7630)),
        ("crawl-map-zoom.png", (588, 469)),
        ("crawl-resource-popup.png", (968, 139)),
    ];
    for (name, old) in old_dims {
        let path = root.join("docs/images").join(name);
        let dims = png_dimensions(&path);
        assert_ne!(
            dims, old,
            "docs/images/{name} still has the old real-subscription dimensions {old:?}; expected it to be replaced"
        );
    }
}

/// The old caption text describing a real live-subscription capture (257
/// rooms, 2,854 resources) must be gone.
#[test]
fn readme_no_longer_describes_real_subscription_capture() {
    let readme = read_readme();
    assert!(
        !readme.contains("against a live subscription (257"),
        "README.md must not describe the screenshots as a live-subscription capture anymore"
    );
    assert!(
        !readme.contains("2,854 resources"),
        "README.md must not reference the old real-subscription resource count anymore"
    );
}

/// The new caption must describe the deterministic offline mock backend,
/// including the exact reproduction command and synthetic tenant size.
#[test]
fn readme_describes_deterministic_mock_backend_capture() {
    // Normalize line-wrapping so substring checks are robust to README reflow.
    let readme = read_readme().replace('\n', " ");
    assert!(
        readme.contains("deterministic offline mock backend"),
        "README.md must describe the screenshots as coming from the deterministic offline mock backend"
    );
    assert!(
        readme.contains("40 resource groups and 520 resources"),
        "README.md must mention the synthetic tenant of 40 resource groups and 520 resources"
    );
    assert!(
        readme.contains("azork crawl --backend mock --mock-size 40x13 --serve"),
        "README.md must give the exact reproduction command `azork crawl --backend mock --mock-size 40x13 --serve`"
    );
}

/// The overview screenshot's alt text must reflect the synthetic tenant,
/// not a live Azure subscription.
#[test]
fn overview_alt_text_describes_synthetic_tenant() {
    let readme = read_readme();
    assert!(
        readme.contains(
            "![Dungeon map of a synthetic 520-resource Azure tenant](docs/images/crawl-map-overview.png)"
        ),
        "README.md overview image alt text must read \
         'Dungeon map of a synthetic 520-resource Azure tenant'"
    );
    assert!(
        !readme.contains(
            "![Dungeon map of an Azure subscription](docs/images/crawl-map-overview.png)"
        ),
        "README.md must not retain the old 'Dungeon map of an Azure subscription' alt text"
    );
}

/// The real-Azure capture script referenced elsewhere in the repo must be
/// left untouched by this change (it remains available for anyone who
/// wants to regenerate real-subscription captures separately).
#[test]
fn real_screenshot_capture_script_is_preserved() {
    let path = repo_root().join("scripts/capture-real-screenshots.mjs");
    assert!(
        path.is_file(),
        "scripts/capture-real-screenshots.mjs must still exist and be untouched"
    );
}
