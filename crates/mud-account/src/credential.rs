//! Password credentials: `argon2id` hashing with a per-account salt (§3.15.1.2).
//!
//! A [`Credential`] holds an argon2id **PHC string** — the standard
//! `$argon2id$...$salt$hash` encoding that embeds the algorithm, parameters, and
//! a unique random salt. Storing the PHC string means the per-account salt is
//! carried with the hash for free; there is no separate salt column to manage.
//! Plaintext never leaves this module, and the hash is redacted from `Debug` so
//! it cannot leak into logs.

use std::fmt;

use argon2::password_hash::{PasswordHash as PhcString, SaltString};
use argon2::{Argon2, PasswordHasher, PasswordVerifier};
use rand_core::OsRng;

/// Failures from constructing or parsing a [`Credential`].
///
/// Variants are intentionally opaque: the underlying `argon2` error is not
/// surfaced, both to keep the third-party type out of the public API and to
/// avoid echoing credential material into error messages.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CredentialError {
    /// Hashing the password failed (e.g. the KDF rejected the parameters).
    #[error("failed to hash password")]
    Hash,
    /// A stored value was not a well-formed argon2 PHC string.
    #[error("stored credential is malformed")]
    Malformed,
}

/// A stored password credential: an argon2id PHC string with an embedded
/// per-account salt.
///
/// No `PartialEq`/`Eq`: credentials are never compared (verification runs
/// through argon2's own constant-time check), so deriving a byte-wise equality
/// over secret hash material would only invite a non-constant-time comparison.
/// `Debug` is redacted so the hash cannot leak into logs.
pub struct Credential(String);

impl fmt::Debug for Credential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Credential(<redacted>)")
    }
}

impl Credential {
    /// Hashes `password` with argon2id and a freshly generated random salt.
    ///
    /// # Errors
    ///
    /// Returns [`CredentialError::Hash`] if the KDF fails to produce a hash.
    pub fn hash(password: &str) -> Result<Self, CredentialError> {
        let salt = SaltString::generate(&mut OsRng);
        let phc = Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map_err(|_| CredentialError::Hash)?
            .to_string();
        Ok(Self(phc))
    }

    /// Reconstructs a [`Credential`] from a stored PHC string.
    ///
    /// # Errors
    ///
    /// Returns [`CredentialError::Malformed`] if `phc` is not a parseable
    /// argon2 PHC string.
    pub fn from_phc(phc: impl Into<String>) -> Result<Self, CredentialError> {
        let phc = phc.into();
        // Parse to validate; we keep the original string for storage round-trips.
        PhcString::new(&phc).map_err(|_| CredentialError::Malformed)?;
        Ok(Self(phc))
    }

    /// Returns `true` iff `password` matches this credential.
    ///
    /// A malformed stored hash verifies as `false` rather than erroring: a
    /// corrupt credential is treated as a failed login, never a panic.
    #[must_use]
    pub fn verify(&self, password: &str) -> bool {
        Self::verify_phc(&self.0, password)
    }

    /// The PHC string for persistence.
    #[must_use]
    pub fn as_phc(&self) -> &str {
        &self.0
    }

    /// Verifies `password` against a stored PHC string in one step.
    ///
    /// Returns `false` for any failure — a wrong password *or* a malformed
    /// stored hash — so a corrupt credential row reads as a failed login rather
    /// than an error the persistence layer must classify. The PHC string is
    /// parsed once here.
    #[must_use]
    pub fn verify_phc(phc: &str, password: &str) -> bool {
        let Ok(parsed) = PhcString::new(phc) else {
            return false;
        };
        Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_then_verify_accepts_the_right_password() {
        let cred = Credential::hash("correct horse battery staple").expect("hashing succeeds");
        assert!(cred.verify("correct horse battery staple"));
    }

    #[test]
    fn verify_rejects_the_wrong_password() {
        let cred = Credential::hash("hunter2").expect("hashing succeeds");
        assert!(!cred.verify("Hunter2"));
        assert!(!cred.verify(""));
    }

    #[test]
    fn the_same_password_hashes_to_distinct_phc_strings() {
        // A fresh random salt per call means two hashes of one password differ,
        // so a stolen file cannot be attacked by grouping equal hashes.
        let a = Credential::hash("same").expect("hashing succeeds");
        let b = Credential::hash("same").expect("hashing succeeds");
        assert_ne!(a.as_phc(), b.as_phc());
        assert!(a.verify("same") && b.verify("same"));
    }

    #[test]
    fn from_phc_round_trips_as_phc_and_still_verifies() {
        let original = Credential::hash("rosebud").expect("hashing succeeds");
        let restored = Credential::from_phc(original.as_phc()).expect("a real PHC string parses");
        assert_eq!(restored.as_phc(), original.as_phc());
        assert!(restored.verify("rosebud"));
    }

    #[test]
    fn from_phc_rejects_a_non_phc_string() {
        assert!(matches!(
            Credential::from_phc("not a hash"),
            Err(CredentialError::Malformed)
        ));
    }

    #[test]
    fn it_uses_the_argon2id_variant() {
        // §3.15.1.2 mandates argon2id specifically (not argon2i/argon2d).
        let cred = Credential::hash("x").expect("hashing succeeds");
        assert!(
            cred.as_phc().starts_with("$argon2id$"),
            "default hash must be argon2id, got {}",
            cred.as_phc()
        );
    }

    #[test]
    fn verify_phc_matches_a_stored_hash() {
        let stored = Credential::hash("swordfish")
            .expect("hashing succeeds")
            .as_phc()
            .to_owned();
        assert!(Credential::verify_phc(&stored, "swordfish"));
        assert!(!Credential::verify_phc(&stored, "nope"));
    }

    #[test]
    fn verify_phc_is_false_for_a_corrupt_stored_hash() {
        assert!(!Credential::verify_phc("$argon2id$garbage", "anything"));
    }

    #[test]
    fn debug_redacts_the_hash() {
        let cred = Credential::hash("secret").expect("hashing succeeds");
        assert_eq!(format!("{cred:?}"), "Credential(<redacted>)");
    }
}
