//! End-to-end mutation flow through the public write model: `Scheduler` drives
//! a `World` over several ticks, crossing scheduler → world → arena →
//! side-tables. Black-box: only the crate's public surface is used.
#![allow(clippy::expect_used)] // test helpers; mirrors `allow-expect-in-tests`

use std::num::NonZeroU64;

use mud_core::{
    ArenaError, Effect, EntityId, MutationCommand, PlaceId, Precondition, Scheduler, TenantTag,
    TickEvent, World,
};

fn tenant(value: u16) -> TenantTag {
    TenantTag::new(value).expect("test tenant tag must be in range")
}

fn place(value: u64) -> PlaceId {
    PlaceId::new(NonZeroU64::new(value).expect("test place id must be non-zero"))
}

const HALL: u64 = 10;
const STUDY: u64 = 11;

/// Submits a single `Create` and drains the minted handle out of the tick.
fn create_entity(scheduler: &mut Scheduler, world: &mut World) -> EntityId {
    scheduler.submit(MutationCommand::new(Effect::Create));
    scheduler
        .tick(world)
        .into_iter()
        .find_map(|event| match event {
            TickEvent::Created { entity } => Some(entity),
            _ => None,
        })
        .expect("Create must emit a Created event")
}

#[test]
fn guarded_inventory_move_fails_then_succeeds_across_ticks() {
    let mut scheduler = Scheduler::new();
    let mut world = World::new(tenant(1));
    let chest = create_entity(&mut scheduler, &mut world);
    let sword = create_entity(&mut scheduler, &mut world);

    // Park the sword in the hall.
    scheduler.submit(MutationCommand::new(Effect::MoveTo {
        entity: sword,
        place: place(HALL),
    }));
    assert_eq!(scheduler.tick(&mut world), vec![]);

    // Guard a move on the chest holding the sword — which it does not yet.
    let guarded = || {
        MutationCommand::new(Effect::MoveTo {
            entity: sword,
            place: place(STUDY),
        })
        .with_precondition(Precondition::Contains {
            container: chest,
            item: sword,
        })
    };
    scheduler.submit(guarded());
    assert_eq!(
        scheduler.tick(&mut world),
        vec![TickEvent::PreconditionFailed {
            precondition: Precondition::Contains {
                container: chest,
                item: sword,
            },
            effect: Effect::MoveTo {
                entity: sword,
                place: place(STUDY),
            },
        }]
    );
    // No partial effect: the sword stayed in the hall.
    assert!(world.is_located_in(sword, place(HALL)));

    // Put the sword in the chest, then the same guard holds and the move applies.
    scheduler.submit(MutationCommand::new(Effect::InventoryAdd {
        container: chest,
        item: sword,
    }));
    assert_eq!(scheduler.tick(&mut world), vec![]);
    assert!(world.contains(chest, sword));

    scheduler.submit(guarded());
    assert_eq!(scheduler.tick(&mut world), vec![]);
    assert!(world.is_located_in(sword, place(STUDY)));
}

#[test]
fn teardown_makes_a_later_effect_on_the_handle_stale() {
    let mut scheduler = Scheduler::new();
    let mut world = World::new(tenant(1));
    let entity = create_entity(&mut scheduler, &mut world);
    scheduler.submit(MutationCommand::new(Effect::MoveTo {
        entity,
        place: place(HALL),
    }));
    scheduler.tick(&mut world);

    scheduler.submit(MutationCommand::new(Effect::Teardown { entity }));
    assert_eq!(scheduler.tick(&mut world), vec![]);
    assert!(!world.is_located_in(entity, place(HALL)));

    scheduler.submit(MutationCommand::new(Effect::MoveTo {
        entity,
        place: place(STUDY),
    }));
    assert_eq!(
        scheduler.tick(&mut world),
        vec![TickEvent::Rejected {
            effect: Effect::MoveTo {
                entity,
                place: place(STUDY),
            },
            error: ArenaError::StaleHandle,
        }]
    );
}

#[test]
fn a_handle_from_another_tenant_is_rejected() {
    // Mint a handle in tenant 2's own world/scheduler.
    let mut foreign_scheduler = Scheduler::new();
    let mut foreign_world = World::new(tenant(2));
    let foreign = create_entity(&mut foreign_scheduler, &mut foreign_world);

    // Use it against tenant 1's world: the arena rejects it cross-tenant.
    let mut scheduler = Scheduler::new();
    let mut world = World::new(tenant(1));
    scheduler.submit(MutationCommand::new(Effect::MoveTo {
        entity: foreign,
        place: place(HALL),
    }));

    assert_eq!(
        scheduler.tick(&mut world),
        vec![TickEvent::Rejected {
            effect: Effect::MoveTo {
                entity: foreign,
                place: place(HALL),
            },
            error: ArenaError::CrossTenant {
                arena: tenant(1),
                handle: tenant(2),
            },
        }]
    );
}

#[test]
fn tick_number_advances_once_per_tick() {
    let mut scheduler = Scheduler::new();
    let mut world = World::new(tenant(1));

    assert_eq!(scheduler.tick_number(), 0);
    for expected in 1..=3 {
        scheduler.tick(&mut world);
        assert_eq!(scheduler.tick_number(), expected);
    }
}
