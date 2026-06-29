//! The per-tenant mutable world aggregate.
//!
//! [`World`] bundles the three things a mutation touches — the liveness
//! [`EntityArena`] and the two hot side-tables [`LocationOf`] and [`Inventory`]
//! — for a single tenant. It is the apply target the scheduler ([`crate::Scheduler`])
//! mutates: the scheduler decides *ordering*, `World` performs the *effect*.
//!
//! `World` keeps its fields private and exposes two surfaces:
//!
//! - **Semantic operations** ([`create`](World::create), [`teardown`](World::teardown),
//!   [`move_to`](World::move_to), [`inventory_add`](World::inventory_add),
//!   [`inventory_remove`](World::inventory_remove)) — the apply surface.
//! - **Read predicates** ([`is_located_in`](World::is_located_in),
//!   [`contains`](World::contains)) — the precondition surface (§2.5.3.5).
//!
//! Every operation that touches a side-table first validates the handle through
//! the arena ([`resolve`](EntityArena::resolve) / [`free`](EntityArena::free)),
//! so the §2.3.2 rule holds: a side-table is never indexed without a live,
//! tenant-owned handle. The side-tables stay ignorant of liveness; `World` is
//! where the two are joined.

use crate::{
    ArenaError, Effect, EntityArena, EntityId, Inventory, Keyword, LocationOf, Naming, PlaceId,
    Precondition, TenantTag, TickEvent,
};

/// The mutable world for one tenant: liveness arena plus the hot side-tables.
///
/// Construct one `World` per tenant. All mutation flows through its semantic
/// operations so liveness is validated before any side-table is touched.
#[must_use]
pub struct World {
    arena: EntityArena,
    locations: LocationOf,
    inventory: Inventory,
    naming: Naming,
}

impl World {
    /// Creates an empty world owned by `tenant`.
    pub fn new(tenant: TenantTag) -> Self {
        Self {
            arena: EntityArena::new(tenant),
            locations: LocationOf::new(),
            inventory: Inventory::new(),
            naming: Naming::new(),
        }
    }

    /// The tenant that owns this world.
    pub fn tenant(&self) -> TenantTag {
        self.arena.tenant()
    }

    /// Creates a new entity, returning its freshly minted handle.
    ///
    /// Returns [`ArenaError::Exhausted`] only when the slot space is full.
    pub fn create(&mut self) -> Result<EntityId, ArenaError> {
        self.arena.alloc()
    }

    /// Tears an entity down: frees its handle and releases its hot-component
    /// slots (location, inventory contents, and keywords).
    ///
    /// Freeing validates tenant ownership and liveness, so a stale or foreign
    /// handle is rejected before any table is touched (§2.3.7.3). Returns the
    /// arena error in that case. Clearing the entity's own location and
    /// inventory keeps the slot clean for reuse (§2.3.7.3); since the side-tables
    /// key on slot, an uncleared slot would otherwise leak state into its next
    /// tenant. Removing the entity from any container that *holds it as an item*
    /// is not handled here: the inventory table has no reverse item→container
    /// index.
    pub fn teardown(&mut self, entity: EntityId) -> Result<(), ArenaError> {
        self.arena.free(entity)?;
        self.locations.remove(entity);
        self.inventory.clear(entity);
        self.naming.clear(entity);
        Ok(())
    }

    /// Moves `entity` to `place`, replacing any previous location.
    ///
    /// Resolves the handle for liveness first; returns the arena error for a
    /// stale or foreign handle.
    pub fn move_to(&mut self, entity: EntityId, place: PlaceId) -> Result<(), ArenaError> {
        // Resolve only to validate liveness; the side-table keys on the slot
        // itself, so the resolved index is not needed.
        let _ = self.arena.resolve(entity)?;
        self.locations.place(entity, place);
        Ok(())
    }

    /// Clears `entity`'s location, so it is located nowhere (e.g. an item lifted
    /// off the ground into an inventory). A no-op if it had no location.
    ///
    /// Resolves the handle for liveness first; returns the arena error for a
    /// stale or foreign handle.
    pub fn clear_location(&mut self, entity: EntityId) -> Result<(), ArenaError> {
        let _ = self.arena.resolve(entity)?;
        self.locations.remove(entity);
        Ok(())
    }

    /// Sets `entity`'s lookup keywords (§2.7 step 5), replacing any prior list.
    ///
    /// Resolves the handle for liveness first; returns the arena error for a
    /// stale or foreign handle.
    pub fn name_entity(
        &mut self,
        entity: EntityId,
        keywords: Vec<Keyword>,
    ) -> Result<(), ArenaError> {
        let _ = self.arena.resolve(entity)?;
        self.naming.set(entity, keywords);
        Ok(())
    }

