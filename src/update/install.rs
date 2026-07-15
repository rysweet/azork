//! Atomic self-replacement of the running binary.
//!
//! The download is verified against its published SHA-256 **before** the binary
//! is extracted, and the new binary is written to a sibling temp file and then
//! `rename`d over the current executable (atomic on the same filesystem). No
//! privilege escalation is attempted: if the install directory is not writable
//! the update fails cleanly with [`UpdateError::TargetNotWritable`] (exit 4).

use super::{checksum, extract_binary, post_install, ResolvedUpdate, Result, UpdateError};
use std::path::{Path, PathBuf};

/// The binary name inside the release archive and on disk.
const BINARY_NAME: &str = "azork";

/// Download, verify, extract and install the resolved update, replacing the
/// currently running executable. Returns the path to the installed binary.
pub fn download_and_replace(release: &ResolvedUpdate) -> Result<PathBuf> {
    // 1. Download the archive.
    let archive_bytes = super::network::download_asset(&release.asset_url)?;

    // 2. Verify the checksum BEFORE touching disk (fail-closed).
    let checksum_url = release.checksum_url.as_deref().ok_or_else(|| {
        UpdateError::Archive(
            "release is missing its .sha256 checksum asset; refusing to install".into(),
        )
    })?;
    let checksum_bytes = super::network::download_asset(checksum_url)?;
    let checksum_text = String::from_utf8_lossy(&checksum_bytes);
    checksum::verify_or_error(&archive_bytes, &checksum_text)?;

    // 3. Extract the binary into a scratch directory.
    let scratch = scratch_dir()?;
    let extracted = extract_binary(&archive_bytes, BINARY_NAME, &scratch);
    let extracted = match extracted {
        Ok(p) => p,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&scratch);
            return Err(e);
        }
    };

    // 4. Atomically replace the current executable.
    let current_exe = std::env::current_exe()
        .map_err(|e| UpdateError::Io(format!("cannot determine current executable: {e}")))?;
    let result = install_binary_atomic(&extracted, &current_exe);
    let _ = std::fs::remove_dir_all(&scratch);
    result?;

    // 5. Stamp the installed version next to the executable (best effort — a
    //    stamp failure must not undo a successful binary swap).
    if let Err(e) = post_install::write_version_stamp(&current_exe, &release.version.to_string()) {
        eprintln!(
            "warning: installed azork {} but failed to write version stamp: {e}",
            release.version
        );
    }

    Ok(current_exe)
}

/// Create a unique scratch directory for extraction.
fn scratch_dir() -> Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!(
        "azork-update-{}-{}",
        std::process::id(),
        super::now_unix()
    ));
    std::fs::create_dir_all(&dir)
        .map_err(|e| UpdateError::Io(format!("create scratch dir {}: {e}", dir.display())))?;
    Ok(dir)
}

/// Copy `source` to a sibling temp of `destination`, make it executable, then
/// atomically rename it over `destination`.
fn install_binary_atomic(source: &Path, destination: &Path) -> Result<()> {
    let parent = destination
        .parent()
        .ok_or_else(|| UpdateError::TargetNotWritable(destination.display().to_string()))?;

    let temp_dest = destination.with_extension(format!("new-{}", std::process::id()));

    std::fs::copy(source, &temp_dest).map_err(|e| map_write_error(parent, e))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(e) = std::fs::set_permissions(&temp_dest, std::fs::Permissions::from_mode(0o755))
        {
            let _ = std::fs::remove_file(&temp_dest);
            return Err(UpdateError::Io(format!(
                "chmod {}: {e}",
                temp_dest.display()
            )));
        }
    }

    if let Err(e) = std::fs::rename(&temp_dest, destination) {
        let _ = std::fs::remove_file(&temp_dest);
        return Err(map_write_error(destination, e));
    }
    Ok(())
}

/// Map a filesystem write error to the right [`UpdateError`], distinguishing
/// permission problems (exit 4) from other I/O failures.
fn map_write_error(path: &Path, e: std::io::Error) -> UpdateError {
    if e.kind() == std::io::ErrorKind::PermissionDenied {
        UpdateError::TargetNotWritable(path.display().to_string())
    } else {
        UpdateError::Io(format!("write {}: {e}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_install_replaces_target_file() {
        let dir = scratch_dir().unwrap();
        let src = dir.join("azork-new");
        std::fs::write(&src, b"NEW BINARY").unwrap();
        let dest = dir.join("azork");
        std::fs::write(&dest, b"OLD BINARY").unwrap();

        install_binary_atomic(&src, &dest).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"NEW BINARY");
        // No leftover temp file.
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains("new-"))
            .collect();
        assert!(leftovers.is_empty(), "temp file leaked: {leftovers:?}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_error_permission_maps_to_exit_4() {
        let e = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        assert_eq!(map_write_error(Path::new("/x"), e).exit_code(), 4);
    }
}
