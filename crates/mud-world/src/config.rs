//! The per-tenant configuration loaded from `<tenant_dir>/config.toml` (§4.1).
//!
//! One folder per tenant (SPEC §5): the directory holds `config.toml`, the
//! `world/` room files, and the welcome banner. Configuration is layered with
//! `figment` — the TOML file is the base, then `FERRODUN_`-prefixed environment
//! variables override it.

use std::path::{Component, Path, PathBuf};

use figment::Figment;
use figment::providers::{Env, Format, Toml};
use serde::Deserialize;

use crate::error::WorldError;

/// The subdirectory of a tenant folder holding the KDL room files.
const WORLD_SUBDIR: &str = "world";

/// A tenant's loaded configuration.
///
/// Paths are stored as authored (relative to the tenant directory) and resolved
/// against it by the accessors, so callers always receive absolute paths.
#[derive(Debug, Clone, Deserialize)]
#[must_use]
pub struct TenantConfig {
    #[serde(default = "default_banner")]
    banner: PathBuf,
    start_room: String,
    /// The tenant directory, supplied by [`load`](TenantConfig::load) rather than
    /// read from the file.
    #[serde(skip)]
    tenant_dir: PathBuf,
}

/// The default welcome-banner file name, relative to the tenant directory.
fn default_banner() -> PathBuf {
    PathBuf::from("welcome.kdl")
}

/// Rejects a configured path that would leave the tenant directory once joined to
/// it: an absolute path (which `Path::join` would let replace the base) or one
/// containing a `..` component.
fn ensure_contained(field: &'static str, path: &Path) -> Result<(), WorldError> {
    let escapes = path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir));
    if escapes {
        return Err(WorldError::EscapingPath {
            field,
            path: path.to_path_buf(),
        });
    }
    Ok(())
}

impl TenantConfig {
    /// Loads `<tenant_dir>/config.toml`, layering `FERRODUN_`-prefixed environment
    /// overrides on top of the file.
    ///
    /// # Errors
    ///
    /// Returns [`WorldError::Config`] if the file is missing, malformed, or omits
    /// a required field (`start_room`), or [`WorldError::EscapingPath`] if the
    /// configured `banner` path would escape the tenant directory.
    pub fn load(tenant_dir: impl AsRef<Path>) -> Result<Self, WorldError> {
        let tenant_dir = tenant_dir.as_ref();
        let mut config: TenantConfig = Figment::new()
            .merge(Toml::file(tenant_dir.join("config.toml")))
            .merge(Env::prefixed("FERRODUN_"))
            .extract()
            .map_err(|error| WorldError::Config(Box::new(error)))?;
        ensure_contained("banner", &config.banner)?;
        config.tenant_dir = tenant_dir.to_path_buf();
        Ok(config)
    }

    /// The slug of the room a new puppet starts in.
    #[must_use]
    pub fn start_room(&self) -> &str {
        &self.start_room
    }

    /// The absolute path to the welcome-banner KDL file.
    #[must_use]
    pub fn banner_path(&self) -> PathBuf {
        self.tenant_dir.join(&self.banner)
    }

    /// The absolute path to the directory holding the KDL room files.
    #[must_use]
    pub fn world_dir(&self) -> PathBuf {
        self.tenant_dir.join(WORLD_SUBDIR)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    // `figment::Jail` closures return `Result<(), figment::Error>`; that error is
    // large, but it is the test harness's type, not ours.
    #![allow(clippy::result_large_err)]

    use super::*;

    #[test]
    fn loads_start_room_and_defaults_the_banner() {
        figment::Jail::expect_with(|jail| {
            jail.create_file("config.toml", "start_room = \"town_square\"")?;
            let config = TenantConfig::load(jail.directory()).expect("config loads");

            assert_eq!(config.start_room(), "town_square");
            assert_eq!(config.banner_path(), jail.directory().join("welcome.kdl"));
            assert_eq!(config.world_dir(), jail.directory().join("world"));
            Ok(())
        });
    }

    #[test]
    fn environment_overrides_the_file() {
        figment::Jail::expect_with(|jail| {
            jail.create_file("config.toml", "start_room = \"town_square\"")?;
            jail.set_env("FERRODUN_START_ROOM", "secret_lair");
            let config = TenantConfig::load(jail.directory()).expect("config loads");

            assert_eq!(config.start_room(), "secret_lair");
            Ok(())
        });
    }

    #[test]
    fn an_explicit_banner_overrides_the_default() {
        figment::Jail::expect_with(|jail| {
            jail.create_file(
                "config.toml",
                "start_room = \"town_square\"\nbanner = \"intro.kdl\"",
            )?;
            let config = TenantConfig::load(jail.directory()).expect("config loads");

            assert_eq!(config.banner_path(), jail.directory().join("intro.kdl"));
            Ok(())
        });
    }

    #[test]
    fn an_absolute_banner_path_is_rejected() {
        figment::Jail::expect_with(|jail| {
            jail.create_file(
                "config.toml",
                "start_room = \"town_square\"\nbanner = \"/etc/passwd\"",
            )?;
            let error = TenantConfig::load(jail.directory());

            assert!(
                matches!(
                    error,
                    Err(WorldError::EscapingPath {
                        field: "banner",
                        ..
                    })
                ),
                "an absolute banner path must be rejected, got {error:?}"
            );
            Ok(())
        });
    }

    #[test]
    fn a_parent_traversal_banner_path_is_rejected() {
        figment::Jail::expect_with(|jail| {
            jail.create_file(
                "config.toml",
                "start_room = \"town_square\"\nbanner = \"../secrets.kdl\"",
            )?;
            let error = TenantConfig::load(jail.directory());

            assert!(
                matches!(
                    error,
                    Err(WorldError::EscapingPath {
                        field: "banner",
                        ..
                    })
                ),
                "a banner path with `..` must be rejected, got {error:?}"
            );
            Ok(())
        });
    }

    #[test]
    fn a_missing_start_room_is_an_error() {
        figment::Jail::expect_with(|jail| {
            jail.create_file("config.toml", "banner = \"welcome.kdl\"")?;
            let error = TenantConfig::load(jail.directory());

            assert!(
                matches!(error, Err(WorldError::Config(_))),
                "a missing start_room must fail to load, got {error:?}"
            );
            Ok(())
        });
    }
}
