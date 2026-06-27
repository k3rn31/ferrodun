-- Initial schema for a single tenant's SQLite database (SPEC §2.5.1.4).
-- Each tenant owns a physically distinct file; there is no tenant column.

-- Entities are keyed by their durable `EntityKey` (§2.3.1.5). AUTOINCREMENT is
-- required, not cosmetic: the spec forbids reusing a key for the lifetime of
-- the database even after the entity is destroyed, and only AUTOINCREMENT stops
-- SQLite from recycling rowids.
CREATE TABLE entities (
    entity_key INTEGER PRIMARY KEY AUTOINCREMENT
);

-- Durable per-player identity (§3.15.1.1). `username` is unique within the
-- file, i.e. within the tenant.
CREATE TABLE accounts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    username TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'active'
);

-- Every table that references `entities` does so `ON DELETE CASCADE`: tearing
-- an entity down (§2.5.3.1) is its destruction, so deleting its `entities` row
-- must remove every dependent row in one step. Encoding the cascade in the
-- schema makes a dangling child row unrepresentable and means a new child table
-- declares its own teardown behaviour locally, rather than the destroy path
-- having to enumerate every referencing table.

-- In-world characters owned by an account (§3.15.1.4). A puppet is itself an
-- entity, so its primary key is an `EntityKey`.
CREATE TABLE puppets (
    entity_key INTEGER PRIMARY KEY REFERENCES entities (
        entity_key
    ) ON DELETE CASCADE,
    account_id INTEGER NOT NULL REFERENCES accounts (id),
    name TEXT NOT NULL
);

-- Where an entity is. One row per entity (PK) = at most one location.
-- `place_id` is a loose reference: the Place graph is in-memory (§2.2), there
-- is no places table in M1.
CREATE TABLE location (
    entity_key INTEGER PRIMARY KEY REFERENCES entities (
        entity_key
    ) ON DELETE CASCADE,
    place_id INTEGER NOT NULL
);

-- Containment. Making `item_key` the primary key encodes the invariant that an
-- item lives in at most one container at a time. Both keys cascade: destroying
-- an item removes its containment row; destroying a container removes the rows
-- for the items it held (the items themselves survive as entities).
CREATE TABLE inventory (
    item_key INTEGER PRIMARY KEY REFERENCES entities (
        entity_key
    ) ON DELETE CASCADE,
    container_key INTEGER NOT NULL REFERENCES entities (
        entity_key
    ) ON DELETE CASCADE
);
