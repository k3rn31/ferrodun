//! The post-merge command table and the §2.7-step-5 line parser.

use crate::command::{Command, CommandName, Switch, SwitchError};
use crate::trie::{PrefixTrie, TrieMatch};

/// The result of parsing a single input line against a [`CommandTable`].
///
/// Only **command-name** disambiguation lives here (exact / unique-prefix /
/// ambiguous). **Object** disambiguation on the arguments — `name.N` ordinals,
/// `all`, the numbered prompt (§2.7 step 5) — is deferred (M1-16); the raw
/// argument remainder is returned untouched.
#[derive(Debug, PartialEq, Eq)]
pub enum ParseOutcome<'a> {
    /// The line was blank.
    Empty,
    /// No command name matched the leading token.
    NotFound,
    /// The leading token was a prefix of several commands; their canonical names
    /// in deterministic order.
    Ambiguous(Vec<&'a CommandName>),
    /// A switch token was malformed.
    BadSwitch(SwitchError),
    /// A command matched.
    Matched {
        /// The resolved command.
        command: &'a Command,
        /// The switches given after the command (e.g. `quiet` in `look/quiet`).
        switches: Vec<Switch>,
        /// The raw argument remainder, trimmed but otherwise verbatim.
        args: &'a str,
    },
}

/// A merged, parseable set of commands: the resolved commands plus the prefix
/// trie indexing every name and alias (§2.7 steps 4–5).
///
/// Built by [`CmdSet::merge`](crate::CmdSet::merge).
#[derive(Debug)]
#[must_use]
pub struct CommandTable {
    commands: Vec<Command>,
    trie: PrefixTrie,
}

impl CommandTable {
    /// Builds a table from the merge's already-resolved commands (canonical-name
    /// order) and the prefix trie that maps every settled token to its owning
    /// command's index.
    ///
    /// Token ownership is settled by the caller — [`CmdSet::merge`](crate::CmdSet::merge),
    /// the last step of the §2.7 merge — so the trie never indexes a colliding
    /// key. This constructor only stores the two halves.
    pub(crate) fn new(commands: Vec<Command>, trie: PrefixTrie) -> Self {
        Self { commands, trie }
    }

    /// The number of resolved commands.
    #[must_use]
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    /// Whether the table holds no commands.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// Iterates the resolved commands in canonical-name order.
    pub fn iter(&self) -> impl Iterator<Item = &Command> {
        self.commands.iter()
    }

    /// Looks up a command by its exact canonical name or alias.
    #[must_use]
    pub fn get(&self, name: &CommandName) -> Option<&Command> {
        match self.trie.lookup(name.as_str()) {
            TrieMatch::Exact(index) => self.commands.get(index),
            TrieMatch::Prefix(_) | TrieMatch::None => None,
        }
    }

    /// Parses one input line into a [`ParseOutcome`] (§2.7 step 5).
    ///
    /// The line is split into a leading command head and an argument remainder
    /// at the first whitespace. The head is lowercased (matching is
    /// case-insensitive) and split on `/` into the command token and switches.
    /// The command token resolves against the trie: an exact name/alias wins,
    /// else a unique prefix, else the candidates are reported as
    /// [`Ambiguous`](ParseOutcome::Ambiguous).
    pub fn parse<'a>(&'a self, line: &'a str) -> ParseOutcome<'a> {
        let mut head_and_args = line.trim().splitn(2, char::is_whitespace);
        let head = match head_and_args.next() {
            Some(head) if !head.is_empty() => head,
            _ => return ParseOutcome::Empty,
        };
        let args = head_and_args.next().unwrap_or("").trim();

        let head = head.to_lowercase();
        let mut segments = head.split('/');
        // `split` always yields at least one element, so the command token is the
        // first segment and any further segments are switches.
        let command_token = segments.next().unwrap_or("");
        if command_token.is_empty() {
            // A leading `/` (switches with no command) names nothing.
            return ParseOutcome::NotFound;
        }

        let mut switches = Vec::new();
        for segment in segments {
            match Switch::parse(segment) {
                Ok(switch) => switches.push(switch),
                Err(error) => return ParseOutcome::BadSwitch(error),
            }
        }

        match self.trie.lookup(command_token) {
            TrieMatch::Exact(index) => self.matched(index, switches, args),
            TrieMatch::Prefix(indices) => match indices.as_slice() {
                [single] => self.matched(*single, switches, args),
                [] => ParseOutcome::NotFound,
                _ => ParseOutcome::Ambiguous(self.canonical_names(&indices)),
            },
            TrieMatch::None => ParseOutcome::NotFound,
        }
    }

