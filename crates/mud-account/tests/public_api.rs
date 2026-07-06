//! Black-box guard on the crate's public surface (§3.15.1). Behavior is
//! unit-tested in-crate; this only confirms the public API composes for an
//! external consumer (no broken re-export, nothing accidentally private).
#![allow(clippy::expect_used)] // test file; mirrors `allow-expect-in-tests`

use mud_account::{AccountState, Credential, LoginError};

#[test]
fn a_credential_round_trips_through_its_phc_string() {
    let cred = Credential::hash("correct-horse").expect("hashing succeeds");
    let restored = Credential::from_phc(cred.as_phc()).expect("its own PHC parses");
    assert!(restored.verify("correct-horse"), "the right password verifies");
    assert!(!restored.verify("wrong"), "the wrong password is refused");
    assert!(
        Credential::verify_phc(cred.as_phc(), "correct-horse"),
        "verify_phc matches a stored hash"
    );
}

#[test]
fn account_state_login_rejection_is_reachable_publicly() {
    assert_eq!(AccountState::Active.login_rejection(), None);
    assert_eq!(
        AccountState::Deleted.login_rejection(),
        Some(LoginError::UnknownUser),
        "a soft-deleted account reads as unknown"
    );
}
