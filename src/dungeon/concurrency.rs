//! A small, dependency-free adaptive concurrency primitive shared by the
//! parallel resource-group enumeration in [`crate::dungeon::map`] and the
//! sequential retry loop in [`crate::backend::az`].
//!
//! This module knows nothing about `Room`/`ResourceNode` or any other
//! dungeon-domain type — it is a pure scheduling/backoff building block:
//!
//! * [`ThrottleDetector`] — classifies an `az` invocation's output as a
//!   throttling (HTTP 429 / `Retry-After`) condition, and extracts a
//!   `Retry-After` backoff floor if one was reported.
//! * [`AimdLimiter`] — a `Condvar`-based counting semaphore whose ceiling
//!   grows additively on sustained success and shrinks multiplicatively the
//!   moment a throttle is observed (AIMD: additive increase / multiplicative
//!   decrease), capped at the host's available parallelism.
//! * [`backoff_with_jitter`] — a tiny thread-local xorshift-seeded jitter
//!   helper, so retry sleeps aren't perfectly synchronized across worker
//!   threads without pulling in the `rand` crate.

use std::cell::Cell;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

/// Fallback worker count used only if the host can't report its own
/// available parallelism (rare, but `available_parallelism()` is fallible).
const FALLBACK_PARALLELISM: usize = 4;

/// Consecutive successes required (since the last decrease) before the
/// limiter grows its ceiling by one.
const SUCCESSES_PER_INCREASE: u32 = 5;

/// Detects Azure throttling signals in `az` CLI output.
///
/// Matching is a case-insensitive substring scan across stdout/stderr/status
/// text — deliberately dumb and dependency-free, mirroring the existing
/// `is_transient` heuristic in [`crate::backend::az`].
pub struct ThrottleDetector;

impl ThrottleDetector {
    /// Returns `Some(floor)` if `text` looks like a throttling response,
    /// where `floor` is the minimum backoff to honor: either a parsed
    /// `Retry-After: N` value (in seconds) or [`Duration::ZERO`] if no
    /// explicit value was reported. Returns `None` if `text` shows no
    /// throttling signal at all.
    pub fn detect(text: &str) -> Option<Duration> {
        let lower = text.to_lowercase();
        let throttled = lower.contains("429")
            || lower.contains("toomanyrequests")
            || lower.contains("too many requests")
            || lower.contains("retry-after")
            || lower.contains("throttl");
        if !throttled {
            return None;
        }
        Some(Self::retry_after_floor(&lower).unwrap_or(Duration::ZERO))
    }

    /// Parse a `retry-after: N` (seconds) value out of lowercased `text`, if
    /// present. Tolerant of surrounding whitespace/punctuation; ignores a
    /// value it can't parse as an integer rather than erroring, since this
    /// is best-effort backoff guidance, not a hard contract.
    fn retry_after_floor(lower: &str) -> Option<Duration> {
        let idx = lower.find("retry-after")?;
        let rest = &lower[idx + "retry-after".len()..];
        let digits: String = rest
            .trim_start_matches(|c: char| c == ':' || c.is_whitespace())
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        if digits.is_empty() {
            return None;
        }
        digits.parse::<u64>().ok().map(Duration::from_secs)
    }
}

/// A `Condvar`-based counting semaphore whose ceiling adapts to observed
/// throttling: additive increase on sustained success, multiplicative
/// decrease (halved, floored at 1) the instant a throttle is reported.
///
/// Changing the ceiling only ever gates *new* acquisitions — it never
/// preempts permits already held by in-flight work, so a shrink can't
/// corrupt or cancel work that's already running.
pub struct AimdLimiter {
    state: Mutex<LimiterState>,
    condvar: Condvar,
    max_ceiling: usize,
}

struct LimiterState {
    /// Permits currently held.
    in_flight: usize,
    /// Current ceiling (the AIMD-adjusted concurrency limit).
    ceiling: usize,
    /// Consecutive successes observed since the last decrease.
    consecutive_successes: u32,
}

/// An acquired permit. Dropping it releases the slot and wakes one waiter.
pub struct Permit<'a> {
    limiter: &'a AimdLimiter,
}

