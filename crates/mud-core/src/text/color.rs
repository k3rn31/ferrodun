//! 24-bit color (§3.20.3.4).
//!
//! Builders author colors in truecolor regardless of any client's capability;
//! per-session downsampling to a terminal's tier happens later, in the renderer
//! (§3.20.5). The domain therefore stores every color as a full 24-bit
//! [`Color`]; the only narrowing the engine knows about is the render boundary.

use std::fmt;

/// A 24-bit RGB color (§3.20.3.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[must_use]
pub struct Color {
    r: u8,
    g: u8,
    b: u8,
}

impl Color {
    /// A color from its red, green, and blue components.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// The red component.
    pub const fn r(self) -> u8 {
        self.r
    }

    /// The green component.
    pub const fn g(self) -> u8 {
        self.g
    }

    /// The blue component.
    pub const fn b(self) -> u8 {
        self.b
    }

    /// Parses a `#rrggbb` hex string (the form palettes are authored in,
    /// §3.20.3.1) into a [`Color`].
    ///
    /// # Errors
    ///
    /// Returns [`ColorParseError::MissingHash`] if the string does not start with
    /// `#`, [`ColorParseError::BadLength`] if it is not exactly six hex digits
    /// after the `#`, or [`ColorParseError::InvalidDigit`] for a non-hex digit.
    pub fn from_hex(text: &str) -> Result<Self, ColorParseError> {
        let digits = text.strip_prefix('#').ok_or(ColorParseError::MissingHash)?;
        if let Some(bad) = digits.chars().find(|c| !c.is_ascii_hexdigit()) {
            return Err(ColorParseError::InvalidDigit(bad));
        }
        // Hex digits are ASCII, so byte length equals character length here.
        let pair = |start: usize| {
            digits
                .get(start..start + 2)
                .and_then(|s| u8::from_str_radix(s, 16).ok())
        };
        match (digits.len(), pair(0), pair(2), pair(4)) {
            (6, Some(r), Some(g), Some(b)) => Ok(Self::rgb(r, g, b)),
            (len, _, _, _) => Err(ColorParseError::BadLength(len)),
        }
    }
}

impl fmt::Display for Color {
    /// Renders the canonical `#rrggbb` authoring form.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

/// The reason a string could not be parsed into a [`Color`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ColorParseError {
    /// The string did not start with `#`.
    #[error("color must start with '#'")]
    MissingHash,
    /// The string was not exactly six hex digits after the `#`.
    #[error("color must be #rrggbb (6 hex digits), got {0}")]
    BadLength(usize),
    /// The string contained a character outside `[0-9a-fA-F]`.
    #[error("color contains a non-hex character {0:?}")]
    InvalidDigit(char),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb_round_trips_through_components() {
        let color = Color::rgb(0x1a, 0x53, 0xff);
        assert_eq!((color.r(), color.g(), color.b()), (0x1a, 0x53, 0xff));
    }

    #[test]
    fn from_hex_parses_a_six_digit_color() {
        assert_eq!(Color::from_hex("#1a53ff"), Ok(Color::rgb(0x1a, 0x53, 0xff)));
        assert_eq!(Color::from_hex("#FFFFFF"), Ok(Color::rgb(255, 255, 255)));
    }

    #[test]
    fn from_hex_round_trips_through_display() {
        let color = Color::rgb(0xff, 0x55, 0x55);
        assert_eq!(color.to_string(), "#ff5555");
        assert_eq!(Color::from_hex(&color.to_string()), Ok(color));
    }

    #[test]
    fn from_hex_rejects_a_missing_hash() {
        assert_eq!(Color::from_hex("1a53ff"), Err(ColorParseError::MissingHash));
    }

    #[test]
    fn from_hex_rejects_a_wrong_length() {
        assert_eq!(Color::from_hex("#abc"), Err(ColorParseError::BadLength(3)));
        assert_eq!(
            Color::from_hex("#1a53ff00"),
            Err(ColorParseError::BadLength(8))
        );
    }

    #[test]
    fn from_hex_rejects_a_non_hex_digit() {
        assert_eq!(
            Color::from_hex("#1a53fz"),
            Err(ColorParseError::InvalidDigit('z'))
        );
    }
}
