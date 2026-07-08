//! Handle-validated movement across a small map, composing the three pieces a
//! caller wires together by hand: the liveness `EntityArena`, the `Place`
//! spatial surface, and the `LocationOf` side-table. This is the composition the
//! `World` aggregate does not cover — it neither exposes `LocationOf` nor uses
//! `Place`.
#![allow(clippy::expect_used)] // integration-test crates are not compiled with cfg(test), so clippy.toml allow-expect-in-tests does not cover their helpers; expect() is permitted in tests per policy

use std::num::NonZeroU64;

use mud_core::{
    ArenaError, Description, Direction, EntityArena, LocationOf, Place, PlaceId, RegionId,
    RoomData, TenantTag,
};

fn place_id(value: u64) -> PlaceId {
    PlaceId::new(NonZeroU64::new(value).expect("test place id must be non-zero"))
}

fn region_id(value: u64) -> RegionId {
    RegionId::new(NonZeroU64::new(value).expect("test region id must be non-zero"))
}

const REGION: u64 = 1;
const HALL: u64 = 10;
const STUDY: u64 = 11;
const LOFT: u64 = 12;
const COURTYARD: u64 = 13;

/// The hall: north to the study, up to the loft, with a visible-but-unreachable
/// courtyard.
fn hall() -> Place {
    Place::Room(
        RoomData::new(
            place_id(HALL),
            region_id(REGION),
            Description::new("A long stone hall."),
        )
        .with_exit(Direction::North, place_id(STUDY))
        .with_exit(Direction::Up, place_id(LOFT))
        .with_visible_places([place_id(COURTYARD)]),
    )
}

fn study() -> Place {
    Place::Room(RoomData::new(
        place_id(STUDY),
        region_id(REGION),
        Description::new("A cramped study."),
    ))
}

#[test]
fn an_entity_walks_north_from_the_hall_to_the_study() {
    let hall = hall();
    let study = study();
    let mut arena = EntityArena::new(TenantTag::new(1).expect("tenant must be in range"));
    let mut locations = LocationOf::new();

    // Validate the handle through the arena before touching the side-table, then
    // drop the entity into the hall.
    let wanderer = arena.alloc().expect("alloc must succeed");
    assert!(arena.resolve(wanderer).is_ok(), "a fresh handle resolves");
    locations.place(wanderer, hall.id());

    assert_eq!(
        hall.occupants(&locations).collect::<Vec<_>>(),
        vec![wanderer]
    );

    // Walk north: the hall's north exit names the study, so move there.
    let north = hall
        .neighbor(Direction::North)
        .expect("the hall has a north exit");
    assert_eq!(north, study.id());
    locations.place(wanderer, north);

    // The reverse index follows the move: the study holds the wanderer, the hall
    // is empty.
    assert_eq!(
        study.occupants(&locations).collect::<Vec<_>>(),
        vec![wanderer]
    );
    assert_eq!(hall.occupants(&locations).count(), 0);
}

#[test]
fn a_visible_place_is_not_an_exit() {
    let hall = hall();

    // The courtyard is observable from the hall but not reachable by any exit.
    assert_eq!(
        hall.visible_places().collect::<Vec<_>>(),
        vec![place_id(COURTYARD)]
    );
    for dir in [
        Direction::North,
        Direction::East,
        Direction::South,
        Direction::West,
        Direction::Up,
        Direction::Down,
    ] {
        assert_ne!(hall.neighbor(dir), Some(place_id(COURTYARD)));
    }
}

#[test]
fn tearing_an_entity_down_clears_its_handle_and_occupancy() {
    let hall = hall();
    let mut arena = EntityArena::new(TenantTag::new(1).expect("tenant must be in range"));
    let mut locations = LocationOf::new();

    let resident = arena.alloc().expect("alloc must succeed");
    locations.place(resident, hall.id());

    // Teardown is two steps a caller composes: free the handle, clear the table.
    arena
        .free(resident)
        .expect("free of a live handle must succeed");
    locations.remove(resident);

    assert_eq!(arena.resolve(resident), Err(ArenaError::StaleHandle));
    assert_eq!(hall.occupants(&locations).count(), 0);
}
