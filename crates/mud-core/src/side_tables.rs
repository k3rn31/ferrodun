//! Dense hot-component side-tables (§2.3.2.2).
//!
//! Hot components are touched every tick / combat round, so §2.3.2.2 mandates
//! they live in dense, slot-indexed arrays rather than the dynamic component
//! bag. This module holds the two M1 needs: [`LocationOf`] (which [`Place`] each
//! entity occupies, plus a reverse occupant index) and [`Inventory`] (which
//! entities a container holds). The other three hot components §2.3.2.2 lists —
//! `Position`, `Health`, `Initiative` — are added when their own milestone first
//! uses them (YAGNI).
//!
//! These tables are **pure storage keyed by [`SlotIndex`]** (the slot half of an
//! [`EntityId`]); they are deliberately *not* the liveness authority. The arena
//! ([`crate::EntityArena`]) owns liveness: a caller resolves a handle through
//! [`EntityArena::resolve`](crate::EntityArena::resolve) — which rejects stale
//! and cross-tenant handles — and only then indexes a side-table. Keeping the
//! tables ignorant of liveness is the §2.3.2 separation, not missing validation.
//!
//! [`Place`]: crate::Place

use std::collections::HashMap;

use crate::{EntityId, PlaceId, SlotIndex};

/// Maps a slot index to a position in a dense `by_slot` vector, or `None` when
/// the index cannot be one — possible only on targets where `usize` is narrower
/// than `u32`, where such a slot could never have been allocated. Mirrors the
/// arena's `slot_position`.
fn slot_index(slot: SlotIndex) -> Option<usize> {
    usize::try_from(slot.get()).ok()
}

/// The location of every resident entity: a dense forward table (entity → the
/// [`Place`](crate::Place) it occupies) plus a reverse occupant index (Place →
/// the entities in it), one of the §2.3.2.2 hot components.
///
/// The reverse index lets [`occupants`](LocationOf::occupants) iterate a Place's
/// occupants in `O(occupants)` rather than scanning every slot. The two halves
/// are maintained in lockstep by [`place`](LocationOf::place) and
/// [`remove`](LocationOf::remove).
#[derive(Debug, Default)]
#[must_use]
pub struct LocationOf {
    /// Forward, dense by slot: the Place each resident entity occupies. `None`
    /// = the slot holds no located entity.
    by_slot: Vec<Option<PlaceId>>,
    /// Reverse index: the entities currently in each Place. Empty lists are
    /// pruned so a Place with no occupants is simply absent.
    occupants: HashMap<PlaceId, Vec<EntityId>>,
}

impl LocationOf {
    /// Creates an empty location table.
    pub fn new() -> Self {
        Self::default()
    }

    /// Places `entity` at the Place `at`, replacing any previous location.
    ///
    /// When the entity was already somewhere, it is first removed from that
    /// Place's occupant list, so the reverse index stays consistent as entities
    /// move.
    pub fn place(&mut self, entity: EntityId, at: PlaceId) {
        let Some(index) = slot_index(entity.slot()) else {
            return;
        };

        if index >= self.by_slot.len() {
            let Some(len) = index.checked_add(1) else {
                return;
            };
            self.by_slot.resize(len, None);
        }

        // INVARIANT: `index < by_slot.len()` — it was either already in range or
        // just grown to cover it above; `by_slot` never shrinks.
        let Some(cell) = self.by_slot.get_mut(index) else {
            unreachable!("slot {} is out of range after grow", entity.slot().get());
        };

        if let Some(previous) = cell.replace(at) {
            remove_occupant(&mut self.occupants, previous, entity.slot());
        }
        self.occupants.entry(at).or_default().push(entity);
    }

    /// Removes `entity` from wherever it is, clearing its location and its
    /// reverse-index entry. A no-op if the entity has no location. Releases the
    /// entity's hot-component slot for teardown (§2.3.7.3).
    pub fn remove(&mut self, entity: EntityId) {
        let Some(previous) = slot_index(entity.slot())
            .and_then(|index| self.by_slot.get_mut(index))
            .and_then(Option::take)
        else {
            return;
        };
        remove_occupant(&mut self.occupants, previous, entity.slot());
    }

    /// The Place `entity` currently occupies, or `None` if it has no location.
    #[must_use]
    pub fn location(&self, entity: EntityId) -> Option<PlaceId> {
        slot_index(entity.slot())
            .and_then(|index| self.by_slot.get(index))
            .copied()
            .flatten()
    }

    /// The entities currently in `place`. Empty for a Place no entity occupies.
    pub fn occupants(&self, place: PlaceId) -> impl Iterator<Item = EntityId> + '_ {
        self.occupants.get(&place).into_iter().flatten().copied()
    }
}

