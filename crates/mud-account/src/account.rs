//! Account identity, lifecycle state, and login outcomes (§3.15.1).

use std::fmt;
use std::num::NonZeroU64;
use std::str::FromStr;

use crate::name::Username;

/// The durable identity of an account: its `accounts.id` rowid, typed.
///
/// 1-based (`NonZeroU64`), so an unassigned reference is `Option::None` for free.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[must_use]
pub struct AccountId(NonZeroU64);

impl AccountId {
    /// Wraps a row id.
    pub const fn new(value: NonZeroU64) -> Self {
        Self(value)
    }

    /// The underlying row id.
    pub const fn get(self) -> NonZeroU64 {
        self.0
    }
}

impl fmt::Display for AccountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// The lifecycle state of an account (§3.15.1.5).
///
/// `suspended` (temporary) and `banned` (permanent) reject login; `deleted`
/// (§3.17) behaves as if the account does not exist, so login surfaces the same
/// non-leaky outcome as an unknown user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountState {
    /// Normal, may log in.
    Active,
    /// Temporarily barred by an admin.
    Suspended,
    /// Permanently barred.
    Banned,
    /// Soft-deleted; treated as nonexistent at login.
    Deleted,
}

impl AccountState {
    /// The login outcome this state forces, or `None` if login may proceed.
    ///
    /// `Deleted` maps to [`LoginError::UnknownUser`] so a soft-deleted account is
    /// indistinguishable from one that never existed (non-leaky, §3.15.1.5).
    #[must_use]
    pub fn login_rejection(self) -> Option<LoginError> {
        match self {
            Self::Active => None,
            Self::Suspended => Some(LoginError::Suspended),
            Self::Banned => Some(LoginError::Banned),
            Self::Deleted => Some(LoginError::UnknownUser),
        }
    }

    /// The canonical lowercase token stored in the `state` column.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Suspended => "suspended",
            Self::Banned => "banned",
            Self::Deleted => "deleted",
        }
    }
}

impl fmt::Display for AccountState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A `state` column value names no known [`AccountState`] — DB corruption or a
/// schema written by a newer engine. Surfaced rather than silently coerced.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown account state: {0:?}")]
pub struct UnknownAccountState(pub String);

impl FromStr for AccountState {
    type Err = UnknownAccountState;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        match raw {
            "active" => Ok(Self::Active),
            "suspended" => Ok(Self::Suspended),
            "banned" => Ok(Self::Banned),
            "deleted" => Ok(Self::Deleted),
            other => Err(UnknownAccountState(other.to_owned())),
        }
    }
}

/// Why a login was refused (§3.15.1.5).
///
/// A pure-domain outcome, distinct from any persistence failure: a caller can
/// tell "wrong credentials / barred account" (re-prompt) from a database error
/// (server fault). The caller is responsible for rendering a non-leaky message
/// — in particular `UnknownUser` and `BadPassword` SHOULD read identically to a
/// player so neither confirms a username exists.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LoginError {
    /// No account with that username.
    #[error("no such account")]
    UnknownUser,
    /// The password did not match.
    #[error("incorrect password")]
    BadPassword,
    /// The account is suspended.
    #[error("account suspended")]
    Suspended,
    /// The account is banned.
    #[error("account banned")]
    Banned,
}

/// Why a registration was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RegisterError {
    /// An account with that username already exists in this tenant.
    #[error("username already taken")]
    UsernameTaken,
}

/// A resolved account: enough to identify the player and gate login.
///
/// Deliberately does not carry the [`Credential`](crate::Credential): it is the
/// value handed back *after* authentication, where the secret is no longer
/// relevant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    /// Durable identity.
    pub id: AccountId,
    /// Login name, unique within the tenant.
    pub username: Username,
    /// Lifecycle state.
    pub state: AccountState,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_permits_login() {
        assert_eq!(AccountState::Active.login_rejection(), None);
    }

    #[test]
    fn suspended_and_banned_reject_login_with_their_own_reason() {
        assert_eq!(
            AccountState::Suspended.login_rejection(),
            Some(LoginError::Suspended)
        );
        assert_eq!(
            AccountState::Banned.login_rejection(),
            Some(LoginError::Banned)
        );
    }

    #[test]
    fn deleted_is_indistinguishable_from_unknown() {
        assert_eq!(
            AccountState::Deleted.login_rejection(),
            Some(LoginError::UnknownUser),
            "a soft-deleted account must not reveal it ever existed"
        );
    }

    #[test]
    fn state_round_trips_through_its_column_token() {
        for state in [
            AccountState::Active,
            AccountState::Suspended,
            AccountState::Banned,
            AccountState::Deleted,
        ] {
            let token = state.as_str();
            assert_eq!(token.parse::<AccountState>(), Ok(state));
            assert_eq!(state.to_string(), token);
        }
    }

    #[test]
    fn an_unknown_state_token_is_rejected() {
        assert_eq!(
            "frozen".parse::<AccountState>(),
            Err(UnknownAccountState("frozen".to_owned()))
        );
    }
}
