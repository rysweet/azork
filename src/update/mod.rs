//! Self-update mechanism for AzZork.
//!
//! Modelled on the amplihack-rs updater, adapted idiomatically for a single
//! `azork` binary. The design keeps a hard separation between:
//!
//! * **pure logic** (this module): version normalisation, semver comparison,
//!   per-target asset selection, the 24h cooldown decision, and the
//!   [`UpdateError`] → exit-code mapping. None of it touches the network,
//!   filesystem, or a TTY, so it is exhaustively unit-testable offline.
//! * **impure edges** (submodules): [`network`] is the *only* module that opens
//!   a socket; [`checksum`] verifies before install; [`archive`] extracts
//!   traversal-safely; [`install`] atomically self-replaces; [`post_install`]
//!   writes the `.installed-version` stamp and detects drift; [`check`] gates
//!   the optional startup check so it is never interactive under CI / non-TTY.
//!
//! # Trust model
//!
//! Releases are downloaded only from GitHub, and the archive's SHA-256 is
//! verified against the published `.sha256` sibling asset *before* the binary
//! is written to disk (fail-closed). Updates are strictly forward
//! (anti-rollback): only a release with a semver strictly greater than the
//! running version is ever installed.
//!
//! # Opt-out & safety
//!
//! Set `AZORK_NO_UPDATE_CHECK=1` to disable the startup check entirely. The
//! check is additionally skipped under CI, `NONINTERACTIVE`, agent execution,
//! `--subprocess-safe`, or when stdin is not a TTY — so it can never hang or
//! prompt in automation.

use semver::Version;
use serde::Deserialize;
use std::fmt;
use std::path::PathBuf;

pub mod archive;
pub mod check;
pub mod checksum;
pub mod install;
pub mod network;
pub mod post_install;

/// Hard cap on a release asset's size, shared by [`network`] (bounding the
/// downloaded bytes) and [`archive`] (bounding the decompressed bytes). A
/// single source of truth keeps the two guards in lockstep: raising one
/// without the other would otherwise turn an early, clear download-time
/// rejection into a confusing post-download extraction failure.
pub(crate) const MAX_RELEASE_ASSET_BYTES: u64 = 512 * 1024 * 1024; // 512 MiB

// Re-export the fail-closed primitives at the module root so callers (and the
// contract tests) can reach them without knowing the internal layout.
pub use archive::extract_binary;
pub use checksum::verify_sha256;

/// The GitHub repository the updater queries for releases.
pub const GITHUB_REPO: &str = "rysweet/azork";

/// Environment variable that, when set to a non-empty value, disables the
/// startup update check.
pub const NO_UPDATE_CHECK_ENV: &str = "AZORK_NO_UPDATE_CHECK";

/// The version of the currently running binary — the single source of truth.
pub const CURRENT_VERSION: &str = crate::VERSION;

/// Minimum interval between startup update checks (24 hours, in seconds).
pub const UPDATE_CHECK_COOLDOWN_SECS: u64 = 24 * 60 * 60;

// ---------------------------------------------------------------------------
// GitHub Releases API data shapes
// ---------------------------------------------------------------------------

/// A single downloadable asset attached to a GitHub release.
///
/// Only the fields the updater needs are modelled; the GitHub API returns many
/// more, which serde ignores by default.
#[derive(Debug, Clone, Deserialize)]
pub struct GithubAsset {
    /// The asset filename, e.g. `azork-x86_64-unknown-linux-gnu.tar.gz`.
    pub name: String,
    /// The direct download URL for the asset.
    pub browser_download_url: String,
}

/// A GitHub release as returned by the `releases/latest` endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct GithubRelease {
    /// The git tag, e.g. `v0.3.0`.
    pub tag_name: String,
    /// Whether the release is a draft (never installed).
    #[serde(default)]
    pub draft: bool,
    /// Whether the release is a prerelease (never installed by the stable path).
    #[serde(default)]
    pub prerelease: bool,
    /// The assets attached to the release.
    #[serde(default)]
    pub assets: Vec<GithubAsset>,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors that can occur during an update, each with a stable process exit code.
