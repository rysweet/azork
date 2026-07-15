//! Startup update-check gate.
//!
//! The optional startup check must be **subprocess- and CI-safe**: under any
//! automation signal it must never prompt or hang. [`classify_skip_reason`] is
//! the pure decision function over an explicit [`SkipEnv`], which lets the
//! non-interactive contract be proven without touching real env vars, TTYs, or
//! the network. The impure [`maybe_startup_check`] wires the real environment
//! into that decision and performs the (cheap, cached, timeout-bounded) check
//! only when it is genuinely safe to prompt.

use super::{
    is_newer, now_unix, read_last_check, resolve_latest_update, should_check, write_last_check,
    CURRENT_VERSION, NO_UPDATE_CHECK_ENV,
};
use semver::Version;
use std::io::{self, IsTerminal, Write};
use std::sync::mpsc;
use std::time::Duration;

/// Outcome of the startup update check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupUpdateOutcome {
    /// Continue with normal execution (the common, safe default).
    Continue,
    /// A self-update completed successfully; the caller should exit.
    ExitSuccess,
}

/// The explicit environment inputs that decide whether the startup check runs.
///
/// Modelling these as data (rather than reading the process environment inline)
/// keeps [`classify_skip_reason`] pure and exhaustively testable.
#[derive(Debug, Clone, Copy)]
pub struct SkipEnv {
    /// `AZORK_NO_UPDATE_CHECK` is set — explicit opt-out.
    pub no_update_check: bool,
    /// `CI` is set — running in continuous integration.
    pub ci: bool,
    /// `NONINTERACTIVE` is set.
    pub noninteractive: bool,
    /// An agent/automation harness set `AGENT_BINARY` (or similar).
    pub agent_binary: bool,
    /// `--subprocess-safe` was passed on the command line.
    pub subprocess_safe_flag: bool,
    /// stdin is connected to a real terminal.
    pub stdin_is_tty: bool,
}

/// Why the startup update check was skipped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    /// Explicit opt-out via `AZORK_NO_UPDATE_CHECK`. Silent; takes precedence.
    OptOut,
    /// Running under CI.
    Ci,
    /// `NONINTERACTIVE` was set.
    NonInteractive,
    /// Running under an agent/automation harness.
    Agent,
    /// `--subprocess-safe` flag was passed.
    SubprocessSafeFlag,
    /// stdin is not a TTY, so we could never safely prompt.
    NoTty,
}

/// The literal skip line emitted so automation can grep for evidence the check
/// ran and was bypassed (rather than silently hanging). Rewording it is a
/// breaking change to that contract.
pub const SUBPROCESS_SAFE_SKIP_LINE: &str =
    "azork: skipping update check (subprocess-safe / no TTY)";

/// Seconds to wait for the user's answer at the update prompt before giving up.
const PROMPT_TIMEOUT_SECS: u64 = 5;

/// Decide whether the startup check must be skipped, and why.
///
/// Returns `Some(reason)` when the check must not prompt, or `None` only for a
/// fully-interactive environment with no opt-out/CI/agent/subprocess signal and
/// a real TTY. Explicit opt-out takes precedence over every other reason.
pub fn classify_skip_reason(env: &SkipEnv) -> Option<SkipReason> {
    if env.no_update_check {
        return Some(SkipReason::OptOut);
    }
    if env.ci {
        return Some(SkipReason::Ci);
    }
    if env.noninteractive {
        return Some(SkipReason::NonInteractive);
    }
    if env.agent_binary {
        return Some(SkipReason::Agent);
    }
    if env.subprocess_safe_flag {
        return Some(SkipReason::SubprocessSafeFlag);
    }
    if !env.stdin_is_tty {
        return Some(SkipReason::NoTty);
    }
    None
}

/// `true` for a non-opt-out skip reason whose bypass should be announced on
/// stderr (so automation sees the check did not silently hang).
fn is_visible_bypass(reason: SkipReason) -> bool {
    !matches!(reason, SkipReason::OptOut)
}

fn env_flag(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|v| !v.is_empty())
}

/// Read the real process environment into a [`SkipEnv`].
fn current_skip_env(args: &[String]) -> SkipEnv {
    SkipEnv {
        no_update_check: env_flag(NO_UPDATE_CHECK_ENV),
        ci: env_flag("CI"),
        noninteractive: env_flag("NONINTERACTIVE"),
        agent_binary: env_flag("AGENT_BINARY"),
        subprocess_safe_flag: args.iter().any(|a| a == "--subprocess-safe"),
        stdin_is_tty: io::stdin().is_terminal(),
    }
}

