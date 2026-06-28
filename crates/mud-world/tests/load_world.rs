//! End-to-end loading of a tenant's authored world: the checked-in fixture loads
//! cleanly, and each malformed input surfaces a typed [`WorldError`].
#![allow(clippy::expect_used)] // test helpers; mirrors `allow-expect-in-tests`

use std::fs;
use std::path::{Path, PathBuf};

use mud_core::{Direction, PlaceKey, RegionKey};
use mud_world::{RegionName, TenantConfig, WorldError, load_world};
use tempfile::TempDir;

/// The checked-in happy-path tenant fixture.
fn fixture_tenant() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tenant")
}

fn slug(value: &str) -> PlaceKey {
    PlaceKey::parse(value).expect("test slug must be valid")
}

fn region_slug(value: &str) -> RegionKey {
    RegionKey::parse(value).expect("test region slug must be valid")
}

/// A region manifest covering `world/zone/…`, so room-focused error cases satisfy
/// the mandatory-region rule (§2.2.7.3) without each re-authoring a manifest.
const ZONE: (&str, &str) = ("world/zone/region.kdl", "region \"zone\"");

/// Writes a synthetic tenant directory from `(relative_path, contents)` pairs and
/// returns it, so malformed-input cases stay self-contained.
fn write_tenant(files: &[(&str, &str)]) -> TempDir {
    let dir = TempDir::new().expect("temp dir");
    for (relative, contents) in files {
        let path = dir.path().join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent dirs");
        }
        fs::write(&path, contents).expect("write fixture file");
    }
    dir
}

/// Loads a synthetic tenant, returning the load result.
fn load(files: &[(&str, &str)]) -> Result<mud_world::LoadedWorld, WorldError> {
    let dir = write_tenant(files);
    let config = TenantConfig::load(dir.path()).expect("config loads");
    load_world(&config)
}

#[test]
fn loads_the_fixture_world() {
    let config = TenantConfig::load(fixture_tenant()).expect("fixture config loads");
    let world = load_world(&config).expect("fixture world loads");

    // Three rooms across two region folders.
    assert_eq!(world.rooms().len(), 3);
    assert!(world.banner().contains("Welcome to Ferrodun"));

    let rooms = world.rooms();
    let town = rooms
        .id_of(&slug("town_square"))
        .expect("town_square loaded");
    let market = rooms.id_of(&slug("market")).expect("market loaded");
    let cellar = rooms
        .id_of(&slug("cellar"))
        .expect("cellar loaded (nested)");

    // The player starts in the configured room.
    assert_eq!(world.player().start_room(), town);

    // Exits resolve across files and regions (town -> cellar lives in another region).
    let town_room = rooms.get(town).expect("town room present");
    assert_eq!(town_room.neighbor(Direction::North), Some(market));
    assert_eq!(town_room.neighbor(Direction::Down), Some(cellar));

    // Title is optional: town has one, the nested cellar does not.
    assert_eq!(
        town_room.title().map(|t| t.to_plain_string()).as_deref(),
        Some("Town Square"),
        "town_square keeps its authored title"
    );
    let cellar_room = rooms.get(cellar).expect("cellar room present");
    assert_eq!(
        cellar_room.title(),
        None,
        "cellar was authored without a title"
    );
}

#[test]
fn a_tenant_palette_is_layered_over_the_baseline() {
    use mud_core::{Attributes, Color, RoleName, Style};

    let world = load(&[
        ("config.toml", "start_room = \"a\""),
        ("welcome.kdl", "banner \"hi\""),
        (
            "palette.kdl",
            "color \"flame\" \"#ff7733\"\nrole \"say\" fg=\"flame\" bold=#true",
        ),
        ZONE,
        ("world/zone/a.kdl", "room \"a\" { description \"x\" }"),
    ])
    .expect("a world with a tenant palette loads");

    let palette = world.palette();
    // The added named color and the overridden baseline role are both present.
    assert_eq!(palette.color("flame"), Some(Color::rgb(0xff, 0x77, 0x33)));
    assert_eq!(
        palette.resolve_role(&RoleName::SAY),
        Some(
            Style::new()
                .with_fg(Color::rgb(0xff, 0x77, 0x33))
                .with_attrs(Attributes::BOLD)
        )
    );
}

