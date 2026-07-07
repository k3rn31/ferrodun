//! The CmdSet model and the §2.7-step-4 merge.
//!
//! A [`CmdSet`] is a named, prioritised bundle of [`Command`]s with a
//! [`MergeType`]. [`merge`](CmdSet::merge) folds several sets into the single
//! [`CommandTable`] the parser drives.
//!
//! Collisions are resolved **per canonical command name** (§2.7 step 4):
//! - a [`Remove`](MergeType::Remove) contributor deletes the name regardless of
//!   precedence (the strongest, most explicit override);
//! - otherwise a [`Replace`](MergeType::Replace) contributor wins regardless of
//!   precedence;
//! - otherwise (every contributor is [`Union`](MergeType::Union)) the
//!   highest-[`Priority`] contributor wins, ties broken by input order.
//!
//! The named source layers of §2.7 step 4 (account → puppet → containers →
//! location → channels) are **not** modelled here: they are mapped onto
//! [`Priority`] values by the World pipeline (M1-16). This crate only knows
//! "higher priority wins".

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

use crate::command::{Command, CommandName};
use crate::parser::CommandTable;
use crate::token::first_invalid_token_char;
use crate::trie::PrefixTrie;

/// A merge priority. Higher beats lower when two [`Union`](MergeType::Union)
/// sets contribute the same command name (§2.7 step 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[must_use]
pub struct Priority(i32);

impl Priority {
    /// The neutral default priority (`0`).
    pub const DEFAULT: Self = Self(0);

    /// A priority from a raw level. Higher wins.
    pub const fn new(level: i32) -> Self {
        Self(level)
    }
}

impl Default for Priority {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Whether a token claim is a command's canonical name or one of its aliases.
///
/// Ordered so a canonical name outranks an alias at equal [`Priority`] when the
/// same token is claimed by two commands (§2.7 step 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum TokenKind {
    Alias,
    Canonical,
}

/// How a [`CmdSet`] merges onto the others (§2.7 step 4).
///
/// A closed, normative set — left exhaustive so every match over it is total.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub enum MergeType {
    /// Combine; same-name collisions resolve by [`Priority`].
    Union,
    /// Override the named command regardless of precedence.
    Replace,
    /// Delete the named command regardless of precedence; contribute nothing.
    Remove,
}

/// A lightweight identifier for a set, for tracing and authoring reference.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[must_use]
pub struct CmdSetKey(String);

impl CmdSetKey {
    /// Parses a raw identifier into a `CmdSetKey`.
    ///
    /// # Errors
    ///
    /// Returns [`CmdSetKeyError::Empty`] for an empty identifier, or
    /// [`CmdSetKeyError::InvalidCharacter`] for any character outside the
    /// alphabet `[a-z0-9_-]`.
    pub fn parse(value: &str) -> Result<Self, CmdSetKeyError> {
        if value.is_empty() {
            return Err(CmdSetKeyError::Empty);
        }
        if let Some(bad) = first_invalid_token_char(value) {
            return Err(CmdSetKeyError::InvalidCharacter(bad));
        }
        Ok(Self(value.to_owned()))
    }

    /// The key text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for CmdSetKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A failure parsing a [`CmdSetKey`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CmdSetKeyError {
    #[error("cmdset key must not be empty")]
    Empty,
    #[error("cmdset key contains an invalid character {0:?} (allowed: a-z, 0-9, '_', '-')")]
    InvalidCharacter(char),
}

/// A named, prioritised bundle of commands with a merge rule (§2.7 step 4).
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub struct CmdSet {
    key: CmdSetKey,
    priority: Priority,
    mergetype: MergeType,
    commands: Vec<Command>,
}

impl CmdSet {
    /// A set identified by `key`, merging via `mergetype` at `priority`.
    pub fn new(
        key: CmdSetKey,
        priority: Priority,
        mergetype: MergeType,
        commands: Vec<Command>,
    ) -> Self {
        Self {
            key,
            priority,
            mergetype,
            commands,
        }
    }

    /// The set's identifier.
    pub fn key(&self) -> &CmdSetKey {
        &self.key
    }

    /// The set's merge priority.
    pub fn priority(&self) -> Priority {
        self.priority
    }

    /// The set's merge rule.
    pub fn mergetype(&self) -> MergeType {
        self.mergetype
    }

