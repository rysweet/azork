//! Fail-closed SHA-256 verification of downloaded release archives.
//!
//! [`verify_sha256`] is the gate that runs **before** any binary is written to
//! disk. It computes the SHA-256 of the downloaded bytes and compares it,
//! case-insensitively, against the expected digest published in the release's
//! `.sha256` asset. Any malformed digest (wrong length, non-hex, empty) causes
//! it to return `false` — it never panics and never trusts on ambiguity.

use super::{Result, UpdateError};
use sha2::{Digest, Sha256};

/// Compute the lowercase hex SHA-256 of `bytes`.
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// Parse and validate a SHA-256 digest from a checksum-file body.
///
/// Checksum files are commonly `"<hex>  <filename>\n"`, so this takes the first
/// whitespace-delimited token and accepts it only when it is exactly 64 hex
/// characters. Returns the canonical lowercase digest, or `None` for anything
/// malformed (empty, wrong length, non-hex). This is the single source of truth
/// for what counts as a valid digest.
fn parse_expected_digest(expected: &str) -> Option<String> {
    let token = expected.split_whitespace().next()?.to_ascii_lowercase();
    if token.len() == 64 && token.bytes().all(|b| b.is_ascii_hexdigit()) {
        Some(token)
    } else {
        None
    }
}

/// Return `true` iff the SHA-256 of `bytes` equals `expected`.
///
/// Accepts upper/lower-case hex and tolerates a trailing filename or
/// surrounding whitespace (as found in `sha256sum` output). Returns `false`
/// for any malformed digest; never panics.
pub fn verify_sha256(bytes: &[u8], expected: &str) -> bool {
    match parse_expected_digest(expected) {
        // Both digests are public, so a plain equality on lowercase hex is fine.
        Some(token) => sha256_hex(bytes) == token,
        None => false,
    }
}

/// Verify `bytes` against `expected`, returning a structured error on mismatch.
///
/// Used by the install path so a checksum failure surfaces as
/// [`UpdateError::ChecksumMismatch`] with exit code 3.
pub(crate) fn verify_or_error(bytes: &[u8], expected: &str) -> Result<()> {
    let token = parse_expected_digest(expected).ok_or_else(|| {
        UpdateError::Parse("checksum file is empty or not a 64-character hex digest".into())
    })?;
    let actual = sha256_hex(bytes);
    if actual == token {
        Ok(())
    } else {
        Err(UpdateError::ChecksumMismatch {
            expected: token,
            actual,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_or_error_ok_on_match() {
        let data = b"hello azork";
        let digest = sha256_hex(data);
        assert!(verify_or_error(data, &digest).is_ok());
    }

    #[test]
    fn verify_or_error_mismatch_maps_to_exit_3() {
        let data = b"hello azork";
        let wrong = "0".repeat(64);
        let err = verify_or_error(data, &wrong).unwrap_err();
        assert_eq!(err.exit_code(), 3);
    }

    #[test]
    fn verify_or_error_rejects_malformed() {
        assert!(verify_or_error(b"x", "short").is_err());
        assert!(verify_or_error(b"x", "").is_err());
    }
}
