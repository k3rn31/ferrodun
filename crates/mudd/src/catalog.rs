//! The tenant catalogue: the operator-side registry that assigns each
//! tenant its listen port and runtime tenant tag (design:
//! docs/superpowers/specs/2026-07-11-tenant-catalog-cli-design.md).

use std::collections::HashSet;
use std::fmt;
use std::path::Path;

use anyhow::Context;
use serde::{Deserialize, Serialize};

use mud_core::TenantTag;

/// A tenant's name: lowercase ASCII alphanumeric plus `-`/`_`, starting with
/// an alphanumeric. It doubles as the tenant's folder name under
/// `tenants_dir`, so the grammar is deliberately filesystem-safe.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct TenantName(String);

impl TenantName {
    /// Parses a tenant name, rejecting anything but lowercase ASCII
    /// alphanumerics, `-`, and `_` (the first character must be
    /// alphanumeric).
    ///
    /// # Errors
    ///
    /// Returns an error naming the offending value when the grammar is
    /// violated.
    pub fn parse(value: &str) -> anyhow::Result<TenantName> {
        let mut chars = value.chars();
        let starts_alnum = chars
            .next()
            .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
        let rest_valid = chars
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
        if !(starts_alnum && rest_valid) {
            anyhow::bail!(
                "invalid tenant name {value:?}: use lowercase letters, digits, `-`, `_`, starting with a letter or digit"
            );
        }
        Ok(TenantName(value.to_owned()))
    }

    /// The name as authored.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TenantName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for TenantName {
    type Error = anyhow::Error;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        TenantName::parse(&value)
    }
}

impl From<TenantName> for String {
    fn from(name: TenantName) -> String {
        name.0
    }
}

/// One catalogue row: a tenant and its assigned runtime values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogEntry {
    pub name: TenantName,
    pub port: u16,
    pub tag: TenantTag,
}

/// The tenant catalogue, as loaded from `catalog.toml`.
///
/// The file is machine-owned — `mudd tenant` subcommands are its only
/// writers — but hand-edits are validated on load: unique names, ports, and
/// tags, with tags in `1..=TenantTag::MAX` (tag 0 is reserved for
/// `--tenant-dir` dev mode).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Catalog {
    entries: Vec<CatalogEntry>,
}

