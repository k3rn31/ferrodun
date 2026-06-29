//! The per-run trace-correlation id (§2.7.1).
//!
//! Every command run carries a [`CommandId`] so failures, lock denials, script
//! errors, and (later) LLM tool calls share one id for log correlation. Ids are
//! minted by the [`CommandIdGen`] counter the [`Pipeline`](crate::Pipeline)
//! owns; they are process-local and never persisted.

use std::fmt;
use std::num::NonZeroU64;

use crate::PipelineError;

/// Correlates everything one command run emits (§2.7.1).
///
/// Backed by `NonZeroU64`: ids are 1-based, so "no command" is representable as
/// `Option::None` for free, never as a meaningless id `0`. Mirrors the
/// [`SessionId`](mud_schema::SessionId) niche.
///
/// Uniqueness is per [`Pipeline`](crate::Pipeline) (one World): every pipeline
/// counts from `1`, so a process running more than one World will reuse ids
/// across them. Disambiguating multi-World logs (by tenant/World) is a runtime
/// concern for M1-22, not a property of this id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[must_use]
pub struct CommandId(NonZeroU64);

impl CommandId {
    /// Returns the underlying id value.
    pub const fn get(self) -> NonZeroU64 {
        self.0
    }
}

impl fmt::Display for CommandId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Mints strictly increasing [`CommandId`]s for one pipeline.
///
/// Single-threaded by construction: the pipeline owns one generator and mints
/// one id per `dispatch`. Holds the *next* id to hand out, or `None` once the
/// space is spent.
pub(crate) struct CommandIdGen(Option<NonZeroU64>);

impl CommandIdGen {
    /// A fresh generator whose first id is `1`.
    pub(crate) fn new() -> Self {
        Self(Some(NonZeroU64::MIN))
    }

    /// Hands out the next id and advances the counter.
    ///
    /// # Errors
    ///
    /// Returns [`PipelineError::CommandIdExhausted`] once the `u64` space is
    /// spent — a counter overflow rather than a silent wrap, so two runs can
    /// never collide on an id. The largest id (`u64::MAX`) is handed out; the
    /// call *after* it fails.
    pub(crate) fn next(&mut self) -> Result<CommandId, PipelineError> {
        let current = self.0.ok_or(PipelineError::CommandIdExhausted)?;
        self.0 = current.checked_add(1);
        Ok(CommandId(current))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The non-zero niche encodes "no command" as `None` for free.
    #[test]
    fn option_command_id_is_niche_optimized() {
        assert_eq!(size_of::<Option<CommandId>>(), 8);
    }

    #[test]
    fn ids_are_strictly_increasing() {
        let mut ids = CommandIdGen::new();
        let first = ids.next().expect("first id");
        let second = ids.next().expect("second id");
        assert!(first < second);
        assert_eq!(first.get(), NonZeroU64::MIN);
    }

    #[test]
    fn exhaustion_is_an_error_not_a_wrap() {
        let mut ids = CommandIdGen(Some(NonZeroU64::new(u64::MAX).expect("non-zero")));
        let last = ids.next().expect("the final id is still handed out");
        assert_eq!(last.get().get(), u64::MAX);
        assert!(matches!(ids.next(), Err(PipelineError::CommandIdExhausted)));
    }
}
