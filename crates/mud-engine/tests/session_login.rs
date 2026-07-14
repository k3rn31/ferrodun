//! End-to-end §3.19.1: login of an existing (boot-hydrated) puppet through a
//! real `mud-db`-backed `LoginBackend`, and login → wrong password → retry.
//! Create-then-enter of a *new* puppet is deferred to M1-22 (live hydration);
//! it is unit-tested with a fake backend in the `mud-engine` `session` module.
#![allow(clippy::expect_used)] // integration-test crates are not compiled with cfg(test), so clippy.toml allow-expect-in-tests does not cover their helpers; expect() is permitted in tests per policy

use std::num::NonZeroU64;

use mud_account::{
    Account, AccountId, Credential, LoginError, Puppet, PuppetName, RegisterError, Username,
};
use mud_core::{EntityId, EntityKey, PlaceId, PlaceKey, TenantTag};
use mud_db::{Accounts, PersistentWorld, PlaceMap, TenantDb};
use mud_engine::{BackendError, LoginBackend, Routing, SessionResolver, SessionService};
use mud_i18n::Locale;
use mud_schema::SessionId;
use secrecy::{ExposeSecret, SecretString};
use tempfile::TempDir;

const HALL: u64 = 10;

fn tenant() -> TenantTag {
    TenantTag::new(1).expect("tenant in range")
}
fn hall() -> PlaceId {
    PlaceId::new(NonZeroU64::new(HALL).expect("non-zero place id"))
}
fn hall_slug() -> PlaceKey {
    PlaceKey::parse("hall").expect("valid slug")
}
fn places() -> PlaceMap {
    PlaceMap::from_pairs([(hall(), hall_slug())])
}
fn sid(n: u64) -> SessionId {
    SessionId::new(NonZeroU64::new(n).expect("non-zero session id"))
}

// --- a real mud-db-backed backend ---------------------------------------

struct DbBackend<'a> {
    accounts: Accounts<'a>,
    world: &'a PersistentWorld,
    start: PlaceKey,
}

impl LoginBackend for DbBackend<'_> {
    async fn authenticate(
        &self,
        username: &Username,
        password: &SecretString,
    ) -> Result<Result<Account, LoginError>, BackendError> {
        self.accounts
            .authenticate(username, password.expose_secret())
            .await
            .map_err(|_| BackendError)
    }

    async fn register(
        &self,
        username: &Username,
        password: &SecretString,
    ) -> Result<Result<Account, RegisterError>, BackendError> {
        let exposed = password.expose_secret().to_owned();
        let credential = tokio::task::spawn_blocking(move || Credential::hash(&exposed))
            .await
            .map_err(|_| BackendError)?
            .map_err(|_| BackendError)?;
        self.accounts
            .register(username.clone(), &credential)
            .await
            .map_err(|_| BackendError)
    }

    async fn puppets_of(&self, account: AccountId) -> Result<Vec<Puppet>, BackendError> {
        self.accounts
            .puppets_of(account)
            .await
            .map_err(|_| BackendError)
    }

    async fn create_puppet(
        &self,
        account: AccountId,
        name: &PuppetName,
    ) -> Result<Puppet, BackendError> {
        // Not driven by this test (create→enter is deferred to M1-22), but the
        // trait requires it; a faithful impl keeps the backend honest.
        self.accounts
            .create_puppet(account, name.clone(), &self.start)
            .await
            .map_err(|_| BackendError)
    }

    fn resolve_puppet(
        &self,
        key: EntityKey,
    ) -> impl std::future::Future<Output = Option<EntityId>> + Send {
        let result = self.world.entity_id(key);
        async move { result }
    }
}

