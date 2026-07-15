//! tests/docs_readme_tests.rs
//!
//! Contract tests for the documentation-cleanup requirements:
//!
//! 1. No internal codenames anywhere outside `vendor/` (third-party vendored
//!    code we don't own). The forbidden names are assembled at runtime below
//!    (never spelled out as a literal, contiguous string in this file) so this
//!    test file cannot itself trip the check it enforces.
//! 2. `README.md` no longer carries a top-level `## Architecture` section.
//! 3. `README.md` no longer presents the outside-in-testing (OIT) agent as a
//!    user-facing feature section.
//! 4. `README.md` embeds the three real `azork crawl` screenshots in its
//!    Dungeon Crawler Mode section, and the referenced image files exist in
//!    `docs/images/`.

use std::fs;
use std::path::{Path, PathBuf};

/// Returns the repository root (the directory containing `Cargo.toml`).
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Recursively collects every file under `dir`, skipping build/VCS/vendor
/// directories that are out of scope for the codename check.
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
/// (case-insensitive) anywhere outside `vendor/`.
#[test]
fn no_internal_codenames_outside_vendor() {
    // Built from non-matching fragments at runtime so this test file itself
    // never contains either name as a literal substring.
    let codenames = [["sim", "ard"].concat(), ["powder", "finger"].concat()];

    let root = repo_root();
    let mut files = Vec::new();
    collect_files(&root, &mut files);

    let mut offenders = Vec::new();
    for file in files {
        let Ok(contents) = fs::read_to_string(&file) else {
            continue;
        };
        let lower = contents.to_lowercase();
        for name in &codenames {
            if lower.contains(name) {
                offenders.push(format!("{}: contains {name:?}", file.display()));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "found forbidden internal codename references outside vendor/:\n{}",
        offenders.join("\n")
    );
}

/// Requirement 2: no top-level `## Architecture` heading (nor a dangling
/// link/anchor to one) in README.md.
#[test]
fn readme_has_no_architecture_section() {
    let readme = fs::read_to_string(repo_root().join("README.md")).expect("README.md missing");
    assert!(
        !readme.contains("## Architecture"),
        "README.md still contains a top-level Architecture section"
    );
    assert!(
        !readme.to_lowercase().contains("(#architecture)"),
        "README.md still links to an Architecture anchor"
    );
}

/// Requirement 3: OIT is not presented as a user-facing feature section.
#[test]
fn readme_does_not_feature_oit_as_user_facing_section() {
    let readme = fs::read_to_string(repo_root().join("README.md")).expect("README.md missing");
    assert!(
        !readme.contains("Outside-in-testing (OIT) agent"),
        "README.md still presents OIT as a headline feature section"
    );

    // The underlying code, binary, and friction report must remain intact.
    let root = repo_root();
    assert!(
        root.join("src/oit/mod.rs").is_file(),
        "src/oit/mod.rs must remain"
    );
    assert!(
        root.join("src/bin/azork-oit.rs").is_file(),
        "src/bin/azork-oit.rs must remain"
    );
    assert!(
        root.join("docs/oit-friction-report.md").is_file(),
        "docs/oit-friction-report.md must remain"
    );
}

/// Requirement 4: the three real crawl screenshots are embedded in the
/// Dungeon Crawler Mode section and exist on disk.
#[test]
fn readme_embeds_real_crawl_screenshots() {
    let readme = fs::read_to_string(repo_root().join("README.md")).expect("README.md missing");

    let images = [
        "docs/images/crawl-map-overview.png",
        "docs/images/crawl-map-zoom.png",
        "docs/images/crawl-resource-popup.png",
    ];

    for image in images {
        assert!(
            readme.contains(image),
            "README.md does not reference {image}"
        );
        assert!(
            repo_root().join(image).is_file(),
            "{image} does not exist on disk"
        );
    }

    let dungeon_idx = readme
        .find("## Dungeon Crawler Mode")
        .expect("README.md must have a Dungeon Crawler Mode section");
    let next_section_idx = readme[dungeon_idx + 1..]
        .find("\n## ")
        .map(|i| dungeon_idx + 1 + i)
        .unwrap_or(readme.len());
    let section = &readme[dungeon_idx..next_section_idx];
    for image in images {
        assert!(
            section.contains(image),
            "{image} is not embedded within the Dungeon Crawler Mode section"
        );
    }
}
