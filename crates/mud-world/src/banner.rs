//! Loading the pre-login welcome banner from KDL (§3.19.1).
//!
//! The banner is a single KDL node with one string argument:
//!
//! ```kdl
//! banner "Welcome to Ferrodun.\nType `register` or `login`."
//! ```

use std::fs;
use std::path::Path;

use kdl::{KdlDocument, KdlValue};

use crate::error::WorldError;

/// Loads the welcome banner text from the KDL file at `path`.
///
/// # Errors
///
/// Returns [`WorldError`] if the file cannot be read or parsed, or if it has no
/// `banner` node carrying a string.
pub fn load_banner(path: &Path) -> Result<String, WorldError> {
    let text = fs::read_to_string(path)?;
    let document = KdlDocument::parse(&text).map_err(|source| WorldError::Kdl {
        path: path.to_path_buf(),
        source: Box::new(source),
    })?;

    document
        .get("banner")
        .and_then(|node| node.get(0))
        .and_then(KdlValue::as_string)
        .map(str::to_owned)
        .ok_or(WorldError::MissingField {
            node: "banner".to_owned(),
            field: "text",
        })
}
