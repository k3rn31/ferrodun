//! The lock DSL end-to-end through the public surface: `parse` → `resolve` →
//! `Lock::evaluate`, plus the error surfaces at each stage.
#![allow(clippy::expect_used)] // integration-test crates are not compiled with cfg(test), so clippy.toml allow-expect-in-tests does not cover their helpers; expect() is permitted in tests per policy

use mud_core::{Lock, LockContext, ResolveError, parse, resolve};

/// Runs the full pipeline, expecting a syntactically and semantically valid lock.
fn compile(input: &str) -> Lock {
    resolve(parse(input).expect("input should parse")).expect("input should resolve")
}

#[test]
fn first_normative_example_grants_uncursed_player_only() {
    let lock = compile("get:perm(player) and not attr(cursed)");

    assert!(lock.evaluate(&LockContext::new().with_perm("player")));
    assert!(!lock.evaluate(&LockContext::new().with_perm("player").with_attr("cursed")));
    assert!(!lock.evaluate(&LockContext::new()));
}

#[test]
fn second_normative_example_grants_self_or_admin() {
    let lock = compile("edit:self() or perm(admin)");

    assert!(lock.evaluate(&LockContext::new().as_self()));
    assert!(lock.evaluate(&LockContext::new().with_perm("admin")));
    assert!(!lock.evaluate(&LockContext::new().with_perm("player")));
}

#[test]
fn third_normative_example_grants_sober_crew_only() {
    let lock = compile("helm:tag(crew) and not status(drunk)");

    assert!(lock.evaluate(&LockContext::new().with_tag("crew")));
    assert!(!lock.evaluate(&LockContext::new().with_tag("crew").with_status("drunk")));
}

#[test]
fn precedence_is_not_binds_tighter_than_and_tighter_than_or() {
    // `(a and (not b)) or c`
    let lock = compile("x:perm(a) and not perm(b) or perm(c)");

    // c alone satisfies the trailing `or`.
    assert!(lock.evaluate(&LockContext::new().with_perm("c")));
    // a without b satisfies the left conjunction.
    assert!(lock.evaluate(&LockContext::new().with_perm("a")));
    // a with b breaks the conjunction, and there is no c.
    assert!(!lock.evaluate(&LockContext::new().with_perm("a").with_perm("b")));
}

#[test]
fn a_syntactically_invalid_lock_fails_to_parse() {
    assert!(parse("x:perm(player) and (perm(admin)").is_err());
}

#[test]
fn an_unknown_function_fails_to_resolve() {
    let parsed = parse("x:wizard(merlin)").expect("input should parse");

    assert_eq!(
        resolve(parsed),
        Err(ResolveError::UnknownFunction {
            name: "wizard".to_string(),
        })
    );
}

#[test]
fn wrong_arity_fails_to_resolve() {
    let nullary_with_arg = parse("x:self(player)").expect("input should parse");
    assert_eq!(
        resolve(nullary_with_arg),
        Err(ResolveError::ArityMismatch {
            name: "self".to_string(),
            expected: 0,
            found: 1,
        })
    );

    let unary_without_arg = parse("x:perm()").expect("input should parse");
    assert_eq!(
        resolve(unary_without_arg),
        Err(ResolveError::ArityMismatch {
            name: "perm".to_string(),
            expected: 1,
            found: 0,
        })
    );
}
