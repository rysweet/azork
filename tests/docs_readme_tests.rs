//! tests/docs_readme_tests.rs
//!
//! Contract tests for the documentation-cleanup requirements tracked by this
//! change:
//!
//! 1. No internal Microsoft project codenames anywhere outside `vendor/`
//!    (which is third-party vendored code we don't own). The forbidden
//!    codenames are assembled at runtime below (never spelled out as a
//!    literal, contiguous string in this source file) so that this test
//!    file itself does not trip the very check it enforces.
//! 2. `README.md` no longer carries a top-level `## Architecture` section
//!    (nor a dangling ToC/anchor link to one).
//! 3. `README.md` no longer presents the outside-in-testing (OIT) agent as a
//!    user-facing feature section.
//! 4. `README.md` embeds the three real `azork crawl` screenshots in its
//!    Dungeon Crawler Mode section, and the referenced image files actually
//!    exist in `docs/images/`.
//!
//! These tests read repository files directly (via `CARGO_MANIFEST_DIR`) and
//! do not depend on any runtime behavior of the `azork` crate, so they apply
//! equally to doc-comments and markdown.

use std::fs;
use std::path::{Path, PathBuf};

/// Returns the repository root (the directory containing `Cargo.toml`).
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Recursively collects every file under `dir`, skipping `vendor/`, `.git/`,
/// and `target/` (build artifacts / vendored third-party code / VCS
/// internals are out of scope for the codename check).
fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == "vendor"
            || name == ".git"
            || name == "target"
            || name == "worktrees"
            || name == ".claude"
        {
            continue;
        }
        if path.is_dir() {
            collect_files(&path, out);
        } else {
            out.push(path);
        }
    }
}

/// Requirement 1: zero occurrences of either forbidden internal codename
/// (case insensitive) anywhere outside `vendor/`.
///
/// The codenames are assembled from non-matching substrings at runtime
/// (rather than written as contiguous literals) so this test file itself
/// stays clean under a literal `git grep` for them.
#[test]
fn no_internal_codenames_outside_vendor() {
    let root = repo_root();
    let mut files = Vec::new();
    collect_files(&root, &mut files);

    let codename_a = format!("{}{}", "sim", "ard");
    let codename_b = format!("{}{}", "powder", "finger");
    let forbidden = [codename_a, codename_b];

    let mut offenders = Vec::new();
    for path in &files {
        // Only inspect text-ish files; skip binary assets like screenshots.
        let Ok(bytes) = fs::read(path) else { continue };
        let Ok(text) = String::from_utf8(bytes) else {
            continue;
        };
        let lower = text.to_lowercase();
        if forbidden.iter().any(|word| lower.contains(word.as_str())) {
            offenders.push(path.display().to_string());
        }
    }

    assert!(
        offenders.is_empty(),
        "found internal codename references outside vendor/: {:?}",
        offenders
    );
}

fn read_readme() -> String {
    fs::read_to_string(repo_root().join("README.md")).expect("README.md must exist and be UTF-8")
}

/// Requirement 2: no top-level `## Architecture` heading, and no dangling
/// link/anchor pointing at one, anywhere in the README.
#[test]
fn readme_has_no_architecture_section() {
    let readme = read_readme();
    for line in readme.lines() {
        let trimmed = line.trim();
        assert!(
            !trimmed.eq_ignore_ascii_case("## Architecture"),
            "README.md must not contain a top-level Architecture section"
        );
    }
    assert!(
        !readme.to_lowercase().contains("(#architecture)"),
        "README.md must not contain a dangling ToC/anchor link to an Architecture section"
    );
}

/// Requirement 3: OIT must not be presented as a user-facing feature section
/// in the README (it's an internal self-testing mechanism). The heading
/// pattern that previously existed was "## Outside-in-testing (OIT) agent".
#[test]
fn readme_does_not_feature_oit_as_user_facing_section() {
    let readme = read_readme();
    for line in readme.lines() {
        let trimmed = line.trim().to_lowercase();
        assert!(
            !(trimmed.starts_with("## ") && trimmed.contains("oit") && trimmed.contains("agent")),
            "README.md must not present OIT as a user-facing feature section: {:?}",
            line
        );
    }
}

/// Requirement 3 (continued): the underlying OIT code and artifacts must
/// still exist — we're only removing the *user-facing marketing*, not the
/// tool itself.
#[test]
fn oit_internals_are_preserved() {
    let root = repo_root();
    assert!(root.join("src/oit").is_dir(), "src/oit/ must still exist");
    assert!(
        root.join("src/bin/azork-oit.rs").is_file(),
        "the azork-oit binary source must still exist"
    );
    assert!(
        root.join("docs/oit-friction-report.md").is_file(),
        "docs/oit-friction-report.md must still exist"
    );
}

/// Requirement 4: the three real crawl screenshots exist on disk under
/// `docs/images/` ...
#[test]
fn crawl_screenshots_exist_on_disk() {
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

/// Requirement 4 (continued): ... and are actually embedded, in order, in
/// the README's Dungeon Crawler Mode section.
#[test]
fn readme_embeds_crawl_screenshots_in_order() {
    let readme = read_readme();

    let section_start = readme
        .find("## Dungeon Crawler Mode")
        .expect("README.md must contain a Dungeon Crawler Mode section");
    // Section body runs until the next top-level heading.
    let rest = &readme[section_start + 2..];
    let section_end = rest
        .find("\n## ")
        .map(|i| section_start + 2 + i)
        .unwrap_or(readme.len());
    let section = &readme[section_start..section_end];

    let overview_pos = section
        .find("docs/images/crawl-map-overview.png")
        .expect("Dungeon Crawler Mode section must embed crawl-map-overview.png");
    let zoom_pos = section
        .find("docs/images/crawl-map-zoom.png")
        .expect("Dungeon Crawler Mode section must embed crawl-map-zoom.png");
    let popup_pos = section
        .find("docs/images/crawl-resource-popup.png")
        .expect("Dungeon Crawler Mode section must embed crawl-resource-popup.png");

    assert!(
        overview_pos < zoom_pos && zoom_pos < popup_pos,
        "screenshots must appear in order: overview, zoom, resource pop-up"
    );

    // Standard Markdown image syntax with descriptive alt text (non-empty).
    assert!(
        section.contains("![") && section.contains("](docs/images/crawl-map-overview.png)"),
        "overview screenshot must use Markdown image syntax with alt text"
    );
}
