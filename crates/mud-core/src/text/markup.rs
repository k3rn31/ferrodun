//! The builder-markup compiler (§3.20.2).
//!
//! Compiles a `{tag}…{/}` markup string into flat [`StyledText`], enforcing a
//! per-field [`FieldStyle`] policy and resolving palette named colors at load.
//! Builder content is trusted (§3.20.7.2), so markup is compiled normally; player
//! input is a separate, escaped path handled downstream (§3.20.7, M1-17).
//!
//! The compiler **never aborts** (§3.20.2.2): a malformed, unknown, or
//! policy-disallowed tag keeps its inner text and records a [`MarkupDiagnostic`]
//! the caller surfaces as a structured warning. It is a tolerant single-pass
//! scanner with a style stack rather than a grammar, because the requirement is
//! to degrade every error in place and emit spans directly, not to parse-or-fail.
//!
//! Tags: `{b}` / `{i}` / `{u}` (attributes), `{fg=<name>}` / `{bg=<name>}`
//! (palette named colors — raw hex is not a palette name and so is rejected),
//! `{/}` (close the nearest open tag). Literal braces are written `{{` and `}}`.

use super::attributes::Attributes;
use super::color::Color;
use super::field::FieldStyle;
use super::palette::Palette;
use super::span::{Span, StyledText};
use super::style::Style;

/// The result of compiling builder markup: the styled text plus any diagnostics
/// raised while degrading malformed or disallowed tags.
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub struct CompiledMarkup {
    /// The compiled styled text.
    pub text: StyledText,
    /// Non-fatal problems found while compiling (§3.20.2.2).
    pub diagnostics: Vec<MarkupDiagnostic>,
}

/// A non-fatal problem found while compiling markup (§3.20.2.2). Each is degraded
/// in place — the inner text is preserved — and reported for a structured warning.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum MarkupDiagnostic {
    /// A `{fg=…}` / `{bg=…}` tag named a color the palette does not define.
    #[error("unknown color name {0:?}")]
    UnknownColor(String),
    /// A tag is not permitted for this field, or is not a recognized tag.
    #[error("disallowed or unknown tag {0:?}")]
    DisallowedTag(String),
    /// The markup was malformed (an unterminated tag, an unmatched `{/}`, or a
    /// tag left open at the end of input).
    #[error("malformed markup: {0}")]
    Malformed(String),
}

/// Compiles `input` into [`StyledText`] under the `field` policy, resolving named
/// colors through `palette`. Never fails; see the module docs.
pub fn compile_markup(input: &str, field: &FieldStyle, palette: &Palette) -> CompiledMarkup {
    let mut compiler = Compiler {
        field,
        palette,
        out: StyledText::new(),
        diagnostics: Vec::new(),
        stack: Vec::new(),
        buf: String::new(),
    };
    compiler.run(input);
    CompiledMarkup {
        text: compiler.out,
        diagnostics: compiler.diagnostics,
    }
}

/// A partial style contributed by one open tag, merged onto the field default and
/// any enclosing tags to give a span its effective style.
struct Compiler<'a> {
    field: &'a FieldStyle,
    palette: &'a Palette,
    out: StyledText,
    diagnostics: Vec<MarkupDiagnostic>,
    stack: Vec<Style>,
    buf: String,
}

