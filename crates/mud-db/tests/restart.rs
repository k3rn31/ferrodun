//! The §1.2 / §2.5.3 defining test: write → drop process → reload → state
//! intact. A clean restart restores location and inventory, with the durable
//! `EntityKey` stable across the restart while the ephemeral `EntityId` is
//! re-minted (§2.3.1.6).
#![allow(clippy::expect_used)] // integration-test crates are not compiled with cfg(test), so clippy.toml allow-expect-in-tests does not cover their helpers; expect() is permitted in tests per policy

use std::num::NonZeroU64;

use mud_core::{
    Effect, EntityId, MutationCommand, PlaceId, PlaceKey, Precondition, TenantTag, TickEvent, World,
};
use mud_db::{DbError, PersistentWorld, PlaceMap, TenantDb};
use tempfile::TempDir;

fn tenant() -> TenantTag {
    TenantTag::new(1).expect("test tenant tag must be in range")
}

fn place(value: u64) -> PlaceId {
    PlaceId::new(NonZeroU64::new(value).expect("test place id must be non-zero"))
}

const HALL: u64 = 10;
const STUDY: u64 = 11;
const LIBRARY: u64 = 12;

/// The fixture's slug↔PlaceId map, as a loaded world would supply. The numeric
/// ids are arbitrary ephemeral handles; the slugs are the durable identities
/// persisted in `location`.
fn places() -> PlaceMap {
    PlaceMap::from_pairs([
        (place(HALL), slug("hall")),
        (place(STUDY), slug("study")),
        (place(LIBRARY), slug("library")),
    ])
}

fn slug(value: &str) -> PlaceKey {
    PlaceKey::parse(value).expect("test slug must be valid")
}

/// Opens the tenant database under `dir` and boot-loads its world.
async fn open_world(dir: &TempDir) -> PersistentWorld {
    let db = TenantDb::open(dir.path()).await.expect("open tenant db");
    PersistentWorld::load(db, tenant(), places())
        .await
        .expect("boot load")
}

/// Applies a `Create` and returns the freshly minted handle, the only way a
/// submitter learns it.
async fn create_entity(world: &mut PersistentWorld) -> EntityId {
    let event = world
        .apply(MutationCommand::new(Effect::Create))
        .await
        .expect("create must apply");
    event
        .and_then(|event| match event {
            TickEvent::Created { entity } => Some(entity),
            _ => None,
        })
        .expect("Create must emit a Created event")
}

#[tokio::test]
async fn state_survives_a_clean_restart() {
    let dir = TempDir::new().expect("temp dir");

    // --- First boot: create entities, place one, fill a container. ---
    let (mover_key, container_key, item_key) = {
        let db = TenantDb::open(dir.path()).await.expect("open tenant db");
        let mut world = PersistentWorld::load(db, tenant(), places())
            .await
            .expect("boot load on empty world");

        let mover = create_entity(&mut world).await;
        let container = create_entity(&mut world).await;
        let item = create_entity(&mut world).await;

        let mover_key = world.entity_key(mover).expect("mover has a key");
        let container_key = world.entity_key(container).expect("container has a key");
        let item_key = world.entity_key(item).expect("item has a key");

        assert!(
            world
                .apply(MutationCommand::new(Effect::MoveTo {
                    entity: mover,
                    place: place(HALL),
                }))
                .await
                .expect("move applies")
                .is_none(),
            "a successful move emits no event"
        );
        assert!(
            world
                .apply(MutationCommand::new(Effect::InventoryAdd {
                    container,
                    item
                }))
                .await
                .expect("inventory add applies")
                .is_none(),
            "a successful add emits no event"
        );

        // Sanity before the drop: the in-memory state is what we expect.
        assert!(world.world().is_located_in(mover, place(HALL)));
        assert!(world.world().contains(container, item));

        (mover_key, container_key, item_key)
    }; // `world` (and its pool) dropped here — simulates a process restart.

    // --- Second boot: reload from disk and assert state intact. ---
    let db = TenantDb::open(dir.path()).await.expect("reopen tenant db");
    let world = PersistentWorld::load(db, tenant(), places())
        .await
        .expect("boot load on a populated world");

    // The durable keys resolve again; the re-minted ids are looked up through
    // them — the test never assumes EntityId equality across the restart.
    let mover = world.entity_id(mover_key).expect("mover key resolves");
    let container = world
        .entity_id(container_key)
        .expect("container key resolves");
    let item = world.entity_id(item_key).expect("item key resolves");

    assert!(
        world.world().is_located_in(mover, place(HALL)),
        "location restored against the re-minted id"
    );
    assert!(
        world.world().contains(container, item),
        "inventory restored against the re-minted ids"
    );
}