///
/// The exit-code mapping is the authoritative contract consumed by scripts and
/// CI: `2` network, `3` checksum mismatch, `4` target not writable, `5` no
/// supported asset / archive problem. Keep [`UpdateError::exit_code`] in sync
/// with `docs/UPDATING.md`.
#[derive(Debug)]
pub enum UpdateError {
    /// A network request failed (DNS, TLS, timeout, HTTP status, rate limit).
    Network(String),
    /// The downloaded archive's SHA-256 did not match the published checksum.
    ChecksumMismatch {
        /// The expected digest from the `.sha256` asset.
        expected: String,
        /// The digest actually computed over the download.
        actual: String,
    },
    /// The install destination (the current executable) could not be written.
    TargetNotWritable(String),
    /// No release asset exists for the running OS/architecture.
    NoSupportedAsset,
    /// The archive could not be read/extracted, or was unsafe (traversal).
    Archive(String),
    /// A filesystem operation failed.
    Io(String),
    /// A release tag or checksum body could not be parsed.
    Parse(String),
    /// The latest release is not newer than the running version.
    AlreadyUpToDate,
}

impl UpdateError {
    /// The process exit code associated with this error.
    pub fn exit_code(&self) -> i32 {
        match self {
            UpdateError::Network(_) => 2,
            UpdateError::ChecksumMismatch { .. } => 3,
            UpdateError::TargetNotWritable(_) => 4,
            UpdateError::NoSupportedAsset => 5,
            UpdateError::Archive(_) => 5,
            UpdateError::Io(_) => 4,
            UpdateError::Parse(_) => 2,
            UpdateError::AlreadyUpToDate => 0,
        }
    }
}

impl fmt::Display for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UpdateError::Network(m) => write!(f, "network error: {m}"),
            UpdateError::ChecksumMismatch { expected, actual } => write!(
                f,
                "checksum mismatch: expected {expected}, computed {actual}"
            ),
            UpdateError::TargetNotWritable(p) => {
                write!(f, "cannot write update to {p}: permission denied")
            }
            UpdateError::NoSupportedAsset => {
                write!(f, "no release asset is published for this platform")
            }
            UpdateError::Archive(m) => write!(f, "archive error: {m}"),
            UpdateError::Io(m) => write!(f, "io error: {m}"),
            UpdateError::Parse(m) => write!(f, "parse error: {m}"),
            UpdateError::AlreadyUpToDate => write!(f, "already up to date"),
        }
    }
}

impl std::error::Error for UpdateError {}

/// Convenient result alias for update operations.
pub type Result<T> = std::result::Result<T, UpdateError>;

// ---------------------------------------------------------------------------
// Pure logic
// ---------------------------------------------------------------------------

/// Parse a git tag into a semantic version, stripping a leading `v` and any
/// surrounding whitespace.
pub fn normalize_tag(tag: &str) -> Result<Version> {
    let trimmed = tag.trim();
    let stripped = trimmed.strip_prefix('v').unwrap_or(trimmed);
    Version::parse(stripped)
        .map_err(|e| UpdateError::Parse(format!("invalid version tag {tag:?}: {e}")))
}

/// Return `true` only when `candidate` is strictly newer than `current`.
///
/// Strict `>` gives anti-rollback: an equal or older release is never
/// installed.
pub fn is_newer(current: &Version, candidate: &Version) -> bool {
    candidate > current
}

/// The Rust target triple this binary supports self-updating for, if any.
///
/// Only platforms with published release assets return `Some`. Additional
/// triples can be added here as the release matrix grows.
pub fn supported_release_target() -> Option<&'static str> {
    if cfg!(target_os = "linux") && cfg!(target_arch = "x86_64") {
        Some("x86_64-unknown-linux-gnu")
    } else if cfg!(target_os = "linux") && cfg!(target_arch = "aarch64") {
        Some("aarch64-unknown-linux-gnu")
    } else {
        None
    }
}

