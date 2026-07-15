//! TDD contract tests for the startup-check gate in `azork::update::check`.
//!
//! The startup check MUST be subprocess- and CI-safe: under any skip signal it
//! must never prompt or hang. `classify_skip_reason` is the pure decision
//! function used by the (impure) startup path; keeping it pure over an explicit
//! `SkipEnv` lets us prove the non-interactive contract without touching real
//! env vars, TTYs, or the network.
//!
//! Contract:
//!   * `classify_skip_reason(&SkipEnv) -> Option<SkipReason>`
//!   * Returns `Some(reason)` when the check must be skipped, `None` only when
//!     it is safe to (potentially) prompt — i.e. fully interactive with no
//!     opt-out / CI / agent / subprocess signal and a real TTY.
//!   * Opt-out takes precedence over all other reasons.

use azork::update::check::{classify_skip_reason, SkipEnv, SkipReason, StartupUpdateOutcome};

/// A fully-interactive environment: nothing set, stdin is a TTY. This is the
/// ONLY configuration for which the check may proceed to a prompt.
fn interactive() -> SkipEnv {
    SkipEnv {
        no_update_check: false,
        ci: false,
        noninteractive: false,
        agent_binary: false,
        subprocess_safe_flag: false,
        stdin_is_tty: true,
    }
}

#[test]
fn interactive_env_is_not_skipped() {
    assert_eq!(classify_skip_reason(&interactive()), None);
}

#[test]
fn opt_out_is_skipped() {
    let env = SkipEnv {
        no_update_check: true,
        ..interactive()
    };
    assert_eq!(classify_skip_reason(&env), Some(SkipReason::OptOut));
}

#[test]
fn opt_out_takes_precedence_over_everything() {
    // Even in an otherwise interactive shell, explicit opt-out wins; and even
    // when every other signal is also set, opt-out is the reported reason.
    let env = SkipEnv {
        no_update_check: true,
        ci: true,
        noninteractive: true,
        agent_binary: true,
        subprocess_safe_flag: true,
        stdin_is_tty: false,
    };
    assert_eq!(classify_skip_reason(&env), Some(SkipReason::OptOut));
}

#[test]
fn ci_is_skipped() {
    let env = SkipEnv {
        ci: true,
        ..interactive()
    };
    assert_eq!(classify_skip_reason(&env), Some(SkipReason::Ci));
}

#[test]
fn noninteractive_is_skipped() {
    let env = SkipEnv {
        noninteractive: true,
        ..interactive()
    };
    assert_eq!(classify_skip_reason(&env), Some(SkipReason::NonInteractive));
}

#[test]
fn agent_binary_is_skipped() {
    let env = SkipEnv {
        agent_binary: true,
        ..interactive()
    };
    assert_eq!(classify_skip_reason(&env), Some(SkipReason::Agent));
}

#[test]
fn subprocess_safe_flag_is_skipped() {
    let env = SkipEnv {
        subprocess_safe_flag: true,
        ..interactive()
    };
    assert_eq!(
        classify_skip_reason(&env),
        Some(SkipReason::SubprocessSafeFlag)
    );
}

#[test]
fn non_tty_stdin_is_skipped() {
    let env = SkipEnv {
        stdin_is_tty: false,
        ..interactive()
    };
    assert_eq!(classify_skip_reason(&env), Some(SkipReason::NoTty));
}

#[test]
fn any_skip_signal_yields_some() {
    // Property: if ANY skip signal is present, the result is never None.
    for env in [
        SkipEnv {
            ci: true,
            ..interactive()
        },
        SkipEnv {
            noninteractive: true,
            ..interactive()
        },
        SkipEnv {
            agent_binary: true,
            ..interactive()
        },
        SkipEnv {
            subprocess_safe_flag: true,
            ..interactive()
        },
        SkipEnv {
            stdin_is_tty: false,
            ..interactive()
        },
    ] {
        assert!(classify_skip_reason(&env).is_some());
    }
}

#[test]
fn startup_outcome_variants_are_distinct() {
    assert_ne!(
        StartupUpdateOutcome::Continue,
        StartupUpdateOutcome::ExitSuccess
    );
    // Continue is the common, safe default.
    assert_eq!(StartupUpdateOutcome::Continue, StartupUpdateOutcome::Continue);
}
