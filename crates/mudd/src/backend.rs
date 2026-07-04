//! The [`LoginBackend`] over the tenant database and live world (design §7).

use std::sync::Arc;

use mud_account::{
    Account, AccountId, Credential, LoginError, Puppet, PuppetName, RegisterError, Username,
};
use mud_core::{EntityId, EntityKey, PlaceKey};
use mud_db::{Accounts, PersistentWorld, TenantDb};
use mud_engine::{BackendError, LoginBackend};
use secrecy::{ExposeSecret, SecretString};
use tokio::sync::Mutex;

/// The `LoginBackend` over the tenant database and live world (design §7).
///
/// Owns a cloneable `TenantDb` handle rather than borrowing one, so it can be
/// constructed once at boot and shared across sessions without lifetime ties
/// to the world it hydrates puppets into.
#[allow(dead_code)] // LINT: constructed by the tenant boot loop in Task 10 (mudd boot); no other consumer exists yet
pub struct DbBackend {
    db: TenantDb,
    world: Arc<Mutex<PersistentWorld>>,
    start_room: PlaceKey,
}

impl DbBackend {
    /// A backend over `db`, hydrating new puppets into `world` at `start_room`.
    #[allow(dead_code)] // LINT: constructed by the tenant boot loop in Task 10 (mudd boot); no other consumer exists yet
    pub fn new(db: TenantDb, world: Arc<Mutex<PersistentWorld>>, start_room: PlaceKey) -> Self {
        Self {
            db,
            world,
            start_room,
        }
    }
}

impl LoginBackend for DbBackend {
    async fn authenticate(
        &self,
        username: &Username,
        password: &SecretString,
    ) -> Result<Result<Account, LoginError>, BackendError> {
        Accounts::new(&self.db)
            .authenticate(username, password.expose_secret())
            .await
            .map_err(|error| {
                tracing::error!(%error, "authenticate failed");
                BackendError
            })
    }

    async fn register(
        &self,
        username: &Username,
        password: &SecretString,
    ) -> Result<Result<Account, RegisterError>, BackendError> {
        let secret = password.expose_secret().to_owned();
        // argon2id hashing is tens of ms by design — off the async path.
        let credential = tokio::task::spawn_blocking(move || Credential::hash(&secret))
            .await
            .map_err(|error| {
                tracing::error!(%error, "hash task failed");
                BackendError
            })?
            .map_err(|error| {
                tracing::error!(%error, "hashing failed");
                BackendError
            })?;
        Accounts::new(&self.db)
            .register(username.clone(), &credential)
            .await
            .map_err(|error| {
                tracing::error!(%error, "register failed");
                BackendError
            })
    }

    async fn puppets_of(&self, account: AccountId) -> Result<Vec<Puppet>, BackendError> {
        Accounts::new(&self.db)
            .puppets_of(account)
            .await
            .map_err(|error| {
                tracing::error!(%error, "puppets_of failed");
                BackendError
            })
    }

    async fn create_puppet(
        &self,
        account: AccountId,
        name: &PuppetName,
    ) -> Result<Puppet, BackendError> {
        let puppet = Accounts::new(&self.db)
            .create_puppet(account, name.clone(), &self.start_room)
            .await
            .map_err(|error| {
                tracing::error!(%error, "create_puppet failed");
                BackendError
            })?;
        // Residency before the FSM's Enter resolves it (design §7).
        let _entity_id = self
            .world
            .lock()
            .await
            .hydrate(puppet.key)
            .await
            .map_err(|error| {
                tracing::error!(%error, "hydrate failed");
                BackendError
            })?;
        Ok(puppet)
    }

    async fn resolve_puppet(&self, key: EntityKey) -> Option<EntityId> {
        self.world.lock().await.entity_id(key)
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;

    use mud_core::{PlaceId, TenantTag};
    use mud_db::PlaceMap;
    use secrecy::SecretString;
    use tempfile::TempDir;

    use super::*;

    fn tenant() -> TenantTag {
        TenantTag::new(1).expect("tenant in range")
    }

    fn town_square() -> PlaceId {
        PlaceId::new(NonZeroU64::new(1).expect("non-zero place id"))
    }

    fn town_square_key() -> PlaceKey {
        PlaceKey::parse("town_square").expect("valid slug")
    }

    #[tokio::test]
    async fn register_create_puppet_and_resolve_round_trip() {
        let dir = TempDir::new().expect("tempdir");
        let db = TenantDb::open(dir.path()).await.expect("open db");
        let places = PlaceMap::from_pairs([(town_square(), town_square_key())]);
        let world = PersistentWorld::load(db.clone(), tenant(), places)
            .await
            .expect("load world");
        let world = Arc::new(Mutex::new(world));
        let backend = DbBackend::new(db, world, town_square_key());

        let username = Username::parse("alice").expect("username");
        let password = SecretString::from("hunter2!".to_owned());
        let account = backend
            .register(&username, &password)
            .await
            .expect("no backend fault")
            .expect("registration succeeds");

        let name = PuppetName::parse("Hero").expect("puppet name");
        let puppet = backend
            .create_puppet(account.id, &name)
            .await
            .expect("create_puppet succeeds");

        let resolved = backend.resolve_puppet(puppet.key).await;
        assert!(
            resolved.is_some(),
            "hydrate during create_puppet should have made the puppet resident"
        );

        let ok = backend
            .authenticate(&username, &password)
            .await
            .expect("no backend fault");
        assert!(ok.is_ok(), "correct password should authenticate");

        let wrong = SecretString::from("wrong".to_owned());
        let bad = backend
            .authenticate(&username, &wrong)
            .await
            .expect("no backend fault");
        assert!(bad.is_err(), "wrong password should be rejected");
    }
}
