//! tests/docs_readme_tests.rs
//!
//! Contract tests for the documentation-cleanup requirements tracked by this
//! change:
//!
//! 1. No internal Microsoft project codenames anywhere in the tracked tree.
//!    The forbidden codenames are assembled at runtime below (never spelled
//!    out as a literal, contiguous string in this source file) so that this
//!    test file itself does not trip the very check it enforces.
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

/// Recursively collects every file under `dir`, skipping `.git/`, `target/`,
/// `worktrees/`, and `.claude/` (VCS internals, build artifacts, worktree
/// checkouts, and tool config are out of scope for the codename check).
fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == ".git" || name == "target" || name == "worktrees" || name == ".claude" {
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
/// (case insensitive) anywhere in the tracked tree.
///
/// The codenames are assembled from non-matching substrings at runtime
/// (rather than written as contiguous literals) so this test file itself
/// stays clean under a literal `git grep` for them.
#[test]
fn no_internal_codenames_in_tree() {
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
        "found internal codename references: {:?}",
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

/// Requirement: the "## Example session" section (a concrete demo of playing
/// AzZork) must appear near the top of the README, immediately after the
/// intro and before the "## The metaphor" section, so new readers see a
/// real example before any conceptual material.
#[test]
fn readme_example_session_appears_before_the_metaphor() {
    let readme = read_readme();
    let lines: Vec<&str> = readme.lines().collect();

    let example_idx = lines
        .iter()
        .position(|l| l.trim() == "## Example session")
        .expect("README.md must contain a top-level '## Example session' section");

    let metaphor_idx = lines
        .iter()
        .position(|l| l.trim() == "## The metaphor")
        .expect("README.md must contain a top-level '## The metaphor' section");

    assert!(
        example_idx < metaphor_idx,
        "'## Example session' (line {}) must appear before '## The metaphor' (line {}) \
         so new readers see a concrete example right after the intro",
        example_idx + 1,
        metaphor_idx + 1
    );

    // It should be one of the first top-level sections encountered (i.e. sit
    // right after the intro), not merely "somewhere before" a later section.
    let first_h2_idx = lines
        .iter()
        .position(|l| l.trim_start().starts_with("## "))
        .expect("README.md must contain at least one top-level section");
    assert_eq!(
        example_idx, first_h2_idx,
        "'## Example session' must be the first top-level section after the intro"
    );
}

/// The "### Getting eaten by a Grue" subsection must remain nested directly
/// beneath "## Example session" after the section move (verbatim content,
/// no orphaning of the subsection at its old location).
#[test]
fn readme_grue_subsection_is_nested_under_example_session() {
    let readme = read_readme();
    let lines: Vec<&str> = readme.lines().collect();

    let example_idx = lines
        .iter()
        .position(|l| l.trim() == "## Example session")
        .expect("README.md must contain '## Example session'");

    let grue_idx = lines
        .iter()
        .position(|l| l.trim() == "### Getting eaten by a Grue")
        .expect("README.md must contain '### Getting eaten by a Grue'");

    let next_h2_idx = lines[example_idx + 1..]
        .iter()
        .position(|l| l.trim_start().starts_with("## ") && !l.trim_start().starts_with("### "))
        .map(|i| i + example_idx + 1)
        .unwrap_or(lines.len());

    assert!(
        grue_idx > example_idx && grue_idx < next_h2_idx,
        "'### Getting eaten by a Grue' must be nested within '## Example session'"
    );
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

/// Regression test for issue #80: as long as the repo has zero published
/// GitHub Releases, the README's Install section must not present the
/// curl-based one-liner as unconditionally working — it must carry a clear
/// warning above the one-liner, and "Build from source" must be reachable
/// as the currently-working alternative. This is a best-effort content
/// check (not a live check against the GitHub Releases API); if a release
/// is eventually published, this note should be revisited/removed.
#[test]
fn readme_install_section_warns_about_missing_release() {
    let readme = read_readme();
    let lower = readme.to_lowercase();

    let install_idx = readme
        .find("## Install")
        .expect("README.md must contain an '## Install' section");
    let curl_idx = readme[install_idx..]
        .find("curl -fsSL https://raw.githubusercontent.com/rysweet/azork/main/install.sh")
        .map(|i| i + install_idx)
        .expect("Install section must contain the curl one-liner");

    let warning_snippet = "no github release has been published yet";
    let warning_idx = lower
        .find(warning_snippet)
        .expect("README.md must warn that no GitHub Release has been published yet");

    assert!(
        warning_idx < curl_idx,
        "the 'no GitHub Release published yet' warning must appear before the curl one-liner \
         in the Install section, not after it"
    );

    assert!(
        lower.contains("build from source"),
        "README.md Install section must reference 'build from source' as the working alternative"
    );
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
