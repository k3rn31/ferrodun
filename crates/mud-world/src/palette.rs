//! Loading a tenant palette from KDL (§3.20.3).
//!
//! A tenant may ship a `palette.kdl` that is layered over the engine
//! [`baseline`](mud_core::Palette::baseline) (§3.20.3.1): it overrides baseline
//! roles and adds named colors. Absent the file, the baseline is used unchanged.
//!
//! ```kdl
//! color "flame" "#ff7733"
//! role "error" fg="#ff5555"
//! role "alert" fg="flame" bg="#aa0000" bold=#true
//! ```
//!
//! Colors are authored as `#rrggbb`; a `role` color may be a hex literal or the
//! name of a `color` declared in the same file or the baseline. Attribute flags
//! use KDL 2.0 keyword booleans (`bold=#true`).

use std::fs;
use std::path::Path;

use kdl::{KdlDocument, KdlNode, KdlValue};
use mud_core::{Attributes, Color, Palette, RoleName, Style};

use crate::error::WorldError;
use crate::kdl::arg;

/// Loads a tenant palette, layering `path` (if given) over the engine baseline.
///
/// # Errors
///
/// Returns [`WorldError`] if the file cannot be read or parsed, a color is not a
/// valid `#rrggbb` value, a role references an unknown color name, a `color` or
/// `role` is missing its name, or an unexpected node appears.
pub fn load_palette(path: Option<&Path>) -> Result<Palette, WorldError> {
    let mut palette = Palette::baseline();
    let Some(path) = path else {
        return Ok(palette);
    };

    let text = fs::read_to_string(path)?;
    let document = KdlDocument::parse(&text).map_err(|source| WorldError::Kdl {
        path: path.to_path_buf(),
        source: Box::new(source),
    })?;

    // Colors first so a role may reference a color regardless of authoring order.
    for node in document.nodes() {
        if node.name().value() == "color" {
            let (name, color) = parse_color(node)?;
            palette.insert_color(name, color);
        }
    }
    for node in document.nodes() {
        match node.name().value() {
            "color" => {} // handled in the first pass
            "role" => {
                let (name, style) = parse_role(node, &palette)?;
                palette.insert_role(name, style);
            }
            other => {
                return Err(WorldError::UnexpectedNode {
                    context: "palette".to_owned(),
                    node: other.to_owned(),
                });
            }
        }
    }

    Ok(palette)
}

/// Parses a `color "<name>" "<#rrggbb>"` node.
fn parse_color(node: &KdlNode) -> Result<(String, Color), WorldError> {
    let name = arg(node, 0).ok_or(WorldError::MissingField {
        node: "color".to_owned(),
        field: "name",
    })?;
    let hex = arg(node, 1).ok_or(WorldError::MissingField {
        node: "color".to_owned(),
        field: "value",
    })?;
    let color = resolve_hex(hex)?;
    Ok((name.to_owned(), color))
}

/// Parses a `role "<name>" fg=… bg=… bold=… …` node into a [`Style`].
fn parse_role(node: &KdlNode, palette: &Palette) -> Result<(RoleName, Style), WorldError> {
    let name = arg(node, 0).ok_or(WorldError::MissingField {
        node: "role".to_owned(),
        field: "name",
    })?;

    let mut style = Style::new();
    if let Some(fg) = property(node, "fg") {
        style = style.with_fg(resolve_color_token(fg, palette)?);
    }
    if let Some(bg) = property(node, "bg") {
        style = style.with_bg(resolve_color_token(bg, palette)?);
    }
    style = style.with_attrs(attributes(node));

    Ok((RoleName::new(name), style))
}

/// Reads the boolean attribute properties of a `role` node into [`Attributes`].
fn attributes(node: &KdlNode) -> Attributes {
    let flags = [
        ("bold", Attributes::BOLD),
        ("italic", Attributes::ITALIC),
        ("underline", Attributes::UNDERLINE),
        ("blink", Attributes::BLINK),
        ("reverse", Attributes::REVERSE),
    ];
    flags
        .into_iter()
        .fold(Attributes::NONE, |acc, (key, flag)| {
            if node.get(key).and_then(KdlValue::as_bool) == Some(true) {
                acc.union(flag)
            } else {
                acc
            }
        })
}

