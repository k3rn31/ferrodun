//! Loading authored regions from `region.kdl` manifests (§2.2.7).
//!
//! A `region.kdl` at a world folder's root declares the durable [`RegionKey`] that
//! every room in that subtree belongs to (§2.2.7.3). Folder *names* carry no
//! identity — the slug inside the manifest does — so a folder may be renamed or
//! moved without changing a region. Regions are mandatory: every room MUST be
//! covered by a manifest (§2.2.7.3), so a room under no `region.kdl` is rejected,
//! as is a manifest at the world-directory root (that slot is reserved for a
//! future tenant-wide defaults manifest). Regions are flat in 1.0: a manifest
//! nested under another region's folder is rejected at load.
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
/// Every region is explicitly authored by a `region.kdl` manifest; there is no
/// implicit fallback region (§2.2.7.3). A tenant that declares no region
/// manifests has no regions.
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
    /// The handle a slug names, if a manifest defines it.
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

    /// The number of loaded regions.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// Whether no region manifests were loaded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }
}

/// Binds each room's folder to the [`RegionId`] of its enclosing manifest folder.
/// Built once per world load and consumed into the [`Regions`] registry.
pub(crate) struct RegionBinder {
    /// `(governed folder, region id)`. Regions are flat (§2.2.7.3), so a folder
    /// has at most one enclosing manifest — no ordering is needed to disambiguate.
    manifests: Vec<(PathBuf, RegionId)>,
    regions: Regions,
}

impl RegionBinder {
    /// Parses every region manifest under `world_dir` and prepares room binding.
    ///
    /// # Errors
    ///
    /// Returns [`WorldError`] if a manifest cannot be read or parsed, declares an
    /// invalid / duplicate slug, sits at the world root, is nested under another
    /// region, or does not declare exactly one region.
    pub(crate) fn load(manifest_files: &[PathBuf], world_dir: &Path) -> Result<Self, WorldError> {
        let mut next_id = NonZeroU64::MIN;
        let mut by_id = HashMap::new();
        let mut key_to_id = HashMap::new();

        // Sorted so id assignment is deterministic regardless of directory order.
        let mut files = manifest_files.to_vec();
        files.sort();

        let mut manifests: Vec<(PathBuf, RegionId)> = Vec::new();
        for path in &files {
            let dir = path.parent().unwrap_or(world_dir).to_path_buf();

            // A manifest must declare a region in a subfolder; the world root is
            // reserved for a future tenant-wide defaults manifest (§2.2.7.3).
            if dir == world_dir {
                return Err(WorldError::RegionManifestAtWorldRoot { path: path.clone() });
            }

            let (key, name) = parse_manifest(path)?;

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
            regions: Regions { by_id, key_to_id },
        })
    }

    /// The region a room file in `dir` belongs to: its enclosing manifest folder
    /// (at most one, since regions are flat), or [`None`] if no manifest covers it.
    pub(crate) fn region_for(&self, dir: &Path) -> Option<RegionId> {
        self.manifests
            .iter()
            .find(|(folder, _)| dir.starts_with(folder))
            .map(|(_, id)| *id)
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