    /// The commands the set contributes.
    pub fn commands(&self) -> &[Command] {
        &self.commands
    }

    /// Merges `sets` into the single [`CommandTable`] the parser drives (§2.7
    /// step 4).
    ///
    /// Resolution is per canonical command name and driven by [`Priority`] and
    /// [`MergeType`], not by the order of `sets` — except that equal-priority
    /// [`Union`](MergeType::Union) ties favour the earlier set (see the module
    /// docs). The resulting table lists surviving commands in canonical-name
    /// order, and alias ownership is settled deterministically, so the table is
    /// fully determined by its inputs.
    pub fn merge(sets: &[CmdSet]) -> CommandTable {
        let mut names: BTreeSet<&CommandName> = BTreeSet::new();
        for set in sets {
            for command in &set.commands {
                names.insert(command.name());
            }
        }

        let resolved: Vec<(Priority, Command)> = names
            .into_iter()
            .filter_map(|name| resolve_name(sets, name))
            .map(|(priority, command)| (priority, command.clone()))
            .collect();

        // Settle token ownership before consuming `resolved` for the command list.
        let trie = settle_token_ownership(&resolved);
        let commands = resolved.into_iter().map(|(_, command)| command).collect();
        CommandTable::new(commands, trie)
    }
}

/// Settles which resolved command owns each token and returns the prefix trie
/// mapping every surviving token to its owning command's index (§2.7 step 4).
///
/// For a token claimed by several commands the strongest claim wins: higher
/// [`Priority`], then a canonical name over an alias ([`TokenKind`]), then the
/// earlier command in canonical-name order. A losing command keeps every token
/// it still owns.
fn settle_token_ownership(resolved: &[(Priority, Command)]) -> PrefixTrie {
    // `Reverse(index)` makes the lower index outrank at equal precedence.
    let mut owner: BTreeMap<&str, (Priority, TokenKind, Reverse<usize>)> = BTreeMap::new();
    for (index, (priority, command)) in resolved.iter().enumerate() {
        let claims = std::iter::once((command.name().as_str(), TokenKind::Canonical)).chain(
            command
                .aliases()
                .iter()
                .map(|alias| (alias.as_str(), TokenKind::Alias)),
        );
        for (token, kind) in claims {
            let rank = (*priority, kind, Reverse(index));
            owner
                .entry(token)
                .and_modify(|current| {
                    if rank > *current {
                        *current = rank;
                    }
                })
                .or_insert(rank);
        }
    }

    let mut trie = PrefixTrie::default();
    for (token, &(_, _, Reverse(index))) in &owner {
        trie.insert(token, index);
    }
    trie
}

/// Resolves the winning command for `name`, paired with the [`Priority`] of the
/// source that won it, or `None` when a [`Remove`](MergeType::Remove) deletes
/// it. The priority then settles alias collisions when the table is built.
fn resolve_name<'a>(sets: &'a [CmdSet], name: &CommandName) -> Option<(Priority, &'a Command)> {
    let mut best_union: Option<(Priority, &Command)> = None;
    let mut best_replace: Option<(Priority, &Command)> = None;

    for set in sets {
        for command in &set.commands {
            if command.name() != name {
                continue;
            }
            match set.mergetype {
                MergeType::Remove => return None,
                MergeType::Replace => keep_higher(&mut best_replace, set.priority, command),
                MergeType::Union => keep_higher(&mut best_union, set.priority, command),
            }
        }
    }

    best_replace.or(best_union)
}

