//! Lock evaluation: walks a resolved [`Lock`] against a [`LockContext`],
//! dispatching on the typed [`LockFn`] with no string matching (SPEC §2.6.2.2).

use std::collections::HashSet;

use super::ast::{Lock, LockFn, ResolvedExpr};

/// The facts a lock check evaluates against.
///
/// The M1 lock functions query different subjects: `perm`/`status` describe the
/// **accessor** (the caller), `attr`/`tag` describe the **accessed** object, and
/// `self` asks whether the two are the same entity. That split is documented
/// here for when real component stores back these facts (later milestones); in
/// M1 the caller populates the context directly.
#[derive(Debug, Default, Clone)]
pub struct LockContext {
    perms: HashSet<String>,
    attrs: HashSet<String>,
    tags: HashSet<String>,
    statuses: HashSet<String>,
    is_self: bool,
}

impl LockContext {
    /// An empty context: no permissions, attributes, tags, or statuses, and the
    /// accessor is not the accessed object.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Grants the accessor permission `name`.
    #[must_use]
    pub fn with_perm(mut self, name: &str) -> Self {
        self.perms.insert(name.to_string());
        self
    }

    /// Records attribute `name` on the accessed object.
    #[must_use]
    pub fn with_attr(mut self, name: &str) -> Self {
        self.attrs.insert(name.to_string());
        self
    }

    /// Records tag `name` on the accessed object.
    #[must_use]
    pub fn with_tag(mut self, name: &str) -> Self {
        self.tags.insert(name.to_string());
        self
    }

    /// Records status `name` on the accessor.
    #[must_use]
    pub fn with_status(mut self, name: &str) -> Self {
        self.statuses.insert(name.to_string());
        self
    }

    /// Marks the accessor as the accessed object, so `self()` holds.
    #[must_use]
    pub fn as_self(mut self) -> Self {
        self.is_self = true;
        self
    }
}

impl Lock {
    /// Evaluates this lock against `ctx`, returning whether access is granted.
    #[must_use]
    pub fn evaluate(&self, ctx: &LockContext) -> bool {
        eval_expr(self.expr(), ctx)
    }
}

fn eval_expr(expr: &ResolvedExpr, ctx: &LockContext) -> bool {
    match expr {
        ResolvedExpr::Fn(function) => eval_fn(function, ctx),
        ResolvedExpr::Not(inner) => !eval_expr(inner, ctx),
        ResolvedExpr::And(lhs, rhs) => eval_expr(lhs, ctx) && eval_expr(rhs, ctx),
        ResolvedExpr::Or(lhs, rhs) => eval_expr(lhs, ctx) || eval_expr(rhs, ctx),
    }
}

fn eval_fn(function: &LockFn, ctx: &LockContext) -> bool {
    match function {
        LockFn::Perm(arg) => ctx.perms.contains(arg.as_str()),
        LockFn::Attr(arg) => ctx.attrs.contains(arg.as_str()),
        LockFn::Tag(arg) => ctx.tags.contains(arg.as_str()),
        LockFn::Status(arg) => ctx.statuses.contains(arg.as_str()),
        LockFn::SelfRef => ctx.is_self,
    }
}

#[cfg(test)]
mod tests {
    use super::super::parser::parse;
    use super::super::resolve::resolve;
    use super::*;

    fn lock(input: &str) -> Lock {
        resolve(parse(input).expect("input should parse")).expect("input should resolve")
    }

    #[test]
    fn first_example_grants_uncursed_player() {
        let lock = lock("get:perm(player) and not attr(cursed)");
        assert!(lock.evaluate(&LockContext::new().with_perm("player")));
    }

    #[test]
    fn first_example_denies_cursed_player() {
        let lock = lock("get:perm(player) and not attr(cursed)");
        let ctx = LockContext::new().with_perm("player").with_attr("cursed");
        assert!(!lock.evaluate(&ctx));
    }

    #[test]
    fn first_example_denies_non_player() {
        let lock = lock("get:perm(player) and not attr(cursed)");
        assert!(!lock.evaluate(&LockContext::new()));
    }

    #[test]
    fn second_example_grants_self_or_admin() {
        let lock = lock("edit:self() or perm(admin)");
        assert!(lock.evaluate(&LockContext::new().as_self()));
        assert!(lock.evaluate(&LockContext::new().with_perm("admin")));
    }

    #[test]
    fn second_example_denies_unrelated_caller() {
        let lock = lock("edit:self() or perm(admin)");
        assert!(!lock.evaluate(&LockContext::new().with_perm("player")));
    }

    #[test]
    fn third_example_grants_sober_crew() {
        let lock = lock("helm:tag(crew) and not status(drunk)");
        assert!(lock.evaluate(&LockContext::new().with_tag("crew")));
    }

    #[test]
    fn third_example_denies_drunk_crew() {
        let lock = lock("helm:tag(crew) and not status(drunk)");
        let ctx = LockContext::new().with_tag("crew").with_status("drunk");
        assert!(!lock.evaluate(&ctx));
    }

    #[test]
    fn negation_inverts_a_leaf() {
        let lock = lock("x:not perm(banned)");
        assert!(lock.evaluate(&LockContext::new()));
        assert!(!lock.evaluate(&LockContext::new().with_perm("banned")));
    }

    #[test]
    fn disjunction_short_circuits_correctly() {
        let lock = lock("x:perm(a) or perm(b)");
        assert!(lock.evaluate(&LockContext::new().with_perm("b")));
        assert!(!lock.evaluate(&LockContext::new().with_perm("c")));
    }
}
