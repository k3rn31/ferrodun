//! The `Place` enum and its room content (§2.2).
//!
//! [`Place`] exposes the §2.2.2 surface — `id`, `region`, `title`, `describe`,
//! `neighbor`, `occupants`, `visible_places` — as inherent methods that `match`
//! on the variant and delegate to the variant's data ([`RoomData`]). Dispatch is
//! therefore static by construction (an enum, never a trait object), so per-tick
//! hot paths pay no virtual-call cost (§2.2.5). The only variant in M1 is
//! [`Room`](Place::Room); a `PlaceView` trait would have a single implementor and
//! buy nothing, so the surface stays inherent until a second variant earns it.

use super::id::PlaceId;
use crate::{EntityId, LocationOf, RegionId};

/// A direction an exit can lead in (§2.2.2).
///
/// The cardinal four follow the §3.2.2.0 fixed map (`x` increases east, `y`
/// increases north). `Up`/`Down` are vertical exits — stairs, ladders, shafts —
/// modeled as explicit exits rather than a `z` coordinate (§3.2.2.0).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    North,
    East,
    South,
    West,
    Up,
    Down,
}

/// A rendered description of a [`Place`] as seen by a viewer (§2.2.2).
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub struct Description(String);

impl Description {
    /// Wraps rendered description text.
    pub fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    /// Returns the description text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A [`Place`]'s short name (§2.2.2): a label like "Town Square", distinct from
/// the prose [`Description`]. A separate type so a title cannot be passed where a
/// description is meant, or the reverse.
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub struct Title(String);

impl Title {
    /// Wraps title text.
    pub fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    /// Returns the title text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Authored static content of a [`Room`](Place::Room): its identity, optional
/// title, description, exits (one optional neighbour per [`Direction`]), and its
/// visibility set.
///
/// Storing exits as one field per direction makes a duplicate-direction exit
/// unrepresentable and keeps [`neighbor`](Place::neighbor) a plain `match`.
#[derive(Debug, Clone)]
#[must_use]
pub struct RoomData {
    id: PlaceId,
    region: RegionId,
    title: Option<Title>,
    description: Description,
    north: Option<PlaceId>,
    east: Option<PlaceId>,
    south: Option<PlaceId>,
    west: Option<PlaceId>,
    up: Option<PlaceId>,
    down: Option<PlaceId>,
    visible: Vec<PlaceId>,
}

impl RoomData {
    /// Creates a room with the given identity and description, no exits, and an
    /// empty visibility set. Wire exits and visibility with [`with_exit`] and
    /// [`with_visible_places`].
    ///
    /// [`with_exit`]: RoomData::with_exit
    /// [`with_visible_places`]: RoomData::with_visible_places
    pub fn new(id: PlaceId, region: RegionId, description: Description) -> Self {
        Self {
            id,
            region,
            title: None,
            description,
            north: None,
            east: None,
            south: None,
            west: None,
            up: None,
            down: None,
            visible: Vec::new(),
        }
    }

    /// Sets the exit in `dir` to `to`, replacing any existing exit that way.
    pub fn with_exit(mut self, dir: Direction, to: PlaceId) -> Self {
        match dir {
            Direction::North => self.north = Some(to),
            Direction::East => self.east = Some(to),
            Direction::South => self.south = Some(to),
            Direction::West => self.west = Some(to),
            Direction::Up => self.up = Some(to),
            Direction::Down => self.down = Some(to),
        }
        self
    }

    /// Replaces the visibility set with `places`.
    pub fn with_visible_places(mut self, places: impl IntoIterator<Item = PlaceId>) -> Self {
        self.visible = places.into_iter().collect();
        self
    }

    /// Sets the room's short title, replacing any previously set title.
    pub fn with_title(mut self, title: Title) -> Self {
        self.title = Some(title);
        self
    }

    // The `Room` variant's answers to the `Place` surface. `Place` matches the
    // variant and delegates here, keeping each variant's logic with its data.

