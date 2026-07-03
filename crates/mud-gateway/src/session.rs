//! Session id minting for accepted connections.

use std::num::NonZeroU64;
use std::sync::atomic::{AtomicU64, Ordering};

use mud_schema::SessionId;

use crate::error::GatewayError;

/// Mints monotonically increasing, non-zero [`SessionId`]s for accepted
/// connections (§2.1.3.1: the Gateway owns the id space of its channel).
#[derive(Debug)]
#[allow(dead_code)]
// LINT: SessionMinter and its methods are public API consumed by M1-21 Tasks 2–6
pub(crate) struct SessionMinter(AtomicU64);

impl SessionMinter {
    /// Starts minting at id 1 (ids are 1-based; 0 is unrepresentable).
    #[allow(dead_code)]
    // LINT: methods are consumed by M1-21 Tasks 2–6
    pub(crate) fn new() -> Self {
        Self(AtomicU64::new(1))
    }

    /// Mints the next id.
    ///
    /// # Errors
    ///
    /// [`GatewayError::SessionIdOverflow`] if the counter wrapped to zero —
    /// practically unreachable (2^64 connections) but mapped to an error so
    /// the non-zero invariant holds without panicking.
    #[allow(dead_code)]
    // LINT: methods are consumed by M1-21 Tasks 2–6
    pub(crate) fn next(&self) -> Result<SessionId, GatewayError> {
        let raw = self.0.fetch_add(1, Ordering::Relaxed);
        NonZeroU64::new(raw)
            .map(SessionId::new)
            .ok_or(GatewayError::SessionIdOverflow)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mints_monotonic_ids_starting_at_one() {
        let minter = SessionMinter::new();
        let first = minter.next().expect("first id must mint");
        let second = minter.next().expect("second id must mint");
        assert_eq!(first.get().get(), 1);
        assert_eq!(second.get().get(), 2);
    }

    #[test]
    fn wrapped_counter_is_an_error_not_a_panic() {
        let minter = SessionMinter(AtomicU64::new(0));
        assert!(matches!(
            minter.next(),
            Err(GatewayError::SessionIdOverflow)
        ));
    }
}
