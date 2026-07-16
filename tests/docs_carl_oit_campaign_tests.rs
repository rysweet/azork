//! tests/docs_carl_oit_campaign_tests.rs
//!
//! Contract tests for `docs/CARL-OIT-CAMPAIGN.md`'s "round-based re-runs"
//! support, added so the Carl campaign process can be re-run against a
//! moving `main` (round 2, round 3, ...) without ambiguity about:
//!
//! 1. There is a documented `## Round-based re-runs` section describing the
//!    round-N process (baseline, fresh re-test, per-PR re-verification,
//!    finding re-verification, no-duplicate-refiling, naming, and parent
//!    tracking issue).
//! 2. The campaign report title is parameterized by round number and the
//!    "Campaign report" section documents both the first-run and round-N
//!    forms.
//! 3. The section clarifies that the full, parameterized title is the
//!    canonical report-issue title, while shorthand forms are acceptable
//!    only in cross-references, not as the report issue's own title.
//! 4. The findings-ledger / duplicate-check text no longer hardcodes a
//!    single fixed umbrella tracking issue number as if it applied to every
//!    round; it explains that the parent issue varies per round.
//! 5. The fix-workstream dispatch section uses the generic
//!    `/home/azureuser/src/azork-fix-<slug>` clone convention (not a
//!    `<bugname>-fix` placeholder that collides with the campaign's own
//!    testing worktree naming).
//! 6. The example report checklist and duplicate-check listing are
//!    internally consistent (issue numbers referenced in prose match the
//!    numbers used in the example checklist/ledger where applicable).
//!
//! These tests read the markdown file directly (via `CARGO_MANIFEST_DIR`)
//! and do not depend on any runtime behavior of the `azork` crate.

use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The doc is immutable for the lifetime of the test binary, and every test
/// reads it independently (each test runs on its own thread), so cache the
/// contents once instead of re-reading the same small file from disk per
/// test.
fn read_doc() -> &'static str {
    static DOC: OnceLock<String> = OnceLock::new();
    DOC.get_or_init(|| {
        fs::read_to_string(repo_root().join("docs/CARL-OIT-CAMPAIGN.md"))
            .expect("docs/CARL-OIT-CAMPAIGN.md must exist and be UTF-8")
    })
}

fn section_body<'a>(doc: &'a str, heading: &str) -> &'a str {
    let start = doc
        .find(heading)
        .unwrap_or_else(|| panic!("expected to find heading {heading:?}"));
    let after = &doc[start + heading.len()..];
    let end = after.find("\n## ").unwrap_or(after.len());
    &after[..end]
}

/// Markdown prose is hard-wrapped at ~80 columns, so a sentence-spanning
/// phrase check on the raw text would spuriously fail on an embedded
/// newline mid-sentence. Collapse all whitespace runs (including newlines)
/// to single spaces before doing substring checks that span line wraps.
fn normalize_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Most phrase-containment checks below don't care about line wrapping or
/// letter case; they just want "does this section mention X". Combine
/// `section_body` + `normalize_whitespace` + lowercasing into a single
/// helper so each test doesn't have to repeat the same three-step pipeline.
fn normalized_section_lower(doc: &str, heading: &str) -> String {
    normalize_whitespace(section_body(doc, heading)).to_lowercase()
}

/// Requirement 1: a top-level "Round-based re-runs" section exists.
#[test]
fn has_round_based_rerun_section() {
    let doc = read_doc();
    assert!(
        doc.lines().any(|l| l.trim() == "## Round-based re-runs"),
        "docs/CARL-OIT-CAMPAIGN.md must contain a top-level '## Round-based re-runs' section"
    );
}

/// Requirement 1 (continued): the section documents all the required
/// round-N obligations, not just a subset.
#[test]
fn round_based_rerun_section_covers_required_obligations() {
    let doc = read_doc();
    let body = normalized_section_lower(doc, "## Round-based re-runs");

    let required_phrases = [
        "baseline",                       // states baseline commit/tag explicitly
        "re-tests every product surface", // fresh full re-test, not just diffed surfaces
        "re-verifies each pr merged",     // per-PR re-verification against its claim
        "re-verifies the status of previously filed findings", // re-verify findings
        "never re-files a duplicate",     // duplicate-avoidance policy
        "names its report issue with the round number", // naming convention
        "references the round's own tracking issue", // parent/umbrella issue varies per round
    ];

    for phrase in required_phrases {
        assert!(
            body.contains(phrase),
            "Round-based re-runs section must cover requirement phrase {phrase:?}; body was:\n{body}"
        );
    }
}

