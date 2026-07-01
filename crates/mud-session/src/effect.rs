//! Effects the FSM emits for the driver to perform, and the results fed back.

use mud_account::{Account, LoginError, Puppet, Username};
use secrecy::SecretString;

/// I/O the driver must perform on the FSM's behalf. Password fields are
/// [`SecretString`]: zeroized on drop and redacted in `Debug`, exposed only at
/// the argon2 boundary inside the driver's backend.
#[derive(Debug)]
#[non_exhaustive]
pub enum Effect {
    /// Verify `username`/`password` against the account store.
    Authenticate { username: Username, password: SecretString },
}

/// The outcome of an [`Effect`], fed back via `SessionFsm::on_effect`.
#[derive(Debug)]
#[non_exhaustive]
pub enum EffectResult {
    /// Authentication succeeded; the account and its puppets (oldest first).
    Authenticated { account: Account, puppets: Vec<Puppet> },
    /// Authentication was refused (non-leaky for unknown-user / bad-password).
    LoginRejected(LoginError),
    /// A server-side fault performing the effect; the player may retry.
    BackendError,
}