impl Compiler<'_> {
    fn run(&mut self, input: &str) {
        let mut chars = input.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '{' if chars.peek() == Some(&'{') => {
                    chars.next();
                    self.buf.push('{');
                }
                '}' if chars.peek() == Some(&'}') => {
                    chars.next();
                    self.buf.push('}');
                }
                '{' => {
                    let mut tag = String::new();
                    let mut terminated = false;
                    while let Some(&next) = chars.peek() {
                        chars.next();
                        if next == '}' {
                            terminated = true;
                            break;
                        }
                        tag.push(next);
                    }
                    if terminated {
                        self.apply_tag(&tag);
                    } else {
                        self.diagnostics.push(MarkupDiagnostic::Malformed(format!(
                            "unterminated tag {{{tag}"
                        )));
                        self.buf.push('{');
                        self.buf.push_str(&tag);
                    }
                }
                _ => self.buf.push(c),
            }
        }
        self.flush();
        if !self.stack.is_empty() {
            self.diagnostics.push(MarkupDiagnostic::Malformed(format!(
                "{} unclosed tag(s)",
                self.stack.len()
            )));
        }
    }

    /// Interprets a single `{…}` tag against the field policy and palette, after
    /// flushing the text that preceded it (its style is the pre-tag style).
    fn apply_tag(&mut self, tag: &str) {
        self.flush();
        if tag == "/" {
            if self.stack.pop().is_none() {
                self.diagnostics
                    .push(MarkupDiagnostic::Malformed("unmatched {/}".to_owned()));
            }
            return;
        }
        match self.resolve_tag(tag) {
            Ok(style) => self.stack.push(style),
            Err(diagnostic) => {
                self.diagnostics.push(diagnostic);
                // Keep nesting balanced; the inner text renders with the
                // enclosing style (§3.20.2.2: the unknown tag adds nothing).
                self.stack.push(Style::new());
            }
        }
    }

    fn resolve_tag(&self, tag: &str) -> Result<Style, MarkupDiagnostic> {
        if let Some(attr) = attribute_tag(tag) {
            return if self.field.allowed_attrs.contains(attr) {
                Ok(Style::new().with_attrs(attr))
            } else {
                Err(MarkupDiagnostic::DisallowedTag(tag.to_owned()))
            };
        }
        if let Some(name) = tag.strip_prefix("fg=") {
            return self
                .color_style(name)
                .map(|color| Style::new().with_fg(color));
        }
        if let Some(name) = tag.strip_prefix("bg=") {
            return self
                .color_style(name)
                .map(|color| Style::new().with_bg(color));
        }
        Err(MarkupDiagnostic::DisallowedTag(tag.to_owned()))
    }

    fn color_style(&self, name: &str) -> Result<Color, MarkupDiagnostic> {
        if !self.field.allow_colors {
            return Err(MarkupDiagnostic::DisallowedTag(format!("color {name:?}")));
        }
        self.palette
            .color(name)
            .ok_or_else(|| MarkupDiagnostic::UnknownColor(name.to_owned()))
    }

    /// Flushes the buffered text as one span under the current effective style.
    fn flush(&mut self) {
        if self.buf.is_empty() {
            return;
        }
        let text = std::mem::take(&mut self.buf);
        let style = self.effective_style();
        if style.is_unstyled() {
            self.out.push(Span::plain(text));
        } else {
            self.out.push(Span::styled(text, style));
        }
    }

    /// The field default merged with every open tag's contribution, innermost
    /// winning for colors and attributes unioning.
    fn effective_style(&self) -> Style {
        self.stack.iter().fold(self.field.default, |acc, layer| {
            let acc = match layer.fg() {
                Some(fg) => acc.with_fg(fg),
                None => acc,
            };
            let acc = match layer.bg() {
                Some(bg) => acc.with_bg(bg),
                None => acc,
            };
            acc.with_attrs(layer.attrs())
        })
    }
}