    /// Builds a [`Matched`](ParseOutcome::Matched) for a resolved command index.
    fn matched<'a>(
        &'a self,
        index: usize,
        switches: Vec<Switch>,
        args: &'a str,
    ) -> ParseOutcome<'a> {
        match self.commands.get(index) {
            Some(command) => ParseOutcome::Matched {
                command,
                switches,
                args,
            },
            // INVARIANT: trie indices are produced alongside `self.commands` when
            // the table is built in `CmdSet::merge`, so a lookup always points at
            // a live command.
            None => ParseOutcome::NotFound,
        }
    }

    /// The canonical names of the commands at `indices`, for an ambiguity report.
    fn canonical_names(&self, indices: &[usize]) -> Vec<&CommandName> {
        indices
            .iter()
            .filter_map(|index| self.commands.get(*index))
            .map(Command::name)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmdset::{CmdSet, CmdSetKey, MergeType, Priority};

    fn name(value: &str) -> CommandName {
        CommandName::parse(value).expect("valid command name")
    }

    fn switch(value: &str) -> Switch {
        Switch::parse(value).expect("valid switch")
    }

    /// A one-set table over `commands`, the common shape for parser tests.
    fn table(commands: Vec<Command>) -> CommandTable {
        CmdSet::merge(&[CmdSet::new(
            CmdSetKey::parse("test").expect("valid key"),
            Priority::DEFAULT,
            MergeType::Union,
            commands,
        )])
    }

    /// A `Matched` outcome carrying no switches and an empty argument remainder.
    fn matched_bare(command: &Command) -> ParseOutcome<'_> {
        ParseOutcome::Matched {
            command,
            switches: Vec::new(),
            args: "",
        }
    }

    #[test]
    fn matches_an_exact_name() {
        let table = table(vec![Command::new(name("look"))]);
        let look = table.get(&name("look")).expect("look present");
        assert_eq!(table.parse("look"), matched_bare(look));
    }

    #[test]
    fn matches_an_alias() {
        let table = table(vec![Command::new(name("north")).with_alias(name("n"))]);
        let north = table.get(&name("north")).expect("north present");
        assert_eq!(table.parse("n"), matched_bare(north));
    }

    #[test]
    fn matches_a_unique_prefix() {
        let table = table(vec![Command::new(name("north"))]);
        let north = table.get(&name("north")).expect("north present");
        assert_eq!(table.parse("no"), matched_bare(north));
    }

    #[test]
    fn an_exact_alias_beats_a_command_it_prefixes() {
        let table = table(vec![
            Command::new(name("north")).with_alias(name("n")),
            Command::new(name("nibble")),
        ]);
        let north = table.get(&name("north")).expect("north present");
        // `n` is an exact alias of north; it must not be ambiguous against nibble.
        assert_eq!(table.parse("n"), matched_bare(north));
    }

    #[test]
    fn an_ambiguous_prefix_lists_candidates() {
        let table = table(vec![Command::new(name("say")), Command::new(name("score"))]);
        let say = table.get(&name("say")).expect("say present");
        let score = table.get(&name("score")).expect("score present");
        assert_eq!(
            table.parse("s"),
            ParseOutcome::Ambiguous(vec![say.name(), score.name()])
        );
    }

    #[test]
    fn an_unknown_command_is_not_found() {
        let table = table(vec![Command::new(name("look"))]);
        assert_eq!(table.parse("dance"), ParseOutcome::NotFound);
    }

    #[test]
    fn a_blank_line_is_empty() {
        let table = table(vec![Command::new(name("look"))]);
        assert_eq!(table.parse("   "), ParseOutcome::Empty);
    }

    #[test]
    fn parses_a_switch_and_keeps_the_argument_remainder() {
        let table = table(vec![
            Command::new(name("look")).with_switch(switch("quiet")),
        ]);
        let look = table.get(&name("look")).expect("look present");
        assert_eq!(
            table.parse("look/quiet here"),
            ParseOutcome::Matched {
                command: look,
                switches: vec![switch("quiet")],
                args: "here",
            }
        );
    }

    #[test]
    fn parses_multiple_switches() {
        let table = table(vec![Command::new(name("look"))]);
        let look = table.get(&name("look")).expect("look present");
        assert_eq!(
            table.parse("look/quiet/brief"),
            ParseOutcome::Matched {
                command: look,
                switches: vec![switch("quiet"), switch("brief")],
                args: "",
            }
        );
    }

    #[test]
    fn a_malformed_switch_is_reported() {
        let table = table(vec![Command::new(name("look"))]);
        // Trailing slash yields an empty switch token.
        assert_eq!(
            table.parse("look/"),
            ParseOutcome::BadSwitch(SwitchError::Empty)
        );
    }

    #[test]
    fn a_leading_slash_with_no_command_is_not_found() {
        let table = table(vec![Command::new(name("look"))]);
        assert_eq!(table.parse("/quiet"), ParseOutcome::NotFound);
    }

    #[test]
    fn command_matching_is_case_insensitive() {
        let table = table(vec![Command::new(name("look"))]);
        let look = table.get(&name("look")).expect("look present");
        assert_eq!(table.parse("LOOK"), matched_bare(look));
    }

    #[test]
    fn the_argument_remainder_is_preserved_verbatim() {
        let table = table(vec![Command::new(name("say"))]);
        let say = table.get(&name("say")).expect("say present");
        assert_eq!(
            table.parse("say Hello, World!"),
            ParseOutcome::Matched {
                command: say,
                switches: vec![],
                args: "Hello, World!",
            }
        );
    }
}
