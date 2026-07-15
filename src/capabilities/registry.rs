//! The persistent registry of learned [`Capability`] records.
//!
//! The registry is AzZork's growing memory of what the `az` CLI can do. It is
//! populated by [`crate::capabilities::derive`] and persisted to a small
//! line-based cache file so learning carries across sessions — the game
//! *evolves* as it is used.
//!
//! The on-disk format is deliberately dependency-free: one capability per line,
//! tab-separated as `command_path \t status \t summary`. Tabs and newlines in
//! fields are neutralised to spaces on write so the format stays line-safe.

use super::derive;
use super::Capability;
use crate::az_runner::AzRunner;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// A collection of learned capabilities, keyed by their command path.
#[derive(Debug, Clone, Default)]
pub struct CapabilityRegistry {
    caps: BTreeMap<String, Capability>,
}

impl CapabilityRegistry {
    /// An empty registry.
    pub fn new() -> CapabilityRegistry {
        CapabilityRegistry {
            caps: BTreeMap::new(),
        }
    }

    /// Number of capabilities known.
    pub fn len(&self) -> usize {
        self.caps.len()
    }

    /// Whether the registry has learned nothing yet.
    pub fn is_empty(&self) -> bool {
        self.caps.is_empty()
    }

    /// Insert or replace a capability, keyed by its command path.
    pub fn insert(&mut self, cap: Capability) {
        self.caps.insert(cap.key(), cap);
    }

    /// Merge many capabilities in. Returns how many were newly added.
    pub fn extend(&mut self, caps: impl IntoIterator<Item = Capability>) -> usize {
        let mut added = 0;
        for cap in caps {
            let key = cap.key();
            if !self.caps.contains_key(&key) {
                added += 1;
            }
            self.caps.insert(key, cap);
        }
        added
    }

    /// Look up a capability by its exact command path key (e.g. `"group create"`).
    pub fn get(&self, key: &str) -> Option<&Capability> {
        self.caps.get(key)
    }

    /// All capabilities whose leaf verb equals `verb` (across groups).
    pub fn find_by_verb(&self, verb: &str) -> Vec<&Capability> {
        let v = verb.to_lowercase();
        self.caps.values().filter(|c| c.verb == v).collect()
    }

    /// Iterate every known capability in stable (path-sorted) order.
    pub fn iter(&self) -> impl Iterator<Item = &Capability> {
        self.caps.values()
    }

    /// The set of groups that have at least one learned capability.
    pub fn groups(&self) -> Vec<String> {
        let mut gs: Vec<String> = self.caps.values().map(|c| c.group.clone()).collect();
        gs.sort();
        gs.dedup();
        gs
    }

    /// Suggest capabilities related to a free-text query, best first.
    ///
    /// Scoring is intentionally simple and deterministic: exact verb match beats
    /// a verb prefix, which beats a substring hit anywhere in the path or
    /// summary. Ties break on the (sorted) command path for stability.
    pub fn suggest(&self, query: &str, limit: usize) -> Vec<&Capability> {
        let q = query.to_lowercase();
        let tokens: Vec<&str> = q.split_whitespace().collect();
        if tokens.is_empty() {
            return Vec::new();
        }
        let mut scored: Vec<(i32, &Capability)> = self
            .caps
            .values()
            .filter_map(|c| {
                let score = score_capability(c, &tokens);
                if score > 0 {
                    Some((score, c))
                } else {
                    None
                }
            })
            .collect();
        // Higher score first; stable path order breaks ties.
        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.key().cmp(&b.1.key())));
        scored.into_iter().take(limit).map(|(_, c)| c).collect()
    }

    /// Render the learned capabilities as a help section, grouped by az group.
    ///
    /// Returns an empty string when nothing has been learned yet, so callers can
    /// omit the section entirely.
    pub fn help_text(&self) -> String {
        if self.caps.is_empty() {
            return String::new();
        }
        let mut out = String::from("Discovered az capabilities (learned at runtime):");
        let mut last_group = String::new();
        for cap in self.caps.values() {
            if cap.group != last_group {
                out.push_str(&format!("\n [{}]", cap.group));
                last_group = cap.group.clone();
            }
            out.push('\n');
            out.push_str(&cap.help_line());
        }
        out
    }

    // ---- Learning -------------------------------------------------------

    /// Learn every command in a single group via `az <group> --help`.
    ///
    /// Returns the number of newly-added capabilities on success.
    pub fn learn_group(&mut self, runner: &dyn AzRunner, group: &str) -> Result<usize, String> {
        let caps = derive::derive_group_capabilities(runner, group)?;
        Ok(self.extend(caps))
    }

    /// Discover the available top-level groups via `az --help`.
    pub fn discover_groups(&self, runner: &dyn AzRunner) -> Result<Vec<String>, String> {
        derive::derive_groups(runner)
    }

    // ---- Persistence ----------------------------------------------------

    /// Load a registry from the cache file at `path`. A missing file yields an
    /// empty registry (first run); malformed lines are skipped defensively.
    pub fn load(path: &Path) -> CapabilityRegistry {
        let mut reg = CapabilityRegistry::new();
        let Ok(text) = fs::read_to_string(path) else {
            return reg;
        };
        for line in text.lines() {
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(cap) = parse_cache_line(line) {
                reg.insert(cap);
            }
        }
        reg
    }

    /// Persist the registry to the cache file at `path`, creating parent dirs.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).map_err(|e| {
                    format!("could not create cache dir {}: {}", parent.display(), e)
                })?;
            }
        }
        let mut body = String::from("# AzZork learned capabilities cache\n");
        for cap in self.caps.values() {
            body.push_str(&format_cache_line(cap));
            body.push('\n');
        }
        fs::write(path, body)
            .map_err(|e| format!("could not write cache {}: {}", path.display(), e))
    }
}

