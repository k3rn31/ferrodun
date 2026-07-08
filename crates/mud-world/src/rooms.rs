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

use kdl::{KdlDocument, KdlNode};
use mud_core::{
    Description, Direction, FieldStyle, Palette, Place, PlaceId, PlaceKey, RegionId, RoomData,
    StyledText, Title, compile_markup,
};

use crate::error::WorldError;
use crate::kdl::arg;
use crate::regions::{REGION_MANIFEST, RegionBinder, Regions};

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
    region: RegionId,
    title: Option<Title>,
    description: Description,
    exits: Vec<(Direction, String)>,
}

/// Loads every `room` from the KDL files under `world_dir`, binding each to its
/// region, and the [`Regions`] declared by the `region.kdl` manifests.
///
/// # Errors
///
/// Returns [`WorldError`] if a file cannot be read or parsed, a slug is invalid
/// or duplicated, an exit names an unknown direction or target room, a room omits
/// its description, or a region manifest is malformed (§2.2.7).
pub fn load_rooms(world_dir: &Path, palette: &Palette) -> Result<(Rooms, Regions), WorldError> {
    let mut files = Vec::new();
    collect_kdl_files(world_dir, &mut files)?;

    // A `region.kdl` is a region manifest (§2.2.7.3), never a room file; the rest
    // are scanned for rooms.
    let (manifest_files, room_files): (Vec<PathBuf>, Vec<PathBuf>) = files
        .into_iter()
        .partition(|path| path.file_name().and_then(OsStr::to_str) == Some(REGION_MANIFEST));
    let binder = RegionBinder::load(&manifest_files, world_dir)?;

    let mut raw_rooms = Vec::new();
    let mut slug_to_id = HashMap::new();
    let mut id_to_slug = HashMap::new();
    let mut next_id = NonZeroU64::MIN;

    for path in &room_files {
        // Every room in a file shares the region of the file's enclosing folder.
        // A room covered by no manifest is rejected (§2.2.7.3): there is no
        // implicit fallback region.
        let region = binder
            .region_for(path.parent().unwrap_or(world_dir))
            .ok_or_else(|| WorldError::RoomOutsideRegion { path: path.clone() })?;
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
            let room = parse_room(node, PlaceId::new(next_id), region, palette)?;
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
    Ok((
        Rooms {
            by_id,
            slug_to_id,
            id_to_slug,
        },
        binder.into_regions(),
    ))
}

/// Parses one `room` node into a [`RawRoom`], assigning it `id`, binding it to
/// `region`, and compiling its styled fields through `palette` (§3.20.2).
fn parse_room(
    node: &KdlNode,
    id: PlaceId,
    region: RegionId,
    palette: &Palette,
) -> Result<RawRoom, WorldError> {
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
            "title" => {
                title = arg(child, 0).map(|raw| {
                    Title::from(compile_field(
                        raw,
                        &FieldStyle::TITLE,
                        palette,
                        slug_text,
                        "title",
                    ))
                });
            }
            "description" => {
                description = arg(child, 0).map(|raw| {
                    Description::from(compile_field(
                        raw,
                        &FieldStyle::DESCRIPTION,
                        palette,
                        slug_text,
                        "description",
                    ))
                });
            }
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
        region,
        title,
        description,
        exits,
    })
}

/// Parses an `exit "<direction>" "<target-slug>"` child node.
fn parse_exit(node: &KdlNode) -> Result<(Direction, String), WorldError> {
    let word = arg(node, 0).ok_or(WorldError::MissingField {
        node: "exit".to_owned(),
        field: "direction",
    })?;
    let direction = word
        .parse::<Direction>()
        .map_err(|error| WorldError::UnknownDirection {
            value: error.value().to_owned(),
        })?;
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
        let mut data = RoomData::new(raw.id, raw.region, raw.description);
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
            direction: direction.name().to_owned(),
            to: target_slug.to_owned(),
        })
}

