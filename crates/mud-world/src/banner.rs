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

#[cfg(test)]
mod tests {

    use std::fs;

    use tempfile::TempDir;

    use super::*;

    /// Writes `contents` to a `welcome.kdl` in a fresh temp dir and returns both,
    /// keeping the dir alive for the duration of the test.
    fn banner_file(contents: &str) -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("welcome.kdl");
        fs::write(&path, contents).expect("write banner");
        (dir, path)
    }

    #[test]
    fn reads_the_banner_text() {
        let (_dir, path) = banner_file("banner \"Welcome, traveler.\"");
        assert_eq!(
            load_banner(&path).expect("banner loads"),
            "Welcome, traveler."
        );
    }

    #[test]
    fn preserves_escaped_newlines_in_the_banner() {
        let (_dir, path) = banner_file(r#"banner "line one\nline two""#);
        assert_eq!(
            load_banner(&path).expect("banner loads"),
            "line one\nline two"
        );
    }

    #[test]
    fn a_missing_file_is_an_io_error() {
        let dir = TempDir::new().expect("temp dir");
        let error = load_banner(&dir.path().join("absent.kdl")).expect_err("missing file fails");
        assert!(matches!(error, WorldError::Io(_)), "got {error:?}");
    }

    #[test]
    fn malformed_kdl_is_a_structured_error() {
        let (_dir, path) = banner_file("banner = \"unterminated");
        let error = load_banner(&path).expect_err("malformed kdl fails");
        assert!(matches!(error, WorldError::Kdl { .. }), "got {error:?}");
    }

    #[test]
    fn a_banner_node_without_text_is_a_missing_field() {
        let (_dir, path) = banner_file("banner");
        let error = load_banner(&path).expect_err("a bannerless node fails");
        assert!(
            matches!(error, WorldError::MissingField { field: "text", .. }),
            "got {error:?}"
        );
    }

    #[test]
    fn a_non_string_banner_argument_is_a_missing_field() {
        let (_dir, path) = banner_file("banner 42");
        let error = load_banner(&path).expect_err("a non-string banner fails");
        assert!(
            matches!(error, WorldError::MissingField { field: "text", .. }),
            "got {error:?}"
        );
    }

    #[test]
    fn no_banner_node_is_a_missing_field() {
        let (_dir, path) = banner_file("greeting \"hi\"");
        let error = load_banner(&path).expect_err("a file without a banner node fails");
        assert!(
            matches!(error, WorldError::MissingField { field: "text", .. }),
            "got {error:?}"
        );
    }
}
