//! The shared command-token alphabet (command names, aliases, switches, set
//! keys).

/// Returns the first character outside the command-token alphabet `[a-z0-9_-]`,
/// or `None` if every character is allowed.
///
/// The alphabet is deliberately lowercase-only: it makes command matching
/// case-insensitive without a separate fold (the parser lowercases input once)
/// and forbids whitespace and `/` for free — the two characters the parser uses
/// to split a line into command, switches, and arguments.
pub(crate) fn first_invalid_token_char(value: &str) -> Option<char> {
    value
        .chars()
        .find(|c| !matches!(c, 'a'..='z' | '0'..='9' | '_' | '-'))
}