/// The on-disk shape of `catalog.toml`.
#[derive(Debug, Default, Serialize, Deserialize)]
struct RawCatalog {
    #[serde(default)]
    tenants: Vec<RawCatalogEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct RawCatalogEntry {
    name: TenantName,
    port: u16,
    tag: u16,
}

impl Catalog {
    /// Loads the catalogue from `path`. A missing file is an empty
    /// catalogue.
    ///
    /// # Errors
    ///
    /// Returns an error if the file is unreadable or malformed, or if it
    /// violates a catalogue invariant (duplicate name/port/tag, or a tag
    /// outside `1..=TenantTag::MAX`).
    pub fn load(path: &Path) -> anyhow::Result<Catalog> {
        if !path.exists() {
            return Ok(Catalog::default());
        }
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading tenant catalogue {}", path.display()))?;
        let raw: RawCatalog = toml::from_str(&text)
            .with_context(|| format!("parsing tenant catalogue {}", path.display()))?;

        let mut entries = Vec::with_capacity(raw.tenants.len());
        let mut names = HashSet::new();
        let mut ports = HashSet::new();
        let mut tags = HashSet::new();
        for raw_entry in raw.tenants {
            let tag = (raw_entry.tag >= 1)
                .then(|| TenantTag::new(raw_entry.tag).ok())
                .flatten()
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "{}: tenant {:?} has tag {} outside 1..={}",
                        path.display(),
                        raw_entry.name.as_str(),
                        raw_entry.tag,
                        TenantTag::MAX,
                    )
                })?;
            if !names.insert(raw_entry.name.clone()) {
                anyhow::bail!(
                    "{}: duplicate tenant name {:?}",
                    path.display(),
                    raw_entry.name.as_str()
                );
            }
            if !ports.insert(raw_entry.port) {
                anyhow::bail!("{}: duplicate port {}", path.display(), raw_entry.port);
            }
            if !tags.insert(tag) {
                anyhow::bail!("{}: duplicate tag {}", path.display(), tag.get());
            }
            entries.push(CatalogEntry {
                name: raw_entry.name,
                port: raw_entry.port,
                tag,
            });
        }
        Ok(Catalog { entries })
    }

    /// Serializes the whole catalogue to `path`, replacing the file.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization or the write fails.
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let raw = RawCatalog {
            tenants: self
                .entries
                .iter()
                .map(|entry| RawCatalogEntry {
                    name: entry.name.clone(),
                    port: entry.port,
                    tag: entry.tag.get(),
                })
                .collect(),
        };
        let text = toml::to_string_pretty(&raw).context("serializing tenant catalogue")?;
        std::fs::write(path, text)
            .with_context(|| format!("writing tenant catalogue {}", path.display()))?;
        Ok(())
    }

    /// The registered tenants, in registration order.
    #[must_use]
    pub fn entries(&self) -> &[CatalogEntry] {
        &self.entries
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_simple_slug_parses() {
        let name = TenantName::parse("my-game_2").expect("valid slug");
        assert_eq!(name.as_str(), "my-game_2");
    }

    #[test]
    fn uppercase_is_rejected() {
        assert!(TenantName::parse("MyGame").is_err());
    }

    #[test]
    fn a_leading_separator_is_rejected() {
        assert!(TenantName::parse("-game").is_err());
        assert!(TenantName::parse("_game").is_err());
    }

    #[test]
    fn empty_and_path_like_names_are_rejected() {
        assert!(TenantName::parse("").is_err());
        assert!(TenantName::parse("a/b").is_err());
        assert!(TenantName::parse("..").is_err());
    }

    use mud_core::TenantTag;

    fn tag(value: u16) -> TenantTag {
        TenantTag::new(value).expect("test tag is in range")
    }

    #[test]
    fn a_missing_file_is_an_empty_catalog() {
        let dir = tempfile::tempdir().expect("temp dir");
        let catalog = Catalog::load(&dir.path().join("catalog.toml")).expect("loads");
        assert!(catalog.entries().is_empty());
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("catalog.toml");
        let catalog = Catalog {
            entries: vec![CatalogEntry {
                name: TenantName::parse("alpha").expect("slug"),
                port: 4000,
                tag: tag(1),
            }],
        };
        catalog.save(&path).expect("saves");

        let reloaded = Catalog::load(&path).expect("loads");
        assert_eq!(reloaded, catalog);
    }

    #[test]
    fn duplicate_ports_in_the_file_are_rejected() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("catalog.toml");
        std::fs::write(
            &path,
            "[[tenants]]\nname = \"a\"\nport = 4000\ntag = 1\n\n[[tenants]]\nname = \"b\"\nport = 4000\ntag = 2\n",
        )
        .expect("write");
        assert!(Catalog::load(&path).is_err(), "duplicate port must be rejected");
    }

    #[test]
    fn duplicate_names_and_tags_are_rejected() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("catalog.toml");
        std::fs::write(
            &path,
            "[[tenants]]\nname = \"a\"\nport = 4000\ntag = 1\n\n[[tenants]]\nname = \"a\"\nport = 4001\ntag = 2\n",
        )
        .expect("write");
        assert!(Catalog::load(&path).is_err(), "duplicate name must be rejected");

        std::fs::write(
            &path,
            "[[tenants]]\nname = \"a\"\nport = 4000\ntag = 1\n\n[[tenants]]\nname = \"b\"\nport = 4001\ntag = 1\n",
        )
        .expect("write");
        assert!(Catalog::load(&path).is_err(), "duplicate tag must be rejected");
    }

    #[test]
    fn out_of_range_tags_are_rejected() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("catalog.toml");
        std::fs::write(&path, "[[tenants]]\nname = \"a\"\nport = 4000\ntag = 0\n")
            .expect("write");
        assert!(Catalog::load(&path).is_err(), "tag 0 is reserved for dev mode");

        std::fs::write(&path, "[[tenants]]\nname = \"a\"\nport = 4000\ntag = 5000\n")
            .expect("write");
        assert!(Catalog::load(&path).is_err(), "tag above 4095 must be rejected");
    }
}
