//! Per-session command rate limiting (SPEC §2.1.1).
//!
//! Leaky bucket: sustained rate 10 commands/s, burst 20 (tenant-configurable
//! via constructor parameters). Time is injected so callers control the clock
//! and tests need no sleeping. Enforcement wiring (dropping input and sending
//! the structured `rate_limited` event) is the gateway's job in M1-21.

use std::num::NonZeroU32;
use std::time::Instant;

/// Sustained command rate in commands per second (SPEC §2.1.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SustainedRate(NonZeroU32);

impl SustainedRate {
    /// SPEC §2.1.1 default: 10 commands per second.
    // NonZeroU32 has no const, lint-clean literal constructor; MIN + 9 == 10.
    pub const DEFAULT: Self = Self(NonZeroU32::MIN.saturating_add(9));

    /// Wraps a sustained rate in commands per second.
    #[must_use]
    pub const fn new(per_second: NonZeroU32) -> Self {
        Self(per_second)
    }
}

/// Maximum burst size in commands (SPEC §2.1.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Burst(NonZeroU32);

impl Burst {
    /// SPEC §2.1.1 default: burst of 20 commands.
    // NonZeroU32 has no const, lint-clean literal constructor; MIN + 19 == 20.
    pub const DEFAULT: Self = Self(NonZeroU32::MIN.saturating_add(19));

    /// Wraps a burst size in commands.
    #[must_use]
    pub const fn new(commands: NonZeroU32) -> Self {
        Self(commands)
    }
}

/// Outcome of a rate-limit check for one command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub enum Decision {
    /// Forward the command to the World.
    Forward,
    /// Drop the command; the gateway reports `rate_limited` to the client.
    Drop,
}

/// Leaky-bucket rate limiter for one session's command stream.
///
/// `check` must be called once per received command with the current time;
/// the bucket drains continuously at the sustained rate and holds at most
/// `burst` undrained commands.
#[derive(Debug)]
pub struct RateLimiter {
    drain_per_sec: f64,
    capacity: f64,
    level: f64,
    last: Instant,
}

impl RateLimiter {
    /// Creates a limiter with the given rate and burst, starting empty at `now`.
    #[must_use]
    pub fn new(rate: SustainedRate, burst: Burst, now: Instant) -> Self {
        Self {
            drain_per_sec: f64::from(rate.0.get()),
            capacity: f64::from(burst.0.get()),
            level: 0.0,
            last: now,
        }
    }

    /// Records one command at `now` and decides whether to forward or drop it.
    ///
    /// A non-monotonic `now` (before the previous call) is treated as zero
    /// elapsed time: nothing drains, nothing panics.
    pub fn check(&mut self, now: Instant) -> Decision {
        let elapsed = now.saturating_duration_since(self.last).as_secs_f64();
        self.last = self.last.max(now);
        self.level = (self.level - elapsed * self.drain_per_sec).max(0.0);
        if self.level + 1.0 <= self.capacity {
            self.level += 1.0;
            Decision::Forward
        } else {
            Decision::Drop
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn burst_of_20_forwards_then_drops() {
        let t0 = Instant::now();
        let mut limiter = RateLimiter::new(SustainedRate::DEFAULT, Burst::DEFAULT, t0);
        for i in 0..20 {
            assert_eq!(
                limiter.check(t0),
                Decision::Forward,
                "command {i} within burst must be forwarded"
            );
        }
        assert_eq!(
            limiter.check(t0),
            Decision::Drop,
            "21st command in the same instant must drop"
        );
    }

    #[test]
    fn bucket_drains_at_sustained_rate() {
        let t0 = Instant::now();
        let mut limiter = RateLimiter::new(SustainedRate::DEFAULT, Burst::DEFAULT, t0);
        for _ in 0..20 {
            let _ = limiter.check(t0);
        }
        assert_eq!(limiter.check(t0), Decision::Drop, "bucket starts full");
        // After 1 s at 10 commands/s, 10 slots have drained.
        let t1 = t0 + Duration::from_secs(1);
        for i in 0..10 {
            assert_eq!(
                limiter.check(t1),
                Decision::Forward,
                "drained slot {i} must be forwarded"
            );
        }
        assert_eq!(
            limiter.check(t1),
            Decision::Drop,
            "11th command after 1 s must drop"
        );
    }

    #[test]
    fn steady_pace_below_rate_never_drops() {
        let t0 = Instant::now();
        let mut limiter = RateLimiter::new(SustainedRate::DEFAULT, Burst::DEFAULT, t0);
        // One command every 200 ms = 5/s, below the 10/s sustained rate.
        for i in 0..50u64 {
            let now = t0 + Duration::from_millis(200 * i);
            assert_eq!(
                limiter.check(now),
                Decision::Forward,
                "command {i} at 5/s must be forwarded"
            );
        }
    }

    #[test]
    fn non_monotonic_clock_does_not_panic_or_refill() {
        let t0 = Instant::now();
        let t1 = t0 + Duration::from_secs(1);
        let mut limiter = RateLimiter::new(SustainedRate::DEFAULT, Burst::DEFAULT, t1);
        for _ in 0..20 {
            let _ = limiter.check(t1);
        }
        // Clock goes backwards: must not panic and must not grant extra slots.
        assert_eq!(limiter.check(t0), Decision::Drop);
    }
}
