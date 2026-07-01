//! Effects the FSM emits for the driver to perform, and the results fed back.

use mud_account::{Account, AccountId, LoginError, Puppet, PuppetName, RegisterError, Username};
use mud_core::EntityKey;
use secrecy::SecretString;

/// I/O the driver must perform on the FSM's behalf. Password fields are
/// [`SecretString`]: zeroized on drop and redacted in `Debug`, exposed only at
/// the argon2 boundary inside the driver's backend.
#[derive(Debug)]
#[non_exhaustive]
pub enum Effect {
    /// Verify `username`/`password` against the account store.
    Authenticate { username: Username, password: SecretString },
    /// Create a new account with `username` and `password`.
    Register { username: Username, password: SecretString },
    /// Create a new puppet named `name` for `account`.
    CreatePuppet { account: AccountId, name: PuppetName },
    /// Bind the session to `puppet` (already resident in the world).
    Enter { account: AccountId, puppet: EntityKey },
}

/// The outcome of an [`Effect`], fed back via `SessionFsm::on_effect`.
#[derive(Debug)]
#[non_exhaustive]
pub enum EffectResult {
    /// Authentication succeeded; the account and its puppets (oldest first).
    Authenticated { account: Account, puppets: Vec<Puppet> },
    /// Authentication was refused (non-leaky for unknown-user / bad-password).
    LoginRejected(LoginError),
    /// Registration succeeded; the freshly created account.
    Registered { account: Account },
    /// Registration was refused (username already taken).
    RegisterRejected(RegisterError),
    /// A server-side fault performing the effect; the player may retry.
    BackendError,
    /// A puppet was created for the account.
    PuppetCreated(Puppet),
    /// The session was bound to its puppet; login is complete.
    Entered,
}
