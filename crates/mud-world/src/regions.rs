//! Loading authored regions from `region.kdl` manifests (§2.2.7).
//!
//! A `region.kdl` at a world folder's root declares the durable [`RegionKey`] that
//! every room in that subtree belongs to (§2.2.7.3). Folder *names* carry no
//! identity — the slug inside the manifest does — so a folder may be renamed or
//! moved without changing a region. Rooms under no manifest belong to an implicit
//! per-tenant **default** region. Regions are flat in 1.0: a manifest nested under
//! another region's folder is rejected at load.
//!
//! ```kdl
//! region "old_keep" {
//!     name "The Old Keep"
//! }
//! ```

use std::collections::HashMap;
use std::fs;
use std::num::NonZeroU64;
use std::path::{Path, PathBuf};

use kdl::{KdlDocument, KdlNode};
use mud_core::{RegionId, RegionKey};

use crate::error::WorldError;
use crate::rooms::arg;

/// The file name that marks a folder as a region root (§2.2.7.3).
pub(crate) const REGION_MANIFEST: &str = "region.kdl";

/// The reserved slug of the implicit per-tenant default region. A `region.kdl`
/// must not author it (§2.2.7.3).
const DEFAULT_REGION_SLUG: &str = "default";

/// A region's human-facing display name (§2.2.7.2), surfaced on entry and to
/// mapping clients. Free display text with no slug constraint — distinct from the
/// durable [`RegionKey`] slug.
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub struct RegionName(String);

impl RegionName {
    /// Wraps display text as a region name.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Returns the display text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The regions loaded from a tenant's world files, keyed both ways between the
/// durable [`RegionKey`] slug and the ephemeral [`RegionId`] handle.
///
/// Every tenant has a **default** region; rooms authored under no `region.kdl`
/// manifest belong to it.
#[derive(Debug, Clone)]
#[must_use]
pub struct Regions {
    by_id: HashMap<RegionId, RegionData>,
    key_to_id: HashMap<RegionKey, RegionId>,
}

/// One region's authored content.
#[derive(Debug, Clone)]
struct RegionData {
    key: RegionKey,
    name: Option<RegionName>,
}

impl Regions {
    /// The handle a slug names, if a manifest (or the default region) defines it.
    #[must_use]
    pub fn id_of(&self, key: &RegionKey) -> Option<RegionId> {
        self.key_to_id.get(key).copied()
    }

    /// The durable slug of the region a handle names.
    #[must_use]
    pub fn key_of(&self, id: RegionId) -> Option<&RegionKey> {
        self.by_id.get(&id).map(|data| &data.key)
    }

    /// The authored display name of a region, if one was authored.
    #[must_use]
    pub fn name_of(&self, id: RegionId) -> Option<&RegionName> {
        self.by_id.get(&id).and_then(|data| data.name.as_ref())
    }

    /// The number of loaded regions, including the default region.
    // No `is_empty`: the default region is always present, so it could only ever
    // return `false` — a meaningless predicate.
    #[allow(clippy::len_without_is_empty)]
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_id.len()
    }
}

/// Binds each room's folder to a [`RegionId`]: its enclosing manifest folder, or
/// the default region. Built once per world load and consumed into the
/// [`Regions`] registry.
pub(crate) struct RegionBinder {
    /// `(governed folder, region id)`. Regions are flat (§2.2.7.3), so a folder
    /// has at most one enclosing manifest — no ordering is needed to disambiguate.
    manifests: Vec<(PathBuf, RegionId)>,
    default_id: RegionId,
    regions: Regions,
}

