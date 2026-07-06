//! Builder-authored static content for one tenant (SPEC §4.1, §5).
//!
//! `mud-world` loads what a builder authors on disk — the per-tenant
//! `config.toml`, the KDL room files under `world/`, and the welcome banner — and
//! lowers them into typed domain values. It produces *static content*: a [`Rooms`]
//! registry, the [`PlayerArchetype`], and the banner text. The dynamic
//! `mud_core::World` (the entity arena) is rebuilt from the database elsewhere;
//! this crate never constructs one.
//!
//! Rooms are keyed by a durable slug ([`mud_core::PlaceKey`]); the loader assigns
//! each an ephemeral [`mud_core::PlaceId`] and exposes the mapping
//! ([`Rooms::place_keys`]) so the persistence layer can translate a stored
//! location slug back to a live handle.

mod archetype;
mod banner;
mod config;
mod error;
mod kdl;
mod palette;
mod regions;
mod rooms;

pub use archetype::PlayerArchetype;
pub use config::TenantConfig;
pub use error::WorldError;
pub use regions::{RegionName, Regions};
pub use rooms::Rooms;

use mud_core::{Palette, PlaceKey};

/// A tenant's loaded static content.
#[derive(Debug, Clone)]
#[must_use]
pub struct LoadedWorld {
    rooms: Rooms,
    regions: Regions,
    palette: Palette,
    banner: String,
    player: PlayerArchetype,
}

impl LoadedWorld {
    /// The loaded room registry.
    pub fn rooms(&self) -> &Rooms {
        &self.rooms
    }

    /// The loaded region registry (§2.2.7).
    pub fn regions(&self) -> &Regions {
        &self.regions
    }

    /// The tenant palette: the engine baseline with any tenant overrides layered
    /// on top (§3.20.3).
    pub fn palette(&self) -> &Palette {
        &self.palette
    }

    /// The pre-login welcome banner text (§3.19.1).
    #[must_use]
    pub fn banner(&self) -> &str {
        &self.banner
    }

    /// The player puppet archetype.
    pub fn player(&self) -> PlayerArchetype {
        self.player
    }
}

/// Loads a tenant's static content: its rooms, welcome banner, and player
/// archetype.
///
/// The `start_room` named in the config is resolved against the loaded rooms; the
/// resulting [`PlayerArchetype`] is ready for the server to spawn puppets into.
///
/// # Errors
///
/// Returns [`WorldError`] if a world file, the palette, or the banner cannot be
/// read or parsed, if a room is malformed, or if `start_room` names no loaded room.
pub fn load_world(config: &TenantConfig) -> Result<LoadedWorld, WorldError> {
    // An absent palette.kdl leaves the engine baseline in force (§3.20.3.1); a
    // present one is layered over it. Room markup resolves against the result.
    let palette_path = config.palette_path();
    let palette = palette::load_palette(palette_path.exists().then_some(palette_path.as_path()))?;
    let (rooms, regions) = rooms::load_rooms(&config.world_dir(), &palette)?;
    let banner = banner::load_banner(&config.banner_path())?;

    let start_slug =
        PlaceKey::parse(config.start_room()).map_err(|source| WorldError::InvalidSlug {
            value: config.start_room().to_owned(),
            source,
        })?;
    let start_room = rooms
        .id_of(&start_slug)
        .ok_or_else(|| WorldError::UnknownStartRoom {
            slug: config.start_room().to_owned(),
        })?;

    Ok(LoadedWorld {
        rooms,
        regions,
        palette,
        banner,
        player: PlayerArchetype::new(start_room),
    })
}
