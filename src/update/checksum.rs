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

/// Extract the bare hex digest from a checksum-file body.
///
/// Checksum files are commonly `"<hex>  <filename>\n"`; this returns the first
/// whitespace-delimited token, lowercased.
fn extract_digest_token(expected: &str) -> Option<String> {
    let token = expected.split_whitespace().next()?;
    if token.is_empty() {
        return None;
    }
    Some(token.to_ascii_lowercase())
}

/// Return `true` iff the SHA-256 of `bytes` equals `expected`.
///
/// Accepts upper/lower-case hex and tolerates a trailing filename or
/// surrounding whitespace (as found in `sha256sum` output). Returns `false`
/// for any malformed digest; never panics.
pub fn verify_sha256(bytes: &[u8], expected: &str) -> bool {
    let token = match extract_digest_token(expected) {
        Some(t) => t,
        None => return false,
    };
    // A SHA-256 digest is exactly 64 hex characters.
    if token.len() != 64 || !token.chars().all(|c| c.is_ascii_hexdigit()) {
        return false;
    }
    let actual = sha256_hex(bytes);
    // Constant-time comparison is unnecessary here (both digests are public),
    // but a straightforward equality on equal-length lowercase hex is fine.
    actual == token
}

/// Verify `bytes` against `expected`, returning a structured error on mismatch.
///
/// Used by the install path so a checksum failure surfaces as
/// [`UpdateError::ChecksumMismatch`] with exit code 3.
pub(crate) fn verify_or_error(bytes: &[u8], expected: &str) -> Result<()> {
    let token = extract_digest_token(expected)
        .ok_or_else(|| UpdateError::Parse("empty or malformed checksum file".into()))?;
    if token.len() != 64 || !token.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(UpdateError::Parse(format!(
            "checksum is not a 64-character hex digest: {token:?}"
        )));
    }
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
