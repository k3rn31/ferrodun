//! Lock DSL parser: a `chumsky` grammar turning a lock string into a
//! [`ParsedLock`] (SPEC §2.6.2.1). Pure syntax — unknown functions and bad
//! arity are not errors here; they are caught by
//! [`resolve`](crate::locks::resolve).

use chumsky::prelude::*;

use super::ast::{AccessType, ParsedLock, SyntaxExpr};

/// A lock-string parse failure. Wraps `chumsky`'s diagnostics into a
/// crate-owned type so the parser dependency does not leak across the API.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("lock parse error: {message}")]
#[non_exhaustive]
pub struct ParseError {
    message: String,
}

/// Parses a lock string of the form `accesstype:expr` into a [`ParsedLock`].
///
/// The expression grammar is Evennia's: function calls (`name(args...)`)
/// combined with `and`, `or`, `not`, and parentheses, where `not` binds
/// tighter than `and`, which binds tighter than `or`. Whitespace around tokens
/// is insignificant.
///
/// # Errors
///
/// Returns [`ParseError`] when the input is not a syntactically valid lock
/// string (malformed expression, missing `accesstype:`, trailing input, …).
pub fn parse(input: &str) -> Result<ParsedLock, ParseError> {
    lock_parser()
        .parse(input)
        .into_result()
        .map_err(|errors| ParseError {
            message: errors
                .into_iter()
                .map(|error| error.to_string())
                .collect::<Vec<_>>()
                .join("; "),
        })
}

/// The reserved words of the expression grammar; never valid function names.
fn is_keyword(word: &str) -> bool {
    matches!(word, "and" | "or" | "not")
}

fn lock_parser<'src>() -> impl Parser<'src, &'src str, ParsedLock, extra::Err<Rich<'src, char>>> {
    let expr = recursive(|expr| {
        let func_name = text::ascii::ident()
            .try_map(|name: &str, span| {
                if is_keyword(name) {
                    Err(Rich::custom(
                        span,
                        "reserved keyword cannot be a lock function",
                    ))
                } else {
                    Ok(name.to_string())
                }
            })
            .padded();

        let args = text::ascii::ident()
            .map(ToString::to_string)
            .padded()
            .separated_by(just(','))
            .collect::<Vec<_>>()
            .delimited_by(just('(').padded(), just(')').padded());

        let call = func_name
            .then(args)
            .map(|(name, args)| SyntaxExpr::Call { name, args });

        let group = expr.delimited_by(just('(').padded(), just(')').padded());

        // No `.padded()` here: each atom variant already consumes surrounding
        // whitespace through its own leading/trailing tokens.
        let atom = group.or(call);

        let unary = text::ascii::keyword("not")
            .padded()
            .ignored()
            .repeated()
            .foldr(atom, |(), rhs| SyntaxExpr::Not(Box::new(rhs)));

        let conjunction = unary.clone().foldl(
            text::ascii::keyword("and")
                .padded()
                .ignore_then(unary)
                .repeated(),
            |lhs, rhs| SyntaxExpr::And(Box::new(lhs), Box::new(rhs)),
        );

        conjunction.clone().foldl(
            text::ascii::keyword("or")
                .padded()
                .ignore_then(conjunction)
                .repeated(),
            |lhs, rhs| SyntaxExpr::Or(Box::new(lhs), Box::new(rhs)),
        )
    });

    let access_type = text::ascii::ident()
        .try_map(|name: &str, span| {
            if is_keyword(name) {
                Err(Rich::custom(
                    span,
                    "reserved keyword cannot be an access type",
                ))
            } else {
                Ok(AccessType::new(name.to_string()))
            }
        })
        .padded();

    access_type
        .then_ignore(just(':'))
        .then(expr)
        .then_ignore(end())
        .map(|(access_type, expr)| ParsedLock::new(access_type, expr))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(name: &str, args: &[&str]) -> SyntaxExpr {
        SyntaxExpr::Call {
            name: name.to_string(),
            args: args.iter().map(ToString::to_string).collect(),
        }
    }

    fn and(lhs: SyntaxExpr, rhs: SyntaxExpr) -> SyntaxExpr {
        SyntaxExpr::And(Box::new(lhs), Box::new(rhs))
    }

    fn or(lhs: SyntaxExpr, rhs: SyntaxExpr) -> SyntaxExpr {
        SyntaxExpr::Or(Box::new(lhs), Box::new(rhs))
    }

    fn not(inner: SyntaxExpr) -> SyntaxExpr {
        SyntaxExpr::Not(Box::new(inner))
    }

    fn parse_ok(input: &str) -> ParsedLock {
        parse(input).expect("input should parse")
    }

    #[test]
    fn parses_first_normative_example() {
        let lock = parse_ok("get:perm(player) and not attr(cursed)");
        assert_eq!(lock.access_type().as_str(), "get");
        assert_eq!(
            *lock.expr(),
            and(call("perm", &["player"]), not(call("attr", &["cursed"])))
        );
    }

    #[test]
    fn parses_second_normative_example() {
        let lock = parse_ok("edit:self() or perm(admin)");
        assert_eq!(lock.access_type().as_str(), "edit");
        assert_eq!(
            *lock.expr(),
            or(call("self", &[]), call("perm", &["admin"]))
        );
    }

    #[test]
    fn parses_third_normative_example() {
        let lock = parse_ok("helm:tag(crew) and not status(drunk)");
        assert_eq!(lock.access_type().as_str(), "helm");
        assert_eq!(
            *lock.expr(),
            and(call("tag", &["crew"]), not(call("status", &["drunk"])))
        );
    }

    #[test]
    fn not_binds_tighter_than_and_binds_tighter_than_or() {
        // a and not b or c  ==  (a and (not b)) or c
        let lock = parse_ok("x:perm(a) and not perm(b) or perm(c)");
        assert_eq!(
            *lock.expr(),
            or(
                and(call("perm", &["a"]), not(call("perm", &["b"]))),
                call("perm", &["c"]),
            )
        );
    }

    #[test]
    fn not_applies_to_a_parenthesized_group() {
        // not (perm(a) or perm(b))
        let lock = parse_ok("x:not (perm(a) or perm(b))");
        assert_eq!(
            *lock.expr(),
            not(or(call("perm", &["a"]), call("perm", &["b"])))
        );
    }

    #[test]
    fn parentheses_override_precedence() {
        // perm(a) and (perm(b) or perm(c))
        let lock = parse_ok("x:perm(a) and (perm(b) or perm(c))");
        assert_eq!(
            *lock.expr(),
            and(
                call("perm", &["a"]),
                or(call("perm", &["b"]), call("perm", &["c"])),
            )
        );
    }

    #[test]
    fn tolerates_surrounding_and_internal_whitespace() {
        let lock = parse_ok("  get : perm( player )  ");
        assert_eq!(*lock.expr(), call("perm", &["player"]));
    }

    #[test]
    fn rejects_missing_access_type() {
        assert!(parse("perm(player)").is_err());
    }

    #[test]
    fn rejects_unbalanced_parentheses() {
        assert!(parse("x:perm(player) and (perm(admin)").is_err());
    }

    #[test]
    fn rejects_trailing_input() {
        assert!(parse("x:perm(player) garbage").is_err());
    }

    #[test]
    fn rejects_keyword_as_function_name() {
        assert!(parse("x:and(player)").is_err());
    }

    #[test]
    fn rejects_keyword_as_access_type() {
        assert!(parse("not:perm(player)").is_err());
    }
}