/// Maps an attribute tag (`b`/`i`/`u`) to its [`Attributes`] flag.
fn attribute_tag(tag: &str) -> Option<Attributes> {
    match tag {
        "b" => Some(Attributes::BOLD),
        "i" => Some(Attributes::ITALIC),
        "u" => Some(Attributes::UNDERLINE),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::{Color, RoleName};

    fn palette() -> Palette {
        Palette::baseline()
    }

    #[test]
    fn plain_text_compiles_to_one_plain_span() {
        let result = compile_markup("a quiet room", &FieldStyle::DESCRIPTION, &palette());
        assert!(result.diagnostics.is_empty());
        assert_eq!(result.text, StyledText::from("a quiet room"));
    }

    #[test]
    fn a_color_tag_resolves_a_palette_name() {
        let result = compile_markup(
            "a {fg=cyan}rune{/} glows",
            &FieldStyle::DESCRIPTION,
            &palette(),
        );
        assert!(result.diagnostics.is_empty());
        let cyan = palette().color("cyan").expect("cyan in baseline");
        assert_eq!(
            result.text,
            StyledText::new()
                .plain("a ")
                .styled("rune", Style::new().with_fg(cyan))
                .plain(" glows")
        );
    }

    #[test]
    fn an_attribute_tag_styles_its_inner_text() {
        let result = compile_markup("{b}danger{/}!", &FieldStyle::DESCRIPTION, &palette());
        assert!(result.diagnostics.is_empty());
        assert_eq!(
            result.text,
            StyledText::new()
                .styled("danger", Style::new().with_attrs(Attributes::BOLD))
                .plain("!")
        );
    }

    #[test]
    fn nested_tags_combine_into_one_span_style() {
        let result = compile_markup("{b}{fg=red}x{/}{/}", &FieldStyle::DESCRIPTION, &palette());
        assert!(result.diagnostics.is_empty());
        let red = palette().color("red").expect("red in baseline");
        assert_eq!(
            result.text,
            StyledText::new().styled("x", Style::new().with_attrs(Attributes::BOLD).with_fg(red))
        );
    }

    #[test]
    fn the_title_default_bolds_the_whole_field() {
        let result = compile_markup("Town Square", &FieldStyle::TITLE, &palette());
        assert!(result.diagnostics.is_empty());
        assert_eq!(
            result.text,
            StyledText::new().styled("Town Square", Style::new().with_attrs(Attributes::BOLD))
        );
    }

    #[test]
    fn an_unknown_color_degrades_to_unstyled_with_a_diagnostic() {
        let result = compile_markup("{fg=chartreuse}x{/}", &FieldStyle::DESCRIPTION, &palette());
        assert_eq!(
            result.diagnostics,
            vec![MarkupDiagnostic::UnknownColor("chartreuse".to_owned())]
        );
        assert_eq!(result.text, StyledText::from("x"));
    }

    #[test]
    fn raw_hex_is_not_a_palette_name_and_is_rejected() {
        let result = compile_markup("{fg=#1a53ff}x{/}", &FieldStyle::DESCRIPTION, &palette());
        assert_eq!(
            result.diagnostics,
            vec![MarkupDiagnostic::UnknownColor("#1a53ff".to_owned())]
        );
        assert_eq!(result.text, StyledText::from("x"));
    }

    #[test]
    fn a_disallowed_tag_for_the_field_degrades_with_a_diagnostic() {
        // TITLE allows no colors and no attributes.
        let result = compile_markup("{fg=cyan}x{/}", &FieldStyle::TITLE, &palette());
        assert!(matches!(
            result.diagnostics.as_slice(),
            [MarkupDiagnostic::DisallowedTag(_)]
        ));
        // Inner text keeps the field default (bold), the tag adds nothing.
        assert_eq!(
            result.text,
            StyledText::new().styled("x", Style::new().with_attrs(Attributes::BOLD))
        );
    }

    #[test]
    fn an_unterminated_tag_is_malformed_and_kept_literal() {
        let result = compile_markup("a {fg=cyan rune", &FieldStyle::DESCRIPTION, &palette());
        assert!(matches!(
            result.diagnostics.as_slice(),
            [MarkupDiagnostic::Malformed(_)]
        ));
        assert_eq!(result.text, StyledText::from("a {fg=cyan rune"));
    }

    #[test]
    fn an_unmatched_close_is_malformed() {
        let result = compile_markup("x{/}", &FieldStyle::DESCRIPTION, &palette());
        assert!(matches!(
            result.diagnostics.as_slice(),
            [MarkupDiagnostic::Malformed(_)]
        ));
        assert_eq!(result.text, StyledText::from("x"));
    }

    #[test]
    fn escaped_braces_render_literally() {
        let result = compile_markup("use {{b}} for bold", &FieldStyle::DESCRIPTION, &palette());
        assert!(result.diagnostics.is_empty());
        assert_eq!(result.text, StyledText::from("use {b} for bold"));
    }

    // Roles are not authored via markup in M1 (no field allows them); they are
    // applied at engine emission sites. A role-shaped tag is just an unknown tag.
    #[test]
    fn a_role_shaped_tag_is_unknown_in_markup() {
        let result = compile_markup("{say}hi{/}", &FieldStyle::DESCRIPTION, &palette());
        assert!(matches!(
            result.diagnostics.as_slice(),
            [MarkupDiagnostic::DisallowedTag(_)]
        ));
        assert_eq!(result.text, StyledText::from("hi"));
        // Sanity: the role constant still exists for emission-site use.
        let _ = RoleName::SAY;
        let _ = Color::rgb(0, 0, 0);
    }
}
