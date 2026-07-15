//! Post-install version stamping and drift detection (self-heal).
//!
//! After a successful install AzZork writes an `.installed-version` stamp next
//! to the executable using an **atomic write** (write-temp-then-rename, so a
//! crash or concurrent reader never observes a half-written stamp). On the next
//! run [`has_drift`] compares the stamp against the running binary's version:
//!
//! * in sync          → no drift,
//! * stamp missing    → drift (re-stamp),
//! * stamp mismatched → drift (binary was swapped out of band, e.g. by a
//!   package manager or a manual copy).
//!
//! Drift detection lets the updater re-stamp so the cache and startup check
//! stay honest about what is actually installed.

use std::io;
use std::path::{Path, PathBuf};

/// The stamp filename written next to the executable.
const STAMP_FILE_NAME: &str = ".installed-version";

/// Path to the `.installed-version` stamp that sits next to `exe`.
///
/// If `exe` has no parent (e.g. a bare filename), the stamp is placed in the
/// current directory.
pub fn stamp_path(exe: &Path) -> PathBuf {
    match exe.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir.join(STAMP_FILE_NAME),
        _ => PathBuf::from(STAMP_FILE_NAME),
    }
}

/// Atomically write `version` to the stamp next to `exe`.
///
/// The value is first written to a sibling `*.tmp` file and then renamed over
/// the final path, so the stamp is never observed partially written and no temp
/// file is left behind on success.
pub fn write_version_stamp(exe: &Path, version: &str) -> io::Result<()> {
    let path = stamp_path(exe);
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, version.as_bytes())?;
    // Rename is atomic on the same filesystem; the tmp sibling shares the dir.
    match std::fs::rename(&tmp, &path) {
        Ok(()) => Ok(()),
        Err(e) => {
            // Clean up the temp file so a failed rename doesn't leak it.
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

/// Read the stamped version next to `exe`, if the stamp exists and is readable.
///
/// Returns `None` when the stamp is absent; surrounding whitespace is trimmed.
pub fn read_version_stamp(exe: &Path) -> Option<String> {
    let path = stamp_path(exe);
    let raw = std::fs::read_to_string(path).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Return `true` when the on-disk stamp disagrees with `running_version`
/// (including when the stamp is missing).
pub fn has_drift(exe: &Path, running_version: &str) -> bool {
    match read_version_stamp(exe) {
        Some(stamped) => stamped != running_version,
        None => true,
    }
}

/// Self-heal the stamp for the currently running executable.
///
/// If the stamp is missing or disagrees with `running_version`, rewrite it so
/// the recorded version matches reality. Returns `Ok(true)` when a re-stamp was
/// performed, `Ok(false)` when the stamp was already in sync.
pub fn self_heal(exe: &Path, running_version: &str) -> io::Result<bool> {
    if has_drift(exe, running_version) {
        write_version_stamp(exe, running_version)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn self_heal_restamps_on_drift() {
        let dir = std::env::temp_dir().join(format!("azork-ph-{}", super::super::now_unix()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("azork");
        std::fs::write(&exe, b"bin").unwrap();

        // Missing stamp → drift → re-stamp.
        assert!(self_heal(&exe, "0.3.0").unwrap());
        assert_eq!(read_version_stamp(&exe).as_deref(), Some("0.3.0"));
        // Already in sync → no re-stamp.
        assert!(!self_heal(&exe, "0.3.0").unwrap());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
