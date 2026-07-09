//! The per-tenant configuration loaded from `<tenant_dir>/config.toml` (§4.1).
//!
//! One folder per tenant (SPEC §5): the directory holds `config.toml`, the
//! `world/` room files, and the welcome banner. The file is the sole source of
//! a tenant's configuration — there is deliberately no environment override, so
//! that in a multi-tenant process no ambient variable can silently reshape one
//! tenant (or, worse, all of them at once).

use std::path::{Component, Path, PathBuf};

use figment::Figment;
use figment::providers::{Format, Toml};
use mud_core::TenantTag;
use mud_i18n::Locale;
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
    #[serde(default = "default_palette")]
    palette: PathBuf,
    start_room: String,
    /// The raw 12-bit tenant tag (§3.11). Validated against `TenantTag::MAX` at
    /// [`load`](TenantConfig::load); use [`tenant_tag`](TenantConfig::tenant_tag)
    /// to get the typed value.
    #[serde(default)]
    tenant_tag: u16,
    /// The tenant's rendering locale (§3.14.6).
    #[serde(default = "default_locale")]
    locale: String,
    /// The tenant directory, supplied by [`load`](TenantConfig::load) rather than
    /// read from the file.
    #[serde(skip)]
    tenant_dir: PathBuf,
}

/// The default welcome-banner file name, relative to the tenant directory.
fn default_banner() -> PathBuf {
    PathBuf::from("welcome.kdl")
}

/// The default palette file name, relative to the tenant directory (§3.20.3.1).
fn default_palette() -> PathBuf {
    PathBuf::from("palette.kdl")
}

/// The default rendering locale, `en` (§3.14.6).
fn default_locale() -> String {
    "en".to_owned()
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
    /// Loads `<tenant_dir>/config.toml`.
    ///
    /// # Errors
    ///
    /// Returns [`WorldError::Config`] if the file is missing, malformed, or omits
    /// a required field (`start_room`), [`WorldError::EscapingPath`] if the
    /// configured `banner` path would escape the tenant directory, or
    /// [`WorldError::TenantTagOutOfRange`] if `tenant_tag` exceeds
    /// `TenantTag::MAX`.
    pub fn load(tenant_dir: impl AsRef<Path>) -> Result<Self, WorldError> {
        let tenant_dir = tenant_dir.as_ref();
        let mut config: TenantConfig = Figment::new()
            .merge(Toml::file(tenant_dir.join("config.toml")))
            .extract()
            .map_err(|error| WorldError::Config(Box::new(error)))?;
        ensure_contained("banner", &config.banner)?;
        ensure_contained("palette", &config.palette)?;
        let _: TenantTag = TenantTag::try_from(config.tenant_tag)
            .map_err(|_| WorldError::TenantTagOutOfRange(config.tenant_tag))?;
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

    /// The absolute path to the tenant palette KDL file (which may be absent,
    /// §3.20.3.1).
    #[must_use]
    pub fn palette_path(&self) -> PathBuf {
        self.tenant_dir.join(&self.palette)
    }

    /// The absolute path to the directory holding the KDL room files.
    #[must_use]
    pub fn world_dir(&self) -> PathBuf {
        self.tenant_dir.join(WORLD_SUBDIR)
    }

    /// The tenant's identity tag (§3.11).
    pub fn tenant_tag(&self) -> TenantTag {
        // load() rejects values > TenantTag::MAX, so try_from always succeeds;
        // unwrap_or_default (tenant 0) is a defensive floor, never taken here.
        TenantTag::try_from(self.tenant_tag).unwrap_or_default()
    }

    /// The tenant's rendering locale (§3.14.6).
    pub fn locale(&self) -> Locale {
        Locale::new(self.locale.clone())
    }
}

#[cfg(test)]
mod tests {
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
    fn tenant_tag_and_locale_are_exposed() {
        figment::Jail::expect_with(|jail| {
            jail.create_file(
                "config.toml",
                "start_room = \"town_square\"\ntenant_tag = 3\nlocale = \"fr\"",
            )?;
            let config = TenantConfig::load(jail.directory()).expect("config loads");

            assert_eq!(config.tenant_tag(), TenantTag::new(3).expect("in range"));
            assert_eq!(config.locale().as_str(), "fr");
            Ok(())
        });
    }

    #[test]
    fn tenant_tag_and_locale_default_when_absent() {
        figment::Jail::expect_with(|jail| {
            jail.create_file("config.toml", "start_room = \"town_square\"")?;
            let config = TenantConfig::load(jail.directory()).expect("config loads");

            assert_eq!(config.tenant_tag(), TenantTag::default());
            assert_eq!(config.locale().as_str(), "en");
            Ok(())
        });
    }

    #[test]
    fn an_out_of_range_tenant_tag_is_rejected() {
        figment::Jail::expect_with(|jail| {
            jail.create_file(
                "config.toml",
                "start_room = \"town_square\"\ntenant_tag = 5000",
            )?;
            let error = TenantConfig::load(jail.directory());

            assert!(
                matches!(error, Err(WorldError::TenantTagOutOfRange(5000))),
                "an out-of-range tenant_tag must be rejected, got {error:?}"
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
