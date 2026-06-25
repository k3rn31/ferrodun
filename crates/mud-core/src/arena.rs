//! Per-tenant generational arena (§2.3.1–2.3.2, §3.11.4).
//!
//! The arena mints and validates the [`EntityId`] handles defined in
//! [`crate::entity_id`]. It is the engine's liveness authority: every entity
//! lookup resolves a handle here first, so a stale handle (the slot was freed
//! and possibly reused) and a foreign handle (minted by another tenant) are
//! both caught at this boundary rather than silently dereferencing the wrong
//! entity (§3.11.4).
//!
//! It is a pure liveness registry and stores no component payload. Per §2.3.2
//! components live in separate dense side-tables and the typed bag; the arena
//! only answers "is this handle live, and is it mine?".
//!
//! Slot reuse follows the standard generational-index discipline: freeing a
//! slot advances its generation so outstanding handles become detectably stale
//! (§2.3.7.3). When a slot's generation cannot advance further it is **burned**
//! — retired forever rather than recycled into a colliding id (§2.3.1.3).

use crate::entity_id::{EntityId, Generation, SlotIndex, TenantTag};

/// Errors from arena allocation and resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ArenaError {
    /// The handle belongs to a different tenant than this arena. Kept distinct
    /// from [`ArenaError::StaleHandle`] because cross-tenant access is a
    /// boundary violation (§3.11.4), not an ordinary use-after-free.
    #[error("handle from tenant {handle} used in tenant {arena}'s arena", handle = handle.get(), arena = arena.get())]
    CrossTenant {
        /// Tenant that owns this arena.
        arena: TenantTag,
        /// Tenant the offending handle was minted in.
        handle: TenantTag,
    },
    /// The handle does not name a live entity in this arena: its slot is out of
    /// range, free, or burned, or its generation no longer matches the slot.
    #[error("stale entity handle")]
    StaleHandle,
    /// The arena's 32-bit slot space is exhausted. Defensive: reaching this
    /// requires roughly four billion concurrently live slots.
    #[error("entity slot space exhausted")]
    Exhausted,
}

/// Liveness state of a single arena slot.
enum SlotState {
    /// Slot holds a live entity at the slot's current generation.
    Live,
    /// Slot is free and available for reuse at the slot's current generation.
    Free,
    /// Slot's generation is exhausted; it is retired forever (§2.3.1.3).
    Burned,
}

struct Slot {
    generation: Generation,
    state: SlotState,
}

/// A per-tenant generational arena that mints [`EntityId`]s and validates
/// handles against tenant ownership and slot liveness.
///
/// Construct one arena per tenant; the arena stamps its [`TenantTag`] onto
/// every id it mints and rejects handles minted elsewhere.
pub struct EntityArena {
    tenant: TenantTag,
    slots: Vec<Slot>,
    /// Indices of slots in [`SlotState::Free`], ready to reuse. Burned slots
    /// are never pushed here, so they are never minted again.
    free: Vec<SlotIndex>,
}

impl EntityArena {
    /// Creates an empty arena owned by `tenant`.
    pub fn new(tenant: TenantTag) -> Self {
        Self {
            tenant,
            slots: Vec::new(),
            free: Vec::new(),
        }
    }

    /// The tenant that owns this arena.
    pub fn tenant(&self) -> TenantTag {
        self.tenant
    }

    /// Allocates a new entity, returning its handle.
    ///
    /// Reuses a previously freed slot when one is available, otherwise grows
    /// the arena. Returns [`ArenaError::Exhausted`] only when the 32-bit slot
    /// space is full.
    pub fn alloc(&mut self) -> Result<EntityId, ArenaError> {
        if let Some(slot) = self.free.pop() {
            let entry = self
                .slots
                .get_mut(slot.get() as usize)
                .ok_or(ArenaError::StaleHandle)?;
            entry.state = SlotState::Live;
            return Ok(EntityId::new(self.tenant, slot, entry.generation));
        }

        let index = u32::try_from(self.slots.len()).map_err(|_| ArenaError::Exhausted)?;
        let slot = SlotIndex::new(index);
        let generation = Generation::FIRST;
        self.slots.push(Slot {
            generation,
            state: SlotState::Live,
        });
        Ok(EntityId::new(self.tenant, slot, generation))
    }

    /// Frees a live entity, invalidating its handle.
    ///
    /// Advances the slot's generation so outstanding handles become stale
    /// (§2.3.7.3); when the generation cannot advance the slot is burned and
    /// never reused (§2.3.1.3). Returns [`ArenaError::CrossTenant`] for a
    /// foreign handle and [`ArenaError::StaleHandle`] for a handle that does
    /// not name a live entity (including a double free).
    pub fn free(&mut self, id: EntityId) -> Result<(), ArenaError> {
        let slot = self.resolve(id)?;
        let entry = self
            .slots
            .get_mut(slot.get() as usize)
            .ok_or(ArenaError::StaleHandle)?;

        match entry.generation.next() {
            Some(next) => {
                entry.generation = next;
                entry.state = SlotState::Free;
                self.free.push(slot);
            }
            // Generation exhausted: burn the slot rather than recycle it into a
            // colliding id (§2.3.1.3). It is deliberately not pushed to `free`.
            None => entry.state = SlotState::Burned,
        }
        Ok(())
    }

