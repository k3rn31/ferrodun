//! SQLite persistence backend.
//!
//! One [`TenantDb`] owns a connection pool over a single tenant's database
//! file. Physical per-file isolation (SPEC §2.5.1.4) is structural: a
//! `TenantDb` is constructed against one directory and never sees another
//! tenant's file, so cross-tenant queries are impossible by construction.

mod accounts;
mod keys;
mod persistent_world;
mod place_map;

pub use accounts::Accounts;
pub use persistent_world::PersistentWorld;
pub use place_map::PlaceMap;

use std::num::NonZeroU64;
use std::path::Path;

use mud_schema::WorldId;
use sqlx::SqlitePool;
use sqlx::migrate::Migrator;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

use crate::error::DbError;

/// SQLite migrations for a single tenant's database, embedded at compile time.
static MIGRATOR: Migrator = sqlx::migrate!("./migrations/sqlite");

/// The file name of a tenant's database within its data directory.
const DATABASE_FILE: &str = "world.db";

/// A connection pool over one tenant's SQLite database file.
#[derive(Clone)]
pub struct TenantDb {
    pool: SqlitePool,
}

impl TenantDb {
    /// Opens (creating if absent) the tenant database under `data_dir` and runs
    /// all pending migrations.
    ///
    /// The database lives at `<data_dir>/world.db`. Foreign-key enforcement is
    /// enabled on every connection — SQLite ignores foreign-key constraints
    /// otherwise. `data_dir` is the tenant's resolved data directory; mapping a
    /// tenant to that directory is the caller's concern (routing/config).
    ///
    /// # Errors
    ///
    /// Returns [`DbError`] if the file cannot be opened or a migration fails.
    pub async fn open(data_dir: &Path) -> Result<Self, DbError> {
        let options = SqliteConnectOptions::new()
            .filename(data_dir.join(DATABASE_FILE))
            .create_if_missing(true)
            .foreign_keys(true);

        let pool = SqlitePoolOptions::new().connect_with(options).await?;
        MIGRATOR.run(&pool).await?;

        Ok(Self { pool })
    }