/// Run the optional startup update check.
///
/// Returns [`StartupUpdateOutcome::Continue`] in every case except a successful
/// self-update (which returns [`StartupUpdateOutcome::ExitSuccess`] so the
/// caller can exit and let the user restart). This never hangs: it is gated by
/// [`classify_skip_reason`], honours the 24h cooldown, and bounds the prompt
/// with a [`PROMPT_TIMEOUT_SECS`] timeout.
pub fn maybe_startup_check(args: &[String]) -> StartupUpdateOutcome {
    // No published asset for this platform → nothing to do.
    if super::supported_release_target().is_none() {
        return StartupUpdateOutcome::Continue;
    }

    let env = current_skip_env(args);
    if let Some(reason) = classify_skip_reason(&env) {
        if is_visible_bypass(reason) {
            eprintln!("{SUBPROCESS_SAFE_SKIP_LINE}");
        }
        return StartupUpdateOutcome::Continue;
    }

    // Interactive path: honour the 24h cooldown before any network call.
    let now = now_unix();
    if !should_check(now, read_last_check()) {
        return StartupUpdateOutcome::Continue;
    }
    // Record the attempt up front so a network failure doesn't cause a re-check
    // storm on every launch.
    let _ = write_last_check(now);

    let resolved = match resolve_latest_update() {
        Ok(Some(r)) => r,
        Ok(None) => return StartupUpdateOutcome::Continue,
        Err(_) => {
            // Startup checks are best-effort — never fail the game over an
            // update-check error. `azork update` surfaces errors explicitly.
            return StartupUpdateOutcome::Continue;
        }
    };

    let current = match Version::parse(CURRENT_VERSION) {
        Ok(v) => v,
        Err(_) => return StartupUpdateOutcome::Continue,
    };
    if !is_newer(&current, &resolved.version) {
        return StartupUpdateOutcome::Continue;
    }

    println!(
        "A new azork release is available: {CURRENT_VERSION} -> {}.",
        resolved.version
    );
    if !prompt_yes_no_timeout("Install it now?", PROMPT_TIMEOUT_SECS) {
        println!("Skipping. Run `azork update` any time, or set {NO_UPDATE_CHECK_ENV}=1 to silence this.");
        return StartupUpdateOutcome::Continue;
    }

    match super::install::download_and_replace(&resolved) {
        Ok(_) => {
            println!("Updated to {}. Restart azork to use it.", resolved.version);
            StartupUpdateOutcome::ExitSuccess
        }
        Err(e) => {
            eprintln!("Update failed: {e}. Continuing with the current version.");
            StartupUpdateOutcome::Continue
        }
    }
}

/// Prompt for a yes/no answer, returning `false` if the user does not answer
/// affirmatively within `timeout_secs`. Defaults to "no".
fn prompt_yes_no_timeout(question: &str, timeout_secs: u64) -> bool {
    print!("{question} [y/N] ");
    io::stdout().flush().ok();

    let (tx, rx) = mpsc::channel();
    // The reader thread is detached; if it outlives the timeout it simply sends
    // to a dropped receiver and exits. We never join it, so we never block.
    std::thread::spawn(move || {
        let mut line = String::new();
        if io::stdin().read_line(&mut line).is_ok() {
            let _ = tx.send(line);
        }
    });

    match rx.recv_timeout(Duration::from_secs(timeout_secs)) {
        Ok(line) => {
            let a = line.trim().to_lowercase();
            a == "y" || a == "yes"
        }
        Err(_) => {
            println!("\n(no response within {timeout_secs}s — skipping update)");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_bypass_is_silent_only_for_opt_out() {
        assert!(!is_visible_bypass(SkipReason::OptOut));
        assert!(is_visible_bypass(SkipReason::Ci));
        assert!(is_visible_bypass(SkipReason::NoTty));
    }

    #[test]
    fn subprocess_flag_detected_from_args() {
        let args = vec!["azork".to_string(), "--subprocess-safe".to_string()];
        let env = current_skip_env(&args);
        assert!(env.subprocess_safe_flag);
    }
}
