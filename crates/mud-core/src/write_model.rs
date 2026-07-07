//! The write-model vocabulary shared by [`World`](crate::World) (which applies
//! it) and [`Scheduler`](crate::Scheduler) (which queues it): the primitive
//! [`Effect`], its optional [`Precondition`] guard, the [`MutationCommand`] that
//! pairs them, and the [`TickEvent`] outcomes an apply produces (§2.5.3.3,
//! §2.5.3.5, §3.16.2).
//!
//! These are plain data with no scheduling or apply logic, so both the domain
//! aggregate and the scheduler can depend on them without a module cycle.

use crate::{ArenaError, EntityId, PlaceId};

/// A primitive world mutation. Each variant maps to one [`World`] operation.
///
/// Effects are deliberately primitive (no compound "pick up" / "drop");
/// higher-level operations compose them. Atomicity of a read-then-write is
/// provided orthogonally by a [`Precondition`] on the [`MutationCommand`], not
/// by a compound effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Effect {
    /// Create a new entity; the minted handle is reported via
    /// [`TickEvent::Created`].
    Create,
    /// Tear an entity down (free its handle, clear its location).
    Teardown {
        /// The entity to tear down.
        entity: EntityId,
    },
    /// Move an entity to a Place, replacing its previous location.
    MoveTo {
        /// The entity being moved.
        entity: EntityId,
        /// The destination Place.
        place: PlaceId,
    },
    /// Clear an entity's location, so it is located nowhere (e.g. an item lifted
    /// off the ground into an inventory).
    ClearLocation {
        /// The entity whose location is cleared.
        entity: EntityId,
    },
    /// Add an item to a container's inventory.
    InventoryAdd {
        /// The container receiving the item.
        container: EntityId,
        /// The item being added.
        item: EntityId,
    },
    /// Remove an item from a container's inventory.
    InventoryRemove {
        /// The container losing the item.
        container: EntityId,
        /// The item being removed.
        item: EntityId,
    },
}

/// A condition evaluated against the [`World`] at apply time. When a
/// [`MutationCommand`] carries one and it does not hold, the effect is skipped
/// and a [`TickEvent::PreconditionFailed`] is emitted (§2.5.3.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Precondition {
    /// Holds when `entity` is currently located at `place`.
    LocatedIn {
        /// The entity whose location is checked.
        entity: EntityId,
        /// The Place it must currently occupy.
        place: PlaceId,
    },
    /// Holds when `container` currently holds `item`.
    Contains {
        /// The container whose contents are checked.
        container: EntityId,
        /// The item it must currently hold.
        item: EntityId,
    },
}

/// A single unit of work for the scheduler: an [`Effect`] with an optional
/// [`Precondition`] guard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub struct MutationCommand {
    precondition: Option<Precondition>,
    effect: Effect,
}

impl MutationCommand {
    /// Creates an unconditional command carrying `effect`.
    pub fn new(effect: Effect) -> Self {
        Self {
            precondition: None,
            effect,
        }
    }

    /// Attaches a precondition, making this a guarded read-then-write
    /// (§2.5.3.5). The effect applies only if `precondition` holds at apply
    /// time.
    pub fn with_precondition(mut self, precondition: Precondition) -> Self {
        self.precondition = Some(precondition);
        self
    }

    /// The effect this command applies.
    #[must_use]
    pub fn effect(&self) -> Effect {
        self.effect
    }

    /// The precondition guarding this command, if any.
    #[must_use]
    pub fn precondition(&self) -> Option<Precondition> {
        self.precondition
    }
}

/// The outcome of applying one [`MutationCommand`] during a tick.
///
/// Successful primitive effects other than [`Effect::Create`] produce no event;
/// only outcomes a caller must observe are reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TickEvent {
    /// An [`Effect::Create`] minted this entity. The only way a submitter learns
    /// the new handle, since commands apply asynchronously to submission.
    Created {
        /// The freshly minted entity handle.
        entity: EntityId,
    },
    /// A command's precondition did not hold; its effect was not applied
    /// (§2.5.3.5).
    PreconditionFailed {
        /// The precondition that failed.
        precondition: Precondition,
        /// The effect that was therefore skipped.
        effect: Effect,
    },
    /// An effect was rejected by the arena (a stale or foreign handle, or slot
    /// exhaustion) and was not applied.
    Rejected {
        /// The effect that was rejected.
        effect: Effect,
        /// Why the arena rejected it.
        error: ArenaError,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::NonZeroU64;

    fn entity_place() -> (EntityId, PlaceId) {
        // EntityId is minted by the arena; for a pure builder test we only need a
        // PlaceId, which has a public constructor.
        let place = PlaceId::new(NonZeroU64::new(10).expect("non-zero place id"));
        // A dummy entity id via the arena keeps this independent of scheduler.
        let mut arena =
            crate::EntityArena::new(crate::TenantTag::new(1).expect("tenant tag in range"));
        let entity = arena.alloc().expect("arena must mint an entity");
        (entity, place)
    }

    #[test]
    fn command_carries_effect_and_optional_precondition() {
        let (entity, place) = entity_place();
        let effect = Effect::MoveTo { entity, place };
        let bare = MutationCommand::new(effect);
        assert_eq!(bare.effect(), effect);
        assert_eq!(bare.precondition(), None);

        let guard = Precondition::LocatedIn { entity, place };
        let guarded = MutationCommand::new(effect).with_precondition(guard);
        assert_eq!(guarded.precondition(), Some(guard));
        assert_eq!(guarded.effect(), effect);
    }
}