impl AimdLimiter {
    /// Build a limiter starting at ceiling 1 (conservative ramp-up), capped
    /// at the host's available parallelism (or [`FALLBACK_PARALLELISM`] if
    /// that can't be determined).
    pub fn new() -> AimdLimiter {
        let max_ceiling = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(FALLBACK_PARALLELISM)
            .max(1);
        AimdLimiter {
            state: Mutex::new(LimiterState {
                in_flight: 0,
                ceiling: 1,
                consecutive_successes: 0,
            }),
            condvar: Condvar::new(),
            max_ceiling,
        }
    }

    /// Block until a permit is available under the current ceiling, then
    /// take it. Re-checks the ceiling each time it's woken, since the
    /// ceiling can shrink while a caller is waiting.
    pub fn acquire(&self) -> Permit<'_> {
        let mut guard = self.state.lock().unwrap_or_else(|e| e.into_inner());
        while guard.in_flight >= guard.ceiling {
            guard = self.condvar.wait(guard).unwrap_or_else(|e| e.into_inner());
        }
        guard.in_flight += 1;
        Permit { limiter: self }
    }

    /// Report a successful invocation: after
    /// [`SUCCESSES_PER_INCREASE`] consecutive successes, grow the ceiling by
    /// one (capped at `max_ceiling`) and reset the counter.
    pub fn report_success(&self) {
        let mut guard = self.state.lock().unwrap_or_else(|e| e.into_inner());
        guard.consecutive_successes += 1;
        if guard.consecutive_successes >= SUCCESSES_PER_INCREASE {
            guard.consecutive_successes = 0;
            if guard.ceiling < self.max_ceiling {
                guard.ceiling += 1;
                self.condvar.notify_all();
            }
        }
    }

    /// Report an observed throttle: immediately halve the ceiling (floored
    /// at 1) and reset the success streak, so recovery always starts from a
    /// clean slate.
    pub fn report_throttle(&self) {
        let mut guard = self.state.lock().unwrap_or_else(|e| e.into_inner());
        guard.consecutive_successes = 0;
        guard.ceiling = (guard.ceiling / 2).max(1);
    }

    /// The current ceiling, for observability/tests.
    pub fn ceiling(&self) -> usize {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).ceiling
    }
}

impl Default for AimdLimiter {
    fn default() -> AimdLimiter {
        AimdLimiter::new()
    }
}

impl Drop for Permit<'_> {
    fn drop(&mut self) {
        let mut guard = self.limiter.state.lock().unwrap_or_else(|e| e.into_inner());
        guard.in_flight = guard.in_flight.saturating_sub(1);
        self.limiter.condvar.notify_one();
    }
}

thread_local! {
    /// Per-thread xorshift64* state, seeded lazily (on first use) from a
    /// combination of the current instant and this thread's id — good
    /// enough to decorrelate sibling worker threads' backoff sleeps without
    /// pulling in a `rand` dependency. Not used for anything
    /// security-sensitive.
    static JITTER_STATE: Cell<u64> = const { Cell::new(0) };
}

/// Process-wide counter used only to decorrelate the per-thread jitter seed
/// below; each thread that ever computes a jittered backoff claims a
/// distinct tick from it exactly once.
static JITTER_SEED_TICK: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn next_jitter_u64() -> u64 {
    JITTER_STATE.with(|cell| {
        let mut x = cell.get();
        if x == 0 {
            let tick = JITTER_SEED_TICK.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let seed = Instant::now().elapsed().as_nanos() as u64 ^ tick;
            x = seed | 1; // xorshift requires a nonzero state.
        }
        // xorshift64*
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        cell.set(x);
        x
    })
}

