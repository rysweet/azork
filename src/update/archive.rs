//! Traversal-safe extraction of a single binary from a `.tar.gz` archive.
//!
//! Release archives are attacker-influenced input (a compromised release, or a
//! MITM that slipped past TLS + checksum, could contain hostile entries), so
//! [`extract_binary`] refuses any entry that could write outside the
//! destination directory: parent-directory (`..`) components, absolute paths,
//! and symlink/hardlink entries are all rejected before anything is written.

use super::{Result, UpdateError};
use flate2::read::GzDecoder;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

/// Returns `true` if `path` is safe to extract *within* a destination dir:
/// it is relative and contains no `..` component.
fn is_safe_relative(path: &Path) -> bool {
    let mut saw_normal = false;
    for component in path.components() {
        match component {
            Component::Normal(_) | Component::CurDir => saw_normal = true,
            // Absolute (root / prefix) or parent (`..`) → unsafe.
            Component::RootDir | Component::Prefix(_) | Component::ParentDir => return false,
        }
    }
    // A completely empty path is not a valid entry.
    saw_normal
}

/// Extract the file named `binary_name` from the gzip-compressed tar archive in
/// `archive_bytes` into `dest_dir`, returning the path to the written file.
///
/// The binary may live at the archive root or inside a single wrapping
/// directory (`azork-<version>/azork`). Every entry is validated for path
/// safety first; a single unsafe entry aborts the whole extraction and writes
/// nothing outside `dest_dir`.
pub fn extract_binary(archive_bytes: &[u8], binary_name: &str, dest_dir: &Path) -> Result<PathBuf> {
    let decoder = GzDecoder::new(archive_bytes);
    let mut archive = tar::Archive::new(decoder);

    let entries = archive
        .entries()
        .map_err(|e| UpdateError::Archive(format!("cannot read tar entries: {e}")))?;

    for entry in entries {
        let mut entry =
            entry.map_err(|e| UpdateError::Archive(format!("corrupt tar entry: {e}")))?;

        // Reject anything that isn't a plain file or directory outright —
        // symlinks/hardlinks/devices could redirect writes outside dest_dir.
        let entry_type = entry.header().entry_type();
        if !(entry_type.is_file() || entry_type.is_dir()) {
            return Err(UpdateError::Archive(format!(
                "archive contains a disallowed entry type: {entry_type:?}"
            )));
        }

        let path = entry
            .path()
            .map_err(|e| UpdateError::Archive(format!("invalid entry path: {e}")))?
            .into_owned();

        if !is_safe_relative(&path) {
            return Err(UpdateError::Archive(format!(
                "unsafe archive entry rejected (path traversal): {}",
                path.display()
            )));
        }

        if entry_type.is_dir() {
            continue;
        }

        let matches = path
            .file_name()
            .map(|n| n == std::ffi::OsStr::new(binary_name))
            .unwrap_or(false);
        if !matches {
            continue;
        }

        let dest = dest_dir.join(binary_name);
        let mut buf = Vec::new();
        entry.read_to_end(&mut buf).map_err(|e| {
            UpdateError::Archive(format!("cannot read {binary_name} from tar: {e}"))
        })?;
        std::fs::write(&dest, &buf)
            .map_err(|e| UpdateError::Io(format!("write {}: {e}", dest.display())))?;
        set_executable(&dest)?;
        return Ok(dest);
    }

    Err(UpdateError::Archive(format!(
        "archive does not contain a '{binary_name}' binary"
    )))
}

#[cfg(unix)]
fn set_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| UpdateError::Io(format!("chmod {}: {e}", path.display())))
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_relative_accepts_nested() {
        assert!(is_safe_relative(Path::new("azork-0.3.0/azork")));
        assert!(is_safe_relative(Path::new("azork")));
    }

    #[test]
    fn safe_relative_rejects_traversal_and_absolute() {
        assert!(!is_safe_relative(Path::new("../evil")));
        assert!(!is_safe_relative(Path::new("/etc/passwd")));
        assert!(!is_safe_relative(Path::new("a/../../b")));
        assert!(!is_safe_relative(Path::new("")));
    }
}
