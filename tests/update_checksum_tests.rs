//! TDD contract tests for `azork::update::verify_sha256`.
//!
//! `verify_sha256(bytes, expected_hex) -> bool` is the fail-closed checksum
//! gate that runs BEFORE any binary is installed. It must:
//!   * return `true` only when the SHA-256 of `bytes` equals `expected_hex`,
//!   * accept upper/lower-case hex (case-insensitive),
//!   * reject malformed digests (wrong length, non-hex) by returning `false`,
//!   * never panic on any input.
//!
//! Offline only — no network.

use azork::update::verify_sha256;
use sha2::{Digest, Sha256};

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}

#[test]
fn verify_accepts_correct_digest() {
    let data = b"the quick brown azork";
    let digest = sha256_hex(data);
    assert!(verify_sha256(data, &digest));
}

#[test]
fn verify_is_case_insensitive() {
    let data = b"azork release payload";
    let digest = sha256_hex(data).to_uppercase();
    assert!(verify_sha256(data, &digest));
}

#[test]
fn verify_tolerates_surrounding_whitespace() {
    // Checksum files often look like "<hex>  <filename>\n"; a bare hex token
    // with trailing whitespace must still validate.
    let data = b"payload";
    let digest = format!("  {}\n", sha256_hex(data));
    assert!(verify_sha256(data, &digest));
}

#[test]
fn verify_rejects_wrong_digest() {
    let data = b"payload";
    // Valid-length hex but not the real digest.
    let wrong = "0".repeat(64);
    assert!(!verify_sha256(data, &wrong));
}

#[test]
fn verify_rejects_tampered_payload() {
    let original = b"trusted payload";
    let digest = sha256_hex(original);
    let tampered = b"trusted payload!"; // one byte different
    assert!(!verify_sha256(tampered, &digest));
}

#[test]
fn verify_rejects_short_hex() {
    let data = b"payload";
    assert!(!verify_sha256(data, "abc123"));
}

#[test]
fn verify_rejects_non_hex() {
    let data = b"payload";
    let non_hex = "z".repeat(64);
    assert!(!verify_sha256(data, &non_hex));
}

#[test]
fn verify_rejects_empty_expected() {
    let data = b"payload";
    assert!(!verify_sha256(data, ""));
}