/// The archive asset filename for a given target triple.
pub fn asset_name_for_target(target: &str) -> String {
    format!("azork-{target}.tar.gz")
}

/// The checksum asset filename that accompanies `archive_name`.
pub fn checksum_asset_name(archive_name: &str) -> String {
    format!("{archive_name}.sha256")
}

/// Select the release asset matching `target`, ignoring the `.sha256` sibling.
///
/// Returns `None` when the exact archive asset is absent (so a lone checksum
/// file can never be mistaken for the binary archive).
pub fn select_asset<'a>(release: &'a GithubRelease, target: &str) -> Option<&'a GithubAsset> {
    let wanted = asset_name_for_target(target);
    release.assets.iter().find(|a| a.name == wanted)
}

/// Decide whether a startup update check should run now.
///
/// * `now` — current unix time (seconds).
/// * `last` — unix time of the previous check, if one is cached.
///
/// Returns `true` when there is no cached check, or when at least
/// [`UPDATE_CHECK_COOLDOWN_SECS`] has elapsed since the last one. A last-check
/// timestamp in the future (clock skew) is treated as "recently checked" and
/// never panics.
pub fn should_check(now: u64, last: Option<u64>) -> bool {
    match last {
        None => true,
        Some(last) => match now.checked_sub(last) {
            // Clock skew: last check is in the future — do not check.
            None => false,
            Some(elapsed) => elapsed >= UPDATE_CHECK_COOLDOWN_SECS,
        },
    }
}

// ---------------------------------------------------------------------------
// Cache path resolution (XDG-aware)
// ---------------------------------------------------------------------------

/// Path to the last-update-check cache file: `~/.config/azork/last_update_check`.
///
/// Honours `XDG_CONFIG_HOME` when it is set and non-empty, otherwise falls back
/// to `$HOME/.config`.
pub fn cache_path() -> Result<PathBuf> {
    let base = config_base_dir()?;
    Ok(base.join("azork").join("last_update_check"))
}

fn config_base_dir() -> Result<PathBuf> {
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return Ok(PathBuf::from(xdg));
        }
    }
    let home = std::env::var_os("HOME")
        .ok_or_else(|| UpdateError::Io("HOME is not set; cannot resolve config dir".into()))?;
    Ok(PathBuf::from(home).join(".config"))
}

/// Read the cached last-check unix timestamp, if present and parseable.
pub fn read_last_check() -> Option<u64> {
    let path = cache_path().ok()?;
    let raw = std::fs::read_to_string(path).ok()?;
    raw.trim().parse::<u64>().ok()
}

/// Persist `now` as the last-check timestamp (best effort; errors are returned).
pub fn write_last_check(now: u64) -> Result<()> {
    let path = cache_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| UpdateError::Io(format!("create {}: {e}", parent.display())))?;
    }
    // Atomic-ish write: temp then rename so a concurrent reader never sees a
    // half-written timestamp.
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, now.to_string())
        .map_err(|e| UpdateError::Io(format!("write {}: {e}", tmp.display())))?;
    std::fs::rename(&tmp, &path)
        .map_err(|e| UpdateError::Io(format!("rename {}: {e}", path.display())))?;
    Ok(())
}

/// Current unix time in seconds (0 on the astronomically-unlikely pre-epoch clock).
pub fn now_unix() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// High-level orchestration (used by `azork update` and the startup check)
// ---------------------------------------------------------------------------

/// A release resolved as a viable update target.
#[derive(Debug, Clone)]
pub struct ResolvedUpdate {
    /// The new version.
    pub version: Version,
    /// Direct download URL for the target archive.
    pub asset_url: String,
    /// Direct download URL for the archive's `.sha256`, if published.
    pub checksum_url: Option<String>,
}