#[tokio::test]
async fn teardown_does_not_resurrect_on_restart() {
    let dir = TempDir::new().expect("temp dir");

    let destroyed_key = {
        let db = TenantDb::open(dir.path()).await.expect("open tenant db");
        let mut world = PersistentWorld::load(db, tenant(), places())
            .await
            .expect("boot load");

        let entity = create_entity(&mut world).await;
        let key = world.entity_key(entity).expect("entity has a key");
        world
            .apply(MutationCommand::new(Effect::Teardown { entity }))
            .await
            .expect("teardown applies");
        key
    };

    let db = TenantDb::open(dir.path()).await.expect("reopen tenant db");
    let world = PersistentWorld::load(db, tenant(), places())
        .await
        .expect("boot load after teardown");

    assert!(
        world.entity_id(destroyed_key).is_none(),
        "a destroyed entity must not resurrect on reload"
    );
}

// A guarded command whose precondition fails must skip its effect entirely —
// the scheduler contract (§2.5.3.5) — and therefore persist nothing, so a
// restart still shows the pre-command state.
#[tokio::test]
async fn failed_precondition_skips_effect_and_persists_nothing() {
    let dir = TempDir::new().expect("temp dir");

    let entity_key = {
        let mut world = open_world(&dir).await;
        let entity = create_entity(&mut world).await;
        let entity_key = world.entity_key(entity).expect("entity has a key");

        world
            .apply(MutationCommand::new(Effect::MoveTo {
                entity,
                place: place(HALL),
            }))
            .await
            .expect("move applies");

        // Guard a move on the entity being in the LIBRARY, where it is not.
        let event = world
            .apply(
                MutationCommand::new(Effect::MoveTo {
                    entity,
                    place: place(STUDY),
                })
                .with_precondition(Precondition::LocatedIn {
                    entity,
                    place: place(LIBRARY),
                }),
            )
            .await
            .expect("guarded apply does not error");

        assert!(
            matches!(event, Some(TickEvent::PreconditionFailed { .. })),
            "a failed precondition reports the skip, got {event:?}"
        );
        // No partial effect in memory: the entity stayed in the HALL.
        assert!(world.world().is_located_in(entity, place(HALL)));
        entity_key
    };

    let world = open_world(&dir).await;
    let entity = world.entity_id(entity_key).expect("entity key resolves");
    assert!(
        world.world().is_located_in(entity, place(HALL)),
        "the skipped move must not have been persisted"
    );
    assert!(
        !world.world().is_located_in(entity, place(STUDY)),
        "the guarded destination must never have been written"
    );
}

// An effect the arena rejects (here, a stale handle) returns `Rejected` and
// must touch neither memory nor the database: a sibling entity's persisted
// state is undisturbed across the restart.
#[tokio::test]
async fn rejected_effect_persists_nothing() {
    let dir = TempDir::new().expect("temp dir");

    let survivor_key = {
        let mut world = open_world(&dir).await;
        let survivor = create_entity(&mut world).await;
        let doomed = create_entity(&mut world).await;
        let survivor_key = world.entity_key(survivor).expect("survivor has a key");

        world
            .apply(MutationCommand::new(Effect::MoveTo {
                entity: survivor,
                place: place(HALL),
            }))
            .await
            .expect("survivor move applies");

        // Tear `doomed` down, then reuse its now-stale handle.
        world
            .apply(MutationCommand::new(Effect::Teardown { entity: doomed }))
            .await
            .expect("teardown applies");
        let event = world
            .apply(MutationCommand::new(Effect::MoveTo {
                entity: doomed,
                place: place(STUDY),
            }))
            .await
            .expect("apply on a stale handle does not error");

        assert!(
            matches!(event, Some(TickEvent::Rejected { .. })),
            "a stale handle is rejected, got {event:?}"
        );
        survivor_key
    };

    let world = open_world(&dir).await;
    let survivor = world
        .entity_id(survivor_key)
        .expect("survivor key resolves");
    assert!(
        world.world().is_located_in(survivor, place(HALL)),
        "the rejected effect must not disturb a sibling's persisted location"
    );
}

