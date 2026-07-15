//! TDD contract tests for the `.installed-version` stamp and drift detection
//! in `azork::update::post_install`.
//!
//! After a successful install AzZork writes an `.installed-version` stamp next
//! to the executable via an **atomic write** (write-temp-then-rename). On the
//! next run it compares the stamp against the running binary's version:
//!   * in sync            -> no drift,
//!   * stamp missing      -> drift (re-stamp),
//!   * stamp mismatched   -> drift (binary swapped out of band).
//!
//! Contract:
//!   * `stamp_path(exe: &Path) -> PathBuf`
//!   * `write_version_stamp(exe: &Path, version: &str) -> io::Result<()>`
//!   * `read_version_stamp(exe: &Path) -> Option<String>`
//!   * `has_drift(exe: &Path, running_version: &str) -> bool`

use azork::update::post_install::{
    has_drift, read_version_stamp, stamp_path, write_version_stamp,
};
use std::path::{Path, PathBuf};

#[test]
fn stamp_path_sits_next_to_executable() {
    let exe = Path::new("/opt/azork/bin/azork");
    let stamp = stamp_path(exe);
    assert_eq!(stamp.parent(), exe.parent());
    assert_eq!(stamp.file_name().unwrap(), ".installed-version");
}

#[test]
fn write_then_read_round_trips() {
    let dir = tempdir();
    let exe = dir.path().join("azork");
    std::fs::write(&exe, b"fake binary").unwrap();

    write_version_stamp(&exe, "0.3.0").expect("write stamp");
    assert_eq!(read_version_stamp(&exe).as_deref(), Some("0.3.0"));
}

#[test]
fn write_is_atomic_and_leaves_no_temp_files() {
    let dir = tempdir();
    let exe = dir.path().join("azork");
    std::fs::write(&exe, b"fake binary").unwrap();

    write_version_stamp(&exe, "1.2.3").unwrap();

    // Only the binary and the final stamp should remain — no leftover *.tmp.
    let names: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(names.contains(&"azork".to_string()));
    assert!(names.contains(&".installed-version".to_string()));
    assert!(
        !names.iter().any(|n| n.ends_with(".tmp")),
        "atomic write must not leave temp files behind: {names:?}"
    );
}

#[test]
fn read_missing_stamp_is_none() {
    let dir = tempdir();
    let exe = dir.path().join("azork");
    std::fs::write(&exe, b"fake binary").unwrap();
    assert_eq!(read_version_stamp(&exe), None);
}

#[test]
fn no_drift_when_stamp_matches_running_version() {
    let dir = tempdir();
    let exe = dir.path().join("azork");
    std::fs::write(&exe, b"fake binary").unwrap();

    write_version_stamp(&exe, "0.3.0").unwrap();
    assert!(!has_drift(&exe, "0.3.0"));
}

#[test]
fn drift_when_stamp_missing() {
    let dir = tempdir();
    let exe = dir.path().join("azork");
    std::fs::write(&exe, b"fake binary").unwrap();
    assert!(has_drift(&exe, "0.3.0"));
}

#[test]
fn drift_when_stamp_mismatched() {
    let dir = tempdir();
    let exe = dir.path().join("azork");
    std::fs::write(&exe, b"fake binary").unwrap();

    write_version_stamp(&exe, "0.2.0").unwrap();
    assert!(has_drift(&exe, "0.3.0"));
}

#[test]
fn rewriting_stamp_updates_value() {
    let dir = tempdir();
    let exe = dir.path().join("azork");
    std::fs::write(&exe, b"fake binary").unwrap();

    write_version_stamp(&exe, "0.2.0").unwrap();
    write_version_stamp(&exe, "0.3.0").unwrap();
    assert_eq!(read_version_stamp(&exe).as_deref(), Some("0.3.0"));
    assert!(!has_drift(&exe, "0.3.0"));
}

// --- tiny temp-dir helper -------------------------------------------------

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn tempdir() -> TempDir {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path =
        std::env::temp_dir().join(format!("azork-stamp-test-{nanos}-{:p}", &nanos as *const _));
    std::fs::create_dir_all(&path).expect("create temp dir");
    TempDir { path }
}