/// Default cache location: `$AZORK_CACHE_DIR/capabilities.tsv`, else
/// `$XDG_DATA_HOME/azork/…`, else `~/.local/share/azork/…`, else `./`.
pub fn default_cache_path() -> PathBuf {
    if let Ok(dir) = std::env::var("AZORK_CACHE_DIR") {
        return PathBuf::from(dir).join("capabilities.tsv");
    }
    let base = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .ok()
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|h| PathBuf::from(h).join(".local/share"))
        })
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("azork").join("capabilities.tsv")
}

/// Score a capability against query tokens (0 = no match).
fn score_capability(cap: &Capability, tokens: &[&str]) -> i32 {
    let verb = cap.verb.to_lowercase();
    let path = cap.key().to_lowercase();
    let summary = cap.summary.to_lowercase();
    let group = cap.group.to_lowercase();
    let mut score = 0;
    for &t in tokens {
        if verb == t {
            score += 100;
        } else if verb.starts_with(t) {
            score += 60;
        } else if group == t {
            score += 40;
        } else if path.contains(t) {
            score += 25;
        } else if summary.contains(t) {
            score += 10;
        }
    }
    score
}

/// Serialise a capability to one cache line.
fn format_cache_line(cap: &Capability) -> String {
    let status = cap.status.clone().unwrap_or_else(|| "-".to_string());
    format!(
        "{}\t{}\t{}",
        sanitize(&cap.command_path.join(" ")),
        sanitize(&status),
        sanitize(&cap.summary),
    )
}

/// Parse one cache line back into a capability.
fn parse_cache_line(line: &str) -> Option<Capability> {
    let mut parts = line.splitn(3, '\t');
    let path = parts.next()?.trim();
    let status_raw = parts.next().unwrap_or("-").trim();
    let summary = parts.next().unwrap_or("").trim();
    let segments: Vec<String> = path.split_whitespace().map(|s| s.to_string()).collect();
    if segments.len() < 2 {
        return None;
    }
    let group = segments[0].clone();
    let verb = segments.last().unwrap().clone();
    let status = if status_raw == "-" || status_raw.is_empty() {
        None
    } else {
        Some(status_raw.to_string())
    };
    Some(Capability {
        group,
        verb,
        summary: summary.to_string(),
        command_path: segments,
        status,
    })
}

/// Replace tabs/newlines with spaces so a field stays on one line.
fn sanitize(s: &str) -> String {
    s.replace(['\t', '\n', '\r'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::az_runner::FakeAzRunner;

    fn cap(group: &str, verb: &str, summary: &str) -> Capability {
        Capability::new(group, verb, summary, None)
    }

    #[test]
    fn insert_and_lookup() {
        let mut reg = CapabilityRegistry::new();
        reg.insert(cap("group", "create", "Create a new resource group."));
        assert_eq!(reg.len(), 1);
        assert_eq!(reg.get("group create").unwrap().verb, "create");
        assert_eq!(reg.find_by_verb("create").len(), 1);
    }

    #[test]
    fn extend_reports_new_additions() {
        let mut reg = CapabilityRegistry::new();
        let added = reg.extend(vec![cap("group", "create", "x"), cap("group", "list", "y")]);
        assert_eq!(added, 2);
        // Re-inserting the same keys adds nothing new.
        let again = reg.extend(vec![cap("group", "create", "x")]);
        assert_eq!(again, 0);
    }

    #[test]
    fn suggest_ranks_exact_verb_first() {
        let mut reg = CapabilityRegistry::new();
        reg.insert(cap("group", "create", "Create a new resource group."));
        reg.insert(cap("storage", "list", "List storage accounts."));
        reg.insert(cap("vm", "create", "Create a virtual machine."));
        let hits = reg.suggest("create", 5);
        assert!(hits.iter().all(|c| c.verb == "create"));
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn help_text_groups_capabilities() {
        let mut reg = CapabilityRegistry::new();
        reg.insert(cap("group", "create", "Create a new resource group."));
        reg.insert(cap("storage", "list", "List storage accounts."));
        let help = reg.help_text();
        assert!(help.contains("[group]"));
        assert!(help.contains("[storage]"));
        assert!(help.contains("az group create"));
    }

    #[test]
    fn empty_registry_help_is_blank() {
        assert_eq!(CapabilityRegistry::new().help_text(), "");
    }

    #[test]
    fn learn_group_from_runner() {
        let help = "\nCommands:\n    create : Create a new resource group.\n    list   : List resource groups.\n";
        let runner = FakeAzRunner::new().with(&["group", "--help"], help);
        let mut reg = CapabilityRegistry::new();
        let added = reg.learn_group(&runner, "group").unwrap();
        assert_eq!(added, 2);
        assert!(reg.get("group create").is_some());
    }

    #[test]
    fn round_trips_through_cache() {
        let dir = std::env::temp_dir().join(format!("azork-reg-{}", std::process::id()));
        let path = dir.join("capabilities.tsv");
        let mut reg = CapabilityRegistry::new();
        reg.insert(Capability::new(
            "group",
            "create",
            "Create a new\tresource group.",
            Some("Preview".to_string()),
        ));
        reg.save(&path).unwrap();

        let loaded = CapabilityRegistry::load(&path);
        assert_eq!(loaded.len(), 1);
        let c = loaded.get("group create").unwrap();
        assert_eq!(c.status.as_deref(), Some("Preview"));
        assert!(!c.summary.contains('\t'));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_missing_file_is_empty() {
        let path = Path::new("/nonexistent/azork/does-not-exist.tsv");
        assert!(CapabilityRegistry::load(path).is_empty());
    }
}
