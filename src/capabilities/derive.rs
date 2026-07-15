//! Parsing of `az` help text into [`Capability`] records.
//!
//! The `az` help format is consistent and plain-text (no JSON dependency
//! needed). A group's help looks like:
//!
//! ```text
//! Group
//!     az group : Manage resource groups and template deployments.
//!
//! Subgroups:
//!     lock   : Manage Azure resource group locks.
//!
//! Commands:
//!     create : Create a new resource group.
//!     list   : List resource groups.
//! ```
//!
//! Entry lines are `    name [Tag] : summary`, where `[Tag]` is an optional
//! lifecycle marker (`[Preview]`, `[Experimental]`, `[Deprecated]`). Long
//! summaries wrap onto more-indented continuation lines with no ` : ` — we
//! stitch those back onto the preceding entry.

use super::Capability;
use crate::az_runner::AzRunner;
use crate::secrets::scrub;

/// One entry parsed out of a help section: a name, its optional lifecycle tag,
/// and its (possibly reflowed) summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelpEntry {
    pub name: String,
    pub status: Option<String>,
    pub summary: String,
}

/// Sections of `az` help we care about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    Subgroups,
    Commands,
    Other,
}

/// Discover the top-level command groups by parsing `az --help`.
///
/// Returns the group names (e.g. `"group"`, `"storage"`, `"vm"`). On CLI
/// failure returns an error string; the caller decides whether that is fatal.
pub fn derive_groups(runner: &dyn AzRunner) -> Result<Vec<String>, String> {
    let text = run_help(runner, &["--help"])?;
    Ok(parse_section(&text, Section::Subgroups)
        .into_iter()
        .map(|e| e.name)
        .collect())
}

/// Derive the capabilities (leaf commands) of a single group via
/// `az <group> --help`.
pub fn derive_group_capabilities(
    runner: &dyn AzRunner,
    group: &str,
) -> Result<Vec<Capability>, String> {
    let text = run_help(runner, &[group, "--help"])?;
    Ok(parse_section(&text, Section::Commands)
        .into_iter()
        .map(|e| Capability::new(group, &e.name, &e.summary, e.status))
        .collect())
}

/// Run `az <args>` and return trimmed stdout, or a friendly error string.
///
/// `az` help text is not expected to contain secrets, but this is the same
/// seam ([`AzRunner::run`]) used by the live backend, and `az` extensions or
/// future help text could in principle echo environment-influenced content.
/// Both the success and failure paths are scrubbed defensively, matching the
/// symmetric treatment applied in [`crate::backend::az`].
fn run_help(runner: &dyn AzRunner, args: &[&str]) -> Result<String, String> {
    let out = runner
        .run(args)
        .map_err(|e| format!("failed to run 'az {}': {}", args.join(" "), e))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!(
            "'az {}' failed: {}",
            args.join(" "),
            scrub(stderr.trim())
        ));
    }
    Ok(scrub(&String::from_utf8_lossy(&out.stdout)))
}

/// Parse a named section (`Subgroups:` / `Commands:`) out of help text.
///
/// The parser is whitespace-tolerant: it recognises a section header, then
/// collects indented `name [Tag] : summary` entries until the next header or a
/// blank-line-terminated section, folding wrapped continuation lines back in.
fn parse_section(text: &str, want: Section) -> Vec<HelpEntry> {
    let mut entries: Vec<HelpEntry> = Vec::new();
    let mut current = Section::Other;

    for line in text.lines() {
        let trimmed = line.trim_end();

        // Section headers sit flush-left and end in a colon.
        if let Some(section) = header_section(trimmed) {
            current = section;
            continue;
        }

        if trimmed.trim().is_empty() {
            // A blank line ends the current section's run of entries.
            current = Section::Other;
            continue;
        }

        if current != want {
            continue;
        }

        // Entry vs. continuation: an entry has a ` : ` (or trailing name with a
        // colon). Continuation lines are indented text with no name column.
        match parse_entry(trimmed) {
            Some(entry) => entries.push(entry),
            None => {
                if let Some(last) = entries.last_mut() {
                    let cont = trimmed.trim();
                    if !cont.is_empty() {
                        if !last.summary.is_empty() {
                            last.summary.push(' ');
                        }
                        last.summary.push_str(cont);
                    }
                }
            }
        }
    }

    entries
}

/// Identify a section header line, if any.
fn header_section(line: &str) -> Option<Section> {
    // Headers are not indented and look like `Commands:` / `Subgroups:`.
    if line.starts_with(char::is_whitespace) {
        return None;
    }
    match line.trim_end_matches(':') {
        "Subgroups" => Some(Section::Subgroups),
        "Commands" => Some(Section::Commands),
        // Any other flush-left, non-indented word starts a section we ignore.
        other if !other.is_empty() && !other.contains(' ') && line.ends_with(':') => {
            Some(Section::Other)
        }
        _ => None,
    }
}