/// Query GitHub for the latest release and, if it is newer than the running
/// version and has an asset for this platform, return it.
///
/// Returns `Ok(None)` when already up to date. This performs network I/O.
pub fn resolve_latest_update() -> Result<Option<ResolvedUpdate>> {
    let target = supported_release_target().ok_or(UpdateError::NoSupportedAsset)?;
    let release = network::fetch_latest_release()?;
    if release.draft || release.prerelease {
        return Ok(None);
    }
    let latest = normalize_tag(&release.tag_name)?;
    let current = Version::parse(CURRENT_VERSION)
        .map_err(|e| UpdateError::Parse(format!("running version {CURRENT_VERSION:?}: {e}")))?;
    if !is_newer(&current, &latest) {
        return Ok(None);
    }
    let asset = select_asset(&release, target).ok_or(UpdateError::NoSupportedAsset)?;
    let checksum_name = checksum_asset_name(&asset.name);
    let checksum_url = release
        .assets
        .iter()
        .find(|a| a.name == checksum_name)
        .map(|a| a.browser_download_url.clone());
    Ok(Some(ResolvedUpdate {
        version: latest,
        asset_url: asset.browser_download_url.clone(),
        checksum_url,
    }))
}

/// Run the explicit `azork update` subcommand.
///
/// Checks for a newer release (ignoring the cooldown), downloads it, verifies
/// its checksum, and self-replaces the binary. Returns the process exit code:
/// `0` on success or already-up-to-date, non-zero (see [`UpdateError::exit_code`])
/// on failure.
pub fn run_update_command() -> i32 {
    run_update_with(false)
}

/// Run `azork update` with an optional check-only mode (`--check`), which
/// reports whether an update is available without installing anything.
pub fn run_update_with(check_only: bool) -> i32 {
    if check_only {
        return match resolve_latest_update() {
            Ok(Some(r)) => {
                println!(
                    "A newer azork release is available: {CURRENT_VERSION} -> {}.",
                    r.version
                );
                println!("Run `azork update` to install it.");
                0
            }
            Ok(None) => {
                println!("azork {CURRENT_VERSION} is already the latest version.");
                0
            }
            Err(e) => {
                eprintln!("azork update check failed: {e}");
                e.exit_code()
            }
        };
    }
    match do_update() {
        Ok(installed) => {
            if let Some(version) = installed {
                println!("Updated azork {CURRENT_VERSION} -> {version}.");
                println!("Restart azork to use the new version.");
            } else {
                println!("azork {CURRENT_VERSION} is already the latest version.");
            }
            0
        }
        Err(e) => {
            eprintln!("azork update failed: {e}");
            e.exit_code()
        }
    }
}

fn do_update() -> Result<Option<Version>> {
    println!("Checking {GITHUB_REPO} for a newer release...");
    let resolved = match resolve_latest_update()? {
        Some(r) => r,
        None => return Ok(None),
    };
    // Record that we checked, so a subsequent startup check honours the
    // cooldown. Best-effort like the sibling write in check.rs: losing this
    // write merely causes one extra startup check later, not a correctness
    // issue, so we don't fail the explicit `azork update` command over it.
    let _ = write_last_check(now_unix());
    install::download_and_replace(&resolved)?;
    Ok(Some(resolved.version))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_path_honours_xdg() {
        // Save & restore to avoid leaking state to sibling tests.
        let old = std::env::var_os("XDG_CONFIG_HOME");
        std::env::set_var("XDG_CONFIG_HOME", "/tmp/xdgtest");
        let p = cache_path().unwrap();
        assert_eq!(p, PathBuf::from("/tmp/xdgtest/azork/last_update_check"));
        match old {
            Some(v) => std::env::set_var("XDG_CONFIG_HOME", v),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
    }

    #[test]
    fn resolved_update_debug_is_stable() {
        let r = ResolvedUpdate {
            version: Version::new(0, 3, 0),
            asset_url: "https://example.invalid/a".into(),
            checksum_url: None,
        };
        assert_eq!(r.version.to_string(), "0.3.0");
        assert!(format!("{r:?}").contains("example.invalid"));
    }
}
