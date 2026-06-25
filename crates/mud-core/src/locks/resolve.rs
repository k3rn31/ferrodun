//! Lock resolution: lowers a syntactic [`ParsedLock`] into a typed [`Lock`],
//! mapping each function call to a [`LockFn`] variant and enforcing argument
//! arity (SPEC §2.6.2.2). This is the seam where unknown functions and bad
//! arity become errors.

use super::ast::{Lock, LockArg, LockFn, ParsedLock, ResolvedExpr, SyntaxExpr};

/// A failure to resolve a parsed lock against the known lock functions.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ResolveError {
    /// A function name that is not part of the known lock-function set.
    #[error("unknown lock function `{name}`")]
    UnknownFunction { name: String },
    /// A known function called with the wrong number of arguments.
    #[error("lock function `{name}` takes {expected} argument(s), got {found}")]
    ArityMismatch {
        name: String,
        expected: usize,
        found: usize,
    },
}

/// Resolves a parsed lock into a typed, evaluable [`Lock`].
///
/// # Errors
///
/// Returns [`ResolveError`] if the lock references a function outside the known
/// set or calls a known function with the wrong arity.
pub fn resolve(parsed: ParsedLock) -> Result<Lock, ResolveError> {
    let (access_type, expr) = parsed.into_parts();
    Ok(Lock::new(access_type, resolve_expr(expr)?))
}

fn resolve_expr(expr: SyntaxExpr) -> Result<ResolvedExpr, ResolveError> {
    match expr {
        SyntaxExpr::Call { name, args } => Ok(ResolvedExpr::Fn(resolve_call(name, args)?)),
        SyntaxExpr::Not(inner) => Ok(ResolvedExpr::Not(Box::new(resolve_expr(*inner)?))),
        SyntaxExpr::And(lhs, rhs) => Ok(ResolvedExpr::And(
            Box::new(resolve_expr(*lhs)?),
            Box::new(resolve_expr(*rhs)?),
        )),
        SyntaxExpr::Or(lhs, rhs) => Ok(ResolvedExpr::Or(
            Box::new(resolve_expr(*lhs)?),
            Box::new(resolve_expr(*rhs)?),
        )),
    }
}

fn resolve_call(name: String, args: Vec<String>) -> Result<LockFn, ResolveError> {
    match name.as_str() {
        "perm" => Ok(LockFn::Perm(unary_arg(&name, args)?)),
        "attr" => Ok(LockFn::Attr(unary_arg(&name, args)?)),
        "tag" => Ok(LockFn::Tag(unary_arg(&name, args)?)),
        "status" => Ok(LockFn::Status(unary_arg(&name, args)?)),
        "self" => {
            ensure_nullary(&name, &args)?;
            Ok(LockFn::SelfRef)
        }
        _ => Err(ResolveError::UnknownFunction { name }),
    }
}

/// Extracts the single argument of a unary function, erroring on any other
/// arity. The `try_from` consumes the vec only when its length is exactly one,
/// so no fallible indexing or panic path is needed.
fn unary_arg(name: &str, args: Vec<String>) -> Result<LockArg, ResolveError> {
    match <[String; 1]>::try_from(args) {
        Ok([arg]) => Ok(LockArg::new(arg)),
        Err(args) => Err(ResolveError::ArityMismatch {
            name: name.to_string(),
            expected: 1,
            found: args.len(),
        }),
    }
}

fn ensure_nullary(name: &str, args: &[String]) -> Result<(), ResolveError> {
    if args.is_empty() {
        Ok(())
    } else {
        Err(ResolveError::ArityMismatch {
            name: name.to_string(),
            expected: 0,
            found: args.len(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::parser::parse;
    use super::*;

    fn resolve_str(input: &str) -> Result<Lock, ResolveError> {
        let parsed = parse(input).expect("input should parse");
        resolve(parsed)
    }

    fn leaf(input: &str) -> ResolvedExpr {
        resolve_str(input).expect("should resolve").expr().clone()
    }

    fn func(f: LockFn) -> ResolvedExpr {
        ResolvedExpr::Fn(f)
    }

    fn arg(value: &str) -> LockArg {
        LockArg::new(value.to_string())
    }

    #[test]
    fn resolves_each_known_function() {
        assert_eq!(leaf("x:perm(player)"), func(LockFn::Perm(arg("player"))));
        assert_eq!(leaf("x:attr(cursed)"), func(LockFn::Attr(arg("cursed"))));
        assert_eq!(leaf("x:tag(crew)"), func(LockFn::Tag(arg("crew"))));
        assert_eq!(leaf("x:status(drunk)"), func(LockFn::Status(arg("drunk"))));
        assert_eq!(leaf("x:self()"), func(LockFn::SelfRef));
    }

    #[test]
    fn resolves_all_three_normative_examples() {
        assert!(resolve_str("get:perm(player) and not attr(cursed)").is_ok());
        assert!(resolve_str("edit:self() or perm(admin)").is_ok());
        assert!(resolve_str("helm:tag(crew) and not status(drunk)").is_ok());
    }

    #[test]
    fn rejects_unknown_function() {
        assert_eq!(
            resolve_str("x:wizard(merlin)"),
            Err(ResolveError::UnknownFunction {
                name: "wizard".to_string()
            })
        );
    }

    #[test]
    fn rejects_unary_function_with_no_argument() {
        assert_eq!(
            resolve_str("x:perm()"),
            Err(ResolveError::ArityMismatch {
                name: "perm".to_string(),
                expected: 1,
                found: 0,
            })
        );
    }

    #[test]
    fn rejects_unary_function_with_extra_arguments() {
        assert_eq!(
            resolve_str("x:perm(player, admin)"),
            Err(ResolveError::ArityMismatch {
                name: "perm".to_string(),
                expected: 1,
                found: 2,
            })
        );
    }

    #[test]
    fn rejects_self_with_argument() {
        assert_eq!(
            resolve_str("x:self(player)"),
            Err(ResolveError::ArityMismatch {
                name: "self".to_string(),
                expected: 0,
                found: 1,
            })
        );
    }
}