impl RegionBinder {
    /// Parses every region manifest under `world_dir` and prepares room binding.
    ///
    /// # Errors
    ///
    /// Returns [`WorldError`] if a manifest cannot be read or parsed, declares an
    /// invalid / reserved / duplicate slug, is nested under another region, or does
    /// not declare exactly one region.
    pub(crate) fn load(manifest_files: &[PathBuf], world_dir: &Path) -> Result<Self, WorldError> {
        // INVARIANT: DEFAULT_REGION_SLUG is a compile-time-valid slug, so parse
        // cannot fail; map the impossible error to a structured error, never panic.
        let default_key = RegionKey::parse(DEFAULT_REGION_SLUG).map_err(|source| {
            WorldError::InvalidRegionSlug {
                value: DEFAULT_REGION_SLUG.to_owned(),
                source,
            }
        })?;

        let mut next_id = NonZeroU64::MIN;
        let default_id = RegionId::new(next_id);
        next_id = advance(next_id);

        let mut by_id = HashMap::new();
        let mut key_to_id = HashMap::new();
        by_id.insert(
            default_id,
            RegionData {
                key: default_key.clone(),
                name: None,
            },
        );
        key_to_id.insert(default_key, default_id);

        // Sorted so id assignment is deterministic regardless of directory order.
        let mut files = manifest_files.to_vec();
        files.sort();

        let mut manifests: Vec<(PathBuf, RegionId)> = Vec::new();
        for path in &files {
            let dir = path.parent().unwrap_or(world_dir).to_path_buf();
            let (key, name) = parse_manifest(path)?;

            if key.as_str() == DEFAULT_REGION_SLUG {
                return Err(WorldError::ReservedRegionSlug {
                    slug: key.to_string(),
                });
            }
            if key_to_id.contains_key(&key) {
                return Err(WorldError::DuplicateRegionSlug {
                    slug: key.to_string(),
                });
            }
            // Flat in 1.0: a manifest folder may neither contain nor sit inside
            // another manifest folder.
            if manifests
                .iter()
                .any(|(other, _)| dir.starts_with(other) || other.starts_with(&dir))
            {
                return Err(WorldError::NestedRegion { path: path.clone() });
            }

            let id = RegionId::new(next_id);
            next_id = advance(next_id);
            by_id.insert(
                id,
                RegionData {
                    key: key.clone(),
                    name,
                },
            );
            key_to_id.insert(key, id);
            manifests.push((dir, id));
        }

        Ok(Self {
            manifests,
            default_id,
            regions: Regions { by_id, key_to_id },
        })
    }

    /// The region a room file in `dir` belongs to: its enclosing manifest folder
    /// (at most one, since regions are flat), or the default region.
    pub(crate) fn region_for(&self, dir: &Path) -> RegionId {
        self.manifests
            .iter()
            .find(|(folder, _)| dir.starts_with(folder))
            .map_or(self.default_id, |(_, id)| *id)
    }

    /// Consumes the binder into the loaded region registry.
    pub(crate) fn into_regions(self) -> Regions {
        self.regions
    }
}

/// Parses one `region.kdl` into its slug and optional display name.
fn parse_manifest(path: &Path) -> Result<(RegionKey, Option<RegionName>), WorldError> {
    let text = fs::read_to_string(path)?;
    let document = KdlDocument::parse(&text).map_err(|source| WorldError::Kdl {
        path: path.to_path_buf(),
        source: Box::new(source),
    })?;

    // A `region.kdl` contains exactly one `region` node; an unknown node is a
    // typo and is rejected at its source rather than silently ignored.
    let mut region_node: Option<&KdlNode> = None;
    for node in document.nodes() {
        if node.name().value() != "region" {
            return Err(WorldError::UnexpectedNode {
                context: "region file".to_owned(),
                node: node.name().value().to_owned(),
            });
        }
        if region_node.is_some() {
            return Err(WorldError::InvalidRegionManifest {
                path: path.to_path_buf(),
                reason: "a region.kdl must declare exactly one region",
            });
        }
        region_node = Some(node);
    }
    let region_node = region_node.ok_or(WorldError::InvalidRegionManifest {
        path: path.to_path_buf(),
        reason: "a region.kdl must declare exactly one region",
    })?;

    let slug_text = arg(region_node, 0).ok_or(WorldError::MissingField {
        node: "region".to_owned(),
        field: "slug",
    })?;
    let key = RegionKey::parse(slug_text).map_err(|source| WorldError::InvalidRegionSlug {
        value: slug_text.to_owned(),
        source,
    })?;

    let mut name = None;
    for child in region_node
        .children()
        .into_iter()
        .flat_map(KdlDocument::nodes)
    {
        match child.name().value() {
            "name" => name = arg(child, 0).map(RegionName::new),
            other => {
                return Err(WorldError::UnexpectedNode {
                    context: format!("region {key}"),
                    node: other.to_owned(),
                });
            }
        }
    }

    Ok((key, name))
}

/// Advances the monotonic region-id counter.
fn advance(id: NonZeroU64) -> NonZeroU64 {
    // INVARIANT: region ids count up from 1; overflowing u64 would require ~1.8e19
    // regions, unrepresentable on disk. checked_add bounds it without a silent
    // wrap; the saturating fallback keeps the counter valid in the impossible case.
    id.checked_add(1).unwrap_or(NonZeroU64::MAX)
}