    /// Returns the underlying connection pool for issuing queries.
    ///
    /// Crate-internal: the `sqlx` pool is an implementation detail and must not
    /// cross the public API boundary.
    #[must_use]
    pub(crate) fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// This tenant's stable World identity (§2.1.3.2), generated at first call
    /// and persisted; every later call — including after a restart — returns
    /// the same value.
    ///
    /// # Errors
    ///
    /// Returns [`DbError`] on a query failure or a corrupt persisted id.
    pub async fn world_id(&self) -> Result<WorldId, DbError> {
        // Random positive i64; NonZeroU64 rules out 0, the CHECK-ed single
        // row rules out a second value ever being written.
        let fresh = i64::from(rand::random::<u32>()).saturating_add(1);
        sqlx::query!(
            "INSERT INTO server (id, world_id) VALUES (1, ?) ON CONFLICT(id) DO NOTHING",
            fresh
        )
        .execute(&self.pool)
        .await?;
        let row = sqlx::query!(r#"SELECT world_id AS "world_id!" FROM server WHERE id = 1"#)
            .fetch_one(&self.pool)
            .await?;
        let raw = u64::try_from(row.world_id).map_err(|_| DbError::InvalidId(row.world_id))?;
        let value = NonZeroU64::new(raw).ok_or(DbError::InvalidId(row.world_id))?;
        Ok(WorldId::new(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn open_in(dir: &TempDir) -> TenantDb {
        TenantDb::open(dir.path())
            .await
            .expect("open tenant database")
    }

    async fn table_exists(db: &TenantDb, table: &str) -> bool {
        let found = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?",
        )
        .bind(table)
        .fetch_one(db.pool())
        .await
        .expect("query sqlite_master");
        found == 1
    }

    async fn accounts_count(db: &TenantDb) -> i64 {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM accounts")
            .fetch_one(db.pool())
            .await
            .expect("count accounts")
    }

    async fn insert_entity(db: &TenantDb) -> i64 {
        sqlx::query("INSERT INTO entities DEFAULT VALUES")
            .execute(db.pool())
            .await
            .expect("insert entity")
            .last_insert_rowid()
    }

    #[tokio::test]
    async fn migration_creates_all_tables() {
        let dir = TempDir::new().expect("temp dir");
        let db = open_in(&dir).await;

        for table in [
            "entities",
            "accounts",
            "puppets",
            "location",
            "inventory",
            "server",
        ] {
            assert!(
                table_exists(&db, table).await,
                "migration should create table {table}"
            );
        }
    }

    // The §2.5.1.4 defining test: two tenants are physically separate files, so
    // a write to one is invisible to the other.
    #[tokio::test]
    async fn tenant_files_are_isolated() {
        let dir_a = TempDir::new().expect("temp dir a");
        let dir_b = TempDir::new().expect("temp dir b");
        let tenant_a = open_in(&dir_a).await;
        let tenant_b = open_in(&dir_b).await;

        sqlx::query("INSERT INTO accounts (username, password_hash) VALUES (?, ?)")
            .bind("alice")
            .bind("hash")
            .execute(tenant_a.pool())
            .await
            .expect("insert account into tenant A");

        assert_eq!(accounts_count(&tenant_a).await, 1);
        assert_eq!(accounts_count(&tenant_b).await, 0);
    }

    // §2.3.1.5: an EntityKey must never be reused, even after the entity is
    // destroyed. AUTOINCREMENT guarantees the next key exceeds every key ever
    // issued, not merely the live maximum.
    #[tokio::test]
    async fn entity_keys_are_never_reused() {
        let dir = TempDir::new().expect("temp dir");
        let db = open_in(&dir).await;

        let first = insert_entity(&db).await;
        let second = insert_entity(&db).await;

        sqlx::query("DELETE FROM entities WHERE entity_key = ?")
            .bind(second)
            .execute(db.pool())
            .await
            .expect("delete entity");

        let third = insert_entity(&db).await;

        assert!(first < second);
        assert!(
            third > second,
            "deleted key {second} must not be reused (got {third})"
        );
    }

    // The inventory schema makes "an item is in at most one container" an
    // unrepresentable-illegal-state: item_key is the primary key.
    #[tokio::test]
    async fn item_lives_in_at_most_one_container() {
        let dir = TempDir::new().expect("temp dir");
        let db = open_in(&dir).await;

        let item = insert_entity(&db).await;
        let container_a = insert_entity(&db).await;
        let container_b = insert_entity(&db).await;

        sqlx::query("INSERT INTO inventory (item_key, container_key) VALUES (?, ?)")
            .bind(item)
            .bind(container_a)
            .execute(db.pool())
            .await
            .expect("place item in first container");

        let second = sqlx::query("INSERT INTO inventory (item_key, container_key) VALUES (?, ?)")
            .bind(item)
            .bind(container_b)
            .execute(db.pool())
            .await;

        assert!(second.is_err(), "item must not occupy two containers");
    }

    // Accounts written before a restart are still present after reopening the
    // file — the accounts table is as durable as the entity state.
    #[tokio::test]
    async fn accounts_survive_restart() {
        let dir = TempDir::new().expect("temp dir");

        {
            let db = open_in(&dir).await;
            sqlx::query("INSERT INTO accounts (username, password_hash) VALUES (?, ?)")
                .bind("alice")
                .bind("hash")
                .execute(db.pool())
                .await
                .expect("insert account");
        } // pool dropped — simulates a process restart.

        let db = open_in(&dir).await;
        assert_eq!(accounts_count(&db).await, 1, "the account persisted");
    }

    #[tokio::test]
    async fn world_id_is_created_once_and_stable_across_reopens() {
        let dir = TempDir::new().expect("temp dir");
        let first = {
            let db = open_in(&dir).await;
            db.world_id().await.expect("first world_id")
        }; // pool dropped — simulates a restart.
        let db = open_in(&dir).await;
        let second = db.world_id().await.expect("second world_id");
        assert_eq!(first, second, "world_id must survive a restart");
    }

    #[tokio::test]
    async fn two_tenants_get_distinct_world_ids() {
        let dir_a = TempDir::new().expect("temp dir a");
        let dir_b = TempDir::new().expect("temp dir b");
        let a = open_in(&dir_a).await.world_id().await.expect("world_id a");
        let b = open_in(&dir_b).await.world_id().await.expect("world_id b");
        assert_ne!(a, b, "random world ids must not collide across tenants");
    }
}
