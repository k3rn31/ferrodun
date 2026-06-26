//! Lock DSL abstract syntax.
//!
//! Two forms live here. The **syntactic** form ([`ParsedLock`] / [`SyntaxExpr`])
//! is the raw shape produced by the parser — function calls are still untyped
//! (a name plus string arguments). The **resolved** form ([`Lock`] /
//! [`ResolvedExpr`] / [`LockFn`]) is produced by
//! [`resolve`](crate::locks::resolve) and is what evaluation runs against;
//! every function is a typed variant, so evaluation dispatches statically with
//! no string matching (SPEC §2.6.2.2).

/// An access-type label: the left-hand side of a lock string naming the
/// operation the lock guards (the `get` in `get:perm(player)`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessType(String);

impl AccessType {
    /// Wraps a parsed access-type token. Crate-internal: the only caller is the
    /// parser, which has already constrained the value to an identifier.
    pub(crate) fn new(raw: String) -> Self {
        Self(raw)
    }

    /// The access-type label as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A lock expression as parsed: a boolean tree whose leaves are still untyped
/// function calls (name + string arguments). Semantic validation — known
/// functions and argument arity — happens in [`resolve`](crate::locks::resolve).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyntaxExpr {
    /// A function call such as `perm(player)` or `self()`.
    Call { name: String, args: Vec<String> },
    /// Logical negation: `not <expr>`.
    Not(Box<SyntaxExpr>),
    /// Logical conjunction: `<expr> and <expr>`.
    And(Box<SyntaxExpr>, Box<SyntaxExpr>),
    /// Logical disjunction: `<expr> or <expr>`.
    Or(Box<SyntaxExpr>, Box<SyntaxExpr>),
}

/// A successfully parsed lock string, before function resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub struct ParsedLock {
    access_type: AccessType,
    expr: SyntaxExpr,
}

impl ParsedLock {
    pub(crate) fn new(access_type: AccessType, expr: SyntaxExpr) -> Self {
        Self { access_type, expr }
    }

    /// The access type this lock guards.
    pub fn access_type(&self) -> &AccessType {
        &self.access_type
    }

    /// The (still untyped) boolean expression.
    pub fn expr(&self) -> &SyntaxExpr {
        &self.expr
    }

    /// Consumes the parsed lock, yielding its parts for resolution.
    pub(crate) fn into_parts(self) -> (AccessType, SyntaxExpr) {
        (self.access_type, self.expr)
    }
}

/// A validated, non-empty argument to a lock function (the `player` in
/// `perm(player)`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockArg(String);

impl LockArg {
    /// Wraps a resolved function argument. Crate-internal: the argument
    /// originates from a parsed identifier, so it is already non-empty.
    pub(crate) fn new(raw: String) -> Self {
        Self(raw)
    }

    /// The argument as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A resolved lock function. Each variant maps to one fixed evaluator, which is
/// what lets evaluation dispatch statically (SPEC §2.6.2.2). The set is `perm`,
/// `attr`, `tag`, `status`, and `self`; more functions may be added, hence
/// `#[non_exhaustive]`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum LockFn {
    /// `perm(name)` — the accessor holds permission `name`.
    Perm(LockArg),
    /// `attr(name)` — the accessed object has attribute `name`.
    Attr(LockArg),
    /// `tag(name)` — the accessed object carries tag `name`.
    Tag(LockArg),
    /// `status(name)` — the accessor has status `name`.
    Status(LockArg),
    /// `self()` — the accessor is the accessed object.
    SelfRef,
}

/// A resolved lock expression: the boolean tree with every leaf lowered to a
/// typed [`LockFn`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedExpr {
    /// A resolved function leaf.
    Fn(LockFn),
    /// Logical negation.
    Not(Box<ResolvedExpr>),
    /// Logical conjunction.
    And(Box<ResolvedExpr>, Box<ResolvedExpr>),
    /// Logical disjunction.
    Or(Box<ResolvedExpr>, Box<ResolvedExpr>),
}

/// A fully resolved lock, ready to [`evaluate`](Lock::evaluate).
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub struct Lock {
    access_type: AccessType,
    expr: ResolvedExpr,
}

impl Lock {
    pub(crate) fn new(access_type: AccessType, expr: ResolvedExpr) -> Self {
        Self { access_type, expr }
    }

    /// The access type this lock guards.
    pub fn access_type(&self) -> &AccessType {
        &self.access_type
    }

    /// The resolved boolean expression.
    pub fn expr(&self) -> &ResolvedExpr {
        &self.expr
    }
}
