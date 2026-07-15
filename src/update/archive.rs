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

/// Hard cap on the *decompressed* size of the extracted binary. The download is
/// already capped, but a compromised (yet checksum-consistent) release could
/// declare an enormous tar entry; bounding the streamed output guards against
/// disk-exhaustion decompression bombs.
const MAX_EXTRACTED_BYTES: u64 = 512 * 1024 * 1024; // 512 MiB

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
        // Stream the entry straight to disk rather than buffering the whole
        // binary in memory first, but bound the output so a malicious tar entry
        // cannot exhaust the disk. Read one byte past the cap to detect overflow.
        let mut out = std::fs::File::create(&dest)
            .map_err(|e| UpdateError::Io(format!("create {}: {e}", dest.display())))?;
        let mut limited = entry.by_ref().take(MAX_EXTRACTED_BYTES + 1);
        let written = std::io::copy(&mut limited, &mut out).map_err(|e| {
            UpdateError::Archive(format!("cannot extract {binary_name} from tar: {e}"))
        })?;
        if written > MAX_EXTRACTED_BYTES {
            let _ = std::fs::remove_file(&dest);
            return Err(UpdateError::Archive(format!(
                "extracted '{binary_name}' exceeded the {MAX_EXTRACTED_BYTES} byte cap"
            )));
        }
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
