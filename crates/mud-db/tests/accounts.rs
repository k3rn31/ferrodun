//! The M1-18 acceptance path (§3.15.1): register → login → wrong-password
//! reject → restart → login again. Plus puppet ownership: a created puppet is
//! listed, located, and survives a restart with its location intact.
#![allow(clippy::expect_used)] // integration-test crates are not compiled with cfg(test), so clippy.toml allow-expect-in-tests does not cover their helpers; expect() is permitted in tests per policy

use std::num::NonZeroU64;

use mud_account::{Credential, LoginError, PuppetName, RegisterError, Username};
use mud_core::{PlaceId, PlaceKey, TenantTag};
use mud_db::{Accounts, PersistentWorld, PlaceMap, TenantDb};
use tempfile::TempDir;

const HALL: u64 = 10;

fn tenant() -> TenantTag {
    TenantTag::new(1).expect("test tenant tag must be in range")
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

fn username(name: &str) -> Username {
    Username::parse(name).expect("test username is valid")
}

async fn open(dir: &TempDir) -> TenantDb {
    TenantDb::open(dir.path()).await.expect("open tenant db")
}

#[tokio::test]
async fn register_then_login_survives_a_restart_and_rejects_a_wrong_password() {
    let dir = TempDir::new().expect("tempdir");
    let name = username("aldous");
    let credential = Credential::hash("correct-horse").expect("hashing succeeds");

    // Register, then log in with the right and wrong passwords.
    {
        let db = open(&dir).await;
        let accounts = Accounts::new(&db);
        let account = accounts
            .register(name.clone(), &credential)
            .await
            .expect("no db fault")
            .expect("registration succeeds");
        assert_eq!(account.username, name);

        accounts
            .authenticate(&name, "correct-horse")
            .await
            .expect("no db fault")
            .expect("the right password logs in");

        let rejected = accounts
            .authenticate(&name, "guess")
            .await
            .expect("no db fault");
        assert_eq!(
            rejected.expect_err("a wrong password is refused"),
            LoginError::BadPassword
        );
    }

    // Restart: a brand-new process opening the same file must still authenticate.
    {
        let db = open(&dir).await;
        let accounts = Accounts::new(&db);
        accounts
            .authenticate(&name, "correct-horse")
            .await
            .expect("no db fault")
            .expect("login still works after a clean restart");
    }
}

#[tokio::test]
async fn an_unknown_user_is_refused() {
    let dir = TempDir::new().expect("tempdir");
    let db = open(&dir).await;
    let accounts = Accounts::new(&db);

    let outcome = accounts
        .authenticate(&username("nobody"), "whatever")
        .await
        .expect("no db fault");
    assert_eq!(
        outcome.expect_err("an unknown user cannot log in"),
        LoginError::UnknownUser
    );
}

#[tokio::test]
async fn registering_a_taken_username_is_refused() {
    let dir = TempDir::new().expect("tempdir");
    let db = open(&dir).await;
    let accounts = Accounts::new(&db);
    let name = username("aldous");
    let credential = Credential::hash("pw").expect("hashing succeeds");

    accounts
        .register(name.clone(), &credential)
        .await
        .expect("no db fault")
        .expect("first registration succeeds");

    let duplicate = accounts
        .register(name.clone(), &credential)
        .await
        .expect("no db fault");
    assert_eq!(
        duplicate.expect_err("the second registration is refused"),
        RegisterError::UsernameTaken
    );
}

#[tokio::test]
async fn a_puppet_is_owned_listed_and_located_across_a_restart() {
    let dir = TempDir::new().expect("tempdir");
    let name = username("aldous");
    let puppet_name = PuppetName::parse("Gandalf").expect("valid puppet name");
    let credential = Credential::hash("pw").expect("hashing succeeds");

    let puppet_key = {
        let db = open(&dir).await;
        let accounts = Accounts::new(&db);
        let account = accounts
            .register(name.clone(), &credential)
            .await
            .expect("no db fault")
            .expect("registration succeeds");

        let puppet = accounts
            .create_puppet(account.id, puppet_name.clone(), &hall_slug())
            .await
            .expect("create puppet");

        let owned = accounts.puppets_of(account.id).await.expect("list puppets");
        assert_eq!(owned, vec![puppet.clone()], "the account owns its puppet");
        puppet.key
    };

    // Restart: the puppet is still owned, and its location reloads into the world.
    let db = open(&dir).await;
    let accounts = Accounts::new(&db);
    let account = accounts
        .authenticate(&name, "pw")
        .await
        .expect("no db fault")
        .expect("login after restart");
    let owned = accounts.puppets_of(account.id).await.expect("list puppets");
    assert_eq!(owned.len(), 1, "the puppet survived the restart");
    let only = owned.first().expect("exactly one puppet after restart");
    assert_eq!(only.key, puppet_key, "the puppet kept its durable key");

    let world = PersistentWorld::load(db, tenant(), places())
        .await
        .expect("boot load");
    let id = world
        .entity_id(puppet_key)
        .expect("the puppet's key reloads to a live handle");
    assert_eq!(
        world.world().location_of(id),
        Some(hall()),
        "the puppet reloads at its starting room"
    );
}