/// Seeds an `alice`/`hunter2` account owning one puppet `arden` in the hall, in
/// a fresh committed DB at `dir`, and returns nothing — later phases reopen the
/// same path. Uses its own short-lived `TenantDb` handle that is dropped here.
async fn seed(dir: &TempDir) {
    let db = TenantDb::open(dir.path()).await.expect("open seed db");
    let accounts = Accounts::new(&db);
    let credential = Credential::hash("hunter2").expect("hash");
    let account = accounts
        .register(Username::parse("alice").expect("username"), &credential)
        .await
        .expect("no db fault")
        .expect("registration succeeds");
    accounts
        .create_puppet(
            account.id,
            PuppetName::parse("arden").expect("puppet name"),
            &hall_slug(),
        )
        .await
        .expect("create puppet");
}

#[tokio::test]
async fn login_of_an_existing_puppet_resolves_in_world() {
    let dir = TempDir::new().expect("tempdir");
    seed(&dir).await;

    // Boot: one handle owned by PersistentWorld (hydrates arden), one for Accounts.
    let world_db = TenantDb::open(dir.path()).await.expect("open world db");
    let world = PersistentWorld::load(world_db, tenant(), places())
        .await
        .expect("load world");
    let accounts_db = TenantDb::open(dir.path()).await.expect("open accounts db");
    let backend = DbBackend {
        accounts: Accounts::new(&accounts_db),
        world: &world,
        start: hall_slug(),
    };

    let mut svc = SessionService::new("WELCOME", Locale::EN);
    svc.connect(sid(1));
    for line in ["login alice", "hunter2", "play arden"] {
        let routing = svc.on_input(sid(1), line, &backend).await;
        assert!(
            matches!(routing, Routing::Login { close: false, .. }),
            "expected an open Login routing on {line:?}, got {routing:?}"
        );
    }
    // The session is now in-world; further input routes to the pipeline.
    assert!(matches!(
        svc.on_input(sid(1), "look", &backend).await,
        Routing::InWorld
    ));

    // The real resolver resolves the in-world session to arden at its persisted hall.
    let mut dispatcher = mud_engine::Dispatcher::new();
    let builtins = mud_engine::register(&mut dispatcher);
    let resolver = svc.resolver(&builtins);
    let resolved = resolver
        .resolve(sid(1), world.world())
        .expect("in-world session resolves");
    assert_eq!(
        resolved.caller.location(),
        hall(),
        "puppet resolves at its persisted room"
    );
}

#[tokio::test]
async fn a_wrong_password_then_retry_succeeds() {
    let dir = TempDir::new().expect("tempdir");
    seed(&dir).await;
    let world_db = TenantDb::open(dir.path()).await.expect("open world db");
    let world = PersistentWorld::load(world_db, tenant(), places())
        .await
        .expect("load world");
    let accounts_db = TenantDb::open(dir.path()).await.expect("open accounts db");
    let backend = DbBackend {
        accounts: Accounts::new(&accounts_db),
        world: &world,
        start: hall_slug(),
    };

    let mut svc = SessionService::new("W", Locale::EN);
    svc.connect(sid(1));
    let _ = svc.on_input(sid(1), "login alice", &backend).await;
    let routing = svc.on_input(sid(1), "wrong", &backend).await;
    assert!(
        matches!(routing, Routing::Login { close: false, .. }),
        "expected an open Login routing, got {routing:?}"
    );
    // INVARIANT: the assertion above already confirmed `routing` is an open `Routing::Login`.
    let Routing::Login { outputs, .. } = routing else {
        unreachable!()
    };
    let text = outputs
        .iter()
        .filter_map(|output| match output {
            mud_engine::LoginOutput::Text(text) => Some(text.text.as_str()),
            mud_engine::LoginOutput::Echo(_) => None,
        })
        .collect::<String>();
    assert!(
        text.contains("Login failed"),
        "expected non-leaky failure, got: {text}"
    );
    // Still pre-login: retry with the right password reaches the world.
    for line in ["login alice", "hunter2", "play arden"] {
        assert!(matches!(
            svc.on_input(sid(1), line, &backend).await,
            Routing::Login { close: false, .. }
        ));
    }
    assert!(matches!(
        svc.on_input(sid(1), "look", &backend).await,
        Routing::InWorld
    ));
}