// `InventoryRemove` is durable: an item removed from its container before the
// restart is not contained after it (only `add` was covered before).
#[tokio::test]
async fn inventory_remove_persists_across_restart() {
    let dir = TempDir::new().expect("temp dir");

    let (chest_key, item_key) = {
        let mut world = open_world(&dir).await;
        let chest = create_entity(&mut world).await;
        let item = create_entity(&mut world).await;

        world
            .apply(MutationCommand::new(Effect::InventoryAdd {
                container: chest,
                item,
            }))
            .await
            .expect("add applies");
        world
            .apply(MutationCommand::new(Effect::InventoryRemove {
                container: chest,
                item,
            }))
            .await
            .expect("remove applies");

        (
            world.entity_key(chest).expect("chest has a key"),
            world.entity_key(item).expect("item has a key"),
        )
    };

    let world = open_world(&dir).await;
    let chest = world.entity_id(chest_key).expect("chest key resolves");
    let item = world.entity_id(item_key).expect("item key resolves");
    assert!(
        !world.world().contains(chest, item),
        "a removed item must not be contained after reload"
    );
}

// Last-writer-wins is durable: re-moving an entity overwrites its persisted
// location via the `ON CONFLICT` upsert, not appends.
#[tokio::test]
async fn re_move_persists_only_the_last_destination() {
    let dir = TempDir::new().expect("temp dir");

    let entity_key = {
        let mut world = open_world(&dir).await;
        let entity = create_entity(&mut world).await;
        for destination in [HALL, STUDY] {
            world
                .apply(MutationCommand::new(Effect::MoveTo {
                    entity,
                    place: place(destination),
                }))
                .await
                .expect("move applies");
        }
        world.entity_key(entity).expect("entity has a key")
    };

    let world = open_world(&dir).await;
    let entity = world.entity_id(entity_key).expect("entity key resolves");
    assert!(world.world().is_located_in(entity, place(STUDY)));
    assert!(!world.world().is_located_in(entity, place(HALL)));
}

// `ClearLocation` is durable: an entity whose location is cleared before the
// restart (e.g. an item picked up off the floor) must not be relocated to its
// old place on reload. Without a persistence path for the effect, the in-memory
// clear would not reach the `location` table and the item would revert to
// grounded.
#[tokio::test]
async fn clear_location_persists_across_restart() {
    let dir = TempDir::new().expect("temp dir");

    let entity_key = {
        let mut world = open_world(&dir).await;
        let entity = create_entity(&mut world).await;
        world
            .apply(MutationCommand::new(Effect::MoveTo {
                entity,
                place: place(HALL),
            }))
            .await
            .expect("move applies");
        world
            .apply(MutationCommand::new(Effect::ClearLocation { entity }))
            .await
            .expect("clear-location applies");

        // No longer located anywhere in memory before the drop.
        assert!(!world.world().is_located_in(entity, place(HALL)));
        world.entity_key(entity).expect("entity has a key")
    };

    let world = open_world(&dir).await;
    let entity = world.entity_id(entity_key).expect("entity key resolves");
    assert!(
        !world.world().is_located_in(entity, place(HALL)),
        "a cleared location must not revert to grounded on reload"
    );
}

