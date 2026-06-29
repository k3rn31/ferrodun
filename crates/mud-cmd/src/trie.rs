//! A prefix trie over command names and aliases backing the §2.7-step-5 lookup.
//!
//! Keys are the lowercase tokens a command answers to; each terminal node
//! carries the command's index into the owning table's command vector. Lookup
//! distinguishes an **exact** terminal from a set of commands reachable by
//! **prefix**, which is what lets an exact name beat a longer command it is a
//! prefix of (e.g. `n` → the `n` alias, never ambiguous against `north`).

use std::collections::BTreeMap;

/// The outcome of resolving a token against the trie.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum TrieMatch {
    /// The token is itself a registered name; resolves to that command index.
    Exact(usize),
    /// The token is a strict prefix of one or more names; the reachable command
    /// indices, deduplicated and sorted ascending for a deterministic order.
    Prefix(Vec<usize>),
    /// No registered name starts with the token.
    None,
}

#[derive(Debug, Default)]
struct Node {
    children: BTreeMap<char, Node>,
    /// The command index when a registered name ends exactly at this node.
    terminal: Option<usize>,
}

/// A char-keyed prefix trie mapping command names to command indices.
#[derive(Debug, Default)]
pub(crate) struct PrefixTrie {
    root: Node,
}

impl PrefixTrie {
    /// Registers `key` as resolving to `command_index`.
    ///
    /// [`CommandTable`](crate::CommandTable) settles token ownership before
    /// building the trie, so each key is inserted exactly once; a repeat insert
    /// would simply overwrite the earlier index.
    pub(crate) fn insert(&mut self, key: &str, command_index: usize) {
        let mut node = &mut self.root;
        for ch in key.chars() {
            node = node.children.entry(ch).or_default();
        }
        node.terminal = Some(command_index);
    }

    /// Resolves `token` to an exact match, a prefix candidate set, or nothing.
    pub(crate) fn lookup(&self, token: &str) -> TrieMatch {
        let mut node = &self.root;
        for ch in token.chars() {
            match node.children.get(&ch) {
                Some(child) => node = child,
                None => return TrieMatch::None,
            }
        }

        if let Some(index) = node.terminal {
            return TrieMatch::Exact(index);
        }

        let mut indices = Vec::new();
        collect_terminals(node, &mut indices);
        indices.sort_unstable();
        indices.dedup();
        TrieMatch::Prefix(indices)
    }
}

/// Depth-first collects every terminal index in `node`'s subtree.
fn collect_terminals(node: &Node, out: &mut Vec<usize>) {
    if let Some(index) = node.terminal {
        out.push(index);
    }
    for child in node.children.values() {
        collect_terminals(child, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trie(entries: &[(&str, usize)]) -> PrefixTrie {
        let mut trie = PrefixTrie::default();
        for (key, index) in entries {
            trie.insert(key, *index);
        }
        trie
    }

    #[test]
    fn resolves_an_exact_name() {
        let trie = trie(&[("look", 0)]);
        assert_eq!(trie.lookup("look"), TrieMatch::Exact(0));
    }

    #[test]
    fn resolves_a_unique_prefix() {
        let trie = trie(&[("north", 0)]);
        assert_eq!(trie.lookup("no"), TrieMatch::Prefix(vec![0]));
    }

    #[test]
    fn an_exact_terminal_beats_a_longer_name_it_prefixes() {
        // `n` is its own command; `north` shares the `n` path. Looking up `n`
        // must resolve the `n` command exactly, not be ambiguous.
        let trie = trie(&[("n", 0), ("north", 1)]);
        assert_eq!(trie.lookup("n"), TrieMatch::Exact(0));
    }

    #[test]
    fn an_ambiguous_prefix_returns_sorted_deduped_indices() {
        let trie = trie(&[("south", 2), ("say", 1), ("score", 0)]);
        assert_eq!(trie.lookup("s"), TrieMatch::Prefix(vec![0, 1, 2]));
    }

    #[test]
    fn an_unknown_prefix_returns_none() {
        let trie = trie(&[("look", 0)]);
        assert_eq!(trie.lookup("z"), TrieMatch::None);
    }
}