/// Requirement 2 & 3: the report-title convention is parameterized by round
/// number, and the doc explicitly states the full title is canonical while
/// shorthand is only acceptable in cross-references.
#[test]
fn round_report_title_is_parameterized_and_canonical() {
    let doc = read_doc();

    let normalized = normalize_whitespace(doc);
    assert!(
        normalized.contains("Carl OIT campaign report (round N) — latest main")
            || normalized.contains("Carl OIT campaign report (round N)"),
        "doc must show the parameterized round-report title template"
    );

    let round_section = normalized_section_lower(doc, "## Round-based re-runs");
    assert!(
        round_section.contains("canonical")
            && (round_section.contains("shorthand") || round_section.contains("shorter form")),
        "Round-based re-runs section must clarify the full parameterized title is canonical \
         and that shorthand forms are only for cross-references"
    );
}

/// Requirement 2 (continued): the "Campaign report" section documents both
/// the first-run title and the round-N title, not just one or the other.
#[test]
fn campaign_report_section_documents_both_title_forms() {
    let doc = read_doc();
    let body = section_body(doc, "## Campaign report\n");

    assert!(
        body.contains("Carl OIT campaign report"),
        "Campaign report section must mention the first-run report title"
    );
    assert!(
        body.to_lowercase().contains("round n"),
        "Campaign report section must mention the round-N report title template"
    );
}

/// Requirement 4: the findings ledger / duplicate-check text must not
/// hardcode a single fixed tracking-issue number as universally applicable;
/// it must state that the parent/umbrella issue varies by round.
#[test]
fn duplicate_check_policy_does_not_hardcode_single_umbrella_issue() {
    let doc = read_doc();
    let body = normalized_section_lower(doc, "## Duplicate-check policy");

    assert!(
        body.contains("whichever github issue requested the current run")
            || (body.contains("varies") && body.contains("round")),
        "Duplicate-check policy section must explain the parent tracking issue is \
         per-round, not a single fixed number; body was:\n{body}"
    );
}

/// Requirement 4 (continued): the campaign-report-format section also
/// references the round's own tracking issue rather than one hardcoded
/// issue number.
#[test]
fn campaign_report_format_references_round_tracking_issue_generically() {
    let doc = read_doc();
    let body = normalized_section_lower(doc, "## Campaign report format");
    assert!(
        body.contains("round's own tracking issue") || body.contains("round-based re-runs"),
        "Campaign report format section must reference the round's own tracking issue, \
         not a single hardcoded umbrella issue; body was:\n{body}"
    );
}

/// Requirement 5: fix-workstream clones use the generic
/// `/home/azureuser/src/azork-fix-<slug>` convention, and the old
/// `<bugname>-fix` placeholder is gone.
#[test]
fn fix_workstream_dispatch_uses_azork_fix_slug_convention() {
    let doc = read_doc();
    assert!(
        doc.contains("/home/azureuser/src/azork-fix-<slug>"),
        "docs/CARL-OIT-CAMPAIGN.md must document the azork-fix-<slug> clone convention"
    );
    assert!(
        !doc.contains("<bugname>-fix"),
        "docs/CARL-OIT-CAMPAIGN.md must not retain the old <bugname>-fix placeholder convention"
    );
}

/// Requirement 6: the example report checklist uses issue numbers that are
/// self-consistent (a finding issue number never equals its own fix PR
/// number within the same example line).
#[test]
fn example_report_checklist_lines_are_internally_consistent() {
    let doc = read_doc();
    let body = section_body(doc, "## Campaign report format");

    // Extract fenced example lines like:
    // - [ ] #91 REPL: ... — fix: #92 (open)
    let mut checked_any = false;
    for line in body.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("- [") {
            continue;
        }
        let hashes: Vec<&str> = trimmed
            .split(|c: char| !c.is_ascii_digit() && c != '#')
            .filter(|s| s.starts_with('#') && s.len() > 1)
            .collect();
        if hashes.len() >= 2 {
            checked_any = true;
            assert_ne!(
                hashes[0], hashes[1],
                "example checklist line must not reference the same issue number for \
                 both the finding and its fix: {trimmed:?}"
            );
        }
    }
    assert!(
        checked_any,
        "expected to find at least one example checklist line with a finding + fix pair"
    );
}