    /// Adds `item` to `container`'s inventory.
    ///
    /// Resolves both handles for liveness first; returns the arena error for a
    /// stale or foreign handle on either.
    pub fn inventory_add(&mut self, container: EntityId, item: EntityId) -> Result<(), ArenaError> {
        let _ = self.arena.resolve(container)?;
        let _ = self.arena.resolve(item)?;
        self.inventory.insert(container, item);
        Ok(())
    }

    /// Removes `item` from `container`'s inventory. A no-op if absent.
    ///
    /// Resolves `container` for liveness first; returns the arena error for a
    /// stale or foreign handle.
    pub fn inventory_remove(
        &mut self,
        container: EntityId,
        item: EntityId,
    ) -> Result<(), ArenaError> {
        let _ = self.arena.resolve(container)?;
        self.inventory.remove(container, item);
        Ok(())
    }

    /// Applies one [`Effect`] to this world, returning a [`TickEvent`] when the
    /// outcome must be observed (a minted handle or an arena rejection) and
    /// `None` on a silent success.
    ///
    /// This is the single source of truth for the `Effect` → semantic-operation
    /// mapping and the arena-error → [`TickEvent::Rejected`] classification, so
    /// every apply path — the scheduler's in-memory drain and the persistence
    /// layer's write-through — shares one dispatch rather than re-deriving it.
    pub fn apply_effect(&mut self, effect: Effect) -> Option<TickEvent> {
        let result = match effect {
            Effect::Create => {
                return Some(match self.create() {
                    Ok(entity) => TickEvent::Created { entity },
                    Err(error) => TickEvent::Rejected { effect, error },
                });
            }
            Effect::Teardown { entity } => self.teardown(entity),
            Effect::MoveTo { entity, place } => self.move_to(entity, place),
            Effect::ClearLocation { entity } => self.clear_location(entity),
            Effect::InventoryAdd { container, item } => self.inventory_add(container, item),
            Effect::InventoryRemove { container, item } => self.inventory_remove(container, item),
        };
        result
            .err()
            .map(|error| TickEvent::Rejected { effect, error })
    }

    /// Whether `precondition` holds against this world's current state
    /// (§2.5.3.5).
    ///
    /// The single source of truth for precondition semantics, shared by the
    /// scheduler and the persistence layer so a guard is evaluated identically
    /// on every apply path.
    #[must_use]
    pub fn satisfies(&self, precondition: Precondition) -> bool {
        match precondition {
            Precondition::LocatedIn { entity, place } => self.is_located_in(entity, place),
            Precondition::Contains { container, item } => self.contains(container, item),
        }
    }

    /// Whether `entity` is a live handle currently located at `place`.
    ///
    /// A stale or foreign handle is never "located" anywhere, so this returns
    /// `false` rather than reading the side-table for it.
    #[must_use]
    pub fn is_located_in(&self, entity: EntityId, place: PlaceId) -> bool {
        self.arena.resolve(entity).is_ok() && self.locations.location(entity) == Some(place)
    }

    /// Whether `container` is a live handle currently holding `item`.
    ///
    /// A stale or foreign container handle holds nothing, so this returns
    /// `false` rather than reading the side-table for it.
    #[must_use]
    pub fn contains(&self, container: EntityId, item: EntityId) -> bool {
        self.arena.resolve(container).is_ok()
            && self.inventory.contents(container).any(|held| held == item)
    }

    /// The place `entity` currently occupies, or `None` if it is located nowhere
    /// or its handle is stale or foreign.
    #[must_use]
    pub fn location_of(&self, entity: EntityId) -> Option<PlaceId> {
        if self.arena.resolve(entity).is_err() {
            return None;
        }
        self.locations.location(entity)
    }