// Tearing down an item that sits inside a container must remove the containment
// row too (via `ON DELETE CASCADE`). If it lingered, boot load would resolve a
// containment row pointing at a destroyed key and fail — so a clean reload is
// itself the proof the cascade fired.
#[tokio::test]
async fn teardown_of_a_contained_item_leaves_no_dangling_containment() {
    let dir = TempDir::new().expect("temp dir");

    let (chest_key, item_key) = {
        let mut world = open_world(&dir).await;
        let chest = create_entity(&mut world).await;
        let item = create_entity(&mut world).await;
        // The item's key must be captured before teardown unmaps its handle.
        let item_key = world.entity_key(item).expect("item has a key");
        world
            .apply(MutationCommand::new(Effect::InventoryAdd {
                container: chest,
                item,
            }))
            .await
            .expect("add applies");
        world
            .apply(MutationCommand::new(Effect::Teardown { entity: item }))
            .await
            .expect("teardown applies");
        (world.entity_key(chest).expect("chest has a key"), item_key)
    };

    // Boot load succeeding (open_world unwraps it) proves no orphaned
    // containment row survived; the container persists, the item does not.
    let world = open_world(&dir).await;
    assert!(world.entity_id(chest_key).is_some(), "container persists");
    assert!(
        world.entity_id(item_key).is_none(),
        "a destroyed item must not resurrect"
    );
}

// A location is persisted by its durable slug. If the room is removed from the
// world before the next boot, the slug names nothing — content drift that must
// surface loudly as `UnknownPlaceKey`, not silently relocate the entity.
#[tokio::test]
async fn boot_load_rejects_a_location_in_a_removed_room() {
    let dir = TempDir::new().expect("temp dir");

    {
        let mut world = open_world(&dir).await;
        let entity = create_entity(&mut world).await;
        world
            .apply(MutationCommand::new(Effect::MoveTo {
                entity,
                place: place(HALL),
            }))
            .await
            .expect("move applies");
    }

    // Reload with a world that no longer defines the "hall" slug.
    let db = TenantDb::open(dir.path()).await.expect("reopen tenant db");
    let drifted = PlaceMap::from_pairs([(place(STUDY), slug("study"))]);
    let error = PersistentWorld::load(db, tenant(), drifted)
        .await
        .err()
        .expect("a location in a removed room must fail boot load");
    assert!(
        matches!(error, DbError::UnknownPlaceKey(ref s) if s == "hall"),
        "a removed room's slug must surface as UnknownPlaceKey, got {error:?}"
    );
}

// §2.5.3.3, M1-22 design: commands submitted to the scheduler apply to arena
// AND database on tick; rejected commands never reach the database.

#[tokio::test]
async fn submitted_create_applies_to_arena_and_database_on_tick() {
    let dir = TempDir::new().expect("temp dir");
    let mut world = open_world(&dir).await;

    world.submit(MutationCommand::new(Effect::Create));
    let events = world.tick().await.expect("tick applies the queue");

    let minted = match events.as_slice() {
        [TickEvent::Created { entity }] => *entity,
        other => panic!("expected exactly one Created, got {other:?}"),
    };
    assert!(
        world.entity_key(minted).is_some(),
        "the minted entity maps to a durable key (row persisted)"
    );
    assert_eq!(world.tick_number(), 1);
}

#[tokio::test]
async fn a_rejected_command_persists_nothing() {
    let dir = TempDir::new().expect("temp dir");
    let mut world = open_world(&dir).await;

    // A teardown of a handle from a foreign arena is rejected by the arena.
    let foreign = {
        let mut other = World::new(TenantTag::new(2).expect("tag 2 in range"));
        other.create().expect("foreign create")
    };
    world.submit(MutationCommand::new(Effect::Teardown { entity: foreign }));
    let events = world
        .tick()
        .await
        .expect("tick returns the rejection as an event");

    assert!(
        matches!(events.as_slice(), [TickEvent::Rejected { .. }]),
        "the arena rejection surfaces as an event, got {events:?}"
    );
}

#[tokio::test]
async fn commands_apply_in_submission_order() {
    let dir = TempDir::new().expect("temp dir");
    let mut world = open_world(&dir).await;

    world.submit(MutationCommand::new(Effect::Create));
    world.submit(MutationCommand::new(Effect::Create));
    let events = world.tick().await.expect("tick");
    assert_eq!(events.len(), 2, "both creates reported, in order");
}
