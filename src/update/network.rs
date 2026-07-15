//! The only module that performs network I/O.
//!
//! Fetches the latest GitHub release metadata and downloads release assets. All
//! requests are bounded by a short timeout and restricted to GitHub hosts, and
//! downloads are capped to guard against decompression/oversize bombs. Keeping
//! every socket in this one module means the rest of the updater — and all of
//! its tests — remain provably offline.

use super::{GithubRelease, Result, UpdateError, GITHUB_REPO};
use std::io::Read;
use std::sync::OnceLock;
use std::time::Duration;

/// Per-request timeout. The startup check must stay cheap, so this is short.
const NETWORK_TIMEOUT_SECS: u64 = 8;

/// Hard cap on any single download (guards against oversize/bomb responses).
/// Shared with [`super::archive`]'s extraction cap via
/// [`super::MAX_RELEASE_ASSET_BYTES`] so both guards move together.
const MAX_DOWNLOAD_BYTES: usize = super::MAX_RELEASE_ASSET_BYTES as usize;

/// GitHub host prefixes a *final* (post-redirect) response may legitimately be
/// served from. GitHub's `releases/download/...` URLs 302 to the opaque signed
/// blob host `objects.githubusercontent.com`, so it is trusted as a redirect
/// target even though it carries no repository path.
const ALLOWED_FINAL_HOSTS: &[&str] = &[
    "https://api.github.com/",
    "https://github.com/",
    "https://objects.githubusercontent.com/",
];

/// The exact URL prefixes the updater is permitted to *request*.
///
/// These are pinned to [`GITHUB_REPO`] — not merely to a GitHub host — so that
/// even if attacker-controlled release JSON were ever parsed, its asset URLs
/// could only point back at `rysweet/azork`'s own releases. Host-only checks
/// (`github.com/...`) would let a fake release reference *any* public GitHub
/// release; pinning the repository path closes that hole.
fn allowed_request_prefixes() -> [String; 2] {
    [
        format!("https://api.github.com/repos/{GITHUB_REPO}/"),
        format!("https://github.com/{GITHUB_REPO}/"),
    ]
}

/// Reject any URL that is not an HTTPS request to this repository's own GitHub
/// release endpoints. Enforces both TLS (via the `https://` prefix) and the
/// `rysweet/azork` repository path.
pub(crate) fn validate_download_url(url: &str) -> Result<()> {
    if allowed_request_prefixes()
        .iter()
        .any(|p| url.starts_with(p))
    {
        Ok(())
    } else {
        Err(UpdateError::Network(format!(
            "refusing to download from an untrusted URL (not a {GITHUB_REPO} release asset): {url}"
        )))
    }
}

/// Reject a *final* (possibly redirected) response URL served from a host
/// outside [`ALLOWED_FINAL_HOSTS`]. This backstops the request-time pin: if a
/// trusted request is redirected off a GitHub host, the download is refused
/// before its body is read.
fn validate_final_url(url: &str) -> Result<()> {
    if ALLOWED_FINAL_HOSTS.iter().any(|p| url.starts_with(p)) {
        Ok(())
    } else {
        Err(UpdateError::Network(format!(
            "refusing a response redirected to a non-GitHub host: {url}"
        )))
    }
}

/// Return a shared [`ureq::Agent`], building it (and its TLS config + root-cert
/// store) exactly once. Cloning an agent is cheap (it is `Arc`-backed) and lets
/// sequential calls — e.g. the archive and its `.sha256` — reuse a connection
/// instead of paying TLS setup on every request.
fn agent() -> ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT
        .get_or_init(|| {
            let t = Duration::from_secs(NETWORK_TIMEOUT_SECS);
            ureq::AgentBuilder::new()
                .timeout_connect(t)
                .timeout_read(t)
                .timeout_write(t)
                .build()
        })
        .clone()
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

    // ureq transparently follows redirects; make sure we did not get bounced to
    // an untrusted host before we read (and later trust-by-checksum) the body.
    validate_final_url(response.get_url())?;

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
/// # Test hook
///
/// In **non-release** builds only (`cfg(any(test, debug_assertions))`), the
/// `AZORK_TEST_FAKE_RELEASE_JSON` environment variable may inject a release body
/// so the resolve/update paths can be exercised without a network call. An empty
/// value falls through to the real path. This hook is **compiled out of release
/// binaries** so a shipped `azork` can never be induced to trust attacker-
/// supplied release JSON (which, combined with a matching checksum, would
/// otherwise let an env var redirect the self-update to arbitrary content).
pub fn fetch_latest_release() -> Result<GithubRelease> {
    #[cfg(any(test, debug_assertions))]
    {
        if let Some(raw) = std::env::var_os("AZORK_TEST_FAKE_RELEASE_JSON") {
            if !raw.is_empty() {
                let body = raw.to_string_lossy();
                return serde_json::from_str(&body)
                    .map_err(|e| UpdateError::Parse(format!("fake release JSON invalid: {e}")));
            }
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
    fn request_allowlist_accepts_only_repo_pinned_urls() {
        // Real release + API asset URLs for rysweet/azork are accepted.
        assert!(validate_download_url(
            "https://github.com/rysweet/azork/releases/download/v0.3.0/azork-x86_64.tar.gz"
        )
        .is_ok());
        assert!(validate_download_url(
            "https://api.github.com/repos/rysweet/azork/releases/latest"
        )
        .is_ok());
    }

    #[test]
    fn request_allowlist_rejects_other_repos_hosts_and_plain_http() {
        // Right host, wrong repository — must be refused (fake-release defense).
        assert!(
            validate_download_url("https://github.com/attacker/repo/releases/download/x").is_err()
        );
        assert!(validate_download_url(
            "https://api.github.com/repos/attacker/repo/releases/latest"
        )
        .is_err());
        // A bare CDN blob is not a valid *request* target (only a redirect one).
        assert!(validate_download_url("https://objects.githubusercontent.com/y").is_err());
        // Non-GitHub host and non-TLS are refused.
        assert!(validate_download_url("https://evil.example.com/rysweet/azork/").is_err());
        assert!(validate_download_url("http://github.com/rysweet/azork/x").is_err()); // no TLS
        assert!(validate_download_url("ftp://github.com/rysweet/azork/x").is_err());
    }

    #[test]
    fn final_url_allowlist_accepts_cdn_and_github_hosts() {
        assert!(validate_final_url("https://objects.githubusercontent.com/blob").is_ok());
        assert!(validate_final_url("https://github.com/rysweet/azork/releases/download/x").is_ok());
        assert!(validate_final_url("https://evil.example.com/x").is_err());
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
