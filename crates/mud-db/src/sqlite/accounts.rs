//! Account and puppet persistence (§3.15.1) over a single tenant's database.
//!
//! [`Accounts`] borrows a [`TenantDb`] and persists the `mud-account` domain
//! types against the `accounts` and `puppets` tables. Two failure axes are kept
//! distinct: the outer [`DbError`] is a server/persistence fault, while the
//! inner [`RegisterError`]/[`LoginError`] is a normal, re-promptable outcome the
//! session FSM (M1-19) renders to the player. Password *hashing* lives in
//! `mud-account` and the caller supplies an already-hashed [`Credential`] to
//! `register`, so the hash KDF never runs on this layer; `authenticate` offloads
//! the (equally CPU-heavy) argon2 verification to a blocking thread for the same
//! reason, keeping both KDF paths off the async runtime.

use mud_account::{
    Account, AccountId, AccountState, Credential, LoginError, Puppet, PuppetName, RegisterError,
    Username,
};
use mud_core::PlaceKey;

use super::TenantDb;
use super::keys::{account_id_from_db, account_id_to_db, entity_key_from_db};
use crate::error::DbError;

/// Persistence for accounts and the puppets they own, scoped to one tenant.
pub struct Accounts<'a> {
    db: &'a TenantDb,
}

impl<'a> Accounts<'a> {
    /// Binds the repository to a tenant's database.
    #[must_use]
    pub fn new(db: &'a TenantDb) -> Self {
        Self { db }
    }

    /// Registers a new account with an already-hashed credential (open
    /// registration, §3.15.1.3). The new account starts [`AccountState::Active`].
    ///
    /// The caller hashes the password into a [`Credential`] first, so the KDF
    /// runs under the caller's control (e.g. on a blocking thread) and never on
    /// this layer's async path.
    ///
    /// # Errors
    ///
    /// `Ok(Err(RegisterError::UsernameTaken))` if the username already exists in
    /// this tenant. `Err(DbError)` only on a persistence fault.
    pub async fn register(
        &self,
        username: Username,
        credential: &Credential,
    ) -> Result<Result<Account, RegisterError>, DbError> {
        let username_str = username.as_str();
        let phc = credential.as_phc();
        let inserted = sqlx::query!(
            r#"INSERT INTO accounts (username, password_hash) VALUES (?, ?) RETURNING id AS "id!""#,
            username_str,
            phc
        )
        .fetch_one(self.db.pool())
        .await;

        match inserted {
            Ok(row) => {
                let id = account_id_from_db(row.id)?;
                Ok(Ok(Account {
                    id,
                    username,
                    state: AccountState::Active,
                }))
            }
            Err(err) if is_unique_violation(&err) => Ok(Err(RegisterError::UsernameTaken)),
            Err(err) => Err(err.into()),
        }
    }

    /// Authenticates `username`/`password`, returning the account on success.
    ///
    /// The password is verified *before* the account state is consulted, so a
    /// suspended/banned outcome (§3.15.1.5) is revealed only to someone holding
    /// the correct credentials.
    ///
    /// # Errors
    ///
    /// `Ok(Err(LoginError))` for an unknown user, wrong password, or a
    /// suspended/banned/deleted account. `Err(DbError)` only on a persistence
    /// fault or a corrupt persisted `state` token.
    pub async fn authenticate(
        &self,
        username: &Username,
        password: &str,
    ) -> Result<Result<Account, LoginError>, DbError> {
        let username_str = username.as_str();
        let row = sqlx::query!(
            r#"SELECT id AS "id!", password_hash, state FROM accounts WHERE username = ?"#,
            username_str
        )
        .fetch_optional(self.db.pool())
        .await?;

        let Some(row) = row else {
            return Ok(Err(LoginError::UnknownUser));
        };

        // argon2id verification is CPU-heavy (tens of ms by design); run it on a
        // blocking thread so a burst of logins cannot starve the async runtime.
        let stored = row.password_hash;
        let attempt = password.to_owned();
        let verified =
            tokio::task::spawn_blocking(move || Credential::verify_phc(&stored, &attempt)).await?;
        if !verified {
            return Ok(Err(LoginError::BadPassword));
        }

        let state = parse_state(&row.state)?;
        if let Some(rejection) = state.login_rejection() {
            return Ok(Err(rejection));
        }

        let id = account_id_from_db(row.id)?;
        Ok(Ok(Account {
            id,
            username: username.clone(),
            state,
        }))
    }

