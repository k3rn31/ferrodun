//! Loading authored rooms from KDL (§4.1).
//!
//! Room files live under `<tenant_dir>/world/` and are scanned recursively, so a
//! builder may organize them into subfolders. Each `room` node is keyed by a
//! durable slug ([`PlaceKey`]); the loader assigns each room an ephemeral
//! [`PlaceId`] for the in-memory hot path and resolves exits between slugs.
//!
//! ```kdl
//! room "town_square" {
//!     title "Town Square"
//!     description "A bustling cobbled square."
//!     exit "north" "market"
//! }
//! ```

use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::num::NonZeroU64;
use std::path::{Path, PathBuf};

use kdl::{KdlDocument, KdlNode, KdlValue};
use mud_core::{Description, Direction, Place, PlaceId, PlaceKey, RegionId, RoomData, Title};

use crate::error::WorldError;

/// The registry of rooms loaded from a tenant's world files.
///
/// Maps the ephemeral [`PlaceId`] hot-path handle to its [`Place`], and carries
/// the durable [`PlaceKey`] slug each handle was minted for so persistence can
/// translate between the two.
#[derive(Debug, Clone)]
#[must_use]
pub struct Rooms {
    by_id: HashMap<PlaceId, Place>,
    slug_to_id: HashMap<PlaceKey, PlaceId>,
    id_to_slug: HashMap<PlaceId, PlaceKey>,
}

impl Rooms {
    /// The room with the given handle, if any.
    #[must_use]
    pub fn get(&self, id: PlaceId) -> Option<&Place> {
        self.by_id.get(&id)
    }

    /// The handle a slug names, if a room defines it.
    #[must_use]
    pub fn id_of(&self, slug: &PlaceKey) -> Option<PlaceId> {
        self.slug_to_id.get(slug).copied()
    }

    /// Iterates over the loaded rooms.
    pub fn iter(&self) -> impl Iterator<Item = &Place> {
        self.by_id.values()
    }

    /// The number of loaded rooms.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// Whether no rooms were loaded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    /// The `(PlaceId, PlaceKey)` pairs, for building the persistence place map.
    pub fn place_keys(&self) -> impl Iterator<Item = (PlaceId, PlaceKey)> + '_ {
        self.id_to_slug.iter().map(|(id, slug)| (*id, slug.clone()))
    }
}

/// A room parsed in the first pass, before its exits are resolved to handles.
struct RawRoom {
    slug: PlaceKey,
    id: PlaceId,
    title: Option<Title>,
    description: Description,
    exits: Vec<(Direction, String)>,
}

/// The region every M1 room belongs to; per-region structure arrives later.
fn default_region() -> RegionId {
    RegionId::new(NonZeroU64::MIN)
}

/// Loads every `room` from the KDL files under `world_dir`.
///
/// # Errors
///
/// Returns [`WorldError`] if a file cannot be read or parsed, a slug is invalid
/// or duplicated, an exit names an unknown direction or target room, or a room
/// omits its description.
pub fn load_rooms(world_dir: &Path) -> Result<Rooms, WorldError> {
    let mut files = Vec::new();
    collect_kdl_files(world_dir, &mut files)?;

    let mut raw_rooms = Vec::new();
    let mut slug_to_id = HashMap::new();
    let mut id_to_slug = HashMap::new();
    let mut next_id = NonZeroU64::MIN;

    for path in &files {
        let text = fs::read_to_string(path)?;
        let document = KdlDocument::parse(&text).map_err(|source| WorldError::Kdl {
            path: path.clone(),
            source: Box::new(source),
        })?;

        for node in document.nodes() {
            // `room` is the only node kind a world file may contain in M1. An
            // unrecognized node is rejected loudly so a typo (`rooom`) surfaces
            // here rather than as a confusing missing-room error downstream.
            if node.name().value() != "room" {
                return Err(WorldError::UnexpectedNode {
                    context: "world file".to_owned(),
                    node: node.name().value().to_owned(),
                });
            }
            let room = parse_room(node, PlaceId::new(next_id))?;
            if slug_to_id.contains_key(&room.slug) {
                return Err(WorldError::DuplicateSlug {
                    slug: room.slug.to_string(),
                });
            }
            slug_to_id.insert(room.slug.clone(), room.id);
            id_to_slug.insert(room.id, room.slug.clone());
            next_id = advance(next_id);
            raw_rooms.push(room);
        }
    }

    let by_id = resolve_rooms(raw_rooms, &slug_to_id)?;
    Ok(Rooms {
        by_id,
        slug_to_id,
        id_to_slug,
    })
}

