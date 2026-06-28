//! The error type for loading a tenant's authored world.

use std::path::PathBuf;

use mud_core::{PlaceKeyError, RegionKeyError};

/// A failure while loading a tenant's configuration or world files.
///
/// Third-party parser and configuration errors are boxed so neither `kdl` nor
/// `figment` leaks across this crate's public API.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum WorldError {
    /// A world directory or file could not be read.
    #[error("world i/o error: {0}")]
    Io(#[from] std::io::Error),

    /// A KDL file failed to parse.
    #[error("failed to parse {path}: {source}")]
    Kdl {
        /// The file that failed to parse.
        path: PathBuf,
        /// The boxed underlying `kdl` error.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// The tenant `config.toml` could not be loaded or deserialized.
    #[error("failed to load tenant config: {0}")]
    Config(#[source] Box<dyn std::error::Error + Send + Sync>),

    /// A room slug was authored more than once across the world files.
    #[error("duplicate room slug: {slug}")]
    DuplicateSlug {
        /// The slug that appeared on more than one room.
        slug: String,
    },

    /// A room's slug is not a valid [`PlaceKey`](mud_core::PlaceKey).
    #[error("invalid room slug {value:?}: {source}")]
    InvalidSlug {
        /// The offending slug text.
        value: String,
        /// Why it is not a valid slug.
        #[source]
        source: PlaceKeyError,
    },

    /// An exit names a target room slug that no room defines.
    #[error("room {from:?} has a {direction} exit to unknown room {to:?}")]
    DanglingExit {
        /// The slug of the room declaring the exit.
        from: String,
        /// The exit direction.
        direction: String,
        /// The unknown target slug.
        to: String,
    },

    /// The configured `start_room` slug names no loaded room.
    #[error("start_room {slug:?} names no loaded room")]
    UnknownStartRoom {
        /// The configured start-room slug.
        slug: String,
    },

    /// An exit direction is not one of north/east/south/west/up/down.
    #[error("unknown exit direction: {value:?}")]
    UnknownDirection {
        /// The unrecognized direction text.
        value: String,
    },

    /// A required field was absent from a node.
    #[error("{node}: missing required {field}")]
    MissingField {
        /// The node that is missing a field (a room slug, or a node name).
        node: String,
        /// The name of the missing field.
        field: &'static str,
    },

    /// A KDL node was not recognized where it appeared — an unknown top-level
    /// node in a world file, or an unknown child of a `room`. Rejected rather
    /// than ignored so an authoring typo fails at its source.
    #[error("unexpected node {node:?} in {context}")]
    UnexpectedNode {
        /// Where the node appeared (`"world file"` or `"room <slug>"`).
        context: String,
        /// The unrecognized node name.
        node: String,
    },

    /// A configured path escaped its tenant directory — it was absolute or
    /// contained a `..` component. Tenant content must stay within the tenant
    /// folder (§5), so such a path is rejected rather than followed.
    #[error("config path for {field} escapes the tenant directory: {path}")]
    EscapingPath {
        /// The config field that held the offending path (e.g. `banner`).
        field: &'static str,
        /// The offending path as authored.
        path: std::path::PathBuf,
    },

    /// A region manifest's slug is not a valid [`RegionKey`](mud_core::RegionKey).
    #[error("invalid region slug {value:?}: {source}")]
    InvalidRegionSlug {
        /// The offending slug text.
        value: String,
        /// Why it is not a valid slug.
        #[source]
        source: RegionKeyError,
    },

    /// Two region manifests authored the same slug (§2.2.7.1: a `RegionKey` is
    /// unique within a tenant).
    #[error("duplicate region slug: {slug}")]
    DuplicateRegionSlug {
        /// The slug that appeared on more than one manifest.
        slug: String,
    },

    /// A room file is covered by no region manifest. Every `Place` MUST belong to
    /// a declared Region (§2.2.7.3), so a room outside every region's folder is
    /// rejected rather than placed in an unconfigurable fallback.
    #[error("room file {path} is under no region; every room must live in a region subfolder")]
    RoomOutsideRegion {
        /// The offending room file.
        path: PathBuf,
    },

    /// A region manifest sits at the world directory's root. Regions MUST be
    /// declared in a subfolder (§2.2.7.3); the root slot is reserved for a future
    /// tenant-wide defaults manifest.
    #[error(
        "region manifest at the world root is not allowed; declare regions in subfolders: {path}"
    )]
    RegionManifestAtWorldRoot {
        /// The offending manifest path.
        path: PathBuf,
    },

    /// A region manifest sits under another region's folder. Regions are flat in
    /// 1.0 (§2.2.7.3), so nesting is rejected at load.
    #[error("nested region manifest is not allowed in 1.0: {path}")]
    NestedRegion {
        /// The offending manifest path.
        path: PathBuf,
    },

    /// A `region.kdl` did not declare exactly one region.
    #[error("invalid region manifest {path}: {reason}")]
    InvalidRegionManifest {
        /// The offending manifest path.
        path: PathBuf,
        /// Why the manifest is malformed.
        reason: &'static str,
    },
}
