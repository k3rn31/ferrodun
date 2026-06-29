//! Command identity: the canonical name, its aliases, and accepted switches
//! (§2.7 step 5, §3.14.5.1).

use std::fmt;

use crate::token::first_invalid_token_char;

/// A canonical or alias command name such as `look` or `north` (§3.14.5.1).
///
/// The canonical name is locale-invariant; localized aliases (§3.14.5.2) are
/// additional `CommandName`s contributed by a locale source at merge time. A
/// newtype so a command token can never be confused with a raw argument or
/// switch. Parsed once at the boundary into the lowercase token alphabet, so
/// inner code never re-validates (§3.14.4.4 spirit).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[must_use]
pub struct CommandName(String);

impl CommandName {
    /// Parses a raw token into a `CommandName`.
    ///
    /// # Errors
    ///
    /// Returns [`CommandNameError::Empty`] for an empty token, or
    /// [`CommandNameError::InvalidCharacter`] for any character outside the
    /// lowercase alphabet `[a-z0-9_-]` (uppercase included — names are
    /// lowercase-canonical).
    pub fn parse(value: &str) -> Result<Self, CommandNameError> {
        if value.is_empty() {
            return Err(CommandNameError::Empty);
        }
        if let Some(bad) = first_invalid_token_char(value) {
            return Err(CommandNameError::InvalidCharacter(bad));
        }
        Ok(Self(value.to_owned()))
    }

    /// The name text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CommandName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A failure parsing a [`CommandName`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CommandNameError {
    #[error("command name must not be empty")]
    Empty,
    #[error("command name contains an invalid character {0:?} (allowed: a-z, 0-9, '_', '-')")]
    InvalidCharacter(char),
}

/// A command switch such as the `quiet` in `look/quiet` (§2.7 step 5).
///
/// Shares the command-token alphabet so a switch likewise carries no whitespace
/// or `/`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[must_use]
pub struct Switch(String);

impl Switch {
    /// Parses a raw token into a `Switch`.
    ///
    /// # Errors
    ///
    /// Returns [`SwitchError::Empty`] for an empty token, or
    /// [`SwitchError::InvalidCharacter`] for any character outside the lowercase
    /// alphabet `[a-z0-9_-]`.
    pub fn parse(value: &str) -> Result<Self, SwitchError> {
        if value.is_empty() {
            return Err(SwitchError::Empty);
        }
        if let Some(bad) = first_invalid_token_char(value) {
            return Err(SwitchError::InvalidCharacter(bad));
        }
        Ok(Self(value.to_owned()))
    }

    /// The switch text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Switch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A failure parsing a [`Switch`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SwitchError {
    #[error("switch must not be empty")]
    Empty,
    #[error("switch contains an invalid character {0:?} (allowed: a-z, 0-9, '_', '-')")]
    InvalidCharacter(char),
}

/// A command definition: the metadata the parser and merge need (§2.7 steps 4–5).
///
/// M1-15 is the model + parser only. A command carries no `run`/dispatch handle
/// yet — dispatch and lock-checking arrive with the World pipeline (M1-16), and
/// the built-in command bodies with M1-17.
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub struct Command {
    name: CommandName,
    aliases: Vec<CommandName>,
    switches: Vec<Switch>,
}

impl Command {
    /// A command answering to `name`, with no aliases or switches yet.
    pub fn new(name: CommandName) -> Self {
        Self {
            name,
            aliases: Vec::new(),
            switches: Vec::new(),
        }
    }

    /// Adds an alias the command also answers to (e.g. `n` for `north`).
    pub fn with_alias(mut self, alias: CommandName) -> Self {
        self.aliases.push(alias);
        self
    }

    /// Declares a switch the command accepts.
    pub fn with_switch(mut self, switch: Switch) -> Self {
        self.switches.push(switch);
        self
    }

    /// The canonical, locale-invariant name (§3.14.5.1).
    pub fn name(&self) -> &CommandName {
        &self.name
    }

    /// The aliases, in declaration order.
    pub fn aliases(&self) -> &[CommandName] {
        &self.aliases
    }

    /// The declared accepted switches.
    pub fn switches(&self) -> &[Switch] {
        &self.switches
    }

    /// Every name the command answers to: the canonical name first, then each
    /// alias. These are the keys under which the parser indexes the command.
    pub fn names(&self) -> impl Iterator<Item = &CommandName> {
        std::iter::once(&self.name).chain(self.aliases.iter())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(value: &str) -> CommandName {
        CommandName::parse(value).expect("valid command name")
    }

    #[test]
    fn parses_a_lowercase_name() {
        assert_eq!(name("north").as_str(), "north");
    }

    #[test]
    fn rejects_an_empty_name() {
        assert_eq!(CommandName::parse(""), Err(CommandNameError::Empty));
    }

    #[test]
    fn rejects_an_uppercase_name() {
        assert_eq!(
            CommandName::parse("Look"),
            Err(CommandNameError::InvalidCharacter('L'))
        );
    }

    #[test]
    fn rejects_a_name_with_whitespace_or_slash() {
        assert_eq!(
            CommandName::parse("go north"),
            Err(CommandNameError::InvalidCharacter(' '))
        );
        assert_eq!(
            CommandName::parse("look/quiet"),
            Err(CommandNameError::InvalidCharacter('/'))
        );
    }

    #[test]
    fn parses_a_switch() {
        assert_eq!(Switch::parse("quiet").expect("valid").as_str(), "quiet");
    }

    #[test]
    fn rejects_an_invalid_switch() {
        assert_eq!(Switch::parse(""), Err(SwitchError::Empty));
        assert_eq!(
            Switch::parse("Quiet"),
            Err(SwitchError::InvalidCharacter('Q'))
        );
    }

    #[test]
    fn names_yields_canonical_then_aliases() {
        let command = Command::new(name("north"))
            .with_alias(name("n"))
            .with_alias(name("forward"));

        let names: Vec<&str> = command.names().map(CommandName::as_str).collect();
        assert_eq!(names, vec!["north", "n", "forward"]);
    }

    #[test]
    fn builders_record_aliases_and_switches() {
        let command = Command::new(name("look"))
            .with_alias(name("l"))
            .with_switch(Switch::parse("quiet").expect("valid"));

        assert_eq!(command.name().as_str(), "look");
        assert_eq!(command.aliases(), &[name("l")]);
        assert_eq!(
            command.switches(),
            &[Switch::parse("quiet").expect("valid")]
        );
    }
}