/// Parses one `room` node into a [`RawRoom`], assigning it `id`.
fn parse_room(node: &KdlNode, id: PlaceId) -> Result<RawRoom, WorldError> {
    let slug_text = arg(node, 0).ok_or(WorldError::MissingField {
        node: "room".to_owned(),
        field: "slug",
    })?;
    let slug = PlaceKey::parse(slug_text).map_err(|source| WorldError::InvalidSlug {
        value: slug_text.to_owned(),
        source,
    })?;

    let children = node.children();
    let mut title = None;
    let mut description = None;
    let mut exits = Vec::new();

    for child in children.into_iter().flat_map(KdlDocument::nodes) {
        match child.name().value() {
            "title" => title = arg(child, 0).map(Title::new),
            "description" => description = arg(child, 0).map(Description::new),
            "exit" => exits.push(parse_exit(child)?),
            // Reject unknown children for the same reason as unknown top-level
            // nodes: a misspelled field (`descriptipn`) must fail at the typo,
            // not silently drop the room's description.
            other => {
                return Err(WorldError::UnexpectedNode {
                    context: format!("room {slug}"),
                    node: other.to_owned(),
                });
            }
        }
    }

    let description = description.ok_or_else(|| WorldError::MissingField {
        node: slug.to_string(),
        field: "description",
    })?;

    Ok(RawRoom {
        slug,
        id,
        title,
        description,
        exits,
    })
}

/// Parses an `exit "<direction>" "<target-slug>"` child node.
fn parse_exit(node: &KdlNode) -> Result<(Direction, String), WorldError> {
    let direction = parse_direction(arg(node, 0).ok_or(WorldError::MissingField {
        node: "exit".to_owned(),
        field: "direction",
    })?)?;
    let target = arg(node, 1)
        .ok_or(WorldError::MissingField {
            node: "exit".to_owned(),
            field: "target",
        })?
        .to_owned();
    Ok((direction, target))
}

/// Builds each [`Place`], resolving exit target slugs to handles.
fn resolve_rooms(
    raw_rooms: Vec<RawRoom>,
    slug_to_id: &HashMap<PlaceKey, PlaceId>,
) -> Result<HashMap<PlaceId, Place>, WorldError> {
    let mut by_id = HashMap::new();
    for raw in raw_rooms {
        let mut data = RoomData::new(raw.id, default_region(), raw.description);
        if let Some(title) = raw.title {
            data = data.with_title(title);
        }
        for (direction, target_slug) in raw.exits {
            let target = resolve_exit_target(&raw.slug, direction, &target_slug, slug_to_id)?;
            data = data.with_exit(direction, target);
        }
        by_id.insert(raw.id, Place::Room(data));
    }
    Ok(by_id)
}

/// Resolves one exit's target slug to a handle, or reports a dangling exit.
fn resolve_exit_target(
    from: &PlaceKey,
    direction: Direction,
    target_slug: &str,
    slug_to_id: &HashMap<PlaceKey, PlaceId>,
) -> Result<PlaceId, WorldError> {
    let slug = PlaceKey::parse(target_slug).map_err(|source| WorldError::InvalidSlug {
        value: target_slug.to_owned(),
        source,
    })?;
    slug_to_id
        .get(&slug)
        .copied()
        .ok_or_else(|| WorldError::DanglingExit {
            from: from.to_string(),
            direction: direction_name(direction).to_owned(),
            to: target_slug.to_owned(),
        })
}

/// The first positional string argument of `node` at `index`, if present.
fn arg(node: &KdlNode, index: usize) -> Option<&str> {
    node.get(index).and_then(KdlValue::as_string)
}

/// Maps an authored direction word to a [`Direction`].
fn parse_direction(value: &str) -> Result<Direction, WorldError> {
    match value {
        "north" => Ok(Direction::North),
        "east" => Ok(Direction::East),
        "south" => Ok(Direction::South),
        "west" => Ok(Direction::West),
        "up" => Ok(Direction::Up),
        "down" => Ok(Direction::Down),
        other => Err(WorldError::UnknownDirection {
            value: other.to_owned(),
        }),
    }
}

/// The authored word for a direction (for error messages).
fn direction_name(direction: Direction) -> &'static str {
    match direction {
        Direction::North => "north",
        Direction::East => "east",
        Direction::South => "south",
        Direction::West => "west",
        Direction::Up => "up",
        Direction::Down => "down",
    }
}

/// Advances the monotonic id counter.
fn advance(id: NonZeroU64) -> NonZeroU64 {
    // INVARIANT: room ids count up from 1; overflowing u64 would require ~1.8e19
    // rooms, unrepresentable on disk. checked_add bounds it without a silent wrap;
    // the saturating fallback keeps the counter valid in the impossible case.
    id.checked_add(1).unwrap_or(NonZeroU64::MAX)
}

/// Recursively collects `*.kdl` files under `dir`, sorted for deterministic order.
fn collect_kdl_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), WorldError> {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<_, _>>()?;
    entries.sort();

    for path in entries {
        if path.is_dir() {
            collect_kdl_files(&path, out)?;
        } else if path.extension().and_then(OsStr::to_str) == Some("kdl") {
            out.push(path);
        }
    }
    Ok(())
}