    /// Resolves a handle to its validated slot index.
    ///
    /// This is the tenant- and liveness-checked gate every entity lookup goes
    /// through. Returns [`ArenaError::CrossTenant`] when the handle was minted
    /// in another tenant (§3.11.4) and [`ArenaError::StaleHandle`] when it does
    /// not name a live entity here. On success the returned [`SlotIndex`] is
    /// guaranteed to name a live entity owned by this arena.
    pub fn resolve(&self, id: EntityId) -> Result<SlotIndex, ArenaError> {
        if id.tenant() != self.tenant {
            return Err(ArenaError::CrossTenant {
                arena: self.tenant,
                handle: id.tenant(),
            });
        }

        let slot = id.slot();
        let entry = self
            .slots
            .get(slot.get() as usize)
            .ok_or(ArenaError::StaleHandle)?;

        let is_live = matches!(entry.state, SlotState::Live) && entry.generation == id.generation();
        if is_live {
            Ok(slot)
        } else {
            Err(ArenaError::StaleHandle)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tenant(value: u16) -> TenantTag {
        TenantTag::new(value).expect("test tenant tag must be in range")
    }

    #[test]
    fn alloc_stamps_the_arena_tenant() {
        let mut arena = EntityArena::new(tenant(7));

        let id = arena.alloc().expect("alloc must succeed on an empty arena");

        assert_eq!(id.tenant(), tenant(7));
    }

    #[test]
    fn fresh_handle_resolves_to_its_slot() {
        let mut arena = EntityArena::new(tenant(1));

        let id = arena.alloc().expect("alloc must succeed");

        assert_eq!(arena.resolve(id), Ok(id.slot()));
    }

    #[test]
    fn freed_handle_is_stale() {
        let mut arena = EntityArena::new(tenant(1));
        let id = arena.alloc().expect("alloc must succeed");

        arena.free(id).expect("freeing a live handle must succeed");

        assert_eq!(arena.resolve(id), Err(ArenaError::StaleHandle));
    }

    #[test]
    fn reused_slot_keeps_index_but_bumps_generation() {
        let mut arena = EntityArena::new(tenant(1));
        let first = arena.alloc().expect("first alloc must succeed");
        arena.free(first).expect("free must succeed");

        let second = arena.alloc().expect("second alloc must reuse the slot");

        assert_eq!(second.slot(), first.slot());
        assert_ne!(second.generation(), first.generation());
        assert_eq!(arena.resolve(first), Err(ArenaError::StaleHandle));
        assert_eq!(arena.resolve(second), Ok(second.slot()));
    }

    // §3.11.4: a handle minted in one tenant must not be resolvable or mutable
    // through another tenant's arena. This is the M1-02 tenant-isolation gate.
    #[test]
    fn foreign_handle_is_rejected_by_another_tenant() {
        let mut arena_a = EntityArena::new(tenant(1));
        let mut arena_b = EntityArena::new(tenant(2));
        let id = arena_a.alloc().expect("alloc in tenant A must succeed");

        let cross_tenant = ArenaError::CrossTenant {
            arena: tenant(2),
            handle: tenant(1),
        };
        assert_eq!(arena_b.resolve(id), Err(cross_tenant));
        assert_eq!(arena_b.free(id), Err(cross_tenant));
    }

    // §2.3.1.3: once a slot's generation is exhausted, freeing it burns the
    // slot rather than recycling it into a colliding id. Exercised through the
    // real free/alloc path by cycling a single slot to its last generation.
    #[test]
    fn exhausted_generation_burns_the_slot() {
        let mut arena = EntityArena::new(tenant(1));
        let first = arena.alloc().expect("first alloc must succeed");
        let burned_slot = first.slot();

        // Cycle the lone slot until its generation reaches MAX. Each free below
        // MAX returns it to the free list; the free at MAX burns it.
        let mut current = first;
        while current.generation().get() < Generation::MAX {
            arena.free(current).expect("free below MAX must recycle");
            current = arena.alloc().expect("recycled slot must realloc");
            assert_eq!(current.slot(), burned_slot);
        }

        arena.free(current).expect("free at MAX must burn the slot");

        // The burned slot is retired: the next alloc grows the arena instead of
        // reusing it, and no handle to the burned slot resolves.
        let next = arena.alloc().expect("alloc after burn must succeed");
        assert_ne!(next.slot(), burned_slot);
        assert_eq!(arena.resolve(current), Err(ArenaError::StaleHandle));
    }

    // A same-tenant handle naming a slot the arena never allocated is stale,
    // not a tenant violation: the bounds check, distinct from the generation
    // check the other stale-handle tests exercise.
    #[test]
    fn out_of_range_slot_is_stale() {
        let arena = EntityArena::new(tenant(1));
        let phantom = EntityId::new(tenant(1), SlotIndex::new(999), Generation::FIRST);

        assert_eq!(arena.resolve(phantom), Err(ArenaError::StaleHandle));
    }

    #[test]
    fn double_free_is_rejected() {
        let mut arena = EntityArena::new(tenant(1));
        let id = arena.alloc().expect("alloc must succeed");
        arena.free(id).expect("first free must succeed");

        assert_eq!(arena.free(id), Err(ArenaError::StaleHandle));
    }
}