/// Keeps `candidate` only when it strictly outranks the current best, so equal
/// priorities preserve the earlier (input-order) winner.
fn keep_higher<'a>(
    best: &mut Option<(Priority, &'a Command)>,
    priority: Priority,
    candidate: &'a Command,
) {
    let outranked = match best {
        Some((current, _)) => priority > *current,
        None => true,
    };
    if outranked {
        *best = Some((priority, candidate));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(value: &str) -> CommandName {
        CommandName::parse(value).expect("valid command name")
    }

    fn command(value: &str) -> Command {
        Command::new(name(value))
    }

    fn set(key: &str, priority: i32, mergetype: MergeType, commands: Vec<Command>) -> CmdSet {
        CmdSet::new(
            CmdSetKey::parse(key).expect("valid key"),
            Priority::new(priority),
            mergetype,
            commands,
        )
    }

    fn names(table: &CommandTable) -> Vec<String> {
        table.iter().map(|c| c.name().as_str().to_owned()).collect()
    }

    #[test]
    fn union_of_disjoint_sets_keeps_every_command() {
        let table = CmdSet::merge(&[
            set("a", 0, MergeType::Union, vec![command("look")]),
            set("b", 0, MergeType::Union, vec![command("say")]),
        ]);

        assert_eq!(names(&table), vec!["look", "say"]);
    }

    #[test]
    fn union_collision_resolves_to_the_higher_priority() {
        // Both define `look`; the high-priority set's command wins. We tell them
        // apart by giving the winner an alias.
        let winner = Command::new(name("look")).with_alias(name("l"));
        let table = CmdSet::merge(&[
            set("low", 1, MergeType::Union, vec![command("look")]),
            set("high", 9, MergeType::Union, vec![winner]),
        ]);

        let look = table.get(&name("look")).expect("look survives");
        assert_eq!(look.aliases(), &[name("l")]);
    }

    #[test]
    fn equal_priority_union_keeps_the_earlier_set() {
        let first = Command::new(name("look")).with_alias(name("first"));
        let second = Command::new(name("look")).with_alias(name("second"));
        let table = CmdSet::merge(&[
            set("a", 5, MergeType::Union, vec![first]),
            set("b", 5, MergeType::Union, vec![second]),
        ]);

        let look = table.get(&name("look")).expect("look survives");
        assert_eq!(look.aliases(), &[name("first")]);
    }

    #[test]
    fn replace_overrides_a_higher_priority_union() {
        // The Replace set sits at a *lower* priority yet still wins the name.
        let replacement = Command::new(name("look")).with_alias(name("replaced"));
        let table = CmdSet::merge(&[
            set("union", 100, MergeType::Union, vec![command("look")]),
            set("replace", 1, MergeType::Replace, vec![replacement]),
        ]);

        let look = table.get(&name("look")).expect("look survives");
        assert_eq!(look.aliases(), &[name("replaced")]);
    }

    #[test]
    fn remove_deletes_the_command_regardless_of_priority() {
        let table = CmdSet::merge(&[
            set(
                "union",
                100,
                MergeType::Union,
                vec![command("look"), command("say")],
            ),
            set("remove", 1, MergeType::Remove, vec![command("look")]),
        ]);

        assert_eq!(names(&table), vec!["say"]);
        assert!(table.get(&name("look")).is_none());
    }

    #[test]
    fn alias_collision_resolves_to_the_higher_priority_command() {
        // Two surviving commands both claim alias `x`; the higher-priority set's
        // command keeps it, and the loser stays reachable by its own name.
        let winner = Command::new(name("examine")).with_alias(name("x"));
        let loser = Command::new(name("exits")).with_alias(name("x"));
        let table = CmdSet::merge(&[
            set("low", 1, MergeType::Union, vec![loser]),
            set("high", 9, MergeType::Union, vec![winner]),
        ]);

        assert_eq!(
            table.get(&name("x")).map(|c| c.name().as_str()),
            Some("examine")
        );
        assert!(table.get(&name("exits")).is_some());
    }

    #[test]
    fn a_canonical_name_beats_another_commands_alias_at_equal_priority() {
        // `go` is one command's canonical name and another's alias at the same
        // precedence; the canonical name owns the token.
        let canonical = Command::new(name("go"));
        let aliaser = Command::new(name("goto")).with_alias(name("go"));
        let table = CmdSet::merge(&[set("s", 0, MergeType::Union, vec![canonical, aliaser])]);

        assert_eq!(
            table.get(&name("go")).map(|c| c.name().as_str()),
            Some("go")
        );
        assert!(table.get(&name("goto")).is_some());
    }

    #[test]
    fn merge_precedence_account_beats_location() {
        // Stand-in for the §2.7 layer order: "account" maps to a higher priority
        // than "location", so the account binding wins a Union collision.
        let account_look = Command::new(name("look")).with_alias(name("account"));
        let location_look = Command::new(name("look")).with_alias(name("location"));
        let table = CmdSet::merge(&[
            set("location", 10, MergeType::Union, vec![location_look]),
            set("account", 40, MergeType::Union, vec![account_look]),
        ]);

        let look = table.get(&name("look")).expect("look survives");
        assert_eq!(look.aliases(), &[name("account")]);
    }
}