/// Removes the occupant sitting in `place` at `slot`, pruning the Place's entry
/// when it empties. Matches by slot, not full [`EntityId`]: a slot occupies at
/// most one Place at a time, so this stays correct even across slot reuse.
fn remove_occupant(
    occupants: &mut HashMap<PlaceId, Vec<EntityId>>,
    place: PlaceId,
    slot: SlotIndex,
) {
    let Some(list) = occupants.get_mut(&place) else {
        return;
    };
    list.retain(|occupant| occupant.slot() != slot);
    if list.is_empty() {
        occupants.remove(&place);
    }
}

/// The entities each container holds: a dense table (container → its items), one
/// of the §2.3.2.2 hot components.
///
/// In M1 this table only records containment. Cross-container exclusivity (an
/// item in at most one inventory) and the location-versus-inventory relationship
/// are enforced by the mutation layer (M1-06), not here.
#[derive(Debug, Default)]
#[must_use]
pub struct Inventory {
    /// Dense by slot: the entities each container holds.
    by_slot: Vec<Vec<EntityId>>,
}

impl Inventory {
    /// Creates an empty inventory table.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds `item` to `container`'s inventory. A no-op if `item` is already in
    /// that container (an entity cannot be in one container twice).
    pub fn insert(&mut self, container: EntityId, item: EntityId) {
        let Some(index) = slot_index(container.slot()) else {
            return;
        };

        if index >= self.by_slot.len() {
            let Some(len) = index.checked_add(1) else {
                return;
            };
            self.by_slot.resize_with(len, Vec::new);
        }

        // INVARIANT: `index < by_slot.len()` — just grown to cover it if needed.
        let Some(contents) = self.by_slot.get_mut(index) else {
            unreachable!(
                "container slot {} is out of range after grow",
                container.slot().get()
            );
        };
        if !contents.iter().any(|held| held.slot() == item.slot()) {
            contents.push(item);
        }
    }

    /// Removes `item` from `container`'s inventory. A no-op if absent.
    pub fn remove(&mut self, container: EntityId, item: EntityId) {
        let Some(contents) =
            slot_index(container.slot()).and_then(|index| self.by_slot.get_mut(index))
        else {
            return;
        };
        contents.retain(|held| held.slot() != item.slot());
    }

    /// Drops every item `container` holds, releasing its hot-component slot for
    /// teardown (§2.3.7.3). A no-op for an empty or unknown container.
    ///
    /// Matches by slot so a freed slot carries no contents into its next tenant:
    /// without this, a reused container slot would inherit the torn-down
    /// entity's items, since [`insert`](Inventory::insert) appends rather than
    /// overwriting a reused slot.
    pub fn clear(&mut self, container: EntityId) {
        if let Some(contents) =
            slot_index(container.slot()).and_then(|index| self.by_slot.get_mut(index))
        {
            contents.clear();
        }
    }

    /// The entities in `container`. Empty for an empty or unknown container.
    pub fn contents(&self, container: EntityId) -> impl Iterator<Item = EntityId> + '_ {
        slot_index(container.slot())
            .and_then(|index| self.by_slot.get(index))
            .into_iter()
            .flatten()
            .copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::NonZeroU64;

    fn entity(slot: u32) -> EntityId {
        // The slot is the only field the side-tables key on; tenant/generation
        // are irrelevant here, so build a handle from raw bits via the slot.
        EntityId::new(tenant(), SlotIndex::new(slot), generation())
    }

    /// A handle to the same `slot` at a later generation, modeling the arena
    /// reusing a freed slot for a new entity.
    fn reused_slot(slot: u32, generation: u32) -> EntityId {
        let generation =
            crate::Generation::new(generation).expect("test generation must be in range");
        EntityId::new(tenant(), SlotIndex::new(slot), generation)
    }

    fn tenant() -> crate::TenantTag {
        crate::TenantTag::new(1).expect("test tenant tag must be in range")
    }

    fn generation() -> crate::Generation {
        crate::Generation::FIRST
    }

    fn place(value: u64) -> PlaceId {
        PlaceId::new(NonZeroU64::new(value).expect("test place id must be non-zero"))
    }

    const HALL: u64 = 10;
    const STUDY: u64 = 11;

    #[test]
    fn place_then_location_round_trips() {
        let mut locations = LocationOf::new();
        let goblin = entity(3);

        locations.place(goblin, place(HALL));

        assert_eq!(locations.location(goblin), Some(place(HALL)));
    }

    #[test]
    fn occupants_lists_every_entity_in_a_place() {
        let mut locations = LocationOf::new();
        let goblin = entity(3);
        let rat = entity(4);

        locations.place(goblin, place(HALL));
        locations.place(rat, place(HALL));

        let occupants: Vec<EntityId> = locations.occupants(place(HALL)).collect();
        assert_eq!(occupants, vec![goblin, rat]);
    }