/// A role color token is either a `#rrggbb` literal or the name of a defined color.
fn resolve_color_token(token: &str, palette: &Palette) -> Result<Color, WorldError> {
    if token.starts_with('#') {
        resolve_hex(token)
    } else {
        palette
            .color(token)
            .ok_or_else(|| WorldError::UnknownColorName {
                value: token.to_owned(),
            })
    }
}

/// Parses a `#rrggbb` literal into a [`Color`], wrapping the parse error.
fn resolve_hex(hex: &str) -> Result<Color, WorldError> {
    Color::from_hex(hex).map_err(|source| WorldError::InvalidColor {
        value: hex.to_owned(),
        source,
    })
}

/// The string value of a named property on `node`, if present.
fn property<'a>(node: &'a KdlNode, key: &str) -> Option<&'a str> {
    node.get(key).and_then(KdlValue::as_string)
}

#[cfg(test)]
mod tests {

    use std::fs;

    use tempfile::TempDir;

    use super::*;

    /// Writes `contents` to a `palette.kdl` in a fresh temp dir and loads it.
    fn load(contents: &str) -> Result<Palette, WorldError> {
        let dir = TempDir::new().expect("temp dir");
        let path = dir.path().join("palette.kdl");
        fs::write(&path, contents).expect("write palette");
        load_palette(Some(&path))
    }

    #[test]
    fn no_file_returns_the_baseline_unchanged() {
        assert_eq!(load_palette(None).expect("baseline"), Palette::baseline());
    }

    #[test]
    fn a_role_overrides_the_baseline() {
        let palette = load(r##"role "error" fg="#001122""##).expect("palette loads");
        assert_eq!(
            palette.resolve_role(&RoleName::ERROR),
            Some(Style::new().with_fg(Color::rgb(0x00, 0x11, 0x22)))
        );
    }

    #[test]
    fn a_named_color_can_be_added_and_referenced_by_a_role() {
        let palette = load("color \"flame\" \"#ff7733\"\nrole \"alert\" fg=\"flame\" bold=#true")
            .expect("palette loads");
        assert_eq!(palette.color("flame"), Some(Color::rgb(0xff, 0x77, 0x33)));
        let alert = palette.resolve_role(&RoleName::ALERT).expect("alert role");
        assert_eq!(alert.fg(), Some(Color::rgb(0xff, 0x77, 0x33)));
        assert!(alert.attrs().contains(Attributes::BOLD));
    }

    #[test]
    fn an_invalid_hex_color_is_an_error() {
        let error = load(r##"color "bad" "#zzzzzz""##).expect_err("bad hex fails");
        assert!(
            matches!(error, WorldError::InvalidColor { .. }),
            "got {error:?}"
        );
    }

    #[test]
    fn a_role_referencing_an_unknown_color_name_is_an_error() {
        let error = load(r#"role "error" fg="chartreuse""#).expect_err("unknown color fails");
        assert!(
            matches!(error, WorldError::UnknownColorName { ref value } if value == "chartreuse"),
            "got {error:?}"
        );
    }

    #[test]
    fn a_color_without_a_name_is_a_missing_field() {
        let error = load("color").expect_err("a nameless color fails");
        assert!(
            matches!(error, WorldError::MissingField { field: "name", .. }),
            "got {error:?}"
        );
    }

    #[test]
    fn an_unexpected_node_is_rejected() {
        let error = load(r#"palette "x""#).expect_err("an unknown node fails");
        assert!(
            matches!(error, WorldError::UnexpectedNode { .. }),
            "got {error:?}"
        );
    }

    #[test]
    fn malformed_kdl_is_a_structured_error() {
        let error = load("role = \"unterminated").expect_err("malformed kdl fails");
        assert!(matches!(error, WorldError::Kdl { .. }), "got {error:?}");
    }
}
