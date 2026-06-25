//! Locks and permissions DSL (SPEC §2.6).
//!
//! A lock string has the Evennia-familiar shape `accesstype:expr`, where `expr`
//! is a boolean tree of lock-function calls combined with `and`, `or`, `not`,
//! and parentheses. The pipeline has three stages:
//!
//! 1. [`parse`] — `chumsky` grammar → syntactic [`ParsedLock`] (§2.6.2.1).
//! 2. [`resolve`] — lower known functions into a typed [`Lock`] (§2.6.2.2);
//!    unknown functions and bad arity become [`ResolveError`]s here.
//! 3. [`Lock::evaluate`] — walk the typed tree against a [`LockContext`],
//!    dispatching statically on [`LockFn`] with no string matching.
//!
//! The M1 lock-function set is `perm`, `attr`, `tag`, `status`, and `self`.

mod ast;
mod eval;
mod parser;
mod resolve;

pub use ast::{AccessType, Lock, LockArg, LockFn, ParsedLock, ResolvedExpr, SyntaxExpr};
pub use eval::LockContext;
pub use parser::{ParseError, parse};
pub use resolve::{ResolveError, resolve};