    fn id(&self) -> PlaceId {
        self.id
    }

    fn title(&self) -> Option<&Title> {
        self.title.as_ref()
    }

    fn region(&self) -> RegionId {
        self.region
    }

    fn describe(&self, _viewer: EntityId) -> Description {
        self.description.clone()
    }

    fn neighbor(&self, dir: Direction) -> Option<PlaceId> {
        match dir {
            Direction::North => self.north,
            Direction::East => self.east,
            Direction::South => self.south,
            Direction::West => self.west,
            Direction::Up => self.up,
            Direction::Down => self.down,
        }
    }

    fn visible_places(&self) -> impl Iterator<Item = PlaceId> {
        self.visible.iter().copied()
    }
}

/// A spatial location (§2.2.1). The only variant is [`Room`](Place::Room).
///
/// `Place` exposes the §2.2.2 surface as inherent methods that `match` on the
/// variant — static dispatch with no trait object (§2.2.5).
#[derive(Debug, Clone)]
#[must_use]
pub enum Place {
    Room(RoomData),
}

impl Place {
    /// This Place's stable identifier (§2.2.2).
    pub fn id(&self) -> PlaceId {
        match self {
            Place::Room(room) => room.id(),
        }
    }

    /// The region this Place belongs to (§2.2.2).
    pub fn region(&self) -> RegionId {
        match self {
            Place::Room(room) => room.region(),
        }
    }

    /// This Place's short title, or `None` if it was authored without one.
    pub fn title(&self) -> Option<&Title> {
        match self {
            Place::Room(room) => room.title(),
        }
    }

    /// This Place's description as seen by `viewer` (§2.2.2).
    ///
    /// Viewer-conditional rendering (invisibility, lighting, language) is not
    /// yet applied: the same `Description` is returned regardless of `viewer`.
    /// The parameter reserves the signature for that behaviour.
    pub fn describe(&self, viewer: EntityId) -> Description {
        match self {
            Place::Room(room) => room.describe(viewer),
        }
    }

    /// The Place reached by leaving in `dir`, or `None` if there is no exit.
    pub fn neighbor(&self, dir: Direction) -> Option<PlaceId> {
        match self {
            Place::Room(room) => room.neighbor(dir),
        }
    }

    /// The entities currently in this Place (§2.2.2), resolved through the
    /// [`LocationOf`] reverse index.
    ///
    /// A Place does not own its occupancy — the dense [`LocationOf`] table is
    /// the authority (§2.3.2.2) — so callers pass it in. (The §2.2.2 sketch's
    /// bare `occupants(&self)` is illustrative; this table join is the honest
    /// signature.)
    pub fn occupants<'a>(&self, locations: &'a LocationOf) -> impl Iterator<Item = EntityId> + 'a {
        locations.occupants(self.id())
    }

