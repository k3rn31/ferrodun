//! The per-tenant mutable world aggregate (M1 subset).
//!
//! [`World`] bundles the three things any M1 mutation touches — the liveness
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

use crate::{ArenaError, EntityArena, EntityId, Inventory, LocationOf, PlaceId, TenantTag};

/// The mutable world for one tenant: liveness arena plus the M1 hot side-tables.
///
/// Construct one `World` per tenant. All mutation flows through its semantic
/// operations so liveness is validated before any side-table is touched.
#[must_use]
pub struct World {
    arena: EntityArena,
    locations: LocationOf,
    inventory: Inventory,
}

impl World {
    /// Creates an empty world owned by `tenant`.
    pub fn new(tenant: TenantTag) -> Self {
        Self {
            arena: EntityArena::new(tenant),
            locations: LocationOf::new(),
            inventory: Inventory::new(),
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
    /// slots (location and inventory contents).
    ///
    /// Freeing validates tenant ownership and liveness, so a stale or foreign
    /// handle is rejected before any table is touched (§2.3.7.3). Returns the
    /// arena error in that case. Clearing the entity's own location and
    /// inventory keeps the slot clean for reuse (§2.3.7.3); since the side-tables
    /// key on slot, an uncleared slot would otherwise leak state into its next
    /// tenant. Removing the entity from any container that *holds it as an item*
    /// is out of M1 scope (the inventory table has no reverse item→container
    /// index yet).
    pub fn teardown(&mut self, entity: EntityId) -> Result<(), ArenaError> {
        self.arena.free(entity)?;
        self.locations.remove(entity);
        self.inventory.clear(entity);
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
}
