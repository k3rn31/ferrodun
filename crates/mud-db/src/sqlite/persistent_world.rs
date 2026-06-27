//! Write-through persistence and boot load for a single tenant's world
//! (SPEC §1.2, §2.3.1.4–2.3.1.6, §2.5.3).
//!
//! [`PersistentWorld`] joins the in-memory [`World`] (a `mud-core`
//! `EntityId`/slot model) to the durable per-tenant database. The database is
//! the source of truth (§2.5.3.1); the arena is a **cache keyed by `EntityKey`**
//! (§2.3.1.6), realized here as a one-to-one `EntityKey`↔`EntityId` map in front
//! of the `EntityId`-based `World`. Loading an entity mints a *fresh* `EntityId`
//! for its durable `EntityKey`, so `EntityId` values are never expected to
//! survive a restart while `EntityKey` is stable across one.
//!
//! ## Consistency model (§2.5.3.3)
//!
//! Every mutation flows through a [`MutationCommand`] and applies to the arena
//! and the database. An in-memory structure cannot enlist in a SQL transaction,
//! so the spec's "same transaction" is realized as: validate and apply
//! in-memory, then commit the database write immediately. The database is
//! authoritative on restart, so a lost commit merely means the mutation did not
//! durably happen and the volatile in-memory state is discarded on the next
//! [`load`](PersistentWorld::load) — the two cannot *durably* diverge.
//!
//! There is a transient in-process **inconsistency window**: when a non-`Create`
//! effect applies in-memory but its database write then fails, memory is briefly
//! ahead of the database and [`apply`](PersistentWorld::apply) returns
//! [`DbError`]. Rolling memory back, crash-on-failure, and the background
//! snapshot (§2.5.3.4) are later hardening; for now `DbError` propagates to the
//! caller.
//!
//! Per effect:
//! - [`Effect::Create`] is database-first — the `EntityKey` comes from
//!   `AUTOINCREMENT`. The row is inserted, then the arena handle minted; on the
//!   astronomically unlikely arena exhaustion the row is rolled back.
//! - All other effects apply in-memory first, so the arena's precise
//!   [`ArenaError`] classification (cross-tenant vs. stale handle) is preserved;
//!   only on success is the database written.
//!
//! ## Teardown is destruction, not eviction
//!
//! [`Effect::Teardown`] *destroys* an entity: deleting its `entities` row
//! cascades (`ON DELETE CASCADE`, see the migration) to every dependent row —
//! its location, its containment as an item, and any items it held as a
//! container — so it does not resurrect on reload (a destroyed entity must stay
//! destroyed, §2.5.3.1). `EntityKey` non-reuse still holds via `AUTOINCREMENT`
//! (§2.3.1.5). Cache eviction (drop the arena handle, keep the row, §2.5.3.2) is
//! a distinct, out-of-scope concept.
//!
//! One memory-vs-database asymmetry follows from keeping `mud-core` untouched:
//! `World::teardown` clears the entity's *own* location and contents but cannot
//! remove it from a container that holds it *as an item* (the in-memory
//! inventory has no reverse item→container index). The database cascade does
//! remove that containment row, so the arena briefly reports the destroyed item
//! as still contained while the database does not. The stale handle makes the
//! entry unobservable through any live id, and the next [`load`] rebuilds memory
//! from the (correct) database, so the two reconcile on reload.

use std::collections::HashMap;

use mud_core::{
    Effect, EntityId, EntityKey, MutationCommand, PlaceId, TenantTag, TickEvent, World,
};

use crate::error::DbError;
use crate::sqlite::TenantDb;
use crate::sqlite::keys::{
    entity_key_from_db, entity_key_to_db, place_id_from_db, place_id_to_db, resolve_loaded,
};

/// A tenant's in-memory world backed by write-through persistence.
///
/// Owns the durable database, the in-memory [`World`] cache, and the
/// one-to-one `EntityKey`↔`EntityId` mapping (§2.3.1.6). Construct it with
/// [`load`](PersistentWorld::load), which rebuilds the world from the database;
/// mutate it through [`apply`](PersistentWorld::apply).
#[must_use]
pub struct PersistentWorld {
    db: TenantDb,
    world: World,
    by_key: HashMap<EntityKey, EntityId>,
    // Keyed by the full `EntityId`: a stale handle to a reused slot carries an
    // older generation and so misses, never resolving to the new occupant.
    by_id: HashMap<EntityId, EntityKey>,
}

