//! The only module that performs network I/O.
//!
//! Fetches the latest GitHub release metadata and downloads release assets. All
//! requests are bounded by a short timeout and restricted to GitHub hosts, and
//! downloads are capped to guard against decompression/oversize bombs. Keeping
//! every socket in this one module means the rest of the updater — and all of
//! its tests — remain provably offline.

use super::{GithubRelease, Result, UpdateError, GITHUB_REPO};
use std::io::Read;
use std::time::Duration;

/// Per-request timeout. The startup check must stay cheap, so this is short.
const NETWORK_TIMEOUT_SECS: u64 = 8;

/// Hard cap on any single download (guards against oversize/bomb responses).
const MAX_DOWNLOAD_BYTES: usize = 512 * 1024 * 1024; // 512 MiB

/// GitHub host prefixes the updater is permitted to talk to.
const ALLOWED_HOSTS: &[&str] = &[
    "https://api.github.com/",
    "https://github.com/",
    "https://objects.githubusercontent.com/",
];

/// Reject any URL that is not served from a trusted GitHub host.
pub(crate) fn validate_download_url(url: &str) -> Result<()> {
    if ALLOWED_HOSTS.iter().any(|p| url.starts_with(p)) {
        Ok(())
    } else {
        Err(UpdateError::Network(format!(
            "refusing to download from a non-GitHub host: {url}"
        )))
    }
}

fn agent() -> ureq::Agent {
    let t = Duration::from_secs(NETWORK_TIMEOUT_SECS);
    ureq::AgentBuilder::new()
        .timeout_connect(t)
        .timeout_read(t)
        .timeout_write(t)
        .build()
}

/// Perform a GET and return the response body, enforcing the host allowlist and
/// size cap.
pub(crate) fn http_get(url: &str) -> Result<Vec<u8>> {
    validate_download_url(url)?;

    let user_agent = format!("azork/{}", crate::VERSION);
    let response = agent()
        .get(url)
        .set("Accept", "application/vnd.github+json")
        .set("User-Agent", &user_agent)
        .call()
        .map_err(|e| UpdateError::Network(describe_ureq_error(url, &e)))?;

    let mut body = Vec::new();
    response
        .into_reader()
        .take(MAX_DOWNLOAD_BYTES as u64)
        .read_to_end(&mut body)
        .map_err(|e| UpdateError::Network(format!("failed reading response from {url}: {e}")))?;

    if body.len() >= MAX_DOWNLOAD_BYTES {
        return Err(UpdateError::Network(format!(
            "response from {url} exceeded the {MAX_DOWNLOAD_BYTES} byte cap"
        )));
    }
    Ok(body)
}

fn describe_ureq_error(url: &str, err: &ureq::Error) -> String {
    match err {
        ureq::Error::Status(404, _) if url.ends_with("/releases/latest") => {
            format!("no published release found for {GITHUB_REPO} yet")
        }
        ureq::Error::Status(403, _) => {
            format!("GitHub returned 403 for {url} (likely API rate limit)")
        }
        ureq::Error::Status(code, _) => format!("GitHub returned HTTP {code} for {url}"),
        ureq::Error::Transport(t) => format!("network transport error for {url}: {t}"),
    }
}

/// Fetch and parse the latest stable release for [`GITHUB_REPO`].
///
/// A test hook (`AZORK_TEST_FAKE_RELEASE_JSON`) allows injecting a release body
/// without a network call; an empty value falls through to the real path so an
/// exported-but-empty variable never silently disables real checks.
pub fn fetch_latest_release() -> Result<GithubRelease> {
    if let Some(raw) = std::env::var_os("AZORK_TEST_FAKE_RELEASE_JSON") {
        if !raw.is_empty() {
            let body = raw.to_string_lossy();
            return serde_json::from_str(&body)
                .map_err(|e| UpdateError::Parse(format!("fake release JSON invalid: {e}")));
        }
    }
    let url = format!("https://api.github.com/repos/{GITHUB_REPO}/releases/latest");
    let body = http_get(&url)?;
    serde_json::from_slice(&body)
        .map_err(|e| UpdateError::Parse(format!("failed to parse release JSON from {url}: {e}")))
}

/// Download a release asset (archive or checksum) from a trusted GitHub host.
pub fn download_asset(url: &str) -> Result<Vec<u8>> {
    http_get(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowlist_accepts_github_hosts() {
        assert!(validate_download_url("https://github.com/rysweet/azork/x").is_ok());
        assert!(validate_download_url("https://api.github.com/repos/x").is_ok());
        assert!(validate_download_url("https://objects.githubusercontent.com/y").is_ok());
    }

    #[test]
    fn allowlist_rejects_other_hosts() {
        assert!(validate_download_url("https://evil.example.com/x").is_err());
        assert!(validate_download_url("http://github.com/x").is_err()); // no TLS
        assert!(validate_download_url("ftp://github.com/x").is_err());
    }

    #[test]
    fn fake_release_hook_parses_without_network() {
        std::env::set_var(
            "AZORK_TEST_FAKE_RELEASE_JSON",
            r#"{"tag_name":"v9.9.9","draft":false,"prerelease":false,"assets":[]}"#,
        );
        let rel = fetch_latest_release().unwrap();
        assert_eq!(rel.tag_name, "v9.9.9");
        std::env::remove_var("AZORK_TEST_FAKE_RELEASE_JSON");
    }
}