#[test]
fn a_room_description_resolves_a_tenant_palette_color() {
    use mud_core::{Color, EntityArena, Style, StyledText, TenantTag};

    let world = load(&[
        ("config.toml", "start_room = \"a\""),
        ("welcome.kdl", "banner \"hi\""),
        ("palette.kdl", "color \"flame\" \"#ff7733\""),
        ZONE,
        (
            "world/zone/a.kdl",
            "room \"a\" { description \"a {fg=flame}rune{/}\" }",
        ),
    ])
    .expect("a world whose room markup uses a tenant color loads");

    let id = world.rooms().id_of(&slug("a")).expect("room a present");
    // describe() takes a viewer; mint one through the public arena API.
    let mut arena = EntityArena::new(TenantTag::new(0).expect("tenant 0"));
    let viewer = arena.alloc().expect("viewer entity");
    let description = world.rooms().get(id).expect("room").describe(viewer);

    // The tenant color resolved end-to-end, through the room loader's markup compiler.
    assert_eq!(
        description.styled(),
        &StyledText::new()
            .plain("a ")
            .styled("rune", Style::new().with_fg(Color::rgb(0xff, 0x77, 0x33)))
    );
}

#[test]
fn an_invalid_tenant_palette_fails_the_load() {
    let error = load(&[
        ("config.toml", "start_room = \"a\""),
        ("welcome.kdl", "banner \"hi\""),
        ("palette.kdl", "color \"bad\" \"#zzzzzz\""),
        ZONE,
        ("world/zone/a.kdl", "room \"a\" { description \"x\" }"),
    ])
    .expect_err("an invalid palette must fail the load");
    assert!(
        matches!(error, WorldError::InvalidColor { .. }),
        "got {error:?}"
    );
}

#[test]
fn malformed_kdl_is_a_structured_error() {
    let error = load(&[
        ("config.toml", "start_room = \"a\""),
        ("welcome.kdl", "banner \"hi\""),
        ZONE,
        ("world/zone/bad.kdl", "room \"a\" { description ="),
    ])
    .expect_err("malformed kdl must fail");
    assert!(matches!(error, WorldError::Kdl { .. }), "got {error:?}");
}

#[test]
fn an_exit_to_an_unknown_room_is_a_dangling_exit() {
    let error = load(&[
        ("config.toml", "start_room = \"a\""),
        ("welcome.kdl", "banner \"hi\""),
        ZONE,
        (
            "world/zone/a.kdl",
            "room \"a\" { description \"x\"; exit \"north\" \"nowhere\" }",
        ),
    ])
    .expect_err("a dangling exit must fail");
    assert!(
        matches!(error, WorldError::DanglingExit { ref to, .. } if to == "nowhere"),
        "got {error:?}"
    );
}

#[test]
fn a_repeated_slug_is_a_duplicate() {
    let error = load(&[
        ("config.toml", "start_room = \"a\""),
        ("welcome.kdl", "banner \"hi\""),
        ZONE,
        (
            "world/zone/a.kdl",
            "room \"a\" { description \"x\" }\nroom \"a\" { description \"y\" }",
        ),
    ])
    .expect_err("a duplicate slug must fail");
    assert!(
        matches!(error, WorldError::DuplicateSlug { ref slug } if slug == "a"),
        "got {error:?}"
    );
}

#[test]
fn an_unknown_exit_direction_is_an_error() {
    let error = load(&[
        ("config.toml", "start_room = \"a\""),
        ("welcome.kdl", "banner \"hi\""),
        ZONE,
        (
            "world/zone/a.kdl",
            "room \"a\" { description \"x\"; exit \"sideways\" \"a\" }",
        ),
    ])
    .expect_err("an unknown direction must fail");
    assert!(
        matches!(error, WorldError::UnknownDirection { ref value } if value == "sideways"),
        "got {error:?}"
    );
}

#[test]
fn an_invalid_slug_is_an_error() {
    let error = load(&[
        ("config.toml", "start_room = \"a\""),
        ("welcome.kdl", "banner \"hi\""),
        ZONE,
        (
            "world/zone/a.kdl",
            "room \"Bad Slug\" { description \"x\" }",
        ),
    ])
    .expect_err("an invalid slug must fail");
    assert!(
        matches!(error, WorldError::InvalidSlug { .. }),
        "got {error:?}"
    );
}

#[test]
fn a_room_without_a_description_is_an_error() {
    let error = load(&[
        ("config.toml", "start_room = \"a\""),
        ("welcome.kdl", "banner \"hi\""),
        ZONE,
        ("world/zone/a.kdl", "room \"a\" { title \"A\" }"),
    ])
    .expect_err("a room without a description must fail");
    assert!(
        matches!(
            error,
            WorldError::MissingField {
                field: "description",
                ..
            }
        ),
        "got {error:?}"
    );
}

