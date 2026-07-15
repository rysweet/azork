//! TDD contract tests for `azork::update::extract_binary`.
//!
//! `extract_binary(archive_bytes, binary_name, dest_dir) -> Result<PathBuf, UpdateError>`
//! extracts the single expected binary from a `.tar.gz` archive into
//! `dest_dir`, and is **traversal-safe**: entries containing `..`, absolute
//! paths, or symlinks must be rejected so a malicious archive can never write
//! outside `dest_dir`.
//!
//! Offline only — archives are assembled in-memory.

use azork::update::extract_binary;
use flate2::write::GzEncoder;
use flate2::Compression;
use std::path::Path;

/// Build a gzip-compressed tar archive from `(path, contents)` entries.
fn make_targz(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let buf = Vec::new();
    let enc = GzEncoder::new(buf, Compression::default());
    let mut builder = tar::Builder::new(enc);
    for (name, contents) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_size(contents.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        builder
            .append_data(&mut header, name, *contents)
            .expect("append tar entry");
    }
    let enc = builder.into_inner().expect("finish tar");
    enc.finish().expect("finish gzip")
}

/// Build a gzip-compressed tar archive with a **raw** entry name, bypassing the
/// tar crate's write-side path validation. This is required to construct the
/// adversarial (traversal / absolute-path) archives that `extract_binary` must
/// reject — the safe `append_data` API refuses to build them.
fn make_targz_raw(name: &str, contents: &[u8]) -> Vec<u8> {
    let buf = Vec::new();
    let enc = GzEncoder::new(buf, Compression::default());
    let mut builder = tar::Builder::new(enc);
    let mut header = tar::Header::new_gnu();
    {
        let gnu = header.as_gnu_mut().expect("gnu header");
        let bytes = name.as_bytes();
        assert!(bytes.len() <= gnu.name.len(), "raw name too long for test");
        gnu.name[..bytes.len()].copy_from_slice(bytes);
    }
    header.set_size(contents.len() as u64);
    header.set_mode(0o644);
    header.set_entry_type(tar::EntryType::Regular);
    header.set_cksum();
    builder.append(&header, contents).expect("append raw entry");
    let enc = builder.into_inner().expect("finish tar");
    enc.finish().expect("finish gzip")
}

#[test]
fn extract_returns_the_named_binary() {
    let payload = b"#!/fake azork binary\x00\x01\x02";
    let archive = make_targz(&[("azork", payload)]);

    let dir = tempdir();
    let extracted = extract_binary(&archive, "azork", dir.path()).expect("extraction succeeds");

    assert!(extracted.exists(), "extracted file should exist");
    assert_eq!(extracted.file_name().unwrap(), "azork");
    let got = std::fs::read(&extracted).unwrap();
    assert_eq!(got, payload);
    // Must stay within dest_dir.
    assert!(extracted.starts_with(dir.path()));
}

#[test]
fn extract_finds_binary_inside_nested_dir() {
    // Release archives sometimes wrap the binary in a top-level folder.
    let payload = b"nested azork";
    let archive = make_targz(&[("azork-0.3.0/azork", payload)]);

    let dir = tempdir();
    let extracted = extract_binary(&archive, "azork", dir.path()).expect("finds nested binary");
    assert_eq!(std::fs::read(&extracted).unwrap(), payload);
}

#[test]
fn extract_rejects_missing_binary() {
    let archive = make_targz(&[("README.txt", b"no binary here")]);
    let dir = tempdir();
    assert!(extract_binary(&archive, "azork", dir.path()).is_err());
}

#[test]
fn extract_rejects_parent_traversal_entry() {
    // Zip-slip style: an entry that would escape dest_dir must be refused and
    // must NOT create a file outside dest_dir.
    let dir = tempdir();
    let sentinel = dir.path().parent().unwrap().join("azork_escaped_marker");
    let _ = std::fs::remove_file(&sentinel);

    let archive = make_targz_raw("../azork_escaped_marker", b"pwned");
    let result = extract_binary(&archive, "azork", dir.path());

    assert!(result.is_err(), "traversal entry must be rejected");
    assert!(
        !sentinel.exists(),
        "no file may be written outside the destination directory"
    );
}

#[test]
fn extract_rejects_absolute_path_entry() {
    let dir = tempdir();
    let archive = make_targz_raw("/tmp/azork_abs_marker", b"pwned");
    let result = extract_binary(&archive, "azork", dir.path());
    assert!(result.is_err(), "absolute-path entry must be rejected");
    assert!(!Path::new("/tmp/azork_abs_marker").exists());
}

#[test]
fn extract_rejects_corrupt_archive() {
    let dir = tempdir();
    let garbage = b"this is definitely not a gzip stream";
    assert!(extract_binary(garbage, "azork", dir.path()).is_err());
}

// --- tiny temp-dir helper (avoids an extra dev-dependency) ---------------

struct TempDir {
    path: std::path::PathBuf,
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
    let path = std::env::temp_dir().join(format!(
        "azork-archive-test-{nanos}-{:p}",
        &nanos as *const _
    ));
    std::fs::create_dir_all(&path).expect("create temp dir");
    TempDir { path }
}
