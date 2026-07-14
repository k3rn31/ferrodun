//! Styled text: a flat sequence of styled spans (§3.20.1.1).
//!
//! This is the engine's transport-neutral representation of all player-facing
//! output. It carries no escape sequences (§3.20.1.2); a per-session renderer
//! compiles it to a terminal tier or, later, to structured spans for a webclient.

use super::style::{RoleName, SpanStyle, Style};

/// A run of text sharing one [`SpanStyle`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[must_use]
pub struct Span {
    text: String,
    style: SpanStyle,
}

impl Span {
    /// An unstyled span.
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            style: SpanStyle::Plain,
        }
    }

    /// A span carrying an already-resolved [`Style`].
    pub fn styled(text: impl Into<String>, style: Style) -> Self {
        Self {
            text: text.into(),
            style: SpanStyle::Direct(style),
        }
    }

    /// A span carrying a semantic role, resolved at render time (§3.20.4.2).
    pub fn role(text: impl Into<String>, role: RoleName) -> Self {
        Self {
            text: text.into(),
            style: SpanStyle::Role(role),
        }
    }

    /// The span's text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// How the span is styled.
    pub fn style(&self) -> &SpanStyle {
        &self.style
    }
}

/// A flat sequence of [`Span`]s — the representation every player-facing string
/// is built from (§3.20.1.1).
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[must_use]
pub struct StyledText {
    spans: Vec<Span>,
}

impl StyledText {
    /// Empty styled text.
    pub fn new() -> Self {
        Self::default()
    }

    /// The spans in order.
    pub fn spans(&self) -> &[Span] {
        &self.spans
    }

    /// Appends `span`, returning `&mut self` for chaining.
    pub fn push(&mut self, span: Span) -> &mut Self {
        self.spans.push(span);
        self
    }

    /// Appends an unstyled span.
    pub fn plain(mut self, text: impl Into<String>) -> Self {
        self.spans.push(Span::plain(text));
        self
    }

    /// Appends a directly-styled span.
    pub fn styled(mut self, text: impl Into<String>, style: Style) -> Self {
        self.spans.push(Span::styled(text, style));
        self
    }

    /// Appends a role-styled span.
    pub fn role(mut self, text: impl Into<String>, role: RoleName) -> Self {
        self.spans.push(Span::role(text, role));
        self
    }

    /// The concatenated text with all styling dropped.
    ///
    /// The plain-text projection of styled output — used where only the
    /// characters matter (logging, length checks, and the bridge for code that
    /// has not yet adopted styled output).
    #[must_use]
    pub fn to_plain_string(&self) -> String {
        self.spans.iter().map(Span::text).collect()
    }
}

impl From<&str> for StyledText {
    fn from(text: &str) -> Self {
        StyledText::new().plain(text)
    }
}

impl From<String> for StyledText {
    fn from(text: String) -> Self {
        StyledText::new().plain(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::Color;

    #[test]
    fn builders_append_spans_in_order() {
        let text = StyledText::new()
            .plain("a ")
            .styled("b", Style::new().with_fg(Color::rgb(1, 2, 3)))
            .role("c", RoleName::SAY);

        assert_eq!(
            text.spans(),
            &[
                Span::plain("a "),
                Span::styled("b", Style::new().with_fg(Color::rgb(1, 2, 3))),
                Span::role("c", RoleName::SAY),
            ]
        );
    }

    #[test]
    fn to_plain_string_concatenates_text_dropping_style() {
        let text = StyledText::new()
            .role("Alice", RoleName::SAY)
            .plain(" waves");
        assert_eq!(text.to_plain_string(), "Alice waves");
    }

    #[test]
    fn from_str_is_a_single_plain_span() {
        let text = StyledText::from("hello");
        assert_eq!(text.spans(), &[Span::plain("hello")]);
    }
}