#[test]
fn an_exit_to_a_malformed_slug_is_an_invalid_slug() {
    let error = load(&[
        ("config.toml", "start_room = \"a\""),
        ("welcome.kdl", "banner \"hi\""),
        ZONE,
        (
            "world/zone/a.kdl",
            "room \"a\" { description \"x\"; exit \"north\" \"Bad Slug\" }",
        ),
    ])
    .expect_err("an exit to a malformed slug must fail");
    assert!(
        matches!(error, WorldError::InvalidSlug { ref value, .. } if value == "Bad Slug"),
        "a malformed exit target is an invalid slug, not a dangling exit, got {error:?}"
    );
}

#[test]
fn an_unknown_top_level_node_is_an_error() {
    let error = load(&[
        ("config.toml", "start_room = \"a\""),
        ("welcome.kdl", "banner \"hi\""),
        ZONE,
        ("world/zone/a.kdl", "rooom \"a\" { description \"x\" }"),
    ])
    .expect_err("an unknown top-level node must fail");
    assert!(
        matches!(error, WorldError::UnexpectedNode { ref node, .. } if node == "rooom"),
        "a misspelled room node must surface as UnexpectedNode, got {error:?}"
    );
}

#[test]
fn an_unknown_room_child_is_an_error() {
    let error = load(&[
        ("config.toml", "start_room = \"a\""),
        ("welcome.kdl", "banner \"hi\""),
        ZONE,
        (
            "world/zone/a.kdl",
            "room \"a\" { description \"x\"; descriptipn \"typo\" }",
        ),
    ])
    .expect_err("an unknown room child must fail");
    assert!(
        matches!(error, WorldError::UnexpectedNode { ref node, .. } if node == "descriptipn"),
        "a misspelled room field must surface as UnexpectedNode, got {error:?}"
    );
}

#[test]
fn a_malformed_start_room_slug_is_an_invalid_slug() {
    let error = load(&[
        ("config.toml", "start_room = \"Bad Slug\""),
        ("welcome.kdl", "banner \"hi\""),
        ZONE,
        ("world/zone/a.kdl", "room \"a\" { description \"x\" }"),
    ])
    .expect_err("a malformed start_room slug must fail");
    assert!(
        matches!(error, WorldError::InvalidSlug { ref value, .. } if value == "Bad Slug"),
        "a malformed start_room is an invalid slug, not an unknown room, got {error:?}"
    );
}

#[test]
fn an_unknown_start_room_is_an_error() {
    let error = load(&[
        ("config.toml", "start_room = \"missing\""),
        ("welcome.kdl", "banner \"hi\""),
        ZONE,
        ("world/zone/a.kdl", "room \"a\" { description \"x\" }"),
    ])
    .expect_err("an unknown start_room must fail");
    assert!(
        matches!(error, WorldError::UnknownStartRoom { ref slug } if slug == "missing"),
        "got {error:?}"
    );
}

#[test]
fn rooms_bind_to_their_folder_region() {
    let config = TenantConfig::load(fixture_tenant()).expect("fixture config loads");
    let world = load_world(&config).expect("fixture world loads");
    let rooms = world.rooms();
    let regions = world.regions();

    let town = rooms.id_of(&slug("town_square")).expect("town loaded");
    let market = rooms.id_of(&slug("market")).expect("market loaded");
    let cellar = rooms.id_of(&slug("cellar")).expect("cellar loaded");

    let town_region = rooms.get(town).expect("town present").region();
    let market_region = rooms.get(market).expect("market present").region();
    let cellar_region = rooms.get(cellar).expect("cellar present").region();

    // The fixture declares exactly two regions; there is no implicit fallback.
    assert_eq!(regions.len(), 2);

    // The cellar lives under `world/keep/region.kdl`, so it binds to that region.
    let old_keep = regions
        .id_of(&region_slug("old_keep"))
        .expect("old_keep region");
    assert_eq!(
        cellar_region, old_keep,
        "cellar binds to its folder's region"
    );
    assert_eq!(
        regions.name_of(old_keep),
        Some(&RegionName::new("The Old Keep")),
        "the manifest's display name is exposed"
    );

    // town_square and market live under `world/town/region.kdl`.
    let town_reg = regions.id_of(&region_slug("town")).expect("town region");
    assert_eq!(
        town_region, town_reg,
        "town_square binds to the town region"
    );
    assert_eq!(
        market_region, town_reg,
        "rooms in the same region folder share it"
    );
    assert_ne!(
        town_region, cellar_region,
        "rooms in distinct folders bind to distinct regions"
    );
    assert_eq!(
        regions.name_of(town_reg),
        Some(&RegionName::new("Town")),
        "the town manifest's display name is exposed"
    );
}