    /// The Places observable from here (§2.2.2). Distinct from exits: a Place
    /// may be visible without being directly reachable.
    pub fn visible_places(&self) -> impl Iterator<Item = PlaceId> {
        // A single variant unifies the return type trivially; multiple variants
        // would need an enum/`Either` iterator to unify the arms.
        match self {
            Place::Room(room) => room.visible_places(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;

    use super::*;

    fn place_id(value: u64) -> PlaceId {
        PlaceId::new(NonZeroU64::new(value).expect("test place id must be non-zero"))
    }

    fn region_id(value: u64) -> RegionId {
        RegionId::new(NonZeroU64::new(value).expect("test region id must be non-zero"))
    }

    fn entity_id(value: u64) -> EntityId {
        EntityId::from_bits(value)
    }

    // A small fixture: a hall with a north exit to a study and an up exit to a
    // loft, able to see a courtyard it cannot directly reach. Built as the
    // `Place` a caller would actually hold.
    const REGION: u64 = 1;
    const HALL: u64 = 10;
    const STUDY: u64 = 11;
    const LOFT: u64 = 12;
    const COURTYARD: u64 = 13;

    fn hall() -> Place {
        Place::Room(
            RoomData::new(
                place_id(HALL),
                region_id(REGION),
                Description::new("A long stone hall."),
            )
            .with_exit(Direction::North, place_id(STUDY))
            .with_exit(Direction::Up, place_id(LOFT))
            .with_visible_places([place_id(STUDY), place_id(COURTYARD)]),
        )
    }

    #[test]
    fn neighbor_returns_wired_exits() {
        let hall = hall();

        assert_eq!(hall.neighbor(Direction::North), Some(place_id(STUDY)));
        assert_eq!(hall.neighbor(Direction::Up), Some(place_id(LOFT)));
    }

    #[test]
    fn neighbor_is_none_for_unwired_directions() {
        let hall = hall();

        assert_eq!(hall.neighbor(Direction::East), None);
        assert_eq!(hall.neighbor(Direction::South), None);
        assert_eq!(hall.neighbor(Direction::West), None);
        assert_eq!(hall.neighbor(Direction::Down), None);
    }

    #[test]
    fn with_exit_replaces_a_previously_wired_exit() {
        let room = Place::Room(
            RoomData::new(
                place_id(HALL),
                region_id(REGION),
                Description::new("A long stone hall."),
            )
            .with_exit(Direction::North, place_id(STUDY))
            .with_exit(Direction::North, place_id(LOFT)),
        );

        assert_eq!(room.neighbor(Direction::North), Some(place_id(LOFT)));
    }

    #[test]
    fn visible_places_yields_the_authored_set() {
        let hall = hall();

        let visible: Vec<PlaceId> = hall.visible_places().collect();

        assert_eq!(visible, vec![place_id(STUDY), place_id(COURTYARD)]);
    }

    #[test]
    fn describe_returns_the_authored_text_independently_of_viewer() {
        let hall = hall();

        let to_alice = hall.describe(entity_id(1));
        let to_bob = hall.describe(entity_id(2));

        assert_eq!(to_alice.as_str(), "A long stone hall.");
        assert_eq!(to_alice, to_bob);
    }

    #[test]
    fn id_and_region_return_the_constructed_values() {
        let hall = hall();

        assert_eq!(hall.id(), place_id(HALL));
        assert_eq!(hall.region(), region_id(REGION));
    }

    // The §2.2.2 occupancy surface is a join against LocationOf: entities placed
    // at the room's id show up through Place::occupants, and a room with no one
    // in it is empty.
    #[test]
    fn occupants_joins_through_the_location_table() {
        let hall = hall();
        let goblin = entity_id(3);
        let mut locations = LocationOf::new();
        locations.place(goblin, hall.id());

        assert_eq!(hall.occupants(&locations).collect::<Vec<_>>(), vec![goblin]);

        let empty = LocationOf::new();
        assert_eq!(hall.occupants(&empty).count(), 0);
    }

    // A room is authored without a title by default; with_title sets it, and the
    // Place surface delegates to the variant.
    #[test]
    fn title_is_absent_by_default_and_set_by_with_title() {
        let untitled = hall();
        assert_eq!(untitled.title(), None);

        let titled = Place::Room(
            RoomData::new(
                place_id(HALL),
                region_id(REGION),
                Description::new("A long stone hall."),
            )
            .with_title(Title::new("Great Hall")),
        );
        assert_eq!(titled.title().map(Title::as_str), Some("Great Hall"));
    }

    #[test]
    fn with_title_replaces_a_previously_set_title() {
        let room = Place::Room(
            RoomData::new(
                place_id(HALL),
                region_id(REGION),
                Description::new("A long stone hall."),
            )
            .with_title(Title::new("Old Name"))
            .with_title(Title::new("New Name")),
        );
        assert_eq!(room.title().map(Title::as_str), Some("New Name"));
    }
}