    /// The live entities currently in `place`, in occupancy order.
    ///
    /// Occupants are recorded only via [`move_to`](World::move_to), which
    /// validates liveness, so the reverse index holds live handles by
    /// construction.
    pub fn occupants_of(&self, place: PlaceId) -> impl Iterator<Item = EntityId> + '_ {
        self.locations.occupants(place)
    }

    /// The items `container` holds, in insertion order. Empty for a stale,
    /// foreign, or empty container.
    pub fn inventory_of(&self, container: EntityId) -> impl Iterator<Item = EntityId> + '_ {
        self.arena
            .resolve(container)
            .ok()
            .into_iter()
            .flat_map(move |_| self.inventory.contents(container))
    }

    /// The lookup keywords `entity` answers to (§2.7 step 5). Empty for a stale,
    /// foreign, or unnamed entity.
    pub fn keywords_of(&self, entity: EntityId) -> &[Keyword] {
        if self.arena.resolve(entity).is_err() {
            return &[];
        }
        self.naming.keywords(entity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::NonZeroU64;

    fn tenant(value: u16) -> TenantTag {
        TenantTag::new(value).expect("test tenant tag must be in range")
    }

    fn place(value: u64) -> PlaceId {
        PlaceId::new(NonZeroU64::new(value).expect("test place id must be non-zero"))
    }

    const HALL: u64 = 10;
    const STUDY: u64 = 11;

    #[test]
    fn created_entity_resolves_in_its_world() {
        let mut world = World::new(tenant(1));

        let entity = world
            .create()
            .expect("create must succeed on an empty world");

        // A created handle is live: it can be moved without an arena error.
        assert!(world.move_to(entity, place(HALL)).is_ok());
    }

    #[test]
    fn teardown_invalidates_the_handle_and_clears_location() {
        let mut world = World::new(tenant(1));
        let entity = world.create().expect("create must succeed");
        world
            .move_to(entity, place(HALL))
            .expect("move of a live entity must succeed");

        world
            .teardown(entity)
            .expect("teardown of a live entity must succeed");

        assert!(!world.is_located_in(entity, place(HALL)));
        // The handle is now stale: re-moving it is rejected by the arena.
        assert_eq!(
            world.move_to(entity, place(STUDY)),
            Err(ArenaError::StaleHandle)
        );
    }

    #[test]
    fn move_to_records_the_new_location() {
        let mut world = World::new(tenant(1));
        let entity = world.create().expect("create must succeed");

        world
            .move_to(entity, place(HALL))
            .expect("move must succeed");

        assert!(world.is_located_in(entity, place(HALL)));
        assert!(!world.is_located_in(entity, place(STUDY)));
    }

    #[test]
    fn move_to_rejects_a_foreign_handle() {
        let mut other = World::new(tenant(2));
        let foreign = other.create().expect("create in tenant 2 must succeed");
        let mut world = World::new(tenant(1));

        let result = world.move_to(foreign, place(HALL));

        assert_eq!(
            result,
            Err(ArenaError::CrossTenant {
                arena: tenant(1),
                handle: tenant(2),
            })
        );
    }

    #[test]
    fn inventory_add_then_contains_round_trips() {
        let mut world = World::new(tenant(1));
        let chest = world.create().expect("create chest must succeed");
        let sword = world.create().expect("create sword must succeed");

        world.inventory_add(chest, sword).expect("add must succeed");

        assert!(world.contains(chest, sword));
    }

    #[test]
    fn inventory_remove_drops_the_item() {
        let mut world = World::new(tenant(1));
        let chest = world.create().expect("create chest must succeed");
        let sword = world.create().expect("create sword must succeed");
        world.inventory_add(chest, sword).expect("add must succeed");

        world
            .inventory_remove(chest, sword)
            .expect("remove must succeed");

        assert!(!world.contains(chest, sword));
    }

    // Tearing down a container must clear its inventory so a fresh entity
    // reusing the freed slot inherits nothing (§2.3.7.3). The side-tables key on
    // slot, so without the teardown clear the reused handle would still "hold"
    // the torn-down container's items.
    #[test]
    fn teardown_clears_inventory_so_a_reused_slot_holds_nothing() {
        let mut world = World::new(tenant(1));
        let chest = world.create().expect("create chest must succeed");
        let sword = world.create().expect("create sword must succeed");
        world.inventory_add(chest, sword).expect("add must succeed");

        world.teardown(chest).expect("teardown must succeed");
        let reused = world.create().expect("create must reuse the freed slot");

        // Guard: the new entity must actually occupy the torn-down chest's slot,
        // otherwise the assertion below would pass vacuously.
        assert_eq!(reused.slot(), chest.slot());
        assert!(!world.contains(reused, sword));
    }

    #[test]
    fn predicates_are_false_for_a_stale_handle() {
        let mut world = World::new(tenant(1));
        let entity = world.create().expect("create must succeed");
        world
            .move_to(entity, place(HALL))
            .expect("move must succeed");
        world.teardown(entity).expect("teardown must succeed");

        assert!(!world.is_located_in(entity, place(HALL)));
        assert!(!world.contains(entity, entity));
    }

    #[test]
    fn apply_effect_create_reports_the_minted_handle() {
        let mut world = World::new(tenant(1));

        let entity = match world.apply_effect(Effect::Create) {
            Some(TickEvent::Created { entity }) => Some(entity),
            Some(TickEvent::PreconditionFailed { .. } | TickEvent::Rejected { .. }) | None => None,
        }
        .expect("apply_effect(Create) must report a Created event");

        // The reported handle is live: a follow-up move applies cleanly.
        assert!(
            world
                .apply_effect(Effect::MoveTo {
                    entity,
                    place: place(HALL),
                })
                .is_none()
        );
        assert!(world.is_located_in(entity, place(HALL)));
    }

    #[test]
    fn apply_effect_rejects_a_stale_handle_without_mutating() {
        let mut world = World::new(tenant(1));
        let entity = world.create().expect("create must succeed");
        world.teardown(entity).expect("teardown must succeed");

        let event = world.apply_effect(Effect::MoveTo {
            entity,
            place: place(HALL),
        });

        assert_eq!(
            event,
            Some(TickEvent::Rejected {
                effect: Effect::MoveTo {
                    entity,
                    place: place(HALL),
                },
                error: ArenaError::StaleHandle,
            })
        );
        // The rejected move left no trace: the stale handle is located nowhere.
        assert!(!world.is_located_in(entity, place(HALL)));
    }

    #[test]
    fn satisfies_reflects_current_location_and_containment() {
        let mut world = World::new(tenant(1));
        let chest = world.create().expect("create chest must succeed");
        let sword = world.create().expect("create sword must succeed");
        let coin = world.create().expect("create coin must succeed");
        world
            .move_to(chest, place(HALL))
            .expect("move must succeed");
        world.inventory_add(chest, sword).expect("add must succeed");

        assert!(world.satisfies(Precondition::LocatedIn {
            entity: chest,
            place: place(HALL),
        }));
        assert!(world.satisfies(Precondition::Contains {
            container: chest,
            item: sword,
        }));
        assert!(!world.satisfies(Precondition::LocatedIn {
            entity: chest,
            place: place(STUDY),
        }));
        assert!(!world.satisfies(Precondition::Contains {
            container: chest,
            item: coin,
        }));
    }

    #[test]
    fn location_of_reports_the_current_place_and_none_when_unlocated() {
        let mut world = World::new(tenant(1));
        let entity = world.create().expect("create must succeed");

        assert_eq!(world.location_of(entity), None);
        world
            .move_to(entity, place(HALL))
            .expect("move must succeed");
        assert_eq!(world.location_of(entity), Some(place(HALL)));
    }

    #[test]
    fn occupants_of_lists_entities_placed_there() {
        let mut world = World::new(tenant(1));
        let alice = world.create().expect("create alice");
        let bob = world.create().expect("create bob");
        world.move_to(alice, place(HALL)).expect("place alice");
        world.move_to(bob, place(HALL)).expect("place bob");

        let occupants: Vec<EntityId> = world.occupants_of(place(HALL)).collect();

        assert!(occupants.contains(&alice));
        assert!(occupants.contains(&bob));
        assert_eq!(world.occupants_of(place(STUDY)).count(), 0);
    }

    #[test]
    fn inventory_of_lists_held_items_and_is_empty_for_a_stale_handle() {
        let mut world = World::new(tenant(1));
        let chest = world.create().expect("create chest");
        let sword = world.create().expect("create sword");
        world.inventory_add(chest, sword).expect("add must succeed");

        assert_eq!(world.inventory_of(chest).collect::<Vec<_>>(), vec![sword]);

        world.teardown(chest).expect("teardown must succeed");
        assert_eq!(world.inventory_of(chest).count(), 0);
    }

    #[test]
    fn name_entity_then_keywords_of_round_trips() {
        let mut world = World::new(tenant(1));
        let sword = world.create().expect("create sword");

        world
            .name_entity(sword, vec![Keyword::new("sword"), Keyword::new("Rusty")])
            .expect("naming a live entity must succeed");

        assert_eq!(
            world.keywords_of(sword),
            &[Keyword::new("sword"), Keyword::new("rusty")]
        );
    }

    #[test]
    fn keywords_of_is_empty_after_teardown() {
        let mut world = World::new(tenant(1));
        let sword = world.create().expect("create sword");
        world
            .name_entity(sword, vec![Keyword::new("sword")])
            .expect("naming must succeed");

        world.teardown(sword).expect("teardown must succeed");

        assert!(world.keywords_of(sword).is_empty());
    }

    #[test]
    fn clear_location_leaves_the_entity_located_nowhere() {
        let mut world = World::new(tenant(1));
        let sword = world.create().expect("create sword");
        world.move_to(sword, place(HALL)).expect("place sword");

        world.clear_location(sword).expect("clear must succeed");

        assert_eq!(world.location_of(sword), None);
        assert_eq!(world.occupants_of(place(HALL)).count(), 0);
    }

    #[test]
    fn apply_effect_clear_location_unlocates_the_entity() {
        let mut world = World::new(tenant(1));
        let sword = world.create().expect("create sword");
        world.move_to(sword, place(HALL)).expect("place sword");

        let event = world.apply_effect(Effect::ClearLocation { entity: sword });

        assert!(event.is_none());
        assert!(!world.is_located_in(sword, place(HALL)));
    }
}