impl PersistentWorld {
    /// Rebuilds a tenant's world from its database (the boot load).
    ///
    /// Mints a fresh [`EntityId`] for every persisted [`EntityKey`] (§2.3.1.6)
    /// and replays the location and inventory tables so a clean restart restores
    /// where every entity is and what every container holds.
    ///
    /// # Errors
    ///
    /// Returns [`DbError`] on a query failure, an out-of-range persisted id, a
    /// dangling reference (a `location`/`inventory` row pointing at an absent
    /// entity — foreign keys make this unreachable in a consistent file), or
    /// arena exhaustion while minting handles.
    pub async fn load(db: TenantDb, tenant: TenantTag) -> Result<Self, DbError> {
        let mut world = World::new(tenant);
        let mut by_key = HashMap::new();
        let mut by_id = HashMap::new();

        let keys =
            sqlx::query!(r#"SELECT entity_key AS "entity_key!" FROM entities ORDER BY entity_key"#)
                .fetch_all(db.pool())
                .await?;
        for row in keys {
            let key = entity_key_from_db(row.entity_key)?;
            let id = world
                .create()
                .map_err(|_| DbError::DanglingReference(row.entity_key))?;
            by_key.insert(key, id);
            by_id.insert(id, key);
        }

        let locations = sqlx::query!(
            r#"SELECT entity_key AS "entity_key!", place_id AS "place_id!" FROM location"#
        )
        .fetch_all(db.pool())
        .await?;
        for row in locations {
            let id = resolve_loaded(&by_key, row.entity_key)?;
            let place = place_id_from_db(row.place_id)?;
            world
                .move_to(id, place)
                .map_err(|_| DbError::DanglingReference(row.entity_key))?;
        }

        let inventory = sqlx::query!(
            r#"SELECT item_key AS "item_key!", container_key AS "container_key!" FROM inventory"#
        )
        .fetch_all(db.pool())
        .await?;
        for row in inventory {
            let item = resolve_loaded(&by_key, row.item_key)?;
            let container = resolve_loaded(&by_key, row.container_key)?;
            world
                .inventory_add(container, item)
                .map_err(|_| DbError::DanglingReference(row.item_key))?;
        }

        Ok(Self {
            db,
            world,
            by_key,
            by_id,
        })
    }

    /// Applies one [`MutationCommand`] to the arena and the database.
    ///
    /// The in-memory mutation, precondition check, and arena-error → event
    /// classification are delegated to `mud-core`'s single source of truth
    /// (`World::satisfies` and `World::apply_effect`); this method layers the
    /// durable write per effect. A successful non-`Create` effect yields
    /// `Ok(None)`; a `Create` yields [`TickEvent::Created`]; a failed
    /// precondition or arena rejection yields the corresponding [`TickEvent`] in
    /// `Ok(Some(..))`. [`DbError`] is reserved for genuine database failures.
    ///
    /// # Errors
    ///
    /// Returns [`DbError`] if a database write fails, or if a persisted id is out
    /// of range.
    pub async fn apply(&mut self, command: MutationCommand) -> Result<Option<TickEvent>, DbError> {
        let effect = command.effect();
        if let Some(precondition) = command.precondition()
            && !self.world.satisfies(precondition)
        {
            return Ok(Some(TickEvent::PreconditionFailed {
                precondition,
                effect,
            }));
        }

        // `Create` is the one database-first effect: the row's `AUTOINCREMENT` key
        // is the entity's durable identity, so the row must exist before the arena
        // handle is minted. See `apply_create`.
        if let Effect::Create = effect {
            return self.apply_create(effect).await;
        }

        // Every other effect is memory-first: apply it through `mud-core`'s single
        // source of truth, then persist only once the arena has accepted it. A
        // rejection (stale/foreign handle, exhaustion) is observable and never
        // reaches the database.
        if let Some(event) = self.world.apply_effect(effect) {
            return Ok(Some(event));
        }

        match effect {
            Effect::MoveTo { entity, place } => self.persist_move(entity, place).await,
            Effect::InventoryAdd { container, item } => {
                self.persist_inventory_add(container, item).await
            }
            Effect::InventoryRemove { container, item } => {
                self.persist_inventory_remove(container, item).await
            }
            Effect::Teardown { entity } => self.persist_teardown(entity).await,
            // `Create` is handled above; `Effect` is `#[non_exhaustive]`, so a
            // future variant with no persistence path here is rejected rather than
            // silently dropped.
            _ => Err(DbError::UnsupportedEffect),
        }
    }

    /// The current `EntityId` mapped to `key`, if the entity is resident.
    #[must_use]
    pub fn entity_id(&self, key: EntityKey) -> Option<EntityId> {
        self.by_key.get(&key).copied()
    }

    /// The durable `EntityKey` of a resident `id`, if it maps to one.
    #[must_use]
    pub fn entity_key(&self, id: EntityId) -> Option<EntityKey> {
        self.by_id.get(&id).copied()
    }

    /// The in-memory world, for read predicates against re-minted handles.
    pub fn world(&self) -> &World {
        &self.world
    }

    /// `Create` is database-first: the row's `AUTOINCREMENT` key is the durable
    /// identity. Mint the arena handle only after the row exists; roll the row
    /// back if the arena is exhausted.
    async fn apply_create(&mut self, effect: Effect) -> Result<Option<TickEvent>, DbError> {
        let mut tx = self.db.pool().begin().await?;
        let row = sqlx::query!(
            r#"INSERT INTO entities DEFAULT VALUES RETURNING entity_key AS "entity_key!""#
        )
        .fetch_one(&mut *tx)
        .await?;
        let key = entity_key_from_db(row.entity_key)?;

        match self.world.create() {
            Ok(id) => {
                tx.commit().await?;
                self.by_key.insert(key, id);
                self.by_id.insert(id, key);
                Ok(Some(TickEvent::Created { entity: id }))
            }
            Err(error) => {
                tx.rollback().await?;
                Ok(Some(TickEvent::Rejected { effect, error }))
            }
        }
    }

    /// Persists a move already applied in memory: upsert the entity's location.
    async fn persist_move(
        &mut self,
        entity: EntityId,
        place: PlaceId,
    ) -> Result<Option<TickEvent>, DbError> {
        let entity_key = entity_key_to_db(self.key_of(entity)?)?;
        let place_id = place_id_to_db(place)?;
        sqlx::query!(
            "INSERT INTO location (entity_key, place_id) VALUES (?, ?) \
             ON CONFLICT(entity_key) DO UPDATE SET place_id = excluded.place_id",
            entity_key,
            place_id
        )
        .execute(self.db.pool())
        .await?;
        Ok(None)
    }

    /// Persists an inventory add already applied in memory: upsert containment.
    async fn persist_inventory_add(
        &mut self,
        container: EntityId,
        item: EntityId,
    ) -> Result<Option<TickEvent>, DbError> {
        let item_key = entity_key_to_db(self.key_of(item)?)?;
        let container_key = entity_key_to_db(self.key_of(container)?)?;
        sqlx::query!(
            "INSERT INTO inventory (item_key, container_key) VALUES (?, ?) \
             ON CONFLICT(item_key) DO UPDATE SET container_key = excluded.container_key",
            item_key,
            container_key
        )
        .execute(self.db.pool())
        .await?;
        Ok(None)
    }

    /// Persists an inventory remove already applied in memory: delete containment.
    async fn persist_inventory_remove(
        &mut self,
        container: EntityId,
        item: EntityId,
    ) -> Result<Option<TickEvent>, DbError> {
        let item_key = entity_key_to_db(self.key_of(item)?)?;
        let container_key = entity_key_to_db(self.key_of(container)?)?;
        sqlx::query!(
            "DELETE FROM inventory WHERE item_key = ? AND container_key = ?",
            item_key,
            container_key
        )
        .execute(self.db.pool())
        .await?;
        Ok(None)
    }

    /// Persists a teardown already applied in memory: delete the entity's
    /// `entities` row. The schema's `ON DELETE CASCADE` removes every dependent
    /// row (location, containment, items held) in the same statement, so the
    /// entity cannot resurrect on reload and the destroy path needs no knowledge
    /// of which tables reference it.
    async fn persist_teardown(&mut self, entity: EntityId) -> Result<Option<TickEvent>, DbError> {
        let key = self.key_of(entity)?;
        let entity_key = entity_key_to_db(key)?;

        sqlx::query!("DELETE FROM entities WHERE entity_key = ?", entity_key)
            .execute(self.db.pool())
            .await?;

        self.by_key.remove(&key);
        self.by_id.remove(&entity);
        Ok(None)
    }

    /// The durable `EntityKey` of a resident `id`. A live handle that just passed
    /// an arena op is always in the map, so an absent entry is an internal
    /// inconsistency surfaced as [`DbError::EntityNotMapped`] rather than a panic.
    fn key_of(&self, id: EntityId) -> Result<EntityKey, DbError> {
        self.by_id.get(&id).copied().ok_or(DbError::EntityNotMapped)
    }
}