    #[test]
    fn unlocated_entity_and_empty_place_are_empty() {
        let locations = LocationOf::new();
        let ghost = entity(7);

        assert_eq!(locations.location(ghost), None);
        assert_eq!(locations.occupants(place(HALL)).count(), 0);
    }

    // The reverse-index consistency property: moving an entity must leave it in
    // exactly one Place's occupant list, not both. A second `place` on the same
    // entity moves it out of the old Place and into the new one.
    #[test]
    fn moving_an_entity_keeps_the_reverse_index_consistent() {
        let mut locations = LocationOf::new();
        let goblin = entity(3);
        locations.place(goblin, place(HALL));

        locations.place(goblin, place(STUDY));

        assert_eq!(locations.location(goblin), Some(place(STUDY)));
        assert_eq!(locations.occupants(place(HALL)).count(), 0);
        assert_eq!(
            locations.occupants(place(STUDY)).collect::<Vec<_>>(),
            vec![goblin]
        );
    }

    // Moving an entity out of a Place must evict only that entity, not clear the
    // whole occupant list: a second resident left behind stays put. The
    // single-occupant move test above cannot tell eviction from a list reset.
    #[test]
    fn moving_one_entity_leaves_other_occupants_in_place() {
        let mut locations = LocationOf::new();
        let goblin = entity(3);
        let rat = entity(4);
        locations.place(goblin, place(HALL));
        locations.place(rat, place(HALL));

        locations.place(goblin, place(STUDY));

        assert_eq!(
            locations.occupants(place(HALL)).collect::<Vec<_>>(),
            vec![rat]
        );
        assert_eq!(
            locations.occupants(place(STUDY)).collect::<Vec<_>>(),
            vec![goblin]
        );
    }

    // Slot-reuse safety: the tables key on slot, so a fresh handle reusing a
    // freed entity's slot must overwrite the forward cell and reverse list rather
    // than coexist with the stale handle. (The mutation layer is expected to
    // `remove` on teardown, but `place` self-heals if it does not.)
    #[test]
    fn placing_a_reused_slot_supersedes_the_stale_handle() {
        let mut locations = LocationOf::new();
        let old = entity(3);
        locations.place(old, place(HALL));

        let new = reused_slot(3, 1);
        locations.place(new, place(HALL));

        assert_eq!(locations.location(new), Some(place(HALL)));
        assert_eq!(
            locations.occupants(place(HALL)).collect::<Vec<_>>(),
            vec![new]
        );
    }

    #[test]
    fn remove_clears_location_and_occupancy() {
        let mut locations = LocationOf::new();
        let goblin = entity(3);
        locations.place(goblin, place(HALL));

        locations.remove(goblin);

        assert_eq!(locations.location(goblin), None);
        assert_eq!(locations.occupants(place(HALL)).count(), 0);
    }

    #[test]
    fn insert_then_contents_round_trips() {
        let mut inventory = Inventory::new();
        let chest = entity(2);
        let sword = entity(5);
        let shield = entity(6);

        inventory.insert(chest, sword);
        inventory.insert(chest, shield);

        let contents: Vec<EntityId> = inventory.contents(chest).collect();
        assert_eq!(contents, vec![sword, shield]);
    }

    #[test]
    fn duplicate_insert_does_not_double_list() {
        let mut inventory = Inventory::new();
        let chest = entity(2);
        let sword = entity(5);

        inventory.insert(chest, sword);
        inventory.insert(chest, sword);

        assert_eq!(inventory.contents(chest).collect::<Vec<_>>(), vec![sword]);
    }

    #[test]
    fn remove_drops_an_item_and_empty_container_is_empty() {
        let mut inventory = Inventory::new();
        let chest = entity(2);
        let sword = entity(5);
        inventory.insert(chest, sword);

        inventory.remove(chest, sword);

        assert_eq!(inventory.contents(chest).count(), 0);
        assert_eq!(inventory.contents(entity(99)).count(), 0);
    }

    // Slot-reuse safety for containers: clearing a torn-down container's slot
    // must leave nothing for a fresh handle reusing that slot to inherit, since
    // `insert` appends rather than self-healing a reused slot.
    #[test]
    fn cleared_container_slot_carries_no_contents_to_a_reused_handle() {
        let mut inventory = Inventory::new();
        let chest = entity(2);
        inventory.insert(chest, entity(5));
        inventory.insert(chest, entity(6));

        inventory.clear(chest);

        assert_eq!(inventory.contents(chest).count(), 0);
        let reused = reused_slot(2, 1);
        assert_eq!(inventory.contents(reused).count(), 0);
    }
}