/// Compiles one authored field's markup under `field`, resolving palette colors,
/// and logs every degraded tag as a structured warning (§3.20.2.2). Builder
/// markup never fails the load; a bad tag keeps its inner text.
fn compile_field(
    raw: &str,
    field: &FieldStyle,
    palette: &Palette,
    room: &str,
    field_name: &'static str,
) -> StyledText {
    let compiled = compile_markup(raw, field, palette);
    for diagnostic in &compiled.diagnostics {
        tracing::warn!(room, field = field_name, %diagnostic, "markup diagnostic");
    }
    compiled.text
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

#[cfg(test)]
mod tests {

    use tempfile::TempDir;

    use super::*;

    /// Writes `(relative_path, contents)` files under a single region subfolder of
    /// a fresh world directory and loads the rooms, dropping the region registry
    /// these room-focused tests ignore. The manifest satisfies the mandatory-region
    /// rule (§2.2.7.3) so the cases here exercise room parsing, not region binding.
    fn load(files: &[(&str, &str)]) -> Result<Rooms, WorldError> {
        let dir = TempDir::new().expect("temp dir");
        let region_dir = dir.path().join("zone");
        fs::create_dir_all(&region_dir).expect("create region dir");
        fs::write(region_dir.join(REGION_MANIFEST), "region \"zone\"")
            .expect("write region manifest");
        for (relative, contents) in files {
            let path = region_dir.join(relative);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("create parent dirs");
            }
            fs::write(&path, contents).expect("write room file");
        }
        load_rooms(dir.path(), &Palette::baseline()).map(|(rooms, _regions)| rooms)
    }

    fn slug(value: &str) -> PlaceKey {
        PlaceKey::parse(value).expect("valid test slug")
    }

    #[test]
    fn parse_exit_maps_an_unknown_direction_to_a_world_error() {
        // The per-word round-trip lives in mud-core; here we only verify that a
        // core parse failure surfaces as WorldError::UnknownDirection carrying
        // the offending word.
        let document =
            KdlDocument::parse("exit \"sideways\" \"target\"").expect("valid kdl for test");
        let node = document.nodes().first().expect("one exit node");

        let error = parse_exit(node).expect_err("sideways is not a direction");

        assert!(
            matches!(error, WorldError::UnknownDirection { ref value } if value == "sideways"),
            "got {error:?}"
        );
    }

    #[test]
    fn advance_increments_the_counter() {
        assert_eq!(advance(NonZeroU64::MIN).get(), 2);
    }

    #[test]
    fn advance_saturates_at_max_rather_than_wrapping() {
        // The overflow branch is unreachable in practice; assert it stays valid.
        assert_eq!(advance(NonZeroU64::MAX), NonZeroU64::MAX);
    }

    #[test]
    fn an_empty_world_directory_loads_no_rooms_and_no_regions() {
        let dir = TempDir::new().expect("temp dir");
        let (rooms, regions) =
            load_rooms(dir.path(), &Palette::baseline()).expect("empty world loads");
        assert_eq!(rooms.len(), 0);
        assert!(rooms.is_empty());
        assert_eq!(rooms.iter().count(), 0);
        // No room authored means no region is mandatory (§2.2.7.3).
        assert_eq!(regions.len(), 0);
        assert!(regions.is_empty());
    }

    #[test]
    fn a_room_under_no_region_manifest_is_rejected() {
        let dir = TempDir::new().expect("temp dir");
        fs::write(dir.path().join("a.kdl"), "room \"a\" { description \"x\" }")
            .expect("write room file");
        let error =
            load_rooms(dir.path(), &Palette::baseline()).expect_err("an uncovered room must fail");
        assert!(
            matches!(error, WorldError::RoomOutsideRegion { .. }),
            "got {error:?}"
        );
    }

    #[test]
    fn a_region_folder_with_no_rooms_loads_the_region_without_rooms() {
        // A builder may author a region before any of its rooms; the region loads
        // and is counted even though it covers no `Place` yet (§2.2.7.3 mandates a
        // region per room, not a room per region).
        let dir = TempDir::new().expect("temp dir");
        let region_dir = dir.path().join("zone");
        fs::create_dir_all(&region_dir).expect("create region dir");
        fs::write(region_dir.join(REGION_MANIFEST), "region \"zone\"")
            .expect("write region manifest");

        let (rooms, regions) =
            load_rooms(dir.path(), &Palette::baseline()).expect("roomless region loads");
        assert!(rooms.is_empty());
        assert_eq!(regions.len(), 1);
        assert!(!regions.is_empty());
    }

    #[test]
    fn accessors_expose_loaded_rooms_and_their_slug_mapping() {
        let rooms = load(&[(
            "a.kdl",
            "room \"town\" { description \"x\" }\nroom \"market\" { description \"y\" }",
        )])
        .expect("rooms load");

        assert_eq!(rooms.len(), 2);
        assert!(!rooms.is_empty());

        let town = rooms.id_of(&slug("town")).expect("town present");
        assert!(rooms.get(town).is_some());
        assert_eq!(rooms.id_of(&slug("absent")), None);
        assert_eq!(rooms.iter().count(), 2);

        // place_keys round-trips every handle back to the slug it was minted for.
        let pairs: HashMap<PlaceId, PlaceKey> = rooms.place_keys().collect();
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs.get(&town), Some(&slug("town")));
    }

    #[test]
    fn non_kdl_files_are_ignored() {
        let rooms = load(&[
            ("a.kdl", "room \"town\" { description \"x\" }"),
            ("notes.txt", "this is not a room file"),
            ("README.md", "# world"),
        ])
        .expect("only kdl files are parsed");
        assert_eq!(rooms.len(), 1);
    }

    #[test]
    fn a_room_without_a_slug_is_a_missing_field() {
        let error =
            load(&[("a.kdl", "room { description \"x\" }")]).expect_err("a slugless room fails");
        assert!(
            matches!(error, WorldError::MissingField { field: "slug", .. }),
            "got {error:?}"
        );
    }

    #[test]
    fn an_exit_without_a_direction_is_a_missing_field() {
        let error = load(&[("a.kdl", "room \"a\" { description \"x\"; exit }")])
            .expect_err("an exit without a direction fails");
        assert!(
            matches!(
                error,
                WorldError::MissingField {
                    field: "direction",
                    ..
                }
            ),
            "got {error:?}"
        );
    }

    #[test]
    fn an_exit_without_a_target_is_a_missing_field() {
        let error = load(&[("a.kdl", "room \"a\" { description \"x\"; exit \"north\" }")])
            .expect_err("an exit without a target fails");
        assert!(
            matches!(
                error,
                WorldError::MissingField {
                    field: "target",
                    ..
                }
            ),
            "got {error:?}"
        );
    }

    #[test]
    fn a_title_is_bolded_by_the_field_policy() {
        use mud_core::{Attributes, Style};

        let rooms = load(&[(
            "a.kdl",
            "room \"a\" { title \"Great Hall\"; description \"x\" }",
        )])
        .expect("rooms load");
        let id = rooms.id_of(&slug("a")).expect("room a present");
        let title = rooms.get(id).expect("room").title().expect("title present");
        assert_eq!(
            title.styled(),
            &StyledText::new().styled("Great Hall", Style::new().with_attrs(Attributes::BOLD))
        );
    }

    #[test]
    fn a_description_with_markup_compiles_through_the_palette() {
        use mud_core::{EntityArena, Style, TenantTag};

        let rooms = load(&[("a.kdl", "room \"a\" { description \"a {fg=cyan}rune{/}\" }")])
            .expect("rooms load");
        let id = rooms.id_of(&slug("a")).expect("room a present");

        // describe() takes a viewer; mint one through the public arena API.
        let mut arena = EntityArena::new(TenantTag::new(0).expect("tenant 0"));
        let viewer = arena.alloc().expect("viewer entity");
        let description = rooms.get(id).expect("room").describe(viewer);

        let cyan = Palette::baseline().color("cyan").expect("cyan in baseline");
        assert_eq!(
            description.styled(),
            &StyledText::new()
                .plain("a ")
                .styled("rune", Style::new().with_fg(cyan))
        );
    }
}