#[test]
fn a_room_outside_any_region_is_rejected() {
    let error = load(&[
        ("config.toml", "start_room = \"a\""),
        ("welcome.kdl", "banner \"hi\""),
        ("world/a.kdl", "room \"a\" { description \"x\" }"),
    ])
    .expect_err("a room under no region must fail");
    assert!(
        matches!(error, WorldError::RoomOutsideRegion { .. }),
        "got {error:?}"
    );
}

#[test]
fn a_region_manifest_at_the_world_root_is_rejected() {
    let error = load(&[
        ("config.toml", "start_room = \"r\""),
        ("welcome.kdl", "banner \"hi\""),
        ("world/region.kdl", "region \"root\""),
        ZONE,
        ("world/zone/r.kdl", "room \"r\" { description \"x\" }"),
    ])
    .expect_err("a world-root manifest must fail");
    assert!(
        matches!(error, WorldError::RegionManifestAtWorldRoot { .. }),
        "got {error:?}"
    );
}

#[test]
fn a_duplicate_region_slug_is_an_error() {
    let error = load(&[
        ("config.toml", "start_room = \"r\""),
        ("welcome.kdl", "banner \"hi\""),
        ("world/a/region.kdl", "region \"dup\""),
        ("world/b/region.kdl", "region \"dup\""),
        ("world/a/r.kdl", "room \"r\" { description \"x\" }"),
    ])
    .expect_err("a duplicate region slug must fail");
    assert!(
        matches!(error, WorldError::DuplicateRegionSlug { ref slug } if slug == "dup"),
        "got {error:?}"
    );
}

#[test]
fn a_nested_region_manifest_is_rejected() {
    let error = load(&[
        ("config.toml", "start_room = \"r\""),
        ("welcome.kdl", "banner \"hi\""),
        ("world/a/region.kdl", "region \"outer\""),
        ("world/a/b/region.kdl", "region \"inner\""),
        ("world/a/r.kdl", "room \"r\" { description \"x\" }"),
    ])
    .expect_err("a nested region must fail");
    assert!(
        matches!(error, WorldError::NestedRegion { .. }),
        "got {error:?}"
    );
}

#[test]
fn a_manifest_without_exactly_one_region_is_an_error() {
    let error = load(&[
        ("config.toml", "start_room = \"r\""),
        ("welcome.kdl", "banner \"hi\""),
        ("world/x/region.kdl", "region \"a\"\nregion \"b\""),
        ("world/x/r.kdl", "room \"r\" { description \"x\" }"),
    ])
    .expect_err("two regions in one manifest must fail");
    assert!(
        matches!(error, WorldError::InvalidRegionManifest { .. }),
        "got {error:?}"
    );
}

#[test]
fn an_unknown_region_manifest_child_is_an_error() {
    let error = load(&[
        ("config.toml", "start_room = \"r\""),
        ("welcome.kdl", "banner \"hi\""),
        ("world/x/region.kdl", "region \"a\" { naem \"typo\" }"),
        ("world/x/r.kdl", "room \"r\" { description \"x\" }"),
    ])
    .expect_err("an unknown manifest child must fail");
    assert!(
        matches!(error, WorldError::UnexpectedNode { ref node, .. } if node == "naem"),
        "got {error:?}"
    );
}

#[test]
fn a_region_with_an_invalid_slug_is_an_error() {
    let error = load(&[
        ("config.toml", "start_room = \"r\""),
        ("welcome.kdl", "banner \"hi\""),
        ("world/x/region.kdl", "region \"Bad Slug\""),
        ("world/x/r.kdl", "room \"r\" { description \"x\" }"),
    ])
    .expect_err("an invalid region slug must fail");
    assert!(
        matches!(error, WorldError::InvalidRegionSlug { ref value, .. } if value == "Bad Slug"),
        "got {error:?}"
    );
}

#[test]
fn a_region_without_a_slug_is_an_error() {
    let error = load(&[
        ("config.toml", "start_room = \"r\""),
        ("welcome.kdl", "banner \"hi\""),
        ("world/x/region.kdl", "region"),
        ("world/x/r.kdl", "room \"r\" { description \"x\" }"),
    ])
    .expect_err("a slug-less region must fail");
    assert!(
        matches!(error, WorldError::MissingField { ref node, field } if node == "region" && field == "slug"),
        "got {error:?}"
    );
}