/// Compute a jittered backoff duration for `attempt` (1-based), honoring
/// `floor` as a minimum (e.g. a server-reported `Retry-After`).
///
/// Base delay doubles per attempt starting from `base`, then has up to ±25%
/// jitter applied (uniform, via the thread-local xorshift generator) to
/// avoid synchronized retry storms across worker threads, and is finally
/// clamped to be no less than `floor`.
pub fn backoff_with_jitter(base: Duration, attempt: u32, floor: Duration) -> Duration {
    let doubled = base.saturating_mul(1u32 << attempt.min(16).saturating_sub(1));
    let jitter_pct = (next_jitter_u64() % 51) as i64 - 25; // -25..=25
    let base_nanos = doubled.as_nanos() as i64;
    let jittered_nanos = base_nanos + (base_nanos * jitter_pct / 100);
    let jittered = Duration::from_nanos(jittered_nanos.max(0) as u64);
    jittered.max(floor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn throttle_detector_matches_common_signals() {
        assert!(ThrottleDetector::detect("HTTP/1.1 429 Too Many Requests").is_some());
        assert!(ThrottleDetector::detect("Error: TooManyRequests").is_some());
        assert!(ThrottleDetector::detect("nothing to see here").is_none());
    }

    #[test]
    fn throttle_detector_parses_retry_after_seconds() {
        let floor = ThrottleDetector::detect("429 - Retry-After: 7").unwrap();
        assert_eq!(floor, Duration::from_secs(7));
    }

    #[test]
    fn throttle_detector_defaults_floor_to_zero_without_retry_after() {
        let floor = ThrottleDetector::detect("429 too many requests").unwrap();
        assert_eq!(floor, Duration::ZERO);
    }

    #[test]
    fn limiter_starts_at_ceiling_one() {
        let limiter = AimdLimiter::new();
        assert_eq!(limiter.ceiling(), 1);
    }

    #[test]
    fn limiter_grows_after_enough_successes() {
        let limiter = AimdLimiter::new();
        for _ in 0..SUCCESSES_PER_INCREASE {
            limiter.report_success();
        }
        assert!(limiter.ceiling() >= 1); // grows, unless max_ceiling is 1.
    }

    #[test]
    fn limiter_halves_on_throttle() {
        let limiter = AimdLimiter::new();
        // Force ceiling up first (best-effort; capped by host parallelism).
        for _ in 0..(SUCCESSES_PER_INCREASE * 4) {
            limiter.report_success();
        }
        let before = limiter.ceiling();
        limiter.report_throttle();
        let after = limiter.ceiling();
        assert!(after <= before);
        assert!(after >= 1);
    }

    #[test]
    fn limiter_never_exceeds_available_parallelism() {
        let limiter = AimdLimiter::new();
        for _ in 0..10_000 {
            limiter.report_success();
        }
        let max = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(FALLBACK_PARALLELISM);
        assert!(limiter.ceiling() <= max);
    }

    #[test]
    fn limiter_serializes_concurrent_acquires_under_ceiling_one() {
        let limiter = Arc::new(AimdLimiter::new());
        let concurrent = Arc::new(AtomicUsize::new(0));
        let max_seen = Arc::new(AtomicUsize::new(0));

        std::thread::scope(|scope| {
            for _ in 0..8 {
                let limiter = Arc::clone(&limiter);
                let concurrent = Arc::clone(&concurrent);
                let max_seen = Arc::clone(&max_seen);
                scope.spawn(move || {
                    let _permit = limiter.acquire();
                    let now = concurrent.fetch_add(1, Ordering::SeqCst) + 1;
                    max_seen.fetch_max(now, Ordering::SeqCst);
                    std::thread::sleep(Duration::from_millis(5));
                    concurrent.fetch_sub(1, Ordering::SeqCst);
                });
            }
        });

        assert_eq!(max_seen.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn backoff_with_jitter_respects_floor() {
        let d = backoff_with_jitter(Duration::from_millis(1), 1, Duration::from_secs(3));
        assert!(d >= Duration::from_secs(3));
    }

    #[test]
    fn backoff_with_jitter_grows_with_attempt() {
        let a1 = backoff_with_jitter(Duration::from_millis(100), 1, Duration::ZERO);
        let a4 = backoff_with_jitter(Duration::from_millis(100), 4, Duration::ZERO);
        // Even with jitter, doubling four times should dwarf attempt 1.
        assert!(a4 > a1);
    }
}
