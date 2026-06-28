//! Styled text and its palette (§3.20.1, §3.20.3).
//!
//! Every player-facing string is representable as transport-neutral
//! [`StyledText`]: a flat sequence of [`Span`]s, each a string plus a
//! [`SpanStyle`] (§3.20.1.1). The representation carries no terminal escape
//! sequences (§3.20.1.2); a per-session renderer (in `mud-net`) compiles it to a
//! client's color tier.
//!
//! Builders author styled fields with a compact `{tag}…{/}` markup compiled by
//! [`compile_markup`] under a per-field [`FieldStyle`] policy, resolving named
//! colors through a [`Palette`]. The engine ships a [`Palette::baseline`] a tenant
//! palette is layered over.

mod attributes;
mod color;
mod field;
mod markup;
mod palette;
mod span;
mod style;

pub use attributes::Attributes;
pub use color::{Color, ColorParseError};
pub use field::FieldStyle;
pub use markup::{CompiledMarkup, MarkupDiagnostic, compile_markup};
pub use palette::Palette;
pub use span::{Span, StyledText};
pub use style::{RoleName, SpanStyle, Style};
