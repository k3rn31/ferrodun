//! Slug validation shared by the durable key types (§2.2.6, §2.2.7.1).

/// Returns the first character outside the slug alphabet `[a-z0-9_-]`, or `None`
/// if every character is allowed. Shared by [`PlaceKey`](crate::PlaceKey) and
/// [`RegionKey`](crate::RegionKey) so their notion of a valid slug cannot drift
/// apart.
pub(crate) fn first_invalid_slug_char(value: &str) -> Option<char> {
    value
        .chars()
        .find(|c| !matches!(c, 'a'..='z' | '0'..='9' | '_' | '-'))
}