/// Parse a single `name [Tag] : summary` entry line. Returns `None` for lines
/// that are continuations rather than fresh entries.
fn parse_entry(line: &str) -> Option<HelpEntry> {
    // Must be indented to be an entry (headers are handled elsewhere).
    if !line.starts_with(char::is_whitespace) {
        return None;
    }
    let body = line.trim();

    // Split name-part from summary at the first " : " separator.
    let (name_part, summary) = match body.split_once(" : ") {
        Some((n, s)) => (n.trim(), s.trim().to_string()),
        None => {
            // Some entries have no summary yet: "name :" or just wrap text.
            let n = body.strip_suffix(" :").or_else(|| body.strip_suffix(':'))?;
            (n.trim(), String::new())
        }
    };

    if name_part.is_empty() {
        return None;
    }

    // A lifecycle tag, if present, is a trailing `[...]` on the name part.
    let (name, status) = split_status(name_part);
    if name.is_empty() || name.contains(' ') {
        // A real command name is a single token; anything else is not an entry.
        return None;
    }

    Some(HelpEntry {
        name: name.to_string(),
        status,
        summary,
    })
}

/// Separate a trailing `[Preview]`-style tag from a name token.
fn split_status(name_part: &str) -> (String, Option<String>) {
    let np = name_part.trim();
    if let Some(open) = np.find('[') {
        if np.ends_with(']') {
            let name = np[..open].trim().to_string();
            let status = np[open + 1..np.len() - 1].trim().to_string();
            return (
                name,
                if status.is_empty() {
                    None
                } else {
                    Some(status)
                },
            );
        }
    }
    (np.to_string(), None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::az_runner::FakeAzRunner;

    const GROUP_HELP: &str = "\nGroup\n    az group : Manage resource groups and template deployments.\n\nSubgroups:\n    lock   : Manage Azure resource group locks.\n\nCommands:\n    create : Create a new resource group.\n    delete : Delete a resource group.\n    list   : List resource groups.\n    wait   : Place the CLI in a waiting state until a condition of the resource\n             group is met.\n\nTo search AI knowledge base for examples, use: az find \"az group\"\n";

    const ROOT_HELP: &str = "\nGroup\n    az\n\nSubgroups:\n    account                 : Manage Azure subscription information.\n    group                   : Manage resource groups.\n    storage                 : Manage Azure Cloud Storage resources.\n    compute-fleet [Preview] : Manage for Azure Compute Fleet.\n\nCommands:\n    login : Log in to Azure.\n";

    #[test]
    fn parses_group_commands() {
        let runner = FakeAzRunner::new().with(&["group", "--help"], GROUP_HELP);
        let caps = derive_group_capabilities(&runner, "group").unwrap();
        let verbs: Vec<&str> = caps.iter().map(|c| c.verb.as_str()).collect();
        assert_eq!(verbs, vec!["create", "delete", "list", "wait"]);
    }

    #[test]
    fn folds_wrapped_summary() {
        let runner = FakeAzRunner::new().with(&["group", "--help"], GROUP_HELP);
        let caps = derive_group_capabilities(&runner, "group").unwrap();
        let wait = caps.iter().find(|c| c.verb == "wait").unwrap();
        assert!(wait.summary.contains("waiting state"));
        assert!(wait
            .summary
            .contains("condition of the resource group is met"));
        assert!(!wait.summary.contains('\n'));
    }

    #[test]
    fn parses_root_groups_and_status_tags() {
        let runner = FakeAzRunner::new().with(&["--help"], ROOT_HELP);
        let groups = derive_groups(&runner).unwrap();
        assert!(groups.contains(&"storage".to_string()));
        assert!(groups.contains(&"group".to_string()));
        assert!(groups.contains(&"compute-fleet".to_string()));
    }

    #[test]
    fn status_tag_is_extracted() {
        let (name, status) = split_status("compute-fleet [Preview]");
        assert_eq!(name, "compute-fleet");
        assert_eq!(status.as_deref(), Some("Preview"));
        let (n2, s2) = split_status("create");
        assert_eq!(n2, "create");
        assert_eq!(s2, None);
    }

    #[test]
    fn cli_failure_surfaces_error() {
        let runner = FakeAzRunner::new();
        let err = derive_group_capabilities(&runner, "group").unwrap_err();
        assert!(err.contains("failed"));
    }

    #[test]
    fn cli_failure_scrubs_secrets_from_stderr() {
        // `run_help`'s error path must scrub `az`'s stderr symmetrically
        // with `backend::az::run_once`, in case a hostile/misconfigured
        // extension ever echoes a secret into help-command failure text.
        let hostile_stderr = format!(
            "az cli extension error: token: {}",
            crate::secrets::test_fixtures::HOSTILE_TOKEN
        );
        let runner = FakeAzRunner::new().with_failure(&["group", "--help"], &hostile_stderr);
        let err = derive_group_capabilities(&runner, "group").unwrap_err();
        assert!(!err.contains(crate::secrets::test_fixtures::HOSTILE_TOKEN));
        assert!(err.contains("failed"));
    }

    #[test]
    fn cli_success_scrubs_secrets_from_stdout() {
        // Defense-in-depth: `run_help`'s success path scrubs stdout too, so
        // a future `az` extension that echoes secret-shaped text in help
        // output can never leak it into derived capability summaries.
        let hostile_help = format!(
            "\nGroup\n    az group : Manage resource groups.\n\nCommands:\n    create : Uses {}.\n",
            crate::secrets::test_fixtures::HOSTILE_ACCOUNT_KEY_FRAGMENT
        );
        let runner = FakeAzRunner::new().with(&["group", "--help"], &hostile_help);
        let caps = derive_group_capabilities(&runner, "group").unwrap();
        let create = caps.iter().find(|c| c.verb == "create").unwrap();
        assert!(!create
            .summary
            .contains(crate::secrets::test_fixtures::HOSTILE_ACCOUNT_KEY_VALUE));
    }
}
