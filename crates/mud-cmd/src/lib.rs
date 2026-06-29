//! Command model and parser for the Ferrodun engine (§2.7 steps 4–5).
//!
//! This crate is the command pipeline's front-end: the [`CmdSet`] data model,
//! the Union / Replace / Remove [`merge`](CmdSet::merge) that resolves several
//! sets into one [`CommandTable`] (§2.7 step 4), and the trie-backed line
//! [`parse`](CommandTable::parse) with prefix matching, aliases, and switches
//! (§2.7 step 5).
//!
//! It is deliberately host-free: a [`Command`] is pure metadata with **no
//! dispatch handle**, and merge precedence is expressed as an integer
//! [`Priority`] rather than the named source layers of §2.7 step 4 — World
//! resolution and dispatch (M1-16), the built-in command bodies (M1-17), and
//! locale-contributed aliases (§3.14.5.2, M2) all build on top of this.

mod cmdset;
mod command;
mod parser;
mod token;
mod trie;

pub use cmdset::{CmdSet, CmdSetKey, CmdSetKeyError, MergeType, Priority};
pub use command::{Command, CommandName, CommandNameError, Switch, SwitchError};
pub use parser::{CommandTable, ParseOutcome};