    /// Creates a puppet owned by `account`, starting in `start`, and returns it.
    ///
    /// A puppet is an entity, so this mints an `entities` row (its durable
    /// [`EntityKey`](mud_core::EntityKey)) and records ownership and starting
    /// location in one transaction — the location persists against the same key,
    /// so the puppet's whereabouts survive a restart.
    ///
    /// # Errors
    ///
    /// `DbError` on any persistence fault; the transaction rolls back so no
    /// partial puppet is left behind.
    pub async fn create_puppet(
        &self,
        account: AccountId,
        name: PuppetName,
        start: &PlaceKey,
    ) -> Result<Puppet, DbError> {
        let account_db = account_id_to_db(account)?;
        let name_str = name.as_str();
        let slug = start.as_str();

        let mut tx = self.db.pool().begin().await?;
        let row = sqlx::query!(
            r#"INSERT INTO entities DEFAULT VALUES RETURNING entity_key AS "entity_key!""#
        )
        .fetch_one(&mut *tx)
        .await?;
        sqlx::query!(
            "INSERT INTO puppets (entity_key, account_id, name) VALUES (?, ?, ?)",
            row.entity_key,
            account_db,
            name_str
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query!(
            "INSERT INTO location (entity_key, place_key) VALUES (?, ?)",
            row.entity_key,
            slug
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;

        let key = entity_key_from_db(row.entity_key)?;
        Ok(Puppet::new(key, name))
    }

    /// Lists the puppets owned by `account`, oldest first.
    ///
    /// # Errors
    ///
    /// `DbError` on a persistence fault or a persisted name that fails domain
    /// validation (corruption).
    pub async fn puppets_of(&self, account: AccountId) -> Result<Vec<Puppet>, DbError> {
        let account_db = account_id_to_db(account)?;
        let rows = sqlx::query!(
            r#"SELECT entity_key AS "entity_key!", name FROM puppets
               WHERE account_id = ? ORDER BY entity_key"#,
            account_db
        )
        .fetch_all(self.db.pool())
        .await?;

        rows.into_iter()
            .map(|row| {
                let key = entity_key_from_db(row.entity_key)?;
                let name = parse_puppet_name(&row.name)?;
                Ok(Puppet::new(key, name))
            })
            .collect()
    }
}

/// Parses a persisted `state` token, mapping an unknown token to corruption.
fn parse_state(raw: &str) -> Result<AccountState, DbError> {
    raw.parse()
        .map_err(|_| DbError::CorruptValue(format!("account state {raw:?}")))
}

/// Parses a persisted puppet name, mapping an invalid one to corruption.
fn parse_puppet_name(raw: &str) -> Result<PuppetName, DbError> {
    PuppetName::parse(raw).map_err(|_| DbError::CorruptValue(format!("puppet name {raw:?}")))
}

/// Whether a query error is a UNIQUE-constraint violation (a taken username),
/// as opposed to a genuine database fault.
fn is_unique_violation(err: &sqlx::Error) -> bool {
    err.as_database_error()
        .is_some_and(sqlx::error::DatabaseError::is_unique_violation)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn open(dir: &TempDir) -> TenantDb {
        TenantDb::open(dir.path()).await.expect("open tenant db")
    }

    fn user(name: &str) -> Username {
        Username::parse(name).expect("test username is valid")
    }

    fn credential() -> Credential {
        Credential::hash("correct-horse").expect("hashing succeeds")
    }

    /// Forces an account's `state` column, standing in for a moderation action
    /// the engine has no command for in M1 (suspend/ban land in M7, §3.15.5).
    async fn force_state(db: &TenantDb, username: &str, state: &str) {
        sqlx::query("UPDATE accounts SET state = ? WHERE username = ?")
            .bind(state)
            .bind(username)
            .execute(db.pool())
            .await
            .expect("force state");
    }

    #[tokio::test]
    async fn a_suspended_account_is_rejected_with_its_own_reason() {
        let dir = TempDir::new().expect("tempdir");
        let db = open(&dir).await;
        let accounts = Accounts::new(&db);
        let name = user("aldous");
        accounts
            .register(name.clone(), &credential())
            .await
            .expect("no db fault")
            .expect("registration succeeds");

        force_state(&db, "aldous", "suspended").await;

        let outcome = accounts
            .authenticate(&name, "correct-horse")
            .await
            .expect("no db fault");
        assert_eq!(
            outcome.expect_err("a suspended account must be refused"),
            LoginError::Suspended
        );
    }

    #[tokio::test]
    async fn a_banned_account_is_rejected_with_its_own_reason() {
        let dir = TempDir::new().expect("tempdir");
        let db = open(&dir).await;
        let accounts = Accounts::new(&db);
        let name = user("mallory");
        accounts
            .register(name.clone(), &credential())
            .await
            .expect("no db fault")
            .expect("registration succeeds");

        force_state(&db, "mallory", "banned").await;

        let outcome = accounts
            .authenticate(&name, "correct-horse")
            .await
            .expect("no db fault");
        assert_eq!(
            outcome.expect_err("a banned account must be refused"),
            LoginError::Banned
        );
    }

    #[tokio::test]
    async fn the_right_password_still_fails_for_a_barred_account() {
        // The state check runs only after the password verifies, so a barred
        // account with the wrong password reads as BadPassword, not Banned —
        // the state is never revealed to someone without the credentials.
        let dir = TempDir::new().expect("tempdir");
        let db = open(&dir).await;
        let accounts = Accounts::new(&db);
        let name = user("mallory");
        accounts
            .register(name.clone(), &credential())
            .await
            .expect("no db fault")
            .expect("registration succeeds");
        force_state(&db, "mallory", "banned").await;

        let outcome = accounts
            .authenticate(&name, "wrong")
            .await
            .expect("no db fault");
        assert_eq!(
            outcome.expect_err("wrong password is refused"),
            LoginError::BadPassword
        );
    }

    #[tokio::test]
    async fn a_corrupt_state_token_surfaces_as_a_db_error() {
        let dir = TempDir::new().expect("tempdir");
        let db = open(&dir).await;
        let accounts = Accounts::new(&db);
        let name = user("aldous");
        accounts
            .register(name.clone(), &credential())
            .await
            .expect("no db fault")
            .expect("registration succeeds");
        force_state(&db, "aldous", "frozen").await;

        let err = accounts
            .authenticate(&name, "correct-horse")
            .await
            .expect_err("an unknown state token is corruption");
        assert!(matches!(err, DbError::CorruptValue(_)), "got {err:?}");
    }

    #[tokio::test]
    async fn deleting_an_account_that_owns_a_puppet_is_refused() {
        let dir = TempDir::new().expect("tempdir");
        let db = open(&dir).await;
        let accounts = Accounts::new(&db);
        let name = user("aldous");
        let account = accounts
            .register(name.clone(), &credential())
            .await
            .expect("no db fault")
            .expect("registration succeeds");
        let start = PlaceKey::parse("town_square").expect("valid slug");
        accounts
            .create_puppet(
                account.id,
                PuppetName::parse("hero").expect("valid name"),
                &start,
            )
            .await
            .expect("puppet created");

        // Foreign keys are enabled on the pool, so the RESTRICT FK rejects the
        // delete while a puppet still references the account.
        let deleted = sqlx::query("DELETE FROM accounts WHERE username = ?")
            .bind("aldous")
            .execute(db.pool())
            .await;

        assert!(
            deleted.is_err(),
            "deleting an account with puppets must be refused by the FK"
        );
    }
}
